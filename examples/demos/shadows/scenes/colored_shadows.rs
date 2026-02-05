//! Colored Shadow Scene
//!
//! Demonstrates colored shadows - shadows that have a color tint rather than
//! being pure black. Useful for stained glass effects, atmospheric lighting,
//! and artistic stylization.

use ryo2d::prelude::*;

/// Preset shadow colors
#[derive(Clone, Copy, PartialEq)]
enum ShadowColorPreset {
  Black,
  Red,
  Blue,
  Purple,
  Warm,
  Cool,
  Custom,
}

impl ShadowColorPreset {
  fn name(&self) -> &'static str {
    match self {
      Self::Black => "Black (Default)",
      Self::Red => "Red",
      Self::Blue => "Blue",
      Self::Purple => "Purple",
      Self::Warm => "Warm (Sunset)",
      Self::Cool => "Cool (Moonlight)",
      Self::Custom => "Custom",
    }
  }

  fn color(&self, custom: Color) -> Color {
    match self {
      Self::Black => Color::BLACK,
      Self::Red => Color::new(200, 50, 50, 255),
      Self::Blue => Color::new(50, 80, 200, 255),
      Self::Purple => Color::new(150, 50, 180, 255),
      Self::Warm => Color::new(255, 150, 80, 255),
      Self::Cool => Color::new(80, 150, 255, 255),
      Self::Custom => custom,
    }
  }

  fn next(&self) -> Self {
    match self {
      Self::Black => Self::Red,
      Self::Red => Self::Blue,
      Self::Blue => Self::Purple,
      Self::Purple => Self::Warm,
      Self::Warm => Self::Cool,
      Self::Cool => Self::Custom,
      Self::Custom => Self::Black,
    }
  }
}

/// Colored shadow demo scene
pub struct ColoredShadowsScene {
  screen_w: f32,
  screen_h: f32,

  // Light entity
  light_entity: Option<Entity>,
  light_x: f32,
  light_y: f32,

  // Shadow color settings
  current_preset: ShadowColorPreset,
  custom_color: Color,

  // Occluders
  static_occluders: Vec<Entity>,
}

impl ColoredShadowsScene {
  pub fn new() -> Self {
    Self {
      screen_w: 1280.0,
      screen_h: 720.0,

      light_entity: None,
      light_x: 640.0,
      light_y: 360.0,

      current_preset: ShadowColorPreset::Black,
      custom_color: Color::new(100, 100, 100, 255),

      static_occluders: Vec::new(),
    }
  }

  fn create_light(&self) -> Light2D {
    let shadow_color = self.current_preset.color(self.custom_color);
    Light2D::point(Color::WHITE, 400.0)
      .with_intensity(1.4)
      .with_shadows()
      .with_shadow_color(shadow_color)
  }

  fn update_light(&mut self, ctx: &mut SceneContext) {
    if let Some(entity) = self.light_entity {
      if let Some(mut light) = ctx.world.get_mut::<Light2D>(entity) {
        *light = self.create_light();
      }

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
    color: (u8, u8, u8),
  ) -> Entity {
    let sprite = ctx.assets.create_sprite(
      Sprite::solid_rect(width as u32, height as u32, color.0, color.1, color.2, 255),
      format!("colored_occluder_{}_{}", x as i32, y as i32),
    );

    ctx.world.spawn((
      Transform::new(x, y).with_pivot_center(),
      Renderable::sprite(sprite).with_z_order(5),
      LightOccluder2D::rect(width, height),
    ))
  }
}

impl Scene for ColoredShadowsScene {
  fn on_exit(&mut self, ctx: &mut SceneContext) {
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
      lighting.set_ambient(AmbientLight::new(Color::new(20, 20, 30, 255), 0.12));
    }

    // Create floor
    let floor = ctx
      .assets
      .create_sprite(Sprite::solid_rect(1280, 720, 50, 45, 55, 255), "colored_floor");
    ctx.world.spawn((
      Transform::new(640.0, 360.0).with_pivot_center(),
      Renderable::sprite(floor).with_z_order(0),
    ));

    // Create colorful occluders (like stained glass panels)
    let occluders = [
      (300.0, 300.0, 70.0, 70.0, (180, 60, 60)),   // Red
      (500.0, 250.0, 50.0, 80.0, (60, 180, 60)),   // Green
      (700.0, 350.0, 80.0, 50.0, (60, 60, 180)),   // Blue
      (450.0, 450.0, 60.0, 60.0, (180, 180, 60)),  // Yellow
      (850.0, 280.0, 55.0, 55.0, (180, 60, 180)),  // Magenta
      (640.0, 180.0, 100.0, 40.0, (60, 180, 180)), // Cyan
      (900.0, 500.0, 65.0, 65.0, (200, 150, 100)), // Tan
    ];

