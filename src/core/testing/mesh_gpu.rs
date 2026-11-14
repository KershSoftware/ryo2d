//! Meshes on the GPU in a running engine: an entity's own triangles of an
//! image, among the entities' draws and drawn as a sprite of the image is.
//! Nothing public draws a mesh of a test's own making, so their pixels are
//! checked here, through a `MeshProbe` on an entity.

use super::HeadlessEngine;
use crate::core::asset::MemorySource;
use crate::core::ecs::Bundle;
use crate::core::engine::EngineBuilder;
use crate::core::logging::capture_log;
use crate::core::render::{DrawItem, DrawPayload, MeshProbe};
use crate::prelude::*;

const RED: [u8; 4] = [220, 30, 40, 255];
const GREEN: [u8; 4] = [30, 200, 60, 255];
const BLUE: [u8; 4] = [30, 60, 230, 255];

/// Whether a draw list item is a mesh
fn is_mesh(item: &DrawItem) -> bool {
  matches!(item.payload, DrawPayload::Mesh(_))
}

/// Where the indices of each of the last frame's mesh items start and
/// end in the mesh stream, the lit ones in draw order and then the unlit
/// ones
fn mesh_ranges(engine: &HeadlessEngine) -> Vec<(u32, u32)> {
  let list = engine.engine.draw_list();
  let ranges = |items: &[DrawItem]| -> Vec<(u32, u32)> {
    items
      .iter()
      .filter_map(|item| match item.payload {
        DrawPayload::Mesh(index) => list.mesh(index),
        _ => None,
      })
      .map(|mesh| (mesh.indices.start, mesh.indices.end))
      .collect()
  };
  [ranges(list.items()), ranges(list.unlit_items())].concat()
}

/// A scene that spawns its entities in `on_enter` and idles.
struct Fixture {
  setup: fn(&mut SceneContext),
}

impl Scene for Fixture {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    (self.setup)(ctx);
  }
}

fn engine(builder: EngineBuilder) -> Option<HeadlessEngine> {
  HeadlessEngine::new_or_skip(builder)
}

/// An image whose every texel has a colour of its own; a `soft` one fades
/// toward its edges, its outer texels clear
fn patterned(width: u32, height: u32, soft: bool) -> Image {
  let mut texels = Vec::with_capacity((width * height * 4) as usize);
  for y in 0..height {
    for x in 0..width {
      let edge = x.min(y).min(width - 1 - x).min(height - 1 - y);
      let alpha = if soft { (edge * 37).min(255) } else { 255 };
      let [r, g, b] = [40 + x * 9, 30 + y * 13, 90 + x * 5 + y * 7];
      texels.extend([r, g, b, alpha].map(|channel| (channel % 256) as u8));
    }
  }
  Image::from_rgba(texels, width, height)
}

/// A two-triangle mesh over `rect` of its entity's image, `size` world
/// units about the entity, its UVs from `from` to `to` across `rect`. Cut
/// along the diagonal the batch's unit quad is cut along, or with `recut`
/// along the other; `mirrored` swaps its left and right vertices' places,
/// so the image is mirrored and its triangles wind the other way
fn quad_mesh(
  rect: [u32; 4],
  size: Vec2,
  (from, to): (Vec2, Vec2),
  (mirrored, recut): (bool, bool),
) -> MeshProbe {
  let half = size / 2.0;
  let x = if mirrored { -half.x } else { half.x };
  MeshProbe {
    rect,
    vertices: vec![
      (Vec2::new(-x, -half.y), from),
      (Vec2::new(x, -half.y), Vec2::new(to.x, from.y)),
      (Vec2::new(-x, half.y), Vec2::new(from.x, to.y)),
      (Vec2::new(x, half.y), to),
    ],
    indices: if recut {
      vec![0, 1, 3, 0, 3, 2]
    } else {
      vec![0, 1, 2, 1, 3, 2]
    },
  }
}

/// [`quad_mesh`] over all of a `width` by `height` image, at its size
fn whole_mesh(width: u32, height: u32, mirrored: bool, recut: bool) -> MeshProbe {
  let size = Vec2::new(width as f32, height as f32);
  quad_mesh(
    [0, 0, width, height],
    size,
    (Vec2::ZERO, Vec2::ONE),
    (mirrored, recut),
  )
}

/// How two frames differ, if they do: at how many pixels, and where and
/// how at the first
fn difference(a: &image::RgbaImage, b: &image::RgbaImage) -> Option<String> {
  let mut differing = a
    .enumerate_pixels()
    .zip(b.pixels())
    .filter(|((_, _, a), b)| a != b);
  let ((x, y, one), other) = differing.next()?;
  let count = 1 + differing.count();
  Some(format!(
    "{count} pixels differ, the first at ({x}, {y}): {:?} in the first, {:?} in the second",
    one.0, other.0
  ))
}

/// A stage for comparing a sprite with a mesh, 96 by 64: a backdrop over
/// its left half, so what is drawn at its middle blends over two colours,
/// and a look that is clamped, not curved
fn stage() -> Fixture {
  Fixture {
    setup: |ctx| {
      ctx.look().tonemap = Tonemap::Clamp;
      let backdrop = ctx
        .assets()
        .add(Image::solid_rect(48, 64, 90, 110, 130, 255));
      ctx.world.spawn((
        Transform::from_xy(24.0, 32.0),
        Renderable::image(backdrop).with_z_order(-1),
      ));
    },
  }
}

