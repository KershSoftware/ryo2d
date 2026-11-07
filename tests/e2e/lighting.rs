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
    let floor = ctx
      .assets()
      .add(Image::solid_rect(128, 64, 255, 255, 255, 255));
    ctx.world.spawn((
      Transform::from_xy(0.0, 0.0),
      Renderable::image(floor).with_anchor(Anchor::TopLeft),
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

/// The red byte at (96, 32), lit by light B; with `with_a`, light A on the
/// left casts an occluder's shadow over all of B.
fn light_b(with_a: bool) -> Option<u8> {
  let frame = lit(move |ctx| {
    ctx
      .lighting()
      .set_ambient(AmbientLight::with_intensity(0.0));
    if with_a {
      ctx.world.spawn((
        Transform::from_xy(24.0, 32.0),
        Light2D::point(Color::WHITE, 100.0).with_shadows(),
      ));
      ctx.world.spawn((
        Transform::from_xy(40.0, 32.0),
        LightOccluder2D::rect(4.0, 16.0),
      ));
    }
    ctx.world.spawn((
      Transform::from_xy(96.0, 32.0),
      Light2D::point(Color::WHITE, 20.0).with_intensity(0.3),
    ));
  })?;
  Some(frame.get_pixel(96, 32).0[0])
}

#[test]
fn a_shadow_darkens_only_its_own_light() {
  let (Some(in_shadow), Some(alone)) = (light_b(true), light_b(false)) else {
    return;
  };
  assert!(
    in_shadow.abs_diff(alone) <= 2,
    "B alone {alone}, in A's shadow {in_shadow}"
  );
}

/// The frame after two steps of the floor lit by `setup`, over a map of 8 x
/// 4 cells of 16 px covering the window whose floor 1 draws 16 px up, its
/// layers empty so it draws nothing; stepped until the map has landed
fn storeyed(mut setup: impl FnMut(&mut SceneContext) + Send + 'static) -> Option<image::RgbaImage> {
  let scene = Lit(Box::new(move |ctx| {
    let mut map = TileMap::new(UVec2::new(8, 4), UVec2::splat(16));
    map.add_tile_layer("ground");
    let upper = map.add_tile_layer("upper");
    map.layers[upper].info.floor = 1;
    map.layers[upper].info.offset = Vec2::new(0.0, -16.0);
    let handle = ctx.assets().add(map);
    ctx.world.spawn((Transform::IDENTITY, MapRoot::new(handle)));
    setup(ctx);
  }));
  let builder = EngineBuilder::new().resolution(128, 64).start_scene(scene);
  let mut engine = HeadlessEngine::new_or_skip(builder)?;
  let start = std::time::Instant::now();
  loop {
    engine.step().unwrap();
    let world = engine.world_mut();
    let roots: Vec<Entity> = world
      .query_filtered::<Entity, With<MapRoot>>()
      .iter(world)
      .collect();
    if roots
      .iter()
      .all(|&root| MapsView::new(world).is_ready(root))
    {
      break;
    }
    assert!(start.elapsed().as_secs() < 5, "the map did not land in 5 s");
    std::thread::yield_now();
  }
  engine.step_frames(2).unwrap();
  Some(engine.frame().unwrap())
}

/// A light on floor 1 standing at (16, 48), so drawn at (16, 32), beside a
/// 4 x 8 wall standing at (32, 48) on `wall`'s floor (`None`: no `Floor`,
/// the ground floor)
fn floor_light(wall: Option<Floor>) -> Option<image::RgbaImage> {
  storeyed(move |ctx| {
    ctx
      .lighting()
      .set_ambient(AmbientLight::with_intensity(0.0));
    ctx.world.spawn((
      Transform::from_xy(16.0, 48.0),
      Light2D::point(Color::WHITE, 100.0).with_shadows(),
      Floor::On(1),
    ));
    let at = Transform::from_xy(32.0, 48.0);
    let occluder = LightOccluder2D::rect(4.0, 8.0);
    match wall {
      Some(floor) => ctx.world.spawn((at, occluder, floor)),
      None => ctx.world.spawn((at, occluder)),
    };
  })
}

#[test]
fn a_floor_light_and_its_shadows_draw_lifted() {
  let (Some(own), Some(ground), Some(every)) = (
    floor_light(Some(Floor::On(1))),
    floor_light(None),
    floor_light(Some(Floor::Every)),
  ) else {
    return;
  };
  let red = |frame: &image::RgbaImage, x: u32, y: u32| frame.get_pixel(x, y).0[0];
  // The light draws 16 px up: as bright 16.5 px above (16, 32) as below
  // (pixel (x, y) is lit at its centre, (x + 0.5, y + 0.5))
  let (above, below) = (red(&ground, 16, 15), red(&ground, 16, 48));
  assert!(
    above > 100 && above.abs_diff(below) <= 2,
    "{above} above, {below} below"
  );
  // Its floor's wall casts behind where the wall draws, (32, 32): its shadow
  // draws lifted with it, and not behind where the wall stands
  let (lifted, standing) = ((48, 32), (48, 48));
  let shade = red(&own, lifted.0, lifted.1);
  let lit = red(&own, standing.0, standing.1);
  assert!(
    shade < 10 && lit > 100,
    "behind it drawn {shade}, standing {lit}"
  );
  // A ground floor wall does not block it; an every floor wall does
  let open = red(&ground, lifted.0, lifted.1);
  assert!(open > 100, "past the ground floor's wall {open}");
  let blocked = red(&every, lifted.0, lifted.1);
  assert!(blocked < 10, "behind every floor's wall {blocked}");
}

/// The red byte at (80, 32) behind `occluders` stacked occluders, under a
/// light whose shadow keeps half its light.
fn behind(occluders: usize) -> Option<u8> {
  let frame = lit(move |ctx| {
    ctx
      .lighting()
      .set_ambient(AmbientLight::with_intensity(0.0));
    let light = Light2D::point(Color::WHITE, 100.0)
      .with_intensity(0.5)
      .with_shadows()
      .with_shadow_color(Color::new(0, 0, 0, 128));
    ctx.world.spawn((Transform::from_xy(24.0, 32.0), light));
    for i in 0..occluders {
      ctx.world.spawn((
        Transform::from_xy(40.0 + i as f32 * 6.0, 32.0),
        LightOccluder2D::rect(4.0, 16.0),
      ));
    }
  })?;
  Some(frame.get_pixel(80, 32).0[0])
}

#[test]
fn overlapping_shadows_combine_instead_of_cutting_twice() {
  let (Some(open), Some(one), Some(two)) = (behind(0), behind(1), behind(2)) else {
    return;
  };
  let linear = |byte: u8| {
    let c = f32::from(byte) / 255.0;
    if c <= 0.04045 {
      c / 12.92
    } else {
      ((c + 0.055) / 1.055).powf(2.4)
    }
  };
  let (open, one, two) = (linear(open), linear(one), linear(two));
  // Alpha 128 keeps about half the light behind one occluder, a quarter behind two
  assert!((one / open - 0.5).abs() < 0.06, "one {one} of {open}");
  assert!((two / open - 0.25).abs() < 0.06, "two {two} of {open}");
}

#[test]
fn a_coloured_shadow_lets_its_colour_through() {
  let Some(frame) = lit(|ctx| {
    ctx
      .lighting()
      .set_ambient(AmbientLight::with_intensity(0.0));
    let light = Light2D::point(Color::WHITE, 100.0)
      .with_intensity(0.5)
      .with_shadows()
      .with_shadow_color(Color::rgb(255, 0, 0));
    ctx.world.spawn((Transform::from_xy(24.0, 32.0), light));
    ctx.world.spawn((
      Transform::from_xy(40.0, 32.0),
      LightOccluder2D::rect(4.0, 16.0),
    ));
  }) else {
    return;
  };
  let pixel = frame.get_pixel(80, 32).0;
  assert!(pixel[0] > 40 && pixel[1] < 10 && pixel[2] < 10, "{pixel:?}");
}

/// Two lights of intensity 60000 on the edge of a black patch over the left
/// half of the floor: together they pass what the light buffer's half
/// floats hold, and bloom spreads whatever they leave.
#[test]
fn lights_past_the_half_float_range_leave_no_nan() {
  let Some(frame) = lit(|ctx| {
    ctx
      .lighting()
      .set_ambient(AmbientLight::with_intensity(0.0));
    ctx.effects().set_bloom(1.0);
    let black = ctx.assets().add(Image::solid_rect(64, 64, 0, 0, 0, 255));
    ctx.world.spawn((
      Transform::from_xy(0.0, 0.0),
      Renderable::image(black)
        .with_anchor(Anchor::TopLeft)
        .with_z_order(1),
    ));
    for _ in 0..2 {
      ctx.world.spawn((
        Transform::from_xy(64.0, 32.0),
        Light2D::point(Color::WHITE, 30.0).with_intensity(60000.0),
      ));
    }
  }) else {
    return;
  };
  // The lit floor shows white and the black patch beside it glows: NaN
  // (0 x inf where the light buffer overflowed) would show black, and
  // bloom would spread it
  for x in 40..=88 {
    let pixel = frame.get_pixel(x, 32).0;
    let lit_floor = x >= 64;
    let shown = if lit_floor {
      pixel[0] >= 242
    } else {
      pixel[0] > 0
    };
    assert!(shown, "x {x}: {pixel:?}");
  }
}

/// Two additive sprites of emission 65504 pile up past the half float range
/// in the scene; with no light and no ambient the lit image stays black
/// (infinity x 0 is NaN, which bloom would spread). Backends that saturate
/// half float blending cannot produce the infinity, so this guards the rest.
#[test]
fn an_overflowed_scene_texel_in_the_dark_stays_black() {
  let Some(frame) = lit(|ctx| {
    {
      let mut lighting = ctx.lighting();
      lighting.set_ambient(AmbientLight::with_intensity(0.0));
      lighting.set_enabled(true);
    }
    ctx.effects().set_bloom(1.0);
    let spark = ctx
      .assets()
      .add(Image::solid_rect(8, 8, 255, 255, 255, 255));
    for _ in 0..2 {
      ctx.world.spawn((
        Transform::from_xy(60.0, 28.0),
        Renderable::image(spark.clone())
          .with_anchor(Anchor::TopLeft)
          .with_blend(BlendMode::Additive)
          .with_emission(65504.0)
          .with_z_order(1),
      ));
    }
  }) else {
    return;
  };
  for (x, y, pixel) in frame.enumerate_pixels() {
    assert!(pixel.0[..3].iter().all(|&c| c < 4), "({x}, {y}): {pixel:?}");
  }
}
