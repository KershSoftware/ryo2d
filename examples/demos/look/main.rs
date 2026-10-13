//! The look: emission and bloom, tonemap curves, colour grading, LUTs made
//! in code and auto-exposure. Loads no files, so it runs on the web too.

use demo_kit::prelude::*;

fn main() {
  Demo::new("Look")
    .resolution(960, 640)
    .page(GlowPage::default)
    .page(CurvesPage::default)
    .page(GradingPage::default)
    .page(LutPage::default)
    .page(ExposurePage::default)
    .run();
}

/// A grey backdrop over the whole window.
fn backdrop(ctx: &mut SceneContext, grey: u8) {
  let sprite = SpriteHandle::from_sprite(
    Sprite::solid_rect(960, 640, grey, grey, grey, 255),
    format!("look-backdrop-{grey}"),
  );
  ctx.world.spawn((
    Transform::from_xy(480.0, 320.0),
    Renderable::sprite(sprite).with_z_order(-10),
  ));
}

/// A square of `color`, `size` wide, at `at`, glowing by `emission`.
fn square(
  ctx: &mut SceneContext,
  at: (f32, f32),
  size: u32,
  color: Color,
  emission: f32,
) -> Entity {
  let sprite = SpriteHandle::from_sprite(
    Sprite::solid_rect(size, size, color.r, color.g, color.b, 255),
    format!("look-square-{size}-{}-{}-{}", color.r, color.g, color.b),
  );
  ctx
    .world
    .spawn((
      Transform::from_xy(at.0, at.1),
      Renderable::sprite(sprite).with_emission(emission),
    ))
    .id()
}

/// A colour chart left of the settings panel: six hues at three
/// brightnesses, and a grey ramp.
fn palette(ctx: &mut SceneContext) {
  backdrop(ctx, 30);
  let hues = [
    (230, 60, 50),
    (240, 170, 40),
    (230, 220, 60),
    (70, 190, 80),
    (60, 130, 230),
    (170, 80, 210),
  ];
  for (i, &(r, g, b)) in hues.iter().enumerate() {
    for (j, scale) in [1.0, 0.6, 0.3].into_iter().enumerate() {
      let shade = |c: u8| (f32::from(c) * scale) as u8;
      let at = (70.0 + i as f32 * 70.0, 180.0 + j as f32 * 70.0);
      square(ctx, at, 56, Color::rgb(shade(r), shade(g), shade(b)), 1.0);
    }
  }
  for i in 0..7u32 {
    let v = (i * 255 / 6) as u8;
    square(
      ctx,
      (70.0 + i as f32 * 60.0, 430.0),
      48,
      Color::rgb(v, v, v),
      1.0,
    );
  }
}

// ---------------------------------------------------------------------------
// Glow
// ---------------------------------------------------------------------------

const EMISSIONS: [f32; 5] = [1.0, 2.0, 4.0, 8.0, 16.0];

/// Squares from 1 to 16 times white, under bloom.
struct GlowPage {
  bloom: f32,
  threshold: f32,
  boost: f32,
  squares: Vec<(Entity, f32)>,
}

impl Default for GlowPage {
  fn default() -> Self {
    Self {
      bloom: 1.0,
      threshold: 1.0,
      boost: 1.0,
      squares: Vec::new(),
    }
  }
}

impl DemoPage for GlowPage {
  fn name(&self) -> &str {
    "Glow"
  }

  fn description(&self) -> &str {
    "Emission lifts light past white; bloom glows only from light above its threshold."
  }

  fn settings(&mut self, s: &mut Settings) {
    s.float("Bloom", &mut self.bloom, 0.0..=3.0, 0.1);
    s.float("Threshold", &mut self.threshold, 0.0..=4.0, 0.1);
    s.float("Emission x", &mut self.boost, 0.0..=4.0, 0.25);
  }

  fn enter(&mut self, ctx: &mut SceneContext) {
    backdrop(ctx, 20);
    self.squares = EMISSIONS
      .iter()
      .enumerate()
      .map(|(i, &emission)| {
        let at = (80.0 + i as f32 * 100.0, 320.0);
        (
          square(ctx, at, 48, Color::rgb(255, 200, 120), emission),
          emission,
        )
      })
      .collect();
  }

  fn update(&mut self, ctx: &mut SceneContext, _dt: f32) {
    ctx
      .effects()
      .set_bloom_custom(self.threshold, self.bloom, 6.0, 3);
    for &(entity, emission) in &self.squares {
      if let Some(mut renderable) = ctx.world.get_mut::<Renderable>(entity) {
        renderable.emission = emission * self.boost;
      }
    }
  }

  fn ui(&mut self, ui: &mut UiContext, _ctx: &mut SceneContext) {
    for (i, emission) in EMISSIONS.iter().enumerate() {
      let x = 66.0 + i as f32 * 100.0;
      ui.draw_text(
        &format!("x{}", emission * self.boost),
        x,
        370.0,
        Color::WHITE,
      );
    }
  }
}

