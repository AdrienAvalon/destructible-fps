use destructible_fps::{
    AuthoritativePlayer, FixedMicrometers3, IVec3, MICROMETERS_PER_VOXEL, Material,
    PlayerInputCommand, SampleWindow, Voxel, World,
    volume::RefinedVolume,
    world::{
        geometry::{GeometryCell, GeometryChange, GeometryState, RefinedWorld},
        query::StaticGeometry,
    },
};
use std::{error::Error, hint::black_box, time::Instant};

const DEFAULT_TICKS: usize = 10_000;
const MAX_TICKS: usize = 1_000_000;
const PLAYER_COUNT: usize = 16;

fn main() -> Result<(), Box<dyn Error>> {
    let (ticks, fine) = parse_options(std::env::args().skip(1))?;
    let world = benchmark_world();
    if fine {
        let refined = refined_fixture(&world)?;
        run(refined.world(), ticks, "sixteen-max-leaf-pages")
    } else {
        run(&world, ticks, "uniform")
    }
}

fn run(world: &impl StaticGeometry, ticks: usize, scene: &str) -> Result<(), Box<dyn Error>> {
    let mut players = benchmark_players()?;
    let mut samples = SampleWindow::new(ticks);
    let started = Instant::now();
    let mut query_cells = 0_u64;
    let mut query_leaf_visits = 0_u64;
    let mut max_step_leaf_visits = 0;
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
            let report = player.step(world)?;
            query_cells += u64::try_from(report.geometry.cells)?;
            query_leaf_visits += u64::try_from(report.geometry.leaf_visits)?;
            max_step_leaf_visits = max_step_leaf_visits.max(report.geometry.leaf_visits);
            black_box(report);
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
    println!("  scene                {scene}");
    println!("  players              {PLAYER_COUNT}");
    println!("  geometry cells       {query_cells}");
    println!("  geometry leaf visits {query_leaf_visits}");
    println!("  maximum step leaves  {max_step_leaf_visits}");
    println!("  final state checksum {:032x}", state_checksum(&players));
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

fn parse_options(
    mut arguments: impl Iterator<Item = String>,
) -> Result<(usize, bool), Box<dyn Error>> {
    let mut ticks = DEFAULT_TICKS;
    let mut fine = false;
    let mut seen_ticks = false;
    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--ticks" if !seen_ticks => {
                seen_ticks = true;
                ticks = arguments
                    .next()
                    .ok_or("--ticks requires a value")?
                    .parse()?;
                if !(1..=MAX_TICKS).contains(&ticks) {
                    return Err(format!("ticks must be between 1 and {MAX_TICKS}").into());
                }
            }
            "--fine" if !fine => fine = true,
            _ => return Err(format!("unknown or repeated argument: {argument}").into()),
        }
    }
    Ok((ticks, fine))
}

fn refined_fixture(coarse: &World) -> Result<GeometryState, Box<dyn Error>> {
    // Full occupancy, nonuniform materials: identical movement geometry, worst accepted page leaf
    // count.16 pages exactly fill the aggregate131072-leaf bound; no budget is raised for this run.
    let mut stream = b"DFVL\x02".to_vec();
    stream.extend_from_slice(&8192_u32.to_le_bytes());
    for z in 0..64_u16 {
        for x in 0..128_u16 {
            for end in [(x + 1) * 2, 256, (z + 1) * 4] {
                stream.extend_from_slice(&end.to_le_bytes());
            }
            let material = if (x + z) % 2 == 0 {
                Material::Wood
            } else {
                Material::Steel
            };
            stream.extend_from_slice(&[material as u8, 255]);
        }
    }
    let page = GeometryCell::refined(RefinedVolume::decode(&stream)?);
    let mut state = GeometryState::new(RefinedWorld::from_uniform(coarse)?, 1)?;
    for x in -16..-12 {
        let changes = (39..43)
            .map(|z| {
                let position = IVec3::new(x, 0, z);
                GeometryChange {
                    position,
                    before: state.world().cell(position),
                    after: page.clone(),
                }
            })
            .collect();
        let transaction = state.prepare(1, changes)?;
        state.apply(&transaction)?;
    }
    Ok(state)
}

fn state_checksum(players: &[AuthoritativePlayer]) -> u128 {
    let mut checksum = 0_u128;
    for player in players {
        let state = player.state();
        for component in [state.position_um, state.velocity_um_per_second] {
            for value in [component.x, component.y, component.z] {
                checksum = (checksum ^ u128::from(value.cast_unsigned()))
                    .rotate_left(17)
                    .wrapping_mul(0x9e37_79b9_7f4a_7c15);
            }
        }
        for value in state.integration_remainder {
            checksum = (checksum ^ u128::from(value.cast_unsigned())).rotate_left(17);
        }
        checksum ^= u128::from(state.last_input_sequence) ^ u128::from(state.grounded);
    }
    checksum
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn benchmark_options_are_strict_and_bounded() {
        assert_eq!(
            parse_options(std::iter::empty()).unwrap(),
            (DEFAULT_TICKS, false)
        );
        assert_eq!(
            parse_options(["--fine", "--ticks", "1"].into_iter().map(str::to_owned)).unwrap(),
            (1, true)
        );
        for values in [
            vec!["--fine", "--fine"],
            vec!["--ticks", "0"],
            vec!["--ticks", "1000001"],
            vec!["--ticks"],
            vec!["--ticks", "1", "--ticks", "2"],
            vec!["--oops"],
        ] {
            assert!(parse_options(values.into_iter().map(str::to_owned)).is_err());
        }
    }
}
