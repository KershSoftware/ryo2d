//! Tiles drawn edge to edge never show a seam: at any zoom, at any sub-pixel
//! camera position and with either filter, every pixel over a filled tile
//! layer is one of its tiles' colours, never the background, a mix of two
//! tiles, or a colour from a tile next to them in the tileset images. And on
//! the pixel-art target a layer's origin snaps to a whole pixel, so a camera
//! a fraction of a pixel off draws what it draws there.
//!
//! Tiles turned, flipped, animated, y-sorted, squashed or drawn as tile
//! objects, and tiles seen through a turned camera, never show a neighbour's
//! colour either, and a tile zoomed out a little keeps its outer texels.
//! A layer's filter picks how its tiles and tile objects are sampled
//! whatever the target: linear on the pixel-art target reads the smaller
//! copies, nearest on the smooth one keeps the full-size texels.

use std::sync::Arc;

use super::HeadlessEngine;
use crate::core::engine::EngineBuilder;
use crate::prelude::*;

/// The colours the layer draws, in a checkerboard
const A: [u8; 4] = [220, 30, 40, 255];
const B: [u8; 4] = [30, 60, 230, 255];
/// Their neighbours in the tileset images, never drawn
const NEIGHBOURS: [[u8; 4]; 2] = [[255, 255, 255, 255], [40, 200, 60, 255]];

/// Camera zooms
const SCALES: [f32; 7] = [0.1, 0.33, 0.5, 1.0, 1.5, 3.0, 4.25];
/// The frame's side in pixels
const SIZE: u32 = 64;
/// A tile's side in pixels. At zoom 0.33 a tile is 5.28 pixels: snapping
/// each quad's corner and size to whole pixels on its own would leave a
/// pixel gap every few tiles, where snapping the layer's origin once does not
const TILE: u32 = 16;

/// A tileset of solid `side` px tiles in a row: `colours`, then `more`
/// tiles of the neighbours' colours in turn
fn row(ctx: &mut SceneContext, colours: [[u8; 4]; 2], more: u32, side: UVec2) -> Tileset {
  let picture = row_image(colours, more, side);
  let size = UVec2::new(picture.width(), picture.height());
  let image = ctx.assets().add(picture);
  let mut set = Tileset::new("row", side, TilesetImage::Atlas { image, size });
  set.columns = 2 + more;
  set.tile_count = 2 + more;
  set
}

/// [`row`]'s image
fn row_image(colours: [[u8; 4]; 2], more: u32, side: UVec2) -> Image {
  let count = 2 + more;
  let colour = |tile: u32| match tile {
    0 | 1 => colours[tile as usize],
    _ => NEIGHBOURS[(tile % 2) as usize],
  };
  let (width, height) = (side.x * count, side.y);
  let data: Vec<u8> = (0..width * height)
    .flat_map(|i| colour((i % width) / side.x))
    .collect();
  Image::from_rgba(data, width, height)
}

/// The two tilesets a checkerboard draws from: A (the first's first tile,
/// next to a white one) and B (the second's second, next to a green one),
/// each with `more` tiles after those two
fn pair(ctx: &mut SceneContext, more: u32, side: UVec2) -> Vec<Tileset> {
  vec![
    row(ctx, [A, NEIGHBOURS[0]], more, side),
    row(ctx, [NEIGHBOURS[1], B], more, side),
  ]
}

/// A map of `cells` cells of `cell` px on `shape` over `sets`, filled with
/// `tile(x, y)`, its middle cell's centre on the world's origin, its root
/// scaled by `scale`
fn spawn_filled(
  ctx: &mut SceneContext,
  (shape, cell, cells): (GridShape, UVec2, u32),
  sets: Vec<Tileset>,
  tile: impl Fn(i32, i32) -> Tile,
  scale: Vec2,
) {
  let mut map = TileMap::new(UVec2::splat(cells), cell).with_shape(shape);
  for set in sets {
    map.add_tileset(TilesetSource::Embedded(Arc::new(set)));
  }
  let layer = map.add_tile_layer("ground");
  let grid = map.tiles_mut(layer).expect("tile layer");
  for y in 0..cells as i32 {
    for x in 0..cells as i32 {
      grid.set(IVec2::new(x, y), Some(tile(x, y)));
    }
  }
  let middle = MapGrid::for_map(shape, cell, UVec2::splat(cells), false)
    .cell_center(IVec2::splat(cells as i32 / 2));
  let map = ctx.assets().add(map);
  let at = -middle * scale;
  ctx.world.spawn((
    MapRoot::new(map),
    Transform::from_xy(at.x, at.y).with_scale(scale),
  ));
}

/// A 128 x 128 map of `tile` px cells (4 x 4 chunks) filled with a
/// checkerboard of A (the first tileset's first tile, next to a white one)
/// and B (the second tileset's second, next to a green one), its centre on
/// the world's origin, where four chunks meet. Each tileset has `more`
/// tiles after those two
struct Filled {
  more: u32,
  tile: UVec2,
}

impl Scene for Filled {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    ctx.look().tonemap = Tonemap::Clamp;
    let sets = pair(ctx, self.more, self.tile);
    let cells = 128;
    let mut map = TileMap::new(UVec2::splat(cells), self.tile);
    for set in sets {
      map.add_tileset(TilesetSource::Embedded(Arc::new(set)));
    }
    let layer = map.add_tile_layer("ground");
    let grid = map.tiles_mut(layer).expect("tile layer");
    for y in 0..cells as i32 {
      for x in 0..cells as i32 {
        let tile = if (x + y) % 2 == 0 {
          Tile::new(0, 0)
        } else {
          Tile::new(1, 1)
        };
        grid.set(IVec2::new(x, y), Some(tile));
      }
    }
    let map = ctx.assets().add(map);
    let half = (UVec2::splat(cells) * self.tile / 2).as_vec2();
    ctx
      .world
      .spawn((MapRoot::new(map), Transform::from_xy(-half.x, -half.y)));
  }
}

/// A scene that builds its world with a function
struct Built(fn(&mut SceneContext));

impl Scene for Built {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    ctx.look().tonemap = Tonemap::Clamp;
    (self.0)(ctx);
  }
}

/// What a sweep of camera zooms and quarter-pixel offsets looks at: the
/// zooms, the camera's turn (radians) and which pixels are wrong
struct Sweep {
  zooms: Vec<f32>,
  turn: f32,
  wrong: fn([u8; 4]) -> bool,
}

impl Sweep {
  /// Every pixel A or B, over a layer they fill
  fn filled(zooms: &[f32]) -> Sweep {
    Sweep {
      zooms: zooms.to_vec(),
      turn: 0.0,
      wrong: |p| p != A && p != B,
    }
  }

