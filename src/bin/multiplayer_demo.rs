#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss
)]

use destructible_fps::{
    AdaptiveRepairTimer, BuildCommand, CHUNK_EDGE, ClientPrediction, ClientPredictionError,
    ClientReplica, ExplosionCommand, FixedMicrometers3, IVec3, MAX_APPLICATION_DATAGRAM_BYTES,
    MAX_BUILD_REACH_VOXELS, MAX_RECEIVED_DATAGRAMS_PER_TICK, MICROMETERS_PER_VOXEL, Material,
    OrderedDeltaInbox, PlayerInputCommand, PlayerInterpolationBuffer, PlayerInterpolationError,
    PlayerStateReceiveError, SecureClientConnection, SecureClientLaunchConfig, SecureDatagramInbox,
    ServerControlMessage, SnapshotAssembler, decode_frame, decode_player_state_packet,
    decode_server_control, demo_world, dirty_chunks, encode_build_request, encode_client_hello,
    encode_explosion_request, encode_player_input, encode_repair_request, encode_snapshot_ack,
    encode_snapshot_fragments_request, encode_snapshot_request, is_delta_datagram,
    is_player_state_datagram, is_snapshot_datagram,
    mesh::{mesh_body, mesh_chunk},
    mesh_scheduler::{
        CompletedMeshJob, MAX_BODIES_PER_MESH_JOB, MAX_BODY_VOXELS_PER_MESH_JOB,
        MAX_CHUNKS_PER_MESH_JOB, MeshScheduler,
    },
    player::{Player, raycast},
    render::{RenderOutcome, Renderer},
};
use glam::Vec3;
use std::{
    collections::{BTreeSet, HashSet},
    error::Error,
    io,
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, UdpSocket},
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};
use winit::{
    application::ApplicationHandler,
    dpi::LogicalSize,
    event::{DeviceEvent, DeviceId, ElementState, MouseButton, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    keyboard::{KeyCode, PhysicalKey},
    window::{CursorGrabMode, Window, WindowId},
};

const FIXED_STEP_SECONDS: f32 = 1.0 / 60.0;
const DEFAULT_SERVER: &str = "127.0.0.1:40000";
const VISUAL_CORRECTION_HALF_LIFE_SECONDS: f32 = 0.08;
const MAX_SMOOTHED_CORRECTION_VOXELS: f32 = 2.0;
const MAX_PENDING_NETWORK_MESH_CHUNKS: usize = 512;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SnapshotPhase {
    Awaiting,
    Ready,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum MeshWorkerPhase {
    Idle,
    InFlight,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DeltaImpairment {
    None,
    DropUntilFuture { sequence: u64 },
    Repairing { sequence: u64 },
    Completed { sequence: u64 },
}

#[derive(Clone, Debug)]
enum TransportOptions {
    Loopback(SocketAddr),
    Secure(SecureClientLaunchConfig),
}

enum GameTransport {
    Loopback {
        socket: UdpSocket,
        server: SocketAddr,
        nonce: u64,
        last_hello_at: Instant,
    },
    Secure {
        _runtime: tokio::runtime::Runtime,
        client: SecureClientConnection,
        inbox: SecureDatagramInbox,
    },
}

impl GameTransport {
    fn connect(options: &TransportOptions) -> Result<Self, String> {
        match options {
            TransportOptions::Loopback(server) => {
                let bind = match server.ip() {
                    IpAddr::V4(_) => SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0),
                    IpAddr::V6(_) => SocketAddr::new(IpAddr::V6(Ipv6Addr::LOCALHOST), 0),
                };
                let socket = UdpSocket::bind(bind)
                    .map_err(|error| format!("socket client loopback: {error}"))?;
                socket
                    .set_nonblocking(true)
                    .map_err(|error| format!("socket non bloquante: {error}"))?;
                let local_port = u64::from(
                    socket
                        .local_addr()
                        .map_err(|error| format!("adresse client: {error}"))?
                        .port(),
                );
                let nonce = (u64::from(std::process::id()) << 16 | local_port).max(1);
                socket
                    .send_to(&encode_client_hello(nonce), server)
                    .map_err(|error| format!("handshake initial: {error}"))?;
                Ok(Self::Loopback {
                    socket,
                    server: *server,
                    nonce,
                    last_hello_at: Instant::now(),
                })
            }
            TransportOptions::Secure(config) => {
                let runtime = tokio::runtime::Builder::new_multi_thread()
                    .worker_threads(2)
                    .enable_all()
                    .thread_name("secure-game-client")
                    .build()
                    .map_err(|error| format!("runtime client QUIC: {error}"))?;
                let client = runtime
                    .block_on(SecureClientConnection::connect(config))
                    .map_err(|error| format!("connexion multijoueur securisee: {error}"))?;
                let inbox = client.spawn_datagram_inbox(runtime.handle());
                Ok(Self::Secure {
                    _runtime: runtime,
                    client,
                    inbox,
                })
            }
        }
    }

    const fn session_id(&self) -> Option<u64> {
        match self {
            Self::Loopback { .. } => None,
            Self::Secure { client, .. } => Some(client.session_id()),
        }
    }

    fn description(&self) -> Result<String, String> {
        match self {
            Self::Loopback { socket, server, .. } => Ok(format!(
                "loopback {} -> {server}",
                socket
                    .local_addr()
                    .map_err(|error| format!("adresse client: {error}"))?
            )),
            Self::Secure { client, .. } => Ok(format!(
                "QUIC/TLS session {} nonce serveur {}",
                client.session_id(),
                client.server_nonce()
            )),
        }
    }

    fn drain(&self) -> Result<Vec<Vec<u8>>, String> {
        let mut datagrams = Vec::new();
        match self {
            Self::Loopback { socket, server, .. } => {
                let mut bytes = [0_u8; MAX_APPLICATION_DATAGRAM_BYTES + 1];
                for _ in 0..MAX_RECEIVED_DATAGRAMS_PER_TICK {
                    let (length, source) = match socket.recv_from(&mut bytes) {
                        Ok(received) => received,
                        Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
                        Err(error) => return Err(format!("reception multijoueur: {error}")),
                    };
                    if source == *server {
                        datagrams.push(bytes[..length].to_vec());
                    }
                }
            }
            Self::Secure { inbox, .. } => {
                for _ in 0..MAX_RECEIVED_DATAGRAMS_PER_TICK {
                    let Some(payload) = inbox
                        .try_receive()
                        .map_err(|error| format!("reception QUIC: {error}"))?
                    else {
                        break;
                    };
                    datagrams.push(payload);
                }
            }
        }
        Ok(datagrams)
    }

    fn send(&self, payload: Vec<u8>) -> Result<(), String> {
        match self {
            Self::Loopback { socket, server, .. } => socket
                .send_to(&payload, server)
                .map(|_bytes| ())
                .map_err(|error| format!("envoi UDP: {error}")),
            Self::Secure { client, .. } => client
                .send(payload)
                .map_err(|error| format!("envoi QUIC: {error}")),
        }
    }

    fn retry_legacy_handshake(&mut self, session_missing: bool) -> Result<(), String> {
        let Self::Loopback {
            socket,
            server,
            nonce,
            last_hello_at,
        } = self
        else {
            return Ok(());
        };
        if session_missing && last_hello_at.elapsed() >= Duration::from_secs(1) {
            socket
                .send_to(&encode_client_hello(*nonce), *server)
                .map_err(|error| format!("nouvelle tentative de handshake: {error}"))?;
            *last_hello_at = Instant::now();
        }
        Ok(())
    }

    const fn expected_legacy_nonce(&self) -> Option<u64> {
        match self {
            Self::Loopback { nonce, .. } => Some(*nonce),
            Self::Secure { .. } => None,
        }
    }

    fn dropped_datagrams(&self) -> u64 {
        match self {
            Self::Loopback { .. } => 0,
            Self::Secure { inbox, .. } => inbox.dropped_datagrams(),
        }
    }
}

struct MultiplayerGame {
    window: Arc<Window>,
    renderer: Renderer,
    replica: ClientReplica,
    delta_inbox: OrderedDeltaInbox,
    delta_gap_since: Option<Instant>,
    delta_repair_timer: AdaptiveRepairTimer,
    delta_repairs_sent: u64,
    delta_impairment: DeltaImpairment,
    mesh_scheduler: MeshScheduler,
    mesh_snapshot: Arc<destructible_fps::World>,
    pending_mesh_chunks: HashSet<IVec3>,
    pending_body_ids: BTreeSet<destructible_fps::BodyId>,
    mesh_worker_phase: MeshWorkerPhase,
    completed_mesh_jobs: u64,
    snapshot_assembler: SnapshotAssembler,
    snapshot_phase: SnapshotPhase,
    last_snapshot_request_at: Option<Instant>,
    last_snapshot_progress_at: Option<Instant>,
    last_snapshot_repair_at: Option<Instant>,
    pristine_world_fingerprint: u128,
    transport: GameTransport,
    session_id: Option<u64>,
    prediction: Option<ClientPrediction>,
    interpolation: PlayerInterpolationBuffer,
    latest_state_received_at: Option<Instant>,
    next_input_sequence: u64,
    next_command_id: u64,
    view: Player,
    visual_correction: Vec3,
    pressed: HashSet<KeyCode>,
    cursor_captured: bool,
    previous_frame: Instant,
    started: Instant,
    accumulator: f32,
    last_status: String,
    smoke_motion: bool,
    initial_position_um: Option<FixedMicrometers3>,
    maximum_horizontal_displacement_um: u64,
    applied_world_deltas: u64,
    smoke_actions_sent: u8,
}

impl MultiplayerGame {
    fn new(
        window: Arc<Window>,
        transport_options: &TransportOptions,
        smoke_motion: bool,
        smoke_drop_first_delta: bool,
        msaa: u32,
    ) -> Result<Self, String> {
        let transport = GameTransport::connect(transport_options)?;
        let session_id = transport.session_id();
        let transport_description = transport.description()?;
        let world = demo_world();
        let meshes = world
            .chunk_positions()
            .into_iter()
            .map(|chunk| (chunk, mesh_chunk(&world, chunk)))
            .collect();
        let mut renderer = pollster::block_on(Renderer::with_msaa(Arc::clone(&window), msaa))?;
        renderer.upload_chunk_meshes(meshes);
        let now = Instant::now();
        println!(
            "Client multijoueur {transport_description}; {} chunks charges",
            renderer.stats().chunks
        );
        println!(
            "Commandes: clic pour capturer | ZQSD/WASD | Maj sprint | Espace saut | clic gauche/droit destruction | molette construction | Echap"
        );
        let mut game = Self {
            window,
            renderer,
            pristine_world_fingerprint: world.fingerprint(),
            mesh_snapshot: Arc::new(world.clone()),
            replica: ClientReplica::new(world),
            delta_inbox: OrderedDeltaInbox::default(),
            delta_gap_since: None,
            delta_repair_timer: AdaptiveRepairTimer::default(),
            delta_repairs_sent: 0,
            delta_impairment: if smoke_drop_first_delta {
                DeltaImpairment::DropUntilFuture { sequence: 1 }
            } else {
                DeltaImpairment::None
            },
            mesh_scheduler: MeshScheduler::new(),
            pending_mesh_chunks: HashSet::new(),
            pending_body_ids: BTreeSet::new(),
            mesh_worker_phase: MeshWorkerPhase::Idle,
            completed_mesh_jobs: 0,
            snapshot_assembler: SnapshotAssembler::default(),
            snapshot_phase: SnapshotPhase::Awaiting,
            last_snapshot_request_at: None,
            last_snapshot_progress_at: None,
            last_snapshot_repair_at: None,
            transport,
            session_id,
            prediction: None,
            interpolation: PlayerInterpolationBuffer::default(),
            latest_state_received_at: None,
            next_input_sequence: 1,
            next_command_id: 1,
            view: Player::default(),
            visual_correction: Vec3::ZERO,
            pressed: HashSet::new(),
            cursor_captured: false,
            previous_frame: now,
            started: now,
            accumulator: 0.0,
            last_status: if session_id.is_some() {
                "session securisee etablie".to_owned()
            } else {
                "connexion au serveur".to_owned()
            },
            smoke_motion,
            initial_position_um: None,
            maximum_horizontal_displacement_um: 0,
            applied_world_deltas: 0,
            smoke_actions_sent: 0,
        };
        if let Some(session_id) = session_id {
            game.request_snapshot(session_id)?;
        }
        Ok(game)
    }

    fn snapshot_ready(&self) -> bool {
        self.snapshot_phase == SnapshotPhase::Ready
    }

    fn repair_timing_ms(&self) -> (u128, u128) {
        let rtt = self
            .delta_repair_timer
            .smoothed_rtt()
            .map_or(0, |duration| duration.as_millis());
        (
            rtt,
            self.delta_repair_timer.retransmission_timeout().as_millis(),
        )
    }

    fn validate_and_report_smoke(
        &self,
        remote_players: usize,
        transport_drops: u64,
    ) -> Result<(), String> {
        if self.session_id.is_none() || self.prediction.is_none() || !self.snapshot_ready() {
            return Err("smoke multijoueur termine sans session jouable".to_owned());
        }
        if self.maximum_horizontal_displacement_um < MICROMETERS_PER_VOXEL.cast_unsigned() {
            return Err(format!(
                "smoke multijoueur sans mouvement autoritaire suffisant: {} um",
                self.maximum_horizontal_displacement_um
            ));
        }
        if self.applied_world_deltas == 0
            && self.replica.world().fingerprint() == self.pristine_world_fingerprint
        {
            return Err(
                "smoke multijoueur sans destruction repliquee ni snapshot modifie".to_owned(),
            );
        }
        if self.smoke_actions_sent < 2 {
            return Err("smoke termine avant les deux actions autoritaires".to_owned());
        }
        if self.delta_impairment != DeltaImpairment::None
            && (!matches!(self.delta_impairment, DeltaImpairment::Completed { .. })
                || self.delta_repairs_sent == 0)
        {
            return Err("smoke termine sans reparer le delta volontairement perdu".to_owned());
        }
        if transport_drops != 0 {
            return Err(format!(
                "smoke termine avec {transport_drops} datagrammes perdus dans la file locale"
            ));
        }
        if self.applied_world_deltas > 0
            && (self.completed_mesh_jobs == 0
                || !self.pending_mesh_chunks.is_empty()
                || !self.pending_body_ids.is_empty()
                || self.mesh_worker_phase == MeshWorkerPhase::InFlight)
        {
            return Err("smoke termine avant la presentation du delta replique".to_owned());
        }
        let (round_trip_ms, retransmission_ms) = self.repair_timing_ms();
        println!(
            "SMOKE session={} joueurs_distants={} tick={} pending={} deplacement_um={} deltas_monde={} reparations={} rtt_ms={} rto_ms={} rtt_samples={} drops_transport={} jobs_mesh={} snapshot_pret={}",
            self.session_id.unwrap_or_default(),
            remote_players,
            self.prediction
                .as_ref()
                .map_or(0, ClientPrediction::last_server_tick),
            self.prediction
                .as_ref()
                .map_or(0, ClientPrediction::pending_inputs),
            self.maximum_horizontal_displacement_um,
            self.applied_world_deltas,
            self.delta_repairs_sent,
            round_trip_ms,
            retransmission_ms,
            self.delta_repair_timer.sample_count(),
            transport_drops,
            self.completed_mesh_jobs,
            self.snapshot_ready()
        );
        Ok(())
    }

    fn capture_cursor(&mut self) {
        let result = self
            .window
            .set_cursor_grab(CursorGrabMode::Locked)
            .or_else(|_| self.window.set_cursor_grab(CursorGrabMode::Confined));
        if result.is_ok() {
            self.window.set_cursor_visible(false);
            self.cursor_captured = true;
        }
    }

    fn release_cursor(&mut self) {
        let _result = self.window.set_cursor_grab(CursorGrabMode::None);
        self.window.set_cursor_visible(true);
        self.cursor_captured = false;
        self.pressed.clear();
    }

    fn pump_network(&mut self) -> Result<(), String> {
        let datagrams = self.transport.drain()?;
        for payload in datagrams {
            if is_player_state_datagram(&payload) {
                self.receive_player_state(&payload)?;
            } else if is_snapshot_datagram(&payload) {
                self.receive_snapshot(&payload)?;
            } else if is_delta_datagram(&payload) {
                self.receive_world_delta(&payload)?;
            } else if payload.starts_with(b"DFCT") {
                self.receive_control(&payload)?;
            }
        }
        self.transport
            .retry_legacy_handshake(self.session_id.is_none())?;
        self.repair_delta_if_needed()?;
        self.repair_snapshot_if_needed()?;
        Ok(())
    }

    fn repair_delta_if_needed(&mut self) -> Result<(), String> {
        if !self.snapshot_ready() {
            return Ok(());
        }
        let (Some(session_id), Some(gap_since)) = (self.session_id, self.delta_gap_since) else {
            return Ok(());
        };
        let now = Instant::now();
        let missing_sequence = self.delta_inbox.expected_sequence();
        let client_time = self.started.elapsed();
        if now.duration_since(gap_since) < self.delta_repair_timer.reorder_grace()
            || !self
                .delta_repair_timer
                .send_due(missing_sequence, client_time)
        {
            return Ok(());
        }
        self.transport
            .send(encode_repair_request(session_id, missing_sequence))
            .map_err(|error| format!("demande de reparation delta {missing_sequence}: {error}"))?;
        self.delta_repair_timer
            .record_send(missing_sequence, client_time);
        self.delta_repairs_sent = self.delta_repairs_sent.saturating_add(1);
        self.last_status = format!(
            "reparation du delta {missing_sequence} demandee | RTO {} ms",
            self.delta_repair_timer.retransmission_timeout().as_millis()
        );
        Ok(())
    }

    fn request_snapshot(&mut self, session_id: u64) -> Result<(), String> {
        self.transport
            .send(encode_snapshot_request(session_id))
            .map_err(|error| format!("demande de snapshot: {error}"))?;
        self.last_snapshot_request_at = Some(Instant::now());
        "synchronisation initiale du monde".clone_into(&mut self.last_status);
        Ok(())
    }

    fn repair_snapshot_if_needed(&mut self) -> Result<(), String> {
        let Some(session_id) = self.session_id else {
            return Ok(());
        };
        if self.snapshot_ready() {
            return Ok(());
        }
        let now = Instant::now();
        if self.snapshot_assembler.active_snapshot_id().is_none() {
            if self
                .last_snapshot_request_at
                .is_none_or(|requested| now.duration_since(requested) >= Duration::from_secs(1))
            {
                self.request_snapshot(session_id)?;
            }
            return Ok(());
        }
        let stalled = self
            .last_snapshot_progress_at
            .is_some_and(|progress| now.duration_since(progress) >= Duration::from_millis(500));
        let repair_due = self
            .last_snapshot_repair_at
            .is_none_or(|repair| now.duration_since(repair) >= Duration::from_millis(500));
        if stalled && repair_due {
            let Some(snapshot_id) = self.snapshot_assembler.active_snapshot_id() else {
                return Ok(());
            };
            for (base_fragment, missing_mask) in self.snapshot_assembler.missing_fragment_windows()
            {
                self.transport
                    .send(encode_snapshot_fragments_request(
                        session_id,
                        snapshot_id,
                        base_fragment,
                        missing_mask,
                    ))
                    .map_err(|error| format!("reparation de snapshot: {error}"))?;
            }
            self.last_snapshot_repair_at = Some(now);
        }
        Ok(())
    }

    fn receive_snapshot(&mut self, payload: &[u8]) -> Result<(), String> {
        let snapshot = self
            .snapshot_assembler
            .push(payload)
            .map_err(|error| format!("snapshot monde invalide: {error}"))?;
        self.last_snapshot_progress_at = Some(Instant::now());
        let Some(snapshot) = snapshot else {
            if self.snapshot_assembler.active_snapshot_id().is_some() {
                self.snapshot_phase = SnapshotPhase::Awaiting;
            }
            return Ok(());
        };
        let Some(session_id) = self.session_id else {
            return Err("snapshot recu avant admission".to_owned());
        };
        let snapshot_id = snapshot.snapshot_id();
        let next_sequence = snapshot.next_sequence();
        let mut chunk_positions = self
            .replica
            .world()
            .chunk_positions()
            .into_iter()
            .collect::<HashSet<_>>();
        snapshot
            .install_into(&mut self.replica)
            .map_err(|error| format!("installation du snapshot refusee: {error}"))?;
        self.delta_inbox = OrderedDeltaInbox::new(next_sequence);
        self.delta_gap_since = None;
        self.delta_repair_timer.clear_probe();
        chunk_positions.extend(self.replica.world().chunk_positions());
        let mut chunk_positions = chunk_positions.into_iter().collect::<Vec<_>>();
        chunk_positions.sort_unstable_by_key(|chunk| (chunk.x, chunk.y, chunk.z));
        let chunk_meshes = chunk_positions
            .into_iter()
            .map(|chunk| (chunk, mesh_chunk(self.replica.world(), chunk)))
            .collect();
        self.renderer.upload_chunk_meshes(chunk_meshes);
        self.renderer.clear_body_meshes();
        let body_meshes = self.replica.bodies().values().map(mesh_body).collect();
        self.renderer.upload_body_meshes(body_meshes)?;
        self.renderer
            .update_body_transforms(self.replica.body_states());
        self.mesh_snapshot = Arc::new(self.replica.world().clone());
        self.pending_mesh_chunks.clear();
        self.pending_body_ids.clear();
        self.transport
            .send(encode_snapshot_ack(session_id, snapshot_id))
            .map_err(|error| format!("acquittement du snapshot: {error}"))?;
        self.snapshot_phase = SnapshotPhase::Ready;
        self.last_status = format!(
            "snapshot {snapshot_id} installe | sequence {next_sequence} | {} corps",
            self.replica.bodies().len()
        );
        Ok(())
    }

    fn receive_world_delta(&mut self, payload: &[u8]) -> Result<(), String> {
        if !self.snapshot_ready() {
            return Ok(());
        }
        let frame_sequence = decode_frame(payload)
            .map_err(|error| format!("fragment delta invalide: {error}"))?
            .sequence;
        match self.delta_impairment {
            DeltaImpairment::DropUntilFuture { sequence } if frame_sequence == sequence => {
                return Ok(());
            }
            DeltaImpairment::DropUntilFuture { sequence } if frame_sequence > sequence => {
                self.delta_impairment = DeltaImpairment::Repairing { sequence };
            }
            DeltaImpairment::Repairing { sequence } if frame_sequence == sequence => {
                self.delta_impairment = DeltaImpairment::Completed { sequence };
            }
            _ => {}
        }
        let packets = self
            .delta_inbox
            .push(payload)
            .map_err(|error| format!("delta monde invalide: {error}"))?;
        let rtt_sample = self
            .delta_repair_timer
            .observe_sequence(self.delta_inbox.expected_sequence(), self.started.elapsed());
        let rtt_suffix = rtt_sample.map_or_else(String::new, |sample| {
            format!(" | RTT reparation {} ms", sample.as_millis())
        });
        if packets.is_empty() {
            if self.delta_inbox.buffered_complete_packets() > 0 {
                self.delta_gap_since.get_or_insert_with(Instant::now);
            }
        } else {
            self.delta_gap_since = None;
        }
        for packet in packets {
            let chunks = dirty_chunks(&packet.changes);
            let mut body_ids = packet
                .body_assignments
                .iter()
                .map(|assignment| assignment.body_id)
                .collect::<Vec<_>>();
            body_ids.sort_unstable();
            body_ids.dedup();
            self.replica
                .receive(&packet)
                .map_err(|error| format!("replication du monde refusee: {error}"))?;
            if !chunks.is_empty() {
                self.mesh_snapshot = Arc::new(self.replica.world().clone());
                self.pending_mesh_chunks.extend(chunks);
                if self.pending_mesh_chunks.len() > MAX_PENDING_NETWORK_MESH_CHUNKS {
                    return Err(format!(
                        "file de remeshing reseau saturee: {} chunks, maximum {MAX_PENDING_NETWORK_MESH_CHUNKS}",
                        self.pending_mesh_chunks.len()
                    ));
                }
            }
            if !body_ids.is_empty() {
                self.pending_body_ids.extend(body_ids);
            }
            self.renderer
                .update_body_transforms(self.replica.body_states());
            self.applied_world_deltas = self.applied_world_deltas.saturating_add(1);
            self.last_status = format!(
                "delta {} applique | {} corps{rtt_suffix}",
                packet.sequence,
                self.replica.bodies().len()
            );
        }
        Ok(())
    }

    fn pump_meshing(&mut self) -> Result<(), String> {
        match self.mesh_scheduler.poll() {
            Ok(Some(CompletedMeshJob::Chunks {
                world_fingerprint,
                meshes,
            })) => {
                self.mesh_worker_phase = MeshWorkerPhase::Idle;
                if world_fingerprint == self.replica.world().fingerprint() {
                    self.renderer.upload_chunk_meshes(meshes);
                    self.completed_mesh_jobs = self.completed_mesh_jobs.saturating_add(1);
                } else {
                    self.pending_mesh_chunks
                        .extend(meshes.into_iter().map(|(chunk, _mesh)| chunk));
                }
            }
            Ok(Some(CompletedMeshJob::Bodies(meshes))) => {
                self.mesh_worker_phase = MeshWorkerPhase::Idle;
                self.renderer.upload_body_meshes(meshes)?;
                self.renderer
                    .update_body_transforms(self.replica.body_states());
                self.completed_mesh_jobs = self.completed_mesh_jobs.saturating_add(1);
            }
            Ok(None) => {}
            Err(error) => return Err(format!("worker de remeshing arrete: {error}")),
        }
        if self.pending_mesh_chunks.len() > MAX_PENDING_NETWORK_MESH_CHUNKS {
            return Err(format!(
                "file de remeshing reseau saturee: {} chunks, maximum {MAX_PENDING_NETWORK_MESH_CHUNKS}",
                self.pending_mesh_chunks.len()
            ));
        }
        if self.mesh_worker_phase == MeshWorkerPhase::InFlight {
            return Ok(());
        }
        if self.queue_body_mesh_job()? {
            return Ok(());
        }
        self.queue_chunk_mesh_job()
    }

    fn queue_body_mesh_job(&mut self) -> Result<bool, String> {
        let mut bodies = Vec::new();
        let mut body_ids = Vec::new();
        let mut voxel_count = 0_usize;
        for &body_id in &self.pending_body_ids {
            let Some(body) = self.replica.bodies().get(&body_id) else {
                continue;
            };
            if bodies.len() == MAX_BODIES_PER_MESH_JOB {
                break;
            }
            let next_voxel_count = voxel_count.saturating_add(body.voxels.len());
            if next_voxel_count > MAX_BODY_VOXELS_PER_MESH_JOB {
                if bodies.is_empty() {
                    return Err(format!(
                        "corps {body_id} trop grand pour le worker: {} voxels",
                        body.voxels.len()
                    ));
                }
                break;
            }
            voxel_count = next_voxel_count;
            body_ids.push(body_id);
            bodies.push(body.clone());
        }
        if bodies.is_empty() {
            self.pending_body_ids
                .retain(|body_id| self.replica.bodies().contains_key(body_id));
            return Ok(false);
        }
        self.mesh_scheduler
            .submit_bodies(bodies)
            .map_err(|error| format!("maillage de corps non planifie: {error}"))?;
        for body_id in body_ids {
            self.pending_body_ids.remove(&body_id);
        }
        self.mesh_worker_phase = MeshWorkerPhase::InFlight;
        Ok(true)
    }

    fn queue_chunk_mesh_job(&mut self) -> Result<(), String> {
        if self.pending_mesh_chunks.is_empty() {
            return Ok(());
        }
        let focus = IVec3::new(
            (self.view.position.x / CHUNK_EDGE as f32).floor() as i32,
            (self.view.position.y / CHUNK_EDGE as f32).floor() as i32,
            (self.view.position.z / CHUNK_EDGE as f32).floor() as i32,
        );
        let mut chunks = self.pending_mesh_chunks.iter().copied().collect::<Vec<_>>();
        chunks.sort_unstable_by_key(|chunk| chunk.squared_distance(focus));
        chunks.truncate(MAX_CHUNKS_PER_MESH_JOB);
        self.mesh_scheduler
            .submit(Arc::clone(&self.mesh_snapshot), chunks.clone())
            .map_err(|error| format!("remeshing reseau non planifie: {error}"))?;
        for chunk in chunks {
            self.pending_mesh_chunks.remove(&chunk);
        }
        self.mesh_worker_phase = MeshWorkerPhase::InFlight;
        Ok(())
    }

    fn receive_control(&mut self, payload: &[u8]) -> Result<(), String> {
        let expected_nonce = self
            .transport
            .expected_legacy_nonce()
            .ok_or_else(|| "message de controle UDP recu sur transport QUIC".to_owned())?;
        let message = decode_server_control(payload)
            .map_err(|error| format!("reponse de handshake invalide: {error}"))?;
        match message {
            ServerControlMessage::Welcome { nonce, session_id } if nonce == expected_nonce => {
                if self.session_id.is_some_and(|current| current != session_id) {
                    return Err("le serveur a remplace une session active".to_owned());
                }
                let first_welcome = self.session_id.is_none();
                self.session_id = Some(session_id);
                if first_welcome {
                    self.request_snapshot(session_id)?;
                }
                Ok(())
            }
            ServerControlMessage::Welcome { .. } => {
                Err("le serveur a repondu avec un nonce inattendu".to_owned())
            }
        }
    }

    fn receive_player_state(&mut self, payload: &[u8]) -> Result<(), String> {
        let packet = decode_player_state_packet(payload)
            .map_err(|error| format!("etat joueur invalide: {error}"))?;
        match self.interpolation.push(payload) {
            Ok(_report) => self.latest_state_received_at = Some(Instant::now()),
            Err(PlayerInterpolationError::Receive(PlayerStateReceiveError::StaleServerTick {
                ..
            })) => return Ok(()),
            Err(error) => return Err(format!("historique joueur invalide: {error}")),
        }
        let Some(session_id) = self.session_id else {
            return Ok(());
        };
        let Some(authoritative) = packet
            .players
            .iter()
            .find(|player| player.session_id == session_id)
            .copied()
        else {
            return Err(format!(
                "session locale {session_id} absente de la vue serveur"
            ));
        };
        let visual_position_before_reconcile = self.view.position;
        let was_predicting = self.prediction.is_some();
        if let Some(prediction) = &mut self.prediction {
            match prediction.reconcile(packet.server_tick, authoritative, self.replica.world()) {
                Ok(report) => {
                    self.last_status = format!(
                        "tick {} | ack {} | rejeu {}",
                        report.server_tick,
                        authoritative.last_input_sequence,
                        report.replayed_inputs
                    );
                }
                Err(ClientPredictionError::StaleServerTick { .. }) => return Ok(()),
                Err(error) => return Err(format!("reconciliation refusee: {error}")),
            }
        } else {
            self.next_input_sequence = authoritative
                .last_input_sequence
                .checked_add(1)
                .ok_or_else(|| "sequence joueur epuisee".to_owned())?;
            self.prediction = Some(
                ClientPrediction::new(packet.server_tick, authoritative)
                    .map_err(|error| format!("prediction initiale: {error}"))?,
            );
        }
        if was_predicting {
            let predicted_position = fixed_to_world(
                self.prediction
                    .as_ref()
                    .ok_or_else(|| "prediction perdue pendant reconciliation".to_owned())?
                    .state()
                    .position_um,
            );
            self.visual_correction = continuity_correction(
                visual_position_before_reconcile,
                predicted_position,
                MAX_SMOOTHED_CORRECTION_VOXELS,
            );
        } else {
            self.visual_correction = Vec3::ZERO;
        }
        self.sync_local_view();
        Ok(())
    }

    fn fixed_tick(&mut self) -> Result<(), String> {
        let Some(session_id) = self.session_id else {
            return Ok(());
        };
        let mut input = movement_command(self.next_input_sequence, self.view.yaw, &self.pressed);
        if self.smoke_motion {
            if session_id.is_multiple_of(2) {
                input.movement_x_per_mille = 0;
                input.movement_z_per_mille = -1_000;
            } else {
                input.movement_x_per_mille = 1_000;
                input.movement_z_per_mille = 0;
            }
            input.sprint = true;
        }
        let Some(prediction) = &mut self.prediction else {
            return Ok(());
        };
        prediction
            .predict(input, self.replica.world())
            .map_err(|error| format!("prediction locale refusee: {error}"))?;
        self.transport
            .send(encode_player_input(session_id, input))
            .map_err(|error| format!("envoi input: {error}"))?;
        self.next_input_sequence = self
            .next_input_sequence
            .checked_add(1)
            .ok_or_else(|| "sequence joueur epuisee".to_owned())?;
        self.sync_local_view();
        Ok(())
    }

    fn send_explosion(&mut self, radius_voxels: u16, peak_energy: u32) -> Result<(), String> {
        if !self.snapshot_ready() {
            "action suspendue pendant la synchronisation".clone_into(&mut self.last_status);
            return Ok(());
        }
        let Some(session_id) = self.session_id else {
            return Ok(());
        };
        let Some(hit) = raycast(
            self.replica.world(),
            self.view.camera_position(),
            self.view.view_direction(),
            120.0,
        ) else {
            "tir sans impact".clone_into(&mut self.last_status);
            return Ok(());
        };
        let command_id = self.take_command_id()?;
        let command = ExplosionCommand {
            command_id,
            center: hit.voxel,
            radius_voxels,
            peak_energy,
        };
        self.transport
            .send(encode_explosion_request(session_id, command))
            .map_err(|error| format!("envoi destruction: {error}"))?;
        self.last_status = format!("destruction {command_id} envoyee en {:?}", hit.voxel);
        Ok(())
    }

    fn send_build(&mut self, material: Material) -> Result<(), String> {
        if !self.snapshot_ready() {
            "action suspendue pendant la synchronisation".clone_into(&mut self.last_status);
            return Ok(());
        }
        let Some(session_id) = self.session_id else {
            return Ok(());
        };
        let Some(position) = raycast(
            self.replica.world(),
            self.view.camera_position(),
            self.view.view_direction(),
            MAX_BUILD_REACH_VOXELS,
        )
        .and_then(|hit| hit.adjacent_empty) else {
            "construction sans support a portee".clone_into(&mut self.last_status);
            return Ok(());
        };
        let command_id = self.take_command_id()?;
        let command = BuildCommand {
            command_id,
            position,
            material,
        };
        self.transport
            .send(encode_build_request(session_id, command))
            .map_err(|error| format!("envoi construction: {error}"))?;
        self.last_status = format!("construction {command_id} envoyee en {position:?}");
        Ok(())
    }

    fn take_command_id(&mut self) -> Result<u64, String> {
        let command_id = self.next_command_id;
        self.next_command_id = self
            .next_command_id
            .checked_add(1)
            .ok_or_else(|| "sequence de commandes epuisee".to_owned())?;
        Ok(command_id)
    }

    fn send_smoke_action(&mut self) -> Result<(), String> {
        let elapsed = self.started.elapsed().as_secs_f32();
        let due_actions = u8::from(elapsed >= 3.0) + u8::from(elapsed >= 4.0);
        if !self.smoke_motion || !self.snapshot_ready() || self.smoke_actions_sent >= due_actions {
            return Ok(());
        }
        let Some(session_id) = self.session_id else {
            return Ok(());
        };
        if !session_id.is_multiple_of(2) {
            let command_id = self.take_command_id()?;
            let command = ExplosionCommand {
                command_id,
                center: if self.smoke_actions_sent == 0 {
                    IVec3::new(-20, 6, 0)
                } else {
                    IVec3::new(20, 6, 0)
                },
                radius_voxels: 2,
                peak_energy: 7_500,
            };
            self.transport
                .send(encode_explosion_request(session_id, command))
                .map_err(|error| format!("envoi destruction smoke: {error}"))?;
        }
        self.smoke_actions_sent = self.smoke_actions_sent.saturating_add(1);
        Ok(())
    }

    fn sync_local_view(&mut self) {
        let Some(prediction) = &self.prediction else {
            return;
        };
        let position_um = prediction.state().position_um;
        let initial = *self.initial_position_um.get_or_insert(position_um);
        self.maximum_horizontal_displacement_um = self.maximum_horizontal_displacement_um.max(
            position_um
                .x
                .abs_diff(initial.x)
                .max(position_um.z.abs_diff(initial.z)),
        );
        self.view.position = fixed_to_world(position_um) + self.visual_correction;
        self.view.velocity = fixed_to_world(prediction.state().velocity_um_per_second);
    }

    fn smooth_visual_correction(&mut self, delta_seconds: f32) {
        self.visual_correction = decay_correction(
            self.visual_correction,
            delta_seconds,
            VISUAL_CORRECTION_HALF_LIFE_SECONDS,
        );
        self.sync_local_view();
    }

    fn update_remote_players(&mut self) -> Result<(), String> {
        let (Some(base_tick), Some(received_at)) = (
            self.interpolation.delayed_target_tick(),
            self.latest_state_received_at,
        ) else {
            return Ok(());
        };
        let elapsed_ticks = received_at.elapsed().as_secs_f64() * 60.0;
        let whole_ticks = elapsed_ticks.floor().max(0.0) as u64;
        let subtick = ((elapsed_ticks.fract() * 1_000.0).round() as u16).min(999);
        let sample = self
            .interpolation
            .sample(base_tick.saturating_add(whole_ticks), subtick)
            .map_err(|error| format!("interpolation distante: {error}"))?;
        self.renderer
            .update_player_transforms(&sample.players, self.session_id)
    }

    fn redraw(
        &mut self,
        event_loop: &ActiveEventLoop,
        exit_after: Option<Duration>,
    ) -> Result<(), String> {
        self.pump_network()?;
        let now = Instant::now();
        let frame_seconds = now
            .duration_since(self.previous_frame)
            .as_secs_f32()
            .min(0.1);
        self.accumulator += frame_seconds;
        self.previous_frame = now;
        let mut steps = 0;
        while self.accumulator >= FIXED_STEP_SECONDS && steps < 6 {
            self.fixed_tick()?;
            self.accumulator -= FIXED_STEP_SECONDS;
            steps += 1;
        }
        self.smooth_visual_correction(frame_seconds);
        self.pump_meshing()?;
        self.send_smoke_action()?;
        self.update_remote_players()?;
        match self.renderer.render(
            self.view.camera_position(),
            self.view.view_direction(),
            now.duration_since(self.started).as_secs_f32(),
        ) {
            RenderOutcome::Presented | RenderOutcome::Skipped => {}
            RenderOutcome::Reconfigure => self.renderer.resize(self.window.inner_size())?,
            RenderOutcome::RecreateSurface => self.renderer.recreate_surface()?,
        }
        let stats = self.renderer.stats();
        let transport_drops = self.transport.dropped_datagrams();
        self.window.set_title(&format!(
            "Destructible FPS multijoueur | session {} | {} joueurs | drops {} | {}",
            self.session_id
                .map_or_else(|| "...".to_owned(), |id| id.to_string()),
            stats
                .players
                .saturating_add(usize::from(self.session_id.is_some())),
            transport_drops,
            self.last_status
        ));
        if exit_after.is_some_and(|duration| self.started.elapsed() >= duration) {
            self.validate_and_report_smoke(stats.players, transport_drops)?;
            event_loop.exit();
        }
        Ok(())
    }
}

fn continuity_correction(
    visual_position_before: Vec3,
    predicted_position_after: Vec3,
    maximum_distance: f32,
) -> Vec3 {
    let correction = visual_position_before - predicted_position_after;
    if !correction.is_finite()
        || !maximum_distance.is_finite()
        || maximum_distance <= 0.0
        || correction.length_squared() > maximum_distance * maximum_distance
    {
        Vec3::ZERO
    } else {
        correction
    }
}

fn decay_correction(correction: Vec3, delta_seconds: f32, half_life_seconds: f32) -> Vec3 {
    if !correction.is_finite()
        || !delta_seconds.is_finite()
        || !half_life_seconds.is_finite()
        || half_life_seconds <= 0.0
    {
        return Vec3::ZERO;
    }
    let retained = (-delta_seconds.max(0.0) / half_life_seconds).exp2();
    let decayed = correction * retained;
    if decayed.length_squared() < 0.000_001 {
        Vec3::ZERO
    } else {
        decayed
    }
}

fn fixed_to_world(value: FixedMicrometers3) -> Vec3 {
    let scale = MICROMETERS_PER_VOXEL as f32;
    Vec3::new(
        value.x as f32 / scale,
        value.y as f32 / scale,
        value.z as f32 / scale,
    )
}

fn movement_command(
    input_sequence: u64,
    yaw: f32,
    pressed: &HashSet<KeyCode>,
) -> PlayerInputCommand {
    let axis = |positive, negative| f32::from(u8::from(positive)) - f32::from(u8::from(negative));
    let forward_input = axis(
        pressed.contains(&KeyCode::KeyW) || pressed.contains(&KeyCode::KeyZ),
        pressed.contains(&KeyCode::KeyS),
    );
    let right_input = axis(
        pressed.contains(&KeyCode::KeyD),
        pressed.contains(&KeyCode::KeyA) || pressed.contains(&KeyCode::KeyQ),
    );
    let forward = Vec3::new(yaw.sin(), 0.0, -yaw.cos());
    let right = Vec3::new(yaw.cos(), 0.0, yaw.sin());
    let mut wish = forward * forward_input + right * right_input;
    if wish.length_squared() > 1.0 {
        wish = wish.normalize();
    }
    PlayerInputCommand {
        input_sequence,
        movement_x_per_mille: (wish.x * 1_000.0).round() as i16,
        movement_z_per_mille: (wish.z * 1_000.0).round() as i16,
        jump: pressed.contains(&KeyCode::Space),
        sprint: pressed.contains(&KeyCode::ShiftLeft) || pressed.contains(&KeyCode::ShiftRight),
    }
}

struct App {
    game: Option<MultiplayerGame>,
    transport: TransportOptions,
    exit_after: Option<Duration>,
    smoke_drop_first_delta: bool,
    msaa: u32,
    failure: Option<String>,
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.game.is_some() {
            return;
        }
        let attributes = Window::default_attributes()
            .with_title("Destructible FPS multijoueur | initialisation")
            .with_inner_size(LogicalSize::new(1280.0, 800.0))
            .with_min_inner_size(LogicalSize::new(800.0, 500.0));
        let Ok(window) = event_loop.create_window(attributes) else {
            self.failure = Some("creation de fenetre impossible".to_owned());
            event_loop.exit();
            return;
        };
        match MultiplayerGame::new(
            Arc::new(window),
            &self.transport,
            self.exit_after.is_some(),
            self.smoke_drop_first_delta,
            self.msaa,
        ) {
            Ok(game) => self.game = Some(game),
            Err(error) => {
                self.failure = Some(error);
                event_loop.exit();
            }
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
    ) {
        let Some(game) = &mut self.game else {
            return;
        };
        if game.window.id() != window_id {
            return;
        }
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                if let Err(error) = game.renderer.resize(size) {
                    self.failure = Some(error);
                    event_loop.exit();
                }
            }
            WindowEvent::Focused(false) => game.release_cursor(),
            WindowEvent::KeyboardInput { event, .. } => {
                if let PhysicalKey::Code(code) = event.physical_key {
                    match event.state {
                        ElementState::Pressed => {
                            game.pressed.insert(code);
                            if code == KeyCode::Escape {
                                if game.cursor_captured {
                                    game.release_cursor();
                                } else {
                                    event_loop.exit();
                                }
                            }
                        }
                        ElementState::Released => {
                            game.pressed.remove(&code);
                        }
                    }
                }
            }
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button,
                ..
            } => {
                if game.cursor_captured {
                    let result = match button {
                        MouseButton::Left => game.send_explosion(2, 7_500),
                        MouseButton::Right => game.send_explosion(6, 42_000),
                        MouseButton::Middle => game.send_build(Material::Wood),
                        _ => Ok(()),
                    };
                    if let Err(error) = result {
                        self.failure = Some(error);
                        event_loop.exit();
                    }
                } else {
                    game.capture_cursor();
                }
            }
            WindowEvent::RedrawRequested => {
                if let Err(error) = game.redraw(event_loop, self.exit_after) {
                    self.failure = Some(error);
                    event_loop.exit();
                }
            }
            _ => {}
        }
    }

    fn device_event(
        &mut self,
        _event_loop: &ActiveEventLoop,
        _device_id: DeviceId,
        event: DeviceEvent,
    ) {
        if let Some(game) = &mut self.game
            && game.cursor_captured
            && let DeviceEvent::MouseMotion { delta } = event
        {
            game.view.look(delta.0, delta.1);
        }
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(game) = &self.game {
            game.window.request_redraw();
        }
    }
}

