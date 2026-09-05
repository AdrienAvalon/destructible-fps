#![allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]

use destructible_fps::{
    DemoSession, FireMode, FixedMicrometers3, IVec3, MICROMETERS_PER_VOXEL, Material,
    ReplicatedPlayerState, World, chunk_position,
    mesh_scheduler::{
        CompletedMeshJob, MAX_BODIES_PER_MESH_JOB, MAX_BODY_VOXELS_PER_MESH_JOB,
        MAX_CHUNKS_PER_MESH_JOB, MeshScheduler,
    },
    player::{MovementInput, Player},
    render::{RenderOutcome, Renderer},
    telemetry::{DistributionSummary, SampleWindow},
};
use glam::Vec3;
use std::{
    collections::{BTreeSet, HashSet},
    error::Error,
    sync::Arc,
    time::Duration,
    time::Instant,
};
use winit::{
    application::ApplicationHandler,
    dpi::LogicalSize,
    event::{DeviceEvent, DeviceId, ElementState, MouseButton, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    keyboard::{KeyCode, PhysicalKey},
    window::{CursorGrabMode, Window, WindowId},
};

const FIXED_STEP_SECONDS: f32 = 1.0 / 120.0;
const MAX_PENDING_MESH_CHUNKS: usize = 512;
const INITIAL_MESH_BATCH_CHUNKS: usize = 16;
const TELEMETRY_WINDOW: usize = 4_096;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum MeshPhase {
    InitialStreaming,
    Live,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum LabPhase {
    Ready,
    Triggered,
}

struct RuntimeTelemetry {
    frame_interval: SampleWindow,
    cpu_frame_work: SampleWindow,
    gpu_shadow: SampleWindow,
    gpu_world_hud: SampleWindow,
    gpu_total: SampleWindow,
}

impl RuntimeTelemetry {
    fn new() -> Self {
        Self {
            frame_interval: SampleWindow::new(TELEMETRY_WINDOW),
            cpu_frame_work: SampleWindow::new(TELEMETRY_WINDOW),
            gpu_shadow: SampleWindow::new(TELEMETRY_WINDOW),
            gpu_world_hud: SampleWindow::new(TELEMETRY_WINDOW),
            gpu_total: SampleWindow::new(TELEMETRY_WINDOW),
        }
    }

    fn record_gpu(&mut self, renderer: &mut Renderer) {
        while let Some(sample) = renderer.take_gpu_frame_time() {
            self.gpu_shadow.record_ms(sample.shadow_ms);
            self.gpu_world_hud.record_ms(sample.world_hud_ms);
            self.gpu_total.record_ms(sample.total_ms);
        }
    }

    fn print_report(&self, renderer: &Renderer) {
        println!(
            "Telemetrie de frame (fenetre bornee aux {TELEMETRY_WINDOW} derniers echantillons)"
        );
        print_distribution("intervalle redraw", self.frame_interval.summary());
        print_distribution("travail CPU frame", self.cpu_frame_work.summary());
        if renderer.gpu_timing_supported() {
            print_distribution("GPU ombres", self.gpu_shadow.summary());
            print_distribution("GPU monde + HUD", self.gpu_world_hud.summary());
            print_distribution("GPU frame totale", self.gpu_total.summary());
            println!(
                "  echantillons GPU abandonnes {:>8}",
                renderer.gpu_timing_dropped_samples()
            );
        } else {
            println!("  GPU                      timestamps indisponibles");
        }
    }
}

fn print_distribution(label: &str, summary: Option<DistributionSummary>) {
    if let Some(summary) = summary {
        println!(
            "  {label:<24} n={:>4} p50={:>6.3} ms p95={:>6.3} ms p99={:>6.3} ms max={:>6.3} ms",
            summary.samples, summary.p50_ms, summary.p95_ms, summary.p99_ms, summary.max_ms
        );
    } else {
        println!("  {label:<24} aucun echantillon");
    }
}

struct Game {
    window: Arc<Window>,
    renderer: Renderer,
    session: DemoSession,
    player: Player,
    pressed: HashSet<KeyCode>,
    cursor_captured: bool,
    previous_frame: Instant,
    started: Instant,
    stats_since: Instant,
    frames_since_stats: u32,
    accumulator: f32,
    fixed_step_index: u64,
    last_action: String,
    showcase_view: Option<ShowcaseView>,
    mesh_scheduler: MeshScheduler,
    mesh_snapshot: Arc<World>,
    mesh_phase: MeshPhase,
    pending_mesh_chunks: HashSet<IVec3>,
    pending_body_ids: BTreeSet<destructible_fps::BodyId>,
    mesh_job_in_flight: bool,
    mesh_started: Option<Instant>,
    telemetry: RuntimeTelemetry,
    lab: Option<LabPhase>,
    simulation_error: Option<String>,
}

impl Game {
    fn new(
        window: Arc<Window>,
        showcase_view: Option<ShowcaseView>,
        structural_lab: bool,
    ) -> Result<Self, String> {
        let showcase = showcase_view.is_some();
        let mut session = if structural_lab {
            let (world, config) = destructible_fps::structural_lab::structural_lab();
            DemoSession::new(world)
                .with_structural_simulation(&config)
                .map_err(|error| error.to_string())?
        } else {
            DemoSession::default()
        };
        let last_action = if showcase {
            let detached = session
                .fire(Vec3::new(0.5, 1.5, 40.0), -Vec3::Z, FireMode::Rifle)
                .map_err(|error| format!("preparation showcase structure: {error}"))?
                .ok_or_else(|| "preparation showcase: le support n a pas ete atteint".to_owned())?;
            let breach = session
                .fire(Vec3::new(0.5, 3.1, 40.0), -Vec3::Z, FireMode::Explosive)
                .map_err(|error| format!("preparation showcase: {error}"))?
                .ok_or_else(|| "preparation showcase: la facade n a pas ete atteinte".to_owned())?;
            if detached.spawned_body_ids.is_empty() {
                return Err("preparation showcase: aucun corps detache".to_owned());
            }
            format!(
                "showcase: {} voxels detaches, {} voxels de facade fractures, {} datagrammes",
                detached.report.detached_voxels,
                breach.report.fractured_voxels,
                detached.datagrams + breach.datagrams
            )
        } else {
            "pret".to_owned()
        };
        let mesh_snapshot = Arc::new(session.world().clone());
        let pending_mesh_chunks = mesh_snapshot
            .chunk_positions()
            .into_iter()
            .collect::<HashSet<_>>();
        let pending_body_ids = session.bodies().keys().copied().collect::<BTreeSet<_>>();
        let initial_chunk_count = pending_mesh_chunks.len();
        if initial_chunk_count > MAX_PENDING_MESH_CHUNKS {
            return Err(format!(
                "monde initial trop grand pour la file bornee: {initial_chunk_count} chunks, maximum {MAX_PENDING_MESH_CHUNKS}"
            ));
        }
        let before = Instant::now();
        let mut renderer = pollster::block_on(Renderer::new(Arc::clone(&window)))?;
        if showcase {
            renderer.update_player_transforms(&showcase_players(), None)?;
        }
        let now = Instant::now();
        let world = session.world().stats();
        println!(
            "Monde: {} voxels solides, {} chunks a streamer; initialisation GPU: {:.1} ms",
            world.solid_voxels,
            initial_chunk_count,
            now.duration_since(before).as_secs_f64() * 1_000.0
        );
        println!(
            "Commandes: clic pour capturer | ZQSD/WASD deplacement | Maj sprint | Espace saut | gauche tir | droit explosif | milieu construit du bois | Echap libere"
        );
        let mut player = Player::default();
        if structural_lab {
            player.position = Vec3::new(1.5, 1.01, 12.0);
            player.pitch = 0.16;
            println!(
                "LAB structure: F applique une charge partielle au point vise. Materiaux synthetiques, pas un modele calibre. Visez la racine gauche de la poutre."
            );
        }
        Ok(Self {
            window,
            renderer,
            session,
            player,
            pressed: HashSet::new(),
            cursor_captured: false,
            previous_frame: now,
            started: now,
            stats_since: now,
            frames_since_stats: 0,
            accumulator: 0.0,
            fixed_step_index: 0,
            last_action,
            showcase_view,
            mesh_scheduler: MeshScheduler::new(),
            mesh_snapshot,
            mesh_phase: MeshPhase::InitialStreaming,
            pending_mesh_chunks,
            pending_body_ids,
            mesh_job_in_flight: false,
            mesh_started: None,
            telemetry: RuntimeTelemetry::new(),
            lab: structural_lab.then_some(LabPhase::Ready),
            simulation_error: None,
        })
    }

    fn movement_input(&self) -> MovementInput {
        let pressed = |key| self.pressed.contains(&key);
        MovementInput {
            forward: axis(
                pressed(KeyCode::KeyW) || pressed(KeyCode::KeyZ),
                pressed(KeyCode::KeyS),
            ),
            right: axis(
                pressed(KeyCode::KeyD),
                pressed(KeyCode::KeyA) || pressed(KeyCode::KeyQ),
            ),
            jump: pressed(KeyCode::Space),
            sprint: pressed(KeyCode::ShiftLeft) || pressed(KeyCode::ShiftRight),
        }
    }

    fn capture_cursor(&mut self) {
        let result = self
            .window
            .set_cursor_grab(CursorGrabMode::Locked)
            .or_else(|_| self.window.set_cursor_grab(CursorGrabMode::Confined));
        if result.is_ok() {
            self.window.set_cursor_visible(false);
            self.cursor_captured = true;
        } else if let Err(error) = result {
            eprintln!("capture de souris indisponible: {error}");
        }
    }

    fn release_cursor(&mut self) {
        let _ = self.window.set_cursor_grab(CursorGrabMode::None);
        self.window.set_cursor_visible(true);
        self.cursor_captured = false;
        self.pressed.clear();
    }

    fn fire(&mut self, mode: FireMode) {
        self.fire_at(
            self.player.camera_position(),
            self.player.view_direction(),
            mode,
        );
    }

    fn fire_at(&mut self, origin: Vec3, direction: Vec3, mode: FireMode) {
        let before = Instant::now();
        match self.session.fire(origin, direction, mode) {
            Ok(Some(result)) => {
                let dirty_count = result.dirty_chunks.len();
                self.pending_body_ids.extend(&result.spawned_body_ids);
                self.mesh_snapshot = Arc::new(self.session.world().clone());
                self.queue_dirty_chunks(result.dirty_chunks);
                self.last_action = format!(
                    "{:?}: {} fractures + {} endommages + {} voxels detaches/{} corps actifs, {} datagrammes/{:.1} KiB, autorite {:.2} ms, {} chunks planifies",
                    mode,
                    result.report.fractured_voxels,
                    result.report.damaged_voxels,
                    result.report.detached_voxels,
                    result.active_bodies,
                    result.datagrams,
                    result.encoded_bytes as f64 / 1_024.0,
                    before.elapsed().as_secs_f64() * 1_000.0,
                    dirty_count
                );
                println!("{}", self.last_action);
            }
            Ok(None) => self.last_action = format!("{mode:?}: aucun impact"),
            Err(error) => {
                self.last_action = format!("erreur d autorite: {error}");
                eprintln!("{}", self.last_action);
            }
        }
    }

    fn build(&mut self, material: Material) {
        let before = Instant::now();
        match self.session.build(
            self.player.camera_position(),
            self.player.view_direction(),
            material,
        ) {
            Ok(Some(result)) => {
                let dirty_count = result.dirty_chunks.len();
                self.mesh_snapshot = Arc::new(self.session.world().clone());
                self.queue_dirty_chunks(result.dirty_chunks);
                self.last_action = format!(
                    "Construction {:?} en {:?}: {} unite(s), {} restantes, {} datagramme(s)/{:.1} KiB, autorite {:.2} ms, {} chunks planifies",
                    result.report.material,
                    result.target,
                    result.report.spent_units,
                    result.report.remaining_units,
                    result.datagrams,
                    result.encoded_bytes as f64 / 1_024.0,
                    before.elapsed().as_secs_f64() * 1_000.0,
                    dirty_count
                );
                println!("{}", self.last_action);
            }
            Ok(None) => "Construction: aucun support a portee".clone_into(&mut self.last_action),
            Err(error) => {
                self.last_action = format!("construction refusee: {error}");
                eprintln!("{}", self.last_action);
            }
        }
    }

    fn queue_dirty_chunks(&mut self, chunks: Vec<IVec3>) {
        for chunk in chunks {
            if self.pending_mesh_chunks.contains(&chunk) {
                continue;
            }
            if self.pending_mesh_chunks.len() >= MAX_PENDING_MESH_CHUNKS {
                "file de remeshing saturee; rendu volontairement bloque"
                    .clone_into(&mut self.last_action);
                break;
            }
            self.pending_mesh_chunks.insert(chunk);
        }
    }

    fn pump_meshing(&mut self, focus: Vec3) {
        match self.mesh_scheduler.poll() {
            Ok(Some(completed)) => self.finish_mesh_job(completed),
            Ok(None) => {}
            Err(error) => {
                self.mesh_job_in_flight = true;
                self.last_action = format!("worker de remeshing arrete: {error}");
            }
        }

        if !self.mesh_job_in_flight
            && self.pending_mesh_chunks.is_empty()
            && self.pending_body_ids.is_empty()
        {
            if self.mesh_phase == MeshPhase::InitialStreaming {
                self.mesh_phase = MeshPhase::Live;
                self.last_action = format!(
                    "streaming initial termine en {:.1} ms: {} chunks, {} faces",
                    self.started.elapsed().as_secs_f64() * 1_000.0,
                    self.renderer.stats().chunks,
                    self.renderer.stats().exposed_faces
                );
                println!("{}", self.last_action);
            }
            return;
        }
        if self.mesh_job_in_flight {
            return;
        }
        if self.queue_body_mesh_job() {
            return;
        }
        let batch_limit = if self.mesh_phase == MeshPhase::InitialStreaming {
            INITIAL_MESH_BATCH_CHUNKS
        } else {
            MAX_CHUNKS_PER_MESH_JOB
        };
        let focus_chunk = world_position_to_chunk(focus);
        let chunks = prioritized_chunks(&self.pending_mesh_chunks, focus_chunk, batch_limit);
        match self
            .mesh_scheduler
            .submit(Arc::clone(&self.mesh_snapshot), chunks.clone())
        {
            Ok(()) => {
                for chunk in chunks {
                    self.pending_mesh_chunks.remove(&chunk);
                }
                self.mesh_job_in_flight = true;
                self.mesh_started = Some(Instant::now());
            }
            Err(error) => self.last_action = format!("remeshing non planifie: {error}"),
        }
    }

    fn finish_mesh_job(&mut self, completed: CompletedMeshJob) {
        self.mesh_job_in_flight = false;
        let elapsed_ms = self
            .mesh_started
            .take()
            .map_or(0.0, |started| started.elapsed().as_secs_f64() * 1_000.0);
        match completed {
            CompletedMeshJob::Chunks {
                world_fingerprint,
                meshes,
            } => {
                if world_fingerprint == self.session.world().fingerprint() {
                    let count = meshes.len();
                    self.renderer.upload_chunk_meshes(meshes);
                    self.last_action =
                        format!("remeshing asynchrone: {count} chunks en {elapsed_ms:.2} ms");
                } else {
                    let stale_chunks = meshes.into_iter().map(|(chunk, _mesh)| chunk).collect();
                    self.queue_dirty_chunks(stale_chunks);
                }
            }
            CompletedMeshJob::Bodies(meshes) => {
                let count = meshes.len();
                if let Err(error) = self.renderer.upload_body_meshes(meshes) {
                    self.last_action = format!("upload de corps refuse: {error}");
                } else {
                    self.renderer
                        .update_body_transforms(self.session.body_states());
                    self.last_action =
                        format!("maillage asynchrone: {count} corps en {elapsed_ms:.2} ms");
                }
            }
        }
    }

    fn queue_body_mesh_job(&mut self) -> bool {
        if self.pending_body_ids.is_empty() {
            return false;
        }
        let mut bodies = Vec::new();
        let mut body_ids = Vec::new();
        let mut voxel_count = 0_usize;
        for &body_id in &self.pending_body_ids {
            let Some(body) = self.session.bodies().get(&body_id) else {
                continue;
            };
            if bodies.len() == MAX_BODIES_PER_MESH_JOB
                || voxel_count.saturating_add(body.voxels.len()) > MAX_BODY_VOXELS_PER_MESH_JOB
            {
                break;
            }
            voxel_count += body.voxels.len();
            bodies.push(body.clone());
            body_ids.push(body_id);
        }
        match self.mesh_scheduler.submit_bodies(bodies) {
            Ok(()) => {
                for body_id in body_ids {
                    self.pending_body_ids.remove(&body_id);
                }
                self.mesh_job_in_flight = true;
                self.mesh_started = Some(Instant::now());
            }
            Err(error) => self.last_action = format!("maillage de corps non planifie: {error}"),
        }
        true
    }

    fn advance_simulation(&mut self, frame_interval: Duration) {
        self.accumulator += frame_interval.as_secs_f32().min(0.1);
        let input = self.movement_input();
        let mut steps = 0;
        while self.accumulator >= FIXED_STEP_SECONDS && steps < 12 {
            if self.showcase_view.is_none() {
                self.player
                    .step(self.session.world(), input, FIXED_STEP_SECONDS);
            }
            self.fixed_step_index = self.fixed_step_index.wrapping_add(1);
            if self.fixed_step_index.is_multiple_of(2) {
                match self.session.tick() {
                    Ok(report) => {
                        if !report.dirty_chunks.is_empty() {
                            self.mesh_snapshot = Arc::new(self.session.world().clone());
                            self.queue_dirty_chunks(report.dirty_chunks);
                        }
                        self.pending_body_ids.extend(report.spawned_body_ids);
                        if report.physics.updated_bodies > 0 {
                            self.renderer
                                .update_body_transforms(self.session.body_states());
                        }
                    }
                    Err(error) => {
                        self.last_action = format!("erreur physique autoritaire: {error}");
                        self.simulation_error = Some(self.last_action.clone());
                    }
                }
            }
            self.accumulator -= FIXED_STEP_SECONDS;
            steps += 1;
        }
    }

    fn redraw(
        &mut self,
        event_loop: &ActiveEventLoop,
        exit_after: Option<Duration>,
    ) -> Option<String> {
        let cpu_frame_started = Instant::now();
        let now = Instant::now();
        let frame_interval = now.duration_since(self.previous_frame);
        self.telemetry
            .frame_interval
            .record_ms(frame_interval.as_secs_f64() * 1_000.0);
        self.previous_frame = now;
        self.advance_simulation(frame_interval);
        let elapsed_seconds = now.duration_since(self.started).as_secs_f32();
        if self.lab == Some(LabPhase::Ready)
            && exit_after.is_some()
            && elapsed_seconds >= 1.0
            && self.session.structural_status().assessed > 0
        {
            self.lab = Some(LabPhase::Triggered);
            println!("LAB charge automatique a {elapsed_seconds:.3} secondes");
            self.fire_at(Vec3::new(1.5, 4.5, 12.0), -Vec3::Z, FireMode::TestCharge);
        }
        let (camera_position, view_direction) = match self.showcase_view {
            Some(ShowcaseView::Orbit) => showcase_camera(elapsed_seconds),
            Some(ShowcaseView::BreachCloseup) => breach_closeup_camera(elapsed_seconds),
            None => (self.player.camera_position(), self.player.view_direction()),
        };
        self.pump_meshing(camera_position);
        match self
            .renderer
            .render(camera_position, view_direction, elapsed_seconds)
        {
            RenderOutcome::Presented | RenderOutcome::Skipped => {}
            RenderOutcome::Reconfigure => {
                self.renderer.resize(self.window.inner_size());
            }
            RenderOutcome::RecreateSurface => {
                if let Err(error) = self.renderer.recreate_surface() {
                    eprintln!("surface Vulkan perdue: {error}");
                    event_loop.exit();
                    return Some(format!("surface Vulkan perdue: {error}"));
                }
            }
        }
        self.telemetry.record_gpu(&mut self.renderer);
        self.telemetry
            .cpu_frame_work
            .record_ms(cpu_frame_started.elapsed().as_secs_f64() * 1_000.0);

        self.frames_since_stats += 1;
        let stats_elapsed = now.duration_since(self.stats_since);
        if stats_elapsed >= Duration::from_millis(500) {
            let fps = f64::from(self.frames_since_stats) / stats_elapsed.as_secs_f64();
            let frame_ms = 1_000.0 / fps.max(0.001);
            let world = self.session.world().stats();
            let render = self.renderer.stats();
            let structural = self.session.structural_status();
            let structural_label = if self.lab.is_some() {
                format!(
                    " | structure: {} attente, actif={}, {} ruptures, {} echecs, incomplete={}",
                    structural.queued,
                    structural.busy,
                    structural.committed,
                    structural.failed,
                    structural.incomplete()
                )
            } else {
                String::new()
            };
            self.window.set_title(&format!(
                "Destructible FPS | {fps:.0} FPS {frame_ms:.2} ms | {} voxels | {}/{} chunks + {}/{} corps + {} joueurs | {} | {}{structural_label}",
                world.solid_voxels,
                render.visible_chunks,
                render.chunks,
                render.visible_bodies,
                render.bodies,
                render.visible_players,
                if self.cursor_captured {
                    "souris capturee"
                } else {
                    "cliquez pour jouer"
                },
                self.last_action
            ));
            self.frames_since_stats = 0;
            self.stats_since = now;
        }
        if exit_after.is_some_and(|duration| now.duration_since(self.started) >= duration) {
            return self.finish_smoke(event_loop);
        }
        None
    }

    fn finish_smoke(&self, event_loop: &ActiveEventLoop) -> Option<String> {
        self.telemetry.print_report(&self.renderer);
        if let Some(error) = &self.simulation_error {
            eprintln!("{error}");
            event_loop.exit();
            return Some(error.clone());
        }
        let render = self.renderer.stats();
        println!(
            "Culling: {}/{} chunks, {}/{} corps et {}/{} joueurs visibles; {} draws monde, {} draws ombres",
            render.visible_chunks,
            render.chunks,
            render.visible_bodies,
            render.bodies,
            render.visible_players,
            render.players,
            render.world_draw_calls,
            render.shadow_draw_calls
        );
        let sleeping_bodies = self
            .session
            .body_states()
            .values()
            .filter(|state| state.sleeping)
            .count();
        println!(
            "Physique: {sleeping_bodies}/{} corps endormis",
            self.session.body_states().len()
        );
        let structural = self.session.structural_status();
        if self.lab.is_some() {
            println!(
                "LAB structure: {structural:?}, erreur={:?}",
                self.session.structural_error()
            );
        }
        let failure = if self.lab.is_some()
            && (self.lab != Some(LabPhase::Triggered)
                || structural.committed != 1
                || structural.failed != 0
                || structural.overflowed
                || structural.stopped
                || structural.busy
                || structural.queued != 0
                || render.bodies != 2
                || sleeping_bodies != 2
                || !self.pending_mesh_chunks.is_empty()
                || self.mesh_job_in_flight)
        {
            Some("lab structure incomplet: charge, rupture, replication, maillages ou chute non valides avant la limite du smoke".to_owned())
        } else if self.mesh_phase != MeshPhase::Live {
            Some(format!(
                "streaming initial incomplet: {} chunks en attente, worker actif={}",
                self.pending_mesh_chunks.len(),
                self.mesh_job_in_flight
            ))
        } else if render.bodies != self.session.bodies().len() {
            Some(format!(
                "rendu de corps incomplet: {} corps GPU pour {} corps autoritaires",
                render.bodies,
                self.session.bodies().len()
            ))
        } else if self.showcase_view.is_some() && render.players != showcase_players().len() {
            Some(format!(
                "rendu joueur incomplet: {} avatars GPU pour {} attendus",
                render.players,
                showcase_players().len()
            ))
        } else if self.showcase_view.is_some()
            && sleeping_bodies != self.session.body_states().len()
        {
            Some(format!(
                "simulation physique incomplete: {sleeping_bodies}/{} corps endormis",
                self.session.body_states().len()
            ))
        } else {
            println!("smoke test graphique termine proprement");
            None
        };
        if let Some(error) = &failure {
            eprintln!("{error}");
        }
        event_loop.exit();
        failure
    }
}

struct App {
    game: Option<Game>,
    exit_after: Option<Duration>,
    showcase_view: Option<ShowcaseView>,
    failure: Option<String>,
    structural_lab: bool,
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.game.is_some() {
            return;
        }
        let attributes = Window::default_attributes()
            .with_title("Destructible FPS | initialisation Vulkan")
            .with_inner_size(LogicalSize::new(1440.0, 900.0))
            .with_min_inner_size(LogicalSize::new(800.0, 500.0));
        let Ok(window) = event_loop.create_window(attributes) else {
            eprintln!("impossible de creer la fenetre");
            self.failure = Some("impossible de creer la fenetre".to_owned());
            event_loop.exit();
            return;
        };
        match Game::new(Arc::new(window), self.showcase_view, self.structural_lab) {
            Ok(game) => self.game = Some(game),
            Err(error) => {
                eprintln!("initialisation impossible: {error}");
                self.failure = Some(error);
                event_loop.exit();
            }
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
    ) {
        let Some(game) = &mut self.game else {
            return;
        };
        if window_id != game.window.id() {
            return;
        }
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => game.renderer.resize(size),
            WindowEvent::Focused(false) => game.release_cursor(),
            WindowEvent::KeyboardInput { event, .. } => {
                if let PhysicalKey::Code(code) = event.physical_key {
                    match event.state {
                        ElementState::Pressed => {
                            game.pressed.insert(code);
                            if code == KeyCode::KeyF
                                && game.lab.is_some()
                                && game.cursor_captured
                                && !event.repeat
                            {
                                game.fire(FireMode::TestCharge);
                            }
                            if code == KeyCode::Escape {
                                if game.cursor_captured {
                                    game.release_cursor();
                                } else {
                                    event_loop.exit();
                                }
                            }
                        }
                        ElementState::Released => {
                            game.pressed.remove(&code);
                        }
                    }
                }
            }
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button,
                ..
            } => {
                if !game.cursor_captured {
                    game.capture_cursor();
                } else if button == MouseButton::Left {
                    game.fire(FireMode::Rifle);
                } else if button == MouseButton::Right {
                    game.fire(FireMode::Explosive);
                } else if button == MouseButton::Middle {
                    game.build(Material::Wood);
                }
            }
            WindowEvent::RedrawRequested => {
                if let Some(error) = game.redraw(event_loop, self.exit_after) {
                    self.failure = Some(error);
                }
            }
            _ => {}
        }
    }

    fn device_event(
        &mut self,
        _event_loop: &ActiveEventLoop,
        _device_id: DeviceId,
        event: DeviceEvent,
    ) {
        if let Some(game) = &mut self.game
            && game.cursor_captured
            && let DeviceEvent::MouseMotion { delta } = event
        {
            game.player.look(delta.0, delta.1);
        }
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(game) = &self.game {
            game.window.request_redraw();
        }
    }
}

