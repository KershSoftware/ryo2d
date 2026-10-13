//! E2E tests for GPU sprites drawn from the texture atlas: masking, effect
//! growth and CPU-matching (sRGB-space) shading.

use ryo2d::prelude::*;
use ryo2d::testing::HeadlessEngine;

const RED: [u8; 4] = [220, 30, 40, 255];
const BLUE: [u8; 4] = [30, 60, 230, 255];
const GREEN: [u8; 4] = [0, 255, 0, 255];
const MAGENTA: [u8; 4] = [255, 0, 255, 255];

fn solid(name: &str, c: [u8; 4]) -> SpriteHandle {
  SpriteHandle::from_sprite(Sprite::solid_rect(4, 4, c[0], c[1], c[2], c[3]), name)
}

fn color(c: [u8; 4]) -> Color {
  Color::new(c[0], c[1], c[2], c[3])
}

/// Draws the first of `sprites`, 4x4 texels, centred at (32, 32) at `scale`,
/// and the others off screen after it, so they sit next to it in the atlas.
struct AtlasScene {
  sprites: Vec<SpriteHandle>,
  scale: f32,
  tint: Color,
  effect: Option<SpriteEffect>,
}

impl AtlasScene {
  fn new(sprites: Vec<SpriteHandle>, scale: f32) -> Self {
    Self {
      sprites,
      scale,
      tint: Color::WHITE,
      effect: None,
    }
  }
}

impl Scene for AtlasScene {
  fn update(&mut self, _ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    SceneAction::Continue
  }

  fn render(&mut self, gpu: &mut GpuRenderContext) {
    let size = 4.0 * self.scale;
    let mut instance = SpriteInstance::new()
      .with_transform(32.0, 32.0, 0.0, size, size)
      .with_tint(self.tint, 1.0);
    if let Some(effect) = &self.effect {
      instance = instance.with_effect(effect);
    }
    let draw = SpriteDraw {
      effects: self.effect.is_some(),
      ..Default::default()
    };
    gpu.add_sprite(&self.sprites[0], instance, draw);
    for sprite in &self.sprites[1..] {
      let off_screen = SpriteInstance::new().with_transform(-100.0, -100.0, 0.0, 4.0, 4.0);
      gpu.add_sprite(sprite, off_screen, SpriteDraw::default());
    }
  }
}

fn render(scene: AtlasScene) -> Option<image::RgbaImage> {
  let builder = EngineBuilder::new().resolution(64, 64).start_scene(scene);
  let mut engine = HeadlessEngine::new_or_skip(builder)?;
  engine.step_frames(2).unwrap();
  Some(engine.frame().unwrap())
}

/// Assert every pixel of the 64x64 frame against `expected` (None = background).
fn assert_pixels(frame: &image::RgbaImage, expected: impl Fn(u32, u32) -> Option<[u8; 4]>) {
  let background = frame.get_pixel(0, 0).0;
  let mut wrong = Vec::new();
  for y in 0..64 {
    for x in 0..64 {
      let want = expected(x, y).unwrap_or(background);
      let got = frame.get_pixel(x, y).0;
      if got != want {
        wrong.push(format!("({x},{y}) {got:?} != {want:?}"));
      }
    }
  }
  assert!(
    wrong.is_empty(),
    "{} wrong pixels, first: {:?}",
    wrong.len(),
    &wrong[..wrong.len().min(6)]
  );
}

fn inside(x: u32, y: u32, min: u32, max: u32) -> bool {
  (min..max).contains(&x) && (min..max).contains(&y)
}

#[test]
fn outline_ignores_atlas_neighbours() {
  let mut scene = AtlasScene::new(
    vec![
      solid("atlas-outline-red", RED),
      solid("atlas-outline-blue", BLUE),
    ],
    4.0,
  );
  scene.effect = Some(SpriteEffect::outline(color(GREEN), 2));
  let Some(frame) = render(scene) else {
    return;
  };
  // Sprite 16x16 px at [24, 40); a 2-texel (8 px) outline ring around it
  assert_pixels(&frame, |x, y| {
    if inside(x, y, 24, 40) {
      Some(RED)
    } else if inside(x, y, 16, 48) {
      Some(GREEN)
    } else {
      None
    }
  });
}

/// The CPU renderer's tint: multiply by the tint fraction, truncate.
fn cpu_tint(source: u8, tint: u8) -> u8 {
  (source as f32 * (tint as f32 / 255.0)).min(255.0) as u8
}

#[test]
fn tint_matches_the_cpu() {
  // Pairs the CPU truncates but a rounding encode would not, e.g. 130 x 200
  // (CPU 101) and 77 x 133 (CPU 40), plus white and an over-bright product
  let sources = [
    [255, 255, 255, 255],
    [130, 77, 200, 255],
    [10, 250, 99, 255],
  ];
  let tints = [Color::rgb(200, 100, 50), Color::rgb(200, 133, 255)];
  for source in sources {
    for tint in tints {
      let mut scene = AtlasScene::new(vec![solid("atlas-tint-source", source)], 4.0);
      scene.tint = tint;
      let Some(frame) = render(scene) else {
        return;
      };
      let want = [
        cpu_tint(source[0], tint.r),
        cpu_tint(source[1], tint.g),
        cpu_tint(source[2], tint.b),
        255,
      ];
      assert_pixels(&frame, |x, y| inside(x, y, 24, 40).then_some(want));
    }
  }
}

#[test]
fn tint_expectations_follow_the_cpu_formula() {
  // Guards the helper against drifting from the CPU's truncation
  assert_eq!(cpu_tint(130, 200), 101);
  assert_eq!(cpu_tint(77, 133), 40);
}

#[test]
fn directional_outline_draws() {
  let mut scene = AtlasScene::new(vec![solid("atlas-directional-red", RED)], 4.0);
  scene.effect = Some(SpriteEffect::directional_outline(
    color(GREEN),
    color(MAGENTA),
    1,
  ));
  let Some(frame) = render(scene) else {
    return;
  };
  // Sprite at [24, 40); a 1-texel (4 px) ring: passable colour below the
  // sprite (bottom corners included), solid colour on the other sides
  assert_pixels(&frame, |x, y| {
    if inside(x, y, 24, 40) {
      Some(RED)
    } else if inside(x, y, 20, 44) {
      Some(if y >= 40 { MAGENTA } else { GREEN })
    } else {
      None
    }
  });
}
