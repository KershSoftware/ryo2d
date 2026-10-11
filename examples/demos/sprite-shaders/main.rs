//! Sprite Shader Effects Demo
//!
//! Per-sprite shader effects: ten built-in effects plus a custom WGSL shader
//! (`assets/shaders/dissolve.wgsl`) on the middle sprite.

use demo_kit::prelude::*;

/// All available sprite effects for demonstration
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DemoEffect {
  Outline,
  Glow,
  Flash,
  ColorShift,
  Silhouette,
  Pixelate,
  Wave,
  Dissolve,
  Invert,
  Grayscale,
}

impl DemoEffect {
  const ALL: [DemoEffect; 10] = [
    DemoEffect::Outline,
    DemoEffect::Glow,
    DemoEffect::Flash,
    DemoEffect::ColorShift,
    DemoEffect::Silhouette,
    DemoEffect::Pixelate,
    DemoEffect::Wave,
    DemoEffect::Dissolve,
    DemoEffect::Invert,
    DemoEffect::Grayscale,
  ];

  fn name(&self) -> &'static str {
    match self {
      Self::Outline => "Outline",
      Self::Glow => "Glow",
      Self::Flash => "Flash",
      Self::ColorShift => "Color Shift",
      Self::Silhouette => "Silhouette",
      Self::Pixelate => "Pixelate",
      Self::Wave => "Wave",
      Self::Dissolve => "Dissolve",
      Self::Invert => "Invert",
      Self::Grayscale => "Grayscale",
    }
  }

  fn description(&self) -> &'static str {
    match self {
      Self::Outline => "Draw a coloured border around the sprite.",
      Self::Glow => "Add a soft halo around the sprite.",
      Self::Flash => "Blend with a solid colour (damage flash).",
      Self::ColorShift => "Shift colours toward a target colour.",
      Self::Silhouette => "Replace every colour (shadow mode).",
      Self::Pixelate => "Group pixels into blocks.",
      Self::Wave => "Wavy distortion over time.",
      Self::Dissolve => "Noise-based pixel dissolve.",
      Self::Invert => "Invert the RGB colours.",
      Self::Grayscale => "Convert to grayscale.",
    }
  }

  fn convenience_method(&self) -> &'static str {
    match self {
      Self::Outline => "SpriteEffect::white_outline(2)",
      Self::Glow => "SpriteEffect::glow(Color::CYAN, 8.0)",
      Self::Flash => "SpriteEffect::damage_flash(0.7)",
      Self::ColorShift => "SpriteEffect::color_shift(Color::GREEN, 0.5)",
      Self::Silhouette => "SpriteEffect::shadow()",
      Self::Pixelate => "SpriteEffect::pixelate(4)",
      Self::Wave => "SpriteEffect::wave(3.0, 2.0, 1.5)",
      Self::Dissolve => "SpriteEffect::dissolve_with_edge(0.5, Color::ORANGE)",
      Self::Invert => "SpriteEffect::invert()",
      Self::Grayscale => "SpriteEffect::full_grayscale()",
    }
  }
}

const COLOR_NAMES: [&str; 6] = ["White", "Red", "Cyan", "Yellow", "Magenta", "Green"];

fn palette(index: usize) -> Color {
  [Color::WHITE, Color::RED, Color::CYAN, Color::YELLOW, Color::MAGENTA, Color::GREEN]
    .get(index)
    .copied()
    .unwrap_or(Color::WHITE)
}

/// Left edges of the plain, custom-shader and effect sprites (96 px wide),
/// left of the settings panel
const SPRITE_X: [f32; 3] = [80.0, 230.0, 380.0];

struct EffectsPage {
  effect: usize,
  color: usize,
  outline_thickness: i32,
  glow_radius: f32,
  glow_intensity: f32,
  flash_intensity: f32,
  color_shift_amount: f32,
  silhouette_alpha: f32,
  pixelate_size: i32,
  wave_amplitude: f32,
  wave_speed: f32,
  dissolve_progress: f32,
  grayscale_intensity: f32,
  time: f32,
  flash_timer: f32,
  dissolve_animating: bool,
  normal_sprite: Option<Entity>,
  effect_sprite: Option<Entity>,
  // Middle sprite driven by the custom WGSL dissolve shader (None if it failed to load)
  custom_sprite: Option<Entity>,
}

