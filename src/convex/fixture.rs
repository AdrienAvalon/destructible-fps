//! Actual closed oblique masonry in the authored industrial scene. Not simulated collapse.
//!
//! Each large slab rests on an exactly matching wedge, itself on a complete ground face.
//! There is no hidden stepped apron beneath these shapes.
use super::{ConvexError, ConvexFaceInput, ConvexFragment, InspectionGeometry};
use crate::{IVec3, Material, Voxel, world::geometry::RefinedWorld};
use std::sync::Arc;

/// A convex chamfered footprint, with affine top and bottom planes in q256 metric units.
fn prism(
    origin: IVec3,
    size: [u16; 2],
    lower: [i32; 3],
    upper: [i32; 3],
    material: Material,
    cut_top: bool,
) -> Result<ConvexFragment, ConvexError> {
    let [w, d] = size;
    // Irregular convex shards, not identical chamfered prisms. All lattice coordinates remain
    // multiples of four so the affine heights below are exact, never a sampled staircase.
    let outline: &[[u16; 2]] = match (origin.x * 3 + origin.z).rem_euclid(3) {
        0 => &[[0, 1], [5, 0], [8, 3], [6, 8], [1, 6]],
        1 => &[[0, 0], [7, 1], [8, 6], [3, 8], [0, 5]],
        _ => &[[0, 2], [8, 0], [5, 8]],
    };
    let footprint: Vec<_> = outline
        .iter()
        .map(|[x, z]| [x * w / 8, z * d / 8])
        .collect();
    let count = u8::try_from(footprint.len()).map_err(|_| ConvexError::Budget)?;
    let height = |plane: [i32; 3], x: u16, z: u16| -> Result<u16, ConvexError> {
        let value = plane[0] + (plane[1] * i32::from(x) + plane[2] * i32::from(z)) / 4;
        u16::try_from(value).map_err(|_| ConvexError::Bounds)
    };
    let mut vertices = Vec::with_capacity(16);
    for plane in [lower, upper] {
        for &[x, z] in &footprint {
            vertices.push([x, height(plane, x, z)?, z]);
        }
    }
    let mut faces = vec![
        ConvexFaceInput {
            indices: (0..count).collect(),
            cut: true,
        },
        ConvexFaceInput {
            indices: (count..2 * count).rev().collect(),
            cut: cut_top,
        },
    ];
    for i in 0_u8..count {
        let j = (i + 1) % count;
        faces.push(ConvexFaceInput {
            indices: vec![j, i, i + count, j + count],
            cut: true,
        });
    }
    ConvexFragment::new(origin, &vertices, &faces, Voxel::new(material))
}

/// Builds canonical fragments only; the assembly then checks their world/pair intersections.
/// # Errors
/// Refuses invalid shape or bounded authoring failures.
pub fn industrial_fragments() -> Result<Vec<ConvexFragment>, ConvexError> {
    let mut fragments = Vec::with_capacity(28);
    // Asymmetric large pieces flank the retained three-metre route into the ruined bay.
    // Every footprint coordinate is divisible by four, so affine planes are never rounded.
    for (origin, size, support, slope, material) in [
        (
            IVec3::new(-22, 1, 17),
            [768, 512],
            80,
            [1, 0],
            Material::Stone,
        ),
        (
            IVec3::new(-13, 1, 17),
            [768, 512],
            368,
            [-1, -1],
            Material::Stone,
        ),
        (
            IVec3::new(-21, 1, 20),
            [512, 384],
            48,
            [1, 1],
            Material::Stone,
        ),
        (
            IVec3::new(-12, 1, 20),
            [512, 384],
            176,
            [-1, 0],
            Material::Stone,
        ),
    ] {
        let lower = [support, slope[0], slope[1]];
        let upper = [support + 48, slope[0], slope[1]];
        fragments.push(prism(origin, size, [0, 0, 0], lower, material, true)?);
        fragments.push(prism(
            origin,
            size,
            lower,
            upper,
            Material::Concrete,
            false,
        )?);
    }
    // Smaller grounded angular pieces, not voxel staircases or unconstrained cosmetic particles.
    for (index, (x, z, w, d, height, sx, sz)) in [
        (-23, 17, 160, 192, 96, 1, 0),
        (-23, 19, 224, 160, 160, -1, 0),
        (-19, 17, 192, 224, 160, 0, -1),
        (-19, 19, 160, 160, 80, 1, 0),
        (-18, 20, 128, 192, 72, 0, 1),
        (-22, 21, 192, 128, 80, 1, -1),
        (-20, 22, 192, 160, 64, 0, 1),
        (-18, 22, 96, 128, 48, 1, 0),
        (-14, 17, 160, 192, 80, 1, 0),
        (-14, 19, 192, 160, 112, -1, 0),
        (-10, 19, 128, 224, 128, 1, -1),
        (-9, 17, 192, 128, 160, -1, 1),
        (-9, 21, 160, 192, 72, 1, 0),
        (-13, 22, 128, 128, 96, -1, 0),
        (-11, 22, 192, 160, 64, 1, 0),
        (-10, 23, 128, 128, 64, 0, 1),
    ]
    .into_iter()
    .enumerate()
    {
        let material = [Material::Brick, Material::Stone, Material::Concrete][index % 3];
        fragments.push(prism(
            IVec3::new(x, 1, z),
            [w, d],
            [0, 0, 0],
            [height, sx, sz],
            material,
            true,
        )?);
    }
    Ok(fragments)
}

/// Combines the empty-apron voxel source and its real oblique solids before publication.
/// # Errors
/// Refuses nonempty apron, intersection, unsupported ground or invalid shapes.
pub fn industrial_scene(world: Arc<RefinedWorld>) -> Result<InspectionGeometry, ConvexError> {
    let fragments = industrial_fragments()?;
    for fragment in &fragments {
        let origin = fragment.origin();
        if fragment.vertices().iter().any(|v| v[1] == 0) {
            let maximum = fragment.bounds().maximum_scaled();
            // Conservative whole-footprint ground proof, including its interior, never just
            // corners. Maximum is exclusive, so a tangent neighbouring cell is not required.
            for x in origin.x
                ..=i32::try_from((maximum[0] - 1).div_euclid(256_000_000))
                    .map_err(|_| ConvexError::Bounds)?
            {
                for z in origin.z
                    ..=i32::try_from((maximum[2] - 1).div_euclid(256_000_000))
                        .map_err(|_| ConvexError::Bounds)?
                {
                    if world.cell(IVec3::new(x, 0, z)).solid_units() != crate::volume::VOLUME_UNITS
                    {
                        return Err(ConvexError::InvalidShape);
                    }
                }
            }
        }
    }
    InspectionGeometry::new(world, fragments)
}
