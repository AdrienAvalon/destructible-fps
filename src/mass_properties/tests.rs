use super::*;
use crate::{
    BodyLimits, Material, RigidBodyDescriptor, Voxel, World,
    volume::{RefinedVolume, VolumeLimits},
    world::geometry::{GeometryChange, GeometryState, RefinedWorld},
};

fn equal(value: ExactRatio, numerator: i128, denominator: i128) {
    assert_eq!(
        value.numerator() * denominator,
        numerator * value.denominator()
    );
}
fn page(volume: RefinedVolume, position: IVec3) -> GeometryState {
    let mut state = GeometryState::new(RefinedWorld::default(), 1).unwrap();
    let transaction = state
        .prepare(
            1,
            vec![GeometryChange {
                position,
                before: GeometryCell::AIR,
                after: GeometryCell::refined(volume),
            }],
        )
        .unwrap();
    state.apply(&transaction).unwrap();
    state
}
fn shape(low: [u16; 3], high: [u16; 3], voxel: Voxel) -> RefinedVolume {
    RefinedVolume::uniform(Voxel::AIR)
        .replace_box(
            LocalBox::new(low, high).unwrap(),
            voxel,
            VolumeLimits::default(),
        )
        .unwrap()
        .0
}

#[test]
fn actual_body_promotion_uses_same_exact_moments_as_world_geometry() {
    let mut world = World::default();
    let positions = [IVec3::new(-2, 3, 0), IVec3::new(-1, 3, 0)];
    world.set_voxel(positions[0], Voxel::new(Material::Wood));
    world.set_voxel(positions[1], Voxel::new(Material::Steel));
    let body = RigidBodyDescriptor::from_world_voxels(
        1,
        &world,
        positions.to_vec(),
        BodyLimits::default(),
    )
    .unwrap();
    let (properties, report) = MassProperties::from_world(&world, &positions).unwrap();
    assert_eq!(body.mass_properties, properties);
    assert_eq!(report.leaves, 2);
    equal(properties.mass_kg(), 8500, 1);
    equal(
        properties.center_of_mass_mm()[0],
        -650 * 1500 - 7850 * 500,
        8500,
    );
    assert_eq!(
        body.center_of_mass_mm.x,
        i64::try_from(properties.center_of_mass_mm()[0].rounded_integer()).unwrap()
    );
    assert_eq!(body.mass_kg, 8500);
    let promoted = RefinedWorld::from_uniform(&world).unwrap();
    assert_eq!(
        MassProperties::from_world(&promoted, &positions).unwrap().0,
        properties
    );
}

#[test]
fn cube_slab_hole_and_smallest_fragment_keep_exact_nonzero_mass() {
    let origin = IVec3::new(0, 0, 0);
    let cube = page(
        RefinedVolume::uniform(Voxel::new(Material::Concrete)),
        origin,
    );
    let (properties, _) = MassProperties::from_world(cube.world(), &[origin]).unwrap();
    equal(properties.mass_kg(), 2400, 1);
    for center in properties.center_of_mass_mm() {
        equal(center, 500, 1);
    }
    let tensor = properties.inertia_about_rounded_center().unwrap();
    for inertia in &tensor.entries_kg_m2()[..3] {
        equal(*inertia, 400, 1);
    }
    for inertia in &tensor.entries_kg_m2()[3..] {
        equal(*inertia, 0, 1);
    }

    let slab = page(
        shape([0, 0, 0], [256, 1, 256], Voxel::new(Material::Steel)),
        origin,
    );
    let (slab_mass, _) = MassProperties::from_world(slab.world(), &[origin]).unwrap();
    equal(slab_mass.mass_kg(), 7850, 256);
    let inertia = slab_mass
        .inertia_about_rounded_center()
        .unwrap()
        .entries_kg_m2();
    equal(inertia[0], 7850 * (65536 + 1), 256 * 65536 * 12);
    equal(inertia[1], 7850, 256 * 6);
    equal(inertia[2], 7850 * (65536 + 1), 256 * 65536 * 12);

    let (bored, _) = RefinedVolume::uniform(Voxel::new(Material::Wood))
        .replace_box(
            LocalBox::new([64, 0, 64], [192, 256, 192]).unwrap(),
            Voxel::AIR,
            VolumeLimits::default(),
        )
        .unwrap();
    let hole = page(bored, origin);
    let (remaining, _) = MassProperties::from_world(hole.world(), &[origin]).unwrap();
    equal(remaining.mass_kg(), 650 * 3, 4);
    for center in remaining.center_of_mass_mm() {
        equal(center, 500, 1);
    }

    let tiny = page(
        shape(
            [0, 0, 0],
            [1, 1, 1],
            Voxel {
                material: Material::Wood,
                integrity: 0,
            },
        ),
        origin,
    );
    let (tiny_mass, _) = MassProperties::from_world(tiny.world(), &[origin]).unwrap();
    equal(tiny_mass.mass_kg(), 650, 256_i128.pow(3));
    assert!(tiny_mass.mass_kg().numerator() > 0); // No integer-kg rounding to a massless fragment.
    for entry in &tiny_mass
        .inertia_about_rounded_center()
        .unwrap()
        .entries_kg_m2()[..3]
    {
        equal(*entry, 650, 6 * 256_i128.pow(5));
    }
}

