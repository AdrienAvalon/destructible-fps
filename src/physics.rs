//! Fixed-unit authoritative rigid-body descriptors derived from detached voxel islands.

use crate::{
    DetachedIsland, IVec3, Voxel, World,
    structural::{ISLAND_FINGERPRINT_SEED, describe_island, mix_island_fingerprint},
};
use core::fmt;
use std::collections::{HashSet, VecDeque};

const MILLIMETERS_PER_VOXEL: i64 = 1_000;
const SQUARE_MILLIMETERS_PER_VOXEL: u128 = 1_000_000;
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
        Ok(Self {
            id: fingerprint,
            voxels,
            minimum,
            maximum,
            mass_kg,
            center_of_mass_mm,
            inertia_diagonal_kg_mm2,
        })
    }
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
}
