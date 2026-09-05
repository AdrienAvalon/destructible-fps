//! Bounded dirty-domain orchestration shared by local and network authority tick loops.

use crate::{
    AuthoritativeServer, DeltaPacket, IVec3, StructuralAnchors, VoxelChange,
    structural_failure::{StructuralFailureError, StructuralStrengths},
    structural_jobs::{
        CompletedStructuralJob, StructuralJobError, StructuralMaterials, StructuralScheduler,
        neighbors,
    },
};
use std::{
    collections::{BTreeSet, VecDeque},
    io,
    time::{Duration, Instant},
};

pub const MAX_PENDING_STRUCTURAL_SEEDS: usize = 8_192;
const MAX_SEED_CHECKS_PER_TICK: usize = 64;
const MAX_JOB_LATENCY: Duration = Duration::from_secs(5);

/// Trusted startup policy, never a network command. Constants require independent calibration.
pub struct StructuralSimulationConfig {
    pub anchors: StructuralAnchors,
    pub materials: StructuralMaterials,
    pub strengths: StructuralStrengths,
    pub initial_seeds: Vec<IVec3>,
}

/// Lifetime counters, not a safety assessment. Faults/overflow never disappear on an idle tick.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct StructuralRuntimeStatus {
    pub queued: usize,
    pub busy: bool,
    pub stopped: bool,
    pub overflowed: bool,
    pub submitted: u64,
    pub assessed: u64,
    pub committed: u64,
    pub stale: u64,
    pub failed: u64,
    pub last_failed_seed: Option<IVec3>,
}

impl StructuralRuntimeStatus {
    /// Empty queues do not erase a rejected assessment or discarded dirty coverage.
    #[must_use]
    pub const fn incomplete(self) -> bool {
        self.failed != 0 || self.overflowed || self.stopped
    }
}

/// Startup-owned, bounded structural worker orchestration.
///
/// Call `observe_changes` for every committed static edit and `tick` once
/// per authority tick, outside receive handlers. Initial map seeds are explicit authored input.
/// Overflow indicates incomplete coverage; it is not permission to report the scene as supported.
pub struct StructuralRuntime {
    scheduler: StructuralScheduler,
    queue: VecDeque<IVec3>,
    pending: BTreeSet<IVec3>,
    in_flight: Option<IVec3>,
    started: Option<Instant>,
    status: StructuralRuntimeStatus,
    last_error: Option<StructuralFailureError>,
}

