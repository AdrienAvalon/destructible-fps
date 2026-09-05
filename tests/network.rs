use destructible_fps::{
    AuthoritativeServer, ClientReplica, ExplosionCommand, IVec3, OrderedDeltaInbox,
    ServerControlMessage, SnapshotAssembler, World, decode_frame, decode_server_control,
    demo_world, encode_client_hello, encode_explosion_request, encode_repair_request,
    encode_snapshot_request, is_delta_datagram, is_snapshot_datagram,
};
use std::{
    io,
    net::{SocketAddr, UdpSocket},
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};

struct ChildGuard(Child);

impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

struct Endpoint {
    socket: UdpSocket,
    inbox: OrderedDeltaInbox,
    replica: ClientReplica,
    applied: usize,
    drop_sequence: Option<u64>,
    dropped_frames: usize,
}

#[test]
fn dedicated_process_synchronizes_two_real_udp_clients() {
    let reservation = UdpSocket::bind("127.0.0.1:0").expect("reserve loopback port");
    let server_address = reservation.local_addr().expect("reserved address");
    drop(reservation);
    let mut child = ChildGuard(
        Command::new(env!("CARGO_BIN_EXE_dedicated-server"))
            .args([
                "--bind",
                &server_address.to_string(),
                "--max-ticks",
                "300",
                "--exit-after-commands",
                "1",
            ])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("start dedicated server process"),
    );

    let first_socket = client_socket();
    let second_socket = client_socket();
    let first_session = handshake(&first_socket, server_address, 0x1111, &mut child);
    let _second_session = handshake(&second_socket, server_address, 0x2222, &mut child);
    first_socket
        .set_nonblocking(true)
        .expect("nonblocking first client");
    second_socket
        .set_nonblocking(true)
        .expect("nonblocking second client");

    let initial = demo_world();
    let command = ExplosionCommand {
        command_id: 1,
        center: IVec3::new(-20, 6, 0),
        radius_voxels: 8,
        peak_energy: 30_000,
    };
    first_socket
        .send_to(
            &encode_explosion_request(first_session, command),
            server_address,
        )
        .expect("send authoritative request");

    let mut endpoints = [
        Endpoint {
            socket: first_socket,
            inbox: OrderedDeltaInbox::default(),
            replica: ClientReplica::new(initial.clone()),
            applied: 0,
            drop_sequence: None,
            dropped_frames: 0,
        },
        Endpoint {
            socket: second_socket,
            inbox: OrderedDeltaInbox::default(),
            replica: ClientReplica::new(initial.clone()),
            applied: 0,
            drop_sequence: None,
            dropped_frames: 0,
        },
    ];
    let status = drive_until_exit(&mut child, &mut endpoints, server_address);
    assert!(status.success());
    assert!(endpoints.iter().all(|endpoint| endpoint.applied >= 1));

    let mut expected = AuthoritativeServer::new(initial);
    expected
        .execute_explosion(first_session, command)
        .expect("same local authoritative command");
    let _ = expected.advance_physics();
    for endpoint in &endpoints {
        assert_eq!(
            endpoint.replica.world().fingerprint(),
            expected.world().fingerprint()
        );
        assert_eq!(endpoint.replica.bodies(), expected.bodies());
        assert_eq!(endpoint.replica.body_states(), expected.body_states());
        assert_eq!(
            endpoint.replica.body_fingerprint(),
            expected.body_fingerprint()
        );
        assert_eq!(endpoint.replica.next_body_id(), expected.next_body_id());
    }
}

