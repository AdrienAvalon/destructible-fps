//! Fail-closed file configuration for the standalone secure authority.

use crate::{
    MAX_OIDC_JWKS_BYTES, OidcSessionVerifier, OidcVerificationError, SecureConfigError,
    SecureDedicatedServer, SessionCredentialVerifier, World, secure_server_config,
};
use core::fmt;
use quinn::{
    ServerConfig,
    rustls::pki_types::{CertificateDer, PrivateKeyDer, pem::PemObject},
};
use serde::Deserialize;
use std::{
    fs::{self, File, Metadata},
    io::{self, Read},
    net::SocketAddr,
    num::NonZeroU64,
    path::{Path, PathBuf},
    sync::Arc,
    time::{Instant, SystemTime, UNIX_EPOCH},
};
use zeroize::Zeroizing;

pub const MAX_SECURE_CONFIG_BYTES: usize = 16 * 1_024;
pub const MAX_CERTIFICATE_CHAIN_BYTES: usize = 256 * 1_024;
pub const MAX_CERTIFICATE_CHAIN_ENTRIES: usize = 8;
pub const MAX_TLS_PRIVATE_KEY_BYTES: usize = 64 * 1_024;
pub const MIN_STATIC_JWKS_VALIDITY_SECONDS: u64 = 60;
pub const MAX_STATIC_JWKS_VALIDITY_SECONDS: u64 = 24 * 60 * 60;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum SecureNetworkExposure {
    Loopback,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawSecureAuthorityConfig {
    bind: SocketAddr,
    exposure: SecureNetworkExposure,
    certificate_chain_file: PathBuf,
    private_key_file: PathBuf,
    oidc_jwks_file: PathBuf,
    oidc_issuer: String,
    oidc_audience: String,
    jwks_valid_until_unix_seconds: u64,
    #[serde(default)]
    max_ticks: Option<NonZeroU64>,
    #[serde(default)]
    stop_after_commands: Option<std::num::NonZeroUsize>,
}

/// Validated, fully materialized inputs required to start one secure authority process.
///
/// The private key and OIDC data are loaded once from bounded regular files. The public API does
/// not expose those buffers, and this milestone deliberately permits only loopback endpoints.
pub struct SecureAuthorityLaunchConfig {
    bind: SocketAddr,
    exposure: SecureNetworkExposure,
    server_config: ServerConfig,
    verifier: Arc<dyn SessionCredentialVerifier>,
    jwks_expiration_deadline: Instant,
    max_ticks: Option<NonZeroU64>,
    stop_after_commands: Option<std::num::NonZeroUsize>,
}

#[derive(Debug)]
pub enum SecureAuthorityLaunchError {
    File {
        purpose: &'static str,
        source: io::Error,
    },
    InvalidFileType(&'static str),
    UnsafeFilePermissions(&'static str),
    OversizedFile {
        purpose: &'static str,
        bytes: u64,
        maximum: usize,
    },
    InvalidConfiguration,
    InvalidCertificateChain,
    InvalidPrivateKey,
    InvalidJwksLifetime,
    Oidc(OidcVerificationError),
    Transport(SecureConfigError),
    Bind(io::Error),
}

impl fmt::Display for SecureAuthorityLaunchError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::File { purpose, source } => write!(formatter, "cannot read {purpose}: {source}"),
            Self::InvalidFileType(purpose) => write!(formatter, "{purpose} is not a regular file"),
            Self::UnsafeFilePermissions(purpose) => {
                write!(formatter, "unsafe permissions on {purpose}")
            }
            Self::OversizedFile {
                purpose,
                bytes,
                maximum,
            } => write!(
                formatter,
                "{purpose} has {bytes} bytes, exceeding the {maximum}-byte limit"
            ),
            Self::InvalidConfiguration => {
                write!(formatter, "invalid secure authority configuration")
            }
            Self::InvalidCertificateChain => write!(formatter, "invalid TLS certificate chain"),
            Self::InvalidPrivateKey => write!(formatter, "invalid TLS private key"),
            Self::InvalidJwksLifetime => write!(formatter, "invalid static JWKS validity window"),
            Self::Oidc(error) => error.fmt(formatter),
            Self::Transport(error) => error.fmt(formatter),
            Self::Bind(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for SecureAuthorityLaunchError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::File { source, .. } | Self::Bind(source) => Some(source),
            Self::Oidc(source) => Some(source),
            Self::Transport(source) => Some(source),
            _ => None,
        }
    }
}

impl SecureAuthorityLaunchConfig {
    /// Reads and validates a bounded JSON configuration and every referenced credential file.
    ///
    /// # Errors
    ///
    /// Rejects links, non-regular or oversized files, unsafe Unix permissions, unknown fields,
    /// relative credential paths, non-loopback binds, invalid TLS material, invalid OIDC policy,
    /// and JWKS validity windows outside one minute to 24 hours.
    pub fn load(path: impl AsRef<Path>) -> Result<Self, SecureAuthorityLaunchError> {
        let config_bytes = read_bounded_file(
            path.as_ref(),
            "secure authority configuration",
            MAX_SECURE_CONFIG_BYTES,
            FilePermissionPolicy::Integrity,
        )?;
        let raw = serde_json::from_slice::<RawSecureAuthorityConfig>(&config_bytes)
            .map_err(|_| SecureAuthorityLaunchError::InvalidConfiguration)?;
        let jwks_remaining_validity = validate_raw_config(&raw)?;
        let jwks_expiration_deadline = Instant::now()
            .checked_add(std::time::Duration::from_secs(jwks_remaining_validity))
            .ok_or(SecureAuthorityLaunchError::InvalidJwksLifetime)?;

        let certificate_bytes = read_bounded_file(
            &raw.certificate_chain_file,
            "TLS certificate chain",
            MAX_CERTIFICATE_CHAIN_BYTES,
            FilePermissionPolicy::Integrity,
        )?;
        let private_key_bytes = Zeroizing::new(read_bounded_file(
            &raw.private_key_file,
            "TLS private key",
            MAX_TLS_PRIVATE_KEY_BYTES,
            FilePermissionPolicy::Private,
        )?);
        let jwks = read_bounded_file(
            &raw.oidc_jwks_file,
            "OIDC JWKS",
            MAX_OIDC_JWKS_BYTES,
            FilePermissionPolicy::Integrity,
        )?;

        let certificates = parse_certificates(&certificate_bytes)?;
        let private_key = parse_private_key(&private_key_bytes)?;
        let server_config = secure_server_config(certificates, private_key)
            .map_err(SecureAuthorityLaunchError::Transport)?;
        let oidc = OidcSessionVerifier::new(raw.oidc_issuer, raw.oidc_audience, &jwks)
            .map_err(SecureAuthorityLaunchError::Oidc)?;
        let verifier: Arc<dyn SessionCredentialVerifier> = Arc::new(ExpiringOidcVerifier {
            inner: oidc,
            expiration_deadline: jwks_expiration_deadline,
        });

        Ok(Self {
            bind: raw.bind,
            exposure: raw.exposure,
            server_config,
            verifier,
            jwks_expiration_deadline,
            max_ticks: raw.max_ticks,
            stop_after_commands: raw.stop_after_commands,
        })
    }

    /// Starts the loopback-only secure authority on the active Tokio runtime.
    ///
    /// # Errors
    ///
    /// Returns endpoint bind or authority construction failures.
    pub fn start(self, world: World) -> Result<SecureDedicatedServer, SecureAuthorityLaunchError> {
        SecureDedicatedServer::bind_validated_loopback(
            self.bind,
            self.server_config,
            self.verifier,
            world,
        )
        .map_err(SecureAuthorityLaunchError::Bind)
    }

    #[must_use]
    pub const fn bind_address(&self) -> SocketAddr {
        self.bind
    }

    #[must_use]
    pub const fn exposure(&self) -> SecureNetworkExposure {
        self.exposure
    }

    #[must_use]
    pub const fn jwks_expiration_deadline(&self) -> Instant {
        self.jwks_expiration_deadline
    }

    #[must_use]
    pub const fn max_ticks(&self) -> Option<NonZeroU64> {
        self.max_ticks
    }

    #[must_use]
    pub const fn stop_after_commands(&self) -> Option<std::num::NonZeroUsize> {
        self.stop_after_commands
    }
}

struct ExpiringOidcVerifier {
    inner: OidcSessionVerifier,
    expiration_deadline: Instant,
}

impl SessionCredentialVerifier for ExpiringOidcVerifier {
    fn verify(&self, credential: &[u8]) -> Option<crate::AuthenticatedPrincipal> {
        (Instant::now() < self.expiration_deadline)
            .then(|| self.inner.verify_oidc(credential).ok())
            .flatten()
    }
}

#[derive(Clone, Copy)]
enum FilePermissionPolicy {
    Integrity,
    Private,
}

fn read_bounded_file(
    path: &Path,
    purpose: &'static str,
    maximum: usize,
    permission_policy: FilePermissionPolicy,
) -> Result<Vec<u8>, SecureAuthorityLaunchError> {
    if !path.is_absolute() {
        return Err(SecureAuthorityLaunchError::InvalidConfiguration);
    }
    let link_metadata = fs::symlink_metadata(path)
        .map_err(|source| SecureAuthorityLaunchError::File { purpose, source })?;
    if link_metadata.file_type().is_symlink() || !link_metadata.is_file() {
        return Err(SecureAuthorityLaunchError::InvalidFileType(purpose));
    }
    let mut file =
        File::open(path).map_err(|source| SecureAuthorityLaunchError::File { purpose, source })?;
    let metadata = file
        .metadata()
        .map_err(|source| SecureAuthorityLaunchError::File { purpose, source })?;
    if !metadata.is_file() {
        return Err(SecureAuthorityLaunchError::InvalidFileType(purpose));
    }
    validate_permissions(&metadata, purpose, permission_policy)?;
    if metadata.len() > u64::try_from(maximum).unwrap_or(u64::MAX) {
        return Err(SecureAuthorityLaunchError::OversizedFile {
            purpose,
            bytes: metadata.len(),
            maximum,
        });
    }
    let mut bytes = Vec::with_capacity(usize::try_from(metadata.len()).unwrap_or(maximum));
    (&mut file)
        .take(u64::try_from(maximum).unwrap_or(u64::MAX).saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|source| SecureAuthorityLaunchError::File { purpose, source })?;
    if bytes.len() > maximum {
        return Err(SecureAuthorityLaunchError::OversizedFile {
            purpose,
            bytes: u64::try_from(bytes.len()).unwrap_or(u64::MAX),
            maximum,
        });
    }
    Ok(bytes)
}

#[cfg(unix)]
fn validate_permissions(
    metadata: &Metadata,
    purpose: &'static str,
    permission_policy: FilePermissionPolicy,
) -> Result<(), SecureAuthorityLaunchError> {
    use std::os::unix::fs::PermissionsExt;

    let disallowed = match permission_policy {
        FilePermissionPolicy::Integrity => 0o022,
        FilePermissionPolicy::Private => 0o077,
    };
    if metadata.permissions().mode() & disallowed != 0 {
        return Err(SecureAuthorityLaunchError::UnsafeFilePermissions(purpose));
    }
    Ok(())
}

#[cfg(not(unix))]
fn validate_permissions(
    _metadata: &Metadata,
    _purpose: &'static str,
    _permission_policy: FilePermissionPolicy,
) -> Result<(), SecureAuthorityLaunchError> {
    Ok(())
}

fn validate_raw_config(raw: &RawSecureAuthorityConfig) -> Result<u64, SecureAuthorityLaunchError> {
    if raw.exposure != SecureNetworkExposure::Loopback
        || !raw.bind.ip().is_loopback()
        || raw.bind.port() == 0 && raw.max_ticks.is_none()
        || !raw.certificate_chain_file.is_absolute()
        || !raw.private_key_file.is_absolute()
        || !raw.oidc_jwks_file.is_absolute()
    {
        return Err(SecureAuthorityLaunchError::InvalidConfiguration);
    }
    let now = unix_seconds().map_err(|_| SecureAuthorityLaunchError::InvalidJwksLifetime)?;
    let remaining = raw
        .jwks_valid_until_unix_seconds
        .checked_sub(now)
        .ok_or(SecureAuthorityLaunchError::InvalidJwksLifetime)?;
    if !(MIN_STATIC_JWKS_VALIDITY_SECONDS..=MAX_STATIC_JWKS_VALIDITY_SECONDS).contains(&remaining) {
        return Err(SecureAuthorityLaunchError::InvalidJwksLifetime);
    }
    Ok(remaining)
}

fn parse_certificates(
    bytes: &[u8],
) -> Result<Vec<CertificateDer<'static>>, SecureAuthorityLaunchError> {
    let mut certificates = Vec::new();
    for certificate in CertificateDer::pem_slice_iter(bytes) {
        let certificate =
            certificate.map_err(|_| SecureAuthorityLaunchError::InvalidCertificateChain)?;
        if certificates.len() == MAX_CERTIFICATE_CHAIN_ENTRIES {
            return Err(SecureAuthorityLaunchError::InvalidCertificateChain);
        }
        certificates.push(certificate);
    }
    if certificates.is_empty() {
        return Err(SecureAuthorityLaunchError::InvalidCertificateChain);
    }
    Ok(certificates)
}

