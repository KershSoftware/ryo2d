//! Trails Demo - Ryo2D Example
//!
//! This example demonstrates the trail rendering system:
//! - Position-based fade (fades from head to tail based on position)
//! - Time-based fade (fades based on point age with lifetime)
//! - Different trail presets (bullet, laser, swipe)
//! - Configurable parameters (width, length, taper, color)
//! - **Textured trails** with stretch and tile modes
//!
//! ## Controls
//!
//! - Mouse: Control the center trail
//! - 1/2/3: Switch trail preset (Bullet / Laser / Swipe)
//! - Q/W/E: Toggle position fade / time fade / no fade
//! - Up/Down: Adjust trail length
//! - Left/Right: Adjust trail width
//! - T: Toggle textured trail (stretch mode)
//! - Y: Toggle textured trail (tile mode)
//! - Space: Pause/resume orbiting entities
//! - R: Reset all trails
//! - Escape: Exit

use ryo2d::prelude::*;

/// Marker for the mouse-controlled trail entity
#[derive(Debug)]
struct MouseTrail;

/// Marker for orbiting trail entities
#[derive(Debug)]
struct OrbitTrail {
  angle: f32,
  radius: f32,
  speed: f32,
  center_x: f32,
  center_y: f32,
}

/// Marker for the textured trail entity
#[derive(Debug)]
struct TexturedTrailMarker {
  angle: f32,
}

/// Trail configuration for display
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TrailPreset {
  Bullet,
  Laser,
  Swipe,
}

impl TrailPreset {
  fn name(&self) -> &'static str {
    match self {
      TrailPreset::Bullet => "Bullet",
      TrailPreset::Laser => "Laser",
      TrailPreset::Swipe => "Swipe",
    }
  }
}

/// Fade mode for trails
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FadeMode {
  Position, // Fades based on position in trail (head to tail)
  Time,     // Fades based on point age (lifetime)
  Distance, // Fades based on actual distance in pixels
  None,     // No fade
}

impl FadeMode {
  fn name(&self) -> &'static str {
    match self {
      FadeMode::Position => "Position (index-based)",
      FadeMode::Time => "Time (age-based)",
      FadeMode::Distance => "Distance (pixels)",
      FadeMode::None => "None",
    }
  }
}

/// Main demo scene
struct TrailsDemoScene {
  mouse_entity: Option<Entity>,
  orbit_entities: Vec<Entity>,
  textured_entity: Option<Entity>,
  texture_sprite: Option<SpriteHandle>,
  texture_mode: Option<TextureMode>,
  paused: bool,
  current_preset: TrailPreset,
  fade_mode: FadeMode,
  trail_length: usize,
  trail_width: f32,
  lifetime: f32,
  fade_distance: f32,
  screen_width: f32,
  screen_height: f32,
}

impl TrailsDemoScene {
  fn new() -> Self {
    Self {
      mouse_entity: None,
      orbit_entities: Vec::new(),
      textured_entity: None,
      texture_sprite: None,
      texture_mode: None,
      paused: false,
      current_preset: TrailPreset::Bullet,
      fade_mode: FadeMode::Position,
      trail_length: 30,
      trail_width: 6.0,
      lifetime: 0.5,
      fade_distance: 100.0,
      screen_width: 1024.0,
      screen_height: 768.0,
    }
  }

  /// Create a trail based on current settings
  fn create_trail(&self, color: Color) -> Trail2D {
    let mut trail = match self.current_preset {
      TrailPreset::Bullet => Trail2D::bullet(color),
      TrailPreset::Laser => Trail2D::laser(color, self.trail_width),
      TrailPreset::Swipe => Trail2D::swipe(color, self.trail_width),
    };

    // Apply width (bullet preset has fixed width, so override it)
    trail.set_width(self.trail_width);

    // Apply length
    trail = trail.with_length(self.trail_length);

    // Apply fade mode
    match self.fade_mode {
      FadeMode::Position => {
        trail = trail.with_fade(true);
      }
      FadeMode::Time => {
        trail = trail.with_fade(false).with_lifetime(self.lifetime);
      }
      FadeMode::Distance => {
        trail = trail.with_fade(true).with_fade_distance(self.fade_distance);
      }
      FadeMode::None => {
        trail = trail.with_fade(false);
      }
    }

    trail
  }

