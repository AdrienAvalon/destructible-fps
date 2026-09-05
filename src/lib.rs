//! Core simulation for the destructible FPS technical spike.
//!
//! The simulation and wire protocol stay independent from rendering. The playable
//! milestone adds a safe Vulkan presentation layer without moving authority into it.

pub mod destruction;
pub mod generator;
pub mod material;
pub mod mesh;
pub mod mesh_scheduler;
pub mod network;
pub mod physics;
pub mod player;
pub mod render;
pub mod replication;
pub mod session;
pub mod snapshot;
pub mod structural;
pub mod telemetry;
pub mod transport;
pub mod world;

pub use destruction::{DestructionReport, Explosion};
pub use generator::demo_world;
pub use material::{Material, MaterialProperties, Voxel};
pub use network::{
    DedicatedServer, MAX_OUTBOUND_DATAGRAMS_PER_TICK, MAX_QUEUED_COMMANDS, MAX_QUEUED_REPAIRS,
    MAX_RECEIVED_DATAGRAMS_PER_TICK, MAX_REPAIRS_PER_TICK, MAX_RETAINED_DELTA_BYTES,
    MAX_RETAINED_DELTA_PACKETS, MAX_SERVER_PEERS, MAX_SIMULATED_COMMANDS_PER_TICK,
    MAX_SNAPSHOT_CATCHUP_BYTES, MAX_SNAPSHOT_CATCHUP_PACKETS,
    MAX_SNAPSHOT_FRAMES_PER_PEER_PER_TICK, NetworkRuntimeError, NetworkTickReport,
    OrderedDeltaInbox,
};
pub use physics::{
    BodyError, BodyId, BodyLimits, BodySimulationReport, BodyStateTransition, BodyStepResult,
    BodyVoxel, BroadPhaseResult, FixedMicrometers3, FixedMillimeters3, InertiaDiagonalKgMm2,
    MAX_BROAD_PHASE_PAIRS, MICROMETERS_PER_VOXEL, RigidBodyDescriptor, RigidBodyState,
    SERVER_PHYSICS_HZ, broad_phase_pairs, step_rigid_bodies, step_rigid_body,
    valid_rigid_body_state,
};
pub use replication::{
    AuthoritativeServer, BodyStateUpdate, BodyVoxelAssignment, ClientReplica, ClientStatus,
    CodecError, CommandError, DeltaFrame, DeltaPacket, ExplosionCommand, FrameAssembler,
    PhysicsTickReport, ReplicationError, decode_frame, encode_frames,
};
pub use session::{DemoSession, FireMode, SessionError, ShotResult, dirty_chunks};
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
    decode_client_control, decode_server_control, encode_client_hello, encode_explosion_request,
    encode_repair_request, encode_server_welcome, encode_snapshot_ack,
    encode_snapshot_fragments_request, encode_snapshot_request, is_delta_datagram,
};
pub use world::{CHUNK_EDGE, IVec3, VoxelChange, World, WorldError, WorldStats, chunk_position};