  /// No pixel showing a neighbour's colour ([`shows_a_neighbour`])
  fn clean(zooms: &[f32]) -> Sweep {
    Sweep {
      wrong: shows_a_neighbour,
      ..Sweep::filled(zooms)
    }
  }

  /// The same through a camera turned `turn` radians
  fn turned(self, turn: f32) -> Sweep {
    Sweep { turn, ..self }
  }
}

/// Whether pixel `p` holds some of a neighbour's colour: it is not A, B, a
/// mix of them, or one of those darkened over the black background.
/// Filters mix a texture's stored bytes, so a mix of A and B is a mix of
/// their bytes; the one taking the pixel's red and blue predicts its green,
/// of which A and B have little and the neighbours much
fn shows_a_neighbour(p: [u8; 4]) -> bool {
  let [r, g, b] = [p[0], p[1], p[2]].map(f32::from);
  let (a, c) = (A.map(f32::from), B.map(f32::from));
  let det = a[0] * c[2] - c[0] * a[2];
  let of_a = (r * c[2] - c[0] * b) / det;
  let of_b = (a[0] * b - r * a[2]) / det;
  g - (of_a * a[1] + of_b * c[1]) > 12.0
}

/// Every camera zoom and quarter-pixel offset of `sweep` over `scene`, the
/// camera clearing to black: the ones where a pixel was wrong, with the
/// first such pixel. `own` says how many of the scene's images have
/// textures of their own (with every mip level) rather than places in the
/// atlas (with four)
fn seams(
  builder: EngineBuilder,
  scene: impl Scene + 'static,
  own: usize,
  sweep: &Sweep,
) -> Option<Vec<String>> {
  let mut engine = HeadlessEngine::new_or_skip(builder.start_scene(scene))?;
  crate::core::tilemap::test_util::step_until_maps_land(&mut engine);
  engine.step_frames(2).unwrap();
  Some(swept(&mut engine, own, sweep))
}

/// `seams` over the engine's world as it is
fn swept(engine: &mut HeadlessEngine, own: usize, sweep: &Sweep) -> Vec<String> {
  let gpu = engine.engine.sprite_gpu.as_ref().expect("renderer");
  assert_eq!(
    gpu.standalone_entity_count(),
    own,
    "how many images have textures of their own"
  );
  let mut found = Vec::new();
  for &scale in &sweep.zooms {
    for (qx, qy) in (0..4).flat_map(|x| (0..4).map(move |y| (x, y))) {
      // Quarter pixels on screen, in world units
      let offset = Vec2::new(qx as f32, qy as f32) * 0.25 / scale;
      let world = engine.world_mut();
      let mut cameras = world.query_filtered::<(&mut Transform, &mut Camera2d), With<MainCamera>>();
      for (mut transform, mut camera) in cameras.iter_mut(world) {
        transform.translation = offset;
        transform.rotation = sweep.turn;
        camera.zoom = scale;
        camera.clear = CameraClear::Color(Color::BLACK);
      }
      // Twice: the main camera is interpolated, and draws where it was
      engine.step_frames(2).unwrap();
      let frame = engine.frame().unwrap();
      let wrong: Vec<(u32, u32, [u8; 4])> = frame
        .enumerate_pixels()
        .filter(|(_, _, p)| (sweep.wrong)(p.0))
        .map(|(x, y, p)| (x, y, p.0))
        .collect();
      if let Some(&(x, y, p)) = wrong.first() {
        found.push(format!(
          "zoom {scale}, offset ({qx}/4, {qy}/4) px: {} pixels, first ({x}, {y}) {p:?}",
          wrong.len()
        ));
      }
      // And the tiles show
      let shown = frame.pixels().filter(|p| p.0 == A || p.0 == B).count();
      if shown < 100 {
        found.push(format!(
          "zoom {scale}, offset ({qx}/4, {qy}/4) px: only {shown} pixels of A or B"
        ));
      }
    }
  }
  found
}

#[test]
fn no_seams_at_any_zoom_or_subpixel_camera() {
  // Pixel art samples nearest and snaps; a resolution-independent target
  // samples linearly, through the mips when zoomed out: the atlas's four
  // levels, or every level of a texture of its own (287 more tiles make the
  // padded images of 14 to 16 px tiles 272 px, over the atlas's 256 px, and
  // 98 more those of 30 px tiles 320 px). Tiles of 14, 15 and 30 px sit
  // just under a power of two, where their cells are a texel or two wider
  // than they are
  let pixel_art = || EngineBuilder::new().pixel_art(SIZE, SIZE);
  let linear = || EngineBuilder::new().resolution(SIZE, SIZE);
  let targets = [
    ("nearest", pixel_art(), TILE, 0),
    ("linear", linear(), TILE, 0),
    ("linear, own textures", linear(), TILE, 287),
    ("linear, 15 px", linear(), 15, 0),
    ("linear, 15 px, own textures", linear(), 15, 287),
    ("linear, 30 px", linear(), 30, 0),
    ("linear, 30 px, own textures", linear(), 30, 98),
    ("linear, 14 px", linear(), 14, 0),
    ("linear, 14 px, own textures", linear(), 14, 287),
  ];
  let mut failures = Vec::new();
  for (filter, builder, tile, more) in targets {
    let own = if more > 0 { 2 } else { 0 };
    let tile = UVec2::splat(tile);
    let sweep = Sweep::filled(&SCALES);
    let Some(found) = seams(builder, Filled { more, tile }, own, &sweep) else {
      return;
    };
    failures.extend(found.into_iter().map(|f| format!("{filter}: {f}")));
  }
  assert!(failures.is_empty(), "seams:\n{}", failures.join("\n"));
}

/// `zooms` but those at which a tile `side` px on its shorter side draws
/// under 4 pixels wide
fn while_wide(zooms: &[f32], side: u32) -> Vec<f32> {
  zooms
    .iter()
    .copied()
    .filter(|zoom| side as f32 * zoom >= 4.0)
    .collect()
}

