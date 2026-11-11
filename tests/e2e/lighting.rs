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

/// `byte` (sRGB) as linear light
fn linear(byte: u8) -> f32 {
  let c = f32::from(byte) / 255.0;
  if c <= 0.04045 {
    c / 12.92
  } else {
    ((c + 0.055) / 1.055).powf(2.4)
  }
}

#[test]
fn strength_applies_once() {
  let (Some(open), Some(one), Some(two)) = (behind(0), behind(1), behind(2)) else {
    return;
  };
  let (open, one, two) = (linear(open), linear(one), linear(two));
  // Alpha 128 keeps about half the light behind one occluder, and behind
  // two: the nearest occluder decides, the strength applies once
  assert!((one / open - 0.5).abs() < 0.06, "one {one} of {open}");
  assert!((two / open - 0.5).abs() < 0.06, "two {two} of {open}");
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
  // A full-strength red shadow lets all of the light's red through: the
  // red is the open floor's
  let Some(open) = behind(0) else {
    return;
  };
  assert!(pixel[0].abs_diff(open) <= 2, "{pixel:?} against {open}");
}

/// The floor under `ambient` white ambient light, lit by `lights` (each at
/// its point), past `occluders` (each at its point)
fn scene(
  ambient: f32,
  lights: Vec<(Vec2, Light2D)>,
  occluders: Vec<(Vec2, LightOccluder2D)>,
) -> Option<image::RgbaImage> {
  lit(move |ctx| {
    ctx
      .lighting()
      .set_ambient(AmbientLight::with_intensity(ambient));
    ctx.lighting().set_enabled(true);
    for (at, light) in &lights {
      ctx
        .world
        .spawn((Transform::from_xy(at.x, at.y), light.clone()));
    }
    for (at, occluder) in &occluders {
      ctx
        .world
        .spawn((Transform::from_xy(at.x, at.y), occluder.clone()));
    }
  })
}

fn red(frame: &image::RgbaImage, x: u32, y: u32) -> u8 {
  frame.get_pixel(x, y).0[0]
}

#[test]
fn a_hard_shadow_falls_behind_an_occluder() {
  // A 256 px light at (16, 32), at zoom 1 and the default quality (High),
  // and a 4 x 8 wall at (40, 32): its shadow at x 100 spans y 16.7 to 47.3
  let light = || {
    vec![(
      Vec2::new(16.0, 32.0),
      Light2D::point(Color::WHITE, 256.0)
        .with_intensity(0.5)
        .with_shadows(),
    )]
  };
  let wall = || vec![(Vec2::new(40.0, 32.0), LightOccluder2D::rect(4.0, 8.0))];
  let (Some(shadowed), Some(open), Some(ambient)) = (
    scene(0.1, light(), wall()),
    scene(0.1, light(), Vec::new()),
    scene(0.1, Vec::new(), Vec::new()),
  ) else {
    return;
  };
  // Behind it: the ambient light alone
  let (behind, alone) = (
    linear(red(&shadowed, 100, 32)),
    linear(red(&ambient, 100, 32)),
  );
  assert!(
    (behind - alone).abs() < 0.05,
    "behind {behind}, ambient {alone}"
  );
  assert!(
    linear(red(&open, 100, 32)) > alone + 0.1,
    "the light reaches it"
  );
  // Beside the shadow: unchanged
  for (x, y) in [(100, 6), (100, 58), (30, 32), (60, 10)] {
    let (beside, unshadowed) = (red(&shadowed, x, y), red(&open, x, y));
    assert!(
      beside.abs_diff(unshadowed) <= 1,
      "({x}, {y}): {beside} against {unshadowed}"
    );
  }
  // Its edge, down the column at x 100, is at most 2 px wide
  let fraction = |y: u32| {
    let (lit, dark) = (linear(red(&open, 100, y)), linear(red(&ambient, 100, y)));
    (linear(red(&shadowed, 100, y)) - dark) / (lit - dark)
  };
  let fractions: Vec<f32> = (0..32).map(fraction).collect();
  let partial = fractions.iter().filter(|&&f| f > 0.1 && f < 0.9).count();
  assert!(partial <= 2, "{fractions:?}");
  assert!(fractions[..10].iter().all(|&f| f > 0.9), "{fractions:?}");
  assert!(fractions[24..].iter().all(|&f| f < 0.1), "{fractions:?}");
}

