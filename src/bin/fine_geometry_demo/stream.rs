//! Progressive fixed-stage bootstrap, then bounded batched atomic dirty-region replacements.
use destructible_fps::{
    IVec3,
    mesh::{
        CpuMesh,
        fine::{
            FineMeshReport, MAX_FINE_MESH_CHUNKS, MAX_FINE_MESH_INDICES, MAX_FINE_MESH_VERTICES,
        },
    },
    mesh_scheduler::CompletedMeshJob,
};
use std::{
    collections::BTreeMap,
    time::{Duration, Instant},
};

const MAX_SCENE_CHUNKS: usize = 256;
const MAX_SCENE_VERTICES: usize = 524_288;
const MAX_SCENE_INDICES: usize = 1_572_864;
const JOB_DEADLINE: Duration = Duration::from_secs(5);

struct Candidate {
    stage: usize,
    bootstrap: bool,
    started: Instant,
    next: usize,
    meshes: Vec<(IVec3, CpuMesh)>,
    report: FineMeshReport,
}
impl Candidate {
    fn accumulate(&mut self, report: FineMeshReport) -> Result<(), String> {
        let vertices = self
            .report
            .vertices
            .checked_add(report.vertices)
            .ok_or("vertex count overflow")?;
        let indices = self
            .report
            .indices
            .checked_add(report.indices)
            .ok_or("index count overflow")?;
        let (max_vertices, max_indices) = if self.bootstrap {
            (MAX_SCENE_VERTICES, MAX_SCENE_INDICES)
        } else {
            (MAX_FINE_MESH_VERTICES, MAX_FINE_MESH_INDICES)
        };
        if vertices > max_vertices || indices > max_indices {
            return Err("inspection candidate output budget".into());
        }
        self.report = FineMeshReport {
            vertices,
            indices,
            quads: self
                .report
                .quads
                .checked_add(report.quads)
                .ok_or("quad count overflow")?,
            work: self
                .report
                .work
                .checked_add(report.work)
                .ok_or("work count overflow")?,
            lines: self
                .report
                .lines
                .checked_add(report.lines)
                .ok_or("line count overflow")?,
        };
        Ok(())
    }
}
struct Pending {
    stage: usize,
    chunks: Vec<IVec3>,
    started: Instant,
}
pub struct Stream {
    all: Vec<IVec3>,
    dirty: Vec<IVec3>,
    fingerprints: [u128; 4],
    finishes_fingerprint: u128,
    replacement_job_chunks: usize,
    pub displayed: Option<usize>,
    candidate: Option<Candidate>,
    pending: Option<Pending>,
    resident: BTreeMap<IVec3, (usize, usize)>,
}
pub struct Delivery {
    pub meshes: Vec<(IVec3, CpuMesh)>,
    pub complete: Option<(usize, FineMeshReport, Duration)>,
}
impl Stream {
    pub fn new(
        all: Vec<IVec3>,
        dirty: Vec<IVec3>,
        fingerprints: [u128; 4],
        replacement_job_chunks: usize,
        finishes_fingerprint: u128,
    ) -> Result<Self, String> {
        if all.is_empty()
            || all.len() > MAX_SCENE_CHUNKS
            || dirty.is_empty()
            || dirty.len() > MAX_FINE_MESH_CHUNKS
            || !(1..=MAX_FINE_MESH_CHUNKS).contains(&replacement_job_chunks)
            || all.windows(2).any(|p| p[0] >= p[1])
            || dirty.windows(2).any(|p| p[0] >= p[1])
            || dirty.iter().any(|c| all.binary_search(c).is_err())
        {
            return Err("invalid bounded inspection chunk sets".into());
        }
        Ok(Self {
            all,
            dirty,
            fingerprints,
            finishes_fingerprint,
            replacement_job_chunks,
            displayed: None,
            candidate: None,
            pending: None,
            resident: BTreeMap::new(),
        })
    }

    pub const fn idle(&self) -> bool {
        self.pending.is_none() && self.candidate.is_none()
    }

