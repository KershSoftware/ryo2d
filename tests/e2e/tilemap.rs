//! Tile layers drawn through the world batch: tiles, flips, mixed tilesets,
//! animation on game time, opacity and tint, offsets, visibility, lighting
//! and render layers, read back as display bytes (pixel art, `Clamp`, so
//! colours come back as they went in).
//!
//! Tests skip when no GPU is available.

use std::sync::{Arc, Mutex};

use ryo2d::prelude::*;
use ryo2d::testing::HeadlessEngine;

const RED: [u8; 4] = [220, 30, 40, 255];
const GREEN: [u8; 4] = [40, 200, 60, 255];
const BLUE: [u8; 4] = [30, 60, 230, 255];
const YELLOW: [u8; 4] = [240, 220, 20, 255];
const WHITE: [u8; 4] = [255; 4];

/// A cell's side, and a tile's, in pixels
const CELL: u32 = 8;
/// The frame's side in pixels; the map covers it
const SIZE: u32 = 64;

type Setup = Box<dyn FnMut(&mut SceneContext) + Send>;

/// Runs its setup on enter and idles
struct Fixture(Option<Setup>);

impl Scene for Fixture {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    ctx.look().tonemap = Tonemap::Clamp;
    if let Some(mut setup) = self.0.take() {
      setup(ctx);
    }
  }

  fn update(&mut self, _ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    SceneAction::Continue
  }
}

/// An engine whose scene runs `setup` as it enters, stepped until the maps
/// it spawned have landed
fn engine_with(
  builder: EngineBuilder,
  setup: impl FnMut(&mut SceneContext) + Send + 'static,
) -> Option<HeadlessEngine> {
  let mut engine =
    HeadlessEngine::new_or_skip(builder.start_scene(Fixture(Some(Box::new(setup)))))?;
  land(&mut engine);
  Some(engine)
}

/// Steps until every map root has landed: a map's colliders are worked out
/// off the main thread, and it lands whole a frame or so after its asset
/// is ready
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

fn engine(setup: impl FnMut(&mut SceneContext) + Send + 'static) -> Option<HeadlessEngine> {
  engine_with(EngineBuilder::new().pixel_art(SIZE, SIZE), setup)
}

/// The frame after two steps of `setup`
fn render(setup: impl FnMut(&mut SceneContext) + Send + 'static) -> Option<image::RgbaImage> {
  let mut engine = engine(setup)?;
  engine.step_frames(2).unwrap();
  Some(engine.frame().unwrap())
}

/// An atlas tileset of `count` tiles in a row; texel (x, y) of tile `id` is
/// `paint(id, x, y)`
fn tileset(
  ctx: &mut SceneContext,
  count: u32,
  paint: impl Fn(u32, u32, u32) -> [u8; 4],
) -> Tileset {
  let (width, height) = (CELL * count, CELL);
  let mut data = Vec::with_capacity((width * height * 4) as usize);
  for y in 0..height {
    for x in 0..width {
      data.extend_from_slice(&paint(x / CELL, x % CELL, y));
    }
  }
  let image = ctx.assets().add(Image::from_rgba(data, width, height));
  let size = UVec2::new(width, height);
  let mut set = Tileset::new(
    "tiles",
    UVec2::splat(CELL),
    TilesetImage::Atlas { image, size },
  );
  set.columns = count;
  set.tile_count = count;
  set
}

/// A tileset of one solid tile per colour
fn solid(ctx: &mut SceneContext, colours: &[[u8; 4]]) -> Tileset {
  let colours = colours.to_vec();
  tileset(ctx, colours.len() as u32, move |id, _, _| {
    colours[id as usize]
  })
}

/// A map covering the frame over `tilesets`, with a tile layer "ground"
/// holding `tiles`
fn map(tilesets: Vec<Tileset>, tiles: &[(IVec2, Tile)]) -> TileMap {
  let mut map = TileMap::new(UVec2::splat(SIZE / CELL), UVec2::splat(CELL));
  for set in tilesets {
    map.add_tileset(TilesetSource::Embedded(Arc::new(set)));
  }
  let layer = map.add_tile_layer("ground");
  put(&mut map, layer, tiles);
  map
}

fn put(map: &mut TileMap, layer: usize, tiles: &[(IVec2, Tile)]) {
  let Some(grid) = map.tiles_mut(layer) else {
    panic!("layer {layer} is not a tile layer");
  };
  for &(cell, tile) in tiles {
    assert!(grid.set(cell, Some(tile)), "{cell} is in the map");
  }
}

fn spawn(ctx: &mut SceneContext, map: TileMap, at: Vec2) -> Entity {
  let handle = ctx.assets().add(map);
  ctx
    .world
    .spawn((MapRoot::new(handle), Transform::from_translation(at)))
    .id()
}

/// Checks every pixel: `expect`'s colour where it names one, the background
/// (the bottom-right pixel, which every test leaves empty) elsewhere
fn assert_pixels(frame: &image::RgbaImage, expect: impl Fn(u32, u32) -> Option<[u8; 4]>) {
  let background = frame.get_pixel(SIZE - 1, SIZE - 1).0;
  for colour in [RED, GREEN, BLUE, YELLOW, WHITE] {
    assert_ne!(background, colour, "the background is a tile colour");
  }
  for y in 0..SIZE {
    for x in 0..SIZE {
      let want = expect(x, y).unwrap_or(background);
      let got = frame.get_pixel(x, y).0;
      assert_eq!(got, want, "pixel ({x}, {y})");
    }
  }
}

/// The colour of the listed cell holding pixel (x, y), if any
fn in_cells(cells: &[(IVec2, [u8; 4])], x: u32, y: u32) -> Option<[u8; 4]> {
  let cell = IVec2::new((x / CELL) as i32, (y / CELL) as i32);
  cells
    .iter()
    .find(|(c, _)| *c == cell)
    .map(|(_, colour)| *colour)
}

#[test]
fn a_tile_layer_draws_its_tiles() {
  let Some(frame) = render(|ctx| {
    let set = solid(ctx, &[RED, GREEN, BLUE, YELLOW]);
    let tiles = [
      (IVec2::new(1, 1), Tile::new(0, 0)),
      (IVec2::new(4, 1), Tile::new(0, 1)),
      (IVec2::new(1, 4), Tile::new(0, 2)),
      (IVec2::new(4, 4), Tile::new(0, 3)),
    ];
    spawn(ctx, map(vec![set], &tiles), Vec2::ZERO);
  }) else {
    return;
  };
  let cells = [
    (IVec2::new(1, 1), RED),
    (IVec2::new(4, 1), GREEN),
    (IVec2::new(1, 4), BLUE),
    (IVec2::new(4, 4), YELLOW),
  ];
  assert_pixels(&frame, |x, y| in_cells(&cells, x, y));
}

/// An 8 px tile in quarters: red and green on top, blue and yellow below
fn quarters(x: u32, y: u32) -> [u8; 4] {
  match (x >= CELL / 2, y >= CELL / 2) {
    (false, false) => RED,
    (true, false) => GREEN,
    (false, true) => BLUE,
    (true, true) => YELLOW,
  }
}

/// The flips of `bits`: horizontal 1, vertical 2, diagonal 4
fn flip_of(bits: u32) -> TileFlip {
  let mut flip = TileFlip::NONE;
  if bits & 1 != 0 {
    flip = flip | TileFlip::HORIZONTAL;
  }
  if bits & 2 != 0 {
    flip = flip | TileFlip::VERTICAL;
  }
  if bits & 4 != 0 {
    flip = flip | TileFlip::DIAGONAL;
  }
  flip
}

#[test]
fn flipped_and_rotated_tiles_draw_as_tiled_shows_them() {
  // Every combination along row 1. A diagonal flip turns the quad over (its
  // axes swap), so a pipeline culling back faces would drop those four
  let Some(frame) = render(|ctx| {
    let set = tileset(ctx, 1, |_, x, y| quarters(x, y));
    let tiles: Vec<(IVec2, Tile)> = (0..8)
      .map(|bits| {
        let cell = IVec2::new(bits as i32, 1);
        (cell, Tile::new(0, 0).with_flip(flip_of(bits)))
      })
      .collect();
    spawn(ctx, map(vec![set], &tiles), Vec2::ZERO);
  }) else {
    return;
  };
  assert_pixels(&frame, |x, y| {
    if !(CELL..2 * CELL).contains(&y) {
      return None;
    }
    let bits = x / CELL;
    // Tiled transposes first, then mirrors: undo them in reverse to find
    // the tile's texel shown here
    let (mut tx, mut ty) = (x % CELL, y % CELL);
    if bits & 2 != 0 {
      ty = CELL - 1 - ty;
    }
    if bits & 1 != 0 {
      tx = CELL - 1 - tx;
    }
    if bits & 4 != 0 {
      std::mem::swap(&mut tx, &mut ty);
    }
    Some(quarters(tx, ty))
  });
}

#[test]
fn a_layer_mixing_tilesets_draws_each_tile_from_its_own() {
  let Some(frame) = render(|ctx| {
    let first = solid(ctx, &[RED, GREEN]);
    let second = solid(ctx, &[BLUE, YELLOW]);
    let tiles = [
      (IVec2::new(1, 1), Tile::new(0, 0)),
      (IVec2::new(2, 1), Tile::new(1, 0)),
      (IVec2::new(3, 1), Tile::new(0, 1)),
      (IVec2::new(4, 1), Tile::new(1, 1)),
    ];
    spawn(ctx, map(vec![first, second], &tiles), Vec2::ZERO);
  }) else {
    return;
  };
  let cells = [
    (IVec2::new(1, 1), RED),
    (IVec2::new(2, 1), BLUE),
    (IVec2::new(3, 1), GREEN),
    (IVec2::new(4, 1), YELLOW),
  ];
  assert_pixels(&frame, |x, y| in_cells(&cells, x, y));
}

