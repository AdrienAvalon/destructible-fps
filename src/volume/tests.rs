use super::surface::{Face, SurfaceLimits, SurfaceQuad};
use super::*;
use crate::Material;
use std::collections::BTreeSet;

#[test]
fn uniform_sampling_and_single_finest_edit_are_compact_and_exact() {
    let solid = RefinedVolume::uniform(Voxel::new(Material::Wood));
    assert_eq!(solid.leaves().len(), 1);
    assert_eq!(solid.solid_units(), VOLUME_UNITS);
    assert_eq!(solid.mass_numerator(), u64::from(VOLUME_UNITS) * 650);
    let cut = LocalBox::new([17, 31, 59], [18, 32, 60]).unwrap();
    let (after, stats) = solid
        .replace_box(cut, Voxel::AIR, VolumeLimits::default())
        .unwrap();
    assert!(stats.changed);
    assert_eq!(after.leaves().len(), 1 + 7 * usize::from(VOLUME_DEPTH));
    assert_eq!(after.solid_units(), VOLUME_UNITS - 1);
    assert_eq!(after.leaf_at([17, 31, 59]).unwrap().voxel(), Voxel::AIR);
    assert_eq!(
        after.leaf_at([18, 31, 59]).unwrap().voxel().material,
        Material::Wood
    );
    assert!(!after.overlaps_solid(cut));
    assert!(solid.overlaps_solid(cut));
    let (restored, _) = after
        .replace_box(cut, Voxel::new(Material::Wood), VolumeLimits::default())
        .unwrap();
    assert_eq!(restored.leaves(), solid.leaves());
    assert_eq!(restored.fingerprint(), solid.fingerprint());
}

#[test]
fn canonical_stream_roundtrips_without_expanding_a_dense_lattice() {
    let base = RefinedVolume::uniform(Voxel::new(Material::Steel));
    let (volume, _) = base
        .replace_box(
            LocalBox::new([8, 16, 24], [32, 40, 56]).unwrap(),
            Voxel::AIR,
            VolumeLimits::default(),
        )
        .unwrap();
    let bytes = volume.encode().unwrap();
    assert_eq!(bytes.len(), volume.encoded_bytes());
    let decoded = RefinedVolume::decode(&bytes).unwrap();
    assert_eq!(decoded.leaves(), volume.leaves());
    assert_eq!(decoded.fingerprint(), volume.fingerprint());
    assert_eq!(decoded.material_units(), volume.material_units());
}

#[test]
fn editing_is_order_independent_and_preserves_readers() {
    let base = RefinedVolume::uniform(Voxel::AIR);
    let boxes = [
        LocalBox::new([1, 2, 3], [2, 3, 4]).unwrap(),
        LocalBox::new([128, 128, 128], [192, 192, 192]).unwrap(),
    ];
    let edit = |source: &RefinedVolume, bounds| {
        source
            .replace_box(bounds, Voxel::new(Material::Brick), VolumeLimits::default())
            .unwrap()
            .0
    };
    let first = edit(&base, boxes[0]);
    let reader = first.clone();
    let a = edit(&first, boxes[1]);
    let b = edit(&edit(&base, boxes[1]), boxes[0]);
    assert_eq!(a.leaves(), b.leaves());
    assert_eq!(a.fingerprint(), b.fingerprint());
    assert_eq!(a.encode().unwrap(), b.encode().unwrap());
    assert!(Arc::ptr_eq(&first.leaves, &reader.leaves));
    assert!(!reader.overlaps_solid(boxes[1]));
    let (same, stats) = a
        .replace_box(
            boxes[0],
            Voxel::new(Material::Brick),
            VolumeLimits::default(),
        )
        .unwrap();
    assert!(!stats.changed);
    assert!(Arc::ptr_eq(&same.leaves, &a.leaves));
}

