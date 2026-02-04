//! Volumetric Lighting Demo
//!
//! Demonstrates volumetric lighting effects in Ryo2D:
//!
//! - **God Rays** - Screen-space radial light shafts from bright sources
//! - **Flashlight Beam** - Visible spotlight cone in dusty atmosphere
//! - **Atmospheric Fog** - Global fog that scatters light
//!
//! Controls:
//! - 1/2/3: Switch between demo scenes
//! - Arrow Up/Down: Adjust god rays intensity
//! - Arrow Left/Right: Adjust number of samples
//! - Q/E: Adjust decay rate
//! - W/S: Adjust threshold
//! - Space: Toggle god rays on/off
//!
//! Flashlight Scene (Scene 2) Additional Controls:
//! - R/F: Adjust beam intensity
//! - T/G: Adjust beam density
//! - Y/H: Adjust beam scattering
//! - Mouse: Aim flashlight
//!
//! - Escape: Quit

use ryo2d::prelude::*;
use std::f32::consts::PI;

/// Current demo scene
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DemoScene {
  GodRays,
  Flashlight,
  Atmosphere,
}

impl DemoScene {
  fn name(&self) -> &'static str {
    match self {
      Self::GodRays => "God Rays",
      Self::Flashlight => "Flashlight Beam",
      Self::Atmosphere => "Atmospheric Fog",
    }
  }
}

/// Main volumetric demo
struct VolumetricDemo {
  // Current scene
  scene: DemoScene,
  time: f32,

  // God rays settings
  god_rays_enabled: bool,
  god_rays_intensity: f32,
  god_rays_samples: u32,
  god_rays_decay: f32,
  god_rays_threshold: f32,

  // Volumetric beam settings
  beam_intensity: f32,
  beam_density: f32,
  beam_scattering: f32,

  // Light entities
  sun_light: Option<Entity>,
  spotlight: Option<Entity>,
  ambient_lights: Vec<Entity>,
}

impl VolumetricDemo {
  fn new() -> Self {
    Self {
      scene: DemoScene::GodRays,
      time: 0.0,
      god_rays_enabled: true,
      god_rays_intensity: 0.8,
      god_rays_samples: 16,
      god_rays_decay: 0.96,
      god_rays_threshold: 0.4,
      beam_intensity: 1.0,
      beam_density: 0.8,
      beam_scattering: 0.5,
      sun_light: None,
      spotlight: None,
      ambient_lights: Vec::new(),
    }
  }

  fn setup_god_rays_scene(&mut self, ctx: &mut SceneContext) {
    // Clear existing lights
    self.clear_lights(ctx);

    // Create a bright "sun" light positioned off-screen (partially visible)
    // This creates god rays emanating from outside the frame
    let sun = ctx.world.spawn((
      Transform::new(150.0, 100.0),
      Light2D::point(Color::new(255, 250, 230, 255), 400.0)
        .with_intensity(2.0)
        .with_god_rays(),
    ));
    self.sun_light = Some(sun);

    // Create a window frame to partially occlude the light
    let window_frame = ctx.assets.create_sprite(
      Sprite::solid_rect(200, 300, 40, 35, 30, 255),
      "window_frame",
    );

    ctx.world.spawn((
      Transform::new(150.0, 200.0).with_pivot_center(),
      Renderable::sprite(window_frame).with_z_order(5),
      LightOccluder2D::rect(200.0, 300.0),
    ));

    // Add some interior objects that cast shadows and create interesting ray patterns
    let pillar_positions = [(400.0, 300.0), (700.0, 400.0), (1000.0, 250.0)];

    for (i, (x, y)) in pillar_positions.iter().enumerate() {
      let sprite = ctx.assets.create_sprite(
        Sprite::solid_rect(60, 150, 60, 55, 50, 255),
        format!("pillar_{}", i),
      );

      ctx.world.spawn((
        Transform::new(*x, *y).with_pivot_center(),
        Renderable::sprite(sprite).with_z_order(5),
        LightOccluder2D::rect(60.0, 150.0),
      ));
    }

    // Floor
    let floor = ctx
      .assets
      .create_sprite(Sprite::solid_rect(1280, 150, 50, 45, 40, 255), "floor");

    ctx.world.spawn((
      Transform::new(640.0, 645.0).with_pivot_center(),
      Renderable::sprite(floor).with_z_order(3),
      LightOccluder2D::rect(1280.0, 150.0),
    ));

    // Set very dark ambient so god rays are visible
    if let Some(lighting) = ctx.lighting {
      lighting.set_ambient(AmbientLight::new(Color::new(10, 10, 15, 255), 0.05));
      lighting.set_ui_affected_by_lighting(false);
    }
  }