impl Default for EffectsPage {
  fn default() -> Self {
    Self {
      effect: 0,
      color: 0,
      outline_thickness: 2,
      glow_radius: 8.0,
      glow_intensity: 0.8,
      flash_intensity: 0.7,
      color_shift_amount: 0.5,
      silhouette_alpha: 0.5,
      pixelate_size: 4,
      wave_amplitude: 3.0,
      wave_speed: 2.0,
      dissolve_progress: 0.3,
      grayscale_intensity: 1.0,
      time: 0.0,
      flash_timer: 0.0,
      dissolve_animating: false,
      normal_sprite: None,
      effect_sprite: None,
      custom_sprite: None,
    }
  }
}

impl EffectsPage {
  fn current(&self) -> DemoEffect {
    DemoEffect::ALL[self.effect.min(DemoEffect::ALL.len() - 1)]
  }

  fn create_sprite_effect(&self) -> SpriteEffect {
    let color = palette(self.color);
    match self.current() {
      DemoEffect::Outline => SpriteEffect::outline(color, self.outline_thickness.max(1) as u32),
      DemoEffect::Glow => {
        SpriteEffect::glow_with_intensity(color, self.glow_radius, self.glow_intensity)
      }
      DemoEffect::Flash => SpriteEffect::flash(color, self.flash_intensity),
      DemoEffect::ColorShift => SpriteEffect::color_shift(color, self.color_shift_amount),
      DemoEffect::Silhouette => {
        let mut c = color;
        c.a = (self.silhouette_alpha * 255.0).round() as u8;
        SpriteEffect::silhouette(c)
      }
      DemoEffect::Pixelate => SpriteEffect::pixelate(self.pixelate_size.max(1) as u32),
      DemoEffect::Wave => SpriteEffect::wave(self.wave_amplitude, 2.0, self.wave_speed),
      DemoEffect::Dissolve => {
        SpriteEffect::dissolve_with_edge(self.dissolve_progress, Color::new(255, 165, 0, 255))
      }
      DemoEffect::Invert => SpriteEffect::invert(),
      DemoEffect::Grayscale => SpriteEffect::grayscale(self.grayscale_intensity),
    }
  }

  fn update_effect_sprite(&self, world: &mut World) {
    if let Some(entity) = self.effect_sprite
      && let Some(mut renderable) = world.get_mut::<Renderable>(entity)
    {
      renderable.shader = Some(SpriteShader::BuiltIn(self.create_sprite_effect()));
    }
  }

  fn trigger_action(&mut self) {
    match self.current() {
      DemoEffect::Flash => {
        self.flash_timer = 0.5;
        self.flash_intensity = 1.0;
      }
      DemoEffect::Dissolve => {
        self.dissolve_animating = !self.dissolve_animating;
      }
      _ => {}
    }
  }
}

impl DemoPage for EffectsPage {
  fn name(&self) -> &str {
    "Effects"
  }

  fn description(&self) -> &str {
    "Plain sprite (left), custom WGSL dissolve shader (middle), the chosen built-in effect (right)."
  }

  fn settings(&mut self, s: &mut Settings) {
    let names: Vec<&str> = DemoEffect::ALL.iter().map(|e| e.name()).collect();
    s.choice("Effect", &mut self.effect, &names);
    match self.current() {
      DemoEffect::Outline => {
        s.int("Thickness", &mut self.outline_thickness, 1..=8);
        s.choice("Color", &mut self.color, &COLOR_NAMES);
      }
      DemoEffect::Glow => {
        s.float("Radius", &mut self.glow_radius, 1.0..=32.0, 1.0);
        s.choice("Color", &mut self.color, &COLOR_NAMES);
      }
      DemoEffect::Flash => {
        s.float("Intensity", &mut self.flash_intensity, 0.0..=1.0, 0.1);
        s.choice("Color", &mut self.color, &COLOR_NAMES);
      }
      DemoEffect::ColorShift => {
        s.float("Amount", &mut self.color_shift_amount, 0.0..=1.0, 0.1);
        s.choice("Color", &mut self.color, &COLOR_NAMES);
      }
      DemoEffect::Silhouette => {
        s.float("Alpha", &mut self.silhouette_alpha, 0.0..=1.0, 0.1);
        s.choice("Color", &mut self.color, &COLOR_NAMES);
      }
      DemoEffect::Pixelate => s.int("Block size", &mut self.pixelate_size, 2..=16),
      DemoEffect::Wave => {
        s.float("Amplitude", &mut self.wave_amplitude, 0.5..=10.0, 0.5);
        s.float("Speed", &mut self.wave_speed, 0.0..=5.0, 0.5);
      }
      DemoEffect::Dissolve => s.float("Progress", &mut self.dissolve_progress, 0.0..=1.0, 0.1),
      DemoEffect::Invert => {}
      DemoEffect::Grayscale => s.float("Intensity", &mut self.grayscale_intensity, 0.0..=1.0, 0.1),
    }
  }

