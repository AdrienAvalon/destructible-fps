use super::*;
use crate::{IVec3, MICROMETERS_PER_VOXEL, Voxel};

fn origin(x: i64, y: i64, z: i64) -> FixedMicrometers3 {
    FixedMicrometers3 { x, y, z }
}

#[test]
fn axial_negative_and_boundary_rays_cover_exact_distances_without_duplicates() {
    for sign in [-1_i16, 1] {
        let ray = FixedRay::new(origin(0, 500_000, 500_000), [sign, 0, 0], 3_000_000).unwrap();
        let cells = ray.cells().unwrap();
        assert_eq!(cells.len(), 3);
        for (index, cell) in cells.iter().enumerate() {
            assert_eq!(
                cell.position.x,
                if sign > 0 {
                    i32::try_from(index).unwrap()
                } else {
                    -i32::try_from(index).unwrap() - 1
                }
            );
            assert_eq!(
                cell.exit_um - cell.enter_um,
                MICROMETERS_PER_VOXEL.cast_unsigned()
            );
        }
        assert_eq!(cells.last().unwrap().exit_um, ray.length_um());
    }
}

#[test]
fn ray_bounds_magnitude_and_malformed_inputs_are_explicit() {
    for direction in [
        [1, 1, 1],
        [32767, 32767, 32767],
        [i16::MIN, 1, 0],
        [1, 0, 0],
        [1, 1, 0],
    ] {
        let ray = FixedRay::new(origin(0, 0, 0), direction, MAX_RIFLE_RANGE_UM).unwrap();
        assert!(ray.length_um() <= MAX_RIFLE_RANGE_UM);
        let cells = ray.cells().unwrap();
        assert!(cells.len() <= ray::MAX_RAY_CELLS);
        assert_eq!(
            cells
                .iter()
                .map(|cell| cell.exit_um - cell.enter_um)
                .sum::<u64>(),
            ray.length_um()
        );
    }
    assert!(FixedRay::new(origin(i64::MAX, 0, 0), [1, 0, 0], 1).is_err());
    assert!(FixedRay::new(origin(0, 0, 0), [0, 0, 0], 100).is_err());
    assert!(FixedRay::new(origin(0, 0, 0), [1, 0, 0], MAX_RIFLE_RANGE_UM + 1).is_err());
}

#[test]
fn maximum_parallel_diagonal_cover_contacts_fit_the_visit_budget() {
    for direction in [[1, 1, 0], [-1, 1, 0], [1, -1, 0], [-1, -1, 0]] {
        let ray =
            FixedRay::new(origin(500_000, 500_000, 0), direction, MAX_RIFLE_RANGE_UM).unwrap();
        let cells = ray.cells().unwrap();
        assert!(cells.len() <= ray::MAX_RAY_CELLS);
        assert_eq!(
            cells
                .iter()
                .map(|cell| cell.exit_um - cell.enter_um)
                .sum::<u64>(),
            ray.length_um()
        );
    }
}

#[test]
fn exact_diagonal_and_parallel_seams_occlude_without_free_cell_damage() {
    for (start, direction, blocker) in [
        (
            origin(500_000, 500_000, 500_000),
            [1, 1, 0],
            IVec3::new(1, 0, 0),
        ),
        (origin(0, 500_000, 500_000), [0, 1, 0], IVec3::new(-1, 0, 0)),
        (origin(500_000, 500_000, 0), [1, 1, 0], IVec3::new(1, 0, -1)),
        (
            origin(508_465, 999_985, 500_000),
            [32767, 1, 0],
            IVec3::new(0, 1, 0),
        ),
    ] {
        let mut world = World::default();
        world.set_voxel(blocker, Voxel::new(Material::Steel));
        let before = world.fingerprint();
        let report = apply_rifle(&mut world, start, direction, None).unwrap();
        assert_eq!(report.first_hit, Some(blocker));
        assert_eq!(report.spent_energy, RIFLE_ENERGY);
        assert_eq!(report.remaining_energy, 0);
        assert_eq!(world.fingerprint(), before);
    }
}

#[test]
fn repeated_wood_hits_are_localized_and_glass_transmits_remaining_energy() {
    let mut world = World::default();
    let wood = IVec3::new(0, 0, 0);
    let side = IVec3::new(0, 1, 0);
    world.set_voxel(wood, Voxel::new(Material::Wood));
    world.set_voxel(side, Voxel::new(Material::Wood));
    let start = origin(-1_500_000, 500_000, 500_000);
    for shot in 0..3 {
        let report = apply_rifle(&mut world, start, [1, 0, 0], None).unwrap();
        assert_eq!(report.spent_energy + report.remaining_energy, RIFLE_ENERGY);
        assert_eq!(world.voxel(side).integrity, 255);
        assert_eq!(world.voxel(wood).is_solid(), shot < 2);
    }
    world.set_voxel(wood, Voxel::new(Material::Glass));
    world.set_voxel(IVec3::new(1, 0, 0), Voxel::new(Material::Wood));
    let report = apply_rifle(&mut world, start, [1, 0, 0], None).unwrap();
    assert!(!world.voxel(wood).is_solid());
    assert!(world.voxel(IVec3::new(1, 0, 0)).integrity < 255);
    assert_eq!(report.destruction.changes.len(), 2);
    assert_eq!(report.spent_energy, RIFLE_ENERGY);
}