  /// Create a small circle sprite
  fn create_circle(radius: u32, color: Color) -> SpriteHandle {
    let sprite = Sprite::solid_circle(radius, color.r, color.g, color.b, color.a);
    SpriteHandle::from_sprite(sprite, format!("circle_{}_{}", radius, color.r))
  }

  /// Create a gradient texture for trails (horizontal gradient: red -> yellow -> green -> cyan)
  fn create_trail_texture() -> SpriteHandle {
    let width = 64u32;
    let height = 32u32;
    let mut data = vec![0u8; (width * height * 4) as usize];

    for y in 0..height {
      for x in 0..width {
        let idx = ((y * width + x) * 4) as usize;
        let t = x as f32 / (width - 1) as f32;

        // Gradient: magenta -> cyan -> yellow -> magenta (to tile seamlessly)
        let (r, g, b) = if t < 0.33 {
          // Magenta to cyan
          let local_t = t / 0.33;
          (
            (255.0 * (1.0 - local_t)) as u8,
            (255.0 * local_t) as u8,
            255u8,
          )
        } else if t < 0.66 {
          // Cyan to yellow
          let local_t = (t - 0.33) / 0.33;
          (
            (255.0 * local_t) as u8,
            255u8,
            (255.0 * (1.0 - local_t)) as u8,
          )
        } else {
          // Yellow to magenta
          let local_t = (t - 0.66) / 0.34;
          (
            255u8,
            (255.0 * (1.0 - local_t)) as u8,
            (255.0 * local_t) as u8,
          )
        };

        // Add a vertical gradient for visual interest (brighter in center)
        let vy = 1.0 - ((y as f32 / (height - 1) as f32 - 0.5).abs() * 2.0).powf(2.0);
        let brightness = 0.5 + 0.5 * vy;

        data[idx] = (r as f32 * brightness) as u8;
        data[idx + 1] = (g as f32 * brightness) as u8;
        data[idx + 2] = (b as f32 * brightness) as u8;
        data[idx + 3] = 255; // Full alpha
      }
    }

    let sprite = Sprite::from_rgba(data, width, height);
    SpriteHandle::from_sprite(sprite, "trail_gradient")
  }

  /// Spawn the mouse-controlled trail entity
  fn spawn_mouse_trail(&mut self, ctx: &mut SceneContext) {
    let (mx, my) = ctx.input.mouse_position();
    let color = Color::rgb(100, 200, 255);

    let entity = ctx.world.spawn((
      Transform::new(mx, my).with_pivot(0.5, 0.5), // Center pivot so trail aligns with sprite
      Renderable::sprite(Self::create_circle(8, color)),
      self.create_trail(color),
      MouseTrail,
    ));

    self.mouse_entity = Some(entity);
  }

  /// Spawn the textured trail entity (orbits in a figure-8 pattern)
  fn spawn_textured_trail(&mut self, ctx: &mut SceneContext) {
    if self.textured_entity.is_some() {
      return; // Already spawned
    }

    // Create the texture if needed
    if self.texture_sprite.is_none() {
      self.texture_sprite = Some(Self::create_trail_texture());
    }

    let texture = self.texture_sprite.clone().unwrap();
    let mode = self.texture_mode.unwrap_or(TextureMode::Stretch);

    let center_x = self.screen_width / 2.0;
    let center_y = self.screen_height / 2.0;

    // Create trail with current settings + texture
    let trail = self
      .create_trail(Color::WHITE)
      .with_texture(texture)
      .with_texture_mode(mode)
      .with_tile_length(32.0);

    let entity = ctx.world.spawn((
      Transform::new(center_x, center_y).with_pivot(0.5, 0.5), // Center pivot so trail aligns with sprite
      Renderable::sprite(Self::create_circle(10, Color::WHITE)),
      trail,
      TexturedTrailMarker { angle: 0.0 },
    ));

    self.textured_entity = Some(entity);
  }

