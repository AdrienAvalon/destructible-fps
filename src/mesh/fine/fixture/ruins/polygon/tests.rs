use super::*;
use crate::{IVec3, world::geometry::RefinedWorld};
use std::collections::{BTreeSet, VecDeque};

#[test]
fn outlines_are_convex_asymmetric_and_exact_quarter_turns_stay_in_bounds() {
    for outline in OUTLINES {
        for i in 0..outline.len() {
            let a = outline[i];
            let b = outline[(i + 1) % outline.len()];
            assert!(outline.iter().all(|&p| cross(a, b, p) >= 0));
            assert!(cross(a, b, outline[(i + 2) % outline.len()]) > 0);
            assert!(a[0] != b[0] || a[1] != b[1]);
            for q in 0..4 {
                let p = rotate(a, q);
                assert!(p.into_iter().all(|c| (16..=240).contains(&c)));
                assert_eq!(rotate(p, (4 - q) % 4), a);
            }
        }
        assert!(
            outline
                .windows(2)
                .filter(|p| p[0][0] != p[1][0] && p[0][1] != p[1][1])
                .count()
                >= 3
        );
    }
}

#[test]
fn every_sample_is_exact_connected_grounded_and_quantized() {
    for (index, shard) in super::super::bay_shards().into_iter().enumerate() {
        let v = volume(shard, index).unwrap();
        let mut filled = BTreeSet::new();
        let mut heights = BTreeSet::new();
        let mut units = 0;
        for z in (0_u16..256).step_by(16) {
            for x in (0_u16..256).step_by(16) {
                let expected = if (16..240).contains(&x) && (16..240).contains(&z) {
                    height(shard, index, [i32::from(x + 8), i32::from(z + 8)]).unwrap()
                } else {
                    None
                };
                // Query all vertical lattice units, not only an aggregate fingerprint.
                for y in 0..256 {
                    assert_eq!(
                        v.leaf_at([x + 8, y, z + 8]).unwrap().voxel().is_solid(),
                        expected.is_some_and(|h| y < h)
                    );
                }
                if let Some(h) = expected {
                    assert_eq!(h % 4, 0);
                    heights.insert(h);
                    filled.insert((i32::from(x), i32::from(z)));
                    units += u32::from(h) * 16 * 16;
                }
            }
        }
        assert!(heights.len() >= 4);
        assert_eq!(v.solid_units(), units);
        assert_eq!(v.fingerprint(), volume(shard, index).unwrap().fingerprint());
        let root = *filled.first().unwrap();
        let mut reached = BTreeSet::from([root]);
        let mut queue = VecDeque::from([root]);
        while let Some((x, z)) = queue.pop_front() {
            for p in [(x - 16, z), (x + 16, z), (x, z - 16), (x, z + 16)] {
                if filled.contains(&p) && reached.insert(p) {
                    queue.push_back(p);
                }
            }
        }
        assert_eq!(
            reached, filled,
            "positive-area face connectivity, not diagonal contact"
        );
        assert!(filled.len() < 160 && filled.len() > 60);
        println!(
            "POLYGON index={index} columns={} leaves={} units={units}",
            filled.len(),
            v.leaves().len()
        );
    }
    assert!(volume(super::super::bay_shards()[0], 12).is_err());
}

#[test]
fn placement_preserves_every_other_cell_and_frozen_small_shards() {
    let coarse = crate::WorldPreset::Industrial.build();
    let original = super::super::courtyard(&RefinedWorld::from_uniform(&coarse).unwrap()).unwrap();
    let before = original.fingerprint();
    let world = super::super::bay_debris(&original).unwrap();
    let positions: BTreeSet<_> = super::super::bay_shards().iter().map(|s| s.cell).collect();
    assert_eq!(positions.len(), 12);
    assert_eq!(original.fingerprint(), before);
    assert_eq!(
        world.fingerprint(),
        super::super::bay_debris(&original).unwrap().fingerprint()
    );
    assert!(super::super::bay_debris(&world).is_err());
    for (p, cell) in world.occupied_cells() {
        if !positions.contains(&p) {
            assert_eq!(cell, original.cell(p));
        }
    }
    for p in positions {
        assert_eq!(original.cell(p).solid_units(), 0);
        assert_eq!(
            world.cell(IVec3::new(p.x, 0, p.z)).solid_units(),
            crate::volume::VOLUME_UNITS
        );
    }
    for stage in 0..4 {
        let scene = crate::mesh::fine::fixture::industrial_inspection_world(stage).unwrap();
        println!(
            "POLYGON_WORLD stage={stage} fingerprint={:032x} stats={:?}",
            scene.fingerprint(),
            scene.geometry_stats()
        );
    }
}

