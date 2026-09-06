//! Authored missing upper facade: exact subtraction, not a simulated structural collapse.
use super::{
    Error, GeometryCell, GeometryChange, GeometryState, IVec3, LocalBox, Material, RefinedVolume,
    RefinedWorld, VolumeLimits, Voxel,
};

const STRIP: u16 = 32;

fn top(u: i32) -> u16 {
    let slope = if u < 800 {
        2_600 - u * 7 / 4
    } else {
        1_200 + (u - 800) * 3 / 4
    };
    let tooth = [-16, 32, -32, 16, 48, -16, 0][usize::try_from((u / 64) % 7).unwrap_or(0)];
    u16::try_from((slope + tooth).clamp(1_152, 2_688) / 16 * 16).unwrap_or(1_152)
}

fn clipped(before: &GeometryCell, x: i32, y: i32) -> Result<GeometryCell, Box<dyn Error>> {
    let mut volume = before
        .volume()
        .cloned()
        .unwrap_or_else(|| RefinedVolume::uniform(before.uniform_voxel().unwrap_or(Voxel::AIR)));
    for u in (0..256).step_by(usize::from(STRIP)) {
        let height = i32::from(top((x + 19) * 256 + i32::from(u + STRIP / 2)));
        let retained = u16::try_from((height - y * 256).clamp(0, 256))?;
        if retained < 256 {
            volume = volume
                .replace_box(
                    LocalBox::new([u, retained, 0], [u + STRIP, 256, 256])?,
                    Voxel::AIR,
                    VolumeLimits::default(),
                )?
                .0;
        }
    }
    Ok(GeometryCell::refined(volume))
}

pub(super) fn install(source: &RefinedWorld) -> Result<RefinedWorld, Box<dyn Error>> {
    // Validate the complete frame, not just the presence of some steel. Remove the entire frame;
    // clipping each bar independently could retain unsupported mullions across a lost sill.
    let frame = super::windows::left_front_cells()?;
    for (position, expected) in &frame {
        if source.cell(*position) != *expected {
            return Err("bay removal requires its complete original frame".into());
        }
    }
    let mut changes = Vec::with_capacity(99);
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
    let mut state = GeometryState::new(source.clone(), 1)?;
    let transaction = state.prepare(source.tick(), changes)?;
    state.apply(&transaction)?;
    Ok(state.world().clone())
}

#[cfg(test)]
mod tests;
