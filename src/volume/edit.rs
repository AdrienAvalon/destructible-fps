use super::{
    EditStats, LocalBox, RefinedVolume, VOLUME_EDGE, VolumeError, VolumeLeaf, VolumeLimits,
    WorkBudget, canonical_voxel, group_end, reserve_bounded, same_profile,
};

impl RefinedVolume {
    /// Builds canonical X runs, Y bands and Z slabs in independent bounded scratch vectors.
    /// Original pages/readers are unchanged on refusal; uniform profiles merge exactly.
    /// # Errors
    /// Rejects invalid limits or exhausted leaf/work/vector-allocation budgets. The final Arc
    /// allocation follows the normal process allocator OOM policy, not a recoverable guarantee.
    pub fn replace_box(
        &self,
        bounds: LocalBox,
        voxel: super::Voxel,
        limits: VolumeLimits,
    ) -> Result<(Self, EditStats), VolumeError> {
        limits.validate()?;
        let mut builder = Builder::new(limits);
        let mut first = 0;
        while first < self.leaves.len() {
            builder.work.tick()?;
            let end = group_end(&self.leaves, first, 2);
            let source = &self.leaves[first..end];
            let source_bounds = source[0].bounds;
            if source_bounds.maximum[2] <= bounds.minimum[2]
                || source_bounds.minimum[2] >= bounds.maximum[2]
            {
                builder.copy_slab(source)?;
                first = end;
                continue;
            }
            let edges = splits(
                source_bounds.minimum[2],
                source_bounds.maximum[2],
                bounds,
                2,
            );
            for pair in edges.windows(2).filter(|pair| pair[0] < pair[1]) {
                builder.slab(source, pair[0], pair[1], bounds, canonical_voxel(voxel))?;
            }
            first = end;
        }
        let changed = builder.page.as_slice() != self.leaves();
        let stats = EditStats {
            visited: builder.work.visited,
            peak_leaves: builder.peak,
            scratch_capacity_bytes: (builder.page.capacity()
                + builder.slab.capacity()
                + builder.band.capacity())
                * size_of::<VolumeLeaf>(),
            changed,
        };
        let candidate = if changed {
            Self::from_canonical(builder.page.into())
        } else {
            self.clone()
        };
        Ok((candidate, stats))
    }
}

fn splits(low: u16, high: u16, cut: LocalBox, axis: usize) -> [u16; 4] {
    [
        low,
        cut.minimum[axis].clamp(low, high),
        cut.maximum[axis].clamp(low, high),
        high,
    ]
}

struct Builder {
    page: Vec<VolumeLeaf>,
    slab: Vec<VolumeLeaf>,
    band: Vec<VolumeLeaf>,
    last_slab: usize,
    last_band: usize,
    maximum: usize,
    work: WorkBudget,
    peak: usize,
}
impl Builder {
    const fn new(limits: VolumeLimits) -> Self {
        Self {
            page: Vec::new(),
            slab: Vec::new(),
            band: Vec::new(),
            last_slab: 0,
            last_band: 0,
            maximum: limits.leaves,
            work: WorkBudget::new(limits.visits),
            peak: 0,
        }
    }
    fn observe(&mut self) {
        self.peak = self
            .peak
            .max(self.page.len() + self.slab.len() + self.band.len());
    }

    // Unaffected profiles are already canonical. Copy their bounded payload directly, but still
    // charge every copied leaf and compare against the last candidate slab at the edit frontier.
    fn copy_slab(&mut self, source: &[VolumeLeaf]) -> Result<(), VolumeError> {
        self.slab.clear();
        self.band.clear();
        if !self.page.is_empty()
            && same_profile(&self.page[self.last_slab..], source, 2, &mut self.work)?
        {
            self.work.charge(source.len())?;
            for leaf in &mut self.page[self.last_slab..] {
                leaf.bounds.maximum[2] = source[0].bounds.maximum[2];
            }
        } else {
            reserve_bounded(&mut self.page, source.len(), self.maximum)?;
            self.work.charge(source.len())?;
            self.last_slab = self.page.len();
            self.page.extend_from_slice(source);
            self.observe();
        }
        Ok(())
    }

