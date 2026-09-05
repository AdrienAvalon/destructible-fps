use super::*;
use crate::{
    BuildCommand, ClientReplica, ExplosionCommand, FixedMicrometers3, character::PlayerBuildContext,
};
use crate::{Material, Voxel};
use std::time::{Duration, Instant};

fn materials() -> StructuralMaterials {
    StructuralMaterials::new(
        [ElasticMaterial {
            young_modulus_pa: 1e9,
            poisson_ratio: 0.25,
        }; 7],
    )
    .unwrap()
}

fn column() -> AuthoritativeServer {
    let mut world = World::default();
    world.fill_box(
        IVec3::new(4, 0, 4),
        IVec3::new(4, 6, 4),
        Voxel::new(Material::Wood),
    );
    AuthoritativeServer::new(world).with_structural_materials(materials())
}

fn request(server: &AuthoritativeServer) -> StructuralRequest {
    server
        .structural_context
        .capture(
            server.world(),
            server.structural_anchors(),
            IVec3::new(4, 1, 4),
        )
        .unwrap()
}

fn calculate(server: &AuthoritativeServer, seed: IVec3) -> CompletedStructuralJob {
    let request = server
        .structural_context
        .capture(server.world(), server.structural_anchors(), seed)
        .unwrap();
    run_request(request, &AtomicBool::new(false))
}

fn wait(scheduler: &mut StructuralScheduler) -> CompletedStructuralJob {
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        if let Some(result) = scheduler.poll().unwrap() {
            return result;
        }
        assert!(Instant::now() < deadline, "bounded worker did not respond");
        thread::yield_now();
    }
}

fn small_blast(command_id: u64, center: IVec3) -> ExplosionCommand {
    ExplosionCommand {
        command_id,
        center,
        radius_voxels: 1,
        peak_energy: 100,
    }
}

fn builder() -> PlayerBuildContext {
    PlayerBuildContext {
        eye_position_um: FixedMicrometers3 {
            x: 8_500_000,
            y: 2_650_000,
            z: 4_500_000,
        },
        bounds_minimum_um: FixedMicrometers3 {
            x: 8_200_000,
            y: 1_000_000,
            z: 4_200_000,
        },
        bounds_maximum_um: FixedMicrometers3 {
            x: 8_800_000,
            y: 2_800_000,
            z: 4_800_000,
        },
    }
}

#[test]
fn authoritative_blast_replicates_then_invalidates_the_immutable_calculation() {
    let mut server = column();
    let mut clients = [
        ClientReplica::new(server.world().clone()),
        ClientReplica::new(server.world().clone()),
    ];
    let request = request(&server);
    let (packet, report) = server
        .execute_explosion(1, small_blast(1, IVec3::new(4, 2, 4)))
        .unwrap();
    assert!(!report.changes.is_empty());
    for client in &mut clients {
        client.receive(&packet).unwrap();
        assert_eq!(client.world().fingerprint(), server.world().fingerprint());
    }
    // The captured world remains immutable even when work begins after the live transaction.
    let completed = run_request(request, &AtomicBool::new(false));
    assert!(completed.result.is_ok());
    assert!(matches!(
        server.structural_result(&completed),
        Err(StructuralJobError::StaleOrForeign)
    ));
    let fresh = calculate(&server, IVec3::new(4, 1, 4));
    assert!(server.structural_result(&fresh).is_ok());
}

#[test]
fn building_in_an_observed_air_cell_adds_load_and_retires_the_previous_result() {
    let mut server = column();
    let before = calculate(&server, IVec3::new(4, 1, 4));
    let previous_reaction = server.structural_result(&before).unwrap().reactions[0][1];
    let position = IVec3::new(5, 3, 4);
    let (packet, _) = server
        .execute_build(
            1,
            BuildCommand {
                command_id: 1,
                position,
                material: Material::Wood,
            },
            builder(),
        )
        .unwrap();
    assert_eq!(packet.changes.len(), 1);
    assert!(matches!(
        server.structural_result(&before),
        Err(StructuralJobError::StaleOrForeign)
    ));
    let after = calculate(&server, IVec3::new(4, 1, 4));
    let result = server.structural_result(&after).unwrap();
    assert!(result.positions.contains(&position));
    assert!(
        (result.reactions[0][1] - previous_reaction)
            .mul_add(1.0, -650.0 * 9.81)
            .abs()
            < 1e-4
    );
}

