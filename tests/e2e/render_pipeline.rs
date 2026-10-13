//! E2E tests for what the render pipeline draws, through the public API only.

use ryo2d::prelude::*;
use ryo2d::testing::HeadlessEngine;
use std::sync::Mutex;

const UI_COLOR: [u8; 4] = [10, 200, 90, 255];

fn engine(scene: impl Scene + 'static) -> Option<HeadlessEngine> {
  HeadlessEngine::new_or_skip(EngineBuilder::new().resolution(64, 64).start_scene(scene))
}

fn white_sprite(ctx: &mut SceneContext) {
  ctx.look().tonemap = Tonemap::Clamp;
  let sprite = SpriteHandle::from_sprite(Sprite::solid_rect(64, 64, 255, 255, 255, 255), "white");
  ctx.world.spawn((
    Transform::from_xy(0.0, 0.0),
    Renderable::sprite(sprite).with_anchor(Anchor::TopLeft),
  ));
}

/// Dark lit scene with a vignette and an opaque UI rectangle in the centre.
struct UiOverLitScene;

impl Scene for UiOverLitScene {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    white_sprite(ctx);
    let mut lighting = ctx.lighting();
    lighting.set_ambient(AmbientLight::with_intensity(0.02));
    lighting.set_enabled(true);
    ctx.effects().set_color_tint(Color::rgb(255, 0, 0), 1.0);
  }

  fn update(&mut self, _ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    SceneAction::Continue
  }

  fn ui(&mut self, ui: &mut UiContext, _ctx: &mut SceneContext) -> SceneAction {
    ui.draw_rect(
      Rect::new(24.0, 24.0, 16.0, 16.0),
      Color::rgb(UI_COLOR[0], UI_COLOR[1], UI_COLOR[2]),
    );
    SceneAction::Continue
  }
}

#[test]
fn ui_is_untouched_by_effects_and_lighting() {
  let Some(mut engine) = engine(UiOverLitScene) else {
    return;
  };

  engine.step_frames(3).unwrap();
  let frame = engine.frame().unwrap();

  assert_eq!(frame.get_pixel(32, 32).0, UI_COLOR);
  // The scene around the UI is dark and tinted, so lighting and the effect really ran
  let scene = frame.get_pixel(10, 32).0;
  assert!(scene[..3].iter().all(|&c| c < 64), "{scene:?}");
  assert!(
    scene[0] > scene[1] && scene[0] > scene[2],
    "not tinted: {scene:?}"
  );
}

/// A white scene under dark ambient light with sepia.
struct DarkWithEffect;

impl Scene for DarkWithEffect {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    white_sprite(ctx);
    let mut lighting = ctx.lighting();
    lighting.set_ambient(AmbientLight::with_intensity(0.02));
    lighting.set_enabled(true);
    ctx.effects().set_sepia(1.0);
  }

  fn update(&mut self, _ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    SceneAction::Continue
  }
}

#[test]
fn effects_apply_to_the_lit_image() {
  let Some(mut engine) = engine(DarkWithEffect) else {
    return;
  };

  engine.step_frames(3).unwrap();
  let center = engine.frame().unwrap().get_pixel(32, 32).0;

  assert!(
    center[..3].iter().all(|&c| c < 64),
    "effects must read the lit image, not the unlit one: {center:?}"
  );
  assert!(
    center[0] >= center[1] && center[1] >= center[2] && center[0] > center[2],
    "sepia did not run: {center:?}"
  );
}

