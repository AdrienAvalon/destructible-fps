//! Bounded directional material work and server-side rifle state.
//!
//! Resistance/energy values are fictional game units, not calibrated real-world ballistics.
//! One integrity value still represents a whole metre-scale cell; sub-cell cavities remain work.

pub mod ray;
pub mod state_wire;

use crate::{BodyId, DestructionReport, FixedMicrometers3, Material, Voxel, World};
use core::fmt;
pub use ray::FixedRay;

pub const MAX_RIFLE_RANGE_UM: u64 = 120_000_000;
pub const RIFLE_ENERGY: u32 = 750;
pub const RIFLE_MAGAZINE: u16 = 30;
pub const RIFLE_RESERVE: u16 = 90;
pub const RIFLE_CADENCE_TICKS: u64 = 6;
pub const RIFLE_RELOAD_TICKS: u64 = 120;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RifleCommand {
    pub command_id: u64,
    pub direction: [i16; 3],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BallisticError {
    InvalidOrigin,
    InvalidCommandId,
    ZeroDirection,
    InvalidRange,
    RayBudget,
    FireTooSoon,
    Reloading,
    EmptyMagazine,
    ReloadUnavailable,
    ClockExhausted,
}

impl fmt::Display for BallisticError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "rifle request refused: {self:?}")
    }
}
impl std::error::Error for BallisticError {}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RifleState {
    pub magazine: u16,
    pub reserve: u16,
    pub next_fire_tick: u64,
    pub reload_complete_tick: Option<u64>,
}

impl Default for RifleState {
    fn default() -> Self {
        Self {
            magazine: RIFLE_MAGAZINE,
            reserve: RIFLE_RESERVE,
            next_fire_tick: 0,
            reload_complete_tick: None,
        }
    }
}

impl RifleState {
    #[must_use]
    pub fn at(mut self, tick: u64) -> Self {
        if self.reload_complete_tick.is_some_and(|end| tick >= end) {
            let transferred = RIFLE_MAGAZINE
                .saturating_sub(self.magazine)
                .min(self.reserve);
            self.magazine += transferred;
            self.reserve -= transferred;
            self.reload_complete_tick = None;
        }
        self
    }

    /// Computes a candidate; the caller installs it only after the world transaction succeeds.
    /// # Errors
    /// Rejects cadence, reload, ammunition and tick-overflow violations without changing self.
    pub fn after_shot(self, tick: u64) -> Result<Self, BallisticError> {
        let mut candidate = self.at(tick);
        if candidate.reload_complete_tick.is_some() {
            return Err(BallisticError::Reloading);
        }
        if tick < candidate.next_fire_tick {
            return Err(BallisticError::FireTooSoon);
        }
        if candidate.magazine == 0 {
            return Err(BallisticError::EmptyMagazine);
        }
        candidate.next_fire_tick = tick
            .checked_add(RIFLE_CADENCE_TICKS)
            .ok_or(BallisticError::ClockExhausted)?;
        candidate.magazine -= 1;
        Ok(candidate)
    }

    /// # Errors
    /// Rejects unavailable/repeated reloads and exhausted simulation clocks.
    pub fn after_reload(self, tick: u64) -> Result<Self, BallisticError> {
        let mut candidate = self.at(tick);
        if candidate.reload_complete_tick.is_some() {
            return Err(BallisticError::Reloading);
        }
        if candidate.magazine == RIFLE_MAGAZINE || candidate.reserve == 0 {
            return Err(BallisticError::ReloadUnavailable);
        }
        candidate.reload_complete_tick = Some(
            tick.checked_add(RIFLE_RELOAD_TICKS)
                .ok_or(BallisticError::ClockExhausted)?,
        );
        Ok(candidate)
    }
}

#[derive(Clone, Debug)]
pub struct ProjectileReport {
    pub destruction: DestructionReport,
    pub first_hit: Option<crate::IVec3>,
    pub spent_energy: u32,
    pub remaining_energy: u32,
    pub visited_cells: usize,
    pub blocked_by_body: Option<BodyId>,
}

