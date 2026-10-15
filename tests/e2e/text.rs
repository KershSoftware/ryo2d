//! World text and text rendering, pixel-checked

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use ryo2d::prelude::*;
use ryo2d::testing::HeadlessEngine;

/// A scene that spawns what `setup` asks and leaves it. The camera is centred
/// on the world origin and clears to black.
struct Spawn<F: FnMut(&mut SceneContext) + 'static>(F);

impl<F: FnMut(&mut SceneContext) + 'static> Scene for Spawn<F> {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    ctx.look().tonemap = Tonemap::Clamp;
    centre_on_black(ctx);
    (self.0)(ctx);
  }
  fn update(&mut self, _ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    SceneAction::Continue
  }
}

/// The main camera centred on the world origin, clearing to black
fn centre_on_black(ctx: &mut SceneContext) {
  let mut camera = ctx.camera();
  camera.set_position(0.0, 0.0);
  let entity = camera.entity();
  if let Some(mut camera) = ctx.world.get_mut::<Camera2d>(entity) {
    camera.clear = CameraClear::Color(Color::BLACK);
  }
}

/// The UI font drawn as MSDF
fn use_msdf(ctx: &mut SceneContext) {
  let mut fonts = ctx.fonts();
  let ui = fonts.role(FontRole::Ui).unwrap();
  fonts.set_render(ui, FontRender::msdf());
}

fn frame_of(
  size: (u32, u32),
  builder: impl FnOnce(EngineBuilder) -> EngineBuilder,
  setup: impl FnMut(&mut SceneContext) + 'static,
) -> Option<image::RgbaImage> {
  let base = EngineBuilder::new()
    .resolution(size.0, size.1)
    .start_scene(Spawn(setup));
  let mut engine = HeadlessEngine::new_or_skip(builder(base))?;
  engine.step_frames(3).unwrap();
  Some(engine.frame().unwrap())
}

fn bright(p: &image::Rgba<u8>) -> bool {
  p.0[0] > 200 && p.0[1] > 200 && p.0[2] > 200
}

fn count(frame: &image::RgbaImage, f: impl Fn(&image::Rgba<u8>) -> bool) -> usize {
  frame.pixels().filter(|p| f(p)).count()
}

/// The box (left, top, right, bottom) of the pixels `f` picks
fn ink(frame: &image::RgbaImage, f: impl Fn(&image::Rgba<u8>) -> bool) -> Option<[u32; 4]> {
  let mut bounds: Option<[u32; 4]> = None;
  for (x, y, p) in frame.enumerate_pixels() {
    if f(p) {
      let b = bounds.get_or_insert([x, y, x, y]);
      *b = [b[0].min(x), b[1].min(y), b[2].max(x), b[3].max(y)];
    }
  }
  bounds
}

#[test]
fn world_text_draws_where_its_entity_is() {
  let Some(frame) = frame_of(
    (96, 64),
    |b| b,
    |ctx| {
      ctx.camera().set_position(0.0, 0.0);
      ctx.world.spawn((
        Transform::from_xy(0.0, 0.0),
        Text2d::new("HH").with_style(TextStyle::new().size(24.0)),
      ));
    },
  ) else {
    return;
  };
  let (w, h) = frame.dimensions();
  let centre = (w / 2 - 12..w / 2 + 12).flat_map(|x| (h / 2 - 8..h / 2 + 8).map(move |y| (x, y)));
  assert!(
    centre
      .filter(|&(x, y)| bright(frame.get_pixel(x, y)))
      .count()
      > 20,
    "text at the centre"
  );
  assert!(!bright(frame.get_pixel(2, 2)), "nothing in the corner");
}

#[test]
fn a_missing_glyph_draws_a_box() {
  let Some(frame) = frame_of(
    (64, 64),
    |b| b,
    |ctx| {
      ctx.world.spawn((
        Transform::from_xy(0.0, 0.0),
        Text2d::new("\u{6F22}").with_style(TextStyle::new().size(32.0)),
      ));
    },
  ) else {
    return;
  };
  assert!(count(&frame, bright) > 10, "the tofu box is drawn");
}

