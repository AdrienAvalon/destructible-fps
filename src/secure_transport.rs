//! Bounded QUIC/TLS session boundary for authenticated remote transport.

use bytes::Bytes;
use core::fmt;
use quinn::{ClientConfig, SendDatagramError, ServerConfig, TransportConfig, VarInt};
use std::{num::NonZeroU64, sync::Arc, time::Duration};
use zeroize::Zeroizing;

use quinn::{
    crypto::rustls::{NoInitialCipherSuite, QuicClientConfig, QuicServerConfig},
    rustls::{
        ClientConfig as RustlsClientConfig, RootCertStore, ServerConfig as RustlsServerConfig,
        pki_types::{CertificateDer, PrivateKeyDer},
    },
};

const SESSION_MAGIC: [u8; 4] = *b"DFQS";
const SESSION_VERSION: u8 = 1;
const SESSION_HELLO_KIND: u8 = 1;
const SESSION_WELCOME_KIND: u8 = 2;
const SESSION_PREFIX_BYTES: usize = 6;
const SESSION_HELLO_FIXED_BYTES: usize = SESSION_PREFIX_BYTES + 8 + 2;
const SESSION_WELCOME_BYTES: usize = SESSION_PREFIX_BYTES + 8 + 8 + 8;
const MIN_SESSION_CREDENTIAL_BYTES: usize = 16;
const QUIC_DATAGRAM_BUFFER_BYTES: usize = 128 * 1_024;
const QUIC_STREAM_WINDOW_BYTES: u32 = 16 * 1_024;
const QUIC_CONNECTION_WINDOW_BYTES: u32 = 32 * 1_024;
const QUIC_SEND_WINDOW_BYTES: u64 = 128 * 1_024;
const QUIC_CRYPTO_BUFFER_BYTES: usize = 16 * 1_024;
const MAX_PENDING_QUIC_HANDSHAKES: usize = 32;
const PENDING_HANDSHAKE_BUFFER_BYTES: u64 = 16 * 1_024;
const TOTAL_PENDING_HANDSHAKE_BUFFER_BYTES: u64 = 512 * 1_024;
const SESSION_ADMISSION_TIMEOUT: Duration = Duration::from_secs(5);

pub const ALPN_PROTOCOL: &[u8] = b"destructible-fps/1";
pub const MAX_QUIC_DATAGRAM_PAYLOAD_BYTES: usize = 1_100;
pub const MAX_SESSION_CREDENTIAL_BYTES: usize = 4_096;
pub const MAX_SESSION_HELLO_BYTES: usize = SESSION_HELLO_FIXED_BYTES + MAX_SESSION_CREDENTIAL_BYTES;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SessionHello<'a> {
    pub client_nonce: u64,
    pub credential: &'a [u8],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SessionWelcome {
    pub client_nonce: u64,
    pub session_id: u64,
    pub server_nonce: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AuthenticatedPrincipal(NonZeroU64);

impl AuthenticatedPrincipal {
    #[must_use]
    pub const fn new(id: NonZeroU64) -> Self {
        Self(id)
    }

    #[must_use]
    pub const fn get(self) -> u64 {
        self.0.get()
    }
}

/// Validates an opaque short-lived credential without logging or retaining it.
///
/// Implementations are expected to enforce issuer, audience, expiry, and revocation policy. A
/// display name or caller-selected identifier is not an authenticated principal.
pub trait SessionCredentialVerifier: Send + Sync {
    fn verify(&self, credential: &[u8]) -> Option<AuthenticatedPrincipal>;
}

pub struct AuthenticatedSession {
    connection: quinn::Connection,
    principal: AuthenticatedPrincipal,
    client_nonce: u64,
    session_id: u64,
    server_nonce: u64,
}

impl AuthenticatedSession {
    #[must_use]
    pub const fn connection(&self) -> &quinn::Connection {
        &self.connection
    }

    #[must_use]
    pub const fn principal(&self) -> AuthenticatedPrincipal {
        self.principal
    }

    #[must_use]
    pub const fn client_nonce(&self) -> u64 {
        self.client_nonce
    }

