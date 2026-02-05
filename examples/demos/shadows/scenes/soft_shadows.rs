//! Soft Shadow Scene
//!
//! Demonstrates soft shadows with configurable edge softness.
//! Soft shadows fade gradually at the edges instead of having hard edges.

use ryo2d::prelude::*;

/// Soft shadow demo scene state
pub struct SoftShadowsScene {
  // Screen dimensions
  screen_w: f32,
  screen_h: f32,

  // Player-controlled light
  light_entity: Option<Entity>,
  light_x: f32,
  light_y: f32,

  // Shadow softness (0.0 = hard, 1.0 = very soft)
  softness: f32,

  // Light radius
  light_radius: f32,

  // Static occluders
  static_occluders: Vec<Entity>,
}

impl SoftShadowsScene {
  pub fn new() -> Self {
    Self {
      screen_w: 1280.0,
      screen_h: 720.0,

      light_entity: None,
      light_x: 640.0,
      light_y: 360.0,

      softness: 0.5,
      light_radius: 400.0,

      static_occluders: Vec::new(),
    }
  }

  fn create_light(&self) -> Light2D {
    Light2D::point(Color::new(255, 240, 200, 255), self.light_radius)
      .with_intensity(1.3)
      .with_soft_shadows(self.softness)
  }

  fn update_light(&mut self, ctx: &mut SceneContext) {
    if let Some(entity) = self.light_entity {
      // Update light component
      if let Some(mut light) = ctx.world.get_mut::<Light2D>(entity) {
        *light = self.create_light();
      }

      // Update position
      if let Some(mut transform) = ctx.world.get_mut::<Transform>(entity) {
        transform.x = self.light_x;
        transform.y = self.light_y;
      }
    }
  }

  fn spawn_occluder(
    &mut self,
    ctx: &mut SceneContext,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
  ) -> Entity {
    // Create visual sprite
    let sprite = ctx.assets.create_sprite(
      Sprite::solid_rect(width as u32, height as u32, 60, 55, 70, 255),
      format!("soft_occluder_{}_{}", x as i32, y as i32),
    );

    // Spawn entity with both visual and occluder
    ctx.world.spawn((
      Transform::new(x, y).with_pivot_center(),
      Renderable::sprite(sprite).with_z_order(5),
      LightOccluder2D::rect(width, height),
    ))
  }
}

impl Scene for SoftShadowsScene {
  fn on_exit(&mut self, ctx: &mut SceneContext) {
    // Clean up all entities when leaving scene
    ctx.world.clear();
    self.static_occluders.clear();
    self.light_entity = None;
  }

  fn on_enter(&mut self, ctx: &mut SceneContext) {
    let (w, h) = ctx.screen_size();
    self.screen_w = w;
    self.screen_h = h;

    // Configure lighting
    if let Some(lighting) = ctx.lighting {
      lighting.set_ui_affected_by_lighting(false);
      // Dark ambient for dramatic shadows
      lighting.set_ambient(AmbientLight::new(Color::new(20, 18, 30, 255), 0.1));
    }

    // Create floor
    let floor = ctx
      .assets
      .create_sprite(Sprite::solid_rect(1280, 720, 35, 30, 40, 255), "soft_floor");
    ctx.world.spawn((
      Transform::new(640.0, 360.0).with_pivot_center(),
      Renderable::sprite(floor).with_z_order(0),
    ));

    // Create various occluders to show soft shadow edges
    // Central pillar
    let entity = self.spawn_occluder(ctx, 640.0, 360.0, 80.0, 80.0);
    self.static_occluders.push(entity);

    // Arranged pillars
    let pillar_positions = [
      (350.0, 280.0, 50.0, 50.0),
      (450.0, 450.0, 60.0, 40.0),
      (850.0, 320.0, 40.0, 60.0),
      (950.0, 480.0, 70.0, 70.0),
      (550.0, 200.0, 100.0, 30.0),
      (750.0, 550.0, 30.0, 100.0),
    ];

    for (x, y, w, h) in pillar_positions {
      let entity = self.spawn_occluder(ctx, x, y, w, h);
      self.static_occluders.push(entity);
    }

    // Create the player light
    self.light_entity = Some(ctx.world.spawn((
      Transform::new(self.light_x, self.light_y),
      self.create_light(),
    )));

    // Create a visual for the light source
    let light_visual = ctx.assets.create_sprite(
      Sprite::solid_circle(20, 255, 240, 200, 255),
      "soft_light_source",
    );
    ctx.world.spawn((
      Transform::new(self.light_x, self.light_y).with_pivot_center(),
      Renderable::sprite(light_visual).with_z_order(10),
    ));
  }