#[test]
fn repeated_remote_edits_in_existing_unobserved_chunks_do_not_starve_the_domain() {
    let mut world = column().world().clone();
    world.fill_box(
        IVec3::new(100, 0, 4),
        IVec3::new(100, 6, 4),
        Voxel::new(Material::Wood),
    );
    let mut server = AuthoritativeServer::new(world).with_structural_materials(materials());
    let completed = calculate(&server, IVec3::new(4, 1, 4));
    for id in 1..=10 {
        let (_, report) = server
            .execute_explosion(2, small_blast(id, IVec3::new(100, 3, 4)))
            .unwrap();
        assert!(!report.changes.is_empty());
        assert!(server.structural_result(&completed).is_ok());
    }
    assert!(server.advance_physics().0.is_none());
    assert!(
        server.structural_result(&completed).is_ok(),
        "static self-weight ignores only clock/body motion"
    );
}

#[test]
fn repeated_local_edits_reject_old_jobs_and_a_quiet_recalculation_recovers() {
    let mut server = column();
    for id in 1..=6 {
        let completed = calculate(&server, IVec3::new(4, 1, 4));
        server
            .execute_explosion(1, small_blast(id, IVec3::new(4, 3, 4)))
            .unwrap();
        assert!(matches!(
            server.structural_result(&completed),
            Err(StructuralJobError::StaleOrForeign)
        ));
    }
    let fresh = calculate(&server, IVec3::new(4, 1, 4));
    assert!(server.structural_result(&fresh).is_ok());
}

#[test]
fn nonexistent_explicit_support_and_invalid_domains_never_become_supported() {
    let mut world = World::default();
    world.set_voxel(IVec3::new(4, 1, 4), Voxel::new(Material::Wood));
    let anchors = StructuralAnchors::default()
        .with_explicit([IVec3::new(4, 0, 4)])
        .unwrap();
    let server = AuthoritativeServer::new(world)
        .with_structural_materials(materials())
        .with_structural_config(
            anchors,
            crate::StructuralLimits::default(),
            crate::BodyLimits::default(),
        );
    let result = calculate(&server, IVec3::new(4, 1, 4));
    assert!(matches!(
        server.structural_result(&result),
        Err(StructuralJobError::Elastic(
            ElasticError::UnanchoredComponent
        ))
    ));
    let empty = calculate(&server, IVec3::new(4, 0, 4));
    assert!(matches!(
        server.structural_result(&empty),
        Err(StructuralJobError::NoStructure)
    ));
    let server = column();
    let fixed = calculate(&server, IVec3::new(4, 0, 4));
    assert!(matches!(
        server.structural_result(&fixed),
        Err(StructuralJobError::FixedSeed)
    ));
}

#[test]
fn clamp_boundary_does_not_infer_other_free_branches_or_terrain_mass() {
    let mut world = World::default();
    world.fill_box(
        IVec3::new(2, 4, 4),
        IVec3::new(6, 4, 4),
        Voxel::new(Material::Wood),
    );
    let anchors = StructuralAnchors::default()
        .with_explicit([IVec3::new(4, 4, 4)])
        .unwrap();
    let server = AuthoritativeServer::new(world)
        .with_structural_materials(materials())
        .with_structural_config(
            anchors,
            crate::StructuralLimits::default(),
            crate::BodyLimits::default(),
        );
    let completed = calculate(&server, IVec3::new(2, 4, 4));
    let result = server.structural_result(&completed).unwrap();
    assert_eq!(
        result.positions,
        vec![
            IVec3::new(2, 4, 4),
            IVec3::new(3, 4, 4),
            IVec3::new(4, 4, 4)
        ]
    );
    assert!(
        (3.0_f64 * 650.0)
            .mul_add(-9.81, result.reactions[2][1])
            .abs()
            < 1e-4
    );
    // Reactions cover the selected clamped-boundary problem, NOT all loads on a shared foundation.
}

