//! The ground the pages' streams generate, a chunk at a time: pure
//! functions of the chunk and a seed, run on the engine's chunk builder
//! thread, so the same chunk always comes out the same. Heights and caves
//! are `ryo2d-procgen`'s noise (`generate_heightmap`, its offset set to the
//! chunk's own cells, so chunks side by side meet); the ground's tiles are
//! picked by the rule the meadow was laid by and the paint page paints by
//! (`ground_tile`); and what stands on the ground (sprouts, pines,
//! mushrooms, bushes and trees) and the clouds are placed by a hash of the
//! column.
//!
//! Two worlds: the meadow's, a strip of ground going on past both ends of
//! the hand-made meadow at its grass row and down to its bottom row, its
//! hills rising gently from the seam; and the endless page's, rolling hills
//! of earth going down without end, with caves below.

use demo_kit::prelude::*;
use ryo2d_procgen::generate_heightmap;
use ryo2d_procgen::prelude::*;

use crate::shared::ground_tile;

/// The layers a generator fills: the earth behind the ground, seen where
/// it is dug or hollow (drawn dark, not colliding); what stands on the
/// ground (not colliding); and the ground
pub(crate) const BACKDROP: &str = "backdrop";
pub(crate) const SCENERY: &str = "scenery";
pub(crate) const GROUND: &str = "ground";

/// A chunk's side in cells
const SIDE: i32 = CHUNK_SIZE as i32;
/// The columns either side of a chunk a block reads: a tree's canopy
/// reaches a column past its trunk, and a tile looks one cell further
const MARGIN: i32 = 2;
/// A block's columns and rows: the chunk's, with the margin either side
/// and a row above and below
const WIDTH: i32 = SIDE + 2 * MARGIN;
const HEIGHT: i32 = SIDE + 2;
/// The row the noise is read along for heights: off the noise's lattice,
/// where every octave would be zero
const HEIGHT_ROW: f64 = 11.3;

/// Pixel Platformer's earth, closed on every side: the backdrop's tile
pub(crate) const EARTH: u32 = 122;
/// Pixel Platformer's scenery tiles (`tilesets/platformer.tsx`)
const PINE: u32 = 126;
const SPROUTS: [u32; 2] = [124, 125];
const MUSHROOM: u32 = 128;
const BUSH: u32 = 16;
/// A tree: its canopy, three by three, over a trunk three tall
const CANOPY: [[u32; 3]; 3] = [[17, 18, 19], [37, 38, 39], [57, 58, 59]];
const TRUNK: [u32; 3] = [97, 117, 137];
/// A cloud: its left end, middle and right end, and one cell alone
const CLOUD: [u32; 3] = [153, 154, 155];
const CLOUDLET: u32 = 156;

/// Whether tile `id` stands on the ground (and goes when the ground under
/// it is dug): not a cloud
pub(crate) fn standing(id: u32) -> bool {
  matches!(id, PINE | MUSHROOM | BUSH) || SPROUTS.contains(&id) || TRUNK.contains(&id)
}

/// A number in [0, 1) from `n` (a column, a slot), a salt and a seed, the
/// same every run
fn hash(n: i32, salt: u32, seed: u32) -> f32 {
  let mut h = (n as u32).wrapping_mul(0x9E37_79B9)
    ^ salt.wrapping_mul(0x85EB_CA6B)
    ^ seed.wrapping_mul(0xC2B2_AE35);
  h ^= h >> 15;
  h = h.wrapping_mul(0x2C1B_3C6D);
  h ^= h >> 12;
  h = h.wrapping_mul(0x297A_2D39);
  h ^= h >> 15;
  (h >> 8) as f32 / (1u32 << 24) as f32
}

/// `count` samples of procgen's noise along the columns from `from`, each 0
/// to 1: a heightmap one row tall, offset to the columns
fn heights(noise: &NoiseConfig, seed: u64, from: i32, count: u32) -> Vec<f32> {
  let noise = noise.clone().with_offset(f64::from(from), HEIGHT_ROW);
  generate_heightmap(count, 1, &noise, &mut SeededRng::new(seed))
}

/// Procgen's noise over the cells from `first`, `size` of them, row by
/// row, each 0 to 1
fn field(noise: &NoiseConfig, seed: u64, first: IVec2, size: IVec2) -> Vec<f32> {
  let noise = noise
    .clone()
    .with_offset(f64::from(first.x), f64::from(first.y));
  generate_heightmap(
    size.x as u32,
    size.y as u32,
    &noise,
    &mut SeededRng::new(seed),
  )
}

