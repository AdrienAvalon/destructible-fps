//! Exact, bounded static overlap and axis sweeps shared by coarse and fine World storage.
//!
//! Physical coordinates are scaled by 256 so every fine plane remains exact. A character's
//! integer-micrometre displacement rounds toward its starting position, never into a solid.

use super::{
    IVec3, World,
    geometry::{GeometryCell, RefinedWorld},
};
use crate::{
    MICROMETERS_PER_VOXEL, Voxel,
    volume::{LocalBox, MAX_VOLUME_LEAVES, VOLUME_EDGE},
};
use core::fmt;

pub mod ray;

pub const SCALED_PER_MICROMETER: i64 = VOLUME_EDGE as i64;
const CELL_SCALED: i64 = MICROMETERS_PER_VOXEL * SCALED_PER_MICROMETER;
const MINIMUM_SCALED: i64 = i32::MIN as i64 * CELL_SCALED;
const MAXIMUM_SCALED: i64 = (i32::MAX as i64 + 1) * CELL_SCALED;
pub const MAX_QUERY_CELLS: usize = 512;
pub const MAX_QUERY_BUDGET_CELLS: usize = 16_384;
pub const MAX_QUERY_LEAF_VISITS: usize = 262_144;
pub const MAX_SWEEP_DISPLACEMENT_UM: i64 = 128 * MICROMETERS_PER_VOXEL;

mod sealed {
    pub trait Sealed {}
    impl Sealed for super::World {}
    impl Sealed for super::RefinedWorld {}
}

/// Only exact engine-owned storage implementations may satisfy physical queries.
pub trait StaticGeometry: sealed::Sealed {
    fn geometry_cell(&self, position: IVec3) -> GeometryCell;
    fn geometry_fingerprint(&self) -> u128;
}
impl StaticGeometry for World {
    fn geometry_cell(&self, position: IVec3) -> GeometryCell {
        GeometryCell::uniform(self.voxel(position))
    }
    fn geometry_fingerprint(&self) -> u128 {
        self.fingerprint()
    }
}
impl StaticGeometry for RefinedWorld {
    fn geometry_cell(&self, position: IVec3) -> GeometryCell {
        self.cell(position)
    }
    fn geometry_fingerprint(&self) -> u128 {
        self.fingerprint()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GeometryQueryError {
    InvalidBox,
    WorldBounds,
    InvalidAxis,
    InvalidDisplacement,
    InvalidLimits,
    CellBudget,
    LeafBudget,
    InitialOverlap,
    UnsafeSpawn,
}
impl fmt::Display for GeometryQueryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "static geometry query refused: {self:?}")
    }
}
impl std::error::Error for GeometryQueryError {}

/// Nonempty half-open box, coordinates in 1/256 micrometre, bounded to the i32-page world.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PhysicalBox {
    minimum: [i64; 3],
    maximum: [i64; 3],
}
impl PhysicalBox {
    /// # Errors
    /// Rejects empty/inverted boxes and coordinates beyond the world's physical domain.
    pub fn from_scaled(minimum: [i64; 3], maximum: [i64; 3]) -> Result<Self, GeometryQueryError> {
        if (0..3).any(|axis| minimum[axis] >= maximum[axis]) {
            return Err(GeometryQueryError::InvalidBox);
        }
        if (0..3).any(|axis| minimum[axis] < MINIMUM_SCALED || maximum[axis] > MAXIMUM_SCALED) {
            return Err(GeometryQueryError::WorldBounds);
        }
        Ok(Self { minimum, maximum })
    }

    /// # Errors
    /// Rejects invalid boxes, checked scaling overflow and coordinates beyond the world domain.
    pub fn from_micrometers(
        minimum: [i64; 3],
        maximum: [i64; 3],
    ) -> Result<Self, GeometryQueryError> {
        let scale = |input: [i64; 3]| -> Result<[i64; 3], GeometryQueryError> {
            let mut scaled = [0; 3];
            for axis in 0..3 {
                scaled[axis] = input[axis]
                    .checked_mul(SCALED_PER_MICROMETER)
                    .ok_or(GeometryQueryError::WorldBounds)?;
            }
            Ok(scaled)
        };
        Self::from_scaled(scale(minimum)?, scale(maximum)?)
    }

    #[must_use]
    pub const fn minimum_scaled(self) -> [i64; 3] {
        self.minimum
    }
    #[must_use]
    pub const fn maximum_scaled(self) -> [i64; 3] {
        self.maximum
    }

