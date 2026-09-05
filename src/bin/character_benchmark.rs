use destructible_fps::{
    AuthoritativePlayer, FixedMicrometers3, IVec3, MICROMETERS_PER_VOXEL, Material,
    PlayerInputCommand, SampleWindow, Voxel, World,
};
use std::{error::Error, hint::black_box, time::Instant};

const DEFAULT_TICKS: usize = 10_000;
const MAX_TICKS: usize = 1_000_000;
const PLAYER_COUNT: usize = 16;

fn main() -> Result<(), Box<dyn Error>> {
    let ticks = parse_ticks()?;
    let world = benchmark_world();
    let mut players = benchmark_players()?;
    let mut samples = SampleWindow::new(ticks);
    let started = Instant::now();
    for tick in 0..ticks {
        let direction = if (tick / 120) % 2 == 0 { 1_000 } else { -1_000 };
        let before = Instant::now();
        for player in &mut players {
            player.accept_input(PlayerInputCommand {
                input_sequence: u64::try_from(tick)? + 1,
                movement_x_per_mille: direction,
                movement_z_per_mille: 0,
                jump: tick % 180 == 0,
                sprint: tick % 240 < 120,
            })?;
            black_box(player.step(&world));
        }
        samples.record_ms(before.elapsed().as_secs_f64() * 1_000.0);
    }
    let elapsed = started.elapsed();
    let summary = samples.summary().ok_or("missing player samples")?;
    let player_steps = ticks.saturating_mul(PLAYER_COUNT);
    if players.iter().any(|player| {
        player.state().last_input_sequence != u64::try_from(ticks).unwrap_or(u64::MAX)
            || player.state().position_um.y < MICROMETERS_PER_VOXEL
    }) {
        return Err("authoritative player benchmark diverged".into());
    }
    println!("Destructible FPS character benchmark - fixed authoritative movement");
    println!("  players              {PLAYER_COUNT}");
    println!("  server ticks          {ticks}");
    println!("  player steps          {player_steps}");
    println!(
        "  elapsed              {:.3} ms",
        elapsed.as_secs_f64() * 1_000.0
    );
    println!(
        "  throughput           {:.0} player steps/s",
        f64::from(u32::try_from(player_steps)?) / elapsed.as_secs_f64()
    );
    println!(
        "  16-player tick       p50={:.3} us p95={:.3} us p99={:.3} us max={:.3} us",
        summary.p50_ms * 1_000.0,
        summary.p95_ms * 1_000.0,
        summary.p99_ms * 1_000.0,
        summary.max_ms * 1_000.0
    );
    Ok(())
}

fn benchmark_world() -> World {
    let mut world = World::default();
    world.fill_box(
        IVec3::new(-64, 0, 20),
        IVec3::new(64, 0, 60),
        Voxel::new(Material::Stone),
    );
    world
}

fn benchmark_players() -> Result<Vec<AuthoritativePlayer>, Box<dyn Error>> {
    (0..PLAYER_COUNT)
        .map(|index| {
            Ok(AuthoritativePlayer::new(FixedMicrometers3 {
                x: (i64::try_from(index)? * 2 - 15) * MICROMETERS_PER_VOXEL,
                y: MICROMETERS_PER_VOXEL,
                z: 40 * MICROMETERS_PER_VOXEL,
            }))
        })
        .collect()
}

fn parse_ticks() -> Result<usize, Box<dyn Error>> {
    let mut arguments = std::env::args().skip(1);
    let mut ticks = DEFAULT_TICKS;
    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--ticks" => {
                ticks = arguments
                    .next()
                    .ok_or("--ticks requires a value")?
                    .parse()?;
                if !(1..=MAX_TICKS).contains(&ticks) {
                    return Err(format!("ticks must be between 1 and {MAX_TICKS}").into());
                }
            }
            _ => return Err(format!("unknown argument: {argument}").into()),
        }
    }
    Ok(ticks)
}