#[test]
fn partition_and_integrity_changes_do_not_change_physical_moments() {
    let origin = IVec3::new(-1, 0, 0);
    let mut volume = RefinedVolume::uniform(Voxel::new(Material::Wood));
    for x in 0..8 {
        volume = volume
            .replace_box(
                LocalBox::new([x * 32, 0, 0], [(x + 1) * 32, 256, 256]).unwrap(),
                Voxel {
                    material: Material::Wood,
                    integrity: u8::try_from(x).unwrap(),
                },
                VolumeLimits::default(),
            )
            .unwrap()
            .0;
    }
    let refined = page(volume, origin);
    let uniform = page(RefinedVolume::uniform(Voxel::new(Material::Wood)), origin);
    let fine = MassProperties::from_world(refined.world(), &[origin]).unwrap();
    let coarse = MassProperties::from_world(uniform.world(), &[origin]).unwrap();
    assert!(fine.1.leaves > coarse.1.leaves);
    assert_eq!(fine.0, coarse.0);
    assert_eq!(
        fine.0.inertia_about_rounded_center().unwrap(),
        coarse.0.inertia_about_rounded_center().unwrap()
    );
}

#[test]
fn negative_extreme_translation_and_exact_rounding_are_not_double_rounded() {
    let volume = shape([3, 19, 27], [24, 156, 201], Voxel::new(Material::Steel));
    let mut baseline = None;
    for position in [
        IVec3::new(0, 0, 0),
        IVec3::new(-11, 8, -3),
        IVec3::new(i32::MIN, i32::MIN, i32::MIN),
        IVec3::new(i32::MAX, i32::MAX, i32::MAX),
    ] {
        let state = page(volume.clone(), position);
        let (properties, _) = MassProperties::from_world(state.world(), &[position]).unwrap();
        let tensor = properties.inertia_about_rounded_center().unwrap();
        let center_local = std::array::from_fn::<_, 3, _>(|axis| {
            tensor.pivot_scaled_mm()[axis] - components(position)[axis] * PIVOT_UNITS_PER_METER
        });
        let current = (properties.mass_kg(), center_local, tensor.entries_kg_m2());
        if let Some(expected) = baseline {
            assert_eq!(current, expected);
        } else {
            baseline = Some(current);
        }
    }
    // Independently verify half ties away from zero; direct exact-to-mm rounding is retained.
    for (numerator, denominator, expected) in [
        (1, 2, 1),
        (-1, 2, -1),
        (1, 3, 0),
        (-1, 3, 0),
        (1499, 1000, 1),
        (-1499, 1000, -1),
    ] {
        assert_eq!(
            ExactRatio {
                numerator,
                denominator
            }
            .rounded_integer(),
            expected
        );
    }
}

