use crate::character::PlayerBuildContext;
use crate::destruction::{DestructionReport, Explosion};
use crate::material::{InvalidMaterial, Voxel};
use crate::physics::{
    BodyError, BodyId, BodyLimits, BodyVoxel, FixedImpulseMilliNewtonSeconds3, FixedMicrometers3,
    FixedMillimeters3, FixedMilliradians3, FixedQuaternion, MICROMETERS_PER_VOXEL,
    RigidBodyDescriptor, RigidBodyState, apply_impulse_at_local_point, step_rigid_bodies,
    valid_rigid_body_state,
};
use crate::structural::{
    StructuralAnchors, StructuralError, StructuralLimits, analyze_structural_changes,
};
use crate::world::{IVec3, VoxelChange, World, WorldError};
use core::fmt;
use std::collections::{BTreeMap, HashMap};

const MAGIC: [u8; 4] = *b"DFPS";
const PROTOCOL_VERSION: u8 = 6;
const DELTA_KIND: u8 = 1;
const HEADER_BYTES: usize = 96;
const CHANGE_BYTES: usize = 16;
const BODY_ASSIGNMENT_BYTES: usize = 22;
const BODY_UPDATE_BYTES: usize = 105;
const MAX_DATAGRAM_BYTES: usize = 1_200;
const MAX_FRAGMENTS: u16 = 1_024;
const MAX_PENDING_PACKETS: usize = 64;
const MAX_PENDING_BYTES: usize = 8 * 1_024 * 1_024;
pub const MAX_ACTIVE_BODIES: usize = 1_024;
const MAX_ACTIVE_BODY_VOXELS: usize = 262_144;
const MAX_SPAWNED_BODY_VOXELS: usize = 16_384;
pub const DEFAULT_CONSTRUCTION_UNITS: u32 = 512;
pub const MAX_BUILD_COORDINATE: i32 = 1_000_000;
pub const MAX_BUILD_REACH_UM: i64 = 6 * MICROMETERS_PER_VOXEL;
pub const MAX_BUILD_REACH_VOXELS: f32 = 6.0;
const BUILD_NEIGHBORS: [IVec3; 6] = [
    IVec3::new(-1, 0, 0),
    IVec3::new(1, 0, 0),
    IVec3::new(0, -1, 0),
    IVec3::new(0, 1, 0),
    IVec3::new(0, 0, -1),
    IVec3::new(0, 0, 1),
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ExplosionCommand {
    pub command_id: u64,
    pub center: IVec3,
    pub radius_voxels: u16,
    pub peak_energy: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BuildCommand {
    pub command_id: u64,
    pub position: IVec3,
    pub material: crate::Material,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BuildReport {
    pub position: IVec3,
    pub material: crate::Material,
    pub spent_units: u32,
    pub remaining_units: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BodyVoxelAssignment {
    pub body_id: BodyId,
    pub position: IVec3,
    pub voxel: Voxel,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BodyStateUpdate {
    pub body_id: BodyId,
    pub state: RigidBodyState,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PhysicsTickReport {
    pub updated_bodies: usize,
    pub static_collisions: usize,
    pub body_collisions: usize,
    pub bodies_put_to_sleep: usize,
    pub bodies_woken: usize,
    pub broad_phase_pairs: usize,
    pub broad_phase_saturated: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DeltaPacket {
    pub sequence: u64,
    pub tick: u64,
    pub base_fingerprint: u128,
    pub final_fingerprint: u128,
    pub base_body_fingerprint: u128,
    pub final_body_fingerprint: u128,
    pub changes: Vec<VoxelChange>,
    pub body_assignments: Vec<BodyVoxelAssignment>,
    pub body_updates: Vec<BodyStateUpdate>,
}

impl DeltaPacket {
    #[must_use]
    pub(crate) const fn retained_bytes(&self) -> usize {
        HEADER_BYTES
            .saturating_add(self.changes.len().saturating_mul(CHANGE_BYTES))
            .saturating_add(
                self.body_assignments
                    .len()
                    .saturating_mul(BODY_ASSIGNMENT_BYTES),
            )
            .saturating_add(self.body_updates.len().saturating_mul(BODY_UPDATE_BYTES))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DeltaFrame {
    pub sequence: u64,
    pub tick: u64,
    pub base_fingerprint: u128,
    pub final_fingerprint: u128,
    pub base_body_fingerprint: u128,
    pub final_body_fingerprint: u128,
    pub fragment_index: u16,
    pub fragment_count: u16,
    pub changes: Vec<VoxelChange>,
    pub body_assignments: Vec<BodyVoxelAssignment>,
    pub body_updates: Vec<BodyStateUpdate>,
}

#[derive(Clone)]
pub struct AuthoritativeServer {
    world: World,
    bodies: BTreeMap<BodyId, RigidBodyDescriptor>,
    body_states: BTreeMap<BodyId, RigidBodyState>,
    body_fingerprint: u128,
    active_body_voxels: usize,
    structural_anchors: StructuralAnchors,
    pub(crate) structural_context: crate::structural_jobs::StructuralContext,
    structural_limits: StructuralLimits,
    body_limits: BodyLimits,
    next_body_id: BodyId,
    next_sequence: u64,
    last_command_id: HashMap<u64, u64>,
    construction_units: HashMap<u64, u32>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CommandError {
    ZeroRadius,
    RadiusTooLarge(u16),
    EnergyTooLarge(u32),
    InvalidNetworkExplosionProfile {
        radius_voxels: u16,
        peak_energy: u32,
    },
    ExplosionOutOfReach(IVec3),
    InvalidBuildMaterial(crate::Material),
    BuildCoordinateOutOfRange(IVec3),
    BuildPositionOccupied(IVec3),
    BuildPositionUnsupported(IVec3),
    BuildOutOfReach(IVec3),
    BuildOccluded(IVec3),
    BuildOverlapsPlayer(IVec3),
    MissingAuthoritativePlayer(u64),
    InvalidPlayerBuildContext(u64),
    BuildOverlapsBody {
        position: IVec3,
        body_id: BodyId,
    },
    InsufficientConstructionUnits {
        available: u32,
        required: u32,
    },
    ReplayedCommand {
        client_id: u64,
        command_id: u64,
    },
    Structural(StructuralError),
    Body(BodyError),
    TooManyActiveBodies(usize),
    TooManyActiveBodyVoxels(usize),
    DuplicateBodyId(BodyId),
    BodyIdExhausted,
    TransactionTooLarge {
        changes: usize,
        body_assignments: usize,
    },
}

impl fmt::Display for CommandError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ZeroRadius => write!(formatter, "explosion radius must be non-zero"),
            Self::RadiusTooLarge(radius) => {
                write!(formatter, "explosion radius {radius} exceeds 16")
            }
            Self::EnergyTooLarge(energy) => {
                write!(formatter, "explosion energy {energy} exceeds 1,000,000")
            }
            Self::InvalidNetworkExplosionProfile {
                radius_voxels,
                peak_energy,
            } => write!(
                formatter,
                "network explosion profile radius {radius_voxels}, energy {peak_energy} is not allowed"
            ),
            Self::ExplosionOutOfReach(position) => {
                write!(
                    formatter,
                    "explosion target {position:?} is outside player reach"
                )
            }
            Self::InvalidBuildMaterial(material) => {
                write!(formatter, "material {material:?} cannot be placed")
            }
            Self::BuildCoordinateOutOfRange(position) => {
                write!(
                    formatter,
                    "build coordinate {position:?} exceeds the world bound"
                )
            }
            Self::BuildPositionOccupied(position) => {
                write!(formatter, "build position {position:?} is already occupied")
            }
            Self::BuildPositionUnsupported(position) => {
                write!(
                    formatter,
                    "build position {position:?} has no static face support"
                )
            }
            Self::BuildOutOfReach(position) => {
                write!(
                    formatter,
                    "build position {position:?} is outside player reach"
                )
            }
            Self::BuildOccluded(position) => {
                write!(formatter, "build position {position:?} is occluded")
            }
            Self::BuildOverlapsPlayer(position) => {
                write!(formatter, "build position {position:?} overlaps the player")
            }
            Self::MissingAuthoritativePlayer(client_id) => {
                write!(
                    formatter,
                    "client {client_id} has no authoritative player state"
                )
            }
            Self::InvalidPlayerBuildContext(client_id) => {
                write!(
                    formatter,
                    "client {client_id} has an invalid player build context"
                )
            }
            Self::BuildOverlapsBody { position, body_id } => write!(
                formatter,
                "build position {position:?} overlaps rigid body {body_id}"
            ),
            Self::InsufficientConstructionUnits {
                available,
                required,
            } => write!(
                formatter,
                "construction requires {required} units but only {available} remain"
            ),
            Self::ReplayedCommand {
                client_id,
                command_id,
            } => write!(
                formatter,
                "replayed command {command_id} from client {client_id}"
            ),
            Self::Structural(error) => error.fmt(formatter),
            Self::Body(error) => error.fmt(formatter),
            Self::TooManyActiveBodies(count) => {
                write!(formatter, "active rigid-body count would reach {count}")
            }
            Self::TooManyActiveBodyVoxels(count) => {
                write!(formatter, "active rigid-body voxels would reach {count}")
            }
            Self::DuplicateBodyId(id) => write!(formatter, "duplicate rigid-body id {id}"),
            Self::BodyIdExhausted => write!(formatter, "rigid-body entity ID space is exhausted"),
            Self::TransactionTooLarge {
                changes,
                body_assignments,
            } => write!(
                formatter,
                "authoritative transaction is too large: {changes} changes and {body_assignments} body assignments"
            ),
        }
    }
}

impl std::error::Error for CommandError {}

impl From<StructuralError> for CommandError {
    fn from(value: StructuralError) -> Self {
        Self::Structural(value)
    }
}

impl From<BodyError> for CommandError {
    fn from(value: BodyError) -> Self {
        Self::Body(value)
    }
}

impl AuthoritativeServer {
    #[must_use]
    pub fn new(world: World) -> Self {
        Self {
            world,
            bodies: BTreeMap::new(),
            body_states: BTreeMap::new(),
            body_fingerprint: 0,
            active_body_voxels: 0,
            structural_anchors: StructuralAnchors::foundation_plane(0),
            structural_context: crate::structural_jobs::StructuralContext::default(),
            structural_limits: StructuralLimits::default(),
            body_limits: BodyLimits::default(),
            next_body_id: 1,
            next_sequence: 1,
            last_command_id: HashMap::new(),
            construction_units: HashMap::new(),
        }
    }

    #[must_use]
    pub fn with_structural_config(
        mut self,
        anchors: StructuralAnchors,
        structural_limits: StructuralLimits,
        body_limits: BodyLimits,
    ) -> Self {
        self.structural_anchors = anchors;
        self.structural_context.invalidate();
        self.structural_limits = structural_limits;
        self.body_limits = body_limits;
        self
    }

    /// Enables explicit elastic parameters for server-selected structural analysis jobs.
    /// Does not activate an automatic fracture law or change any replicated voxel state.
    #[must_use]
    pub fn with_structural_materials(
        mut self,
        materials: crate::structural_jobs::StructuralMaterials,
    ) -> Self {
        self.structural_context.configure(materials);
        self
    }

    pub(crate) const fn structural_anchors(&self) -> &StructuralAnchors {
        &self.structural_anchors
    }

    /// Revalidates the originating authority, material/anchor configuration and every read chunk.
    /// The returned borrow prevents mutation through this authority while the result is inspected.
    ///
    /// # Errors
    /// Rejects stale or foreign results and propagates explicit domain/solver failure. This method
    /// does not commit damage; a future fracture commit must repeat these checks atomically.
    pub fn structural_result<'a>(
        &'a self,
        completed: &'a crate::structural_jobs::CompletedStructuralJob,
    ) -> Result<&'a crate::elasticity::ElasticSolution, crate::structural_jobs::StructuralJobError>
    {
        self.structural_context.validate(&self.world, completed)
    }

    /// Validates and applies one client request as an authoritative transaction.
    ///
    /// # Errors
    ///
    /// Rejects unsafe blast bounds and replayed or out-of-order client command identifiers.
    pub fn execute_explosion(
        &mut self,
        client_id: u64,
        command: ExplosionCommand,
    ) -> Result<(DeltaPacket, DestructionReport), CommandError> {
        validate_command(command)?;
        self.validate_fresh_command(client_id, command.command_id)?;

        let base_fingerprint = self.world.fingerprint();
        let base_body_fingerprint = self.body_fingerprint;
        let mut report = self.world.apply_explosion(Explosion {
            center: command.center,
            radius_voxels: command.radius_voxels,
            peak_energy: command.peak_energy,
        });
        let bodies = match self.prepare_detached_bodies(&report.changes) {
            Ok(bodies) => bodies,
            Err(error) => {
                rollback_changes(&mut self.world, &report.changes);
                return Err(error);
            }
        };
        let spawned_voxels = bodies.iter().map(|body| body.voxels.len()).sum::<usize>();
        let Ok(spawned_body_count) = BodyId::try_from(bodies.len()) else {
            rollback_changes(&mut self.world, &report.changes);
            return Err(CommandError::BodyIdExhausted);
        };
        let Some(next_body_id) = self.next_body_id.checked_add(spawned_body_count) else {
            rollback_changes(&mut self.world, &report.changes);
            return Err(CommandError::BodyIdExhausted);
        };
        let active_body_count = self.bodies.len().saturating_add(bodies.len());
        let active_body_voxels = self.active_body_voxels.saturating_add(spawned_voxels);
        if active_body_count > MAX_ACTIVE_BODIES {
            rollback_changes(&mut self.world, &report.changes);
            return Err(CommandError::TooManyActiveBodies(active_body_count));
        }
        if active_body_voxels > MAX_ACTIVE_BODY_VOXELS {
            rollback_changes(&mut self.world, &report.changes);
            return Err(CommandError::TooManyActiveBodyVoxels(active_body_voxels));
        }
        let (changes, body_assignments) = merged_detachment_changes(&self.world, &report, &bodies);
        let (spawned_states, body_updates) = initial_blast_states(&bodies, command);
        if !payload_fits_protocol(
            changes.len(),
            body_assignments.len(),
            body_updates.len(),
            MAX_DATAGRAM_BYTES,
        ) {
            rollback_changes(&mut self.world, &report.changes);
            return Err(CommandError::TransactionTooLarge {
                changes: changes.len(),
                body_assignments: body_assignments.len(),
            });
        }
        for assignment in &body_assignments {
            self.world.set_voxel(assignment.position, Voxel::AIR);
        }
        report.detached_voxels = spawned_voxels;
        report.changes = changes;
        for (body, state) in bodies.into_iter().zip(spawned_states) {
            self.body_fingerprint ^= body_fingerprint_token(&body, state);
            self.body_states.insert(body.id, state);
            self.bodies.insert(body.id, body);
        }
        self.next_body_id = next_body_id;
        self.active_body_voxels = active_body_voxels;
        let tick = self.world.tick().wrapping_add(1);
        self.world.set_tick(tick);
        let packet = DeltaPacket {
            sequence: self.next_sequence,
            tick,
            base_fingerprint,
            final_fingerprint: self.world.fingerprint(),
            base_body_fingerprint,
            final_body_fingerprint: self.body_fingerprint,
            changes: report.changes.clone(),
            body_assignments,
            body_updates,
        };
        self.next_sequence = self.next_sequence.wrapping_add(1);
        self.last_command_id.insert(client_id, command.command_id);
        Ok((packet, report))
    }

    /// Validates and commits one resource-backed static voxel placement.
    ///
    /// # Errors
    ///
    /// Rejects replay, air, extreme coordinates, occupied or unsupported targets, conservative
    /// rigid-body overlap, and exhausted per-client construction units before mutating the world.
    pub fn execute_build(
        &mut self,
        client_id: u64,
        command: BuildCommand,
        player: PlayerBuildContext,
    ) -> Result<(DeltaPacket, BuildReport), CommandError> {
        self.execute_build_with_players(client_id, command, player, &[player])
    }

    pub(crate) fn execute_build_with_players(
        &mut self,
        client_id: u64,
        command: BuildCommand,
        player: PlayerBuildContext,
        players: &[PlayerBuildContext],
    ) -> Result<(DeltaPacket, BuildReport), CommandError> {
        if !player.is_canonical()
            || !players.contains(&player)
            || players.iter().any(|candidate| !candidate.is_canonical())
        {
            return Err(CommandError::InvalidPlayerBuildContext(client_id));
        }
        self.validate_fresh_command(client_id, command.command_id)?;
        let cost = build_material_cost(command.material)
            .ok_or(CommandError::InvalidBuildMaterial(command.material))?;
        if [command.position.x, command.position.y, command.position.z]
            .iter()
            .any(|coordinate| coordinate.unsigned_abs() > MAX_BUILD_COORDINATE.cast_unsigned())
        {
            return Err(CommandError::BuildCoordinateOutOfRange(command.position));
        }
        if self.world.voxel(command.position).is_solid() {
            return Err(CommandError::BuildPositionOccupied(command.position));
        }
        if !BUILD_NEIGHBORS.iter().any(|offset| {
            self.world
                .voxel(saturating_position_add(command.position, *offset))
                .is_solid()
        }) {
            return Err(CommandError::BuildPositionUnsupported(command.position));
        }
        if !build_is_in_reach(player.eye_position_um, command.position) {
            return Err(CommandError::BuildOutOfReach(command.position));
        }
        if players.iter().any(|candidate| {
            voxel_overlaps_bounds(
                command.position,
                candidate.bounds_minimum_um,
                candidate.bounds_maximum_um,
            )
        }) {
            return Err(CommandError::BuildOverlapsPlayer(command.position));
        }
        if !build_has_line_of_sight(&self.world, player.eye_position_um, command.position) {
            return Err(CommandError::BuildOccluded(command.position));
        }
        if let Some(body_id) = self.bodies.iter().find_map(|(&body_id, body)| {
            self.body_states
                .get(&body_id)
                .copied()
                .filter(|state| body_overlaps_voxel(body, *state, command.position))
                .map(|_| body_id)
        }) {
            return Err(CommandError::BuildOverlapsBody {
                position: command.position,
                body_id,
            });
        }
        let available = self.construction_units(client_id);
        if available < cost {
            return Err(CommandError::InsufficientConstructionUnits {
                available,
                required: cost,
            });
        }
        let remaining_units = available - cost;
        let before = Voxel::AIR;
        let after = Voxel::new(command.material);
        let base_fingerprint = self.world.fingerprint();
        self.world.set_voxel(command.position, after);
        let tick = self.world.tick().wrapping_add(1);
        self.world.set_tick(tick);
        let packet = DeltaPacket {
            sequence: self.next_sequence,
            tick,
            base_fingerprint,
            final_fingerprint: self.world.fingerprint(),
            base_body_fingerprint: self.body_fingerprint,
            final_body_fingerprint: self.body_fingerprint,
            changes: vec![VoxelChange {
                position: command.position,
                before,
                after,
            }],
            body_assignments: Vec::new(),
            body_updates: Vec::new(),
        };
        self.next_sequence = self.next_sequence.wrapping_add(1);
        self.last_command_id.insert(client_id, command.command_id);
        self.construction_units.insert(client_id, remaining_units);
        Ok((
            packet,
            BuildReport {
                position: command.position,
                material: command.material,
                spent_units: cost,
                remaining_units,
            },
        ))
    }

    #[must_use]
    pub fn construction_units(&self, client_id: u64) -> u32 {
        self.construction_units
            .get(&client_id)
            .copied()
            .unwrap_or(DEFAULT_CONSTRUCTION_UNITS)
    }

    /// Releases replay and ephemeral construction accounting for a transport session that can no
    /// longer submit commands. Session identifiers must never be reused by the caller.
    pub fn release_client(&mut self, client_id: u64) {
        self.last_command_id.remove(&client_id);
        self.construction_units.remove(&client_id);
    }

    #[must_use]
    pub const fn world(&self) -> &World {
        &self.world
    }

    #[must_use]
    pub const fn bodies(&self) -> &BTreeMap<BodyId, RigidBodyDescriptor> {
        &self.bodies
    }

    #[must_use]
    pub const fn body_fingerprint(&self) -> u128 {
        self.body_fingerprint
    }

    #[must_use]
    pub const fn body_states(&self) -> &BTreeMap<BodyId, RigidBodyState> {
        &self.body_states
    }

    #[must_use]
    pub const fn next_body_id(&self) -> BodyId {
        self.next_body_id
    }

    #[must_use]
    pub const fn next_sequence(&self) -> u64 {
        self.next_sequence
    }

    #[must_use]
    pub fn advance_physics(&mut self) -> (Option<DeltaPacket>, PhysicsTickReport) {
        let simulation = step_rigid_bodies(&self.world, &self.bodies, &mut self.body_states);
        let report = PhysicsTickReport {
            updated_bodies: simulation.transitions.len(),
            static_collisions: simulation.static_collisions,
            body_collisions: simulation.body_collisions,
            bodies_put_to_sleep: simulation.bodies_put_to_sleep,
            bodies_woken: simulation.bodies_woken,
            broad_phase_pairs: simulation.broad_phase_pairs,
            broad_phase_saturated: simulation.broad_phase_saturated,
        };
        let base_body_fingerprint = self.body_fingerprint;
        let mut updates = Vec::with_capacity(simulation.transitions.len());
        for transition in simulation.transitions {
            let Some(body) = self.bodies.get(&transition.body_id) else {
                continue;
            };
            self.body_fingerprint ^= body_fingerprint_token(body, transition.before)
                ^ body_fingerprint_token(body, transition.after);
            updates.push(BodyStateUpdate {
                body_id: transition.body_id,
                state: transition.after,
            });
        }
        let tick = self.world.tick().wrapping_add(1);
        self.world.set_tick(tick);
        if updates.is_empty() {
            return (None, report);
        }
        let packet = DeltaPacket {
            sequence: self.next_sequence,
            tick,
            base_fingerprint: self.world.fingerprint(),
            final_fingerprint: self.world.fingerprint(),
            base_body_fingerprint,
            final_body_fingerprint: self.body_fingerprint,
            changes: Vec::new(),
            body_assignments: Vec::new(),
            body_updates: updates,
        };
        self.next_sequence = self.next_sequence.wrapping_add(1);
        (Some(packet), report)
    }

    fn validate_fresh_command(&self, client_id: u64, command_id: u64) -> Result<(), CommandError> {
        if self
            .last_command_id
            .get(&client_id)
            .is_some_and(|last| command_id <= *last)
        {
            return Err(CommandError::ReplayedCommand {
                client_id,
                command_id,
            });
        }
        Ok(())
    }

    fn prepare_detached_bodies(
        &self,
        changes: &[VoxelChange],
    ) -> Result<Vec<RigidBodyDescriptor>, CommandError> {
        let structural = analyze_structural_changes(
            &self.world,
            changes,
            &self.structural_anchors,
            self.structural_limits,
        )?;
        let spawned_voxels = structural
            .detached_islands
            .iter()
            .map(|island| island.voxels().len())
            .sum::<usize>();
        if spawned_voxels > MAX_SPAWNED_BODY_VOXELS {
            return Err(CommandError::TransactionTooLarge {
                changes: changes.len(),
                body_assignments: spawned_voxels,
            });
        }
        let body_count = BodyId::try_from(structural.detached_islands.len())
            .map_err(|_| CommandError::BodyIdExhausted)?;
        self.next_body_id
            .checked_add(body_count)
            .ok_or(CommandError::BodyIdExhausted)?;
        let mut bodies = Vec::with_capacity(structural.detached_islands.len());
        for (offset, island) in structural.detached_islands.into_iter().enumerate() {
            let offset = BodyId::try_from(offset).map_err(|_| CommandError::BodyIdExhausted)?;
            let body_id = self
                .next_body_id
                .checked_add(offset)
                .ok_or(CommandError::BodyIdExhausted)?;
            let body = RigidBodyDescriptor::from_detached_island(
                body_id,
                &self.world,
                &island,
                self.body_limits,
            )?;
            if self.bodies.contains_key(&body.id)
                || bodies
                    .iter()
                    .any(|existing: &RigidBodyDescriptor| existing.id == body.id)
            {
                return Err(CommandError::DuplicateBodyId(body.id));
            }
            bodies.push(body);
        }
        Ok(bodies)
    }
}

fn initial_blast_states(
    bodies: &[RigidBodyDescriptor],
    command: ExplosionCommand,
) -> (Vec<RigidBodyState>, Vec<BodyStateUpdate>) {
    let states = bodies
        .iter()
        .map(|body| {
            let mut state = RigidBodyState::at_spawn(body);
            let impulse = blast_impulse(body, command);
            let application_point = blast_application_point(body, command);
            let _ = apply_impulse_at_local_point(body, &mut state, impulse, application_point);
            state
        })
        .collect::<Vec<_>>();
    let updates = bodies
        .iter()
        .zip(&states)
        .filter_map(|(body, &state)| {
            (state != RigidBodyState::at_spawn(body)).then_some(BodyStateUpdate {
                body_id: body.id,
                state,
            })
        })
        .collect();
    (states, updates)
}

fn blast_application_point(
    body: &RigidBodyDescriptor,
    command: ExplosionCommand,
) -> FixedMillimeters3 {
    let center = [
        i128::from(command.center.x)
            .saturating_mul(1_000)
            .saturating_add(500),
        i128::from(command.center.y)
            .saturating_mul(1_000)
            .saturating_add(500),
        i128::from(command.center.z)
            .saturating_mul(1_000)
            .saturating_add(500),
    ];
    let voxel = body
        .voxels
        .iter()
        .min_by_key(|body_voxel| {
            let point = [
                i128::from(body_voxel.position.x)
                    .saturating_mul(1_000)
                    .saturating_add(500),
                i128::from(body_voxel.position.y)
                    .saturating_mul(1_000)
                    .saturating_add(500),
                i128::from(body_voxel.position.z)
                    .saturating_mul(1_000)
                    .saturating_add(500),
            ];
            let distance_squared = point
                .iter()
                .zip(center)
                .fold(0_u128, |sum, (value, center)| {
                    sum.saturating_add(value.saturating_sub(center).unsigned_abs().pow(2))
                });
            (distance_squared, body_voxel.position)
        })
        .expect("rigid-body descriptors are non-empty");
    FixedMillimeters3 {
        x: i64::from(voxel.position.x.saturating_sub(body.minimum.x)) * 1_000 + 500,
        y: i64::from(voxel.position.y.saturating_sub(body.minimum.y)) * 1_000 + 500,
        z: i64::from(voxel.position.z.saturating_sub(body.minimum.z)) * 1_000 + 500,
    }
}

fn blast_impulse(
    body: &RigidBodyDescriptor,
    command: ExplosionCommand,
) -> FixedImpulseMilliNewtonSeconds3 {
    const UPWARD_BIAS_MM: i128 = 500;
    let center_mm = |coordinate: i32| {
        i128::from(coordinate)
            .saturating_mul(1_000)
            .saturating_add(500)
    };
    let direction = [
        i128::from(body.center_of_mass_mm.x).saturating_sub(center_mm(command.center.x)),
        i128::from(body.center_of_mass_mm.y)
            .saturating_sub(center_mm(command.center.y))
            .saturating_add(UPWARD_BIAS_MM),
        i128::from(body.center_of_mass_mm.z).saturating_sub(center_mm(command.center.z)),
    ];
    let normalizer = direction
        .iter()
        .fold(0_u128, |sum, value| {
            sum.saturating_add(value.unsigned_abs())
        })
        .max(1);
    let magnitude =
        u128::from(command.peak_energy).saturating_mul(u128::from(body.fragmentation_per_mille));
    let component = |value: i128| {
        let absolute = magnitude.saturating_mul(value.unsigned_abs()) / normalizer;
        let bounded = i64::try_from(absolute).unwrap_or(i64::MAX);
        bounded.saturating_mul(i64::try_from(value.signum()).unwrap_or_default())
    };
    FixedImpulseMilliNewtonSeconds3 {
        x: component(direction[0]),
        y: component(direction[1]),
        z: component(direction[2]),
    }
}

fn merged_detachment_changes(
    world: &World,
    report: &DestructionReport,
    bodies: &[RigidBodyDescriptor],
) -> (Vec<VoxelChange>, Vec<BodyVoxelAssignment>) {
    let mut changes: BTreeMap<_, _> = report
        .changes
        .iter()
        .copied()
        .map(|change| (change.position, change))
        .collect();
    let mut assignments = Vec::new();
    for body in bodies {
        for body_voxel in &body.voxels {
            changes
                .entry(body_voxel.position)
                .and_modify(|change| change.after = Voxel::AIR)
                .or_insert_with(|| VoxelChange {
                    position: body_voxel.position,
                    before: world.voxel(body_voxel.position),
                    after: Voxel::AIR,
                });
            assignments.push(BodyVoxelAssignment {
                body_id: body.id,
                position: body_voxel.position,
                voxel: body_voxel.voxel,
            });
        }
    }
    assignments.sort_unstable_by_key(|assignment| (assignment.body_id, assignment.position));
    (changes.into_values().collect(), assignments)
}

fn rollback_changes(world: &mut World, changes: &[VoxelChange]) {
    for change in changes.iter().rev() {
        debug_assert_eq!(world.voxel(change.position), change.after);
        world.set_voxel(change.position, change.before);
    }
}

fn body_fingerprint_token(body: &RigidBodyDescriptor, state: RigidBodyState) -> u128 {
    let mut token = u128::from(body.id).rotate_left(41)
        ^ body.geometry_fingerprint.rotate_left(67)
        ^ 0xa076_1d64_78bd_642f_e703_7ed1_a0b4_28db_u128;
    let values = [
        state.translation_um.x,
        state.translation_um.y,
        state.translation_um.z,
        state.linear_velocity_um_per_second.x,
        state.linear_velocity_um_per_second.y,
        state.linear_velocity_um_per_second.z,
        i64::from(state.orientation.x),
        i64::from(state.orientation.y),
        i64::from(state.orientation.z),
        i64::from(state.orientation.w),
        state.angular_velocity_mrad_per_second.x,
        state.angular_velocity_mrad_per_second.y,
        state.angular_velocity_mrad_per_second.z,
    ];
    let mut index = 0;
    while index < values.len() {
        token = token
            .rotate_left(23)
            .wrapping_add(u128::from(values[index].cast_unsigned()))
            .wrapping_mul(0x0000_0000_0100_0000_0000_0000_0000_013b_u128);
        index += 1;
    }
    let remainder = u128::from(state.integration_remainder[0])
        | (u128::from(state.integration_remainder[1]) << 8)
        | (u128::from(state.integration_remainder[2]) << 16);
    let angular_remainder = u128::from(state.angular_integration_remainder[0])
        | (u128::from(state.angular_integration_remainder[1]) << 8)
        | (u128::from(state.angular_integration_remainder[2]) << 16);
    token
        ^ remainder.rotate_left(79)
        ^ angular_remainder.rotate_left(89)
        ^ u128::from(state.sleep_ticks).rotate_left(101)
        ^ u128::from(state.sleeping).rotate_left(127)
}

fn payload_fragment_count(
    changes: usize,
    assignments: usize,
    updates: usize,
    mtu: usize,
) -> Option<usize> {
    if !(HEADER_BYTES + CHANGE_BYTES..=MAX_DATAGRAM_BYTES).contains(&mtu) {
        return None;
    }
    let changes_per_frame = (mtu - HEADER_BYTES) / CHANGE_BYTES;
    let assignments_per_frame = (mtu - HEADER_BYTES) / BODY_ASSIGNMENT_BYTES;
    let updates_per_frame = (mtu - HEADER_BYTES) / BODY_UPDATE_BYTES;
    if changes > 0 && changes_per_frame == 0
        || assignments > 0 && assignments_per_frame == 0
        || updates > 0 && updates_per_frame == 0
    {
        return None;
    }
    let change_fragments = changes.div_ceil(changes_per_frame.max(1));
    let assignment_fragments = assignments.div_ceil(assignments_per_frame.max(1));
    let update_fragments = updates.div_ceil(updates_per_frame.max(1));
    Some(
        change_fragments
            .saturating_add(assignment_fragments)
            .saturating_add(update_fragments)
            .max(1),
    )
}

fn payload_fits_protocol(changes: usize, assignments: usize, updates: usize, mtu: usize) -> bool {
    payload_fragment_count(changes, assignments, updates, mtu)
        .is_some_and(|count| count <= usize::from(MAX_FRAGMENTS))
}

const fn validate_command(command: ExplosionCommand) -> Result<(), CommandError> {
    if command.radius_voxels == 0 {
        return Err(CommandError::ZeroRadius);
    }
    if command.radius_voxels > 16 {
        return Err(CommandError::RadiusTooLarge(command.radius_voxels));
    }
    if command.peak_energy > 1_000_000 {
        return Err(CommandError::EnergyTooLarge(command.peak_energy));
    }
    Ok(())
}

const fn build_material_cost(material: crate::Material) -> Option<u32> {
    match material {
        crate::Material::Air => None,
        crate::Material::Soil | crate::Material::Wood => Some(1),
        crate::Material::Stone | crate::Material::Brick | crate::Material::Glass => Some(2),
        crate::Material::Concrete => Some(3),
        crate::Material::Steel => Some(6),
    }
}

const fn saturating_position_add(position: IVec3, offset: IVec3) -> IVec3 {
    IVec3::new(
        position.x.saturating_add(offset.x),
        position.y.saturating_add(offset.y),
        position.z.saturating_add(offset.z),
    )
}

fn build_is_in_reach(eye: FixedMicrometers3, position: IVec3) -> bool {
    let center = voxel_center_um(position);
    let delta = [
        i128::from(center.x) - i128::from(eye.x),
        i128::from(center.y) - i128::from(eye.y),
        i128::from(center.z) - i128::from(eye.z),
    ];
    let squared_distance = delta
        .into_iter()
        .map(|component| component.saturating_mul(component))
        .fold(0_i128, i128::saturating_add);
    squared_distance <= i128::from(MAX_BUILD_REACH_UM).pow(2)
}

fn voxel_overlaps_bounds(
    position: IVec3,
    minimum: FixedMicrometers3,
    maximum: FixedMicrometers3,
) -> bool {
    let voxel_minimum = [position.x, position.y, position.z]
        .map(|coordinate| i64::from(coordinate).saturating_mul(MICROMETERS_PER_VOXEL));
    let voxel_maximum = voxel_minimum.map(|value| value.saturating_add(MICROMETERS_PER_VOXEL));
    let minimum = [minimum.x, minimum.y, minimum.z];
    let maximum = [maximum.x, maximum.y, maximum.z];
    (0..3).all(|axis| minimum[axis] < voxel_maximum[axis] && voxel_minimum[axis] < maximum[axis])
}

fn build_has_line_of_sight(world: &World, eye: FixedMicrometers3, target: IVec3) -> bool {
    let Some(mut current) = fixed_position_to_voxel(eye) else {
        return false;
    };
    if current == target {
        return true;
    }
    if world.voxel(current).is_solid() {
        return false;
    }
    let end = voxel_center_um(target);
    let origin = [eye.x, eye.y, eye.z];
    let end = [end.x, end.y, end.z];
    let mut steps = [0_i32; 3];
    let mut absolute_delta = [0_u64; 3];
    let mut next_boundary_distance = [u64::MAX; 3];
    for axis in 0..3 {
        let delta = end[axis].saturating_sub(origin[axis]);
        steps[axis] = delta.signum().try_into().unwrap_or_default();
        absolute_delta[axis] = delta.unsigned_abs();
        if steps[axis] == 0 {
            continue;
        }
        let coordinate = ivec_component(current, axis);
        let boundary = if steps[axis] > 0 {
            i64::from(coordinate)
                .saturating_add(1)
                .saturating_mul(MICROMETERS_PER_VOXEL)
        } else {
            i64::from(coordinate).saturating_mul(MICROMETERS_PER_VOXEL)
        };
        next_boundary_distance[axis] = boundary.abs_diff(origin[axis]);
    }

    // Six metres can cross at most 21 voxel planes. The larger hard ceiling remains fail-closed if
    // a future reach constant changes without updating this traversal budget.
    for _ in 0..64 {
        if current == target {
            return true;
        }
        let mut tied = [false; 3];
        for axis in 0..3 {
            if absolute_delta[axis] == 0 {
                continue;
            }
            let is_minimum = (0..3).all(|other| {
                absolute_delta[other] == 0
                    || u128::from(next_boundary_distance[axis])
                        .saturating_mul(u128::from(absolute_delta[other]))
                        <= u128::from(next_boundary_distance[other])
                            .saturating_mul(u128::from(absolute_delta[axis]))
            });
            tied[axis] = is_minimum;
        }
        if !tied.into_iter().any(core::convert::identity) {
            return false;
        }

        // A ray exactly touching an edge or corner is conservatively blocked by every crossed
        // neighbor, not only by an arbitrary axis order.
        for subset in 1_u8..8 {
            if (0..3).any(|axis| subset & (1 << axis) != 0 && !tied[axis]) {
                continue;
            }
            let mut crossed = current;
            for (axis, step) in steps.iter().copied().enumerate() {
                if subset & (1 << axis) != 0 {
                    let crossed_axis = ivec_component(crossed, axis).saturating_add(step);
                    set_ivec_component(&mut crossed, axis, crossed_axis);
                }
            }
            if crossed != target && world.voxel(crossed).is_solid() {
                return false;
            }
        }
        for axis in 0..3 {
            if tied[axis] {
                let current_axis = ivec_component(current, axis).saturating_add(steps[axis]);
                set_ivec_component(&mut current, axis, current_axis);
                next_boundary_distance[axis] = next_boundary_distance[axis]
                    .saturating_add(MICROMETERS_PER_VOXEL.cast_unsigned());
            }
        }
    }
    false
}

fn voxel_center_um(position: IVec3) -> FixedMicrometers3 {
    const HALF_VOXEL: i64 = MICROMETERS_PER_VOXEL / 2;
    FixedMicrometers3 {
        x: i64::from(position.x)
            .saturating_mul(MICROMETERS_PER_VOXEL)
            .saturating_add(HALF_VOXEL),
        y: i64::from(position.y)
            .saturating_mul(MICROMETERS_PER_VOXEL)
            .saturating_add(HALF_VOXEL),
        z: i64::from(position.z)
            .saturating_mul(MICROMETERS_PER_VOXEL)
            .saturating_add(HALF_VOXEL),
    }
}

fn fixed_position_to_voxel(position: FixedMicrometers3) -> Option<IVec3> {
    Some(IVec3::new(
        i32::try_from(position.x.div_euclid(MICROMETERS_PER_VOXEL)).ok()?,
        i32::try_from(position.y.div_euclid(MICROMETERS_PER_VOXEL)).ok()?,
        i32::try_from(position.z.div_euclid(MICROMETERS_PER_VOXEL)).ok()?,
    ))
}

const fn ivec_component(vector: IVec3, axis: usize) -> i32 {
    match axis {
        0 => vector.x,
        1 => vector.y,
        _ => vector.z,
    }
}

const fn set_ivec_component(vector: &mut IVec3, axis: usize, value: i32) {
    match axis {
        0 => vector.x = value,
        1 => vector.y = value,
        _ => vector.z = value,
    }
}

fn body_overlaps_voxel(body: &RigidBodyDescriptor, state: RigidBodyState, position: IVec3) -> bool {
    let body_minimum = [
        state.translation_um.x,
        state.translation_um.y,
        state.translation_um.z,
    ];
    let body_maximum = [
        i64::from(
            body.maximum
                .x
                .saturating_sub(body.minimum.x)
                .saturating_add(1),
        )
        .saturating_mul(MICROMETERS_PER_VOXEL)
        .saturating_add(state.translation_um.x),
        i64::from(
            body.maximum
                .y
                .saturating_sub(body.minimum.y)
                .saturating_add(1),
        )
        .saturating_mul(MICROMETERS_PER_VOXEL)
        .saturating_add(state.translation_um.y),
        i64::from(
            body.maximum
                .z
                .saturating_sub(body.minimum.z)
                .saturating_add(1),
        )
        .saturating_mul(MICROMETERS_PER_VOXEL)
        .saturating_add(state.translation_um.z),
    ];
    let voxel_minimum = [position.x, position.y, position.z]
        .map(|coordinate| i64::from(coordinate).saturating_mul(MICROMETERS_PER_VOXEL));
    let voxel_maximum = voxel_minimum.map(|minimum| minimum.saturating_add(MICROMETERS_PER_VOXEL));
    (0..3).all(|axis| {
        body_minimum[axis] < voxel_maximum[axis] && voxel_minimum[axis] < body_maximum[axis]
    })
}

#[derive(Clone)]
pub struct ClientReplica {
    world: World,
    bodies: BTreeMap<BodyId, RigidBodyDescriptor>,
    body_states: BTreeMap<BodyId, RigidBodyState>,
    body_fingerprint: u128,
    active_body_voxels: usize,
    next_body_id: BodyId,
    expected_sequence: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ClientStatus {
    Applied,
    DuplicateIgnored,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ReplicationError {
    SequenceGap {
        expected: u64,
        received: u64,
    },
    BaseFingerprintMismatch {
        expected: u128,
        received: u128,
    },
    BodyFingerprintMismatch {
        expected: u128,
        received: u128,
    },
    NonCanonicalBodyAssignments,
    BodyAssignmentWithoutRemoval(IVec3),
    DuplicateBodyId(BodyId),
    NonMonotonicBodyId {
        expected: BodyId,
        received: BodyId,
    },
    BodyIdExhausted,
    NonCanonicalBodyUpdates,
    UnknownBody(BodyId),
    InvalidBodyState(BodyId),
    SnapshotBodySetMismatch,
    InvalidSnapshotBodyId {
        map_key: BodyId,
        descriptor_id: BodyId,
        next_body_id: BodyId,
    },
    InvalidSnapshotHighWaterMark(BodyId),
    InvalidSnapshotSequence(u64),
    InvalidSnapshotDescriptor(BodyId),
    SnapshotBodyOverlapsStatic {
        body_id: BodyId,
        position: IVec3,
    },
    TooManyActiveBodies(usize),
    TooManyActiveBodyVoxels(usize),
    Body(BodyError),
    World(WorldError),
}

impl fmt::Display for ReplicationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SequenceGap { expected, received } => {
                write!(
                    formatter,
                    "sequence gap: expected {expected}, received {received}"
                )
            }
            Self::BaseFingerprintMismatch { expected, received } => write!(
                formatter,
                "base fingerprint mismatch: local {expected:032x}, packet {received:032x}"
            ),
            Self::BodyFingerprintMismatch { expected, received } => write!(
                formatter,
                "body fingerprint mismatch: local {expected:032x}, packet {received:032x}"
            ),
            Self::NonCanonicalBodyAssignments => {
                write!(formatter, "body assignments are not canonically ordered")
            }
            Self::BodyAssignmentWithoutRemoval(position) => write!(
                formatter,
                "body assignment at {position:?} has no matching static-world removal"
            ),
            Self::DuplicateBodyId(id) => write!(formatter, "duplicate rigid-body id {id}"),
            Self::NonMonotonicBodyId { expected, received } => write!(
                formatter,
                "non-monotonic rigid-body ID: expected {expected}, received {received}"
            ),
            Self::BodyIdExhausted => write!(formatter, "rigid-body entity ID space is exhausted"),
            Self::NonCanonicalBodyUpdates => {
                write!(formatter, "body updates are not canonically ordered")
            }
            Self::UnknownBody(id) => write!(formatter, "body update targets unknown id {id}"),
            Self::InvalidBodyState(id) => {
                write!(formatter, "body update contains invalid state for {id}")
            }
            Self::SnapshotBodySetMismatch => {
                write!(
                    formatter,
                    "snapshot body descriptors and states do not match"
                )
            }
            Self::InvalidSnapshotBodyId {
                map_key,
                descriptor_id,
                next_body_id,
            } => write!(
                formatter,
                "invalid snapshot body ID: key {map_key}, descriptor {descriptor_id}, next {next_body_id}"
            ),
            Self::InvalidSnapshotHighWaterMark(id) => {
                write!(formatter, "invalid snapshot body ID high-water mark {id}")
            }
            Self::InvalidSnapshotSequence(sequence) => {
                write!(formatter, "invalid snapshot next sequence {sequence}")
            }
            Self::InvalidSnapshotDescriptor(id) => {
                write!(formatter, "snapshot body descriptor {id} is not canonical")
            }
            Self::SnapshotBodyOverlapsStatic { body_id, position } => write!(
                formatter,
                "snapshot body {body_id} overlaps static voxel at {position:?}"
            ),
            Self::TooManyActiveBodies(count) => {
                write!(
                    formatter,
                    "replica active rigid-body count would reach {count}"
                )
            }
            Self::TooManyActiveBodyVoxels(count) => write!(
                formatter,
                "replica active rigid-body voxels would reach {count}"
            ),
            Self::Body(error) => error.fmt(formatter),
            Self::World(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for ReplicationError {}

impl From<WorldError> for ReplicationError {
    fn from(value: WorldError) -> Self {
        Self::World(value)
    }
}

impl From<BodyError> for ReplicationError {
    fn from(value: BodyError) -> Self {
        Self::Body(value)
    }
}

impl ClientReplica {
    #[must_use]
    pub const fn new(world: World) -> Self {
        Self {
            world,
            bodies: BTreeMap::new(),
            body_states: BTreeMap::new(),
            body_fingerprint: 0,
            active_body_voxels: 0,
            next_body_id: 1,
            expected_sequence: 1,
        }
    }

    /// Applies the next complete authoritative delta.
    ///
    /// # Errors
    ///
    /// Rejects sequence gaps, a divergent base fingerprint, or an invalid world transaction.
    pub fn receive(&mut self, packet: &DeltaPacket) -> Result<ClientStatus, ReplicationError> {
        if packet.sequence < self.expected_sequence {
            return Ok(ClientStatus::DuplicateIgnored);
        }
        if packet.sequence > self.expected_sequence {
            return Err(ReplicationError::SequenceGap {
                expected: self.expected_sequence,
                received: packet.sequence,
            });
        }
        if self.world.fingerprint() != packet.base_fingerprint {
            return Err(ReplicationError::BaseFingerprintMismatch {
                expected: self.world.fingerprint(),
                received: packet.base_fingerprint,
            });
        }
        if self.body_fingerprint != packet.base_body_fingerprint {
            return Err(ReplicationError::BodyFingerprintMismatch {
                expected: self.body_fingerprint,
                received: packet.base_body_fingerprint,
            });
        }
        let bodies =
            rebuild_replicated_bodies(&packet.changes, &packet.body_assignments, &self.bodies)?;
        let next_body_id = validate_spawned_body_ids(&bodies, self.next_body_id)?;
        let active_body_count = self.bodies.len().saturating_add(bodies.len());
        let spawned_voxels = bodies.iter().map(|body| body.voxels.len()).sum::<usize>();
        let active_body_voxels = self.active_body_voxels.saturating_add(spawned_voxels);
        if active_body_count > MAX_ACTIVE_BODIES {
            return Err(ReplicationError::TooManyActiveBodies(active_body_count));
        }
        if active_body_voxels > MAX_ACTIVE_BODY_VOXELS {
            return Err(ReplicationError::TooManyActiveBodyVoxels(
                active_body_voxels,
            ));
        }
        let mut spawned_states = BTreeMap::new();
        let mut final_body_fingerprint = self.body_fingerprint;
        for body in &bodies {
            let state = RigidBodyState::at_spawn(body);
            final_body_fingerprint ^= body_fingerprint_token(body, state);
            spawned_states.insert(body.id, state);
        }
        for pair in packet.body_updates.windows(2) {
            if pair[0].body_id >= pair[1].body_id {
                return Err(ReplicationError::NonCanonicalBodyUpdates);
            }
        }
        for update in &packet.body_updates {
            if !valid_rigid_body_state(update.state) {
                return Err(ReplicationError::InvalidBodyState(update.body_id));
            }
            let previous = spawned_states
                .get(&update.body_id)
                .or_else(|| self.body_states.get(&update.body_id))
                .copied()
                .ok_or(ReplicationError::UnknownBody(update.body_id))?;
            let body = bodies
                .iter()
                .find(|body| body.id == update.body_id)
                .or_else(|| self.bodies.get(&update.body_id))
                .ok_or(ReplicationError::UnknownBody(update.body_id))?;
            final_body_fingerprint ^=
                body_fingerprint_token(body, previous) ^ body_fingerprint_token(body, update.state);
            if let Some(state) = spawned_states.get_mut(&update.body_id) {
                *state = update.state;
            }
        }
        if final_body_fingerprint != packet.final_body_fingerprint {
            return Err(ReplicationError::BodyFingerprintMismatch {
                expected: final_body_fingerprint,
                received: packet.final_body_fingerprint,
            });
        }
        self.world
            .apply_checked(&packet.changes, packet.final_fingerprint)?;
        for body in bodies {
            self.bodies.insert(body.id, body);
        }
        self.body_states.extend(spawned_states);
        for update in &packet.body_updates {
            self.body_states.insert(update.body_id, update.state);
        }
        self.body_fingerprint = final_body_fingerprint;
        self.active_body_voxels = active_body_voxels;
        self.next_body_id = next_body_id;
        self.world.set_tick(packet.tick);
        self.expected_sequence = self.expected_sequence.wrapping_add(1);
        Ok(ClientStatus::Applied)
    }

    /// Installs a server snapshot after a detected gap. The next delta is explicit to avoid
    /// accepting a stale snapshot that would silently move the client backwards.
    ///
    /// # Errors
    ///
    /// Rejects an internally inconsistent world, descriptor/state mismatch, non-canonical body,
    /// invalid dynamic state, exhausted limit, or body ID at/above the supplied high-water mark.
    pub fn install_snapshot(
        &mut self,
        world: World,
        bodies: BTreeMap<BodyId, RigidBodyDescriptor>,
        body_states: &BTreeMap<BodyId, RigidBodyState>,
        next_body_id: BodyId,
        next_sequence: u64,
    ) -> Result<(), ReplicationError> {
        validate_snapshot(&world, &bodies, body_states, next_body_id, next_sequence)?;
        let active_body_voxels = bodies.values().map(|body| body.voxels.len()).sum();
        self.body_fingerprint = bodies.iter().fold(0, |fingerprint, (&id, body)| {
            fingerprint ^ body_fingerprint_token(body, body_states[&id])
        });
        self.world = world;
        self.bodies = bodies;
        self.body_states.clone_from(body_states);
        self.active_body_voxels = active_body_voxels;
        self.next_body_id = next_body_id;
        self.expected_sequence = next_sequence;
        Ok(())
    }

    #[must_use]
    pub const fn world(&self) -> &World {
        &self.world
    }

    #[must_use]
    pub const fn bodies(&self) -> &BTreeMap<BodyId, RigidBodyDescriptor> {
        &self.bodies
    }

    #[must_use]
    pub const fn body_fingerprint(&self) -> u128 {
        self.body_fingerprint
    }

    #[must_use]
    pub const fn body_states(&self) -> &BTreeMap<BodyId, RigidBodyState> {
        &self.body_states
    }

    #[must_use]
    pub const fn next_body_id(&self) -> BodyId {
        self.next_body_id
    }
}

fn validate_snapshot(
    world: &World,
    bodies: &BTreeMap<BodyId, RigidBodyDescriptor>,
    body_states: &BTreeMap<BodyId, RigidBodyState>,
    next_body_id: BodyId,
    next_sequence: u64,
) -> Result<(), ReplicationError> {
    let recomputed = world.recompute_fingerprint();
    if world.fingerprint() != recomputed {
        return Err(ReplicationError::World(
            WorldError::FinalFingerprintMismatch {
                expected: world.fingerprint(),
                actual: recomputed,
            },
        ));
    }
    if next_body_id == 0 {
        return Err(ReplicationError::InvalidSnapshotHighWaterMark(next_body_id));
    }
    if next_sequence == 0 {
        return Err(ReplicationError::InvalidSnapshotSequence(next_sequence));
    }
    if bodies.len() > MAX_ACTIVE_BODIES || body_states.len() != bodies.len() {
        return Err(ReplicationError::SnapshotBodySetMismatch);
    }
    let active_body_voxels = bodies
        .values()
        .try_fold(0_usize, |total, body| total.checked_add(body.voxels.len()))
        .ok_or(ReplicationError::TooManyActiveBodyVoxels(usize::MAX))?;
    if active_body_voxels > MAX_ACTIVE_BODY_VOXELS {
        return Err(ReplicationError::TooManyActiveBodyVoxels(
            active_body_voxels,
        ));
    }
    for (&id, body) in bodies {
        if id == 0 || body.id != id || id >= next_body_id {
            return Err(ReplicationError::InvalidSnapshotBodyId {
                map_key: id,
                descriptor_id: body.id,
                next_body_id,
            });
        }
        let rebuilt = RigidBodyDescriptor::from_replicated_voxels(
            id,
            body.voxels.clone(),
            BodyLimits::default(),
        )?;
        if rebuilt != *body {
            return Err(ReplicationError::InvalidSnapshotDescriptor(id));
        }
        if let Some(overlap) = body
            .voxels
            .iter()
            .find(|body_voxel| world.voxel(body_voxel.position).is_solid())
        {
            return Err(ReplicationError::SnapshotBodyOverlapsStatic {
                body_id: id,
                position: overlap.position,
            });
        }
        let state = body_states
            .get(&id)
            .copied()
            .ok_or(ReplicationError::SnapshotBodySetMismatch)?;
        if !valid_rigid_body_state(state) {
            return Err(ReplicationError::InvalidBodyState(id));
        }
    }
    if body_states.keys().any(|id| !bodies.contains_key(id)) {
        return Err(ReplicationError::SnapshotBodySetMismatch);
    }
    Ok(())
}

fn validate_spawned_body_ids(
    bodies: &[RigidBodyDescriptor],
    mut next_body_id: BodyId,
) -> Result<BodyId, ReplicationError> {
    for body in bodies {
        if body.id != next_body_id {
            return Err(ReplicationError::NonMonotonicBodyId {
                expected: next_body_id,
                received: body.id,
            });
        }
        next_body_id = next_body_id
            .checked_add(1)
            .ok_or(ReplicationError::BodyIdExhausted)?;
    }
    Ok(next_body_id)
}

fn rebuild_replicated_bodies(
    changes: &[VoxelChange],
    assignments: &[BodyVoxelAssignment],
    existing: &BTreeMap<BodyId, RigidBodyDescriptor>,
) -> Result<Vec<RigidBodyDescriptor>, ReplicationError> {
    if assignments.len() > MAX_SPAWNED_BODY_VOXELS {
        return Err(ReplicationError::TooManyActiveBodyVoxels(assignments.len()));
    }
    for pair in changes.windows(2) {
        if pair[0].position >= pair[1].position {
            return Err(ReplicationError::World(
                WorldError::DuplicateOrUnsortedChange(pair[1].position),
            ));
        }
    }
    for pair in assignments.windows(2) {
        if (pair[0].body_id, pair[0].position) >= (pair[1].body_id, pair[1].position) {
            return Err(ReplicationError::NonCanonicalBodyAssignments);
        }
    }
    let changes_by_position: HashMap<_, _> = changes
        .iter()
        .map(|change| (change.position, change))
        .collect();
    let mut grouped = BTreeMap::<BodyId, Vec<BodyVoxel>>::new();
    for assignment in assignments {
        let valid_removal = changes_by_position
            .get(&assignment.position)
            .is_some_and(|change| {
                change.after == Voxel::AIR
                    && change.before.material == assignment.voxel.material
                    && change.before.integrity >= assignment.voxel.integrity
            });
        if !valid_removal {
            return Err(ReplicationError::BodyAssignmentWithoutRemoval(
                assignment.position,
            ));
        }
        grouped
            .entry(assignment.body_id)
            .or_default()
            .push(BodyVoxel {
                position: assignment.position,
                voxel: assignment.voxel,
            });
    }

    let mut bodies = Vec::with_capacity(grouped.len());
    for (id, voxels) in grouped {
        if existing.contains_key(&id) {
            return Err(ReplicationError::DuplicateBodyId(id));
        }
        bodies.push(RigidBodyDescriptor::from_replicated_voxels(
            id,
            voxels,
            BodyLimits::default(),
        )?);
    }
    Ok(bodies)
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CodecError {
    MtuTooSmall(usize),
    MtuTooLarge(usize),
    TooManyFragments(usize),
    Truncated,
    InvalidMagic,
    UnsupportedVersion(u8),
    InvalidKind(u8),
    InvalidFragmentLayout,
    InvalidLength { expected: usize, actual: usize },
    InvalidMaterial(InvalidMaterial),
    InvalidBodyState,
    InconsistentFragment,
    TooManyPendingPackets,
    TooManyPendingBytes,
}

impl fmt::Display for CodecError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl std::error::Error for CodecError {}

impl From<InvalidMaterial> for CodecError {
    fn from(value: InvalidMaterial) -> Self {
        Self::InvalidMaterial(value)
    }
}

/// Fragments a complete delta into application frames no larger than `mtu`.
///
/// # Errors
///
/// Returns an error when the MTU cannot carry one change or the packet needs too many fragments.
pub fn encode_frames(packet: &DeltaPacket, mtu: usize) -> Result<Vec<Vec<u8>>, CodecError> {
    if mtu < HEADER_BYTES + CHANGE_BYTES {
        return Err(CodecError::MtuTooSmall(mtu));
    }
    if mtu > MAX_DATAGRAM_BYTES {
        return Err(CodecError::MtuTooLarge(mtu));
    }
    let changes_per_frame = (mtu - HEADER_BYTES) / CHANGE_BYTES;
    let assignments_per_frame = (mtu - HEADER_BYTES) / BODY_ASSIGNMENT_BYTES;
    let updates_per_frame = (mtu - HEADER_BYTES) / BODY_UPDATE_BYTES;
    if !packet.body_assignments.is_empty() && assignments_per_frame == 0 {
        return Err(CodecError::MtuTooSmall(mtu));
    }
    if !packet.body_updates.is_empty() && updates_per_frame == 0 {
        return Err(CodecError::MtuTooSmall(mtu));
    }
    let change_fragments = packet.changes.len().div_ceil(changes_per_frame.max(1));
    let assignment_fragments = packet
        .body_assignments
        .len()
        .div_ceil(assignments_per_frame.max(1));
    let update_fragments = packet.body_updates.len().div_ceil(updates_per_frame.max(1));
    let fragment_count = payload_fragment_count(
        packet.changes.len(),
        packet.body_assignments.len(),
        packet.body_updates.len(),
        mtu,
    )
    .ok_or(CodecError::MtuTooSmall(mtu))?;
    if fragment_count > usize::from(MAX_FRAGMENTS) || fragment_count > usize::from(u16::MAX) {
        return Err(CodecError::TooManyFragments(fragment_count));
    }
    let fragment_count =
        u16::try_from(fragment_count).map_err(|_| CodecError::TooManyFragments(fragment_count))?;
    let mut frames = Vec::with_capacity(usize::from(fragment_count));
    for fragment_index in 0..fragment_count {
        let index = usize::from(fragment_index);
        let (changes, assignments, updates) = if index < change_fragments {
            let start = index * changes_per_frame;
            let end = (start + changes_per_frame).min(packet.changes.len());
            (&packet.changes[start..end], &[][..], &[][..])
        } else if index < change_fragments + assignment_fragments {
            let assignment_index = index.saturating_sub(change_fragments);
            let start = assignment_index * assignments_per_frame.max(1);
            let end = (start + assignments_per_frame.max(1)).min(packet.body_assignments.len());
            (&[][..], &packet.body_assignments[start..end], &[][..])
        } else {
            let update_index = index.saturating_sub(change_fragments + assignment_fragments);
            let start = update_index * updates_per_frame.max(1);
            let end = (start + updates_per_frame.max(1)).min(packet.body_updates.len());
            (&[][..], &[][..], &packet.body_updates[start..end])
        };
        debug_assert!(
            index < change_fragments + assignment_fragments + update_fragments
                || fragment_count == 1
        );
        let mut bytes = Vec::with_capacity(
            HEADER_BYTES
                + changes.len() * CHANGE_BYTES
                + assignments.len() * BODY_ASSIGNMENT_BYTES
                + updates.len() * BODY_UPDATE_BYTES,
        );
        bytes.extend_from_slice(&MAGIC);
        bytes.push(PROTOCOL_VERSION);
        bytes.push(DELTA_KIND);
        push_u64(&mut bytes, packet.sequence);
        push_u64(&mut bytes, packet.tick);
        push_u128(&mut bytes, packet.base_fingerprint);
        push_u128(&mut bytes, packet.final_fingerprint);
        push_u128(&mut bytes, packet.base_body_fingerprint);
        push_u128(&mut bytes, packet.final_body_fingerprint);
        push_u16(&mut bytes, fragment_index);
        push_u16(&mut bytes, fragment_count);
        let change_count = u16::try_from(changes.len())
            .map_err(|_| CodecError::TooManyFragments(usize::from(fragment_count)))?;
        push_u16(&mut bytes, change_count);
        let assignment_count = u16::try_from(assignments.len())
            .map_err(|_| CodecError::TooManyFragments(usize::from(fragment_count)))?;
        push_u16(&mut bytes, assignment_count);
        let update_count = u16::try_from(updates.len())
            .map_err(|_| CodecError::TooManyFragments(usize::from(fragment_count)))?;
        push_u16(&mut bytes, update_count);
        for change in changes {
            encode_change(&mut bytes, *change);
        }
        for assignment in assignments {
            encode_body_assignment(&mut bytes, *assignment);
        }
        for update in updates {
            encode_body_update(&mut bytes, *update);
        }
        debug_assert!(bytes.len() <= mtu);
        frames.push(bytes);
    }
    Ok(frames)
}

fn encode_change(bytes: &mut Vec<u8>, change: VoxelChange) {
    push_i32(bytes, change.position.x);
    push_i32(bytes, change.position.y);
    push_i32(bytes, change.position.z);
    bytes.push(change.before.material as u8);
    bytes.push(change.before.integrity);
    bytes.push(change.after.material as u8);
    bytes.push(change.after.integrity);
}

fn encode_body_assignment(bytes: &mut Vec<u8>, assignment: BodyVoxelAssignment) {
    push_u64(bytes, assignment.body_id);
    push_i32(bytes, assignment.position.x);
    push_i32(bytes, assignment.position.y);
    push_i32(bytes, assignment.position.z);
    bytes.push(assignment.voxel.material as u8);
    bytes.push(assignment.voxel.integrity);
}

/// Validates and decodes one untrusted application frame.
///
/// # Errors
///
/// Rejects malformed headers, unsupported protocol values, inconsistent lengths, and materials.
pub fn decode_frame(bytes: &[u8]) -> Result<DeltaFrame, CodecError> {
    if bytes.len() > MAX_DATAGRAM_BYTES {
        return Err(CodecError::MtuTooLarge(bytes.len()));
    }
    if bytes.len() < HEADER_BYTES {
        return Err(CodecError::Truncated);
    }
    let mut cursor = Cursor::new(bytes);
    if cursor.take_array::<4>()? != MAGIC {
        return Err(CodecError::InvalidMagic);
    }
    let version = cursor.take_u8()?;
    if version != PROTOCOL_VERSION {
        return Err(CodecError::UnsupportedVersion(version));
    }
    let kind = cursor.take_u8()?;
    if kind != DELTA_KIND {
        return Err(CodecError::InvalidKind(kind));
    }
    let sequence = cursor.take_u64()?;
    let tick = cursor.take_u64()?;
    let base_fingerprint = cursor.take_u128()?;
    let final_fingerprint = cursor.take_u128()?;
    let base_body_fingerprint = cursor.take_u128()?;
    let final_body_fingerprint = cursor.take_u128()?;
    let fragment_index = cursor.take_u16()?;
    let fragment_count = cursor.take_u16()?;
    let change_count = usize::from(cursor.take_u16()?);
    let assignment_count = usize::from(cursor.take_u16()?);
    let update_count = usize::from(cursor.take_u16()?);
    if fragment_count == 0 || fragment_count > MAX_FRAGMENTS || fragment_index >= fragment_count {
        return Err(CodecError::InvalidFragmentLayout);
    }
    let expected_length = HEADER_BYTES
        .checked_add(
            change_count
                .checked_mul(CHANGE_BYTES)
                .ok_or(CodecError::Truncated)?,
        )
        .and_then(|length| length.checked_add(assignment_count.checked_mul(BODY_ASSIGNMENT_BYTES)?))
        .and_then(|length| length.checked_add(update_count.checked_mul(BODY_UPDATE_BYTES)?))
        .ok_or(CodecError::Truncated)?;
    if bytes.len() != expected_length {
        return Err(CodecError::InvalidLength {
            expected: expected_length,
            actual: bytes.len(),
        });
    }

    let mut changes = Vec::with_capacity(change_count);
    for _ in 0..change_count {
        let position = IVec3::new(cursor.take_i32()?, cursor.take_i32()?, cursor.take_i32()?);
        let before = Voxel::from_wire(cursor.take_u8()?, cursor.take_u8()?)?;
        let after = Voxel::from_wire(cursor.take_u8()?, cursor.take_u8()?)?;
        changes.push(VoxelChange {
            position,
            before,
            after,
        });
    }
    let mut body_assignments = Vec::with_capacity(assignment_count);
    for _ in 0..assignment_count {
        let body_id = cursor.take_u64()?;
        let position = IVec3::new(cursor.take_i32()?, cursor.take_i32()?, cursor.take_i32()?);
        let voxel = Voxel::from_wire(cursor.take_u8()?, cursor.take_u8()?)?;
        body_assignments.push(BodyVoxelAssignment {
            body_id,
            position,
            voxel,
        });
    }
    let mut body_updates = Vec::with_capacity(update_count);
    for _ in 0..update_count {
        body_updates.push(decode_body_update(&mut cursor)?);
    }
    Ok(DeltaFrame {
        sequence,
        tick,
        base_fingerprint,
        final_fingerprint,
        base_body_fingerprint,
        final_body_fingerprint,
        fragment_index,
        fragment_count,
        changes,
        body_assignments,
        body_updates,
    })
}

fn encode_body_update(bytes: &mut Vec<u8>, update: BodyStateUpdate) {
    push_u64(bytes, update.body_id);
    push_i64(bytes, update.state.translation_um.x);
    push_i64(bytes, update.state.translation_um.y);
    push_i64(bytes, update.state.translation_um.z);
    push_i64(bytes, update.state.linear_velocity_um_per_second.x);
    push_i64(bytes, update.state.linear_velocity_um_per_second.y);
    push_i64(bytes, update.state.linear_velocity_um_per_second.z);
    push_i32(bytes, update.state.orientation.x);
    push_i32(bytes, update.state.orientation.y);
    push_i32(bytes, update.state.orientation.z);
    push_i32(bytes, update.state.orientation.w);
    push_i64(bytes, update.state.angular_velocity_mrad_per_second.x);
    push_i64(bytes, update.state.angular_velocity_mrad_per_second.y);
    push_i64(bytes, update.state.angular_velocity_mrad_per_second.z);
    bytes.extend_from_slice(&update.state.integration_remainder);
    bytes.extend_from_slice(&update.state.angular_integration_remainder);
    push_u16(bytes, update.state.sleep_ticks);
    bytes.push(u8::from(update.state.sleeping));
}

fn decode_body_update(cursor: &mut Cursor<'_>) -> Result<BodyStateUpdate, CodecError> {
    let body_id = cursor.take_u64()?;
    let state = RigidBodyState {
        translation_um: FixedMicrometers3 {
            x: cursor.take_i64()?,
            y: cursor.take_i64()?,
            z: cursor.take_i64()?,
        },
        linear_velocity_um_per_second: FixedMicrometers3 {
            x: cursor.take_i64()?,
            y: cursor.take_i64()?,
            z: cursor.take_i64()?,
        },
        orientation: FixedQuaternion {
            x: cursor.take_i32()?,
            y: cursor.take_i32()?,
            z: cursor.take_i32()?,
            w: cursor.take_i32()?,
        },
        angular_velocity_mrad_per_second: FixedMilliradians3 {
            x: cursor.take_i64()?,
            y: cursor.take_i64()?,
            z: cursor.take_i64()?,
        },
        integration_remainder: cursor.take_array::<3>()?,
        angular_integration_remainder: cursor.take_array::<3>()?,
        sleep_ticks: cursor.take_u16()?,
        sleeping: match cursor.take_u8()? {
            0 => false,
            1 => true,
            _ => return Err(CodecError::InvalidBodyState),
        },
    };
    if !valid_rigid_body_state(state) {
        return Err(CodecError::InvalidBodyState);
    }
    Ok(BodyStateUpdate { body_id, state })
}

#[derive(Default)]
pub struct FrameAssembler {
    pending: HashMap<u64, PendingPacket>,
    pending_bytes: usize,
}

struct PendingPacket {
    tick: u64,
    base_fingerprint: u128,
    final_fingerprint: u128,
    base_body_fingerprint: u128,
    final_body_fingerprint: u128,
    fragments: Vec<Option<FrameFragment>>,
    retained_bytes: usize,
}

#[derive(Clone, Eq, PartialEq)]
struct FrameFragment {
    changes: Vec<VoxelChange>,
    body_assignments: Vec<BodyVoxelAssignment>,
    body_updates: Vec<BodyStateUpdate>,
}

impl FrameAssembler {
    /// Adds a validated fragment and returns a packet only when every fragment is present.
    ///
    /// # Errors
    ///
    /// Rejects fragments that disagree with metadata already recorded for the same sequence.
    pub fn push(&mut self, frame: DeltaFrame) -> Result<Option<DeltaPacket>, CodecError> {
        if !self.pending.contains_key(&frame.sequence) && self.pending.len() >= MAX_PENDING_PACKETS
        {
            return Err(CodecError::TooManyPendingPackets);
        }
        if self.pending.get(&frame.sequence).is_some_and(|pending| {
            pending.tick != frame.tick
                || pending.base_fingerprint != frame.base_fingerprint
                || pending.final_fingerprint != frame.final_fingerprint
                || pending.base_body_fingerprint != frame.base_body_fingerprint
                || pending.final_body_fingerprint != frame.final_body_fingerprint
                || pending.fragments.len() != usize::from(frame.fragment_count)
        }) {
            self.remove_pending(frame.sequence);
            return Err(CodecError::InconsistentFragment);
        }
        let pending = self
            .pending
            .entry(frame.sequence)
            .or_insert_with(|| PendingPacket {
                tick: frame.tick,
                base_fingerprint: frame.base_fingerprint,
                final_fingerprint: frame.final_fingerprint,
                base_body_fingerprint: frame.base_body_fingerprint,
                final_body_fingerprint: frame.final_body_fingerprint,
                fragments: vec![None; usize::from(frame.fragment_count)],
                retained_bytes: 0,
            });
        let slot = &mut pending.fragments[usize::from(frame.fragment_index)];
        let fragment = FrameFragment {
            changes: frame.changes,
            body_assignments: frame.body_assignments,
            body_updates: frame.body_updates,
        };
        if let Some(existing) = slot {
            if existing != &fragment {
                self.remove_pending(frame.sequence);
                return Err(CodecError::InconsistentFragment);
            }
        } else {
            let retained_bytes = HEADER_BYTES
                .saturating_add(fragment.changes.len().saturating_mul(CHANGE_BYTES))
                .saturating_add(
                    fragment
                        .body_assignments
                        .len()
                        .saturating_mul(BODY_ASSIGNMENT_BYTES),
                )
                .saturating_add(
                    fragment
                        .body_updates
                        .len()
                        .saturating_mul(BODY_UPDATE_BYTES),
                );
            if self.pending_bytes.saturating_add(retained_bytes) > MAX_PENDING_BYTES {
                self.remove_pending(frame.sequence);
                return Err(CodecError::TooManyPendingBytes);
            }
            self.pending_bytes += retained_bytes;
            pending.retained_bytes += retained_bytes;
            *slot = Some(fragment);
        }
        if pending.fragments.iter().any(Option::is_none) {
            return Ok(None);
        }

        let complete = self
            .pending
            .remove(&frame.sequence)
            .ok_or(CodecError::InconsistentFragment)?;
        self.pending_bytes = self.pending_bytes.saturating_sub(complete.retained_bytes);
        let mut changes = Vec::new();
        let mut body_assignments = Vec::new();
        let mut body_updates = Vec::new();
        for fragment in complete.fragments.into_iter().flatten() {
            changes.extend(fragment.changes);
            body_assignments.extend(fragment.body_assignments);
            body_updates.extend(fragment.body_updates);
        }
        Ok(Some(DeltaPacket {
            sequence: frame.sequence,
            tick: complete.tick,
            base_fingerprint: complete.base_fingerprint,
            final_fingerprint: complete.final_fingerprint,
            base_body_fingerprint: complete.base_body_fingerprint,
            final_body_fingerprint: complete.final_body_fingerprint,
            changes,
            body_assignments,
            body_updates,
        }))
    }

    #[must_use]
    pub fn pending_packets(&self) -> usize {
        self.pending.len()
    }

    #[must_use]
    pub const fn pending_bytes(&self) -> usize {
        self.pending_bytes
    }

    fn remove_pending(&mut self, sequence: u64) {
        if let Some(packet) = self.pending.remove(&sequence) {
            self.pending_bytes = self.pending_bytes.saturating_sub(packet.retained_bytes);
        }
    }
}

struct Cursor<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> Cursor<'a> {
    const fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    fn take_array<const N: usize>(&mut self) -> Result<[u8; N], CodecError> {
        let end = self.offset.checked_add(N).ok_or(CodecError::Truncated)?;
        let slice = self
            .bytes
            .get(self.offset..end)
            .ok_or(CodecError::Truncated)?;
        self.offset = end;
        slice.try_into().map_err(|_| CodecError::Truncated)
    }

    fn take_u8(&mut self) -> Result<u8, CodecError> {
        Ok(self.take_array::<1>()?[0])
    }

    fn take_u16(&mut self) -> Result<u16, CodecError> {
        Ok(u16::from_le_bytes(self.take_array()?))
    }

    fn take_u64(&mut self) -> Result<u64, CodecError> {
        Ok(u64::from_le_bytes(self.take_array()?))
    }

    fn take_u128(&mut self) -> Result<u128, CodecError> {
        Ok(u128::from_le_bytes(self.take_array()?))
    }

    fn take_i32(&mut self) -> Result<i32, CodecError> {
        Ok(i32::from_le_bytes(self.take_array()?))
    }

    fn take_i64(&mut self) -> Result<i64, CodecError> {
        Ok(i64::from_le_bytes(self.take_array()?))
    }
}

fn push_u16(bytes: &mut Vec<u8>, value: u16) {
    bytes.extend_from_slice(&value.to_le_bytes());
}

fn push_u64(bytes: &mut Vec<u8>, value: u64) {
    bytes.extend_from_slice(&value.to_le_bytes());
}

fn push_u128(bytes: &mut Vec<u8>, value: u128) {
    bytes.extend_from_slice(&value.to_le_bytes());
}

fn push_i32(bytes: &mut Vec<u8>, value: i32) {
    bytes.extend_from_slice(&value.to_le_bytes());
}

fn push_i64(bytes: &mut Vec<u8>, value: i64) {
    bytes.extend_from_slice(&value.to_le_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Material;

    const fn nearby_player_context() -> PlayerBuildContext {
        PlayerBuildContext {
            eye_position_um: FixedMicrometers3 {
                x: 500_000,
                y: 2_650_000,
                z: 3_500_000,
            },
            bounds_minimum_um: FixedMicrometers3 {
                x: 200_000,
                y: 1_000_000,
                z: 3_200_000,
            },
            bounds_maximum_um: FixedMicrometers3 {
                x: 800_000,
                y: 2_800_000,
                z: 3_800_000,
            },
        }
    }

    #[test]
    fn snapshot_validation_is_atomic_and_fail_closed() {
        let mut snapshot_world = World::default();
        let position = IVec3::new(0, 4, 0);
        snapshot_world.set_voxel(position, Voxel::new(Material::Wood));
        let body = RigidBodyDescriptor::from_world_voxels(
            1,
            &snapshot_world,
            vec![position],
            BodyLimits::default(),
        )
        .expect("snapshot body");
        snapshot_world.set_voxel(position, Voxel::AIR);
        let state = RigidBodyState::at_spawn(&body);
        let bodies = BTreeMap::from([(1, body.clone())]);
        let states = BTreeMap::from([(1, state)]);
        let mut client = ClientReplica::new(World::default());

        assert_eq!(
            client.install_snapshot(
                snapshot_world.clone(),
                bodies.clone(),
                &BTreeMap::new(),
                2,
                7
            ),
            Err(ReplicationError::SnapshotBodySetMismatch)
        );
        assert!(client.bodies().is_empty());
        assert_eq!(client.next_body_id(), 1);

        let mut overlapping_world = snapshot_world.clone();
        overlapping_world.set_voxel(position, Voxel::new(Material::Wood));
        assert_eq!(
            client.install_snapshot(overlapping_world, bodies.clone(), &states, 2, 7),
            Err(ReplicationError::SnapshotBodyOverlapsStatic {
                body_id: 1,
                position,
            })
        );
        assert!(client.bodies().is_empty());

        let mut forged = body;
        forged.geometry_fingerprint ^= 1;
        assert_eq!(
            client.install_snapshot(
                snapshot_world.clone(),
                BTreeMap::from([(1, forged)]),
                &states,
                2,
                7,
            ),
            Err(ReplicationError::InvalidSnapshotDescriptor(1))
        );
        assert!(client.bodies().is_empty());

        let mut displaced_states = states.clone();
        displaced_states
            .get_mut(&1)
            .expect("snapshot state")
            .translation_um
            .x += 1;
        let mut moving_client = ClientReplica::new(World::default());
        assert_eq!(
            moving_client.install_snapshot(
                snapshot_world.clone(),
                bodies.clone(),
                &displaced_states,
                2,
                7,
            ),
            Ok(())
        );
        assert_eq!(moving_client.body_states(), &displaced_states);

        let mut invalid_states = states.clone();
        invalid_states
            .get_mut(&1)
            .expect("snapshot state")
            .translation_um
            .x = i64::MAX;
        assert_eq!(
            client.install_snapshot(
                snapshot_world.clone(),
                bodies.clone(),
                &invalid_states,
                2,
                7,
            ),
            Err(ReplicationError::InvalidBodyState(1))
        );
        assert!(client.bodies().is_empty());

        assert_eq!(
            client.install_snapshot(snapshot_world, bodies, &states, 2, 7),
            Ok(())
        );
        assert_eq!(client.bodies().len(), 1);
        assert_eq!(client.next_body_id(), 2);
    }

    #[test]
    fn supported_build_is_atomic_resource_backed_and_replay_protected() {
        let mut world = World::default();
        world.set_voxel(IVec3::new(0, 0, 0), Voxel::new(Material::Stone));
        let mut server = AuthoritativeServer::new(world);
        let command = BuildCommand {
            command_id: 1,
            position: IVec3::new(0, 1, 0),
            material: Material::Wood,
        };

        let (packet, report) = server
            .execute_build(7, command, nearby_player_context())
            .expect("supported build");

        assert_eq!(packet.changes.len(), 1);
        assert_eq!(packet.changes[0].before, Voxel::AIR);
        assert_eq!(packet.changes[0].after, Voxel::new(Material::Wood));
        assert_eq!(report.spent_units, 1);
        assert_eq!(report.remaining_units, DEFAULT_CONSTRUCTION_UNITS - 1);
        assert_eq!(server.construction_units(7), DEFAULT_CONSTRUCTION_UNITS - 1);
        assert_eq!(
            server.world().voxel(command.position),
            Voxel::new(Material::Wood)
        );
        assert_eq!(
            server.execute_explosion(
                7,
                ExplosionCommand {
                    command_id: 1,
                    center: IVec3::default(),
                    radius_voxels: 1,
                    peak_energy: 1,
                },
            ),
            Err(CommandError::ReplayedCommand {
                client_id: 7,
                command_id: 1,
            })
        );
        server.release_client(7);
        assert_eq!(server.construction_units(7), DEFAULT_CONSTRUCTION_UNITS);
    }

    #[test]
    fn invalid_builds_leave_world_sequence_and_resources_unchanged() {
        let mut world = World::default();
        world.set_voxel(IVec3::new(0, 0, 0), Voxel::new(Material::Stone));
        let initial_fingerprint = world.fingerprint();
        let mut server = AuthoritativeServer::new(world);
        for (command, expected) in [
            (
                BuildCommand {
                    command_id: 1,
                    position: IVec3::new(0, 1, 0),
                    material: Material::Air,
                },
                CommandError::InvalidBuildMaterial(Material::Air),
            ),
            (
                BuildCommand {
                    command_id: 2,
                    position: IVec3::new(MAX_BUILD_COORDINATE + 1, 0, 0),
                    material: Material::Wood,
                },
                CommandError::BuildCoordinateOutOfRange(IVec3::new(MAX_BUILD_COORDINATE + 1, 0, 0)),
            ),
            (
                BuildCommand {
                    command_id: 3,
                    position: IVec3::new(4, 4, 4),
                    material: Material::Wood,
                },
                CommandError::BuildPositionUnsupported(IVec3::new(4, 4, 4)),
            ),
            (
                BuildCommand {
                    command_id: 4,
                    position: IVec3::new(0, 0, 0),
                    material: Material::Wood,
                },
                CommandError::BuildPositionOccupied(IVec3::new(0, 0, 0)),
            ),
        ] {
            assert_eq!(
                server.execute_build(9, command, nearby_player_context()),
                Err(expected)
            );
        }
        server.construction_units.insert(10, 1);
        assert_eq!(
            server.execute_build(
                10,
                BuildCommand {
                    command_id: 1,
                    position: IVec3::new(0, 1, 0),
                    material: Material::Brick,
                },
                nearby_player_context(),
            ),
            Err(CommandError::InsufficientConstructionUnits {
                available: 1,
                required: 2,
            })
        );
        assert_eq!(server.world().fingerprint(), initial_fingerprint);
        assert_eq!(server.world().tick(), 0);
        assert_eq!(server.next_sequence(), 1);
        assert_eq!(server.construction_units(9), DEFAULT_CONSTRUCTION_UNITS);
    }

    #[test]
    fn build_rejects_conservative_overlap_with_a_dynamic_body() {
        let mut world = World::default();
        world.set_voxel(IVec3::new(0, 0, 0), Voxel::new(Material::Stone));
        let mut server = AuthoritativeServer::new(world);
        let body = RigidBodyDescriptor::from_replicated_voxels(
            1,
            vec![BodyVoxel {
                position: IVec3::new(0, 1, 0),
                voxel: Voxel::new(Material::Wood),
            }],
            BodyLimits::default(),
        )
        .expect("body fixture");
        server
            .body_states
            .insert(1, RigidBodyState::at_spawn(&body));
        server.bodies.insert(1, body);

        assert_eq!(
            server.execute_build(
                3,
                BuildCommand {
                    command_id: 1,
                    position: IVec3::new(0, 1, 0),
                    material: Material::Wood,
                },
                nearby_player_context(),
            ),
            Err(CommandError::BuildOverlapsBody {
                position: IVec3::new(0, 1, 0),
                body_id: 1,
            })
        );
        assert_eq!(server.world().voxel(IVec3::new(0, 1, 0)), Voxel::AIR);
    }

    #[test]
    fn build_rejects_distance_occlusion_and_player_overlap_before_commit() {
        let mut world = World::default();
        world.set_voxel(IVec3::new(0, 0, 0), Voxel::new(Material::Stone));
        world.set_voxel(IVec3::new(0, 2, 2), Voxel::new(Material::Steel));
        let initial_fingerprint = world.fingerprint();
        let mut server = AuthoritativeServer::new(world);
        let command = BuildCommand {
            command_id: 1,
            position: IVec3::new(0, 1, 0),
            material: Material::Wood,
        };
        let mut invalid_context = nearby_player_context();
        invalid_context.bounds_maximum_um.x += 1;
        assert_eq!(
            server.execute_build(1, command, invalid_context),
            Err(CommandError::InvalidPlayerBuildContext(1))
        );
        assert_eq!(
            server.execute_build(1, command, nearby_player_context()),
            Err(CommandError::BuildOccluded(command.position))
        );
        let mut distant = nearby_player_context();
        distant.eye_position_um.z = 20 * MICROMETERS_PER_VOXEL;
        distant.bounds_minimum_um.z = 20 * MICROMETERS_PER_VOXEL - 300_000;
        distant.bounds_maximum_um.z = 20 * MICROMETERS_PER_VOXEL + 300_000;
        assert_eq!(
            server.execute_build(1, command, distant),
            Err(CommandError::BuildOutOfReach(command.position))
        );
        let overlapping = PlayerBuildContext {
            eye_position_um: FixedMicrometers3 {
                x: 500_000,
                y: 2_650_000,
                z: 500_000,
            },
            bounds_minimum_um: FixedMicrometers3 {
                x: 200_000,
                y: 1_000_000,
                z: 200_000,
            },
            bounds_maximum_um: FixedMicrometers3 {
                x: 800_000,
                y: 2_800_000,
                z: 800_000,
            },
        };
        assert_eq!(
            server.execute_build(1, command, overlapping),
            Err(CommandError::BuildOverlapsPlayer(command.position))
        );
        assert_eq!(server.world().fingerprint(), initial_fingerprint);
        assert_eq!(server.next_sequence(), 1);
    }

    #[test]
    fn line_of_sight_conservatively_checks_voxel_corner_ties() {
        let mut world = World::default();
        world.set_voxel(IVec3::new(1, 0, 0), Voxel::new(Material::Steel));
        let eye = FixedMicrometers3 {
            x: 500_000,
            y: 500_000,
            z: 500_000,
        };

        assert!(!build_has_line_of_sight(&world, eye, IVec3::new(2, 2, 0)));
        world.set_voxel(IVec3::new(1, 0, 0), Voxel::AIR);
        assert!(build_has_line_of_sight(&world, eye, IVec3::new(2, 2, 0)));
    }

    #[test]
    fn build_rejects_overlap_with_another_authoritative_player() {
        let mut world = World::default();
        world.set_voxel(IVec3::new(0, 0, 0), Voxel::new(Material::Stone));
        let mut server = AuthoritativeServer::new(world);
        let acting = nearby_player_context();
        let other = PlayerBuildContext {
            eye_position_um: FixedMicrometers3 {
                x: 500_000,
                y: 2_650_000,
                z: 500_000,
            },
            bounds_minimum_um: FixedMicrometers3 {
                x: 200_000,
                y: 1_000_000,
                z: 200_000,
            },
            bounds_maximum_um: FixedMicrometers3 {
                x: 800_000,
                y: 2_800_000,
                z: 800_000,
            },
        };
        let command = BuildCommand {
            command_id: 1,
            position: IVec3::new(0, 1, 0),
            material: Material::Wood,
        };

        assert_eq!(
            server.execute_build_with_players(1, command, acting, &[acting, other]),
            Err(CommandError::BuildOverlapsPlayer(command.position))
        );
        assert_eq!(server.world().voxel(command.position), Voxel::AIR);
    }

    #[test]
    fn exhausted_body_id_space_rolls_back_before_commit() {
        let mut world = World::default();
        world.set_voxel(IVec3::new(0, 0, 0), Voxel::new(Material::Steel));
        world.set_voxel(IVec3::new(0, 1, 0), Voxel::new(Material::Glass));
        world.fill_box(
            IVec3::new(0, 2, 0),
            IVec3::new(0, 3, 0),
            Voxel::new(Material::Wood),
        );
        let expected_fingerprint = world.fingerprint();
        let mut server = AuthoritativeServer::new(world);
        server.next_body_id = BodyId::MAX;

        let result = server.execute_explosion(
            1,
            ExplosionCommand {
                command_id: 1,
                center: IVec3::new(0, 1, 0),
                radius_voxels: 1,
                peak_energy: 600,
            },
        );

        assert_eq!(result, Err(CommandError::BodyIdExhausted));
        assert_eq!(server.world().fingerprint(), expected_fingerprint);
        assert_eq!(
            server.world().voxel(IVec3::new(0, 1, 0)),
            Voxel::new(Material::Glass)
        );
        assert!(server.bodies().is_empty());
        assert_eq!(server.next_body_id(), BodyId::MAX);
    }
}
