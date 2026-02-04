//! Trails Demo - Ryo2D Example
//!
//! This example demonstrates the trail rendering system:
//! - Position-based fade (fades from head to tail based on position)
//! - Time-based fade (fades based on point age with lifetime)
//! - Different trail presets (bullet, laser, swipe)
//! - Configurable parameters (width, length, taper, color)
//!
//! ## Controls
//!
//! - Mouse: Control the center trail
//! - 1/2/3: Switch trail preset (Bullet / Laser / Swipe)
//! - Q/W/E: Toggle position fade / time fade / no fade
//! - Up/Down: Adjust trail length
//! - Left/Right: Adjust trail width
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
  None,     // No fade
}

impl FadeMode {
  fn name(&self) -> &'static str {
    match self {
      FadeMode::Position => "Position (head-to-tail)",
      FadeMode::Time => "Time (age-based)",
      FadeMode::None => "None",
    }
  }
}

/// Main demo scene
struct TrailsDemoScene {
  mouse_entity: Option<Entity>,
  orbit_entities: Vec<Entity>,
  paused: bool,
  current_preset: TrailPreset,
  fade_mode: FadeMode,
  trail_length: usize,
  trail_width: f32,
  lifetime: f32,
  screen_width: f32,
  screen_height: f32,
}

impl TrailsDemoScene {
  fn new() -> Self {
    Self {
      mouse_entity: None,
      orbit_entities: Vec::new(),
      paused: false,
      current_preset: TrailPreset::Bullet,
      fade_mode: FadeMode::Position,
      trail_length: 30,
      trail_width: 6.0,
      lifetime: 0.5,
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

  /// Spawn the mouse-controlled trail entity
  fn spawn_mouse_trail(&mut self, ctx: &mut SceneContext) {
    let (mx, my) = ctx.input.mouse_position();
    let color = Color::rgb(100, 200, 255);

    let entity = ctx.world.spawn((
      Transform::new(mx, my),
      Renderable::sprite(Self::create_circle(8, color)),
      self.create_trail(color),
      MouseTrail,
    ));

    self.mouse_entity = Some(entity);
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
        Transform::new(x, y),
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
      }
    }
  }

  /// Update mouse-controlled entity
  fn update_mouse_trail(&mut self, ctx: &mut SceneContext) {
    if let Some(entity) = self.mouse_entity {
      let (mx, my) = ctx.input.mouse_position();

      if let Some(mut transform) = ctx.world.get_component_mut::<Transform>(entity) {
        transform.x = mx;
        transform.y = my;
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
    println!("Q/W/E: Fade mode (Position/Time/None)");
    println!("Up/Down: Trail length");
    println!("Left/Right: Trail width");
    println!("Space: Pause orbits");
    println!("R: Reset trails");
    println!("ESC: Exit");

    // Spawn entities
    self.spawn_mouse_trail(ctx);
    self.spawn_orbit_trails(ctx);
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

    // Fade mode switching
    if ctx.input.key_pressed(KeyCode::KeyQ) {
      self.fade_mode = FadeMode::Position;
      self.recreate_trails(ctx);
      println!("Fade: Position (head-to-tail gradient)");
    }
    if ctx.input.key_pressed(KeyCode::KeyW) {
      self.fade_mode = FadeMode::Time;
      self.recreate_trails(ctx);
      println!("Fade: Time (age-based, lifetime={:.1}s)", self.lifetime);
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

    // Lifetime adjustment (only works in Time fade mode)
    if ctx.input.key_pressed(KeyCode::Equal) || ctx.input.key_pressed(KeyCode::Minus) {
      if self.fade_mode == FadeMode::Time {
        if ctx.input.key_pressed(KeyCode::Equal) {
          self.lifetime = (self.lifetime + 0.1).min(2.0);
        } else {
          self.lifetime = (self.lifetime - 0.1).max(0.1);
        }
        self.recreate_trails(ctx);
        println!("Lifetime: {:.1}s", self.lifetime);
      } else {
        println!("Lifetime only works in Time fade mode (press W)");
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

    // Update entities
    self.update_mouse_trail(ctx);
    self.update_orbits(ctx, dt);

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
      &format!("Fade: {} (Q/W/E)", self.fade_mode.name()),
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

    y += 10.0;

    // Orbit colors
    ui.draw_text("Orbiting trails (same settings):", 10.0, y, text_muted);
    y += line_height;

    ui.draw_text("  Red | Green | Yellow | Purple", 10.0, y, text_muted);
    y += line_height + 10.0;

    // Controls
    ui.draw_text(
      "Space: Pause | R: Reset | +/-: Lifetime",
      10.0,
      y,
      text_muted,
    );

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
