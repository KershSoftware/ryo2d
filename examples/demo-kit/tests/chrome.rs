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
fn tiny_screen_keeps_the_panel_and_legend_in_their_places() {
  let Some(image) = frame_of(Backdrop::new(30, 12), (160, 120), &[]) else {
    return;
  };
  // Block averages, so glyphs don't decide it
  assert!(
    mean_red(&image, 95..150, 45..75) < 120.0,
    "the panel shows in the right column"
  );
  // A dozen long controls stay in a few lines at the bottom instead of
  // covering the page
  assert!(
    mean_red(&image, 10..60, 40..70) > 180.0,
    "the page shows above the legend"
  );
  assert!(
    mean_red(&image, 10..150, 108..116) < 150.0,
    "the legend shows at the bottom"
  );
  let Some(help) = frame_of(Backdrop::new(30, 12), (160, 120), &[KeyCode::KeyH]) else {
    return;
  };
  assert!(
    mean_red(&help, 20..140, 30..90) < 120.0,
    "help covers the screen"
  );
}

fn mean_red(image: &RgbaImage, xs: std::ops::Range<u32>, ys: std::ops::Range<u32>) -> f32 {
  let mut sum = 0.0;
  let mut n = 0.0;
  for y in ys {
    for x in xs.clone() {
      sum += red_at(image, x, y) as f32;
      n += 1.0;
    }
  }
  sum / n
}

/// The bright page's content area and line height, from its last UI frame
type Seen = std::sync::Arc<std::sync::Mutex<Option<(Rect, u32)>>>;

/// A white page with a long description and one control, for text-backing checks
struct Bright {
  seen: Seen,
}

impl DemoPage for Bright {
  fn name(&self) -> &str {
    "Bright"
  }
  fn description(&self) -> &str {
    "A long description line that runs across most of the screen on a bright page."
  }
  fn controls(&self) -> Vec<Control> {
    vec![Control::key(KeyCode::Digit1, "Do the thing")]
  }
  fn render_ui(&mut self, ui: &mut UiContext) {
    let (w, h) = ui.screen_size();
    if let Ok(mut seen) = self.seen.lock() {
      *seen = Some((content_area(ui), ui.text_size("Ag").1));
    }
    ui.draw_rect(
      Rect::new(0.0, 0.0, w as f32, h as f32),
      Color::new(255, 255, 255, 255),
    );
  }
}

/// Where the shell drew around the bright page, read from the public content
/// area: the side padding is its x, the description sits a pad above it and
/// the legend a pad below it
struct Places {
  pad: u32,
  line_h: u32,
  description_mid_y: u32,
  legend_mid_y: u32,
}

/// The bright page's frame at `size`, and where the shell drew
fn bright_frame(size: (u32, u32)) -> Option<(RgbaImage, Places)> {
  let seen = Seen::default();
  let page = seen.clone();
  let demo = Demo::new("Chrome")
    .resolution(size.0, size.1)
    .page(move || Bright { seen: page.clone() });
  let mut engine = HeadlessEngine::new_or_skip(demo.engine_builder())?;
  engine.step_frames(2).ok()?;
  let image = engine.frame().ok()?;
  let (content, line_h) = seen.lock().ok()?.take()?;
  let pad = content.x as u32;
  let places = Places {
    pad,
    line_h,
    description_mid_y: content.y as u32 - pad - line_h / 2,
    legend_mid_y: (content.y + content.height) as u32 + pad + line_h / 2,
  };
  Some((image, places))
}

#[test]
fn description_has_a_dark_backing_on_bright_pages() {
  let Some((image, at)) = bright_frame((320, 240)) else {
    return;
  };
  // Just inside the backing, which starts half a pad from the edge
  let x = at.pad / 2 + 1;
  assert!(
    red_at(&image, x, at.description_mid_y) < 120,
    "left padding of the description is backed"
  );
}

#[test]
fn legend_has_a_dark_backing_on_bright_pages() {
  let Some((image, at)) = bright_frame((320, 240)) else {
    return;
  };
  // The backing reaches half a pad past the line's right edge, a pad from the
  // screen edge
  let x = 320 - at.pad / 2 - 1;
  assert!(
    red_at(&image, x, at.legend_mid_y) < 120,
    "right padding of the legend is backed"
  );
  assert!(at.line_h > 0);
}

#[test]
fn a_description_too_wide_for_any_text_draws_nothing() {
  // So narrow that not even "..." fits
  let Some((image, at)) = bright_frame((20, 240)) else {
    return;
  };
  let x = at.pad / 2 + 1;
  assert!(
    red_at(&image, x, at.description_mid_y) > 200,
    "an empty description is not backed"
  );
}
