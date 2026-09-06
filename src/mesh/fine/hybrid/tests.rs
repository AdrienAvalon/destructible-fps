use super::*;
use crate::{
    Material, World, WorldPreset,
    mesh::mesh_chunk,
    volume::{LocalBox, RefinedVolume, VolumeLimits},
    world::geometry::{GeometryChange, GeometryState},
};

fn install(coarse: &World, position: IVec3) -> RefinedWorld {
    let mut state = GeometryState::new(RefinedWorld::from_uniform(coarse).unwrap(), 1).unwrap();
    let volume = RefinedVolume::uniform(Voxel::new(Material::Brick))
        .replace_box(
            LocalBox::new([31, 43, 0], [211, 197, 256]).unwrap(),
            Voxel::AIR,
            VolumeLimits::default(),
        )
        .unwrap()
        .0;
    let tx = state
        .prepare(
            1,
            vec![GeometryChange {
                position,
                before: state.world().cell(position),
                after: GeometryCell::refined(volume),
            }],
        )
        .unwrap();
    state.apply(&tx).unwrap();
    state.world().clone()
}

fn same_mesh(a: &CpuMesh, b: &CpuMesh) {
    assert_eq!(
        bytemuck::cast_slice::<_, u8>(&a.vertices),
        bytemuck::cast_slice::<_, u8>(&b.vertices)
    );
    assert_eq!(a.indices, b.indices);
}

#[test]
fn uniform_sources_keep_the_existing_industrial_and_natural_mesh_bytes() {
    let coarse = WorldPreset::Industrial.build();
    let refined = RefinedWorld::from_uniform(&coarse).unwrap();
    for chunk in [
        IVec3::new(-4, 0, -1),
        IVec3::new(-1, 0, 0),
        IVec3::new(0, 0, 0),
        IVec3::new(3, 0, -1),
    ] {
        let batch = mesh_hybrid_chunks(&refined, &[chunk], FineMeshLimits::default()).unwrap();
        same_mesh(&mesh_chunk(&coarse, chunk), &batch.meshes[0].1);
    }
}

#[test]
fn collar_and_bounded_shared_work_are_explicit() {
    let p = IVec3::new(-1, 15, 0);
    let world = install(&World::default(), p);
    let chunks = hybrid_dirty_chunks(&[p]).unwrap();
    assert_eq!(chunks.len(), 8);
    let work = WorkMeter::new(super::super::MAX_FINE_MESH_WORK);
    let source = HybridSource::new(&world, &world, &chunks, &work).unwrap();
    assert_eq!(source.collar.len(), 27);
    assert!(source.uniform_voxel(p).is_none());
    for x in -1..=1 {
        for y in -1..=1 {
            for z in -1..=1 {
                assert!(!source.allows_derived(IVec3::new(p.x + x, p.y + y, p.z + z)));
            }
        }
    }
    assert!(source.allows_derived(IVec3::new(p.x + 2, p.y, p.z)));
    for limit in [1, 30, 500] {
        assert!(
            mesh_hybrid_chunks(
                &world,
                &chunks,
                FineMeshLimits {
                    work: limit,
                    ..FineMeshLimits::default()
                }
            )
            .is_err()
        );
    }
}

#[test]
fn budget_exhausted_inside_derived_queries_refuses_the_whole_candidate() {
    let mut coarse = World::default();
    coarse.set_voxel(IVec3::default(), Voxel::new(Material::Soil));
    let world = RefinedWorld::from_uniform(&coarse).unwrap();
    let fingerprint = world.fingerprint();
    assert_eq!(
        mesh_hybrid_chunks(
            &world,
            &[IVec3::default()],
            FineMeshLimits {
                work: 32,
                ..FineMeshLimits::default()
            }
        )
        .unwrap_err(),
        FineMeshError::WorkBudget
    );
    assert_eq!(world.fingerprint(), fingerprint);
}

