//! The GPU UI, pixel-checked

use ryo2d::prelude::*;
use ryo2d::testing::HeadlessEngine;

/// Draws `draw` as its UI over black (or over `world`), with the clamp
/// curve so colours come through unchanged
struct Ui<F: FnMut(&mut UiContext) + 'static> {
  draw: F,
  world: Option<fn(&mut SceneContext)>,
}

impl<F: FnMut(&mut UiContext) + 'static> Scene for Ui<F> {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    ctx.look().tonemap = Tonemap::Clamp;
    let camera = ctx.camera().entity();
    if let Some(mut camera) = ctx.world.get_mut::<Camera2d>(camera) {
      camera.clear = CameraClear::Color(Color::BLACK);
    }
    if let Some(setup) = self.world {
      setup(ctx);
    }
  }
  fn update(&mut self, _ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    SceneAction::Continue
  }
  fn ui(&mut self, ui: &mut UiContext, _ctx: &mut SceneContext) -> SceneAction {
    (self.draw)(ui);
    SceneAction::Continue
  }
}

fn white_world(ctx: &mut SceneContext) {
  let white = SpriteHandle::from_sprite(Sprite::solid_rect(400, 400, 255, 255, 255, 255), "white");
  ctx
    .world
    .spawn((Transform::from_xy(0.0, 0.0), Renderable::sprite(white)));
}

fn engine(
  scale_factor: Option<f64>,
  builder: impl FnOnce(EngineBuilder) -> EngineBuilder,
  scene: impl Scene + 'static,
) -> Option<HeadlessEngine> {
  let base = builder(EngineBuilder::new().resolution(128, 96).start_scene(scene));
  let mut engine = match scale_factor {
    Some(f) => HeadlessEngine::new_or_skip_with_scale_factor(base, f)?,
    None => HeadlessEngine::new_or_skip(base)?,
  };
  engine.step_frames(2).unwrap();
  Some(engine)
}

fn frame(
  scale_factor: Option<f64>,
  builder: impl FnOnce(EngineBuilder) -> EngineBuilder,
  scene: impl Scene + 'static,
) -> Option<image::RgbaImage> {
  Some(engine(scale_factor, builder, scene)?.frame().unwrap())
}

#[test]
fn a_half_transparent_panel_has_its_specified_colour() {
  let scene = Ui {
    draw: |ui: &mut UiContext| {
      ui.draw_rect(Rect::new(0.0, 0.0, 64.0, 96.0), Color::new(0, 0, 0, 128))
    },
    world: Some(white_world),
  };
  let Some(frame) = frame(None, |b| b, scene) else {
    return;
  };
  let p = frame.get_pixel(32, 48).0;
  assert!(
    (i32::from(p[0]) - 127).abs() <= 2,
    "gamma-space half black over white: {p:?}"
  );
  assert_eq!(frame.get_pixel(96, 48).0, [255, 255, 255, 255]);
}

#[test]
fn faded_text_is_translucent() {
  let scene = Ui {
    draw: |ui: &mut UiContext| {
      ui.draw_text_styled(
        "HHHH",
        8.0,
        8.0,
        &TextStyle::new()
          .size(32.0)
          .color(Color::new(255, 255, 255, 128)),
      )
    },
    world: None,
  };
  let Some(frame) = frame(None, |b| b, scene) else {
    return;
  };
  let brightest = frame.pixels().map(|p| p.0[0]).max().unwrap();
  assert!(
    (120..=136).contains(&brightest),
    "half-opaque white over black: {brightest}"
  );
}

#[test]
fn text_at_twice_the_scale_factor_is_sharp() {
  let scene = Ui {
    draw: |ui: &mut UiContext| ui.draw_text_styled("Il|", 8.0, 8.0, &TextStyle::new().size(20.0)),
    world: None,
  };
  let Some(frame) = frame(Some(2.0), |b| b, scene) else {
    return;
  };
  let lit = frame.pixels().filter(|p| p.0[0] > 24).count();
  let solid = frame.pixels().filter(|p| p.0[0] > 230).count();
  assert!(lit > 50);
  assert!(
    solid * 10 >= lit * 5,
    "rendered at physical resolution: {solid} of {lit}"
  );
}

