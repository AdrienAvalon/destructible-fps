//! Exact queries for closed authored convex fragments, outside gameplay World storage.
//!
//! Rays return strict-interior chords: a segment coplanar with any face has no chord. This
//! deliberately differs from the lower-closed/upper-open ownership of legacy volume box faces.
//! Continuous sweeps have fixed orientation and one translation axis; no angular sweep is implied.
use super::{ConvexError, ConvexFragment};
use crate::{
    IVec3, Voxel,
    ballistics::FixedRay,
    volume::ray::SegmentParameter,
    world::query::{PhysicalBox, SweepResult},
};
use std::cmp::Ordering;

const SCALE: i64 = 256;
const LATTICE_TO_SCALED_UM: i64 = 1_000_000;
const LOCAL_BOUND: i64 = 256_000_000 * SCALE;
const BOX_WIDTH: i64 = 16_000_000 * SCALE;
const MAX_DISPLACEMENT: i64 = 128_000_000;
const XYZ: [[i64; 3]; 3] = [[1, 0, 0], [0, 1, 0], [0, 0, 1]];
pub const MAX_CONVEX_QUERY_FRAGMENTS: usize = 65_536;
pub const MAX_CONVEX_QUERY_AXES: usize = 1_048_576;
pub const MAX_CONVEX_QUERY_PROJECTIONS: usize = 16_777_216;