#[test]
fn an_animated_tile_shows_the_frame_for_the_game_time_and_holds_while_paused() {
  let frames = [RED, GREEN, BLUE];
  let builder = EngineBuilder::new().pixel_art(SIZE, SIZE).fixed_hz(10);
  let Some(mut engine) = engine_with(builder, move |ctx| {
    let mut set = solid(ctx, &[WHITE, RED, GREEN, BLUE]);
    let frame = |tile| TileFrame {
      tile,
      duration: std::time::Duration::from_millis(300),
    };
    set.insert_tile(
      0,
      TileData {
        animation: vec![frame(1), frame(2), frame(3)],
        ..TileData::default()
      },
    );
    spawn(
      ctx,
      map(vec![set], &[(IVec2::new(2, 2), Tile::new(0, 0))]),
      Vec2::ZERO,
    );
  }) else {
    return;
  };
  let shown = |engine: &HeadlessEngine| engine.frame().unwrap().get_pixel(20, 20).0;
  let mut seen = Vec::new();
  for _ in 0..12 {
    engine.step().unwrap();
    let time = engine.world().resource::<Time>().game.elapsed();
    let beats = time / 0.3;
    // Too near a change of frame to say which shows
    if (beats - beats.round()).abs() < 0.05 {
      continue;
    }
    let want = frames[beats.floor() as usize % frames.len()];
    assert_eq!(shown(&engine), want, "at {time} s");
    if !seen.contains(&want) {
      seen.push(want);
    }
  }
  assert_eq!(seen.len(), frames.len(), "every frame showed");

  // Paused, the frame holds
  engine.world_mut().resource_mut::<Time>().game.pause();
  engine.step().unwrap();
  let held = shown(&engine);
  for _ in 0..6 {
    engine.step().unwrap();
    assert_eq!(shown(&engine), held);
  }
}

#[test]
fn opacity_and_tint_apply() {
  // A white tile on a tinted, half-opaque layer draws as a white sprite
  // tinted and faded alike
  let tint = Color::rgb(220, 120, 40);
  let Some(frame) = render(move |ctx| {
    let set = solid(ctx, &[WHITE]);
    let mut tiles = map(vec![set], &[(IVec2::new(1, 1), Tile::new(0, 0))]);
    tiles.layers[0].info.tint = tint;
    tiles.layers[0].info.opacity = 0.5;
    spawn(ctx, tiles, Vec2::ZERO);
    let white = ctx
      .assets()
      .add(Image::solid_rect(CELL, CELL, 255, 255, 255, 255));
    ctx.world.spawn((
      Transform::from_xy(32.0, 8.0),
      Renderable::image(white)
        .with_anchor(Anchor::TopLeft)
        .with_tint(tint)
        .with_alpha(0.5),
    ));
  }) else {
    return;
  };
  let background = frame.get_pixel(SIZE - 1, SIZE - 1).0;
  let sprite = frame.get_pixel(36, 12).0;
  assert_ne!(sprite, background);
  assert_ne!(sprite, WHITE);
  for y in CELL..2 * CELL {
    for x in CELL..2 * CELL {
      assert_eq!(frame.get_pixel(x, y).0, sprite, "pixel ({x}, {y})");
    }
  }
}

#[test]
fn a_disabled_map_or_a_hidden_layer_draws_nothing() {
  let spawned = Arc::new(Mutex::new(Vec::new()));
  let maps = spawned.clone();
  let Some(mut engine) = engine(move |ctx| {
    let set = solid(ctx, &[RED, GREEN]);
    let shown = spawn(
      ctx,
      map(vec![set.clone()], &[(IVec2::new(1, 1), Tile::new(0, 0))]),
      Vec2::ZERO,
    );
    let mut hidden = map(vec![set], &[(IVec2::new(4, 4), Tile::new(0, 1))]);
    hidden.layers[0].info.visible = false;
    let hidden = spawn(ctx, hidden, Vec2::ZERO);
    if let Ok(mut maps) = maps.lock() {
      maps.extend([shown, hidden]);
    }
  }) else {
    return;
  };
  engine.step_frames(2).unwrap();
  let shown = spawned.lock().unwrap()[0];
  let red = [(IVec2::new(1, 1), RED)];
  assert_pixels(&engine.frame().unwrap(), |x, y| in_cells(&red, x, y));

  // Disabling the map entity hides its layers, though they are not disabled
  engine.world_mut().entity_mut(shown).insert(Disabled);
  engine.step().unwrap();
  assert_pixels(&engine.frame().unwrap(), |_, _| None);

  engine.world_mut().entity_mut(shown).remove::<Disabled>();
  engine.step().unwrap();
  assert_pixels(&engine.frame().unwrap(), |x, y| in_cells(&red, x, y));
}

#[test]
fn a_layer_offset_shifts_its_tiles() {
  let Some(frame) = render(|ctx| {
    let set = solid(ctx, &[RED]);
    let mut tiles = map(vec![set], &[(IVec2::new(1, 1), Tile::new(0, 0))]);
    tiles.layers[0].info.offset = Vec2::new(3.0, 5.0);
    spawn(ctx, tiles, Vec2::ZERO);
  }) else {
    return;
  };
  assert_pixels(&frame, |x, y| {
    ((CELL + 3..2 * CELL + 3).contains(&x) && (CELL + 5..2 * CELL + 5).contains(&y)).then_some(RED)
  });
}

#[test]
fn a_map_draws_where_its_entity_is() {
  let Some(frame) = render(|ctx| {
    let set = solid(ctx, &[GREEN]);
    let tiles = map(vec![set], &[(IVec2::new(1, 1), Tile::new(0, 0))]);
    spawn(ctx, tiles, Vec2::new(16.0, 24.0));
  }) else {
    return;
  };
  assert_pixels(&frame, |x, y| {
    ((24..32).contains(&x) && (32..40).contains(&y)).then_some(GREEN)
  });
}

#[test]
fn a_lit_layer_takes_the_light_and_an_unlit_one_does_not() {
  let Some(frame) = render(|ctx| {
    let mut lighting = ctx.lighting();
    lighting.set_ambient(AmbientLight::with_intensity(0.0));
    lighting.set_enabled(true);
    let set = solid(ctx, &[WHITE]);
    let mut tiles = map(vec![set], &[(IVec2::new(1, 1), Tile::new(0, 0))]);
    let unlit = tiles.add_tile_layer("unlit");
    put(&mut tiles, unlit, &[(IVec2::new(4, 1), Tile::new(0, 0))]);
    tiles.layers[unlit].info.lit = false;
    spawn(ctx, tiles, Vec2::ZERO);
  }) else {
    return;
  };
  let lit = frame.get_pixel(12, 12).0;
  assert!(lit[0] < 10 && lit[1] < 10 && lit[2] < 10, "lit {lit:?}");
  assert_eq!(frame.get_pixel(36, 12).0, WHITE);
}

#[test]
fn a_map_off_the_camera_s_render_layers_draws_nothing() {
  let Some(frame) = render(|ctx| {
    let set = solid(ctx, &[RED]);
    let tiles = map(vec![set], &[(IVec2::new(1, 1), Tile::new(0, 0))]);
    let map = spawn(ctx, tiles, Vec2::ZERO);
    ctx.world.entity_mut(map).insert(RenderLayers::layer(3));
  }) else {
    return;
  };
  assert_pixels(&frame, |_, _| None);
}

#[test]
fn layers_draw_bottom_first_at_the_map_s_z_among_entities() {
  // Layer 0 at z 4, layer 1 at z 5: the top layer covers the bottom one at
  // (1, 1); a sprite at z 3 is under the map at (4, 1), one at z 6 over it
  // at (6, 1)
  let Some(frame) = render(|ctx| {
    let set = solid(ctx, &[RED, GREEN]);
    let bottom = [
      (IVec2::new(1, 1), Tile::new(0, 0)),
      (IVec2::new(6, 1), Tile::new(0, 0)),
    ];
    let mut tiles = map(vec![set], &bottom);
    let top = tiles.add_tile_layer("top");
    let over = [
      (IVec2::new(1, 1), Tile::new(0, 1)),
      (IVec2::new(4, 1), Tile::new(0, 1)),
    ];
    put(&mut tiles, top, &over);
    let handle = ctx.assets().add(tiles);
    ctx
      .world
      .spawn((MapRoot::new(handle).with_z(4), Transform::IDENTITY));
    let blue = ctx
      .assets()
      .add(Image::solid_rect(CELL, CELL, 30, 60, 230, 255));
    for (x, z) in [(32.0, 3), (48.0, 6)] {
      ctx.world.spawn((
        Transform::from_xy(x, 8.0),
        Renderable::image(blue.clone())
          .with_anchor(Anchor::TopLeft)
          .with_z_order(z),
      ));
    }
  }) else {
    return;
  };
  let cells = [
    (IVec2::new(1, 1), GREEN),
    (IVec2::new(4, 1), GREEN),
    (IVec2::new(6, 1), BLUE),
  ];
  assert_pixels(&frame, |x, y| in_cells(&cells, x, y));
}

/// A camera position the scene moves to on its next update
type Pan = Arc<Mutex<Option<Vec2>>>;

/// Moves the main camera to where `Pan` says, then idles
struct Panned(Option<Setup>, Pan);

impl Scene for Panned {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    ctx.look().tonemap = Tonemap::Clamp;
    if let Some(mut setup) = self.0.take() {
      setup(ctx);
    }
  }

  fn update(&mut self, ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    if let Some(at) = self.1.lock().ok().and_then(|mut pan| pan.take()) {
      ctx.camera().set_position(at.x, at.y);
    }
    SceneAction::Continue
  }
}

#[test]
fn a_parallax_layer_follows_part_of_the_camera() {
  // Parallax 0.5: the layer draws offset by half the camera's centre from
  // the map's corner (Tiled's rule), so it moves half as far as the camera
  let pan = Pan::default();
  let setup: Setup = Box::new(|ctx| {
    let set = solid(ctx, &[RED, GREEN]);
    let mut tiles = map(vec![set], &[(IVec2::new(1, 1), Tile::new(0, 0))]);
    let fixed = tiles.add_tile_layer("fixed");
    put(&mut tiles, fixed, &[(IVec2::new(3, 5), Tile::new(0, 1))]);
    tiles.layers[0].info.parallax = Vec2::splat(0.5);
    spawn(ctx, tiles, Vec2::ZERO);
  });
  let builder = EngineBuilder::new()
    .pixel_art(SIZE, SIZE)
    .start_scene(Panned(Some(setup), pan.clone()));
  let Some(mut engine) = HeadlessEngine::new_or_skip(builder) else {
    return;
  };
  engine.step_frames(2).unwrap();
  // The camera is centred on (32, 32): the layer is 16 px down and right
  let at = |x: u32, y: u32, colour| {
    move |px: u32, py: u32| {
      ((x..x + CELL).contains(&px) && (y..y + CELL).contains(&py)).then_some(colour)
    }
  };
  let (red, green) = (at(24, 24, RED), at(24, 40, GREEN));
  assert_pixels(&engine.frame().unwrap(), |x, y| red(x, y).or(green(x, y)));

  // 16 px right: the world moves 16 px left, the layer 8
  *pan.lock().unwrap() = Some(Vec2::new(48.0, 32.0));
  engine.step_frames(2).unwrap();
  let (red, green) = (at(16, 24, RED), at(8, 40, GREEN));
  assert_pixels(&engine.frame().unwrap(), |x, y| red(x, y).or(green(x, y)));
}

