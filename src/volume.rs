//! Canonical locally refined material volumes. A uniform metre needs one leaf, not 256³ voxels.
//!
//! Coordinates are dyadic local indices; physical interpretation belongs to a separate transform.
//! This is the fine-geometry core, not yet a replacement for the legacy world's uniform cells.

pub mod codec;
pub mod surface;

use crate::{Material, Voxel};
use core::fmt;
use std::sync::Arc;

pub const VOLUME_DEPTH: u8 = 8;
pub const VOLUME_EDGE: u16 = 1 << VOLUME_DEPTH;
pub const VOLUME_UNITS: u32 = 1 << (3 * VOLUME_DEPTH);
pub const MAX_VOLUME_LEAVES: usize = 8_192;
pub const MAX_EDIT_VISITS: usize = 65_536;
// A canonical Morton prefix can retain seven pending siblings at each tree level before
// subsequent leaves coalesce it. Bound that carry explicitly, even for a one-leaf result.
const CANONICAL_CARRY: usize = 7 * VOLUME_DEPTH as usize;

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

    /// Half-open, nonempty bounds in the local dyadic lattice.
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
    InvalidLimits,
    LeafBudget,
    VisitBudget,
    SurfaceBudget,
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

/// A disjoint dyadic cube in a complete, Morton-ordered partition. Fields stay private so callers
/// cannot manufacture a leaf outside the validated partition or skip canonical coalescing.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct VolumeLeaf {
    start: u32,
    depth: u8,
    voxel: Voxel,
}

const _: () = assert!(size_of::<VolumeLeaf>() == 8);

