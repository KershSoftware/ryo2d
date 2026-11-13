//! Skeletons drawn through the world batch as entities are: where their
//! bones put their regions, in their draw order, sorted among sprites and
//! tile layers, lit, lifted on a floor, shadowed, flipped and drawn between
//! steps, read back as display bytes (pixel art, `Clamp`, so colours come
//! back as they went in).
//!
//! Tests skip when no GPU is available.

use std::f32::consts::FRAC_PI_2;
use std::sync::Arc;

use ryo2d::prelude::*;
use ryo2d::testing::HeadlessEngine;

const RED: [u8; 4] = [220, 30, 40, 255];
const GREEN: [u8; 4] = [40, 200, 60, 255];
const BLUE: [u8; 4] = [30, 60, 230, 255];
const YELLOW: [u8; 4] = [240, 220, 20, 255];
const WHITE: [u8; 4] = [255; 4];
const BLACK: [u8; 4] = [0, 0, 0, 255];
/// What an atlas holds where no part is: never drawn
const GUTTER: [u8; 4] = [255, 0, 255, 255];

/// The frame's side in pixels
const SIZE: u32 = 64;
/// A tile map cell's side
const CELL: u32 = 8;

type Setup = Box<dyn FnMut(&mut SceneContext) + Send>;

/// Runs its setup on enter and idles
struct Stage(Option<Setup>);

impl Scene for Stage {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    ctx.look().tonemap = Tonemap::Clamp;
    if let Some(mut setup) = self.0.take() {
      setup(ctx);
    }
  }
}

fn engine_with(setup: impl FnMut(&mut SceneContext) + Send + 'static) -> Option<HeadlessEngine> {
  let scene = Stage(Some(Box::new(setup)));
  HeadlessEngine::new_or_skip(
    EngineBuilder::new()
      .pixel_art(SIZE, SIZE)
      .start_scene(scene),
  )
}

fn engine() -> Option<HeadlessEngine> {
  engine_with(|_| {})
}

fn time(engine: &mut HeadlessEngine) -> Mut<'_, Time> {
  engine.world_mut().resource_mut::<Time>()
}

/// A frame `tenths` tenths of a fixed step long, which must run no step:
/// it draws that much further between the last two steps
fn part_step(engine: &mut HeadlessEngine, tenths: u32) {
  let timestep = engine.world().resource::<Time>().fixed.timestep_duration();
  let steps = engine.world().resource::<Time>().fixed.step_count();
  engine.step_real(timestep * tenths / 10);
  assert_eq!(
    engine.world().resource::<Time>().fixed.step_count(),
    steps,
    "part of a step runs no step"
  );
}

/// Whether `a` is `b`, give or take what a few steps' sums round by
fn near(a: f32, b: f32) -> bool {
  (a - b).abs() < 1e-3
}

fn solid(world: &mut World, (width, height): (u32, u32), colour: [u8; 4]) -> Handle<Image> {
  let [r, g, b, a] = colour;
  world
    .resource_mut::<Assets<Image>>()
    .add(Image::solid_rect(width, height, r, g, b, a))
}

/// One part of a test rig: a bone, and a slot of its name showing a solid
/// rectangle of `colour`, `size` px, centred `at` in the bone's space
#[derive(Clone)]
struct Part {
  name: &'static str,
  parent: Option<&'static str>,
  bone: BoneSetup,
  colour: [u8; 4],
  size: (u32, u32),
  at: Vec2,
  /// Its slot's colour
  slot: Color,
}

/// A part on a root bone at the skeleton's origin, centred on it
fn part(name: &'static str, colour: [u8; 4], size: (u32, u32)) -> Part {
  Part {
    name,
    parent: None,
    bone: BoneSetup::default(),
    colour,
    size,
    at: Vec2::ZERO,
    slot: Color::WHITE,
  }
}

impl Part {
  /// Its bone is a child of `parent`'s, at (x, y) in it
  fn under(mut self, parent: &'static str, x: f32, y: f32) -> Self {
    self.parent = Some(parent);
    self.bone.x = x;
    self.bone.y = y;
    self
  }

  /// Its bone is turned (clockwise, in radians)
  fn turned(mut self, radians: f32) -> Self {
    self.bone.rotation = radians;
    self
  }

  /// Its rectangle's centre in its bone's space
  fn at(mut self, x: f32, y: f32) -> Self {
    self.at = Vec2::new(x, y);
    self
  }

  /// Its slot's colour, which multiplies its image's
  fn slot(mut self, colour: Color) -> Self {
    self.slot = colour;
    self
  }
}

/// Where a rig's parts get their pixels
#[derive(Clone, Copy)]
enum Images {
  /// An image each
  Each,
  /// One image holding every part side by side, the rest of it
  /// [`GUTTER`]
  Atlas,
}

/// A skeleton of `parts`, each on its own bone, drawn in the order given,
/// with whatever `more` adds
fn rig_with(
  world: &mut World,
  parts: &[Part],
  images: Images,
  more: impl FnOnce(SkeletonDataBuilder) -> SkeletonDataBuilder,
) -> Handle<SkeletonData> {
  // Each part's image and where its rectangle is in it
  let placed: Vec<(Handle<Image>, u32)> = match images {
    Images::Each => parts
      .iter()
      .map(|part| (solid(world, part.size, part.colour), 0))
      .collect(),
    Images::Atlas => {
      let width: u32 = parts.iter().map(|part| part.size.0).sum();
      let height = parts.iter().map(|part| part.size.1).max().unwrap_or(1);
      let mut data = Vec::with_capacity((width * height * 4) as usize);
      for y in 0..height {
        for part in parts {
          let colour = if y < part.size.1 { part.colour } else { GUTTER };
          for _ in 0..part.size.0 {
            data.extend_from_slice(&colour);
          }
        }
      }
      let atlas = world
        .resource_mut::<Assets<Image>>()
        .add(Image::from_rgba(data, width, height));
      let mut x = 0;
      parts
        .iter()
        .map(|part| {
          let at = x;
          x += part.size.0;
          (atlas.clone(), at)
        })
        .collect()
    }
  };
  let mut builder = SkeletonData::builder();
  for (index, (part, (image, x))) in parts.iter().zip(placed).enumerate() {
    let (width, height) = part.size;
    let shown = SlotSetup {
      color: part.slot,
      attachment: Some(Arc::from(part.name)),
      ..SlotSetup::default()
    };
    let region = RegionAttachment {
      x: part.at.x,
      y: part.at.y,
      ..RegionAttachment::new(index, width as f32, height as f32)
    };
    builder = builder
      .bone(part.name, part.parent, part.bone)
      .slot(part.name, part.name, shown)
      .region(SkeletonRegion {
        name: Arc::from(part.name),
        image,
        x,
        y: 0,
        width,
        height,
      })
      .attachment(
        SkeletonData::DEFAULT_SKIN,
        part.name,
        part.name,
        SkeletonAttachment::Region(region),
      );
  }
  let data = more(builder).build().expect("a valid skeleton");
  world.resource_mut::<Assets<SkeletonData>>().add(data)
}

fn rig(world: &mut World, parts: &[Part]) -> Handle<SkeletonData> {
  rig_with(world, parts, Images::Each, |builder| builder)
}

/// A skeleton entity of `data` standing at `at`
fn stand(
  world: &mut World,
  data: Handle<SkeletonData>,
  at: (f32, f32),
  renderable: Renderable,
) -> Entity {
  world
    .spawn((
      Transform::from_xy(at.0, at.1),
      Skeleton::new(data),
      renderable,
    ))
    .id()
}

/// A rectangle of pixels: its left, top, width and height
type Box4 = (u32, u32, u32, u32);

fn inside(x: u32, y: u32, (x0, y0, w, h): Box4) -> bool {
  (x0..x0 + w).contains(&x) && (y0..y0 + h).contains(&y)
}

/// Checks every pixel: the colour of the last of `boxes` holding it, else
/// the background (the bottom-right pixel, which every test leaves empty)
fn assert_boxes(frame: &image::RgbaImage, boxes: &[(Box4, [u8; 4])], what: &str) {
  let background = frame.get_pixel(SIZE - 1, SIZE - 1).0;
  for (_, colour) in boxes {
    assert_ne!(
      background, *colour,
      "{what}: the background is a colour drawn"
    );
  }
  for y in 0..SIZE {
    for x in 0..SIZE {
      let want = boxes
        .iter()
        .rev()
        .find(|(area, _)| inside(x, y, *area))
        .map_or(background, |(_, colour)| *colour);
      assert_eq!(frame.get_pixel(x, y).0, want, "{what}: pixel ({x}, {y})");
    }
  }
}

fn frame(engine: &HeadlessEngine) -> image::RgbaImage {
  engine.frame().expect("a frame")
}

/// One step, and its frame
fn stepped(engine: &mut HeadlessEngine) -> image::RgbaImage {
  engine.step().expect("a step");
  frame(engine)
}

