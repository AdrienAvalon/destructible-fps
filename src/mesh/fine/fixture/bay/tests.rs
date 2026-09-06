use super::*;
use crate::{
    FixedMicrometers3,
    ballistics::FixedRay,
    world::query::ray::{TraceLimits, trace_materials},
};

fn source(stage: usize) -> RefinedWorld {
    let world = super::super::install_wall(
        &crate::WorldPreset::Industrial.build(),
        stage,
        IVec3::new(-17, 1, 15),
        true,
    )
    .unwrap();
    let world = super::super::ruins::courtyard(&world).unwrap();
    let world = super::super::windows::install(&world).unwrap();
    let world = super::super::roof::install(&world).unwrap();
    super::super::hardstand::install(&world).unwrap()
}

#[test]
fn bay_is_pure_subtraction_and_preserves_every_cell_outside_its_explicit_scope() {
    for stage in 0..4 {
        let original = source(stage);
        let fingerprint = original.fingerprint();
        let result = install(&original, stage).unwrap();
        assert_eq!(original.fingerprint(), fingerprint);
        assert_eq!(
            result.fingerprint(),
            install(&original, stage).unwrap().fingerprint()
        );
        assert_eq!(result.tick(), original.tick());
        assert!(install(&result, stage).is_err());
        let mut changed = 0;
        let mut removed = 0;
        for (position, before) in original.occupied_cells() {
            let after = result.cell(position);
            let scoped = ([-16, -15].contains(&position.x) && position.y == 3 && position.z == 15)
                || (-19..-10).contains(&position.x)
                    && ((position.z == 15 && (4..14).contains(&position.y))
                        || (position.z == 16 && position.y == 13));
            if !scoped {
                assert_eq!(after, before, "preserve {position:?}");
            }
            if after != before {
                changed += 1;
                assert!(after.solid_units() <= before.solid_units());
                removed += u64::from(before.solid_units() - after.solid_units());
                // A surviving leaf retains the exact original material and integrity everywhere.
                if let Some(volume) = after.volume() {
                    for leaf in volume
                        .leaves()
                        .iter()
                        .filter(|leaf| leaf.voxel().is_solid())
                    {
                        let b = leaf.bounds();
                        for x in [b.minimum()[0], b.maximum()[0] - 1] {
                            for y in [b.minimum()[1], b.maximum()[1] - 1] {
                                for z in [b.minimum()[2], b.maximum()[2] - 1] {
                                    let old = before.uniform_voxel().unwrap_or_else(|| {
                                        before.volume().unwrap().leaf_at([x, y, z]).unwrap().voxel()
                                    });
                                    assert_eq!(old, leaf.voxel());
                                }
                            }
                        }
                    }
                }
            }
        }
        assert!(changed > 50 && changed <= 99);
        for (p, _) in super::super::windows::left_front_cells().unwrap() {
            assert_eq!(result.cell(p), GeometryCell::AIR);
        }
        println!(
            "BAY stage={stage} changed={changed} removed_units={removed} stats={:?}",
            result.geometry_stats()
        );
    }
}