#[test]
fn rotated_samples_and_original_face_planes_match_the_actual_surface() {
    use crate::{mesh::fine::finishes::FinishPolicy, volume::surface::SurfaceLimits};
    let air = RefinedVolume::uniform(Voxel::AIR);
    for index in 0..12 {
        let shard = super::super::bay_shards()[index];
        for z in (24..240).step_by(16) {
            for x in (24..240).step_by(16) {
                assert_eq!(
                    height(shard, index / 4 * 4, [x, z]).unwrap(),
                    height(shard, index, rotate([x, z], index % 4)).unwrap()
                );
            }
        }
        let FinishPolicy::CutTopAndSidesAtPlane(face, plane) = finish(index) else {
            panic!("plane policy")
        };
        let v = volume(shard, index).unwrap();
        let quads = v
            .surface([&air; 6], SurfaceLimits::default())
            .unwrap()
            .quads;
        let original_area: u32 = quads
            .iter()
            .filter(|q| q.face() == face && q.origin()[face.axis()] == plane)
            .map(|q| q.area_units())
            .sum();
        let other_area: u32 = quads
            .iter()
            .filter(|q| q.face() == face && q.origin()[face.axis()] != plane)
            .map(|q| q.area_units())
            .sum();
        assert!(
            original_area > 1024 && other_area > 0,
            "real original face AND same-facing broken steps"
        );
    }
}

#[test]
fn scene_material_rays_hit_the_stored_tops_and_leave_empty_corners_clear() {
    use crate::{
        FixedMicrometers3,
        ballistics::FixedRay,
        world::query::ray::{TraceLimits, trace_materials},
    };
    let scene = crate::mesh::fine::fixture::industrial_inspection_world(3).unwrap();
    let mut clear = 0;
    let mut hit = 0;
    for (index, shard) in super::super::bay_shards().into_iter().enumerate() {
        for z in (24..240).step_by(32) {
            for x in (24..240).step_by(32) {
                let ray = FixedRay::new(
                    FixedMicrometers3 {
                        x: i64::from(shard.cell.x) * 1_000_000 + i64::from(x) * 1_000_000 / 256,
                        y: 2_000_000,
                        z: i64::from(shard.cell.z) * 1_000_000 + i64::from(z) * 1_000_000 / 256,
                    },
                    [0, -1, 0],
                    1_000_000,
                )
                .unwrap();
                let trace = trace_materials(&scene, &ray, TraceLimits::default()).unwrap();
                if let Some(h) = height(shard, index, [x, z]).unwrap() {
                    assert!(!trace.chords.is_empty());
                    assert!(trace.chords.iter().all(|c| c.cell == shard.cell
                        && c.material.leaf.voxel() == Voxel::new(shard.material)));
                    assert!(
                        trace
                            .chords
                            .windows(2)
                            .all(|p| p[0].material.exit == p[1].material.entry)
                    );
                    let chord = trace.chords[0].material;
                    assert_eq!(
                        chord.entry.numerator() * 256,
                        chord.entry.denominator() * i64::from(256 - h)
                    );
                    let exit = trace.chords.last().unwrap().material.exit;
                    assert_eq!(exit.numerator(), exit.denominator());
                    hit += 1;
                } else {
                    assert!(trace.chords.is_empty());
                    clear += 1;
                }
            }
        }
    }
    assert!(clear > 100 && hit > 100);
}