  fn setup_flashlight_scene(&mut self, ctx: &mut SceneContext) {
    // Clear existing lights
    self.clear_lights(ctx);

    // Create a flashlight that the user can move
    // Use custom volumetric settings for the visible beam
    let vol_settings =
      VolumetricSettings::new(self.beam_intensity, self.beam_density, self.beam_scattering)
        .with_god_rays();

    let flashlight = ctx.world.spawn((
      Transform::new(640.0, 360.0),
      Light2D::flashlight(350.0, 0.0)
        .with_intensity(1.5)
        .with_volumetric_settings(vol_settings),
    ));
    self.spotlight = Some(flashlight);

    // Create a dusty room environment
    let room_objects = [
      (200.0, 500.0, 100.0, 80.0, Color::new(120, 80, 60, 255)),
      (500.0, 450.0, 80.0, 120.0, Color::new(80, 100, 70, 255)),
      (900.0, 520.0, 120.0, 60.0, Color::new(100, 70, 90, 255)),
      (1100.0, 400.0, 60.0, 180.0, Color::new(70, 80, 100, 255)),
    ];

    for (i, (x, y, w, h, color)) in room_objects.iter().enumerate() {
      let sprite = ctx.assets.create_sprite(
        Sprite::solid_rect(*w as u32, *h as u32, color.r, color.g, color.b, color.a),
        format!("room_object_{}", i),
      );

      ctx.world.spawn((
        Transform::new(*x, *y).with_pivot_center(),
        Renderable::sprite(sprite).with_z_order(5),
        LightOccluder2D::rect(*w, *h),
      ));
    }

    // Floor
    let floor = ctx
      .assets
      .create_sprite(Sprite::solid_rect(1280, 100, 45, 40, 38, 255), "floor");

    ctx.world.spawn((
      Transform::new(640.0, 670.0).with_pivot_center(),
      Renderable::sprite(floor).with_z_order(3),
    ));

    // Very dark ambient for dramatic flashlight effect
    if let Some(lighting) = ctx.lighting {
      lighting.set_ambient(AmbientLight::new(Color::new(5, 5, 8, 255), 0.02));
      lighting.set_ui_affected_by_lighting(false);
    }
  }

