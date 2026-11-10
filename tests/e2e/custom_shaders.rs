//! Custom sprite shaders: game WGSL drawing entities and `render_gpu` sprites.
//!
//! Tests skip when no GPU is available.

use std::sync::{Arc, Mutex};

use ryo2d::prelude::*;
use ryo2d::testing::HeadlessEngine;
use ryo2d::testing::LogCapture;

const RED: [u8; 4] = [220, 30, 40, 255];
const GREEN: [u8; 4] = [30, 200, 60, 255];

/// A `size` px square of `c`, added to the scene's world
fn solid(ctx: &mut SceneContext, size: u32, c: [u8; 4]) -> Handle<Image> {
  ctx
    .assets()
    .add(Image::solid_rect(size, size, c[0], c[1], c[2], c[3]))
}

/// The WGSL `source`, named `name`, added to the scene's world
fn shader(ctx: &mut SceneContext, name: &str, source: &str) -> Handle<Shader> {
  ctx.assets().add(Shader::from_wgsl(name, source))
}

/// A slot a scene puts a shader handle in, for the test to change it through
type ShaderSlot = Arc<Mutex<Option<Handle<Shader>>>>;

/// The handle in `slot`
fn held(slot: &ShaderSlot) -> Handle<Shader> {
  slot
    .lock()
    .unwrap()
    .clone()
    .expect("the scene added its shader")
}

const INVERT: &str = "
fn sprite(in: SpriteIn, params: Params) -> vec4<f32> {
  let c = sample(in.uv);
  return vec4<f32>(1.0 - c.rgb, c.a);
}
";

const IDENTITY: &str = "
fn sprite(in: SpriteIn, params: Params) -> vec4<f32> {
  return sample(in.uv);
}
";

type Setup = Box<dyn FnMut(&mut SceneContext) + Send>;
type GpuDraw = Box<dyn FnMut(&mut GpuRenderContext) + Send>;

/// A scene that runs `setup` once and `gpu` every frame.
struct Fixture {
  setup: Option<Setup>,
  gpu: Option<GpuDraw>,
}

impl Fixture {
  fn entities(setup: impl FnMut(&mut SceneContext) + Send + 'static) -> Self {
    Self {
      setup: Some(Box::new(setup)),
      gpu: None,
    }
  }

  fn gpu(draw: impl FnMut(&mut GpuRenderContext) + Send + 'static) -> Self {
    Self {
      setup: None,
      gpu: Some(Box::new(draw)),
    }
  }
}

impl Scene for Fixture {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    ctx.look().tonemap = Tonemap::Clamp;
    if let Some(setup) = self.setup.as_mut() {
      setup(ctx);
    }
  }

  fn update(&mut self, _ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    SceneAction::Continue
  }

  fn render(&mut self, gpu: &mut GpuRenderContext) {
    if let Some(draw) = self.gpu.as_mut() {
      draw(gpu);
    }
  }
}

fn start(scene: Fixture) -> Option<HeadlessEngine> {
  let builder = EngineBuilder::new().resolution(64, 64).start_scene(scene);
  HeadlessEngine::new_or_skip(builder)
}

fn render(engine: &mut HeadlessEngine) -> image::RgbaImage {
  engine.step_frames(2).unwrap();
  engine.frame().unwrap()
}

/// Steps until `done` holds of the last frame (five seconds at most),
/// calling `each` with every frame before it: a shader drawn already
/// compiles a new version on a thread of its own while the old one draws
fn step_until(
  engine: &mut HeadlessEngine,
  mut each: impl FnMut(&image::RgbaImage),
  done: impl Fn(&image::RgbaImage) -> bool,
) {
  let start = std::time::Instant::now();
  loop {
    engine.step().unwrap();
    let frame = engine.frame().unwrap();
    if done(&frame) {
      return;
    }
    each(&frame);
    assert!(
      start.elapsed() < std::time::Duration::from_secs(5),
      "it never came"
    );
    std::thread::sleep(std::time::Duration::from_millis(1));
  }
}

#[test]
fn custom_shader_inverts_its_sprite() {
  let scene = Fixture::entities(move |ctx| {
    let shader = shader(ctx, "t2-invert.wgsl", INVERT);
    let image = solid(ctx, 20, RED);
    ctx.world.spawn((
      Transform::from_xy(10.0, 20.0),
      Renderable::image(image)
        .with_anchor(Anchor::TopLeft)
        .with_shader(SpriteShader::custom(shader)),
    ));
  });
  let Some(mut engine) = start(scene) else {
    return;
  };
  let frame = render(&mut engine);
  let px = |x, y| frame.get_pixel(x, y).0;

  assert_eq!(px(20, 30), [35, 225, 215, 255]);
  let background = px(0, 0);
  assert_eq!(px(9, 30), background);
  assert_eq!(px(30, 30), background);
  assert_eq!(px(20, 19), background);
  assert_eq!(px(20, 40), background);
}

const PARAMS: &str = "
struct Params { a: f32, color: vec4<f32> }
fn sprite(in: SpriteIn, params: Params) -> vec4<f32> {
  let c = sample(in.uv);
  if (c.a > 0.0 && params.a > 0.5) {
    return params.color;
  }
  return c;
}
";

const CLOCK: &str = "
fn sprite(in: SpriteIn, params: Params) -> vec4<f32> {
  let c = sample(in.uv);
  if (in.time > 0.0) {
    return vec4<f32>(0.0, 0.0, 1.0, c.a);
  }
  return vec4<f32>(0.0, 1.0, 0.0, c.a);
}
";

