//! Offline certificate evidence for a future private-LAN authority deployment.
//!
//! This module reads only public certificate material. It cannot load a private key, resolve a
//! name, bind a socket, or construct the authority server.

use crate::LanDeploymentPolicy;
use core::fmt;
use quinn::rustls::{
    RootCertStore,
    client::{WebPkiServerVerifier, danger::ServerCertVerifier},
    crypto::ring::default_provider,
    pki_types::{CertificateDer, ServerName, UnixTime, pem::PemObject},
};
use ring::digest::{Context, SHA256};
use std::{
    fs::{self, File, Metadata},
    io::{self, Read},
    path::Path,
    sync::Arc,
    time::Duration,
};
use x509_parser::{extensions::GeneralName, parse_x509_certificate};

pub const MAX_LAN_CERTIFICATE_CHAIN_BYTES: usize = 256 * 1_024;
pub const MAX_LAN_CERTIFICATE_CHAIN_ENTRIES: usize = 8;
pub const MAX_LAN_TRUST_ANCHOR_BYTES: usize = 256 * 1_024;
pub const MAX_LAN_TRUST_ANCHORS: usize = 1;
pub const LAN_CERTIFICATE_SAFETY_MARGIN_SECONDS: u64 = 60;

const CERTIFICATE_CHAIN_PURPOSE: &str = "LAN certificate chain";
const TRUST_ANCHOR_PURPOSE: &str = "LAN trust anchor";

/// Bounded evidence that one exact public certificate chain is suitable for a LAN policy window.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LanCertificateAttestation {
    certificate_entries: usize,
    trust_anchor_entries: usize,
    minimum_remaining_seconds: u64,
    certificate_chain_fingerprint: [u8; 32],
    trust_anchor_fingerprint: [u8; 32],
}

impl LanCertificateAttestation {
    #[must_use]
    pub const fn certificate_entries(&self) -> usize {
        self.certificate_entries
    }

    #[must_use]
    pub const fn trust_anchor_entries(&self) -> usize {
        self.trust_anchor_entries
    }

    #[must_use]
    pub const fn minimum_remaining_seconds(&self) -> u64 {
        self.minimum_remaining_seconds
    }

    #[must_use]
    pub const fn certificate_chain_fingerprint(&self) -> [u8; 32] {
        self.certificate_chain_fingerprint
    }

    #[must_use]
    pub const fn trust_anchor_fingerprint(&self) -> [u8; 32] {
        self.trust_anchor_fingerprint
    }
}

#[derive(Debug)]
pub enum LanCertificateAttestationError {
    InvalidAbsolutePath,
    File {
        purpose: &'static str,
        source: io::Error,
    },
    InvalidFileType(&'static str),
    UnsafeParentDirectory(&'static str),
    UnsafeFilePermissions(&'static str),
    OversizedFile {
        purpose: &'static str,
        bytes: u64,
        maximum: usize,
    },
    InvalidPem,
    CertificateLimitExceeded,
    InvalidCertificate,
    InvalidLeafIdentity,
    InvalidTrustAnchor,
    InvalidCertificateOrder,
    InvalidCertificateLifetime,
    ExpiredPolicy,
    UntrustedCertificateChain,
}

impl fmt::Display for LanCertificateAttestationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidAbsolutePath => write!(formatter, "certificate paths must be absolute"),
            Self::File { purpose, source } => write!(formatter, "cannot read {purpose}: {source}"),
            Self::InvalidFileType(purpose) => write!(formatter, "{purpose} is not a regular file"),
            Self::UnsafeParentDirectory(purpose) => {
                write!(formatter, "{purpose} has an unsafe parent directory")
            }
            Self::UnsafeFilePermissions(purpose) => {
                write!(formatter, "{purpose} has unsafe integrity permissions")
            }
            Self::OversizedFile {
                purpose,
                bytes,
                maximum,
            } => write!(
                formatter,
                "{purpose} has {bytes} bytes, exceeding the {maximum}-byte limit"
            ),
            Self::InvalidPem => write!(formatter, "invalid canonical certificate PEM"),
            Self::CertificateLimitExceeded => {
                write!(formatter, "certificate entry limit exceeded")
            }
            Self::InvalidCertificate => write!(formatter, "invalid X.509 certificate"),
            Self::InvalidLeafIdentity => write!(formatter, "invalid exact server certificate"),
            Self::InvalidTrustAnchor => write!(formatter, "invalid reviewed trust anchor"),
            Self::InvalidCertificateOrder => {
                write!(formatter, "certificate chain is not exact and ordered")
            }
            Self::InvalidCertificateLifetime => {
                write!(
                    formatter,
                    "certificate validity does not cover the policy window"
                )
            }
            Self::ExpiredPolicy => write!(formatter, "LAN policy is no longer active"),
            Self::UntrustedCertificateChain => {
                write!(
                    formatter,
                    "server certificate does not chain to the reviewed anchor"
                )
            }
        }
    }
}

