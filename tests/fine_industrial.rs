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
