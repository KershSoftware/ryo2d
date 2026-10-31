//! World drawing checks that need engine internals: texture counts, what was
//! uploaded and the draw-call count. Pixel-level behaviour lives in
//! `tests/e2e/gpu_world.rs`.

use super::HeadlessEngine;
use crate::core::asset::AssetsView;
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
      let sprite = ctx
        .assets()
        .add(Image::solid_rect(10, 10, 220, 30, 40, 255));
      ctx.world.spawn((
        Transform::from_xy(40.0, 40.0),
        Renderable::image(sprite).with_anchor(Anchor::TopLeft),
      ));
    },
  };
  let Some(mut engine) = engine(EngineBuilder::new().resolution(120, 90).start_scene(scene)) else {
    return;
  };
  engine.step_frames(2).unwrap();
  assert_eq!(engine.frame().unwrap().get_pixel(45, 45).0, RED);

  // A reload replaces the pixels behind the same handle
  let handle = {
    let world = engine.world_mut();
    let mut renderables = world.query::<&Renderable>();
    renderables.iter(world).next().unwrap().image.clone()
  };
  *AssetsView::new(engine.world_mut())
    .get_mut(&handle)
    .unwrap() = Image::solid_rect(10, 10, 10, 200, 30, 255);
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
    let sprite = ctx.assets().add(Image::solid_rect(8, 8, 220, 30, 40, 255));
    ctx.world.spawn((
      Transform::from_xy(60.0, 40.0),
      Renderable::image(sprite)
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
      let near = ctx.assets().add(Image::solid_rect(8, 8, 1, 2, 3, 255));
      let far = ctx.assets().add(Image::solid_rect(8, 8, 4, 5, 6, 255));
      ctx.world.spawn((
        Transform::from_xy(20.0, 20.0),
        Renderable::image(near).with_anchor(Anchor::TopLeft),
      ));
      ctx.world.spawn((
        Transform::from_xy(5000.0, 20.0),
        Renderable::image(far).with_anchor(Anchor::TopLeft),
      ));
    },
  };
  let Some(mut engine) = engine(EngineBuilder::new().resolution(120, 90).start_scene(scene)) else {
    return;
  };
  engine.step_frames(2).unwrap();
  // Near is at x = 20, far at x = 5000
  let images: Vec<(f32, Handle<Image>)> = {
    let world = engine.world_mut();
    let mut renderables = world.query::<(&Transform, &Renderable)>();
    renderables
      .iter(world)
      .map(|(at, renderable)| (at.translation.x, renderable.image.clone()))
      .collect()
  };
  let gpu = engine.engine.sprite_gpu.as_ref().unwrap();
  let uploaded = |x: f32| {
    images
      .iter()
      .filter(|(at, _)| *at == x)
      .any(|(_, image)| gpu.has_image(image.id()))
  };
  assert!(uploaded(20.0));
  assert!(!uploaded(5000.0), "an off-screen sprite was uploaded");
}