#[test]
fn retained_delta_repairs_a_deliberate_process_client_gap() {
    let reservation = UdpSocket::bind("127.0.0.1:0").expect("reserve loopback port");
    let server_address = reservation.local_addr().expect("reserved address");
    drop(reservation);
    let mut child = ChildGuard(
        Command::new(env!("CARGO_BIN_EXE_dedicated-server"))
            .args([
                "--bind",
                &server_address.to_string(),
                "--max-ticks",
                "300",
                "--exit-after-repairs",
                "1",
            ])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("start repair server process"),
    );

    let good_socket = client_socket();
    let lossy_socket = client_socket();
    let good_session = handshake(&good_socket, server_address, 0x3333, &mut child);
    let lossy_session = handshake(&lossy_socket, server_address, 0x4444, &mut child);
    good_socket
        .set_nonblocking(true)
        .expect("nonblocking good client");
    lossy_socket
        .set_nonblocking(true)
        .expect("nonblocking lossy client");

    let initial = demo_world();
    let mut endpoints = [
        Endpoint {
            socket: good_socket,
            inbox: OrderedDeltaInbox::default(),
            replica: ClientReplica::new(initial.clone()),
            applied: 0,
            drop_sequence: None,
            dropped_frames: 0,
        },
        Endpoint {
            socket: lossy_socket,
            inbox: OrderedDeltaInbox::default(),
            replica: ClientReplica::new(initial),
            applied: 0,
            drop_sequence: Some(1),
            dropped_frames: 0,
        },
    ];
    let first = ExplosionCommand {
        command_id: 1,
        center: IVec3::new(-20, 6, 0),
        radius_voxels: 8,
        peak_energy: 30_000,
    };
    endpoints[0]
        .socket
        .send_to(
            &encode_explosion_request(good_session, first),
            server_address,
        )
        .expect("send first command");

    wait_for_future_packet(&mut child, &mut endpoints, server_address, good_session);
    assert!(endpoints[1].dropped_frames > 0);
    assert_eq!(endpoints[1].inbox.expected_sequence(), 1);
    assert!(endpoints[1].inbox.buffered_complete_packets() > 0);

    thread::sleep(Duration::from_millis(10));
    receive_available(&mut endpoints[1], server_address);
    endpoints[1].drop_sequence = None;
    endpoints[1]
        .socket
        .send_to(&encode_repair_request(lossy_session, 1), server_address)
        .expect("request missing delta");

    let status = drive_until_exit(&mut child, &mut endpoints, server_address);
    assert!(status.success());
    assert_eq!(
        endpoints[0].replica.world().fingerprint(),
        endpoints[1].replica.world().fingerprint()
    );
    assert_eq!(endpoints[0].replica.bodies(), endpoints[1].replica.bodies());
    assert_eq!(
        endpoints[0].replica.body_states(),
        endpoints[1].replica.body_states()
    );
    assert_eq!(
        endpoints[0].inbox.expected_sequence(),
        endpoints[1].inbox.expected_sequence()
    );
}

