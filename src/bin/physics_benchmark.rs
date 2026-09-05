use destructible_fps::{
    BodyLimits, IVec3, Material, RigidBodyDescriptor, RigidBodyState, SampleWindow, Voxel, World,
    step_rigid_bodies,
};
use std::{collections::BTreeMap, error::Error, hint::black_box, time::Instant};

const DEFAULT_BODIES: usize = 1_024;
const DEFAULT_TICKS: usize = 300;
const MAX_BODIES: usize = 1_024;
const MAX_TICKS: usize = 10_000;
const BODIES_PER_COLUMN: usize = 4;

type BodyMap = BTreeMap<u128, RigidBodyDescriptor>;
type StateMap = BTreeMap<u128, RigidBodyState>;
type Fixture = (World, BodyMap, StateMap);

fn main() -> Result<(), Box<dyn Error>> {
    let (body_count, ticks) = parse_arguments()?;
    let (world, bodies, mut states) = fixture(body_count)?;
    let mut samples = SampleWindow::new(ticks);
    let started = Instant::now();
    let mut maximum_pairs = 0_usize;
    let mut maximum_updated = 0_usize;
    let mut static_collisions = 0_usize;
    let mut body_collisions = 0_usize;

    for _ in 0..ticks {
        let tick_started = Instant::now();
        let report = step_rigid_bodies(&world, &bodies, &mut states);
        if report.broad_phase_saturated {
            return Err("physics broad-phase pair budget saturated".into());
        }
        maximum_pairs = maximum_pairs.max(report.broad_phase_pairs);
        maximum_updated = maximum_updated.max(report.transitions.len());
        static_collisions += report.static_collisions;
        body_collisions += report.body_collisions;
        samples.record_ms(tick_started.elapsed().as_secs_f64() * 1_000.0);
        black_box((&states, report));
    }

    let elapsed = started.elapsed();
    let summary = samples
        .summary()
        .ok_or("physics benchmark produced no samples")?;
    let sleeping = states.values().filter(|state| state.sleeping).count();
    println!("Destructible FPS physics benchmark — fixed 60 Hz bodies");
    println!("  bodies               {body_count}");
    println!("  ticks                {ticks}");
    println!(
        "  elapsed              {:.3} ms",
        elapsed.as_secs_f64() * 1_000.0
    );
    println!("  tick p50             {:.3} ms", summary.p50_ms);
    println!("  tick p95             {:.3} ms", summary.p95_ms);
    println!("  tick p99             {:.3} ms", summary.p99_ms);
    println!("  tick max             {:.3} ms", summary.max_ms);
    println!("  maximum updated      {maximum_updated}");
    println!("  maximum broad pairs  {maximum_pairs}");
    println!("  static contacts      {static_collisions}");
    println!("  body contacts        {body_collisions}");
    println!("  sleeping             {sleeping}/{body_count}");
    if sleeping != body_count {
        return Err("not every benchmark body settled within the requested ticks".into());
    }
    verify_stacks(&states, body_count)?;
    Ok(())
}

fn verify_stacks(states: &StateMap, body_count: usize) -> Result<(), Box<dyn Error>> {
    let mut heights = states
        .values()
        .map(|state| {
            (
                state.translation_um.x,
                state.translation_um.z,
                state.translation_um.y,
            )
        })
        .collect::<Vec<_>>();
    heights.sort_unstable();
    for (index, column) in heights.chunks(BODIES_PER_COLUMN).enumerate() {
        let expected_len = BODIES_PER_COLUMN.min(body_count - index * BODIES_PER_COLUMN);
        if column.len() != expected_len {
            return Err("benchmark stack grouping is incomplete".into());
        }
        for (level, &(_, _, height)) in column.iter().enumerate() {
            let expected = i64::try_from(level + 1)? * destructible_fps::MICROMETERS_PER_VOXEL;
            if height != expected {
                return Err(format!(
                    "body stack did not settle canonically: expected {expected}, got {height}"
                )
                .into());
            }
        }
    }
    Ok(())
}

fn fixture(body_count: usize) -> Result<Fixture, Box<dyn Error>> {
    let column_count = body_count.div_ceil(BODIES_PER_COLUMN);
    let mut side = 1_usize;
    while side.saturating_mul(side) < column_count {
        side += 1;
    }
    let side_i32 = i32::try_from(side)?;
    let mut world = World::default();
    world.fill_box(
        IVec3::new(0, 0, 0),
        IVec3::new(side_i32 * 2, 0, side_i32 * 2),
        Voxel::new(Material::Stone),
    );
    let mut bodies = BTreeMap::new();
    let mut states = BTreeMap::new();
    for index in 0..body_count {
        let column = index / BODIES_PER_COLUMN;
        let x = i32::try_from(column % side)? * 2;
        let z = i32::try_from(column / side)? * 2;
        let y = 8 + i32::try_from(index % BODIES_PER_COLUMN)? * 2;
        let position = IVec3::new(x, y, z);
        world.set_voxel(position, Voxel::new(Material::Concrete));
        let body =
            RigidBodyDescriptor::from_world_voxels(&world, vec![position], BodyLimits::default())?;
        world.set_voxel(position, Voxel::AIR);
        states.insert(body.id, RigidBodyState::at_spawn(&body));
        bodies.insert(body.id, body);
    }
    Ok((world, bodies, states))
}

fn parse_arguments() -> Result<(usize, usize), Box<dyn Error>> {
    let mut bodies = DEFAULT_BODIES;
    let mut ticks = DEFAULT_TICKS;
    let mut arguments = std::env::args().skip(1);
    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--bodies" => {
                bodies = arguments
                    .next()
                    .ok_or("--bodies requires a value")?
                    .parse()?;
            }
            "--ticks" => {
                ticks = arguments
                    .next()
                    .ok_or("--ticks requires a value")?
                    .parse()?;
            }
            _ => return Err(format!("unknown argument: {argument}").into()),
        }
    }
    if !(1..=MAX_BODIES).contains(&bodies) {
        return Err(format!("bodies must be between 1 and {MAX_BODIES}").into());
    }
    if !(1..=MAX_TICKS).contains(&ticks) {
        return Err(format!("ticks must be between 1 and {MAX_TICKS}").into());
    }
    Ok((bodies, ticks))
}
