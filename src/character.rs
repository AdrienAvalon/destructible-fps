//! Fixed-step server-authoritative player movement and construction context.

use crate::{
    FixedMicrometers3, MICROMETERS_PER_VOXEL, SERVER_PHYSICS_HZ,
    world::query::{
        GeometryQueryError, PhysicalBox, QueryBudget, QueryLimits, QueryStats, StaticGeometry,
        overlaps_solid, sweep_axis,
    },
};
#[cfg(test)]
use crate::{IVec3, World};
use core::fmt;

pub const PLAYER_RADIUS_UM: i64 = 300_000;
pub const PLAYER_HEIGHT_UM: i64 = 1_800_000;
pub const PLAYER_EYE_HEIGHT_UM: i64 = 1_650_000;
pub const MAX_PLAYER_INPUT_PER_MILLE: i16 = 1_000;
pub const MAX_PLAYER_INPUT_HOLD_TICKS: u8 = 15;
const DEFAULT_PLAYER_Y_UM: i64 = MICROMETERS_PER_VOXEL;
const DEFAULT_PLAYER_Z_UM: i64 = 40 * MICROMETERS_PER_VOXEL;
const WALK_SPEED_UM_PER_SECOND: i64 = 6_500_000;
const SPRINT_SPEED_UM_PER_SECOND: i64 = 10_075_000;
const JUMP_SPEED_UM_PER_SECOND: i64 = 8_250_000;
const GRAVITY_UM_PER_SECOND_PER_TICK: i64 = 400_000;
const MAX_FALL_SPEED_UM_PER_SECOND: i64 = -50_000_000;
const VELOCITY_RESPONSE_DIVISOR: i64 = 5;
const FALL_RESET_Y_UM: i64 = -30 * MICROMETERS_PER_VOXEL;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PlayerInputCommand {
    pub input_sequence: u64,
    /// Desired world-space X movement in thousandths of full input.
    pub movement_x_per_mille: i16,
    /// Desired world-space Z movement in thousandths of full input.
    pub movement_z_per_mille: i16,
    pub jump: bool,
    pub sprint: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PlayerBuildContext {
    pub eye_position_um: FixedMicrometers3,
    pub bounds_minimum_um: FixedMicrometers3,
    pub bounds_maximum_um: FixedMicrometers3,
}

impl PlayerBuildContext {
    #[must_use]
    pub fn is_canonical(self) -> bool {
        self.bounds_minimum_um.x.checked_add(PLAYER_RADIUS_UM) == Some(self.eye_position_um.x)
            && self.bounds_maximum_um.x.checked_sub(PLAYER_RADIUS_UM)
                == Some(self.eye_position_um.x)
            && self.bounds_minimum_um.z.checked_add(PLAYER_RADIUS_UM)
                == Some(self.eye_position_um.z)
            && self.bounds_maximum_um.z.checked_sub(PLAYER_RADIUS_UM)
                == Some(self.eye_position_um.z)
            && self.bounds_minimum_um.y.checked_add(PLAYER_EYE_HEIGHT_UM)
                == Some(self.eye_position_um.y)
            && self.bounds_minimum_um.y.checked_add(PLAYER_HEIGHT_UM)
                == Some(self.bounds_maximum_um.y)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AuthoritativePlayerState {
    pub position_um: FixedMicrometers3,
    pub velocity_um_per_second: FixedMicrometers3,
    pub integration_remainder: [i64; 3],
    pub grounded: bool,
    pub last_input_sequence: u64,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PlayerStepReport {
    pub moved: bool,
    pub collided: bool,
    pub input_expired: bool,
    pub geometry: QueryStats,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PlayerInputError {
    ReplayedInput { received: u64, last_accepted: u64 },
    AxisOutOfRange { axis: &'static str, value: i16 },
    MovementMagnitudeTooLarge { x: i16, z: i16 },
}

impl fmt::Display for PlayerInputError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ReplayedInput {
                received,
                last_accepted,
            } => write!(
                formatter,
                "player input {received} is not newer than {last_accepted}"
            ),
            Self::AxisOutOfRange { axis, value } => write!(
                formatter,
                "player {axis} input {value} exceeds {MAX_PLAYER_INPUT_PER_MILLE} per mille"
            ),
            Self::MovementMagnitudeTooLarge { x, z } => {
                write!(
                    formatter,
                    "player movement vector ({x}, {z}) exceeds unit length"
                )
            }
        }
    }
}

impl std::error::Error for PlayerInputError {}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AuthoritativePlayer {
    state: AuthoritativePlayerState,
    held_input: PlayerInputCommand,
    input_age_ticks: u8,
    spawn_position_um: FixedMicrometers3,
}

impl Default for AuthoritativePlayer {
    fn default() -> Self {
        Self::new(FixedMicrometers3 {
            x: 0,
            y: DEFAULT_PLAYER_Y_UM,
            z: DEFAULT_PLAYER_Z_UM,
        })
    }
}

impl AuthoritativePlayer {
    #[must_use]
    pub const fn new(spawn_position_um: FixedMicrometers3) -> Self {
        Self {
            state: AuthoritativePlayerState {
                position_um: spawn_position_um,
                velocity_um_per_second: FixedMicrometers3 { x: 0, y: 0, z: 0 },
                integration_remainder: [0; 3],
                grounded: false,
                last_input_sequence: 0,
            },
            held_input: PlayerInputCommand {
                input_sequence: 0,
                movement_x_per_mille: 0,
                movement_z_per_mille: 0,
                jump: false,
                sprint: false,
            },
            input_age_ticks: MAX_PLAYER_INPUT_HOLD_TICKS,
            spawn_position_um,
        }
    }

    #[must_use]
    pub const fn state(&self) -> AuthoritativePlayerState {
        self.state
    }

    pub(crate) fn restore_authoritative_state(&mut self, state: AuthoritativePlayerState) {
        self.state = state;
        self.held_input = PlayerInputCommand {
            input_sequence: state.last_input_sequence,
            ..PlayerInputCommand::default()
        };
        self.input_age_ticks = MAX_PLAYER_INPUT_HOLD_TICKS;
    }

    #[must_use]
    pub const fn build_context(&self) -> PlayerBuildContext {
        let position = self.state.position_um;
        PlayerBuildContext {
            eye_position_um: FixedMicrometers3 {
                x: position.x,
                y: position.y.saturating_add(PLAYER_EYE_HEIGHT_UM),
                z: position.z,
            },
            bounds_minimum_um: FixedMicrometers3 {
                x: position.x.saturating_sub(PLAYER_RADIUS_UM),
                y: position.y,
                z: position.z.saturating_sub(PLAYER_RADIUS_UM),
            },
            bounds_maximum_um: FixedMicrometers3 {
                x: position.x.saturating_add(PLAYER_RADIUS_UM),
                y: position.y.saturating_add(PLAYER_HEIGHT_UM),
                z: position.z.saturating_add(PLAYER_RADIUS_UM),
            },
        }
    }

    /// Proves clearance using full character bounds, without treating a failed query as air.
    /// # Errors
    /// Reports invalid physical bounds or exhausted query work.
    pub fn is_clear_of_static_world(
        &self,
        world: &impl StaticGeometry,
    ) -> Result<bool, GeometryQueryError> {
        let mut budget = QueryBudget::new(QueryLimits::default())?;
        Ok(!overlaps_solid(
            world,
            player_bounds(self.state.position_um)?,
            &mut budget,
        )?)
    }

    /// Retains only the newest bounded input. Simulation remains fixed to one step per server tick.
    ///
    /// # Errors
    ///
    /// Rejects replayed sequences, out-of-range axes, and diagonal vectors beyond unit length.
    pub fn accept_input(&mut self, input: PlayerInputCommand) -> Result<(), PlayerInputError> {
        if input.input_sequence <= self.state.last_input_sequence {
            return Err(PlayerInputError::ReplayedInput {
                received: input.input_sequence,
                last_accepted: self.state.last_input_sequence,
            });
        }
        for (axis, value) in [
            ("X", input.movement_x_per_mille),
            ("Z", input.movement_z_per_mille),
        ] {
            if value.unsigned_abs() > MAX_PLAYER_INPUT_PER_MILLE.cast_unsigned() {
                return Err(PlayerInputError::AxisOutOfRange { axis, value });
            }
        }
        let x = i64::from(input.movement_x_per_mille);
        let z = i64::from(input.movement_z_per_mille);
        if x.saturating_mul(x).saturating_add(z.saturating_mul(z))
            > i64::from(MAX_PLAYER_INPUT_PER_MILLE).pow(2)
        {
            return Err(PlayerInputError::MovementMagnitudeTooLarge {
                x: input.movement_x_per_mille,
                z: input.movement_z_per_mille,
            });
        }
        self.state.last_input_sequence = input.input_sequence;
        self.held_input = input;
        self.input_age_ticks = 0;
        Ok(())
    }

    /// Exactly one bounded 60-Hz step through shared static geometry.
    /// # Errors
    /// Invalid bounds, initial penetration and query exhaustion leave the entire player intact.
    pub fn step(
        &mut self,
        world: &impl StaticGeometry,
    ) -> Result<PlayerStepReport, GeometryQueryError> {
        self.step_with_limits(world, QueryLimits::default())
    }

    /// # Errors
    /// As with step, also rejecting invalid caller budgets. Failure is atomic including held input.
    pub fn step_with_limits(
        &mut self,
        world: &impl StaticGeometry,
        limits: QueryLimits,
    ) -> Result<PlayerStepReport, GeometryQueryError> {
        let mut budget = QueryBudget::new(limits)?;
        let mut candidate = *self;
        let mut report = candidate.step_candidate(world, &mut budget)?;
        report.geometry = budget.stats();
        *self = candidate;
        Ok(report)
    }

    fn step_candidate(
        &mut self,
        world: &impl StaticGeometry,
        budget: &mut QueryBudget,
    ) -> Result<PlayerStepReport, GeometryQueryError> {
        let mut report = PlayerStepReport::default();
        if overlaps_solid(world, player_bounds(self.state.position_um)?, budget)? {
            return Err(GeometryQueryError::InitialOverlap);
        }
        let input = if self.input_age_ticks < MAX_PLAYER_INPUT_HOLD_TICKS {
            self.held_input
        } else {
            report.input_expired = self.held_input.input_sequence != 0
                && self.input_age_ticks == MAX_PLAYER_INPUT_HOLD_TICKS;
            PlayerInputCommand {
                input_sequence: self.state.last_input_sequence,
                ..PlayerInputCommand::default()
            }
        };
        self.input_age_ticks = self.input_age_ticks.saturating_add(1);
        self.held_input.jump = false;

        let speed = if input.sprint {
            SPRINT_SPEED_UM_PER_SECOND
        } else {
            WALK_SPEED_UM_PER_SECOND
        };
        let desired_x = i64::from(input.movement_x_per_mille).saturating_mul(speed)
            / i64::from(MAX_PLAYER_INPUT_PER_MILLE);
        let desired_z = i64::from(input.movement_z_per_mille).saturating_mul(speed)
            / i64::from(MAX_PLAYER_INPUT_PER_MILLE);
        self.state.velocity_um_per_second.x =
            approach_velocity(self.state.velocity_um_per_second.x, desired_x);
        self.state.velocity_um_per_second.z =
            approach_velocity(self.state.velocity_um_per_second.z, desired_z);
        self.state.grounded =
            sweep_axis(world, player_bounds(self.state.position_um)?, 1, -1, budget)?.contact;
        if self.state.grounded && input.jump {
            self.state.velocity_um_per_second.y = JUMP_SPEED_UM_PER_SECOND;
            self.state.grounded = false;
        }
        self.state.velocity_um_per_second.y = self
            .state
            .velocity_um_per_second
            .y
            .saturating_sub(GRAVITY_UM_PER_SECOND_PER_TICK)
            .max(MAX_FALL_SPEED_UM_PER_SECOND);
        self.state.grounded = false;

        for axis in 0..3 {
            let velocity = component(self.state.velocity_um_per_second, axis);
            let numerator = velocity.saturating_add(self.state.integration_remainder[axis]);
            let displacement = numerator / SERVER_PHYSICS_HZ;
            self.state.integration_remainder[axis] = numerator % SERVER_PHYSICS_HZ;
            if displacement == 0 {
                continue;
            }
            let sweep = sweep_axis(
                world,
                player_bounds(self.state.position_um)?,
                axis,
                displacement,
                budget,
            )?;
            if sweep.displacement_um != 0 {
                let value = component(self.state.position_um, axis)
                    .checked_add(sweep.displacement_um)
                    .ok_or(GeometryQueryError::WorldBounds)?;
                set_component(&mut self.state.position_um, axis, value);
                report.moved = true;
            }
            if sweep.contact {
                report.collided = true;
                if axis == 1 && displacement < 0 {
                    self.state.grounded = true;
                }
                set_component(&mut self.state.velocity_um_per_second, axis, 0);
                self.state.integration_remainder[axis] = 0;
            }
        }

        if self.state.position_um.y < FALL_RESET_Y_UM {
            if overlaps_solid(world, player_bounds(self.spawn_position_um)?, budget)? {
                return Err(GeometryQueryError::UnsafeSpawn);
            }
            let last_input_sequence = self.state.last_input_sequence;
            self.state = AuthoritativePlayerState {
                position_um: self.spawn_position_um,
                velocity_um_per_second: FixedMicrometers3::default(),
                integration_remainder: [0; 3],
                grounded: false,
                last_input_sequence,
            };
            self.held_input = PlayerInputCommand {
                input_sequence: last_input_sequence,
                ..PlayerInputCommand::default()
            };
            self.input_age_ticks = MAX_PLAYER_INPUT_HOLD_TICKS;
        }
        Ok(report)
    }
}

const fn approach_velocity(current: i64, target: i64) -> i64 {
    let difference = target.saturating_sub(current);
    let step = difference / VELOCITY_RESPONSE_DIVISOR;
    if step == 0 {
        target
    } else {
        current.saturating_add(step)
    }
}

fn player_bounds(position: FixedMicrometers3) -> Result<PhysicalBox, GeometryQueryError> {
    let minimum = [
        position.x.checked_sub(PLAYER_RADIUS_UM),
        Some(position.y),
        position.z.checked_sub(PLAYER_RADIUS_UM),
    ];
    let maximum = [
        position.x.checked_add(PLAYER_RADIUS_UM),
        position.y.checked_add(PLAYER_HEIGHT_UM),
        position.z.checked_add(PLAYER_RADIUS_UM),
    ];
    PhysicalBox::from_micrometers(
        [
            minimum[0].ok_or(GeometryQueryError::WorldBounds)?,
            minimum[1].ok_or(GeometryQueryError::WorldBounds)?,
            minimum[2].ok_or(GeometryQueryError::WorldBounds)?,
        ],
        [
            maximum[0].ok_or(GeometryQueryError::WorldBounds)?,
            maximum[1].ok_or(GeometryQueryError::WorldBounds)?,
            maximum[2].ok_or(GeometryQueryError::WorldBounds)?,
        ],
    )
}

const fn component(vector: FixedMicrometers3, axis: usize) -> i64 {
    match axis {
        0 => vector.x,
        1 => vector.y,
        _ => vector.z,
    }
}

const fn set_component(vector: &mut FixedMicrometers3, axis: usize, value: i64) {
    match axis {
        0 => vector.x = value,
        1 => vector.y = value,
        _ => vector.z = value,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Material, Voxel};

    fn floor_world() -> World {
        let mut world = World::default();
        world.fill_box(
            IVec3::new(-20, 0, 20),
            IVec3::new(20, 0, 60),
            Voxel::new(Material::Stone),
        );
        world
    }

    #[test]
    fn input_validation_is_atomic_and_monotonic() {
        let mut player = AuthoritativePlayer::default();
        let valid = PlayerInputCommand {
            input_sequence: 1,
            movement_x_per_mille: 600,
            movement_z_per_mille: -800,
            jump: true,
            sprint: false,
        };
        player.accept_input(valid).expect("unit vector");
        assert_eq!(player.state().last_input_sequence, 1);
        assert!(matches!(
            player.accept_input(valid),
            Err(PlayerInputError::ReplayedInput { .. })
        ));
        assert!(matches!(
            player.accept_input(PlayerInputCommand {
                input_sequence: 2,
                movement_x_per_mille: 1_001,
                ..PlayerInputCommand::default()
            }),
            Err(PlayerInputError::AxisOutOfRange { .. })
        ));
        assert!(matches!(
            player.accept_input(PlayerInputCommand {
                input_sequence: 2,
                movement_x_per_mille: 800,
                movement_z_per_mille: 800,
                ..PlayerInputCommand::default()
            }),
            Err(PlayerInputError::MovementMagnitudeTooLarge { .. })
        ));
        assert_eq!(player.state().last_input_sequence, 1);
    }

    #[test]
    fn one_input_never_advances_more_than_one_fixed_step_per_tick() {
        let world = floor_world();
        let mut once = AuthoritativePlayer::default();
        let mut flooded = AuthoritativePlayer::default();
        once.accept_input(PlayerInputCommand {
            input_sequence: 1,
            movement_x_per_mille: 1_000,
            ..PlayerInputCommand::default()
        })
        .expect("first input");
        for sequence in 1..=64 {
            flooded
                .accept_input(PlayerInputCommand {
                    input_sequence: sequence,
                    movement_x_per_mille: 1_000,
                    ..PlayerInputCommand::default()
                })
                .expect("newest input");
        }

        let _ = once.step(&world).unwrap();
        let _ = flooded.step(&world).unwrap();

        assert_eq!(once.state().position_um, flooded.state().position_um);
        assert_eq!(
            once.state().velocity_um_per_second,
            flooded.state().velocity_um_per_second
        );
    }

    #[test]
    fn movement_lands_jumps_and_expires_stale_input_deterministically() {
        let world = floor_world();
        let mut first = AuthoritativePlayer::default();
        let mut second = AuthoritativePlayer::default();
        let input = PlayerInputCommand {
            input_sequence: 1,
            movement_x_per_mille: 1_000,
            jump: true,
            sprint: true,
            ..PlayerInputCommand::default()
        };
        first.accept_input(input).expect("input");
        second.accept_input(input).expect("input");
        let mut expired = false;
        for _ in 0..120 {
            expired |= first.step(&world).unwrap().input_expired;
            let _ = second.step(&world).unwrap();
            assert_eq!(first, second);
        }

        assert!(expired);
        assert!(first.state().grounded);
        assert_eq!(first.state().velocity_um_per_second.x, 0);
        assert_eq!(first.state().velocity_um_per_second.z, 0);
        assert!(first.state().position_um.x > 0);
    }

    #[test]
    fn build_context_tracks_fixed_player_bounds_and_eye() {
        let player = AuthoritativePlayer::default();
        let context = player.build_context();
        assert_eq!(
            context.eye_position_um.y,
            DEFAULT_PLAYER_Y_UM + PLAYER_EYE_HEIGHT_UM
        );
        assert_eq!(
            context.bounds_maximum_um.y - context.bounds_minimum_um.y,
            PLAYER_HEIGHT_UM
        );
        assert!(context.is_canonical());
    }

    #[test]
    fn terminal_fall_speed_cannot_tunnel_through_a_one_voxel_floor() {
        let world = floor_world();
        let mut player = AuthoritativePlayer::new(FixedMicrometers3 {
            x: 0,
            y: 10 * MICROMETERS_PER_VOXEL,
            z: 40 * MICROMETERS_PER_VOXEL,
        });
        player.state.velocity_um_per_second.y = MAX_FALL_SPEED_UM_PER_SECOND;
        for _ in 0..30 {
            let _ = player.step(&world).unwrap();
        }

        assert!(player.state().grounded);
        assert!(player.state().position_um.y >= MICROMETERS_PER_VOXEL);
        assert!(player.state().position_um.y < 2 * MICROMETERS_PER_VOXEL);
    }
}

#[cfg(test)]
mod fine_tests;
