//! Exact fine/static surfaces and bounded integration with uniform Surface Nets terrain.
//!
//! Fine occupancy is never replaced by a coarse proxy; failed candidates are never published.
//! Edge subdivisions depend on the complete source geometry, not the requested chunk set.

use super::{CpuMesh, Vertex, material_surface, voxel_damage};
use crate::{
    CHUNK_EDGE, IVec3, Voxel,
    volume::{LocalBox, RefinedVolume, VolumeError, surface::SurfaceLimits},
    world::{geometry::GeometryCell, query::StaticGeometry},
};
use std::{cell::Cell, collections::HashMap, fmt};

pub mod finishes;
pub mod fixture;
mod hybrid;
mod normals;
pub use hybrid::{hybrid_dirty_chunks, mesh_hybrid_chunks, mesh_hybrid_chunks_with_finishes};

pub const MAX_FINE_MESH_CHUNKS: usize = 16;
pub const MAX_FINE_MESH_VERTICES: usize = 262_144;
pub const MAX_FINE_MESH_INDICES: usize = 786_432;
pub const MAX_FINE_MESH_QUADS: usize = 32_768;
pub const MAX_FINE_MESH_WORK: usize = 4_194_304;
pub const MAX_FINE_MESH_LINES: usize = 32_768;
/// Existing GPU vertices use absolute f32 metres. Half-lattice fan centres remain exact here.
pub const FINE_RENDER_EXTENT_METRES: i32 = 16_384;

