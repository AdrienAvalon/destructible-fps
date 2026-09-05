//! Fail-closed QUIC/TLS client bootstrap for graphical and headless frontends.

use crate::{
    MAX_SESSION_CREDENTIAL_BYTES, SecureConfigError, SecureDatagramError,
    SecureDatagramReceiveError, SessionAdmissionError, establish_session,
    receive_gameplay_datagram, secure_client_config, send_gameplay_datagram,
};
use bytes::Bytes;
use quinn::rustls::pki_types::{CertificateDer, pem::PemObject};
use quinn::{Connection, Endpoint, rustls::RootCertStore};
use ring::rand::{SecureRandom, SystemRandom};
use std::{
    fmt, fs, io,
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
    path::{Path, PathBuf},
    time::Duration,
};
use zeroize::Zeroizing;

pub const MAX_CLIENT_ROOT_CERTIFICATE_BYTES: usize = 256 * 1_024;
pub const MAX_CLIENT_ROOT_CERTIFICATES: usize = 16;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SecureClientLaunchConfig {
    pub server_address: SocketAddr,
    pub server_name: String,
    pub root_certificate_file: PathBuf,
    pub credential_file: PathBuf,
}

#[derive(Debug)]
pub enum SecureClientError {
    RelativePath(&'static str),
    InvalidServerName,
    FileMetadata {
        kind: &'static str,
        source: io::Error,
    },
    FileRead {
        kind: &'static str,
        source: io::Error,
    },
    FileTooLarge {
        kind: &'static str,
        bytes: usize,
        maximum: usize,
    },
    InsecureCredentialPermissions(u32),
    InvalidCertificate,
    InvalidCertificateCount(usize),
    RandomNonce,
    ClientConfiguration(SecureConfigError),
    Endpoint(io::Error),
    Connect(quinn::ConnectError),
    Connection(quinn::ConnectionError),
    ConnectTimeout,
    Admission(SessionAdmissionError),
    Send(SecureDatagramError),
    Receive(SecureDatagramReceiveError),
}

impl fmt::Display for SecureClientError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RelativePath(kind) => write!(formatter, "{kind} path must be absolute"),
            Self::InvalidServerName => write!(formatter, "invalid TLS server name"),
            Self::FileMetadata { kind, source } => {
                write!(formatter, "cannot inspect {kind} file: {source}")
            }
            Self::FileRead { kind, source } => {
                write!(formatter, "cannot read {kind} file: {source}")
            }
            Self::FileTooLarge {
                kind,
                bytes,
                maximum,
            } => write!(
                formatter,
                "{kind} file has {bytes} bytes; maximum is {maximum}"
            ),
            Self::InsecureCredentialPermissions(mode) => write!(
                formatter,
                "credential file permissions {:04o} expose secret material",
                mode & 0o7777
            ),
            Self::InvalidCertificate => write!(formatter, "invalid root certificate PEM"),
            Self::InvalidCertificateCount(count) => write!(
                formatter,
                "root certificate file has {count} certificates; expected 1..={MAX_CLIENT_ROOT_CERTIFICATES}"
            ),
            Self::RandomNonce => write!(formatter, "cryptographic nonce generation failed"),
            Self::ClientConfiguration(error) => error.fmt(formatter),
            Self::Endpoint(error) => write!(formatter, "cannot bind QUIC client endpoint: {error}"),
            Self::Connect(error) => {
                write!(formatter, "cannot start verified QUIC connection: {error}")
            }
            Self::Connection(error) => {
                write!(formatter, "verified QUIC connection failed: {error}")
            }
            Self::ConnectTimeout => write!(formatter, "verified QUIC connection timed out"),
            Self::Admission(error) => error.fmt(formatter),
            Self::Send(error) => error.fmt(formatter),
            Self::Receive(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for SecureClientError {}

pub struct SecureClientConnection {
    endpoint: Endpoint,
    connection: Connection,
    session_id: u64,
    server_nonce: u64,
}

impl SecureClientConnection {
    /// Establishes a certificate-verified QUIC connection and a credential-authenticated session.
    ///
    /// # Errors
    ///
    /// Fails closed on path, size, permission, certificate, transport, timeout, nonce, or admission
    /// errors. Credential bytes are zeroed when this function returns.
    pub async fn connect(config: &SecureClientLaunchConfig) -> Result<Self, SecureClientError> {
        validate_config(config)?;
        let certificate_bytes = read_bounded_file(
            &config.root_certificate_file,
            "root certificate",
            MAX_CLIENT_ROOT_CERTIFICATE_BYTES,
        )?;
        let roots = parse_root_certificates(&certificate_bytes)?;
        check_credential_permissions(&config.credential_file)?;
        let credential = Zeroizing::new(read_bounded_file(
            &config.credential_file,
            "credential",
            MAX_SESSION_CREDENTIAL_BYTES,
        )?);
        let credential = trim_ascii_whitespace(&credential);
        let client_config =
            secure_client_config(roots).map_err(SecureClientError::ClientConfiguration)?;
        let bind_address = match config.server_address.ip() {
            IpAddr::V4(_) => SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 0),
            IpAddr::V6(_) => SocketAddr::new(IpAddr::V6(Ipv6Addr::UNSPECIFIED), 0),
        };
        let mut endpoint = Endpoint::client(bind_address).map_err(SecureClientError::Endpoint)?;
        endpoint.set_default_client_config(client_config);
        let connecting = endpoint
            .connect(config.server_address, &config.server_name)
            .map_err(SecureClientError::Connect)?;
        let connection = tokio::time::timeout(CONNECT_TIMEOUT, connecting)
            .await
            .map_err(|_| SecureClientError::ConnectTimeout)?
            .map_err(SecureClientError::Connection)?;
        let client_nonce = cryptographic_nonce()?;
        let welcome = establish_session(&connection, client_nonce, credential)
            .await
            .map_err(SecureClientError::Admission)?;
        Ok(Self {
            endpoint,
            connection,
            session_id: welcome.session_id,
            server_nonce: welcome.server_nonce,
        })
    }

    #[must_use]
    pub const fn session_id(&self) -> u64 {
        self.session_id
    }

    #[must_use]
    pub const fn server_nonce(&self) -> u64 {
        self.server_nonce
    }

    /// Sends one bounded encrypted gameplay datagram.
    ///
    /// # Errors
    ///
    /// Forwards local payload validation and QUIC transport failures.
    pub fn send(&self, payload: Vec<u8>) -> Result<(), SecureClientError> {
        send_gameplay_datagram(&self.connection, payload).map_err(SecureClientError::Send)
    }

    /// Receives one bounded encrypted gameplay datagram.
    ///
    /// # Errors
    ///
    /// Forwards connection closure and payload bound failures.
    pub async fn receive(&self) -> Result<Bytes, SecureClientError> {
        receive_gameplay_datagram(&self.connection)
            .await
            .map_err(SecureClientError::Receive)
    }

    pub fn close(&self) {
        self.connection
            .close(quinn::VarInt::from_u32(0), b"client shutdown");
        self.endpoint
            .close(quinn::VarInt::from_u32(0), b"client shutdown");
    }
}

impl Drop for SecureClientConnection {
    fn drop(&mut self) {
        self.close();
    }
}

fn validate_config(config: &SecureClientLaunchConfig) -> Result<(), SecureClientError> {
    if !config.root_certificate_file.is_absolute() {
        return Err(SecureClientError::RelativePath("root certificate"));
    }
    if !config.credential_file.is_absolute() {
        return Err(SecureClientError::RelativePath("credential"));
    }
    if config.server_name.is_empty()
        || config.server_name.len() > 253
        || config
            .server_name
            .bytes()
            .any(|byte| byte == 0 || byte.is_ascii_whitespace())
    {
        return Err(SecureClientError::InvalidServerName);
    }
    Ok(())
}

fn read_bounded_file(
    path: &Path,
    kind: &'static str,
    maximum: usize,
) -> Result<Vec<u8>, SecureClientError> {
    let metadata =
        fs::metadata(path).map_err(|source| SecureClientError::FileMetadata { kind, source })?;
    let announced = usize::try_from(metadata.len()).unwrap_or(usize::MAX);
    if announced > maximum {
        return Err(SecureClientError::FileTooLarge {
            kind,
            bytes: announced,
            maximum,
        });
    }
    let bytes = fs::read(path).map_err(|source| SecureClientError::FileRead { kind, source })?;
    if bytes.len() > maximum {
        return Err(SecureClientError::FileTooLarge {
            kind,
            bytes: bytes.len(),
            maximum,
        });
    }
    Ok(bytes)
}

fn parse_root_certificates(bytes: &[u8]) -> Result<RootCertStore, SecureClientError> {
    let certificates = CertificateDer::pem_slice_iter(bytes)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| SecureClientError::InvalidCertificate)?;
    if certificates.is_empty() || certificates.len() > MAX_CLIENT_ROOT_CERTIFICATES {
        return Err(SecureClientError::InvalidCertificateCount(
            certificates.len(),
        ));
    }
    let mut roots = RootCertStore::empty();
    for certificate in certificates {
        roots
            .add(certificate)
            .map_err(|_| SecureClientError::InvalidCertificate)?;
    }
    Ok(roots)
}

