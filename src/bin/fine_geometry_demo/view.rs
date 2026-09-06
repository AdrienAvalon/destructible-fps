//! Reproducible native viewpoints; these are inspection cameras, not player collision control.
use super::{Vec3, WorldKind};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum View {
    #[default]
    Fracture,
    Approach,
    Wide,
}

impl View {
    pub const fn distance(self, kind: WorldKind) -> f32 {
        match (kind, self) {
            (WorldKind::Inspection, _) => 5.5,
            (_, Self::Fracture) => 8.5,
            (_, Self::Approach) => 14.0,
            (_, Self::Wide) => 38.0,
        }
    }

    pub fn camera(self, kind: WorldKind, yaw: f32, distance: f32) -> (Vec3, Vec3) {
        let (focus, facing, eye_height) = match (kind, self) {
            (WorldKind::Inspection, _) => (Vec3::new(2.0, 1.4, 0.1), -1.0, 1.9),
            (_, Self::Fracture) => (Vec3::new(-15.0, 2.4, 15.8), 1.0, 3.7),
            // Courtyard surface y=1; the eye is 1.65 m above it, independent of zoom.
            (_, Self::Approach) => (Vec3::new(-15.0, 4.8, 15.8), 1.0, 2.65),
            (_, Self::Wide) => (Vec3::new(-3.0, 8.0, 15.0), 1.0, 2.65),
        };
        let camera = Vec3::new(focus.x, eye_height, focus.z)
            + Vec3::new(yaw.sin() * distance, 0.0, facing * yaw.cos() * distance);
        (camera, (focus - camera).normalize())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_player_height_viewpoints_stand_over_actual_ground_and_outside_solids() {
        use destructible_fps::{
            mesh::fine::fixture::industrial_inspection_world,
            world::query::{PhysicalBox, QueryBudget, QueryLimits, overlaps_solid},
        };
        let world = industrial_inspection_world(0).unwrap();
        for view in [View::Approach, View::Wide] {
            let (eye, _) = view.camera(
                WorldKind::Industrial,
                -0.25,
                view.distance(WorldKind::Industrial),
            );
            #[allow(clippy::cast_possible_truncation)]
            let point = eye
                .to_array()
                .map(|v| (f64::from(v) * 1_000_000.0).round() as i64);
            let bounds = |y| {
                PhysicalBox::from_micrometers(
                    [point[0], y, point[2]],
                    [point[0] + 1, y + 1, point[2] + 1],
                )
                .unwrap()
            };
            let mut budget = QueryBudget::new(QueryLimits::default()).unwrap();
            assert!(overlaps_solid(&world, bounds(999_999), &mut budget).unwrap());
            assert!(!overlaps_solid(&world, bounds(1_000_000), &mut budget).unwrap());
            assert!(!overlaps_solid(&world, bounds(point[1]), &mut budget).unwrap());
        }
    }

    #[test]
    fn player_height_views_keep_eye_height_and_finite_unit_directions_when_zoomed() {
        for view in [View::Approach, View::Wide] {
            for distance in [1.0, view.distance(WorldKind::Industrial), 60.0] {
                for yaw in [-1.0, -0.25, 0.0, 1.0] {
                    let (eye, direction) = view.camera(WorldKind::Industrial, yaw, distance);
                    assert_eq!(eye.y.to_bits(), 2.65_f32.to_bits());
                    assert!(direction.is_finite());
                    assert!((direction.length() - 1.0).abs() < 1.0e-6);
                }
            }
        }
    }
}
