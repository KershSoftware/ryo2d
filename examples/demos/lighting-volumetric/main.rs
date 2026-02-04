//! Volumetric Lighting Demo
//!
//! Showcases advanced lighting features in Ryo2D:
//!
//! - **God Rays**: Light shafts radiating from bright sources
//! - **Spotlights**: Directional cone lights with soft edges
//! - **Volumetric Beams**: Visible light cones in dusty atmosphere
//! - **Atmosphere Presets**: Clear, hazy, dusty, foggy, smoky, underwater
//! - **Light Presets**: Torch, flashlight, moonlight, lamp, warning, magic
//!
//! ## Controls
//!
//! - **1-6**: Switch atmosphere preset (Clear, Hazy, Dusty, Foggy, Smoky, Underwater)
//! - **Q/E**: Rotate spotlight left/right
//! - **W/S**: Increase/decrease spotlight angle (cone width)
//! - **A/D**: Move spotlight left/right
//! - **Space**: Toggle god rays on/off
//! - **Tab**: Cycle through light color presets
//! - **Arrow Up/Down**: Adjust god rays intensity
//! - **Escape**: Quit

use ryo2d::prelude::*;

/// Current atmosphere preset
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AtmospherePreset {
  Clear,
  Hazy,
  Dusty,
  Foggy,
  Smoky,
  Underwater,
}

impl AtmospherePreset {
  fn name(&self) -> &'static str {
    match self {
      Self::Clear => "Clear",
      Self::Hazy => "Hazy",
      Self::Dusty => "Dusty",
      Self::Foggy => "Foggy",
      Self::Smoky => "Smoky",
      Self::Underwater => "Underwater",
    }
  }

  /// Get the Atmosphere settings for this preset
  fn atmosphere(&self) -> Atmosphere {
    match self {
      Self::Clear => Atmosphere::clear(),
      Self::Hazy => Atmosphere::hazy(),
      Self::Dusty => Atmosphere::dusty(),
      Self::Foggy => Atmosphere::foggy(),
      Self::Smoky => Atmosphere::smoky(),
      Self::Underwater => Atmosphere::underwater(),
    }
  }

  /// Get ambient light settings for this atmosphere
  fn ambient(&self) -> (Color, f32) {
    match self {
      // (color, intensity)
      Self::Clear => (Color::new(20, 20, 35, 255), 0.08),
      Self::Hazy => (Color::new(40, 40, 50, 255), 0.12),
      Self::Dusty => (Color::new(50, 45, 35, 255), 0.10),
      Self::Foggy => (Color::new(60, 65, 70, 255), 0.20),
      Self::Smoky => (Color::new(40, 35, 30, 255), 0.08),
      Self::Underwater => (Color::new(30, 50, 70, 255), 0.15),
    }
  }

  /// Get god rays intensity multiplier for this atmosphere
  fn god_rays_multiplier(&self) -> f32 {
    match self {
      Self::Clear => 0.3,
      Self::Hazy => 0.5,
      Self::Dusty => 0.8,
      Self::Foggy => 1.0,
      Self::Smoky => 0.9,
      Self::Underwater => 0.6,
    }
  }
}

/// Light color preset
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LightPreset {
  Warm,    // Torch-like warm orange
  Cool,    // Moonlight blue
  White,   // Pure white
  Warning, // Red warning
  Magic,   // Purple magic
  Nature,  // Green nature
}

impl LightPreset {
  fn name(&self) -> &'static str {
    match self {
      Self::Warm => "Warm (Torch)",
      Self::Cool => "Cool (Moonlight)",
      Self::White => "White (Lamp)",
      Self::Warning => "Red (Warning)",
      Self::Magic => "Purple (Magic)",
      Self::Nature => "Green (Nature)",
    }
  }

  fn color(&self) -> Color {
    match self {
      Self::Warm => Color::new(255, 200, 100, 255),
      Self::Cool => Color::new(150, 180, 255, 255),
      Self::White => Color::new(255, 255, 255, 255),
      Self::Warning => Color::new(255, 80, 80, 255),
      Self::Magic => Color::new(200, 100, 255, 255),
      Self::Nature => Color::new(100, 255, 150, 255),
    }
  }

  fn next(&self) -> Self {
    match self {
      Self::Warm => Self::Cool,
      Self::Cool => Self::White,
      Self::White => Self::Warning,
      Self::Warning => Self::Magic,
      Self::Magic => Self::Nature,
      Self::Nature => Self::Warm,
    }
  }
}

