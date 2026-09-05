//! Bounded local character prediction and authoritative reconciliation.

use crate::{
    AuthoritativePlayer, AuthoritativePlayerState, FixedMicrometers3, PlayerInputCommand,
    PlayerInputError, PlayerStateCodecError, PlayerStepReport, ReplicatedPlayerState, World,
};
use core::fmt;
use std::collections::VecDeque;

pub const MAX_PENDING_PREDICTED_INPUTS: usize = 128;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ClientPredictionStep {
    pub input_sequence: u64,
    pub state: AuthoritativePlayerState,
    pub simulation: PlayerStepReport,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PredictionReconcileReport {
    pub server_tick: u64,
    pub acknowledged_inputs: usize,
    pub replayed_inputs: usize,
    pub correction_um: FixedMicrometers3,
    pub state: AuthoritativePlayerState,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ClientPredictionError {
    Input(PlayerInputError),
    State(PlayerStateCodecError),
    InvalidServerTick,
    InputSequenceExhausted,
    NonSequentialInput { expected: u64, received: u64 },
    PendingInputLimit,
    WrongSession { expected: u64, received: u64 },
    StaleServerTick { received: u64, last_applied: u64 },
    StaleAcknowledgement { received: u64, last_applied: u64 },
    UnknownAcknowledgement { received: u64, last_produced: u64 },
}

impl fmt::Display for ClientPredictionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Input(error) => error.fmt(formatter),
            Self::State(error) => error.fmt(formatter),
            Self::InvalidServerTick => write!(formatter, "prediction server tick must be non-zero"),
            Self::InputSequenceExhausted => write!(formatter, "client input sequence exhausted"),
            Self::NonSequentialInput { expected, received } => write!(
                formatter,
                "client prediction expected input {expected}, received {received}"
            ),
            Self::PendingInputLimit => write!(
                formatter,
                "client prediction reached {MAX_PENDING_PREDICTED_INPUTS} pending inputs"
            ),
            Self::WrongSession { expected, received } => write!(
                formatter,
                "player-state session {received} does not match local session {expected}"
            ),
            Self::StaleServerTick {
                received,
                last_applied,
            } => write!(
                formatter,
                "prediction server tick {received} is not newer than {last_applied}"
            ),
            Self::StaleAcknowledgement {
                received,
                last_applied,
            } => write!(
                formatter,
                "prediction acknowledgement {received} is older than {last_applied}"
            ),
            Self::UnknownAcknowledgement {
                received,
                last_produced,
            } => write!(
                formatter,
                "prediction acknowledgement {received} exceeds produced input {last_produced}"
            ),
        }
    }
}

impl std::error::Error for ClientPredictionError {}

impl From<PlayerInputError> for ClientPredictionError {
    fn from(value: PlayerInputError) -> Self {
        Self::Input(value)
    }
}

impl From<PlayerStateCodecError> for ClientPredictionError {
    fn from(value: PlayerStateCodecError) -> Self {
        Self::State(value)
    }
}

/// Local controlled-player state with a fixed two-second unacknowledged-input ceiling at 60 Hz.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ClientPrediction {
    session_id: u64,
    player: AuthoritativePlayer,
    pending_inputs: VecDeque<PlayerInputCommand>,
    last_server_tick: u64,
    last_acknowledged_input: u64,
    last_produced_input: u64,
}

impl ClientPrediction {
    /// Creates local prediction from one validated authoritative sample.
    ///
    /// # Errors
    ///
    /// Rejects a zero server tick or an invalid externally constructed player state.
    pub fn new(
        server_tick: u64,
        authoritative: ReplicatedPlayerState,
    ) -> Result<Self, ClientPredictionError> {
        if server_tick == 0 {
            return Err(ClientPredictionError::InvalidServerTick);
        }
        authoritative.validate()?;
        let mut player = AuthoritativePlayer::new(authoritative.position_um);
        player.restore_authoritative_state(authoritative_state(authoritative));
        Ok(Self {
            session_id: authoritative.session_id,
            player,
            pending_inputs: VecDeque::new(),
            last_server_tick: server_tick,
            last_acknowledged_input: authoritative.last_input_sequence,
            last_produced_input: authoritative.last_input_sequence,
        })
    }