#[test]
fn exact_leaf_face_graph_roots_all_surviving_front_remnants_in_the_foundation() {
    // Coordinate-compressed exact 3D boxes, not a voxel-centre/raster approximation. Only the
    // modified facade and its immediate projecting column layer are in this topology proof.
    for stage in 0..4 {
        let result = install(&source(stage), stage).unwrap();
        let mut boxes = Vec::<([i32; 3], [i32; 3])>::new();
        for x in -20..=-10 {
            for y in 0..=14 {
                for z in 15..=16 {
                    let cell = result.cell(IVec3::new(x, y, z));
                    let volume = cell
                        .volume()
                        .cloned()
                        .unwrap_or_else(|| RefinedVolume::uniform(cell.uniform_voxel().unwrap()));
                    for leaf in volume
                        .leaves()
                        .iter()
                        .filter(|leaf| leaf.voxel().is_solid())
                    {
                        let low = leaf.bounds().minimum();
                        let high = leaf.bounds().maximum();
                        boxes.push((
                            std::array::from_fn(|a| [x, y, z][a] * 256 + i32::from(low[a])),
                            std::array::from_fn(|a| [x, y, z][a] * 256 + i32::from(high[a])),
                        ));
                    }
                }
            }
        }
        assert!(
            boxes.len() < 2048,
            "bound quadratic test-only adjacency work"
        );
        let mut reached: Vec<bool> = boxes.iter().map(|(lo, _)| lo[1] == 0).collect();
        let mut queue: std::collections::VecDeque<_> = reached
            .iter()
            .enumerate()
            .filter_map(|(i, &r)| r.then_some(i))
            .collect();
        let mut edges = 0;
        while let Some(i) = queue.pop_front() {
            let (a, b) = boxes[i];
            for (j, &(c, d)) in boxes.iter().enumerate() {
                if reached[j] {
                    continue;
                }
                let adjacent = (0..3).any(|axis| {
                    (b[axis] == c[axis] || d[axis] == a[axis])
                        && (0..3)
                            .filter(|&other| other != axis)
                            .all(|other| a[other] < d[other] && c[other] < b[other])
                });
                if adjacent {
                    reached[j] = true;
                    queue.push_back(j);
                    edges += 1;
                }
            }
        }
        assert!(
            reached.iter().all(|&v| v),
            "no face-disconnected fragment in facade layers"
        );
        println!(
            "BAY_TOPOLOGY stage={stage} boxes={} traversed_edges={edges}",
            boxes.len()
        );
    }
}

#[test]
fn missing_frame_and_upper_masonry_are_real_air_for_material_rays() {
    let original = source(0);
    let result = install(&original, 0).unwrap();
    for (x, y) in [
        (-16_500_000, 9_500_000),
        (-16_500_000, 12_500_000),
        (-16_500_000, 6_500_000),
    ] {
        let ray = FixedRay::new(
            FixedMicrometers3 {
                x,
                y,
                z: 16_500_000,
            },
            [0, 0, -1],
            2_000_000,
        )
        .unwrap();
        assert!(
            !trace_materials(&original, &ray, TraceLimits::default())
                .unwrap()
                .chords
                .is_empty()
        );
        assert!(
            trace_materials(&result, &ray, TraceLimits::default())
                .unwrap()
                .chords
                .is_empty()
        );
    }
    for position in [IVec3::new(-18, 7, 15), IVec3::new(-19, 4, 15)] {
        let mut state = GeometryState::new(original.clone(), 1).unwrap();
        let tx = state
            .prepare(
                original.tick(),
                vec![GeometryChange {
                    position,
                    before: original.cell(position),
                    after: GeometryCell::AIR,
                }],
            )
            .unwrap();
        state.apply(&tx).unwrap();
        let fingerprint = state.world().fingerprint();
        assert!(install(state.world(), 0).is_err());
        assert_eq!(state.world().fingerprint(), fingerprint);
    }
}

#[test]
fn authored_shoulders_and_depth_chips_have_exact_material_chords() {
    assert!(PROFILE.windows(2).all(|p| p[0].0 < p[1].0));
    assert!(top(-1, false).is_err());
    assert!(top(2_305, true).is_err());
    for u in (32..2_304).step_by(usize::from(STRIP)) {
        let back = top(u, false).unwrap();
        let front = top(u, true).unwrap();
        assert!(front <= back && back - front <= 64);
        assert!(front >= 768 && back <= 2_496);
        assert_eq!(front % 16, 0);
        assert_eq!(back % 16, 0);
    }
    // Authored landmarks prevent accidental reversion to a single symmetric valley.
    assert!(top(544, false).unwrap() > top(672, false).unwrap() + 256);
    assert!(top(1_824, false).unwrap() > top(1_696, false).unwrap() + 512);
    let original = source(0);
    let world = install(&original, 0).unwrap();
    for x in [-19, -18, -12, -11] {
        let u = (x + 19) * 256 + 32;
        let back = i64::from(top(u, false).unwrap());
        let front = i64::from(top(u, true).unwrap());
        let ray_at = |height: i64| {
            FixedRay::new(
                FixedMicrometers3 {
                    x: i64::from(x) * 1_000_000 + 125_000,
                    y: height * 1_000_000 / 256,
                    z: 16_500_000,
                },
                [0, 0, -1],
                2_000_000,
            )
            .unwrap()
        };
        let cut_ray = ray_at(i64::midpoint(back, front));
        let trace = trace_materials(&world, &cut_ray, TraceLimits::default()).unwrap();
        assert_eq!(trace.chords.len(), 1, "one retained back layer at x={x}");
        let chord = trace.chords[0].material;
        // Exactly AIR over the front 25cm and material over the rear 75cm.
        assert_eq!(chord.entry.numerator() * 8, chord.entry.denominator() * 3);
        assert_eq!(chord.exit.numerator() * 4, chord.exit.denominator() * 3);
        assert!(
            trace_materials(&world, &ray_at(back + 8), TraceLimits::default())
                .unwrap()
                .chords
                .is_empty()
        );
        let uncut = trace_materials(&original, &cut_ray, TraceLimits::default()).unwrap();
        assert_eq!(uncut.chords.len(), 1);
        assert_eq!(
            uncut.chords[0].material.entry.numerator() * 4,
            uncut.chords[0].material.entry.denominator()
        );
    }
}

