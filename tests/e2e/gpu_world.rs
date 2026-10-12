//! The world draws on the GPU from one z-sorted list: entities, `render_gpu`
//! sprites, trails and lines, composite tiles, shadows, debug overlays and the
//! transition fade.
//!
//! Tests skip when no GPU is available.

use std::time::Duration;

use ryo2d::prelude::*;
use ryo2d::testing::HeadlessEngine;

const RED: [u8; 4] = [220, 30, 40, 255];
const GREEN: [u8; 4] = [30, 200, 60, 255];
const BLUE: [u8; 4] = [30, 60, 230, 255];
const MAGENTA: [u8; 4] = [255, 0, 255, 255];

fn solid(name: &str, size: u32, c: [u8; 4]) -> SpriteHandle {
  SpriteHandle::from_sprite(Sprite::solid_rect(size, size, c[0], c[1], c[2], c[3]), name)
}

/// Whether a pixel is the test line's green (lines are slightly translucent,
/// so compare by channel dominance).
fn is_line(pixel: [u8; 4]) -> bool {
  pixel[1] > 200 && pixel[0] < 100 && pixel[2] < 100
}

/// A scene that spawns its entities in `on_enter`, optionally uploads
/// `render_gpu` sprites, draws a UI rect and idles.
struct Fixture {
  setup: fn(&mut SceneContext),
  /// `(sprite, centre x, centre y, size, z)` drawn through `render_gpu`
  gpu_sprites: Vec<(SpriteHandle, f32, f32, f32, i32)>,
}

impl Fixture {
  fn new(setup: fn(&mut SceneContext)) -> Self {
    Self {
      setup,
      gpu_sprites: Vec::new(),
    }
  }

  fn with_gpu_sprite(mut self, sprite: SpriteHandle, x: f32, y: f32, size: f32, z: i32) -> Self {
    self.gpu_sprites.push((sprite, x, y, size, z));
    self
  }
}

impl Scene for Fixture {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    (self.setup)(ctx);
  }

  fn update(&mut self, _ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    SceneAction::Continue
  }

  fn render(&mut self, gpu: &mut GpuRenderContext) {
    for (sprite, x, y, size, z) in &self.gpu_sprites {
      gpu.upload_sprite(sprite);
      let instance = SpriteInstance::new().with_transform(*x, *y, 0.0, *size, *size);
      gpu.add_sprite_instance(&sprite.id(), instance, *z, false);
    }
  }

  fn ui(&mut self, ui: &mut UiContext, _ctx: &mut SceneContext) -> SceneAction {
    ui.draw_rect(Rect::new(4.0, 4.0, 10.0, 10.0), Color::rgb(30, 60, 230));
    SceneAction::Continue
  }
}

fn start(builder: EngineBuilder) -> Option<HeadlessEngine> {
  HeadlessEngine::new_or_skip(builder)
}

fn frame_of(scene: Fixture, size: (u32, u32)) -> Option<image::RgbaImage> {
  let builder = EngineBuilder::new()
    .resolution(size.0, size.1)
    .start_scene(scene);
  let mut engine = start(builder)?;
  engine.step_frames(2).unwrap();
  Some(engine.frame().unwrap())
}

fn spawn_entity(ctx: &mut SceneContext, name: &str, x: f32, y: f32, c: [u8; 4], z: i32) {
  ctx.world.spawn((
    Transform::from_xy(x, y),
    Renderable::sprite(solid(name, 20, c))
      .with_anchor(Anchor::TopLeft)
      .with_z_order(z),
  ));
}

// ---------------------------------------------------------------------------
// One z order for everything
// ---------------------------------------------------------------------------

