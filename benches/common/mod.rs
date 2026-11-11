//! Shared by the benches that build the cave map: the art, the noise and the
//! cells of the cave (rock where noise is high, props and details on a
//! background), as `benches/tilemap.rs` builds it

use ryo2d::prelude::*;

/// The tileset: 16 x 16 tiles of 16 px. Rock (a full-cell collision
/// shape), the edit map's ring tile (on the hexagonal grid a shape of its
/// own: `Grid::ring`), background, props and foreground details
pub const COLUMNS: u32 = 16;
pub const ROCK: std::ops::Range<u32> = 0..8;
pub const RING: u32 = 8;
pub const BACK: std::ops::Range<u32> = 16..24;
pub const PROPS: std::ops::Range<u32> = 32..48;
pub const FRONT: std::ops::Range<u32> = 48..56;

/// A hash of a cell, for the maps' scatter and noise
pub fn hash(x: i32, y: i32, salt: u32) -> u32 {
  let mut h = (x as u32).wrapping_mul(0x8DA6_B343)
    ^ (y as u32).wrapping_mul(0xD816_3841)
    ^ salt.wrapping_mul(0xCB1A_B31F);
  h ^= h >> 13;
  h = h.wrapping_mul(0x5BD1_E995);
  h ^ (h >> 15)
}

/// Smooth value noise in 0..1, on a lattice one unit apart
pub fn noise(x: f32, y: f32, salt: u32) -> f32 {
  let (x0, y0) = (x.floor(), y.floor());
  let (fx, fy) = (x - x0, y - y0);
  let (sx, sy) = (fx * fx * (3.0 - 2.0 * fx), fy * fy * (3.0 - 2.0 * fy));
  let (ix, iy) = (x0 as i32, y0 as i32);
  let at = |dx: i32, dy: i32| (hash(ix + dx, iy + dy, salt) & 0xFFFF) as f32 / 65535.0;
  let top = at(0, 0) + (at(1, 0) - at(0, 0)) * sx;
  let bottom = at(0, 1) + (at(1, 1) - at(0, 1)) * sx;
  top + (bottom - top) * sy
}

/// The pixels of an atlas of `COLUMNS` x `COLUMNS` tiles of `tile` px (the
/// orthogonal maps' is 256 x 256), each tile its own colour with a darker
/// rim, so no two tiles look alike
pub fn atlas_pixels(tile: UVec2) -> Vec<u8> {
  let (width, height) = (COLUMNS * tile.x, COLUMNS * tile.y);
  let mut data = Vec::with_capacity((width * height * 4) as usize);
  for y in 0..height {
    for x in 0..width {
      let id = (y / tile.y) * COLUMNS + x / tile.x;
      let h = hash(id as i32, 0, 9);
      let (u, v) = (x % tile.x, y % tile.y);
      let rim = u == 0 || v == 0 || u == tile.x - 1 || v == tile.y - 1;
      let shade = |c: u32| if rim { (c / 2) as u8 } else { c as u8 };
      data.extend_from_slice(&[
        shade(h & 0xFF),
        shade((h >> 8) & 0xFF),
        shade((h >> 16) & 0xFF),
        255,
      ]);
    }
  }
  data
}

/// The tileset over `image`, `COLUMNS` x `COLUMNS` tiles of `tile` px, its
/// rock tiles carrying `rock` as their collision shape and its ring tile
/// `ring` (none without either)
pub fn tileset(
  image: Handle<Image>,
  tile: UVec2,
  rock: Option<Vec<Vec2>>,
  ring: Option<Vec<Vec2>>,
) -> Tileset {
  let mut set = Tileset::new(
    "bench",
    tile,
    TilesetImage::Atlas {
      image,
      size: tile * COLUMNS,
    },
  );
  set.columns = COLUMNS;
  set.tile_count = COLUMNS * COLUMNS;
  let shaped = rock
    .into_iter()
    .flat_map(|points| ROCK.map(move |id| (id, points.clone())))
    .chain(ring.map(|points| (RING, points)));
  for (id, points) in shaped {
    set.insert_tile(
      id,
      TileData {
        shapes: vec![TileShape {
          points,
          ..TileShape::default()
        }],
        ..TileData::default()
      },
    );
  }
  set
}

/// A tile of `range` picked by `h`
pub fn pick(range: std::ops::Range<u32>, h: u32) -> u32 {
  range.start + h % (range.end - range.start)
}

/// Whether the cell (`x`, `y`) is cave rock: where noise is high
pub fn rocky(x: i32, y: i32) -> bool {
  let (fx, fy) = (x as f32, y as f32);
  noise(fx / 22.0, fy / 22.0, 1) * 0.7 + noise(fx / 7.0, fy / 7.0, 2) * 0.3 > 0.6
}

/// The cells of a cave map `side` cells square, layer by layer (background,
/// ground, props, front), as tile ids: a background on every cell, rock
/// where noise is high, props on about one open cell in 16 and front
/// details on one in 64
pub fn cave_cells(side: u32) -> [Vec<Option<u32>>; 4] {
  let n = (side * side) as usize;
  let mut layers = [vec![None; n], vec![None; n], vec![None; n], vec![None; n]];
  for y in 0..side as i32 {
    for x in 0..side as i32 {
      let i = (y as u32 * side + x as u32) as usize;
      let h = hash(x, y, 3);
      layers[0][i] = Some(pick(BACK, h));
      if rocky(x, y) {
        layers[1][i] = Some(pick(ROCK, h));
        continue;
      }
      let scatter = hash(x, y, 4);
      if scatter.is_multiple_of(16) {
        layers[2][i] = Some(pick(PROPS, scatter / 16));
      } else if scatter % 64 == 1 {
        layers[3][i] = Some(pick(FRONT, scatter / 64));
      }
    }
  }
  layers
}