/// 0 at 0, 1 at 1, easing in and out
fn smooth(t: f32) -> f32 {
  let t = t.clamp(0.0, 1.0);
  t * t * (3.0 - 2.0 * t)
}

/// The cells round one chunk, worked out before its tiles are picked: each
/// column's top row (its first ground cell from above), and which cells are
/// ground, a margin wide round the chunk so the tiles at its edges see
/// their neighbours
pub(crate) struct Block {
  /// The block's first cell: the chunk's, less the margin
  first: IVec2,
  tops: Vec<i32>,
  solid: Vec<bool>,
}

impl Block {
  /// The block round the chunk whose first cell is `chunk_first`, its
  /// columns' tops in `tops` (`WIDTH` of them, from the block's first
  /// column), a cell ground where `solid(cell, top, index)` says (`index`
  /// the cell's in a row-by-row walk of the block)
  fn new(
    chunk_first: IVec2,
    tops: Vec<i32>,
    mut solid: impl FnMut(IVec2, i32, usize) -> bool,
  ) -> Block {
    let first = chunk_first - IVec2::new(MARGIN, 1);
    let mut cells = Vec::with_capacity((WIDTH * HEIGHT) as usize);
    for y in 0..HEIGHT {
      for x in 0..WIDTH {
        let index = cells.len();
        let top = tops.get(x as usize).copied().unwrap_or(i32::MAX);
        cells.push(solid(first + IVec2::new(x, y), top, index));
      }
    }
    Block {
      first,
      tops,
      solid: cells,
    }
  }

  /// Whether `cell` is ground; false outside the block
  fn ground(&self, cell: IVec2) -> bool {
    let at = cell - self.first;
    if at.x < 0 || at.y < 0 || at.x >= WIDTH || at.y >= HEIGHT {
      return false;
    }
    self.solid[(at.y * WIDTH + at.x) as usize]
  }

  /// The top row of column `x`, within the block's columns
  fn top(&self, x: i32) -> Option<i32> {
    let at = x - self.first.x;
    (0..WIDTH).contains(&at).then(|| self.tops[at as usize])
  }
}

/// What a world stands on its ground and puts in its sky
#[derive(Debug, Clone, Copy)]
pub(crate) struct Scenery {
  pub(crate) seed: u32,
  /// Whether trees grow (three columns wide, seven cells tall)
  pub(crate) trees: bool,
  /// The rows clouds float at, the highest first
  pub(crate) clouds: (i32, i32),
  /// Columns left alone, both ends included: nothing stands there and no
  /// tree or cloud reaches into them
  pub(crate) quiet: Option<(i32, i32)>,
}

/// Columns a slot of trees or clouds spans: one of each at most a slot
const TREE_SLOT: i32 = 13;
const CLOUD_SLOT: i32 = 19;

impl Scenery {
  fn quiet(&self, from: i32, to: i32) -> bool {
    self.quiet.is_some_and(|(a, b)| from <= b && to >= a)
  }

  /// The tree rooted in slot `slot`, if one is: its trunk's column
  fn tree_in(&self, slot: i32) -> Option<i32> {
    let present = self.trees && hash(slot, 1, self.seed) < 0.5;
    let root = slot * TREE_SLOT + 2 + (hash(slot, 2, self.seed) * 9.0) as i32;
    (present && !self.quiet(root - 1, root + 1)).then_some(root)
  }

