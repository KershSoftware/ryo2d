//! Menu scene with single player, host, and join options

use ryo2d::prelude::*;

use crate::constants::*;

use super::GameScene;

/// Generate a random 5-character room code
fn generate_room_code() -> String {
  const CHARS: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";
  let mut rng = rand::thread_rng();
  (0..ROOM_CODE_LENGTH)
    .map(|_| CHARS[rng.gen_range(0..CHARS.len())] as char)
    .collect()
}

/// Menu items that can be selected
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MenuItem {
  SinglePlayer,
  CreateRoom,
  JoinRoom,
}

impl MenuItem {
  fn count() -> usize {
    3
  }

  fn from_index(i: usize) -> Self {
    match i % 3 {
      0 => MenuItem::SinglePlayer,
      1 => MenuItem::CreateRoom,
      _ => MenuItem::JoinRoom,
    }
  }

  fn index(self) -> usize {
    match self {
      MenuItem::SinglePlayer => 0,
      MenuItem::CreateRoom => 1,
      MenuItem::JoinRoom => 2,
    }
  }
}

/// Host mode: LAN (UDP) or Internet (WebRTC)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HostMode {
  Lan,
  Internet,
}

/// Fairy light data for menu background
#[derive(Debug, Clone, Copy)]
struct MenuFairyLight {
  base_x: f32,
  base_y: f32,
  phase: f32,
}

pub struct MenuScene {
  room_code_input: TextInputState,
  address_input: TextInputState,
  signal_url_input: TextInputState,
  error_message: Option<String>,
  last_dt: f32,
  pending_scene: Option<Box<dyn Scene>>,

  // Host mode toggle
  host_mode: HostMode,

  // Navigation
  selected: MenuItem,
  input_cooldown: f32, // Prevent rapid navigation