#[test]
fn a_region_on_a_rotated_bone_draws_where_expected() {
  let Some(mut engine) = engine() else {
    return;
  };
  // A body 8 px square on the root, and an arm 10 px along it turned a
  // quarter turn clockwise (pointing down the screen), whose 12 x 4 region
  // is centred 6 px along it: in the skeleton's space the arm's region
  // spans x 8..12 (across the bone) and y 0..12 (along it). Both regions
  // are parts of one image, each its own rectangle of it
  let world = engine.world_mut();
  let parts = [
    part("body", GREEN, (8, 8)),
    part("arm", RED, (12, 4))
      .under("body", 10.0, 0.0)
      .turned(FRAC_PI_2)
      .at(6.0, 0.0),
  ];
  let data = rig_with(world, &parts, Images::Atlas, |builder| builder);
  let hero = stand(world, data, (32.0, 32.0), Renderable::skeleton());
  let drawn = stepped(&mut engine);
  assert_boxes(
    &drawn,
    &[((28, 28, 8, 8), GREEN), ((40, 32, 4, 12), RED)],
    "standing at (32, 32)",
  );

  // The entity's own transform carries every bone: moved, turned a quarter
  // turn (the skeleton's x axis points down the screen, its y axis left)
  // and doubled, the body is 16 px square about the entity and the arm's
  // region spans x -24..0 and y 16..24 from it
  let mut transform = engine.world_mut().get_mut::<Transform>(hero).unwrap();
  *transform = Transform::from_xy(40.0, 20.0)
    .with_rotation(FRAC_PI_2)
    .with_scale(Vec2::splat(2.0));
  let drawn = stepped(&mut engine);
  assert_boxes(
    &drawn,
    &[((32, 12, 16, 16), GREEN), ((16, 36, 24, 8), RED)],
    "turned and doubled",
  );
}

#[test]
fn a_draw_order_key_swaps_which_region_is_on_top() {
  let Some(mut engine) = engine() else {
    return;
  };
  // A red square with a smaller green one over it, as the slots are
  // listed; an animation keys the other order
  let world = engine.world_mut();
  let parts = [part("under", RED, (16, 16)), part("over", GREEN, (8, 8))];
  let data = rig_with(world, &parts, Images::Each, |builder| {
    builder.animation(SkeletonClip {
      name: Arc::from("swap"),
      duration: 1.0,
      timelines: vec![SkeletonTimeline::DrawOrder {
        frames: vec![(0.0, Some(vec![SlotId(1), SlotId(0)]))],
      }],
    })
  });
  let hero = stand(world, data, (32.0, 32.0), Renderable::skeleton());
  world.entity_mut(hero).insert(SkeletonAnimation::new());
  let both = [((24, 24, 16, 16), RED), ((28, 28, 8, 8), GREEN)];
  assert_boxes(&stepped(&mut engine), &both, "the setup order");

  let mut animation = engine
    .world_mut()
    .get_mut::<SkeletonAnimation>(hero)
    .unwrap();
  animation.set_animation(0, "swap", true);
  assert_boxes(
    &stepped(&mut engine),
    &[((24, 24, 16, 16), RED)],
    "the keyed order puts the red square on top",
  );

  let mut animation = engine
    .world_mut()
    .get_mut::<SkeletonAnimation>(hero)
    .unwrap();
  animation.clear(0);
  assert_boxes(&stepped(&mut engine), &both, "the setup order again");
}

/// A 16 px blue skeleton whose art stands on its origin, the feet
fn standing_hero(world: &mut World) -> Handle<SkeletonData> {
  rig(world, &[part("body", BLUE, (16, 16)).at(0.0, -8.0)])
}

/// What draws where when a 16 px blue character whose feet are at
/// (24, `feet`) stands among a red tree at x 16..24 with its roots at 32
/// and a yellow one at x 24..32 with its roots at 48, each 8 x 16 px: the
/// three by their feet, the lowest on screen in front
fn among_trees(feet: u32) -> Vec<(Box4, [u8; 4])> {
  let mut drawn = [
    (32, ((16, 16, 8, 16), RED)),
    (48, ((24, 32, 8, 16), YELLOW)),
    (feet, ((16, feet - 16, 16, 16), BLUE)),
  ];
  drawn.sort_by_key(|(feet, _)| *feet);
  drawn.into_iter().map(|(_, drawn)| drawn).collect()
}

#[test]
fn a_y_sorted_skeleton_sorts_among_ysort_sprites_by_its_feet() {
  let Some(mut engine) = engine() else {
    return;
  };
  // Two tree sprites and a skeleton between them in entity order, all
  // y-sorted at z 1: entity order alone would draw red, blue, yellow
  let world = engine.world_mut();
  let tree = |world: &mut World, colour, (x, y): (f32, f32)| {
    let image = solid(world, (8, 16), colour);
    world.spawn((
      Transform::from_xy(x, y),
      Renderable::image(image)
        .with_anchor(Anchor::TopLeft)
        .with_z_order(1),
      YSort::new(),
    ));
  };
  tree(world, RED, (16.0, 16.0));
  let data = standing_hero(world);
  let hero = stand(
    world,
    data,
    (24.0, 40.0),
    Renderable::skeleton().with_z_order(1),
  );
  world.entity_mut(hero).insert(YSort::new());
  tree(world, YELLOW, (24.0, 32.0));

  // Between the trees, then up behind both, then down in front of both
  for feet in [40, 30, 52] {
    let mut transform = engine.world_mut().get_mut::<Transform>(hero).unwrap();
    transform.translation.y = feet as f32;
    let what = format!("feet at {feet}");
    assert_boxes(&stepped(&mut engine), &among_trees(feet), &what);
  }

  // Upside down its art hangs below its origin (y 40..56), over the yellow
  // tree's lower half: it still sorts where it stands, at 40, behind that
  // tree's roots at 48
  let mut transform = engine.world_mut().get_mut::<Transform>(hero).unwrap();
  transform.translation.y = 40.0;
  engine
    .world_mut()
    .get_mut::<Renderable>(hero)
    .unwrap()
    .flip_y = true;
  assert_boxes(
    &stepped(&mut engine),
    &[
      ((16, 16, 8, 16), RED),
      ((16, 40, 16, 16), BLUE),
      ((24, 32, 8, 16), YELLOW),
    ],
    "flipped upside down, standing at 40",
  );
}

/// Steps until every map root has landed
fn land(engine: &mut HeadlessEngine) {
  let start = std::time::Instant::now();
  loop {
    let world = engine.world_mut();
    let roots: Vec<Entity> = world
      .query_filtered::<Entity, With<MapRoot>>()
      .iter(world)
      .collect();
    if roots
      .iter()
      .all(|&root| MapsView::new(world).is_ready(root))
    {
      return;
    }
    engine.step().unwrap();
    assert!(start.elapsed().as_secs() < 5, "a map did not land in 5 s");
    std::thread::yield_now();
  }
}

#[test]
fn a_y_sorted_skeleton_sorts_within_a_y_sorted_tile_layer() {
  // Trees 8 x 16 px on 8 px cells stand on their cell's bottom edge: a red
  // one on cell (2, 3) (y 16..32, roots at 32) and a yellow one on (3, 5)
  // (y 32..48, roots at 48), in a y-sorted layer at z 1, each row its own
  // draw. The skeleton, y-sorted at z 1, draws between the rows by its feet
  let Some(mut engine) = engine_with(|ctx| {
    let data: Vec<u8> = (0..4 * CELL * CELL)
      .flat_map(|i| if i % (2 * CELL) < CELL { RED } else { YELLOW })
      .collect();
    let image = ctx.assets().add(Image::from_rgba(data, 2 * CELL, 2 * CELL));
    let size = UVec2::new(2 * CELL, 2 * CELL);
    let mut trees = Tileset::new(
      "trees",
      UVec2::new(CELL, 2 * CELL),
      TilesetImage::Atlas { image, size },
    );
    trees.columns = 2;
    trees.tile_count = 2;
    let mut forest = TileMap::new(UVec2::splat(SIZE / CELL), UVec2::splat(CELL));
    forest.add_tileset(TilesetSource::Embedded(Arc::new(trees)));
    let layer = forest.add_tile_layer("trees");
    let grid = forest.tiles_mut(layer).expect("a tile layer");
    assert!(grid.set(IVec2::new(2, 3), Some(Tile::new(0, 0))));
    assert!(grid.set(IVec2::new(3, 5), Some(Tile::new(0, 1))));
    forest.layers[layer].info.y_sorted = true;
    let forest = ctx.assets().add(forest);
    ctx
      .world
      .spawn((MapRoot::new(forest).with_z(1), Transform::IDENTITY));
  }) else {
    return;
  };
  land(&mut engine);
  let world = engine.world_mut();
  let data = standing_hero(world);
  let hero = stand(
    world,
    data,
    (24.0, 40.0),
    Renderable::skeleton().with_z_order(1),
  );
  world.entity_mut(hero).insert(YSort::new());
  for feet in [40, 30, 52] {
    let mut transform = engine.world_mut().get_mut::<Transform>(hero).unwrap();
    transform.translation.y = feet as f32;
    engine.step().unwrap();
    let what = format!("feet at {feet}");
    assert_boxes(&stepped(&mut engine), &among_trees(feet), &what);
  }
}

