use super::*;
use crate::{
    FixedMicrometers3,
    ballistics::FixedRay,
    world::query::ray::{TraceLimits, trace_materials},
};
use std::collections::VecDeque;

fn source() -> RefinedWorld {
    let coarse = RefinedWorld::from_uniform(&crate::WorldPreset::Industrial.build()).unwrap();
    super::super::windows::install(&coarse).unwrap()
}

fn solid(cell: &GeometryCell, local: [u16; 3]) -> bool {
    cell.uniform_voxel().map_or_else(
        || {
            cell.volume()
                .unwrap()
                .leaf_at(local)
                .unwrap()
                .voxel()
                .is_solid()
        },
        Voxel::is_solid,
    )
}

#[test]
fn roof_preserves_windows_and_raised_wall_contacts_and_refuses_unexpected_source() {
    let original = source();
    let fingerprint = original.fingerprint();
    let roof = install(&original).unwrap();
    assert_eq!(original.fingerprint(), fingerprint);
    assert_eq!(roof.tick(), original.tick());
    for (position, before) in original.occupied_cells() {
        let modified = (position.y == 14
            && (-20..=20).contains(&position.x)
            && (-16..=16).contains(&position.z))
            || (position.y == 18
                && (-8..=8).contains(&position.x)
                && (-13..=13).contains(&position.z))
            || (position.y == 13
                && (-19..-10).contains(&position.x)
                && [15, 16].contains(&position.z));
        if !modified {
            assert_eq!(
                roof.cell(position),
                before,
                "preserve source at {position:?}"
            );
        }
        if position.y == 15 && clerestory_support(position.x, position.z) {
            assert_eq!(
                roof.cell(IVec3::new(position.x, 14, position.z)),
                GeometryCell::uniform(Voxel::new(Material::Concrete))
            );
        }
    }
    for x in -19..-10 {
        let cell = roof.cell(IVec3::new(x, 13, 15));
        assert_eq!(
            roof.cell(IVec3::new(x, 12, 15)).solid_units(),
            crate::volume::VOLUME_UNITS
        );
        for u in (0..256).step_by(16) {
            assert!(solid(&cell, [u + 8, 0, 128]));
            assert!(!solid(&cell, [u + 8, 255, 128]));
        }
        let stub = roof.cell(IVec3::new(x, 13, 16));
        match x {
            -19 => {
                assert!(solid(&stub, [0, 8, 128]));
                assert!(!solid(&stub, [128, 8, 128]));
                assert_eq!(
                    roof.cell(IVec3::new(-20, 13, 16)).solid_units(),
                    crate::volume::VOLUME_UNITS
                );
            }
            -11 => {
                assert!(solid(&stub, [255, 8, 128]));
                assert!(!solid(&stub, [127, 8, 128]));
                assert_eq!(
                    roof.cell(IVec3::new(-10, 13, 16)).solid_units(),
                    crate::volume::VOLUME_UNITS
                );
            }
            _ => assert_eq!(stub, GeometryCell::AIR),
        }
    }
    assert_eq!(
        roof.fingerprint(),
        install(&original).unwrap().fingerprint()
    );
    assert!(
        install(&roof).is_err(),
        "never reprofile unexpected partial material"
    );
    assert_eq!(
        roof.fingerprint(),
        install(&original).unwrap().fingerprint()
    );
}

#[test]
fn authored_changes_are_subtractive_and_replay_with_unchanged_transaction_bounds() {
    let original = source();
    let roof = install(&original).unwrap();
    let mut changes: Vec<_> = original
        .occupied_cells()
        .into_iter()
        .filter_map(|(position, before)| {
            let after = roof.cell(position);
            (after != before).then_some(GeometryChange {
                position,
                before,
                after,
            })
        })
        .collect();
    assert_eq!(changes.len(), 1_455);
    assert!(changes.len() <= MAX_AUTHORED_CHANGES);
    let mut replay = GeometryState::new(original.clone(), 1).unwrap();
    changes.sort_by_key(|c| c.position);
    let mut maximum_leaves = 0;
    for batch in changes.chunks(MAX_GEOMETRY_CHANGES) {
        let leaves: usize = batch
            .iter()
            .map(|c| {
                c.before.volume().map_or(0, |v| v.leaves().len())
                    + c.after.volume().map_or(0, |v| v.leaves().len())
            })
            .sum();
        maximum_leaves = maximum_leaves.max(leaves);
        assert!(leaves <= crate::world::geometry::MAX_TRANSACTION_LEAVES);
        let tx = replay.prepare(original.tick(), batch.to_vec()).unwrap();
        replay.apply(&tx).unwrap();
    }
    assert_eq!(replay.world().fingerprint(), roof.fingerprint());
    for (position, after) in roof.occupied_cells() {
        let before = original.cell(position);
        assert!(after.solid_units() <= before.solid_units());
        if after != before {
            let expected = before.uniform_voxel().unwrap();
            if let Some(volume) = after.volume() {
                assert!(
                    volume
                        .leaves()
                        .iter()
                        .all(|leaf| !leaf.voxel().is_solid() || leaf.voxel() == expected)
                );
            }
        }
    }
    println!(
        "ROOF_AUTHORING changes={} transactions={} maximum_transaction_leaves={maximum_leaves}",
        changes.len(),
        changes.len().div_ceil(MAX_GEOMETRY_CHANGES)
    );
}

