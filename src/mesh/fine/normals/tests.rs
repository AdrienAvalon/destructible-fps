use super::*;
use crate::{
    Voxel,
    volume::{VolumeLimits, surface::SurfaceLimits},
};

fn ramp(slope: [i32; 2]) -> RefinedVolume {
    let mut volume = RefinedVolume::uniform(Voxel::AIR);
    for z in (0..128_u16).step_by(8) {
        for x in (0..128_u16).step_by(8) {
            let height = u16::try_from(
                96 + slope[0] * i32::from(x + 4) / 8 + slope[1] * i32::from(z + 4) / 8,
            )
            .unwrap()
                / 4
                * 4;
            volume = volume
                .replace_box(
                    LocalBox::new([32 + x, 0, 32 + z], [40 + x, height, 40 + z]).unwrap(),
                    Voxel::new(Material::Concrete),
                    VolumeLimits::default(),
                )
                .unwrap()
                .0;
        }
    }
    volume
}

fn surface(volume: &RefinedVolume) -> Vec<SurfaceQuad> {
    let air = RefinedVolume::uniform(Voxel::AIR);
    volume
        .surface([&air; 6], SurfaceLimits::default())
        .unwrap()
        .quads
}

#[test]
fn signed_slopes_produce_unit_outward_normals_and_preserve_footprint_sides() {
    for slope in [[2, 1], [-2, 1], [2, -1], [-2, -1], [0, 2], [2, 0]] {
        let volume = ramp(slope);
        let quads = surface(&volume);
        let meter = WorkMeter::new(super::super::MAX_FINE_MESH_WORK);
        let plane = fit(&volume, &quads, &meter).unwrap().unwrap();
        let expected =
            glam::DVec3::new(-f64::from(slope[0]) / 8.0, 1.0, -f64::from(slope[1]) / 8.0)
                .normalize();
        let normal = glam::DVec3::from_array(plane.normal.map(f64::from));
        assert!(normal.abs_diff_eq(expected, 0.012));
        assert!((normal.length() - 1.0).abs() < 1e-6);
        let mut risers = 0;
        let mut tops = 0;
        for quad in &quads {
            let shaded = plane.for_quad(*quad, &volume, &meter).unwrap();
            if let Some(shaded) = shaded {
                let axis = quad.face().axis();
                let sign = if quad.face().positive() { 1.0 } else { -1.0 };
                assert!(shaded[axis] * sign > 0.001);
                if axis == 1 {
                    tops += 1;
                } else {
                    risers += 1;
                }
            }
            if quad.face() == Face::NegativeY
                || (quad.face().axis() != 1
                    && [32, 160].contains(&quad.origin()[quad.face().axis()]))
            {
                assert!(
                    shaded.is_none(),
                    "outer sides stay hard, including split upper pieces"
                );
            }
        }
        assert!(tops > 0 && risers > 0);
    }
}

#[test]
fn rectangle_moments_are_invariant_to_coplanar_splits() {
    let volume = ramp([2, -1]);
    let mut whole = Moments::default();
    let mut split = Moments::default();
    for quad in surface(&volume) {
        if quad.face() != Face::PositiveY {
            continue;
        }
        whole.add(quad.origin(), quad.extent());
        let [depth, width] = quad.extent();
        for z in 0..depth {
            for x in 0..width {
                let mut origin = quad.origin();
                origin[0] += x;
                origin[2] += z;
                split.add(origin, [1, 1]);
            }
        }
    }
    let first = whole.plane().unwrap();
    let second = split.plane().unwrap();
    for (a, b) in first.slope.into_iter().zip(second.slope) {
        assert!((a - b).abs() < 1e-10);
    }
    assert!((first.intercept - second.intercept).abs() < 1e-8);
}

