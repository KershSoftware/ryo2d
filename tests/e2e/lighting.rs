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

/// A 320 x 160 white floor with no ambient light, then `setup`, after two
/// frames
fn wide(mut setup: impl FnMut(&mut SceneContext) + Send + 'static) -> Option<HeadlessEngine> {
  let scene = Lit(Box::new(move |ctx| {
    let floor = ctx
      .assets()
      .add(Image::solid_rect(320, 160, 255, 255, 255, 255));
    ctx.world.spawn((
      Transform::from_xy(0.0, 0.0),
      Renderable::image(floor).with_anchor(Anchor::TopLeft),
    ));
    ctx
      .lighting()
      .set_ambient(AmbientLight::with_intensity(0.0));
    ctx.lighting().set_enabled(true);
    setup(ctx);
  }));
  let builder = EngineBuilder::new().resolution(320, 160).start_scene(scene);
  let mut engine = HeadlessEngine::new_or_skip(builder)?;
  engine.step_frames(2).unwrap();
  Some(engine)
}

/// Moves every light to `to`, then draws two frames
fn move_lights(engine: &mut HeadlessEngine, to: Vec2) -> image::RgbaImage {
  let world = engine.world_mut();
  let mut lights = world.query_filtered::<&mut Transform, With<Light2D>>();
  for mut transform in lights.iter_mut(world) {
    transform.translation = to;
  }
  engine.step_frames(2).unwrap();
  engine.frame().unwrap()
}

/// The share of the open floor's light that reaches (x, y) in `shadowed`
/// (no ambient light, so 0 is fully shadowed)
fn share(shadowed: &image::RgbaImage, open: &image::RgbaImage, x: u32, y: u32) -> f32 {
  let lit = linear(red(open, x, y));
  if lit <= 0.0 {
    return 0.0;
  }
  linear(red(shadowed, x, y)) / lit
}

/// The shares down column `x` over `ys`, lit at the top and dark at the
/// bottom
fn column(
  shadowed: &image::RgbaImage,
  open: &image::RgbaImage,
  x: u32,
  ys: std::ops::Range<u32>,
) -> Vec<f32> {
  ys.map(|y| share(shadowed, open, x, y)).collect()
}

/// Where the shares first fall below `level`, between pixel centres, from
/// the column's first
fn crossing(shares: &[f32], level: f32) -> f32 {
  for i in 1..shares.len() {
    if shares[i] < level {
      let t = (shares[i - 1] - level) / (shares[i - 1] - shares[i]);
      return (i - 1) as f32 + t;
    }
  }
  panic!("the shares never fall below {level}: {shares:?}");
}

/// The 10%-90% width of the shadow edge down a column of shares
fn penumbra(shares: &[f32]) -> f32 {
  crossing(shares, 0.1) - crossing(shares, 0.9)
}

/// Where the edge down a column of shares lies from its first pixel, by
/// the shadow it holds: a step there holds as much (so it moves as
/// smoothly as the shadow does, whatever the profile)
fn edge_at(shares: &[f32]) -> f32 {
  assert!(
    shares[0] > 0.97 && shares[shares.len() - 1] < 0.03,
    "the column spans the edge: {shares:?}"
  );
  shares.len() as f32 - shares.iter().map(|s| 1.0 - s).sum::<f32>()
}

/// A 256 px light of source `size` at `at`, and (with `wall`) a 4 x 120
/// block from (60, 80) to (64, 200): the shadow's upper edge runs from the
/// block's top right corner away from the light
fn hardening(size: f32, at: Vec2, wall: bool) -> Option<HeadlessEngine> {
  wide(move |ctx| {
    ctx.world.spawn((
      Transform::from_xy(at.x, at.y),
      Light2D::point(Color::WHITE, 256.0).with_soft_shadows(size),
    ));
    if wall {
      ctx.world.spawn((
        Transform::from_xy(62.0, 140.0),
        LightOccluder2D::rect(4.0, 120.0),
      ));
    }
  })
}

