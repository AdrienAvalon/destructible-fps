//! Transport-independent bounded authority core, loopback UDP adapter, and client delta inbox.

use crate::{
    AuthenticatedPrincipal, AuthoritativeServer, ClientControlMessage, CodecError, DeltaPacket,
    ExplosionCommand, FrameAssembler, PhysicsTickReport, World, decode_client_control,
    decode_frame, encode_frames, encode_server_welcome, encode_snapshot_frames,
};
use core::fmt;
use std::{
    collections::{BTreeMap, VecDeque},
    io,
    net::{SocketAddr, ToSocketAddrs, UdpSocket},
    sync::Arc,
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
pub const MAX_SNAPSHOT_FRAMES_PER_PEER_PER_TICK: usize = 16;
pub const MAX_SNAPSHOT_CATCHUP_PACKETS: usize = 256;
pub const MAX_SNAPSHOT_CATCHUP_BYTES: usize = 8 * 1_024 * 1_024;
const MAX_PEER_IDLE_TICKS: u64 = 3_600;
const SNAPSHOT_RETRY_COOLDOWN_TICKS: u64 = 60;
const MAX_COMPLETE_PACKETS: usize = 16;
const MAX_COMPLETE_PACKET_BYTES: usize = 8 * 1_024 * 1_024;
pub const MAX_APPLICATION_DATAGRAM_BYTES: usize = 1_200;
pub const MIN_APPLICATION_DATAGRAM_BYTES: usize = 256;
pub const LEGACY_UDP_APPLICATION_DATAGRAM_BYTES: usize = MAX_APPLICATION_DATAGRAM_BYTES;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct NetworkTickReport {
    pub received_datagrams: usize,
    pub receive_limit_drops: usize,
    pub malformed_datagrams: usize,
    pub rejected_sessions: usize,
    pub peer_limit_drops: usize,
    pub queue_limit_drops: usize,
    pub repair_queue_drops: usize,
    pub repairs_served: usize,
    pub repair_misses: usize,
    pub snapshot_fallbacks_served: usize,
    pub snapshot_build_failures: usize,
    pub snapshot_request_drops: usize,
    pub snapshot_catchup_stalls: usize,
    pub snapshot_catchups_completed: usize,
    pub snapshot_fragment_requests_served: usize,
    pub snapshot_fragments_retransmitted: usize,
    pub snapshot_acks_accepted: usize,
    pub invalid_snapshot_controls: usize,
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
    InvalidApplicationDatagramBytes(usize),
}

impl fmt::Display for NetworkRuntimeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => error.fmt(formatter),
            Self::Codec(error) => error.fmt(formatter),
            Self::InvalidApplicationDatagramBytes(bytes) => {
                write!(formatter, "invalid application datagram bound {bytes}")
            }
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
    principal: Option<AuthenticatedPrincipal>,
    last_seen_tick: u64,
    last_snapshot_tick: Option<u64>,
}

#[derive(Clone, Copy)]
struct QueuedCommand {
    session_id: u64,
    command: ExplosionCommand,
}

#[derive(Clone, Copy)]
enum RecoveryRequest {
    Delta(u64),
    Snapshot,
    SnapshotFragments {
        snapshot_id: u64,
        base_fragment: u16,
        missing_mask: u64,
    },
    SnapshotAck {
        snapshot_id: u64,
    },
}

#[derive(Clone, Copy)]
struct QueuedRepair<PeerId> {
    source: PeerId,
    session_id: u64,
    request: RecoveryRequest,
}

struct RetainedDelta {
    sequence: u64,
    frames: Arc<[Vec<u8>]>,
    bytes: usize,
}

struct CatchupDelta {
    sequence: u64,
    frames: Arc<[Vec<u8>]>,
    bytes: usize,
}

enum SnapshotTransferStage {
    Snapshot { next_frame: usize },
    AwaitingAck,
    Catchup { next_sequence: u64 },
    Stalled,
}

struct SnapshotTransfer {
    snapshot_id: u64,
    frames: Arc<[Vec<u8>]>,
    snapshot_next_sequence: u64,
    stage: SnapshotTransferStage,
    catchup: VecDeque<CatchupDelta>,
    catchup_bytes: usize,
}

struct CachedSnapshot {
    snapshot_id: u64,
    next_sequence: u64,
    frames: Arc<[Vec<u8>]>,
}

/// Bounded deterministic authority state independent from any concrete network transport.
pub struct AuthorityCore<PeerId> {
    authority: AuthoritativeServer,
    peers: BTreeMap<PeerId, Peer>,
    commands: VecDeque<QueuedCommand>,
    repairs: VecDeque<QueuedRepair<PeerId>>,
    retained_deltas: VecDeque<RetainedDelta>,
    retained_delta_bytes: usize,
    snapshot_transfers: BTreeMap<PeerId, SnapshotTransfer>,
    next_session_id: u64,
    next_snapshot_id: u64,
    tick: u64,
    application_datagram_bytes: usize,
    received_datagrams_this_tick: usize,
}

