//! Canonical locally refined material volumes, with rectangular runs instead of dyadic cubes.
//!
//! Precision remains 256 units per page; physical interpretation belongs to a separate transform.
//! This is not yet a replacement for the legacy world's uniform cells.

pub mod codec;
mod edit;
mod query;
pub mod ray;
pub mod surface;

use crate::{Material, Voxel};
use core::fmt;
use std::sync::Arc;

pub const VOLUME_DEPTH: u8 = 8;
pub const VOLUME_EDGE: u16 = 1 << VOLUME_DEPTH;
pub const VOLUME_UNITS: u32 = 1 << (3 * VOLUME_DEPTH);
pub const MAX_VOLUME_LEAVES: usize = 8_192;
pub const MAX_EDIT_VISITS: usize = 65_536;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LocalBox {
    minimum: [u16; 3],
    maximum: [u16; 3],
}

impl LocalBox {
    pub const FULL: Self = Self {
        minimum: [0; 3],
        maximum: [VOLUME_EDGE; 3],
    };

    /// Half-open, nonempty integer bounds in the local page.
    /// # Errors
    /// Rejects empty, inverted or out-of-page boxes.
    pub fn new(minimum: [u16; 3], maximum: [u16; 3]) -> Result<Self, VolumeError> {
        if (0..3).any(|axis| minimum[axis] >= maximum[axis] || maximum[axis] > VOLUME_EDGE) {
            return Err(VolumeError::InvalidBox);
        }
        Ok(Self { minimum, maximum })
    }

    #[must_use]
    pub const fn minimum(self) -> [u16; 3] {
        self.minimum
    }
    #[must_use]
    pub const fn maximum(self) -> [u16; 3] {
        self.maximum
    }
    #[must_use]
    pub fn units(self) -> u32 {
        (0..3)
            .map(|axis| u32::from(self.maximum[axis] - self.minimum[axis]))
            .product()
    }
    #[must_use]
    pub fn intersection(self, other: Self) -> Option<Self> {
        Self::new(
            std::array::from_fn(|axis| self.minimum[axis].max(other.minimum[axis])),
            std::array::from_fn(|axis| self.maximum[axis].min(other.maximum[axis])),
        )
        .ok()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct VolumeLimits {
    pub leaves: usize,
    pub visits: usize,
}
impl Default for VolumeLimits {
    fn default() -> Self {
        Self {
            leaves: MAX_VOLUME_LEAVES,
            visits: MAX_EDIT_VISITS,
        }
    }
}
impl VolumeLimits {
    fn validate(self) -> Result<(), VolumeError> {
        if !(1..=MAX_VOLUME_LEAVES).contains(&self.leaves)
            || !(1..=MAX_EDIT_VISITS).contains(&self.visits)
        {
            return Err(VolumeError::InvalidLimits);
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VolumeError {
    InvalidBox,
    InvalidPoint,
    InvalidSegment,
    InvalidLimits,
    LeafBudget,
    VisitBudget,
    SurfaceBudget,
    RayBudget,
    Allocation,
    InvalidEncoding,
    NonCanonical,
}
impl fmt::Display for VolumeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "refined volume refused: {self:?}")
    }
}
impl std::error::Error for VolumeError {}

/// A rectangular run in a complete canonical Z-slab/Y-band/X-run partition. Fields stay private.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct VolumeLeaf {
    bounds: LocalBox,
    voxel: Voxel,
}
const _: () = assert!(size_of::<VolumeLeaf>() == 14);
impl VolumeLeaf {
    #[must_use]
    pub const fn voxel(self) -> Voxel {
        self.voxel
    }
    #[must_use]
    pub fn units(self) -> u32 {
        self.bounds.units()
    }
    #[must_use]
    pub const fn bounds(self) -> LocalBox {
        self.bounds
    }
}

#[derive(Clone, Debug)]
pub struct RefinedVolume {
    leaves: Arc<[VolumeLeaf]>,
    fingerprint: u128,
    material_units: [u32; 8],
}
impl RefinedVolume {
    #[must_use]
    pub fn uniform(voxel: Voxel) -> Self {
        Self::from_canonical(Arc::from([VolumeLeaf {
            bounds: LocalBox::FULL,
            voxel: canonical_voxel(voxel),
        }]))
    }

    fn from_canonical(leaves: Arc<[VolumeLeaf]>) -> Self {
        // New schema domain: the former Morton-v1 fingerprint is not reused.
        let mut fingerprint = 0x241e_b873_8f95_bae1_5317_20c4_f1ad_a702_u128;
        let mut material_units = [0; 8];
        for leaf in &*leaves {
            material_units[material_slot(leaf.voxel.material)] += leaf.units();
            let mut packed = u128::from(leaf.voxel.material as u8) << 96
                | u128::from(leaf.voxel.integrity) << 104;
            for (index, value) in leaf
                .bounds
                .minimum
                .into_iter()
                .chain(leaf.bounds.maximum)
                .enumerate()
            {
                packed |= u128::from(value) << (16 * index);
            }
            fingerprint = (fingerprint ^ packed)
                .rotate_left(31)
                .wrapping_mul(0x9e37_79b9_7f4a_7c15_d6e8_feb8_6659_fd93);
        }
        Self {
            leaves,
            fingerprint,
            material_units,
        }
    }

