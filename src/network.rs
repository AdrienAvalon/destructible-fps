//! Bounded real-UDP dedicated-server runtime and ordered client delta inbox.

use crate::{
    AuthoritativeServer, ClientControlMessage, CodecError, DeltaPacket, ExplosionCommand,
    FrameAssembler, PhysicsTickReport, World, decode_client_control, decode_frame, encode_frames,
    encode_server_welcome,
};
use core::fmt;
use std::{
    collections::{BTreeMap, VecDeque},
    io,
    net::{SocketAddr, ToSocketAddrs, UdpSocket},
};

pub const MAX_SERVER_PEERS: usize = 16;
pub const MAX_QUEUED_COMMANDS: usize = 256;
pub const MAX_QUEUED_REPAIRS: usize = 64;
pub const MAX_RECEIVED_DATAGRAMS_PER_TICK: usize = 64;
pub const MAX_SIMULATED_COMMANDS_PER_TICK: usize = 32;
pub const MAX_REPAIRS_PER_TICK: usize = 16;
pub const MAX_OUTBOUND_DATAGRAMS_PER_TICK: usize = 4_096;
pub const MAX_RETAINED_DELTA_PACKETS: usize = 64;
pub const MAX_RETAINED_DELTA_BYTES: usize = 8 * 1_024 * 1_024;
const MAX_PEER_IDLE_TICKS: u64 = 3_600;
const MAX_COMPLETE_PACKETS: usize = 16;
const MAX_COMPLETE_PACKET_BYTES: usize = 8 * 1_024 * 1_024;
const APPLICATION_MTU: usize = 1_200;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct NetworkTickReport {
    pub received_datagrams: usize,
    pub malformed_datagrams: usize,
    pub rejected_sessions: usize,
    pub peer_limit_drops: usize,
    pub queue_limit_drops: usize,
    pub repair_queue_drops: usize,
    pub repairs_served: usize,
    pub repair_misses: usize,
    pub commands_applied: usize,
    pub commands_rejected: usize,
    pub outbound_attempts: usize,
    pub outbound_datagrams: usize,
    pub outbound_drops: usize,
    pub physics: PhysicsTickReport,
}

#[derive(Debug)]
pub enum NetworkRuntimeError {
    Io(io::Error),
    Codec(CodecError),
}