#[test]
fn seeded_fine_integrals_match_independent_uniform_subcube_oracle_and_checkpoint() {
    let origin = IVec3::new(0, 0, 0);
    let mut seed = 0x4d59_5df4_d0f3_3173_u64;
    for _ in 0..64 {
        let mut volume = RefinedVolume::uniform(Voxel::AIR);
        let mut density = 0_i128;
        let mut centers = [0_i128; 3];
        let mut diagonal = [0_i128; 3];
        let mut products = [0_i128; 3];
        for x in 0..4_u16 {
            for y in 0..4_u16 {
                for z in 0..4_u16 {
                    seed = seed
                        .wrapping_mul(6_364_136_223_846_793_005)
                        .wrapping_add(1_442_695_040_888_963_407);
                    let material = Material::from_wire((seed >> 61) as u8).unwrap();
                    volume = volume
                        .replace_box(
                            LocalBox::new(
                                [x * 64, y * 64, z * 64],
                                [(x + 1) * 64, (y + 1) * 64, (z + 1) * 64],
                            )
                            .unwrap(),
                            Voxel::new(material),
                            VolumeLimits::default(),
                        )
                        .unwrap()
                        .0;
                    let rho = i128::from(material.properties().density_kg_m3);
                    let odd = [x, y, z].map(|value| i128::from(2 * value + 1));
                    density += rho;
                    for axis in 0..3 {
                        centers[axis] += rho * odd[axis];
                        let j = (axis + 1) % 3;
                        let k = (axis + 2) % 3;
                        diagonal[axis] += rho * (3 * (odd[j] * odd[j] + odd[k] * odd[k]) + 2);
                    }
                    for (index, &(i, j)) in SECOND_PAIRS[3..].iter().enumerate() {
                        products[index] -= rho * odd[i] * odd[j];
                    }
                }
            }
        }
        let state = page(volume, origin);
        let properties = MassProperties::from_world(state.world(), &[origin])
            .unwrap()
            .0;
        equal(properties.mass_kg(), density, 64);
        for (index, center) in properties.center_of_mass_mm().into_iter().enumerate() {
            equal(center, centers[index] * 1000, 8 * density);
        }
        let about_origin = properties
            .inertia_about_pivot([0; 3])
            .unwrap()
            .entries_kg_m2();
        for axis in 0..3 {
            equal(about_origin[axis], diagonal[axis], 12288);
            equal(about_origin[axis + 3], products[axis], 4096);
        }
        let restored =
            GeometryState::decode_checkpoint(&state.encode_checkpoint().unwrap()).unwrap();
        assert_eq!(
            MassProperties::from_world(restored.world(), &[origin])
                .unwrap()
                .0,
            properties
        );
    }
}

#[test]
fn checked_selection_bounds_and_pivots_fail_without_world_mutation() {
    let mut world = World::default();
    let positions = [IVec3::new(0, 0, 0), IVec3::new(1, 0, 0)];
    for &position in &positions {
        world.set_voxel(position, Voxel::new(Material::Wood));
    }
    assert_eq!(
        MassProperties::from_world(&world, &[]),
        Err(MassPropertyError::Empty)
    );
    assert_eq!(
        MassProperties::from_world(&world, &vec![positions[0]; MAX_MASS_CELLS + 1]),
        Err(MassPropertyError::CellBudget)
    );
    assert_eq!(
        MassProperties::from_world(&world, &[positions[0]; 2]),
        Err(MassPropertyError::NonCanonicalPositions)
    );
    assert_eq!(
        MassProperties::from_world(&world, &[positions[1], positions[0]]),
        Err(MassPropertyError::NonCanonicalPositions)
    );
    assert_eq!(
        MassProperties::from_world(&world, &[IVec3::new(7, 0, 0)]),
        Err(MassPropertyError::EmptyCell(IVec3::new(7, 0, 0)))
    );
    assert_eq!(
        MassProperties::from_world(&world, &[positions[0], IVec3::new(16_384, 0, 0)]),
        Err(MassPropertyError::Extent)
    );
    let properties = MassProperties::from_world(&world, &positions).unwrap().0;
    for pivot in [[-1, 0, 0], [i64::MIN; 3], [i64::MAX; 3], [0, 256_001, 0]] {
        assert_eq!(
            properties.inertia_about_pivot(pivot),
            Err(MassPropertyError::PivotBounds)
        );
    }
    equal(
        MassProperties::from_world(&world, &positions)
            .unwrap()
            .0
            .mass_kg(),
        1300,
        1,
    );
}