struct Options {
    transport: TransportOptions,
    exit_after: Option<Duration>,
    smoke_drop_first_delta: bool,
    msaa: u32,
}

fn options() -> Result<Options, Box<dyn Error>> {
    options_from(std::env::args().skip(1))
}

fn options_from<I, S>(arguments: I) -> Result<Options, Box<dyn Error>>
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    let mut server: SocketAddr = DEFAULT_SERVER.parse()?;
    let mut legacy_server_selected = false;
    let mut secure_server = None;
    let mut secure_server_name = None;
    let mut secure_root_certificate = None;
    let mut secure_credential = None;
    let mut exit_after = None;
    let mut smoke_drop_first_delta = false;
    let mut msaa = 4;
    let mut arguments = arguments.into_iter().map(Into::into);
    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--msaa" => {
                msaa = arguments.next().ok_or("--msaa exige 1 ou 4")?.parse()?;
                if !matches!(msaa, 1 | 4) {
                    return Err("--msaa exige exactement 1 ou 4".into());
                }
            }
            "--server" => {
                legacy_server_selected = true;
                server = arguments
                    .next()
                    .ok_or("--server exige une adresse")?
                    .parse()?;
            }
            "--secure-server" => {
                secure_server = Some(
                    arguments
                        .next()
                        .ok_or("--secure-server exige une adresse")?
                        .parse()?,
                );
            }
            "--server-name" => {
                secure_server_name =
                    Some(arguments.next().ok_or("--server-name exige un nom TLS")?);
            }
            "--ca-cert" => {
                secure_root_certificate = Some(PathBuf::from(
                    arguments.next().ok_or("--ca-cert exige un chemin")?,
                ));
            }
            "--credential-file" => {
                secure_credential = Some(PathBuf::from(
                    arguments
                        .next()
                        .ok_or("--credential-file exige un chemin")?,
                ));
            }
            "--smoke-seconds" => {
                let seconds: f64 = arguments
                    .next()
                    .ok_or("--smoke-seconds exige une duree")?
                    .parse()?;
                if !seconds.is_finite() || seconds <= 0.0 {
                    return Err("duree de smoke test invalide".into());
                }
                exit_after = Some(Duration::from_secs_f64(seconds));
            }
            "--smoke-drop-first-delta" => smoke_drop_first_delta = true,
            _ => return Err(format!("argument inconnu: {argument}").into()),
        }
    }
    let secure_option_count = usize::from(secure_server.is_some())
        + usize::from(secure_server_name.is_some())
        + usize::from(secure_root_certificate.is_some())
        + usize::from(secure_credential.is_some());
    let transport = if secure_option_count == 0 {
        if !server.ip().is_loopback() {
            return Err(
                "le transport UDP de developpement accepte uniquement une adresse loopback".into(),
            );
        }
        TransportOptions::Loopback(server)
    } else if secure_option_count == 4 && !legacy_server_selected {
        TransportOptions::Secure(SecureClientLaunchConfig {
            server_address: secure_server.ok_or("adresse QUIC absente")?,
            server_name: secure_server_name.ok_or("nom TLS absent")?,
            root_certificate_file: secure_root_certificate.ok_or("certificat racine absent")?,
            credential_file: secure_credential.ok_or("fichier de jeton absent")?,
        })
    } else {
        return Err(
            "le mode securise exige ensemble --secure-server, --server-name, --ca-cert et --credential-file, sans --server"
                .into(),
        );
    };
    if smoke_drop_first_delta && exit_after.is_none() {
        return Err("--smoke-drop-first-delta exige --smoke-seconds".into());
    }
    Ok(Options {
        transport,
        exit_after,
        smoke_drop_first_delta,
        msaa,
    })
}