#[test]
fn sprite_time_advances() {
  let scene = Fixture::entities(move |ctx| {
    let shader = shader(ctx, "t2-clock.wgsl", CLOCK);
    let image = solid(ctx, 10, RED);
    ctx.world.spawn((
      Transform::from_xy(10.0, 10.0),
      Renderable::image(image)
        .with_anchor(Anchor::TopLeft)
        .with_shader(SpriteShader::custom(shader)),
    ));
  });
  let Some(mut engine) = start(scene) else {
    return;
  };
  engine.step_frames(10).unwrap();
  let frame = engine.frame().unwrap();
  assert_eq!(
    frame.get_pixel(15, 15).0,
    [0, 0, 255, 255],
    "in.time is still 0"
  );
}

#[test]
fn params_reach_the_shader() {
  let scene = Fixture::entities(move |ctx| {
    let shader = shader(ctx, "t2-params.wgsl", PARAMS);
    let sprite = solid(ctx, 10, RED);
    let on = SpriteShader::custom(shader.clone())
      .with_param("a", 1.0)
      .with_param("color", Color::rgb(10, 200, 90));
    let off = SpriteShader::custom(shader)
      .with_param("a", 0.0)
      .with_param("color", Color::rgb(10, 200, 90));
    ctx.world.spawn((
      Transform::from_xy(5.0, 5.0),
      Renderable::image(sprite.clone())
        .with_anchor(Anchor::TopLeft)
        .with_shader(on),
    ));
    ctx.world.spawn((
      Transform::from_xy(30.0, 5.0),
      Renderable::image(sprite)
        .with_anchor(Anchor::TopLeft)
        .with_shader(off),
    ));
  });
  let Some(mut engine) = start(scene) else {
    return;
  };
  let frame = render(&mut engine);
  let px = |x, y| frame.get_pixel(x, y).0;

  assert_eq!(px(10, 10), [10, 200, 90, 255]);
  assert_eq!(px(35, 10), RED);
}

#[test]
fn custom_shader_samples_only_its_sprite() {
  let shifted = "
fn sprite(in: SpriteIn, params: Params) -> vec4<f32> {
  return sample(in.uv + vec2<f32>(0.2, 0.0));
}
";
  let scene = Fixture::entities(move |ctx| {
    let shader = shader(ctx, "t2-shift.wgsl", shifted);
    let image = solid(ctx, 8, RED);
    ctx.world.spawn((
      Transform::from_xy(10.0, 10.0),
      Renderable::image(image)
        .with_anchor(Anchor::TopLeft)
        .with_shader(SpriteShader::custom(shader)),
    ));
    // Right next to it, on the same atlas page
    let image = solid(ctx, 8, GREEN);
    ctx.world.spawn((
      Transform::from_xy(18.0, 10.0),
      Renderable::image(image).with_anchor(Anchor::TopLeft),
    ));
  });
  let Some(mut engine) = start(scene) else {
    return;
  };
  let frame = render(&mut engine);

  assert_eq!(frame.get_pixel(20, 14).0, GREEN, "the neighbour is drawn");
  for y in 10..18 {
    for x in 10..18 {
      assert_ne!(
        frame.get_pixel(x, y).0,
        GREEN,
        "neighbour leaked at {x},{y}"
      );
    }
  }
  assert_eq!(frame.get_pixel(11, 14).0, RED);
  // The columns the shift pushed past the sprite's edge are transparent
  let background = frame.get_pixel(0, 0).0;
  for y in 10..18 {
    for x in 16..18 {
      assert_eq!(frame.get_pixel(x, y).0, background, "edge column {x},{y}");
    }
  }
}

#[test]
fn tint_applies_after_the_custom_shader() {
  let scene = Fixture::entities(move |ctx| {
    let shader = shader(ctx, "t2-tint.wgsl", IDENTITY);
    let image = solid(ctx, 10, [200, 200, 200, 255]);
    ctx.world.spawn((
      Transform::from_xy(10.0, 10.0),
      Renderable::image(image)
        .with_anchor(Anchor::TopLeft)
        .with_tint(Color::rgb(130, 130, 130))
        .with_shader(SpriteShader::custom(shader)),
    ));
  });
  let Some(mut engine) = start(scene) else {
    return;
  };
  let frame = render(&mut engine);

  let expected = (200.0f32 * (130.0 / 255.0)).floor() as u8;
  assert_eq!(
    frame.get_pixel(15, 15).0,
    [expected, expected, expected, 255]
  );
}

#[test]
fn material_wins_over_renderable_shader() {
  let blue = "
fn sprite(in: SpriteIn, params: Params) -> vec4<f32> {
  return vec4<f32>(0.0, 0.0, 1.0, sample(in.uv).a);
}
";
  let scene = Fixture::entities(move |ctx| {
    let invert = shader(ctx, "t2-mat-invert.wgsl", INVERT);
    let blue = shader(ctx, "t2-mat-blue.wgsl", blue);
    let image = solid(ctx, 10, RED);
    ctx.world.spawn((
      Transform::from_xy(10.0, 10.0),
      Renderable::image(image)
        .with_anchor(Anchor::TopLeft)
        .with_shader(SpriteShader::custom(invert)),
      Material::custom(blue),
    ));
  });
  let Some(mut engine) = start(scene) else {
    return;
  };
  let frame = render(&mut engine);

  assert_eq!(frame.get_pixel(15, 15).0, [0, 0, 255, 255]);
}

