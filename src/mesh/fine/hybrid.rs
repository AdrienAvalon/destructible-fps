//! Shared uniform smoothing with an exact one-cell collar around fine geometry.
use super::{Builder, FineMeshBatch, FineMeshError, FineMeshLimits, WorkMeter, mesh_chunks};
use crate::{
    CHUNK_EDGE, IVec3, Voxel, chunk_position,
    mesh::{
        CpuMesh, DerivedCellCache, FRACTURE_RENDER_DEPENDENCY_RADIUS_VOXELS, UniformSurfaceSource,
        append_derived_surface, append_voxel, uses_derived_surface_at,
    },
    world::{
        geometry::{GeometryCell, MAX_GEOMETRY_CHANGES, RefinedWorld},
        query::StaticGeometry,
    },
};
use std::collections::{BTreeSet, HashSet};

/// Renders exact fine material boundaries alongside the existing uniform Surface Nets terrain.
/// # Errors
/// Same aggregate caps and atomic refusal as `mesh_fine_chunks`, including all derived lookups.
pub fn mesh_hybrid_chunks(
    world: &RefinedWorld,
    chunks: &[IVec3],
    limits: FineMeshLimits,
) -> Result<FineMeshBatch, FineMeshError> {
    mesh_chunks(world, chunks, limits, Some(world))
}

/// Conservative complete invalidation for a fine edit, including collar and old masonry halo.
/// # Errors
/// Rejects empty/oversized position sets and unsupported render coordinates before offsetting.
pub fn hybrid_dirty_chunks(positions: &[IVec3]) -> Result<Vec<IVec3>, FineMeshError> {
    if positions.is_empty() || positions.len() > MAX_GEOMETRY_CHANGES {
        return Err(FineMeshError::ChunkSet);
    }
    let mut chunks = BTreeSet::new();
    let radius = FRACTURE_RENDER_DEPENDENCY_RADIUS_VOXELS;
    for p in positions {
        if [p.x, p.y, p.z].into_iter().any(|v| {
            !(-super::FINE_RENDER_EXTENT_METRES + radius..super::FINE_RENDER_EXTENT_METRES - radius)
                .contains(&v)
        }) {
            return Err(FineMeshError::CoordinatePrecision);
        }
        for x in [-radius, radius] {
            for y in [-radius, radius] {
                for z in [-radius, radius] {
                    chunks.insert(chunk_position(IVec3::new(p.x + x, p.y + y, p.z + z)));
                }
            }
        }
    }
    Ok(chunks.into_iter().collect())
}

pub(super) struct HybridSource<'a, G> {
    world: &'a G,
    collar: HashSet<IVec3>,
    work: &'a WorkMeter,
}
impl<'a, G: StaticGeometry> HybridSource<'a, G> {
    pub(super) fn new(
        world: &'a G,
        refined: &RefinedWorld,
        chunks: &[IVec3],
        work: &'a WorkMeter,
    ) -> Result<Self, FineMeshError> {
        let mut collar = HashSet::new();
        work.charge(refined.geometry_stats().chunks)?;
        for p in refined.refined_positions() {
            work.charge(chunks.len() + 1)?;
            // Only collar classifications queried by this chunk batch can affect its output.
            // The existing radius7 dependency exceeds the radius2 actually needed by dual cells.
            // nearby_masonry_damage reads actual uniform source cells, NOT allows_derived;
            // its radius6 scan therefore does not expand collar-classification dependencies.
            if !chunks.iter().any(|c| {
                (0..3).all(|a| {
                    let start = [c.x, c.y, c.z][a] * CHUNK_EDGE;
                    let radius = FRACTURE_RENDER_DEPENDENCY_RADIUS_VOXELS;
                    (start - radius..start + CHUNK_EDGE + radius).contains(&[p.x, p.y, p.z][a])
                })
            }) {
                continue;
            }
            collar
                .try_reserve(27)
                .map_err(|_| FineMeshError::Allocation)?;
            for x in -1..=1 {
                for y in -1..=1 {
                    for z in -1..=1 {
                        work.charge(1)?;
                        collar.insert(IVec3::new(p.x + x, p.y + y, p.z + z));
                    }
                }
            }
        }
        Ok(Self {
            world,
            collar,
            work,
        })
    }
}
impl<G: StaticGeometry> UniformSurfaceSource for HybridSource<'_, G> {
    fn uniform_voxel(&self, position: IVec3) -> Option<Voxel> {
        self.work.charge(1).ok()?;
        self.world.geometry_cell(position).uniform_voxel()
    }
    fn allows_derived(&self, position: IVec3) -> bool {
        self.work.charge(1).is_ok() && !self.collar.contains(&position)
    }
}

impl<G: StaticGeometry> Builder<'_, G> {
    pub(super) fn append_hybrid(
        &mut self,
        mesh: &mut CpuMesh,
        p: IVec3,
        cell: &GeometryCell,
        source: &HybridSource<'_, G>,
        derived: &mut DerivedCellCache,
    ) -> Result<(), FineMeshError> {
        self.charge(1)?;
        if source.collar.contains(&p) {
            // The exact-side render cap is explicit, never a physical occupancy proxy. A fine
            // neighbor is never removed by this rule, and fine owners have no derived neighbors.
            let transition = std::array::from_fn(|i| {
                let mut q = [p.x, p.y, p.z];
                q[i / 2] += if i % 2 == 0 { -1 } else { 1 };
                let q = IVec3::new(q[0], q[1], q[2]);
                source
                    .uniform_voxel(q)
                    .is_some_and(|voxel| uses_derived_surface_at(source, q, voxel))
            });
            self.work.check()?;
            return self.append_cell(mesh, p, cell, transition);
        }
        let voxel = cell
            .uniform_voxel()
            .ok_or(FineMeshError::DerivedInvariant)?;
        // Every legacy append can emit at most six quads, and reserves before its infallible pushes.
        if self.report.vertices + 24 > self.limits.vertices
            || self.report.indices + 36 > self.limits.indices
            || self.report.quads + 6 > self.limits.quads
        {
            return Err(FineMeshError::OutputBudget);
        }
        mesh.vertices
            .try_reserve(24)
            .map_err(|_| FineMeshError::Allocation)?;
        mesh.indices
            .try_reserve(36)
            .map_err(|_| FineMeshError::Allocation)?;
        let before = (mesh.vertices.len(), mesh.indices.len());
        let success = if uses_derived_surface_at(source, p, voxel) {
            append_derived_surface(mesh, source, derived, p, voxel)
        } else {
            append_voxel(mesh, p, p, voxel, &|sample| {
                source.uniform_voxel(sample).is_some_and(|neighbor| {
                    neighbor.is_solid() && !uses_derived_surface_at(source, sample, neighbor)
                })
            })
        };
        self.work.check()?;
        if !success {
            return Err(FineMeshError::DerivedInvariant);
        }
        self.report.vertices += mesh.vertices.len() - before.0;
        self.report.indices += mesh.indices.len() - before.1;
        self.report.quads += (mesh.indices.len() - before.1) / 6;
        Ok(())
    }
}

#[cfg(test)]
mod tests;