#[test]
fn particle_between_entities() {
  // Entities: red z 0 (x 10..30), blue z 10 (x 30..50), both y 20..40.
  // A sprite at z 5 covers x 20..40: over red, under blue.
  let scene = Fixture::new(|ctx| {
    spawn_entity(ctx, "pb-red", 10.0, 20.0, RED, 0);
    spawn_entity(ctx, "pb-blue", 30.0, 20.0, BLUE, 10);
  })
  .with_gpu_sprite(solid("pb-green", 4, GREEN), 30.0, 30.0, 20.0, 5);
  let Some(frame) = frame_of(scene, (64, 64)) else {
    return;
  };
  let px = |x, y| frame.get_pixel(x, y).0;

  // Red only
  assert_eq!(px(14, 30), RED);
  // Red and the sprite: the sprite is in front
  assert_eq!(px(25, 30), GREEN, "sprite over the z-0 entity");
  // The sprite and blue: blue is in front
  assert_eq!(px(35, 30), BLUE, "z-10 entity over the sprite");
  // Blue only
  assert_eq!(px(46, 30), BLUE);
}

/// A horizontal green line across y 32 at `z`.
fn spawn_line(ctx: &mut SceneContext, z: i32) {
  let line = Line2D::horizontal(32.0, 0.0, 64.0, Color::new(0, 255, 0, 255), 4.0).with_z_order(z);
  ctx.world.spawn((Transform::from_xy(0.0, 0.0), line));
}

#[test]
fn trail_between_entities() {
  let scene = Fixture::new(|ctx| {
    spawn_entity(ctx, "tb-red", 10.0, 22.0, RED, 0);
    spawn_entity(ctx, "tb-blue", 30.0, 22.0, BLUE, 10);
    spawn_line(ctx, 5);
  });
  let Some(frame) = frame_of(scene, (64, 64)) else {
    return;
  };
  let px = |x, y| frame.get_pixel(x, y).0;

  assert!(
    is_line(px(20, 32)),
    "line over the z-0 entity: {:?}",
    px(20, 32)
  );
  assert_eq!(px(40, 32), BLUE, "line under the z-10 entity");
  assert!(is_line(px(58, 32)), "line over the background");
  // Away from the line the entities show
  assert_eq!(px(20, 26), RED);
}

#[test]
fn equal_z_order_entity_sprite_trail() {
  // All at z 0 and overlapping: entity, then sprite, then trail on top.
  let scene = Fixture::new(|ctx| {
    spawn_entity(ctx, "eq-red", 10.0, 22.0, RED, 0);
    spawn_line(ctx, 0);
  })
  .with_gpu_sprite(solid("eq-blue", 4, BLUE), 30.0, 32.0, 20.0, 0);
  let Some(frame) = frame_of(scene, (64, 64)) else {
    return;
  };
  let px = |x, y| frame.get_pixel(x, y).0;

  assert_eq!(px(14, 26), RED, "entity alone");
  assert_eq!(px(25, 26), BLUE, "sprite over the entity at equal z");
  assert!(is_line(px(14, 32)), "trail over the entity at equal z");
  assert!(is_line(px(30, 32)), "trail over the sprite at equal z");
}

#[test]
fn ysort_lower_draws_in_front() {
  // Two overlapping Y-sorted entities at one z. The one lower on screen
  // (larger y) is in front, whichever spawned first.
  let scene = Fixture::new(|ctx| {
    let sorted = |name: &str, y: f32, c: [u8; 4]| {
      (
        Transform::from_xy(20.0, y),
        Renderable::sprite(solid(name, 20, c)).with_anchor(Anchor::TopLeft),
        YSort::new(),
      )
    };
    // The lower one spawns first, so submission order would put it behind
    ctx.world.spawn(sorted("ys-low", 26.0, BLUE));
    ctx.world.spawn(sorted("ys-high", 20.0, RED));
  });
  let Some(frame) = frame_of(scene, (64, 64)) else {
    return;
  };
  // Overlap rows 26..40
  assert_eq!(frame.get_pixel(30, 33).0, BLUE, "lower entity in front");
  assert_eq!(frame.get_pixel(30, 22).0, RED);
  assert_eq!(frame.get_pixel(30, 44).0, BLUE);
}

