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
fn bay_is_pure_subtraction_and_preserves_roof_columns_low_patch_and_other_windows() {
    for stage in 0..4 {
        let original = source(stage);
        let fingerprint = original.fingerprint();
        let result = install(&original).unwrap();
        assert_eq!(original.fingerprint(), fingerprint);
        assert_eq!(
            result.fingerprint(),
            install(&original).unwrap().fingerprint()
        );
        assert_eq!(result.tick(), original.tick());
        assert!(install(&result).is_err());
        let mut changed = 0;
        let mut removed = 0;
        for (position, before) in original.occupied_cells() {
            let after = result.cell(position);
            let scoped = (-19..-10).contains(&position.x)
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
        let result = install(&source(stage)).unwrap();
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
    let result = install(&original).unwrap();
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
        assert!(install(state.world()).is_err());
        assert_eq!(state.world().fingerprint(), fingerprint);
    }
}
