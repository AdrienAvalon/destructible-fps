//! Render finishes cannot silently mutate physical source or other GPU attributes.
use destructible_fps::{
    IVec3, Material,
    mesh::fine::{
        FineMeshError, FineMeshLimits,
        finishes::{CUT_CORE_MARKER, FinishPolicy, MAX_FINISH_CELLS, SurfaceFinishes},
        fixture::{
            industrial_inspection_world, industrial_patch_positions, industrial_surface_finishes,
        },
        hybrid_dirty_chunks, mesh_hybrid_chunks, mesh_hybrid_chunks_with_finishes,
    },
    world::geometry::{GeometryCell, GeometryChange, GeometryState, RefinedWorld},
};

#[test]
fn authored_finishes_change_only_markers_and_never_mix_inside_triangles() {
    let fingerprints = [
        0xa31f_c6be_dc94_9804_6ab5_129d_6b7c_7b26_u128,
        0xdce1_84dc_3a53_64c4_2977_1b22_4ae2_7ff5,
        0x169f_63c4_e190_2400_054d_8020_0e9c_d3e0,
        0xb8f3_d69f_8722_e4ef_a9fb_0ded_3540_5e86,
    ];
    let first = industrial_inspection_world(0).unwrap();
    let finishes = industrial_surface_finishes(&first).unwrap();
    assert_ne!(finishes.fingerprint(), 0);
    let mut count = 0;
    for (stage, fingerprint) in fingerprints.into_iter().enumerate() {
        let world = industrial_inspection_world(stage).unwrap();
        assert_eq!(
            world.fingerprint(),
            fingerprint,
            "finishes preserve the authored polygon rubble source revision"
        );
        assert_eq!(
            finishes.fingerprint(),
            industrial_surface_finishes(&world).unwrap().fingerprint()
        );
        for chunk in hybrid_dirty_chunks(&industrial_patch_positions()).unwrap() {
            let plain = mesh_hybrid_chunks(&world, &[chunk], FineMeshLimits::default()).unwrap();
            let styled = mesh_hybrid_chunks_with_finishes(
                &world,
                &[chunk],
                FineMeshLimits::default(),
                &finishes,
            )
            .unwrap();
            assert_eq!(plain.report.vertices, styled.report.vertices);
            assert_eq!(plain.report.indices, styled.report.indices);
            for ((p, a), (q, mut b)) in plain.meshes.into_iter().zip(styled.meshes) {
                assert_eq!(p, q);
                assert_eq!(a.indices, b.indices);
                for triangle in b.indices.chunks_exact(3) {
                    let marker = b.vertices[triangle[0] as usize].fracture_depth.to_bits();
                    assert!(
                        triangle
                            .iter()
                            .all(|i| b.vertices[*i as usize].fracture_depth.to_bits() == marker)
                    );
                }
                for vertex in &mut b.vertices {
                    if vertex.fracture_depth.to_bits() == CUT_CORE_MARKER.to_bits() {
                        if vertex.normal[1] <= 0.5 {
                            assert_authored_cut_side(vertex);
                        }
                        assert!(
                            vertex.material == u32::from(Material::Brick as u8)
                                || vertex.material == u32::from(Material::Concrete as u8)
                        );
                        assert_eq!(vertex.damage.to_bits(), 0.0_f32.to_bits());
                        vertex.fracture_depth = -1.0;
                        count += 1;
                    }
                }
                assert_eq!(
                    bytemuck::cast_slice::<_, u8>(&a.vertices),
                    bytemuck::cast_slice::<_, u8>(&b.vertices)
                );
            }
        }
        assert_eq!(world.fingerprint(), fingerprint);
    }
    assert!(count > 1000);
    println!(
        "FINISH_MARKERS vertices_across_stages={count} fingerprint={:032x}",
        finishes.fingerprint()
    );
}

fn assert_authored_cut_side(vertex: &destructible_fps::mesh::Vertex) {
    // Independent page and original-plane oracle, not the production authoring registry.
    assert!(vertex.normal[0].abs() > 0.99 || vertex.normal[2].abs() > 0.99);
    assert!((1.0..2.0).contains(&vertex.position[1]));
    let (i, (x, z)) = [
        (-19_i16, 16_i16),
        (-18, 16),
        (-16, 16),
        (-12, 16),
        (-11, 17),
        (-18, 17),
        (-17, 18),
        (-16, 18),
        (-14, 18),
        (-12, 19),
        (-14, 20),
        (-18, 21),
    ]
    .into_iter()
    .enumerate()
    .find(|(_, (x, z))| {
        (f32::from(*x)..f32::from(*x + 1)).contains(&vertex.position[0])
            && (f32::from(*z)..f32::from(*z + 1)).contains(&vertex.position[2])
    })
    .expect("only twelve large authored fragments have cut sides");
    let (axis, sign, plane) = match i % 4 {
        0 => (2, 1.0_f32, 208.0),
        1 => (0, -1.0, 48.0),
        2 => (2, -1.0, 48.0),
        _ => (0, 1.0, 208.0),
    };
    let coordinate = f32::from(if axis == 0 { x } else { z }) + plane / 256.0;
    assert!(
        !(vertex.normal[axis].to_bits() == sign.to_bits()
            && vertex.position[axis].to_bits() == coordinate.to_bits()),
        "original flat skin is not cut core"
    );
}