fn axis(positive: bool, negative: bool) -> f32 {
    f32::from(u8::from(positive)) - f32::from(u8::from(negative))
}

const fn world_position_to_chunk(position: Vec3) -> IVec3 {
    chunk_position(IVec3::new(
        position.x.floor() as i32,
        position.y.floor() as i32,
        position.z.floor() as i32,
    ))
}

fn prioritized_chunks(pending: &HashSet<IVec3>, focus: IVec3, limit: usize) -> Vec<IVec3> {
    let mut chunks: Vec<_> = pending.iter().copied().collect();
    chunks.sort_unstable_by_key(|chunk| (chunk.squared_distance(focus), *chunk));
    chunks.truncate(limit);
    chunks
}

struct LaunchOptions {
    exit_after: Option<Duration>,
    showcase_view: Option<ShowcaseView>,
    structural_lab: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ShowcaseView {
    Orbit,
    BreachCloseup,
}

fn launch_options() -> Result<LaunchOptions, Box<dyn Error>> {
    let mut arguments = std::env::args().skip(1);
    let mut options = LaunchOptions {
        exit_after: None,
        showcase_view: None,
        structural_lab: false,
    };
    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--structural-lab" => options.structural_lab = true,
            "--showcase" => options.showcase_view = Some(ShowcaseView::Orbit),
            "--showcase-closeup" => options.showcase_view = Some(ShowcaseView::BreachCloseup),
            "--smoke-seconds" => {
                let seconds: f64 = arguments
                    .next()
                    .ok_or("--smoke-seconds exige une duree")?
                    .parse()?;
                if !seconds.is_finite() || seconds <= 0.0 {
                    return Err("duree de smoke test invalide".into());
                }
                options.exit_after = Some(Duration::from_secs_f64(seconds));
            }
            _ => return Err(format!("argument inconnu: {argument}").into()),
        }
    }
    if options.structural_lab && options.showcase_view.is_some() {
        return Err("--structural-lab est incompatible avec --showcase".into());
    }
    Ok(options)
}

