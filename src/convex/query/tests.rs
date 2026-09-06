use super::*;
use crate::{
    FixedMicrometers3, Material,
    convex::ConvexFaceInput,
    volume::{LocalBox, RefinedVolume, VolumeLimits, ray::RayLimits},
};

fn budget() -> ConvexQueryBudget {
    ConvexQueryBudget::new(ConvexQueryLimits::default()).unwrap()
}

fn slab(origin: IVec3, shear: bool) -> ConvexFragment {
    let vertices: Vec<_> = (0_u16..8)
        .map(|bits| {
            let x = (bits & 1) * 256;
            [
                x,
                ((bits >> 1) & 1) * 256 + if shear { x / 2 } else { 0 },
                ((bits >> 2) & 1) * 256,
            ]
        })
        .collect();
    let faces: Vec<_> = [
        [0, 2, 3, 1],
        [4, 5, 7, 6],
        [0, 4, 6, 2],
        [1, 3, 7, 5],
        [0, 1, 5, 4],
        [2, 6, 7, 3],
    ]
    .into_iter()
    .map(|indices| ConvexFaceInput {
        indices: indices.to_vec(),
        cut: true,
    })
    .collect();
    ConvexFragment::new(origin, &vertices, &faces, Voxel::new(Material::Concrete)).unwrap()
}

fn ray(origin: [i64; 3], direction: [i16; 3], length: u64) -> FixedRay {
    FixedRay::new(
        FixedMicrometers3 {
            x: origin[0],
            y: origin[1],
            z: origin[2],
        },
        direction,
        length,
    )
    .unwrap()
}

fn bounds(low: [i64; 3], high: [i64; 3]) -> PhysicalBox {
    PhysicalBox::from_micrometers(low, high).unwrap()
}

#[test]
fn axis_box_chords_match_the_independent_volume_oracle_off_boundary() {
    let shape = slab(IVec3::new(0, 0, 0), false);
    let volume = RefinedVolume::uniform(Voxel::AIR)
        .replace_box(
            LocalBox::FULL,
            Voxel::new(Material::Concrete),
            VolumeLimits::default(),
        )
        .unwrap()
        .0;
    for axis in 0..3 {
        for sign in [-1_i16, 1] {
            for transverse in [1, 123_456, 500_000, 999_999] {
                let mut start = [transverse; 3];
                start[axis] = if sign > 0 { -1_000_000 } else { 2_000_000 };
                let mut direction = [0; 3];
                direction[axis] = sign;
                let segment = ray(start, direction, 3_000_000);
                let end = std::array::from_fn(|i| {
                    i32::try_from(segment.origin[i] + segment.delta[i]).unwrap()
                });
                let expected = volume
                    .trace_segment(
                        start.map(|v| i32::try_from(v).unwrap()),
                        end,
                        RayLimits::default(),
                    )
                    .unwrap();
                let actual = shape.trace(&segment, &mut budget()).unwrap().unwrap();
                assert_eq!(expected.chords.len(), 1);
                assert_eq!(actual.entry, expected.chords[0].entry);
                assert_eq!(actual.exit, expected.chords[0].exit);
                assert_eq!(actual.material, expected.chords[0].leaf.voxel());
            }
        }
    }
}

#[test]
fn oblique_planes_have_analytical_chords_and_conservative_box_contacts() {
    let shape = slab(IVec3::new(0, 0, 0), true);
    let chord = shape
        .trace(
            &ray([500_000, 3_000_000, 500_000], [0, -1, 0], 3_000_000),
            &mut budget(),
        )
        .unwrap()
        .unwrap();
    assert_eq!(chord.entry, SegmentParameter::bounded(7, 12).unwrap());
    assert_eq!(chord.exit, SegmentParameter::bounded(11, 12).unwrap());
    let start = bounds([250_000, 2_000_000, 250_000], [750_000, 2_250_000, 750_000]);
    let contact = shape
        .sweep_axis(start, 1, -2_000_000, &mut budget())
        .unwrap();
    assert_eq!(contact.displacement_um, -625_000);
    assert!(contact.contact);
    let just_before = bounds([250_000, 1_375_000, 250_000], [750_000, 1_625_000, 750_000]);
    assert!(!shape.overlaps(just_before, &mut budget()).unwrap());
    assert!(
        shape
            .overlaps(
                bounds([250_000, 1_374_999, 250_000], [750_000, 1_624_999, 750_000]),
                &mut budget()
            )
            .unwrap()
    );
}