impl std::error::Error for LanCertificateAttestationError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::File { source, .. } => Some(source),
            _ => None,
        }
    }
}

/// Proves that public certificate files match one already validated LAN deployment policy.
///
/// # Errors
///
/// Rejects non-absolute, mutable, non-regular or oversized inputs, non-canonical PEM, ambiguous
/// chains, non-CA anchors, unexpected leaf identities, invalid lifetimes, and untrusted chains.
pub fn attest_lan_server_certificate(
    policy: &LanDeploymentPolicy,
    certificate_chain_path: impl AsRef<Path>,
    trust_anchor_path: impl AsRef<Path>,
    now_unix_seconds: u64,
) -> Result<LanCertificateAttestation, LanCertificateAttestationError> {
    let certificate_chain_path = certificate_chain_path.as_ref();
    let trust_anchor_path = trust_anchor_path.as_ref();
    if !certificate_chain_path.is_absolute() || !trust_anchor_path.is_absolute() {
        return Err(LanCertificateAttestationError::InvalidAbsolutePath);
    }
    let certificate_chain = read_bounded_integrity_file(
        certificate_chain_path,
        CERTIFICATE_CHAIN_PURPOSE,
        MAX_LAN_CERTIFICATE_CHAIN_BYTES,
    )?;
    let trust_anchor = read_bounded_integrity_file(
        trust_anchor_path,
        TRUST_ANCHOR_PURPOSE,
        MAX_LAN_TRUST_ANCHOR_BYTES,
    )?;
    attest_certificate_material(policy, &certificate_chain, &trust_anchor, now_unix_seconds)
}

fn attest_certificate_material(
    policy: &LanDeploymentPolicy,
    certificate_chain_pem: &[u8],
    trust_anchor_pem: &[u8],
    now_unix_seconds: u64,
) -> Result<LanCertificateAttestation, LanCertificateAttestationError> {
    if policy.expires_at_unix_seconds() <= now_unix_seconds {
        return Err(LanCertificateAttestationError::ExpiredPolicy);
    }
    let required_valid_through = policy
        .expires_at_unix_seconds()
        .checked_add(LAN_CERTIFICATE_SAFETY_MARGIN_SECONDS)
        .ok_or(LanCertificateAttestationError::InvalidCertificateLifetime)?;
    let certificate_chain =
        parse_canonical_certificate_pem(certificate_chain_pem, MAX_LAN_CERTIFICATE_CHAIN_ENTRIES)?;
    let trust_anchors = parse_canonical_certificate_pem(trust_anchor_pem, MAX_LAN_TRUST_ANCHORS)?;
    let [trust_anchor] = trust_anchors.as_slice() else {
        return Err(LanCertificateAttestationError::InvalidTrustAnchor);
    };

    validate_unique_material(&certificate_chain, trust_anchor)?;
    validate_exact_leaf_identity(&certificate_chain[0], policy.certificate_dns_name())?;
    validate_exact_chain_order(&certificate_chain, trust_anchor)?;
    validate_certificate_lifetimes(
        certificate_chain
            .iter()
            .chain(std::iter::once(trust_anchor)),
        now_unix_seconds,
        required_valid_through,
    )?;
    validate_webpki_chain(
        &certificate_chain,
        trust_anchor,
        policy.certificate_dns_name(),
        now_unix_seconds,
        required_valid_through,
    )?;

    let minimum_remaining_seconds = minimum_remaining_lifetime(
        certificate_chain
            .iter()
            .chain(std::iter::once(trust_anchor)),
        now_unix_seconds,
    )?;
    Ok(LanCertificateAttestation {
        certificate_entries: certificate_chain.len(),
        trust_anchor_entries: 1,
        minimum_remaining_seconds,
        certificate_chain_fingerprint: certificate_fingerprint(&certificate_chain),
        trust_anchor_fingerprint: certificate_fingerprint(&trust_anchors),
    })
}

