//! Ambient Occlusion Scene
//!
//! Demonstrates 2D screen-space ambient occlusion effect.
//! AO darkens corners and crevices based on edge detection.

use ryo2d::prelude::*;

/// Ambient occlusion demo scene
pub struct AmbientOcclusionScene {
  screen_w: f32,
  screen_h: f32,

  // AO settings
  ao_intensity: f32,
  ao_radius: f32,
  ao_bias: f32,
  ao_enabled: bool,

  // Sprites to demonstrate AO on
  sprites: Vec<Entity>,
}

impl AmbientOcclusionScene {
  pub fn new() -> Self {
    Self {
      screen_w: 1280.0,
      screen_h: 720.0,
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

    ctx.world.spawn((
      Transform::new(x, y),
      Renderable::sprite(sprite).with_z_order(5),
    ))
  }
}

impl Scene for AmbientOcclusionScene {
  fn on_exit(&mut self, ctx: &mut SceneContext) {
    ctx.world.clear();
    self.sprites.clear();
    ctx.effects.disable_ambient_occlusion();
  }

  fn on_enter(&mut self, ctx: &mut SceneContext) {
    let (w, h) = ctx.screen_size();
    self.screen_w = w;
    self.screen_h = h;

    // Bright ambient lighting for this demo
    if let Some(lighting) = ctx.lighting {
      lighting.set_ui_affected_by_lighting(false);
      lighting.set_ambient(AmbientLight::new(Color::WHITE, 0.95));
    }

    // Create background
    let bg = ctx.assets.create_sprite(
      Sprite::solid_rect(1280, 720, 180, 180, 190, 255), // Light gray
      "background",
    );
    ctx.world.spawn((
      Transform::new(0.0, 0.0),
      Renderable::sprite(bg).with_z_order(-1),
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

    // Apply initial AO settings
    if self.ao_enabled {
      ctx
        .effects
        .set_ambient_occlusion_custom(self.ao_intensity, self.ao_radius, self.ao_bias);
    }
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

    // Quit on Escape
    if ctx.input.key_pressed(KeyCode::Escape) {
      return SceneAction::Quit;
    }

    // Toggle AO
    if ctx.input.key_pressed(KeyCode::KeyA) {
      self.ao_enabled = !self.ao_enabled;
      if self.ao_enabled {
        ctx
          .effects
          .set_ambient_occlusion_custom(self.ao_intensity, self.ao_radius, self.ao_bias);
      } else {
        ctx.effects.disable_ambient_occlusion();
      }
    }

    // Adjust intensity
    if ctx.input.key_down(KeyCode::KeyI) {
      self.ao_intensity = (self.ao_intensity + 0.01).min(1.0);
      if self.ao_enabled {
        ctx
          .effects
          .set_ambient_occlusion_custom(self.ao_intensity, self.ao_radius, self.ao_bias);
      }
    }
    if ctx.input.key_down(KeyCode::KeyK) {
      self.ao_intensity = (self.ao_intensity - 0.01).max(0.0);
      if self.ao_enabled {
        ctx
          .effects
          .set_ambient_occlusion_custom(self.ao_intensity, self.ao_radius, self.ao_bias);
      }
    }

    // Adjust radius
    if ctx.input.key_down(KeyCode::KeyR) {
      self.ao_radius = (self.ao_radius + 0.5).min(64.0);
      if self.ao_enabled {
        ctx
          .effects
          .set_ambient_occlusion_custom(self.ao_intensity, self.ao_radius, self.ao_bias);
      }
    }
    if ctx.input.key_down(KeyCode::KeyF) {
      self.ao_radius = (self.ao_radius - 0.5).max(1.0);
      if self.ao_enabled {
        ctx
          .effects
          .set_ambient_occlusion_custom(self.ao_intensity, self.ao_radius, self.ao_bias);
      }
    }

    // Adjust bias
    if ctx.input.key_down(KeyCode::KeyB) {
      self.ao_bias = (self.ao_bias + 0.005).min(0.5);
      if self.ao_enabled {
        ctx
          .effects
          .set_ambient_occlusion_custom(self.ao_intensity, self.ao_radius, self.ao_bias);
      }
    }
    if ctx.input.key_down(KeyCode::KeyV) {
      self.ao_bias = (self.ao_bias - 0.005).max(0.001);
      if self.ao_enabled {
        ctx
          .effects
          .set_ambient_occlusion_custom(self.ao_intensity, self.ao_radius, self.ao_bias);
      }
    }

    SceneAction::Continue
  }

  fn render_ui(&mut self, ui: &mut UiContext) {
    let white = Color::WHITE;
    let gray = Color::new(180, 180, 180, 255);
    let highlight = Color::new(255, 255, 100, 255);
    let bg = Some(Color::new(0, 0, 0, 180));

    // Scene indicator
    ui.label(
      "[1] Hard [2] Drop [3] Blob [4] Soft [5] Color [6] Contact [7] AO",
      10.0,
      10.0,
      highlight,
      bg,
    );

    // Title
    ui.label("Ambient Occlusion (Post-Process)", 10.0, 40.0, white, bg);

    // AO status
    let ao_status = if self.ao_enabled { "ON" } else { "OFF" };
    ui.label(
      &format!("AO: {} (A)", ao_status),
      10.0,
      70.0,
      if self.ao_enabled { highlight } else { gray },
      bg,
    );

    // Parameters
    ui.label(
      &format!("Intensity: {:.2} (I/K)", self.ao_intensity),
      10.0,
      100.0,
      gray,
      bg,
    );
    ui.label(
      &format!("Radius: {:.1}px (R/F)", self.ao_radius),
      10.0,
      120.0,
      gray,
      bg,
    );
    ui.label(
      &format!("Bias: {:.3} (B/V)", self.ao_bias),
      10.0,
      140.0,
      gray,
      bg,
    );

    // Controls
    ui.label("Controls:", 10.0, 175.0, white, bg);
    ui.label("  A: Toggle ambient occlusion", 10.0, 195.0, gray, bg);
    ui.label("  I/K: Increase/decrease intensity", 10.0, 215.0, gray, bg);
    ui.label("  R/F: Increase/decrease radius", 10.0, 235.0, gray, bg);
    ui.label("  B/V: Increase/decrease bias", 10.0, 255.0, gray, bg);
    ui.label("  1-7: Switch shadow scenes", 10.0, 275.0, gray, bg);
    ui.label("  ESC: Quit", 10.0, 295.0, gray, bg);

    // Description
    ui.label(
      "Edge-based AO darkens corners where surfaces meet.",
      10.0,
      330.0,
      gray,
      bg,
    );

    // API example
    ui.label(
      "ctx.effects.set_ambient_occlusion(intensity, radius)",
      self.screen_w - 420.0,
      self.screen_h - 30.0,
      gray,
      bg,
    );
  }
}