#[test]
fn tangency_coplanarity_initial_overlap_and_fast_crossing_have_explicit_semantics() {
    let shape = slab(IVec3::new(0, 0, 0), false);
    // Both boundary faces are excluded for strict-interior convex rays, unlike legacy min faces.
    for y in [0, 1_000_000] {
        assert!(
            shape
                .trace(
                    &ray([-1_000_000, y, 500_000], [1, 0, 0], 3_000_000),
                    &mut budget()
                )
                .unwrap()
                .is_none()
        );
    }
    assert!(
        shape
            .trace(
                &ray([-1_000_000, 500_000, 500_000], [1, 0, 0], 1_000_000),
                &mut budget()
            )
            .unwrap()
            .is_none()
    );
    let resting = bounds([250_000, 1_000_000, 250_000], [750_000, 1_500_000, 750_000]);
    assert!(
        !shape
            .sweep_axis(resting, 0, 100_000, &mut budget())
            .unwrap()
            .contact
    );
    assert!(
        !shape
            .sweep_axis(resting, 1, 100_000, &mut budget())
            .unwrap()
            .contact
    );
    let inward = shape.sweep_axis(resting, 1, -1, &mut budget()).unwrap();
    assert!(inward.contact);
    assert_eq!(inward.displacement_um, 0);
    let inside = bounds([250_000; 3], [750_000; 3]);
    assert_eq!(
        shape.sweep_axis(inside, 0, 0, &mut budget()),
        Err(ConvexError::InitialOverlap)
    );
    assert_eq!(
        shape.sweep_axis(inside, 0, 100_000, &mut budget()),
        Err(ConvexError::InitialOverlap)
    );
    let fast = bounds(
        [-50_000_000, 250_000, 250_000],
        [-49_000_000, 750_000, 750_000],
    );
    let result = shape
        .sweep_axis(fast, 0, 100_000_000, &mut budget())
        .unwrap();
    assert!(result.contact);
    assert_eq!(result.displacement_um, 49_000_000);
    let endpoint = shape
        .sweep_axis(fast, 0, 49_000_000, &mut budget())
        .unwrap();
    assert!(endpoint.contact);
    assert_eq!(endpoint.displacement_um, 49_000_000);
}

#[test]
fn edge_cross_axes_reject_a_false_face_only_overlap() {
    let vertices = [[0, 0, 0], [256, 48, 96], [96, 256, 16], [48, 80, 256]];
    let faces: Vec<_> = [[0, 2, 1], [0, 1, 3], [0, 3, 2], [1, 2, 3]]
        .into_iter()
        .map(|indices| ConvexFaceInput {
            indices: indices.to_vec(),
            cut: true,
        })
        .collect();
    let shape = ConvexFragment::new(
        IVec3::new(0, 0, 0),
        &vertices,
        &faces,
        Voxel::new(Material::Stone),
    )
    .unwrap();
    let test = PhysicalBox::from_scaled(
        [-16, 0, 96].map(|v| v * 1_000_000),
        [16, 32, 128].map(|v| v * 1_000_000),
    )
    .unwrap();
    let box_source = BoxProjectionSource::new(test, shape.origin()).unwrap();
    for axis in shape.planes().iter().map(|plane| plane.normal()).chain(XYZ) {
        let fixed = shape.project(axis, [0; 3], &mut budget()).unwrap();
        let moving = box_source.project(axis, &mut budget()).unwrap();
        assert!(
            moving.0 < fixed.1 && moving.1 > fixed.0,
            "fixture must fool face/XYZ-only SAT"
        );
    }
    assert!(!shape.overlaps(test, &mut budget()).unwrap());
    assert!(
        !shape
            .sweep_axis(test, 2, -1, &mut budget())
            .unwrap()
            .contact
    );
}