  /// Despawn the textured trail entity
  fn despawn_textured_trail(&mut self, ctx: &mut SceneContext) {
    if let Some(entity) = self.textured_entity.take() {
      let _ = ctx.world.despawn(entity);
    }
  }

  /// Update the textured trail entity (figure-8 pattern)
  fn update_textured_trail(&mut self, ctx: &mut SceneContext, dt: f32) {
    if self.paused {
      return;
    }

    if let Some(entity) = self.textured_entity {
      if let Some(mut marker) = ctx.world.get_component_mut::<TexturedTrailMarker>(entity) {
        marker.angle += 1.2 * dt;

        let center_x = self.screen_width / 2.0;
        let center_y = self.screen_height / 2.0;

        // Figure-8 / lemniscate pattern
        let a = 150.0; // Size of the figure-8
        let t = marker.angle;
        let new_x = center_x + a * t.sin();
        let new_y = center_y + a * t.sin() * t.cos();

        drop(marker);

        if let Some(mut transform) = ctx.world.get_component_mut::<Transform>(entity) {
          transform.x = new_x;
          transform.y = new_y;
        }

        // Update trail with new position
        if let Some(mut trail) = ctx.world.get_component_mut::<Trail2D>(entity) {
          trail.update(new_x, new_y, dt);
        }
      }
    }
  }

  /// Spawn orbiting trail entities
  fn spawn_orbit_trails(&mut self, ctx: &mut SceneContext) {
    let center_x = self.screen_width / 2.0;
    let center_y = self.screen_height / 2.0;

    // Different colored orbiting trails at different radii/speeds
    let configs = [
      // (radius, speed, color)
      (120.0, 1.5, Color::rgb(255, 100, 100)),
      (180.0, -1.2, Color::rgb(100, 255, 100)),
      (240.0, 0.8, Color::rgb(255, 200, 50)),
      (300.0, -0.6, Color::rgb(200, 100, 255)),
    ];

    for (i, (radius, speed, color)) in configs.iter().enumerate() {
      let angle = (i as f32) * std::f32::consts::PI / 2.0;
      let x = center_x + angle.cos() * radius;
      let y = center_y + angle.sin() * radius;

      // Create trail with current settings
      let trail = self.create_trail(*color);

      let entity = ctx.world.spawn((
        Transform::new(x, y).with_pivot(0.5, 0.5), // Center pivot so trail aligns with sprite
        Renderable::sprite(Self::create_circle(6, *color)),
        trail,
        OrbitTrail {
          angle,
          radius: *radius,
          speed: *speed,
          center_x,
          center_y,
        },
      ));

      self.orbit_entities.push(entity);
    }
  }

  /// Update orbiting entities
  fn update_orbits(&mut self, ctx: &mut SceneContext, dt: f32) {
    if self.paused {
      return;
    }

    for entity in &self.orbit_entities {
      if let Some(mut orbit) = ctx.world.get_component_mut::<OrbitTrail>(*entity) {
        orbit.angle += orbit.speed * dt;

        let new_x = orbit.center_x + orbit.angle.cos() * orbit.radius;
        let new_y = orbit.center_y + orbit.angle.sin() * orbit.radius;

        drop(orbit);

        if let Some(mut transform) = ctx.world.get_component_mut::<Transform>(*entity) {
          transform.x = new_x;
          transform.y = new_y;
        }

        // Update trail with new position
        if let Some(mut trail) = ctx.world.get_component_mut::<Trail2D>(*entity) {
          trail.update(new_x, new_y, dt);
        }
      }
    }
  }