impl<PeerId> AuthorityCore<PeerId>
where
    PeerId: Copy + Ord,
{
    /// Creates a bounded authority core for a transport's application payload limit.
    ///
    /// # Errors
    ///
    /// Rejects limits too small for useful protocol frames or larger than the repository-wide
    /// datagram ceiling.
    pub fn new(
        world: World,
        application_datagram_bytes: usize,
    ) -> Result<Self, NetworkRuntimeError> {
        if !(MIN_APPLICATION_DATAGRAM_BYTES..=MAX_APPLICATION_DATAGRAM_BYTES)
            .contains(&application_datagram_bytes)
        {
            return Err(NetworkRuntimeError::InvalidApplicationDatagramBytes(
                application_datagram_bytes,
            ));
        }
        Ok(Self {
            authority: AuthoritativeServer::new(world),
            peers: BTreeMap::new(),
            commands: VecDeque::new(),
            repairs: VecDeque::new(),
            retained_deltas: VecDeque::new(),
            retained_delta_bytes: 0,
            snapshot_transfers: BTreeMap::new(),
            next_session_id: 1,
            next_snapshot_id: 1,
            tick: 0,
            application_datagram_bytes,
            received_datagrams_this_tick: 0,
        })
    }

    /// Starts one deterministic simulation tick and expires idle transport peers.
    #[must_use]
    pub fn begin_tick(&mut self) -> NetworkTickReport {
        self.tick = self.tick.saturating_add(1);
        self.received_datagrams_this_tick = 0;
        self.prune_idle_peers();
        NetworkTickReport::default()
    }

    /// Admits one already-authenticated transport connection without trusting wire identity fields.
    ///
    /// Session IDs are allocated by the secure transport supervisor and must be unique and non-zero.
    pub fn admit_authenticated(
        &mut self,
        source: PeerId,
        client_nonce: u64,
        session_id: u64,
        principal: AuthenticatedPrincipal,
        report: &mut NetworkTickReport,
    ) -> bool {
        if session_id == 0
            || self.peers.contains_key(&source)
            || self
                .peers
                .values()
                .any(|peer| peer.session_id == session_id)
        {
            report.rejected_sessions += 1;
            return false;
        }
        if self.peers.len() >= MAX_SERVER_PEERS {
            report.peer_limit_drops += 1;
            return false;
        }
        self.peers.insert(
            source,
            Peer {
                nonce: client_nonce,
                session_id,
                principal: Some(principal),
                last_seen_tick: self.tick,
                last_snapshot_tick: None,
            },
        );
        true
    }

    /// Removes exactly one authenticated session and any queued work owned by it.
    pub fn disconnect_authenticated(&mut self, source: PeerId, session_id: u64) -> bool {
        let matches = self
            .peers
            .get(&source)
            .is_some_and(|peer| peer.principal.is_some() && peer.session_id == session_id);
        if !matches {
            return false;
        }
        self.remove_peer_state(source, session_id);
        true
    }

    /// Ingests one bounded transport payload. The supplied sender is invoked only for a legacy
    /// loopback welcome; authenticated peers cannot renegotiate through a wire `Hello`.
    pub fn ingest_datagram(
        &mut self,
        source: PeerId,
        bytes: &[u8],
        sender: &mut impl FnMut(PeerId, &[u8]) -> bool,
        report: &mut NetworkTickReport,
    ) {
        if self.received_datagrams_this_tick >= MAX_RECEIVED_DATAGRAMS_PER_TICK {
            report.receive_limit_drops += 1;
            return;
        }
        self.received_datagrams_this_tick += 1;
        report.received_datagrams += 1;
        if bytes.len() > self.application_datagram_bytes {
            report.malformed_datagrams += 1;
            return;
        }
        let Ok(message) = decode_client_control(bytes) else {
            report.malformed_datagrams += 1;
            return;
        };
        match message {
            ClientControlMessage::Hello { nonce } => {
                if self
                    .peers
                    .get(&source)
                    .is_some_and(|peer| peer.principal.is_some())
                {
                    report.rejected_sessions += 1;
                } else {
                    self.accept_hello(source, nonce, sender, report);
                }
            }
            ClientControlMessage::Explosion {
                session_id,
                command,
            } => self.enqueue_command(source, session_id, command, report),
            ClientControlMessage::RepairRequest {
                session_id,
                missing_sequence,
            } => self.enqueue_repair(source, session_id, missing_sequence, report),
            ClientControlMessage::SnapshotRequest { session_id } => {
                self.enqueue_snapshot(source, session_id, report);
            }
            ClientControlMessage::SnapshotFragmentsRequest {
                session_id,
                snapshot_id,
                base_fragment,
                missing_mask,
            } => self.enqueue_recovery(
                source,
                session_id,
                RecoveryRequest::SnapshotFragments {
                    snapshot_id,
                    base_fragment,
                    missing_mask,
                },
                report,
            ),
            ClientControlMessage::SnapshotAck {
                session_id,
                snapshot_id,
            } => self.enqueue_recovery(
                source,
                session_id,
                RecoveryRequest::SnapshotAck { snapshot_id },
                report,
            ),
        }
    }

    /// Applies bounded repair, command, physics, and replication work for the current tick.
    ///
    /// # Errors
    ///
    /// Returns an impossible authoritative frame-encoding failure.
    pub fn complete_tick(
        &mut self,
        sender: &mut impl FnMut(PeerId, &[u8]) -> bool,
        mut report: NetworkTickReport,
    ) -> Result<NetworkTickReport, NetworkRuntimeError> {
        self.process_repairs(sender, &mut report);
        self.service_snapshot_transfers(sender, &mut report);
        self.simulate_commands(sender, &mut report)?;
        let (physics_packet, physics) = self.authority.advance_physics();
        report.physics = physics;
        if let Some(packet) = physics_packet {
            self.broadcast(&packet, sender, &mut report)?;
        }
        Ok(report)
    }

    #[must_use]
    pub const fn authority(&self) -> &AuthoritativeServer {
        &self.authority
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

    #[must_use]
    pub fn principal(&self, source: PeerId) -> Option<AuthenticatedPrincipal> {
        self.peers.get(&source).and_then(|peer| peer.principal)
    }

    #[must_use]
    pub const fn application_datagram_bytes(&self) -> usize {
        self.application_datagram_bytes
    }

    fn remove_peer_state(&mut self, source: PeerId, session_id: u64) {
        self.peers.remove(&source);
        self.snapshot_transfers.remove(&source);
        self.repairs
            .retain(|repair| repair.source != source && repair.session_id != session_id);
        self.commands
            .retain(|command| command.session_id != session_id);
    }

    fn accept_hello(
        &mut self,
        source: PeerId,
        nonce: u64,
        sender: &mut impl FnMut(PeerId, &[u8]) -> bool,
        report: &mut NetworkTickReport,
    ) {
        if let Some(peer) = self.peers.get_mut(&source)
            && peer.nonce == nonce
        {
            peer.last_seen_tick = self.tick;
            send_welcome(sender, source, *peer, report);
            return;
        }
        if let Some(replaced_session) = self.peers.get(&source).map(|peer| peer.session_id) {
            self.remove_peer_state(source, replaced_session);
        }
        if !self.peers.contains_key(&source) && self.peers.len() >= MAX_SERVER_PEERS {
            report.peer_limit_drops += 1;
            return;
        }
        while self
            .peers
            .values()
            .any(|peer| peer.session_id == self.next_session_id)
        {
            let Some(next) = self.next_session_id.checked_add(1) else {
                report.peer_limit_drops += 1;
                return;
            };
            self.next_session_id = next;
        }
        let session_id = self.next_session_id;
        let Some(next_session_id) = session_id.checked_add(1) else {
            report.peer_limit_drops += 1;
            return;
        };
        let peer = Peer {
            nonce,
            session_id,
            principal: None,
            last_seen_tick: self.tick,
            last_snapshot_tick: None,
        };
        self.next_session_id = next_session_id;
        self.snapshot_transfers.remove(&source);
        self.peers.insert(source, peer);
        send_welcome(sender, source, peer, report);
    }

    fn enqueue_command(
        &mut self,
        source: PeerId,
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
        source: PeerId,
        session_id: u64,
        missing_sequence: u64,
        report: &mut NetworkTickReport,
    ) {
        if missing_sequence == 0 {
            report.rejected_sessions += 1;
            return;
        }
        self.enqueue_recovery(
            source,
            session_id,
            RecoveryRequest::Delta(missing_sequence),
            report,
        );
    }

    fn enqueue_snapshot(
        &mut self,
        source: PeerId,
        session_id: u64,
        report: &mut NetworkTickReport,
    ) {
        self.enqueue_recovery(source, session_id, RecoveryRequest::Snapshot, report);
    }

    fn enqueue_recovery(
        &mut self,
        source: PeerId,
        session_id: u64,
        request: RecoveryRequest,
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
        if self.repairs.len() >= MAX_QUEUED_REPAIRS {
            report.repair_queue_drops += 1;
            return;
        }
        self.repairs.push_back(QueuedRepair {
            source,
            session_id,
            request,
        });
    }

    fn process_repairs(
        &mut self,
        sender: &mut impl FnMut(PeerId, &[u8]) -> bool,
        report: &mut NetworkTickReport,
    ) {
        let mut snapshot_attempted = false;
        let mut snapshot_cache = None;
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
            match repair.request {
                RecoveryRequest::SnapshotFragments {
                    snapshot_id,
                    base_fragment,
                    missing_mask,
                } => {
                    self.process_snapshot_fragments(
                        repair.source,
                        snapshot_id,
                        base_fragment,
                        missing_mask,
                        sender,
                        report,
                    );
                    continue;
                }
                RecoveryRequest::SnapshotAck { snapshot_id } => {
                    self.process_snapshot_ack(repair.source, snapshot_id, report);
                    continue;
                }
                RecoveryRequest::Delta(missing_sequence) => {
                    if let Some(retained) = self
                        .retained_deltas
                        .iter()
                        .find(|packet| packet.sequence == missing_sequence)
                    {
                        if send_packet_frames(sender, &retained.frames, repair.source, report) {
                            report.repairs_served += 1;
                        }
                        continue;
                    }
                    report.repair_misses += 1;
                    if missing_sequence >= self.authority.next_sequence() {
                        report.snapshot_request_drops += 1;
                        continue;
                    }
                }
                RecoveryRequest::Snapshot => {}
            }
            let transfer_is_active = self
                .snapshot_transfers
                .get(&repair.source)
                .is_some_and(|transfer| !matches!(transfer.stage, SnapshotTransferStage::Stalled));
            if transfer_is_active {
                report.snapshot_request_drops += 1;
                continue;
            }
            let snapshot_is_throttled = self
                .peers
                .get(&repair.source)
                .and_then(|peer| peer.last_snapshot_tick)
                .is_some_and(|last| self.tick.saturating_sub(last) < SNAPSHOT_RETRY_COOLDOWN_TICKS);
            if snapshot_is_throttled {
                report.snapshot_request_drops += 1;
                continue;
            }
            if !snapshot_attempted {
                snapshot_attempted = true;
                snapshot_cache = self.build_snapshot(report);
            }
            let Some(snapshot) = &snapshot_cache else {
                continue;
            };
            if let Some(peer) = self.peers.get_mut(&repair.source) {
                peer.last_snapshot_tick = Some(self.tick);
            }
            self.snapshot_transfers.insert(
                repair.source,
                SnapshotTransfer {
                    snapshot_id: snapshot.snapshot_id,
                    frames: Arc::clone(&snapshot.frames),
                    snapshot_next_sequence: snapshot.next_sequence,
                    stage: SnapshotTransferStage::Snapshot { next_frame: 0 },
                    catchup: VecDeque::new(),
                    catchup_bytes: 0,
                },
            );
        }
    }

    fn build_snapshot(&mut self, report: &mut NetworkTickReport) -> Option<CachedSnapshot> {
        let Some(next_snapshot_id) = self.next_snapshot_id.checked_add(1) else {
            report.snapshot_build_failures += 1;
            return None;
        };
        let frames = match encode_snapshot_frames(
            self.next_snapshot_id,
            &self.authority,
            self.application_datagram_bytes,
        ) {
            Ok(frames) => frames,
            Err(_error) => {
                report.snapshot_build_failures += 1;
                return None;
            }
        };
        let snapshot = CachedSnapshot {
            snapshot_id: self.next_snapshot_id,
            next_sequence: self.authority.next_sequence(),
            frames: frames.into(),
        };
        self.next_snapshot_id = next_snapshot_id;
        Some(snapshot)
    }

    fn process_snapshot_fragments(
        &self,
        source: PeerId,
        snapshot_id: u64,
        base_fragment: u16,
        missing_mask: u64,
        sender: &mut impl FnMut(PeerId, &[u8]) -> bool,
        report: &mut NetworkTickReport,
    ) {
        let Some(transfer) = self.snapshot_transfers.get(&source) else {
            report.invalid_snapshot_controls += 1;
            return;
        };
        let base = usize::from(base_fragment);
        if snapshot_id == 0
            || transfer.snapshot_id != snapshot_id
            || base % 64 != 0
            || missing_mask == 0
        {
            report.invalid_snapshot_controls += 1;
            return;
        }
        let requested = usize::try_from(missing_mask.count_ones()).unwrap_or(64);
        if (0..64).any(|offset| {
            missing_mask & (1_u64 << offset) != 0
                && base.saturating_add(offset) >= transfer.frames.len()
        }) {
            report.invalid_snapshot_controls += 1;
            return;
        }
        if send_selected_snapshot_frames(
            sender,
            &transfer.frames,
            source,
            base,
            missing_mask,
            report,
        ) {
            report.snapshot_fragment_requests_served += 1;
            report.snapshot_fragments_retransmitted = report
                .snapshot_fragments_retransmitted
                .saturating_add(requested);
        }
    }

    fn process_snapshot_ack(
        &mut self,
        source: PeerId,
        snapshot_id: u64,
        report: &mut NetworkTickReport,
    ) {
        let Some(transfer) = self.snapshot_transfers.get_mut(&source) else {
            report.invalid_snapshot_controls += 1;
            return;
        };
        if snapshot_id == 0 || transfer.snapshot_id != snapshot_id {
            report.invalid_snapshot_controls += 1;
            return;
        }
        match transfer.stage {
            SnapshotTransferStage::AwaitingAck => {
                transfer.stage = SnapshotTransferStage::Catchup {
                    next_sequence: transfer.snapshot_next_sequence,
                };
                report.snapshot_acks_accepted += 1;
            }
            SnapshotTransferStage::Catchup { .. } => {
                report.snapshot_acks_accepted += 1;
            }
            SnapshotTransferStage::Snapshot { .. } | SnapshotTransferStage::Stalled => {
                report.invalid_snapshot_controls += 1;
            }
        }
    }

    fn service_snapshot_transfers(
        &mut self,
        sender: &mut impl FnMut(PeerId, &[u8]) -> bool,
        report: &mut NetworkTickReport,
    ) {
        let sources = self.snapshot_transfers.keys().copied().collect::<Vec<_>>();
        for source in sources {
            let Some(mut transfer) = self.snapshot_transfers.remove(&source) else {
                continue;
            };
            let keep = match transfer.stage {
                SnapshotTransferStage::Snapshot { next_frame } => {
                    let end = next_frame
                        .saturating_add(MAX_SNAPSHOT_FRAMES_PER_PEER_PER_TICK)
                        .min(transfer.frames.len());
                    if send_packet_frames(sender, &transfer.frames[next_frame..end], source, report)
                    {
                        if end == transfer.frames.len() {
                            transfer.stage = SnapshotTransferStage::AwaitingAck;
                            report.snapshot_fallbacks_served += 1;
                        } else {
                            transfer.stage = SnapshotTransferStage::Snapshot { next_frame: end };
                        }
                    }
                    true
                }
                SnapshotTransferStage::Catchup { next_sequence } => {
                    if let Some(catchup) = transfer.catchup.front() {
                        if catchup.sequence != next_sequence {
                            transfer.stage = SnapshotTransferStage::Stalled;
                            transfer.catchup.clear();
                            transfer.catchup_bytes = 0;
                            report.snapshot_catchup_stalls += 1;
                        } else if send_packet_frames(sender, &catchup.frames, source, report) {
                            if let Some(sent) = transfer.catchup.pop_front() {
                                transfer.catchup_bytes =
                                    transfer.catchup_bytes.saturating_sub(sent.bytes);
                            }
                            if let Some(next_sequence) = next_sequence.checked_add(1) {
                                transfer.stage = SnapshotTransferStage::Catchup { next_sequence };
                            } else {
                                transfer.stage = SnapshotTransferStage::Stalled;
                                transfer.catchup.clear();
                                transfer.catchup_bytes = 0;
                                report.snapshot_catchup_stalls += 1;
                            }
                        }
                        true
                    } else if next_sequence >= self.authority.next_sequence() {
                        report.snapshot_catchups_completed += 1;
                        false
                    } else {
                        transfer.stage = SnapshotTransferStage::Stalled;
                        report.snapshot_catchup_stalls += 1;
                        true
                    }
                }
                SnapshotTransferStage::AwaitingAck | SnapshotTransferStage::Stalled => true,
            };
            if keep {
                self.snapshot_transfers.insert(source, transfer);
            }
        }
    }

    fn simulate_commands(
        &mut self,
        sender: &mut impl FnMut(PeerId, &[u8]) -> bool,
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
                    self.broadcast(&packet, sender, report)?;
                }
                Err(_error) => report.commands_rejected += 1,
            }
        }
        Ok(())
    }

    fn broadcast(
        &mut self,
        packet: &DeltaPacket,
        sender: &mut impl FnMut(PeerId, &[u8]) -> bool,
        report: &mut NetworkTickReport,
    ) -> Result<(), NetworkRuntimeError> {
        let frames: Arc<[Vec<u8>]> = encode_frames(packet, self.application_datagram_bytes)?.into();
        for destination in self
            .peers
            .keys()
            .filter(|destination| !self.snapshot_transfers.contains_key(destination))
        {
            send_packet_frames(sender, &frames, *destination, report);
        }
        self.queue_snapshot_catchup(packet.sequence, &frames, report);
        self.retain_delta(packet.sequence, frames);
        Ok(())
    }

    fn queue_snapshot_catchup(
        &mut self,
        sequence: u64,
        frames: &Arc<[Vec<u8>]>,
        report: &mut NetworkTickReport,
    ) {
        let bytes = frames.iter().map(Vec::len).sum::<usize>();
        for transfer in self.snapshot_transfers.values_mut() {
            if matches!(transfer.stage, SnapshotTransferStage::Stalled) {
                continue;
            }
            let expected = transfer.catchup.back().map_or_else(
                || {
                    Some(match transfer.stage {
                        SnapshotTransferStage::Snapshot { .. }
                        | SnapshotTransferStage::AwaitingAck => transfer.snapshot_next_sequence,
                        SnapshotTransferStage::Catchup { next_sequence } => next_sequence,
                        SnapshotTransferStage::Stalled => sequence,
                    })
                },
                |last| last.sequence.checked_add(1),
            );
            let Some(expected) = expected else {
                transfer.stage = SnapshotTransferStage::Stalled;
                transfer.catchup.clear();
                transfer.catchup_bytes = 0;
                report.snapshot_catchup_stalls += 1;
                continue;
            };
            if sequence != expected
                || transfer.catchup.len() >= MAX_SNAPSHOT_CATCHUP_PACKETS
                || transfer.catchup_bytes.saturating_add(bytes) > MAX_SNAPSHOT_CATCHUP_BYTES
            {
                transfer.stage = SnapshotTransferStage::Stalled;
                transfer.catchup.clear();
                transfer.catchup_bytes = 0;
                report.snapshot_catchup_stalls += 1;
                continue;
            }
            transfer.catchup.push_back(CatchupDelta {
                sequence,
                frames: Arc::clone(frames),
                bytes,
            });
            transfer.catchup_bytes = transfer.catchup_bytes.saturating_add(bytes);
        }
    }

    fn retain_delta(&mut self, sequence: u64, frames: Arc<[Vec<u8>]>) {
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
        let mut expired = Vec::with_capacity(self.peers.len());
        self.peers.retain(|address, peer| {
            let keep = peer.last_seen_tick >= earliest;
            if !keep {
                expired.push((*address, peer.session_id));
            }
            keep
        });
        for (source, session_id) in expired {
            self.snapshot_transfers.remove(&source);
            self.repairs
                .retain(|repair| repair.source != source && repair.session_id != session_id);
            self.commands
                .retain(|command| command.session_id != session_id);
        }
        self.snapshot_transfers
            .retain(|address, _transfer| self.peers.contains_key(address));
    }
}