/// A collection of twenty 12 x 20 images (A, B, and the neighbours' colours
/// in turn) and one 72 x 72 (white): a 128 x 128 map of 12 x 20 cells holds
/// A and B in a checkerboard
fn mixed_collection(ctx: &mut SceneContext) {
  let mut set = Tileset::new("mixed", UVec2::splat(72), TilesetImage::Collection);
  let mut add = |id: u32, size: UVec2, [r, g, b, a]: [u8; 4]| {
    let image = ctx
      .assets()
      .add(Image::solid_rect(size.x, size.y, r, g, b, a));
    set.insert_tile(
      id,
      TileData {
        image: Some((image, size)),
        ..TileData::default()
      },
    );
  };
  for id in 0..20 {
    let colour = match id {
      0 => A,
      1 => B,
      _ => NEIGHBOURS[(id % 2) as usize],
    };
    add(id, UVec2::new(12, 20), colour);
  }
  add(20, UVec2::splat(72), NEIGHBOURS[0]);
  set.tile_count = 21;
  let tile = |x: i32, y: i32| Tile::new(0, ((x + y) % 2) as u32);
  let shape = (GridShape::Orthogonal, UVec2::new(12, 20), 128);
  spawn_filled(ctx, shape, vec![set], tile, Vec2::ONE);
}

#[test]
fn big_tiles_and_mixed_collections_show_no_neighbour_at_any_zoom() {
  // Through every level a zoom reads (the smooth target), wherever a tile
  // is 4 pixels wide or more: 40 x 56 tiles in 48 x 64 cells (four clean
  // levels) in a texture of their own (30 tiles); a collection mixing one
  // 72 x 72 image with twenty 12 x 20 ones; and 40 x 56 tiles in the atlas
  // (9 tiles), whose cells are the tiles themselves
  const ZOOMS: [f32; 7] = [1.0, 0.5, 0.25, 0.125, 0.0625, 0.7, 0.3];
  let linear = || EngineBuilder::new().resolution(SIZE, SIZE);
  let big = UVec2::new(40, 56);
  let mut failures = Vec::new();
  for (name, more, own) in [("own texture", 28, 2), ("atlas", 7, 0)] {
    let sweep = Sweep::filled(&while_wide(&ZOOMS, big.x));
    let Some(found) = seams(linear(), Filled { more, tile: big }, own, &sweep) else {
      return;
    };
    failures.extend(found.into_iter().map(|f| format!("{name}: {f}")));
  }
  let sweep = Sweep::filled(&while_wide(&ZOOMS, 12));
  let Some(found) = seams(linear(), Built(mixed_collection), 0, &sweep) else {
    return;
  };
  failures.extend(found.into_iter().map(|f| format!("mixed collection: {f}")));
  assert!(failures.is_empty(), "neighbours:\n{}", failures.join("\n"));
}

/// A hexagonal map of 48 px tiles from the atlas, A and B in a
/// checkerboard, turned by none, 60, 120 and 180 degrees in turn
fn hex_turns(ctx: &mut SceneContext) {
  let sets = pair(ctx, 2, UVec2::splat(48));
  let turns = [
    TileFlip::NONE,
    TileFlip::HEX_ROTATE_60,
    TileFlip::HEX_ROTATE_120,
    TileFlip::HEX_ROTATE_60 | TileFlip::HEX_ROTATE_120,
  ];
  let tile = move |x: i32, y: i32| {
    let set = ((x + y) % 2) as u16;
    Tile::new(set, u32::from(set)).with_flip(turns[((x + 2 * y) % 4) as usize])
  };
  let shape = GridShape::Hexagonal {
    axis: StaggerAxis::Y,
    index: StaggerIndex::Odd,
    side: 24,
  };
  spawn_filled(ctx, (shape, UVec2::splat(48), 32), sets, tile, Vec2::ONE);
}

/// The tiles of [`flips_and_y_sorted`], [`squashed`] and [`upright`]: their
/// own size a whole number of texels of four levels, so their cells are
/// the tiles themselves and only the shader keeps a zoomed-out read off
/// the next cell
const SNUG: UVec2 = UVec2::new(48, 64);

/// 48 x 64 tiles in a texture of their own on an orthogonal map, A and B in
/// a checkerboard under every flip in turn, and over them a y-sorted layer
/// holding them on every third cell
fn flips_and_y_sorted(ctx: &mut SceneContext) {
  let sets = pair(ctx, 34, SNUG);
  let flip = |x: i32, y: i32| {
    let bits = (3 * x + y).rem_euclid(8);
    let mut flip = TileFlip::NONE;
    for (bit, one) in [
      (1, TileFlip::HORIZONTAL),
      (2, TileFlip::VERTICAL),
      (4, TileFlip::DIAGONAL),
    ] {
      if bits & bit != 0 {
        flip = flip | one;
      }
    }
    flip
  };
  let tile = |x: i32, y: i32| {
    let set = ((x + y) % 2) as u16;
    Tile::new(set, u32::from(set)).with_flip(flip(x, y))
  };
  let cells = 64;
  let mut map = TileMap::new(UVec2::splat(cells), SNUG);
  for set in sets {
    map.add_tileset(TilesetSource::Embedded(Arc::new(set)));
  }
  for name in ["ground", "sorted"] {
    let layer = map.add_tile_layer(name);
    let sorted = name == "sorted";
    map.layers[layer].info.y_sorted = sorted;
    let grid = map.tiles_mut(layer).expect("tile layer");
    for y in 0..cells as i32 {
      for x in 0..cells as i32 {
        if !sorted || (x + y) % 3 == 0 {
          grid.set(IVec2::new(x, y), Some(tile(x + i32::from(sorted), y)));
        }
      }
    }
  }
  let map = ctx.assets().add(map);
  let half = (UVec2::splat(cells) * SNUG / 2).as_vec2();
  ctx
    .world
    .spawn((MapRoot::new(map), Transform::from_xy(-half.x, -half.y)));
}

/// A collection whose A and B tiles (40 x 56) animate through themselves
/// and a 48 px picture of their colour, with neighbours of both sizes, in a
/// checkerboard on an orthogonal map of 40 x 56 cells
fn animated(ctx: &mut SceneContext) {
  let (tall, square) = (UVec2::new(40, 56), UVec2::splat(48));
  let pictures = [
    (tall, A),
    (square, A),
    (square, NEIGHBOURS[0]),
    (tall, NEIGHBOURS[1]),
    (tall, B),
    (square, B),
    (square, NEIGHBOURS[1]),
    (tall, NEIGHBOURS[0]),
  ];
  let mut set = Tileset::new("animated", tall.max(square), TilesetImage::Collection);
  for (id, (size, [r, g, b, a])) in pictures.into_iter().enumerate() {
    let image = ctx
      .assets()
      .add(Image::solid_rect(size.x, size.y, r, g, b, a));
    set.insert_tile(
      id as u32,
      TileData {
        image: Some((image, size)),
        ..TileData::default()
      },
    );
  }
  set.tile_count = pictures.len() as u32;
  for first in [0, 4] {
    let mut data = set.tile(first).cloned().unwrap_or_default();
    data.animation = [first, first + 1]
      .map(|tile| TileFrame {
        tile,
        duration: std::time::Duration::from_millis(50),
      })
      .to_vec();
    set.insert_tile(first, data);
  }
  let tile = |x: i32, y: i32| Tile::new(0, 4 * ((x + y) % 2) as u32);
  let shape = (GridShape::Orthogonal, tall, 32);
  spawn_filled(ctx, shape, vec![set], tile, Vec2::ONE);
}

