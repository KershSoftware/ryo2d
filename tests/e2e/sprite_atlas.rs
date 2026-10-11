//! E2E tests for GPU sprites drawn from the texture atlas: masking, effect
//! growth and CPU-matching (sRGB-space) shading.

use ryo2d::core::graphics::SpriteGpuRenderer;
use ryo2d::prelude::*;
use ryo2d::testing::HeadlessEngine;

const RED: [u8; 4] = [220, 30, 40, 255];
const BLUE: [u8; 4] = [30, 60, 230, 255];
const GREEN: [u8; 4] = [0, 255, 0, 255];
const MAGENTA: [u8; 4] = [255, 0, 255, 255];
const WHITE: [u8; 4] = [255, 255, 255, 255];

fn solid(name: &str, c: [u8; 4]) -> SpriteHandle {
  SpriteHandle::from_sprite(Sprite::solid_rect(4, 4, c[0], c[1], c[2], c[3]), name)
}

fn color(c: [u8; 4]) -> Color {
  Color::new(c[0], c[1], c[2], c[3])
}

/// Uploads `sprites` in order (so they sit next to each other in the atlas)
/// and draws the first one, 4x4 texels, centred at (32, 32) at `scale`.
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

  fn render_gpu(&mut self, gpu: &mut GpuRenderContext) {
    for sprite in &self.sprites {
      gpu.upload_sprite(sprite);
    }
    let size = 4.0 * self.scale;
    let mut instance = SpriteInstance::new()
      .with_transform(32.0, 32.0, 0.0, size, size)
      .with_tint(self.tint, 1.0);
    if let Some(effect) = &self.effect {
      instance = instance.with_effect(effect);
    }
    let id = self.sprites[0].id();
    gpu.add_sprite_instance(&id, instance, 0, self.effect.is_some());
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
fn no_bleeding_under_scaling() {
  let red = solid("atlas-bleed-red", RED);
  let blue = solid("atlas-bleed-blue", BLUE);

  // Bleeding is only possible when the sprites share an atlas page
  let Ok((device, queue)) = ryo2d::testing::shared_gpu() else {
    return;
  };
  let mut renderer =
    SpriteGpuRenderer::new(&device, &queue, ryo2d::wgpu::TextureFormat::Rgba8UnormSrgb);
  renderer.upload_sprite(&device, &queue, &red);
  renderer.upload_sprite(&device, &queue, &blue);
  let (Some(a), Some(b)) = (
    renderer.get_texture_bind_group(&red.id()),
    renderer.get_texture_bind_group(&blue.id()),
  ) else {
    panic!("both sprites must be uploaded");
  };
  assert!(
    std::ptr::eq(a, b),
    "the two sprites must share an atlas page"
  );

  let Some(frame) = render(AtlasScene::new(vec![red, blue], 3.0)) else {
    return;
  };
  // 12x12 px rect centred at (32, 32)
  assert_pixels(&frame, |x, y| inside(x, y, 26, 38).then_some(RED));
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

#[test]
fn tint_matches_the_cpu() {
  let mut scene = AtlasScene::new(vec![solid("atlas-tint-white", WHITE)], 4.0);
  scene.tint = Color::rgb(200, 100, 50);
  let Some(frame) = render(scene) else {
    return;
  };
  assert_pixels(&frame, |x, y| {
    inside(x, y, 24, 40).then_some([200, 100, 50, 255])
  });
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
