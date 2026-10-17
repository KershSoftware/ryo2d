//! Menu scene with single player, host, and join options

use ryo2d::prelude::*;

use crate::constants::*;
use crate::look::use_game_look;

use super::GameScene;

/// Generate a random 5-character room code
fn generate_room_code() -> String {
  const CHARS: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";
  let mut rng = rand::thread_rng();
  (0..ROOM_CODE_LENGTH)
    .map(|_| CHARS[rng.gen_range(0..CHARS.len())] as char)
    .collect()
}

/// Host mode: LAN (UDP) or Internet (WebRTC)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HostMode {
  Lan,
  Internet,
}

/// The menu button pressed this frame
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Pressed {
  SinglePlayer,
  CreateRoom,
  JoinRoom,
}

/// The menu buttons' size
const BUTTON_WIDTH: f32 = 320.0;
const BUTTON_HEIGHT: f32 = 40.0;

/// Fairy light data for menu background
#[derive(Debug, Clone, Copy)]
struct MenuFairyLight {
  base_x: f32,
  base_y: f32,
  phase: f32,
}

pub struct MenuScene {
  room_code: String,
  address: String,
  signal_url: String,
  error_message: Option<String>,
  pending_scene: Option<Box<dyn Scene>>,

  // Host mode toggle
  host_mode: HostMode,

  rain_emitter: Option<Entity>,
  rain_sprite: Option<SpriteHandle>,
  particle_system: ParticleSystem,

  // Fairy lights
  fairy_lights: Vec<(Entity, MenuFairyLight)>,
  elapsed_time: f32,

  // Screen dimensions (set from engine in on_enter)
  screen_width: f32,
  screen_height: f32,
}

impl MenuScene {
  pub fn new() -> Self {
    Self {
      room_code: String::new(),
      address: DEFAULT_ADDRESS.to_string(),
      signal_url: DEFAULT_SIGNAL_URL.to_string(),
      error_message: None,
      pending_scene: None,
      host_mode: HostMode::Lan,
      rain_emitter: None,
      rain_sprite: None,
      particle_system: ParticleSystem::new(),
      fairy_lights: Vec::new(),
      elapsed_time: 0.0,
      screen_width: 0.0,
      screen_height: 0.0,
    }
  }

  fn spawn_rain(&mut self, ctx: &mut SceneContext) {
    // Create particle sprite
    let rain_sprite = ctx.assets.create_sprite(
      Sprite::solid_circle(8, 255, 255, 255, 255), // White circle
      "rain_particle_menu",
    );
    self.rain_sprite = Some(rain_sprite.clone());

    // Use the built-in rain preset with wider coverage
    // Add glow effect so rain is visible even with dark lighting
    let emitter = ParticleEmitter::rain(rain_sprite)
      .with_mode(EmissionMode::Continuous { rate: 80.0 })
      .with_shape(EmissionShape::Line {
        start_x: -self.screen_width / 2.0,
        start_y: -50.0, // Above emitter
        end_x: self.screen_width / 2.0,
        end_y: -50.0,
      })
      .with_z_order(Z_PLATFORM - 1) // Above background, below platforms
      .with_effect(SpriteEffect::glow_with_intensity(
        Color::new(180, 200, 255, 255), // Blue-white glow
        3.0,
        0.8,
      ));

    // Position emitter at top of visible area (camera is at screen center)
    let rain_entity = ctx
      .world
      .spawn((Transform::from_xy(self.screen_width / 2.0, 0.0), emitter))
      .id();

    self.rain_emitter = Some(rain_entity);
  }

  fn try_single_player(&mut self) {
    match GameScene::new_single_player() {
      Ok(scene) => {
        self.pending_scene = Some(Box::new(scene));
      }
      Err(e) => {
        self.error_message = Some(format!("Failed to start: {}", e));
      }
    }
  }

  fn try_create_room(&mut self) {
    let room_code = generate_room_code();
    let result = match self.host_mode {
      HostMode::Lan => GameScene::new_host(room_code),
      HostMode::Internet => {
        let signal_url = self.signal_url.trim().to_string();
        if signal_url.is_empty() {
          self.error_message = Some("Enter signal server URL".to_string());
          return;
        }
        GameScene::new_host_webrtc(room_code, &signal_url)
      }
    };
    match result {
      Ok(scene) => {
        self.pending_scene = Some(Box::new(scene));
      }
      Err(e) => {
        self.error_message = Some(format!("Failed to create: {}", e));
      }
    }
  }