#[derive(Clone, Copy, Debug)]
pub struct FineMeshLimits {
    pub vertices: usize,
    pub indices: usize,
    pub quads: usize,
    pub work: usize,
    pub lines: usize,
}
impl Default for FineMeshLimits {
    fn default() -> Self {
        Self {
            vertices: MAX_FINE_MESH_VERTICES,
            indices: MAX_FINE_MESH_INDICES,
            quads: MAX_FINE_MESH_QUADS,
            work: MAX_FINE_MESH_WORK,
            lines: MAX_FINE_MESH_LINES,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct FineMeshReport {
    pub quads: usize,
    pub vertices: usize,
    pub indices: usize,
    pub work: usize,
    pub lines: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FineMeshError {
    InvalidLimits,
    ChunkSet,
    CoordinatePrecision,
    OutputBudget,
    WorkBudget,
    LineBudget,
    Allocation,
    Surface(VolumeError),
    DerivedInvariant,
    SurfaceFinish,
}
impl fmt::Display for FineMeshError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "fine mesh refused: {self:?}")
    }
}
impl std::error::Error for FineMeshError {}
impl From<VolumeError> for FineMeshError {
    fn from(value: VolumeError) -> Self {
        Self::Surface(value)
    }
}

#[derive(Debug)]
pub struct FineMeshBatch {
    pub meshes: Vec<(IVec3, CpuMesh)>,
    pub report: FineMeshReport,
}

/// Extracts actual uniform/fine boundaries into the ordinary GPU vertex contract on a worker.
///
/// Natural smoothing is deliberately NOT substituted for exact geometry on this path. Existing
/// playable worlds continue using the hybrid coarse mesher until a shared transition is proved.
///
/// # Errors
/// Rejects unordered/duplicate/oversized chunk sets, precision loss and aggregate work/output
/// exhaustion. Failure drops the whole candidate, including preceding chunks. Vector/map growth
/// is fallible; uniform-volume Arc creation follows the process allocator's OOM policy.
pub fn mesh_fine_chunks(
    world: &impl StaticGeometry,
    chunks: &[IVec3],
    limits: FineMeshLimits,
) -> Result<FineMeshBatch, FineMeshError> {
    mesh_chunks(world, chunks, limits, None, None)
}

fn mesh_chunks(
    world: &impl StaticGeometry,
    chunks: &[IVec3],
    limits: FineMeshLimits,
    refined: Option<&crate::world::geometry::RefinedWorld>,
    finishes: Option<&finishes::SurfaceFinishes>,
) -> Result<FineMeshBatch, FineMeshError> {
    for (value, hard) in [
        (limits.vertices, MAX_FINE_MESH_VERTICES),
        (limits.indices, MAX_FINE_MESH_INDICES),
        (limits.quads, MAX_FINE_MESH_QUADS),
        (limits.work, MAX_FINE_MESH_WORK),
        (limits.lines, MAX_FINE_MESH_LINES),
    ] {
        if !(1..=hard).contains(&value) {
            return Err(FineMeshError::InvalidLimits);
        }
    }
    if chunks.is_empty()
        || chunks.len() > MAX_FINE_MESH_CHUNKS
        || chunks.windows(2).any(|pair| pair[0] >= pair[1])
    {
        return Err(FineMeshError::ChunkSet);
    }
    // Check before any multiplication, halo lookup, output allocation or world traversal.
    if chunks.iter().any(|p| {
        [p.x, p.y, p.z].into_iter().any(|v| {
            !(-FINE_RENDER_EXTENT_METRES / CHUNK_EDGE..FINE_RENDER_EXTENT_METRES / CHUNK_EDGE)
                .contains(&v)
        })
    }) {
        return Err(FineMeshError::CoordinatePrecision);
    }
    let work = WorkMeter::new(limits.work);
    if let Some(finishes) = finishes {
        finishes.validate(world, &work)?;
    }
    let hybrid = refined
        .map(|refined| hybrid::HybridSource::new(world, refined, chunks, &work))
        .transpose()?;
    let mut builder = Builder {
        world,
        work: &work,
        limits,
        report: FineMeshReport::default(),
        lines: HashMap::new(),
        finishes,
    };
    let mut meshes = Vec::new();
    meshes
        .try_reserve_exact(chunks.len())
        .map_err(|_| FineMeshError::Allocation)?;
    for &chunk in chunks {
        let mut mesh = CpuMesh::default();
        let mut derived = hybrid.as_ref().map(|_| {
            super::DerivedCellCache::new(IVec3::new(
                chunk.x * CHUNK_EDGE,
                chunk.y * CHUNK_EDGE,
                chunk.z * CHUNK_EDGE,
            ))
        });
        for z in 0..CHUNK_EDGE {
            for y in 0..CHUNK_EDGE {
                for x in 0..CHUNK_EDGE {
                    builder.charge(1)?;
                    let position = IVec3::new(
                        chunk.x * CHUNK_EDGE + x,
                        chunk.y * CHUNK_EDGE + y,
                        chunk.z * CHUNK_EDGE + z,
                    );
                    let cell = world.geometry_cell(position);
                    if cell.solid_units() != 0 {
                        if let (Some(hybrid), Some(derived)) = (&hybrid, &mut derived) {
                            builder.append_hybrid(&mut mesh, position, &cell, hybrid, derived)?;
                        } else {
                            builder.append_cell(&mut mesh, position, &cell, [false; 6])?;
                        }
                    }
                }
            }
        }
        meshes.push((chunk, mesh));
    }
    work.check()?;
    builder.report.work = work.used.get();
    Ok(FineMeshBatch {
        meshes,
        report: builder.report,
    })
}

// One line within a metre interval, with two fixed global lattice coordinates. The variable
// coordinate is the interval origin, independent of the emitting rectangle or its orientation.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
struct Line {
    axis: usize,
    origin: [i32; 3],
}
type Cuts = [bool; 257];

struct Builder<'a, G> {
    world: &'a G,
    work: &'a WorkMeter,
    limits: FineMeshLimits,
    report: FineMeshReport,
    lines: HashMap<Line, Cuts>,
    finishes: Option<&'a finishes::SurfaceFinishes>,
}
impl<G: StaticGeometry> Builder<'_, G> {
    fn charge(&self, count: usize) -> Result<(), FineMeshError> {
        self.work.charge(count)
    }

    #[allow(clippy::many_single_char_names)] // Axis/tangent notation matches SurfaceQuad.
    fn append_cell(
        &mut self,
        mesh: &mut CpuMesh,
        p: IVec3,
        cell: &GeometryCell,
        transition: [bool; 6],
    ) -> Result<(), FineMeshError> {
        self.charge(6)?;
        let cut_top = self
            .finishes
            .map(|s| s.contains(p, self.work))
            .transpose()?
            .unwrap_or(false);
        let volume = page(cell);
        let neighbors = std::array::from_fn::<_, 6, _>(|i| {
            let mut v = [p.x, p.y, p.z];
            v[i / 2] += if i % 2 == 0 { -1 } else { 1 };
            if transition[i] {
                RefinedVolume::uniform(Voxel::AIR)
            } else {
                page(&self.world.geometry_cell(IVec3::new(v[0], v[1], v[2])))
            }
        });
        let surface = volume.surface(
            neighbors.each_ref(),
            SurfaceLimits {
                quads: (self.limits.quads - self.report.quads).max(1),
                visits: (self.limits.work - self.work.used.get())
                    .clamp(1, crate::volume::surface::MAX_SURFACE_VISITS),
            },
        )?;
        self.charge(surface.visits)?;
        if surface.quads.len() > self.limits.quads - self.report.quads {
            return Err(FineMeshError::OutputBudget);
        }
        self.report.quads += surface.quads.len();
        let terrace = if cell.volume().is_some() {
            normals::fit(&volume, &surface.quads, self.work)?
        } else {
            None
        };
        for quad in surface.quads {
            self.charge(1)?;
            let a = quad.face().axis();
            let u = (a + 1) % 3;
            let v = (a + 2) % 3;
            let q = quad.origin();
            let lo = std::array::from_fn(|i| [p.x, p.y, p.z][i] * 256 + i32::from(q[i]));
            let mut hi = lo;
            hi[u] += i32::from(quad.extent()[0]);
            hi[v] += i32::from(quad.extent()[1]);
            let mut corners = [lo; 4];
            corners[1][u] = hi[u];
            corners[2] = hi;
            corners[3][v] = hi[v];
            if !quad.face().positive() {
                corners.reverse();
            }
            let center = std::array::from_fn(|i| (lo[i] as f32 + hi[i] as f32) / 512.0);
            let normal = normals::shading_normal(quad, terrace, &volume, self.work)?;
            let finish = if cut_top && normal[1] > 0.5 {
                finishes::CUT_CORE_MARKER
            } else {
                -1.0
            };
            let first = self.vertex(mesh, center, normal, quad.voxel(), finish)?;
            let boundary =
                u32::try_from(mesh.vertices.len()).map_err(|_| FineMeshError::OutputBudget)?;
            for edge in 0..4 {
                let start = corners[edge];
                let end = corners[(edge + 1) % 4];
                let axis = if start[u] == end[u] { v } else { u };
                let lower = start[axis].min(end[axis]);
                let upper = start[axis].max(end[axis]);
                let mut origin = start;
                origin[axis] = lower.div_euclid(256) * 256;
                let cuts = self.cuts(Line { axis, origin })?;
                for step in 0..(upper - lower) {
                    self.charge(1)?;
                    let t = if start[axis] < end[axis] {
                        start[axis] + step
                    } else {
                        start[axis] - step
                    };
                    let offset = usize::try_from(t - origin[axis])
                        .map_err(|_| FineMeshError::CoordinatePrecision)?;
                    if step == 0 || cuts[offset] {
                        let mut point = start;
                        point[axis] = t;
                        self.vertex(
                            mesh,
                            point.map(|c| c as f32 / 256.0),
                            normal,
                            quad.voxel(),
                            finish,
                        )?;
                    }
                }
            }
            self.fan(mesh, first, boundary)?;
        }
        Ok(())
    }

    fn fan(&mut self, mesh: &mut CpuMesh, first: u32, boundary: u32) -> Result<(), FineMeshError> {
        let last = u32::try_from(mesh.vertices.len()).map_err(|_| FineMeshError::OutputBudget)?;
        for index in boundary..last {
            self.charge(1)?;
            if self.report.indices + 3 > self.limits.indices {
                return Err(FineMeshError::OutputBudget);
            }
            mesh.indices
                .try_reserve(3)
                .map_err(|_| FineMeshError::Allocation)?;
            mesh.indices.extend_from_slice(&[
                first,
                index,
                if index + 1 == last {
                    boundary
                } else {
                    index + 1
                },
            ]);
            self.report.indices += 3;
        }
        Ok(())
    }

    fn vertex(
        &mut self,
        mesh: &mut CpuMesh,
        position: [f32; 3],
        normal: [f32; 3],
        voxel: Voxel,
        finish: f32,
    ) -> Result<u32, FineMeshError> {
        if self.report.vertices == self.limits.vertices {
            return Err(FineMeshError::OutputBudget);
        }
        mesh.vertices
            .try_reserve(1)
            .map_err(|_| FineMeshError::Allocation)?;
        let index = u32::try_from(mesh.vertices.len()).map_err(|_| FineMeshError::OutputBudget)?;
        let base = material_surface(voxel.material);
        mesh.vertices.push(Vertex {
            position,
            normal,
            albedo_roughness: [base[0], base[1], base[2], base[3]],
            ambient_occlusion: 1.0,
            metallic: base[4],
            material: u32::from(voxel.material as u8),
            damage: voxel_damage(voxel),
            fracture_depth: finish,
        });
        self.report.vertices += 1;
        Ok(index)
    }

    fn cuts(&mut self, line: Line) -> Result<Cuts, FineMeshError> {
        self.charge(1)?;
        if let Some(cuts) = self.lines.get(&line) {
            return Ok(*cuts);
        }
        if self.report.lines == self.limits.lines {
            return Err(FineMeshError::LineBudget);
        }
        let a = line.axis;
        let u = (a + 1) % 3;
        let v = (a + 2) % 3;
        let base = line.origin.map(|c| c.div_euclid(256));
        let mut cuts = [false; 257];
        cuts[0] = true;
        cuts[256] = true;
        for du in 0..=i32::from(line.origin[u].rem_euclid(256) == 0) {
            for dv in 0..=i32::from(line.origin[v].rem_euclid(256) == 0) {
                self.charge(1)?;
                let mut cell = base;
                cell[u] -= du;
                cell[v] -= dv;
                let local = std::array::from_fn(|i| line.origin[i] - cell[i] * 256);
                let geometry = self
                    .world
                    .geometry_cell(IVec3::new(cell[0], cell[1], cell[2]));
                if let Some(volume) = geometry.volume() {
                    for leaf in volume.leaves() {
                        self.charge(1)?;
                        add_cuts(&mut cuts, leaf.bounds(), local, a);
                    }
                } else {
                    self.charge(1)?;
                    add_cuts(&mut cuts, LocalBox::FULL, local, a);
                }
            }
        }
        self.lines
            .try_reserve(1)
            .map_err(|_| FineMeshError::Allocation)?;
        self.lines.insert(line, cuts);
        self.report.lines += 1;
        Ok(cuts)
    }
}

