//! Explicit immutable composition: no query may see the voxel source but omit the slabs.
use super::{ConvexError, ConvexFragment, ConvexQueryBudget, ConvexQueryLimits};
use crate::{
    IVec3, Voxel,
    ballistics::FixedRay,
    volume::ray::SegmentParameter,
    world::{
        geometry::RefinedWorld,
        query::{self, PhysicalBox, QueryBudget, QueryLimits, SweepResult, ray::TraceLimits},
    },
};
use std::sync::Arc;

pub const MAX_SCENE_FRAGMENTS: usize = 32;

/// An authored shape's stable index is scoped to this immutable inspection scene.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum SceneOwner {
    World(IVec3),
    Fragment(usize),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SceneChord {
    pub owner: SceneOwner,
    pub entry: SegmentParameter,
    pub exit: SegmentParameter,
    pub material: Voxel,
}

#[derive(Debug)]
pub struct SceneTrace {
    pub source_fingerprint: u128,
    pub length_um: u64,
    pub chords: Vec<SceneChord>,
}

/// Read-only authored geometry, deliberately not accepted by authoritative simulation/codecs.
///
/// ```compile_fail
/// use destructible_fps::{convex::InspectionGeometry, world::query::StaticGeometry};
/// fn require_static<T: StaticGeometry>() {}
/// require_static::<InspectionGeometry>();
/// ```
pub struct InspectionGeometry {
    world: Arc<RefinedWorld>,
    fragments: Vec<ConvexFragment>,
    fingerprint: u128,
}

impl InspectionGeometry {
    /// Validate every overlap before publishing the composite. Boundary contact is allowed.
    /// # Errors
    /// Refuses oversized collections, overlapping interiors or exhausted query work.
    pub fn new(
        world: Arc<RefinedWorld>,
        mut fragments: Vec<ConvexFragment>,
    ) -> Result<Self, ConvexError> {
        if fragments.len() > MAX_SCENE_FRAGMENTS {
            return Err(ConvexError::Budget);
        }
        // Canonical ordering uses actual content, never relies on collision-free fingerprints.
        fragments.sort_unstable_by(|a, b| {
            (
                a.origin(),
                a.material().material as u8,
                a.material().integrity,
                a.vertices(),
                a.faces(),
            )
                .cmp(&(
                    b.origin(),
                    b.material().material as u8,
                    b.material().integrity,
                    b.vertices(),
                    b.faces(),
                ))
        });
        let mut budget = ConvexQueryBudget::new(ConvexQueryLimits {
            fragments: super::MAX_CONVEX_QUERY_FRAGMENTS,
            axes: super::MAX_CONVEX_QUERY_AXES,
            projections: super::MAX_CONVEX_QUERY_PROJECTIONS,
        })?;
        let mut world_budget = QueryBudget::new(QueryLimits {
            cells: query::MAX_QUERY_BUDGET_CELLS,
            leaf_visits: query::MAX_QUERY_LEAF_VISITS,
        })
        .map_err(ConvexError::WorldQuery)?;
        for (index, fragment) in fragments.iter().enumerate() {
            for other in &fragments[..index] {
                if fragment.overlaps_fragment(other, &mut budget)? {
                    return Err(ConvexError::Overlap);
                }
            }
            let mut failure = None;
            query::visit_solids(
                world.as_ref(),
                fragment.bounds(),
                &mut world_budget,
                |solid, _| match fragment.overlaps(solid, &mut budget) {
                    Ok(false) => Ok(true),
                    Ok(true) => {
                        failure = Some(ConvexError::Overlap);
                        Ok(false)
                    }
                    Err(error) => {
                        failure = Some(error);
                        Ok(false)
                    }
                },
            )
            .map_err(ConvexError::WorldQuery)?;
            if let Some(error) = failure {
                return Err(error);
            }
        }
        // Domain-separated deterministic identity, not authentication or a cryptographic proof.
        let mut bytes = Vec::with_capacity(41 + fragments.len() * 16);
        bytes.extend_from_slice(b"inspection-convex-scene-v1");
        bytes.extend_from_slice(&world.fingerprint().to_le_bytes());
        for fragment in &fragments {
            bytes.extend_from_slice(&fragment.fingerprint().to_le_bytes());
        }
        let fingerprint = bytes
            .into_iter()
            .fold(0x7363_656e_652d_7631_u128, |key, byte| {
                (key ^ u128::from(byte)).wrapping_mul(0x0100_0000_0000_0000_0000_013b)
            });
        Ok(Self {
            world,
            fragments,
            fingerprint,
        })
    }

