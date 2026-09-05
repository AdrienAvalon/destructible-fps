//! Reproducible playable load-failure lab. Synthetic constants, not realistic material presets.

use crate::{
    IVec3, Material, StructuralAnchors, Voxel, World,
    structural_failure::{SectionStrength, StructuralStrengths},
    structural_jobs::{ElasticMaterial, StructuralMaterials},
    structural_runtime::StructuralSimulationConfig,
};

pub const LAB_SEED: IVec3 = IVec3::new(1, 4, 0);

#[must_use]
/// # Panics
/// Panics if internal fixture constants violate the validated model's input contracts.
pub fn structural_lab() -> (World, StructuralSimulationConfig) {
    let mut world = World::default();
    world.fill_box(
        IVec3::new(-16, 0, -8),
        IVec3::new(16, 0, 45),
        Voxel::new(Material::Stone),
    );
    world.fill_box(
        IVec3::new(0, 1, 0),
        IVec3::new(0, 4, 0),
        Voxel::new(Material::Stone),
    );
    world.fill_box(LAB_SEED, IVec3::new(5, 4, 0), Voxel::new(Material::Wood));
    let config = StructuralSimulationConfig {
        anchors: StructuralAnchors::foundation_plane(0)
            .with_explicit([IVec3::new(0, 4, 0)])
            .expect("bounded lab anchor"),
        materials: StructuralMaterials::new(
            [ElasticMaterial {
                young_modulus_pa: 1e9,
                poisson_ratio: 0.25,
            }; 7],
        )
        .expect("finite synthetic lab elasticity"),
        strengths: StructuralStrengths::new(
            [SectionStrength {
                tension_pa: 1e6,
                compression_pa: 5e6,
                shear_pa: 1e6,
            }; 7],
        )
        .expect("finite synthetic lab strengths"),
        initial_seeds: vec![LAB_SEED],
    };
    (world, config)
}