#[test]
fn limits_refuse_atomically_including_an_off_grid_high_surface_area_cut() {
    let base = RefinedVolume::uniform(Voxel::new(Material::Steel));
    let bytes = base.encode().unwrap();
    let tiny = LocalBox::new([17, 31, 59], [18, 32, 60]).unwrap();
    for (limits, error) in [
        (
            VolumeLimits {
                leaves: 1,
                visits: MAX_EDIT_VISITS,
            },
            VolumeError::LeafBudget,
        ),
        (
            VolumeLimits {
                leaves: MAX_VOLUME_LEAVES,
                visits: 16,
            },
            VolumeError::VisitBudget,
        ),
        (
            VolumeLimits {
                leaves: 0,
                visits: MAX_EDIT_VISITS,
            },
            VolumeError::InvalidLimits,
        ),
        (
            VolumeLimits {
                leaves: MAX_VOLUME_LEAVES + 1,
                visits: 1,
            },
            VolumeError::InvalidLimits,
        ),
        (
            VolumeLimits {
                leaves: 1,
                visits: 0,
            },
            VolumeError::InvalidLimits,
        ),
        (
            VolumeLimits {
                leaves: 1,
                visits: MAX_EDIT_VISITS + 1,
            },
            VolumeError::InvalidLimits,
        ),
    ] {
        assert_eq!(
            base.replace_box(tiny, Voxel::AIR, limits).unwrap_err(),
            error
        );
        assert_eq!(base.encode().unwrap(), bytes);
    }
    // This unaligned, modest-sized box is deliberately *not* hidden by increasing the cap.
    // Octree surface complexity remains a known promotion blocker for arbitrary gameplay cuts.
    assert_eq!(
        base.replace_box(
            LocalBox::new([1, 2, 3], [27, 38, 49]).unwrap(),
            Voxel::AIR,
            VolumeLimits::default()
        )
        .unwrap_err(),
        VolumeError::LeafBudget
    );
    assert_eq!(base.encode().unwrap(), bytes);
}

#[test]
fn restoring_a_refined_page_needs_only_one_final_leaf_despite_prefix_carry() {
    let base = RefinedVolume::uniform(Voxel::new(Material::Wood));
    let (cut, _) = base
        .replace_box(
            LocalBox::new([0; 3], [1; 3]).unwrap(),
            Voxel::AIR,
            VolumeLimits::default(),
        )
        .unwrap();
    let (restored, stats) = cut
        .replace_box(
            LocalBox::FULL,
            Voxel::new(Material::Wood),
            VolumeLimits {
                leaves: 1,
                visits: MAX_EDIT_VISITS,
            },
        )
        .unwrap();
    assert!(stats.peak_leaves > 1);
    assert!(stats.peak_leaves <= 1 + CANONICAL_CARRY);
    assert_eq!(restored.leaves(), base.leaves());
}

fn random(state: &mut u32) -> u32 {
    *state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
    *state
}

const fn dense_index(x: usize, y: usize, z: usize) -> usize {
    x + 8 * (y + 8 * z)
}

fn random_fixture() -> (RefinedVolume, [Voxel; 512]) {
    let mut state = 0x72bc_4d31;
    let mut volume = RefinedVolume::uniform(Voxel::AIR);
    let mut dense = [Voxel::AIR; 512];
    for iteration in 0..64 {
        let start: [u16; 3] =
            std::array::from_fn(|_| u16::try_from(random(&mut state) >> 29).unwrap());
        let end: [u16; 3] = std::array::from_fn(|axis| {
            start[axis]
                + 1
                + u16::try_from(random(&mut state) % u32::from(8 - start[axis])).unwrap()
        });
        let voxel = match iteration % 4 {
            0 => Voxel::AIR,
            1 => Voxel::new(Material::Wood),
            2 => Voxel::new(Material::Glass),
            _ => Voxel {
                material: Material::Brick,
                integrity: 17,
            },
        };
        let (next, work) = volume
            .replace_box(
                LocalBox::new(start.map(|n| n * 32), end.map(|n| n * 32)).unwrap(),
                voxel,
                VolumeLimits::default(),
            )
            .unwrap();
        assert!(work.visited <= MAX_EDIT_VISITS);
        volume = next;
        for z in start[2]..end[2] {
            for y in start[1]..end[1] {
                for x in start[0]..end[0] {
                    dense[dense_index(usize::from(x), usize::from(y), usize::from(z))] = voxel;
                }
            }
        }
        // Independent dense occupancy oracle, with no Morton encoding or tree traversal reuse.
        let mut counts = [0_u32; 8];
        let mut mass = 0_u64;
        for z in 0_u16..8 {
            for y in 0_u16..8 {
                for x in 0_u16..8 {
                    let expected =
                        dense[dense_index(usize::from(x), usize::from(y), usize::from(z))];
                    for offset in [0, 31] {
                        assert_eq!(
                            volume
                                .leaf_at([x, y, z].map(|n| n * 32 + offset))
                                .unwrap()
                                .voxel(),
                            expected
                        );
                    }
                    counts[expected.material as usize] += 32_u32.pow(3);
                    mass += u64::from(expected.material.properties().density_kg_m3) * 32_u64.pow(3);
                }
            }
        }
        assert_eq!(volume.material_units(), &counts);
        assert_eq!(volume.mass_numerator(), mass);
        assert_eq!(
            volume.leaves().iter().map(|leaf| leaf.units()).sum::<u32>(),
            VOLUME_UNITS
        );
        assert_eq!(
            RefinedVolume::decode(&volume.encode().unwrap())
                .unwrap()
                .leaves(),
            volume.leaves()
        );
    }
    (volume, dense)
}