impl fmt::Display for NetworkRuntimeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => error.fmt(formatter),
            Self::Codec(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for NetworkRuntimeError {}

impl From<io::Error> for NetworkRuntimeError {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<CodecError> for NetworkRuntimeError {
    fn from(value: CodecError) -> Self {
        Self::Codec(value)
    }
}

#[derive(Clone, Copy)]
struct Peer {
    nonce: u64,
    session_id: u64,
    last_seen_tick: u64,
}

#[derive(Clone, Copy)]
struct QueuedCommand {
    session_id: u64,
    command: ExplosionCommand,
}

#[derive(Clone, Copy)]
struct QueuedRepair {
    source: SocketAddr,
    session_id: u64,
    missing_sequence: u64,
}

struct RetainedDelta {
    sequence: u64,
    frames: Vec<Vec<u8>>,
    bytes: usize,
}

pub struct DedicatedServer {
    socket: UdpSocket,
    authority: AuthoritativeServer,
    peers: BTreeMap<SocketAddr, Peer>,
    commands: VecDeque<QueuedCommand>,
    repairs: VecDeque<QueuedRepair>,
    retained_deltas: VecDeque<RetainedDelta>,
    retained_delta_bytes: usize,
    next_session_id: u64,
    tick: u64,
}

impl DedicatedServer {
    /// Binds a nonblocking UDP authority. Use an explicit loopback address until authenticated
    /// remote transport is implemented.
    ///
    /// # Errors
    ///
    /// Returns socket resolution, bind, or nonblocking-configuration failures.
    pub fn bind(address: impl ToSocketAddrs, world: World) -> io::Result<Self> {
        let socket = UdpSocket::bind(address)?;
        if !socket.local_addr()?.ip().is_loopback() {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "unauthenticated dedicated transport is restricted to loopback",
            ));
        }
        socket.set_nonblocking(true)?;
        Ok(Self {
            socket,
            authority: AuthoritativeServer::new(world),
            peers: BTreeMap::new(),
            commands: VecDeque::new(),
            repairs: VecDeque::new(),
            retained_deltas: VecDeque::new(),
            retained_delta_bytes: 0,
            next_session_id: 1,
            tick: 0,
        })
    }

    /// Receives a bounded batch, enqueues validated commands, then performs simulation outside the
    /// receive phase and broadcasts canonical authority deltas.
    ///
    /// # Errors
    ///
    /// Returns non-transient socket failures or an impossible authoritative frame-encoding error.
    pub fn tick(&mut self) -> Result<NetworkTickReport, NetworkRuntimeError> {
        self.tick = self.tick.wrapping_add(1);
        self.prune_idle_peers();
        let mut report = NetworkTickReport::default();
        self.receive_batch(&mut report)?;
        self.process_repairs(&mut report);
        self.simulate_commands(&mut report)?;
        let (physics_packet, physics) = self.authority.advance_physics();
        report.physics = physics;
        if let Some(packet) = physics_packet {
            self.broadcast(&packet, &mut report)?;
        }
        Ok(report)
    }

    #[must_use]
    pub const fn authority(&self) -> &AuthoritativeServer {
        &self.authority
    }

    /// Returns the actual bound address, including an ephemeral port selected for port zero.
    ///
    /// # Errors
    ///
    /// Returns the underlying socket address query error.
    pub fn local_addr(&self) -> io::Result<SocketAddr> {
        self.socket.local_addr()
    }

    #[must_use]
    pub fn peer_count(&self) -> usize {
        self.peers.len()
    }

    #[must_use]
    pub fn queued_commands(&self) -> usize {
        self.commands.len()
    }

    #[must_use]
    pub fn retained_delta_packets(&self) -> usize {
        self.retained_deltas.len()
    }

    #[must_use]
    pub const fn retained_delta_bytes(&self) -> usize {
        self.retained_delta_bytes
    }

    fn receive_batch(&mut self, report: &mut NetworkTickReport) -> io::Result<()> {
        let mut datagram = [0_u8; APPLICATION_MTU + 1];
        for _ in 0..MAX_RECEIVED_DATAGRAMS_PER_TICK {
            let (length, source) = match self.socket.recv_from(&mut datagram) {
                Ok(received) => received,
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
                Err(error) => return Err(error),
            };
            report.received_datagrams += 1;
            if length > APPLICATION_MTU {
                report.malformed_datagrams += 1;
                continue;
            }
            let Ok(message) = decode_client_control(&datagram[..length]) else {
                report.malformed_datagrams += 1;
                continue;
            };
            match message {
                ClientControlMessage::Hello { nonce } => {
                    self.accept_hello(source, nonce, report);
                }
                ClientControlMessage::Explosion {
                    session_id,
                    command,
                } => self.enqueue_command(source, session_id, command, report),
                ClientControlMessage::RepairRequest {
                    session_id,
                    missing_sequence,
                } => self.enqueue_repair(source, session_id, missing_sequence, report),
            }
        }
        Ok(())
    }

    fn accept_hello(&mut self, source: SocketAddr, nonce: u64, report: &mut NetworkTickReport) {
        if let Some(peer) = self.peers.get_mut(&source)
            && peer.nonce == nonce
        {
            peer.last_seen_tick = self.tick;
            send_welcome(&self.socket, source, *peer, report);
            return;
        }
        if !self.peers.contains_key(&source) && self.peers.len() >= MAX_SERVER_PEERS {
            report.peer_limit_drops += 1;
            return;
        }
        let Some(next_session_id) = self.next_session_id.checked_add(1) else {
            report.peer_limit_drops += 1;
            return;
        };
        let peer = Peer {
            nonce,
            session_id: self.next_session_id,
            last_seen_tick: self.tick,
        };
        self.next_session_id = next_session_id;
        self.peers.insert(source, peer);
        send_welcome(&self.socket, source, peer, report);
    }

    fn enqueue_command(
        &mut self,
        source: SocketAddr,
        session_id: u64,
        command: ExplosionCommand,
        report: &mut NetworkTickReport,
    ) {
        let Some(peer) = self.peers.get_mut(&source) else {
            report.rejected_sessions += 1;
            return;
        };
        if peer.session_id != session_id {
            report.rejected_sessions += 1;
            return;
        }
        peer.last_seen_tick = self.tick;
        if self.commands.len() >= MAX_QUEUED_COMMANDS {
            report.queue_limit_drops += 1;
            return;
        }
        self.commands.push_back(QueuedCommand {
            session_id,
            command,
        });
    }

    fn enqueue_repair(
        &mut self,
        source: SocketAddr,
        session_id: u64,
        missing_sequence: u64,
        report: &mut NetworkTickReport,
    ) {
        let Some(peer) = self.peers.get_mut(&source) else {
            report.rejected_sessions += 1;
            return;
        };
        if peer.session_id != session_id || missing_sequence == 0 {
            report.rejected_sessions += 1;
            return;
        }
        peer.last_seen_tick = self.tick;
        if self.repairs.len() >= MAX_QUEUED_REPAIRS {
            report.repair_queue_drops += 1;
            return;
        }
        self.repairs.push_back(QueuedRepair {
            source,
            session_id,
            missing_sequence,
        });
    }

    fn process_repairs(&mut self, report: &mut NetworkTickReport) {
        for _ in 0..MAX_REPAIRS_PER_TICK {
            let Some(repair) = self.repairs.pop_front() else {
                break;
            };
            if self
                .peers
                .get(&repair.source)
                .is_none_or(|peer| peer.session_id != repair.session_id)
            {
                report.rejected_sessions += 1;
                continue;
            }
            let Some(retained) = self
                .retained_deltas
                .iter()
                .find(|packet| packet.sequence == repair.missing_sequence)
            else {
                report.repair_misses += 1;
                continue;
            };
            if send_packet_frames(&self.socket, &retained.frames, repair.source, report) {
                report.repairs_served += 1;
            }
        }
    }

    fn simulate_commands(
        &mut self,
        report: &mut NetworkTickReport,
    ) -> Result<(), NetworkRuntimeError> {
        for _ in 0..MAX_SIMULATED_COMMANDS_PER_TICK {
            let Some(queued) = self.commands.pop_front() else {
                break;
            };
            match self
                .authority
                .execute_explosion(queued.session_id, queued.command)
            {
                Ok((packet, _destruction)) => {
                    report.commands_applied += 1;
                    self.broadcast(&packet, report)?;
                }
                Err(_error) => report.commands_rejected += 1,
            }
        }
        Ok(())
    }

    fn broadcast(
        &mut self,
        packet: &DeltaPacket,
        report: &mut NetworkTickReport,
    ) -> Result<(), NetworkRuntimeError> {
        let frames = encode_frames(packet, APPLICATION_MTU)?;
        for destination in self.peers.keys() {
            send_packet_frames(&self.socket, &frames, *destination, report);
        }
        self.retain_delta(packet.sequence, frames);
        Ok(())
    }

    fn retain_delta(&mut self, sequence: u64, frames: Vec<Vec<u8>>) {
        let bytes = frames.iter().map(Vec::len).sum::<usize>();
        if bytes > MAX_RETAINED_DELTA_BYTES {
            return;
        }
        while self.retained_deltas.len() >= MAX_RETAINED_DELTA_PACKETS
            || self.retained_delta_bytes.saturating_add(bytes) > MAX_RETAINED_DELTA_BYTES
        {
            let Some(removed) = self.retained_deltas.pop_front() else {
                break;
            };
            self.retained_delta_bytes = self.retained_delta_bytes.saturating_sub(removed.bytes);
        }
        self.retained_delta_bytes = self.retained_delta_bytes.saturating_add(bytes);
        self.retained_deltas.push_back(RetainedDelta {
            sequence,
            frames,
            bytes,
        });
    }

    fn prune_idle_peers(&mut self) {
        let earliest = self.tick.saturating_sub(MAX_PEER_IDLE_TICKS);
        self.peers
            .retain(|_address, peer| peer.last_seen_tick >= earliest);
    }
}