    /// Predicts exactly one 60 Hz client tick and retains its input for later replay.
    ///
    /// # Errors
    ///
    /// Rejects an exhausted/non-contiguous sequence, a full history, or invalid movement before
    /// changing the predicted player.
    pub fn predict(
        &mut self,
        input: PlayerInputCommand,
        world: &World,
    ) -> Result<ClientPredictionStep, ClientPredictionError> {
        let expected = self
            .last_produced_input
            .checked_add(1)
            .ok_or(ClientPredictionError::InputSequenceExhausted)?;
        if input.input_sequence != expected {
            return Err(ClientPredictionError::NonSequentialInput {
                expected,
                received: input.input_sequence,
            });
        }
        if self.pending_inputs.len() >= MAX_PENDING_PREDICTED_INPUTS {
            return Err(ClientPredictionError::PendingInputLimit);
        }
        let mut player = self.player;
        player.accept_input(input)?;
        let simulation = player.step(world);
        let state = player.state();
        self.player = player;
        self.pending_inputs.push_back(input);
        self.last_produced_input = input.input_sequence;
        Ok(ClientPredictionStep {
            input_sequence: input.input_sequence,
            state,
            simulation,
        })
    }

    /// Restores a newer authoritative state and deterministically replays every unacknowledged
    /// local input on top of it.
    ///
    /// # Errors
    ///
    /// Rejects the wrong session, stale state/acknowledgement, or an acknowledgement for an input
    /// never produced locally. Reconciliation is atomic on every failure.
    pub fn reconcile(
        &mut self,
        server_tick: u64,
        authoritative: ReplicatedPlayerState,
        world: &World,
    ) -> Result<PredictionReconcileReport, ClientPredictionError> {
        authoritative.validate()?;
        if authoritative.session_id != self.session_id {
            return Err(ClientPredictionError::WrongSession {
                expected: self.session_id,
                received: authoritative.session_id,
            });
        }
        if server_tick <= self.last_server_tick {
            return Err(ClientPredictionError::StaleServerTick {
                received: server_tick,
                last_applied: self.last_server_tick,
            });
        }
        if authoritative.last_input_sequence < self.last_acknowledged_input {
            return Err(ClientPredictionError::StaleAcknowledgement {
                received: authoritative.last_input_sequence,
                last_applied: self.last_acknowledged_input,
            });
        }
        if authoritative.last_input_sequence > self.last_produced_input {
            return Err(ClientPredictionError::UnknownAcknowledgement {
                received: authoritative.last_input_sequence,
                last_produced: self.last_produced_input,
            });
        }

        let predicted_before = self.player.state().position_um;
        let mut player = self.player;
        player.restore_authoritative_state(authoritative_state(authoritative));
        let acknowledged_inputs = self
            .pending_inputs
            .iter()
            .take_while(|input| input.input_sequence <= authoritative.last_input_sequence)
            .count();
        for input in self.pending_inputs.iter().skip(acknowledged_inputs) {
            player.accept_input(*input)?;
            let _step = player.step(world);
        }
        let state = player.state();
        let correction_um = FixedMicrometers3 {
            x: state.position_um.x.saturating_sub(predicted_before.x),
            y: state.position_um.y.saturating_sub(predicted_before.y),
            z: state.position_um.z.saturating_sub(predicted_before.z),
        };
        for _ in 0..acknowledged_inputs {
            self.pending_inputs.pop_front();
        }
        self.player = player;
        self.last_server_tick = server_tick;
        self.last_acknowledged_input = authoritative.last_input_sequence;
        Ok(PredictionReconcileReport {
            server_tick,
            acknowledged_inputs,
            replayed_inputs: self.pending_inputs.len(),
            correction_um,
            state,
        })
    }

    #[must_use]
    pub const fn session_id(&self) -> u64 {
        self.session_id
    }

    #[must_use]
    pub const fn state(&self) -> AuthoritativePlayerState {
        self.player.state()
    }

    #[must_use]
    pub fn pending_inputs(&self) -> usize {
        self.pending_inputs.len()
    }

    #[must_use]
    pub const fn last_server_tick(&self) -> u64 {
        self.last_server_tick
    }
}