  // GPU-rendered background and rain
  background_sprite: Option<SpriteHandle>,
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
    let mut address_input = TextInputState::new();
    address_input.set_text(DEFAULT_ADDRESS);
    let mut signal_url_input = TextInputState::new();
    signal_url_input.set_text(DEFAULT_SIGNAL_URL);
    Self {
      room_code_input: TextInputState::new(),
      address_input,
      signal_url_input,
      error_message: None,
      last_dt: 0.016,
      pending_scene: None,
      host_mode: HostMode::Lan,
      selected: MenuItem::SinglePlayer,
      input_cooldown: 0.0,
      background_sprite: None,
      rain_emitter: None,
      rain_sprite: None,
      particle_system: ParticleSystem::new(),
      fairy_lights: Vec::new(),
      elapsed_time: 0.0,
      screen_width: 0.0,
      screen_height: 0.0,
    }
  }

  fn navigate_up(&mut self) {
    let idx = self.selected.index();
    let new_idx = if idx == 0 {
      MenuItem::count() - 1
    } else {
      idx - 1
    };
    self.selected = MenuItem::from_index(new_idx);
    self.input_cooldown = 0.15;
  }

  fn navigate_down(&mut self) {
    let idx = self.selected.index();
    let new_idx = (idx + 1) % MenuItem::count();
    self.selected = MenuItem::from_index(new_idx);
    self.input_cooldown = 0.15;
  }

  fn activate_selected(&mut self) {
    match self.selected {
      MenuItem::SinglePlayer => self.try_single_player(),
      MenuItem::CreateRoom => self.try_create_room(),
      MenuItem::JoinRoom => self.try_join_room(),
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
      .spawn((Transform::new(self.screen_width / 2.0, 0.0), emitter));

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
        let signal_url = self.signal_url_input.text().trim().to_string();
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
    let room_code = self.room_code_input.text().trim().to_uppercase();
    let address = self.address_input.text().trim().to_string();

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
    self.screen_width = ctx.screen_width();
    self.screen_height = ctx.screen_height();

    // Set camera to show the menu area (center of screen in world space)
    ctx
      .camera
      .set_position(self.screen_width / 2.0, self.screen_height / 2.0);

    // Set up ambient lighting - bright enough to see rain particles
    if let Some(lighting) = ctx.lighting {
      lighting.set_ambient(AmbientLight::new(
        Color::rgb(60, 70, 100), // Brighter blue-gray ambient
        0.6,                     // High intensity so rain is visible everywhere
      ));
      lighting.set_ui_affected_by_lighting(false);
    }

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
      let outer = ctx.world.spawn((
        Transform::new(x, y),
        Light2D::point_with_intensity(
          Color::rgb(LAMP_LIGHT_COLOR.0, LAMP_LIGHT_COLOR.1, LAMP_LIGHT_COLOR.2),
          LAMP_LIGHT_RADIUS,
          LAMP_LIGHT_INTENSITY,
        ),
      ));

      // Inner bright core (separate entity)
      let inner = ctx.world.spawn((
        Transform::new(x, y),
        Light2D::point_with_intensity(
          Color::rgb(LAMP_INNER_COLOR.0, LAMP_INNER_COLOR.1, LAMP_INNER_COLOR.2),
          LAMP_INNER_RADIUS,
          LAMP_INNER_INTENSITY,
        ),
      ));

      // Track both lights with same animation data
      self.fairy_lights.push((outer, light_data));
      self.fairy_lights.push((inner, light_data));
    }

    // Spawn rain particles
    self.spawn_rain(ctx);

    // Play rain ambient sound (persists between scenes with play_music_once)
    if let Some(audio) = &mut ctx.audio {
      let _ = audio.play_music_once(
        "assets/rain.mp3",
        MusicOptions::new().volume(0.4).looping(true),
      );
    }
  }

  fn update(&mut self, ctx: &mut SceneContext, dt: f32) -> SceneAction {
    self.last_dt = dt;
    self.elapsed_time += dt;

    // Update cooldown
    if self.input_cooldown > 0.0 {
      self.input_cooldown -= dt;
    }

    // Animate fairy lights
    let t = self.elapsed_time;
    for (entity, light) in &self.fairy_lights {
      if let Some(mut transform) = ctx.world.get_component_mut::<Transform>(*entity) {
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

        transform.x = light.base_x + offset_x;
        transform.y = light.base_y + offset_y;
      }
    }

    // Update particle system
    self.particle_system.update(ctx.world, ctx.events, dt);

    // Check for pending scene switch
    if let Some(scene) = self.pending_scene.take() {
      return SceneAction::Switch(scene);
    }

    // ESC or gamepad B to quit
    if ctx.input.key_pressed(KeyCode::Escape)
      || ctx.input.gamepad_button_pressed(GamepadButton::East)
    {
      return SceneAction::Quit;
    }

    // Check if text inputs are focused
    let text_input_focused = self.room_code_input.is_focused()
      || self.address_input.is_focused()
      || self.signal_url_input.is_focused();

    // Navigation (only when text inputs not focused)
    if !text_input_focused && self.input_cooldown <= 0.0 {
      // Up navigation
      let nav_up = ctx.input.key_pressed(KeyCode::ArrowUp)
        || ctx.input.key_pressed(KeyCode::KeyW)
        || ctx.input.gamepad_button_pressed(GamepadButton::DPadUp)
        || ctx.input.left_stick_y() > 0.5;

      // Down navigation
      let nav_down = ctx.input.key_pressed(KeyCode::ArrowDown)
        || ctx.input.key_pressed(KeyCode::KeyS)
        || ctx.input.gamepad_button_pressed(GamepadButton::DPadDown)
        || ctx.input.left_stick_y() < -0.5;

      // Left/right navigation (toggle host mode when on CreateRoom)
      let nav_left = ctx.input.key_pressed(KeyCode::ArrowLeft)
        || ctx.input.key_pressed(KeyCode::KeyA)
        || ctx.input.gamepad_button_pressed(GamepadButton::DPadLeft)
        || ctx.input.left_stick_x() < -0.5;

      let nav_right = ctx.input.key_pressed(KeyCode::ArrowRight)
        || ctx.input.key_pressed(KeyCode::KeyD)
        || ctx.input.gamepad_button_pressed(GamepadButton::DPadRight)
        || ctx.input.left_stick_x() > 0.5;

      if nav_up {
        self.navigate_up();
      } else if nav_down {
        self.navigate_down();
      } else if self.selected == MenuItem::CreateRoom && (nav_left || nav_right) {
        self.host_mode = match self.host_mode {
          HostMode::Lan => HostMode::Internet,
          HostMode::Internet => HostMode::Lan,
        };
        self.input_cooldown = 0.15;
      }
    }

    // Activation (Enter, Space, or gamepad A/X)
    let activate = ctx.input.key_pressed(KeyCode::Enter)
      || ctx.input.key_pressed(KeyCode::Space)
      || ctx.input.gamepad_button_pressed(GamepadButton::South)
      || ctx.input.gamepad_button_pressed(GamepadButton::Start);

    if activate && !text_input_focused {
      self.activate_selected();
    }

    // Special case: Enter while in text input tries to join
    if ctx.input.key_pressed(KeyCode::Enter) && text_input_focused {
      let has_code = self.room_code_input.text().len() == ROOM_CODE_LENGTH;
      let has_address = !self.address_input.text().trim().is_empty();
      if has_code && has_address {
        self.try_join_room();
      }
    }

    SceneAction::Continue
  }

  fn render_ui(&mut self, ui: &mut UiContext) {
    let dt = self.last_dt;
    let (sw, sh) = ui.screen_size();
    let center_x = sw as f32 / 2.0;
    let center_y = sh as f32 / 2.0;

    // No background rect - let the world (rain particles + lighting) show through

    // Title (with extra top padding)
    ui.draw_text_anchored("LEAP KING", Anchor::TopCenter, 0.0, 100.0, Color::WHITE);
    ui.draw_text_anchored(
      "Charge. Aim. Release.",
      Anchor::TopCenter,
      0.0,
      130.0,
      Color::GRAY,
    );

    // Button dimensions
    let button_width = 200.0;
    let button_height = 40.0;
    let button_x = center_x - button_width / 2.0;
    let highlight_color = Color::rgb(PLAYER_A_COLOR.0, PLAYER_A_COLOR.1, PLAYER_A_COLOR.2);

    // Single Player button
    let single_rect = Rect::new(button_x, center_y - 80.0, button_width, button_height);
    let single_selected = self.selected == MenuItem::SinglePlayer;
    if single_selected {
      // Draw selection indicator
      ui.draw_rect(
        Rect::new(
          single_rect.x - 4.0,
          single_rect.y - 4.0,
          single_rect.width + 8.0,
          single_rect.height + 8.0,
        ),
        highlight_color,
      );
      // Draw selector arrow (vertically centered in button)
      ui.label(
        ">",
        single_rect.x - 20.0,
        single_rect.y + button_height / 2.0 - 4.0,
        highlight_color,
        None,
      );
    }
    if ui.button("Single Player", single_rect) {
      self.try_single_player();
    }

    // Host mode toggle (LAN / Internet) - above Create Room
    let toggle_y = center_y - 18.0;
    let toggle_width = 90.0;
    let toggle_height = 24.0;
    let toggle_gap = 24.0;
    let total_toggle_width = toggle_width * 2.0 + toggle_gap;
    let toggle_x = center_x - total_toggle_width / 2.0;

    let lan_rect = Rect::new(toggle_x, toggle_y, toggle_width, toggle_height);
    let internet_rect = Rect::new(
      toggle_x + toggle_width + toggle_gap,
      toggle_y,
      toggle_width,
      toggle_height,
    );

    let active_color = highlight_color;
    let inactive_color = Color::new(60, 60, 70, 255);

    // Draw arrow indicator on the active toggle when CreateRoom is selected
    let show_toggle_arrow = self.selected == MenuItem::CreateRoom;

    // LAN toggle button
    if self.host_mode == HostMode::Lan {
      ui.draw_rect(lan_rect, active_color);
      if show_toggle_arrow {
        ui.label(
          ">",
          lan_rect.x - 16.0,
          lan_rect.y + toggle_height / 2.0 - 4.0,
          highlight_color,
          None,
        );
      }
    } else {
      ui.draw_rect(lan_rect, inactive_color);
    }
    if ui.button("LAN", lan_rect) {
      self.host_mode = HostMode::Lan;
    }

    // Internet toggle button
    if self.host_mode == HostMode::Internet {
      ui.draw_rect(internet_rect, active_color);
      if show_toggle_arrow {
        ui.label(
          ">",
          internet_rect.x - 16.0,
          internet_rect.y + toggle_height / 2.0 - 4.0,
          highlight_color,
          None,
        );
      }
    } else {
      ui.draw_rect(internet_rect, inactive_color);
    }
    if ui.button("Internet", internet_rect) {
      self.host_mode = HostMode::Internet;
    }

    // Signal URL input (only visible in Internet mode)
    let mut extra_offset = 0.0;
    if self.host_mode == HostMode::Internet {
      let signal_input_y = center_y + 14.0;
      let signal_input_width = 240.0;
      let signal_input_rect = Rect::new(
        center_x - signal_input_width / 2.0,
        signal_input_y,
        signal_input_width,
        28.0,
      );
      ui.text_input(
        &mut self.signal_url_input,
        signal_input_rect,
        "wss://...trycloudflare.com",
        dt,
      );
      extra_offset = 37.0;
    }

    // Create Room button (below toggle or signal URL input)
    let create_rect = Rect::new(
      button_x,
      center_y + 14.0 + extra_offset,
      button_width,
      button_height,
    );
    let create_selected = self.selected == MenuItem::CreateRoom;
    if create_selected {
      ui.draw_rect(
        Rect::new(
          create_rect.x - 4.0,
          create_rect.y - 4.0,
          create_rect.width + 8.0,
          create_rect.height + 8.0,
        ),
        highlight_color,
      );
      ui.label(
        ">",
        create_rect.x - 20.0,
        create_rect.y + button_height / 2.0 - 4.0,
        highlight_color,
        None,
      );
    }
    let create_label = match self.host_mode {
      HostMode::Lan => "Create Room (LAN)",
      HostMode::Internet => "Create Room (Net)",
    };
    if ui.button(create_label, create_rect) {
      self.try_create_room();
    }

    // Separator
    ui.draw_text_anchored(
      "- or join -",
      Anchor::Center,
      0.0,
      69.0 + extra_offset,
      Color::GRAY,
    );

    // Join section - Room Code
    ui.draw_text_anchored(
      "Room Code:",
      Anchor::Center,
      -80.0,
      99.0 + extra_offset,
      Color::WHITE,
    );

    let code_input_width = 100.0;
    let code_input_rect = Rect::new(
      center_x - 80.0 - code_input_width / 2.0,
      center_y + 119.0 + extra_offset,
      code_input_width,
      32.0,
    );
    ui.text_input(&mut self.room_code_input, code_input_rect, "XXXXX", dt);

    // Join section - Server Address
    ui.draw_text_anchored(
      "Address:",
      Anchor::Center,
      80.0,
      99.0 + extra_offset,
      Color::WHITE,
    );

    let addr_input_width = 160.0;
    let addr_input_rect = Rect::new(
      center_x + 80.0 - addr_input_width / 2.0,
      center_y + 119.0 + extra_offset,
      addr_input_width,
      32.0,
    );
    ui.text_input(
      &mut self.address_input,
      addr_input_rect,
      DEFAULT_ADDRESS,
      dt,
    );

    // Hint for join: address format
    ui.draw_text_anchored(
      "Use ws:// or wss:// URL for internet, IP for LAN",
      Anchor::Center,
      0.0,
      162.0 + extra_offset,
      Color::new(100, 180, 255, 150),
    );

    // Join button
    let join_rect = Rect::new(
      button_x,
      center_y + 180.0 + extra_offset,
      button_width,
      button_height,
    );
    let join_selected = self.selected == MenuItem::JoinRoom;
    if join_selected {
      ui.draw_rect(
        Rect::new(
          join_rect.x - 4.0,
          join_rect.y - 4.0,
          join_rect.width + 8.0,
          join_rect.height + 8.0,
        ),
        highlight_color,
      );
      ui.label(
        ">",
        join_rect.x - 20.0,
        join_rect.y + button_height / 2.0 - 4.0,
        highlight_color,
        None,
      );
    }
    if ui.button("Join Room", join_rect) {
      self.try_join_room();
    }

    // Error message
    if let Some(ref err) = self.error_message {
      ui.draw_text_anchored(err, Anchor::Center, 0.0, 234.0 + extra_offset, Color::RED);
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
  }

  fn render_gpu(&mut self, ctx: &mut GpuRenderContext) {
    // Render background first (lowest z-order)
    if let Some(ref bg_sprite) = self.background_sprite {
      ctx.upload_sprite(bg_sprite);
      let (screen_x, screen_y) = ctx.camera.world_to_screen(0.0, 0.0);
      let bg_instance = SpriteInstance::new().with_transform(
        screen_x,
        screen_y,
        0.0,
        self.screen_width,
        self.screen_height,
      );
      ctx
        .batch
        .add(bg_sprite.id(), false, bg_instance, Z_BACKGROUND);
    }

    // Upload rain sprite texture and collect particles
    if let Some(ref sprite) = self.rain_sprite {
      ctx.upload_sprite(sprite);
    }

    // Collect particles for rendering (uses Z_PLATFORM - 1 set in emitter)
    self
      .particle_system
      .collect_for_render(ctx.world, ctx.camera, ctx.batch);
  }
}
