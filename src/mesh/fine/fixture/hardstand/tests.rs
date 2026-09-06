use super::*;
use crate::{
    FixedMicrometers3,
    ballistics::FixedRay,
    world::query::ray::{TraceLimits, trace_materials},
};

fn source() -> RefinedWorld {
    let coarse = RefinedWorld::from_uniform(&crate::WorldPreset::Industrial.build()).unwrap();
    super::super::ruins::courtyard(&coarse).unwrap()
}

#[test]
fn apron_is_bounded_deterministic_and_preserves_all_other_geometry_and_rubble_support() {
    let original = source();
    let fingerprint = original.fingerprint();
    let result = install(&original).unwrap();
    assert_eq!(original.fingerprint(), fingerprint);
    assert_eq!(
        result.fingerprint(),
        install(&original).unwrap().fingerprint()
    );
    assert_eq!(result.tick(), original.tick());
    assert!(install(&result).is_err());
    let mut changes = Vec::new();
    for (position, before) in original.occupied_cells() {
        let after = result.cell(position);
        if position.y != 0 || !footprint(position.x, position.z) {
            assert_eq!(after, before, "preserve source at {position:?}");
            continue;
        }
        let volume = after.volume().unwrap();
        let soil: u32 = volume
            .leaves()
            .iter()
            .filter(|leaf| leaf.voxel().material == Material::Soil)
            .map(|leaf| leaf.units())
            .sum();
        assert_eq!(soil, 256 * 256 * u32::from(SOIL_TOP));
        for leaf in volume.leaves() {
            let bounds = leaf.bounds();
            match leaf.voxel().material {
                Material::Soil => assert_eq!(bounds.maximum()[1], SOIL_TOP),
                Material::Concrete | Material::Air => {
                    assert_eq!(bounds.minimum()[1], SOIL_TOP);
                    assert_eq!(bounds.maximum()[1], 256);
                }
                material => panic!("unexpected apron material {material:?}"),
            }
        }
        // All material columns sit directly on a complete, unchanged soil bed.
        for x in [0, 127, 255] {
            for z in [0, 127, 255] {
                assert_eq!(
                    volume
                        .leaf_at([x, SOIL_TOP - 1, z])
                        .unwrap()
                        .voxel()
                        .material,
                    Material::Soil
                );
            }
        }
        changes.push(GeometryChange {
            position,
            before,
            after,
        });
    }
    assert_eq!(changes.len(), MAX_CELLS);
    assert_eq!(
        result.occupied_cells().len(),
        original.occupied_cells().len()
    );
    changes.sort_by_key(|c| c.position);
    let mut replay = GeometryState::new(original, 1).unwrap();
    let mut max_leaves = 0;
    for batch in changes.chunks(MAX_GEOMETRY_CHANGES) {
        let leaves: usize = batch
            .iter()
            .map(|c| c.after.volume().unwrap().leaves().len())
            .sum();
        max_leaves = max_leaves.max(leaves);
        assert!(leaves < crate::world::geometry::MAX_TRANSACTION_LEAVES);
        let tx = replay.prepare(result.tick(), batch.to_vec()).unwrap();
        replay.apply(&tx).unwrap();
    }
    assert_eq!(replay.world().fingerprint(), result.fingerprint());
    println!(
        "HARDSTAND changes={} max_transaction_leaves={max_leaves}",
        changes.len()
    );
}

#[test]
fn physical_rays_observe_real_slab_thickness_and_open_joint_soil() {
    let result = install(&source()).unwrap();
    for (x, z, expected) in [
        (-19_500_000, 24_500_000, Material::Concrete),
        (-19_990_000, 24_500_000, Material::Soil),
        (-19_500_000, 24_010_000, Material::Soil),
        // Away from both narrow seams, inside the missing triangular corner.
        (-19_900_000, 24_100_000, Material::Soil),
        // Beyond every retained tooth of the broken front edge.
        (-19_500_000, 34_950_000, Material::Soil),
        (-19_500_000, 34_250_000, Material::Concrete),
    ] {
        let ray = FixedRay::new(
            FixedMicrometers3 { x, y: 2_000_000, z },
            [0, -1, 0],
            3_000_000,
        )
        .unwrap();
        let trace = trace_materials(&result, &ray, TraceLimits::default()).unwrap();
        let first = &trace.chords[0];
        assert_eq!(first.material.leaf.voxel().material, expected);
        if expected == Material::Concrete {
            let a = first.material.entry;
            let b = first.material.exit;
            let numerator = i128::from(b.numerator()) * i128::from(a.denominator())
                - i128::from(a.numerator()) * i128::from(b.denominator());
            let denominator = i128::from(a.denominator()) * i128::from(b.denominator());
            assert_eq!(
                numerator * i128::from(trace.length_ceil_um),
                125_000 * denominator
            );
        }
    }
    // A horizontal ray inside an open joint sees its concrete side, not soil or a fake decal.
    let ray = FixedRay::new(
        FixedMicrometers3 {
            x: -19_990_000,
            y: 950_000,
            z: 24_750_000,
        },
        [1, 0, 0],
        100_000,
    )
    .unwrap();
    let trace = trace_materials(&result, &ray, TraceLimits::default()).unwrap();
    assert_eq!(trace.chords.len(), 1);
    assert_eq!(
        trace.chords[0].material.leaf.voxel().material,
        Material::Concrete
    );
}

#[test]
fn unexpected_ground_or_object_above_is_rejected_without_mutation() {
    for position in [IVec3::new(-24, 0, 22), IVec3::new(-24, 1, 22)] {
        let original = source();
        let mut state = GeometryState::new(original.clone(), 1).unwrap();
        let tx = state
            .prepare(
                original.tick(),
                vec![GeometryChange {
                    position,
                    before: original.cell(position),
                    after: GeometryCell::uniform(Voxel::new(Material::Stone)),
                }],
            )
            .unwrap();
        state.apply(&tx).unwrap();
        let fingerprint = state.world().fingerprint();
        assert!(install(state.world()).is_err());
        assert_eq!(state.world().fingerprint(), fingerprint);
    }
}