#[test]
fn a_skeleton_is_lit_by_a_light_and_sits_on_its_floor_s_lift() {
  // A map whose floor 1 is 32 px up
  let Some(mut engine) = engine_with(|ctx| {
    let mut tiles = TileMap::new(UVec2::splat(SIZE / CELL), UVec2::splat(CELL));
    tiles.add_tile_layer("ground");
    let upper = tiles.add_tile_layer("upper");
    tiles.layers[upper].info.floor = 1;
    tiles.layers[upper].info.offset = Vec2::new(0.0, -32.0);
    let tiles = ctx.assets().add(tiles);
    ctx
      .world
      .spawn((MapRoot::new(tiles).with_z(4), Transform::IDENTITY));
  }) else {
    return;
  };
  land(&mut engine);
  // On floor 1, a white skeleton 8 px square standing with its feet at
  // (32, 52), a blob shadow under them
  let world = engine.world_mut();
  let data = rig(world, &[part("body", WHITE, (8, 8)).at(0.0, -4.0)]);
  let hero = stand(
    world,
    data,
    (32.0, 52.0),
    Renderable::skeleton()
      .with_z_order(5)
      .with_blob_shadow(BlobShadow::circle(6.0)),
  );
  world.entity_mut(hero).insert(Floor::On(1));
  engine.step_frames(2).unwrap();
  let lifted = frame(&engine);
  engine.world_mut().entity_mut(hero).insert(Floor::GROUND);
  engine.step_frames(2).unwrap();
  let ground = frame(&engine);

  // On the ground floor it draws where it stands, its shadow under its
  // feet, and nothing in the upper half
  let background = ground.get_pixel(SIZE - 1, SIZE - 1).0;
  for y in 0..SIZE {
    for x in 0..SIZE {
      let at = ground.get_pixel(x, y).0;
      if inside(x, y, (28, 44, 8, 8)) {
        assert_eq!(at, WHITE, "the body at ({x}, {y})");
      } else if y < 32 {
        assert_eq!(at, background, "nothing up at ({x}, {y})");
      }
    }
  }
  assert_ne!(ground.get_pixel(32, 53).0, background, "the blob shadow");
  // On floor 1 all of it draws 32 px up
  for y in 0..SIZE {
    for x in 0..SIZE {
      let want = match y + 32 {
        below if below < SIZE => ground.get_pixel(x, below).0,
        _ => background,
      };
      assert_eq!(lifted.get_pixel(x, y).0, want, "lifted, at ({x}, {y})");
    }
  }

  // In the dark, a light where it draws on floor 1 lights it, and a
  // second skeleton across the frame stays dark; unlit, that one draws
  // at full brightness
  let world = engine.world_mut();
  world.entity_mut(hero).insert(Floor::On(1));
  world
    .resource_mut::<LightingSettings>()
    .set_ambient(AmbientLight::with_intensity(0.0));
  world.spawn((
    Transform::from_xy(32.0, 16.0),
    Light2D::point(Color::WHITE, 14.0),
  ));
  let data = rig(world, &[part("body", WHITE, (8, 8))]);
  let far = stand(
    world,
    data,
    (8.0, 56.0),
    Renderable::skeleton().with_z_order(5),
  );
  engine.step_frames(2).unwrap();
  let lit = frame(&engine);
  let (near, away) = (lit.get_pixel(32, 16).0, lit.get_pixel(8, 56).0);
  assert!(near[0] > 200, "lit where it draws: {near:?}");
  assert!(away[0] < 10, "dark away from the light: {away:?}");
  engine
    .world_mut()
    .get_mut::<Renderable>(far)
    .unwrap()
    .lighting = false;
  engine.step_frames(2).unwrap();
  let away = frame(&engine).get_pixel(8, 56).0;
  assert_eq!(away, WHITE, "an unlit skeleton draws after the lighting");
}

#[test]
fn two_skeletons_at_different_z_put_a_sprite_between_them() {
  let Some(mut engine) = engine() else {
    return;
  };
  // Spawned front to back, so entity order would draw them the other way
  // round: a blue skeleton at z 2, a green sprite at z 1, a red skeleton
  // at z 0, each 16 px square and 4 px right of the one below
  let world = engine.world_mut();
  let blue = rig(world, &[part("body", BLUE, (16, 16))]);
  stand(
    world,
    blue,
    (36.0, 32.0),
    Renderable::skeleton().with_z_order(2),
  );
  let green = solid(world, (16, 16), GREEN);
  world.spawn((
    Transform::from_xy(32.0, 32.0),
    Renderable::image(green).with_z_order(1),
  ));
  let red = rig(world, &[part("body", RED, (16, 16))]);
  stand(world, red, (28.0, 32.0), Renderable::skeleton());
  assert_boxes(
    &stepped(&mut engine),
    &[
      ((20, 24, 16, 16), RED),
      ((24, 24, 16, 16), GREEN),
      ((28, 24, 16, 16), BLUE),
    ],
    "by z",
  );
}

/// A white sprite under the whole frame but its last column, which stays
/// the background
fn white_floor(world: &mut World) {
  let floor = solid(world, (SIZE - 1, SIZE), WHITE);
  world.spawn((
    Transform::IDENTITY,
    Renderable::image(floor)
      .with_anchor(Anchor::TopLeft)
      .with_z_order(-1),
  ));
}

#[test]
fn a_skeleton_casts_a_drop_shadow() {
  let Some(mut engine) = engine() else {
    return;
  };
  // Over a white floor: a red body 8 px square at (24, 24) and a blue hand
  // 4 px square on a bone at (10, 6) from it, with a black shadow 6 px
  // down and right. Each region casts its own, and every shadow is under
  // every region: the body's falls on the hand's place, which stays blue
  let world = engine.world_mut();
  white_floor(world);
  let parts = [
    part("body", RED, (8, 8)),
    part("hand", BLUE, (4, 4)).under("body", 10.0, 6.0),
  ];
  let data = rig(world, &parts);
  let shadow = DropShadow::new()
    .offset(6.0, 6.0)
    .color(Color::new(0, 0, 0, 255));
  let hero = stand(
    world,
    data,
    (24.0, 24.0),
    Renderable::skeleton().with_drop_shadow(shadow),
  );
  let floor = ((0, 0, SIZE - 1, SIZE), WHITE);
  assert_boxes(
    &stepped(&mut engine),
    &[
      floor,
      ((26, 26, 8, 8), BLACK),
      ((38, 34, 4, 4), BLACK),
      ((20, 20, 8, 8), RED),
      ((32, 28, 4, 4), BLUE),
    ],
    "a shadow 6 px down and right of each region",
  );

  // Faded with the skeleton: at half alpha a black shadow leaves the white
  // floor mid grey, where no region covers it
  let mut renderable = engine.world_mut().get_mut::<Renderable>(hero).unwrap();
  renderable.alpha = 0.5;
  let faded = stepped(&mut engine).get_pixel(40, 36).0;
  assert!(
    faded[0] > 100 && faded[0] < 220 && faded[0] == faded[1] && faded[1] == faded[2],
    "a half shadow: {faded:?}"
  );

  // A shadow's scale shrinks the silhouette toward where the skeleton
  // stands: at half height, 16 px down, the body's shadow spans x 20..28
  // and y 38..42, and the hand's x 32..36 and y 42..44
  let mut renderable = engine.world_mut().get_mut::<Renderable>(hero).unwrap();
  renderable.alpha = 1.0;
  renderable.drop_shadow = Some(
    DropShadow::new()
      .offset(0.0, 16.0)
      .scale(1.0, 0.5)
      .color(Color::new(0, 0, 0, 255)),
  );
  assert_boxes(
    &stepped(&mut engine),
    &[
      floor,
      ((20, 38, 8, 4), BLACK),
      ((32, 42, 4, 2), BLACK),
      ((20, 20, 8, 8), RED),
      ((32, 28, 4, 4), BLUE),
    ],
    "a squashed shadow",
  );
}

/// A rig with nothing mirrored about it: an arm from (10, -6) pointing
/// right, whose red 8 x 4 region spans x 10..18 and y -8..-4 in the
/// skeleton's space, with a green 4 px square over its first half
fn lopsided(world: &mut World) -> Handle<SkeletonData> {
  let parts = [
    part("arm", RED, (8, 4)).at(4.0, 0.0),
    part("cuff", GREEN, (4, 4))
      .under("arm", 0.0, 0.0)
      .at(2.0, 0.0),
  ];
  let mut parts = parts.to_vec();
  parts[0].bone.x = 10.0;
  parts[0].bone.y = -6.0;
  rig(world, &parts)
}

/// A yellow 4 px square on a socket 8 px along `hero`'s arm (its tip),
/// drawn over the skeleton
fn held(world: &mut World, hero: Entity) -> Entity {
  let image = solid(world, (4, 4), YELLOW);
  world
    .spawn((
      BoneSocket::new("arm").with_offset(Transform::from_xy(8.0, 0.0)),
      Renderable::image(image).with_z_order(1),
      ChildOf(hero),
    ))
    .id()
}

fn bone(engine: &HeadlessEngine, hero: Entity, name: &str) -> Affine2 {
  let skeleton = engine.world().get::<Skeleton>(hero).expect("a skeleton");
  skeleton.bone_world(name).expect("a posed bone")
}

