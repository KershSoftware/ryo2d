//! Sprite Shader Effects Demo
//!
//! Demonstrates all per-sprite shader effects available in Ryo2D:
//!
//! - Outline: Draw colored borders around sprites
//! - Glow: Add bloom/halo effects
//! - Flash: Damage/power-up flashes
//! - ColorShift: Status effect tints
//! - Silhouette: Shadow/death effects
//! - Pixelate: Retro/transition effects
//! - Wave: Water/heat shimmer
//! - Dissolve: Death/teleport animations
//! - Invert: Color inversion
//! - Grayscale: Desaturation
//!
//! Controls:
//! - Left/Right: Navigate between effects
//! - Up/Down: Adjust effect parameters
//! - Space: Trigger animated effects
//! - R: Reset parameters
//! - Escape: Quit

use ryo2d::prelude::*;

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
      Self::Outline => {
        "Draw a colored border around the sprite.\nUp/Down: Thickness (1-8)  C: Cycle color"
      }
      Self::Glow => "Add a soft halo around the sprite.\nUp/Down: Radius (1-32)  C: Cycle color",
      Self::Flash => "Blend with a solid color (damage flash).\nUp/Down: Intensity  Space: Trigger",
      Self::ColorShift => "Shift colors toward a target.\nUp/Down: Amount",
      Self::Silhouette => "Replace all colors (shadow mode).\nUp/Down: Alpha",
      Self::Pixelate => "Group pixels into blocks.\nUp/Down: Block size (2-16)",
      Self::Wave => "Wavy distortion effect.\nUp/Down: Amplitude (0.5-10)  C: Change speed",
      Self::Dissolve => "Noise-based pixel dissolve.\nUp/Down: Progress (0-1)  Space: Animate",
      Self::Invert => "Invert all RGB colors.\n(No parameters)",
      Self::Grayscale => "Convert to grayscale.\nUp/Down: Intensity",
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

struct SpriteShaderDemo {
  // Current effect being demonstrated
  current_index: usize,

  // Demo sprites - one normal, one with effect
  normal_sprite: Option<Entity>,
  effect_sprite: Option<Entity>,

  // Effect parameters (adjustable via keyboard)
  outline_thickness: u32,
  glow_radius: f32,
  glow_intensity: f32,
  flash_intensity: f32,
  color_shift_amount: f32,
  silhouette_alpha: u8,
  pixelate_size: u32,
  wave_amplitude: f32,
  wave_speed: f32,
  dissolve_progress: f32,
  grayscale_intensity: f32,

  // Animation state
  time: f32,
  flash_timer: f32,
  dissolve_animating: bool,

  // Input debouncing - track cooldown for navigation
  nav_cooldown: f32,

  // Colors for cycling
  effect_colors: Vec<Color>,
  current_color_index: usize,
}

impl SpriteShaderDemo {
  fn new() -> Self {
    Self {
      current_index: 0,
      normal_sprite: None,
      effect_sprite: None,

      // Default parameters
      outline_thickness: 2,
      glow_radius: 8.0,
      glow_intensity: 0.8,
      flash_intensity: 0.7,
      color_shift_amount: 0.5,
      silhouette_alpha: 128,
      pixelate_size: 4,
      wave_amplitude: 3.0,
      wave_speed: 2.0,
      dissolve_progress: 0.3,
      grayscale_intensity: 1.0,

      time: 0.0,
      flash_timer: 0.0,
      dissolve_animating: false,

      nav_cooldown: 0.0,

      effect_colors: vec![
        Color::WHITE,
        Color::RED,
        Color::CYAN,
        Color::YELLOW,
        Color::MAGENTA,
        Color::GREEN,
      ],
      current_color_index: 0,
    }
  }

  fn current_effect(&self) -> DemoEffect {
    DemoEffect::ALL[self.current_index]
  }

  fn current_color(&self) -> Color {
    self.effect_colors[self.current_color_index]
  }