#[test]
fn a_parallax_layer_is_where_it_would_be_with_the_camera_on_the_map_s_parallax_origin() {
  // Tiled's Parallax Origin moved to (48, 40): a camera centred there shows
  // the layer unmoved, and one at (32, 32) shows it moved by half of
  // (32, 32) - (48, 40), 8 px left and 4 up
  let pan = Pan::default();
  let setup: Setup = Box::new(|ctx| {
    let set = solid(ctx, &[RED]);
    let mut tiles = map(vec![set], &[(IVec2::new(3, 3), Tile::new(0, 0))]);
    tiles.layers[0].info.parallax = Vec2::splat(0.5);
    tiles.parallax_origin = Vec2::new(48.0, 40.0);
    spawn(ctx, tiles, Vec2::ZERO);
  });
  let builder = EngineBuilder::new()
    .pixel_art(SIZE, SIZE)
    .start_scene(Panned(Some(setup), pan.clone()));
  let Some(mut engine) = HeadlessEngine::new_or_skip(builder) else {
    return;
  };
  engine.step_frames(2).unwrap();
  let at = |x: u32, y: u32| {
    move |px: u32, py: u32| {
      ((x..x + CELL).contains(&px) && (y..y + CELL).contains(&py)).then_some(RED)
    }
  };
  assert_pixels(&engine.frame().unwrap(), at(24 - 8, 24 - 4));
  // Centred on the origin: the cell at (24, 24) in the world, which the
  // camera's view (16 px right, 8 down of the first) shows at (8, 16)
  *pan.lock().unwrap() = Some(Vec2::new(48.0, 40.0));
  engine.step_frames(2).unwrap();
  assert_pixels(&engine.frame().unwrap(), at(8, 16));
}

/// A tileset image of 16 px tiles, four a row, two rows: tile 0 red, 1
/// green on its left half and clear on its right, 2 blue, 3 yellow, 4 white
fn stacked_tiles_png() -> Vec<u8> {
  let mut image = image::RgbaImage::new(64, 32);
  for (x, y, pixel) in image.enumerate_pixels_mut() {
    let tile = (y / 16) * 4 + x / 16;
    let left = x % 16 < 8;
    *pixel = image::Rgba(match tile {
      0 => RED,
      1 if left => GREEN,
      2 => BLUE,
      3 => YELLOW,
      4 => WHITE,
      _ => [0; 4],
    });
  }
  let mut bytes = Vec::new();
  image
    .write_to(
      &mut std::io::Cursor::new(&mut bytes),
      image::ImageFormat::Png,
    )
    .expect("a png");
  bytes
}

/// An engine of 64 x 48 drawing `stacked.ldtk`'s level, landed, its
/// tileset image the one `stacked_tiles_png` makes
fn stacked_level() -> Option<HeadlessEngine> {
  let files = MemorySource::new();
  let fixture = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/tilemap/ldtk/stacked.ldtk"
  );
  files.insert(
    "ldtk/stacked.ldtk",
    std::fs::read(fixture).expect("the fixture"),
  );
  files.insert("ldtk/images/tiles.png", stacked_tiles_png());
  let builder = EngineBuilder::new().pixel_art(64, 48).asset_source(files);
  engine_with(builder, |ctx| {
    let project = ctx.assets().load::<LdtkProject>("ldtk/stacked.ldtk");
    ctx
      .world
      .spawn((MapRoot::ldtk(project, "Stacked"), Transform::IDENTITY));
  })
}

/// The layer entity called `name`, and its info
fn layer_named(engine: &mut HeadlessEngine, name: &str) -> Entity {
  let world = engine.world_mut();
  let found = world
    .query::<(Entity, &MapLayerRef)>()
    .iter(world)
    .find(|(_, layer)| layer.name == name)
    .map(|(entity, _)| entity);
  found.unwrap_or_else(|| panic!("no layer {name}"))
}

#[test]
fn hiding_an_ldtk_layer_or_keeping_it_from_a_camera_takes_its_stacked_tiles_too() {
  // Ground's own tiles are a red cell under a half green one (0, 0), a
  // moved blue one (2, 0), a half yellow one (0, 2) and a white one
  // (3, 2): the green, the moved blue and the yellow are its sub-layers'.
  // Hidden, or on render layers the camera lacks, none of them draws;
  // Collision's (1, 1) still does
  let Some(mut engine) = stacked_level() else {
    return;
  };
  engine.step_frames(2).unwrap();
  let background = engine.frame().unwrap().get_pixel(60, 24).0;
  let collision_only = |frame: &image::RgbaImage| {
    for y in 0..48 {
      for x in 0..64 {
        let want = if (16..24).contains(&x) && (16..32).contains(&y) {
          GREEN
        } else if (24..32).contains(&x) && (16..32).contains(&y) {
          BLUE
        } else {
          background
        };
        assert_eq!(frame.get_pixel(x, y).0, want, "pixel ({x}, {y})");
      }
    }
  };
  let ground = layer_named(&mut engine, "Ground");
  engine
    .world_mut()
    .get_mut::<LayerInfo>(ground)
    .expect("a layer")
    .visible = false;
  engine.step_frames(2).unwrap();
  collision_only(&engine.frame().unwrap());

  engine
    .world_mut()
    .get_mut::<LayerInfo>(ground)
    .expect("a layer")
    .visible = true;
  engine
    .world_mut()
    .entity_mut(ground)
    .insert(RenderLayers::layer(3));
  engine.step_frames(2).unwrap();
  collision_only(&engine.frame().unwrap());
}

#[test]
fn a_y_sorted_ldtk_layer_s_stacked_tiles_sort_with_a_character_as_one_layer() {
  // Ground made y-sorted on its own entity, as a game sets a flag: its cell
  // (0, 0), a red tile under a half green one standing on y 16, sorts with
  // a 16 px blue character at its z by the feet, both halves alike
  let Some(mut engine) = stacked_level() else {
    return;
  };
  let ground = layer_named(&mut engine, "Ground");
  engine
    .world_mut()
    .get_mut::<LayerInfo>(ground)
    .expect("a layer")
    .y_sorted = true;
  let blue = engine
    .assets()
    .add(Image::solid_rect(16, 16, 30, 60, 230, 255));
  let character = engine
    .world_mut()
    .spawn((
      Transform::from_xy(0.0, 4.0),
      Renderable::image(blue)
        .with_anchor(Anchor::TopLeft)
        .with_z_order(1),
      YSort::new(),
    ))
    .id();
  for (feet, in_front) in [(20.0, true), (12.0, false)] {
    engine
      .world_mut()
      .get_mut::<Transform>(character)
      .expect("a transform")
      .translation = Vec2::new(0.0, feet - 16.0);
    engine.step_frames(2).unwrap();
    let frame = engine.frame().unwrap();
    let top = (feet - 16.0).max(0.0) as u32;
    for y in top..16 {
      for x in 0..16 {
        let tile = if x < 8 { GREEN } else { RED };
        let want = if in_front { BLUE } else { tile };
        assert_eq!(
          frame.get_pixel(x, y).0,
          want,
          "feet at {feet}: pixel ({x}, {y})"
        );
      }
    }
  }
}

#[test]
fn an_ldtk_level_s_stacked_offset_and_translucent_tiles_draw_as_ldtk_shows_them() {
  // `stacked.ldtk`, shaped like LDtk's saved output, over a tileset image
  // made here: a half green tile stacked on a red one (cell 0, 0), and on
  // the IntGrid a half green on a blue (cell 1, 1); a blue tile a rule
  // moved 4 px right and down (cell 2, 0); a yellow one at half alpha
  // (cell 0, 2), matched against a sprite drawn at half alpha beside it
  let files = MemorySource::new();
  let fixture = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/tilemap/ldtk/stacked.ldtk"
  );
  files.insert(
    "ldtk/stacked.ldtk",
    std::fs::read(fixture).expect("the fixture"),
  );
  files.insert("ldtk/images/tiles.png", stacked_tiles_png());
  let builder = EngineBuilder::new().pixel_art(64, 48).asset_source(files);
  let Some(mut engine) = engine_with(builder, |ctx| {
    let project = ctx.assets().load::<LdtkProject>("ldtk/stacked.ldtk");
    ctx
      .world
      .spawn((MapRoot::ldtk(project, "Stacked"), Transform::IDENTITY));
    let yellow = ctx
      .assets()
      .add(Image::solid_rect(16, 16, 240, 220, 20, 255));
    ctx.world.spawn((
      Transform::from_xy(16.0, 32.0),
      Renderable::image(yellow)
        .with_anchor(Anchor::TopLeft)
        .with_alpha(0.5),
    ));
  }) else {
    return;
  };
  engine.step_frames(2).unwrap();
  let frame = engine.frame().unwrap();
  let background = frame.get_pixel(60, 24).0;
  let half_yellow = frame.get_pixel(20, 36).0;
  assert_ne!(half_yellow, background);
  assert_ne!(half_yellow, YELLOW);
  let inside = |x: u32, y: u32, (x0, y0): (u32, u32), (w, h): (u32, u32)| {
    (x0..x0 + w).contains(&x) && (y0..y0 + h).contains(&y)
  };
  for y in 0..48 {
    for x in 0..64 {
      let want = if inside(x, y, (0, 0), (8, 16)) || inside(x, y, (16, 16), (8, 16)) {
        GREEN
      } else if inside(x, y, (8, 0), (8, 16)) {
        RED
      } else if inside(x, y, (24, 16), (8, 16)) || inside(x, y, (36, 4), (16, 16)) {
        BLUE
      } else if inside(x, y, (0, 32), (32, 16)) {
        half_yellow
      } else if inside(x, y, (48, 32), (16, 16)) {
        WHITE
      } else {
        background
      };
      assert_eq!(frame.get_pixel(x, y).0, want, "pixel ({x}, {y})");
    }
  }
}

