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
    for x in -64..64 {
        for z in -64..64 {
            let height = terrain_height(x, z);
            if height == 0 {
                continue;
            }
            let position = |y| IVec3::new(x, y, z);
            world.set_voxel(position(0), Voxel::new(Material::Stone));
            if height > 1 {
                world.fill_box(
                    position(1),
                    position(height - 1),
                    Voxel::new(Material::Stone),
                );
            }
            world.set_voxel(position(height), Voxel::new(Material::Soil));
        }
    }

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

    // A deliberately fragile test column lets the showcase and GPU smoke exercise authoritative
    // structural detachment and rigid-body rendering before the main facade is breached.
    world.set_voxel(IVec3::new(0, 1, 24), Voxel::new(Material::Glass));
    world.fill_box(
        IVec3::new(0, 2, 24),
        IVec3::new(0, 6, 24),
        Voxel::new(Material::Wood),
    );
    world
}

/// Integer-only height field: deterministic across operating systems and deliberately flat under
/// the building, player spawn, and showcase shot corridor. Surface Nets turns these bounded voxel
/// terraces into continuous quarry banks and rubble mounds at render time.
fn terrain_height(x: i32, z: i32) -> i32 {
    if ((-23..=23).contains(&x) && (-19..=19).contains(&z))
        || ((-5..=5).contains(&x) && (17..=48).contains(&z))
    {
        return 0;
    }

    [
        mound_height(x, z, -40, 4, 29, 7),
        mound_height(x, z, 40, 3, 28, 7),
        mound_height(x, z, 0, -52, 31, 11),
        mound_height(x, z, -14, 29, 11, 2),
        mound_height(x, z, 15, 27, 10, 2),
    ]
    .into_iter()
    .max()
    .unwrap_or_default()
}

fn mound_height(x: i32, z: i32, center_x: i32, center_z: i32, radius: i32, peak: i32) -> i32 {
    let delta_x = i64::from(x) - i64::from(center_x);
    let delta_z = i64::from(z) - i64::from(center_z);
    let distance_squared = delta_x * delta_x + delta_z * delta_z;
    let radius_squared = i64::from(radius) * i64::from(radius);
    if distance_squared >= radius_squared {
        return 0;
    }
    i32::try_from(i64::from(peak) * (radius_squared - distance_squared) / radius_squared)
        .unwrap_or_default()
}

fn window_opening(horizontal: i32, y: i32) -> bool {
    let local = horizontal.rem_euclid(8);
    (3..=5).contains(&local) && matches!(y, 4..=6 | 11..=13)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terrain_preserves_flat_gameplay_corridors() {
        for x in -5..=5 {
            for z in 17..=48 {
                assert_eq!(terrain_height(x, z), 0);
            }
        }
        assert_eq!(terrain_height(0, 0), 0);
        assert!(terrain_height(-40, 4) >= 7);
        assert!(terrain_height(40, 3) >= 7);
    }

    #[test]
    fn generated_mounds_have_stone_mass_and_a_soil_skin() {
        let world = demo_world();
        let height = terrain_height(-40, 4);

        assert_eq!(world.voxel(IVec3::new(-40, 0, 4)).material, Material::Stone);
        assert_eq!(
            world.voxel(IVec3::new(-40, height, 4)).material,
            Material::Soil
        );
        assert_eq!(
            world.voxel(IVec3::new(-40, height + 1, 4)).material,
            Material::Air
        );
    }
}