struct VolumetricLightingScene {
  // Scene dimensions
  screen_w: f32,
  screen_h: f32,

  // Main spotlight (player-controlled)
  spotlight_entity: Option<Entity>,
  spotlight_x: f32,
  spotlight_direction: f32, // degrees
  spotlight_angle: f32,     // cone angle in degrees

  // God rays sun light (upper area)
  sun_entity: Option<Entity>,
  sun_x: f32,
  sun_y: f32,
  god_rays_enabled: bool,
  god_rays_intensity: f32,

  // Secondary lights for atmosphere
  torch_entities: Vec<Entity>,

  // Current presets
  atmosphere_preset: AtmospherePreset,
  light_preset: LightPreset,

  // Animation time
  time: f32,
}

impl VolumetricLightingScene {
  fn new() -> Self {
    Self {
      screen_w: 1280.0,
      screen_h: 720.0,

      spotlight_entity: None,
      spotlight_x: 640.0,
      spotlight_direction: 270.0, // pointing up
      spotlight_angle: 35.0,

      sun_entity: None,
      sun_x: 200.0,
      sun_y: 80.0,
      god_rays_enabled: true,
      god_rays_intensity: 0.4,

      torch_entities: Vec::new(),

      atmosphere_preset: AtmospherePreset::Dusty,
      light_preset: LightPreset::Warm,

      time: 0.0,
    }
  }

  fn apply_atmosphere(&mut self, ctx: &mut SceneContext) {
    // Apply atmosphere and ambient light based on preset
    let (ambient_color, ambient_intensity) = self.atmosphere_preset.ambient();
    let atmosphere = self.atmosphere_preset.atmosphere();
    if let Some(lighting) = ctx.lighting {
      lighting.set_ambient(AmbientLight::new(ambient_color, ambient_intensity));
      lighting.set_atmosphere(atmosphere);
    }

    // Apply the current atmosphere preset to god rays effect
    if self.god_rays_enabled {
      // Get light positions for god rays
      let mut light_positions = Vec::new();

      // Add sun position if it exists
      if let Some(sun_entity) = self.sun_entity {
        if let Some(transform) = ctx.world.get_component::<Transform>(sun_entity) {
          // Convert world position to screen UV (approximate)
          let uv_x = transform.x / self.screen_w;
          let uv_y = transform.y / self.screen_h;
          light_positions.push((uv_x, uv_y));
        }
      }

      if !light_positions.is_empty() {
        let intensity = self.god_rays_intensity * self.atmosphere_preset.god_rays_multiplier();
        ctx.effects.set_god_rays(intensity, light_positions);
      }
    } else {
      ctx.effects.disable_god_rays();
    }
  }

  fn update_spotlight(&mut self, ctx: &mut SceneContext) {
    if let Some(entity) = self.spotlight_entity {
      // Update spotlight properties
      if let Some(mut light) = ctx.world.get_mut::<Light2D>(entity) {
        *light = Light2D::spot(self.light_preset.color(), 400.0, self.spotlight_angle, self.spotlight_direction)
          .with_intensity(1.2)
          .with_volumetric();
      }

      // Update position
      if let Some(mut transform) = ctx.world.get_mut::<Transform>(entity) {
        transform.x = self.spotlight_x;
      }
    }
  }

