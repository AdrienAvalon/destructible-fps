use destructible_fps::{
    AdaptiveRepairTimer, AuthoritativeServer, ClientReplica, ExplosionCommand, IVec3,
    MAX_REPAIR_RTO, MIN_REPAIR_RTO, OrderedDeltaInbox, PlayerInputCommand, ServerControlMessage,
    SnapshotAssembler, World, decode_frame, decode_player_state_packet, decode_server_control,
    demo_world, encode_client_hello, encode_explosion_request, encode_player_input,
    encode_repair_request, encode_snapshot_ack, encode_snapshot_fragments_request,
    encode_snapshot_request, is_delta_datagram, is_player_state_datagram, is_snapshot_datagram,
};
use std::{
    io::{self, BufRead, BufReader, Read},
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

const MAX_PROXY_PENDING_DATAGRAMS: usize = 2_048;
const MAX_PROXY_PENDING_BYTES: usize = 2 * 1_024 * 1_024;
const MAX_PROXY_RECEIVES_PER_PUMP: usize = 128;
const MAX_IMPAIRMENT_TRACE_STEPS: usize = 256;
const MAX_IMPAIRMENT_TRACE_DELAY_TICKS: u16 = 128;
const IMPAIRMENT_FLOW_COUNT: usize = 8;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ProxyDirection {
    ClientToServer,
    ServerToClient,
}

impl ProxyDirection {
    const fn index(self) -> usize {
        match self {
            Self::ClientToServer => 0,
            Self::ServerToClient => 1,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DatagramChannel {
    Control,
    Delta,
    Snapshot,
    Unknown,
}

impl DatagramChannel {
    const fn index(self) -> usize {
        match self {
            Self::Control => 0,
            Self::Delta => 1,
            Self::Snapshot => 2,
            Self::Unknown => 3,
        }
    }
}

const fn impairment_flow(direction: ProxyDirection, channel: DatagramChannel) -> usize {
    direction.index() * 4 + channel.index()
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct TraceStep {
    delay_ticks: u16,
    deliveries: u8,
}

impl TraceStep {
    const CLEAN: Self = Self {
        delay_ticks: 0,
        deliveries: 1,
    };
}

const fn trace_step(delay_ticks: u16, deliveries: u8) -> TraceStep {
    TraceStep {
        delay_ticks,
        deliveries,
    }
}

struct ImpairmentTrace {
    flows: [Vec<TraceStep>; IMPAIRMENT_FLOW_COUNT],
}

impl ImpairmentTrace {
    fn new() -> Self {
        Self {
            flows: std::array::from_fn(|_| Vec::new()),
        }
    }

    fn with_flow(
        mut self,
        direction: ProxyDirection,
        channel: DatagramChannel,
        steps: &[TraceStep],
    ) -> Result<Self, &'static str> {
        if steps.len() > MAX_IMPAIRMENT_TRACE_STEPS {
            return Err("impairment trace has too many steps");
        }
        if steps
            .iter()
            .any(|step| step.delay_ticks > MAX_IMPAIRMENT_TRACE_DELAY_TICKS || step.deliveries > 2)
        {
            return Err("impairment trace step is out of bounds");
        }
        self.flows[impairment_flow(direction, channel)] = steps.to_vec();
        Ok(self)
    }

    fn step(
        &self,
        direction: ProxyDirection,
        channel: DatagramChannel,
        cursor: usize,
    ) -> TraceStep {
        self.flows[impairment_flow(direction, channel)]
            .get(cursor)
            .copied()
            .unwrap_or(TraceStep::CLEAN)
    }
}

enum ImpairmentProfile {
    SelectiveRegression,
    Trace(ImpairmentTrace),
}

#[derive(Clone, Copy, Debug, Default)]
struct ChannelStats {
    received: usize,
    received_bytes: usize,
    delivered: usize,
    dropped: usize,
    duplicated: usize,
    delivered_bytes: usize,
}

#[derive(Clone, Copy, Debug, Default)]
struct ImpairmentStats {
    channels: [ChannelStats; 4],
    reordered_deliveries: usize,
    peak_pending_datagrams: usize,
    peak_pending_bytes: usize,
    queue_drops: usize,
    max_delay_ticks: u64,
}

struct ScheduledDatagram {
    direction: ProxyDirection,
    channel: DatagramChannel,
    deliver_at: u64,
    ordinal: u64,
    bytes: Vec<u8>,
}

struct DeterministicUdpProxy {
    client_socket: UdpSocket,
    server_socket: UdpSocket,
    server_address: SocketAddr,
    client_address: Option<SocketAddr>,
    pending: Vec<ScheduledDatagram>,
    pending_bytes: usize,
    next_ordinal: u64,
    tick: u64,
    highest_delivered_ordinal: [u64; 2],
    dropped_delta_one_once: [bool; 1_024],
    duplicated_delta_two_once: [bool; 1_024],
    dropped_snapshot_once: [bool; 4_096],
    duplicated_snapshot_once: [bool; 4_096],
    dropped_first_ack: bool,
    profile: ImpairmentProfile,
    trace_cursors: [usize; IMPAIRMENT_FLOW_COUNT],
    stats: ImpairmentStats,
}

struct TracedEndpoint {
    socket: UdpSocket,
    proxy: DeterministicUdpProxy,
    proxy_address: SocketAddr,
    session_id: u64,
    inbox: OrderedDeltaInbox,
    replica: ClientReplica,
    repair_timer: AdaptiveRepairTimer,
    gap_since: Option<Duration>,
    applied: usize,
    repairs: usize,
}

impl DeterministicUdpProxy {
    fn bind(server_address: SocketAddr) -> io::Result<Self> {
        Self::bind_with_profile(server_address, ImpairmentProfile::SelectiveRegression)
    }

    fn bind_with_trace(server_address: SocketAddr, trace: ImpairmentTrace) -> io::Result<Self> {
        Self::bind_with_profile(server_address, ImpairmentProfile::Trace(trace))
    }

    fn bind_with_profile(
        server_address: SocketAddr,
        profile: ImpairmentProfile,
    ) -> io::Result<Self> {
        let client_socket = UdpSocket::bind("127.0.0.1:0")?;
        let server_socket = UdpSocket::bind("127.0.0.1:0")?;
        client_socket.set_nonblocking(true)?;
        server_socket.set_nonblocking(true)?;
        Ok(Self {
            client_socket,
            server_socket,
            server_address,
            client_address: None,
            pending: Vec::with_capacity(MAX_PROXY_PENDING_DATAGRAMS),
            pending_bytes: 0,
            next_ordinal: 1,
            tick: 0,
            highest_delivered_ordinal: [0; 2],
            dropped_delta_one_once: [false; 1_024],
            duplicated_delta_two_once: [false; 1_024],
            dropped_snapshot_once: [false; 4_096],
            duplicated_snapshot_once: [false; 4_096],
            dropped_first_ack: false,
            profile,
            trace_cursors: [0; IMPAIRMENT_FLOW_COUNT],
            stats: ImpairmentStats::default(),
        })
    }

    fn client_address(&self) -> io::Result<SocketAddr> {
        self.client_socket.local_addr()
    }

    const fn stats(&self) -> ImpairmentStats {
        self.stats
    }

    const fn has_pending(&self) -> bool {
        !self.pending.is_empty()
    }

    fn pump(&mut self) -> io::Result<()> {
        self.tick = self.tick.saturating_add(1);
        self.receive_direction(ProxyDirection::ClientToServer)?;
        self.receive_direction(ProxyDirection::ServerToClient)?;
        self.deliver_due()
    }

    fn receive_direction(&mut self, direction: ProxyDirection) -> io::Result<()> {
        let mut buffer = [0_u8; 1_201];
        for _ in 0..MAX_PROXY_RECEIVES_PER_PUMP {
            let received = match direction {
                ProxyDirection::ClientToServer => self.client_socket.recv_from(&mut buffer),
                ProxyDirection::ServerToClient => self.server_socket.recv_from(&mut buffer),
            };
            let (length, source) = match received {
                Ok(received) => received,
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
                Err(error) => return Err(error),
            };
            if length > 1_200 || !self.accept_source(direction, source) {
                self.stats.queue_drops = self.stats.queue_drops.saturating_add(1);
                continue;
            }
            self.schedule(direction, &buffer[..length]);
        }
        Ok(())
    }

    fn accept_source(&mut self, direction: ProxyDirection, source: SocketAddr) -> bool {
        match direction {
            ProxyDirection::ClientToServer => {
                if let Some(expected) = self.client_address {
                    source == expected
                } else {
                    self.client_address = Some(source);
                    true
                }
            }
            ProxyDirection::ServerToClient => source == self.server_address,
        }
    }

    fn schedule(&mut self, direction: ProxyDirection, bytes: &[u8]) {
        let channel = classify_datagram(bytes);
        let channel_stats = &mut self.stats.channels[channel.index()];
        channel_stats.received = channel_stats.received.saturating_add(1);
        channel_stats.received_bytes = channel_stats.received_bytes.saturating_add(bytes.len());
        if let Some(step) = self.trace_step(direction, channel) {
            self.schedule_trace_step(direction, channel, bytes, step);
            return;
        }
        if self.should_drop(direction, channel, bytes) {
            self.stats.channels[channel.index()].dropped = self.stats.channels[channel.index()]
                .dropped
                .saturating_add(1);
            return;
        }
        let ordinal = self.next_ordinal;
        self.next_ordinal = self.next_ordinal.saturating_add(1);
        let delay = deterministic_delay(ordinal);
        self.stats.max_delay_ticks = self.stats.max_delay_ticks.max(delay);
        self.admit(ScheduledDatagram {
            direction,
            channel,
            deliver_at: self.tick.saturating_add(delay),
            ordinal,
            bytes: bytes.to_vec(),
        });
        if self.should_duplicate(direction, channel, bytes) {
            self.stats.channels[channel.index()].duplicated = self.stats.channels[channel.index()]
                .duplicated
                .saturating_add(1);
            self.admit(ScheduledDatagram {
                direction,
                channel,
                deliver_at: self.tick.saturating_add(delay),
                ordinal,
                bytes: bytes.to_vec(),
            });
        }
    }

    fn trace_step(
        &mut self,
        direction: ProxyDirection,
        channel: DatagramChannel,
    ) -> Option<TraceStep> {
        let ImpairmentProfile::Trace(trace) = &self.profile else {
            return None;
        };
        let flow = impairment_flow(direction, channel);
        let cursor = self.trace_cursors[flow];
        self.trace_cursors[flow] = cursor.saturating_add(1);
        Some(trace.step(direction, channel, cursor))
    }

    fn schedule_trace_step(
        &mut self,
        direction: ProxyDirection,
        channel: DatagramChannel,
        bytes: &[u8],
        step: TraceStep,
    ) {
        if step.deliveries == 0 {
            self.stats.channels[channel.index()].dropped = self.stats.channels[channel.index()]
                .dropped
                .saturating_add(1);
            return;
        }
        let ordinal = self.next_ordinal;
        self.next_ordinal = self.next_ordinal.saturating_add(1);
        let delay = u64::from(step.delay_ticks);
        self.stats.max_delay_ticks = self.stats.max_delay_ticks.max(delay);
        self.admit(ScheduledDatagram {
            direction,
            channel,
            deliver_at: self.tick.saturating_add(delay),
            ordinal,
            bytes: bytes.to_vec(),
        });
        if step.deliveries == 2 {
            self.stats.channels[channel.index()].duplicated = self.stats.channels[channel.index()]
                .duplicated
                .saturating_add(1);
            self.admit(ScheduledDatagram {
                direction,
                channel,
                deliver_at: self.tick.saturating_add(delay).saturating_add(1),
                ordinal,
                bytes: bytes.to_vec(),
            });
        }
    }

    fn should_drop(
        &mut self,
        direction: ProxyDirection,
        channel: DatagramChannel,
        bytes: &[u8],
    ) -> bool {
        if direction == ProxyDirection::ClientToServer
            && channel == DatagramChannel::Control
            && bytes.get(5) == Some(&7)
            && !self.dropped_first_ack
        {
            self.dropped_first_ack = true;
            return true;
        }
        if direction != ProxyDirection::ServerToClient || channel != DatagramChannel::Snapshot {
            return self.should_drop_delta(direction, channel, bytes);
        }
        let Some(index) = snapshot_fragment_index(bytes) else {
            return false;
        };
        if index % 53 == 0 && !self.dropped_snapshot_once[index] {
            self.dropped_snapshot_once[index] = true;
            return true;
        }
        false
    }

    fn should_drop_delta(
        &mut self,
        direction: ProxyDirection,
        channel: DatagramChannel,
        bytes: &[u8],
    ) -> bool {
        if direction != ProxyDirection::ServerToClient || channel != DatagramChannel::Delta {
            return false;
        }
        let Some((sequence, index)) = delta_identity(bytes) else {
            return false;
        };
        if sequence == 1 && !self.dropped_delta_one_once[index] {
            self.dropped_delta_one_once[index] = true;
            return true;
        }
        false
    }

    fn should_duplicate(
        &mut self,
        direction: ProxyDirection,
        channel: DatagramChannel,
        bytes: &[u8],
    ) -> bool {
        if direction != ProxyDirection::ServerToClient || channel != DatagramChannel::Snapshot {
            return self.should_duplicate_delta(direction, channel, bytes);
        }
        let Some(index) = snapshot_fragment_index(bytes) else {
            return false;
        };
        if index % 47 == 3 && !self.duplicated_snapshot_once[index] {
            self.duplicated_snapshot_once[index] = true;
            return true;
        }
        false
    }

    fn should_duplicate_delta(
        &mut self,
        direction: ProxyDirection,
        channel: DatagramChannel,
        bytes: &[u8],
    ) -> bool {
        if direction != ProxyDirection::ServerToClient || channel != DatagramChannel::Delta {
            return false;
        }
        let Some((sequence, index)) = delta_identity(bytes) else {
            return false;
        };
        if sequence == 2 && !self.duplicated_delta_two_once[index] {
            self.duplicated_delta_two_once[index] = true;
            return true;
        }
        false
    }

    fn admit(&mut self, datagram: ScheduledDatagram) {
        let bytes = datagram.bytes.len();
        if self.pending.len() >= MAX_PROXY_PENDING_DATAGRAMS
            || self.pending_bytes.saturating_add(bytes) > MAX_PROXY_PENDING_BYTES
        {
            self.stats.queue_drops = self.stats.queue_drops.saturating_add(1);
            return;
        }
        self.pending_bytes = self.pending_bytes.saturating_add(bytes);
        self.pending.push(datagram);
        self.stats.peak_pending_datagrams =
            self.stats.peak_pending_datagrams.max(self.pending.len());
        self.stats.peak_pending_bytes = self.stats.peak_pending_bytes.max(self.pending_bytes);
    }

    fn deliver_due(&mut self) -> io::Result<()> {
        let mut index = 0;
        while index < self.pending.len() {
            if self.pending[index].deliver_at > self.tick {
                index += 1;
                continue;
            }
            let datagram = self.pending.swap_remove(index);
            self.pending_bytes = self.pending_bytes.saturating_sub(datagram.bytes.len());
            let destination = match datagram.direction {
                ProxyDirection::ClientToServer => self.server_address,
                ProxyDirection::ServerToClient => self.client_address.ok_or_else(|| {
                    io::Error::new(io::ErrorKind::NotConnected, "proxy client is unknown")
                })?,
            };
            let socket = match datagram.direction {
                ProxyDirection::ClientToServer => &self.server_socket,
                ProxyDirection::ServerToClient => &self.client_socket,
            };
            let sent = socket.send_to(&datagram.bytes, destination)?;
            if sent != datagram.bytes.len() {
                return Err(io::Error::new(
                    io::ErrorKind::WriteZero,
                    "proxy sent a partial UDP datagram",
                ));
            }
            let direction = datagram.direction.index();
            if datagram.ordinal < self.highest_delivered_ordinal[direction] {
                self.stats.reordered_deliveries = self.stats.reordered_deliveries.saturating_add(1);
            }
            self.highest_delivered_ordinal[direction] =
                self.highest_delivered_ordinal[direction].max(datagram.ordinal);
            let channel_stats = &mut self.stats.channels[datagram.channel.index()];
            channel_stats.delivered = channel_stats.delivered.saturating_add(1);
            channel_stats.delivered_bytes = channel_stats
                .delivered_bytes
                .saturating_add(datagram.bytes.len());
        }
        Ok(())
    }
}

const fn deterministic_delay(ordinal: u64) -> u64 {
    2 + match ordinal % 6 {
        0 | 4 => 0,
        1 => 4,
        2 => 1,
        3 => 3,
        _ => 2,
    }
}

const fn impaired_commands() -> [ExplosionCommand; 2] {
    [
        ExplosionCommand {
            command_id: 1,
            center: IVec3::new(-20, 6, 0),
            radius_voxels: 8,
            peak_energy: 30_000,
        },
        ExplosionCommand {
            command_id: 2,
            center: IVec3::new(20, 6, 0),
            radius_voxels: 4,
            peak_energy: 10_000,
        },
    ]
}

fn classify_datagram(bytes: &[u8]) -> DatagramChannel {
    if bytes.starts_with(b"DFCT") {
        DatagramChannel::Control
    } else if bytes.starts_with(b"DFPS") {
        DatagramChannel::Delta
    } else if bytes.starts_with(b"DFSN") {
        DatagramChannel::Snapshot
    } else {
        DatagramChannel::Unknown
    }
}

fn snapshot_fragment_index(bytes: &[u8]) -> Option<usize> {
    let encoded = bytes.get(14..16)?;
    let index = usize::from(u16::from_le_bytes(encoded.try_into().ok()?));
    (index < 4_096).then_some(index)
}

fn delta_identity(bytes: &[u8]) -> Option<(u64, usize)> {
    let sequence = u64::from_le_bytes(bytes.get(6..14)?.try_into().ok()?);
    let fragment_index = usize::from(u16::from_le_bytes(bytes.get(86..88)?.try_into().ok()?));
    if fragment_index >= 1_024 {
        return None;
    }
    Some((sequence, fragment_index))
}

#[test]
fn impairment_metadata_parsers_reject_out_of_range_frames() {
    let mut snapshot = vec![0_u8; 16];
    snapshot[14..16].copy_from_slice(&u16::MAX.to_le_bytes());
    assert_eq!(snapshot_fragment_index(&snapshot), None);
    let mut delta = vec![0_u8; 88];
    delta[86..88].copy_from_slice(&u16::MAX.to_le_bytes());
    assert_eq!(delta_identity(&delta), None);
    assert_eq!(delta_identity(&[]), None);
}

#[test]
fn impairment_trace_is_bounded_and_becomes_clean_after_replay() {
    let steps = [
        TraceStep {
            delay_ticks: 7,
            deliveries: 0,
        },
        TraceStep {
            delay_ticks: 3,
            deliveries: 2,
        },
    ];
    let trace = ImpairmentTrace::new()
        .with_flow(
            ProxyDirection::ServerToClient,
            DatagramChannel::Delta,
            &steps,
        )
        .expect("bounded trace");
    assert_eq!(
        trace.step(ProxyDirection::ServerToClient, DatagramChannel::Delta, 0),
        steps[0]
    );
    assert_eq!(
        trace.step(ProxyDirection::ServerToClient, DatagramChannel::Delta, 1),
        steps[1]
    );
    assert_eq!(
        trace.step(ProxyDirection::ServerToClient, DatagramChannel::Delta, 2),
        TraceStep::CLEAN
    );

    let oversized = vec![TraceStep::CLEAN; MAX_IMPAIRMENT_TRACE_STEPS + 1];
    assert!(
        ImpairmentTrace::new()
            .with_flow(
                ProxyDirection::ServerToClient,
                DatagramChannel::Delta,
                &oversized,
            )
            .is_err()
    );
    for invalid in [
        TraceStep {
            delay_ticks: MAX_IMPAIRMENT_TRACE_DELAY_TICKS + 1,
            deliveries: 1,
        },
        TraceStep {
            delay_ticks: 0,
            deliveries: 3,
        },
    ] {
        assert!(
            ImpairmentTrace::new()
                .with_flow(
                    ProxyDirection::ServerToClient,
                    DatagramChannel::Delta,
                    &[invalid],
                )
                .is_err()
        );
    }
}

#[test]
fn impairment_queue_fails_closed_at_its_byte_and_count_limits() {
    let server_address = SocketAddr::from(([127, 0, 0, 1], 9));
    let mut proxy = DeterministicUdpProxy::bind(server_address).expect("bounded proxy");
    for ordinal in 0..=MAX_PROXY_PENDING_DATAGRAMS {
        proxy.admit(ScheduledDatagram {
            direction: ProxyDirection::ServerToClient,
            channel: DatagramChannel::Unknown,
            deliver_at: u64::MAX,
            ordinal: u64::try_from(ordinal).expect("small proxy queue"),
            bytes: vec![0; 1_200],
        });
    }

    assert!(proxy.pending.len() <= MAX_PROXY_PENDING_DATAGRAMS);
    assert!(proxy.pending_bytes <= MAX_PROXY_PENDING_BYTES);
    assert!(proxy.stats().queue_drops > 0);
}

#[test]
fn dedicated_process_synchronizes_two_real_udp_clients() {
    let (mut child, server_address) =
        spawn_ready_ephemeral_server(300, Some("--exit-after-commands"));

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
fn dedicated_process_replicates_moving_players_to_two_real_udp_clients() {
    let (mut child, server_address) = spawn_ready_ephemeral_server(300, None);
    let sockets = [client_socket(), client_socket()];
    let first_session = handshake(&sockets[0], server_address, 0x3333, &mut child);
    let second_session = handshake(&sockets[1], server_address, 0x4444, &mut child);
    for socket in &sockets {
        socket.set_nonblocking(true).expect("nonblocking client");
    }
    sockets[0]
        .send_to(
            &encode_player_input(
                first_session,
                PlayerInputCommand {
                    input_sequence: 1,
                    movement_x_per_mille: 1_000,
                    ..PlayerInputCommand::default()
                },
            ),
            server_address,
        )
        .expect("send player input");

    let deadline = Instant::now() + Duration::from_secs(2);
    let mut converged = [false; 2];
    let mut buffer = [0_u8; 1_201];
    while Instant::now() < deadline && !converged.into_iter().all(|ready| ready) {
        for (index, socket) in sockets.iter().enumerate() {
            loop {
                match socket.recv_from(&mut buffer) {
                    Ok((length, source))
                        if source == server_address
                            && is_player_state_datagram(&buffer[..length]) =>
                    {
                        let packet = decode_player_state_packet(&buffer[..length])
                            .expect("valid process player state");
                        converged[index] = packet.players.len() == 2
                            && packet.players.iter().any(|player| {
                                player.session_id == first_session
                                    && player.last_input_sequence == 1
                                    && player.position_um.x > 0
                            })
                            && packet
                                .players
                                .iter()
                                .any(|player| player.session_id == second_session);
                    }
                    Ok(_) => {}
                    Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
                    Err(error) => panic!("receive process player state: {error}"),
                }
            }
        }
        thread::sleep(Duration::from_millis(2));
    }
    assert!(converged.into_iter().all(|ready| ready));
}

#[test]
fn retained_delta_repairs_a_deliberate_process_client_gap() {
    let (mut child, server_address) =
        spawn_ready_ephemeral_server(300, Some("--exit-after-repairs"));

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
    let (mut child, server_address) =
        spawn_ready_ephemeral_server(300, Some("--exit-after-catchups"));
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
    let mut requested_missing_fragments = false;
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
        if !requested_missing_fragments && exactly_one_snapshot_fragment_is_missing(&assembler) {
            let snapshot_id = assembler
                .active_snapshot_id()
                .expect("incomplete snapshot has an active ID");
            let missing = assembler.missing_fragment_windows();
            assert!(!missing.is_empty());
            for (base_fragment, missing_mask) in missing {
                socket
                    .send_to(
                        &encode_snapshot_fragments_request(
                            session,
                            snapshot_id,
                            base_fragment,
                            missing_mask,
                        ),
                        server_address,
                    )
                    .expect("request only missing snapshot fragments");
            }
            requested_missing_fragments = true;
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
    assert!(requested_missing_fragments);
    let snapshot_id = snapshot.snapshot_id();
    let mut replica = ClientReplica::new(World::default());
    snapshot
        .install_into(&mut replica)
        .expect("atomic process snapshot install");
    socket
        .send_to(&encode_snapshot_ack(session, snapshot_id), server_address)
        .expect("acknowledge installed process snapshot");
    assert_eq!(replica.world().fingerprint(), demo_world().fingerprint());
    assert_eq!(replica.next_body_id(), 1);

    let status = wait_for_child_exit(&mut child, Duration::from_secs(2));
    assert!(status.success());
}

fn exactly_one_snapshot_fragment_is_missing(assembler: &SnapshotAssembler) -> bool {
    assembler
        .active_fragment_progress()
        .is_some_and(|(received, expected)| received.checked_add(1) == Some(expected))
}

#[test]
fn process_snapshot_catches_up_motion_before_returning_to_live_deltas() {
    let (mut child, server_address) =
        spawn_ready_ephemeral_server(600, Some("--exit-after-catchups"));
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
            joining_session,
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
            joining_session,
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

#[test]
fn retained_delta_converges_through_deterministic_network_impairments() {
    let (mut child, server_address) =
        spawn_ready_ephemeral_server(300, Some("--exit-after-repairs"));
    let mut proxy = DeterministicUdpProxy::bind(server_address).expect("bind deterministic proxy");
    let proxy_address = proxy.client_address().expect("proxy client address");
    let socket = client_socket();
    socket
        .set_nonblocking(true)
        .expect("nonblocking impaired client");
    let session = handshake_through_proxy(&socket, proxy_address, &mut proxy, 0x7878, &mut child);
    let [first, second] = impaired_commands();
    socket
        .send_to(&encode_explosion_request(session, first), proxy_address)
        .expect("send first impaired command");
    pump_proxy_for(&mut proxy, Duration::from_millis(50));
    socket
        .send_to(&encode_explosion_request(session, second), proxy_address)
        .expect("send second impaired command");
    pump_proxy_for(&mut proxy, Duration::from_millis(50));
    let mut endpoint = Endpoint {
        socket,
        inbox: OrderedDeltaInbox::default(),
        replica: ClientReplica::new(demo_world()),
        applied: 0,
        drop_sequence: None,
        dropped_frames: 0,
    };

    let future_deadline = Instant::now() + Duration::from_secs(3);
    while endpoint.inbox.buffered_complete_packets() == 0 && Instant::now() < future_deadline {
        assert!(
            child.0.try_wait().expect("query impaired server").is_none(),
            "impaired server exited before delta repair"
        );
        proxy.pump().expect("pump impaired deltas");
        receive_available(&mut endpoint, proxy_address);
        thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(endpoint.inbox.expected_sequence(), 1);
    assert!(endpoint.inbox.buffered_complete_packets() > 0);
    endpoint
        .socket
        .send_to(&encode_repair_request(session, 1), proxy_address)
        .expect("request impaired delta repair");

    let exit_deadline = Instant::now() + Duration::from_secs(3);
    let mut exit_status = None;
    while Instant::now() < exit_deadline {
        proxy.pump().expect("pump repaired deltas");
        receive_available(&mut endpoint, proxy_address);
        if let Some(status) = child.0.try_wait().expect("query repaired server") {
            exit_status = Some(status);
            break;
        }
        thread::sleep(Duration::from_millis(1));
    }
    assert!(
        exit_status
            .expect("impaired delta server did not exit")
            .success()
    );
    let drain_deadline = Instant::now() + Duration::from_millis(100);
    while Instant::now() < drain_deadline {
        proxy.pump().expect("drain impaired deltas");
        receive_available(&mut endpoint, proxy_address);
        if !proxy.has_pending() && endpoint.inbox.buffered_complete_packets() == 0 {
            break;
        }
        thread::sleep(Duration::from_millis(1));
    }

    assert!(endpoint.applied >= 2);
    assert!(endpoint.inbox.expected_sequence() >= 3);
    let mut expected = AuthoritativeServer::new(demo_world());
    expected
        .execute_explosion(session, first)
        .expect("first expected command");
    expected
        .execute_explosion(session, second)
        .expect("second expected command");
    assert_eq!(
        endpoint.replica.world().fingerprint(),
        expected.world().fingerprint()
    );
    let impairment = proxy.stats();
    let delta = impairment.channels[DatagramChannel::Delta.index()];
    assert!(delta.dropped >= 1, "delta loss was not injected");
    assert!(delta.duplicated >= 1, "delta duplication was not injected");
    assert!(
        impairment.reordered_deliveries >= 1,
        "delta reordering was not observed"
    );
    assert_eq!(impairment.queue_drops, 0);
    assert!(impairment.peak_pending_bytes <= MAX_PROXY_PENDING_BYTES);
    assert!(delta.delivered_bytes < 512 * 1_024);
    eprintln!(
        "deterministic delta impairment: delta={delta:?} reordered={} peak={}datagrams/{}bytes max_delay={}ticks",
        impairment.reordered_deliveries,
        impairment.peak_pending_datagrams,
        impairment.peak_pending_bytes,
        impairment.max_delay_ticks
    );
}

#[test]
fn four_trace_replay_clients_converge_with_adaptive_fair_repair() {
    let (mut child, server_address) = spawn_ready_ephemeral_server(1_200, None);
    let mut endpoints = Vec::with_capacity(4);
    for index in 0..4 {
        let mut proxy =
            DeterministicUdpProxy::bind_with_trace(server_address, four_client_trace(index))
                .expect("bind traced proxy");
        let proxy_address = proxy.client_address().expect("traced proxy address");
        let socket = client_socket();
        socket
            .set_nonblocking(true)
            .expect("nonblocking traced client");
        let session_id = handshake_through_proxy(
            &socket,
            proxy_address,
            &mut proxy,
            0x9000 + u64::try_from(index).expect("small client index"),
            &mut child,
        );
        endpoints.push(TracedEndpoint {
            socket,
            proxy,
            proxy_address,
            session_id,
            inbox: OrderedDeltaInbox::default(),
            replica: ClientReplica::new(demo_world()),
            repair_timer: AdaptiveRepairTimer::default(),
            gap_since: None,
            applied: 0,
            repairs: 0,
        });
    }

    let commands = impaired_commands();
    for command in commands {
        endpoints[0]
            .socket
            .send_to(
                &encode_explosion_request(endpoints[0].session_id, command),
                endpoints[0].proxy_address,
            )
            .expect("send traced authoritative command");
    }

    let started = Instant::now();
    let deadline = started + Duration::from_secs(8);
    while Instant::now() < deadline
        && !endpoints
            .iter()
            .all(|endpoint| endpoint.applied >= 2 && endpoint.inbox.expected_sequence() >= 3)
    {
        assert!(
            child.0.try_wait().expect("query trace server").is_none(),
            "trace server exited before four-client convergence"
        );
        let client_time = started.elapsed();
        for endpoint in &mut endpoints {
            endpoint.proxy.pump().expect("pump traced client");
            receive_traced_deltas(endpoint, client_time);
            request_traced_repair(endpoint, client_time);
        }
        thread::sleep(Duration::from_millis(1));
    }

    assert!(
        endpoints
            .iter()
            .all(|endpoint| endpoint.applied >= 2 && endpoint.inbox.expected_sequence() >= 3),
        "four traced clients did not converge"
    );
    settle_traced_endpoints(&mut endpoints, &mut child);
    let mut expected = AuthoritativeServer::new(demo_world());
    for command in commands {
        expected
            .execute_explosion(endpoints[0].session_id, command)
            .expect("replay traced authoritative command");
    }
    assert_four_trace_results(&endpoints, expected.world().fingerprint());
}

fn settle_traced_endpoints(endpoints: &mut [TracedEndpoint], child: &mut ChildGuard) {
    let deadline = Instant::now() + Duration::from_millis(250);
    while Instant::now() < deadline {
        assert!(
            child
                .0
                .try_wait()
                .expect("query settling trace server")
                .is_none(),
            "trace server exited before four-client egress settled"
        );
        for endpoint in &mut *endpoints {
            endpoint.proxy.pump().expect("settle traced client");
        }
        thread::sleep(Duration::from_millis(1));
    }
}

fn assert_four_trace_results(endpoints: &[TracedEndpoint], expected_world: u128) {
    for endpoint in endpoints {
        assert_eq!(endpoint.replica.world().fingerprint(), expected_world);
        assert_eq!(endpoint.repairs, 1);
        assert_eq!(endpoint.repair_timer.sample_count(), 1);
        assert!(endpoint.repair_timer.smoothed_rtt().is_some());
        assert!(endpoint.repair_timer.retransmission_timeout() >= MIN_REPAIR_RTO);
        assert!(endpoint.repair_timer.retransmission_timeout() <= MAX_REPAIR_RTO);
        let stats = endpoint.proxy.stats();
        let delta = stats.channels[DatagramChannel::Delta.index()];
        assert!(delta.dropped >= 1);
        assert!(delta.duplicated >= 1);
        assert!(stats.reordered_deliveries >= 1);
        assert_eq!(stats.queue_drops, 0);
        assert!(stats.peak_pending_bytes <= MAX_PROXY_PENDING_BYTES);
    }
    let received_bytes = endpoints
        .iter()
        .map(|endpoint| {
            endpoint.proxy.stats().channels[DatagramChannel::Delta.index()].received_bytes
        })
        .collect::<Vec<_>>();
    let minimum = received_bytes.iter().copied().min().unwrap_or_default();
    let maximum = received_bytes.iter().copied().max().unwrap_or_default();
    assert_eq!(minimum, maximum, "server delta egress was not client-fair");
    let delivered_bytes = endpoints
        .iter()
        .map(|endpoint| {
            endpoint.proxy.stats().channels[DatagramChannel::Delta.index()].delivered_bytes
        })
        .collect::<Vec<_>>();
    let delivered_minimum = delivered_bytes.iter().copied().min().unwrap_or_default();
    let delivered_maximum = delivered_bytes.iter().copied().max().unwrap_or_default();
    assert!(
        delivered_maximum.saturating_sub(delivered_minimum) <= 4 * 1_200,
        "trace delivery skew exceeded four datagrams: {delivered_bytes:?}"
    );
    eprintln!(
        "four-client trace replay: received={received_bytes:?} delivered={delivered_bytes:?} rtt={:?} rto={:?}",
        endpoints
            .iter()
            .map(|endpoint| endpoint.repair_timer.smoothed_rtt())
            .collect::<Vec<_>>(),
        endpoints
            .iter()
            .map(|endpoint| endpoint.repair_timer.retransmission_timeout())
            .collect::<Vec<_>>()
    );
}

#[test]
fn process_snapshot_converges_through_deterministic_network_impairments() {
    let (mut child, server_address) =
        spawn_ready_ephemeral_server(600, Some("--exit-after-catchups"));
    let mut proxy = DeterministicUdpProxy::bind(server_address).expect("bind deterministic proxy");
    let proxy_address = proxy.client_address().expect("proxy client address");
    let socket = client_socket();
    socket
        .set_nonblocking(true)
        .expect("nonblocking impaired client");
    let session = handshake_through_proxy(&socket, proxy_address, &mut proxy, 0x8888, &mut child);
    socket
        .send_to(&encode_snapshot_request(session), proxy_address)
        .expect("request impaired snapshot");

    let mut assembler = SnapshotAssembler::default();
    let mut replica = ClientReplica::new(World::default());
    let mut installed_snapshot_id = None;
    let mut next_repair = Instant::now() + Duration::from_millis(1_100);
    let mut next_ack = Instant::now();
    let deadline = Instant::now() + Duration::from_secs(8);
    let mut exit_status = None;
    let mut buffer = [0_u8; 1_201];
    while Instant::now() < deadline {
        proxy.pump().expect("pump deterministic proxy");
        loop {
            match socket.recv_from(&mut buffer) {
                Ok((length, source))
                    if source == proxy_address && is_snapshot_datagram(&buffer[..length]) =>
                {
                    if let Some(snapshot) = assembler
                        .push(&buffer[..length])
                        .expect("valid impaired snapshot")
                    {
                        let snapshot_id = snapshot.snapshot_id();
                        snapshot
                            .install_into(&mut replica)
                            .expect("install impaired snapshot");
                        installed_snapshot_id = Some(snapshot_id);
                        next_ack = Instant::now();
                    }
                }
                Ok(_) => {}
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
                Err(error) => panic!("impaired snapshot receive failed: {error}"),
            }
        }
        let now = Instant::now();
        if installed_snapshot_id.is_none() && now >= next_repair {
            request_missing_snapshot_fragments(&socket, proxy_address, session, &assembler);
            next_repair = now + Duration::from_millis(200);
        }
        if let Some(snapshot_id) = installed_snapshot_id
            && now >= next_ack
        {
            socket
                .send_to(&encode_snapshot_ack(session, snapshot_id), proxy_address)
                .expect("retry snapshot acknowledgement");
            next_ack = now + Duration::from_millis(50);
        }
        if let Some(status) = child.0.try_wait().expect("query impaired server") {
            exit_status = Some(status);
            break;
        }
        thread::sleep(Duration::from_millis(1));
    }

    let exit_code = exit_status.expect("impaired snapshot catch-up did not complete");
    assert!(exit_code.success());
    assert!(installed_snapshot_id.is_some());
    assert_eq!(replica.world().fingerprint(), demo_world().fingerprint());
    let impairment = proxy.stats();
    let control = impairment.channels[DatagramChannel::Control.index()];
    let snapshot = impairment.channels[DatagramChannel::Snapshot.index()];
    assert!(control.dropped >= 1, "first snapshot ACK was not dropped");
    assert!(snapshot.dropped >= 2, "snapshot loss was not injected");
    assert!(
        snapshot.duplicated >= 1,
        "snapshot duplication was not injected"
    );
    assert!(
        impairment.reordered_deliveries >= 1,
        "reordering was not observed"
    );
    assert!(
        impairment.max_delay_ticks >= 6,
        "jitter profile was not exercised"
    );
    assert_eq!(impairment.queue_drops, 0);
    assert!(impairment.peak_pending_datagrams <= MAX_PROXY_PENDING_DATAGRAMS);
    assert!(impairment.peak_pending_bytes <= MAX_PROXY_PENDING_BYTES);
    assert!(snapshot.delivered_bytes < 2 * 1_024 * 1_024);
    eprintln!(
        "deterministic impairment: control={control:?} snapshot={snapshot:?} reordered={} peak={}datagrams/{}bytes max_delay={}ticks",
        impairment.reordered_deliveries,
        impairment.peak_pending_datagrams,
        impairment.peak_pending_bytes,
        impairment.max_delay_ticks
    );
}

fn four_client_trace(index: usize) -> ImpairmentTrace {
    const PROFILES: [[TraceStep; 8]; 4] = [
        [
            trace_step(8, 1),
            trace_step(0, 0),
            trace_step(1, 1),
            trace_step(5, 2),
            trace_step(0, 1),
            trace_step(3, 1),
            trace_step(2, 1),
            trace_step(0, 1),
        ],
        [
            trace_step(6, 1),
            trace_step(1, 1),
            trace_step(0, 0),
            trace_step(4, 1),
            trace_step(0, 2),
            trace_step(2, 1),
            trace_step(5, 1),
            trace_step(0, 1),
        ],
        [
            trace_step(7, 2),
            trace_step(0, 1),
            trace_step(3, 1),
            trace_step(0, 0),
            trace_step(1, 1),
            trace_step(4, 1),
            trace_step(0, 1),
            trace_step(2, 1),
        ],
        [
            trace_step(5, 1),
            trace_step(2, 2),
            trace_step(0, 1),
            trace_step(6, 1),
            trace_step(0, 0),
            trace_step(1, 1),
            trace_step(3, 1),
            trace_step(0, 1),
        ],
    ];
    ImpairmentTrace::new()
        .with_flow(
            ProxyDirection::ServerToClient,
            DatagramChannel::Delta,
            &PROFILES[index],
        )
        .expect("bounded four-client trace")
}

fn receive_traced_deltas(endpoint: &mut TracedEndpoint, now: Duration) {
    let mut buffer = [0_u8; 1_201];
    loop {
        match endpoint.socket.recv_from(&mut buffer) {
            Ok((length, source))
                if source == endpoint.proxy_address && is_delta_datagram(&buffer[..length]) =>
            {
                let packets = endpoint
                    .inbox
                    .push(&buffer[..length])
                    .expect("valid traced delta");
                endpoint
                    .repair_timer
                    .observe_sequence(endpoint.inbox.expected_sequence(), now);
                if packets.is_empty() {
                    if endpoint.inbox.buffered_complete_packets() > 0 {
                        endpoint.gap_since.get_or_insert(now);
                    }
                } else {
                    endpoint.gap_since = None;
                }
                for packet in packets {
                    endpoint
                        .replica
                        .receive(&packet)
                        .expect("apply traced delta");
                    endpoint.applied = endpoint.applied.saturating_add(1);
                }
            }
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
            Err(error) => panic!("traced client receive failed: {error}"),
        }
    }
}

fn request_traced_repair(endpoint: &mut TracedEndpoint, now: Duration) {
    let Some(gap_since) = endpoint.gap_since else {
        return;
    };
    let sequence = endpoint.inbox.expected_sequence();
    if now.saturating_sub(gap_since) < endpoint.repair_timer.reorder_grace()
        || !endpoint.repair_timer.send_due(sequence, now)
    {
        return;
    }
    endpoint
        .socket
        .send_to(
            &encode_repair_request(endpoint.session_id, sequence),
            endpoint.proxy_address,
        )
        .expect("send traced adaptive repair");
    endpoint.repair_timer.record_send(sequence, now);
    endpoint.repairs = endpoint.repairs.saturating_add(1);
}

fn client_socket() -> UdpSocket {
    let socket = UdpSocket::bind("127.0.0.1:0").expect("bind loopback client");
    socket
        .set_read_timeout(Some(Duration::from_millis(40)))
        .expect("client read timeout");
    socket
}

fn spawn_ready_ephemeral_server(
    max_ticks: u64,
    exit_flag: Option<&str>,
) -> (ChildGuard, SocketAddr) {
    // A released reservation is not ownership: concurrent client/proxy binds can take that port
    // before the child. Let the child own an ephemeral socket and announce its actual address.
    let mut command = Command::new(env!("CARGO_BIN_EXE_dedicated-server"));
    command
        .args(["--bind", "127.0.0.1:0", "--max-ticks"])
        .arg(max_ticks.to_string());
    if let Some(flag) = exit_flag {
        command.args([flag, "1"]);
    }
    let mut child = ChildGuard(
        command
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("start ephemeral test server"),
    );
    let stdout = child.0.stdout.take().expect("server readiness pipe");
    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    let reader = thread::spawn(move || {
        let mut line = String::new();
        let mut input = BufReader::new(stdout.take(128));
        let result = input.read_line(&mut line).map(|_| line);
        let _ = sender.send((result, input.into_inner().into_inner()));
    });
    let ready = receiver.recv_timeout(Duration::from_secs(3));
    if ready.is_err() {
        let _ = child.0.kill();
        let _ = child.0.wait();
    }
    reader.join().expect("readiness reader completed");
    let (line, stdout) = ready.expect("bounded server readiness deadline");
    // Keep the pipe alive for the bounded STOP record; do not induce a later broken-pipe exit.
    child.0.stdout = Some(stdout);
    let line = line.expect("read server readiness");
    assert!(line.ends_with('\n'), "truncated server readiness record");
    let address: SocketAddr = line
        .trim_end()
        .strip_prefix("READY ")
        .expect("server announced readiness")
        .parse()
        .expect("valid bound address");
    assert!(address.ip().is_loopback() && address.port() != 0);
    assert!(
        child
            .0
            .try_wait()
            .expect("readiness process state")
            .is_none()
    );
    (child, address)
}

#[test]
fn ephemeral_test_servers_retain_distinct_owned_ready_sockets() {
    let servers = [
        (300, None),
        (300, Some("--exit-after-commands")),
        (600, Some("--exit-after-catchups")),
    ]
    .into_iter()
    .map(|(max_ticks, flag)| spawn_ready_ephemeral_server(max_ticks, flag))
    .collect::<Vec<_>>();
    for (_, address) in &servers {
        assert_eq!(
            UdpSocket::bind(address)
                .expect_err("ready server owns its port")
                .kind(),
            io::ErrorKind::AddrInUse
        );
    }
    let addresses = servers
        .iter()
        .map(|(_, address)| address)
        .collect::<std::collections::HashSet<_>>();
    assert_eq!(addresses.len(), servers.len());
}

fn handshake_through_proxy(
    socket: &UdpSocket,
    proxy_address: SocketAddr,
    proxy: &mut DeterministicUdpProxy,
    nonce: u64,
    child: &mut ChildGuard,
) -> u64 {
    let hello = encode_client_hello(nonce);
    let deadline = Instant::now() + Duration::from_secs(3);
    let mut next_hello = Instant::now();
    let mut buffer = [0_u8; 1_201];
    while Instant::now() < deadline {
        assert!(
            child.0.try_wait().expect("query impaired server").is_none(),
            "dedicated server exited before impaired handshake"
        );
        let now = Instant::now();
        if now >= next_hello {
            socket
                .send_to(&hello, proxy_address)
                .expect("send impaired hello");
            next_hello = now + Duration::from_millis(20);
        }
        proxy.pump().expect("pump proxy handshake");
        loop {
            match socket.recv_from(&mut buffer) {
                Ok((length, source)) if source == proxy_address => {
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
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
                Err(error) => panic!("impaired handshake receive failed: {error}"),
            }
        }
        thread::sleep(Duration::from_millis(1));
    }
    panic!("dedicated server did not complete impaired handshake")
}

fn pump_proxy_for(proxy: &mut DeterministicUdpProxy, duration: Duration) {
    let deadline = Instant::now() + duration;
    while Instant::now() < deadline {
        proxy.pump().expect("pump deterministic proxy interval");
        thread::sleep(Duration::from_millis(1));
    }
}

fn request_missing_snapshot_fragments(
    socket: &UdpSocket,
    proxy_address: SocketAddr,
    session: u64,
    assembler: &SnapshotAssembler,
) {
    let Some(snapshot_id) = assembler.active_snapshot_id() else {
        return;
    };
    for (base_fragment, missing_mask) in assembler.missing_fragment_windows() {
        socket
            .send_to(
                &encode_snapshot_fragments_request(
                    session,
                    snapshot_id,
                    base_fragment,
                    missing_mask,
                ),
                proxy_address,
            )
            .expect("request impaired missing fragments");
    }
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
    session: u64,
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
                        let snapshot_id = snapshot.snapshot_id();
                        let next_sequence = snapshot.next_sequence();
                        snapshot
                            .install_into(replica)
                            .expect("install moving-world snapshot");
                        *inbox = Some(OrderedDeltaInbox::new(next_sequence));
                        socket
                            .send_to(&encode_snapshot_ack(session, snapshot_id), server)
                            .expect("acknowledge installed moving-world snapshot");
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