#[test]
fn unsupported_shapes_fall_back_and_budget_exhaustion_is_an_error() {
    let volume = ramp([2, 1]);
    let before = volume.fingerprint();
    let meter = WorkMeter::new(1);
    assert!(matches!(
        fit(&volume, &surface(&volume), &meter),
        Err(FineMeshError::WorkBudget)
    ));
    assert_eq!(volume.fingerprint(), before);
    let quads = surface(&volume);
    let full = WorkMeter::new(super::super::MAX_FINE_MESH_WORK);
    let plane = fit(&volume, &quads, &full).unwrap().unwrap();
    let riser = quads
        .iter()
        .find(|q| q.face().axis() != 1 && plane.for_quad(**q, &volume, &full).unwrap().is_some())
        .unwrap();
    let exhausted = WorkMeter::new(1);
    assert!(matches!(
        plane.for_quad(*riser, &volume, &exhausted),
        Err(FineMeshError::WorkBudget)
    ));
    assert!(exhausted.check().is_err());
    let change = |bounds, voxel| {
        volume
            .replace_box(bounds, voxel, VolumeLimits::default())
            .unwrap()
            .0
    };
    let unsupported = [
        ramp([0, 0]),
        change(
            LocalBox::new([32, 0, 32], [40, 8, 40]).unwrap(),
            Voxel::new(Material::Brick),
        ),
        change(
            LocalBox::new([48, 16, 48], [64, 24, 64]).unwrap(),
            Voxel::AIR,
        ),
        change(
            LocalBox::new([0, 0, 32], [40, 32, 40]).unwrap(),
            Voxel::new(Material::Concrete),
        ),
        change(
            LocalBox::new([48, 0, 48], [64, 220, 64]).unwrap(),
            Voxel::new(Material::Concrete),
        ),
    ];
    for candidate in unsupported {
        let meter = WorkMeter::new(super::super::MAX_FINE_MESH_WORK);
        assert!(
            fit(&candidate, &surface(&candidate), &meter)
                .unwrap()
                .is_none()
        );
    }
}

#[test]
fn all_authored_shards_are_recognized_but_layered_wall_pages_are_not() {
    let world = super::super::fixture::industrial_inspection_world(3).unwrap();
    let mut accepted = 0;
    let mut added_work = 0;
    for position in world.refined_positions() {
        let cell = world.cell(position);
        let volume = cell.volume().unwrap();
        let quads = surface(volume);
        let meter = WorkMeter::new(super::super::MAX_FINE_MESH_WORK);
        let plane = fit(volume, &quads, &meter).unwrap();
        assert_eq!(plane.is_some(), position.y == 1 && position.z > 15);
        if let Some(plane) = plane {
            accepted += 1;
            for quad in quads {
                plane.for_quad(quad, volume, &meter).unwrap();
            }
        }
        added_work += meter.used.get();
    }
    assert_eq!(accepted, 20);
    assert!(
        added_work < 150_000,
        "keep margin in the unchanged 4M job budget"
    );
    println!("TERRACE_NORMALS accepted={accepted} added_work={added_work}");
}

#[test]
fn dense_unit_terraces_keep_strip_queries_bounded() {
    let mut volume = RefinedVolume::uniform(Voxel::AIR);
    for z in 0..32_u16 {
        for x in 0..64_u16 {
            let height = 96 + x / 8 + (x + z) % 2;
            volume = volume
                .replace_box(
                    LocalBox::new([32 + x, 0, 32 + z], [33 + x, height, 33 + z]).unwrap(),
                    Voxel::new(Material::Concrete),
                    VolumeLimits::default(),
                )
                .unwrap()
                .0;
        }
    }
    let quads = surface(&volume);
    let meter = WorkMeter::new(super::super::MAX_FINE_MESH_WORK);
    let plane = fit(&volume, &quads, &meter).unwrap().unwrap();
    let mut shaded = 0;
    for quad in &quads {
        shaded += usize::from(plane.for_quad(*quad, &volume, &meter).unwrap().is_some());
    }
    assert!(quads.len() > 4000 && shaded > 2000);
    assert!(meter.used.get() < volume.leaves().len() + 20 * quads.len());
    println!(
        "DENSE_TERRACE leaves={} quads={} shaded={shaded} work={}",
        volume.leaves().len(),
        quads.len(),
        meter.used.get()
    );
}
