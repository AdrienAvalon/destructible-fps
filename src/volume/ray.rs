//! Exact material chords through one metre-sized page, from integer-micrometre segment endpoints.
//! Page boundaries remain rational micrometres: a finest cell is exactly `1_000_000 / 256` um.

use super::{
    LocalBox, RefinedVolume, VOLUME_EDGE, VolumeError, VolumeLeaf, WorkBudget, reserve_bounded,
};
use std::cmp::Ordering;

pub const PAGE_MICROMETERS: i64 = 1_000_000;
pub const MAX_SEGMENT_COORDINATE_UM: i32 = 128_000_000;
pub const MAX_RAY_HITS: usize = 768;
pub const MAX_RAY_VISITS: usize = 32_768;

/// Exact parameter on the submitted segment. Returned hit parameters are in [0,1]; denominator
/// is positive. Equality/order compare rational values, not their unreduced numerator/denominator.
#[derive(Clone, Copy, Debug)]
pub struct SegmentParameter {
    numerator: i64,
    denominator: i64,
}
impl SegmentParameter {
    const ZERO: Self = Self {
        numerator: 0,
        denominator: 1,
    };
    const ONE: Self = Self {
        numerator: 1,
        denominator: 1,
    };
    #[must_use]
    pub const fn numerator(self) -> i64 {
        self.numerator
    }
    #[must_use]
    pub const fn denominator(self) -> i64 {
        self.denominator
    }
}
impl Ord for SegmentParameter {
    fn cmp(&self, other: &Self) -> Ordering {
        (i128::from(self.numerator) * i128::from(other.denominator))
            .cmp(&(i128::from(other.numerator) * i128::from(self.denominator)))
    }
}
impl PartialOrd for SegmentParameter {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl PartialEq for SegmentParameter {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}
impl Eq for SegmentParameter {}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MaterialChord {
    pub leaf: VolumeLeaf,
    pub entry: SegmentParameter,
    pub exit: SegmentParameter,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RayLimits {
    pub hits: usize,
    pub visits: usize,
}
impl Default for RayLimits {
    fn default() -> Self {
        Self {
            hits: MAX_RAY_HITS,
            visits: MAX_RAY_VISITS,
        }
    }
}
#[derive(Debug)]
pub struct SegmentTrace {
    pub chords: Vec<MaterialChord>,
    pub visits: usize,
}

impl RefinedVolume {
    /// Returns front-to-back positive-length material chords, preserving true air gaps. Endpoints
    /// are local integer micrometres and may lie outside the page, bounded to +/-128 metres.
    /// Tangential contact has no chord. For a ray parallel to a face, a minimum plane belongs to
    /// the box and its maximum plane does not. Neither endpoints nor geometric planes are rounded.
    ///
    /// This geometry query is not a weapon policy, damage transaction or migrated World consumer.
    /// # Errors
    /// Invalid/zero segments or hit/work/vector-allocation exhaustion discard the entire trace.
    pub fn trace_segment(
        &self,
        start_um: [i32; 3],
        end_um: [i32; 3],
        limits: RayLimits,
    ) -> Result<SegmentTrace, VolumeError> {
        let (start, direction) = segment(start_um, end_um)?;
        if !(1..=MAX_RAY_HITS).contains(&limits.hits)
            || !(1..=MAX_RAY_VISITS).contains(&limits.visits)
        {
            return Err(VolumeError::InvalidLimits);
        }
        let end = end_um.map(|v| i64::from(v) * i64::from(VOLUME_EDGE));
        let query = LocalBox::new(
            std::array::from_fn(|axis| local_index(start[axis].min(end[axis]))),
            std::array::from_fn(|axis| local_index(start[axis].max(end[axis])) + 1),
        )?;
        let mut work = WorkBudget::new(limits.visits);
        let mut chords = Vec::new();
        self.visit_overlaps(query, &mut work, |leaf, _| {
            if leaf.voxel().is_solid()
                && let Some((entry, exit)) = intersect(leaf.bounds(), start, direction)
            {
                if chords.len() == limits.hits {
                    return Err(VolumeError::RayBudget);
                }
                reserve_bounded(&mut chords, 1, limits.hits)?;
                chords.push(MaterialChord { leaf, entry, exit });
            }
            Ok(true)
        })?;
        // No allocation. At most 768 chords bound comparison work independently of traversal.
        chords.sort_unstable_by_key(|chord| chord.entry);
        Ok(SegmentTrace {
            chords,
            visits: work.visited,
        })
    }
}

/// Allocation-free uniform fast path with exactly the refined page's half-open semantics.
pub(crate) fn trace_uniform_segment(
    voxel: crate::Voxel,
    start_um: [i32; 3],
    end_um: [i32; 3],
) -> Result<Option<MaterialChord>, VolumeError> {
    let (start, direction) = segment(start_um, end_um)?;
    Ok(if voxel.is_solid() {
        intersect(LocalBox::FULL, start, direction).map(|(entry, exit)| MaterialChord {
            leaf: VolumeLeaf {
                bounds: LocalBox::FULL,
                voxel,
            },
            entry,
            exit,
        })
    } else {
        None
    })
}

fn segment(start: [i32; 3], end: [i32; 3]) -> Result<([i64; 3], [i64; 3]), VolumeError> {
    if start == end
        || start
            .into_iter()
            .chain(end)
            .any(|v| !(-MAX_SEGMENT_COORDINATE_UM..=MAX_SEGMENT_COORDINATE_UM).contains(&v))
    {
        return Err(VolumeError::InvalidSegment);
    }
    let start = start.map(|v| i64::from(v) * i64::from(VOLUME_EDGE));
    let direction =
        std::array::from_fn(|axis| i64::from(end[axis]) * i64::from(VOLUME_EDGE) - start[axis]);
    Ok((start, direction))
}

fn local_index(scaled_um: i64) -> u16 {
    u16::try_from(
        scaled_um
            .div_euclid(PAGE_MICROMETERS)
            .clamp(0, i64::from(VOLUME_EDGE - 1)),
    )
    .unwrap_or(0)
}

fn intersect(
    bounds: LocalBox,
    start: [i64; 3],
    direction: [i64; 3],
) -> Option<(SegmentParameter, SegmentParameter)> {
    let mut entry = SegmentParameter::ZERO;
    let mut exit = SegmentParameter::ONE;
    for axis in 0..3 {
        let low = i64::from(bounds.minimum()[axis]) * PAGE_MICROMETERS;
        let high = i64::from(bounds.maximum()[axis]) * PAGE_MICROMETERS;
        let delta = direction[axis];
        if delta == 0 {
            if start[axis] < low || start[axis] >= high {
                return None;
            }
            continue;
        }
        let (near, far) = if delta > 0 {
            (
                SegmentParameter {
                    numerator: low - start[axis],
                    denominator: delta,
                },
                SegmentParameter {
                    numerator: high - start[axis],
                    denominator: delta,
                },
            )
        } else {
            (
                SegmentParameter {
                    numerator: start[axis] - high,
                    denominator: -delta,
                },
                SegmentParameter {
                    numerator: start[axis] - low,
                    denominator: -delta,
                },
            )
        };
        entry = entry.max(near);
        exit = exit.min(far);
        if entry >= exit {
            return None;
        }
    }
    Some((entry, exit))
}

#[cfg(test)]
mod tests;
