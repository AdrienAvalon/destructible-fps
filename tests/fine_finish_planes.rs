//! Retained skin is one explicit plane, not every stair with the same outward axis.
use destructible_fps::{
    IVec3, Material, Voxel,
    mesh::fine::{
        FineMeshLimits,
        finishes::{CUT_CORE_MARKER, FinishPolicy, SurfaceFinishes},
        mesh_hybrid_chunks, mesh_hybrid_chunks_with_finishes,
    },
    volume::{LocalBox, RefinedVolume, VolumeLimits, surface::Face},
    world::geometry::{GeometryCell, GeometryChange, GeometryState, RefinedWorld},
};

fn notched_fragment() -> RefinedWorld {
    let v = RefinedVolume::uniform(Voxel::AIR)
        .replace_box(
            LocalBox::new([16, 0, 16], [224, 96, 208]).unwrap(),
            Voxel::new(Material::Brick),
            VolumeLimits::default(),
        )
        .unwrap()
        .0
        .replace_box(
            LocalBox::new([16, 0, 96], [112, 96, 208]).unwrap(),
            Voxel::AIR,
            VolumeLimits::default(),
        )
        .unwrap()
        .0;
    let mut s = GeometryState::new(RefinedWorld::default(), 1).unwrap();
    let tx = s
        .prepare(
            1,
            vec![GeometryChange {
                position: IVec3::new(0, 0, 0),
                before: GeometryCell::AIR,
                after: GeometryCell::refined(v),
            }],
        )
        .unwrap();
    s.apply(&tx).unwrap();
    s.world().clone()
}

#[test]
fn same_facing_steps_do_not_inherit_the_original_skin_plane() {
    let world = notched_fragment();
    let fingerprint = world.fingerprint();
    let chunks = [IVec3::new(0, 0, 0)];
    let plain = mesh_hybrid_chunks(&world, &chunks, FineMeshLimits::default()).unwrap();
    for (face, plane) in [
        (Face::NegativeX, 16),
        (Face::PositiveX, 224),
        (Face::NegativeZ, 16),
        (Face::PositiveZ, 208),
    ] {
        let policy = SurfaceFinishes::with_policies(
            &world,
            &[(chunks[0], FinishPolicy::CutTopAndSidesAtPlane(face, plane))],
        )
        .unwrap();
        let styled =
            mesh_hybrid_chunks_with_finishes(&world, &chunks, FineMeshLimits::default(), &policy)
                .unwrap();
        let a = &plain.meshes[0].1;
        let mut b = styled.meshes.into_iter().next().unwrap().1;
        assert_eq!(a.indices, b.indices);
        let mut preserved = 0;
        let mut cut_same_facing = 0;
        for tri in b.indices.chunks_exact(3) {
            assert!(
                tri.iter()
                    .all(|&i| b.vertices[i as usize].fracture_depth.to_bits()
                        == b.vertices[tri[0] as usize].fracture_depth.to_bits())
            );
        }
        for v in &mut b.vertices {
            let same_face = v.normal[face.axis()].to_bits()
                == if face.positive() { 1.0_f32 } else { -1.0_f32 }.to_bits();
            let on_plane =
                v.position[face.axis()].to_bits() == (f32::from(plane) / 256.0).to_bits();
            let intact = same_face && on_plane;
            let bottom = v.normal[1].to_bits() == (-1.0_f32).to_bits();
            let cut = !intact && !bottom;
            assert_eq!(
                v.fracture_depth.to_bits(),
                if cut { CUT_CORE_MARKER } else { -1.0_f32 }.to_bits()
            );
            preserved += usize::from(intact);
            cut_same_facing += usize::from(cut && same_face);
            if cut {
                v.fracture_depth = -1.0;
            }
        }
        assert!(preserved > 0);
        if [Face::PositiveZ, Face::NegativeX].contains(&face) {
            assert!(cut_same_facing > 0);
        }
        assert_eq!(
            bytemuck::cast_slice::<_, u8>(&a.vertices),
            bytemuck::cast_slice::<_, u8>(&b.vertices)
        );
        assert_eq!(world.fingerprint(), fingerprint);
    }
}

#[test]
fn plane_coordinates_are_bounded_and_part_of_the_appearance_key() {
    let world = notched_fragment();
    let p = IVec3::new(0, 0, 0);
    let make = |policy| SurfaceFinishes::with_policies(&world, &[(p, policy)]);
    for f in [Face::NegativeY, Face::PositiveY] {
        assert!(make(FinishPolicy::CutTopAndSidesAtPlane(f, 16)).is_err());
    }
    for c in [257, u16::MAX] {
        assert!(make(FinishPolicy::CutTopAndSidesAtPlane(Face::PositiveZ, c)).is_err());
    }
    let mut keys = std::collections::BTreeSet::new();
    for c in [0, 16, 96, 208, 209, 256] {
        let policy = FinishPolicy::CutTopAndSidesAtPlane(Face::PositiveZ, c);
        let key = make(policy).unwrap().fingerprint();
        assert!(keys.insert(key));
        assert_eq!(key, make(policy).unwrap().fingerprint());
    }
    assert!(
        keys.insert(
            make(FinishPolicy::CutTopAndSides(Face::PositiveZ))
                .unwrap()
                .fingerprint()
        )
    );
    assert!(keys.insert(make(FinishPolicy::CutTop).unwrap().fingerprint()));
}
