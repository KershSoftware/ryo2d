//! The GPU world path draws opaque, axis-aligned entities byte-for-byte like
//! the CPU renderer.
//!
//! Each scene runs on two engines, one on the CPU entity path and one on the
//! GPU draw list, and the frames are compared pixel for pixel after every
//! step. Only translucent overlaps, rotation edges and fractional scales may
//! differ between the paths, so none of these scenes use them.

use super::HeadlessEngine;
use crate::core::engine::EngineBuilder;
use crate::prelude::*;

const WIDTH: u32 = 160;
const HEIGHT: u32 = 120;
const STEPS: u32 = 3;

/// A scene that spawns its entities in `on_enter` and then idles.
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

/// An opaque sprite whose every pixel differs, so sampling mistakes show.
fn pattern(name: &str, width: u32, height: u32, seed: u32) -> SpriteHandle {
  let mut data = Vec::with_capacity((width * height * 4) as usize);
  for y in 0..height {
    for x in 0..width {
      let n = x * 31 + y * 17 + seed * 53;
      data.extend_from_slice(&[
        (n * 7 % 256) as u8,
        (n * 13 % 256) as u8,
        (n * 29 % 256) as u8,
        255,
      ]);
    }
  }
  SpriteHandle::from_sprite(Sprite::from_rgba(data, width, height), name)
}

/// Run `setup` on both paths and compare the frames after every step.
fn assert_paths_match(name: &str, setup: fn(&mut SceneContext)) {
  let engine = || {
    HeadlessEngine::new_or_skip(
      EngineBuilder::new()
        .resolution(WIDTH, HEIGHT)
        .start_scene(Fixture { setup }),
    )
  };
  let (Some(mut cpu), Some(mut gpu)) = (engine(), engine()) else {
    return;
  };
  gpu.engine.use_gpu_world();
  assert!(gpu.engine.render_pipeline.contains("world"));

  for step in 1..=STEPS {
    cpu.step().unwrap();
    gpu.step().unwrap();
    let (cpu_frame, gpu_frame) = (cpu.frame().unwrap(), gpu.frame().unwrap());
    let differing = cpu_frame
      .pixels()
      .zip(gpu_frame.pixels())
      .filter(|(a, b)| a != b)
      .count();
    eprintln!("{name} step {step}: {differing} differing pixels");
    let background = *cpu_frame.get_pixel(0, 0);
    assert!(
      cpu_frame.pixels().any(|p| *p != background),
      "{name}: the scene drew nothing"
    );
    assert!(
      gpu.engine.draw_list.sprite_count() > 0,
      "{name}: the GPU engine drew nothing through the world pass"
    );
    assert_eq!(differing, 0, "{name}: frames differ at step {step}");
  }
}

fn spawn(ctx: &mut SceneContext, x: f32, y: f32, renderable: Renderable) -> Entity {
  ctx.world.spawn((Transform::new(x, y), renderable))
}

#[test]
fn opaque_sprites_at_several_z() {
  assert_paths_match("opaque_sprites_at_several_z", |ctx| {
    let a = pattern("cmp-a", 24, 16, 1);
    let b = pattern("cmp-b", 20, 20, 2);
    let c = pattern("cmp-c", 12, 30, 3);
    // Spawned out of z order, overlapping
    spawn(ctx, 40.0, 30.0, Renderable::sprite(b).with_z_order(5));
    spawn(ctx, 30.0, 25.0, Renderable::sprite(a).with_z_order(0));
    spawn(ctx, 50.0, 20.0, Renderable::sprite(c).with_z_order(-3));
    spawn(
      ctx,
      100.0,
      60.0,
      Renderable::sprite(pattern("cmp-d", 9, 7, 4)),
    );
  });
}

#[test]
fn tiled_horizontal_and_vertical() {
  assert_paths_match("tiled_horizontal_and_vertical", |ctx| {
    let tile = pattern("cmp-tile", 8, 6, 5);
    let right = Renderable::tiled(tile.clone(), TileDirection::Right, 30.0);
    let left = Renderable::tiled(tile.clone(), TileDirection::Left, 20.0);
    let down = Renderable::tiled(tile.clone(), TileDirection::Down, 25.0);
    let up = Renderable::tiled(tile, TileDirection::Up, 18.0);
    spawn(ctx, 10.0, 10.0, right);
    spawn(ctx, 100.0, 40.0, left);
    spawn(ctx, 20.0, 40.0, down);
    spawn(ctx, 130.0, 100.0, up);
  });
}

#[test]
fn composite_tile_parts() {
  // The CPU draws composites after every entity; here the composite shares
  // the entity's z (0), so the draw list orders it after the entity too.
  assert_paths_match("composite_tile_parts", |ctx| {
    let cap = pattern("cmp-cap", 16, 8, 6);
    let body = pattern("cmp-body", 16, 4, 7);
    let composite = CompositeTile::new()
      .add_part(TilePart::static_part(cap, 0.0, 0.0))
      .add_part(TilePart::tiled_part(body, 0.0, 8.0, TileDirection::Down, 30.0).with_flip_x(true));
    ctx.world.spawn((Transform::new(60.0, 20.0), composite));
    spawn(
      ctx,
      55.0,
      30.0,
      Renderable::sprite(pattern("cmp-under", 30, 12, 8)),
    );
  });
}

