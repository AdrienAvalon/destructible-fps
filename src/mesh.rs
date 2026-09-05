//! CPU chunk meshing. Only exposed voxel faces become GPU geometry.

#![allow(clippy::cast_precision_loss)]

use crate::{BodyId, CHUNK_EDGE, IVec3, Material, RigidBodyDescriptor, Voxel, World};
use bytemuck::{Pod, Zeroable};
use core::mem::size_of;
use std::collections::HashSet;

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub struct Vertex {
    pub position: [f32; 3],
    pub normal: [f32; 3],
    /// Linear-ish RGB and perceptual roughness.
    pub albedo_roughness: [f32; 4],
    pub ambient_occlusion: f32,
    /// Metalness for the renderer's Cook-Torrance material response.
    pub metallic: f32,
}

const _: () = assert!(size_of::<Vertex>() == 48);

#[derive(Debug, Default)]
pub struct CpuMesh {
    pub vertices: Vec<Vertex>,
    pub indices: Vec<u32>,
}

#[derive(Debug)]
pub struct CpuBodyMesh {
    pub body_id: BodyId,
    pub origin: IVec3,
    pub maximum: IVec3,
    /// Local-space mass centre in voxel units, used as the authoritative rotation pivot.
    pub rotation_pivot: [f32; 3],
    pub mesh: CpuMesh,
}

impl CpuMesh {
    #[must_use]
    pub const fn exposed_faces(&self) -> usize {
        self.indices.len() / 6
    }
}

struct Face {
    neighbor: IVec3,
    tangent_u: IVec3,
    tangent_v: IVec3,
    normal: [f32; 3],
    corners: [[f32; 3]; 4],
}

