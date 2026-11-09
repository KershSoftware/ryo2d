//! Life: what a particle does over its life, side by side: a colour ramp,
//! a size curve, a speed curve, and a smoke sheet played over life and at a
//! frame rate

use demo_kit::prelude::*;

use crate::shared::{count_label_below, emit, label_centered, particle_image};

/// Where each column sits, and the width of its plot
const COLUMNS: [f32; 5] = [100.0, 240.0, 380.0, 520.0, 660.0];
const PLOT: f32 = 90.0;

/// The five columns' titles
const TITLES: [&str; 5] = ["Colour", "Size", "Speed", "Sheet: life", "Sheet: fps"];

pub struct LifePage {
  /// Frames a second of the last column's sheet
  fps: f32,
  /// The effect of the last column, whose frame rate follows `fps`
  sheet: Option<Handle<ParticleEffect>>,
  /// The shapes drawn under the columns
  ramp: Gradient,
  size: Curve,
  speed: Curve,
}

impl Default for LifePage {
  fn default() -> Self {
    Self {
      fps: 8.0,
      sheet: None,
      ramp: Gradient::new([
        (0.0, Color::new(255, 240, 160, 255)),
        (0.4, Color::new(255, 120, 30, 255)),
        (1.0, Color::new(90, 0, 120, 0)),
      ]),
      size: Curve::eased([
        (0.0, 0.2, Ease::Linear),
        (0.35, 1.0, Ease::OutQuad),
        (1.0, 0.0, Ease::InQuad),
      ]),
      speed: Curve::new([(0.0, 1.0), (0.5, 0.35), (1.0, 0.0)]),
    }
  }
}

impl DemoPage for LifePage {
  fn name(&self) -> &str {
    "Life"
  }

  fn description(&self) -> &str {
    "Colour, size and speed over a particle's life, and a smoke sheet over life and at a frame rate"
  }

  fn settings(&mut self, s: &mut Settings) {
    s.float("Sheet fps", &mut self.fps, 1.0..=24.0, 1.0);
  }

  fn enter(&mut self, ctx: &mut SceneContext) {
    ctx.camera().set_position(640.0, 360.0);
    let disc = particle_image(ctx);
    let smoke = ctx
      .assets()
      .load_now::<Image>("fx/smoke_sheet.png")
      .unwrap_or_else(|error| panic!("fx/smoke_sheet.png: {}", error.report()));

    // Every column shoots straight up from the same height
    let column = |image: &Handle<Image>| {
      ParticleEffect::new(image.clone())
        .with_rate(10.0)
        // A cycle as long as a life, played ahead, so each column starts full
        .with_duration(3.0)
        .with_prewarm(true)
        .with_direction(-90.0, 0.0)
        .with_speed(200.0)
        .with_lifetime(2.0)
        .with_size(18.0)
        .with_color(Color::WHITE)
    };
    let effects = [
      column(&disc).with_color_over_life(self.ramp.clone()),
      column(&disc).with_size_over_life(self.size.clone()),
      column(&disc).with_speed_over_life(self.speed.clone()),
      column(&smoke)
        .with_size(54.0)
        .with_speed(80.0)
        .with_rate(1.5)
        .with_lifetime(3.0)
        .with_color_over_life(Gradient::constant(Color::new(210, 220, 235, 255)))
        .with_frames(Flipbook::new(4, 2)),
      column(&smoke)
        .with_size(54.0)
        .with_speed(80.0)
        .with_rate(1.5)
        .with_lifetime(3.0)
        .with_color_over_life(Gradient::constant(Color::new(210, 220, 235, 255)))
        .with_frames(Flipbook::new(4, 2).at_fps(self.fps)),
    ];
    for (i, (x, effect)) in COLUMNS.into_iter().zip(effects).enumerate() {
      let effect = ctx.assets().add(effect);
      emit(ctx, x, 620.0, &effect);
      if i == COLUMNS.len() - 1 {
        self.sheet = Some(effect);
      }
    }
  }

  fn update(&mut self, ctx: &mut SceneContext, _dt: f32) {
    // The setting changes the frame rate of the sheet playing now
    if let Some(sheet) = &self.sheet
      && let Some(effect) = ctx.assets().get_mut(sheet)
      && let Some(frames) = &mut effect.frames
      && frames.mode != FrameMode::Fps(self.fps)
    {
      frames.mode = FrameMode::Fps(self.fps);
    }
  }

  fn ui(&mut self, ui: &mut UiContext, ctx: &mut SceneContext) {
    count_label_below(ui, ctx, 1);
    for (x, title) in COLUMNS.into_iter().zip(TITLES) {
      label_centered(ui, x, 660.0, title, Color::WHITE);
    }
    // Each curve drawn above its column: the ramp as a bar, the others as
    // lines from birth (left) to death (right)
    let top = 150.0;
    let steps = 24;
    for i in 0..steps {
      let t = i as f32 / (steps - 1) as f32;
      let mut color = self.ramp.sample(t);
      color.a = 255;
      let w = PLOT / steps as f32;
      ui.draw_rect(
        Rect::new(COLUMNS[0] - PLOT / 2.0 + i as f32 * w, top, w + 1.0, 24.0),
        color,
      );
    }
    for (x, curve) in [(COLUMNS[1], &self.size), (COLUMNS[2], &self.speed)] {
      let points: Vec<Vec2> = (0..steps)
        .map(|i| {
          let t = i as f32 / (steps - 1) as f32;
          Vec2::new(
            x - PLOT / 2.0 + t * PLOT,
            top + 40.0 - curve.sample(t) * 40.0,
          )
        })
        .collect();
      ui.draw_line(&points, 2.0, Color::GREEN);
    }
  }
}