#[test]
fn a_soft_shadow_hardens_where_it_meets_its_occluder() {
  // The light at (20, 70): the edge crosses x 84 (20 px behind the block's
  // back face) at y 84.5, and x 184 (120 px behind) at y 107.3. A source
  // 12 px in radius gives a penumbra 2 x 12 x 20 / 44 = 10.9 px wide near,
  // and far 65 px, capped at 0.35 rad (57 px at 164 px from the light)
  let at = Vec2::new(20.0, 70.0);
  let widths = |size: f32| {
    let (Some(shadowed), Some(open)) = (hardening(size, at, true), hardening(size, at, false))
    else {
      return None;
    };
    let (shadowed, open) = (shadowed.frame().unwrap(), open.frame().unwrap());
    let (near, far) = (
      column(&shadowed, &open, 84, 70..100),
      column(&shadowed, &open, 184, 70..150),
    );
    if size > 0.0 {
      // No step inside a soft edge. Near, the 12 look-ups (High) are less
      // than a pixel apart across the 10.9 px penumbra, so two may change
      // within one pixel (2 / 12): 0.2 there
      for (name, shares, most) in [("near", &near, 0.2), ("far", &far, 0.15)] {
        for (i, pair) in shares.windows(2).enumerate() {
          assert!(
            (pair[0] - pair[1]).abs() <= most,
            "{name}: a jump of {} at y {}: {shares:?}",
            pair[0] - pair[1],
            70 + i
          );
        }
      }
    }
    Some((penumbra(&near), penumbra(&far)))
  };
  let (Some((hard_near, hard_far)), Some((near, far))) = (widths(0.0), widths(12.0)) else {
    return;
  };
  println!("hard {hard_near} px near, {hard_far} px far; size 12: {near} near, {far} far");
  assert!(
    hard_near <= 2.0 && hard_far <= 2.0,
    "hard: {hard_near} near, {hard_far} far"
  );
  assert!(near > hard_near, "near: {near} against hard {hard_near}");
  assert!(near < far, "near {near}, far {far}");
}

#[test]
fn a_soft_edge_moves_smoothly_with_its_light() {
  // The light moves down from (20, 70) a quarter pixel at a time: the edge
  // through the block's corner rises 0.32 px a step at x 120 and 0.68 px
  // at x 184
  let start = Vec2::new(20.0, 70.0);
  let (Some(mut shadowed), Some(mut open)) =
    (hardening(12.0, start, true), hardening(12.0, start, false))
  else {
    return;
  };
  let mut frames = Vec::new();
  for step in 0..=8 {
    let at = start + Vec2::new(0.0, step as f32 * 0.25);
    frames.push((move_lights(&mut shadowed, at), move_lights(&mut open, at)));
  }
  for x in [120, 184] {
    let edges: Vec<f32> = frames
      .iter()
      .map(|(s, o)| edge_at(&column(s, o, x, 60..150)))
      .collect();
    for pair in edges.windows(2) {
      let moved = pair[1] - pair[0];
      assert!(
        moved < -0.05 && moved > -1.0,
        "x {x}: the edge moves {moved} px in a step: {edges:?}"
      );
    }
  }
  // No pixel goes back towards the light once the edge passes it
  for x in 70..200 {
    for y in 60..150 {
      if red(&frames[0].1, x, y) < 80 {
        continue;
      }
      let mut darkest = f32::MAX;
      for (s, o) in &frames {
        let now = share(s, o, x, y);
        assert!(
          now <= darkest + 0.05,
          "({x}, {y}) went dark and lit again: {now} after {darkest}"
        );
        darkest = darkest.min(now);
      }
    }
  }
}

/// A wall from x -96 to 416 at y 50 to 58 in 32 px chunks: as a map's
/// chunks build it (each chunk an open chain along its top and one along
/// its bottom, the chunks' borders casting nothing), or as closed 32 x 8
/// rectangles side by side
fn chunked_wall(chains: bool) -> Vec<(Vec2, LightOccluder2D)> {
  (0..16)
    .map(|i| {
      let centre = Vec2::new(-80.0 + i as f32 * 32.0, 54.0);
      let occluder = if chains {
        LightOccluder2D::chain(vec![Vec2::new(16.0, -4.0), Vec2::new(-16.0, -4.0)])
      } else {
        LightOccluder2D::rect(32.0, 8.0)
      };
      (centre, occluder)
    })
    .chain((0..16).filter(|_| chains).map(|i| {
      let centre = Vec2::new(-80.0 + i as f32 * 32.0, 54.0);
      (
        centre,
        LightOccluder2D::chain(vec![Vec2::new(-16.0, 4.0), Vec2::new(16.0, 4.0)]),
      )
    }))
    .collect()
}

