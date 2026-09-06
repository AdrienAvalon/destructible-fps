use super::*;
use crate::{
    FixedMicrometers3,
    ballistics::FixedRay,
    world::query::{
        PhysicalBox, QueryBudget, QueryLimits,
        ray::{TraceLimits, trace_materials},
        sweep_axis,
    },
};
use std::collections::{BTreeSet, VecDeque};

fn assert_fringe_recomposition_preserves_the_entire_occupied_volume() {
    let mut original_columns = columns().unwrap();
    for column in &mut original_columns {
        column.material =
            match (column.x.div_euclid(128) * 3 + column.z.div_euclid(128) * 7).rem_euclid(7) {
                0 | 1 => Material::Stone,
                2 => Material::Concrete,
                _ => Material::Brick,
            };
    }
    let before = prepare_pages(&original_columns).unwrap();
    let after = prepare_pages(&columns().unwrap()).unwrap();
    assert!(before.keys().eq(after.keys()));
    let mut changed_material = false;
    for (position, original) in before {
        let revised = &after[&position];
        assert_eq!(original.solid_units(), revised.solid_units());
        for old_leaf in original.leaves() {
            for new_leaf in revised.leaves() {
                if old_leaf.bounds().intersection(new_leaf.bounds()).is_some() {
                    // Exact intersections of complete canonical partitions, not point samples.
                    assert_eq!(old_leaf.voxel().is_solid(), new_leaf.voxel().is_solid());
                    if old_leaf.voxel() != new_leaf.voxel() {
                        changed_material = true;
                        assert_eq!(new_leaf.voxel().material, Material::Stone);
                    }
                }
            }
        }
    }
    assert!(changed_material);
}

fn source() -> RefinedWorld {
    let source = RefinedWorld::from_uniform(&crate::WorldPreset::Industrial.build()).unwrap();
    super::super::hardstand::install(&source).unwrap()
}

fn replace(source: &RefinedWorld, position: IVec3, after: GeometryCell) -> RefinedWorld {
    let mut state = GeometryState::new(source.clone(), 1).unwrap();
    let tx = state
        .prepare(
            source.tick(),
            vec![GeometryChange {
                position,
                before: source.cell(position),
                after,
            }],
        )
        .unwrap();
    state.apply(&tx).unwrap();
    state.world().clone()
}

#[test]
fn lobes_are_deterministic_bounded_and_preserve_every_other_source_cell() {
    assert_fringe_recomposition_preserves_the_entire_occupied_volume();
    let original = source();
    let original_hash = original.fingerprint();
    let result = install(&original).unwrap();
    assert_eq!(original.fingerprint(), original_hash);
    assert_eq!(
        result.fingerprint(),
        install(&original).unwrap().fingerprint()
    );
    assert_eq!(result.tick(), original.tick());
    assert!(install(&result).is_err());
    let pages = prepare_pages(&columns().unwrap()).unwrap();
    let leaves: usize = pages.values().map(|p| p.leaves().len()).sum();
    let mut refined_masonry = 0;
    let mut uniform_masonry = 0;
    let mut stone_only = 0;
    for page in pages.values() {
        let masonry = page
            .leaves()
            .iter()
            .any(|leaf| matches!(leaf.voxel().material, Material::Brick | Material::Concrete));
        if masonry {
            if page.uniform_voxel().is_some() {
                uniform_masonry += 1;
            } else {
                refined_masonry += 1;
            }
        } else {
            assert!(
                page.leaves().iter().all(|leaf| {
                    matches!(leaf.voxel().material, Material::Air | Material::Stone)
                })
            );
            stone_only += 1;
        }
    }
    println!(
        "COLLAPSE_MATERIALS refined_masonry={refined_masonry} uniform_masonry={uniform_masonry} stone_only={stone_only}"
    );
    assert!(pages.len() >= 40 && pages.len() <= MAX_CELLS);
    assert!(leaves > 1_000 && leaves <= MAX_LEAVES);
    for (position, before) in original.occupied_cells() {
        assert_eq!(
            result.cell(position),
            before,
            "preserve occupied source {position:?}"
        );
    }
    for (position, cell) in result.occupied_cells() {
        if let Some(expected) = pages.get(&position) {
            assert_eq!(cell, GeometryCell::refined(expected.clone()));
            assert!((1..=2).contains(&position.y));
            assert!((-22..-9).contains(&position.x));
            assert!((16..22).contains(&position.z));
            assert!(!(-17..-14).contains(&position.x));
        } else {
            assert_eq!(cell, original.cell(position));
        }
    }
    let finishes = cut_cells(&result).unwrap();
    assert!(!finishes.is_empty() && finishes.len() <= MAX_CUT_CELLS);
    assert_eq!(finishes.len(), refined_masonry);
    assert_eq!(refined_masonry + uniform_masonry + stone_only, pages.len());
    assert!(finishes.windows(2).all(|pair| pair[0] < pair[1]));
    println!(
        "COLLAPSE columns={} cells={} leaves={leaves} cut_cells={} fingerprint={:032x}",
        columns().unwrap().len(),
        pages.len(),
        finishes.len(),
        result.fingerprint()
    );
}

