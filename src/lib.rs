//! Core simulation for the destructible FPS technical spike.
//!
//! The simulation and wire protocol stay independent from rendering. The playable
//! milestone adds a safe Vulkan presentation layer without moving authority into it.

pub mod character;
pub mod destruction;
pub mod generator;
pub mod material;
pub mod mesh;
pub mod mesh_scheduler;
pub mod network;
pub mod oidc;
pub mod physics;
pub mod player;
pub mod render;
pub mod replication;
pub mod secure_server;
pub mod secure_transport;
pub mod server_config;
pub mod session;
pub mod snapshot;
pub mod structural;
pub mod telemetry;
pub mod transport;
pub mod world;

pub use character::{
    AuthoritativePlayer, AuthoritativePlayerState, MAX_PLAYER_INPUT_HOLD_TICKS,
    MAX_PLAYER_INPUT_PER_MILLE, PLAYER_EYE_HEIGHT_UM, PLAYER_HEIGHT_UM, PLAYER_RADIUS_UM,
    PlayerBuildContext, PlayerInputCommand, PlayerInputError, PlayerStepReport,
};
pub use destruction::{DestructionReport, Explosion};
pub use generator::demo_world;
pub use material::{Material, MaterialProperties, Voxel};
pub use network::{
    AuthorityCore, DedicatedServer, LEGACY_UDP_APPLICATION_DATAGRAM_BYTES,
    MAX_APPLICATION_DATAGRAM_BYTES, MAX_OUTBOUND_DATAGRAMS_PER_TICK, MAX_QUEUED_COMMANDS,
    MAX_QUEUED_REPAIRS, MAX_RECEIVED_DATAGRAMS_PER_TICK, MAX_REPAIRS_PER_TICK,
    MAX_RETAINED_DELTA_BYTES, MAX_RETAINED_DELTA_PACKETS, MAX_SERVER_PEERS,
    MAX_SIMULATED_COMMANDS_PER_TICK, MAX_SNAPSHOT_CATCHUP_BYTES, MAX_SNAPSHOT_CATCHUP_PACKETS,
    MAX_SNAPSHOT_FRAMES_PER_PEER_PER_TICK, MIN_APPLICATION_DATAGRAM_BYTES, NetworkRuntimeError,
    NetworkTickReport, OrderedDeltaInbox,
};
pub use oidc::{
    MAX_OIDC_JWKS_BYTES, MAX_OIDC_JWKS_KEYS, MAX_OIDC_REPLAY_ENTRIES,
    MAX_OIDC_TOKEN_LIFETIME_SECONDS, OIDC_CLOCK_SKEW_SECONDS, OIDC_MINIMUM_REMAINING_SECONDS,
    OidcSessionVerifier, OidcVerificationError,
};
pub use physics::{
    BodyError, BodyId, BodyLimits, BodySimulationReport, BodyStateTransition, BodyStepResult,
    BodyVoxel, BroadPhaseResult, FIXED_QUATERNION_SCALE, FixedImpulseMilliNewtonSeconds3,
    FixedMicrometers3, FixedMillimeters3, FixedMilliradians3, FixedQuaternion,
    InertiaDiagonalKgMm2, MAX_ANGULAR_SPEED_MRAD_PER_SECOND, MAX_BODY_SOLVER_PASSES,
    MAX_BROAD_PHASE_PAIRS, MICROMETERS_PER_VOXEL, RigidBodyDescriptor, RigidBodyState,
    SERVER_PHYSICS_HZ, apply_impulse_at_local_point, apply_linear_impulse, broad_phase_pairs,
    step_rigid_bodies, step_rigid_body, valid_fixed_quaternion, valid_rigid_body_state,
};
pub use replication::{
    AuthoritativeServer, BodyStateUpdate, BodyVoxelAssignment, BuildCommand, BuildReport,
    ClientReplica, ClientStatus, CodecError, CommandError, DEFAULT_CONSTRUCTION_UNITS, DeltaFrame,
    DeltaPacket, ExplosionCommand, FrameAssembler, MAX_BUILD_COORDINATE, MAX_BUILD_REACH_UM,
    MAX_BUILD_REACH_VOXELS, PhysicsTickReport, ReplicationError, decode_frame, encode_frames,
};
pub use secure_server::{
    MAX_CONSECUTIVE_GAMEPLAY_QUEUE_DROPS, MAX_SECURE_CONTROL_EVENTS, MAX_SECURE_GAMEPLAY_BYTES,
    MAX_SECURE_GAMEPLAY_EVENTS, MAX_SESSION_DATAGRAMS_PER_SECOND, SecureDedicatedServer,
    SecureNetworkTickReport,
};
pub use secure_transport::{
    ALPN_PROTOCOL, AuthenticatedPrincipal, AuthenticatedSession, MAX_PENDING_QUIC_HANDSHAKES,
    MAX_QUIC_DATAGRAM_PAYLOAD_BYTES, MAX_SESSION_CREDENTIAL_BYTES, MAX_SESSION_HELLO_BYTES,
    SecureConfigError, SecureDatagramError, SecureDatagramReceiveError, SessionAdmissionError,
    SessionCodecError, SessionCredentialVerifier, SessionHello, SessionWelcome, admit_session,
    decode_session_hello, decode_session_welcome, encode_session_hello, encode_session_welcome,
    establish_session, receive_gameplay_datagram, secure_client_config, secure_server_config,
    send_gameplay_datagram,
};
pub use server_config::{
    MAX_CERTIFICATE_CHAIN_BYTES, MAX_CERTIFICATE_CHAIN_ENTRIES, MAX_SECURE_CONFIG_BYTES,
    MAX_STATIC_JWKS_VALIDITY_SECONDS, MAX_TLS_PRIVATE_KEY_BYTES, MIN_STATIC_JWKS_VALIDITY_SECONDS,
    SecureAuthorityLaunchConfig, SecureAuthorityLaunchError, SecureNetworkExposure,
};
pub use session::{BuildResult, DemoSession, FireMode, SessionError, ShotResult, dirty_chunks};
pub use snapshot::{
    AuthoritativeSnapshot, MAX_SNAPSHOT_BODY_VOXELS, MAX_SNAPSHOT_DATAGRAM_BYTES,
    MAX_SNAPSHOT_PAYLOAD_BYTES, MAX_SNAPSHOT_STATIC_VOXELS, SnapshotAssembler, SnapshotCodecError,
    encode_snapshot_frames, is_snapshot_datagram,
};
pub use structural::{
    DetachedIsland, StructuralAnchors, StructuralError, StructuralLimits, StructuralReport,
    analyze_structural_changes,
};
pub use telemetry::{DistributionSummary, SampleWindow};
pub use transport::{
    ClientControlMessage, ControlCodecError, MAX_UDP_DATAGRAM_BYTES, ServerControlMessage,
    decode_client_control, decode_server_control, encode_build_request, encode_client_hello,
    encode_explosion_request, encode_player_input, encode_repair_request, encode_server_welcome,
    encode_snapshot_ack, encode_snapshot_fragments_request, encode_snapshot_request,
    is_delta_datagram,
};
pub use world::{CHUNK_EDGE, IVec3, VoxelChange, World, WorldError, WorldStats, chunk_position};
