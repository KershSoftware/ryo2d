//! Ambient Occlusion page
//!
//! Demonstrates 2D screen-space ambient occlusion effect.
//! AO darkens corners and crevices based on edge detection.

use demo_kit::prelude::*;

/// Ambient occlusion page state
pub struct AmbientOcclusionPage {
  // AO settings
  ao_intensity: f32,
  ao_radius: f32,
  ao_bias: f32,
  ao_enabled: bool,

  // Sprites to demonstrate AO on
  sprites: Vec<Entity>,
}

impl AmbientOcclusionPage {
  pub fn new() -> Self {
    Self {
      ao_intensity: 0.5,
      ao_radius: 16.0,
      ao_bias: 0.05,
      ao_enabled: true,
      sprites: Vec::new(),
    }
  }

  fn spawn_box(
    ctx: &mut SceneContext,
    x: f32,
    y: f32,
    width: u32,
    height: u32,
    color: (u8, u8, u8),
    name: &str,
  ) -> Entity {
    let sprite = ctx.assets.create_sprite(
      Sprite::solid_rect(width, height, color.0, color.1, color.2, 255),
      name,
    );

    ctx
      .world
      .spawn((
        Transform::from_xy(x, y),
        Renderable::sprite(sprite)
          .with_anchor(Anchor::TopLeft)
          .with_z_order(5),
      ))
      .id()
  }
}

impl DemoPage for AmbientOcclusionPage {
  fn name(&self) -> &str {
    "Ambient Occlusion"
  }

  fn description(&self) -> &str {
    "Screen-space ambient occlusion: edge-based darkening of corners where surfaces meet."
  }

  fn settings(&mut self, s: &mut Settings) {
    s.toggle("AO", &mut self.ao_enabled);
    s.float("Intensity", &mut self.ao_intensity, 0.0..=1.0, 0.05);
    s.float("Radius", &mut self.ao_radius, 1.0..=64.0, 1.0);
    s.float("Bias", &mut self.ao_bias, 0.001..=0.5, 0.005);
  }

  fn enter(&mut self, ctx: &mut SceneContext) {
    // Bright ambient lighting for this demo
    ctx
      .lighting()
      .set_ambient(AmbientLight::new(Color::WHITE, 0.95));

    // Create background
    let bg = ctx.assets.create_sprite(
      Sprite::solid_rect(1280, 720, 180, 180, 190, 255), // Light gray
      "background",
    );
    ctx.world.spawn((
      Transform::from_xy(0.0, 0.0),
      Renderable::sprite(bg)
        .with_anchor(Anchor::TopLeft)
        .with_z_order(-1),
    ));

    // Create a variety of boxes arranged to show AO in corners
    // Large center structure
    self.sprites.push(Self::spawn_box(
      ctx,
      400.0,
      200.0,
      200,
      300,
      (100, 120, 140),
      "wall_left",
    ));
    self.sprites.push(Self::spawn_box(
      ctx,
      680.0,
      200.0,
      200,
      300,
      (100, 120, 140),
      "wall_right",
    ));
    self.sprites.push(Self::spawn_box(
      ctx,
      400.0,
      200.0,
      480,
      80,
      (120, 140, 160),
      "ceiling",
    ));
    self.sprites.push(Self::spawn_box(
      ctx,
      400.0,
      420.0,
      480,
      80,
      (80, 100, 120),
      "floor",
    ));

    // Inner box creating more corners
    self.sprites.push(Self::spawn_box(
      ctx,
      540.0,
      300.0,
      120,
      100,
      (140, 150, 170),
      "inner_box",
    ));

    // Side columns
    self.sprites.push(Self::spawn_box(
      ctx,
      100.0,
      150.0,
      60,
      400,
      (90, 110, 130),
      "column_left",
    ));
    self.sprites.push(Self::spawn_box(
      ctx,
      1120.0,
      150.0,
      60,
      400,
      (90, 110, 130),
      "column_right",
    ));

    // Small boxes scattered around
    let small_boxes = [
      (200.0, 350.0, 50, 50, (120, 80, 80)),
      (250.0, 380.0, 40, 40, (80, 120, 80)),
      (1000.0, 300.0, 60, 60, (80, 80, 120)),
      (950.0, 350.0, 45, 45, (100, 100, 80)),
    ];

    for (i, (x, y, w, h, color)) in small_boxes.iter().enumerate() {
      self.sprites.push(Self::spawn_box(
        ctx,
        *x,
        *y,
        *w,
        *h,
        *color,
        &format!("small_box_{}", i),
      ));
    }
  }

  fn update(&mut self, ctx: &mut SceneContext, _dt: f32) {
    if self.ao_enabled {
      ctx
        .effects()
        .set_ambient_occlusion_custom(self.ao_intensity, self.ao_radius, self.ao_bias);
    } else {
      ctx.effects().disable_ambient_occlusion();
    }
  }
}