#[test]
fn missing_retained_delta_falls_back_to_a_process_snapshot() {
    let reservation = UdpSocket::bind("127.0.0.1:0").expect("reserve loopback port");
    let server_address = reservation.local_addr().expect("reserved address");
    drop(reservation);
    let mut child = ChildGuard(
        Command::new(env!("CARGO_BIN_EXE_dedicated-server"))
            .args([
                "--bind",
                &server_address.to_string(),
                "--max-ticks",
                "300",
                "--exit-after-snapshots",
                "2",
            ])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("start snapshot server process"),
    );
    let socket = client_socket();
    let session = handshake(&socket, server_address, 0x5555, &mut child);
    socket
        .set_nonblocking(true)
        .expect("nonblocking snapshot client");
    socket
        .send_to(&encode_snapshot_request(session), server_address)
        .expect("request snapshot fallback");

    let deadline = Instant::now() + Duration::from_secs(5);
    let mut assembler = SnapshotAssembler::default();
    let mut snapshot = None;
    let mut received_snapshot_frames = 0_usize;
    let mut dropped_snapshot_frame = false;
    let mut retried_snapshot = false;
    let retry_at = Instant::now() + Duration::from_millis(1_200);
    let mut buffer = [0_u8; 1_201];
    while Instant::now() < deadline && snapshot.is_none() {
        loop {
            match socket.recv_from(&mut buffer) {
                Ok((length, source))
                    if source == server_address && is_snapshot_datagram(&buffer[..length]) =>
                {
                    received_snapshot_frames += 1;
                    if !dropped_snapshot_frame {
                        dropped_snapshot_frame = true;
                        continue;
                    }
                    snapshot = assembler
                        .push(&buffer[..length])
                        .expect("valid process snapshot")
                        .or(snapshot);
                }
                Ok(_) => {}
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
                Err(error) => panic!("snapshot receive failed: {error}"),
            }
        }
        if !retried_snapshot && Instant::now() >= retry_at {
            socket
                .send_to(&encode_snapshot_request(session), server_address)
                .expect("retry incomplete snapshot fallback");
            retried_snapshot = true;
        }
        if snapshot.is_none() {
            thread::sleep(Duration::from_millis(1));
        }
    }
    let snapshot = snapshot.unwrap_or_else(|| {
        panic!(
            "complete snapshot fallback: received {received_snapshot_frames} frames retaining {} bytes",
            assembler.retained_payload_bytes()
        )
    });
    assert!(dropped_snapshot_frame);
    assert!(retried_snapshot);
    let mut replica = ClientReplica::new(World::default());
    snapshot
        .install_into(&mut replica)
        .expect("atomic process snapshot install");
    assert_eq!(replica.world().fingerprint(), demo_world().fingerprint());
    assert_eq!(replica.next_body_id(), 1);

    let status = wait_for_child_exit(&mut child, Duration::from_secs(2));
    assert!(status.success());
}

#[test]
fn process_snapshot_catches_up_motion_before_returning_to_live_deltas() {
    let reservation = UdpSocket::bind("127.0.0.1:0").expect("reserve loopback port");
    let server_address = reservation.local_addr().expect("reserved address");
    drop(reservation);
    let mut child = spawn_test_server(server_address, 600, "--exit-after-catchups");
    let good_socket = client_socket();
    let joining_socket = client_socket();
    let good_session = handshake(&good_socket, server_address, 0x6666, &mut child);
    let joining_session = handshake(&joining_socket, server_address, 0x7777, &mut child);
    good_socket
        .set_nonblocking(true)
        .expect("nonblocking good client");
    joining_socket
        .set_nonblocking(true)
        .expect("nonblocking joining client");
    let initial = demo_world();
    let mut good = Endpoint {
        socket: good_socket,
        inbox: OrderedDeltaInbox::default(),
        replica: ClientReplica::new(initial),
        applied: 0,
        drop_sequence: None,
        dropped_frames: 0,
    };
    good.socket
        .send_to(
            &encode_explosion_request(
                good_session,
                ExplosionCommand {
                    command_id: 1,
                    center: IVec3::new(-20, 6, 0),
                    radius_voxels: 8,
                    peak_energy: 30_000,
                },
            ),
            server_address,
        )
        .expect("create moving body");

    let command_deadline = Instant::now() + Duration::from_secs(3);
    while good.applied == 0 && Instant::now() < command_deadline {
        receive_available(&mut good, server_address);
        discard_available(&joining_socket);
        thread::sleep(Duration::from_millis(1));
    }
    assert!(good.applied > 0);
    discard_available(&joining_socket);
    joining_socket
        .send_to(&encode_snapshot_request(joining_session), server_address)
        .expect("request moving-world snapshot");

    let mut snapshot_assembler = SnapshotAssembler::default();
    let mut joining_replica = ClientReplica::new(World::default());
    let mut joining_inbox = None;
    let mut joining_applied = 0_usize;
    let deadline = Instant::now() + Duration::from_secs(8);
    let mut exit_status = None;
    while Instant::now() < deadline {
        receive_available(&mut good, server_address);
        receive_joining_stream(
            &joining_socket,
            server_address,
            &mut snapshot_assembler,
            &mut joining_replica,
            &mut joining_inbox,
            &mut joining_applied,
        );
        if let Some(status) = child.0.try_wait().expect("query catch-up server") {
            exit_status = Some(status);
            break;
        }
        thread::sleep(Duration::from_millis(1));
    }
    let status = exit_status.expect("snapshot catch-up did not complete");
    assert!(status.success());
    let drain_deadline = Instant::now() + Duration::from_millis(100);
    while Instant::now() < drain_deadline {
        receive_available(&mut good, server_address);
        receive_joining_stream(
            &joining_socket,
            server_address,
            &mut snapshot_assembler,
            &mut joining_replica,
            &mut joining_inbox,
            &mut joining_applied,
        );
        thread::sleep(Duration::from_millis(1));
    }

    let joining_inbox = joining_inbox.expect("joining client installed a snapshot");
    assert!(joining_applied > 0);
    assert_eq!(
        good.replica.world().fingerprint(),
        joining_replica.world().fingerprint()
    );
    assert_eq!(good.replica.bodies(), joining_replica.bodies());
    assert_eq!(good.replica.body_states(), joining_replica.body_states());
    assert_eq!(
        good.inbox.expected_sequence(),
        joining_inbox.expected_sequence()
    );
}

