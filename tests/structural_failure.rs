use destructible_fps::{
    AuthoritativeServer, BodyLimits, ClientReplica, ExplosionCommand, FrameAssembler, IVec3,
    Material, SnapshotAssembler, StructuralAnchors, StructuralLimits, Voxel, World, decode_frame,
    encode_frames, encode_snapshot_frames,
    structural_failure::{SectionStrength, StructuralFailureError, StructuralStrengths},
    structural_jobs::{
        CompletedStructuralJob, ElasticMaterial, StructuralJobError, StructuralMaterials,
        StructuralScheduler,
    },
};
use std::{
    thread,
    time::{Duration, Instant},
};

fn strengths(value: f64) -> StructuralStrengths {
    StructuralStrengths::new(
        [SectionStrength {
            tension_pa: value,
            compression_pa: value * 5.0,
            shear_pa: value,
        }; 7],
    )
    .unwrap()
}

fn fixture() -> AuthoritativeServer {
    let mut world = World::default();
    world.fill_box(
        IVec3::new(-2, 0, -1),
        IVec3::new(8, 0, 1),
        Voxel::new(Material::Stone),
    );
    world.fill_box(
        IVec3::new(0, 1, 0),
        IVec3::new(0, 4, 0),
        Voxel::new(Material::Stone),
    );
    world.fill_box(
        IVec3::new(1, 4, 0),
        IVec3::new(5, 4, 0),
        Voxel::new(Material::Wood),
    );
    world.fill_box(
        IVec3::new(100, 0, 0),
        IVec3::new(102, 0, 0),
        Voxel::new(Material::Stone),
    );
    world.fill_box(
        IVec3::new(100, 1, 0),
        IVec3::new(100, 5, 0),
        Voxel::new(Material::Wood),
    );
    AuthoritativeServer::new(world)
        .with_structural_materials(
            StructuralMaterials::new(
                [ElasticMaterial {
                    young_modulus_pa: 1e9,
                    poisson_ratio: 0.25,
                }; 7],
            )
            .unwrap(),
        )
        .with_structural_strengths(strengths(1e6))
        .with_structural_config(
            StructuralAnchors::foundation_plane(0)
                .with_explicit([IVec3::new(0, 4, 0)])
                .unwrap(),
            StructuralLimits::default(),
            BodyLimits::default(),
        )
}

fn calculate(server: &AuthoritativeServer) -> CompletedStructuralJob {
    let mut scheduler = StructuralScheduler::new().unwrap();
    scheduler.submit(server, IVec3::new(1, 4, 0)).unwrap();
    wait(&mut scheduler)
}

fn wait(scheduler: &mut StructuralScheduler) -> CompletedStructuralJob {
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        if let Some(completed) = scheduler.poll().unwrap() {
            return completed;
        }
        assert!(
            Instant::now() < deadline,
            "bounded structural worker stalled"
        );
        thread::yield_now();
    }
}

fn weaken(server: &mut AuthoritativeServer, id: u64) -> destructible_fps::DeltaPacket {
    server
        .execute_explosion(
            7,
            ExplosionCommand {
                command_id: id,
                center: IVec3::new(1, 4, 0),
                radius_voxels: 1,
                peak_energy: 1000,
            },
        )
        .unwrap()
        .0
}

fn mass(server: &AuthoritativeServer) -> u64 {
    server
        .world()
        .occupied_voxels()
        .iter()
        .map(|(_, v)| u64::from(v.material.properties().density_kg_m3))
        .sum::<u64>()
        + server
            .bodies()
            .values()
            .map(|body| body.mass_kg)
            .sum::<u64>()
}

fn same(server: &AuthoritativeServer, client: &ClientReplica) {
    assert_eq!(client.world().fingerprint(), server.world().fingerprint());
    assert_eq!(client.bodies(), server.bodies());
    assert_eq!(client.body_states(), server.body_states());
    assert_eq!(client.body_fingerprint(), server.body_fingerprint());
}

