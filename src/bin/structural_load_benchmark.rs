use destructible_fps::{
    IVec3, SampleWindow,
    elasticity::{ElasticJob, ElasticModel, ElasticNode, ElasticOptions, ElasticProgress},
};
use std::{error::Error, hint::black_box, time::Instant};

const fn node(x: i32, y: i32, fixed: bool, mass: f64) -> ElasticNode {
    ElasticNode {
        position: IVec3::new(x, y, 0),
        young_modulus_pa: 1e9,
        poisson_ratio: 0.25,
        mass_kg: mass,
        integrity: 255,
        fixed,
        load: [0.0; 6],
    }
}

fn fixture(name: &str) -> (Vec<ElasticNode>, [f64; 3]) {
    match name {
        "cantilever-64" => {
            let mut nodes: Vec<_> = (0..65).map(|x| node(x, 0, x == 0, 0.0)).collect();
            nodes[64].load[1] = -1.0;
            (nodes, [0.0; 3])
        }
        "column-64" => (
            (0..65).map(|y| node(0, y, y == 0, 100.0)).collect(),
            [0.0, -9.81, 0.0],
        ),
        _ => {
            let mut nodes = Vec::new();
            for x in 0..64 {
                for y in 0..32 {
                    if name == "wall-two-supports" && y == 0 && x != 0 && x != 63 {
                        continue;
                    }
                    nodes.push(node(x, y, y == 0, 100.0));
                }
            }
            (nodes, [0.0, -9.81, 0.0])
        }
    }
}

fn measure(name: &str, iterations: usize) -> Result<(), Box<dyn Error>> {
    let (nodes, gravity) = fixture(name);
    let mut build = SampleWindow::new(iterations);
    let mut solve = SampleWindow::new(iterations);
    let mut steps = SampleWindow::new(iterations * 512);
    let mut max_iterations = 0;
    let mut worst_residual: f64 = 0.0;
    let mut support_load: f64 = 0.0;
    let mut beams = 0;
    for _ in 0..iterations {
        let started = Instant::now();
        let model = ElasticModel::new(&nodes, 1.0, gravity)?;
        beams = model.beam_count();
        let mut job = ElasticJob::new(model, ElasticOptions::default())?;
        build.record_ms(started.elapsed().as_secs_f64() * 1000.0);
        let started = Instant::now();
        loop {
            let step_started = Instant::now();
            let progress = job.advance(8)?;
            steps.record_ms(step_started.elapsed().as_secs_f64() * 1000.0);
            if progress == ElasticProgress::Converged {
                break;
            }
        }
        let solution = job.finish()?;
        solve.record_ms(started.elapsed().as_secs_f64() * 1000.0);
        max_iterations = max_iterations.max(solution.iterations);
        worst_residual = worst_residual.max(solution.relative_residual);
        support_load = solution
            .reactions
            .iter()
            .map(|value| value[1].abs())
            .fold(0.0, f64::max);
        let reaction_sum: f64 = solution.reactions.iter().map(|value| value[1]).sum();
        let load: f64 = nodes
            .iter()
            .map(|node| gravity[1].mul_add(node.mass_kg, node.load[1]))
            .sum();
        if (reaction_sum + load).abs() > load.abs().max(1.0) * 1e-5 {
            return Err("support reactions do not balance the applied weight".into());
        }
        if name == "cantilever-64" {
            let expected = -64.0_f64.powi(3) / (3.0 * 1e9 / 12.0) - 64.0 / ((5.0 / 6.0) * 4e8);
            if (solution.displacements[64][1] / expected - 1.0).abs() > 1e-5 {
                return Err("long cantilever differs from the analytical deflection".into());
            }
        }
        black_box(solution);
    }
    let build = build.summary().ok_or("empty build samples")?;
    let solve = solve.summary().ok_or("empty solve samples")?;
    let steps = steps.summary().ok_or("empty step samples")?;
    println!(
        "{name}: nodes={} beams={beams} repetitions={iterations} max_iterations={max_iterations} relative_residual={worst_residual:.3e} max_support_vertical_n={support_load:.3}",
        nodes.len()
    );
    println!(
        "  build p50/p95/p99 {:.3}/{:.3}/{:.3} ms",
        build.p50_ms, build.p95_ms, build.p99_ms
    );
    println!(
        "  solve p50/p95/p99 {:.3}/{:.3}/{:.3} ms",
        solve.p50_ms, solve.p95_ms, solve.p99_ms
    );
    println!(
        "  8-iteration slice p50/p95/p99 {:.3}/{:.3}/{:.3} ms; max {:.3} ms",
        steps.p50_ms, steps.p95_ms, steps.p99_ms, steps.max_ms
    );
    Ok(())
}

fn main() -> Result<(), Box<dyn Error>> {
    let arguments: Vec<_> = std::env::args().skip(1).collect();
    let iterations: usize = match arguments.as_slice() {
        [] => 20,
        [flag, value] if flag == "--iterations" => value.parse()?,
        _ => return Err("usage: structural-load-benchmark [--iterations 1..100]".into()),
    };
    if !(1..=100).contains(&iterations) {
        return Err("iterations must be in 1..100".into());
    }
    for name in [
        "column-64",
        "cantilever-64",
        "wall-64x32",
        "wall-two-supports",
    ] {
        measure(name, iterations)?;
    }
    Ok(())
}