impl StructuralRuntime {
    /// # Errors
    /// Rejects oversized initial input or an OS worker-creation failure. No default calibration.
    pub fn new(initial_seeds: &[IVec3]) -> io::Result<Self> {
        if initial_seeds.len() > MAX_PENDING_STRUCTURAL_SEEDS {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "too many structural seeds",
            ));
        }
        let mut runtime = Self {
            scheduler: StructuralScheduler::new()?,
            queue: VecDeque::new(),
            pending: BTreeSet::new(),
            in_flight: None,
            started: None,
            status: StructuralRuntimeStatus::default(),
            last_error: None,
        };
        for &seed in initial_seeds {
            runtime.enqueue(seed);
        }
        Ok(runtime)
    }

    #[must_use]
    pub fn status(&self) -> StructuralRuntimeStatus {
        StructuralRuntimeStatus {
            queued: self.queue.len(),
            busy: self.in_flight.is_some(),
            ..self.status
        }
    }

    #[must_use]
    pub const fn last_error(&self) -> Option<&StructuralFailureError> {
        self.last_error.as_ref()
    }

    fn enqueue(&mut self, seed: IVec3) {
        if self.pending.contains(&seed) {
            return;
        }
        if self.queue.len() == MAX_PENDING_STRUCTURAL_SEEDS {
            self.status.overflowed = true;
            return;
        }
        self.pending.insert(seed);
        self.queue.push_back(seed);
    }

    fn eligible(authority: &AuthoritativeServer, seed: IVec3) -> bool {
        authority.world().voxel(seed).is_solid() && !authority.structural_anchors().contains(seed)
    }

    /// Queue surviving sides of damage, construction, or fracture. Never takes seeds from wire
    /// requests: the caller supplies the actual committed delta, including partial-integrity edits.
    pub fn observe_changes(&mut self, authority: &AuthoritativeServer, changes: &[VoxelChange]) {
        for change in changes {
            for seed in std::iter::once(change.position).chain(neighbors(change.position)) {
                if Self::eligible(authority, seed) {
                    self.enqueue(seed);
                }
            }
        }
    }

    const fn record_error(&mut self, seed: IVec3, error: StructuralFailureError) {
        self.status.failed = self.status.failed.saturating_add(1);
        self.status.last_failed_seed = Some(seed);
        self.last_error = Some(error);
    }

    fn complete(
        &mut self,
        authority: &mut AuthoritativeServer,
        seed: IVec3,
        completed: CompletedStructuralJob,
    ) -> Option<DeltaPacket> {
        match authority
            .structural_context
            .validate_domain(authority.world(), &completed)
        {
            Ok(positions) => {
                // Only a current, completely extracted domain may subsume other dirty seeds.
                // Numerical failure can be coalesced too, but remains an explicit unresolved fault.
                self.queue.retain(|position| {
                    if positions.binary_search(position).is_ok() {
                        self.pending.remove(position);
                        false
                    } else {
                        true
                    }
                });
            }
            Err(StructuralJobError::StaleOrForeign) => {
                self.status.stale = self.status.stale.saturating_add(1);
                self.enqueue(seed);
                return None;
            }
            Err(error) => {
                self.record_error(seed, error.into());
                return None;
            }
        }
        match authority.commit_structural_failure(completed) {
            Ok(result) => {
                self.status.assessed = self.status.assessed.saturating_add(1);
                result.map(|(packet, _report)| {
                    self.status.committed = self.status.committed.saturating_add(1);
                    self.observe_changes(authority, &packet.changes);
                    packet
                })
            }
            Err(error) => {
                self.record_error(seed, error);
                None
            }
        }
    }

    /// At most one completion/commit and one new job. No wait, domain extraction or numeric solve.
    /// Returned deltas must use the caller's ordinary broadcast/retained-repair path before physics.
    /// Worker death is latched; replacement belongs outside the hot path, never an implicit retry.
    pub fn tick(&mut self, authority: &mut AuthoritativeServer) -> Option<DeltaPacket> {
        if self.status.stopped {
            return None;
        }
        let mut packet = None;
        if let Some(seed) = self.in_flight {
            if self
                .started
                .is_some_and(|started| started.elapsed() >= MAX_JOB_LATENCY)
            {
                let _ = self.scheduler.cancel_pending();
                self.status.stopped = true;
                self.record_error(seed, StructuralFailureError::DeadlineExceeded);
                // A thread cannot safely be killed. Keep the slot owned and do not join, reuse,
                // respawn or accept a late result on this tick path. Recovery is explicit startup.
                return None;
            }
            match self.scheduler.poll() {
                Ok(Some(completed)) => {
                    self.in_flight = None;
                    self.started = None;
                    packet = self.complete(authority, seed, completed);
                }
                Ok(None) => return None,
                Err(error) => {
                    self.in_flight = None;
                    self.started = None;
                    self.enqueue(seed);
                    self.status.stopped = true;
                    self.record_error(seed, error.into());
                    return None;
                }
            }
        }
        for _ in 0..MAX_SEED_CHECKS_PER_TICK {
            let Some(seed) = self.queue.pop_front() else {
                break;
            };
            self.pending.remove(&seed);
            if !Self::eligible(authority, seed) {
                continue;
            }
            match self.scheduler.submit(authority, seed) {
                Ok(()) => {
                    self.in_flight = Some(seed);
                    self.started = Some(Instant::now());
                    self.status.submitted = self.status.submitted.saturating_add(1);
                }
                Err(error) => {
                    self.status.stopped = error == StructuralJobError::WorkerStopped;
                    self.record_error(seed, error.into());
                }
            }
            break;
        }
        packet
    }
}

#[cfg(test)]
mod tests;
