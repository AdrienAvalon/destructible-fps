//! Repeated industrial component-state transitions. No transport, physics, meshing or GPU claim.

use destructible_fps::{
    IVec3, SampleWindow, Voxel, World,
    industrial::industrial_world,
    volume::{LocalBox, RefinedVolume, VolumeLimits},
    world::geometry::{
        GeometryCell, GeometryChange, GeometryState, GeometryTransaction, RefinedWorld,
    },
};
use std::{error::Error, hint::black_box, time::Instant};

fn main() -> Result<(), Box<dyn Error>> {
    if std::env::args().len() != 1 {
        return Err("fixed benchmark takes no arguments".into());
    }
    let coarse = industrial_world();
    let source = RefinedWorld::from_uniform(&coarse)?;
    for batch in [1, 64, 256] {
        let cells = fixture(&coarse, batch);
        let mut authority = GeometryState::new(source.clone(), 1)?;
        let mut replicas = [authority.clone(), authority.clone()];
        let mut phases: [_; 6] = std::array::from_fn(|_| SampleWindow::new(100));
        let mut checkpoint_phases: [_; 2] = std::array::from_fn(|_| SampleWindow::new(10));
        let mut wire_bytes = 0;
        let mut checkpoint_bytes = 0;
        let mut first_ms = 0.0;
        for iteration in 0..100_u64 {
            let total = Instant::now();
            // Alternate exact coarse/fine cells. No no-op shots and no shrinking scene.
            let changes = cells
                .iter()
                .map(|(position, uniform, refined)| GeometryChange {
                    position: *position,
                    before: authority.world().cell(*position),
                    after: if iteration % 2 == 0 { refined } else { uniform }.clone(),
                })
                .collect();
            let start = Instant::now();
            let packet = authority.prepare(iteration + 1, changes)?;
            record(&mut phases[0], start);
            let start = Instant::now();
            authority.apply(&packet)?;
            record(&mut phases[1], start);
            let start = Instant::now();
            let bytes = packet.encode()?;
            record(&mut phases[2], start);
            wire_bytes = bytes.len();
            let start = Instant::now();
            let decoded = GeometryTransaction::decode(&bytes)?;
            record(&mut phases[3], start);
            let start = Instant::now();
            for replica in &mut replicas {
                replica.apply(&decoded)?;
            }
            record(&mut phases[4], start);
            let elapsed = total.elapsed().as_secs_f64() * 1000.0;
            phases[5].record_ms(elapsed);
            if iteration == 0 {
                first_ms = elapsed;
            }
            for replica in &replicas {
                if replica.world().fingerprint() != authority.world().fingerprint()
                    || replica.world().geometry_stats() != authority.world().geometry_stats()
                {
                    return Err("component replicas diverged".into());
                }
            }
            if iteration % 10 == 0 {
                let start = Instant::now();
                let checkpoint = authority.encode_checkpoint()?;
                record(&mut checkpoint_phases[0], start);
                let start = Instant::now();
                let repaired = GeometryState::decode_checkpoint(&checkpoint)?;
                record(&mut checkpoint_phases[1], start);
                if repaired.encode_checkpoint()? != checkpoint {
                    return Err("checkpoint changed exact geometry".into());
                }
                checkpoint_bytes = checkpoint.len();
                black_box(repaired);
            }
        }
        println!(
            "WORLD_GEOMETRY batch={batch} iterations=100 first_ms={first_ms:.5} cells={} chunks={} transaction_bytes={wire_bytes} refined_checkpoint_bytes={checkpoint_bytes} fingerprint={:032x}",
            authority.world().geometry_stats().occupied_cells,
            authority.world().geometry_stats().chunks,
            authority.world().fingerprint()
        );
        for (name, samples) in [
            "prepare",
            "authority_apply",
            "encode",
            "decode",
            "two_replica_apply",
            "exchange",
        ]
        .into_iter()
        .zip(phases)
        .chain(
            ["checkpoint_encode", "checkpoint_decode"]
                .into_iter()
                .zip(checkpoint_phases),
        ) {
            let summary = samples.summary().ok_or("missing measurements")?;
            println!(
                "WORLD_GEOMETRY_PHASE batch={batch} phase={name} samples={} p50_ms={:.5} p95_ms={:.5} p99_ms={:.5} max_ms={:.5}",
                summary.samples, summary.p50_ms, summary.p95_ms, summary.p99_ms, summary.max_ms
            );
        }
    }
    Ok(())
}

fn record(samples: &mut SampleWindow, start: Instant) {
    samples.record_ms(start.elapsed().as_secs_f64() * 1000.0);
}

fn fixture(coarse: &World, batch: usize) -> Vec<(IVec3, GeometryCell, GeometryCell)> {
    coarse
        .occupied_voxels()
        .into_iter()
        .take(batch)
        .map(|(position, voxel)| {
            let (page, _) = RefinedVolume::uniform(voxel)
                .replace_box(
                    LocalBox::new([127, 127, 0], [129, 129, 256]).unwrap(),
                    Voxel::AIR,
                    VolumeLimits::default(),
                )
                .unwrap();
            (
                position,
                GeometryCell::uniform(voxel),
                GeometryCell::refined(page),
            )
        })
        .collect()
}
