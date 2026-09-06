//! Mixed rubble appearance is opt-in metadata, never a physical-material replacement.
use destructible_fps::{
    IVec3, Material, Voxel,
    mesh::fine::{
        FineMeshError, FineMeshLimits,
        finishes::{
            CUT_CORE_MARKER, FinishPolicy, MAX_FINISH_CELLS, MAX_FINISH_LEAVES, SurfaceFinishes,
        },
        mesh_hybrid_chunks, mesh_hybrid_chunks_with_finishes,
    },
    volume::{LocalBox, RefinedVolume, VolumeLimits, surface::Face},
    world::geometry::{GeometryCell, GeometryChange, GeometryState, RefinedWorld},
};
use std::collections::BTreeSet;

const OWNER: IVec3 = IVec3::new(1, 1, 1);
const NEIGHBOR: IVec3 = IVec3::new(3, 1, 1);
const CHUNK: IVec3 = IVec3::new(0, 0, 0);

fn fragments(materials: &[Material]) -> GeometryCell {
    assert!(!materials.is_empty() && materials.len() <= 3);
    let mut volume = RefinedVolume::uniform(Voxel::AIR);
    for (i, &material) in materials.iter().enumerate() {
        let x = 16 + u16::try_from(i).unwrap() * 80;
        volume = volume
            .replace_box(
                LocalBox::new([x, 32, 32], [x + 48, 96, 96]).unwrap(),
                Voxel::new(material),
                VolumeLimits::default(),
            )
            .unwrap()
            .0;
    }
    GeometryCell::refined(volume)
}

fn install(cell: &GeometryCell, positions: &[IVec3]) -> RefinedWorld {
    let mut state = GeometryState::new(RefinedWorld::default(), 1).unwrap();
    if *cell != GeometryCell::AIR {
        for batch in positions.chunks(8) {
            let tx = state
                .prepare(
                    1,
                    batch
                        .iter()
                        .map(|&position| GeometryChange {
                            position,
                            before: GeometryCell::AIR,
                            after: cell.clone(),
                        })
                        .collect(),
                )
                .unwrap();
            state.apply(&tx).unwrap();
        }
    }
    state.world().clone()
}

fn policy(world: &RefinedWorld, finish: FinishPolicy) -> Result<SurfaceFinishes, FineMeshError> {
    SurfaceFinishes::with_policies(world, &[(OWNER, finish)])
}

#[test]
fn mixed_rubble_acceptance_does_not_relax_the_original_homogeneous_policies() {
    for materials in [
        vec![Material::Brick],
        vec![Material::Concrete],
        vec![Material::Brick, Material::Concrete],
        vec![Material::Brick, Material::Stone],
        vec![Material::Concrete, Material::Stone],
        vec![Material::Brick, Material::Concrete, Material::Stone],
    ] {
        let world = install(&fragments(&materials), &[OWNER]);
        assert!(policy(&world, FinishPolicy::BrokenMasonry).is_ok());
        for old in [
            FinishPolicy::CutTop,
            FinishPolicy::CutTopAndSides(Face::PositiveZ),
            FinishPolicy::CutTopAndSidesAtPlane(Face::PositiveZ, 96),
        ] {
            assert_eq!(
                policy(&world, old).is_ok(),
                materials.len() == 1,
                "old homogeneous policy changed for {materials:?}"
            );
        }
    }
    for material in [
        Material::Soil,
        Material::Wood,
        Material::Steel,
        Material::Glass,
    ] {
        for materials in [vec![material], vec![Material::Brick, material]] {
            let world = install(&fragments(&materials), &[OWNER]);
            assert!(
                matches!(
                    policy(&world, FinishPolicy::BrokenMasonry),
                    Err(FineMeshError::SurfaceFinish)
                ),
                "accepted unsupported {materials:?}"
            );
        }
    }
    for cell in [
        fragments(&[Material::Stone]),
        GeometryCell::AIR,
        GeometryCell::uniform(Voxel::new(Material::Brick)),
    ] {
        assert!(policy(&install(&cell, &[OWNER]), FinishPolicy::BrokenMasonry).is_err());
    }
}