  /// Update mouse-controlled entity
  fn update_mouse_trail(&mut self, ctx: &mut SceneContext, dt: f32) {
    if let Some(entity) = self.mouse_entity {
      let (mx, my) = ctx.input.mouse_position();

      if let Some(mut transform) = ctx.world.get_component_mut::<Transform>(entity) {
        transform.x = mx;
        transform.y = my;
      }

      // Update trail with new position
      if let Some(mut trail) = ctx.world.get_component_mut::<Trail2D>(entity) {
        trail.update(mx, my, dt);
      }
    }
  }

  /// Recreate all trails with new settings
  fn recreate_trails(&mut self, ctx: &mut SceneContext) {
    // Recreate mouse trail
    if let Some(entity) = self.mouse_entity {
      let color = Color::rgb(100, 200, 255);
      let trail = self.create_trail(color);
      ctx.world.add_component(entity, trail);
    }

    // Recreate orbit trails with their original colors
    let colors = [
      Color::rgb(255, 100, 100),
      Color::rgb(100, 255, 100),
      Color::rgb(255, 200, 50),
      Color::rgb(200, 100, 255),
    ];

    for (i, entity) in self.orbit_entities.iter().enumerate() {
      if let Some(color) = colors.get(i) {
        let trail = self.create_trail(*color);
        ctx.world.add_component(*entity, trail);
      }
    }

    // Recreate textured trail with current settings (if active)
    if let (Some(entity), Some(mode)) = (self.textured_entity, self.texture_mode) {
      if let Some(texture) = self.texture_sprite.clone() {
        let trail = self
          .create_trail(Color::WHITE)
          .with_texture(texture)
          .with_texture_mode(mode)
          .with_tile_length(32.0);
        ctx.world.add_component(entity, trail);
      }
    }
  }

  /// Reset all trails
  fn reset_trails(&mut self, ctx: &mut SceneContext) {
    // Clear mouse trail
    if let Some(entity) = self.mouse_entity {
      if let Some(mut trail) = ctx.world.get_component_mut::<Trail2D>(entity) {
        trail.clear();
      }
    }

    // Clear orbit trails
    for entity in &self.orbit_entities {
      if let Some(mut trail) = ctx.world.get_component_mut::<Trail2D>(*entity) {
        trail.clear();
      }
    }

    // Clear textured trail
    if let Some(entity) = self.textured_entity {
      if let Some(mut trail) = ctx.world.get_component_mut::<Trail2D>(entity) {
        trail.clear();
      }
    }
  }
}

impl Scene for TrailsDemoScene {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    let (w, h) = ctx.screen_size();
    self.screen_width = w;
    self.screen_height = h;

    // Print controls
    println!("=== TRAILS DEMO ===");
    println!("Mouse: Control trail");
    println!("1/2/3: Switch preset (Bullet/Laser/Swipe)");
    println!("Q/W/D/E: Fade mode (Position/Time/Distance/None)");
    println!("Up/Down: Trail length");
    println!("Left/Right: Trail width");
    println!("+/-: Lifetime (Time mode) or Fade Distance (Distance mode)");
    println!("T: Toggle texture mode (Stretch/Tile/Off)");
    println!("Space: Pause orbits");
    println!("R: Reset trails");
    println!("ESC: Exit");

    // Spawn entities
    self.spawn_mouse_trail(ctx);
    self.spawn_orbit_trails(ctx);

