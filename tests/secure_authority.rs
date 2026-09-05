use destructible_fps::{
    AuthenticatedPrincipal, BuildCommand, DeltaPacket, ExplosionCommand, IVec3,
    MAX_SESSION_DATAGRAMS_PER_SECOND, Material, OrderedDeltaInbox, PlayerInputCommand,
    SecureDedicatedServer, SessionCredentialVerifier, Voxel, World, demo_world,
    encode_build_request, encode_explosion_request, encode_player_input, encode_snapshot_request,
    establish_session, receive_gameplay_datagram, secure_client_config, secure_server_config,
    send_gameplay_datagram,
};
use quinn::rustls::{
    RootCertStore,
    pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer},
};
use quinn::{Endpoint, VarInt};
use std::{
    net::{Ipv4Addr, SocketAddr, SocketAddrV4},
    num::NonZeroU64,
    sync::Arc,
    time::Duration,
};
use tokio::time::{sleep, timeout};

const TEST_CREDENTIAL: [u8; 32] = [0x5a; 32];
const LOOPBACK_EPHEMERAL: SocketAddr = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0));

struct TestVerifier;

impl SessionCredentialVerifier for TestVerifier {
    fn verify(&self, credential: &[u8]) -> Option<AuthenticatedPrincipal> {
        (credential == TEST_CREDENTIAL).then(|| {
            AuthenticatedPrincipal::new(
                NonZeroU64::new(271).expect("non-zero integration principal"),
            )
        })
    }
}

