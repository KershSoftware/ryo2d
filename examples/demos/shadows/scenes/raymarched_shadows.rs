//! Ray-marched Shadows page
//!
//! Demonstrates per-pixel ray casting for accurate 2D shadows.
//! Ray-marching provides more precise shadows for complex occluder shapes.

use demo_kit::prelude::*;

/// Ray-marched shadow page state
pub struct RaymarchedShadowsPage {
  // Player-controlled light
  light_entity: Option<Entity>,
  /// The marker drawn at the light
  light_visual: Option<Entity>,
  light_x: f32,
  light_y: f32,
  light_color: Color,

  // Ray-march settings
  raymarch_samples: i32,

  // Placed occluders (by player)
  placed_occluders: Vec<Entity>,

  // Static occluders
  static_occluders: Vec<Entity>,
}

impl RaymarchedShadowsPage {
  pub fn new() -> Self {
    Self {
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
      .with_raymarched_shadows(self.raymarch_samples.clamp(4, 64) as u32)
  }

  fn update_light(&mut self, ctx: &mut SceneContext) {
    if let Some(entity) = self.light_entity {
      // Update light component
      if let Some(mut light) = ctx.world.get_mut::<Light2D>(entity) {
        *light = self.create_light();
      }

      // Update position
      if let Some(mut transform) = ctx.world.get_mut::<Transform>(entity) {
        transform.translation = Vec2::new(self.light_x, self.light_y);
      }
    }

    // Keep the marker on the light
    if let Some(visual) = self.light_visual
      && let Some(mut transform) = ctx.world.get_mut::<Transform>(visual)
    {
      transform.translation = Vec2::new(self.light_x, self.light_y);
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
    ctx
      .world
      .spawn((
        Transform::from_xy(x, y),
        Renderable::sprite(sprite).with_z_order(5),
        LightOccluder2D::rect(width, height),
      ))
      .id()
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
        let dx = transform.translation.x - x;
        let dy = transform.translation.y - y;
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
      ctx.world.despawn(entity);
    }
  }
}

impl DemoPage for RaymarchedShadowsPage {
  fn name(&self) -> &str {
    "Ray-marched"
  }

  fn description(&self) -> &str {
    "Per-pixel ray casting for accurate shadows; more samples are smoother."
  }

  fn settings(&mut self, s: &mut Settings) {
    s.int("Samples", &mut self.raymarch_samples, 4..=64);
  }

  fn controls(&self) -> Vec<Control> {
    vec![Control::mouse(
      "Move the light; left click places a box, right click removes one",
    )]
  }

  fn enter(&mut self, ctx: &mut SceneContext) {
    // Configure lighting - dark ambient for dramatic effect
    ctx
      .lighting()
      .set_ambient(AmbientLight::new(Color::new(10, 12, 18, 255), 0.05));

    // Create floor with subtle pattern
    let floor = ctx
      .assets
      .create_sprite(Sprite::solid_rect(1280, 720, 25, 28, 35, 255), "rm_floor");
    ctx.world.spawn((
      Transform::from_xy(640.0, 360.0),
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
    self.light_entity = Some(
      ctx
        .world
        .spawn((
          Transform::from_xy(self.light_x, self.light_y),
          self.create_light(),
        ))
        .id(),
    );

    // Create a visual for the light source
    let light_visual = ctx.assets.create_sprite(
      Sprite::solid_circle(20, 255, 220, 150, 255),
      "rm_light_source",
    );
    self.light_visual = Some(
      ctx
        .world
        .spawn((
          Transform::from_xy(self.light_x, self.light_y),
          Renderable::sprite(light_visual).with_z_order(10),
        ))
        .id(),
    );
  }

  fn update(&mut self, ctx: &mut SceneContext, _dt: f32) {
    // Move light to mouse position
    let (mx, my) = ctx.input.mouse_position();
    self.light_x = mx;
    self.light_y = my;

    // Place occluder on left click
    if ctx.input.mouse_pressed(MouseButton::Left) {
      let entity = self.spawn_occluder(ctx, mx, my, 50.0, 50.0);
      self.placed_occluders.push(entity);
    }

    // Remove occluder on right click
    if ctx.input.mouse_pressed(MouseButton::Right) {
      self.remove_nearest_occluder(ctx, mx, my);
    }

    // Update the light
    self.update_light(ctx);
  }

  fn ui(&mut self, ui: &mut UiContext, _ctx: &mut SceneContext) {
    let gray = Color::new(180, 180, 180, 255);
    let bg = Some(Color::new(0, 0, 0, 180));
    let area = content_area(ui);

    let placed = self.placed_occluders.len();
    let total = placed + self.static_occluders.len();
    ui.label(
      &format!("Occluders: {} placed / {} total", placed, total),
      area.x + 10.0,
      area.y + 10.0,
      gray,
      bg,
    );
  }
}
