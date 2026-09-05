use super::*;

fn node(position: IVec3, fixed: bool) -> ElasticNode {
    ElasticNode {
        position,
        young_modulus_pa: 1e9,
        poisson_ratio: 0.25,
        mass_kg: 0.0,
        integrity: 255,
        fixed,
        load: [0.0; 6],
    }
}

fn close(actual: f64, expected: f64) {
    assert!(
        (actual - expected).abs() <= 1e-7 * expected.abs().max(1e-7),
        "{actual} != {expected}"
    );
}

fn solve(
    mut nodes: Vec<ElasticNode>,
    length: f64,
    gravity: [f64; 3],
    step: usize,
) -> ElasticSolution {
    nodes.sort_by_key(|node| node.position);
    let model = ElasticModel::new(&nodes, length, gravity).expect("valid model");
    let mut job = ElasticJob::new(model, ElasticOptions::default()).expect("valid solver");
    while job.advance(step).expect("converged equilibrium") == ElasticProgress::Pending {}
    job.finish().expect("complete solution")
}

#[test]
fn beam_rigid_translation_and_rotation_have_zero_internal_force_on_all_axes() {
    let nodes = [
        node(IVec3::new(0, 0, 0), true),
        node(IVec3::new(1, 0, 0), false),
    ];
    for axis in 0..3 {
        for length in [0.25, 1.0, 3.0] {
            let beam = Beam::new([0, 1], axis, &nodes, length);
            let angular = glam::DVec3::new(0.002, -0.003, 0.004);
            let mut offset = glam::DVec3::ZERO;
            offset[axis] = length;
            let displacement = angular.cross(offset);
            let left = [0.1, 0.2, -0.3, angular.x, angular.y, angular.z];
            let mut right = left;
            for coordinate in 0..3 {
                right[coordinate] += displacement[coordinate];
            }
            assert!(
                beam.forces(left, right)
                    .iter()
                    .flatten()
                    .all(|force| force.abs() < 1e-5)
            );
        }
    }
}

#[test]
fn beam_matrix_is_symmetric_and_its_diagonal_matches_basis_actions() {
    let nodes = [
        node(IVec3::new(0, 0, 0), true),
        node(IVec3::new(1, 0, 0), false),
    ];
    for axis in 0..3 {
        let beam = Beam::new([0, 1], axis, &nodes, 0.3);
        let mut matrix = [[0.0; 12]; 12];
        for column in 0..12 {
            let mut displacement = [[0.0; 6]; 2];
            displacement[column / 6][column % 6] = 1.0;
            for (row, force) in beam
                .forces(displacement[0], displacement[1])
                .into_iter()
                .flatten()
                .enumerate()
            {
                matrix[row][column] = force;
            }
        }
        for (row, values) in matrix.iter().enumerate() {
            close(values[row], beam.diagonal()[row % 6]);
            for (column, other) in matrix.iter().enumerate() {
                close(values[column], other[row]);
            }
        }
    }
}

#[test]
fn analytical_axial_bending_and_torsion_hold_in_every_signed_direction() {
    for axis in 0..3 {
        for sign in [-1, 1] {
            for length in [0.25, 1.0, 2.0] {
                let mut position = [0, 0, 0];
                position[axis] = sign;
                let position = IVec3::new(position[0], position[1], position[2]);
                for dof in 0..6 {
                    let mut tip = node(position, false);
                    tip.load[dof] = 10.0;
                    let answer = solve(
                        vec![node(IVec3::new(0, 0, 0), true), tip],
                        length,
                        [0.0; 3],
                        1,
                    );
                    let index = usize::from(sign > 0);
                    let inertia = length.powi(4) / 12.0;
                    let expected = if dof == axis {
                        10.0 * length / (1e9 * length.powi(2))
                    } else if dof < 3 {
                        10.0 * length.powi(3) / (3.0 * 1e9 * inertia)
                            + 10.0 * length / ((5.0 / 6.0) * 4e8 * length.powi(2))
                    } else if dof == axis + 3 {
                        10.0 * length / (4e8 * 2.0 * inertia)
                    } else {
                        10.0 * length / (1e9 * inertia)
                    };
                    close(answer.displacements[index][dof], expected);
                    close(answer.reactions[1 - index][dof], -10.0);
                }
            }
        }
    }
}