fn global(engine: &HeadlessEngine, entity: Entity) -> Affine2 {
  let global = engine.world().get::<GlobalTransform>(entity);
  global.expect("a placed entity").affine()
}

#[test]
fn a_flipped_skeleton_mirrors_its_regions_and_sockets() {
  let Some(mut engine) = engine() else {
    return;
  };
  let world = engine.world_mut();
  let data = lopsided(world);
  let hero = stand(world, data, (32.0, 32.0), Renderable::skeleton());
  let item = held(world, hero);
  // As built: the arm right of and above the entity, the cuff over its
  // first half, the socket at its tip
  assert_boxes(
    &stepped(&mut engine),
    &[
      ((42, 24, 8, 4), RED),
      ((42, 24, 4, 4), GREEN),
      ((48, 24, 4, 4), YELLOW),
    ],
    "unflipped",
  );
  assert_eq!(
    bone(&engine, hero, "arm").translation,
    Vec2::new(42.0, 26.0)
  );
  assert_eq!(global(&engine, item).translation, Vec2::new(50.0, 26.0));

  // Flipped left to right about the entity's origin: every region, the
  // bones as the skeleton answers for them, and the socket mirror, in the
  // same step; the cuff is still over the arm
  let flip = |engine: &mut HeadlessEngine, x: bool, y: bool| {
    let mut renderable = engine.world_mut().get_mut::<Renderable>(hero).unwrap();
    renderable.flip_x = x;
    renderable.flip_y = y;
  };
  flip(&mut engine, true, false);
  assert_boxes(
    &stepped(&mut engine),
    &[
      ((14, 24, 8, 4), RED),
      ((18, 24, 4, 4), GREEN),
      ((12, 24, 4, 4), YELLOW),
    ],
    "flipped left to right",
  );
  let arm = bone(&engine, hero, "arm");
  assert_eq!(arm.translation, Vec2::new(22.0, 26.0));
  assert_eq!(arm.matrix2.x_axis, Vec2::new(-1.0, 0.0), "it points left");
  assert_eq!(arm.matrix2.y_axis, Vec2::new(0.0, 1.0));
  let socket = global(&engine, item);
  assert!(socket.translation.abs_diff_eq(Vec2::new(14.0, 26.0), 1e-4));
  assert!(socket.matrix2.determinant() < 0.0, "the socket is mirrored");

  // Upside down as well
  flip(&mut engine, true, true);
  assert_boxes(
    &stepped(&mut engine),
    &[
      ((14, 36, 8, 4), RED),
      ((18, 36, 4, 4), GREEN),
      ((12, 36, 4, 4), YELLOW),
    ],
    "flipped both ways",
  );
  assert_eq!(
    bone(&engine, hero, "arm").translation,
    Vec2::new(22.0, 38.0)
  );
  let socket = global(&engine, item);
  assert!(socket.translation.abs_diff_eq(Vec2::new(14.0, 38.0), 1e-4));

  // A flip made while no step runs (the clock paused) shows in that frame,
  // the socket with the regions
  time(&mut engine).game.pause();
  flip(&mut engine, false, false);
  assert_boxes(
    &stepped(&mut engine),
    &[
      ((42, 24, 8, 4), RED),
      ((42, 24, 4, 4), GREEN),
      ((48, 24, 4, 4), YELLOW),
    ],
    "unflipped while paused",
  );
  assert_eq!(
    bone(&engine, hero, "arm").translation,
    Vec2::new(42.0, 26.0)
  );
  time(&mut engine).game.resume();

  // A negative scale on the entity mirrors it the same way
  engine.world_mut().get_mut::<Transform>(hero).unwrap().scale = Vec2::new(-1.0, 1.0);
  assert_boxes(
    &stepped(&mut engine),
    &[
      ((14, 24, 8, 4), RED),
      ((18, 24, 4, 4), GREEN),
      ((12, 24, 4, 4), YELLOW),
    ],
    "mirrored by the entity's scale",
  );
  let arm = bone(&engine, hero, "arm");
  assert!(arm.translation.abs_diff_eq(Vec2::new(22.0, 26.0), 1e-4));
  assert!(arm.matrix2.x_axis.abs_diff_eq(Vec2::new(-1.0, 0.0), 1e-5));
  let socket = global(&engine, item);
  assert!(socket.translation.abs_diff_eq(Vec2::new(14.0, 26.0), 1e-4));
}

/// A rig whose white 4 px hand slides right 20 px a step (1200 px/s), on a
/// bone "hand" of a still body with no art
fn slider(world: &mut World) -> Handle<SkeletonData> {
  slider_by(world, 20.0)
}

/// A rig whose white 4 px hand slides right `per_step` px a step
fn slider_by(world: &mut World, per_step: f32) -> Handle<SkeletonData> {
  let parts = [
    Part {
      size: (1, 1),
      colour: [0, 0, 0, 0],
      ..part("body", WHITE, (1, 1))
    },
    part("hand", WHITE, (4, 4)).under("body", 0.0, 0.0),
  ];
  rig_with(world, &parts, Images::Each, |builder| {
    builder.animation(SkeletonClip {
      name: Arc::from("slide"),
      duration: 10.0,
      timelines: vec![SkeletonTimeline::Translate {
        bone: BoneId(1),
        frames: vec![
          (0.0, Vec2::ZERO, SkeletonCurve::Linear),
          (
            10.0,
            Vec2::new(per_step * 600.0, 0.0),
            SkeletonCurve::Linear,
          ),
        ],
      }],
    })
  })
}

/// A sliding skeleton standing at `at`, its animation playing
fn sliding(world: &mut World, data: &Handle<SkeletonData>, at: (f32, f32)) -> Entity {
  let hero = stand(world, data.clone(), at, Renderable::skeleton());
  let mut animation = SkeletonAnimation::new();
  animation.set_animation(0, "slide", false);
  world.entity_mut(hero).insert(animation);
  hero
}

/// Whether the 4 px square centred on (x, y) is all white, all the
/// background, or neither
fn square(frame: &image::RgbaImage, x: f32, y: f32) -> Option<bool> {
  let background = frame.get_pixel(SIZE - 1, SIZE - 1).0;
  let (left, top) = ((x - 2.0).round() as u32, (y - 2.0).round() as u32);
  let pixels = (0..16).map(|i| frame.get_pixel(left + i % 4, top + i / 4).0);
  let (mut white, mut empty) = (true, true);
  for pixel in pixels {
    white &= pixel == WHITE;
    empty &= pixel == background;
  }
  match (white, empty) {
    (true, false) => Some(true),
    (false, true) => Some(false),
    _ => None,
  }
}

#[test]
fn skeleton_bones_are_interpolated_between_steps() {
  let Some(mut engine) = engine() else {
    return;
  };
  // Two skeletons whose hands slide 20 px a step: one on an interpolated
  // entity, with a socket on its hand holding a white square 8 px below
  // it, and one on a plain entity
  let world = engine.world_mut();
  let data = slider(world);
  let smooth = sliding(world, &data, (4.0, 16.0));
  world.entity_mut(smooth).insert(Interpolated);
  let image = solid(world, (4, 4), WHITE);
  world.spawn((
    BoneSocket::new("hand").with_offset(Transform::from_xy(0.0, 8.0)),
    Renderable::image(image),
    ChildOf(smooth),
  ));
  let plain = sliding(world, &data, (4.0, 48.0));
  engine.step_frames(2).unwrap();
  let drawn = frame(&engine);
  let now = bone(&engine, smooth, "hand").translation.x;
  assert!(near(now, 44.0), "two steps of 20 px from x 4: {now}");
  assert_eq!(bone(&engine, plain, "hand").translation.x, now);

  // A frame of whole steps draws the interpolated one where it was the
  // step before, its socket with it, and the plain one where it is
  assert_eq!(
    square(&drawn, now - 20.0, 16.0),
    Some(true),
    "the step before"
  );
  assert_eq!(square(&drawn, now, 16.0), Some(false));
  assert_eq!(square(&drawn, now - 20.0, 24.0), Some(true), "its socket");
  assert_eq!(square(&drawn, now, 48.0), Some(true), "the plain one");
  assert_eq!(square(&drawn, now - 20.0, 48.0), Some(false));

  // Half a step later no step has run: the bone and its socket draw half
  // way, and the plain one where it is
  part_step(&mut engine, 5);
  let drawn = frame(&engine);
  assert_eq!(bone(&engine, smooth, "hand").translation.x, now);
  assert_eq!(square(&drawn, now - 10.0, 16.0), Some(true), "half way");
  assert_eq!(square(&drawn, now - 20.0, 16.0), Some(false));
  assert_eq!(square(&drawn, now + 2.0, 16.0), Some(false));
  assert_eq!(square(&drawn, now - 10.0, 24.0), Some(true), "its socket");
  assert_eq!(square(&drawn, now, 48.0), Some(true), "the plain one");
  assert_eq!(square(&drawn, now - 10.0, 48.0), Some(false));

  // A teleport: the entity skips interpolation, and its bones and their
  // sockets skip with it, drawn where they are in that same frame
  engine.world_mut().skip_interpolation(smooth);
  part_step(&mut engine, 2);
  let drawn = frame(&engine);
  assert_eq!(square(&drawn, now, 16.0), Some(true), "the bone, at once");
  assert_eq!(square(&drawn, now - 10.0, 16.0), Some(false));
  assert_eq!(square(&drawn, now, 24.0), Some(true), "its socket, at once");
}