/// A tile object of a 128 x 160 tile (A, from an atlas of four with
/// neighbours, a texture of its own, its cells the tiles themselves)
/// centred on the world's origin
fn tile_object(ctx: &mut SceneContext) {
  let size = UVec2::new(128, 160);
  let set = row(ctx, [A, NEIGHBOURS[0]], 2, size);
  let mut map = TileMap::new(UVec2::ONE, size);
  map.add_tileset(TilesetSource::Embedded(Arc::new(set)));
  let objects = map.add_object_layer("things");
  map.layers[objects].kind = LayerKind::Objects(vec![MapObjectData {
    id: "1".to_string(),
    name: String::new(),
    class: None,
    position: -size.as_vec2() / 2.0,
    size: size.as_vec2(),
    rotation: 0.0,
    shape: ObjectShape::Rect,
    tile: Some(Tile::new(0, 0)),
    visible: true,
    properties: Properties::default(),
  }]);
  let map = ctx.assets().add(map);
  ctx.world.spawn((MapRoot::new(map), Transform::IDENTITY));
}

#[test]
fn turned_flipped_and_animated_tiles_and_tile_objects_show_no_neighbour() {
  // On the smooth target, zoomed out to an eighth: hexagonal tiles turned
  // 60 and 120 degrees; tiles flipped every way and a y-sorted layer over
  // them; tiles animating through pictures of two sizes; and a tile object
  // of a big tile, drawn as a sprite. No pixel shows a neighbour's colour
  const ZOOMS: [f32; 4] = [1.0, 0.5, 0.25, 0.125];
  let linear = || EngineBuilder::new().resolution(SIZE, SIZE);
  let cases = [
    ("hexagonal turns", Built(hex_turns), 0),
    ("flips and a y-sorted layer", Built(flips_and_y_sorted), 2),
    ("animated", Built(animated), 0),
    ("tile object", Built(tile_object), 1),
  ];
  let mut failures = Vec::new();
  for (name, scene, own) in cases {
    let Some(found) = seams(linear(), scene, own, &Sweep::clean(&ZOOMS)) else {
      return;
    };
    failures.extend(found.into_iter().map(|f| format!("{name}: {f}")));
  }
  assert!(failures.is_empty(), "neighbours:\n{}", failures.join("\n"));
}

/// 48 x 64 tiles in a texture of their own, A and B in a checkerboard, the
/// map's transform squashing it to a quarter of its height
fn squashed(ctx: &mut SceneContext) {
  checkered(ctx, Vec2::new(1.0, 0.25));
}

/// The same, unsquashed
fn upright(ctx: &mut SceneContext) {
  checkered(ctx, Vec2::ONE);
}

/// 48 x 64 tiles in a texture of their own, A and B in a checkerboard, the
/// map's root scaled by `scale`
fn checkered(ctx: &mut SceneContext, scale: Vec2) {
  let sets = pair(ctx, 34, SNUG);
  let tile = |x: i32, y: i32| {
    let set = ((x + y) % 2) as u16;
    Tile::new(set, u32::from(set))
  };
  spawn_filled(ctx, (GridShape::Orthogonal, SNUG, 64), sets, tile, scale);
}

#[test]
fn a_squashed_layer_and_a_turned_camera_show_no_neighbour() {
  // A layer drawn at a quarter of its height reads a coarser level than its
  // width alone asks; a camera turned 30 degrees reads each tile slanted.
  // Neither shows a neighbour's colour
  const ZOOMS: [f32; 3] = [1.0, 0.5, 0.25];
  let linear = || EngineBuilder::new().resolution(SIZE, SIZE);
  let mut failures = Vec::new();
  let cases = [
    ("squashed", Built(squashed), Sweep::clean(&ZOOMS)),
    (
      "turned camera",
      Built(upright),
      Sweep::clean(&ZOOMS).turned(30f32.to_radians()),
    ),
  ];
  for (name, scene, sweep) in cases {
    let Some(found) = seams(linear(), scene, 2, &sweep) else {
      return;
    };
    failures.extend(found.into_iter().map(|f| format!("{name}: {f}")));
  }
  assert!(failures.is_empty(), "neighbours:\n{}", failures.join("\n"));
}

/// The ring's and the inside's colours of [`ringed`]'s tile
const RING: [u8; 4] = [230, 20, 20, 255];
const INSIDE: [u8; 4] = [20, 200, 20, 255];

/// One 16 px tile whose outer ring of texels is `RING` and whose inside is
/// `INSIDE`, in the cell at the world's origin
fn ringed(ctx: &mut SceneContext) {
  let data: Vec<u8> = (0..16 * 16u32)
    .flat_map(|i| {
      let (x, y) = (i % 16, i / 16);
      if x == 0 || y == 0 || x == 15 || y == 15 {
        RING
      } else {
        INSIDE
      }
    })
    .collect();
  let image = ctx.assets().add(Image::from_rgba(data, 16, 16));
  let size = UVec2::splat(16);
  let mut set = Tileset::new("ringed", size, TilesetImage::Atlas { image, size });
  set.columns = 1;
  set.tile_count = 1;
  let mut map = TileMap::new(UVec2::ONE, size);
  map.add_tileset(TilesetSource::Embedded(Arc::new(set)));
  let layer = map.add_tile_layer("ground");
  map
    .tiles_mut(layer)
    .expect("tile layer")
    .set(IVec2::ZERO, Some(Tile::new(0, 0)));
  let map = ctx.assets().add(map);
  ctx.world.spawn((MapRoot::new(map), Transform::IDENTITY));
}

