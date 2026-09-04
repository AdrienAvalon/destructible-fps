//! Core simulation for the destructible FPS technical spike.
//!
//! The simulation and wire protocol stay independent from rendering. The playable
//! milestone adds a safe Vulkan presentation layer without moving authority into it.

pub mod destruction;
pub mod generator;
pub mod material;
pub mod mesh;
pub mod mesh_scheduler;
pub mod player;
pub mod render;
pub mod replication;
pub mod session;
pub mod world;

pub use destruction::{DestructionReport, Explosion};
pub use generator::demo_world;
pub use material::{Material, MaterialProperties, Voxel};
pub use replication::{
    AuthoritativeServer, ClientReplica, ClientStatus, CodecError, CommandError, DeltaFrame,
    DeltaPacket, ExplosionCommand, FrameAssembler, ReplicationError, decode_frame, encode_frames,
};
pub use session::{DemoSession, FireMode, SessionError, ShotResult, dirty_chunks};
pub use world::{CHUNK_EDGE, IVec3, VoxelChange, World, WorldError, WorldStats, chunk_position};