#[test]
fn weakened_cantilever_fails_conserves_mass_falls_and_repairs_on_two_replicas() {
    let mut server = fixture();
    let mut clients = [
        ClientReplica::new(server.world().clone()),
        ClientReplica::new(server.world().clone()),
    ];
    let original_mass = mass(&server);
    let intact = calculate(&server);
    assert!(server.commit_structural_failure(intact).unwrap().is_none());
    assert_eq!(server.next_sequence(), 1);
    let old = calculate(&server);
    let blast = weaken(&mut server, 1);
    for client in &mut clients {
        client.receive(&blast).unwrap();
    }
    assert_eq!(
        mass(&server),
        original_mass,
        "partial damage must not erase matter"
    );
    assert!(matches!(
        server.commit_structural_failure(old),
        Err(StructuralFailureError::Job(
            StructuralJobError::StaleOrForeign
        ))
    ));
    let ready = calculate(&server);
    let (packet, report) = server
        .commit_structural_failure(ready)
        .unwrap()
        .expect("weakened root must fail");
    assert_eq!(report.candidate.position, IVec3::new(1, 4, 0));
    assert_eq!(report.spawned_bodies, 2);
    assert_eq!(report.moved_voxels, 5);
    assert_eq!(report.mass_kg, 5 * 650);
    assert_eq!(mass(&server), original_mass);
    assert_eq!(packet.sequence, 2);
    assert!(packet.body_updates.is_empty(), "no fabricated blast kick");
    let mut assembler = FrameAssembler::default();
    let mut decoded = None;
    for frame in encode_frames(&packet, 1200).unwrap().into_iter().rev() {
        decoded = assembler
            .push(decode_frame(&frame).unwrap())
            .unwrap()
            .or(decoded);
    }
    let decoded = decoded.unwrap();
    assert_eq!(decoded, packet);
    for client in &mut clients {
        client.receive(&decoded).unwrap();
        same(&server, client);
    }
    assert_eq!(
        clients[0].receive(&decoded).unwrap(),
        destructible_fps::ClientStatus::DuplicateIgnored,
        "replay cannot duplicate mass"
    );
    same(&server, &clients[0]);
    for _ in 0..360 {
        if let (Some(motion), _) = server.advance_physics() {
            for client in &mut clients {
                client.receive(&motion).unwrap();
                same(&server, client);
            }
        }
    }
    assert!(
        server
            .body_states()
            .values()
            .all(|state| (999_000..=1_001_000).contains(&state.translation_um.y) && state.sleeping),
        "bodies must fall from y=4m to the floor top at y=1m: {:?}",
        server.body_states()
    );
    assert_eq!(mass(&server), original_mass);
    let mut snapshot = SnapshotAssembler::default();
    let mut rebuilt = None;
    for frame in encode_snapshot_frames(17, &server, 1200)
        .unwrap()
        .into_iter()
        .rev()
    {
        rebuilt = snapshot.push(&frame).unwrap().or(rebuilt);
    }
    let mut late = ClientReplica::new(World::default());
    rebuilt.unwrap().install_into(&mut late).unwrap();
    same(&server, &late);
}

#[test]
fn foreign_reconfigured_cancelled_and_unconfigured_jobs_do_not_commit() {
    let mut server = fixture();
    weaken(&mut server, 1);
    let before = (
        server.world().fingerprint(),
        server.next_sequence(),
        mass(&server),
    );
    let completed = calculate(&server);
    let mut foreign = server.clone();
    assert!(matches!(
        foreign.commit_structural_failure(completed),
        Err(StructuralFailureError::Job(
            StructuralJobError::StaleOrForeign
        ))
    ));
    let completed = calculate(&server);
    server = server.with_structural_strengths(strengths(1e6));
    assert!(matches!(
        server.commit_structural_failure(completed),
        Err(StructuralFailureError::Job(
            StructuralJobError::StaleOrForeign
        ))
    ));
    let mut scheduler = StructuralScheduler::new().unwrap();
    scheduler.submit(&server, IVec3::new(1, 4, 0)).unwrap();
    assert!(scheduler.cancel_pending());
    let cancelled = wait(&mut scheduler);
    assert!(matches!(
        server.commit_structural_failure(cancelled),
        Err(StructuralFailureError::Job(StructuralJobError::Cancelled))
    ));
    assert_eq!(
        before,
        (
            server.world().fingerprint(),
            server.next_sequence(),
            mass(&server)
        )
    );
    assert!(server.bodies().is_empty());

    let mut analysis_only = AuthoritativeServer::new(server.world().clone())
        .with_structural_materials(
            StructuralMaterials::new(
                [ElasticMaterial {
                    young_modulus_pa: 1e9,
                    poisson_ratio: 0.25,
                }; 7],
            )
            .unwrap(),
        );
    let unconfigured = calculate(&analysis_only);
    assert!(matches!(
        analysis_only.commit_structural_failure(unconfigured),
        Err(StructuralFailureError::NotConfigured)
    ));
}

#[test]
fn per_body_limit_refusal_preserves_state_and_does_not_consume_command_ids() {
    let mut server = fixture().with_structural_config(
        StructuralAnchors::foundation_plane(0)
            .with_explicit([IVec3::new(0, 4, 0)])
            .unwrap(),
        StructuralLimits::default(),
        BodyLimits { max_voxels: 1 },
    );
    weaken(&mut server, 1);
    let before = (
        server.world().fingerprint(),
        server.next_sequence(),
        server.world().tick(),
    );
    let ready = calculate(&server);
    assert!(matches!(
        server.commit_structural_failure(ready),
        Err(StructuralFailureError::Body(_))
    ));
    assert_eq!(
        before,
        (
            server.world().fingerprint(),
            server.next_sequence(),
            server.world().tick()
        )
    );
    assert!(server.bodies().is_empty());
    // ID 2 is still the player's next command: internal jobs do not consume its replay state.
    server
        .execute_explosion(
            7,
            ExplosionCommand {
                command_id: 2,
                center: IVec3::new(80, 4, 0),
                radius_voxels: 1,
                peak_energy: 1,
            },
        )
        .unwrap();
}

