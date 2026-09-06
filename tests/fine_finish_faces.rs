//! Independent six-face oracle for explicit authored fragment finishes.
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

fn fixture(material: Material) -> RefinedWorld {
    let volume = RefinedVolume::uniform(Voxel::AIR)
        .replace_box(
            LocalBox::new([16, 0, 16], [224, 96, 208]).unwrap(),
            Voxel::new(material),
            VolumeLimits::default(),
        )
        .unwrap()
        .0;
    let mut state = GeometryState::new(RefinedWorld::default(), 1).unwrap();
    let tx = state
        .prepare(
            1,
            [1, 3]
                .into_iter()
                .map(|x| GeometryChange {
                    position: IVec3::new(x, 1, 1),
                    before: GeometryCell::AIR,
                    after: GeometryCell::refined(volume.clone()),
                })
                .collect(),
        )
        .unwrap();
    state.apply(&tx).unwrap();
    state.world().clone()
}

#[test]
fn each_original_side_is_preserved_with_exact_geometry_and_unenrolled_neighbor_bytes() {
    let position = IVec3::new(1, 1, 1);
    let chunks = [IVec3::new(0, 0, 0)];
    for material in [Material::Brick, Material::Concrete] {
        let world = fixture(material);
        let fingerprint = world.fingerprint();
        let plain = mesh_hybrid_chunks(&world, &chunks, FineMeshLimits::default()).unwrap();
        let empty = mesh_hybrid_chunks_with_finishes(
            &world,
            &chunks,
            FineMeshLimits::default(),
            &SurfaceFinishes::default(),
        )
        .unwrap();
        // Two refined cells: the pre-existing seven-unit registry lookup per cell,
        // but no per-quad predicate charge on unenrolled surfaces.
        assert_eq!(empty.report.work - plain.report.work, 14);
        let mut keys = std::collections::BTreeSet::new();
        let top = SurfaceFinishes::cut_tops(&world, &[position]).unwrap();
        assert_eq!(
            top.fingerprint(),
            SurfaceFinishes::with_policies(&world, &[(position, FinishPolicy::CutTop)])
                .unwrap()
                .fingerprint()
        );
        keys.insert(top.fingerprint());
        let top_mesh =
            mesh_hybrid_chunks_with_finishes(&world, &chunks, FineMeshLimits::default(), &top)
                .unwrap();
        let leaves = world.cell(position).volume().unwrap().leaves().len();
        assert_eq!(
            top_mesh.report.work - empty.report.work,
            1 + 2 * leaves + 4 * 6
        );
        for intact in [
            Face::NegativeX,
            Face::PositiveX,
            Face::NegativeZ,
            Face::PositiveZ,
        ] {
            let policy = [(position, FinishPolicy::CutTopAndSides(intact))];
            let finishes = SurfaceFinishes::with_policies(&world, &policy).unwrap();
            assert!(
                keys.insert(finishes.fingerprint()),
                "different policy must invalidate the worker key"
            );
            assert_eq!(
                finishes.fingerprint(),
                SurfaceFinishes::with_policies(&world, &policy)
                    .unwrap()
                    .fingerprint()
            );
            let styled = mesh_hybrid_chunks_with_finishes(
                &world,
                &chunks,
                FineMeshLimits::default(),
                &finishes,
            )
            .unwrap();
            let a = &plain.meshes[0].1;
            assert_eq!(styled.report.work, top_mesh.report.work);
            verify_mesh(a, styled.meshes.into_iter().next().unwrap().1, intact);
            assert_eq!(world.fingerprint(), fingerprint);
        }
    }
}

fn verify_mesh(
    a: &destructible_fps::mesh::CpuMesh,
    mut b: destructible_fps::mesh::CpuMesh,
    intact: Face,
) {
    assert_eq!(a.indices, b.indices);
    let mut seen = [false; 6];
    let mut untouched = 0;
    for tri in b.indices.chunks_exact(3) {
        let marker = b.vertices[tri[0] as usize].fracture_depth.to_bits();
        assert!(
            tri.iter()
                .all(|&i| b.vertices[i as usize].fracture_depth.to_bits() == marker)
        );
    }
    for vertex in &mut b.vertices {
        let face = Face::ALL
            .into_iter()
            .find(|f| {
                vertex.normal[f.axis()].to_bits()
                    == if f.positive() { 1.0_f32 } else { -1.0_f32 }.to_bits()
            })
            .unwrap();
        let enrolled = vertex.position[0] < 2.0;
        let expected_cut = enrolled && face != intact && face != Face::NegativeY;
        assert_eq!(
            vertex.fracture_depth.to_bits(),
            if expected_cut {
                CUT_CORE_MARKER
            } else {
                -1.0_f32
            }
            .to_bits()
        );
        if enrolled {
            seen[face.index()] = true;
        } else {
            untouched += 1;
        }
        if expected_cut {
            vertex.fracture_depth = -1.0;
        }
    }
    assert!(seen.into_iter().all(|s| s) && untouched > 0);
    assert_eq!(
        bytemuck::cast_slice::<_, u8>(&a.vertices),
        bytemuck::cast_slice::<_, u8>(&b.vertices)
    );
}

#[test]
fn new_policy_cannot_bypass_orientation_source_or_list_guards() {
    let world = fixture(Material::Brick);
    let p = IVec3::new(1, 1, 1);
    let valid = (p, FinishPolicy::CutTopAndSides(Face::PositiveZ));
    for entries in [
        vec![(p, FinishPolicy::CutTopAndSides(Face::PositiveY))],
        vec![(p, FinishPolicy::CutTopAndSides(Face::NegativeY))],
        vec![valid, valid],
        vec![valid, (p, FinishPolicy::CutTop)],
        vec![valid; 65],
        vec![(IVec3::new(3, 1, 1), valid.1), valid],
        vec![(IVec3::new(0, 0, 0), valid.1)],
    ] {
        assert!(SurfaceFinishes::with_policies(&world, &entries).is_err());
    }
    assert!(SurfaceFinishes::with_policies(&fixture(Material::Steel), &[valid]).is_err());
    let finishes = SurfaceFinishes::with_policies(&world, &[valid]).unwrap();
    assert!(
        mesh_hybrid_chunks_with_finishes(
            &RefinedWorld::default(),
            &[IVec3::new(0, 0, 0)],
            FineMeshLimits::default(),
            &finishes
        )
        .is_err()
    );
}
