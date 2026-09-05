use destructible_fps::{
    AuthoritativeServer, BodyError, BodyLimits, ClientReplica, ClientStatus, CodecError,
    CommandError, DeltaPacket, DemoSession, Explosion, ExplosionCommand, FireMode, FrameAssembler,
    IVec3, MICROMETERS_PER_VOXEL, Material, ReplicationError, StructuralAnchors, StructuralLimits,
    Voxel, VoxelChange, World, WorldError, decode_frame, encode_frames,
};
use glam::Vec3;
use std::net::UdpSocket;
use std::time::Duration;

fn fragile_column() -> World {
    let mut world = World::default();
    world.set_voxel(IVec3::new(0, 0, 0), Voxel::new(Material::Steel));
    world.set_voxel(IVec3::new(0, 1, 0), Voxel::new(Material::Glass));
    world.fill_box(
        IVec3::new(0, 2, 0),
        IVec3::new(0, 3, 0),
        Voxel::new(Material::Wood),
    );
    world
}

const fn sever_column_command() -> ExplosionCommand {
    ExplosionCommand {
        command_id: 1,
        center: IVec3::new(0, 1, 0),
        radius_voxels: 1,
        peak_energy: 600,
    }
}

#[test]
fn blast_response_depends_on_material_properties() {
    let mut world = World::default();
    let glass = IVec3::new(-1, 0, 0);
    let steel = IVec3::new(1, 0, 0);
    world.set_voxel(glass, Voxel::new(Material::Glass));
    world.set_voxel(steel, Voxel::new(Material::Steel));

    let report = world.apply_explosion(Explosion {
        center: IVec3::new(0, 0, 0),
        radius_voxels: 3,
        peak_energy: 2_000,
    });

    assert_eq!(world.voxel(glass), Voxel::AIR);
    assert!(world.voxel(steel).is_solid());
    assert_eq!(report.fractured_voxels, 1);
    assert_eq!(world.fingerprint(), world.recompute_fingerprint());
}

#[test]
fn failed_transaction_is_atomic() {
    let mut world = World::default();
    let position = IVec3::new(1, 2, 3);
    let before = Voxel::new(Material::Concrete);
    world.set_voxel(position, before);
    let fingerprint = world.fingerprint();
    let change = VoxelChange {
        position,
        before,
        after: Voxel::AIR,
    };

    let error = world
        .apply_checked(&[change], 123)
        .expect_err("incorrect final fingerprint must fail");
    assert!(matches!(error, WorldError::FinalFingerprintMismatch { .. }));
    assert_eq!(world.voxel(position), before);
    assert_eq!(world.fingerprint(), fingerprint);
}

#[test]
fn fragmented_out_of_order_delta_keeps_replicas_identical() {
    let initial = destructible_fps::demo_world();
    let mut server = AuthoritativeServer::new(initial.clone());
    let mut client = ClientReplica::new(initial);
    let (packet, _) = server
        .execute_explosion(
            7,
            ExplosionCommand {
                command_id: 1,
                center: IVec3::new(-20, 6, 0),
                radius_voxels: 8,
                peak_energy: 30_000,
            },
        )
        .expect("valid command");
    let frames = encode_frames(&packet, 220).expect("valid MTU");
    assert!(frames.len() > 1);

    let mut assembler = FrameAssembler::default();
    let mut complete = None;
    for bytes in frames.into_iter().rev() {
        let frame = decode_frame(&bytes).expect("valid frame");
        if let Some(packet) = assembler.push(frame).expect("consistent fragment") {
            complete = Some(packet);
        }
    }
    let complete = complete.expect("all fragments were supplied");
    assert_eq!(client.receive(&complete), Ok(ClientStatus::Applied));
    assert_eq!(client.world().fingerprint(), server.world().fingerprint());
    assert_eq!(client.body_fingerprint(), server.body_fingerprint());
    assert_eq!(client.bodies(), server.bodies());
    assert_eq!(
        client.world().stats().solid_voxels,
        server.world().stats().solid_voxels
    );
}

#[test]
fn structural_detachment_is_one_replicated_authoritative_transaction() {
    let initial = fragile_column();
    let mut server = AuthoritativeServer::new(initial.clone());
    let mut client = ClientReplica::new(initial);

    let (packet, report) = server
        .execute_explosion(7, sever_column_command())
        .expect("connector blast must detach the upper column");
    assert_eq!(report.detached_voxels, 2);
    assert_eq!(packet.body_assignments.len(), 2);
    assert_eq!(server.bodies().len(), 1);
    assert_eq!(server.world().voxel(IVec3::new(0, 2, 0)), Voxel::AIR);
    assert_eq!(server.world().voxel(IVec3::new(0, 3, 0)), Voxel::AIR);
    assert!(packet.body_assignments[0].voxel.integrity < u8::MAX);

    let mut assembler = FrameAssembler::default();
    let mut complete = None;
    for bytes in encode_frames(&packet, 126)
        .expect("minimum body-assignment MTU")
        .into_iter()
        .rev()
    {
        if let Some(delta) = assembler
            .push(decode_frame(&bytes).expect("valid body frame"))
            .expect("consistent body transaction")
        {
            complete = Some(delta);
        }
    }
    assert_eq!(
        client.receive(&complete.expect("all body fragments arrived")),
        Ok(ClientStatus::Applied)
    );
    assert_eq!(client.world().fingerprint(), server.world().fingerprint());
    assert_eq!(client.body_fingerprint(), server.body_fingerprint());
    assert_eq!(client.bodies(), server.bodies());
}