  fn try_join_room(&mut self) {
    let room_code = self.room_code.trim().to_uppercase();
    let address = self.address.trim().to_string();

    if room_code.len() != ROOM_CODE_LENGTH {
      self.error_message = Some(format!("Room code must be {} characters", ROOM_CODE_LENGTH));
      return;
    }

    if address.is_empty() {
      self.error_message = Some("Enter server address".to_string());
      return;
    }

    match GameScene::new_client(&room_code, &address) {
      Ok(scene) => {
        self.pending_scene = Some(Box::new(scene));
      }
      Err(e) => {
        self.error_message = Some(format!("Failed to join: {}", e));
      }
    }
  }
}

impl Scene for MenuScene {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    use_game_look(ctx);

    self.screen_width = ctx.screen_width();
    self.screen_height = ctx.screen_height();

    // Set camera to show the menu area (center of screen in world space)
    ctx
      .camera()
      .set_position(self.screen_width / 2.0, self.screen_height / 2.0);

    // Set up ambient lighting - bright enough to see rain particles
    ctx.lighting().set_ambient(AmbientLight::new(
      Color::rgb(133, 143, 168), // Brighter blue-gray ambient ((60, 70, 100) read as linear)
      0.6,                       // High intensity so rain is visible everywhere
    ));

    // Spawn fairy lights at random positions
    let mut rng = rand::thread_rng();
    let fairy_count = 5;
    for _ in 0..fairy_count {
      let x = rng.gen_range(50.0..(self.screen_width - 50.0));
      let y = rng.gen_range(100.0..(self.screen_height - 100.0));
      let phase = rng.gen_range(0.0..std::f32::consts::TAU);

      let light_data = MenuFairyLight {
        base_x: x,
        base_y: y,
        phase,
      };

      // Outer glow light
      let outer = ctx
        .world
        .spawn((
          Transform::from_xy(x, y),
          Light2D::point_with_intensity(
            Color::rgb(LAMP_LIGHT_COLOR.0, LAMP_LIGHT_COLOR.1, LAMP_LIGHT_COLOR.2),
            LAMP_LIGHT_RADIUS,
            LAMP_LIGHT_INTENSITY,
          ),
        ))
        .id();

      // Inner bright core (separate entity)
      let inner = ctx
        .world
        .spawn((
          Transform::from_xy(x, y),
          Light2D::point_with_intensity(
            Color::rgb(LAMP_INNER_COLOR.0, LAMP_INNER_COLOR.1, LAMP_INNER_COLOR.2),
            LAMP_INNER_RADIUS,
            LAMP_INNER_INTENSITY,
          ),
        ))
        .id();

      // Track both lights with same animation data
      self.fairy_lights.push((outer, light_data));
      self.fairy_lights.push((inner, light_data));
    }

    // Spawn rain particles
    self.spawn_rain(ctx);