#[test]
fn misses_and_body_cover_obey_the_energy_budget() {
    let mut world = World::default();
    let start = origin(-1_500_000, 500_000, 500_000);
    let miss = apply_rifle(&mut world, start, [1, 0, 0], None).unwrap();
    assert_eq!(miss.remaining_energy, RIFLE_ENERGY);
    world.set_voxel(IVec3::new(0, 0, 0), Voxel::new(Material::Wood));
    let before = world.fingerprint();
    let covered = apply_rifle(&mut world, start, [1, 0, 0], Some((7, 1_000_000))).unwrap();
    assert_eq!(covered.blocked_by_body, Some(7));
    assert_eq!(world.fingerprint(), before);
    let ray = FixedRay::new(start, [1, 0, 0], MAX_RIFLE_RANGE_UM).unwrap();
    assert!(
        (1_499_999..=1_500_000).contains(&ray.box_entry_um([0, 0, 0], [1_000_000; 3]).unwrap())
    );
    assert_eq!(
        ray.box_entry_um([0, 2_000_000, 0], [1_000_000, 3_000_000, 1_000_000]),
        None
    );
}

#[test]
fn starting_inside_material_and_the_finite_endpoint_do_not_skip_cover() {
    let start = origin(500_000, 500_000, 500_000);
    let mut world = World::default();
    world.set_voxel(IVec3::new(0, 0, 0), Voxel::new(Material::Wood));
    let report = apply_rifle(&mut world, start, [1, 0, 0], None).unwrap();
    assert_eq!(report.first_hit, Some(IVec3::new(0, 0, 0)));
    assert_eq!(world.voxel(IVec3::new(0, 0, 0)).integrity, 68);
    let mut distant = World::default();
    distant.set_voxel(IVec3::new(121, 0, 0), Voxel::new(Material::Wood));
    let miss = apply_rifle(&mut distant, start, [1, 0, 0], None).unwrap();
    assert!(miss.first_hit.is_none());
    distant.set_voxel(IVec3::new(120, 0, 0), Voxel::new(Material::Wood));
    let hit = apply_rifle(&mut distant, start, [1, 0, 0], None).unwrap();
    assert_eq!(hit.first_hit, Some(IVec3::new(120, 0, 0)));
    assert_eq!(distant.voxel(IVec3::new(121, 0, 0)).integrity, 255);
}

#[test]
fn visual_aim_quantization_rejects_invalid_values_and_discards_magnitude() {
    use crate::player::quantized_direction;
    use glam::Vec3;
    for direction in [
        Vec3::ZERO,
        Vec3::splat(f32::NAN),
        Vec3::splat(f32::INFINITY),
    ] {
        assert_eq!(quantized_direction(direction), None);
    }
    assert_eq!(quantized_direction(Vec3::NEG_Z), Some([0, 0, -32767]));
    assert_eq!(
        quantized_direction(Vec3::NEG_Z * 100.0),
        Some([0, 0, -32767])
    );
}

#[test]
fn weapon_cadence_reload_and_reserve_are_finite_on_the_simulation_clock() {
    let initial = RifleState::default();
    let mut state = initial.after_shot(10).unwrap();
    assert_eq!(state.magazine, 29);
    assert_eq!(state.after_shot(10), Err(BallisticError::FireTooSoon));
    state = state.after_reload(11).unwrap();
    assert_eq!(state.after_shot(130), Err(BallisticError::Reloading));
    assert_eq!(state.at(131).magazine, 30);
    assert_eq!(state.at(131).reserve, 89);
    assert_eq!(state.after_shot(131).unwrap().magazine, 29);
    assert_eq!(
        initial.after_reload(0),
        Err(BallisticError::ReloadUnavailable)
    );
    assert_eq!(
        initial.after_shot(u64::MAX),
        Err(BallisticError::ClockExhausted)
    );
}

#[test]
fn material_order_and_oblique_chord_change_remaining_section() {
    let mut residuals = Vec::new();
    for material in [
        Material::Glass,
        Material::Wood,
        Material::Brick,
        Material::Concrete,
        Material::Steel,
    ] {
        let mut world = World::default();
        world.set_voxel(IVec3::new(0, 0, 0), Voxel::new(material));
        let report = apply_rifle(
            &mut world,
            origin(-500_000, 500_000, 500_000),
            [1, 0, 0],
            None,
        )
        .unwrap();
        assert_eq!(report.spent_energy + report.remaining_energy, RIFLE_ENERGY);
        residuals.push(world.voxel(IVec3::new(0, 0, 0)).integrity);
    }
    assert_eq!(residuals[0], 0);
    assert!(residuals.windows(2).all(|pair| pair[0] < pair[1]));
    let mut diagonal = World::default();
    diagonal.set_voxel(IVec3::new(0, 0, 0), Voxel::new(Material::Wood));
    apply_rifle(
        &mut diagonal,
        origin(-500_000, -500_000, 500_000),
        [1, 1, 0],
        None,
    )
    .unwrap();
    assert!(diagonal.voxel(IVec3::new(0, 0, 0)).integrity > residuals[1]);
}

