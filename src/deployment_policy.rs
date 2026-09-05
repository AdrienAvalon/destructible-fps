//! Offline, fail-closed contract for a future private-LAN authority deployment.
//!
//! A valid document is evidence that the intended exposure is explicit. It is deliberately not a
//! network capability: the secure authority does not load this module and remains loopback-only.

use crate::{
    MAX_PENDING_QUIC_HANDSHAKES, MAX_SECURE_GAMEPLAY_EVENTS, MAX_SERVER_PEERS,
    MAX_SESSION_DATAGRAMS_PER_SECOND,
};
use reqwest::Url;
use serde::Deserialize;
use std::{
    fmt,
    fs::File,
    io::{self, Read},
    net::{IpAddr, Ipv4Addr, Ipv6Addr},
    num::{NonZeroU16, NonZeroU64, NonZeroUsize},
    path::Path,
};

pub const LAN_POLICY_SCHEMA_VERSION: u16 = 1;
pub const MAX_LAN_POLICY_BYTES: usize = 16 * 1_024;
pub const MAX_LAN_POLICY_LIFETIME_SECONDS: u64 = 24 * 60 * 60;
const MIN_LAN_POLICY_REMAINING_SECONDS: u64 = 60;
const MAX_DEPLOYMENT_ID_BYTES: usize = 64;
const MAX_INTERFACE_NAME_BYTES: usize = 128;
const MAX_CERTIFICATE_NAME_BYTES: usize = 253;
const MAX_ROLLBACK_OWNER_BYTES: usize = 128;
const MAX_FIREWALL_SOURCE_RANGES: usize = 16;
const MAX_STATUS_SILENCE_SECONDS: u64 = 60;
const MAX_LOG_EVENTS_PER_MINUTE: usize = 10_000;
const MAX_METRIC_SERIES: usize = 256;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawLanDeploymentPolicy {
    schema_version: u16,
    deployment_id: String,
    interface: String,
    bind_address: IpAddr,
    udp_port: NonZeroU16,
    certificate_dns_name: String,
    oidc_issuer: String,
    firewall_source_cidrs: Vec<String>,
    runtime_limits: RawRuntimeLimits,
    observability: RawObservabilityBudget,
    rollback_owner: String,
    expires_at_unix_seconds: u64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawRuntimeLimits {
    #[serde(rename = "max_players")]
    players: usize,
    #[serde(rename = "max_pending_handshakes")]
    pending_handshakes: usize,
    #[serde(rename = "max_gameplay_queue_events")]
    gameplay_queue_events: usize,
    #[serde(rename = "max_session_datagrams_per_second")]
    session_datagrams_per_second: usize,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawObservabilityBudget {
    #[serde(rename = "max_status_silence_seconds")]
    status_silence_seconds: NonZeroU64,
    #[serde(rename = "max_log_events_per_minute")]
    log_events_per_minute: NonZeroUsize,
    #[serde(rename = "max_metric_series")]
    metric_series: NonZeroUsize,
}

/// Validated declaration for one short-lived private-LAN demonstration.
///
/// This type intentionally exposes no method that binds a socket or constructs a server. A future
/// promotion must introduce a separate capability only after every remote-exposure proof passes.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LanDeploymentPolicy {
    deployment_id: String,
    interface: String,
    bind_address: IpAddr,
    udp_port: NonZeroU16,
    certificate_dns_name: String,
    oidc_issuer: String,
    firewall_source_cidrs: Vec<String>,
    expires_at_unix_seconds: u64,
}

#[derive(Debug)]
pub enum LanDeploymentPolicyError {
    File(io::Error),
    OversizedFile { bytes: u64, maximum: usize },
    InvalidDocument,
    UnsupportedSchema,
    InvalidDeploymentId,
    InvalidInterface,
    InvalidBindAddress,
    InvalidCertificateName,
    InvalidOidcIssuer,
    InvalidFirewallSources,
    RuntimeLimitMismatch,
    InvalidObservabilityBudget,
    InvalidRollbackOwner,
    InvalidExpiry,
}

impl fmt::Display for LanDeploymentPolicyError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::File(source) => write!(formatter, "cannot read LAN deployment policy: {source}"),
            Self::OversizedFile { bytes, maximum } => write!(
                formatter,
                "LAN deployment policy has {bytes} bytes, exceeding the {maximum}-byte limit"
            ),
            Self::InvalidDocument => write!(formatter, "invalid LAN deployment policy document"),
            Self::UnsupportedSchema => {
                write!(formatter, "unsupported LAN deployment policy schema")
            }
            Self::InvalidDeploymentId => write!(formatter, "invalid deployment identifier"),
            Self::InvalidInterface => write!(formatter, "invalid exact network interface"),
            Self::InvalidBindAddress => write!(formatter, "invalid private bind address or port"),
            Self::InvalidCertificateName => write!(formatter, "invalid certificate DNS name"),
            Self::InvalidOidcIssuer => write!(formatter, "invalid OIDC issuer"),
            Self::InvalidFirewallSources => write!(formatter, "invalid firewall source ranges"),
            Self::RuntimeLimitMismatch => {
                write!(formatter, "policy runtime limits differ from engine limits")
            }
            Self::InvalidObservabilityBudget => {
                write!(formatter, "invalid observability budget")
            }
            Self::InvalidRollbackOwner => write!(formatter, "invalid rollback owner"),
            Self::InvalidExpiry => write!(formatter, "invalid or stale policy expiry"),
        }
    }
}

