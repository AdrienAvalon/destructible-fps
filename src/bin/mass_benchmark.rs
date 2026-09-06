//! Fixed geometry with no shrinking workload: shared fine integrals and actual coarse body import.
use destructible_fps::{
    BodyLimits, IVec3, Material, RigidBodyDescriptor, SampleWindow, Voxel, World,
    mass_properties::{MAX_MASS_CELLS, MassProperties, MassPropertyReport},
    volume::{LocalBox, RefinedVolume, VolumeLimits},
    world::{
        geometry::{GeometryCell, GeometryChange, GeometryState, RefinedWorld},
        query::StaticGeometry,
    },
};
use std::{error::Error, hint::black_box, time::Instant};

fn main() -> Result<(), Box<dyn Error>> {
    let iterations = options(std::env::args().skip(1))?;
    let mut world = World::default();
    let positions: Vec<_> = (0..16).map(|x| IVec3::new(x, 0, 0)).collect();
    for &position in &positions {
        world.set_voxel(position, Voxel::new(Material::Wood));
    }
    let coarse = sample_world("uniform16", &world, &positions, iterations)?;
    let fine = refined(&world, &positions, &dense_volume()?)?;
    let dense = sample_world("dense16_131072leaves", fine.world(), &positions, iterations)?;
    if dense != coarse {
        return Err("fine partition changed uniform mass moments".into());
    }
    let bored = RefinedVolume::uniform(Voxel::new(Material::Wood))
        .replace_box(
            LocalBox::new([64, 0, 64], [192, 256, 192])?,
            Voxel::AIR,
            VolumeLimits::default(),
        )?
        .0;
    let damaged = refined(&world, &positions, &bored)?;
    let remaining = sample_world("bored16", damaged.world(), &positions, iterations)?;
    if remaining.mass_kg().numerator() * 4 != coarse.mass_kg().numerator() * 3 {
        return Err("bore did not remove exactly one quarter of mass".into());
    }
    sample_body(iterations)?;
    println!("MASS_PROPERTIES_BYTES {}", size_of::<MassProperties>());
    Ok(())
}

fn options(mut arguments: impl Iterator<Item = String>) -> Result<usize, Box<dyn Error>> {
    let Some(argument) = arguments.next() else {
        return Ok(100);
    };
    if argument != "--iterations" {
        return Err("expected --iterations".into());
    }
    let value = arguments.next().ok_or("missing iterations")?.parse()?;
    if arguments.next().is_some() || !(1..=1000).contains(&value) {
        return Err("iterations must be in1..=1000 with no extra arguments".into());
    }
    Ok(value)
}

fn sample_world(
    name: &str,
    world: &impl StaticGeometry,
    positions: &[IVec3],
    iterations: usize,
) -> Result<MassProperties, Box<dyn Error>> {
    let mut samples = SampleWindow::new(iterations);
    let mut first_ms = 0.0;
    let mut expected = None;
    let mut work = MassPropertyReport::default();
    for iteration in 0..iterations {
        let start = Instant::now();
        let (properties, report) = MassProperties::from_world(world, positions)?;
        let tensor = properties.inertia_about_rounded_center()?;
        let elapsed = start.elapsed().as_secs_f64() * 1000.0;
        samples.record_ms(elapsed);
        if iteration == 0 {
            first_ms = elapsed;
        }
        if expected
            .as_ref()
            .is_some_and(|previous| previous != &properties)
        {
            return Err("mass moments drifted".into());
        }
        expected = Some(properties);
        work = report;
        black_box(tensor);
    }
    report(name, &samples, first_ms)?;
    let properties = expected.ok_or("no mass samples")?;
    println!(
        "MASS_WORK scene={name} cells={} leaves={} solids={} mass_numerator={} mass_denominator={}",
        work.cells,
        work.leaves,
        work.solid_leaves,
        properties.mass_kg().numerator(),
        properties.mass_kg().denominator()
    );
    Ok(properties)
}

fn sample_body(iterations: usize) -> Result<(), Box<dyn Error>> {
    let mut world = World::default();
    world.fill_box(
        IVec3::new(0, 0, 0),
        IVec3::new(31, 15, 31),
        Voxel::new(Material::Concrete),
    );
    let positions: Vec<_> = world
        .occupied_voxels()
        .into_iter()
        .map(|(position, _)| position)
        .collect();
    if positions.len() != MAX_MASS_CELLS {
        return Err("unexpected body fixture size".into());
    }
    let mut samples = SampleWindow::new(iterations);
    let mut first_ms = 0.0;
    for iteration in 0..iterations {
        let start = Instant::now();
        let body = RigidBodyDescriptor::from_world_voxels(
            1,
            &world,
            positions.clone(),
            BodyLimits::default(),
        )?;
        let elapsed = start.elapsed().as_secs_f64() * 1000.0;
        samples.record_ms(elapsed);
        if iteration == 0 {
            first_ms = elapsed;
        }
        if body.mass_kg != 2400 * u64::try_from(MAX_MASS_CELLS)? {
            return Err("body mass mismatch".into());
        }
        black_box(body);
    }
    report("actual_body16384", &samples, first_ms)
}

fn report(name: &str, samples: &SampleWindow, first_ms: f64) -> Result<(), Box<dyn Error>> {
    let summary = samples.summary().ok_or("no measurements")?;
    println!(
        "MASS_BENCH scene={name} samples={} first_ms={first_ms:.5} p50_ms={:.5} p95_ms={:.5} p99_ms={:.5} max_ms={:.5}",
        summary.samples, summary.p50_ms, summary.p95_ms, summary.p99_ms, summary.max_ms
    );
    Ok(())
}

fn dense_volume() -> Result<RefinedVolume, Box<dyn Error>> {
    let mut encoded = b"DFVL\x02".to_vec();
    encoded.extend_from_slice(&8192_u32.to_le_bytes());
    for z in 0..64_u16 {
        for x in 0..128_u16 {
            for end in [(x + 1) * 2, 256, (z + 1) * 4] {
                encoded.extend_from_slice(&end.to_le_bytes());
            }
            encoded.extend_from_slice(&[Material::Wood as u8, u8::try_from((x + z) % 2)?]);
        }
    }
    Ok(RefinedVolume::decode(&encoded)?)
}

fn refined(
    world: &World,
    positions: &[IVec3],
    volume: &RefinedVolume,
) -> Result<GeometryState, Box<dyn Error>> {
    let mut state = GeometryState::new(RefinedWorld::from_uniform(world)?, 1)?;
    for batch in positions.chunks(4) {
        let changes = batch
            .iter()
            .map(|&position| GeometryChange {
                position,
                before: state.world().cell(position),
                after: GeometryCell::refined(volume.clone()),
            })
            .collect();
        let transaction = state.prepare(1, changes)?;
        state.apply(&transaction)?;
    }
    Ok(state)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn benchmark_options_refuse_unbounded_or_ambiguous_work() {
        assert_eq!(options(std::iter::empty()).unwrap(), 100);
        assert_eq!(
            options(["--iterations", "1"].into_iter().map(str::to_owned)).unwrap(),
            1
        );
        for invalid in [
            vec!["--oops"],
            vec!["--iterations"],
            vec!["--iterations", "0"],
            vec!["--iterations", "1001"],
            vec!["--iterations", "1", "2"],
        ] {
            assert!(options(invalid.into_iter().map(str::to_owned)).is_err());
        }
    }
}