#[test]
fn bounded_edits_match_dense_occupancy_integrity_volume_and_mass_oracle() {
    random_fixture();
}

#[test]
fn finest_points_bounds_and_material_changes_are_exact() {
    for point in [[0; 3], [255; 3], [1, 2, 3], [128, 31, 254]] {
        assert_eq!(unmorton(morton(point)), point);
        let bounds = LocalBox::new(point, point.map(|value| value + 1)).unwrap();
        let empty = RefinedVolume::uniform(Voxel::AIR);
        let (wood, _) = empty
            .replace_box(bounds, Voxel::new(Material::Wood), VolumeLimits::default())
            .unwrap();
        assert_eq!(wood.mass_numerator(), 650);
        assert_eq!(wood.solid_units(), 1);
        assert_eq!(wood.leaf_at(point).unwrap().bounds(), bounds);
        let (steel, _) = wood
            .replace_box(bounds, Voxel::new(Material::Steel), VolumeLimits::default())
            .unwrap();
        assert_ne!(wood.fingerprint(), steel.fingerprint());
        assert_eq!(steel.solid_units(), 1);
        assert_eq!(
            steel.mass_numerator(),
            u64::from(Material::Steel.properties().density_kg_m3)
        );
    }
    let empty = RefinedVolume::uniform(Voxel {
        material: Material::Air,
        integrity: 19,
    });
    assert_eq!(empty.uniform_voxel(), Some(Voxel::AIR));
    for point in [[256, 0, 0], [0, 256, 0], [0, 0, 256], [u16::MAX; 3]] {
        assert_eq!(empty.leaf_at(point), Err(VolumeError::InvalidPoint));
    }
    for (minimum, maximum) in [([0; 3], [0; 3]), ([1; 3], [0; 3]), ([0; 3], [257; 3])] {
        assert_eq!(
            LocalBox::new(minimum, maximum),
            Err(VolumeError::InvalidBox)
        );
    }
}

fn stream(leaves: &[[u8; 3]]) -> Vec<u8> {
    let mut bytes = b"DFVL\x01".to_vec();
    bytes.extend_from_slice(&u32::try_from(leaves.len()).unwrap().to_le_bytes());
    for leaf in leaves {
        bytes.extend_from_slice(leaf);
    }
    bytes
}