// ---------------------------------------------------------------------------
// Curves
// ---------------------------------------------------------------------------

const CURVES: [&str; 4] = ["Soft", "Clamp", "ACES", "AgX"];

/// Ramps of light from a quarter of white to 64 times it.
struct CurvesPage {
  curve: usize,
  exposure: f32,
  knee: f32,
  white: f32,
}

impl Default for CurvesPage {
  fn default() -> Self {
    Self {
      curve: 0,
      exposure: 0.0,
      knee: 0.8,
      white: 4.0,
    }
  }
}

impl DemoPage for CurvesPage {
  fn name(&self) -> &str {
    "Curves"
  }

  fn description(&self) -> &str {
    "The tonemap curve rolls light above white off to the display: ramps from 1/4 to 64 times white."
  }

  fn settings(&mut self, s: &mut Settings) {
    s.choice("Curve", &mut self.curve, &CURVES);
    s.float("Exposure (EV)", &mut self.exposure, -4.0..=4.0, 0.25);
    if self.curve == 0 {
      s.float("Knee", &mut self.knee, 0.0..=0.95, 0.05);
      s.float("White", &mut self.white, 1.0..=16.0, 0.5);
    }
  }

  fn enter(&mut self, ctx: &mut SceneContext) {
    backdrop(ctx, 10);
    let rows = [
      Color::rgb(255, 255, 255),
      Color::rgb(255, 140, 40),
      Color::rgb(60, 120, 255),
    ];
    for (row, color) in rows.into_iter().enumerate() {
      for step in 0..9 {
        let emission = 0.25 * 2f32.powi(step);
        let at = (50.0 + step as f32 * 52.0, 220.0 + row as f32 * 80.0);
        square(ctx, at, 40, color, emission);
      }
    }
  }

  fn update(&mut self, ctx: &mut SceneContext, _dt: f32) {
    let tonemap = match self.curve {
      0 => Tonemap::Soft {
        knee: self.knee,
        white: self.white,
      },
      1 => Tonemap::Clamp,
      2 => Tonemap::Aces,
      _ => Tonemap::AgX,
    };
    let mut look = ctx.look();
    look.tonemap = tonemap;
    look.exposure = self.exposure;
  }
}

// ---------------------------------------------------------------------------
// Grading
// ---------------------------------------------------------------------------

/// White balance, contrast, saturation, lift, gamma and gain on a colour chart.
struct GradingPage {
  temperature: f32,
  tint: f32,
  contrast: f32,
  saturation: f32,
  lift: f32,
  gamma: f32,
  gain: f32,
}

impl Default for GradingPage {
  fn default() -> Self {
    Self {
      temperature: 0.0,
      tint: 0.0,
      contrast: 1.0,
      saturation: 1.0,
      lift: 0.0,
      gamma: 1.0,
      gain: 1.0,
    }
  }
}

impl DemoPage for GradingPage {
  fn name(&self) -> &str {
    "Grading"
  }

  fn description(&self) -> &str {
    "White balance, contrast and saturation on the light; lift, gamma and gain on the display colour."
  }

  fn settings(&mut self, s: &mut Settings) {
    s.float("Temperature", &mut self.temperature, -1.0..=1.0, 0.05);
    s.float("Tint", &mut self.tint, -1.0..=1.0, 0.05);
    s.float("Contrast", &mut self.contrast, 0.25..=3.0, 0.05);
    s.float("Saturation", &mut self.saturation, 0.0..=2.0, 0.05);
    s.float("Lift", &mut self.lift, -0.2..=0.2, 0.01);
    s.float("Gamma", &mut self.gamma, 0.25..=3.0, 0.05);
    s.float("Gain", &mut self.gain, 0.0..=2.0, 0.05);
  }

  fn enter(&mut self, ctx: &mut SceneContext) {
    palette(ctx);
  }

  fn update(&mut self, ctx: &mut SceneContext, _dt: f32) {
    ctx.look().grading = ColorGrading {
      temperature: self.temperature,
      tint: self.tint,
      contrast: self.contrast,
      saturation: self.saturation,
      lift: Vec3::splat(self.lift),
      gamma: Vec3::splat(self.gamma),
      gain: Vec3::splat(self.gain),
      ..ColorGrading::default()
    };
  }
}

// ---------------------------------------------------------------------------
// LUT
// ---------------------------------------------------------------------------

const LUTS: [&str; 4] = ["Warm film", "Cool night", "Sepia", "Invert"];

/// A 16-step LUT made by mapping every colour of the identity strip.
fn make_lut(name: &str, map: impl Fn([f32; 3]) -> [f32; 3]) -> Option<ColorLut> {
  let strip = ColorLut::identity_strip(16);
  let byte = |c: f32| (c.clamp(0.0, 1.0) * 255.0).round() as u8;
  let data: Vec<u8> = strip
    .data()
    .chunks(4)
    .flat_map(|p| {
      let [r, g, b] = map([p[0], p[1], p[2]].map(|c| f32::from(c) / 255.0));
      [byte(r), byte(g), byte(b), 255]
    })
    .collect();
  let sprite = Sprite::from_rgba(data, strip.width(), strip.height());
  ColorLut::from_sprite(&SpriteHandle::from_sprite(
    sprite,
    format!("look-lut-{name}"),
  ))
  .ok()
}

