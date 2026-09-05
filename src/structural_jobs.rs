//! Immutable, bounded server-side structural jobs. No fracture is committed by this module.

use crate::{
    AuthoritativeServer, IVec3, Voxel, World, chunk_position,
    elasticity::{
        ElasticError, ElasticJob, ElasticModel, ElasticNode, ElasticOptions, ElasticProgress,
        ElasticSolution, MAX_ELASTIC_NODES,
    },
    structural::StructuralAnchors,
    structural_failure::{
        PreparedStructuralFailure, StructuralFailureError, StructuralStrengths, prepare_failure,
    },
    world::ChunkObservation,
};
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    io,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, SyncSender, TryRecvError, TrySendError},
    },
    thread::{self, JoinHandle},
};

pub const MAX_STRUCTURAL_SNAPSHOT_CHUNKS: usize = 512;
const MAX_OBSERVED_CHUNKS: usize = 7 * MAX_ELASTIC_NODES + 1;
const _: () = assert!(crate::MICROMETERS_PER_VOXEL == 1_000_000);
const _: () = assert!(crate::physics::GRAVITY_UM_PER_SECOND_SQUARED == -9_810_000);

#[derive(Clone, Copy, Debug)]
pub struct ElasticMaterial {
    pub young_modulus_pa: f64,
    pub poisson_ratio: f64,
}

/// Explicit server-owned constants for Soil, Stone, Wood, Brick, Concrete, Steel and Glass.
/// There is deliberately no default claiming that synthetic constants are calibrated materials.
#[derive(Clone, Copy, Debug)]
pub struct StructuralMaterials([ElasticMaterial; 7]);

impl StructuralMaterials {
    /// # Errors
    /// Rejects nonfinite or out-of-kernel-range elastic parameters before a job can be captured.
    pub fn new(materials: [ElasticMaterial; 7]) -> Result<Self, StructuralJobError> {
        if materials.iter().any(|entry| {
            !entry.young_modulus_pa.is_finite()
                || !(1e4..=1e12).contains(&entry.young_modulus_pa)
                || !entry.poisson_ratio.is_finite()
                || !(-0.9..=0.49).contains(&entry.poisson_ratio)
        }) {
            return Err(StructuralJobError::InvalidMaterials);
        }
        Ok(Self(materials))
    }

    fn node(
        self,
        position: IVec3,
        voxel: Voxel,
        fixed: bool,
    ) -> Result<ElasticNode, StructuralJobError> {
        // Exhaustive matching makes adding a material an explicit calibration decision; wire
        // discriminants must never become an unchecked worker-side array index.
        let index = match voxel.material {
            crate::Material::Air => return Err(StructuralJobError::InvalidVoxel),
            crate::Material::Soil => 0,
            crate::Material::Stone => 1,
            crate::Material::Wood => 2,
            crate::Material::Brick => 3,
            crate::Material::Concrete => 4,
            crate::Material::Steel => 5,
            crate::Material::Glass => 6,
        };
        let parameters = self.0[index];
        Ok(ElasticNode {
            position,
            young_modulus_pa: parameters.young_modulus_pa,
            poisson_ratio: parameters.poisson_ratio,
            mass_kg: f64::from(voxel.material.properties().density_kg_m3),
            integrity: voxel.integrity,
            fixed,
            load: [0.0; 6],
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StructuralJobError {
    NotConfigured,
    InvalidMaterials,
    SnapshotTooLarge,
    NoStructure,
    FixedSeed,
    InvalidVoxel,
    DomainTooLarge,
    ObservationLimit,
    Elastic(ElasticError),
    StaleOrForeign,
    Busy,
    WorkerStopped,
    Cancelled,
}

impl core::fmt::Display for StructuralJobError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "structural job: {self:?}")
    }
}
impl std::error::Error for StructuralJobError {}
impl From<ElasticError> for StructuralJobError {
    fn from(error: ElasticError) -> Self {
        Self::Elastic(error)
    }
}

#[derive(Default)]
pub(crate) struct StructuralContext {
    identity: Arc<()>,
    materials: Option<StructuralMaterials>,
    strengths: Option<StructuralStrengths>,
}

// A fork is a different authority even if all initial chunk payloads are shared.
impl Clone for StructuralContext {
    fn clone(&self) -> Self {
        Self {
            identity: Arc::new(()),
            materials: self.materials,
            strengths: self.strengths,
        }
    }
}

impl StructuralContext {
    pub(crate) fn invalidate(&mut self) {
        self.identity = Arc::new(());
    }

    pub(crate) fn configure(&mut self, materials: StructuralMaterials) {
        self.materials = Some(materials);
        self.invalidate();
    }

    pub(crate) fn configure_strengths(&mut self, strengths: StructuralStrengths) {
        self.strengths = Some(strengths);
        self.invalidate();
    }