#[test]
fn configuration_changes_invalidate_even_when_voxel_fingerprint_is_unchanged() {
    let server = column();
    let completed = calculate(&server, IVec3::new(4, 1, 4));
    let fingerprint = server.world().fingerprint();
    let server = server.with_structural_materials(materials());
    assert_eq!(server.world().fingerprint(), fingerprint);
    assert!(matches!(
        server.structural_result(&completed),
        Err(StructuralJobError::StaleOrForeign)
    ));
    let completed = calculate(&server, IVec3::new(4, 1, 4));
    let server = server.with_structural_config(
        StructuralAnchors::foundation_plane(0),
        crate::StructuralLimits::default(),
        crate::BodyLimits::default(),
    );
    assert!(matches!(
        server.structural_result(&completed),
        Err(StructuralJobError::StaleOrForeign)
    ));
}

#[test]
fn all_solid_materials_keep_density_and_integrity_and_profiles_reject_nonfinite_parameters() {
    let parameters = StructuralMaterials::new(std::array::from_fn(|index| ElasticMaterial {
        young_modulus_pa: f64::from(u32::try_from(index + 1).unwrap()) * 1e9,
        poisson_ratio: 0.25,
    }))
    .unwrap();
    for id in 1..=7 {
        let material = Material::from_wire(id).unwrap();
        let node = parameters
            .node(
                IVec3::new(1, 2, 3),
                Voxel {
                    material,
                    integrity: 128,
                },
                false,
            )
            .unwrap();
        assert_eq!(
            node.mass_kg.to_bits(),
            f64::from(material.properties().density_kg_m3).to_bits()
        );
        assert_eq!(node.integrity, 128);
        assert_eq!(
            node.young_modulus_pa.to_bits(),
            (f64::from(id) * 1e9).to_bits()
        );
    }
    assert!(matches!(
        parameters.node(IVec3::new(0, 0, 0), Voxel::AIR, false),
        Err(StructuralJobError::InvalidVoxel)
    ));
    for invalid in [
        ElasticMaterial {
            young_modulus_pa: f64::NAN,
            poisson_ratio: 0.25,
        },
        ElasticMaterial {
            young_modulus_pa: 1e9,
            poisson_ratio: 0.5,
        },
        ElasticMaterial {
            young_modulus_pa: 0.0,
            poisson_ratio: 0.25,
        },
        ElasticMaterial {
            young_modulus_pa: 1e9,
            poisson_ratio: f64::INFINITY,
        },
    ] {
        assert!(matches!(
            StructuralMaterials::new([invalid; 7]),
            Err(StructuralJobError::InvalidMaterials)
        ));
    }
}

#[test]
fn worker_reads_remain_immutable_across_an_interleaved_authoritative_write() {
    let mut server = column();
    let captured = request(&server);
    let position = IVec3::new(4, 2, 4);
    let (read, observed) = mpsc::sync_channel(1);
    let (resume, resumed) = mpsc::sync_channel(1);
    let worker = thread::spawn(move || {
        let mut observations = BTreeSet::new();
        let before = read_voxel(&captured.world, position, &mut observations).unwrap();
        read.send(()).unwrap();
        resumed.recv_timeout(Duration::from_secs(3)).unwrap();
        let after = read_voxel(&captured.world, position, &mut observations).unwrap();
        assert_eq!(before, after);
        // Domain extraction and solve continue on this same immutable world after the live write.
        run_request(captured, &AtomicBool::new(false))
    });
    observed.recv_timeout(Duration::from_secs(3)).unwrap();
    let (_, report) = server
        .execute_explosion(1, small_blast(1, position))
        .unwrap();
    assert!(!report.changes.is_empty());
    resume.send(()).unwrap();
    let result = worker.join().unwrap();
    assert!(result.result.is_ok());
    assert!(matches!(
        server.structural_result(&result),
        Err(StructuralJobError::StaleOrForeign)
    ));
}

#[test]
fn observation_guard_accepts_existing_entries_but_refuses_one_beyond_capacity() {
    let world = World::default();
    // Prepopulate the defensive bound: a valid 4096-node domain cannot ordinarily reach it.
    let mut observed: BTreeSet<_> = (0..MAX_OBSERVED_CHUNKS)
        .map(|index| IVec3::new(i32::try_from(index).unwrap(), 0, 0))
        .collect();
    assert_eq!(
        read_voxel(&world, IVec3::new(0, 0, 0), &mut observed),
        Ok(Voxel::AIR)
    );
    assert_eq!(
        read_voxel(&world, IVec3::new(-1, 0, 0), &mut observed),
        Err(StructuralJobError::ObservationLimit)
    );
    assert_eq!(observed.len(), MAX_OBSERVED_CHUNKS);
}