#[test]
fn gravity_and_anchor_reactions_balance_force_and_eccentric_moment() {
    let mut root = node(IVec3::new(0, 0, 0), true);
    root.mass_kg = 20.0;
    let mut tip = node(IVec3::new(1, 0, 0), false);
    tip.mass_kg = 30.0;
    let result = solve(vec![root, tip], 2.0, [0.0, -10.0, 0.0], 2);
    close(result.reactions[0][1], 500.0);
    close(result.reactions[0][5], 600.0);
    assert!(result.residual_n < 1e-5);
}

#[test]
fn heterogeneous_three_dimensional_lattice_balances_all_forces_moments_and_work() {
    let gravity = glam::DVec3::new(1.0, -9.81, 0.5);
    for length in [0.25, 1.0, 2.0] {
        let mut nodes = Vec::new();
        for x in -1..=1 {
            for y in 0..4 {
                for z in -1..=1 {
                    let mut node = node(IVec3::new(x, y, z), y == 0);
                    node.mass_kg = f64::from(5 + x + z);
                    node.young_modulus_pa = 1e8 * f64::from(2 + y);
                    node.integrity = u8::try_from(192 + 16 * x + 8 * z).unwrap();
                    node.load = [
                        f64::from(13 * x),
                        10.0,
                        f64::from(-7 * z),
                        f64::from(1 + x),
                        f64::from(2 - z),
                        f64::from(3 + y),
                    ];
                    nodes.push(node);
                }
            }
        }
        nodes.sort_by_key(|node| node.position);
        let solution = solve(nodes.clone(), length, gravity.to_array(), 8);
        let mut net_force = glam::DVec3::ZERO;
        let mut net_moment = glam::DVec3::ZERO;
        let mut external_work = 0.0;
        for (index, node) in nodes.iter().enumerate() {
            let reaction = solution.reactions[index];
            let force = glam::DVec3::from_slice(&node.load) + node.mass_kg * gravity;
            let moment = glam::DVec3::from_slice(&node.load[3..]);
            let force_balance = force + glam::DVec3::from_slice(&reaction);
            let position = glam::DVec3::new(
                f64::from(node.position.x),
                f64::from(node.position.y),
                f64::from(node.position.z),
            ) * length;
            net_force += force_balance;
            net_moment +=
                moment + glam::DVec3::from_slice(&reaction[3..]) + position.cross(force_balance);
            external_work += glam::DVec3::from_slice(&solution.displacements[index]).dot(force)
                + glam::DVec3::from_slice(&solution.displacements[index][3..]).dot(moment);
        }
        assert!(net_force.length() < 1e-3, "unbalanced force: {net_force}");
        assert!(
            net_moment.length() < 1e-3,
            "unbalanced moment: {net_moment}"
        );
        let internal_work: f64 = solution
            .bond_ends
            .iter()
            .zip(&solution.bond_end_forces)
            .map(|(ends, forces)| {
                dot(
                    &[
                        solution.displacements[ends[0]],
                        solution.displacements[ends[1]],
                    ],
                    forces,
                )
            })
            .sum();
        assert!(internal_work > 0.0);
        close(internal_work, external_work);
    }
}

#[test]
fn alternate_support_removal_redistributes_a_still_connected_structures_load() {
    let mut middle = node(IVec3::new(1, 0, 0), false);
    middle.load[1] = -1_000.0;
    let first = node(IVec3::new(0, 0, 0), true);
    let last = node(IVec3::new(2, 0, 0), true);
    let intact = solve(vec![first, middle, last], 1.0, [0.0; 3], 4);
    close(intact.reactions[0][1], 500.0);
    close(intact.reactions[2][1], 500.0);
    let remaining = solve(vec![first, middle], 1.0, [0.0; 3], 4);
    close(remaining.reactions[0][1], 1_000.0);
    close(remaining.reactions[0][5], 1_000.0);
    assert!(remaining.displacements[1][1].abs() > intact.displacements[1][1].abs());
}

