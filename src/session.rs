//! In-process playable session that still crosses the authoritative wire-format boundary.

use crate::{
    AuthoritativeServer, BodyId, BuildCommand, BuildReport, CHUNK_EDGE, ClientReplica, CodecError,
    CommandError, DestructionReport, ExplosionCommand, FixedMicrometers3, FrameAssembler, IVec3,
    MAX_BUILD_REACH_VOXELS, MICROMETERS_PER_VOXEL, Material, PLAYER_EYE_HEIGHT_UM,
    PLAYER_HEIGHT_UM, PLAYER_RADIUS_UM, PhysicsTickReport, PlayerBuildContext, ReplicationError,
    RigidBodyDescriptor, RigidBodyState, VoxelChange, World, chunk_position, decode_frame,
    demo_world, encode_frames, player::raycast,
};
use core::fmt;
use glam::Vec3;
use std::collections::{BTreeMap, HashSet};

const CLIENT_ID: u64 = 1;
const DATAGRAM_MTU: usize = 1_200;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FireMode {
    Rifle,
    Explosive,
}

#[derive(Clone, Debug)]
pub struct ShotResult {
    pub target: IVec3,
    pub report: DestructionReport,
    pub datagrams: usize,
    pub encoded_bytes: usize,
    pub dirty_chunks: Vec<IVec3>,
    pub spawned_body_ids: Vec<BodyId>,
    pub active_bodies: usize,
}

#[derive(Clone, Debug)]
pub struct BuildResult {
    pub target: IVec3,
    pub report: BuildReport,
    pub datagrams: usize,
    pub encoded_bytes: usize,
    pub dirty_chunks: Vec<IVec3>,
}

#[derive(Debug)]
pub enum SessionError {
    Command(CommandError),
    Codec(CodecError),
    Replication(ReplicationError),
    MissingCompletePacket,
    DivergedReplica,
    InvalidBuildOrigin,
}

impl fmt::Display for SessionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Command(error) => error.fmt(formatter),
            Self::Codec(error) => error.fmt(formatter),
            Self::Replication(error) => error.fmt(formatter),
            Self::MissingCompletePacket => {
                write!(formatter, "all frames arrived but no delta assembled")
            }
            Self::DivergedReplica => {
                write!(formatter, "authoritative and rendered replicas diverged")
            }
            Self::InvalidBuildOrigin => {
                write!(formatter, "build origin is outside fixed world bounds")
            }
        }
    }
}

impl std::error::Error for SessionError {}

impl From<CommandError> for SessionError {
    fn from(value: CommandError) -> Self {
        Self::Command(value)
    }
}

impl From<CodecError> for SessionError {
    fn from(value: CodecError) -> Self {
        Self::Codec(value)
    }
}

impl From<ReplicationError> for SessionError {
    fn from(value: ReplicationError) -> Self {
        Self::Replication(value)
    }
}

pub struct DemoSession {
    server: AuthoritativeServer,
    client: ClientReplica,
    assembler: FrameAssembler,
    next_command_id: u64,
}

impl Default for DemoSession {
    fn default() -> Self {
        Self::new(demo_world())
    }
}

impl DemoSession {
    #[must_use]
    pub fn new(world: World) -> Self {
        Self {
            server: AuthoritativeServer::new(world.clone()),
            client: ClientReplica::new(world),
            assembler: FrameAssembler::default(),
            next_command_id: 1,
        }
    }

    #[must_use]
    pub const fn world(&self) -> &World {
        self.client.world()
    }

    #[must_use]
    pub const fn bodies(&self) -> &BTreeMap<BodyId, RigidBodyDescriptor> {
        self.client.bodies()
    }

    #[must_use]
    pub const fn body_states(&self) -> &BTreeMap<BodyId, RigidBodyState> {
        self.client.body_states()
    }

    /// Advances the 60 Hz authoritative rigid-body simulation and crosses the same bounded
    /// protocol path whenever at least one state changes.
    ///
    /// # Errors
    ///
    /// Returns codec, reassembly, replication, or final consistency failures.
    pub fn advance_physics(&mut self) -> Result<PhysicsTickReport, SessionError> {
        let (packet, report) = self.server.advance_physics();
        if let Some(packet) = packet {
            let mut encoded = encode_frames(&packet, DATAGRAM_MTU)?;
            encoded.reverse();
            let mut assembled = None;
            for bytes in encoded {
                if let Some(packet) = self.assembler.push(decode_frame(&bytes)?)? {
                    assembled = Some(packet);
                }
            }
            let assembled = assembled.ok_or(SessionError::MissingCompletePacket)?;
            self.client.receive(&assembled)?;
        }
        if self.client.world().fingerprint() != self.server.world().fingerprint()
            || self.client.body_fingerprint() != self.server.body_fingerprint()
            || self.client.body_states() != self.server.body_states()
            || self.client.next_body_id() != self.server.next_body_id()
        {
            return Err(SessionError::DivergedReplica);
        }
        Ok(report)
    }

