#![allow(clippy::cast_precision_loss)]
use super::*;
use crate::{
    FixedMicrometers3, Material, Voxel, World,
    volume::{LocalBox, RefinedVolume, VolumeLimits},
    world::geometry::{GeometryCell, GeometryChange, GeometryState, RefinedWorld},
};

fn install(state: &mut GeometryState, position: IVec3, after: GeometryCell) {
    let change = GeometryChange {
        position,
        before: state.world().cell(position),
        after,
    };
    let tx = state
        .prepare(state.world().tick() + 1, vec![change])
        .unwrap();
    state.apply(&tx).unwrap();
}

fn ray(origin: [i64; 3], direction: [i16; 3], range: u64) -> FixedRay {
    FixedRay::new(
        FixedMicrometers3 {
            x: origin[0],
            y: origin[1],
            z: origin[2],
        },
        direction,
        range,
    )
    .unwrap()
}

#[test]
fn six_signed_cross_chunk_bores_and_fractional_edges_share_actual_volume() {
    for axis in 0..3 {
        for sign in [-1, 1] {
            let mut state = GeometryState::new(RefinedWorld::default(), 1).unwrap();
            let mut low = [127; 3];
            low[axis] = 0;
            let mut high = [129; 3];
            high[axis] = 256;
            let page = RefinedVolume::uniform(Voxel::new(Material::Wood))
                .replace_box(
                    LocalBox::new(low, high).unwrap(),
                    Voxel::AIR,
                    VolumeLimits::default(),
                )
                .unwrap()
                .0;
            for offset in [0, 1] {
                let mut p = [-17; 3];
                p[axis] += offset;
                install(
                    &mut state,
                    IVec3::new(p[0], p[1], p[2]),
                    GeometryCell::refined(page.clone()),
                );
            }
            let mut origin = [-16_500_000; 3];
            origin[axis] = if sign == 1 { -17_500_000 } else { -14_500_000 };
            let mut direction = [0; 3];
            direction[axis] = sign;
            for (coordinate, count) in [(-16_500_000, 0), (-16_496_094, 0), (-16_496_093, 2)] {
                origin[(axis + 1) % 3] = coordinate;
                let trace = trace_materials(
                    state.world(),
                    &ray(origin, direction, 3_000_000),
                    TraceLimits::default(),
                )
                .unwrap();
                assert_eq!(trace.chords.len(), count);
                assert_eq!(trace.source_fingerprint, state.world().fingerprint());
                if count == 2 {
                    assert_eq!(
                        trace.chords[0].material.exit,
                        trace.chords[1].material.entry
                    );
                }
            }
        }
    }
}

#[test]
fn uniform_fast_path_matches_promoted_geometry_and_page_oracle_at_seams() {
    let mut coarse = World::default();
    coarse.fill_box(
        IVec3::new(-2, -2, -2),
        IVec3::new(2, 2, 2),
        Voxel::new(Material::Glass),
    );
    let promoted = RefinedWorld::from_uniform(&coarse).unwrap();
    for direction in [
        [1, 0, 0],
        [-1, 0, 0],
        [1, 1, 0],
        [1, 1, 1],
        [32767, -32768, 2],
    ] {
        let ray = ray([-1_000_000, 0, 0], direction, 7_000_000);
        let a = trace_materials(&coarse, &ray, TraceLimits::default()).unwrap();
        let b = trace_materials(&promoted, &ray, TraceLimits::default()).unwrap();
        assert_eq!(a.chords, b.chords);
        assert_eq!(a.stats, b.stats);
        assert_eq!(a.source_fingerprint, b.source_fingerprint);
        for hit in &a.chords {
            let p = [hit.cell.x, hit.cell.y, hit.cell.z].map(|v| i64::from(v) * 1_000_000);
            let start = std::array::from_fn(|i| i32::try_from(ray.origin[i] - p[i]).unwrap());
            let end = std::array::from_fn(|i| {
                i32::try_from(ray.origin[i] + ray.delta[i] - p[i]).unwrap()
            });
            let oracle = RefinedVolume::uniform(Voxel::new(Material::Glass))
                .trace_segment(start, end, RayLimits::default())
                .unwrap();
            assert_eq!(oracle.chords.as_slice(), &[hit.material]);
        }
    }
}

