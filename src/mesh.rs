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
    /// Stable material identifier used only for procedural visual synthesis on the GPU.
    pub material: u32,
}

const _: () = assert!(size_of::<Vertex>() == 52);

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
    /// Returns the number of rendered surface quads. Architectural materials use exact voxel
    /// faces, while natural materials use one Surface Nets quad per exposed density edge.
    pub const fn exposed_faces(&self) -> usize {
        self.indices.len() / 6
    }
}

const NATURAL_CELL_EDGE: usize = CHUNK_EDGE as usize + 1;
const NATURAL_CELL_COUNT: usize = NATURAL_CELL_EDGE * NATURAL_CELL_EDGE * NATURAL_CELL_EDGE;

#[derive(Clone, Copy, Debug)]
struct NaturalSurfacePoint {
    position: [f32; 3],
    normal: [f32; 3],
}

#[derive(Clone, Copy, Debug, Default)]
enum NaturalCell {
    #[default]
    Uncomputed,
    Empty,
    Surface(NaturalSurfacePoint),
}

/// Fixed-size cache covering the 17³ cell coordinates in the inclusive range
/// `chunk_origin - 1..=chunk_origin + 15`. A cell samples its eight voxel centres directly from the
/// complete immutable world snapshot, so the corresponding 18 possible sample coordinates are not
/// cache entries. This makes smoothing independent of scene complexity and prevents an adversarial
/// world from growing temporary meshing memory beyond a compile-time bound.
struct NaturalCellCache {
    minimum: IVec3,
    cells: Vec<NaturalCell>,
}

impl NaturalCellCache {
    fn new(chunk_origin: IVec3) -> Self {
        Self {
            minimum: add(chunk_origin, IVec3::new(-1, -1, -1)),
            cells: vec![NaturalCell::Uncomputed; NATURAL_CELL_COUNT],
        }
    }

    fn surface(&mut self, world: &World, cell: IVec3) -> Option<NaturalSurfacePoint> {
        let index = self.index(cell)?;
        match self.cells[index] {
            NaturalCell::Uncomputed => {
                let surface = natural_surface_point(world, cell);
                self.cells[index] = surface.map_or(NaturalCell::Empty, NaturalCell::Surface);
                surface
            }
            NaturalCell::Empty => None,
            NaturalCell::Surface(surface) => Some(surface),
        }
    }