#[test]
fn worker_backpressure_cancellation_and_reuse_hold_until_the_result_is_consumed() {
    let server = column();
    let mut scheduler = StructuralScheduler::new().unwrap();
    assert!(!scheduler.cancel_pending());
    assert_eq!(
        scheduler.submit(
            &AuthoritativeServer::new(World::default()),
            IVec3::new(4, 1, 4)
        ),
        Err(StructuralJobError::NotConfigured)
    );
    assert!(scheduler.poll().unwrap().is_none());
    for _ in 0..4 {
        scheduler.submit(&server, IVec3::new(4, 1, 4)).unwrap();
        assert_eq!(
            scheduler.submit(&server, IVec3::new(4, 1, 4)),
            Err(StructuralJobError::Busy)
        );
        assert!(scheduler.cancel_pending());
        assert_eq!(
            scheduler.submit(&server, IVec3::new(4, 1, 4)),
            Err(StructuralJobError::Busy)
        );
        let cancelled = wait(&mut scheduler);
        assert!(matches!(
            server.structural_result(&cancelled),
            Err(StructuralJobError::Cancelled)
        ));
        scheduler.submit(&server, IVec3::new(4, 1, 4)).unwrap();
        let completed = wait(&mut scheduler);
        assert_eq!(
            server
                .structural_result(&completed)
                .unwrap()
                .positions
                .len(),
            7
        );
    }
}

#[test]
fn snapshot_bounds_include_historical_hash_table_storage() {
    let mut world = World::default();
    for x in 0..1025 {
        world.set_voxel(IVec3::new(x * 16, 0, 0), Voxel::new(Material::Wood));
    }
    let server = AuthoritativeServer::new(world.clone()).with_structural_materials(materials());
    assert!(matches!(
        server.structural_context.capture(
            server.world(),
            server.structural_anchors(),
            IVec3::new(0, 0, 0)
        ),
        Err(StructuralJobError::SnapshotTooLarge)
    ));
    for x in 1..1025 {
        world.set_voxel(IVec3::new(x * 16, 0, 0), Voxel::AIR);
    }
    assert_eq!(world.stats().chunks, 1);
    let server = AuthoritativeServer::new(world).with_structural_materials(materials());
    assert!(matches!(
        server.structural_context.capture(
            server.world(),
            server.structural_anchors(),
            IVec3::new(0, 0, 0)
        ),
        Err(StructuralJobError::SnapshotTooLarge)
    ));
}

#[test]
fn domain_does_not_stop_at_the_first_foundation() {
    let server = column();
    let completed = run_request(request(&server), &AtomicBool::new(false));
    let solution = server.structural_result(&completed).unwrap();
    assert_eq!(solution.positions.len(), 7);
    assert!(
        (7.0_f64 * 650.0)
            .mul_add(-9.81, solution.reactions[0][1])
            .abs()
            < 1e-5
    );
    assert_eq!(completed.observed_chunks(), 1);
}

#[test]
fn a_cloned_authority_cannot_accept_another_servers_result() {
    let server = column();
    let completed = run_request(request(&server), &AtomicBool::new(false));
    assert!(server.structural_result(&completed).is_ok());
    let mut fork = server.clone();
    assert!(matches!(
        fork.structural_result(&completed),
        Err(StructuralJobError::StaleOrForeign)
    ));
    assert!(fork.advance_physics().0.is_none());
    assert_ne!(fork.world().tick(), server.world().tick());
    assert!(server.structural_result(&completed).is_ok());
}

#[test]
fn cancellation_never_exposes_partial_equilibrium() {
    let server = column();
    let completed = run_request(request(&server), &AtomicBool::new(true));
    assert!(matches!(
        server.structural_result(&completed),
        Err(StructuralJobError::Cancelled)
    ));
}

