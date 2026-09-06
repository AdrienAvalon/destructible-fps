use super::*;
use crate::{
    IVec3, Material, World,
    volume::{LocalBox, RefinedVolume, VolumeLimits},
    world::geometry::{GeometryCell, GeometryChange, GeometryState, RefinedWorld},
};

fn install(state: &mut GeometryState, position: IVec3, volume: RefinedVolume) {
    let tx = state
        .prepare(
            state.world().tick() + 1,
            vec![GeometryChange {
                position,
                before: state.world().cell(position),
                after: GeometryCell::refined(volume),
            }],
        )
        .unwrap();
    state.apply(&tx).unwrap();
}
fn cut(page: &RefinedVolume, low: [u16; 3], high: [u16; 3], voxel: Voxel) -> RefinedVolume {
    page.replace_box(
        LocalBox::new(low, high).unwrap(),
        voxel,
        VolumeLimits::default(),
    )
    .unwrap()
    .0
}
fn probe(world: &impl StaticGeometry) -> PenetrationProbe {
    probe_static_rifle(
        world,
        FixedMicrometers3 {
            x: -500_000,
            y: 500_000,
            z: 500_000,
        },
        [1, 0, 0],
        TraceLimits::default(),
    )
    .unwrap()
}

#[test]
fn exact_thin_layers_consume_material_work_and_preserve_real_air_gaps() {
    let mut state = GeometryState::new(RefinedWorld::default(), 1).unwrap();
    let mut page = RefinedVolume::uniform(Voxel::AIR);
    for (low, high, material) in [
        (0, 32, Material::Glass),
        (64, 96, Material::Wood),
        (200, 201, Material::Steel),
    ] {
        page = cut(&page, [low, 0, 0], [high, 256, 256], Voxel::new(material));
    }
    install(&mut state, IVec3::default(), page);
    let result = probe(state.world());
    assert_eq!(result.trace.chords.len(), 3);
    assert_eq!(result.material_runs, 3);
    assert_eq!(
        result.spent_micro_work,
        37_500_000 + 250_000_000 + 195_312_500
    );
    assert_eq!(result.remaining_micro_work, 267_187_500);
    assert!(result.stopped_in.is_none());
    assert!(
        result
            .trace
            .chords
            .windows(2)
            .all(|p| p[0].material.exit < p[1].material.entry)
    );
}

#[test]
fn unrelated_leaf_splits_and_page_boundaries_do_not_change_work() {
    let mut coarse = World::default();
    coarse.fill_box(
        IVec3::new(0, 0, 0),
        IVec3::new(1, 0, 0),
        Voxel::new(Material::Glass),
    );
    let original = probe(&coarse);
    assert_eq!(original.spent_micro_work, 600_000_000);
    assert_eq!(original.material_runs, 1);
    let mut state = GeometryState::new(RefinedWorld::from_uniform(&coarse).unwrap(), 1).unwrap();
    let mut page = RefinedVolume::uniform(Voxel::new(Material::Glass));
    // An off-ray edit splits Z slabs. A diagonal ray traverses them while seeing the SAME field.
    for x in (1..256).step_by(13) {
        page = cut(&page, [x, 0, 0], [x + 1, 1, 256], Voxel::AIR);
    }
    for x in 0..2 {
        install(&mut state, IVec3::new(x, 0, 0), page.clone());
    }
    let modified = probe(state.world());
    // X intervals in unaffected Y bands canonicalize, so use fine Z-slab splits along a diagonal
    // for a genuinely different source partition, not just a different source fingerprint.
    assert_eq!(original.spent_micro_work, modified.spent_micro_work);
    let mut p = RefinedVolume::uniform(Voxel::new(Material::Glass));
    for z in (1..256).step_by(13) {
        p = cut(&p, [0, 0, z], [1, 256, z + 1], Voxel::AIR);
    }
    let mut layered = GeometryState::new(RefinedWorld::default(), 1).unwrap();
    install(&mut layered, IVec3::default(), p);
    let origin = FixedMicrometers3 {
        x: 500_000,
        y: 500_000,
        z: -500_000,
    };
    let a = probe_static_rifle(layered.world(), origin, [0, 0, 1], TraceLimits::default()).unwrap();
    let mut baseline = World::default();
    baseline.set_voxel(IVec3::default(), Voxel::new(Material::Glass));
    let b = probe_static_rifle(&baseline, origin, [0, 0, 1], TraceLimits::default()).unwrap();
    assert!(a.trace.chords.len() > 20);
    assert_eq!(a.material_runs, 1);
    assert_eq!(a.spent_micro_work, b.spent_micro_work);
}

#[test]
fn oblique_chords_cost_more_and_remaining_solid_with_zero_integrity_is_not_free() {
    let mut world = World::default();
    world.fill_box(
        IVec3::new(0, -3, -3),
        IVec3::new(0, 3, 3),
        Voxel::new(Material::Glass),
    );
    let axial = probe(&world);
    let oblique = probe_static_rifle(
        &world,
        FixedMicrometers3 {
            x: -500_000,
            y: 500_000,
            z: 500_000,
        },
        [3, 4, 0],
        TraceLimits::default(),
    )
    .unwrap();
    assert_eq!(axial.spent_micro_work, 300_000_000);
    assert!((500_000_000..=500_000_002).contains(&oblique.spent_micro_work));
    world.set_voxel(
        IVec3::default(),
        Voxel {
            material: Material::Steel,
            integrity: 0,
        },
    );
    let zero = probe(&world);
    assert_eq!(zero.spent_micro_work, 196_078_432);
    assert!(zero.trace.chords[0].material.leaf.voxel().is_solid());
}

