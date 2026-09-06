use super::*;
use crate::{
    Material, Voxel,
    volume::{
        VolumeLimits,
        tests::{IRREGULAR, REGULAR, random_fixture},
    },
};

#[test]
fn a_visible_bore_passes_the_segment_in_all_six_directions_without_rounding() {
    let solid = RefinedVolume::uniform(Voxel::new(Material::Wood));
    for axis in 0..3 {
        let mut minimum = [127; 3];
        minimum[axis] = 0;
        let mut maximum = [129; 3];
        maximum[axis] = 256;
        let (bored, _) = solid
            .replace_box(
                LocalBox::new(minimum, maximum).unwrap(),
                Voxel::AIR,
                VolumeLimits::default(),
            )
            .unwrap();
        for reverse in [false, true] {
            let mut start = [500_000; 3];
            start[axis] = -500_000;
            let mut end = [500_000; 3];
            end[axis] = 1_500_000;
            if reverse {
                std::mem::swap(&mut start, &mut end);
            }
            assert_eq!(
                solid
                    .trace_segment(start, end, RayLimits::default())
                    .unwrap()
                    .chords
                    .len(),
                1
            );
            assert!(
                bored
                    .trace_segment(start, end, RayLimits::default())
                    .unwrap()
                    .chords
                    .is_empty()
            );
            // The bore ends at precisely 503906.25 um. Integer endpoints on either side differ
            // by one micrometre; rounding the material plane would lose this cover distinction.
            for (coordinate, covered) in [(503_906, false), (503_907, true)] {
                start[(axis + 1) % 3] = coordinate;
                end[(axis + 1) % 3] = coordinate;
                assert_eq!(
                    !bored
                        .trace_segment(start, end, RayLimits::default())
                        .unwrap()
                        .chords
                        .is_empty(),
                    covered
                );
            }
        }
    }
}

fn layers() -> RefinedVolume {
    let mut volume = RefinedVolume::uniform(Voxel::AIR);
    for (low, high, material) in [
        (31, 32, Material::Glass),
        (63, 67, Material::Wood),
        (191, 192, Material::Steel),
    ] {
        volume = volume
            .replace_box(
                LocalBox::new([low, 0, 0], [high, 256, 256]).unwrap(),
                Voxel::new(material),
                VolumeLimits::default(),
            )
            .unwrap()
            .0;
    }
    volume
}

#[test]
fn material_chords_are_ordered_exact_and_preserve_thickness_and_air_gaps() {
    let volume = layers();
    let start = [-500_000, 500_000, 500_000];
    let end = [1_500_000, 500_000, 500_000];
    let trace = volume
        .trace_segment(start, end, RayLimits::default())
        .unwrap();
    assert_eq!(trace.chords.len(), 3);
    for (chord, (low, high, material)) in trace.chords.iter().zip([
        (31, 32, Material::Glass),
        (63, 67, Material::Wood),
        (191, 192, Material::Steel),
    ]) {
        assert_eq!(chord.leaf.voxel().material, material);
        assert_eq!(
            chord.entry,
            SegmentParameter {
                numerator: low + 128,
                denominator: 512
            }
        );
        assert_eq!(
            chord.exit,
            SegmentParameter {
                numerator: high + 128,
                denominator: 512
            }
        );
    }
    let reverse = volume
        .trace_segment(end, start, RayLimits::default())
        .unwrap();
    let materials: Vec<_> = reverse
        .chords
        .iter()
        .map(|hit| hit.leaf.voxel().material)
        .collect();
    assert_eq!(
        materials,
        vec![Material::Steel, Material::Wood, Material::Glass]
    );
    assert!(
        trace
            .chords
            .windows(2)
            .all(|pair| pair[0].exit < pair[1].entry)
    );
}

