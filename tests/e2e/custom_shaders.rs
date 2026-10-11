//! Custom sprite shaders: game WGSL drawing entities and `render_gpu` sprites.
//!
//! Tests skip when no GPU is available.

use ryo2d::core::dev_log::{self, LogLevel};
use ryo2d::prelude::*;
use ryo2d::testing::HeadlessEngine;

const RED: [u8; 4] = [220, 30, 40, 255];
const GREEN: [u8; 4] = [30, 200, 60, 255];

fn solid(name: &str, size: u32, c: [u8; 4]) -> SpriteHandle {
  SpriteHandle::from_sprite(Sprite::solid_rect(size, size, c[0], c[1], c[2], c[3]), name)
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
    if let Some(setup) = self.setup.as_mut() {
      setup(ctx);
    }
  }

  fn update(&mut self, _ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    SceneAction::Continue
  }

  fn render_gpu(&mut self, gpu: &mut GpuRenderContext) {
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

#[test]
fn custom_shader_inverts_its_sprite() {
  let shader = ShaderHandle::from_source("t2-invert.wgsl", INVERT);
  let scene = Fixture::entities(move |ctx| {
    ctx.world.spawn((
      Transform::new(10.0, 20.0),
      Renderable::sprite(solid("t2-inv-red", 20, RED))
        .with_shader(SpriteShader::custom(shader.clone())),
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

#[test]
fn params_reach_the_shader() {
  let shader = ShaderHandle::from_source("t2-params.wgsl", PARAMS);
  let scene = Fixture::entities(move |ctx| {
    let sprite = solid("t2-params-red", 10, RED);
    let on = SpriteShader::custom(shader.clone())
      .with_param("a", 1.0)
      .with_param("color", Color::rgb(10, 200, 90));
    let off = SpriteShader::custom(shader.clone())
      .with_param("a", 0.0)
      .with_param("color", Color::rgb(10, 200, 90));
    ctx.world.spawn((
      Transform::new(5.0, 5.0),
      Renderable::sprite(sprite.clone()).with_shader(on),
    ));
    ctx.world.spawn((
      Transform::new(30.0, 5.0),
      Renderable::sprite(sprite).with_shader(off),
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
  let shader = ShaderHandle::from_source("t2-shift.wgsl", shifted);
  let scene = Fixture::entities(move |ctx| {
    ctx.world.spawn((
      Transform::new(10.0, 10.0),
      Renderable::sprite(solid("t2-shift-red", 8, RED))
        .with_shader(SpriteShader::custom(shader.clone())),
    ));
    // Right next to it, on the same atlas page
    ctx.world.spawn((
      Transform::new(18.0, 10.0),
      Renderable::sprite(solid("t2-shift-green", 8, GREEN)),
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
  let shader = ShaderHandle::from_source("t2-tint.wgsl", IDENTITY);
  let scene = Fixture::entities(move |ctx| {
    ctx.world.spawn((
      Transform::new(10.0, 10.0),
      Renderable::sprite(solid("t2-tint-grey", 10, [200, 200, 200, 255]))
        .with_tint(Color::rgb(130, 130, 130))
        .with_shader(SpriteShader::custom(shader.clone())),
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
  let invert = ShaderHandle::from_source("t2-mat-invert.wgsl", INVERT);
  let blue = "
fn sprite(in: SpriteIn, params: Params) -> vec4<f32> {
  return vec4<f32>(0.0, 0.0, 1.0, sample(in.uv).a);
}
";
  let blue = ShaderHandle::from_source("t2-mat-blue.wgsl", blue);
  let scene = Fixture::entities(move |ctx| {
    ctx.world.spawn((
      Transform::new(10.0, 10.0),
      Renderable::sprite(solid("t2-mat-red", 10, RED))
        .with_shader(SpriteShader::custom(invert.clone())),
      Material::custom(blue.clone()),
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
  let shader = ShaderHandle::from_source("t2-gpu-invert.wgsl", INVERT);
  let sprite = solid("t2-gpu-red", 4, RED);
  let scene = Fixture::gpu(move |gpu| {
    gpu.upload_sprite(&sprite);
    let instance = SpriteInstance::new().with_transform(30.0, 30.0, 0.0, 20.0, 20.0);
    gpu.add_custom_sprite(&sprite.id(), &shader, &ShaderParams::new(), instance, 0);
  });
  let Some(mut engine) = start(scene) else {
    return;
  };
  let frame = render(&mut engine);

  assert_eq!(frame.get_pixel(30, 30).0, [35, 225, 215, 255]);
}

#[test]
fn broken_sprite_shader_falls_back() {
  let id = "t2-broken-sprite.wgsl";
  let source = "fn sprite(in: SpriteIn, params: Params) -> vec4<f32> {
  return vec4<f32>(1.0, 0.0 0.0, 1.0);
}
";
  let shader = ShaderHandle::from_source(id, source);
  let scene = Fixture::entities(move |ctx| {
    ctx.world.spawn((
      Transform::new(10.0, 10.0),
      Renderable::sprite(solid("t2-broken-red", 10, RED))
        .with_shader(SpriteShader::custom(shader.clone())),
    ));
  });
  let Some(mut engine) = start(scene) else {
    return;
  };
  engine.step_frames(4).unwrap();
  let frame = engine.frame().unwrap();

  assert_eq!(frame.get_pixel(15, 15).0, RED, "drawn as a plain sprite");
  let errors: Vec<_> = dev_log::get_entries()
    .into_iter()
    .filter(|e| e.message.contains(id))
    .collect();
  assert_eq!(errors.len(), 1, "{errors:?}");
  assert_eq!(errors[0].level, LogLevel::Error);
  assert!(errors[0].message.contains(":2:"), "{}", errors[0].message);
}

#[test]
fn replaced_source_changes_the_next_frame() {
  let shader = ShaderHandle::from_source("t2-replace.wgsl", IDENTITY);
  let handle = shader.clone();
  let scene = Fixture::entities(move |ctx| {
    ctx.world.spawn((
      Transform::new(10.0, 10.0),
      Renderable::sprite(solid("t2-replace-red", 10, RED))
        .with_shader(SpriteShader::custom(shader.clone())),
    ));
  });
  let Some(mut engine) = start(scene) else {
    return;
  };
  assert_eq!(render(&mut engine).get_pixel(15, 15).0, RED);

  handle.replace_source(INVERT);

  assert_eq!(render(&mut engine).get_pixel(15, 15).0, [35, 225, 215, 255]);
}

#[test]
fn material_built_in_effect_wins_over_renderable_shader() {
  let identity = ShaderHandle::from_source("t2-mat-builtin-identity.wgsl", IDENTITY);
  let scene = Fixture::entities(move |ctx| {
    ctx.world.spawn((
      Transform::new(10.0, 10.0),
      Renderable::sprite(solid("t2-mat-builtin-red", 10, RED))
        .with_shader(SpriteShader::custom(identity.clone())),
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
  let shader = ShaderHandle::from_source("t2-recover.wgsl", "fn sprite( {");
  let handle = shader.clone();
  let scene = Fixture::entities(move |ctx| {
    ctx.world.spawn((
      Transform::new(10.0, 10.0),
      Renderable::sprite(solid("t2-recover-red", 10, RED))
        .with_shader(SpriteShader::custom(shader.clone())),
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

  handle.replace_source(INVERT);

  assert_eq!(render(&mut engine).get_pixel(15, 15).0, [35, 225, 215, 255]);
}