#[test]
fn render_gpu_sprites_can_use_custom_shaders() {
  // Added on enter, drawn every frame
  type Drawn = Arc<Mutex<Option<(Handle<Image>, Handle<Shader>)>>>;
  let drawn: Drawn = Arc::default();
  let added = drawn.clone();
  let mut scene = Fixture::gpu(move |gpu| {
    let instance = SpriteInstance::new().with_transform(30.0, 30.0, 0.0, 20.0, 20.0);
    let draw = SpriteDraw::default();
    if let Some((sprite, shader)) = &*drawn.lock().unwrap() {
      gpu.add_custom_sprite(sprite, shader, &ShaderParams::new(), instance, draw);
    }
  });
  scene.setup = Some(Box::new(move |ctx| {
    let shader = shader(ctx, "t2-gpu-invert.wgsl", INVERT);
    *added.lock().unwrap() = Some((solid(ctx, 4, RED), shader));
  }));
  let Some(mut engine) = start(scene) else {
    return;
  };
  let frame = render(&mut engine);

  assert_eq!(frame.get_pixel(30, 30).0, [35, 225, 215, 255]);
}

#[test]
fn broken_sprite_shader_falls_back() {
  let log = ryo2d::testing::capture_log();
  let id = "t2-broken-sprite.wgsl";
  let source = "fn sprite(in: SpriteIn, params: Params) -> vec4<f32> {
  return vec4<f32>(1.0, 0.0 0.0, 1.0);
}
";
  let scene = Fixture::entities(move |ctx| {
    let shader = shader(ctx, id, source);
    let image = solid(ctx, 10, RED);
    ctx.world.spawn((
      Transform::from_xy(10.0, 10.0),
      Renderable::image(image)
        .with_anchor(Anchor::TopLeft)
        .with_shader(SpriteShader::custom(shader)),
    ));
  });
  let Some(mut engine) = start(scene) else {
    return;
  };
  engine.step_frames(4).unwrap();
  let frame = engine.frame().unwrap();

  assert_eq!(frame.get_pixel(15, 15).0, RED, "drawn as a plain sprite");
  assert_eq!(
    logged(&log, id),
    [(
      log::Level::Warn,
      Some("shaders".to_string()),
      format!("could not load \"{id}\": 2:29: expected `)`, found \"0.0\"")
    )],
    "warned once, as a file that does not load is"
  );
}

#[test]
fn a_replaced_source_draws_once_it_is_compiled() {
  let slot = ShaderSlot::default();
  let added = slot.clone();
  let scene = Fixture::entities(move |ctx| {
    let shader = shader(ctx, "t2-replace.wgsl", IDENTITY);
    *added.lock().unwrap() = Some(shader.clone());
    let image = solid(ctx, 10, RED);
    ctx.world.spawn((
      Transform::from_xy(10.0, 10.0),
      Renderable::image(image)
        .with_anchor(Anchor::TopLeft)
        .with_shader(SpriteShader::custom(shader)),
    ));
  });
  let Some(mut engine) = start(scene) else {
    return;
  };
  assert_eq!(render(&mut engine).get_pixel(15, 15).0, RED);

  let replaced = Shader::from_wgsl("t2-replace.wgsl", INVERT);
  *engine.assets().get_mut(&held(&slot)).unwrap() = replaced;

  // The old source draws until the new one's pipeline is made
  step_until(
    &mut engine,
    |frame| assert_eq!(frame.get_pixel(15, 15).0, RED, "the old source"),
    |frame| frame.get_pixel(15, 15).0 != RED,
  );
  assert_eq!(
    engine.frame().unwrap().get_pixel(15, 15).0,
    [35, 225, 215, 255]
  );
}

#[test]
fn material_built_in_effect_wins_over_renderable_shader() {
  let scene = Fixture::entities(move |ctx| {
    let identity = shader(ctx, "t2-mat-builtin-identity.wgsl", IDENTITY);
    let image = solid(ctx, 10, RED);
    ctx.world.spawn((
      Transform::from_xy(10.0, 10.0),
      Renderable::image(image)
        .with_anchor(Anchor::TopLeft)
        .with_shader(SpriteShader::custom(identity)),
      Material::invert(),
    ));
  });
  let Some(mut engine) = start(scene) else {
    return;
  };
  let frame = render(&mut engine);

  assert_eq!(frame.get_pixel(15, 15).0, [35, 225, 215, 255]);
}

#[test]
fn fixed_shader_recovers_after_an_error() {
  let slot = ShaderSlot::default();
  let added = slot.clone();
  let scene = Fixture::entities(move |ctx| {
    let shader = shader(ctx, "t2-recover.wgsl", "fn sprite( {");
    *added.lock().unwrap() = Some(shader.clone());
    let image = solid(ctx, 10, RED);
    ctx.world.spawn((
      Transform::from_xy(10.0, 10.0),
      Renderable::image(image)
        .with_anchor(Anchor::TopLeft)
        .with_shader(SpriteShader::custom(shader)),
    ));
  });
  let Some(mut engine) = start(scene) else {
    return;
  };
  assert_eq!(
    render(&mut engine).get_pixel(15, 15).0,
    RED,
    "plain while broken"
  );

  let fixed = Shader::from_wgsl("t2-recover.wgsl", INVERT);
  *engine.assets().get_mut(&held(&slot)).unwrap() = fixed;

  assert_eq!(render(&mut engine).get_pixel(15, 15).0, [35, 225, 215, 255]);
}

// ---------------------------------------------------------------------------
// Custom post-process effects
// ---------------------------------------------------------------------------

const GRAYSCALE: &str = "
struct Params { strength: f32 }
fn effect(in: EffectIn, params: Params) -> vec4<f32> {
  let c = scene(in.uv);
  let g = dot(c.rgb, vec3<f32>(0.299, 0.587, 0.114));
  return vec4<f32>(mix(c.rgb, vec3<f32>(g), params.strength), c.a);
}
";

