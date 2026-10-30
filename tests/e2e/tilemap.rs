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

fn engine_with(
  builder: EngineBuilder,
  setup: impl FnMut(&mut SceneContext) + Send + 'static,
) -> Option<HeadlessEngine> {
  HeadlessEngine::new_or_skip(builder.start_scene(Fixture(Some(Box::new(setup)))))
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