#[test]
fn all_4096_sparse_pages_are_counted_without_building_an_irrelevant_collar() {
    use crate::world::geometry::MAX_REFINED_PAGES;
    let page = GeometryCell::refined(
        RefinedVolume::uniform(Voxel::new(Material::Brick))
            .replace_box(
                LocalBox::new([0, 0, 0], [128, 256, 256]).unwrap(),
                Voxel::AIR,
                VolumeLimits::default(),
            )
            .unwrap()
            .0,
    );
    let mut state = GeometryState::new(RefinedWorld::default(), 1).unwrap();
    for base in (0..MAX_REFINED_PAGES).step_by(MAX_GEOMETRY_CHANGES) {
        let mut changes = (base..base + MAX_GEOMETRY_CHANGES)
            .map(|index| {
                let index = i32::try_from(index).unwrap();
                GeometryChange {
                    position: IVec3::new(1024 + index % 16, index / 16 % 16, index / 256),
                    before: GeometryCell::uniform(Voxel::AIR),
                    after: page.clone(),
                }
            })
            .collect::<Vec<_>>();
        changes.sort_by_key(|c| c.position);
        let transaction = state
            .prepare(
                u64::try_from(base / MAX_GEOMETRY_CHANGES + 1).unwrap(),
                changes,
            )
            .unwrap();
        state.apply(&transaction).unwrap();
    }
    assert_eq!(state.world().refined_positions().count(), MAX_REFINED_PAGES);
    let batch = mesh_hybrid_chunks(
        state.world(),
        &[IVec3::default()],
        FineMeshLimits::default(),
    )
    .unwrap();
    assert!(batch.meshes[0].1.indices.is_empty());
    assert_eq!(batch.report.work, 1 + MAX_REFINED_PAGES * 2 + 4096);
}

#[test]
fn independently_meshed_fine_collar_matches_shared_batch_in_six_directions() {
    for axis in 0..3 {
        for sign in [-1, 1] {
            let mut p = [0; 3];
            p[axis] = if sign > 0 { 15 } else { -16 };
            let p = IVec3::new(p[0], p[1], p[2]);
            let mut coarse = World::default();
            coarse.fill_box(
                IVec3::new(p.x - 3, p.y - 3, p.z - 3),
                IVec3::new(p.x + 3, p.y + 3, p.z + 3),
                Voxel::new(Material::Soil),
            );
            let world = install(&coarse, p);
            let mut chunks = world.chunk_positions();
            chunks.sort_unstable();
            let batch = mesh_hybrid_chunks(&world, &chunks, FineMeshLimits::default()).unwrap();
            for (chunk, mesh) in batch.meshes {
                let separate =
                    mesh_hybrid_chunks(&world, &[chunk], FineMeshLimits::default()).unwrap();
                same_mesh(&mesh, &separate.meshes[0].1);
                assert!(mesh.vertices.iter().all(|v| {
                    v.position
                        .iter()
                        .chain(v.normal.iter())
                        .all(|f| f.is_finite())
                }));
            }
        }
    }
}

#[test]
fn insertion_and_removal_invalidation_cover_every_changed_chunk() {
    let mut coarse = World::default();
    coarse.fill_box(
        IVec3::new(11, 11, 11),
        IVec3::new(20, 20, 20),
        Voxel::new(Material::Stone),
    );
    coarse.fill_box(
        IVec3::new(49, 0, 49),
        IVec3::new(53, 3, 53),
        Voxel::new(Material::Soil),
    );
    let uniform = RefinedWorld::from_uniform(&coarse).unwrap();
    let changed = IVec3::new(15, 15, 15);
    let refined = install(&coarse, changed);
    let dirty = hybrid_dirty_chunks(&[changed]).unwrap();
    let mut different = 0;
    for chunk in coarse.chunk_positions() {
        let before = mesh_hybrid_chunks(&uniform, &[chunk], FineMeshLimits::default()).unwrap();
        let after = mesh_hybrid_chunks(&refined, &[chunk], FineMeshLimits::default()).unwrap();
        if bytemuck::cast_slice::<_, u8>(&before.meshes[0].1.vertices)
            != bytemuck::cast_slice::<_, u8>(&after.meshes[0].1.vertices)
        {
            different += 1;
            assert!(dirty.contains(&chunk));
        }
    }
    assert!(different > 0);
    let far = IVec3::new(3, 0, 3);
    assert!(!mesh_chunk(&coarse, far).indices.is_empty());
    same_mesh(
        &mesh_chunk(&coarse, far),
        &mesh_hybrid_chunks(&refined, &[far], FineMeshLimits::default())
            .unwrap()
            .meshes[0]
            .1,
    );
    assert!(hybrid_dirty_chunks(&[IVec3::new(i32::MAX, 0, 0)]).is_err());
}