  /// Stands things on the ground of the chunk from `first` and floats
  /// clouds over it, into `tiles`, from `block`
  fn place(&self, first: IVec2, block: &Block, tileset: u16, tiles: &mut ChunkTiles) {
    let mut put = |cell: IVec2, id: u32| {
      let at = cell - first;
      if at.x >= 0 && at.y >= 0 && at.x < SIDE && at.y < SIDE {
        tiles.set(
          SCENERY,
          UVec2::new(at.x as u32, at.y as u32),
          Some(Tile::new(tileset, id)),
        );
      }
    };
    // Trees, a slot of columns each at most, and what stands between them
    let mut trunks = [i32::MIN; 4];
    let slots = (first.x - 1).div_euclid(TREE_SLOT)..=(first.x + SIDE).div_euclid(TREE_SLOT);
    for (n, slot) in slots.enumerate() {
      let Some(root) = self.tree_in(slot) else {
        continue;
      };
      let (Some(top), Some(left), Some(right)) =
        (block.top(root), block.top(root - 1), block.top(root + 1))
      else {
        continue;
      };
      // On level ground only, so the canopy clears its neighbours
      if (left - top).abs() > 1 || (right - top).abs() > 1 || !block.ground(IVec2::new(root, top)) {
        continue;
      }
      if let Some(slot) = trunks.get_mut(n) {
        *slot = root;
      }
      for (r, row) in CANOPY.iter().enumerate() {
        for (c, id) in row.iter().enumerate() {
          put(IVec2::new(root - 1 + c as i32, top - 6 + r as i32), *id);
        }
      }
      for (r, id) in TRUNK.iter().enumerate() {
        put(IVec2::new(root, top - 3 + r as i32), *id);
      }
    }
    for x in first.x..first.x + SIDE {
      let Some(top) = block.top(x) else {
        continue;
      };
      if trunks.contains(&x) || self.quiet(x, x) || !block.ground(IVec2::new(x, top)) {
        continue;
      }
      let roll = hash(x, 3, self.seed);
      let id = match roll {
        r if r < 0.03 => PINE,
        r if r < 0.045 => MUSHROOM,
        r if r < 0.06 => BUSH,
        r if r < 0.2 => SPROUTS[usize::from(r < 0.13)],
        _ => continue,
      };
      put(IVec2::new(x, top - 1), id);
    }
    // Clouds, a slot of columns each at most
    let (high, low) = self.clouds;
    let slots = first.x.div_euclid(CLOUD_SLOT) - 1..=(first.x + SIDE).div_euclid(CLOUD_SLOT);
    for slot in slots {
      if hash(slot, 5, self.seed) >= 0.6 {
        continue;
      }
      let start = slot * CLOUD_SLOT + (hash(slot, 6, self.seed) * 8.0) as i32;
      let middle = (hash(slot, 7, self.seed) * 4.0) as i32;
      let span = (low - high + 1).max(1) as f32;
      let row = high + (hash(slot, 8, self.seed) * span) as i32;
      let end = if middle == 0 {
        start
      } else {
        start + middle + 1
      };
      if self.quiet(start, end) {
        continue;
      }
      if middle == 0 {
        put(IVec2::new(start, row), CLOUDLET);
        continue;
      }
      put(IVec2::new(start, row), CLOUD[0]);
      for m in 1..=middle {
        put(IVec2::new(start + m, row), CLOUD[1]);
      }
      put(IVec2::new(end, row), CLOUD[2]);
    }
  }
}

/// The chunk from `first` of `block`'s world: the ground where the block
/// has it, each tile picked by which cells beside it are ground, the
/// scenery on top, and with `backdrop` the earth behind every cell under
/// the grass
fn fill(
  first: IVec2,
  block: &Block,
  tileset: u16,
  scenery: &Scenery,
  backdrop: bool,
) -> ChunkTiles {
  let mut tiles = ChunkTiles::default();
  for y in 0..SIDE {
    for x in 0..SIDE {
      let cell = first + IVec2::new(x, y);
      let at = UVec2::new(x as u32, y as u32);
      if backdrop && block.top(cell.x).is_some_and(|top| cell.y > top) {
        tiles.set(BACKDROP, at, Some(Tile::new(tileset, EARTH)));
      }
      if !block.ground(cell) {
        continue;
      }
      let id = ground_tile(
        block.ground(cell + IVec2::NEG_Y),
        block.ground(cell + IVec2::Y),
        block.ground(cell + IVec2::NEG_X),
        block.ground(cell + IVec2::X),
      );
      tiles.set(GROUND, at, Some(Tile::new(tileset, id)));
    }
  }
  scenery.place(first, block, tileset, &mut tiles);
  tiles
}

/// The meadow's world: the hand-made meadow's ground going on past both
/// its ends, at the grass row of each end and down to the meadow's bottom
/// row, its hills rising from the seam over a few columns as the noise
/// takes over, with the meadow's scenery
#[derive(Debug, Clone)]
pub(crate) struct MeadowWorld {
  /// The map's index of the Pixel Platformer tileset
  pub(crate) tileset: u16,
  /// The meadow's first and last columns, each with its grass row
  pub(crate) left: IVec2,
  pub(crate) right: IVec2,
  /// The meadow's bottom row, the world's
  pub(crate) floor: i32,
}

/// The meadow world's seed, and how many columns its hills take to rise
/// from the seam
const MEADOW_SEED: u64 = 2026;
const RAMP: f32 = 18.0;
/// Columns either side of the meadow its scenery leaves alone
const QUIET: i32 = 3;

impl MeadowWorld {
  /// Gentle hills: about a tenth of a column's height a column
  fn noise() -> NoiseConfig {
    NoiseConfig::new()
      .with_scale(0.045)
      .with_octaves(3)
      .with_persistence(0.45)
  }

