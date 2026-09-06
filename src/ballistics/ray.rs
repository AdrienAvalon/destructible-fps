//! Fixed-point segment traversal. Face-order comparisons are exact rational cross-products.

use super::{BallisticError, MAX_RIFLE_RANGE_UM};
use crate::{FixedMicrometers3, IVec3, MAX_BUILD_COORDINATE, MICROMETERS_PER_VOXEL};
use core::cmp::Ordering;

pub const MAX_RAY_CELLS: usize = 512;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CellInterval {
    pub position: IVec3,
    pub enter_um: u64,
    pub exit_um: u64,
    pub parallel_owner: Option<IVec3>,
}

#[derive(Clone, Copy, Debug)]
struct Fraction {
    numerator: u64,
    denominator: u64,
}

impl Fraction {
    const END: Self = Self {
        numerator: 1,
        denominator: 1,
    };

    fn compare(self, other: Self) -> Ordering {
        (u128::from(self.numerator) * u128::from(other.denominator))
            .cmp(&(u128::from(other.numerator) * u128::from(self.denominator)))
    }

    fn distance(self, length: u64) -> u64 {
        u64::try_from(
            (u128::from(length) * u128::from(self.numerator))
                .div_ceil(u128::from(self.denominator)),
        )
        .expect("bounded segment distance")
    }
}

/// A finite ray has an inward-rounded micrometre endpoint. Direction magnitude never sets range.
#[derive(Clone, Debug)]
pub struct FixedRay {
    pub(crate) origin: [i64; 3],
    pub(crate) delta: [i64; 3],
    length: u64,
}

impl FixedRay {
    /// Builds a bounded ray without floating-point authority state.
    /// # Errors
    /// Rejects invalid origins, zero aim and out-of-contract range.
    /// # Panics
    /// Only if the internal displacement/norm bounds are violated despite input validation.
    pub fn new(
        origin: FixedMicrometers3,
        direction: [i16; 3],
        range_um: u64,
    ) -> Result<Self, BallisticError> {
        let origin = [origin.x, origin.y, origin.z];
        let bound = i64::from(MAX_BUILD_COORDINATE) * MICROMETERS_PER_VOXEL;
        if origin
            .iter()
            .any(|value| value.unsigned_abs() > bound.cast_unsigned())
        {
            return Err(BallisticError::InvalidOrigin);
        }
        if range_um == 0 || range_um > MAX_RIFLE_RANGE_UM {
            return Err(BallisticError::InvalidRange);
        }
        let norm_squared: u64 = direction
            .iter()
            .map(|&value| i64::from(value).pow(2).cast_unsigned())
            .sum();
        if norm_squared == 0 {
            return Err(BallisticError::ZeroDirection);
        }
        // Upward-rounded Q16 norm prevents a truncated square root from extending the range.
        let squared_q32 = u128::from(norm_squared) << 32;
        let floor = squared_q32.isqrt();
        let norm_q16 = floor + u128::from(floor * floor != squared_q32);
        let delta = direction.map(|value| {
            i64::try_from(
                (i128::from(value) * i128::from(range_um) * 65_536)
                    / i128::try_from(norm_q16).expect("bounded norm"),
            )
            .expect("bounded ray displacement")
        });
        let length = u64::try_from(
            delta
                .iter()
                .map(|&value| i128::from(value).pow(2).cast_unsigned())
                .sum::<u128>()
                .isqrt(),
        )
        .expect("bounded ray length");
        if length == 0 {
            return Err(BallisticError::InvalidRange);
        }
        Ok(Self {
            origin,
            delta,
            length,
        })
    }

    #[must_use]
    pub const fn length_um(&self) -> u64 {
        self.length
    }

