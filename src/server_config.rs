//! Fail-closed file configuration for the standalone secure authority.

use crate::{
    MAX_OIDC_DISCOVERY_ROOT_BYTES, MAX_OIDC_JWKS_BYTES, OidcDiscoveryClient, OidcDiscoveryError,
    OidcSessionVerifier, OidcVerificationError, SecureConfigError, SecureDedicatedServer,
    SecureTlsConfigUpdater, SessionCredentialVerifier, World, secure_server_config,
};
use core::fmt;
use quinn::{
    ServerConfig,
    rustls::pki_types::{CertificateDer, PrivateKeyDer, pem::PemObject},
};
use ring::digest::{Context, SHA256};
use serde::Deserialize;
use std::{
    fs::{self, File, Metadata},
    io::{self, Read},
    net::SocketAddr,
    num::NonZeroU64,
    path::{Path, PathBuf},
    sync::{Arc, RwLock},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use x509_parser::parse_x509_certificate;
use zeroize::Zeroizing;

pub const MAX_SECURE_CONFIG_BYTES: usize = 16 * 1_024;
pub const MAX_CERTIFICATE_CHAIN_BYTES: usize = 256 * 1_024;
pub const MAX_CERTIFICATE_CHAIN_ENTRIES: usize = 8;
pub const MAX_TLS_PRIVATE_KEY_BYTES: usize = 64 * 1_024;
pub const MIN_TLS_CERTIFICATE_REMAINING_SECONDS: u64 = 60;
pub const MIN_TLS_RELOAD_INTERVAL_SECONDS: u64 = 5;
pub const MAX_TLS_RELOAD_INTERVAL_SECONDS: u64 = 60 * 60;
pub const MIN_STATIC_JWKS_VALIDITY_SECONDS: u64 = 60;
pub const MAX_STATIC_JWKS_VALIDITY_SECONDS: u64 = 24 * 60 * 60;
const OIDC_REFRESH_GRACE_INTERVALS: u32 = 3;

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
    oidc_discovery: Option<RawOidcDiscoveryConfig>,
    #[serde(default)]
    tls_reload: Option<RawTlsReloadConfig>,
    #[serde(default)]
    max_ticks: Option<NonZeroU64>,
    #[serde(default)]
    stop_after_commands: Option<std::num::NonZeroUsize>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawOidcDiscoveryConfig {
    refresh_interval_seconds: NonZeroU64,
    #[serde(default)]
    root_certificate_file: Option<PathBuf>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawTlsReloadConfig {
    interval_seconds: NonZeroU64,
}

/// Validated, fully materialized inputs required to start one secure authority process.
///
/// The private key and bootstrap OIDC data are loaded from bounded regular files. Optional trusted
/// discovery can replace the verifier's public keys without exposing credential bytes. This
/// milestone deliberately permits only loopback endpoints.
pub struct SecureAuthorityLaunchConfig {
    bind: SocketAddr,
    exposure: SecureNetworkExposure,
    server_config: ServerConfig,
    verifier: Arc<ExpiringOidcVerifier>,
    oidc_refresh: Option<OidcRefreshController>,
    tls_refresh: Option<TlsIdentityRefreshController>,
    tls_identity_state: Arc<RwLock<TlsIdentityState>>,
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
    UnsafeFileOwnership(&'static str),
    UnsafeParentDirectory(&'static str),
    PrivilegedProcess,
    OversizedFile {
        purpose: &'static str,
        bytes: u64,
        maximum: usize,
    },
    InvalidConfiguration,
    InvalidCertificateChain,
    InvalidCertificateLifetime,
    InvalidTlsReloadInterval,
    TlsRefreshStateUnavailable,
    InvalidPrivateKey,
    InvalidJwksLifetime,
    Oidc(OidcVerificationError),
    OidcDiscovery(OidcDiscoveryError),
    OidcRefreshStateUnavailable,
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
            Self::UnsafeFileOwnership(purpose) => {
                write!(formatter, "unsafe ownership on {purpose}")
            }
            Self::UnsafeParentDirectory(purpose) => {
                write!(formatter, "unsafe parent directory for {purpose}")
            }
            Self::PrivilegedProcess => {
                write!(
                    formatter,
                    "secure authority refuses a privileged process identity"
                )
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
            Self::InvalidCertificateLifetime => {
                write!(formatter, "invalid TLS certificate validity window")
            }
            Self::InvalidTlsReloadInterval => write!(formatter, "invalid TLS reload interval"),
            Self::TlsRefreshStateUnavailable => {
                write!(formatter, "TLS refresh state unavailable")
            }
            Self::InvalidPrivateKey => write!(formatter, "invalid TLS private key"),
            Self::InvalidJwksLifetime => write!(formatter, "invalid static JWKS validity window"),
            Self::Oidc(error) => error.fmt(formatter),
            Self::OidcDiscovery(error) => error.fmt(formatter),
            Self::OidcRefreshStateUnavailable => {
                write!(formatter, "OIDC refresh state unavailable")
            }
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
            Self::OidcDiscovery(source) => Some(source),
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
        validate_runtime_identity()?;
        let config_bytes = read_bounded_file(
            path.as_ref(),
            "secure authority configuration",
            MAX_SECURE_CONFIG_BYTES,
            FilePermissionPolicy::Integrity,
        )?;
        let raw = serde_json::from_slice::<RawSecureAuthorityConfig>(&config_bytes)
            .map_err(|_| SecureAuthorityLaunchError::InvalidConfiguration)?;
        let now = unix_seconds().map_err(|_| SecureAuthorityLaunchError::InvalidJwksLifetime)?;
        let jwks_remaining_validity = validate_raw_config(&raw, now)?;
        let monotonic_now = Instant::now();
        let jwks_expiration_deadline = monotonic_now
            .checked_add(std::time::Duration::from_secs(jwks_remaining_validity))
            .ok_or(SecureAuthorityLaunchError::InvalidJwksLifetime)?;

        let tls_reload_interval = raw
            .tls_reload
            .as_ref()
            .map(|config| validate_tls_reload_interval(config.interval_seconds.get()))
            .transpose()?;
        let minimum_certificate_remaining =
            tls_reload_interval.map_or(MIN_TLS_CERTIFICATE_REMAINING_SECONDS, |interval| {
                interval
                    .as_secs()
                    .saturating_add(MIN_TLS_CERTIFICATE_REMAINING_SECONDS)
            });
        let (server_config, certificate_expiration_deadline, certificate_fingerprint) =
            load_tls_identity(
                &raw.certificate_chain_file,
                &raw.private_key_file,
                now,
                monotonic_now,
                minimum_certificate_remaining,
            )?;
        let jwks = read_bounded_file(
            &raw.oidc_jwks_file,
            "OIDC JWKS",
            MAX_OIDC_JWKS_BYTES,
            FilePermissionPolicy::Integrity,
        )?;

        let tls_identity_state = Arc::new(RwLock::new(TlsIdentityState {
            expiration_deadline: certificate_expiration_deadline,
            certificate_fingerprint,
        }));
        let tls_refresh = tls_reload_interval.map(|interval| TlsIdentityRefreshController {
            certificate_chain_file: raw.certificate_chain_file.clone(),
            private_key_file: raw.private_key_file.clone(),
            interval,
            identity_state: Arc::clone(&tls_identity_state),
            wall_clock_anchor: now,
            monotonic_clock_anchor: monotonic_now,
        });
        let oidc = OidcSessionVerifier::new(&raw.oidc_issuer, raw.oidc_audience, &jwks)
            .map_err(SecureAuthorityLaunchError::Oidc)?;
        let verifier = Arc::new(ExpiringOidcVerifier {
            inner: oidc,
            expiration_deadline: RwLock::new(jwks_expiration_deadline),
        });
        let oidc_refresh = raw
            .oidc_discovery
            .map(|config| {
                let root_bundle = config
                    .root_certificate_file
                    .as_deref()
                    .map(|path| {
                        read_bounded_file(
                            path,
                            "OIDC discovery root bundle",
                            MAX_OIDC_DISCOVERY_ROOT_BYTES,
                            FilePermissionPolicy::Integrity,
                        )
                    })
                    .transpose()?;
                let discovery = OidcDiscoveryClient::new(
                    &raw.oidc_issuer,
                    Duration::from_secs(config.refresh_interval_seconds.get()),
                    root_bundle.as_deref(),
                )
                .map_err(SecureAuthorityLaunchError::OidcDiscovery)?;
                Ok(OidcRefreshController {
                    discovery,
                    verifier: Arc::clone(&verifier),
                })
            })
            .transpose()?;

        Ok(Self {
            bind: raw.bind,
            exposure: raw.exposure,
            server_config,
            verifier,
            oidc_refresh,
            tls_refresh,
            tls_identity_state,
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
        let verifier: Arc<dyn SessionCredentialVerifier> = self.verifier;
        SecureDedicatedServer::bind_validated_loopback(
            self.bind,
            self.server_config,
            verifier,
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
    pub fn jwks_expiration_deadline(&self) -> Option<Instant> {
        self.verifier.expiration_deadline()
    }

    #[must_use]
    pub fn oidc_refresh_controller(&self) -> Option<OidcRefreshController> {
        self.oidc_refresh.clone()
    }

    #[must_use]
    pub fn tls_refresh_controller(&self) -> Option<TlsIdentityRefreshController> {
        self.tls_refresh.clone()
    }

    #[must_use]
    pub fn certificate_expiration_deadline(&self) -> Option<Instant> {
        self.tls_identity_state
            .read()
            .ok()
            .map(|state| state.expiration_deadline)
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
    expiration_deadline: RwLock<Instant>,
}

impl SessionCredentialVerifier for ExpiringOidcVerifier {
    fn verify(&self, credential: &[u8]) -> Option<crate::AuthenticatedPrincipal> {
        let keys_are_current = self
            .expiration_deadline
            .read()
            .ok()
            .is_some_and(|deadline| Instant::now() < *deadline);
        keys_are_current
            .then(|| self.inner.verify_oidc(credential).ok())
            .flatten()
    }
}

impl ExpiringOidcVerifier {
    fn expiration_deadline(&self) -> Option<Instant> {
        self.expiration_deadline
            .read()
            .ok()
            .map(|deadline| *deadline)
    }

    fn replace_jwks(
        &self,
        jwks: &[u8],
        validity: Duration,
    ) -> Result<usize, SecureAuthorityLaunchError> {
        let deadline = Instant::now()
            .checked_add(validity)
            .ok_or(SecureAuthorityLaunchError::InvalidJwksLifetime)?;
        let replacement_count = self
            .inner
            .replace_jwks(jwks)
            .map_err(SecureAuthorityLaunchError::Oidc)?;
        let mut current = self
            .expiration_deadline
            .write()
            .map_err(|_| SecureAuthorityLaunchError::OidcRefreshStateUnavailable)?;
        *current = deadline;
        drop(current);
        Ok(replacement_count)
    }
}

#[derive(Clone)]
pub struct OidcRefreshController {
    discovery: OidcDiscoveryClient,
    verifier: Arc<ExpiringOidcVerifier>,
}

impl OidcRefreshController {
    #[must_use]
    pub const fn refresh_interval(&self) -> Duration {
        self.discovery.refresh_interval()
    }

    #[must_use]
    pub fn expiration_deadline(&self) -> Option<Instant> {
        self.verifier.expiration_deadline()
    }

    /// Fetches, validates, and atomically installs a complete discovered JWKS.
    ///
    /// # Errors
    ///
    /// Discovery, key-policy, and clock failures leave the previous key set and deadline in force.
    /// A poisoned synchronization state rejects future admissions rather than extending validity.
    pub async fn refresh_once(&self) -> Result<usize, SecureAuthorityLaunchError> {
        let jwks = self
            .discovery
            .fetch_jwks()
            .await
            .map_err(SecureAuthorityLaunchError::OidcDiscovery)?;
        let validity = self
            .refresh_interval()
            .checked_mul(OIDC_REFRESH_GRACE_INTERVALS)
            .ok_or(SecureAuthorityLaunchError::InvalidJwksLifetime)?;
        self.verifier.replace_jwks(&jwks, validity)
    }
}

#[derive(Clone)]
pub struct TlsIdentityRefreshController {
    certificate_chain_file: PathBuf,
    private_key_file: PathBuf,
    interval: Duration,
    identity_state: Arc<RwLock<TlsIdentityState>>,
    wall_clock_anchor: u64,
    monotonic_clock_anchor: Instant,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TlsIdentityRefreshOutcome {
    Unchanged,
    Installed,
}

#[derive(Clone, Copy)]
struct TlsIdentityState {
    expiration_deadline: Instant,
    certificate_fingerprint: [u8; 32],
}

impl TlsIdentityRefreshController {
    #[must_use]
    pub const fn refresh_interval(&self) -> Duration {
        self.interval
    }

    #[must_use]
    pub fn expiration_deadline(&self) -> Option<Instant> {
        self.identity_state
            .read()
            .ok()
            .map(|state| state.expiration_deadline)
    }

    /// Validates the complete current file pair and installs it for future QUIC handshakes.
    ///
    /// # Errors
    ///
    /// File, permission, lifetime, key-pair, and synchronization failures retain the previous
    /// endpoint configuration and deadline.
    pub fn refresh_once(
        &self,
        updater: &SecureTlsConfigUpdater,
    ) -> Result<TlsIdentityRefreshOutcome, SecureAuthorityLaunchError> {
        let observed_now =
            unix_seconds().map_err(|_| SecureAuthorityLaunchError::InvalidCertificateLifetime)?;
        let monotonic_now = Instant::now();
        let now = monotonic_unix_seconds(
            self.wall_clock_anchor,
            self.monotonic_clock_anchor,
            observed_now,
            monotonic_now,
        )?;
        let minimum_remaining = self
            .interval
            .as_secs()
            .saturating_add(MIN_TLS_CERTIFICATE_REMAINING_SECONDS);
        let (server_config, expiration_deadline, certificate_fingerprint) = load_tls_identity(
            &self.certificate_chain_file,
            &self.private_key_file,
            now,
            monotonic_now,
            minimum_remaining,
        )?;
        let mut current = self
            .identity_state
            .write()
            .map_err(|_| SecureAuthorityLaunchError::TlsRefreshStateUnavailable)?;
        if current.certificate_fingerprint == certificate_fingerprint {
            return Ok(TlsIdentityRefreshOutcome::Unchanged);
        }
        updater.replace_for_new_connections(server_config);
        *current = TlsIdentityState {
            expiration_deadline,
            certificate_fingerprint,
        };
        drop(current);
        Ok(TlsIdentityRefreshOutcome::Installed)
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
    validate_parent_directory(path, purpose)?;
    let link_metadata = fs::symlink_metadata(path)
        .map_err(|source| SecureAuthorityLaunchError::File { purpose, source })?;
    if link_metadata.file_type().is_symlink() || !link_metadata.is_file() {
        return Err(SecureAuthorityLaunchError::InvalidFileType(purpose));
    }
    let mut file = open_no_follow(path)
        .map_err(|source| SecureAuthorityLaunchError::File { purpose, source })?;
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
fn open_no_follow(path: &Path) -> io::Result<File> {
    use rustix::fs::{Mode, OFlags, open};

    open(
        path,
        OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NOFOLLOW,
        Mode::empty(),
    )
    .map(File::from)
    .map_err(io::Error::from)
}

#[cfg(not(unix))]
fn open_no_follow(path: &Path) -> io::Result<File> {
    File::open(path)
}

#[cfg(unix)]
fn validate_permissions(
    metadata: &Metadata,
    purpose: &'static str,
    permission_policy: FilePermissionPolicy,
) -> Result<(), SecureAuthorityLaunchError> {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};

    let disallowed = match permission_policy {
        FilePermissionPolicy::Integrity => 0o022,
        FilePermissionPolicy::Private => 0o077,
    };
    if metadata.permissions().mode() & disallowed != 0 {
        return Err(SecureAuthorityLaunchError::UnsafeFilePermissions(purpose));
    }
    let effective_uid = rustix::process::geteuid().as_raw();
    if !unix_file_owner_allowed(metadata.uid(), effective_uid, permission_policy) {
        return Err(SecureAuthorityLaunchError::UnsafeFileOwnership(purpose));
    }
    Ok(())
}

#[cfg(unix)]
const fn unix_file_owner_allowed(
    owner_uid: u32,
    effective_uid: u32,
    permission_policy: FilePermissionPolicy,
) -> bool {
    match permission_policy {
        FilePermissionPolicy::Integrity => owner_uid == 0 || owner_uid == effective_uid,
        FilePermissionPolicy::Private => owner_uid == effective_uid,
    }
}

#[cfg(unix)]
fn validate_runtime_identity() -> Result<(), SecureAuthorityLaunchError> {
    if !unix_process_identity_allowed(rustix::process::geteuid().as_raw()) {
        return Err(SecureAuthorityLaunchError::PrivilegedProcess);
    }
    Ok(())
}

#[cfg(unix)]
fn validate_parent_directory(
    path: &Path,
    purpose: &'static str,
) -> Result<(), SecureAuthorityLaunchError> {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};

    let parent = path
        .parent()
        .ok_or(SecureAuthorityLaunchError::UnsafeParentDirectory(purpose))?;
    let metadata = fs::symlink_metadata(parent)
        .map_err(|_| SecureAuthorityLaunchError::UnsafeParentDirectory(purpose))?;
    let effective_uid = rustix::process::geteuid().as_raw();
    if metadata.file_type().is_symlink()
        || !metadata.is_dir()
        || metadata.permissions().mode() & 0o022 != 0
        || !unix_file_owner_allowed(
            metadata.uid(),
            effective_uid,
            FilePermissionPolicy::Integrity,
        )
    {
        return Err(SecureAuthorityLaunchError::UnsafeParentDirectory(purpose));
    }
    Ok(())
}

#[cfg(unix)]
const fn unix_process_identity_allowed(effective_uid: u32) -> bool {
    effective_uid != 0
}

#[cfg(not(unix))]
fn validate_permissions(
    _metadata: &Metadata,
    _purpose: &'static str,
    _permission_policy: FilePermissionPolicy,
) -> Result<(), SecureAuthorityLaunchError> {
    Ok(())
}

#[cfg(not(unix))]
const fn validate_runtime_identity() -> Result<(), SecureAuthorityLaunchError> {
    Ok(())
}

#[cfg(not(unix))]
const fn validate_parent_directory(
    _path: &Path,
    _purpose: &'static str,
) -> Result<(), SecureAuthorityLaunchError> {
    Ok(())
}

fn validate_raw_config(
    raw: &RawSecureAuthorityConfig,
    now: u64,
) -> Result<u64, SecureAuthorityLaunchError> {
    if raw.exposure != SecureNetworkExposure::Loopback
        || !raw.bind.ip().is_loopback()
        || raw.bind.port() == 0 && raw.max_ticks.is_none()
        || !raw.certificate_chain_file.is_absolute()
        || !raw.private_key_file.is_absolute()
        || !raw.oidc_jwks_file.is_absolute()
        || raw
            .oidc_discovery
            .as_ref()
            .and_then(|config| config.root_certificate_file.as_ref())
            .is_some_and(|path| !path.is_absolute())
    {
        return Err(SecureAuthorityLaunchError::InvalidConfiguration);
    }
    let remaining = raw
        .jwks_valid_until_unix_seconds
        .checked_sub(now)
        .ok_or(SecureAuthorityLaunchError::InvalidJwksLifetime)?;
    if !(MIN_STATIC_JWKS_VALIDITY_SECONDS..=MAX_STATIC_JWKS_VALIDITY_SECONDS).contains(&remaining) {
        return Err(SecureAuthorityLaunchError::InvalidJwksLifetime);
    }
    Ok(remaining)
}

fn validate_tls_reload_interval(seconds: u64) -> Result<Duration, SecureAuthorityLaunchError> {
    if !(MIN_TLS_RELOAD_INTERVAL_SECONDS..=MAX_TLS_RELOAD_INTERVAL_SECONDS).contains(&seconds) {
        return Err(SecureAuthorityLaunchError::InvalidTlsReloadInterval);
    }
    Ok(Duration::from_secs(seconds))
}

fn monotonic_unix_seconds(
    wall_clock_anchor: u64,
    monotonic_clock_anchor: Instant,
    observed_wall_clock: u64,
    monotonic_now: Instant,
) -> Result<u64, SecureAuthorityLaunchError> {
    let elapsed = monotonic_now
        .checked_duration_since(monotonic_clock_anchor)
        .ok_or(SecureAuthorityLaunchError::InvalidCertificateLifetime)?;
    let monotonic_floor = wall_clock_anchor
        .checked_add(elapsed.as_secs())
        .ok_or(SecureAuthorityLaunchError::InvalidCertificateLifetime)?;
    Ok(observed_wall_clock.max(monotonic_floor))
}

fn load_tls_identity(
    certificate_chain_file: &Path,
    private_key_file: &Path,
    now: u64,
    monotonic_now: Instant,
    minimum_remaining: u64,
) -> Result<(ServerConfig, Instant, [u8; 32]), SecureAuthorityLaunchError> {
    let certificate_bytes = read_bounded_file(
        certificate_chain_file,
        "TLS certificate chain",
        MAX_CERTIFICATE_CHAIN_BYTES,
        FilePermissionPolicy::Integrity,
    )?;
    let private_key_bytes = Zeroizing::new(read_bounded_file(
        private_key_file,
        "TLS private key",
        MAX_TLS_PRIVATE_KEY_BYTES,
        FilePermissionPolicy::Private,
    )?);
    let certificates = parse_certificates(&certificate_bytes)?;
    let certificate_fingerprint = certificate_chain_fingerprint(&certificates);
    let certificate_remaining_validity =
        validate_certificate_lifetimes(&certificates, now, minimum_remaining)?;
    let expiration_deadline = monotonic_now
        .checked_add(Duration::from_secs(certificate_remaining_validity))
        .ok_or(SecureAuthorityLaunchError::InvalidCertificateLifetime)?;
    let private_key = parse_private_key(&private_key_bytes)?;
    let server_config = secure_server_config(certificates, private_key)
        .map_err(SecureAuthorityLaunchError::Transport)?;
    Ok((server_config, expiration_deadline, certificate_fingerprint))
}

fn certificate_chain_fingerprint(certificates: &[CertificateDer<'_>]) -> [u8; 32] {
    let mut canonical_der = Context::new(&SHA256);
    for certificate in certificates {
        canonical_der.update(
            &u64::try_from(certificate.len())
                .unwrap_or(u64::MAX)
                .to_be_bytes(),
        );
        canonical_der.update(certificate.as_ref());
    }
    canonical_der
        .finish()
        .as_ref()
        .try_into()
        .expect("SHA-256 has a fixed 32-byte output")
}

fn validate_certificate_lifetimes(
    certificates: &[CertificateDer<'static>],
    now: u64,
    required_remaining: u64,
) -> Result<u64, SecureAuthorityLaunchError> {
    let now =
        i64::try_from(now).map_err(|_| SecureAuthorityLaunchError::InvalidCertificateLifetime)?;
    let mut minimum_remaining = u64::MAX;
    for certificate in certificates {
        let (remainder, certificate) = parse_x509_certificate(certificate.as_ref())
            .map_err(|_| SecureAuthorityLaunchError::InvalidCertificateChain)?;
        if !remainder.is_empty() {
            return Err(SecureAuthorityLaunchError::InvalidCertificateChain);
        }
        let validity = certificate.validity();
        if now < validity.not_before.timestamp() || now > validity.not_after.timestamp() {
            return Err(SecureAuthorityLaunchError::InvalidCertificateLifetime);
        }
        let remaining = u64::try_from(validity.not_after.timestamp().saturating_sub(now))
            .map_err(|_| SecureAuthorityLaunchError::InvalidCertificateLifetime)?;
        if remaining < required_remaining {
            return Err(SecureAuthorityLaunchError::InvalidCertificateLifetime);
        }
        minimum_remaining = minimum_remaining.min(remaining);
    }
    (minimum_remaining != u64::MAX)
        .then_some(minimum_remaining)
        .ok_or(SecureAuthorityLaunchError::InvalidCertificateChain)
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
        assert!(config.oidc_refresh_controller().is_none());
    }

    #[test]
    fn bounded_discovery_configuration_materializes_a_refresh_controller() {
        let fixture = Fixture::new();
        let mut document: serde_json::Value =
            serde_json::from_slice(&fs::read(&fixture.config).expect("read discovery fixture"))
                .expect("parse discovery fixture");
        document["oidc_discovery"] = serde_json::json!({
            "refresh_interval_seconds": 60,
            "root_certificate_file": fixture.certificate,
        });
        fs::write(
            &fixture.config,
            serde_json::to_vec(&document).expect("discovery config JSON"),
        )
        .expect("discovery config");

        let config = SecureAuthorityLaunchConfig::load(&fixture.config)
            .expect("valid discovery launch config");
        let refresh = config
            .oidc_refresh_controller()
            .expect("configured refresh controller");
        assert_eq!(refresh.refresh_interval(), Duration::from_mins(1));
        assert!(refresh.expiration_deadline().is_some());
    }

    #[test]
    fn bounded_tls_reload_configuration_materializes_a_refresh_controller() {
        let fixture = Fixture::new();
        let mut document: serde_json::Value =
            serde_json::from_slice(&fs::read(&fixture.config).expect("read TLS reload fixture"))
                .expect("parse TLS reload fixture");
        document["tls_reload"] = serde_json::json!({"interval_seconds": 5});
        fs::write(
            &fixture.config,
            serde_json::to_vec(&document).expect("TLS reload config JSON"),
        )
        .expect("TLS reload config");

        let config = SecureAuthorityLaunchConfig::load(&fixture.config)
            .expect("valid TLS reload launch config");
        let refresh = config
            .tls_refresh_controller()
            .expect("configured TLS refresh controller");
        assert_eq!(refresh.refresh_interval(), Duration::from_secs(5));
        assert!(refresh.expiration_deadline().is_some());
    }

    #[test]
    fn unsafe_tls_reload_interval_fails_closed() {
        let fixture = Fixture::new();
        let mut document: serde_json::Value =
            serde_json::from_slice(&fs::read(&fixture.config).expect("read TLS reload fixture"))
                .expect("parse TLS reload fixture");
        document["tls_reload"] = serde_json::json!({"interval_seconds": 4});
        fs::write(
            &fixture.config,
            serde_json::to_vec(&document).expect("short TLS reload config JSON"),
        )
        .expect("short TLS reload config");
        assert!(matches!(
            SecureAuthorityLaunchConfig::load(&fixture.config),
            Err(SecureAuthorityLaunchError::InvalidTlsReloadInterval)
        ));
    }

    #[tokio::test]
    async fn tls_reload_keeps_the_previous_deadline_until_a_valid_pair_is_installed() {
        let fixture = Fixture::new();
        let mut document: serde_json::Value =
            serde_json::from_slice(&fs::read(&fixture.config).expect("read TLS reload fixture"))
                .expect("parse TLS reload fixture");
        document["tls_reload"] = serde_json::json!({"interval_seconds": 5});
        fs::write(
            &fixture.config,
            serde_json::to_vec(&document).expect("TLS reload config JSON"),
        )
        .expect("TLS reload config");
        let launch =
            SecureAuthorityLaunchConfig::load(&fixture.config).expect("TLS reload launch config");
        let controller = launch
            .tls_refresh_controller()
            .expect("TLS refresh controller");
        let initial_deadline = controller
            .expiration_deadline()
            .expect("initial certificate deadline");
        let server = launch.start(World::default()).expect("TLS reload server");
        let updater = server.tls_config_updater();

        assert_eq!(
            controller
                .refresh_once(&updater)
                .expect("unchanged TLS identity check"),
            TlsIdentityRefreshOutcome::Unchanged
        );
        assert_eq!(controller.expiration_deadline(), Some(initial_deadline));

        let replacement_key = rcgen::KeyPair::generate().expect("replacement TLS key");
        fs::write(&fixture.key, replacement_key.serialize_pem()).expect("mismatched TLS key");
        secure_private_key(&fixture.key);
        assert!(controller.refresh_once(&updater).is_err());
        assert_eq!(controller.expiration_deadline(), Some(initial_deadline));

        let mut parameters = rcgen::CertificateParams::new(vec!["localhost".into()])
            .expect("replacement TLS parameters");
        parameters.not_after = rcgen::date_time_ymd(4090, 1, 1);
        let replacement = parameters
            .self_signed(&replacement_key)
            .expect("replacement TLS certificate");
        fs::write(&fixture.certificate, replacement.pem()).expect("replacement TLS certificate");
        assert_eq!(
            controller
                .refresh_once(&updater)
                .expect("valid TLS identity reload"),
            TlsIdentityRefreshOutcome::Installed
        );
        assert_ne!(controller.expiration_deadline(), Some(initial_deadline));
        server.shutdown().await;
    }

    #[test]
    fn unsafe_discovery_configuration_fails_closed() {
        let fixture = Fixture::new();
        let original: serde_json::Value =
            serde_json::from_slice(&fs::read(&fixture.config).expect("read discovery fixture"))
                .expect("parse discovery fixture");
        let mut document = original.clone();
        document["oidc_discovery"] = serde_json::json!({
            "refresh_interval_seconds": 59,
        });
        fs::write(
            &fixture.config,
            serde_json::to_vec(&document).expect("short-interval config JSON"),
        )
        .expect("short-interval config");
        assert!(matches!(
            SecureAuthorityLaunchConfig::load(&fixture.config),
            Err(SecureAuthorityLaunchError::OidcDiscovery(
                OidcDiscoveryError::InvalidRefreshInterval
            ))
        ));

        document = original;
        document["oidc_discovery"] = serde_json::json!({
            "refresh_interval_seconds": 60,
            "root_certificate_file": "relative-root.pem",
        });
        fs::write(
            &fixture.config,
            serde_json::to_vec(&document).expect("relative-root config JSON"),
        )
        .expect("relative-root config");
        assert!(matches!(
            SecureAuthorityLaunchConfig::load(&fixture.config),
            Err(SecureAuthorityLaunchError::InvalidConfiguration)
        ));
    }

    #[test]
    fn refresh_deadline_changes_only_after_a_valid_complete_key_set() {
        let initial_deadline = Instant::now()
            .checked_add(Duration::from_mins(1))
            .expect("initial refresh deadline");
        let verifier = ExpiringOidcVerifier {
            inner: OidcSessionVerifier::new(
                "https://identity.example.test/realms/game",
                "destructible-fps",
                &valid_jwks(),
            )
            .expect("initial refresh verifier"),
            expiration_deadline: RwLock::new(initial_deadline),
        };

        assert!(
            verifier
                .replace_jwks(br#"{"keys":[]}"#, Duration::from_mins(3))
                .is_err()
        );
        assert_eq!(verifier.expiration_deadline(), Some(initial_deadline));

        let before = Instant::now()
            .checked_add(Duration::from_mins(3))
            .expect("minimum replacement deadline");
        assert_eq!(
            verifier
                .replace_jwks(&valid_jwks(), Duration::from_mins(3))
                .expect("valid replacement JWKS"),
            1
        );
        assert!(
            verifier
                .expiration_deadline()
                .is_some_and(|deadline| deadline >= before)
        );
    }

    #[test]
    fn unknown_fields_and_every_remote_bind_class_fail_closed() {
        let fixture = Fixture::new();
        let original = fs::read_to_string(&fixture.config).expect("config text");
        let unknown = original.replacen('{', "{\"surprise\":true,", 1);
        fs::write(&fixture.config, unknown).expect("unknown-field config");
        assert!(matches!(
            SecureAuthorityLaunchConfig::load(&fixture.config),
            Err(SecureAuthorityLaunchError::InvalidConfiguration)
        ));

        for remote in [
            "0.0.0.0:40000",
            "192.168.1.50:40000",
            "[::]:40000",
            "[::ffff:127.0.0.1]:40000",
        ] {
            fs::write(&fixture.config, original.replace("127.0.0.1:0", remote))
                .expect("remote-bind config");
            assert!(matches!(
                SecureAuthorityLaunchConfig::load(&fixture.config),
                Err(SecureAuthorityLaunchError::InvalidConfiguration)
            ));
        }
    }

    #[test]
    fn lifecycle_workers_cannot_implicitly_unlock_remote_exposure() {
        let fixture = Fixture::new();
        let mut document: serde_json::Value =
            serde_json::from_slice(&fs::read(&fixture.config).expect("read remote fixture"))
                .expect("parse remote fixture");
        document["bind"] = serde_json::json!("192.168.1.50:40000");
        document["oidc_discovery"] = serde_json::json!({
            "refresh_interval_seconds": 60,
            "root_certificate_file": fixture.certificate,
        });
        document["tls_reload"] = serde_json::json!({"interval_seconds": 5});
        fs::write(
            &fixture.config,
            serde_json::to_vec(&document).expect("remote lifecycle config JSON"),
        )
        .expect("remote lifecycle config");

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

    #[cfg(unix)]
    #[test]
    fn writable_trust_parent_directory_fails_closed() {
        use std::os::unix::fs::PermissionsExt;

        let fixture = Fixture::new();
        fs::set_permissions(&fixture.directory, fs::Permissions::from_mode(0o770))
            .expect("unsafe parent permissions");
        assert!(matches!(
            SecureAuthorityLaunchConfig::load(&fixture.config),
            Err(SecureAuthorityLaunchError::UnsafeParentDirectory(
                "secure authority configuration"
            ))
        ));
    }

    #[cfg(unix)]
    #[test]
    fn kernel_no_follow_open_rejects_symbolic_links() {
        use std::os::unix::fs::symlink;

        let fixture = Fixture::new();
        let link = fixture.directory.join("server-key-link.pem");
        symlink(&fixture.key, &link).expect("private-key symlink");

        assert!(open_no_follow(&link).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn unix_service_account_policy_rejects_root_and_foreign_file_owners() {
        let service_uid = 1_000;
        assert!(!unix_process_identity_allowed(0));
        assert!(unix_process_identity_allowed(service_uid));
        assert!(unix_file_owner_allowed(
            service_uid,
            service_uid,
            FilePermissionPolicy::Private
        ));
        assert!(!unix_file_owner_allowed(
            0,
            service_uid,
            FilePermissionPolicy::Private
        ));
        assert!(!unix_file_owner_allowed(
            2_000,
            service_uid,
            FilePermissionPolicy::Private
        ));
        assert!(unix_file_owner_allowed(
            0,
            service_uid,
            FilePermissionPolicy::Integrity
        ));
        assert!(!unix_file_owner_allowed(
            2_000,
            service_uid,
            FilePermissionPolicy::Integrity
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

    #[test]
    fn expired_and_not_yet_valid_certificate_chains_fail_closed() {
        let fixture = Fixture::new();
        for (not_before, not_after) in [((2020, 1, 1), (2021, 1, 1)), ((4090, 1, 1), (4091, 1, 1))]
        {
            let signing_key = rcgen::KeyPair::generate().expect("lifetime test signing key");
            let mut parameters = rcgen::CertificateParams::new(vec!["localhost".into()])
                .expect("certificate params");
            parameters.not_before = rcgen::date_time_ymd(not_before.0, not_before.1, not_before.2);
            parameters.not_after = rcgen::date_time_ymd(not_after.0, not_after.1, not_after.2);
            let certificate = parameters
                .self_signed(&signing_key)
                .expect("lifetime test certificate");
            fs::write(&fixture.certificate, certificate.pem()).expect("replace test certificate");
            fs::write(&fixture.key, signing_key.serialize_pem()).expect("replace test key");
            secure_private_key(&fixture.key);

            assert!(matches!(
                SecureAuthorityLaunchConfig::load(&fixture.config),
                Err(SecureAuthorityLaunchError::InvalidCertificateLifetime)
            ));
        }
    }

    #[test]
    fn tls_reload_requires_one_interval_plus_the_expiry_margin() {
        let identity = rcgen::generate_simple_self_signed(vec!["localhost".into()])
            .expect("reload margin identity");
        let certificate = identity.cert.der().clone();
        let (_, parsed) =
            parse_x509_certificate(certificate.as_ref()).expect("reload margin certificate parse");
        let not_after = u64::try_from(parsed.validity().not_after.timestamp())
            .expect("positive reload margin expiry");
        let required = MAX_TLS_RELOAD_INTERVAL_SECONDS + MIN_TLS_CERTIFICATE_REMAINING_SECONDS;
        let exact_now = not_after
            .checked_sub(required)
            .expect("reload margin test clock");

        assert_eq!(
            validate_certificate_lifetimes(std::slice::from_ref(&certificate), exact_now, required)
                .expect("exact reload margin"),
            required
        );
        assert!(matches!(
            validate_certificate_lifetimes(
                std::slice::from_ref(&certificate),
                exact_now + 1,
                required,
            ),
            Err(SecureAuthorityLaunchError::InvalidCertificateLifetime)
        ));
    }

    #[test]
    fn tls_reload_clock_cannot_move_behind_its_monotonic_anchor() {
        let monotonic_anchor = Instant::now();
        let wall_anchor = 2_000_000_000_u64;
        let later = monotonic_anchor
            .checked_add(Duration::from_mins(2))
            .expect("later monotonic instant");

        assert_eq!(
            monotonic_unix_seconds(wall_anchor, monotonic_anchor, wall_anchor - 3_600, later)
                .expect("monotonic wall-clock floor"),
            wall_anchor + 120
        );
        assert_eq!(
            monotonic_unix_seconds(wall_anchor, monotonic_anchor, wall_anchor + 300, later)
                .expect("forward wall clock"),
            wall_anchor + 300
        );
    }

    struct Fixture {
        directory: PathBuf,
        config: PathBuf,
        certificate: PathBuf,
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
                certificate,
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