fn send_welcome(
    socket: &UdpSocket,
    destination: SocketAddr,
    peer: Peer,
    report: &mut NetworkTickReport,
) {
    let message = encode_server_welcome(peer.nonce, peer.session_id);
    if report.outbound_attempts >= MAX_OUTBOUND_DATAGRAMS_PER_TICK {
        report.outbound_drops += 1;
        return;
    }
    report.outbound_attempts += 1;
    match socket.send_to(&message, destination) {
        Ok(length) if length == message.len() => report.outbound_datagrams += 1,
        Ok(_) | Err(_) => report.outbound_drops += 1,
    }
}

fn send_packet_frames(
    socket: &UdpSocket,
    frames: &[Vec<u8>],
    destination: SocketAddr,
    report: &mut NetworkTickReport,
) -> bool {
    if frames.len() > MAX_OUTBOUND_DATAGRAMS_PER_TICK.saturating_sub(report.outbound_attempts) {
        report.outbound_drops = report.outbound_drops.saturating_add(frames.len());
        return false;
    }
    let mut complete = true;
    for frame in frames {
        report.outbound_attempts += 1;
        match socket.send_to(frame, destination) {
            Ok(length) if length == frame.len() => report.outbound_datagrams += 1,
            Ok(_) | Err(_) => {
                report.outbound_drops += 1;
                complete = false;
            }
        }
    }
    complete
}

