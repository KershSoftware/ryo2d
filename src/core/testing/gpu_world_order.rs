//! On the GPU world path, a line at z 1 draws between sprites at z 0 and z 2.

use super::HeadlessEngine;
use crate::core::engine::EngineBuilder;
use crate::prelude::*;

const RED: [u8; 4] = [220, 30, 40, 255];
const BLUE: [u8; 4] = [30, 60, 230, 255];

/// Whether a pixel is the line's green (the line is slightly translucent, so
/// it is compared by channel dominance, not exactly).
fn is_line(pixel: [u8; 4]) -> bool {
  pixel[1] > 200 && pixel[2] < 100
}

/// Two overlapping sprites (z 0 red, z 2 blue) and a line (z 1) across both.
struct Layers {
  red: SpriteHandle,
  blue: SpriteHandle,
}

impl Scene for Layers {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    let line = Line2D::horizontal(32.0, 0.0, 64.0, Color::new(0, 255, 0, 255), 4.0).with_z_order(1);
    ctx.world.spawn((Transform::new(0.0, 0.0), line));
  }

  fn update(&mut self, _ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    SceneAction::Continue
  }

  fn render_gpu(&mut self, gpu: &mut GpuRenderContext) {
    gpu.upload_sprite(&self.red);
    gpu.upload_sprite(&self.blue);
    let at = |x: f32| SpriteInstance::new().with_transform(x, 32.0, 0.0, 20.0, 20.0);
    // Submitted blue first: z decides, not submission order
    gpu.add_sprite_instance(&self.blue.id(), at(36.0), 2, false);
    gpu.add_sprite_instance(&self.red.id(), at(28.0), 0, false);
  }
}

#[test]
fn trail_between_gpu_sprites() {
  let solid = |name: &str, c: [u8; 4]| {
    SpriteHandle::from_sprite(Sprite::solid_rect(4, 4, c[0], c[1], c[2], c[3]), name)
  };
  let scene = Layers {
    red: solid("world-red", RED),
    blue: solid("world-blue", BLUE),
  };
  let builder = EngineBuilder::new().resolution(64, 64).start_scene(scene);
  let Some(mut engine) = HeadlessEngine::new_or_skip(builder) else {
    return;
  };
  engine.engine.use_gpu_world();
  assert!(engine.engine.render_pipeline.contains("world"));
  engine.step_frames(2).unwrap();
  let frame = engine.frame().unwrap();
  let px = |x, y| frame.get_pixel(x, y).0;

  // Red only, below the line's band: the sprite is drawn
  assert_eq!(px(22, 24), RED);
  // The line crosses the red sprite on top of it
  assert!(
    is_line(px(22, 32)),
    "line over the z-0 sprite: {:?}",
    px(22, 32)
  );
  // Where the sprites overlap, blue covers the line
  assert_eq!(px(30, 32), BLUE, "line under the z-2 sprite");
  assert_eq!(px(42, 32), BLUE, "line under the z-2 sprite");
  // Past the sprites the line is over the background
  assert!(is_line(px(60, 32)), "line over the background");
}
