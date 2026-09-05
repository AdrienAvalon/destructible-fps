//! Server-owned, coarse brittle failure. No default profile claims real-material calibration.

use crate::{
    BodyError, BodyLimits, CommandError, IVec3, Material, RigidBodyDescriptor, World,
    elasticity::{ElasticSolution, MAX_ELASTIC_NODES},
    structural::StructuralAnchors,
    structural_jobs::{StructuralJobError, neighbors},
};
use std::{
    collections::{BTreeSet, VecDeque},
    sync::atomic::{AtomicBool, Ordering},
};

/// Positive section strengths in Pa. Isotropic maximum-stress envelope, not a plasticity model.
#[derive(Clone, Copy, Debug)]
pub struct SectionStrength {
    pub tension_pa: f64,
    pub compression_pa: f64,
    pub shear_pa: f64,
}

/// Soil, Stone, Wood, Brick, Concrete, Steel, Glass; explicit server configuration only.
#[derive(Clone, Copy, Debug)]
pub struct StructuralStrengths([SectionStrength; 7]);

impl StructuralStrengths {
    /// # Errors
    /// Rejects nonfinite or out-of-range strengths before accepting a worker configuration.
    pub fn new(strengths: [SectionStrength; 7]) -> Result<Self, StructuralFailureError> {
        if strengths.iter().any(|entry| {
            [entry.tension_pa, entry.compression_pa, entry.shear_pa]
                .into_iter()
                .any(|value| !value.is_finite() || !(1.0..=1e12).contains(&value))
        }) {
            return Err(StructuralFailureError::InvalidStrength);
        }
        Ok(Self(strengths))
    }