#[test]
fn a_swapped_grid_redraws_its_layer() {
  // A new grid made the way the map's own was (one tile set, then every
  // chunk marked, as spawning does), holding another tile: the layer
  // draws the new grid
  let Some(mut engine) = engine(|ctx| {
    let set = solid(ctx, &[RED, GREEN]);
    spawn(
      ctx,
      map(vec![set], &[(IVec2::new(1, 1), Tile::new(0, 0))]),
      Vec2::ZERO,
    );
  }) else {
    return;
  };
  engine.step_frames(2).unwrap();
  let red = [(IVec2::new(1, 1), RED)];
  assert_pixels(&engine.frame().unwrap(), |x, y| in_cells(&red, x, y));

  let mut grid = TileGrid::new(UVec2::splat(SIZE / CELL));
  grid.set(IVec2::new(1, 1), Some(Tile::new(0, 1)));
  grid.mark_all_dirty();
  let world = engine.world_mut();
  let mut layers = world.query::<&mut TileLayerData>();
  for mut data in layers.iter_mut(world) {
    data.tiles = grid.clone();
  }
  engine.step().unwrap();
  let green = [(IVec2::new(1, 1), GREEN)];
  assert_pixels(&engine.frame().unwrap(), |x, y| in_cells(&green, x, y));
}

#[test]
fn an_unbounded_layer_draws_tiles_at_negative_cells_and_culls_by_signed_chunk() {
  // Red at (-40, -3), in chunk (-2, -1), and green at (40, 3), in chunk
  // (1, 0), on an unbounded layer; the camera centred on the red tile's
  // top-left corner
  let cell = CELL as f32;
  let pan = Pan::default();
  if let Ok(mut at) = pan.lock() {
    *at = Some(Vec2::new(-40.0, -3.0) * cell);
  }
  let setup: Setup = Box::new(|ctx| {
    let set = solid(ctx, &[RED, GREEN]);
    let mut tiles = map(vec![set], &[]);
    let Some(grid) = tiles.tiles_mut(0) else {
      panic!("ground is a tile layer");
    };
    *grid = TileGrid::unbounded();
    assert!(grid.set(IVec2::new(-40, -3), Some(Tile::new(0, 0))));
    assert!(grid.set(IVec2::new(40, 3), Some(Tile::new(0, 1))));
    spawn(ctx, tiles, Vec2::ZERO);
  });
  let builder = EngineBuilder::new()
    .pixel_art(SIZE, SIZE)
    .start_scene(Panned(Some(setup), pan.clone()));
  let Some(mut engine) = HeadlessEngine::new_or_skip(builder) else {
    return;
  };
  land(&mut engine);
  engine.step_frames(2).unwrap();
  // The camera's centre is the frame's: the tile fills the cell right of
  // and below it
  let centre = |colour| {
    move |x: u32, y: u32| {
      let inside = |v: u32| (SIZE / 2..SIZE / 2 + CELL).contains(&v);
      (inside(x) && inside(y)).then_some(colour)
    }
  };
  assert_pixels(&engine.frame().unwrap(), centre(RED));
  let world = engine.world_mut();
  let layers: Vec<Entity> = world
    .query_filtered::<Entity, With<TileLayerData>>()
    .iter(world)
    .collect();
  assert_eq!(layers.len(), 1);
  let built = |engine: &HeadlessEngine| -> Vec<IVec2> {
    engine
      .tile_chunks()
      .into_iter()
      .filter(|(layer, _)| *layer == layers[0])
      .map(|(_, chunk)| chunk)
      .collect()
  };
  assert_eq!(
    built(&engine),
    [IVec2::new(-2, -1)],
    "chunk (1, 0) is out of view and not built, and no chunk the layer lacks is"
  );

  // Over the green tile, its chunk is built and drawn
  if let Ok(mut at) = pan.lock() {
    *at = Some(Vec2::new(40.0, 3.0) * cell);
  }
  engine.step_frames(2).unwrap();
  assert_pixels(&engine.frame().unwrap(), centre(GREEN));
  assert_eq!(built(&engine), [IVec2::new(-2, -1), IVec2::new(1, 0)]);
}

#[test]
fn a_tile_painted_far_from_the_origin_on_an_infinite_map_draws_with_a_camera_there() {
  // An infinite map built in code, grown at run time: a tile written at
  // (5000, -5000) draws under a camera centred on it, and the origin shows
  // nothing there
  let pan = Pan::default();
  let root: Arc<Mutex<Option<Entity>>> = Arc::default();
  let kept = root.clone();
  let setup: Setup = Box::new(move |ctx| {
    let set = solid(ctx, &[RED, GREEN]);
    let mut tiles = TileMap::infinite(UVec2::splat(CELL));
    tiles.add_tileset(TilesetSource::Embedded(Arc::new(set)));
    tiles.add_tile_layer("ground");
    let at = spawn(ctx, tiles, Vec2::ZERO);
    if let Ok(mut kept) = kept.lock() {
      *kept = Some(at);
    }
  });
  let builder = EngineBuilder::new()
    .pixel_art(SIZE, SIZE)
    .start_scene(Panned(Some(setup), pan.clone()));
  let Some(mut engine) = HeadlessEngine::new_or_skip(builder) else {
    return;
  };
  land(&mut engine);
  let Some(map) = root.lock().ok().and_then(|kept| *kept) else {
    panic!("the map was not spawned");
  };
  let cell = IVec2::new(5000, -5000);
  assert!(MapsView::new(engine.world_mut()).set_tile(map, "ground", cell, Some(Tile::new(0, 1))));
  if let Ok(mut at) = pan.lock() {
    *at = Some(cell.as_vec2() * CELL as f32);
  }
  engine.step_frames(3).unwrap();
  // The camera's centre is the frame's: the tile fills the cell right of
  // and below it
  assert_pixels(&engine.frame().unwrap(), |x, y| {
    let inside = |v: u32| (SIZE / 2..SIZE / 2 + CELL).contains(&v);
    (inside(x) && inside(y)).then_some(GREEN)
  });
  // Back at the origin, where nothing was written, the frame is empty
  if let Ok(mut at) = pan.lock() {
    *at = Some(Vec2::ZERO);
  }
  engine.step_frames(2).unwrap();
  assert_pixels(&engine.frame().unwrap(), |_, _| None);
}

/// Where `character` is moved to before the next frame
type Moved = Arc<Mutex<Option<Entity>>>;

/// A 16 px blue square whose feet (its bottom edge) are at `feet`, its left
/// edge at x 16
fn character_at(feet: f32) -> Vec2 {
  Vec2::new(16.0, feet - 16.0)
}

#[test]
fn a_character_walks_behind_a_y_sorted_tree_and_in_front_of_its_roots() {
  // Trees 8 x 16 px on 8 px cells stand on their cell's bottom edge: a red
  // one on cell (2, 3) (y 16..32, roots at 32) and a yellow one on (3, 5)
  // (y 32..48, roots at 48), both in one chunk of a y-sorted layer at z 1,
  // each row its own draw. A 16 px blue character, y-sorted at z 1, draws
  // between the rows by its feet. Under them all a ground map's plain layer
  // shares their z: plain draws before y-sorted at one z
  let moved = Moved::default();
  let character = moved.clone();
  let Some(mut engine) = engine(move |ctx| {
    let ground = solid(ctx, &[GREEN]);
    let mut floor = TileMap::new(UVec2::splat(SIZE / CELL), UVec2::splat(CELL));
    floor.add_tileset(TilesetSource::Embedded(Arc::new(ground)));
    let layer = floor.add_tile_layer("ground");
    let cells: Vec<(IVec2, Tile)> = (0..7)
      .flat_map(|y| (0..7).map(move |x| (IVec2::new(x, y), Tile::new(0, 0))))
      .collect();
    put(&mut floor, layer, &cells);
    let floor = ctx.assets().add(floor);
    ctx
      .world
      .spawn((MapRoot::new(floor).with_z(1), Transform::IDENTITY));

    // Two tall trees: tiles 8 x 16 on the map's 8 px cells
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
    let planted = [
      (IVec2::new(2, 3), Tile::new(0, 0)),
      (IVec2::new(3, 5), Tile::new(0, 1)),
    ];
    put(&mut forest, layer, &planted);
    forest.layers[layer].info.y_sorted = true;
    let forest = ctx.assets().add(forest);
    ctx
      .world
      .spawn((MapRoot::new(forest).with_z(1), Transform::IDENTITY));

    let blue = ctx
      .assets()
      .add(Image::solid_rect(16, 16, 30, 60, 230, 255));
    let at = character_at(40.0);
    let entity = ctx
      .world
      .spawn((
        Transform::from_xy(at.x, at.y),
        Renderable::image(blue)
          .with_anchor(Anchor::TopLeft)
          .with_z_order(1),
        YSort::new(),
      ))
      .id();
    *character.lock().unwrap() = Some(entity);
  }) else {
    return;
  };
  engine.step_frames(2).unwrap();
  let character = moved.lock().unwrap().expect("spawned");

  // What draws where, bottom first: the ground, then the trees and the
  // character by their feet
  let expect = |feet: f32| {
    move |x: u32, y: u32| {
      let (x, y) = (x as f32, y as f32);
      let inside =
        |x0: f32, y0: f32, w: f32, h: f32| x >= x0 && x < x0 + w && y >= y0 && y < y0 + h;
      let mut drawn = [
        (32.0, RED, (16.0, 16.0, 8.0, 16.0)),
        (48.0, YELLOW, (24.0, 32.0, 8.0, 16.0)),
        (feet, BLUE, (16.0, feet - 16.0, 16.0, 16.0)),
      ];
      drawn.sort_by(|a, b| a.0.total_cmp(&b.0));
      let mut colour = (x < 56.0 && y < 56.0).then_some(GREEN);
      for (_, c, (x0, y0, w, h)) in drawn {
        if inside(x0, y0, w, h) {
          colour = Some(c);
        }
      }
      colour
    }
  };
  // Between the trees: in front of the red one's roots, behind the yellow
  assert_pixels(&engine.frame().unwrap(), expect(40.0));

  // Up behind both, then down in front of both
  for feet in [30.0, 52.0] {
    let at = character_at(feet);
    engine
      .world_mut()
      .get_mut::<Transform>(character)
      .unwrap()
      .translation = at;
    engine.step().unwrap();
    assert_pixels(&engine.frame().unwrap(), expect(feet));
  }
}