impl std::error::Error for LanDeploymentPolicyError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::File(source) => Some(source),
            _ => None,
        }
    }
}

impl LanDeploymentPolicy {
    /// Loads one bounded policy file and validates it at the supplied wall-clock instant.
    ///
    /// # Errors
    ///
    /// Rejects unreadable or oversized files and every invalid or ambiguous policy field.
    pub fn load(
        path: impl AsRef<Path>,
        now_unix_seconds: u64,
    ) -> Result<Self, LanDeploymentPolicyError> {
        let mut file = File::open(path).map_err(LanDeploymentPolicyError::File)?;
        let metadata = file.metadata().map_err(LanDeploymentPolicyError::File)?;
        if !metadata.is_file() {
            return Err(LanDeploymentPolicyError::InvalidDocument);
        }
        if metadata.len() > u64::try_from(MAX_LAN_POLICY_BYTES).unwrap_or(u64::MAX) {
            return Err(LanDeploymentPolicyError::OversizedFile {
                bytes: metadata.len(),
                maximum: MAX_LAN_POLICY_BYTES,
            });
        }
        let mut bytes =
            Vec::with_capacity(usize::try_from(metadata.len()).unwrap_or(MAX_LAN_POLICY_BYTES));
        (&mut file)
            .take(
                u64::try_from(MAX_LAN_POLICY_BYTES)
                    .unwrap_or(u64::MAX)
                    .saturating_add(1),
            )
            .read_to_end(&mut bytes)
            .map_err(LanDeploymentPolicyError::File)?;
        if bytes.len() > MAX_LAN_POLICY_BYTES {
            return Err(LanDeploymentPolicyError::OversizedFile {
                bytes: u64::try_from(bytes.len()).unwrap_or(u64::MAX),
                maximum: MAX_LAN_POLICY_BYTES,
            });
        }
        Self::parse(&bytes, now_unix_seconds)
    }

    /// Validates one bounded in-memory JSON policy.
    ///
    /// # Errors
    ///
    /// Rejects unknown fields, public/wildcard addressing, ambiguous identifiers, unsafe identity
    /// endpoints, overlapping source ranges, limit drift, unbounded observability, and stale policy.
    pub fn parse(bytes: &[u8], now_unix_seconds: u64) -> Result<Self, LanDeploymentPolicyError> {
        if bytes.is_empty() || bytes.len() > MAX_LAN_POLICY_BYTES {
            return Err(LanDeploymentPolicyError::InvalidDocument);
        }
        let raw = serde_json::from_slice::<RawLanDeploymentPolicy>(bytes)
            .map_err(|_| LanDeploymentPolicyError::InvalidDocument)?;
        if raw.schema_version != LAN_POLICY_SCHEMA_VERSION {
            return Err(LanDeploymentPolicyError::UnsupportedSchema);
        }
        if !valid_identifier(&raw.deployment_id, MAX_DEPLOYMENT_ID_BYTES, false) {
            return Err(LanDeploymentPolicyError::InvalidDeploymentId);
        }
        if !valid_interface_name(&raw.interface) {
            return Err(LanDeploymentPolicyError::InvalidInterface);
        }
        if !is_private_lan_address(raw.bind_address) {
            return Err(LanDeploymentPolicyError::InvalidBindAddress);
        }
        if !valid_certificate_dns_name(&raw.certificate_dns_name) {
            return Err(LanDeploymentPolicyError::InvalidCertificateName);
        }
        if !valid_oidc_issuer(&raw.oidc_issuer) {
            return Err(LanDeploymentPolicyError::InvalidOidcIssuer);
        }
        validate_firewall_sources(&raw.firewall_source_cidrs, raw.bind_address)?;
        validate_runtime_limits(&raw.runtime_limits)?;
        validate_observability(&raw.observability)?;
        if !valid_identifier(&raw.rollback_owner, MAX_ROLLBACK_OWNER_BYTES, true) {
            return Err(LanDeploymentPolicyError::InvalidRollbackOwner);
        }
        validate_expiry(raw.expires_at_unix_seconds, now_unix_seconds)?;

        Ok(Self {
            deployment_id: raw.deployment_id,
            interface: raw.interface,
            bind_address: raw.bind_address,
            udp_port: raw.udp_port,
            certificate_dns_name: raw.certificate_dns_name,
            oidc_issuer: raw.oidc_issuer,
            firewall_source_cidrs: raw.firewall_source_cidrs,
            expires_at_unix_seconds: raw.expires_at_unix_seconds,
        })
    }