#[test]
fn codec_rejects_noncanonical_partitions_and_hostile_lengths() {
    let valid = stream(&[[0, Material::Wood as u8, 255]]);
    for size in 0..valid.len() {
        assert!(RefinedVolume::decode(&valid[..size]).is_err());
    }
    for invalid in [
        stream(&[]),
        stream(&[[9, 0, 0]]),
        stream(&[[0, 255, 0]]),
        stream(&[[0, 0, 1]]),
        stream(&[[1, 1, 255]]),              // Incomplete page.
        stream(&[[0, 1, 255], [0, 2, 255]]), // Overlaps/end beyond page.
        stream(&[[2, 1, 255], [1, 2, 255]]), // Unaligned next leaf.
        stream(&[[1, 1, 255]; 8]),           // Reducible siblings.
        stream(&vec![[8, 1, 255]; MAX_VOLUME_LEAVES + 1]),
    ] {
        assert!(RefinedVolume::decode(&invalid).is_err());
    }
    for index in [0, 4, 5, 8] {
        let mut invalid = valid.clone();
        invalid[index] = 255;
        assert!(RefinedVolume::decode(&invalid).is_err());
    }
    let mut trailing = valid;
    trailing.push(0);
    assert!(RefinedVolume::decode(&trailing).is_err());
}

#[test]
fn every_wire_material_and_zero_integrity_share_edit_decode_semantics() {
    for id in 0_u8..=255 {
        for integrity in [0, 1, 255] {
            let encoded = stream(&[[0, id, integrity]]);
            if id > 7 || (id == 0 && integrity != 0) {
                assert!(RefinedVolume::decode(&encoded).is_err());
                continue;
            }
            let material = Material::from_wire(id).unwrap();
            assert_eq!(material_slot(material), usize::from(id));
            let expected = RefinedVolume::uniform(Voxel {
                material,
                integrity,
            });
            let decoded = RefinedVolume::decode(&encoded).unwrap();
            assert_eq!(decoded.leaves(), expected.leaves());
            assert_eq!(decoded.material_units(), expected.material_units());
            assert_eq!(decoded.fingerprint(), expected.fingerprint());
            assert_eq!(decoded.encode().unwrap(), encoded);
            // Existing semantics: material (not integrity) defines occupancy, including strength 0.
            assert_eq!(
                decoded.solid_units(),
                if id == 0 { 0 } else { VOLUME_UNITS }
            );
        }
    }
}

#[test]
fn mutated_wire_never_panics_and_every_accepted_stream_is_canonical() {
    let base = RefinedVolume::uniform(Voxel::new(Material::Wood));
    let (volume, _) = base
        .replace_box(
            LocalBox::new([255; 3], [256; 3]).unwrap(),
            Voxel::AIR,
            VolumeLimits::default(),
        )
        .unwrap();
    let bytes = volume.encode().unwrap();
    let mut state = 0x3a7b_915e;
    for _ in 0..2_000 {
        let mut altered = bytes.clone();
        for _ in 0..=(random(&mut state) % 4) {
            let index = usize::try_from(random(&mut state)).unwrap() % altered.len();
            altered[index] = u8::try_from(random(&mut state) >> 24).unwrap();
        }
        if let Ok(decoded) = RefinedVolume::decode(&altered) {
            assert_eq!(decoded.encode().unwrap(), altered);
            assert!(decoded.leaves().len() <= MAX_VOLUME_LEAVES);
            assert_eq!(
                decoded
                    .leaves()
                    .iter()
                    .map(|leaf| leaf.units())
                    .sum::<u32>(),
                VOLUME_UNITS
            );
        }
    }
}

#[test]
fn material_checkerboard_stays_bounded_and_collapses_canonically_after_repaint() {
    let encoded: Vec<_> = (0..4096)
        .map(|index| {
            [
                4,
                if index % 2 == 0 {
                    Material::Wood as u8
                } else {
                    Material::Steel as u8
                },
                255,
            ]
        })
        .collect();
    let checker = RefinedVolume::decode(&stream(&encoded)).unwrap();
    assert_eq!(checker.leaves().len(), 4096);
    assert_eq!(checker.solid_units(), VOLUME_UNITS);
    let reader = checker.clone();
    let (repainted, stats) = checker
        .replace_box(
            LocalBox::FULL,
            Voxel::new(Material::Brick),
            VolumeLimits {
                leaves: 1,
                visits: 4096,
            },
        )
        .unwrap();
    assert_eq!(stats.visited, 4096);
    assert!(stats.peak_leaves <= 1 + CANONICAL_CARRY);
    assert_eq!(repainted.uniform_voxel(), Some(Voxel::new(Material::Brick)));
    assert_eq!(reader.encode().unwrap(), stream(&encoded));
    let air = RefinedVolume::uniform(Voxel::AIR);
    let surface = checker
        .surface([&air; 6], SurfaceLimits::default())
        .unwrap();
    assert_eq!(
        surface
            .quads
            .iter()
            .map(|quad| quad.area_units())
            .sum::<u32>(),
        6 * 256 * 256
    );
    assert_eq!(surface.quads.len(), 6 * 16 * 16);
    assert_eq!(surface.visits, 6 * 4096);
}

