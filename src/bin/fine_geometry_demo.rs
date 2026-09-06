//! Actual Vulkan fine-geometry inspection. Authored stages, not fine multiplayer gameplay.
#![allow(clippy::cast_precision_loss)]
use destructible_fps::{
    IVec3,
    mesh::fine::{
        FineMeshBatch, FineMeshLimits,
        fixture::{STAGE_NAMES, inspection_world},
    },
    mesh_scheduler::{CompletedMeshJob, MeshScheduler},
    render::{RenderOutcome, Renderer},
    telemetry::SampleWindow,
    world::geometry::RefinedWorld,
};
use glam::Vec3;
use std::{
    error::Error,
    sync::Arc,
    time::{Duration, Instant},
};
use winit::{
    application::ApplicationHandler,
    dpi::LogicalSize,
    event::{ElementState, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    keyboard::{KeyCode, PhysicalKey},
    window::{Window, WindowId},
};

struct Scene {
    window: Arc<Window>,
    renderer: Renderer,
    worker: MeshScheduler,
    worlds: Vec<Arc<RefinedWorld>>,
    chunks: Vec<IVec3>,
    desired: usize,
    pending: Option<(usize, Instant)>,
    displayed: Option<usize>,
    presented: [usize; 4],
    started: Instant,
    smoke: Option<Duration>,
    smoke_complete: bool,
    cpu: SampleWindow,
    gpu: SampleWindow,
    mesh: SampleWindow,
    yaw: f32,
    distance: f32,
}
impl Scene {
    fn new(
        window: Arc<Window>,
        worlds: Vec<Arc<RefinedWorld>>,
        smoke: Option<Duration>,
    ) -> Result<Self, String> {
        let mut chunks: Vec<_> = worlds.iter().flat_map(|w| w.chunk_positions()).collect();
        chunks.sort_unstable();
        chunks.dedup();
        Ok(Self {
            renderer: pollster::block_on(Renderer::new(Arc::clone(&window)))?,
            window,
            worker: MeshScheduler::new(),
            worlds,
            chunks,
            desired: 0,
            pending: None,
            displayed: None,
            presented: [0; 4],
            started: Instant::now(),
            smoke,
            smoke_complete: false,
            cpu: SampleWindow::new(16384),
            gpu: SampleWindow::new(16384),
            mesh: SampleWindow::new(128),
            yaw: -0.25,
            distance: 5.5,
        })
    }

    fn frame(&mut self, event_loop: &ActiveEventLoop) -> Result<(), String> {
        let frame = Instant::now();
        if let Some(job) = self.worker.poll().map_err(|e| e.to_string())?
            && let Some(MeshDelivery {
                stage,
                start,
                result,
            }) = complete_fine_job(self.pending.take(), self.desired, &self.worlds, job)?
        {
            let world_fingerprint = self.worlds[stage].fingerprint();
            println!(
                "FINE_STAGE stage={stage} name={} fingerprint={world_fingerprint:032x} quads={} vertices={} triangles={} work={} cached_lines={} worker_ms={:.3}",
                STAGE_NAMES[stage],
                result.report.quads,
                result.report.vertices,
                result.report.indices / 3,
                result.report.work,
                result.report.lines,
                start.elapsed().as_secs_f64() * 1000.0
            );
            self.mesh.record_ms(start.elapsed().as_secs_f64() * 1000.0);
            // All requested chunks (including now-empty ones) replace the preceding stage.
            self.renderer.upload_chunk_meshes(result.meshes);
            self.displayed = Some(stage);
            self.window.set_title(&format!("Fine geometry inspection | {} | arrows: orbit/stage, W/S: zoom | not weapon simulation",STAGE_NAMES[stage]));
        }
        if self.smoke.is_some()
            && self.pending.is_none()
            && self.displayed == Some(self.desired)
            && self.presented[self.desired] >= 35
            && self.desired + 1 < STAGE_NAMES.len()
        {
            self.desired += 1;
        }
        if self.pending.is_none() && self.displayed != Some(self.desired) {
            self.worker
                .submit_fine(
                    Arc::clone(&self.worlds[self.desired]),
                    self.chunks.clone(),
                    FineMeshLimits::default(),
                )
                .map_err(|e| e.to_string())?;
            self.pending = Some((self.desired, Instant::now()));
        }
        let focus = Vec3::new(2.0, 1.4, 0.1);
        let camera = focus
            + Vec3::new(
                self.yaw.sin() * self.distance,
                0.5,
                -self.yaw.cos() * self.distance,
            );
        match self.renderer.render(
            camera,
            (focus - camera).normalize(),
            self.started.elapsed().as_secs_f32(),
        ) {
            RenderOutcome::Presented => {
                if let Some(stage) = self.displayed {
                    self.presented[stage] += 1;
                }
            }
            RenderOutcome::Skipped => {}
            RenderOutcome::Reconfigure => self.renderer.resize(self.window.inner_size())?,
            RenderOutcome::RecreateSurface => self.renderer.recreate_surface()?,
        }
        while let Some(sample) = self.renderer.take_gpu_frame_time() {
            self.gpu.record_ms(sample.total_ms);
        }
        self.cpu.record_ms(frame.elapsed().as_secs_f64() * 1000.0);
        self.finish_smoke(event_loop)
    }

    fn finish_smoke(&mut self, event_loop: &ActiveEventLoop) -> Result<(), String> {
        if self.smoke.is_some_and(|d| self.started.elapsed() >= d) {
            if self.pending.is_some() || self.presented.iter().any(|n| *n < 35) {
                return Err(format!(
                    "incomplete fine render smoke: {:?}",
                    self.presented
                ));
            }
            if self.gpu.summary().is_none() {
                return Err("no actual GPU timestamps for fine render smoke".into());
            }
            for (label, samples) in [
                ("cpu_frame_with_present", &self.cpu),
                ("gpu_total", &self.gpu),
                ("mesh_job_latency", &self.mesh),
            ] {
                if let Some(s) = samples.summary() {
                    println!("FINE_TIMING {label} {s:?}");
                }
            }
            println!(
                "FINE_PRESENTED {:?}; GPU dropped={}",
                self.presented,
                self.renderer.gpu_timing_dropped_samples()
            );
            println!(
                "smoke test graphique termine proprement (fine inspection, authored stages, not multiplayer)"
            );
            self.smoke_complete = true;
            event_loop.exit();
        }
        Ok(())
    }
}

struct MeshDelivery {
    stage: usize,
    start: Instant,
    result: FineMeshBatch,
}

fn complete_fine_job(
    pending: Option<(usize, Instant)>,
    desired: usize,
    worlds: &[Arc<RefinedWorld>],
    job: CompletedMeshJob,
) -> Result<Option<MeshDelivery>, String> {
    let (stage, start) = pending.ok_or("unsolicited mesh result")?;
    let CompletedMeshJob::Fine {
        world_fingerprint,
        result,
    } = job
    else {
        return Err("unexpected mesh kind".into());
    };
    let result = result.map_err(|e| e.to_string())?;
    let expected = worlds
        .get(stage)
        .ok_or("invalid pending inspection stage")?
        .fingerprint();
    if world_fingerprint != expected {
        return Err("fine mesh fingerprint mismatch".into());
    }
    Ok((stage == desired).then_some(MeshDelivery {
        stage,
        start,
        result,
    }))
}

struct App {
    scene: Option<Scene>,
    worlds: Option<Vec<Arc<RefinedWorld>>>,
    smoke: Option<Duration>,
    failure: Option<String>,
}
impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.scene.is_some() {
            return;
        }
        let result = event_loop
            .create_window(
                Window::default_attributes()
                    .with_title("Fine geometry | Vulkan initialization")
                    .with_inner_size(LogicalSize::new(1440.0, 900.0)),
            )
            .map_err(|e| e.to_string())
            .and_then(|w| {
                Scene::new(
                    Arc::new(w),
                    self.worlds.take().ok_or("missing inspection worlds")?,
                    self.smoke,
                )
            });
        match result {
            Ok(s) => self.scene = Some(s),
            Err(e) => {
                self.failure = Some(e);
                event_loop.exit();
            }
        }
    }
    fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        let Some(s) = &mut self.scene else {
            return;
        };
        if s.window.id() != id {
            return;
        }
        let result = match event {
            WindowEvent::CloseRequested => {
                event_loop.exit();
                Ok(())
            }
            WindowEvent::Resized(size) => s.renderer.resize(size),
            WindowEvent::RedrawRequested => s.frame(event_loop),
            WindowEvent::KeyboardInput { event, .. } => {
                if event.state == ElementState::Pressed
                    && let PhysicalKey::Code(key) = event.physical_key
                {
                    match key {
                        KeyCode::Escape => event_loop.exit(),
                        KeyCode::ArrowRight if s.smoke.is_none() => {
                            s.desired = (s.desired + 1) % STAGE_NAMES.len();
                        }
                        KeyCode::ArrowLeft if s.smoke.is_none() => {
                            s.desired = (s.desired + STAGE_NAMES.len() - 1) % STAGE_NAMES.len();
                        }
                        KeyCode::ArrowUp => s.yaw += 0.1,
                        KeyCode::ArrowDown => s.yaw -= 0.1,
                        KeyCode::KeyW => s.distance = (s.distance - 0.25).max(1.0),
                        KeyCode::KeyS => s.distance = (s.distance + 0.25).min(12.0),
                        _ => {}
                    }
                }
                Ok(())
            }
            _ => Ok(()),
        };
        if let Err(e) = result {
            self.failure = Some(e);
            event_loop.exit();
        }
    }
    fn about_to_wait(&mut self, _: &ActiveEventLoop) {
        if let Some(s) = &self.scene {
            s.window.request_redraw();
        }
    }
}

