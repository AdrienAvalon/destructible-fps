//! In-process playable session that still crosses the authoritative wire-format boundary.

use crate::structural_runtime::{
    StructuralRuntime, StructuralRuntimeStatus, StructuralSimulationConfig,
};
use crate::{
    AuthoritativeServer, BodyId, BuildCommand, BuildReport, ClientReplica, CodecError,
    CommandError, DestructionReport, ExplosionCommand, FixedMicrometers3, FrameAssembler, IVec3,
    MAX_BUILD_REACH_VOXELS, MICROMETERS_PER_VOXEL, Material, PLAYER_EYE_HEIGHT_UM,
    PLAYER_HEIGHT_UM, PLAYER_RADIUS_UM, PhysicsTickReport, PlayerBuildContext, ReplicationError,
    RigidBodyDescriptor, RigidBodyState, VoxelChange, World, chunk_position, decode_frame,
    demo_world, encode_frames, player::raycast,
};
use core::fmt;
use glam::Vec3;
use std::collections::{BTreeMap, HashSet};

use crate::mesh::FRACTURE_RENDER_DEPENDENCY_RADIUS_VOXELS;

const CLIENT_ID: u64 = 1;
const DATAGRAM_MTU: usize = 1_200;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FireMode {
    Rifle,
    Explosive,
    /// Synthetic partial-damage probe for the structural lab, not calibrated rifle ballistics.
    TestCharge,
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
    structural: Option<StructuralRuntime>,
}

