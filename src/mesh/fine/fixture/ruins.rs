//! Authored masonry silhouette and grounded rubble, not explosion-generated rigid bodies.
use super::{
    Error, GeometryCell, GeometryChange, GeometryState, IVec3, LocalBox, Material, RefinedVolume,
    RefinedWorld, VolumeLimits, Voxel,
};

/// Half-open aperture in the four-by-three-metre wall patch, in 1/256 m units.
/// The intact strips at both sides and the upper band remain connected to the surrounding wall.
pub(super) fn breach_row(y: i32) -> (i32, i32) {
    if !(0..704).contains(&y) {
        return (0, 0);
    }
    let course = y / 32;
    let tooth = [0, 13, -7, 21, 4, -11, 9][usize::try_from(course % 7).unwrap_or(0)];
    let taper = (y - 288).max(0);
    let left = 120 + taper / 2 + tooth;
    let right = 830 - taper / 3 + tooth / 2;
    (left, right)
}

#[derive(Clone, Copy)]
struct Shard {
    cell: IVec3,
    extent: [u16; 2],
    thickness: i32,
    slope: [i32; 2],
    material: Material,
}

const SHARDS: [Shard; 8] = [
    Shard {
        cell: IVec3::new(-17, 1, 16),
        extent: [192, 160],
        thickness: 64,
        slope: [72, -24],
        material: Material::Concrete,
    },
    Shard {
        cell: IVec3::new(-15, 1, 16),
        extent: [128, 96],
        thickness: 36,
        slope: [-16, 44],
        material: Material::Brick,
    },
    Shard {
        cell: IVec3::new(-14, 1, 17),
        extent: [96, 160],
        thickness: 48,
        slope: [24, -28],
        material: Material::Brick,
    },
    Shard {
        cell: IVec3::new(-16, 1, 17),
        extent: [160, 128],
        thickness: 80,
        slope: [-48, 20],
        material: Material::Concrete,
    },
    Shard {
        cell: IVec3::new(-18, 1, 18),
        extent: [96, 80],
        thickness: 28,
        slope: [32, 16],
        material: Material::Brick,
    },
    Shard {
        cell: IVec3::new(-15, 1, 19),
        extent: [128, 192],
        thickness: 48,
        slope: [44, -20],
        material: Material::Concrete,
    },
    Shard {
        cell: IVec3::new(-13, 1, 18),
        extent: [80, 64],
        thickness: 24,
        slope: [12, 16],
        material: Material::Brick,
    },
    Shard {
        cell: IVec3::new(-17, 1, 20),
        extent: [64, 96],
        thickness: 32,
        slope: [-12, 24],
        material: Material::Brick,
    },
];

fn column_height(shard: Shard, x: u16, z: u16) -> Option<u16> {
    let [width, depth] = shard.extent;
    let bevel = width.min(depth) / 4;
    if x + z < bevel
        || width - x + z < bevel
        || x + depth - z < bevel
        || width - x + depth - z < bevel
    {
        return None;
    }
    let height = shard.thickness
        + shard.slope[0] * i32::from(x) / i32::from(width)
        + shard.slope[1] * i32::from(z) / i32::from(depth);
    // Explicit authored quantization; these are the actual queried/stored surfaces, not proxies.
    u16::try_from(height.clamp(16, 200) / 4 * 4).ok()
}

fn shard_volume(shard: Shard) -> Result<RefinedVolume, Box<dyn Error>> {
    sampled_shard(shard, 8)
}

fn sampled_shard(shard: Shard, step: u16) -> Result<RefinedVolume, Box<dyn Error>> {
    let mut volume = RefinedVolume::uniform(Voxel::AIR);
    for z in (0..shard.extent[1]).step_by(usize::from(step)) {
        for x in (0..shard.extent[0]).step_by(usize::from(step)) {
            if let Some(height) = column_height(shard, x + step / 2, z + step / 2) {
                volume = volume
                    .replace_box(
                        LocalBox::new([16 + x, 0, 16 + z], [16 + x + step, height, 16 + z + step])?,
                        Voxel::new(shard.material),
                        VolumeLimits::default(),
                    )?
                    .0;
            }
        }
    }
    Ok(volume)
}

pub(super) fn courtyard(source: &RefinedWorld) -> Result<RefinedWorld, Box<dyn Error>> {
    place(source, &SHARDS, 8)
}

fn bay_shards() -> Vec<Shard> {
    [
        (-19, 16),
        (-18, 16),
        (-16, 16),
        (-12, 16),
        (-11, 17),
        (-18, 17),
        (-17, 18),
        (-16, 18),
        (-14, 18),
        (-12, 19),
        (-14, 20),
        (-18, 21),
    ]
    .into_iter()
    .enumerate()
    .map(|(i, (x, z))| Shard {
        cell: IVec3::new(x, 1, z),
        extent: [[224, 208], [208, 192], [192, 224]][i % 3],
        thickness: [144, 96, 128, 64][i % 4],
        slope: [[32, -16], [-24, 16], [16, 24]][i % 3],
        material: if i % 3 == 0 {
            Material::Concrete
        } else {
            Material::Brick
        },
    })
    .collect()
}

