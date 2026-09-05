//! Bounded background scheduler for CPU chunk meshing.

use crate::{
    IVec3, RigidBodyDescriptor, World, mesh::CpuBodyMesh, mesh::CpuMesh, mesh::mesh_body,
    mesh::mesh_chunk,
};
use core::fmt;
use std::{
    sync::Arc,
    sync::mpsc::{self, Receiver, SyncSender, TryRecvError, TrySendError},
    thread::{self, JoinHandle},
};

pub const MAX_CHUNKS_PER_MESH_JOB: usize = 256;
pub const MAX_BODIES_PER_MESH_JOB: usize = 16;
pub const MAX_BODY_VOXELS_PER_MESH_JOB: usize = 32_768;

enum MeshJob {
    Chunks {
        world: Arc<World>,
        chunks: Vec<IVec3>,
    },
    Bodies(Vec<RigidBodyDescriptor>),
}

#[derive(Debug)]
pub enum CompletedMeshJob {
    Chunks {
        world_fingerprint: u128,
        meshes: Vec<(IVec3, CpuMesh)>,
    },
    Bodies(Vec<CpuBodyMesh>),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MeshScheduleError {
    EmptyJob,
    TooManyChunks(usize),
    TooManyBodies(usize),
    TooManyBodyVoxels(usize),
    Busy,
    WorkerStopped,
}

impl fmt::Display for MeshScheduleError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyJob => write!(formatter, "mesh job is empty"),
            Self::TooManyChunks(count) => write!(
                formatter,
                "mesh job has {count} chunks; maximum is {MAX_CHUNKS_PER_MESH_JOB}"
            ),
            Self::TooManyBodies(count) => write!(
                formatter,
                "mesh job has {count} bodies; maximum is {MAX_BODIES_PER_MESH_JOB}"
            ),
            Self::TooManyBodyVoxels(count) => write!(
                formatter,
                "mesh job has {count} body voxels; maximum is {MAX_BODY_VOXELS_PER_MESH_JOB}"
            ),
            Self::Busy => write!(formatter, "mesh worker queue is full"),
            Self::WorkerStopped => write!(formatter, "mesh worker stopped"),
        }
    }
}

impl std::error::Error for MeshScheduleError {}

/// One worker and one queued job bound memory while keeping CPU meshing off the render thread.
pub struct MeshScheduler {
    jobs: Option<SyncSender<MeshJob>>,
    results: Option<Receiver<CompletedMeshJob>>,
    worker: Option<JoinHandle<()>>,
}

impl Default for MeshScheduler {
    fn default() -> Self {
        Self::new()
    }
}

impl MeshScheduler {
    /// Starts the single bounded worker.
    ///
    /// # Panics
    ///
    /// Panics when the operating system refuses to create the worker thread.
    #[must_use]
    pub fn new() -> Self {
        let (job_sender, job_receiver) = mpsc::sync_channel::<MeshJob>(1);
        let (result_sender, result_receiver) = mpsc::sync_channel::<CompletedMeshJob>(1);
        let worker = thread::Builder::new()
            .name("chunk-mesher".to_owned())
            .spawn(move || mesh_worker(&job_receiver, &result_sender))
            .expect("operating system must allow the bounded mesh worker");
        Self {
            jobs: Some(job_sender),
            results: Some(result_receiver),
            worker: Some(worker),
        }
    }

    /// Queues one immutable world snapshot without waiting for the worker.
    ///
    /// # Errors
    ///
    /// Rejects empty or oversized jobs, backpressure, and a stopped worker.
    pub fn submit(&self, world: Arc<World>, chunks: Vec<IVec3>) -> Result<(), MeshScheduleError> {
        if chunks.is_empty() {
            return Err(MeshScheduleError::EmptyJob);
        }
        if chunks.len() > MAX_CHUNKS_PER_MESH_JOB {
            return Err(MeshScheduleError::TooManyChunks(chunks.len()));
        }
        let sender = self.jobs.as_ref().ok_or(MeshScheduleError::WorkerStopped)?;
        match sender.try_send(MeshJob::Chunks { world, chunks }) {
            Ok(()) => Ok(()),
            Err(TrySendError::Full(_)) => Err(MeshScheduleError::Busy),
            Err(TrySendError::Disconnected(_)) => Err(MeshScheduleError::WorkerStopped),
        }
    }

