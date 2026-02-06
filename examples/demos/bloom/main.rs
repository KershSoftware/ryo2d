use ryo2d::prelude::*;

/// Preset bloom configurations to showcase different looks.
#[derive(Debug, Clone, Copy, PartialEq)]
enum BloomPreset {
  Off,
  Subtle,
  Dreamy,
  Neon,
  Explosive,
  Custom,
}

impl BloomPreset {
  fn name(&self) -> &'static str {
    match self {
      Self::Off => "Off",
      Self::Subtle => "Subtle Glow",
      Self::Dreamy => "Dreamy / Soft Focus",
      Self::Neon => "Neon Glow",
      Self::Explosive => "Explosive Bloom",
      Self::Custom => "Custom (Tweak Live)",
    }
  }

  fn description(&self) -> &'static str {
    match self {
      Self::Off => "No bloom. Base scene for comparison.",
      Self::Subtle => "Low threshold, low intensity. Gentle highlight glow.",
      Self::Dreamy => "Mid threshold, high radius. Soft ethereal look.",
      Self::Neon => "Low threshold, high intensity. Punchy neon vibe.",
      Self::Explosive => "Very low threshold, max intensity & radius.",
      Self::Custom => "Up/Down: intensity  Left/Right: radius\nW/S: threshold  Q/E: blur passes",
    }
  }

  fn params(&self) -> (f32, f32, f32, u32) {
    // (threshold, intensity, radius, blur_passes)
    match self {
      Self::Off => (0.7, 0.0, 5.0, 2),
      Self::Subtle => (0.8, 0.6, 3.0, 1),
      Self::Dreamy => (0.5, 1.2, 12.0, 3),
      Self::Neon => (0.3, 2.5, 6.0, 2),
      Self::Explosive => (0.15, 4.0, 18.0, 4),
      Self::Custom => (0.7, 1.0, 5.0, 2), // starting values
    }
  }
}

const PRESETS: [BloomPreset; 6] = [
  BloomPreset::Off,
  BloomPreset::Subtle,
  BloomPreset::Dreamy,
  BloomPreset::Neon,
  BloomPreset::Explosive,
  BloomPreset::Custom,
];

struct BloomDemo {
  // Scene entities
  bg_tiles: Vec<Entity>,
  bright_sprites: Vec<Entity>,
  orbiting_sprites: Vec<Entity>,

  // State
  current_preset: usize,
  threshold: f32,
  intensity: f32,
  radius: f32,
  blur_passes: u32,
  time: f32,
}

impl BloomDemo {
  fn new() -> Self {
    let (threshold, intensity, radius, blur_passes) = PRESETS[0].params();
    Self {
      bg_tiles: Vec::new(),
      bright_sprites: Vec::new(),
      orbiting_sprites: Vec::new(),
      current_preset: 0,
      threshold,
      intensity,
      radius,
      blur_passes,
      time: 0.0,
    }
  }

