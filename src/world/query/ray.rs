//! Exact point-segment material intervals through the shared static world.
//! This is NOT finite-radius bullet cover: tangencies have no material thickness.

use super::StaticGeometry;
use crate::{
    IVec3, MICROMETERS_PER_VOXEL,
    ballistics::{BallisticError, FixedRay, ray::MAX_RAY_CELLS},
    volume::{
        VolumeError,
        ray::{MAX_RAY_HITS, MAX_RAY_VISITS, MaterialChord, RayLimits, trace_uniform_segment},
    },
};
use core::fmt;

pub const MAX_TRACE_CHORDS: usize = 4_096;
pub const MAX_TRACE_LEAF_VISITS: usize = 262_144;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TraceLimits {
    pub cells: usize,
    pub leaf_visits: usize,
    pub chords: usize,
}
impl Default for TraceLimits {
    fn default() -> Self {
        Self {
            cells: MAX_RAY_CELLS,
            leaf_visits: MAX_TRACE_LEAF_VISITS,
            chords: MAX_TRACE_CHORDS,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct TraceStats {
    pub cells: usize,
    pub leaf_visits: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TraceError {
    InvalidLimits,
    Cells,
    Leaves,
    Chords,
    Allocation,
    Ray(BallisticError),
    Page(VolumeError),
    Overlap,
}
impl fmt::Display for TraceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "static material trace refused: {self:?}")
    }
}
impl std::error::Error for TraceError {}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WorldChord {
    pub cell: IVec3,
    /// Parameters are on the ORIGINAL world segment, not a rounded page-clipped subsegment.
    pub material: MaterialChord,
}

#[derive(Debug)]
pub struct MaterialTrace {
    pub source_fingerprint: u128,
    /// Inward integer length, retained for legacy/cosmetic distance reporting.
    pub length_um: u64,
    /// Outward integer length; differs by at most 1um. Work uses this bound, not the floor.
    pub length_ceil_um: u64,
    pub chords: Vec<WorldChord>,
    pub stats: TraceStats,
}

/// Complete bounded ray through canonical material, including actual partial-cell air gaps.
///
/// All candidate cells (even conservative zero-distance contacts) get exact narrow-phase tests.
/// The per-page coordinates remain <=121m for the current <=120m `FixedRay` contract.
/// Both phases use exactly `ray.origin` and `ray.origin + ray.delta`: the canonical inward-rounded
/// integer endpoint, not an independently recomputed ideal endpoint at the nominal range.
///
/// No network/authority mutation, early-hit fallback or partial result on failure. The full trace
/// can refuse on distant complexity even if an eventual weapon would stop earlier. Sorting is
/// bounded separately by 512 cells and 4096 output chords; a page scratch trace has <=768 chords.
/// # Errors
/// Refuses invalid limits, cell/work/output/allocation exhaustion or inconsistent overlapping hits.
/// # Panics
/// Only if a sealed canonical cell violates its private uniform-or-refined representation invariant.
pub fn trace_materials(
    world: &impl StaticGeometry,
    ray: &FixedRay,
    limits: TraceLimits,
) -> Result<MaterialTrace, TraceError> {
    if !(1..=MAX_RAY_CELLS).contains(&limits.cells)
        || !(1..=MAX_TRACE_LEAF_VISITS).contains(&limits.leaf_visits)
        || !(1..=MAX_TRACE_CHORDS).contains(&limits.chords)
    {
        return Err(TraceError::InvalidLimits);
    }
    let mut candidates = ray.cells().map_err(TraceError::Ray)?;
    candidates.sort_unstable_by_key(|c| c.position);
    candidates.dedup_by_key(|c| c.position);
    if candidates.len() > limits.cells {
        return Err(TraceError::Cells);
    }
    let mut trace = MaterialTrace {
        source_fingerprint: world.geometry_fingerprint(),
        length_um: ray.length_um(),
        length_ceil_um: ray.length_ceil_um(),
        chords: Vec::new(),
        stats: TraceStats::default(),
    };
    for candidate in candidates {
        trace.stats.cells += 1;
        let position = candidate.position;
        let page_origin =
            [position.x, position.y, position.z].map(|v| i64::from(v) * MICROMETERS_PER_VOXEL);
        let mut start = [0; 3];
        let mut end = [0; 3];
        for axis in 0..3 {
            start[axis] = i32::try_from(ray.origin[axis] - page_origin[axis])
                .map_err(|_| TraceError::Page(VolumeError::InvalidSegment))?;
            end[axis] = i32::try_from(ray.origin[axis] + ray.delta[axis] - page_origin[axis])
                .map_err(|_| TraceError::Page(VolumeError::InvalidSegment))?;
        }
        let remaining = limits.leaf_visits - trace.stats.leaf_visits;
        if remaining == 0 {
            return Err(TraceError::Leaves);
        }
        let cell = world.geometry_cell(position);
        if let Some(voxel) = cell.uniform_voxel() {
            trace.stats.leaf_visits += 1;
            if let Some(material) =
                trace_uniform_segment(voxel, start, end).map_err(TraceError::Page)?
            {
                push(
                    &mut trace.chords,
                    WorldChord {
                        cell: position,
                        material,
                    },
                    limits.chords,
                )?;
            }
        } else {
            let page = cell
                .volume()
                .expect("sealed canonical nonuniform cell has a page");
            let local = page
                .trace_segment(
                    start,
                    end,
                    RayLimits {
                        hits: MAX_RAY_HITS,
                        visits: remaining.min(MAX_RAY_VISITS),
                    },
                )
                .map_err(|e| {
                    if e == VolumeError::VisitBudget {
                        TraceError::Leaves
                    } else {
                        TraceError::Page(e)
                    }
                })?;
            trace.stats.leaf_visits += local.visits;
            for material in local.chords {
                push(
                    &mut trace.chords,
                    WorldChord {
                        cell: position,
                        material,
                    },
                    limits.chords,
                )?;
            }
        }
    }
    trace.chords.sort_unstable_by_key(|c| c.material.entry);
    if trace
        .chords
        .windows(2)
        .any(|p| p[0].material.exit > p[1].material.entry)
    {
        return Err(TraceError::Overlap);
    }
    Ok(trace)
}

fn push(output: &mut Vec<WorldChord>, chord: WorldChord, maximum: usize) -> Result<(), TraceError> {
    if output.len() == maximum {
        return Err(TraceError::Chords);
    }
    if output.len() == output.capacity() {
        let target = output.capacity().saturating_mul(2).max(8).min(maximum);
        output
            .try_reserve_exact(target - output.len())
            .map_err(|_| TraceError::Allocation)?;
    }
    output.push(chord);
    Ok(())
}

#[cfg(test)]
mod tests;
