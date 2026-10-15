//! World drawing checks that need engine internals: texture counts, what was
//! uploaded and the draw-call count. Pixel-level behaviour lives in
//! `tests/e2e/gpu_world.rs`.

use super::HeadlessEngine;
use crate::core::engine::EngineBuilder;
use crate::prelude::*;

const RED: [u8; 4] = [220, 30, 40, 255];

/// A scene that spawns its entities in `on_enter` and idles.
struct Fixture {
  setup: fn(&mut SceneContext),
}

impl Scene for Fixture {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    (self.setup)(ctx);
  }
}

fn engine(builder: EngineBuilder) -> Option<HeadlessEngine> {
  HeadlessEngine::new_or_skip(builder)
}

#[test]
fn replaced_sprite_pixels_are_drawn_again() {
  let scene = Fixture {
    setup: |ctx| {
      let sprite =
        SpriteHandle::from_sprite(Sprite::solid_rect(10, 10, 220, 30, 40, 255), "reload");
      ctx.world.spawn((
        Transform::from_xy(40.0, 40.0),
        Renderable::sprite(sprite).with_anchor(Anchor::TopLeft),
      ));
    },
  };
  let Some(mut engine) = engine(EngineBuilder::new().resolution(120, 90).start_scene(scene)) else {
    return;
  };
  engine.step_frames(2).unwrap();
  assert_eq!(engine.frame().unwrap().get_pixel(45, 45).0, RED);

  // A hot reload replaces the pixels behind the same handle
  let handle = {
    let world = engine.world_mut();
    let mut renderables = world.query::<&Renderable>();
    renderables.iter(world).next().unwrap().sprite.clone()
  };
  handle.update(Sprite::solid_rect(10, 10, 10, 200, 30, 255));
  engine.step_frames(2).unwrap();
  assert_eq!(
    engine.frame().unwrap().get_pixel(45, 45).0,
    [10, 200, 30, 255]
  );
}

/// Changes the blob shadow's radius and colour every update.
struct VaryingBlob;

impl Scene for VaryingBlob {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    let sprite = SpriteHandle::from_sprite(Sprite::solid_rect(8, 8, 220, 30, 40, 255), "varying");
    ctx.world.spawn((
      Transform::from_xy(60.0, 40.0),
      Renderable::sprite(sprite)
        .with_anchor(Anchor::TopLeft)
        .with_blob_shadow(BlobShadow::default()),
    ));
  }

  fn update(&mut self, ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    let mut renderables = ctx.world.query::<&mut Renderable>();
    for mut renderable in renderables.iter_mut(ctx.world) {
      if let Some(blob) = renderable.blob_shadow.as_mut() {
        blob.radius_x += 0.37;
        blob.radius_y += 0.21;
        let shift = (blob.radius_x * 7.0) as u8;
        blob.color = Color::new(shift, 255 - shift, shift / 2, 200);
      }
    }
    SceneAction::Continue
  }
}

#[test]
fn varying_shadows_do_not_grow_the_texture_cache() {
  let builder = EngineBuilder::new()
    .resolution(120, 90)
    .start_scene(VaryingBlob);
  let Some(mut engine) = engine(builder) else {
    return;
  };
  let count = |engine: &HeadlessEngine| engine.engine.sprite_gpu.as_ref().unwrap().texture_count();
  engine.step().unwrap();
  let first = count(&engine);
  engine.step_frames(200).unwrap();
  assert_eq!(count(&engine), first, "shadow textures leaked");
}

#[test]
fn off_screen_entities_are_not_uploaded() {
  let scene = Fixture {
    setup: |ctx| {
      let near = SpriteHandle::from_sprite(Sprite::solid_rect(8, 8, 1, 2, 3, 255), "near");
      let far = SpriteHandle::from_sprite(Sprite::solid_rect(8, 8, 4, 5, 6, 255), "far");
      ctx.world.spawn((
        Transform::from_xy(20.0, 20.0),
        Renderable::sprite(near).with_anchor(Anchor::TopLeft),
      ));
      ctx.world.spawn((
        Transform::from_xy(5000.0, 20.0),
        Renderable::sprite(far).with_anchor(Anchor::TopLeft),
      ));
    },
  };
  let Some(mut engine) = engine(EngineBuilder::new().resolution(120, 90).start_scene(scene)) else {
    return;
  };
  engine.step_frames(2).unwrap();
  let handles: Vec<SpriteHandle> = {
    let world = engine.world_mut();
    let mut renderables = world.query::<&Renderable>();
    renderables
      .iter(world)
      .map(|renderable| renderable.sprite.clone())
      .collect()
  };
  let gpu = engine.engine.sprite_gpu.as_ref().unwrap();
  let uploaded = |name: &str| {
    handles
      .iter()
      .filter(|h| h.id() == name)
      .any(|h| gpu.has_entity_sprite(h))
  };
  assert!(uploaded("near"));
  assert!(!uploaded("far"), "an off-screen sprite was uploaded");
}