    // Spawn textured trail by default (Stretch mode)
    self.texture_mode = Some(TextureMode::Stretch);
    self.spawn_textured_trail(ctx);
  }

  fn update(&mut self, ctx: &mut SceneContext, dt: f32) -> SceneAction {
    // Exit on escape
    if ctx.input.key_pressed(KeyCode::Escape) {
      return SceneAction::Quit;
    }

    // Preset switching
    if ctx.input.key_pressed(KeyCode::Digit1) {
      self.current_preset = TrailPreset::Bullet;
      self.recreate_trails(ctx);
      println!("Preset: Bullet");
    }
    if ctx.input.key_pressed(KeyCode::Digit2) {
      self.current_preset = TrailPreset::Laser;
      self.recreate_trails(ctx);
      println!("Preset: Laser");
    }
    if ctx.input.key_pressed(KeyCode::Digit3) {
      self.current_preset = TrailPreset::Swipe;
      self.recreate_trails(ctx);
      println!("Preset: Swipe");
    }

    // Fade mode switching (Q/W/E/D)
    if ctx.input.key_pressed(KeyCode::KeyQ) {
      self.fade_mode = FadeMode::Position;
      self.recreate_trails(ctx);
      println!("Fade: Position (index-based gradient)");
    }
    if ctx.input.key_pressed(KeyCode::KeyW) {
      self.fade_mode = FadeMode::Time;
      self.recreate_trails(ctx);
      println!("Fade: Time (age-based, lifetime={:.1}s)", self.lifetime);
    }
    if ctx.input.key_pressed(KeyCode::KeyD) {
      self.fade_mode = FadeMode::Distance;
      self.recreate_trails(ctx);
      println!("Fade: Distance (pixel-based, {:.0}px)", self.fade_distance);
    }
    if ctx.input.key_pressed(KeyCode::KeyE) {
      self.fade_mode = FadeMode::None;
      self.recreate_trails(ctx);
      println!("Fade: None");
    }

    // Length adjustment
    if ctx.input.key_pressed(KeyCode::ArrowUp) {
      self.trail_length = (self.trail_length + 10).min(100);
      self.recreate_trails(ctx);
      println!("Length: {}", self.trail_length);
    }
    if ctx.input.key_pressed(KeyCode::ArrowDown) {
      self.trail_length = (self.trail_length.saturating_sub(10)).max(5);
      self.recreate_trails(ctx);
      println!("Length: {}", self.trail_length);
    }

    // Width adjustment
    if ctx.input.key_pressed(KeyCode::ArrowRight) {
      self.trail_width = (self.trail_width + 2.0).min(30.0);
      self.recreate_trails(ctx);
      println!("Width: {:.1}", self.trail_width);
    }
    if ctx.input.key_pressed(KeyCode::ArrowLeft) {
      self.trail_width = (self.trail_width - 2.0).max(2.0);
      self.recreate_trails(ctx);
      println!("Width: {:.1}", self.trail_width);
    }

    // +/- adjustment (Lifetime in Time mode, Fade Distance in Distance mode)
    if ctx.input.key_pressed(KeyCode::Equal) || ctx.input.key_pressed(KeyCode::Minus) {
      let is_plus = ctx.input.key_pressed(KeyCode::Equal);
      match self.fade_mode {
        FadeMode::Time => {
          if is_plus {
            self.lifetime = (self.lifetime + 0.1).min(2.0);
          } else {
            self.lifetime = (self.lifetime - 0.1).max(0.1);
          }
          self.recreate_trails(ctx);
          println!("Lifetime: {:.1}s", self.lifetime);
        }
        FadeMode::Distance => {
          if is_plus {
            self.fade_distance = (self.fade_distance + 20.0).min(500.0);
          } else {
            self.fade_distance = (self.fade_distance - 20.0).max(20.0);
          }
          self.recreate_trails(ctx);
          println!("Fade Distance: {:.0}px", self.fade_distance);
        }
        _ => {
          println!("+/- adjusts Lifetime (Time mode) or Fade Distance (Distance mode)");
        }
      }
    }

    // Pause/resume
    if ctx.input.key_pressed(KeyCode::Space) {
      self.paused = !self.paused;
      println!("{}", if self.paused { "PAUSED" } else { "RESUMED" });
    }

    // Reset
    if ctx.input.key_pressed(KeyCode::KeyR) {
      self.reset_trails(ctx);
      println!("Trails reset");
    }

    // Textured trail mode cycling (T cycles: Stretch -> Tile -> Off -> Stretch...)
    if ctx.input.key_pressed(KeyCode::KeyT) {
      match self.texture_mode {
        Some(TextureMode::Stretch) => {
          // Switch to Tile mode
          self.texture_mode = Some(TextureMode::Tile);
          self.recreate_trails(ctx);
          println!("Textured trail: TILE mode (texture repeats every 32px)");
        }
        Some(TextureMode::Tile) => {
          // Turn off textured trail
          self.despawn_textured_trail(ctx);
          self.texture_mode = None;
          println!("Textured trail: OFF");
        }
        None => {
          // Turn on with Stretch mode
          self.texture_mode = Some(TextureMode::Stretch);
          self.spawn_textured_trail(ctx);
          println!("Textured trail: STRETCH mode (texture spans entire trail)");
        }
      }
    }

    // Update entities
    self.update_mouse_trail(ctx, dt);
    self.update_orbits(ctx, dt);
    self.update_textured_trail(ctx, dt);

    SceneAction::Continue
  }

  fn render_ui(&mut self, ui: &mut UiContext) {
    let line_height = 16.0;
    let mut y = 10.0;
    let text_white = Color::WHITE;
    let text_gray = Color::rgb(200, 200, 200);
    let text_muted = Color::rgb(150, 150, 150);

    // Title
    ui.draw_text("TRAILS DEMO", 10.0, y, text_white);
    y += line_height + 5.0;

    // Current settings
    ui.draw_text(
      &format!("Preset: {} (1/2/3)", self.current_preset.name()),
      10.0,
      y,
      text_gray,
    );
    y += line_height;

    ui.draw_text(
      &format!("Fade: {} (Q/W/D/E)", self.fade_mode.name()),
      10.0,
      y,
      text_gray,
    );
    y += line_height;

    ui.draw_text(
      &format!("Length: {} (Up/Down)", self.trail_length),
      10.0,
      y,
      text_gray,
    );
    y += line_height;

    ui.draw_text(
      &format!("Width: {:.1} (Left/Right)", self.trail_width),
      10.0,
      y,
      text_gray,
    );
    y += line_height;

    if self.fade_mode == FadeMode::Time {
      ui.draw_text(
        &format!("Lifetime: {:.1}s (+/-)", self.lifetime),
        10.0,
        y,
        text_gray,
      );
      y += line_height;
    }

    if self.fade_mode == FadeMode::Distance {
      ui.draw_text(
        &format!("Fade Dist: {:.0}px (+/-)", self.fade_distance),
        10.0,
        y,
        text_gray,
      );
      y += line_height;
    }

    y += 10.0;

    // Textured trail status
    let texture_text = match self.texture_mode {
      Some(TextureMode::Stretch) => "Textured: STRETCH (T to cycle)",
      Some(TextureMode::Tile) => "Textured: TILE (T to cycle)",
      None => "Textured: OFF (T to enable)",
    };
    ui.draw_text(texture_text, 10.0, y, Color::rgb(255, 200, 100));
    y += line_height + 5.0;

    // Orbit colors
    ui.draw_text("Orbiting trails (same settings):", 10.0, y, text_muted);
    y += line_height;

    ui.draw_text("  Red | Green | Yellow | Purple", 10.0, y, text_muted);
    y += line_height + 10.0;

    // Controls
    ui.draw_text("Space: Pause | R: Reset | +/-: Adjust", 10.0, y, text_muted);

    // Paused indicator
    if self.paused {
      ui.draw_text(
        "PAUSED",
        self.screen_width / 2.0 - 30.0,
        self.screen_height / 2.0,
        Color::rgb(255, 255, 100),
      );
    }
  }
}

fn main() {
  let engine = EngineBuilder::new()
    .title("Trails Demo - Ryo2D")
    .resolution(1024, 768)
    .window_mode(WindowMode::ResizableAspectRatio)
    .fps(60)
    .physics_hz(60)
    .with_scene(TrailsDemoScene::new())
    .build();

  if let Err(e) = engine.run() {
    eprintln!("Engine error: {}", e);
  }
}
