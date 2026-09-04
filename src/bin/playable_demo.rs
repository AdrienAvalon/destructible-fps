#![allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]

use destructible_fps::{
    DemoSession, FireMode,
    player::{MovementInput, Player},
    render::{RenderOutcome, Renderer},
};
use glam::Vec3;
use std::{collections::HashSet, error::Error, sync::Arc, time::Duration, time::Instant};
use winit::{
    application::ApplicationHandler,
    dpi::LogicalSize,
    event::{DeviceEvent, DeviceId, ElementState, MouseButton, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    keyboard::{KeyCode, PhysicalKey},
    window::{CursorGrabMode, Window, WindowId},
};

const FIXED_STEP_SECONDS: f32 = 1.0 / 120.0;

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
    last_action: String,
    showcase: bool,
}

impl Game {
    fn new(window: Arc<Window>, showcase: bool) -> Result<Self, String> {
        let mut session = DemoSession::default();
        let last_action = if showcase {
            let result = session
                .fire(Vec3::new(0.5, 3.1, 40.0), -Vec3::Z, FireMode::Explosive)
                .map_err(|error| format!("preparation showcase: {error}"))?
                .ok_or_else(|| "preparation showcase: la facade n a pas ete atteinte".to_owned())?;
            format!(
                "showcase: {} voxels fractures, {} datagrammes",
                result.report.fractured_voxels, result.datagrams
            )
        } else {
            "pret".to_owned()
        };
        let before = Instant::now();
        let renderer = pollster::block_on(Renderer::new(Arc::clone(&window), session.world()))?;
        let now = Instant::now();
        let world = session.world().stats();
        let render = renderer.stats();
        println!(
            "Monde: {} voxels solides, {} chunks; {} faces; initialisation GPU + maillage: {:.1} ms",
            world.solid_voxels,
            render.chunks,
            render.exposed_faces,
            now.duration_since(before).as_secs_f64() * 1_000.0
        );
        println!(
            "Commandes: clic pour capturer | ZQSD/WASD deplacement | Maj sprint | Espace saut | gauche tir | droit explosif | Echap libere"
        );
        Ok(Self {
            window,
            renderer,
            session,
            player: Player::default(),
            pressed: HashSet::new(),
            cursor_captured: false,
            previous_frame: now,
            started: now,
            stats_since: now,
            frames_since_stats: 0,
            accumulator: 0.0,
            last_action,
            showcase,
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
        let before = Instant::now();
        match self.session.fire(
            self.player.camera_position(),
            self.player.view_direction(),
            mode,
        ) {
            Ok(Some(result)) => {
                self.renderer
                    .rebuild_chunks(self.session.world(), &result.dirty_chunks);
                self.last_action = format!(
                    "{:?}: {} fractures + {} endommages, {} datagrammes/{:.1} KiB, remesh {:.1} ms",
                    mode,
                    result.report.fractured_voxels,
                    result.report.damaged_voxels,
                    result.datagrams,
                    result.encoded_bytes as f64 / 1_024.0,
                    before.elapsed().as_secs_f64() * 1_000.0
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

    fn redraw(&mut self, event_loop: &ActiveEventLoop, exit_after: Option<Duration>) {
        let now = Instant::now();
        self.accumulator += now
            .duration_since(self.previous_frame)
            .as_secs_f32()
            .min(0.1);
        self.previous_frame = now;
        if !self.showcase {
            let input = self.movement_input();
            let mut steps = 0;
            while self.accumulator >= FIXED_STEP_SECONDS && steps < 12 {
                self.player
                    .step(self.session.world(), input, FIXED_STEP_SECONDS);
                self.accumulator -= FIXED_STEP_SECONDS;
                steps += 1;
            }
        }

        let elapsed_seconds = now.duration_since(self.started).as_secs_f32();
        let (camera_position, view_direction) = if self.showcase {
            showcase_camera(elapsed_seconds)
        } else {
            (self.player.camera_position(), self.player.view_direction())
        };
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
                }
            }
        }

        self.frames_since_stats += 1;
        let stats_elapsed = now.duration_since(self.stats_since);
        if stats_elapsed >= Duration::from_millis(500) {
            let fps = f64::from(self.frames_since_stats) / stats_elapsed.as_secs_f64();
            let frame_ms = 1_000.0 / fps.max(0.001);
            let world = self.session.world().stats();
            self.window.set_title(&format!(
                "Destructible FPS | {fps:.0} FPS {frame_ms:.2} ms | {} voxels | {} | {}",
                world.solid_voxels,
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
            println!("smoke test graphique termine proprement");
            event_loop.exit();
        }
    }
}

struct App {
    game: Option<Game>,
    exit_after: Option<Duration>,
    showcase: bool,
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
            event_loop.exit();
            return;
        };
        match Game::new(Arc::new(window), self.showcase) {
            Ok(game) => self.game = Some(game),
            Err(error) => {
                eprintln!("initialisation impossible: {error}");
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
                }
            }
            WindowEvent::RedrawRequested => game.redraw(event_loop, self.exit_after),
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

struct LaunchOptions {
    exit_after: Option<Duration>,
    showcase: bool,
}

fn launch_options() -> Result<LaunchOptions, Box<dyn Error>> {
    let mut arguments = std::env::args().skip(1);
    let mut options = LaunchOptions {
        exit_after: None,
        showcase: false,
    };
    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--showcase" => options.showcase = true,
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
    Ok(options)
}

fn showcase_camera(elapsed_seconds: f32) -> (Vec3, Vec3) {
    let angle = elapsed_seconds.mul_add(0.105, -0.20);
    let position = Vec3::new(angle.sin() * 41.0, 11.5, angle.cos() * 41.0);
    let direction = (Vec3::new(0.0, 7.0, 0.0) - position).normalize();
    (position, direction)
}

fn main() -> Result<(), Box<dyn Error>> {
    let event_loop = EventLoop::new()?;
    event_loop.set_control_flow(ControlFlow::Poll);
    let options = launch_options()?;
    let mut app = App {
        game: None,
        exit_after: options.exit_after,
        showcase: options.showcase,
    };
    event_loop.run_app(&mut app)?;
    Ok(())
}
