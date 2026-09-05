use destructible_fps::{
    AuthorityCore, ClientReplica, DemoSession, ExplosionCommand, FireMode, NetworkTickReport,
    OrderedDeltaInbox, ServerControlMessage, SnapshotAssembler, World, decode_frame,
    decode_server_control, encode_client_hello, encode_explosion_request, encode_repair_request,
    encode_snapshot_ack, encode_snapshot_request, is_delta_datagram, is_snapshot_datagram,
    structural_lab::{LAB_SEED, structural_lab},
};
use glam::Vec3;
use std::{
    thread,
    time::{Duration, Instant},
};

struct Endpoint {
    replica: ClientReplica,
    inbox: OrderedDeltaInbox,
}

impl Endpoint {
    fn new(world: World) -> Self {
        Self {
            replica: ClientReplica::new(world),
            inbox: OrderedDeltaInbox::default(),
        }
    }
    fn receive(&mut self, bytes: &[u8]) {
        for packet in self.inbox.push(bytes).unwrap() {
            self.replica.receive(&packet).unwrap();
        }
    }
}

fn tick(
    core: &mut AuthorityCore<u8>,
    controls: &[(u8, Vec<u8>)],
) -> (NetworkTickReport, Vec<(u8, Vec<u8>)>) {
    let before = core.authority().next_sequence();
    let bodies = core.authority().bodies().len();
    let mut report = core.begin_tick();
    let mut outbound = Vec::new();
    let mut sender = |peer, bytes: &[u8]| {
        outbound.push((peer, bytes.to_vec()));
        true
    };
    for (peer, bytes) in controls {
        core.ingest_datagram(*peer, bytes, &mut sender, &mut report);
    }
    assert_eq!(
        core.authority().next_sequence(),
        before,
        "ingest must not simulate"
    );
    assert_eq!(core.authority().bodies().len(), bodies);
    (core.complete_tick(&mut sender, report).unwrap(), outbound)
}

fn deliver(endpoints: &mut [Endpoint], frames: &[(u8, Vec<u8>)], drop_structural: bool) {
    for (peer, bytes) in frames.iter().rev() {
        if is_delta_datagram(bytes) && usize::from(*peer) < endpoints.len() {
            if drop_structural && *peer == 0 && decode_frame(bytes).unwrap().sequence == 2 {
                continue;
            }
            endpoints[usize::from(*peer)].receive(bytes);
        }
    }
}

fn synchronized(core: &AuthorityCore<u8>, endpoints: &[Endpoint]) {
    for endpoint in endpoints {
        assert_eq!(
            endpoint.replica.world().fingerprint(),
            core.authority().world().fingerprint()
        );
        assert_eq!(endpoint.replica.bodies(), core.authority().bodies());
        assert_eq!(
            endpoint.replica.body_states(),
            core.authority().body_states()
        );
        assert_eq!(
            endpoint.replica.body_fingerprint(),
            core.authority().body_fingerprint()
        );
    }
}

fn session(frames: &[(u8, Vec<u8>)], peer: u8) -> u64 {
    frames
        .iter()
        .find_map(|(destination, bytes)| {
            if *destination != peer {
                return None;
            }
            let ServerControlMessage::Welcome { session_id, .. } =
                decode_server_control(bytes).ok()?;
            Some(session_id)
        })
        .expect("welcome")
}

#[test]
fn tick_driven_fracture_repairs_creation_before_motion_and_late_join_at_both_mtu_limits() {
    for mtu in [256, 1200] {
        network_case(mtu);
    }
}