#[test]
fn no_light_leaks_through_a_map_wall() {
  // A 120 px light 30 px above the wall's top, over a joint (x 128, 160),
  // a quarter pixel from one (191.75) and between them
  let positions = [
    Vec2::new(160.0, 20.0),
    Vec2::new(128.0, 20.0),
    Vec2::new(191.75, 20.0),
    Vec2::new(145.3, 20.0),
    Vec2::new(176.0, 20.0),
  ];
  let light = |size: f32, quality: ShadowQuality| {
    move |ctx: &mut SceneContext| {
      ctx.settings().set_shadow_quality(quality);
      ctx.world.spawn((
        Transform::from_xy(160.0, 20.0),
        Light2D::point(Color::WHITE, 120.0).with_soft_shadows(size),
      ));
    }
  };
  for (size, quality) in [
    (0.0, ShadowQuality::Low),
    (12.0, ShadowQuality::Low),
    (0.0, ShadowQuality::High),
    (12.0, ShadowQuality::High),
  ] {
    let Some(mut open) = wide(light(size, quality)) else {
      return;
    };
    let mut worst = 0.0f32;
    for chains in [true, false] {
      let add = light(size, quality);
      let Some(mut walled) = wide(move |ctx| {
        add(ctx);
        for (at, occluder) in chunked_wall(chains) {
          ctx.world.spawn((Transform::from_xy(at.x, at.y), occluder));
        }
      }) else {
        return;
      };
      for at in positions {
        let (shadowed, unshadowed) = (move_lights(&mut walled, at), move_lights(&mut open, at));
        let mut checked = 0;
        for y in 59..160 {
          for x in 0..320 {
            let d = Vec2::new(x as f32 + 0.5, y as f32 + 0.5).distance(at);
            if d > 110.0 || red(&unshadowed, x, y) < 20 {
              continue;
            }
            let (shown, lit) = (linear(red(&shadowed, x, y)), linear(red(&unshadowed, x, y)));
            assert!(
              shown <= 0.03 * lit,
              "size {size}, {quality:?}, chains {chains}, light at {at}: ({x}, {y}) {shown} of {lit}"
            );
            worst = worst.max(shown / lit);
            checked += 1;
          }
        }
        assert!(checked > 1000, "{checked} pixels behind the wall");
      }
    }
    println!("size {size}, {quality:?}: the worst share behind the wall is {worst}");
  }
}

#[test]
fn no_acne_on_lit_ground() {
  // A 256 px light at (20, 80) and an 8 px block at (100, 80), spanning
  // 0.0526 rad either side of the axis, its back face at x 104. Checked:
  // the ground beside the soft path's farthest reach (the block's half
  // width, half the widest filter arc and one search look-up's spacing at
  // High, 0.35 / 6), and the ground in front of the block and inside it up
  // to half a pixel before its back face (from 0.5 to 2 px before it, a
  // receiver within a few texels of the face the map holds): lit as with
  // no block
  let at = Vec2::new(20.0, 80.0);
  let half_block = (4.0f32 / 76.0).atan();
  let beside = half_block + 0.35 / 2.0 + 0.35 / 6.0;
  for size in [0.0, 12.0] {
    let scene = move |block: bool| {
      wide(move |ctx| {
        ctx.world.spawn((
          Transform::from_xy(at.x, at.y),
          Light2D::point(Color::WHITE, 256.0).with_soft_shadows(size),
        ));
        if block {
          ctx.world.spawn((
            Transform::from_xy(100.0, 80.0),
            LightOccluder2D::rect(8.0, 8.0),
          ));
        }
      })
    };
    let (Some(blocked), Some(open)) = (scene(true), scene(false)) else {
      return;
    };
    let (blocked, open) = (blocked.frame().unwrap(), open.frame().unwrap());
    let (mut beside_checked, mut within_checked) = (0, 0);
    for y in 0..160 {
      for x in 0..320 {
        let offset = Vec2::new(x as f32 + 0.5, y as f32 + 0.5) - at;
        let angle = offset.y.atan2(offset.x).abs();
        // Where the pixel's ray leaves the block: its back face, or its
        // top or bottom face near the corners
        let back = (84.0 / angle.cos()).min(4.0 / angle.sin().max(1e-6));
        let is_beside = angle > beside;
        let within = angle <= half_block && offset.length() < back - 0.5;
        if !(is_beside || within) || red(&open, x, y) < 20 {
          continue;
        }
        let (shown, lit) = (red(&blocked, x, y), red(&open, x, y));
        let ratio = linear(shown) / linear(lit);
        assert!(
          shown.abs_diff(lit) <= 1 || (ratio - 1.0).abs() <= 0.02,
          "size {size}: ({x}, {y}) {shown} against {lit}"
        );
        if is_beside {
          beside_checked += 1;
        } else {
          within_checked += 1;
        }
      }
    }
    assert!(
      beside_checked > 5000 && within_checked > 300,
      "{beside_checked} beside, {within_checked} in front of and in the block"
    );
  }
}