/// An image layer of `image` at `offset`
fn image_layer(
  map: &mut TileMap,
  name: &str,
  image: Handle<Image>,
  offset: Vec2,
  (repeat_x, repeat_y): (bool, bool),
) -> usize {
  let mut info = LayerInfo::new(100 + map.layers.len() as u32, name);
  info.offset = offset;
  info.collides = false;
  map.layers.push(MapLayer {
    info,
    kind: LayerKind::Image(ImageLayer {
      image: Some(image),
      repeat_x,
      repeat_y,
      transparent: None,
    }),
  });
  map.layers.len() - 1
}

/// A 16 x 8 image: red on the left, blue on the right
fn red_blue(ctx: &mut SceneContext) -> Handle<Image> {
  let data: Vec<u8> = (0..2 * CELL * CELL)
    .flat_map(|i| if i % (2 * CELL) < CELL { RED } else { BLUE })
    .collect();
  ctx.assets().add(Image::from_rgba(data, 2 * CELL, CELL))
}

#[test]
fn an_image_layer_s_transparent_colour_draws_clear() {
  // A 16 x 8 image, red with its right half magenta, keyed to magenta: the
  // right half shows what is under it
  let Some(frame) = render(|ctx| {
    let data: Vec<u8> = (0..2 * CELL * CELL)
      .flat_map(|i| {
        if i % (2 * CELL) < CELL {
          RED
        } else {
          [255, 0, 255, 255]
        }
      })
      .collect();
    let image = ctx.assets().add(Image::from_rgba(data, 2 * CELL, CELL));
    let mut layers = TileMap::new(UVec2::splat(SIZE / CELL), UVec2::splat(CELL));
    let at = image_layer(
      &mut layers,
      "keyed",
      image,
      Vec2::new(8.0, 8.0),
      (false, false),
    );
    if let LayerKind::Image(layer) = &mut layers.layers[at].kind {
      layer.transparent = Some(Color::rgb(255, 0, 255));
    }
    spawn(ctx, layers, Vec2::ZERO);
  }) else {
    return;
  };
  assert_pixels(&frame, |x, y| {
    ((8..16).contains(&x) && (8..16).contains(&y)).then_some(RED)
  });
}

#[test]
fn an_image_layer_draws_and_repeats() {
  // A 16 x 8 red and blue image: once at (8, 8); repeated across at y 24
  // from x 4; repeated down at x 40 (over the row, being above it)
  let tint = Color::rgb(220, 120, 40);
  let Some(frame) = render(move |ctx| {
    let image = red_blue(ctx);
    let mut layers = TileMap::new(UVec2::splat(SIZE / CELL), UVec2::splat(CELL));
    let once = (false, false);
    image_layer(
      &mut layers,
      "once",
      image.clone(),
      Vec2::new(8.0, 8.0),
      once,
    );
    let across = (true, false);
    image_layer(
      &mut layers,
      "across",
      image.clone(),
      Vec2::new(4.0, 24.0),
      across,
    );
    let down = (false, true);
    image_layer(&mut layers, "down", image, Vec2::new(40.0, 0.0), down);
    // Tinted and half opaque, like a sprite with the same tint and alpha
    let white = ctx
      .assets()
      .add(Image::solid_rect(CELL, CELL, 255, 255, 255, 255));
    let faded = image_layer(
      &mut layers,
      "faded",
      white.clone(),
      Vec2::new(8.0, 40.0),
      once,
    );
    layers.layers[faded].info.tint = tint;
    layers.layers[faded].info.opacity = 0.5;
    spawn(ctx, layers, Vec2::ZERO);
    ctx.world.spawn((
      Transform::from_xy(24.0, 40.0),
      Renderable::image(white)
        .with_anchor(Anchor::TopLeft)
        .with_tint(tint)
        .with_alpha(0.5),
    ));
  }) else {
    return;
  };
  let faded = frame.get_pixel(28, 44).0;
  assert_ne!(faded, WHITE);
  // Red where the image's left half falls, blue where its right half does
  let half = |x: f32, from: f32| {
    if (x - from).rem_euclid(16.0) < 8.0 {
      RED
    } else {
      BLUE
    }
  };
  assert_pixels(&frame, |x, y| {
    let (fx, fy) = (x as f32, y as f32);
    if (40..56).contains(&x) {
      return Some(half(fx, 40.0));
    }
    if (24..32).contains(&y) {
      return Some(half(fx, 4.0));
    }
    if (8..24).contains(&x) && (8..16).contains(&y) {
      return Some(half(fx, 8.0));
    }
    if (8..16).contains(&x) && (40..48).contains(&y)
      || (24..32).contains(&x) && (40..48).contains(&y)
    {
      return Some(faded);
    }
    let _ = fy;
    None
  });
}

#[test]
fn blend_modes_draw_as_their_batch_modes() {
  // Over a backdrop, column m draws mode m four ways: a tile layer (y 8), an
  // image layer (y 24), a tile object (y 40), and a sprite (y 56), which the
  // three must match
  let modes = [
    BlendMode::Alpha,
    BlendMode::Additive,
    BlendMode::Multiply,
    BlendMode::Screen,
  ];
  const X: [u8; 4] = [200, 100, 50, 255];
  let Some(frame) = render(move |ctx| {
    let backdrop = ctx
      .assets()
      .add(Image::solid_rect(SIZE, SIZE, 60, 120, 180, 255));
    ctx.world.spawn((
      Transform::IDENTITY,
      Renderable::image(backdrop)
        .with_anchor(Anchor::TopLeft)
        .with_z_order(-10),
    ));
    let set = solid(ctx, &[X]);
    let picture = ctx
      .assets()
      .add(Image::solid_rect(CELL, CELL, X[0], X[1], X[2], X[3]));
    let mut layers = map(vec![set], &[]);
    for (m, mode) in modes.into_iter().enumerate() {
      let x = (2 * m) as i32;
      let tiles = layers.add_tile_layer("tiles");
      put(&mut layers, tiles, &[(IVec2::new(x, 1), Tile::new(0, 0))]);
      layers.layers[tiles].info.blend = mode;
      let once = (false, false);
      let at = Vec2::new(16.0 * m as f32, 24.0);
      let image = image_layer(&mut layers, "image", picture.clone(), at, once);
      layers.layers[image].info.blend = mode;
      let objects = layers.add_object_layer("objects");
      layers.layers[objects].kind = LayerKind::Objects(vec![tile_object(
        "1",
        Vec2::new(16.0 * m as f32, 40.0),
        Vec2::splat(CELL as f32),
        Tile::new(0, 0),
      )]);
      layers.layers[objects].info.blend = mode;
      ctx.world.spawn((
        Transform::from_xy(16.0 * m as f32, 56.0),
        Renderable::image(picture.clone())
          .with_anchor(Anchor::TopLeft)
          .with_blend(mode),
      ));
    }
    spawn(ctx, layers, Vec2::ZERO);
  }) else {
    return;
  };
  let mut seen = Vec::new();
  for m in 0..modes.len() as u32 {
    let x = 16 * m + 4;
    let sprite = frame.get_pixel(x, 60).0;
    for (y, what) in [(12, "tile layer"), (28, "image layer"), (44, "tile object")] {
      assert_eq!(
        frame.get_pixel(x, y).0,
        sprite,
        "{what} in {:?}",
        modes[m as usize]
      );
    }
    assert!(!seen.contains(&sprite), "the modes differ");
    seen.push(sprite);
  }
}

/// A tile object showing `tile`, its top-left at `at`, `size` px
fn tile_object(id: &str, at: Vec2, size: Vec2, tile: Tile) -> MapObjectData {
  MapObjectData {
    id: id.to_string(),
    name: String::new(),
    class: None,
    position: at,
    size,
    rotation: 0.0,
    shape: ObjectShape::Rect,
    tile: Some(tile),
    visible: true,
    properties: Properties::default(),
  }
}