#[test]
fn exact_node_and_snapshot_limits_are_accepted_and_one_more_is_rejected() {
    let mut world = World::default();
    world.fill_box(
        IVec3::new(0, 0, 0),
        IVec3::new(63, 63, 0),
        Voxel::new(Material::Brick),
    );
    let server = AuthoritativeServer::new(world.clone()).with_structural_materials(materials());
    let capture = |server: &AuthoritativeServer| {
        server.structural_context.capture(
            server.world(),
            server.structural_anchors(),
            IVec3::new(0, 1, 0),
        )
    };
    let nodes = extract_domain(
        &capture(&server).unwrap(),
        &mut BTreeSet::new(),
        &AtomicBool::new(false),
    )
    .unwrap();
    assert_eq!(nodes.len(), MAX_ELASTIC_NODES);
    assert_eq!(nodes.iter().filter(|node| node.fixed).count(), 64);
    world.set_voxel(IVec3::new(64, 1, 0), Voxel::new(Material::Brick));
    let oversized = AuthoritativeServer::new(world).with_structural_materials(materials());
    assert!(matches!(
        extract_domain(
            &capture(&oversized).unwrap(),
            &mut BTreeSet::new(),
            &AtomicBool::new(false)
        ),
        Err(StructuralJobError::DomainTooLarge)
    ));

    let mut world = World::default();
    for x in 0..512 {
        world.set_voxel(IVec3::new(x * 16, 0, 0), Voxel::new(Material::Brick));
    }
    let at_limit = AuthoritativeServer::new(world.clone()).with_structural_materials(materials());
    assert!(capture(&at_limit).is_ok());
    world.set_voxel(IVec3::new(512 * 16, 0, 0), Voxel::new(Material::Brick));
    let oversized = AuthoritativeServer::new(world).with_structural_materials(materials());
    assert!(matches!(
        capture(&oversized),
        Err(StructuralJobError::SnapshotTooLarge)
    ));
}

#[test]
fn extreme_neighbors_and_zero_integrity_do_not_fabricate_connections() {
    for coordinate in [i32::MIN, i32::MAX] {
        let corner = IVec3::new(coordinate, coordinate, coordinate);
        let adjacent: Vec<_> = neighbors(corner).collect();
        assert_eq!(adjacent.len(), 3);
        assert!(
            adjacent
                .iter()
                .all(|position| corner.squared_distance(*position) == 1)
        );
        let mut world = World::default();
        world.fill_box(
            IVec3::new(coordinate, 0, coordinate),
            IVec3::new(coordinate, 1, coordinate),
            Voxel::new(Material::Wood),
        );
        let server = AuthoritativeServer::new(world).with_structural_materials(materials());
        let completed = calculate(&server, IVec3::new(coordinate, 1, coordinate));
        assert_eq!(
            server
                .structural_result(&completed)
                .unwrap()
                .positions
                .len(),
            2
        );
    }
    let mut world = World::default();
    world.set_voxel(
        IVec3::new(4, 1, 4),
        Voxel {
            material: Material::Wood,
            integrity: 0,
        },
    );
    let server = AuthoritativeServer::new(world).with_structural_materials(materials());
    let invalid = calculate(&server, IVec3::new(4, 1, 4));
    assert!(matches!(
        server.structural_result(&invalid),
        Err(StructuralJobError::InvalidVoxel)
    ));
}

#[test]
fn building_into_a_previously_absent_neighbor_chunk_invalidates_the_boundary() {
    let mut world = World::default();
    world.fill_box(
        IVec3::new(15, 0, 4),
        IVec3::new(15, 6, 4),
        Voxel::new(Material::Wood),
    );
    let mut server = AuthoritativeServer::new(world).with_structural_materials(materials());
    let before = calculate(&server, IVec3::new(15, 1, 4));
    assert_eq!(
        server.structural_result(&before).unwrap().positions.len(),
        7
    );
    assert_eq!(before.observed_chunks(), 2);
    let mut player = builder();
    player.eye_position_um.x += 11_000_000;
    player.bounds_minimum_um.x += 11_000_000;
    player.bounds_maximum_um.x += 11_000_000;
    server
        .execute_build(
            1,
            BuildCommand {
                command_id: 1,
                position: IVec3::new(16, 3, 4),
                material: Material::Wood,
            },
            player,
        )
        .unwrap();
    assert_eq!(server.world().stats().chunks, 2);
    assert!(matches!(
        server.structural_result(&before),
        Err(StructuralJobError::StaleOrForeign)
    ));
    let after = calculate(&server, IVec3::new(15, 1, 4));
    assert_eq!(server.structural_result(&after).unwrap().positions.len(), 8);
}

