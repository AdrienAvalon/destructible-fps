use super::*;
use crate::Material;
use crate::{
    volume::{RefinedVolume, VolumeLimits},
    world::geometry::{GeometryChange, GeometryState},
};

fn budget() -> QueryBudget {
    QueryBudget::new(QueryLimits::default()).unwrap()
}

#[test]
fn uniform_overlap_touch_and_sweep_use_exact_half_open_bounds() {
    let mut world = World::default();
    world.set_voxel(IVec3::new(1, 0, 0), Voxel::new(Material::Stone));
    let start = PhysicalBox::from_micrometers([0; 3], [500_000; 3]).unwrap();
    assert!(!overlaps_solid(&world, start, &mut budget()).unwrap());
    assert_eq!(
        sweep_axis(&world, start, 0, 1_000_000, &mut budget()).unwrap(),
        SweepResult {
            displacement_um: 500_000,
            contact: true
        }
    );
    let tangent =
        PhysicalBox::from_micrometers([500_000, 0, 0], [1_000_000, 500_000, 500_000]).unwrap();
    assert!(!overlaps_solid(&world, tangent, &mut budget()).unwrap());
    assert_eq!(
        sweep_axis(&world, tangent, 0, 1, &mut budget())
            .unwrap()
            .displacement_um,
        0
    );
    assert_eq!(
        sweep_axis(&world, tangent, 0, -1, &mut budget()).unwrap(),
        SweepResult {
            displacement_um: -1,
            contact: false
        }
    );
}

#[test]
fn invalid_boxes_limits_and_excessive_queries_fail_explicitly() {
    assert_eq!(
        PhysicalBox::from_micrometers([i64::MIN; 3], [i64::MAX; 3]),
        Err(GeometryQueryError::WorldBounds)
    );
    assert_eq!(
        PhysicalBox::from_micrometers([0; 3], [0; 3]),
        Err(GeometryQueryError::InvalidBox)
    );
    assert!(
        QueryBudget::new(QueryLimits {
            cells: 0,
            leaf_visits: 1
        })
        .is_err()
    );
    let bounds = PhysicalBox::from_micrometers([0; 3], [9_000_000; 3]).unwrap();
    assert_eq!(
        overlaps_solid(&World::default(), bounds, &mut budget()),
        Err(GeometryQueryError::CellBudget)
    );
}