fn cryptographic_nonce() -> Result<u64, SecureClientError> {
    let mut bytes = [0_u8; 8];
    SystemRandom::new()
        .fill(&mut bytes)
        .map_err(|_| SecureClientError::RandomNonce)?;
    Ok(u64::from_le_bytes(bytes).max(1))
}

fn trim_ascii_whitespace(bytes: &[u8]) -> &[u8] {
    let start = bytes
        .iter()
        .position(|byte| !byte.is_ascii_whitespace())
        .unwrap_or(bytes.len());
    let end = bytes
        .iter()
        .rposition(|byte| !byte.is_ascii_whitespace())
        .map_or(start, |index| index + 1);
    &bytes[start..end]
}

#[cfg(unix)]
fn check_credential_permissions(path: &Path) -> Result<(), SecureClientError> {
    use std::os::unix::fs::PermissionsExt;
    let metadata = fs::metadata(path).map_err(|source| SecureClientError::FileMetadata {
        kind: "credential",
        source,
    })?;
    let mode = metadata.permissions().mode();
    if mode & 0o077 != 0 {
        return Err(SecureClientError::InsecureCredentialPermissions(mode));
    }
    Ok(())
}

#[cfg(not(unix))]
fn check_credential_permissions(path: &Path) -> Result<(), SecureClientError> {
    fs::metadata(path)
        .map(|_| ())
        .map_err(|source| SecureClientError::FileMetadata {
            kind: "credential",
            source,
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn configuration_rejects_relative_secret_paths_and_invalid_names() {
        let mut config = SecureClientLaunchConfig {
            server_address: "127.0.0.1:4433".parse().expect("address"),
            server_name: "localhost".to_owned(),
            root_certificate_file: PathBuf::from("ca.pem"),
            credential_file: PathBuf::from("token"),
        };
        assert!(matches!(
            validate_config(&config),
            Err(SecureClientError::RelativePath("root certificate"))
        ));
        config.root_certificate_file = PathBuf::from("/tmp/ca.pem");
        assert!(matches!(
            validate_config(&config),
            Err(SecureClientError::RelativePath("credential"))
        ));
        config.credential_file = PathBuf::from("/tmp/token");
        config.server_name = "bad name".to_owned();
        assert!(matches!(
            validate_config(&config),
            Err(SecureClientError::InvalidServerName)
        ));
    }

    #[test]
    fn credential_trimming_never_changes_internal_bytes() {
        assert_eq!(
            trim_ascii_whitespace(b" \nopaque token\r\n"),
            b"opaque token"
        );
        assert!(trim_ascii_whitespace(b"\t \n").is_empty());
    }
}