    /// Positive-volume intervals plus zero-length conservative edge/corner cover contacts.
    /// # Errors
    /// Returns an explicit budget error rather than a partial ray when the visit cap is exhausted.
    pub fn cells(&self) -> Result<Vec<CellInterval>, BallisticError> {
        let mut cell = self
            .origin
            .map(|value| value.div_euclid(MICROMETERS_PER_VOXEL));
        for (axis, coordinate) in cell.iter_mut().enumerate() {
            if self.delta[axis] < 0 && self.origin[axis].rem_euclid(MICROMETERS_PER_VOXEL) == 0 {
                *coordinate -= 1;
            }
        }
        let mut faces: [Option<Fraction>; 3] = std::array::from_fn(|axis| {
            let delta = self.delta[axis];
            if delta == 0 {
                return None;
            }
            let boundary = (cell[axis] + i64::from(delta > 0)) * MICROMETERS_PER_VOXEL;
            Some(Fraction {
                numerator: boundary.abs_diff(self.origin[axis]),
                denominator: delta.unsigned_abs(),
            })
        });
        let mut intervals = Vec::with_capacity(128);
        let mut enter_um = 0;
        for _ in 0..MAX_RAY_CELLS {
            let exit = faces
                .iter()
                .flatten()
                .copied()
                .fold(Fraction::END, |earliest, face| {
                    if face.compare(earliest).is_lt() {
                        face
                    } else {
                        earliest
                    }
                });
            let exit_um = exit.distance(self.length);
            let parallel = (0..3).fold(0, |mask, axis| {
                mask | (usize::from(
                    self.delta[axis] == 0
                        && self.origin[axis].rem_euclid(MICROMETERS_PER_VOXEL) == 0,
                ) << axis)
            });
            for subset in 1..8 {
                if subset & parallel == subset {
                    let touching = std::array::from_fn(|axis| {
                        cell[axis] - i64::from(subset & (1 << axis) != 0)
                    });
                    push_interval(&mut intervals, touching, enter_um, enter_um, Some(cell))?;
                }
            }
            // Distinct rational faces can round to the same micrometre. Preserve that grazing
            // cell as a conservative contact instead of silently tunnelling through its corner.
            push_interval(&mut intervals, cell, enter_um, exit_um, None)?;
            if exit.compare(Fraction::END).is_eq() {
                return Ok(intervals);
            }
            let tied = faces.iter().enumerate().fold(0, |mask, (axis, face)| {
                mask | (usize::from(face.is_some_and(|face| face.compare(exit).is_eq())) << axis)
            });
            for subset in 1..8 {
                if subset != tied && subset & tied == subset {
                    let touching = std::array::from_fn(|axis| {
                        cell[axis]
                            + if subset & (1 << axis) == 0 {
                                0
                            } else {
                                self.delta[axis].signum()
                            }
                    });
                    push_tied_contacts(&mut intervals, touching, parallel, exit_um)?;
                }
            }
            for (axis, face) in faces.iter_mut().enumerate() {
                if let Some(face) = face
                    && face.compare(exit).is_eq()
                {
                    cell[axis] += self.delta[axis].signum();
                    face.numerator += MICROMETERS_PER_VOXEL.cast_unsigned();
                }
            }
            enter_um = exit_um;
        }
        Err(BallisticError::RayBudget)
    }

    /// Conservative closed-box intersection for significant dynamic cover. Fixed Q32 fractions
    /// round entrance outward toward the shooter; no floating-point body proxy enters authority.
    #[must_use]
    pub fn box_entry_um(&self, minimum: [i64; 3], maximum: [i64; 3]) -> Option<u64> {
        const SCALE: i128 = 1_i128 << 32;
        let mut enter = 0;
        let mut exit = SCALE;
        for axis in 0..3 {
            if minimum[axis] > maximum[axis] {
                return None;
            }
            let origin = i128::from(self.origin[axis]);
            let delta = i128::from(self.delta[axis]);
            if delta == 0 {
                if origin < i128::from(minimum[axis]) || origin > i128::from(maximum[axis]) {
                    return None;
                }
                continue;
            }
            let (near, far) = if delta > 0 {
                (minimum[axis], maximum[axis])
            } else {
                (maximum[axis], minimum[axis])
            };
            let denominator = delta.abs();
            let numerator = |bound: i64| (i128::from(bound) - origin) * delta.signum() * SCALE;
            enter = enter.max(numerator(near).div_euclid(denominator));
            exit = exit.min(-(-numerator(far)).div_euclid(denominator));
            if enter > exit {
                return None;
            }
        }
        if exit < 0 || enter > SCALE {
            return None;
        }
        u64::try_from(enter.max(0) * i128::from(self.length) / SCALE).ok()
    }
}

fn push_tied_contacts(
    intervals: &mut Vec<CellInterval>,
    touching: [i64; 3],
    parallel: usize,
    distance: u64,
) -> Result<(), BallisticError> {
    // A diagonal crossing on a parallel grid plane also touches the corner cells across that
    // plane. Visiting only the two positive-length lanes would leave an exact corner gap.
    for subset in 0..8 {
        if subset & parallel == subset {
            let cell =
                std::array::from_fn(|axis| touching[axis] - i64::from(subset & (1 << axis) != 0));
            push_interval(intervals, cell, distance, distance, None)?;
        }
    }
    Ok(())
}

fn push_interval(
    intervals: &mut Vec<CellInterval>,
    cell: [i64; 3],
    enter_um: u64,
    exit_um: u64,
    parallel_owner: Option<[i64; 3]>,
) -> Result<(), BallisticError> {
    if intervals.len() == MAX_RAY_CELLS {
        return Err(BallisticError::RayBudget);
    }
    intervals.push(CellInterval {
        position: IVec3::new(
            i32::try_from(cell[0]).expect("bounded cell"),
            i32::try_from(cell[1]).expect("bounded cell"),
            i32::try_from(cell[2]).expect("bounded cell"),
        ),
        enter_um,
        exit_um,
        parallel_owner: parallel_owner.map(|owner| {
            IVec3::new(
                i32::try_from(owner[0]).expect("bounded cell"),
                i32::try_from(owner[1]).expect("bounded cell"),
                i32::try_from(owner[2]).expect("bounded cell"),
            )
        }),
    });
    Ok(())
}
