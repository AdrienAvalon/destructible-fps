use super::*;

#[test]
fn clamped_end_bending_can_sever_the_adjacent_free_cell() {
    use crate::{
        Voxel,
        elasticity::{ElasticJob, ElasticModel, ElasticNode, ElasticOptions, ElasticProgress},
    };
    let mut world = World::default();
    let positions = [IVec3::new(0, 4, 0), IVec3::new(1, 4, 0)];
    for position in positions {
        world.set_voxel(position, Voxel::new(Material::Wood));
    }
    let nodes: Vec<_> = positions
        .into_iter()
        .enumerate()
        .map(|(index, position)| ElasticNode {
            position,
            young_modulus_pa: 1e9,
            poisson_ratio: 0.25,
            mass_kg: 0.0,
            integrity: 255,
            fixed: index == 0,
            load: if index == 0 {
                [0.0; 6]
            } else {
                [0.0, -100.0, 0.0, 0.0, 0.0, 0.0]
            },
        })
        .collect();
    let mut job = ElasticJob::new(
        ElasticModel::new(&nodes, 1.0, [0.0; 3]).unwrap(),
        ElasticOptions::default(),
    )
    .unwrap();
    while job.advance(8).unwrap() == ElasticProgress::Pending {}
    let result = job.finish().unwrap();
    assert!(result.bond_end_forces[0][1][5].abs() < 1e-8);
    assert!((result.bond_end_forces[0][0][5].abs() - 100.0).abs() < 1e-8);
    let anchors = StructuralAnchors::default()
        .with_explicit([positions[0]])
        .unwrap();
    let strengths = StructuralStrengths::new(
        [SectionStrength {
            tension_pa: 500.0,
            compression_pa: 500.0,
            shear_pa: 500.0,
        }; 7],
    )
    .unwrap();
    let selected = select_failure(
        &world,
        &anchors,
        &result,
        strengths,
        &AtomicBool::new(false),
    )
    .unwrap()
    .unwrap();
    assert_eq!(selected.position, positions[1]);
    assert_eq!(selected.mode, FailureMode::Tension);
    assert!((1199..=1200).contains(&selected.utilization_per_mille));
    world.set_voxel(positions[0], Voxel::new(Material::Stone));
    let mut profiles = [SectionStrength {
        tension_pa: 1e6,
        compression_pa: 1e6,
        shear_pa: 1e6,
    }; 7];
    profiles[1] = SectionStrength {
        tension_pa: 1.0,
        compression_pa: 1.0,
        shear_pa: 1.0,
    };
    assert!(
        select_failure(
            &world,
            &anchors,
            &result,
            StructuralStrengths::new(profiles).unwrap(),
            &AtomicBool::new(false)
        )
        .unwrap()
        .is_none(),
        "this clamped-domain policy must not invent a complete foundation-bearing assessment"
    );
}

#[test]
fn strengths_and_section_inputs_fail_closed_and_quantization_has_an_explicit_deadband() {
    for value in [f64::NAN, f64::INFINITY, -1.0, 0.0, 1e13] {
        for field in 0..3 {
            let mut strength = SectionStrength {
                tension_pa: 100.0,
                compression_pa: 100.0,
                shear_pa: 100.0,
            };
            match field {
                0 => strength.tension_pa = value,
                1 => strength.compression_pa = value,
                _ => strength.shear_pa = value,
            }
            assert!(matches!(
                StructuralStrengths::new([strength; 7]),
                Err(StructuralFailureError::InvalidStrength)
            ));
        }
    }
    for axis in 0..3 {
        assert!(demand(axis, 0, [0.0; 6], 1.0, 0).is_err());
        assert!(demand(axis, 0, [f64::NAN; 6], 1.0, 255).is_err());
        assert!(demand(axis, 0, [1e308; 6], 0.01, 1).is_err());
        assert!(demand(axis, 0, [1.0; 6], 1.0, 1).is_ok());
    }
    assert_eq!(quantize_ratio(1.0), 1000);
    assert_eq!(quantize_ratio(1.000_9), 1000);
    assert_eq!(quantize_ratio(1.001_1), 1001);
    assert_eq!(quantize_ratio(1e300), u32::MAX);
}

#[test]
fn all_material_strength_slots_are_distinct_and_air_is_not_a_section() {
    let profiles = StructuralStrengths::new(std::array::from_fn(|index| SectionStrength {
        tension_pa: f64::from(u32::try_from(index + 1).unwrap()),
        compression_pa: 100.0,
        shear_pa: 100.0,
    }))
    .unwrap();
    for id in 1..=7 {
        assert_eq!(
            profiles
                .for_material(Material::from_wire(id).unwrap())
                .unwrap()
                .tension_pa
                .to_bits(),
            f64::from(id).to_bits()
        );
    }
    assert!(profiles.for_material(Material::Air).is_err());
}

#[test]
fn square_torsion_matches_published_section_example() {
    // AutoFEM's square: 50 mm side, 1000 N.m torque -> 38.462 MPa peak shear.
    let result = demand(0, 1, [0.0, 0.0, 0.0, 1000.0, 0.0, 0.0], 0.05, 255).unwrap();
    assert!((result.shear - 38_461_538.461_538_46).abs() < 1e-6);
}

#[test]
fn signed_axial_bending_shear_and_section_loss_hold_on_every_axis() {
    for axis in 0..3 {
        for end in 0..2 {
            for sign in [-1.0, 1.0] {
                let mut forces = [0.0; 6];
                forces[axis] = 100.0 * sign * if end == 0 { -1.0 } else { 1.0 };
                let result = demand(axis, end, forces, 1.0, 255).unwrap();
                assert_eq!(
                    result.tension.to_bits(),
                    (100.0_f64 * sign).max(0.0).to_bits()
                );
                assert_eq!(
                    result.compression.to_bits(),
                    (-100.0_f64 * sign).max(0.0).to_bits()
                );
                let damaged = demand(axis, end, forces, 1.0, 85).unwrap();
                assert!((damaged.tension + damaged.compression - 300.0).abs() < 1e-10);
            }
            let [u, v] = [(axis + 1) % 3, (axis + 2) % 3];
            let mut forces = [0.0; 6];
            forces[u + 3] = 10.0;
            forces[v + 3] = -20.0;
            forces[u] = 3.0;
            forces[v] = 4.0;
            let result = demand(axis, end, forces, 1.0, 255).unwrap();
            assert!((result.tension - 180.0).abs() < 1e-10);
            assert!((result.compression - 180.0).abs() < 1e-10);
            assert!((result.shear - 7.5).abs() < 1e-10);
        }
    }
}
