//! Full-scene evidence for the inspection's incremental dirty-region publication contract.
use destructible_fps::mesh::fine::{
    FineMeshLimits,
    fixture::{STAGE_NAMES, industrial_inspection_world, industrial_patch_positions},
    hybrid_dirty_chunks, mesh_hybrid_chunks,
};
use std::collections::BTreeMap;

#[test]
fn every_industrial_stage_matches_full_remeshing_outside_the_complete_dirty_region() {
    let dirty = hybrid_dirty_chunks(&industrial_patch_positions()).unwrap();
    let first = industrial_inspection_world(0).unwrap();
    let mut baseline = BTreeMap::new();
    for chunk in first.chunk_positions() {
        let batch = mesh_hybrid_chunks(&first, &[chunk], FineMeshLimits::default()).unwrap();
        baseline.insert(chunk, batch.meshes.into_iter().next().unwrap().1);
    }
    assert!(
        baseline.len() > 100,
        "exercise the complete industrial map, not only its fine patch"
    );
    for stage in 1..STAGE_NAMES.len() {
        let world = industrial_inspection_world(stage).unwrap();
        let grouped: BTreeMap<_, _> = mesh_hybrid_chunks(&world, &dirty, FineMeshLimits::default())
            .unwrap()
            .meshes
            .into_iter()
            .collect();
        let mut positions = world.chunk_positions();
        positions.extend(baseline.keys());
        positions.extend(&dirty);
        positions.sort_unstable();
        positions.dedup();
        let mut unchanged_nonempty = 0;
        let mut changed = 0;
        for chunk in positions {
            let batch = mesh_hybrid_chunks(&world, &[chunk], FineMeshLimits::default()).unwrap();
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
        .into_iter()
        .filter(|p| p.z > 15)
        .collect();
    assert_eq!(rubble.len(), 8);
    for (stage, reference) in references.into_iter().enumerate() {
        assert_eq!(inspection_world(stage).unwrap().fingerprint(), reference);
        let world = industrial_inspection_world(stage).unwrap();
        for position in &rubble {
            assert_eq!(world.cell(*position), baseline.cell(*position));
        }
        let stats = world.geometry_stats();
        // The ground-reaching final aperture empties three entire wall pages; they canonicalize
        // back to AIR, while all eight permanent rubble pages remain refined.
        assert_eq!(stats.refined_pages, if stage == 3 { 17 } else { 20 });
        assert!(
            stats.refined_leaves < 16_384,
            "keep headroom under the unchanged transaction cap"
        );
        let chunks = hybrid_dirty_chunks(&industrial_patch_positions()).unwrap();
        let report = mesh_hybrid_chunks(&world, &chunks, FineMeshLimits::default())
            .unwrap()
            .report;
        println!("RUIN_BUDGET stage={stage} {stats:?} {report:?}");
    }
}
