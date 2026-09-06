use destructible_fps::{
    AuthenticatedPrincipal, AuthorityCore, ClientReplica, FrameAssembler, IVec3, Material,
    NetworkTickReport, Voxel, World,
    ballistics::{RIFLE_MAGAZINE, RifleCommand, RifleState, state_wire::WeaponStatePacket},
    decode_frame, is_delta_datagram,
    transport::{encode_reload_request, encode_rifle_request},
};

#[test]
fn server_eye_cadence_reload_and_personal_weapon_state_cross_the_bounded_core() {
    let mut world = World::default();
    world.fill_box(
        IVec3::new(-4, 0, 30),
        IVec3::new(4, 0, 44),
        Voxel::new(Material::Stone),
    );
    world.fill_box(
        IVec3::new(-2, 1, 38),
        IVec3::new(2, 3, 38),
        Voxel::new(Material::Wood),
    );
    let mut replicas = [
        ClientReplica::new(world.clone()),
        ClientReplica::new(world.clone()),
    ];
    let mut core = AuthorityCore::new(world, 1100).unwrap();
    let mut first = core.begin_tick();
    let principal = AuthenticatedPrincipal::new(std::num::NonZeroU64::new(1).unwrap());
    assert!(core.admit_authenticated(1_u8, 11, 19, principal, &mut first));
    assert!(core.admit_authenticated(2_u8, 12, 20, principal, &mut first));
    for command_id in 1..=32 {
        core.ingest_datagram(
            1,
            &encode_rifle_request(
                19,
                RifleCommand {
                    command_id,
                    direction: [0, 0, -32767],
                },
            ),
            &mut |_, _| true,
            &mut first,
        );
    }
    core.ingest_datagram(
        2,
        &encode_reload_request(19, 33),
        &mut |_, _| true,
        &mut first,
    );
    let mut frames = Vec::new();
    let first = core
        .complete_tick(
            &mut |target, bytes| {
                frames.push((target, bytes.to_vec()));
                true
            },
            first,
        )
        .unwrap();
    assert_eq!(first.commands_applied, 1);
    assert_eq!(first.commands_rejected, 31);
    assert_eq!(first.rejected_sessions, 1);
    assert_eq!(
        core.authority().rifle_state(19, 1).magazine,
        RIFLE_MAGAZINE - 1
    );
    assert_eq!(
        core.authority()
            .world()
            .voxel(IVec3::new(0, 2, 38))
            .integrity,
        162
    );
    assert_eq!(
        core.authority()
            .world()
            .voxel(IVec3::new(1, 2, 38))
            .integrity,
        255
    );
    replicate(&frames, &mut replicas);

    reload_and_verify(&mut core, &mut replicas);
}

fn reload_and_verify(core: &mut AuthorityCore<u8>, replicas: &mut [ClientReplica; 2]) {
    let mut frames = Vec::new();
    let mut second = core.begin_tick();
    core.ingest_datagram(
        1,
        &encode_reload_request(19, 33),
        &mut |_, _| true,
        &mut second,
    );
    core.ingest_datagram(
        1,
        &encode_rifle_request(
            19,
            RifleCommand {
                command_id: 34,
                direction: [0, 0, -1],
            },
        ),
        &mut |_, _| true,
        &mut second,
    );
    frames.clear();
    let second = core
        .complete_tick(
            &mut |target, bytes| {
                frames.push((target, bytes.to_vec()));
                true
            },
            second,
        )
        .unwrap();
    assert_eq!(second.commands_applied, 1);
    assert_eq!(second.commands_rejected, 1);
    replicate(&frames, replicas);
    let mut last_weapon = [None, None];
    for _ in 3..=122 {
        let tick = core.begin_tick();
        core.complete_tick(
            &mut |target, bytes| {
                if let Some(packet) = WeaponStatePacket::decode(bytes) {
                    assert_eq!(packet.session_id, if target == 1 { 19 } else { 20 });
                    last_weapon[usize::from(target - 1)] = Some(packet);
                }
                true
            },
            tick,
        )
        .unwrap();
    }
    assert_eq!(last_weapon[0].unwrap().last_command_id, 33);
    assert_eq!(last_weapon[1].unwrap().rifle.magazine, 30);
    assert_eq!(core.authority().rifle_state(19, 122).magazine, 30);
    assert_eq!(core.authority().rifle_state(19, 122).reserve, 89);
    for replica in replicas {
        assert_eq!(
            replica.world().fingerprint(),
            core.authority().world().fingerprint()
        );
    }
}

fn replicate(frames: &[(u8, Vec<u8>)], replicas: &mut [ClientReplica; 2]) {
    for (index, replica) in replicas.iter_mut().enumerate() {
        let mut assembler = FrameAssembler::default();
        for (_, bytes) in frames
            .iter()
            .rev()
            .filter(|(target, bytes)| usize::from(*target - 1) == index && is_delta_datagram(bytes))
        {
            if let Some(packet) = assembler.push(decode_frame(bytes).unwrap()).unwrap() {
                replica.receive(&packet).unwrap();
            }
        }
    }
}

#[test]
fn forged_weapon_status_and_arbitrary_rifle_bytes_never_mutate_before_validation() {
    let mut core = AuthorityCore::new(World::default(), 1100).unwrap();
    let mut report = core.begin_tick();
    let principal = AuthenticatedPrincipal::new(std::num::NonZeroU64::new(1).unwrap());
    assert!(core.admit_authenticated(1_u8, 11, 19, principal, &mut report));
    let valid = encode_rifle_request(
        19,
        RifleCommand {
            command_id: 1,
            direction: [1, 0, 0],
        },
    );
    for length in 0..valid.len() {
        core.ingest_datagram(1, &valid[..length], &mut |_, _| true, &mut report);
    }
    let forged = WeaponStatePacket {
        session_id: 19,
        server_tick: 1,
        last_command_id: 999,
        rifle: RifleState::default(),
    }
    .encode()
    .unwrap();
    core.ingest_datagram(1, &forged, &mut |_, _| true, &mut report);
    core.ingest_datagram(
        1,
        &encode_rifle_request(
            19,
            RifleCommand {
                command_id: 1,
                direction: [0; 3],
            },
        ),
        &mut |_, _| true,
        &mut report,
    );
    let final_report: NetworkTickReport = core.complete_tick(&mut |_, _| true, report).unwrap();
    assert_eq!(final_report.malformed_datagrams, valid.len() + 1);
    assert_eq!(final_report.commands_rejected, 1);
    assert_eq!(final_report.commands_applied, 0);
    assert_eq!(core.authority().last_command_id(19), 0);
    assert_eq!(core.authority().rifle_state(19, 1).magazine, 30);
}