    #[must_use]
    pub const fn session_id(&self) -> u64 {
        self.session_id
    }

    #[must_use]
    pub const fn server_nonce(&self) -> u64 {
        self.server_nonce
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SessionCodecError {
    Oversized(usize),
    InvalidLength { expected: usize, actual: usize },
    InvalidMagic,
    UnsupportedVersion(u8),
    InvalidKind(u8),
    InvalidNonce,
    InvalidSession,
    InvalidCredentialLength(usize),
}

impl fmt::Display for SessionCodecError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Oversized(bytes) => write!(formatter, "session message has {bytes} bytes"),
            Self::InvalidLength { expected, actual } => write!(
                formatter,
                "session message length mismatch: expected {expected}, received {actual}"
            ),
            Self::InvalidMagic => write!(formatter, "invalid session magic"),
            Self::UnsupportedVersion(version) => {
                write!(formatter, "unsupported session version {version}")
            }
            Self::InvalidKind(kind) => write!(formatter, "invalid session message kind {kind}"),
            Self::InvalidNonce => write!(formatter, "session nonce must be non-zero"),
            Self::InvalidSession => write!(formatter, "session ID must be non-zero"),
            Self::InvalidCredentialLength(bytes) => {
                write!(formatter, "invalid session credential length {bytes}")
            }
        }
    }
}

impl std::error::Error for SessionCodecError {}

#[derive(Debug)]
pub enum SessionAdmissionError {
    Connection(quinn::ConnectionError),
    Read(quinn::ReadToEndError),
    Write(quinn::WriteError),
    Finish(quinn::ClosedStream),
    Codec(SessionCodecError),
    RejectedCredential,
    NonceMismatch,
    TimedOut,
}

impl fmt::Display for SessionAdmissionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Connection(error) => error.fmt(formatter),
            Self::Read(error) => error.fmt(formatter),
            Self::Write(error) => error.fmt(formatter),
            Self::Finish(error) => error.fmt(formatter),
            Self::Codec(error) => error.fmt(formatter),
            Self::RejectedCredential => write!(formatter, "session credential rejected"),
            Self::NonceMismatch => write!(formatter, "session welcome nonce mismatch"),
            Self::TimedOut => write!(formatter, "session admission timed out"),
        }
    }
}

impl std::error::Error for SessionAdmissionError {}

#[derive(Debug)]
pub enum SecureConfigError {
    Rustls(quinn::rustls::Error),
    NoInitialCipherSuite(NoInitialCipherSuite),
}

