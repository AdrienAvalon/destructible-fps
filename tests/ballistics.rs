use destructible_fps::{
    AuthoritativeServer, BodyLimits, ClientReplica, CommandError, FixedMicrometers3,
    FrameAssembler, IVec3, Material, PlayerBuildContext, StructuralAnchors, StructuralLimits,
    Voxel, World,
    ballistics::{
        BallisticError, RIFLE_CADENCE_TICKS, RIFLE_MAGAZINE, RIFLE_RELOAD_TICKS, RifleCommand,
    },
    decode_frame, encode_frames,
};

const fn player() -> PlayerBuildContext {
    PlayerBuildContext {
        eye_position_um: FixedMicrometers3 {
            x: -1_500_000,
            y: 2_650_000,
            z: 500_000,
        },
        bounds_minimum_um: FixedMicrometers3 {
            x: -1_800_000,
            y: 1_000_000,
            z: 200_000,
        },
        bounds_maximum_um: FixedMicrometers3 {
            x: -1_200_000,
            y: 2_800_000,
            z: 800_000,
        },
    }
}

const fn command(command_id: u64) -> RifleCommand {
    RifleCommand {
        command_id,
        direction: [1, 0, 0],
    }
}

fn apply_packet(replica: &mut ClientReplica, packet: &destructible_fps::DeltaPacket, mtu: usize) {
    let mut assembler = FrameAssembler::default();
    let mut complete = None;
    for frame in encode_frames(packet, mtu).unwrap().into_iter().rev() {
        assert!(frame.len() <= mtu);
        complete = assembler
            .push(decode_frame(&frame).unwrap())
            .unwrap()
            .or(complete);
    }
    replica.receive(&complete.unwrap()).unwrap();
}

#[test]
fn repeated_directional_hits_penetrate_layers_and_replicas_without_radial_neighbors() {
    let mut world = World::default();
    world.fill_box(
        IVec3::new(-4, 0, -4),
        IVec3::new(4, 0, 4),
        Voxel::new(Material::Stone),
    );
    world.fill_box(
        IVec3::new(0, 1, 0),
        IVec3::new(1, 2, 1),
        Voxel::new(Material::Wood),
    );
    let mut replicas = [
        ClientReplica::new(world.clone()),
        ClientReplica::new(world.clone()),
    ];
    let mut authority = AuthoritativeServer::new(world);
    let first = IVec3::new(0, 2, 0);
    let behind = IVec3::new(1, 2, 0);
    for id in 1..=3 {
        let (packet, report) = authority
            .execute_rifle(7, command(id), player(), id * RIFLE_CADENCE_TICKS)
            .unwrap();
        assert_eq!(report.first_hit, Some(first));
        assert_eq!(report.spent_energy + report.remaining_energy, 750);
        assert_eq!(authority.world().voxel(IVec3::new(0, 2, 1)).integrity, 255);
        assert_eq!(authority.world().voxel(first).is_solid(), id < 3);
        if id < 3 {
            assert_eq!(authority.world().voxel(behind).integrity, 255);
        }
        for (replica, mtu) in replicas.iter_mut().zip([1100, 1200]) {
            apply_packet(replica, &packet, mtu);
            assert_eq!(
                replica.world().occupied_voxels(),
                authority.world().occupied_voxels()
            );
        }
    }
    assert!(authority.world().voxel(behind).integrity < 255);
    assert_eq!(authority.rifle_state(7, 18).magazine, RIFLE_MAGAZINE - 3);
}

#[test]
fn invalid_and_replayed_shots_cannot_spend_ammo_or_advance_a_transaction() {
    let mut authority = AuthoritativeServer::new(World::default());
    let initial = authority.rifle_state(7, 1);
    for bad in [
        RifleCommand {
            command_id: 0,
            direction: [1, 0, 0],
        },
        RifleCommand {
            command_id: 1,
            direction: [0; 3],
        },
    ] {
        assert!(authority.execute_rifle(7, bad, player(), 1).is_err());
        assert_eq!(authority.world().tick(), 0);
        assert_eq!(authority.rifle_state(7, 1), initial);
    }
    let (packet, miss) = authority.execute_rifle(7, command(1), player(), 1).unwrap();
    assert!(miss.first_hit.is_none());
    assert!(packet.changes.is_empty());
    assert_eq!(packet.sequence, 1);
    assert_eq!(authority.rifle_state(7, 1).magazine, 29);
    let before = authority.rifle_state(7, 1);
    assert!(matches!(
        authority.execute_rifle(7, command(1), player(), 7),
        Err(CommandError::ReplayedCommand { .. })
    ));
    assert_eq!(
        authority
            .execute_rifle(7, command(2), player(), 1)
            .unwrap_err(),
        CommandError::Ballistic(BallisticError::FireTooSoon)
    );
    assert_eq!(authority.rifle_state(7, 1), before);
    assert_eq!(authority.world().tick(), 1);
    assert_eq!(authority.last_command_id(7), 1);
}

