//! Full-scene evidence for the inspection's incremental dirty-region publication contract.
use destructible_fps::mesh::fine::{
    FineMeshLimits,
    fixture::{
        STAGE_NAMES, industrial_inspection_world, industrial_patch_positions,
        industrial_surface_finishes,
    },
    hybrid_dirty_chunks, mesh_hybrid_chunks_with_finishes,
};
use destructible_fps::{IVec3, Material, world::geometry::RefinedWorld};
use std::collections::BTreeMap;

fn replacement_meshes(
    world: &RefinedWorld,
    chunks: &[IVec3],
) -> Vec<(IVec3, destructible_fps::mesh::CpuMesh)> {
    let finishes = industrial_surface_finishes(world).unwrap();
    // Same one-chunk job partition as the native industrial inspector; every job keeps its cap.
    chunks
        .iter()
        .flat_map(|chunk| {
            mesh_hybrid_chunks_with_finishes(world, &[*chunk], FineMeshLimits::default(), &finishes)
                .unwrap()
                .meshes
        })
        .collect()
}

fn window_positions(world: &RefinedWorld) -> Vec<IVec3> {
    let mut positions: Vec<_> = world
        .refined_positions()
        .filter(|position| {
            world
                .cell(*position)
                .volume()
                .unwrap()
                .leaves()
                .iter()
                .any(|leaf| leaf.voxel().material == Material::Steel)
        })
        .collect();
    positions.sort_unstable();
    positions
}

#[test]
fn every_industrial_stage_matches_full_remeshing_outside_the_complete_dirty_region() {
    let dirty = hybrid_dirty_chunks(&industrial_patch_positions()).unwrap();
    let first = industrial_inspection_world(0).unwrap();
    let finishes = industrial_surface_finishes(&first).unwrap();
    let mut baseline = BTreeMap::new();
    for chunk in first.chunk_positions() {
        let batch = mesh_hybrid_chunks_with_finishes(
            &first,
            &[chunk],
            FineMeshLimits::default(),
            &finishes,
        )
        .unwrap();
        baseline.insert(chunk, batch.meshes.into_iter().next().unwrap().1);
    }
    assert!(
        baseline.len() > 100,
        "exercise the complete industrial map, not only its fine patch"
    );
    // Content-specific headroom under the inspector's unchanged 524288/1572864 resident caps.
    let vertices: usize = baseline.values().map(|mesh| mesh.vertices.len()).sum();
    let indices: usize = baseline.values().map(|mesh| mesh.indices.len()).sum();
    assert!(vertices < 400_000 && indices < 1_200_000);
    println!("INDUSTRIAL_RESIDENT vertices={vertices} indices={indices}");
    for stage in 1..STAGE_NAMES.len() {
        let world = industrial_inspection_world(stage).unwrap();
        let grouped: BTreeMap<_, _> = replacement_meshes(&world, &dirty).into_iter().collect();
        let mut positions = world.chunk_positions();
        positions.extend(baseline.keys());
        positions.extend(&dirty);
        positions.sort_unstable();
        positions.dedup();
        let mut unchanged_nonempty = 0;
        let mut changed = 0;
        for chunk in positions {
            let batch = mesh_hybrid_chunks_with_finishes(
                &world,
                &[chunk],
                FineMeshLimits::default(),
                &finishes,
            )
            .unwrap();
            let mesh = &batch.meshes[0].1;
            if let Some(grouped_mesh) = grouped.get(&chunk) {
                assert_eq!(
                    bytemuck::cast_slice::<_, u8>(&mesh.vertices),
                    bytemuck::cast_slice::<_, u8>(&grouped_mesh.vertices)
                );
                assert_eq!(mesh.indices, grouped_mesh.indices);
            }
            let same = baseline.get(&chunk).map_or(
                mesh.vertices.is_empty() && mesh.indices.is_empty(),
                |old| {
                    bytemuck::cast_slice::<_, u8>(&old.vertices)
                        == bytemuck::cast_slice::<_, u8>(&mesh.vertices)
                        && old.indices == mesh.indices
                },
            );
            if !same {
                changed += 1;
                assert!(
                    dirty.contains(&chunk),
                    "stage {stage} unexpectedly changed {chunk:?}"
                );
            }
            if same && !dirty.contains(&chunk) && !mesh.indices.is_empty() {
                unchanged_nonempty += 1;
            }
        }
        assert!(
            changed > 0,
            "a prepared stage must actually change the visible geometry"
        );
        assert!(
            unchanged_nonempty > 40,
            "unchanged coverage must not be only air chunks"
        );
    }
}

#[test]
fn exact_inspection_reference_is_unchanged_and_industrial_rubble_is_persistent() {
    use destructible_fps::mesh::fine::fixture::inspection_world;
    let references = [
        0x9427_f174_b861_e1a6_e9d6_77c7_d689_4ca7,
        0xd3e2_ec98_0efc_a553_d2df_cc2e_95bf_29cf,
        0x9575_4ab6_d3fd_20ef_64b2_f205_9342_ae1a,
        0xbdf6_9bc3_e6d2_0d9f_2e1f_7e0c_cbf4_0752,
    ];
    let baseline = industrial_inspection_world(0).unwrap();
    let rubble: Vec<_> = baseline
        .refined_positions()
        .filter(|p| p.y == 1 && p.z > 15)
        .collect();
    assert_eq!(rubble.len(), 20);
    let windows = window_positions(&baseline);
    assert!(windows.len() > 300);
    let apron: Vec<_> = baseline.refined_positions().filter(|p| p.y == 0).collect();
    assert_eq!(apron.len(), 566);
    for (stage, reference) in references.into_iter().enumerate() {
        assert_eq!(inspection_world(stage).unwrap().fingerprint(), reference);
        let world = industrial_inspection_world(stage).unwrap();
        for position in &rubble {
            assert_eq!(world.cell(*position), baseline.cell(*position));
        }
        assert_eq!(window_positions(&world), windows);
        for position in &windows {
            assert_eq!(world.cell(*position), baseline.cell(*position));
        }
        for position in &apron {
            assert_eq!(world.cell(*position), baseline.cell(*position));
        }
        let stats = world.geometry_stats();
        // The ground-reaching final aperture empties three entire wall pages; they canonicalize
        // back to AIR, while all twenty permanent rubble pages remain refined.
        assert_eq!(
            stats.refined_pages,
            baseline.geometry_stats().refined_pages - if stage == 3 { 3 } else { 0 }
        );
        assert!(
            stats.refined_leaves < 16_384,
            "keep headroom under the unchanged transaction cap"
        );
        let chunks = hybrid_dirty_chunks(&industrial_patch_positions()).unwrap();
        let mut max_work = 0;
        let mut vertices = 0;
        let mut indices = 0;
        let mut total_work = 0;
        for chunk in chunks {
            let report = mesh_hybrid_chunks_with_finishes(
                &world,
                &[chunk],
                FineMeshLimits::default(),
                &industrial_surface_finishes(&world).unwrap(),
            )
            .unwrap()
            .report;
            max_work = max_work.max(report.work);
            total_work += report.work;
            vertices += report.vertices;
            indices += report.indices;
        }
        assert!(vertices <= destructible_fps::mesh::fine::MAX_FINE_MESH_VERTICES);
        assert!(indices <= destructible_fps::mesh::fine::MAX_FINE_MESH_INDICES);
        println!(
            "RUIN_BUDGET stage={stage} {stats:?} max_job_work={max_work} total_work={total_work} vertices={vertices} indices={indices}"
        );
    }
}