    pub fn request(&mut self, desired: usize) -> Result<Option<(usize, Vec<IVec3>)>, String> {
        if desired >= self.fingerprints.len() || (self.displayed.is_none() && desired != 0) {
            return Err("invalid inspection stage during bootstrap".into());
        }
        if let Some(pending) = &self.pending {
            if pending.started.elapsed() > JOB_DEADLINE {
                return Err("inspection mesh worker deadline".into());
            }
            return Ok(None);
        }
        if self.candidate.as_ref().is_some_and(|c| c.stage != desired) {
            self.candidate = None;
        }
        if self.displayed == Some(desired) {
            return Ok(None);
        }
        let candidate = self.candidate.get_or_insert_with(|| Candidate {
            stage: desired,
            bootstrap: self.displayed.is_none(),
            started: Instant::now(),
            next: 0,
            meshes: Vec::new(),
            report: FineMeshReport::default(),
        });
        let chunks = if candidate.bootstrap {
            &self.all
        } else {
            &self.dirty
        };
        let count = if candidate.bootstrap {
            1
        } else {
            self.replacement_job_chunks
        };
        let end = (candidate.next + count).min(chunks.len());
        let request = chunks
            .get(candidate.next..end)
            .filter(|c| !c.is_empty())
            .ok_or("inspection cursor exhausted")?
            .to_vec();
        candidate.next = end;
        self.pending = Some(Pending {
            stage: desired,
            chunks: request.clone(),
            started: Instant::now(),
        });
        Ok(Some((desired, request)))
    }

    fn verify_identity(
        &self,
        pending: &Pending,
        world: u128,
        finishes: u128,
    ) -> Result<(), String> {
        if pending.started.elapsed() > JOB_DEADLINE {
            return Err("inspection mesh worker deadline".into());
        }
        if world != self.fingerprints[pending.stage] || finishes != self.finishes_fingerprint {
            return Err("fine mesh fingerprint mismatch".into());
        }
        Ok(())
    }