#[test]
fn continuous_wall_on_a_grid_plane_receives_damage_instead_of_becoming_invulnerable() {
    let mut world = World::default();
    world.fill_box(
        IVec3::new(-1, 0, 0),
        IVec3::new(0, 0, 0),
        Voxel::new(Material::Wood),
    );
    let report = apply_rifle(&mut world, origin(0, 500_000, 2_500_000), [0, 0, -1], None).unwrap();
    assert_eq!(report.first_hit, Some(IVec3::new(0, 0, 0)));
    assert_eq!(report.destruction.damaged_voxels, 1);
}

#[test]
fn weapon_state_is_personal_bounded_loss_tolerant_and_not_a_client_command() {
    use state_wire::{WEAPON_STATE_BYTES, WeaponStateInbox, WeaponStatePacket};
    let packet = WeaponStatePacket {
        session_id: 7,
        server_tick: 10,
        last_command_id: 1,
        rifle: RifleState::default().after_shot(10).unwrap(),
    };
    let bytes = packet.encode().unwrap();
    assert_eq!(bytes.len(), WEAPON_STATE_BYTES);
    assert_eq!(WeaponStatePacket::decode(&bytes), Some(packet));
    assert!(crate::decode_client_control(&bytes).is_err());
    for length in 0..bytes.len() {
        assert!(WeaponStatePacket::decode(&bytes[..length]).is_none());
    }
    let mut extra = bytes.clone();
    extra.push(0);
    assert!(WeaponStatePacket::decode(&extra).is_none());
    let mut forged = bytes.clone();
    forged[29] = 255;
    assert!(WeaponStatePacket::decode(&forged).is_none());
    let mut inbox = WeaponStateInbox::default();
    assert!(inbox.receive(7, &bytes).unwrap());
    assert!(!inbox.receive(7, &bytes).unwrap());
    assert!(inbox.receive(8, &bytes).is_err());
    let older = WeaponStatePacket {
        server_tick: 9,
        rifle: RifleState::default(),
        ..packet
    };
    assert!(!inbox.receive(7, &older.encode().unwrap()).unwrap());
    let regressed = WeaponStatePacket {
        server_tick: 11,
        last_command_id: 0,
        ..packet
    };
    assert!(inbox.receive(7, &regressed.encode().unwrap()).is_err());
    assert_eq!(inbox.latest(), Some(packet));
}

#[test]
fn random_fixed_ray_intervals_match_an_independent_brute_force_slab_oracle() {
    let mut seed = 0x7f82_910a_50f3_0001_u64;
    let mut next = || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    for _ in 0..80 {
        let start = std::array::from_fn::<_, 3, _>(|_| {
            i64::try_from(next() % 2_000_000).unwrap() - 1_000_000
        });
        let direction = std::array::from_fn(|_| {
            i16::try_from(i32::try_from(next() % 60_001).unwrap() - 30_000).unwrap()
        });
        let ray =
            FixedRay::new(origin(start[0], start[1], start[2]), direction, 4_000_000).unwrap();
        let actual: std::collections::BTreeSet<_> = ray
            .cells()
            .unwrap()
            .into_iter()
            .filter(|cell| cell.exit_um > cell.enter_um)
            .map(|cell| cell.position)
            .collect();
        let mut expected = std::collections::BTreeSet::new();
        for x in -6..=6 {
            for y in -6..=6 {
                for z in -6..=6 {
                    if positive_slab_interval(&ray, [x, y, z]) {
                        expected.insert(IVec3::new(x, y, z));
                    }
                }
            }
        }
        assert_eq!(
            actual, expected,
            "origin {start:?}, direction {direction:?}"
        );
    }
}

fn positive_slab_interval(ray: &FixedRay, cell: [i32; 3]) -> bool {
    let mut entry = 0.0_f64;
    let mut exit = 1.0_f64;
    for (axis, coordinate) in cell.into_iter().enumerate() {
        // Independent floating slab oracle over this bounded +/-6m fixture, not runtime authority.
        let start = f64::from(i32::try_from(ray.origin[axis]).unwrap());
        let delta = f64::from(i32::try_from(ray.delta[axis]).unwrap());
        let minimum = f64::from(coordinate) * 1_000_000.0;
        let maximum = minimum + 1_000_000.0;
        if delta == 0.0 {
            if start < minimum || start >= maximum {
                return false;
            }
        } else {
            let a = (minimum - start) / delta;
            let b = (maximum - start) / delta;
            entry = entry.max(a.min(b));
            exit = exit.min(a.max(b));
        }
    }
    exit > entry + 1.0e-10
}
