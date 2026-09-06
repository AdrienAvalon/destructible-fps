//! Frozen normal-treatment geometry oracle, deliberately separate from the evolving map art.
use super::{IVec3, industrial_patch_positions, install_wall, ruins};
use crate::mesh::{
    CpuMesh,
    fine::{FineMeshLimits, hybrid_dirty_chunks, mesh_hybrid_chunks},
};

fn geometry_signature(meshes: &[(IVec3, CpuMesh)]) -> u128 {
    // Regression checksum only, not authentication. Normals are deliberately excluded.
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
fn normal_treatment_preserves_exact_original_masonry_and_rubble_geometry() {
    let dirty = hybrid_dirty_chunks(&industrial_patch_positions()).unwrap();
    // Measured from c62c96a BEFORE normal reconstruction; no checksum regeneration.
    let expected = [
        0x7f8b_46ce_4e6e_53fe_7a43_0ac7_bbed_aecf,
        0x85be_4a74_cbbb_53b8_7244_cbda_814b_8902,
        0x4be9_5aa4_9d22_ba69_3775_0d61_86a8_ca2e,
        0xef78_897e_10e1_2f79_998f_2daa_6744_5538,
    ];
    for (stage, expected) in expected.into_iter().enumerate() {
        let masonry = install_wall(
            &crate::WorldPreset::Industrial.build(),
            stage,
            IVec3::new(-17, 1, 15),
            true,
        )
        .unwrap();
        let original = ruins::courtyard(&masonry).unwrap();
        let batch = mesh_hybrid_chunks(&original, &dirty, FineMeshLimits::default()).unwrap();
        assert_eq!(geometry_signature(&batch.meshes), expected);
    }
}