/// The share of the open light at `p` (between pixel centres, blended
/// from the four round it)
fn share_at(shadowed: &image::RgbaImage, open: &image::RgbaImage, p: Vec2) -> f32 {
  let q = p - Vec2::splat(0.5);
  let (x0, y0) = (q.x.floor(), q.y.floor());
  let (tx, ty) = (q.x - x0, q.y - y0);
  let at = |dx: f32, dy: f32| share(shadowed, open, (x0 + dx) as u32, (y0 + dy) as u32);
  let top = at(0.0, 0.0) * (1.0 - tx) + at(1.0, 0.0) * tx;
  let bottom = at(0.0, 1.0) * (1.0 - tx) + at(1.0, 1.0) * tx;
  top * (1.0 - ty) + bottom * ty
}

#[test]
fn a_soft_edge_crosses_a_view_boundary_smoothly() {
  // A 256 px light of size 12 at (40, 20), and a slab 6 px deep and 30
  // across whose far corner stands 56 px from the light at `angle`, the
  // slab on the side of larger angles: its shadow's edge is the ray at
  // `angle`, on the right and down views' boundary at 45°, inside the down
  // view at 50°
  let light = Vec2::new(40.0, 20.0);
  let scene = move |angle: f32, slab: bool| {
    wide(move |ctx| {
      ctx.world.spawn((
        Transform::from_xy(light.x, light.y),
        Light2D::point(Color::WHITE, 256.0).with_soft_shadows(12.0),
      ));
      if slab {
        let along = Vec2::from_angle(angle.to_radians());
        let across = along.perp();
        let corner = light + along * 50.0;
        ctx.world.spawn((
          Transform::IDENTITY,
          LightOccluder2D::polygon(vec![
            corner,
            corner + across * 30.0,
            corner + across * 30.0 + along * 6.0,
            corner + along * 6.0,
          ]),
        ));
      }
    })
  };
  // The edge's place across the ray, 110 px from the light, from 20 px on
  // its lit side, by the shadow held over 40 px
  let edge = |angle: f32| {
    let (Some(shadowed), Some(open)) = (scene(angle, true), scene(angle, false)) else {
      return None;
    };
    let (shadowed, open) = (shadowed.frame().unwrap(), open.frame().unwrap());
    let along = Vec2::from_angle(angle.to_radians());
    let shares: Vec<f32> = (-20..=20)
      .map(|s| {
        share_at(
          &shadowed,
          &open,
          light + along * 110.0 + along.perp() * s as f32,
        )
      })
      .collect();
    Some((edge_at(&shares), shadowed, open))
  };
  let (Some((diagonal, shadowed, open)), Some((off, _, _))) = (edge(45.0), edge(50.0)) else {
    return;
  };
  println!("the edge {diagonal} px along on the diagonal, {off} px at 50°");
  assert!(
    (diagonal - off).abs() <= 1.0,
    "on the diagonal {diagonal}, off it {off}"
  );
  // No step between neighbouring pixels across the penumbra, where it
  // crosses from the right view into the down view: from 100 px out, where
  // the penumbra is 19 px or more (nearer, it is narrow enough that its
  // true slope passes 0.15 a pixel)
  for y in 0..159 {
    for x in 0..319 {
      let offset = Vec2::new(x as f32 + 0.5, y as f32 + 0.5) - light;
      let d = offset.length();
      if !(100.0..150.0).contains(&d) || red(&open, x, y) < 60 {
        continue;
      }
      let here = share(&shadowed, &open, x, y);
      for (nx, ny) in [(x + 1, y), (x, y + 1)] {
        if red(&open, nx, ny) < 60 {
          continue;
        }
        let there = share(&shadowed, &open, nx, ny);
        assert!(
          (here - there).abs() <= 0.15,
          "({x}, {y}) {here} beside ({nx}, {ny}) {there}"
        );
      }
    }
  }
}

