use destructible_fps::{
    MAX_LAN_POLICY_BYTES, MAX_PENDING_QUIC_HANDSHAKES, MAX_SECURE_GAMEPLAY_EVENTS,
    MAX_SERVER_PEERS, MAX_SESSION_DATAGRAMS_PER_SECOND,
};
use std::{
    fs,
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

struct Fixture {
    directory: PathBuf,
    policy: PathBuf,
}

impl Fixture {
    fn new() -> Self {
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
                "expires_at_unix_seconds": now + 3600
            }))
            .expect("LAN policy fixture JSON"),
        )
        .expect("LAN policy fixture");
        Self { directory, policy }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        remove_fixture(&self.directory);
    }
}

fn remove_fixture(path: &Path) {
    let _ = fs::remove_dir_all(path);
}
