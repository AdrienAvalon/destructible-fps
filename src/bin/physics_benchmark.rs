use destructible_fps::{
    BodyId, BodyLimits, FixedQuaternion, IVec3, Material, RigidBodyDescriptor, RigidBodyState,
    SampleWindow, Voxel, World, step_rigid_bodies,
};
use std::{collections::BTreeMap, error::Error, hint::black_box, time::Instant};

const DEFAULT_BODIES: usize = 1_024;
const DEFAULT_TICKS: usize = 300;
const MAX_BODIES: usize = 1_024;
const MAX_TICKS: usize = 10_000;
const BODIES_PER_COLUMN: usize = 4;

type BodyMap = BTreeMap<BodyId, RigidBodyDescriptor>;
type StateMap = BTreeMap<BodyId, RigidBodyState>;
type Fixture = (World, BodyMap, StateMap);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Scenario {
    Stacks,
    LateralSweep,
    RotatedLateralSweep,
    AngularSweep,
    DynamicHeadOn,
}

impl Scenario {
    const fn name(self) -> &'static str {
        match self {
            Self::Stacks => "stacks",
            Self::LateralSweep => "lateral-sweep",
            Self::RotatedLateralSweep => "rotated-lateral-sweep",
            Self::AngularSweep => "angular-sweep",
            Self::DynamicHeadOn => "dynamic-head-on",
        }
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    let (body_count, ticks, scenario) = parse_arguments()?;
    let (world, bodies, mut states) = fixture(body_count, scenario)?;
    let initial_states = states.clone();
    let mut samples = SampleWindow::new(ticks);
    let started = Instant::now();
    let mut maximum_pairs = 0_usize;
    let mut maximum_updated = 0_usize;
    let mut static_collisions = 0_usize;
    let mut body_collisions = 0_usize;