#[test]
fn large_standalone_entity_textures_are_bounded() {
  // A new 300x20 sprite (standalone, over the 256 px atlas limit) every frame
  let scene = Fixture { setup: |_| {} };
  let Some(mut engine) = engine(EngineBuilder::new().resolution(120, 90).start_scene(scene)) else {
    return;
  };
  for _ in 0..200 {
    {
      let world = engine.world_mut();
      let old: Vec<Entity> = world
        .query_filtered::<Entity, With<Renderable>>()
        .iter(world)
        .collect();
      for entity in old {
        world.despawn(entity);
      }
      let sprite = AssetsView::new(world).add(Image::solid_rect(300, 20, 1, 2, 3, 255));
      world.spawn((
        Transform::from_xy(0.0, 10.0),
        Renderable::image(sprite).with_anchor(Anchor::TopLeft),
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
      let textures: Vec<Handle<Image>> = (0..3)
        .map(|i| {
          let c = 60 + i as u8 * 80;
          ctx.assets().add(Image::solid_rect(2, 2, c, 30, 40, 255))
        })
        .collect();
      for i in 0..10_000usize {
        let (col, row) = ((i % 100) as f32, (i / 100) as f32);
        ctx.world.spawn((
          Transform::from_xy(col * 3.0, row * 3.0),
          Renderable::image(textures[i % 3].clone()).with_anchor(Anchor::TopLeft),
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
      let a = ctx
        .assets()
        .add(Shader::from_wgsl("split-a.wgsl", self.sources.0));
      let b = ctx
        .assets()
        .add(Shader::from_wgsl("split-b.wgsl", self.sources.1));
      let sprite = ctx.assets().add(Image::solid_rect(4, 4, 200, 30, 40, 255));
      for (i, shader) in [&a, &a, &b, &b].into_iter().enumerate() {
        ctx.world.spawn((
          Transform::from_xy(i as f32 * 6.0, 0.0),
          Renderable::image(sprite.clone())
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
  const GREEN_SHADER: &str = "fn sprite(in: SpriteIn, params: Params) -> vec4<f32> { return vec4<f32>(0.0, 1.0, 0.0, 1.0); }";
  const BLUE_SHADER: &str = "fn sprite(in: SpriteIn, params: Params) -> vec4<f32> { return vec4<f32>(0.0, 0.0, 1.0, 1.0); }";

  /// Draws a sprite with the shader in `edit.wgsl`
  struct Edits;
  impl Scene for Edits {
    fn on_enter(&mut self, ctx: &mut SceneContext) {
      ctx.look().tonemap = Tonemap::Clamp;
      let shader = ctx
        .assets()
        .load_now::<Shader>("edit.wgsl")
        .expect("load shader");
      let sprite = ctx.assets().add(Image::solid_rect(8, 8, 200, 30, 40, 255));
      ctx.world.spawn((
        Transform::from_xy(4.0, 4.0),
        Renderable::image(sprite)
          .with_anchor(Anchor::TopLeft)
          .with_shader(SpriteShader::custom(shader)),
      ));
    }
  }

  let dir = tempfile::tempdir().expect("temp dir");
  let path = dir.path().join("edit.wgsl");
  std::fs::write(&path, GREEN_SHADER).expect("write shader");
  let builder = EngineBuilder::new()
    .resolution(32, 32)
    .asset_source(DiskSource::new(dir.path()))
    .start_scene(Edits);
  let Some(mut engine) = engine(builder) else {
    return;
  };
  engine.step_frames(2).unwrap();
  assert_eq!(engine.frame().unwrap().get_pixel(8, 8).0, [0, 255, 0, 255]);
  // The file changes, and the registry reloads it as its watcher would
  std::fs::write(&path, BLUE_SHADER).expect("rewrite shader");
  engine.assets().server().reload("edit.wgsl");
  let start = std::time::Instant::now();
  let blue = [0, 0, 255, 255];
  while engine.frame().unwrap().get_pixel(8, 8).0 != blue
    && start.elapsed() < std::time::Duration::from_secs(2)
  {
    engine.step().unwrap();
    std::thread::sleep(std::time::Duration::from_millis(2));
  }
  assert_eq!(engine.frame().unwrap().get_pixel(8, 8).0, blue);
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
      let sprite = ctx.assets().add(Image::solid_rect(4, 4, 220, 30, 40, 255));
      ctx.world.spawn((
        Transform::from_xy(0.0, 10.0),
        Renderable::image(sprite).with_anchor(Anchor::TopLeft),
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
      let sprite = ctx.assets().add(Image::solid_rect(4, 4, 220, 30, 40, 255));
      ctx.world.spawn((
        Transform::from_xy(58.0, 10.0),
        Renderable::image(sprite).with_anchor(Anchor::TopLeft),
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

/// Sprites, a line, world text, a batch sprite and two effects (one of
/// them bloom).
#[derive(Default)]
struct Busy {
  /// The batch sprite's image and the UI's, added on enter
  images: Option<(Handle<Image>, Handle<Image>)>,
}

impl Scene for Busy {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    let sprite = ctx.assets().add(Image::solid_rect(8, 8, 220, 30, 40, 255));
    for i in 0..20 {
      ctx.world.spawn((
        Transform::from_xy(10.0 + i as f32 * 5.0, 30.0),
        Renderable::image(sprite.clone()),
      ));
    }
    let style = TextStyle::new()
      .size(12.0)
      .outline(1.0, Color::BLACK)
      .shadow(Vec2::new(1.0, 1.0), Color::BLACK, 0.0);
    ctx.world.spawn((
      Transform::from_xy(60.0, 75.0),
      Text2d::markup("Busy [wave]text[/wave] 42").with_style(style),
    ));
    ctx.world.spawn((
      Transform::from_xy(0.0, 0.0),
      Line2D::horizontal(50.0, 5.0, 100.0, Color::WHITE, 2.0),
    ));
    // A map over two tilesets, the second layer unlit, on screen
    let first = solid_tiles(ctx, &[RED, [30, 200, 60, 255]]);
    let second = solid_tiles(ctx, &[[30, 60, 230, 255]]);
    let mut map = filled_map(UVec2::new(16, 12), vec![first, second], |cell| {
      (cell.y % 3 == 0).then(|| Tile::new((cell.x % 2) as u16, 0))
    });
    let top = map.add_tile_layer("top");
    if let Some(grid) = map.tiles_mut(top) {
      grid.set(IVec2::new(3, 4), Some(Tile::new(0, 1)));
    }
    map.layers[top].info.lit = false;
    // and an image layer under the tiles, repeated across
    let backdrop = ctx.assets().add(Image::solid_rect(8, 8, 40, 40, 90, 255));
    map.layers.insert(
      0,
      MapLayer {
        info: LayerInfo::new(100, "backdrop"),
        kind: LayerKind::Image(ImageLayer {
          image: backdrop,
          repeat_x: true,
          repeat_y: false,
        }),
      },
    );
    spawn_map(ctx, map, Vec2::new(-4.0, 2.0));
    ctx.effects().set_vignette(0.4);
    ctx.effects().set_bloom(0.6);
    let lut = ctx.assets().add(ColorLut::identity_strip(16));
    let lut = ColorLut::from_sprite(&lut, ctx.world.resource::<Assets<Image>>()).ok();
    let mut look = ctx.look();
    look.grading.lut = lut;
    look.auto_exposure = Some(AutoExposure::default());
    self.images = Some((
      ctx.assets().add(Image::solid_rect(4, 4, 30, 200, 60, 255)),
      ctx.assets().add(Image::solid_rect(4, 4, 255, 255, 0, 255)),
    ));
  }

  fn update(&mut self, _ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    SceneAction::Continue
  }

  fn render(&mut self, gpu: &mut GpuRenderContext) {
    if let Some((sprite, _)) = &self.images {
      let instance = SpriteInstance::new().with_transform(60.0, 60.0, 0.0, 4.0, 4.0);
      gpu.add_sprite(sprite, instance, SpriteDraw::default());
    }
  }

  /// A UI of every kind: a bordered panel, clipped text, a sprite and a line
  fn ui(&mut self, ui: &mut UiContext, _ctx: &mut SceneContext) -> SceneAction {
    let panel = Rect::new(4.0, 4.0, 50.0, 20.0);
    ui.draw_rect_bordered(panel, Color::new(0, 0, 0, 160), 4.0, 1.0, Color::WHITE);
    ui.push_clip(Rect::new(4.0, 4.0, 40.0, 20.0));
    ui.draw_text("UI 42", 8.0, 6.0, Color::WHITE);
    ui.pop_clip();
    if let Some((_, icon)) = &self.images {
      ui.draw_sprite(icon, 60.0, 6.0);
    }
    let points = [Vec2::new(70.0, 6.0), Vec2::new(90.0, 16.0)];
    ui.draw_line(&points, 2.0, Color::WHITE);
    SceneAction::Continue
  }
}

#[test]
fn steady_frames_create_no_bind_groups() {
  use crate::core::graphics::bind_groups::created_on_this_thread;
  let Some(mut engine) = engine(
    EngineBuilder::new()
      .resolution(120, 90)
      .start_scene(Busy::default()),
  ) else {
    return;
  };
  engine.step_frames(3).unwrap();
  assert!(chunks(&engine).chunk_count() > 0, "a map on screen");
  let world = engine.world_mut();
  let image_layers = world.query::<&ImageLayer>().iter(world).count();
  assert_eq!(image_layers, 1, "an image layer on screen");
  let before = created_on_this_thread();
  engine.step_frames(3).unwrap();
  assert_eq!(created_on_this_thread() - before, 0);
}

#[test]
fn a_resize_rebuilds_bind_groups_once() {
  use crate::core::graphics::bind_groups::created_on_this_thread;
  let Some(mut engine) = engine(
    EngineBuilder::new()
      .resolution(120, 90)
      .start_scene(Busy::default()),
  ) else {
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
    Busy::default().on_enter(ctx);
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
      let dot = ctx.assets().add(Image::solid_rect(1, 1, 200, 40, 40, 255));
      for i in 0..2_000usize {
        let (col, row) = ((i % 50) as f32, (i / 50) as f32);
        ctx.world.spawn((
          Transform::from_xy(col * 2.0, row * 2.0),
          Renderable::image(dot.clone()).with_anchor(Anchor::TopLeft),
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
      let red = Image::solid_rect(64, 64, 220, 30, 40, 255);
      let sprite = ctx.assets().add(red);
      ctx.world.spawn((
        Transform::from_xy(16.0, 16.0).with_scale(Vec2::splat(0.125)),
        Renderable::image(sprite).with_filter(Filter::Linear),
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
    renderables.iter(world).next().unwrap().image.clone()
  };
  *AssetsView::new(engine.world_mut())
    .get_mut(&handle)
    .unwrap() = Image::solid_rect(64, 64, 30, 200, 60, 255);
  engine.step_frames(2).unwrap();
  assert_eq!(
    engine.frame().unwrap().get_pixel(16, 16).0,
    [30, 200, 60, 255]
  );
}

#[test]
fn glyph_pages_bind_once_and_reuse_their_glyphs() {
  let Ok((device, queue)) = crate::core::testing::shared_gpu() else {
    eprintln!("skipping: no GPU available");
    return;
  };
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

#[test]
fn text_scaled_to_zero_is_quietly_skipped_and_a_broken_one_warned_once() {
  let scene = Fixture {
    setup: |ctx| {
      // Pop-in tweens start at scale zero
      for i in 0..10 {
        ctx.world.spawn((
          Transform::from_xy(i as f32 * 4.0, 0.0).with_scale(Vec2::ZERO),
          Text2d::new("pop"),
        ));
      }
      ctx.world.spawn((
        Transform::from_xy(0.0, 0.0).with_scale(Vec2::splat(f32::NAN)),
        Text2d::new("broken"),
      ));
    },
  };
  let Some(mut engine) = engine(EngineBuilder::new().resolution(64, 48).start_scene(scene)) else {
    return;
  };
  engine.step_frames(3).unwrap();
  assert_eq!(
    engine.engine.text_warned.len(),
    1,
    "one warning, for the non-finite text alone"
  );
}

#[test]
fn a_glyph_too_big_for_a_page_is_rasterized_once() {
  use std::cell::Cell;
  let Ok((device, queue)) = crate::core::testing::shared_gpu() else {
    eprintln!("skipping: no GPU available");
    return;
  };
  let mut sprites = crate::core::graphics::SpriteGpuRenderer::new(&device, &queue);
  let key = crate::core::text::GlyphKey::fill(crate::core::text::FontId(7), 3, 900.0, 0.0);
  let rasterized = Cell::new(0);
  // Wider than a 1024 px page with its border
  let wide = || {
    rasterized.set(rasterized.get() + 1);
    Some(crate::core::text::GlyphImage {
      format: crate::core::text::GlyphFormat::Mask,
      width: 1100,
      height: 10,
      left: 0,
      top: 10,
      data: vec![255; 11_000],
      base: 900.0,
    })
  };
  let list = crate::core::render::DrawList::default();
  for _ in 0..2 {
    assert!(sprites.glyph(&device, &queue, key, wide).is_none());
    sprites.prepare_textures(&device, &queue, &list, &[]);
  }
  assert_eq!(rasterized.get(), 1, "refused once, then remembered");
}

/// The batch's pipelines build in the scene format and in the UI format
/// (where they gamma-encode, as the UI pipeline does: what the UI line
/// proof compares against)
#[test]
fn the_batch_pipelines_build_in_both_formats() {
  use crate::core::render::batch::{BatchShader, BlendMode, LightStep, Stream};
  use crate::core::render::{SCENE_FORMAT, UI_FORMAT};
  let Ok((device, _queue)) = crate::core::testing::shared_gpu() else {
    eprintln!("skipping: no GPU available");
    return;
  };
  let mut batch =
    crate::core::render::batch::Batch2D::new(&device, SCENE_FORMAT, Default::default());
  let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
  let shaders = [
    (BatchShader::TextMask, Stream::Quads),
    (BatchShader::TextColor, Stream::Quads),
    (BatchShader::TextMsdf, Stream::Quads),
    (BatchShader::Sprite, Stream::Quads),
    (BatchShader::Tile, Stream::Quads),
    (BatchShader::Repeat, Stream::Quads),
    (BatchShader::Line, Stream::Triangles),
    (BatchShader::Light(LightStep::Add), Stream::Quads),
    (BatchShader::Light(LightStep::Shadow), Stream::Triangles),
  ];
  for (shader, stream) in shaders {
    for blend in [BlendMode::Alpha, BlendMode::Premultiplied] {
      for format in [SCENE_FORMAT, UI_FORMAT] {
        assert!(
          batch.ensure_pipeline_for_test(&device, shader, blend, stream, format),
          "{shader:?} {blend:?} {format:?}"
        );
      }
    }
  }
  // A game shader, whose generated entries go through `ui_out` too
  let src = "fn sprite(in: SpriteIn, params: Params) -> vec4<f32> { return sample(in.uv); }";
  let wgsl = crate::core::shaders::custom::compile(
    crate::core::shaders::custom::ShaderKind::Sprite,
    "test.wgsl",
    src,
  )
  .unwrap()
  .wgsl;
  for format in [SCENE_FORMAT, UI_FORMAT] {
    assert!(
      batch.ensure_custom_for_test(&device, &wgsl, BlendMode::Alpha, format),
      "custom {format:?}"
    );
  }
  let error = pollster::block_on(scope.pop());
  assert!(error.is_none(), "{error:?}");
}

// Tile chunks: what they build, upload and draw. Their pixels are checked in
// `tests/e2e/tilemap.rs`

/// A tileset of one solid 8 px tile per colour, in a row
fn solid_tiles(ctx: &mut SceneContext, colours: &[[u8; 4]]) -> Tileset {
  tile_row(ctx, 8, colours)
}

/// A tileset of one solid `side` px tile per colour, in a row
fn tile_row(ctx: &mut SceneContext, side: u32, colours: &[[u8; 4]]) -> Tileset {
  let count = colours.len() as u32;
  let (width, height) = (side * count, side);
  let mut data = Vec::with_capacity((width * height * 4) as usize);
  for _ in 0..height {
    for x in 0..width {
      data.extend_from_slice(&colours[(x / side) as usize]);
    }
  }
  let image = ctx.assets().add(Image::from_rgba(data, width, height));
  let size = UVec2::new(width, height);
  let mut set = Tileset::new(
    "tiles",
    UVec2::splat(side),
    TilesetImage::Atlas { image, size },
  );
  set.columns = count;
  set.tile_count = count;
  set
}

/// A map of `cells` 8 px cells over `tilesets` with a tile layer "ground"
/// holding `tile(cell)` in every cell
fn filled_map(
  cells: UVec2,
  tilesets: Vec<Tileset>,
  tile: impl Fn(IVec2) -> Option<Tile>,
) -> TileMap {
  let mut map = TileMap::new(cells, UVec2::splat(8));
  for set in tilesets {
    map.add_tileset(TilesetSource::Embedded(std::sync::Arc::new(set)));
  }
  let layer = map.add_tile_layer("ground");
  if let Some(grid) = map.tiles_mut(layer) {
    for y in 0..cells.y as i32 {
      for x in 0..cells.x as i32 {
        let cell = IVec2::new(x, y);
        grid.set(cell, tile(cell));
      }
    }
  }
  map
}

fn spawn_map(ctx: &mut SceneContext, map: TileMap, at: Vec2) -> Entity {
  let handle = ctx.assets().add(map);
  ctx
    .world
    .spawn((MapRoot::new(handle), Transform::from_translation(at)))
    .id()
}

/// The world's one map entity
fn the_map(engine: &mut HeadlessEngine) -> Entity {
  let world = engine.world_mut();
  let maps: Vec<Entity> = world
    .query_filtered::<Entity, With<MapRoot>>()
    .iter(world)
    .collect();
  assert_eq!(maps.len(), 1, "one map");
  maps[0]
}

fn chunks(engine: &HeadlessEngine) -> &crate::core::render::batch::ChunkStore {
  engine.engine.batch.as_ref().unwrap().chunks()
}

/// A pixel-art engine `size` px square over `setup`, read back exactly
fn tile_engine(size: u32, setup: fn(&mut SceneContext)) -> Option<HeadlessEngine> {
  struct Tiles(fn(&mut SceneContext));
  impl Scene for Tiles {
    fn on_enter(&mut self, ctx: &mut SceneContext) {
      ctx.look().tonemap = Tonemap::Clamp;
      (self.0)(ctx);
    }
  }
  let mut engine = engine(
    EngineBuilder::new()
      .pixel_art(size, size)
      .start_scene(Tiles(setup)),
  )?;
  // A map lands a frame or so after its asset is ready
  crate::core::tilemap::test_util::step_until_maps_land(&mut engine);
  Some(engine)
}

#[test]
fn a_moving_map_draws_where_it_is_without_rebuilding_chunks() {
  let Some(mut engine) = tile_engine(64, |ctx| {
    let set = solid_tiles(ctx, &[RED]);
    let map = filled_map(UVec2::splat(8), vec![set], |cell| {
      (cell == IVec2::ZERO).then_some(Tile::new(0, 0))
    });
    spawn_map(ctx, map, Vec2::ZERO);
  }) else {
    return;
  };
  engine.step_frames(2).unwrap();
  assert_eq!(engine.frame().unwrap().get_pixel(4, 4).0, RED);
  let built = chunks(&engine).rebuilds();
  assert_eq!(built, 1, "one chunk built");
  let map = the_map(&mut engine);
  for step in 1..=4 {
    let at = Vec2::new(9.0 * step as f32, 5.0 * step as f32);
    engine
      .world_mut()
      .get_mut::<Transform>(map)
      .unwrap()
      .translation = at;
    engine.step().unwrap();
    let frame = engine.frame().unwrap();
    let (x, y) = (at.x as u32, at.y as u32);
    assert_eq!(frame.get_pixel(x + 4, y + 4).0, RED, "drawn at {at}");
    assert_ne!(frame.get_pixel(x - 1, y + 4).0, RED, "not left of {at}");
    assert_eq!(chunks(&engine).rebuilds(), built, "rebuilt at {at}");
  }
}

#[test]
fn an_interpolated_map_draws_between_its_last_two_steps() {
  use std::time::Duration;
  /// Moves its map 20 px right every fixed step
  struct Mover;
  impl Scene for Mover {
    fn on_enter(&mut self, ctx: &mut SceneContext) {
      ctx.look().tonemap = Tonemap::Clamp;
      let set = solid_tiles(ctx, &[RED]);
      let map = filled_map(UVec2::splat(4), vec![set], |cell| {
        (cell == IVec2::ZERO).then_some(Tile::new(0, 0))
      });
      let map = spawn_map(ctx, map, Vec2::new(0.0, 8.0));
      ctx.world.entity_mut(map).insert(Interpolated);
    }
    fn update(&mut self, ctx: &mut SceneContext, _dt: f32) -> SceneAction {
      // From the frame it lands in
      let mut maps = ctx
        .world
        .query_filtered::<(Entity, &mut Transform), With<MapRoot>>();
      let roots: Vec<Entity> = maps.iter(ctx.world).map(|(map, _)| map).collect();
      for map in roots {
        if MapsView::new(ctx.world).is_ready(map)
          && let Some(mut transform) = ctx.world.get_mut::<Transform>(map)
        {
          transform.translation.x += 20.0;
        }
      }
      SceneAction::Continue
    }
  }
  let builder = EngineBuilder::new()
    .pixel_art(120, 30)
    .fixed_hz(50)
    .start_scene(Mover);
  let Some(mut engine) = engine(builder) else {
    return;
  };
  // The frame it lands in moves it to x = 20; one more step: x = 40, was 20
  crate::core::tilemap::test_util::step_until_maps_land(&mut engine);
  engine.step_frames(1).unwrap();
  // One more step (to 60, from 40) and half a step left over: x = 50
  engine.engine.frame(Some(Duration::from_millis(30)));
  let frame = engine.frame().unwrap();
  assert_eq!(frame.get_pixel(51, 11).0, RED, "drawn at x = 50");
  assert_ne!(frame.get_pixel(61, 11).0, RED, "not at the stepped x = 60");
}

/// A 128 x 128 map (4 x 4 chunks of 256 px) full of tiles from two
/// tilesets in a checkerboard, with its corner up and left of the view so
/// the view's 64 px meet the corners of four chunks
fn big_map(ctx: &mut SceneContext) {
  let first = solid_tiles(ctx, &[RED, [240, 220, 20, 255]]);
  let second = solid_tiles(ctx, &[[30, 60, 230, 255]]);
  let map = filled_map(UVec2::splat(128), vec![first, second], |cell| {
    Some(Tile::new(((cell.x + cell.y) % 2) as u16, 0))
  });
  spawn_map(ctx, map, Vec2::splat(-224.0));
}

#[test]
fn only_visible_chunks_draw() {
  let Some(mut engine) = tile_engine(64, big_map) else {
    return;
  };
  engine.step_frames(2).unwrap();
  // Four chunks, each a draw per tileset
  assert_eq!(engine.engine.debug_stats().draw_calls, 8);
  assert_eq!(
    chunks(&engine).chunk_count(),
    4,
    "only those seen are built"
  );
  // Inside chunk (1, 1) alone
  let map = the_map(&mut engine);
  engine
    .world_mut()
    .get_mut::<Transform>(map)
    .unwrap()
    .translation = Vec2::splat(-300.0);
  engine.step().unwrap();
  assert_eq!(engine.engine.debug_stats().draw_calls, 2);
  assert_eq!(chunks(&engine).rebuilds(), 4, "nothing built again");
}

#[test]
fn a_view_past_the_chunk_arena_s_cap_draws_what_fits_and_warns_once() {
  // The 64 px view meets the corners of four full chunks of 1,024 tiles
  // each; the arena may hold 2,500 instances (the GPU's largest buffer is
  // about two million), so two chunks fit and the other two do not
  const YELLOW: [u8; 4] = [240, 220, 20, 255];
  const BLUE: [u8; 4] = [30, 60, 230, 255];
  let builder = EngineBuilder::new().pixel_art(64, 64).start_scene(Fixture {
    setup: |ctx| {
      ctx.look().tonemap = Tonemap::Clamp;
      let first = solid_tiles(ctx, &[RED, YELLOW]);
      let second = solid_tiles(ctx, &[BLUE]);
      let map = filled_map(UVec2::splat(64), vec![first, second], |cell| {
        Some(Tile::new(((cell.x + cell.y) % 2) as u16, 0))
      });
      // Chunks of 256 px: the four meet at the view's centre, (32, 32)
      spawn_map(ctx, map, Vec2::splat(-224.0));
    },
  });
  let Some(mut engine) = engine(builder) else {
    return;
  };
  engine
    .engine
    .batch
    .as_mut()
    .unwrap()
    .chunks_mut()
    .set_cap(2500);
  crate::core::tilemap::test_util::step_until_maps_land(&mut engine);
  for _ in 0..4 {
    engine.step().expect("the game goes on");
  }
  let store = chunks(&engine);
  assert_eq!(store.chunk_count(), 2, "the chunks that fit are built");
  assert!(store.instances_in_use() <= 2500, "within the cap");
  assert_eq!(store.full_warnings(), 1, "warned once");
  let batch = engine.engine.batch.as_ref().unwrap();
  assert!(
    batch.chunk_buffer_capacity() <= 2500,
    "the GPU buffer stays within the cap: {}",
    batch.chunk_buffer_capacity()
  );
  // The first two chunks met (row by row) are the top half of the view
  let frame = engine.frame().unwrap();
  let tile = |p: [u8; 4]| [RED, YELLOW, BLUE].contains(&p);
  for x in [4, 28, 36, 60] {
    assert!(tile(frame.get_pixel(x, 8).0), "({x}, 8) drawn");
    assert!(!tile(frame.get_pixel(x, 56).0), "({x}, 56) not drawn");
  }
}

#[test]
fn an_edit_rebuilds_one_chunk() {
  let Some(mut engine) = tile_engine(64, big_map) else {
    return;
  };
  engine.step_frames(2).unwrap();
  let built = chunks(&engine).rebuilds();
  let map = the_map(&mut engine);
  // Cell (30, 30) is in chunk (0, 0), at (16, 16) on screen
  let yellow = Tile::new(0, 1);
  let mut maps = MapsView::new(engine.world_mut());
  assert!(maps.set_tile(map, "ground", IVec2::new(30, 30), Some(yellow)));
  engine.step().unwrap();
  assert_eq!(chunks(&engine).rebuilds(), built + 1);
  assert_eq!(
    engine.frame().unwrap().get_pixel(20, 20).0,
    [240, 220, 20, 255]
  );
  // A chunk nothing draws is not built for an edit
  let mut maps = MapsView::new(engine.world_mut());
  assert!(maps.set_tile(map, "ground", IVec2::new(100, 100), Some(yellow)));
  engine.step_frames(2).unwrap();
  assert_eq!(chunks(&engine).rebuilds(), built + 1);
}

#[test]
fn a_despawned_map_frees_its_chunks() {
  let Some(mut engine) = tile_engine(64, big_map) else {
    return;
  };
  engine.step_frames(2).unwrap();
  assert!(chunks(&engine).instances_in_use() > 0);
  assert!(engine.engine.batch.as_ref().unwrap().has_chunk_buffer());
  assert_eq!(
    crate::core::render::tiles::TileDrawer::padded_count(engine.world()),
    2
  );
  let map = the_map(&mut engine);
  engine.world_mut().despawn(map);
  engine.step().unwrap();
  assert_eq!(chunks(&engine).chunk_count(), 0);
  assert_eq!(chunks(&engine).instances_in_use(), 0);
  assert!(
    !engine.engine.batch.as_ref().unwrap().has_chunk_buffer(),
    "the arena's buffer goes"
  );
  // The padded tilesets are kept a while, as a map of them may come back
  // (a level restarted), then go
  assert_eq!(
    crate::core::render::tiles::TileDrawer::padded_count(engine.world()),
    2,
    "kept a while"
  );
  engine.step_frames(120).unwrap();
  assert_eq!(
    crate::core::render::tiles::TileDrawer::padded_count(engine.world()),
    0,
    "the padded tilesets go"
  );
}

#[test]
fn a_large_tileset_draws_from_its_own_texture() {
  // 100 tiles of 16 px pad into 32 px cells, ten a row, with 24 px of
  // margin on each side: 368 px, over the atlas's 256 px, so the padded
  // image is a texture of its own (mips too)
  let Some(mut engine) = tile_engine(64, |ctx| {
    let colours: Vec<[u8; 4]> = (0..100u8)
      .map(|i| [i * 2 + 20, 250 - i * 2, 90, 255])
      .collect();
    let set = tile_row(ctx, 16, &colours);
    let mut map = TileMap::new(UVec2::splat(4), UVec2::splat(16));
    map.add_tileset(TilesetSource::Embedded(std::sync::Arc::new(set)));
    let layer = map.add_tile_layer("ground");
    if let Some(grid) = map.tiles_mut(layer) {
      grid.set(IVec2::new(0, 0), Some(Tile::new(0, 0)));
      grid.set(IVec2::new(1, 0), Some(Tile::new(0, 57)));
      grid.set(IVec2::new(2, 1), Some(Tile::new(0, 99)));
    }
    spawn_map(ctx, map, Vec2::ZERO);
  }) else {
    return;
  };
  engine.step_frames(2).unwrap();
  let frame = engine.frame().unwrap();
  let colour = |i: u8| [i * 2 + 20, 250 - i * 2, 90, 255];
  assert_eq!(frame.get_pixel(8, 8).0, colour(0));
  assert_eq!(frame.get_pixel(24, 8).0, colour(57));
  assert_eq!(frame.get_pixel(40, 24).0, colour(99));
  let gpu = engine.engine.sprite_gpu.as_ref().unwrap();
  assert_eq!(gpu.standalone_entity_count(), 1, "a texture of its own");
}

#[test]
fn two_cameras_each_draw_their_own_view_of_a_map() {
  // One 128 x 128 map (4 x 4 chunks of 256 px) seen by two cameras side by
  // side, each 32 px square: the main one on the left at zoom 1 over
  // (16, 16) on render layers 0 and 1, another on the right at zoom 2 over
  // (300, 16) on layer 0 alone. The ground has a red tile in each view; the
  // "near" layer, put on render layer 1, a green one in each (the right
  // camera does not draw its); the "clouds" layer (parallax 0.5) a blue one
  // that each camera's own parallax brings into its view, from a chunk its
  // unshifted view does not meet
  const GREEN: [u8; 4] = [40, 200, 60, 255];
  const BLUE: [u8; 4] = [30, 60, 230, 255];
  let builder = EngineBuilder::new().pixel_art(64, 32).start_scene(Fixture {
    setup: |ctx| {
      ctx.look().tonemap = Tonemap::Clamp;
      let set = solid_tiles(ctx, &[RED, GREEN, BLUE]);
      let mut map = TileMap::new(UVec2::splat(128), UVec2::splat(8));
      map.add_tileset(TilesetSource::Embedded(std::sync::Arc::new(set)));
      let cells = [
        ("ground", [(1, 1), (37, 1)], 0),
        ("near", [(2, 2), (38, 2)], 1),
        ("clouds", [(0, 2), (18, 1)], 2),
      ];
      for (name, at, tile) in cells {
        let layer = map.add_tile_layer(name);
        let grid = map.tiles_mut(layer).expect("tile layer");
        for (x, y) in at {
          grid.set(IVec2::new(x, y), Some(Tile::new(0, tile)));
        }
      }
      map.layers[2].info.parallax = Vec2::splat(0.5);
      spawn_map(ctx, map, Vec2::ZERO);
      ctx.camera().set_position(16.0, 16.0);
      ctx.camera().set_viewport(Viewport::new(0.0, 0.0, 0.5, 1.0));
      let main = ctx
        .world
        .query_filtered::<Entity, With<MainCamera>>()
        .single(ctx.world)
        .expect("a main camera");
      let mut main = ctx.world.entity_mut(main);
      main.insert(RenderLayers::layer(0).with(1));
      // One world unit a pixel, as the other camera at zoom 1
      if let Some(mut camera) = main.get_mut::<Camera2d>() {
        camera.projection = Projection::ScreenUnits;
      }
      ctx.world.spawn((
        Transform::from_xy(300.0, 16.0),
        Camera2d {
          zoom: 2.0,
          viewport: Viewport::new(0.5, 0.0, 0.5, 1.0),
          order: 1,
          ..Default::default()
        },
        RenderLayers::layer(0),
      ));
    },
  });
  let Some(mut engine) = engine(builder) else {
    return;
  };
  crate::core::tilemap::test_util::step_until_maps_land(&mut engine);
  engine.step_frames(2).unwrap();
  let map = the_map(&mut engine);
  let near = MapsView::new(engine.world_mut())
    .layer_entity(map, "near")
    .expect("near");
  engine
    .world_mut()
    .entity_mut(near)
    .insert(RenderLayers::layer(1));
  engine.step().unwrap();

  let frame = engine.frame().unwrap();
  let background = frame.get_pixel(0, 0).0;
  let boxes = [
    // Left: the ground, the near tile, the clouds moved by (8, 8)
    ((8, 8), (16, 16), RED),
    ((16, 16), (24, 24), GREEN),
    ((8, 24), (16, 32), BLUE),
    // Right, twice the size: the ground, the clouds moved by (150, 8)
    ((40, 0), (56, 16), RED),
    ((36, 16), (52, 32), BLUE),
  ];
  for y in 0..32 {
    for x in 0..64 {
      let want = boxes
        .iter()
        .find(|((x0, y0), (x1, y1), _)| (*x0..*x1).contains(&x) && (*y0..*y1).contains(&y))
        .map_or(background, |b| b.2);
      assert_eq!(frame.get_pixel(x, y).0, want, "pixel ({x}, {y})");
    }
  }

  // Each camera drew the chunks it sees: the left three layers' chunk
  // (0, 0); the right the ground's chunk (1, 0) and the clouds' (0, 0)
  let world = engine.world_mut();
  let mut cameras: Vec<(i32, Entity)> = world
    .query::<(Entity, &Camera2d)>()
    .iter(world)
    .map(|(entity, camera)| (camera.order, entity))
    .collect();
  cameras.sort();
  let batch = engine.engine.batch.as_ref().unwrap();
  let chunk_draws = |camera: Entity| {
    batch
      .draws(camera, true)
      .iter()
      .filter(|draw| {
        matches!(
          draw.source,
          crate::core::render::batch::DrawSource::Chunk { .. }
        )
      })
      .count()
  };
  assert_eq!(chunk_draws(cameras[0].1), 3, "the left camera's chunks");
  assert_eq!(chunk_draws(cameras[1].1), 2, "the right camera's chunks");
}

#[test]
fn a_tiny_image_too_small_to_repeat_warns_once() {
  // A 1 px image repeated both ways under a camera at zoom 0.01 over a 1024
  // px wide target: the view is 102,400 px across, more than the 65,536
  // copies an image repeats at most. The layer is warned about once, and
  // the copies cover the view's middle, as far on the left as on the right
  let builder = EngineBuilder::new()
    .pixel_art(1024, 64)
    .start_scene(Fixture {
      setup: |ctx| {
        ctx.look().tonemap = Tonemap::Clamp;
        let image = ctx.assets().add(Image::solid_rect(1, 1, 220, 30, 40, 255));
        let mut map = TileMap::new(UVec2::ONE, UVec2::splat(8));
        let mut info = LayerInfo::new(1, "dots");
        info.collides = false;
        map.layers.push(MapLayer {
          info,
          kind: LayerKind::Image(ImageLayer {
            image,
            repeat_x: true,
            repeat_y: true,
          }),
        });
        spawn_map(ctx, map, Vec2::ZERO);
        ctx.camera().set_zoom(0.01);
      },
    });
  let Some(mut engine) = engine(builder) else {
    return;
  };
  crate::core::tilemap::test_util::step_until_maps_land(&mut engine);
  engine.step_frames(4).unwrap();
  assert_eq!(engine.engine.tile_drawer.too_small_warnings(), 1);
  let frame = engine.frame().unwrap();
  let row: Vec<bool> = (0..1024).map(|x| frame.get_pixel(x, 32).0 == RED).collect();
  let covered = row.iter().filter(|&&c| c).count();
  // 65,536 px at zoom 0.01: about 655 of the 1024 pixels
  assert!((650..=660).contains(&covered), "{covered} pixels covered");
  let left = row.iter().position(|&c| c).expect("covered");
  let right = 1024 - 1 - row.iter().rposition(|&c| c).expect("covered");
  assert!(
    left.abs_diff(right) <= 1,
    "{left} px bare on the left, {right} on the right"
  );
  assert!(row[left..1024 - right].iter().all(|&c| c), "one run");
  // Down, the view is 6,400 px: covered whole
  assert!((0..64).all(|y| frame.get_pixel(512, y).0 == RED));
}