const FLAT_GREY: &str = "
fn effect(in: EffectIn, params: Params) -> vec4<f32> {
  return vec4<f32>(scene(in.uv).rgb * 0.0 + 0.5, 1.0);
}
";

type EffectCommand = Box<dyn FnOnce(&mut SceneContext) + Send>;

/// A red screen whose effects are set up on enter and changed by queued commands.
struct EffectScene {
  setup: Option<EffectCommand>,
  queue: std::sync::Arc<std::sync::Mutex<Vec<EffectCommand>>>,
}

impl Scene for EffectScene {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    let image = solid(ctx, 64, RED);
    ctx.world.spawn((
      Transform::from_xy(0.0, 0.0),
      Renderable::image(image).with_anchor(Anchor::TopLeft),
    ));
    if let Some(setup) = self.setup.take() {
      setup(ctx);
    }
  }

  fn update(&mut self, ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    let pending: Vec<_> = self
      .queue
      .lock()
      .map(|mut q| q.drain(..).collect())
      .unwrap_or_default();
    for command in pending {
      command(ctx);
    }
    SceneAction::Continue
  }
}

type Queue = std::sync::Arc<std::sync::Mutex<Vec<EffectCommand>>>;

fn effect_engine(
  setup: impl FnOnce(&mut SceneContext) + Send + 'static,
) -> Option<(HeadlessEngine, Queue)> {
  let queue = Queue::default();
  let scene = EffectScene {
    setup: Some(Box::new(setup)),
    queue: queue.clone(),
  };
  let builder = EngineBuilder::new().resolution(64, 64).start_scene(scene);
  HeadlessEngine::new_or_skip(builder).map(|e| (e, queue))
}

fn send(queue: &Queue, command: impl FnOnce(&mut SceneContext) + Send + 'static) {
  if let Ok(mut q) = queue.lock() {
    q.push(Box::new(command));
  }
}

#[test]
fn custom_grayscale_effect() {
  let Some((mut engine, queue)) = effect_engine(|ctx| {
    let shader = shader(ctx, "t3-gray.wgsl", GRAYSCALE);
    let mut fx = ctx.effects();
    assert!(fx.add_custom("gray", shader).is_ok());
    assert!(fx.set_custom_param("gray", "strength", 0.0));
  }) else {
    return;
  };
  let off = render(&mut engine).get_pixel(10, 10).0;
  assert_eq!(off, RED, "strength 0 leaves the frame unchanged");

  send(&queue, |ctx| {
    assert!(ctx.effects().set_custom_param("gray", "strength", 1.0));
  });
  let on = render(&mut engine).get_pixel(10, 10).0;
  let (r, g, b) = (on[0] as i32, on[1] as i32, on[2] as i32);
  assert!((r - g).abs() <= 2 && (g - b).abs() <= 2, "{on:?}");
  assert_ne!(on, RED);
}

#[test]
fn custom_effect_runs_after_built_ins() {
  let Some((mut engine, _)) = effect_engine(|ctx| {
    let shader = shader(ctx, "t3-flat.wgsl", FLAT_GREY);
    let mut fx = ctx.effects();
    fx.set_vignette_custom(1.0, 0.2, 0.3);
    assert!(fx.add_custom("flat", shader).is_ok());
  }) else {
    return;
  };
  let frame = render(&mut engine);
  let centre = frame.get_pixel(32, 32).0;
  // Linear 0.5 encodes to sRGB 188
  assert!((187..=189).contains(&centre[0]), "{centre:?}");
  for (x, y, p) in frame.enumerate_pixels() {
    assert_eq!(p.0, centre, "pixel {x},{y}");
  }
}

#[test]
fn disabled_and_removed_effects_do_not_run() {
  let Some((mut engine, queue)) = effect_engine(|ctx| {
    let shader = shader(ctx, "t3-toggle.wgsl", FLAT_GREY);
    let mut fx = ctx.effects();
    assert!(fx.add_custom("flat", shader).is_ok());
    assert!(fx.has_custom("flat"));
  }) else {
    return;
  };
  assert_ne!(render(&mut engine).get_pixel(10, 10).0, RED, "runs");

  send(&queue, |ctx| {
    assert!(ctx.effects().set_custom_enabled("flat", false));
  });
  assert_eq!(render(&mut engine).get_pixel(10, 10).0, RED, "disabled");

  send(&queue, |ctx| {
    assert!(ctx.effects().set_custom_enabled("flat", true));
  });
  assert_ne!(render(&mut engine).get_pixel(10, 10).0, RED, "re-enabled");

  send(&queue, |ctx| {
    let mut fx = ctx.effects();
    assert!(fx.remove_custom("flat"));
    assert!(!fx.has_custom("flat"));
    assert!(!fx.remove_custom("flat"));
    assert!(!fx.set_custom_enabled("flat", true));
    assert!(!fx.set_custom_param("flat", "x", 1.0));
  });
  assert_eq!(render(&mut engine).get_pixel(10, 10).0, RED, "removed");
}

#[test]
fn duplicate_custom_effect_names_are_rejected() {
  let mut fx = EffectsManager::new();
  let mut registry = ryo2d::testing::asset_world();
  let mut shaders = registry.resource_mut::<Assets<Shader>>();
  let shader = shaders.add(Shader::from_wgsl("t3-dup.wgsl", FLAT_GREY));
  assert!(fx.add_custom("a", shader.clone()).is_ok());
  let err = fx.add_custom("a", shader).unwrap_err();
  assert!(err.to_string().contains("\"a\""), "{err}");
  assert!(fx.has_custom("a"));
}