/// A game pass that clears the current image to magenta and records the
/// format it saw.
struct Magenta(&'static Mutex<Option<ryo2d::wgpu::TextureFormat>>);

/// The format `RenderContext::format` reported to the pass after post-processing.
static SEEN_FORMAT: Mutex<Option<ryo2d::wgpu::TextureFormat>> = Mutex::new(None);
/// The format it reported to the pass before the tonemap.
static SEEN_BEFORE_TONEMAP: Mutex<Option<ryo2d::wgpu::TextureFormat>> = Mutex::new(None);

impl RenderPass for Magenta {
  fn id(&self) -> &str {
    "magenta"
  }

  fn execute(&mut self, ctx: &mut RenderContext<'_>) {
    if let Ok(mut seen) = self.0.lock() {
      *seen = Some(ctx.format());
    }
    let (encoder, view) = ctx.encoder_and_current_view();
    let _pass = encoder.begin_render_pass(&ryo2d::wgpu::RenderPassDescriptor {
      label: Some("magenta"),
      color_attachments: &[Some(ryo2d::wgpu::RenderPassColorAttachment {
        view,
        depth_slice: None,
        resolve_target: None,
        ops: ryo2d::wgpu::Operations {
          load: ryo2d::wgpu::LoadOp::Clear(ryo2d::wgpu::Color {
            r: 1.0,
            g: 0.0,
            b: 1.0,
            a: 1.0,
          }),
          store: ryo2d::wgpu::StoreOp::Store,
        },
      })],
      depth_stencil_attachment: None,
      timestamp_writes: None,
      occlusion_query_set: None,
      multiview_mask: None,
    });
  }
}

struct WithGamePass;

impl Scene for WithGamePass {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    white_sprite(ctx);
    // Post-processing runs too: magenta only survives if the pass comes after it
    ctx.effects().set_color_tint(Color::rgb(0, 255, 0), 1.0);
    let _ = ctx
      .render_pipeline()
      .insert_after(pass_ids::POST_PROCESS, Magenta(&SEEN_FORMAT));
  }

  fn update(&mut self, _ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    SceneAction::Continue
  }
}

#[test]
fn a_game_pass_after_post_process_draws_on_the_frame() {
  let Some(mut engine) = engine(WithGamePass) else {
    return;
  };

  engine.step_frames(2).unwrap();

  assert_eq!(
    engine.frame().unwrap().get_pixel(32, 32).0,
    [255, 0, 255, 255]
  );
  assert_eq!(
    *SEEN_FORMAT.lock().unwrap(),
    Some(ryo2d::wgpu::TextureFormat::Rgba8UnormSrgb)
  );
}

struct WithEarlyPass;

impl Scene for WithEarlyPass {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    white_sprite(ctx);
    let _ = ctx
      .render_pipeline()
      .insert_before(pass_ids::TONEMAP, Magenta(&SEEN_BEFORE_TONEMAP));
  }

  fn update(&mut self, _ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    SceneAction::Continue
  }
}

#[test]
fn a_frame_pass_before_the_tonemap_reaches_the_frame() {
  let Some(mut engine) = engine(WithEarlyPass) else {
    return;
  };
  engine.step_frames(2).unwrap();
  assert_eq!(
    engine.frame().unwrap().get_pixel(32, 32).0,
    [255, 0, 255, 255]
  );
  assert_eq!(
    *SEEN_BEFORE_TONEMAP.lock().unwrap(),
    Some(ryo2d::wgpu::TextureFormat::Rgba16Float),
    "frame passes before the tonemap draw on the HDR scene image"
  );
}

/// A uniform grey scene under a strong vignette; turns the post-process pass off at update 3.
struct VignetteThenOff {
  updates: u32,
}

impl Scene for VignetteThenOff {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    let sprite =
      SpriteHandle::from_sprite(Sprite::solid_rect(128, 128, 128, 128, 128, 255), "grey");
    ctx.world.spawn((
      Transform::from_xy(0.0, 0.0),
      Renderable::sprite(sprite).with_anchor(Anchor::TopLeft),
    ));
    ctx.effects().set_vignette_custom(1.0, 0.2, 0.3);
  }

  fn update(&mut self, ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    self.updates += 1;
    if self.updates == 3 {
      let _ = ctx
        .render_pipeline()
        .set_enabled(pass_ids::POST_PROCESS, false);
    }
    SceneAction::Continue
  }
}

#[test]
fn disabling_post_process_removes_the_effect() {
  let Some(mut engine) = engine(VignetteThenOff { updates: 0 }) else {
    return;
  };

  engine.step_frames(2).unwrap();
  let frame = engine.frame().unwrap();
  let (corner, centre) = (frame.get_pixel(1, 1).0, frame.get_pixel(32, 32).0);
  assert!(
    corner[0] < centre[0],
    "the vignette did not darken the corner: {corner:?} vs {centre:?}"
  );

  engine.step_frames(3).unwrap();
  let frame = engine.frame().unwrap();
  assert_eq!(frame.get_pixel(1, 1).0, frame.get_pixel(32, 32).0);
}