#[test]
fn damaged_section_changes_stiffness_without_losing_supported_mass() {
    let root = node(IVec3::new(0, 0, 0), true);
    let mut tip = node(IVec3::new(1, 0, 0), false);
    tip.mass_kg = 100.0;
    let intact = solve(vec![root, tip], 1.0, [0.0, -10.0, 0.0], 4);
    tip.integrity = 128;
    let damaged = solve(vec![root, tip], 1.0, [0.0, -10.0, 0.0], 4);
    close(damaged.reactions[0][1], intact.reactions[0][1]);
    close(
        damaged.displacements[1][1] / intact.displacements[1][1],
        3e-9_f64.mul_add(255.0 / 128.0, 4e-9 * (255.0_f64 / 128.0).powi(2)) / 7e-9,
    );
}

#[test]
fn disparate_materials_have_series_compliance_not_strongest_endpoint_stiffness() {
    let mut root = node(IVec3::new(0, 0, 0), true);
    root.young_modulus_pa = 1e6;
    let mut tip = node(IVec3::new(1, 0, 0), false);
    tip.load[0] = 1.0;
    let answer = solve(vec![root, tip], 1.0, [0.0; 3], 1);
    close(answer.displacements[1][0], 0.5 / 1e6 + 0.5 / 1e9);
}

#[test]
fn invalid_and_unanchored_domains_fail_before_iteration() {
    let valid = node(IVec3::new(0, 0, 0), true);
    for invalid in [
        ElasticNode {
            young_modulus_pa: f64::NAN,
            ..valid
        },
        ElasticNode {
            poisson_ratio: 0.5,
            ..valid
        },
        ElasticNode {
            mass_kg: -1.0,
            ..valid
        },
        ElasticNode {
            integrity: 0,
            ..valid
        },
        ElasticNode {
            load: [f64::INFINITY; 6],
            ..valid
        },
    ] {
        assert!(matches!(
            ElasticModel::new(&[invalid], 1.0, [0.0; 3]),
            Err(ElasticError::InvalidInput)
        ));
    }
    assert!(matches!(
        ElasticModel::new(&[valid, valid], 1.0, [0.0; 3]),
        Err(ElasticError::NonCanonicalNodes)
    ));
    assert!(matches!(
        ElasticModel::new(&vec![valid; MAX_ELASTIC_NODES + 1], 1.0, [0.0; 3]),
        Err(ElasticError::TooManyNodes)
    ));
    assert!(matches!(
        ElasticModel::new(&[valid, node(IVec3::new(2, 0, 0), false)], 1.0, [0.0; 3]),
        Err(ElasticError::UnanchoredComponent)
    ));
}

#[test]
fn budget_failure_and_incomplete_jobs_never_expose_a_solution() {
    let mut tip = node(IVec3::new(1, 0, 0), false);
    tip.load[1] = 10.0;
    let nodes = [node(IVec3::new(0, 0, 0), true), tip];
    let model = ElasticModel::new(&nodes, 1.0, [0.0; 3]).unwrap();
    let job = ElasticJob::new(model, ElasticOptions::default()).unwrap();
    assert!(matches!(job.finish(), Err(ElasticError::NotConverged)));
    let mut job = ElasticJob::new(
        ElasticModel::new(&nodes, 1.0, [0.0; 3]).unwrap(),
        ElasticOptions {
            max_iterations: 1,
            ..ElasticOptions::default()
        },
    )
    .unwrap();
    assert_eq!(job.advance(0), Err(ElasticError::InvalidStepBudget));
    assert_eq!(
        job.advance(MAX_ELASTIC_STEP + 1),
        Err(ElasticError::InvalidStepBudget)
    );
    assert_eq!(job.advance(1), Err(ElasticError::IterationLimit));
    assert_eq!(job.advance(1), Err(ElasticError::IterationLimit));
    assert!(matches!(job.finish(), Err(ElasticError::IterationLimit)));
}

