use super::surface::{Face, SurfaceLimits, SurfaceQuad};
use super::*;
use std::collections::BTreeSet;

pub(super) const REGULAR: [u16; 9] = [0, 32, 64, 96, 128, 160, 192, 224, 256];
pub(super) const IRREGULAR: [u16; 9] = [0, 1, 7, 18, 37, 89, 151, 217, 256];

#[test]
fn finest_edit_and_offgrid_box_are_compact_exact_and_reversible() {
    let solid = RefinedVolume::uniform(Voxel::new(Material::Wood));
    for cut in [
        LocalBox::new([17, 31, 59], [18, 32, 60]).unwrap(),
        LocalBox::new([1, 2, 3], [27, 38, 49]).unwrap(),
    ] {
        let (after, work) = solid
            .replace_box(cut, Voxel::AIR, VolumeLimits::default())
            .unwrap();
        assert!(work.changed);
        assert_eq!(after.leaves().len(), 7);
        assert_eq!(after.solid_units(), VOLUME_UNITS - cut.units());
        assert_eq!(
            after.mass_numerator(),
            u64::from(VOLUME_UNITS - cut.units()) * 650
        );
        assert_eq!(after.leaf_at(cut.minimum()).unwrap().voxel(), Voxel::AIR);
        assert!(!after.overlaps_solid(cut));
        assert!(solid.overlaps_solid(cut));
        let (restored, _) = after
            .replace_box(cut, Voxel::new(Material::Wood), VolumeLimits::default())
            .unwrap();
        assert_eq!(restored.leaves(), solid.leaves());
        assert_eq!(restored.fingerprint(), solid.fingerprint());
        let bytes = after.encode().unwrap();
        assert_eq!(bytes.len(), 65);
        assert_eq!(
            RefinedVolume::decode(&bytes).unwrap().leaves(),
            after.leaves()
        );
    }
}

#[test]
fn internal_offset_box_has_seven_leaves_in_every_axis_permutation() {
    let base = RefinedVolume::uniform(Voxel::new(Material::Concrete));
    for axes in [
        [0, 1, 2],
        [0, 2, 1],
        [1, 0, 2],
        [1, 2, 0],
        [2, 0, 1],
        [2, 1, 0],
    ] {
        let cut = LocalBox::new(axes.map(|i| [1, 2, 3][i]), axes.map(|i| [27, 38, 49][i])).unwrap();
        let (volume, _) = base
            .replace_box(cut, Voxel::AIR, VolumeLimits::default())
            .unwrap();
        assert_eq!(volume.leaves().len(), 7);
        assert_eq!(volume.solid_units(), VOLUME_UNITS - cut.units());
    }
}

