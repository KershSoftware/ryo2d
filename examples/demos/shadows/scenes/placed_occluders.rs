//! Placed occluders page
//!
//! Place and remove occluders around a light that casts soft shadow volumes.

use demo_kit::prelude::*;

/// Placed occluders page state
pub struct PlacedOccludersPage {
  // Player-controlled light
  light_entity: Option<Entity>,
  /// The marker drawn at the light
  light_visual: Option<Entity>,
  light_x: f32,
  light_y: f32,
  /// The mouse when the page opened: the light waits among the occluders
  /// until the mouse first moves away from it
  mouse_at_open: Option<(f32, f32)>,
  light_color: Color,

  // Shadow softness: how much light the shadows let through (0 = hard)
  softness: f32,

  // Placed occluders (by player)
  placed_occluders: Vec<Entity>,

  // Static occluders
  static_occluders: Vec<Entity>,
}

impl PlacedOccludersPage {
  pub fn new() -> Self {
    Self {
      light_entity: None,
      light_visual: None,
      light_x: 500.0,
      light_y: 380.0,
      mouse_at_open: None,
      // Warm light: (255, 220, 180) read as linear, as when the page was tuned
      light_color: Color::new(255, 239, 219, 255),

      softness: 0.2,

      placed_occluders: Vec::new(),
      static_occluders: Vec::new(),
    }
  }

  fn create_light(&self) -> Light2D {
    Light2D::point(self.light_color, 400.0)
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
      format!("po_occluder_{}_{}", x as i32, y as i32),
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

impl DemoPage for PlacedOccludersPage {
  fn name(&self) -> &str {
    "Placed occluders"
  }

  fn description(&self) -> &str {
    "Place boxes around a light that casts soft shadow volumes."
  }

  fn settings(&mut self, s: &mut Settings) {
    s.float("Softness", &mut self.softness, 0.0..=1.0, 0.05);
  }

  fn controls(&self) -> Vec<Control> {
    vec![Control::mouse(
      "Move the light; left click places a box, right click removes one",
    )]
  }

  fn enter(&mut self, ctx: &mut SceneContext) {
    self.mouse_at_open = Some(ctx.input.mouse_position());

    // Configure lighting - dark ambient for dramatic effect: (10, 12, 18) read
    // as linear, as when the page was tuned
    ctx
      .lighting()
      .set_ambient(AmbientLight::new(Color::new(56, 61, 75, 255), 0.05));

    // Create floor with subtle pattern
    let floor = ctx
      .assets
      .create_sprite(Sprite::solid_rect(1280, 720, 25, 28, 35, 255), "po_floor");
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

    // Create the player light with soft shadow volumes
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
      "po_light_source",
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
    // The light waits among the occluders until the mouse first moves
    let (mx, my) = ctx.input.mouse_position();
    if self.mouse_at_open != Some((mx, my)) {
      self.mouse_at_open = None;
      self.light_x = mx;
      self.light_y = my;
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