#[test]
fn maximum_accepted_extent_and_mass_remain_finite_without_saturation() {
    let mut world = World::default();
    let positions: Vec<_> = (0..i32::try_from(MAX_MASS_CELLS).unwrap())
        .map(|x| IVec3::new(x, 0, 0))
        .collect();
    for &position in &positions {
        world.set_voxel(position, Voxel::new(Material::Steel));
    }
    let (properties, report) = MassProperties::from_world(&world, &positions).unwrap();
    assert_eq!(report.leaves, MAX_MASS_CELLS);
    equal(properties.mass_kg(), 7850 * MAX_MASS_CELLS as i128, 1);
    let tensor = properties.inertia_about_rounded_center().unwrap();
    equal(tensor.entries_kg_m2()[0], 7850 * MAX_MASS_CELLS as i128, 6);
    assert!(tensor.entries_kg_m2()[1].numerator() > 0);
}

#[test]
fn worst_case_u16_density_products_fit_and_zero_density_refuses_before_mutation() {
    // Adversarial mathematical bound, including a density above any current material table entry.
    // These 16,384 disjoint diagonal cells span the maximum extent on every axis.
    let mut properties = MassProperties {
        minimum: IVec3::new(0, 0, 0),
        maximum: IVec3::new(16_383, 16_383, 16_383),
        weight: 0,
        first: [0; 3],
        second: [0; 6],
    };
    let before = properties.clone();
    assert_eq!(
        properties.add_box([0; 3], LocalBox::FULL, 0),
        Err(MassPropertyError::ZeroDensity)
    );
    assert_eq!(properties, before);
    for cell in 0..MAX_MASS_CELLS {
        properties
            .add_box(
                [i128::try_from(cell).unwrap() * 256; 3],
                LocalBox::FULL,
                u16::MAX,
            )
            .unwrap();
    }
    equal(properties.mass_kg(), i128::from(u16::MAX) * 16_384, 1);
    for pivot in [[0; 3], [MAX_MASS_EXTENT_CELLS * PIVOT_UNITS_PER_METER; 3]] {
        let tensor = properties.inertia_about_pivot(pivot).unwrap();
        assert!(tensor.entries_kg_m2()[0].numerator() > 0);
        assert!(tensor.entries_kg_m2()[3].numerator() < 0);
        assert_eq!(tensor.entries_kg_m2()[0], tensor.entries_kg_m2()[1]);
    }
    assert_eq!(
        properties.inertia_about_pivot([MAX_MASS_EXTENT_CELLS * PIVOT_UNITS_PER_METER + 1; 3]),
        Err(MassPropertyError::PivotBounds)
    );
}

fn max_leaf_volume() -> RefinedVolume {
    let mut encoded = b"DFVL\x02".to_vec();
    encoded.extend_from_slice(&8192_u32.to_le_bytes());
    for z in 0..64_u16 {
        for x in 0..128_u16 {
            for end in [(x + 1) * 2, 256, (z + 1) * 4] {
                encoded.extend_from_slice(&end.to_le_bytes());
            }
            // Integrity partitions preserve equal density but prevent merging the physical boxes.
            encoded.extend_from_slice(&[Material::Wood as u8, u8::try_from((x + z) % 2).unwrap()]);
        }
    }
    RefinedVolume::decode(&encoded).unwrap()
}

#[test]
fn fine_leaf_budget_accepts_exact_limit_and_refuses_one_more_without_fallback() {
    let cell = GeometryCell::refined(max_leaf_volume());
    let (properties, report) = MassProperties::build(16, |index| {
        (
            IVec3::new(i32::try_from(index).unwrap(), 0, 0),
            cell.clone(),
        )
    })
    .unwrap();
    assert_eq!(report.leaves, MAX_MASS_LEAVES);
    equal(properties.mass_kg(), 650 * 16, 1);
    let attempt = MassProperties::build(17, |index| {
        (
            IVec3::new(i32::try_from(index).unwrap(), 0, 0),
            if index < 16 {
                cell.clone()
            } else {
                GeometryCell::uniform(Voxel::new(Material::Wood))
            },
        )
    });
    assert_eq!(attempt, Err(MassPropertyError::LeafBudget));
    let coarse = MassProperties::build(16, |index| {
        (
            IVec3::new(i32::try_from(index).unwrap(), 0, 0),
            GeometryCell::uniform(Voxel::new(Material::Wood)),
        )
    })
    .unwrap()
    .0;
    assert_eq!(properties, coarse);
}

