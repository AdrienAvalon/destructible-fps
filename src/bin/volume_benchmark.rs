//! Isolated refined-volume workload, never presented as a full-world or combat benchmark.

use destructible_fps::{
    Material, SampleWindow, Voxel,
    volume::{
        LocalBox, RefinedVolume, VolumeError, VolumeLimits,
        surface::{SurfaceLimits, SurfaceQuad},
    },
};
use std::{error::Error, hint::black_box, time::Instant};

fn main() -> Result<(), Box<dyn Error>> {
    let count = iterations(std::env::args().skip(1))?;
    let base = RefinedVolume::uniform(Voxel::new(Material::Wood));
    let air = RefinedVolume::uniform(Voxel::AIR);
    for (name, minimum, maximum, refused) in [
        ("finest-cell", [17, 31, 59], [18, 32, 60], false),
        ("thin-bore", [120, 120, 0], [122, 122, 32], false),
        ("aligned-breach", [64, 64, 0], [192, 192, 128], false),
        ("offgrid-budget", [1, 2, 3], [27, 38, 49], true),
    ] {
        let bounds = LocalBox::new(minimum, maximum)?;
        let mut total = SampleWindow::new(count);
        let mut edits = SampleWindow::new(count);
        let mut surfaces = SampleWindow::new(count);
        let mut codecs = SampleWindow::new(count);
        let mut first_ms = 0.0;
        let mut counts = None;
        for iteration in 0..count {
            // Same original every iteration: workload never becomes an already empty cut.
            let start = Instant::now();
            let result = base.replace_box(bounds, Voxel::AIR, VolumeLimits::default());
            edits.record_ms(start.elapsed().as_secs_f64() * 1000.0);
            if refused {
                if result.unwrap_err() != VolumeError::LeafBudget {
                    return Err("unexpected refusal mode".into());
                }
            } else {
                let (volume, work) = result?;
                let surface_start = Instant::now();
                let surface = volume.surface([&air; 6], SurfaceLimits::default())?;
                surfaces.record_ms(surface_start.elapsed().as_secs_f64() * 1000.0);
                let codec_start = Instant::now();
                let bytes = volume.encode()?;
                let decoded = RefinedVolume::decode(&bytes)?;
                codecs.record_ms(codec_start.elapsed().as_secs_f64() * 1000.0);
                if volume.leaves() != decoded.leaves()
                    || volume.fingerprint() != decoded.fingerprint()
                    || volume.solid_units() + bounds.units() != base.solid_units()
                {
                    return Err("volume benchmark changed occupancy or codec state".into());
                }
                let current = (
                    volume.leaves().len(),
                    bytes.len(),
                    surface.quads.len(),
                    work.visited,
                    surface.visits,
                    work.scratch_capacity_bytes,
                    surface.quads.capacity() * size_of::<SurfaceQuad>(),
                );
                if counts
                    .replace(current)
                    .is_some_and(|previous| previous != current)
                {
                    return Err("volume benchmark changed workload".into());
                }
                black_box((decoded, volume, surface, bytes));
            }
            let elapsed = start.elapsed().as_secs_f64() * 1000.0;
            total.record_ms(elapsed);
            if iteration == 0 {
                first_ms = elapsed;
            }
        }
        let summary = total.summary().ok_or("missing samples")?;
        println!(
            "VOLUME_BENCH scene={name} iterations={count} refused={refused} first_ms={first_ms:.5} p50_ms={:.5} p95_ms={:.5} p99_ms={:.5} max_ms={:.5}",
            summary.p50_ms, summary.p95_ms, summary.p99_ms, summary.max_ms
        );
        for (phase, samples) in [("edit", edits), ("surface", surfaces), ("codec", codecs)] {
            if let Some(summary) = samples.summary() {
                println!(
                    "VOLUME_PHASE scene={name} phase={phase} p50_ms={:.5} p95_ms={:.5} p99_ms={:.5} max_ms={:.5}",
                    summary.p50_ms, summary.p95_ms, summary.p99_ms, summary.max_ms
                );
            }
        }
        if let Some((leaves, bytes, quads, visits, surface_visits, scratch, surface_capacity)) =
            counts
        {
            println!(
                "VOLUME_COUNTS scene={name} leaves={leaves} wire_bytes={bytes} quads={quads} edit_visits={visits} surface_visits={surface_visits} edit_leaf_capacity_bytes={scratch} surface_capacity_bytes={surface_capacity}"
            );
        }
    }
    Ok(())
}

fn iterations(mut args: impl Iterator<Item = String>) -> Result<usize, Box<dyn Error>> {
    let Some(flag) = args.next() else {
        return Ok(500);
    };
    if flag != "--iterations" {
        return Err("expected --iterations".into());
    }
    let count = args.next().ok_or("missing iterations")?.parse()?;
    if !(1..=10_000).contains(&count) || args.next().is_some() {
        return Err("expected 1..=10000 iterations and no extra arguments".into());
    }
    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn iterations_are_bounded_and_strict() {
        assert_eq!(iterations(Vec::new().into_iter()).unwrap(), 500);
        for values in [
            vec!["--iterations", "0"],
            vec!["--iterations", "10001"],
            vec!["--iterations"],
            vec!["--unknown"],
            vec!["--iterations", "2", "extra"],
        ] {
            assert!(iterations(values.into_iter().map(str::to_owned)).is_err());
        }
    }
}
