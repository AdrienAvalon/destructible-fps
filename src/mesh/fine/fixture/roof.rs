//! Authored roof ruin in exact source geometry; not a load solver or mass-conserved blast result.
use super::{
    Error, GeometryCell, GeometryChange, GeometryState, IVec3, LocalBox, Material, RefinedVolume,
    RefinedWorld, VolumeLimits, Voxel,
};
use crate::world::geometry::MAX_GEOMETRY_CHANGES;

const SLAB_THICKNESS: u16 = 64;
const STRIP: u16 = 16;
const MAX_AUTHORED_CHANGES: usize = 2_048;

/// All surviving strips of the notch connect to the rear sheet; no closed cut or isolated island.
fn rear_edge(u: i32) -> i32 {
    let tooth = [0, 3, -2, 5, 1, -3, 2][usize::try_from((u / 64) % 7).unwrap_or(0)];
    (7 * 256 + (u - 980).abs() / 3 + tooth * 16) / 16 * 16
}

fn beam_height(u: i32) -> u16 {
    let distance = u.min(9 * 256 - u);
    let tooth = [0, 6, -3, 4, -5, 2, 1][usize::try_from((u / 48) % 7).unwrap_or(0)];
    u16::try_from((192 - distance / 10 + tooth * 4).clamp(48, 224) / 8 * 8).unwrap_or(48)
}

const fn clerestory_support(x: i32, z: i32) -> bool {
    ((x == -7 || x == 7) && z >= -12 && z <= 12) || ((z == -12 || z == 12) && x >= -7 && x <= 7)
}

fn slab(x: i32, z: i32, main: bool) -> Result<GeometryCell, Box<dyn Error>> {
    let mut volume = RefinedVolume::uniform(Voxel::AIR)
        .replace_box(
            LocalBox::new([0, 0, 0], [256, SLAB_THICKNESS, 256])?,
            Voxel::new(Material::Concrete),
            VolumeLimits::default(),
        )?
        .0;
    if main && (-19..-10).contains(&x) {
        for local_x in (0..256).step_by(usize::from(STRIP)) {
            let edge = rear_edge((x + 19) * 256 + i32::from(local_x) + i32::from(STRIP / 2));
            let remaining = u16::try_from((edge - z * 256).clamp(0, 256))?;
            if remaining < 256 {
                volume = volume
                    .replace_box(
                        LocalBox::new(
                            [local_x, 0, remaining],
                            [local_x + STRIP, SLAB_THICKNESS, 256],
                        )?,
                        Voxel::AIR,
                        VolumeLimits::default(),
                    )?
                    .0;
            }
        }
    }
    Ok(GeometryCell::refined(volume))
}

fn broken_beam(x: i32, material: Material) -> Result<GeometryCell, Box<dyn Error>> {
    let mut volume = RefinedVolume::uniform(Voxel::AIR);
    for local_x in (0..256).step_by(usize::from(STRIP)) {
        let u = (x + 19) * 256 + i32::from(local_x) + i32::from(STRIP / 2);
        let height = if material == Material::Concrete {
            // The projecting z16 beam has AIR below, unlike the z15 masonry row. Its centre
            // is missing; retain only short stubs in face contact with the side columns.
            let distance = u.min(9 * 256 - u);
            if distance >= 128 {
                continue;
            }
            u16::try_from(160 - distance)?
        } else {
            beam_height(u)
        };
        volume = volume
            .replace_box(
                LocalBox::new([local_x, 0, 0], [local_x + STRIP, height, 256])?,
                Voxel::new(material),
                VolumeLimits::default(),
            )?
            .0;
    }
    Ok(GeometryCell::refined(volume))
}

fn push_change(
    source: &RefinedWorld,
    changes: &mut Vec<GeometryChange>,
    position: IVec3,
    expected: Material,
    after: GeometryCell,
) -> Result<(), Box<dyn Error>> {
    let before = source.cell(position);
    if before != GeometryCell::uniform(Voxel::new(expected)) {
        return Err(format!("roof authoring refuses unexpected source at {position:?}").into());
    }
    if before != after {
        if changes.len() == MAX_AUTHORED_CHANGES {
            return Err("roof authoring bound".into());
        }
        changes.push(GeometryChange {
            position,
            before,
            after,
        });
    }
    Ok(())
}

pub(super) fn install(source: &RefinedWorld) -> Result<RefinedWorld, Box<dyn Error>> {
    let mut changes = Vec::with_capacity(MAX_AUTHORED_CHANGES);
    // Main sheet and raised cap are 25 cm thick. The raised walls start at y15: retain their
    // complete support strips in the y14 sheet, otherwise thinning would make them float.
    for (y, extent_x, extent_z) in [(14, 20, 16), (18, 8, 13)] {
        for x in -extent_x..=extent_x {
            for z in -extent_z..=extent_z {
                let hole = y == 14 && (-6..=6).contains(&x) && (-11..=11).contains(&z);
                let retained_support = y == 14 && clerestory_support(x, z);
                let after = if hole {
                    GeometryCell::AIR
                } else if retained_support {
                    GeometryCell::uniform(Voxel::new(Material::Concrete))
                } else {
                    slab(x, z, y == 14)?
                };
                push_change(
                    source,
                    &mut changes,
                    IVec3::new(x, y, z),
                    if hole {
                        Material::Air
                    } else {
                        Material::Concrete
                    },
                    after,
                )?;
            }
        }
    }
    // Keep the whole y12 lintel and all y7..11 steel frames. The irregular remnants rest on
    // that continuous lintel. Projecting beam stubs attach laterally to the side columns;
    // the columns below the reprofiled roof sheet remain uncut.
    for x in -19..-10 {
        for (z, material) in [(15, Material::Brick), (16, Material::Concrete)] {
            push_change(
                source,
                &mut changes,
                IVec3::new(x, 13, z),
                material,
                broken_beam(x, material)?,
            )?;
        }
    }
    changes.sort_by_key(|c| c.position);
    let mut state = GeometryState::new(source.clone(), 1)?;
    for batch in changes.chunks(MAX_GEOMETRY_CHANGES) {
        let tx = state.prepare(source.tick(), batch.to_vec())?;
        state.apply(&tx)?;
    }
    Ok(state.world().clone())
}

#[cfg(test)]
mod tests;
