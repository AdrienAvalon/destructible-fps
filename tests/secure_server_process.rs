use aws_lc_rs::{
    encoding::AsDer,
    rand::SystemRandom,
    rsa::{KeyPair as RsaKeyPair, KeySize},
    signature::{KeyPair as _, RSA_PKCS1_SHA256, RsaKeyPair as SigningRsaKeyPair},
};
use base64::Engine as _;
use destructible_fps::{
    ExplosionCommand, IVec3, MAX_PENDING_QUIC_HANDSHAKES, MAX_QUIC_DATAGRAM_PAYLOAD_BYTES,
    MAX_SERVER_PEERS, MAX_SESSION_DATAGRAMS_PER_SECOND, encode_explosion_request,
    establish_session, secure_client_config, send_gameplay_datagram,
};
use jsonwebtoken::{
    Algorithm, DecodingKey, Header,
    jwk::{Jwk, JwkSet, KeyAlgorithm, KeyOperations, PublicKeyUse},
};
use quinn::{
    Endpoint,
    rustls::{RootCertStore, pki_types::CertificateDer},
};
use serde::Serialize;
use serde_json::json;
use std::{
    fs,
    io::{BufRead, BufReader, Read},
    net::{Ipv4Addr, SocketAddr, SocketAddrV4},
    path::{Path, PathBuf},
    process::{Child, ChildStdout, Command, ExitStatus, Stdio},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    time::{sleep, timeout},
};
use tokio_rustls::{
    TlsAcceptor,
    rustls::{ServerConfig, pki_types::PrivatePkcs8KeyDer},
};

const ISSUER: &str = "https://identity.example.test/realms/game";
const AUDIENCE: &str = "destructible-fps";
const KEY_ID: &str = "process-test-key";
const LOOPBACK_EPHEMERAL: SocketAddr = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0));
static NEXT_TEST_DIRECTORY: AtomicU64 = AtomicU64::new(1);

