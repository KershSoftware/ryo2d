//! 2D lighting through the engine: lights, shadows, ambient light and
//! atmosphere, read back as display bytes.
//!
//! Tests skip when no GPU is available.

use ryo2d::prelude::*;
use ryo2d::testing::HeadlessEngine;

type Setup = Box<dyn FnMut(&mut SceneContext) + Send>;

/// A white floor over the whole 128x64 window, then `setup`.
struct Lit(Setup);

impl Scene for Lit {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    let floor =
      SpriteHandle::from_sprite(Sprite::solid_rect(128, 64, 255, 255, 255, 255), "lit-floor");
    ctx.world.spawn((
      Transform::from_xy(0.0, 0.0),
      Renderable::sprite(floor).with_anchor(Anchor::TopLeft),
    ));
    (self.0)(ctx);
  }

  fn update(&mut self, _ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    SceneAction::Continue
  }
}

/// The frame after two steps of the floor lit by `setup`.
fn lit(setup: impl FnMut(&mut SceneContext) + Send + 'static) -> Option<image::RgbaImage> {
  let scene = Lit(Box::new(setup));
  let builder = EngineBuilder::new().resolution(128, 64).start_scene(scene);
  let mut engine = HeadlessEngine::new_or_skip(builder)?;
  engine.step_frames(2).unwrap();
  Some(engine.frame().unwrap())
}

#[test]
fn ambient_mid_grey_lights_white_to_mid_grey() {
  let Some(frame) = lit(|ctx| {
    let mut lighting = ctx.lighting();
    lighting.set_ambient(AmbientLight::new(Color::rgb(128, 128, 128), 1.0));
    lighting.set_enabled(true);
  }) else {
    return;
  };
  let pixel = frame.get_pixel(64, 32).0;
  // Read as linear (the old rule) it showed about 188
  assert!(pixel[0].abs_diff(128) <= 1, "{pixel:?}");
}

/// No ambient light, and `light` at the centre of the floor.
fn centre_light(light: Light2D) -> impl FnMut(&mut SceneContext) + Send + 'static {
  move |ctx| {
    ctx
      .lighting()
      .set_ambient(AmbientLight::with_intensity(0.0));
    ctx
      .world
      .spawn((Transform::from_xy(64.0, 32.0), light.clone()));
  }
}

#[test]
fn a_light_lights_what_it_covers() {
  let Some(frame) = lit(centre_light(Light2D::point(Color::WHITE, 30.0))) else {
    return;
  };
  let (centre, corner) = (frame.get_pixel(64, 32).0, frame.get_pixel(4, 4).0);
  assert!(centre[0] > 200, "{centre:?}");
  assert!(corner[0] < 10, "{corner:?}");
}

#[test]
fn zero_ambient_without_lights_is_black() {
  let Some(frame) = lit(|ctx| {
    let mut lighting = ctx.lighting();
    lighting.set_ambient(AmbientLight::with_intensity(0.0));
    lighting.set_enabled(true);
  }) else {
    return;
  };
  assert!(
    frame.get_pixel(64, 32).0[0] < 3,
    "{:?}",
    frame.get_pixel(64, 32).0
  );
}

#[test]
fn disabled_lighting_leaves_the_scene_unlit() {
  let Some(frame) = lit(|ctx| {
    ctx.world.spawn((
      Transform::from_xy(64.0, 32.0),
      Light2D::point(Color::rgb(40, 40, 40), 10.0),
    ));
    let mut lighting = ctx.lighting();
    lighting.set_ambient(AmbientLight::with_intensity(0.0));
    lighting.set_enabled(false);
  }) else {
    return;
  };
  assert!(
    frame.get_pixel(4, 4).0[0] > 230,
    "{:?}",
    frame.get_pixel(4, 4).0
  );
}

/// The red byte inside a dim spotlight's cone, with or without its beam.
fn in_the_beam(volumetric: bool) -> Option<u8> {
  let frame = lit(move |ctx| {
    ctx
      .lighting()
      .set_ambient(AmbientLight::with_intensity(0.0));
    let spot = Light2D::spot(Color::WHITE, 120.0, 40.0, 0.0).with_intensity(0.2);
    let spot = if volumetric {
      spot.with_volumetric()
    } else {
      spot
    };
    ctx.world.spawn((Transform::from_xy(8.0, 32.0), spot));
  })?;
  Some(frame.get_pixel(60, 32).0[0])
}

#[test]
fn a_volumetric_spotlight_brightens_its_beam() {
  let (Some(plain), Some(beam)) = (in_the_beam(false), in_the_beam(true)) else {
    return;
  };
  assert!(beam > plain + 3, "plain {plain}, beam {beam}");
}

#[test]
fn a_light_on_layer_one_does_not_light_a_layer_zero_camera() {
  let Some(frame) = lit(|ctx| {
    ctx
      .lighting()
      .set_ambient(AmbientLight::with_intensity(0.0));
    ctx.world.spawn((
      Transform::from_xy(64.0, 32.0),
      Light2D::point(Color::WHITE, 30.0),
      RenderLayers::layer(1),
    ));
  }) else {
    return;
  };
  assert!(
    frame.get_pixel(64, 32).0[0] < 3,
    "{:?}",
    frame.get_pixel(64, 32).0
  );
}

#[test]
fn lights_past_the_old_cap_all_draw() {
  let Some(frame) = lit(|ctx| {
    ctx
      .lighting()
      .set_ambient(AmbientLight::with_intensity(0.0));
    // 300 faint lights in a corner, then the one that lights the centre
    for _ in 0..300 {
      ctx.world.spawn((
        Transform::from_xy(2.0, 2.0),
        Light2D::point(Color::WHITE, 1.0).with_intensity(0.001),
      ));
    }
    ctx.world.spawn((
      Transform::from_xy(64.0, 32.0),
      Light2D::point(Color::WHITE, 30.0),
    ));
  }) else {
    return;
  };
  assert!(
    frame.get_pixel(64, 32).0[0] > 200,
    "{:?}",
    frame.get_pixel(64, 32).0
  );
}

#[test]
fn degenerate_lights_draw_nothing() {
  let Some(frame) = lit(|ctx| {
    ctx
      .lighting()
      .set_ambient(AmbientLight::with_intensity(0.5));
    ctx.effects().set_bloom(1.0);
    let mut lights = vec![Light2D::point(Color::WHITE, 30.0); 4];
    if let Light2D::Point { radius, .. } = &mut lights[0] {
      *radius = 0.0;
    }
    if let Light2D::Point { radius, .. } = &mut lights[1] {
      *radius = f32::NAN;
    }
    if let Light2D::Point { intensity, .. } = &mut lights[2] {
      *intensity = f32::NAN;
    }
    if let Light2D::Point { intensity, .. } = &mut lights[3] {
      *intensity = f32::INFINITY;
    }
    for light in lights {
      ctx.world.spawn((Transform::from_xy(64.0, 32.0), light));
    }
    ctx.world.spawn((
      Transform::from_xy(f32::NAN, 32.0),
      Light2D::point(Color::WHITE, 30.0),
    ));
  }) else {
    return;
  };
  // Half the ambient light (sRGB 188) everywhere: no light, no NaN spread by bloom
  let corner = frame.get_pixel(4, 4).0;
  assert!(corner[0].abs_diff(188) <= 1, "{corner:?}");
  assert!(
    frame.pixels().all(|p| p.0 == corner),
    "the floor is lit evenly"
  );
}
