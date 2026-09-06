//! Bounded shading-only recognition of nearly planar terraces on isolated homogeneous fragments.
//! The exact material boundary, triangle positions/indices and physical queries remain untouched.
use super::{FineMeshError, WorkMeter};
use crate::{
    Material,
    volume::{
        LocalBox, RefinedVolume,
        surface::{Face, SurfaceQuad},
    },
};

const MAX_RESIDUAL: f64 = 6.0; // 6/256 m vertical fit error, not a geometry displacement allowance.
const MAX_RISER: u16 = 8;

pub(super) fn shading_normal(
    quad: SurfaceQuad,
    terrace: Option<TerraceNormal>,
    volume: &RefinedVolume,
    work: &WorkMeter,
) -> Result<[f32; 3], FineMeshError> {
    let mut geometric = [0.0; 3];
    geometric[quad.face().axis()] = if quad.face().positive() { 1.0 } else { -1.0 };
    if let Some(terrace) = terrace {
        work.charge(5)?;
        Ok(terrace.for_quad(quad, volume, work)?.unwrap_or(geometric))
    } else {
        Ok(geometric)
    }
}

#[derive(Clone, Copy, Debug)]
pub(super) struct TerraceNormal {
    slope: [f64; 2],
    intercept: f64,
    normal: [f32; 3],
}

#[derive(Default)]
struct Moments {
    area: f64,
    x: f64,
    z: f64,
    y: f64,
    xx: f64,
    zz: f64,
    xz: f64,
    xy: f64,
    zy: f64,
}
impl Moments {
    fn add(&mut self, origin: [u16; 3], extent: [u16; 2]) {
        // PositiveY tangents are Z then X. Integrate over the rectangle, NOT just its centroid:
        // E[x²]=center²+width²/12 makes the least-squares fit independent of coplanar subdivision.
        let depth = f64::from(extent[0]);
        let width = f64::from(extent[1]);
        let area = width * depth;
        let x = f64::from(origin[0]) + width / 2.0;
        let z = f64::from(origin[2]) + depth / 2.0;
        let y = f64::from(origin[1]);
        self.area += area;
        self.x = area.mul_add(x, self.x);
        self.z = area.mul_add(z, self.z);
        self.y = area.mul_add(y, self.y);
        self.xx = area.mul_add(x.mul_add(x, width * width / 12.0), self.xx);
        self.zz = area.mul_add(z.mul_add(z, depth * depth / 12.0), self.zz);
        self.xz = (area * x).mul_add(z, self.xz);
        self.xy = (area * x).mul_add(y, self.xy);
        self.zy = (area * z).mul_add(y, self.zy);
    }

    #[allow(clippy::cast_possible_truncation)] // Render-only bounded unit normal; no replicated float.
    fn plane(&self) -> Option<TerraceNormal> {
        if self.area < 256.0 {
            return None;
        }
        let xx = self.xx - self.x * self.x / self.area;
        let zz = self.zz - self.z * self.z / self.area;
        let xz = self.xz - self.x * self.z / self.area;
        let xy = self.xy - self.x * self.y / self.area;
        let zy = self.zy - self.z * self.y / self.area;
        let determinant = xz.mul_add(-xz, xx * zz);
        if !determinant.is_finite() || determinant <= (xx + zz).powi(2) * 1e-8 {
            return None;
        }
        let slope = [
            zy.mul_add(-xz, xy * zz) / determinant,
            xy.mul_add(-xz, zy * xx) / determinant,
        ];
        let squared = slope[0].mul_add(slope[0], slope[1] * slope[1]);
        if !squared.is_finite() || !(1e-6..=1.0).contains(&squared) {
            return None; // Flat faces already have correct normals; steep faces keep hard edges.
        }
        let intercept = slope[1].mul_add(-self.z, slope[0].mul_add(-self.x, self.y)) / self.area;
        if !intercept.is_finite() {
            return None;
        }
        let length = (1.0 + squared).sqrt();
        Some(TerraceNormal {
            slope,
            intercept,
            normal: [
                (-slope[0] / length) as f32,
                (1.0 / length) as f32,
                (-slope[1] / length) as f32,
            ],
        })
    }
}