#[test]
fn a_soft_edge_over_a_far_blocker_moves_smoothly_with_its_light() {
  // A 256 px light of size 12 at (20, 80), a 4 px post 30 px from it, and
  // a wall from x 120 to 124 whose end (y 71) lies 0.09 rad off the axis,
  // just past the post's upper edge (0.071 rad): the search finds the wall
  // (the farthest blocker) for some pixels of the post's penumbra and not
  // others. The light steps a quarter pixel towards the post eight times;
  // the shadow held down x 160 (140 px out) may move by less than a pixel
  // a step (the post's edge itself moves about 0.07 px)
  let start = Vec2::new(20.0, 80.0);
  let scene = move |occluders: bool| {
    wide(move |ctx| {
      ctx.world.spawn((
        Transform::from_xy(start.x, start.y),
        Light2D::point(Color::WHITE, 256.0).with_soft_shadows(12.0),
      ));
      if occluders {
        ctx.world.spawn((
          Transform::from_xy(50.0, 80.0),
          LightOccluder2D::rect(4.0, 4.0),
        ));
        ctx.world.spawn((
          Transform::from_xy(122.0, 15.5),
          LightOccluder2D::rect(4.0, 111.0),
        ));
      }
    })
  };
  let (Some(mut shadowed), Some(mut open)) = (scene(true), scene(false)) else {
    return;
  };
  let mut held = Vec::new();
  for step in 0..=8 {
    let at = start + Vec2::new(step as f32 * 0.25, 0.0);
    let (s, o) = (move_lights(&mut shadowed, at), move_lights(&mut open, at));
    held.push(
      column(&s, &o, 160, 30..110)
        .iter()
        .map(|f| 1.0 - f)
        .sum::<f32>(),
    );
  }
  let worst = held
    .windows(2)
    .map(|pair| (pair[1] - pair[0]).abs())
    .fold(0.0f32, f32::max);
  println!("the shadow held moves at most {worst} px a step: {held:?}");
  assert!(worst < 1.0, "{held:?}");
}

