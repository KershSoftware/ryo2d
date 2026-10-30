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

/// A tileset of solid tiles in a row: `colours`, then `more` tiles of the
/// neighbours' colours in turn
fn row(ctx: &mut SceneContext, colours: [[u8; 4]; 2], more: u32) -> Tileset {
  let count = 2 + more;
  let colour = |tile: u32| match tile {
    0 | 1 => colours[tile as usize],
    _ => NEIGHBOURS[(tile % 2) as usize],
  };
  let (width, height) = (TILE * count, TILE);
  let data: Vec<u8> = (0..width * height)
    .flat_map(|i| colour((i % width) / TILE))
    .collect();
  let image = ctx.assets().add(Image::from_rgba(data, width, height));
  let size = UVec2::new(width, height);
  let mut set = Tileset::new(
    "row",
    UVec2::splat(TILE),
    TilesetImage::Atlas { image, size },
  );
  set.columns = count;
  set.tile_count = count;
  set
}

/// A 128 x 128 map (4 x 4 chunks of 512 px) filled with a checkerboard of
/// A (the first tileset's first tile, next to a white one) and B (the
/// second tileset's second, next to a green one), its centre on the world's
/// origin, where four chunks meet. Each tileset has `more` tiles after
/// those two
struct Filled {
  more: u32,
}

impl Scene for Filled {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    ctx.look().tonemap = Tonemap::Clamp;
    let first = row(ctx, [A, NEIGHBOURS[0]], self.more);
    let second = row(ctx, [NEIGHBOURS[1], B], self.more);
    let cells = 128;
    let mut map = TileMap::new(UVec2::splat(cells), UVec2::splat(TILE));
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
    let half = (cells * TILE / 2) as f32;
    ctx
      .world
      .spawn((MapRoot::new(map), Transform::from_xy(-half, -half)));
  }
}

/// Every camera zoom and quarter-pixel offset over `scene`: the ones where
/// a pixel was neither A nor B, with the first such pixel. `own` says
/// whether the padded tilesets have textures of their own (with every mip
/// level) rather than places in the atlas (with four)
fn seams(builder: EngineBuilder, scene: Filled, own: bool) -> Option<Vec<String>> {
  let mut engine = HeadlessEngine::new_or_skip(builder.start_scene(scene))?;
  engine.step_frames(2).unwrap();
  let gpu = engine.engine.sprite_gpu.as_ref().expect("renderer");
  assert_eq!(
    gpu.standalone_entity_count(),
    if own { 2 } else { 0 },
    "where the padded tilesets are"
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
  // levels, or every level of a texture of its own (98 more 16 px tiles
  // pad into 32 px cells, ten a row: 368 px with the margins, over the
  // atlas's 256)
  let pixel_art = || EngineBuilder::new().pixel_art(SIZE, SIZE);
  let linear = || EngineBuilder::new().resolution(SIZE, SIZE);
  let targets = [
    ("nearest", pixel_art(), 0, false),
    ("linear", linear(), 0, false),
    ("linear, own textures", linear(), 98, true),
  ];
  let mut failures = Vec::new();
  for (filter, builder, more, own) in targets {
    let Some(found) = seams(builder, Filled { more }, own) else {
      return;
    };
    failures.extend(found.into_iter().map(|f| format!("{filter}: {f}")));
  }
  assert!(failures.is_empty(), "seams:\n{}", failures.join("\n"));
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

#[test]
fn a_sub_pixel_camera_draws_tiles_as_at_the_whole_pixel_they_round_to() {
  // Pixel art at zoom 4/3: a 6 px tile is 8 pixels and a texel 4/3 of one,
  // so which texel a pixel shows hangs on where the layer starts to a
  // fraction of a pixel. Its origin snaps to a whole pixel: with the map's
  // corner a fraction of a pixel from pixel (10, 10) the frame is the one
  // drawn with the corner on it. (Without the snap the texels slide under
  // the pixels as the camera moves)
  let zoom = 4.0 / 3.0;
  let builder = EngineBuilder::new()
    .pixel_art(SIZE, SIZE)
    .start_scene(Checked);
  let Some(mut engine) = HeadlessEngine::new_or_skip(builder) else {
    return;
  };
  // The camera centre that puts the map's corner at `corner` on screen
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
  assert!(found.is_empty(), "the tiles slid:\n{}", found.join("\n"));
}