fn parse_private_key(bytes: &[u8]) -> Result<PrivateKeyDer<'static>, SecureAuthorityLaunchError> {
    let mut keys = PrivateKeyDer::pem_slice_iter(bytes);
    let key = keys
        .next()
        .ok_or(SecureAuthorityLaunchError::InvalidPrivateKey)?
        .map_err(|_| SecureAuthorityLaunchError::InvalidPrivateKey)?;
    if keys.next().is_some() {
        return Err(SecureAuthorityLaunchError::InvalidPrivateKey);
    }
    Ok(key)
}

fn unix_seconds() -> Result<u64, std::time::SystemTimeError> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
}

#[cfg(test)]
mod tests {
    use super::*;
    use aws_lc_rs::{
        rsa::{KeyPair as RsaKeyPair, KeySize},
        signature::KeyPair as _,
    };
    use jsonwebtoken::{
        Algorithm, DecodingKey,
        jwk::{Jwk, JwkSet, KeyAlgorithm, KeyOperations, PublicKeyUse},
    };
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_TEST_DIRECTORY: AtomicU64 = AtomicU64::new(1);

    #[test]
    fn valid_bounded_files_create_a_loopback_launch_config() {
        let fixture = Fixture::new();
        let config =
            SecureAuthorityLaunchConfig::load(&fixture.config).expect("valid launch config");

        assert!(config.bind_address().ip().is_loopback());
        assert_eq!(config.exposure(), SecureNetworkExposure::Loopback);
        assert_eq!(config.max_ticks().map(NonZeroU64::get), Some(120));
        assert_eq!(config.stop_after_commands(), None);
    }

