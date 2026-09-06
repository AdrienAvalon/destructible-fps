//! Bounded render-only authoring metadata. Never alters material, integrity or physical geometry.
use super::{FineMeshError, WorkMeter};
use crate::{
    IVec3, Material,
    world::{
        geometry::{GeometryCell, RefinedWorld},
        query::StaticGeometry,
    },
};

pub const MAX_FINISH_CELLS: usize = 64;
pub const MAX_FINISH_LEAVES: usize = 8_192;
/// Distinct from ordinary -1 and the coarse mesher's 0..1 wall-depth interpolation.
pub const CUT_CORE_MARKER: f32 = -2.0;

#[derive(Clone, Debug, Default)]
pub struct SurfaceFinishes {
    entries: Vec<(IVec3, GeometryCell)>,
    fingerprint: u128,
}

impl SurfaceFinishes {
    /// Explicitly author cut tops on sorted unique, homogeneous fine masonry cells.
    /// # Errors
    /// Rejects oversized lists/leaf totals, duplicates, uniform or non-masonry source cells.
    pub fn cut_tops(world: &RefinedWorld, positions: &[IVec3]) -> Result<Self, FineMeshError> {
        if positions.len() > MAX_FINISH_CELLS || positions.windows(2).any(|p| p[0] >= p[1]) {
            return Err(FineMeshError::SurfaceFinish);
        }
        let mut result = Self::default();
        result
            .entries
            .try_reserve_exact(positions.len())
            .map_err(|_| FineMeshError::Allocation)?;
        let mut leaves = 0;
        let mut fingerprint = 0x6375_742d_746f_7073_2d76_3100_u128;
        for &position in positions {
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
            {
                fingerprint =
                    (fingerprint ^ u128::from(byte)).wrapping_mul(0x0100_0000_0000_0000_0000_013b);
            }
            result.entries.push((position, cell));
        }
        if !positions.is_empty() {
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
        for (position, expected) in &self.entries {
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

    pub(super) fn contains(
        &self,
        position: IVec3,
        work: &WorkMeter,
    ) -> Result<bool, FineMeshError> {
        work.charge(7)?; // Upper bound for binary search in at most 64 entries.
        Ok(self
            .entries
            .binary_search_by_key(&position, |(p, _)| *p)
            .is_ok())
    }
}