#[test]
fn surface_has_no_internal_material_sheets_and_requires_exact_budget() {
    let air = RefinedVolume::uniform(Voxel::AIR);
    let solid = RefinedVolume::uniform(Voxel::new(Material::Wood));
    let other = RefinedVolume::uniform(Voxel::new(Material::Brick));
    let surface = solid
        .surface(
            [&air; 6],
            SurfaceLimits {
                quads: 6,
                visits: 6,
            },
        )
        .unwrap();
    assert_eq!(surface.quads.len(), 6);
    assert_eq!(surface.visits, 6);
    assert_eq!(
        surface
            .quads
            .iter()
            .map(|quad| quad.area_units())
            .sum::<u32>(),
        6 * 256 * 256
    );
    assert!(
        solid
            .surface([&other; 6], SurfaceLimits::default())
            .unwrap()
            .quads
            .is_empty()
    );
    assert!(
        air.surface([&solid; 6], SurfaceLimits::default())
            .unwrap()
            .quads
            .is_empty()
    );
    assert_eq!(
        solid
            .surface(
                [&air; 6],
                SurfaceLimits {
                    quads: 5,
                    visits: 6
                }
            )
            .unwrap_err(),
        VolumeError::SurfaceBudget
    );
    assert_eq!(
        solid
            .surface(
                [&air; 6],
                SurfaceLimits {
                    quads: 6,
                    visits: 5
                }
            )
            .unwrap_err(),
        VolumeError::VisitBudget
    );
    for limits in [
        SurfaceLimits {
            quads: 0,
            visits: 1,
        },
        SurfaceLimits {
            quads: 1,
            visits: 0,
        },
        SurfaceLimits {
            quads: surface::MAX_SURFACE_QUADS + 1,
            visits: 1,
        },
        SurfaceLimits {
            quads: 1,
            visits: surface::MAX_SURFACE_VISITS + 1,
        },
    ] {
        assert_eq!(
            solid.surface([&air; 6], limits).unwrap_err(),
            VolumeError::InvalidLimits
        );
    }
    assert_eq!(solid.leaves().len(), 1);
}

fn boundary(quad: &SurfaceQuad, face: Face) -> bool {
    quad.face() == face
        && quad.origin()[face.axis()] == if face.positive() { VOLUME_EDGE } else { 0 }
}

#[test]
fn coarsest_solid_against_finest_neighbor_hole_has_exact_six_direction_seams() {
    let solid = RefinedVolume::uniform(Voxel::new(Material::Wood));
    for face in Face::ALL {
        let mut point = [73, 91, 117];
        point[face.axis()] = if face.positive() { 0 } else { 255 };
        let (neighbor, _) = solid
            .replace_box(
                LocalBox::new(point, point.map(|v| v + 1)).unwrap(),
                Voxel::AIR,
                VolumeLimits::default(),
            )
            .unwrap();
        let mut neighbors = [&solid; 6];
        neighbors[face.index()] = &neighbor;
        let surface = solid.surface(neighbors, SurfaceLimits::default()).unwrap();
        assert_eq!(surface.quads.len(), 1);
        let quad = surface.quads[0];
        assert!(boundary(&quad, face));
        assert_eq!(quad.edge(), 1);
        for axis in (0..3).filter(|&axis| axis != face.axis()) {
            assert_eq!(quad.origin()[axis], point[axis]);
        }
        // Neighbor emits no duplicate sheet at the same interface: it faces occupied material.
        let reverse = neighbor
            .surface([&solid; 6], SurfaceLimits::default())
            .unwrap();
        assert!(
            !reverse
                .quads
                .iter()
                .any(|quad| boundary(quad, face.opposite()))
        );
    }
}

