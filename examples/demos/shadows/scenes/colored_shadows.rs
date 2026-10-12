//! Colored Shadows page
//!
//! Demonstrates colored shadows - shadows that have a color tint rather than
//! being pure black. Useful for stained glass effects, atmospheric lighting,
//! and artistic stylization.

use demo_kit::prelude::*;

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
  const ALL: [ShadowColorPreset; 7] = [
    Self::Black,
    Self::Red,
    Self::Blue,
    Self::Purple,
    Self::Warm,
    Self::Cool,
    Self::Custom,
  ];

  const NAMES: [&'static str; 7] = [
    "Black",
    "Red",
    "Blue",
    "Purple",
    "Warm (Sunset)",
    "Cool (Moonlight)",
    "Custom",
  ];

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
}

/// Colored shadow page state
pub struct ColoredShadowsPage {
  // Light entity
  light_entity: Option<Entity>,
  /// The marker drawn at the light
  light_visual: Option<Entity>,
  light_x: f32,
  light_y: f32,

  // Shadow color settings (preset indexes ShadowColorPreset::ALL)
  preset: usize,
  custom_rgb: [i32; 3],

  // Occluders
  static_occluders: Vec<Entity>,
}

impl ColoredShadowsPage {
  pub fn new() -> Self {
    Self {
      light_entity: None,
      light_visual: None,
      light_x: 640.0,
      light_y: 360.0,

      preset: 0,
      custom_rgb: [100, 100, 100],

      static_occluders: Vec::new(),
    }
  }

  fn current_preset(&self) -> ShadowColorPreset {
    ShadowColorPreset::ALL[self.preset.min(ShadowColorPreset::ALL.len() - 1)]
  }

  fn custom_color(&self) -> Color {
    let [r, g, b] = self.custom_rgb.map(|c| c.clamp(0, 255) as u8);
    Color::new(r, g, b, 255)
  }

  fn create_light(&self) -> Light2D {
    let shadow_color = self.current_preset().color(self.custom_color());
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
    color: (u8, u8, u8),
  ) -> Entity {
    let sprite = ctx.assets.create_sprite(
      Sprite::solid_rect(width as u32, height as u32, color.0, color.1, color.2, 255),
      format!("colored_occluder_{}_{}", x as i32, y as i32),
    );

    ctx
      .world
      .spawn((
        Transform::from_xy(x, y),
        Renderable::sprite(sprite).with_z_order(5),
        LightOccluder2D::rect(width, height),
      ))
      .id()
  }
}

impl DemoPage for ColoredShadowsPage {
  fn name(&self) -> &str {
    "Colored"
  }

  fn description(&self) -> &str {
    "Tinted shadows for stained glass, atmosphere and stylization."
  }

  fn settings(&mut self, s: &mut Settings) {
    s.choice("Shadow colour", &mut self.preset, &ShadowColorPreset::NAMES);
    if self.current_preset() == ShadowColorPreset::Custom {
      s.int("R", &mut self.custom_rgb[0], 0..=255);
      s.int("G", &mut self.custom_rgb[1], 0..=255);
      s.int("B", &mut self.custom_rgb[2], 0..=255);
    }
  }

  fn controls(&self) -> Vec<Control> {
    vec![Control::mouse("Move the light")]
  }

  fn enter(&mut self, ctx: &mut SceneContext) {
    // Configure lighting
    ctx
      .lighting()
      .set_ambient(AmbientLight::new(Color::new(20, 20, 30, 255), 0.12));

    // Create floor
    let floor = ctx.assets.create_sprite(
      Sprite::solid_rect(1280, 720, 50, 45, 55, 255),
      "colored_floor",
    );
    ctx.world.spawn((
      Transform::from_xy(640.0, 360.0),
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
    self.light_entity = Some(
      ctx
        .world
        .spawn((
          Transform::from_xy(self.light_x, self.light_y),
          self.create_light(),
        ))
        .id(),
    );

    // Light source visual
    let light_visual = ctx.assets.create_sprite(
      Sprite::solid_circle(18, 255, 255, 255, 255),
      "colored_light_source",
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
    // Move light to mouse
    let (mx, my) = ctx.input.mouse_position();
    self.light_x = mx;
    self.light_y = my;

    self.update_light(ctx);
  }
}
