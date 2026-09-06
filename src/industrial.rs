//! Authored quarry workshop. Every solid is ordinary authoritative, destructible material.
//!
//! Coordinates retain the current one-metre cell scale. Apertures are genuinely empty, not
//! transparent-looking collision walls. Fine frames, thin sections and calibrated loads remain work.

use crate::{IVec3, Material, Voxel, World};

pub const CANOPY_SUPPORTS: [IVec3; 4] = [
    IVec3::new(24, 1, 12),
    IVec3::new(36, 1, 12),
    IVec3::new(24, 1, 26),
    IVec3::new(36, 1, 26),
];
pub const BARRICADE: IVec3 = IVec3::new(-5, 2, -7);
pub const HALL_FRONT_WINDOW_STARTS: [i32; 4] = [-18, -8, 3, 13];
pub const HALL_SIDE_WINDOW_STARTS: [i32; 3] = [-12, -3, 6];
pub const HALL_SIDE_COLUMNS: [i32; 4] = [-16, -6, 6, 16];

/// Builds an intact industrial map using bounded, platform-independent integer construction.
#[must_use]
pub fn industrial_world() -> World {
    let mut world = ground();
    main_hall(&mut world);
    annex(&mut world);
    clerestory(&mut world);
    loading_canopy(&mut world);
    // A material-response target in the hall, not a fake rendered prop.
    fill(&mut world, [-7, 1, -7], [-3, 2, -7], Material::Wood);
    world
}

fn fill(world: &mut World, low: [i32; 3], high: [i32; 3], material: Material) {
    world.fill_box(
        IVec3::new(low[0], low[1], low[2]),
        IVec3::new(high[0], high[1], high[2]),
        Voxel::new(material),
    );
}

fn ground() -> World {
    let mut world = World::default();
    fill(&mut world, [-64, -3, -64], [63, -1, 63], Material::Stone);
    fill(&mut world, [-64, 0, -64], [63, 0, 63], Material::Soil);
    for x in -64..64 {
        for z in -64..64 {
            // Building footprints, all sixteen existing spawn slots and traversable forecourt.
            if (-41..=40).contains(&x) && (-25..=46).contains(&z) {
                continue;
            }
            let height = [(-53, 0, 23, 9), (54, -4, 24, 10), (0, -53, 30, 14)]
                .into_iter()
                .map(|(cx, cz, r, peak)| {
                    let d2 = (x - cx) * (x - cx) + (z - cz) * (z - cz);
                    peak * (r * r - d2).max(0) / (r * r)
                })
                .max()
                .unwrap_or(0);
            if height > 0 {
                fill(&mut world, [x, 0, z], [x, height - 1, z], Material::Stone);
                fill(&mut world, [x, height, z], [x, height, z], Material::Soil);
            }
        }
    }
    world
}

fn main_hall(world: &mut World) {
    // Flush threshold: the floor surface and existing player spawn are both at y=1 metre.
    fill(world, [-20, 0, -16], [20, 0, 16], Material::Concrete);
    fill(world, [-20, 14, -16], [20, 14, 16], Material::Concrete);
    for z in [-15, 15] {
        fill(world, [-19, 1, z], [19, 13, z], Material::Brick);
        for x in HALL_FRONT_WINDOW_STARTS {
            fill(world, [x, 7, z], [x + 5, 11, z], Material::Air);
            fill(world, [x, 6, z], [x + 5, 6, z], Material::Concrete);
            fill(world, [x, 12, z], [x + 5, 12, z], Material::Concrete);
        }
    }
    for x in [-20, 20] {
        fill(world, [x, 1, -16], [x, 13, 16], Material::Brick);
        for z in HALL_SIDE_WINDOW_STARTS {
            fill(world, [x, 7, z], [x, 11, z + 5], Material::Air);
            fill(world, [x, 6, z], [x, 6, z + 5], Material::Concrete);
        }
        for z in HALL_SIDE_COLUMNS {
            fill(world, [x, 1, z], [x, 14, z], Material::Concrete);
        }
    }
    // Projecting concrete bays make the brick infill read as a distinct recessed layer.
    for x in [-20, -10, 10, 20] {
        for z in [-16, 16] {
            fill(world, [x, 1, z], [x, 14, z], Material::Concrete);
        }
    }
    for z in [-16, 16] {
        fill(world, [-20, 13, z], [20, 13, z], Material::Concrete);
    }
    // Large loading portal and opposite service exit provide real traversable sight lines.
    fill(world, [-4, 1, 15], [4, 5, 16], Material::Air);
    fill(world, [-5, 6, 15], [5, 6, 16], Material::Concrete);
    for x in [-5, 5] {
        fill(world, [x, 1, 15], [x, 5, 16], Material::Concrete);
    }
    fill(world, [-2, 1, -16], [2, 3, -15], Material::Air);
    // Interior bearing posts and transverse beams support the raised roof, not floating trim.
    for x in [-8, 8] {
        for z in [-12, 0, 12] {
            fill(world, [x, 1, z], [x, 13, z], Material::Concrete);
        }
        fill(world, [x, 13, -16], [x, 13, 16], Material::Concrete);
    }
}