#[test]
fn a_tile_zoomed_out_a_little_keeps_its_outer_texels_sharp() {
  // At zoom 0.75 a pixel spans 4/3 texels, so the pixel at each of the
  // tile's edges (on a whole pixel) covers the ring's texel and a third of
  // the next: in proportion three quarters ring. Reading the two levels
  // round the zoom's, each kept inside the cell at its own level, keeps
  // about that; one read at the zoom's level, kept inside for the coarser
  // one, would blur the ring into the inside, half and half. So each edge
  // pixel is at least two thirds of the way from the inside to the ring
  let builder = EngineBuilder::new()
    .resolution(SIZE, SIZE)
    .start_scene(Built(ringed));
  let Some(mut engine) = HeadlessEngine::new_or_skip(builder) else {
    return;
  };
  crate::core::tilemap::test_util::step_until_maps_land(&mut engine);
  let world = engine.world_mut();
  let mut cameras = world.query_filtered::<(&mut Transform, &mut Camera2d), With<MainCamera>>();
  for (mut transform, mut camera) in cameras.iter_mut(world) {
    // The tile's 12 pixels from pixel 26 to 38 on both axes
    transform.translation = Vec2::splat(8.0);
    camera.zoom = 0.75;
  }
  engine.step_frames(2).unwrap();
  let frame = engine.frame().unwrap();
  // How far `p` is from the inside's colour toward the ring's, in RGB
  let along = |p: [u8; 4]| {
    let from_inside = |c: [u8; 4]| [0, 1, 2].map(|i| f32::from(c[i]) - f32::from(INSIDE[i]));
    let dot = |u: [f32; 3], v: [f32; 3]| u[0] * v[0] + u[1] * v[1] + u[2] * v[2];
    let (p, ring) = (from_inside(p), from_inside(RING));
    dot(p, ring) / dot(ring, ring)
  };
  let mut blurred = Vec::new();
  for x in [26, 37] {
    for y in 29..=34 {
      let p = frame.get_pixel(x, y).0;
      if along(p) < 2.0 / 3.0 {
        blurred.push(format!(
          "({x}, {y}) {p:?}: {:.2} of the way to the ring",
          along(p)
        ));
      }
    }
  }
  assert!(
    blurred.is_empty(),
    "the ring blurred:\n{}",
    blurred.join("\n")
  );
}

/// A map whose one layer is an image repeated across and down from the
/// world's origin: 16 px square, A on its left half and B on its right
struct Repeated;

impl Scene for Repeated {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    ctx.look().tonemap = Tonemap::Clamp;
    let data: Vec<u8> = (0..TILE * TILE)
      .flat_map(|i| if i % TILE < TILE / 2 { A } else { B })
      .collect();
    let image = ctx.assets().add(Image::from_rgba(data, TILE, TILE));
    let mut map = TileMap::new(UVec2::ONE, UVec2::splat(TILE));
    let mut info = LayerInfo::new(1, "sky");
    info.collides = false;
    map.layers.push(MapLayer {
      info,
      kind: LayerKind::Image(ImageLayer {
        image: Some(image),
        repeat_x: true,
        repeat_y: true,
        transparent: None,
      }),
    });
    let map = ctx.assets().add(map);
    ctx.world.spawn((MapRoot::new(map), Transform::IDENTITY));
  }
}

#[test]
fn a_repeated_image_layer_shows_no_gaps_at_fractional_zoom() {
  // On the pixel-art target, at every zoom and quarter-pixel camera offset,
  // every pixel is the image's: copies one after another, never a gap
  // between two (at zoom 0.33 a copy is 5.28 pixels)
  let builder = EngineBuilder::new().pixel_art(SIZE, SIZE);
  let Some(found) = seams(builder, Repeated, 0, &Sweep::filled(&SCALES)) else {
    return;
  };
  assert!(found.is_empty(), "gaps:\n{}", found.join("\n"));
  // And the copies start at the layer's origin snapped once, as a tile
  // layer's tiles do: a camera a fraction of a pixel off draws them as at
  // the whole pixel it rounds to
  let Some(found) = slides(Repeated) else {
    return;
  };
  assert!(found.is_empty(), "the copies slid:\n{}", found.join("\n"));
}

/// A map of 6 px tiles whose texels are in four colours by the parity of
/// their column and row, its top-left corner on the world's origin
struct Checked;

impl Scene for Checked {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    ctx.look().tonemap = Tonemap::Clamp;
    let colours = [A, B, NEIGHBOURS[0], NEIGHBOURS[1]];
    let data: Vec<u8> = (0..36u32)
      .flat_map(|i| colours[((i % 6) % 2 + 2 * ((i / 6) % 2)) as usize])
      .collect();
    let image = ctx.assets().add(Image::from_rgba(data, 6, 6));
    let size = UVec2::splat(6);
    let mut set = Tileset::new("checked", size, TilesetImage::Atlas { image, size });
    set.columns = 1;
    set.tile_count = 1;
    let mut map = TileMap::new(UVec2::splat(16), size);
    map.add_tileset(TilesetSource::Embedded(Arc::new(set)));
    let layer = map.add_tile_layer("ground");
    let grid = map.tiles_mut(layer).expect("tile layer");
    for y in 0..16 {
      for x in 0..16 {
        grid.set(IVec2::new(x, y), Some(Tile::new(0, 0)));
      }
    }
    let map = ctx.assets().add(map);
    ctx.world.spawn((MapRoot::new(map), Transform::IDENTITY));
  }
}

/// Over `scene` on the pixel-art target at zoom 4/3, where a texel is 4/3
/// of a pixel: the camera positions putting the world's origin a fraction of
/// a pixel from pixel (10, 10) whose frames differ from the one with the
/// origin on it, each with its first differing pixel
fn slides(scene: impl Scene + 'static) -> Option<Vec<String>> {
  let zoom = 4.0 / 3.0;
  let builder = EngineBuilder::new()
    .pixel_art(SIZE, SIZE)
    .start_scene(scene);
  let mut engine = HeadlessEngine::new_or_skip(builder)?;
  crate::core::tilemap::test_util::step_until_maps_land(&mut engine);
  // The camera centre that puts the world's origin at `corner` on screen
  let mut frame_at = |corner: Vec2| {
    let centre = (Vec2::splat(SIZE as f32 / 2.0) - corner) / zoom;
    let world = engine.world_mut();
    let mut cameras = world.query_filtered::<(&mut Transform, &mut Camera2d), With<MainCamera>>();
    for (mut transform, mut camera) in cameras.iter_mut(world) {
      transform.translation = centre;
      camera.zoom = zoom;
    }
    // Twice: the main camera is interpolated, and draws where it was
    engine.step_frames(2).unwrap();
    engine.frame().unwrap()
  };
  let whole = frame_at(Vec2::splat(10.0));
  let mut found = Vec::new();
  for fraction in [(0.2, 0.4), (0.4, -0.2), (-0.4, 0.3), (-0.2, -0.45)] {
    let corner = Vec2::splat(10.0) + Vec2::from(fraction);
    let frame = frame_at(corner);
    let differ: Vec<(u32, u32)> = frame
      .enumerate_pixels()
      .filter(|(x, y, p)| p.0 != whole.get_pixel(*x, *y).0)
      .map(|(x, y, _)| (x, y))
      .collect();
    if let Some(first) = differ.first() {
      found.push(format!(
        "corner at {corner}: {} pixels, first {first:?}",
        differ.len()
      ));
    }
  }
  Some(found)
}