/// The stage on a smooth target, and on a pixel-art one, where a sprite's
/// quad is snapped to whole pixels and a mesh's triangles are not: both
/// stand on whole pixels in these tests
fn stages() -> [(&'static str, EngineBuilder); 2] {
  let files = MemorySource::new;
  [
    (
      "smooth",
      EngineBuilder::new()
        .resolution(96, 64)
        .asset_source(files()),
    ),
    (
      "pixel art",
      EngineBuilder::new().pixel_art(96, 64).asset_source(files()),
    ),
  ]
}

/// The frame with `subject` drawn at the stage's middle for `frames` frames
fn frame_of(engine: &mut HeadlessEngine, subject: impl Bundle, frames: u32) -> image::RgbaImage {
  let at = Transform::from_xy(48.0, 32.0);
  let subject = engine.world_mut().spawn((at, subject)).id();
  engine.step_frames(frames).unwrap();
  let frame = engine.frame().unwrap();
  engine.world_mut().despawn(subject);
  frame
}

/// What a test's cases got wrong, each named: a test goes through every
/// case and then fails with them all
#[derive(Default)]
struct Wrong(Vec<String>);

impl Wrong {
  /// Notes `what` unless every pixel of `one` and `other` is equal
  fn unless_equal(&mut self, what: &str, one: &image::RgbaImage, other: &image::RgbaImage) {
    if let Some(differs) = difference(one, other) {
      self.0.push(format!("{what}: {differs}"));
    }
  }

  /// Notes `what` unless `one` and `other` differ somewhere
  fn unless_different(&mut self, what: &str, one: &image::RgbaImage, other: &image::RgbaImage) {
    if difference(one, other).is_none() {
      self.0.push(what.to_string());
    }
  }

  /// Fails the test if any case was noted
  fn finish(self) {
    assert!(self.0.is_empty(), "\n{}", self.0.join("\n"));
  }
}

/// Draws `subject` (a renderable, and what else its entity has) at the
/// stage's middle as a sprite, then as `mesh` on the same entity, each for
/// `frames` frames, and notes `what` in `wrong` unless every pixel of the
/// two frames is equal. The sprite's frame
fn sprite_then_mesh(
  (engine, wrong): (&mut HeadlessEngine, &mut Wrong),
  what: &str,
  subject: impl Bundle,
  mesh: MeshProbe,
  frames: u32,
) -> image::RgbaImage {
  let at = Transform::from_xy(48.0, 32.0);
  let subject = engine.world_mut().spawn((at, subject)).id();
  engine.step_frames(frames).unwrap();
  let sprite = engine.frame().unwrap();
  engine.world_mut().entity_mut(subject).insert(mesh);
  engine.step_frames(frames).unwrap();
  let drawn = engine.frame().unwrap();
  engine.world_mut().despawn(subject);
  let what = format!("{what}: the sprite's frame and the mesh's");
  wrong.unless_equal(&what, &sprite, &drawn);
  sprite
}

/// `image`, added to the engine's world
fn add_image(engine: &mut HeadlessEngine, image: Image) -> Handle<Image> {
  engine
    .world_mut()
    .resource_mut::<Assets<Image>>()
    .add(image)
}

#[test]
fn a_two_triangle_mesh_draws_as_the_sprite_of_its_image() {
  /// A case's image, how its renderable differs from a plain one, and how
  /// its mesh is made: mirrored, recut
  struct Case {
    name: &'static str,
    size: (u32, u32),
    soft: bool,
    look: fn(Renderable) -> Renderable,
    mesh: (bool, bool),
  }
  const SIZE: (u32, u32) = (24, 16);
  let tinted = |renderable: Renderable| {
    renderable
      .with_tint(Color::rgb(200, 120, 255))
      .with_alpha(0.5)
  };
  let case = |name, soft, look, mesh| Case {
    name,
    size: SIZE,
    soft,
    look,
    mesh,
  };
  let plain = (false, false);
  let cases = [
    case("opaque", false, |renderable| renderable, plain),
    case("soft edges", true, |renderable| renderable, plain),
    case(
      "soft edges, tinted and half-transparent",
      true,
      tinted,
      plain,
    ),
    case("cut along the other diagonal", true, tinted, (false, true)),
    case(
      "mirrored, its triangles wound the other way",
      true,
      |renderable| renderable.with_flip_x(true).with_alpha(0.75),
      (true, false),
    ),
    case(
      "a premultiplied image",
      true,
      |renderable| {
        renderable
          .with_blend(BlendMode::Premultiplied)
          .with_tint(Color::rgb(255, 200, 150))
          .with_alpha(0.5)
      },
      plain,
    ),
    case(
      "adding light",
      true,
      |renderable| renderable.with_blend(BlendMode::Additive).with_alpha(0.5),
      plain,
    ),
    case(
      "multiplying",
      true,
      |renderable| renderable.with_blend(BlendMode::Multiply).with_alpha(0.75),
      plain,
    ),
    case(
      "screening",
      true,
      |renderable| renderable.with_blend(BlendMode::Screen).with_alpha(0.75),
      plain,
    ),
    case(
      "glowing",
      true,
      |renderable| renderable.with_emission(1.5).with_alpha(0.5),
      plain,
    ),
    case(
      "sampled nearest",
      true,
      |renderable| renderable.with_filter(Filter::Nearest).with_alpha(0.5),
      plain,
    ),
    // Over the atlas's 256 px, so on a page of its own
    Case {
      size: (300, 20),
      ..case("an image on a page of its own", true, tinted, plain)
    },
  ];
  let mut wrong = Wrong::default();
  for (target, builder) in stages() {
    let Some(mut engine) = engine(builder.start_scene(stage())) else {
      return;
    };
    engine.step_frames(2).unwrap();
    let empty = engine.frame().unwrap();
    for case in &cases {
      let (width, height) = case.size;
      let image = add_image(&mut engine, patterned(width, height, case.soft));
      let (mirrored, recut) = case.mesh;
      let what = format!("{target}, {}", case.name);
      let sprite = sprite_then_mesh(
        (&mut engine, &mut wrong),
        &what,
        (case.look)(Renderable::image(image)),
        whole_mesh(width, height, mirrored, recut),
        2,
      );
      let nothing = format!("{what}: the sprite draws nothing");
      wrong.unless_different(&nothing, &empty, &sprite);
    }
  }
  wrong.finish();
}

#[test]
fn a_mesh_under_an_effect_in_place_draws_as_the_sprite_under_it() {
  let effects = [
    (
      "a flash",
      SpriteEffect::flash(Color::rgb(255, 240, 200), 0.6),
    ),
    (
      "a colour shift",
      SpriteEffect::color_shift(Color::rgb(40, 90, 255), 0.5),
    ),
    (
      "a silhouette",
      SpriteEffect::silhouette(Color::new(20, 30, 60, 200)),
    ),
    ("grayscale", SpriteEffect::grayscale(0.7)),
    ("inverted", SpriteEffect::invert()),
    ("pixelated", SpriteEffect::pixelate(4)),
    ("a still wave", SpriteEffect::wave(3.0, 2.0, 0.0)),
    (
      "dissolved part-way, with an edge",
      SpriteEffect::dissolve_with_edge(0.4, Color::rgb(255, 120, 0)),
    ),
  ];
  let mut wrong = Wrong::default();
  for (target, builder) in stages() {
    let Some(mut engine) = engine(builder.start_scene(stage())) else {
      return;
    };
    let image = add_image(&mut engine, patterned(24, 16, true));
    let look = |renderable: Renderable| {
      renderable
        .with_tint(Color::rgb(230, 200, 255))
        .with_alpha(0.75)
    };
    let plain = frame_of(&mut engine, look(Renderable::image(image.clone())), 2);
    for (name, effect) in &effects {
      let shaded =
        look(Renderable::image(image.clone())).with_shader(SpriteShader::BuiltIn(effect.clone()));
      let what = format!("{target}, {name}");
      let sprite = sprite_then_mesh(
        (&mut engine, &mut wrong),
        &what,
        shaded,
        whole_mesh(24, 16, false, false),
        2,
      );
      let nothing = format!("{what}: the effect shows nothing");
      wrong.unless_different(&nothing, &plain, &sprite);
    }
  }
  wrong.finish();
}

/// The smallest game shader the engine's tests draw with
const INVERT: &str = "
fn sprite(in: SpriteIn, params: Params) -> vec4<f32> {
  let c = sample(in.uv);
  return vec4<f32>(1.0 - c.rgb, c.a);
}
";

/// A game shader that reads where it is in its sprite, the sprite's size
/// and its own parameters
const PLACED: &str = "
struct Params { level: f32, shade: vec4<f32> }
fn sprite(in: SpriteIn, params: Params) -> vec4<f32> {
  let c = sample(in.uv);
  let across = in.uv.x * params.level + in.size.y / 64.0;
  return vec4<f32>(across, in.uv.y, c.b, c.a) * params.shade;
}
";

#[test]
fn a_game_s_sprite_shader_draws_a_mesh_as_it_draws_the_sprite() {
  let mut wrong = Wrong::default();
  for (target, builder) in stages() {
    let Some(mut engine) = engine(builder.start_scene(stage())) else {
      return;
    };
    let image = add_image(&mut engine, patterned(24, 16, true));
    let mut shaders = engine.world_mut().resource_mut::<Assets<Shader>>();
    let invert = shaders.add(Shader::from_wgsl("mesh-invert.wgsl", INVERT));
    let placed = shaders.add(Shader::from_wgsl("mesh-placed.wgsl", PLACED));
    let look = |renderable: Renderable| renderable.with_alpha(0.75);
    let plain = frame_of(&mut engine, look(Renderable::image(image.clone())), 2);
    let placed = SpriteShader::custom(placed)
      .with_param("level", 0.5)
      .with_param("shade", Vec4::new(1.0, 0.75, 0.5, 1.0));
    let inline = [
      (
        "inverted by a game's shader",
        SpriteShader::custom(invert.clone()),
      ),
      ("a game's shader reading its place and parameters", placed),
    ];
    // A game shader's pipeline draws plain until the GPU has checked it:
    // ten frames each, as the shader tests give one
    for (name, shader) in inline {
      let what = format!("{target}, {name}");
      let shaded = look(Renderable::image(image.clone())).with_shader(shader);
      let sprite = sprite_then_mesh(
        (&mut engine, &mut wrong),
        &what,
        shaded,
        whole_mesh(24, 16, false, false),
        10,
      );
      let nothing = format!("{what}: the shader shows nothing");
      wrong.unless_different(&nothing, &plain, &sprite);
    }
    // And as a `Material`, which wins over the renderable's own
    let what = format!("{target}, a game's shader as a material");
    let subject = (
      look(Renderable::image(image.clone())),
      Material::custom(invert),
    );
    let sprite = sprite_then_mesh(
      (&mut engine, &mut wrong),
      &what,
      subject,
      whole_mesh(24, 16, false, false),
      10,
    );
    let nothing = format!("{what}: the shader shows nothing");
    wrong.unless_different(&nothing, &plain, &sprite);
  }
  wrong.finish();
}

#[test]
fn an_effect_that_grows_a_quad_leaves_a_mesh_plain_and_warns_once() {
  let log = capture_log();
  let (_, builder) = stages().into_iter().next().unwrap();
  let Some(mut engine) = engine(builder.start_scene(stage())) else {
    return;
  };
  engine.step_frames(2).unwrap();
  let empty = engine.frame().unwrap();
  let image = add_image(&mut engine, patterned(24, 16, true));
  let mesh = || whole_mesh(24, 16, false, false);
  let plain = frame_of(&mut engine, (Renderable::image(image.clone()), mesh()), 2);
  assert!(difference(&empty, &plain).is_some(), "the plain mesh shows");
  assert_eq!(log.containing("grows"), 0, "{:?}", log.warnings());
  let (white, black) = (Color::WHITE, Color::rgb(0, 0, 0));
  let effects = [
    ("an outline", SpriteEffect::outline(white, 2)),
    ("a glow", SpriteEffect::glow(Color::rgb(255, 255, 0), 3.0)),
    (
      "a directional outline",
      SpriteEffect::directional_outline(white, black, 2),
    ),
  ];
  let mut wrong = Wrong::default();
  for (name, effect) in effects {
    let shaded = Renderable::image(image.clone()).with_shader(SpriteShader::BuiltIn(effect));
    let drawn = frame_of(&mut engine, (shaded, mesh()), 3);
    let what = format!("{name}: the plain mesh's frame and the mesh's under it");
    wrong.unless_equal(&what, &plain, &drawn);
  }
  // One warning for them all, however many frames and meshes
  let warned = log.containing("grows");
  if warned != 1 {
    let warnings = log.warnings();
    wrong
      .0
      .push(format!("{warned} warnings, not one: {warnings:?}"));
  }
  wrong.finish();
}

#[test]
fn two_meshes_with_different_effects_draw_in_order_each_with_its_own() {
  const WHITE: [u8; 4] = [255, 255, 255, 255];
  // Three 20 px meshes of one red image at one z, each over the right half
  // of the one before, in entity order: flashed white, a green silhouette,
  // and plain
  let scene = Fixture {
    setup: |ctx| {
      ctx.look().tonemap = Tonemap::Clamp;
      let [r, g, b, a] = RED;
      let red = ctx.assets().add(Image::solid_rect(20, 20, r, g, b, a));
      let [r, g, b, _] = GREEN;
      let effects = [
        Some(SpriteEffect::flash(Color::WHITE, 1.0)),
        Some(SpriteEffect::silhouette(Color::rgb(r, g, b))),
        None,
      ];
      for (i, effect) in effects.into_iter().enumerate() {
        let mut renderable = Renderable::image(red.clone());
        if let Some(effect) = effect {
          renderable = renderable.with_shader(SpriteShader::BuiltIn(effect));
        }
        ctx.world.spawn((
          Transform::from_xy(20.0 + i as f32 * 10.0, 20.0),
          renderable,
          whole_mesh(20, 20, false, false),
        ));
      }
    },
  };
  let Some(mut engine) = engine(EngineBuilder::new().resolution(64, 40).start_scene(scene)) else {
    return;
  };
  engine.step_frames(2).unwrap();
  let frame = engine.frame().unwrap();
  let at = |x: u32| frame.get_pixel(x, 20).0;
  assert_eq!(at(15), WHITE, "the first mesh, flashed");
  assert_eq!(at(25), GREEN, "the second, a silhouette, over the first");
  assert_eq!(at(35), RED, "the third, plain, over the second");
  assert_eq!(at(45), RED, "the third");
  // Each a mesh item of its own, in the order pushed
  assert_eq!(mesh_ranges(&engine), [(0, 6), (6, 12), (12, 18)]);
}

#[test]
fn a_mesh_of_a_rectangle_draws_as_a_region_quad_of_it() {
  use crate::core::skeleton::{Attachment, Region, RegionAttachment};
  use std::sync::Arc;
  // An odd rectangle of a packed image, so its UVs are no round fractions
  const RECT: [u32; 4] = [13, 5, 16, 12];
  let size = Vec2::new(RECT[2] as f32, RECT[3] as f32);
  let tinted = |renderable: Renderable| {
    renderable
      .with_tint(Color::rgb(200, 120, 255))
      .with_alpha(0.5)
  };
  let dissolved = |renderable: Renderable| {
    let edge = Color::rgb(255, 120, 0);
    renderable.with_shader(SpriteShader::BuiltIn(SpriteEffect::dissolve_with_edge(
      0.4, edge,
    )))
  };
  let pixelated = |renderable: Renderable| {
    renderable.with_shader(SpriteShader::BuiltIn(SpriteEffect::pixelate(3)))
  };
  /// How a case's renderable differs from a plain one
  type Look = fn(Renderable) -> Renderable;
  let looks: [(&str, Look); 3] = [
    ("tinted and half-transparent", tinted),
    ("dissolved part-way", dissolved),
    ("pixelated", pixelated),
  ];
  let mut wrong = Wrong::default();
  for (target, builder) in stages() {
    let Some(mut engine) = engine(builder.start_scene(stage())) else {
      return;
    };
    let packed = add_image(&mut engine, patterned(40, 24, true));
    // A skeleton whose one slot shows the rectangle as a region, centred
    // on its root
    let setup = SlotSetup {
      attachment: Some(Arc::from("part")),
      ..SlotSetup::default()
    };
    let data = SkeletonData::builder()
      .bone("root", None, BoneSetup::default())
      .region(Region {
        name: Arc::from("part"),
        image: packed.clone(),
        x: RECT[0],
        y: RECT[1],
        width: RECT[2],
        height: RECT[3],
      })
      .slot("part", "root", setup)
      .attachment(
        SkeletonData::DEFAULT_SKIN,
        "part",
        "part",
        Attachment::Region(RegionAttachment::new(0, size.x, size.y)),
      )
      .build()
      .expect("a valid skeleton");
    let data = engine
      .world_mut()
      .resource_mut::<Assets<SkeletonData>>()
      .add(data);
    engine.step_frames(2).unwrap();
    let empty = engine.frame().unwrap();
    for (name, look) in looks {
      // A skeleton's entity is interpolated: it draws where it stands from
      // its second frame
      let hero = (Skeleton::new(data.clone()), look(Renderable::skeleton()));
      let region = frame_of(&mut engine, hero, 4);
      let whole = (Vec2::ZERO, Vec2::ONE);
      let mesh = quad_mesh(RECT, size, whole, (false, false));
      let drawn = frame_of(
        &mut engine,
        (look(Renderable::image(packed.clone())), mesh),
        4,
      );
      let what = format!("{target}, {name}");
      let nothing = format!("{what}: the region draws nothing");
      wrong.unless_different(&nothing, &empty, &region);
      let frames = format!("{what}: the region's frame and the mesh's");
      wrong.unless_equal(&frames, &region, &drawn);
    }
  }
  wrong.finish();
}

#[test]
fn a_mesh_with_no_alpha_is_not_pushed() {
  // Faded out by its renderable's alpha, then by its tint's, and one that
  // shows
  let scene = Fixture {
    setup: |ctx| {
      let [r, g, b, a] = RED;
      let red = ctx.assets().add(Image::solid_rect(8, 8, r, g, b, a));
      let clear = Color::new(255, 255, 255, 0);
      let looks = [
        Renderable::image(red.clone()).with_alpha(0.0),
        Renderable::image(red.clone()).with_tint(clear),
        Renderable::image(red),
      ];
      for (i, renderable) in looks.into_iter().enumerate() {
        ctx.world.spawn((
          Transform::from_xy(8.0 + i as f32 * 16.0, 8.0),
          renderable,
          whole_mesh(8, 8, false, false),
        ));
      }
    },
  };
  let Some(mut engine) = engine(EngineBuilder::new().resolution(48, 16).start_scene(scene)) else {
    return;
  };
  engine.step_frames(2).unwrap();
  // Only the third is an item, and its triangles start the stream
  assert_eq!(mesh_ranges(&engine), [(0, 6)]);
  let frame = engine.frame().unwrap();
  let background = frame.get_pixel(0, 0).0;
  assert_eq!(frame.get_pixel(8, 8).0, background);
  assert_eq!(frame.get_pixel(24, 8).0, background);
  assert_ne!(frame.get_pixel(40, 8).0, background);
}

/// Marks the one entity a test looks for
#[derive(Component)]
struct Subject;

#[test]
fn an_unloaded_image_s_mesh_is_the_whole_checker_in_debug_and_nothing_in_release() {
  // An image that will never load (its file is missing), drawn at twice
  // its stand-in's size, its texels whole
  let scene = Fixture {
    setup: |ctx| {
      ctx.look().tonemap = Tonemap::Clamp;
      let ghost = ctx.assets().load::<Image>("ghost.png");
      ctx.world.spawn((
        Transform::from_xy(48.0, 32.0).with_scale(Vec2::splat(2.0)),
        Renderable::image(ghost).with_filter(Filter::Nearest),
        Subject,
      ));
    },
  };
  let builder = EngineBuilder::new()
    .resolution(96, 64)
    .asset_source(MemorySource::new());
  let Some(mut engine) = engine(builder.start_scene(scene)) else {
    return;
  };
  engine.step_frames(3).unwrap();
  let sprite = engine.frame().unwrap();
  // The mesh is over a rectangle the image was to have: of the checker
  // standing in for it, 16 px a side, it shows all, as the sprite does
  let world = engine.world_mut();
  let subject = world
    .query_filtered::<Entity, With<Subject>>()
    .single(world)
    .unwrap();
  let whole = (Vec2::ZERO, Vec2::ONE);
  let mesh = quad_mesh([3, 2, 5, 7], Vec2::splat(32.0), whole, (false, false));
  world.entity_mut(subject).insert(mesh);
  engine.step_frames(3).unwrap();
  let drawn = engine.frame().unwrap();
  if let Some(differs) = difference(&sprite, &drawn) {
    panic!("the sprite's frame and the mesh's: {differs}");
  }
  let background = drawn.get_pixel(2, 2).0;
  let corner = drawn.get_pixel(33, 17).0;
  if cfg!(debug_assertions) {
    assert_ne!(corner, background, "the checker, from the mesh's corner");
    assert_eq!(mesh_ranges(&engine), [(0, 6)]);
  } else {
    assert_eq!(corner, background, "nothing drawn");
    assert!(mesh_ranges(&engine).is_empty(), "nothing pushed");
  }
}

#[test]
fn a_mesh_item_sorts_between_two_entities_at_one_z() {
  use crate::core::render::DrawKind;
  // Three 20 px squares at one z, each over the right half of the one
  // before, in entity order: a sprite, a mesh and a sprite
  let scene = Fixture {
    setup: |ctx| {
      ctx.look().tonemap = Tonemap::Clamp;
      let mut square = |x: f32, [r, g, b, a]: [u8; 4], mesh: bool| {
        let image = ctx.assets().add(Image::solid_rect(20, 20, r, g, b, a));
        let mut entity = ctx
          .world
          .spawn((Transform::from_xy(x, 20.0), Renderable::image(image)));
        if mesh {
          entity.insert(whole_mesh(20, 20, false, false));
        }
      };
      square(20.0, RED, false);
      square(30.0, GREEN, true);
      square(40.0, BLUE, false);
    },
  };
  let Some(mut engine) = engine(EngineBuilder::new().resolution(64, 40).start_scene(scene)) else {
    return;
  };
  engine.step_frames(2).unwrap();
  let frame = engine.frame().unwrap();
  let at = |x: u32| frame.get_pixel(x, 20).0;
  assert_eq!(at(15), RED, "the first sprite");
  assert_eq!(at(25), GREEN, "the mesh draws over the sprite before it");
  assert_eq!(at(35), BLUE, "and under the sprite after it");
  assert_eq!(at(45), BLUE, "the second sprite");
  // It is an entity's item, in the order pushed
  let list = engine.engine.draw_list();
  let items: Vec<_> = list
    .items()
    .iter()
    .map(|item| (item.kind, is_mesh(item)))
    .collect();
  let entity = DrawKind::Entity;
  assert_eq!(items, [(entity, false), (entity, true), (entity, false)]);
}

#[test]
fn a_lit_mesh_item_is_lit_and_an_unlit_one_is_not() {
  // In dim light: a lit sprite and a lit mesh, then an unlit sprite and an
  // unlit mesh, all of one red image
  let scene = Fixture {
    setup: |ctx| {
      ctx.look().tonemap = Tonemap::Clamp;
      ctx
        .lighting()
        .set_ambient(AmbientLight::with_intensity(0.25));
      ctx.lighting().set_enabled(true);
      let [r, g, b, a] = RED;
      let red = ctx.assets().add(Image::solid_rect(8, 8, r, g, b, a));
      for (x, lit, mesh) in [
        (8.0, true, false),
        (24.0, true, true),
        (40.0, false, false),
        (56.0, false, true),
      ] {
        let mut entity = ctx.world.spawn((
          Transform::from_xy(x, 8.0),
          Renderable::image(red.clone()).with_lighting(lit),
        ));
        if mesh {
          entity.insert(whole_mesh(8, 8, false, false));
        }
      }
    },
  };
  let Some(mut engine) = engine(EngineBuilder::new().resolution(64, 16).start_scene(scene)) else {
    return;
  };
  engine.step_frames(2).unwrap();
  let frame = engine.frame().unwrap();
  let at = |x: u32| frame.get_pixel(x, 8).0;
  let (lit_sprite, lit_mesh) = (at(8), at(24));
  assert!(
    lit_sprite[0] > 40 && lit_sprite[0] < 180,
    "the lit sprite is dimmed: {lit_sprite:?}"
  );
  assert_eq!(lit_mesh, lit_sprite, "the lit mesh is lit as the sprite is");
  assert_eq!(at(40), RED, "the unlit sprite");
  assert_eq!(at(56), RED, "the unlit mesh keeps its full brightness");
}

#[test]
fn a_mesh_uv_outside_its_region_reads_no_neighbour() {
  const YELLOW: [u8; 4] = [240, 200, 40, 255];
  let scene = Fixture {
    setup: |ctx| {
      ctx.look().tonemap = Tonemap::Clamp;
      // A mesh whose rectangle reaches past its image, an 8 px square, by
      // the image's width again: off the image's place on its page, toward
      // the image placed after it
      let [r, g, b, a] = YELLOW;
      let small = ctx.assets().add(Image::solid_rect(8, 8, r, g, b, a));
      let whole = (Vec2::ZERO, Vec2::ONE);
      ctx.world.spawn((
        Transform::from_xy(32.0, 32.0),
        Renderable::image(small),
        quad_mesh([0, 0, 16, 8], Vec2::new(16.0, 8.0), whole, (false, false)),
      ));
      // An image packed with three 8 px regions side by side: green, red
      // and blue. The mesh shows the red one, its UVs running half a region
      // past it on every side: into the green and the blue, and off the
      // image
      let mut texels = Vec::new();
      for _ in 0..8 {
        for colour in [GREEN, RED, BLUE] {
          texels.extend(std::iter::repeat_n(colour, 8).flatten());
        }
      }
      let packed = ctx.assets().add(Image::from_rgba(texels, 24, 8));
      let (from, to) = (Vec2::splat(-0.5), Vec2::splat(1.5));
      ctx.world.spawn((
        Transform::from_xy(32.0, 12.0),
        Renderable::image(packed),
        quad_mesh([8, 0, 8, 8], Vec2::splat(16.0), (from, to), (false, false)),
      ));
    },
  };
  let Some(mut engine) = engine(EngineBuilder::new().resolution(64, 40).start_scene(scene)) else {
    return;
  };
  engine.step_frames(2).unwrap();
  let frame = engine.frame().unwrap();
  let background = frame.get_pixel(0, 0).0;
  // The red region where the UVs are inside it, the square where its image
  // is, and nothing round either
  for (x, y, pixel) in frame.enumerate_pixels() {
    let expected = if (28..36).contains(&x) && (8..16).contains(&y) {
      RED
    } else if (24..32).contains(&x) && (28..36).contains(&y) {
      YELLOW
    } else {
      background
    };
    assert_eq!(pixel.0, expected, "at ({x}, {y})");
  }
}

#[test]
fn a_mesh_no_camera_sees_is_not_pushed_or_uploaded() {
  // A mesh far off the screen, then one on it, in entity order
  let scene = Fixture {
    setup: |ctx| {
      for (x, shade) in [(5000.0, 4), (20.0, 1)] {
        let image = ctx.assets().add(Image::solid_rect(8, 8, shade, 2, 3, 255));
        ctx.world.spawn((
          Transform::from_xy(x, 20.0),
          Renderable::image(image),
          whole_mesh(8, 8, false, false),
        ));
      }
    },
  };
  let Some(mut engine) = engine(EngineBuilder::new().resolution(120, 90).start_scene(scene)) else {
    return;
  };
  engine.step_frames(2).unwrap();
  let images: Vec<(f32, Handle<Image>)> = {
    let world = engine.world_mut();
    let mut renderables = world.query::<(&Transform, &Renderable)>();
    renderables
      .iter(world)
      .map(|(at, renderable)| (at.translation.x, renderable.image.clone()))
      .collect()
  };
  // Only the mesh on the screen is an item, and its triangles start the
  // stream: the far one's were taken back
  assert_eq!(mesh_ranges(&engine), [(0, 6)]);
  let gpu = engine.engine.sprite_gpu.as_ref().unwrap();
  let uploaded = |x: f32| {
    images
      .iter()
      .filter(|(at, _)| *at == x)
      .any(|(_, image)| gpu.has_image(image.id()))
  };
  assert!(uploaded(20.0));
  assert!(!uploaded(5000.0), "an off-screen mesh's image was uploaded");
}

#[test]
fn a_mesh_draws_among_trails_and_lines() {
  const YELLOW: [u8; 4] = [240, 200, 40, 255];
  // A mesh with a line under it and a line over it, and a textured trail
  // beside them: the pass goes from the lines' triangles to the mesh's and
  // back
  let scene = Fixture {
    setup: |ctx| {
      ctx.look().tonemap = Tonemap::Clamp;
      let [r, g, b, a] = BLUE;
      let blue = ctx.assets().add(Image::solid_rect(16, 16, r, g, b, a));
      ctx.world.spawn((
        Transform::from_xy(16.0, 16.0),
        Renderable::image(blue).with_z_order(5),
        whole_mesh(16, 16, false, false),
      ));
      let green = Color::new(0, 255, 0, 255);
      for (y, z) in [(14.0, 0), (20.0, 10)] {
        let line = Line2D::horizontal(y, 0.0, 64.0, green, 4.0).with_z_order(z);
        ctx.world.spawn((Transform::from_xy(0.0, 0.0), line));
      }
      let [r, g, b, a] = YELLOW;
      let texture = ctx.assets().add(Image::solid_rect(8, 2, r, g, b, a));
      let mut trail = Trail2D::new(Color::WHITE, 6.0)
        .with_end_width(6.0)
        .with_length(32)
        .with_fade(false)
        .with_texture(texture)
        .with_filter(Filter::Nearest)
        .with_lighting(false);
      trail.populate_line(4.0, 40.0, 60.0, 40.0, 32);
      ctx.world.spawn((Transform::from_xy(0.0, 0.0), trail));
    },
  };
  let Some(mut engine) = engine(EngineBuilder::new().resolution(64, 48).start_scene(scene)) else {
    return;
  };
  // Over several frames: the mesh's triangles start their stream in each
  for _ in 0..3 {
    engine.step_frames(1).unwrap();
    assert_eq!(mesh_ranges(&engine), [(0, 6)]);
  }
  let frame = engine.frame().unwrap();
  let at = |x: u32, y: u32| frame.get_pixel(x, y).0;
  let is_line = |[r, g, b, _]: [u8; 4]| g > 200 && r < 100 && b < 100;
  assert_eq!(at(16, 10), BLUE, "the mesh");
  assert_eq!(at(16, 14), BLUE, "the mesh over the line under it");
  assert!(is_line(at(40, 14)), "that line beside the mesh");
  assert!(is_line(at(16, 20)), "the line over the mesh");
  assert_eq!(at(30, 40), YELLOW, "the trail, in its own texture");
}

#[test]
fn a_zero_area_triangle_draws_nothing() {
  // A square mesh, then the same with triangles of no area added: a corner
  // three times, an edge with one end twice, and three points in a line
  // (the fifth vertex is the middle of the top edge)
  let scene = Fixture {
    setup: |ctx| {
      ctx.look().tonemap = Tonemap::Clamp;
      let image = ctx.assets().add(patterned(16, 16, true));
      ctx.world.spawn((
        Transform::from_xy(16.0, 16.0),
        Renderable::image(image),
        whole_mesh(16, 16, false, false),
      ));
    },
  };
  let Some(mut engine) = engine(EngineBuilder::new().resolution(32, 32).start_scene(scene)) else {
    return;
  };
  engine.step_frames(2).unwrap();
  let square = engine.frame().unwrap();
  let world = engine.world_mut();
  let mut meshes = world.query::<&mut MeshProbe>();
  let mut mesh = meshes.single_mut(world).unwrap();
  mesh
    .vertices
    .push((Vec2::new(0.0, -8.0), Vec2::new(0.5, 0.0)));
  mesh.indices.extend([3, 3, 3, 0, 0, 2, 0, 4, 1]);
  engine.step_frames(2).unwrap();
  let drawn = engine.frame().unwrap();
  assert_eq!(
    mesh_ranges(&engine),
    [(0, 15)],
    "the triangles of no area are in the stream"
  );
  assert_ne!(square.get_pixel(16, 16).0, square.get_pixel(0, 0).0);
  if let Some(differs) = difference(&square, &drawn) {
    panic!("without the triangles of no area and with them: {differs}");
  }
}

#[test]
fn each_camera_draws_the_meshes_it_sees_with_their_own_instances() {
  const WHITE: [u8; 4] = [255, 255, 255, 255];
  // Two cameras, each on half the window, looking at places far apart. At
  // each a sprite and then a mesh of one red image: flashed white at the
  // left one, a green silhouette at the right one. So each camera's own
  // stretch of the instances holds a sprite's and then a mesh's
  let scene = Fixture {
    setup: |ctx| {
      ctx.look().tonemap = Tonemap::Clamp;
      let [r, g, b, a] = RED;
      let red = ctx.assets().add(Image::solid_rect(12, 12, r, g, b, a));
      let [r, g, b, a] = BLUE;
      let blue = ctx.assets().add(Image::solid_rect(6, 6, r, g, b, a));
      let [r, g, b, _] = GREEN;
      let places = [
        (-1000.0, SpriteEffect::flash(Color::WHITE, 1.0)),
        (1000.0, SpriteEffect::silhouette(Color::rgb(r, g, b))),
      ];
      for (x, effect) in places {
        ctx.world.spawn((
          Transform::from_xy(x - 11.0, 0.0),
          Renderable::image(blue.clone()),
        ));
        ctx.world.spawn((
          Transform::from_xy(x, 0.0),
          Renderable::image(red.clone()).with_shader(SpriteShader::BuiltIn(effect)),
          whole_mesh(12, 12, false, false),
        ));
      }
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
    },
  };
  let Some(mut engine) = engine(EngineBuilder::new().resolution(64, 32).start_scene(scene)) else {
    return;
  };
  engine.step_frames(2).unwrap();
  let frame = engine.frame().unwrap();
  let at = |x: u32| frame.get_pixel(x, 16).0;
  // The main camera's view is the window's, drawn into its half of it at
  // half the width; the second camera's is its own half
  assert_eq!(at(10), BLUE, "the left camera's sprite");
  assert_eq!(at(16), WHITE, "the left camera's mesh, flashed");
  assert_eq!(at(37), BLUE, "the right camera's sprite");
  assert_eq!(at(48), GREEN, "the right camera's mesh, a silhouette");
}