    fn capture(
        &self,
        world: &World,
        anchors: &StructuralAnchors,
        seed: IVec3,
    ) -> Result<StructuralRequest, StructuralJobError> {
        let materials = self.materials.ok_or(StructuralJobError::NotConfigured)?;
        let world = world
            .bounded_snapshot(MAX_STRUCTURAL_SNAPSHOT_CHUNKS)
            .ok_or(StructuralJobError::SnapshotTooLarge)?;
        Ok(StructuralRequest {
            world,
            anchors: anchors.clone(),
            seed,
            materials,
            strengths: self.strengths,
            identity: Arc::clone(&self.identity),
        })
    }

    pub(crate) fn validate<'a>(
        &self,
        world: &World,
        completed: &'a CompletedStructuralJob,
    ) -> Result<&'a ElasticSolution, StructuralJobError> {
        if !Arc::ptr_eq(&self.identity, &completed.identity)
            || completed
                .observations
                .iter()
                .any(|observed| !observed.matches(world))
        {
            return Err(StructuralJobError::StaleOrForeign);
        }
        completed.result.as_ref().map_err(|error| *error)
    }
}

struct StructuralRequest {
    world: World,
    anchors: StructuralAnchors,
    seed: IVec3,
    materials: StructuralMaterials,
    strengths: Option<StructuralStrengths>,
    identity: Arc<()>,
}

/// An opaque calculation, not permission to mutate the world. Inspect through the originating
/// authority's `structural_result` to reject stale chunks and changed configuration first.
pub struct CompletedStructuralJob {
    identity: Arc<()>,
    observations: Vec<ChunkObservation>,
    result: Result<ElasticSolution, StructuralJobError>,
    failure: Result<Option<PreparedStructuralFailure>, StructuralFailureError>,
}

impl CompletedStructuralJob {
    pub(crate) fn into_failure(
        self,
    ) -> Result<Option<PreparedStructuralFailure>, StructuralFailureError> {
        self.failure
    }
    #[must_use]
    pub const fn observed_chunks(&self) -> usize {
        self.observations.len()
    }
}

fn read_voxel(
    world: &World,
    position: IVec3,
    observed: &mut BTreeSet<IVec3>,
) -> Result<Voxel, StructuralJobError> {
    let chunk = chunk_position(position);
    if observed.len() == MAX_OBSERVED_CHUNKS && !observed.contains(&chunk) {
        return Err(StructuralJobError::ObservationLimit);
    }
    observed.insert(chunk);
    let voxel = world.voxel(position);
    if voxel.is_solid() && voxel.integrity == 0 {
        return Err(StructuralJobError::InvalidVoxel);
    }
    Ok(voxel)
}

pub(crate) fn neighbors(position: IVec3) -> impl Iterator<Item = IVec3> {
    (0..3).flat_map(move |axis| {
        [-1, 1].into_iter().filter_map(move |direction| {
            let mut coordinates = [position.x, position.y, position.z];
            coordinates[axis] = coordinates[axis].checked_add(direction)?;
            Some(IVec3::new(coordinates[0], coordinates[1], coordinates[2]))
        })
    })
}

fn extract_domain(
    request: &StructuralRequest,
    observed: &mut BTreeSet<IVec3>,
    cancelled: &AtomicBool,
) -> Result<Vec<ElasticNode>, StructuralJobError> {
    let first = read_voxel(&request.world, request.seed, observed)?;
    if !first.is_solid() {
        return Err(StructuralJobError::NoStructure);
    }
    if request.anchors.contains(request.seed) {
        return Err(StructuralJobError::FixedSeed);
    }
    let mut nodes = BTreeMap::from([(request.seed, first)]);
    let mut queue = VecDeque::from([request.seed]);
    while let Some(position) = queue.pop_front() {
        if cancelled.load(Ordering::Relaxed) {
            return Err(StructuralJobError::Cancelled);
        }
        // Do not terminate the component when the first clamp is found. Every free node's full
        // neighborhood belongs to the mechanical domain; only actual clamped solids stop a branch.
        for neighbor in neighbors(position) {
            if nodes.contains_key(&neighbor) {
                continue;
            }
            let voxel = read_voxel(&request.world, neighbor, observed)?;
            if !voxel.is_solid() {
                continue;
            }
            if nodes.len() == MAX_ELASTIC_NODES {
                return Err(StructuralJobError::DomainTooLarge);
            }
            nodes.insert(neighbor, voxel);
            if !request.anchors.contains(neighbor) {
                queue.push_back(neighbor);
            }
        }
    }
    nodes
        .into_iter()
        .map(|(position, voxel)| {
            request
                .materials
                .node(position, voxel, request.anchors.contains(position))
        })
        .collect()
}