    const fn for_material(
        self,
        material: Material,
    ) -> Result<SectionStrength, StructuralFailureError> {
        let index = match material {
            Material::Air => return Err(StructuralFailureError::InvalidSolution),
            Material::Soil => 0,
            Material::Stone => 1,
            Material::Wood => 2,
            Material::Brick => 3,
            Material::Concrete => 4,
            Material::Steel => 5,
            Material::Glass => 6,
        };
        Ok(self.0[index])
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StructuralFailureError {
    NotConfigured,
    InvalidStrength,
    InvalidSolution,
    InvalidPlan,
    TooManyFragments,
    SequenceExhausted,
    DeadlineExceeded,
    Job(StructuralJobError),
    Body(BodyError),
    Commit(CommandError),
}

impl core::fmt::Display for StructuralFailureError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "structural failure: {self:?}")
    }
}
impl std::error::Error for StructuralFailureError {}
impl From<StructuralJobError> for StructuralFailureError {
    fn from(error: StructuralJobError) -> Self {
        Self::Job(error)
    }
}
impl From<BodyError> for StructuralFailureError {
    fn from(error: BodyError) -> Self {
        Self::Body(error)
    }
}
impl From<CommandError> for StructuralFailureError {
    fn from(error: CommandError) -> Self {
        Self::Commit(error)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FailureMode {
    Tension,
    Compression,
    Shear,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FailureCandidate {
    pub position: IVec3,
    pub mode: FailureMode,
    /// Floor of demand/capacity * 1000, saturated at `u32::MAX`. Failure requires >1000.
    pub utilization_per_mille: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StructuralFailureReport {
    pub candidate: FailureCandidate,
    pub spawned_bodies: usize,
    pub moved_voxels: usize,
    pub mass_kg: u64,
}

pub(crate) struct PreparedStructuralFailure {
    pub(crate) candidate: FailureCandidate,
    pub(crate) bodies: Vec<RigidBodyDescriptor>,
}

#[derive(Clone, Copy, Debug)]
struct SectionDemand {
    tension: f64,
    compression: f64,
    shear: f64,
}

fn demand(
    axis: usize,
    end: usize,
    forces: [f64; 6],
    length: f64,
    integrity: u8,
) -> Result<SectionDemand, StructuralFailureError> {
    if axis >= 3
        || end >= 2
        || integrity == 0
        || !(0.01..=10.0).contains(&length)
        || forces.iter().any(|force| !force.is_finite())
    {
        return Err(StructuralFailureError::InvalidSolution);
    }
    let fraction = f64::from(integrity) / 255.0;
    let width = length * fraction.sqrt();
    let area = length.powi(2) * fraction;
    let inertia = length.powi(4) * fraction.powi(2) / 12.0;
    let [u, v] = [(axis + 1) % 3, (axis + 2) % 3];
    let axial = forces[axis] * if end == 0 { -1.0 } else { 1.0 } / area;
    let bending = (forces[u + 3].abs() + forces[v + 3].abs()) * width / (2.0 * inertia);
    let shear =
        1.5 * forces[u].hypot(forces[v]) / area + forces[axis + 3].abs() / (0.208 * width.powi(3));
    let result = SectionDemand {
        tension: (axial + bending).max(0.0),
        compression: (bending - axial).max(0.0),
        shear,
    };
    if [result.tension, result.compression, result.shear]
        .iter()
        .any(|value| !value.is_finite())
    {
        return Err(StructuralFailureError::InvalidSolution);
    }
    Ok(result)
}

// Inputs are finite nonnegative demands and positive bounded capacities. Saturation is explicit;
// the cast then only quantizes downward, with no float geometry or stress entering the wire.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn quantize_ratio(ratio: f64) -> u32 {
    (ratio * 1000.0).clamp(0.0, f64::from(u32::MAX)) as u32
}

fn candidate(
    position: IVec3,
    stress: SectionDemand,
    strength: SectionStrength,
) -> FailureCandidate {
    let mut result = FailureCandidate {
        position,
        mode: FailureMode::Tension,
        utilization_per_mille: 0,
    };
    for (mode, ratio) in [
        (FailureMode::Tension, stress.tension / strength.tension_pa),
        (
            FailureMode::Compression,
            stress.compression / strength.compression_pa,
        ),
        (FailureMode::Shear, stress.shear / strength.shear_pa),
    ] {
        let utilization = quantize_ratio(ratio);
        if utilization > result.utilization_per_mille {
            result.mode = mode;
            result.utilization_per_mille = utilization;
        }
    }
    result
}

fn beam_axis(left: IVec3, right: IVec3) -> Result<usize, StructuralFailureError> {
    let delta = [
        i64::from(right.x) - i64::from(left.x),
        i64::from(right.y) - i64::from(left.y),
        i64::from(right.z) - i64::from(left.z),
    ];
    (0..3)
        .find(|&axis| delta[axis] == 1 && (0..3).all(|other| other == axis || delta[other] == 0))
        .ok_or(StructuralFailureError::InvalidSolution)
}

fn select_failure(
    world: &World,
    anchors: &StructuralAnchors,
    solution: &ElasticSolution,
    strengths: StructuralStrengths,
    cancelled: &AtomicBool,
) -> Result<Option<FailureCandidate>, StructuralFailureError> {
    if solution.positions.is_empty()
        || solution.positions.len() > MAX_ELASTIC_NODES
        || solution.positions.windows(2).any(|pair| pair[0] >= pair[1])
        || solution.bond_ends.len() > 3 * solution.positions.len()
        || solution.bond_ends.len() != solution.bond_end_forces.len()
    {
        return Err(StructuralFailureError::InvalidSolution);
    }
    let mut selected: Option<FailureCandidate> = None;
    for (ends, forces) in solution.bond_ends.iter().zip(&solution.bond_end_forces) {
        check_cancelled(cancelled)?;
        let [Some(left), Some(right)] = ends.map(|index| solution.positions.get(index).copied())
        else {
            return Err(StructuralFailureError::InvalidSolution);
        };
        let axis = beam_axis(left, right)?;
        let voxels = [world.voxel(left), world.voxel(right)];
        let integrity = voxels[0].integrity.min(voxels[1].integrity);
        let first = demand(axis, 0, forces[0], 1.0, integrity)?;
        let last = demand(axis, 1, forces[1], 1.0, integrity)?;
        // A cantilever's root moment can peak at its clamped end. Evaluate the whole bond's
        // section envelope even though only its free material cell can be severed here.
        let stress = SectionDemand {
            tension: first.tension.max(last.tension),
            compression: first.compression.max(last.compression),
            shear: first.shear.max(last.shear),
        };
        for (end, position) in [left, right].into_iter().enumerate() {
            let strength = strengths.for_material(voxels[end].material)?;
            // These are clamped boundaries, not fully observed soil/foundation bearing problems.
            if anchors.contains(position) {
                continue;
            }
            let value = candidate(position, stress, strength);
            if value.utilization_per_mille > 1000
                && selected.is_none_or(|previous| {
                    value.utilization_per_mille > previous.utilization_per_mille
                        || value.utilization_per_mille == previous.utilization_per_mille
                            && value.position < previous.position
                })
            {
                selected = Some(value);
            }
        }
    }
    Ok(selected)
}

fn check_cancelled(cancelled: &AtomicBool) -> Result<(), StructuralFailureError> {
    if cancelled.load(Ordering::Relaxed) {
        return Err(StructuralJobError::Cancelled.into());
    }
    Ok(())
}

pub(crate) fn prepare_failure(
    world: &World,
    anchors: &StructuralAnchors,
    solution: &ElasticSolution,
    strengths: StructuralStrengths,
    cancelled: &AtomicBool,
) -> Result<Option<PreparedStructuralFailure>, StructuralFailureError> {
    let Some(candidate) = select_failure(world, anchors, solution, strengths, cancelled)? else {
        return Ok(None);
    };
    let mut remaining: BTreeSet<_> = solution.positions.iter().copied().collect();
    remaining.remove(&candidate.position);
    // The severed cell itself becomes physical debris. Its matter is not deleted or welded back
    // into a formerly connected component; every other detached component is a separate body.
    let mut pieces = vec![vec![candidate.position]];
    while let Some(seed) = remaining.pop_first() {
        let mut queue = VecDeque::from([seed]);
        let mut component = Vec::new();
        let mut supported = false;
        while let Some(position) = queue.pop_front() {
            check_cancelled(cancelled)?;
            supported |= anchors.contains(position);
            component.push(position);
            for neighbor in neighbors(position) {
                if remaining.remove(&neighbor) {
                    queue.push_back(neighbor);
                }
            }
        }
        if !supported {
            component.sort_unstable();
            pieces.push(component);
            if pieces.len() > 7 {
                return Err(StructuralFailureError::TooManyFragments);
            }
        }
    }
    pieces.sort_unstable_by_key(|piece| piece[0]);
    let mut bodies = Vec::with_capacity(pieces.len());
    for piece in pieces {
        check_cancelled(cancelled)?;
        bodies.push(RigidBodyDescriptor::from_world_voxels(
            1,
            world,
            piece,
            BodyLimits {
                max_voxels: MAX_ELASTIC_NODES,
            },
        )?);
    }
    Ok(Some(PreparedStructuralFailure { candidate, bodies }))
}

#[cfg(test)]
mod tests;