/// Legacy real-UDP adapter retained for loopback protocol and impairment testing.
pub struct DedicatedServer {
    socket: UdpSocket,
    core: AuthorityCore<SocketAddr>,
}

impl DedicatedServer {
    /// Binds a nonblocking UDP authority. This unauthenticated adapter is always loopback-only.
    ///
    /// # Errors
    ///
    /// Returns socket resolution, bind, nonblocking-configuration, or core-configuration failures.
    pub fn bind(address: impl ToSocketAddrs, world: World) -> io::Result<Self> {
        let socket = UdpSocket::bind(address)?;
        if !socket.local_addr()?.ip().is_loopback() {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "unauthenticated dedicated transport is restricted to loopback",
            ));
        }
        socket.set_nonblocking(true)?;
        let core = AuthorityCore::new(world, LEGACY_UDP_APPLICATION_DATAGRAM_BYTES)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error.to_string()))?;
        Ok(Self { socket, core })
    }

    /// Receives a bounded batch, then advances the transport-independent authority core once.
    ///
    /// # Errors
    ///
    /// Returns non-transient socket failures or authoritative frame-encoding failures.
    pub fn tick(&mut self) -> Result<NetworkTickReport, NetworkRuntimeError> {
        let Self { socket, core } = self;
        let mut report = core.begin_tick();
        let mut sender = |destination: SocketAddr, payload: &[u8]| matches!(socket.send_to(payload, destination), Ok(length) if length == payload.len());
        let mut datagram = [0_u8; MAX_APPLICATION_DATAGRAM_BYTES + 1];
        for _ in 0..MAX_RECEIVED_DATAGRAMS_PER_TICK {
            let (length, source) = match socket.recv_from(&mut datagram) {
                Ok(received) => received,
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
                Err(error) => return Err(NetworkRuntimeError::Io(error)),
            };
            core.ingest_datagram(source, &datagram[..length], &mut sender, &mut report);
        }
        core.complete_tick(&mut sender, report)
    }

    #[must_use]
    pub const fn authority(&self) -> &AuthoritativeServer {
        self.core.authority()
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
        self.core.peer_count()
    }

    #[must_use]
    pub fn queued_commands(&self) -> usize {
        self.core.queued_commands()
    }

    #[must_use]
    pub fn retained_delta_packets(&self) -> usize {
        self.core.retained_delta_packets()
    }

    #[must_use]
    pub const fn retained_delta_bytes(&self) -> usize {
        self.core.retained_delta_bytes()
    }
}