#[test]
fn large_standalone_entity_textures_are_bounded() {
  // A new 300x20 sprite (standalone, over the 256 px atlas limit) every frame
  let scene = Fixture { setup: |_| {} };
  let Some(mut engine) = engine(EngineBuilder::new().resolution(120, 90).start_scene(scene)) else {
    return;
  };
  for i in 0..200 {
    {
      let world = engine.world_mut();
      let old: Vec<Entity> = world
        .query_filtered::<Entity, With<Renderable>>()
        .iter(world)
        .collect();
      for entity in old {
        world.despawn(entity);
      }
      let sprite = SpriteHandle::from_sprite(
        Sprite::solid_rect(300, 20, 1, 2, 3, 255),
        format!("wide-{i}"),
      );
      world.spawn((
        Transform::from_xy(0.0, 10.0),
        Renderable::sprite(sprite).with_anchor(Anchor::TopLeft),
      ));
    }
    engine.step().unwrap();
  }
  let gpu = engine.engine.sprite_gpu.as_ref().unwrap();
  assert!(
    gpu.standalone_entity_count() <= 64,
    "standalone entity textures: {}",
    gpu.standalone_entity_count()
  );
}

#[test]
fn ten_thousand_sprites_few_draws() {
  // 10,000 visible entities from 3 textures, 2x2 px each on a 3 px grid
  let scene = Fixture {
    setup: |ctx| {
      let textures: Vec<SpriteHandle> = (0..3)
        .map(|i| {
          let c = 60 + i as u8 * 80;
          SpriteHandle::from_sprite(
            Sprite::solid_rect(2, 2, c, 30, 40, 255),
            format!("many-{i}"),
          )
        })
        .collect();
      for i in 0..10_000usize {
        let (col, row) = ((i % 100) as f32, (i / 100) as f32);
        ctx.world.spawn((
          Transform::from_xy(col * 3.0, row * 3.0),
          Renderable::sprite(textures[i % 3].clone()).with_anchor(Anchor::TopLeft),
        ));
      }
    },
  };
  let builder = EngineBuilder::new().resolution(300, 300).start_scene(scene);
  let Some(mut engine) = engine(builder) else {
    return;
  };
  engine.step_frames(2).unwrap();
  let stats = engine.engine.debug_stats();
  assert_eq!(stats.entity_count, 10_000);
  assert!(
    (1..=8).contains(&stats.draw_calls),
    "draw calls for 10,000 sprites: {}",
    stats.draw_calls
  );
}

/// Four sprites in z order using shaders `a`, `a`, `b`, `b`; the frame's draw calls
fn draw_calls_for(a_src: &'static str, b_src: &'static str) -> Option<u32> {
  struct Shaded {
    sources: (&'static str, &'static str),
  }
  impl Scene for Shaded {
    fn on_enter(&mut self, ctx: &mut SceneContext) {
      let a = ShaderHandle::from_source("split-a.wgsl", self.sources.0);
      let b = ShaderHandle::from_source("split-b.wgsl", self.sources.1);
      let sprite = SpriteHandle::from_sprite(Sprite::solid_rect(4, 4, 200, 30, 40, 255), "split");
      for (i, shader) in [&a, &a, &b, &b].into_iter().enumerate() {
        ctx.world.spawn((
          Transform::from_xy(i as f32 * 6.0, 0.0),
          Renderable::sprite(sprite.clone())
            .with_anchor(Anchor::TopLeft)
            .with_z_order(i as i32)
            .with_shader(SpriteShader::custom(shader.clone())),
        ));
      }
    }
    fn update(&mut self, _ctx: &mut SceneContext, _dt: f32) -> SceneAction {
      SceneAction::Continue
    }
  }
  let builder = EngineBuilder::new().resolution(64, 64).start_scene(Shaded {
    sources: (a_src, b_src),
  });
  let mut engine = engine(builder)?;
  engine.step_frames(2).unwrap();
  Some(engine.engine.debug_stats().draw_calls)
}

