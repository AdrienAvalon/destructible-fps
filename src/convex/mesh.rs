//! Exact render adapter for the same canonical convex planes used by inspection queries.
//! Body upload is only an instance-buffer transport here, not a rigid-body simulation claim.
use super::shape::{ConvexFragment, MAX_CONVEX_FACE_VERTICES, MAX_CONVEX_FACES};
use crate::{
    BodyId, IVec3,
    mesh::{
        CpuBodyMesh, CpuMesh, Vertex, fine::finishes::CUT_CORE_MARKER, material_surface,
        voxel_damage,
    },
};

pub const MAX_CONVEX_MESH_VERTICES: usize = MAX_CONVEX_FACES * MAX_CONVEX_FACE_VERTICES;
pub const MAX_CONVEX_MESH_INDICES: usize = MAX_CONVEX_FACES * (MAX_CONVEX_FACE_VERTICES - 2) * 3;

impl ConvexFragment {
    /// Emits only source vertices, duplicated at hard face boundaries, and outward triangle fans.
    /// The caller must retain the matching convex source for inspection queries. This adapter
    /// neither creates a physics body nor authorizes a simulated transform or a network identity.
    ///
    /// The validated shape bounds limit output to 128 vertices and 288 indices (8,832 bytes).
    /// Allocation follows the existing infallible `CpuBodyMesh` constructor contract.
    ///
    /// # Panics
    /// Only if the private canonical shape invariants are broken; `ConvexFragment::new` rejects
    /// oversized faces, zero normals and out-of-domain coordinates before this adapter is reached.
    #[must_use]
    pub fn body_mesh(&self, id: BodyId) -> CpuBodyMesh {
        let vertices = self.faces().iter().map(|face| face.indices().len()).sum();
        let indices = self
            .faces()
            .iter()
            .map(|face| (face.indices().len() - 2) * 3)
            .sum();
        assert!(vertices <= MAX_CONVEX_MESH_VERTICES);
        assert!(indices <= MAX_CONVEX_MESH_INDICES);
        let mut mesh = CpuMesh {
            vertices: Vec::with_capacity(vertices),
            indices: Vec::with_capacity(indices),
        };
        let voxel = self.material();
        let surface = material_surface(voxel.material);
        for (face, plane) in self.faces().iter().zip(self.planes()) {
            // Cross products of q256 coordinates <=2048 fit i32 exactly; normalization affects
            // only illumination, never canonical vertices, planes or physical query bounds.
            let plane = plane
                .normal()
                .map(|v| f64::from(i32::try_from(v).expect("validated plane bound")));
            let normal = glam::DVec3::from_array(plane)
                .normalize()
                .as_vec3()
                .to_array();
            let first = u32::try_from(mesh.vertices.len()).expect("bounded convex mesh index");
            for &index in face.indices() {
                mesh.vertices.push(Vertex {
                    position: self.vertices()[usize::from(index)].map(|q| f32::from(q) / 256.0),
                    normal,
                    albedo_roughness: [surface[0], surface[1], surface[2], surface[3]],
                    ambient_occlusion: 1.0,
                    metallic: surface[4],
                    material: u32::from(voxel.material as u8),
                    damage: voxel_damage(voxel),
                    fracture_depth: if face.cut() { CUT_CORE_MARKER } else { -1.0 },
                });
            }
            for triangle in 1..face.indices().len() - 1 {
                let second = first + u32::try_from(triangle).expect("bounded convex face index");
                mesh.indices.extend_from_slice(&[first, second, second + 1]);
            }
        }
        let origin = self.origin();
        let origin_components = [origin.x, origin.y, origin.z];
        let maximum: [i32; 3] = std::array::from_fn(|axis| {
            let endpoint = self
                .vertices()
                .iter()
                .map(|q| q[axis])
                .max()
                .expect("validated vertices");
            // CpuBodyMesh.maximum is an INCLUSIVE cell. The existing renderer adds one when
            // deriving extent, so ceil(local endpoint)-1 encloses without an extra metre.
            origin_components[axis]
                .checked_add(i32::from(endpoint.div_ceil(256)) - 1)
                .expect("validated global convex bounds")
        });
        CpuBodyMesh {
            body_id: id,
            origin,
            maximum: IVec3::new(maximum[0], maximum[1], maximum[2]),
            rotation_pivot: [0.0; 3],
            mesh,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Material, Voxel, convex::shape::ConvexFaceInput};
    use glam::Vec3;
    use std::collections::BTreeMap;

    fn slab(voxel: Voxel) -> ConvexFragment {
        let vertices = [
            [128, 0, 128],
            [640, 128, 128],
            [640, 192, 640],
            [128, 64, 640],
            [128, 96, 128],
            [640, 224, 128],
            [640, 288, 640],
            [128, 160, 640],
        ];
        let faces = [
            [0, 1, 2, 3],
            [4, 7, 6, 5],
            [0, 4, 5, 1],
            [1, 5, 6, 2],
            [2, 6, 7, 3],
            [3, 7, 4, 0],
        ]
        .into_iter()
        .enumerate()
        .map(|(i, indices)| ConvexFaceInput {
            indices: indices.to_vec(),
            cut: i != 1,
        })
        .collect::<Vec<_>>();
        ConvexFragment::new(IVec3::new(-23, 0, 19), &vertices, &faces, voxel).unwrap()
    }

    fn verify_source_and_closure(fragment: &ConvexFragment) {
        let mesh = fragment.body_mesh(42);
        let mut start = 0;
        for (face, plane) in fragment.faces().iter().zip(fragment.planes()) {
            for (i, &source) in face.indices().iter().enumerate() {
                let vertex = mesh.mesh.vertices[start + i];
                let exact = fragment.vertices()[usize::from(source)].map(|q| f32::from(q) / 256.0);
                assert_eq!(vertex.position.map(f32::to_bits), exact.map(f32::to_bits));
                assert!(vertex.normal.into_iter().all(f32::is_finite));
                let n = Vec3::from_array(vertex.normal);
                assert!((n.length() - 1.0).abs() < 1e-6);
                let direction = Vec3::from_array(plane.normal().map(|v| {
                    f32::from(i16::try_from(v).expect("fixture reduced plane component"))
                }))
                .normalize();
                assert!(n.dot(direction) > 0.999_999);
            }
            start += face.indices().len();
        }
        assert_eq!(start, mesh.mesh.vertices.len());
        let mut edges = BTreeMap::new();
        for triangle in mesh.mesh.indices.chunks_exact(3) {
            let vertices: [Vertex; 3] =
                std::array::from_fn(|i| mesh.mesh.vertices[usize::try_from(triangle[i]).unwrap()]);
            let [a, b, c] = vertices.map(|vertex| Vec3::from_array(vertex.position));
            let winding = (b - a).cross(c - a);
            assert!(winding.dot(Vec3::from_array(vertices[0].normal)) > 0.0);
            assert!(
                vertices
                    .iter()
                    .all(|v| v.normal.map(f32::to_bits) == vertices[0].normal.map(f32::to_bits))
            );
            for (first, second) in [(0, 1), (1, 2), (2, 0)] {
                let first = vertices[first].position.map(f32::to_bits);
                let second = vertices[second].position.map(f32::to_bits);
                assert_ne!(first, second);
                let forward = first < second;
                let key = if forward {
                    (first, second)
                } else {
                    (second, first)
                };
                let count = edges.entry(key).or_insert([0; 2]);
                count[usize::from(forward)] += 1;
            }
        }
        assert!(
            edges.values().all(|counts| *counts == [1, 1]),
            "every triangle edge closes exactly twice with opposite winding"
        );
    }

    #[test]
    fn oblique_slab_uses_exact_shared_source_with_closed_outward_triangle_fans() {
        let fragment = slab(Voxel::new(Material::Concrete));
        verify_source_and_closure(&fragment);
        let body = fragment.body_mesh(77);
        assert_eq!(body.body_id, 77);
        assert_eq!(body.origin, IVec3::new(-23, 0, 19));
        assert_eq!(body.maximum, IVec3::new(-21, 1, 21));
        assert_eq!(
            body.rotation_pivot.map(f32::to_bits),
            [0.0_f32.to_bits(); 3]
        );
        assert_eq!(body.mesh.vertices.len(), 24);
        assert_eq!(body.mesh.indices.len(), 36);
        assert!(
            body.mesh
                .vertices
                .iter()
                .any(|v| v.normal[0] != 0.0 && v.normal[1] != 0.0 && v.normal[2] != 0.0)
        );
    }

    #[test]
    fn face_finishes_integrity_and_material_attributes_do_not_mutate_canonical_geometry() {
        for material in [Material::Brick, Material::Concrete, Material::Stone] {
            let voxel = Voxel {
                material,
                integrity: 173,
            };
            let fragment = slab(voxel);
            let original_vertices = fragment.vertices().to_vec();
            let original_bounds = fragment.bounds();
            let original_fingerprint = fragment.fingerprint();
            let body = fragment.body_mesh(1);
            let surface = material_surface(material);
            let mut offset = 0;
            let mut finishes = [0; 2];
            for face in fragment.faces() {
                for vertex in &body.mesh.vertices[offset..offset + face.indices().len()] {
                    assert_eq!(
                        vertex.fracture_depth.to_bits(),
                        if face.cut() { -2.0_f32 } else { -1.0_f32 }.to_bits()
                    );
                    assert_eq!(vertex.material, u32::from(material as u8));
                    assert_eq!(
                        vertex.albedo_roughness.map(f32::to_bits),
                        [surface[0], surface[1], surface[2], surface[3]].map(f32::to_bits)
                    );
                    assert_eq!(vertex.metallic.to_bits(), surface[4].to_bits());
                    assert_eq!(vertex.ambient_occlusion.to_bits(), 1.0_f32.to_bits());
                    assert_eq!(vertex.damage.to_bits(), voxel_damage(voxel).to_bits());
                    assert!(vertex.albedo_roughness.into_iter().all(f32::is_finite));
                    finishes[usize::from(face.cut())] += 1;
                }
                offset += face.indices().len();
            }
            assert!(finishes.into_iter().all(|count| count > 0));
            assert_eq!(fragment.vertices(), original_vertices);
            assert_eq!(fragment.bounds(), original_bounds);
            assert_eq!(fragment.material(), voxel);
            assert_eq!(fragment.fingerprint(), original_fingerprint);
        }
    }

    #[test]
    fn translated_world_vertices_stay_exact_at_both_render_domain_boundaries() {
        let source = slab(Voxel::new(Material::Brick));
        // Exercise individual q256 units, not only dyadic halves/eighths that would also pass
        // after an accidental reduction in render precision at the domain boundary.
        let vertices: Vec<_> = source.vertices().iter().map(|q| q.map(|v| v + 1)).collect();
        let faces: Vec<_> = source
            .faces()
            .iter()
            .map(|face| ConvexFaceInput {
                indices: face.indices().to_vec(),
                cut: face.cut(),
            })
            .collect();
        for coordinate in [
            -super::super::shape::MAX_CONVEX_ORIGIN,
            super::super::shape::MAX_CONVEX_ORIGIN,
        ] {
            let origin = IVec3::new(coordinate, coordinate, coordinate);
            let fragment =
                ConvexFragment::new(origin, &vertices, &faces, source.material()).unwrap();
            let body = fragment.body_mesh(7);
            let translation = Vec3::splat(f32::from(i16::try_from(coordinate).unwrap()));
            let mut index = 0;
            for face in fragment.faces() {
                for &source_index in face.indices() {
                    let source_position = fragment.vertices()[usize::from(source_index)];
                    let expected = glam::DVec3::from_array(
                        source_position.map(|q| f64::from(coordinate) + f64::from(q) / 256.0),
                    )
                    .as_vec3();
                    let transformed =
                        translation + Vec3::from_array(body.mesh.vertices[index].position);
                    assert_eq!(
                        transformed.to_array().map(f32::to_bits),
                        expected.to_array().map(f32::to_bits)
                    );
                    index += 1;
                }
            }
            assert_eq!(index, body.mesh.vertices.len());
            assert_eq!(
                body.maximum,
                IVec3::new(coordinate + 2, coordinate + 1, coordinate + 2)
            );
        }
    }

    #[test]
    fn largest_face_fans_and_eight_metre_local_endpoints_stay_inside_derived_mesh_bounds() {
        let outline = [
            [512, 0],
            [1536, 0],
            [2048, 512],
            [2048, 1536],
            [1536, 2048],
            [512, 2048],
            [0, 1536],
            [0, 512],
        ];
        let vertices: Vec<_> = [0, 2048]
            .into_iter()
            .flat_map(|y| outline.map(|[x, z]| [x, y, z]))
            .collect();
        let mut faces = vec![
            ConvexFaceInput {
                indices: (0..8).collect(),
                cut: true,
            },
            ConvexFaceInput {
                indices: (8..16).rev().collect(),
                cut: false,
            },
        ];
        for i in 0..8 {
            let next = (i + 1) % 8;
            faces.push(ConvexFaceInput {
                indices: vec![i, i + 8, next + 8, next],
                cut: true,
            });
        }
        let fragment = ConvexFragment::new(
            IVec3::new(-8, -8, -8),
            &vertices,
            &faces,
            Voxel::new(Material::Concrete),
        )
        .unwrap();
        verify_source_and_closure(&fragment);
        let body = fragment.body_mesh(u64::MAX);
        assert_eq!(body.maximum, IVec3::new(-1, -1, -1));
        assert_eq!(body.mesh.vertices.len(), 48);
        assert_eq!(body.mesh.indices.len(), 84);
        assert!(body.mesh.vertices.len() <= MAX_CONVEX_MESH_VERTICES);
        assert!(body.mesh.indices.len() <= MAX_CONVEX_MESH_INDICES);
        assert_eq!(
            MAX_CONVEX_MESH_VERTICES * size_of::<Vertex>()
                + MAX_CONVEX_MESH_INDICES * size_of::<u32>(),
            8_832
        );
        for vertex in &body.mesh.vertices {
            assert!(
                vertex
                    .position
                    .into_iter()
                    .all(|v| v.is_finite() && (0.0..=8.0).contains(&v))
            );
        }
    }
}
