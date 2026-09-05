use destructible_fps::{
    AuthoritativeServer, ClientReplica, IVec3, Material, SampleWindow, Voxel, World,
    structural_failure::{SectionStrength, StructuralStrengths},
    structural_jobs::{ElasticMaterial, StructuralMaterials, StructuralScheduler},
    structural_runtime::StructuralRuntime,
};
use std::{
    error::Error,
    thread,
    time::{Duration, Instant},
};

fn fixture() -> Result<AuthoritativeServer, Box<dyn Error>> {
    let mut world = World::default();
    world.fill_box(
        IVec3::new(0, 2, 4),
        IVec3::new(31, 31, 4),
        Voxel::new(Material::Brick),
    );
    for x in [0, 31] {
        world.set_voxel(IVec3::new(x, 0, 4), Voxel::new(Material::Stone));
        world.set_voxel(IVec3::new(x, 1, 4), Voxel::new(Material::Wood));
    }
    let mut strengths = [SectionStrength {
        tension_pa: 1e12,
        compression_pa: 1e12,
        shear_pa: 1e12,
    }; 7];
    strengths[2] = SectionStrength {
        tension_pa: 1e5,
        compression_pa: 1e5,
        shear_pa: 1e5,
    };
    Ok(AuthoritativeServer::new(world)
        .with_structural_materials(StructuralMaterials::new(
            [ElasticMaterial {
                // Stiff synthetic fixture keeps both stages inside the small-deformation model.
                // E=1e9 on this large one-support wall correctly reports OutsideLinearRegime.
                young_modulus_pa: 1e11,
                poisson_ratio: 0.25,
            }; 7],
        )?)
        .with_structural_strengths(StructuralStrengths::new(strengths)?))
}

fn main() -> Result<(), Box<dyn Error>> {
    let arguments: Vec<_> = std::env::args().skip(1).collect();
    if arguments.first().is_some_and(|flag| flag == "--scheduled") {
        let iterations = match &arguments[1..] {
            [] => 20,
            [flag, count] if flag == "--iterations" => count.parse()?,
            _ => {
                return Err(
                    "usage: structural-failure-benchmark --scheduled [--iterations 1..100]".into(),
                );
            }
        };
        if !(1..=100).contains(&iterations) {
            return Err("iterations must be 1..100".into());
        }
        return scheduled(iterations);
    }
    let iterations = match arguments.as_slice() {
        [] => 20,
        [flag, value] if flag == "--iterations" => value.parse()?,
        _ => return Err("usage: structural-failure-benchmark [--iterations 1..100]".into()),
    };
    if !(1..=100).contains(&iterations) {
        return Err("iterations must be 1..100".into());
    }
    let mut jobs = [SampleWindow::new(iterations), SampleWindow::new(iterations)];
    let mut commits = [SampleWindow::new(iterations), SampleWindow::new(iterations)];
    let mut scheduler = StructuralScheduler::new()?;
    let mut moved = [0; 2];
    for _ in 0..iterations {
        let mut server = fixture()?;
        let mut clients = [
            ClientReplica::new(server.world().clone()),
            ClientReplica::new(server.world().clone()),
        ];
        for stage in 0..2 {
            let started = Instant::now();
            scheduler.submit(&server, IVec3::new(16, 2, 4))?;
            let deadline = started + Duration::from_secs(5);
            let completed = loop {
                if let Some(result) = scheduler.poll()? {
                    break result;
                }
                if Instant::now() > deadline {
                    return Err("failure preparation exceeded benchmark deadline".into());
                }
                thread::yield_now();
            };
            jobs[stage].record_ms(started.elapsed().as_secs_f64() * 1000.0);
            let commit = Instant::now();
            let (packet, report) = server
                .commit_structural_failure(completed)?
                .ok_or("overloaded timber did not fail")?;
            commits[stage].record_ms(commit.elapsed().as_secs_f64() * 1000.0);
            moved[stage] = report.moved_voxels;
            for client in &mut clients {
                client.receive(&packet)?;
                if client.world().fingerprint() != server.world().fingerprint()
                    || client.bodies() != server.bodies()
                    || client.body_states() != server.body_states()
                {
                    return Err("replica did not converge".into());
                }
            }
        }
        let body_mass: u64 = server.bodies().values().map(|body| body.mass_kg).sum();
        let expected = 960 * u64::from(Material::Brick.properties().density_kg_m3)
            + 2 * u64::from(Material::Wood.properties().density_kg_m3);
        if server.world().stats().solid_voxels != 2
            || body_mass != expected
            || server.bodies().len() != 3
        {
            return Err("progressive detachment lost mass, left unsupported static cells or merged a failed cell".into());
        }
    }
    println!(
        "two-stage weak-support wall: 964 initial nodes, iterations={iterations}, moved voxels={moved:?}, replicas=2; explicit synthetic strengths"
    );
    for stage in 0..2 {
        for (name, samples) in [
            ("worker preparation", &jobs[stage]),
            ("atomic commit", &commits[stage]),
        ] {
            let metric = samples.summary().ok_or("no samples")?;
            println!(
                "  stage {} {name}: p50/p95/p99 {:.3}/{:.3}/{:.3} ms, max {:.3} ms",
                stage + 1,
                metric.p50_ms,
                metric.p95_ms,
                metric.p99_ms,
                metric.max_ms
            );
        }
    }
    Ok(())
}