#[test]
fn a_sub_pixel_camera_draws_tiles_as_at_the_whole_pixel_they_round_to() {
  // A 6 px tile is 8 pixels at zoom 4/3, so which texel a pixel shows hangs
  // on where the layer starts to a fraction of a pixel. Its origin snaps to
  // a whole pixel: with the map's corner a fraction of a pixel from pixel
  // (10, 10) the frame is the one drawn with the corner on it. (Without the
  // snap the texels slide under the pixels as the camera moves)
  let Some(found) = slides(Checked) else {
    return;
  };
  assert!(found.is_empty(), "the tiles slid:\n{}", found.join("\n"));
}

/// A 48 x 48 map of `shape` on cells of `cell` px, every cell holding a
/// solid tile its cell's size: A (the first tileset's first tile) or B (the
/// second's second) in a checkerboard of x + y, at the world's origin
struct Odd {
  shape: GridShape,
  cell: UVec2,
}

/// The odd maps' side, in cells
const ODD_CELLS: i32 = 48;

impl Scene for Odd {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    ctx.look().tonemap = Tonemap::Clamp;
    let mut map = TileMap::new(UVec2::splat(ODD_CELLS as u32), self.cell).with_shape(self.shape);
    for colours in [[A, NEIGHBOURS[0]], [NEIGHBOURS[1], B]] {
      let (width, height) = (self.cell.x * 2, self.cell.y);
      let data: Vec<u8> = (0..width * height)
        .flat_map(|i| colours[((i % width) / self.cell.x) as usize])
        .collect();
      let image = ctx.assets().add(Image::from_rgba(data, width, height));
      let size = UVec2::new(width, height);
      let mut set = Tileset::new("odd", self.cell, TilesetImage::Atlas { image, size });
      set.columns = 2;
      set.tile_count = 2;
      map.add_tileset(TilesetSource::Embedded(Arc::new(set)));
    }
    let layer = map.add_tile_layer("ground");
    let grid = map.tiles_mut(layer).expect("tile layer");
    for y in 0..ODD_CELLS {
      for x in 0..ODD_CELLS {
        let tile = if (x + y) % 2 == 0 {
          Tile::new(0, 0)
        } else {
          Tile::new(1, 1)
        };
        grid.set(IVec2::new(x, y), Some(tile));
      }
    }
    let map = ctx.assets().add(map);
    ctx.world.spawn((MapRoot::new(map), Transform::IDENTITY));
  }
}

/// Where Tiled's renderer puts a tile, of a tileset no wider or taller
/// than the cell is wide, whose bottom-left the grid places at `anchor`: an
/// isometric grid of odd sizes places tiles on half pixels, which it rounds
/// up, and its odd cell heights put a tile a pixel lower where its bottom
/// is a whole pixel (measured; the formats note has it); every other grid
/// places them on whole pixels
fn tileds_raster(grid: &MapGrid, anchor: Vec2) -> IVec2 {
  if grid.shape != GridShape::Isometric {
    assert_eq!(
      anchor,
      anchor.round(),
      "{grid:?} places tiles on whole pixels"
    );
    return anchor.as_ivec2();
  }
  let y = if grid.cell.y % 2 == 1 {
    anchor.y.floor() + 1.0
  } else {
    anchor.y
  };
  Vec2::new(anchor.x.ceil(), y).as_ivec2()
}

/// What Tiled's raster draws of an `Odd` map in the world's pixels from
/// `min`, `side` a side: each cell's tile where `tileds_raster` puts it, in
/// Tiled's paint order (the later over the earlier)
fn odd_reference(grid: &MapGrid, min: IVec2, side: i32) -> Vec<Option<[u8; 4]>> {
  let mut cells: Vec<IVec2> = (0..ODD_CELLS)
    .flat_map(|y| (0..ODD_CELLS).map(move |x| IVec2::new(x, y)))
    .collect();
  cells.sort_by_key(|&cell| grid.paint_key(cell));
  let mut out = vec![None; (side * side) as usize];
  let size = grid.cell.as_ivec2();
  for cell in cells {
    let bottom_left = tileds_raster(grid, grid.tile_anchor(cell));
    let colour = if (cell.x + cell.y) % 2 == 0 { A } else { B };
    for y in bottom_left.y - size.y..bottom_left.y {
      for x in bottom_left.x..bottom_left.x + size.x {
        let at = IVec2::new(x, y) - min;
        if at.cmpge(IVec2::ZERO).all() && at.cmplt(IVec2::splat(side)).all() {
          out[(at.y * side + at.x) as usize] = Some(colour);
        }
      }
    }
  }
  out
}

#[test]
fn half_pixel_steps_draw_without_seams_in_pixel_art() {
  // Odd cell sizes, whose half cells fall on half pixels: Tiled steps
  // staggered and hexagonal grids in whole pixels (so tiles a pixel larger
  // than the step overlap their neighbours) and isometric ones on half
  // pixels, which its raster rounds. Solid tiles the cell's size from two
  // tilesets cover each map; at zooms 1 to 4, and with the camera a fraction
  // of a pixel off, the pixel-art frame over a corner where four chunks meet
  // is what Tiled's raster draws, scaled: each tile where Tiled puts it, in
  // Tiled's order, every pixel one tile's colour, never the background
  // between two
  const FRAME: u32 = 96;
  let cell = UVec2::new(33, 17);
  let (x, y) = (StaggerAxis::X, StaggerAxis::Y);
  let grids = [
    GridShape::Isometric,
    GridShape::Staggered {
      axis: x,
      index: StaggerIndex::Odd,
    },
    GridShape::Staggered {
      axis: y,
      index: StaggerIndex::Even,
    },
    GridShape::Hexagonal {
      axis: x,
      index: StaggerIndex::Odd,
      side: 13,
    },
    GridShape::Hexagonal {
      axis: y,
      index: StaggerIndex::Odd,
      side: 13,
    },
  ];
  let fractions = [
    (0.0, 0.0),
    (0.2, 0.4),
    (0.4, -0.2),
    (-0.4, 0.3),
    (-0.2, -0.45),
  ];
  let mut failures = Vec::new();
  for shape in grids {
    let builder = EngineBuilder::new()
      .pixel_art(FRAME, FRAME)
      .start_scene(Odd { shape, cell });
    let Some(mut engine) = HeadlessEngine::new_or_skip(builder) else {
      return;
    };
    crate::core::tilemap::test_util::step_until_maps_land(&mut engine);
    let grid = MapGrid::for_map(shape, cell, UVec2::splat(ODD_CELLS as u32), false);
    // The camera on a whole pixel by cell (32, 32), where chunks meet
    let centre = grid.cell_center(IVec2::splat(32)).round();
    let half = FRAME as i32 / 2;
    let min = centre.as_ivec2() - half - 2;
    let side = FRAME as i32 + 4;
    let reference = odd_reference(&grid, min, side);
    assert!(
      reference.iter().all(Option::is_some),
      "{shape:?}: the tiles cover the view"
    );
    for zoom in [1.0f32, 2.0, 3.0, 4.0] {
      for fraction in fractions {
        let at = centre + Vec2::from(fraction) / zoom;
        let world = engine.world_mut();
        let mut cameras =
          world.query_filtered::<(&mut Transform, &mut Camera2d), With<MainCamera>>();
        for (mut transform, mut camera) in cameras.iter_mut(world) {
          transform.translation = at;
          camera.zoom = zoom;
        }
        // Twice: the main camera is interpolated, and draws where it was
        engine.step_frames(2).unwrap();
        let frame = engine.frame().unwrap();
        let unlike: Vec<(u32, u32, [u8; 4], [u8; 4])> = frame
          .enumerate_pixels()
          .filter_map(|(sx, sy, p)| {
            let world = centre + (Vec2::new(sx as f32, sy as f32) + 0.5 - half as f32) / zoom;
            let at = world.floor().as_ivec2() - min;
            let want = reference[(at.y * side + at.x) as usize]?;
            (p.0 != want).then_some((sx, sy, p.0, want))
          })
          .collect();
        if let Some(&(sx, sy, got, want)) = unlike.first() {
          failures.push(format!(
            "{shape:?}, zoom {zoom}, camera {fraction:?} px off: {} pixels, first ({sx}, {sy}) {got:?}, Tiled {want:?}",
            unlike.len()
          ));
        }
      }
    }
  }
  assert!(
    failures.is_empty(),
    "unlike Tiled:\n{}",
    failures.join("\n")
  );
}

