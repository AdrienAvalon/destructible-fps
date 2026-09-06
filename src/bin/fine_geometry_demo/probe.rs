//! Read-only material/collision probes of the snapshot actually displayed by the inspector.
use super::WorldKind;
use destructible_fps::{
    FixedMicrometers3, IVec3,
    ballistics::{FixedRay, fine::probe_static_rifle},
    convex::{ConvexQueryBudget, ConvexQueryLimits, InspectionGeometry, SceneOwner},
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
pub fn camera(scene: &InspectionGeometry, origin: Vec3, direction: Vec3) -> Result<String, String> {
    if !origin.is_finite() || origin.abs().max_element() > 16_384.0 {
        return Err("invalid inspection probe camera".into());
    }
    let aim =
        destructible_fps::player::quantized_direction(direction).ok_or("invalid inspection aim")?;
    let coordinates = origin
        .to_array()
        .map(|v| (f64::from(v) * 1_000_000.0).round() as i64);
    let ray = FixedRay::new(
        FixedMicrometers3 {
            x: coordinates[0],
            y: coordinates[1],
            z: coordinates[2],
        },
        aim,
        120_000_000,
    )
    .map_err(|e| e.to_string())?;
    let mut budget =
        ConvexQueryBudget::new(ConvexQueryLimits::default()).map_err(|e| e.to_string())?;
    let trace = scene
        .trace(&ray, TraceLimits::default(), &mut budget)
        .map_err(|e| e.to_string())?;
    let contact = trace.chords.first().map_or_else(
        || "no static material".to_owned(),
        |hit| {
            let distance = trace.length_um as f64 * hit.entry.numerator() as f64
                / hit.entry.denominator() as f64
                / 1_000_000.0;
            format!(
                "{:?} {:?} at {distance:.4}m",
                hit.owner, hit.material.material
            )
        },
    );
    Ok(format!(
        "Composite point-ray | {contact} | read-only, no damage or penetration simulation"
    ))
}

pub fn verify_convex(scene: &InspectionGeometry) -> Result<(), String> {
    let started = Instant::now();
    for (index, fragment) in scene.fragments().iter().enumerate() {
        let origin = fragment.origin();
        let base = [origin.x, origin.y, origin.z].map(|v| i64::from(v) * 1_000_000);
        let count = i64::try_from(fragment.vertices().len()).map_err(|e| e.to_string())?;
        let centre: [i64; 3] = std::array::from_fn(|axis| {
            base[axis]
                + fragment
                    .vertices()
                    .iter()
                    .map(|v| i64::from(v[axis]) * 1_000_000)
                    .sum::<i64>()
                    / (256 * count)
        });
        let top = fragment.bounds().maximum_scaled()[1] / 256 + 250_000;
        let ray = FixedRay::new(
            FixedMicrometers3 {
                x: centre[0],
                y: top,
                z: centre[2],
            },
            [0, -1, 0],
            4_000_000,
        )
        .map_err(|e| e.to_string())?;
        let mut convex =
            ConvexQueryBudget::new(ConvexQueryLimits::default()).map_err(|e| e.to_string())?;
        let trace = scene
            .trace(&ray, TraceLimits::default(), &mut convex)
            .map_err(|e| e.to_string())?;
        if !trace
            .chords
            .iter()
            .any(|hit| hit.owner == SceneOwner::Fragment(index))
        {
            return Err(format!("composite trace omitted rendered fragment {index}"));
        }
        let inner = PhysicalBox::from_micrometers(centre, centre.map(|v| v + 1))
            .map_err(|e| e.to_string())?;
        let mut world = QueryBudget::new(QueryLimits::default()).map_err(|e| e.to_string())?;
        let mut convex =
            ConvexQueryBudget::new(ConvexQueryLimits::default()).map_err(|e| e.to_string())?;
        if !scene
            .overlaps(inner, &mut world, &mut convex)
            .map_err(|e| e.to_string())?
        {
            return Err(format!(
                "composite overlap omitted rendered fragment {index}"
            ));
        }
        let start = PhysicalBox::from_micrometers(
            [centre[0], top, centre[2]],
            [centre[0] + 1, top + 1, centre[2] + 1],
        )
        .map_err(|e| e.to_string())?;
        let mut world = QueryBudget::new(QueryLimits::default()).map_err(|e| e.to_string())?;
        let mut convex =
            ConvexQueryBudget::new(ConvexQueryLimits::default()).map_err(|e| e.to_string())?;
        let sweep = scene
            .sweep_axis(start, 1, -4_000_000, &mut world, &mut convex)
            .map_err(|e| e.to_string())?;
        if !sweep.contact || sweep.displacement_um <= -4_000_000 {
            return Err(format!("composite sweep omitted rendered fragment {index}"));
        }
    }
    println!(
        "FINE_CONVEX_PROBE fragments={} cpu_ms={:.6} (same rendered source; point-rays, overlaps, translation sweeps)",
        scene.fragments().len(),
        started.elapsed().as_secs_f64() * 1000.0
    );
    Ok(())
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
