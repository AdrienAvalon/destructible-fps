//! Exact, solid-owned boundary squares. Different refinement levels share integer planes.
//! This extractor is deliberately separate from the legacy world's mesh and render workers.

use super::{LocalBox, RefinedVolume, VOLUME_EDGE, VolumeError};
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

/// Square on an exact solid/air boundary. Origin is the minimum corner; its face-axis coordinate
/// is the boundary plane, and the other two axes span `[origin, origin + edge]`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SurfaceQuad {
    origin: [u16; 3],
    edge: u16,
    face: Face,
    voxel: Voxel,
}

impl SurfaceQuad {
    #[must_use]
    pub const fn origin(self) -> [u16; 3] {
        self.origin
    }
    #[must_use]
    pub const fn edge(self) -> u16 {
        self.edge
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
        self.edge as u32 * self.edge as u32
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
    /// Extracts only this page's solid-owned surfaces. All six neighbors must be supplied in
    /// `Face::ALL` order; an explicitly uniform-air page denotes *known* empty space, not an
    /// unloaded neighbor. A solid/solid interface never emits an internal material sheet.
    ///
    /// The result is discarded on any refusal. Call this on an immutable worker snapshot, not in
    /// a network receive callback. Output squares can have T-junctions; this is not a conforming
    /// triangle manifold or a replacement collision representation.
    /// # Errors
    /// Rejects invalid budgets and exhausted work, geometry or allocation limits.
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
        let mut builder = SurfaceBuilder {
            page: self,
            neighbors,
            limits,
            result: VolumeSurface {
                quads: Vec::new(),
                visits: 0,
            },
        };
        // Bounded, fallible allocation; no partially produced geometry escapes on a refusal.
        builder
            .result
            .quads
            .try_reserve_exact(limits.quads.min(64))
            .map_err(|_| VolumeError::Allocation)?;
        for leaf in self.leaves().iter().filter(|leaf| leaf.voxel().is_solid()) {
            let bounds = leaf.bounds();
            for face in Face::ALL {
                let mut origin = bounds.minimum();
                if face.positive() {
                    origin[face.axis()] = bounds.maximum()[face.axis()];
                }
                builder.face(SurfaceQuad {
                    origin,
                    edge: leaf.edge(),
                    face,
                    voxel: leaf.voxel(),
                })?;
            }
        }
        Ok(builder.result)
    }
}

struct SurfaceBuilder<'a> {
    page: &'a RefinedVolume,
    neighbors: [&'a RefinedVolume; 6],
    limits: SurfaceLimits,
    result: VolumeSurface,
}

impl SurfaceBuilder<'_> {
    fn face(&mut self, quad: SurfaceQuad) -> Result<(), VolumeError> {
        if self.result.visits == self.limits.visits {
            return Err(VolumeError::VisitBudget);
        }
        self.result.visits += 1;
        let axis = quad.face.axis();
        let plane = quad.origin[axis];
        let external = if quad.face.positive() {
            plane == VOLUME_EDGE
        } else {
            plane == 0
        };
        let page = if external {
            self.neighbors[quad.face.index()]
        } else {
            self.page
        };
        let mut point = quad.origin;
        point[axis] = if quad.face.positive() {
            if external { 0 } else { plane }
        } else if external {
            VOLUME_EDGE - 1
        } else {
            plane - 1
        };
        let neighbor = page.leaf_at(point)?;
        if covers_face(neighbor.bounds(), quad) {
            if !neighbor.voxel().is_solid() {
                if self.result.quads.len() == self.limits.quads {
                    return Err(VolumeError::SurfaceBudget);
                }
                if self.result.quads.len() == self.result.quads.capacity() {
                    let target = (self.result.quads.capacity() * 2).min(self.limits.quads);
                    self.result
                        .quads
                        .try_reserve_exact(target - self.result.quads.len())
                        .map_err(|_| VolumeError::Allocation)?;
                }
                self.result.quads.push(quad);
            }
            return Ok(());
        }
        // Dyadic alignment guarantees a partial neighbor has a finer level; edge == 1 is covered.
        let edge = quad.edge / 2;
        let u = (axis + 1) % 3;
        let v = (axis + 2) % 3;
        for child in 0..4 {
            let mut origin = quad.origin;
            origin[u] += (child & 1) * edge;
            origin[v] += (child >> 1) * edge;
            self.face(SurfaceQuad {
                origin,
                edge,
                ..quad
            })?;
        }
        Ok(())
    }
}

fn covers_face(bounds: LocalBox, quad: SurfaceQuad) -> bool {
    (0..3).filter(|&axis| axis != quad.face.axis()).all(|axis| {
        bounds.minimum()[axis] <= quad.origin[axis]
            && bounds.maximum()[axis] >= quad.origin[axis] + quad.edge
    })
}
