//! Bloom Effect Demo
//!
//! Bright pixels bleed light into their neighbours: a dark tile grid with bright
//! orbiting sprites, a preset choice and live bloom parameters. Runs on the
//! demo-kit shell.

use demo_kit::prelude::*;

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
  fn description(&self) -> &'static str {
    match self {
      Self::Off => "No bloom. Base scene for comparison.",
      Self::Subtle => "Low threshold, low intensity. Gentle highlight glow.",
      Self::Dreamy => "Mid threshold, high radius. Soft ethereal look.",
      Self::Neon => "Low threshold, high intensity. Punchy neon vibe.",
      Self::Explosive => "Very low threshold, max intensity & radius.",
      Self::Custom => "Your own values: tweak the rows on the right.",
    }
  }

  fn params(&self) -> (f32, f32, f32, i32) {
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

const PRESET_NAMES: [&str; 6] = ["Off", "Subtle", "Dreamy", "Neon", "Explosive", "Custom"];
const CUSTOM_PRESET: usize = 5;

const PRESETS: [BloomPreset; 6] = [
  BloomPreset::Off,
  BloomPreset::Subtle,
  BloomPreset::Dreamy,
  BloomPreset::Neon,
  BloomPreset::Explosive,
  BloomPreset::Custom,
];

struct BloomPage {
  // Scene entities
  bg_tiles: Vec<Entity>,
  bright_sprites: Vec<Entity>,
  orbiting_sprites: Vec<Entity>,

  // State
  current_preset: usize,
  threshold: f32,
  intensity: f32,
  radius: f32,
  blur_passes: i32,
  // Values seen last frame, to notice edits made through the settings rows
  last_preset: usize,
  last_values: (f32, f32, f32, i32),
  time: f32,

  // Screen dimensions (set in enter)
  screen_width: f32,
  screen_height: f32,
}

impl BloomPage {
  fn new() -> Self {
    let (threshold, intensity, radius, blur_passes) = PRESETS[1].params();
    Self {
      bg_tiles: Vec::new(),
      bright_sprites: Vec::new(),
      orbiting_sprites: Vec::new(),
      current_preset: 1,
      threshold,
      intensity,
      radius,
      blur_passes,
      last_preset: 1,
      last_values: (threshold, intensity, radius, blur_passes),
      time: 0.0,
      screen_width: 0.0,
      screen_height: 0.0,
    }
  }

  /// Build a scene with a mix of dark and bright elements so bloom is clearly visible.
  fn build_scene(&mut self, ctx: &mut SceneContext) {
    let cx = self.screen_width / 2.0;
    let cy = self.screen_height / 2.0;

    // Dark background grid — these should NOT bloom
    let dark_colors: [(u8, u8, u8); 4] = [(30, 30, 50), (40, 30, 50), (30, 40, 50), (35, 35, 55)];

    let tile_size = 80.0;
    let cols = (self.screen_width / tile_size).ceil() as usize;
    let rows = (self.screen_height / tile_size).ceil() as usize;

    for row in 0..rows {
      for col in 0..cols {
        let (r, g, b) = dark_colors[(row + col) % dark_colors.len()];
        let sprite = Sprite::solid_rect(80, 80, r, g, b, 255);
        let handle = ctx
          .assets
          .create_sprite(sprite, format!("bg_{}_{}", row, col));
        let entity = ctx
          .world
          .spawn((
            Transform::from_xy(col as f32 * tile_size, row as f32 * tile_size),
            Renderable::sprite(handle)
              .with_anchor(Anchor::TopLeft)
              .with_z_order(0),
          ))
          .id();
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

    for (i, &(r, g, b)) in bright_colors.iter().enumerate() {
      let size = 40 + i as u32 * 6;
      let sprite = Sprite::solid_rect(size, size, r, g, b, 255);
      let handle = ctx.assets.create_sprite(sprite, format!("bright_{}", i));

      let angle = (i as f32 / bright_colors.len() as f32) * std::f32::consts::TAU;
      let dist = 120.0;
      let entity = ctx
        .world
        .spawn((
          Transform::from_xy(cx + angle.cos() * dist, cy + angle.sin() * dist),
          Renderable::sprite(handle).with_z_order(2),
        ))
        .id();
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
      let handle = ctx.assets.create_sprite(sprite, format!("orbit_{}", i));
      let entity = ctx
        .world
        .spawn((
          Transform::from_xy(cx, cy),
          Renderable::sprite(handle).with_z_order(3),
        ))
        .id();
      self.orbiting_sprites.push(entity);
    }
  }

  fn animate(&mut self, ctx: &mut SceneContext) {
    let cx = self.screen_width / 2.0;
    let cy = self.screen_height / 2.0;

    // Rotate bright center sprites slowly
    for (i, &entity) in self.bright_sprites.iter().enumerate() {
      if let Some(mut t) = ctx.world.get_mut::<Transform>(entity) {
        let base_angle = (i as f32 / self.bright_sprites.len() as f32) * std::f32::consts::TAU;
        let angle = base_angle + self.time * 0.3;
        let dist = 120.0 + (self.time * 0.8 + i as f32).sin() * 20.0;
        t.translation = Vec2::new(cx + angle.cos() * dist, cy + angle.sin() * dist);
        t.rotation = self.time * 1.5;
      }
    }

    // Fast orbiting particles at varying radii
    for (i, &entity) in self.orbiting_sprites.iter().enumerate() {
      if let Some(mut t) = ctx.world.get_mut::<Transform>(entity) {
        let speed = 1.0 + (i as f32) * 0.25;
        let r = 60.0 + (i as f32) * 22.0;
        let angle = self.time * speed
          + (i as f32 / self.orbiting_sprites.len() as f32) * std::f32::consts::TAU;
        t.translation = Vec2::new(cx + angle.cos() * r, cy + angle.sin() * r);
      }
    }
  }

  fn apply_bloom(&self, ctx: &mut SceneContext) {
    ctx.effects().clear();

    if self.intensity > 0.001 {
      ctx.effects().set_bloom_custom(
        self.threshold,
        self.intensity,
        self.radius,
        self.blur_passes.max(1) as u32,
      );
    }
  }

  fn switch_preset(&mut self, index: usize) {
    self.current_preset = index;
    if index != CUSTOM_PRESET {
      let (threshold, intensity, radius, blur_passes) = PRESETS[index].params();
      self.threshold = threshold;
      self.intensity = intensity;
      self.radius = radius;
      self.blur_passes = blur_passes;
    }
    self.remember_values();
  }

  fn values(&self) -> (f32, f32, f32, i32) {
    (
      self.threshold,
      self.intensity,
      self.radius,
      self.blur_passes,
    )
  }

  fn remember_values(&mut self) {
    self.last_preset = self.current_preset;
    self.last_values = self.values();
  }

  /// Picking a preset loads its values; editing a value under any other
  /// preset switches to Custom.
  fn sync_preset(&mut self) {
    if self.current_preset != self.last_preset {
      self.switch_preset(self.current_preset);
    } else if self.values() != self.last_values {
      if self.current_preset != CUSTOM_PRESET {
        self.current_preset = CUSTOM_PRESET;
      }
      self.remember_values();
    }
  }
}

impl DemoPage for BloomPage {
  fn name(&self) -> &str {
    "Bloom"
  }

  fn description(&self) -> &str {
    PRESETS[self.current_preset.min(CUSTOM_PRESET)].description()
  }

  fn settings(&mut self, s: &mut Settings) {
    s.choice("Preset", &mut self.current_preset, &PRESET_NAMES);
    s.float("Intensity", &mut self.intensity, 0.0..=5.0, 0.1);
    s.float("Radius", &mut self.radius, 1.0..=20.0, 0.5);
    s.float("Threshold", &mut self.threshold, 0.0..=1.0, 0.05);
    s.int("Blur passes", &mut self.blur_passes, 1..=5);
  }

  fn enter(&mut self, ctx: &mut SceneContext) {
    let (width, height) = ctx.screen_size();
    self.screen_width = width;
    self.screen_height = height;

    self.build_scene(ctx);
    // Start on "Subtle" so bloom is visible immediately
    self.switch_preset(1);
  }

  fn update(&mut self, ctx: &mut SceneContext, dt: f32) {
    self.time += dt;
    self.sync_preset();
    self.animate(ctx);
    self.apply_bloom(ctx);
  }
}

fn main() {
  Demo::new("Bloom")
    .resolution(960, 640)
    .page(BloomPage::new)
    .run();
}