#[test]
fn the_atlas_grows_and_changes_quality_between_frames() {
  // A soft 256 px light at (16, 32) and a 4 x 8 wall above its row or
  // below it: 30 px behind the wall its shadow is about 16 px across,
  // centred at (70, 15) or (70, 49)
  let scene = Lit(Box::new(|ctx: &mut SceneContext| {
    ctx
      .lighting()
      .set_ambient(AmbientLight::with_intensity(0.0));
    ctx.world.spawn((
      Transform::from_xy(16.0, 32.0),
      Light2D::point(Color::WHITE, 256.0)
        .with_intensity(0.5)
        .with_soft_shadows(4.0),
    ));
    ctx.world.spawn((
      Transform::from_xy(40.0, 24.0),
      LightOccluder2D::rect(4.0, 8.0),
    ));
  }));
  let builder = EngineBuilder::new().resolution(128, 64).start_scene(scene);
  let Some(mut engine) = HeadlessEngine::new_or_skip(builder) else {
    return;
  };
  let move_wall = |engine: &mut HeadlessEngine, above: bool| {
    let world = engine.world_mut();
    let mut walls = world.query_filtered::<&mut Transform, With<LightOccluder2D>>();
    for mut transform in walls.iter_mut(world) {
      transform.translation = Vec2::new(40.0, if above { 24.0 } else { 40.0 });
    }
  };
  let set_quality = |engine: &mut HeadlessEngine, quality: ShadowQuality| {
    engine
      .world_mut()
      .resource_mut::<Settings>()
      .set_shadow_quality(quality);
  };
  // The frame's shadow falls behind the wall where it is now, and only there
  let check = |engine: &mut HeadlessEngine, above: bool, phase: &str| {
    engine.step_frames(1).unwrap();
    let frame = engine.frame().unwrap();
    let (dark, lit) = if above {
      ((70, 15), (70, 49))
    } else {
      ((70, 49), (70, 15))
    };
    assert!(
      red(&frame, dark.0, dark.1) < 3 && red(&frame, lit.0, lit.1) > 40,
      "{phase}: behind the wall {}, where it was {}",
      red(&frame, dark.0, dark.1),
      red(&frame, lit.0, lit.1)
    );
  };
  check(&mut engine, true, "one light");
  // 299 more shadowed lights, faint and far from the pixels read
  for _ in 0..299 {
    engine.world_mut().spawn((
      Transform::from_xy(120.0, 4.0),
      Light2D::point(Color::WHITE, 4.0)
        .with_intensity(0.001)
        .with_shadows(),
    ));
  }
  move_wall(&mut engine, false);
  check(&mut engine, false, "300 lights");
  check(&mut engine, false, "300 lights, the next frame");
  set_quality(&mut engine, ShadowQuality::Low);
  move_wall(&mut engine, true);
  check(&mut engine, true, "low");
  set_quality(&mut engine, ShadowQuality::Ultra);
  move_wall(&mut engine, false);
  check(&mut engine, false, "ultra");
  // Settled: the frames after make no GPU resource
  let builds = engine.shadow_atlas_builds();
  assert!(builds >= 3, "the atlas grew and changed twice: {builds}");
  for _ in 0..3 {
    check(&mut engine, false, "settled");
  }
  assert_eq!(
    engine.shadow_atlas_builds(),
    builds,
    "a settled frame built"
  );
}

/// The 128 x 64 floor with no ambient light, a hard shadowed light at
/// (16, 16) and a 4 x 8 wall at (40, 16); once `go` is set, its next
/// `update` moves both 32 px down
struct MovedInUpdate(std::sync::Arc<std::sync::atomic::AtomicBool>);

impl Scene for MovedInUpdate {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    let floor = ctx
      .assets()
      .add(Image::solid_rect(128, 64, 255, 255, 255, 255));
    ctx.world.spawn((
      Transform::from_xy(0.0, 0.0),
      Renderable::image(floor).with_anchor(Anchor::TopLeft),
    ));
    ctx
      .lighting()
      .set_ambient(AmbientLight::with_intensity(0.0));
    ctx.world.spawn((
      Transform::from_xy(16.0, 16.0),
      Light2D::point(Color::WHITE, 256.0)
        .with_intensity(0.5)
        .with_shadows(),
    ));
    ctx.world.spawn((
      Transform::from_xy(40.0, 16.0),
      LightOccluder2D::rect(4.0, 8.0),
    ));
  }

  fn update(&mut self, ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    if self.0.swap(false, std::sync::atomic::Ordering::Relaxed) {
      let world = &mut *ctx.world;
      let mut moved =
        world.query_filtered::<&mut Transform, Or<(With<Light2D>, With<LightOccluder2D>)>>();
      for mut transform in moved.iter_mut(world) {
        transform.translation.y += 32.0;
      }
    }
    SceneAction::Continue
  }
}

