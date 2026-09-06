//! Exact rectangular-volume mass moments shared by World geometry and real body promotion.
//!
//! Air has no mass. Integrity changes do not remove occupied material. Integer moments retain
//! sub-kilogram fragments; an inertia tensor states its actual pivot instead of silently calling
//! a quantized point the exact centre of mass. Connectivity is a separate, mandatory body gate.

use crate::{
    IVec3,
    physics::BodyVoxel,
    volume::LocalBox,
    world::{geometry::GeometryCell, query::StaticGeometry},
};
use core::fmt;

pub const MAX_MASS_CELLS: usize = 16_384;
pub const MAX_MASS_LEAVES: usize = 131_072;
pub const MAX_MASS_EXTENT_CELLS: i64 = 16_384;
const LATTICE: i128 = 256;
const MASS_DENOMINATOR: i128 = LATTICE * LATTICE * LATTICE;
/// One unit is 1/256 millimetre. This is a pivot precision, not a geometric resolution.
pub const PIVOT_UNITS_PER_METER: i64 = 256_000;
const SECOND_PAIRS: [(usize, usize); 6] = [(0, 0), (1, 1), (2, 2), (0, 1), (0, 2), (1, 2)];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MassPropertyError {
    Empty,
    CellBudget,
    LeafBudget,
    NonCanonicalPositions,
    EmptyCell(IVec3),
    ZeroDensity,
    Extent,
    ArithmeticOverflow,
    PivotBounds,
}
impl fmt::Display for MassPropertyError {
    fn fmt(&self, output: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(output, "mass property calculation refused: {self:?}")
    }
}
impl std::error::Error for MassPropertyError {}

/// Exact signed rational. Units belong to the accessor that produced it. No floating conversion
/// is used by authority or codecs. Denominator is positive; representation need not be reduced.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ExactRatio {
    numerator: i128,
    denominator: i128,
}
impl ExactRatio {
    #[must_use]
    pub const fn numerator(self) -> i128 {
        self.numerator
    }
    #[must_use]
    pub const fn denominator(self) -> i128 {
        self.denominator
    }
    /// Nearest integer, with half ties away from zero, computed directly from the exact fraction.
    #[must_use]
    pub const fn rounded_integer(self) -> i128 {
        if self.numerator >= 0 {
            (self.numerator + self.denominator / 2) / self.denominator
        } else {
            -((-self.numerator + self.denominator / 2) / self.denominator)
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct MassPropertyReport {
    pub cells: usize,
    /// Every canonical leaf, including air, is counted toward the work limit.
    pub leaves: usize,
    pub solid_leaves: usize,
}

/// Symmetric tensor in kg*m^2 about the stated world-space pivot, axes aligned with the world.
/// Off-diagonal entries already contain the NEGATIVE product of inertia.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InertiaTensor {
    pivot_scaled_mm: [i64; 3],
    entries: [ExactRatio; 6],
}
impl InertiaTensor {
    #[must_use]
    pub const fn pivot_scaled_mm(self) -> [i64; 3] {
        self.pivot_scaled_mm
    }
    /// XX, YY, ZZ, XY, XZ, YZ; the opposite off-diagonal entries are equal by symmetry.
    #[must_use]
    pub const fn entries_kg_m2(self) -> [ExactRatio; 6] {
        self.entries
    }
}

/// Immutable raw integrals relative to the selected cells' minimum corner. Raw integrals, rather
/// than a rounded mass/centroid, preserve exact additivity across material/leaf partitions.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MassProperties {
    minimum: IVec3,
    maximum: IVec3,
    weight: i128,
    first: [i128; 3],
    second: [i128; 6],
}
impl MassProperties {
    /// Computes exact properties of explicitly selected occupied cells; does NOT prove connection,
    /// support or permission to create a body. The source world is never mutated.
    /// # Errors
    /// Rejects empty/unsorted/duplicate selections, empty cells, extent/work bounds and overflow.
    pub fn from_world(
        world: &impl StaticGeometry,
        positions: &[IVec3],
    ) -> Result<(Self, MassPropertyReport), MassPropertyError> {
        Self::build(positions.len(), |index| {
            (positions[index], world.geometry_cell(positions[index]))
        })
    }

