use super::{
    AuthoritativeServer, BodyError, BodyId, CommandError, DeltaPacket, DestructionReport,
    MAX_ACTIVE_BODIES, MAX_ACTIVE_BODY_VOXELS, MAX_DATAGRAM_BYTES, RigidBodyState,
    body_fingerprint_token, merged_detachment_changes, payload_fits_protocol,
};
use crate::{
    elasticity::MAX_ELASTIC_NODES,
    structural_failure::{StructuralFailureError, StructuralFailureReport, StructuralStrengths},
    structural_jobs::CompletedStructuralJob,
};

impl AuthoritativeServer {
    /// Enables explicitly supplied brittle section strengths for subsequent worker jobs.
    /// Does not schedule work automatically or calibrate these values as game material presets.
    #[must_use]
    pub fn with_structural_strengths(mut self, strengths: StructuralStrengths) -> Self {
        self.structural_context.configure_strengths(strengths);
        self
    }

    /// Atomically promotes a worker-prepared, mass-preserving coarse structural failure.
    /// The caller owns tick scheduling and broadcast/retention of the returned ordinary delta.
    /// No domain traversal, numerical solve or descriptor construction occurs here.
    ///
    /// # Errors
    /// Rejects stale/foreign/cancelled/unconfigured jobs, incomplete preparation, live capacity,
    /// protocol limits and exhausted counters before mutating world, bodies, replay or inventory.
    pub fn commit_structural_failure(
        &mut self,
        completed: CompletedStructuralJob,
    ) -> Result<Option<(DeltaPacket, StructuralFailureReport)>, StructuralFailureError> {
        self.structural_result(&completed)?;
        let Some(mut plan) = completed.into_failure()? else {
            return Ok(None);
        };
        let spawned = plan
            .bodies
            .iter()
            .map(|body| body.voxels.len())
            .sum::<usize>();
        if plan.bodies.is_empty() || plan.bodies.len() > 7 || spawned > MAX_ELASTIC_NODES {
            return Err(StructuralFailureError::InvalidPlan);
        }
        let active_count = self.bodies.len().saturating_add(plan.bodies.len());
        let active_voxels = self.active_body_voxels.saturating_add(spawned);
        if active_count > MAX_ACTIVE_BODIES {
            return Err(CommandError::TooManyActiveBodies(active_count).into());
        }
        if active_voxels > MAX_ACTIVE_BODY_VOXELS {
            return Err(CommandError::TooManyActiveBodyVoxels(active_voxels).into());
        }
        let count =
            BodyId::try_from(plan.bodies.len()).map_err(|_| CommandError::BodyIdExhausted)?;
        let next_body_id = self
            .next_body_id
            .checked_add(count)
            .ok_or(CommandError::BodyIdExhausted)?;
        let next_sequence = self
            .next_sequence
            .checked_add(1)
            .ok_or(StructuralFailureError::SequenceExhausted)?;
        let tick = self
            .world
            .tick()
            .checked_add(1)
            .ok_or(StructuralFailureError::SequenceExhausted)?;
        let mut mass_kg = 0;
        for (offset, body) in plan.bodies.iter_mut().enumerate() {
            if body.voxels.len() > self.body_limits.max_voxels {
                return Err(BodyError::TooManyVoxels(body.voxels.len()).into());
            }
            body.id = self.next_body_id
                + BodyId::try_from(offset).map_err(|_| CommandError::BodyIdExhausted)?;
            if self.bodies.contains_key(&body.id) {
                return Err(CommandError::DuplicateBodyId(body.id).into());
            }
            if body
                .voxels
                .iter()
                .any(|voxel| self.world.voxel(voxel.position) != voxel.voxel)
            {
                return Err(StructuralFailureError::InvalidPlan);
            }
            mass_kg += body.mass_kg;
        }
        let (changes, body_assignments) =
            merged_detachment_changes(&self.world, &DestructionReport::default(), &plan.bodies);
        if changes.len() != spawned {
            return Err(StructuralFailureError::InvalidPlan);
        }
        if !payload_fits_protocol(changes.len(), body_assignments.len(), 0, MAX_DATAGRAM_BYTES) {
            return Err(CommandError::TransactionTooLarge {
                changes: changes.len(),
                body_assignments: body_assignments.len(),
            }
            .into());
        }
        let mut packet = DeltaPacket {
            sequence: self.next_sequence,
            tick,
            base_fingerprint: self.world.fingerprint(),
            final_fingerprint: 0,
            base_body_fingerprint: self.body_fingerprint,
            final_body_fingerprint: 0,
            changes,
            body_assignments,
            body_updates: Vec::new(),
        };
        let report = StructuralFailureReport {
            candidate: plan.candidate,
            spawned_bodies: plan.bodies.len(),
            moved_voxels: spawned,
            mass_kg,
        };
        // All fallible checks precede this mutation boundary. New bodies begin at rest in the
        // original integer pose; no fabricated blast impulse or floating displacement is applied.
        for body in plan.bodies {
            let state = RigidBodyState::at_spawn(&body);
            self.body_fingerprint ^= body_fingerprint_token(&body, state);
            self.body_states.insert(body.id, state);
            self.bodies.insert(body.id, body);
        }
        for change in &packet.changes {
            self.world.set_voxel(change.position, change.after);
        }
        self.active_body_voxels = active_voxels;
        self.next_body_id = next_body_id;
        self.next_sequence = next_sequence;
        self.world.set_tick(tick);
        packet.final_fingerprint = self.world.fingerprint();
        packet.final_body_fingerprint = self.body_fingerprint;
        Ok(Some((packet, report)))
    }
}

#[cfg(test)]
mod tests;