fn showcase_camera(elapsed_seconds: f32) -> (Vec3, Vec3) {
    let angle = elapsed_seconds.mul_add(0.105, -0.20);
    let position = Vec3::new(angle.sin() * 41.0, 11.5, angle.cos() * 41.0);
    let direction = (Vec3::new(0.0, 7.0, 0.0) - position).normalize();
    (position, direction)
}

fn breach_closeup_camera(elapsed_seconds: f32) -> (Vec3, Vec3) {
    let drift = (elapsed_seconds * 0.16).sin() * 0.45;
    let position = Vec3::new(8.2 + drift, 6.4, 28.0);
    let direction = (Vec3::new(-0.5, 4.4, 15.7) - position).normalize();
    (position, direction)
}

fn showcase_players() -> [ReplicatedPlayerState; 2] {
    [
        showcase_player(101, -4, 1, 8),
        showcase_player(103, 5, 1, 5),
    ]
}

fn showcase_player(session_id: u64, x: i64, y: i64, z: i64) -> ReplicatedPlayerState {
    ReplicatedPlayerState {
        session_id,
        position_um: FixedMicrometers3 {
            x: x * MICROMETERS_PER_VOXEL,
            y: y * MICROMETERS_PER_VOXEL,
            z: z * MICROMETERS_PER_VOXEL,
        },
        velocity_um_per_second: FixedMicrometers3::default(),
        integration_remainder: [0; 3],
        grounded: true,
        last_input_sequence: 0,
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    let event_loop = EventLoop::new()?;
    event_loop.set_control_flow(ControlFlow::Poll);
    let options = launch_options()?;
    let mut app = App {
        game: None,
        exit_after: options.exit_after,
        showcase_view: options.showcase_view,
        failure: None,
        structural_lab: options.structural_lab,
    };
    event_loop.run_app(&mut app)?;
    if let Some(error) = app.failure {
        return Err(error.into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn world_position_maps_to_negative_chunk_with_euclidean_division() {
        assert_eq!(
            world_position_to_chunk(Vec3::new(-0.1, 16.0, -16.1)),
            IVec3::new(-1, 1, -2)
        );
    }

    #[test]
    fn streaming_batch_selects_nearest_chunks_deterministically() {
        let pending = [
            IVec3::new(8, 0, 0),
            IVec3::new(1, 0, 0),
            IVec3::new(-1, 0, 0),
        ]
        .into_iter()
        .collect();

        assert_eq!(
            prioritized_chunks(&pending, IVec3::default(), 2),
            vec![IVec3::new(-1, 0, 0), IVec3::new(1, 0, 0)]
        );
    }

    #[test]
    fn breach_closeup_is_finite_normalized_and_closer_than_the_orbit() {
        let (close_position, close_direction) = breach_closeup_camera(3.0);
        let (orbit_position, _orbit_direction) = showcase_camera(3.0);

        assert!(close_position.is_finite());
        assert!(close_direction.is_finite());
        assert!((close_direction.length() - 1.0).abs() < 0.000_001);
        assert!(close_position.length() < orbit_position.length());
    }
}
