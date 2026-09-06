use super::*;
use crate::{
    ClientPrediction, Material, ReplicatedPlayerState, Voxel,
    volume::{LocalBox, RefinedVolume, VolumeLimits},
    world::geometry::{
        GeometryCell, GeometryChange, GeometryState, GeometryTransaction, RefinedWorld,
    },
};

fn install(
    state: &mut GeometryState,
    mut cells: Vec<(IVec3, GeometryCell)>,
) -> GeometryTransaction {
    cells.sort_unstable_by_key(|(position, _)| *position);
    let changes = cells
        .into_iter()
        .map(|(position, after)| GeometryChange {
            position,
            before: state.world().cell(position),
            after,
        })
        .collect();
    let transaction = state.prepare(state.world().tick() + 1, changes).unwrap();
    state.apply(&transaction).unwrap();
    transaction
}

fn door(width: [u16; 2]) -> (GeometryState, GeometryTransaction, GeometryState) {
    let mut coarse = World::default();
    coarse.fill_box(
        IVec3::new(-5, 0, -8),
        IVec3::new(5, 0, 8),
        Voxel::new(Material::Stone),
    );
    coarse.fill_box(
        IVec3::new(-2, 1, 0),
        IVec3::new(2, 3, 0),
        Voxel::new(Material::Brick),
    );
    let base = GeometryState::new(RefinedWorld::from_uniform(&coarse).unwrap(), 1).unwrap();
    let mut state = base.clone();
    let (volume, _) = RefinedVolume::uniform(Voxel::new(Material::Brick))
        .replace_box(
            LocalBox::new([width[0], 0, 0], [width[1], 256, 256]).unwrap(),
            Voxel::AIR,
            VolumeLimits::default(),
        )
        .unwrap();
    let transaction = install(
        &mut state,
        (1..=2)
            .map(|y| (IVec3::new(0, y, 0), GeometryCell::refined(volume.clone())))
            .collect(),
    );
    (state, transaction, base)
}

fn spawn() -> AuthoritativePlayer {
    AuthoritativePlayer::new(FixedMicrometers3 {
        x: 500_000,
        y: 1_000_000,
        z: 2_000_000,
    })
}

fn input(sequence: u64) -> PlayerInputCommand {
    PlayerInputCommand {
        input_sequence: sequence,
        movement_z_per_mille: -1000,
        ..PlayerInputCommand::default()
    }
}

#[test]
fn actual_character_and_prediction_pass_fine_door_on_two_replicas_and_repaired_checkpoint() {
    let (authority, transaction, mut replica) = door([26, 230]); // 796.875mm gap; player600mm.
    replica
        .apply(&GeometryTransaction::decode(&transaction.encode().unwrap()).unwrap())
        .unwrap();
    let repaired =
        GeometryState::decode_checkpoint(&authority.encode_checkpoint().unwrap()).unwrap();
    let mut server = spawn();
    let mut restored_reader = server;
    let mut prediction = ClientPrediction::new(
        1,
        ReplicatedPlayerState::from_authoritative(7, server.state()),
    )
    .unwrap();
    let mut delayed = server.state();
    for sequence in 1..=60 {
        server.accept_input(input(sequence)).unwrap();
        restored_reader.accept_input(input(sequence)).unwrap();
        let actual = server.step(authority.world()).unwrap();
        let restored = restored_reader.step(repaired.world()).unwrap();
        let predicted = prediction
            .predict(input(sequence), replica.world())
            .unwrap();
        assert_eq!(actual, restored); // Includes exact canonical query work, not just movement.
        assert_eq!(actual, predicted.simulation);
        assert_eq!(server.state(), prediction.state());
        assert_eq!(server, restored_reader);
        if sequence == 30 {
            delayed = server.state();
        }
        if sequence == 33 {
            let report = prediction
                .reconcile(
                    31,
                    ReplicatedPlayerState::from_authoritative(7, delayed),
                    replica.world(),
                )
                .unwrap();
            assert_eq!(report.replayed_inputs, 3);
            assert_eq!(prediction.state(), server.state());
        }
    }
    assert!(
        server.state.position_um.z < -2_000_000,
        "must cross actual remaining wall, not a deleted metre cell"
    );
    assert_eq!(authority.world().geometry_stats().refined_pages, 2);
}

#[test]
fn remaining_material_at_narrow_gap_blocks_the_full_character() {
    let (state, _, _) = door([80, 176]); // 375mm gap, narrower than the600mm body.
    let mut player = spawn();
    for sequence in 1..=60 {
        player.accept_input(input(sequence)).unwrap();
        player.step(state.world()).unwrap();
    }
    assert_eq!(player.state.position_um.z, 1_300_000);
    assert!(player.is_clear_of_static_world(state.world()).unwrap());
}

fn thin_floor() -> GeometryState {
    let (volume, _) = RefinedVolume::uniform(Voxel::AIR)
        .replace_box(
            LocalBox::new([0, 128, 0], [256, 129, 256]).unwrap(),
            Voxel::new(Material::Steel),
            VolumeLimits::default(),
        )
        .unwrap();
    let mut state = GeometryState::new(RefinedWorld::default(), 1).unwrap();
    install(
        &mut state,
        vec![(IVec3::new(0, 0, 0), GeometryCell::refined(volume))],
    );
    state
}