    for (x, y, w, h, color) in occluders {
      let entity = self.spawn_occluder(ctx, x, y, w, h, color);
      self.static_occluders.push(entity);
    }

    // Create the light
    self.light_entity = Some(ctx.world.spawn((
      Transform::new(self.light_x, self.light_y),
      self.create_light(),
    )));

    // Light source visual
    let light_visual = ctx
      .assets
      .create_sprite(Sprite::solid_circle(18, 255, 255, 255, 255), "colored_light_source");
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
    if ctx.input.key_pressed(KeyCode::Digit4) {
      return SceneAction::Switch(Box::new(super::SoftShadowsScene::new()));
    }
    if ctx.input.key_pressed(KeyCode::Digit6) {
      return SceneAction::Switch(Box::new(super::ContactShadowsScene::new()));
    }

    if ctx.input.key_pressed(KeyCode::Escape) {
      return SceneAction::Quit;
    }

    // Move light to mouse
    let (mx, my) = ctx.input.mouse_position();
    self.light_x = mx;
    self.light_y = my;

    // Cycle presets with Tab
    if ctx.input.key_pressed(KeyCode::Tab) {
      self.current_preset = self.current_preset.next();
    }

    // Custom color adjustments (when in custom mode)
    if self.current_preset == ShadowColorPreset::Custom {
      let speed = 150.0 * (1.0 / 60.0);

      // R channel (Q/A)
      if ctx.input.key_down(KeyCode::KeyQ) {
        self.custom_color.r = self.custom_color.r.saturating_add(speed as u8);
      }
      if ctx.input.key_down(KeyCode::KeyA) {
        self.custom_color.r = self.custom_color.r.saturating_sub(speed as u8);
      }

      // G channel (W/S)
      if ctx.input.key_down(KeyCode::KeyW) {
        self.custom_color.g = self.custom_color.g.saturating_add(speed as u8);
      }
      if ctx.input.key_down(KeyCode::KeyS) {
        self.custom_color.g = self.custom_color.g.saturating_sub(speed as u8);
      }

      // B channel (E/D)
      if ctx.input.key_down(KeyCode::KeyE) {
        self.custom_color.b = self.custom_color.b.saturating_add(speed as u8);
      }
      if ctx.input.key_down(KeyCode::KeyD) {
        self.custom_color.b = self.custom_color.b.saturating_sub(speed as u8);
      }
    }

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
      "[1] Hard [2] Drop [3] Blob [4] Soft [5] Color [6] Contact",
      10.0,
      10.0,
      highlight,
      bg,
    );

    // Title
    ui.label("Colored Shadows", 10.0, 40.0, white, bg);

    // Current preset
    let shadow_color = self.current_preset.color(self.custom_color);
    ui.label(
      &format!("Shadow Color: {} (Tab)", self.current_preset.name()),
      10.0,
      70.0,
      highlight,
      bg,
    );

    // Show RGB values
    ui.label(
      &format!("RGB: ({}, {}, {})", shadow_color.r, shadow_color.g, shadow_color.b),
      10.0,
      95.0,
      gray,
      bg,
    );

    // Custom controls
    if self.current_preset == ShadowColorPreset::Custom {
      ui.label("Custom Color Controls:", 10.0, 130.0, white, bg);
      ui.label("  Q/A: Red +/-", 10.0, 150.0, Color::new(255, 150, 150, 255), bg);
      ui.label("  W/S: Green +/-", 10.0, 170.0, Color::new(150, 255, 150, 255), bg);
      ui.label("  E/D: Blue +/-", 10.0, 190.0, Color::new(150, 150, 255, 255), bg);
    }

    // Controls
    let ctrl_y = if self.current_preset == ShadowColorPreset::Custom { 225.0 } else { 130.0 };
    ui.label("Controls:", 10.0, ctrl_y, white, bg);
    ui.label("  Mouse: Move light", 10.0, ctrl_y + 20.0, gray, bg);
    ui.label("  Tab: Cycle shadow colors", 10.0, ctrl_y + 40.0, gray, bg);
    ui.label("  1-5: Switch shadow scenes", 10.0, ctrl_y + 60.0, gray, bg);
    ui.label("  ESC: Quit", 10.0, ctrl_y + 80.0, gray, bg);

    // Feature description
    ui.label(
      "Tinted shadows for atmosphere",
      self.screen_w - 260.0,
      self.screen_h - 30.0,
      gray,
      bg,
    );
  }
}
