//! Headless smoke tests of what the shell draws

use demo_kit::prelude::*;
use image::RgbaImage;
use ryo2d::testing::HeadlessEngine;

const RED: Color = Color::new(200, 60, 60, 255);

/// Fills the screen red; `rows` settings rows and `controls` legend items
struct Backdrop {
  rows: usize,
  controls: usize,
  values: Vec<f32>,
}

impl Backdrop {
  fn new(rows: usize, controls: usize) -> Self {
    Self {
      rows,
      controls,
      values: vec![0.5; rows],
    }
  }
}

impl DemoPage for Backdrop {
  fn name(&self) -> &str {
    "Backdrop"
  }
  fn description(&self) -> &str {
    "A page that fills the screen so the shell's drawing can be seen against it."
  }
  fn settings(&mut self, s: &mut Settings) {
    for (i, value) in self.values.iter_mut().enumerate().take(self.rows) {
      s.float(&format!("Value {i}"), value, 0.0..=1.0, 0.1);
    }
  }
  fn controls(&self) -> Vec<Control> {
    (0..self.controls)
      .map(|i| {
        Control::key(
          KeyCode::Digit1,
          format!("A rather long control description {i}"),
        )
      })
      .collect()
  }
  fn render_ui(&mut self, ui: &mut UiContext) {
    let (w, h) = ui.screen_size();
    ui.draw_rect(Rect::new(0.0, 0.0, w as f32, h as f32), RED);
  }
}

fn frame_of(page: Backdrop, size: (u32, u32), keys: &[KeyCode]) -> Option<RgbaImage> {
  let rows = page.rows;
  let controls = page.controls;
  let demo = Demo::new("Chrome")
    .resolution(size.0, size.1)
    .page(move || Backdrop::new(rows, controls));
  let mut engine = HeadlessEngine::new_or_skip(demo.engine_builder())?;
  engine.step_frames(2).ok()?;
  for key in keys {
    engine.tap_key(*key);
    engine.step_frames(2).ok()?;
  }
  engine.frame().ok()
}

fn red_at(image: &RgbaImage, x: u32, y: u32) -> u8 {
  image.get_pixel(x, y).0[0]
}

#[test]
fn the_top_bar_darkens_the_top_of_the_screen() {
  let Some(image) = frame_of(Backdrop::new(0, 0), (320, 240), &[]) else {
    return;
  };
  assert!(red_at(&image, 160, 1) < 80);
  assert!(
    red_at(&image, 160, 120) > 180,
    "no settings: the middle stays clear"
  );
}

#[test]
fn the_settings_panel_appears_only_with_settings() {
  let Some(with) = frame_of(Backdrop::new(10, 0), (320, 240), &[]) else {
    return;
  };
  let Some(without) = frame_of(Backdrop::new(0, 0), (320, 240), &[]) else {
    return;
  };
  assert!(red_at(&with, 312, 120) < 120, "panel in the right column");
  assert!(red_at(&without, 312, 120) > 180);
}

#[test]
fn help_dims_the_screen() {
  let Some(image) = frame_of(Backdrop::new(0, 0), (320, 240), &[KeyCode::KeyH]) else {
    return;
  };
  assert!(red_at(&image, 160, 120) < 150);
}

#[test]
fn tiny_screen_with_many_rows_and_a_long_legend_renders() {
  let Some(image) = frame_of(Backdrop::new(30, 12), (160, 120), &[KeyCode::KeyH]) else {
    return;
  };
  assert_eq!((image.width(), image.height()), (160, 120));
}
