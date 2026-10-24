//! Game shaders changed in a running engine: a shader with a pipeline the
//! GPU accepted compiles its new version on a thread of its own while the
//! old one draws, and the new one takes over at the start of a frame; a
//! shader with none compiles at once, as it always did.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use super::HeadlessEngine;
use crate::core::asset::AssetsView;
use crate::core::dev_log;
use crate::core::engine::EngineBuilder;
use crate::prelude::*;

/// The sprites' own colour: what a sprite drawn plain shows
const PLAIN: [u8; 4] = [220, 30, 40, 255];
const GREEN: [u8; 4] = [0, 255, 0, 255];
const BLUE: [u8; 4] = [0, 0, 255, 255];
const WHITE: [u8; 4] = [255, 255, 255, 255];
const YELLOW: [u8; 4] = [255, 255, 0, 255];

/// How long a test waits for a compile on the shader thread
const PATIENCE: Duration = Duration::from_secs(5);

/// `colour` as WGSL floats
fn floats(colour: [u8; 4]) -> String {
  let [r, g, b, _] = colour.map(|c| f32::from(c) / 255.0);
  format!("{r:?}, {g:?}, {b:?}")
}

/// A sprite shader filling its sprite with `colour`
fn fill(colour: [u8; 4]) -> String {
  format!(
    "fn sprite(in: SpriteIn, params: Params) -> vec4<f32> {{\n  return vec4<f32>({}, sample(in.uv).a);\n}}\n",
    floats(colour)
  )
}

/// An effect covering the screen with `colour`
fn cover(colour: [u8; 4]) -> String {
  format!(
    "fn effect(in: EffectIn, params: Params) -> vec4<f32> {{\n  return vec4<f32>({}, 1.0);\n}}\n",
    floats(colour)
  )
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

type Setup = Box<dyn FnOnce(&mut SceneContext) + Send>;

/// Runs its setup on enter, with colours exact (no tonemap curve)
struct Staged(Option<Setup>);

impl Scene for Staged {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    ctx.look().tonemap = Tonemap::Clamp;
    if let Some(setup) = self.0.take() {
      setup(ctx);
    }
  }
}

/// A 64 x 32 headless engine running `setup`, reading `files` with hot
/// reload on if there are any
fn start(
  files: Option<MemorySource>,
  setup: impl FnOnce(&mut SceneContext) + Send + 'static,
) -> Option<HeadlessEngine> {
  let mut builder = EngineBuilder::new()
    .resolution(64, 32)
    .start_scene(Staged(Some(Box::new(setup))));
  if let Some(files) = files {
    builder = builder.asset_source(files).hot_reload(true);
  }
  HeadlessEngine::new_or_skip(builder)
}

/// A place the scene puts a handle in, for the test to change or free it
type Slot<T> = Arc<Mutex<Option<T>>>;

/// What the scene put in `slot`
fn taken<T: Clone>(slot: &Slot<T>) -> T {
  slot
    .lock()
    .unwrap_or_else(std::sync::PoisonError::into_inner)
    .clone()
    .expect("the scene filled its slot")
}

/// An 8 px square of the plain colour with its top-left at (x, 8), drawn
/// with `shader` and `blend`
fn spawn_shaded(
  ctx: &mut SceneContext,
  shader: Handle<Shader>,
  x: f32,
  blend: BlendMode,
) -> Entity {
  let [r, g, b, a] = PLAIN;
  let image = ctx.assets().add(Image::solid_rect(8, 8, r, g, b, a));
  ctx
    .world
    .spawn((
      Transform::from_xy(x, 8.0),
      Renderable::image(image)
        .with_anchor(Anchor::TopLeft)
        .with_blend(blend)
        .with_shader(SpriteShader::custom(shader)),
    ))
    .id()
}

/// The last frame's pixel at (x, y)
fn pixel(engine: &HeadlessEngine, x: u32, y: u32) -> [u8; 4] {
  engine.frame().expect("a frame").get_pixel(x, y).0
}