pub struct OrderedDeltaInbox {
    assembler: FrameAssembler,
    complete: BTreeMap<u64, DeltaPacket>,
    complete_bytes: usize,
    next_sequence: u64,
}

impl OrderedDeltaInbox {
    #[must_use]
    pub fn new(next_sequence: u64) -> Self {
        Self {
            assembler: FrameAssembler::default(),
            complete: BTreeMap::new(),
            complete_bytes: 0,
            next_sequence,
        }
    }

    /// Accepts one raw application datagram and releases only complete contiguous packets.
    ///
    /// # Errors
    ///
    /// Rejects malformed frames, inconsistent duplicates, and complete-packet buffering beyond the
    /// fixed packet or byte budget.
    pub fn push(&mut self, bytes: &[u8]) -> Result<Vec<DeltaPacket>, CodecError> {
        let Some(packet) = self.assembler.push(decode_frame(bytes)?)? else {
            return Ok(Vec::new());
        };
        if packet.sequence < self.next_sequence {
            return Ok(Vec::new());
        }
        let retained = packet.retained_bytes();
        if let Some(existing) = self.complete.get(&packet.sequence) {
            return if existing == &packet {
                Ok(Vec::new())
            } else {
                Err(CodecError::InconsistentFragment)
            };
        }
        if self.complete.len() >= MAX_COMPLETE_PACKETS
            || self.complete_bytes.saturating_add(retained) > MAX_COMPLETE_PACKET_BYTES
        {
            return Err(CodecError::TooManyPendingBytes);
        }
        self.complete_bytes = self.complete_bytes.saturating_add(retained);
        self.complete.insert(packet.sequence, packet);
        let mut ready = Vec::new();
        while let Some(packet) = self.complete.remove(&self.next_sequence) {
            self.complete_bytes = self.complete_bytes.saturating_sub(packet.retained_bytes());
            ready.push(packet);
            self.next_sequence = self.next_sequence.wrapping_add(1);
        }
        Ok(ready)
    }

    #[must_use]
    pub fn buffered_complete_packets(&self) -> usize {
        self.complete.len()
    }

    #[must_use]
    pub const fn buffered_complete_bytes(&self) -> usize {
        self.complete_bytes
    }

    #[must_use]
    pub const fn expected_sequence(&self) -> u64 {
        self.next_sequence
    }
}