fn scheduled(iterations: usize) -> Result<(), Box<dyn Error>> {
    let mut runtime_cost = SampleWindow::new(4096);
    let mut tick_cost = SampleWindow::new(4096);
    let mut latency = [
        SampleWindow::new(iterations),
        SampleWindow::new(iterations),
        SampleWindow::new(iterations),
    ];
    let interval = Duration::from_nanos(1_000_000_000 / 60);
    for _ in 0..iterations {
        let mut server = fixture()?;
        let mut replicas = [
            ClientReplica::new(server.world().clone()),
            ClientReplica::new(server.world().clone()),
        ];
        let seeds: Vec<_> = server
            .world()
            .occupied_voxels()
            .into_iter()
            .map(|(position, _)| position)
            .collect();
        let mut runtime = StructuralRuntime::new(&seeds)?;
        let began = Instant::now();
        let mut prior = began;
        let mut deadline = began;
        let mut previous_commits = 0;
        loop {
            let tick = Instant::now();
            let fracture = runtime.tick(&mut server);
            runtime_cost.record_ms(tick.elapsed().as_secs_f64() * 1000.0);
            let (motion, _) = server.advance_physics();
            tick_cost.record_ms(tick.elapsed().as_secs_f64() * 1000.0);
            for packet in fracture.iter().chain(motion.iter()) {
                for replica in &mut replicas {
                    replica.receive(packet)?;
                }
            }
            for replica in &replicas {
                if replica.world().fingerprint() != server.world().fingerprint()
                    || replica.bodies() != server.bodies()
                    || replica.body_states() != server.body_states()
                {
                    return Err("scheduled replicas diverged".into());
                }
            }
            let status = runtime.status();
            if status.failed != 0 || status.overflowed || status.stopped {
                return Err(format!(
                    "scheduled runtime unresolved: {status:?} {:?}",
                    runtime.last_error()
                )
                .into());
            }
            if status.committed > previous_commits {
                let stage = usize::try_from(previous_commits)?;
                latency
                    .get_mut(stage)
                    .ok_or("unexpected third rupture")?
                    .record_ms(prior.elapsed().as_secs_f64() * 1000.0);
                prior = Instant::now();
                previous_commits = status.committed;
            }
            if status.committed == 2 && !status.busy && status.queued == 0 {
                latency[2].record_ms(began.elapsed().as_secs_f64() * 1000.0);
                break;
            }
            if began.elapsed() > Duration::from_secs(5) {
                return Err("scheduled cascade deadline exceeded".into());
            }
            deadline += interval;
            if let Some(wait) = deadline.checked_duration_since(Instant::now()) {
                thread::sleep(wait);
            }
        }
        let mass: u64 = server.bodies().values().map(|body| body.mass_kg).sum();
        if server.world().stats().solid_voxels != 2
            || server.bodies().len() != 3
            || mass != 960 * 1900 + 2 * 650
        {
            return Err("scheduled cascade lost mass or left static panel cells".into());
        }
    }
    println!(
        "scheduled 60Hz cascade: 964 nodes/initial seeds, {iterations} repetitions, two automatic cuts, two replicas; synthetic strengths, not crushing validation"
    );
    for (label, samples) in [
        ("runtime", &runtime_cost),
        ("runtime + physics", &tick_cost),
        ("first rupture latency", &latency[0]),
        ("second rupture interval", &latency[1]),
        ("cascade total", &latency[2]),
    ] {
        let metric = samples.summary().ok_or("no scheduled samples")?;
        println!(
            "{label}: n={} p50/p95/p99 {:.3}/{:.3}/{:.3} ms max {:.3} ms",
            metric.samples, metric.p50_ms, metric.p95_ms, metric.p99_ms, metric.max_ms
        );
    }
    Ok(())
}