    /// Finds the targeted voxel, executes destruction on the authority, serializes it to bounded
    /// datagrams, and applies the reassembled delta to the rendered replica.
    ///
    /// # Errors
    ///
    /// Returns protocol, command, or consistency failures. A clean miss returns `Ok(None)`.
    pub fn fire(
        &mut self,
        origin: Vec3,
        direction: Vec3,
        mode: FireMode,
    ) -> Result<Option<ShotResult>, SessionError> {
        let Some(hit) = raycast(self.client.world(), origin, direction, 120.0) else {
            return Ok(None);
        };
        let (radius_voxels, peak_energy) = match mode {
            FireMode::Rifle => (2, 7_500),
            FireMode::Explosive => (6, 42_000),
        };
        let command = ExplosionCommand {
            command_id: self.next_command_id,
            center: hit.voxel,
            radius_voxels,
            peak_energy,
        };
        self.next_command_id = self.next_command_id.wrapping_add(1);
        let (packet, report) = self.server.execute_explosion(CLIENT_ID, command)?;
        let mut encoded = encode_frames(&packet, DATAGRAM_MTU)?;
        let encoded_bytes = encoded.iter().map(Vec::len).sum();
        let datagrams = encoded.len();

        // The assembler is deliberately exercised with reverse delivery order.
        encoded.reverse();
        let mut assembled = None;
        for bytes in encoded {
            if let Some(packet) = self.assembler.push(decode_frame(&bytes)?)? {
                assembled = Some(packet);
            }
        }
        let assembled = assembled.ok_or(SessionError::MissingCompletePacket)?;
        self.client.receive(&assembled)?;
        if self.client.world().fingerprint() != self.server.world().fingerprint()
            || self.client.body_fingerprint() != self.server.body_fingerprint()
            || self.client.body_states() != self.server.body_states()
            || self.client.next_body_id() != self.server.next_body_id()
        {
            return Err(SessionError::DivergedReplica);
        }

        let mut spawned_body_ids = packet
            .body_assignments
            .iter()
            .map(|assignment| assignment.body_id)
            .collect::<Vec<_>>();
        spawned_body_ids.dedup();
        Ok(Some(ShotResult {
            target: hit.voxel,
            dirty_chunks: dirty_chunks(&packet.changes),
            report,
            spawned_body_ids,
            active_bodies: self.client.bodies().len(),
            datagrams,
            encoded_bytes,
        }))
    }

    /// Finds the empty voxel immediately before a ray hit and requests a validated construction
    /// transaction from the authority.
    ///
    /// # Errors
    ///
    /// Returns protocol, construction-policy, or consistency failures. A clean miss returns
    /// `Ok(None)`.
    pub fn build(
        &mut self,
        origin: Vec3,
        direction: Vec3,
        material: Material,
    ) -> Result<Option<BuildResult>, SessionError> {
        let player = local_build_context(origin).ok_or(SessionError::InvalidBuildOrigin)?;
        let Some(target) = raycast(
            self.client.world(),
            origin,
            direction,
            MAX_BUILD_REACH_VOXELS,
        )
        .and_then(|hit| hit.adjacent_empty) else {
            return Ok(None);
        };
        let command = BuildCommand {
            command_id: self.next_command_id,
            position: target,
            material,
        };
        self.next_command_id = self.next_command_id.wrapping_add(1);
        let (packet, report) = self.server.execute_build(CLIENT_ID, command, player)?;
        let mut encoded = encode_frames(&packet, DATAGRAM_MTU)?;
        let encoded_bytes = encoded.iter().map(Vec::len).sum();
        let datagrams = encoded.len();
        encoded.reverse();
        let mut assembled = None;
        for bytes in encoded {
            if let Some(packet) = self.assembler.push(decode_frame(&bytes)?)? {
                assembled = Some(packet);
            }
        }
        let assembled = assembled.ok_or(SessionError::MissingCompletePacket)?;
        self.client.receive(&assembled)?;
        if self.client.world().fingerprint() != self.server.world().fingerprint()
            || self.client.body_fingerprint() != self.server.body_fingerprint()
            || self.client.body_states() != self.server.body_states()
            || self.client.next_body_id() != self.server.next_body_id()
        {
            return Err(SessionError::DivergedReplica);
        }
        Ok(Some(BuildResult {
            target,
            report,
            datagrams,
            encoded_bytes,
            dirty_chunks: dirty_chunks(&packet.changes),
        }))
    }
}