#[test]
fn tile_objects_draw_their_tile_and_hidden_ones_do_not() {
  // The quartered tile as objects: twice its size at (8, 8); mirrored at
  // (32, 8). Hidden ones, one on a hidden layer and a shape with no tile
  // draw nothing; a white one on a tinted, half-opaque layer at (8, 40)
  // matches a sprite tinted and faded alike at (24, 40)
  let tint = Color::rgb(220, 120, 40);
  let spawned = Moved::default();
  let map_entity = spawned.clone();
  let Some(mut engine) = engine(move |ctx| {
    let quartered = tileset(
      ctx,
      2,
      |id, x, y| if id == 0 { quarters(x, y) } else { WHITE },
    );
    let mut layers = map(vec![quartered], &[]);
    let objects = layers.add_object_layer("things");
    let mut hidden = tile_object("3", Vec2::new(8.0, 32.0), Vec2::splat(8.0), Tile::new(0, 0));
    hidden.visible = false;
    let mut shape = tile_object(
      "4",
      Vec2::new(40.0, 32.0),
      Vec2::splat(8.0),
      Tile::new(0, 0),
    );
    shape.tile = None;
    layers.layers[objects].kind = LayerKind::Objects(vec![
      tile_object("1", Vec2::new(8.0, 8.0), Vec2::splat(16.0), Tile::new(0, 0)),
      tile_object(
        "2",
        Vec2::new(32.0, 8.0),
        Vec2::splat(8.0),
        Tile::new(0, 0).with_flip(TileFlip::HORIZONTAL),
      ),
      hidden,
      shape,
    ]);
    let off = layers.add_object_layer("off");
    layers.layers[off].kind = LayerKind::Objects(vec![tile_object(
      "5",
      Vec2::new(48.0, 48.0),
      Vec2::splat(8.0),
      Tile::new(0, 0),
    )]);
    layers.layers[off].info.visible = false;
    let faded = layers.add_object_layer("faded");
    layers.layers[faded].kind = LayerKind::Objects(vec![tile_object(
      "6",
      Vec2::new(8.0, 40.0),
      Vec2::splat(8.0),
      Tile::new(0, 1),
    )]);
    layers.layers[faded].info.tint = tint;
    layers.layers[faded].info.opacity = 0.5;
    let map = spawn(ctx, layers, Vec2::ZERO);
    *map_entity.lock().unwrap() = Some(map);
    let white = ctx
      .assets()
      .add(Image::solid_rect(CELL, CELL, 255, 255, 255, 255));
    ctx.world.spawn((
      Transform::from_xy(24.0, 40.0),
      Renderable::image(white)
        .with_anchor(Anchor::TopLeft)
        .with_tint(tint)
        .with_alpha(0.5),
    ));
  }) else {
    return;
  };
  engine.step_frames(2).unwrap();
  let frame = engine.frame().unwrap();
  let faded = frame.get_pixel(28, 44).0;
  assert_ne!(faded, WHITE);
  let expect = |faded_tile: bool| {
    move |x: u32, y: u32| {
      if (8..24).contains(&x) && (8..24).contains(&y) {
        return Some(quarters((x - 8) / 2, (y - 8) / 2));
      }
      if (32..40).contains(&x) && (8..16).contains(&y) {
        return Some(quarters(CELL - 1 - (x - 32), y - 8));
      }
      if (24..32).contains(&x) && (40..48).contains(&y) {
        return Some(faded);
      }
      ((8..16).contains(&x) && (40..48).contains(&y) && faded_tile).then_some(faded)
    }
  };
  assert_pixels(&frame, expect(true));

  // A disabled map hides its objects too
  let map = spawned.lock().unwrap().expect("spawned");
  engine.world_mut().entity_mut(map).insert(Disabled);
  engine.step().unwrap();
  let only_sprite =
    |x: u32, y: u32| ((24..32).contains(&x) && (40..48).contains(&y)).then_some(faded);
  assert_pixels(&engine.frame().unwrap(), only_sprite);
}

#[test]
fn tile_objects_on_a_y_sorted_layer_sort_with_characters_by_their_feet() {
  // A red tile object 8 x 16 px with its feet at y 32, on a y-sorted object
  // layer at z 1; a blue character, y-sorted at z 1, overlaps it with its
  // feet at y 28 (behind it), then at y 36 (in front)
  let moved = Moved::default();
  let character = moved.clone();
  let Some(mut engine) = engine(move |ctx| {
    let image = ctx.assets().add(Image::solid_rect(
      CELL,
      2 * CELL,
      RED[0],
      RED[1],
      RED[2],
      255,
    ));
    let size = UVec2::new(CELL, 2 * CELL);
    let mut set = Tileset::new(
      "tall",
      UVec2::new(CELL, 2 * CELL),
      TilesetImage::Atlas { image, size },
    );
    set.columns = 1;
    set.tile_count = 1;
    let mut layers = map(vec![set], &[]);
    let objects = layers.add_object_layer("things");
    let tree = tile_object(
      "1",
      Vec2::new(16.0, 16.0),
      Vec2::new(8.0, 16.0),
      Tile::new(0, 0),
    );
    layers.layers[objects].kind = LayerKind::Objects(vec![tree]);
    layers.layers[objects].info.y_sorted = true;
    let handle = ctx.assets().add(layers);
    ctx.world.spawn((MapRoot::new(handle), Transform::IDENTITY));
    let blue = ctx.assets().add(Image::solid_rect(16, 8, 30, 60, 230, 255));
    let entity = ctx
      .world
      .spawn((
        Transform::from_xy(12.0, 20.0),
        Renderable::image(blue)
          .with_anchor(Anchor::TopLeft)
          .with_z_order(1),
        YSort::new(),
      ))
      .id();
    *character.lock().unwrap() = Some(entity);
  }) else {
    return;
  };
  engine.step_frames(2).unwrap();
  let character = moved.lock().unwrap().expect("spawned");
  let expect = |feet: u32| {
    move |x: u32, y: u32| {
      let tree = (16..24).contains(&x) && (16..32).contains(&y);
      let blue = (12..28).contains(&x) && (feet - 8..feet).contains(&y);
      match (tree, blue) {
        (true, true) => Some(if feet < 32 { RED } else { BLUE }),
        (true, false) => Some(RED),
        (false, true) => Some(BLUE),
        (false, false) => None,
      }
    }
  };
  assert_pixels(&engine.frame().unwrap(), expect(28));
  engine
    .world_mut()
    .get_mut::<Transform>(character)
    .unwrap()
    .translation = Vec2::new(12.0, 28.0);
  engine.step().unwrap();
  assert_pixels(&engine.frame().unwrap(), expect(36));
}

#[test]
fn a_small_image_repeated_both_ways_covers_a_1080p_view() {
  // An 8 px image (red on its left half, blue on its right) repeated across
  // and down from (3, 5) covers every pixel of a 1920 x 1080 view: some
  // 32,000 copies
  let (width, height) = (1920, 1080);
  let builder = EngineBuilder::new().pixel_art(width, height);
  let Some(mut engine) = engine_with(builder, |ctx| {
    let data: Vec<u8> = (0..CELL * CELL)
      .flat_map(|i| if i % CELL < CELL / 2 { RED } else { BLUE })
      .collect();
    let image = ctx.assets().add(Image::from_rgba(data, CELL, CELL));
    let mut layers = TileMap::new(UVec2::ONE, UVec2::splat(CELL));
    image_layer(&mut layers, "sky", image, Vec2::new(3.0, 5.0), (true, true));
    spawn(ctx, layers, Vec2::ZERO);
  }) else {
    return;
  };
  engine.step_frames(2).unwrap();
  let frame = engine.frame().unwrap();
  let mut wrong = 0;
  let mut first = None;
  for (x, y, p) in frame.enumerate_pixels() {
    let want = if (x + CELL - 3) % CELL < CELL / 2 {
      RED
    } else {
      BLUE
    };
    if p.0 != want {
      wrong += 1;
      first.get_or_insert((x, y, p.0));
    }
  }
  assert_eq!(wrong, 0, "pixels not the image's; the first {first:?}");
}

// ---- every grid: Tiled's renders ---------------------------------------------

/// Tiled's fixtures: maps of every grid and Tiled's renders of them
const TILED: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/tilemap/tiled");

/// The colours of the shape fixtures' tiles
const SHAPE_COLOURS: [[u8; 4]; 5] = [
  [255, 0, 0, 255],
  [0, 255, 0, 255],
  [0, 0, 255, 255],
  [255, 255, 0, 255],
  [255, 255, 255, 255],
];

/// Tiled's render of `maps/{name}.tmj` (`toImage()`, every alpha 0 or 255)
fn tiled_render(name: &str) -> image::RgbaImage {
  let path = format!("{TILED}/renders/{name}.png");
  let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
  match image::load_from_memory(&bytes) {
    Ok(image) => image.to_rgba8(),
    Err(e) => panic!("{path}: {e}"),
  }
}

/// An engine drawing `maps/{name}.tmj`, read from Tiled's fixtures, as pixel
/// art at zoom 1 on a frame of `size`, landed, with the map's grid and the
/// box its cells cover
fn tiled_engine(name: &str, size: (u32, u32)) -> Option<(HeadlessEngine, MapGrid, Rect)> {
  let held: Arc<Mutex<Option<Handle<TileMap>>>> = Arc::default();
  let kept = held.clone();
  let path = format!("maps/{name}.tmj");
  let builder = EngineBuilder::new()
    .pixel_art(size.0, size.1)
    .asset_source(DiskSource::new(TILED));
  let engine = engine_with(builder, move |ctx| {
    let map = ctx.assets().load::<TileMap>(&path);
    ctx
      .world
      .spawn((MapRoot::new(map.clone()), Transform::IDENTITY));
    if let Ok(mut kept) = kept.lock() {
      *kept = Some(map);
    }
  })?;
  let Some(handle) = held.lock().ok().and_then(|held| held.clone()) else {
    panic!("{name}: the map was not loaded");
  };
  let (grid, bounds) = {
    let maps = engine.world().resource::<Assets<TileMap>>();
    let Some(map) = maps.get(&handle) else {
      panic!("{name}: the map is not loaded");
    };
    let grid = map.grid();
    (grid, grid.bounds_of(map.bounds()))
  };
  Some((engine, grid, bounds))
}

/// The engine's frame of `maps/{name}.tmj` the size of Tiled's render of it,
/// its camera on where the render starts (the top-left of the box the map's
/// cells cover: a negative skew's content left of or above the origin
/// included), and the map's grid
fn as_tiled_draws(name: &str, render: &image::RgbaImage) -> Option<(image::RgbaImage, MapGrid)> {
  let size = (render.width(), render.height());
  let (mut engine, grid, bounds) = tiled_engine(name, size)?;
  let centre = Vec2::new(bounds.x, bounds.y) + Vec2::new(size.0 as f32, size.1 as f32) / 2.0;
  if let Some(mut camera) = CameraMut::main(engine.world_mut()) {
    camera.set_position(centre.x, centre.y);
  }
  // More than once: the main camera is interpolated, and draws where it was
  engine.step_frames(3).unwrap();
  Some((engine.frame().unwrap(), grid))
}

/// The pixels where `frame` differs from Tiled's `render`: where Tiled drew
/// a tile, its colour; where it drew nothing, the background (the frame's
/// pixel at the first place Tiled left clear)
fn unlike_tiled(frame: &image::RgbaImage, render: &image::RgbaImage) -> Vec<String> {
  let Some((x0, y0, _)) = render.enumerate_pixels().find(|(_, _, p)| p.0[3] == 0) else {
    panic!("Tiled's render covers every pixel");
  };
  let background = frame.get_pixel(x0, y0).0;
  assert!(
    !SHAPE_COLOURS.contains(&background),
    "the background {background:?} is a tile colour"
  );
  render
    .enumerate_pixels()
    .filter_map(|(x, y, tiled)| {
      let want = match tiled.0[3] {
        255 => tiled.0,
        0 => background,
        alpha => panic!("Tiled's render has alpha {alpha} at ({x}, {y})"),
      };
      let got = frame.get_pixel(x, y).0;
      (got != want).then(|| format!("({x}, {y}): {got:?}, Tiled {want:?}"))
    })
    .collect()
}