#[test]
fn pixel_art_ui_draws_on_the_target_or_the_window() {
  let draw = |ui: &mut UiContext| ui.draw_rect(Rect::new(10.3, 10.3, 5.0, 5.0), Color::WHITE);
  // An 80x45 target in a window four times its size
  let target = engine(Some(4.0), |b| b.pixel_art(80, 45), Ui { draw, world: None });
  let window = engine(
    Some(4.0),
    |b| b.pixel_art(80, 45).ui_resolution(UiResolution::Window),
    Ui { draw, world: None },
  );
  let (Some(target), Some(window)) = (target, window) else {
    return;
  };
  // frame() is the render-size composite: target-resolution UI has whole
  // target pixels
  let frame = target.frame().unwrap();
  assert_eq!(frame.dimensions(), (80, 45));
  let partial = frame
    .pixels()
    .filter(|p| p.0[0] > 10 && p.0[0] < 245)
    .count();
  assert_eq!(partial, 0, "snapped to the target's pixels");
  assert!(frame.pixels().any(|p| p.0[0] >= 245), "the rect is drawn");

  // Window resolution: the edge at 10.3 target pixels is 41.2 window
  // pixels, anti-aliased into window pixel 41
  let frame = window.window_frame().unwrap();
  assert_eq!(frame.dimensions(), (320, 180));
  let red = |x: u32| frame.get_pixel(x, 50).0[0];
  assert!(red(40) <= 10, "outside: {}", red(40));
  assert!(
    (10..245).contains(&red(41)),
    "a partial window pixel: {}",
    red(41)
  );
  assert!(red(42) >= 245, "inside: {}", red(42));
  assert!(
    (10..245).contains(&red(61)),
    "the far edge at 61.2: {}",
    red(61)
  );
  assert!(red(62) <= 10, "outside: {}", red(62));
}

#[test]
fn a_bad_ui_scale_still_draws() {
  let scene = Ui {
    draw: |ui: &mut UiContext| ui.draw_rect(Rect::new(10.0, 10.0, 20.0, 20.0), Color::WHITE),
    world: None,
  };
  let Some(frame) = frame(None, |b| b.ui_scale(Some(f32::NAN)), scene) else {
    return;
  };
  assert_eq!(frame.get_pixel(20, 20).0, [255, 255, 255, 255]);
}

#[test]
fn a_sub_pixel_rect_edge_is_anti_aliased_on_both_sides() {
  // x from 10.5 to 20.5: the edge pixels 10 and 20 are each half covered
  let scene = Ui {
    draw: |ui: &mut UiContext| ui.draw_rect(Rect::new(10.5, 10.0, 10.0, 20.0), Color::WHITE),
    world: None,
  };
  let Some(frame) = frame(None, |b| b, scene) else {
    return;
  };
  let red = |x: u32| frame.get_pixel(x, 20).0[0];
  for edge in [10, 20] {
    assert!(
      (64..=192).contains(&red(edge)),
      "pixel {edge} is partly covered: {}",
      red(edge)
    );
  }
  assert_eq!(red(15), 255);
  assert!(red(9) <= 2 && red(21) <= 2, "{} {}", red(9), red(21));
}

#[test]
fn clipped_ui_stays_inside_its_scissor() {
  let scene = Ui {
    draw: |ui: &mut UiContext| {
      ui.push_clip(Rect::new(20.0, 20.0, 30.0, 30.0));
      ui.draw_rect(Rect::new(0.0, 0.0, 128.0, 96.0), Color::WHITE);
      ui.draw_line(
        &[Vec2::new(0.0, 35.0), Vec2::new(128.0, 35.0)],
        4.0,
        Color::RED,
      );
      ui.pop_clip();
    },
    world: None,
  };
  let Some(frame) = frame(None, |b| b, scene) else {
    return;
  };
  assert_eq!(frame.get_pixel(30, 25).0, [255, 255, 255, 255]);
  assert_eq!(frame.get_pixel(10, 25).0, [0, 0, 0, 255]);
  assert_eq!(frame.get_pixel(60, 60).0, [0, 0, 0, 255]);
  // The line crosses the clip; outside it nothing shows
  assert_eq!(frame.get_pixel(30, 35).0, [255, 0, 0, 255]);
  assert_eq!(frame.get_pixel(80, 35).0, [0, 0, 0, 255]);
}

