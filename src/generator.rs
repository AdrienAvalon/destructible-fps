use crate::material::{Material, Voxel};
use crate::world::{IVec3, World};

/// Generates a deterministic test range with terrain and a multi-material building.
#[must_use]
pub fn demo_world() -> World {
    let mut world = World::default();

    world.fill_box(
        IVec3::new(-64, -3, -64),
        IVec3::new(63, -1, 63),
        Voxel::new(Material::Stone),
    );
    world.fill_box(
        IVec3::new(-64, 0, -64),
        IVec3::new(63, 0, 63),
        Voxel::new(Material::Soil),
    );

    // Foundation, intermediate slab, and roof.
    world.fill_box(
        IVec3::new(-20, 1, -16),
        IVec3::new(20, 1, 16),
        Voxel::new(Material::Concrete),
    );
    world.fill_box(
        IVec3::new(-20, 8, -16),
        IVec3::new(20, 8, 16),
        Voxel::new(Material::Concrete),
    );
    world.fill_box(
        IVec3::new(-20, 15, -16),
        IVec3::new(20, 15, 16),
        Voxel::new(Material::Concrete),
    );

    // Brick envelope with regular window openings.
    for y in 2..15 {
        for x in -20..=20 {
            for z in [-16, 16] {
                if window_opening(x, y) {
                    world.set_voxel(IVec3::new(x, y, z), Voxel::new(Material::Glass));
                } else {
                    world.set_voxel(IVec3::new(x, y, z), Voxel::new(Material::Brick));
                }
            }
        }
        for z in -15..=15 {
            for x in [-20, 20] {
                if window_opening(z, y) {
                    world.set_voxel(IVec3::new(x, y, z), Voxel::new(Material::Glass));
                } else {
                    world.set_voxel(IVec3::new(x, y, z), Voxel::new(Material::Brick));
                }
            }
        }
    }

    // Steel frame and a wooden interior wall provide distinct material responses.
    for x in [-19, 0, 19] {
        for z in [-15, 0, 15] {
            world.fill_box(
                IVec3::new(x, 2, z),
                IVec3::new(x, 14, z),
                Voxel::new(Material::Steel),
            );
        }
    }
    world.fill_box(
        IVec3::new(-1, 2, -14),
        IVec3::new(-1, 7, 14),
        Voxel::new(Material::Wood),
    );
    world
}

fn window_opening(horizontal: i32, y: i32) -> bool {
    let local = horizontal.rem_euclid(8);
    (3..=5).contains(&local) && matches!(y, 4..=6 | 11..=13)
}