#[test]
fn parallel_boundaries_tangency_and_endpoint_only_contact_have_explicit_semantics() {
    let solid = RefinedVolume::uniform(Voxel::new(Material::Wood));
    for (start, end, count) in [
        ([-1_000_000, 0, 0], [2_000_000, 0, 0], 1),
        ([-1_000_000, 1_000_000, 0], [2_000_000, 1_000_000, 0], 0),
        ([-1_000_000, 0, 0], [0, 0, 0], 0),
        ([1_000_000, 0, 0], [2_000_000, 0, 0], 0),
        ([1_000_000, 0, 0], [0, 0, 0], 1),
        ([-1_000_000, 0, 0], [0, 1_000_000, 0], 0),
        (
            [-MAX_SEGMENT_COORDINATE_UM, 500_000, 500_000],
            [MAX_SEGMENT_COORDINATE_UM, 500_000, 500_000],
            1,
        ),
    ] {
        assert_eq!(
            solid
                .trace_segment(start, end, RayLimits::default())
                .unwrap()
                .chords
                .len(),
            count
        );
    }
}

#[test]
fn hostile_segments_and_every_exhausted_budget_refuse_without_partial_trace() {
    let volume = layers();
    let fingerprint = volume.fingerprint();
    let start = [-500_000, 500_000, 500_000];
    let end = [1_500_000, 500_000, 500_000];
    let trace = volume
        .trace_segment(start, end, RayLimits::default())
        .unwrap();
    for visits in 1..trace.visits {
        assert_eq!(
            volume
                .trace_segment(
                    start,
                    end,
                    RayLimits {
                        hits: MAX_RAY_HITS,
                        visits
                    }
                )
                .unwrap_err(),
            VolumeError::VisitBudget
        );
    }
    for hits in 1..trace.chords.len() {
        assert_eq!(
            volume
                .trace_segment(
                    start,
                    end,
                    RayLimits {
                        hits,
                        visits: MAX_RAY_VISITS
                    }
                )
                .unwrap_err(),
            VolumeError::RayBudget
        );
    }
    assert!(
        volume
            .trace_segment(
                start,
                end,
                RayLimits {
                    hits: 3,
                    visits: trace.visits
                }
            )
            .is_ok()
    );
    for invalid in [
        [i32::MIN; 3],
        [i32::MAX; 3],
        [MAX_SEGMENT_COORDINATE_UM + 1, 0, 0],
        [-MAX_SEGMENT_COORDINATE_UM - 1, 0, 0],
    ] {
        assert_eq!(
            volume
                .trace_segment(invalid, end, RayLimits::default())
                .unwrap_err(),
            VolumeError::InvalidSegment
        );
    }
    assert_eq!(
        volume
            .trace_segment(start, start, RayLimits::default())
            .unwrap_err(),
        VolumeError::InvalidSegment
    );
    for limits in [
        RayLimits { hits: 0, visits: 1 },
        RayLimits { hits: 1, visits: 0 },
        RayLimits {
            hits: MAX_RAY_HITS + 1,
            visits: 1,
        },
        RayLimits {
            hits: 1,
            visits: MAX_RAY_VISITS + 1,
        },
    ] {
        assert_eq!(
            volume.trace_segment(start, end, limits).unwrap_err(),
            VolumeError::InvalidLimits
        );
    }
    assert_eq!(volume.fingerprint(), fingerprint);
}

#[test]
fn random_segments_match_dense_event_midpoint_oracle_on_regular_and_irregular_grids() {
    for grid in [&REGULAR, &IRREGULAR] {
        let (volume, dense) = random_fixture(grid);
        let mut seed = 0x791f_6bc2_u32;
        for _ in 0..128 {
            let mut coordinate = || {
                seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                i32::try_from(seed % 2_000_001).unwrap() - 500_000
            };
            let start = std::array::from_fn(|_| coordinate());
            let end = std::array::from_fn(|_| coordinate());
            assert_dense_oracle(&volume, &dense, grid, start, end);
        }
    }
}

