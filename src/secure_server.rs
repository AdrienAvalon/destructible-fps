//! Bounded QUIC adapter for the transport-independent authoritative simulation.

use crate::{
    AuthenticatedSession, AuthoritativeServer, AuthorityCore, MAX_PENDING_QUIC_HANDSHAKES,
    MAX_QUIC_DATAGRAM_PAYLOAD_BYTES, MAX_RECEIVED_DATAGRAMS_PER_TICK, MAX_SERVER_PEERS,
    NetworkRuntimeError, NetworkTickReport, SessionCredentialVerifier, World, admit_session,
    receive_gameplay_datagram, send_gameplay_datagram,
};
use bytes::Bytes;
use quinn::{Endpoint, ServerConfig, VarInt};
use ring::rand::{SecureRandom, SystemRandom};
use std::{
    collections::BTreeMap,
    io,
    net::{SocketAddr, ToSocketAddrs},
    num::NonZeroU64,
    sync::{
        Arc,
        atomic::{AtomicU64, AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::{
    runtime::Handle,
    sync::mpsc,
    task::JoinHandle,
    time::{Instant, timeout},
};

pub const MAX_SECURE_CONTROL_EVENTS: usize = MAX_PENDING_QUIC_HANDSHAKES + MAX_SERVER_PEERS * 2;
pub const MAX_SECURE_GAMEPLAY_EVENTS: usize = MAX_RECEIVED_DATAGRAMS_PER_TICK * 4;
pub const MAX_SECURE_GAMEPLAY_BYTES: usize =
    MAX_SECURE_GAMEPLAY_EVENTS * MAX_QUIC_DATAGRAM_PAYLOAD_BYTES;
pub const MAX_SESSION_DATAGRAMS_PER_SECOND: usize = 240;
pub const MAX_CONSECUTIVE_GAMEPLAY_QUEUE_DROPS: usize = 32;

const MAX_SECURE_CONTROL_EVENTS_PER_TICK: usize = MAX_SECURE_CONTROL_EVENTS;
const SESSION_RATE_WINDOW: Duration = Duration::from_secs(1);
const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(6);
const CLOSE_SERVER_FULL: u32 = 0x110;
const CLOSE_RATE_LIMIT: u32 = 0x111;
const CLOSE_QUEUE_PRESSURE: u32 = 0x112;
const CLOSE_PROTOCOL: u32 = 0x113;
const CLOSE_SHUTDOWN: u32 = 0x114;

enum ControlEvent {
    Admitted(AuthenticatedSession),
    Disconnected { peer_id: u64, session_id: u64 },
}

struct GameplayEvent {
    peer_id: u64,
    session_id: u64,
    payload: Bytes,
}

struct ActiveSession {
    session_id: u64,
    connection: quinn::Connection,
    receiver: JoinHandle<()>,
}

#[derive(Default)]
struct SharedCounters {
    refused_connections: AtomicUsize,
    handshake_failures: AtomicUsize,
    admission_failures: AtomicUsize,
    gameplay_queue_drops: AtomicUsize,
    protocol_rejections: AtomicUsize,
    rate_limited_sessions: AtomicUsize,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SecureNetworkTickReport {
    pub authority: NetworkTickReport,
    pub admitted_sessions: usize,
    pub disconnected_sessions: usize,
    pub refused_connections: usize,
    pub handshake_failures: usize,
    pub admission_failures: usize,
    pub gameplay_queue_drops: usize,
    pub protocol_rejections: usize,
    pub rate_limited_sessions: usize,
    pub active_sessions: usize,
}

/// Async QUIC ingress/egress supervisor around the deterministic authority core.
///
/// TLS handshakes and credential admission run outside the fixed-step simulation. Only bounded
/// control events and 1,100-byte gameplay datagrams cross into `tick`, which remains synchronous.
pub struct SecureDedicatedServer {
    endpoint: Endpoint,
    authority: AuthorityCore<u64>,
    sessions: BTreeMap<u64, ActiveSession>,
    control_sender: mpsc::Sender<ControlEvent>,
    control_receiver: mpsc::Receiver<ControlEvent>,
    gameplay_sender: mpsc::Sender<GameplayEvent>,
    gameplay_receiver: mpsc::Receiver<GameplayEvent>,
    counters: Arc<SharedCounters>,
    accept_supervisor: Option<JoinHandle<()>>,
    runtime: Handle,
}

impl SecureDedicatedServer {
    /// Binds a QUIC endpoint and starts a bounded admission supervisor on the current Tokio runtime.
    ///
    /// # Errors
    ///
    /// Returns address resolution, endpoint bind, or authority configuration errors.
    pub fn bind(
        address: impl ToSocketAddrs,
        server_config: ServerConfig,
        verifier: Arc<dyn SessionCredentialVerifier>,
        world: World,
    ) -> io::Result<Self> {
        let address = resolve_one(address)?;
        if !address.ip().is_loopback() {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "secure authority remains loopback-only until trusted process configuration",
            ));
        }
        Self::bind_resolved(address, server_config, verifier, world)
    }

    pub(crate) fn bind_validated_loopback(
        address: SocketAddr,
        server_config: ServerConfig,
        verifier: Arc<dyn SessionCredentialVerifier>,
        world: World,
    ) -> io::Result<Self> {
        if !address.ip().is_loopback() {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "validated secure authority policy permits only loopback",
            ));
        }
        Self::bind_resolved(address, server_config, verifier, world)
    }

    fn bind_resolved(
        address: SocketAddr,
        server_config: ServerConfig,
        verifier: Arc<dyn SessionCredentialVerifier>,
        world: World,
    ) -> io::Result<Self> {
        let runtime = Handle::try_current().map_err(|_error| {
            io::Error::other("secure authority requires an active Tokio runtime")
        })?;
        let endpoint = Endpoint::server(server_config, address)?;
        let authority = AuthorityCore::new(world, MAX_QUIC_DATAGRAM_PAYLOAD_BYTES)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error.to_string()))?;
        let (control_sender, control_receiver) = mpsc::channel(MAX_SECURE_CONTROL_EVENTS);
        let (gameplay_sender, gameplay_receiver) = mpsc::channel(MAX_SECURE_GAMEPLAY_EVENTS);
        let counters = Arc::new(SharedCounters::default());
        let accept_supervisor = runtime.spawn(accept_connections(
            endpoint.clone(),
            verifier,
            control_sender.clone(),
            Arc::clone(&counters),
        ));
        Ok(Self {
            endpoint,
            authority,
            sessions: BTreeMap::new(),
            control_sender,
            control_receiver,
            gameplay_sender,
            gameplay_receiver,
            counters,
            accept_supervisor: Some(accept_supervisor),
            runtime,
        })
    }

    /// Advances the authority by one fixed tick after draining bounded session and gameplay queues.
    ///
    /// # Errors
    ///
    /// Returns authoritative frame-encoding failures.
    pub fn tick(&mut self) -> Result<SecureNetworkTickReport, NetworkRuntimeError> {
        let mut authority_report = self.authority.begin_tick();
        let (admitted_sessions, mut disconnected_sessions) =
            self.drain_control_events(&mut authority_report);
        authority_report = self.drain_gameplay_and_complete(authority_report)?;
        disconnected_sessions += self.expire_core_sessions();

        Ok(SecureNetworkTickReport {
            authority: authority_report,
            admitted_sessions,
            disconnected_sessions,
            refused_connections: take_counter(&self.counters.refused_connections),
            handshake_failures: take_counter(&self.counters.handshake_failures),
            admission_failures: take_counter(&self.counters.admission_failures),
            gameplay_queue_drops: take_counter(&self.counters.gameplay_queue_drops),
            protocol_rejections: take_counter(&self.counters.protocol_rejections),
            rate_limited_sessions: take_counter(&self.counters.rate_limited_sessions),
            active_sessions: self.sessions.len(),
        })
    }

    fn drain_control_events(&mut self, report: &mut NetworkTickReport) -> (usize, usize) {
        let mut admitted = 0_usize;
        let mut disconnected = 0_usize;
        for _ in 0..MAX_SECURE_CONTROL_EVENTS_PER_TICK {
            let event = match self.control_receiver.try_recv() {
                Ok(event) => event,
                Err(mpsc::error::TryRecvError::Empty | mpsc::error::TryRecvError::Disconnected) => {
                    break;
                }
            };
            match event {
                ControlEvent::Admitted(session) => {
                    let peer_id = session.session_id();
                    if !self.authority.admit_authenticated(
                        peer_id,
                        session.client_nonce(),
                        session.session_id(),
                        session.principal(),
                        report,
                    ) {
                        session.connection().close(
                            VarInt::from_u32(CLOSE_SERVER_FULL),
                            b"authority session rejected",
                        );
                        continue;
                    }
                    let connection = session.connection().clone();
                    let receiver = self.runtime.spawn(receive_session_datagrams(
                        peer_id,
                        session.session_id(),
                        connection.clone(),
                        self.gameplay_sender.clone(),
                        self.control_sender.clone(),
                        Arc::clone(&self.counters),
                    ));
                    let replaced = self.sessions.insert(
                        peer_id,
                        ActiveSession {
                            session_id: session.session_id(),
                            connection,
                            receiver,
                        },
                    );
                    debug_assert!(replaced.is_none());
                    admitted += 1;
                }
                ControlEvent::Disconnected {
                    peer_id,
                    session_id,
                } => {
                    if self.authority.disconnect_authenticated(peer_id, session_id) {
                        if let Some(active) = self.sessions.remove(&peer_id) {
                            active.receiver.abort();
                        }
                        disconnected += 1;
                    }
                }
            }
        }
        (admitted, disconnected)
    }

    fn drain_gameplay_and_complete(
        &mut self,
        mut report: NetworkTickReport,
    ) -> Result<NetworkTickReport, NetworkRuntimeError> {
        {
            let sessions = &self.sessions;
            let mut sender = |peer_id: u64, payload: &[u8]| {
                sessions.get(&peer_id).is_some_and(|session| {
                    send_gameplay_datagram(&session.connection, payload.to_vec()).is_ok()
                })
            };
            for _ in 0..MAX_RECEIVED_DATAGRAMS_PER_TICK {
                let event = match self.gameplay_receiver.try_recv() {
                    Ok(event) => event,
                    Err(
                        mpsc::error::TryRecvError::Empty | mpsc::error::TryRecvError::Disconnected,
                    ) => break,
                };
                if self
                    .sessions
                    .get(&event.peer_id)
                    .is_none_or(|active| active.session_id != event.session_id)
                {
                    report.rejected_sessions += 1;
                    continue;
                }
                self.authority.ingest_datagram(
                    event.peer_id,
                    &event.payload,
                    &mut sender,
                    &mut report,
                );
            }
            report = self.authority.complete_tick(&mut sender, report)?;
        }
        Ok(report)
    }

    fn expire_core_sessions(&mut self) -> usize {
        let expired = self
            .sessions
            .iter()
            .filter_map(|(peer_id, active)| {
                (!self
                    .authority
                    .session_is_active(*peer_id, active.session_id))
                .then_some(*peer_id)
            })
            .collect::<Vec<_>>();
        let mut disconnected = 0_usize;
        for peer_id in expired {
            if let Some(active) = self.sessions.remove(&peer_id) {
                active.connection.close(
                    VarInt::from_u32(CLOSE_PROTOCOL),
                    b"authority session expired",
                );
                active.receiver.abort();
                disconnected += 1;
            }
        }
        disconnected
    }

    #[must_use]
    pub const fn authority(&self) -> &AuthoritativeServer {
        self.authority.authority()
    }

    #[must_use]
    pub fn active_sessions(&self) -> usize {
        self.sessions.len()
    }

    /// Returns the bound endpoint address, including an ephemeral port selected for port zero.
    ///
    /// # Errors
    ///
    /// Returns the endpoint socket address query error.
    pub fn local_addr(&self) -> io::Result<SocketAddr> {
        self.endpoint.local_addr()
    }

    /// Closes the endpoint and all active connections, then bounds shutdown-task waiting.
    pub async fn shutdown(mut self) {
        self.close_all();
        if let Some(supervisor) = self.accept_supervisor.take() {
            let _ = timeout(SHUTDOWN_TIMEOUT, supervisor).await;
        }
        self.endpoint.wait_idle().await;
    }

    fn close_all(&mut self) {
        self.endpoint
            .close(VarInt::from_u32(CLOSE_SHUTDOWN), b"authority shutting down");
        for active in self.sessions.values() {
            active
                .connection
                .close(VarInt::from_u32(CLOSE_SHUTDOWN), b"authority shutting down");
            active.receiver.abort();
        }
        self.sessions.clear();
    }
}