/// Replaces `shader`'s value with WGSL `source`, as code may
fn change(engine: &mut HeadlessEngine, shader: &Handle<Shader>, name: &str, source: String) {
  *engine
    .assets()
    .get_mut(shader)
    .expect("the shader is loaded") = Shader::from_wgsl(name, source);
}

/// Steps until `done` holds after a frame (at most `PATIENCE`), calling
/// `each` on every frame before that; the frames stepped
fn step_until(
  engine: &mut HeadlessEngine,
  mut each: impl FnMut(&HeadlessEngine, u32),
  done: impl Fn(&HeadlessEngine) -> bool,
) -> u32 {
  let start = Instant::now();
  let mut frames = 0;
  loop {
    engine.step().expect("a frame");
    frames += 1;
    if done(engine) {
      return frames;
    }
    each(engine, frames);
    assert!(start.elapsed() < PATIENCE, "gave up after {frames} frames");
    std::thread::sleep(Duration::from_millis(1));
  }
}

/// The warnings in the dev log naming `name`
fn warnings(name: &str) -> usize {
  dev_log::get_entries()
    .iter()
    .filter(|entry| entry.level == dev_log::LogLevel::Warn && entry.message.contains(name))
    .count()
}

/// A sprite drawn with a shader the scene adds from `source`, under `name`
fn shaded(name: &'static str, source: String) -> Option<(HeadlessEngine, Handle<Shader>)> {
  let slot: Slot<Handle<Shader>> = Slot::default();
  let held = slot.clone();
  let mut engine = start(None, move |ctx| {
    let shader = ctx.assets().add(Shader::from_wgsl(name, source));
    *held
      .lock()
      .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(shader.clone());
    spawn_shaded(ctx, shader, 8.0, BlendMode::Alpha);
  })?;
  // Compiled at once, drawn once the GPU's check is back
  engine.step_frames(3).expect("the first frames");
  Some((engine, taken(&slot)))
}

#[test]
fn a_reloaded_shader_draws_the_old_one_until_the_new_compiles_then_the_new() {
  let path = "shaders/t14c-look.wgsl";
  let files = MemorySource::new();
  files.insert(path, fill(GREEN));
  let slot: Slot<Handle<Shader>> = Slot::default();
  let held = slot.clone();
  let Some(mut engine) = start(Some(files.clone()), move |ctx| {
    let shader = ctx.assets().load_now::<Shader>(path).expect("it loads");
    *held
      .lock()
      .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(shader.clone());
    spawn_shaded(ctx, shader, 8.0, BlendMode::Alpha);
  }) else {
    return;
  };
  engine.step_frames(3).expect("the first frames");
  assert_eq!(pixel(&engine, 12, 12), GREEN, "the first version draws");
  let shader = taken(&slot);
  engine.hold_shader_compiles(true);
  files.insert(path, fill(BLUE));
  // The new version lands, then compiles: the old one draws every frame
  let new = fill(BLUE);
  step_until(
    &mut engine,
    |engine, frame| assert_eq!(pixel(engine, 12, 12), GREEN, "frame {frame}"),
    |engine| {
      engine
        .world()
        .resource::<Assets<Shader>>()
        .get(&shader)
        .map(Shader::source)
        == Some(new.as_str())
    },
  );
  for frame in 0..5 {
    assert_eq!(
      pixel(&engine, 12, 12),
      GREEN,
      "frame {frame} after landing: the old shader draws while the new one compiles"
    );
    assert!(engine.shader_reload_pending(shader.id()));
    engine.step().expect("a frame");
  }
  engine.hold_shader_compiles(false);
  step_until(
    &mut engine,
    |engine, frame| assert_eq!(pixel(engine, 12, 12), GREEN, "frame {frame} after the hold"),
    |engine| pixel(engine, 12, 12) != GREEN,
  );
  assert_eq!(
    pixel(&engine, 12, 12),
    BLUE,
    "then the new one, never plain"
  );
  assert!(!engine.shader_reload_pending(shader.id()));
  for _ in 0..3 {
    engine.step().expect("a frame");
    assert_eq!(pixel(&engine, 12, 12), BLUE);
  }
  assert_eq!(engine.shader_pipeline_count(), 1, "the old pipeline went");
}