  /// Build a scene with a mix of dark and bright elements so bloom is clearly visible.
  fn build_scene(&mut self, ctx: &mut GameContext) {
    // Dark background grid — these should NOT bloom
    let dark_colors: [(u8, u8, u8); 4] = [(30, 30, 50), (40, 30, 50), (30, 40, 50), (35, 35, 55)];

    for row in 0..8 {
      for col in 0..12 {
        let (r, g, b) = dark_colors[(row + col) % dark_colors.len()];
        let sprite = Sprite::solid_rect(80, 80, r, g, b, 255);
        let handle = ctx.assets.create_sprite(sprite, &format!("bg_{}_{}", row, col));
        let entity = ctx.world.spawn((
          Transform::new(col as f32 * 80.0, row as f32 * 80.0),
          Renderable::sprite(handle).with_z_order(0),
        ));
        self.bg_tiles.push(entity);
      }
    }

    // Bright center sprites — these WILL bloom
    let bright_colors: [(u8, u8, u8); 5] = [
      (255, 255, 255), // pure white
      (255, 220, 80),  // gold
      (80, 200, 255),  // sky blue
      (255, 100, 200), // hot pink
      (140, 255, 100), // lime green
    ];

    let cx = 480.0;
    let cy = 320.0;

    for (i, &(r, g, b)) in bright_colors.iter().enumerate() {
      let size = 40 + i as u32 * 6;
      let sprite = Sprite::solid_rect(size, size, r, g, b, 255);
      let handle = ctx.assets.create_sprite(sprite, &format!("bright_{}", i));

      let angle = (i as f32 / bright_colors.len() as f32) * std::f32::consts::TAU;
      let dist = 120.0;
      let entity = ctx.world.spawn((
        Transform::new(cx + angle.cos() * dist, cy + angle.sin() * dist),
        Renderable::sprite(handle).with_z_order(2),
      ));
      self.bright_sprites.push(entity);
    }

    // Small orbiting particles — fast-moving bright dots
    let orbit_colors: [(u8, u8, u8); 8] = [
      (255, 255, 200),
      (200, 255, 255),
      (255, 200, 255),
      (255, 240, 150),
      (150, 255, 220),
      (220, 150, 255),
      (255, 180, 180),
      (180, 255, 180),
    ];
    for (i, &(r, g, b)) in orbit_colors.iter().enumerate() {
      let sprite = Sprite::solid_rect(12, 12, r, g, b, 255);
      let handle = ctx.assets.create_sprite(sprite, &format!("orbit_{}", i));
      let entity = ctx.world.spawn((
        Transform::new(cx, cy),
        Renderable::sprite(handle).with_z_order(3),
      ));
      self.orbiting_sprites.push(entity);
    }
  }

  fn animate(&mut self, ctx: &mut GameContext) {
    let cx = 480.0;
    let cy = 320.0;

    // Rotate bright center sprites slowly
    for (i, &entity) in self.bright_sprites.iter().enumerate() {
      if let Some(mut t) = ctx.world.get_component_mut::<Transform>(entity) {
        let base_angle = (i as f32 / self.bright_sprites.len() as f32) * std::f32::consts::TAU;
        let angle = base_angle + self.time * 0.3;
        let dist = 120.0 + (self.time * 0.8 + i as f32).sin() * 20.0;
        t.x = cx + angle.cos() * dist;
        t.y = cy + angle.sin() * dist;
        t.rotation = self.time * 1.5;
      }
    }

    // Fast orbiting particles at varying radii
    for (i, &entity) in self.orbiting_sprites.iter().enumerate() {
      if let Some(mut t) = ctx.world.get_component_mut::<Transform>(entity) {
        let speed = 1.0 + (i as f32) * 0.25;
        let r = 60.0 + (i as f32) * 22.0;
        let angle = self.time * speed + (i as f32 / self.orbiting_sprites.len() as f32) * std::f32::consts::TAU;
        t.x = cx + angle.cos() * r;
        t.y = cy + angle.sin() * r;
      }
    }
  }

  fn apply_bloom(&self, ctx: &mut GameContext) {
    ctx.effects.clear();

    if self.intensity > 0.001 {
      ctx
        .effects
        .set_bloom_custom(self.threshold, self.intensity, self.radius, self.blur_passes);
    }
  }

  fn switch_preset(&mut self, index: usize) {
    self.current_preset = index;
    let (threshold, intensity, radius, blur_passes) = PRESETS[index].params();
    self.threshold = threshold;
    self.intensity = intensity;
    self.radius = radius;
    self.blur_passes = blur_passes;
  }