#[test]
fn broken_effect_is_skipped() {
  let log = ryo2d::testing::capture_log();
  let id = "t3-broken-effect.wgsl";
  let source = "fn effect(in: EffectIn, params: Params) -> vec4<f32> {
  return vec4<f32>(1.0, 0.0 0.0, 1.0);
}
";
  let Some((mut engine, _)) = effect_engine(move |ctx| {
    let shader = shader(ctx, id, source);
    assert!(ctx.effects().add_custom("broken", shader).is_ok());
  }) else {
    return;
  };
  engine.step_frames(4).unwrap();
  let frame = engine.frame().unwrap();

  let Some((mut plain, _)) = effect_engine(|_| {}) else {
    return;
  };
  let expected = render(&mut plain);
  assert_eq!(
    frame.as_raw(),
    expected.as_raw(),
    "frame without the effect"
  );
  assert_eq!(
    logged(&log, id),
    [(
      log::Level::Warn,
      Some("shaders".to_string()),
      format!("could not load \"{id}\": 2:29: expected `)`, found \"0.0\"")
    )],
    "warned once, as a file that does not load is"
  );
}

/// Red top-left quadrant on green, with the WGSL `effect` added as a custom
/// effect
struct Quadrants {
  effect: &'static str,
}

impl Scene for Quadrants {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    let pixels = (0..64 * 64)
      .flat_map(|i| {
        let (x, y) = (i % 64, i / 64);
        if x < 32 && y < 32 { RED } else { GREEN }
      })
      .collect();
    let image = ctx.assets().add(Image::from_rgba(pixels, 64, 64));
    ctx.world.spawn((
      Transform::from_xy(0.0, 0.0),
      Renderable::image(image).with_anchor(Anchor::TopLeft),
    ));
    let shader = shader(ctx, "t3-quadrants.wgsl", self.effect);
    assert!(ctx.effects().add_custom("fx", shader).is_ok());
  }

  fn update(&mut self, _ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    SceneAction::Continue
  }
}

fn quadrants_with(effect: &'static str) -> Option<image::RgbaImage> {
  let builder = EngineBuilder::new()
    .resolution(64, 64)
    .start_scene(Quadrants { effect });
  let mut engine = HeadlessEngine::new_or_skip(builder)?;
  Some(render(&mut engine))
}

#[test]
fn effects_read_the_scene_the_right_way_up() {
  let Some(frame) =
    quadrants_with("fn effect(in: EffectIn, params: Params) -> vec4<f32> { return scene(in.uv); }")
  else {
    return;
  };
  assert_eq!(frame.get_pixel(8, 8).0, RED, "top left stays top left");
  assert_eq!(frame.get_pixel(56, 56).0, GREEN);
  assert_eq!(frame.get_pixel(56, 8).0, GREEN, "not mirrored");
  assert_eq!(frame.get_pixel(8, 56).0, GREEN, "not flipped");
}

#[test]
fn effect_uv_runs_from_the_top_left() {
  let Some(frame) = quadrants_with(
    "fn effect(in: EffectIn, params: Params) -> vec4<f32> { return vec4<f32>(in.uv, 0.0, 1.0); }",
  ) else {
    return;
  };
  // Corner pixels (uv of their centres, sRGB-encoded on output)
  let top_left = frame.get_pixel(0, 0).0;
  let bottom_right = frame.get_pixel(63, 63).0;
  let top_right = frame.get_pixel(63, 0).0;
  assert!(top_left[0] < 40 && top_left[1] < 40, "{top_left:?}");
  assert!(
    bottom_right[0] > 220 && bottom_right[1] > 220,
    "{bottom_right:?}"
  );
  assert!(
    top_right[0] > 220 && top_right[1] < 40,
    "x grows rightward, y downward: {top_right:?}"
  );
}

#[test]
fn a_custom_shader_applies_once_the_gpu_has_accepted_it() {
  let scene = Fixture::entities(move |ctx| {
    let shader = shader(ctx, "t3-pending.wgsl", INVERT);
    let image = solid(ctx, 20, RED);
    ctx.world.spawn((
      Transform::from_xy(10.0, 20.0),
      Renderable::image(image)
        .with_anchor(Anchor::TopLeft)
        .with_shader(SpriteShader::custom(shader)),
    ));
  });
  let Some(mut engine) = start(scene) else {
    return;
  };
  engine.step_frames(1).unwrap();
  assert_eq!(
    engine.frame().unwrap().get_pixel(20, 30).0,
    RED,
    "plain while the GPU check is pending"
  );
  engine.step_frames(1).unwrap();
  assert_eq!(
    engine.frame().unwrap().get_pixel(20, 30).0,
    [35, 225, 215, 255]
  );
}

/// A red square seen by a texture camera, shown by a sprite with the shader
/// `shader` makes.
fn red_through_a_target(
  shader: impl Fn(&mut SceneContext) -> SpriteShader + Send + 'static,
) -> Fixture {
  Fixture::entities(move |ctx| {
    let image = solid(ctx, 16, RED);
    ctx
      .world
      .spawn((Transform::from_xy(1000.0, 1000.0), Renderable::image(image)));
    let target = ctx.render_targets().create(16, 16);
    ctx.world.spawn((
      Transform::from_xy(1000.0, 1000.0),
      Camera2d {
        target: CameraTarget::Texture(target),
        ..Default::default()
      },
    ));
    let shader = shader(ctx);
    ctx.world.spawn((
      Transform::from_xy(32.0, 32.0),
      Renderable::render_target(target).with_shader(shader),
    ));
  })
}

fn assert_near(pixel: [u8; 4], want: [u8; 4]) {
  for (got, want) in pixel.iter().zip(want) {
    assert!(got.abs_diff(want) <= 1, "{pixel:?} vs {want:?}");
  }
}

