//! Read-only material/collision probes of the snapshot actually displayed by the inspector.
use super::WorldKind;
use destructible_fps::{
    FixedMicrometers3, IVec3,
    ballistics::fine::{PenetrationProbe, probe_static_rifle},
    world::{
        geometry::RefinedWorld,
        query::{PhysicalBox, QueryBudget, QueryLimits, overlaps_solid, ray::TraceLimits},
    },
};
use glam::Vec3;
use std::time::Instant;

pub fn verify_stage(kind: WorldKind, world: &RefinedWorld, stage: usize) -> Result<(), String> {
    let industrial = kind == WorldKind::Industrial;
    let x = if industrial { -15_000_000 } else { 2_000_000 };
    let y = if industrial { 2_367_187 } else { 1_367_187 };
    let z = if industrial { 18_000_000 } else { -2_000_000 };
    let direction = [0, 0, if industrial { -1 } else { 1 }];
    let started = Instant::now();
    for (label, offset, expected_wall) in [("centre", 0, stage < 2), ("rim", 1_500_000, true)] {
        let result = probe_static_rifle(
            world,
            FixedMicrometers3 {
                x: x + offset,
                y,
                z,
            },
            direction,
            TraceLimits::default(),
        )
        .map_err(|e| e.to_string())?;
        let patch_z = if industrial { 15 } else { 0 };
        let at_wall = result
            .trace
            .chords
            .first()
            .is_some_and(|c| c.cell.z == patch_z);
        if at_wall != expected_wall || result.trace.source_fingerprint != world.fingerprint() {
            return Err(format!("fine probe disagrees with stage {stage} {label}"));
        }
        let depth = if industrial {
            16_000_000 - 156_250
        } else {
            156_250
        };
        let bounds = PhysicalBox::from_micrometers(
            [x + offset, y, depth],
            [x + offset + 2, y + 2, depth + 2],
        )
        .map_err(|e| e.to_string())?;
        let mut budget = QueryBudget::new(QueryLimits::default()).map_err(|e| e.to_string())?;
        if overlaps_solid(world, bounds, &mut budget).map_err(|e| e.to_string())? != expected_wall {
            return Err(format!(
                "fine static overlap disagrees with stage {stage} {label}"
            ));
        }
        let first: Option<IVec3> = result.trace.chords.first().map(|c| c.cell);
        println!(
            "FINE_PROBE stage={stage} label={label} fingerprint={:032x} first={first:?} runs={} remaining_micro_work={} cells={} leaves={} (read-only point-ray, not damage)",
            world.fingerprint(),
            result.material_runs,
            result.remaining_micro_work,
            result.trace.stats.cells,
            result.trace.stats.leaf_visits
        );
    }
    println!(
        "FINE_PROBE_TIME stage={stage} cpu_ms={:.6}",
        started.elapsed().as_secs_f64() * 1000.0
    );
    Ok(())
}

#[allow(clippy::cast_possible_truncation)]
pub fn camera(world: &RefinedWorld, origin: Vec3, direction: Vec3) -> Result<String, String> {
    if !origin.is_finite() || origin.abs().max_element() > 16_384.0 {
        return Err("invalid inspection probe camera".into());
    }
    let aim =
        destructible_fps::player::quantized_direction(direction).ok_or("invalid inspection aim")?;
    let coordinates = origin
        .to_array()
        .map(|v| (f64::from(v) * 1_000_000.0).round() as i64);
    let result = probe_static_rifle(
        world,
        FixedMicrometers3 {
            x: coordinates[0],
            y: coordinates[1],
            z: coordinates[2],
        },
        aim,
        TraceLimits::default(),
    )
    .map_err(|e| e.to_string())?;
    Ok(summary(&result))
}

#[allow(clippy::cast_precision_loss)]
fn summary(result: &PenetrationProbe) -> String {
    let contact = result.trace.chords.first().map_or_else(
        || "no static material".to_owned(),
        |c| {
            let distance = result.trace.length_um as f64 * c.material.entry.numerator() as f64
                / c.material.entry.denominator() as f64
                / 1_000_000.0;
            format!("{:?} at {distance:.4}m", c.material.leaf.voxel().material)
        },
    );
    format!(
        "Fine point-ray probe | {contact} | remaining {}/750000000 micro-work | read-only, no damage",
        result.remaining_micro_work
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_stage_probe_checks_bores_remaining_material_and_same_static_collisions() {
        for stage in 0..4 {
            let small = destructible_fps::mesh::fine::fixture::inspection_world(stage).unwrap();
            verify_stage(WorldKind::Inspection, &small, stage).unwrap();
            let industrial =
                destructible_fps::mesh::fine::fixture::industrial_inspection_world(stage).unwrap();
            verify_stage(WorldKind::Industrial, &industrial, stage).unwrap();
        }
    }
}