#[test]
fn non_ascii_glyphs_draw() {
  let Some(frame) = frame_of(
    (96, 64),
    |b| b,
    |ctx| {
      ctx.world.spawn((
        Transform::from_xy(0.0, 0.0),
        Text2d::new("\u{03A9}\u{0436}\u{00E9}").with_style(TextStyle::new().size(28.0)),
      ));
    },
  ) else {
    return;
  };
  assert!(
    count(&frame, bright) > 30,
    "Greek, Cyrillic and Latin-1 glyphs draw"
  );
}

#[test]
fn zoomed_world_text_stays_sharp() {
  let Some(frame) = frame_of(
    (128, 96),
    |b| b,
    |ctx| {
      ctx.camera().set_zoom(4.0);
      ctx.world.spawn((
        Transform::from_xy(0.0, 0.0),
        Text2d::new("Il").with_style(TextStyle::new().size(10.0)),
      ));
    },
  ) else {
    return;
  };
  let lit = count(&frame, |p| p.0[0] > 24);
  let solid = count(&frame, |p| p.0[0] > 230);
  assert!(lit > 50);
  assert!(
    solid * 10 >= lit * 6,
    "mostly solid, not an upscaled blur: {solid} of {lit}"
  );
}

#[test]
fn an_msdf_outline_surrounds_the_fill() {
  let Some(frame) = frame_of(
    (96, 64),
    |b| b,
    |ctx| {
      use_msdf(ctx);
      let style = TextStyle::new()
        .size(40.0)
        .color(Color::rgb(255, 0, 0))
        .outline(1.5, Color::WHITE);
      ctx.world.spawn((
        Transform::from_xy(0.0, 0.0),
        Text2d::new("O").with_style(style),
      ));
    },
  ) else {
    return;
  };
  assert!(
    count(&frame, |p| p.0[0] > 200 && p.0[1] < 60) > 20,
    "red fill"
  );
  assert!(count(&frame, bright) > 20, "white outline");
}

#[test]
fn msdf_glow_reaches_past_the_glyph() {
  let glowing = |glow: bool| {
    frame_of(
      (96, 64),
      |b| b,
      move |ctx| {
        use_msdf(ctx);
        let mut style = TextStyle::new().size(40.0).color(Color::rgb(0, 0, 255));
        if glow {
          style = style.glow(1.5, Color::rgb(0, 255, 0));
        }
        ctx.world.spawn((
          Transform::from_xy(0.0, 0.0),
          Text2d::new("I").with_style(style),
        ));
      },
    )
  };
  let (Some(plain), Some(glow)) = (glowing(false), glowing(true)) else {
    return;
  };
  assert_eq!(count(&plain, |p| p.0[1] > 40), 0);
  assert!(
    count(&glow, |p| p.0[1] > 40) > 10,
    "green glow around the blue glyph"
  );
}

#[test]
fn a_turned_msdf_outline_stays_off_its_quad_edges() {
  // A wide outline on a turned glyph: past the reach of its field the
  // outline lane must fade out, or the quad's empty corners show as a faint
  // box in the outline colour
  let faint = |rotation: f32| {
    let frame = frame_of(
      (96, 96),
      |b| b,
      move |ctx| {
        use_msdf(ctx);
        let style = TextStyle::new()
          .size(20.0)
          .color(Color::WHITE)
          .outline(1.5, Color::rgb(255, 0, 0));
        ctx.world.spawn((
          Transform::from_xy(0.0, 0.0).with_rotation(rotation),
          Text2d::new("T").with_style(style),
        ));
      },
    )?;
    Some(count(&frame, |p| {
      (4..200).contains(&p.0[0]) && p.0[1] < 4 && p.0[2] < 4
    }))
  };
  let (Some(upright), Some(turned)) = (faint(0.0), faint(0.3)) else {
    return;
  };
  assert!(
    turned <= 2 * upright.max(10),
    "faint outline pixels: {upright} upright, {turned} turned"
  );
}