    #[test]
    fn unknown_fields_and_remote_binds_fail_closed() {
        let fixture = Fixture::new();
        let original = fs::read_to_string(&fixture.config).expect("config text");
        let unknown = original.replacen('{', "{\"surprise\":true,", 1);
        fs::write(&fixture.config, unknown).expect("unknown-field config");
        assert!(matches!(
            SecureAuthorityLaunchConfig::load(&fixture.config),
            Err(SecureAuthorityLaunchError::InvalidConfiguration)
        ));

        fs::write(
            &fixture.config,
            original.replace("127.0.0.1:0", "0.0.0.0:40000"),
        )
        .expect("remote-bind config");
        assert!(matches!(
            SecureAuthorityLaunchConfig::load(&fixture.config),
            Err(SecureAuthorityLaunchError::InvalidConfiguration)
        ));
    }

    #[cfg(unix)]
    #[test]
    fn group_readable_private_keys_fail_closed() {
        use std::os::unix::fs::PermissionsExt;

        let fixture = Fixture::new();
        fs::set_permissions(&fixture.key, fs::Permissions::from_mode(0o640))
            .expect("unsafe key permissions");
        assert!(matches!(
            SecureAuthorityLaunchConfig::load(&fixture.config),
            Err(SecureAuthorityLaunchError::UnsafeFilePermissions(
                "TLS private key"
            ))
        ));
    }