fn run_request(request: StructuralRequest, cancelled: &AtomicBool) -> CompletedStructuralJob {
    let mut observed = BTreeSet::new();
    let result = (|| {
        if cancelled.load(Ordering::Relaxed) {
            return Err(StructuralJobError::Cancelled);
        }
        let nodes = extract_domain(&request, &mut observed, cancelled)?;
        let model = ElasticModel::new(&nodes, 1.0, [0.0, -9.81, 0.0])?;
        let mut job = ElasticJob::new(model, ElasticOptions::default())?;
        loop {
            if cancelled.load(Ordering::Relaxed) {
                return Err(StructuralJobError::Cancelled);
            }
            if job.advance(8)? == ElasticProgress::Converged {
                return job.finish().map_err(Into::into);
            }
        }
    })();
    let failure = match (&result, request.strengths) {
        (Ok(solution), Some(strengths)) => prepare_failure(
            &request.world,
            &request.anchors,
            solution,
            strengths,
            cancelled,
        ),
        (Err(error), _) => Err(StructuralFailureError::Job(*error)),
        (Ok(_), None) => Err(StructuralFailureError::NotConfigured),
    };
    let observations = observed
        .into_iter()
        .map(|position| request.world.observe_chunk(position))
        .collect();
    CompletedStructuralJob {
        identity: request.identity,
        observations,
        result,
        failure,
    }
}

/// One worker and at most one outstanding request, including a completed-but-unconsumed result.
///
/// Expensive domain traversal and numerical work happen only on the worker. Capturing a request
/// shares at most 512 dense chunks and copies only bounded map/configuration metadata.
/// A stopped worker is a terminal scheduler failure: the owner must replace the scheduler and
/// investigate the failure, never accept an incomplete result or silently treat it as support.
pub struct StructuralScheduler {
    sender: Option<SyncSender<StructuralRequest>>,
    receiver: Option<Receiver<CompletedStructuralJob>>,
    worker: Option<JoinHandle<()>>,
    cancelled: Arc<AtomicBool>,
    busy: bool,
}

impl StructuralScheduler {
    /// # Errors
    /// Returns the operating system's thread-creation failure without starting a partial scheduler.
    pub fn new() -> io::Result<Self> {
        let (sender, jobs) = mpsc::sync_channel(1);
        let (completed, receiver) = mpsc::sync_channel(1);
        let cancelled = Arc::new(AtomicBool::new(false));
        let cancellation = Arc::clone(&cancelled);
        let worker = thread::Builder::new()
            .name("structural-load".to_owned())
            .spawn(move || {
                while let Ok(request) = jobs.recv() {
                    let result = run_request(request, &cancellation);
                    if completed.send(result).is_err() {
                        break;
                    }
                }
            })?;
        Ok(Self {
            sender: Some(sender),
            receiver: Some(receiver),
            worker: Some(worker),
            cancelled,
            busy: false,
        })
    }

    /// Captures a server-selected free solid seed after checking backpressure; never traverses it.
    ///
    /// # Errors
    /// Rejects backpressure before snapshot allocation, absent configuration, oversized residency
    /// and a disconnected worker. This API is not exposed to network command decoding.
    pub fn submit(
        &mut self,
        authority: &AuthoritativeServer,
        seed: IVec3,
    ) -> Result<(), StructuralJobError> {
        if self.busy {
            return Err(StructuralJobError::Busy);
        }
        let sender = self
            .sender
            .as_ref()
            .ok_or(StructuralJobError::WorkerStopped)?;
        let request = authority.structural_context.capture(
            authority.world(),
            authority.structural_anchors(),
            seed,
        )?;
        // Polling consumed the preceding result before busy became false, so that request no
        // longer reads this flag. Reuse it only when admitting the next single outstanding job.
        self.cancelled.store(false, Ordering::Relaxed);
        match sender.try_send(request) {
            Ok(()) => {
                self.busy = true;
                Ok(())
            }
            Err(TrySendError::Full(_)) => Err(StructuralJobError::Busy),
            Err(TrySendError::Disconnected(_)) => Err(StructuralJobError::WorkerStopped),
        }
    }

    /// Requests cancellation, including a result already waiting in the completion channel.
    /// The outstanding slot remains occupied until `poll` drains its Cancelled result.
    #[must_use]
    pub fn cancel_pending(&self) -> bool {
        if !self.busy {
            return false;
        }
        self.cancelled.store(true, Ordering::Relaxed);
        true
    }

    /// # Errors
    /// Reports a stopped worker; a pending result is never presented as a supported structure.
    pub fn poll(&mut self) -> Result<Option<CompletedStructuralJob>, StructuralJobError> {
        let receiver = self
            .receiver
            .as_ref()
            .ok_or(StructuralJobError::WorkerStopped)?;
        match receiver.try_recv() {
            Ok(mut result) => {
                if self.cancelled.load(Ordering::Relaxed) {
                    result.result = Err(StructuralJobError::Cancelled);
                    result.failure = Err(StructuralJobError::Cancelled.into());
                }
                self.busy = false;
                Ok(Some(result))
            }
            Err(TryRecvError::Empty) => Ok(None),
            Err(TryRecvError::Disconnected) => {
                self.busy = false;
                Err(StructuralJobError::WorkerStopped)
            }
        }
    }
}

impl Drop for StructuralScheduler {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Relaxed);
        self.sender.take();
        self.receiver.take();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

#[cfg(test)]
mod tests;