const FACES: [Face; 6] = [
    Face {
        neighbor: IVec3::new(1, 0, 0),
        tangent_u: IVec3::new(0, 1, 0),
        tangent_v: IVec3::new(0, 0, 1),
        normal: [1.0, 0.0, 0.0],
        corners: [
            [1.0, 0.0, 0.0],
            [1.0, 1.0, 0.0],
            [1.0, 1.0, 1.0],
            [1.0, 0.0, 1.0],
        ],
    },
    Face {
        neighbor: IVec3::new(-1, 0, 0),
        tangent_u: IVec3::new(0, 1, 0),
        tangent_v: IVec3::new(0, 0, 1),
        normal: [-1.0, 0.0, 0.0],
        corners: [
            [0.0, 0.0, 1.0],
            [0.0, 1.0, 1.0],
            [0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0],
        ],
    },
    Face {
        neighbor: IVec3::new(0, 1, 0),
        tangent_u: IVec3::new(1, 0, 0),
        tangent_v: IVec3::new(0, 0, 1),
        normal: [0.0, 1.0, 0.0],
        corners: [
            [0.0, 1.0, 1.0],
            [1.0, 1.0, 1.0],
            [1.0, 1.0, 0.0],
            [0.0, 1.0, 0.0],
        ],
    },
    Face {
        neighbor: IVec3::new(0, -1, 0),
        tangent_u: IVec3::new(1, 0, 0),
        tangent_v: IVec3::new(0, 0, 1),
        normal: [0.0, -1.0, 0.0],
        corners: [
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [1.0, 0.0, 1.0],
            [0.0, 0.0, 1.0],
        ],
    },
    Face {
        neighbor: IVec3::new(0, 0, 1),
        tangent_u: IVec3::new(1, 0, 0),
        tangent_v: IVec3::new(0, 1, 0),
        normal: [0.0, 0.0, 1.0],
        corners: [
            [1.0, 0.0, 1.0],
            [1.0, 1.0, 1.0],
            [0.0, 1.0, 1.0],
            [0.0, 0.0, 1.0],
        ],
    },
    Face {
        neighbor: IVec3::new(0, 0, -1),
        tangent_u: IVec3::new(1, 0, 0),
        tangent_v: IVec3::new(0, 1, 0),
        normal: [0.0, 0.0, -1.0],
        corners: [
            [0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [1.0, 1.0, 0.0],
            [1.0, 0.0, 0.0],
        ],
    },
];

/// Builds one chunk mesh while sampling neighbors from the complete world.
#[must_use]
pub fn mesh_chunk(world: &World, chunk: IVec3) -> CpuMesh {
    let origin = IVec3::new(
        chunk.x * CHUNK_EDGE,
        chunk.y * CHUNK_EDGE,
        chunk.z * CHUNK_EDGE,
    );
    let mut mesh = CpuMesh::default();
    for local_z in 0..CHUNK_EDGE {
        for local_y in 0..CHUNK_EDGE {
            for local_x in 0..CHUNK_EDGE {
                let position =
                    IVec3::new(origin.x + local_x, origin.y + local_y, origin.z + local_z);
                let voxel = world.voxel(position);
                if !voxel.is_solid() {
                    continue;
                }
                let occupied = |sample| world.voxel(sample).is_solid();
                if !append_voxel(&mut mesh, position, position, voxel, &occupied) {
                    return mesh;
                }
            }
        }
    }
    mesh
}

/// Builds an immutable local-space mesh for a detached body. Its origin is kept separately so the
/// renderer can move it by updating a compact instance transform instead of rewriting vertices.
#[must_use]
pub fn mesh_body(body: &RigidBodyDescriptor) -> CpuBodyMesh {
    let occupied: HashSet<_> = body
        .voxels
        .iter()
        .map(|body_voxel| body_voxel.position)
        .collect();
    let is_occupied = |sample| occupied.contains(&sample);
    let mut mesh = CpuMesh::default();
    for body_voxel in &body.voxels {
        let local = IVec3::new(
            body_voxel.position.x - body.minimum.x,
            body_voxel.position.y - body.minimum.y,
            body_voxel.position.z - body.minimum.z,
        );
        if !append_voxel(
            &mut mesh,
            body_voxel.position,
            local,
            body_voxel.voxel,
            &is_occupied,
        ) {
            break;
        }
    }
    CpuBodyMesh {
        body_id: body.id,
        origin: body.minimum,
        maximum: body.maximum,
        rotation_pivot: [
            (body.center_of_mass_mm.x - i64::from(body.minimum.x) * 1_000) as f32 / 1_000.0,
            (body.center_of_mass_mm.y - i64::from(body.minimum.y) * 1_000) as f32 / 1_000.0,
            (body.center_of_mass_mm.z - i64::from(body.minimum.z) * 1_000) as f32 / 1_000.0,
        ],
        mesh,
    }
}

fn append_voxel(
    mesh: &mut CpuMesh,
    occupancy_position: IVec3,
    vertex_position: IVec3,
    voxel: Voxel,
    occupied: &impl Fn(IVec3) -> bool,
) -> bool {
    let base_color = material_surface(voxel.material);
    let integrity = 0.65 + 0.35 * f32::from(voxel.integrity) / 255.0;
    let surface = [
        base_color[0] * integrity,
        base_color[1] * integrity,
        base_color[2] * integrity,
        base_color[3],
    ];
    for face in &FACES {
        if occupied(add(occupancy_position, face.neighbor)) {
            continue;
        }
        let Ok(first) = u32::try_from(mesh.vertices.len()) else {
            return false;
        };
        for corner in face.corners {
            mesh.vertices.push(Vertex {
                position: [
                    vertex_position.x as f32 + corner[0],
                    vertex_position.y as f32 + corner[1],
                    vertex_position.z as f32 + corner[2],
                ],
                normal: face.normal,
                albedo_roughness: surface,
                ambient_occlusion: vertex_ambient_occlusion(
                    occupancy_position,
                    face,
                    corner,
                    occupied,
                ),
                metallic: base_color[4],
            });
        }
        mesh.indices
            .extend_from_slice(&[first, first + 1, first + 2, first, first + 2, first + 3]);
    }
    true
}

fn vertex_ambient_occlusion(
    position: IVec3,
    face: &Face,
    corner: [f32; 3],
    occupied: &impl Fn(IVec3) -> bool,
) -> f32 {
    let outside = add(position, face.neighbor);
    let side_u = signed_tangent(face.tangent_u, corner);
    let side_v = signed_tangent(face.tangent_v, corner);
    let occupied_u = occupied(add(outside, side_u));
    let occupied_v = occupied(add(outside, side_v));
    let occupied_corner = occupied(add(add(outside, side_u), side_v));
    let level = if occupied_u && occupied_v {
        0
    } else {
        3 - u8::from(occupied_u) - u8::from(occupied_v) - u8::from(occupied_corner)
    };
    match level {
        0 => 0.42,
        1 => 0.60,
        2 => 0.79,
        _ => 1.0,
    }
}

fn signed_tangent(tangent: IVec3, corner: [f32; 3]) -> IVec3 {
    let coordinate = if tangent.x != 0 {
        corner[0]
    } else if tangent.y != 0 {
        corner[1]
    } else {
        corner[2]
    };
    let sign = if coordinate < 0.5 { -1 } else { 1 };
    IVec3::new(tangent.x * sign, tangent.y * sign, tangent.z * sign)
}

const fn add(left: IVec3, right: IVec3) -> IVec3 {
    IVec3::new(
        left.x.saturating_add(right.x),
        left.y.saturating_add(right.y),
        left.z.saturating_add(right.z),
    )
}

const fn material_surface(material: Material) -> [f32; 5] {
    match material {
        Material::Air => [0.0, 0.0, 0.0, 1.0, 0.0],
        Material::Soil => [0.22, 0.095, 0.035, 0.96, 0.0],
        Material::Stone => [0.34, 0.36, 0.39, 0.88, 0.0],
        Material::Wood => [0.42, 0.18, 0.055, 0.72, 0.0],
        Material::Brick => [0.52, 0.095, 0.045, 0.84, 0.0],
        Material::Concrete => [0.42, 0.44, 0.46, 0.94, 0.0],
        Material::Steel => [0.32, 0.37, 0.43, 0.28, 0.92],
        Material::Glass => [0.18, 0.42, 0.50, 0.12, 0.0],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Voxel;

    #[test]
    fn adjacent_voxels_hide_the_shared_faces() {
        let mut world = World::default();
        world.set_voxel(IVec3::new(0, 0, 0), Voxel::new(Material::Brick));
        assert_eq!(mesh_chunk(&world, IVec3::new(0, 0, 0)).exposed_faces(), 6);

        world.set_voxel(IVec3::new(1, 0, 0), Voxel::new(Material::Brick));
        assert_eq!(mesh_chunk(&world, IVec3::new(0, 0, 0)).exposed_faces(), 10);
    }

    #[test]
    fn only_steel_uses_a_metallic_surface_response() {
        assert!((material_surface(Material::Steel)[4] - 0.92).abs() < f32::EPSILON);
        for material in [
            Material::Soil,
            Material::Stone,
            Material::Wood,
            Material::Brick,
            Material::Concrete,
            Material::Glass,
        ] {
            assert!(material_surface(material)[4].abs() < f32::EPSILON);
        }
    }

    #[test]
    fn culling_samples_across_chunk_boundaries() {
        let mut world = World::default();
        world.set_voxel(IVec3::new(15, 0, 0), Voxel::new(Material::Stone));
        world.set_voxel(IVec3::new(16, 0, 0), Voxel::new(Material::Stone));
        assert_eq!(mesh_chunk(&world, IVec3::new(0, 0, 0)).exposed_faces(), 5);
        assert_eq!(mesh_chunk(&world, IVec3::new(1, 0, 0)).exposed_faces(), 5);
    }

    #[test]
    fn concave_neighbors_darkens_only_the_shared_vertex() {
        let mut world = World::default();
        world.set_voxel(IVec3::new(0, 0, 0), Voxel::new(Material::Stone));
        world.set_voxel(IVec3::new(1, -1, 0), Voxel::new(Material::Stone));
        world.set_voxel(IVec3::new(1, 0, -1), Voxel::new(Material::Stone));
        let mesh = mesh_chunk(&world, IVec3::new(0, 0, 0));
        assert!((mesh.vertices[0].ambient_occlusion - 0.42).abs() < f32::EPSILON);
        assert!((mesh.vertices[2].ambient_occlusion - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn body_mesh_is_local_and_culls_only_its_internal_faces() {
        use crate::{BodyLimits, structural::describe_island};

        let mut world = World::default();
        world.fill_box(
            IVec3::new(10, 20, -4),
            IVec3::new(11, 20, -4),
            Voxel::new(Material::Wood),
        );
        let island = describe_island(&world, vec![IVec3::new(10, 20, -4), IVec3::new(11, 20, -4)]);
        let body =
            RigidBodyDescriptor::from_detached_island(1, &world, &island, BodyLimits::default())
                .expect("connected body");

        let body_mesh = mesh_body(&body);
        assert_eq!(body_mesh.origin, IVec3::new(10, 20, -4));
        assert_eq!(body_mesh.maximum, IVec3::new(11, 20, -4));
        assert!(
            body_mesh
                .rotation_pivot
                .iter()
                .zip([1.0, 0.5, 0.5])
                .all(|(actual, expected)| (actual - expected).abs() < f32::EPSILON)
        );
        assert_eq!(body_mesh.mesh.exposed_faces(), 10);
        assert!(
            body_mesh
                .mesh
                .vertices
                .iter()
                .all(|vertex| vertex.position[0] >= 0.0 && vertex.position[0] <= 2.0)
        );
    }
}