#[test]
fn a_carried_light_lights_round_its_bearer() {
  // A red light inside a closed 32 px occluder (its bearer), and a green
  // light out to the left whose shadow of it falls to the right
  let bearer = || vec![(Vec2::new(64.0, 32.0), LightOccluder2D::rect(32.0, 32.0))];
  let lights = || {
    vec![
      (
        Vec2::new(64.0, 32.0),
        Light2D::point(Color::rgb(255, 0, 0), 60.0)
          .with_intensity(0.5)
          .with_shadows(),
      ),
      (
        Vec2::new(8.0, 32.0),
        Light2D::point(Color::rgb(0, 255, 0), 160.0)
          .with_intensity(0.5)
          .with_shadows(),
      ),
    ]
  };
  let (Some(carried), Some(open)) = (
    scene(0.0, lights(), bearer()),
    scene(0.0, lights(), Vec::new()),
  ) else {
    return;
  };
  // Lit all round at 40 px, as with no occluder
  for (x, y) in [(24, 32), (104, 32), (36, 4), (92, 4), (36, 60), (92, 60)] {
    let (shown, unshadowed) = (red(&carried, x, y), red(&open, x, y));
    assert!(
      unshadowed > 20 && shown.abs_diff(unshadowed) <= 2,
      "({x}, {y}): {shown} against {unshadowed}"
    );
  }
  // The green light's shadow of it still falls
  let green = |frame: &image::RgbaImage| frame.get_pixel(110, 32).0[1];
  assert!(
    green(&open) > 40 && green(&carried) < 3,
    "{} against {}",
    green(&carried),
    green(&open)
  );
}

#[test]
fn a_light_on_an_occluder_s_edge_lights_the_side_it_faces() {
  // A 32 px block spanning x 48 to 80 and y 16 to 48
  let block = || vec![(Vec2::new(64.0, 32.0), LightOccluder2D::rect(32.0, 32.0))];
  let at = |x: f32, y: f32| {
    vec![(
      Vec2::new(x, y),
      Light2D::point(Color::WHITE, 100.0)
        .with_intensity(0.5)
        .with_shadows(),
    )]
  };
  // On its right edge, its left edge and its top right corner: (x, y) of a
  // point on the side it faces, then of one behind the block
  for (light, faced, behind) in [
    ((80.0, 32.0), (110, 32), (20, 32)),
    ((48.0, 32.0), (18, 32), (110, 32)),
    ((80.0, 16.0), (110, 40), (30, 60)),
  ] {
    let (Some(edge), Some(open)) = (
      scene(0.0, at(light.0, light.1), block()),
      scene(0.0, at(light.0, light.1), Vec::new()),
    ) else {
      return;
    };
    let (shown, unshadowed) = (red(&edge, faced.0, faced.1), red(&open, faced.0, faced.1));
    assert!(
      unshadowed > 20 && shown.abs_diff(unshadowed) <= 2,
      "{light:?}, faced: {shown} against {unshadowed}"
    );
    let (shown, unshadowed) = (
      red(&edge, behind.0, behind.1),
      red(&open, behind.0, behind.1),
    );
    assert!(
      unshadowed > 20 && shown < 3,
      "{light:?}, behind: {shown} against {unshadowed}"
    );
  }
}