#[test]
fn failed_body_promotion_rolls_back_integrity_ammo_and_command_watermarks() {
    let mut world = World::default();
    world.fill_box(
        IVec3::new(0, 0, 0),
        IVec3::new(0, 1, 0),
        Voxel::new(Material::Stone),
    );
    world.set_voxel(IVec3::new(0, 2, 0), Voxel::new(Material::Glass));
    world.fill_box(
        IVec3::new(0, 3, 0),
        IVec3::new(0, 5, 0),
        Voxel::new(Material::Wood),
    );
    let initial = world.occupied_voxels();
    let mut authority = AuthoritativeServer::new(world).with_structural_config(
        StructuralAnchors::foundation_plane(0),
        StructuralLimits::default(),
        BodyLimits { max_voxels: 1 },
    );
    assert!(authority.execute_rifle(7, command(1), player(), 1).is_err());
    assert_eq!(authority.world().occupied_voxels(), initial);
    assert_eq!(
        authority.world().fingerprint(),
        authority.world().recompute_fingerprint()
    );
    assert_eq!(authority.world().tick(), 0);
    assert_eq!(authority.next_body_id(), 1);
    assert_eq!(authority.last_command_id(7), 0);
    assert_eq!(authority.rifle_state(7, 1).magazine, RIFLE_MAGAZINE);
    assert!(authority.bodies().is_empty());
}

#[test]
fn depletion_and_reload_have_finite_reserves_and_exact_tick_boundaries() {
    let mut authority = AuthoritativeServer::new(World::default());
    for id in 1..=30 {
        authority
            .execute_rifle(7, command(id), player(), id * 6)
            .unwrap();
    }
    assert_eq!(
        authority
            .execute_rifle(7, command(31), player(), 186)
            .unwrap_err(),
        CommandError::Ballistic(BallisticError::EmptyMagazine)
    );
    let reload = authority.execute_reload(7, 31, 186).unwrap();
    assert!(reload.changes.is_empty());
    assert_eq!(
        authority
            .execute_rifle(7, command(32), player(), 186 + RIFLE_RELOAD_TICKS - 1)
            .unwrap_err(),
        CommandError::Ballistic(BallisticError::Reloading)
    );
    authority
        .execute_rifle(7, command(32), player(), 186 + RIFLE_RELOAD_TICKS)
        .unwrap();
    assert_eq!(authority.rifle_state(7, 306).magazine, 29);
    assert_eq!(authority.rifle_state(7, 306).reserve, 60);
    authority.release_client(7);
    assert_eq!(authority.rifle_state(7, 307).reserve, 90);
    assert_eq!(authority.last_command_id(7), 0);
}

#[test]
fn severed_bodies_occlude_without_a_fake_blast_and_static_cover_in_front_still_wins() {
    let mut world = World::default();
    world.fill_box(
        IVec3::new(0, 0, 0),
        IVec3::new(0, 1, 0),
        Voxel::new(Material::Stone),
    );
    world.set_voxel(IVec3::new(0, 2, 0), Voxel::new(Material::Glass));
    world.fill_box(
        IVec3::new(0, 3, 0),
        IVec3::new(0, 5, 0),
        Voxel::new(Material::Wood),
    );
    world.fill_box(
        IVec3::new(2, 0, 0),
        IVec3::new(2, 5, 0),
        Voxel::new(Material::Wood),
    );
    let mut authority = AuthoritativeServer::new(world);
    let (packet, _) = authority.execute_rifle(7, command(1), player(), 1).unwrap();
    assert_eq!(authority.bodies().len(), 1);
    assert!(packet.body_updates.is_empty());
    let body = authority.bodies().values().next().unwrap().clone();
    assert_eq!(
        authority.body_states()[&body.id],
        destructible_fps::RigidBodyState::at_spawn(&body)
    );
    let mut elevated = player();
    elevated.eye_position_um.y += 1_500_000;
    elevated.bounds_minimum_um.y += 1_500_000;
    elevated.bounds_maximum_um.y += 1_500_000;
    let before = authority.world().fingerprint();
    let (blocked_packet, blocked) = authority.execute_rifle(7, command(2), elevated, 7).unwrap();
    assert_eq!(blocked.blocked_by_body, Some(body.id));
    assert!(blocked_packet.changes.is_empty());
    assert_eq!(authority.world().fingerprint(), before);
    elevated.eye_position_um.x += 6_000_000;
    elevated.bounds_minimum_um.x += 6_000_000;
    elevated.bounds_maximum_um.x += 6_000_000;
    let (_, front) = authority
        .execute_rifle(
            7,
            RifleCommand {
                command_id: 3,
                direction: [-1, 0, 0],
            },
            elevated,
            13,
        )
        .unwrap();
    assert_eq!(front.first_hit, Some(IVec3::new(2, 4, 0)));
    assert!(front.blocked_by_body.is_none());
    assert!(authority.world().voxel(IVec3::new(2, 4, 0)).integrity < 255);
}