#[test]
fn recursive_surface_refusals_discard_output_and_preserve_both_page_readers() {
    // A valid complete depth-four checkerboard, not an impossible incomplete stream consisting
    // of 8192 depth-eight leaves. Many fine solid/air seams force actual recursive subdivision.
    let encoded: Vec<_> = (0_u32..4096)
        .map(|index| {
            if index.count_ones() % 2 == 0 {
                [4, Material::Wood as u8, 255]
            } else {
                [4, 0, 0]
            }
        })
        .collect();
    let neighbor = RefinedVolume::decode(&stream(&encoded)).unwrap();
    let solid = RefinedVolume::uniform(Voxel::new(Material::Brick));
    let old_neighbor = neighbor.encode().unwrap();
    let old_solid = solid.encode().unwrap();
    for face in Face::ALL {
        let mut neighbors = [&solid; 6];
        neighbors[face.index()] = &neighbor;
        let complete = solid.surface(neighbors, SurfaceLimits::default()).unwrap();
        assert_eq!(complete.quads.len(), 128);
        assert!(complete.visits > 8);
        assert_eq!(
            solid
                .surface(
                    neighbors,
                    SurfaceLimits {
                        quads: 1,
                        visits: surface::MAX_SURFACE_VISITS
                    }
                )
                .unwrap_err(),
            VolumeError::SurfaceBudget
        );
        assert_eq!(
            solid
                .surface(
                    neighbors,
                    SurfaceLimits {
                        quads: surface::MAX_SURFACE_QUADS,
                        visits: 8
                    }
                )
                .unwrap_err(),
            VolumeError::VisitBudget
        );
        assert_eq!(neighbor.encode().unwrap(), old_neighbor);
        assert_eq!(solid.encode().unwrap(), old_solid);
        assert_eq!(
            solid
                .surface(neighbors, SurfaceLimits::default())
                .unwrap()
                .quads,
            complete.quads
        );
    }
}

#[test]
fn surfaces_match_independent_dense_face_oracle_without_duplicates() {
    let (volume, dense) = random_fixture();
    let air = RefinedVolume::uniform(Voxel::AIR);
    let surface = volume.surface([&air; 6], SurfaceLimits::default()).unwrap();
    let mut actual = BTreeSet::new();
    for quad in surface.quads {
        assert!(quad.edge() >= 32 && quad.edge().is_multiple_of(32));
        assert!(quad.origin().iter().all(|value| value.is_multiple_of(32)));
        let axis = quad.face().axis();
        let u = (axis + 1) % 3;
        let v = (axis + 2) % 3;
        for du in 0..quad.edge() / 32 {
            for dv in 0..quad.edge() / 32 {
                let mut point = quad.origin().map(|value| value / 32);
                point[u] += du;
                point[v] += dv;
                assert!(
                    actual.insert((
                        quad.face().index(),
                        point,
                        quad.voxel().material as u8,
                        quad.voxel().integrity
                    )),
                    "duplicate surface square"
                );
            }
        }
    }
    let mut expected = BTreeSet::new();
    for z in 0_u16..8 {
        for y in 0_u16..8 {
            for x in 0_u16..8 {
                let point = [x, y, z];
                let voxel = dense[dense_index(usize::from(x), usize::from(y), usize::from(z))];
                if !voxel.is_solid() {
                    continue;
                }
                for face in Face::ALL {
                    let mut adjacent = point.map(i32::from);
                    adjacent[face.axis()] += if face.positive() { 1 } else { -1 };
                    let empty = adjacent.iter().any(|&value| !(0..8).contains(&value)) || {
                        let p = adjacent.map(|value| usize::try_from(value).unwrap());
                        !dense[dense_index(p[0], p[1], p[2])].is_solid()
                    };
                    if empty {
                        let mut origin = point;
                        if face.positive() {
                            origin[face.axis()] += 1;
                        }
                        expected.insert((
                            face.index(),
                            origin,
                            voxel.material as u8,
                            voxel.integrity,
                        ));
                    }
                }
            }
        }
    }
    assert_eq!(actual, expected);
}