/// Static dressed debris accompanying the missing facade, not a mass-conserved blast result.
pub(super) fn bay_debris(source: &RefinedWorld) -> Result<RefinedWorld, Box<dyn Error>> {
    place(source, &bay_shards(), 16)
}

pub(super) fn surface_finishes(
    source: &RefinedWorld,
) -> Result<super::super::finishes::SurfaceFinishes, super::super::FineMeshError> {
    let mut positions: Vec<_> = SHARDS
        .iter()
        .chain(bay_shards().iter())
        .map(|s| s.cell)
        .collect();
    positions.extend(super::bay::cut_top_cells(source));
    positions.sort_unstable();
    super::super::finishes::SurfaceFinishes::cut_tops(source, &positions)
}

fn place(
    source: &RefinedWorld,
    shards: &[Shard],
    step: u16,
) -> Result<RefinedWorld, Box<dyn Error>> {
    let mut state = GeometryState::new(source.clone(), 1)?;
    let mut changes = Vec::with_capacity(shards.len());
    for &shard in shards {
        if source.cell(shard.cell).solid_units() != 0
            || source
                .cell(IVec3::new(shard.cell.x, 0, shard.cell.z))
                .solid_units()
                != crate::volume::VOLUME_UNITS
        {
            return Err(
                "authored rubble needs empty space and a full supporting ground cell".into(),
            );
        }
        changes.push(GeometryChange {
            position: shard.cell,
            before: source.cell(shard.cell),
            after: GeometryCell::refined(if step == 8 {
                shard_volume(shard)?
            } else {
                sampled_shard(shard, step)?
            }),
        });
    }
    changes.sort_by_key(|c| c.position);
    let transaction = state.prepare(1, changes)?;
    state.apply(&transaction)?;
    Ok(state.world().clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aperture_is_ground_reaching_asymmetric_and_keeps_connected_side_and_top_bands() {
        let mut previous = None;
        let mut varies = false;
        let mut asymmetric = false;
        for y in (0..768).step_by(4) {
            let (left, right) = breach_row(y);
            if y >= 704 {
                assert_eq!((left, right), (0, 0));
            } else {
                assert!(left >= 100 && right <= 850 && left < 512 && right > 512);
                asymmetric |= left + right != 1024;
                varies |= previous.is_some_and(|p| p != (left, right));
                previous = Some((left, right));
            }
        }
        assert!(varies);
        assert!(asymmetric, "not a mirrored circular aperture");
        assert_eq!(breach_row(-1), (0, 0));
    }

    #[test]
    fn rubble_has_exact_ground_contact_empty_corners_and_tilted_quantized_tops() {
        let shards: Vec<_> = SHARDS
            .into_iter()
            .map(|s| (s, 8_u16))
            .chain(bay_shards().into_iter().map(|s| (s, 16)))
            .collect();
        let cells: std::collections::BTreeSet<_> = shards.iter().map(|(s, _)| s.cell).collect();
        assert_eq!(cells.len(), shards.len(), "no overlapping shard pages");
        for (shard, step) in shards {
            let volume = sampled_shard(shard, step).unwrap();
            let mut heights = std::collections::BTreeSet::new();
            let mut volume_units = 0;
            for z in (0..shard.extent[1]).step_by(usize::from(step)) {
                for x in (0..shard.extent[0]).step_by(usize::from(step)) {
                    let expected = column_height(shard, x + step / 2, z + step / 2);
                    let centre_x = 16 + x + step / 2;
                    let centre_z = 16 + z + step / 2;
                    let sample = |y| {
                        LocalBox::new([centre_x, y, centre_z], [centre_x + 1, y + 1, centre_z + 1])
                            .unwrap()
                    };
                    assert_eq!(volume.overlaps_solid(sample(0)), expected.is_some());
                    if let Some(height) = expected {
                        assert!(volume.overlaps_solid(sample(height - 1)));
                        assert!(!volume.overlaps_solid(sample(height)));
                        heights.insert(height);
                        volume_units += u32::from(height) * u32::from(step).pow(2);
                    }
                }
            }
            assert!(heights.len() >= 4, "not another axis-aligned full cube");
            assert_eq!(volume.solid_units(), volume_units);
            assert_eq!(
                volume.fingerprint(),
                sampled_shard(shard, step).unwrap().fingerprint()
            );
        }
    }

    #[test]
    fn courtyard_refuses_missing_support_and_occupied_authored_cells_without_mutating_source() {
        let empty = RefinedWorld::default();
        assert!(courtyard(&empty).is_err());
        assert!(bay_debris(&empty).is_err());
        assert_eq!(empty.geometry_stats().chunks, 0);
        let mut coarse = crate::WorldPreset::Industrial.build();
        let s = SHARDS[0];
        coarse.fill_box(s.cell, s.cell, Voxel::new(Material::Concrete));
        let occupied = RefinedWorld::from_uniform(&coarse).unwrap();
        let before = occupied.fingerprint();
        assert!(courtyard(&occupied).is_err());
        assert_eq!(occupied.fingerprint(), before);
        let coarse = crate::WorldPreset::Industrial.build();
        let original = RefinedWorld::from_uniform(&coarse).unwrap();
        let dressed = bay_debris(&original).unwrap();
        let before = dressed.fingerprint();
        assert!(bay_debris(&dressed).is_err());
        assert_eq!(dressed.fingerprint(), before);
    }
}