#[test]
fn msdf_and_raster_glyphs_land_in_the_same_place() {
  let drawn = |msdf: bool| {
    frame_of(
      (192, 128),
      |b| b,
      move |ctx| {
        if msdf {
          use_msdf(ctx);
        }
        // Past the hinting limit, so grid-fitting does not move raster edges
        let style = TextStyle::new().size(64.0);
        ctx.world.spawn((
          Transform::from_xy(0.0, 0.0),
          Text2d::new("Hog").with_style(style),
        ));
      },
    )
  };
  let (Some(raster), Some(msdf)) = (drawn(false), drawn(true)) else {
    return;
  };
  let inked = |p: &image::Rgba<u8>| p.0[0] >= 128;
  let (a, b) = (ink(&raster, inked).unwrap(), ink(&msdf, inked).unwrap());
  for (x, y) in a.iter().zip(&b) {
    assert!(x.abs_diff(*y) <= 1, "raster {a:?} vs msdf {b:?}");
  }
}

#[test]
fn a_pixel_font_lands_on_whole_pixels() {
  let Some(frame) = frame_of(
    (160, 90),
    |b| b.pixel_art(160, 90),
    |ctx| {
      let style = TextStyle::new()
        .role(FontRole::Pixel)
        .size(16.0)
        .color(Color::WHITE);
      ctx.world.spawn((
        Transform::from_xy(0.3, 0.6),
        Text2d::new("HELLO").with_style(style),
      ));
    },
  ) else {
    return;
  };
  let mid = count(&frame, |p| p.0[0] > 10 && p.0[0] < 245);
  assert_eq!(mid, 0, "every pixel is background or text colour");
  assert!(count(&frame, |p| p.0[0] >= 245) > 20);
}

#[test]
fn the_reveal_shows_text_over_time_and_says_so_once() {
  struct Reveal {
    entity: Option<Entity>,
    revealed: Arc<AtomicUsize>,
  }
  impl Scene for Reveal {
    fn on_enter(&mut self, ctx: &mut SceneContext) {
      ctx.look().tonemap = Tonemap::Clamp;
      centre_on_black(ctx);
      let entity = ctx
        .world
        .spawn((
          Transform::from_xy(0.0, 0.0),
          Text2d::new("HHHHHH").with_style(TextStyle::new().size(20.0)),
          TextReveal::new(6.0),
        ))
        .id();
      self.entity = Some(entity);
    }
    fn update(&mut self, ctx: &mut SceneContext, _dt: f32) -> SceneAction {
      let mine = ctx
        .messages::<TextRevealed>()
        .iter()
        .filter(|m| Some(m.entity) == self.entity)
        .count();
      self.revealed.fetch_add(mine, Ordering::Relaxed);
      SceneAction::Continue
    }
  }
  let revealed = Arc::new(AtomicUsize::new(0));
  let builder = EngineBuilder::new()
    .resolution(128, 64)
    .fixed_hz(60)
    .start_scene(Reveal {
      entity: None,
      revealed: Arc::clone(&revealed),
    });
  let Some(mut engine) = HeadlessEngine::new_or_skip(builder) else {
    return;
  };
  engine.step_frames(20).unwrap();
  let early = count(&engine.frame().unwrap(), bright);
  engine.step_frames(100).unwrap();
  let late = count(&engine.frame().unwrap(), bright);
  assert!(late > early * 2, "more shows later: {early} then {late}");
  assert_eq!(revealed.load(Ordering::Relaxed), 1, "said once");
}

#[test]
fn rotated_world_text_turns_with_its_entity() {
  let Some(frame) = frame_of(
    (64, 96),
    |b| b,
    |ctx| {
      ctx.world.spawn((
        Transform::from_xy(0.0, 0.0).with_rotation(std::f32::consts::FRAC_PI_2),
        Text2d::new("HHHH").with_style(TextStyle::new().size(16.0)),
      ));
    },
  ) else {
    return;
  };
  let [left, top, right, bottom] = ink(&frame, bright).unwrap();
  assert!(
    bottom - top > 2 * (right - left),
    "a tall column of text: {left},{top} to {right},{bottom}"
  );
}

#[test]
fn a_stretched_text_keeps_its_height() {
  let inked = |scale: Vec2| {
    let frame = frame_of(
      (128, 64),
      |b| b,
      move |ctx| {
        ctx.world.spawn((
          Transform::from_xy(0.0, 0.0).with_scale(scale),
          Text2d::new("HH").with_style(TextStyle::new().size(16.0)),
        ));
      },
    )?;
    ink(&frame, |p| p.0[0] > 128)
  };
  let (Some([l0, t0, r0, b0]), Some([l1, t1, r1, b1])) =
    (inked(Vec2::ONE), inked(Vec2::new(2.0, 1.0)))
  else {
    return;
  };
  let (w0, h0, w1, h1) = (r0 - l0, b0 - t0, r1 - l1, b1 - t1);
  assert!(h1.abs_diff(h0) <= 1, "as tall: {h0} then {h1}");
  assert!(w1.abs_diff(2 * w0) <= 3, "twice as wide: {w0} then {w1}");
}