#[test]
fn ysort_entities_stay_in_their_layer() {
  // A y-sorted entity at z 0 whose sort y is about 500 must still sit
  // between the z -1 and z 1 layers.
  let scene = Fixture::new(|ctx| {
    ctx.world.spawn((
      Transform::from_xy(20.0, 10.0),
      Renderable::sprite(solid("yl-mid", 20, RED)).with_anchor(Anchor::TopLeft),
      YSort {
        sort_offset: 500.0,
        ..YSort::new()
      },
    ));
  })
  .with_gpu_sprite(solid("yl-back", 4, GREEN), 20.0, 20.0, 20.0, -1)
  .with_gpu_sprite(solid("yl-front", 4, BLUE), 40.0, 20.0, 20.0, 1);
  let Some(frame) = frame_of(scene, (64, 64)) else {
    return;
  };
  // Sprites cover x 10..30 and 30..50; the entity covers x 20..40
  assert_eq!(
    frame.get_pixel(25, 20).0,
    RED,
    "entity over the z -1 sprite"
  );
  assert_eq!(
    frame.get_pixel(35, 20).0,
    BLUE,
    "z 1 sprite over the entity"
  );
}

#[test]
fn a_higher_layer_beats_a_far_down_ysort_entity() {
  let scene = Fixture::new(|ctx| {
    ctx.world.spawn((
      Transform::from_xy(20.0, 20.0),
      Renderable::sprite(solid("hl-ysort", 20, RED)).with_anchor(Anchor::TopLeft),
      YSort {
        sort_offset: 2000.0,
        ..YSort::new()
      },
    ));
    spawn_entity(ctx, "hl-top", 30.0, 20.0, BLUE, 1);
  });
  let Some(frame) = frame_of(scene, (64, 64)) else {
    return;
  };
  assert_eq!(
    frame.get_pixel(35, 30).0,
    BLUE,
    "z 1 over the z 0 YSort entity"
  );
  assert_eq!(frame.get_pixel(25, 30).0, RED);
}

#[test]
fn skeleton_debug_lines_draw_on_top() {
  let def = SkeletonDef::builder()
    .bone("root", None, Vec2::new(0.0, 32.0), 0.0, 10.0)
    .bone("tip", Some("root"), Vec2::new(64.0, 0.0), 0.0, 10.0)
    .build();
  let transforms = [
    BoneWorldTransform {
      position: Vec2::new(0.0, 32.0),
      rotation: 0.0,
    },
    BoneWorldTransform {
      position: Vec2::new(64.0, 32.0),
      rotation: 0.0,
    },
  ];
  let lines = skeleton_debug_lines(
    &def,
    &transforms,
    Color::new(0, 255, 0, 255),
    Color::new(0, 255, 0, 255),
    4.0,
    1.0,
  );
  assert!(!lines.is_empty());
  let scene = Fixture::new(|ctx| spawn_entity(ctx, "sk-cover", 20.0, 22.0, RED, 10));
  let Some(mut engine) = start(EngineBuilder::new().resolution(64, 64).start_scene(scene)) else {
    return;
  };
  for line in lines {
    engine
      .world_mut()
      .spawn((Transform::from_xy(0.0, 0.0), line));
  }
  engine.step_frames(2).unwrap();
  let frame = engine.frame().unwrap();
  assert!(
    is_line(frame.get_pixel(30, 32).0),
    "debug line over the z-10 sprite"
  );
}

#[test]
fn composite_tile_honours_z() {
  // Entity z 1 over a composite z 0 (overlap x 50..60); then the composite
  // above a z-(-1) entity.
  let scene = Fixture::new(|ctx| {
    spawn_entity(ctx, "ct-over", 40.0, 40.0, RED, 1);
    let tile = |name: &str| {
      CompositeTile::new().add_part(TilePart::static_part(solid(name, 20, GREEN), 0.0, 0.0))
    };
    ctx.world.spawn((
      Transform::from_xy(50.0, 40.0),
      tile("ct-under").with_z_order(0),
    ));
    spawn_entity(ctx, "ct-below", 40.0, 10.0, RED, -1);
    ctx.world.spawn((
      Transform::from_xy(50.0, 10.0),
      tile("ct-above").with_z_order(0),
    ));
  });
  let Some(frame) = frame_of(scene, (120, 90)) else {
    return;
  };
  let px = |x, y| frame.get_pixel(x, y).0;
  assert_eq!(
    px(55, 50),
    RED,
    "entity at the higher z covers the composite"
  );
  assert_eq!(px(65, 50), GREEN, "composite part beyond the entity");
  assert_eq!(
    px(55, 20),
    GREEN,
    "composite covers the entity at the lower z"
  );
  assert_eq!(px(45, 20), RED);
}