fn send_welcome<PeerId: Copy>(
    sender: &mut impl FnMut(PeerId, &[u8]) -> bool,
    destination: PeerId,
    peer: Peer,
    report: &mut NetworkTickReport,
) {
    let message = encode_server_welcome(peer.nonce, peer.session_id);
    if report.outbound_attempts >= MAX_OUTBOUND_DATAGRAMS_PER_TICK {
        report.outbound_drops += 1;
        return;
    }
    report.outbound_attempts += 1;
    if sender(destination, &message) {
        report.outbound_datagrams += 1;
    } else {
        report.outbound_drops += 1;
    }
}

fn send_packet_frames<PeerId: Copy>(
    sender: &mut impl FnMut(PeerId, &[u8]) -> bool,
    frames: &[Vec<u8>],
    destination: PeerId,
    report: &mut NetworkTickReport,
) -> bool {
    if frames.len() > MAX_OUTBOUND_DATAGRAMS_PER_TICK.saturating_sub(report.outbound_attempts) {
        report.outbound_drops = report.outbound_drops.saturating_add(frames.len());
        return false;
    }
    let mut complete = true;
    for frame in frames {
        report.outbound_attempts += 1;
        if sender(destination, frame) {
            report.outbound_datagrams += 1;
        } else {
            report.outbound_drops += 1;
            complete = false;
        }
    }
    complete
}

