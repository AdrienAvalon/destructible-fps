//! Whole-scene publication and content budgets for the native ruined-factory reference scene.
//! These are authored inspection stages, not evidence of weapon or structural simulation.
use destructible_fps::{
    CHUNK_EDGE, IVec3,
    mesh::{
        CpuMesh,
        fine::{
            FineMeshLimits, FineMeshReport, MAX_FINE_MESH_INDICES, MAX_FINE_MESH_VERTICES,
            finishes::SurfaceFinishes,
            fixture::{
                STAGE_NAMES, industrial_patch_positions, industrial_reference_world,
                reference_surface_finishes,
            },
            hybrid_dirty_chunks, mesh_hybrid_chunks_with_finishes,
        },
    },
    world::geometry::RefinedWorld,
};
use std::collections::{BTreeMap, BTreeSet};

fn same_mesh(first: &CpuMesh, second: &CpuMesh) -> bool {
    bytemuck::cast_slice::<_, u8>(&first.vertices)
        == bytemuck::cast_slice::<_, u8>(&second.vertices)
        && first.indices == second.indices
}

fn bounded_mesh(
    world: &RefinedWorld,
    chunk: IVec3,
    finishes: &SurfaceFinishes,
    stage: usize,
) -> (CpuMesh, FineMeshReport) {
    // Match the native inspector's one-chunk jobs, without increasing any engine limit.
    let limits = FineMeshLimits::default();
    let mut batch = mesh_hybrid_chunks_with_finishes(world, &[chunk], limits, finishes)
        .unwrap_or_else(|error| panic!("reference stage {stage} chunk {chunk:?}: {error}"));
    let report = batch.report;
    assert!(report.vertices <= limits.vertices);
    assert!(report.indices <= limits.indices);
    assert!(report.quads <= limits.quads);
    assert!(report.work <= limits.work);
    assert!(report.lines <= limits.lines);
    assert_eq!(batch.meshes.len(), 1);
    let (actual_chunk, mesh) = batch.meshes.pop().unwrap();
    assert_eq!(actual_chunk, chunk);
    assert_eq!(report.vertices, mesh.vertices.len());
    assert_eq!(report.indices, mesh.indices.len());
    (mesh, report)
}

fn assert_unchanged_source_outside_patch(first: &RefinedWorld, world: &RefinedWorld, stage: usize) {
    let patch: BTreeSet<_> = industrial_patch_positions().into_iter().collect();
    let chunks: BTreeSet<_> = first
        .chunk_positions()
        .into_iter()
        .chain(world.chunk_positions())
        .collect();
    let mut changed = 0;
    let mut unchanged_solids = 0;
    // Inspect both old and new chunk sets: checking refined positions alone would miss a
    // scenic page canonicalized to uniform, a changed ordinary wall, or a newly added chunk.
    for chunk in chunks {
        for z in 0..CHUNK_EDGE {
            for y in 0..CHUNK_EDGE {
                for x in 0..CHUNK_EDGE {
                    let position = IVec3::new(
                        chunk.x * CHUNK_EDGE + x,
                        chunk.y * CHUNK_EDGE + y,
                        chunk.z * CHUNK_EDGE + z,
                    );
                    let old = first.cell(position);
                    let current = world.cell(position);
                    if old != current {
                        changed += 1;
                        assert!(
                            patch.contains(&position),
                            "reference stage {stage} changed scenic source at {position:?}"
                        );
                    } else if !patch.contains(&position) && current.solid_units() != 0 {
                        unchanged_solids += 1;
                    }
                }
            }
        }
    }
    assert!(
        unchanged_solids > 1_000,
        "source coverage must include the surrounding map"
    );
    if stage > 0 {
        assert!(
            changed > 0,
            "stage {stage} must change the actual source, not only a render key"
        );
    }
}

