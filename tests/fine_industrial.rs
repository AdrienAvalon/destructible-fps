//! Full-scene evidence for the inspection's incremental dirty-region publication contract.
use destructible_fps::mesh::fine::{
    FineMeshLimits,
    fixture::{STAGE_NAMES, industrial_inspection_world, industrial_patch_positions},
    hybrid_dirty_chunks, mesh_hybrid_chunks,
};
use destructible_fps::{
    IVec3, Material,
    world::geometry::{GeometryCell, GeometryChange, GeometryState, RefinedWorld},
};
use std::collections::BTreeMap;

fn replacement_meshes(
    world: &RefinedWorld,
    chunks: &[IVec3],
) -> Vec<(IVec3, destructible_fps::mesh::CpuMesh)> {
    // Same one-chunk job partition as the native industrial inspector; every job keeps its cap.
    chunks
        .iter()
        .flat_map(|chunk| {
            mesh_hybrid_chunks(world, &[*chunk], FineMeshLimits::default())
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

fn geometry_signature(
    meshes: &[(destructible_fps::IVec3, destructible_fps::mesh::CpuMesh)],
) -> u128 {
    // A fixed regression checksum, NOT an authentication or integrity primitive. Normals are
    // deliberately excluded; ordered positions/indices capture the exact pre-treatment geometry.
    let mut signature = 0x6c62_272e_07bb_0142_62b8_2175_6295_c58d_u128;
    let mut add = |bytes: &[u8]| {
        for byte in bytes {
            signature = (signature ^ u128::from(*byte))
                .wrapping_mul(0x0000_0000_0100_0000_0000_0000_0000_013b);
        }
    };
    for (position, mesh) in meshes {
        for coordinate in [position.x, position.y, position.z] {
            add(&coordinate.to_le_bytes());
        }
        add(&u64::try_from(mesh.vertices.len()).unwrap().to_le_bytes());
        add(&u64::try_from(mesh.indices.len()).unwrap().to_le_bytes());
        for vertex in &mesh.vertices {
            for coordinate in vertex.position {
                add(&coordinate.to_bits().to_le_bytes());
            }
        }
        for index in &mesh.indices {
            add(&index.to_le_bytes());
        }
    }
    signature
}

#[test]
fn normal_treatment_preserves_exact_industrial_geometry() {
    let dirty = hybrid_dirty_chunks(&industrial_patch_positions()).unwrap();
    // Measured from c62c96a BEFORE the normal reconstruction was installed.
    let expected = [
        0x7f8b_46ce_4e6e_53fe_7a43_0ac7_bbed_aecf,
        0x85be_4a74_cbbb_53b8_7244_cbda_814b_8902,
        0x4be9_5aa4_9d22_ba69_3775_0d61_86a8_ca2e,
        0xef78_897e_10e1_2f79_998f_2daa_6744_5538,
    ];
    for (stage, expected) in expected.into_iter().enumerate() {
        let current = industrial_inspection_world(stage).unwrap();
        // Isolate the pre-fenestration fixture for this NORMAL-only oracle, not for full-scene
        // acceptance. Windows occupy previously AIR cells, proved separately against coarse source.
        // No production legacy mode; the real map and all publication tests below keep every frame.
        let mut legacy = GeometryState::new(current.clone(), 1).unwrap();
        for pages in window_positions(&current).chunks(256) {
            let changes = pages
                .iter()
                .map(|position| GeometryChange {
                    position: *position,
                    before: current.cell(*position),
                    after: GeometryCell::AIR,
                })
                .collect();
            let tx = legacy.prepare(current.tick(), changes).unwrap();
            legacy.apply(&tx).unwrap();
        }
        let batch = mesh_hybrid_chunks(legacy.world(), &dirty, FineMeshLimits::default()).unwrap();
        assert_eq!(geometry_signature(&batch.meshes), expected);
    }
}

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
        let grouped: BTreeMap<_, _> = replacement_meshes(&world, &dirty).into_iter().collect();
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
    let rubble: Vec<_> = baseline.refined_positions().filter(|p| p.z > 15).collect();
    assert_eq!(rubble.len(), 8);
    let windows = window_positions(&baseline);
    assert!(windows.len() > 300);
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
        let stats = world.geometry_stats();
        // The ground-reaching final aperture empties three entire wall pages; they canonicalize
        // back to AIR, while all eight permanent rubble pages remain refined.
        assert_eq!(
            stats.refined_pages,
            windows.len() + if stage == 3 { 17 } else { 20 }
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
            let report = mesh_hybrid_chunks(&world, &[chunk], FineMeshLimits::default())
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
