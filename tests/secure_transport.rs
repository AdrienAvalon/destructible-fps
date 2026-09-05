use destructible_fps::{
    AuthenticatedPrincipal, ClientControlMessage, ExplosionCommand, IVec3,
    MAX_QUIC_DATAGRAM_PAYLOAD_BYTES, MAX_SESSION_HELLO_BYTES, SampleWindow,
    SecureDatagramReceiveError, ServerControlMessage, SessionAdmissionError,
    SessionCredentialVerifier, SessionWelcome, admit_session, decode_client_control,
    decode_server_control, decode_session_hello, encode_explosion_request, encode_server_welcome,
    encode_session_welcome, establish_session, receive_gameplay_datagram, secure_client_config,
    secure_server_config, send_gameplay_datagram,
};
use quinn::rustls::{
    RootCertStore,
    pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer},
};
use quinn::{Endpoint, VarInt};
use std::{
    net::{Ipv4Addr, SocketAddr, SocketAddrV4},
    num::NonZeroU64,
    time::{Duration, Instant},
};
use tokio::time::timeout;

const TEST_CREDENTIAL: [u8; 32] = [0x5a; 32];
const LOOPBACK_EPHEMERAL: SocketAddr = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0));

struct TestVerifier;

impl SessionCredentialVerifier for TestVerifier {
    fn verify(&self, credential: &[u8]) -> Option<AuthenticatedPrincipal> {
        (credential == TEST_CREDENTIAL).then(|| {
            AuthenticatedPrincipal::new(NonZeroU64::new(41).expect("non-zero test principal"))
        })
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn verified_quic_session_carries_authenticated_gameplay_datagrams() {
    let (certificate, private_key) = test_identity();
    let server_config =
        secure_server_config(vec![certificate.clone()], private_key).expect("secure server config");
    let server = Endpoint::server(server_config, LOOPBACK_EPHEMERAL).expect("secure server");
    let server_address = server.local_addr().expect("secure server address");
    let server_endpoint = server.clone();
    let server_task = tokio::spawn(async move {
        let incoming = timeout(Duration::from_secs(2), server_endpoint.accept())
            .await
            .expect("server accept deadline")
            .expect("incoming QUIC connection");
        let connection = incoming.await.expect("TLS 1.3 server handshake");
        let session = admit_session(
            connection,
            &TestVerifier,
            NonZeroU64::new(73).expect("non-zero session"),
            NonZeroU64::new(79).expect("non-zero server nonce"),
        )
        .await
        .expect("authenticated application session");
        assert_eq!(
            session.principal(),
            AuthenticatedPrincipal::new(NonZeroU64::new(41).expect("non-zero test principal"))
        );
        assert_eq!(session.client_nonce(), 67);
        assert_eq!(session.session_id(), 73);
        assert_eq!(session.server_nonce(), 79);

        let datagram = timeout(
            Duration::from_secs(2),
            receive_gameplay_datagram(session.connection()),
        )
        .await
        .expect("gameplay datagram deadline")
        .expect("encrypted gameplay datagram");
        assert!(matches!(
            decode_client_control(datagram.as_ref()),
            Ok(ClientControlMessage::Explosion { session_id: 73, .. })
        ));
        session
            .connection()
            .send_datagram(vec![0; MAX_QUIC_DATAGRAM_PAYLOAD_BYTES + 1].into())
            .expect("peer sends oversized inner datagram");
        send_gameplay_datagram(session.connection(), encode_server_welcome(67, 73))
            .expect("encrypted server datagram");
        let _ = timeout(Duration::from_secs(2), session.connection().closed()).await;
    });

    let client = trusted_client(certificate);
    let connection = timeout(
        Duration::from_secs(2),
        client
            .connect(server_address, "localhost")
            .expect("start verified connection"),
    )
    .await
    .expect("client handshake deadline")
    .expect("verified server certificate");
    assert!(connection.peer_identity().is_some());
    let welcome = establish_session(&connection, 67, &TEST_CREDENTIAL)
        .await
        .expect("establish authenticated session");
    assert_eq!(welcome.session_id, 73);
    assert_eq!(welcome.server_nonce, 79);
    let command = ExplosionCommand {
        command_id: 1,
        center: IVec3::new(0, 1, 2),
        radius_voxels: 3,
        peak_energy: 4,
    };
    send_gameplay_datagram(&connection, encode_explosion_request(73, command))
        .expect("send authenticated gameplay command");
    assert!(matches!(
        send_gameplay_datagram(&connection, vec![0; MAX_QUIC_DATAGRAM_PAYLOAD_BYTES + 1]),
        Err(destructible_fps::SecureDatagramError::Oversized(_))
    ));
    assert_bounded_server_datagrams(&connection).await;
    connection.close(VarInt::from_u32(0), b"test complete");
    server_task.await.expect("secure server task");
    client.wait_idle().await;
    server.close(VarInt::from_u32(0), b"test complete");
    server.wait_idle().await;
}

async fn assert_bounded_server_datagrams(connection: &quinn::Connection) {
    let mut valid = false;
    let mut oversized = false;
    for _ in 0..2 {
        match timeout(
            Duration::from_secs(2),
            receive_gameplay_datagram(connection),
        )
        .await
        .expect("server datagram deadline")
        {
            Ok(response) => {
                assert_eq!(
                    decode_server_control(response.as_ref()),
                    Ok(ServerControlMessage::Welcome {
                        nonce: 67,
                        session_id: 73,
                    })
                );
                valid = true;
            }
            Err(SecureDatagramReceiveError::Oversized(bytes)) => {
                assert_eq!(bytes, MAX_QUIC_DATAGRAM_PAYLOAD_BYTES + 1);
                oversized = true;
            }
            Err(error) => panic!("unexpected secure datagram error: {error}"),
        }
    }
    assert!(valid && oversized);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn invalid_application_credential_closes_the_quic_connection() {
    let (certificate, private_key) = test_identity();
    let server_config =
        secure_server_config(vec![certificate.clone()], private_key).expect("secure server config");
    let server = Endpoint::server(server_config, LOOPBACK_EPHEMERAL).expect("secure server");
    let server_address = server.local_addr().expect("secure server address");
    let server_endpoint = server.clone();
    let server_task = tokio::spawn(async move {
        let incoming = timeout(Duration::from_secs(2), server_endpoint.accept())
            .await
            .expect("server accept deadline")
            .expect("incoming QUIC connection");
        let connection = incoming.await.expect("TLS 1.3 server handshake");
        admit_session(
            connection,
            &TestVerifier,
            NonZeroU64::new(83).expect("non-zero session"),
            NonZeroU64::new(89).expect("non-zero server nonce"),
        )
        .await
    });

    let client = trusted_client(certificate);
    let connection = client
        .connect(server_address, "localhost")
        .expect("start verified connection")
        .await
        .expect("verified server certificate");
    let rejected = establish_session(&connection, 97, &[0x6b; 32]).await;
    assert!(rejected.is_err());
    assert!(matches!(
        server_task.await.expect("credential rejection task"),
        Err(SessionAdmissionError::RejectedCredential)
    ));
    client.wait_idle().await;
    server.close(VarInt::from_u32(0), b"test complete");
    server.wait_idle().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn mismatched_server_nonce_echo_closes_the_client_connection() {
    let (certificate, private_key) = test_identity();
    let server_config =
        secure_server_config(vec![certificate.clone()], private_key).expect("secure server config");
    let server = Endpoint::server(server_config, LOOPBACK_EPHEMERAL).expect("secure server");
    let server_address = server.local_addr().expect("secure server address");
    let server_endpoint = server.clone();
    let server_task = tokio::spawn(async move {
        let incoming = timeout(Duration::from_secs(2), server_endpoint.accept())
            .await
            .expect("server accept deadline")
            .expect("incoming QUIC connection");
        let connection = incoming.await.expect("TLS 1.3 server handshake");
        let (mut send, mut receive) = connection.accept_bi().await.expect("admission stream");
        let request = receive
            .read_to_end(MAX_SESSION_HELLO_BYTES)
            .await
            .expect("bounded session hello");
        let hello = decode_session_hello(&request).expect("valid session hello");
        let response = encode_session_welcome(SessionWelcome {
            client_nonce: hello.client_nonce + 1,
            session_id: 107,
            server_nonce: 109,
        })
        .expect("mismatched test welcome");
        send.write_all(&response).await.expect("write test welcome");
        send.finish().expect("finish test welcome");
        assert!(matches!(
            timeout(Duration::from_secs(2), connection.closed())
                .await
                .expect("client rejection deadline"),
            quinn::ConnectionError::ApplicationClosed(_)
        ));
    });

    let client = trusted_client(certificate);
    let connection = client
        .connect(server_address, "localhost")
        .expect("start verified connection")
        .await
        .expect("verified server certificate");
    assert!(matches!(
        establish_session(&connection, 113, &TEST_CREDENTIAL).await,
        Err(SessionAdmissionError::NonceMismatch)
    ));
    server_task.await.expect("nonce mismatch task");
    client.wait_idle().await;
    server.close(VarInt::from_u32(0), b"test complete");
    server.wait_idle().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn stalled_application_admission_times_out_and_closes() {
    let (certificate, private_key) = test_identity();
    let server_config =
        secure_server_config(vec![certificate.clone()], private_key).expect("secure server config");
    let server = Endpoint::server(server_config, LOOPBACK_EPHEMERAL).expect("secure server");
    let server_address = server.local_addr().expect("secure server address");
    let server_endpoint = server.clone();
    let server_task = tokio::spawn(async move {
        let incoming = timeout(Duration::from_secs(2), server_endpoint.accept())
            .await
            .expect("server accept deadline")
            .expect("incoming QUIC connection");
        let connection = incoming.await.expect("TLS 1.3 server handshake");
        admit_session(
            connection,
            &TestVerifier,
            NonZeroU64::new(101).expect("non-zero session"),
            NonZeroU64::new(103).expect("non-zero server nonce"),
        )
        .await
    });

    let client = trusted_client(certificate);
    let connection = client
        .connect(server_address, "localhost")
        .expect("start verified connection")
        .await
        .expect("verified server certificate");
    let closed = timeout(Duration::from_secs(7), connection.closed())
        .await
        .expect("internal admission deadline closes connection");
    assert!(matches!(
        closed,
        quinn::ConnectionError::ApplicationClosed(_)
    ));
    assert!(matches!(
        server_task.await.expect("admission timeout task"),
        Err(SessionAdmissionError::TimedOut)
    ));
    client.wait_idle().await;
    server.close(VarInt::from_u32(0), b"test complete");
    server.wait_idle().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn untrusted_server_certificate_fails_before_application_admission() {
    let (server_certificate, private_key) = test_identity();
    let (untrusted_certificate, _unused_key) = test_identity();
    let server_config =
        secure_server_config(vec![server_certificate], private_key).expect("secure server config");
    let server = Endpoint::server(server_config, LOOPBACK_EPHEMERAL).expect("secure server");
    let server_address = server.local_addr().expect("secure server address");
    let server_endpoint = server.clone();
    let server_task = tokio::spawn(async move {
        let incoming = timeout(Duration::from_secs(2), server_endpoint.accept())
            .await
            .expect("server accept deadline")
            .expect("incoming QUIC connection");
        assert!(incoming.await.is_err());
    });

    let client = trusted_client(untrusted_certificate);
    let result = timeout(
        Duration::from_secs(2),
        client
            .connect(server_address, "localhost")
            .expect("start untrusted connection"),
    )
    .await
    .expect("untrusted handshake deadline");
    assert!(result.is_err());
    server_task.await.expect("untrusted server task");
    client.wait_idle().await;
    server.close(VarInt::from_u32(0), b"test complete");
    server.wait_idle().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn repeated_secure_session_latency_is_measured_separately() {
    const ITERATIONS: usize = 20;
    let (certificate, private_key) = test_identity();
    let server_config =
        secure_server_config(vec![certificate.clone()], private_key).expect("secure server config");
    let server = Endpoint::server(server_config, LOOPBACK_EPHEMERAL).expect("secure server");
    let server_address = server.local_addr().expect("secure server address");
    let server_task = tokio::spawn(admit_repeated_sessions(server.clone(), ITERATIONS));
    let client = trusted_client(certificate);
    let mut tls_samples = SampleWindow::new(ITERATIONS);
    let mut admission_samples = SampleWindow::new(ITERATIONS);
    let mut total_samples = SampleWindow::new(ITERATIONS);

    for iteration in 0..ITERATIONS {
        let total_started = Instant::now();
        let tls_started = Instant::now();
        let connection = client
            .connect(server_address, "localhost")
            .expect("start measured connection")
            .await
            .expect("measured server certificate");
        tls_samples.record_ms(tls_started.elapsed().as_secs_f64() * 1_000.0);
        let admission_started = Instant::now();
        let welcome = establish_session(&connection, measured_nonce(iteration), &TEST_CREDENTIAL)
            .await
            .expect("measured application admission");
        admission_samples.record_ms(admission_started.elapsed().as_secs_f64() * 1_000.0);
        total_samples.record_ms(total_started.elapsed().as_secs_f64() * 1_000.0);
        assert_eq!(welcome.session_id, measured_session_id(iteration));
        connection.close(VarInt::from_u32(0), b"measured session complete");
    }

    server_task.await.expect("measured server task");
    let tls = tls_samples.summary().expect("TLS samples");
    let admission = admission_samples.summary().expect("admission samples");
    let total = total_samples.summary().expect("total samples");
    assert_eq!(tls.samples, ITERATIONS);
    assert_eq!(admission.samples, ITERATIONS);
    assert_eq!(total.samples, ITERATIONS);
    eprintln!(
        "secure sessions: TLS p50={:.3}ms p95={:.3}ms p99={:.3}ms; admission p50={:.3}ms p95={:.3}ms p99={:.3}ms; total p50={:.3}ms p95={:.3}ms p99={:.3}ms",
        tls.p50_ms,
        tls.p95_ms,
        tls.p99_ms,
        admission.p50_ms,
        admission.p95_ms,
        admission.p99_ms,
        total.p50_ms,
        total.p95_ms,
        total.p99_ms
    );
    client.wait_idle().await;
    server.close(VarInt::from_u32(0), b"test complete");
    server.wait_idle().await;
}

async fn admit_repeated_sessions(endpoint: Endpoint, iterations: usize) {
    for iteration in 0..iterations {
        let incoming = timeout(Duration::from_secs(2), endpoint.accept())
            .await
            .expect("measured accept deadline")
            .expect("measured incoming connection");
        let connection = incoming.await.expect("measured TLS server handshake");
        let session = admit_session(
            connection,
            &TestVerifier,
            NonZeroU64::new(measured_session_id(iteration)).expect("measured session ID"),
            NonZeroU64::new(10_001).expect("measured server nonce"),
        )
        .await
        .expect("measured authenticated session");
        assert_eq!(session.client_nonce(), measured_nonce(iteration));
        let _ = timeout(Duration::from_secs(2), session.connection().closed()).await;
    }
}

fn measured_nonce(iteration: usize) -> u64 {
    1_001_u64.saturating_add(u64::try_from(iteration).expect("small measured iteration"))
}

fn measured_session_id(iteration: usize) -> u64 {
    2_001_u64.saturating_add(u64::try_from(iteration).expect("small measured iteration"))
}

fn trusted_client(certificate: CertificateDer<'static>) -> Endpoint {
    let mut roots = RootCertStore::empty();
    roots.add(certificate).expect("trusted test certificate");
    let config = secure_client_config(roots).expect("secure client config");
    let mut endpoint = Endpoint::client(LOOPBACK_EPHEMERAL).expect("secure client");
    endpoint.set_default_client_config(config);
    endpoint
}

fn test_identity() -> (CertificateDer<'static>, PrivateKeyDer<'static>) {
    let identity =
        rcgen::generate_simple_self_signed(vec!["localhost".into()]).expect("test certificate");
    let certificate = identity.cert.der().clone();
    let private_key = PrivatePkcs8KeyDer::from(identity.signing_key.serialize_der()).into();
    (certificate, private_key)
}