#[test]
fn thin_fractional_floor_stops_terminal_fall_and_removed_support_cannot_supply_a_jump() {
    let mut state = thin_floor();
    let mut player = AuthoritativePlayer::new(FixedMicrometers3 {
        x: 500_000,
        y: 10_000_000,
        z: 500_000,
    });
    player.state.velocity_um_per_second.y = MAX_FALL_SPEED_UM_PER_SECOND;
    for _ in 0..60 {
        player.step(state.world()).unwrap();
    }
    // Plane at503906.25um: whole-um state stays0.75um ABOVE the exact floor.
    assert_eq!(player.state.position_um.y, 503_907);
    assert!(player.state.grounded);
    let retained_floor = state.clone();
    install(&mut state, vec![(IVec3::new(0, 0, 0), GeometryCell::AIR)]);
    player
        .accept_input(PlayerInputCommand {
            input_sequence: 1,
            jump: true,
            ..PlayerInputCommand::default()
        })
        .unwrap();
    player.step(state.world()).unwrap();
    assert_eq!(
        player.state.velocity_um_per_second.y,
        -GRAVITY_UM_PER_SECOND_PER_TICK
    );
    assert!(!player.state.grounded);
    assert!(player.state.position_um.y < 503_907);
    assert_eq!(retained_floor.world().geometry_stats().refined_pages, 1);
}

#[test]
fn high_legal_reconciled_velocity_cannot_skip_a_thin_wall() {
    let (volume, _) = RefinedVolume::uniform(Voxel::AIR)
        .replace_box(
            LocalBox::new([128, 0, 0], [129, 256, 256]).unwrap(),
            Voxel::new(Material::Steel),
            VolumeLimits::default(),
        )
        .unwrap();
    let mut state = GeometryState::new(RefinedWorld::default(), 1).unwrap();
    install(
        &mut state,
        (0..=2)
            .map(|y| (IVec3::new(0, y, 0), GeometryCell::refined(volume.clone())))
            .collect(),
    );
    let mut player = AuthoritativePlayer::new(FixedMicrometers3 {
        x: 100_000,
        y: 0,
        z: 500_000,
    });
    player.state.velocity_um_per_second.x =
        crate::player_replication::MAX_REPLICATED_PLAYER_VELOCITY_UM_PER_SECOND;
    let report = player.step(state.world()).unwrap();
    assert!(report.collided);
    assert_eq!(player.state.position_um.x, 200_000);
    assert_eq!(player.state.velocity_um_per_second.x, 0);
}

#[test]
fn query_refusal_preserves_every_character_and_prediction_field() {
    let (state, _, _) = door([26, 230]);
    let mut player = spawn();
    player
        .accept_input(PlayerInputCommand {
            jump: true,
            ..input(1)
        })
        .unwrap();
    let original = player;
    assert_eq!(
        player.step_with_limits(
            state.world(),
            QueryLimits {
                cells: 1,
                leaf_visits: 1
            }
        ),
        Err(GeometryQueryError::CellBudget)
    );
    assert_eq!(player, original);
    player.step(state.world()).unwrap();
    let mut blocked = World::default();
    blocked.fill_box(
        IVec3::new(-1, 0, 1),
        IVec3::new(1, 3, 3),
        Voxel::new(Material::Stone),
    );
    let server_state = ReplicatedPlayerState::from_authoritative(7, spawn().state());
    let mut prediction = ClientPrediction::new(1, server_state).unwrap();
    let original = prediction.clone();
    assert!(matches!(
        prediction.predict(input(1), &blocked),
        Err(crate::ClientPredictionError::Geometry(
            GeometryQueryError::InitialOverlap
        ))
    ));
    assert_eq!(prediction, original);
    prediction.predict(input(1), state.world()).unwrap();
    let original = prediction.clone();
    assert!(matches!(
        prediction.reconcile(2, server_state, &blocked),
        Err(crate::ClientPredictionError::Geometry(
            GeometryQueryError::InitialOverlap
        ))
    ));
    assert_eq!(prediction, original);
}