#[test]
fn body_ids_and_packet_bases_are_reserved_at_commit_after_an_unrelated_blast() {
    let mut server = fixture();
    let mut replica = ClientReplica::new(server.world().clone());
    replica.receive(&weaken(&mut server, 1)).unwrap();
    let ready = calculate(&server);
    let unrelated = server
        .execute_explosion(
            7,
            ExplosionCommand {
                command_id: 2,
                center: IVec3::new(100, 1, 0),
                radius_voxels: 1,
                peak_energy: 10_000,
            },
        )
        .unwrap()
        .0;
    assert_eq!(server.bodies().len(), 1);
    replica.receive(&unrelated).unwrap();
    server.structural_result(&ready).unwrap();
    let before = server.body_fingerprint();
    let (packet, _) = server.commit_structural_failure(ready).unwrap().unwrap();
    assert_eq!(packet.sequence, 3);
    assert_eq!(packet.base_body_fingerprint, before);
    assert_eq!(
        server.bodies().keys().copied().collect::<Vec<_>>(),
        vec![1, 2, 3]
    );
    replica.receive(&packet).unwrap();
    same(&server, &replica);
}

#[test]
fn deleting_the_anchor_or_editing_a_different_domain_cell_rejects_prepared_failure() {
    for center in [IVec3::new(0, 4, 0), IVec3::new(4, 4, 0)] {
        let mut server = fixture();
        weaken(&mut server, 1);
        let ready = calculate(&server);
        server
            .execute_explosion(
                7,
                ExplosionCommand {
                    command_id: 2,
                    center,
                    radius_voxels: 1,
                    peak_energy: if center.x == 0 { 30_000 } else { 100 },
                },
            )
            .unwrap();
        let state = (
            server.world().fingerprint(),
            server.body_fingerprint(),
            server.next_sequence(),
            mass(&server),
        );
        assert!(matches!(
            server.commit_structural_failure(ready),
            Err(StructuralFailureError::Job(
                StructuralJobError::StaleOrForeign
            ))
        ));
        assert_eq!(
            state,
            (
                server.world().fingerprint(),
                server.body_fingerprint(),
                server.next_sequence(),
                mass(&server)
            )
        );
    }
}

#[test]
fn a_second_explicit_solve_detaches_the_panel_after_the_other_weak_support_fails() {
    let mut world = World::default();
    world.fill_box(
        IVec3::new(0, 2, 0),
        IVec3::new(5, 7, 0),
        Voxel::new(Material::Brick),
    );
    for x in [0, 5] {
        world.set_voxel(IVec3::new(x, 0, 0), Voxel::new(Material::Stone));
        world.set_voxel(IVec3::new(x, 1, 0), Voxel::new(Material::Wood));
    }
    let mut profiles = [SectionStrength {
        tension_pa: 1e12,
        compression_pa: 1e12,
        shear_pa: 1e12,
    }; 7];
    profiles[2] = SectionStrength {
        tension_pa: 1000.0,
        compression_pa: 1000.0,
        shear_pa: 1000.0,
    };
    let mut server = AuthoritativeServer::new(world)
        .with_structural_materials(
            StructuralMaterials::new(
                [ElasticMaterial {
                    young_modulus_pa: 1e9,
                    poisson_ratio: 0.25,
                }; 7],
            )
            .unwrap(),
        )
        .with_structural_strengths(StructuralStrengths::new(profiles).unwrap());
    let original_mass = mass(&server);
    let mut replica = ClientReplica::new(server.world().clone());
    for stage in 0..2 {
        let ready = calculate(&server);
        let (packet, report) = server.commit_structural_failure(ready).unwrap().unwrap();
        assert_eq!(report.candidate.position.y, 1);
        assert_eq!(report.moved_voxels, if stage == 0 { 1 } else { 37 });
        replica.receive(&packet).unwrap();
        same(&server, &replica);
        assert_eq!(mass(&server), original_mass);
    }
    assert_eq!(server.world().stats().solid_voxels, 2);
    assert_eq!(server.bodies().len(), 3);
}

#[test]
fn outside_linear_regime_is_an_unresolved_error_not_a_success_or_automatic_cut() {
    let mut server = fixture().with_structural_materials(
        StructuralMaterials::new(
            [ElasticMaterial {
                young_modulus_pa: 1e4,
                poisson_ratio: 0.25,
            }; 7],
        )
        .unwrap(),
    );
    let before = (
        server.world().fingerprint(),
        server.next_sequence(),
        mass(&server),
    );
    let ready = calculate(&server);
    assert!(matches!(
        server.commit_structural_failure(ready),
        Err(StructuralFailureError::Job(StructuralJobError::Elastic(
            destructible_fps::elasticity::ElasticError::OutsideLinearRegime
        )))
    ));
    assert_eq!(
        before,
        (
            server.world().fingerprint(),
            server.next_sequence(),
            mass(&server)
        )
    );
    assert!(server.bodies().is_empty());
}