#[test]
fn exact_point_rays_see_open_skylight_and_quarter_metre_sheets() {
    let original = source();
    let roof = install(&original).unwrap();
    for (x, y, z, expected) in [
        (-15_500_000, 13_000_000, 13_500_000, false),
        (-11_500_000, 13_000_000, 500_000, true),
        (500_000, 17_000_000, 500_000, true),
    ] {
        let ray = FixedRay::new(FixedMicrometers3 { x, y, z }, [0, 1, 0], 3_000_000).unwrap();
        assert!(
            !trace_materials(&original, &ray, TraceLimits::default())
                .unwrap()
                .chords
                .is_empty()
        );
        let trace = trace_materials(&roof, &ray, TraceLimits::default()).unwrap();
        assert_eq!(trace.chords.len(), usize::from(expected));
        for chord in trace.chords {
            let a = chord.material.entry;
            let b = chord.material.exit;
            let numerator = i128::from(b.numerator()) * i128::from(a.denominator())
                - i128::from(a.numerator()) * i128::from(b.denominator());
            let denominator = i128::from(a.denominator()) * i128::from(b.denominator());
            assert_eq!(
                numerator * i128::from(trace.length_ceil_um),
                250_000 * denominator
            );
            assert_eq!(chord.material.leaf.voxel().material, Material::Concrete);
        }
    }
}

#[test]
fn retained_roof_sheets_have_no_detached_islands_on_the_authored_strip_lattice() {
    let roof = install(&source()).unwrap();
    for (y, extent_x, extent_z) in [(14, 20, 16), (18, 8, 13)] {
        let width = usize::try_from((2 * extent_x + 1) * 16).unwrap();
        let depth = usize::try_from((2 * extent_z + 1) * 16).unwrap();
        let mut occupied = vec![false; width * depth];
        for x in -extent_x..=extent_x {
            for z in -extent_z..=extent_z {
                let cell = roof.cell(IVec3::new(x, y, z));
                for u in (0_u16..256).step_by(16) {
                    for v in (0_u16..256).step_by(16) {
                        let column =
                            usize::try_from((x + extent_x) * 16 + i32::from(u / 16)).unwrap();
                        let row = usize::try_from((z + extent_z) * 16 + i32::from(v / 16)).unwrap();
                        occupied[column * depth + row] = solid(&cell, [u + 8, 32, v + 8]);
                    }
                }
            }
        }
        let count = occupied.iter().filter(|v| **v).count();
        assert!(count > 100_000);
        assert!(occupied[0]);
        let mut pending = VecDeque::from([0]);
        occupied[0] = false;
        let mut connected = 0;
        while let Some(index) = pending.pop_front() {
            connected += 1;
            let column = index / depth;
            let row = index % depth;
            for next in [
                (column > 0).then(|| index - depth),
                (column + 1 < width).then_some(index + depth),
                (row > 0).then(|| index - 1),
                (row + 1 < depth).then_some(index + 1),
            ]
            .into_iter()
            .flatten()
            {
                if occupied[next] {
                    occupied[next] = false;
                    pending.push_back(next);
                }
            }
        }
        assert_eq!(
            connected, count,
            "one connected roof sheet at y={y}, not structural-load proof"
        );
    }
    // The connected main sheet touches this actual uninterrupted ground-connected corner post.
    for y in 0..14 {
        assert_eq!(
            roof.cell(IVec3::new(-20, y, -16)).solid_units(),
            crate::volume::VOLUME_UNITS
        );
    }
}