#[test]
fn trail_between_gpu_sprites() {
  let scene = Fixture::new(|ctx| spawn_line(ctx, 1))
    .with_gpu_sprite(solid("tg-blue", 4, BLUE), 36.0, 32.0, 20.0, 2)
    .with_gpu_sprite(solid("tg-red", 4, RED), 28.0, 32.0, 20.0, 0);
  let Some(frame) = frame_of(scene, (64, 64)) else {
    return;
  };
  let px = |x, y| frame.get_pixel(x, y).0;
  assert_eq!(px(22, 24), RED);
  assert!(is_line(px(22, 32)), "line over the z-0 sprite");
  assert_eq!(px(30, 32), BLUE, "line under the z-2 sprite");
  assert!(is_line(px(60, 32)), "line over the background");
}

// ---------------------------------------------------------------------------
// Entities, shadows, overlays and the fade
// ---------------------------------------------------------------------------

#[test]
fn blob_shadow_under_its_entity() {
  let scene = Fixture::new(|ctx| {
    let sprite =
      SpriteHandle::from_sprite(Sprite::solid_rect(10, 10, 220, 30, 40, 255), "blob-owner");
    let blob = BlobShadow {
      radius_x: 12.0,
      radius_y: 6.0,
      offset_y: 10.0,
      color: Color::new(0, 0, 0, 255),
      ..BlobShadow::default()
    };
    ctx.world.spawn((
      Transform::from_xy(60.0, 40.0),
      Renderable::sprite(sprite)
        .with_anchor(Anchor::TopLeft)
        .with_blob_shadow(blob),
    ));
  });
  let Some(frame) = frame_of(scene, (120, 90)) else {
    return;
  };
  let px = |x, y| frame.get_pixel(x, y).0;
  let background = px(110, 80);

  assert_eq!(px(65, 48), RED, "the sprite covers the shadow");
  assert_eq!(px(52, 50), [0, 0, 0, 255], "shadow beside the sprite");
  assert_eq!(px(110, 80), background);
  assert_ne!(background, [0, 0, 0, 255]);
}

#[test]
fn debug_colliders_draw() {
  let scene = Fixture::new(|ctx| {
    ctx.world.spawn((
      Transform::from_xy(50.0, 40.0),
      Collider::box_collider(30.0, 20.0),
    ));
  });
  let builder = EngineBuilder::new()
    .resolution(120, 90)
    .show_colliders(true)
    .start_scene(scene);
  let Some(mut engine) = start(builder) else {
    return;
  };
  engine.step_frames(2).unwrap();
  let frame = engine.frame().unwrap();
  let green = frame
    .pixels()
    .filter(|p| p.0[1] > 200 && p.0[0] < 80 && p.0[2] < 80)
    .count();
  // The outline of a 30x20 box is about 96 pixels
  assert!(green > 60, "collider outline pixels: {green}");
  // Centred on (50, 40): the top edge runs along y = 30
  assert_eq!(frame.get_pixel(60, 30).0, [0, 255, 0, 255]);
}

/// Draws a green unlit line and a UI rect, and pushes a fade through magenta.
struct FadesOut {
  started: bool,
}

/// The scene pushed over `FadesOut`; only the top scene draws UI.
struct Next;

impl Scene for Next {
  fn update(&mut self, _ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    SceneAction::Continue
  }

