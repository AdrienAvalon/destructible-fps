//! Read-only host-interface evidence for an already validated LAN policy.
//!
//! Enumeration never binds a socket and this module has no dependency on the authority server.

use crate::deployment_policy::{LanDeploymentPolicy, valid_private_interface_network};
use if_addrs::{IfAddr, Interface};
use std::{fmt, io, net::IpAddr};

pub const MAX_HOST_INTERFACE_RECORDS: usize = 256;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LanHostAttestation {
    interface_index: u32,
    address: IpAddr,
    prefix: u8,
    inspected_records: usize,
}

impl LanHostAttestation {
    #[must_use]
    pub const fn interface_index(&self) -> u32 {
        self.interface_index
    }

    #[must_use]
    pub const fn address(&self) -> IpAddr {
        self.address
    }

    #[must_use]
    pub const fn prefix(&self) -> u8 {
        self.prefix
    }

    #[must_use]
    pub const fn inspected_records(&self) -> usize {
        self.inspected_records
    }
}

#[derive(Debug)]
pub enum LanHostAttestationError {
    Inventory(io::Error),
    InventoryLimitExceeded,
    InterfaceMissing,
    AddressNotAssigned,
    AddressAmbiguous,
    InterfaceNotOperational,
    InterfaceIndexUnavailable,
    InvalidNetmask,
    InvalidPrivatePrefix,
}

impl fmt::Display for LanHostAttestationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Inventory(source) => {
                write!(formatter, "cannot inspect host interfaces: {source}")
            }
            Self::InventoryLimitExceeded => {
                write!(formatter, "host interface inventory exceeds limit")
            }
            Self::InterfaceMissing => write!(formatter, "declared host interface is absent"),
            Self::AddressNotAssigned => {
                write!(
                    formatter,
                    "declared address is not assigned to the interface"
                )
            }
            Self::AddressAmbiguous => write!(formatter, "declared host address is ambiguous"),
            Self::InterfaceNotOperational => {
                write!(formatter, "declared host interface is not operational")
            }
            Self::InterfaceIndexUnavailable => {
                write!(formatter, "declared host interface has no stable index")
            }
            Self::InvalidNetmask => {
                write!(formatter, "declared host interface has an invalid netmask")
            }
            Self::InvalidPrivatePrefix => {
                write!(
                    formatter,
                    "declared host address has an unsafe network prefix"
                )
            }
        }
    }
}

impl std::error::Error for LanHostAttestationError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Inventory(source) => Some(source),
            _ => None,
        }
    }
}