#[test]
fn two_shaders_split_draw_runs() {
  // One draw per shader; the shared texture doesn't merge them
  let Some(calls) = draw_calls_for(
    "fn sprite(in: SpriteIn, params: Params) -> vec4<f32> { return sample(in.uv); }",
    "fn sprite(in: SpriteIn, params: Params) -> vec4<f32> { return sample(in.uv).bgra; }",
  ) else {
    return;
  };
  assert_eq!(calls, 2);
}

#[test]
fn handles_with_one_source_share_a_draw_run() {
  let src = "fn sprite(in: SpriteIn, params: Params) -> vec4<f32> { return sample(in.uv); }";
  let Some(calls) = draw_calls_for(src, src) else {
    return;
  };
  assert_eq!(calls, 1);
}

#[test]
fn editing_a_shader_file_redraws_with_the_new_source() {
  use crate::core::asset::HotReloadEvent;

  const GREEN_SHADER: &str = "fn sprite(in: SpriteIn, params: Params) -> vec4<f32> { return vec4<f32>(0.0, 1.0, 0.0, 1.0); }";
  const BLUE_SHADER: &str = "fn sprite(in: SpriteIn, params: Params) -> vec4<f32> { return vec4<f32>(0.0, 0.0, 1.0, 1.0); }";

  /// Draws a sprite with the shader in `path`, then rewrites the file and
  /// delivers the change the way the engine's per-frame reload does
  struct Edits {
    path: std::path::PathBuf,
    updates: u32,
  }
  impl Scene for Edits {
    fn on_enter(&mut self, ctx: &mut SceneContext) {
      ctx.look().tonemap = Tonemap::Clamp;
      let shader = ctx.assets.load_shader(&self.path).expect("load shader");
      let sprite = SpriteHandle::from_sprite(Sprite::solid_rect(8, 8, 200, 30, 40, 255), "edit");
      ctx.world.spawn((
        Transform::from_xy(4.0, 4.0),
        Renderable::sprite(sprite)
          .with_anchor(Anchor::TopLeft)
          .with_shader(SpriteShader::custom(shader)),
      ));
    }
    fn update(&mut self, ctx: &mut SceneContext, _dt: f32) -> SceneAction {
      self.updates += 1;
      if self.updates == 3 {
        std::fs::write(&self.path, BLUE_SHADER).expect("rewrite shader");
        assert!(
          ctx
            .assets
            .handle_reload_event(HotReloadEvent::FileModified(self.path.clone()))
        );
      }
      SceneAction::Continue
    }
  }

  let dir = tempfile::tempdir().expect("temp dir");
  let path = dir.path().join("edit.wgsl");
  std::fs::write(&path, GREEN_SHADER).expect("write shader");
  let builder = EngineBuilder::new()
    .resolution(32, 32)
    .start_scene(Edits { path, updates: 0 });
  let Some(mut engine) = engine(builder) else {
    return;
  };
  engine.step_frames(2).unwrap();
  assert_eq!(engine.frame().unwrap().get_pixel(8, 8).0, [0, 255, 0, 255]);
  engine.step_frames(3).unwrap();
  assert_eq!(engine.frame().unwrap().get_pixel(8, 8).0, [0, 0, 255, 255]);
}

#[test]
fn a_tapped_key_is_seen_by_exactly_one_fixed_update() {
  use std::sync::{Arc, Mutex};
  let seen = Arc::new(Mutex::new(0));
  struct Counter(Arc<Mutex<u32>>);
  impl Scene for Counter {
    fn update(&mut self, ctx: &mut SceneContext, _dt: f32) -> SceneAction {
      if ctx.input.key_pressed(KeyCode::Space) {
        *self.0.lock().unwrap() += 1;
      }
      SceneAction::Continue
    }
  }
  let Some(mut engine) = HeadlessEngine::new_or_skip(
    EngineBuilder::new()
      .resolution(32, 32)
      .start_scene(Counter(seen.clone())),
  ) else {
    return;
  };
  engine.tap_key(KeyCode::Space);
  engine.step_frames(3).unwrap();
  assert_eq!(*seen.lock().unwrap(), 1);
}