fn parse_canonical_certificate_pem(
    bytes: &[u8],
    maximum_entries: usize,
) -> Result<Vec<CertificateDer<'static>>, LanCertificateAttestationError> {
    let expected_entries = validate_canonical_pem_envelope(bytes, maximum_entries)?;
    let mut certificates = Vec::with_capacity(expected_entries);
    for certificate in CertificateDer::pem_slice_iter(bytes) {
        let certificate = certificate.map_err(|_| LanCertificateAttestationError::InvalidPem)?;
        if certificates.len() == maximum_entries {
            return Err(LanCertificateAttestationError::CertificateLimitExceeded);
        }
        certificates.push(certificate);
    }
    if certificates.len() != expected_entries || certificates.is_empty() {
        return Err(LanCertificateAttestationError::InvalidPem);
    }
    Ok(certificates)
}

fn validate_canonical_pem_envelope(
    bytes: &[u8],
    maximum_entries: usize,
) -> Result<usize, LanCertificateAttestationError> {
    let text =
        std::str::from_utf8(bytes).map_err(|_| LanCertificateAttestationError::InvalidPem)?;
    let mut inside_certificate = false;
    let mut encoded_lines = 0_usize;
    let mut entries = 0_usize;
    for line in text.lines() {
        if !inside_certificate {
            if line.is_empty() {
                continue;
            }
            if line != "-----BEGIN CERTIFICATE-----" {
                return Err(LanCertificateAttestationError::InvalidPem);
            }
            if entries == maximum_entries {
                return Err(LanCertificateAttestationError::CertificateLimitExceeded);
            }
            inside_certificate = true;
            encoded_lines = 0;
        } else if line == "-----END CERTIFICATE-----" {
            if encoded_lines == 0 {
                return Err(LanCertificateAttestationError::InvalidPem);
            }
            entries += 1;
            inside_certificate = false;
        } else {
            if line.is_empty()
                || !line
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'/' | b'='))
            {
                return Err(LanCertificateAttestationError::InvalidPem);
            }
            encoded_lines += 1;
        }
    }
    if inside_certificate || entries == 0 {
        return Err(LanCertificateAttestationError::InvalidPem);
    }
    Ok(entries)
}

