//! Bounded render-only authoring metadata. Never alters material, integrity or physical geometry.
use super::{FineMeshError, WorkMeter};
use crate::{
    IVec3, Material,
    volume::surface::Face,
    world::{
        geometry::{GeometryCell, RefinedWorld},
        query::StaticGeometry,
    },
};

pub const MAX_FINISH_CELLS: usize = 64;
pub const MAX_FINISH_LEAVES: usize = 8_192;
/// Distinct from ordinary -1 and the coarse mesher's 0..1 wall-depth interpolation.
pub const CUT_CORE_MARKER: f32 = -2.0;

/// Authored surface provenance, not damage or a reconstruction of fracture history.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FinishPolicy {
    CutTop,
    /// Preserve one original vertical side and the underside of a grounded fragment.
    /// The constructor rejects Y faces; smoothed top terraces remain cut on every side.
    CutTopAndSides(Face),
}

impl FinishPolicy {
    fn key(self) -> Result<u8, FineMeshError> {
        match self {
            Self::CutTop => Ok(0),
            Self::CutTopAndSides(face) if face.axis() != 1 => {
                Ok(1 + u8::try_from(face.index()).map_err(|_| FineMeshError::SurfaceFinish)?)
            }
            Self::CutTopAndSides(_) => Err(FineMeshError::SurfaceFinish),
        }
    }

    pub(super) fn marks(self, face: Face, normal_y: f32) -> bool {
        normal_y > 0.5
            || matches!(self, Self::CutTopAndSides(intact) if face.axis() != 1 && face != intact)
    }
}

#[derive(Clone, Debug, Default)]
pub struct SurfaceFinishes {
    entries: Vec<(IVec3, GeometryCell, FinishPolicy)>,
    fingerprint: u128,
}

impl SurfaceFinishes {
    /// Explicitly author cut tops on sorted unique, homogeneous fine masonry cells.
    /// # Errors
    /// Rejects oversized lists/leaf totals, duplicates, uniform or non-masonry source cells.
    pub fn cut_tops(world: &RefinedWorld, positions: &[IVec3]) -> Result<Self, FineMeshError> {
        if positions.len() > MAX_FINISH_CELLS {
            return Err(FineMeshError::SurfaceFinish);
        }
        let mut policies = Vec::new();
        policies
            .try_reserve_exact(positions.len())
            .map_err(|_| FineMeshError::Allocation)?;
        policies.extend(positions.iter().map(|&p| (p, FinishPolicy::CutTop)));
        Self::with_policies(world, &policies)
    }

    /// Bind sorted, unique, explicitly authored finish policies to exact immutable source pages.
    /// # Errors
    /// Rejects invalid policies and the same source/size violations as `cut_tops`.
    pub fn with_policies(
        world: &RefinedWorld,
        policies: &[(IVec3, FinishPolicy)],
    ) -> Result<Self, FineMeshError> {
        if policies.len() > MAX_FINISH_CELLS || policies.windows(2).any(|p| p[0].0 >= p[1].0) {
            return Err(FineMeshError::SurfaceFinish);
        }
        let mut result = Self::default();
        result
            .entries
            .try_reserve_exact(policies.len())
            .map_err(|_| FineMeshError::Allocation)?;
        let mut leaves = 0;
        let mut fingerprint = 0x6375_742d_6661_6365_732d_7632_u128;
        for &(position, policy) in policies {
            let policy_key = policy.key()?;
            let cell = world.cell(position);
            let volume = cell.volume().ok_or(FineMeshError::SurfaceFinish)?;
            leaves += volume.leaves().len();
            if leaves > MAX_FINISH_LEAVES {
                return Err(FineMeshError::SurfaceFinish);
            }
            let mut material = None;
            for leaf in volume.leaves().iter().filter(|l| l.voxel().is_solid()) {
                let m = leaf.voxel().material;
                if ![Material::Brick, Material::Concrete].contains(&m)
                    || material.is_some_and(|old| old != m)
                {
                    return Err(FineMeshError::SurfaceFinish);
                }
                material = Some(m);
            }
            if material.is_none() {
                return Err(FineMeshError::SurfaceFinish);
            }
            // A reproducibility/stale-result key, not authentication or a physical world hash.
            for byte in [position.x, position.y, position.z]
                .into_iter()
                .flat_map(i32::to_le_bytes)
                .chain(volume.fingerprint().to_le_bytes())
                .chain([policy_key])
            {
                fingerprint =
                    (fingerprint ^ u128::from(byte)).wrapping_mul(0x0100_0000_0000_0000_0000_013b);
            }
            result.entries.push((position, cell, policy));
        }
        if !policies.is_empty() {
            result.fingerprint = fingerprint;
        }
        Ok(result)
    }

    #[must_use]
    pub const fn fingerprint(&self) -> u128 {
        self.fingerprint
    }

    pub(super) fn validate(
        &self,
        world: &impl StaticGeometry,
        work: &WorkMeter,
    ) -> Result<(), FineMeshError> {
        for (position, expected, _) in &self.entries {
            let actual = world.geometry_cell(*position);
            work.charge(
                1 + expected.volume().map_or(0, |v| v.leaves().len())
                    + actual.volume().map_or(0, |v| v.leaves().len()),
            )?;
            if actual != *expected {
                return Err(FineMeshError::SurfaceFinish);
            }
        }
        Ok(())
    }

    pub(super) fn policy(
        &self,
        position: IVec3,
        work: &WorkMeter,
    ) -> Result<Option<FinishPolicy>, FineMeshError> {
        work.charge(7)?; // Upper bound for binary search in at most 64 entries.
        Ok(self
            .entries
            .binary_search_by_key(&position, |(p, _, _)| *p)
            .ok()
            .map(|i| self.entries[i].2))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terrace_risers_remain_core_even_on_the_preserved_vertical_side() {
        for intact in [
            Face::NegativeX,
            Face::PositiveX,
            Face::NegativeZ,
            Face::PositiveZ,
        ] {
            let policy = FinishPolicy::CutTopAndSides(intact);
            assert!(policy.marks(intact, 0.8));
            assert!(!policy.marks(intact, 0.0));
            assert!(!policy.marks(Face::NegativeY, -1.0));
            assert!(policy.marks(intact.opposite(), 0.0));
            assert!(policy.marks(Face::PositiveY, 1.0));
            assert!(!FinishPolicy::CutTop.marks(intact.opposite(), 0.0));
            assert!(!FinishPolicy::CutTop.marks(Face::PositiveY, 0.5));
        }
    }
}
