use destructible_fps::{
    MAX_LAN_POLICY_BYTES, MAX_PENDING_QUIC_HANDSHAKES, MAX_SECURE_GAMEPLAY_EVENTS,
    MAX_SERVER_PEERS, MAX_SESSION_DATAGRAMS_PER_SECOND,
};
use std::{
    fs,
    net::{IpAddr, Ipv4Addr, Ipv6Addr},
    path::{Path, PathBuf},
    process::Command,
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(1);

#[test]
fn checker_accepts_one_bounded_policy_without_echoing_topology() {
    let fixture = Fixture::new();
    let output = Command::new(env!("CARGO_BIN_EXE_lan-policy-check"))
        .arg(&fixture.policy)
        .output()
        .expect("run LAN policy checker");
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("UTF-8 checker output");
    assert!(stdout.starts_with("POLICY_OK schema=1 sources=1 expires_in_seconds="));
    assert!(!stdout.contains("192.168"));
    assert!(!stdout.contains("identity"));
    assert!(!stdout.contains("team:avalon"));
    assert!(output.stderr.is_empty());
}

#[test]
fn checker_requires_an_absolute_path() {
    let output = Command::new(env!("CARGO_BIN_EXE_lan-policy-check"))
        .arg("relative-policy.json")
        .output()
        .expect("run LAN policy checker with relative path");
    assert!(!output.status.success());
}

#[test]
fn checker_rejects_an_oversized_file_before_json_parsing() {
    let fixture = Fixture::new();
    fs::write(&fixture.policy, vec![b' '; MAX_LAN_POLICY_BYTES + 1])
        .expect("oversized policy fixture");
    let output = Command::new(env!("CARGO_BIN_EXE_lan-policy-check"))
        .arg(&fixture.policy)
        .output()
        .expect("run LAN policy checker with oversized file");
    assert!(!output.status.success());
}

#[test]
fn checker_attests_a_real_private_host_assignment_when_available() {
    let interfaces = if_addrs::get_if_addrs().expect("enumerate real host interfaces");
    let Some(interface) = interfaces.into_iter().find(|interface| {
        interface.is_oper_up()
            && interface.index.is_some()
            && private_network(interface.ip(), prefix(interface))
    }) else {
        eprintln!("no operational private host interface; live attestation coverage unavailable");
        return;
    };
    let fixture = Fixture::for_interface(&interface);
    let output = Command::new(env!("CARGO_BIN_EXE_lan-policy-check"))
        .arg("--verify-host")
        .arg(&fixture.policy)
        .output()
        .expect("run live host attestation");
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("UTF-8 attestation output");
    assert!(stdout.contains("host_verified=true"));
    assert!(!stdout.contains(&interface.name));
    assert!(!stdout.contains(&interface.ip().to_string()));
    assert!(output.stderr.is_empty());
    eprintln!("live private host attestation exercised");
}

struct Fixture {
    directory: PathBuf,
    policy: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        Self::from_values("enp5s0.42", "192.168.42.20".parse().expect("test IP"), 24)
    }

    fn for_interface(interface: &if_addrs::Interface) -> Self {
        Self::from_values(&interface.name, interface.ip(), prefix(interface))
    }

    fn from_values(interface: &str, address: IpAddr, prefix: u8) -> Self {
        let sequence = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let directory = std::env::temp_dir().join(format!(
            "destructible-fps-lan-policy-{}-{sequence}",
            std::process::id()
        ));
        fs::create_dir(&directory).expect("LAN policy fixture directory");
        let policy = directory.join("policy.json");
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("test wall clock")
            .as_secs();
        fs::write(
            &policy,
            serde_json::to_vec(&serde_json::json!({
                "schema_version": 1,
                "deployment_id": "avalon-lan-alpha",
                "interface": interface,
                "bind_address": address,
                "udp_port": 40001,
                "certificate_dns_name": "game.home.arpa",
                "oidc_issuer": "https://identity.home.arpa/realms/game",
                "firewall_source_cidrs": [network_cidr(address, prefix)],
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
                "expires_at_unix_seconds": now + 3600
            }))
            .expect("LAN policy fixture JSON"),
        )
        .expect("LAN policy fixture");
        Self { directory, policy }
    }
}

const fn prefix(interface: &if_addrs::Interface) -> u8 {
    match &interface.addr {
        if_addrs::IfAddr::V4(address) => address.prefixlen,
        if_addrs::IfAddr::V6(address) => address.prefixlen,
    }
}

fn private_network(address: IpAddr, prefix: u8) -> bool {
    match address {
        IpAddr::V4(address) if prefix <= 32 => {
            let raw = u32::from(address);
            let mask = if prefix == 0 {
                0
            } else {
                u32::MAX << (32 - prefix)
            };
            private_ipv4(Ipv4Addr::from(raw & mask)) && private_ipv4(Ipv4Addr::from(raw | !mask))
        }
        IpAddr::V6(address) if prefix <= 128 && address.to_ipv4_mapped().is_none() => {
            let raw = u128::from(address);
            let mask = if prefix == 0 {
                0
            } else {
                u128::MAX << (128 - prefix)
            };
            private_ipv6(Ipv6Addr::from(raw & mask)) && private_ipv6(Ipv6Addr::from(raw | !mask))
        }
        _ => false,
    }
}

fn network_cidr(address: IpAddr, prefix: u8) -> String {
    match address {
        IpAddr::V4(address) => {
            let mask = if prefix == 0 {
                0
            } else {
                u32::MAX << (32 - prefix)
            };
            format!("{}/{prefix}", Ipv4Addr::from(u32::from(address) & mask))
        }
        IpAddr::V6(address) => {
            let mask = if prefix == 0 {
                0
            } else {
                u128::MAX << (128 - prefix)
            };
            format!("{}/{prefix}", Ipv6Addr::from(u128::from(address) & mask))
        }
    }
}

const fn private_ipv4(address: Ipv4Addr) -> bool {
    let octets = address.octets();
    octets[0] == 10
        || octets[0] == 172 && octets[1] >= 16 && octets[1] <= 31
        || octets[0] == 192 && octets[1] == 168
}

const fn private_ipv6(address: Ipv6Addr) -> bool {
    address.octets()[0] & 0xfe == 0xfc
}

impl Drop for Fixture {
    fn drop(&mut self) {
        remove_fixture(&self.directory);
    }
}

fn remove_fixture(path: &Path) {
    let _ = fs::remove_dir_all(path);
}