  fn setup_atmosphere_scene(&mut self, ctx: &mut SceneContext) {
    // Clear existing lights
    self.clear_lights(ctx);

    // Create multiple lights in a foggy environment
    let light_configs = [
      (200.0, 200.0, Color::new(255, 200, 150, 255), 200.0),
      (640.0, 150.0, Color::new(255, 255, 255, 255), 250.0),
      (1080.0, 200.0, Color::new(150, 200, 255, 255), 200.0),
    ];

    for (i, (x, y, color, radius)) in light_configs.iter().enumerate() {
      let light = ctx.world.spawn((
        Transform::new(*x, *y),
        Light2D::point(*color, *radius)
          .with_intensity(1.2)
          .with_god_rays(),
      ));
      self.ambient_lights.push(light);

      // Add a visible light fixture
      let fixture = ctx.assets.create_sprite(
        Sprite::solid_circle(30, color.r, color.g, color.b, 255),
        format!("fixture_{}", i),
      );

      ctx.world.spawn((
        Transform::new(*x, *y).with_pivot_center(),
        Renderable::sprite(fixture).with_z_order(10),
      ));
    }

    // Create fog-obscured environment
    let fog_elements = [
      (300.0, 450.0, 80.0, 200.0),
      (600.0, 500.0, 150.0, 100.0),
      (950.0, 420.0, 100.0, 180.0),
    ];

    for (i, (x, y, w, h)) in fog_elements.iter().enumerate() {
      let sprite = ctx.assets.create_sprite(
        Sprite::solid_rect(*w as u32, *h as u32, 60, 55, 50, 255),
        format!("fog_element_{}", i),
      );

      ctx.world.spawn((
        Transform::new(*x, *y).with_pivot_center(),
        Renderable::sprite(sprite).with_z_order(5),
        LightOccluder2D::rect(*w, *h),
      ));
    }

    // Floor
    let floor = ctx
      .assets
      .create_sprite(Sprite::solid_rect(1280, 120, 50, 48, 45, 255), "floor");

    ctx.world.spawn((
      Transform::new(640.0, 660.0).with_pivot_center(),
      Renderable::sprite(floor).with_z_order(3),
    ));

    // Foggy ambient
    if let Some(lighting) = ctx.lighting {
      lighting.set_ambient(AmbientLight::new(Color::new(40, 45, 55, 255), 0.15));
      lighting.set_ui_affected_by_lighting(false);
    }
  }

  fn clear_lights(&mut self, ctx: &mut SceneContext) {
    if let Some(entity) = self.sun_light.take() {
      let _ = ctx.world.despawn(entity);
    }
    if let Some(entity) = self.spotlight.take() {
      let _ = ctx.world.despawn(entity);
    }
    for entity in self.ambient_lights.drain(..) {
      let _ = ctx.world.despawn(entity);
    }
  }

  fn get_light_screen_positions(&self, ctx: &SceneContext) -> Vec<(f32, f32)> {
    let mut positions = Vec::new();
    let screen_width = 1280.0;
    let screen_height = 720.0;

    // Helper to convert world position to UV
    let world_to_uv = |transform: &Transform| -> (f32, f32) {
      let u = transform.x / screen_width;
      let v = transform.y / screen_height;
      (u.clamp(0.0, 1.0), v.clamp(0.0, 1.0))
    };

    if let Some(entity) = self.sun_light {
      if let Some(transform) = ctx.world.get::<Transform>(entity) {
        positions.push(world_to_uv(&transform));
      }
    }

    if let Some(entity) = self.spotlight {
      if let Some(transform) = ctx.world.get::<Transform>(entity) {
        positions.push(world_to_uv(&transform));
      }
    }

    for &entity in &self.ambient_lights {
      if let Some(transform) = ctx.world.get::<Transform>(entity) {
        positions.push(world_to_uv(&transform));
      }
    }

    positions
  }
}

