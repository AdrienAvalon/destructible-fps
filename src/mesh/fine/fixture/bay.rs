//! Authored missing upper facade: exact subtraction, not a simulated structural collapse.
use super::{
    Error, GeometryCell, GeometryChange, GeometryState, IVec3, LocalBox, Material, RefinedVolume,
    RefinedWorld, VolumeLimits, Voxel,
};

const STRIP: u16 = 64;
// Named authoring control points, not a sampled blast/strength model. Broken shoulders and
// staggered ledges replace the previous two straight slopes without touching bearing columns.
const PROFILE: [(i32, i32); 15] = [
    (0, 2_496),
    (192, 2_336),
    (256, 1_712),
    (576, 1_648),
    (640, 1_328),
    (704, 1_280),
    (768, 768),
    (1_280, 768),
    (1_344, 1_152),
    (1_408, 1_152),
    (1_472, 1_504),
    (1_728, 1_440),
    (1_792, 2_016),
    (2_112, 2_144),
    (2_304, 2_448),
];

fn top(u: i32, front: bool) -> Result<u16, Box<dyn Error>> {
    let pair = PROFILE
        .windows(2)
        .find(|p| p[0].0 <= u && u <= p[1].0)
        .ok_or("bay profile coordinate out of range")?;
    let (a, b) = (pair[0], pair[1]);
    let height = a.1 + (b.1 - a.1) * (u - a.0) / (b.0 - a.0);
    let chip = if front {
        [16, 48, 32, 64, 16, 32][usize::try_from(u / i32::from(STRIP))? % 6]
    } else {
        0
    };
    Ok(u16::try_from((height - chip).max(768) / 16 * 16)?)
}

fn clipped(before: &GeometryCell, x: i32, y: i32) -> Result<GeometryCell, Box<dyn Error>> {
    let mut volume = before
        .volume()
        .cloned()
        .unwrap_or_else(|| RefinedVolume::uniform(before.uniform_voxel().unwrap_or(Voxel::AIR)));
    for u in (0..256).step_by(usize::from(STRIP)) {
        for (lo_z, hi_z, front) in [(0, 192, false), (192, 256, true)] {
            let height = i32::from(top((x + 19) * 256 + i32::from(u + STRIP / 2), front)?);
            let retained = u16::try_from((height - y * 256).clamp(0, 256))?;
            if retained < 256 {
                volume = volume
                    .replace_box(
                        LocalBox::new([u, retained, lo_z], [u + STRIP, 256, hi_z])?,
                        Voxel::AIR,
                        VolumeLimits::default(),
                    )?
                    .0;
            }
        }
    }
    Ok(GeometryCell::refined(volume))
}

fn cut_low_notch(
    source: &RefinedWorld,
    stage: usize,
    changes: &mut Vec<GeometryChange>,
) -> Result<(), Box<dyn Error>> {
    // Reuse the exact low-patch authoring, independent of the surrounding coarse world. The
    // caller's stage must match these whole cells; never accept arbitrary pre-damaged material.
    let expected = super::install_wall(
        &crate::World::default(),
        stage,
        IVec3::new(-17, 1, 15),
        true,
    )?;
    for x in [-16, -15] {
        let position = IVec3::new(x, 3, 15);
        let before = source.cell(position);
        if before != expected.cell(position) {
            return Err("bay notch requires its exact low-patch stage".into());
        }
        let after = clipped(&before, x, 3)?;
        if before != after {
            changes.push(GeometryChange {
                position,
                before,
                after,
            });
        }
    }
    Ok(())
}

pub(super) fn install(source: &RefinedWorld, stage: usize) -> Result<RefinedWorld, Box<dyn Error>> {
    // Validate the complete frame, not just the presence of some steel. Remove the entire frame;
    // clipping each bar independently could retain unsupported mullions across a lost sill.
    let frame = super::windows::left_front_cells()?;
    for (position, expected) in &frame {
        if source.cell(*position) != *expected {
            return Err("bay removal requires its complete original frame".into());
        }
    }
    let mut changes = Vec::with_capacity(99);
    cut_low_notch(source, stage, &mut changes)?;
    for x in -19..-10 {
        for y in 4..14 {
            let position = IVec3::new(x, y, 15);
            let before = source.cell(position);
            let after = if frame.iter().any(|(p, _)| *p == position) {
                GeometryCell::AIR
            } else {
                // y13 is the already reprofiled roof-ruin row. Every lower masonry source is
                // full, so unexpected prior cuts or foreign materials cannot be paved over.
                let material = if (y == 6 || y == 12) && (-18..=-13).contains(&x) {
                    Material::Concrete
                } else {
                    Material::Brick
                };
                if y < 13 && before != GeometryCell::uniform(Voxel::new(material)) {
                    return Err(format!(
                        "bay removal refuses unexpected lower masonry at {position:?}"
                    )
                    .into());
                }
                if y == 13
                    && before.volume().is_none_or(|v| {
                        v.leaves().iter().any(|leaf| {
                            ![Material::Air, Material::Brick].contains(&leaf.voxel().material)
                        })
                    })
                {
                    return Err("bay removal requires the authored ragged roof row".into());
                }
                clipped(&before, x, y)?
            };
            if before != after {
                changes.push(GeometryChange {
                    position,
                    before,
                    after,
                });
            }
        }
        // The old projecting beam stubs no longer belong to this ruin silhouette. No steel or
        // concrete remains floating above the removed sill/lintel; bearing columns are outside.
        let position = IVec3::new(x, 13, 16);
        let before = source.cell(position);
        let allowed = before == GeometryCell::AIR
            || before.volume().is_some_and(|v| {
                v.leaves().iter().all(|leaf| {
                    [Material::Air, Material::Concrete].contains(&leaf.voxel().material)
                })
            });
        if !allowed {
            return Err("bay removal refuses unexpected projecting beam".into());
        }
        if before != GeometryCell::AIR {
            changes.push(GeometryChange {
                position,
                before,
                after: GeometryCell::AIR,
            });
        }
    }
    if changes.len() > 99 {
        return Err("bay authoring bound".into());
    }
    changes.sort_by_key(|c| c.position);
    let mut geometry = GeometryState::new(source.clone(), 1)?;
    let transaction = geometry.prepare(source.tick(), changes)?;
    geometry.apply(&transaction)?;
    Ok(geometry.world().clone())
}

/// Only the authored facade-cut region: no low changing patch, roof, column or arbitrary body.
pub(super) fn cut_top_cells(source: &RefinedWorld) -> Vec<IVec3> {
    (-19..-10)
        .flat_map(|x| (4..14).map(move |y| IVec3::new(x, y, 15)))
        .filter(|&p| source.cell(p).volume().is_some())
        .collect()
}

#[cfg(test)]
mod tests;