#[test]
fn fragment_overlap_and_translation_invariance_use_real_shapes() {
    let first = slab(IVec3::new(0, 0, 0), true);
    assert!(first.overlaps_fragment(&first, &mut budget()).unwrap());
    let neighbor = slab(IVec3::new(1, 0, 0), true);
    assert!(!first.overlaps_fragment(&neighbor, &mut budget()).unwrap());
    for origin in [
        IVec3::new(-16_000, -16_000, -16_000),
        IVec3::new(16_000, 16_000, 16_000),
    ] {
        let shifted = slab(origin, true);
        let offset = [origin.x, origin.y, origin.z].map(|v| i64::from(v) * 1_000_000);
        let r = ray(
            [
                offset[0] + 500_000,
                offset[1] + 3_000_000,
                offset[2] + 500_000,
            ],
            [0, -1, 0],
            3_000_000,
        );
        let hit = shifted.trace(&r, &mut budget()).unwrap().unwrap();
        assert_eq!(hit.entry, SegmentParameter::bounded(7, 12).unwrap());
        assert_eq!(hit.exit, SegmentParameter::bounded(11, 12).unwrap());
    }
    let far = slab(IVec3::new(16_000, 0, 0), false);
    assert!(!first.overlaps_fragment(&far, &mut budget()).unwrap());
    assert!(
        !far.overlaps(bounds([0; 3], [1_000_000; 3]), &mut budget())
            .unwrap()
    );
    assert!(
        far.trace(&ray([0; 3], [1, 0, 0], 1_000_000), &mut budget())
            .unwrap()
            .is_none()
    );
}

#[test]
fn exact_work_limits_and_invalid_inputs_fail_without_partial_results() {
    let shape = slab(IVec3::new(0, 0, 0), false);
    let segment = ray([-1_000_000, 500_000, 500_000], [1, 0, 0], 3_000_000);
    let mut measured = budget();
    let expected = shape.trace(&segment, &mut measured).unwrap();
    let used = measured.stats();
    let exact = ConvexQueryLimits {
        fragments: used.fragments,
        axes: used.axes,
        projections: used.projections,
    };
    assert_eq!(
        shape
            .trace(&segment, &mut ConvexQueryBudget::new(exact).unwrap())
            .unwrap(),
        expected
    );
    for limits in [
        ConvexQueryLimits {
            axes: exact.axes - 1,
            ..exact
        },
        ConvexQueryLimits {
            projections: exact.projections - 1,
            ..exact
        },
    ] {
        assert_eq!(
            shape.trace(&segment, &mut ConvexQueryBudget::new(limits).unwrap()),
            Err(ConvexError::Budget)
        );
    }
    let mut single = ConvexQueryBudget::new(ConvexQueryLimits {
        fragments: 1,
        ..ConvexQueryLimits::default()
    })
    .unwrap();
    let far = ray([20_000_000; 3], [1, 0, 0], 1_000_000);
    assert!(shape.trace(&far, &mut single).unwrap().is_none());
    assert_eq!(single.stats().fragments, 1);
    assert_eq!(shape.trace(&far, &mut single), Err(ConvexError::Budget));
    let valid = bounds([2_000_000; 3], [3_000_000; 3]);
    assert_eq!(
        shape.sweep_axis(valid, 3, 1, &mut budget()),
        Err(ConvexError::Bounds)
    );
    assert_eq!(
        shape.sweep_axis(valid, 0, 128_000_001, &mut budget()),
        Err(ConvexError::Bounds)
    );
    assert_eq!(
        shape.overlaps(bounds([0; 3], [16_000_001; 3]), &mut budget()),
        Err(ConvexError::Bounds)
    );
    assert!(ConvexQueryBudget::new(ConvexQueryLimits { axes: 0, ..exact }).is_err());
    assert!(SegmentParameter::bounded(-1, 2).is_none());
    assert!(SegmentParameter::bounded(1, 0).is_none());
    assert!(SegmentParameter::bounded(3, 2).is_none());
}

#[test]
fn six_direction_box_sweeps_match_the_existing_refined_world_oracle() {
    use crate::{
        World,
        world::{
            geometry::RefinedWorld,
            query::{QueryBudget, QueryLimits, sweep_axis},
        },
    };
    let shape = slab(IVec3::new(0, 0, 0), false);
    let mut coarse = World::default();
    coarse.fill_box(
        IVec3::new(0, 0, 0),
        IVec3::new(0, 0, 0),
        Voxel::new(Material::Concrete),
    );
    let world = RefinedWorld::from_uniform(&coarse).unwrap();
    for axis in 0..3 {
        for sign in [-1, 1] {
            for gap in [0, 1, 17, 500_000] {
                let mut low = [250_000; 3];
                let mut high = [750_000; 3];
                if sign > 0 {
                    low[axis] = -gap - 500_000;
                    high[axis] = -gap;
                } else {
                    low[axis] = 1_000_000 + gap;
                    high[axis] = 1_500_000 + gap;
                }
                let start = bounds(low, high);
                for displacement in [sign * gap, sign * (gap + 1), sign * (gap + 2_000_000)] {
                    let expected = sweep_axis(
                        &world,
                        start,
                        axis,
                        displacement,
                        &mut QueryBudget::new(QueryLimits::default()).unwrap(),
                    )
                    .unwrap();
                    let actual = shape
                        .sweep_axis(start, axis, displacement, &mut budget())
                        .unwrap();
                    assert_eq!(
                        actual, expected,
                        "axis={axis} gap={gap} displacement={displacement}"
                    );
                }
            }
        }
    }
}