impl Drop for SecureDedicatedServer {
    fn drop(&mut self) {
        self.close_all();
        if let Some(supervisor) = self.accept_supervisor.take() {
            supervisor.abort();
        }
    }
}

async fn accept_connections(
    endpoint: Endpoint,
    verifier: Arc<dyn SessionCredentialVerifier>,
    control_sender: mpsc::Sender<ControlEvent>,
    counters: Arc<SharedCounters>,
) {
    let next_session_id = AtomicU64::new(1);
    let random = SystemRandom::new();
    let mut admissions = tokio::task::JoinSet::new();
    loop {
        tokio::select! {
            incoming = endpoint.accept() => {
                let Some(incoming) = incoming else {
                    break;
                };
                if admissions.len() >= MAX_PENDING_QUIC_HANDSHAKES {
                    counters.refused_connections.fetch_add(1, Ordering::Relaxed);
                    incoming.refuse();
                    continue;
                }
                let Some(session_id) = allocate_session_id(&next_session_id) else {
                    counters.refused_connections.fetch_add(1, Ordering::Relaxed);
                    incoming.refuse();
                    continue;
                };
                let Some(server_nonce) = random_nonzero(&random) else {
                    counters.refused_connections.fetch_add(1, Ordering::Relaxed);
                    incoming.refuse();
                    continue;
                };
                let verifier = Arc::clone(&verifier);
                let control_sender = control_sender.clone();
                let counters = Arc::clone(&counters);
                admissions.spawn(async move {
                    let connection = match incoming.await {
                        Ok(connection) => connection,
                        Err(_error) => {
                            counters.handshake_failures.fetch_add(1, Ordering::Relaxed);
                            return;
                        }
                    };
                    match admit_session(connection, verifier.as_ref(), session_id, server_nonce).await {
                        Ok(session) => {
                            if let Err(error) = control_sender.send(ControlEvent::Admitted(session)).await
                                && let ControlEvent::Admitted(session) = error.0
                            {
                                session.connection().close(
                                    VarInt::from_u32(CLOSE_SHUTDOWN),
                                    b"authority unavailable",
                                );
                            }
                        }
                        Err(_error) => {
                            counters.admission_failures.fetch_add(1, Ordering::Relaxed);
                        }
                    }
                });
            }
            result = admissions.join_next(), if !admissions.is_empty() => {
                let _ = result;
            }
        }
    }
    admissions.abort_all();
    while admissions.join_next().await.is_some() {}
}