fn add_cuts(cuts: &mut Cuts, bounds: LocalBox, local: [i32; 3], axis: usize) {
    if (0..3).all(|i| {
        i == axis
            || (i32::from(bounds.minimum()[i])..=i32::from(bounds.maximum()[i])).contains(&local[i])
    }) {
        cuts[usize::from(bounds.minimum()[axis])] = true;
        cuts[usize::from(bounds.maximum()[axis])] = true;
    }
}

fn page(cell: &GeometryCell) -> RefinedVolume {
    cell.volume().map_or_else(
        || RefinedVolume::uniform(cell.uniform_voxel().expect("canonical uniform cell")),
        Clone::clone,
    )
}

struct WorkMeter {
    used: Cell<usize>,
    limit: usize,
    exhausted: Cell<bool>,
}
impl WorkMeter {
    const fn new(limit: usize) -> Self {
        Self {
            used: Cell::new(0),
            limit,
            exhausted: Cell::new(false),
        }
    }
    fn charge(&self, count: usize) -> Result<(), FineMeshError> {
        if self.exhausted.get() || count > self.limit - self.used.get() {
            self.exhausted.set(true);
            return Err(FineMeshError::WorkBudget);
        }
        self.used.set(self.used.get() + count);
        Ok(())
    }
    const fn check(&self) -> Result<(), FineMeshError> {
        if self.exhausted.get() {
            Err(FineMeshError::WorkBudget)
        } else {
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests;