/// An effect in `stage` loaded from a file, saved: the old effect runs
/// until the new one compiled, then the new one
fn reload_effect(stage: EffectStage) {
  let path = "shaders/t14c-effect.wgsl";
  let files = MemorySource::new();
  files.insert(path, cover(GREEN));
  let slot: Slot<Handle<Shader>> = Slot::default();
  let held = slot.clone();
  let Some(mut engine) = start(Some(files.clone()), move |ctx| {
    let shader = ctx.assets().load_now::<Shader>(path).expect("it loads");
    *held
      .lock()
      .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(shader.clone());
    assert!(ctx.effects().add_custom_in("fx", shader, stage).is_ok());
  }) else {
    return;
  };
  engine.step_frames(3).expect("the first frames");
  assert_eq!(pixel(&engine, 32, 16), GREEN, "the first version runs");
  let shader = taken(&slot);
  engine.hold_shader_compiles(true);
  files.insert(path, cover(BLUE));
  let new = cover(BLUE);
  step_until(
    &mut engine,
    |engine, frame| assert_eq!(pixel(engine, 32, 16), GREEN, "frame {frame}"),
    |engine| {
      engine
        .world()
        .resource::<Assets<Shader>>()
        .get(&shader)
        .map(Shader::source)
        == Some(new.as_str())
    },
  );
  for frame in 0..5 {
    assert_eq!(
      pixel(&engine, 32, 16),
      GREEN,
      "frame {frame} after landing: the old effect runs while the new one compiles"
    );
    engine.step().expect("a frame");
  }
  engine.hold_shader_compiles(false);
  step_until(
    &mut engine,
    |engine, frame| assert_eq!(pixel(engine, 32, 16), GREEN, "frame {frame} after the hold"),
    |engine| pixel(engine, 32, 16) != GREEN,
  );
  assert_eq!(
    pixel(&engine, 32, 16),
    BLUE,
    "then the new one, never skipped"
  );
  for _ in 0..3 {
    engine.step().expect("a frame");
    assert_eq!(pixel(&engine, 32, 16), BLUE);
  }
  assert_eq!(engine.shader_pipeline_count(), 1, "the old pipeline went");
}

#[test]
fn a_reloaded_effect_runs_the_old_one_until_the_new_compiles_then_the_new() {
  reload_effect(EffectStage::Display);
}

#[test]
fn a_reloaded_hdr_effect_runs_the_old_one_until_the_new_compiles_then_the_new() {
  reload_effect(EffectStage::Hdr);
}

#[test]
fn a_second_save_while_compiling_draws_the_newest_and_drops_the_stale_one() {
  let name = "t14c-second.wgsl";
  let Some((mut engine, shader)) = shaded(name, fill(GREEN)) else {
    return;
  };
  assert_eq!(pixel(&engine, 12, 12), GREEN);
  engine.hold_shader_compiles(true);
  for colour in [BLUE, WHITE, YELLOW] {
    change(&mut engine, &shader, name, fill(colour));
    for _ in 0..2 {
      engine.step().expect("a frame");
      assert_eq!(pixel(&engine, 12, 12), GREEN, "the old one draws meanwhile");
    }
    assert_eq!(
      engine.shader_compiles_started(),
      1,
      "nothing more is sent while a compile is out"
    );
  }
  engine.hold_shader_compiles(false);
  step_until(
    &mut engine,
    |engine, frame| {
      assert_eq!(
        pixel(engine, 12, 12),
        GREEN,
        "frame {frame}: never a stale one"
      )
    },
    |engine| pixel(engine, 12, 12) != GREEN,
  );
  assert_eq!(pixel(&engine, 12, 12), YELLOW, "the newest");
  assert_eq!(
    engine.shader_compiles_started(),
    2,
    "the first save's compile, then the newest; the second save's never"
  );
  assert_eq!(engine.shader_pipeline_count(), 1);
}