#[test]
fn a_skeleton_spawned_while_the_clock_is_paused_draws_that_frame() {
  let Some(mut engine) = engine() else {
    return;
  };
  engine.step().unwrap();
  // No fixed step runs, yet the frame it appears in draws it, in its
  // animation's first pose: 20 px a step from x 4 has not started
  time(&mut engine).game.pause();
  let world = engine.world_mut();
  let data = slider(world);
  let hero = sliding(world, &data, (4.0, 16.0));
  world.entity_mut(hero).insert(Interpolated);
  let steps = engine.world().resource::<Time>().fixed.step_count();
  let drawn = stepped(&mut engine);
  assert_eq!(engine.world().resource::<Time>().fixed.step_count(), steps);
  assert_eq!(
    square(&drawn, 4.0, 16.0),
    Some(true),
    "drawn where it starts"
  );

  // Moved while no step runs, it draws at its new place in that frame, as
  // a sprite does: its bones follow its entity
  engine
    .world_mut()
    .get_mut::<Transform>(hero)
    .unwrap()
    .translation = Vec2::new(30.0, 40.0);
  engine.world_mut().skip_interpolation(hero);
  let drawn = stepped(&mut engine);
  assert_eq!(square(&drawn, 30.0, 40.0), Some(true), "at its new place");
  assert_eq!(square(&drawn, 4.0, 16.0), Some(false));
}

#[test]
fn a_skeleton_moved_between_steps_draws_at_its_new_place_that_frame() {
  let Some(mut engine) = engine() else {
    return;
  };
  // A still skeleton on a plain entity, stepped; then the clock stops and
  // the entity moves: no step poses its bones again, and the frame draws
  // it where its entity now is, as it would a sprite
  let world = engine.world_mut();
  let data = rig(world, &[part("body", WHITE, (4, 4))]);
  let hero = stand(world, data, (10.0, 10.0), Renderable::skeleton());
  let drawn = stepped(&mut engine);
  assert_eq!(square(&drawn, 10.0, 10.0), Some(true));
  time(&mut engine).game.pause();
  engine
    .world_mut()
    .get_mut::<Transform>(hero)
    .unwrap()
    .translation = Vec2::new(40.0, 30.0);
  let drawn = stepped(&mut engine);
  assert_eq!(square(&drawn, 40.0, 30.0), Some(true), "at its new place");
  assert_eq!(square(&drawn, 10.0, 10.0), Some(false));
}

#[test]
fn a_skeleton_enabled_again_does_not_blend_from_its_old_pose() {
  let Some(mut engine) = engine() else {
    return;
  };
  // An interpolated skeleton whose hand slides 20 px a step from x -12:
  // a frame of whole steps draws it where it was the step before
  let world = engine.world_mut();
  let data = slider(world);
  let hero = sliding(world, &data, (-12.0, 16.0));
  world.entity_mut(hero).insert(Interpolated);
  engine.step_frames(2).unwrap();
  let before = bone(&engine, hero, "hand").translation.x;
  assert_eq!(square(&frame(&engine), before - 20.0, 16.0), Some(true));

  // Disabled for some steps it is not drawn, and does not move
  engine.world_mut().entity_mut(hero).insert(Disabled);
  engine.step_frames(3).unwrap();
  let drawn = frame(&engine);
  assert_eq!(square(&drawn, before - 20.0, 16.0), Some(false));
  assert_eq!(square(&drawn, before, 16.0), Some(false));

  // Enabled again while no step runs, it draws the pose it has, not a
  // blend from the one before it
  time(&mut engine).game.pause();
  engine.world_mut().entity_mut(hero).remove::<Disabled>();
  let drawn = stepped(&mut engine);
  assert_eq!(bone(&engine, hero, "hand").translation.x, before);
  assert_eq!(square(&drawn, before, 16.0), Some(true), "the pose it has");
  assert_eq!(square(&drawn, before - 20.0, 16.0), Some(false));

  // Its first step afterwards is a first placing: drawn where that step
  // put it, where a skeleton that never stopped would be drawn a step
  // behind
  time(&mut engine).game.resume();
  let drawn = stepped(&mut engine);
  let now = bone(&engine, hero, "hand").translation.x;
  assert!(near(now, before + 20.0), "one more step: {now}");
  assert_eq!(square(&drawn, now, 16.0), Some(true), "where it now is");
  assert_eq!(square(&drawn, before, 16.0), Some(false));
  // From its next step it is drawn between steps again: a step behind
  let drawn = stepped(&mut engine);
  let next = bone(&engine, hero, "hand").translation.x;
  assert!(near(next, now + 20.0), "and one more: {next}");
  assert_eq!(square(&drawn, now, 16.0), Some(true), "a step behind again");
}

/// An image whose left half is red and right half green, the top row blue
fn two_tone(world: &mut World) -> Handle<Image> {
  let data: Vec<u8> = (0..8 * 6)
    .flat_map(|i| match (i % 8, i / 8) {
      (_, 0) => BLUE,
      (x, _) if x < 4 => RED,
      _ => GREEN,
    })
    .collect();
  world
    .resource_mut::<Assets<Image>>()
    .add(Image::from_rgba(data, 8, 6))
}

#[test]
fn a_one_region_skeleton_draws_as_the_sprite_of_its_image() {
  // A sprite, and a skeleton whose one region is the sprite's image centred
  // on its root: whatever the renderable and the transform say, the two
  // draw the same pixels
  let Some(mut engine) = engine() else {
    return;
  };
  let world = engine.world_mut();
  white_floor(world);
  let image = two_tone(world);
  let data = SkeletonData::builder()
    .bone("root", None, BoneSetup::default())
    .slot(
      "body",
      "root",
      SlotSetup {
        attachment: Some(Arc::from("body")),
        ..SlotSetup::default()
      },
    )
    .region(SkeletonRegion {
      name: Arc::from("body"),
      image: image.clone(),
      x: 0,
      y: 0,
      width: 8,
      height: 6,
    })
    .attachment(
      SkeletonData::DEFAULT_SKIN,
      "body",
      "body",
      SkeletonAttachment::Region(RegionAttachment::new(0, 8.0, 6.0)),
    )
    .build()
    .expect("a valid skeleton");
  let data = world.resource_mut::<Assets<SkeletonData>>().add(data);
  world.spawn((
    Transform::from_xy(48.0, 48.0),
    Light2D::point(Color::rgb(255, 240, 200), 40.0),
  ));
  let sprite = world
    .spawn((Transform::IDENTITY, Renderable::image(image.clone())))
    .id();
  let bones = stand(world, data, (0.0, 0.0), Renderable::skeleton());

  // Each look, and whether it shows only where the image is drawn at
  // another size than its own (a filter)
  type Dress = fn(Renderable) -> Renderable;
  let looks: [(&str, Dress, bool); 11] = [
    (
      "tinted and faded",
      |r| r.with_tint(Color::new(255, 128, 64, 200)).with_alpha(0.6),
      false,
    ),
    ("flipped left to right", |r| r.with_flip_x(true), false),
    (
      "flipped both ways",
      |r| r.with_flip_x(true).with_flip_y(true),
      false,
    ),
    (
      "a drop shadow",
      |r| r.with_drop_shadow(DropShadow::new().offset(5.0, 3.0)),
      false,
    ),
    (
      "a blob shadow",
      |r| r.with_blob_shadow(BlobShadow::ellipse(8.0, 4.0).offset(5.0)),
      false,
    ),
    (
      "a contact shadow",
      |r| r.with_contact_shadow(ContactShadow::new().radius(6.0).offset(4.0)),
      false,
    ),
    ("unlit", |r| r.with_lighting(false), false),
    ("additive", |r| r.with_blend(BlendMode::Additive), false),
    ("glowing", |r| r.with_emission(3.0), false),
    ("smooth", |r| r.with_filter(Filter::Linear), true),
    (
      "flashing",
      |r| r.with_shader(SpriteShader::flash(Color::WHITE, 0.5)),
      false,
    ),
  ];
  let places = [
    ("upright", Transform::from_xy(30.0, 30.0), false),
    (
      "half as large again",
      Transform::from_xy(30.0, 30.0).with_scale(Vec2::splat(1.5)),
      true,
    ),
    (
      "turned and scaled",
      Transform::from_xy(24.5, 31.25)
        .with_rotation(0.6)
        .with_scale(Vec2::new(3.0, 2.0)),
      true,
    ),
  ];
  // Neither in view: what the floor alone looks like
  let hidden = Transform::from_xy(-500.0, -500.0);
  for entity in [sprite, bones] {
    *engine.world_mut().get_mut::<Transform>(entity).unwrap() = hidden;
  }
  engine.step_frames(2).unwrap();
  let empty = frame(&engine);
  for (place, transform, resized) in places {
    // One of the two dressed and in place, the other out of view
    let shown = |engine: &mut HeadlessEngine, dress: Dress, which: Entity, other: Entity| {
      let world = engine.world_mut();
      let was = world.get::<Renderable>(which).unwrap().clone();
      *world.get_mut::<Renderable>(which).unwrap() = dress(Renderable {
        image: was.image,
        style: was.style,
        ..Renderable::skeleton()
      });
      *world.get_mut::<Transform>(which).unwrap() = transform;
      *world.get_mut::<Transform>(other).unwrap() = hidden;
      engine.step_frames(2).unwrap();
      frame(engine)
    };
    let same = |as_sprite: &image::RgbaImage, as_bones: &image::RgbaImage, look: &str| {
      for (x, y, pixel) in as_sprite.enumerate_pixels() {
        let got = as_bones.get_pixel(x, y).0;
        assert_eq!(got, pixel.0, "{place}, {look}: pixel ({x}, {y})");
      }
    };
    // Plain first: the sprite draws, and the skeleton draws the same
    let plain = shown(&mut engine, |r| r, sprite, bones);
    let drew = plain
      .pixels()
      .zip(empty.pixels())
      .filter(|(drawn, empty)| drawn != empty)
      .count();
    assert!(drew > 20, "{place}: the sprite drew ({drew} pixels)");
    same(&plain, &shown(&mut engine, |r| r, bones, sprite), "plain");
    for (look, dress, needs_resizing) in looks {
      if needs_resizing && !resized {
        continue;
      }
      // The look shows on the sprite, or the comparison would prove nothing
      let as_sprite = shown(&mut engine, dress, sprite, bones);
      assert!(
        as_sprite != plain,
        "{place}, {look}: the look changes how the sprite draws"
      );
      same(&as_sprite, &shown(&mut engine, dress, bones, sprite), look);
    }
  }
}

