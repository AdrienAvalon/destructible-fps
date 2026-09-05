use super::*;
use crate::{
    ExplosionCommand,
    structural_lab::{LAB_SEED, structural_lab},
};
use std::thread;

fn fixture() -> (AuthoritativeServer, StructuralRuntime) {
    let (world, config) = structural_lab();
    let runtime = StructuralRuntime::new(&config.initial_seeds).unwrap();
    let mut authority = AuthoritativeServer::new(world);
    authority.configure_structural_simulation(&config);
    (authority, runtime)
}

fn drain(runtime: &mut StructuralRuntime, authority: &mut AuthoritativeServer) -> Vec<DeltaPacket> {
    let deadline = Instant::now() + Duration::from_secs(3);
    let mut packets = Vec::new();
    loop {
        if let Some(packet) = runtime.tick(authority) {
            packets.push(packet);
        }
        if !runtime.status().busy && runtime.status().queued == 0 {
            return packets;
        }
        assert!(
            Instant::now() < deadline,
            "runtime stalled: {:?}",
            runtime.status()
        );
        thread::yield_now();
    }
}

#[test]
fn partial_damage_automatically_promotes_and_coalesces_a_complete_domain() {
    let (mut authority, mut runtime) = fixture();
    for x in 1..=5 {
        runtime.enqueue(IVec3::new(x, 4, 0));
    }
    assert!(drain(&mut runtime, &mut authority).is_empty());
    assert_eq!(runtime.status().submitted, 1);
    let (blast, _) = authority
        .execute_explosion(
            1,
            ExplosionCommand {
                command_id: 1,
                center: LAB_SEED,
                radius_voxels: 1,
                peak_energy: 1000,
            },
        )
        .unwrap();
    runtime.observe_changes(&authority, &blast.changes);
    let packets = drain(&mut runtime, &mut authority);
    assert_eq!(packets.len(), 1);
    assert_eq!(authority.bodies().len(), 2);
    assert_eq!(runtime.status().failed, 0);
}

#[test]
fn queue_is_bounded_fifo_and_invalid_seed_skipping_is_tick_bounded() {
    let mut runtime = StructuralRuntime::new(&[]).unwrap();
    for x in 0..=MAX_PENDING_STRUCTURAL_SEEDS {
        runtime.enqueue(IVec3::new(i32::try_from(x).unwrap(), 1, 0));
    }
    assert_eq!(runtime.status().queued, MAX_PENDING_STRUCTURAL_SEEDS);
    assert!(runtime.status().overflowed);
    let mut authority = AuthoritativeServer::new(crate::World::default());
    assert!(runtime.tick(&mut authority).is_none());
    assert_eq!(
        runtime.status().queued,
        MAX_PENDING_STRUCTURAL_SEEDS - MAX_SEED_CHECKS_PER_TICK
    );
    assert_eq!(runtime.queue.front(), Some(&IVec3::new(64, 1, 0)));
    drain(&mut runtime, &mut authority);
    assert!(
        runtime.status().overflowed,
        "draining must not erase lost coverage"
    );
}

#[test]
fn deadline_is_latched_without_reusing_or_waiting_for_the_worker() {
    let (mut authority, mut runtime) = fixture();
    runtime.tick(&mut authority);
    runtime.started = Instant::now().checked_sub(MAX_JOB_LATENCY);
    let fingerprint = authority.world().fingerprint();
    assert!(runtime.tick(&mut authority).is_none());
    assert!(runtime.status().stopped);
    assert!(runtime.status().busy);
    assert_eq!(
        runtime.last_error(),
        Some(&StructuralFailureError::DeadlineExceeded)
    );
    for _ in 0..10 {
        assert!(runtime.tick(&mut authority).is_none());
    }
    assert_eq!(authority.world().fingerprint(), fingerprint);
    assert_eq!(runtime.status().failed, 1);
}

#[test]
fn stale_domain_moves_behind_another_structure_without_erasing_dirty_work() {
    let (mut world, config) = structural_lab();
    let second = IVec3::new(12, 1, 8);
    world.fill_box(
        second,
        IVec3::new(12, 2, 8),
        crate::Voxel::new(crate::Material::Wood),
    );
    let mut authority = AuthoritativeServer::new(world);
    authority.configure_structural_simulation(&config);
    let mut runtime = StructuralRuntime::new(&[LAB_SEED, second]).unwrap();
    runtime.tick(&mut authority);
    let (packet, _) = authority
        .execute_explosion(
            1,
            ExplosionCommand {
                command_id: 1,
                center: LAB_SEED,
                radius_voxels: 1,
                peak_energy: 1000,
            },
        )
        .unwrap();
    runtime.observe_changes(&authority, &packet.changes);
    let deadline = Instant::now() + Duration::from_secs(3);
    while runtime.status().stale == 0 {
        assert!(runtime.tick(&mut authority).is_none());
        assert!(Instant::now() < deadline);
        thread::yield_now();
    }
    assert_eq!(runtime.in_flight, Some(second));
    assert!(runtime.pending.contains(&LAB_SEED));
    assert_eq!(drain(&mut runtime, &mut authority).len(), 1);
    assert_eq!(runtime.status().failed, 0);
}

