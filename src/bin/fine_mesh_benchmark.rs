//! Exact rendered geometry, fixed snapshots: preparation is outside the timed region.
use destructible_fps::{
    IVec3, SampleWindow,
    mesh::fine::{
        FineMeshLimits, FineMeshReport,
        fixture::{
            STAGE_NAMES, industrial_inspection_world, industrial_patch_positions, inspection_world,
        },
        hybrid_dirty_chunks, mesh_fine_chunks, mesh_hybrid_chunks,
    },
    world::geometry::RefinedWorld,
};
use std::{error::Error, hint::black_box, time::Instant};

fn main() -> Result<(), Box<dyn Error>> {
    let mut args = std::env::args().skip(1);
    let mut iterations = None;
    let mut industrial = None;
    while let Some(flag) = args.next() {
        match flag.as_str() {
            "--iterations" if iterations.is_none() => {
                iterations = Some(args.next().ok_or("missing iterations")?.parse::<usize>()?);
            }
            "--world" if industrial.is_none() => {
                industrial = Some(match args.next().as_deref() {
                    Some("inspection") => false,
                    Some("industrial") => true,
                    _ => return Err("expected inspection or industrial world".into()),
                });
            }
            _ => return Err("unknown or duplicate benchmark option".into()),
        }
    }
    let iterations = iterations.unwrap_or(100);
    let industrial = industrial.unwrap_or(false);
    if !(1..=1000).contains(&iterations) {
        return Err("iterations must be1..=1000".into());
    }
    for (stage, name) in STAGE_NAMES.iter().enumerate() {
        let world = if industrial {
            industrial_inspection_world(stage)?
        } else {
            inspection_world(stage)?
        };
        if industrial && stage == 0 {
            let start = Instant::now();
            let (report, max_work) = hybrid_report(&world, &world.chunk_positions())?;
            println!(
                "FINE_BOOTSTRAP chunks={} elapsed_ms={:.3} max_job_work={max_work} {report:?}",
                world.chunk_positions().len(),
                start.elapsed().as_secs_f64() * 1000.0
            );
        }
        let mut chunks = if industrial {
            hybrid_dirty_chunks(&industrial_patch_positions())?
        } else {
            world.chunk_positions()
        };
        chunks.sort_unstable();
        let mut samples = SampleWindow::new(iterations);
        let mut expected = None;
        for iteration in 0..iterations {
            let start = Instant::now();
            let batch = if industrial {
                mesh_hybrid_chunks(&world, &chunks, FineMeshLimits::default())?
            } else {
                mesh_fine_chunks(&world, &chunks, FineMeshLimits::default())?
            };
            let report = batch.report;
            let max_work = report.work;
            let elapsed = start.elapsed().as_secs_f64() * 1000.0;
            samples.record_ms(elapsed);
            if iteration == 0 {
                println!(
                    "FINE_MESH industrial={industrial} first_ms={elapsed:.5} stage={name} fingerprint={:032x} chunks={} max_job_work={max_work} {:?}",
                    world.fingerprint(),
                    chunks.len(),
                    report
                );
            }
            if expected.is_some_and(|previous| previous != (report, max_work)) {
                return Err("fine work counters drifted".into());
            }
            expected = Some((report, max_work));
            black_box(batch); // Keep mesh destruction outside the extraction timer.
        }
        println!(
            "FINE_MESH_TIMING stage={name} {:?}",
            samples.summary().ok_or("missing measurements")?
        );
    }
    Ok(())
}

fn hybrid_report(
    world: &RefinedWorld,
    chunks: &[IVec3],
) -> Result<(FineMeshReport, usize), Box<dyn Error>> {
    let mut total = FineMeshReport::default();
    let mut max_work = 0;
    for chunk in chunks {
        let batch = mesh_hybrid_chunks(world, &[*chunk], FineMeshLimits::default())?;
        total.vertices += batch.report.vertices;
        total.indices += batch.report.indices;
        total.quads += batch.report.quads;
        total.work += batch.report.work;
        total.lines += batch.report.lines;
        max_work = max_work.max(batch.report.work);
        black_box(batch);
    }
    Ok((total, max_work))
}