#[test]
fn reference_stages_preserve_scenic_sources_finishes_and_geometry_headroom() {
    let first = industrial_reference_world(0).unwrap();
    let finish_key = reference_surface_finishes(&first).unwrap().fingerprint();
    assert_ne!(
        finish_key, 0,
        "the reference scene must exercise explicit cut finishes"
    );
    let mut fingerprints = BTreeSet::new();
    for stage in 0..STAGE_NAMES.len() {
        let world = industrial_reference_world(stage).unwrap();
        assert!(world.chunk_positions().len() > 100);
        assert!(
            fingerprints.insert(world.fingerprint()),
            "stage {stage} repeats a physical world"
        );
        assert_eq!(
            reference_surface_finishes(&world).unwrap().fingerprint(),
            finish_key,
            "stage {stage} changed immutable scenic finish provenance"
        );
        let stats = world.geometry_stats();
        assert!(
            stats.refined_leaves < 16_384,
            "stage {stage} must retain the existing content guard: {stats:?}"
        );
        assert_unchanged_source_outside_patch(&first, &world, stage);
        println!("REFERENCE_SOURCE stage={stage} {stats:?} finish_key={finish_key:032x}");
    }
    assert!(industrial_reference_world(STAGE_NAMES.len()).is_err());
}

#[test]
fn every_reference_stage_matches_full_remeshing_with_unchanged_job_and_resident_caps() {
    let dirty = hybrid_dirty_chunks(&industrial_patch_positions()).unwrap();
    let first = industrial_reference_world(0).unwrap();
    let finishes = reference_surface_finishes(&first).unwrap();
    let baseline: BTreeMap<_, _> = first
        .chunk_positions()
        .into_iter()
        .map(|chunk| (chunk, bounded_mesh(&first, chunk, &finishes, 0).0))
        .collect();
    assert!(
        baseline.len() > 100,
        "exercise the complete map, not only its fine patch"
    );
    for stage in 0..STAGE_NAMES.len() {
        let world = industrial_reference_world(stage).unwrap();
        let stage_finishes = reference_surface_finishes(&world).unwrap();
        assert_eq!(stage_finishes.fingerprint(), finishes.fingerprint());
        let replacements: BTreeMap<_, _> = dirty
            .iter()
            .map(|&chunk| (chunk, bounded_mesh(&world, chunk, &stage_finishes, stage).0))
            .collect();
        let dirty_vertices: usize = replacements.values().map(|mesh| mesh.vertices.len()).sum();
        let dirty_indices: usize = replacements.values().map(|mesh| mesh.indices.len()).sum();
        assert!(dirty_vertices <= MAX_FINE_MESH_VERTICES);
        assert!(dirty_indices <= MAX_FINE_MESH_INDICES);
        let positions: BTreeSet<_> = world
            .chunk_positions()
            .into_iter()
            .chain(baseline.keys().copied())
            .chain(dirty.iter().copied())
            .collect();
        let mut changed = 0;
        let mut unchanged_nonempty = 0;
        let (mut vertices, mut indices, mut max_work) = (0, 0, 0);
        for chunk in positions {
            // Reuse the original immutable finish sidecar, as native stage publication does.
            let (mesh, report) = bounded_mesh(&world, chunk, &finishes, stage);
            vertices += report.vertices;
            indices += report.indices;
            max_work = max_work.max(report.work);
            let published = replacements.get(&chunk).or_else(|| baseline.get(&chunk));
            assert!(
                published.map_or(
                    mesh.vertices.is_empty() && mesh.indices.is_empty(),
                    |candidate| same_mesh(candidate, &mesh)
                ),
                "stage {stage} published mesh differs from a full remesh at {chunk:?}"
            );
            let same = baseline
                .get(&chunk)
                .map_or(mesh.vertices.is_empty() && mesh.indices.is_empty(), |old| {
                    same_mesh(old, &mesh)
                });
            if !same {
                changed += 1;
                assert!(
                    dirty.contains(&chunk),
                    "stage {stage} changed clean chunk {chunk:?}"
                );
            } else if !dirty.contains(&chunk) && !mesh.indices.is_empty() {
                unchanged_nonempty += 1;
            }
        }
        if stage > 0 {
            assert!(changed > 0, "stage {stage} must change rendered geometry");
        }
        assert!(
            unchanged_nonempty > 40,
            "unchanged coverage must include occupied chunks"
        );
        // Stricter content headroom than the unchanged 524288/1572864 inspector resident caps.
        assert!(
            vertices < 400_000 && indices < 1_200_000,
            "reference stage {stage} exceeds content headroom: vertices={vertices} indices={indices}"
        );
        println!(
            "REFERENCE_RESIDENT stage={stage} vertices={vertices} indices={indices} max_job_work={max_work} dirty_vertices={dirty_vertices} dirty_indices={dirty_indices}"
        );
    }
}