#[test]
fn editing_is_order_independent_and_preserves_readers() {
    let base = RefinedVolume::uniform(Voxel::AIR);
    let boxes = [
        LocalBox::new([1, 2, 3], [2, 3, 4]).unwrap(),
        LocalBox::new([128; 3], [192; 3]).unwrap(),
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
    let (same, work) = a
        .replace_box(
            boxes[0],
            Voxel::new(Material::Brick),
            VolumeLimits::default(),
        )
        .unwrap();
    assert!(!work.changed);
    assert!(Arc::ptr_eq(&same.leaves, &a.leaves));
}

#[test]
fn every_work_cutoff_refuses_without_mutating_retained_source() {
    let base = RefinedVolume::uniform(Voxel::new(Material::Wood));
    let cut = LocalBox::new([17, 31, 59], [18, 32, 60]).unwrap();
    let (expected, work) = base
        .replace_box(cut, Voxel::AIR, VolumeLimits::default())
        .unwrap();
    let bytes = base.encode().unwrap();
    for visits in 1..work.visited {
        assert_eq!(
            base.replace_box(
                cut,
                Voxel::AIR,
                VolumeLimits {
                    leaves: MAX_VOLUME_LEAVES,
                    visits
                }
            )
            .unwrap_err(),
            VolumeError::VisitBudget
        );
        assert_eq!(base.encode().unwrap(), bytes);
    }
    for leaves in 1..7 {
        assert_eq!(
            base.replace_box(
                cut,
                Voxel::AIR,
                VolumeLimits {
                    leaves,
                    visits: MAX_EDIT_VISITS
                }
            )
            .unwrap_err(),
            VolumeError::LeafBudget
        );
    }
    let (exact, _) = base
        .replace_box(
            cut,
            Voxel::AIR,
            VolumeLimits {
                leaves: 7,
                visits: work.visited,
            },
        )
        .unwrap();
    assert_eq!(exact.leaves(), expected.leaves());
    for limits in [
        VolumeLimits {
            leaves: 0,
            visits: 1,
        },
        VolumeLimits {
            leaves: MAX_VOLUME_LEAVES + 1,
            visits: 1,
        },
        VolumeLimits {
            leaves: 1,
            visits: 0,
        },
        VolumeLimits {
            leaves: 1,
            visits: MAX_EDIT_VISITS + 1,
        },
    ] {
        assert_eq!(
            base.replace_box(cut, Voxel::AIR, limits).unwrap_err(),
            VolumeError::InvalidLimits
        );
    }
}

#[test]
fn full_repaint_coalesces_inner_profiles_before_outer_ones_at_final_cap_one() {
    let (source, _) = random_fixture(&IRREGULAR);
    let reader = source.clone();
    let (repainted, work) = source
        .replace_box(
            LocalBox::FULL,
            Voxel::new(Material::Brick),
            VolumeLimits {
                leaves: 1,
                visits: MAX_EDIT_VISITS,
            },
        )
        .unwrap();
    assert_eq!(repainted.uniform_voxel(), Some(Voxel::new(Material::Brick)));
    assert!(work.peak_leaves <= 3); // One leaf in each of page, slab and band buffers.
    assert_eq!(reader.encode().unwrap(), source.encode().unwrap());
    assert_eq!(
        RefinedVolume::decode(&repainted.encode().unwrap())
            .unwrap()
            .leaves(),
        repainted.leaves()
    );
}

fn random(seed: &mut u32) -> u32 {
    *seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
    *seed
}
const fn dense_index(x: usize, y: usize, z: usize) -> usize {
    x + 8 * (y + 8 * z)
}

pub(super) fn random_fixture(grid: &[u16; 9]) -> (RefinedVolume, [Voxel; 512]) {
    let mut seed = 0x72bc_4d31;
    let mut volume = RefinedVolume::uniform(Voxel::AIR);
    let mut dense = [Voxel::AIR; 512];
    for iteration in 0..64 {
        let start: [usize; 3] =
            std::array::from_fn(|_| usize::try_from(random(&mut seed) >> 29).unwrap());
        let end: [usize; 3] = std::array::from_fn(|axis| {
            start[axis] + 1 + usize::try_from(random(&mut seed)).unwrap() % (8 - start[axis])
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
                LocalBox::new(start.map(|i| grid[i]), end.map(|i| grid[i])).unwrap(),
                voxel,
                VolumeLimits::default(),
            )
            .unwrap();
        assert!(work.visited <= MAX_EDIT_VISITS);
        assert!(work.peak_leaves <= 2 * MAX_VOLUME_LEAVES + usize::from(VOLUME_EDGE));
        volume = next;
        for z in start[2]..end[2] {
            for y in start[1]..end[1] {
                for x in start[0]..end[0] {
                    dense[dense_index(x, y, z)] = voxel;
                }
            }
        }
        let mut counts = [0_u32; 8];
        let mut mass = 0_u64;
        for z in 0..8 {
            for y in 0..8 {
                for x in 0..8 {
                    let expected = dense[dense_index(x, y, z)];
                    let lower = [x, y, z].map(|i| grid[i]);
                    let upper = [x, y, z].map(|i| grid[i + 1]);
                    for point in [lower, upper.map(|value| value - 1)] {
                        assert_eq!(volume.leaf_at(point).unwrap().voxel(), expected);
                    }
                    let units = (0..3)
                        .map(|axis| u32::from(upper[axis] - lower[axis]))
                        .product::<u32>();
                    counts[expected.material as usize] += units;
                    mass +=
                        u64::from(units) * u64::from(expected.material.properties().density_kg_m3);
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
fn regular_and_non_dyadic_edits_match_dense_occupancy_integrity_mass_oracle() {
    random_fixture(&REGULAR);
    random_fixture(&IRREGULAR);
}

#[test]
fn finest_points_bounds_and_material_changes_are_exact() {
    for point in [[0; 3], [255; 3], [1, 2, 3], [128, 31, 254]] {
        let bounds = LocalBox::new(point, point.map(|v| v + 1)).unwrap();
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
        assert_eq!(steel.mass_numerator(), 7850);
        assert_eq!(steel.solid_units(), 1);
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

// Independent wire fixture builder; no encoder or canonical normalizer reused.
fn stream(leaves: &[([u16; 3], u8, u8)]) -> Vec<u8> {
    let mut bytes = b"DFVL\x02".to_vec();
    bytes.extend_from_slice(&u32::try_from(leaves.len()).unwrap().to_le_bytes());
    for &(end, material, integrity) in leaves {
        for value in end {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes.extend_from_slice(&[material, integrity]);
    }
    bytes
}

#[test]
fn codec_rejects_bad_hierarchy_reducible_profiles_legacy_version_and_lengths() {
    let full = [256; 3];
    let valid = stream(&[(full, Material::Wood as u8, 255)]);
    for size in 0..valid.len() {
        assert!(RefinedVolume::decode(&valid[..size]).is_err());
    }
    for invalid in [
        stream(&[]),
        stream(&[([0, 256, 256], 3, 255)]),
        stream(&[([257; 3], 3, 255)]),
        stream(&[(full, 255, 0)]),
        stream(&[(full, 0, 1)]),
        stream(&[([128, 256, 256], 3, 255)]), // Incomplete X profile.
        stream(&[([256, 128, 256], 3, 255)]), // Incomplete Y profile.
        stream(&[([256, 256, 128], 3, 255)]), // Incomplete Z profile.
        stream(&[(full, 3, 255), (full, 4, 255)]), // Trailing complete page.
        stream(&[([128, 128, 256], 3, 255), ([256, 256, 256], 4, 255)]), // Changed band end mid-run.
        stream(&[([256, 128, 128], 3, 255), ([256, 256, 256], 4, 255)]), // Changed slab end mid-band.
        stream(&[([128, 256, 256], 3, 255), (full, 3, 255)]),            // Reducible X.
        stream(&[([256, 128, 256], 3, 255), (full, 3, 255)]),            // Reducible Y.
        stream(&[([256, 256, 128], 3, 255), (full, 3, 255)]),            // Reducible Z.
        stream(&vec![(full, 3, 255); MAX_VOLUME_LEAVES + 1]),
    ] {
        assert!(
            RefinedVolume::decode(&invalid).is_err(),
            "accepted {invalid:?}"
        );
    }
    for index in [0, 4, 5, 8] {
        let mut invalid = valid.clone();
        invalid[index] = 255;
        assert!(RefinedVolume::decode(&invalid).is_err());
    }
    for version in [0, 1, 3] {
        let mut legacy = valid.clone();
        legacy[4] = version;
        assert!(RefinedVolume::decode(&legacy).is_err());
    }
    let mut trailing = valid;
    trailing.push(0);
    assert!(RefinedVolume::decode(&trailing).is_err());
}

#[test]
fn every_wire_material_and_zero_integrity_share_edit_decode_semantics() {
    for id in 0_u8..=255 {
        for integrity in [0, 1, 255] {
            let encoded = stream(&[([256; 3], id, integrity)]);
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
            assert_eq!(decoded.fingerprint(), expected.fingerprint());
            assert_eq!(decoded.encode().unwrap(), encoded);
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
            LocalBox::new([17, 31, 59], [18, 32, 60]).unwrap(),
            Voxel::AIR,
            VolumeLimits::default(),
        )
        .unwrap();
    let bytes = volume.encode().unwrap();
    let mut seed = 0x3a7b_915e;
    for _ in 0..4000 {
        let mut altered = bytes.clone();
        for _ in 0..=(random(&mut seed) % 4) {
            let index = usize::try_from(random(&mut seed)).unwrap() % altered.len();
            altered[index] = u8::try_from(random(&mut seed) >> 24).unwrap();
        }
        if let Ok(decoded) = RefinedVolume::decode(&altered) {
            assert_eq!(decoded.encode().unwrap(), altered);
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

pub(super) fn checkerboard(dimensions: [u16; 3], air: bool) -> Vec<u8> {
    let mut records = Vec::new();
    for z in 0..dimensions[2] {
        for y in 0..dimensions[1] {
            for x in 0..dimensions[0] {
                let voxel = if (x + y + z) % 2 == 0 {
                    Voxel::new(Material::Wood)
                } else if air {
                    Voxel::AIR
                } else {
                    Voxel::new(Material::Steel)
                };
                let end = [x, y, z]
                    .into_iter()
                    .zip(dimensions)
                    .map(|(i, n)| (i + 1) * (256 / n))
                    .collect::<Vec<_>>();
                records.push((
                    [end[0], end[1], end[2]],
                    voxel.material as u8,
                    voxel.integrity,
                ));
            }
        }
    }
    stream(&records)
}

#[test]
fn maximum_leaf_material_checkerboard_decodes_and_repaints_under_work_limit() {
    let bytes = checkerboard([32, 16, 16], false);
    let checker = RefinedVolume::decode(&bytes).unwrap();
    assert_eq!(bytes.len(), codec::MAX_VOLUME_BYTES);
    assert_eq!(checker.leaves().len(), MAX_VOLUME_LEAVES);
    assert_eq!(checker.solid_units(), VOLUME_UNITS);
    let (repainted, work) = checker
        .replace_box(
            LocalBox::FULL,
            Voxel::new(Material::Brick),
            VolumeLimits {
                leaves: 1,
                visits: MAX_EDIT_VISITS,
            },
        )
        .unwrap();
    assert!(work.visited <= MAX_EDIT_VISITS);
    assert!(work.peak_leaves <= 3);
    assert_eq!(repainted.uniform_voxel(), Some(Voxel::new(Material::Brick)));
    assert_eq!(checker.encode().unwrap(), bytes);
    let air = RefinedVolume::uniform(Voxel::AIR);
    let surface = checker
        .surface([&air; 6], SurfaceLimits::default())
        .unwrap();
    assert_eq!(
        surface.quads.iter().map(|q| q.area_units()).sum::<u32>(),
        6 * 256 * 256
    );
    assert!(surface.visits <= surface::MAX_SURFACE_VISITS);
}

#[test]
fn surface_has_no_internal_material_sheets_and_requires_exact_budget() {
    let air = RefinedVolume::uniform(Voxel::AIR);
    let solid = RefinedVolume::uniform(Voxel::new(Material::Wood));
    let other = RefinedVolume::uniform(Voxel::new(Material::Brick));
    let surface = solid.surface([&air; 6], SurfaceLimits::default()).unwrap();
    assert_eq!(surface.quads.len(), 6);
    assert_eq!(
        surface.quads.iter().map(|q| q.area_units()).sum::<u32>(),
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
    assert!(
        solid
            .surface(
                [&air; 6],
                SurfaceLimits {
                    quads: 6,
                    visits: surface.visits
                }
            )
            .is_ok()
    );
    assert_eq!(
        solid
            .surface(
                [&air; 6],
                SurfaceLimits {
                    quads: 5,
                    visits: surface.visits
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
                    visits: surface.visits - 1
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
}

fn boundary(quad: &SurfaceQuad, face: Face) -> bool {
    quad.face() == face
        && quad.origin()[face.axis()] == if face.positive() { VOLUME_EDGE } else { 0 }
}

#[test]
fn coarse_solid_against_finest_neighbor_hole_has_exact_six_direction_seams() {
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
        assert_eq!(quad.extent(), [1; 2]);
        for axis in (0..3).filter(|&axis| axis != face.axis()) {
            assert_eq!(quad.origin()[axis], point[axis]);
        }
        let reverse = neighbor
            .surface([&solid; 6], SurfaceLimits::default())
            .unwrap();
        assert!(!reverse.quads.iter().any(|q| boundary(q, face.opposite())));
    }
}

#[test]
fn interval_surface_refusals_preserve_both_pages_and_can_regenerate() {
    let neighbor = RefinedVolume::decode(&checkerboard([16; 3], true)).unwrap();
    let solid = RefinedVolume::uniform(Voxel::new(Material::Brick));
    let old_neighbor = neighbor.encode().unwrap();
    let old_solid = solid.encode().unwrap();
    for face in Face::ALL {
        let mut neighbors = [&solid; 6];
        neighbors[face.index()] = &neighbor;
        let complete = solid.surface(neighbors, SurfaceLimits::default()).unwrap();
        assert_eq!(complete.quads.len(), 128);
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
fn surfaces_match_dense_face_oracle_without_duplicates_on_both_grids() {
    for grid in [&REGULAR, &IRREGULAR] {
        assert_surface_oracle(grid);
    }
}

#[test]
fn solid_overlap_interval_budget_never_hides_unknown_space() {
    let (volume, _) = random_fixture(&IRREGULAR);
    for minimum in [[0; 3], [255; 3], [17, 36, 150], [217, 89, 7]] {
        let bounds = LocalBox::new(minimum, minimum.map(|v| v + 1)).unwrap();
        assert_eq!(
            volume
                .overlaps_solid_bounded(bounds, 3 * MAX_VOLUME_LEAVES)
                .unwrap(),
            volume.overlaps_solid(bounds)
        );
        assert_eq!(
            volume.overlaps_solid_bounded(bounds, 2),
            Err(VolumeError::VisitBudget)
        );
    }
    for visits in [0, 3 * MAX_VOLUME_LEAVES + 1] {
        assert_eq!(
            volume.overlaps_solid_bounded(LocalBox::FULL, visits),
            Err(VolumeError::InvalidLimits)
        );
    }
    let solid = RefinedVolume::uniform(Voxel::new(Material::Wood));
    assert!(solid.overlaps_solid_bounded(LocalBox::FULL, 3).unwrap());
    assert!(
        !RefinedVolume::uniform(Voxel::AIR)
            .overlaps_solid_bounded(LocalBox::FULL, 3)
            .unwrap()
    );
}

#[test]
fn maximum_checkerboard_with_six_maximum_neighbors_respects_exact_surface_capacity() {
    let checker = RefinedVolume::decode(&checkerboard([32, 16, 16], true)).unwrap();
    let surface = checker
        .surface([&checker; 6], SurfaceLimits::default())
        .unwrap();
    assert_eq!(surface.quads.len(), 24_576);
    assert!(surface.visits <= surface::MAX_SURFACE_VISITS);
    let limits = SurfaceLimits {
        quads: surface.quads.len(),
        visits: surface.visits,
    };
    assert_eq!(
        checker.surface([&checker; 6], limits).unwrap().quads,
        surface.quads
    );
    assert_eq!(
        checker
            .surface(
                [&checker; 6],
                SurfaceLimits {
                    quads: limits.quads - 1,
                    ..limits
                }
            )
            .unwrap_err(),
        VolumeError::SurfaceBudget
    );
}

#[test]
fn material_wire_contract_keeps_zero_integrity_solids_and_canonical_air() {
    assert!(
        Voxel {
            material: Material::Wood,
            integrity: 0
        }
        .is_solid()
    );
    assert_eq!(Voxel::from_wire(0, 7).unwrap(), Voxel::AIR);
}
fn assert_surface_oracle(grid: &[u16; 9]) {
    let (volume, dense) = random_fixture(grid);
    let air = RefinedVolume::uniform(Voxel::AIR);
    let surface = volume.surface([&air; 6], SurfaceLimits::default()).unwrap();
    let mut actual = BTreeSet::new();
    for quad in surface.quads {
        let axis = quad.face().axis();
        let u = (axis + 1) % 3;
        let v = (axis + 2) % 3;
        let start = quad
            .origin()
            .map(|coordinate| grid.binary_search(&coordinate).unwrap());
        let end_u = grid
            .binary_search(&(quad.origin()[u] + quad.extent()[0]))
            .unwrap();
        let end_v = grid
            .binary_search(&(quad.origin()[v] + quad.extent()[1]))
            .unwrap();
        for du in start[u]..end_u {
            for dv in start[v]..end_v {
                let mut point = start;
                point[u] = du;
                point[v] = dv;
                assert!(
                    actual.insert((
                        quad.face().index(),
                        point,
                        quad.voxel().material as u8,
                        quad.voxel().integrity
                    )),
                    "duplicate surface rectangle coverage"
                );
            }
        }
    }
    let mut expected = BTreeSet::new();
    for z in 0..8 {
        for y in 0..8 {
            for x in 0..8 {
                let point = [x, y, z];
                let voxel = dense[dense_index(x, y, z)];
                if !voxel.is_solid() {
                    continue;
                }
                for face in Face::ALL {
                    let mut adjacent = point.map(|i| i32::try_from(i).unwrap());
                    adjacent[face.axis()] += if face.positive() { 1 } else { -1 };
                    let empty = adjacent.iter().any(|&i| !(0..8).contains(&i)) || {
                        let p = adjacent.map(|i| usize::try_from(i).unwrap());
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
