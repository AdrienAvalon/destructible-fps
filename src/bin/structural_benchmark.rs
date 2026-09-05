use destructible_fps::{
    BodyLimits, IVec3, Material, RigidBodyDescriptor, SampleWindow, StructuralAnchors,
    StructuralLimits, Voxel, VoxelChange, World, analyze_structural_changes,
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
    let mut analysis_samples = SampleWindow::new(iterations);
    let mut promotion_samples = SampleWindow::new(iterations);
    let mut total_samples = SampleWindow::new(iterations);
    let started = Instant::now();
    let mut expected_fingerprint = None;

    for _ in 0..iterations {
        let iteration_started = Instant::now();
        let analysis_started = Instant::now();
        let report = analyze_structural_changes(&world, &changes, &anchors, limits)?;
        analysis_samples.record_ms(analysis_started.elapsed().as_secs_f64() * 1_000.0);
        let [island] = report.detached_islands.as_slice() else {
            return Err("benchmark fixture did not produce exactly one detached island".into());
        };
        if island.voxels().len() != EXPECTED_ISLAND_VOXELS {
            return Err(format!(
                "benchmark island has {} voxels instead of {EXPECTED_ISLAND_VOXELS}",
                island.voxels().len()
            )
            .into());
        }
        let promotion_started = Instant::now();
        let body =
            RigidBodyDescriptor::from_detached_island(1, &world, island, BodyLimits::default())?;
        promotion_samples.record_ms(promotion_started.elapsed().as_secs_f64() * 1_000.0);
        total_samples.record_ms(iteration_started.elapsed().as_secs_f64() * 1_000.0);
        if expected_fingerprint
            .replace(body.geometry_fingerprint)
            .is_some_and(|expected| expected != body.geometry_fingerprint)
        {
            return Err("structural island fingerprint changed between iterations".into());
        }
        black_box((report, body));
    }

    let elapsed = started.elapsed();
    let analysis = analysis_samples
        .summary()
        .ok_or("structural benchmark produced no samples")?;
    let promotion = promotion_samples
        .summary()
        .ok_or("rigid-body benchmark produced no samples")?;
    let total = total_samples
        .summary()
        .ok_or("combined structural benchmark produced no samples")?;
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
    println!("  analysis p50         {:.3} ms", analysis.p50_ms);
    println!("  analysis p95         {:.3} ms", analysis.p95_ms);
    println!("  analysis p99         {:.3} ms", analysis.p99_ms);
    println!("  promotion p50        {:.3} ms", promotion.p50_ms);
    println!("  promotion p95        {:.3} ms", promotion.p95_ms);
    println!("  promotion p99        {:.3} ms", promotion.p99_ms);
    println!("  combined p50         {:.3} ms", total.p50_ms);
    println!("  combined p95         {:.3} ms", total.p95_ms);
    println!("  combined p99         {:.3} ms", total.p99_ms);
    println!("  combined max         {:.3} ms", total.max_ms);
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