/// Fictional work needed to remove a metre-long pristine material interval.
#[must_use]
pub const fn resistance(material: Material) -> u32 {
    match material {
        Material::Air => 0,
        Material::Glass => 300,
        Material::Wood => 2_000,
        Material::Soil => 3_000,
        Material::Brick => 6_000,
        Material::Concrete => 12_000,
        Material::Stone => 15_000,
        Material::Steel => 50_000,
    }
}

/// Mutates only cells actually traversed by the shot; the authority owns rollback/promotion.
/// # Errors
/// Rejects malformed rays before modifying any voxel.
/// # Panics
/// Only if the internal integrity/remaining-energy conversion bounds are violated.
pub fn apply_rifle(
    world: &mut World,
    origin: FixedMicrometers3,
    direction: [i16; 3],
    body_cover: Option<(BodyId, u64)>,
) -> Result<ProjectileReport, BallisticError> {
    let ray = FixedRay::new(origin, direction, MAX_RIFLE_RANGE_UM)?;
    let cells = ray.cells()?;
    let mut report = ProjectileReport {
        destruction: DestructionReport::default(),
        first_hit: None,
        spent_energy: 0,
        remaining_energy: RIFLE_ENERGY,
        visited_cells: 0,
        blocked_by_body: None,
    };
    for cell in cells {
        if let Some((id, distance)) = body_cover
            && (distance <= cell.enter_um || distance < cell.exit_um)
        {
            report.blocked_by_body = Some(id);
            break;
        }
        report.visited_cells += 1;
        let before = world.voxel(cell.position);
        if before.is_solid() {
            // Coplanar adjacent cells do not suppress a positive-length hit in a continuous
            // wall. If the owned lane is empty, the neighboring face still blocks a seam shot.
            if cell
                .parallel_owner
                .is_some_and(|owner| world.voxel(owner).is_solid())
            {
                continue;
            }
            report.first_hit.get_or_insert(cell.position);
            let length = cell.exit_um - cell.enter_um;
            // A finite projectile cannot tunnel through an exact diagonal seam. A point contact
            // blocks but does not erase a whole cell for an arbitrarily small energy cost.
            if length == 0 {
                report.remaining_energy = 0;
                break;
            }
            let unit_cost = (u64::from(resistance(before.material)) * length)
                .div_ceil(255_000_000)
                .max(1);
            let damage = u8::try_from(
                (u64::from(report.remaining_energy) / unit_cost).min(u64::from(before.integrity)),
            )
            .expect("bounded integrity");
            let work =
                u32::try_from(u64::from(damage) * unit_cost).expect("work cannot exceed energy");
            report.remaining_energy -= work;
            let after = if damage >= before.integrity {
                report.destruction.fractured_voxels += 1;
                report.destruction.released_mass_kg +=
                    u64::from(before.material.properties().density_kg_m3);
                Voxel::AIR
            } else {
                // An obstructed shot deposits its unspent sub-integrity work, not a second hit.
                report.remaining_energy = 0;
                if damage > 0 {
                    report.destruction.damaged_voxels += 1;
                }
                Voxel {
                    integrity: before.integrity - damage,
                    ..before
                }
            };
            if after != before {
                world.set_voxel(cell.position, after);
                report.destruction.changes.push(crate::VoxelChange {
                    position: cell.position,
                    before,
                    after,
                });
            }
            if report.remaining_energy == 0 {
                break;
            }
        }
    }
    if report.blocked_by_body.is_some() {
        report.remaining_energy = 0;
    }
    report.spent_energy = RIFLE_ENERGY - report.remaining_energy;
    report
        .destruction
        .changes
        .sort_unstable_by_key(|change| change.position);
    Ok(report)
}

#[cfg(test)]
mod tests;
