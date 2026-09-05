use destructible_fps::{
    AuthoritativeServer, BuildCommand, DEFAULT_CONSTRUCTION_UNITS, IVec3, Material, SampleWindow,
    Voxel, World,
};
use std::{error::Error, hint::black_box, time::Instant};

const DEFAULT_ITERATIONS: usize = 100;
const MAX_ITERATIONS: usize = 1_000;
const BUILDS_PER_ITERATION: usize = 256;

fn main() -> Result<(), Box<dyn Error>> {
    let iterations = parse_iterations()?;
    let mut samples = SampleWindow::new(iterations.saturating_mul(BUILDS_PER_ITERATION));
    let started = Instant::now();
    for _ in 0..iterations {
        let mut world = World::default();
        world.fill_box(
            IVec3::new(0, 0, 0),
            IVec3::new(i32::try_from(BUILDS_PER_ITERATION)? - 1, 0, 0),
            Voxel::new(Material::Stone),
        );
        let mut authority = AuthoritativeServer::new(world);
        for index in 0..BUILDS_PER_ITERATION {
            let before = Instant::now();
            let command = BuildCommand {
                command_id: u64::try_from(index)? + 1,
                position: IVec3::new(i32::try_from(index)?, 1, 0),
                material: Material::Wood,
            };
            let (packet, report) = authority.execute_build(1, command)?;
            samples.record_ms(before.elapsed().as_secs_f64() * 1_000.0);
            black_box((packet, report));
        }
        if authority.construction_units(1)
            != DEFAULT_CONSTRUCTION_UNITS - u32::try_from(BUILDS_PER_ITERATION)?
        {
            return Err("construction budget diverged".into());
        }
        black_box(authority);
    }
    let elapsed = started.elapsed();
    let summary = samples.summary().ok_or("missing construction samples")?;
    let operations = iterations.saturating_mul(BUILDS_PER_ITERATION);
    println!("Destructible FPS construction benchmark - authoritative placements");
    println!("  iterations           {iterations}");
    println!("  placements           {operations}");
    println!(
        "  elapsed              {:.3} ms",
        elapsed.as_secs_f64() * 1_000.0
    );
    println!(
        "  throughput           {:.0} placements/s",
        f64::from(u32::try_from(operations)?) / elapsed.as_secs_f64()
    );
    println!(
        "  placement            p50={:.3} us p95={:.3} us p99={:.3} us max={:.3} us",
        summary.p50_ms * 1_000.0,
        summary.p95_ms * 1_000.0,
        summary.p99_ms * 1_000.0,
        summary.max_ms * 1_000.0
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