    #[must_use]
    pub fn deployment_id(&self) -> &str {
        &self.deployment_id
    }

    #[must_use]
    pub fn interface(&self) -> &str {
        &self.interface
    }

    #[must_use]
    pub const fn bind_address(&self) -> IpAddr {
        self.bind_address
    }

    #[must_use]
    pub const fn udp_port(&self) -> u16 {
        self.udp_port.get()
    }

    #[must_use]
    pub fn certificate_dns_name(&self) -> &str {
        &self.certificate_dns_name
    }

    #[must_use]
    pub fn oidc_issuer(&self) -> &str {
        &self.oidc_issuer
    }

    #[must_use]
    pub fn firewall_source_cidrs(&self) -> &[String] {
        &self.firewall_source_cidrs
    }

    #[must_use]
    pub const fn expires_at_unix_seconds(&self) -> u64 {
        self.expires_at_unix_seconds
    }
}

const fn validate_runtime_limits(
    limits: &RawRuntimeLimits,
) -> Result<(), LanDeploymentPolicyError> {
    if limits.players != MAX_SERVER_PEERS
        || limits.pending_handshakes != MAX_PENDING_QUIC_HANDSHAKES
        || limits.gameplay_queue_events != MAX_SECURE_GAMEPLAY_EVENTS
        || limits.session_datagrams_per_second != MAX_SESSION_DATAGRAMS_PER_SECOND
    {
        return Err(LanDeploymentPolicyError::RuntimeLimitMismatch);
    }
    Ok(())
}

const fn validate_observability(
    budget: &RawObservabilityBudget,
) -> Result<(), LanDeploymentPolicyError> {
    if budget.status_silence_seconds.get() > MAX_STATUS_SILENCE_SECONDS
        || budget.log_events_per_minute.get() > MAX_LOG_EVENTS_PER_MINUTE
        || budget.metric_series.get() > MAX_METRIC_SERIES
    {
        return Err(LanDeploymentPolicyError::InvalidObservabilityBudget);
    }
    Ok(())
}

fn validate_expiry(expires_at: u64, now: u64) -> Result<(), LanDeploymentPolicyError> {
    let remaining = expires_at
        .checked_sub(now)
        .ok_or(LanDeploymentPolicyError::InvalidExpiry)?;
    if remaining <= MIN_LAN_POLICY_REMAINING_SECONDS || remaining > MAX_LAN_POLICY_LIFETIME_SECONDS
    {
        return Err(LanDeploymentPolicyError::InvalidExpiry);
    }
    Ok(())
}

fn valid_identifier(value: &str, maximum: usize, allow_owner_punctuation: bool) -> bool {
    !value.is_empty()
        && value.len() <= maximum
        && value.is_ascii()
        && value
            .as_bytes()
            .first()
            .is_some_and(u8::is_ascii_alphanumeric)
        && value
            .as_bytes()
            .last()
            .is_some_and(u8::is_ascii_alphanumeric)
        && value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric()
                || matches!(byte, b'-' | b'_' | b'.')
                || allow_owner_punctuation && matches!(byte, b':' | b'@' | b'/')
        })
}

fn valid_interface_name(interface: &str) -> bool {
    !["any", "all", "default", "*"]
        .iter()
        .any(|wildcard| interface.eq_ignore_ascii_case(wildcard))
        && !interface.is_empty()
        && interface.len() <= MAX_INTERFACE_NAME_BYTES
        && interface.trim() == interface
        && !interface.chars().any(char::is_control)
}