#[test]
fn later_ui_draws_over_earlier_ui() {
  let scene = Ui {
    draw: |ui: &mut UiContext| {
      ui.draw_text_styled("MMMM", 4.0, 4.0, &TextStyle::new().size(40.0));
      ui.draw_rect(Rect::new(0.0, 0.0, 128.0, 96.0), Color::BLUE);
      ui.draw_rect(Rect::new(40.0, 40.0, 20.0, 20.0), Color::GREEN);
    },
    world: None,
  };
  let Some(frame) = frame(None, |b| b, scene) else {
    return;
  };
  assert!(
    frame.pixels().all(|p| p.0[0] == 0),
    "the blue panel covers the text"
  );
  assert_eq!(frame.get_pixel(50, 50).0, [0, 255, 0, 255]);
}

#[test]
fn ui_units_and_mouse_hits_hold_at_scale_factor_two() {
  use std::cell::Cell;
  use std::rc::Rc;
  let clicks = Rc::new(Cell::new(0u32));
  let counted = clicks.clone();
  let scene = Ui {
    draw: move |ui: &mut UiContext| {
      ui.draw_rect(Rect::new(10.0, 10.0, 20.0, 20.0), Color::GREEN);
      if ui
        .button("Go")
        .at(Rect::new(80.0, 10.0, 30.0, 20.0))
        .clicked()
      {
        counted.set(counted.get() + 1);
      }
    },
    world: None,
  };
  let Some(mut engine) = engine(Some(2.0), |b| b, scene) else {
    return;
  };
  let frame = engine.frame().unwrap();
  assert_eq!(frame.dimensions(), (256, 192));
  // UI (10, 10, 20, 20) covers render pixels 20..60
  assert_eq!(frame.get_pixel(20, 20).0, [0, 255, 0, 255]);
  assert_eq!(frame.get_pixel(59, 59).0, [0, 255, 0, 255]);
  assert_ne!(frame.get_pixel(18, 40).0, [0, 255, 0, 255]);
  assert_ne!(frame.get_pixel(61, 40).0, [0, 255, 0, 255]);
  // Mouse positions are screen units, which are the UI's units
  let click = |engine: &mut HeadlessEngine, x: f32, y: f32| {
    engine.mouse_move(x, y);
    engine.press_mouse(MouseButton::Left);
    let _ = engine.step();
    engine.release_mouse(MouseButton::Left);
    let _ = engine.step();
  };
  click(&mut engine, 40.0, 20.0);
  assert_eq!(clicks.get(), 0, "beside the button");
  click(&mut engine, 95.0, 20.0);
  assert_eq!(clicks.get(), 1, "on the button");
}

#[test]
fn ui_text_far_larger_than_a_glyph_page_draws_and_warns_once() {
  use ryo2d::core::dev_log;
  // An 'I' 3000 px tall, centred: its stem covers the frame
  let scene = Ui {
    draw: |ui: &mut UiContext| {
      ui.text("I")
        .size(3000.0)
        .at(64.0, 48.0)
        .anchor(Anchor::Center)
        .draw();
    },
    world: None,
  };
  let Some(mut engine) = engine(None, |b| b, scene) else {
    return;
  };
  engine.step_frames(2).unwrap();
  let frame = engine.frame().unwrap();
  let lit = frame.pixels().filter(|p| p.0[0] > 200).count();
  assert!(
    lit > 128 * 96 * 9 / 10,
    "the huge glyph draws, scaled up: {lit} lit pixels"
  );
  let warnings = dev_log::get_entries()
    .iter()
    .filter(|e| e.message.contains("larger than 512 px") && e.message.contains("Inter"))
    .count();
  assert_eq!(warnings, 1, "warned once over four frames");
}

#[test]
fn a_centred_panel_draws_in_the_middle() {
  let scene = Ui {
    draw: |ui: &mut UiContext| {
      ui.panel()
        .width(40.0)
        .height(30.0)
        .anchor(Anchor::Center)
        .show(|_| {});
    },
    world: None,
  };
  let Some(frame) = frame(None, |b| b, scene) else {
    return;
  };
  // 128x96: the panel covers 44..84 x 33..63
  let centre = frame.get_pixel(64, 48).0;
  assert!(
    centre[0] > 0 || centre[1] > 0 || centre[2] > 0,
    "panel fill at the centre: {centre:?}"
  );
  assert_eq!(frame.get_pixel(10, 10).0, [0, 0, 0, 255], "black outside");
  assert_eq!(
    frame.get_pixel(40, 48).0,
    [0, 0, 0, 255],
    "left of the panel"
  );
}