  fn ui(&mut self, ui: &mut UiContext, _ctx: &mut SceneContext) -> SceneAction {
    ui.draw_rect(Rect::new(4.0, 4.0, 10.0, 10.0), Color::rgb(30, 60, 230));
    SceneAction::Continue
  }
}

impl Scene for FadesOut {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    let line =
      Line2D::horizontal(30.0, 10.0, 100.0, Color::new(0, 255, 0, 255), 4.0).with_lighting(false);
    ctx.world.spawn((Transform::from_xy(0.0, 0.0), line));
  }

  fn update(&mut self, _ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    if self.started {
      return SceneAction::Continue;
    }
    self.started = true;
    // Push keeps the world, so the line stays under the fade
    SceneAction::PushWith(
      Box::new(Next),
      Transition::fade_color(Duration::from_millis(200), 255, 0, 255),
    )
  }

  fn ui(&mut self, ui: &mut UiContext, _ctx: &mut SceneContext) -> SceneAction {
    ui.draw_rect(Rect::new(4.0, 4.0, 10.0, 10.0), Color::rgb(30, 60, 230));
    SceneAction::Continue
  }
}

#[test]
fn transition_covers_trails_not_ui() {
  let builder = EngineBuilder::new()
    .resolution(120, 90)
    .start_scene(FadesOut { started: false });
  let Some(mut engine) = start(builder) else {
    return;
  };

  // Step until the fade-out has fully covered the frame
  let mut covered = None;
  for _ in 0..40 {
    engine.step().unwrap();
    let frame = engine.frame().unwrap();
    if frame.get_pixel(60, 30).0 == MAGENTA {
      covered = Some(frame);
      break;
    }
  }
  let frame = covered.expect("the fade never covered the line");
  assert_eq!(frame.get_pixel(8, 8).0, BLUE, "the UI draws above the fade");
  assert_eq!(frame.get_pixel(100, 70).0, MAGENTA);
}

#[test]
fn inactive_and_invisible_entities_do_not_draw() {
  let scene = Fixture::new(|ctx| {
    ctx.world.spawn((
      Transform::from_xy(10.0, 10.0),
      Renderable::sprite(solid("inactive", 10, RED)).with_anchor(Anchor::TopLeft),
      Disabled,
    ));
    ctx.world.spawn((
      Transform::from_xy(40.0, 10.0),
      Renderable::sprite(solid("hidden", 10, RED))
        .with_anchor(Anchor::TopLeft)
        .with_visible(false),
    ));
    ctx.world.spawn((
      Transform::from_xy(70.0, 10.0),
      Renderable::sprite(solid("shown", 10, RED)).with_anchor(Anchor::TopLeft),
    ));
  });
  let Some(frame) = frame_of(scene, (120, 90)) else {
    return;
  };
  assert_ne!(frame.get_pixel(15, 15).0, RED, "inactive entity drew");
  assert_ne!(frame.get_pixel(45, 15).0, RED, "invisible entity drew");
  assert_eq!(frame.get_pixel(75, 15).0, RED);
}

#[test]
fn lines_stay_in_place_after_a_stretch_resize() {
  let builder = EngineBuilder::new()
    .resolution(64, 64)
    .window_mode(WindowMode::ResizableStretch)
    .start_scene(Fixture::new(|ctx| spawn_line(ctx, 0)));
  let Some(mut engine) = HeadlessEngine::new_or_skip(builder) else {
    return;
  };
  engine.step_frames(1).unwrap();
  engine.resize(128, 128);
  engine.step_frames(2).unwrap();
  let frame = engine.frame().unwrap();
  assert_eq!(frame.dimensions(), (128, 128));
  let px = |x, y| frame.get_pixel(x, y).0;

  // The world keeps its scale: y 32 stays at y 32 and the line ends at x 64
  assert!(is_line(px(20, 32)), "line at y 32: {:?}", px(20, 32));
  assert!(!is_line(px(20, 64)), "line drawn at the old size's scale");
  assert!(!is_line(px(100, 32)), "line stretched past x 64");
}