pub(crate) fn valid_private_interface_network(address: IpAddr, prefix: u8) -> bool {
    match address {
        IpAddr::V4(address) if prefix <= 32 => {
            let raw = u32::from(address);
            let mask = if prefix == 0 {
                0
            } else {
                u32::MAX << (32 - prefix)
            };
            is_private_ipv4(Ipv4Addr::from(raw & mask))
                && is_private_ipv4(Ipv4Addr::from(raw | !mask))
        }
        IpAddr::V6(address) if prefix <= 128 && address.to_ipv4_mapped().is_none() => {
            let raw = u128::from(address);
            let mask = if prefix == 0 {
                0
            } else {
                u128::MAX << (128 - prefix)
            };
            is_private_ipv6(Ipv6Addr::from(raw & mask))
                && is_private_ipv6(Ipv6Addr::from(raw | !mask))
        }
        _ => false,
    }
}

fn valid_certificate_dns_name(name: &str) -> bool {
    if name.is_empty()
        || name.len() > MAX_CERTIFICATE_NAME_BYTES
        || !name.is_ascii()
        || name != name.to_ascii_lowercase()
        || name.parse::<IpAddr>().is_ok()
    {
        return false;
    }
    let mut labels = name.split('.');
    let Some(first) = labels.next() else {
        return false;
    };
    let mut count = 1_usize;
    if !valid_dns_label(first) {
        return false;
    }
    for label in labels {
        count += 1;
        if !valid_dns_label(label) {
            return false;
        }
    }
    count >= 2
}

fn valid_dns_label(label: &str) -> bool {
    (1..=63).contains(&label.len())
        && label
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        && label
            .as_bytes()
            .first()
            .is_some_and(u8::is_ascii_alphanumeric)
        && label
            .as_bytes()
            .last()
            .is_some_and(u8::is_ascii_alphanumeric)
}

