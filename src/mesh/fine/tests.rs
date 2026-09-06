#![allow(clippy::float_cmp, clippy::cast_possible_truncation)] // Exact binary lattice oracle.
use super::*;
use crate::{
    Material, World, chunk_position,
    volume::VolumeLimits,
    world::geometry::{GeometryChange, GeometryState, RefinedWorld},
};
use std::collections::{BTreeMap, BTreeSet};

#[test]
fn authored_stages_fit_the_actual_worker_budget_and_are_closed() {
    let mut fingerprints = BTreeSet::new();
    for stage in 0..fixture::STAGE_NAMES.len() {
        let world = fixture::inspection_world(stage).unwrap();
        fingerprints.insert(world.fingerprint());
        let mut chunks = world.chunk_positions();
        chunks.sort_unstable();
        let batch = build(&world, &chunks);
        assert!(closed(&batch) > 0.0);
        println!("inspection stage={stage} {:?}", batch.report);
    }
    assert_eq!(fingerprints.len(), fixture::STAGE_NAMES.len());
}

#[test]
fn closed_corner_contact_contributes_canonical_edge_cuts() {
    let world = install(
        &World::default(),
        vec![(
            IVec3::new(-1, -1, 0),
            slab([255, 255, 47], [256, 256, 213], Material::Steel),
        )],
    );
    let meter = WorkMeter::new(MAX_FINE_MESH_WORK);
    let mut builder = Builder {
        finishes: None,
        world: &world,
        work: &meter,
        limits: FineMeshLimits::default(),
        report: FineMeshReport::default(),
        lines: HashMap::new(),
    };
    let line = Line {
        axis: 2,
        origin: [0, 0, 0],
    };
    let cuts = builder.cuts(line).unwrap();
    assert!(cuts[0] && cuts[47] && cuts[213] && cuts[256]);
    let work = meter.used.get();
    assert_eq!(cuts, builder.cuts(line).unwrap());
    assert_eq!(meter.used.get(), work + 1);
    assert_eq!(builder.report.lines, 1);
}

fn install(world: &World, edits: Vec<(IVec3, RefinedVolume)>) -> RefinedWorld {
    let mut state = GeometryState::new(RefinedWorld::from_uniform(world).unwrap(), 1).unwrap();
    let mut changes: Vec<_> = edits
        .into_iter()
        .map(|(position, volume)| GeometryChange {
            position,
            before: state.world().cell(position),
            after: GeometryCell::refined(volume),
        })
        .collect();
    changes.sort_by_key(|c| c.position);
    let tx = state.prepare(0, changes).unwrap();
    state.apply(&tx).unwrap();
    state.world().clone()
}

fn slab(min: [u16; 3], max: [u16; 3], material: Material) -> RefinedVolume {
    RefinedVolume::uniform(Voxel::AIR)
        .replace_box(
            LocalBox::new(min, max).unwrap(),
            Voxel::new(material),
            VolumeLimits::default(),
        )
        .unwrap()
        .0
}

fn build(world: &impl StaticGeometry, chunks: &[IVec3]) -> FineMeshBatch {
    mesh_fine_chunks(world, chunks, FineMeshLimits::default()).unwrap()
}

// Weld by exact half-lattice coordinates. Every triangle edge must have two opposite owners.
// This catches T junctions as well as missing faces, duplicated sheets, winding and zero areas.
fn closed(batch: &FineMeshBatch) -> f64 {
    let mut edges = BTreeMap::new();
    let mut volume = 0.0;
    for (_, mesh) in &batch.meshes {
        for tri in mesh.indices.chunks_exact(3) {
            let points: [glam::DVec3; 3] = std::array::from_fn(|i| {
                glam::DVec3::from_array(mesh.vertices[tri[i] as usize].position.map(f64::from))
            });
            let cross = (points[1] - points[0]).cross(points[2] - points[0]);
            assert!(cross.length_squared() > 0.0);
            let normal =
                glam::DVec3::from_array(mesh.vertices[tri[0] as usize].normal.map(f64::from));
            assert!(cross.dot(normal) > 0.0, "inward triangle");
            volume += points[0].dot(points[1].cross(points[2])) / 6.0;
            let p = points.map(|p| p.to_array().map(|v| (v * 512.0) as i64));
            for i in 0..3 {
                let (a, b) = (p[i], p[(i + 1) % 3]);
                let (key, sign) = if a < b { ((a, b), 1_i32) } else { ((b, a), -1) };
                let entry = edges.entry(key).or_insert((0, 0));
                entry.0 += 1;
                entry.1 += sign;
            }
        }
    }
    for (edge, count) in edges {
        assert_eq!(count, (2, 0), "unmatched edge {edge:?}");
    }
    volume
}

#[test]
fn bore_is_closed_with_exact_volume_and_no_internal_material_sheet() {
    let full = RefinedVolume::uniform(Voxel::new(Material::Brick));
    let bore = full
        .replace_box(
            LocalBox::new([63, 71, 0], [193, 189, 256]).unwrap(),
            Voxel::AIR,
            VolumeLimits::default(),
        )
        .unwrap()
        .0;
    let world = install(&World::default(), vec![(IVec3::default(), bore)]);
    let batch = build(&world, &[IVec3::default()]);
    let expected = 1.0 - 130.0 * 118.0 / 65_536.0;
    assert!((closed(&batch) - expected).abs() < 1e-10);
    assert_eq!(batch.report.vertices, batch.meshes[0].1.vertices.len());
    assert_eq!(batch.report.indices, batch.meshes[0].1.indices.len());

    let layered = full
        .replace_box(
            LocalBox::new([0, 0, 73], [256, 256, 256]).unwrap(),
            Voxel::new(Material::Concrete),
            VolumeLimits::default(),
        )
        .unwrap()
        .0;
    let world = install(&World::default(), vec![(IVec3::default(), layered)]);
    let batch = build(&world, &[IVec3::default()]);
    assert!((closed(&batch) - 1.0).abs() < 1e-10);
    assert!(batch.meshes[0].1.vertices.iter().all(|v| {
        let axis = v.normal.iter().position(|n| *n != 0.0).unwrap();
        v.position[axis] == 0.0 || v.position[axis] == 1.0
    }));
}

