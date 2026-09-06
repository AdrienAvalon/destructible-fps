//! Authored worn concrete apron. Its open joints are actual air above retained soil, not decals.
use super::{
    Error, GeometryCell, GeometryChange, GeometryState, IVec3, LocalBox, Material, RefinedVolume,
    RefinedWorld, VolumeLimits, Voxel,
};
use crate::world::geometry::MAX_GEOMETRY_CHANGES;

const SOIL_TOP: u16 = 224;
const MAX_CELLS: usize = 566;

// A buried forecourt edge, authored in metric lattice coordinates. Broad asymmetric tongues
// tie the collapse apron to the old paving; this replaces physical slab material, not its tint.
const RECLAIM_EDGE: [(i32, i32); 8] = [
    (-24 * 256, 24 * 256),
    (-22 * 256, 26 * 256),
    (-19 * 256, 25 * 256),
    (-17 * 256, 27 * 256 + 128),
    (-15 * 256, 26 * 256),
    (-12 * 256, 28 * 256 + 128),
    (-9 * 256, 24 * 256 + 128),
    (-6 * 256, 23 * 256),
];

fn reclaim_edge(x: i32) -> Option<i32> {
    RECLAIM_EDGE.windows(2).find_map(|pair| {
        let [(x0, z0), (x1, z1)] = [pair[0], pair[1]];
        (x0 <= x && x < x1).then(|| z0 + (z1 - z0) * (x - x0) / (x1 - x0))
    })
}

pub(super) fn reclaim(source: &RefinedWorld) -> Result<RefinedWorld, Box<dyn Error>> {
    let mut changes = Vec::new();
    for x in -24..-6 {
        for z in 22..29 {
            let expected = pavement(x, z)?;
            let mut volume = expected.volume().ok_or("missing paved source")?.clone();
            // Eight columns per metre only at the material boundary; canonicalization merges
            // the fully reclaimed interior back into ordinary soil cells.
            for u in (0_u16..256).step_by(32) {
                let edge = reclaim_edge(x * 256 + i32::from(u) + 16).ok_or("reclaim domain")?;
                let end = u16::try_from((edge - z * 256).clamp(0, 256))?;
                if end > 0 {
                    volume = volume
                        .replace_box(
                            LocalBox::new([u, SOIL_TOP, 0], [u + 32, 256, end])?,
                            Voxel::new(Material::Soil),
                            VolumeLimits::default(),
                        )?
                        .0;
                }
            }
            let after = GeometryCell::refined(volume);
            if expected == after {
                continue;
            }
            let position = IVec3::new(x, 0, z);
            if source.cell(position) != expected
                || source.cell(IVec3::new(x, 1, z)) != GeometryCell::AIR
            {
                return Err(
                    format!("reclaimed apron refuses unexpected source at {position:?}").into(),
                );
            }
            changes.push(GeometryChange {
                position,
                before: expected,
                after,
            });
        }
    }
    let mut state = GeometryState::new(source.clone(), 1)?;
    for batch in changes.chunks(MAX_GEOMETRY_CHANGES) {
        let tx = state.prepare(source.tick(), batch.to_vec())?;
        state.apply(&tx)?;
    }
    Ok(state.world().clone())
}

fn footprint(x: i32, z: i32) -> bool {
    ((-24..=12).contains(&x) && (22..=34).contains(&z))
        || ((-8..=8).contains(&x) && (17..=21).contains(&z))
}

fn cut(
    volume: &RefinedVolume,
    low: [u16; 2],
    high: [u16; 2],
) -> Result<RefinedVolume, Box<dyn Error>> {
    Ok(volume
        .replace_box(
            LocalBox::new([low[0], SOIL_TOP, low[1]], [high[0], 256, high[1]])?,
            Voxel::AIR,
            VolumeLimits::default(),
        )?
        .0)
}

fn pavement(x: i32, z: i32) -> Result<GeometryCell, Box<dyn Error>> {
    let mut volume = RefinedVolume::uniform(Voxel::new(Material::Soil))
        .replace_box(
            LocalBox::new([0, SOIL_TOP, 0], [256, 256, 256])?,
            Voxel::new(Material::Concrete),
            VolumeLimits::default(),
        )?
        .0;
    // Four-metre bays, 3.125 cm joints. Integer world anchoring stays continuous across pages.
    let seam_x = x.rem_euclid(4) == 0;
    let seam_z = z.rem_euclid(4) == 0;
    if seam_x {
        volume = cut(&volume, [0, 0], [8, 256])?;
    }
    if seam_z {
        volume = cut(&volume, [0, 0], [256, 8])?;
    }
    if seam_x && seam_z {
        // Missing corners join an existing seam, never an isolated floating fragment.
        let corner =
            u16::try_from(3 + (x.div_euclid(4) * 7 + z.div_euclid(4) * 11).rem_euclid(8))? * 16;
        for row in (0..corner).step_by(16) {
            volume = cut(&volume, [0, row], [corner - row, row + 16])?;
        }
    }
    // A broken leading edge, sampled at 12.5 cm, exposes the substrate without moving its base.
    if z == 34 {
        for u in (0..256).step_by(32) {
            let tooth = (x * 17 + i32::from(u / 32) * 7).rem_euclid(13);
            let retained = u16::try_from(128 + tooth * 8)?;
            volume = cut(&volume, [u, retained], [u + 32, 256])?;
        }
    }
    Ok(GeometryCell::refined(volume))
}

pub(super) fn install(source: &RefinedWorld) -> Result<RefinedWorld, Box<dyn Error>> {
    let mut changes = Vec::with_capacity(MAX_CELLS);
    for x in -24..=12 {
        for z in 17..=34 {
            if !footprint(x, z) {
                continue;
            }
            let position = IVec3::new(x, 0, z);
            let before = source.cell(position);
            // Never pave over existing masonry, damaged ground, or the support of a placed prop.
            if before != GeometryCell::uniform(Voxel::new(Material::Soil))
                || source.cell(IVec3::new(x, 1, z)) != GeometryCell::AIR
            {
                return Err(format!("hardstand refuses unexpected source at {position:?}").into());
            }
            if changes.len() == MAX_CELLS {
                return Err("hardstand authoring bound".into());
            }
            changes.push(GeometryChange {
                position,
                before,
                after: pavement(x, z)?,
            });
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
