//! Composite-source checks independent of the convex implementation's local fixtures.
use destructible_fps::{
    FixedMicrometers3, IVec3, Material, Voxel, World,
    ballistics::FixedRay,
    convex::{
        ConvexError, ConvexFaceInput, ConvexFragment, ConvexQueryBudget, ConvexQueryLimits,
        InspectionGeometry, SceneOwner, fixture::industrial_scene,
    },
    mesh::fine::{
        FineMeshLimits,
        fixture::{industrial_oblique_base, industrial_reference_world, oblique_surface_finishes},
        mesh_hybrid_chunks_with_finishes,
    },
    volume::{VOLUME_UNITS, ray::SegmentParameter},
    world::{
        geometry::{GeometryCell, GeometryChange, GeometryState, RefinedWorld},
        query::{PhysicalBox, QueryBudget, QueryLimits, ray::TraceLimits},
    },
};
use std::sync::Arc;

fn convex_budget() -> ConvexQueryBudget {
    ConvexQueryBudget::new(ConvexQueryLimits::default()).unwrap()
}
fn world_budget() -> QueryBudget {
    QueryBudget::new(QueryLimits::default()).unwrap()
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
fn cuboid(origin: IVec3, size: [u16; 3]) -> ConvexFragment {
    let [x, y, z] = size;
    let vertices = [
        [0, 0, 0],
        [x, 0, 0],
        [x, y, 0],
        [0, y, 0],
        [0, 0, z],
        [x, 0, z],
        [x, y, z],
        [0, y, z],
    ];
    let faces = [
        [0, 3, 2, 1],
        [4, 5, 6, 7],
        [0, 4, 7, 3],
        [1, 2, 6, 5],
        [0, 1, 5, 4],
        [3, 7, 6, 2],
    ]
    .into_iter()
    .map(|indices| ConvexFaceInput {
        indices: indices.to_vec(),
        cut: false,
    })
    .collect::<Vec<_>>();
    ConvexFragment::new(origin, &vertices, &faces, Voxel::new(Material::Concrete)).unwrap()
}
fn ground_and_wall() -> Arc<RefinedWorld> {
    let mut world = World::default();
    world.fill_box(
        IVec3::new(-4, 0, -4),
        IVec3::new(8, 0, 4),
        Voxel::new(Material::Stone),
    );
    world.fill_box(
        IVec3::new(5, 1, 0),
        IVec3::new(5, 2, 0),
        Voxel::new(Material::Brick),
    );
    Arc::new(RefinedWorld::from_uniform(&world).unwrap())
}
fn fraction_equals(value: SegmentParameter, numerator: i64, denominator: i64) {
    assert_eq!(
        i128::from(value.numerator()) * i128::from(denominator),
        i128::from(numerator) * i128::from(value.denominator())
    );
}

#[test]
fn scene_identity_is_order_independent_and_rays_choose_the_actual_nearest_source() {
    let world = ground_and_wall();
    let a = cuboid(IVec3::new(2, 1, 0), [256; 3]);
    let b = cuboid(IVec3::new(1, 1, 2), [128; 3]);
    let scene = InspectionGeometry::new(world.clone(), vec![a.clone(), b.clone()]).unwrap();
    let reversed = InspectionGeometry::new(world.clone(), vec![b, a]).unwrap();
    assert_eq!(scene.fingerprint(), reversed.fingerprint());
    assert_eq!(scene.fragments(), reversed.fragments());
    assert_ne!(
        scene.fingerprint(),
        InspectionGeometry::new(world.clone(), vec![])
            .unwrap()
            .fingerprint()
    );
    assert!(Arc::ptr_eq(scene.world(), &world));
    let forward = scene
        .trace(
            &ray([0, 1_500_000, 500_000], [1, 0, 0], 7_000_000),
            TraceLimits::default(),
            &mut convex_budget(),
        )
        .unwrap();
    assert_eq!(forward.source_fingerprint, scene.fingerprint());
    assert_eq!(forward.chords.len(), 2);
    assert!(matches!(forward.chords[0].owner, SceneOwner::Fragment(_)));
    assert_eq!(
        forward.chords[1].owner,
        SceneOwner::World(IVec3::new(5, 1, 0))
    );
    fraction_equals(forward.chords[0].entry, 2, 7);
    fraction_equals(forward.chords[0].exit, 3, 7);
    fraction_equals(forward.chords[1].entry, 5, 7);
    fraction_equals(forward.chords[1].exit, 6, 7);
    let backward = scene
        .trace(
            &ray([7_000_000, 1_500_000, 500_000], [-1, 0, 0], 7_000_000),
            TraceLimits::default(),
            &mut convex_budget(),
        )
        .unwrap();
    assert_eq!(
        backward.chords[0].owner,
        SceneOwner::World(IVec3::new(5, 1, 0))
    );
    assert!(matches!(backward.chords[1].owner, SceneOwner::Fragment(_)));
    fraction_equals(backward.chords[0].entry, 1, 7);
    fraction_equals(backward.chords[1].entry, 4, 7);
}

#[test]
fn construction_rejects_overlapping_interiors_but_accepts_face_contact() {
    let world = ground_and_wall();
    let a = cuboid(IVec3::new(2, 1, 0), [256; 3]);
    let b = cuboid(IVec3::new(3, 1, 0), [256; 3]);
    assert!(InspectionGeometry::new(world.clone(), vec![a.clone(), b]).is_ok());
    assert!(matches!(
        InspectionGeometry::new(world.clone(), vec![a.clone(), a]),
        Err(ConvexError::Overlap)
    ));
    assert!(matches!(
        InspectionGeometry::new(world.clone(), vec![cuboid(IVec3::new(5, 1, 0), [256; 3])]),
        Err(ConvexError::Overlap)
    ));
    assert!(matches!(
        InspectionGeometry::new(world, vec![cuboid(IVec3::new(0, 0, 0), [128; 3])]),
        Err(ConvexError::Overlap)
    ));
}

#[test]
fn full_fragment_capacity_and_invalid_trace_limits_are_explicit() {
    let fragments = (0..32)
        .map(|index| cuboid(IVec3::new(index * 2, 1, 0), [128; 3]))
        .collect::<Vec<_>>();
    let world = Arc::new(RefinedWorld::default());
    let scene = InspectionGeometry::new(world.clone(), fragments.clone()).unwrap();
    assert_eq!(scene.fragments().len(), 32);
    let mut excessive = fragments;
    excessive.push(cuboid(IVec3::new(64, 1, 0), [128; 3]));
    assert!(matches!(
        InspectionGeometry::new(world, excessive),
        Err(ConvexError::Budget)
    ));
    assert!(matches!(
        scene.trace(
            &ray([0, 1_250_000, 250_000], [1, 0, 0], 64_000_000),
            TraceLimits {
                chords: 0,
                ..TraceLimits::default()
            },
            &mut convex_budget()
        ),
        Err(ConvexError::WorldTrace(
            destructible_fps::world::query::ray::TraceError::InvalidLimits
        ))
    ));
}

#[test]
fn shared_world_fragment_coplanar_rays_do_not_create_double_thickness() {
    let above = InspectionGeometry::new(
        ground_and_wall(),
        vec![cuboid(IVec3::new(2, 1, 0), [256; 3])],
    )
    .unwrap();
    let top = above
        .trace(
            &ray([0, 1_000_000, 500_000], [1, 0, 0], 4_000_000),
            TraceLimits::default(),
            &mut convex_budget(),
        )
        .unwrap();
    // World's upper face is open and convex lower face is strict: no material chord.
    assert!(top.chords.is_empty());
    let below = InspectionGeometry::new(
        ground_and_wall(),
        vec![cuboid(IVec3::new(2, -1, 0), [256; 3])],
    )
    .unwrap();
    let bottom = below
        .trace(
            &ray([0, 0, 500_000], [1, 0, 0], 4_000_000),
            TraceLimits::default(),
            &mut convex_budget(),
        )
        .unwrap();
    // World's minimum-closed ownership is retained, with no additional convex surface chord.
    assert!(!bottom.chords.is_empty());
    assert!(
        bottom
            .chords
            .iter()
            .all(|hit| matches!(hit.owner, SceneOwner::World(_)))
    );
    assert!(
        bottom
            .chords
            .windows(2)
            .all(|pair| pair[0].exit <= pair[1].entry)
    );
}

#[test]
fn combined_sweeps_test_the_full_motion_against_both_sources() {
    let scene = InspectionGeometry::new(
        ground_and_wall(),
        vec![cuboid(IVec3::new(2, 1, 0), [256; 3])],
    )
    .unwrap();
    let start =
        PhysicalBox::from_micrometers([0, 1_250_000, 250_000], [500_000, 1_750_000, 750_000])
            .unwrap();
    let hit = scene
        .sweep_axis(
            start,
            0,
            7_000_000,
            &mut world_budget(),
            &mut convex_budget(),
        )
        .unwrap();
    assert!(hit.contact);
    assert_eq!(hit.displacement_um, 1_500_000);
    let reverse = PhysicalBox::from_micrometers(
        [6_500_000, 1_250_000, 250_000],
        [7_000_000, 1_750_000, 750_000],
    )
    .unwrap();
    let hit = scene
        .sweep_axis(
            reverse,
            0,
            -7_000_000,
            &mut world_budget(),
            &mut convex_budget(),
        )
        .unwrap();
    assert!(hit.contact);
    assert_eq!(hit.displacement_um, -500_000);
    let touching = PhysicalBox::from_micrometers(
        [1_500_000, 1_250_000, 250_000],
        [2_000_000, 1_750_000, 750_000],
    )
    .unwrap();
    assert!(
        !scene
            .overlaps(touching, &mut world_budget(), &mut convex_budget())
            .unwrap()
    );
    let hit = scene
        .sweep_axis(touching, 0, 1, &mut world_budget(), &mut convex_budget())
        .unwrap();
    assert!(hit.contact);
    assert_eq!(hit.displacement_um, 0);
    let away = scene
        .sweep_axis(touching, 0, -1, &mut world_budget(), &mut convex_budget())
        .unwrap();
    assert!(!away.contact);
    assert_eq!(away.displacement_um, -1);
}

#[test]
fn source_hits_never_hide_budget_failure_or_return_a_partial_trace() {
    let scene = InspectionGeometry::new(
        ground_and_wall(),
        vec![
            cuboid(IVec3::new(2, 1, 0), [256; 3]),
            cuboid(IVec3::new(1, 1, 2), [128; 3]),
        ],
    )
    .unwrap();
    let tiny = || {
        ConvexQueryBudget::new(ConvexQueryLimits {
            fragments: 1,
            ..ConvexQueryLimits::default()
        })
        .unwrap()
    };
    let forward = ray([0, 1_500_000, 500_000], [1, 0, 0], 7_000_000);
    assert!(matches!(
        scene.trace(&forward, TraceLimits::default(), &mut tiny()),
        Err(ConvexError::Budget)
    ));
    assert!(matches!(
        scene.trace(
            &forward,
            TraceLimits {
                chords: 1,
                ..TraceLimits::default()
            },
            &mut convex_budget()
        ),
        Err(ConvexError::Budget)
    ));
    // The base world already overlaps this box: convex exhaustion still invalidates the answer.
    let wall = PhysicalBox::from_micrometers(
        [5_100_000, 1_100_000, 100_000],
        [5_200_000, 1_200_000, 200_000],
    )
    .unwrap();
    assert!(matches!(
        scene.overlaps(wall, &mut world_budget(), &mut tiny()),
        Err(ConvexError::Budget)
    ));
    let start =
        PhysicalBox::from_micrometers([0, 1_250_000, 250_000], [500_000, 1_750_000, 750_000])
            .unwrap();
    assert!(matches!(
        scene.sweep_axis(start, 0, 7_000_000, &mut world_budget(), &mut tiny()),
        Err(ConvexError::Budget)
    ));
}

#[test]
fn industrial_scene_has_no_hidden_stepped_apron_and_its_slabs_match_real_support_faces() {
    let mut identities = Vec::new();
    let mut source_fragments = None;
    for stage in 0..4 {
        let base = Arc::new(industrial_oblique_base(stage).unwrap());
        let scene = industrial_scene(base.clone()).unwrap();
        assert_eq!(scene.fragments().len(), 24);
        if let Some(expected) = &source_fragments {
            assert_eq!(scene.fragments(), expected);
        } else {
            source_fragments = Some(scene.fragments().to_vec());
        }
        assert!(Arc::ptr_eq(scene.world(), &base));
        identities.push(scene.fingerprint());
        for x in -24..=-9 {
            for z in 17..=24 {
                for y in 1..=4 {
                    assert_eq!(
                        base.cell(IVec3::new(x, y, z)),
                        GeometryCell::AIR,
                        "no legacy geometry below convex solids at {x},{y},{z}"
                    );
                }
            }
        }
        let caps: Vec<_> = scene
            .fragments()
            .iter()
            .filter(|f| f.faces().iter().any(|face| !face.cut()))
            .collect();
        assert_eq!(caps.len(), 4);
        for cap in caps {
            let bottom = cap
                .faces()
                .iter()
                .zip(cap.planes())
                .find(|(_, p)| p.normal()[1] < 0)
                .unwrap();
            let mut underside: Vec<_> = bottom
                .0
                .indices()
                .iter()
                .map(|&i| cap.vertices()[usize::from(i)])
                .collect();
            underside.sort_unstable();
            assert!(
                scene
                    .fragments()
                    .iter()
                    .filter(|other| other.origin() == cap.origin() && *other != cap)
                    .any(|support| support.faces().iter().zip(support.planes()).any(
                        |(face, plane)| {
                            let mut top: Vec<_> = face
                                .indices()
                                .iter()
                                .map(|&i| support.vertices()[usize::from(i)])
                                .collect();
                            top.sort_unstable();
                            plane.normal() == bottom.1.normal().map(|v| -v)
                                && plane.offset() == -bottom.1.offset()
                                && top == underside
                        }
                    )),
                "entire underside must match an actual support face"
            );
        }
    }
    identities.sort_unstable();
    identities.dedup();
    assert_eq!(
        identities.len(),
        4,
        "world stage contributes to composite identity"
    );
    assert!(
        industrial_scene(Arc::new(industrial_reference_world(0).unwrap())).is_err(),
        "old material apron must not remain hidden underneath the new convex scene"
    );
}

#[test]
fn missing_ground_in_the_middle_of_a_large_support_is_refused() {
    let original = industrial_oblique_base(0).unwrap();
    let position = IVec3::new(-21, 0, 18);
    assert_eq!(original.cell(position).solid_units(), VOLUME_UNITS);
    let mut state = GeometryState::new(original.clone(), 1).unwrap();
    let tx = state
        .prepare(
            original.tick(),
            vec![GeometryChange {
                position,
                before: original.cell(position),
                after: GeometryCell::AIR,
            }],
        )
        .unwrap();
    state.apply(&tx).unwrap();
    let fingerprint = state.world().fingerprint();
    assert!(industrial_scene(Arc::new(state.world().clone())).is_err());
    assert_eq!(state.world().fingerprint(), fingerprint);
}

#[test]
fn actual_approach_eye_can_query_visible_oblique_material_without_voxel_proxy_hits() {
    let scene = industrial_scene(Arc::new(industrial_oblique_base(3).unwrap())).unwrap();
    // Rounded micrometre coordinates of the native approach camera: yaw=-0.25, distance=14,
    // eye y=2.65. This inspection ray aims from that eye at the left foreground slab.
    let eye = [-18_463_656, 2_650_000, 29_364_774];
    let eye_box = PhysicalBox::from_micrometers(eye, eye.map(|v| v + 1)).unwrap();
    assert!(
        !scene
            .overlaps(eye_box, &mut world_budget(), &mut convex_budget())
            .unwrap()
    );
    let probe = ray(eye, [-2_036, -900, -11_365], 13_000_000);
    let trace = scene
        .trace(&probe, TraceLimits::default(), &mut convex_budget())
        .unwrap();
    assert!(matches!(
        trace.chords.first().map(|hit| hit.owner),
        Some(SceneOwner::Fragment(_))
    ));
    assert!(trace.chords.iter().all(|hit| hit.entry < hit.exit));
    let upward = scene
        .trace(
            &ray([-20_500_000, 1_001_000, 18_000_000], [0, 1, 0], 2_000_000),
            TraceLimits::default(),
            &mut convex_budget(),
        )
        .unwrap();
    assert_eq!(
        upward.chords.len(),
        2,
        "support and concrete cap share one exact interface"
    );
    assert!(
        upward
            .chords
            .iter()
            .all(|hit| matches!(hit.owner, SceneOwner::Fragment(_)))
    );
    assert_eq!(upward.chords[0].exit, upward.chords[1].entry);
    let span = upward.chords[1];
    let a = span.entry;
    let b = span.exit;
    assert_eq!(
        (i128::from(b.numerator()) * i128::from(a.denominator())
            - i128::from(a.numerator()) * i128::from(b.denominator()))
            * 2_000_000,
        187_500 * i128::from(a.denominator()) * i128::from(b.denominator())
    );
}

#[test]
fn every_oblique_stage_counts_world_and_fragment_geometry_under_unchanged_resident_caps() {
    // Independent frozen contracts from the native inspector: these resident limits are NOT
    // the smaller single-job/replacement limits. A body upload must not escape their accounting.
    const RESIDENT_VERTICES: usize = 524_288;
    const RESIDENT_INDICES: usize = 1_572_864;
    let limits = FineMeshLimits::default();
    assert_eq!((limits.vertices, limits.indices), (262_144, 786_432));
    let mut expected_fragments = None;
    let mut expected_finish = None;
    for stage in 0..4 {
        let world = Arc::new(industrial_oblique_base(stage).unwrap());
        let scene = industrial_scene(world.clone()).unwrap();
        let finishes = oblique_surface_finishes(&world).unwrap();
        if let Some(expected) = expected_finish {
            assert_eq!(finishes.fingerprint(), expected);
        } else {
            expected_finish = Some(finishes.fingerprint());
        }
        let chunks = world.chunk_positions();
        assert!(
            chunks.len() > 100,
            "full map, not just visible or dirty chunks"
        );
        let (mut world_vertices, mut world_indices, mut max_work, mut occupied) = (0, 0, 0, 0);
        for chunk in &chunks {
            let mut batch = mesh_hybrid_chunks_with_finishes(&world, &[*chunk], limits, &finishes)
                .unwrap_or_else(|error| panic!("oblique stage {stage} chunk {chunk:?}: {error}"));
            let report = batch.report;
            assert!(report.vertices <= limits.vertices && report.indices <= limits.indices);
            assert!(report.quads <= limits.quads && report.work <= limits.work);
            assert!(report.lines <= limits.lines);
            assert_eq!(batch.meshes.len(), 1);
            let (actual, mesh) = batch.meshes.pop().unwrap();
            assert_eq!(actual, *chunk);
            assert_eq!(
                (mesh.vertices.len(), mesh.indices.len()),
                (report.vertices, report.indices)
            );
            world_vertices += mesh.vertices.len();
            world_indices += mesh.indices.len();
            max_work = max_work.max(report.work);
            occupied += usize::from(!mesh.indices.is_empty());
        }
        assert!(
            occupied > 40,
            "surrounding geometry contributes to the resident total"
        );
        let mut fragment_identity = Vec::new();
        let (mut fragment_vertices, mut fragment_indices) = (0, 0);
        for (index, fragment) in scene.fragments().iter().enumerate() {
            let body = fragment.body_mesh(u64::try_from(index + 1).unwrap());
            let vertices: usize = fragment
                .faces()
                .iter()
                .map(|face| face.indices().len())
                .sum();
            let indices: usize = fragment
                .faces()
                .iter()
                .map(|face| (face.indices().len() - 2) * 3)
                .sum();
            assert_eq!(
                (body.mesh.vertices.len(), body.mesh.indices.len()),
                (vertices, indices)
            );
            assert!(!body.mesh.vertices.is_empty() && !body.mesh.indices.is_empty());
            fragment_vertices += vertices;
            fragment_indices += indices;
            fragment_identity.push((fragment.fingerprint(), vertices, indices));
        }
        assert_eq!(fragment_identity.len(), 24);
        if let Some(expected) = &expected_fragments {
            assert_eq!(&fragment_identity, expected);
        } else {
            expected_fragments = Some(fragment_identity);
        }
        let vertices = world_vertices + fragment_vertices;
        let indices = world_indices + fragment_indices;
        assert!(vertices > world_vertices && indices > world_indices);
        assert!(vertices <= RESIDENT_VERTICES && indices <= RESIDENT_INDICES);
        // Retain the stricter established map-content headroom, now INCLUDING convex pieces.
        assert!(
            vertices < 400_000 && indices < 1_200_000,
            "oblique stage {stage} total vertices={vertices} indices={indices}"
        );
        println!(
            "OBLIQUE_RESIDENT stage={stage} chunks={} occupied={occupied} world_vertices={world_vertices} world_indices={world_indices} fragment_vertices={fragment_vertices} fragment_indices={fragment_indices} total_vertices={vertices} total_indices={indices} max_job_work={max_work}",
            chunks.len()
        );
    }
}