  fn update(&mut self, ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    // Scene switching
    if ctx.input.key_pressed(KeyCode::Digit1) {
      return SceneAction::Switch(Box::new(super::HardShadowsScene::new()));
    }
    if ctx.input.key_pressed(KeyCode::Digit2) {
      return SceneAction::Switch(Box::new(super::DropShadowsScene::new()));
    }
    if ctx.input.key_pressed(KeyCode::Digit3) {
      return SceneAction::Switch(Box::new(super::BlobShadowsScene::new()));
    }
    if ctx.input.key_pressed(KeyCode::Digit5) {
      return SceneAction::Switch(Box::new(super::ColoredShadowsScene::new()));
    }
    if ctx.input.key_pressed(KeyCode::Digit6) {
      return SceneAction::Switch(Box::new(super::ContactShadowsScene::new()));
    }
    if ctx.input.key_pressed(KeyCode::Digit7) {
      return SceneAction::Switch(Box::new(super::AmbientOcclusionScene::new()));
    }
    if ctx.input.key_pressed(KeyCode::Digit8) {
      return SceneAction::Switch(Box::new(super::RaymarchedShadowsScene::new()));
    }

    // Quit on Escape
    if ctx.input.key_pressed(KeyCode::Escape) {
      return SceneAction::Quit;
    }

    // Move light to mouse position
    let (mx, my) = ctx.input.mouse_position();
    self.light_x = mx;
    self.light_y = my;

    // Adjust softness with W/S keys
    if ctx.input.key_down(KeyCode::KeyW) {
      self.softness = (self.softness + 0.5 * (1.0 / 60.0)).min(1.0);
    }
    if ctx.input.key_down(KeyCode::KeyS) {
      self.softness = (self.softness - 0.5 * (1.0 / 60.0)).max(0.0);
    }

    // Adjust light radius with Q/E keys
    if ctx.input.key_down(KeyCode::KeyE) {
      self.light_radius = (self.light_radius + 100.0 * (1.0 / 60.0)).min(600.0);
    }
    if ctx.input.key_down(KeyCode::KeyQ) {
      self.light_radius = (self.light_radius - 100.0 * (1.0 / 60.0)).max(150.0);
    }

    // Toggle between presets with number keys (5-7)
    if ctx.input.key_pressed(KeyCode::Digit5) {
      self.softness = 0.0; // Hard shadows
    }
    if ctx.input.key_pressed(KeyCode::Digit6) {
      self.softness = 0.5; // Medium soft
    }
    if ctx.input.key_pressed(KeyCode::Digit7) {
      self.softness = 1.0; // Very soft
    }

    // Update the light
    self.update_light(ctx);

    SceneAction::Continue
  }

  fn render_ui(&mut self, ui: &mut UiContext) {
    let white = Color::WHITE;
    let gray = Color::new(180, 180, 180, 255);
    let highlight = Color::new(255, 255, 100, 255);
    let bg = Some(Color::new(0, 0, 0, 180));

    // Scene indicator
    ui.label(
      "[1] Hard [2] Drop [3] Blob [4] Soft [5] Color [6] Contact [7] AO [8] Ray",
      10.0,
      10.0,
      highlight,
      bg,
    );

    // Title
    ui.label("Soft Shadows", 10.0, 40.0, white, bg);

    // Softness level
    let softness_desc = match self.softness {
      s if s < 0.1 => "Hard",
      s if s < 0.4 => "Slight",
      s if s < 0.7 => "Medium",
      _ => "Very Soft",
    };
    ui.label(
      &format!("Softness: {:.2} ({}) (W/S)", self.softness, softness_desc),
      10.0,
      70.0,
      gray,
      bg,
    );

    // Light radius
    ui.label(
      &format!("Light Radius: {:.0} (Q/E)", self.light_radius),
      10.0,
      95.0,
      gray,
      bg,
    );

    // Presets
    ui.label(
      "Presets: [5] Hard  [6] Medium  [7] Very Soft",
      10.0,
      130.0,
      gray,
      bg,
    );

    // Controls
    ui.label("Controls:", 10.0, 165.0, white, bg);
    ui.label("  Mouse: Move light", 10.0, 185.0, gray, bg);
    ui.label("  W/S: Adjust softness", 10.0, 205.0, gray, bg);
    ui.label("  Q/E: Adjust radius", 10.0, 225.0, gray, bg);
    ui.label("  ESC: Quit", 10.0, 245.0, gray, bg);

    // Feature description
    ui.label(
      "Shadows with gradual edge fade",
      self.screen_w - 280.0,
      self.screen_h - 30.0,
      gray,
      bg,
    );
  }
}