#[test]
fn maximum_leaf_diagonal_and_128_run_axial_queries_stay_within_ray_budgets() {
    let checker =
        RefinedVolume::decode(&crate::volume::tests::checkerboard([128, 8, 8], false)).unwrap();
    assert_eq!(checker.leaves().len(), crate::volume::MAX_VOLUME_LEAVES);
    let axial = checker
        .trace_segment(
            [-1, 500_000, 500_000],
            [1_000_001, 500_000, 500_000],
            RayLimits::default(),
        )
        .unwrap();
    assert_eq!(axial.chords.len(), 128);
    assert!(axial.chords.windows(2).all(|p| p[0].exit == p[1].entry));
    let start = [-500_000, -250_000, -125_000];
    let end = [1_500_000, 1_250_000, 1_125_000];
    let diagonal = checker
        .trace_segment(start, end, RayLimits::default())
        .unwrap();
    assert!(diagonal.visits <= MAX_RAY_VISITS && diagonal.chords.len() <= MAX_RAY_HITS);
    assert!(!diagonal.chords.is_empty());
    assert_eq!(
        checker
            .trace_segment(
                start,
                end,
                RayLimits {
                    visits: diagonal.visits - 1,
                    ..RayLimits::default()
                }
            )
            .unwrap_err(),
        VolumeError::VisitBudget
    );
}

fn assert_dense_oracle(
    volume: &RefinedVolume,
    dense: &[Voxel; 512],
    grid: &[u16; 9],
    start: [i32; 3],
    end: [i32; 3],
) {
    let trace = volume
        .trace_segment(start, end, RayLimits::default())
        .unwrap();
    assert!(
        trace
            .chords
            .windows(2)
            .all(|pair| pair[0].exit <= pair[1].entry)
    );
    // Independent algorithm: enumerate all grid-crossing times, sort them and sample each open
    // interval at its rational midpoint. No leaf AABB intersection/query code is reused.
    let mut events = vec![(0_i128, 1_i128), (1, 1)];
    for axis in 0..3 {
        let delta = i128::from(end[axis] - start[axis]) * 256;
        if delta == 0 {
            continue;
        }
        for &plane in grid {
            let mut numerator = i128::from(plane) * 1_000_000 - i128::from(start[axis]) * 256;
            let mut denominator = delta;
            if denominator < 0 {
                numerator = -numerator;
                denominator = -denominator;
            }
            if (0..=denominator).contains(&numerator) {
                events.push((numerator, denominator));
            }
        }
    }
    events.sort_unstable_by(|a, b| (a.0 * b.1).cmp(&(b.0 * a.1)));
    events.dedup_by(|a, b| a.0 * b.1 == b.0 * a.1);
    for pair in events.windows(2) {
        let n = pair[0].0 * pair[1].1 + pair[1].0 * pair[0].1;
        let d = 2 * pair[0].1 * pair[1].1;
        let point: [i128; 3] = std::array::from_fn(|axis| {
            i128::from(start[axis]) * 256 * d + i128::from(end[axis] - start[axis]) * 256 * n
        });
        let expected = if point.iter().any(|&p| p < 0 || p >= 256 * 1_000_000 * d) {
            Voxel::AIR
        } else {
            let cell = point.map(|p| {
                let unit = u16::try_from(p / (1_000_000 * d)).unwrap();
                grid.partition_point(|&v| v <= unit) - 1
            });
            dense[cell[0] + 8 * (cell[1] + 8 * cell[2])]
        };
        let actual: Vec<_> = trace
            .chords
            .iter()
            .filter(|hit| {
                i128::from(hit.entry.numerator()) * d <= n * i128::from(hit.entry.denominator())
                    && n * i128::from(hit.exit.denominator()) < i128::from(hit.exit.numerator()) * d
            })
            .collect();
        if expected.is_solid() {
            assert_eq!(actual.len(), 1, "missing/duplicate chord at midpoint");
            assert_eq!(actual[0].leaf.voxel(), expected);
        } else {
            assert!(actual.is_empty(), "invented cover through air/outside page");
        }
    }
}