/// A white scene under dark ambient light with one small light in the centre;
/// `forced` is the scene's lighting override.
struct LitCorner {
  forced: Option<bool>,
}

impl Scene for LitCorner {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    white_sprite(ctx);
    ctx.world.spawn((
      Transform::from_xy(32.0, 32.0),
      Light2D::point(Color::rgb(255, 255, 255), 6.0),
    ));
    let mut lighting = ctx.lighting();
    lighting.set_ambient(AmbientLight::with_intensity(0.02));
    if let Some(enabled) = self.forced {
      lighting.set_enabled(enabled);
    }
  }
}

#[test]
fn a_light_turns_lighting_on_by_itself() {
  let Some(mut engine) = engine(LitCorner { forced: None }) else {
    return;
  };
  engine.step_frames(3).unwrap();
  let corner = engine.frame().unwrap().get_pixel(4, 4).0;
  assert!(corner[..3].iter().all(|&c| c < 64), "{corner:?}");
}

#[test]
fn lighting_forced_off_leaves_the_scene_unlit() {
  let Some(mut engine) = engine(LitCorner {
    forced: Some(false),
  }) else {
    return;
  };
  engine.step_frames(3).unwrap();
  assert_eq!(
    engine.frame().unwrap().get_pixel(4, 4).0,
    [255, 255, 255, 255]
  );
}

/// A white scene under a dark ambient and one small light, which switches to
/// a plain white scene after its second update.
struct LitThenPlain {
  updates: u32,
}

impl Scene for LitThenPlain {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    white_sprite(ctx);
    ctx.world.spawn((
      Transform::from_xy(32.0, 32.0),
      Light2D::point(Color::rgb(255, 255, 255), 6.0),
    ));
    ctx
      .lighting()
      .set_ambient(AmbientLight::with_intensity(0.02));
  }

  fn update(&mut self, _ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    self.updates += 1;
    if self.updates == 2 {
      return SceneAction::Switch(Box::new(Plain));
    }
    SceneAction::Continue
  }
}

/// White, with no lights.
struct Plain;

impl Scene for Plain {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    white_sprite(ctx);
  }
}

#[test]
fn lighting_turned_on_by_a_scene_is_off_in_the_next() {
  let Some(mut engine) = engine(LitThenPlain { updates: 0 }) else {
    return;
  };
  engine.step_frames(1).unwrap();
  let lit = engine.frame().unwrap().get_pixel(4, 4).0;
  assert!(lit[..3].iter().all(|&c| c < 64), "first scene lit: {lit:?}");
  engine.step_frames(4).unwrap();
  assert_eq!(
    engine.frame().unwrap().get_pixel(4, 4).0,
    [255, 255, 255, 255],
    "the next scene has no lights, so it is unlit"
  );
}

/// A white scene lit from the left by a shadow-casting light, with an
/// optional wall between the light and the right half.
struct LitPastAWall {
  wall: bool,
}

impl Scene for LitPastAWall {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    white_sprite(ctx);
    ctx.world.spawn((
      Transform::from_xy(16.0, 32.0),
      Light2D::point(Color::rgb(255, 255, 255), 64.0).with_shadows(),
    ));
    if self.wall {
      ctx.world.spawn((
        Transform::from_xy(32.0, 32.0),
        LightOccluder2D::rect(4.0, 30.0),
      ));
    }
    ctx
      .lighting()
      .set_ambient(AmbientLight::with_intensity(0.02));
  }
}

#[test]
fn an_occluder_shadows_what_is_behind_it() {
  let behind_the_wall = |wall| {
    let mut engine = engine(LitPastAWall { wall })?;
    engine.step_frames(3).unwrap();
    Some(engine.frame().unwrap().get_pixel(48, 32).0)
  };
  let (Some(open), Some(shadowed)) = (behind_the_wall(false), behind_the_wall(true)) else {
    return;
  };
  assert!(
    u16::from(shadowed[0]) + 40 < u16::from(open[0]),
    "{shadowed:?} vs {open:?}"
  );
}