fn network_case(mtu: usize) {
    let (world, config) = structural_lab();
    let mut endpoints = vec![Endpoint::new(world.clone()), Endpoint::new(world.clone())];
    let mut core = AuthorityCore::new(world, mtu)
        .unwrap()
        .with_structural_simulation(&config)
        .unwrap();
    let (mut report, frames) = tick(
        &mut core,
        &[(0, encode_client_hello(11)), (1, encode_client_hello(12))],
    );
    let client_id = session(&frames, 0);
    let deadline = Instant::now() + Duration::from_secs(3);
    while report.structural.assessed == 0 {
        (report, _) = tick(&mut core, &[]);
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(report.structural.committed, 0);
    let request = encode_explosion_request(
        client_id,
        ExplosionCommand {
            command_id: 1,
            center: LAB_SEED,
            radius_voxels: 1,
            peak_energy: 1000,
        },
    );
    let (mut report, frames) = tick(&mut core, &[(0, request)]);
    assert_eq!(report.commands_applied, 1);
    deliver(&mut endpoints, &frames, true);
    while report.structural.committed == 0 {
        let (next, frames) = tick(&mut core, &[]);
        report = next;
        deliver(&mut endpoints, &frames, true);
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(report.structural.failed, 0);
    assert!(endpoints[0].replica.bodies().is_empty());
    assert!(
        endpoints[0].inbox.buffered_complete_packets() > 0,
        "motion must wait for body creation"
    );
    assert_eq!(endpoints[1].replica.bodies().len(), 2);
    let (report, frames) = tick(&mut core, &[(0, encode_repair_request(client_id, 2))]);
    assert_eq!(report.repairs_served, 1);
    deliver(&mut endpoints, &frames, false);
    assert_eq!(endpoints[0].inbox.buffered_complete_packets(), 0);
    synchronized(&core, &endpoints);
    for _ in 0..360 {
        let (report, frames) = tick(&mut core, &[]);
        assert_eq!(report.structural.failed, 0);
        deliver(&mut endpoints, &frames, false);
        synchronized(&core, &endpoints);
    }
    assert!(
        core.authority()
            .body_states()
            .values()
            .all(|body| body.sleeping && (999_000..=1_001_000).contains(&body.translation_um.y))
    );
    let (_, frames) = tick(&mut core, &[(2, encode_client_hello(13))]);
    let late_session = session(&frames, 2);
    let (_, mut frames) = tick(&mut core, &[(2, encode_snapshot_request(late_session))]);
    let mut assembler = SnapshotAssembler::default();
    let mut snapshot = None;
    for _ in 0..256 {
        for (peer, bytes) in &frames {
            if *peer == 2 && is_snapshot_datagram(bytes) {
                snapshot = assembler.push(bytes).unwrap().or(snapshot);
            }
        }
        if snapshot.is_some() {
            break;
        }
        (_, frames) = tick(&mut core, &[]);
    }
    let snapshot = snapshot.expect("bounded late snapshot transfer");
    let snapshot_id = snapshot.snapshot_id();
    let mut late = Endpoint::new(World::default());
    late.inbox = OrderedDeltaInbox::new(snapshot.next_sequence());
    snapshot.install_into(&mut late.replica).unwrap();
    endpoints.push(late);
    let (report, frames) = tick(
        &mut core,
        &[(2, encode_snapshot_ack(late_session, snapshot_id))],
    );
    assert_eq!(report.snapshot_acks_accepted, 1);
    deliver(&mut endpoints, &frames, false);
    synchronized(&core, &endpoints);
}

#[test]
fn playable_session_reports_structural_mesh_changes_and_settles_without_manual_commit() {
    let (world, config) = structural_lab();
    let mut session = DemoSession::new(world)
        .with_structural_simulation(&config)
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(3);
    while session.structural_status().assessed == 0 {
        session.tick().unwrap();
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(1));
    }
    let shot = session
        .fire(Vec3::new(1.5, 4.5, 12.0), -Vec3::Z, FireMode::TestCharge)
        .unwrap()
        .unwrap();
    assert_eq!(shot.target, LAB_SEED);
    assert_eq!(
        shot.report.fractured_voxels, 0,
        "charge weakens but does not itself detach the beam"
    );
    assert!(shot.spawned_body_ids.is_empty());
    let mut notifications = 0;
    for _ in 0..720 {
        let tick = session.tick().unwrap();
        if !tick.spawned_body_ids.is_empty() {
            assert_eq!(tick.spawned_body_ids, vec![1, 2]);
            assert!(!tick.dirty_chunks.is_empty());
            notifications += 1;
        }
        thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(notifications, 1);
    assert_eq!(session.structural_status().committed, 1);
    assert_eq!(session.structural_status().failed, 0);
    assert!(session.body_states().values().all(|state| state.sleeping));
    assert_eq!(
        session
            .bodies()
            .values()
            .map(|body| body.mass_kg)
            .sum::<u64>(),
        3250
    );
}

#[test]
fn local_and_network_policies_reject_late_or_repeated_enablement() {
    let (world, config) = structural_lab();
    let mut local = DemoSession::new(world.clone());
    local.tick().unwrap();
    assert!(local.with_structural_simulation(&config).is_err());
    let local = DemoSession::new(world.clone())
        .with_structural_simulation(&config)
        .unwrap();
    assert!(local.with_structural_simulation(&config).is_err());
    let mut core = AuthorityCore::<u8>::new(world.clone(), 1200).unwrap();
    let _ = core.begin_tick();
    assert!(core.with_structural_simulation(&config).is_err());
    let core = AuthorityCore::<u8>::new(world, 1200)
        .unwrap()
        .with_structural_simulation(&config)
        .unwrap();
    assert!(core.with_structural_simulation(&config).is_err());
}

fn receive_udp(
    sockets: &[std::net::UdpSocket],
    endpoints: &mut [Endpoint],
    server: std::net::SocketAddr,
    drop_creation: bool,
) -> Vec<(usize, u64)> {
    let mut welcomes = Vec::new();
    for (index, socket) in sockets.iter().enumerate() {
        for _ in 0..256 {
            let mut bytes = [0_u8; 1201];
            let (length, source) = match socket.recv_from(&mut bytes) {
                Ok(packet) => packet,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(error) => panic!("test UDP receive failed: {error}"),
            };
            assert_eq!(source, server);
            let bytes = &bytes[..length];
            if is_delta_datagram(bytes) {
                if drop_creation && index == 0 && decode_frame(bytes).unwrap().sequence == 2 {
                    continue;
                }
                endpoints[index].receive(bytes);
            } else if let Ok(ServerControlMessage::Welcome { session_id, .. }) =
                decode_server_control(bytes)
            {
                welcomes.push((index, session_id));
            }
        }
    }
    welcomes
}

fn udp_clients(address: std::net::SocketAddr) -> [std::net::UdpSocket; 2] {
    let sockets = [
        std::net::UdpSocket::bind(("127.0.0.1", 0)).unwrap(),
        std::net::UdpSocket::bind(("127.0.0.1", 0)).unwrap(),
    ];
    for (index, socket) in sockets.iter().enumerate() {
        socket.set_nonblocking(true).unwrap();
        socket
            .send_to(
                &encode_client_hello(u64::try_from(index + 1).unwrap()),
                address,
            )
            .unwrap();
    }
    sockets
}

#[test]
fn actual_loopback_udp_adapter_runs_the_policy_and_repairs_lost_creation() {
    let (world, config) = structural_lab();
    let mut endpoints = [Endpoint::new(world.clone()), Endpoint::new(world.clone())];
    let core = AuthorityCore::new(world, 1200)
        .unwrap()
        .with_structural_simulation(&config)
        .unwrap();
    let mut server = destructible_fps::DedicatedServer::bind_core(("127.0.0.1", 0), core).unwrap();
    let address = server.local_addr().unwrap();
    let sockets = udp_clients(address);
    let mut sessions = [0_u64; 2];
    for _ in 0..200 {
        let report = server.tick().unwrap();
        for (index, id) in receive_udp(&sockets, &mut endpoints, address, false) {
            sessions[index] = id;
        }
        if sessions.iter().all(|id| *id != 0) && report.structural.assessed > 0 {
            break;
        }
        thread::sleep(Duration::from_millis(1));
    }
    assert!(sessions.iter().all(|id| *id != 0));
    sockets[0]
        .send_to(
            &encode_explosion_request(
                sessions[0],
                ExplosionCommand {
                    command_id: 1,
                    center: LAB_SEED,
                    radius_voxels: 1,
                    peak_energy: 1000,
                },
            ),
            address,
        )
        .unwrap();
    let mut cuts = 0;
    for _ in 0..200 {
        let report = server.tick().unwrap();
        receive_udp(&sockets, &mut endpoints, address, true);
        cuts = report.structural.committed;
        assert!(!report.structural.incomplete());
        if cuts == 1 {
            break;
        }
        thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(cuts, 1);
    for _ in 0..50 {
        receive_udp(&sockets, &mut endpoints, address, true);
        if endpoints[0].inbox.buffered_complete_packets() > 0
            && endpoints[1].replica.bodies().len() == 2
        {
            break;
        }
        thread::sleep(Duration::from_millis(1));
    }
    assert!(endpoints[0].replica.bodies().is_empty());
    assert!(endpoints[0].inbox.buffered_complete_packets() > 0);
    assert_eq!(endpoints[1].replica.bodies().len(), 2);
    sockets[0]
        .send_to(&encode_repair_request(sessions[0], 2), address)
        .unwrap();
    let mut repaired = 0;
    for _ in 0..20 {
        let report = server.tick().unwrap();
        receive_udp(&sockets, &mut endpoints, address, false);
        repaired += report.repairs_served;
        if repaired > 0 {
            break;
        }
        thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(repaired, 1);
    for _ in 0..50 {
        receive_udp(&sockets, &mut endpoints, address, false);
        if endpoints.iter().all(|endpoint| {
            endpoint.replica.body_fingerprint() == server.authority().body_fingerprint()
        }) {
            break;
        }
        thread::sleep(Duration::from_millis(1));
    }
    for endpoint in endpoints {
        assert_eq!(
            endpoint.replica.world().fingerprint(),
            server.authority().world().fingerprint()
        );
        assert_eq!(endpoint.replica.bodies(), server.authority().bodies());
        assert_eq!(
            endpoint.replica.body_states(),
            server.authority().body_states()
        );
        assert_eq!(endpoint.inbox.buffered_complete_packets(), 0);
    }
}