    pub(crate) fn from_body_voxels(voxels: &[BodyVoxel]) -> Result<Self, MassPropertyError> {
        Self::build(voxels.len(), |index| {
            let voxel = voxels[index];
            (voxel.position, GeometryCell::uniform(voxel.voxel))
        })
        .map(|(properties, _)| properties)
    }

    fn build(
        count: usize,
        cell_at: impl Fn(usize) -> (IVec3, GeometryCell),
    ) -> Result<(Self, MassPropertyReport), MassPropertyError> {
        if count == 0 {
            return Err(MassPropertyError::Empty);
        }
        if count > MAX_MASS_CELLS {
            return Err(MassPropertyError::CellBudget);
        }
        let mut minimum = cell_at(0).0;
        let mut maximum = minimum;
        let mut previous = None;
        for index in 0..count {
            let position = cell_at(index).0;
            if previous.is_some_and(|last| last >= position) {
                return Err(MassPropertyError::NonCanonicalPositions);
            }
            previous = Some(position);
            minimum = IVec3::new(
                minimum.x.min(position.x),
                minimum.y.min(position.y),
                minimum.z.min(position.z),
            );
            maximum = IVec3::new(
                maximum.x.max(position.x),
                maximum.y.max(position.y),
                maximum.z.max(position.z),
            );
        }
        for (low, high) in components(minimum).into_iter().zip(components(maximum)) {
            if high - low + 1 > MAX_MASS_EXTENT_CELLS {
                return Err(MassPropertyError::Extent);
            }
        }
        let mut result = Self {
            minimum,
            maximum,
            weight: 0,
            first: [0; 3],
            second: [0; 6],
        };
        let mut report = MassPropertyReport {
            cells: count,
            ..MassPropertyReport::default()
        };
        for index in 0..count {
            let (position, cell) = cell_at(index);
            let origin = std::array::from_fn(|axis| {
                i128::from(components(position)[axis] - components(minimum)[axis]) * LATTICE
            });
            let before = result.weight;
            if let Some(voxel) = cell.uniform_voxel() {
                charge(&mut report, 1)?;
                if voxel.is_solid() {
                    result.add_box(
                        origin,
                        LocalBox::FULL,
                        voxel.material.properties().density_kg_m3,
                    )?;
                    report.solid_leaves += 1;
                }
            } else if let Some(volume) = cell.volume() {
                charge(&mut report, volume.leaves().len())?;
                for leaf in volume.leaves() {
                    if leaf.voxel().is_solid() {
                        result.add_box(
                            origin,
                            leaf.bounds(),
                            leaf.voxel().material.properties().density_kg_m3,
                        )?;
                        report.solid_leaves += 1;
                    }
                }
            }
            if result.weight == before {
                return Err(MassPropertyError::EmptyCell(position));
            }
        }
        // Prove all published accessors' integer ranges before publishing any result.
        result.inertia_about_rounded_center()?;
        Ok((result, report))
    }

    fn add_box(
        &mut self,
        origin: [i128; 3],
        bounds: LocalBox,
        density: u16,
    ) -> Result<(), MassPropertyError> {
        if density == 0 {
            return Err(MassPropertyError::ZeroDensity);
        }
        let low: [i128; 3] =
            std::array::from_fn(|axis| origin[axis] + i128::from(bounds.minimum()[axis]));
        let high: [i128; 3] =
            std::array::from_fn(|axis| origin[axis] + i128::from(bounds.maximum()[axis]));
        let weight =
            i128::from(density) * (high[0] - low[0]) * (high[1] - low[1]) * (high[2] - low[2]);
        self.weight = checked_add(self.weight, weight)?;
        for axis in 0..3 {
            self.first[axis] = checked_add(self.first[axis], weight * (low[axis] + high[axis]))?;
        }
        for (index, &(i, j)) in SECOND_PAIRS.iter().enumerate() {
            let term = if i == j {
                4 * weight * (low[i].pow(2) + low[i] * high[i] + high[i].pow(2))
            } else {
                3 * weight * (low[i] + high[i]) * (low[j] + high[j])
            };
            self.second[index] = checked_add(self.second[index], term)?;
        }
        Ok(())
    }