fn client_socket() -> UdpSocket {
    let socket = UdpSocket::bind("127.0.0.1:0").expect("bind loopback client");
    socket
        .set_read_timeout(Some(Duration::from_millis(40)))
        .expect("client read timeout");
    socket
}

fn spawn_test_server(address: SocketAddr, max_ticks: u64, exit_flag: &str) -> ChildGuard {
    ChildGuard(
        Command::new(env!("CARGO_BIN_EXE_dedicated-server"))
            .arg("--bind")
            .arg(address.to_string())
            .arg("--max-ticks")
            .arg(max_ticks.to_string())
            .arg(exit_flag)
            .arg("1")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("start dedicated test server process"),
    )
}

fn discard_available(socket: &UdpSocket) {
    let mut buffer = [0_u8; 1_201];
    loop {
        match socket.recv_from(&mut buffer) {
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
            Err(error) => panic!("discard receive failed: {error}"),
        }
    }
}

fn receive_joining_stream(
    socket: &UdpSocket,
    server: SocketAddr,
    snapshot_assembler: &mut SnapshotAssembler,
    replica: &mut ClientReplica,
    inbox: &mut Option<OrderedDeltaInbox>,
    applied: &mut usize,
) {
    let mut buffer = [0_u8; 1_201];
    loop {
        match socket.recv_from(&mut buffer) {
            Ok((length, source)) if source == server => {
                let datagram = &buffer[..length];
                if is_snapshot_datagram(datagram) {
                    if let Some(snapshot) = snapshot_assembler
                        .push(datagram)
                        .expect("valid moving-world snapshot")
                    {
                        let next_sequence = snapshot.next_sequence();
                        snapshot
                            .install_into(replica)
                            .expect("install moving-world snapshot");
                        *inbox = Some(OrderedDeltaInbox::new(next_sequence));
                    }
                } else if is_delta_datagram(datagram)
                    && let Some(inbox) = inbox
                {
                    for packet in inbox.push(datagram).expect("valid catch-up delta") {
                        replica
                            .receive(&packet)
                            .expect("apply contiguous catch-up delta");
                        *applied += 1;
                    }
                }
            }
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
            Err(error) => panic!("joining client receive failed: {error}"),
        }
    }
}

