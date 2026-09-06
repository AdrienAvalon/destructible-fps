//! Rifle intent is resolved at an authoritative eye and simulation tick, never a client hit point.

use super::{AuthoritativeServer, CommandError, DeltaPacket};
use crate::{
    DestructionReport, PlayerBuildContext,
    ballistics::{
        BallisticError, FixedRay, MAX_RIFLE_RANGE_UM, ProjectileReport, RifleCommand, RifleState,
        apply_rifle,
    },
};

impl From<BallisticError> for CommandError {
    fn from(value: BallisticError) -> Self {
        Self::Ballistic(value)
    }
}

impl AuthoritativeServer {
    /// Resolves a rifle shot from trusted server player state, then commits ammo and world together.
    /// The tick is the fixed simulation clock, NOT the per-transaction world tick.
    /// # Errors
    /// Invalid/replayed commands, cadence, ammunition, malformed rays and failed promotions leave
    /// weapon accounting and canonical world state unchanged. A miss still spends a round.
    pub fn execute_rifle(
        &mut self,
        client_id: u64,
        command: RifleCommand,
        player: PlayerBuildContext,
        simulation_tick: u64,
    ) -> Result<(DeltaPacket, ProjectileReport), CommandError> {
        if command.command_id == 0 {
            return Err(BallisticError::InvalidCommandId.into());
        }
        self.validate_fresh_command(client_id, command.command_id)?;
        if !player.is_canonical() {
            return Err(BallisticError::InvalidOrigin.into());
        }
        let ray = FixedRay::new(
            player.eye_position_um,
            command.direction,
            MAX_RIFLE_RANGE_UM,
        )?;
        let candidate = self
            .rifle_state(client_id, simulation_tick)
            .after_shot(simulation_tick)?;
        let cover = self
            .bodies
            .iter()
            .filter_map(|(&id, body)| {
                let (minimum, maximum) = crate::physics::body_aabb(body, self.body_states[&id]);
                // Enclose integer rotation rounding. This is coarse cover, not a fine collision mesh.
                ray.box_entry_um(
                    [minimum.x, minimum.y, minimum.z].map(|v| v.saturating_sub(2)),
                    [maximum.x, maximum.y, maximum.z].map(|v| v.saturating_add(2)),
                )
                .map(|distance| (id, distance))
            })
            .min_by_key(|&(id, distance)| (distance, id));
        let base = [self.world.fingerprint(), self.body_fingerprint];
        let mut report = apply_rifle(
            &mut self.world,
            player.eye_position_um,
            command.direction,
            cover,
        )?;
        let (packet, destruction) = self.commit_damage(
            client_id,
            command.command_id,
            report.destruction,
            None,
            base,
        )?;
        report.destruction = destruction;
        self.rifles.insert(client_id, candidate);
        Ok((packet, report))
    }

    /// Begins a finite-reserve reload at a trusted simulation tick.
    /// # Errors
    /// Rejects replay, unavailable reloads or tick overflow before changing any state.
    pub fn execute_reload(
        &mut self,
        client_id: u64,
        command_id: u64,
        simulation_tick: u64,
    ) -> Result<DeltaPacket, CommandError> {
        if command_id == 0 {
            return Err(BallisticError::InvalidCommandId.into());
        }
        self.validate_fresh_command(client_id, command_id)?;
        let candidate = self
            .rifle_state(client_id, simulation_tick)
            .after_reload(simulation_tick)?;
        let base = [self.world.fingerprint(), self.body_fingerprint];
        let (packet, _) = self.commit_damage(
            client_id,
            command_id,
            DestructionReport::default(),
            None,
            base,
        )?;
        self.rifles.insert(client_id, candidate);
        Ok(packet)
    }

    #[must_use]
    pub fn rifle_state(&self, client_id: u64, simulation_tick: u64) -> RifleState {
        self.rifles
            .get(&client_id)
            .copied()
            .unwrap_or_default()
            .at(simulation_tick)
    }

    #[must_use]
    pub fn last_command_id(&self, client_id: u64) -> u64 {
        self.last_command_id.get(&client_id).copied().unwrap_or(0)
    }
}