#[test]
fn every_column_has_exact_material_thickness_and_continuous_ground_contact() {
    let result = install(&source()).unwrap();
    let columns = columns().unwrap();
    let mut materials = BTreeSet::new();
    let mut caps = 0;
    for column in columns {
        let ray = FixedRay::new(
            FixedMicrometers3 {
                x: i64::from(column.x + column.step / 2) * 1_000_000 / 256,
                y: 4_000_000,
                z: i64::from(column.z + column.step / 2) * 1_000_000 / 256,
            },
            [0, -1, 0],
            3_500_000,
        )
        .unwrap();
        let trace = trace_materials(&result, &ray, TraceLimits::default()).unwrap();
        assert!(!trace.chords.is_empty());
        let first = trace.chords[0].material;
        let expected_top = if column.height > column.cap_base {
            caps += 1;
            Material::Concrete
        } else {
            column.material
        };
        assert_eq!(first.leaf.voxel().material, expected_top);
        // Ray starts at 4m and ends at 0.5m. Surface is exactly 1m + height/256m.
        assert_eq!(
            first.entry.numerator() * 896,
            first.entry.denominator() * i64::from(768 - column.height)
        );
        assert!(
            trace
                .chords
                .windows(2)
                .all(|pair| pair[0].material.exit == pair[1].material.entry),
            "no air gap between cap, aggregate, lower page and unchanged ground: {column:?}"
        );
        let exit = trace.chords.last().unwrap().material.exit;
        assert_eq!(exit.numerator(), exit.denominator());
        for chord in trace.chords {
            if chord.cell.y >= 1 {
                let centre_y = u16::midpoint(
                    chord.material.leaf.bounds().minimum()[1],
                    chord.material.leaf.bounds().maximum()[1],
                );
                let authored_y = (chord.cell.y - 1) * 256 + i32::from(centre_y);
                assert_eq!(
                    chord.material.leaf.voxel().material,
                    if authored_y >= column.cap_base {
                        Material::Concrete
                    } else {
                        column.material
                    }
                );
                materials.insert(chord.material.leaf.voxel().material as u8);
            }
        }
    }
    assert_eq!(
        materials,
        BTreeSet::from([
            Material::Stone as u8,
            Material::Brick as u8,
            Material::Concrete as u8
        ])
    );
    assert!(caps > 20);
}

#[test]
fn each_lobe_is_face_connected_and_the_character_route_is_unobstructed() {
    let result = install(&source()).unwrap();
    let mut points = BTreeSet::new();
    // Expand only to the coarsest shared 12.5cm grid, not a finest-resolution flood fill.
    for column in columns().unwrap() {
        for dz in (0..column.step).step_by(32) {
            for dx in (0..column.step).step_by(32) {
                points.insert((column.x + dx, column.z + dz));
            }
        }
    }
    for left in [true, false] {
        let lobe: BTreeSet<_> = points
            .iter()
            .copied()
            .filter(|&(x, _)| (x < -17 * 256) == left)
            .collect();
        let root = *lobe.first().unwrap();
        let mut reached = BTreeSet::from([root]);
        let mut queue = VecDeque::from([root]);
        while let Some((x, z)) = queue.pop_front() {
            for next in [(x - 32, z), (x + 32, z), (x, z - 32), (x, z + 32)] {
                if lobe.contains(&next) && reached.insert(next) {
                    queue.push_back(next);
                }
            }
        }
        assert_eq!(reached, lobe, "one physically face-connected pile per side");
    }
    for x in [-16_500_000, -15_500_000, -14_500_000] {
        let start = PhysicalBox::from_micrometers(
            [x - 300_000, 1_000_000, 23_000_000],
            [x + 300_000, 2_800_000, 23_600_000],
        )
        .unwrap();
        let mut budget = QueryBudget::new(QueryLimits::default()).unwrap();
        let sweep = sweep_axis(&result, start, 2, -6_750_000, &mut budget).unwrap();
        assert!(!sweep.contact);
        assert_eq!(sweep.displacement_um, -6_750_000);
    }
}

#[test]
fn invalid_support_conflicts_and_local_budget_excess_fail_without_source_mutation() {
    let original = source();
    assert!(cut_cells(&original).is_err());
    let column = columns().unwrap()[0];
    let position = IVec3::new(column.x.div_euclid(256), 1, column.z.div_euclid(256));
    for (position, cell) in [
        (position, GeometryCell::uniform(Voxel::new(Material::Steel))),
        (IVec3::new(position.x, 0, position.z), GeometryCell::AIR),
        (
            IVec3::new(position.x, 0, position.z),
            GeometryCell::refined(
                RefinedVolume::uniform(Voxel::new(Material::Soil))
                    .replace_box(
                        LocalBox::new([0, 255, 0], [1, 256, 1]).unwrap(),
                        Voxel::AIR,
                        VolumeLimits::default(),
                    )
                    .unwrap()
                    .0,
            ),
        ),
    ] {
        let invalid = replace(&original, position, cell);
        let fingerprint = invalid.fingerprint();
        assert!(install(&invalid).is_err());
        assert_eq!(invalid.fingerprint(), fingerprint);
    }
    assert!(prepare_pages(&vec![column; MAX_COLUMNS + 1]).is_err());
    assert!(add_span(&mut BTreeMap::new(), column, 0, 513, Material::Brick).is_err());
    assert!(
        add_span(
            &mut BTreeMap::new(),
            Column { step: 1, ..column },
            0,
            32,
            Material::Brick
        )
        .is_err()
    );
    assert!(aggregate_height(-17 * 256, 17 * 256).is_none());
    assert!(aggregate_height(-20 * 256, 15 * 256).is_none());
    let installed = install(&original).unwrap();
    for (position, page) in prepare_pages(&columns().unwrap()).unwrap() {
        let changed = page
            .replace_box(
                LocalBox::new([0; 3], [1; 3]).unwrap(),
                Voxel::new(Material::Steel),
                VolumeLimits::default(),
            )
            .unwrap()
            .0;
        let invalid = replace(&installed, position, GeometryCell::refined(changed));
        let fingerprint = invalid.fingerprint();
        assert!(
            cut_cells(&invalid).is_err(),
            "reject changed apron page {position:?}"
        );
        assert_eq!(invalid.fingerprint(), fingerprint);
    }
}