#[test]
fn a_region_s_colour_is_its_slot_s_times_its_attachment_s_times_the_tint() {
  let Some(mut engine) = engine() else {
    return;
  };
  // A white image through a slot that halves green and blue, an attachment
  // that halves red and blue, and a tint that halves red and green: every
  // channel is 255 x 128/255 x 128/255 = 64, a product of two factors that
  // are not 1, each from a different pair of the three. Beside it a slot
  // that adds light, over a grey sprite
  let world = engine.world_mut();
  let grey = solid(world, (16, 16), [100, 100, 100, 255]);
  world.spawn((
    Transform::from_xy(48.0, 32.0),
    Renderable::image(grey).with_z_order(-1),
  ));
  let white = solid(world, (8, 8), WHITE);
  let slot = |name: &str, color: Color, blend: SlotBlend| SlotSetup {
    color,
    attachment: Some(Arc::from(name)),
    blend,
  };
  let region = |x: f32, color: Color| {
    SkeletonAttachment::Region(RegionAttachment {
      x,
      color,
      ..RegionAttachment::new(0, 8.0, 8.0)
    })
  };
  let skin = SkeletonData::DEFAULT_SKIN;
  let data = SkeletonData::builder()
    .bone("root", None, BoneSetup::default())
    .slot(
      "body",
      "root",
      slot("body", Color::new(255, 128, 128, 255), SlotBlend::Normal),
    )
    .slot(
      "glow",
      "root",
      slot("glow", Color::new(100, 0, 0, 255), SlotBlend::Additive),
    )
    .region(SkeletonRegion {
      name: Arc::from("white"),
      image: white,
      x: 0,
      y: 0,
      width: 8,
      height: 8,
    })
    .attachment(
      skin,
      "body",
      "body",
      region(0.0, Color::new(128, 255, 128, 255)),
    )
    .attachment(skin, "glow", "glow", region(32.0, Color::WHITE))
    .build()
    .expect("a valid skeleton");
  let data = world.resource_mut::<Assets<SkeletonData>>().add(data);
  let hero = stand(world, data, (16.0, 32.0), Renderable::skeleton());
  // Without the tint: the slot's times the attachment's alone
  let drawn = stepped(&mut engine);
  assert_eq!(drawn.get_pixel(16, 32).0, [128, 128, 64, 255], "untinted");
  // With it, 64 in every channel
  engine.world_mut().get_mut::<Renderable>(hero).unwrap().tint = Color::new(128, 128, 255, 255);
  let drawn = stepped(&mut engine);
  assert_eq!(drawn.get_pixel(16, 32).0, [64, 64, 64, 255], "tinted");
  // The additive slot over grey: more red, the rest as it was (the tint
  // takes its green and blue, which it has none of)
  let glow = drawn.get_pixel(48, 32).0;
  assert!(glow[0] > 105, "red added: {glow:?}");
  assert_eq!((glow[1], glow[2]), (100, 100), "{glow:?}");
  assert_eq!(drawn.get_pixel(48, 26).0, [100, 100, 100, 255], "the grey");

  // The alphas multiply too: at half alpha it is part of the way to the
  // background, and at none it is gone
  let background = drawn.get_pixel(SIZE - 1, SIZE - 1).0;
  assert!(background[0] < 60, "a dark background: {background:?}");
  engine
    .world_mut()
    .get_mut::<Renderable>(hero)
    .unwrap()
    .alpha = 0.5;
  let faded = stepped(&mut engine).get_pixel(16, 32).0;
  assert!(
    faded[0] > background[0] && faded[0] < 64,
    "between {background:?} and 64: {faded:?}"
  );
  engine
    .world_mut()
    .get_mut::<Renderable>(hero)
    .unwrap()
    .alpha = 0.0;
  assert_eq!(stepped(&mut engine).get_pixel(16, 32).0, background);
}

#[cfg(dev_tools)]
#[test]
fn debug_skeletons_draws_bones_bounding_boxes_and_points_on_top() {
  let Some(mut engine) = engine() else {
    return;
  };
  // A white body 24 px square in the dark, with a bone 16 px long pointing
  // right from the entity, a bounding box and a point
  let world = engine.world_mut();
  let image = solid(world, (24, 24), WHITE);
  let shown = |name: &str| SlotSetup {
    attachment: Some(Arc::from(name)),
    ..SlotSetup::default()
  };
  let data = SkeletonData::builder()
    .bone(
      "root",
      None,
      BoneSetup {
        length: 16.0,
        ..BoneSetup::default()
      },
    )
    .slot("body", "root", shown("body"))
    .slot("hit", "root", shown("hit"))
    .slot("muzzle", "root", shown("muzzle"))
    .region(SkeletonRegion {
      name: Arc::from("body"),
      image,
      x: 0,
      y: 0,
      width: 24,
      height: 24,
    })
    .attachment(
      SkeletonData::DEFAULT_SKIN,
      "body",
      "body",
      SkeletonAttachment::Region(RegionAttachment::new(0, 24.0, 24.0)),
    )
    .attachment(
      SkeletonData::DEFAULT_SKIN,
      "hit",
      "hit",
      SkeletonAttachment::BoundingBox(BoundingBoxAttachment {
        vertices: vec![
          Vec2::new(-8.0, 4.0),
          Vec2::new(8.0, 4.0),
          Vec2::new(8.0, 10.0),
          Vec2::new(-8.0, 10.0),
        ],
      }),
    )
    .attachment(
      SkeletonData::DEFAULT_SKIN,
      "muzzle",
      "muzzle",
      SkeletonAttachment::Point(PointAttachment {
        x: 0.0,
        y: -8.0,
        rotation: 0.0,
      }),
    )
    .build()
    .expect("a valid skeleton");
  let data = world.resource_mut::<Assets<SkeletonData>>().add(data);
  // On a pixel's centre, so its lines cover whole pixels
  stand(world, data, (32.5, 32.5), Renderable::skeleton());
  world
    .resource_mut::<LightingSettings>()
    .set_ambient(AmbientLight::with_intensity(0.0));
  world.resource_mut::<LightingSettings>().set_enabled(true);
  engine.step_frames(2).unwrap();
  let dark = frame(&engine);
  for (x, y, pixel) in dark.enumerate_pixels() {
    assert!(pixel.0[0] < 3, "dark at ({x}, {y}): {:?}", pixel.0);
  }

  // The overlay is unlit and over the body: the bone along the row at y 32
  // from the entity, the box's edges on the rows at 36 and 42, the point's
  // mark 8 px above the entity
  assert!(!engine.console_run("debug:skeletons").is_empty());
  engine.step_frames(2).unwrap();
  let drawn = frame(&engine);
  let lit = |x: u32, y: u32| drawn.get_pixel(x, y).0.iter().take(3).any(|&c| c > 100);
  assert!((34..46).all(|x| lit(x, 32)), "the bone");
  assert!((26..38).all(|x| lit(x, 36)), "the bounding box's top edge");
  assert!((26..38).all(|x| lit(x, 42)), "its bottom edge");
  assert!(lit(32, 24), "the point");
  assert!(!lit(26, 30) && !lit(38, 30), "the body stays dark");

  engine.console_run("debug:skeletons");
  engine.step_frames(2).unwrap();
  assert_eq!(frame(&engine), dark, "switched off again");
}

