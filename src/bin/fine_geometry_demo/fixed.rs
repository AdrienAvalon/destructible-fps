//! Bind immutable inspected sources to their one-time fixed render meshes before presentation.
use destructible_fps::{
    convex::InspectionGeometry,
    mesh::{CpuBodyMesh, fine::fixture::STAGE_NAMES},
    world::geometry::RefinedWorld,
};
use std::sync::Arc;

pub struct PreparedGeometry {
    geometry: Vec<InspectionGeometry>,
    meshes: Vec<CpuBodyMesh>,
}

impl PreparedGeometry {
    pub fn new(
        worlds: &[Arc<RefinedWorld>],
        geometry: Vec<InspectionGeometry>,
    ) -> Result<Self, String> {
        if worlds.len() != STAGE_NAMES.len() || geometry.len() != STAGE_NAMES.len() {
            return Err("fixed geometry requires every inspection stage".into());
        }
        let reference = &geometry[0];
        for (stage, (world, scene)) in worlds.iter().zip(&geometry).enumerate() {
            if !Arc::ptr_eq(world, scene.world()) {
                return Err(format!(
                    "fixed geometry world snapshot mismatch at stage {stage}"
                ));
            }
            // Full canonical equality, including vertices, faces, material and cut provenance:
            // a matching count or a cache fingerprint alone cannot bind a fixed mesh source.
            if scene.fragments() != reference.fragments() {
                return Err(format!("fixed geometry fragment mismatch at stage {stage}"));
            }
        }
        let mut meshes = Vec::new();
        meshes
            .try_reserve_exact(reference.fragments().len())
            .map_err(|_| "fixed geometry mesh-list allocation")?;
        for (index, fragment) in reference.fragments().iter().enumerate() {
            // InspectionGeometry's private constructor already caps the collection at 32.
            let id = u64::try_from(index)
                .map_err(|_| "fixed geometry identifier")?
                .checked_add(1)
                .ok_or("fixed geometry identifier overflow")?;
            meshes.push(fragment.body_mesh(id));
        }
        Ok(Self { geometry, meshes })
    }

