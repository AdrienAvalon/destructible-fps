use super::{LocalBox, RefinedVolume, VOLUME_EDGE, VolumeError, VolumeLeaf, WorkBudget, group_end};

impl RefinedVolume {
    // Shared physical-query consumer. Visits include air and group work. A visitor can observe
    // a prefix before exhaustion, so it must stage results locally and discard them on error.
    pub(crate) fn visit_solid_bounded(
        &self,
        bounds: LocalBox,
        maximum_visits: usize,
        mut visit: impl FnMut(VolumeLeaf) -> bool,
    ) -> Result<usize, VolumeError> {
        if !(1..=3 * super::MAX_VOLUME_LEAVES).contains(&maximum_visits) {
            return Err(VolumeError::InvalidLimits);
        }
        let mut work = WorkBudget::new(maximum_visits);
        self.visit_overlaps(bounds, &mut work, |leaf, _| {
            Ok(!leaf.voxel().is_solid() || visit(leaf))
        })?;
        Ok(work.visited)
    }

    /// Exact solid overlap with interval skipping, early exit and a caller-visible budget.
    /// # Errors
    /// Rejects zero/excessive work budgets or exhaustion; never converts exhaustion into empty
    /// space or a silently solid fallback. The maximum is three steps per possible page leaf.
    pub fn overlaps_solid_bounded(
        &self,
        bounds: LocalBox,
        maximum_visits: usize,
    ) -> Result<bool, VolumeError> {
        if !(1..=3 * super::MAX_VOLUME_LEAVES).contains(&maximum_visits) {
            return Err(VolumeError::InvalidLimits);
        }
        let mut work = WorkBudget::new(maximum_visits);
        let mut occupied = false;
        self.visit_overlaps(bounds, &mut work, |leaf, _| {
            occupied = leaf.voxel().is_solid();
            Ok(!occupied)
        })?;
        Ok(occupied)
    }

    /// Finds one canonical run with bounded binary searches through Z/Y groups and X intervals.
    /// # Errors
    /// Rejects a point outside the half-open page.
    pub fn leaf_at(&self, point: [u16; 3]) -> Result<VolumeLeaf, VolumeError> {
        if point.iter().any(|&coordinate| coordinate >= VOLUME_EDGE) {
            return Err(VolumeError::InvalidPoint);
        }
        let mut siblings = self.leaves();
        for axis in [2, 1] {
            let end = siblings.partition_point(|leaf| leaf.bounds.minimum[axis] <= point[axis]);
            let coordinate = siblings[end - 1].bounds.minimum[axis];
            let first =
                siblings[..end].partition_point(|leaf| leaf.bounds.minimum[axis] < coordinate);
            siblings = &siblings[first..end];
        }
        let index = siblings.partition_point(|leaf| leaf.bounds.minimum[0] <= point[0]) - 1;
        Ok(siblings[index])
    }

    // Every intersected slab/band/run is charged, including air. Binary skip/group searches have
    // logarithmic cost bounded by MAX_VOLUME_LEAVES. No full N-leaf scan for every extracted face.
    pub(super) fn visit_overlaps(
        &self,
        bounds: LocalBox,
        work: &mut WorkBudget,
        mut visit: impl FnMut(VolumeLeaf, LocalBox) -> Result<bool, VolumeError>,
    ) -> Result<(), VolumeError> {
        let mut z = self
            .leaves
            .partition_point(|leaf| leaf.bounds.maximum[2] <= bounds.minimum[2]);
        while z < self.leaves.len() && self.leaves[z].bounds.minimum[2] < bounds.maximum[2] {
            work.tick()?;
            let next_z = group_end(&self.leaves, z, 2);
            let slab = &self.leaves[z..next_z];
            let mut y = slab.partition_point(|leaf| leaf.bounds.maximum[1] <= bounds.minimum[1]);
            while y < slab.len() && slab[y].bounds.minimum[1] < bounds.maximum[1] {
                work.tick()?;
                let next_y = group_end(slab, y, 1);
                let band = &slab[y..next_y];
                let x = band.partition_point(|leaf| leaf.bounds.maximum[0] <= bounds.minimum[0]);
                for &leaf in band[x..]
                    .iter()
                    .take_while(|leaf| leaf.bounds.minimum[0] < bounds.maximum[0])
                {
                    work.tick()?;
                    // The three interval filters prove a nonempty exact intersection.
                    if let Some(intersection) = bounds.intersection(leaf.bounds)
                        && !visit(leaf, intersection)?
                    {
                        return Ok(());
                    }
                }
                y = next_y;
            }
            z = next_z;
        }
        Ok(())
    }
}