    #[test]
    fn stale_and_excessively_long_jwks_windows_fail_closed() {
        let fixture = Fixture::new();
        let mut document: serde_json::Value =
            serde_json::from_slice(&fs::read(&fixture.config).expect("read validity fixture"))
                .expect("parse validity fixture");
        let now = unix_seconds().expect("test clock");
        for deadline in [
            now.saturating_sub(1),
            now + MAX_STATIC_JWKS_VALIDITY_SECONDS + 3_600,
        ] {
            document["jwks_valid_until_unix_seconds"] = serde_json::Value::from(deadline);
            fs::write(
                &fixture.config,
                serde_json::to_vec(&document).expect("validity config JSON"),
            )
            .expect("validity config");
            assert!(matches!(
                SecureAuthorityLaunchConfig::load(&fixture.config),
                Err(SecureAuthorityLaunchError::InvalidJwksLifetime)
            ));
        }
    }

    struct Fixture {
        directory: PathBuf,
        config: PathBuf,
        key: PathBuf,
    }

    impl Fixture {
        fn new() -> Self {
            let sequence = NEXT_TEST_DIRECTORY.fetch_add(1, Ordering::Relaxed);
            let directory = std::env::temp_dir().join(format!(
                "destructible-fps-config-{}-{sequence}",
                std::process::id()
            ));
            fs::create_dir(&directory).expect("test directory");
            let certificate = directory.join("server.pem");
            let key = directory.join("server-key.pem");
            let jwks = directory.join("jwks.json");
            let config = directory.join("server.json");
            let identity = rcgen::generate_simple_self_signed(vec!["localhost".into()])
                .expect("test TLS identity");
            fs::write(&certificate, identity.cert.pem()).expect("test certificate");
            fs::write(&key, identity.signing_key.serialize_pem()).expect("test private key");
            secure_private_key(&key);
            fs::write(&jwks, valid_jwks()).expect("test JWKS");
            let valid_until = unix_seconds().expect("test clock") + 300;
            fs::write(
                &config,
                format!(
                    "{{\"bind\":\"127.0.0.1:0\",\"exposure\":\"loopback\",\
                     \"certificate_chain_file\":{},\"private_key_file\":{},\
                     \"oidc_jwks_file\":{},\"oidc_issuer\":\"https://identity.example.test/realms/game\",\
                     \"oidc_audience\":\"destructible-fps\",\"jwks_valid_until_unix_seconds\":{valid_until},\
                     \"max_ticks\":120}}",
                    serde_json::to_string(&certificate).expect("certificate path"),
                    serde_json::to_string(&key).expect("key path"),
                    serde_json::to_string(&jwks).expect("JWKS path"),
                ),
            )
            .expect("test config");
            Self {
                directory,
                config,
                key,
            }
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.directory);
        }
    }

    fn valid_jwks() -> Vec<u8> {
        let key_pair = RsaKeyPair::generate(KeySize::Rsa2048).expect("ephemeral config RSA key");
        let decoding_key = DecodingKey::from_rsa_der(key_pair.public_key().as_ref());
        let mut jwk =
            Jwk::from_decoding_key(&decoding_key, Some(Algorithm::RS256)).expect("test public JWK");
        jwk.common.key_id = Some("config-test-key".into());
        jwk.common.key_algorithm = Some(KeyAlgorithm::RS256);
        jwk.common.public_key_use = Some(PublicKeyUse::Signature);
        jwk.common.key_operations = Some(vec![KeyOperations::Verify]);
        serde_json::to_vec(&JwkSet { keys: vec![jwk] }).expect("test JWKS")
    }

    #[cfg(unix)]
    fn secure_private_key(path: &Path) {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))
            .expect("secure key permissions");
    }

    #[cfg(not(unix))]
    fn secure_private_key(_path: &Path) {}
}