#[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
fn local_build_context(eye: Vec3) -> Option<PlayerBuildContext> {
    fn fixed(value: f32) -> Option<i64> {
        if !value.is_finite() || value.abs() > 1_000_016.0 {
            return None;
        }
        #[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
        Some((f64::from(value) * MICROMETERS_PER_VOXEL as f64).round() as i64)
    }

    let eye = FixedMicrometers3 {
        x: fixed(eye.x)?,
        y: fixed(eye.y)?,
        z: fixed(eye.z)?,
    };
    let bottom_y = eye.y.saturating_sub(PLAYER_EYE_HEIGHT_UM);
    Some(PlayerBuildContext {
        eye_position_um: eye,
        bounds_minimum_um: FixedMicrometers3 {
            x: eye.x.saturating_sub(PLAYER_RADIUS_UM),
            y: bottom_y,
            z: eye.z.saturating_sub(PLAYER_RADIUS_UM),
        },
        bounds_maximum_um: FixedMicrometers3 {
            x: eye.x.saturating_add(PLAYER_RADIUS_UM),
            y: bottom_y.saturating_add(PLAYER_HEIGHT_UM),
            z: eye.z.saturating_add(PLAYER_RADIUS_UM),
        },
    })
}

#[must_use]
pub fn dirty_chunks(changes: &[VoxelChange]) -> Vec<IVec3> {
    let mut chunks = HashSet::new();
    for change in changes {
        let primary = chunk_position(change.position);
        chunks.insert(primary);
        insert_boundary_neighbor(
            &mut chunks,
            primary,
            change.position.x.rem_euclid(CHUNK_EDGE),
            IVec3::new(1, 0, 0),
        );
        insert_boundary_neighbor(
            &mut chunks,
            primary,
            change.position.y.rem_euclid(CHUNK_EDGE),
            IVec3::new(0, 1, 0),
        );
        insert_boundary_neighbor(
            &mut chunks,
            primary,
            change.position.z.rem_euclid(CHUNK_EDGE),
            IVec3::new(0, 0, 1),
        );
    }
    let mut chunks: Vec<_> = chunks.into_iter().collect();
    chunks.sort_unstable();
    chunks
}

fn insert_boundary_neighbor(
    chunks: &mut HashSet<IVec3>,
    primary: IVec3,
    local: i32,
    positive_axis: IVec3,
) {
    let direction = if local == 0 {
        -1
    } else if local == CHUNK_EDGE - 1 {
        1
    } else {
        return;
    };
    chunks.insert(IVec3::new(
        primary.x + positive_axis.x * direction,
        primary.y + positive_axis.y * direction,
        primary.z + positive_axis.z * direction,
    ));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DEFAULT_CONSTRUCTION_UNITS, Material, Voxel};

    #[test]
    fn playable_shot_crosses_codec_and_preserves_replica() {
        let mut world = World::default();
        world.fill_box(
            IVec3::new(-4, -4, -8),
            IVec3::new(4, 4, -4),
            Voxel::new(Material::Brick),
        );
        let initial = world.fingerprint();
        let mut session = DemoSession::new(world);
        let result = session
            .fire(Vec3::new(0.5, 0.5, 0.0), -Vec3::Z, FireMode::Explosive)
            .expect("shot must replicate")
            .expect("shot must hit");
        assert!(result.report.fractured_voxels > 0);
        assert!(result.datagrams > 1);
        assert_ne!(session.world().fingerprint(), initial);
    }

    #[test]
    fn playable_build_crosses_codec_and_preserves_replica() {
        let mut world = World::default();
        world.set_voxel(IVec3::new(0, 2, -5), Voxel::new(Material::Stone));
        let mut session = DemoSession::new(world);

        let result = session
            .build(Vec3::new(0.5, 2.5, 0.0), -Vec3::Z, Material::Wood)
            .expect("build must replicate")
            .expect("supported target is visible");

        assert_eq!(result.target, IVec3::new(0, 2, -4));
        assert_eq!(
            result.report.remaining_units,
            DEFAULT_CONSTRUCTION_UNITS - 1
        );
        assert_eq!(result.datagrams, 1);
        assert_eq!(
            session.world().voxel(result.target),
            Voxel::new(Material::Wood)
        );
        assert_eq!(
            result.dirty_chunks,
            dirty_chunks(&[VoxelChange {
                position: result.target,
                before: Voxel::AIR,
                after: Voxel::new(Material::Wood),
            }])
        );
    }

    #[test]
    fn a_miss_does_not_advance_world() {
        let mut session = DemoSession::new(World::default());
        assert!(
            session
                .fire(Vec3::ZERO, -Vec3::Z, FireMode::Rifle)
                .expect("miss is valid")
                .is_none()
        );
        assert_eq!(session.world().tick(), 0);
    }

    #[test]
    fn non_finite_build_origin_is_rejected_before_raycast_or_command_advance() {
        let mut session = DemoSession::new(World::default());
        assert!(matches!(
            session.build(Vec3::splat(f32::NAN), -Vec3::Z, Material::Wood),
            Err(SessionError::InvalidBuildOrigin)
        ));
        assert_eq!(session.next_command_id, 1);
    }

    #[test]
    fn remeshing_only_crosses_touched_chunk_boundaries() {
        let change = |position| VoxelChange {
            position,
            before: Voxel::new(Material::Brick),
            after: Voxel::AIR,
        };
        assert_eq!(dirty_chunks(&[change(IVec3::new(4, 5, 6))]).len(), 1);
        assert_eq!(dirty_chunks(&[change(IVec3::new(15, 0, -16))]).len(), 4);
    }
}