impl fmt::Display for SecureConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Rustls(error) => error.fmt(formatter),
            Self::NoInitialCipherSuite(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for SecureConfigError {}

#[derive(Debug)]
pub enum SecureDatagramError {
    Empty,
    Oversized(usize),
    Transport(SendDatagramError),
}

impl fmt::Display for SecureDatagramError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => write!(formatter, "empty gameplay datagram"),
            Self::Oversized(bytes) => write!(formatter, "gameplay datagram has {bytes} bytes"),
            Self::Transport(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for SecureDatagramError {}

#[derive(Debug)]
pub enum SecureDatagramReceiveError {
    Empty,
    Oversized(usize),
    Connection(quinn::ConnectionError),
}

impl fmt::Display for SecureDatagramReceiveError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => write!(formatter, "empty gameplay datagram"),
            Self::Oversized(bytes) => write!(formatter, "gameplay datagram has {bytes} bytes"),
            Self::Connection(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for SecureDatagramReceiveError {}

/// Builds a TLS 1.3 QUIC server configuration with a game-specific ALPN and bounded buffers.
///
/// # Errors
///
/// Rejects invalid certificate chains, private keys, or unavailable QUIC cipher suites.
pub fn secure_server_config(
    certificate_chain: Vec<CertificateDer<'static>>,
    private_key: PrivateKeyDer<'static>,
) -> Result<ServerConfig, SecureConfigError> {
    let mut tls = RustlsServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certificate_chain, private_key)
        .map_err(SecureConfigError::Rustls)?;
    tls.alpn_protocols = vec![ALPN_PROTOCOL.to_vec()];
    let crypto =
        QuicServerConfig::try_from(tls).map_err(SecureConfigError::NoInitialCipherSuite)?;
    let mut config = ServerConfig::with_crypto(Arc::new(crypto));
    config.transport_config(bounded_transport_config());
    config.max_incoming(MAX_PENDING_QUIC_HANDSHAKES);
    config.incoming_buffer_size(PENDING_HANDSHAKE_BUFFER_BYTES);
    config.incoming_buffer_size_total(TOTAL_PENDING_HANDSHAKE_BUFFER_BYTES);
    Ok(config)
}

/// Builds a TLS 1.3 QUIC client configuration from explicit trusted roots.
///
/// This deliberately does not enable 0-RTT, which would permit replay of non-idempotent gameplay
/// commands.
///
/// # Errors
///
/// Returns an error when the selected crypto provider lacks QUIC's required initial cipher suite.
pub fn secure_client_config(roots: RootCertStore) -> Result<ClientConfig, SecureConfigError> {
    let mut tls = RustlsClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth();
    tls.alpn_protocols = vec![ALPN_PROTOCOL.to_vec()];
    let crypto =
        QuicClientConfig::try_from(tls).map_err(SecureConfigError::NoInitialCipherSuite)?;
    let mut config = ClientConfig::new(Arc::new(crypto));
    config.transport_config(bounded_transport_config());
    Ok(config)
}

/// Sends one bounded unreliable gameplay payload through an established authenticated connection.
///
/// # Errors
///
/// Rejects empty or oversized payloads before entering Quinn and forwards bounded transport errors.
pub fn send_gameplay_datagram(
    connection: &quinn::Connection,
    payload: Vec<u8>,
) -> Result<(), SecureDatagramError> {
    if payload.is_empty() {
        return Err(SecureDatagramError::Empty);
    }
    if payload.len() > MAX_QUIC_DATAGRAM_PAYLOAD_BYTES {
        return Err(SecureDatagramError::Oversized(payload.len()));
    }
    connection
        .send_datagram(payload.into())
        .map_err(SecureDatagramError::Transport)
}

/// Receives one bounded unreliable gameplay payload from an authenticated connection.
///
/// # Errors
///
/// Rejects empty or oversized peer payloads before application decoding and forwards connection
/// failures.
pub async fn receive_gameplay_datagram(
    connection: &quinn::Connection,
) -> Result<Bytes, SecureDatagramReceiveError> {
    let payload = connection
        .read_datagram()
        .await
        .map_err(SecureDatagramReceiveError::Connection)?;
    if payload.is_empty() {
        return Err(SecureDatagramReceiveError::Empty);
    }
    if payload.len() > MAX_QUIC_DATAGRAM_PAYLOAD_BYTES {
        return Err(SecureDatagramReceiveError::Oversized(payload.len()));
    }
    Ok(payload)
}

/// Authenticates the first reliable stream and returns a connection-bound principal.
///
/// The credential is borrowed only during the verifier call, zeroed immediately afterwards, and is
/// never included in errors. A fixed internal deadline prevents a peer from retaining an admission
/// task indefinitely. QUIC 0-RTT must not be used for this stream or for gameplay mutations.
///
/// # Errors
///
/// Rejects stream failures, malformed messages, invalid credentials, or invalid server IDs. The
/// connection is closed on every rejected admission.
pub async fn admit_session(
    connection: quinn::Connection,
    verifier: &(impl SessionCredentialVerifier + ?Sized),
    session_id: NonZeroU64,
    server_nonce: NonZeroU64,
) -> Result<AuthenticatedSession, SessionAdmissionError> {
    let result = tokio::time::timeout(
        SESSION_ADMISSION_TIMEOUT,
        admit_session_inner(&connection, verifier, session_id.get(), server_nonce.get()),
    )
    .await
    .unwrap_or(Err(SessionAdmissionError::TimedOut));
    match result {
        Ok((principal, client_nonce)) => Ok(AuthenticatedSession {
            connection,
            principal,
            client_nonce,
            session_id: session_id.get(),
            server_nonce: server_nonce.get(),
        }),
        Err(error) => {
            connection.close(VarInt::from_u32(0x100), b"session admission rejected");
            Err(error)
        }
    }
}

/// Establishes an authenticated application session over an already server-authenticated QUIC
/// connection.
///
/// # Errors
///
/// Rejects malformed local input, QUIC stream failures, oversized responses, and nonce mismatch.
pub async fn establish_session(
    connection: &quinn::Connection,
    client_nonce: u64,
    credential: &[u8],
) -> Result<SessionWelcome, SessionAdmissionError> {
    let result = tokio::time::timeout(
        SESSION_ADMISSION_TIMEOUT,
        establish_session_inner(connection, client_nonce, credential),
    )
    .await
    .unwrap_or(Err(SessionAdmissionError::TimedOut));
    if result.is_err() {
        connection.close(VarInt::from_u32(0x101), b"session establishment rejected");
    }
    result
}

async fn establish_session_inner(
    connection: &quinn::Connection,
    client_nonce: u64,
    credential: &[u8],
) -> Result<SessionWelcome, SessionAdmissionError> {
    let hello = Zeroizing::new(
        encode_session_hello(client_nonce, credential).map_err(SessionAdmissionError::Codec)?,
    );
    let (mut send, mut receive) = connection
        .open_bi()
        .await
        .map_err(SessionAdmissionError::Connection)?;
    send.write_all(hello.as_slice())
        .await
        .map_err(SessionAdmissionError::Write)?;
    send.finish().map_err(SessionAdmissionError::Finish)?;
    drop(hello);
    let response = receive
        .read_to_end(SESSION_WELCOME_BYTES)
        .await
        .map_err(SessionAdmissionError::Read)?;
    let welcome = decode_session_welcome(&response).map_err(SessionAdmissionError::Codec)?;
    if welcome.client_nonce != client_nonce {
        return Err(SessionAdmissionError::NonceMismatch);
    }
    Ok(welcome)
}

/// Encodes a bounded opaque credential for the post-TLS admission stream.
///
/// # Errors
///
/// Rejects zero nonces and credentials outside the fixed 16-byte to four-KiB envelope.
pub fn encode_session_hello(
    client_nonce: u64,
    credential: &[u8],
) -> Result<Vec<u8>, SessionCodecError> {
    validate_hello_fields(client_nonce, credential.len())?;
    let mut bytes = session_prefix(
        SESSION_HELLO_KIND,
        SESSION_HELLO_FIXED_BYTES + credential.len(),
    );
    push_u64(&mut bytes, client_nonce);
    push_u16(
        &mut bytes,
        u16::try_from(credential.len())
            .map_err(|_| SessionCodecError::InvalidCredentialLength(credential.len()))?,
    );
    bytes.extend_from_slice(credential);
    Ok(bytes)
}

/// Decodes one bounded post-TLS admission message while borrowing its opaque credential.
///
/// # Errors
///
/// Rejects malformed, oversized, mismatched, zero-nonce, or invalid-length messages.
pub fn decode_session_hello(bytes: &[u8]) -> Result<SessionHello<'_>, SessionCodecError> {
    validate_prefix(bytes, SESSION_HELLO_KIND, MAX_SESSION_HELLO_BYTES)?;
    if bytes.len() < SESSION_HELLO_FIXED_BYTES {
        return Err(SessionCodecError::InvalidLength {
            expected: SESSION_HELLO_FIXED_BYTES,
            actual: bytes.len(),
        });
    }
    let client_nonce = read_u64(bytes, SESSION_PREFIX_BYTES);
    let credential_bytes = usize::from(read_u16(bytes, SESSION_PREFIX_BYTES + 8));
    validate_hello_fields(client_nonce, credential_bytes)?;
    let expected = SESSION_HELLO_FIXED_BYTES.saturating_add(credential_bytes);
    if bytes.len() != expected {
        return Err(SessionCodecError::InvalidLength {
            expected,
            actual: bytes.len(),
        });
    }
    Ok(SessionHello {
        client_nonce,
        credential: &bytes[SESSION_HELLO_FIXED_BYTES..],
    })
}

/// Encodes the server's connection-bound session assignment.
///
/// # Errors
///
/// Rejects zero nonce or session identifiers.
pub fn encode_session_welcome(message: SessionWelcome) -> Result<Vec<u8>, SessionCodecError> {
    validate_welcome(message)?;
    let mut bytes = session_prefix(SESSION_WELCOME_KIND, SESSION_WELCOME_BYTES);
    push_u64(&mut bytes, message.client_nonce);
    push_u64(&mut bytes, message.session_id);
    push_u64(&mut bytes, message.server_nonce);
    Ok(bytes)
}

/// Decodes one exact-size server session assignment.
///
/// # Errors
///
/// Rejects malformed, oversized, wrong-kind, or zero identifier messages.
pub fn decode_session_welcome(bytes: &[u8]) -> Result<SessionWelcome, SessionCodecError> {
    validate_prefix(bytes, SESSION_WELCOME_KIND, SESSION_WELCOME_BYTES)?;
    if bytes.len() != SESSION_WELCOME_BYTES {
        return Err(SessionCodecError::InvalidLength {
            expected: SESSION_WELCOME_BYTES,
            actual: bytes.len(),
        });
    }
    let message = SessionWelcome {
        client_nonce: read_u64(bytes, SESSION_PREFIX_BYTES),
        session_id: read_u64(bytes, SESSION_PREFIX_BYTES + 8),
        server_nonce: read_u64(bytes, SESSION_PREFIX_BYTES + 16),
    };
    validate_welcome(message)?;
    Ok(message)
}

fn bounded_transport_config() -> Arc<TransportConfig> {
    let mut transport = TransportConfig::default();
    transport
        .max_concurrent_bidi_streams(VarInt::from_u32(1))
        .max_concurrent_uni_streams(VarInt::from_u32(0))
        .max_idle_timeout(Some(VarInt::from_u32(15_000).into()))
        .keep_alive_interval(Some(Duration::from_secs(5)))
        .stream_receive_window(VarInt::from_u32(QUIC_STREAM_WINDOW_BYTES))
        .receive_window(VarInt::from_u32(QUIC_CONNECTION_WINDOW_BYTES))
        .send_window(QUIC_SEND_WINDOW_BYTES)
        .crypto_buffer_size(QUIC_CRYPTO_BUFFER_BYTES)
        .datagram_receive_buffer_size(Some(QUIC_DATAGRAM_BUFFER_BYTES))
        .datagram_send_buffer_size(QUIC_DATAGRAM_BUFFER_BYTES);
    Arc::new(transport)
}

async fn admit_session_inner(
    connection: &quinn::Connection,
    verifier: &(impl SessionCredentialVerifier + ?Sized),
    session_id: u64,
    server_nonce: u64,
) -> Result<(AuthenticatedPrincipal, u64), SessionAdmissionError> {
    let (mut send, mut receive) = connection
        .accept_bi()
        .await
        .map_err(SessionAdmissionError::Connection)?;
    let request = Zeroizing::new(
        receive
            .read_to_end(MAX_SESSION_HELLO_BYTES)
            .await
            .map_err(SessionAdmissionError::Read)?,
    );
    let (principal, client_nonce) = {
        let hello =
            decode_session_hello(request.as_slice()).map_err(SessionAdmissionError::Codec)?;
        let principal = verifier
            .verify(hello.credential)
            .ok_or(SessionAdmissionError::RejectedCredential)?;
        (principal, hello.client_nonce)
    };
    drop(request);
    let response = encode_session_welcome(SessionWelcome {
        client_nonce,
        session_id,
        server_nonce,
    })
    .map_err(SessionAdmissionError::Codec)?;
    send.write_all(&response)
        .await
        .map_err(SessionAdmissionError::Write)?;
    send.finish().map_err(SessionAdmissionError::Finish)?;
    Ok((principal, client_nonce))
}

fn validate_prefix(bytes: &[u8], kind: u8, maximum: usize) -> Result<(), SessionCodecError> {
    if bytes.len() > maximum {
        return Err(SessionCodecError::Oversized(bytes.len()));
    }
    if bytes.len() < SESSION_PREFIX_BYTES {
        return Err(SessionCodecError::InvalidLength {
            expected: SESSION_PREFIX_BYTES,
            actual: bytes.len(),
        });
    }
    if bytes[..4] != SESSION_MAGIC {
        return Err(SessionCodecError::InvalidMagic);
    }
    if bytes[4] != SESSION_VERSION {
        return Err(SessionCodecError::UnsupportedVersion(bytes[4]));
    }
    if bytes[5] != kind {
        return Err(SessionCodecError::InvalidKind(bytes[5]));
    }
    Ok(())
}

const fn validate_hello_fields(
    client_nonce: u64,
    credential_bytes: usize,
) -> Result<(), SessionCodecError> {
    if client_nonce == 0 {
        return Err(SessionCodecError::InvalidNonce);
    }
    if credential_bytes < MIN_SESSION_CREDENTIAL_BYTES
        || credential_bytes > MAX_SESSION_CREDENTIAL_BYTES
    {
        return Err(SessionCodecError::InvalidCredentialLength(credential_bytes));
    }
    Ok(())
}

const fn validate_welcome(message: SessionWelcome) -> Result<(), SessionCodecError> {
    if message.client_nonce == 0 || message.server_nonce == 0 {
        return Err(SessionCodecError::InvalidNonce);
    }
    if message.session_id == 0 {
        return Err(SessionCodecError::InvalidSession);
    }
    Ok(())
}

fn session_prefix(kind: u8, capacity: usize) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(capacity);
    bytes.extend_from_slice(&SESSION_MAGIC);
    bytes.push(SESSION_VERSION);
    bytes.push(kind);
    bytes
}

