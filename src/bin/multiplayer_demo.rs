#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss
)]

use destructible_fps::{
    ClientPrediction, ClientPredictionError, FixedMicrometers3, MAX_APPLICATION_DATAGRAM_BYTES,
    MAX_RECEIVED_DATAGRAMS_PER_TICK, MICROMETERS_PER_VOXEL, PlayerInputCommand,
    PlayerInterpolationBuffer, PlayerInterpolationError, PlayerStateReceiveError,
    ServerControlMessage, World, decode_player_state_packet, decode_server_control, demo_world,
    encode_client_hello, encode_player_input, is_player_state_datagram,
    mesh::mesh_chunk,
    player::Player,
    render::{RenderOutcome, Renderer},
};
use glam::Vec3;
use std::{
    collections::HashSet,
    error::Error,
    io,
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, UdpSocket},
    sync::Arc,
    time::{Duration, Instant},
};
use winit::{
    application::ApplicationHandler,
    dpi::LogicalSize,
    event::{DeviceEvent, DeviceId, ElementState, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    keyboard::{KeyCode, PhysicalKey},
    window::{CursorGrabMode, Window, WindowId},
};

const FIXED_STEP_SECONDS: f32 = 1.0 / 60.0;
const DEFAULT_SERVER: &str = "127.0.0.1:40000";

struct MultiplayerGame {
    window: Arc<Window>,
    renderer: Renderer,
    world: World,
    socket: UdpSocket,
    server: SocketAddr,
    nonce: u64,
    session_id: Option<u64>,
    prediction: Option<ClientPrediction>,
    interpolation: PlayerInterpolationBuffer,
    latest_state_received_at: Option<Instant>,
    next_input_sequence: u64,
    view: Player,
    pressed: HashSet<KeyCode>,
    cursor_captured: bool,
    previous_frame: Instant,
    started: Instant,
    accumulator: f32,
    last_hello_at: Instant,
    last_status: String,
    smoke_motion: bool,
    initial_position_um: Option<FixedMicrometers3>,
    maximum_horizontal_displacement_um: u64,
}

impl MultiplayerGame {
    fn new(window: Arc<Window>, server: SocketAddr, smoke_motion: bool) -> Result<Self, String> {
        let bind = match server.ip() {
            IpAddr::V4(_) => SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0),
            IpAddr::V6(_) => SocketAddr::new(IpAddr::V6(Ipv6Addr::LOCALHOST), 0),
        };
        let socket = UdpSocket::bind(bind).map_err(|error| format!("socket client: {error}"))?;
        socket
            .set_nonblocking(true)
            .map_err(|error| format!("socket non bloquante: {error}"))?;
        let local_port = u64::from(
            socket
                .local_addr()
                .map_err(|error| format!("adresse client: {error}"))?
                .port(),
        );
        let nonce = (u64::from(std::process::id()) << 16 | local_port).max(1);
        socket
            .send_to(&encode_client_hello(nonce), server)
            .map_err(|error| format!("handshake initial: {error}"))?;

        let world = demo_world();
        let meshes = world
            .chunk_positions()
            .into_iter()
            .map(|chunk| (chunk, mesh_chunk(&world, chunk)))
            .collect();
        let mut renderer = pollster::block_on(Renderer::new(Arc::clone(&window)))?;
        renderer.upload_chunk_meshes(meshes);
        let now = Instant::now();
        println!(
            "Client multijoueur loopback {} -> {server}; {} chunks charges",
            socket
                .local_addr()
                .map_err(|error| format!("adresse client: {error}"))?,
            renderer.stats().chunks
        );
        println!("Commandes: clic pour capturer | ZQSD/WASD | Maj sprint | Espace saut | Echap");
        Ok(Self {
            window,
            renderer,
            world,
            socket,
            server,
            nonce,
            session_id: None,
            prediction: None,
            interpolation: PlayerInterpolationBuffer::default(),
            latest_state_received_at: None,
            next_input_sequence: 1,
            view: Player::default(),
            pressed: HashSet::new(),
            cursor_captured: false,
            previous_frame: now,
            started: now,
            accumulator: 0.0,
            last_hello_at: now,
            last_status: "connexion au serveur".to_owned(),
            smoke_motion,
            initial_position_um: None,
            maximum_horizontal_displacement_um: 0,
        })
    }

    fn capture_cursor(&mut self) {
        let result = self
            .window
            .set_cursor_grab(CursorGrabMode::Locked)
            .or_else(|_| self.window.set_cursor_grab(CursorGrabMode::Confined));
        if result.is_ok() {
            self.window.set_cursor_visible(false);
            self.cursor_captured = true;
        }
    }

    fn release_cursor(&mut self) {
        let _result = self.window.set_cursor_grab(CursorGrabMode::None);
        self.window.set_cursor_visible(true);
        self.cursor_captured = false;
        self.pressed.clear();
    }

    fn pump_network(&mut self) -> Result<(), String> {
        let mut bytes = [0_u8; MAX_APPLICATION_DATAGRAM_BYTES + 1];
        for _ in 0..MAX_RECEIVED_DATAGRAMS_PER_TICK {
            let (length, source) = match self.socket.recv_from(&mut bytes) {
                Ok(received) => received,
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
                Err(error) => return Err(format!("reception multijoueur: {error}")),
            };
            if source != self.server {
                continue;
            }
            let payload = &bytes[..length];
            if is_player_state_datagram(payload) {
                self.receive_player_state(payload)?;
            } else if payload.starts_with(b"DFCT") {
                self.receive_control(payload)?;
            }
        }
        if self.session_id.is_none() && self.last_hello_at.elapsed() >= Duration::from_secs(1) {
            self.socket
                .send_to(&encode_client_hello(self.nonce), self.server)
                .map_err(|error| format!("nouvelle tentative de handshake: {error}"))?;
            self.last_hello_at = Instant::now();
        }
        Ok(())
    }

    fn receive_control(&mut self, payload: &[u8]) -> Result<(), String> {
        let message = decode_server_control(payload)
            .map_err(|error| format!("reponse de handshake invalide: {error}"))?;
        match message {
            ServerControlMessage::Welcome { nonce, session_id } if nonce == self.nonce => {
                if self.session_id.is_some_and(|current| current != session_id) {
                    return Err("le serveur a remplace une session active".to_owned());
                }
                self.session_id = Some(session_id);
                self.last_status = format!("session {session_id} etablie");
                Ok(())
            }
            ServerControlMessage::Welcome { .. } => {
                Err("le serveur a repondu avec un nonce inattendu".to_owned())
            }
        }
    }

    fn receive_player_state(&mut self, payload: &[u8]) -> Result<(), String> {
        let packet = decode_player_state_packet(payload)
            .map_err(|error| format!("etat joueur invalide: {error}"))?;
        match self.interpolation.push(payload) {
            Ok(_report) => self.latest_state_received_at = Some(Instant::now()),
            Err(PlayerInterpolationError::Receive(PlayerStateReceiveError::StaleServerTick {
                ..
            })) => return Ok(()),
            Err(error) => return Err(format!("historique joueur invalide: {error}")),
        }
        let Some(session_id) = self.session_id else {
            return Ok(());
        };
        let Some(authoritative) = packet
            .players
            .iter()
            .find(|player| player.session_id == session_id)
            .copied()
        else {
            return Err(format!(
                "session locale {session_id} absente de la vue serveur"
            ));
        };
        if let Some(prediction) = &mut self.prediction {
            match prediction.reconcile(packet.server_tick, authoritative, &self.world) {
                Ok(report) => {
                    self.last_status = format!(
                        "tick {} | ack {} | rejeu {}",
                        report.server_tick,
                        authoritative.last_input_sequence,
                        report.replayed_inputs
                    );
                }
                Err(ClientPredictionError::StaleServerTick { .. }) => return Ok(()),
                Err(error) => return Err(format!("reconciliation refusee: {error}")),
            }
        } else {
            self.next_input_sequence = authoritative
                .last_input_sequence
                .checked_add(1)
                .ok_or_else(|| "sequence joueur epuisee".to_owned())?;
            self.prediction = Some(
                ClientPrediction::new(packet.server_tick, authoritative)
                    .map_err(|error| format!("prediction initiale: {error}"))?,
            );
        }
        self.sync_local_view();
        Ok(())
    }

    fn fixed_tick(&mut self) -> Result<(), String> {
        let Some(session_id) = self.session_id else {
            return Ok(());
        };
        let mut input = movement_command(self.next_input_sequence, self.view.yaw, &self.pressed);
        if self.smoke_motion {
            if session_id.is_multiple_of(2) {
                input.movement_x_per_mille = 0;
                input.movement_z_per_mille = -1_000;
            } else {
                input.movement_x_per_mille = 1_000;
                input.movement_z_per_mille = 0;
            }
            input.sprint = true;
        }
        let Some(prediction) = &mut self.prediction else {
            return Ok(());
        };
        prediction
            .predict(input, &self.world)
            .map_err(|error| format!("prediction locale refusee: {error}"))?;
        self.socket
            .send_to(&encode_player_input(session_id, input), self.server)
            .map_err(|error| format!("envoi input: {error}"))?;
        self.next_input_sequence = self
            .next_input_sequence
            .checked_add(1)
            .ok_or_else(|| "sequence joueur epuisee".to_owned())?;
        self.sync_local_view();
        Ok(())
    }

    fn sync_local_view(&mut self) {
        let Some(prediction) = &self.prediction else {
            return;
        };
        let position_um = prediction.state().position_um;
        let initial = *self.initial_position_um.get_or_insert(position_um);
        self.maximum_horizontal_displacement_um = self.maximum_horizontal_displacement_um.max(
            position_um
                .x
                .abs_diff(initial.x)
                .max(position_um.z.abs_diff(initial.z)),
        );
        self.view.position = fixed_to_world(position_um);
        self.view.velocity = fixed_to_world(prediction.state().velocity_um_per_second);
    }

    fn update_remote_players(&mut self) -> Result<(), String> {
        let (Some(base_tick), Some(received_at)) = (
            self.interpolation.delayed_target_tick(),
            self.latest_state_received_at,
        ) else {
            return Ok(());
        };
        let elapsed_ticks = received_at.elapsed().as_secs_f64() * 60.0;
        let whole_ticks = elapsed_ticks.floor().max(0.0) as u64;
        let subtick = ((elapsed_ticks.fract() * 1_000.0).round() as u16).min(999);
        let sample = self
            .interpolation
            .sample(base_tick.saturating_add(whole_ticks), subtick)
            .map_err(|error| format!("interpolation distante: {error}"))?;
        self.renderer
            .update_player_transforms(&sample.players, self.session_id)
    }

    fn redraw(
        &mut self,
        event_loop: &ActiveEventLoop,
        exit_after: Option<Duration>,
    ) -> Result<(), String> {
        self.pump_network()?;
        let now = Instant::now();
        self.accumulator += now
            .duration_since(self.previous_frame)
            .as_secs_f32()
            .min(0.1);
        self.previous_frame = now;
        let mut steps = 0;
        while self.accumulator >= FIXED_STEP_SECONDS && steps < 6 {
            self.fixed_tick()?;
            self.accumulator -= FIXED_STEP_SECONDS;
            steps += 1;
        }
        self.update_remote_players()?;
        match self.renderer.render(
            self.view.camera_position(),
            self.view.view_direction(),
            now.duration_since(self.started).as_secs_f32(),
        ) {
            RenderOutcome::Presented | RenderOutcome::Skipped => {}
            RenderOutcome::Reconfigure => self.renderer.resize(self.window.inner_size()),
            RenderOutcome::RecreateSurface => self.renderer.recreate_surface()?,
        }
        let stats = self.renderer.stats();
        self.window.set_title(&format!(
            "Destructible FPS multijoueur | session {} | {} joueurs | {}",
            self.session_id
                .map_or_else(|| "...".to_owned(), |id| id.to_string()),
            stats
                .players
                .saturating_add(usize::from(self.session_id.is_some())),
            self.last_status
        ));
        if exit_after.is_some_and(|duration| self.started.elapsed() >= duration) {
            if self.session_id.is_none() || self.prediction.is_none() {
                return Err("smoke multijoueur termine sans session jouable".to_owned());
            }
            if self.maximum_horizontal_displacement_um < MICROMETERS_PER_VOXEL.cast_unsigned() {
                return Err(format!(
                    "smoke multijoueur sans mouvement autoritaire suffisant: {} um",
                    self.maximum_horizontal_displacement_um
                ));
            }
            println!(
                "SMOKE session={} joueurs_distants={} tick={} pending={} deplacement_um={}",
                self.session_id.unwrap_or_default(),
                stats.players,
                self.prediction
                    .as_ref()
                    .map_or(0, ClientPrediction::last_server_tick),
                self.prediction
                    .as_ref()
                    .map_or(0, ClientPrediction::pending_inputs),
                self.maximum_horizontal_displacement_um
            );
            event_loop.exit();
        }
        Ok(())
    }
}