#[test]
fn cancelling_a_completed_unconsumed_result_does_not_expose_it() {
    let server = column();
    let mut scheduler = StructuralScheduler::new().unwrap();
    let (completed, receiver) = mpsc::sync_channel(1);
    completed
        .send(calculate(&server, IVec3::new(4, 1, 4)))
        .unwrap();
    // Deterministically put a complete result in the same slot used by the real worker.
    scheduler.receiver = Some(receiver);
    scheduler.busy = true;
    assert!(scheduler.cancel_pending());
    let result = scheduler.poll().unwrap().unwrap();
    assert!(matches!(
        server.structural_result(&result),
        Err(StructuralJobError::Cancelled)
    ));
    assert!(!scheduler.cancel_pending());
}

#[test]
fn stopped_worker_is_explicit_and_drop_joins_inflight_work() {
    let server = column();
    let mut scheduler = StructuralScheduler::new().unwrap();
    scheduler.sender.take();
    scheduler.worker.take().unwrap().join().unwrap();
    assert!(matches!(
        scheduler.poll(),
        Err(StructuralJobError::WorkerStopped)
    ));
    assert_eq!(
        scheduler.submit(&server, IVec3::new(4, 1, 4)),
        Err(StructuralJobError::WorkerStopped)
    );

    let (finished, completion) = mpsc::sync_channel(1);
    let owner = thread::spawn(move || {
        for _ in 0..20 {
            let mut scheduler = StructuralScheduler::new().unwrap();
            scheduler.submit(&server, IVec3::new(4, 1, 4)).unwrap();
            drop(scheduler);
        }
        finished.send(()).unwrap();
    });
    completion
        .recv_timeout(Duration::from_secs(3))
        .expect("scheduler leaked or blocked on shutdown");
    owner.join().unwrap();
}

#[test]
fn foundation_blast_redistributes_weight_onto_the_actual_surviving_supports() {
    let mut world = World::default();
    world.fill_box(
        IVec3::new(0, 0, 0),
        IVec3::new(7, 7, 0),
        Voxel::new(Material::Brick),
    );
    let mut server = AuthoritativeServer::new(world.clone()).with_structural_materials(materials());
    let mut replica = ClientReplica::new(world);
    let seed = IVec3::new(7, 1, 0);
    let intact = calculate(&server, seed);
    let before_maximum = server
        .structural_result(&intact)
        .unwrap()
        .reactions
        .iter()
        .map(|force| force[1])
        .fold(0.0_f64, f64::max);
    let (packet, _) = server
        .execute_explosion(
            1,
            ExplosionCommand {
                command_id: 1,
                center: IVec3::new(0, 0, 0),
                radius_voxels: 2,
                peak_energy: 15_000,
            },
        )
        .unwrap();
    replica.receive(&packet).unwrap();
    assert_eq!(replica.world().fingerprint(), server.world().fingerprint());
    assert!(!server.world().voxel(IVec3::new(0, 0, 0)).is_solid());
    assert!(matches!(
        server.structural_result(&intact),
        Err(StructuralJobError::StaleOrForeign)
    ));
    let damaged = calculate(&server, seed);
    let after = server.structural_result(&damaged).unwrap();
    let after_maximum = after
        .reactions
        .iter()
        .map(|force| force[1])
        .fold(0.0_f64, f64::max);
    assert!(
        after_maximum > before_maximum * 1.1,
        "remaining supports must carry redistributed weight"
    );
    let mass: f64 = after
        .positions
        .iter()
        .map(|position| {
            f64::from(
                server
                    .world()
                    .voxel(*position)
                    .material
                    .properties()
                    .density_kg_m3,
            )
        })
        .sum();
    let reaction: f64 = after.reactions.iter().map(|force| force[1]).sum();
    assert!((-9.81_f64).mul_add(mass, reaction).abs() < mass * 1e-5);
}
