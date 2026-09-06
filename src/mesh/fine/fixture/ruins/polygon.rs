//! Integer authored footprints. The raster columns, not an ideal polygon proxy, are physical.
use super::{Error, LocalBox, RefinedVolume, Shard, VolumeLimits, Voxel};

const STEP: u16 = 16;
const OUTLINES: [&[[i32; 2]]; 3] = [
    &[
        [32, 16],
        [184, 32],
        [224, 96],
        [176, 208],
        [48, 208],
        [16, 128],
    ],
    &[[48, 32], [224, 112], [176, 208], [80, 208], [16, 144]],
    &[[16, 64], [144, 16], [224, 96], [144, 208], [32, 208]],
];

const fn rotate([x, z]: [i32; 2], turns: usize) -> [i32; 2] {
    match turns % 4 {
        0 => [x, z],
        1 => [256 - z, x],
        2 => [256 - x, 256 - z],
        _ => [z, 256 - x],
    }
}

const fn cross(a: [i32; 2], b: [i32; 2], p: [i32; 2]) -> i32 {
    (b[0] - a[0]) * (p[1] - a[1]) - (b[1] - a[1]) * (p[0] - a[0])
}

fn height(shard: Shard, index: usize, point: [i32; 2]) -> Result<Option<u16>, Box<dyn Error>> {
    let outline = OUTLINES
        .get(index / 4)
        .ok_or("polygon rubble index out of range")?;
    let p = rotate(point, (4 - index % 4) % 4);
    if (0..outline.len()).any(|i| cross(outline[i], outline[(i + 1) % outline.len()], p) < 0) {
        return Ok(None);
    }
    let h =
        shard.thickness + shard.slope[0] * (p[0] - 16) / 224 + shard.slope[1] * (p[1] - 16) / 224;
    Ok(Some(u16::try_from(h.clamp(16, 200) / 4 * 4)?))
}

pub(super) fn volume(shard: Shard, index: usize) -> Result<RefinedVolume, Box<dyn Error>> {
    if index >= 12 {
        return Err("polygon rubble index out of range".into());
    }
    let mut result = RefinedVolume::uniform(Voxel::AIR);
    for z in (16..240).step_by(usize::from(STEP)) {
        for x in (16..240).step_by(usize::from(STEP)) {
            if let Some(h) = height(
                shard,
                index,
                [i32::from(x + STEP / 2), i32::from(z + STEP / 2)],
            )? {
                result = result
                    .replace_box(
                        LocalBox::new([x, 0, z], [x + STEP, h, z + STEP])?,
                        Voxel::new(shard.material),
                        VolumeLimits::default(),
                    )?
                    .0;
            }
        }
    }
    Ok(result)
}

pub(super) const fn finish(index: usize) -> crate::mesh::fine::finishes::FinishPolicy {
    use crate::{mesh::fine::finishes::FinishPolicy, volume::surface::Face};
    // All templates retain their flat z=208 edge before the exact quarter-turn.
    let (face, plane) = match index % 4 {
        0 => (Face::PositiveZ, 208),
        1 => (Face::NegativeX, 48),
        2 => (Face::NegativeZ, 48),
        _ => (Face::PositiveX, 208),
    };
    FinishPolicy::CutTopAndSidesAtPlane(face, plane)
}

#[cfg(test)]
mod tests;