#[test]
fn failed_body_promotion_rolls_back_the_complete_server_transaction() {
    let initial = fragile_column();
    let initial_fingerprint = initial.fingerprint();
    let mut server = AuthoritativeServer::new(initial.clone()).with_structural_config(
        StructuralAnchors::foundation_plane(0),
        StructuralLimits::default(),
        BodyLimits { max_voxels: 1 },
    );

    let error = server
        .execute_explosion(7, sever_column_command())
        .expect_err("two-voxel body exceeds the configured promotion limit");
    assert_eq!(error, CommandError::Body(BodyError::TooManyVoxels(2)));
    assert_eq!(server.world().fingerprint(), initial_fingerprint);
    assert_eq!(
        server.world().voxel(IVec3::new(0, 1, 0)),
        initial.voxel(IVec3::new(0, 1, 0))
    );
    assert_eq!(
        server.world().voxel(IVec3::new(0, 2, 0)),
        initial.voxel(IVec3::new(0, 2, 0))
    );
    assert!(server.bodies().is_empty());
    assert_eq!(server.body_fingerprint(), 0);
    assert_eq!(server.world().tick(), 0);
    assert_eq!(
        server.execute_explosion(7, sever_column_command()),
        Err(CommandError::Body(BodyError::TooManyVoxels(2)))
    );
}

#[test]
fn corrupted_body_assignment_is_rejected_before_client_mutation() {
    let initial = fragile_column();
    let initial_fingerprint = initial.fingerprint();
    let mut server = AuthoritativeServer::new(initial.clone());
    let mut client = ClientReplica::new(initial);
    let (mut packet, _) = server
        .execute_explosion(7, sever_column_command())
        .expect("valid body transaction");
    packet.body_assignments[0].voxel.integrity =
        packet.body_assignments[0].voxel.integrity.saturating_sub(1);

    assert!(matches!(
        client.receive(&packet),
        Err(ReplicationError::Body(BodyError::IdentifierMismatch { .. }))
    ));
    assert_eq!(client.world().fingerprint(), initial_fingerprint);
    assert!(client.bodies().is_empty());
    assert_eq!(client.body_fingerprint(), 0);
}

#[test]
fn authoritative_body_motion_replicates_until_static_sleep() {
    let mut world = World::default();
    world.fill_box(
        IVec3::new(-2, 0, -2),
        IVec3::new(2, 0, 2),
        Voxel::new(Material::Stone),
    );
    world.set_voxel(IVec3::new(0, 1, 0), Voxel::new(Material::Glass));
    world.fill_box(
        IVec3::new(0, 2, 0),
        IVec3::new(0, 4, 0),
        Voxel::new(Material::Wood),
    );
    let mut session = DemoSession::new(world);
    let shot = session
        .fire(Vec3::new(0.5, 1.5, 4.0), -Vec3::Z, FireMode::Rifle)
        .expect("authoritative shot")
        .expect("support is visible");
    assert_eq!(shot.spawned_body_ids.len(), 1);
    let body_id = shot.spawned_body_ids[0];

    for _ in 0..240 {
        session.advance_physics().expect("physics tick replicates");
    }
    let state = session.body_states()[&body_id];
    assert!(state.sleeping);
    assert_eq!(state.translation_um.y, MICROMETERS_PER_VOXEL);
    assert_eq!(state.linear_velocity_um_per_second.y, 0);
}

#[test]
fn corrupted_body_motion_is_rejected_before_replica_state_changes() {
    let initial = fragile_column();
    let mut server = AuthoritativeServer::new(initial.clone());
    let mut client = ClientReplica::new(initial);
    let (spawn, _) = server
        .execute_explosion(7, sever_column_command())
        .expect("body spawn");
    client.receive(&spawn).expect("spawn applies");
    let before = client.body_states().clone();
    let (packet, _) = server.advance_physics();
    let mut packet = packet.expect("falling body changes on first physics tick");
    let frames = encode_frames(&packet, 166).expect("one minimum-size body-state frame");
    assert_eq!(frames.len(), 1);
    assert_eq!(frames[0].len(), 166);
    let mut invalid_wire_state = frames[0].clone();
    *invalid_wire_state.last_mut().expect("sleeping flag") = 2;
    assert_eq!(
        decode_frame(&invalid_wire_state),
        Err(CodecError::InvalidBodyState)
    );
    packet.body_updates[0].state.translation_um.y -= 1;

    assert!(matches!(
        client.receive(&packet),
        Err(ReplicationError::BodyFingerprintMismatch { .. })
    ));
    assert_eq!(client.body_states(), &before);
    assert_eq!(client.world().tick(), spawn.tick);
}

