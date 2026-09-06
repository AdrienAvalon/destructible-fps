//! Solid-owned exact boundary rectangles, found by interval queries rather than dyadic subdivision.
//! Not yet a conforming triangle mesh or integrated legacy-world render path.

use super::{LocalBox, RefinedVolume, VOLUME_EDGE, VolumeError, WorkBudget, reserve_bounded};
use crate::Voxel;

pub const MAX_SURFACE_QUADS: usize = 32_768;
pub const MAX_SURFACE_VISITS: usize = 262_144;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Face {
    NegativeX,
    PositiveX,
    NegativeY,
    PositiveY,
    NegativeZ,
    PositiveZ,
}
impl Face {
    pub const ALL: [Self; 6] = [
        Self::NegativeX,
        Self::PositiveX,
        Self::NegativeY,
        Self::PositiveY,
        Self::NegativeZ,
        Self::PositiveZ,
    ];
    #[must_use]
    pub const fn index(self) -> usize {
        self as usize
    }
    #[must_use]
    pub const fn axis(self) -> usize {
        self.index() / 2
    }
    #[must_use]
    pub const fn positive(self) -> bool {
        self.index() % 2 == 1
    }
    #[must_use]
    pub const fn opposite(self) -> Self {
        Self::ALL[self.index() ^ 1]
    }
}

/// Rectangle on a solid/air boundary. Origin's face-axis coordinate is the plane; extents are
/// along (axis+1)%3 and (axis+2)%3. Fine offsets are exact and need not form a square.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SurfaceQuad {
    origin: [u16; 3],
    extent: [u16; 2],
    face: Face,
    voxel: Voxel,
}
impl SurfaceQuad {
    #[must_use]
    pub const fn origin(self) -> [u16; 3] {
        self.origin
    }
    #[must_use]
    pub const fn extent(self) -> [u16; 2] {
        self.extent
    }
    #[must_use]
    pub const fn face(self) -> Face {
        self.face
    }
    #[must_use]
    pub const fn voxel(self) -> Voxel {
        self.voxel
    }
    #[must_use]
    pub const fn area_units(self) -> u32 {
        self.extent[0] as u32 * self.extent[1] as u32
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SurfaceLimits {
    pub quads: usize,
    pub visits: usize,
}
impl Default for SurfaceLimits {
    fn default() -> Self {
        Self {
            quads: MAX_SURFACE_QUADS,
            visits: MAX_SURFACE_VISITS,
        }
    }
}
#[derive(Debug)]
pub struct VolumeSurface {
    pub quads: Vec<SurfaceQuad>,
    pub visits: usize,
}

impl RefinedVolume {
    /// Emits this page's solid-owned boundaries against six explicit known neighbors in `Face::ALL`
    /// order. Air means known empty space, not an unloaded neighbor. No internal material sheets.
    /// Run on an immutable worker snapshot, never on the network receive callback.
    /// # Errors
    /// Invalid budgets or exhausted output/work/vector allocation discard the entire candidate.
    pub fn surface(
        &self,
        neighbors: [&Self; 6],
        limits: SurfaceLimits,
    ) -> Result<VolumeSurface, VolumeError> {
        if !(1..=MAX_SURFACE_QUADS).contains(&limits.quads)
            || !(1..=MAX_SURFACE_VISITS).contains(&limits.visits)
        {
            return Err(VolumeError::InvalidLimits);
        }
        let mut work = WorkBudget::new(limits.visits);
        let mut quads = Vec::new();
        for leaf in self.leaves() {
            work.tick()?; // Charge even air leaves; empty regions do not hide a full scan.
            if !leaf.voxel().is_solid() {
                continue;
            }
            for face in Face::ALL {
                work.tick()?;
                let axis = face.axis();
                let mut minimum = leaf.bounds().minimum();
                let mut maximum = leaf.bounds().maximum();
                let plane = if face.positive() {
                    maximum[axis]
                } else {
                    minimum[axis]
                };
                let external = if face.positive() {
                    plane == VOLUME_EDGE
                } else {
                    plane == 0
                };
                let page = if external {
                    neighbors[face.index()]
                } else {
                    self
                };
                minimum[axis] = if face.positive() {
                    if external { 0 } else { plane }
                } else if external {
                    VOLUME_EDGE - 1
                } else {
                    plane - 1
                };
                maximum[axis] = minimum[axis] + 1;
                let query = LocalBox::new(minimum, maximum)?;
                page.visit_overlaps(query, &mut work, |neighbor, overlap| {
                    if neighbor.voxel().is_solid() {
                        return Ok(true);
                    }
                    if quads.len() == limits.quads {
                        return Err(VolumeError::SurfaceBudget);
                    }
                    reserve_bounded(&mut quads, 1, limits.quads)?;
                    let mut origin = overlap.minimum();
                    origin[axis] = plane;
                    let extent = std::array::from_fn(|tangent| {
                        let direction = (axis + 1 + tangent) % 3;
                        overlap.maximum()[direction] - overlap.minimum()[direction]
                    });
                    quads.push(SurfaceQuad {
                        origin,
                        extent,
                        face,
                        voxel: leaf.voxel(),
                    });
                    Ok(true)
                })?;
            }
        }
        Ok(VolumeSurface {
            quads,
            visits: work.visited,
        })
    }
}