#[test]
fn a_shader_freed_while_compiling_installs_nothing() {
  let name = "t14c-freed.wgsl";
  let slot: Slot<(Handle<Shader>, Entity)> = Slot::default();
  let held = slot.clone();
  let Some(mut engine) = start(None, move |ctx| {
    let shader = ctx.assets().add(Shader::from_wgsl(name, fill(GREEN)));
    let sprite = spawn_shaded(ctx, shader.clone(), 8.0, BlendMode::Alpha);
    *held
      .lock()
      .unwrap_or_else(std::sync::PoisonError::into_inner) = Some((shader, sprite));
  }) else {
    return;
  };
  engine.step_frames(3).expect("the first frames");
  assert_eq!(pixel(&engine, 12, 12), GREEN);
  let (shader, sprite) = taken(&slot);
  *slot
    .lock()
    .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
  engine.hold_shader_compiles(true);
  change(&mut engine, &shader, name, fill(BLUE));
  engine.step_frames(2).expect("frames");
  assert_eq!(pixel(&engine, 12, 12), GREEN, "the old one draws meanwhile");
  assert_eq!(engine.shader_compiles_out(), 1, "its new version compiles");
  // Its only holders go: the shader is freed while its compile is out
  engine.world_mut().despawn(sprite);
  drop(shader);
  engine.step_frames(2).expect("frames");
  engine.hold_shader_compiles(false);
  step_until(
    &mut engine,
    |_, _| {},
    |engine| engine.shader_compiles_out() == 0,
  );
  engine.step_frames(2).expect("frames");
  assert_eq!(engine.shader_pipeline_count(), 0, "nothing was installed");
  assert_ne!(pixel(&engine, 12, 12), BLUE);
}

#[test]
fn two_shaders_of_one_source_keep_theirs_while_one_compiles_its_new_text() {
  let slot: Slot<[Handle<Shader>; 2]> = Slot::default();
  let held = slot.clone();
  let Some(mut engine) = start(None, move |ctx| {
    let a = ctx
      .assets()
      .add(Shader::from_wgsl("t14c-twin-a.wgsl", fill(GREEN)));
    let b = ctx
      .assets()
      .add(Shader::from_wgsl("t14c-twin-b.wgsl", fill(GREEN)));
    spawn_shaded(ctx, a.clone(), 8.0, BlendMode::Alpha);
    spawn_shaded(ctx, b.clone(), 32.0, BlendMode::Alpha);
    *held
      .lock()
      .unwrap_or_else(std::sync::PoisonError::into_inner) = Some([a, b]);
  }) else {
    return;
  };
  engine.step_frames(3).expect("the first frames");
  let [a, b] = taken(&slot);
  assert_eq!(
    (pixel(&engine, 12, 12), pixel(&engine, 36, 12)),
    (GREEN, GREEN)
  );
  assert_eq!(
    engine.shader_pipeline_count(),
    1,
    "one source, one pipeline"
  );
  engine.hold_shader_compiles(true);
  change(&mut engine, &a, "t14c-twin-a.wgsl", fill(BLUE));
  for _ in 0..3 {
    engine.step().expect("a frame");
    assert_eq!(
      (pixel(&engine, 12, 12), pixel(&engine, 36, 12)),
      (GREEN, GREEN)
    );
  }
  engine.hold_shader_compiles(false);
  step_until(
    &mut engine,
    |engine, frame| assert_eq!(pixel(engine, 12, 12), GREEN, "frame {frame}"),
    |engine| pixel(engine, 12, 12) != GREEN,
  );
  assert_eq!(pixel(&engine, 12, 12), BLUE, "a draws its new text");
  assert_eq!(pixel(&engine, 36, 12), GREEN, "b keeps the shared one");
  assert_eq!(engine.shader_pipeline_count(), 2);
  assert_eq!(engine.shader_compiles_started(), 1);
  // b takes a's new text: compiled already, it is drawn from the next frame
  change(&mut engine, &b, "t14c-twin-b.wgsl", fill(BLUE));
  engine.step().expect("a frame");
  assert_eq!(
    pixel(&engine, 36, 12),
    GREEN,
    "the old one draws in the frame it changed"
  );
  engine.step().expect("a frame");
  assert_eq!(
    pixel(&engine, 36, 12),
    BLUE,
    "the shared new one from the next"
  );
  assert_eq!(
    engine.shader_compiles_started(),
    1,
    "with no compile of its own"
  );
  engine.step().expect("a frame");
  assert_eq!(
    engine.shader_pipeline_count(),
    1,
    "the old text's pipeline went"
  );
}