fn validate_unique_material(
    certificate_chain: &[CertificateDer<'_>],
    trust_anchor: &CertificateDer<'_>,
) -> Result<(), LanCertificateAttestationError> {
    for (index, certificate) in certificate_chain.iter().enumerate() {
        if certificate.as_ref() == trust_anchor.as_ref()
            || certificate_chain[..index]
                .iter()
                .any(|previous| previous.as_ref() == certificate.as_ref())
        {
            return Err(LanCertificateAttestationError::InvalidCertificateOrder);
        }
    }
    Ok(())
}

fn validate_exact_leaf_identity(
    leaf: &CertificateDer<'_>,
    expected_dns_name: &str,
) -> Result<(), LanCertificateAttestationError> {
    let leaf = parse_certificate(leaf)?;
    let subject_alternative_name = leaf
        .subject_alternative_name()
        .map_err(|_| LanCertificateAttestationError::InvalidLeafIdentity)?
        .ok_or(LanCertificateAttestationError::InvalidLeafIdentity)?;
    match subject_alternative_name.value.general_names.as_slice() {
        [GeneralName::DNSName(name)] if *name == expected_dns_name => {}
        _ => return Err(LanCertificateAttestationError::InvalidLeafIdentity),
    }
    if leaf
        .basic_constraints()
        .map_err(|_| LanCertificateAttestationError::InvalidLeafIdentity)?
        .is_some_and(|constraints| constraints.value.ca)
    {
        return Err(LanCertificateAttestationError::InvalidLeafIdentity);
    }
    let extended_key_usage = leaf
        .extended_key_usage()
        .map_err(|_| LanCertificateAttestationError::InvalidLeafIdentity)?
        .ok_or(LanCertificateAttestationError::InvalidLeafIdentity)?;
    if !extended_key_usage.value.server_auth || extended_key_usage.value.any {
        return Err(LanCertificateAttestationError::InvalidLeafIdentity);
    }
    if leaf
        .key_usage()
        .map_err(|_| LanCertificateAttestationError::InvalidLeafIdentity)?
        .is_some_and(|usage| !usage.value.digital_signature())
    {
        return Err(LanCertificateAttestationError::InvalidLeafIdentity);
    }
    Ok(())
}

fn validate_exact_chain_order(
    certificate_chain: &[CertificateDer<'_>],
    trust_anchor: &CertificateDer<'_>,
) -> Result<(), LanCertificateAttestationError> {
    let root = parse_certificate(trust_anchor)?;
    validate_ca_certificate(&root, true)?;
    for pair in certificate_chain.windows(2) {
        let child_certificate = parse_certificate(&pair[0])?;
        let parent_certificate = parse_certificate(&pair[1])?;
        if child_certificate.issuer().as_raw() != parent_certificate.subject().as_raw() {
            return Err(LanCertificateAttestationError::InvalidCertificateOrder);
        }
    }
    for intermediate in &certificate_chain[1..] {
        validate_ca_certificate(&parse_certificate(intermediate)?, false)?;
    }
    let final_certificate = parse_certificate(
        certificate_chain
            .last()
            .ok_or(LanCertificateAttestationError::InvalidCertificateOrder)?,
    )?;
    if final_certificate.issuer().as_raw() != root.subject().as_raw() {
        return Err(LanCertificateAttestationError::InvalidCertificateOrder);
    }
    Ok(())
}

fn validate_ca_certificate(
    certificate: &x509_parser::certificate::X509Certificate<'_>,
    require_self_issued: bool,
) -> Result<(), LanCertificateAttestationError> {
    let constraints = certificate
        .basic_constraints()
        .map_err(|_| LanCertificateAttestationError::InvalidTrustAnchor)?
        .ok_or(LanCertificateAttestationError::InvalidTrustAnchor)?;
    let usage = certificate
        .key_usage()
        .map_err(|_| LanCertificateAttestationError::InvalidTrustAnchor)?
        .ok_or(LanCertificateAttestationError::InvalidTrustAnchor)?;
    if !constraints.value.ca
        || !usage.value.key_cert_sign()
        || require_self_issued && certificate.subject().as_raw() != certificate.issuer().as_raw()
    {
        return Err(LanCertificateAttestationError::InvalidTrustAnchor);
    }
    Ok(())
}

fn validate_certificate_lifetimes<'a>(
    certificates: impl Iterator<Item = &'a CertificateDer<'a>>,
    now_unix_seconds: u64,
    required_valid_through: u64,
) -> Result<(), LanCertificateAttestationError> {
    let now = i64::try_from(now_unix_seconds)
        .map_err(|_| LanCertificateAttestationError::InvalidCertificateLifetime)?;
    let required_valid_through = i64::try_from(required_valid_through)
        .map_err(|_| LanCertificateAttestationError::InvalidCertificateLifetime)?;
    for certificate in certificates {
        let certificate = parse_certificate(certificate)?;
        let validity = certificate.validity();
        if now < validity.not_before.timestamp()
            || now >= validity.not_after.timestamp()
            || required_valid_through >= validity.not_after.timestamp()
        {
            return Err(LanCertificateAttestationError::InvalidCertificateLifetime);
        }
    }
    Ok(())
}

fn minimum_remaining_lifetime<'a>(
    mut certificates: impl Iterator<Item = &'a CertificateDer<'a>>,
    now_unix_seconds: u64,
) -> Result<u64, LanCertificateAttestationError> {
    let now = i64::try_from(now_unix_seconds)
        .map_err(|_| LanCertificateAttestationError::InvalidCertificateLifetime)?;
    certificates.try_fold(u64::MAX, |minimum, certificate| {
        let certificate = parse_certificate(certificate)?;
        let remaining = u64::try_from(certificate.validity().not_after.timestamp() - now)
            .map_err(|_| LanCertificateAttestationError::InvalidCertificateLifetime)?;
        Ok(minimum.min(remaining))
    })
}

