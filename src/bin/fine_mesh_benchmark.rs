//! Exact rendered geometry, fixed snapshots: preparation is outside the timed region.
use destructible_fps::{
    SampleWindow,
    mesh::fine::{
        FineMeshLimits,
        fixture::{STAGE_NAMES, inspection_world},
        mesh_fine_chunks,
    },
};
use std::{error::Error, hint::black_box, time::Instant};

fn main() -> Result<(), Box<dyn Error>> {
    let mut args = std::env::args().skip(1);
    let iterations: usize = if let Some(flag) = args.next() {
        if flag != "--iterations" {
            return Err("expected --iterations".into());
        }
        args.next().ok_or("missing iterations")?.parse()?
    } else {
        100
    };
    if args.next().is_some() || !(1..=1000).contains(&iterations) {
        return Err("iterations must be1..=1000".into());
    }
    for (stage, name) in STAGE_NAMES.iter().enumerate() {
        let world = inspection_world(stage)?;
        let mut chunks = world.chunk_positions();
        chunks.sort_unstable();
        let mut samples = SampleWindow::new(iterations);
        let mut expected = None;
        for iteration in 0..iterations {
            let start = Instant::now();
            let batch = mesh_fine_chunks(&world, &chunks, FineMeshLimits::default())?;
            let elapsed = start.elapsed().as_secs_f64() * 1000.0;
            samples.record_ms(elapsed);
            if iteration == 0 {
                println!(
                    "FINE_MESH first_ms={elapsed:.5} stage={name} fingerprint={:032x} chunks={} {:?}",
                    world.fingerprint(),
                    chunks.len(),
                    batch.report
                );
            }
            if expected.is_some_and(|report| report != batch.report) {
                return Err("fine work counters drifted".into());
            }
            expected = Some(batch.report);
            black_box(batch);
        }
        println!(
            "FINE_MESH_TIMING stage={name} {:?}",
            samples.summary().ok_or("missing measurements")?
        );
    }
    Ok(())
}