async fn receive_session_datagrams(
    peer_id: u64,
    session_id: u64,
    connection: quinn::Connection,
    gameplay_sender: mpsc::Sender<GameplayEvent>,
    control_sender: mpsc::Sender<ControlEvent>,
    counters: Arc<SharedCounters>,
) {
    let mut window_started = Instant::now();
    let mut datagrams_in_window = 0_usize;
    let mut consecutive_queue_drops = 0_usize;
    loop {
        let payload = match receive_gameplay_datagram(&connection).await {
            Ok(payload) => payload,
            Err(crate::SecureDatagramReceiveError::Connection(_error)) => break,
            Err(
                crate::SecureDatagramReceiveError::Empty
                | crate::SecureDatagramReceiveError::Oversized(_),
            ) => {
                counters.protocol_rejections.fetch_add(1, Ordering::Relaxed);
                connection.close(
                    VarInt::from_u32(CLOSE_PROTOCOL),
                    b"invalid gameplay datagram",
                );
                break;
            }
        };
        let now = Instant::now();
        if now.duration_since(window_started) >= SESSION_RATE_WINDOW {
            window_started = now;
            datagrams_in_window = 0;
        }
        datagrams_in_window += 1;
        if datagrams_in_window > MAX_SESSION_DATAGRAMS_PER_SECOND {
            counters
                .rate_limited_sessions
                .fetch_add(1, Ordering::Relaxed);
            connection.close(
                VarInt::from_u32(CLOSE_RATE_LIMIT),
                b"gameplay rate exceeded",
            );
            break;
        }
        match gameplay_sender.try_send(GameplayEvent {
            peer_id,
            session_id,
            payload,
        }) {
            Ok(()) => consecutive_queue_drops = 0,
            Err(mpsc::error::TrySendError::Closed(_event)) => break,
            Err(mpsc::error::TrySendError::Full(_event)) => {
                counters
                    .gameplay_queue_drops
                    .fetch_add(1, Ordering::Relaxed);
                consecutive_queue_drops += 1;
                if consecutive_queue_drops >= MAX_CONSECUTIVE_GAMEPLAY_QUEUE_DROPS {
                    connection.close(
                        VarInt::from_u32(CLOSE_QUEUE_PRESSURE),
                        b"gameplay queue pressure",
                    );
                    break;
                }
            }
        }
    }
    let _ = control_sender
        .send(ControlEvent::Disconnected {
            peer_id,
            session_id,
        })
        .await;
}