#[test]
fn solid_stops_probe_without_mutation_while_a_fine_bore_transmits_it() {
    let mut world = World::default();
    world.set_voxel(IVec3::default(), Voxel::new(Material::Wood));
    let mut state = GeometryState::new(RefinedWorld::from_uniform(&world).unwrap(), 1).unwrap();
    let original = state.world().fingerprint();
    let blocked = probe(state.world());
    assert_eq!(blocked.remaining_micro_work, 0);
    assert_eq!(blocked.spent_micro_work, RIFLE_MICRO_WORK);
    assert_eq!(blocked.stopped_in.unwrap().voxel.material, Material::Wood);
    assert_eq!(state.world().fingerprint(), original);
    let page = cut(
        &RefinedVolume::uniform(Voxel::new(Material::Wood)),
        [0, 127, 127],
        [256, 129, 129],
        Voxel::AIR,
    );
    install(&mut state, IVec3::default(), page);
    let bored = probe(state.world());
    assert!(bored.trace.chords.is_empty());
    assert_eq!(bored.remaining_micro_work, RIFLE_MICRO_WORK);
    assert_eq!(bored.spent_micro_work, 0);
    assert_ne!(bored.trace.source_fingerprint, original);
}

#[test]
fn all_axis_directions_at_far_world_coordinates_have_equal_resistance() {
    for axis in 0..3 {
        for sign in [-1, 1] {
            let mut world = World::default();
            let p = IVec3::new(-1_000_000, -1_000_000, -1_000_000);
            world.set_voxel(p, Voxel::new(Material::Glass));
            let mut origin = [-999_999_500_000; 3];
            origin[axis] = if sign == 1 {
                -1_000_000_000_000
            } else {
                -999_999_000_000
            };
            let mut direction = [0; 3];
            direction[axis] = sign;
            let result = probe_static_rifle(
                &world,
                FixedMicrometers3 {
                    x: origin[0],
                    y: origin[1],
                    z: origin[2],
                },
                direction,
                TraceLimits::default(),
            )
            .unwrap();
            assert_eq!(result.spent_micro_work, 300_000_000);
        }
    }
}

#[test]
fn grazing_material_uses_positive_high_precision_work_without_overflow() {
    let mut world = World::default();
    world.set_voxel(IVec3::default(), Voxel::new(Material::Steel));
    let result = probe_static_rifle(
        &world,
        FixedMicrometers3 {
            x: -1,
            y: 999_999,
            z: 500_000,
        },
        [32767, 32766, 0],
        TraceLimits::default(),
    )
    .unwrap();
    assert_eq!(result.trace.chords.len(), 1);
    assert!(result.spent_micro_work > 0 && result.spent_micro_work < 1000);
    assert!(result.stopped_in.is_none());
    assert_eq!(
        result.remaining_micro_work + result.spent_micro_work,
        RIFLE_MICRO_WORK
    );
}

#[test]
fn point_probe_does_not_replace_conservative_coarse_weapon_seam_protection() {
    let mut world = World::default();
    world.set_voxel(IVec3::default(), Voxel::new(Material::Wood));
    let origin = FixedMicrometers3 {
        x: -1_000_000,
        y: 1_000_000,
        z: 500_000,
    };
    let probe = probe_static_rifle(&world, origin, [1, 0, 0], TraceLimits::default()).unwrap();
    assert!(probe.trace.chords.is_empty());
    let original = world.fingerprint();
    let coarse = crate::ballistics::apply_rifle(&mut world, origin, [1, 0, 0], None).unwrap();
    assert_eq!(coarse.remaining_energy, 0);
    assert!(coarse.destruction.changes.is_empty());
    assert_eq!(world.fingerprint(), original);
}

#[test]
fn checkpoint_replica_has_identical_trace_work_and_budget_refusal() {
    let source = crate::mesh::fine::fixture::industrial_inspection_world(1).unwrap();
    let state = GeometryState::new(source, 7).unwrap();
    let restored = GeometryState::decode_checkpoint(&state.encode_checkpoint().unwrap()).unwrap();
    let origin = FixedMicrometers3 {
        x: -15_000_000,
        y: 2_367_187,
        z: 18_000_000,
    };
    let a = probe_static_rifle(state.world(), origin, [0, 0, -1], TraceLimits::default()).unwrap();
    let b =
        probe_static_rifle(restored.world(), origin, [0, 0, -1], TraceLimits::default()).unwrap();
    assert_eq!(a.trace.chords, b.trace.chords);
    assert_eq!(a.trace.stats, b.trace.stats);
    assert_eq!(a.trace.source_fingerprint, b.trace.source_fingerprint);
    assert_eq!(a.remaining_micro_work, b.remaining_micro_work);
    assert_eq!(a.stopped_in, b.stopped_in);
    let limits = TraceLimits {
        leaf_visits: a.trace.stats.leaf_visits - 1,
        ..TraceLimits::default()
    };
    assert_eq!(
        probe_static_rifle(state.world(), origin, [0, 0, -1], limits).unwrap_err(),
        TraceError::Leaves
    );
    assert_eq!(
        probe_static_rifle(restored.world(), origin, [0, 0, -1], limits).unwrap_err(),
        TraceError::Leaves
    );
}