#[test]
fn a_blend_mode_first_drawn_while_the_new_version_compiles_is_compiled_too() {
  let name = "t14c-variant.wgsl";
  let Some((mut engine, shader)) = shaded(name, fill(GREEN)) else {
    return;
  };
  engine.hold_shader_compiles(true);
  change(&mut engine, &shader, name, fill(BLUE));
  engine.step_frames(2).expect("frames");
  assert_eq!(engine.shader_compiles_started(), 1);
  // A second sprite with the shader, in another blend mode: its first
  // frame draws it plain, as any blend mode's first does, then the old one
  let added = shader.clone();
  let world = engine.world_mut();
  let [r, g, b, a] = PLAIN;
  let image = AssetsView::new(world).add(Image::solid_rect(8, 8, r, g, b, a));
  world.spawn((
    Transform::from_xy(32.0, 8.0),
    Renderable::image(image)
      .with_anchor(Anchor::TopLeft)
      .with_blend(BlendMode::Additive)
      .with_shader(SpriteShader::custom(added)),
  ));
  engine.step_frames(3).expect("frames");
  let additive_green = pixel(&engine, 36, 12);
  assert_eq!(pixel(&engine, 12, 12), GREEN);
  engine.hold_shader_compiles(false);
  step_until(
    &mut engine,
    |engine, frame| {
      assert_eq!(pixel(engine, 12, 12), GREEN, "frame {frame}");
      assert_eq!(pixel(engine, 36, 12), additive_green, "frame {frame}");
    },
    |engine| pixel(engine, 12, 12) != GREEN,
  );
  assert_eq!(pixel(&engine, 12, 12), BLUE);
  let additive_blue = pixel(&engine, 36, 12);
  assert_ne!(
    additive_blue, additive_green,
    "both sprites take the new one in one frame"
  );
  assert_eq!(
    engine.shader_compiles_started(),
    2,
    "the new blend mode compiled too, before the new version drew"
  );
  for _ in 0..3 {
    engine.step().expect("a frame");
    assert_eq!(
      (pixel(&engine, 12, 12), pixel(&engine, 36, 12)),
      (BLUE, additive_blue)
    );
  }
}

#[test]
fn a_change_the_gpu_rejects_draws_the_last_good_shader_in_every_frame() {
  let name = "t14c-rejected.wgsl";
  let Some((mut engine, shader)) = shaded(name, fill(GREEN)) else {
    return;
  };
  change(&mut engine, &shader, name, REJECTED_SPRITE.to_string());
  step_until(
    &mut engine,
    |engine, frame| assert_eq!(pixel(engine, 12, 12), GREEN, "frame {frame}: never plain"),
    |_| warnings(name) > 0,
  );
  for _ in 0..3 {
    assert_eq!(pixel(&engine, 12, 12), GREEN);
    engine.step().expect("a frame");
  }
  assert_eq!(warnings(name), 1, "warned once for the version");
}