  fn controls(&self) -> Vec<Control> {
    match self.current() {
      DemoEffect::Flash => vec![Control::key(KeyCode::Space, "Flash")],
      DemoEffect::Dissolve => vec![Control::key(KeyCode::Space, "Animate the dissolve")],
      _ => Vec::new(),
    }
  }

  fn enter(&mut self, ctx: &mut SceneContext) {
    let (_, height) = ctx.screen_size();
    let cy = height / 2.0;

    // Create a colorful test sprite
    let sprite = create_test_sprite();
    let handle = ctx.assets.create_sprite(sprite.clone(), "test_sprite");
    let handle2 = ctx.assets.create_sprite(sprite, "test_sprite_2");

    // Left: normal sprite (no effect)
    let mut normal_transform = Transform::new(SPRITE_X[0], cy);
    normal_transform.scale_x = 3.0;
    normal_transform.scale_y = 3.0;
    self.normal_sprite = Some(ctx.world.spawn((
      normal_transform,
      Renderable::sprite(handle.clone()).with_z_order(0),
    )));

    // Middle: sprite with a custom WGSL shader loaded from disk. A missing or
    // unreadable file is reported and the sprite is simply left out.
    match ctx.assets.load_shader("assets/shaders/dissolve.wgsl") {
      Ok(shader) => {
        let handle3 = ctx.assets.create_sprite(create_test_sprite(), "test_sprite_3");
        let mut custom_transform = Transform::new(SPRITE_X[1], cy);
        custom_transform.scale_x = 3.0;
        custom_transform.scale_y = 3.0;
        self.custom_sprite = Some(
          ctx.world.spawn((
            custom_transform,
            Renderable::sprite(handle3)
              .with_z_order(0)
              .with_shader(
                SpriteShader::custom(shader)
                  .with_param("amount", 0.0)
                  .with_param("edge", Color::rgb(255, 120, 0)),
              ),
          )),
        );
      }
      Err(e) => eprintln!("Could not load dissolve.wgsl: {e}"),
    }

    // Right: sprite with effect
    let mut effect_transform = Transform::new(SPRITE_X[2], cy);
    effect_transform.scale_x = 3.0;
    effect_transform.scale_y = 3.0;
    self.effect_sprite = Some(
      ctx.world.spawn((
        effect_transform,
        Renderable::sprite(handle2)
          .with_z_order(0)
          .with_shader(SpriteShader::BuiltIn(self.create_sprite_effect())),
      )),
    );
  }

  fn update(&mut self, ctx: &mut SceneContext, dt: f32) {
    self.time += dt;

    // Handle flash timer
    if self.flash_timer > 0.0 {
      self.flash_timer -= dt;
      self.flash_intensity = self.flash_timer.max(0.0) * 2.0;
    }

    // Handle dissolve animation
    if self.dissolve_animating {
      self.dissolve_progress += dt * 0.5;
      if self.dissolve_progress >= 1.0 {
        self.dissolve_progress = 0.0;
      }
    }

    // Animate the custom shader's `amount` back and forth (0 -> 1 -> 0)
    if let Some(entity) = self.custom_sprite
      && let Some(mut renderable) = ctx.world.get_mut::<Renderable>(entity)
      && let Some(params) = renderable.shader.as_mut().and_then(|s| s.params_mut())
    {
      let amount = 0.5 - 0.5 * (self.time * 0.8).cos();
      params.set("amount", amount);
    }

    // Settings may change any frame, so rebuild the effect every update
    self.update_effect_sprite(ctx.world);

    // Update wave time (animated effect); must follow the rebuild above
    if self.current() == DemoEffect::Wave
      && let Some(entity) = self.effect_sprite
      && let Some(mut renderable) = ctx.world.get_mut::<Renderable>(entity)
      && let Some(SpriteShader::BuiltIn(SpriteEffect::Wave {
        ref mut time,
        speed,
        ..
      })) = renderable.shader
    {
      // Apply speed multiplier to time
      *time = self.time * speed;
    }

    if ctx.input.key_pressed(KeyCode::Space) {
      self.trigger_action();
    }
  }

