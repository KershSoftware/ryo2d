//! Ray-marched Shadows Scene
//!
//! Demonstrates per-pixel ray casting for accurate 2D shadows.
//! Ray-marching provides more precise shadows for complex occluder shapes.

use ryo2d::prelude::*;

/// Ray-marched shadow demo scene state
pub struct RaymarchedShadowsScene {
  // Screen dimensions
  screen_w: f32,
  screen_h: f32,

  // Player-controlled light
  light_entity: Option<Entity>,
  /// The marker drawn at the light
  light_visual: Option<Entity>,
  light_x: f32,
  light_y: f32,
  light_color: Color,

  // Ray-march settings
  raymarch_samples: u32,

  // Placed occluders (by player)
  placed_occluders: Vec<Entity>,

  // Static occluders
  static_occluders: Vec<Entity>,
}

impl RaymarchedShadowsScene {
  pub fn new() -> Self {
    Self {
      screen_w: 1280.0,
      screen_h: 720.0,

      light_entity: None,
      light_visual: None,
      light_x: 640.0,
      light_y: 360.0,
      light_color: Color::new(255, 220, 180, 255), // Warm light

      raymarch_samples: 32,

      placed_occluders: Vec::new(),
      static_occluders: Vec::new(),
    }
  }

  fn create_light(&self) -> Light2D {
    Light2D::point(self.light_color, 400.0)
      .with_intensity(1.3)
      .with_raymarched_shadows(self.raymarch_samples)
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

    // Keep the marker on the light
    if let Some(visual) = self.light_visual
      && let Some(mut transform) = ctx.world.get_mut::<Transform>(visual)
    {
      transform.x = self.light_x;
      transform.y = self.light_y;
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
      Sprite::solid_rect(width as u32, height as u32, 60, 70, 80, 255),
      format!("rm_occluder_{}_{}", x as i32, y as i32),
    );

    // Spawn entity with both visual and occluder
    ctx.world.spawn((
      Transform::new(x, y).with_pivot_center(),
      Renderable::sprite(sprite).with_z_order(5),
      LightOccluder2D::rect(width, height),
    ))
  }

  fn remove_nearest_occluder(&mut self, ctx: &mut SceneContext, x: f32, y: f32) {
    if self.placed_occluders.is_empty() {
      return;
    }

    // Find nearest placed occluder
    let mut nearest_idx = 0;
    let mut nearest_dist = f32::MAX;

    for (idx, &entity) in self.placed_occluders.iter().enumerate() {
      if let Some(transform) = ctx.world.get::<Transform>(entity) {
        let dx = transform.x - x;
        let dy = transform.y - y;
        let dist = dx * dx + dy * dy;
        if dist < nearest_dist {
          nearest_dist = dist;
          nearest_idx = idx;
        }
      }
    }

    // Remove if within reasonable distance (100 pixels)
    if nearest_dist < 100.0 * 100.0 {
      let entity = self.placed_occluders.remove(nearest_idx);
      let _ = ctx.world.despawn(entity);
    }
  }

  fn reset_placed_occluders(&mut self, ctx: &mut SceneContext) {
    for entity in self.placed_occluders.drain(..) {
      let _ = ctx.world.despawn(entity);
    }
  }
}

impl Scene for RaymarchedShadowsScene {
  fn on_exit(&mut self, ctx: &mut SceneContext) {
    ctx.world.clear();
    self.placed_occluders.clear();
    self.static_occluders.clear();
    self.light_entity = None;
    self.light_visual = None;
  }