#[test]
fn an_effect_change_the_gpu_rejects_runs_the_last_good_effect_in_every_frame() {
  let name = "t14c-rejected-effect.wgsl";
  let slot: Slot<Handle<Shader>> = Slot::default();
  let held = slot.clone();
  let Some(mut engine) = start(None, move |ctx| {
    let shader = ctx.assets().add(Shader::from_wgsl(name, cover(GREEN)));
    *held
      .lock()
      .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(shader.clone());
    assert!(ctx.effects().add_custom("fx", shader).is_ok());
  }) else {
    return;
  };
  engine.step_frames(3).expect("the first frames");
  assert_eq!(pixel(&engine, 32, 16), GREEN);
  let shader = taken(&slot);
  change(&mut engine, &shader, name, REJECTED_EFFECT.to_string());
  step_until(
    &mut engine,
    |engine, frame| assert_eq!(pixel(engine, 32, 16), GREEN, "frame {frame}: never skipped"),
    |_| warnings(name) > 0,
  );
  for _ in 0..3 {
    assert_eq!(pixel(&engine, 32, 16), GREEN);
    engine.step().expect("a frame");
  }
  assert_eq!(warnings(name), 1, "warned once for the version");
}

#[test]
fn a_shader_with_no_version_the_gpu_accepted_compiles_its_next_at_once() {
  let name = "t14c-at-once.wgsl";
  let Some((mut engine, shader)) = shaded(name, "fn sprite( {".to_string()) else {
    return;
  };
  assert_eq!(pixel(&engine, 12, 12), PLAIN, "drawn plain while broken");
  change(&mut engine, &shader, name, fill(GREEN));
  engine.step().expect("a frame");
  assert_eq!(
    pixel(&engine, 12, 12),
    PLAIN,
    "compiled at once, its check out"
  );
  engine.step().expect("a frame");
  assert_eq!(
    pixel(&engine, 12, 12),
    GREEN,
    "drawn once the check is back"
  );
  assert_eq!(
    engine.shader_compiles_started(),
    0,
    "nothing went to the thread"
  );
}

#[test]
fn an_effect_changed_while_disabled_runs_its_last_good_version_until_the_new_compiles() {
  let name = "t14c-disabled.wgsl";
  let slot: Slot<Handle<Shader>> = Slot::default();
  let held = slot.clone();
  let Some(mut engine) = start(None, move |ctx| {
    let shader = ctx.assets().add(Shader::from_wgsl(name, cover(GREEN)));
    *held
      .lock()
      .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(shader.clone());
    assert!(ctx.effects().add_custom("fx", shader).is_ok());
  }) else {
    return;
  };
  engine.step_frames(3).expect("the first frames");
  assert_eq!(pixel(&engine, 32, 16), GREEN);
  let shader = taken(&slot);
  let enable = |engine: &mut HeadlessEngine, on: bool| {
    let mut effects = engine.world_mut().resource_mut::<EffectsManager>();
    assert!(effects.set_custom_enabled("fx", on));
  };
  enable(&mut engine, false);
  change(&mut engine, &shader, name, cover(BLUE));
  // Past the shader caches' sweep, which runs every 120 frames
  engine.step_frames(130).expect("frames");
  engine.hold_shader_compiles(true);
  enable(&mut engine, true);
  for frame in 0..3 {
    engine.step().expect("a frame");
    assert_eq!(
      pixel(&engine, 32, 16),
      GREEN,
      "frame {frame}: its last good version"
    );
  }
  engine.hold_shader_compiles(false);
  step_until(
    &mut engine,
    |engine, frame| assert_eq!(pixel(engine, 32, 16), GREEN, "frame {frame}"),
    |engine| pixel(engine, 32, 16) != GREEN,
  );
  assert_eq!(pixel(&engine, 32, 16), BLUE);
}

#[test]
fn dropping_the_engine_while_a_shader_compiles_waits_for_nothing() {
  let name = "t14c-shutdown.wgsl";
  let Some((mut engine, shader)) = shaded(name, fill(GREEN)) else {
    return;
  };
  let gates = engine.shader_compile_gates();
  engine.hold_shader_compiles(true);
  change(&mut engine, &shader, name, fill(BLUE));
  engine.step_frames(2).expect("frames");
  assert_eq!(engine.shader_compiles_out(), 1, "a compile is out");
  drop(shader);
  let start = Instant::now();
  drop(engine);
  let took = start.elapsed();
  for gate in &gates {
    gate.hold(false);
  }
  assert!(
    took < Duration::from_millis(500),
    "the engine dropped in {took:?}"
  );
}