#[test]
fn an_inline_icon_draws_its_sprite() {
  let Some(frame) = frame_of(
    (96, 64),
    |b| b,
    |ctx| {
      let red = SpriteHandle::from_sprite(Sprite::solid_rect(8, 8, 255, 0, 0, 255), "text-icon");
      let text = RichText::new("A ").icon(red);
      ctx.world.spawn((
        Transform::from_xy(0.0, 0.0),
        Text2d::new(text).with_style(TextStyle::new().size(20.0)),
      ));
    },
  ) else {
    return;
  };
  let red = count(&frame, |p| p.0[0] > 200 && p.0[1] < 40 && p.0[2] < 40);
  assert!(red > 200, "a 20 unit square of red: {red}");
  assert!(count(&frame, bright) > 20, "beside the A");
}

#[test]
fn a_bitmap_font_draws_its_cells() {
  let Some(frame) = frame_of(
    (96, 64),
    |b| b,
    |ctx| {
      // Two 4x4 cells: "#" solid, "." empty
      let mut pixels = vec![0u8; 8 * 4 * 4];
      for y in 0..4 {
        for x in 0..4 {
          pixels[(y * 8 + x) * 4..][..4].copy_from_slice(&[255, 255, 255, 255]);
        }
      }
      let sheet = SpriteHandle::from_sprite(Sprite::from_rgba(pixels, 8, 4), "text-grid");
      let font = ctx.fonts().add_bitmap("Grid", sheet, (4, 4), "#.").unwrap();
      let style = TextStyle::new().font(TextFont::Id(font)).size(16.0);
      ctx.world.spawn((
        Transform::from_xy(0.0, 0.0),
        Text2d::new("#.#").with_style(style),
      ));
    },
  ) else {
    return;
  };
  // Two solid 16x16 cells
  let lit = count(&frame, bright);
  assert!((480..=560).contains(&lit), "two cells of 256 pixels: {lit}");
}

#[test]
fn a_finished_fading_reveal_shows_its_last_glyph_fully() {
  // 30 clusters a second, each fading in over a tenth of a second: done
  // after 12 frames, the fade three clusters long
  let builder =
    EngineBuilder::new()
      .resolution(128, 64)
      .start_scene(Spawn(|ctx: &mut SceneContext| {
        ctx.world.spawn((
          Transform::from_xy(0.0, 0.0),
          Text2d::new("HHHHHH").with_style(TextStyle::new().size(20.0)),
          TextReveal::new(30.0).with_fade(0.1),
        ));
      }));
  let Some(mut engine) = HeadlessEngine::new_or_skip(builder) else {
    return;
  };
  engine.step_frames(60).unwrap();
  let frame = engine.frame().unwrap();
  let [left, _, right, _] = ink(&frame, |p| p.0[0] > 40).unwrap();
  let frame = &frame;
  let brightest = |xs: std::ops::RangeInclusive<u32>| {
    xs.flat_map(|x| (0..frame.height()).map(move |y| frame.get_pixel(x, y).0[0]))
      .max()
      .unwrap_or(0)
  };
  let (first, last) = (brightest(left..=left + 2), brightest(right - 2..=right));
  assert!(
    first >= 250 && last >= 250,
    "the first H's stem {first}, the last H's {last}"
  );
}

#[test]
fn a_text_entity_scaled_to_zero_draws_nothing() {
  let Some(frame) = frame_of(
    (64, 64),
    |b| b,
    |ctx| {
      ctx.world.spawn((
        Transform::from_xy(0.0, 0.0).with_scale(Vec2::ZERO),
        Text2d::new("HHH").with_style(TextStyle::new().size(24.0)),
      ));
    },
  ) else {
    return;
  };
  assert_eq!(count(&frame, bright), 0);
}
