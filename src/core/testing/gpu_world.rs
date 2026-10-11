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

  fn update(&mut self, _ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    SceneAction::Continue
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
      ctx
        .world
        .spawn((Transform::new(40.0, 40.0), Renderable::sprite(sprite)));
    },
  };
  let Some(mut engine) = engine(EngineBuilder::new().resolution(120, 90).start_scene(scene)) else {
    return;
  };
  engine.step_frames(2).unwrap();
  assert_eq!(engine.frame().unwrap().get_pixel(45, 45).0, RED);

  // A hot reload replaces the pixels behind the same handle
  let handle = {
    let world = engine.world();
    let entity = *world.entities_with::<Renderable>().first().unwrap();
    world.get::<Renderable>(entity).unwrap().sprite.clone()
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
      Transform::new(60.0, 40.0),
      Renderable::sprite(sprite).with_blob_shadow(BlobShadow::default()),
    ));
  }

  fn update(&mut self, ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    for entity in ctx.world.entities_with::<Renderable>() {
      if let Some(mut renderable) = ctx.world.get_mut::<Renderable>(entity)
        && let Some(blob) = renderable.blob_shadow.as_mut()
      {
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
      ctx
        .world
        .spawn((Transform::new(20.0, 20.0), Renderable::sprite(near)));
      ctx
        .world
        .spawn((Transform::new(5000.0, 20.0), Renderable::sprite(far)));
    },
  };
  let Some(mut engine) = engine(EngineBuilder::new().resolution(120, 90).start_scene(scene)) else {
    return;
  };
  engine.step_frames(2).unwrap();
  let handles: Vec<SpriteHandle> = {
    let world = engine.world();
    world
      .entities_with::<Renderable>()
      .into_iter()
      .map(|e| world.get::<Renderable>(e).unwrap().sprite.clone())
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
fn raycast_frames_do_not_collect_the_2d_world() {
  let scene = Fixture {
    setup: |ctx| {
      ctx.enable_raycast();
      let sprite = SpriteHandle::from_sprite(Sprite::solid_rect(8, 8, 1, 2, 3, 255), "ray-near");
      ctx
        .world
        .spawn((Transform::new(20.0, 20.0), Renderable::sprite(sprite)));
    },
  };
  let Some(mut engine) = engine(EngineBuilder::new().resolution(120, 90).start_scene(scene)) else {
    return;
  };
  engine.step_frames(3).unwrap();
  let sprite = {
    let world = engine.world();
    let entity = *world.entities_with::<Renderable>().first().unwrap();
    world.get::<Renderable>(entity).unwrap().sprite.clone()
  };
  let gpu = engine.engine.sprite_gpu.as_ref().unwrap();
  assert!(
    !gpu.has_entity_sprite(&sprite),
    "a raycast frame uploaded a 2D entity sprite"
  );
}

#[test]
fn raycast_frames_count_their_own_draws() {
  // Three shaders draw three runs in 2D; then the raycast view takes over
  struct ThenRaycast {
    updates: u32,
  }
  impl Scene for ThenRaycast {
    fn on_enter(&mut self, ctx: &mut SceneContext) {
      let sprite =
        SpriteHandle::from_sprite(Sprite::solid_rect(4, 4, 200, 30, 40, 255), "ray-runs");
      for (i, swizzle) in ["rgba", "bgra", "gbra"].into_iter().enumerate() {
        let src = format!(
          "fn sprite(in: SpriteIn, params: Params) -> vec4<f32> {{ return sample(in.uv).{swizzle}; }}"
        );
        let shader = ShaderHandle::from_source(format!("ray-runs-{i}.wgsl"), src);
        ctx.world.spawn((
          Transform::new(i as f32 * 6.0, 0.0),
          Renderable::sprite(sprite.clone())
            .with_z_order(i as i32)
            .with_shader(SpriteShader::custom(shader)),
        ));
      }
    }
    fn update(&mut self, ctx: &mut SceneContext, _dt: f32) -> SceneAction {
      self.updates += 1;
      if self.updates == 3 {
        ctx.enable_raycast();
      }
      SceneAction::Continue
    }
  }
  let builder = EngineBuilder::new()
    .resolution(64, 64)
    .start_scene(ThenRaycast { updates: 0 });
  let Some(mut engine) = engine(builder) else {
    return;
  };
  engine.step_frames(2).unwrap();
  assert_eq!(engine.engine.debug_stats().draw_calls, 3, "2D frame");
  engine.step_frames(3).unwrap();
  // Walls, plus sprites when there are any
  assert!(
    engine.engine.debug_stats().draw_calls <= 2,
    "raycast frame reported {} draws",
    engine.engine.debug_stats().draw_calls
  );
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
      for entity in world.entities_with::<Renderable>() {
        let _ = world.despawn(entity);
      }
      let sprite = SpriteHandle::from_sprite(
        Sprite::solid_rect(300, 20, 1, 2, 3, 255),
        format!("wide-{i}"),
      );
      world.spawn((Transform::new(0.0, 10.0), Renderable::sprite(sprite)));
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
          Transform::new(col * 3.0, row * 3.0),
          Renderable::sprite(textures[i % 3].clone()),
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
          Transform::new(i as f32 * 6.0, 0.0),
          Renderable::sprite(sprite.clone())
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
      let shader = ctx.assets.load_shader(&self.path).expect("load shader");
      let sprite = SpriteHandle::from_sprite(Sprite::solid_rect(8, 8, 200, 30, 40, 255), "edit");
      ctx.world.spawn((
        Transform::new(4.0, 4.0),
        Renderable::sprite(sprite).with_shader(SpriteShader::custom(shader)),
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