impl Default for OrderedDeltaInbox {
    fn default() -> Self {
        Self::new(1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_COMMAND: ExplosionCommand = ExplosionCommand {
        command_id: 1,
        center: crate::IVec3::new(0, 0, 0),
        radius_voxels: 1,
        peak_energy: 1,
    };

    fn empty_packet(sequence: u64) -> DeltaPacket {
        DeltaPacket {
            sequence,
            tick: sequence,
            base_fingerprint: 0,
            final_fingerprint: 0,
            base_body_fingerprint: 0,
            final_body_fingerprint: 0,
            changes: Vec::new(),
            body_assignments: Vec::new(),
            body_updates: Vec::new(),
        }
    }

    #[test]
    fn ordered_inbox_holds_a_complete_future_packet() {
        let first = encode_frames(&empty_packet(1), APPLICATION_MTU)
            .expect("first packet")
            .remove(0);
        let second = encode_frames(&empty_packet(2), APPLICATION_MTU)
            .expect("second packet")
            .remove(0);
        let mut inbox = OrderedDeltaInbox::default();

        assert!(inbox.push(&second).expect("future packet").is_empty());
        assert_eq!(inbox.buffered_complete_packets(), 1);
        let ready = inbox.push(&first).expect("missing packet arrives");
        assert_eq!(
            ready
                .iter()
                .map(|packet| packet.sequence)
                .collect::<Vec<_>>(),
            vec![1, 2]
        );
        assert_eq!(inbox.buffered_complete_packets(), 0);
        assert_eq!(inbox.buffered_complete_bytes(), 0);
    }

    #[test]
    fn command_queue_and_session_validation_are_bounded() {
        let mut server = DedicatedServer::bind("127.0.0.1:0", World::default()).expect("server");
        let source = SocketAddr::from(([127, 0, 0, 1], 20_001));
        server.peers.insert(
            source,
            Peer {
                nonce: 1,
                session_id: 7,
                last_seen_tick: 0,
            },
        );
        let mut report = NetworkTickReport::default();

        server.enqueue_command(source, 6, TEST_COMMAND, &mut report);
        for _ in 0..=MAX_QUEUED_COMMANDS {
            server.enqueue_command(source, 7, TEST_COMMAND, &mut report);
        }

        assert_eq!(server.queued_commands(), MAX_QUEUED_COMMANDS);
        assert_eq!(report.rejected_sessions, 1);
        assert_eq!(report.queue_limit_drops, 1);
    }

    #[test]
    fn peer_admission_stops_at_the_fixed_limit() {
        let mut server = DedicatedServer::bind("127.0.0.1:0", World::default()).expect("server");
        let mut report = NetworkTickReport::default();

        for index in 0..=MAX_SERVER_PEERS {
            let index = u16::try_from(index).expect("small peer limit");
            let source = SocketAddr::from(([127, 0, 0, 1], 21_000 + index));
            server.accept_hello(source, u64::from(index), &mut report);
        }

        assert_eq!(server.peer_count(), MAX_SERVER_PEERS);
        assert_eq!(report.peer_limit_drops, 1);
    }

    #[test]
    fn public_server_api_rejects_non_loopback_bind() {
        let error = DedicatedServer::bind("0.0.0.0:0", World::default())
            .err()
            .expect("wildcard bind must be rejected");

        assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
    }

    #[test]
    fn broadcast_does_not_attempt_a_partial_packet_past_the_tick_budget() {
        let mut server = DedicatedServer::bind("127.0.0.1:0", World::default()).expect("server");
        let destination = SocketAddr::from(([127, 0, 0, 1], 22_001));
        server.peers.insert(
            destination,
            Peer {
                nonce: 1,
                session_id: 1,
                last_seen_tick: 0,
            },
        );
        let mut report = NetworkTickReport {
            outbound_attempts: MAX_OUTBOUND_DATAGRAMS_PER_TICK,
            ..NetworkTickReport::default()
        };

        server
            .broadcast(&empty_packet(1), &mut report)
            .expect("valid packet encoding");

        assert_eq!(report.outbound_attempts, MAX_OUTBOUND_DATAGRAMS_PER_TICK);
        assert_eq!(report.outbound_datagrams, 0);
        assert_eq!(report.outbound_drops, 1);
    }

    #[test]
    fn repair_queue_and_retained_history_are_bounded() {
        let mut server = DedicatedServer::bind("127.0.0.1:0", World::default()).expect("server");
        let source = SocketAddr::from(([127, 0, 0, 1], 23_001));
        server.peers.insert(
            source,
            Peer {
                nonce: 1,
                session_id: 7,
                last_seen_tick: 0,
            },
        );
        let mut report = NetworkTickReport::default();
        server.enqueue_repair(source, 6, 1, &mut report);
        server.enqueue_repair(source, 7, 0, &mut report);
        for sequence in 1..=MAX_QUEUED_REPAIRS + 1 {
            server.enqueue_repair(
                source,
                7,
                u64::try_from(sequence).expect("small repair queue"),
                &mut report,
            );
        }
        for sequence in 1..=MAX_RETAINED_DELTA_PACKETS + 1 {
            server.retain_delta(
                u64::try_from(sequence).expect("small retained history"),
                vec![vec![0]],
            );
        }

        assert_eq!(server.repairs.len(), MAX_QUEUED_REPAIRS);
        assert_eq!(report.rejected_sessions, 2);
        assert_eq!(report.repair_queue_drops, 1);
        assert_eq!(server.retained_delta_packets(), MAX_RETAINED_DELTA_PACKETS);
        assert_eq!(server.retained_delta_bytes(), MAX_RETAINED_DELTA_PACKETS);
        assert_eq!(
            server.retained_deltas.front().map(|packet| packet.sequence),
            Some(2)
        );
    }
}