fn valid_oidc_issuer(issuer: &str) -> bool {
    if !(9..=crate::MAX_OIDC_DISCOVERY_ISSUER_BYTES).contains(&issuer.len()) {
        return false;
    }
    let Ok(url) = Url::parse(issuer) else {
        return false;
    };
    url.scheme() == "https"
        && url
            .host_str()
            .is_some_and(|host| host.parse::<IpAddr>().is_err())
        && url.username().is_empty()
        && url.password().is_none()
        && url.query().is_none()
        && url.fragment().is_none()
        && url.as_str() == issuer
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PrivateCidr {
    V4 { first: u32, last: u32 },
    V6 { first: u128, last: u128 },
}

impl PrivateCidr {
    fn parse(value: &str) -> Option<Self> {
        let (address, prefix) = value.split_once('/')?;
        if prefix.contains('/') {
            return None;
        }
        let address = address.parse::<IpAddr>().ok()?;
        let prefix = prefix.parse::<u8>().ok()?;
        match address {
            IpAddr::V4(address) if prefix <= 32 => {
                let raw = u32::from(address);
                let mask = if prefix == 0 {
                    0
                } else {
                    u32::MAX << (32 - prefix)
                };
                let first = raw & mask;
                let last = first | !mask;
                (raw == first
                    && is_private_ipv4(Ipv4Addr::from(first))
                    && is_private_ipv4(Ipv4Addr::from(last)))
                .then_some(Self::V4 { first, last })
            }
            IpAddr::V6(address) if prefix <= 128 && address.to_ipv4_mapped().is_none() => {
                let raw = u128::from(address);
                let mask = if prefix == 0 {
                    0
                } else {
                    u128::MAX << (128 - prefix)
                };
                let first = raw & mask;
                let last = first | !mask;
                (raw == first
                    && is_private_ipv6(Ipv6Addr::from(first))
                    && is_private_ipv6(Ipv6Addr::from(last)))
                .then_some(Self::V6 { first, last })
            }
            _ => None,
        }
    }

    const fn same_family_as(self, address: IpAddr) -> bool {
        matches!(
            (self, address),
            (Self::V4 { .. }, IpAddr::V4(_)) | (Self::V6 { .. }, IpAddr::V6(_))
        )
    }

    const fn overlaps(self, other: Self) -> bool {
        match (self, other) {
            (
                Self::V4 {
                    first: first_a,
                    last: last_a,
                },
                Self::V4 {
                    first: first_b,
                    last: last_b,
                },
            ) => first_a <= last_b && first_b <= last_a,
            (
                Self::V6 {
                    first: first_a,
                    last: last_a,
                },
                Self::V6 {
                    first: first_b,
                    last: last_b,
                },
            ) => first_a <= last_b && first_b <= last_a,
            _ => false,
        }
    }
}

fn validate_firewall_sources(
    values: &[String],
    bind_address: IpAddr,
) -> Result<(), LanDeploymentPolicyError> {
    if values.is_empty() || values.len() > MAX_FIREWALL_SOURCE_RANGES {
        return Err(LanDeploymentPolicyError::InvalidFirewallSources);
    }
    let mut parsed = Vec::with_capacity(values.len());
    for value in values {
        let range = PrivateCidr::parse(value)
            .filter(|range| range.same_family_as(bind_address))
            .ok_or(LanDeploymentPolicyError::InvalidFirewallSources)?;
        if parsed.iter().any(|existing| range.overlaps(*existing)) {
            return Err(LanDeploymentPolicyError::InvalidFirewallSources);
        }
        parsed.push(range);
    }
    Ok(())
}

const fn is_private_lan_address(address: IpAddr) -> bool {
    match address {
        IpAddr::V4(address) => is_private_ipv4(address),
        IpAddr::V6(address) => address.to_ipv4_mapped().is_none() && is_private_ipv6(address),
    }
}

const fn is_private_ipv4(address: Ipv4Addr) -> bool {
    let octets = address.octets();
    octets[0] == 10
        || octets[0] == 172 && octets[1] >= 16 && octets[1] <= 31
        || octets[0] == 192 && octets[1] == 168
}

const fn is_private_ipv6(address: Ipv6Addr) -> bool {
    address.octets()[0] & 0xfe == 0xfc
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: u64 = 2_000_000_000;

    fn valid_policy() -> serde_json::Value {
        serde_json::json!({
            "schema_version": 1,
            "deployment_id": "avalon-lan-alpha",
            "interface": "enp5s0.42",
            "bind_address": "192.168.42.20",
            "udp_port": 40001,
            "certificate_dns_name": "game.home.arpa",
            "oidc_issuer": "https://identity.home.arpa/realms/game",
            "firewall_source_cidrs": ["192.168.42.0/24"],
            "runtime_limits": {
                "max_players": MAX_SERVER_PEERS,
                "max_pending_handshakes": MAX_PENDING_QUIC_HANDSHAKES,
                "max_gameplay_queue_events": MAX_SECURE_GAMEPLAY_EVENTS,
                "max_session_datagrams_per_second": MAX_SESSION_DATAGRAMS_PER_SECOND
            },
            "observability": {
                "max_status_silence_seconds": 15,
                "max_log_events_per_minute": 1000,
                "max_metric_series": 64
            },
            "rollback_owner": "team:avalon",
            "expires_at_unix_seconds": NOW + 3600
        })
    }

    fn parse(
        document: &serde_json::Value,
    ) -> Result<LanDeploymentPolicy, LanDeploymentPolicyError> {
        LanDeploymentPolicy::parse(&serde_json::to_vec(document).expect("policy JSON"), NOW)
    }

    #[test]
    fn exact_private_policy_is_valid_but_has_no_network_capability() {
        let policy = parse(&valid_policy()).expect("valid LAN policy");
        assert_eq!(policy.deployment_id(), "avalon-lan-alpha");
        assert_eq!(policy.interface(), "enp5s0.42");
        assert_eq!(
            policy.bind_address(),
            IpAddr::V4(Ipv4Addr::new(192, 168, 42, 20))
        );
        assert_eq!(policy.udp_port(), 40001);
        assert_eq!(policy.certificate_dns_name(), "game.home.arpa");
        assert_eq!(
            policy.oidc_issuer(),
            "https://identity.home.arpa/realms/game"
        );
        assert_eq!(policy.firewall_source_cidrs(), ["192.168.42.0/24"]);
        assert_eq!(policy.expires_at_unix_seconds(), NOW + 3600);
    }

    #[test]
    fn unknown_schema_and_fields_fail_closed() {
        let mut document = valid_policy();
        document["schema_version"] = serde_json::json!(2);
        assert!(matches!(
            parse(&document),
            Err(LanDeploymentPolicyError::UnsupportedSchema)
        ));

        let mut document = valid_policy();
        document["unlock_remote"] = serde_json::json!(true);
        assert!(matches!(
            parse(&document),
            Err(LanDeploymentPolicyError::InvalidDocument)
        ));
    }

    #[test]
    fn wildcard_public_loopback_and_mapped_bind_addresses_fail_closed() {
        for address in [
            "0.0.0.0",
            "127.0.0.1",
            "203.0.113.7",
            "::",
            "::1",
            "::ffff:192.168.42.20",
            "2001:db8::1",
        ] {
            let mut document = valid_policy();
            document["bind_address"] = serde_json::json!(address);
            assert!(matches!(
                parse(&document),
                Err(LanDeploymentPolicyError::InvalidBindAddress)
            ));
        }
    }

    #[test]
    fn ambiguous_identity_and_interface_values_fail_closed() {
        for interface in ["", "any", "ANY", " eth0", "eth0 ", "eth0\nwan"] {
            let mut document = valid_policy();
            document["interface"] = serde_json::json!(interface);
            assert!(matches!(
                parse(&document),
                Err(LanDeploymentPolicyError::InvalidInterface)
            ));
        }
        for interface in ["Ethernet 2", "Réseau privé", ".vlan.42"] {
            let mut document = valid_policy();
            document["interface"] = serde_json::json!(interface);
            assert!(parse(&document).is_ok());
        }
        for name in ["game", "*.home.arpa", "Game.home.arpa", "192.168.42.20"] {
            let mut document = valid_policy();
            document["certificate_dns_name"] = serde_json::json!(name);
            assert!(matches!(
                parse(&document),
                Err(LanDeploymentPolicyError::InvalidCertificateName)
            ));
        }
        for issuer in [
            "http://identity.home.arpa/realms/game",
            "https://user@identity.home.arpa/realms/game",
            "https://192.168.42.10/realms/game",
            "https://identity.home.arpa/realms/game?admin=true",
            "https://identity.home.arpa/realms/game#fragment",
        ] {
            let mut document = valid_policy();
            document["oidc_issuer"] = serde_json::json!(issuer);
            assert!(matches!(
                parse(&document),
                Err(LanDeploymentPolicyError::InvalidOidcIssuer)
            ));
        }
    }

    #[test]
    fn source_ranges_are_private_canonical_non_overlapping_and_same_family() {
        for sources in [
            serde_json::json!([]),
            serde_json::json!(["0.0.0.0/0"]),
            serde_json::json!(["192.168.42.1/24"]),
            serde_json::json!(["192.168.42.0/24", "192.168.42.128/25"]),
            serde_json::json!(["fc00:42::/64"]),
            serde_json::json!(["::ffff:192.168.42.0/120"]),
        ] {
            let mut document = valid_policy();
            document["firewall_source_cidrs"] = sources;
            assert!(matches!(
                parse(&document),
                Err(LanDeploymentPolicyError::InvalidFirewallSources)
            ));
        }

        let mut document = valid_policy();
        document["bind_address"] = serde_json::json!("fd42::20");
        document["firewall_source_cidrs"] = serde_json::json!(["fd42::/64"]);
        assert!(parse(&document).is_ok());
    }

    #[test]
    fn runtime_observability_and_expiry_cannot_relax_the_reviewed_envelope() {
        let mut document = valid_policy();
        document["runtime_limits"]["max_players"] = serde_json::json!(MAX_SERVER_PEERS + 1);
        assert!(matches!(
            parse(&document),
            Err(LanDeploymentPolicyError::RuntimeLimitMismatch)
        ));

        let mut document = valid_policy();
        document["observability"]["max_metric_series"] = serde_json::json!(257);
        assert!(matches!(
            parse(&document),
            Err(LanDeploymentPolicyError::InvalidObservabilityBudget)
        ));

        let mut document = valid_policy();
        document["observability"]["max_metric_series"] = serde_json::json!(0);
        assert!(matches!(
            parse(&document),
            Err(LanDeploymentPolicyError::InvalidDocument)
        ));

        let mut document = valid_policy();
        document["rollback_owner"] = serde_json::json!("team:avalon ");
        assert!(matches!(
            parse(&document),
            Err(LanDeploymentPolicyError::InvalidRollbackOwner)
        ));

        let mut document = valid_policy();
        document["runtime_limits"]["unbounded"] = serde_json::json!(true);
        assert!(matches!(
            parse(&document),
            Err(LanDeploymentPolicyError::InvalidDocument)
        ));

        for expires_at in [NOW, NOW + 60, NOW + MAX_LAN_POLICY_LIFETIME_SECONDS + 1] {
            let mut document = valid_policy();
            document["expires_at_unix_seconds"] = serde_json::json!(expires_at);
            assert!(matches!(
                parse(&document),
                Err(LanDeploymentPolicyError::InvalidExpiry)
            ));
        }
    }
}