#[test]
fn exact_point_trace_does_not_invent_tangent_thickness_or_lose_submicrometre_chords() {
    let mut world = World::default();
    world.set_voxel(IVec3::new(0, 0, 0), Voxel::new(Material::Steel));
    for (origin, direction, range, count) in [
        ([-1_000_000, 1_000_000, 0], [1, 0, 0], 3_000_000, 0),
        ([-1_000_000, 0, 0], [1, 0, 0], 1_000_000, 0),
        ([1_000_000, 0, 0], [-1, 0, 0], 1_000_000, 1),
        ([-1_000_000, 0, 0], [1, 1, 0], 3_000_000, 0),
    ] {
        assert_eq!(
            trace_materials(
                &world,
                &ray(origin, direction, range),
                TraceLimits::default()
            )
            .unwrap()
            .chords
            .len(),
            count
        );
    }
    // 1um before the corner; exact positive chord is narrower than one micrometre.
    let grazing = ray([-1, 999_999, 500_000], [32767, 32766, 0], 3_000_000);
    let trace = trace_materials(&world, &grazing, TraceLimits::default()).unwrap();
    assert_eq!(trace.chords.len(), 1);
    assert!(trace.chords[0].material.entry < trace.chords[0].material.exit);
}

#[test]
fn aggregate_budgets_refuse_instead_of_publishing_prefix_and_allow_exact_limits() {
    let mut world = World::default();
    world.fill_box(
        IVec3::new(-1, 0, 0),
        IVec3::new(2, 0, 0),
        Voxel::new(Material::Wood),
    );
    let ray = ray([-1_500_000, 500_000, 500_000], [1, 0, 0], 5_000_000);
    let trace = trace_materials(&world, &ray, TraceLimits::default()).unwrap();
    let exact = TraceLimits {
        cells: trace.stats.cells,
        leaf_visits: trace.stats.leaf_visits,
        chords: trace.chords.len(),
    };
    assert!(trace_materials(&world, &ray, exact).is_ok());
    for (limits, error) in [
        (
            TraceLimits {
                cells: exact.cells - 1,
                ..exact
            },
            TraceError::Cells,
        ),
        (
            TraceLimits {
                leaf_visits: exact.leaf_visits - 1,
                ..exact
            },
            TraceError::Leaves,
        ),
        (
            TraceLimits {
                chords: exact.chords - 1,
                ..exact
            },
            TraceError::Chords,
        ),
        (TraceLimits { cells: 0, ..exact }, TraceError::InvalidLimits),
        (
            TraceLimits {
                leaf_visits: MAX_TRACE_LEAF_VISITS + 1,
                ..exact
            },
            TraceError::InvalidLimits,
        ),
        (
            TraceLimits {
                chords: MAX_TRACE_CHORDS + 1,
                ..exact
            },
            TraceError::InvalidLimits,
        ),
    ] {
        assert_eq!(trace_materials(&world, &ray, limits).unwrap_err(), error);
    }
    assert_eq!(trace.source_fingerprint, world.fingerprint());
}