#[test]
fn uniform_centres_match_legacy_rounding_including_signed_half_millimetres() {
    // Four unlike densities have a centroid exactly on a half millimetre: local x=2912.5mm.
    // Compare directly with an independent whole-cell legacy sum, never through the fine pivot.
    for origin in [-100_i32, -14, -1, 0, 100] {
        let voxels: Vec<_> = [
            Material::Wood,
            Material::Soil,
            Material::Brick,
            Material::Steel,
        ]
        .into_iter()
        .enumerate()
        .map(|(x, material)| BodyVoxel {
            position: IVec3::new(origin + i32::try_from(x).unwrap(), 0, 0),
            voxel: Voxel::new(material),
        })
        .collect();
        let properties = MassProperties::from_body_voxels(&voxels).unwrap();
        let weighted: i128 = voxels
            .iter()
            .map(|voxel| {
                (i128::from(voxel.position.x) * 1000 + 500)
                    * i128::from(voxel.voxel.material.properties().density_kg_m3)
            })
            .sum();
        let mass: i128 = voxels
            .iter()
            .map(|voxel| i128::from(voxel.voxel.material.properties().density_kg_m3))
            .sum();
        let legacy = weighted.signum() * ((weighted.abs() + mass / 2) / mass);
        assert_eq!((weighted.abs() * 2) % (mass * 2), mass);
        assert_eq!(properties.center_of_mass_mm()[0].rounded_integer(), legacy);
    }
}

#[test]
fn fine_pivot_half_ties_quantize_in_absolute_world_space_then_subtract_origin() {
    let mut volume = RefinedVolume::uniform(Voxel::AIR);
    for (x, material) in [
        Material::Wood,
        Material::Soil,
        Material::Brick,
        Material::Steel,
    ]
    .into_iter()
    .enumerate()
    {
        let x = u16::try_from(x).unwrap();
        volume = volume
            .replace_box(
                LocalBox::new([x, 0, 0], [x + 1, 256, 256]).unwrap(),
                Voxel::new(material),
                VolumeLimits::default(),
            )
            .unwrap()
            .0;
    }
    for (x, expected) in [(0, 2913), (-1, -253_088)] {
        let position = IVec3::new(x, 0, 0);
        let state = page(volume.clone(), position);
        let properties = MassProperties::from_world(state.world(), &[position])
            .unwrap()
            .0;
        let center = properties.center_of_mass_mm()[0];
        equal(center, i128::from(x) * 1000 * 512 + 5825, 512);
        assert_eq!(
            properties
                .inertia_about_rounded_center()
                .unwrap()
                .pivot_scaled_mm()[0],
            expected
        );
    }
}

#[test]
fn body_import_enforces_hard_cap_before_world_lookup_or_connectivity_work() {
    assert_eq!(BodyLimits::default().max_voxels, MAX_MASS_CELLS);
    let positions: Vec<_> = (0..=i32::try_from(MAX_MASS_CELLS).unwrap())
        .map(|x| IVec3::new(x, 0, 0))
        .collect();
    let limits = BodyLimits { max_voxels: 20_000 };
    let expected = Err(crate::BodyError::TooManyVoxels(MAX_MASS_CELLS + 1));
    assert_eq!(
        RigidBodyDescriptor::from_world_voxels(1, &World::default(), positions.clone(), limits),
        expected
    );
    let voxels = positions
        .into_iter()
        .map(|position| BodyVoxel {
            position,
            voxel: Voxel::new(Material::Wood),
        })
        .collect();
    assert_eq!(
        RigidBodyDescriptor::from_replicated_voxels(1, voxels, limits),
        expected
    );
}

#[test]
fn every_current_solid_material_has_nonzero_mass_even_at_zero_integrity() {
    for id in 1..=7 {
        let material = Material::from_wire(id).unwrap();
        assert!(material.properties().density_kg_m3 > 0);
        let body = RigidBodyDescriptor::from_replicated_voxels(
            1,
            vec![BodyVoxel {
                position: IVec3::new(0, 0, 0),
                voxel: Voxel {
                    material,
                    integrity: 0,
                },
            }],
            BodyLimits::default(),
        )
        .unwrap();
        assert_eq!(body.mass_kg, u64::from(material.properties().density_kg_m3));
    }
}