  fn update_sun(&mut self, ctx: &mut SceneContext) {
    if let Some(entity) = self.sun_entity {
      // Gentle horizontal sway
      let sway = (self.time * 0.3).sin() * 50.0;

      if let Some(mut transform) = ctx.world.get_mut::<Transform>(entity) {
        transform.x = 200.0 + sway;
        self.sun_x = transform.x;
      }

      // Update sun light with god rays if enabled
      if let Some(mut light) = ctx.world.get_mut::<Light2D>(entity) {
        if self.god_rays_enabled {
          *light = Light2D::point(Color::new(255, 250, 220, 255), 300.0)
            .with_intensity(1.5)
            .with_god_rays();
        } else {
          *light = Light2D::point(Color::new(255, 250, 220, 255), 300.0).with_intensity(1.5);
        }
      }
    }
  }

  fn update_torches(&mut self, ctx: &mut SceneContext) {
    // Flicker the torches
    for (i, &entity) in self.torch_entities.iter().enumerate() {
      if let Some(mut light) = ctx.world.get_mut::<Light2D>(entity) {
        let phase = i as f32 * 1.7; // offset each torch
        let flicker = 0.7 + 0.3 * (self.time * 8.0 + phase).sin() * (self.time * 12.0 + phase * 2.0).cos();
        light.set_intensity(flicker);
      }
    }
  }
}

impl Scene for VolumetricLightingScene {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    let (w, h) = ctx.screen_size();
    self.screen_w = w;
    self.screen_h = h;

    // Configure lighting - UI should remain visible
    if let Some(lighting) = ctx.lighting {
      lighting.set_ui_affected_by_lighting(false);
      // Very dark ambient for dramatic effect
      lighting.set_ambient(AmbientLight::new(Color::new(15, 15, 25, 255), 0.05));
    }

    // Create ground/floor
    let ground_sprite = ctx
      .assets
      .create_sprite(Sprite::solid_rect(1280, 100, 40, 35, 30, 255), "ground");
    ctx.world.spawn((
      Transform::new(640.0, 670.0).with_pivot_center(),
      Renderable::sprite(ground_sprite).with_z_order(1),
      LightOccluder2D::rect(1280.0, 100.0),
    ));

    // Create some pillars/obstacles
    let pillar_positions = [(300.0, 500.0), (500.0, 450.0), (800.0, 480.0), (1000.0, 520.0)];

    for (i, (x, y)) in pillar_positions.iter().enumerate() {
      let height = 150.0 + (i as f32 * 30.0);
      let pillar_sprite = ctx.assets.create_sprite(
        Sprite::solid_rect(40, height as u32, 60, 55, 50, 255),
        format!("pillar_{}", i),
      );
      ctx.world.spawn((
        Transform::new(*x, *y).with_pivot_bottom_center(),
        Renderable::sprite(pillar_sprite).with_z_order(2),
        LightOccluder2D::rect(40.0, height),
      ));
    }

    // Create floating platforms
    let platform_positions = [(200.0, 350.0, 120.0), (600.0, 280.0, 150.0), (1000.0, 320.0, 100.0)];

    for (i, (x, y, width)) in platform_positions.iter().enumerate() {
      let platform_sprite = ctx.assets.create_sprite(
        Sprite::solid_rect(*width as u32, 20, 70, 65, 60, 255),
        format!("platform_{}", i),
      );
      ctx.world.spawn((
        Transform::new(*x, *y).with_pivot_center(),
        Renderable::sprite(platform_sprite).with_z_order(2),
        LightOccluder2D::rect(*width, 20.0),
      ));
    }

    // Create the main spotlight (player-controlled)
    self.spotlight_entity = Some(ctx.world.spawn((
      Transform::new(self.spotlight_x, 620.0),
      Light2D::spot(self.light_preset.color(), 400.0, self.spotlight_angle, self.spotlight_direction)
        .with_intensity(1.2)
        .with_volumetric(),
    )));

    // Create a visible sprite for the spotlight source
    let spotlight_base = ctx
      .assets
      .create_sprite(Sprite::solid_rect(30, 20, 80, 80, 80, 255), "spotlight_base");
    ctx.world.spawn((
      Transform::new(self.spotlight_x, 610.0).with_pivot_center(),
      Renderable::sprite(spotlight_base).with_z_order(5),
    ));

