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
    let world = super::super::hardstand::install(&world).unwrap();
    super::super::bay::install(&world, stage).unwrap()
}

#[test]
fn two_floor_remnants_preserve_all_existing_material_and_are_bounded_and_deterministic() {
    for stage in 0..4 {
        let original = source(stage);
        let fingerprint = original.fingerprint();
        let result = install(&original).unwrap();
        assert_eq!(original.fingerprint(), fingerprint);
        assert_eq!(result.tick(), original.tick());
        assert_eq!(result.fingerprint(), result.recompute_fingerprint());
        assert_eq!(
            result.fingerprint(),
            install(&original).unwrap().fingerprint()
        );
        assert!(install(&result).is_err());
        for (position, before) in original.occupied_cells() {
            assert_eq!(result.cell(position), before, "preserve {position:?}");
        }
        let added: Vec<_> = result
            .occupied_cells()
            .into_iter()
            .filter(|(p, _)| original.cell(*p) == GeometryCell::AIR)
            .collect();
        assert!(added.len() > 100 && added.len() <= MAX_AUTHORED_CELLS);
        let leaves: usize = added
            .iter()
            .map(|(_, c)| c.volume().unwrap().leaves().len())
            .sum();
        assert!(leaves < MAX_AUTHORED_LEAVES);
        for (p, _) in &added {
            assert!(
                (-19..=-11).contains(&p.x) && (1..=9).contains(&p.y) && (7..=14).contains(&p.z)
            );
        }
        println!(
            "CROSS_SECTION stage={stage} added_cells={} added_leaves={leaves}",
            added.len()
        );
    }
}

#[test]
fn actual_material_rays_prove_quarter_metre_floors_and_clear_storeys() {
    let result = install(&source(0)).unwrap();
    let ray = FixedRay::new(
        FixedMicrometers3 {
            x: -15_500_000,
            y: 10_000_000,
            z: 10_500_000,
        },
        [0, -1, 0],
        6_000_000,
    )
    .unwrap();
    let trace = trace_materials(&result, &ray, TraceLimits::default()).unwrap();
    assert_eq!(trace.chords.len(), 2);
    assert_eq!(
        trace.chords.iter().map(|c| c.cell.y).collect::<Vec<_>>(),
        [9, 5]
    );
    for chord in trace.chords {
        let a = chord.material.entry;
        let b = chord.material.exit;
        assert_eq!(chord.material.leaf.voxel().material, Material::Concrete);
        assert_eq!(
            (i128::from(b.numerator()) * i128::from(a.denominator())
                - i128::from(a.numerator()) * i128::from(b.denominator()))
                * 6_000_000,
            250_000 * i128::from(a.denominator()) * i128::from(b.denominator()),
        );
    }
    for y in [2_000_000, 5_500_000, 7_500_000, 9_500_000] {
        let ray = FixedRay::new(
            FixedMicrometers3 {
                x: -15_500_000,
                y,
                z: 7_500_000,
            },
            [0, 0, 1],
            7_000_000,
        )
        .unwrap();
        assert!(
            trace_materials(&result, &ray, TraceLimits::default())
                .unwrap()
                .chords
                .is_empty()
        );
    }
    // Same depth and X, lower terrace exists where the more damaged upper floor is really absent.
    let local = [128, 32, 128];
    let lower = result.cell(IVec3::new(-15, 5, 13));
    let upper = result.cell(IVec3::new(-15, 9, 13));
    assert!(
        lower
            .volume()
            .unwrap()
            .leaf_at(local)
            .unwrap()
            .voxel()
            .is_solid()
    );
    assert_eq!(upper, GeometryCell::AIR);
}

#[test]
fn exact_face_graph_connects_every_remnant_to_four_concrete_foundations() {
    let original = source(0);
    let result = install(&original).unwrap();
    let mut boxes = Vec::<([i32; 3], [i32; 3])>::new();
    for (position, cell) in result.occupied_cells() {
        let is_added = original.cell(position) == GeometryCell::AIR;
        let is_root =
            position.y == 0 && POST_X.contains(&position.x) && BEAM_Z.contains(&position.z);
        if !is_added && !is_root {
            continue;
        }
        let volume = cell
            .volume()
            .cloned()
            .unwrap_or_else(|| RefinedVolume::uniform(cell.uniform_voxel().unwrap()));
        for leaf in volume.leaves().iter().filter(|l| l.voxel().is_solid()) {
            let low = leaf.bounds().minimum();
            let high = leaf.bounds().maximum();
            let p = [position.x, position.y, position.z];
            boxes.push((
                std::array::from_fn(|a| p[a] * 256 + i32::from(low[a])),
                std::array::from_fn(|a| p[a] * 256 + i32::from(high[a])),
            ));
        }
    }
    assert!(
        boxes.len() < 1_024,
        "bound quadratic test-only adjacency work"
    );
    let mut reached: Vec<bool> = boxes.iter().map(|(lo, _)| lo[1] == 0).collect();
    assert_eq!(reached.iter().filter(|&&r| r).count(), 4);
    let mut queue: std::collections::VecDeque<_> = reached
        .iter()
        .enumerate()
        .filter_map(|(i, &r)| r.then_some(i))
        .collect();
    while let Some(i) = queue.pop_front() {
        let (a, b) = boxes[i];
        for (j, &(c, d)) in boxes.iter().enumerate() {
            if reached[j] {
                continue;
            }
            let touching = (0..3).any(|axis| {
                (b[axis] == c[axis] || d[axis] == a[axis])
                    && (0..3)
                        .filter(|&other| other != axis)
                        .all(|other| a[other] < d[other] && c[other] < b[other])
            });
            if touching {
                reached[j] = true;
                queue.push_back(j);
            }
        }
    }
    assert!(
        reached.iter().all(|&r| r),
        "no floating floor, beam or post leaf"
    );
}

#[test]
fn missing_partial_or_foreign_anchors_and_obstructions_refuse_without_source_mutation() {
    let original = source(0);
    let damaged_anchor = GeometryCell::refined(
        concrete(
            &RefinedVolume::uniform(Voxel::AIR),
            [0, 0, 0],
            [256, 128, 256],
        )
        .unwrap(),
    );
    for (position, after) in [
        (IVec3::new(-18, 0, 8), GeometryCell::AIR),
        (IVec3::new(-12, 0, 11), damaged_anchor),
        (
            IVec3::new(-18, 0, 11),
            GeometryCell::uniform(Voxel::new(Material::Wood)),
        ),
        (
            IVec3::new(-15, 5, 10),
            GeometryCell::uniform(Voxel::new(Material::Brick)),
        ),
        (
            IVec3::new(-15, 6, 10),
            GeometryCell::uniform(Voxel::new(Material::Steel)),
        ),
    ] {
        let mut state = GeometryState::new(original.clone(), 1).unwrap();
        let tx = state
            .prepare(
                original.tick(),
                vec![GeometryChange {
                    position,
                    before: original.cell(position),
                    after,
                }],
            )
            .unwrap();
        state.apply(&tx).unwrap();
        let fingerprint = state.world().fingerprint();
        assert!(install(state.world()).is_err());
        assert_eq!(state.world().fingerprint(), fingerprint);
    }
}