#[test]
fn linear_model_refuses_large_deformation_instead_of_claiming_realistic_collapse() {
    let mut tip = node(IVec3::new(1, 0, 0), false);
    tip.load[1] = 1e9;
    let model = ElasticModel::new(&[node(IVec3::new(0, 0, 0), true), tip], 1.0, [0.0; 3]).unwrap();
    let mut job = ElasticJob::new(model, ElasticOptions::default()).unwrap();
    assert_eq!(job.advance(32), Err(ElasticError::OutsideLinearRegime));
    assert!(matches!(
        job.finish(),
        Err(ElasticError::OutsideLinearRegime)
    ));
}

#[test]
fn solver_batching_does_not_change_the_result_or_iteration_order() {
    let mut nodes: Vec<_> = (0..8).map(|x| node(IVec3::new(x, 0, 0), x == 0)).collect();
    nodes[7].load[1] = -10.0;
    let single = solve(nodes.clone(), 1.0, [0.0; 3], 1);
    let batched = solve(nodes, 1.0, [0.0; 3], 32);
    assert_eq!(single.iterations, batched.iterations);
    let bits = |solution: &ElasticSolution| {
        solution
            .displacements
            .iter()
            .flatten()
            .map(|value| value.to_bits())
            .collect::<Vec<_>>()
    };
    assert_eq!(bits(&single), bits(&batched));
    close(
        single.displacements[7][1],
        -10.0 * 7.0_f64.powi(3) / (3.0 * 1e9 / 12.0) - 10.0 * 7.0 / ((5.0 / 6.0) * 4e8),
    );
}

#[test]
fn minimal_nonzero_integrity_remains_finite_under_a_small_load() {
    let mut tip = node(IVec3::new(1, 0, 0), false);
    tip.integrity = 1;
    tip.load[1] = -0.01;
    let answer = solve(vec![node(IVec3::new(0, 0, 0), true), tip], 1.0, [0.0; 3], 2);
    close(
        answer.displacements[1][1],
        -0.01 * 4e-9_f64.mul_add(255.0_f64.powi(2), 3e-9 * 255.0),
    );
    close(answer.reactions[0][1], 0.01);
}

#[test]
fn exact_node_cap_and_extreme_integer_positions_do_not_alias_neighbors() {
    let nodes: Vec<_> = (0..4096).map(|x| node(IVec3::new(x, 0, 0), true)).collect();
    let model = ElasticModel::new(&nodes, 1.0, [0.0; 3]).unwrap();
    assert_eq!(model.node_count(), MAX_ELASTIC_NODES);
    assert_eq!(model.beam_count(), MAX_ELASTIC_NODES - 1);
    for position in [i32::MIN, i32::MAX] {
        let model = ElasticModel::new(
            &[node(IVec3::new(position, position, position), true)],
            1.0,
            [0.0; 3],
        )
        .unwrap();
        assert_eq!(model.beam_count(), 0);
        let result = ElasticJob::new(model, ElasticOptions::default())
            .unwrap()
            .finish()
            .unwrap();
        assert_eq!(result.iterations, 0);
    }
}

#[test]
fn solver_rejects_nonfinite_options_and_out_of_contract_global_budgets() {
    for options in [
        ElasticOptions {
            max_iterations: 0,
            ..ElasticOptions::default()
        },
        ElasticOptions {
            max_iterations: MAX_ELASTIC_ITERATIONS + 1,
            ..ElasticOptions::default()
        },
        ElasticOptions {
            relative_tolerance: f64::NAN,
            ..ElasticOptions::default()
        },
        ElasticOptions {
            absolute_tolerance_n: 0.0,
            ..ElasticOptions::default()
        },
    ] {
        let model = ElasticModel::new(&[node(IVec3::new(0, 0, 0), true)], 1.0, [0.0; 3]).unwrap();
        assert!(matches!(
            ElasticJob::new(model, options),
            Err(ElasticError::InvalidOptions)
        ));
    }
}