/// The tileset [`grows`] adds and its image
#[derive(Resource, Clone)]
struct Grown(Handle<Tileset>, Handle<Image>);

/// A 64 x 64 map of 40 px cells holding A and B in a checkerboard from a
/// tileset asset of 9 tiles (A, B and seven neighbours), which pads into
/// the atlas
fn grows(ctx: &mut SceneContext) {
  let mut set = row(ctx, [A, B], 7, UVec2::splat(40));
  set.name = "grows-4e1b".to_string();
  let TilesetImage::Atlas { image, .. } = set.image.clone() else {
    return;
  };
  let handle = ctx.assets().add(set);
  ctx.world.insert_resource(Grown(handle.clone(), image));
  let cells = 64;
  let mut map = TileMap::new(UVec2::splat(cells), UVec2::splat(40));
  map.add_tileset(TilesetSource::Asset(handle));
  let layer = map.add_tile_layer("ground");
  let grid = map.tiles_mut(layer).expect("tile layer");
  for y in 0..cells as i32 {
    for x in 0..cells as i32 {
      grid.set(IVec2::new(x, y), Some(Tile::new(0, ((x + y) % 2) as u32)));
    }
  }
  let map = ctx.assets().add(map);
  let half = (cells * 20) as f32;
  ctx
    .world
    .spawn((MapRoot::new(map), Transform::from_xy(-half, -half)));
}

#[test]
fn a_tileset_that_grows_past_the_atlas_pads_again_and_draws_clean() {
  // Drawn at zoom 0.25 (a tile 10 pixels wide, reading level 2), a tileset
  // of 9 tiles of 40 px pads into 40 px cells, 120 px, in the atlas. Its
  // image replaced by one of 36 tiles, it pads again into 48 px cells (four
  // clean levels), 288 px, a texture of its own with every level, and draws
  // clean from it
  use crate::core::tilemap::padded::{PaddedSize, padded_sizes};
  let builder = EngineBuilder::new()
    .resolution(SIZE, SIZE)
    .start_scene(Built(grows));
  let Some(mut engine) = HeadlessEngine::new_or_skip(builder) else {
    return;
  };
  crate::core::tilemap::test_util::step_until_maps_land(&mut engine);
  engine.step_frames(2).unwrap();
  let sweep = Sweep::filled(&[0.25]);
  let found = swept(&mut engine, 0, &sweep);
  assert!(found.is_empty(), "in the atlas:\n{}", found.join("\n"));
  let size = |bytes, tiles: u64| PaddedSize {
    name: "grows-4e1b".to_string(),
    bytes,
    art_bytes: tiles * 40 * 40 * 4,
  };
  // 120 px and its three smaller copies
  let atlas = (120 * 120 + 60 * 60 + 30 * 30 + 15 * 15) * 4;
  assert_eq!(padded_sizes(engine.world()), [size(atlas, 9)]);

  let Grown(set, picture) = engine.world().resource::<Grown>().clone();
  let world = engine.world_mut();
  let grown = row_image([A, B], 34, UVec2::splat(40));
  let grown_size = UVec2::new(grown.width(), grown.height());
  if let Some(image) = world.resource_mut::<Assets<Image>>().get_mut(&picture) {
    *image = grown;
  }
  if let Some(tileset) = world.resource_mut::<Assets<Tileset>>().get_mut(&set) {
    tileset.columns = 36;
    tileset.tile_count = 36;
    tileset.image = TilesetImage::Atlas {
      image: picture.clone(),
      size: grown_size,
    };
  }
  engine.step_frames(2).unwrap();
  let found = swept(&mut engine, 1, &sweep);
  assert!(found.is_empty(), "padded again:\n{}", found.join("\n"));
  // 288 px and every smaller copy, down to one texel
  let own: u64 = (0..9).map(|k| (288u64 >> k).pow(2) * 4).sum();
  assert_eq!(padded_sizes(engine.world()), [size(own, 36)]);
}

/// A row of `count` [`SNUG`] tiles: A, white, stripes a texel wide of A and
/// B in turn across (A first), green, then the neighbours' colours in turn
fn striped_row(ctx: &mut SceneContext, count: u32) -> Tileset {
  let side = SNUG;
  let width = side.x * count;
  let data: Vec<u8> = (0..width * side.y)
    .flat_map(|i| {
      let x = i % width;
      match x / side.x {
        0 => A,
        2 if (x % side.x).is_multiple_of(2) => A,
        2 => B,
        1 => NEIGHBOURS[0],
        3 => NEIGHBOURS[1],
        tile => NEIGHBOURS[(tile % 2) as usize],
      }
    })
    .collect();
  let image = ctx.assets().add(Image::from_rgba(data, width, side.y));
  let size = UVec2::new(width, side.y);
  let mut set = Tileset::new("striped", side, TilesetImage::Atlas { image, size });
  set.columns = count;
  set.tile_count = count;
  set
}