#[test]
fn a_wall_across_two_views_casts_without_a_gap() {
  // A light at (64, 8) over a wall from x 14 to 114 at y 22 to 26: its
  // shadow crosses the right, down and left views, meeting at 45° at
  // (96.5, 40.5) and (31.5, 40.5)
  let light = || {
    vec![(
      Vec2::new(64.0, 8.0),
      Light2D::point(Color::WHITE, 100.0)
        .with_intensity(0.5)
        .with_shadows(),
    )]
  };
  let wall = || vec![(Vec2::new(64.0, 24.0), LightOccluder2D::rect(100.0, 4.0))];
  let (Some(shadowed), Some(open)) = (scene(0.0, light(), wall()), scene(0.0, light(), Vec::new()))
  else {
    return;
  };
  assert!(red(&open, 96, 40) > 20 && red(&open, 31, 40) > 20);
  for y in 28..64 {
    for x in 0..128 {
      let d = Vec2::new(x as f32 + 0.5, y as f32 + 0.5).distance(Vec2::new(64.0, 8.0));
      if d < 90.0 {
        assert!(
          red(&shadowed, x, y) < 3,
          "({x}, {y}): {}",
          red(&shadowed, x, y)
        );
      }
    }
  }
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

/// A 320 x 240 white floor lit only by a 400 px light at (10, 10), at the
/// `Low` shadow quality (256 texels a view), past `wall`
fn low_quality(wall: Option<LightOccluder2D>) -> Option<image::RgbaImage> {
  let scene = Lit(Box::new(move |ctx| {
    let floor = ctx
      .assets()
      .add(Image::solid_rect(320, 240, 255, 255, 255, 255));
    ctx.world.spawn((
      Transform::from_xy(0.0, 0.0),
      Renderable::image(floor).with_anchor(Anchor::TopLeft),
    ));
    ctx.settings().set_shadow_quality(ShadowQuality::Low);
    ctx
      .lighting()
      .set_ambient(AmbientLight::with_intensity(0.0));
    ctx.world.spawn((
      Transform::from_xy(10.0, 10.0),
      Light2D::point(Color::WHITE, 400.0)
        .with_intensity(0.5)
        .with_shadows(),
    ));
    if let Some(wall) = &wall {
      ctx.world.spawn((Transform::IDENTITY, wall.clone()));
    }
  }));
  let builder = EngineBuilder::new().resolution(320, 240).start_scene(scene);
  let mut engine = HeadlessEngine::new_or_skip(builder)?;
  engine.step_frames(2).unwrap();
  Some(engine.frame().unwrap())
}

#[test]
fn a_shadow_starts_at_its_occluder_near_a_view_s_diagonal() {
  // A wall 2 px thick across the ray at 40° from the light, its back face
  // 250 px away: near the right view's end, where its texels cover the
  // least angle
  let light = Vec2::new(10.0, 10.0);
  let ray = Vec2::from_angle(40f32.to_radians());
  let across = ray.perp();
  let centre = light + ray * 249.0;
  let wall = LightOccluder2D::line(centre - across * 30.0, centre + across * 30.0, 2.0);
  let (Some(shadowed), Some(open)) = (low_quality(Some(wall)), low_quality(None)) else {
    return;
  };
  let mut checked = 0;
  for y in 0..240 {
    for x in 0..320 {
      let offset = Vec2::new(x as f32 + 0.5, y as f32 + 0.5) - light;
      let (along, off) = (offset.dot(ray), offset.dot(across).abs());
      if off > 0.75 {
        continue;
      }
      let (shown, unshadowed) = (linear(red(&shadowed, x, y)), linear(red(&open, x, y)));
      if (252.0..262.0).contains(&along) {
        // Within 2 px behind the back face it is in shadow
        assert!(
          shown < unshadowed * 0.5,
          "{along} px along: {shown} against {unshadowed}"
        );
        checked += 1;
      } else if (230.0..247.0).contains(&along) {
        assert!(
          shown > unshadowed * 0.9,
          "{along} px along, in front: {shown} against {unshadowed}"
        );
      }
    }
  }
  assert!(checked >= 10, "{checked} pixels behind the wall");
}