#[test]
fn an_interpolated_mirrored_skeleton_keeps_its_mirror_between_steps() {
  let Some(mut engine) = engine() else {
    return;
  };
  // Mirrored by its entity's scale, and interpolated: the arm lies left of
  // the entity, x 14..22 and y 24..28, the cuff over its near half
  let world = engine.world_mut();
  let data = lopsided(world);
  let hero = stand(world, data, (32.0, 32.0), Renderable::skeleton());
  world.entity_mut(hero).insert(Interpolated);
  world.get_mut::<Transform>(hero).unwrap().scale = Vec2::new(-1.0, 1.0);
  engine.step_frames(2).unwrap();
  let as_it_was = [((14, 24, 8, 4), RED), ((18, 24, 4, 4), GREEN)];
  assert_boxes(&frame(&engine), &as_it_was, "mirrored");

  // It grows to twice as wide in one step. The whole step's frame draws it
  // as it was, and half a step later it is half as wide again, still
  // mirrored and the right way up: the arm spans x 5..17 (32 - 1.5 x 18 to
  // 32 - 1.5 x 10) and the cuff x 11..17
  engine.world_mut().get_mut::<Transform>(hero).unwrap().scale = Vec2::new(-2.0, 1.0);
  assert_boxes(&stepped(&mut engine), &as_it_was, "the step it grows in");
  part_step(&mut engine, 5);
  assert_boxes(
    &frame(&engine),
    &[((5, 24, 12, 4), RED), ((11, 24, 6, 4), GREEN)],
    "half way, still mirrored",
  );
}

#[test]
fn an_item_on_a_flipped_skeleton_s_socket_mirrors_with_it() {
  let Some(mut engine) = engine() else {
    return;
  };
  // On the tip of the arm, an item whose image is the same neither mirrored
  // nor turned: 8 x 6 px, a blue top row over a red left half and a green
  // right half, drawn over the skeleton
  let world = engine.world_mut();
  let data = lopsided(world);
  let hero = stand(world, data, (32.0, 32.0), Renderable::skeleton());
  let image = two_tone(world);
  world.spawn((
    BoneSocket::new("arm").with_offset(Transform::from_xy(8.0, 0.0)),
    Renderable::image(image).with_z_order(1),
    ChildOf(hero),
  ));
  // The item with its centre on (x, y): its halves as listed left to
  // right, its blue row at the top or the bottom
  let item = |x: u32, y: u32, halves: [[u8; 4]; 2], blue_on_top: bool| {
    let blue_row = if blue_on_top { y - 3 } else { y + 2 };
    [
      ((x - 4, y - 3, 4, 6), halves[0]),
      ((x, y - 3, 4, 6), halves[1]),
      ((x - 4, blue_row, 8, 1), BLUE),
    ]
  };
  let drawn = |arm: Box4, cuff: Box4, item: [(Box4, [u8; 4]); 3]| {
    let mut boxes = vec![(arm, RED), (cuff, GREEN)];
    boxes.extend(item);
    boxes
  };
  assert_boxes(
    &stepped(&mut engine),
    &drawn(
      (42, 24, 8, 4),
      (42, 24, 4, 4),
      item(50, 26, [RED, GREEN], true),
    ),
    "unflipped",
  );

  // Flipped left to right, the item mirrors with the character: its halves
  // swap and its top row stays on top (turned half a turn instead, its top
  // row would be at the bottom)
  engine
    .world_mut()
    .get_mut::<Renderable>(hero)
    .unwrap()
    .flip_x = true;
  assert_boxes(
    &stepped(&mut engine),
    &drawn(
      (14, 24, 8, 4),
      (18, 24, 4, 4),
      item(14, 26, [GREEN, RED], true),
    ),
    "flipped left to right",
  );

  // Flipped both ways it is turned half a turn, as the character is
  engine
    .world_mut()
    .get_mut::<Renderable>(hero)
    .unwrap()
    .flip_y = true;
  assert_boxes(
    &stepped(&mut engine),
    &drawn(
      (14, 36, 8, 4),
      (18, 36, 4, 4),
      item(14, 38, [GREEN, RED], false),
    ),
    "flipped both ways",
  );
}

#[test]
fn a_skeleton_riding_a_flipped_skeleton_s_socket_keeps_its_mirror_between_steps() {
  let Some(mut engine) = engine() else {
    return;
  };
  // A mount, flipped left to right and interpolated, whose seat turns 0.3
  // radians a step; and on a socket on the seat a rider: a skeleton whose
  // white 6 px square hangs 12 px below its origin
  let world = engine.world_mut();
  let parts = [
    Part {
      colour: [0, 0, 0, 0],
      ..part("body", WHITE, (1, 1))
    },
    Part {
      colour: [0, 0, 0, 0],
      ..part("seat", WHITE, (1, 1)).under("body", 0.0, 0.0)
    },
  ];
  let mount = rig_with(world, &parts, Images::Each, |builder| {
    builder.animation(SkeletonClip {
      name: Arc::from("turn"),
      duration: 10.0,
      timelines: vec![SkeletonTimeline::Rotate {
        bone: BoneId(1),
        frames: vec![
          (0.0, 0.0, SkeletonCurve::Linear),
          (10.0, 180.0, SkeletonCurve::Linear),
        ],
      }],
    })
  });
  let mount = stand(
    world,
    mount,
    (32.0, 32.0),
    Renderable::skeleton().with_flip_x(true),
  );
  let mut animation = SkeletonAnimation::new();
  animation.set_animation(0, "turn", false);
  world.entity_mut(mount).insert((animation, Interpolated));
  let rider = rig(world, &[part("body", WHITE, (6, 6)).at(0.0, 12.0)]);
  let rider = world
    .spawn((
      BoneSocket::new("seat"),
      Skeleton::new(rider),
      Renderable::skeleton(),
      ChildOf(mount),
    ))
    .id();
  engine.step_frames(3).unwrap();
  assert!(
    global(&engine, rider).matrix2.determinant() < 0.0,
    "the rider is mirrored with its mount"
  );

  // The seat's turn now, read from its bone: mirrored, its x axis is
  // (-cos, sin) of it. A step ago it was 18 radians a second less
  let seat = bone(&engine, mount, "seat").matrix2.x_axis;
  let now = seat.y.atan2(-seat.x);
  let a_step = 18.0 * engine.world().resource::<Time>().fixed.timestep();
  assert!(now > 0.5 && now < 1.2, "three steps of 0.3: {now}");
  // Where the rider's square is with the seat turned by `turn`: mirrored,
  // 12 px down the seat's y axis is (12 sin, 12 cos) from the mount. Turned
  // half a turn instead of mirrored, it would be straight across the mount
  let centre = |turn: f32| Vec2::new(32.0, 32.0) + Vec2::new(turn.sin(), turn.cos()) * 12.0;
  let across = |turn: f32| Vec2::new(64.0, 64.0) - centre(turn);
  let background = frame(&engine).get_pixel(SIZE - 1, SIZE - 1).0;
  let at = |drawn: &image::RgbaImage, at: Vec2| drawn.get_pixel(at.x as u32, at.y as u32).0;

  // A frame of whole steps draws it where it was a step ago, and half a
  // step later half way round to where it is, mirrored all the way
  let drawn = frame(&engine);
  assert_eq!(at(&drawn, centre(now - a_step)), WHITE, "a step ago");
  assert_eq!(at(&drawn, across(now - a_step)), background);
  part_step(&mut engine, 5);
  let drawn = frame(&engine);
  let half_way = now - a_step / 2.0;
  assert_eq!(at(&drawn, centre(half_way)), WHITE, "half way round");
  assert_eq!(at(&drawn, across(half_way)), background, "not turned over");
}

#[test]
fn overlapping_parts_cast_a_darker_drop_shadow_than_one_part() {
  let Some(mut engine) = engine() else {
    return;
  };
  // A limit, pinned so that it shows if it ever changes: each region casts
  // its own drop shadow, so where two parts overlap a translucent shadow is
  // laid twice and is darker than a sprite's uniform one. Two 8 px parts,
  // 4 px apart, over a white floor, with a half black shadow 16 px down
  let world = engine.world_mut();
  white_floor(world);
  let parts = [
    part("left", RED, (8, 8)),
    part("right", BLUE, (8, 8)).under("left", 4.0, 0.0),
  ];
  let data = rig(world, &parts);
  let shadow = DropShadow::new()
    .offset(0.0, 16.0)
    .color(Color::new(0, 0, 0, 128));
  stand(
    world,
    data,
    (20.0, 20.0),
    Renderable::skeleton().with_drop_shadow(shadow),
  );
  let drawn = stepped(&mut engine);
  // The left part's shadow alone at x 16..20, both at 20..24, the right
  // one's alone at 24..28
  let (one, both, other) = (
    drawn.get_pixel(18, 36).0,
    drawn.get_pixel(22, 36).0,
    drawn.get_pixel(26, 36).0,
  );
  assert_eq!(
    one,
    [SHADOW_ONCE, SHADOW_ONCE, SHADOW_ONCE, 255],
    "one part"
  );
  assert_eq!(other, one, "the other part alone");
  assert_eq!(
    both,
    [SHADOW_TWICE, SHADOW_TWICE, SHADOW_TWICE, 255],
    "both parts"
  );
}

/// What a half black shadow leaves of a white floor, laid once and laid
/// twice (blended in linear light)
const SHADOW_ONCE: u8 = 187;
const SHADOW_TWICE: u8 = 136;

