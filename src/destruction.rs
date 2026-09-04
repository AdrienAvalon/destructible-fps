use crate::material::Voxel;
use crate::world::{IVec3, VoxelChange, World};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Explosion {
    pub center: IVec3,
    pub radius_voxels: u16,
    pub peak_energy: u32,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct DestructionReport {
    pub changes: Vec<VoxelChange>,
    pub fractured_voxels: usize,
    pub damaged_voxels: usize,
    pub released_mass_kg: u64,
    pub fragment_energy: u64,
}

impl World {
    /// Applies an integer-only radial blast. Integer math makes server results independent from
    /// client CPU floating-point modes and therefore suitable for authoritative replication.
    pub fn apply_explosion(&mut self, explosion: Explosion) -> DestructionReport {
        let radius = i32::from(explosion.radius_voxels);
        let radius_squared = u64::from(explosion.radius_voxels).pow(2);
        if radius == 0 || explosion.peak_energy == 0 {
            return DestructionReport::default();
        }

        let mut report = DestructionReport::default();
        for x in
            explosion.center.x.saturating_sub(radius)..=explosion.center.x.saturating_add(radius)
        {
            for y in explosion.center.y.saturating_sub(radius)
                ..=explosion.center.y.saturating_add(radius)
            {
                for z in explosion.center.z.saturating_sub(radius)
                    ..=explosion.center.z.saturating_add(radius)
                {
                    let position = IVec3::new(x, y, z);
                    let distance_squared = position.squared_distance(explosion.center);
                    if distance_squared > radius_squared {
                        continue;
                    }
                    let before = self.voxel(position);
                    if !before.is_solid() {
                        continue;
                    }

                    let falloff = radius_squared - distance_squared + 1;
                    let delivered_energy =
                        u64::from(explosion.peak_energy) * falloff / (radius_squared + 1);
                    let properties = before.material.properties();
                    let raw_damage = delivered_energy.saturating_mul(u64::from(u8::MAX))
                        / u64::from(properties.blast_resistance.max(1));
                    let damage =
                        u8::try_from(raw_damage.min(u64::from(u8::MAX))).unwrap_or(u8::MAX);
                    if damage == 0 {
                        continue;
                    }

                    let after = if damage >= before.integrity {
                        report.fractured_voxels += 1;
                        report.released_mass_kg += u64::from(properties.density_kg_m3);
                        report.fragment_energy +=
                            delivered_energy * u64::from(properties.fragmentation) / 100;
                        Voxel::AIR
                    } else {
                        report.damaged_voxels += 1;
                        Voxel {
                            integrity: before.integrity - damage,
                            ..before
                        }
                    };
                    self.set_voxel(position, after);
                    report.changes.push(VoxelChange {
                        position,
                        before,
                        after,
                    });
                }
            }
        }
        report
            .changes
            .sort_unstable_by_key(|change| change.position);
        report
    }
}