#[test]
fn drop_shadow_without_blur() {
  assert_paths_match("drop_shadow_without_blur", |ctx| {
    let shadow = DropShadow::new()
      .offset(5.0, 4.0)
      .color(Color::new(20, 30, 40, 255));
    spawn(
      ctx,
      40.0,
      30.0,
      Renderable::sprite(pattern("cmp-shadowed", 20, 14, 9)).with_drop_shadow(shadow),
    );
  });
}

#[test]
fn tinted_opaque_sprites() {
  assert_paths_match("tinted_opaque_sprites", |ctx| {
    let sprite = pattern("cmp-tinted", 16, 16, 10);
    spawn(
      ctx,
      20.0,
      20.0,
      Renderable::sprite(sprite.clone()).with_tint(Color::new(200, 100, 50, 255)),
    );
    spawn(
      ctx,
      60.0,
      20.0,
      Renderable::sprite(sprite.clone()).with_tint(Color::new(17, 255, 233, 255)),
    );
    spawn(ctx, 100.0, 20.0, Renderable::sprite(sprite));
  });
}

#[test]
fn flipped_sprites() {
  assert_paths_match("flipped_sprites", |ctx| {
    let sprite = pattern("cmp-flip", 14, 10, 11);
    let at = |x: f32, fx: bool, fy: bool| {
      (
        x,
        Renderable::sprite(sprite.clone())
          .with_flip_x(fx)
          .with_flip_y(fy),
      )
    };
    for (x, r) in [
      at(10.0, true, false),
      at(40.0, false, true),
      at(70.0, true, true),
      at(100.0, false, false),
    ] {
      spawn(ctx, x, 50.0, r);
    }
  });
}

#[test]
fn integer_scaled_sprites() {
  assert_paths_match("integer_scaled_sprites", |ctx| {
    let sprite = pattern("cmp-scaled", 10, 8, 12);
    ctx.world.spawn((
      Transform::new(10.0, 10.0).with_uniform_scale(2.0),
      Renderable::sprite(sprite.clone()),
    ));
    ctx.world.spawn((
      Transform::new(60.0, 10.0).with_uniform_scale(3.0),
      Renderable::sprite(sprite.clone()).with_flip_x(true),
    ));
    ctx.world.spawn((
      Transform::new(100.0, 60.0).with_scale(2.0, 1.0),
      Renderable::sprite(sprite),
    ));
  });
}

#[test]
fn camera_offset_and_zoom_2x() {
  assert_paths_match("camera_offset_and_zoom_2x", |ctx| {
    ctx.camera.x = 70.0;
    ctx.camera.y = 50.0;
    ctx.camera.zoom = 2.0;
    spawn(
      ctx,
      60.0,
      40.0,
      Renderable::sprite(pattern("cmp-zoom-a", 12, 9, 13)),
    );
    spawn(
      ctx,
      80.0,
      55.0,
      Renderable::sprite(pattern("cmp-zoom-b", 7, 7, 14)).with_z_order(2),
    );
    spawn(
      ctx,
      40.0,
      50.0,
      Renderable::tiled(pattern("cmp-zoom-t", 5, 5, 15), TileDirection::Right, 20.0),
    );
  });
}

#[test]
fn pivot_and_y_sort_order() {
  assert_paths_match("pivot_and_y_sort_order", |ctx| {
    // Centre pivots on even sizes land on whole pixels
    ctx.world.spawn((
      Transform::new(40.0, 40.0).with_pivot(0.5, 0.5),
      Renderable::sprite(pattern("cmp-pivot", 20, 12, 16)),
    ));
    // Same z, overlapping: the lower one (larger y) draws on top whatever
    // the spawn order
    for (x, y, seed) in [(90.0, 60.0, 17), (96.0, 52.0, 18), (84.0, 56.0, 19)] {
      ctx.world.spawn((
        Transform::new(x, y),
        Renderable::sprite(pattern(&format!("cmp-ysort-{seed}"), 16, 16, seed)),
        YSort::new(),
      ));
    }
  });
}

