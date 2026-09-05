use destructible_fps::{
    AuthoritativeServer, ClientReplica, ExplosionCommand, IVec3, MAX_SNAPSHOT_DATAGRAM_BYTES,
    SampleWindow, SnapshotAssembler, World, demo_world, encode_snapshot_frames,
};
use std::{error::Error, hint::black_box, time::Instant};

const DEFAULT_ITERATIONS: usize = 20;
const MAX_ITERATIONS: usize = 1_000;

fn main() -> Result<(), Box<dyn Error>> {
    let iterations = parse_iterations()?;
    let mut authority = AuthoritativeServer::new(demo_world());
    authority.execute_explosion(
        1,
        ExplosionCommand {
            command_id: 1,
            center: IVec3::new(-20, 6, 0),
            radius_voxels: 8,
            peak_energy: 30_000,
        },
    )?;
    let _ = authority.advance_physics();
    let mut encode_samples = SampleWindow::new(iterations);
    let mut decode_samples = SampleWindow::new(iterations);
    let mut install_samples = SampleWindow::new(iterations);
    let mut total_samples = SampleWindow::new(iterations);
    let mut expected_wire = None;
    let mut expected_frames = None;
    let started = Instant::now();

    for iteration in 0..iterations {
        let iteration_started = Instant::now();
        let encode_started = Instant::now();
        let snapshot_id = u64::try_from(iteration)?.saturating_add(1);
        let frames = encode_snapshot_frames(snapshot_id, &authority, MAX_SNAPSHOT_DATAGRAM_BYTES)?;
        encode_samples.record_ms(encode_started.elapsed().as_secs_f64() * 1_000.0);
        let wire_bytes = frames.iter().map(Vec::len).sum::<usize>();
        if expected_wire
            .replace(wire_bytes)
            .is_some_and(|value| value != wire_bytes)
            || expected_frames
                .replace(frames.len())
                .is_some_and(|value| value != frames.len())
        {
            return Err("snapshot wire size changed between iterations".into());
        }

        let decode_started = Instant::now();
        let mut assembler = SnapshotAssembler::default();
        let mut snapshot = None;
        for frame in frames.iter().rev() {
            snapshot = assembler.push(frame)?.or(snapshot);
        }
        let snapshot = snapshot.ok_or("snapshot benchmark did not complete")?;
        decode_samples.record_ms(decode_started.elapsed().as_secs_f64() * 1_000.0);

        let install_started = Instant::now();
        let mut replica = ClientReplica::new(World::default());
        snapshot.install_into(&mut replica)?;
        install_samples.record_ms(install_started.elapsed().as_secs_f64() * 1_000.0);
        total_samples.record_ms(iteration_started.elapsed().as_secs_f64() * 1_000.0);
        if replica.world().fingerprint() != authority.world().fingerprint()
            || replica.bodies() != authority.bodies()
            || replica.body_states() != authority.body_states()
        {
            return Err("snapshot benchmark replica diverged".into());
        }
        black_box((frames, replica));
    }

    let elapsed = started.elapsed();
    let encode = encode_samples.summary().ok_or("missing encode samples")?;
    let decode = decode_samples.summary().ok_or("missing decode samples")?;
    let install = install_samples.summary().ok_or("missing install samples")?;
    let total = total_samples.summary().ok_or("missing total samples")?;
    println!("Destructible FPS snapshot benchmark — representative world");
    println!("  iterations           {iterations}");
    println!(
        "  solid voxels         {}",
        authority.world().stats().solid_voxels
    );
    println!("  active bodies        {}", authority.bodies().len());
    println!(
        "  wire                 {} frames / {:.3} MiB",
        expected_frames.ok_or("missing frame count")?,
        f64::from(u32::try_from(expected_wire.ok_or("missing wire size")?)?) / 1_048_576.0
    );
    println!(
        "  elapsed              {:.3} ms",
        elapsed.as_secs_f64() * 1_000.0
    );
    print_summary("encode", encode);
    print_summary("reassemble/decode", decode);
    print_summary("validate/install", install);
    print_summary("total", total);
    Ok(())
}

fn print_summary(label: &str, summary: destructible_fps::DistributionSummary) {
    println!(
        "  {label:<20} p50={:.3} ms p95={:.3} ms p99={:.3} ms max={:.3} ms",
        summary.p50_ms, summary.p95_ms, summary.p99_ms, summary.max_ms
    );
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