    fn slab(
        &mut self,
        source: &[VolumeLeaf],
        low_z: u16,
        high_z: u16,
        cut: LocalBox,
        voxel: super::Voxel,
    ) -> Result<(), VolumeError> {
        self.work.tick()?;
        self.slab.clear();
        self.last_band = 0;
        let affected = low_z >= cut.minimum[2] && high_z <= cut.maximum[2];
        let mut first = 0;
        while first < source.len() {
            self.work.tick()?;
            let end = group_end(source, first, 1);
            let bounds = source[first].bounds;
            let edges = if affected {
                splits(bounds.minimum[1], bounds.maximum[1], cut, 1)
            } else {
                [
                    bounds.minimum[1],
                    bounds.minimum[1],
                    bounds.maximum[1],
                    bounds.maximum[1],
                ]
            };
            for pair in edges.windows(2).filter(|pair| pair[0] < pair[1]) {
                self.band(
                    &source[first..end],
                    [0, pair[0], low_z],
                    [VOLUME_EDGE, pair[1], high_z],
                    affected && pair[0] >= cut.minimum[1] && pair[1] <= cut.maximum[1],
                    cut,
                    voxel,
                )?;
            }
            first = end;
        }
        if !self.page.is_empty()
            && same_profile(&self.page[self.last_slab..], &self.slab, 2, &mut self.work)?
        {
            for leaf in &mut self.page[self.last_slab..] {
                self.work.tick()?;
                leaf.bounds.maximum[2] = high_z;
            }
        } else {
            reserve_bounded(&mut self.page, self.slab.len(), self.maximum)?;
            self.last_slab = self.page.len();
            for &leaf in &self.slab {
                self.work.tick()?;
                self.page.push(leaf);
            }
            self.observe();
        }
        Ok(())
    }

    fn band(
        &mut self,
        source: &[VolumeLeaf],
        minimum: [u16; 3],
        maximum: [u16; 3],
        affected: bool,
        cut: LocalBox,
        voxel: super::Voxel,
    ) -> Result<(), VolumeError> {
        self.work.tick()?;
        self.band.clear();
        for &leaf in source {
            self.work.tick()?;
            let bounds = leaf.bounds;
            let edges = if affected {
                splits(bounds.minimum[0], bounds.maximum[0], cut, 0)
            } else {
                [
                    bounds.minimum[0],
                    bounds.minimum[0],
                    bounds.maximum[0],
                    bounds.maximum[0],
                ]
            };
            for pair in edges.windows(2).filter(|pair| pair[0] < pair[1]) {
                self.work.tick()?;
                let material = if affected && pair[0] >= cut.minimum[0] && pair[1] <= cut.maximum[0]
                {
                    voxel
                } else {
                    leaf.voxel
                };
                if let Some(last) = self.band.last_mut().filter(|last| last.voxel == material) {
                    last.bounds.maximum[0] = pair[1];
                } else {
                    reserve_bounded(
                        &mut self.band,
                        1,
                        self.maximum.min(usize::from(VOLUME_EDGE)),
                    )?;
                    self.band.push(VolumeLeaf {
                        bounds: LocalBox {
                            minimum: [pair[0], minimum[1], minimum[2]],
                            maximum: [pair[1], maximum[1], maximum[2]],
                        },
                        voxel: material,
                    });
                    self.observe();
                }
            }
        }
        if !self.slab.is_empty()
            && same_profile(&self.slab[self.last_band..], &self.band, 1, &mut self.work)?
        {
            for leaf in &mut self.slab[self.last_band..] {
                self.work.tick()?;
                leaf.bounds.maximum[1] = maximum[1];
            }
        } else {
            reserve_bounded(&mut self.slab, self.band.len(), self.maximum)?;
            self.last_band = self.slab.len();
            for &leaf in &self.band {
                self.work.tick()?;
                self.slab.push(leaf);
            }
            self.observe();
        }
        Ok(())
    }
}