    pub fn into_parts(self) -> (Vec<InspectionGeometry>, Vec<CpuBodyMesh>) {
        (self.geometry, self.meshes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use destructible_fps::{
        IVec3, Material, Voxel,
        convex::{ConvexFaceInput, ConvexFragment, fixture::industrial_scene},
        mesh::fine::fixture::industrial_oblique_base,
    };

    fn worlds() -> Vec<Arc<RefinedWorld>> {
        (0..STAGE_NAMES.len())
            .map(|_| Arc::new(RefinedWorld::default()))
            .collect()
    }

    fn empty_geometry(worlds: &[Arc<RefinedWorld>]) -> Vec<InspectionGeometry> {
        worlds
            .iter()
            .map(|world| InspectionGeometry::new(Arc::clone(world), Vec::new()).unwrap())
            .collect()
    }

    fn fragment(x: i32, cut: bool) -> ConvexFragment {
        let vertices = [
            [0, 0, 0],
            [64, 0, 0],
            [64, 0, 64],
            [0, 0, 64],
            [0, 64, 0],
            [64, 64, 0],
            [64, 64, 64],
            [0, 64, 64],
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
        .map(|indices| ConvexFaceInput {
            indices: indices.to_vec(),
            cut,
        })
        .collect::<Vec<_>>();
        ConvexFragment::new(
            IVec3::new(x, 1, 1),
            &vertices,
            &faces,
            Voxel::new(Material::Brick),
        )
        .unwrap()
    }

    #[test]
    fn every_source_stage_is_required_before_preparation() {
        let worlds = worlds();
        for count in [0, 1, STAGE_NAMES.len() - 1] {
            assert!(PreparedGeometry::new(&worlds[..count], empty_geometry(&worlds)).is_err());
            assert!(PreparedGeometry::new(&worlds, empty_geometry(&worlds[..count])).is_err());
        }
        let mut extra_worlds = worlds;
        extra_worlds.push(Arc::new(RefinedWorld::default()));
        assert!(PreparedGeometry::new(&extra_worlds, empty_geometry(&extra_worlds)).is_err());
    }

    #[test]
    fn an_equal_world_value_cannot_substitute_for_the_inspected_snapshot_arc() {
        let worlds = worlds();
        let mut geometry = empty_geometry(&worlds);
        let other = Arc::new(worlds[2].as_ref().clone());
        assert_eq!(other.fingerprint(), worlds[2].fingerprint());
        assert!(!Arc::ptr_eq(&other, &worlds[2]));
        geometry[2] = InspectionGeometry::new(other, Vec::new()).unwrap();
        let error = PreparedGeometry::new(&worlds, geometry).err().unwrap();
        assert!(error.contains("world snapshot mismatch at stage 2"));
    }

    #[test]
    fn moving_a_fragment_or_changing_only_its_finish_refuses_fixed_publication() {
        let worlds = worlds();
        let original = fragment(1, true);
        for changed in [fragment(2, true), fragment(1, false)] {
            let mut geometry: Vec<_> = worlds
                .iter()
                .map(|world| {
                    InspectionGeometry::new(Arc::clone(world), vec![original.clone()]).unwrap()
                })
                .collect();
            geometry[2] = InspectionGeometry::new(Arc::clone(&worlds[2]), vec![changed]).unwrap();
            let error = PreparedGeometry::new(&worlds, geometry).err().unwrap();
            assert!(error.contains("fragment mismatch at stage 2"));
        }
        let mut geometry = empty_geometry(&worlds);
        geometry[3] = InspectionGeometry::new(Arc::clone(&worlds[3]), vec![original]).unwrap();
        assert!(PreparedGeometry::new(&worlds, geometry).is_err());
    }

    fn verify_prepared(worlds: &[Arc<RefinedWorld>], geometry: Vec<InspectionGeometry>) {
        let (scenes, meshes) = PreparedGeometry::new(worlds, geometry)
            .unwrap()
            .into_parts();
        assert_eq!(scenes.len(), STAGE_NAMES.len());
        for (scene, world) in scenes.iter().zip(worlds) {
            assert!(Arc::ptr_eq(scene.world(), world));
            assert_eq!(scene.fragments(), scenes[0].fragments());
        }
        assert_eq!(meshes.len(), scenes[0].fragments().len());
        for (index, (mesh, fragment)) in meshes.iter().zip(scenes[0].fragments()).enumerate() {
            let id = u64::try_from(index).unwrap() + 1;
            let expected = fragment.body_mesh(id);
            assert_eq!(mesh.body_id, id);
            assert_eq!(mesh.origin, expected.origin);
            assert_eq!(mesh.maximum, expected.maximum);
            assert_eq!(
                mesh.rotation_pivot.map(f32::to_bits),
                expected.rotation_pivot.map(f32::to_bits)
            );
            assert_eq!(mesh.mesh.indices, expected.mesh.indices);
            assert_eq!(
                bytemuck::cast_slice::<_, u8>(&mesh.mesh.vertices),
                bytemuck::cast_slice::<_, u8>(&expected.mesh.vertices)
            );
        }
    }

    #[test]
    fn empty_four_stage_sources_prepare_without_any_fixed_draws() {
        let worlds = worlds();
        verify_prepared(&worlds, empty_geometry(&worlds));
    }

    #[test]
    fn industrial_stages_bind_exact_worlds_and_byte_identical_fixed_meshes() {
        let worlds: Vec<_> = (0..STAGE_NAMES.len())
            .map(|stage| Arc::new(industrial_oblique_base(stage).unwrap()))
            .collect();
        let geometry = worlds
            .iter()
            .map(|world| industrial_scene(Arc::clone(world)).unwrap())
            .collect();
        verify_prepared(&worlds, geometry);
    }
}
