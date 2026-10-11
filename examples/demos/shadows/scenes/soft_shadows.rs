//! Soft Shadows page
//!
//! Demonstrates soft shadows with configurable edge softness.
//! Soft shadows fade gradually at the edges instead of having hard edges.

use demo_kit::prelude::*;

/// Soft shadow page state
pub struct SoftShadowsPage {
  // Player-controlled light
  light_entity: Option<Entity>,
  /// The marker drawn at the light
  light_visual: Option<Entity>,
  light_x: f32,
  light_y: f32,

  // Shadow softness (0.0 = hard, 1.0 = very soft)
  softness: f32,

  // Light radius
  light_radius: f32,

  // Static occluders
  static_occluders: Vec<Entity>,
}

impl SoftShadowsPage {
  pub fn new() -> Self {
    Self {
      light_entity: None,
      light_visual: None,
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

impl DemoPage for SoftShadowsPage {
  fn name(&self) -> &str {
    "Soft"
  }

  fn description(&self) -> &str {
    "Shadows with a gradual edge fade instead of hard edges."
  }

  fn settings(&mut self, s: &mut Settings) {
    s.float("Softness", &mut self.softness, 0.0..=1.0, 0.05);
    s.float("Light radius", &mut self.light_radius, 150.0..=600.0, 10.0);
  }

  fn controls(&self) -> Vec<Control> {
    vec![Control::mouse("Move the light")]
  }

  fn enter(&mut self, ctx: &mut SceneContext) {
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
    self.light_visual = Some(ctx.world.spawn((
      Transform::new(self.light_x, self.light_y).with_pivot_center(),
      Renderable::sprite(light_visual).with_z_order(10),
    )));
  }

  fn update(&mut self, ctx: &mut SceneContext, _dt: f32) {
    // Move light to mouse position
    let (mx, my) = ctx.input.mouse_position();
    self.light_x = mx;
    self.light_y = my;

    // Update the light
    self.update_light(ctx);
  }
}
