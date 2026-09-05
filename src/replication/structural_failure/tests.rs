use super::*;
use crate::{
    BodyLimits, IVec3, Material, Voxel, World,
    structural_failure::SectionStrength,
    structural_jobs::{ElasticMaterial, StructuralMaterials, StructuralScheduler},
};
use std::{
    thread,
    time::{Duration, Instant},
};

fn fixture() -> AuthoritativeServer {
    let mut world = World::default();
    world.fill_box(
        IVec3::new(4, 0, 4),
        IVec3::new(4, 4, 4),
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
        .with_structural_strengths(
            StructuralStrengths::new(
                [SectionStrength {
                    tension_pa: 1000.0,
                    compression_pa: 1000.0,
                    shear_pa: 1000.0,
                }; 7],
            )
            .unwrap(),
        )
}

fn calculate(server: &AuthoritativeServer) -> CompletedStructuralJob {
    let mut scheduler = StructuralScheduler::new().unwrap();
    scheduler.submit(server, IVec3::new(4, 1, 4)).unwrap();
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        if let Some(result) = scheduler.poll().unwrap() {
            return result;
        }
        assert!(Instant::now() < deadline);
        thread::yield_now();
    }
}

fn unchanged(before: &AuthoritativeServer, after: &AuthoritativeServer) {
    assert_eq!(
        before.world.occupied_voxels(),
        after.world.occupied_voxels()
    );
    assert_eq!(before.world.fingerprint(), after.world.fingerprint());
    assert_eq!(before.world.tick(), after.world.tick());
    assert_eq!(before.next_sequence, after.next_sequence);
    assert_eq!(before.next_body_id, after.next_body_id);
    assert_eq!(before.bodies, after.bodies);
    assert_eq!(before.body_states, after.body_states);
    assert_eq!(before.body_fingerprint, after.body_fingerprint);
    assert_eq!(before.active_body_voxels, after.active_body_voxels);
    assert_eq!(before.last_command_id, after.last_command_id);
    assert_eq!(before.construction_units, after.construction_units);
}

#[test]
fn exhaustion_and_late_limits_preserve_every_authoritative_field() {
    for fault in 0..6 {
        let mut server = fixture();
        let ready = calculate(&server);
        // Fault-inject counters after worker preparation to exercise live precommit checks.
        match fault {
            0 => server.next_body_id = u64::MAX,
            1 => server.next_sequence = u64::MAX,
            2 => server.world.set_tick(u64::MAX),
            3 => server.active_body_voxels = MAX_ACTIVE_BODY_VOXELS,
            4 => server.body_limits.max_voxels = 1,
            _ => {
                // Valid disjoint descriptors emulate unrelated dynamic bodies admitted since capture.
                for id in 1..1024 {
                    let body = crate::RigidBodyDescriptor::from_replicated_voxels(
                        id,
                        vec![crate::BodyVoxel {
                            position: IVec3::new(1000 + i32::try_from(id).unwrap() * 3, 1, 0),
                            voxel: Voxel::new(Material::Wood),
                        }],
                        BodyLimits::default(),
                    )
                    .unwrap();
                    let state = RigidBodyState::at_spawn(&body);
                    server.body_fingerprint ^= body_fingerprint_token(&body, state);
                    server.body_states.insert(id, state);
                    server.bodies.insert(id, body);
                }
                server.active_body_voxels = 1023;
                server.next_body_id = 1024;
            }
        }
        let before = server.clone();
        let result = server.commit_structural_failure(ready);
        match fault {
            0 => assert!(matches!(
                result,
                Err(StructuralFailureError::Commit(
                    CommandError::BodyIdExhausted
                ))
            )),
            1 | 2 => assert!(matches!(
                result,
                Err(StructuralFailureError::SequenceExhausted)
            )),
            3 => assert!(matches!(
                result,
                Err(StructuralFailureError::Commit(
                    CommandError::TooManyActiveBodyVoxels(_)
                ))
            )),
            4 => assert!(matches!(
                result,
                Err(StructuralFailureError::Body(BodyError::TooManyVoxels(_)))
            )),
            _ => assert!(matches!(
                result,
                Err(StructuralFailureError::Commit(
                    CommandError::TooManyActiveBodies(1025)
                ))
            )),
        }
        unchanged(&before, &server);
    }
}

#[test]
fn failed_reservation_leaves_no_id_holes_and_two_old_results_cannot_double_commit() {
    let mut server = fixture();
    let ready = calculate(&server);
    server.body_limits.max_voxels = 1;
    assert!(server.commit_structural_failure(ready).is_err());
    assert_eq!(server.next_body_id, 1);
    server.body_limits.max_voxels = MAX_ELASTIC_NODES;
    let ready = calculate(&server);
    let older = calculate(&server);
    let (packet, _) = server.commit_structural_failure(ready).unwrap().unwrap();
    assert_eq!(packet.body_assignments[0].body_id, 1);
    let before = server.clone();
    assert!(server.commit_structural_failure(older).is_err());
    unchanged(&before, &server);
}

#[test]
fn repeated_real_fracture_commits_reach_the_body_cap_without_overflow_or_partial_mutation() {
    let mut server = fixture();
    server.world = World::default();
    let position = |index: usize| {
        IVec3::new(
            i32::try_from(index % 33).unwrap() * 2,
            1,
            i32::try_from(index / 33).unwrap() * 2,
        )
    };
    for index in 0..=MAX_ACTIVE_BODIES {
        let point = position(index);
        server
            .world
            .set_voxel(IVec3::new(point.x, 0, point.z), Voxel::new(Material::Wood));
        server.world.set_voxel(point, Voxel::new(Material::Wood));
    }
    let mut scheduler = StructuralScheduler::new().unwrap();
    for index in 0..=MAX_ACTIVE_BODIES {
        scheduler.submit(&server, position(index)).unwrap();
        let deadline = Instant::now() + Duration::from_secs(3);
        let ready = loop {
            if let Some(result) = scheduler.poll().unwrap() {
                break result;
            }
            assert!(Instant::now() < deadline);
            thread::yield_now();
        };
        if index == MAX_ACTIVE_BODIES {
            let before = server.clone();
            assert!(matches!(
                server.commit_structural_failure(ready),
                Err(StructuralFailureError::Commit(
                    CommandError::TooManyActiveBodies(1025)
                ))
            ));
            unchanged(&before, &server);
        } else {
            let (_, report) = server.commit_structural_failure(ready).unwrap().unwrap();
            assert_eq!(report.spawned_bodies, 1);
            assert_eq!(server.bodies.len(), index + 1);
        }
    }
}