#[test]
fn head_contact_uses_full_build_bounds_and_reset_never_enters_an_occupied_spawn() {
    let mut world = World::default();
    world.fill_box(
        IVec3::new(0, 0, 0),
        IVec3::new(0, 0, 0),
        Voxel::new(Material::Stone),
    );
    let mut player = AuthoritativePlayer::new(FixedMicrometers3 {
        x: 500_000,
        y: 1_000_000,
        z: 500_000,
    });
    // Ceiling starts781.25um beyond the full1.8m head bounds; the jump must stop there.
    let mut fine = GeometryState::new(RefinedWorld::from_uniform(&world).unwrap(), 1).unwrap();
    let (ceiling, _) = RefinedVolume::uniform(Voxel::AIR)
        .replace_box(
            LocalBox::new([0, 205, 0], [256, 256, 256]).unwrap(),
            Voxel::new(Material::Wood),
            VolumeLimits::default(),
        )
        .unwrap();
    install(
        &mut fine,
        vec![(IVec3::new(0, 2, 0), GeometryCell::refined(ceiling))],
    );
    player
        .accept_input(PlayerInputCommand {
            input_sequence: 1,
            jump: true,
            ..PlayerInputCommand::default()
        })
        .unwrap();
    player.step(fine.world()).unwrap();
    assert_eq!(player.state.position_um.y, 1_000_781); // Ceiling2800781.25 minus height1800000, floor toward start.
    player.state.position_um.y = -29_900_000;
    player.state.velocity_um_per_second.y = MAX_FALL_SPEED_UM_PER_SECOND;
    let mut blocked = fine.clone();
    install(
        &mut blocked,
        vec![(
            IVec3::new(0, 1, 0),
            GeometryCell::uniform(Voxel::new(Material::Stone)),
        )],
    );
    let before = player;
    assert_eq!(
        player.step(blocked.world()),
        Err(GeometryQueryError::UnsafeSpawn)
    );
    assert_eq!(player, before);
}

#[test]
fn full_prediction_history_replays_dense_geometry_with_the_same_per_tick_budget_as_authority() {
    let mut stream = b"DFVL\x02".to_vec();
    stream.extend_from_slice(&8192_u32.to_le_bytes());
    for z in 0..64_u16 {
        for x in 0..128_u16 {
            for end in [(x + 1) * 2, 256, (z + 1) * 4] {
                stream.extend_from_slice(&end.to_le_bytes());
            }
            let material = if (x + z) % 2 == 0 {
                Material::Wood
            } else {
                Material::Steel
            };
            stream.extend_from_slice(&[material as u8, 255]);
        }
    }
    let mut geometry = GeometryState::new(RefinedWorld::default(), 1).unwrap();
    install(
        &mut geometry,
        vec![(
            IVec3::new(0, 0, 0),
            GeometryCell::refined(RefinedVolume::decode(&stream).unwrap()),
        )],
    );
    let mut server = AuthoritativePlayer::new(FixedMicrometers3 {
        x: 500_000,
        y: 1_000_000,
        z: 500_000,
    });
    let initial = ReplicatedPlayerState::from_authoritative(7, server.state());
    // Exhaust partway through the finely partitioned support scan, not just on a cell preflight.
    let before = server;
    assert_eq!(
        server.step_with_limits(
            geometry.world(),
            QueryLimits {
                cells: 128,
                leaf_visits: 32,
            },
        ),
        Err(GeometryQueryError::LeafBudget)
    );
    assert_eq!(server, before);
    let mut prediction = ClientPrediction::new(1, initial).unwrap();
    let mut total_work = 0;
    for sequence in 1..=crate::prediction::MAX_PENDING_PREDICTED_INPUTS {
        let input = PlayerInputCommand {
            input_sequence: u64::try_from(sequence).unwrap(),
            ..PlayerInputCommand::default()
        };
        server.accept_input(input).unwrap();
        let authority_work = server.step(geometry.world()).unwrap();
        let predicted = prediction.predict(input, geometry.world()).unwrap();
        assert_eq!(predicted.simulation, authority_work);
        total_work += authority_work.geometry.leaf_visits;
    }
    assert!(
        total_work > crate::world::query::MAX_QUERY_LEAF_VISITS,
        "a lower shared replay allowance would incorrectly reject valid server ticks"
    );
    let replay = prediction.reconcile(2, initial, geometry.world()).unwrap();
    assert_eq!(
        replay.replayed_inputs,
        crate::prediction::MAX_PENDING_PREDICTED_INPUTS
    );
    assert_eq!(prediction.state(), server.state());
}

#[test]
fn uniform_character_queries_fit_regular_budget_at_replicated_velocity_extremes() {
    let world = World::default();
    let speed = crate::player_replication::MAX_REPLICATED_PLAYER_VELOCITY_UM_PER_SECOND;
    // Uniform scans charge one visit per candidate cell. Empty space preserves the full sweep;
    // collisions only shorten later sweeps. Exercise page-boundary phases and signed remainders.
    for x in [-1, 0, 1, 500_000, 999_999] {
        for y in [-1, 0, 1, 500_000, 999_999] {
            for z in [-1, 0, 1, 500_000, 999_999] {
                for signs in 0..8 {
                    let velocities: [i64; 3] = std::array::from_fn(|axis| {
                        if signs & (1 << axis) == 0 {
                            speed
                        } else {
                            -speed
                        }
                    });
                    let mut player = AuthoritativePlayer::new(FixedMicrometers3 {
                        x,
                        y: y + 2 * MICROMETERS_PER_VOXEL,
                        z,
                    });
                    player.state.velocity_um_per_second = FixedMicrometers3 {
                        x: velocities[0],
                        y: velocities[1],
                        z: velocities[2],
                    };
                    player.state.integration_remainder =
                        velocities.map(|value| value.signum() * 59);
                    let report = player.step(&world).unwrap();
                    assert_eq!(report.geometry.cells, report.geometry.leaf_visits);
                    assert!(report.geometry.cells <= QueryLimits::default().cells);
                }
            }
        }
    }
}
