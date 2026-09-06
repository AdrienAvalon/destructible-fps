//! Fixed adversarial geometry sequences. Also compiles against the retained v1 library for comparison.
use destructible_fps::{
    Material, Voxel,
    volume::{
        LocalBox, RefinedVolume, VOLUME_UNITS, VolumeError, VolumeLimits, surface::SurfaceLimits,
    },
};
use std::{error::Error, time::Instant};

const PERMUTATIONS: [[usize; 3]; 6] = [
    [0, 1, 2],
    [0, 2, 1],
    [1, 0, 2],
    [1, 2, 0],
    [2, 0, 1],
    [2, 1, 0],
];

fn main() -> Result<(), Box<dyn Error>> {
    if std::env::args().len() != 1 {
        return Err("volume-stress-benchmark accepts no arguments".into());
    }
    for radius in [16, 32, 64] {
        let rows = sphere(radius)?;
        for axes in PERMUTATIONS {
            run(&format!("sphere-{radius}"), &rows, axes, true, false)?;
        }
    }
    let diagonal = diagonal()?;
    for axes in PERMUTATIONS {
        run("diagonal", &diagonal, axes, true, false)?;
    }
    let noise = noise()?;
    for axes in PERMUTATIONS {
        run("noise-1000", &noise, axes, false, true)?;
    }
    Ok(())
}

fn sphere(radius: i32) -> Result<Vec<LocalBox>, VolumeError> {
    let mut rows = Vec::new();
    for z in -radius..=radius {
        for y in -radius..=radius {
            let cross = radius * radius - y * y - z * z;
            if cross < 0 {
                continue;
            }
            let half = cross.isqrt();
            let minimum = [128 - half, 128 + y, 128 + z].map(|v| u16::try_from(v).unwrap_or(256));
            let maximum = [129 + half, 129 + y, 129 + z].map(|v| u16::try_from(v).unwrap_or(256));
            rows.push(LocalBox::new(minimum, maximum)?);
        }
    }
    Ok(rows)
}

fn diagonal() -> Result<Vec<LocalBox>, VolumeError> {
    let mut rows = Vec::new();
    for z in 64..192 {
        for y in 64..192 {
            let x = 64 + (y + z - 128) / 2;
            rows.push(LocalBox::new([x, y, z], [x + 3, y + 1, z + 1])?);
        }
    }
    Ok(rows)
}

fn noise() -> Result<Vec<LocalBox>, VolumeError> {
    let mut rows = Vec::new();
    let mut seed = 0x13cb_5d9e_u32;
    for _ in 0..1000 {
        let minimum = std::array::from_fn(|_| {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            u16::try_from((seed >> 24) % 248).unwrap_or(0)
        });
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let edge = 1 + u16::try_from(seed >> 29).unwrap_or(0);
        rows.push(LocalBox::new(minimum, minimum.map(|v| v + edge))?);
    }
    Ok(rows)
}

fn run(
    name: &str,
    rows: &[LocalBox],
    axes: [usize; 3],
    disjoint: bool,
    continue_after_refusal: bool,
) -> Result<(), Box<dyn Error>> {
    let mut volume = RefinedVolume::uniform(Voxel::new(Material::Wood));
    let mut accepted = 0;
    let mut refused = 0;
    let mut units = 0;
    let mut max_visits = 0;
    let mut max_capacity = 0;
    let mut max_edit_ms = 0.0_f64;
    let mut last_error = String::from("none");
    let started = Instant::now();
    for &row in rows {
        let before = volume.fingerprint();
        let bounds = LocalBox::new(
            axes.map(|i| row.minimum()[i]),
            axes.map(|i| row.maximum()[i]),
        )?;
        let edit_start = Instant::now();
        let result = volume.replace_box(bounds, Voxel::AIR, VolumeLimits::default());
        max_edit_ms = max_edit_ms.max(edit_start.elapsed().as_secs_f64() * 1000.0);
        match result {
            Ok((next, work)) => {
                max_visits = max_visits.max(work.visited);
                max_capacity = max_capacity.max(work.scratch_capacity_bytes);
                volume = next;
                accepted += 1;
                units += bounds.units();
            }
            Err(error @ (VolumeError::LeafBudget | VolumeError::VisitBudget)) => {
                if before != volume.fingerprint() {
                    return Err("refused edit mutated source".into());
                }
                refused += 1;
                last_error = format!("{error:?}");
                if !continue_after_refusal {
                    break;
                }
            }
            Err(error) => return Err(error.into()),
        }
    }
    let elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;
    if disjoint && VOLUME_UNITS - volume.solid_units() != units {
        return Err("carved volume mismatch".into());
    }
    let bytes = volume.encode()?;
    if RefinedVolume::decode(&bytes)?.leaves() != volume.leaves() {
        return Err("codec mismatch".into());
    }
    let air = RefinedVolume::uniform(Voxel::AIR);
    let surface = match volume.surface([&air; 6], SurfaceLimits::default()) {
        Ok(mesh) => format!("quads:{}:visits:{}", mesh.quads.len(), mesh.visits),
        Err(error @ (VolumeError::SurfaceBudget | VolumeError::VisitBudget)) => {
            format!("refused:{error:?}")
        }
        Err(error) => return Err(error.into()),
    };
    println!(
        "VOLUME_STRESS scene={name} axes={axes:?} planned={} accepted={accepted} refused={refused} complete={} leaves={} bytes={} max_edit_visits={max_visits} max_edit_capacity_bytes={max_capacity} elapsed_ms={elapsed_ms:.3} max_edit_ms={max_edit_ms:.3} last_error={last_error} surface={surface}",
        rows.len(),
        accepted == rows.len() && refused == 0,
        volume.leaves().len(),
        bytes.len()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fixed_stress_sequences_are_bounded_and_deterministic() {
        for radius in [16, 32, 64] {
            let rows = sphere(radius).unwrap();
            assert!(rows.len() <= 129 * 129);
            assert!(
                rows.iter()
                    .all(|row| row.maximum().iter().all(|&v| v <= 256))
            );
        }
        assert_eq!(noise().unwrap(), noise().unwrap());
        assert_eq!(noise().unwrap().len(), 1000);
        assert_eq!(diagonal().unwrap().len(), 128 * 128);
    }
}