  /// Column `x`'s grass row, `noise` its sample: the meadow's own at its
  /// ends (and across it: only its end columns are read, by the tiles
  /// beside them), the noise's rising away from them
  fn top(&self, x: i32, noise: f32) -> i32 {
    let (seam, beyond) = if x < self.left.x {
      (self.left.y, self.left.x - x)
    } else if x > self.right.x {
      (self.right.y, x - self.right.x)
    } else if x - self.left.x <= self.right.x - x {
      return self.left.y;
    } else {
      return self.right.y;
    };
    // Rows up from the seam's: -2 to 4
    let lift = ((noise - 0.4) * 9.0).clamp(-2.0, 4.0);
    let ramp = smooth((beyond - 2) as f32 / RAMP);
    (seam - (lift * ramp).round() as i32).min(self.floor - 1)
  }

  /// The chunk `request` asks for
  pub(crate) fn chunk(&self, request: ChunkRequest) -> ChunkTiles {
    let first = request.first_cell;
    // Below the floor, or above the clouds and the tallest tree: nothing
    let scenery = self.scenery();
    if first.y > self.floor || first.y + SIDE < scenery.clouds.0 - 1 {
      return ChunkTiles::default();
    }
    let from = first.x - MARGIN;
    let tops: Vec<i32> = heights(&Self::noise(), MEADOW_SEED, from, WIDTH as u32)
      .into_iter()
      .enumerate()
      .map(|(i, noise)| self.top(from + i as i32, noise))
      .collect();
    let floor = self.floor;
    let block = Block::new(first, tops, |cell, top, _| cell.y >= top && cell.y <= floor);
    fill(first, &block, self.tileset, &scenery, false)
  }

  fn scenery(&self) -> Scenery {
    Scenery {
      seed: MEADOW_SEED as u32,
      trees: true,
      clouds: (-14, -9),
      quiet: Some((self.left.x - QUIET, self.right.x + QUIET)),
    }
  }
}

/// The endless page's world: rolling hills of earth going down without
/// end, caves opening a few cells under the grass
#[derive(Debug, Clone, Copy)]
pub(crate) struct Wilds {
  /// The map's index of the Pixel Platformer tileset
  pub(crate) tileset: u16,
}

/// The wilds' seed; how far the hills rise and fall either side of row 0;
/// how deep under the grass caves open, and how much of the earth they take
const WILDS_SEED: u64 = 77;
const HILLS: f32 = 9.0;
const CAVE_DEPTH: i32 = 6;
const CAVES: f32 = 0.36;

impl Wilds {
  fn hills() -> NoiseConfig {
    NoiseConfig::new()
      .with_scale(0.021)
      .with_octaves(4)
      .with_persistence(0.5)
  }

  fn caves() -> NoiseConfig {
    NoiseConfig::new()
      .with_scale(0.075)
      .with_octaves(2)
      .with_persistence(0.5)
  }

  fn top_of(noise: f32) -> i32 {
    ((0.5 - noise) * 2.0 * HILLS).round() as i32
  }

  /// Column `x`'s grass row, as the generator makes it
  pub(crate) fn top(&self, x: i32) -> i32 {
    heights(&Self::hills(), WILDS_SEED, x, 1)
      .first()
      .map_or(0, |noise| Self::top_of(*noise))
  }

  /// The chunk `request` asks for
  pub(crate) fn chunk(&self, request: ChunkRequest) -> ChunkTiles {
    let first = request.first_cell;
    let scenery = Scenery {
      seed: WILDS_SEED as u32,
      trees: false,
      clouds: (-26, -18),
      quiet: None,
    };
    // Above the clouds: nothing
    if first.y + SIDE < scenery.clouds.0 - 1 {
      return ChunkTiles::default();
    }
    let from = first.x - MARGIN;
    let tops: Vec<i32> = heights(&Self::hills(), WILDS_SEED, from, WIDTH as u32)
      .into_iter()
      .map(Self::top_of)
      .collect();
    let block_first = first - IVec2::new(MARGIN, 1);
    // Caves only where some cell lies deep enough for one
    let shallowest = tops.iter().copied().min().unwrap_or(0);
    let deep = block_first.y + HEIGHT > shallowest + CAVE_DEPTH;
    let caves = if deep {
      field(
        &Self::caves(),
        WILDS_SEED + 1,
        block_first,
        IVec2::new(WIDTH, HEIGHT),
      )
    } else {
      Vec::new()
    };
    let block = Block::new(first, tops, |cell, top, index| {
      let open = cell.y > top + CAVE_DEPTH && caves.get(index).is_some_and(|v| *v < CAVES);
      cell.y >= top && !open
    });
    fill(first, &block, self.tileset, &scenery, true)
  }
}