/// A white scene under dark ambient with one light 16 px right of the
/// camera centre; the camera turns a quarter clockwise.
struct LitTurned;

impl Scene for LitTurned {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    white_sprite(ctx);
    ctx.world.spawn((
      Transform::from_xy(48.0, 32.0),
      Light2D::point(Color::rgb(255, 255, 255), 8.0),
    ));
    ctx
      .lighting()
      .set_ambient(AmbientLight::with_intensity(0.02));
    ctx.camera().set_position(32.0, 32.0);
    ctx.camera().set_rotation(std::f32::consts::FRAC_PI_2);
  }
}

#[test]
fn lights_turn_with_the_camera() {
  let Some(mut engine) = engine(LitTurned) else {
    return;
  };
  engine.step_frames(3).unwrap();
  let frame = engine.frame().unwrap();
  // 16 right of the centre shows 16 above it: (32, 16)
  let lit = frame.get_pixel(32, 16).0;
  let dark = frame.get_pixel(48, 32).0;
  assert!(lit[0] > dark[0].saturating_add(60), "{lit:?} vs {dark:?}");
}

struct CountPerCamera(std::sync::Arc<std::sync::atomic::AtomicUsize>);

impl RenderPass for CountPerCamera {
  fn id(&self) -> &str {
    "count_per_camera"
  }
  fn scope(&self) -> PassScope {
    PassScope::Camera
  }
  fn execute(&mut self, ctx: &mut RenderContext<'_>) {
    assert!(ctx.camera().is_some());
    self.0.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
  }
}

struct TwoCameras(std::sync::Arc<std::sync::atomic::AtomicUsize>);

impl Scene for TwoCameras {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    ctx.world.spawn((
      Transform::from_xy(0.0, 0.0),
      Camera2d {
        order: 1,
        ..Default::default()
      },
    ));
    ctx
      .render_pipeline()
      .insert_after(pass_ids::WORLD, CountPerCamera(self.0.clone()))
      .unwrap();
  }
}

#[test]
fn a_camera_pass_runs_once_per_camera() {
  let count = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
  let Some(mut engine) = engine(TwoCameras(count.clone())) else {
    return;
  };
  engine.step_frames(3).unwrap();
  assert_eq!(count.load(std::sync::atomic::Ordering::Relaxed), 6);
}

/// Two lit halves: each camera looks at its own light over white.
struct LitSplit;

impl Scene for LitSplit {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    for x in [-1000.0, 1000.0] {
      let white =
        SpriteHandle::from_sprite(Sprite::solid_rect(64, 64, 255, 255, 255, 255), "white-half");
      ctx
        .world
        .spawn((Transform::from_xy(x, 0.0), Renderable::sprite(white)));
      ctx.world.spawn((
        Transform::from_xy(x, 0.0),
        Light2D::point(Color::rgb(255, 255, 255), 6.0),
      ));
    }
    ctx
      .lighting()
      .set_ambient(AmbientLight::with_intensity(0.02));
    ctx.camera().set_position(-1000.0, 0.0);
    ctx.camera().set_viewport(Viewport::new(0.0, 0.0, 0.5, 1.0));
    ctx.world.spawn((
      Transform::from_xy(1000.0, 0.0),
      Camera2d {
        viewport: Viewport::new(0.5, 0.0, 0.5, 1.0),
        order: 1,
        ..Default::default()
      },
    ));
  }
}

#[test]
fn every_camera_is_lit() {
  let Some(mut engine) = engine(LitSplit) else {
    return;
  };
  engine.step_frames(3).unwrap();
  let frame = engine.frame().unwrap();
  for (lit_x, dark_x) in [(16, 2), (48, 34)] {
    let (lit, dark) = (frame.get_pixel(lit_x, 32).0, frame.get_pixel(dark_x, 32).0);
    assert!(
      lit[0] > dark[0].saturating_add(60),
      "x = {lit_x}: {lit:?} vs {dark:?}"
    );
  }
}