#[test]
fn seeded_world_rays_match_independent_exhaustive_slab_intersections() {
    let mut world = World::default();
    for x in -2..=2 {
        for y in -2..=2 {
            for z in -2..=2 {
                if (x * 3 + y * 5 + z * 7) % 3 == 0 {
                    world.set_voxel(IVec3::new(x, y, z), Voxel::new(Material::Brick));
                }
            }
        }
    }
    let mut seed = 0x6795_3412_u32;
    for _ in 0..500 {
        let mut next = || {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            seed
        };
        let origin = std::array::from_fn(|_| i64::from(next() % 8_000_001) - 4_000_000);
        let direction = std::array::from_fn(|_| {
            let bytes = next().to_le_bytes();
            i16::from_le_bytes([bytes[0], bytes[1]])
        });
        let ray = ray(origin, direction, 12_000_000);
        let squared: u128 = ray
            .delta
            .iter()
            .map(|v| i128::from(*v).pow(2).cast_unsigned())
            .sum();
        assert!(u128::from(ray.length_um()).pow(2) <= squared);
        assert!(u128::from(ray.length_ceil_um()).pow(2) >= squared);
        assert!(ray.length_ceil_um() - ray.length_um() <= 1);
        let trace = trace_materials(&world, &ray, TraceLimits::default()).unwrap();
        let mut expected = Vec::new();
        for (p, _) in world.occupied_voxels() {
            let mut near = 0_f64;
            let mut far = 1_f64;
            for (axis, cell) in [p.x, p.y, p.z].into_iter().enumerate() {
                let start = ray.origin[axis] as f64;
                let delta = ray.delta[axis] as f64;
                let low = f64::from(cell) * 1e6;
                if delta == 0.0 {
                    if start < low || start >= low + 1e6 {
                        far = -1.0;
                    }
                } else {
                    let a = (low - start) / delta;
                    let b = (low + 1e6 - start) / delta;
                    near = near.max(a.min(b));
                    far = far.min(a.max(b));
                }
            }
            if near < far {
                expected.push((near, far, p));
            }
        }
        expected.sort_by(|a, b| a.0.total_cmp(&b.0));
        assert_eq!(expected.len(), trace.chords.len());
        for ((near, far, p), actual) in expected.iter().zip(&trace.chords) {
            assert_eq!(*p, actual.cell);
            let ratio = |p: crate::volume::ray::SegmentParameter| {
                p.numerator() as f64 / p.denominator() as f64
            };
            assert!((near - ratio(actual.material.entry)).abs() < 1e-12);
            assert!((far - ratio(actual.material.exit)).abs() < 1e-12);
        }
    }
}

#[test]
fn dense_multi_page_trace_charges_one_global_budget_and_refuses_its_4097th_chord() {
    let mut page = RefinedVolume::uniform(Voxel::new(Material::Glass));
    for z in (1..256).step_by(2) {
        page = page
            .replace_box(
                LocalBox::new([0, 0, z], [256, 256, z + 1]).unwrap(),
                Voxel::new(Material::Wood),
                VolumeLimits::default(),
            )
            .unwrap()
            .0;
    }
    assert_eq!(page.leaves().len(), 256);
    let mut state = GeometryState::new(RefinedWorld::default(), 1).unwrap();
    for z in 0..16 {
        install(
            &mut state,
            IVec3::new(0, 0, z),
            GeometryCell::refined(page.clone()),
        );
    }
    let ray = ray([500_000, 500_000, -500_000], [0, 0, 1], 20_000_000);
    let full = trace_materials(state.world(), &ray, TraceLimits::default()).unwrap();
    assert_eq!(full.chords.len(), MAX_TRACE_CHORDS);
    assert!(full.stats.leaf_visits > 16 * 256);
    assert_eq!(
        trace_materials(
            state.world(),
            &ray,
            TraceLimits {
                leaf_visits: full.stats.leaf_visits - 1,
                ..TraceLimits::default()
            }
        )
        .unwrap_err(),
        TraceError::Leaves
    );
    assert!(
        trace_materials(
            state.world(),
            &ray,
            TraceLimits {
                leaf_visits: full.stats.leaf_visits,
                ..TraceLimits::default()
            }
        )
        .is_ok()
    );
    let original = state.world().fingerprint();
    assert_eq!(full.source_fingerprint, original);
    install(
        &mut state,
        IVec3::new(0, 0, 16),
        GeometryCell::refined(page),
    );
    let before = state.world().fingerprint();
    assert_eq!(
        trace_materials(state.world(), &ray, TraceLimits::default()).unwrap_err(),
        TraceError::Chords
    );
    assert_eq!(state.world().fingerprint(), before);
}