  fn create_sprite_effect(&self) -> SpriteEffect {
    let color = self.current_color();
    match self.current_effect() {
      DemoEffect::Outline => SpriteEffect::outline(color, self.outline_thickness),
      DemoEffect::Glow => {
        SpriteEffect::glow_with_intensity(color, self.glow_radius, self.glow_intensity)
      }
      DemoEffect::Flash => SpriteEffect::flash(color, self.flash_intensity),
      DemoEffect::ColorShift => SpriteEffect::color_shift(color, self.color_shift_amount),
      DemoEffect::Silhouette => {
        let mut c = color;
        c.a = self.silhouette_alpha;
        SpriteEffect::silhouette(c)
      }
      DemoEffect::Pixelate => SpriteEffect::pixelate(self.pixelate_size),
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
      && let Some(mut renderable) = world.get_component_mut::<Renderable>(entity) {
        renderable.shader = Some(SpriteShader::BuiltIn(self.create_sprite_effect()));
      }
  }

  fn adjust_param(&mut self, delta: f32) {
    match self.current_effect() {
      DemoEffect::Outline => {
        self.outline_thickness = (self.outline_thickness as i32 + delta as i32).clamp(1, 8) as u32;
      }
      DemoEffect::Glow => {
        self.glow_radius = (self.glow_radius + delta * 1.0).clamp(1.0, 32.0);
      }
      DemoEffect::Flash => {
        self.flash_intensity = (self.flash_intensity + delta * 0.1).clamp(0.0, 1.0);
      }
      DemoEffect::ColorShift => {
        self.color_shift_amount = (self.color_shift_amount + delta * 0.1).clamp(0.0, 1.0);
      }
      DemoEffect::Silhouette => {
        self.silhouette_alpha =
          (self.silhouette_alpha as i32 + (delta * 25.0) as i32).clamp(0, 255) as u8;
      }
      DemoEffect::Pixelate => {
        self.pixelate_size = (self.pixelate_size as i32 + delta as i32).clamp(2, 16) as u32;
      }
      DemoEffect::Wave => {
        self.wave_amplitude = (self.wave_amplitude + delta * 0.5).clamp(0.5, 10.0);
      }
      DemoEffect::Dissolve => {
        self.dissolve_progress = (self.dissolve_progress + delta * 0.1).clamp(0.0, 1.0);
      }
      DemoEffect::Invert => {
        // No parameters
      }
      DemoEffect::Grayscale => {
        self.grayscale_intensity = (self.grayscale_intensity + delta * 0.1).clamp(0.0, 1.0);
      }
    }
  }

  fn adjust_secondary_param(&mut self, delta: f32) {
    match self.current_effect() {
      DemoEffect::Wave => {
        // For Wave, C changes speed (wrap around at max)
        self.wave_speed += delta * 0.5;
        if self.wave_speed > 5.0 {
          self.wave_speed = 0.0;
        } else if self.wave_speed < 0.0 {
          self.wave_speed = 5.0;
        }
      }
      _ => {
        // For all other effects (including Glow), C cycles colors
        if delta > 0.0 {
          self.current_color_index = (self.current_color_index + 1) % self.effect_colors.len();
        } else if self.current_color_index > 0 {
          self.current_color_index -= 1;
        } else {
          self.current_color_index = self.effect_colors.len() - 1;
        }
      }
    }
  }

  fn trigger_action(&mut self) {
    match self.current_effect() {
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

  fn reset_params(&mut self) {
    self.outline_thickness = 2;
    self.glow_radius = 8.0;
    self.glow_intensity = 0.8;
    self.flash_intensity = 0.7;
    self.color_shift_amount = 0.5;
    self.silhouette_alpha = 128;
    self.pixelate_size = 4;
    self.wave_amplitude = 3.0;
    self.wave_speed = 2.0;
    self.dissolve_progress = 0.3;
    self.grayscale_intensity = 1.0;
    self.current_color_index = 0;
    self.dissolve_animating = false;
  }
}

impl Game for SpriteShaderDemo {
  fn init(&mut self, ctx: &mut GameContext) {
    // Create a colorful test sprite
    let sprite = create_test_sprite();
    let handle = ctx.assets.create_sprite(sprite.clone(), "test_sprite");
    let handle2 = ctx.assets.create_sprite(sprite, "test_sprite_2");

    // Left side: normal sprite (no effect)
    let mut normal_transform = Transform::new(200.0, 300.0);
    normal_transform.scale_x = 3.0;
    normal_transform.scale_y = 3.0;
    self.normal_sprite = Some(ctx.world.spawn((
      normal_transform,
      Renderable::sprite(handle.clone()).with_z_order(0),
    )));

    // Right side: sprite with effect
    let mut effect_transform = Transform::new(550.0, 300.0);
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

  fn update(&mut self, ctx: &mut GameContext, dt: f32) {
    self.time += dt;

    // Update navigation cooldown
    if self.nav_cooldown > 0.0 {
      self.nav_cooldown -= dt;
    }

    // Handle flash timer
    if self.flash_timer > 0.0 {
      self.flash_timer -= dt;
      self.flash_intensity = self.flash_timer.max(0.0) * 2.0;
      self.update_effect_sprite(ctx.world);
    }

    // Handle dissolve animation
    if self.dissolve_animating {
      self.dissolve_progress += dt * 0.5;
      if self.dissolve_progress >= 1.0 {
        self.dissolve_progress = 0.0;
      }
      self.update_effect_sprite(ctx.world);
    }

    // Update wave time (animated effect)
    if self.current_effect() == DemoEffect::Wave
      && let Some(entity) = self.effect_sprite
        && let Some(mut renderable) = ctx.world.get_component_mut::<Renderable>(entity)
          && let Some(SpriteShader::BuiltIn(SpriteEffect::Wave {
            ref mut time,
            speed,
            ..
          })) = renderable.shader
          {
            // Apply speed multiplier to time
            *time = self.time * speed;
          }

    // Navigate effects (with cooldown to prevent rapid jumps)
    if self.nav_cooldown <= 0.0 {
      if ctx.input.key_pressed(KeyCode::ArrowRight) {
        self.current_index = (self.current_index + 1) % DemoEffect::ALL.len();
        self.update_effect_sprite(ctx.world);
        self.nav_cooldown = 0.15; // 150ms cooldown
      } else if ctx.input.key_pressed(KeyCode::ArrowLeft) {
        if self.current_index > 0 {
          self.current_index -= 1;
        } else {
          self.current_index = DemoEffect::ALL.len() - 1;
        }
        self.update_effect_sprite(ctx.world);
        self.nav_cooldown = 0.15; // 150ms cooldown
      }
    }

    // Adjust parameters
    if ctx.input.key_pressed(KeyCode::ArrowUp) {
      self.adjust_param(1.0);
      self.update_effect_sprite(ctx.world);
    }
    if ctx.input.key_pressed(KeyCode::ArrowDown) {
      self.adjust_param(-1.0);
      self.update_effect_sprite(ctx.world);
    }

    // Secondary parameter / color cycling
    if ctx.input.key_pressed(KeyCode::KeyC) {
      self.adjust_secondary_param(1.0);
      self.update_effect_sprite(ctx.world);
    }

    // Trigger action
    if ctx.input.key_pressed(KeyCode::Space) {
      self.trigger_action();
      self.update_effect_sprite(ctx.world);
    }

    // Reset
    if ctx.input.key_pressed(KeyCode::KeyR) {
      self.reset_params();
      self.update_effect_sprite(ctx.world);
    }

    // Quit
    if ctx.input.key_pressed(KeyCode::Escape) {
      std::process::exit(0);
    }
  }

  fn render_ui(&mut self, ui: &mut UiContext) {
    let effect = self.current_effect();

    // Title
    ui.draw_text("Per-Sprite Shader Effects Demo", 10.0, 10.0, Color::WHITE);
    ui.draw_text(&"-".repeat(40), 10.0, 25.0, Color::WHITE);

    // Current effect info
    ui.draw_text(
      &format!(
        "Effect {}/{}: {}",
        self.current_index + 1,
        DemoEffect::ALL.len(),
        effect.name()
      ),
      10.0,
      45.0,
      Color::WHITE,
    );

    // Description
    for (i, line) in effect.description().lines().enumerate() {
      ui.draw_text(line, 10.0, 65.0 + i as f32 * 15.0, Color::WHITE);
    }

    // Convenience method
    ui.draw_text("Code:", 10.0, 110.0, Color::WHITE);
    ui.draw_text(effect.convenience_method(), 20.0, 125.0, Color::WHITE);

    // Current parameters
    ui.draw_text("Current values:", 10.0, 155.0, Color::WHITE);
    let param_text = match effect {
      DemoEffect::Outline => format!("thickness: {}", self.outline_thickness),
      DemoEffect::Glow => {
        format!(
          "radius: {:.1}, intensity: {:.2}",
          self.glow_radius, self.glow_intensity
        )
      }
      DemoEffect::Flash => format!("intensity: {:.2}", self.flash_intensity),
      DemoEffect::ColorShift => format!("amount: {:.2}", self.color_shift_amount),
      DemoEffect::Silhouette => format!("alpha: {}", self.silhouette_alpha),
      DemoEffect::Pixelate => format!("block_size: {}", self.pixelate_size),
      DemoEffect::Wave => format!(
        "amplitude: {:.1}  (C for speed: {:.1})",
        self.wave_amplitude, self.wave_speed
      ),
      DemoEffect::Dissolve => format!("progress: {:.2}", self.dissolve_progress),
      DemoEffect::Invert => "none".to_string(),
      DemoEffect::Grayscale => format!("intensity: {:.2}", self.grayscale_intensity),
    };
    ui.draw_text(&param_text, 20.0, 170.0, Color::WHITE);

    // Color (for effects that use it)
    let uses_color = !matches!(
      effect,
      DemoEffect::Pixelate
        | DemoEffect::Invert
        | DemoEffect::Grayscale
        | DemoEffect::Dissolve
        | DemoEffect::Wave
    );
    if uses_color {
      ui.draw_text(
        &format!("color: {:?} (C to cycle)", self.current_color()),
        20.0,
        185.0,
        Color::WHITE,
      );
    }

    // Labels
    ui.draw_text("Normal", 170.0, 440.0, Color::WHITE);
    ui.draw_text("With Effect", 500.0, 440.0, Color::WHITE);

    // Controls reminder
    ui.draw_text(
      "[Left/Right]: Change effect  [Up/Down]: Adjust param  C: Cycle color  Space: Trigger  R: Reset",
      10.0,
      570.0,
      Color::WHITE,
    );
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
  EngineBuilder::new()
    .title("Sprite Shaders Demo")
    .with_game(SpriteShaderDemo::new())
    .build()
    .run()
    .expect("Failed to run demo");
}
