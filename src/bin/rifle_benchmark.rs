//! Fixed material/cover fixtures, reset outside timing: each sample is three shots plus two replicas.

use destructible_fps::{
    AuthoritativeServer, ClientReplica, FixedMicrometers3, FrameAssembler, IVec3, Material,
    PlayerBuildContext, SampleWindow, Voxel, World,
    ballistics::{RIFLE_CADENCE_TICKS, RifleCommand},
    decode_frame, encode_frames,
};
use std::{error::Error, hint::black_box, time::Instant};

fn main() -> Result<(), Box<dyn Error>> {
    let iterations = iterations(std::env::args().skip(1))?;
    for name in ["wood", "glass-wood", "steel", "support"] {
        let fixture = fixture(name);
        let mut samples = SampleWindow::new(iterations);
        let mut expected_counts = None;
        let mut first_ms = 0.0;
        for iteration in 0..iterations {
            // Resetting the same material state prevents sustained tests from becoming empty rays.
            let mut authority = AuthoritativeServer::new(fixture.clone());
            let mut replicas = [
                ClientReplica::new(fixture.clone()),
                ClientReplica::new(fixture.clone()),
            ];
            let mut bytes = 0;
            let mut frames = 0;
            let mut changes = 0;
            let started = Instant::now();
            for id in 1..=3 {
                let (packet, _) = authority.execute_rifle(
                    7,
                    RifleCommand {
                        command_id: id,
                        direction: [1, 0, 0],
                    },
                    player(),
                    id * RIFLE_CADENCE_TICKS,
                )?;
                changes += packet.changes.len();
                for (replica, mtu) in replicas.iter_mut().zip([1100, 1200]) {
                    let mut assembler = FrameAssembler::default();
                    let mut completed = false;
                    for frame in encode_frames(&packet, mtu)?.into_iter().rev() {
                        bytes += frame.len();
                        frames += 1;
                        if let Some(delta) = assembler.push(decode_frame(&frame)?)? {
                            replica.receive(&delta)?;
                            completed = true;
                        }
                    }
                    if !completed {
                        return Err("rifle benchmark incomplete transaction".into());
                    }
                }
            }
            let elapsed = started.elapsed().as_secs_f64() * 1000.0;
            samples.record_ms(elapsed);
            if iteration == 0 {
                first_ms = elapsed;
            }
            for replica in &replicas {
                if replica.world().fingerprint() != authority.world().fingerprint()
                    || replica.bodies() != authority.bodies()
                    || replica.body_states() != authority.body_states()
                {
                    return Err("rifle benchmark replica divergence".into());
                }
            }
            let counts = (bytes, frames, changes, authority.bodies().len());
            if expected_counts
                .replace(counts)
                .is_some_and(|old| old != counts)
            {
                return Err("rifle benchmark workload changed between iterations".into());
            }
            black_box((authority, replicas));
        }
        let summary = samples.summary().ok_or("missing samples")?;
        let (bytes, frames, changes, bodies) = expected_counts.ok_or("missing counts")?;
        println!(
            "RIFLE_BENCH scene={name} iterations={iterations} shots_per_sample=3 replicas=2 first_ms={first_ms:.4} p50_ms={:.4} p95_ms={:.4} p99_ms={:.4} max_ms={:.4} bytes={bytes} frames={frames} changes={changes} bodies={bodies}",
            summary.p50_ms, summary.p95_ms, summary.p99_ms, summary.max_ms
        );
    }
    Ok(())
}

fn fixture(name: &str) -> World {
    let mut world = World::default();
    world.fill_box(
        IVec3::new(-2, 0, -2),
        IVec3::new(4, 0, 2),
        Voxel::new(Material::Stone),
    );
    let material = match name {
        "steel" => Material::Steel,
        "glass-wood" | "support" => Material::Glass,
        _ => Material::Wood,
    };
    world.fill_box(
        IVec3::new(0, 1, 0),
        IVec3::new(0, 2, 0),
        Voxel::new(material),
    );
    if name == "glass-wood" {
        world.fill_box(
            IVec3::new(1, 1, 0),
            IVec3::new(1, 2, 0),
            Voxel::new(Material::Wood),
        );
    }
    if name == "support" {
        world.fill_box(
            IVec3::new(0, 3, 0),
            IVec3::new(0, 5, 0),
            Voxel::new(Material::Wood),
        );
    }
    world
}

const fn player() -> PlayerBuildContext {
    PlayerBuildContext {
        eye_position_um: FixedMicrometers3 {
            x: -1_500_000,
            y: 2_650_000,
            z: 500_000,
        },
        bounds_minimum_um: FixedMicrometers3 {
            x: -1_800_000,
            y: 1_000_000,
            z: 200_000,
        },
        bounds_maximum_um: FixedMicrometers3 {
            x: -1_200_000,
            y: 2_800_000,
            z: 800_000,
        },
    }
}

fn iterations(mut arguments: impl Iterator<Item = String>) -> Result<usize, Box<dyn Error>> {
    let Some(flag) = arguments.next() else {
        return Ok(500);
    };
    if flag != "--iterations" {
        return Err("expected --iterations".into());
    }
    let value = arguments.next().ok_or("missing iterations")?.parse()?;
    if !(1..=10_000).contains(&value) || arguments.next().is_some() {
        return Err("expected 1..=10000 iterations and no extra arguments".into());
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn benchmark_bounds_are_explicit() {
        assert_eq!(iterations(Vec::new().into_iter()).unwrap(), 500);
        for values in [
            vec!["--iterations", "0"],
            vec!["--iterations", "10001"],
            vec!["--unknown"],
            vec!["--iterations", "2", "extra"],
        ] {
            assert!(iterations(values.into_iter().map(str::to_owned)).is_err());
        }
    }
}