    // Create the "sun" light source (for god rays)
    self.sun_entity = Some(ctx.world.spawn((
      Transform::new(self.sun_x, self.sun_y),
      Light2D::point(Color::new(255, 250, 220, 255), 300.0)
        .with_intensity(1.5)
        .with_god_rays(),
    )));

    // Sun visual (bright circle)
    let sun_sprite = ctx
      .assets
      .create_sprite(Sprite::solid_circle(60, 255, 250, 200, 255), "sun");
    ctx.world.spawn((
      Transform::new(self.sun_x, self.sun_y).with_pivot_center(),
      Renderable::sprite(sun_sprite).with_z_order(0),
    ));

    // Create wall-mounted torches
    let torch_positions = [(100.0, 400.0), (1180.0, 350.0), (640.0, 150.0)];

    for (i, (x, y)) in torch_positions.iter().enumerate() {
      // Torch bracket
      let bracket = ctx.assets.create_sprite(
        Sprite::solid_rect(15, 25, 100, 80, 50, 255),
        format!("torch_bracket_{}", i),
      );
      ctx.world.spawn((
        Transform::new(*x, *y).with_pivot_center(),
        Renderable::sprite(bracket).with_z_order(3),
      ));

      // Torch flame (visual)
      let flame = ctx.assets.create_sprite(
        Sprite::solid_circle(16, 255, 150, 50, 200),
        format!("torch_flame_{}", i),
      );
      ctx.world.spawn((
        Transform::new(*x, y - 20.0).with_pivot_center(),
        Renderable::sprite(flame).with_z_order(4),
      ));

      // Torch light
      let torch_entity = ctx.world.spawn((
        Transform::new(*x, y - 20.0),
        Light2D::torch(120.0).with_intensity(0.8),
      ));
      self.torch_entities.push(torch_entity);
    }

