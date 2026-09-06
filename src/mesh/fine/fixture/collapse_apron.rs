//! Authored, continuously grounded collapse lobes; not an explosion or rigid-body simulation.
//!
//! The 25 cm aggregate columns and selected 12.5 cm slab columns are the physical source.
//! No high-poly skin, hidden support proxy, floor replacement or floating dressing is added.
use super::{
    Error, GeometryCell, GeometryChange, GeometryState, IVec3, LocalBox, Material, RefinedVolume,
    RefinedWorld, VolumeLimits, Voxel,
};
use std::collections::BTreeMap;

const STEP: i32 = 64;
const MAX_COLUMNS: usize = 1_536;
const MAX_CELLS: usize = 128;
const MAX_LEAVES: usize = 4_500;
// The complete reference scene reserves fifteen of its existing 64 finish entries for the bay.
const MAX_CUT_CELLS: usize = 49;

#[derive(Clone, Copy, Debug)]
struct Column {
    x: i32,
    z: i32,
    step: i32,
    height: i32,
    cap_base: i32,
    material: Material,
}

fn aggregate_height(x: i32, z: i32) -> Option<i32> {
    if !(-22 * 256..-9 * 256).contains(&x)
        || !(16 * 256..22 * 256).contains(&z)
        || (-17 * 256..-14 * 256).contains(&x)
        || (z < 17 * 256 && [-20, -10].contains(&x.div_euclid(256)))
    {
        return None;
    }
    let distance = z - 16 * 256;
    let centre = if x < -17 * 256 {
        -19 * 256 - 128 + distance / 12
    } else {
        -11 * 256 - 128 - distance / 16
    };
    let lateral = x - centre;
    // Taper in both directions, with modest deterministic block-scale variation.
    let tooth = (x.div_euclid(128) * 7 + z.div_euclid(128) * 11).rem_euclid(5) * 8;
    let height =
        420 - lateral * lateral / 1_024 - distance * 60 / 256 - distance * distance / 8_192 + tooth;
    (height >= 32).then_some(height / 16 * 16)
}

fn slab_top(x: i32, z: i32) -> Option<i32> {
    // Three asymmetric, chamfered plates with different physical top-plane directions.
    let plates = [
        (-20 * 256, 17 * 256 + 128, 208, 144, 272, 396, 1, -2),
        (-11 * 256 - 96, 18 * 256 + 64, 224, 160, 288, 300, -2, 1),
        (-18 * 256 - 64, 20 * 256, 176, 144, 232, 176, 1, -1),
    ];
    plates
        .into_iter()
        .find_map(|(cx, cz, hx, hz, bevel, level, sx, sz)| {
            let dx = x - cx;
            let dz = z - cz;
            (dx.abs() < hx && dz.abs() < hz && dx.abs() + dz.abs() < bevel)
                .then_some((level + sx * dx / 3 + sz * dz / 4) / 4 * 4)
        })
}

fn columns() -> Result<Vec<Column>, Box<dyn Error>> {
    let mut result = Vec::new();
    for z in (16 * 256..22 * 256).step_by(64) {
        for x in (-22 * 256..-9 * 256).step_by(64) {
            let Some(base) = aggregate_height(x + STEP / 2, z + STEP / 2) else {
                continue;
            };
            let fine = [16, 48].into_iter().any(|dz| {
                [16, 48]
                    .into_iter()
                    .any(|dx| slab_top(x + dx, z + dz).is_some())
            });
            let step = if fine { STEP / 2 } else { STEP };
            for dz in (0..STEP).step_by(usize::try_from(step)?) {
                for dx in (0..STEP).step_by(usize::try_from(step)?) {
                    let px = x + dx;
                    let pz = z + dz;
                    let top =
                        slab_top(px + step / 2, pz + step / 2).filter(|&top| top >= base + 16);
                    // Low toe material is quarry aggregate, grading into masonry nearer the
                    // facade. The footprint, physical heights and concrete caps stay unchanged.
                    let material = if base < 128 {
                        Material::Stone
                    } else {
                        match (x.div_euclid(128) * 3 + z.div_euclid(128) * 7).rem_euclid(7) {
                            0 | 1 => Material::Stone,
                            2 => Material::Concrete,
                            _ => Material::Brick,
                        }
                    };
                    if result.len() == MAX_COLUMNS {
                        return Err("collapse apron column bound".into());
                    }
                    result.push(Column {
                        x: px,
                        z: pz,
                        step,
                        height: top.unwrap_or(base),
                        cap_base: top.map_or(base, |top| base.min(top - 48)),
                        material,
                    });
                }
            }
        }
    }
    Ok(result)
}

