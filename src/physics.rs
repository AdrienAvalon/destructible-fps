//! Fixed-unit authoritative rigid-body descriptors derived from detached voxel islands.

use crate::{
    DetachedIsland, IVec3, Voxel, World,
    structural::{ISLAND_FINGERPRINT_SEED, describe_island, mix_island_fingerprint},
};
use core::fmt;
use std::collections::{BTreeMap, HashSet, VecDeque};

const MILLIMETERS_PER_VOXEL: i64 = 1_000;
const SQUARE_MILLIMETERS_PER_VOXEL: u128 = 1_000_000;
pub const MICROMETERS_PER_VOXEL: i64 = 1_000_000;
pub const SERVER_PHYSICS_HZ: i64 = 60;
const GRAVITY_UM_PER_SECOND_SQUARED: i64 = -9_810_000;
const MAX_LINEAR_SPEED_UM_PER_SECOND: i64 = 250_000_000;
const MAX_WORLD_TRANSLATION_UM: i64 = 3_000_000_000_000_000;
const SLEEP_TICKS: u16 = 30;
pub const MAX_BROAD_PHASE_PAIRS: usize = 8_192;
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
    pub id: u128,
    pub voxels: Vec<BodyVoxel>,
    pub minimum: IVec3,
    pub maximum: IVec3,
    pub mass_kg: u64,
    pub center_of_mass_mm: FixedMillimeters3,
    pub inertia_diagonal_kg_mm2: InertiaDiagonalKgMm2,
    /// Lowest occupied voxel in each local X/Z column, ordered by world position.
    pub collision_bottom: Vec<IVec3>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BodyError {
    EmptyIsland,
    TooManyVoxels(usize),
    NonCanonicalVoxels(IVec3),
    NonStructuralVoxel(IVec3),
    DisconnectedVoxels,
    IslandDescriptorMismatch,
    IdentifierMismatch { expected: u128, actual: u128 },
    ZeroMass,
    CenterOfMassOverflow,
}