#[test]
fn a_custom_shader_sees_a_render_target_as_straight_srgb() {
  let invert =
    |ctx: &mut SceneContext| SpriteShader::custom(shader(ctx, "t2-rt-invert.wgsl", INVERT));
  let Some(mut engine) = start(red_through_a_target(invert)) else {
    return;
  };
  assert_near(render(&mut engine).get_pixel(32, 32).0, [35, 225, 215, 255]);
}

#[test]
fn a_built_in_effect_sees_a_render_target_as_straight_srgb() {
  let invert = |_: &mut SceneContext| SpriteShader::BuiltIn(SpriteEffect::Invert);
  let Some(mut engine) = start(red_through_a_target(invert)) else {
    return;
  };
  assert_near(render(&mut engine).get_pixel(32, 32).0, [35, 225, 215, 255]);
}

// ---------------------------------------------------------------------------
// Shaders as registry assets: reloads, and pipelines freed with the shader
// ---------------------------------------------------------------------------

const INVERTED: [u8; 4] = [35, 225, 215, 255];

#[test]
fn a_reloaded_shader_recompiles() {
  let files = MemorySource::new();
  files.insert("shaders/look.wgsl", IDENTITY);
  let slot = ShaderSlot::default();
  let loaded = slot.clone();
  let scene = Fixture::entities(move |ctx| {
    let shader = ctx
      .assets()
      .load_now::<Shader>("shaders/look.wgsl")
      .expect("look.wgsl loads");
    *loaded.lock().unwrap() = Some(shader.clone());
    let image = solid(ctx, 10, RED);
    ctx.world.spawn((
      Transform::from_xy(10.0, 10.0),
      Renderable::image(image)
        .with_anchor(Anchor::TopLeft)
        .with_shader(SpriteShader::custom(shader)),
    ));
  });
  let builder = EngineBuilder::new()
    .resolution(64, 64)
    .asset_source(files.clone())
    .hot_reload(true)
    .start_scene(scene);
  let Some(mut engine) = HeadlessEngine::new_or_skip(builder) else {
    return;
  };
  assert_eq!(render(&mut engine).get_pixel(15, 15).0, RED, "the identity");
  // Over the file the shader came from: the source's watcher sees it, and
  // the file is read again on a worker
  files.insert("shaders/look.wgsl", INVERT);
  let shader = held(&slot);
  let start = std::time::Instant::now();
  while engine.assets().get(&shader).map(Shader::source) != Some(INVERT) {
    assert!(
      start.elapsed() < std::time::Duration::from_secs(2),
      "the reload never landed"
    );
    engine.step().unwrap();
    std::thread::sleep(std::time::Duration::from_millis(2));
  }
  // The identity draws until the new version's pipeline is made, on a
  // thread of its own; then the new one
  step_until(
    &mut engine,
    |frame| assert_eq!(frame.get_pixel(15, 15).0, RED, "the identity meanwhile"),
    |frame| frame.get_pixel(15, 15).0 != RED,
  );
  assert_eq!(
    engine.frame().unwrap().get_pixel(15, 15).0,
    INVERTED,
    "drawn with the reloaded source"
  );
}

/// Draws a sprite with a shader only the sprite holds, until `drop` is
/// set; then despawns it, and the shader's last handle goes with it
struct DropsItsShader {
  sprite: Option<Entity>,
  drop: Arc<std::sync::atomic::AtomicBool>,
}

impl Scene for DropsItsShader {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    ctx.look().tonemap = Tonemap::Clamp;
    let shader = ctx
      .assets()
      .add(Shader::from_wgsl("t9-unheld.wgsl", INVERT));
    let image = solid(ctx, 10, RED);
    let sprite = ctx.world.spawn((
      Transform::from_xy(10.0, 10.0),
      Renderable::image(image)
        .with_anchor(Anchor::TopLeft)
        .with_shader(SpriteShader::custom(shader)),
    ));
    self.sprite = Some(sprite.id());
  }

  fn update(&mut self, ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    if self.drop.load(std::sync::atomic::Ordering::SeqCst)
      && let Some(sprite) = self.sprite.take()
    {
      ctx.world.despawn(sprite);
    }
    SceneAction::Continue
  }
}

#[test]
fn an_unheld_shader_drops_its_pipeline() {
  let drop = Arc::new(std::sync::atomic::AtomicBool::new(false));
  let scene = DropsItsShader {
    sprite: None,
    drop: drop.clone(),
  };
  let builder = EngineBuilder::new().resolution(64, 64).start_scene(scene);
  let Some(mut engine) = HeadlessEngine::new_or_skip(builder) else {
    return;
  };
  engine.step_frames(3).unwrap();
  assert_eq!(engine.frame().unwrap().get_pixel(15, 15).0, INVERTED);
  assert_eq!(engine.shader_pipeline_count(), 1, "one blend mode drawn");
  drop.store(true, std::sync::atomic::Ordering::SeqCst);
  engine.step_frames(2).unwrap();
  assert_eq!(
    engine.shader_pipeline_count(),
    0,
    "the pipeline went with the shader"
  );
}

#[test]
fn a_removed_effect_drops_its_pipeline() {
  let Some((mut engine, queue)) = effect_engine(|ctx| {
    let shader = ctx
      .assets()
      .add(Shader::from_wgsl("t9-removed-effect.wgsl", FLAT_GREY));
    assert!(ctx.effects().add_custom("flat", shader).is_ok());
  }) else {
    return;
  };
  assert_ne!(render(&mut engine).get_pixel(10, 10).0, RED, "runs");
  assert_eq!(engine.shader_pipeline_count(), 1);
  // The effect held the shader's only handle
  send(&queue, |ctx| {
    assert!(ctx.effects().remove_custom("flat"));
  });
  engine.step_frames(2).unwrap();
  assert_eq!(engine.shader_pipeline_count(), 0);
}