#[test]
fn all_six_masonry_faces_are_cut_but_stone_and_unenrolled_mesh_bytes_are_unchanged() {
    let cell = fragments(&[Material::Brick, Material::Concrete, Material::Stone]);
    let world = install(&cell, &[OWNER, NEIGHBOR]);
    let fingerprint = world.fingerprint();
    let finishes = policy(&world, FinishPolicy::BrokenMasonry).unwrap();
    let plain = mesh_hybrid_chunks(&world, &[CHUNK], FineMeshLimits::default()).unwrap();
    let empty = mesh_hybrid_chunks_with_finishes(
        &world,
        &[CHUNK],
        FineMeshLimits::default(),
        &SurfaceFinishes::default(),
    )
    .unwrap();
    let styled =
        mesh_hybrid_chunks_with_finishes(&world, &[CHUNK], FineMeshLimits::default(), &finishes)
            .unwrap();
    assert_eq!(plain.report.vertices, styled.report.vertices);
    assert_eq!(plain.report.indices, styled.report.indices);
    assert_eq!(plain.report.quads, styled.report.quads);
    let leaves = cell.volume().unwrap().leaves().len();
    // Two equal, isolated pages: exactly half the surface quads belong to the enrolled owner.
    assert_eq!(
        styled.report.work - empty.report.work,
        1 + 2 * leaves + 8 * (styled.report.quads / 2)
    );
    let a = &plain.meshes[0].1;
    let mut b = styled.meshes.into_iter().next().unwrap().1;
    assert_eq!(a.indices, b.indices);
    let mut seen = [[false; 6]; 3];
    let mut untouched_neighbor = 0;
    for triangle in b.indices.chunks_exact(3) {
        let first = b.vertices[triangle[0] as usize];
        assert!(triangle.iter().all(|&i| {
            let vertex = b.vertices[i as usize];
            vertex.fracture_depth.to_bits() == first.fracture_depth.to_bits()
                && vertex.material == first.material
        }));
    }
    for vertex in &mut b.vertices {
        let enrolled = vertex.position[0] < 2.0;
        let material = [Material::Brick, Material::Concrete, Material::Stone]
            .into_iter()
            .position(|m| vertex.material == u32::from(m as u8))
            .unwrap();
        let expected_cut = enrolled && material < 2;
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
            let face = Face::ALL
                .into_iter()
                .find(|face| {
                    vertex.normal[face.axis()].to_bits()
                        == if face.positive() { 1.0_f32 } else { -1.0_f32 }.to_bits()
                })
                .unwrap();
            seen[material][face.index()] = true;
        } else {
            untouched_neighbor += 1;
        }
        if expected_cut {
            vertex.fracture_depth = -1.0;
        }
    }
    assert!(seen.into_iter().flatten().all(|face| face));
    assert!(untouched_neighbor > 0);
    assert_eq!(
        bytemuck::cast_slice::<_, u8>(&a.vertices),
        bytemuck::cast_slice::<_, u8>(&b.vertices)
    );
    assert_eq!(world.fingerprint(), fingerprint);
    assert_eq!(world.cell(OWNER), cell);
    assert_eq!(world.cell(NEIGHBOR), cell);
}

#[test]
fn rubble_policy_has_a_distinct_reproducible_key_and_stale_sources_fail_closed() {
    let world = install(&fragments(&[Material::Brick]), &[OWNER]);
    let mut keys = BTreeSet::new();
    for finish in [
        FinishPolicy::CutTop,
        FinishPolicy::CutTopAndSides(Face::PositiveZ),
        FinishPolicy::CutTopAndSidesAtPlane(Face::PositiveZ, 96),
        FinishPolicy::BrokenMasonry,
    ] {
        let key = policy(&world, finish).unwrap().fingerprint();
        assert_ne!(key, 0);
        assert!(keys.insert(key));
        assert_eq!(key, policy(&world, finish).unwrap().fingerprint());
    }
    let finishes = policy(&world, FinishPolicy::BrokenMasonry).unwrap();
    for after in [
        GeometryCell::AIR,
        fragments(&[Material::Brick, Material::Stone]),
    ] {
        let mut state = GeometryState::new(world.clone(), 1).unwrap();
        let tx = state
            .prepare(
                1,
                vec![GeometryChange {
                    position: OWNER,
                    before: world.cell(OWNER),
                    after,
                }],
            )
            .unwrap();
        state.apply(&tx).unwrap();
        let fingerprint = state.world().fingerprint();
        assert!(matches!(
            mesh_hybrid_chunks_with_finishes(
                state.world(),
                &[CHUNK],
                FineMeshLimits::default(),
                &finishes
            ),
            Err(FineMeshError::SurfaceFinish)
        ));
        assert_eq!(state.world().fingerprint(), fingerprint);
    }
}