impl TerraceNormal {
    fn near_plane(self, quad: SurfaceQuad) -> bool {
        let axis = quad.face().axis();
        let u = (axis + 1) % 3;
        let v = (axis + 2) % 3;
        for corner in 0..4 {
            let mut point = quad.origin().map(f64::from);
            point[u] = f64::from(quad.extent()[0]).mul_add(f64::from(corner & 1), point[u]);
            point[v] = f64::from(quad.extent()[1]).mul_add(f64::from(corner >> 1), point[v]);
            let height =
                self.slope[0].mul_add(point[0], self.slope[1].mul_add(point[2], self.intercept));
            if (point[1] - height).abs() > MAX_RESIDUAL {
                return false;
            }
        }
        true
    }

    pub(super) fn for_quad(
        self,
        quad: SurfaceQuad,
        volume: &RefinedVolume,
        work: &WorkMeter,
    ) -> Result<Option<[f32; 3]>, FineMeshError> {
        if quad.face() == Face::PositiveY {
            return Ok(Some(self.normal));
        }
        let axis = quad.face().axis();
        if axis == 1 || quad.origin()[1] == 0 {
            return Ok(None); // Undersides and full-height footprint sides must remain hard.
        }
        let height = quad.extent()[usize::from(axis == 2)];
        let alignment = self.normal[axis] * if quad.face().positive() { 1.0 } else { -1.0 };
        if height > MAX_RISER || alignment <= 0.001 || !self.near_plane(quad) {
            return Ok(None);
        }
        // Surface rectangles can split a full-height outer side into short upper pieces. Require
        // the ENTIRE neighboring footprint strip to be solid at the base before calling it a riser.
        let mut low = quad.origin();
        if !quad.face().positive() {
            let Some(coordinate) = low[axis].checked_sub(1) else {
                return Ok(None);
            };
            low[axis] = coordinate;
        }
        low[1] = 0;
        let mut high = low;
        high[1] = 1;
        high[axis] += 1;
        high[2 - axis] += quad.extent()[usize::from(axis == 0)];
        let strip = LocalBox::new(low, high)?;
        work.charge(1)?;
        let remaining = work.limit - work.used.get();
        if remaining == 0 {
            work.charge(1)?; // Mark exhausted before attempting any volume traversal.
        }
        let mut filled = 0;
        // This strip is one lattice unit wide and high. X-oriented strips visit at most
        // L runs + two groups; Z-oriented strips visit at most L slabs with one band/run each.
        // A complete integer partition therefore needs at most 3*L visits, not 3*page_leaves.
        let strip_visits = 3 * usize::from(quad.extent()[usize::from(axis == 0)]);
        let visits = volume.visit_solid_bounded(strip, remaining.min(strip_visits), |leaf| {
            filled += leaf.bounds().intersection(strip).map_or(0, LocalBox::units);
            filled < strip.units()
        })?;
        work.charge(visits)?;
        Ok((filled == strip.units()).then_some(self.normal))
    }
}

/// Constant scratch space, charged to the existing mesh-job meter. Budget errors propagate rather
/// than partially smoothing a candidate. Shape rejection preserves the original geometric normals.
pub(super) fn fit(
    volume: &RefinedVolume,
    quads: &[SurfaceQuad],
    work: &WorkMeter,
) -> Result<Option<TerraceNormal>, FineMeshError> {
    let mut material = None;
    let mut bottom = 256;
    for leaf in volume.leaves() {
        work.charge(1)?;
        let voxel = leaf.voxel();
        if !voxel.is_solid() {
            continue;
        }
        let low = leaf.bounds().minimum();
        let high = leaf.bounds().maximum();
        if !matches!(
            voxel.material,
            Material::Brick | Material::Concrete | Material::Stone
        ) || material.is_some_and(|m| m != voxel)
            || low[0] == 0
            || low[2] == 0
            || high[0] == 256
            || high[2] == 256
            || high[1] == 256
        {
            return Ok(None);
        }
        material = Some(voxel);
        bottom = bottom.min(low[1]);
    }
    if bottom != 0 {
        return Ok(None);
    }
    let mut moments = Moments::default();
    for quad in quads {
        work.charge(1)?;
        if quad.face() == Face::NegativeY && quad.origin()[1] != 0 {
            return Ok(None); // Includes interior cavities and overhanging material.
        }
        if quad.face() == Face::PositiveY {
            moments.add(quad.origin(), quad.extent());
        }
    }
    let Some(plane) = moments.plane() else {
        return Ok(None);
    };
    for quad in quads {
        work.charge(5)?;
        if quad.face() == Face::PositiveY && !plane.near_plane(*quad) {
            return Ok(None);
        }
    }
    Ok(Some(plane))
}

#[cfg(test)]
mod tests;