fn push_u16(bytes: &mut Vec<u8>, value: u16) {
    bytes.extend_from_slice(&value.to_le_bytes());
}

fn push_u64(bytes: &mut Vec<u8>, value: u64) {
    bytes.extend_from_slice(&value.to_le_bytes());
}

fn read_u16(bytes: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes(
        bytes[offset..offset + 2]
            .try_into()
            .expect("session field bounds were validated"),
    )
}

fn read_u64(bytes: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes(
        bytes[offset..offset + 8]
            .try_into()
            .expect("session field bounds were validated"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestVerifier([u8; 32]);

    impl SessionCredentialVerifier for TestVerifier {
        fn verify(&self, credential: &[u8]) -> Option<AuthenticatedPrincipal> {
            (credential == self.0).then(|| {
                AuthenticatedPrincipal::new(NonZeroU64::new(7).expect("non-zero test principal"))
            })
        }
    }

    #[test]
    fn session_messages_round_trip_without_copying_credentials() {
        let credential = [0x5a; 32];
        let encoded = encode_session_hello(11, &credential).expect("session hello");
        let decoded = decode_session_hello(&encoded).expect("decode session hello");
        assert_eq!(decoded.client_nonce, 11);
        assert_eq!(decoded.credential, credential);
        assert_eq!(
            decoded.credential.as_ptr(),
            encoded[SESSION_HELLO_FIXED_BYTES..].as_ptr()
        );
        let principal = TestVerifier(credential)
            .verify(decoded.credential)
            .expect("valid opaque credential");
        assert_eq!(principal.get(), 7);

        let welcome = SessionWelcome {
            client_nonce: 11,
            session_id: 13,
            server_nonce: 17,
        };
        assert_eq!(
            decode_session_welcome(&encode_session_welcome(welcome).expect("session welcome")),
            Ok(welcome)
        );
    }

    #[test]
    fn malformed_session_messages_fail_before_unbounded_work() {
        assert_eq!(
            encode_session_hello(0, &[1; 32]),
            Err(SessionCodecError::InvalidNonce)
        );
        assert_eq!(
            encode_session_hello(1, &[1; 8]),
            Err(SessionCodecError::InvalidCredentialLength(8))
        );
        assert!(matches!(
            decode_session_hello(&vec![0; MAX_SESSION_HELLO_BYTES + 1]),
            Err(SessionCodecError::Oversized(_))
        ));
        let verifier = TestVerifier([2; 32]);
        assert_eq!(verifier.verify(&[3; 32]), None);
    }
}