    fn index(&self, cell: IVec3) -> Option<usize> {
        let x = usize::try_from(cell.x.checked_sub(self.minimum.x)?).ok()?;
        let y = usize::try_from(cell.y.checked_sub(self.minimum.y)?).ok()?;
        let z = usize::try_from(cell.z.checked_sub(self.minimum.z)?).ok()?;
        if x >= NATURAL_CELL_EDGE || y >= NATURAL_CELL_EDGE || z >= NATURAL_CELL_EDGE {
            return None;
        }
        Some(x + y * NATURAL_CELL_EDGE + z * NATURAL_CELL_EDGE * NATURAL_CELL_EDGE)
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
    let mut natural_cells = NaturalCellCache::new(origin);
    for local_z in 0..CHUNK_EDGE {
        for local_y in 0..CHUNK_EDGE {
            for local_x in 0..CHUNK_EDGE {
                let position =
                    IVec3::new(origin.x + local_x, origin.y + local_y, origin.z + local_z);
                let voxel = world.voxel(position);
                if !voxel.is_solid() {
                    continue;
                }
                if is_natural(voxel.material) {
                    if !append_natural_surface(
                        &mut mesh,
                        world,
                        &mut natural_cells,
                        position,
                        voxel,
                    ) {
                        return mesh;
                    }
                    continue;
                }
                // An exact architectural face closes the transition to a smoothed natural
                // surface. Culling both sides would expose a crack because Surface Nets vertices
                // are generally inset from the voxel boundary.
                let occupied = |sample| {
                    let neighbor = world.voxel(sample);
                    neighbor.is_solid() && !is_natural(neighbor.material)
                };
                if !append_voxel(&mut mesh, position, position, voxel, &occupied) {
                    return mesh;
                }
            }
        }
    }
    mesh
}

fn append_natural_surface(
    mesh: &mut CpuMesh,
    world: &World,
    cells: &mut NaturalCellCache,
    position: IVec3,
    voxel: Voxel,
) -> bool {
    for face in &FACES {
        if world.voxel(add(position, face.neighbor)).is_solid() {
            continue;
        }
        let (axis, tangent_u, tangent_v) = natural_edge_basis(face.neighbor);
        let edge_base = if axis_component(face.neighbor, axis) > 0 {
            position
        } else {
            add(position, face.neighbor)
        };
        let cell_positions = [
            edge_base,
            add(edge_base, negate(tangent_u)),
            add(add(edge_base, negate(tangent_u)), negate(tangent_v)),
            add(edge_base, negate(tangent_v)),
        ];
        let [Some(first), Some(second), Some(third), Some(fourth)] =
            cell_positions.map(|cell| cells.surface(world, cell))
        else {
            // Every cell around a sign-changing edge must itself contain that edge. Returning an
            // incomplete mesh is safer than creating invalid indices if the invariant is broken.
            return false;
        };
        let surface_points = [first, second, third, fourth];
        if !append_natural_quad(mesh, &surface_points, voxel, face.normal) {
            return false;
        }
    }
    true
}

fn append_natural_quad(
    mesh: &mut CpuMesh,
    points: &[NaturalSurfacePoint],
    voxel: Voxel,
    outward: [f32; 3],
) -> bool {
    let Ok(first) = u32::try_from(mesh.vertices.len()) else {
        return false;
    };
    let base_color = material_surface(voxel.material);
    let integrity = 0.65 + 0.35 * f32::from(voxel.integrity) / 255.0;
    for point in points {
        mesh.vertices.push(Vertex {
            position: point.position,
            normal: point.normal,
            albedo_roughness: [
                base_color[0] * integrity,
                base_color[1] * integrity,
                base_color[2] * integrity,
                base_color[3],
            ],
            ambient_occlusion: 1.0,
            metallic: base_color[4],
            material: u32::from(voxel.material as u8),
        });
    }
    let diagonal_zero_two = squared_distance(points[0].position, points[2].position);
    let diagonal_one_three = squared_distance(points[1].position, points[3].position);
    if outward[0] + outward[1] + outward[2] > 0.0 && diagonal_zero_two <= diagonal_one_three {
        mesh.indices
            .extend_from_slice(&[first, first + 1, first + 2, first, first + 2, first + 3]);
    } else if outward[0] + outward[1] + outward[2] > 0.0 {
        mesh.indices.extend_from_slice(&[
            first,
            first + 1,
            first + 3,
            first + 1,
            first + 2,
            first + 3,
        ]);
    } else if diagonal_zero_two <= diagonal_one_three {
        mesh.indices
            .extend_from_slice(&[first, first + 3, first + 2, first, first + 2, first + 1]);
    } else {
        mesh.indices.extend_from_slice(&[
            first,
            first + 3,
            first + 1,
            first + 1,
            first + 3,
            first + 2,
        ]);
    }
    true
}

fn natural_surface_point(world: &World, cell: IVec3) -> Option<NaturalSurfacePoint> {
    const CORNERS: [IVec3; 8] = [
        IVec3::new(0, 0, 0),
        IVec3::new(1, 0, 0),
        IVec3::new(0, 1, 0),
        IVec3::new(1, 1, 0),
        IVec3::new(0, 0, 1),
        IVec3::new(1, 0, 1),
        IVec3::new(0, 1, 1),
        IVec3::new(1, 1, 1),
    ];
    const EDGES: [(usize, usize); 12] = [
        (0, 1),
        (2, 3),
        (4, 5),
        (6, 7),
        (0, 2),
        (1, 3),
        (4, 6),
        (5, 7),
        (0, 4),
        (1, 5),
        (2, 6),
        (3, 7),
    ];

    let samples = CORNERS.map(|offset| world.voxel(add(cell, offset)));
    let mut position_sum = [0.0_f32; 3];
    let mut normal_sum = [0.0_f32; 3];
    let mut first_normal = [0.0_f32; 3];
    let mut crossings = 0_u8;
    for (left_index, right_index) in EDGES {
        let left_natural = is_natural(samples[left_index].material);
        let right_natural = is_natural(samples[right_index].material);
        let left_air = !samples[left_index].is_solid();
        let right_air = !samples[right_index].is_solid();
        if !(left_natural && right_air || right_natural && left_air) {
            continue;
        }

        let left = CORNERS[left_index];
        let right = CORNERS[right_index];
        position_sum[0] += ((left.x + right.x) as f32).mul_add(0.5, cell.x as f32 + 0.5);
        position_sum[1] += ((left.y + right.y) as f32).mul_add(0.5, cell.y as f32 + 0.5);
        position_sum[2] += ((left.z + right.z) as f32).mul_add(0.5, cell.z as f32 + 0.5);
        let direction = if left_natural {
            [
                (right.x - left.x) as f32,
                (right.y - left.y) as f32,
                (right.z - left.z) as f32,
            ]
        } else {
            [
                (left.x - right.x) as f32,
                (left.y - right.y) as f32,
                (left.z - right.z) as f32,
            ]
        };
        if crossings == 0 {
            first_normal = direction;
        }
        normal_sum[0] += direction[0];
        normal_sum[1] += direction[1];
        normal_sum[2] += direction[2];
        crossings = crossings.saturating_add(1);
    }
    if crossings == 0 {
        return None;
    }
    let divisor = f32::from(crossings);
    let normal = normalize_or(normal_sum, first_normal);
    Some(NaturalSurfacePoint {
        position: [
            position_sum[0] / divisor,
            position_sum[1] / divisor,
            position_sum[2] / divisor,
        ],
        normal,
    })
}

const fn is_natural(material: Material) -> bool {
    matches!(material, Material::Soil | Material::Stone)
}

const fn natural_edge_basis(direction: IVec3) -> (usize, IVec3, IVec3) {
    if direction.x != 0 {
        (0, IVec3::new(0, 1, 0), IVec3::new(0, 0, 1))
    } else if direction.y != 0 {
        (1, IVec3::new(0, 0, 1), IVec3::new(1, 0, 0))
    } else {
        (2, IVec3::new(1, 0, 0), IVec3::new(0, 1, 0))
    }
}

const fn axis_component(vector: IVec3, axis: usize) -> i32 {
    match axis {
        0 => vector.x,
        1 => vector.y,
        _ => vector.z,
    }
}

const fn negate(vector: IVec3) -> IVec3 {
    IVec3::new(-vector.x, -vector.y, -vector.z)
}

fn normalize_or(vector: [f32; 3], fallback: [f32; 3]) -> [f32; 3] {
    let squared_length = vector[2].mul_add(
        vector[2],
        vector[1].mul_add(vector[1], vector[0] * vector[0]),
    );
    if squared_length <= f32::EPSILON {
        return fallback;
    }
    let inverse_length = squared_length.sqrt().recip();
    [
        vector[0] * inverse_length,
        vector[1] * inverse_length,
        vector[2] * inverse_length,
    ]
}

fn squared_distance(left: [f32; 3], right: [f32; 3]) -> f32 {
    let x = left[0] - right[0];
    let y = left[1] - right[1];
    let z = left[2] - right[2];
    z.mul_add(z, y.mul_add(y, x * x))
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
                material: u32::from(voxel.material as u8),
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
    fn material_identity_survives_cpu_meshing() {
        let mut world = World::default();
        world.set_voxel(IVec3::new(0, 0, 0), Voxel::new(Material::Brick));

        let mesh = mesh_chunk(&world, IVec3::new(0, 0, 0));

        assert_eq!(mesh.vertices.len(), 24);
        assert!(
            mesh.vertices
                .iter()
                .all(|vertex| vertex.material == u32::from(Material::Brick as u8))
        );
        assert!(mesh.vertices.iter().all(|vertex| {
            vertex
                .position
                .iter()
                .all(|coordinate| coordinate.fract().abs() < f32::EPSILON)
        }));
    }

    #[test]
    fn natural_voxel_uses_smooth_deterministic_surface_nets() {
        let mut world = World::default();
        world.set_voxel(IVec3::new(0, 0, 0), Voxel::new(Material::Stone));

        let first = mesh_chunk(&world, IVec3::new(0, 0, 0));
        let second = mesh_chunk(&world, IVec3::new(0, 0, 0));

        assert_eq!(first.exposed_faces(), 6);
        assert_eq!(first.vertices.len(), 24);
        assert_eq!(first.indices, second.indices);
        assert!(
            first
                .vertices
                .iter()
                .zip(&second.vertices)
                .all(|(left, right)| {
                    left.position.map(f32::to_bits) == right.position.map(f32::to_bits)
                        && left.normal.map(f32::to_bits) == right.normal.map(f32::to_bits)
                })
        );
        assert!(first.vertices.iter().all(|vertex| {
            vertex.material == u32::from(Material::Stone as u8)
                && vertex
                    .position
                    .iter()
                    .all(|coordinate| coordinate.is_finite())
                && vertex
                    .normal
                    .iter()
                    .all(|coordinate| coordinate.is_finite())
        }));
        assert!(first.vertices.iter().any(|vertex| {
            vertex
                .position
                .iter()
                .any(|coordinate| coordinate.fract().abs() > 0.01)
        }));
        assert!(first.vertices.iter().any(|vertex| {
            vertex
                .normal
                .iter()
                .filter(|coordinate| coordinate.abs() > 0.1)
                .count()
                > 1
        }));

        for indices in first.indices.chunks_exact(6) {
            let a = first.vertices[indices[0] as usize].position;
            let b = first.vertices[indices[1] as usize].position;
            let c = first.vertices[indices[2] as usize].position;
            let triangle_normal = cross(subtract(b, a), subtract(c, a));
            let vertex_normal = first.vertices[indices[0] as usize].normal;
            assert!(dot(triangle_normal, vertex_normal) > 0.0);
        }
    }

    #[test]
    fn architectural_face_closes_the_natural_material_transition() {
        let mut world = World::default();
        world.set_voxel(IVec3::new(0, 0, 0), Voxel::new(Material::Soil));
        world.set_voxel(IVec3::new(1, 0, 0), Voxel::new(Material::Brick));

        let mesh = mesh_chunk(&world, IVec3::new(0, 0, 0));

        assert_eq!(mesh.exposed_faces(), 11);
        assert_eq!(
            mesh.vertices
                .iter()
                .filter(|vertex| {
                    vertex.material == u32::from(Material::Brick as u8)
                        && (vertex.position[0] - 1.0).abs() < f32::EPSILON
                        && (vertex.normal[0] + 1.0).abs() < f32::EPSILON
                })
                .count(),
            4
        );
    }

    #[test]
    fn irregular_natural_surface_keeps_every_triangle_front_facing() {
        let mut world = World::default();
        for x in 0..4 {
            for z in 0..4 {
                for y in 0..=x.min(2) {
                    world.set_voxel(IVec3::new(x, y, z), Voxel::new(Material::Stone));
                }
            }
        }

        let mesh = mesh_chunk(&world, IVec3::new(0, 0, 0));
        for indices in mesh.indices.chunks_exact(3) {
            let a = mesh.vertices[indices[0] as usize].position;
            let b = mesh.vertices[indices[1] as usize].position;
            let c = mesh.vertices[indices[2] as usize].position;
            let triangle_normal = cross(subtract(b, a), subtract(c, a));
            let mut average_normal = [0.0_f32; 3];
            for index in indices {
                let normal = mesh.vertices[*index as usize].normal;
                average_normal[0] += normal[0];
                average_normal[1] += normal[1];
                average_normal[2] += normal[2];
            }
            assert!(dot(triangle_normal, average_normal) > 0.0);
        }
    }

    #[test]
    fn natural_surface_stitches_across_all_chunk_boundary_directions() {
        let cases = [
            (
                IVec3::new(15, 4, 4),
                IVec3::new(16, 4, 4),
                IVec3::new(0, 0, 0),
                IVec3::new(1, 0, 0),
            ),
            (
                IVec3::new(0, 4, 4),
                IVec3::new(-1, 4, 4),
                IVec3::new(0, 0, 0),
                IVec3::new(-1, 0, 0),
            ),
            (
                IVec3::new(4, 15, 4),
                IVec3::new(4, 16, 4),
                IVec3::new(0, 0, 0),
                IVec3::new(0, 1, 0),
            ),
            (
                IVec3::new(4, 0, 4),
                IVec3::new(4, -1, 4),
                IVec3::new(0, 0, 0),
                IVec3::new(0, -1, 0),
            ),
            (
                IVec3::new(4, 4, 15),
                IVec3::new(4, 4, 16),
                IVec3::new(0, 0, 0),
                IVec3::new(0, 0, 1),
            ),
            (
                IVec3::new(4, 4, 0),
                IVec3::new(4, 4, -1),
                IVec3::new(0, 0, 0),
                IVec3::new(0, 0, -1),
            ),
        ];

        for (first_position, second_position, first_chunk, second_chunk) in cases {
            let mut world = World::default();
            world.set_voxel(first_position, Voxel::new(Material::Stone));
            world.set_voxel(second_position, Voxel::new(Material::Stone));
            let first = mesh_chunk(&world, first_chunk);
            let second = mesh_chunk(&world, second_chunk);

            assert_eq!(first.exposed_faces(), 5);
            assert_eq!(second.exposed_faces(), 5);
            let shared_vertices = first
                .vertices
                .iter()
                .filter(|first_vertex| {
                    second.vertices.iter().any(|second_vertex| {
                        first_vertex.position.map(f32::to_bits)
                            == second_vertex.position.map(f32::to_bits)
                    })
                })
                .count();
            assert!(
                shared_vertices >= 2,
                "surface seam must share exact vertices"
            );
        }
    }

    #[test]
    fn concave_neighbors_darkens_only_the_shared_vertex() {
        let mut world = World::default();
        world.set_voxel(IVec3::new(0, 0, 0), Voxel::new(Material::Concrete));
        world.set_voxel(IVec3::new(1, -1, 0), Voxel::new(Material::Concrete));
        world.set_voxel(IVec3::new(1, 0, -1), Voxel::new(Material::Concrete));
        let mesh = mesh_chunk(&world, IVec3::new(0, 0, 0));
        assert!((mesh.vertices[0].ambient_occlusion - 0.42).abs() < f32::EPSILON);
        assert!((mesh.vertices[2].ambient_occlusion - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn natural_cell_cache_has_a_fixed_chunk_local_bound() {
        let cache = NaturalCellCache::new(IVec3::new(16, -32, 48));

        assert_eq!(cache.cells.len(), 17 * 17 * 17);
        assert!(cache.index(IVec3::new(15, -33, 47)).is_some());
        assert!(cache.index(IVec3::new(31, -17, 63)).is_some());
        assert!(cache.index(IVec3::new(14, -33, 47)).is_none());
        assert!(cache.index(IVec3::new(32, -17, 63)).is_none());
    }

    #[test]
    fn symmetric_natural_cell_uses_a_finite_deterministic_normal_fallback() {
        let mut world = World::default();
        for x in 0..=1 {
            for y in 0..=1 {
                for z in 0..=1 {
                    if (x + y + z) % 2 == 0 {
                        world.set_voxel(IVec3::new(x, y, z), Voxel::new(Material::Stone));
                    }
                }
            }
        }

        let surface = natural_surface_point(&world, IVec3::new(0, 0, 0))
            .expect("checkerboard cell crosses its density boundary");

        assert_eq!(
            surface.normal.map(f32::to_bits),
            [1.0, 0.0, 0.0].map(f32::to_bits)
        );
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

    fn subtract(left: [f32; 3], right: [f32; 3]) -> [f32; 3] {
        [left[0] - right[0], left[1] - right[1], left[2] - right[2]]
    }

    fn cross(left: [f32; 3], right: [f32; 3]) -> [f32; 3] {
        [
            left[2].mul_add(-right[1], left[1] * right[2]),
            left[0].mul_add(-right[2], left[2] * right[0]),
            left[1].mul_add(-right[0], left[0] * right[1]),
        ]
    }

    fn dot(left: [f32; 3], right: [f32; 3]) -> f32 {
        left[2].mul_add(right[2], left[1].mul_add(right[1], left[0] * right[0]))
    }
}