    #[must_use]
    pub fn overlaps(self, other: Self) -> bool {
        (0..3).all(|axis| {
            self.minimum[axis] < other.maximum[axis] && self.maximum[axis] > other.minimum[axis]
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct QueryLimits {
    pub cells: usize,
    pub leaf_visits: usize,
}
impl Default for QueryLimits {
    fn default() -> Self {
        Self {
            cells: 128,
            leaf_visits: 65_536,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct QueryStats {
    pub cells: usize,
    pub leaf_visits: usize,
    pub solid_boxes: usize,
}

pub struct QueryBudget {
    limits: QueryLimits,
    stats: QueryStats,
}
impl QueryBudget {
    /// A caller may share one budget across a whole tick or reconciliation, not reset it per cell.
    /// # Errors
    /// Rejects zero or above-contract limits.
    pub fn new(limits: QueryLimits) -> Result<Self, GeometryQueryError> {
        if !(1..=MAX_QUERY_BUDGET_CELLS).contains(&limits.cells)
            || !(1..=MAX_QUERY_LEAF_VISITS).contains(&limits.leaf_visits)
        {
            return Err(GeometryQueryError::InvalidLimits);
        }
        Ok(Self {
            limits,
            stats: QueryStats::default(),
        })
    }
    #[must_use]
    pub const fn stats(&self) -> QueryStats {
        self.stats
    }
}

/// Any positive-volume intersection with a solid, not a contact/tangency test.
/// # Errors
/// Rejects excessive candidate cells or traversal work. Exhaustion never means clear or solid.
pub fn overlaps_solid(
    world: &impl StaticGeometry,
    bounds: PhysicalBox,
    budget: &mut QueryBudget,
) -> Result<bool, GeometryQueryError> {
    let mut found = false;
    visit_solids(world, bounds, budget, |solid, _| {
        found = bounds.overlaps(solid);
        Ok(!found)
    })?;
    Ok(found)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SweepResult {
    pub displacement_um: i64,
    pub contact: bool,
}

/// Continuous axis sweep through actual material boxes, not an endpoint overlap approximation.
/// # Errors
/// Rejects invalid axis/displacement/world bounds, initial penetration or exhausted query budget.
pub fn sweep_axis(
    world: &impl StaticGeometry,
    start: PhysicalBox,
    axis: usize,
    displacement_um: i64,
    budget: &mut QueryBudget,
) -> Result<SweepResult, GeometryQueryError> {
    if axis >= 3 {
        return Err(GeometryQueryError::InvalidAxis);
    }
    if displacement_um.unsigned_abs() > MAX_SWEEP_DISPLACEMENT_UM.cast_unsigned() {
        return Err(GeometryQueryError::InvalidDisplacement);
    }
    if displacement_um == 0 {
        if overlaps_solid(world, start, budget)? {
            return Err(GeometryQueryError::InitialOverlap);
        }
        return Ok(SweepResult {
            displacement_um: 0,
            contact: false,
        });
    }
    let displacement = displacement_um * SCALED_PER_MICROMETER;
    let mut minimum = start.minimum;
    let mut maximum = start.maximum;
    let mut end_minimum = minimum;
    let mut end_maximum = maximum;
    end_minimum[axis] += displacement;
    end_maximum[axis] += displacement;
    PhysicalBox::from_scaled(end_minimum, end_maximum)?;
    if displacement > 0 {
        maximum[axis] = (end_maximum[axis] + 1).min(MAXIMUM_SCALED);
    } else {
        minimum[axis] = (end_minimum[axis] - 1).max(MINIMUM_SCALED);
    }
    let swept = PhysicalBox::from_scaled(minimum, maximum)?;
    let mut allowed = displacement;
    let mut contact = false;
    visit_solids(world, swept, budget, |solid, _| {
        if (0..3).any(|other| {
            other != axis
                && (start.minimum[other] >= solid.maximum[other]
                    || start.maximum[other] <= solid.minimum[other])
        }) {
            return Ok(true);
        }
        if start.overlaps(solid) {
            return Err(GeometryQueryError::InitialOverlap);
        }
        let gap = if displacement > 0 {
            solid.minimum[axis] - start.maximum[axis]
        } else {
            solid.maximum[axis] - start.minimum[axis]
        };
        if (displacement > 0 && gap >= 0 && gap <= allowed)
            || (displacement < 0 && gap <= 0 && gap >= allowed)
        {
            allowed = gap;
            contact = true;
        }
        Ok(true)
    })?;
    Ok(SweepResult {
        displacement_um: allowed / SCALED_PER_MICROMETER,
        contact,
    })
}

pub(crate) fn visit_solids(
    world: &impl StaticGeometry,
    bounds: PhysicalBox,
    budget: &mut QueryBudget,
    mut visit: impl FnMut(PhysicalBox, Voxel) -> Result<bool, GeometryQueryError>,
) -> Result<(), GeometryQueryError> {
    let minimum = bounds.minimum.map(|value| value.div_euclid(CELL_SCALED));
    let maximum = bounds
        .maximum
        .map(|value| (value - 1).div_euclid(CELL_SCALED));
    let mut count = 1_usize;
    for axis in 0..3 {
        count = count
            .checked_mul(
                usize::try_from(maximum[axis] - minimum[axis] + 1)
                    .map_err(|_| GeometryQueryError::CellBudget)?,
            )
            .ok_or(GeometryQueryError::CellBudget)?;
    }
    if count > MAX_QUERY_CELLS || count > budget.limits.cells - budget.stats.cells {
        return Err(GeometryQueryError::CellBudget);
    }
    for x in minimum[0]..=maximum[0] {
        for y in minimum[1]..=maximum[1] {
            for z in minimum[2]..=maximum[2] {
                budget.stats.cells += 1;
                let position = IVec3::new(
                    i32::try_from(x).map_err(|_| GeometryQueryError::WorldBounds)?,
                    i32::try_from(y).map_err(|_| GeometryQueryError::WorldBounds)?,
                    i32::try_from(z).map_err(|_| GeometryQueryError::WorldBounds)?,
                );
                let origin = [x, y, z].map(|value| value * CELL_SCALED);
                let cell = world.geometry_cell(position);
                let remaining = budget.limits.leaf_visits - budget.stats.leaf_visits;
                if remaining == 0 {
                    return Err(GeometryQueryError::LeafBudget);
                }
                if let Some(voxel) = cell.uniform_voxel() {
                    budget.stats.leaf_visits += 1;
                    if voxel.is_solid() {
                        budget.stats.solid_boxes += 1;
                        if !visit(
                            PhysicalBox {
                                minimum: origin,
                                maximum: origin.map(|value| value + CELL_SCALED),
                            },
                            voxel,
                        )? {
                            return Ok(());
                        }
                    }
                } else if let Some(volume) = cell.volume()
                    && !visit_refined(volume, origin, bounds, budget, &mut visit)?
                {
                    return Ok(());
                }
            }
        }
    }
    Ok(())
}

fn visit_refined(
    volume: &crate::volume::RefinedVolume,
    origin: [i64; 3],
    bounds: PhysicalBox,
    budget: &mut QueryBudget,
    visit: &mut impl FnMut(PhysicalBox, Voxel) -> Result<bool, GeometryQueryError>,
) -> Result<bool, GeometryQueryError> {
    let mut minimum = [0; 3];
    let mut maximum = [0; 3];
    for axis in 0..3 {
        let low = bounds.minimum[axis].max(origin[axis]) - origin[axis];
        let high = bounds.maximum[axis].min(origin[axis] + CELL_SCALED) - origin[axis];
        minimum[axis] = u16::try_from(low / MICROMETERS_PER_VOXEL)
            .map_err(|_| GeometryQueryError::InvalidBox)?;
        maximum[axis] = u16::try_from((high + MICROMETERS_PER_VOXEL - 1) / MICROMETERS_PER_VOXEL)
            .map_err(|_| GeometryQueryError::InvalidBox)?;
    }
    let local = LocalBox::new(minimum, maximum).map_err(|_| GeometryQueryError::InvalidBox)?;
    let maximum_visits =
        (budget.limits.leaf_visits - budget.stats.leaf_visits).min(3 * MAX_VOLUME_LEAVES);
    let mut outcome = Ok(true);
    let result = volume.visit_solid_bounded(local, maximum_visits, |leaf| {
        budget.stats.solid_boxes += 1;
        let solid = PhysicalBox {
            minimum: std::array::from_fn(|axis| {
                origin[axis] + i64::from(leaf.bounds().minimum()[axis]) * MICROMETERS_PER_VOXEL
            }),
            maximum: std::array::from_fn(|axis| {
                origin[axis] + i64::from(leaf.bounds().maximum()[axis]) * MICROMETERS_PER_VOXEL
            }),
        };
        outcome = visit(solid, leaf.voxel());
        matches!(outcome, Ok(true))
    });
    if let Ok(visits) = result {
        budget.stats.leaf_visits += visits;
    } else {
        // The scan reports budget exhaustion only after using its full assigned work allowance.
        budget.stats.leaf_visits += maximum_visits;
        return Err(GeometryQueryError::LeafBudget);
    }
    outcome
}

#[cfg(test)]
mod tests;