#[test]
fn invalid_lists_stale_sources_and_work_exhaustion_fail_without_geometry_changes() {
    let world = industrial_inspection_world(0).unwrap();
    let p = IVec3::new(-17, 1, 16);
    for positions in [
        vec![p, p],
        vec![p; MAX_FINISH_CELLS + 1],
        vec![IVec3::new(0, 0, 0)],
        vec![IVec3::new(0, 200, 0)],
        vec![IVec3::new(-8, 7, 15)],
        vec![p, IVec3::new(-19, 1, 16)],
    ] {
        assert!(SurfaceFinishes::cut_tops(&world, &positions).is_err());
    }
    let finishes = SurfaceFinishes::cut_tops(&world, &[p]).unwrap();
    let chunks = hybrid_dirty_chunks(&[p]).unwrap();
    assert!(matches!(
        mesh_hybrid_chunks_with_finishes(
            &world,
            &chunks[..1],
            FineMeshLimits {
                work: 1,
                ..FineMeshLimits::default()
            },
            &finishes
        ),
        Err(FineMeshError::WorkBudget)
    ));
    let mut state = GeometryState::new(world.clone(), 1).unwrap();
    let tx = state
        .prepare(
            world.tick(),
            vec![GeometryChange {
                position: p,
                before: world.cell(p),
                after: GeometryCell::AIR,
            }],
        )
        .unwrap();
    state.apply(&tx).unwrap();
    let fingerprint = state.world().fingerprint();
    assert!(matches!(
        mesh_hybrid_chunks_with_finishes(
            state.world(),
            &chunks[..1],
            FineMeshLimits::default(),
            &finishes
        ),
        Err(FineMeshError::SurfaceFinish)
    ));
    assert_eq!(state.world().fingerprint(), fingerprint);
    assert_eq!(
        SurfaceFinishes::cut_tops(&world, &[])
            .unwrap()
            .fingerprint(),
        0
    );
    let plain = mesh_hybrid_chunks(&world, &chunks[..1], FineMeshLimits::default()).unwrap();
    let empty = mesh_hybrid_chunks_with_finishes(
        &world,
        &chunks[..1],
        FineMeshLimits::default(),
        &SurfaceFinishes::default(),
    )
    .unwrap();
    assert_eq!(plain.meshes[0].1.indices, empty.meshes[0].1.indices);
    assert_eq!(
        bytemuck::cast_slice::<_, u8>(&plain.meshes[0].1.vertices),
        bytemuck::cast_slice::<_, u8>(&empty.meshes[0].1.vertices)
    );
}

#[test]
fn scheduler_carries_finishes_and_rejects_stale_source_without_stopping_worker() {
    use destructible_fps::mesh_scheduler::{CompletedMeshJob, MeshScheduler};
    use std::{
        sync::Arc,
        time::{Duration, Instant},
    };
    let world = Arc::new(industrial_inspection_world(0).unwrap());
    let finishes = Arc::new(industrial_surface_finishes(&world).unwrap());
    let scheduler = MeshScheduler::new();
    for source in [
        Arc::new(industrial_inspection_world(1).unwrap()),
        Arc::new(RefinedWorld::default()),
        Arc::clone(&world),
    ] {
        scheduler
            .submit_hybrid_with_finishes(
                Arc::clone(&source),
                vec![IVec3::new(-1, 0, 1)],
                FineMeshLimits::default(),
                Arc::clone(&finishes),
            )
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        let job = loop {
            if let Some(job) = scheduler.poll().unwrap() {
                break job;
            }
            assert!(Instant::now() < deadline);
            std::thread::yield_now();
        };
        let CompletedMeshJob::Fine {
            world_fingerprint,
            finishes_fingerprint,
            result,
        } = job
        else {
            panic!("wrong mesh kind")
        };
        assert_eq!(world_fingerprint, source.fingerprint());
        assert_eq!(finishes_fingerprint, finishes.fingerprint());
        if source.geometry_stats().chunks == 0 {
            assert!(matches!(result, Err(FineMeshError::SurfaceFinish)));
        } else {
            assert!(result.is_ok());
        }
    }
}

#[test]
fn finish_leaf_budget_cannot_be_bypassed_with_valid_unique_pages() {
    let original = industrial_inspection_world(0).unwrap();
    let cell = original.cell(IVec3::new(-17, 1, 16));
    let positions: Vec<_> = (0..64).map(|x| IVec3::new(x, 0, 0)).collect();
    let mut state = GeometryState::new(RefinedWorld::default(), 1).unwrap();
    for batch in positions.chunks(8) {
        let tx = state
            .prepare(
                1,
                batch
                    .iter()
                    .map(|&position| GeometryChange {
                        position,
                        before: GeometryCell::AIR,
                        after: cell.clone(),
                    })
                    .collect(),
            )
            .unwrap();
        state.apply(&tx).unwrap();
    }
    assert!(
        state.world().geometry_stats().refined_leaves
            > destructible_fps::mesh::fine::finishes::MAX_FINISH_LEAVES
    );
    assert!(SurfaceFinishes::cut_tops(state.world(), &positions).is_err());
    let policies: Vec<_> = positions
        .into_iter()
        .map(|p| {
            (
                p,
                FinishPolicy::CutTopAndSides(destructible_fps::volume::surface::Face::PositiveZ),
            )
        })
        .collect();
    assert!(SurfaceFinishes::with_policies(state.world(), &policies).is_err());
}