fn lut(index: usize) -> Option<ColorLut> {
  match index {
    0 => make_lut("warm", |[r, g, b]| [r * 1.08 + 0.03, g + 0.01, b * 0.85]),
    1 => make_lut("cool", |[r, g, b]| {
      [r * 0.8, g * 0.92 + 0.02, b * 1.1 + 0.05]
    }),
    2 => make_lut("sepia", |[r, g, b]| {
      let y = 0.299 * r + 0.587 * g + 0.114 * b;
      [y * 1.07 + 0.05, y * 0.95 + 0.02, y * 0.75]
    }),
    _ => make_lut("invert", |[r, g, b]| [1.0 - r, 1.0 - g, 1.0 - b]),
  }
}

/// LUTs built in code, blended by strength.
struct LutPage {
  choice: usize,
  strength: f32,
  luts: Vec<Option<ColorLut>>,
}

impl Default for LutPage {
  fn default() -> Self {
    Self {
      choice: 0,
      strength: 1.0,
      luts: Vec::new(),
    }
  }
}

impl DemoPage for LutPage {
  fn name(&self) -> &str {
    "LUT"
  }

  fn description(&self) -> &str {
    "Colour lookup tables made in code from the identity strip, blended by strength."
  }

  fn settings(&mut self, s: &mut Settings) {
    s.choice("LUT", &mut self.choice, &LUTS);
    s.float("Strength", &mut self.strength, 0.0..=1.0, 0.05);
  }

  fn enter(&mut self, ctx: &mut SceneContext) {
    palette(ctx);
    self.luts = (0..LUTS.len()).map(lut).collect();
  }

  fn update(&mut self, ctx: &mut SceneContext, _dt: f32) {
    let mut look = ctx.look();
    look.grading.lut = self.luts.get(self.choice).cloned().flatten();
    look.grading.lut_strength = self.strength;
  }
}

// ---------------------------------------------------------------------------
// Auto-exposure
// ---------------------------------------------------------------------------

/// A dark cave opening onto daylight; the view pans between them.
struct ExposurePage {
  auto: bool,
  compensation: f32,
  speed_up: f32,
  speed_down: f32,
  time: f32,
}

impl Default for ExposurePage {
  fn default() -> Self {
    Self {
      auto: true,
      compensation: 0.0,
      speed_up: 3.0,
      speed_down: 1.0,
      time: 0.0,
    }
  }
}

impl DemoPage for ExposurePage {
  fn name(&self) -> &str {
    "Auto-exposure"
  }

  fn description(&self) -> &str {
    "Exposure follows the scene as the view pans between a dark cave and daylight."
  }

  fn settings(&mut self, s: &mut Settings) {
    s.toggle("Auto-exposure", &mut self.auto);
    s.float(
      "Compensation (EV)",
      &mut self.compensation,
      -2.0..=2.0,
      0.25,
    );
    s.float("Brightening (EV/s)", &mut self.speed_up, 0.25..=8.0, 0.25);
    s.float("Darkening (EV/s)", &mut self.speed_down, 0.25..=8.0, 0.25);
  }

  fn enter(&mut self, ctx: &mut SceneContext) {
    // Rock from x -600 to 600, sky from 600 to 1500
    let rock =
      SpriteHandle::from_sprite(Sprite::solid_rect(1200, 640, 40, 34, 30, 255), "look-cave");
    ctx.world.spawn((
      Transform::from_xy(0.0, 320.0),
      Renderable::sprite(rock).with_z_order(-1),
    ));
    let sky =
      SpriteHandle::from_sprite(Sprite::solid_rect(900, 640, 200, 220, 255, 255), "look-sky");
    ctx.world.spawn((
      Transform::from_xy(1050.0, 320.0),
      Renderable::sprite(sky).with_z_order(-1).with_emission(4.0),
    ));
    for i in 0..6 {
      square(
        ctx,
        (-400.0 + i as f32 * 120.0, 420.0),
        40,
        Color::rgb(90, 70, 50),
        1.0,
      );
    }
    for i in 0..5 {
      square(
        ctx,
        (800.0 + i as f32 * 110.0, 400.0),
        40,
        Color::rgb(60, 140, 60),
        4.0,
      );
    }
  }

  fn update(&mut self, ctx: &mut SceneContext, dt: f32) {
    self.time += dt;
    let x = 450.0 + 500.0 * (self.time * 0.25).sin();
    ctx.camera().set_position(x, 320.0);
    ctx.look().auto_exposure = self.auto.then_some(AutoExposure {
      compensation: self.compensation,
      speed_up: self.speed_up,
      speed_down: self.speed_down,
      ..AutoExposure::default()
    });
  }
}