#[test]
fn an_interpolated_sprite_draws_between_its_last_two_steps() {
  use std::time::Duration;
  /// Moves its sprite 20 px right every fixed step.
  struct Mover;
  impl Scene for Mover {
    fn on_enter(&mut self, ctx: &mut SceneContext) {
      let sprite = SpriteHandle::from_sprite(Sprite::solid_rect(4, 4, 220, 30, 40, 255), "lerp");
      ctx.world.spawn((
        Transform::from_xy(0.0, 10.0),
        Renderable::sprite(sprite).with_anchor(Anchor::TopLeft),
        Interpolated,
      ));
    }
    fn update(&mut self, ctx: &mut SceneContext, _dt: f32) -> SceneAction {
      let mut sprites = ctx
        .world
        .query_filtered::<&mut Transform, With<Renderable>>();
      for mut transform in sprites.iter_mut(ctx.world) {
        transform.translation.x += 20.0;
      }
      SceneAction::Continue
    }
  }
  let builder = EngineBuilder::new()
    .resolution(120, 30)
    .fixed_hz(50)
    .start_scene(Mover);
  let Some(mut engine) = engine(builder) else {
    return;
  };
  // Two whole steps: the sprite is at x = 40 and was at 20
  engine.step_frames(2).unwrap();
  // A step and a half: one more step (to x = 60, from 40) and half a step
  // left over, so it draws half way, at x = 50
  engine.engine.frame(Some(Duration::from_millis(30)));
  let frame = engine.frame().unwrap();
  assert_eq!(frame.get_pixel(51, 11).0, RED, "drawn at x = 50");
  assert_ne!(frame.get_pixel(61, 11).0, RED, "not at the stepped x = 60");
}

#[test]
fn an_interpolated_camera_draws_between_its_last_two_steps() {
  use std::time::Duration;
  /// Pans the main camera 20 px right every fixed step over a still sprite.
  struct Pan;
  impl Scene for Pan {
    fn on_enter(&mut self, ctx: &mut SceneContext) {
      let sprite = SpriteHandle::from_sprite(Sprite::solid_rect(4, 4, 220, 30, 40, 255), "still");
      ctx.world.spawn((
        Transform::from_xy(58.0, 10.0),
        Renderable::sprite(sprite).with_anchor(Anchor::TopLeft),
      ));
    }
    fn update(&mut self, ctx: &mut SceneContext, _dt: f32) -> SceneAction {
      let mut camera = ctx.camera();
      let p = camera.position();
      camera.set_position(p.x + 20.0, p.y);
      SceneAction::Continue
    }
  }
  let builder = EngineBuilder::new()
    .resolution(120, 30)
    .fixed_hz(50)
    .start_scene(Pan);
  let Some(mut engine) = engine(builder) else {
    return;
  };
  // The camera starts at x = 60; two steps take it to 100 (from 80)
  engine.step_frames(2).unwrap();
  // One more step (to 120) and half a step left over: drawn at 110, so the
  // sprite at world 58 shows at screen 58 - 110 + 60 = 8
  engine.engine.frame(Some(Duration::from_millis(30)));
  let frame = engine.frame().unwrap();
  assert_eq!(
    frame.get_pixel(9, 11).0,
    RED,
    "drawn from the interpolated camera"
  );
}

#[test]
fn despawned_cameras_free_their_gpu_state() {
  /// Spawns three cameras, then despawns them on the fourth update.
  struct Churn(u32, Vec<Entity>);
  impl Scene for Churn {
    fn update(&mut self, ctx: &mut SceneContext, _dt: f32) -> SceneAction {
      self.0 += 1;
      if self.0 == 1 {
        for i in 0..3 {
          let camera = Camera2d {
            viewport: Viewport::new(0.0, 0.0, 0.5, 0.5),
            order: i + 1,
            ..Default::default()
          };
          self
            .1
            .push(ctx.world.spawn((Transform::IDENTITY, camera)).id());
        }
      } else if self.0 == 4 {
        for entity in self.1.drain(..) {
          ctx.world.despawn(entity);
        }
      }
      SceneAction::Continue
    }
  }
  let Some(mut engine) = engine(
    EngineBuilder::new()
      .resolution(32, 32)
      .start_scene(Churn(0, Vec::new())),
  ) else {
    return;
  };
  engine.step_frames(3).unwrap();
  let busy = engine.engine.camera_gpu_entries();
  engine.step_frames(3).unwrap();
  let calm = engine.engine.camera_gpu_entries();
  assert!(busy > calm, "{busy} then {calm}");
  assert_eq!(calm, 1, "only the main camera's uniform remains");
}