#[test]
fn an_occluder_moved_this_frame_casts_where_it_now_is() {
  let go = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
  let builder = EngineBuilder::new()
    .resolution(128, 64)
    .start_scene(MovedInUpdate(go.clone()));
  let Some(mut engine) = HeadlessEngine::new_or_skip(builder) else {
    return;
  };
  engine.step_frames(2).unwrap();
  let before = engine.frame().unwrap();
  // Behind the wall at y 16 is dark, y 48 lit
  assert!(
    red(&before, 70, 16) < 3 && red(&before, 70, 48) > 40,
    "before: {} at y 16, {} at y 48",
    red(&before, 70, 16),
    red(&before, 70, 48)
  );
  // Moved in this frame's update: the index updates after propagation and
  // the plan reads it as the frame is drawn
  go.store(true, std::sync::atomic::Ordering::Relaxed);
  engine.step_frames(1).unwrap();
  let after = engine.frame().unwrap();
  assert!(
    red(&after, 70, 48) < 3 && red(&after, 70, 16) > 40,
    "the same frame: {} behind the wall at y 48, {} where it was",
    red(&after, 70, 48),
    red(&after, 70, 16)
  );
}

#[test]
fn past_512_shadowed_lights_the_nearest_keep_their_shadows() {
  let log = ryo2d::testing::capture_log();
  // A 320 x 240 white floor with no ambient light, its camera centred at
  // (160, 120). 600 shadowed lights in view: 510 faint 1 px ones at the
  // centre; two test lights 60 px out (ranks 510 and 511, the last kept)
  // and two 120 px out (512 and 513, the first dropped), each 30 px in
  // radius with a wall 10 px to its right and the point 22 px to its
  // right behind it, which no other light reaches; and 86 faint ones at
  // the corners, about 195 px out. Spawned out of distance order (the
  // corners, the far test lights, the centre, the near test lights), so
  // the nearest 512 are not the first 512 in light order: a cut by light
  // index would shadow the far ones and leave the near ones unshadowed
  let near = [Vec2::new(100.0, 120.0), Vec2::new(220.0, 120.0)];
  let far = [Vec2::new(40.0, 120.0), Vec2::new(280.0, 120.0)];
  let scene = Lit(Box::new(move |ctx: &mut SceneContext| {
    let floor = ctx
      .assets()
      .add(Image::solid_rect(320, 240, 255, 255, 255, 255));
    ctx.world.spawn((
      Transform::from_xy(0.0, 0.0),
      Renderable::image(floor).with_anchor(Anchor::TopLeft),
    ));
    ctx
      .lighting()
      .set_ambient(AmbientLight::with_intensity(0.0));
    let faint = || {
      Light2D::point(Color::WHITE, 1.0)
        .with_intensity(0.001)
        .with_shadows()
    };
    let corners = [(4.0, 4.0), (316.0, 4.0), (4.0, 236.0), (316.0, 236.0)];
    for i in 0..86 {
      let (x, y) = corners[i % 4];
      ctx.world.spawn((Transform::from_xy(x, y), faint()));
    }
    let tested = |ctx: &mut SceneContext, at: Vec2| {
      ctx.world.spawn((
        Transform::from_xy(at.x, at.y),
        Light2D::point(Color::WHITE, 30.0).with_shadows(),
      ));
      ctx.world.spawn((
        Transform::from_xy(at.x + 10.0, at.y),
        LightOccluder2D::rect(4.0, 12.0),
      ));
    };
    for at in far {
      tested(ctx, at);
    }
    for _ in 0..510 {
      ctx.world.spawn((Transform::from_xy(160.0, 120.0), faint()));
    }
    for at in near {
      tested(ctx, at);
    }
  }));
  let builder = EngineBuilder::new().resolution(320, 240).start_scene(scene);
  let Some(mut engine) = HeadlessEngine::new_or_skip(builder) else {
    return;
  };
  for frame in 0..10 {
    engine.step_frames(1).unwrap();
    let image = engine.frame().unwrap();
    // A pixel x covers x to x + 1, its centre at x + 0.5, so the pixels
    // at -23 and +22 have their centres 22.5 px either side of the light:
    // the same distance, so an unshadowed light lights them alike (-22
    // and +22 would read 21.5 and 22.5 px out, about 10 bytes apart)
    let at = |p: Vec2, dx: f32| red(&image, (p.x + dx) as u32, p.y as u32);
    for light in near {
      // Lit on its open side, shadowed behind its wall
      assert!(
        at(light, -23.0) > 20 && at(light, 22.0) < 3,
        "frame {frame}: the light at {light} kept its shadow: {} open, {} behind its wall",
        at(light, -23.0),
        at(light, 22.0)
      );
    }
    for light in far {
      assert!(
        at(light, -23.0) > 20 && at(light, 22.0).abs_diff(at(light, -23.0)) <= 2,
        "frame {frame}: the light at {light} draws unshadowed: {} open, {} behind its wall",
        at(light, -23.0),
        at(light, 22.0)
      );
    }
  }
  let warnings = log.warnings();
  assert_eq!(warnings.len(), 1, "one warning in 10 frames: {warnings:?}");
  assert!(
    warnings[0].contains("88"),
    "it names the count: {warnings:?}"
  );
}