#[test]
fn a_capacity_boundary_is_not_a_clamp_or_a_successful_assessment() {
    let (_, config) = structural_lab();
    let mut world = crate::World::default();
    world.fill_box(
        IVec3::new(0, 0, 8),
        IVec3::new(4096, 1, 8),
        crate::Voxel::new(crate::Material::Wood),
    );
    let mut authority = AuthoritativeServer::new(world);
    authority.configure_structural_simulation(&config);
    let before = authority.world().fingerprint();
    let mut runtime = StructuralRuntime::new(&[IVec3::new(0, 1, 8)]).unwrap();
    assert!(drain(&mut runtime, &mut authority).is_empty());
    assert_eq!(
        runtime.last_error(),
        Some(&StructuralFailureError::Job(
            StructuralJobError::DomainTooLarge
        ))
    );
    assert_eq!(runtime.status().assessed, 0);
    assert_eq!(runtime.status().failed, 1);
    assert_eq!(authority.world().fingerprint(), before);
    assert!(authority.bodies().is_empty());
}

#[test]
fn complete_but_out_of_range_domain_is_coalesced_without_hiding_failure() {
    let (world, mut config) = structural_lab();
    config.materials = crate::structural_jobs::StructuralMaterials::new(
        [crate::structural_jobs::ElasticMaterial {
            young_modulus_pa: 1e4,
            poisson_ratio: 0.25,
        }; 7],
    )
    .unwrap();
    let mut authority = AuthoritativeServer::new(world);
    authority.configure_structural_simulation(&config);
    let seeds: Vec<_> = (1..=5).map(|x| IVec3::new(x, 4, 0)).collect();
    let mut runtime = StructuralRuntime::new(&seeds).unwrap();
    assert!(drain(&mut runtime, &mut authority).is_empty());
    assert_eq!(runtime.status().submitted, 1);
    assert_eq!(runtime.status().assessed, 0);
    assert_eq!(runtime.status().failed, 1);
    assert_eq!(
        runtime.last_error(),
        Some(&StructuralFailureError::Job(StructuralJobError::Elastic(
            crate::elasticity::ElasticError::OutsideLinearRegime
        )))
    );
}

#[test]
fn submit_failure_keeps_an_explicit_incomplete_state_and_exact_seed() {
    let (world, _) = structural_lab();
    let mut authority = AuthoritativeServer::new(world);
    let mut runtime = StructuralRuntime::new(&[LAB_SEED]).unwrap();
    assert!(runtime.tick(&mut authority).is_none());
    assert!(runtime.status().incomplete());
    assert_eq!(runtime.status().last_failed_seed, Some(LAB_SEED));
    assert_eq!(
        runtime.last_error(),
        Some(&StructuralFailureError::Job(
            StructuralJobError::NotConfigured
        ))
    );
    for _ in 0..10 {
        assert!(runtime.tick(&mut authority).is_none());
    }
    assert!(runtime.status().incomplete());
    assert_eq!(
        runtime.status().failed,
        1,
        "do not busy-retry an unconfigured policy"
    );
}

#[test]
fn coalescing_positions_follow_canonical_map_order_not_breadth_first_order() {
    let (authority, _) = fixture();
    let mut scheduler = StructuralScheduler::new().unwrap();
    scheduler.submit(&authority, IVec3::new(3, 4, 0)).unwrap();
    let deadline = Instant::now() + Duration::from_secs(3);
    let completed = loop {
        if let Some(completed) = scheduler.poll().unwrap() {
            break completed;
        }
        assert!(Instant::now() < deadline);
        thread::yield_now();
    };
    let positions = authority
        .structural_context
        .validate_domain(authority.world(), &completed)
        .unwrap();
    assert_eq!(positions.len(), 6);
    assert!(positions.windows(2).all(|pair| pair[0] < pair[1]));
    assert_eq!(
        positions,
        authority.structural_result(&completed).unwrap().positions
    );
}

#[test]
fn stale_requeue_at_exact_capacity_reports_lost_coverage() {
    let (mut authority, mut runtime) = fixture();
    runtime.tick(&mut authority);
    for x in 0..MAX_PENDING_STRUCTURAL_SEEDS {
        runtime.enqueue(IVec3::new(10_000 + i32::try_from(x).unwrap(), 1, 0));
    }
    assert!(!runtime.status().overflowed);
    authority
        .execute_explosion(
            1,
            ExplosionCommand {
                command_id: 1,
                center: LAB_SEED,
                radius_voxels: 1,
                peak_energy: 1000,
            },
        )
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(3);
    let completed = loop {
        if let Some(completed) = runtime.scheduler.poll().unwrap() {
            break completed;
        }
        assert!(Instant::now() < deadline);
        thread::yield_now();
    };
    let before = authority.world().fingerprint();
    assert!(
        runtime
            .complete(&mut authority, LAB_SEED, completed)
            .is_none()
    );
    assert!(runtime.status().overflowed);
    assert!(runtime.status().incomplete());
    assert!(!runtime.pending.contains(&LAB_SEED));
    assert_eq!(authority.world().fingerprint(), before);
}