/// A checkerboard of A and stripes ([`striped_row`]'s tiles 0 and 2), its
/// middle on the world's origin, on a tile layer sampled with `filter`, or
/// as tile objects on an object layer sampled so
struct Striped {
  filter: Filter,
  objects: bool,
}

impl Scene for Striped {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    ctx.look().tonemap = Tonemap::Clamp;
    let set = striped_row(ctx, 36);
    let cells = if self.objects { 8 } else { 64 };
    let tile = |x: i32, y: i32| Tile::new(0, 2 * ((x + y) % 2) as u32);
    let mut map = TileMap::new(UVec2::splat(cells), SNUG);
    map.add_tileset(TilesetSource::Embedded(Arc::new(set)));
    let layer = if self.objects {
      let layer = map.add_object_layer("things");
      let objects = (0..cells as i32)
        .flat_map(|y| (0..cells as i32).map(move |x| (x, y)))
        .map(|(x, y)| MapObjectData {
          id: format!("{}", y * cells as i32 + x + 1),
          name: String::new(),
          class: None,
          position: (IVec2::new(x, y).as_vec2()) * SNUG.as_vec2(),
          size: SNUG.as_vec2(),
          rotation: 0.0,
          shape: ObjectShape::Rect,
          tile: Some(tile(x, y)),
          visible: true,
          properties: Properties::default(),
        })
        .collect();
      map.layers[layer].kind = LayerKind::Objects(objects);
      layer
    } else {
      let layer = map.add_tile_layer("ground");
      let grid = map.tiles_mut(layer).expect("tile layer");
      for y in 0..cells as i32 {
        for x in 0..cells as i32 {
          grid.set(IVec2::new(x, y), Some(tile(x, y)));
        }
      }
      layer
    };
    map.layers[layer].info.filter = self.filter;
    let map = ctx.assets().add(map);
    let half = (UVec2::splat(cells) * SNUG / 2).as_vec2();
    ctx
      .world
      .spawn((MapRoot::new(map), Transform::from_xy(-half.x, -half.y)));
  }
}

/// The frames of `scene` at each of `zooms` and each quarter-pixel camera
/// offset, the camera clearing to black, each with where it was taken
fn frames_of(
  builder: EngineBuilder,
  scene: impl Scene + 'static,
  zooms: &[f32],
) -> Option<Vec<(String, image::RgbaImage)>> {
  let mut engine = HeadlessEngine::new_or_skip(builder.start_scene(scene))?;
  crate::core::tilemap::test_util::step_until_maps_land(&mut engine);
  engine.step_frames(2).unwrap();
  let mut frames = Vec::new();
  for &zoom in zooms {
    for (qx, qy) in (0..4).flat_map(|x| (0..4).map(move |y| (x, y))) {
      let offset = Vec2::new(qx as f32, qy as f32) * 0.25 / zoom;
      let world = engine.world_mut();
      let mut cameras = world.query_filtered::<(&mut Transform, &mut Camera2d), With<MainCamera>>();
      for (mut transform, mut camera) in cameras.iter_mut(world) {
        transform.translation = offset;
        camera.zoom = zoom;
        camera.clear = CameraClear::Color(Color::BLACK);
      }
      engine.step_frames(2).unwrap();
      let at = format!("zoom {zoom}, offset ({qx}/4, {qy}/4) px");
      frames.push((at, engine.frame().unwrap()));
    }
  }
  Some(frames)
}

#[test]
fn a_layer_s_filter_picks_its_sampling_whatever_the_target() {
  // At a half and a quarter, stripes a texel wide sampled nearest show one
  // of their colours a pixel, and sampled smoothly the level the zoom needs,
  // their mix. On the pixel-art target a layer asking for linear sampling
  // reads its smaller copies, as do tile objects on an object layer asking
  // for it; on the smooth target a layer asking for nearest keeps the
  // full-size texels. None shows a neighbour, the tiles being their own
  // cells. A layer that asks for nothing draws exactly as a nearest one
  const ZOOMS: [f32; 2] = [0.5, 0.25];
  let pixel_art = || EngineBuilder::new().pixel_art(SIZE, SIZE);
  let smooth = || EngineBuilder::new().resolution(SIZE, SIZE);
  let striped = |filter, objects| Striped { filter, objects };
  let Some(linear) = frames_of(pixel_art(), striped(Filter::Linear, false), &ZOOMS) else {
    return;
  };
  let Some(objects) = frames_of(pixel_art(), striped(Filter::Linear, true), &ZOOMS) else {
    return;
  };
  let Some(default) = frames_of(pixel_art(), striped(Filter::Default, false), &ZOOMS) else {
    return;
  };
  let Some(nearest) = frames_of(pixel_art(), striped(Filter::Nearest, false), &ZOOMS) else {
    return;
  };
  let Some(kept) = frames_of(smooth(), striped(Filter::Nearest, false), &ZOOMS) else {
    return;
  };
  const BLACK: [u8; 4] = [0, 0, 0, 255];
  let mut failures = Vec::new();
  let cases = [
    ("linear layer, pixel art", &linear, true),
    ("linear tile objects, pixel art", &objects, true),
    ("layer asking for nothing, pixel art", &default, false),
    ("nearest layer, smooth target", &kept, false),
  ];
  for (name, frames, smoothed) in cases {
    for (at, frame) in frames.iter() {
      let pixels: Vec<[u8; 4]> = frame.pixels().map(|p| p.0).collect();
      if let Some(p) = pixels.iter().find(|p| shows_a_neighbour(**p)) {
        failures.push(format!("{name}, {at}: a neighbour's colour {p:?}"));
      }
      let mixes = pixels
        .iter()
        .filter(|p| ![A, B, BLACK].contains(*p))
        .count();
      if smoothed && mixes < pixels.len() / 4 {
        failures.push(format!("{name}, {at}: only {mixes} pixels mix A and B"));
      }
      if !smoothed && mixes > 0 {
        failures.push(format!("{name}, {at}: {mixes} pixels are neither A nor B"));
      }
      let solid = pixels.iter().filter(|p| **p == A).count();
      if solid < 100 {
        failures.push(format!("{name}, {at}: only {solid} pixels of A"));
      }
    }
  }
  for ((at, asked), (_, near)) in default.iter().zip(&nearest) {
    if asked != near {
      failures.push(format!(
        "at {at}, the layer asking for nothing differs from a nearest one"
      ));
    }
  }
  assert!(failures.is_empty(), "sampling:\n{}", failures.join("\n"));
}