fn fixed_to_world(value: FixedMicrometers3) -> Vec3 {
    let scale = MICROMETERS_PER_VOXEL as f32;
    Vec3::new(
        value.x as f32 / scale,
        value.y as f32 / scale,
        value.z as f32 / scale,
    )
}

fn movement_command(
    input_sequence: u64,
    yaw: f32,
    pressed: &HashSet<KeyCode>,
) -> PlayerInputCommand {
    let axis = |positive, negative| f32::from(u8::from(positive)) - f32::from(u8::from(negative));
    let forward_input = axis(
        pressed.contains(&KeyCode::KeyW) || pressed.contains(&KeyCode::KeyZ),
        pressed.contains(&KeyCode::KeyS),
    );
    let right_input = axis(
        pressed.contains(&KeyCode::KeyD),
        pressed.contains(&KeyCode::KeyA) || pressed.contains(&KeyCode::KeyQ),
    );
    let forward = Vec3::new(yaw.sin(), 0.0, -yaw.cos());
    let right = Vec3::new(yaw.cos(), 0.0, yaw.sin());
    let mut wish = forward * forward_input + right * right_input;
    if wish.length_squared() > 1.0 {
        wish = wish.normalize();
    }
    PlayerInputCommand {
        input_sequence,
        movement_x_per_mille: (wish.x * 1_000.0).round() as i16,
        movement_z_per_mille: (wish.z * 1_000.0).round() as i16,
        jump: pressed.contains(&KeyCode::Space),
        sprint: pressed.contains(&KeyCode::ShiftLeft) || pressed.contains(&KeyCode::ShiftRight),
    }
}