fn add_span(
    pages: &mut BTreeMap<IVec3, RefinedVolume>,
    column: Column,
    low: i32,
    high: i32,
    material: Material,
) -> Result<(), Box<dyn Error>> {
    if low < 0 || high <= low || high > 512 || ![32, 64].contains(&column.step) {
        return Err("collapse apron span bound".into());
    }
    let local_x = u16::try_from(column.x.rem_euclid(256))?;
    let local_z = u16::try_from(column.z.rem_euclid(256))?;
    let step = u16::try_from(column.step)?;
    for layer in low.div_euclid(256)..=(high - 1).div_euclid(256) {
        let position = IVec3::new(
            column.x.div_euclid(256),
            1 + layer,
            column.z.div_euclid(256),
        );
        if !pages.contains_key(&position) && pages.len() == MAX_CELLS {
            return Err("collapse apron cell bound".into());
        }
        let page = pages
            .entry(position)
            .or_insert_with(|| RefinedVolume::uniform(Voxel::AIR));
        *page = page
            .replace_box(
                LocalBox::new(
                    [local_x, u16::try_from((low - layer * 256).max(0))?, local_z],
                    [
                        local_x + step,
                        u16::try_from((high - layer * 256).min(256))?,
                        local_z + step,
                    ],
                )?,
                Voxel::new(material),
                VolumeLimits::default(),
            )?
            .0;
    }
    Ok(())
}

fn prepare_pages(columns: &[Column]) -> Result<BTreeMap<IVec3, RefinedVolume>, Box<dyn Error>> {
    if columns.len() > MAX_COLUMNS {
        return Err("collapse apron column bound".into());
    }
    let mut pages = BTreeMap::new();
    for &column in columns {
        add_span(&mut pages, column, 0, column.cap_base, column.material)?;
        if column.height > column.cap_base {
            add_span(
                &mut pages,
                column,
                column.cap_base,
                column.height,
                Material::Concrete,
            )?;
        }
    }
    if pages.values().map(|p| p.leaves().len()).sum::<usize>() > MAX_LEAVES {
        return Err("collapse apron aggregate leaf bound".into());
    }
    Ok(pages)
}

/// Builds both lobes in an unpublished candidate and refuses conflicting source geometry.
pub(super) fn install(source: &RefinedWorld) -> Result<RefinedWorld, Box<dyn Error>> {
    let columns = columns()?;
    let pages = prepare_pages(&columns)?;
    // The authored footprint ends before the paved joints. Every column has a complete ground
    // face; upper pages have continuous material beneath them, even where the material changes.
    for position in pages.keys().filter(|p| p.y == 1) {
        let support = IVec3::new(position.x, 0, position.z);
        if source.cell(support).solid_units() != crate::volume::VOLUME_UNITS {
            return Err(
                format!("collapse apron requires complete ground support at {support:?}").into(),
            );
        }
    }
    let mut changes = Vec::with_capacity(pages.len());
    for (position, page) in pages {
        let before = source.cell(position);
        if before != GeometryCell::AIR {
            return Err(format!("collapse apron refuses occupied source at {position:?}").into());
        }
        changes.push(GeometryChange {
            position,
            before,
            after: GeometryCell::refined(page),
        });
    }
    let mut state = GeometryState::new(source.clone(), 1)?;
    let transaction = state.prepare(source.tick(), changes)?;
    state.apply(&transaction)?;
    Ok(state.world().clone())
}

/// Broken-masonry appearance covers every refined masonry page, leaving the stone-only toe alone.
/// The complete expected apron is checked, including stone pages, before returning any selection.
pub(super) fn cut_cells(source: &RefinedWorld) -> Result<Vec<IVec3>, Box<dyn Error>> {
    let expected = prepare_pages(&columns()?)?;
    let mut cells = Vec::new();
    for (position, page) in expected {
        let masonry = page
            .leaves()
            .iter()
            .any(|leaf| matches!(leaf.voxel().material, Material::Brick | Material::Concrete));
        let cell = GeometryCell::refined(page);
        if source.cell(position) != cell {
            return Err(format!("collapse apron cut-cell source changed at {position:?}").into());
        }
        if masonry {
            if cell.volume().is_none() {
                return Err("collapse apron masonry finish requires refined geometry".into());
            }
            if cells.len() == MAX_CUT_CELLS {
                return Err("collapse apron cut-cell bound".into());
            }
            cells.push(position);
        }
    }
    Ok(cells)
}

#[cfg(test)]
mod tests;