fn options(mut args: impl Iterator<Item = String>) -> Result<Option<Duration>, Box<dyn Error>> {
    let Some(flag) = args.next() else {
        return Ok(None);
    };
    if flag != "--smoke-seconds" {
        return Err("expected --smoke-seconds".into());
    }
    let seconds: u64 = args.next().ok_or("missing smoke seconds")?.parse()?;
    if args.next().is_some() || !(5..=30).contains(&seconds) {
        return Err("smoke seconds must be5..=30".into());
    }
    Ok(Some(Duration::from_secs(seconds)))
}
fn main() -> Result<(), Box<dyn Error>> {
    let smoke = options(std::env::args().skip(1))?;
    // Source authoring is done before presentation; meshing stays on the existing bounded worker.
    let worlds = (0..STAGE_NAMES.len())
        .map(|i| inspection_world(i).map(Arc::new))
        .collect::<Result<Vec<_>, _>>()?;
    let mut app = App {
        scene: None,
        worlds: Some(worlds),
        smoke,
        failure: None,
    };
    let event_loop = EventLoop::new()?;
    event_loop.set_control_flow(ControlFlow::Poll);
    event_loop.run_app(&mut app)?;
    if let Some(e) = app.failure {
        return Err(e.into());
    }
    if smoke.is_some() && app.scene.as_ref().is_none_or(|s| !s.smoke_complete) {
        return Err("fine render smoke interrupted".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use destructible_fps::mesh::fine::{FineMeshError, FineMeshReport};

    #[test]
    fn completion_discards_stale_and_refuses_unsolicited_failed_or_mismatched_results() {
        let shared = Arc::new(RefinedWorld::default());
        let worlds = vec![shared; 2];
        let job = || CompletedMeshJob::Fine {
            world_fingerprint: worlds[0].fingerprint(),
            result: Ok(FineMeshBatch {
                meshes: Vec::new(),
                report: FineMeshReport::default(),
            }),
        };
        let pending = Some((0, Instant::now()));
        assert!(
            complete_fine_job(pending, 0, &worlds, job())
                .unwrap()
                .is_some()
        );
        assert!(
            complete_fine_job(pending, 1, &worlds, job())
                .unwrap()
                .is_none()
        );
        assert!(complete_fine_job(None, 0, &worlds, job()).is_err());
        assert!(complete_fine_job(Some((2, Instant::now())), 0, &worlds, job()).is_err());
        assert!(
            complete_fine_job(pending, 0, &worlds, CompletedMeshJob::Bodies(Vec::new())).is_err()
        );
        let CompletedMeshJob::Fine { result, .. } = job() else {
            unreachable!()
        };
        assert!(
            complete_fine_job(
                pending,
                0,
                &worlds,
                CompletedMeshJob::Fine {
                    world_fingerprint: worlds[0].fingerprint() ^ 1,
                    result
                }
            )
            .is_err()
        );
        assert!(
            complete_fine_job(
                pending,
                0,
                &worlds,
                CompletedMeshJob::Fine {
                    world_fingerprint: worlds[0].fingerprint(),
                    result: Err(FineMeshError::WorkBudget)
                }
            )
            .is_err()
        );
    }
    #[test]
    fn strict_finite_smoke_options() {
        for args in [
            vec!["--smoke-seconds", "0"],
            vec!["--smoke-seconds", "31"],
            vec!["--smoke-seconds", "NaN"],
            vec!["--world", "industrial"],
            vec!["--smoke-seconds", "8", "extra"],
        ] {
            assert!(options(args.into_iter().map(str::to_owned)).is_err());
        }
        assert_eq!(
            options(["--smoke-seconds", "8"].into_iter().map(str::to_owned)).unwrap(),
            Some(Duration::from_secs(8))
        );
    }
}