#[test]
fn fine_uniform_and_fine_fine_chunk_seams_are_conforming_in_six_directions() {
    for axis in 0..3 {
        for sign in [-1, 1] {
            for uniform in [false, true] {
                let mut p = [0; 3];
                p[axis] = if sign > 0 { 15 } else { -16 };
                let a = IVec3::new(p[0], p[1], p[2]);
                p[axis] += sign;
                let b = IVec3::new(p[0], p[1], p[2]);
                let mut minimum = [19, 31, 47];
                let mut maximum = [231, 217, 193];
                minimum[axis] = 0;
                maximum[axis] = 256;
                let fine = slab(minimum, maximum, Material::Brick);
                let mut coarse = World::default();
                let mut edits = vec![(a, fine)];
                if uniform {
                    coarse.set_voxel(b, Voxel::new(Material::Concrete));
                } else {
                    minimum[(axis + 1) % 3] += 17;
                    maximum[(axis + 2) % 3] -= 13;
                    edits.push((b, slab(minimum, maximum, Material::Wood)));
                }
                let world = install(&coarse, edits);
                let chunks: Vec<_> = [chunk_position(a), chunk_position(b)]
                    .into_iter()
                    .collect::<BTreeSet<_>>()
                    .into_iter()
                    .collect();
                let together = build(&world, &chunks);
                let separately = FineMeshBatch {
                    meshes: chunks
                        .iter()
                        .flat_map(|p| build(&world, &[*p]).meshes)
                        .collect(),
                    report: FineMeshReport::default(),
                };
                assert_eq!(closed(&together), closed(&separately));
                for ((_, a), (_, b)) in together.meshes.iter().zip(&separately.meshes) {
                    assert_eq!(
                        bytemuck::cast_slice::<_, u8>(&a.vertices),
                        bytemuck::cast_slice::<_, u8>(&b.vertices)
                    );
                    assert_eq!(a.indices, b.indices);
                }
            }
        }
    }
}

#[test]
fn refusing_any_aggregate_budget_never_returns_a_partial_batch() {
    let mut world = World::default();
    world.set_voxel(IVec3::default(), Voxel::new(Material::Wood));
    world.set_voxel(IVec3::new(16, 0, 0), Voxel::new(Material::Wood));
    let chunks = [IVec3::default(), IVec3::new(1, 0, 0)];
    let first = build(&world, &chunks[..1]);
    for limits in [
        FineMeshLimits {
            vertices: first.report.vertices,
            ..FineMeshLimits::default()
        },
        FineMeshLimits {
            indices: first.report.indices,
            ..FineMeshLimits::default()
        },
        FineMeshLimits {
            quads: first.report.quads,
            ..FineMeshLimits::default()
        },
        FineMeshLimits {
            work: first.report.work,
            ..FineMeshLimits::default()
        },
        FineMeshLimits {
            lines: first.report.lines,
            ..FineMeshLimits::default()
        },
    ] {
        assert!(mesh_fine_chunks(&world, &chunks, limits).is_err());
    }
    assert_eq!(
        mesh_fine_chunks(&world, &[], FineMeshLimits::default()).unwrap_err(),
        FineMeshError::ChunkSet
    );
    assert_eq!(
        mesh_fine_chunks(&world, &[chunks[0]; 2], FineMeshLimits::default()).unwrap_err(),
        FineMeshError::ChunkSet
    );
    assert_eq!(
        mesh_fine_chunks(
            &world,
            &chunks,
            FineMeshLimits {
                work: MAX_FINE_MESH_WORK + 1,
                ..FineMeshLimits::default()
            }
        )
        .unwrap_err(),
        FineMeshError::InvalidLimits
    );
}

#[test]
fn exact_half_lattice_positions_at_precision_limit_and_extreme_rejection() {
    for cell in [-16_384, 16_383] {
        let p = IVec3::new(cell, cell, cell);
        let world = install(
            &World::default(),
            vec![(p, slab([1, 3, 5], [2, 4, 6], Material::Steel))],
        );
        let batch = build(&world, &[chunk_position(p)]);
        assert!(
            batch.meshes[0]
                .1
                .vertices
                .iter()
                .all(|v| v.position.iter().all(|f| f.is_finite()))
        );
        // Translation-invariant non-degeneracy even where general absolute world meshes lose detail.
        for tri in batch.meshes[0].1.indices.chunks_exact(3) {
            let pts: [glam::Vec3; 3] = std::array::from_fn(|i| {
                glam::Vec3::from_array(batch.meshes[0].1.vertices[tri[i] as usize].position)
            });
            assert!((pts[1] - pts[0]).cross(pts[2] - pts[0]).length_squared() > 0.0);
        }
    }
    for c in [i32::MIN, -1025, 1024, i32::MAX] {
        assert_eq!(
            mesh_fine_chunks(
                &World::default(),
                &[IVec3::new(c, 0, 0)],
                FineMeshLimits::default()
            )
            .unwrap_err(),
            FineMeshError::CoordinatePrecision
        );
    }
}