#[derive(Debug, Default)]
pub struct SessionTick {
    pub physics: PhysicsTickReport,
    pub structural: StructuralRuntimeStatus,
    pub dirty_chunks: Vec<IVec3>,
    pub spawned_body_ids: Vec<BodyId>,
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
            structural: None,
        }
    }

    /// Enables the same dirty-domain runtime as the transport-independent server, at startup.
    /// # Errors
    /// Returns seed-limit or OS worker-creation errors without partially enabling the policy.
    pub fn with_structural_simulation(
        mut self,
        config: &StructuralSimulationConfig,
    ) -> std::io::Result<Self> {
        if self.structural.is_some() || self.next_command_id != 1 || self.server.world().tick() != 0
        {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "structural policy is startup-only",
            ));
        }
        let runtime = StructuralRuntime::new(&config.initial_seeds)?;
        self.server.configure_structural_simulation(config);
        self.structural = Some(runtime);
        Ok(self)
    }

    #[must_use]
    pub fn structural_status(&self) -> StructuralRuntimeStatus {
        self.structural
            .as_ref()
            .map_or_else(StructuralRuntimeStatus::default, StructuralRuntime::status)
    }

    #[must_use]
    pub fn structural_error(&self) -> Option<&crate::structural_failure::StructuralFailureError> {
        self.structural
            .as_ref()
            .and_then(StructuralRuntime::last_error)
    }

    /// Advances structural commits before rigid-body physics, notifying the renderer of topology.
    /// # Errors
    /// Returns ordinary framed replication/consistency errors, not silent partial presentation.
    pub fn tick(&mut self) -> Result<SessionTick, SessionError> {
        let mut result = SessionTick::default();
        if let Some(runtime) = &mut self.structural {
            let packet = runtime.tick(&mut self.server);
            result.structural = runtime.status();
            if let Some(packet) = packet {
                result.dirty_chunks = dirty_chunks(&packet.changes);
                result.spawned_body_ids = packet
                    .body_assignments
                    .iter()
                    .map(|entry| entry.body_id)
                    .collect();
                result.spawned_body_ids.sort_unstable();
                result.spawned_body_ids.dedup();
                let mut assembled = None;
                for bytes in encode_frames(&packet, DATAGRAM_MTU)?.into_iter().rev() {
                    assembled = self.assembler.push(decode_frame(&bytes)?)?.or(assembled);
                }
                self.client
                    .receive(&assembled.ok_or(SessionError::MissingCompletePacket)?)?;
            }
        }
        result.physics = self.advance_physics()?;
        Ok(result)
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
            FireMode::TestCharge => (1, 1_000),
        };
        let command = ExplosionCommand {
            command_id: self.next_command_id,
            center: hit.voxel,
            radius_voxels,
            peak_energy,
        };
        self.next_command_id = self.next_command_id.wrapping_add(1);
        let (packet, report) = self.server.execute_explosion(CLIENT_ID, command)?;
        if let Some(runtime) = &mut self.structural {
            runtime.observe_changes(&self.server, &packet.changes);
        }
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
        if let Some(runtime) = &mut self.structural {
            runtime.observe_changes(&self.server, &packet.changes);
        }
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
        let touches_masonry = [change.before.material, change.after.material]
            .into_iter()
            .any(|material| matches!(material, Material::Brick | Material::Concrete));
        // Ordinary topology can change a neighboring masonry classification, whose derived cells
        // add one more voxel of dependency. A masonry change can additionally toggle cut surfaces
        // throughout the bounded visual halo. Each diameter fits within CHUNK_EDGE, so one
        // isolated change still invalidates at most 2^3 chunks.
        let radius = if touches_masonry {
            FRACTURE_RENDER_DEPENDENCY_RADIUS_VOXELS
        } else {
            2
        };
        let minimum = chunk_position(IVec3::new(
            change.position.x.saturating_sub(radius),
            change.position.y.saturating_sub(radius),
            change.position.z.saturating_sub(radius),
        ));
        let maximum = chunk_position(IVec3::new(
            change.position.x.saturating_add(radius),
            change.position.y.saturating_add(radius),
            change.position.z.saturating_add(radius),
        ));
        // Enumerate the resulting chunks directly instead of hashing 15^3 voxel samples on the
        // client's receive path. The closed integer bounding range yields the identical set.
        for x in minimum.x..=maximum.x {
            for y in minimum.y..=maximum.y {
                for z in minimum.z..=maximum.z {
                    chunks.insert(IVec3::new(x, y, z));
                }
            }
        }
    }
    let mut chunks: Vec<_> = chunks.into_iter().collect();
    chunks.sort_unstable();
    chunks
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DEFAULT_CONSTRUCTION_UNITS, Material, Voxel, mesh::mesh_chunk};

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
    fn ordinary_remeshing_only_crosses_bounded_chunk_boundaries() {
        let change = |position| VoxelChange {
            position,
            before: Voxel::new(Material::Wood),
            after: Voxel::AIR,
        };
        assert_eq!(dirty_chunks(&[change(IVec3::new(4, 5, 6))]).len(), 1);
        assert_eq!(dirty_chunks(&[change(IVec3::new(15, 5, 6))]).len(), 2);
        assert_eq!(dirty_chunks(&[change(IVec3::new(15, 0, 6))]).len(), 4);
        assert_eq!(dirty_chunks(&[change(IVec3::new(15, 0, -16))]).len(), 8);
    }

    #[test]
    fn masonry_damage_invalidates_a_cross_chunk_fracture_halo() {
        let intact = Voxel::new(Material::Brick);
        let damaged = Voxel {
            material: Material::Brick,
            integrity: 224,
        };
        let damage_position = IVec3::new(10, 0, 0);
        let mut world = World::default();
        world.fill_box(IVec3::new(9, -1, 0), IVec3::new(18, 1, 0), intact);
        world.set_voxel(IVec3::new(15, 0, 0), Voxel::AIR);

        let neighboring_chunk = IVec3::new(1, 0, 0);
        let before = mesh_chunk(&world, neighboring_chunk);
        assert!(
            before
                .vertices
                .iter()
                .all(|vertex| vertex.fracture_depth < 0.0)
        );

        let change = VoxelChange {
            position: damage_position,
            before: intact,
            after: damaged,
        };
        world.set_voxel(damage_position, damaged);
        let after = mesh_chunk(&world, neighboring_chunk);

        assert!(dirty_chunks(&[change]).contains(&neighboring_chunk));
        assert!(
            after
                .vertices
                .iter()
                .any(|vertex| vertex.fracture_depth >= 0.0),
            "damage in chunk zero must refresh the layered cut surface in chunk one"
        );
    }

    #[test]
    fn direct_chunk_invalidation_matches_voxel_enumeration_at_signed_boundaries() {
        for material in [Material::Brick, Material::Wood] {
            let radius = if material == Material::Brick {
                FRACTURE_RENDER_DEPENDENCY_RADIUS_VOXELS
            } else {
                2
            };
            for x in -17_i32..=17 {
                let position = IVec3::new(x, x + 5, -x - 6);
                let change = VoxelChange {
                    position,
                    before: Voxel::new(material),
                    after: Voxel::AIR,
                };
                let mut reference = HashSet::new();
                for dx in -radius..=radius {
                    for dy in -radius..=radius {
                        for dz in -radius..=radius {
                            reference.insert(chunk_position(IVec3::new(
                                x + dx,
                                position.y + dy,
                                position.z + dz,
                            )));
                        }
                    }
                }
                let actual = dirty_chunks(&[change]);
                assert!(actual.len() <= 8);
                assert_eq!(actual.into_iter().collect::<HashSet<_>>(), reference);
            }
        }
        for value in [i32::MIN, i32::MAX] {
            let chunks = dirty_chunks(&[VoxelChange {
                position: IVec3::new(value, value, value),
                before: Voxel::new(Material::Concrete),
                after: Voxel::AIR,
            }]);
            assert!(!chunks.is_empty() && chunks.len() <= 8);
        }
    }
}