#[test]
fn a_long_wall_partly_in_a_spot_s_cone_casts() {
  // A 30° spot at (10, 80) pointing right, and a wall from x 100 to 110
  // and y 60 to 480: its far face crosses the beam (from -10° to 75°
  // round the light) though its middle is 60° off the axis
  let spot = |wall: bool| {
    wide(move |ctx| {
      ctx.world.spawn((
        Transform::from_xy(10.0, 80.0),
        Light2D::spot(Color::WHITE, 300.0, 30.0, 0.0).with_shadows(),
      ));
      if wall {
        ctx.world.spawn((
          Transform::from_xy(105.0, 270.0),
          LightOccluder2D::rect(10.0, 420.0),
        ));
      }
    })
  };
  let (Some(walled), Some(open)) = (spot(true), spot(false)) else {
    return;
  };
  let (walled, open) = (walled.frame().unwrap(), open.frame().unwrap());
  for (x, y) in [(150, 80), (250, 80), (200, 70)] {
    assert!(red(&open, x, y) > 20, "the beam reaches ({x}, {y})");
    assert!(
      share(&walled, &open, x, y) < 0.03,
      "behind the wall ({x}, {y}): {} against {}",
      red(&walled, x, y),
      red(&open, x, y)
    );
  }
  // In the beam in front of the wall: lit as with no wall
  assert!(red(&open, 60, 80) > 20);
  assert!(
    share(&walled, &open, 60, 80) > 0.97,
    "in front: {} against {}",
    red(&walled, 60, 80),
    red(&open, 60, 80)
  );
}

#[test]
fn a_wide_volumetric_spot_s_scatter_is_shadowed_behind_a_wall() {
  // A 300° volumetric spot at (250, 80) pointing right: past its 150° half
  // angle its scatter alone reaches on, out to 1.2 times it. A pixel 165°
  // off its axis, 150 px out, and an 8 px block halfway to it (162° to
  // 168° round), where only the scatter reaches
  let at = Vec2::new(250.0, 80.0);
  let ray = Vec2::from_angle(165f32.to_radians());
  let (block, pixel) = (at + ray * 75.0, at + ray * 150.0);
  let spot = move |wall: bool| {
    wide(move |ctx| {
      ctx.world.spawn((
        Transform::from_xy(at.x, at.y),
        Light2D::spot(Color::WHITE, 300.0, 300.0, 0.0)
          .with_volumetric_settings(VolumetricSettings {
            enabled: true,
            intensity: 0.01,
            density: 0.5,
            scattering: 1.0,
            god_rays: false,
          })
          .with_shadows(),
      ));
      if wall {
        ctx.world.spawn((
          Transform::from_xy(block.x, block.y),
          LightOccluder2D::rect(8.0, 8.0),
        ));
      }
    })
  };
  let (Some(walled), Some(open)) = (spot(true), spot(false)) else {
    return;
  };
  let (walled, open) = (walled.frame().unwrap(), open.frame().unwrap());
  let (x, y) = (pixel.x as u32, pixel.y as u32);
  assert!(
    red(&open, x, y) > 20,
    "the scatter reaches ({x}, {y}): {}",
    red(&open, x, y)
  );
  assert!(
    share(&walled, &open, x, y) < 0.03,
    "behind the block ({x}, {y}): {} against {}",
    red(&walled, x, y),
    red(&open, x, y)
  );
}
