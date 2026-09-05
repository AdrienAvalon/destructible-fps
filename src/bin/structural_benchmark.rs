use destructible_fps::{
    IVec3, Material, SampleWindow, StructuralAnchors, StructuralLimits, Voxel, VoxelChange, World,
    analyze_structural_changes,
};
use std::{error::Error, hint::black_box, time::Instant};

const DEFAULT_ITERATIONS: usize = 100;
const MAX_ITERATIONS: usize = 10_000;
const EXPECTED_ISLAND_VOXELS: usize = 8_192;

fn main() -> Result<(), Box<dyn Error>> {
    let iterations = parse_iterations()?;
    let (world, changes) = benchmark_fixture();
    let anchors = StructuralAnchors::foundation_plane(0);
    let limits = StructuralLimits::default();
    let mut samples = SampleWindow::new(iterations);
    let started = Instant::now();
    let mut expected_fingerprint = None;

    for _ in 0..iterations {
        let iteration_started = Instant::now();
        let report = analyze_structural_changes(&world, &changes, &anchors, limits)?;
        samples.record_ms(iteration_started.elapsed().as_secs_f64() * 1_000.0);
        let [island] = report.detached_islands.as_slice() else {
            return Err("benchmark fixture did not produce exactly one detached island".into());
        };
        if island.voxels.len() != EXPECTED_ISLAND_VOXELS {
            return Err(format!(
                "benchmark island has {} voxels instead of {EXPECTED_ISLAND_VOXELS}",
                island.voxels.len()
            )
            .into());
        }
        if expected_fingerprint
            .replace(island.fingerprint)
            .is_some_and(|expected| expected != island.fingerprint)
        {
            return Err("structural island fingerprint changed between iterations".into());
        }
        black_box(report);
    }

    let elapsed = started.elapsed();
    let summary = samples
        .summary()
        .ok_or("structural benchmark produced no samples")?;
    let iterations_float = f64::from(u32::try_from(iterations)?);
    println!("Destructible FPS structural benchmark — detached slab");
    println!("  iterations           {iterations}");
    println!("  island voxels        {EXPECTED_ISLAND_VOXELS}");
    println!(
        "  elapsed              {:.3} ms",
        elapsed.as_secs_f64() * 1_000.0
    );
    println!(
        "  throughput           {:.0} analyses/s",
        iterations_float / elapsed.as_secs_f64()
    );
    println!("  analysis p50         {:.3} ms", summary.p50_ms);
    println!("  analysis p95         {:.3} ms", summary.p95_ms);
    println!("  analysis p99         {:.3} ms", summary.p99_ms);
    println!("  analysis max         {:.3} ms", summary.max_ms);
    println!(
        "  island fingerprint  {:032x}",
        expected_fingerprint.ok_or("missing structural fingerprint")?
    );
    Ok(())
}

fn parse_iterations() -> Result<usize, Box<dyn Error>> {
    let mut arguments = std::env::args().skip(1);
    let mut iterations = DEFAULT_ITERATIONS;
    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--iterations" => {
                iterations = arguments
                    .next()
                    .ok_or("--iterations requires a value")?
                    .parse()?;
                if !(1..=MAX_ITERATIONS).contains(&iterations) {
                    return Err(format!("iterations must be between 1 and {MAX_ITERATIONS}").into());
                }
            }
            _ => return Err(format!("unknown argument: {argument}").into()),
        }
    }
    Ok(iterations)
}

fn benchmark_fixture() -> (World, Vec<VoxelChange>) {
    let mut world = World::default();
    world.set_voxel(IVec3::new(0, 0, 0), Voxel::new(Material::Concrete));
    world.set_voxel(IVec3::new(0, 1, 0), Voxel::new(Material::Concrete));
    world.fill_box(
        IVec3::new(-15, 2, -15),
        IVec3::new(16, 9, 16),
        Voxel::new(Material::Concrete),
    );
    let position = IVec3::new(0, 1, 0);
    let before = world.set_voxel(position, Voxel::AIR);
    (
        world,
        vec![VoxelChange {
            position,
            before,
            after: Voxel::AIR,
        }],
    )
}