struct App {
    game: Option<MultiplayerGame>,
    server: SocketAddr,
    exit_after: Option<Duration>,
    failure: Option<String>,
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.game.is_some() {
            return;
        }
        let attributes = Window::default_attributes()
            .with_title("Destructible FPS multijoueur | initialisation")
            .with_inner_size(LogicalSize::new(1280.0, 800.0))
            .with_min_inner_size(LogicalSize::new(800.0, 500.0));
        let Ok(window) = event_loop.create_window(attributes) else {
            self.failure = Some("creation de fenetre impossible".to_owned());
            event_loop.exit();
            return;
        };
        match MultiplayerGame::new(Arc::new(window), self.server, self.exit_after.is_some()) {
            Ok(game) => self.game = Some(game),
            Err(error) => {
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
        if game.window.id() != window_id {
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
                ..
            } if !game.cursor_captured => game.capture_cursor(),
            WindowEvent::RedrawRequested => {
                if let Err(error) = game.redraw(event_loop, self.exit_after) {
                    self.failure = Some(error);
                    event_loop.exit();
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
            game.view.look(delta.0, delta.1);
        }
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(game) = &self.game {
            game.window.request_redraw();
        }
    }
}

struct Options {
    server: SocketAddr,
    exit_after: Option<Duration>,
}

fn options() -> Result<Options, Box<dyn Error>> {
    let mut server: SocketAddr = DEFAULT_SERVER.parse()?;
    let mut exit_after = None;
    let mut arguments = std::env::args().skip(1);
    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--server" => {
                server = arguments
                    .next()
                    .ok_or("--server exige une adresse")?
                    .parse()?;
            }
            "--smoke-seconds" => {
                let seconds: f64 = arguments
                    .next()
                    .ok_or("--smoke-seconds exige une duree")?
                    .parse()?;
                if !seconds.is_finite() || seconds <= 0.0 {
                    return Err("duree de smoke test invalide".into());
                }
                exit_after = Some(Duration::from_secs_f64(seconds));
            }
            _ => return Err(format!("argument inconnu: {argument}").into()),
        }
    }
    if !server.ip().is_loopback() {
        return Err(
            "multiplayer-demo utilise uniquement le serveur UDP loopback de developpement".into(),
        );
    }
    Ok(Options { server, exit_after })
}