    pub fn complete(&mut self, desired: usize, job: CompletedMeshJob) -> Result<Delivery, String> {
        let pending = self.pending.take().ok_or("unsolicited mesh result")?;
        let CompletedMeshJob::Fine {
            world_fingerprint,
            finishes_fingerprint,
            result,
        } = job
        else {
            return Err("unexpected mesh kind".into());
        };
        self.verify_identity(&pending, world_fingerprint, finishes_fingerprint)?;
        if pending.stage != desired {
            if let Err(error) = result {
                eprintln!("FINE_STALE_FAILURE stage={} {error}", pending.stage);
            }
            self.candidate = None;
            return Ok(Delivery {
                meshes: Vec::new(),
                complete: None,
            });
        }
        let batch = result.map_err(|e| e.to_string())?;
        if batch.meshes.len() != pending.chunks.len()
            || batch
                .meshes
                .iter()
                .zip(&pending.chunks)
                .any(|((chunk, _), expected)| chunk != expected)
            || batch
                .meshes
                .iter()
                .map(|(_, mesh)| mesh.vertices.len())
                .sum::<usize>()
                != batch.report.vertices
            || batch
                .meshes
                .iter()
                .map(|(_, mesh)| mesh.indices.len())
                .sum::<usize>()
                != batch.report.indices
        {
            return Err("fine mesh result chunk/count mismatch".into());
        }
        let candidate = self
            .candidate
            .as_mut()
            .ok_or("missing inspection candidate")?;
        if candidate.stage != pending.stage {
            return Err("inspection candidate mismatch".into());
        }
        // Check before retaining the new mesh. Per-job extraction limits remain unchanged.
        candidate.accumulate(batch.report)?;
        let done = candidate.next
            == if candidate.bootstrap {
                self.all.len()
            } else {
                self.dirty.len()
            };
        let meshes = if candidate.bootstrap {
            batch.meshes
        } else {
            candidate
                .meshes
                .try_reserve(batch.meshes.len())
                .map_err(|_| "inspection allocation")?;
            candidate.meshes.extend(batch.meshes);
            if done {
                std::mem::take(&mut candidate.meshes)
            } else {
                Vec::new()
            }
        };
        let complete = done.then(|| {
            (
                candidate.stage,
                candidate.report,
                candidate.started.elapsed(),
            )
        });
        // Compute the final resident footprint without partially modifying resident bookkeeping.
        let mut counts = self.resident.clone();
        for (chunk, mesh) in &meshes {
            counts.insert(*chunk, (mesh.vertices.len(), mesh.indices.len()));
        }
        let (total_v, total_i) = counts
            .values()
            .fold((0_usize, 0_usize), |(v, i), (nv, ni)| (v + nv, i + ni));
        if counts.len() > MAX_SCENE_CHUNKS
            || total_v > MAX_SCENE_VERTICES
            || total_i > MAX_SCENE_INDICES
        {
            return Err("inspection resident output budget".into());
        }
        self.resident = counts;
        if done {
            self.displayed = Some(pending.stage);
            self.candidate = None;
        }
        Ok(Delivery { meshes, complete })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use destructible_fps::mesh::{
        Vertex,
        fine::{FineMeshBatch, FineMeshError},
    };
    fn stream() -> Stream {
        Stream::new(
            vec![IVec3::new(0, 0, 0), IVec3::new(1, 0, 0)],
            vec![IVec3::new(0, 0, 0), IVec3::new(1, 0, 0)],
            [1, 2, 3, 4],
            1,
            0,
        )
        .unwrap()
    }
    fn result(stage: usize, chunks: Vec<IVec3>, vertices: usize) -> CompletedMeshJob {
        let count = chunks.len();
        CompletedMeshJob::Fine {
            finishes_fingerprint: 0,
            world_fingerprint: (stage + 1) as u128,
            result: Ok(FineMeshBatch {
                meshes: chunks
                    .into_iter()
                    .map(|chunk| {
                        (
                            chunk,
                            CpuMesh {
                                vertices: vec![Vertex::zeroed(); vertices],
                                indices: Vec::new(),
                            },
                        )
                    })
                    .collect(),
                report: FineMeshReport {
                    vertices: vertices * count,
                    ..FineMeshReport::default()
                },
            }),
        }
    }
    fn bootstrap(s: &mut Stream) {
        for n in 0..2 {
            let (stage, chunk) = s.request(0).unwrap().unwrap();
            let delivery = s.complete(0, result(stage, chunk, 1)).unwrap();
            assert_eq!(delivery.meshes.len(), 1);
            assert_eq!(delivery.complete.is_some(), n == 1);
        }
        assert_eq!(s.displayed, Some(0));
    }
    use bytemuck::Zeroable;
    #[test]
    fn bootstrap_then_atomic_replacement_and_stale_discard() {
        let mut s = stream();
        assert!(s.request(1).is_err());
        bootstrap(&mut s);
        let (_, chunk) = s.request(1).unwrap().unwrap();
        assert!(s.request(1).unwrap().is_none());
        assert!(
            s.complete(1, result(1, chunk, 1))
                .unwrap()
                .meshes
                .is_empty()
        );
        assert_eq!(s.displayed, Some(0));
        let (_, chunk) = s.request(1).unwrap().unwrap();
        assert!(
            s.complete(2, result(1, chunk, 1))
                .unwrap()
                .meshes
                .is_empty()
        );
        assert_eq!(s.displayed, Some(0));
        let (_, first) = s.request(2).unwrap().unwrap();
        assert!(
            s.complete(2, result(2, first, 0))
                .unwrap()
                .meshes
                .is_empty()
        );
        let (_, second) = s.request(2).unwrap().unwrap();
        let update = s.complete(2, result(2, second, 1)).unwrap();
        assert_eq!(update.meshes.len(), 2);
        assert!(update.meshes[0].1.vertices.is_empty()); // Removed geometry must reach the GPU.
        assert_eq!(s.displayed, Some(2));
        assert!(s.request(2).unwrap().is_none());
    }
    #[test]
    fn bad_results_and_aggregate_exhaustion_do_not_publish() {
        let mut s = stream();
        assert!(s.complete(0, result(0, vec![IVec3::default()], 0)).is_err());
        bootstrap(&mut s);
        let (_, chunk) = s.request(1).unwrap().unwrap();
        assert!(s.complete(1, result(2, chunk, 1)).is_err());
        assert_eq!(s.displayed, Some(0));
        let mut s = stream();
        bootstrap(&mut s);
        let (_, chunk) = s.request(1).unwrap().unwrap();
        s.complete(1, result(1, chunk, MAX_FINE_MESH_VERTICES))
            .unwrap();
        let (_, chunk) = s.request(1).unwrap().unwrap();
        assert!(s.complete(1, result(1, chunk, 1)).is_err());
        assert_eq!(s.displayed, Some(0));
        let mut s = stream();
        bootstrap(&mut s);
        s.request(1).unwrap();
        assert!(
            s.complete(
                1,
                CompletedMeshJob::Fine {
                    finishes_fingerprint: 0,
                    world_fingerprint: 2,
                    result: Err(FineMeshError::WorkBudget)
                }
            )
            .is_err()
        );
        assert_eq!(s.displayed, Some(0));
        let mut s = stream();
        s.request(0).unwrap();
        assert!(s.complete(0, CompletedMeshJob::Bodies(Vec::new())).is_err());
        let mut s = stream();
        let (_, chunk) = s.request(0).unwrap().unwrap();
        assert!(
            s.complete(0, result(0, vec![IVec3::new(chunk[0].x + 1, 0, 0)], 0))
                .is_err()
        );
    }
    #[test]
    fn stage_and_chunk_bounds_and_worker_deadline_are_explicit() {
        assert!(Stream::new(Vec::new(), Vec::new(), [0; 4], 1, 0).is_err());
        let mut s = stream();
        assert!(s.request(4).is_err());
        s.request(0).unwrap();
        s.pending.as_mut().unwrap().started = Instant::now()
            .checked_sub(JOB_DEADLINE + Duration::from_secs(1))
            .unwrap();
        assert!(s.request(0).is_err());
        assert!(s.complete(0, result(0, vec![IVec3::default()], 0)).is_err());
    }

    #[test]
    fn stale_geometry_failure_is_reported_but_does_not_replace_the_displayed_stage() {
        let mut s = stream();
        bootstrap(&mut s);
        s.request(1).unwrap();
        let delivery = s
            .complete(
                2,
                CompletedMeshJob::Fine {
                    finishes_fingerprint: 0,
                    world_fingerprint: 2,
                    result: Err(FineMeshError::WorkBudget),
                },
            )
            .unwrap();
        assert!(delivery.meshes.is_empty());
        assert_eq!(s.displayed, Some(0));
        assert_eq!(s.request(2).unwrap().unwrap().0, 2);
    }

    #[test]
    fn resident_budget_refuses_an_otherwise_valid_dirty_candidate() {
        let mut s = stream();
        bootstrap(&mut s);
        s.resident
            .insert(IVec3::new(2, 0, 0), (MAX_SCENE_VERTICES - 2, 0));
        for n in 0..2 {
            let (_, chunk) = s.request(1).unwrap().unwrap();
            let delivery = s.complete(1, result(1, chunk, 2));
            if n == 0 {
                assert!(delivery.unwrap().meshes.is_empty());
            } else {
                assert_eq!(
                    delivery.err().as_deref(),
                    Some("inspection resident output budget")
                );
            }
        }
        assert_eq!(s.displayed, Some(0));
        assert_eq!(s.resident[&IVec3::new(0, 0, 0)], (1, 0));
    }

    #[test]
    fn replacement_batches_do_not_wait_one_frame_per_chunk() {
        let mut s = stream();
        s.replacement_job_chunks = MAX_FINE_MESH_CHUNKS;
        bootstrap(&mut s);
        let (_, chunks) = s.request(1).unwrap().unwrap();
        assert_eq!(chunks.len(), 2);
        let delivery = s.complete(1, result(1, chunks, 1)).unwrap();
        assert_eq!(delivery.meshes.len(), 2);
        assert_eq!(s.displayed, Some(1));
        assert!(s.idle());
    }

    #[test]
    fn appearance_mismatch_is_rejected_before_any_candidate_or_resident_publication() {
        let mut s = stream();
        bootstrap(&mut s);
        let before = s.resident.clone();
        let (_, chunks) = s.request(1).unwrap().unwrap();
        let mut job = result(1, chunks, 1);
        if let CompletedMeshJob::Fine {
            finishes_fingerprint,
            ..
        } = &mut job
        {
            *finishes_fingerprint = 42;
        }
        assert!(s.complete(1, job).is_err());
        assert_eq!(s.resident, before);
        assert_eq!(s.displayed, Some(0));
    }
}
