//! Actual Vulkan fine-geometry inspection. Authored stages, not fine multiplayer gameplay.
#![allow(clippy::cast_precision_loss)]
use destructible_fps::{
    mesh::fine::{
        FineMeshLimits, MAX_FINE_MESH_CHUNKS,
        fixture::{
            STAGE_NAMES, industrial_inspection_world, industrial_patch_positions, inspection_world,
        },
        hybrid_dirty_chunks,
    },
    mesh_scheduler::MeshScheduler,
    render::{RenderOutcome, Renderer},
    telemetry::SampleWindow,
    world::geometry::RefinedWorld,
};

#[path = "fine_geometry_demo/stream.rs"]
mod stream;
use stream::Stream;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum WorldKind {
    #[default]
    Inspection,
    Industrial,
}
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
    stream: Stream,
    kind: WorldKind,
    desired: usize,
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
        kind: WorldKind,
        smoke: Option<Duration>,
    ) -> Result<Self, String> {
        let mut chunks: Vec<_> = worlds.iter().flat_map(|w| w.chunk_positions()).collect();
        chunks.sort_unstable();
        chunks.dedup();
        let dirty = if kind == WorldKind::Industrial {
            let dirty =
                hybrid_dirty_chunks(&industrial_patch_positions()).map_err(|e| e.to_string())?;
            // Include empty halo chunks so a stage can remove a previously rendered surface.
            chunks.extend(&dirty);
            chunks.sort_unstable();
            chunks.dedup();
            dirty
        } else {
            chunks.clone()
        };
        let fingerprints = std::array::from_fn(|i| worlds[i].fingerprint());
        Ok(Self {
            renderer: pollster::block_on(Renderer::new(Arc::clone(&window)))?,
            window,
            worker: MeshScheduler::new(),
            worlds,
            stream: Stream::new(chunks, dirty, fingerprints, MAX_FINE_MESH_CHUNKS)?,
            kind,
            desired: 0,
            presented: [0; 4],
            started: Instant::now(),
            smoke,
            smoke_complete: false,
            cpu: SampleWindow::new(16384),
            gpu: SampleWindow::new(16384),
            mesh: SampleWindow::new(128),
            yaw: -0.25,
            distance: if kind == WorldKind::Industrial {
                8.5
            } else {
                5.5
            },
        })
    }

    fn frame(&mut self, event_loop: &ActiveEventLoop) -> Result<(), String> {
        let frame = Instant::now();
        if let Some(job) = self.worker.poll().map_err(|e| e.to_string())? {
            let delivery = self.stream.complete(self.desired, job)?;
            if !delivery.meshes.is_empty() {
                self.renderer.upload_chunk_meshes(delivery.meshes);
            }
            if let Some((stage, report, elapsed)) = delivery.complete {
                let world_fingerprint = self.worlds[stage].fingerprint();
                println!(
                    "FINE_STAGE world={:?} stage={stage} name={} fingerprint={world_fingerprint:032x} quads={} vertices={} triangles={} work={} cached_lines={} stage_latency_ms={:.3}",
                    self.kind,
                    STAGE_NAMES[stage],
                    report.quads,
                    report.vertices,
                    report.indices / 3,
                    report.work,
                    report.lines,
                    elapsed.as_secs_f64() * 1000.0
                );
                self.mesh.record_ms(elapsed.as_secs_f64() * 1000.0);
                self.window.set_title(&format!("Fine geometry {:?} | {} | arrows: orbit/stage, W/S: zoom | authored inspection, not weapon simulation", self.kind, STAGE_NAMES[stage]));
            }
        }
        if self.smoke.is_some()
            && self.stream.idle()
            && self.stream.displayed == Some(self.desired)
            && self.presented[self.desired] >= 35
            && self.desired + 1 < STAGE_NAMES.len()
        {
            self.desired += 1;
        }
        if let Some((stage, chunks)) = self.stream.request(self.desired)? {
            let world = Arc::clone(&self.worlds[stage]);
            let result = if self.kind == WorldKind::Industrial {
                self.worker
                    .submit_hybrid(world, chunks, FineMeshLimits::default())
            } else {
                self.worker
                    .submit_fine(world, chunks, FineMeshLimits::default())
            };
            result.map_err(|e| e.to_string())?;
        }
        let (focus, facing, elevation) = if self.kind == WorldKind::Industrial {
            (Vec3::new(-15.0, 2.4, 15.8), 1.0, 1.3)
        } else {
            (Vec3::new(2.0, 1.4, 0.1), -1.0, 0.5)
        };
        let camera = focus
            + Vec3::new(
                self.yaw.sin() * self.distance,
                elevation,
                facing * self.yaw.cos() * self.distance,
            );
        match self.renderer.render(
            camera,
            (focus - camera).normalize(),
            self.started.elapsed().as_secs_f32(),
        ) {
            RenderOutcome::Presented => {
                if let Some(stage) = self.stream.displayed {
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
            if !self.stream.idle() || self.presented.iter().any(|n| *n < 35) {
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
                ("mesh_stage_latency", &self.mesh),
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

struct App {
    scene: Option<Scene>,
    worlds: Option<Vec<Arc<RefinedWorld>>>,
    smoke: Option<Duration>,
    kind: WorldKind,
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
                    self.kind,
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
                        KeyCode::ArrowRight
                            if s.smoke.is_none() && s.stream.displayed.is_some() =>
                        {
                            s.desired = (s.desired + 1) % STAGE_NAMES.len();
                        }
                        KeyCode::ArrowLeft if s.smoke.is_none() && s.stream.displayed.is_some() => {
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

fn options(
    mut args: impl Iterator<Item = String>,
) -> Result<(WorldKind, Option<Duration>), Box<dyn Error>> {
    let mut kind = None;
    let mut smoke = None;
    while let Some(flag) = args.next() {
        match flag.as_str() {
            "--world" if kind.is_none() => {
                kind = Some(match args.next().as_deref() {
                    Some("inspection") => WorldKind::Inspection,
                    Some("industrial") => WorldKind::Industrial,
                    _ => return Err("world must be inspection or industrial".into()),
                });
            }
            "--smoke-seconds" if smoke.is_none() => {
                let seconds: u64 = args.next().ok_or("missing smoke seconds")?.parse()?;
                if !(5..=30).contains(&seconds) {
                    return Err("smoke seconds must be 5..=30".into());
                }
                smoke = Some(Duration::from_secs(seconds));
            }
            _ => return Err("unknown or duplicate inspection option".into()),
        }
    }
    Ok((kind.unwrap_or_default(), smoke))
}
fn main() -> Result<(), Box<dyn Error>> {
    let (kind, smoke) = options(std::env::args().skip(1))?;
    // Source authoring is done before presentation; meshing stays on the existing bounded worker.
    let worlds = (0..STAGE_NAMES.len())
        .map(|i| {
            match kind {
                WorldKind::Inspection => inspection_world(i),
                WorldKind::Industrial => industrial_inspection_world(i),
            }
            .map(Arc::new)
        })
        .collect::<Result<Vec<_>, _>>()?;
    let mut app = App {
        scene: None,
        worlds: Some(worlds),
        smoke,
        kind,
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
    #[test]
    fn strict_finite_smoke_options() {
        for args in [
            vec!["--smoke-seconds", "0"],
            vec!["--smoke-seconds", "31"],
            vec!["--smoke-seconds", "NaN"],
            vec!["--world", "unknown"],
            vec!["--world", "industrial", "--world", "inspection"],
            vec!["--world"],
            vec!["--smoke-seconds", "8", "--smoke-seconds", "8"],
            vec!["--smoke-seconds", "8", "extra"],
        ] {
            assert!(options(args.into_iter().map(str::to_owned)).is_err());
        }
        assert_eq!(
            options(["--smoke-seconds", "8"].into_iter().map(str::to_owned)).unwrap(),
            (WorldKind::Inspection, Some(Duration::from_secs(8)))
        );
        assert_eq!(
            options(
                ["--world", "industrial", "--smoke-seconds", "12"]
                    .into_iter()
                    .map(str::to_owned)
            )
            .unwrap(),
            (WorldKind::Industrial, Some(Duration::from_secs(12)))
        );
    }
}
