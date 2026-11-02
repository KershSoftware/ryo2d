//! Tiles drawn edge to edge never show a seam: at any zoom, at any sub-pixel
//! camera position and with either filter, every pixel over a filled tile
//! layer is one of its tiles' colours, never the background, a mix of two
//! tiles, or a colour from a tile next to them in the tileset images. And on
//! the pixel-art target a layer's origin snaps to a whole pixel, so a camera
//! a fraction of a pixel off draws what it draws there.

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
fn row(ctx: &mut SceneContext, colours: [[u8; 4]; 2], more: u32, side: u32) -> Tileset {
  let count = 2 + more;
  let colour = |tile: u32| match tile {
    0 | 1 => colours[tile as usize],
    _ => NEIGHBOURS[(tile % 2) as usize],
  };
  let (width, height) = (side * count, side);
  let data: Vec<u8> = (0..width * height)
    .flat_map(|i| colour((i % width) / side))
    .collect();
  let image = ctx.assets().add(Image::from_rgba(data, width, height));
  let size = UVec2::new(width, height);
  let mut set = Tileset::new(
    "row",
    UVec2::splat(side),
    TilesetImage::Atlas { image, size },
  );
  set.columns = count;
  set.tile_count = count;
  set
}

/// A 128 x 128 map of `tile` px cells (4 x 4 chunks) filled with a
/// checkerboard of A (the first tileset's first tile, next to a white one)
/// and B (the second tileset's second, next to a green one), its centre on
/// the world's origin, where four chunks meet. Each tileset has `more`
/// tiles after those two
struct Filled {
  more: u32,
  tile: u32,
}

impl Scene for Filled {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    ctx.look().tonemap = Tonemap::Clamp;
    let first = row(ctx, [A, NEIGHBOURS[0]], self.more, self.tile);
    let second = row(ctx, [NEIGHBOURS[1], B], self.more, self.tile);
    let cells = 128;
    let mut map = TileMap::new(UVec2::splat(cells), UVec2::splat(self.tile));
    map.add_tileset(TilesetSource::Embedded(Arc::new(first)));
    map.add_tileset(TilesetSource::Embedded(Arc::new(second)));
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
    let half = (cells * self.tile / 2) as f32;
    ctx
      .world
      .spawn((MapRoot::new(map), Transform::from_xy(-half, -half)));
  }
}

/// Every camera zoom and quarter-pixel offset over `scene`: the ones where
/// a pixel was neither A nor B, with the first such pixel. `own` says
/// whether the scene's images have textures of their own (with every mip
/// level) rather than places in the atlas (with four)
fn seams(builder: EngineBuilder, scene: impl Scene + 'static, own: usize) -> Option<Vec<String>> {
  let mut engine = HeadlessEngine::new_or_skip(builder.start_scene(scene))?;
  crate::core::tilemap::test_util::step_until_maps_land(&mut engine);
  engine.step_frames(2).unwrap();
  let gpu = engine.engine.sprite_gpu.as_ref().expect("renderer");
  assert_eq!(
    gpu.standalone_entity_count(),
    own,
    "how many images have textures of their own"
  );
  let mut found = Vec::new();
  for scale in SCALES {
    for (qx, qy) in (0..4).flat_map(|x| (0..4).map(move |y| (x, y))) {
      // Quarter pixels on screen, in world units
      let offset = Vec2::new(qx as f32, qy as f32) * 0.25 / scale;
      let world = engine.world_mut();
      let mut cameras = world.query_filtered::<(&mut Transform, &mut Camera2d), With<MainCamera>>();
      for (mut transform, mut camera) in cameras.iter_mut(world) {
        transform.translation = offset;
        camera.zoom = scale;
      }
      // Twice: the main camera is interpolated, and draws where it was
      engine.step_frames(2).unwrap();
      let frame = engine.frame().unwrap();
      let wrong: Vec<(u32, u32, [u8; 4])> = frame
        .enumerate_pixels()
        .filter(|(_, _, p)| p.0 != A && p.0 != B)
        .map(|(x, y, p)| (x, y, p.0))
        .collect();
      if let Some(&(x, y, p)) = wrong.first() {
        found.push(format!(
          "zoom {scale}, offset ({qx}/4, {qy}/4) px: {} pixels, first ({x}, {y}) {p:?}",
          wrong.len()
        ));
      }
    }
  }
  Some(found)
}

#[test]
fn no_seams_at_any_zoom_or_subpixel_camera() {
  // Pixel art samples nearest and snaps; a resolution-independent target
  // samples linearly, through the mips when zoomed out: the atlas's four
  // levels, or every level of a texture of its own (98 more tiles make the
  // padded images over the atlas's 256 px, and 254 more for 14 px tiles).
  // Tiles of 14, 15 and 30 px sit just under a power of two, where a cell
  // only two texels wider than its tile would let the mips reach a
  // neighbour
  let pixel_art = || EngineBuilder::new().pixel_art(SIZE, SIZE);
  let linear = || EngineBuilder::new().resolution(SIZE, SIZE);
  let targets = [
    ("nearest", pixel_art(), TILE, 0),
    ("linear", linear(), TILE, 0),
    ("linear, own textures", linear(), TILE, 98),
    ("linear, 15 px", linear(), 15, 0),
    ("linear, 15 px, own textures", linear(), 15, 98),
    ("linear, 30 px", linear(), 30, 0),
    ("linear, 30 px, own textures", linear(), 30, 98),
    ("linear, 14 px", linear(), 14, 0),
    ("linear, 14 px, own textures", linear(), 14, 254),
  ];
  let mut failures = Vec::new();
  for (filter, builder, tile, more) in targets {
    let own = if more > 0 { 2 } else { 0 };
    let Some(found) = seams(builder, Filled { more, tile }, own) else {
      return;
    };
    failures.extend(found.into_iter().map(|f| format!("{filter}: {f}")));
  }
  assert!(failures.is_empty(), "seams:\n{}", failures.join("\n"));
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
  let Some(found) = seams(builder, Repeated, 0) else {
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

/// Where Tiled's renderer puts a tile whose bottom-left the grid places at
/// `anchor`: an isometric grid of odd sizes places tiles on half pixels,
/// which it rounds up, and its odd cell heights put a tile a pixel lower
/// where its bottom is a whole pixel (measured; the formats note has it);
/// every other grid places them on whole pixels
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
