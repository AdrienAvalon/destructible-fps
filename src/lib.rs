//! Core simulation for the destructible FPS technical spike.
//!
//! The simulation and wire protocol stay independent from rendering. The playable
//! milestone adds a safe Vulkan presentation layer without moving authority into it.

pub mod destruction;
pub mod generator;
pub mod material;
pub mod mesh;
pub mod mesh_scheduler;
pub mod physics;
pub mod player;
pub mod render;
pub mod replication;
pub mod session;
pub mod structural;
pub mod telemetry;
pub mod world;

pub use destruction::{DestructionReport, Explosion};
pub use generator::demo_world;
pub use material::{Material, MaterialProperties, Voxel};
pub use physics::{
    BodyError, BodyLimits, BodyStepResult, BodyVoxel, FixedMicrometers3, FixedMillimeters3,
    InertiaDiagonalKgMm2, MAX_BROAD_PHASE_PAIRS, MICROMETERS_PER_VOXEL, RigidBodyDescriptor,
    RigidBodyState, SERVER_PHYSICS_HZ, broad_phase_pairs, step_rigid_body, valid_rigid_body_state,
};
pub use replication::{
    AuthoritativeServer, BodyStateUpdate, BodyVoxelAssignment, ClientReplica, ClientStatus,
    CodecError, CommandError, DeltaFrame, DeltaPacket, ExplosionCommand, FrameAssembler,
    PhysicsTickReport, ReplicationError, decode_frame, encode_frames,
};
pub use session::{DemoSession, FireMode, SessionError, ShotResult, dirty_chunks};
pub use structural::{
    DetachedIsland, StructuralAnchors, StructuralError, StructuralLimits, StructuralReport,
    analyze_structural_changes,
};
pub use telemetry::{DistributionSummary, SampleWindow};
pub use world::{CHUNK_EDGE, IVec3, VoxelChange, World, WorldError, WorldStats, chunk_position};