    #[must_use]
    pub const fn world(&self) -> &Arc<RefinedWorld> {
        &self.world
    }
    #[must_use]
    pub fn fragments(&self) -> &[ConvexFragment] {
        &self.fragments
    }
    #[must_use]
    pub const fn fingerprint(&self) -> u128 {
        self.fingerprint
    }

    /// Trace both sources, refusing partial or double-counted material intervals.
    /// Convex rays use strict interiors (coplanar boundary travel has no thickness);
    /// historical voxel rays retain their documented minimum-closed boundary ownership.
    /// # Errors
    /// Propagates either query's refusal, aggregate chord exhaustion or intersecting intervals.
    pub fn trace(
        &self,
        ray: &FixedRay,
        limits: TraceLimits,
        budget: &mut ConvexQueryBudget,
    ) -> Result<SceneTrace, ConvexError> {
        let base = query::ray::trace_materials(self.world.as_ref(), ray, limits)
            .map_err(ConvexError::WorldTrace)?;
        let mut chords = Vec::new();
        chords
            .try_reserve_exact(limits.chords.min(base.chords.len() + self.fragments.len()))
            .map_err(|_| ConvexError::Budget)?;
        chords.extend(base.chords.into_iter().map(|hit| SceneChord {
            owner: SceneOwner::World(hit.cell),
            entry: hit.material.entry,
            exit: hit.material.exit,
            material: hit.material.leaf.voxel(),
        }));
        for (index, fragment) in self.fragments.iter().enumerate() {
            if let Some(hit) = fragment.trace(ray, budget)? {
                if chords.len() == limits.chords {
                    return Err(ConvexError::Budget);
                }
                chords.push(SceneChord {
                    owner: SceneOwner::Fragment(index),
                    entry: hit.entry,
                    exit: hit.exit,
                    material: hit.material,
                });
            }
        }
        chords.sort_unstable_by_key(|hit| (hit.entry, hit.exit, hit.owner));
        if chords.windows(2).any(|hits| hits[0].exit > hits[1].entry) {
            return Err(ConvexError::Overlap);
        }
        Ok(SceneTrace {
            source_fingerprint: self.fingerprint,
            length_um: base.length_um,
            chords,
        })
    }

    /// # Errors
    /// Any error invalidates the whole query; a first-source hit never hides an error in the other.
    pub fn overlaps(
        &self,
        bounds: PhysicalBox,
        world_budget: &mut QueryBudget,
        budget: &mut ConvexQueryBudget,
    ) -> Result<bool, ConvexError> {
        let mut result = query::overlaps_solid(self.world.as_ref(), bounds, world_budget)
            .map_err(ConvexError::WorldQuery)?;
        for fragment in &self.fragments {
            result |= fragment.overlaps(bounds, budget)?;
        }
        Ok(result)
    }

    /// Both sources see the original requested translation, then the closest safe result wins.
    /// Returned micrometers are conservative, not a global rational TOI or contact manifold.
    /// # Errors
    /// Refuses initial overlap, invalid movement or either source's exhausted budget.
    pub fn sweep_axis(
        &self,
        start: PhysicalBox,
        axis: usize,
        displacement_um: i64,
        world_budget: &mut QueryBudget,
        budget: &mut ConvexQueryBudget,
    ) -> Result<SweepResult, ConvexError> {
        let mut result = query::sweep_axis(
            self.world.as_ref(),
            start,
            axis,
            displacement_um,
            world_budget,
        )
        .map_err(ConvexError::WorldQuery)?;
        for fragment in &self.fragments {
            let hit = fragment.sweep_axis(start, axis, displacement_um, budget)?;
            if hit.displacement_um.unsigned_abs() < result.displacement_um.unsigned_abs() {
                result = hit;
            } else if hit.displacement_um == result.displacement_um {
                result.contact |= hit.contact;
            }
        }
        Ok(result)
    }
}
