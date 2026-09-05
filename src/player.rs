//! Deterministic-enough local character controller and voxel targeting for the playable slice.

#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss
)]

use crate::{IVec3, World};
use glam::Vec3;

const PLAYER_RADIUS: f32 = 0.30;
const PLAYER_HEIGHT: f32 = 1.80;
const EYE_HEIGHT: f32 = 1.65;
const WALK_SPEED: f32 = 6.5;
const AIR_CONTROL: f32 = 0.42;
const GRAVITY: f32 = 24.0;
const JUMP_SPEED: f32 = 8.25;
const COLLISION_EPSILON: f32 = 0.001;

#[derive(Clone, Copy, Debug, Default)]
pub struct MovementInput {
    pub forward: f32,
    pub right: f32,
    pub jump: bool,
    pub sprint: bool,
}

#[derive(Clone, Copy, Debug)]
pub struct Player {
    /// Bottom-center of the standing capsule approximation.
    pub position: Vec3,
    pub velocity: Vec3,
    pub yaw: f32,
    pub pitch: f32,
    grounded: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RayHit {
    pub voxel: IVec3,
    /// Last empty voxel crossed before impact, when the ray did not start inside geometry.
    pub adjacent_empty: Option<IVec3>,
    pub point: Vec3,
    pub distance: f32,
}

impl Default for Player {
    fn default() -> Self {
        Self {
            position: Vec3::new(0.0, 1.01, 40.0),
            velocity: Vec3::ZERO,
            yaw: 0.0,
            pitch: 0.0,
            grounded: false,
        }
    }
}

impl Player {
    #[must_use]
    pub fn camera_position(self) -> Vec3 {
        self.position + Vec3::Y * EYE_HEIGHT
    }

    #[must_use]
    pub fn view_direction(self) -> Vec3 {
        let horizontal = self.pitch.cos();
        Vec3::new(
            self.yaw.sin() * horizontal,
            self.pitch.sin(),
            -self.yaw.cos() * horizontal,
        )
        .normalize()
    }

    pub fn look(&mut self, mouse_delta_x: f64, mouse_delta_y: f64) {
        const SENSITIVITY: f32 = 0.0018;
        self.yaw = (mouse_delta_x as f32).mul_add(-SENSITIVITY, self.yaw);
        self.pitch = (mouse_delta_y as f32)
            .mul_add(-SENSITIVITY, self.pitch)
            .clamp(-1.553, 1.553);
    }

    pub fn step(&mut self, world: &World, input: MovementInput, delta_seconds: f32) {
        let delta_seconds = delta_seconds.clamp(0.0, 0.05);
        let forward = Vec3::new(self.yaw.sin(), 0.0, -self.yaw.cos());
        let right = Vec3::new(self.yaw.cos(), 0.0, self.yaw.sin());
        let mut wish = forward * input.forward + right * input.right;
        if wish.length_squared() > 1.0 {
            wish = wish.normalize();
        }
        let speed = WALK_SPEED * if input.sprint { 1.55 } else { 1.0 };
        let control = if self.grounded { 1.0 } else { AIR_CONTROL };
        let response = control.min(12.0 * delta_seconds);
        self.velocity.x = wish
            .x
            .mul_add(speed, -self.velocity.x)
            .mul_add(response, self.velocity.x);
        self.velocity.z = wish
            .z
            .mul_add(speed, -self.velocity.z)
            .mul_add(response, self.velocity.z);

        if self.grounded && input.jump {
            self.velocity.y = JUMP_SPEED;
            self.grounded = false;
        }
        self.velocity.y = GRAVITY.mul_add(-delta_seconds, self.velocity.y);
        self.grounded = false;

        let displacement = self.velocity * delta_seconds;
        self.move_axis(world, Vec3::new(displacement.x, 0.0, 0.0));
        self.move_axis(world, Vec3::new(0.0, displacement.y, 0.0));
        self.move_axis(world, Vec3::new(0.0, 0.0, displacement.z));

        if self.position.y < -30.0 {
            *self = Self::default();
        }
    }

    fn move_axis(&mut self, world: &World, displacement: Vec3) {
        if displacement == Vec3::ZERO {
            return;
        }
        let candidate = self.position + displacement;
        if collides(world, candidate) {
            if displacement.y < 0.0 {
                self.grounded = true;
            }
            if displacement.x != 0.0 {
                self.velocity.x = 0.0;
            }
            if displacement.y != 0.0 {
                self.velocity.y = 0.0;
            }
            if displacement.z != 0.0 {
                self.velocity.z = 0.0;
            }
        } else {
            self.position = candidate;
        }
    }
}

#[must_use]
pub fn raycast(world: &World, origin: Vec3, direction: Vec3, max_distance: f32) -> Option<RayHit> {
    let direction = direction.try_normalize()?;
    let mut previous = IVec3::new(i32::MIN, i32::MIN, i32::MIN);
    let steps = (max_distance.max(0.0) / 0.04).ceil() as u32;
    for step in 0..=steps {
        let distance = step as f32 * 0.04;
        let point = origin + direction * distance;
        let voxel = point_to_voxel(point);
        if voxel != previous {
            if world.voxel(voxel).is_solid() {
                return Some(RayHit {
                    voxel,
                    adjacent_empty: (previous != IVec3::new(i32::MIN, i32::MIN, i32::MIN))
                        .then_some(previous),
                    point,
                    distance,
                });
            }
            previous = voxel;
        }
    }
    None
}

fn collides(world: &World, position: Vec3) -> bool {
    let minimum = Vec3::new(
        position.x - PLAYER_RADIUS,
        position.y + COLLISION_EPSILON,
        position.z - PLAYER_RADIUS,
    );
    let maximum = Vec3::new(
        position.x + PLAYER_RADIUS,
        position.y + PLAYER_HEIGHT - COLLISION_EPSILON,
        position.z + PLAYER_RADIUS,
    );
    let minimum = point_to_voxel(minimum);
    let maximum = point_to_voxel(maximum);
    for x in minimum.x..=maximum.x {
        for y in minimum.y..=maximum.y {
            for z in minimum.z..=maximum.z {
                if world.voxel(IVec3::new(x, y, z)).is_solid() {
                    return true;
                }
            }
        }
    }
    false
}

const fn point_to_voxel(point: Vec3) -> IVec3 {
    IVec3::new(
        point.x.floor() as i32,
        point.y.floor() as i32,
        point.z.floor() as i32,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Material, Voxel};

    #[test]
    fn player_lands_on_solid_floor() {
        let mut world = World::default();
        world.fill_box(
            IVec3::new(-3, 0, -3),
            IVec3::new(3, 0, 3),
            Voxel::new(Material::Stone),
        );
        let mut player = Player {
            position: Vec3::new(0.0, 4.0, 0.0),
            ..Player::default()
        };
        for _ in 0..240 {
            player.step(&world, MovementInput::default(), 1.0 / 120.0);
        }
        assert!((player.position.y - 1.0).abs() < 0.15);
    }

    #[test]
    fn raycast_returns_first_solid_voxel() {
        let mut world = World::default();
        world.set_voxel(IVec3::new(0, 2, -5), Voxel::new(Material::Brick));
        let hit = raycast(&world, Vec3::new(0.5, 2.5, 0.0), -Vec3::Z, 20.0)
            .expect("ray should intersect brick");
        assert_eq!(hit.voxel, IVec3::new(0, 2, -5));
        assert_eq!(hit.adjacent_empty, Some(IVec3::new(0, 2, -4)));
        assert!((3.9..=4.1).contains(&hit.distance));
    }
}