fn validate_webpki_chain(
    certificate_chain: &[CertificateDer<'static>],
    trust_anchor: &CertificateDer<'static>,
    dns_name: &str,
    now_unix_seconds: u64,
    required_valid_through: u64,
) -> Result<(), LanCertificateAttestationError> {
    let mut roots = RootCertStore::empty();
    roots
        .add(trust_anchor.clone())
        .map_err(|_| LanCertificateAttestationError::InvalidTrustAnchor)?;
    let verifier =
        WebPkiServerVerifier::builder_with_provider(Arc::new(roots), Arc::new(default_provider()))
            .build()
            .map_err(|_| LanCertificateAttestationError::InvalidTrustAnchor)?;
    let server_name = ServerName::try_from(dns_name.to_owned())
        .map_err(|_| LanCertificateAttestationError::InvalidLeafIdentity)?;
    let leaf = &certificate_chain[0];
    let intermediates = &certificate_chain[1..];
    for unix_seconds in [now_unix_seconds, required_valid_through] {
        verifier
            .verify_server_cert(
                leaf,
                intermediates,
                &server_name,
                &[],
                UnixTime::since_unix_epoch(Duration::from_secs(unix_seconds)),
            )
            .map_err(|_| LanCertificateAttestationError::UntrustedCertificateChain)?;
    }
    Ok(())
}

fn parse_certificate<'a>(
    certificate: &'a CertificateDer<'a>,
) -> Result<x509_parser::certificate::X509Certificate<'a>, LanCertificateAttestationError> {
    let (remainder, certificate) = parse_x509_certificate(certificate.as_ref())
        .map_err(|_| LanCertificateAttestationError::InvalidCertificate)?;
    if !remainder.is_empty() {
        return Err(LanCertificateAttestationError::InvalidCertificate);
    }
    Ok(certificate)
}