fn send_selected_snapshot_frames<PeerId: Copy>(
    sender: &mut impl FnMut(PeerId, &[u8]) -> bool,
    frames: &[Vec<u8>],
    destination: PeerId,
    base: usize,
    missing_mask: u64,
    report: &mut NetworkTickReport,
) -> bool {
    let requested = usize::try_from(missing_mask.count_ones()).unwrap_or(64);
    if requested > MAX_OUTBOUND_DATAGRAMS_PER_TICK.saturating_sub(report.outbound_attempts) {
        report.outbound_drops = report.outbound_drops.saturating_add(requested);
        return false;
    }
    let mut complete = true;
    for offset in 0..64 {
        if missing_mask & (1_u64 << offset) == 0 {
            continue;
        }
        let Some(frame) = frames.get(base.saturating_add(offset)) else {
            return false;
        };
        report.outbound_attempts += 1;
        if sender(destination, frame) {
            report.outbound_datagrams += 1;
        } else {
            report.outbound_drops += 1;
            complete = false;
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
        let first = encode_frames(&empty_packet(1), LEGACY_UDP_APPLICATION_DATAGRAM_BYTES)
            .expect("first packet")
            .remove(0);
        let second = encode_frames(&empty_packet(2), LEGACY_UDP_APPLICATION_DATAGRAM_BYTES)
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
    fn authority_core_rejects_invalid_payload_bounds() {
        for bound in [
            MIN_APPLICATION_DATAGRAM_BYTES - 1,
            MAX_APPLICATION_DATAGRAM_BYTES + 1,
        ] {
            assert!(matches!(
                AuthorityCore::<u8>::new(World::default(), bound),
                Err(NetworkRuntimeError::InvalidApplicationDatagramBytes(bytes)) if bytes == bound
            ));
        }
    }

    #[test]
    fn authenticated_peer_is_bound_and_wire_hello_cannot_replace_it() {
        let mut server = AuthorityCore::new(World::default(), 1_100).expect("authority core");
        let principal = AuthenticatedPrincipal::new(
            std::num::NonZeroU64::new(77).expect("non-zero test principal"),
        );
        let mut report = server.begin_tick();
        assert!(server.admit_authenticated(5_u64, 11, 13, principal, &mut report));
        assert_eq!(server.principal(5), Some(principal));
        assert!(!server.admit_authenticated(5, 12, 14, principal, &mut report));
        assert!(!server.admit_authenticated(6, 12, 13, principal, &mut report));

        let mut sent = Vec::new();
        server.ingest_datagram(
            5,
            &crate::encode_client_hello(99),
            &mut |destination, payload| {
                sent.push((destination, payload.to_vec()));
                true
            },
            &mut report,
        );
        server.ingest_datagram(
            5,
            &crate::encode_explosion_request(13, TEST_COMMAND),
            &mut |_destination, _payload| true,
            &mut report,
        );

        assert!(sent.is_empty());
        assert_eq!(report.rejected_sessions, 3);
        assert_eq!(server.queued_commands(), 1);
        assert!(!server.disconnect_authenticated(5, 14));
        assert!(server.disconnect_authenticated(5, 13));
        assert_eq!(server.principal(5), None);
        assert_eq!(server.queued_commands(), 0);
    }

    #[test]
    fn core_receive_work_and_secure_sized_output_are_bounded() {
        let mut server = AuthorityCore::new(crate::demo_world(), 1_100).expect("authority core");
        let principal = AuthenticatedPrincipal::new(
            std::num::NonZeroU64::new(91).expect("non-zero test principal"),
        );
        let mut report = server.begin_tick();
        assert!(server.admit_authenticated(1_u8, 17, 19, principal, &mut report));
        server.ingest_datagram(
            1,
            &crate::encode_explosion_request(19, TEST_COMMAND),
            &mut |_destination, _payload| true,
            &mut report,
        );
        for _ in 1..MAX_RECEIVED_DATAGRAMS_PER_TICK + 2 {
            server.ingest_datagram(1, &[], &mut |_destination, _payload| true, &mut report);
        }
        let mut output_lengths = Vec::new();
        let report = server
            .complete_tick(
                &mut |destination, payload| {
                    assert_eq!(destination, 1);
                    output_lengths.push(payload.len());
                    true
                },
                report,
            )
            .expect("bounded authority tick");

        assert_eq!(report.received_datagrams, MAX_RECEIVED_DATAGRAMS_PER_TICK);
        assert_eq!(report.receive_limit_drops, 2);
        assert_eq!(report.commands_applied, 1);
        assert!(!output_lengths.is_empty());
        assert!(output_lengths.into_iter().all(|bytes| bytes <= 1_100));
    }

    #[test]
    fn command_queue_and_session_validation_are_bounded() {
        let mut server =
            AuthorityCore::new(World::default(), LEGACY_UDP_APPLICATION_DATAGRAM_BYTES)
                .expect("server");
        let source = SocketAddr::from(([127, 0, 0, 1], 20_001));
        server.peers.insert(
            source,
            Peer {
                nonce: 1,
                session_id: 7,
                principal: None,
                last_seen_tick: 0,
                last_snapshot_tick: None,
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
        let mut server =
            AuthorityCore::new(World::default(), LEGACY_UDP_APPLICATION_DATAGRAM_BYTES)
                .expect("server");
        let mut report = NetworkTickReport::default();
        let mut sender = |_destination, _payload: &[u8]| true;

        for index in 0..=MAX_SERVER_PEERS {
            let index = u16::try_from(index).expect("small peer limit");
            let source = SocketAddr::from(([127, 0, 0, 1], 21_000 + index));
            server.accept_hello(source, u64::from(index), &mut sender, &mut report);
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
        let mut server =
            AuthorityCore::new(World::default(), LEGACY_UDP_APPLICATION_DATAGRAM_BYTES)
                .expect("server");
        let destination = SocketAddr::from(([127, 0, 0, 1], 22_001));
        server.peers.insert(
            destination,
            Peer {
                nonce: 1,
                session_id: 1,
                principal: None,
                last_seen_tick: 0,
                last_snapshot_tick: None,
            },
        );
        let mut report = NetworkTickReport {
            outbound_attempts: MAX_OUTBOUND_DATAGRAMS_PER_TICK,
            ..NetworkTickReport::default()
        };

        server
            .broadcast(
                &empty_packet(1),
                &mut |_destination, _payload: &[u8]| true,
                &mut report,
            )
            .expect("valid packet encoding");

        assert_eq!(report.outbound_attempts, MAX_OUTBOUND_DATAGRAMS_PER_TICK);
        assert_eq!(report.outbound_datagrams, 0);
        assert_eq!(report.outbound_drops, 1);
    }

    #[test]
    fn repair_queue_and_retained_history_are_bounded() {
        let mut server =
            AuthorityCore::new(World::default(), LEGACY_UDP_APPLICATION_DATAGRAM_BYTES)
                .expect("server");
        let source = SocketAddr::from(([127, 0, 0, 1], 23_001));
        server.peers.insert(
            source,
            Peer {
                nonce: 1,
                session_id: 7,
                principal: None,
                last_seen_tick: 0,
                last_snapshot_tick: None,
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
                vec![vec![0]].into(),
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

    #[test]
    fn snapshot_catchup_queue_fails_closed_at_its_packet_limit() {
        let mut server =
            AuthorityCore::new(World::default(), LEGACY_UDP_APPLICATION_DATAGRAM_BYTES)
                .expect("server");
        let source = SocketAddr::from(([127, 0, 0, 1], 24_001));
        server.snapshot_transfers.insert(
            source,
            SnapshotTransfer {
                snapshot_id: 1,
                frames: vec![vec![0]].into(),
                snapshot_next_sequence: 1,
                stage: SnapshotTransferStage::Snapshot { next_frame: 0 },
                catchup: VecDeque::new(),
                catchup_bytes: 0,
            },
        );
        let frame: Arc<[Vec<u8>]> = vec![vec![0]].into();
        let mut report = NetworkTickReport::default();

        for sequence in 1..=MAX_SNAPSHOT_CATCHUP_PACKETS + 1 {
            server.queue_snapshot_catchup(
                u64::try_from(sequence).expect("small catch-up limit"),
                &frame,
                &mut report,
            );
        }

        let transfer = server.snapshot_transfers.get(&source).expect("transfer");
        assert!(matches!(transfer.stage, SnapshotTransferStage::Stalled));
        assert!(transfer.catchup.is_empty());
        assert_eq!(transfer.catchup_bytes, 0);
        assert_eq!(report.snapshot_catchup_stalls, 1);
    }

    #[test]
    fn snapshot_controls_require_matching_bounded_transfer_state() {
        let mut server =
            AuthorityCore::new(World::default(), LEGACY_UDP_APPLICATION_DATAGRAM_BYTES)
                .expect("server");
        let source = SocketAddr::from(([127, 0, 0, 1], 24_002));
        server.snapshot_transfers.insert(
            source,
            SnapshotTransfer {
                snapshot_id: 5,
                frames: vec![vec![0]].into(),
                snapshot_next_sequence: 3,
                stage: SnapshotTransferStage::AwaitingAck,
                catchup: VecDeque::new(),
                catchup_bytes: 0,
            },
        );
        let mut report = NetworkTickReport::default();

        let mut sender = |_destination, _payload: &[u8]| true;
        server.process_snapshot_fragments(source, 5, 0, 1, &mut sender, &mut report);
        server.process_snapshot_fragments(source, 5, 1, 1, &mut sender, &mut report);
        server.process_snapshot_ack(source, 4, &mut report);
        server.process_snapshot_ack(source, 5, &mut report);

        let transfer = server.snapshot_transfers.get(&source).expect("transfer");
        assert!(matches!(
            transfer.stage,
            SnapshotTransferStage::Catchup { next_sequence: 3 }
        ));
        assert_eq!(report.snapshot_fragment_requests_served, 1);
        assert_eq!(report.snapshot_fragments_retransmitted, 1);
        assert_eq!(report.snapshot_acks_accepted, 1);
        assert_eq!(report.invalid_snapshot_controls, 2);
    }

    #[test]
    fn future_repair_sequence_cannot_force_a_snapshot() {
        let mut server =
            AuthorityCore::new(World::default(), LEGACY_UDP_APPLICATION_DATAGRAM_BYTES)
                .expect("server");
        let source = SocketAddr::from(([127, 0, 0, 1], 25_001));
        server.peers.insert(
            source,
            Peer {
                nonce: 1,
                session_id: 7,
                principal: None,
                last_seen_tick: 0,
                last_snapshot_tick: None,
            },
        );
        let mut report = NetworkTickReport::default();

        server.enqueue_repair(source, 7, 2, &mut report);
        server.process_repairs(&mut |_destination, _payload: &[u8]| true, &mut report);

        assert_eq!(report.repair_misses, 1);
        assert_eq!(report.snapshot_request_drops, 1);
        assert!(server.snapshot_transfers.is_empty());
    }
}