#[test]
fn rubble_policy_retains_sorted_unique_cell_limits_and_mesh_budgets() {
    let cell = fragments(&[Material::Brick, Material::Stone]);
    let positions: Vec<_> = (0..=MAX_FINISH_CELLS)
        .map(|x| IVec3::new(i32::try_from(x).unwrap(), 1, 1))
        .collect();
    let world = install(&cell, &positions);
    let entries: Vec<_> = positions
        .into_iter()
        .map(|p| (p, FinishPolicy::BrokenMasonry))
        .collect();
    assert!(SurfaceFinishes::with_policies(&world, &entries[..MAX_FINISH_CELLS]).is_ok());
    for invalid in [
        entries.clone(),
        vec![entries[0], entries[0]],
        vec![entries[1], entries[0]],
        vec![(IVec3::new(-1, 1, 1), FinishPolicy::BrokenMasonry)],
    ] {
        assert!(matches!(
            SurfaceFinishes::with_policies(&world, &invalid),
            Err(FineMeshError::SurfaceFinish)
        ));
    }
    let world = install(&cell, &[OWNER]);
    let fingerprint = world.fingerprint();
    let finishes = policy(&world, FinishPolicy::BrokenMasonry).unwrap();
    let limits = FineMeshLimits::default();
    for limited in [
        FineMeshLimits { work: 1, ..limits },
        FineMeshLimits {
            vertices: 1,
            ..limits
        },
        FineMeshLimits {
            indices: 1,
            ..limits
        },
        FineMeshLimits { quads: 1, ..limits },
        FineMeshLimits { lines: 1, ..limits },
    ] {
        assert!(mesh_hybrid_chunks_with_finishes(&world, &[CHUNK], limited, &finishes).is_err());
        assert_eq!(world.fingerprint(), fingerprint);
    }
    let complete = mesh_hybrid_chunks_with_finishes(&world, &[CHUNK], limits, &finishes).unwrap();
    assert!(matches!(
        mesh_hybrid_chunks_with_finishes(
            &world,
            &[CHUNK],
            FineMeshLimits {
                work: complete.report.work - 1,
                ..limits
            },
            &finishes
        ),
        Err(FineMeshError::WorkBudget)
    ));
}

#[test]
fn mixed_pages_respect_the_exact_aggregate_finish_leaf_limit() {
    // 256 alternating one-unit runs make the independent fixture's total exact, not estimated.
    let mut volume = RefinedVolume::uniform(Voxel::new(Material::Brick));
    for x in (1..256).step_by(2) {
        volume = volume
            .replace_box(
                LocalBox::new([x, 0, 0], [x + 1, 256, 256]).unwrap(),
                Voxel::new(Material::Stone),
                VolumeLimits::default(),
            )
            .unwrap()
            .0;
    }
    assert_eq!(volume.leaves().len(), 256);
    assert_eq!(MAX_FINISH_LEAVES % 256, 0);
    let allowed = MAX_FINISH_LEAVES / 256;
    let positions: Vec<_> = (0..=allowed)
        .map(|x| IVec3::new(i32::try_from(x).unwrap(), 0, 0))
        .collect();
    let world = install(&GeometryCell::refined(volume), &positions);
    let entries: Vec<_> = positions
        .into_iter()
        .map(|p| (p, FinishPolicy::BrokenMasonry))
        .collect();
    assert!(entries.len() <= MAX_FINISH_CELLS);
    assert_eq!(
        world.geometry_stats().refined_leaves,
        MAX_FINISH_LEAVES + 256
    );
    assert!(SurfaceFinishes::with_policies(&world, &entries[..allowed]).is_ok());
    assert!(matches!(
        SurfaceFinishes::with_policies(&world, &entries),
        Err(FineMeshError::SurfaceFinish)
    ));
}