/// Sprites, a line, a batch sprite and two effects (one of them bloom).
struct Busy;

impl Scene for Busy {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    let sprite = SpriteHandle::from_sprite(Sprite::solid_rect(8, 8, 220, 30, 40, 255), "busy");
    for i in 0..20 {
      ctx.world.spawn((
        Transform::from_xy(10.0 + i as f32 * 5.0, 30.0),
        Renderable::sprite(sprite.clone()),
      ));
    }
    ctx.world.spawn((
      Transform::from_xy(0.0, 0.0),
      Line2D::horizontal(50.0, 5.0, 100.0, Color::WHITE, 2.0),
    ));
    ctx.effects().set_vignette(0.4);
    ctx.effects().set_bloom(0.6);
    let lut = SpriteHandle::from_sprite(ColorLut::identity_strip(16), "busy-lut");
    let mut look = ctx.look();
    look.grading.lut = ColorLut::from_sprite(&lut).ok();
    look.auto_exposure = Some(AutoExposure::default());
  }

  fn update(&mut self, _ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    SceneAction::Continue
  }

  fn render(&mut self, gpu: &mut GpuRenderContext) {
    let sprite =
      SpriteHandle::from_sprite(Sprite::solid_rect(4, 4, 30, 200, 60, 255), "busy-batch");
    let instance = SpriteInstance::new().with_transform(60.0, 60.0, 0.0, 4.0, 4.0);
    gpu.add_sprite(&sprite, instance, SpriteDraw::default());
  }
}

#[test]
fn steady_frames_create_no_bind_groups() {
  use crate::core::graphics::bind_groups::created_on_this_thread;
  let Some(mut engine) = engine(EngineBuilder::new().resolution(120, 90).start_scene(Busy)) else {
    return;
  };
  engine.step_frames(3).unwrap();
  let before = created_on_this_thread();
  engine.step_frames(3).unwrap();
  assert_eq!(created_on_this_thread() - before, 0);
}

#[test]
fn a_resize_rebuilds_bind_groups_once() {
  use crate::core::graphics::bind_groups::created_on_this_thread;
  let Some(mut engine) = engine(EngineBuilder::new().resolution(120, 90).start_scene(Busy)) else {
    return;
  };
  engine.step_frames(3).unwrap();
  engine.resize(200, 150);
  engine.step_frames(1).unwrap();
  let before = created_on_this_thread();
  engine.step_frames(3).unwrap();
  assert_eq!(
    created_on_this_thread() - before,
    0,
    "none after the rebuilding frame"
  );
  assert!(
    engine.frame().unwrap().pixels().any(|p| p.0[0] > 100),
    "it still draws"
  );
}

/// `Busy` plus a shadow-casting light, an occluder, a second lit camera and a
/// long fade.
struct BusyLit {
  started: bool,
}

impl Scene for BusyLit {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    Busy.on_enter(ctx);
    ctx.world.spawn((
      Transform::from_xy(60.0, 20.0),
      Light2D::point(Color::WHITE, 80.0).with_shadows(),
    ));
    ctx.world.spawn((
      Transform::from_xy(60.0, 45.0),
      LightOccluder2D::rect(10.0, 10.0),
    ));
    ctx.world.spawn((
      Transform::from_xy(60.0, 45.0),
      Camera2d {
        viewport: Viewport::new(0.5, 0.0, 0.5, 0.5),
        order: 1,
        ..Default::default()
      },
    ));
  }

  fn update(&mut self, _ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    if std::mem::replace(&mut self.started, true) {
      return SceneAction::Continue;
    }
    // Push keeps this scene's world (and its cameras) drawing under the fade
    SceneAction::PushWith(
      Box::new(Idle),
      Transition::fade(std::time::Duration::from_secs(10)),
    )
  }
}

/// A scene that does nothing (pushed over `BusyLit`, which keeps drawing).
struct Idle;

impl Scene for Idle {
  fn update(&mut self, _ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    SceneAction::Continue
  }
}