#[derive(Clone, Copy, Debug)]
pub struct ConvexQueryLimits {
    pub fragments: usize,
    pub axes: usize,
    /// One dot product, including each projected vertex, counts as one projection.
    pub projections: usize,
}
impl Default for ConvexQueryLimits {
    fn default() -> Self {
        Self {
            fragments: 64,
            axes: 16_384,
            projections: 1_048_576,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ConvexQueryStats {
    pub fragments: usize,
    pub axes: usize,
    pub projections: usize,
}

pub struct ConvexQueryBudget {
    limits: ConvexQueryLimits,
    stats: ConvexQueryStats,
}
impl ConvexQueryBudget {
    /// # Errors
    /// Rejects zero or above-contract limits. Failed queries retain their consumed work.
    pub fn new(limits: ConvexQueryLimits) -> Result<Self, ConvexError> {
        if [
            (limits.fragments, MAX_CONVEX_QUERY_FRAGMENTS),
            (limits.axes, MAX_CONVEX_QUERY_AXES),
            (limits.projections, MAX_CONVEX_QUERY_PROJECTIONS),
        ]
        .into_iter()
        .any(|(value, maximum)| value == 0 || value > maximum)
        {
            return Err(ConvexError::Budget);
        }
        Ok(Self {
            limits,
            stats: ConvexQueryStats::default(),
        })
    }
    #[must_use]
    pub const fn stats(&self) -> ConvexQueryStats {
        self.stats
    }
    const fn fragment(&mut self) -> Result<(), ConvexError> {
        charge(&mut self.stats.fragments, 1, self.limits.fragments)
    }
    const fn axis(&mut self) -> Result<(), ConvexError> {
        charge(&mut self.stats.axes, 1, self.limits.axes)
    }
    const fn projections(&mut self, count: usize) -> Result<(), ConvexError> {
        charge(&mut self.stats.projections, count, self.limits.projections)
    }
}
const fn charge(used: &mut usize, count: usize, maximum: usize) -> Result<(), ConvexError> {
    if count > maximum - *used {
        return Err(ConvexError::Budget);
    }
    *used += count;
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ConvexChord {
    pub entry: SegmentParameter,
    pub exit: SegmentParameter,
    pub material: Voxel,
}

#[derive(Clone, Copy)]
struct Fraction {
    numerator: i64,
    denominator: i64,
}
impl Fraction {
    const ZERO: Self = Self {
        numerator: 0,
        denominator: 1,
    };
    const ONE: Self = Self {
        numerator: 1,
        denominator: 1,
    };
    fn new(numerator: i64, denominator: i64) -> Result<Self, ConvexError> {
        if denominator == 0 {
            return Err(ConvexError::Arithmetic);
        }
        if denominator < 0 {
            Ok(Self {
                numerator: numerator.checked_neg().ok_or(ConvexError::Arithmetic)?,
                denominator: denominator.checked_neg().ok_or(ConvexError::Arithmetic)?,
            })
        } else {
            Ok(Self {
                numerator,
                denominator,
            })
        }
    }
    fn compare(self, other: Self) -> Ordering {
        (i128::from(self.numerator) * i128::from(other.denominator))
            .cmp(&(i128::from(other.numerator) * i128::from(self.denominator)))
    }
    fn parameter(self) -> Result<SegmentParameter, ConvexError> {
        SegmentParameter::bounded(self.numerator, self.denominator).ok_or(ConvexError::Arithmetic)
    }
}

fn narrow(value: i128) -> Result<i64, ConvexError> {
    i64::try_from(value).map_err(|_| ConvexError::Arithmetic)
}
fn dot(axis: [i64; 3], point: [i64; 3]) -> Result<i64, ConvexError> {
    narrow(
        (0..3)
            .map(|i| i128::from(axis[i]) * i128::from(point[i]))
            .sum(),
    )
}
fn cross(a: [i64; 3], b: [i64; 3]) -> Result<[i64; 3], ConvexError> {
    Ok([
        narrow(i128::from(a[1]) * i128::from(b[2]) - i128::from(a[2]) * i128::from(b[1]))?,
        narrow(i128::from(a[2]) * i128::from(b[0]) - i128::from(a[0]) * i128::from(b[2]))?,
        narrow(i128::from(a[0]) * i128::from(b[1]) - i128::from(a[1]) * i128::from(b[0]))?,
    ])
}
fn origin_scaled(origin: IVec3) -> [i64; 3] {
    [origin.x, origin.y, origin.z].map(|v| i64::from(v) * LATTICE_TO_SCALED_UM * SCALE)
}
fn local_point(point: [i64; 3], origin: [i64; 3]) -> Result<[i64; 3], ConvexError> {
    let mut local = [0; 3];
    for i in 0..3 {
        local[i] = narrow(i128::from(point[i]) - i128::from(origin[i]))?;
        if !(-LOCAL_BOUND..=LOCAL_BOUND).contains(&local[i]) {
            return Err(ConvexError::Bounds);
        }
    }
    Ok(local)
}
fn validate_box(bounds: PhysicalBox) -> Result<(), ConvexError> {
    if (0..3).any(|i| bounds.maximum_scaled()[i] - bounds.minimum_scaled()[i] > BOX_WIDTH) {
        return Err(ConvexError::Bounds);
    }
    Ok(())
}
fn boxes_touch(a: PhysicalBox, b: PhysicalBox) -> bool {
    (0..3).all(|i| {
        a.minimum_scaled()[i] <= b.maximum_scaled()[i]
            && a.maximum_scaled()[i] >= b.minimum_scaled()[i]
    })
}

#[derive(Clone, Copy)]
struct BoxProjectionSource {
    minimum: [i64; 3],
    maximum: [i64; 3],
}
impl BoxProjectionSource {
    fn new(bounds: PhysicalBox, origin: IVec3) -> Result<Self, ConvexError> {
        let origin = origin_scaled(origin);
        Ok(Self {
            minimum: local_point(bounds.minimum_scaled(), origin)?,
            maximum: local_point(bounds.maximum_scaled(), origin)?,
        })
    }
    fn project(
        self,
        axis: [i64; 3],
        budget: &mut ConvexQueryBudget,
    ) -> Result<(i64, i64), ConvexError> {
        budget.projections(2)?;
        let low = std::array::from_fn(|i| {
            if axis[i] >= 0 {
                self.minimum[i]
            } else {
                self.maximum[i]
            }
        });
        let high = std::array::from_fn(|i| {
            if axis[i] >= 0 {
                self.maximum[i]
            } else {
                self.minimum[i]
            }
        });
        Ok((dot(axis, low)?, dot(axis, high)?))
    }
}

impl ConvexFragment {
    fn project(
        &self,
        axis: [i64; 3],
        shift: [i64; 3],
        budget: &mut ConvexQueryBudget,
    ) -> Result<(i64, i64), ConvexError> {
        budget.projections(self.vertices().len())?;
        let mut low = i64::MAX;
        let mut high = i64::MIN;
        for vertex in self.vertices() {
            let point =
                std::array::from_fn(|i| i64::from(vertex[i]) * LATTICE_TO_SCALED_UM + shift[i]);
            let projection = dot(axis, point)?;
            low = low.min(projection);
            high = high.max(projection);
        }
        Ok((low, high))
    }
    fn edge_direction(&self, edge: [u8; 2]) -> [i64; 3] {
        let a = self.vertices()[usize::from(edge[0])];
        let b = self.vertices()[usize::from(edge[1])];
        std::array::from_fn(|i| i64::from(b[i]) - i64::from(a[i]))
    }
    fn box_axes(
        &self,
        budget: &mut ConvexQueryBudget,
        mut visit: impl FnMut([i64; 3], &mut ConvexQueryBudget) -> Result<bool, ConvexError>,
    ) -> Result<bool, ConvexError> {
        for axis in self.planes().iter().map(|p| p.normal()).chain(XYZ) {
            budget.axis()?;
            if !visit(axis, budget)? {
                return Ok(false);
            }
        }
        for &edge in self.edges() {
            for basis in XYZ {
                budget.axis()?;
                let axis = cross(self.edge_direction(edge), basis)?;
                if axis != [0; 3] && !visit(axis, budget)? {
                    return Ok(false);
                }
            }
        }
        Ok(true)
    }

    /// Exact positive-length strict-interior chord, with the original `FixedRay` parameters.
    /// # Errors
    /// Refuses exhausted shared work or arithmetic/local-domain violations; no partial chord.
    pub fn trace(
        &self,
        ray: &FixedRay,
        budget: &mut ConvexQueryBudget,
    ) -> Result<Option<ConvexChord>, ConvexError> {
        budget.fragment()?;
        let mut start = [0; 3];
        let mut end = [0; 3];
        for i in 0..3 {
            start[i] = narrow(i128::from(ray.origin[i]) * i128::from(SCALE))?;
            end[i] =
                narrow((i128::from(ray.origin[i]) + i128::from(ray.delta[i])) * i128::from(SCALE))?;
        }
        let bounds = self.bounds();
        if (0..3).any(|i| {
            start[i].max(end[i]) < bounds.minimum_scaled()[i]
                || start[i].min(end[i]) > bounds.maximum_scaled()[i]
        }) {
            return Ok(None);
        }
        let origin = origin_scaled(self.origin());
        start = local_point(start, origin)?;
        end = local_point(end, origin)?;
        let delta = std::array::from_fn(|i| end[i] - start[i]);
        let mut entry = Fraction::ZERO;
        let mut exit = Fraction::ONE;
        for plane in self.planes() {
            budget.axis()?;
            budget.projections(2)?;
            let distance = narrow(
                i128::from(plane.offset()) * i128::from(LATTICE_TO_SCALED_UM)
                    - i128::from(dot(plane.normal(), start)?),
            )?;
            let velocity = dot(plane.normal(), delta)?;
            if velocity == 0 {
                if distance <= 0 {
                    return Ok(None);
                }
                continue;
            }
            let time = Fraction::new(distance, velocity)?;
            if velocity < 0 {
                if time.compare(entry).is_gt() {
                    entry = time;
                }
            } else if time.compare(exit).is_lt() {
                exit = time;
            }
            if !entry.compare(exit).is_lt() {
                return Ok(None);
            }
        }
        Ok(Some(ConvexChord {
            entry: entry.parameter()?,
            exit: exit.parameter()?,
            material: self.material(),
        }))
    }

    /// Positive-volume overlap, not mere closed-boundary touching.
    /// # Errors
    /// Refuses invalid query size/domain, arithmetic failure or exhausted shared work.
    pub fn overlaps(
        &self,
        bounds: PhysicalBox,
        budget: &mut ConvexQueryBudget,
    ) -> Result<bool, ConvexError> {
        budget.fragment()?;
        validate_box(bounds)?;
        if !self.bounds().overlaps(bounds) {
            return Ok(false);
        }
        let box_source = BoxProjectionSource::new(bounds, self.origin())?;
        self.box_axes(budget, |axis, budget| {
            let (low, high) = self.project(axis, [0; 3], budget)?;
            let (box_low, box_high) = box_source.project(axis, budget)?;
            Ok(box_low < high && box_high > low)
        })
    }

    /// Continuous translation of a fixed AABB. Sliding/isolated grazing does not block movement.
    /// # Errors
    /// Refuses invalid inputs, positive-volume initial overlap, arithmetic or exhausted budgets.
    pub fn sweep_axis(
        &self,
        start: PhysicalBox,
        axis: usize,
        displacement_um: i64,
        budget: &mut ConvexQueryBudget,
    ) -> Result<SweepResult, ConvexError> {
        budget.fragment()?;
        validate_box(start)?;
        if axis >= 3 || displacement_um.unsigned_abs() > MAX_DISPLACEMENT.cast_unsigned() {
            return Err(ConvexError::Bounds);
        }
        let mut end_low = start.minimum_scaled();
        let mut end_high = start.maximum_scaled();
        let displacement = displacement_um * SCALE;
        end_low[axis] = narrow(i128::from(end_low[axis]) + i128::from(displacement))?;
        end_high[axis] = narrow(i128::from(end_high[axis]) + i128::from(displacement))?;
        let end = PhysicalBox::from_scaled(end_low, end_high).map_err(|_| ConvexError::Bounds)?;
        let swept = PhysicalBox::from_scaled(
            std::array::from_fn(|i| start.minimum_scaled()[i].min(end.minimum_scaled()[i])),
            std::array::from_fn(|i| start.maximum_scaled()[i].max(end.maximum_scaled()[i])),
        )
        .map_err(|_| ConvexError::Bounds)?;
        let clear = SweepResult {
            displacement_um,
            contact: false,
        };
        if !boxes_touch(self.bounds(), swept) {
            return Ok(clear);
        }
        let box_source = BoxProjectionSource::new(start, self.origin())?;
        BoxProjectionSource::new(end, self.origin())?;
        let mut window = SweepWindow::default();
        let possible = self.box_axes(budget, |normal, budget| {
            let fixed = self.project(normal, [0; 3], budget)?;
            let moving = box_source.project(normal, budget)?;
            let velocity = narrow(i128::from(normal[axis]) * i128::from(displacement))?;
            window.axis(fixed, moving, velocity)
        })?;
        if !possible {
            return Ok(clear);
        }
        if window.initial_overlap {
            return Err(ConvexError::InitialOverlap);
        }
        if displacement_um == 0
            || window.entry.compare(Fraction::ONE).is_gt()
            || window
                .exit
                .is_some_and(|exit| !window.entry.compare(exit).is_lt())
        {
            return Ok(clear);
        }
        let parameter = window.entry.parameter()?;
        Ok(SweepResult {
            displacement_um: narrow(
                i128::from(displacement_um) * i128::from(parameter.numerator())
                    / i128::from(parameter.denominator()),
            )?,
            contact: true,
        })
    }

    /// Exact positive-volume fragment overlap for bounded, atomic inspection-scene validation.
    /// # Errors
    /// Refuses arithmetic/local-domain violations or exhausted shared work.
    pub fn overlaps_fragment(
        &self,
        other: &Self,
        budget: &mut ConvexQueryBudget,
    ) -> Result<bool, ConvexError> {
        budget.fragment()?;
        if !self.bounds().overlaps(other.bounds()) {
            return Ok(false);
        }
        let shift = local_point(origin_scaled(other.origin()), origin_scaled(self.origin()))?;
        let separated = |axis, budget: &mut ConvexQueryBudget| -> Result<bool, ConvexError> {
            let a = self.project(axis, [0; 3], budget)?;
            let b = other.project(axis, shift, budget)?;
            Ok(a.0 >= b.1 || b.0 >= a.1)
        };
        for axis in self
            .planes()
            .iter()
            .chain(other.planes())
            .map(|p| p.normal())
        {
            budget.axis()?;
            if separated(axis, budget)? {
                return Ok(false);
            }
        }
        for &a in self.edges() {
            for &b in other.edges() {
                budget.axis()?;
                let axis = cross(self.edge_direction(a), other.edge_direction(b))?;
                if axis != [0; 3] && separated(axis, budget)? {
                    return Ok(false);
                }
            }
        }
        Ok(true)
    }
}

struct SweepWindow {
    entry: Fraction,
    exit: Option<Fraction>,
    initial_overlap: bool,
}
impl Default for SweepWindow {
    fn default() -> Self {
        Self {
            entry: Fraction::ZERO,
            exit: None,
            initial_overlap: true,
        }
    }
}
impl SweepWindow {
    fn axis(
        &mut self,
        fixed: (i64, i64),
        moving: (i64, i64),
        velocity: i64,
    ) -> Result<bool, ConvexError> {
        let overlaps = moving.0 < fixed.1 && moving.1 > fixed.0;
        self.initial_overlap &= overlaps;
        if velocity == 0 {
            return Ok(overlaps);
        }
        let a = Fraction::new(
            narrow(i128::from(fixed.0) - i128::from(moving.1))?,
            velocity,
        )?;
        let b = Fraction::new(
            narrow(i128::from(fixed.1) - i128::from(moving.0))?,
            velocity,
        )?;
        let (near, far) = if a.compare(b).is_lt() { (a, b) } else { (b, a) };
        if near.compare(self.entry).is_gt() {
            self.entry = near;
        }
        if self.exit.is_none_or(|old| far.compare(old).is_lt()) {
            self.exit = Some(far);
        }
        Ok(far.compare(Fraction::ZERO).is_gt()
            && self
                .exit
                .is_none_or(|exit| self.entry.compare(exit).is_lt()))
    }
}

#[cfg(test)]
mod tests;
