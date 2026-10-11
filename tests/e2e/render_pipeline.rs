//! E2E tests for what the render pipeline draws, through the public API only.

use ryo2d::prelude::*;
use ryo2d::testing::HeadlessEngine;

const UI_COLOR: [u8; 4] = [10, 200, 90, 255];

fn engine(scene: impl Scene + 'static) -> Option<HeadlessEngine> {
  HeadlessEngine::new_or_skip(EngineBuilder::new().resolution(64, 64).start_scene(scene))
}

fn white_sprite(ctx: &mut SceneContext) {
  let sprite = SpriteHandle::from_sprite(Sprite::solid_rect(64, 64, 255, 255, 255, 255), "white");
  ctx
    .world
    .spawn((Transform::new(0.0, 0.0), Renderable::sprite(sprite)));
}

/// Draws a sprite for two updates, then switches to raycasting with no wall textures.
struct RaycastWithoutWalls {
  updates: u32,
}

impl Scene for RaycastWithoutWalls {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    white_sprite(ctx);
  }

  fn update(&mut self, ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    self.updates += 1;
    if self.updates == 3 {
      ctx.enable_raycast();
    }
    SceneAction::Continue
  }
}

#[test]
fn raycast_without_wall_textures_is_black() {
  let Some(mut engine) = engine(RaycastWithoutWalls { updates: 0 }) else {
    return;
  };

  engine.step_frames(2).unwrap();
  assert_eq!(
    engine.frame().unwrap().get_pixel(32, 32).0,
    [255, 255, 255, 255]
  );

  engine.step_frames(3).unwrap();
  let frame = engine.frame().unwrap();
  assert!(
    frame.pixels().all(|p| p.0 == [0, 0, 0, 255]),
    "a raycast frame without walls must be black, not the previous frame"
  );
}

/// Dark lit scene with a vignette and an opaque UI rectangle in the centre.
struct UiOverLitScene;

impl Scene for UiOverLitScene {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    white_sprite(ctx);
    if let Some(lighting) = ctx.lighting.as_mut() {
      lighting.set_ambient(AmbientLight::with_intensity(0.02));
      lighting.set_enabled(true);
    }
    ctx.effects.set_color_tint(Color::rgb(255, 0, 0), 1.0);
  }

  fn update(&mut self, _ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    SceneAction::Continue
  }

  fn render_ui(&mut self, ui: &mut UiContext) -> SceneAction {
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
    if let Some(lighting) = ctx.lighting.as_mut() {
      lighting.set_ambient(AmbientLight::with_intensity(0.02));
      lighting.set_enabled(true);
    }
    ctx.effects.set_sepia(1.0);
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

/// A game pass that clears the current image to magenta.
struct Magenta;

impl RenderPass for Magenta {
  fn id(&self) -> &str {
    "magenta"
  }

  fn execute(&mut self, ctx: &mut RenderContext<'_>) {
    let (encoder, view) = ctx.encoder_and_current_view();
    let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
      label: Some("magenta"),
      color_attachments: &[Some(wgpu::RenderPassColorAttachment {
        view,
        resolve_target: None,
        ops: wgpu::Operations {
          load: wgpu::LoadOp::Clear(wgpu::Color {
            r: 1.0,
            g: 0.0,
            b: 1.0,
            a: 1.0,
          }),
          store: wgpu::StoreOp::Store,
        },
      })],
      depth_stencil_attachment: None,
      timestamp_writes: None,
      occlusion_query_set: None,
    });
  }
}

struct WithGamePass;

impl Scene for WithGamePass {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    white_sprite(ctx);
    // Post-processing runs too: magenta only survives if the pass comes after it
    ctx.effects.set_color_tint(Color::rgb(0, 255, 0), 1.0);
    let _ = ctx
      .render_pipeline
      .insert_after(pass_ids::POST_PROCESS, Magenta);
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
}

/// A uniform grey scene under a strong vignette; turns the post-process pass off at update 3.
struct VignetteThenOff {
  updates: u32,
}

impl Scene for VignetteThenOff {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    let sprite =
      SpriteHandle::from_sprite(Sprite::solid_rect(128, 128, 128, 128, 128, 255), "grey");
    ctx
      .world
      .spawn((Transform::new(0.0, 0.0), Renderable::sprite(sprite)));
    ctx.effects.set_vignette_custom(1.0, 0.2, 0.3);
  }

  fn update(&mut self, ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    self.updates += 1;
    if self.updates == 3 {
      let _ = ctx
        .render_pipeline
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