  fn on_enter(&mut self, ctx: &mut SceneContext) {
    let (w, h) = ctx.screen_size();
    self.screen_w = w;
    self.screen_h = h;

    // Configure lighting - dark ambient for dramatic effect
    if let Some(lighting) = ctx.lighting {
      lighting.set_ui_affected_by_lighting(false);
      lighting.set_ambient(AmbientLight::new(Color::new(10, 12, 18, 255), 0.05));
    }

    // Create floor with subtle pattern
    let floor = ctx
      .assets
      .create_sprite(Sprite::solid_rect(1280, 720, 25, 28, 35, 255), "rm_floor");
    ctx.world.spawn((
      Transform::new(640.0, 360.0).with_pivot_center(),
      Renderable::sprite(floor).with_z_order(0),
    ));

    // Create static occluders - simple squares like placed ones
    let pillar_positions = [
      (300.0, 300.0),
      (500.0, 300.0),
      (700.0, 300.0),
      (400.0, 450.0),
      (600.0, 450.0),
    ];

    for (x, y) in pillar_positions {
      let entity = self.spawn_occluder(ctx, x, y, 50.0, 50.0);
      self.static_occluders.push(entity);
    }

    // Create the player light with ray-marched shadows
    self.light_entity = Some(ctx.world.spawn((
      Transform::new(self.light_x, self.light_y),
      self.create_light(),
    )));

    // Create a visual for the light source
    let light_visual = ctx.assets.create_sprite(
      Sprite::solid_circle(20, 255, 220, 150, 255),
      "rm_light_source",
    );
    self.light_visual = Some(ctx.world.spawn((
      Transform::new(self.light_x, self.light_y).with_pivot_center(),
      Renderable::sprite(light_visual).with_z_order(10),
    )));
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
    if ctx.input.key_pressed(KeyCode::Digit4) {
      return SceneAction::Switch(Box::new(super::SoftShadowsScene::new()));
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

    // Quit on Escape
    if ctx.input.key_pressed(KeyCode::Escape) {
      return SceneAction::Quit;
    }

    // Move light to mouse position
    let (mx, my) = ctx.input.mouse_position();
    self.light_x = mx;
    self.light_y = my;

    // Adjust ray-march samples
    if ctx.input.key_pressed(KeyCode::KeyW) {
      self.raymarch_samples = (self.raymarch_samples + 4).min(64);
    }
    if ctx.input.key_pressed(KeyCode::KeyS) {
      self.raymarch_samples = (self.raymarch_samples.saturating_sub(4)).max(4);
    }

    // Place occluder on left click
    if ctx.input.mouse_pressed(MouseButton::Left) {
      let entity = self.spawn_occluder(ctx, mx, my, 50.0, 50.0);
      self.placed_occluders.push(entity);
    }

    // Remove occluder on right click
    if ctx.input.mouse_pressed(MouseButton::Right) {
      self.remove_nearest_occluder(ctx, mx, my);
    }

    // Reset placed occluders
    if ctx.input.key_pressed(KeyCode::KeyR) {
      self.reset_placed_occluders(ctx);
    }

    // Update the light
    self.update_light(ctx);

    SceneAction::Continue
  }

  fn render_ui(&mut self, ui: &mut UiContext) -> SceneAction {
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
    ui.label("Ray-marched Shadows (Per-Pixel)", 10.0, 40.0, white, bg);

    // Ray-march info
    ui.label(
      &format!("Samples: {} (W/S)", self.raymarch_samples),
      10.0,
      70.0,
      gray,
      bg,
    );

    // Occluder count
    let placed = self.placed_occluders.len();
    let total = placed + self.static_occluders.len();
    ui.label(
      &format!("Occluders: {} placed / {} total", placed, total),
      10.0,
      100.0,
      gray,
      bg,
    );

    // Controls
    ui.label("Controls:", 10.0, 135.0, white, bg);
    ui.label("  Mouse: Move light", 10.0, 155.0, gray, bg);
    ui.label("  W/S: Adjust samples (4-64)", 10.0, 175.0, gray, bg);
    ui.label("  Left Click: Place box", 10.0, 195.0, gray, bg);
    ui.label("  Right Click: Remove box", 10.0, 215.0, gray, bg);
    ui.label("  R: Reset placed boxes", 10.0, 235.0, gray, bg);
    ui.label("  1-8: Switch scenes", 10.0, 255.0, gray, bg);
    ui.label("  ESC: Quit", 10.0, 275.0, gray, bg);

    // Description
    ui.label(
      "Per-pixel ray casting produces accurate shadows",
      10.0,
      310.0,
      gray,
      bg,
    );
    ui.label(
      "for complex occluder shapes. Higher samples = smoother.",
      10.0,
      330.0,
      gray,
      bg,
    );

    // API example
    ui.label(
      "Light2D::point(c, r).with_raymarched_shadows(32)",
      self.screen_w - 400.0,
      self.screen_h - 30.0,
      gray,
      bg,
    );
    SceneAction::Continue
  }
}