  fn handle_input(&mut self, ctx: &mut GameContext) {
    // Preset switching: 1-6
    if ctx.input.key_pressed(KeyCode::Digit1) {
      self.switch_preset(0);
    } else if ctx.input.key_pressed(KeyCode::Digit2) {
      self.switch_preset(1);
    } else if ctx.input.key_pressed(KeyCode::Digit3) {
      self.switch_preset(2);
    } else if ctx.input.key_pressed(KeyCode::Digit4) {
      self.switch_preset(3);
    } else if ctx.input.key_pressed(KeyCode::Digit5) {
      self.switch_preset(4);
    } else if ctx.input.key_pressed(KeyCode::Digit6) {
      self.switch_preset(5);
    }

    // Live parameter tweaking (works in any preset, switches to Custom if changed)
    let mut tweaked = false;

    if ctx.input.key_down(KeyCode::ArrowUp) {
      self.intensity = (self.intensity + 0.02).min(5.0);
      tweaked = true;
    }
    if ctx.input.key_down(KeyCode::ArrowDown) {
      self.intensity = (self.intensity - 0.02).max(0.0);
      tweaked = true;
    }
    if ctx.input.key_down(KeyCode::ArrowRight) {
      self.radius = (self.radius + 0.1).min(20.0);
      tweaked = true;
    }
    if ctx.input.key_down(KeyCode::ArrowLeft) {
      self.radius = (self.radius - 0.1).max(1.0);
      tweaked = true;
    }
    if ctx.input.key_down(KeyCode::KeyW) {
      self.threshold = (self.threshold + 0.005).min(1.0);
      tweaked = true;
    }
    if ctx.input.key_down(KeyCode::KeyS) {
      self.threshold = (self.threshold - 0.005).max(0.0);
      tweaked = true;
    }
    if ctx.input.key_pressed(KeyCode::KeyQ) {
      self.blur_passes = (self.blur_passes.saturating_sub(1)).max(1);
      tweaked = true;
    }
    if ctx.input.key_pressed(KeyCode::KeyE) {
      self.blur_passes = (self.blur_passes + 1).min(5);
      tweaked = true;
    }

    if tweaked && PRESETS[self.current_preset] != BloomPreset::Custom {
      self.current_preset = 5; // Switch to Custom
    }

    if ctx.input.key_pressed(KeyCode::Escape) {
      std::process::exit(0);
    }
  }
}

impl Game for BloomDemo {
  fn init(&mut self, ctx: &mut GameContext) {
    self.build_scene(ctx);
    // Start on "Subtle" so bloom is visible immediately
    self.switch_preset(1);
  }

  fn update(&mut self, ctx: &mut GameContext, dt: f32) {
    self.time += dt;
    self.handle_input(ctx);
    self.animate(ctx);
    self.apply_bloom(ctx);
  }

  fn render_ui(&mut self, ui: &mut UiContext) {
    let preset = &PRESETS[self.current_preset];

    // Title
    ui.draw_text("BLOOM EFFECT DEMO", 10.0, 10.0, Color::WHITE);

    // Current preset
    ui.draw_text(
      &format!("[{}] {}", self.current_preset + 1, preset.name()),
      10.0,
      35.0,
      Color::rgb(100, 255, 100),
    );

    // Description
    for (i, line) in preset.description().lines().enumerate() {
      ui.draw_text(line, 10.0, 58.0 + i as f32 * 15.0, Color::rgb(200, 200, 200));
    }

    // Live parameter readout
    let py = 100.0;
    ui.draw_text(
      &format!("threshold: {:.2}", self.threshold),
      10.0,
      py,
      Color::rgb(255, 255, 100),
    );
    ui.draw_text(
      &format!("intensity: {:.2}", self.intensity),
      10.0,
      py + 16.0,
      Color::rgb(255, 255, 100),
    );
    ui.draw_text(
      &format!("radius:    {:.1}", self.radius),
      10.0,
      py + 32.0,
      Color::rgb(255, 255, 100),
    );
    ui.draw_text(
      &format!("blur_passes: {}", self.blur_passes),
      10.0,
      py + 48.0,
      Color::rgb(255, 255, 100),
    );

    // Controls
    let cy = 560.0;
    ui.draw_text("1-6: Presets", 10.0, cy, Color::rgb(180, 180, 180));
    ui.draw_text(
      "Up/Down: intensity  Left/Right: radius",
      10.0,
      cy + 15.0,
      Color::rgb(180, 180, 180),
    );
    ui.draw_text(
      "W/S: threshold  Q/E: blur passes",
      10.0,
      cy + 30.0,
      Color::rgb(180, 180, 180),
    );
    ui.draw_text("ESC: Quit", 10.0, cy + 45.0, Color::rgb(180, 180, 180));
  }
}

fn main() {
  EngineBuilder::new()
    .title("Bloom Effect Demo")
    .window_mode(WindowMode::ResizableAspectRatio)
    .with_game(BloomDemo::new())
    .build()
    .run()
    .expect("Failed to run engine");
}
