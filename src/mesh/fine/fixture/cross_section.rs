//! A supported two-level factory remnant, authored in the actual queried material geometry.
//! The broken floors are static composition, not the result of a calibrated collapse solver.
use super::{
    Error, GeometryCell, GeometryChange, GeometryState, IVec3, LocalBox, Material, RefinedVolume,
    RefinedWorld, VolumeLimits, Voxel,
};
use crate::world::geometry::MAX_GEOMETRY_CHANGES;

const FLOOR_THICKNESS: u16 = 64;
const STRIP: u16 = 64;
const MAX_AUTHORED_CELLS: usize = 192;
const MAX_AUTHORED_LEAVES: usize = 1_000;
const POST_X: [i32; 2] = [-18, -12];
const BEAM_Z: [i32; 2] = [8, 11];
// Front edges in global 1/256 m Z coordinates. Both profiles leave the complete z=11
// beam supported. The upper floor recedes farther, exposing a legible lower floor terrace.
const LOWER_FRONT: [i32; 10] = [
    3_840, 3_808, 3_712, 3_552, 3_488, 3_648, 3_744, 3_808, 3_840, 3_776,
];
const UPPER_FRONT: [i32; 10] = [
    3_648, 3_520, 3_424, 3_264, 3_136, 3_232, 3_360, 3_456, 3_584, 3_648,
];

fn front(x: i32, local_x: u16, upper: bool) -> Result<i32, Box<dyn Error>> {
    let index = usize::try_from(x + 19)?;
    let profile = if upper { UPPER_FRONT } else { LOWER_FRONT };
    let a = *profile
        .get(index)
        .ok_or("cross-section profile coordinate")?;
    let b = *profile
        .get(index + 1)
        .ok_or("cross-section profile coordinate")?;
    let midpoint = i32::from(local_x + STRIP / 2);
    let chip = [0, 16, 48, 16][usize::from(local_x / STRIP) % 4];
    Ok((a + (b - a) * midpoint / 256 - chip) / 16 * 16)
}

fn concrete(
    volume: &RefinedVolume,
    low: [u16; 3],
    high: [u16; 3],
) -> Result<RefinedVolume, Box<dyn Error>> {
    Ok(volume
        .replace_box(
            LocalBox::new(low, high)?,
            Voxel::new(Material::Concrete),
            VolumeLimits::default(),
        )?
        .0)
}

fn page(position: IVec3) -> Result<GeometryCell, Box<dyn Error>> {
    let mut volume = RefinedVolume::uniform(Voxel::AIR);
    if [5, 9].contains(&position.y) {
        for local_x in (0..256).step_by(usize::from(STRIP)) {
            let depth = u16::try_from(
                (front(position.x, local_x, position.y == 9)? - position.z * 256).clamp(0, 256),
            )?;
            if depth > 0 {
                volume = concrete(
                    &volume,
                    [local_x, 0, 0],
                    [local_x + STRIP, FLOOR_THICKNESS, depth],
                )?;
            }
        }
    }
    if [4, 8].contains(&position.y) && BEAM_Z.contains(&position.z) {
        // A 50 cm-wide, 31.25 cm-deep beam directly touches the slab above and each post.
        volume = concrete(&volume, [0, 176, 64], [256, 256, 192])?;
    }
    if POST_X.contains(&position.x) && BEAM_Z.contains(&position.z) {
        // 37.5 cm square posts continue through the lower floor and terminate at the upper top.
        let top = if position.y == 9 {
            FLOOR_THICKNESS
        } else {
            256
        };
        volume = concrete(&volume, [80, 0, 80], [176, top, 176])?;
    }
    Ok(GeometryCell::refined(volume))
}

pub(super) fn install(source: &RefinedWorld) -> Result<RefinedWorld, Box<dyn Error>> {
    for x in POST_X {
        for z in BEAM_Z {
            if source.cell(IVec3::new(x, 0, z))
                != GeometryCell::uniform(Voxel::new(Material::Concrete))
            {
                return Err("cross-section requires its complete concrete post foundations".into());
            }
        }
    }
    let mut changes = Vec::with_capacity(MAX_AUTHORED_CELLS);
    let mut leaves = 0;
    for x in -19..=-11 {
        for y in 1..=9 {
            for z in 7..=14 {
                let position = IVec3::new(x, y, z);
                let before = source.cell(position);
                // Validate the complete working volume, including the air between floors.
                // Never bury a foreign prop, close an existing hole, or apply this authoring twice.
                if before != GeometryCell::AIR {
                    return Err(
                        format!("cross-section refuses occupied source at {position:?}").into(),
                    );
                }
                let after = page(position)?;
                if after != GeometryCell::AIR {
                    leaves += after.volume().map_or(0, |v| v.leaves().len());
                    if changes.len() == MAX_AUTHORED_CELLS || leaves > MAX_AUTHORED_LEAVES {
                        return Err("cross-section authoring bound".into());
                    }
                    changes.push(GeometryChange {
                        position,
                        before,
                        after,
                    });
                }
            }
        }
    }
    changes.sort_by_key(|change| change.position);
    let mut state = GeometryState::new(source.clone(), 1)?;
    for batch in changes.chunks(MAX_GEOMETRY_CHANGES) {
        let transaction = state.prepare(source.tick(), batch.to_vec())?;
        state.apply(&transaction)?;
    }
    Ok(state.world().clone())
}

#[cfg(test)]
mod tests;