fn clerestory(world: &mut World) {
    fill(world, [-6, 14, -11], [6, 14, 11], Material::Air);
    for x in [-7, 7] {
        fill(world, [x, 15, -12], [x, 17, 12], Material::Concrete);
        for z in [-10, -3, 4] {
            fill(world, [x, 15, z], [x, 16, z + 4], Material::Air);
        }
    }
    for z in [-12, 12] {
        fill(world, [-7, 15, z], [7, 17, z], Material::Concrete);
        fill(world, [-5, 15, z], [5, 16, z], Material::Air);
    }
    fill(world, [-8, 18, -13], [8, 18, 13], Material::Concrete);
}

fn annex(world: &mut World) {
    fill(world, [-38, 0, -8], [-21, 0, 12], Material::Concrete);
    for z in [-8, 12] {
        fill(world, [-38, 1, z], [-21, 5, z], Material::Brick);
        for x in [-36, -27] {
            fill(world, [x, 3, z], [x + 4, 4, z], Material::Air);
        }
    }
    fill(world, [-38, 1, -8], [-38, 5, 12], Material::Brick);
    fill(world, [-21, 1, -8], [-21, 5, 12], Material::Brick);
    // Join the annex slab to the hall's west wall, replacing its shared brick row with concrete.
    fill(world, [-39, 6, -9], [-20, 6, 13], Material::Concrete);
    for x in [-38, -30, -21] {
        for z in [-8, 12] {
            fill(world, [x, 1, z], [x, 5, z], Material::Concrete);
        }
    }
    fill(world, [-33, 1, 12], [-31, 3, 12], Material::Air);
    fill(world, [-21, 1, -2], [-20, 3, 2], Material::Air);
    // Taller service tower breaks the roof line behind the low workshop wing.
    fill(world, [-34, 0, -22], [-25, 0, -9], Material::Concrete);
    for x in [-34, -25] {
        fill(world, [x, 1, -22], [x, 12, -9], Material::Brick);
        fill(world, [x, 7, -18], [x, 10, -15], Material::Air);
    }
    for z in [-22, -9] {
        fill(world, [-34, 1, z], [-25, 12, z], Material::Brick);
        for x in [-34, -25] {
            fill(world, [x, 1, z], [x, 12, z], Material::Concrete);
        }
    }
    fill(world, [-35, 13, -23], [-24, 13, -8], Material::Concrete);
    fill(world, [-31, 1, -9], [-29, 3, -8], Material::Air);
}

fn loading_canopy(world: &mut World) {
    // Deliberate air gap to the main hall: only these four supports hold the slab.
    fill(world, [23, 6, 11], [37, 6, 27], Material::Concrete);
    for support in CANOPY_SUPPORTS {
        fill(
            world,
            [support.x, 1, support.z],
            [support.x, 5, support.z],
            Material::Concrete,
        );
        fill(
            world,
            [support.x, 0, support.z],
            [support.x, 0, support.z],
            Material::Concrete,
        );
    }
}