#[test]
fn sequence_gap_is_detected_before_world_mutation() {
    let initial = World::default();
    let mut server = AuthoritativeServer::new(initial.clone());
    let mut client = ClientReplica::new(initial);
    let (first, _) = server
        .execute_explosion(
            1,
            ExplosionCommand {
                command_id: 1,
                center: IVec3::new(0, 0, 0),
                radius_voxels: 2,
                peak_energy: 100,
            },
        )
        .expect("valid command");
    let (second, _) = server
        .execute_explosion(
            1,
            ExplosionCommand {
                command_id: 2,
                center: IVec3::new(0, 0, 0),
                radius_voxels: 2,
                peak_energy: 100,
            },
        )
        .expect("valid command");

    let error = client.receive(&second).expect_err("packet one is missing");
    assert_eq!(
        error,
        ReplicationError::SequenceGap {
            expected: 1,
            received: 2
        }
    );
    assert_eq!(client.receive(&first), Ok(ClientStatus::Applied));
}

#[test]
fn codec_rejects_truncated_and_corrupted_frames() {
    let packet = DeltaPacket {
        sequence: 1,
        tick: 1,
        base_fingerprint: 0,
        final_fingerprint: 0,
        base_body_fingerprint: 0,
        final_body_fingerprint: 0,
        changes: Vec::new(),
        body_assignments: Vec::new(),
        body_updates: Vec::new(),
    };
    let frames = encode_frames(&packet, 1_200).expect("empty delta still has one frame");
    assert!(decode_frame(&frames[0][..10]).is_err());
    let mut corrupted = frames[0].clone();
    corrupted[0] = b'X';
    assert!(decode_frame(&corrupted).is_err());
}

#[test]
fn server_rejects_replayed_client_commands() {
    let mut server = AuthoritativeServer::new(World::default());
    let command = ExplosionCommand {
        command_id: 42,
        center: IVec3::new(0, 0, 0),
        radius_voxels: 2,
        peak_energy: 1_000,
    };
    server
        .execute_explosion(9, command)
        .expect("first command is fresh");
    assert!(server.execute_explosion(9, command).is_err());
}

#[test]
fn incomplete_packet_flood_is_bounded() {
    let changes = vec![
        VoxelChange {
            position: IVec3::new(0, 0, 0),
            before: Voxel::AIR,
            after: Voxel::new(Material::Wood),
        },
        VoxelChange {
            position: IVec3::new(1, 0, 0),
            before: Voxel::AIR,
            after: Voxel::new(Material::Wood),
        },
    ];
    let mut assembler = FrameAssembler::default();
    for sequence in 1..=64 {
        let packet = DeltaPacket {
            sequence,
            tick: sequence,
            base_fingerprint: 0,
            final_fingerprint: 1,
            base_body_fingerprint: 0,
            final_body_fingerprint: 0,
            changes: changes.clone(),
            body_assignments: Vec::new(),
            body_updates: Vec::new(),
        };
        let first = encode_frames(&packet, 112)
            .expect("one change per frame")
            .remove(0);
        assert!(
            assembler
                .push(decode_frame(&first).expect("valid frame"))
                .expect("within pending bound")
                .is_none()
        );
    }
    let packet = DeltaPacket {
        sequence: 65,
        tick: 65,
        base_fingerprint: 0,
        final_fingerprint: 1,
        base_body_fingerprint: 0,
        final_body_fingerprint: 0,
        changes,
        body_assignments: Vec::new(),
        body_updates: Vec::new(),
    };
    let first = encode_frames(&packet, 112)
        .expect("one change per frame")
        .remove(0);
    assert_eq!(
        assembler.push(decode_frame(&first).expect("valid frame")),
        Err(CodecError::TooManyPendingPackets)
    );
}