fn page_world(position: IVec3, volume: RefinedVolume) -> GeometryState {
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

#[test]
fn fractional_fine_planes_resolve_both_directions_on_all_axes_at_negative_world_coordinates() {
    let position = IVec3::new(-17, -1, -16);
    let origin =
        [position.x, position.y, position.z].map(|value| i64::from(value) * MICROMETERS_PER_VOXEL);
    for axis in 0..3 {
        let mut low = [0; 3];
        let mut high = [256; 3];
        low[axis] = 129;
        high[axis] = 130;
        let (page, _) = RefinedVolume::uniform(Voxel::AIR)
            .replace_box(
                LocalBox::new(low, high).unwrap(),
                Voxel::new(Material::Steel),
                VolumeLimits::default(),
            )
            .unwrap();
        let state = page_world(position, page);
        let minimum = origin.map(|value| value + 100_000);
        let maximum = origin.map(|value| value + 200_000);
        let forward = PhysicalBox::from_micrometers(minimum, maximum).unwrap();
        assert_eq!(
            sweep_axis(state.world(), forward, axis, 1_000_000, &mut budget()).unwrap(),
            SweepResult {
                displacement_um: 303_906,
                contact: true
            }
        );
        assert_eq!(
            sweep_axis(state.world(), forward, axis, 303_906, &mut budget()).unwrap(),
            SweepResult {
                displacement_um: 303_906,
                contact: false
            }
        );
        let mut minimum = minimum;
        let mut maximum = maximum;
        minimum[axis] = origin[axis] + 800_000;
        maximum[axis] = origin[axis] + 900_000;
        let reverse = PhysicalBox::from_micrometers(minimum, maximum).unwrap();
        assert_eq!(
            sweep_axis(state.world(), reverse, axis, -1_000_000, &mut budget()).unwrap(),
            SweepResult {
                displacement_um: -292_187,
                contact: true
            }
        );
        // Offset rounds toward zero; POSITION need not be positive. Both results remain outside.
        let mut touching_min = forward.minimum;
        let mut touching_max = forward.maximum;
        touching_max[axis] = origin[axis] * SCALED_PER_MICROMETER + 129 * MICROMETERS_PER_VOXEL;
        touching_min[axis] = touching_max[axis] - SCALED_PER_MICROMETER;
        let touching = PhysicalBox::from_scaled(touching_min, touching_max).unwrap();
        assert!(!overlaps_solid(state.world(), touching, &mut budget()).unwrap());
        assert_eq!(
            sweep_axis(state.world(), touching, axis, 1, &mut budget()).unwrap(),
            SweepResult {
                displacement_um: 0,
                contact: true
            }
        );
        assert!(
            !sweep_axis(state.world(), touching, axis, -1, &mut budget())
                .unwrap()
                .contact
        );
        touching_min[axis] += 1;
        touching_max[axis] += 1;
        let overlapping = PhysicalBox::from_scaled(touching_min, touching_max).unwrap();
        assert!(overlaps_solid(state.world(), overlapping, &mut budget()).unwrap());
        assert_eq!(
            sweep_axis(state.world(), overlapping, axis, 0, &mut budget()),
            Err(GeometryQueryError::InitialOverlap)
        );
    }
}

#[test]
fn sweep_crosses_empty_cells_but_not_an_intermediate_thin_sheet() {
    let (page, _) = RefinedVolume::uniform(Voxel::AIR)
        .replace_box(
            LocalBox::new([0, 0, 1], [256, 256, 2]).unwrap(),
            Voxel::new(Material::Glass),
            VolumeLimits::default(),
        )
        .unwrap();
    let state = page_world(IVec3::new(0, 0, 0), page);
    let start = PhysicalBox::from_micrometers(
        [100_000, 100_000, -2_000_000],
        [200_000, 200_000, -1_900_000],
    )
    .unwrap();
    let end =
        PhysicalBox::from_micrometers([100_000, 100_000, 2_000_000], [200_000, 200_000, 2_100_000])
            .unwrap();
    assert!(!overlaps_solid(state.world(), end, &mut budget()).unwrap());
    assert_eq!(
        sweep_axis(state.world(), start, 2, 4_000_000, &mut budget()).unwrap(),
        SweepResult {
            displacement_um: 1_903_906,
            contact: true
        }
    );
}

#[test]
fn endpoint_world_boundaries_invalid_motion_and_shared_work_are_explicit() {
    let empty = World::default();
    let box_at_end = PhysicalBox::from_scaled(
        [MAXIMUM_SCALED - 512, 0, 0],
        [MAXIMUM_SCALED - 256, 256, 256],
    )
    .unwrap();
    assert_eq!(
        sweep_axis(&empty, box_at_end, 0, 1, &mut budget())
            .unwrap()
            .displacement_um,
        1
    );
    assert_eq!(
        sweep_axis(&empty, box_at_end, 0, 2, &mut budget()),
        Err(GeometryQueryError::WorldBounds)
    );
    assert_eq!(
        sweep_axis(&empty, box_at_end, usize::MAX, 1, &mut budget()),
        Err(GeometryQueryError::InvalidAxis)
    );
    assert_eq!(
        sweep_axis(&empty, box_at_end, 0, i64::MIN, &mut budget()),
        Err(GeometryQueryError::InvalidDisplacement)
    );
    let bounds = PhysicalBox::from_micrometers([0; 3], [1; 3]).unwrap();
    let mut shared = QueryBudget::new(QueryLimits {
        cells: 1,
        leaf_visits: 1,
    })
    .unwrap();
    assert!(!overlaps_solid(&empty, bounds, &mut shared).unwrap());
    assert_eq!(
        shared.stats(),
        QueryStats {
            cells: 1,
            leaf_visits: 1,
            solid_boxes: 0
        }
    );
    assert_eq!(
        overlaps_solid(&empty, bounds, &mut shared),
        Err(GeometryQueryError::CellBudget)
    );
    let (page, _) = RefinedVolume::uniform(Voxel::AIR)
        .replace_box(
            LocalBox::new([128; 3], [256; 3]).unwrap(),
            Voxel::new(Material::Stone),
            VolumeLimits::default(),
        )
        .unwrap();
    let fine = page_world(IVec3::new(0, 0, 0), page);
    let mut tiny = QueryBudget::new(QueryLimits {
        cells: 20,
        leaf_visits: 1,
    })
    .unwrap();
    assert_eq!(
        overlaps_solid(fine.world(), bounds, &mut tiny),
        Err(GeometryQueryError::LeafBudget)
    );
    assert_eq!(tiny.stats().leaf_visits, 1);
    assert_eq!(
        overlaps_solid(fine.world(), bounds, &mut tiny),
        Err(GeometryQueryError::LeafBudget)
    );
}

#[test]
fn seeded_overlap_matches_an_independent_dense_grid_and_restored_work_counters() {
    let position = IVec3::new(-1, 0, -1);
    let origin = [-CELL_SCALED, 0, -CELL_SCALED];
    let mut page = RefinedVolume::uniform(Voxel::AIR);
    let mut dense = [false; 512];
    let mut seed = 0xdec5_9b13_30e7_52aa_u64;
    let mut random = || {
        seed = super::super::splitmix64(seed);
        seed
    };
    for _ in 0..32 {
        let low = std::array::from_fn::<_, 3, _>(|_| u16::try_from(random() % 8).unwrap());
        let high = std::array::from_fn(|axis| {
            low[axis] + 1 + u16::try_from(random() % u64::from(8 - low[axis])).unwrap()
        });
        let solid = random() & 1 != 0;
        page = page
            .replace_box(
                LocalBox::new(low.map(|v| v * 32), high.map(|v| v * 32)).unwrap(),
                if solid {
                    Voxel::new(Material::Wood)
                } else {
                    Voxel::AIR
                },
                VolumeLimits::default(),
            )
            .unwrap()
            .0;
        for x in low[0]..high[0] {
            for y in low[1]..high[1] {
                for z in low[2]..high[2] {
                    dense[usize::from(x + 8 * y + 64 * z)] = solid;
                }
            }
        }
    }
    let state = page_world(position, page);
    let restored = GeometryState::decode_checkpoint(&state.encode_checkpoint().unwrap()).unwrap();
    for _ in 0..2000 {
        let minimum = std::array::from_fn(|axis| {
            origin[axis] - CELL_SCALED / 8
                + i64::try_from(random() % u64::try_from(CELL_SCALED * 5 / 4).unwrap()).unwrap()
        });
        let maximum = std::array::from_fn(|axis| {
            minimum[axis]
                + 1
                + i64::try_from(random() % u64::try_from(CELL_SCALED / 3).unwrap()).unwrap()
        });
        let bounds = PhysicalBox::from_scaled(minimum, maximum).unwrap();
        let expected = dense.iter().enumerate().any(|(index, &occupied)| {
            let cell = [index % 8, index / 8 % 8, index / 64];
            occupied
                && (0..3).all(|axis| {
                    let low = i128::from(origin[axis])
                        + i128::try_from(cell[axis]).unwrap() * i128::from(CELL_SCALED / 8);
                    let high = low + i128::from(CELL_SCALED / 8);
                    i128::from(minimum[axis]) < high && i128::from(maximum[axis]) > low
                })
        });
        let mut first_budget = budget();
        let mut second_budget = budget();
        assert_eq!(
            overlaps_solid(state.world(), bounds, &mut first_budget).unwrap(),
            expected
        );
        assert_eq!(
            overlaps_solid(restored.world(), bounds, &mut second_budget).unwrap(),
            expected
        );
        assert_eq!(first_budget.stats(), second_budget.stats());
    }
}