#[derive(Serialize)]
struct Claims<'a> {
    iss: &'a str,
    aud: &'a str,
    sub: &'a str,
    exp: u64,
    iat: u64,
    jti: &'a str,
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn standalone_process_accepts_oidc_and_applies_an_authoritative_command() {
    let fixture = Fixture::new(300, Some(1));
    let token = fixture.signed_token("process-positive-jti");
    let mut process = RunningServer::spawn(&fixture.config);
    let client = trusted_client(fixture.certificate.clone());
    let connection = connect(&client, process.address).await;
    let welcome = establish_session(&connection, 401, token.as_bytes())
        .await
        .expect("OIDC-authenticated process session");
    send_gameplay_datagram(
        &connection,
        encode_explosion_request(
            welcome.session_id,
            ExplosionCommand {
                command_id: 1,
                center: IVec3::new(0, 1, 0),
                radius_voxels: 4,
                peak_energy: 10_000,
            },
        ),
    )
    .expect("encrypted process command");

    let output = process.finish().await;
    assert!(output.status.success(), "process stderr: {}", output.stderr);
    assert!(output.stdout.contains("commands=1"), "{}", output.stdout);
    assert!(output.stdout.contains("admitted=1"), "{}", output.stdout);
    assert!(!output.stdout.contains(&token));
    assert!(!output.stderr.contains(&token));
    client.wait_idle().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn standalone_process_refreshes_discovered_jwks_before_ready() {
    let fixture = Fixture::new(300, Some(1));
    let discovery_identity = rcgen::generate_simple_self_signed(vec!["localhost".into()])
        .expect("process discovery TLS identity");
    let tls = ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(
            vec![discovery_identity.cert.der().clone()],
            PrivatePkcs8KeyDer::from(discovery_identity.signing_key.serialize_der()).into(),
        )
        .expect("process discovery TLS config");
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("process discovery listener");
    let port = listener
        .local_addr()
        .expect("process discovery address")
        .port();
    let issuer = format!("https://localhost:{port}/realms/game");
    let root = fixture.directory.join("discovery-root.pem");
    fs::write(&root, discovery_identity.cert.pem()).expect("process discovery root");
    fixture.configure_discovery(&issuer, &root);

    let (_, wrong_static_jwks) = oidc_material();
    fs::write(&fixture.jwks_path, wrong_static_jwks).expect("replace static process JWKS");
    let metadata = serde_json::to_vec(&json!({
        "issuer": issuer,
        "jwks_uri": format!("https://localhost:{port}/realms/game/keys"),
    }))
    .expect("process discovery metadata");
    let discovery_server = tokio::spawn(serve_https_documents(
        listener,
        Arc::new(tls),
        vec![
            (
                "/realms/game/.well-known/openid-configuration",
                "application/json",
                metadata,
            ),
            (
                "/realms/game/keys",
                "application/jwk-set+json",
                fixture.oidc_jwks.clone(),
            ),
        ],
    ));

    let token = fixture.signed_token_for_issuer("process-discovered-jti", &issuer);
    let mut process = RunningServer::spawn(&fixture.config);
    let client = trusted_client(fixture.certificate.clone());
    let connection = connect(&client, process.address).await;
    let welcome = establish_session(&connection, 407, token.as_bytes())
        .await
        .expect("discovered-key OIDC process session");
    send_gameplay_datagram(
        &connection,
        encode_explosion_request(
            welcome.session_id,
            ExplosionCommand {
                command_id: 1,
                center: IVec3::new(0, 1, 0),
                radius_voxels: 4,
                peak_energy: 10_000,
            },
        ),
    )
    .expect("encrypted discovered-key command");

    let output = process.finish().await;
    assert!(output.status.success(), "process stderr: {}", output.stderr);
    assert!(output.stdout.contains("commands=1"), "{}", output.stdout);
    assert!(output.stdout.contains("admitted=1"), "{}", output.stdout);
    assert!(
        output.stdout.contains("oidc_refresh_attempts=1"),
        "{}",
        output.stdout
    );
    assert!(
        output.stdout.contains("oidc_refresh_successes=1"),
        "{}",
        output.stdout
    );
    assert!(
        output.stdout.contains("oidc_refresh_failures=0"),
        "{}",
        output.stdout
    );
    assert!(!output.stdout.contains(&token));
    assert!(!output.stderr.contains(&token));
    discovery_server
        .await
        .expect("process discovery server task");
    client.wait_idle().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn standalone_process_refuses_mismatched_discovery_before_ready() {
    let fixture = Fixture::new(1, None);
    let discovery_identity = rcgen::generate_simple_self_signed(vec!["localhost".into()])
        .expect("mismatched discovery TLS identity");
    let tls = ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(
            vec![discovery_identity.cert.der().clone()],
            PrivatePkcs8KeyDer::from(discovery_identity.signing_key.serialize_der()).into(),
        )
        .expect("mismatched discovery TLS config");
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("mismatched discovery listener");
    let port = listener
        .local_addr()
        .expect("mismatched discovery address")
        .port();
    let issuer = format!("https://localhost:{port}/realms/game");
    let root = fixture.directory.join("mismatched-discovery-root.pem");
    fs::write(&root, discovery_identity.cert.pem()).expect("mismatched discovery root");
    fixture.configure_discovery(&issuer, &root);
    let metadata = serde_json::to_vec(&json!({
        "issuer": "https://attacker.invalid/realms/game",
        "jwks_uri": format!("https://localhost:{port}/realms/game/keys"),
    }))
    .expect("mismatched discovery metadata");
    let discovery_server = tokio::spawn(serve_https_documents(
        listener,
        Arc::new(tls),
        vec![(
            "/realms/game/.well-known/openid-configuration",
            "application/json",
            metadata,
        )],
    ));

    let output = process_command(&fixture.config)
        .output()
        .expect("mismatched discovery process");
    assert!(!output.status.success());
    assert!(!String::from_utf8_lossy(&output.stdout).contains("READY"));
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("IssuerMismatch"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!String::from_utf8_lossy(&output.stderr).contains(&issuer));
    discovery_server
        .await
        .expect("mismatched discovery server task");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn standalone_process_reloads_tls_files_without_dropping_the_active_session() {
    let fixture = Fixture::new(900, Some(1));
    fixture.configure_tls_reload(5);
    let mut process = RunningServer::spawn(&fixture.config);
    let old_client = trusted_client(fixture.certificate.clone());
    let old_connection = connect(&old_client, process.address).await;
    establish_session(
        &old_connection,
        409,
        fixture.signed_token("process-pre-rotation-jti").as_bytes(),
    )
    .await
    .expect("pre-rotation process session");

    let new_certificate = fixture.replace_tls_identity();
    sleep(Duration::from_secs(6)).await;
    assert!(old_connection.close_reason().is_none());

    let new_client = trusted_client(new_certificate);
    let new_connection = connect(&new_client, process.address).await;
    let welcome = establish_session(
        &new_connection,
        411,
        fixture.signed_token("process-post-rotation-jti").as_bytes(),
    )
    .await
    .expect("post-rotation process session");
    send_gameplay_datagram(
        &new_connection,
        encode_explosion_request(
            welcome.session_id,
            ExplosionCommand {
                command_id: 1,
                center: IVec3::new(0, 1, 0),
                radius_voxels: 4,
                peak_energy: 10_000,
            },
        ),
    )
    .expect("post-rotation encrypted command");

    let output = process.finish().await;
    assert!(output.status.success(), "process stderr: {}", output.stderr);
    assert!(output.stdout.contains("commands=1"), "{}", output.stdout);
    assert!(output.stdout.contains("admitted=2"), "{}", output.stdout);
    assert!(
        output.stdout.contains("tls_reload_attempts=1"),
        "{}",
        output.stdout
    );
    assert!(
        output.stdout.contains("tls_reload_successes=1"),
        "{}",
        output.stdout
    );
    assert!(
        output.stdout.contains("tls_reload_failures=0"),
        "{}",
        output.stdout
    );
    assert!(
        output.stdout.contains("tls_reload_installed=1"),
        "{}",
        output.stdout
    );
    assert!(
        output.stdout.contains("tls_reload_unchanged=0"),
        "{}",
        output.stdout
    );
    old_client.wait_idle().await;
    new_client.wait_idle().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn standalone_process_stops_at_the_tls_safety_deadline_during_a_reload_outage() {
    let fixture = Fixture::new(10_000, None);
    fixture.replace_with_short_lived_tls_identity(Duration::from_secs(72));
    fixture.configure_tls_reload(5);
    let mut process = RunningServer::spawn(&fixture.config);
    fs::write(&fixture.key, b"renewal provisioner unavailable")
        .expect("invalidate process renewal key");
    secure_private_key(&fixture.key);

    let started = Instant::now();
    let output = process.finish_within(Duration::from_secs(15)).await;
    let elapsed = started.elapsed();
    assert!(
        !output.status.success(),
        "process unexpectedly survived outage"
    );
    assert!(
        elapsed >= Duration::from_secs(5),
        "stopped before one reload"
    );
    assert!(
        output
            .stderr
            .contains("TLS certificate renewal safety deadline expired"),
        "{}",
        output.stderr
    );
    assert!(
        output.stderr.contains("TLS_RELOAD_FAILED failures=1"),
        "{}",
        output.stderr
    );
    let attempts = stop_counter(&output.stdout, "tls_reload_attempts");
    let failures = stop_counter(&output.stdout, "tls_reload_failures");
    assert!(attempts >= 1, "{}", output.stdout);
    assert_eq!(failures, attempts, "{}", output.stdout);
    assert_eq!(stop_counter(&output.stdout, "tls_reload_successes"), 0);
    assert_eq!(stop_counter(&output.stdout, "tls_reload_installed"), 0);
    assert_eq!(stop_counter(&output.stdout, "tls_reload_unchanged"), 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn standalone_process_stops_at_the_static_oidc_trust_safety_deadline() {
    let fixture = Fixture::new(10_000, None);
    fixture.configure_static_jwks_validity(Duration::from_secs(66));
    let mut process = RunningServer::spawn(&fixture.config);

    let started = Instant::now();
    let output = process.finish_within(Duration::from_secs(10)).await;
    let elapsed = started.elapsed();
    assert!(
        !output.status.success(),
        "process unexpectedly outlived static OIDC trust"
    );
    assert!(
        elapsed >= Duration::from_secs(4),
        "process stopped before the reserved static OIDC margin"
    );
    assert!(
        output
            .stderr
            .contains("OIDC JWKS trust safety deadline expired"),
        "{}",
        output.stderr
    );
    assert_eq!(stop_counter(&output.stdout, "oidc_refresh_attempts"), 0);
    assert_eq!(stop_counter(&output.stdout, "oidc_refresh_successes"), 0);
    assert_eq!(stop_counter(&output.stdout, "oidc_refresh_failures"), 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn standalone_process_refuses_excess_stalled_admission_without_simulation_work() {
    let fixture = Fixture::new(240, None);
    let mut process = RunningServer::spawn(&fixture.config);
    let client = trusted_client(fixture.certificate.clone());

    let mut handshakes = tokio::task::JoinSet::new();
    for _ in 0..MAX_PENDING_QUIC_HANDSHAKES {
        let connecting = client
            .connect(process.address, "localhost")
            .expect("start external stalled-admission connection");
        handshakes.spawn(async move {
            timeout(Duration::from_secs(2), connecting)
                .await
                .expect("external stalled-admission TLS deadline")
                .expect("trusted external stalled-admission connection")
        });
    }
    let mut stalled_connections = Vec::with_capacity(MAX_PENDING_QUIC_HANDSHAKES);
    while let Some(result) = handshakes.join_next().await {
        stalled_connections.push(result.expect("external stalled-admission handshake task"));
    }
    assert_eq!(stalled_connections.len(), MAX_PENDING_QUIC_HANDSHAKES);

    let excess = timeout(
        Duration::from_secs(2),
        client
            .connect(process.address, "localhost")
            .expect("start external excess connection"),
    )
    .await
    .expect("external excess refusal deadline");
    assert!(excess.is_err(), "external excess admission connected");
    for connection in stalled_connections {
        connection.close(0_u32.into(), b"external hostile-load test complete");
    }

    let output = process.finish_within(Duration::from_secs(8)).await;
    assert!(output.status.success(), "process stderr: {}", output.stderr);
    assert_eq!(stop_counter(&output.stdout, "ticks"), 240);
    assert_eq!(stop_counter(&output.stdout, "refused"), 1);
    assert_eq!(stop_counter(&output.stdout, "handshake_failures"), 0);
    assert_eq!(stop_counter(&output.stdout, "admitted"), 0);
    assert_eq!(stop_counter(&output.stdout, "commands"), 0);
    assert_eq!(stop_counter(&output.stdout, "inbound"), 0);
    assert_eq!(stop_counter(&output.stdout, "outbound"), 0);
    client.wait_idle().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn standalone_process_closes_an_external_oversized_datagram_before_simulation_work() {
    let fixture = Fixture::new(180, None);
    let token = fixture.signed_token("external-oversized-datagram-jti");
    let mut process = RunningServer::spawn(&fixture.config);
    let client = trusted_client(fixture.certificate.clone());
    let connection = connect(&client, process.address).await;
    establish_session(&connection, 419, token.as_bytes())
        .await
        .expect("external oversized-datagram session");
    connection
        .send_datagram(vec![0_u8; MAX_QUIC_DATAGRAM_PAYLOAD_BYTES + 1].into())
        .expect("send external oversized datagram inside QUIC path MTU");
    timeout(Duration::from_secs(2), connection.closed())
        .await
        .expect("external oversized-datagram rejection deadline");

    let output = process.finish_within(Duration::from_secs(6)).await;
    assert!(output.status.success(), "process stderr: {}", output.stderr);
    assert_eq!(stop_counter(&output.stdout, "protocol_rejections"), 1);
    assert_eq!(stop_counter(&output.stdout, "admitted"), 1);
    assert_eq!(stop_counter(&output.stdout, "commands"), 0);
    assert_eq!(stop_counter(&output.stdout, "inbound"), 0);
    assert!(!output.stdout.contains(&token));
    assert!(!output.stderr.contains(&token));
    client.wait_idle().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn standalone_process_bounds_external_multi_session_gameplay_queue_pressure() {
    let fixture = Fixture::new(360, None);
    let mut process = RunningServer::spawn(&fixture.config);
    let client = trusted_client(fixture.certificate.clone());
    let mut connections = Vec::with_capacity(MAX_SERVER_PEERS);
    for client_index in 0..MAX_SERVER_PEERS {
        let connection = connect(&client, process.address).await;
        let token = fixture.signed_token(&format!("queue-pressure-{client_index}"));
        establish_session(
            &connection,
            500_u64 + u64::try_from(client_index).expect("bounded client index"),
            token.as_bytes(),
        )
        .await
        .expect("external queue-pressure session");
        connections.push(connection);
    }

    let mut senders = tokio::task::JoinSet::new();
    for connection in &connections {
        let connection = connection.clone();
        senders.spawn(async move {
            let mut sent = 0_usize;
            for _ in 0..MAX_SESSION_DATAGRAMS_PER_SECOND {
                if connection
                    .send_datagram_wait(vec![0xff_u8; MAX_QUIC_DATAGRAM_PAYLOAD_BYTES].into())
                    .await
                    .is_err()
                {
                    break;
                }
                sent += 1;
            }
            sent
        });
    }
    let mut sent = 0_usize;
    while !senders.is_empty() {
        sent += timeout(Duration::from_secs(3), senders.join_next())
            .await
            .expect("external queue-pressure sender deadline")
            .expect("external queue-pressure sender exists")
            .expect("external queue-pressure sender task");
    }
    assert!(sent > 256, "insufficient offered queue pressure: {sent}");

    let mut pressure_closed = 0_usize;
    for _ in 0..100 {
        pressure_closed = connections
            .iter()
            .filter(|connection| connection.close_reason().is_some())
            .count();
        if pressure_closed > 0 {
            break;
        }
        sleep(Duration::from_millis(10)).await;
    }
    assert!(pressure_closed > 0, "queue pressure closed no session");
    for connection in &connections {
        connection.close(0_u32.into(), b"external queue-pressure test complete");
    }

    let output = process.finish_within(Duration::from_secs(8)).await;
    assert!(output.status.success(), "process stderr: {}", output.stderr);
    assert_eq!(
        stop_counter(&output.stdout, "admitted"),
        MAX_SERVER_PEERS as u64
    );
    assert!(stop_counter(&output.stdout, "gameplay_queue_drops") >= 32);
    let malformed = stop_counter(&output.stdout, "malformed");
    let rejected = stop_counter(&output.stdout, "rejected_session_datagrams");
    assert!(malformed.saturating_add(rejected) > 0);
    assert_eq!(stop_counter(&output.stdout, "inbound"), malformed);
    assert_eq!(stop_counter(&output.stdout, "commands"), 0);
    assert_eq!(stop_counter(&output.stdout, "rate_limited"), 0);
    client.wait_idle().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn standalone_process_survives_repeated_authenticated_reconnect_cycles() {
    const RECONNECT_CYCLES: usize = 32;

    let fixture = Fixture::new(480, None);
    let mut process = RunningServer::spawn(&fixture.config);
    let client = trusted_client(fixture.certificate.clone());
    for cycle in 0..RECONNECT_CYCLES {
        let connection = connect(&client, process.address).await;
        let token = fixture.signed_token(&format!("reconnect-cycle-{cycle}"));
        establish_session(
            &connection,
            800_u64 + u64::try_from(cycle).expect("bounded reconnect cycle"),
            token.as_bytes(),
        )
        .await
        .expect("authenticated reconnect cycle");
        sleep(Duration::from_millis(25)).await;
        connection.close(0_u32.into(), b"reconnect cycle complete");
        sleep(Duration::from_millis(25)).await;
    }

    let output = process.finish_within(Duration::from_secs(10)).await;
    assert!(output.status.success(), "process stderr: {}", output.stderr);
    assert_eq!(
        stop_counter(&output.stdout, "admitted"),
        RECONNECT_CYCLES as u64
    );
    assert_eq!(
        stop_counter(&output.stdout, "disconnected"),
        RECONNECT_CYCLES as u64
    );
    assert_eq!(stop_counter(&output.stdout, "active"), 0);
    assert_eq!(stop_counter(&output.stdout, "refused"), 0);
    assert_eq!(stop_counter(&output.stdout, "handshake_failures"), 0);
    assert_eq!(stop_counter(&output.stdout, "admission_failures"), 0);
    assert_eq!(stop_counter(&output.stdout, "commands"), 0);
    client.wait_idle().await;
}

#[test]
fn standalone_process_distinguishes_an_unchanged_tls_check_from_a_rotation() {
    let fixture = Fixture::new(420, None);
    fixture.configure_tls_reload(5);

    let output = process_command(&fixture.config)
        .output()
        .expect("unchanged TLS process");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(stdout.contains("tls_reload_attempts=1"), "{stdout}");
    assert!(stdout.contains("tls_reload_successes=1"), "{stdout}");
    assert!(stdout.contains("tls_reload_failures=0"), "{stdout}");
    assert!(stdout.contains("tls_reload_installed=0"), "{stdout}");
    assert!(stdout.contains("tls_reload_unchanged=1"), "{stdout}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn standalone_process_rejects_an_invalid_oidc_credential_without_simulation_work() {
    let fixture = Fixture::new(60, None);
    let rejected = "this-is-not-a-signed-access-token";
    let mut process = RunningServer::spawn(&fixture.config);
    let client = trusted_client(fixture.certificate.clone());
    let connection = connect(&client, process.address).await;
    assert!(
        establish_session(&connection, 403, rejected.as_bytes())
            .await
            .is_err()
    );

    let output = process.finish().await;
    assert!(output.status.success(), "process stderr: {}", output.stderr);
    assert!(
        output.stdout.contains("admission_failures=1"),
        "{}",
        output.stdout
    );
    assert!(output.stdout.contains("commands=0"), "{}", output.stdout);
    assert!(!output.stdout.contains(rejected));
    assert!(!output.stderr.contains(rejected));
    client.wait_idle().await;
}

#[test]
fn standalone_process_refuses_remote_configuration_before_ready() {
    let fixture = Fixture::new(1, None);
    let mut document = fixture.read_config();
    document["bind"] = json!("0.0.0.0:40000");
    fixture.write_config(&document);

    let output = process_command(&fixture.config)
        .output()
        .expect("rejected remote process");
    assert!(!output.status.success());
    assert!(!String::from_utf8_lossy(&output.stdout).contains("READY"));
}

#[test]
fn standalone_process_refuses_expired_certificate_before_ready() {
    let fixture = Fixture::new(1, None);
    fixture.replace_with_expired_tls_identity();

    let output = process_command(&fixture.config)
        .output()
        .expect("rejected expired-certificate process");
    assert!(!output.status.success());
    assert!(!String::from_utf8_lossy(&output.stdout).contains("READY"));
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("InvalidCertificateLifetime"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[cfg(unix)]
#[test]
fn standalone_process_refuses_group_readable_private_key_before_ready() {
    use std::os::unix::fs::PermissionsExt;

    let fixture = Fixture::new(1, None);
    fs::set_permissions(&fixture.key, fs::Permissions::from_mode(0o640))
        .expect("unsafe process key permissions");

    let output = process_command(&fixture.config)
        .output()
        .expect("rejected unsafe-key process");
    assert!(!output.status.success());
    assert!(!String::from_utf8_lossy(&output.stdout).contains("READY"));
}

async fn connect(client: &Endpoint, address: SocketAddr) -> quinn::Connection {
    timeout(
        Duration::from_secs(2),
        client
            .connect(address, "localhost")
            .expect("start process connection"),
    )
    .await
    .expect("process TLS deadline")
    .expect("trusted process TLS identity")
}

fn trusted_client(certificate: CertificateDer<'static>) -> Endpoint {
    let mut roots = RootCertStore::empty();
    roots.add(certificate).expect("trusted process certificate");
    let config = secure_client_config(roots).expect("process client config");
    let mut endpoint = Endpoint::client(LOOPBACK_EPHEMERAL).expect("process client endpoint");
    endpoint.set_default_client_config(config);
    endpoint
}

struct ProcessOutput {
    status: ExitStatus,
    stdout: String,
    stderr: String,
}

struct RunningServer {
    child: Child,
    stdout: BufReader<ChildStdout>,
    address: SocketAddr,
    complete: bool,
}

impl RunningServer {
    fn spawn(config: &Path) -> Self {
        let mut child = process_command(config)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("secure authority process");
        let mut stdout = BufReader::new(child.stdout.take().expect("process stdout"));
        let mut ready = String::new();
        stdout.read_line(&mut ready).expect("process READY line");
        let address = ready
            .split_ascii_whitespace()
            .nth(1)
            .expect("READY address")
            .parse()
            .expect("socket address");
        Self {
            child,
            stdout,
            address,
            complete: false,
        }
    }

    async fn finish(&mut self) -> ProcessOutput {
        self.finish_within(Duration::from_secs(6)).await
    }

    async fn finish_within(&mut self, maximum_duration: Duration) -> ProcessOutput {
        let deadline = Instant::now() + maximum_duration;
        let status = loop {
            if let Some(status) = self.child.try_wait().expect("poll process") {
                break status;
            }
            assert!(Instant::now() < deadline, "secure process did not stop");
            sleep(Duration::from_millis(10)).await;
        };
        self.complete = true;
        let mut stdout = String::new();
        self.stdout
            .read_to_string(&mut stdout)
            .expect("remaining process stdout");
        let mut stderr = String::new();
        self.child
            .stderr
            .take()
            .expect("process stderr")
            .read_to_string(&mut stderr)
            .expect("remaining process stderr");
        ProcessOutput {
            status,
            stdout,
            stderr,
        }
    }
}

impl Drop for RunningServer {
    fn drop(&mut self) {
        if !self.complete {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

struct Fixture {
    directory: PathBuf,
    config: PathBuf,
    key: PathBuf,
    certificate_path: PathBuf,
    certificate: CertificateDer<'static>,
    oidc_signing_key: SigningRsaKeyPair,
    oidc_jwks: Vec<u8>,
    jwks_path: PathBuf,
}

impl Fixture {
    fn new(max_ticks: u64, stop_after_commands: Option<usize>) -> Self {
        let sequence = NEXT_TEST_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let directory = std::env::temp_dir().join(format!(
            "destructible-fps-process-{}-{sequence}",
            std::process::id()
        ));
        fs::create_dir(&directory).expect("process fixture directory");
        let certificate_path = directory.join("server.pem");
        let key_path = directory.join("server-key.pem");
        let jwks_path = directory.join("jwks.json");
        let config = directory.join("server.json");
        let identity = rcgen::generate_simple_self_signed(vec!["localhost".into()])
            .expect("process TLS identity");
        fs::write(&certificate_path, identity.cert.pem()).expect("process certificate");
        fs::write(&key_path, identity.signing_key.serialize_pem()).expect("process private key");
        secure_private_key(&key_path);
        let (oidc_signing_key, jwks) = oidc_material();
        fs::write(&jwks_path, &jwks).expect("process JWKS");
        let valid_until = unix_seconds() + 300;
        let mut document = json!({
            "bind": "127.0.0.1:0",
            "exposure": "loopback",
            "certificate_chain_file": path_string(&certificate_path),
            "private_key_file": path_string(&key_path),
            "oidc_jwks_file": path_string(&jwks_path),
            "oidc_issuer": ISSUER,
            "oidc_audience": AUDIENCE,
            "jwks_valid_until_unix_seconds": valid_until,
            "max_ticks": max_ticks,
        });
        if let Some(commands) = stop_after_commands {
            document["stop_after_commands"] = json!(commands);
        }
        fs::write(
            &config,
            serde_json::to_vec_pretty(&document).expect("process config JSON"),
        )
        .expect("process config");
        Self {
            directory,
            config,
            key: key_path,
            certificate_path,
            certificate: identity.cert.der().clone(),
            oidc_signing_key,
            oidc_jwks: jwks,
            jwks_path,
        }
    }

    fn read_config(&self) -> serde_json::Value {
        serde_json::from_slice(&fs::read(&self.config).expect("read process config"))
            .expect("parse process config")
    }

    fn write_config(&self, document: &serde_json::Value) {
        fs::write(
            &self.config,
            serde_json::to_vec_pretty(document).expect("encode process config"),
        )
        .expect("rewrite process config");
    }

    fn replace_with_expired_tls_identity(&self) {
        let signing_key = rcgen::KeyPair::generate().expect("expired process signing key");
        let mut parameters =
            rcgen::CertificateParams::new(vec!["localhost".into()]).expect("certificate params");
        parameters.not_before = rcgen::date_time_ymd(2020, 1, 1);
        parameters.not_after = rcgen::date_time_ymd(2021, 1, 1);
        let certificate = parameters
            .self_signed(&signing_key)
            .expect("expired process certificate");
        fs::write(&self.certificate_path, certificate.pem()).expect("replace process certificate");
        fs::write(&self.key, signing_key.serialize_pem()).expect("replace process key");
        secure_private_key(&self.key);
    }

    fn replace_with_short_lived_tls_identity(&self, remaining: Duration) {
        let signing_key = rcgen::KeyPair::generate().expect("short-lived process signing key");
        let mut parameters =
            rcgen::CertificateParams::new(vec!["localhost".into()]).expect("certificate params");
        let now = SystemTime::now();
        parameters.not_before = now
            .checked_sub(Duration::from_mins(1))
            .expect("short-lived not-before")
            .into();
        parameters.not_after = now
            .checked_add(remaining)
            .expect("short-lived not-after")
            .into();
        let certificate = parameters
            .self_signed(&signing_key)
            .expect("short-lived process certificate");
        fs::write(&self.certificate_path, certificate.pem())
            .expect("replace short-lived process certificate");
        fs::write(&self.key, signing_key.serialize_pem()).expect("replace short-lived process key");
        secure_private_key(&self.key);
    }

    fn configure_discovery(&self, issuer: &str, root: &Path) {
        let mut document = self.read_config();
        document["oidc_issuer"] = json!(issuer);
        document["oidc_discovery"] = json!({
            "refresh_interval_seconds": 60,
            "root_certificate_file": path_string(root),
        });
        self.write_config(&document);
    }

    fn configure_static_jwks_validity(&self, remaining: Duration) {
        let mut document = self.read_config();
        document["jwks_valid_until_unix_seconds"] =
            json!(unix_seconds().saturating_add(remaining.as_secs()));
        self.write_config(&document);
    }

    fn configure_tls_reload(&self, interval_seconds: u64) {
        let mut document = self.read_config();
        document["tls_reload"] = json!({"interval_seconds": interval_seconds});
        self.write_config(&document);
    }

    fn replace_tls_identity(&self) -> CertificateDer<'static> {
        let identity = rcgen::generate_simple_self_signed(vec!["localhost".into()])
            .expect("replacement process TLS identity");
        fs::write(&self.certificate_path, identity.cert.pem())
            .expect("replacement process certificate");
        fs::write(&self.key, identity.signing_key.serialize_pem())
            .expect("replacement process private key");
        secure_private_key(&self.key);
        identity.cert.der().clone()
    }

    fn signed_token(&self, jti: &str) -> String {
        self.signed_token_for_issuer(jti, ISSUER)
    }

    fn signed_token_for_issuer(&self, jti: &str, issuer: &str) -> String {
        let now = unix_seconds();
        let mut header = Header::new(Algorithm::RS256);
        header.kid = Some(KEY_ID.into());
        let claims = Claims {
            iss: issuer,
            aud: AUDIENCE,
            sub: "process-player",
            exp: now + 120,
            iat: now,
            jti,
        };
        let encoded_header = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .encode(serde_json::to_vec(&header).expect("serialized process token header"));
        let encoded_claims = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .encode(serde_json::to_vec(&claims).expect("serialized process token claims"));
        let message = format!("{encoded_header}.{encoded_claims}");
        let mut signature = vec![0_u8; self.oidc_signing_key.public_modulus_len()];
        self.oidc_signing_key
            .sign(
                &RSA_PKCS1_SHA256,
                &SystemRandom::new(),
                message.as_bytes(),
                &mut signature,
            )
            .expect("signed process token");
        format!(
            "{message}.{}",
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(signature)
        )
    }
}

async fn serve_https_documents(
    listener: TcpListener,
    config: Arc<ServerConfig>,
    documents: Vec<(&'static str, &'static str, Vec<u8>)>,
) {
    let acceptor = TlsAcceptor::from(config);
    for (expected_path, content_type, document) in documents {
        let (stream, _) = listener.accept().await.expect("process discovery client");
        let mut stream = acceptor
            .accept(stream)
            .await
            .expect("process discovery TLS handshake");
        let mut request = vec![0_u8; 8 * 1_024];
        let mut received = 0_usize;
        loop {
            let count = stream
                .read(&mut request[received..])
                .await
                .expect("process discovery request");
            assert!(count > 0, "process discovery request ended before headers");
            received += count;
            if request[..received]
                .windows(4)
                .any(|window| window == b"\r\n\r\n")
            {
                break;
            }
            assert!(
                received < request.len(),
                "process discovery request exceeded bound"
            );
        }
        let request =
            std::str::from_utf8(&request[..received]).expect("UTF-8 process discovery request");
        assert!(
            request.starts_with(&format!("GET {expected_path} HTTP/1.1\r\n")),
            "unexpected process discovery path"
        );
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            document.len()
        );
        stream
            .write_all(response.as_bytes())
            .await
            .expect("process discovery response headers");
        stream
            .write_all(&document)
            .await
            .expect("process discovery response body");
        stream
            .shutdown()
            .await
            .expect("process discovery TLS shutdown");
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.directory);
    }
}

fn oidc_material() -> (SigningRsaKeyPair, Vec<u8>) {
    let key_pair = RsaKeyPair::generate(KeySize::Rsa2048).expect("ephemeral process RSA key");
    let private_der = key_pair.as_der().expect("ephemeral process private key");
    let signing_key =
        SigningRsaKeyPair::from_pkcs8(private_der.as_ref()).expect("ephemeral process signing key");
    let decoding_key = DecodingKey::from_rsa_der(key_pair.public_key().as_ref());
    let mut jwk =
        Jwk::from_decoding_key(&decoding_key, Some(Algorithm::RS256)).expect("process public JWK");
    jwk.common.key_id = Some(KEY_ID.into());
    jwk.common.key_algorithm = Some(KeyAlgorithm::RS256);
    jwk.common.public_key_use = Some(PublicKeyUse::Signature);
    jwk.common.key_operations = Some(vec![KeyOperations::Verify]);
    let jwks = serde_json::to_vec(&JwkSet { keys: vec![jwk] }).expect("process JWKS");
    (signing_key, jwks)
}

fn unix_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("process test clock")
        .as_secs()
}

fn path_string(path: &Path) -> &str {
    path.to_str().expect("UTF-8 process fixture path")
}

fn stop_counter(stdout: &str, name: &str) -> u64 {
    let prefix = format!("{name}=");
    stdout
        .lines()
        .find(|line| line.starts_with("STOP "))
        .and_then(|line| {
            line.split_ascii_whitespace()
                .find_map(|field| field.strip_prefix(&prefix))
        })
        .and_then(|value| value.parse().ok())
        .unwrap_or_else(|| panic!("missing {name} in process output: {stdout}"))
}

fn process_command(config: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_secure-dedicated-server"));
    command.arg("--config").arg(config);
    command
}

#[cfg(unix)]
fn secure_private_key(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
        .expect("secure process key permissions");
}

#[cfg(not(unix))]
fn secure_private_key(_path: &Path) {}