#[test]
fn lit_split_frames_create_no_bind_groups() {
  use crate::core::graphics::bind_groups::created_on_this_thread;
  let scene = BusyLit { started: false };
  let Some(mut engine) = engine(EngineBuilder::new().resolution(120, 90).start_scene(scene)) else {
    return;
  };
  engine.step_frames(4).unwrap();
  let before = created_on_this_thread();
  engine.step_frames(3).unwrap();
  assert_eq!(created_on_this_thread() - before, 0);
}

#[test]
fn the_batch_grows_past_its_first_capacity() {
  // 2,000 visible 1x1 sprites, more than the instance buffer starts with
  let scene = Fixture {
    setup: |ctx| {
      let dot = SpriteHandle::from_sprite(Sprite::solid_rect(1, 1, 200, 40, 40, 255), "grow-dot");
      for i in 0..2_000usize {
        let (col, row) = ((i % 50) as f32, (i / 50) as f32);
        ctx.world.spawn((
          Transform::from_xy(col * 2.0, row * 2.0),
          Renderable::sprite(dot.clone()).with_anchor(Anchor::TopLeft),
        ));
      }
    },
  };
  let builder = EngineBuilder::new().resolution(100, 80).start_scene(scene);
  let Some(mut engine) = engine(builder) else {
    return;
  };
  engine.step_frames(2).unwrap();
  let frame = engine.frame().unwrap();
  let batch = engine.engine.batch.as_ref().unwrap();
  assert_eq!(batch.capacity(), 2048);
  // The last sprite drawn (column 49, row 39) shows
  assert_eq!(frame.get_pixel(98, 78).0, [200, 40, 40, 255]);
}

#[test]
fn a_reloaded_sprite_is_new_at_every_mip_level() {
  // 64 px drawn at 8: sampled from the smaller mip levels
  let scene = Fixture {
    setup: |ctx| {
      let red = Sprite::solid_rect(64, 64, 220, 30, 40, 255);
      let sprite = SpriteHandle::from_sprite(red, "reload-mips");
      ctx.world.spawn((
        Transform::from_xy(16.0, 16.0).with_scale(Vec2::splat(0.125)),
        Renderable::sprite(sprite).with_filter(Filter::Linear),
      ));
    },
  };
  let Some(mut engine) = engine(EngineBuilder::new().resolution(32, 32).start_scene(scene)) else {
    return;
  };
  engine.step_frames(2).unwrap();
  assert_eq!(engine.frame().unwrap().get_pixel(16, 16).0, RED);
  let handle = {
    let world = engine.world_mut();
    let mut renderables = world.query::<&Renderable>();
    renderables.iter(world).next().unwrap().sprite.clone()
  };
  handle.update(Sprite::solid_rect(64, 64, 30, 200, 60, 255));
  engine.step_frames(2).unwrap();
  assert_eq!(
    engine.frame().unwrap().get_pixel(16, 16).0,
    [30, 200, 60, 255]
  );
}

#[test]
fn glyph_pages_bind_once_and_reuse_their_glyphs() {
  let (device, queue) = crate::core::testing::shared_gpu().expect("this check needs a GPU");
  let mut sprites = crate::core::graphics::SpriteGpuRenderer::new(&device, &queue);
  let image = |w: u32, h: u32| crate::core::text::GlyphImage {
    format: crate::core::text::GlyphFormat::Mask,
    width: w,
    height: h,
    left: 0,
    top: h as i32,
    data: vec![255; (w * h) as usize],
    base: 16.0,
  };
  let before = crate::core::graphics::bind_groups::created_on_this_thread();
  let a = crate::core::text::GlyphKey::fill(crate::core::text::FontId(0), 1, 16.0, 0.0);
  let b = crate::core::text::GlyphKey::fill(crate::core::text::FontId(0), 2, 16.0, 0.0);
  let slot_a = sprites
    .glyph(&device, &queue, a, || Some(image(8, 10)))
    .expect("placed");
  let _ = sprites
    .glyph(&device, &queue, b, || Some(image(6, 9)))
    .expect("placed");
  let made = crate::core::graphics::bind_groups::created_on_this_thread() - before;
  assert_eq!(made, 2, "one page, bound once per sampler");
  let again = sprites
    .glyph(&device, &queue, a, || panic!("no second raster"))
    .expect("cached");
  assert_eq!(again, slot_a);
  assert!(
    sprites
      .page_bind_group(
        crate::core::graphics::SlotPage::Glyph(slot_a.page),
        crate::core::render::batch::Sampling::Linear
      )
      .is_some()
  );
}
