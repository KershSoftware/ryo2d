//! Hard Shadow Scene
//!
//! Demonstrates 2D hard shadows from light occluders using shadow volumes.

use ryo2d::prelude::*;

/// Hard shadow demo scene state
pub struct HardShadowsScene {
  // Screen dimensions
  screen_w: f32,
  screen_h: f32,

  // Player-controlled light
  light_entity: Option<Entity>,
  /// The marker drawn at the light
  light_visual: Option<Entity>,
  light_x: f32,
  light_y: f32,
  is_spotlight: bool,
  spotlight_direction: f32,
  spotlight_angle: f32,
  light_color: Color,

  // Placed occluders (by player)
  placed_occluders: Vec<Entity>,

  // Static occluders (walls, pillars)
  static_occluders: Vec<Entity>,
}

impl HardShadowsScene {
  pub fn new() -> Self {
    Self {
      screen_w: 1280.0,
      screen_h: 720.0,

      light_entity: None,
      light_visual: None,
      light_x: 640.0,
      light_y: 360.0,
      is_spotlight: false,
      spotlight_direction: 270.0, // pointing up
      spotlight_angle: 45.0,
      light_color: Color::WHITE,

      placed_occluders: Vec::new(),
      static_occluders: Vec::new(),
    }
  }

  fn create_light(&self) -> Light2D {
    if self.is_spotlight {
      Light2D::spot(
        self.light_color,
        400.0,
        self.spotlight_angle,
        self.spotlight_direction,
      )
      .with_intensity(1.5)
      .with_shadows()
    } else {
      Light2D::point(self.light_color, 350.0)
        .with_intensity(1.2)
        .with_shadows()
    }
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
      Sprite::solid_rect(width as u32, height as u32, 80, 80, 90, 255),
      format!("occluder_{}_{}", x as i32, y as i32),
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

impl Scene for HardShadowsScene {
  fn on_exit(&mut self, ctx: &mut SceneContext) {
    // Clean up all entities when leaving scene
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

    // Configure lighting
    if let Some(lighting) = ctx.lighting {
      lighting.set_ui_affected_by_lighting(false);
      // Dark ambient for dramatic shadows
      lighting.set_ambient(AmbientLight::new(Color::new(15, 15, 25, 255), 0.05));
    }

    // Create floor
    let floor = ctx
      .assets
      .create_sprite(Sprite::solid_rect(1280, 720, 40, 40, 45, 255), "floor");
    ctx.world.spawn((
      Transform::new(640.0, 360.0).with_pivot_center(),
      Renderable::sprite(floor).with_z_order(0),
    ));

    // Create static occluders (walls around the edge)
    // Left wall
    let wall_entity = self.spawn_occluder(ctx, 20.0, 360.0, 40.0, 720.0);
    self.static_occluders.push(wall_entity);

    // Right wall
    let wall_entity = self.spawn_occluder(ctx, 1260.0, 360.0, 40.0, 720.0);
    self.static_occluders.push(wall_entity);

    // Top wall
    let wall_entity = self.spawn_occluder(ctx, 640.0, 20.0, 1280.0, 40.0);
    self.static_occluders.push(wall_entity);

    // Bottom wall
    let wall_entity = self.spawn_occluder(ctx, 640.0, 700.0, 1280.0, 40.0);
    self.static_occluders.push(wall_entity);

    // Create some pillars
    let pillar_positions = [
      (300.0, 250.0),
      (500.0, 450.0),
      (800.0, 300.0),
      (1000.0, 500.0),
      (640.0, 360.0),
    ];

    for (x, y) in pillar_positions {
      let entity = self.spawn_occluder(ctx, x, y, 60.0, 60.0);
      self.static_occluders.push(entity);
    }

    // Create the player light
    self.light_entity = Some(ctx.world.spawn((
      Transform::new(self.light_x, self.light_y),
      self.create_light(),
    )));

    // Create a visual for the light source
    let light_visual = ctx
      .assets
      .create_sprite(Sprite::solid_circle(16, 255, 255, 200, 255), "light_source");
    self.light_visual = Some(ctx.world.spawn((
      Transform::new(self.light_x, self.light_y).with_pivot_center(),
      Renderable::sprite(light_visual).with_z_order(10),
    )));
  }

  fn update(&mut self, ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    // Scene switching
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

    // Toggle spotlight mode
    if ctx.input.key_pressed(KeyCode::Space) {
      self.is_spotlight = !self.is_spotlight;
    }

    // Spotlight controls
    if self.is_spotlight {
      let dt = 1.0 / 60.0; // Approximate dt for smooth controls
      if ctx.input.key_down(KeyCode::KeyQ) {
        self.spotlight_direction = (self.spotlight_direction - 90.0 * dt) % 360.0;
        if self.spotlight_direction < 0.0 {
          self.spotlight_direction += 360.0;
        }
      }
      if ctx.input.key_down(KeyCode::KeyE) {
        self.spotlight_direction = (self.spotlight_direction + 90.0 * dt) % 360.0;
      }
      if ctx.input.key_down(KeyCode::KeyW) {
        self.spotlight_angle = (self.spotlight_angle + 30.0 * dt).min(120.0);
      }
      if ctx.input.key_down(KeyCode::KeyS) {
        self.spotlight_angle = (self.spotlight_angle - 30.0 * dt).max(15.0);
      }
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
    ui.label("Hard Shadows (Shadow Volumes)", 10.0, 40.0, white, bg);

    // Light mode
    let mode = if self.is_spotlight {
      "Spotlight"
    } else {
      "Point Light"
    };
    ui.label(
      &format!("Light Mode: {} (Space)", mode),
      10.0,
      70.0,
      gray,
      bg,
    );

    // Spotlight info
    if self.is_spotlight {
      ui.label(
        &format!("Direction: {:.0} deg (Q/E)", self.spotlight_direction),
        10.0,
        95.0,
        gray,
        bg,
      );
      ui.label(
        &format!("Cone Angle: {:.0} deg (W/S)", self.spotlight_angle),
        10.0,
        120.0,
        gray,
        bg,
      );
    }

    // Occluder count
    let placed = self.placed_occluders.len();
    let total = placed + self.static_occluders.len();
    ui.label(
      &format!("Occluders: {} placed / {} total", placed, total),
      10.0,
      150.0,
      gray,
      bg,
    );

    // Controls
    ui.label("Controls:", 10.0, 185.0, white, bg);
    ui.label("  Mouse: Move light", 10.0, 205.0, gray, bg);
    ui.label("  Left Click: Place box", 10.0, 225.0, gray, bg);
    ui.label("  Right Click: Remove box", 10.0, 245.0, gray, bg);
    ui.label("  R: Reset placed boxes", 10.0, 265.0, gray, bg);
    ui.label("  ESC: Quit", 10.0, 285.0, gray, bg);

    // Feature description
    ui.label(
      "Real-time shadows from LightOccluder2D",
      self.screen_w - 320.0,
      self.screen_h - 30.0,
      gray,
      bg,
    );
    SceneAction::Continue
  }
}