#[test]
fn large_coprime_plane_coefficients_and_query_limits_stay_exact() {
    let vertices = [[0, 0, 0], [2048, 1, 17], [15, 2047, 3], [1, 19, 2048]];
    let faces: Vec<_> = [[0, 2, 1], [0, 1, 3], [0, 3, 2], [1, 2, 3]]
        .into_iter()
        .map(|indices| ConvexFaceInput {
            indices: indices.to_vec(),
            cut: true,
        })
        .collect();
    let shape = ConvexFragment::new(
        IVec3::new(0, 0, 0),
        &vertices,
        &faces,
        Voxel::new(Material::Stone),
    )
    .unwrap();
    assert!(
        shape
            .planes()
            .iter()
            .any(|plane| plane.normal().iter().any(|n| n.unsigned_abs() > 1_000_000))
    );
    let hit = shape
        .trace(
            &ray([-112_000_000, 1_000_000, 1_000_000], [1, 0, 0], 120_000_000),
            &mut budget(),
        )
        .unwrap()
        .unwrap();
    assert!(hit.entry < hit.exit);
    let start = bounds(
        [-128_000_000, 1_000_000, 1_000_000],
        [-112_000_000, 2_000_000, 2_000_000],
    );
    let contact = shape
        .sweep_axis(start, 0, 128_000_000, &mut budget())
        .unwrap();
    assert!(contact.contact);
    assert!((112_000_000..128_000_000).contains(&contact.displacement_um));
    // The rounded result is outside; one additional micrometre reaches positive overlap.
    let mut low = start.minimum_scaled();
    let mut high = start.maximum_scaled();
    low[0] += contact.displacement_um * 256;
    high[0] += contact.displacement_um * 256;
    assert!(
        !shape
            .overlaps(PhysicalBox::from_scaled(low, high).unwrap(), &mut budget())
            .unwrap()
    );
    low[0] += 256;
    high[0] += 256;
    assert!(
        shape
            .overlaps(PhysicalBox::from_scaled(low, high).unwrap(), &mut budget())
            .unwrap()
    );
}

#[test]
fn repeated_tangential_and_downhill_axis_queries_do_not_accumulate_penetration() {
    fn moved(source: PhysicalBox, axis: usize, displacement: i64) -> PhysicalBox {
        let mut low = source.minimum_scaled();
        let mut high = source.maximum_scaled();
        low[axis] += displacement * 256;
        high[axis] += displacement * 256;
        PhysicalBox::from_scaled(low, high).unwrap()
    }
    let shape = slab(IVec3::new(0, 0, 0), true);
    let mut position = bounds([600_000, 1_400_000, 250_000], [800_000, 1_600_000, 450_000]);
    // Deliberately prescribed axis queries, NOT a complete slope/step or player controller.
    for _ in 0..30 {
        let tangent = shape.sweep_axis(position, 2, 2_500, &mut budget()).unwrap();
        assert!(!tangent.contact);
        assert_eq!(tangent.displacement_um, 2_500);
        position = moved(position, 2, tangent.displacement_um);
        let downhill = shape
            .sweep_axis(position, 0, -10_000, &mut budget())
            .unwrap();
        assert!(!downhill.contact);
        assert_eq!(downhill.displacement_um, -10_000);
        position = moved(position, 0, downhill.displacement_um);
        let settle = shape
            .sweep_axis(position, 1, -10_000, &mut budget())
            .unwrap();
        assert!(settle.contact);
        assert_eq!(settle.displacement_um, -5_000);
        position = moved(position, 1, settle.displacement_um);
        assert!(!shape.overlaps(position, &mut budget()).unwrap());
        assert_eq!(
            position.minimum_scaled()[1],
            256_000_000 + position.maximum_scaled()[0] / 2
        );
    }
}