fn certificate_fingerprint(certificates: &[CertificateDer<'_>]) -> [u8; 32] {
    let mut digest = Context::new(&SHA256);
    for certificate in certificates {
        digest.update(
            &u64::try_from(certificate.len())
                .unwrap_or(u64::MAX)
                .to_be_bytes(),
        );
        digest.update(certificate.as_ref());
    }
    digest
        .finish()
        .as_ref()
        .try_into()
        .expect("SHA-256 has a fixed 32-byte output")
}

fn read_bounded_integrity_file(
    path: &Path,
    purpose: &'static str,
    maximum: usize,
) -> Result<Vec<u8>, LanCertificateAttestationError> {
    validate_parent_directory(path, purpose)?;
    let link_metadata = fs::symlink_metadata(path)
        .map_err(|source| LanCertificateAttestationError::File { purpose, source })?;
    if link_metadata.file_type().is_symlink() || !link_metadata.is_file() {
        return Err(LanCertificateAttestationError::InvalidFileType(purpose));
    }
    let mut file = open_no_follow(path)
        .map_err(|source| LanCertificateAttestationError::File { purpose, source })?;
    let metadata = file
        .metadata()
        .map_err(|source| LanCertificateAttestationError::File { purpose, source })?;
    if !metadata.is_file() {
        return Err(LanCertificateAttestationError::InvalidFileType(purpose));
    }
    validate_permissions(&metadata, purpose)?;
    if metadata.len() > u64::try_from(maximum).unwrap_or(u64::MAX) {
        return Err(LanCertificateAttestationError::OversizedFile {
            purpose,
            bytes: metadata.len(),
            maximum,
        });
    }
    let mut bytes = Vec::with_capacity(usize::try_from(metadata.len()).unwrap_or(maximum));
    (&mut file)
        .take(u64::try_from(maximum).unwrap_or(u64::MAX).saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|source| LanCertificateAttestationError::File { purpose, source })?;
    if bytes.len() > maximum {
        return Err(LanCertificateAttestationError::OversizedFile {
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
fn validate_parent_directory(
    path: &Path,
    purpose: &'static str,
) -> Result<(), LanCertificateAttestationError> {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};

    let parent = path
        .parent()
        .ok_or(LanCertificateAttestationError::UnsafeParentDirectory(
            purpose,
        ))?;
    let metadata = fs::symlink_metadata(parent)
        .map_err(|_| LanCertificateAttestationError::UnsafeParentDirectory(purpose))?;
    let effective_uid = rustix::process::geteuid().as_raw();
    if metadata.file_type().is_symlink()
        || !metadata.is_dir()
        || metadata.permissions().mode() & 0o022 != 0
        || !integrity_owner_allowed(metadata.uid(), effective_uid)
    {
        return Err(LanCertificateAttestationError::UnsafeParentDirectory(
            purpose,
        ));
    }
    Ok(())
}

#[cfg(not(unix))]
const fn validate_parent_directory(
    _path: &Path,
    _purpose: &'static str,
) -> Result<(), LanCertificateAttestationError> {
    Ok(())
}

#[cfg(unix)]
fn validate_permissions(
    metadata: &Metadata,
    purpose: &'static str,
) -> Result<(), LanCertificateAttestationError> {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};

    let effective_uid = rustix::process::geteuid().as_raw();
    if metadata.permissions().mode() & 0o022 != 0
        || !integrity_owner_allowed(metadata.uid(), effective_uid)
    {
        return Err(LanCertificateAttestationError::UnsafeFilePermissions(
            purpose,
        ));
    }
    Ok(())
}

#[cfg(unix)]
const fn integrity_owner_allowed(owner_uid: u32, effective_uid: u32) -> bool {
    owner_uid == 0 || owner_uid == effective_uid
}

#[cfg(not(unix))]
const fn validate_permissions(
    _metadata: &Metadata,
    _purpose: &'static str,
) -> Result<(), LanCertificateAttestationError> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rcgen::{
        BasicConstraints, CertifiedIssuer, ExtendedKeyUsagePurpose, IsCa, KeyPair, KeyUsagePurpose,
    };

    const NOW: u64 = 1_735_689_600;

    #[test]
    fn exact_ca_signed_leaf_covers_the_entire_policy_window() {
        let (chain, root) = certificate_material(
            &["game.home.arpa"],
            &[ExtendedKeyUsagePurpose::ServerAuth],
            ((2024, 1, 1), (2030, 1, 1)),
            ((2024, 1, 1), (2030, 1, 1)),
        );
        let proof = attest_certificate_material(&policy(NOW + 3_600), &chain, &root, NOW)
            .expect("exact certificate attestation");
        assert_eq!(proof.certificate_entries(), 1);
        assert_eq!(proof.trust_anchor_entries(), 1);
        assert!(proof.minimum_remaining_seconds() > 3_660);
        assert_ne!(proof.certificate_chain_fingerprint(), [0; 32]);
        assert_ne!(proof.trust_anchor_fingerprint(), [0; 32]);
    }

    #[test]
    fn an_ordered_intermediate_chain_is_accepted() {
        let (chain, root) = certificate_material_with_intermediate();
        let proof = attest_certificate_material(&policy(NOW + 3_600), &chain, &root, NOW)
            .expect("ordered intermediate chain");
        assert_eq!(proof.certificate_entries(), 2);
    }

    #[test]
    fn wrong_anchor_and_redundant_or_reordered_entries_fail_closed() {
        let (chain, root) = valid_certificate_material();
        let (_, wrong_root) = certificate_material(
            &["unrelated.home.arpa"],
            &[ExtendedKeyUsagePurpose::ServerAuth],
            ((2024, 1, 1), (2030, 1, 1)),
            ((2024, 1, 1), (2030, 1, 1)),
        );
        assert!(matches!(
            attest_certificate_material(&policy(NOW + 3_600), &chain, &wrong_root, NOW),
            Err(LanCertificateAttestationError::InvalidCertificateOrder
                | LanCertificateAttestationError::UntrustedCertificateChain)
        ));

        let redundant_chain = [chain.as_slice(), root.as_slice()].concat();
        assert!(matches!(
            attest_certificate_material(&policy(NOW + 3_600), &redundant_chain, &root, NOW),
            Err(LanCertificateAttestationError::InvalidCertificateOrder)
        ));
        let duplicated_chain = [chain.as_slice(), chain.as_slice()].concat();
        assert!(matches!(
            attest_certificate_material(&policy(NOW + 3_600), &duplicated_chain, &root, NOW),
            Err(LanCertificateAttestationError::InvalidCertificateOrder)
        ));

        let (intermediate_chain, root) = certificate_material_with_intermediate();
        let end_marker = b"-----END CERTIFICATE-----\n";
        let boundary = intermediate_chain
            .windows(end_marker.len())
            .position(|window| window == end_marker)
            .expect("first PEM boundary")
            + end_marker.len();
        let reordered_chain = [
            &intermediate_chain[boundary..],
            &intermediate_chain[..boundary],
        ]
        .concat();
        assert!(matches!(
            attest_certificate_material(&policy(NOW + 3_600), &reordered_chain, &root, NOW),
            Err(LanCertificateAttestationError::InvalidLeafIdentity
                | LanCertificateAttestationError::InvalidCertificateOrder)
        ));
    }

    #[test]
    fn aliases_wildcards_and_non_server_eku_fail_closed() {
        for names in [
            vec!["other.home.arpa"],
            vec!["*.home.arpa"],
            vec!["game.home.arpa", "alias.home.arpa"],
        ] {
            let (chain, root) = certificate_material(
                &names,
                &[ExtendedKeyUsagePurpose::ServerAuth],
                ((2024, 1, 1), (2030, 1, 1)),
                ((2024, 1, 1), (2030, 1, 1)),
            );
            assert!(matches!(
                attest_certificate_material(&policy(NOW + 3_600), &chain, &root, NOW),
                Err(LanCertificateAttestationError::InvalidLeafIdentity)
            ));
        }
        let (chain, root) = certificate_material(
            &["game.home.arpa"],
            &[ExtendedKeyUsagePurpose::ClientAuth],
            ((2024, 1, 1), (2030, 1, 1)),
            ((2024, 1, 1), (2030, 1, 1)),
        );
        assert!(matches!(
            attest_certificate_material(&policy(NOW + 3_600), &chain, &root, NOW),
            Err(LanCertificateAttestationError::InvalidLeafIdentity)
        ));
    }

    #[test]
    fn current_and_full_policy_horizon_lifetimes_are_mandatory() {
        for leaf_window in [
            ((2020, 1, 1), (2024, 1, 1)),
            ((2026, 1, 1), (2030, 1, 1)),
            ((2024, 1, 1), (2025, 1, 2)),
        ] {
            let (chain, root) = certificate_material(
                &["game.home.arpa"],
                &[ExtendedKeyUsagePurpose::ServerAuth],
                leaf_window,
                ((2024, 1, 1), (2030, 1, 1)),
            );
            assert!(matches!(
                attest_certificate_material(&policy(NOW + 86_340), &chain, &root, NOW),
                Err(LanCertificateAttestationError::InvalidCertificateLifetime)
            ));
        }
        let (chain, short_root) = certificate_material(
            &["game.home.arpa"],
            &[ExtendedKeyUsagePurpose::ServerAuth],
            ((2024, 1, 1), (2030, 1, 1)),
            ((2024, 1, 1), (2025, 1, 2)),
        );
        assert!(matches!(
            attest_certificate_material(&policy(NOW + 86_340), &chain, &short_root, NOW),
            Err(LanCertificateAttestationError::InvalidCertificateLifetime)
        ));
        assert!(matches!(
            attest_certificate_material(&policy(NOW + 3_600), b"invalid", b"invalid", NOW + 3_601),
            Err(LanCertificateAttestationError::ExpiredPolicy)
        ));
    }

    #[test]
    fn pem_type_cardinality_and_canonical_text_are_bounded() {
        let (chain, root) = valid_certificate_material();
        let private_key_block = b"-----BEGIN PRIVATE KEY-----\nAA==\n-----END PRIVATE KEY-----\n";
        for invalid in [private_key_block.as_slice(), b"comment\n".as_slice()] {
            assert!(matches!(
                attest_certificate_material(&policy(NOW + 3_600), invalid, &root, NOW),
                Err(LanCertificateAttestationError::InvalidPem)
            ));
        }
        let excessive_chain = chain.repeat(MAX_LAN_CERTIFICATE_CHAIN_ENTRIES + 1);
        assert!(matches!(
            attest_certificate_material(&policy(NOW + 3_600), &excessive_chain, &root, NOW),
            Err(LanCertificateAttestationError::CertificateLimitExceeded)
        ));
        let duplicated_anchor = [root.as_slice(), root.as_slice()].concat();
        assert!(matches!(
            attest_certificate_material(&policy(NOW + 3_600), &chain, &duplicated_anchor, NOW),
            Err(LanCertificateAttestationError::CertificateLimitExceeded)
        ));
    }

    #[test]
    fn lan_certificate_limits_match_the_runtime_identity_limits() {
        assert_eq!(
            MAX_LAN_CERTIFICATE_CHAIN_BYTES,
            crate::MAX_CERTIFICATE_CHAIN_BYTES
        );
        assert_eq!(
            MAX_LAN_CERTIFICATE_CHAIN_ENTRIES,
            crate::MAX_CERTIFICATE_CHAIN_ENTRIES
        );
        assert_eq!(
            LAN_CERTIFICATE_SAFETY_MARGIN_SECONDS,
            crate::MIN_TLS_CERTIFICATE_REMAINING_SECONDS
        );
    }

    fn valid_certificate_material() -> (Vec<u8>, Vec<u8>) {
        certificate_material(
            &["game.home.arpa"],
            &[ExtendedKeyUsagePurpose::ServerAuth],
            ((2024, 1, 1), (2030, 1, 1)),
            ((2024, 1, 1), (2030, 1, 1)),
        )
    }

    fn certificate_material(
        names: &[&str],
        extended_key_usages: &[ExtendedKeyUsagePurpose],
        leaf_window: ((i32, u8, u8), (i32, u8, u8)),
        root_window: ((i32, u8, u8), (i32, u8, u8)),
    ) -> (Vec<u8>, Vec<u8>) {
        let root = root_issuer(root_window);
        let key = KeyPair::generate().expect("leaf key");
        let mut parameters = rcgen::CertificateParams::new(
            names
                .iter()
                .map(|name| (*name).to_owned())
                .collect::<Vec<_>>(),
        )
        .expect("leaf parameters");
        set_window(&mut parameters, leaf_window);
        parameters.key_usages = vec![KeyUsagePurpose::DigitalSignature];
        parameters.extended_key_usages = extended_key_usages.to_vec();
        let certificate = parameters.signed_by(&key, &root).expect("signed leaf");
        (certificate.pem().into_bytes(), root.pem().into_bytes())
    }

    fn certificate_material_with_intermediate() -> (Vec<u8>, Vec<u8>) {
        let root = root_issuer(((2024, 1, 1), (2030, 1, 1)));
        let intermediate_key = KeyPair::generate().expect("intermediate key");
        let mut intermediate_parameters =
            rcgen::CertificateParams::new(Vec::<String>::new()).expect("intermediate parameters");
        set_window(&mut intermediate_parameters, ((2024, 1, 1), (2030, 1, 1)));
        intermediate_parameters.is_ca = IsCa::Ca(BasicConstraints::Constrained(0));
        intermediate_parameters.key_usages = vec![KeyUsagePurpose::KeyCertSign];
        let intermediate =
            CertifiedIssuer::signed_by(intermediate_parameters, intermediate_key, &root)
                .expect("signed intermediate");

        let leaf_key = KeyPair::generate().expect("leaf key");
        let mut leaf_parameters = rcgen::CertificateParams::new(vec!["game.home.arpa".to_owned()])
            .expect("leaf parameters");
        set_window(&mut leaf_parameters, ((2024, 1, 1), (2030, 1, 1)));
        leaf_parameters.key_usages = vec![KeyUsagePurpose::DigitalSignature];
        leaf_parameters.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
        let leaf = leaf_parameters
            .signed_by(&leaf_key, &intermediate)
            .expect("signed leaf");
        (
            [leaf.pem(), intermediate.pem()].concat().into_bytes(),
            root.pem().into_bytes(),
        )
    }

    fn root_issuer(window: ((i32, u8, u8), (i32, u8, u8))) -> CertifiedIssuer<'static, KeyPair> {
        let mut parameters =
            rcgen::CertificateParams::new(Vec::<String>::new()).expect("root parameters");
        set_window(&mut parameters, window);
        parameters.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        parameters.key_usages = vec![KeyUsagePurpose::KeyCertSign, KeyUsagePurpose::CrlSign];
        CertifiedIssuer::self_signed(parameters, KeyPair::generate().expect("root key"))
            .expect("self-signed root")
    }

    fn set_window(
        parameters: &mut rcgen::CertificateParams,
        window: ((i32, u8, u8), (i32, u8, u8)),
    ) {
        parameters.not_before = rcgen::date_time_ymd(window.0.0, window.0.1, window.0.2);
        parameters.not_after = rcgen::date_time_ymd(window.1.0, window.1.1, window.1.2);
    }

    fn policy(expires_at: u64) -> LanDeploymentPolicy {
        LanDeploymentPolicy::parse(
            &serde_json::to_vec(&serde_json::json!({
                "schema_version": 1,
                "deployment_id": "certificate-proof",
                "interface": "Ethernet 2",
                "bind_address": "192.168.42.20",
                "udp_port": 40001,
                "certificate_dns_name": "game.home.arpa",
                "oidc_issuer": "https://identity.home.arpa/realms/game",
                "firewall_source_cidrs": ["192.168.42.0/24"],
                "runtime_limits": {
                    "max_players": crate::MAX_SERVER_PEERS,
                    "max_pending_handshakes": crate::MAX_PENDING_QUIC_HANDSHAKES,
                    "max_gameplay_queue_events": crate::MAX_SECURE_GAMEPLAY_EVENTS,
                    "max_session_datagrams_per_second": crate::MAX_SESSION_DATAGRAMS_PER_SECOND
                },
                "observability": {
                    "max_status_silence_seconds": 15,
                    "max_log_events_per_minute": 1000,
                    "max_metric_series": 64
                },
                "rollback_owner": "team:avalon",
                "expires_at_unix_seconds": expires_at
            }))
            .expect("certificate policy JSON"),
            NOW,
        )
        .expect("certificate policy")
    }
}