const fn authoritative_state(state: ReplicatedPlayerState) -> AuthoritativePlayerState {
    AuthoritativePlayerState {
        position_um: state.position_um,
        velocity_um_per_second: state.velocity_um_per_second,
        integration_remainder: state.integration_remainder,
        grounded: state.grounded,
        last_input_sequence: state.last_input_sequence,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{IVec3, MICROMETERS_PER_VOXEL, Material, Voxel};

    fn floor_world() -> World {
        let mut world = World::default();
        world.fill_box(
            IVec3::new(-16, 0, 20),
            IVec3::new(16, 0, 60),
            Voxel::new(Material::Stone),
        );
        world
    }

    fn replicated(session_id: u64, state: AuthoritativePlayerState) -> ReplicatedPlayerState {
        ReplicatedPlayerState::from_authoritative(session_id, state)
    }

    #[test]
    fn reconciliation_replays_unacknowledged_inputs_to_the_exact_server_state() {
        let world = floor_world();
        let spawn = FixedMicrometers3 {
            x: 0,
            y: MICROMETERS_PER_VOXEL,
            z: 40 * MICROMETERS_PER_VOXEL,
        };
        let mut server = AuthoritativePlayer::new(spawn);
        let mut client = ClientPrediction::new(1, replicated(7, server.state()))
            .expect("valid initial authority");
        let mut state_at_three = server.state();
        for input_sequence in 1..=6 {
            let input = PlayerInputCommand {
                input_sequence,
                movement_x_per_mille: 1_000,
                sprint: input_sequence >= 4,
                ..PlayerInputCommand::default()
            };
            server.accept_input(input).expect("server input");
            let _server_step = server.step(&world);
            client.predict(input, &world).expect("client prediction");
            if input_sequence == 3 {
                state_at_three = server.state();
            }
        }
        assert_eq!(client.state(), server.state());

        let report = client
            .reconcile(4, replicated(7, state_at_three), &world)
            .expect("authoritative reconciliation");
        assert_eq!(report.acknowledged_inputs, 3);
        assert_eq!(report.replayed_inputs, 3);
        assert_eq!(report.correction_um, FixedMicrometers3::default());
        assert_eq!(report.state, server.state());
        assert_eq!(client.pending_inputs(), 3);
    }

    #[test]
    fn invalid_reconciliation_is_atomic() {
        let world = floor_world();
        let server = AuthoritativePlayer::default();
        let initial = replicated(9, server.state());
        let mut client = ClientPrediction::new(10, initial).expect("valid initial authority");
        client
            .predict(
                PlayerInputCommand {
                    input_sequence: 1,
                    movement_z_per_mille: 1_000,
                    ..PlayerInputCommand::default()
                },
                &world,
            )
            .expect("prediction");
        let before = client.clone();
        let mut wrong = initial;
        wrong.session_id = 10;
        assert_eq!(
            client.reconcile(11, wrong, &world),
            Err(ClientPredictionError::WrongSession {
                expected: 9,
                received: 10,
            })
        );
        assert_eq!(client, before);
        let mut unknown = initial;
        unknown.last_input_sequence = 2;
        assert_eq!(
            client.reconcile(11, unknown, &world),
            Err(ClientPredictionError::UnknownAcknowledgement {
                received: 2,
                last_produced: 1,
            })
        );
        assert_eq!(client, before);
    }

    #[test]
    fn prediction_history_and_sequence_are_bounded_before_mutation() {
        let world = floor_world();
        let server = AuthoritativePlayer::default();
        let mut client = ClientPrediction::new(1, replicated(11, server.state()))
            .expect("valid initial authority");
        assert_eq!(
            client.predict(
                PlayerInputCommand {
                    input_sequence: 2,
                    ..PlayerInputCommand::default()
                },
                &world,
            ),
            Err(ClientPredictionError::NonSequentialInput {
                expected: 1,
                received: 2,
            })
        );
        for input_sequence in 1..=MAX_PENDING_PREDICTED_INPUTS {
            client
                .predict(
                    PlayerInputCommand {
                        input_sequence: u64::try_from(input_sequence).expect("bounded sequence"),
                        ..PlayerInputCommand::default()
                    },
                    &world,
                )
                .expect("bounded prediction");
        }
        let before = client.state();
        assert_eq!(
            client.predict(
                PlayerInputCommand {
                    input_sequence: u64::try_from(MAX_PENDING_PREDICTED_INPUTS)
                        .expect("bounded sequence")
                        + 1,
                    ..PlayerInputCommand::default()
                },
                &world,
            ),
            Err(ClientPredictionError::PendingInputLimit)
        );
        assert_eq!(client.state(), before);
        assert_eq!(client.pending_inputs(), MAX_PENDING_PREDICTED_INPUTS);
    }
}