impl Scene for VolumetricDemo {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    self.setup_god_rays_scene(ctx);
  }

  fn update(&mut self, ctx: &mut SceneContext, dt: f32) -> SceneAction {
    self.time += dt;

    // Handle input
    if ctx.input.key_pressed(KeyCode::Escape) {
      return SceneAction::Quit;
    }

    // Scene switching
    let prev_scene = self.scene;
    if ctx.input.key_pressed(KeyCode::Digit1) {
      self.scene = DemoScene::GodRays;
    }
    if ctx.input.key_pressed(KeyCode::Digit2) {
      self.scene = DemoScene::Flashlight;
    }
    if ctx.input.key_pressed(KeyCode::Digit3) {
      self.scene = DemoScene::Atmosphere;
    }

    // Setup new scene if changed
    if self.scene != prev_scene {
      match self.scene {
        DemoScene::GodRays => self.setup_god_rays_scene(ctx),
        DemoScene::Flashlight => self.setup_flashlight_scene(ctx),
        DemoScene::Atmosphere => self.setup_atmosphere_scene(ctx),
      }
    }

    // Toggle god rays
    if ctx.input.key_pressed(KeyCode::Space) {
      self.god_rays_enabled = !self.god_rays_enabled;
    }

    // Adjust god rays intensity
    if ctx.input.key_down(KeyCode::ArrowUp) {
      self.god_rays_intensity = (self.god_rays_intensity + 0.02).min(2.0);
    }
    if ctx.input.key_down(KeyCode::ArrowDown) {
      self.god_rays_intensity = (self.god_rays_intensity - 0.02).max(0.0);
    }

    // Adjust samples
    if ctx.input.key_pressed(KeyCode::ArrowRight) {
      self.god_rays_samples = (self.god_rays_samples + 4).min(64);
    }
    if ctx.input.key_pressed(KeyCode::ArrowLeft) {
      self.god_rays_samples = self.god_rays_samples.saturating_sub(4).max(4);
    }

    // Adjust decay
    if ctx.input.key_down(KeyCode::KeyE) {
      self.god_rays_decay = (self.god_rays_decay + 0.002).min(0.999);
    }
    if ctx.input.key_down(KeyCode::KeyQ) {
      self.god_rays_decay = (self.god_rays_decay - 0.002).max(0.8);
    }

    // Adjust threshold
    if ctx.input.key_down(KeyCode::KeyW) {
      self.god_rays_threshold = (self.god_rays_threshold + 0.01).min(1.0);
    }
    if ctx.input.key_down(KeyCode::KeyS) {
      self.god_rays_threshold = (self.god_rays_threshold - 0.01).max(0.0);
    }

    // Volumetric beam controls (flashlight scene)
    if self.scene == DemoScene::Flashlight {
      // Adjust beam intensity (R/F)
      if ctx.input.key_down(KeyCode::KeyR) {
        self.beam_intensity = (self.beam_intensity + 0.02).min(2.0);
      }
      if ctx.input.key_down(KeyCode::KeyF) {
        self.beam_intensity = (self.beam_intensity - 0.02).max(0.0);
      }

      // Adjust beam density (T/G)
      if ctx.input.key_down(KeyCode::KeyT) {
        self.beam_density = (self.beam_density + 0.02).min(2.0);
      }
      if ctx.input.key_down(KeyCode::KeyG) {
        self.beam_density = (self.beam_density - 0.02).max(0.0);
      }

      // Adjust beam scattering (Y/H)
      if ctx.input.key_down(KeyCode::KeyY) {
        self.beam_scattering = (self.beam_scattering + 0.02).min(2.0);
      }
      if ctx.input.key_down(KeyCode::KeyH) {
        self.beam_scattering = (self.beam_scattering - 0.02).max(0.0);
      }
    }

    // Update flashlight position and direction (follows mouse)
    if self.scene == DemoScene::Flashlight {
      if let Some(entity) = self.spotlight {
        let mouse_pos = ctx.input.mouse_position();

        if let Some(mut transform) = ctx.world.get_mut::<Transform>(entity) {
          // Calculate direction from center to mouse
          let dx = mouse_pos.0 - 640.0;
          let dy = mouse_pos.1 - 360.0;
          let direction = dy.atan2(dx) * 180.0 / PI;

          // Move light towards mouse but not all the way
          transform.x = 640.0 + dx * 0.3;
          transform.y = 360.0 + dy * 0.3;

          // Update light direction and volumetric settings
          if let Some(mut light) = ctx.world.get_mut::<Light2D>(entity) {
            light.set_direction(direction);
            // Update volumetric beam settings
            let vol_settings =
              VolumetricSettings::new(self.beam_intensity, self.beam_density, self.beam_scattering)
                .with_god_rays();
            light.set_volumetric(Some(vol_settings));
          }
        }
      }
    }

    // Animate sun position slightly (god rays scene)
    if self.scene == DemoScene::GodRays {
      if let Some(entity) = self.sun_light {
        if let Some(mut transform) = ctx.world.get_mut::<Transform>(entity) {
          transform.y = 100.0 + (self.time * 0.3).sin() * 30.0;
        }
      }
    }

    // Update god rays effect
    let light_positions = self.get_light_screen_positions(ctx);

    if self.god_rays_enabled && !light_positions.is_empty() {
      ctx.effects.set_god_rays_custom(
        self.god_rays_intensity,
        self.god_rays_samples,
        self.god_rays_decay,
        1.0,
        self.god_rays_threshold,
        light_positions,
      );
    } else {
      ctx.effects.disable_god_rays();
    }

    SceneAction::Continue
  }

  fn render_ui(&mut self, ui: &mut UiContext) {
    let bg = Some(Color::new(0, 0, 0, 180));

    ui.label(
      &format!("Volumetric Lighting Demo - {}", self.scene.name()),
      10.0,
      10.0,
      Color::WHITE,
      bg,
    );

    ui.label(
      "Press 1/2/3 to switch scenes",
      10.0,
      35.0,
      Color::new(200, 200, 200, 255),
      bg,
    );

    ui.label(
      &format!(
        "God Rays: {} (Space to toggle)",
        if self.god_rays_enabled { "ON" } else { "OFF" }
      ),
      10.0,
      65.0,
      Color::new(255, 255, 100, 255),
      bg,
    );

    ui.label(
      &format!("Intensity: {:.2} (Up/Down)", self.god_rays_intensity),
      10.0,
      90.0,
      Color::new(200, 200, 200, 255),
      bg,
    );

    ui.label(
      &format!("Samples: {} (Left/Right)", self.god_rays_samples),
      10.0,
      115.0,
      Color::new(200, 200, 200, 255),
      bg,
    );

    ui.label(
      &format!("Decay: {:.3} (Q/E)", self.god_rays_decay),
      10.0,
      140.0,
      Color::new(200, 200, 200, 255),
      bg,
    );

    ui.label(
      &format!("Threshold: {:.2} (W/S)", self.god_rays_threshold),
      10.0,
      165.0,
      Color::new(200, 200, 200, 255),
      bg,
    );

    let mut y_offset = 195.0;

    if self.scene == DemoScene::Flashlight {
      ui.label(
        "Move mouse to aim flashlight",
        10.0,
        y_offset,
        Color::new(150, 200, 255, 255),
        bg,
      );
      y_offset += 30.0;

      ui.label(
        "--- Volumetric Beam ---",
        10.0,
        y_offset,
        Color::new(255, 200, 100, 255),
        bg,
      );
      y_offset += 25.0;

      ui.label(
        &format!("Beam Intensity: {:.2} (R/F)", self.beam_intensity),
        10.0,
        y_offset,
        Color::new(200, 200, 200, 255),
        bg,
      );
      y_offset += 25.0;

      ui.label(
        &format!("Beam Density: {:.2} (T/G)", self.beam_density),
        10.0,
        y_offset,
        Color::new(200, 200, 200, 255),
        bg,
      );
      y_offset += 25.0;

      ui.label(
        &format!("Beam Scattering: {:.2} (Y/H)", self.beam_scattering),
        10.0,
        y_offset,
        Color::new(200, 200, 200, 255),
        bg,
      );
      y_offset += 25.0;
    }

    ui.label(
      "Press ESC to quit",
      10.0,
      y_offset,
      Color::new(150, 150, 150, 255),
      bg,
    );
  }
}

fn main() {
  let engine = EngineBuilder::new()
    .title("Volumetric Lighting Demo")
    .resolution(1280, 720)
    .fps(60)
    .with_scene(VolumetricDemo::new())
    .build();

  if let Err(e) = engine.run() {
    eprintln!("Engine error: {}", e);
    std::process::exit(1);
  }
}
