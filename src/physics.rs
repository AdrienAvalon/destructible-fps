//! Fixed-unit authoritative rigid-body descriptors derived from detached voxel islands.

use crate::{
    DetachedIsland, IVec3, Voxel, World,
    structural::{ISLAND_FINGERPRINT_SEED, describe_island, mix_island_fingerprint},
};
use core::fmt;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};

const MILLIMETERS_PER_VOXEL: i64 = 1_000;
const SQUARE_MILLIMETERS_PER_VOXEL: u128 = 1_000_000;
pub const MICROMETERS_PER_VOXEL: i64 = 1_000_000;
pub const SERVER_PHYSICS_HZ: i64 = 60;
pub const MAX_BODY_SOLVER_PASSES: usize = 4;
pub const FIXED_QUATERNION_SCALE: i32 = 1_000_000;
const GRAVITY_UM_PER_SECOND_SQUARED: i64 = -9_810_000;
const MAX_LINEAR_SPEED_UM_PER_SECOND: i64 = 250_000_000;
pub const MAX_ANGULAR_SPEED_MRAD_PER_SECOND: i64 = 12_000;
const MAX_WORLD_TRANSLATION_UM: i64 = 3_000_000_000_000_000;
const RESPONSE_SCALE: i64 = 1_000;
const QUATERNION_NORMALIZATION_TOLERANCE: u128 = 8_000_000;
const MIN_BOUNCE_SPEED_UM_PER_SECOND: i64 = 500_000;
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

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct FixedImpulseMilliNewtonSeconds3 {
    pub x: i64,
    pub y: i64,
    pub z: i64,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct FixedMilliradians3 {
    pub x: i64,
    pub y: i64,
    pub z: i64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FixedQuaternion {
    pub x: i32,
    pub y: i32,
    pub z: i32,
    pub w: i32,
}

impl FixedQuaternion {
    pub const IDENTITY: Self = Self {
        x: 0,
        y: 0,
        z: 0,
        w: FIXED_QUATERNION_SCALE,
    };
}

impl Default for FixedQuaternion {
    fn default() -> Self {
        Self::IDENTITY
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RigidBodyState {
    /// Absolute world translation of the body's local-space minimum corner.
    pub translation_um: FixedMicrometers3,
    pub linear_velocity_um_per_second: FixedMicrometers3,
    /// Canonical local-to-world unit quaternion scaled by [`FIXED_QUATERNION_SCALE`].
    pub orientation: FixedQuaternion,
    /// World-space angular velocity in milliradians per second.
    pub angular_velocity_mrad_per_second: FixedMilliradians3,
    /// Euclidean remainders retained when velocity is divided by the fixed 60 Hz rate.
    pub integration_remainder: [u8; 3],
    pub angular_integration_remainder: [u8; 3],
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
    /// Extremal occupied voxels for swept lateral contacts, each in canonical column order.
    pub collision_left: Vec<IVec3>,
    pub collision_right: Vec<IVec3>,
    pub collision_back: Vec<IVec3>,
    pub collision_front: Vec<IVec3>,
    /// Mass-weighted material response coefficients in thousandths.
    pub friction_per_mille: u16,
    pub restitution_per_mille: u16,
    pub fragmentation_per_mille: u16,
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
        let mut weighted_friction = 0_u128;
        let mut weighted_restitution = 0_u128;
        let mut weighted_fragmentation = 0_u128;
        let mut fingerprint = ISLAND_FINGERPRINT_SEED;
        for body_voxel in &voxels {
            let properties = body_voxel.voxel.material.properties();
            let voxel_mass = u64::from(properties.density_kg_m3);
            mass_kg = mass_kg.saturating_add(voxel_mass);
            weighted_friction = weighted_friction.saturating_add(
                u128::from(voxel_mass).saturating_mul(u128::from(properties.friction_per_mille)),
            );
            weighted_restitution = weighted_restitution.saturating_add(
                u128::from(voxel_mass).saturating_mul(u128::from(properties.restitution_per_mille)),
            );
            weighted_fragmentation = weighted_fragmentation.saturating_add(
                u128::from(voxel_mass)
                    .saturating_mul(u128::from(properties.fragmentation).saturating_mul(10)),
            );
            fingerprint =
                mix_island_fingerprint(fingerprint, body_voxel.position, body_voxel.voxel);
        }
        if mass_kg == 0 {
            return Err(BodyError::ZeroMass);
        }
        let center_of_mass_mm = center_of_mass(&voxels, mass_kg)?;
        let inertia_diagonal_kg_mm2 = inertia_diagonal(&voxels, center_of_mass_mm);
        let collision = collision_surfaces(&voxels);
        let friction_per_mille = weighted_response(weighted_friction, mass_kg);
        let restitution_per_mille = weighted_response(weighted_restitution, mass_kg);
        let fragmentation_per_mille = weighted_response(weighted_fragmentation, mass_kg);
        Ok(Self {
            id: body_id,
            geometry_fingerprint: fingerprint,
            voxels,
            minimum,
            maximum,
            mass_kg,
            center_of_mass_mm,
            inertia_diagonal_kg_mm2,
            collision_bottom: collision.bottom,
            collision_top: collision.top,
            collision_left: collision.left,
            collision_right: collision.right,
            collision_back: collision.back,
            collision_front: collision.front,
            friction_per_mille,
            restitution_per_mille,
            fragmentation_per_mille,
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
            orientation: FixedQuaternion::IDENTITY,
            angular_velocity_mrad_per_second: FixedMilliradians3::default(),
            integration_remainder: [0; 3],
            angular_integration_remainder: [0; 3],
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
    let angular_velocities = [
        state.angular_velocity_mrad_per_second.x,
        state.angular_velocity_mrad_per_second.y,
        state.angular_velocity_mrad_per_second.z,
    ];
    let valid_sleep = if state.sleeping {
        state.sleep_ticks == SLEEP_TICKS
            && state.linear_velocity_um_per_second == FixedMicrometers3::default()
            && state.angular_velocity_mrad_per_second == FixedMilliradians3::default()
            && state.integration_remainder == [0; 3]
            && state.angular_integration_remainder == [0; 3]
    } else {
        state.sleep_ticks < SLEEP_TICKS
    };
    translations
        .iter()
        .all(|value| value.unsigned_abs() <= MAX_WORLD_TRANSLATION_UM.cast_unsigned())
        && velocities
            .iter()
            .all(|value| value.unsigned_abs() <= MAX_LINEAR_SPEED_UM_PER_SECOND.cast_unsigned())
        && angular_velocities
            .iter()
            .all(|value| value.unsigned_abs() <= MAX_ANGULAR_SPEED_MRAD_PER_SECOND.cast_unsigned())
        && valid_fixed_quaternion(state.orientation)
        && state
            .integration_remainder
            .iter()
            .all(|&remainder| i64::from(remainder) < SERVER_PHYSICS_HZ)
        && state
            .angular_integration_remainder
            .iter()
            .all(|&remainder| i64::from(remainder) < SERVER_PHYSICS_HZ)
        && valid_sleep
}

#[must_use]
pub fn valid_fixed_quaternion(orientation: FixedQuaternion) -> bool {
    let values = [orientation.x, orientation.y, orientation.z, orientation.w];
    if values
        .iter()
        .any(|value| value.unsigned_abs() > FIXED_QUATERNION_SCALE.cast_unsigned())
        || !quaternion_is_canonical(orientation)
    {
        return false;
    }
    let norm_squared = values.iter().fold(0_u128, |sum, value| {
        sum.saturating_add(u128::from(value.unsigned_abs()).pow(2))
    });
    let expected = u128::from(FIXED_QUATERNION_SCALE.cast_unsigned()).pow(2);
    norm_squared.abs_diff(expected) <= QUATERNION_NORMALIZATION_TOLERANCE
}

/// Applies a world-space linear impulse using integer milli-newton seconds and wakes the body.
///
/// One milli-newton second changes one kilogram by 1,000 micrometres per second. Components are
/// divided independently with truncation toward zero and clamped to the authoritative speed bound.
#[must_use]
pub fn apply_linear_impulse(
    body: &RigidBodyDescriptor,
    state: &mut RigidBodyState,
    impulse: FixedImpulseMilliNewtonSeconds3,
) -> bool {
    let before = *state;
    for (velocity, impulse) in [
        (&mut state.linear_velocity_um_per_second.x, impulse.x),
        (&mut state.linear_velocity_um_per_second.y, impulse.y),
        (&mut state.linear_velocity_um_per_second.z, impulse.z),
    ] {
        let delta = i128::from(impulse)
            .saturating_mul(1_000)
            .checked_div(i128::from(body.mass_kg))
            .and_then(|value| i64::try_from(value).ok())
            .unwrap_or_else(|| {
                impulse
                    .signum()
                    .saturating_mul(MAX_LINEAR_SPEED_UM_PER_SECOND)
            });
        *velocity = velocity.saturating_add(delta).clamp(
            -MAX_LINEAR_SPEED_UM_PER_SECOND,
            MAX_LINEAR_SPEED_UM_PER_SECOND,
        );
    }
    if state.linear_velocity_um_per_second != FixedMicrometers3::default() {
        state.sleep_ticks = 0;
        state.sleeping = false;
    }
    *state != before
}

/// Applies one impulse at a body-local point and derives angular velocity from the inertia tensor.
///
/// The application point is expressed in millimetres from the minimum corner of the immutable body
/// mesh. The lever arm is rotated into world space, while the diagonal inertia response is evaluated
/// in body space. This keeps the authoritative calculation integer-only and deterministic.
#[must_use]
pub fn apply_impulse_at_local_point(
    body: &RigidBodyDescriptor,
    state: &mut RigidBodyState,
    impulse: FixedImpulseMilliNewtonSeconds3,
    application_point_mm: FixedMillimeters3,
) -> bool {
    let before = *state;
    let _ = apply_linear_impulse(body, state, impulse);
    let local_center = [
        i128::from(body.center_of_mass_mm.x)
            .saturating_sub(i128::from(body.minimum.x) * i128::from(MILLIMETERS_PER_VOXEL)),
        i128::from(body.center_of_mass_mm.y)
            .saturating_sub(i128::from(body.minimum.y) * i128::from(MILLIMETERS_PER_VOXEL)),
        i128::from(body.center_of_mass_mm.z)
            .saturating_sub(i128::from(body.minimum.z) * i128::from(MILLIMETERS_PER_VOXEL)),
    ];
    let local_lever = [
        i128::from(application_point_mm.x).saturating_sub(local_center[0]),
        i128::from(application_point_mm.y).saturating_sub(local_center[1]),
        i128::from(application_point_mm.z).saturating_sub(local_center[2]),
    ];
    let world_lever = rotate_fixed_vector(state.orientation, local_lever);
    let world_impulse = [
        i128::from(impulse.x),
        i128::from(impulse.y),
        i128::from(impulse.z),
    ];
    let world_angular_impulse = cross_i128(world_lever, world_impulse);
    let body_angular_impulse =
        rotate_fixed_vector(conjugate(state.orientation), world_angular_impulse);
    let inertia = [
        body.inertia_diagonal_kg_mm2.x,
        body.inertia_diagonal_kg_mm2.y,
        body.inertia_diagonal_kg_mm2.z,
    ];
    let mut body_delta = [0_i128; 3];
    for index in 0..3 {
        let denominator = i128::try_from(inertia[index]).unwrap_or(i128::MAX).max(1);
        body_delta[index] = body_angular_impulse[index].saturating_mul(1_000) / denominator;
    }
    let world_delta = rotate_fixed_vector(state.orientation, body_delta);
    for (velocity, delta) in [
        (
            &mut state.angular_velocity_mrad_per_second.x,
            world_delta[0],
        ),
        (
            &mut state.angular_velocity_mrad_per_second.y,
            world_delta[1],
        ),
        (
            &mut state.angular_velocity_mrad_per_second.z,
            world_delta[2],
        ),
    ] {
        let delta = bounded_i64(delta, MAX_ANGULAR_SPEED_MRAD_PER_SECOND);
        *velocity = velocity.saturating_add(delta).clamp(
            -MAX_ANGULAR_SPEED_MRAD_PER_SECOND,
            MAX_ANGULAR_SPEED_MRAD_PER_SECOND,
        );
    }
    if state.angular_velocity_mrad_per_second != FixedMilliradians3::default() {
        state.sleep_ticks = 0;
        state.sleeping = false;
    }
    *state != before
}

const fn quaternion_is_canonical(orientation: FixedQuaternion) -> bool {
    orientation.w > 0
        || orientation.w == 0
            && (orientation.x > 0
                || orientation.x == 0
                    && (orientation.y > 0 || orientation.y == 0 && orientation.z >= 0))
}

const fn conjugate(orientation: FixedQuaternion) -> FixedQuaternion {
    FixedQuaternion {
        x: -orientation.x,
        y: -orientation.y,
        z: -orientation.z,
        w: orientation.w,
    }
}

fn integrate_orientation(state: &mut RigidBodyState) {
    let mut delta_mrad = [0_i64; 3];
    for (index, velocity) in [
        state.angular_velocity_mrad_per_second.x,
        state.angular_velocity_mrad_per_second.y,
        state.angular_velocity_mrad_per_second.z,
    ]
    .into_iter()
    .enumerate()
    {
        let (delta, remainder) =
            integrate_axis(velocity, state.angular_integration_remainder[index]);
        delta_mrad[index] = delta;
        state.angular_integration_remainder[index] = remainder;
    }
    if delta_mrad == [0; 3] {
        return;
    }
    let scale = i128::from(FIXED_QUATERNION_SCALE);
    let delta = [
        i128::from(delta_mrad[0]).saturating_mul(scale) / 2_000,
        i128::from(delta_mrad[1]).saturating_mul(scale) / 2_000,
        i128::from(delta_mrad[2]).saturating_mul(scale) / 2_000,
        scale,
    ];
    let orientation = [
        i128::from(state.orientation.x),
        i128::from(state.orientation.y),
        i128::from(state.orientation.z),
        i128::from(state.orientation.w),
    ];
    state.orientation = normalize_quaternion(quaternion_product(delta, orientation));
}

const fn quaternion_product(first: [i128; 4], second: [i128; 4]) -> [i128; 4] {
    let [ax, ay, az, aw] = first;
    let [bx, by, bz, bw] = second;
    [
        aw.saturating_mul(bx)
            .saturating_add(ax.saturating_mul(bw))
            .saturating_add(ay.saturating_mul(bz))
            .saturating_sub(az.saturating_mul(by)),
        aw.saturating_mul(by)
            .saturating_sub(ax.saturating_mul(bz))
            .saturating_add(ay.saturating_mul(bw))
            .saturating_add(az.saturating_mul(bx)),
        aw.saturating_mul(bz)
            .saturating_add(ax.saturating_mul(by))
            .saturating_sub(ay.saturating_mul(bx))
            .saturating_add(az.saturating_mul(bw)),
        aw.saturating_mul(bw)
            .saturating_sub(ax.saturating_mul(bx))
            .saturating_sub(ay.saturating_mul(by))
            .saturating_sub(az.saturating_mul(bz)),
    ]
}

fn normalize_quaternion(raw: [i128; 4]) -> FixedQuaternion {
    let norm_squared = raw.iter().fold(0_u128, |sum, value| {
        sum.saturating_add(value.unsigned_abs().saturating_mul(value.unsigned_abs()))
    });
    if norm_squared == 0 {
        return FixedQuaternion::IDENTITY;
    }
    let norm = i128::try_from(norm_squared.isqrt())
        .unwrap_or(i128::MAX)
        .max(1);
    let scale = i128::from(FIXED_QUATERNION_SCALE);
    let mut normalized = raw.map(|value| {
        let scaled = value.saturating_mul(scale);
        let rounded = if scaled >= 0 {
            scaled.saturating_add(norm / 2)
        } else {
            scaled.saturating_sub(norm / 2)
        } / norm;
        i32::try_from(rounded)
            .unwrap_or_else(|_| rounded.signum() as i32 * FIXED_QUATERNION_SCALE)
            .clamp(-FIXED_QUATERNION_SCALE, FIXED_QUATERNION_SCALE)
    });
    let candidate = FixedQuaternion {
        x: normalized[0],
        y: normalized[1],
        z: normalized[2],
        w: normalized[3],
    };
    if quaternion_is_canonical(candidate) {
        candidate
    } else {
        for value in &mut normalized {
            *value = -*value;
        }
        FixedQuaternion {
            x: normalized[0],
            y: normalized[1],
            z: normalized[2],
            w: normalized[3],
        }
    }
}

fn rotate_fixed_vector(orientation: FixedQuaternion, vector: [i128; 3]) -> [i128; 3] {
    let q = [
        i128::from(orientation.x),
        i128::from(orientation.y),
        i128::from(orientation.z),
    ];
    let first_cross = cross_i128(q, vector);
    let second_cross = cross_i128(q, first_cross);
    let denominator = i128::from(FIXED_QUATERNION_SCALE).pow(2);
    let w = i128::from(orientation.w);
    std::array::from_fn(|index| {
        let correction = w
            .saturating_mul(first_cross[index])
            .saturating_add(second_cross[index])
            .saturating_mul(2);
        vector[index].saturating_add(correction / denominator)
    })
}

const fn cross_i128(first: [i128; 3], second: [i128; 3]) -> [i128; 3] {
    [
        first[1]
            .saturating_mul(second[2])
            .saturating_sub(first[2].saturating_mul(second[1])),
        first[2]
            .saturating_mul(second[0])
            .saturating_sub(first[0].saturating_mul(second[2])),
        first[0]
            .saturating_mul(second[1])
            .saturating_sub(first[1].saturating_mul(second[0])),
    ]
}

fn bounded_i64(value: i128, limit: i64) -> i64 {
    i64::try_from(value)
        .unwrap_or_else(|_| value.signum() as i64 * limit)
        .clamp(-limit, limit)
}

fn clamp_angular_velocity(state: &mut RigidBodyState) {
    for velocity in [
        &mut state.angular_velocity_mrad_per_second.x,
        &mut state.angular_velocity_mrad_per_second.y,
        &mut state.angular_velocity_mrad_per_second.z,
    ] {
        *velocity = (*velocity).clamp(
            -MAX_ANGULAR_SPEED_MRAD_PER_SECOND,
            MAX_ANGULAR_SPEED_MRAD_PER_SECOND,
        );
    }
}

fn damp_supported_angular_motion(state: &mut RigidBodyState) {
    for velocity in [
        &mut state.angular_velocity_mrad_per_second.x,
        &mut state.angular_velocity_mrad_per_second.y,
        &mut state.angular_velocity_mrad_per_second.z,
    ] {
        let reduction = i64::try_from(velocity.unsigned_abs() / 20)
            .unwrap_or(i64::MAX)
            .max(1);
        *velocity = approach_zero(*velocity, reduction);
    }
    if state.angular_velocity_mrad_per_second == FixedMilliradians3::default() {
        state.angular_integration_remainder = [0; 3];
    }
}

/// Advances one axis-aligned body with integer semi-implicit Euler integration and three-axis
/// swept collision queries against static voxels.
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
    state.linear_velocity_um_per_second.x = state.linear_velocity_um_per_second.x.clamp(
        -MAX_LINEAR_SPEED_UM_PER_SECOND,
        MAX_LINEAR_SPEED_UM_PER_SECOND,
    );
    state.linear_velocity_um_per_second.z = state.linear_velocity_um_per_second.z.clamp(
        -MAX_LINEAR_SPEED_UM_PER_SECOND,
        MAX_LINEAR_SPEED_UM_PER_SECOND,
    );
    clamp_angular_velocity(state);
    integrate_orientation(state);
    let mut collided_with_static = false;
    for axis in [Axis::X, Axis::Z, Axis::Y] {
        let velocity = axis.component(state.linear_velocity_um_per_second);
        let remainder_index = axis.index();
        let (delta, remainder) =
            integrate_axis(velocity, state.integration_remainder[remainder_index]);
        let current = axis.component(state.translation_um);
        let proposed = current
            .saturating_add(delta)
            .clamp(-MAX_WORLD_TRANSLATION_UM, MAX_WORLD_TRANSLATION_UM);
        let contact = sweep_static_axis(world, body, state.translation_um, axis, proposed);
        if let Some(contact) = contact {
            collided_with_static = true;
            axis.set_component(&mut state.translation_um, contact.origin);
            let incoming = axis.component(state.linear_velocity_um_per_second);
            let restitution =
                combined_response(body.restitution_per_mille, contact.restitution_per_mille);
            let reflected = reflected_velocity(incoming, restitution);
            axis.set_component(&mut state.linear_velocity_um_per_second, reflected);
            state.integration_remainder[remainder_index] = 0;
            if axis == Axis::Y && incoming < 0 {
                let friction =
                    combined_response(body.friction_per_mille, contact.friction_per_mille);
                apply_ground_friction(state, incoming.unsigned_abs(), friction);
            }
        } else {
            axis.set_component(&mut state.translation_um, proposed);
            state.integration_remainder[remainder_index] = remainder;
        }
    }

    let supported = sweep_static_axis(
        world,
        body,
        state.translation_um,
        Axis::Y,
        state.translation_um.y.saturating_sub(1),
    )
    .is_some();
    if supported {
        damp_supported_angular_motion(state);
    }
    if supported
        && state.linear_velocity_um_per_second == FixedMicrometers3::default()
        && state.angular_velocity_mrad_per_second == FixedMilliradians3::default()
    {
        state.integration_remainder = [0; 3];
        state.angular_integration_remainder = [0; 3];
        state.sleep_ticks = state.sleep_ticks.saturating_add(1).min(SLEEP_TICKS);
        state.sleeping = state.sleep_ticks == SLEEP_TICKS;
    } else {
        state.sleep_ticks = 0;
        state.sleeping = false;
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
    let lateral_body_collisions =
        resolve_lateral_body_contacts(bodies, states, &mut next, &broad_phase.pairs);
    let vertical_body_collisions =
        resolve_vertical_body_contacts(bodies, states, &mut next, &adjacency);
    let body_collisions = lateral_body_collisions.saturating_add(vertical_body_collisions);
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

fn resolve_vertical_body_contacts(
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

fn resolve_lateral_body_contacts(
    bodies: &BTreeMap<BodyId, RigidBodyDescriptor>,
    before: &BTreeMap<BodyId, RigidBodyState>,
    next: &mut BTreeMap<BodyId, RigidBodyState>,
    pairs: &[(BodyId, BodyId)],
) -> usize {
    let mut collisions = 0_usize;
    for _ in 0..MAX_BODY_SOLVER_PASSES {
        let resolved = resolve_lateral_body_contact_pass(bodies, before, next, pairs);
        collisions = collisions.saturating_add(resolved);
        if resolved == 0 {
            break;
        }
    }
    collisions
}

fn resolve_lateral_body_contact_pass(
    bodies: &BTreeMap<BodyId, RigidBodyDescriptor>,
    before: &BTreeMap<BodyId, RigidBodyState>,
    next: &mut BTreeMap<BodyId, RigidBodyState>,
    pairs: &[(BodyId, BodyId)],
) -> usize {
    let mut resolved = 0_usize;
    for &(first_id, second_id) in pairs {
        let (Some(first_body), Some(second_body)) = (bodies.get(&first_id), bodies.get(&second_id))
        else {
            continue;
        };
        let (Some(first_before), Some(second_before), Some(mut first), Some(mut second)) = (
            before.get(&first_id).copied(),
            before.get(&second_id).copied(),
            next.get(&first_id).copied(),
            next.get(&second_id).copied(),
        ) else {
            continue;
        };
        let Some(contact) = lateral_body_contact(
            first_body,
            first_before,
            first,
            second_body,
            second_before,
            second,
        ) else {
            continue;
        };
        if !separate_lateral_contact(first_body, &mut first, second_body, &mut second, contact) {
            continue;
        }
        next.insert(first_id, first);
        next.insert(second_id, second);
        resolved = resolved.saturating_add(1);
    }
    resolved
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
            if right.0.x > left.1.x {
                break;
            }
            if left_sleeping && right_sleeping {
                continue;
            }
            if intervals_overlap_or_touch(left.0.y, left.1.y, right.0.y, right.1.y)
                && intervals_overlap_or_touch(left.0.z, left.1.z, right.0.z, right.1.z)
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
    let stopped_horizontally =
        state.linear_velocity_um_per_second.x == 0 && state.linear_velocity_um_per_second.z == 0;
    if stable_contact && stopped_horizontally {
        state.sleep_ticks = before.sleep_ticks.saturating_add(1).min(SLEEP_TICKS);
        state.sleeping = state.sleep_ticks == SLEEP_TICKS;
    } else {
        state.sleep_ticks = 0;
        state.sleeping = false;
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Axis {
    X,
    Y,
    Z,
}

#[derive(Clone, Copy)]
struct LateralBodyContact {
    axis: Axis,
    first_before_second: bool,
    penetration_um: u64,
    gap_um: u64,
    closing_travel_um: u64,
}

impl Axis {
    const fn index(self) -> usize {
        match self {
            Self::X => 0,
            Self::Y => 1,
            Self::Z => 2,
        }
    }

    const fn component(self, vector: FixedMicrometers3) -> i64 {
        match self {
            Self::X => vector.x,
            Self::Y => vector.y,
            Self::Z => vector.z,
        }
    }

    const fn set_component(self, vector: &mut FixedMicrometers3, value: i64) {
        match self {
            Self::X => vector.x = value,
            Self::Y => vector.y = value,
            Self::Z => vector.z = value,
        }
    }

    const fn orthogonal(self) -> [Self; 2] {
        match self {
            Self::X => [Self::Y, Self::Z],
            Self::Y => [Self::X, Self::Z],
            Self::Z => [Self::X, Self::Y],
        }
    }
}

fn lateral_body_contact(
    first_body: &RigidBodyDescriptor,
    first_before: RigidBodyState,
    first_after: RigidBodyState,
    second_body: &RigidBodyDescriptor,
    second_before: RigidBodyState,
    second_after: RigidBodyState,
) -> Option<LateralBodyContact> {
    let first_before_bounds = body_aabb(first_body, first_before);
    let second_before_bounds = body_aabb(second_body, second_before);
    if bounds_overlap_on_axis(first_before_bounds, second_before_bounds, Axis::X)
        && bounds_overlap_on_axis(first_before_bounds, second_before_bounds, Axis::Z)
    {
        return None;
    }
    let first_after_bounds = body_aabb(first_body, first_after);
    let second_after_bounds = body_aabb(second_body, second_after);
    let x = axis_body_contact(
        Axis::X,
        first_before_bounds,
        first_after_bounds,
        second_before_bounds,
        second_after_bounds,
    )
    .filter(|contact| {
        overlaps_on_orthogonal_axes_at_contact(
            *contact,
            first_before_bounds,
            first_after_bounds,
            second_before_bounds,
            second_after_bounds,
        )
    });
    let z = axis_body_contact(
        Axis::Z,
        first_before_bounds,
        first_after_bounds,
        second_before_bounds,
        second_after_bounds,
    )
    .filter(|contact| {
        overlaps_on_orthogonal_axes_at_contact(
            *contact,
            first_before_bounds,
            first_after_bounds,
            second_before_bounds,
            second_after_bounds,
        )
    });
    match (x, z) {
        (Some(x), Some(z)) => {
            let x_order = u128::from(x.gap_um).saturating_mul(u128::from(z.closing_travel_um));
            let z_order = u128::from(z.gap_um).saturating_mul(u128::from(x.closing_travel_um));
            Some(if x_order <= z_order { x } else { z })
        }
        (Some(contact), None) | (None, Some(contact)) => Some(contact),
        (None, None) => None,
    }
}

const fn bounds_overlap_on_axis(
    first: (FixedMicrometers3, FixedMicrometers3),
    second: (FixedMicrometers3, FixedMicrometers3),
    axis: Axis,
) -> bool {
    let first = axis_bounds(first, axis);
    let second = axis_bounds(second, axis);
    first.0 < second.1 && second.0 < first.1
}

fn overlaps_on_orthogonal_axes_at_contact(
    contact: LateralBodyContact,
    first_before: (FixedMicrometers3, FixedMicrometers3),
    first_after: (FixedMicrometers3, FixedMicrometers3),
    second_before: (FixedMicrometers3, FixedMicrometers3),
    second_after: (FixedMicrometers3, FixedMicrometers3),
) -> bool {
    contact.axis.orthogonal().into_iter().all(|axis| {
        let first = scaled_axis_bounds(
            first_before,
            first_after,
            axis,
            contact.gap_um,
            contact.closing_travel_um,
        );
        let second = scaled_axis_bounds(
            second_before,
            second_after,
            axis,
            contact.gap_um,
            contact.closing_travel_um,
        );
        first.0 < second.1 && second.0 < first.1
    })
}

fn scaled_axis_bounds(
    before: (FixedMicrometers3, FixedMicrometers3),
    after: (FixedMicrometers3, FixedMicrometers3),
    axis: Axis,
    time_numerator: u64,
    time_denominator: u64,
) -> (i128, i128) {
    let scale = i128::from(time_denominator);
    let time = i128::from(time_numerator);
    let interpolate = |before: i64, after: i64| {
        i128::from(before).saturating_mul(scale).saturating_add(
            i128::from(after)
                .saturating_sub(i128::from(before))
                .saturating_mul(time),
        )
    };
    (
        interpolate(axis.component(before.0), axis.component(after.0)),
        interpolate(axis.component(before.1), axis.component(after.1)),
    )
}

const fn axis_body_contact(
    axis: Axis,
    first_before: (FixedMicrometers3, FixedMicrometers3),
    first_after: (FixedMicrometers3, FixedMicrometers3),
    second_before: (FixedMicrometers3, FixedMicrometers3),
    second_after: (FixedMicrometers3, FixedMicrometers3),
) -> Option<LateralBodyContact> {
    let (first_min, first_max) = axis_bounds(first_before, axis);
    let (second_min, second_max) = axis_bounds(second_before, axis);
    let first_travel = axis
        .component(first_after.0)
        .saturating_sub(axis.component(first_before.0));
    let second_travel = axis
        .component(second_after.0)
        .saturating_sub(axis.component(second_before.0));
    let (first_before_second, gap, closing) = if first_max <= second_min {
        (
            true,
            second_min.saturating_sub(first_max),
            first_travel.saturating_sub(second_travel),
        )
    } else if second_max <= first_min {
        (
            false,
            first_min.saturating_sub(second_max),
            second_travel.saturating_sub(first_travel),
        )
    } else {
        return None;
    };
    if closing <= 0 || closing < gap {
        return None;
    }
    Some(LateralBodyContact {
        axis,
        first_before_second,
        penetration_um: closing.saturating_sub(gap).cast_unsigned(),
        gap_um: gap.cast_unsigned(),
        closing_travel_um: closing.cast_unsigned(),
    })
}

fn separate_lateral_contact(
    first_body: &RigidBodyDescriptor,
    first: &mut RigidBodyState,
    second_body: &RigidBodyDescriptor,
    second: &mut RigidBodyState,
    contact: LateralBodyContact,
) -> bool {
    let initial_first_velocity = contact.axis.component(first.linear_velocity_um_per_second);
    let initial_second_velocity = contact.axis.component(second.linear_velocity_um_per_second);
    let approaching = if contact.first_before_second {
        initial_first_velocity > initial_second_velocity
    } else {
        initial_second_velocity > initial_first_velocity
    };
    if contact.penetration_um == 0 && !approaching {
        return false;
    }
    let total_mass = u128::from(first_body.mass_kg).saturating_add(u128::from(second_body.mass_kg));
    let penetration = u128::from(contact.penetration_um);
    let first_correction = penetration
        .saturating_mul(u128::from(second_body.mass_kg))
        .saturating_add(total_mass.saturating_sub(1))
        / total_mass;
    let first_correction = i64::try_from(first_correction).unwrap_or(i64::MAX);
    let second_correction = i64::try_from(penetration).unwrap_or(i64::MAX) - first_correction;
    let direction = if contact.first_before_second { -1 } else { 1 };
    let first_origin = contact
        .axis
        .component(first.translation_um)
        .saturating_add(direction * first_correction)
        .clamp(-MAX_WORLD_TRANSLATION_UM, MAX_WORLD_TRANSLATION_UM);
    let second_origin = contact
        .axis
        .component(second.translation_um)
        .saturating_sub(direction * second_correction)
        .clamp(-MAX_WORLD_TRANSLATION_UM, MAX_WORLD_TRANSLATION_UM);
    contact
        .axis
        .set_component(&mut first.translation_um, first_origin);
    contact
        .axis
        .set_component(&mut second.translation_um, second_origin);

    if approaching {
        let restitution = combined_response(
            first_body.restitution_per_mille,
            second_body.restitution_per_mille,
        );
        let (resolved_first_velocity, resolved_second_velocity) = dynamic_contact_velocities(
            first_body.mass_kg,
            initial_first_velocity,
            second_body.mass_kg,
            initial_second_velocity,
            restitution,
        );
        contact.axis.set_component(
            &mut first.linear_velocity_um_per_second,
            resolved_first_velocity,
        );
        contact.axis.set_component(
            &mut second.linear_velocity_um_per_second,
            resolved_second_velocity,
        );
        let initial_relative = initial_first_velocity.saturating_sub(initial_second_velocity);
        let resolved_relative = resolved_first_velocity.saturating_sub(resolved_second_velocity);
        apply_dynamic_friction(
            first_body,
            first,
            second_body,
            second,
            contact.axis,
            initial_relative.abs_diff(resolved_relative),
        );
    }
    first.integration_remainder[contact.axis.index()] = 0;
    second.integration_remainder[contact.axis.index()] = 0;
    first.sleep_ticks = 0;
    first.sleeping = false;
    second.sleep_ticks = 0;
    second.sleeping = false;
    true
}

fn apply_dynamic_friction(
    first_body: &RigidBodyDescriptor,
    first: &mut RigidBodyState,
    second_body: &RigidBodyDescriptor,
    second: &mut RigidBodyState,
    normal_axis: Axis,
    normal_relative_change: u64,
) {
    let friction = combined_response(
        first_body.friction_per_mille,
        second_body.friction_per_mille,
    );
    let reduction = u128::from(normal_relative_change).saturating_mul(u128::from(friction))
        / u128::from(RESPONSE_SCALE.cast_unsigned());
    let reduction = i64::try_from(reduction).unwrap_or(i64::MAX);
    for axis in normal_axis.orthogonal() {
        let first_velocity = axis.component(first.linear_velocity_um_per_second);
        let second_velocity = axis.component(second.linear_velocity_um_per_second);
        let relative = first_velocity.saturating_sub(second_velocity);
        let target_relative = approach_zero(relative, reduction);
        if target_relative == relative {
            continue;
        }
        let (resolved_first, resolved_second) = contact_velocities_for_relative(
            first_body.mass_kg,
            first_velocity,
            second_body.mass_kg,
            second_velocity,
            target_relative,
        );
        axis.set_component(&mut first.linear_velocity_um_per_second, resolved_first);
        axis.set_component(&mut second.linear_velocity_um_per_second, resolved_second);
        first.integration_remainder[axis.index()] = 0;
        second.integration_remainder[axis.index()] = 0;
    }
}

fn dynamic_contact_velocities(
    first_mass: u64,
    first_velocity: i64,
    second_mass: u64,
    second_velocity: i64,
    restitution_per_mille: u16,
) -> (i64, i64) {
    let relative = first_velocity.saturating_sub(second_velocity);
    let retained = i128::from(relative).saturating_mul(i128::from(restitution_per_mille))
        / i128::from(RESPONSE_SCALE);
    let target_relative = i64::try_from(retained.saturating_neg())
        .unwrap_or_else(|_| -relative.signum() * MAX_LINEAR_SPEED_UM_PER_SECOND);
    contact_velocities_for_relative(
        first_mass,
        first_velocity,
        second_mass,
        second_velocity,
        target_relative,
    )
}

fn contact_velocities_for_relative(
    first_mass: u64,
    first_velocity: i64,
    second_mass: u64,
    second_velocity: i64,
    target_relative: i64,
) -> (i64, i64) {
    let first_mass = i128::from(first_mass);
    let second_mass = i128::from(second_mass);
    let total_mass = first_mass.saturating_add(second_mass);
    let momentum = first_mass
        .saturating_mul(i128::from(first_velocity))
        .saturating_add(second_mass.saturating_mul(i128::from(second_velocity)));
    let target_relative = i128::from(target_relative);
    let first = momentum.saturating_add(second_mass.saturating_mul(target_relative)) / total_mass;
    let second = momentum.saturating_sub(first_mass.saturating_mul(target_relative)) / total_mass;
    (bounded_velocity(first), bounded_velocity(second))
}

fn bounded_velocity(value: i128) -> i64 {
    i64::try_from(value)
        .unwrap_or_else(|_| value.signum() as i64 * MAX_LINEAR_SPEED_UM_PER_SECOND)
        .clamp(
            -MAX_LINEAR_SPEED_UM_PER_SECOND,
            MAX_LINEAR_SPEED_UM_PER_SECOND,
        )
}

const fn axis_bounds(bounds: (FixedMicrometers3, FixedMicrometers3), axis: Axis) -> (i64, i64) {
    (axis.component(bounds.0), axis.component(bounds.1))
}

#[derive(Clone, Copy)]
struct StaticContact {
    origin: i64,
    friction_per_mille: u16,
    restitution_per_mille: u16,
}

fn integrate_axis(velocity: i64, remainder: u8) -> (i64, u8) {
    let numerator = velocity.saturating_add(i64::from(remainder));
    let delta = numerator.div_euclid(SERVER_PHYSICS_HZ);
    let remainder = numerator.rem_euclid(SERVER_PHYSICS_HZ);
    (delta, u8::try_from(remainder).unwrap_or_default())
}

fn sweep_static_axis(
    world: &World,
    body: &RigidBodyDescriptor,
    translation: FixedMicrometers3,
    axis: Axis,
    proposed: i64,
) -> Option<StaticContact> {
    let current = axis.component(translation);
    let direction = proposed.cmp(&current);
    if direction == std::cmp::Ordering::Equal {
        return None;
    }
    let positive = direction == std::cmp::Ordering::Greater;
    let surface = match (axis, positive) {
        (Axis::X, false) => &body.collision_left,
        (Axis::X, true) => &body.collision_right,
        (Axis::Y, false) => &body.collision_bottom,
        (Axis::Y, true) => &body.collision_top,
        (Axis::Z, false) => &body.collision_back,
        (Axis::Z, true) => &body.collision_front,
    };
    let orthogonal = axis.orthogonal();
    let mut nearest = None;
    for &surface_voxel in surface {
        let local = local_voxel_minimum(body, surface_voxel);
        let local_axis = axis.component(local);
        let local_face = if positive {
            local_axis.saturating_add(MICROMETERS_PER_VOXEL)
        } else {
            local_axis
        };
        let current_face = current.saturating_add(local_face);
        let proposed_face = proposed.saturating_add(local_face);
        let (first_candidate, last_candidate) =
            swept_candidate_cells(current_face, proposed_face, positive);
        let first_orthogonal = overlapped_cells(
            orthogonal[0]
                .component(translation)
                .saturating_add(orthogonal[0].component(local)),
        );
        let second_orthogonal = overlapped_cells(
            orthogonal[1]
                .component(translation)
                .saturating_add(orthogonal[1].component(local)),
        );
        let candidate_span = last_candidate.saturating_sub(first_candidate);
        for offset in 0..=candidate_span {
            let candidate = if positive {
                first_candidate.saturating_add(offset)
            } else {
                last_candidate.saturating_sub(offset)
            };
            let mut candidate_hit = false;
            for first in first_orthogonal.0..=first_orthogonal.1 {
                for second in second_orthogonal.0..=second_orthogonal.1 {
                    let mut coordinates = [0_i64; 3];
                    coordinates[axis.index()] = candidate;
                    coordinates[orthogonal[0].index()] = first;
                    coordinates[orthogonal[1].index()] = second;
                    let Ok(x) = i32::try_from(coordinates[0]) else {
                        return Some(out_of_bounds_contact(current));
                    };
                    let Ok(y) = i32::try_from(coordinates[1]) else {
                        return Some(out_of_bounds_contact(current));
                    };
                    let Ok(z) = i32::try_from(coordinates[2]) else {
                        return Some(out_of_bounds_contact(current));
                    };
                    let voxel = world.voxel(IVec3::new(x, y, z));
                    if !voxel.is_solid() {
                        continue;
                    }
                    candidate_hit = true;
                    let boundary = if positive {
                        candidate.saturating_mul(MICROMETERS_PER_VOXEL)
                    } else {
                        candidate
                            .saturating_add(1)
                            .saturating_mul(MICROMETERS_PER_VOXEL)
                    };
                    let contact = StaticContact {
                        origin: boundary.saturating_sub(local_face),
                        friction_per_mille: voxel.material.properties().friction_per_mille,
                        restitution_per_mille: voxel.material.properties().restitution_per_mille,
                    };
                    merge_contact(&mut nearest, contact, positive);
                }
            }
            if candidate_hit {
                break;
            }
        }
    }
    nearest
}

const fn swept_candidate_cells(
    current_face: i64,
    proposed_face: i64,
    positive: bool,
) -> (i64, i64) {
    if positive {
        (
            current_face.div_euclid(MICROMETERS_PER_VOXEL),
            proposed_face
                .saturating_sub(1)
                .div_euclid(MICROMETERS_PER_VOXEL),
        )
    } else {
        (
            proposed_face
                .saturating_sub(1)
                .div_euclid(MICROMETERS_PER_VOXEL),
            current_face
                .saturating_sub(1)
                .div_euclid(MICROMETERS_PER_VOXEL),
        )
    }
}

fn local_voxel_minimum(body: &RigidBodyDescriptor, position: IVec3) -> FixedMicrometers3 {
    FixedMicrometers3 {
        x: i64::from(position.x.saturating_sub(body.minimum.x)) * MICROMETERS_PER_VOXEL,
        y: i64::from(position.y.saturating_sub(body.minimum.y)) * MICROMETERS_PER_VOXEL,
        z: i64::from(position.z.saturating_sub(body.minimum.z)) * MICROMETERS_PER_VOXEL,
    }
}

const fn overlapped_cells(start: i64) -> (i64, i64) {
    (
        start.div_euclid(MICROMETERS_PER_VOXEL),
        start
            .saturating_add(MICROMETERS_PER_VOXEL - 1)
            .div_euclid(MICROMETERS_PER_VOXEL),
    )
}

fn merge_contact(nearest: &mut Option<StaticContact>, contact: StaticContact, positive: bool) {
    match nearest {
        Some(current) if current.origin == contact.origin => {
            current.friction_per_mille = current.friction_per_mille.max(contact.friction_per_mille);
            current.restitution_per_mille = current
                .restitution_per_mille
                .max(contact.restitution_per_mille);
        }
        Some(current)
            if (positive && current.origin <= contact.origin)
                || (!positive && current.origin >= contact.origin) => {}
        _ => *nearest = Some(contact),
    }
}

const fn out_of_bounds_contact(origin: i64) -> StaticContact {
    StaticContact {
        origin,
        friction_per_mille: 1_000,
        restitution_per_mille: 0,
    }
}

fn reflected_velocity(incoming: i64, restitution_per_mille: u16) -> i64 {
    if incoming.unsigned_abs() < MIN_BOUNCE_SPEED_UM_PER_SECOND.cast_unsigned()
        || restitution_per_mille == 0
    {
        return 0;
    }
    let reflected = -i128::from(incoming).saturating_mul(i128::from(restitution_per_mille))
        / i128::from(RESPONSE_SCALE);
    i64::try_from(reflected).unwrap_or_else(|_| -incoming.signum() * MAX_LINEAR_SPEED_UM_PER_SECOND)
}

fn apply_ground_friction(state: &mut RigidBodyState, normal_speed: u64, friction_per_mille: u16) {
    let reduction = u128::from(normal_speed).saturating_mul(u128::from(friction_per_mille))
        / u128::from(RESPONSE_SCALE.cast_unsigned());
    let reduction = i64::try_from(reduction).unwrap_or(i64::MAX);
    for (axis, remainder_index) in [(Axis::X, 0), (Axis::Z, 2)] {
        let velocity = axis.component(state.linear_velocity_um_per_second);
        let slowed = approach_zero(velocity, reduction);
        axis.set_component(&mut state.linear_velocity_um_per_second, slowed);
        if slowed == 0 {
            state.integration_remainder[remainder_index] = 0;
        }
    }
}

fn approach_zero(value: i64, amount: i64) -> i64 {
    match value.cmp(&0) {
        std::cmp::Ordering::Greater => value.saturating_sub(amount).max(0),
        std::cmp::Ordering::Less => value.saturating_add(amount).min(0),
        std::cmp::Ordering::Equal => 0,
    }
}

const fn combined_response(body: u16, surface: u16) -> u16 {
    u16::midpoint(body, surface)
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

struct CollisionSurfaces {
    bottom: Vec<IVec3>,
    top: Vec<IVec3>,
    left: Vec<IVec3>,
    right: Vec<IVec3>,
    back: Vec<IVec3>,
    front: Vec<IVec3>,
}

fn collision_surfaces(voxels: &[BodyVoxel]) -> CollisionSurfaces {
    let mut vertical = HashMap::<(i32, i32), (IVec3, IVec3)>::with_capacity(voxels.len());
    let mut lateral_x = HashMap::<(i32, i32), (IVec3, IVec3)>::with_capacity(voxels.len());
    let mut lateral_z = HashMap::<(i32, i32), (IVec3, IVec3)>::with_capacity(voxels.len());
    for body_voxel in voxels {
        vertical
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
        lateral_x
            .entry((body_voxel.position.y, body_voxel.position.z))
            .and_modify(|(left, right)| {
                if body_voxel.position.x < left.x {
                    *left = body_voxel.position;
                }
                if body_voxel.position.x > right.x {
                    *right = body_voxel.position;
                }
            })
            .or_insert((body_voxel.position, body_voxel.position));
        lateral_z
            .entry((body_voxel.position.x, body_voxel.position.y))
            .and_modify(|(back, front)| {
                if body_voxel.position.z < back.z {
                    *back = body_voxel.position;
                }
                if body_voxel.position.z > front.z {
                    *front = body_voxel.position;
                }
            })
            .or_insert((body_voxel.position, body_voxel.position));
    }
    let (bottom, top) = sorted_collision_pair(vertical);
    let (left, right) = sorted_collision_pair(lateral_x);
    let (back, front) = sorted_collision_pair(lateral_z);
    CollisionSurfaces {
        bottom,
        top,
        left,
        right,
        back,
        front,
    }
}

fn sorted_collision_pair(columns: HashMap<(i32, i32), (IVec3, IVec3)>) -> (Vec<IVec3>, Vec<IVec3>) {
    let mut columns = columns.into_iter().collect::<Vec<_>>();
    columns.sort_unstable_by_key(|(key, _surfaces)| *key);
    columns.into_iter().map(|(_key, surfaces)| surfaces).unzip()
}

fn weighted_response(weighted: u128, mass_kg: u64) -> u16 {
    let response = weighted / u128::from(mass_kg);
    u16::try_from(response.min(u128::from(RESPONSE_SCALE.cast_unsigned()))).unwrap_or(u16::MAX)
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

    fn one_voxel_body(id: BodyId, material: Material) -> RigidBodyDescriptor {
        RigidBodyDescriptor::from_replicated_voxels(
            id,
            vec![BodyVoxel {
                position: IVec3::new(0, 5, 0),
                voxel: Voxel::new(material),
            }],
            BodyLimits::default(),
        )
        .expect("one-voxel body")
    }

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
    fn material_response_and_collision_surfaces_are_canonical() {
        let voxels = vec![
            BodyVoxel {
                position: IVec3::new(0, 2, 0),
                voxel: Voxel::new(Material::Steel),
            },
            BodyVoxel {
                position: IVec3::new(1, 2, 0),
                voxel: Voxel::new(Material::Wood),
            },
        ];

        let body = RigidBodyDescriptor::from_replicated_voxels(1, voxels, BodyLimits::default())
            .expect("connected mixed-material body");

        assert_eq!(body.friction_per_mille, 435);
        assert_eq!(body.restitution_per_mille, 183);
        assert_eq!(body.fragmentation_per_mille, 365);
        assert_eq!(
            body.collision_bottom,
            vec![IVec3::new(0, 2, 0), IVec3::new(1, 2, 0)]
        );
        assert_eq!(body.collision_top, body.collision_bottom);
        assert_eq!(body.collision_left, vec![IVec3::new(0, 2, 0)]);
        assert_eq!(body.collision_right, vec![IVec3::new(1, 2, 0)]);
        assert_eq!(body.collision_back, body.collision_bottom);
        assert_eq!(body.collision_front, body.collision_bottom);
    }

    #[test]
    fn integer_impulse_changes_all_axes_and_wakes_a_sleeping_body() {
        let body = RigidBodyDescriptor::from_replicated_voxels(
            1,
            vec![BodyVoxel {
                position: IVec3::new(0, 2, 0),
                voxel: Voxel::new(Material::Concrete),
            }],
            BodyLimits::default(),
        )
        .expect("one concrete voxel");
        let mut state = RigidBodyState::at_spawn(&body);
        state.sleep_ticks = SLEEP_TICKS;
        state.sleeping = true;

        assert!(apply_linear_impulse(
            &body,
            &mut state,
            FixedImpulseMilliNewtonSeconds3 {
                x: 2_400,
                y: -4_800,
                z: 7_200,
            },
        ));
        assert_eq!(
            state.linear_velocity_um_per_second,
            FixedMicrometers3 {
                x: 1_000,
                y: -2_000,
                z: 3_000,
            }
        );
        assert_eq!(state.sleep_ticks, 0);
        assert!(!state.sleeping);
    }

    #[test]
    fn three_axis_integration_is_repeatable_and_preserves_remainders() {
        let body = RigidBodyDescriptor::from_replicated_voxels(
            1,
            vec![BodyVoxel {
                position: IVec3::new(0, 100, 0),
                voxel: Voxel::new(Material::Wood),
            }],
            BodyLimits::default(),
        )
        .expect("airborne body");
        let world = World::default();
        let mut first = RigidBodyState::at_spawn(&body);
        first.linear_velocity_um_per_second.x = 1_000_001;
        first.linear_velocity_um_per_second.z = -2_000_003;
        let mut second = first;

        for _ in 0..120 {
            let _ = step_rigid_body(&world, &body, &mut first);
            let _ = step_rigid_body(&world, &body, &mut second);
        }

        assert_eq!(first, second);
        assert_ne!(first.translation_um.x, 0);
        assert_ne!(first.translation_um.z, 0);
        let physics_hz = u8::try_from(SERVER_PHYSICS_HZ).expect("physics Hz fits wire remainder");
        assert!(first.integration_remainder[0] < physics_hz);
        assert!(first.integration_remainder[2] < physics_hz);
    }

    #[test]
    fn high_speed_lateral_sweep_is_symmetric_across_negative_coordinates() {
        let position = IVec3::new(0, 5, 0);
        let body = RigidBodyDescriptor::from_replicated_voxels(
            1,
            vec![BodyVoxel {
                position,
                voxel: Voxel::new(Material::Wood),
            }],
            BodyLimits::default(),
        )
        .expect("one wood voxel");
        for (wall_x, velocity, expected_origin, expected_velocity) in [
            (2, 120_000_000, MICROMETERS_PER_VOXEL, -32_400_000),
            (-2, -120_000_000, -MICROMETERS_PER_VOXEL, 32_400_000),
        ] {
            let mut world = World::default();
            world.set_voxel(IVec3::new(wall_x, 5, 0), Voxel::new(Material::Glass));
            let mut state = RigidBodyState::at_spawn(&body);
            state.linear_velocity_um_per_second.x = velocity;

            let result = step_rigid_body(&world, &body, &mut state);

            assert!(result.collided_with_static);
            assert_eq!(state.translation_um.x, expected_origin);
            assert_eq!(state.linear_velocity_um_per_second.x, expected_velocity);
            assert!(state.translation_um.y < 5 * MICROMETERS_PER_VOXEL);
        }
    }

    #[test]
    fn upward_sweep_hits_ceiling_without_tunneling() {
        let position = IVec3::new(0, 1, 0);
        let body = RigidBodyDescriptor::from_replicated_voxels(
            1,
            vec![BodyVoxel {
                position,
                voxel: Voxel::new(Material::Concrete),
            }],
            BodyLimits::default(),
        )
        .expect("one concrete voxel");
        let mut world = World::default();
        world.set_voxel(IVec3::new(0, 3, 0), Voxel::new(Material::Steel));
        let mut state = RigidBodyState::at_spawn(&body);
        state.linear_velocity_um_per_second.y = 120_000_000;

        let result = step_rigid_body(&world, &body, &mut state);

        assert!(result.collided_with_static);
        assert_eq!(state.translation_um.y, 2 * MICROMETERS_PER_VOXEL);
        assert!(state.linear_velocity_um_per_second.y < 0);
    }

    #[test]
    fn grounded_friction_stops_horizontal_motion_and_allows_sleep() {
        let position = IVec3::new(0, 1, 0);
        let body = RigidBodyDescriptor::from_replicated_voxels(
            1,
            vec![BodyVoxel {
                position,
                voxel: Voxel::new(Material::Wood),
            }],
            BodyLimits::default(),
        )
        .expect("one wood voxel");
        let mut world = World::default();
        world.fill_box(
            IVec3::new(-32, 0, -2),
            IVec3::new(32, 0, 2),
            Voxel::new(Material::Soil),
        );
        let mut state = RigidBodyState::at_spawn(&body);
        state.linear_velocity_um_per_second.x = 2_000_000;

        for _ in 0..180 {
            let _ = step_rigid_body(&world, &body, &mut state);
        }

        assert_eq!(
            state.linear_velocity_um_per_second,
            FixedMicrometers3::default()
        );
        assert!(state.translation_um.x > 0);
        assert!(state.sleeping);
    }

    #[test]
    fn equal_mass_dynamic_sweeps_exchange_velocity_on_both_lateral_axes() {
        for axis in [Axis::X, Axis::Z] {
            let first_position = IVec3::new(0, 5, 0);
            let second_position = match axis {
                Axis::X => IVec3::new(3, 5, 0),
                Axis::Z => IVec3::new(0, 5, 3),
                Axis::Y => unreachable!("lateral fixture"),
            };
            let first = RigidBodyDescriptor::from_replicated_voxels(
                1,
                vec![BodyVoxel {
                    position: first_position,
                    voxel: Voxel::new(Material::Wood),
                }],
                BodyLimits::default(),
            )
            .expect("first dynamic body");
            let second = RigidBodyDescriptor::from_replicated_voxels(
                2,
                vec![BodyVoxel {
                    position: second_position,
                    voxel: Voxel::new(Material::Wood),
                }],
                BodyLimits::default(),
            )
            .expect("second dynamic body");
            let mut first_state = RigidBodyState::at_spawn(&first);
            let mut second_state = RigidBodyState::at_spawn(&second);
            axis.set_component(&mut first_state.linear_velocity_um_per_second, 120_000_000);
            axis.set_component(
                &mut second_state.linear_velocity_um_per_second,
                -120_000_000,
            );
            let bodies = BTreeMap::from([(first.id, first), (second.id, second)]);
            let mut states = BTreeMap::from([(1, first_state), (2, second_state)]);

            let report = step_rigid_bodies(&World::default(), &bodies, &mut states);

            assert_eq!(report.body_collisions, 1);
            assert_eq!(
                axis.component(states[&1].translation_um),
                MICROMETERS_PER_VOXEL
            );
            assert_eq!(
                axis.component(states[&2].translation_um),
                2 * MICROMETERS_PER_VOXEL
            );
            assert_eq!(
                axis.component(states[&1].linear_velocity_um_per_second),
                -26_400_000
            );
            assert_eq!(
                axis.component(states[&2].linear_velocity_um_per_second),
                26_400_000
            );
        }
    }

    #[test]
    fn lateral_impact_separates_and_wakes_an_unequal_sleeping_body() {
        let first = RigidBodyDescriptor::from_replicated_voxels(
            1,
            vec![BodyVoxel {
                position: IVec3::new(0, 5, 0),
                voxel: Voxel::new(Material::Wood),
            }],
            BodyLimits::default(),
        )
        .expect("light moving body");
        let second = RigidBodyDescriptor::from_replicated_voxels(
            2,
            vec![BodyVoxel {
                position: IVec3::new(2, 5, 0),
                voxel: Voxel::new(Material::Steel),
            }],
            BodyLimits::default(),
        )
        .expect("heavy sleeping body");
        let mut first_state = RigidBodyState::at_spawn(&first);
        first_state.linear_velocity_um_per_second.x = 120_000_000;
        let mut second_state = RigidBodyState::at_spawn(&second);
        second_state.sleep_ticks = SLEEP_TICKS;
        second_state.sleeping = true;
        let bodies = BTreeMap::from([(first.id, first), (second.id, second)]);
        let mut states = BTreeMap::from([(1, first_state), (2, second_state)]);

        let report = step_rigid_bodies(&World::default(), &bodies, &mut states);

        assert_eq!(report.body_collisions, 1);
        assert_eq!(report.bodies_woken, 1);
        assert!(!states[&2].sleeping);
        assert!(states[&1].linear_velocity_um_per_second.x < 0);
        assert!(states[&2].linear_velocity_um_per_second.x > 0);
        let first_bounds = body_aabb(&bodies[&1], states[&1]);
        let second_bounds = body_aabb(&bodies[&2], states[&2]);
        assert_eq!(first_bounds.1.x, second_bounds.0.x);
    }

    #[test]
    fn dynamic_contact_friction_reduces_tangential_slip_without_losing_momentum() {
        let first = RigidBodyDescriptor::from_replicated_voxels(
            1,
            vec![BodyVoxel {
                position: IVec3::new(0, 5, 0),
                voxel: Voxel::new(Material::Wood),
            }],
            BodyLimits::default(),
        )
        .expect("glancing first body");
        let second = RigidBodyDescriptor::from_replicated_voxels(
            2,
            vec![BodyVoxel {
                position: IVec3::new(3, 5, 0),
                voxel: Voxel::new(Material::Wood),
            }],
            BodyLimits::default(),
        )
        .expect("glancing second body");
        let mut first_state = RigidBodyState::at_spawn(&first);
        first_state.linear_velocity_um_per_second.x = 120_000_000;
        first_state.linear_velocity_um_per_second.z = 10_000_000;
        let mut second_state = RigidBodyState::at_spawn(&second);
        second_state.linear_velocity_um_per_second.x = -120_000_000;
        let bodies = BTreeMap::from([(first.id, first), (second.id, second)]);
        let mut states = BTreeMap::from([(1, first_state), (2, second_state)]);

        let report = step_rigid_bodies(&World::default(), &bodies, &mut states);

        assert_eq!(report.body_collisions, 1);
        assert_eq!(states[&1].linear_velocity_um_per_second.z, 5_000_000);
        assert_eq!(states[&2].linear_velocity_um_per_second.z, 5_000_000);
        assert_eq!(states[&1].integration_remainder[2], 0);
        assert_eq!(states[&2].integration_remainder[2], 0);
    }

    #[test]
    fn bounded_solver_passes_propagate_a_reverse_order_contact_chain() {
        let mut bodies = BTreeMap::new();
        let mut initial_states = BTreeMap::new();
        for index in 0..4_u64 {
            let position = IVec3::new(i32::try_from(index).expect("small chain"), 5, 0);
            let body = RigidBodyDescriptor::from_replicated_voxels(
                index + 1,
                vec![BodyVoxel {
                    position,
                    voxel: Voxel::new(Material::Concrete),
                }],
                BodyLimits::default(),
            )
            .expect("chain body");
            let mut state = RigidBodyState::at_spawn(&body);
            if index == 3 {
                state.linear_velocity_um_per_second.x = -120_000_000;
            }
            initial_states.insert(body.id, state);
            bodies.insert(body.id, body);
        }
        let mut first_run = initial_states.clone();
        let mut second_run = initial_states;

        let first_report = step_rigid_bodies(&World::default(), &bodies, &mut first_run);
        let second_report = step_rigid_bodies(&World::default(), &bodies, &mut second_run);

        assert_eq!(first_run, second_run);
        assert_eq!(first_report, second_report);
        assert_eq!(first_report.broad_phase_pairs, 5);
        assert!((3..=5 * MAX_BODY_SOLVER_PASSES).contains(&first_report.body_collisions));
        assert!(first_run[&1].linear_velocity_um_per_second.x < 0);
        for pair in [
            (&bodies[&1], first_run[&1], &bodies[&2], first_run[&2]),
            (&bodies[&2], first_run[&2], &bodies[&3], first_run[&3]),
            (&bodies[&3], first_run[&3], &bodies[&4], first_run[&4]),
        ] {
            let left = body_aabb(pair.0, pair.1);
            let right = body_aabb(pair.2, pair.3);
            assert!(
                left.1.x.saturating_sub(right.0.x) <= MICROMETERS_PER_VOXEL / 8,
                "bounded passes left excessive penetration: {left:?} versus {right:?}"
            );
        }
    }

    #[test]
    fn swept_corner_touch_without_orthogonal_overlap_does_not_false_collide() {
        let first = RigidBodyDescriptor::from_replicated_voxels(
            1,
            vec![BodyVoxel {
                position: IVec3::new(0, 5, 0),
                voxel: Voxel::new(Material::Wood),
            }],
            BodyLimits::default(),
        )
        .expect("x-moving body");
        let second = RigidBodyDescriptor::from_replicated_voxels(
            2,
            vec![BodyVoxel {
                position: IVec3::new(2, 5, 3),
                voxel: Voxel::new(Material::Wood),
            }],
            BodyLimits::default(),
        )
        .expect("z-moving body");
        let mut first_state = RigidBodyState::at_spawn(&first);
        first_state.linear_velocity_um_per_second.x = 120_000_000;
        let mut second_state = RigidBodyState::at_spawn(&second);
        second_state.linear_velocity_um_per_second.z = -240_000_000;
        let bodies = BTreeMap::from([(first.id, first), (second.id, second)]);
        let mut states = BTreeMap::from([(1, first_state), (2, second_state)]);

        let report = step_rigid_bodies(&World::default(), &bodies, &mut states);

        assert_eq!(report.broad_phase_pairs, 1);
        assert_eq!(report.body_collisions, 0);
        assert_eq!(states[&1].translation_um.x, 2 * MICROMETERS_PER_VOXEL);
        assert_eq!(states[&2].translation_um.z, -MICROMETERS_PER_VOXEL);
        assert_eq!(states[&1].linear_velocity_um_per_second.x, 120_000_000);
        assert_eq!(states[&2].linear_velocity_um_per_second.z, -240_000_000);
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
    fn bounded_horizontal_wire_states_are_valid_but_incoherent_sleep_is_not() {
        let mut state = RigidBodyState {
            translation_um: FixedMicrometers3::default(),
            linear_velocity_um_per_second: FixedMicrometers3::default(),
            orientation: FixedQuaternion::IDENTITY,
            angular_velocity_mrad_per_second: FixedMilliradians3::default(),
            integration_remainder: [0; 3],
            angular_integration_remainder: [0; 3],
            sleep_ticks: 0,
            sleeping: false,
        };
        assert!(valid_rigid_body_state(state));
        state.linear_velocity_um_per_second.x = 1;
        state.linear_velocity_um_per_second.z = -1;
        assert!(valid_rigid_body_state(state));
        state.sleeping = true;
        assert!(!valid_rigid_body_state(state));
        state.linear_velocity_um_per_second = FixedMicrometers3::default();
        state.sleep_ticks = SLEEP_TICKS;
        assert!(valid_rigid_body_state(state));
        state.sleeping = false;
        state.sleep_ticks = 0;
        state.linear_velocity_um_per_second.x = MAX_LINEAR_SPEED_UM_PER_SECOND + 1;
        assert!(!valid_rigid_body_state(state));
    }

    #[test]
    fn quaternion_and_angular_integration_are_canonical_and_repeatable() {
        let mut first = RigidBodyState {
            angular_velocity_mrad_per_second: FixedMilliradians3 {
                x: 1_200,
                y: -2_400,
                z: 3_600,
            },
            ..RigidBodyState::at_spawn(&one_voxel_body(1, Material::Wood))
        };
        let mut second = first;

        for _ in 0..120 {
            integrate_orientation(&mut first);
            integrate_orientation(&mut second);
            assert!(valid_fixed_quaternion(first.orientation));
        }

        assert_eq!(first, second);
        assert_ne!(first.orientation, FixedQuaternion::IDENTITY);
        first.orientation.w = -first.orientation.w;
        assert!(!valid_fixed_quaternion(first.orientation));
    }

    #[test]
    fn fixed_quaternion_rotates_vectors_and_its_conjugate_reverses_the_rotation() {
        let scale = i128::from(FIXED_QUATERNION_SCALE);
        let quarter_turn = normalize_quaternion([0, 0, scale, scale]);
        let rotated = rotate_fixed_vector(quarter_turn, [1_000, 0, 0]);
        let restored = rotate_fixed_vector(conjugate(quarter_turn), rotated);

        assert!(rotated[0].unsigned_abs() <= 1);
        assert!(rotated[1].abs_diff(1_000) <= 1);
        assert_eq!(rotated[2], 0);
        assert!(restored[0].abs_diff(1_000) <= 2);
        assert!(restored[1].unsigned_abs() <= 2);
        assert_eq!(restored[2], 0);
    }

    #[test]
    fn off_center_impulse_uses_inertia_and_wakes_angular_motion() {
        let body = RigidBodyDescriptor::from_replicated_voxels(
            1,
            vec![
                BodyVoxel {
                    position: IVec3::new(0, 5, 0),
                    voxel: Voxel::new(Material::Wood),
                },
                BodyVoxel {
                    position: IVec3::new(1, 5, 0),
                    voxel: Voxel::new(Material::Wood),
                },
            ],
            BodyLimits::default(),
        )
        .expect("two-voxel body");
        let mut state = RigidBodyState::at_spawn(&body);

        assert!(apply_impulse_at_local_point(
            &body,
            &mut state,
            FixedImpulseMilliNewtonSeconds3 {
                x: 0,
                y: 10_000,
                z: 0,
            },
            FixedMillimeters3 {
                x: 1_500,
                y: 500,
                z: 500,
            },
        ));

        assert!(state.linear_velocity_um_per_second.y > 0);
        assert!(state.angular_velocity_mrad_per_second.z > 0);
        assert_eq!(state.angular_velocity_mrad_per_second.x, 0);
        assert_eq!(state.angular_velocity_mrad_per_second.y, 0);
        assert!(valid_rigid_body_state(state));
        assert!(!state.sleeping);
    }
}
