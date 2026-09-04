//! CPU chunk meshing. Only exposed voxel faces become GPU geometry.

#![allow(clippy::cast_precision_loss)]

use crate::{CHUNK_EDGE, IVec3, Material, World};
use bytemuck::{Pod, Zeroable};

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub struct Vertex {
    pub position: [f32; 3],
    pub normal: [f32; 3],
    /// Linear-ish RGB and perceptual roughness.
    pub albedo_roughness: [f32; 4],
}

#[derive(Debug, Default)]
pub struct CpuMesh {
    pub vertices: Vec<Vertex>,
    pub indices: Vec<u32>,
}

impl CpuMesh {
    #[must_use]
    pub const fn exposed_faces(&self) -> usize {
        self.indices.len() / 6
    }
}

struct Face {
    neighbor: IVec3,
    normal: [f32; 3],
    corners: [[f32; 3]; 4],
}

const FACES: [Face; 6] = [
    Face {
        neighbor: IVec3::new(1, 0, 0),
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
                let base_color = material_surface(voxel.material);
                let integrity = 0.65 + 0.35 * f32::from(voxel.integrity) / 255.0;
                let surface = [
                    base_color[0] * integrity,
                    base_color[1] * integrity,
                    base_color[2] * integrity,
                    base_color[3],
                ];
                for face in &FACES {
                    let neighbor = IVec3::new(
                        position.x + face.neighbor.x,
                        position.y + face.neighbor.y,
                        position.z + face.neighbor.z,
                    );
                    if world.voxel(neighbor).is_solid() {
                        continue;
                    }
                    let Ok(first) = u32::try_from(mesh.vertices.len()) else {
                        return mesh;
                    };
                    for corner in face.corners {
                        mesh.vertices.push(Vertex {
                            position: [
                                position.x as f32 + corner[0],
                                position.y as f32 + corner[1],
                                position.z as f32 + corner[2],
                            ],
                            normal: face.normal,
                            albedo_roughness: surface,
                        });
                    }
                    mesh.indices.extend_from_slice(&[
                        first,
                        first + 1,
                        first + 2,
                        first,
                        first + 2,
                        first + 3,
                    ]);
                }
            }
        }
    }
    mesh
}

const fn material_surface(material: Material) -> [f32; 4] {
    match material {
        Material::Air => [0.0, 0.0, 0.0, 1.0],
        Material::Soil => [0.22, 0.095, 0.035, 0.96],
        Material::Stone => [0.34, 0.36, 0.39, 0.88],
        Material::Wood => [0.42, 0.18, 0.055, 0.72],
        Material::Brick => [0.52, 0.095, 0.045, 0.84],
        Material::Concrete => [0.42, 0.44, 0.46, 0.94],
        Material::Steel => [0.32, 0.37, 0.43, 0.28],
        Material::Glass => [0.18, 0.42, 0.50, 0.12],
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
    fn culling_samples_across_chunk_boundaries() {
        let mut world = World::default();
        world.set_voxel(IVec3::new(15, 0, 0), Voxel::new(Material::Stone));
        world.set_voxel(IVec3::new(16, 0, 0), Voxel::new(Material::Stone));
        assert_eq!(mesh_chunk(&world, IVec3::new(0, 0, 0)).exposed_faces(), 5);
        assert_eq!(mesh_chunk(&world, IVec3::new(1, 0, 0)).exposed_faces(), 5);
    }
}