/// Every pixel of `name`'s frame is as Tiled draws it
fn assert_draws_as_tiled(name: &str) {
  let render = tiled_render(name);
  let Some((frame, _)) = as_tiled_draws(name, &render) else {
    return;
  };
  let unlike = unlike_tiled(&frame, &render);
  if !unlike.is_empty() {
    let path = std::env::temp_dir().join(format!("ryo2d-{name}.png"));
    let _ = frame.save(&path);
    panic!(
      "{name}: {} pixels differ from Tiled's render (the frame is at {}), the first: {:#?}",
      unlike.len(),
      path.display(),
      &unlike[..unlike.len().min(8)]
    );
  }
}

#[test]
fn every_grid_draws_its_tiles_as_tileds_render_shows() {
  // Each shape fixture: the engine's frame of the map (pixel art, zoom 1,
  // the camera on the map's box) is Tiled's render, pixel for pixel. The
  // tiles are shaped like their cells (rectangles, diamonds, hexagons) and
  // are opaque or clear with no smoothed edge, and the maps leave their
  // outer ring empty so no tile overhangs the render's edge (Tiled's
  // `toImage` clips there). Odd sizes overlap their neighbours by a pixel
  // (staggered and hexagonal grids step in whole pixels, isometric ones on
  // half pixels, which Tiled's raster rounds)
  for name in [
    "shape-iso",
    "shape-iso-48x32",
    "shape-stag-x-odd",
    "shape-stag-x-even",
    "shape-stag-y-odd",
    "shape-stag-y-even",
    "shape-hex-x-odd",
    "shape-hex-x-even",
    "shape-hex-y-odd",
    "shape-hex-y-even",
    "shape-hex-x-side0",
    "shape-hex-y-side0",
    "shape-hex-long-side",
    "shape-oblique",
    "shape-oblique-neg-x",
    "shape-oblique-neg-y",
    "shape-odd-ortho",
    "shape-odd-iso",
    "shape-odd-stag-x-odd",
    "shape-odd-stag-y-even",
    "shape-odd-hex-x-odd",
    "shape-odd-hex-y-odd",
    "shape-odd-oblique",
  ] {
    assert_draws_as_tiled(name);
  }
}

#[test]
fn hex_rotations_and_flips_draw_as_tiled_shows_them() {
  // The hex rotation fixture: a marker hexagon (56 x 64, side 32), white
  // with its top-left quarter red, at cell (1 + i mod 4, 1 + i / 4) with
  // flags i (1 flips it across, 2 down, 4 turns it 60 degrees clockwise, 8
  // 120). Tiled flips first, then turns about the middle of the cell's box.
  // A turned picture is resampled, so its edges are not compared: at the
  // middle of each quarter of each marker, Tiled shows red at one and white
  // at three, and the engine shows what Tiled does
  let name = "shape-hex-rotations";
  let render = tiled_render(name);
  let Some((frame, grid)) = as_tiled_draws(name, &render) else {
    return;
  };
  let (red, white) = (SHAPE_COLOURS[0], SHAPE_COLOURS[4]);
  let mut unlike = Vec::new();
  for i in 0..16u32 {
    let cell = IVec2::new(1 + (i % 4) as i32, 1 + (i / 4) as i32);
    let centre = grid.cell_center(cell);
    let turn = (60.0 * f32::from(u8::from(i & 4 != 0)) + 120.0 * f32::from(u8::from(i & 8 != 0)))
      .to_radians();
    let mut reds = 0;
    for quarter in [(-14.0, -16.0), (14.0, -16.0), (-14.0, 16.0), (14.0, 16.0)] {
      // The quarter's middle in the picture, from its centre: flipped,
      // then turned clockwise (y is down)
      let mut at = Vec2::from(quarter);
      if i & 1 != 0 {
        at.x = -at.x;
      }
      if i & 2 != 0 {
        at.y = -at.y;
      }
      let at = Vec2::from_angle(turn).rotate(at);
      let (x, y) = ((centre.x + at.x) as u32, (centre.y + at.y) as u32);
      let tiled = render.get_pixel(x, y).0;
      assert!(
        tiled == red || tiled == white,
        "flags {i}: Tiled's render at ({x}, {y}) is {tiled:?}, no quarter of the marker"
      );
      reds += usize::from(tiled == red);
      let got = frame.get_pixel(x, y).0;
      if got != tiled {
        unlike.push(format!("flags {i}, ({x}, {y}): {got:?}, Tiled {tiled:?}"));
      }
    }
    assert_eq!(reds, 1, "flags {i}: one quarter is red in Tiled's render");
  }
  assert!(unlike.is_empty(), "unlike Tiled: {unlike:#?}");
}

/// The box of the pixels in `frame` within `size` of `centre` that are not
/// `clear` (or, with `clear` None, not transparent), as its middle and its
/// size
fn drawn_box(
  frame: &image::RgbaImage,
  (centre, size): (Vec2, i32),
  clear: Option<[u8; 4]>,
) -> (Vec2, Vec2) {
  let (mut low, mut high) = (IVec2::MAX, IVec2::MIN);
  let c = centre.floor().as_ivec2();
  for y in c.y - size..c.y + size {
    for x in c.x - size..c.x + size {
      let p = frame.get_pixel(x as u32, y as u32).0;
      let drawn = match clear {
        Some(clear) => p != clear,
        None => p[3] > 0,
      };
      if drawn {
        low = low.min(IVec2::new(x, y));
        high = high.max(IVec2::new(x + 1, y + 1));
      }
    }
  }
  assert!(low.x < high.x, "nothing drawn about {centre}");
  let (low, high) = (low.as_vec2(), high.as_vec2());
  ((low + high) / 2.0, high - low)
}

#[test]
fn hex_tiles_turn_about_their_own_middle_as_tiled_shows() {
  // Turned markers (a white rectangle, its top-left quarter red, flags i at
  // cell (2 + 4 (i mod 4), 2 + 4 (i / 4))) that are not their box's size:
  // 48 x 40 on 32 x 32 hexagons of side 16; their box's size with the
  // tileset offset by (6, -4); on 33 x 33 hexagons of side 15, whose box is
  // 32 across, the cell's size, and 45 x 41 offset by (5, -3). Tiled turns
  // each about its own middle where it stands unturned, offset and all
  // (measured). Each marker's drawn box has the middle Tiled's has, and at
  // the middle of each quarter the engine shows what Tiled does: red at
  // one, white at three
  let (red, white) = (SHAPE_COLOURS[0], SHAPE_COLOURS[4]);
  let maps = [
    ("shape-hex-turns-large", Vec2::new(48.0, 40.0)),
    ("shape-hex-turns-offset", Vec2::new(32.0, 32.0)),
    ("shape-hex-turns-odd", Vec2::new(33.0, 33.0)),
    ("shape-hex-turns-odd-offset", Vec2::new(45.0, 41.0)),
  ];
  let mut unlike = Vec::new();
  for (name, marker) in maps {
    let render = tiled_render(name);
    let Some((frame, grid)) = as_tiled_draws(name, &render) else {
      return;
    };
    let Some((x0, y0, _)) = render.enumerate_pixels().find(|(_, _, p)| p.0[3] == 0) else {
      panic!("{name}: Tiled's render covers every pixel");
    };
    let background = frame.get_pixel(x0, y0).0;
    for i in 0..16u32 {
      let cell = IVec2::new(2 + 4 * (i % 4) as i32, 2 + 4 * (i / 4) as i32);
      // Markers are 128 px apart across and 96 down
      let near = (grid.cell_center(cell), 48);
      let (tiled_middle, tiled_size) = drawn_box(&render, near, None);
      let (middle, size) = drawn_box(&frame, near, Some(background));
      if (middle - tiled_middle).abs().max_element() > 0.5
        || (size - tiled_size).abs().max_element() > 2.0
      {
        unlike.push(format!(
          "{name}, flags {i}: drawn about {middle} ({size}), Tiled {tiled_middle} ({tiled_size})"
        ));
      }
      let turn = (60.0 * f32::from(u8::from(i & 4 != 0)) + 120.0 * f32::from(u8::from(i & 8 != 0)))
        .to_radians();
      let mut reds = 0;
      for (sx, sy) in [(-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)] {
        // The quarter's middle in the picture, from its middle: flipped,
        // then turned clockwise (y is down)
        let mut at = Vec2::new(sx, sy) * marker / 4.0;
        if i & 1 != 0 {
          at.x = -at.x;
        }
        if i & 2 != 0 {
          at.y = -at.y;
        }
        let at = tiled_middle + Vec2::from_angle(turn).rotate(at);
        let (x, y) = (at.x as u32, at.y as u32);
        let tiled = render.get_pixel(x, y).0;
        assert!(
          tiled == red || tiled == white,
          "{name}, flags {i}: Tiled's render at ({x}, {y}) is {tiled:?}, no quarter of the marker"
        );
        reds += usize::from(tiled == red);
        let got = frame.get_pixel(x, y).0;
        if got != tiled {
          unlike.push(format!(
            "{name}, flags {i}, ({x}, {y}): {got:?}, Tiled {tiled:?}"
          ));
        }
      }
      assert_eq!(
        reds, 1,
        "{name}, flags {i}: one quarter is red in Tiled's render"
      );
    }
  }
  assert!(unlike.is_empty(), "unlike Tiled: {unlike:#?}");
}

#[test]
fn tall_tiles_on_an_odd_isometric_grid_draw_where_tileds_raster_does() {
  // Tiles of 33 x 40, 33 x 41, 32 x 40 and 32 x 41 on 33 x 17 isometric
  // cells, each at a whole and a half anchor: Tiled's raster rounds an x
  // on a half pixel up, puts tiles of tilesets wider or taller than the
  // cell's width a pixel further right where x is whole, and puts a tile a
  // pixel lower where its bottom is whole (the cell's height being odd),
  // whatever its own height. Pixel art draws them all where Tiled does
  assert_draws_as_tiled("shape-tall-odd-iso");
}