impl fmt::Display for BodyError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
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
            Self::IdentifierMismatch { expected, actual } => write!(
                formatter,
                "rigid-body identifier mismatch: expected {expected:032x}, computed {actual:032x}"
            ),
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
        world: &World,
        voxels: Vec<IVec3>,
        limits: BodyLimits,
    ) -> Result<Self, BodyError> {
        let island = describe_island(world, voxels);
        Self::from_detached_island(world, &island, limits)
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
        Self::from_replicated_voxels(canonical.fingerprint, voxels, limits)
    }

    /// Rebuilds and verifies a body from untrusted replicated voxel membership.
    ///
    /// # Errors
    ///
    /// Applies the same size, canonical-order, structural-material, connectivity, mass, and identity
    /// checks as local promotion.
    pub fn from_replicated_voxels(
        expected_id: u128,
        voxels: Vec<BodyVoxel>,
        limits: BodyLimits,
    ) -> Result<Self, BodyError> {
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
        if fingerprint != expected_id {
            return Err(BodyError::IdentifierMismatch {
                expected: expected_id,
                actual: fingerprint,
            });
        }
        let center_of_mass_mm = center_of_mass(&voxels, mass_kg)?;
        let inertia_diagonal_kg_mm2 = inertia_diagonal(&voxels, center_of_mass_mm);
        let collision_bottom = collision_bottom(&voxels);
        Ok(Self {
            id: fingerprint,
            voxels,
            minimum,
            maximum,
            mass_kg,
            center_of_mass_mm,
            inertia_diagonal_kg_mm2,
            collision_bottom,
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
    bodies: &BTreeMap<u128, RigidBodyDescriptor>,
    states: &BTreeMap<u128, RigidBodyState>,
) -> Vec<(u128, u128)> {
    if states.values().all(|state| state.sleeping) {
        return Vec::new();
    }
    let mut entries = bodies
        .iter()
        .filter_map(|(&id, body)| {
            states
                .get(&id)
                .map(|state| (id, body_aabb(body, *state), state.sleeping))
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
            if intervals_overlap(left.0.y, left.1.y, right.0.y, right.1.y)
                && intervals_overlap(left.0.z, left.1.z, right.0.z, right.1.z)
            {
                if pairs.len() == MAX_BROAD_PHASE_PAIRS {
                    return pairs;
                }
                pairs.push((left_id, right_id));
            }
        }
    }
    pairs
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

const fn intervals_overlap(left_min: i64, left_max: i64, right_min: i64, right_max: i64) -> bool {
    left_min < right_max && right_min < left_max
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

fn collision_bottom(voxels: &[BodyVoxel]) -> Vec<IVec3> {
    let mut columns = BTreeMap::<(i32, i32), IVec3>::new();
    for body_voxel in voxels {
        columns
            .entry((body_voxel.position.x, body_voxel.position.z))
            .and_modify(|bottom| {
                if body_voxel.position.y < bottom.y {
                    *bottom = body_voxel.position;
                }
            })
            .or_insert(body_voxel.position);
    }
    columns.into_values().collect()
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
            RigidBodyDescriptor::from_detached_island(&world, &island, BodyLimits::default())
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
            RigidBodyDescriptor::from_detached_island(&world, &island, BodyLimits::default())
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
            RigidBodyDescriptor::from_detached_island(&world, &island, BodyLimits::default()),
            Err(BodyError::IslandDescriptorMismatch)
        );
    }

    #[test]
    fn voxel_limit_is_checked_before_promotion() {
        let mut world = World::default();
        let island = detached_island(&mut world, &[IVec3::new(0, 2, 0)]);

        assert_eq!(
            RigidBodyDescriptor::from_detached_island(
                &world,
                &island,
                BodyLimits { max_voxels: 0 }
            ),
            Err(BodyError::TooManyVoxels(1))
        );
    }

    #[test]
    fn replicated_voxels_require_connectivity_and_matching_identity() {
        let voxel = Voxel::new(Material::Steel);
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
            RigidBodyDescriptor::from_replicated_voxels(0, disconnected, BodyLimits::default()),
            Err(BodyError::DisconnectedVoxels)
        );

        let canonical = vec![BodyVoxel {
            position: IVec3::new(0, 2, 0),
            voxel,
        }];
        assert!(matches!(
            RigidBodyDescriptor::from_replicated_voxels(123, canonical, BodyLimits::default()),
            Err(BodyError::IdentifierMismatch { expected: 123, .. })
        ));
    }

    #[test]
    fn falling_body_sweeps_to_ground_and_sleeps_deterministically() {
        let mut world = World::default();
        world.set_voxel(IVec3::new(0, 0, 0), Voxel::new(Material::Stone));
        world.set_voxel(IVec3::new(0, 3, 0), Voxel::new(Material::Wood));
        let island = describe_island(&world, vec![IVec3::new(0, 3, 0)]);
        let body =
            RigidBodyDescriptor::from_detached_island(&world, &island, BodyLimits::default())
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
        let first =
            RigidBodyDescriptor::from_detached_island(&world, &first_island, BodyLimits::default())
                .expect("first body");
        let second = RigidBodyDescriptor::from_detached_island(
            &world,
            &second_island,
            BodyLimits::default(),
        )
        .expect("second body");
        let mut first_state = RigidBodyState::at_spawn(&first);
        let mut second_state = RigidBodyState::at_spawn(&second);
        first_state.translation_um.x = 0;
        second_state.translation_um.x = MICROMETERS_PER_VOXEL / 2;
        let bodies = BTreeMap::from([(first.id, first), (second.id, second)]);
        let states = BTreeMap::from([
            (first_island.fingerprint(), first_state),
            (second_island.fingerprint(), second_state),
        ]);

        assert_eq!(broad_phase_pairs(&bodies, &states).len(), 1);
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