/// Captures one bounded interface snapshot and proves the policy's exact name/address relation.
///
/// # Errors
///
/// Fails if enumeration is unavailable or oversized, the interface/address/index is missing, the
/// address appears under another name, the interface is not operational, or its subnet escapes the
/// private address space accepted by the policy.
pub fn attest_lan_policy_on_current_host(
    policy: &LanDeploymentPolicy,
) -> Result<LanHostAttestation, LanHostAttestationError> {
    let interfaces = if_addrs::get_if_addrs().map_err(LanHostAttestationError::Inventory)?;
    if interfaces.len() > MAX_HOST_INTERFACE_RECORDS {
        return Err(LanHostAttestationError::InventoryLimitExceeded);
    }
    let records = interfaces
        .into_iter()
        .map(HostInterfaceRecord::from)
        .collect::<Vec<_>>();
    attest_records(policy, &records)
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct HostInterfaceRecord {
    name: String,
    address: IpAddr,
    prefix: u8,
    netmask: IpAddr,
    index: Option<u32>,
    operational: bool,
}

impl From<Interface> for HostInterfaceRecord {
    fn from(interface: Interface) -> Self {
        let (prefix, netmask) = match &interface.addr {
            IfAddr::V4(address) => (address.prefixlen, IpAddr::V4(address.netmask)),
            IfAddr::V6(address) => (address.prefixlen, IpAddr::V6(address.netmask)),
        };
        let address = interface.ip();
        let operational = interface.is_oper_up();
        Self {
            name: interface.name,
            address,
            prefix,
            netmask,
            index: interface.index,
            operational,
        }
    }
}

fn attest_records(
    policy: &LanDeploymentPolicy,
    records: &[HostInterfaceRecord],
) -> Result<LanHostAttestation, LanHostAttestationError> {
    if records.len() > MAX_HOST_INTERFACE_RECORDS {
        return Err(LanHostAttestationError::InventoryLimitExceeded);
    }
    if !records
        .iter()
        .any(|record| record.name == policy.interface())
    {
        return Err(LanHostAttestationError::InterfaceMissing);
    }
    if records
        .iter()
        .any(|record| record.address == policy.bind_address() && record.name != policy.interface())
    {
        return Err(LanHostAttestationError::AddressAmbiguous);
    }
    let mut exact = records.iter().filter(|record| {
        record.name == policy.interface() && record.address == policy.bind_address()
    });
    let first = exact
        .next()
        .ok_or(LanHostAttestationError::AddressNotAssigned)?;
    if !first.operational {
        return Err(LanHostAttestationError::InterfaceNotOperational);
    }
    let index = first
        .index
        .filter(|index| *index != 0)
        .ok_or(LanHostAttestationError::InterfaceIndexUnavailable)?;
    if !netmask_matches_prefix(first.netmask, first.prefix) {
        return Err(LanHostAttestationError::InvalidNetmask);
    }
    if !valid_private_interface_network(first.address, first.prefix) {
        return Err(LanHostAttestationError::InvalidPrivatePrefix);
    }
    for duplicate in exact {
        if duplicate.prefix != first.prefix
            || duplicate.netmask != first.netmask
            || duplicate.index != first.index
            || !duplicate.operational
        {
            return Err(LanHostAttestationError::AddressAmbiguous);
        }
    }
    Ok(LanHostAttestation {
        interface_index: index,
        address: first.address,
        prefix: first.prefix,
        inspected_records: records.len(),
    })
}

fn netmask_matches_prefix(netmask: IpAddr, prefix: u8) -> bool {
    match netmask {
        IpAddr::V4(netmask) if prefix <= 32 => {
            let expected = if prefix == 0 {
                0
            } else {
                u32::MAX << (32 - prefix)
            };
            u32::from(netmask) == expected
        }
        IpAddr::V6(netmask) if prefix <= 128 => {
            let expected = if prefix == 0 {
                0
            } else {
                u128::MAX << (128 - prefix)
            };
            u128::from(netmask) == expected
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: u64 = 2_000_000_000;

    fn policy() -> LanDeploymentPolicy {
        LanDeploymentPolicy::parse(
            &serde_json::to_vec(&serde_json::json!({
                "schema_version": 1,
                "deployment_id": "host-proof",
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
                "expires_at_unix_seconds": NOW + 3600
            }))
            .expect("host policy JSON"),
            NOW,
        )
        .expect("host policy")
    }

    fn record(name: &str, address: [u8; 4]) -> HostInterfaceRecord {
        HostInterfaceRecord {
            name: name.into(),
            address: address.into(),
            prefix: 24,
            netmask: IpAddr::V4([255, 255, 255, 0].into()),
            index: Some(7),
            operational: true,
        }
    }

    #[test]
    fn exact_unique_operational_assignment_is_attested() {
        let policy = policy();
        let records = [
            record("loopback", [127, 0, 0, 1]),
            record("Ethernet 2", [192, 168, 42, 20]),
        ];
        let proof = attest_records(&policy, &records).expect("exact host assignment");
        assert_eq!(proof.interface_index(), 7);
        assert_eq!(proof.address(), policy.bind_address());
        assert_eq!(proof.prefix(), 24);
        assert_eq!(proof.inspected_records(), 2);
    }

    #[test]
    fn missing_interface_or_assignment_fails_closed() {
        let policy = policy();
        assert!(matches!(
            attest_records(&policy, &[record("Wi-Fi", [192, 168, 42, 20])]),
            Err(LanHostAttestationError::InterfaceMissing)
        ));
        assert!(matches!(
            attest_records(&policy, &[record("Ethernet 2", [192, 168, 42, 21])]),
            Err(LanHostAttestationError::AddressNotAssigned)
        ));
    }

    #[test]
    fn duplicate_or_inconsistent_assignments_fail_closed() {
        let policy = policy();
        assert!(matches!(
            attest_records(
                &policy,
                &[
                    record("Ethernet 2", [192, 168, 42, 20]),
                    record("Wi-Fi", [192, 168, 42, 20]),
                ],
            ),
            Err(LanHostAttestationError::AddressAmbiguous)
        ));
        let mut inconsistent = record("Ethernet 2", [192, 168, 42, 20]);
        inconsistent.index = Some(8);
        assert!(matches!(
            attest_records(
                &policy,
                &[record("Ethernet 2", [192, 168, 42, 20]), inconsistent,],
            ),
            Err(LanHostAttestationError::AddressAmbiguous)
        ));
    }

    #[test]
    fn down_missing_index_and_unsafe_prefix_fail_closed() {
        let policy = policy();
        let mut invalid = record("Ethernet 2", [192, 168, 42, 20]);
        invalid.operational = false;
        assert!(matches!(
            attest_records(&policy, &[invalid]),
            Err(LanHostAttestationError::InterfaceNotOperational)
        ));
        let mut invalid = record("Ethernet 2", [192, 168, 42, 20]);
        invalid.index = None;
        assert!(matches!(
            attest_records(&policy, &[invalid]),
            Err(LanHostAttestationError::InterfaceIndexUnavailable)
        ));
        let mut invalid = record("Ethernet 2", [192, 168, 42, 20]);
        invalid.prefix = 7;
        invalid.netmask = IpAddr::V4([254, 0, 0, 0].into());
        assert!(matches!(
            attest_records(&policy, &[invalid]),
            Err(LanHostAttestationError::InvalidPrivatePrefix)
        ));
        let mut invalid = record("Ethernet 2", [192, 168, 42, 20]);
        invalid.netmask = IpAddr::V4([255, 0, 255, 0].into());
        assert!(matches!(
            attest_records(&policy, &[invalid]),
            Err(LanHostAttestationError::InvalidNetmask)
        ));
    }

    #[test]
    fn inventory_cardinality_is_bounded() {
        let policy = policy();
        let records =
            vec![record("Ethernet 2", [192, 168, 42, 20]); MAX_HOST_INTERFACE_RECORDS + 1];
        assert!(matches!(
            attest_records(&policy, &records),
            Err(LanHostAttestationError::InventoryLimitExceeded)
        ));
    }
}
