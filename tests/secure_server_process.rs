use aws_lc_rs::{
    encoding::AsDer,
    rand::SystemRandom,
    rsa::{KeyPair as RsaKeyPair, KeySize},
    signature::{KeyPair as _, RSA_PKCS1_SHA256, RsaKeyPair as SigningRsaKeyPair},
};
use base64::Engine as _;
use destructible_fps::{
    ExplosionCommand, IVec3, encode_explosion_request, establish_session, secure_client_config,
    send_gameplay_datagram,
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
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tokio::time::{sleep, timeout};

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
        let deadline = Instant::now() + Duration::from_secs(6);
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
        fs::write(&jwks_path, jwks).expect("process JWKS");
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

    fn signed_token(&self, jti: &str) -> String {
        let now = unix_seconds();
        let mut header = Header::new(Algorithm::RS256);
        header.kid = Some(KEY_ID.into());
        let claims = Claims {
            iss: ISSUER,
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
