//! Fixed-unit authoritative rigid-body descriptors derived from detached voxel islands.

use crate::{
    DetachedIsland, IVec3, Voxel, World,
    structural::{ISLAND_FINGERPRINT_SEED, describe_island, mix_island_fingerprint},
};
use core::fmt;
use std::collections::{BTreeMap, BTreeSet, HashSet, VecDeque};

const MILLIMETERS_PER_VOXEL: i64 = 1_000;
const SQUARE_MILLIMETERS_PER_VOXEL: u128 = 1_000_000;
pub const MICROMETERS_PER_VOXEL: i64 = 1_000_000;
pub const SERVER_PHYSICS_HZ: i64 = 60;
const GRAVITY_UM_PER_SECOND_SQUARED: i64 = -9_810_000;
const MAX_LINEAR_SPEED_UM_PER_SECOND: i64 = 250_000_000;
const MAX_WORLD_TRANSLATION_UM: i64 = 3_000_000_000_000_000;
const SLEEP_TICKS: u16 = 30;
pub const MAX_BROAD_PHASE_PAIRS: usize = 8_192;
pub type BodyId = u64;
const BODY_NEIGHBORS: [IVec3; 6] = [
    IVec3::new(-1, 0, 0),
    IVec3::new(1, 0, 0),
    IVec3::new(0, -1, 0),
    IVec3::new(0, 1, 0),
    IVec3::new(0, 0, -1),
    IVec3::new(0, 0, 1),
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BodyLimits {
    pub max_voxels: usize,
}

impl Default for BodyLimits {
    fn default() -> Self {
        Self { max_voxels: 16_384 }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct FixedMillimeters3 {
    pub x: i64,
    pub y: i64,
    pub z: i64,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct FixedMicrometers3 {
    pub x: i64,
    pub y: i64,
    pub z: i64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RigidBodyState {
    /// Absolute world translation of the body's local-space minimum corner.
    pub translation_um: FixedMicrometers3,
    pub linear_velocity_um_per_second: FixedMicrometers3,
    /// Euclidean remainders retained when velocity is divided by the fixed 60 Hz rate.
    pub integration_remainder: [u8; 3],
    pub sleep_ticks: u16,
    pub sleeping: bool,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct BodyStepResult {
    pub moved: bool,
    pub collided_with_static: bool,
    pub became_sleeping: bool,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct BroadPhaseResult {
    pub pairs: Vec<(BodyId, BodyId)>,
    pub saturated: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BodyStateTransition {
    pub body_id: BodyId,
    pub before: RigidBodyState,
    pub after: RigidBodyState,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct BodySimulationReport {
    pub transitions: Vec<BodyStateTransition>,
    pub static_collisions: usize,
    pub body_collisions: usize,
    pub bodies_put_to_sleep: usize,
    pub bodies_woken: usize,
    pub broad_phase_pairs: usize,
    /// The complete tentative tick was discarded when the pair budget was exceeded.
    pub broad_phase_saturated: bool,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct InertiaDiagonalKgMm2 {
    pub x: u128,
    pub y: u128,
    pub z: u128,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BodyVoxel {
    pub position: IVec3,
    pub voxel: Voxel,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RigidBodyDescriptor {
    pub id: BodyId,
    /// Content identity derived independently from the monotonic runtime entity ID.
    pub geometry_fingerprint: u128,
    pub voxels: Vec<BodyVoxel>,
    pub minimum: IVec3,
    pub maximum: IVec3,
    pub mass_kg: u64,
    pub center_of_mass_mm: FixedMillimeters3,
    pub inertia_diagonal_kg_mm2: InertiaDiagonalKgMm2,
    /// Lowest occupied voxel in each local X/Z column, ordered by world position.
    pub collision_bottom: Vec<IVec3>,
    /// Highest occupied voxel in each local X/Z column, in the same canonical column order.
    pub collision_top: Vec<IVec3>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BodyError {
    InvalidEntityId(BodyId),
    EmptyIsland,
    TooManyVoxels(usize),
    NonCanonicalVoxels(IVec3),
    NonStructuralVoxel(IVec3),
    DisconnectedVoxels,
    IslandDescriptorMismatch,
    ZeroMass,
    CenterOfMassOverflow,
}

impl fmt::Display for BodyError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidEntityId(id) => write!(formatter, "invalid rigid-body entity ID {id}"),
            Self::EmptyIsland => write!(formatter, "detached island is empty"),
            Self::TooManyVoxels(count) => write!(formatter, "rigid body has {count} voxels"),
            Self::NonCanonicalVoxels(position) => {
                write!(
                    formatter,
                    "rigid-body voxels are not canonical at {position:?}"
                )
            }
            Self::NonStructuralVoxel(position) => {
                write!(
                    formatter,
                    "rigid body contains a non-structural voxel at {position:?}"
                )
            }
            Self::DisconnectedVoxels => write!(formatter, "rigid-body voxels are disconnected"),
            Self::IslandDescriptorMismatch => {
                write!(
                    formatter,
                    "detached island descriptor does not match its world voxels"
                )
            }
            Self::ZeroMass => write!(formatter, "detached island has zero physical mass"),
            Self::CenterOfMassOverflow => write!(formatter, "center of mass exceeds fixed units"),
        }
    }
}

impl std::error::Error for BodyError {}

impl RigidBodyDescriptor {
    /// Builds a validated body descriptor directly from occupied world positions for tooling,
    /// authored fixtures, and deterministic importers.
    ///
    /// # Errors
    ///
    /// Rejects empty, oversized, non-canonical, disconnected, non-structural, or zero-mass input.
    pub fn from_world_voxels(
        body_id: BodyId,
        world: &World,
        voxels: Vec<IVec3>,
        limits: BodyLimits,
    ) -> Result<Self, BodyError> {
        let island = describe_island(world, voxels);
        Self::from_detached_island(body_id, world, &island, limits)
    }

    /// Revalidates and promotes one detached island into deterministic server physics state.
    ///
    /// Voxel centres are represented in integer millimetres. Inertia is the diagonal of the point
    /// mass distribution plus each voxel's solid-cube inertia, also in integer fixed units.
    ///
    /// # Errors
    ///
    /// Rejects empty, oversized, non-canonical, forged, or zero-mass island descriptors and fixed
    /// coordinate overflow.
    pub fn from_detached_island(
        body_id: BodyId,
        world: &World,
        island: &DetachedIsland,
        limits: BodyLimits,
    ) -> Result<Self, BodyError> {
        if island.voxels.is_empty() {
            return Err(BodyError::EmptyIsland);
        }
        if island.voxels.len() > limits.max_voxels {
            return Err(BodyError::TooManyVoxels(island.voxels.len()));
        }
        for pair in island.voxels.windows(2) {
            if pair[0] >= pair[1] {
                return Err(BodyError::NonCanonicalVoxels(pair[1]));
            }
        }
        let canonical = describe_island(world, island.voxels.clone());
        if canonical != *island {
            return Err(BodyError::IslandDescriptorMismatch);
        }
        if canonical.mass_kg == 0 {
            return Err(BodyError::ZeroMass);
        }

        let voxels = canonical
            .voxels
            .iter()
            .copied()
            .map(|position| BodyVoxel {
                position,
                voxel: world.voxel(position),
            })
            .collect();
        Self::from_replicated_voxels(body_id, voxels, limits)
    }

    /// Rebuilds and verifies a body from untrusted replicated voxel membership and an independent
    /// non-zero runtime entity ID.
    ///
    /// # Errors
    ///
    /// Applies the same size, canonical-order, structural-material, connectivity, and mass checks as
    /// local promotion. The derived geometry fingerprint remains independent from `body_id`.
    pub fn from_replicated_voxels(
        body_id: BodyId,
        voxels: Vec<BodyVoxel>,
        limits: BodyLimits,
    ) -> Result<Self, BodyError> {
        if body_id == 0 {
            return Err(BodyError::InvalidEntityId(body_id));
        }
        if voxels.is_empty() {
            return Err(BodyError::EmptyIsland);
        }
        if voxels.len() > limits.max_voxels {
            return Err(BodyError::TooManyVoxels(voxels.len()));
        }
        for pair in voxels.windows(2) {
            if pair[0].position >= pair[1].position {
                return Err(BodyError::NonCanonicalVoxels(pair[1].position));
            }
        }
        if let Some(invalid) = voxels.iter().find(|body_voxel| {
            !body_voxel.voxel.is_solid()
                || body_voxel.voxel.material.properties().structural_strength == 0
        }) {
            return Err(BodyError::NonStructuralVoxel(invalid.position));
        }
        if !is_connected(&voxels) {
            return Err(BodyError::DisconnectedVoxels);
        }

        let (minimum, maximum) = body_bounds(&voxels);
        let mut mass_kg = 0_u64;
        let mut fingerprint = ISLAND_FINGERPRINT_SEED;
        for body_voxel in &voxels {
            mass_kg = mass_kg.saturating_add(u64::from(
                body_voxel.voxel.material.properties().density_kg_m3,
            ));
            fingerprint =
                mix_island_fingerprint(fingerprint, body_voxel.position, body_voxel.voxel);
        }
        if mass_kg == 0 {
            return Err(BodyError::ZeroMass);
        }
        let center_of_mass_mm = center_of_mass(&voxels, mass_kg)?;
        let inertia_diagonal_kg_mm2 = inertia_diagonal(&voxels, center_of_mass_mm);
        let (collision_bottom, collision_top) = collision_surfaces(&voxels);
        Ok(Self {
            id: body_id,
            geometry_fingerprint: fingerprint,
            voxels,
            minimum,
            maximum,
            mass_kg,
            center_of_mass_mm,
            inertia_diagonal_kg_mm2,
            collision_bottom,
            collision_top,
        })
    }
}

impl RigidBodyState {
    #[must_use]
    pub fn at_spawn(body: &RigidBodyDescriptor) -> Self {
        Self {
            translation_um: FixedMicrometers3 {
                x: i64::from(body.minimum.x) * MICROMETERS_PER_VOXEL,
                y: i64::from(body.minimum.y) * MICROMETERS_PER_VOXEL,
                z: i64::from(body.minimum.z) * MICROMETERS_PER_VOXEL,
            },
            linear_velocity_um_per_second: FixedMicrometers3::default(),
            integration_remainder: [0; 3],
            sleep_ticks: 0,
            sleeping: false,
        }
    }
}

#[must_use]
pub fn valid_rigid_body_state(state: RigidBodyState) -> bool {
    let translations = [
        state.translation_um.x,
        state.translation_um.y,
        state.translation_um.z,
    ];
    let velocities = [
        state.linear_velocity_um_per_second.x,
        state.linear_velocity_um_per_second.y,
        state.linear_velocity_um_per_second.z,
    ];
    let valid_sleep = if state.sleeping {
        state.sleep_ticks == SLEEP_TICKS
            && state.linear_velocity_um_per_second == FixedMicrometers3::default()
            && state.integration_remainder == [0; 3]
    } else {
        state.sleep_ticks < SLEEP_TICKS
    };
    translations
        .iter()
        .all(|value| value.unsigned_abs() <= MAX_WORLD_TRANSLATION_UM.cast_unsigned())
        && velocities
            .iter()
            .all(|value| value.unsigned_abs() <= MAX_LINEAR_SPEED_UM_PER_SECOND.cast_unsigned())
        && state
            .integration_remainder
            .iter()
            .all(|&remainder| i64::from(remainder) < SERVER_PHYSICS_HZ)
        && state.linear_velocity_um_per_second.x == 0
        && state.linear_velocity_um_per_second.z == 0
        && state.integration_remainder[0] == 0
        && state.integration_remainder[2] == 0
        && valid_sleep
}

/// Advances one axis-aligned body with integer semi-implicit Euler integration and a swept
/// downward collision query against static voxels.
#[must_use]
pub fn step_rigid_body(
    world: &World,
    body: &RigidBodyDescriptor,
    state: &mut RigidBodyState,
) -> BodyStepResult {
    if state.sleeping {
        return BodyStepResult::default();
    }
    let previous = *state;
    state.linear_velocity_um_per_second.y = state
        .linear_velocity_um_per_second
        .y
        .saturating_add(GRAVITY_UM_PER_SECOND_SQUARED / SERVER_PHYSICS_HZ)
        .clamp(
            -MAX_LINEAR_SPEED_UM_PER_SECOND,
            MAX_LINEAR_SPEED_UM_PER_SECOND,
        );
    let (delta_y, remainder_y) = integrate_axis(
        state.linear_velocity_um_per_second.y,
        state.integration_remainder[1],
    );
    let proposed_y = state.translation_um.y.saturating_add(delta_y);
    let collision_y = static_collision_height(world, body, state.translation_um, proposed_y);
    let collided_with_static = collision_y.is_some_and(|height| proposed_y < height);
    if let Some(height) = collision_y.filter(|height| proposed_y < *height) {
        state.translation_um.y = height;
        state.linear_velocity_um_per_second.y = 0;
        state.integration_remainder[1] = 0;
        state.sleep_ticks = state.sleep_ticks.saturating_add(1);
        if state.sleep_ticks >= SLEEP_TICKS {
            state.sleeping = true;
        }
    } else {
        state.translation_um.y = proposed_y;
        state.integration_remainder[1] = remainder_y;
        state.sleep_ticks = 0;
    }
    BodyStepResult {
        moved: *state != previous,
        collided_with_static,
        became_sleeping: !previous.sleeping && state.sleeping,
    }
}

#[must_use]
pub fn broad_phase_pairs(
    bodies: &BTreeMap<BodyId, RigidBodyDescriptor>,
    states: &BTreeMap<BodyId, RigidBodyState>,
) -> BroadPhaseResult {
    broad_phase_between(bodies, states, states)
}

/// Advances all bodies as one deterministic transaction. If the complete swept broad phase does
/// not fit its pair budget, no tentative state is committed.
#[must_use]
pub fn step_rigid_bodies(
    world: &World,
    bodies: &BTreeMap<BodyId, RigidBodyDescriptor>,
    states: &mut BTreeMap<BodyId, RigidBodyState>,
) -> BodySimulationReport {
    let mut next = states.clone();
    let static_collisions = integrate_static_bodies(world, bodies, &mut next);

    let broad_phase = broad_phase_between(bodies, states, &next);
    if broad_phase.saturated {
        return BodySimulationReport {
            broad_phase_pairs: broad_phase.pairs.len(),
            broad_phase_saturated: true,
            ..BodySimulationReport::default()
        };
    }

    let adjacency = pair_adjacency(&broad_phase.pairs);
    propagate_waking(bodies, states, &mut next, &adjacency);
    let body_collisions = resolve_body_contacts(bodies, states, &mut next, &adjacency);
    let transitions = state_transitions(bodies, states, &next);
    let bodies_put_to_sleep = transitions
        .iter()
        .filter(|transition| !transition.before.sleeping && transition.after.sleeping)
        .count();
    let bodies_woken = transitions
        .iter()
        .filter(|transition| transition.before.sleeping && !transition.after.sleeping)
        .count();
    *states = next;
    BodySimulationReport {
        transitions,
        static_collisions,
        body_collisions,
        bodies_put_to_sleep,
        bodies_woken,
        broad_phase_pairs: broad_phase.pairs.len(),
        broad_phase_saturated: false,
    }
}

fn integrate_static_bodies(
    world: &World,
    bodies: &BTreeMap<BodyId, RigidBodyDescriptor>,
    states: &mut BTreeMap<BodyId, RigidBodyState>,
) -> usize {
    bodies
        .iter()
        .filter_map(|(&body_id, body)| {
            states
                .get_mut(&body_id)
                .map(|state| usize::from(step_rigid_body(world, body, state).collided_with_static))
        })
        .sum()
}

fn resolve_body_contacts(
    bodies: &BTreeMap<BodyId, RigidBodyDescriptor>,
    before: &BTreeMap<BodyId, RigidBodyState>,
    next: &mut BTreeMap<BodyId, RigidBodyState>,
    adjacency: &BTreeMap<BodyId, Vec<BodyId>>,
) -> usize {
    let mut ordered_bodies = bodies
        .keys()
        .filter_map(|&body_id| {
            next.get(&body_id)
                .map(|state| (state.translation_um.y, body_id))
        })
        .collect::<Vec<_>>();
    ordered_bodies.sort_unstable();
    let mut processed = BTreeSet::new();
    let mut body_collisions = 0_usize;
    for (_, body_id) in ordered_bodies {
        let Some(body) = bodies.get(&body_id) else {
            continue;
        };
        let Some(body_before) = before.get(&body_id).copied() else {
            continue;
        };
        let Some(tentative) = next.get(&body_id).copied() else {
            continue;
        };
        let mut contact_height = None;
        let mut stable_contact = true;
        if let Some(candidates) = adjacency.get(&body_id) {
            for &support_id in candidates {
                if !processed.contains(&support_id) {
                    continue;
                }
                let (Some(support), Some(support_before), Some(support_after)) = (
                    bodies.get(&support_id),
                    before.get(&support_id).copied(),
                    next.get(&support_id).copied(),
                ) else {
                    continue;
                };
                let Some(height) = swept_dynamic_support_origin(
                    body,
                    body_before,
                    tentative,
                    support,
                    support_before,
                    support_after,
                ) else {
                    continue;
                };
                match contact_height {
                    Some(current) if height < current => {}
                    Some(current) if height == current => {
                        stable_contact &= support_after.sleeping;
                    }
                    _ => {
                        contact_height = Some(height);
                        stable_contact = support_after.sleeping;
                    }
                }
            }
        }
        if let Some(height) = contact_height
            && let Some(state) = next.get_mut(&body_id)
        {
            settle_on_dynamic_support(state, body_before, height, stable_contact);
            body_collisions += 1;
        }
        processed.insert(body_id);
    }
    body_collisions
}

fn state_transitions(
    bodies: &BTreeMap<BodyId, RigidBodyDescriptor>,
    before: &BTreeMap<BodyId, RigidBodyState>,
    after: &BTreeMap<BodyId, RigidBodyState>,
) -> Vec<BodyStateTransition> {
    bodies
        .keys()
        .filter_map(|&body_id| {
            let before = before.get(&body_id).copied()?;
            let after = after.get(&body_id).copied()?;
            (before != after).then_some(BodyStateTransition {
                body_id,
                before,
                after,
            })
        })
        .collect()
}

fn broad_phase_between(
    bodies: &BTreeMap<BodyId, RigidBodyDescriptor>,
    before: &BTreeMap<BodyId, RigidBodyState>,
    after: &BTreeMap<BodyId, RigidBodyState>,
) -> BroadPhaseResult {
    if before.values().all(|state| state.sleeping) && after.values().all(|state| state.sleeping) {
        return BroadPhaseResult::default();
    }
    let mut entries = bodies
        .iter()
        .filter_map(|(&id, body)| {
            let before = before.get(&id).copied()?;
            let after = after.get(&id).copied()?;
            Some((
                id,
                swept_body_aabb(body, before, after),
                before.sleeping && after.sleeping,
            ))
        })
        .collect::<Vec<_>>();
    entries.sort_unstable_by_key(|(id, bounds, _sleeping)| (bounds.0.x, *id));
    let mut pairs = Vec::new();
    for (index, &(left_id, left, left_sleeping)) in entries.iter().enumerate() {
        for &(right_id, right, right_sleeping) in &entries[index + 1..] {
            if right.0.x >= left.1.x {
                break;
            }
            if left_sleeping && right_sleeping {
                continue;
            }
            if intervals_overlap_or_touch(left.0.y, left.1.y, right.0.y, right.1.y)
                && intervals_overlap(left.0.z, left.1.z, right.0.z, right.1.z)
            {
                if pairs.len() == MAX_BROAD_PHASE_PAIRS {
                    return BroadPhaseResult {
                        pairs,
                        saturated: true,
                    };
                }
                pairs.push((left_id, right_id));
            }
        }
    }
    BroadPhaseResult {
        pairs,
        saturated: false,
    }
}

fn pair_adjacency(pairs: &[(BodyId, BodyId)]) -> BTreeMap<BodyId, Vec<BodyId>> {
    let mut adjacency = BTreeMap::<BodyId, Vec<BodyId>>::new();
    for &(left, right) in pairs {
        adjacency.entry(left).or_default().push(right);
        adjacency.entry(right).or_default().push(left);
    }
    for candidates in adjacency.values_mut() {
        candidates.sort_unstable();
    }
    adjacency
}

fn propagate_waking(
    bodies: &BTreeMap<BodyId, RigidBodyDescriptor>,
    before: &BTreeMap<BodyId, RigidBodyState>,
    next: &mut BTreeMap<BodyId, RigidBodyState>,
    adjacency: &BTreeMap<BodyId, Vec<BodyId>>,
) {
    let mut moving = before
        .iter()
        .filter_map(|(&body_id, state)| (!state.sleeping).then_some(body_id))
        .collect::<BTreeSet<_>>();
    let mut queue = moving.iter().copied().collect::<VecDeque<_>>();
    while let Some(support_id) = queue.pop_front() {
        let Some(candidates) = adjacency.get(&support_id) else {
            continue;
        };
        for &body_id in candidates {
            if moving.contains(&body_id) {
                continue;
            }
            let (Some(body), Some(body_state), Some(support), Some(support_state)) = (
                bodies.get(&body_id),
                before.get(&body_id).copied(),
                bodies.get(&support_id),
                before.get(&support_id).copied(),
            ) else {
                continue;
            };
            if !body_state.sleeping || !rests_on(body, body_state, support, support_state) {
                continue;
            }
            moving.insert(body_id);
            queue.push_back(body_id);
            if let Some(state) = next.get_mut(&body_id) {
                state.sleeping = false;
                state.sleep_ticks = 0;
            }
        }
    }
}

fn settle_on_dynamic_support(
    state: &mut RigidBodyState,
    before: RigidBodyState,
    height: i64,
    stable_contact: bool,
) {
    state.translation_um.y = height;
    state.linear_velocity_um_per_second.y = 0;
    state.integration_remainder[1] = 0;
    if stable_contact {
        state.sleep_ticks = before.sleep_ticks.saturating_add(1).min(SLEEP_TICKS);
        state.sleeping = state.sleep_ticks == SLEEP_TICKS;
    } else {
        state.sleep_ticks = 0;
        state.sleeping = false;
    }
}

fn integrate_axis(velocity: i64, remainder: u8) -> (i64, u8) {
    let numerator = velocity.saturating_add(i64::from(remainder));
    let delta = numerator.div_euclid(SERVER_PHYSICS_HZ);
    let remainder = numerator.rem_euclid(SERVER_PHYSICS_HZ);
    (delta, u8::try_from(remainder).unwrap_or_default())
}

fn static_collision_height(
    world: &World,
    body: &RigidBodyDescriptor,
    translation: FixedMicrometers3,
    proposed_y: i64,
) -> Option<i64> {
    let mut highest_origin = None;
    for bottom in &body.collision_bottom {
        let local_x = i64::from(bottom.x - body.minimum.x) * MICROMETERS_PER_VOXEL;
        let local_y = i64::from(bottom.y - body.minimum.y) * MICROMETERS_PER_VOXEL;
        let local_z = i64::from(bottom.z - body.minimum.z) * MICROMETERS_PER_VOXEL;
        let world_x = (translation.x.saturating_add(local_x)).div_euclid(MICROMETERS_PER_VOXEL);
        let world_z = (translation.z.saturating_add(local_z)).div_euclid(MICROMETERS_PER_VOXEL);
        let current_bottom = translation.y.saturating_add(local_y);
        let proposed_bottom = proposed_y.saturating_add(local_y);
        let first_y = current_bottom
            .saturating_sub(1)
            .div_euclid(MICROMETERS_PER_VOXEL);
        let last_y = proposed_bottom.div_euclid(MICROMETERS_PER_VOXEL);
        let (Ok(world_x), Ok(world_z)) = (i32::try_from(world_x), i32::try_from(world_z)) else {
            continue;
        };
        for candidate_y in (last_y..=first_y).rev() {
            let Ok(candidate_y) = i32::try_from(candidate_y) else {
                continue;
            };
            if world
                .voxel(IVec3::new(world_x, candidate_y, world_z))
                .is_solid()
            {
                let support_top =
                    i64::from(candidate_y.saturating_add(1)).saturating_mul(MICROMETERS_PER_VOXEL);
                let origin_y = support_top.saturating_sub(local_y);
                highest_origin =
                    Some(highest_origin.map_or(origin_y, |height: i64| height.max(origin_y)));
                break;
            }
        }
    }
    highest_origin
}

fn body_aabb(
    body: &RigidBodyDescriptor,
    state: RigidBodyState,
) -> (FixedMicrometers3, FixedMicrometers3) {
    let extent = |maximum: i32, minimum: i32| {
        i64::from(maximum.saturating_sub(minimum).saturating_add(1)) * MICROMETERS_PER_VOXEL
    };
    (
        state.translation_um,
        FixedMicrometers3 {
            x: state
                .translation_um
                .x
                .saturating_add(extent(body.maximum.x, body.minimum.x)),
            y: state
                .translation_um
                .y
                .saturating_add(extent(body.maximum.y, body.minimum.y)),
            z: state
                .translation_um
                .z
                .saturating_add(extent(body.maximum.z, body.minimum.z)),
        },
    )
}

fn swept_body_aabb(
    body: &RigidBodyDescriptor,
    before: RigidBodyState,
    after: RigidBodyState,
) -> (FixedMicrometers3, FixedMicrometers3) {
    let before = body_aabb(body, before);
    let after = body_aabb(body, after);
    (
        FixedMicrometers3 {
            x: before.0.x.min(after.0.x),
            y: before.0.y.min(after.0.y),
            z: before.0.z.min(after.0.z),
        },
        FixedMicrometers3 {
            x: before.1.x.max(after.1.x),
            y: before.1.y.max(after.1.y),
            z: before.1.z.max(after.1.z),
        },
    )
}

fn swept_dynamic_support_origin(
    body: &RigidBodyDescriptor,
    before: RigidBodyState,
    after: RigidBodyState,
    support: &RigidBodyDescriptor,
    support_before: RigidBodyState,
    support_after: RigidBodyState,
) -> Option<i64> {
    if after.translation_um.y >= before.translation_um.y {
        return None;
    }
    let mut body_index = 0_usize;
    let mut support_index = 0_usize;
    let mut highest_origin = None;
    while body_index < body.collision_bottom.len() && support_index < support.collision_top.len() {
        let bottom = body.collision_bottom[body_index];
        let top = support.collision_top[support_index];
        let body_column = world_column(body, before, bottom);
        let support_column = world_column(support, support_before, top);
        match body_column.cmp(&support_column) {
            std::cmp::Ordering::Less => body_index += 1,
            std::cmp::Ordering::Greater => support_index += 1,
            std::cmp::Ordering::Equal => {
                let local_bottom = i64::from(bottom.y - body.minimum.y) * MICROMETERS_PER_VOXEL;
                let local_support_top =
                    i64::from(top.y - support.minimum.y + 1).saturating_mul(MICROMETERS_PER_VOXEL);
                let previous_bottom = before.translation_um.y.saturating_add(local_bottom);
                let proposed_bottom = after.translation_um.y.saturating_add(local_bottom);
                let previous_support_top = support_before
                    .translation_um
                    .y
                    .saturating_add(local_support_top);
                let final_support_top = support_after
                    .translation_um
                    .y
                    .saturating_add(local_support_top);
                if previous_bottom >= previous_support_top && proposed_bottom < final_support_top {
                    let origin = final_support_top.saturating_sub(local_bottom);
                    highest_origin =
                        Some(highest_origin.map_or(origin, |height: i64| height.max(origin)));
                }
                body_index += 1;
                support_index += 1;
            }
        }
    }
    highest_origin
}

fn rests_on(
    body: &RigidBodyDescriptor,
    state: RigidBodyState,
    support: &RigidBodyDescriptor,
    support_state: RigidBodyState,
) -> bool {
    let mut body_index = 0_usize;
    let mut support_index = 0_usize;
    let mut highest_origin = None;
    while body_index < body.collision_bottom.len() && support_index < support.collision_top.len() {
        let bottom = body.collision_bottom[body_index];
        let top = support.collision_top[support_index];
        match world_column(body, state, bottom).cmp(&world_column(support, support_state, top)) {
            std::cmp::Ordering::Less => body_index += 1,
            std::cmp::Ordering::Greater => support_index += 1,
            std::cmp::Ordering::Equal => {
                let local_bottom = i64::from(bottom.y - body.minimum.y) * MICROMETERS_PER_VOXEL;
                let local_support_top =
                    i64::from(top.y - support.minimum.y + 1).saturating_mul(MICROMETERS_PER_VOXEL);
                let origin = support_state
                    .translation_um
                    .y
                    .saturating_add(local_support_top)
                    .saturating_sub(local_bottom);
                highest_origin =
                    Some(highest_origin.map_or(origin, |height: i64| height.max(origin)));
                body_index += 1;
                support_index += 1;
            }
        }
    }
    highest_origin == Some(state.translation_um.y)
}

fn world_column(body: &RigidBodyDescriptor, state: RigidBodyState, surface: IVec3) -> (i64, i64) {
    (
        state.translation_um.x.saturating_add(
            i64::from(surface.x - body.minimum.x).saturating_mul(MICROMETERS_PER_VOXEL),
        ),
        state.translation_um.z.saturating_add(
            i64::from(surface.z - body.minimum.z).saturating_mul(MICROMETERS_PER_VOXEL),
        ),
    )
}

const fn intervals_overlap(left_min: i64, left_max: i64, right_min: i64, right_max: i64) -> bool {
    left_min < right_max && right_min < left_max
}

const fn intervals_overlap_or_touch(
    left_min: i64,
    left_max: i64,
    right_min: i64,
    right_max: i64,
) -> bool {
    left_min <= right_max && right_min <= left_max
}

fn center_of_mass(
    voxels: &[BodyVoxel],
    total_mass_kg: u64,
) -> Result<FixedMillimeters3, BodyError> {
    let mut weighted = [0_i128; 3];
    for body_voxel in voxels {
        let position = body_voxel.position;
        let mass = i128::from(body_voxel.voxel.material.properties().density_kg_m3);
        for (sum, coordinate) in weighted.iter_mut().zip([
            voxel_center_mm(position.x),
            voxel_center_mm(position.y),
            voxel_center_mm(position.z),
        ]) {
            *sum = sum.saturating_add(mass.saturating_mul(i128::from(coordinate)));
        }
    }
    let denominator = i128::from(total_mass_kg);
    Ok(FixedMillimeters3 {
        x: fixed_coordinate(weighted[0], denominator)?,
        y: fixed_coordinate(weighted[1], denominator)?,
        z: fixed_coordinate(weighted[2], denominator)?,
    })
}

fn inertia_diagonal(voxels: &[BodyVoxel], center: FixedMillimeters3) -> InertiaDiagonalKgMm2 {
    let mut inertia = InertiaDiagonalKgMm2::default();
    for body_voxel in voxels {
        let position = body_voxel.position;
        let mass = u128::from(body_voxel.voxel.material.properties().density_kg_m3);
        let dx = absolute_difference(center.x, voxel_center_mm(position.x));
        let dy = absolute_difference(center.y, voxel_center_mm(position.y));
        let dz = absolute_difference(center.z, voxel_center_mm(position.z));
        let intrinsic = mass.saturating_mul(SQUARE_MILLIMETERS_PER_VOXEL) / 6;
        inertia.x = inertia.x.saturating_add(
            mass.saturating_mul(dy.saturating_mul(dy).saturating_add(dz.saturating_mul(dz)))
                .saturating_add(intrinsic),
        );
        inertia.y = inertia.y.saturating_add(
            mass.saturating_mul(dx.saturating_mul(dx).saturating_add(dz.saturating_mul(dz)))
                .saturating_add(intrinsic),
        );
        inertia.z = inertia.z.saturating_add(
            mass.saturating_mul(dx.saturating_mul(dx).saturating_add(dy.saturating_mul(dy)))
                .saturating_add(intrinsic),
        );
    }
    inertia
}

fn is_connected(voxels: &[BodyVoxel]) -> bool {
    let positions: HashSet<_> = voxels
        .iter()
        .map(|body_voxel| body_voxel.position)
        .collect();
    let mut visited = HashSet::with_capacity(voxels.len());
    let mut queue = VecDeque::from([voxels[0].position]);
    visited.insert(voxels[0].position);
    while let Some(position) = queue.pop_front() {
        for offset in BODY_NEIGHBORS {
            let neighbor = IVec3::new(
                position.x.saturating_add(offset.x),
                position.y.saturating_add(offset.y),
                position.z.saturating_add(offset.z),
            );
            if positions.contains(&neighbor) && visited.insert(neighbor) {
                queue.push_back(neighbor);
            }
        }
    }
    visited.len() == voxels.len()
}

fn body_bounds(voxels: &[BodyVoxel]) -> (IVec3, IVec3) {
    voxels.iter().fold(
        (
            IVec3::new(i32::MAX, i32::MAX, i32::MAX),
            IVec3::new(i32::MIN, i32::MIN, i32::MIN),
        ),
        |(minimum, maximum), body_voxel| {
            let position = body_voxel.position;
            (
                IVec3::new(
                    minimum.x.min(position.x),
                    minimum.y.min(position.y),
                    minimum.z.min(position.z),
                ),
                IVec3::new(
                    maximum.x.max(position.x),
                    maximum.y.max(position.y),
                    maximum.z.max(position.z),
                ),
            )
        },
    )
}

fn collision_surfaces(voxels: &[BodyVoxel]) -> (Vec<IVec3>, Vec<IVec3>) {
    let mut columns = BTreeMap::<(i32, i32), (IVec3, IVec3)>::new();
    for body_voxel in voxels {
        columns
            .entry((body_voxel.position.x, body_voxel.position.z))
            .and_modify(|(bottom, top)| {
                if body_voxel.position.y < bottom.y {
                    *bottom = body_voxel.position;
                }
                if body_voxel.position.y > top.y {
                    *top = body_voxel.position;
                }
            })
            .or_insert((body_voxel.position, body_voxel.position));
    }
    columns.into_values().unzip()
}

#[allow(clippy::missing_const_for_fn)]
fn voxel_center_mm(coordinate: i32) -> i64 {
    i64::from(coordinate) * MILLIMETERS_PER_VOXEL + MILLIMETERS_PER_VOXEL / 2
}

fn fixed_coordinate(numerator: i128, denominator: i128) -> Result<i64, BodyError> {
    let half = denominator / 2;
    let rounded = if numerator >= 0 {
        numerator.saturating_add(half) / denominator
    } else {
        -numerator.saturating_neg().saturating_add(half) / denominator
    };
    i64::try_from(rounded).map_err(|_| BodyError::CenterOfMassOverflow)
}

#[allow(clippy::missing_const_for_fn)]
fn absolute_difference(left: i64, right: i64) -> u128 {
    (i128::from(left) - i128::from(right)).unsigned_abs()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Material, StructuralAnchors, StructuralLimits, Voxel, VoxelChange};

    fn detached_island(world: &mut World, voxels: &[IVec3]) -> DetachedIsland {
        world.set_voxel(IVec3::new(0, 0, 0), Voxel::new(Material::Wood));
        world.set_voxel(IVec3::new(0, 1, 0), Voxel::new(Material::Wood));
        for &position in voxels {
            world.set_voxel(position, Voxel::new(Material::Wood));
        }
        let connector = IVec3::new(0, 1, 0);
        let before = world.set_voxel(connector, Voxel::AIR);
        let report = crate::analyze_structural_changes(
            world,
            &[VoxelChange {
                position: connector,
                before,
                after: Voxel::AIR,
            }],
            &StructuralAnchors::foundation_plane(0),
            StructuralLimits::default(),
        )
        .expect("fixture topology");
        report
            .detached_islands
            .into_iter()
            .next()
            .expect("fixture detached island")
    }

    #[test]
    fn one_voxel_body_has_exact_fixed_mass_properties() {
        let mut world = World::default();
        world.set_voxel(IVec3::new(0, 0, 0), Voxel::new(Material::Concrete));
        let island = describe_island(&world, vec![IVec3::new(0, 0, 0)]);

        let body =
            RigidBodyDescriptor::from_detached_island(1, &world, &island, BodyLimits::default())
                .expect("one concrete voxel is physical");

        assert_eq!(body.mass_kg, 2_400);
        assert_eq!(
            body.center_of_mass_mm,
            FixedMillimeters3 {
                x: 500,
                y: 500,
                z: 500,
            }
        );
        assert_eq!(
            body.inertia_diagonal_kg_mm2,
            InertiaDiagonalKgMm2 {
                x: 400_000_000,
                y: 400_000_000,
                z: 400_000_000,
            }
        );
    }

    #[test]
    fn symmetric_body_center_is_deterministic_for_negative_coordinates() {
        let mut world = World::default();
        world.set_voxel(IVec3::new(-1, 2, 0), Voxel::new(Material::Wood));
        world.set_voxel(IVec3::new(0, 2, 0), Voxel::new(Material::Wood));
        let island = describe_island(&world, vec![IVec3::new(-1, 2, 0), IVec3::new(0, 2, 0)]);

        let body =
            RigidBodyDescriptor::from_detached_island(1, &world, &island, BodyLimits::default())
                .expect("symmetric body");

        assert_eq!(body.center_of_mass_mm.x, 0);
        assert_eq!(body.center_of_mass_mm.y, 2_500);
        assert_eq!(body.inertia_diagonal_kg_mm2.x, 216_666_666);
        assert_eq!(body.inertia_diagonal_kg_mm2.y, 541_666_666);
        assert_eq!(body.inertia_diagonal_kg_mm2.z, 541_666_666);
    }

    #[test]
    fn forged_island_metadata_is_rejected() {
        let mut world = World::default();
        world.set_voxel(IVec3::new(0, 2, 0), Voxel::new(Material::Stone));
        let mut island = describe_island(&world, vec![IVec3::new(0, 2, 0)]);
        island.mass_kg += 1;

        assert_eq!(
            RigidBodyDescriptor::from_detached_island(1, &world, &island, BodyLimits::default()),
            Err(BodyError::IslandDescriptorMismatch)
        );
    }

    #[test]
    fn voxel_limit_is_checked_before_promotion() {
        let mut world = World::default();
        let island = detached_island(&mut world, &[IVec3::new(0, 2, 0)]);

        assert_eq!(
            RigidBodyDescriptor::from_detached_island(
                1,
                &world,
                &island,
                BodyLimits { max_voxels: 0 }
            ),
            Err(BodyError::TooManyVoxels(1))
        );
    }

    #[test]
    fn replicated_voxels_require_connectivity_but_not_unique_geometry() {
        let voxel = Voxel::new(Material::Steel);
        assert_eq!(
            RigidBodyDescriptor::from_replicated_voxels(
                0,
                vec![BodyVoxel {
                    position: IVec3::new(0, 2, 0),
                    voxel,
                }],
                BodyLimits::default(),
            ),
            Err(BodyError::InvalidEntityId(0))
        );
        let disconnected = vec![
            BodyVoxel {
                position: IVec3::new(0, 2, 0),
                voxel,
            },
            BodyVoxel {
                position: IVec3::new(2, 2, 0),
                voxel,
            },
        ];
        assert_eq!(
            RigidBodyDescriptor::from_replicated_voxels(1, disconnected, BodyLimits::default()),
            Err(BodyError::DisconnectedVoxels)
        );

        let canonical = vec![BodyVoxel {
            position: IVec3::new(0, 2, 0),
            voxel,
        }];
        let first = RigidBodyDescriptor::from_replicated_voxels(
            123,
            canonical.clone(),
            BodyLimits::default(),
        )
        .expect("first entity");
        let second =
            RigidBodyDescriptor::from_replicated_voxels(124, canonical, BodyLimits::default())
                .expect("same geometry under a new entity ID");
        assert_ne!(first.id, second.id);
        assert_eq!(first.geometry_fingerprint, second.geometry_fingerprint);
    }

    #[test]
    fn falling_body_sweeps_to_ground_and_sleeps_deterministically() {
        let mut world = World::default();
        world.set_voxel(IVec3::new(0, 0, 0), Voxel::new(Material::Stone));
        world.set_voxel(IVec3::new(0, 3, 0), Voxel::new(Material::Wood));
        let island = describe_island(&world, vec![IVec3::new(0, 3, 0)]);
        let body =
            RigidBodyDescriptor::from_detached_island(1, &world, &island, BodyLimits::default())
                .expect("one falling voxel");
        world.set_voxel(IVec3::new(0, 3, 0), Voxel::AIR);
        let mut state = RigidBodyState::at_spawn(&body);
        let mut collision_observed = false;

        for _ in 0..240 {
            collision_observed |= step_rigid_body(&world, &body, &mut state).collided_with_static;
        }

        assert!(collision_observed);
        assert!(state.sleeping);
        assert_eq!(state.translation_um.y, MICROMETERS_PER_VOXEL);
        assert_eq!(state.linear_velocity_um_per_second.y, 0);
        let settled = state;
        assert_eq!(
            step_rigid_body(&world, &body, &mut state),
            BodyStepResult::default()
        );
        assert_eq!(state, settled);
    }

    #[test]
    fn sweep_and_prune_reports_overlapping_body_bounds() {
        let mut world = World::default();
        world.set_voxel(IVec3::new(0, 2, 0), Voxel::new(Material::Wood));
        world.set_voxel(IVec3::new(2, 2, 0), Voxel::new(Material::Steel));
        let first_island = describe_island(&world, vec![IVec3::new(0, 2, 0)]);
        let second_island = describe_island(&world, vec![IVec3::new(2, 2, 0)]);
        let first = RigidBodyDescriptor::from_detached_island(
            1,
            &world,
            &first_island,
            BodyLimits::default(),
        )
        .expect("first body");
        let second = RigidBodyDescriptor::from_detached_island(
            2,
            &world,
            &second_island,
            BodyLimits::default(),
        )
        .expect("second body");
        let mut first_state = RigidBodyState::at_spawn(&first);
        let mut second_state = RigidBodyState::at_spawn(&second);
        first_state.translation_um.x = 0;
        second_state.translation_um.x = MICROMETERS_PER_VOXEL / 2;
        let first_id = first.id;
        let second_id = second.id;
        let bodies = BTreeMap::from([(first.id, first), (second.id, second)]);
        let states = BTreeMap::from([(first_id, first_state), (second_id, second_state)]);

        let broad_phase = broad_phase_pairs(&bodies, &states);
        assert_eq!(broad_phase.pairs.len(), 1);
        assert!(!broad_phase.saturated);
    }

    #[test]
    fn vertical_bodies_stack_and_sleep_without_interpenetration() {
        let mut world = World::default();
        world.set_voxel(IVec3::new(0, 0, 0), Voxel::new(Material::Stone));
        let mut bodies = BTreeMap::new();
        let mut states = BTreeMap::new();
        let mut body_ids = Vec::new();
        for (height, material) in [(4, Material::Concrete), (6, Material::Wood)] {
            let position = IVec3::new(0, height, 0);
            world.set_voxel(position, Voxel::new(material));
            let body = RigidBodyDescriptor::from_world_voxels(
                u64::try_from(body_ids.len() + 1).expect("small fixture"),
                &world,
                vec![position],
                BodyLimits::default(),
            )
            .expect("one voxel body");
            world.set_voxel(position, Voxel::AIR);
            body_ids.push(body.id);
            states.insert(body.id, RigidBodyState::at_spawn(&body));
            bodies.insert(body.id, body);
        }

        let mut observed_body_contact = false;
        for _ in 0..300 {
            let report = step_rigid_bodies(&world, &bodies, &mut states);
            assert!(!report.broad_phase_saturated);
            observed_body_contact |= report.body_collisions > 0;
        }

        assert!(observed_body_contact);
        assert_eq!(states[&body_ids[0]].translation_um.y, MICROMETERS_PER_VOXEL);
        assert_eq!(
            states[&body_ids[1]].translation_um.y,
            2 * MICROMETERS_PER_VOXEL
        );
        assert!(states.values().all(|state| state.sleeping));

        let lower = states.get_mut(&body_ids[0]).expect("lower state");
        lower.sleeping = false;
        lower.sleep_ticks = 0;
        let report = step_rigid_bodies(&world, &bodies, &mut states);
        assert_eq!(report.bodies_woken, 1);
        assert!(!states[&body_ids[1]].sleeping);
    }

    #[test]
    fn broad_phase_overflow_discards_the_complete_tick() {
        const OVERLAPPING_BODIES: usize = 130;
        let mut world = World::default();
        let mut bodies = BTreeMap::new();
        let mut states = BTreeMap::new();
        for index in 0..OVERLAPPING_BODIES {
            let position = IVec3::new(i32::try_from(index).expect("small fixture"), 4, 0);
            world.set_voxel(position, Voxel::new(Material::Concrete));
            let body = RigidBodyDescriptor::from_world_voxels(
                u64::try_from(index + 1).expect("small fixture"),
                &world,
                vec![position],
                BodyLimits::default(),
            )
            .expect("one voxel body");
            world.set_voxel(position, Voxel::AIR);
            let mut state = RigidBodyState::at_spawn(&body);
            state.translation_um = FixedMicrometers3 {
                x: 0,
                y: 4 * MICROMETERS_PER_VOXEL,
                z: 0,
            };
            states.insert(body.id, state);
            bodies.insert(body.id, body);
        }
        let before = states.clone();

        let report = step_rigid_bodies(&world, &bodies, &mut states);

        assert!(report.broad_phase_saturated);
        assert_eq!(report.broad_phase_pairs, MAX_BROAD_PHASE_PAIRS);
        assert!(report.transitions.is_empty());
        assert_eq!(states, before);
    }

    #[test]
    fn unsupported_or_incoherent_wire_states_are_invalid() {
        let mut state = RigidBodyState {
            translation_um: FixedMicrometers3::default(),
            linear_velocity_um_per_second: FixedMicrometers3::default(),
            integration_remainder: [0; 3],
            sleep_ticks: 0,
            sleeping: false,
        };
        assert!(valid_rigid_body_state(state));
        state.linear_velocity_um_per_second.x = 1;
        assert!(!valid_rigid_body_state(state));
        state.linear_velocity_um_per_second.x = 0;
        state.sleeping = true;
        assert!(!valid_rigid_body_state(state));
        state.sleep_ticks = SLEEP_TICKS;
        assert!(valid_rigid_body_state(state));
    }
}
