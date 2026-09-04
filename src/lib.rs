//! Core simulation for the destructible FPS technical spike.
//!
//! The first milestone intentionally has no third-party dependency. It proves the
//! authoritative destruction transaction and its network representation before a
//! renderer or a high-frequency physics solver is allowed to hide correctness bugs.

pub mod destruction;
pub mod generator;
pub mod material;
pub mod replication;
pub mod world;

pub use destruction::{DestructionReport, Explosion};
pub use generator::demo_world;
pub use material::{Material, MaterialProperties, Voxel};
pub use replication::{
    AuthoritativeServer, ClientReplica, ClientStatus, CodecError, DeltaFrame, DeltaPacket,
    ExplosionCommand, FrameAssembler, ReplicationError, decode_frame, encode_frames,
};
pub use world::{CHUNK_EDGE, IVec3, VoxelChange, World, WorldError, WorldStats};
