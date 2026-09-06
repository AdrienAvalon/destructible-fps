//! Authored masonry silhouette and grounded rubble, not explosion-generated rigid bodies.
use super::*;

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
    let mut volume = RefinedVolume::uniform(Voxel::AIR);
    for z in (0..shard.extent[1]).step_by(8) {
        for x in (0..shard.extent[0]).step_by(8) {
            if let Some(height) = column_height(shard, x + 4, z + 4) {
                volume = volume
                    .replace_box(
                        LocalBox::new([16 + x, 0, 16 + z], [24 + x, height, 24 + z])?,
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
    let mut state = GeometryState::new(source.clone(), 1)?;
    let mut changes = Vec::with_capacity(SHARDS.len());
    for shard in SHARDS {
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
            after: GeometryCell::refined(shard_volume(shard)?),
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
        let cells: std::collections::BTreeSet<_> = SHARDS.iter().map(|s| s.cell).collect();
        assert_eq!(cells.len(), SHARDS.len(), "no overlapping shard pages");
        for shard in SHARDS {
            let volume = shard_volume(shard).unwrap();
            let mut heights = std::collections::BTreeSet::new();
            let mut volume_units = 0;
            for z in (0..shard.extent[1]).step_by(8) {
                for x in (0..shard.extent[0]).step_by(8) {
                    let expected = column_height(shard, x + 4, z + 4);
                    let sample =
                        |y| LocalBox::new([20 + x, y, 20 + z], [21 + x, y + 1, 21 + z]).unwrap();
                    assert_eq!(volume.overlaps_solid(sample(0)), expected.is_some());
                    if let Some(height) = expected {
                        assert!(volume.overlaps_solid(sample(height - 1)));
                        assert!(!volume.overlaps_solid(sample(height)));
                        heights.insert(height);
                        volume_units += u32::from(height) * 64;
                    }
                }
            }
            assert!(heights.len() >= 4, "not another axis-aligned full cube");
            assert_eq!(volume.solid_units(), volume_units);
            assert_eq!(
                volume.fingerprint(),
                shard_volume(shard).unwrap().fingerprint()
            );
        }
    }

    #[test]
    fn courtyard_refuses_missing_support_and_occupied_authored_cells_without_mutating_source() {
        let empty = RefinedWorld::default();
        assert!(courtyard(&empty).is_err());
        assert_eq!(empty.geometry_stats().chunks, 0);
        let mut coarse = crate::WorldPreset::Industrial.build();
        let s = SHARDS[0];
        coarse.fill_box(s.cell, s.cell, Voxel::new(Material::Concrete));
        let occupied = RefinedWorld::from_uniform(&coarse).unwrap();
        let before = occupied.fingerprint();
        assert!(courtyard(&occupied).is_err());
        assert_eq!(occupied.fingerprint(), before);
    }
}
