//! Read-only static penetration-work probe, using exact fine material chords.
//!
//! Fictional game energy, not real-world calibration, finite projectile collision or damage.
//! Never install this point-ray policy in the coarse rifle's conservative cover path.

use super::{FixedRay, MAX_RIFLE_RANGE_UM, RIFLE_ENERGY, resistance};
use crate::{
    FixedMicrometers3, Voxel,
    volume::ray::SegmentParameter,
    world::query::{
        StaticGeometry,
        ray::{MaterialTrace, TraceError, TraceLimits, trace_materials},
    },
};

pub const MICRO_WORK_PER_UNIT: u64 = 1_000_000;
pub const RIFLE_MICRO_WORK: u64 = RIFLE_ENERGY as u64 * MICRO_WORK_PER_UNIT;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MaterialRun {
    pub voxel: Voxel,
    pub entry: SegmentParameter,
    pub exit: SegmentParameter,
}

#[derive(Debug)]
pub struct PenetrationProbe {
    pub trace: MaterialTrace,
    /// Adjacent intervals with identical material AND integrity coalesce, across page boundaries.
    pub material_runs: usize,
    pub spent_micro_work: u64,
    pub remaining_micro_work: u64,
    /// A stopping INTERVAL, not a calculated impact point or already-carved penetration channel.
    pub stopped_in: Option<MaterialRun>,
}

/// Evaluates only static material resistance. Bodies, flight, radial footprint, fracture,
/// ammunition, cadence and world transactions belong to the enclosing future fine weapon path.
/// # Errors
/// Rejects malformed rays or a refused complete static trace, never returning a partial probe.
pub fn probe_static_rifle(
    world: &impl StaticGeometry,
    origin: FixedMicrometers3,
    direction: [i16; 3],
    limits: TraceLimits,
) -> Result<PenetrationProbe, TraceError> {
    let ray = FixedRay::new(origin, direction, MAX_RIFLE_RANGE_UM).map_err(TraceError::Ray)?;
    let trace = trace_materials(world, &ray, limits)?;
    Ok(evaluate(trace))
}

fn evaluate(trace: MaterialTrace) -> PenetrationProbe {
    let mut probe = PenetrationProbe {
        trace,
        material_runs: 0,
        spent_micro_work: 0,
        remaining_micro_work: RIFLE_MICRO_WORK,
        stopped_in: None,
    };
    let mut cursor = 0;
    while let Some(first) = probe.trace.chords.get(cursor) {
        let mut run = MaterialRun {
            voxel: first.material.leaf.voxel(),
            entry: first.material.entry,
            exit: first.material.exit,
        };
        cursor += 1;
        while let Some(next) = probe.trace.chords.get(cursor) {
            if next.material.leaf.voxel() != run.voxel || next.material.entry != run.exit {
                break;
            }
            run.exit = next.material.exit;
            cursor += 1;
        }
        probe.material_runs += 1;
        let cost = run_work(run, probe.trace.length_ceil_um);
        let spent = cost.min(probe.remaining_micro_work);
        probe.remaining_micro_work -= spent;
        if probe.remaining_micro_work == 0 {
            probe.stopped_in = Some(run);
            break;
        }
    }
    probe.spent_micro_work = RIFLE_MICRO_WORK - probe.remaining_micro_work;
    probe
}

fn run_work(run: MaterialRun, length_um: u64) -> u64 {
    // Positive rational chord difference. FixedRay<=120m gives each denominator<=30,720,000,000;
    // the cross-product times scaled length is <3e31, safely below u128::MAX. Conversion from
    // the private page parameters is safe: accepted parameters are in [0,1], denominator>0.
    let n = |p: SegmentParameter| u128::try_from(p.numerator()).expect("nonnegative ray parameter");
    let d =
        |p: SegmentParameter| u128::try_from(p.denominator()).expect("positive ray denominator");
    let numerator = n(run.exit) * d(run.entry) - n(run.entry) * d(run.exit);
    let denominator = d(run.entry) * d(run.exit);
    // Use the segment's outward integer length (<=1um above Euclidean length). Round chord
    // LENGTH once per coalesced material run to 1/256um, then work once to 1e-6 game unit.
    // No ceil-per-leaf integrity tax; unrelated partitions cannot change the resistance.
    let length_scaled = (numerator * u128::from(length_um) * 256).div_ceil(denominator);
    // Integrity zero is still occupied under the Voxel contract: give it a one-unit floor,
    // never free penetration merely because a stored solid has zero remaining integrity.
    let cost = (u128::from(resistance(run.voxel.material))
        * u128::from(run.voxel.integrity.max(1))
        * length_scaled
        * u128::from(MICRO_WORK_PER_UNIT))
    .div_ceil(255 * 1_000_000 * 256);
    u64::try_from(cost).expect("bounded 120m material work")
}

#[cfg(test)]
mod tests;