#[test]
fn tall_tiles_across_a_chunk_border_draw_in_tileds_order() {
  // Plain layers of tiles larger than their cells, from two tilesets,
  // straddling the engine's chunk border at x 32 (y 32 on the orthogonal
  // map), each overlapping its neighbours. Tiled draws them in its order
  // whatever their tileset or chunk: on the isometric map, diagonal by
  // diagonal, so of 24 x 32 tiles in 16 x 8 cells (30, 1) of one tileset
  // over (29, 2) of the other, across the cells' sides; of 16 x 32 tiles,
  // reaching only above their cells, (30, 2) over (30, 1); on the
  // staggered map's rows, a shifted row's last cell before the cell of the
  // next chunk below it; on the orthogonal map, the rows above the border
  // before those below
  for name in [
    "shape-tall-chunk-border",
    "shape-tall-iso-upright",
    "shape-tall-stag-upright",
    "shape-tall-ortho-upright",
  ] {
    assert_draws_as_tiled(name);
  }
}

// ---- every grid: how chunks draw and cull -------------------------------------

/// A map of `shape` whose cells are `cell` px, `size` of them a side, every
/// cell holding a tile its cell's box's size from one of two tilesets
/// (red and green, blue and yellow), as `tileset(cell)` says
fn two_tileset_map(
  ctx: &mut SceneContext,
  (shape, cell, size): (GridShape, UVec2, u32),
  tileset: impl Fn(IVec2) -> u16,
) -> TileMap {
  let tile = MapGrid::new(shape, cell).cell_bounds(IVec2::ZERO);
  let tile = UVec2::new(tile.width as u32, tile.height as u32);
  let mut map = TileMap::new(UVec2::splat(size), cell).with_shape(shape);
  for colours in [[RED, GREEN], [BLUE, YELLOW]] {
    let (width, height) = (tile.x * 2, tile.y);
    let data: Vec<u8> = (0..width * height)
      .flat_map(|i| colours[((i % width) / tile.x) as usize])
      .collect();
    let image = ctx.assets().add(Image::from_rgba(data, width, height));
    let mut set = Tileset::new(
      "solid",
      tile,
      TilesetImage::Atlas {
        image,
        size: UVec2::new(width, height),
      },
    );
    set.columns = 2;
    set.tile_count = 2;
    map.add_tileset(TilesetSource::Embedded(Arc::new(set)));
  }
  let layer = map.add_tile_layer("ground");
  let Some(grid) = map.tiles_mut(layer) else {
    panic!("ground is a tile layer");
  };
  for y in 0..size as i32 {
    for x in 0..size as i32 {
      let cell = IVec2::new(x, y);
      grid.set(cell, Some(Tile::new(tileset(cell), (x % 2) as u32)));
    }
  }
  map
}

#[test]
fn a_layer_whose_tiles_fit_their_cells_keeps_its_per_chunk_draws() {
  // Tiles the size of their cell's box, from two tilesets, on an
  // orthogonal, an isometric and a hexagonal map of 3 x 3 chunks, seen
  // whole: nothing leaves its cell, so each chunk draws as one draw per
  // tileset in it, all of the tileset's tiles there, chunk by chunk
  let hex = GridShape::Hexagonal {
    axis: StaggerAxis::Y,
    index: StaggerIndex::Odd,
    side: 8,
  };
  let grids = [
    (GridShape::Orthogonal, UVec2::splat(8)),
    (GridShape::Isometric, UVec2::new(16, 8)),
    (hex, UVec2::splat(16)),
  ];
  // Bands of the two tilesets, and a last column of chunks of the first
  // alone
  let tileset = |cell: IVec2| u16::from(cell.x < 64 && (cell.x / 7 + cell.y / 5) % 2 == 1);
  for (shape, cell) in grids {
    let pan = Pan::default();
    let setup: Setup = Box::new(move |ctx| {
      let map = two_tileset_map(ctx, (shape, cell, 96), tileset);
      spawn(ctx, map, Vec2::ZERO);
    });
    let builder = EngineBuilder::new()
      .pixel_art(SIZE, SIZE)
      .start_scene(Panned(Some(setup), pan.clone()));
    let Some(mut engine) = HeadlessEngine::new_or_skip(builder) else {
      return;
    };
    land(&mut engine);
    let grid = MapGrid::for_map(shape, cell, UVec2::splat(96), false);
    let whole = grid.bounds_of(CellRect::from_size(UVec2::splat(96)));
    if let Ok(mut at) = pan.lock() {
      *at = Some(Vec2::new(
        whole.x + whole.width / 2.0,
        whole.y + whole.height / 2.0,
      ));
    }
    for (_, mut camera) in engine
      .world_mut()
      .query::<(&MainCamera, &mut Camera2d)>()
      .iter_mut(engine.world_mut())
    {
      camera.zoom = SIZE as f32 / whole.width.max(whole.height) / 1.25;
    }
    engine.step_frames(3).unwrap();
    let draws = engine.tile_draws();
    let mut chunks: Vec<IVec2> = draws.iter().map(|d| d.1).collect();
    chunks.dedup();
    assert_eq!(
      chunks.len(),
      9,
      "{shape:?}: every chunk draws, chunk by chunk"
    );
    for chunk in chunks {
      let mut counts = [0u32; 2];
      for y in 0..32 {
        for x in 0..32 {
          counts[usize::from(tileset(chunk * 32 + IVec2::new(x, y)))] += 1;
        }
      }
      let want: Vec<(u16, u32, u32)> = (0..2u16)
        .filter(|&set| counts[usize::from(set)] > 0)
        .map(|set| (set, 0, counts[usize::from(set)]))
        .collect();
      let got: Vec<(u16, u32, u32)> = draws
        .iter()
        .filter(|d| d.1 == chunk)
        .map(|d| (d.2, d.3, d.4))
        .collect();
      assert_eq!(
        got, want,
        "{shape:?}, chunk {chunk}: a draw per tileset, whole"
      );
    }
  }
}

#[test]
fn culling_draws_only_chunks_a_camera_sees_on_every_grid() {
  // A 64 px camera over an isometric and a hexagonal map of 3 x 3 chunks:
  // every chunk holding a cell whose box meets its view is built and drawn,
  // and no chunk whose cells' box lies further than 64 px from it is (a
  // view's chunks are those holding cells its box widened by how far a tile
  // may draw past its cell meets). At (144, 288), by the isometric map's
  // top-left edge, the corners of the view are in the cells of chunks (0, 1)
  // and (0, 2), but the view sees only (0, 2): (0, 1)'s cells lie more than
  // 64 px to its right
  let hex = GridShape::Hexagonal {
    axis: StaggerAxis::Y,
    index: StaggerIndex::Odd,
    side: 8,
  };
  let cases = [
    (
      GridShape::Isometric,
      UVec2::new(16, 8),
      [
        Vec2::new(144.0, 288.0),
        Vec2::new(768.0, 300.0),
        Vec2::new(1200.0, 400.0),
      ],
    ),
    (
      hex,
      UVec2::splat(16),
      [
        Vec2::new(300.0, 900.0),
        Vec2::new(515.0, 500.0),
        Vec2::new(1300.0, 100.0),
      ],
    ),
  ];
  for (shape, cell, places) in cases {
    let pan = Pan::default();
    let setup: Setup = Box::new(move |ctx| {
      let map = two_tileset_map(ctx, (shape, cell, 96), |cell| (cell.x % 2) as u16);
      spawn(ctx, map, Vec2::ZERO);
    });
    let builder = EngineBuilder::new()
      .pixel_art(SIZE, SIZE)
      .start_scene(Panned(Some(setup), pan.clone()));
    let Some(mut engine) = HeadlessEngine::new_or_skip(builder) else {
      return;
    };
    land(&mut engine);
    let grid = MapGrid::for_map(shape, cell, UVec2::splat(96), false);
    let half = SIZE as f32 / 2.0;
    let meets = |b: Rect, low: Vec2, high: Vec2| {
      b.x <= high.x && b.x + b.width >= low.x && b.y <= high.y && b.y + b.height >= low.y
    };
    for at in places {
      let (low, high) = (at - half, at + half);
      // The chunks holding a cell whose box meets the view
      let seen: Vec<IVec2> = (0..3)
        .flat_map(|y| (0..3).map(move |x| IVec2::new(x, y)))
        .filter(|&chunk| {
          (0..32 * 32).any(|i| {
            let cell = chunk * 32 + IVec2::new(i % 32, i / 32);
            meets(grid.cell_bounds(cell), low, high)
          })
        })
        .collect();
      // The chunks whose cells' box comes within 64 px of it
      let near: Vec<IVec2> = (0..3)
        .flat_map(|y| (0..3).map(move |x| IVec2::new(x, y)))
        .filter(|&chunk| {
          let b = grid.bounds_of(CellRect::new(chunk * 32, chunk * 32 + 32));
          meets(b, low - 64.0, high + 64.0)
        })
        .collect();
      // The store keeps chunks drawn in the last 120 frames: a fresh engine
      // for each place would be slow, so each place is looked at after the
      // last place's chunks have gone
      if let Ok(mut pan) = pan.lock() {
        *pan = Some(at);
      }
      engine.step_frames(130).unwrap();
      let built: Vec<IVec2> = engine.tile_chunks().into_iter().map(|(_, c)| c).collect();
      let mut drawn: Vec<IVec2> = engine.tile_draws().into_iter().map(|d| d.1).collect();
      drawn.sort_by_key(|c| (c.y, c.x));
      drawn.dedup();
      assert!(
        !seen.is_empty(),
        "{shape:?} at {at}: the view is over the map"
      );
      for chunk in &seen {
        assert!(
          built.contains(chunk),
          "{shape:?} at {at}: chunk {chunk} is seen, built {built:?}"
        );
        assert!(
          drawn.contains(chunk),
          "{shape:?} at {at}: chunk {chunk} is seen, drawn {drawn:?}"
        );
      }
      for chunk in &built {
        assert!(
          near.contains(chunk),
          "{shape:?} at {at}: chunk {chunk} is out of view, built {built:?}"
        );
      }
    }
    // The one the view's corners name but the view does not see
    if shape == GridShape::Isometric {
      if let Ok(mut pan) = pan.lock() {
        *pan = Some(Vec2::new(144.0, 288.0));
      }
      engine.step_frames(130).unwrap();
      let built: Vec<IVec2> = engine.tile_chunks().into_iter().map(|(_, c)| c).collect();
      assert_eq!(built, [IVec2::new(0, 2)], "{shape:?}");
    }
  }
}