/// A y-sorted entity at `(x, y)` in layer `z`
fn spawn_sorted(ctx: &mut SceneContext, name: &str, x: f32, y: f32, c: [u8; 4], z: i32) {
  ctx.world.spawn((
    Transform::from_xy(x, y),
    Renderable::sprite(solid(name, 20, c))
      .with_anchor(Anchor::TopLeft)
      .with_z_order(z),
    YSort::default(),
  ));
}

#[test]
fn plain_sprites_draw_under_y_sorted_ones_in_their_layer() {
  // In every layer, whatever the sign of z or y, a plain floor sits under
  // the y-sorted characters on it
  for z in [-3, 0, 4] {
    let frame = {
      let builder = EngineBuilder::new()
        .resolution(64, 64)
        .start_scene(Fixture::new(match z {
          -3 => |ctx: &mut SceneContext| {
            spawn_sorted(ctx, "ys-red-n", 10.0, 10.0, RED, -3);
            spawn_entity(ctx, "ys-floor-n", 0.0, 0.0, GREEN, -3);
          },
          0 => |ctx: &mut SceneContext| {
            spawn_sorted(ctx, "ys-red-0", 10.0, 10.0, RED, 0);
            spawn_entity(ctx, "ys-floor-0", 0.0, 0.0, GREEN, 0);
          },
          _ => |ctx: &mut SceneContext| {
            spawn_sorted(ctx, "ys-red-p", 10.0, 10.0, RED, 4);
            spawn_entity(ctx, "ys-floor-p", 0.0, 0.0, GREEN, 4);
          },
        }));
      let Some(mut engine) = start(builder) else {
        return;
      };
      engine.step_frames(2).unwrap();
      engine.frame().unwrap()
    };
    // The floor covers (0..20, 0..20); the character (10..30, 10..30)
    assert_eq!(
      frame.get_pixel(15, 15).0,
      RED,
      "z {z}: character over floor"
    );
    // (Clear of the fixture's UI rect)
    assert_eq!(frame.get_pixel(17, 5).0, GREEN, "z {z}: floor shows");
  }
}

/// Black left half, mid-grey (128) right half; pushes a fade through white
struct FadesToWhite {
  started: bool,
}

impl Scene for FadesToWhite {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    let halves = Sprite::from_rgba(
      (0..32 * 64)
        .flat_map(|i| {
          let v = if i % 64 < 32 { 0 } else { 128 };
          [v, v, v, 255]
        })
        .collect(),
      64,
      32,
    );
    ctx.world.spawn((
      Transform::from_xy(0.0, 0.0),
      Renderable::sprite(SpriteHandle::from_sprite(halves, "fade-halves"))
        .with_anchor(Anchor::TopLeft),
    ));
  }

  fn update(&mut self, _ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    if self.started {
      return SceneAction::Continue;
    }
    self.started = true;
    SceneAction::PushWith(
      Box::new(Next),
      Transition::fade_color(Duration::from_millis(300), 255, 255, 255),
    )
  }
}

#[test]
fn transition_fades_like_srgb_bytes() {
  let builder = EngineBuilder::new()
    .resolution(64, 32)
    .start_scene(FadesToWhite { started: false });
  let Some(mut engine) = start(builder) else {
    return;
  };
  // At fade opacity a, black becomes 255a and grey 128 + 127a, as the CPU
  // blended bytes; blending in linear light would hold grey back mid-fade
  let mut checked = 0;
  for _ in 0..40 {
    engine.step().unwrap();
    let frame = engine.frame().unwrap();
    let black = frame.get_pixel(20, 20).0[0] as f32;
    let grey = frame.get_pixel(50, 20).0[0] as f32;
    if (40.0..215.0).contains(&black) {
      let expected = 128.0 + black * 127.0 / 255.0;
      assert!(
        (grey - expected).abs() <= 3.0,
        "black {black}: grey {grey}, expected {expected}"
      );
      checked += 1;
    }
  }
  assert!(checked >= 2, "too few mid-fade frames ({checked})");
}
