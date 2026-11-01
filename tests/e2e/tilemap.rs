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
    data.grid = grid.clone();
  }
  engine.step().unwrap();
  let green = [(IVec2::new(1, 1), GREEN)];
  assert_pixels(&engine.frame().unwrap(), |x, y| in_cells(&green, x, y));
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