// ---------------------------------------------------------------------------
// A shader that does not compile keeps the last good one
// ---------------------------------------------------------------------------

/// A sprite shader with a comma missing on its second line
const BROKEN_SPRITE: &str = "fn sprite(in: SpriteIn, params: Params) -> vec4<f32> {
  return vec4<f32>(1.0, 0.0 0.0, 1.0);
}
";

/// An effect with a comma missing on its second line
const BROKEN_EFFECT: &str = "fn effect(in: EffectIn, params: Params) -> vec4<f32> {
  return vec4<f32>(1.0, 0.0 0.0, 1.0);
}
";

/// The records `log` captured naming `name`: their level, engine module
/// (`shaders` for `ryo2d::core::shaders::…`) and message
fn logged(log: &LogCapture, name: &str) -> Vec<(log::Level, Option<String>, String)> {
  log
    .records()
    .into_iter()
    .filter(|record| record.message.contains(name))
    .map(|record| {
      let module = record
        .target
        .strip_prefix("ryo2d::core::")
        .and_then(|rest| rest.split("::").next())
        .map(str::to_string);
      (record.level, module, record.message)
    })
    .collect()
}

/// Saves `text` over `path` and steps until a shader's reload fails (two
/// seconds at most); the shader events sent meanwhile
fn save_until_it_fails(
  engine: &mut HeadlessEngine,
  files: &MemorySource,
  path: &str,
  text: &str,
) -> Vec<String> {
  let mut events = engine
    .world()
    .resource::<Messages<AssetEvent<Shader>>>()
    .get_cursor_current();
  files.insert(path, text);
  let mut sent = Vec::new();
  let start = std::time::Instant::now();
  while !sent.iter().any(|event: &String| event == "ReloadFailed") {
    assert!(
      start.elapsed() < std::time::Duration::from_secs(2),
      "the reload never failed: {sent:?}"
    );
    engine.step().unwrap();
    let messages = engine.world().resource::<Messages<AssetEvent<Shader>>>();
    sent.extend(events.read(messages).map(|event| match event {
      AssetEvent::Modified { .. } => "Modified".to_string(),
      AssetEvent::ReloadFailed { .. } => "ReloadFailed".to_string(),
      other => format!("{other:?}"),
    }));
    std::thread::sleep(std::time::Duration::from_millis(2));
  }
  sent
}

#[test]
fn a_broken_shader_save_keeps_the_last_good_shader() {
  let log = ryo2d::testing::capture_log();
  let path = "shaders/t33-saved.wgsl";
  let files = MemorySource::new();
  files.insert(path, INVERT);
  let scene = Fixture::entities(move |ctx| {
    let shader = ctx.assets().load_now::<Shader>(path).expect("it loads");
    let image = solid(ctx, 10, RED);
    ctx.world.spawn((
      Transform::from_xy(10.0, 10.0),
      Renderable::image(image)
        .with_anchor(Anchor::TopLeft)
        .with_shader(SpriteShader::custom(shader)),
    ));
  });
  let builder = EngineBuilder::new()
    .resolution(64, 64)
    .asset_source(files.clone())
    .hot_reload(true)
    .start_scene(scene);
  let Some(mut engine) = HeadlessEngine::new_or_skip(builder) else {
    return;
  };
  assert_eq!(render(&mut engine).get_pixel(15, 15).0, INVERTED);
  let sent = save_until_it_fails(&mut engine, &files, path, BROKEN_SPRITE);
  assert_eq!(sent, ["ReloadFailed"], "not Modified");
  for _ in 0..3 {
    engine.step().unwrap();
    assert_eq!(
      engine.frame().unwrap().get_pixel(15, 15).0,
      INVERTED,
      "the last good shader draws"
    );
  }
  assert_eq!(
    logged(&log, path),
    [(
      log::Level::Warn,
      Some("asset".to_string()),
      format!("could not load \"{path}\": 2:29: expected `)`, found \"0.0\"")
    )]
  );
}

/// A red screen running the effect loaded from `path` in `files`, watched
fn watched_effect(files: &MemorySource, path: &'static str) -> Option<HeadlessEngine> {
  let scene = EffectScene {
    setup: Some(Box::new(move |ctx: &mut SceneContext| {
      let shader = ctx.assets().load_now::<Shader>(path).expect("it loads");
      assert!(ctx.effects().add_custom("fx", shader).is_ok());
    })),
    queue: Queue::default(),
  };
  let builder = EngineBuilder::new()
    .resolution(64, 64)
    .asset_source(files.clone())
    .hot_reload(true)
    .start_scene(scene);
  HeadlessEngine::new_or_skip(builder)
}

/// Whether the effect's flat grey covers `frame` (linear 0.5 is sRGB 188)
fn grey(frame: &image::RgbaImage) -> bool {
  frame
    .pixels()
    .all(|p| (187..=189).contains(&p.0[0]) && p.0[0] == p.0[1] && p.0[1] == p.0[2])
}

#[test]
fn a_broken_effect_save_keeps_the_last_good_effect() {
  let log = ryo2d::testing::capture_log();
  let path = "shaders/t33-effect.wgsl";
  let files = MemorySource::new();
  files.insert(path, FLAT_GREY);
  let Some(mut engine) = watched_effect(&files, path) else {
    return;
  };
  assert!(grey(&render(&mut engine)), "the effect runs");
  let sent = save_until_it_fails(&mut engine, &files, path, BROKEN_EFFECT);
  assert_eq!(sent, ["ReloadFailed"], "not Modified");
  for _ in 0..3 {
    engine.step().unwrap();
    assert!(grey(&engine.frame().unwrap()), "the last good effect runs");
  }
  assert_eq!(
    logged(&log, path),
    [(
      log::Level::Warn,
      Some("asset".to_string()),
      format!("could not load \"{path}\": 2:29: expected `)`, found \"0.0\"")
    )]
  );
}