#[tokio::test]
async fn secure_authority_rejects_non_loopback_without_production_policy() {
    let (certificate, private_key) = test_identity();
    let server_config =
        secure_server_config(vec![certificate], private_key).expect("server config");
    let error = SecureDedicatedServer::bind(
        SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 0)),
        server_config,
        Arc::new(TestVerifier),
        World::default(),
    )
    .err()
    .expect("wildcard bind must require production policy");

    assert_eq!(error.kind(), std::io::ErrorKind::PermissionDenied);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn two_authenticated_quic_clients_receive_one_authoritative_transaction() {
    let (certificate, private_key) = test_identity();
    let server_config =
        secure_server_config(vec![certificate.clone()], private_key).expect("server config");
    let mut server = SecureDedicatedServer::bind(
        LOOPBACK_EPHEMERAL,
        server_config,
        Arc::new(TestVerifier),
        demo_world(),
    )
    .expect("secure authority");
    let client = trusted_client(certificate);
    let first = connect(&client, server.local_addr().expect("server address")).await;
    let second = connect(&client, server.local_addr().expect("server address")).await;
    let (first_welcome, second_welcome) = tokio::join!(
        establish_session(&first, 101, &TEST_CREDENTIAL),
        establish_session(&second, 103, &TEST_CREDENTIAL),
    );
    let first_welcome = first_welcome.expect("first admitted session");
    let second_welcome = second_welcome.expect("second admitted session");
    assert!(first_welcome.session_id < second_welcome.session_id);

    let mut admitted = 0_usize;
    for _ in 0..100 {
        let report = server.tick().expect("admission tick");
        admitted += report.admitted_sessions;
        if server.active_sessions() == 2 {
            break;
        }
        sleep(Duration::from_millis(2)).await;
    }
    assert_eq!(admitted, 2);
    assert_eq!(server.active_sessions(), 2);

    let command = ExplosionCommand {
        command_id: 1,
        center: IVec3::new(0, 1, 0),
        radius_voxels: 4,
        peak_energy: 10_000,
    };
    send_gameplay_datagram(
        &first,
        encode_explosion_request(first_welcome.session_id, command),
    )
    .expect("encrypted gameplay command");
    let mut applied = 0_usize;
    for _ in 0..100 {
        let report = server.tick().expect("authority tick");
        applied += report.authority.commands_applied;
        if applied == 1 {
            break;
        }
        sleep(Duration::from_millis(2)).await;
    }
    assert_eq!(applied, 1);

    let first_packet = receive_transaction(&first).await;
    let second_packet = receive_transaction(&second).await;
    assert_eq!(first_packet, second_packet);
    assert_eq!(first_packet.sequence, 1);
    assert_eq!(
        first_packet.final_fingerprint,
        server.authority().world().fingerprint()
    );

    first.close(VarInt::from_u32(0), b"first client complete");
    for _ in 0..100 {
        let report = server.tick().expect("disconnect tick");
        if report.disconnected_sessions > 0 && server.active_sessions() == 1 {
            break;
        }
        sleep(Duration::from_millis(2)).await;
    }
    assert_eq!(server.active_sessions(), 1);
    second.close(VarInt::from_u32(0), b"second client complete");
    server.shutdown().await;
    client.wait_idle().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn authenticated_quic_client_builds_one_authoritative_voxel() {
    let (certificate, private_key) = test_identity();
    let server_config =
        secure_server_config(vec![certificate.clone()], private_key).expect("server config");
    let mut world = World::default();
    world.fill_box(
        IVec3::new(-1, 0, 35),
        IVec3::new(1, 0, 41),
        Voxel::new(Material::Stone),
    );
    let mut server = SecureDedicatedServer::bind(
        LOOPBACK_EPHEMERAL,
        server_config,
        Arc::new(TestVerifier),
        world,
    )
    .expect("secure authority");
    let client = trusted_client(certificate);
    let connection = connect(&client, server.local_addr().expect("server address")).await;
    let welcome = establish_session(&connection, 107, &TEST_CREDENTIAL)
        .await
        .expect("admitted session");
    for _ in 0..100 {
        server.tick().expect("admission tick");
        if server.active_sessions() == 1 {
            break;
        }
        sleep(Duration::from_millis(2)).await;
    }
    assert_eq!(server.active_sessions(), 1);
    send_gameplay_datagram(
        &connection,
        encode_player_input(
            welcome.session_id,
            PlayerInputCommand {
                input_sequence: 1,
                movement_x_per_mille: 1_000,
                ..PlayerInputCommand::default()
            },
        ),
    )
    .expect("encrypted player input");
    for _ in 0..100 {
        let report = server.tick().expect("player simulation tick");
        if report.authority.player_inputs_accepted == 1 {
            break;
        }
        sleep(Duration::from_millis(2)).await;
    }
    assert!(
        server
            .player_state(welcome.session_id)
            .expect("authoritative player")
            .position_um
            .x
            > 0
    );
    let command = BuildCommand {
        command_id: 1,
        position: IVec3::new(0, 1, 36),
        material: Material::Wood,
    };
    send_gameplay_datagram(
        &connection,
        encode_build_request(welcome.session_id, command),
    )
    .expect("encrypted build command");
    for _ in 0..100 {
        let report = server.tick().expect("construction tick");
        if report.authority.commands_applied == 1 {
            break;
        }
        sleep(Duration::from_millis(2)).await;
    }

    let packet = receive_transaction(&connection).await;
    assert_eq!(packet.sequence, 1);
    assert_eq!(packet.changes.len(), 1);
    assert_eq!(packet.changes[0].position, command.position);
    assert_eq!(packet.changes[0].after, Voxel::new(Material::Wood));
    assert_eq!(
        server.authority().world().voxel(command.position),
        Voxel::new(Material::Wood)
    );

    connection.close(VarInt::from_u32(0), b"build complete");
    server.shutdown().await;
    client.wait_idle().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn invalid_credential_never_enters_the_authority() {
    let (certificate, private_key) = test_identity();
    let server_config =
        secure_server_config(vec![certificate.clone()], private_key).expect("server config");
    let mut server = SecureDedicatedServer::bind(
        LOOPBACK_EPHEMERAL,
        server_config,
        Arc::new(TestVerifier),
        demo_world(),
    )
    .expect("secure authority");
    let client = trusted_client(certificate);
    let connection = connect(&client, server.local_addr().expect("server address")).await;
    assert!(
        establish_session(&connection, 107, &[0x6b; 32])
            .await
            .is_err()
    );

    let mut admission_failures = 0_usize;
    for _ in 0..100 {
        let report = server.tick().expect("rejection tick");
        admission_failures += report.admission_failures;
        if admission_failures > 0 {
            break;
        }
        sleep(Duration::from_millis(2)).await;
    }
    assert_eq!(admission_failures, 1);
    assert_eq!(server.active_sessions(), 0);
    server.shutdown().await;
    client.wait_idle().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn per_session_rate_limit_closes_a_burst_before_simulation_work() {
    let (certificate, private_key) = test_identity();
    let server_config =
        secure_server_config(vec![certificate.clone()], private_key).expect("server config");
    let mut server = SecureDedicatedServer::bind(
        LOOPBACK_EPHEMERAL,
        server_config,
        Arc::new(TestVerifier),
        demo_world(),
    )
    .expect("secure authority");
    let client = trusted_client(certificate);
    let connection = connect(&client, server.local_addr().expect("server address")).await;
    let welcome = establish_session(&connection, 109, &TEST_CREDENTIAL)
        .await
        .expect("admitted session");
    for _ in 0..100 {
        server.tick().expect("admission tick");
        if server.active_sessions() == 1 {
            break;
        }
        sleep(Duration::from_millis(2)).await;
    }
    assert_eq!(server.active_sessions(), 1);

    for _ in 0..=MAX_SESSION_DATAGRAMS_PER_SECOND {
        send_gameplay_datagram(&connection, encode_snapshot_request(welcome.session_id))
            .expect("bounded burst datagram");
    }
    timeout(Duration::from_secs(2), connection.closed())
        .await
        .expect("rate-limited connection closes");
    let mut rate_limited = 0_usize;
    for _ in 0..100 {
        let report = server.tick().expect("rate-limit tick");
        rate_limited += report.rate_limited_sessions;
        if rate_limited > 0 && server.active_sessions() == 0 {
            break;
        }
        sleep(Duration::from_millis(2)).await;
    }
    assert_eq!(rate_limited, 1);
    assert_eq!(server.active_sessions(), 0);
    server.shutdown().await;
    client.wait_idle().await;
}

async fn connect(client: &Endpoint, server_address: SocketAddr) -> quinn::Connection {
    timeout(
        Duration::from_secs(2),
        client
            .connect(server_address, "localhost")
            .expect("start verified connection"),
    )
    .await
    .expect("TLS handshake deadline")
    .expect("verified server certificate")
}

async fn receive_transaction(connection: &quinn::Connection) -> DeltaPacket {
    let mut inbox = OrderedDeltaInbox::default();
    for _ in 0..256 {
        let payload = timeout(
            Duration::from_secs(2),
            receive_gameplay_datagram(connection),
        )
        .await
        .expect("authoritative datagram deadline")
        .expect("bounded authoritative datagram");
        let ready = inbox.push(&payload).expect("valid authoritative frame");
        if let Some(packet) = ready.into_iter().next() {
            return packet;
        }
    }
    panic!("authoritative transaction exceeded bounded test receive loop");
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