#[test]
fn a_part_faded_by_its_slot_casts_less_shadow() {
  let Some(mut engine) = engine() else {
    return;
  };
  // Two 8 px parts over a white floor with a black shadow 16 px down: the
  // one whose slot is half transparent casts half a shadow
  let world = engine.world_mut();
  white_floor(world);
  let parts = [
    part("solid", RED, (8, 8)).at(-10.0, 0.0),
    part("ghost", RED, (8, 8))
      .at(10.0, 0.0)
      .slot(Color::new(255, 255, 255, 128)),
  ];
  let data = rig(world, &parts);
  let shadow = DropShadow::new()
    .offset(0.0, 16.0)
    .color(Color::new(0, 0, 0, 255));
  stand(
    world,
    data,
    (32.0, 20.0),
    Renderable::skeleton().with_drop_shadow(shadow),
  );
  let drawn = stepped(&mut engine);
  assert_eq!(drawn.get_pixel(22, 36).0, BLACK, "a solid part's shadow");
  let ghost = drawn.get_pixel(42, 36).0;
  assert_eq!(
    ghost,
    [SHADOW_ONCE, SHADOW_ONCE, SHADOW_ONCE, 255],
    "a half transparent part's shadow"
  );
}

#[test]
fn a_flipped_skeleton_s_bones_and_sockets_jump_together() {
  let Some(mut engine) = engine() else {
    return;
  };
  // An interpolated skeleton at x 32 whose hand slides 4 px a step, a
  // white square on a socket 8 px below the hand
  let world = engine.world_mut();
  let data = slider_by(world, 4.0);
  let hero = sliding(world, &data, (32.0, 16.0));
  world.entity_mut(hero).insert(Interpolated);
  let image = solid(world, (4, 4), WHITE);
  world.spawn((
    BoneSocket::new("hand").with_offset(Transform::from_xy(0.0, 8.0)),
    Renderable::image(image),
    ChildOf(hero),
  ));
  engine.step_frames(2).unwrap();
  // Two steps: the hand is 8 px along, drawn a step behind with its socket
  let drawn = frame(&engine);
  assert!(near(bone(&engine, hero, "hand").translation.x, 40.0));
  assert_eq!(square(&drawn, 36.0, 16.0), Some(true), "a step behind");
  assert_eq!(square(&drawn, 36.0, 24.0), Some(true), "its socket with it");

  // Flipped in the next step, the hand is 12 px the other way, at x 20.
  // The socket lands there at once, and so does the bone: not drawn a step
  // behind on the new side (at 24) while its socket is at 20
  engine
    .world_mut()
    .get_mut::<Renderable>(hero)
    .unwrap()
    .flip_x = true;
  let drawn = stepped(&mut engine);
  assert!(near(bone(&engine, hero, "hand").translation.x, 20.0));
  assert_eq!(square(&drawn, 20.0, 24.0), Some(true), "the socket lands");
  assert_eq!(square(&drawn, 20.0, 16.0), Some(true), "the bone with it");
  assert_eq!(square(&drawn, 24.0, 16.0), Some(false));

  // The step after it is drawn between steps again: a step behind, at 20
  // while it is at 16
  let drawn = stepped(&mut engine);
  assert!(near(bone(&engine, hero, "hand").translation.x, 16.0));
  assert_eq!(
    square(&drawn, 20.0, 16.0),
    Some(true),
    "a step behind again"
  );
  assert_eq!(square(&drawn, 20.0, 24.0), Some(true));

  // Flipped back while no step runs, bone and socket land together in that
  // frame, 16 px along on the first side: at 48, not the bone a step behind
  time(&mut engine).game.pause();
  engine
    .world_mut()
    .get_mut::<Renderable>(hero)
    .unwrap()
    .flip_x = false;
  let drawn = stepped(&mut engine);
  assert!(near(bone(&engine, hero, "hand").translation.x, 48.0));
  assert_eq!(square(&drawn, 48.0, 24.0), Some(true), "the socket lands");
  assert_eq!(square(&drawn, 48.0, 16.0), Some(true), "the bone with it");
  assert_eq!(square(&drawn, 44.0, 16.0), Some(false));
}

#[test]
fn a_skeleton_whose_pose_is_stale_draws_nothing_not_even_its_shadows() {
  let Some(mut engine) = engine() else {
    return;
  };
  // Over a white floor, a red body standing on (32, 40) with a blob shadow
  // under its feet
  let world = engine.world_mut();
  white_floor(world);
  let data = rig(world, &[part("body", RED, (8, 8)).at(0.0, -4.0)]);
  let hero = stand(
    world,
    data.clone(),
    (32.0, 40.0),
    Renderable::skeleton().with_blob_shadow(BlobShadow::circle(6.0)),
  );
  let drawn = stepped(&mut engine);
  assert_eq!(drawn.get_pixel(32, 36).0, RED, "the body");
  assert_ne!(drawn.get_pixel(32, 41).0, WHITE, "its shadow");

  // Its data changes while it is disabled, and it is enabled again while
  // no step runs: nothing has made it a pose for the data as it now is.
  // It draws nothing, its shadows neither, until its next step
  engine.world_mut().entity_mut(hero).insert(Disabled);
  let mut assets = engine.world_mut().resource_mut::<Assets<SkeletonData>>();
  let again = assets.get(&data).expect("the data").clone();
  *assets.get_mut(&data).expect("the data") = again;
  engine.step().unwrap();
  time(&mut engine).game.pause();
  engine.world_mut().entity_mut(hero).remove::<Disabled>();
  let drawn = stepped(&mut engine);
  assert_eq!(drawn.get_pixel(32, 36).0, WHITE, "no body");
  assert_eq!(
    drawn.get_pixel(32, 41).0,
    WHITE,
    "and no shadow without one"
  );

  time(&mut engine).game.resume();
  let drawn = stepped(&mut engine);
  assert_eq!(drawn.get_pixel(32, 36).0, RED, "the body again");
  assert_ne!(drawn.get_pixel(32, 41).0, WHITE, "and its shadow");
}

/// What the scenes of the hold test tell the test, and what it asks of them
#[derive(Default)]
struct Board {
  /// The level's skeleton
  hero: Option<Entity>,
  /// The level pushes the pause menu in its next update
  push: bool,
  /// The pause menu pops in its next update
  pop: bool,
}

type Shared = Arc<std::sync::Mutex<Board>>;

/// A level with an interpolated skeleton whose hand slides 4 px a step
/// from x 8
struct Level(Shared);

impl Scene for Level {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    ctx.look().tonemap = Tonemap::Clamp;
    let data = slider_by(ctx.world, 4.0);
    let hero = sliding(ctx.world, &data, (8.0, 16.0));
    ctx.world.entity_mut(hero).insert(Interpolated);
    self.0.lock().expect("the board").hero = Some(hero);
  }

  fn update(&mut self, _ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    if std::mem::take(&mut self.0.lock().expect("the board").push) {
      return SceneAction::Push(Box::new(Menu(self.0.clone())));
    }
    SceneAction::Continue
  }
}

/// A pause menu over the level, which stops the level updating
struct Menu(Shared);

impl Scene for Menu {
  fn update(&mut self, _ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    if std::mem::take(&mut self.0.lock().expect("the board").pop) {
      SceneAction::Pop
    } else {
      SceneAction::Continue
    }
  }
}

#[test]
fn a_held_skeleton_still_draws_and_stands_still() {
  let board = Shared::default();
  let builder = EngineBuilder::new()
    .pixel_art(SIZE, SIZE)
    .start_scene(Level(board.clone()));
  let Some(mut engine) = HeadlessEngine::new_or_skip(builder) else {
    return;
  };
  engine.step_frames(3).unwrap();
  let hero = board.lock().unwrap().hero.expect("the level's skeleton");
  let hand = |engine: &HeadlessEngine| bone(engine, hero, "hand").translation.x;
  // Three steps: the hand is at 20, drawn a step behind at 16
  assert!(near(hand(&engine), 20.0), "{}", hand(&engine));
  assert_eq!(square(&frame(&engine), 16.0, 16.0), Some(true));

  // A pause menu goes over the level, which stops updating: its skeleton
  // is held. It still draws, where it is, and stands still there while
  // steps run and frames fall between them: it is not drawn a step behind,
  // nor swaying with the frame's fraction
  board.lock().unwrap().push = true;
  engine.step_frames(2).unwrap();
  let held_at = hand(&engine);
  assert!(held_at < 30.0, "it stopped soon after: {held_at}");
  for part in [0, 5, 2, 0] {
    if part == 0 {
      engine.step().unwrap();
    } else {
      part_step(&mut engine, part);
    }
    let drawn = frame(&engine);
    assert_eq!(hand(&engine), held_at, "held");
    assert_eq!(
      square(&drawn, held_at, 16.0),
      Some(true),
      "drawn where it is (after {part} tenths of a step)"
    );
    assert_eq!(square(&drawn, held_at - 4.0, 16.0), Some(false));
  }

  // The menu goes: the level's skeleton carries on from where it was
  board.lock().unwrap().pop = true;
  engine.step_frames(3).unwrap();
  assert!(hand(&engine) > held_at, "it moves again: {}", hand(&engine));
}