    // Apply initial atmosphere
    self.apply_atmosphere(ctx);
  }

  fn update(&mut self, ctx: &mut SceneContext, dt: f32) -> SceneAction {
    self.time += dt;

    // Quit on Escape
    if ctx.input.key_pressed(KeyCode::Escape) {
      return SceneAction::Quit;
    }

    // Atmosphere preset selection (1-6)
    if ctx.input.key_pressed(KeyCode::Digit1) {
      self.atmosphere_preset = AtmospherePreset::Clear;
    }
    if ctx.input.key_pressed(KeyCode::Digit2) {
      self.atmosphere_preset = AtmospherePreset::Hazy;
    }
    if ctx.input.key_pressed(KeyCode::Digit3) {
      self.atmosphere_preset = AtmospherePreset::Dusty;
    }
    if ctx.input.key_pressed(KeyCode::Digit4) {
      self.atmosphere_preset = AtmospherePreset::Foggy;
    }
    if ctx.input.key_pressed(KeyCode::Digit5) {
      self.atmosphere_preset = AtmospherePreset::Smoky;
    }
    if ctx.input.key_pressed(KeyCode::Digit6) {
      self.atmosphere_preset = AtmospherePreset::Underwater;
    }

    // Toggle god rays
    if ctx.input.key_pressed(KeyCode::Space) {
      self.god_rays_enabled = !self.god_rays_enabled;
    }

    // Adjust god rays intensity
    if ctx.input.key_down(KeyCode::ArrowUp) {
      self.god_rays_intensity = (self.god_rays_intensity + dt).min(2.0);
    }
    if ctx.input.key_down(KeyCode::ArrowDown) {
      self.god_rays_intensity = (self.god_rays_intensity - dt).max(0.1);
    }

    // Cycle light color preset
    if ctx.input.key_pressed(KeyCode::Tab) {
      self.light_preset = self.light_preset.next();
    }

    // Spotlight controls
    // Rotate
    if ctx.input.key_down(KeyCode::KeyQ) {
      self.spotlight_direction = (self.spotlight_direction - 90.0 * dt) % 360.0;
      if self.spotlight_direction < 0.0 {
        self.spotlight_direction += 360.0;
      }
    }
    if ctx.input.key_down(KeyCode::KeyE) {
      self.spotlight_direction = (self.spotlight_direction + 90.0 * dt) % 360.0;
    }

    // Adjust cone angle
    if ctx.input.key_down(KeyCode::KeyW) {
      self.spotlight_angle = (self.spotlight_angle + 30.0 * dt).min(90.0);
    }
    if ctx.input.key_down(KeyCode::KeyS) {
      self.spotlight_angle = (self.spotlight_angle - 30.0 * dt).max(10.0);
    }

    // Move spotlight
    if ctx.input.key_down(KeyCode::KeyA) {
      self.spotlight_x = (self.spotlight_x - 200.0 * dt).max(100.0);
    }
    if ctx.input.key_down(KeyCode::KeyD) {
      self.spotlight_x = (self.spotlight_x + 200.0 * dt).min(self.screen_w - 100.0);
    }

    // Update all lights
    self.update_spotlight(ctx);
    self.update_sun(ctx);
    self.update_torches(ctx);

    // Apply atmosphere effects
    self.apply_atmosphere(ctx);

    SceneAction::Continue
  }

  fn render_ui(&mut self, ui: &mut UiContext) {
    let white = Color::WHITE;
    let gray = Color::new(180, 180, 180, 255);
    let highlight = Color::new(255, 255, 100, 255);
    let bg = Some(Color::new(0, 0, 0, 180));

    // Title
    ui.label("Volumetric Lighting Demo", 10.0, 10.0, white, bg);

    // Current settings
    ui.label(
      &format!("Atmosphere: {} (1-6 to change)", self.atmosphere_preset.name()),
      10.0,
      40.0,
      highlight,
      bg,
    );

    ui.label(
      &format!(
        "God Rays: {} (Space to toggle)",
        if self.god_rays_enabled { "ON" } else { "OFF" }
      ),
      10.0,
      65.0,
      if self.god_rays_enabled { highlight } else { gray },
      bg,
    );

    ui.label(
      &format!("God Rays Intensity: {:.1} (Up/Down)", self.god_rays_intensity),
      10.0,
      90.0,
      gray,
      bg,
    );

    ui.label(
      &format!("Light Color: {} (Tab)", self.light_preset.name()),
      10.0,
      115.0,
      self.light_preset.color(),
      bg,
    );

    // Spotlight info
    ui.label("", 10.0, 145.0, white, None); // spacer
    ui.label("Spotlight Controls:", 10.0, 150.0, white, bg);
    ui.label(
      &format!("  Direction: {:.0} deg (Q/E)", self.spotlight_direction),
      10.0,
      175.0,
      gray,
      bg,
    );
    ui.label(
      &format!("  Cone Angle: {:.0} deg (W/S)", self.spotlight_angle),
      10.0,
      200.0,
      gray,
      bg,
    );
    ui.label(&format!("  Position: {:.0} (A/D)", self.spotlight_x), 10.0, 225.0, gray, bg);

    // Instructions at bottom
    ui.label("ESC to quit", 10.0, self.screen_h - 30.0, gray, bg);

    // Feature list on right side
    let right_x = self.screen_w - 250.0;
    ui.label("Features Shown:", right_x, 10.0, white, bg);
    ui.label("- God rays post-effect", right_x, 35.0, gray, bg);
    ui.label("- Volumetric spotlight", right_x, 55.0, gray, bg);
    ui.label("- Flickering torches", right_x, 75.0, gray, bg);
    ui.label("- Atmosphere presets", right_x, 95.0, gray, bg);
    ui.label("- Light color presets", right_x, 115.0, gray, bg);
    ui.label("- Shadow occluders", right_x, 135.0, gray, bg);
  }
}

fn main() {
  let engine = EngineBuilder::new()
    .title("Volumetric Lighting Demo - Ryo2D")
    .resolution(1280, 720)
    .fps(60)
    .with_scene(VolumetricLightingScene::new())
    .build();

  if let Err(e) = engine.run() {
    eprintln!("Engine error: {}", e);
    std::process::exit(1);
  }
}