fn allocate_session_id(next: &AtomicU64) -> Option<NonZeroU64> {
    next.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |value| {
        value.checked_add(1)
    })
    .ok()
    .and_then(NonZeroU64::new)
}

fn random_nonzero(random: &SystemRandom) -> Option<NonZeroU64> {
    for _ in 0..4 {
        let mut bytes = [0_u8; 8];
        random.fill(&mut bytes).ok()?;
        if let Some(value) = NonZeroU64::new(u64::from_le_bytes(bytes)) {
            return Some(value);
        }
    }
    None
}

fn resolve_one(address: impl ToSocketAddrs) -> io::Result<SocketAddr> {
    address
        .to_socket_addrs()?
        .next()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "address resolved to no socket"))
}

fn take_counter(counter: &AtomicUsize) -> usize {
    counter.swap(0, Ordering::Relaxed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_ids_are_monotonic_and_do_not_wrap() {
        let next = AtomicU64::new(1);
        assert_eq!(allocate_session_id(&next).map(NonZeroU64::get), Some(1));
        assert_eq!(allocate_session_id(&next).map(NonZeroU64::get), Some(2));
        next.store(u64::MAX, Ordering::Relaxed);
        assert_eq!(allocate_session_id(&next), None);
        assert_eq!(next.load(Ordering::Relaxed), u64::MAX);
    }

    #[test]
    fn cryptographic_server_nonce_is_nonzero() {
        let random = SystemRandom::new();
        assert!(random_nonzero(&random).is_some());
    }
}