    for _ in 0..ticks {
        if matches!(
            scenario,
            Scenario::LateralSweep
                | Scenario::RotatedLateralSweep
                | Scenario::AngularSweep
                | Scenario::DynamicHeadOn
        ) {
            states.clone_from(&initial_states);
        }
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
    println!("  scenario             {}", scenario.name());
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
    match scenario {
        Scenario::Stacks => {
            if sleeping != body_count {
                return Err("not every benchmark body settled within the requested ticks".into());
            }
            verify_stacks(&states, body_count)?;
        }
        Scenario::LateralSweep => {
            if static_collisions != body_count.saturating_mul(ticks) {
                return Err("not every lateral sweep reached its static wall".into());
            }
            verify_lateral_sweeps(&states, &initial_states)?;
        }
        Scenario::RotatedLateralSweep => {
            if static_collisions != body_count.saturating_mul(ticks) {
                return Err("not every rotated sweep reached its static wall".into());
            }
            verify_rotated_lateral_sweeps(&states, &initial_states)?;
        }
        Scenario::AngularSweep => {
            if static_collisions != 0 {
                return Err("free angular sweep unexpectedly touched static geometry".into());
            }
            verify_angular_sweeps(&states, &initial_states)?;
        }
        Scenario::DynamicHeadOn => {
            let expected = body_count.saturating_mul(ticks) / 2;
            if body_collisions != expected {
                return Err("not every dynamic pair resolved one head-on contact".into());
            }
            verify_dynamic_head_on(&states, &initial_states)?;
        }
    }
    Ok(())
}

fn verify_angular_sweeps(
    states: &StateMap,
    initial_states: &StateMap,
) -> Result<(), Box<dyn Error>> {
    let mut canonical = None;
    for (&body_id, state) in states {
        let initial = initial_states
            .get(&body_id)
            .ok_or("angular fixture lost an initial body state")?;
        if state.orientation == initial.orientation
            || state.angular_velocity_mrad_per_second != initial.angular_velocity_mrad_per_second
        {
            return Err(format!("body {body_id} did not complete its free angular sweep").into());
        }
        let outcome = (
            state.orientation,
            state
                .translation_um
                .y
                .saturating_sub(initial.translation_um.y),
            state.linear_velocity_um_per_second,
            state.angular_velocity_mrad_per_second,
        );
        match canonical {
            Some(expected) if outcome != expected => {
                return Err("angular sweeps did not produce one canonical outcome".into());
            }
            None => canonical = Some(outcome),
            Some(_) => {}
        }
    }
    Ok(())
}

fn verify_rotated_lateral_sweeps(
    states: &StateMap,
    initial_states: &StateMap,
) -> Result<(), Box<dyn Error>> {
    let mut canonical = None;
    for (&body_id, state) in states {
        let initial = initial_states
            .get(&body_id)
            .ok_or("rotated fixture lost an initial body state")?;
        if state.linear_velocity_um_per_second.x >= 0
            || state.angular_velocity_mrad_per_second.z == 0
        {
            return Err(format!("rotated body {body_id} did not resolve wall contact").into());
        }
        let outcome = (
            state
                .translation_um
                .x
                .saturating_sub(initial.translation_um.x),
            state.translation_um.y,
            state.linear_velocity_um_per_second,
            state.angular_velocity_mrad_per_second,
        );
        match canonical {
            Some(expected) if outcome != expected => {
                return Err("rotated sweeps did not produce one canonical outcome".into());
            }
            None => canonical = Some(outcome),
            Some(_) => {}
        }
    }
    Ok(())
}

fn verify_dynamic_head_on(
    states: &StateMap,
    initial_states: &StateMap,
) -> Result<(), Box<dyn Error>> {
    for first_id in (1..=states.len()).step_by(2) {
        let first_id = BodyId::try_from(first_id)?;
        let second_id = first_id.saturating_add(1);
        let first = states
            .get(&first_id)
            .ok_or("dynamic fixture lost its first body")?;
        let second = states
            .get(&second_id)
            .ok_or("dynamic fixture lost its second body")?;
        let first_initial = initial_states
            .get(&first_id)
            .ok_or("dynamic fixture lost its first initial state")?;
        let expected_first_x = first_initial
            .translation_um
            .x
            .saturating_add(destructible_fps::MICROMETERS_PER_VOXEL);
        if first.translation_um.x != expected_first_x
            || second.translation_um.x
                != expected_first_x.saturating_add(destructible_fps::MICROMETERS_PER_VOXEL)
            || first.linear_velocity_um_per_second.x != -26_400_000
            || second.linear_velocity_um_per_second.x != 26_400_000
        {
            return Err(
                format!("dynamic pair {first_id}/{second_id} did not resolve canonically").into(),
            );
        }
    }
    Ok(())
}

fn verify_lateral_sweeps(
    states: &StateMap,
    initial_states: &StateMap,
) -> Result<(), Box<dyn Error>> {
    for (&body_id, state) in states {
        let initial = initial_states
            .get(&body_id)
            .ok_or("lateral fixture lost an initial body state")?;
        let expected_x = initial
            .translation_um
            .x
            .saturating_add(4 * destructible_fps::MICROMETERS_PER_VOXEL);
        if state.translation_um.x != expected_x || state.linear_velocity_um_per_second.x >= 0 {
            return Err(format!("body {body_id} did not reflect at its canonical wall").into());
        }
    }
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

fn fixture(body_count: usize, scenario: Scenario) -> Result<Fixture, Box<dyn Error>> {
    match scenario {
        Scenario::Stacks => stack_fixture(body_count),
        Scenario::LateralSweep => lateral_sweep_fixture(body_count),
        Scenario::RotatedLateralSweep => rotated_lateral_sweep_fixture(body_count),
        Scenario::AngularSweep => angular_sweep_fixture(body_count),
        Scenario::DynamicHeadOn => dynamic_head_on_fixture(body_count),
    }
}

fn dynamic_head_on_fixture(body_count: usize) -> Result<Fixture, Box<dyn Error>> {
    const PAIR_SPACING: i32 = 8;
    const HEIGHT: i32 = 8;
    if !body_count.is_multiple_of(2) {
        return Err("dynamic-head-on requires an even body count".into());
    }
    let world = World::default();
    let mut bodies = BTreeMap::new();
    let mut states = BTreeMap::new();
    for index in 0..body_count {
        let pair = i32::try_from(index / 2)?;
        let within_pair = if index.is_multiple_of(2) { 0 } else { 3 };
        let position = IVec3::new(
            pair.saturating_mul(PAIR_SPACING)
                .saturating_add(within_pair),
            HEIGHT,
            0,
        );
        let body = RigidBodyDescriptor::from_replicated_voxels(
            BodyId::try_from(index + 1)?,
            vec![destructible_fps::BodyVoxel {
                position,
                voxel: Voxel::new(Material::Wood),
            }],
            BodyLimits::default(),
        )?;
        let mut state = RigidBodyState::at_spawn(&body);
        state.linear_velocity_um_per_second.x = if index.is_multiple_of(2) {
            120_000_000
        } else {
            -120_000_000
        };
        states.insert(body.id, state);
        bodies.insert(body.id, body);
    }
    Ok((world, bodies, states))
}

fn stack_fixture(body_count: usize) -> Result<Fixture, Box<dyn Error>> {
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
        let body_id = BodyId::try_from(index + 1)?;
        let body = RigidBodyDescriptor::from_world_voxels(
            body_id,
            &world,
            vec![position],
            BodyLimits::default(),
        )?;
        world.set_voxel(position, Voxel::AIR);
        states.insert(body.id, RigidBodyState::at_spawn(&body));
        bodies.insert(body.id, body);
    }
    Ok((world, bodies, states))
}

fn lateral_sweep_fixture(body_count: usize) -> Result<Fixture, Box<dyn Error>> {
    const SPACING: i32 = 8;
    const HEIGHT: i32 = 8;
    let mut world = World::default();
    let mut bodies = BTreeMap::new();
    let mut states = BTreeMap::new();
    for index in 0..body_count {
        let x = i32::try_from(index)?.saturating_mul(SPACING);
        let position = IVec3::new(x, HEIGHT, 0);
        world.set_voxel(position, Voxel::new(Material::Wood));
        let body = RigidBodyDescriptor::from_world_voxels(
            BodyId::try_from(index + 1)?,
            &world,
            vec![position],
            BodyLimits::default(),
        )?;
        world.set_voxel(position, Voxel::AIR);
        world.set_voxel(
            IVec3::new(x.saturating_add(5), HEIGHT, 0),
            Voxel::new(Material::Glass),
        );
        let mut state = RigidBodyState::at_spawn(&body);
        state.linear_velocity_um_per_second.x = 250_000_000;
        states.insert(body.id, state);
        bodies.insert(body.id, body);
    }
    Ok((world, bodies, states))
}

fn rotated_lateral_sweep_fixture(body_count: usize) -> Result<Fixture, Box<dyn Error>> {
    const SPACING: i32 = 12;
    const HEIGHT: i32 = 8;
    let mut world = World::default();
    let mut bodies = BTreeMap::new();
    let mut states = BTreeMap::new();
    for index in 0..body_count {
        let x = i32::try_from(index)?.saturating_mul(SPACING);
        let positions = [IVec3::new(x, HEIGHT, 0), IVec3::new(x + 1, HEIGHT, 0)];
        for position in positions {
            world.set_voxel(position, Voxel::new(Material::Wood));
        }
        let body = RigidBodyDescriptor::from_world_voxels(
            BodyId::try_from(index + 1)?,
            &world,
            positions.to_vec(),
            BodyLimits::default(),
        )?;
        for position in positions {
            world.set_voxel(position, Voxel::AIR);
        }
        world.set_voxel(
            IVec3::new(x.saturating_add(5), HEIGHT, 0),
            Voxel::new(Material::Glass),
        );
        let mut state = RigidBodyState::at_spawn(&body);
        state.orientation = FixedQuaternion {
            x: 0,
            y: 0,
            z: 707_107,
            w: 707_107,
        };
        state.linear_velocity_um_per_second.x = 250_000_000;
        states.insert(body.id, state);
        bodies.insert(body.id, body);
    }
    Ok((world, bodies, states))
}

fn angular_sweep_fixture(body_count: usize) -> Result<Fixture, Box<dyn Error>> {
    const SPACING: i32 = 8;
    const HEIGHT: i32 = 8;
    let world = World::default();
    let mut bodies = BTreeMap::new();
    let mut states = BTreeMap::new();
    for index in 0..body_count {
        let x = i32::try_from(index)?.saturating_mul(SPACING);
        let body = RigidBodyDescriptor::from_replicated_voxels(
            BodyId::try_from(index + 1)?,
            vec![
                destructible_fps::BodyVoxel {
                    position: IVec3::new(x, HEIGHT, 0),
                    voxel: Voxel::new(Material::Wood),
                },
                destructible_fps::BodyVoxel {
                    position: IVec3::new(x.saturating_add(1), HEIGHT, 0),
                    voxel: Voxel::new(Material::Wood),
                },
            ],
            BodyLimits::default(),
        )?;
        let mut state = RigidBodyState::at_spawn(&body);
        state.angular_velocity_mrad_per_second.z =
            destructible_fps::MAX_ANGULAR_SPEED_MRAD_PER_SECOND;
        states.insert(body.id, state);
        bodies.insert(body.id, body);
    }
    Ok((world, bodies, states))
}

fn parse_arguments() -> Result<(usize, usize, Scenario), Box<dyn Error>> {
    let mut bodies = DEFAULT_BODIES;
    let mut ticks = DEFAULT_TICKS;
    let mut scenario = Scenario::Stacks;
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
            "--scenario" => {
                scenario = match arguments
                    .next()
                    .ok_or(
                        "--scenario requires stacks, lateral-sweep, rotated-lateral-sweep, angular-sweep, or dynamic-head-on",
                    )?
                    .as_str()
                {
                    "stacks" => Scenario::Stacks,
                    "lateral-sweep" => Scenario::LateralSweep,
                    "rotated-lateral-sweep" => Scenario::RotatedLateralSweep,
                    "angular-sweep" => Scenario::AngularSweep,
                    "dynamic-head-on" => Scenario::DynamicHeadOn,
                    value => return Err(format!("unknown physics scenario: {value}").into()),
                };
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
    Ok((bodies, ticks, scenario))
}