/// A sprite shader naga accepts that the sprite pipeline cannot take: its
/// binding is in no group the pipeline has
const REJECTED_SPRITE: &str = "@group(3) @binding(0) var<uniform> extra: vec4<f32>;
fn sprite(in: SpriteIn, params: Params) -> vec4<f32> {
  return sample(in.uv) * extra;
}
";

/// An effect naga accepts that the effect pipeline cannot take
const REJECTED_EFFECT: &str = "@group(1) @binding(0) var<uniform> extra: vec4<f32>;
fn effect(in: EffectIn, params: Params) -> vec4<f32> {
  return scene(in.uv) * extra;
}
";

/// An engine drawing a red sprite with the shader `source` named `name`,
/// and the slot holding the shader's handle
fn shaded(name: &'static str, source: &'static str) -> Option<(HeadlessEngine, ShaderSlot)> {
  let slot = ShaderSlot::default();
  let added = slot.clone();
  let scene = Fixture::entities(move |ctx| {
    let shader = shader(ctx, name, source);
    *added.lock().unwrap() = Some(shader.clone());
    let image = solid(ctx, 10, RED);
    ctx.world.spawn((
      Transform::from_xy(10.0, 10.0),
      Renderable::image(image)
        .with_anchor(Anchor::TopLeft)
        .with_shader(SpriteShader::custom(shader)),
    ));
  });
  Some((start(scene)?, slot))
}

#[test]
fn a_shader_changed_in_code_that_does_not_compile_draws_its_last_good_version() {
  let log = ryo2d::testing::capture_log();
  let name = "t33-changed.wgsl";
  let Some((mut engine, slot)) = shaded(name, INVERT) else {
    return;
  };
  assert_eq!(render(&mut engine).get_pixel(15, 15).0, INVERTED);
  let broken = Shader::from_wgsl(name, BROKEN_SPRITE);
  *engine.assets().get_mut(&held(&slot)).unwrap() = broken;
  // Compiled on the shader thread, it fails there
  step_until(
    &mut engine,
    |frame| {
      assert_eq!(
        frame.get_pixel(15, 15).0,
        INVERTED,
        "its last good version draws"
      )
    },
    |_| !logged(&log, name).is_empty(),
  );
  for _ in 0..3 {
    engine.step().unwrap();
    assert_eq!(
      engine.frame().unwrap().get_pixel(15, 15).0,
      INVERTED,
      "its last good version draws"
    );
  }
  assert_eq!(
    logged(&log, name),
    [(
      log::Level::Warn,
      Some("shaders".to_string()),
      format!("could not load \"{name}\": 2:29: expected `)`, found \"0.0\"")
    )],
    "once for the version"
  );
}

#[test]
fn a_shader_the_gpu_rejects_draws_its_last_good_version() {
  let log = ryo2d::testing::capture_log();
  let name = "t33-rejected.wgsl";
  let Some((mut engine, slot)) = shaded(name, INVERT) else {
    return;
  };
  assert_eq!(render(&mut engine).get_pixel(15, 15).0, INVERTED);
  let rejected = Shader::from_wgsl(name, REJECTED_SPRITE);
  *engine.assets().get_mut(&held(&slot)).unwrap() = rejected;
  // Its pipeline is made and checked on the shader thread, which comes back
  step_until(
    &mut engine,
    |frame| {
      assert_eq!(
        frame.get_pixel(15, 15).0,
        INVERTED,
        "its last good version draws"
      )
    },
    |_| !logged(&log, name).is_empty(),
  );
  for _ in 0..3 {
    engine.step().unwrap();
    assert_eq!(
      engine.frame().unwrap().get_pixel(15, 15).0,
      INVERTED,
      "its last good version draws"
    );
  }
  let logged = logged(&log, name);
  assert!(
    matches!(
      logged.as_slice(),
      [(log::Level::Warn, Some(module), message)]
        if module == "shaders" && message.starts_with(&format!("could not load \"{name}\": "))
    ),
    "once for the version: {logged:?}"
  );
}

#[test]
fn an_effect_the_gpu_rejects_runs_its_last_good_version() {
  let log = ryo2d::testing::capture_log();
  let name = "t33-rejected-effect.wgsl";
  let slot = ShaderSlot::default();
  let added = slot.clone();
  let Some((mut engine, _)) = effect_engine(move |ctx| {
    let shader = shader(ctx, name, FLAT_GREY);
    *added.lock().unwrap() = Some(shader.clone());
    assert!(ctx.effects().add_custom("fx", shader).is_ok());
  }) else {
    return;
  };
  assert!(grey(&render(&mut engine)), "the effect runs");
  let rejected = Shader::from_wgsl(name, REJECTED_EFFECT);
  *engine.assets().get_mut(&held(&slot)).unwrap() = rejected;
  step_until(
    &mut engine,
    |frame| assert!(grey(frame), "its last good version runs"),
    |_| !logged(&log, name).is_empty(),
  );
  for _ in 0..3 {
    engine.step().unwrap();
    assert!(grey(&engine.frame().unwrap()), "its last good version runs");
  }
  let logged = logged(&log, name);
  assert!(
    matches!(
      logged.as_slice(),
      [(log::Level::Warn, Some(module), message)]
        if module == "shaders" && message.starts_with(&format!("could not load \"{name}\": "))
    ),
    "once for the version: {logged:?}"
  );
}