  fn render_ui(&mut self, ui: &mut UiContext) {
    let area = content_area(ui);
    let effect = self.current();

    ui.draw_text(effect.description(), area.x, area.y, Color::WHITE);
    ui.draw_text(
      &format!("Code: {}", effect.convenience_method()),
      area.x,
      area.y + 20.0,
      Color::WHITE,
    );

    // Labels, centred under the three 96 px sprites
    let label = |x: f32, y: f32| Rect {
      x: x - 50.0,
      y,
      width: 196.0,
      height: 16.0,
    };
    ui.draw_text_centered("Normal", label(SPRITE_X[0], 432.0), Color::WHITE);
    if self.custom_sprite.is_some() {
      ui.draw_text_centered("Custom WGSL", label(SPRITE_X[1], 432.0), Color::WHITE);
      ui.draw_text_centered("(dissolve.wgsl)", label(SPRITE_X[1], 448.0), Color::WHITE);
    }
    ui.draw_text_centered("With Effect", label(SPRITE_X[2], 432.0), Color::WHITE);
  }
}

/// Create a colorful test sprite with recognizable patterns
fn create_test_sprite() -> Sprite {
  let size = 32;
  let mut pixels = vec![0u8; size * size * 4];

  for y in 0..size {
    for x in 0..size {
      let idx = (y * size + x) * 4;

      // Create a circular pattern with radial gradient
      let cx = size as f32 / 2.0;
      let cy = size as f32 / 2.0;
      let dx = x as f32 - cx;
      let dy = y as f32 - cy;
      let dist = (dx * dx + dy * dy).sqrt();
      let max_dist = cx;

      if dist < max_dist - 2.0 {
        // Inside circle
        let t = dist / max_dist;

        // Colorful gradient based on angle and distance
        let angle = dy.atan2(dx) + std::f32::consts::PI;
        let hue = angle / (2.0 * std::f32::consts::PI);

        let (r, g, b) = hsv_to_rgb(hue, 0.8, 1.0 - t * 0.5);

        pixels[idx] = r;
        pixels[idx + 1] = g;
        pixels[idx + 2] = b;
        pixels[idx + 3] = 255;
      } else if dist < max_dist {
        // Border
        pixels[idx] = 40;
        pixels[idx + 1] = 40;
        pixels[idx + 2] = 60;
        pixels[idx + 3] = 255;
      } else {
        // Outside - transparent
        pixels[idx] = 0;
        pixels[idx + 1] = 0;
        pixels[idx + 2] = 0;
        pixels[idx + 3] = 0;
      }
    }
  }

  Sprite::from_rgba(pixels, size as u32, size as u32)
}

/// Convert HSV to RGB (for colorful sprite generation)
fn hsv_to_rgb(h: f32, s: f32, v: f32) -> (u8, u8, u8) {
  let c = v * s;
  let x = c * (1.0 - ((h * 6.0) % 2.0 - 1.0).abs());
  let m = v - c;

  let (r, g, b) = match (h * 6.0) as u32 {
    0 => (c, x, 0.0),
    1 => (x, c, 0.0),
    2 => (0.0, c, x),
    3 => (0.0, x, c),
    4 => (x, 0.0, c),
    _ => (c, 0.0, x),
  };

  (
    ((r + m) * 255.0) as u8,
    ((g + m) * 255.0) as u8,
    ((b + m) * 255.0) as u8,
  )
}

fn main() {
  Demo::new("Sprite Shaders")
    .resolution(800, 600)
    .page(EffectsPage::default)
    .run();
}