#[test]
fn exposed_collar_occludes_seams_in_six_directions_without_sealing_the_bore() {
    use glam::Vec3;
    for axis in 0..3 {
        for sign in [-1, 1] {
            let mut origin = [0; 3];
            origin[axis] = if sign > 0 { 14 } else { -15 };
            let p = IVec3::new(origin[0], origin[1], origin[2]);
            let mut direction = [0; 3];
            direction[axis] = sign;
            let offset = |n| {
                IVec3::new(
                    p.x + direction[0] * n,
                    p.y + direction[1] * n,
                    p.z + direction[2] * n,
                )
            };
            let mut coarse = World::default();
            coarse.set_voxel(offset(1), Voxel::new(Material::Stone)); // Exact collar.
            coarse.set_voxel(offset(2), Voxel::new(Material::Stone)); // Derived surface.
            let world = install(&coarse, p);
            let mut chunks = world.chunk_positions();
            chunks.sort_unstable();
            let batch = mesh_hybrid_chunks(&world, &chunks, FineMeshLimits::default()).unwrap();
            let normal = Vec3::from_array(direction.map(|v| v as f32));
            let transverse = if axis == 2 { Vec3::X } else { Vec3::Z };
            let centre = Vec3::from_array(origin.map(|v| v as f32)) + Vec3::splat(0.5);
            // Both sides of the exact/derived edge, from both transverse viewpoints. Rays cross
            // only the exposed thin chain, not a full block which could conceal a missing seam.
            for distance in [1.49, 1.51] {
                for side in [-1.0, 1.0] {
                    let ray_origin = centre + normal * distance + transverse * (3.0 * side);
                    assert!(
                        hits(&batch, ray_origin, transverse * -side),
                        "axis={axis} sign={sign} distance={distance} side={side}"
                    );
                }
            }
            // When the neighboring chain runs transverse to the bore, neither its collar caps
            // nor the fine cell may fabricate a face across the actual through-hole.
            if axis != 2 {
                assert!(!hits(&batch, centre - Vec3::Z * 3.0, Vec3::Z));
                assert!(hits(
                    &batch,
                    centre + Vec3::X * 0.4 - Vec3::Z * 3.0,
                    Vec3::Z
                ));
            }
        }
    }
}

#[allow(clippy::many_single_char_names)]
fn hits(batch: &FineMeshBatch, origin: glam::Vec3, direction: glam::Vec3) -> bool {
    use glam::Vec3;
    batch.meshes.iter().any(|(_, mesh)| {
        mesh.indices.chunks_exact(3).any(|triangle| {
            let a = Vec3::from_array(mesh.vertices[triangle[0] as usize].position);
            let b = Vec3::from_array(mesh.vertices[triangle[1] as usize].position);
            let c = Vec3::from_array(mesh.vertices[triangle[2] as usize].position);
            let ab = b - a;
            let ac = c - a;
            let cross = direction.cross(ac);
            let determinant = ab.dot(cross);
            if determinant.abs() < 1e-6 {
                return false;
            }
            let inverse = determinant.recip();
            let offset = origin - a;
            let u = offset.dot(cross) * inverse;
            let v = direction.dot(offset.cross(ab)) * inverse;
            (0.0..=1.0).contains(&u)
                && v >= 0.0
                && u + v <= 1.0
                && ac.dot(offset.cross(ab)) * inverse >= 0.0
        })
    })
}