    #[must_use]
    pub const fn mass_kg(&self) -> ExactRatio {
        ExactRatio {
            numerator: self.weight,
            denominator: MASS_DENOMINATOR,
        }
    }

    /// Exact centre coordinates in world millimetres; negative positions keep their sign.
    #[must_use]
    pub fn center_of_mass_mm(&self) -> [ExactRatio; 3] {
        let denominator = 2 * self.weight * LATTICE;
        std::array::from_fn(|axis| ExactRatio {
            numerator: i128::from(components(self.minimum)[axis]) * 1000 * denominator
                + self.first[axis] * 1000,
            denominator,
        })
    }

    /// Nearest 1/256 mm to the exact centre, with ties away from world zero. Inertia is exact about
    /// THAT pivot, not about an unrepresentable rational centre. No double rounding via whole mm.
    /// # Errors
    /// Rejects arithmetic overflow rather than returning saturated physical properties.
    pub fn inertia_about_rounded_center(&self) -> Result<InertiaTensor, MassPropertyError> {
        let center = self.center_of_mass_mm();
        let mut pivot = [0; 3];
        for axis in 0..3 {
            let value = ExactRatio {
                numerator: center[axis].numerator * LATTICE,
                denominator: center[axis].denominator,
            };
            pivot[axis] = i64::try_from(value.rounded_integer())
                .map_err(|_| MassPropertyError::ArithmeticOverflow)?;
        }
        self.inertia_about_pivot(pivot)
    }

    /// Exact tensor about a supplied world pivot in 1/256 mm, inside the selection's bounding box.
    /// # Errors
    /// Rejects out-of-bounds pivots and checked overflow; no arbitrary distant lever arms accepted.
    pub fn inertia_about_pivot(
        &self,
        pivot_scaled_mm: [i64; 3],
    ) -> Result<InertiaTensor, MassPropertyError> {
        let mut pivot = [0_i128; 3];
        for axis in 0..3 {
            let origin = components(self.minimum)[axis] * PIVOT_UNITS_PER_METER;
            let end = (components(self.maximum)[axis] + 1) * PIVOT_UNITS_PER_METER;
            if !(origin..=end).contains(&pivot_scaled_mm[axis]) {
                return Err(MassPropertyError::PivotBounds);
            }
            pivot[axis] = i128::from(pivot_scaled_mm[axis] - origin);
        }
        let mut about = [0_i128; 6];
        for (index, &(i, j)) in SECOND_PAIRS.iter().enumerate() {
            let second = self.second[index]
                .checked_mul(1_000_000)
                .ok_or(MassPropertyError::ArithmeticOverflow)?;
            let first = 6000 * (pivot[i] * self.first[j] + pivot[j] * self.first[i]);
            let offset = 12 * self.weight * pivot[i] * pivot[j];
            about[index] = checked_add(second - first, offset)?;
        }
        let numerator = [
            checked_add(about[1], about[2])?,
            checked_add(about[0], about[2])?,
            checked_add(about[0], about[1])?,
            -about[3],
            -about[4],
            -about[5],
        ];
        let denominator = 12 * MASS_DENOMINATOR * i128::from(PIVOT_UNITS_PER_METER).pow(2);
        Ok(InertiaTensor {
            pivot_scaled_mm,
            entries: numerator.map(|numerator| ExactRatio {
                numerator,
                denominator,
            }),
        })
    }
}

fn components(position: IVec3) -> [i64; 3] {
    [
        i64::from(position.x),
        i64::from(position.y),
        i64::from(position.z),
    ]
}
fn checked_add(left: i128, right: i128) -> Result<i128, MassPropertyError> {
    left.checked_add(right)
        .ok_or(MassPropertyError::ArithmeticOverflow)
}
const fn charge(report: &mut MassPropertyReport, leaves: usize) -> Result<(), MassPropertyError> {
    if leaves > MAX_MASS_LEAVES - report.leaves {
        return Err(MassPropertyError::LeafBudget);
    }
    report.leaves += leaves;
    Ok(())
}

#[cfg(test)]
mod tests;