/// Rotation is one of the documented fidelity exceptions: the CPU samples
/// bilinearly and drops the sprite's last row and column, so edges differ.
/// The rotated sprite must still cover about the same area in the same place.
#[test]
fn rotated_sprite_covers_the_same_area() {
  let engine = || {
    HeadlessEngine::new_or_skip(EngineBuilder::new().resolution(WIDTH, HEIGHT).start_scene(
      Fixture {
        setup: |ctx| {
          ctx.world.spawn((
            Transform::new(60.0, 40.0).with_rotation(std::f32::consts::FRAC_PI_4),
            Renderable::sprite(pattern("cmp-rotated", 40, 24, 20)),
          ));
        },
      },
    ))
  };
  let (Some(mut cpu), Some(mut gpu)) = (engine(), engine()) else {
    return;
  };
  gpu.engine.use_gpu_world();
  cpu.step_frames(2).unwrap();
  gpu.step_frames(2).unwrap();
  let (cpu_frame, gpu_frame) = (cpu.frame().unwrap(), gpu.frame().unwrap());
  let background = *cpu_frame.get_pixel(0, 0);
  let covered = |frame: &image::RgbaImage| frame.pixels().filter(|p| **p != background).count();
  let (cpu_area, gpu_area) = (covered(&cpu_frame), covered(&gpu_frame));
  eprintln!("rotated sprite: CPU covers {cpu_area} pixels, GPU {gpu_area}");
  assert!(cpu_area > 400);
  assert!(
    cpu_area.abs_diff(gpu_area) * 10 < cpu_area,
    "areas differ by more than 10% (the CPU drops the last row and column, about 6%): {cpu_area} vs {gpu_area}"
  );
}

/// The bounding box of everything that is not the background.
fn content_bounds(frame: &image::RgbaImage) -> Option<(u32, u32, u32, u32)> {
  let background = *frame.get_pixel(0, 0);
  let mut bounds: Option<(u32, u32, u32, u32)> = None;
  for (x, y, pixel) in frame.enumerate_pixels() {
    if *pixel != background {
      let (x0, y0, x1, y1) = bounds.unwrap_or((x, y, x, y));
      bounds = Some((x0.min(x), y0.min(y), x1.max(x), y1.max(y)));
    }
  }
  bounds
}

/// An outline or glow grows the sprite; both paths must place the grown
/// image on the same pixels (their contents may differ).
#[test]
fn effect_growth_has_the_cpu_bounds() {
  type MakeEffect = fn() -> SpriteShader;
  let effects: [(&str, MakeEffect); 3] = [
    ("outline", || {
      SpriteShader::BuiltIn(SpriteEffect::outline(Color::new(255, 255, 255, 255), 2))
    }),
    ("outline3", || {
      SpriteShader::BuiltIn(SpriteEffect::outline(Color::new(255, 255, 255, 255), 3))
    }),
    ("glow", || {
      SpriteShader::BuiltIn(SpriteEffect::glow(Color::new(255, 255, 255, 255), 3.0))
    }),
  ];
  for (effect_name, effect) in effects {
    for scale in [1.0_f32, 2.0, 1.5] {
      let builder = || {
        EngineBuilder::new()
          .resolution(WIDTH, HEIGHT)
          .start_scene(EffectScene {
            scale,
            effect: effect(),
          })
      };
      let (Some(mut cpu), Some(mut gpu)) = (
        HeadlessEngine::new_or_skip(builder()),
        HeadlessEngine::new_or_skip(builder()),
      ) else {
        return;
      };
      gpu.engine.use_gpu_world();
      cpu.step_frames(2).unwrap();
      gpu.step_frames(2).unwrap();
      let cpu_bounds = content_bounds(&cpu.frame().unwrap());
      let gpu_bounds = content_bounds(&gpu.frame().unwrap());
      eprintln!("{effect_name} x{scale}: CPU {cpu_bounds:?}, GPU {gpu_bounds:?}");
      let (Some(cpu_box), Some(gpu_box)) = (cpu_bounds, gpu_bounds) else {
        panic!("{effect_name} at scale {scale}: nothing drawn");
      };
      if effect_name == "glow" {
        // The two glows fade differently, so the CPU's faint outer ring can
        // be invisible where the GPU's shows: the GPU box holds the CPU's and
        // stays within the grown image (3 px of padding, scaled, plus rounding)
        let slack = (3.0 * scale).ceil() as u32 + 1;
        assert!(
          gpu_box.0 <= cpu_box.0
            && gpu_box.1 <= cpu_box.1
            && gpu_box.2 >= cpu_box.2
            && gpu_box.3 >= cpu_box.3,
          "glow at scale {scale}: {gpu_box:?} does not hold {cpu_box:?}"
        );
        assert!(
          cpu_box.0 - gpu_box.0 <= slack
            && gpu_box.2 - cpu_box.2 <= slack
            && cpu_box.1 - gpu_box.1 <= slack
            && gpu_box.3 - cpu_box.3 <= slack,
          "glow at scale {scale}: {gpu_box:?} too far past {cpu_box:?}"
        );
      } else {
        assert_eq!(cpu_box, gpu_box, "{effect_name} at scale {scale}");
      }
    }
  }
}

struct EffectScene {
  scale: f32,
  effect: SpriteShader,
}

impl Scene for EffectScene {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    ctx.world.spawn((
      Transform::new(50.0, 40.0).with_uniform_scale(self.scale),
      Renderable::sprite(pattern("cmp-effect", 11, 9, 21)).with_shader(self.effect.clone()),
    ));
  }

  fn update(&mut self, _ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    SceneAction::Continue
  }
}