fn handshake(socket: &UdpSocket, server: SocketAddr, nonce: u64, child: &mut ChildGuard) -> u64 {
    let hello = encode_client_hello(nonce);
    let deadline = Instant::now() + Duration::from_secs(3);
    let mut buffer = [0_u8; 1_201];
    while Instant::now() < deadline {
        assert!(
            child.0.try_wait().expect("query server process").is_none(),
            "dedicated server exited before handshake"
        );
        socket.send_to(&hello, server).expect("send hello");
        match socket.recv_from(&mut buffer) {
            Ok((length, source)) if source == server => {
                if let Ok(ServerControlMessage::Welcome {
                    nonce: received,
                    session_id,
                }) = decode_server_control(&buffer[..length])
                    && received == nonce
                {
                    return session_id;
                }
            }
            Ok(_) => {}
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                ) => {}
            Err(error) => panic!("handshake receive failed: {error}"),
        }
    }
    panic!("dedicated server did not complete handshake")
}

fn wait_for_future_packet(
    child: &mut ChildGuard,
    endpoints: &mut [Endpoint; 2],
    server: SocketAddr,
    good_session: u64,
) {
    let deadline = Instant::now() + Duration::from_secs(3);
    let mut second_command_sent = false;
    while Instant::now() < deadline {
        assert!(
            child.0.try_wait().expect("query repair server").is_none(),
            "repair server exited before receiving a repair request"
        );
        for endpoint in &mut *endpoints {
            receive_available(endpoint, server);
        }
        if endpoints[1].inbox.buffered_complete_packets() > 0 {
            return;
        }
        if endpoints[0].applied > 0 && !second_command_sent {
            let second = ExplosionCommand {
                command_id: 2,
                center: IVec3::new(20, 6, 0),
                radius_voxels: 4,
                peak_energy: 10_000,
            };
            endpoints[0]
                .socket
                .send_to(&encode_explosion_request(good_session, second), server)
                .expect("send second command");
            second_command_sent = true;
        }
        thread::sleep(Duration::from_millis(2));
    }
    panic!("lossy client did not retain a complete future delta")
}

fn drive_until_exit(
    child: &mut ChildGuard,
    endpoints: &mut [Endpoint; 2],
    server: SocketAddr,
) -> std::process::ExitStatus {
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut exited = None;
    while Instant::now() < deadline {
        for endpoint in &mut *endpoints {
            receive_available(endpoint, server);
        }
        if let Some(status) = child.0.try_wait().expect("query dedicated server") {
            exited = Some(status);
            break;
        }
        thread::sleep(Duration::from_millis(2));
    }
    let status = exited.expect("dedicated server did not stop after one command");
    let drain_deadline = Instant::now() + Duration::from_millis(100);
    while Instant::now() < drain_deadline {
        for endpoint in &mut *endpoints {
            receive_available(endpoint, server);
        }
        thread::sleep(Duration::from_millis(1));
    }
    status
}

fn wait_for_child_exit(child: &mut ChildGuard, timeout: Duration) -> std::process::ExitStatus {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if let Some(status) = child.0.try_wait().expect("query dedicated server") {
            return status;
        }
        thread::sleep(Duration::from_millis(2));
    }
    panic!("dedicated server did not exit before timeout")
}

fn receive_available(endpoint: &mut Endpoint, server: SocketAddr) {
    let mut buffer = [0_u8; 1_201];
    loop {
        match endpoint.socket.recv_from(&mut buffer) {
            Ok((length, source)) if source == server && is_delta_datagram(&buffer[..length]) => {
                let frame = decode_frame(&buffer[..length]).expect("valid server delta frame");
                if endpoint.drop_sequence == Some(frame.sequence) {
                    endpoint.dropped_frames += 1;
                    continue;
                }
                let ready = endpoint
                    .inbox
                    .push(&buffer[..length])
                    .expect("bounded ordered delta");
                for packet in ready {
                    endpoint
                        .replica
                        .receive(&packet)
                        .expect("contiguous authoritative delta");
                    endpoint.applied += 1;
                }
                if endpoint.drop_sequence.is_some()
                    && endpoint.inbox.buffered_complete_packets() > 0
                {
                    return;
                }
            }
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
            Err(error) => panic!("client receive failed: {error}"),
        }
    }
}