#[test]
fn fragment_memory_accounting_is_stable_and_released() {
    let changes = vec![
        VoxelChange {
            position: IVec3::new(0, 0, 0),
            before: Voxel::AIR,
            after: Voxel::new(Material::Wood),
        },
        VoxelChange {
            position: IVec3::new(1, 0, 0),
            before: Voxel::AIR,
            after: Voxel::new(Material::Wood),
        },
    ];
    let packet = DeltaPacket {
        sequence: 1,
        tick: 1,
        base_fingerprint: 0,
        final_fingerprint: 1,
        base_body_fingerprint: 0,
        final_body_fingerprint: 0,
        changes,
        body_assignments: Vec::new(),
        body_updates: Vec::new(),
    };
    let frames = encode_frames(&packet, 112).expect("one change per frame");
    let first = decode_frame(&frames[0]).expect("valid first frame");
    let second = decode_frame(&frames[1]).expect("valid second frame");
    let mut assembler = FrameAssembler::default();

    assert!(
        assembler
            .push(first.clone())
            .expect("new fragment")
            .is_none()
    );
    let retained = assembler.pending_bytes();
    assert_eq!(retained, frames[0].len());
    assert!(
        assembler
            .push(first)
            .expect("identical duplicate")
            .is_none()
    );
    assert_eq!(assembler.pending_bytes(), retained);
    assert!(assembler.push(second).expect("complete packet").is_some());
    assert_eq!(assembler.pending_bytes(), 0);
    assert_eq!(assembler.pending_packets(), 0);

    let mut inconsistent = decode_frame(&frames[0]).expect("valid first frame");
    assert!(
        assembler
            .push(inconsistent.clone())
            .expect("new packet")
            .is_none()
    );
    inconsistent.tick += 1;
    assert_eq!(
        assembler.push(inconsistent),
        Err(CodecError::InconsistentFragment)
    );
    assert_eq!(assembler.pending_bytes(), 0);
    assert_eq!(assembler.pending_packets(), 0);
}

#[test]
fn long_authoritative_session_never_diverges() {
    let initial = destructible_fps::demo_world();
    let mut server = AuthoritativeServer::new(initial.clone());
    let mut client = ClientReplica::new(initial);
    let mut assembler = FrameAssembler::default();

    for command_id in 1..=2_000_u64 {
        let coordinate = i32::try_from(command_id % 31).expect("small modulo") - 15;
        let (packet, _) = server
            .execute_explosion(
                11,
                ExplosionCommand {
                    command_id,
                    center: IVec3::new(coordinate, 4, -coordinate),
                    radius_voxels: 4,
                    peak_energy: 8_000,
                },
            )
            .expect("monotonic bounded command");
        let mut frames = encode_frames(&packet, 256).expect("valid MTU");
        frames.reverse();
        for bytes in frames {
            if let Some(delta) = assembler
                .push(decode_frame(&bytes).expect("valid frame"))
                .expect("consistent packet")
            {
                client.receive(&delta).expect("next authoritative delta");
            }
        }
    }

    assert_eq!(client.world().fingerprint(), server.world().fingerprint());
    assert_eq!(client.body_fingerprint(), server.body_fingerprint());
    assert_eq!(client.bodies(), server.bodies());
    assert_eq!(
        server.world().fingerprint(),
        server.world().recompute_fingerprint()
    );
}

#[test]
fn udp_loopback_carries_a_fragmented_authoritative_delta() {
    let initial = destructible_fps::demo_world();
    let mut server = AuthoritativeServer::new(initial.clone());
    let mut client = ClientReplica::new(initial);
    let (packet, _) = server
        .execute_explosion(
            21,
            ExplosionCommand {
                command_id: 1,
                center: IVec3::new(20, 7, 0),
                radius_voxels: 9,
                peak_energy: 35_000,
            },
        )
        .expect("valid command");
    let mut frames = encode_frames(&packet, 1_200).expect("valid datagram MTU");
    assert!(frames.len() > 1);
    frames.reverse();

    let sender = UdpSocket::bind("127.0.0.1:0").expect("loopback sender");
    let receiver = UdpSocket::bind("127.0.0.1:0").expect("loopback receiver");
    receiver
        .set_read_timeout(Some(Duration::from_secs(1)))
        .expect("read timeout");
    let destination = receiver.local_addr().expect("receiver address");
    let expected_source = sender.local_addr().expect("sender address");
    for frame in &frames {
        assert_eq!(
            sender.send_to(frame, destination).expect("loopback send"),
            frame.len()
        );
    }

    let mut assembler = FrameAssembler::default();
    let mut complete = None;
    for _ in 0..frames.len() {
        let mut datagram = [0_u8; 1_200];
        let (length, source) = receiver.recv_from(&mut datagram).expect("loopback receive");
        assert_eq!(source, expected_source);
        if let Some(delta) = assembler
            .push(decode_frame(&datagram[..length]).expect("valid datagram"))
            .expect("consistent fragments")
        {
            complete = Some(delta);
        }
    }
    client
        .receive(&complete.expect("every datagram arrived"))
        .expect("authoritative delta applies");
    assert_eq!(client.world().fingerprint(), server.world().fingerprint());
    assert_eq!(client.body_fingerprint(), server.body_fingerprint());
    assert_eq!(client.bodies(), server.bodies());
}
