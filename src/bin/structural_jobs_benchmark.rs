use destructible_fps::{
    AuthoritativeServer, IVec3, Material, SampleWindow, Voxel, World,
    structural_jobs::{
        CompletedStructuralJob, ElasticMaterial, StructuralJobError, StructuralMaterials,
        StructuralScheduler,
    },
};
use std::{
    error::Error,
    hint::black_box,
    thread,
    time::{Duration, Instant},
};

fn authority(two_supports: bool) -> Result<AuthoritativeServer, Box<dyn Error>> {
    let mut world = World::default();
    for x in 0..32 {
        for y in 0..32 {
            if two_supports && y == 0 && x != 0 && x != 31 {
                continue;
            }
            world.set_voxel(IVec3::new(x, y, 4), Voxel::new(Material::Brick));
        }
    }
    // Explicit synthetic stiffness, real game material density. Not a calibrated material profile.
    let materials = StructuralMaterials::new(
        [ElasticMaterial {
            young_modulus_pa: 1e9,
            poisson_ratio: 0.25,
        }; 7],
    )?;
    Ok(AuthoritativeServer::new(world).with_structural_materials(materials))
}

fn wait(scheduler: &mut StructuralScheduler) -> Result<CompletedStructuralJob, Box<dyn Error>> {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(result) = scheduler.poll()? {
            return Ok(result);
        }
        if Instant::now() > deadline {
            return Err("worker exceeded the benchmark timeout".into());
        }
        // This is an end-to-end queue/worker measurement, including polling and OS scheduling.
        thread::yield_now();
    }
}

fn measure(two_supports: bool, iterations: usize) -> Result<f64, Box<dyn Error>> {
    let authority = authority(two_supports)?;
    let mut scheduler = StructuralScheduler::new()?;
    let mut submission = SampleWindow::new(iterations);
    let mut latency = SampleWindow::new(iterations);
    let mut validation = SampleWindow::new(iterations);
    let mut maximum_reaction: f64 = 0.0;
    let mut maximum_iterations = 0;
    let mut maximum_observations = 0;
    for _ in 0..iterations {
        let started = Instant::now();
        scheduler.submit(&authority, IVec3::new(0, 1, 4))?;
        submission.record_ms(started.elapsed().as_secs_f64() * 1000.0);
        if scheduler.submit(&authority, IVec3::new(0, 1, 4)) != Err(StructuralJobError::Busy) {
            return Err("outstanding work did not enforce backpressure".into());
        }
        let completed = wait(&mut scheduler)?;
        latency.record_ms(started.elapsed().as_secs_f64() * 1000.0);
        let validating = Instant::now();
        let solution = black_box(authority.structural_result(&completed)?);
        validation.record_ms(validating.elapsed().as_secs_f64() * 1000.0);
        maximum_iterations = maximum_iterations.max(solution.iterations);
        maximum_observations = maximum_observations.max(completed.observed_chunks());
        let reaction: f64 = solution.reactions.iter().map(|force| force[1]).sum();
        let mass = f64::from(u32::try_from(solution.positions.len())?)
            * f64::from(Material::Brick.properties().density_kg_m3);
        if (-9.81_f64).mul_add(mass, reaction).abs() > mass * 1e-5 {
            return Err("support force does not balance selected domain weight".into());
        }
        maximum_reaction = solution
            .reactions
            .iter()
            .map(|force| force[1])
            .fold(maximum_reaction, f64::max);
    }
    println!(
        "wall32x32 two_supports={two_supports}: chunks={} nodes={} iterations={iterations} max_solver_iterations={maximum_iterations} observed_chunks={maximum_observations} maximum_support_n={maximum_reaction:.3}",
        authority.world().stats().chunks,
        authority.world().stats().solid_voxels
    );
    for (name, samples) in [
        ("submit", submission),
        ("complete job including scheduling", latency),
        ("revalidate", validation),
    ] {
        let result = samples.summary().ok_or("no samples")?;
        println!(
            "  {name}: p50/p95/p99 {:.3}/{:.3}/{:.3} ms, maximum {:.3} ms",
            result.p50_ms, result.p95_ms, result.p99_ms, result.max_ms
        );
    }
    Ok(maximum_reaction)
}

fn main() -> Result<(), Box<dyn Error>> {
    let arguments: Vec<_> = std::env::args().skip(1).collect();
    let iterations = match arguments.as_slice() {
        [] => 20,
        [flag, value] if flag == "--iterations" => value.parse()?,
        _ => return Err("usage: structural-jobs-benchmark [--iterations 1..100]".into()),
    };
    if !(1..=100).contains(&iterations) {
        return Err("iterations must be in 1..100".into());
    }
    let all_supports = measure(false, iterations)?;
    let two_supports = measure(true, iterations)?;
    if two_supports < all_supports * 10.0 {
        return Err("partial support loss did not concentrate foundation reactions".into());
    }
    Ok(())
}