impl VolumeLeaf {
    #[must_use]
    pub const fn depth(self) -> u8 {
        self.depth
    }
    #[must_use]
    pub const fn voxel(self) -> Voxel {
        self.voxel
    }
    #[must_use]
    pub const fn units(self) -> u32 {
        1 << (3 * (VOLUME_DEPTH - self.depth))
    }
    #[must_use]
    pub const fn edge(self) -> u16 {
        VOLUME_EDGE >> self.depth
    }
    #[must_use]
    pub fn bounds(self) -> LocalBox {
        let minimum = unmorton(self.start);
        LocalBox {
            minimum,
            maximum: minimum.map(|value| value + self.edge()),
        }
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
            start: 0,
            depth: 0,
            voxel: canonical_voxel(voxel),
        }]))
    }

    fn from_canonical(leaves: Arc<[VolumeLeaf]>) -> Self {
        let mut fingerprint = 0xb6db_4d95_af68_cba9_613d_85b9_dced_7f81_u128;
        let mut material_units = [0; 8];
        for leaf in &*leaves {
            material_units[material_slot(leaf.voxel.material)] += leaf.units();
            let packed = u128::from(leaf.start)
                | (u128::from(leaf.depth) << 32)
                | (u128::from(leaf.voxel.material as u8) << 40)
                | (u128::from(leaf.voxel.integrity) << 48);
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

    /// Exact density-weighted volume numerator, in kg / `VOLUME_UNITS` for a metre-sized page.
    /// Consumers must retain the fraction until summing a complete fragment, not round each leaf.
    #[must_use]
    pub fn mass_numerator(&self) -> u64 {
        self.leaves
            .iter()
            .map(|leaf| {
                u64::from(leaf.units()) * u64::from(leaf.voxel.material.properties().density_kg_m3)
            })
            .sum()
    }

    /// # Errors
    /// Rejects coordinates outside the half-open local page.
    pub fn leaf_at(&self, point: [u16; 3]) -> Result<VolumeLeaf, VolumeError> {
        if point.iter().any(|&value| value >= VOLUME_EDGE) {
            return Err(VolumeError::InvalidPoint);
        }
        let address = morton(point);
        let index = self.leaves.partition_point(|leaf| leaf.start <= address) - 1;
        Ok(self.leaves[index])
    }

    /// Exact intersection against disjoint solid leaf boxes; no coarse whole-page cover fallback.
    #[must_use]
    pub fn overlaps_solid(&self, bounds: LocalBox) -> bool {
        self.leaves
            .iter()
            .any(|leaf| leaf.voxel.is_solid() && leaf.bounds().intersection(bounds).is_some())
    }

    /// Creates an independent bounded candidate; the original and every retained reader stay
    /// unchanged on a visit, leaf or vector-allocation refusal. Uniform siblings coalesce exactly.
    /// The final standard-library Arc allocation follows the process allocator's OOM policy.
    /// # Errors
    /// Reports invalid limits or an exhausted edit budget without installing a partial volume.
    pub fn replace_box(
        &self,
        bounds: LocalBox,
        voxel: Voxel,
        limits: VolumeLimits,
    ) -> Result<(Self, EditStats), VolumeError> {
        limits.validate()?;
        let mut builder = LeafBuilder::new(limits.leaves)?;
        let mut visited = 0;
        for &leaf in &*self.leaves {
            edit_leaf(
                leaf,
                bounds,
                canonical_voxel(voxel),
                &mut builder,
                &mut visited,
                limits.visits,
            )?;
        }
        let peak_leaves = builder.peak;
        let scratch_capacity_bytes = builder.leaves.capacity() * size_of::<VolumeLeaf>();
        let leaves = builder.finish()?;
        let changed = leaves.as_slice() != &*self.leaves;
        let candidate = if changed {
            Self::from_canonical(leaves.into())
        } else {
            self.clone()
        };
        Ok((
            candidate,
            EditStats {
                visited,
                peak_leaves,
                scratch_capacity_bytes,
                changed,
            },
        ))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EditStats {
    pub visited: usize,
    pub peak_leaves: usize,
    /// Leaf-vector capacity only; excludes allocator metadata and the final immutable Arc copy.
    pub scratch_capacity_bytes: usize,
    pub changed: bool,
}

fn edit_leaf(
    leaf: VolumeLeaf,
    bounds: LocalBox,
    voxel: Voxel,
    builder: &mut LeafBuilder,
    visited: &mut usize,
    maximum_visits: usize,
) -> Result<(), VolumeError> {
    if *visited == maximum_visits {
        return Err(VolumeError::VisitBudget);
    }
    *visited += 1;
    if leaf.voxel == voxel {
        return builder.push(leaf);
    }
    let leaf_bounds = leaf.bounds();
    let Some(intersection) = leaf_bounds.intersection(bounds) else {
        return builder.push(leaf);
    };
    if intersection == leaf_bounds {
        return builder.push(VolumeLeaf { voxel, ..leaf });
    }
    let child_depth = leaf.depth + 1;
    let span = 1 << (3 * (VOLUME_DEPTH - child_depth));
    for child in 0..8 {
        edit_leaf(
            VolumeLeaf {
                start: leaf.start + child * span,
                depth: child_depth,
                voxel: leaf.voxel,
            },
            bounds,
            voxel,
            builder,
            visited,
            maximum_visits,
        )?;
    }
    Ok(())
}

struct LeafBuilder {
    leaves: Vec<VolumeLeaf>,
    maximum: usize,
    peak: usize,
}

impl LeafBuilder {
    fn new(maximum: usize) -> Result<Self, VolumeError> {
        let mut leaves = Vec::new();
        leaves
            .try_reserve_exact((maximum + CANONICAL_CARRY).min(64))
            .map_err(|_| VolumeError::Allocation)?;
        Ok(Self {
            leaves,
            maximum,
            peak: 0,
        })
    }

    fn push(&mut self, leaf: VolumeLeaf) -> Result<(), VolumeError> {
        if self.leaves.len() == self.maximum + CANONICAL_CARRY {
            return Err(VolumeError::LeafBudget);
        }
        if self.leaves.len() == self.leaves.capacity() {
            let target = (self.leaves.capacity() * 2).min(self.maximum + CANONICAL_CARRY);
            self.leaves
                .try_reserve_exact(target - self.leaves.len())
                .map_err(|_| VolumeError::Allocation)?;
        }
        self.leaves.push(leaf);
        self.peak = self.peak.max(self.leaves.len());
        while let Some(parent) = coalesced_tail(&self.leaves) {
            self.leaves.truncate(self.leaves.len() - 8);
            self.leaves.push(parent);
        }
        Ok(())
    }

    fn finish(self) -> Result<Vec<VolumeLeaf>, VolumeError> {
        if self.leaves.len() > self.maximum {
            return Err(VolumeError::LeafBudget);
        }
        Ok(self.leaves)
    }
}

fn coalesced_tail(leaves: &[VolumeLeaf]) -> Option<VolumeLeaf> {
    let siblings = leaves.get(leaves.len().checked_sub(8)?..)?;
    let first = siblings[0];
    if first.depth == 0 || !first.start.is_multiple_of(first.units() * 8) {
        return None;
    }
    if siblings.iter().enumerate().any(|(index, leaf)| {
        leaf.depth != first.depth
            || leaf.voxel != first.voxel
            || leaf.start != first.start + u32::try_from(index).unwrap_or(8) * first.units()
    }) {
        return None;
    }
    Some(VolumeLeaf {
        depth: first.depth - 1,
        ..first
    })
}

// Exhaustiveness forces a deliberate volume-schema update if a new Material variant is added;
// no unchecked enum discriminant can silently grow beyond the fixed eight-slot accounting table.
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

fn morton(point: [u16; 3]) -> u32 {
    let mut result = 0;
    for bit in 0..VOLUME_DEPTH {
        for (axis, &value) in point.iter().enumerate() {
            result |= u32::from((value >> bit) & 1) << (3 * usize::from(bit) + axis);
        }
    }
    result
}

fn unmorton(address: u32) -> [u16; 3] {
    std::array::from_fn(|axis| {
        let mut value = 0;
        for bit in 0..VOLUME_DEPTH {
            value |=
                u16::try_from((address >> (3 * usize::from(bit) + axis)) & 1).unwrap_or(0) << bit;
        }
        value
    })
}

#[cfg(test)]
mod tests;
