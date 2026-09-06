//! Reproducible material/geometry inspection stages, NOT a weapon or structural simulation.
use crate::{
    IVec3, Material, Voxel, World,
    volume::{LocalBox, RefinedVolume, VolumeLimits},
    world::geometry::{GeometryCell, GeometryChange, GeometryState, RefinedWorld},
};
use std::error::Error;

mod bay;
mod hardstand;
mod roof;
mod ruins;
#[cfg(test)]
mod tests;
mod windows;

pub const STAGE_NAMES: [&str; 4] = ["intact", "shallow chip", "through bore", "breach"];

/// Explicit authored finishes for industrial cut surfaces, separate from physical world state.
/// # Errors
/// Refuses missing, unsupported or oversized source pages.
pub fn industrial_surface_finishes(
    world: &RefinedWorld,
) -> Result<super::finishes::SurfaceFinishes, super::FineMeshError> {
    ruins::surface_finishes(world)
}

/// The authored patch is the only changing source region in the industrial inspection.
#[must_use]
pub fn industrial_patch_positions() -> Vec<IVec3> {
    (0..4)
        .flat_map(|x| (0..3).map(move |y| IVec3::new(-17 + x, 1 + y, 15)))
        .collect()
}

/// Adds real thin masonry to the complete existing industrial material world.
/// # Errors
/// Refuses unknown stages or bounded geometry preparation failures.
pub fn industrial_inspection_world(stage: usize) -> Result<RefinedWorld, Box<dyn Error>> {
    let world = install_wall(
        &crate::WorldPreset::Industrial.build(),
        stage,
        IVec3::new(-17, 1, 15),
        true,
    )?;
    let world = ruins::courtyard(&world)?;
    let world = windows::install(&world)?;
    let world = roof::install(&world)?;
    let world = hardstand::install(&world)?;
    let world = bay::install(&world, stage)?;
    ruins::bay_debris(&world)
}

/// Builds thin layered masonry with exact air cuts and a surrounding concrete inspection pad.
/// # Errors
/// Propagates bounded geometry edit/transaction refusals. Only the four authored stages exist.
pub fn inspection_world(stage: usize) -> Result<RefinedWorld, Box<dyn Error>> {
    if stage >= STAGE_NAMES.len() {
        return Err("unknown fine inspection stage".into());
    }
    let mut coarse = World::default();
    coarse.fill_box(
        IVec3::new(-2, -1, -3),
        IVec3::new(5, -1, 4),
        Voxel::new(Material::Concrete),
    );
    coarse.fill_box(
        IVec3::new(-1, 0, 0),
        IVec3::new(-1, 3, 0),
        Voxel::new(Material::Concrete),
    );
    coarse.fill_box(
        IVec3::new(4, 0, 0),
        IVec3::new(4, 3, 0),
        Voxel::new(Material::Concrete),
    );
    coarse.fill_box(
        IVec3::new(0, 3, 0),
        IVec3::new(3, 3, 0),
        Voxel::new(Material::Concrete),
    );
    install_wall(&coarse, stage, IVec3::default(), false)
}

fn install_wall(
    coarse: &World,
    stage: usize,
    origin: IVec3,
    positive: bool,
) -> Result<RefinedWorld, Box<dyn Error>> {
    if stage >= STAGE_NAMES.len() {
        return Err("unknown fine inspection stage".into());
    }
    let mut geometry = GeometryState::new(RefinedWorld::from_uniform(coarse)?, 1)?;
    let mut changes = Vec::new();
    for x in 0..4 {
        for y in 0..3 {
            let mut wall = RefinedVolume::uniform(Voxel::AIR)
                .replace_box(
                    LocalBox::new(
                        [0, 0, if positive { 176 } else { 0 }],
                        [256, 256, if positive { 256 } else { 80 }],
                    )?,
                    Voxel::new(Material::Brick),
                    VolumeLimits::default(),
                )?
                .0
                .replace_box(
                    LocalBox::new(
                        [0, 0, if positive { 176 } else { 48 }],
                        [256, 256, if positive { 208 } else { 80 }],
                    )?,
                    Voxel::new(Material::Concrete),
                    VolumeLimits::default(),
                )?
                .0;
            if stage != 0 {
                let radius: i32 = match stage {
                    1 => 110,
                    2 => 75,
                    _ => 290,
                };
                let depth = if stage == 1 { 24 } else { 80 };
                // A deterministic, fine stepped aperture; the grid is 1/256 m and the authored
                // contour samples every 4 units. It is intentionally not called calibrated blast.
                for row in (0_u16..256).step_by(4) {
                    let dy = y * 256 + i32::from(row) + 2 - 350;
                    let (left, right) = if positive && stage == 3 {
                        ruins::breach_row(y * 256 + i32::from(row))
                    } else {
                        let remainder = radius * radius - dy * dy;
                        if remainder <= 0 {
                            continue;
                        }
                        let half = i32::try_from(u32::try_from(remainder)?.isqrt())?;
                        (512 - half, 512 + half)
                    };
                    let left = (left - x * 256).clamp(0, 256);
                    let right = (right - x * 256).clamp(0, 256);
                    if left >= right {
                        continue;
                    }
                    wall = wall
                        .replace_box(
                            LocalBox::new(
                                [
                                    u16::try_from(left)?,
                                    row,
                                    if positive { 256 - depth } else { 0 },
                                ],
                                [
                                    u16::try_from(right)?,
                                    row + 4,
                                    if positive { 256 } else { depth },
                                ],
                            )?,
                            Voxel::AIR,
                            VolumeLimits::default(),
                        )?
                        .0;
                }
            }
            let position = IVec3::new(origin.x + x, origin.y + y, origin.z);
            changes.push(GeometryChange {
                position,
                before: geometry.world().cell(position),
                after: GeometryCell::refined(wall),
            });
        }
    }
    changes.sort_by_key(|c| c.position);
    let tx = geometry.prepare(1, changes)?;
    geometry.apply(&tx)?;
    Ok(geometry.world().clone())
}
