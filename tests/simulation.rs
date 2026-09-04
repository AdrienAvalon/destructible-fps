use destructible_fps::{
    AuthoritativeServer, ClientReplica, ClientStatus, CodecError, DeltaPacket, Explosion,
    ExplosionCommand, FrameAssembler, IVec3, Material, ReplicationError, Voxel, VoxelChange, World,
    WorldError, decode_frame, encode_frames,
};
use std::net::UdpSocket;
use std::time::Duration;

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
    assert_eq!(
        client.world().stats().solid_voxels,
        server.world().stats().solid_voxels
    );
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
        changes: Vec::new(),
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
            changes: changes.clone(),
        };
        let first = encode_frames(&packet, 76)
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
        changes,
    };
    let first = encode_frames(&packet, 76)
        .expect("one change per frame")
        .remove(0);
    assert_eq!(
        assembler.push(decode_frame(&first).expect("valid frame")),
        Err(CodecError::TooManyPendingPackets)
    );
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
}