fn main() -> Result<(), Box<dyn Error>> {
    let options = options()?;
    let event_loop = EventLoop::new()?;
    event_loop.set_control_flow(ControlFlow::Poll);
    let mut app = App {
        game: None,
        server: options.server,
        exit_after: options.exit_after,
        failure: None,
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
    fn movement_is_camera_relative_and_unit_bounded() {
        let pressed = [KeyCode::KeyW, KeyCode::KeyD].into_iter().collect();
        let input = movement_command(7, 0.0, &pressed);
        assert_eq!(input.input_sequence, 7);
        assert_eq!(input.movement_x_per_mille, 707);
        assert_eq!(input.movement_z_per_mille, -707);
        assert!(
            i64::from(input.movement_x_per_mille).pow(2)
                + i64::from(input.movement_z_per_mille).pow(2)
                <= 1_000_000
        );
    }

    #[test]
    fn fixed_positions_convert_to_voxel_world_units() {
        assert_eq!(
            fixed_to_world(FixedMicrometers3 {
                x: 2 * MICROMETERS_PER_VOXEL,
                y: MICROMETERS_PER_VOXEL / 2,
                z: -3 * MICROMETERS_PER_VOXEL,
            }),
            Vec3::new(2.0, 0.5, -3.0)
        );
    }
}