#[test]
fn finish_registry_is_only_the_stable_authored_cut_pages() {
    let first = install(&source(0), 0).unwrap();
    let positions = cut_top_cells(&first);
    assert!(!positions.is_empty() && positions.len() <= 24);
    assert!(positions.windows(2).all(|p| p[0] < p[1]));
    for stage in 0..4 {
        let world = install(&source(stage), stage).unwrap();
        assert_eq!(cut_top_cells(&world), positions);
        for &position in &positions {
            assert!(
                (-19..-10).contains(&position.x)
                    && (4..14).contains(&position.y)
                    && position.z == 15
            );
            assert_eq!(world.cell(position), first.cell(position));
            assert!(
                world
                    .cell(position)
                    .volume()
                    .unwrap()
                    .leaves()
                    .iter()
                    .all(|l| [Material::Air, Material::Brick, Material::Concrete]
                        .contains(&l.voxel().material))
            );
        }
        super::super::super::finishes::SurfaceFinishes::cut_tops(&world, &positions).unwrap();
    }
    let bay_leaves: usize = positions
        .iter()
        .map(|&p| first.cell(p).volume().unwrap().leaves().len())
        .sum();
    println!("BAY_FINISH_CELLS {} leaves={bay_leaves}", positions.len());
}

#[test]
fn central_notch_removes_the_bridge_and_rejects_a_mismatched_source_stage() {
    use crate::world::query::{PhysicalBox, QueryBudget, QueryLimits, sweep_axis};
    let body = PhysicalBox::from_micrometers(
        [-15_600_000, 3_000_000, 16_250_000],
        [-15_000_000, 4_800_000, 16_850_000],
    )
    .unwrap();
    for stage in 0..4 {
        let original = source(stage);
        let fingerprint = original.fingerprint();
        let world = install(&original, stage).unwrap();
        for x in [-16, -15] {
            assert_eq!(world.cell(IVec3::new(x, 3, 15)), GeometryCell::AIR);
        }
        let clear = sweep_axis(
            &world,
            body,
            2,
            -2_000_000,
            &mut QueryBudget::new(QueryLimits::default()).unwrap(),
        )
        .unwrap();
        assert_eq!(clear.displacement_um, -2_000_000);
        assert!(
            !clear.contact,
            "no invisible bridge collider at stage {stage}"
        );
        let blocked = sweep_axis(
            &original,
            body,
            2,
            -2_000_000,
            &mut QueryBudget::new(QueryLimits::default()).unwrap(),
        )
        .unwrap();
        assert!(blocked.contact && blocked.displacement_um > -2_000_000);
        assert!(install(&original, 4).is_err());
        assert!(install(&original, if stage == 3 { 0 } else { 3 }).is_err());
        assert_eq!(original.fingerprint(), fingerprint);
    }
    let original = source(0);
    let position = IVec3::new(-16, 3, 15);
    let mut state = GeometryState::new(original.clone(), 1).unwrap();
    let tx = state
        .prepare(
            original.tick(),
            vec![GeometryChange {
                position,
                before: original.cell(position),
                after: GeometryCell::AIR,
            }],
        )
        .unwrap();
    state.apply(&tx).unwrap();
    let fingerprint = state.world().fingerprint();
    assert!(install(state.world(), 0).is_err());
    assert_eq!(state.world().fingerprint(), fingerprint);
}
