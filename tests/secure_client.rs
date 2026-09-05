use destructible_fps::{
    AuthenticatedPrincipal, SecureClientConnection, SecureClientLaunchConfig,
    SecureDedicatedServer, SessionCredentialVerifier, demo_world, encode_snapshot_request,
    is_snapshot_datagram, secure_server_config,
};
use quinn::rustls::pki_types::PrivatePkcs8KeyDer;
use std::{
    fs,
    net::{Ipv4Addr, SocketAddr, SocketAddrV4},
    num::NonZeroU64,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};
use tokio::time::{sleep, timeout};

const TEST_CREDENTIAL: [u8; 32] = [0x7c; 32];
const LOOPBACK_EPHEMERAL: SocketAddr = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0));
static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(1);

struct TestVerifier;

impl SessionCredentialVerifier for TestVerifier {
    fn verify(&self, credential: &[u8]) -> Option<AuthenticatedPrincipal> {
        (credential == TEST_CREDENTIAL)
            .then(|| AuthenticatedPrincipal::new(NonZeroU64::new(417).expect("non-zero principal")))
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_backed_secure_client_establishes_and_receives_bounded_gameplay() {
    let fixture = Fixture::new();
    let identity =
        rcgen::generate_simple_self_signed(vec!["localhost".into()]).expect("test TLS identity");
    fs::write(&fixture.certificate, identity.cert.pem()).expect("root certificate fixture");
    write_private(&fixture.credential, &TEST_CREDENTIAL);
    let private_key = PrivatePkcs8KeyDer::from(identity.signing_key.serialize_der()).into();
    let server_config = secure_server_config(vec![identity.cert.der().clone()], private_key)
        .expect("server TLS config");
    let mut server = SecureDedicatedServer::bind(
        LOOPBACK_EPHEMERAL,
        server_config,
        Arc::new(TestVerifier),
        demo_world(),
    )
    .expect("secure authority");
    let config = SecureClientLaunchConfig {
        server_address: server.local_addr().expect("server address"),
        server_name: "localhost".to_owned(),
        root_certificate_file: fixture.certificate.clone(),
        credential_file: fixture.credential.clone(),
    };
    let connect = tokio::spawn(async move { SecureClientConnection::connect(&config).await });
    for _ in 0..200 {
        server.tick().expect("admission tick");
        if connect.is_finished() {
            break;
        }
        sleep(Duration::from_millis(2)).await;
    }
    let client = timeout(Duration::from_secs(2), connect)
        .await
        .expect("client connect deadline")
        .expect("client task")
        .expect("verified authenticated client");
    assert!(client.session_id() > 0);
    assert!(client.server_nonce() > 0);
    assert_eq!(server.active_sessions(), 1);
    let inbox = client.spawn_datagram_inbox(&tokio::runtime::Handle::current());

    client
        .send(encode_snapshot_request(client.session_id()))
        .expect("encrypted snapshot request");
    let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
    let mut received_snapshot = false;
    while tokio::time::Instant::now() < deadline && !received_snapshot {
        server.tick().expect("snapshot tick");
        for _ in 0..64 {
            match inbox.try_receive().expect("bounded encrypted gameplay") {
                Some(payload) if is_snapshot_datagram(&payload) => {
                    received_snapshot = true;
                    break;
                }
                Some(_) => {}
                None => break,
            }
        }
        sleep(Duration::from_millis(1)).await;
    }
    assert!(received_snapshot);
    assert_eq!(inbox.dropped_datagrams(), 0);
    client.close();
    for _ in 0..100 {
        server.tick().expect("disconnect tick");
        if server.active_sessions() == 0 {
            break;
        }
        sleep(Duration::from_millis(2)).await;
    }
    assert_eq!(server.active_sessions(), 0);
    server.shutdown().await;
}

struct Fixture {
    directory: PathBuf,
    certificate: PathBuf,
    credential: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let sequence = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
        let directory = std::env::temp_dir().join(format!(
            "destructible-fps-secure-client-{}-{sequence}",
            std::process::id()
        ));
        fs::create_dir(&directory).expect("secure client fixture directory");
        Self {
            certificate: directory.join("root.pem"),
            credential: directory.join("credential"),
            directory,
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.directory);
    }
}

#[cfg(unix)]
fn write_private(path: &Path, bytes: &[u8]) {
    use std::{io::Write, os::unix::fs::OpenOptionsExt};
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .expect("private credential fixture");
    file.write_all(bytes).expect("write credential fixture");
}

#[cfg(not(unix))]
fn write_private(path: &Path, bytes: &[u8]) {
    fs::write(path, bytes).expect("private credential fixture");
}