    #[must_use]
    pub fn leaves(&self) -> &[VolumeLeaf] {
        &self.leaves
    }
    #[must_use]
    pub const fn fingerprint(&self) -> u128 {
        self.fingerprint
    }
    #[must_use]
    pub const fn material_units(&self) -> &[u32; 8] {
        &self.material_units
    }
    #[must_use]
    pub const fn solid_units(&self) -> u32 {
        VOLUME_UNITS - self.material_units[0]
    }
    #[must_use]
    pub fn uniform_voxel(&self) -> Option<Voxel> {
        (self.leaves.len() == 1).then_some(self.leaves[0].voxel)
    }

    /// Exact density-weighted mass numerator in kg / `VOLUME_UNITS` for a metre page.
    /// Retain fractions until summing a complete fragment, not rounding individual leaves.
    #[must_use]
    pub fn mass_numerator(&self) -> u64 {
        self.leaves
            .iter()
            .map(|leaf| {
                u64::from(leaf.units()) * u64::from(leaf.voxel.material.properties().density_kg_m3)
            })
            .sum()
    }

    /// Compatibility scan bounded by the maximum page leaf count, not intended for hot physics
    /// loops. Use `overlaps_solid_bounded` for interval skipping and an explicit work budget.
    #[must_use]
    pub fn overlaps_solid(&self, bounds: LocalBox) -> bool {
        self.leaves
            .iter()
            .any(|leaf| leaf.voxel.is_solid() && leaf.bounds.intersection(bounds).is_some())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EditStats {
    pub visited: usize,
    /// Peak combined live leaves in page/slab/band candidate vectors, not retained source readers.
    pub peak_leaves: usize,
    /// Combined vector capacities only; excludes metadata and final immutable Arc copy.
    pub scratch_capacity_bytes: usize,
    pub changed: bool,
}

struct WorkBudget {
    visited: usize,
    maximum: usize,
}
impl WorkBudget {
    const fn new(maximum: usize) -> Self {
        Self {
            visited: 0,
            maximum,
        }
    }
    const fn tick(&mut self) -> Result<(), VolumeError> {
        self.charge(1)
    }
    const fn charge(&mut self, steps: usize) -> Result<(), VolumeError> {
        if steps > self.maximum - self.visited {
            return Err(VolumeError::VisitBudget);
        }
        self.visited += steps;
        Ok(())
    }
}

fn reserve_bounded<T>(
    values: &mut Vec<T>,
    additional: usize,
    maximum: usize,
) -> Result<(), VolumeError> {
    if additional > maximum.saturating_sub(values.len()) {
        return Err(VolumeError::LeafBudget);
    }
    let required = values.len() + additional;
    if required > values.capacity() {
        let target = required
            .max(values.capacity().saturating_mul(2).max(8))
            .min(maximum);
        values
            .try_reserve_exact(target - values.len())
            .map_err(|_| VolumeError::Allocation)?;
    }
    Ok(())
}

// Called only on a sorted sibling slice (entire page for Z, one slab for Y).
fn group_end(leaves: &[VolumeLeaf], first: usize, axis: usize) -> usize {
    let coordinate = leaves[first].bounds.minimum[axis];
    first + leaves[first..].partition_point(|leaf| leaf.bounds.minimum[axis] == coordinate)
}

// Profiles ignore the axis being coalesced and outer axes, but retain all inner bounds/material.
fn same_profile(
    a: &[VolumeLeaf],
    b: &[VolumeLeaf],
    axis: usize,
    work: &mut WorkBudget,
) -> Result<bool, VolumeError> {
    work.tick()?;
    if a.len() != b.len() {
        return Ok(false);
    }
    for (left, right) in a.iter().zip(b) {
        work.tick()?;
        if left.voxel != right.voxel
            || (0..axis).any(|inner| {
                left.bounds.minimum[inner] != right.bounds.minimum[inner]
                    || left.bounds.maximum[inner] != right.bounds.maximum[inner]
            })
        {
            return Ok(false);
        }
    }
    Ok(true)
}

// Exhaustive so new Material variants require a deliberate accounting-schema decision.
const fn material_slot(material: Material) -> usize {
    match material {
        Material::Air => 0,
        Material::Soil => 1,
        Material::Stone => 2,
        Material::Wood => 3,
        Material::Brick => 4,
        Material::Concrete => 5,
        Material::Steel => 6,
        Material::Glass => 7,
    }
}
const fn canonical_voxel(voxel: Voxel) -> Voxel {
    if voxel.is_solid() { voxel } else { Voxel::AIR }
}

#[cfg(test)]
mod tests;