fn main() -> Result<(), Box<dyn Error>> {
    let options = options()?;
    let event_loop = EventLoop::new()?;
    event_loop.set_control_flow(ControlFlow::Poll);
    let mut app = App {
        game: None,
        transport: options.transport,
        exit_after: options.exit_after,
        smoke_drop_first_delta: options.smoke_drop_first_delta,
        msaa: options.msaa,
        failure: None,
    };
    event_loop.run_app(&mut app)?;
    if let Some(error) = app.failure {
        return Err(error.into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn msaa_cli_is_explicit_and_bounded() {
        assert_eq!(options_from(Vec::<String>::new()).unwrap().msaa, 4);
        for value in ["1", "4"] {
            assert_eq!(
                options_from(["--msaa", value]).unwrap().msaa.to_string(),
                value
            );
        }
        for arguments in [
            vec!["--msaa"],
            vec!["--msaa", "0"],
            vec!["--msaa", "2"],
            vec!["--msaa", "8"],
            vec!["--msaa", "-1"],
            vec!["--msaa", "NaN"],
        ] {
            assert!(options_from(arguments).is_err());
        }
    }

    #[test]
    fn movement_is_camera_relative_and_unit_bounded() {
        let pressed = [KeyCode::KeyW, KeyCode::KeyD].into_iter().collect();
        let input = movement_command(7, 0.0, &pressed);
        assert_eq!(input.input_sequence, 7);
        assert_eq!(input.movement_x_per_mille, 707);
        assert_eq!(input.movement_z_per_mille, -707);
        assert!(
            i64::from(input.movement_x_per_mille).pow(2)
                + i64::from(input.movement_z_per_mille).pow(2)
                <= 1_000_000
        );
    }

    #[test]
    fn fixed_positions_convert_to_voxel_world_units() {
        assert_eq!(
            fixed_to_world(FixedMicrometers3 {
                x: 2 * MICROMETERS_PER_VOXEL,
                y: MICROMETERS_PER_VOXEL / 2,
                z: -3 * MICROMETERS_PER_VOXEL,
            }),
            Vec3::new(2.0, 0.5, -3.0)
        );
    }

    #[test]
    fn small_reconciliation_preserves_visual_position_then_decays() {
        let visual_before = Vec3::new(4.0, 2.0, -3.0);
        let predicted_after = Vec3::new(3.5, 2.0, -3.25);
        let correction = continuity_correction(visual_before, predicted_after, 2.0);
        assert_eq!(predicted_after + correction, visual_before);
        let decayed = decay_correction(correction, 0.08, 0.08);
        assert!((decayed - correction * 0.5).length() < 0.000_01);
    }

    #[test]
    fn large_or_invalid_reconciliation_snaps_safely() {
        assert_eq!(
            continuity_correction(Vec3::splat(10.0), Vec3::ZERO, 2.0),
            Vec3::ZERO
        );
        assert_eq!(
            decay_correction(Vec3::splat(f32::NAN), 0.016, 0.08),
            Vec3::ZERO
        );
    }

    #[test]
    fn secure_cli_requires_the_complete_file_backed_contract() {
        assert!(options_from(["--secure-server", "127.0.0.1:40001"]).is_err());
        assert!(
            options_from([
                "--server",
                "127.0.0.1:40000",
                "--secure-server",
                "127.0.0.1:40001",
                "--server-name",
                "game.local",
                "--ca-cert",
                "/tmp/ca.pem",
                "--credential-file",
                "/tmp/token",
            ])
            .is_err()
        );

        let options = options_from([
            "--secure-server",
            "127.0.0.1:40001",
            "--server-name",
            "game.local",
            "--ca-cert",
            "/tmp/ca.pem",
            "--credential-file",
            "/tmp/token",
        ])
        .expect("complete secure launch contract");
        assert!(matches!(options.transport, TransportOptions::Secure(_)));
    }

    #[test]
    fn legacy_cli_remains_strictly_loopback() {
        assert!(options_from(["--server", "192.0.2.10:40000"]).is_err());
        let options =
            options_from(["--server", "127.0.0.1:40000"]).expect("loopback development transport");
        assert!(matches!(
            options.transport,
            TransportOptions::Loopback(address) if address.ip().is_loopback()
        ));
    }
}