    // Play rain ambient sound (persists between scenes with play_music_once)
    let _ = ctx.audio.play_music_once(
      "assets/rain.mp3",
      MusicOptions::new().volume(0.4).looping(true),
    );
  }

  fn update(&mut self, ctx: &mut SceneContext, dt: f32) -> SceneAction {
    self.elapsed_time += dt;

    // Animate fairy lights
    let t = self.elapsed_time;
    for (entity, light) in &self.fairy_lights {
      if let Some(mut transform) = ctx.world.get_mut::<Transform>(*entity) {
        let p = light.phase;

        // Fairy-like floating motion using Lissajous curves
        let fairy_x = (t * LAMP_OSCILLATE_SPEED * 0.7 + p).sin()
          * (t * LAMP_OSCILLATE_SPEED * 0.3 + p * 0.5).cos();
        let fairy_y = (t * LAMP_OSCILLATE_SPEED * 0.5 + p * 1.3).sin();

        let drift_x = (t * LAMP_OSCILLATE_SPEED * 0.15 + p * 2.1).sin() * 0.5
          + (t * LAMP_OSCILLATE_SPEED * 0.08 + p * 0.7).cos() * 0.3;
        let drift_y = (t * LAMP_OSCILLATE_SPEED * 0.25 + p * 1.7).cos() * 0.3
          + (t * LAMP_OSCILLATE_SPEED * 0.9 + p * 2.3).sin() * 0.2;

        let offset_x = LAMP_OSCILLATE_RADIUS * (fairy_x * 0.8 + drift_x) * 3.0;
        let offset_y = LAMP_OSCILLATE_RADIUS * (fairy_y * 0.5 + drift_y);

        transform.translation.x = light.base_x + offset_x;
        transform.translation.y = light.base_y + offset_y;
      }
    }

    // Update particle system
    self.particle_system.update(ctx.world, dt);

    // Check for pending scene switch
    if let Some(scene) = self.pending_scene.take() {
      return SceneAction::Switch(scene);
    }

    SceneAction::Continue
  }

  fn ui(&mut self, ui: &mut UiContext, _ctx: &mut SceneContext) -> SceneAction {
    // No background rect - let the world (rain particles + lighting) show through

    let mut pressed = None;
    // Enter in either join field tries to join
    let mut submitted = false;
    let host_mode = &mut self.host_mode;
    let (signal_url, room_code, address) =
      (&mut self.signal_url, &mut self.room_code, &mut self.address);
    let error = self.error_message.as_deref();
    ui.column()
      .focus_scope()
      .anchor(Anchor::Center)
      .gap(10.0)
      .align(Align::Center)
      .show(|ui| {
        ui.column().gap(6.0).align(Align::Center).show(|ui| {
          ui.text("LEAP KING").color(Color::WHITE);
          ui.text("Charge. Aim. Release.").color(Color::GRAY);
        });

        if ui
          .button("Single Player")
          .width(BUTTON_WIDTH)
          .height(BUTTON_HEIGHT)
          .default_focus()
          .clicked()
        {
          pressed = Some(Pressed::SinglePlayer);
        }

        // Host mode: LAN (UDP) or Internet (WebRTC), for Create Room
        ui.row().gap(24.0).show(|ui| {
          ui.radio(&mut *host_mode, HostMode::Lan, "LAN").show();
          ui.radio(&mut *host_mode, HostMode::Internet, "Internet")
            .show();
        });
        // The signal server, in Internet mode only
        if *host_mode == HostMode::Internet {
          ui.text_input(signal_url)
            .placeholder("wss://...trycloudflare.com")
            .fill_width()
            .show();
        }

        let create_label = match *host_mode {
          HostMode::Lan => "Create Room (LAN)",
          HostMode::Internet => "Create Room (Net)",
        };
        if ui
          .button(create_label)
          .width(BUTTON_WIDTH)
          .height(BUTTON_HEIGHT)
          .clicked()
        {
          pressed = Some(Pressed::CreateRoom);
        }

        ui.text("- or join -").color(Color::GRAY);

        ui.row().gap(20.0).show(|ui| {
          ui.column().gap(6.0).show(|ui| {
            ui.text("Room Code:").color(Color::WHITE);
            submitted |= ui
              .text_input(room_code)
              .placeholder("XXXXX")
              .max_len(ROOM_CODE_LENGTH)
              .width(100.0)
              .height(32.0)
              .show()
              .submitted();
          });
          ui.column().gap(6.0).show(|ui| {
            ui.text("Address:").color(Color::WHITE);
            submitted |= ui
              .text_input(address)
              .placeholder(DEFAULT_ADDRESS)
              .width(160.0)
              .height(32.0)
              .show()
              .submitted();
          });
        });

        if ui
          .button("Join Room")
          .width(BUTTON_WIDTH)
          .height(BUTTON_HEIGHT)
          .clicked()
        {
          pressed = Some(Pressed::JoinRoom);
        }

        ui.text("Use ws:// or wss:// URL for internet, IP for LAN")
          .color(Color::new(100, 180, 255, 150));
        if let Some(err) = error {
          ui.text(err).color(Color::RED);
        }
      });

    match pressed {
      Some(Pressed::SinglePlayer) => self.try_single_player(),
      Some(Pressed::CreateRoom) => self.try_create_room(),
      Some(Pressed::JoinRoom) => self.try_join_room(),
      None if submitted => self.try_join_room(),
      None => {}
    }

    // Controls hint
    ui.draw_text_anchored(
      "Navigate: Arrows/D-Pad/Joysticks | Select: Enter/A/X",
      Anchor::BottomCenter,
      0.0,
      -40.0,
      Color::DARK_GRAY,
    );

    // ESC hint
    ui.draw_text_anchored(
      "ESC/B/Circle to quit",
      Anchor::BottomCenter,
      0.0,
      -20.0,
      Color::DARK_GRAY,
    );

    // Back (Escape, B) quits; a focused field takes it first, to leave it
    if ui.back() {
      return SceneAction::Quit;
    }
    SceneAction::Continue
  }

  fn render(&mut self, gpu: &mut GpuRenderContext) {
    // Collect particles for rendering (uses Z_PLATFORM - 1 set in emitter)
    self
      .particle_system
      .collect_for_render(gpu.world, gpu.batch);
  }
}