    /// Queues immutable body geometry without waiting for the worker.
    ///
    /// # Errors
    ///
    /// Rejects empty or oversized batches, excessive aggregate voxel work, backpressure, and a
    /// stopped worker.
    pub fn submit_bodies(&self, bodies: Vec<RigidBodyDescriptor>) -> Result<(), MeshScheduleError> {
        if bodies.is_empty() {
            return Err(MeshScheduleError::EmptyJob);
        }
        if bodies.len() > MAX_BODIES_PER_MESH_JOB {
            return Err(MeshScheduleError::TooManyBodies(bodies.len()));
        }
        let voxel_count = bodies.iter().map(|body| body.voxels.len()).sum::<usize>();
        if voxel_count > MAX_BODY_VOXELS_PER_MESH_JOB {
            return Err(MeshScheduleError::TooManyBodyVoxels(voxel_count));
        }
        let sender = self.jobs.as_ref().ok_or(MeshScheduleError::WorkerStopped)?;
        match sender.try_send(MeshJob::Bodies(bodies)) {
            Ok(()) => Ok(()),
            Err(TrySendError::Full(_)) => Err(MeshScheduleError::Busy),
            Err(TrySendError::Disconnected(_)) => Err(MeshScheduleError::WorkerStopped),
        }
    }

    /// Polls one completed batch without ever blocking the render loop.
    ///
    /// # Errors
    ///
    /// Reports a stopped worker after all preceding results have been consumed.
    pub fn poll(&self) -> Result<Option<CompletedMeshJob>, MeshScheduleError> {
        let receiver = self
            .results
            .as_ref()
            .ok_or(MeshScheduleError::WorkerStopped)?;
        match receiver.try_recv() {
            Ok(result) => Ok(Some(result)),
            Err(TryRecvError::Empty) => Ok(None),
            Err(TryRecvError::Disconnected) => Err(MeshScheduleError::WorkerStopped),
        }
    }
}

impl Drop for MeshScheduler {
    fn drop(&mut self) {
        self.jobs.take();
        self.results.take();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn mesh_worker(receiver: &Receiver<MeshJob>, sender: &SyncSender<CompletedMeshJob>) {
    while let Ok(job) = receiver.recv() {
        let completed = match job {
            MeshJob::Chunks { world, chunks } => {
                let world_fingerprint = world.fingerprint();
                let meshes = chunks
                    .into_iter()
                    .map(|chunk| (chunk, mesh_chunk(&world, chunk)))
                    .collect();
                CompletedMeshJob::Chunks {
                    world_fingerprint,
                    meshes,
                }
            }
            MeshJob::Bodies(bodies) => {
                CompletedMeshJob::Bodies(bodies.iter().map(mesh_body).collect())
            }
        };
        if sender.send(completed).is_err() {
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Material, Voxel};
    use std::time::{Duration, Instant};

    #[test]
    fn worker_meshes_an_immutable_world_snapshot() {
        let mut world = World::default();
        world.set_voxel(IVec3::new(0, 0, 0), Voxel::new(Material::Brick));
        let fingerprint = world.fingerprint();
        let scheduler = MeshScheduler::new();
        scheduler
            .submit(Arc::new(world), vec![IVec3::new(0, 0, 0)])
            .expect("valid job should be queued");

        let deadline = Instant::now() + Duration::from_secs(2);
        let completed = loop {
            if let Some(completed) = scheduler.poll().expect("worker should remain connected") {
                break completed;
            }
            assert!(Instant::now() < deadline, "mesh worker timed out");
            thread::yield_now();
        };
        let CompletedMeshJob::Chunks {
            world_fingerprint,
            meshes,
        } = completed
        else {
            panic!("chunk job returned body meshes");
        };
        assert_eq!(world_fingerprint, fingerprint);
        assert_eq!(meshes.len(), 1);
        assert_eq!(meshes[0].1.exposed_faces(), 6);
    }

    #[test]
    fn oversized_jobs_are_rejected_before_allocation_on_the_worker() {
        let scheduler = MeshScheduler::new();
        let chunks = vec![IVec3::default(); MAX_CHUNKS_PER_MESH_JOB + 1];
        assert_eq!(
            scheduler.submit(Arc::new(World::default()), chunks),
            Err(MeshScheduleError::TooManyChunks(
                MAX_CHUNKS_PER_MESH_JOB + 1
            ))
        );
    }

    #[test]
    fn worker_meshes_body_geometry_off_thread() {
        use crate::{BodyLimits, Material, Voxel, structural::describe_island};

        let mut world = World::default();
        world.set_voxel(IVec3::new(4, 8, 2), Voxel::new(Material::Brick));
        let island = describe_island(&world, vec![IVec3::new(4, 8, 2)]);
        let body =
            RigidBodyDescriptor::from_detached_island(1, &world, &island, BodyLimits::default())
                .expect("body fixture");
        let scheduler = MeshScheduler::new();
        scheduler
            .submit_bodies(vec![body])
            .expect("body job should be queued");

        let deadline = Instant::now() + Duration::from_secs(2);
        let completed = loop {
            if let Some(completed) = scheduler.poll().expect("worker should remain connected") {
                break completed;
            }
            assert!(Instant::now() < deadline, "body mesh worker timed out");
            thread::yield_now();
        };
        let CompletedMeshJob::Bodies(meshes) = completed else {
            panic!("body job returned chunk meshes");
        };
        assert_eq!(meshes.len(), 1);
        assert_eq!(meshes[0].mesh.exposed_faces(), 6);
    }
}
