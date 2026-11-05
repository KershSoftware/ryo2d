//! Padded tileset images: every tile in a cell of its own a little bigger
//! than itself, its edge texels extruded to fill the cell, so no read the
//! tile shader makes reaches a neighbouring tile at any zoom.
//!
//! A tile's cell is the tile rounded up on each side to a multiple of 2^K
//! texels, K the tile's clean levels ([`clean_levels`]), and starts on a
//! multiple of 2^K; the image's sides are multiples of the largest 2^K
//! among its cells, so every level up to that one halves it exactly and
//! each cell is whole texels of every level up to its own K. The tile
//! fragment stages (`sample_cell`, `shaders/include/sample.wgsl`) pick the
//! level from the UV's derivatives, cap it at the tile's K and keep each
//! read half a texel of its level inside the cell.
//!
//! The padded image is an ordinary `Image` asset of its own (held by
//! [`PaddedTilesets`]), drawn through the renderer's usual image path: one
//! over 256 px gets a texture of its own with a mip chain and no border, a
//! smaller one sits in the shared atlas (its cells' K at most 3, as its
//! pages keep four levels), where its slot can move (the draw maps UVs
//! through the slot). UVs written for it are in padded-image space, 0..1
//! over the whole image.
//!
//! Public but hidden from the docs: `tests/tilemap_alloc.rs` and
//! `benches/tilemap.rs` drive the cache from outside the crate.

use std::cmp::Reverse;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, Weak};

use glam::{IVec2, U64Vec2, UVec2};

use super::map::TilesetSource;
use super::tileset::{Tileset, TilesetImage, URect};
use crate::core::asset::{AssetId, Assets, Handle};
use crate::core::ecs::{Resource, World};
use crate::core::graphics::Image;
use crate::core::graphics::atlas;
use crate::core::hash::FastMap;

/// The most texels a padded image may span on a side under the limits the
/// engine asks its device for (`wgpu::Limits::default()`): a texture's
/// largest side
fn default_max_side() -> u32 {
  texture_side_room(wgpu::Limits::default().max_texture_dimension_2d)
}

/// The most texels a padded image may span on a side on a device whose
/// textures span `max_texture_dimension_2d`: the dimension itself, as a
/// padded image in a texture of its own has no border
fn texture_side_room(max_texture_dimension_2d: u32) -> u32 {
  max_texture_dimension_2d
}
/// The most cells one animation may take once its frames are repeated to a
/// common beat; past that it plays on a beat of the mean frame time
const MAX_ANIMATION_CELLS: u64 = 128;
/// Rounds (frames) a padding no map asked for is kept, as long as the
/// batch keeps an undrawn chunk (`chunks::KEEP_FRAMES`)
const KEEP_ROUNDS: u64 = crate::core::render::batch::KEEP_FRAMES;
/// The clean levels a tile in the shared atlas may keep: its pages have
/// four levels, the full-size one and three below
const ATLAS_LEVELS: u32 = atlas::MIP_LEVELS - 1;
/// The most widths [`arrange`] tries for a tileset of mixed cells
const MIXED_WIDTHS: u64 = 128;

/// How a tileset's tiles sit in its padded image
#[derive(Clone, Debug)]
pub struct PaddedLayout {
  /// The padded image's size in texels: no margin, so cells start at texel
  /// 0. The image is borderless (`Image::borderless`): in a texture of its
  /// own its cells start at texel 0 of the texture, and on an atlas page a
  /// slot starts on 8 texels, keeping cells whole texels of the page's four
  /// levels
  pub size: UVec2,
  /// The tileset's `tile_offset`: where every tile draws from its cell's
  /// bottom-left corner, in pixels, x right and y down
  pub offset: IVec2,
  /// The widest and the tallest of its tiles, in texels (as drawn
  /// unflipped), which may be two tiles
  pub largest: UVec2,
  /// The widest and the tallest of its cells, in texels, which may be two
  /// cells: how far past its grid cell a tile may draw, at most
  pub largest_cell: UVec2,
  /// The tiles' own texels times 4 (RGBA), each picture once: what the
  /// padded image is measured against
  pub art_bytes: u64,
  /// The tileset's tile size (Tiled's `tilewidth` and `tileheight`; an
  /// image collection's largest image, as Tiled saves it), which Tiled's
  /// raster of an isometric map of odd cells goes by
  /// (`MapGrid::raster_offset`)
  pub tile_size: UVec2,
  /// Which padding this is: [`PaddedTilesets`] gives each image it makes a
  /// number never given before (0 until then), so what was built from a
  /// layout can tell it was padded again
  pub serial: u64,
  tiles: Arc<Vec<Option<PaddedTile>>>,
}

/// Where a tile's picture sits: `size` texels from `origin`, both multiples
/// of 2^levels, so the cell is whole texels of every level up to `levels`
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PaddedCell {
  pub origin: UVec2,
  pub size: UVec2,
  /// The tile's clean levels: the shader reads no level past this one
  pub levels: u32,
}

/// Where one tile sits in the padded image
#[derive(Clone, Copy, Debug)]
pub struct PaddedTile {
  /// The tile's cell, its picture centred in it
  pub cell: PaddedCell,
  /// The tile's size in texels, as drawn unflipped
  pub size: UVec2,
  pub animation: Option<PaddedAnimation>,
  /// The tile's `sort_offset`, in pixels
  pub sort_offset: f32,
}

/// An animation's frames: equal cells (the largest frame's) row-major from
/// `first`, `columns` a row, each shown for `frame_duration` seconds
#[derive(Clone, Copy, Debug)]
pub struct PaddedAnimation {
  pub first: PaddedCell,
  pub columns: u32,
  pub frames: u32,
  pub frame_duration: f32,
}

impl PaddedAnimation {
  /// Frame `k`'s cell
  pub fn frame(&self, k: u32) -> PaddedCell {
    let columns = self.columns.max(1);
    let step = UVec2::new(k % columns, k / columns) * self.first.size;
    PaddedCell {
      origin: self.first.origin + step,
      ..self.first
    }
  }
}

impl PaddedLayout {
  /// Tile `id`'s place; `None` for an id the tileset has no picture for
  pub fn tile(&self, id: u32) -> Option<&PaddedTile> {
    self.tiles.get(id as usize)?.as_ref()
  }

  /// The padded image's bytes on the GPU with the levels its home keeps:
  /// an atlas page's four, or a texture of its own's whole chain
  pub fn gpu_bytes(&self) -> u64 {
    let UVec2 { x, y } = self.size;
    let levels = if atlas::own_texture(x, y) {
      32 - x.max(y).leading_zeros()
    } else {
      atlas::MIP_LEVELS
    };
    (0..levels)
      .map(|k| u64::from((x >> k).max(1)) * u64::from((y >> k).max(1)) * 4)
      .sum()
  }
}

/// How many levels a tile of `size` texels keeps clean: the largest k for
/// which rounding each side up to a multiple of 2^k adds at most a quarter
/// of it (and leaves a side a `u32` holds), and at most 3 in the atlas,
/// whose pages keep four levels. Rounding only grows with k, so the levels
/// below are clean too
pub(crate) fn clean_levels(size: UVec2, in_atlas: bool) -> u32 {
  let fits = |k: u32| {
    [size.x, size.y].into_iter().all(|side| {
      let side = u64::from(side);
      let cell = side.next_multiple_of(1 << k);
      4 * (cell - side) <= side && cell <= u64::from(u32::MAX)
    })
  };
  let most = if in_atlas {
    ATLAS_LEVELS
  } else {
    u32::BITS - 1
  };
  (1..=most).take_while(|&k| fits(k)).last().unwrap_or(0)
}

/// The cell of a tile of `size` texels keeping `levels` clean: each side
/// rounded up to a multiple of 2^levels (at most `u32::MAX`)
pub(crate) fn cell_for(size: UVec2, levels: u32) -> UVec2 {
  let round = |side: u32| {
    let cell = u64::from(side).next_multiple_of(1 << levels.min(u32::BITS));
    u32::try_from(cell).unwrap_or(u32::MAX)
  };
  UVec2::new(round(size.x), round(size.y))
}

/// Where padding reads a tileset's images from: the store, or copies of
/// what a tileset uses taken for a landing job's thread
pub(crate) trait Pixels {
  fn image(&self, handle: &Handle<Image>) -> Option<&Image>;
}

impl Pixels for Assets<Image> {
  fn image(&self, handle: &Handle<Image>) -> Option<&Image> {
    self.get(handle)
  }
}

impl Pixels for FastMap<AssetId<Image>, Image> {
  fn image(&self, handle: &Handle<Image>) -> Option<&Image> {
    self.get(&handle.id())
  }
}

/// Where a tile's texels come from
struct Source<'a> {
  image: &'a Image,
  rect: URect,
  /// The colour that pads clear (the tileset's transparent colour)
  key: Option<[u8; 3]>,
}

fn source<'a>(tileset: &Tileset, images: &'a impl Pixels, id: u32) -> Option<Source<'a>> {
  let key = tileset.transparent.map(|c| [c.r, c.g, c.b]);
  match &tileset.image {
    TilesetImage::Atlas { image, .. } => Some(Source {
      image: images.image(image)?,
      rect: tileset.tile_rect(id)?,
      key,
    }),
    TilesetImage::Collection => {
      let (handle, size) = tileset.tile(id)?.image.as_ref()?;
      Some(Source {
        image: images.image(handle)?,
        rect: URect::new(0, 0, size.x, size.y),
        key,
      })
    }
  }
}

/// The size tile `id` draws at: the tileset's for an atlas, its own for a
/// collection's. `None` for an empty or absent tile
fn tile_size(tileset: &Tileset, id: u32) -> Option<UVec2> {
  let size = match &tileset.image {
    TilesetImage::Atlas { .. } => tileset.tile_rect(id).map(|_| tileset.tile_size)?,
    TilesetImage::Collection => tileset.tile(id)?.image.as_ref()?.1,
  };
  (size.x > 0 && size.y > 0).then_some(size)
}

/// Frames of an animated tile to lay out: the tile id of each cell and the
/// seconds each cell is shown. `None` when the tile is not animated, or a
/// frame has no picture
fn animation_cells(tileset: &Tileset, images: &impl Pixels, id: u32) -> Option<(Vec<u32>, f32)> {
  let frames = &tileset.tile(id)?.animation;
  if frames.len() < 2
    || frames
      .iter()
      .any(|f| source(tileset, images, f.tile).is_none())
  {
    return None;
  }
  let millis: Vec<u64> = frames
    .iter()
    .map(|f| u64::try_from(f.duration.as_millis()).unwrap_or(u64::MAX))
    .collect();
  if millis.contains(&0) {
    return None;
  }
  let beat = millis.iter().copied().fold(0, gcd);
  let cells: u64 = millis.iter().map(|m| m / beat).sum();
  if cells <= MAX_ANIMATION_CELLS {
    let mut tiles = Vec::new();
    for (f, m) in frames.iter().zip(&millis) {
      tiles.extend(std::iter::repeat_n(f.tile, (m / beat) as usize));
    }
    Some((tiles, beat as f32 / 1000.0))
  } else {
    let mean = millis.iter().sum::<u64>() as f32 / millis.len() as f32;
    Some((frames.iter().map(|f| f.tile).collect(), mean / 1000.0))
  }
}

fn gcd(a: u64, b: u64) -> u64 {
  if b == 0 { a } else { gcd(b, a % b) }
}

/// Why a tileset has no padded image
#[derive(Debug)]
pub(crate) enum Unpadded {
  /// Nothing to draw yet: no tile with a picture, or its atlas image not
  /// loaded
  Nothing,
  /// Its `cells` cells would make an image of `size` texels at the best,
  /// over the most a padded image may span on a side
  TooBig { cells: u64, size: (u64, u64) },
  /// A tile of `size` texels has a cell over the most a padded image may
  /// span on a side
  TileTooBig { size: UVec2 },
}

/// One thing the padded image holds: a tile's cell, or an animation's
/// frames, each in a cell of `cell` texels starting on a multiple of
/// 2^`levels`
#[derive(Clone, Copy, Debug)]
struct Block {
  cell: UVec2,
  levels: u32,
  frames: u32,
}

impl Block {
  /// Its alignment in texels
  fn align(&self) -> u64 {
    1 << self.levels
  }

  /// Its columns and its size in texels in an image `width` wide: an
  /// animation's frames as many to a row as fit; `None` when even one cell
  /// does not
  fn shape(&self, width: u64) -> Option<(u32, U64Vec2)> {
    let cell = self.cell.as_u64vec2();
    if cell.x == 0 || cell.x > width {
      return None;
    }
    let frames = u64::from(self.frames.max(1));
    let columns = frames.min(width / cell.x);
    let rows = frames.div_ceil(columns);
    Some((columns as u32, U64Vec2::new(columns, rows) * cell))
  }
}

/// Blocks alike (still tiles of one cell), packed together: the block and
/// the indices of those like it
struct Run {
  block: Block,
  members: Vec<usize>,
}

/// `blocks` gathered into runs: still tiles of one cell together, in order,
/// each animation alone
fn runs_of(blocks: &[Block]) -> Vec<Run> {
  let mut runs: Vec<Run> = Vec::new();
  let mut still: FastMap<(UVec2, u32), usize> = FastMap::default();
  for (i, block) in blocks.iter().enumerate() {
    if block.frames > 1 {
      runs.push(Run {
        block: *block,
        members: vec![i],
      });
      continue;
    }
    let at = *still.entry((block.cell, block.levels)).or_insert_with(|| {
      runs.push(Run {
        block: *block,
        members: Vec::new(),
      });
      runs.len() - 1
    });
    runs[at].members.push(i);
  }
  runs
}

/// A row of blocks as tall as its first: its height, how far along it is
/// filled and the largest alignment on it
struct Shelf {
  height: u64,
  used: u64,
  align: u64,
}

/// `runs` of `blocks` packed `width` texels wide: tallest first (then
/// widest), each block on the first shelf with room for it, across on a
/// multiple of its alignment, each shelf down on a multiple of the largest
/// alignment on it. The image's size, its sides multiples of `align` (the
/// largest alignment of all), and with `places`, each block's top-left and
/// columns (indexed as `blocks`); `None` when a block is wider than `width`
fn pack(
  blocks: &[Block],
  runs: &[Run],
  (width, align): (u64, u64),
  places: Option<&mut Vec<(UVec2, u32)>>,
) -> Option<U64Vec2> {
  let mut order = Vec::with_capacity(runs.len());
  for (i, run) in runs.iter().enumerate() {
    let (columns, size) = run.block.shape(width)?;
    order.push((Reverse(size.y), Reverse(size.x), i, columns));
  }
  order.sort_unstable();
  // Where each block went, when asked: its shelf, how far along it and its
  // columns
  let mut spots: Vec<(usize, u64, u32)> = Vec::new();
  if places.is_some() {
    spots.resize(blocks.len(), (0, 0, 1));
  }
  let mut shelves: Vec<Shelf> = Vec::new();
  for (Reverse(h), Reverse(w), i, columns) in order {
    let run = &runs[i];
    let a = run.block.align();
    let mut members = run.members.iter();
    let mut left = run.members.len() as u64;
    let mut put = |shelf: usize, x: u64, count: u64| {
      if spots.is_empty() {
        return;
      }
      for (k, &member) in (0..count).zip(members.by_ref()) {
        if let Some(spot) = spots.get_mut(member) {
          *spot = (shelf, x + k * w, columns);
        }
      }
    };
    for (s, shelf) in shelves.iter_mut().enumerate() {
      if left == 0 {
        break;
      }
      let x = shelf.used.next_multiple_of(a);
      if shelf.height < h || x + w > width {
        continue;
      }
      let fit = ((width - x) / w).min(left);
      put(s, x, fit);
      shelf.used = x + fit * w;
      shelf.align = shelf.align.max(a);
      left -= fit;
    }
    while left > 0 {
      let fit = (width / w).min(left);
      put(shelves.len(), 0, fit);
      shelves.push(Shelf {
        height: h,
        used: fit * w,
        align: a,
      });
      left -= fit;
    }
  }
  // The shelves down the image
  let mut tops = Vec::with_capacity(shelves.len());
  let mut down = 0u64;
  for shelf in &shelves {
    down = down.next_multiple_of(shelf.align);
    tops.push(down);
    down += shelf.height;
  }
  if let Some(places) = places {
    let texel = |at: u64| u32::try_from(at).unwrap_or(u32::MAX);
    places.clear();
    places.extend(spots.iter().map(|&(shelf, x, columns)| {
      let top = tops.get(shelf).copied().unwrap_or(0);
      (UVec2::new(texel(x), texel(top)), columns)
    }));
  }
  let across = shelves.iter().map(|s| s.used).max().unwrap_or(0);
  Some(U64Vec2::new(
    across.next_multiple_of(align),
    down.next_multiple_of(align),
  ))
}

/// Where `blocks` go in the smallest image within `max_side` texels a side
/// (the squarer of two alike, then the wider): its size and each block's
/// top-left and columns. Blocks of one cell size go in a grid of the best
/// column count; mixed ones are packed at widths up to the image's longest
/// row ([`pack`]). `Unpadded::TooBig` with the size of the attempt with the
/// shortest longest side when none fits
fn arrange(blocks: &[Block], max_side: u32) -> Result<(UVec2, Vec<(UVec2, u32)>), Unpadded> {
  let runs = runs_of(blocks);
  let align = blocks.iter().map(Block::align).max().unwrap_or(1);
  let cells: u64 = blocks.iter().map(|b| u64::from(b.frames.max(1))).sum();
  let max_side = u64::from(max_side);
  let first = blocks.first().map_or(UVec2::ZERO, |b| b.cell);
  let widths: Vec<u64> = if blocks.iter().all(|b| b.cell == first) {
    // A grid: every column count up to all in one row
    let cell = u64::from(first.x).max(1);
    (1..=cells.min(max_side / cell)).map(|n| n * cell).collect()
  } else {
    // No narrower than the widest cell, no wider than every cell in a row
    let widest = blocks
      .iter()
      .map(|b| u64::from(b.cell.x))
      .max()
      .unwrap_or(1);
    let one_row: u64 = blocks
      .iter()
      .map(|b| u64::from(b.cell.x) * u64::from(b.frames.max(1)))
      .sum();
    let low = widest.next_multiple_of(align);
    let high = (max_side / align * align).min(one_row.next_multiple_of(align));
    let step = ((high.saturating_sub(low)) / MIXED_WIDTHS)
      .max(1)
      .next_multiple_of(align);
    let mut widths: Vec<u64> = (0..)
      .map(|k| low + k * step)
      .take_while(|&w| w <= high)
      .collect();
    widths.push(high.max(low));
    widths
  };
  // The smaller image first, then the squarer, then the wider
  let rank = |s: U64Vec2| (s.x * s.y, s.max_element(), Reverse(s.x));
  let mut best: Option<(U64Vec2, u64)> = None;
  let mut closest: Option<U64Vec2> = None;
  for width in widths {
    let Some(size) = pack(blocks, &runs, (width, align), None) else {
      continue;
    };
    if size.max_element() > max_side {
      if closest.is_none_or(|c| size.max_element() < c.max_element()) {
        closest = Some(size);
      }
      continue;
    }
    if best.is_none_or(|(b, _)| rank(size) < rank(b)) {
      best = Some((size, width));
    }
  }
  let Some((size, width)) = best else {
    let size = closest.unwrap_or(U64Vec2::splat(u64::MAX));
    return Err(Unpadded::TooBig {
      cells,
      size: (size.x, size.y),
    });
  };
  let mut places = Vec::new();
  pack(blocks, &runs, (width, align), Some(&mut places));
  Ok((size.as_uvec2(), places))
}

/// A tileset's layout and what each cell shows: the tile painted there and
/// the size it is painted at
pub(crate) struct Laid {
  pub(crate) layout: PaddedLayout,
  paints: Vec<(u32, PaddedCell, UVec2)>,
}

/// An animated tile: its id and size, the tile of each of its frames, the
/// seconds each shows and the largest of its frames (and of itself)
struct Animated {
  id: u32,
  size: UVec2,
  frames: Vec<u32>,
  frame_duration: f32,
  largest: UVec2,
}

/// The blocks of `tiles` (each tile with a picture, and its size) and
/// `animations`, keeping clean levels for a texture of its own or, with
/// `in_atlas`, for an atlas page: still tiles first, in order, then each
/// animation. `Unpadded::TileTooBig` for a cell over `max_side`
fn blocks_of(
  tiles: &[(u32, UVec2)],
  animations: &[Animated],
  in_atlas: bool,
  max_side: u32,
) -> Result<Vec<Block>, Unpadded> {
  let still = tiles.iter().map(|&(_, size)| (size, 1));
  let moving = animations
    .iter()
    .map(|a| (a.largest, a.frames.len() as u32));
  still
    .chain(moving)
    .map(|(size, frames)| {
      let levels = clean_levels(size, in_atlas);
      let cell = cell_for(size, levels);
      if cell.max_element() > max_side {
        return Err(Unpadded::TileTooBig { size });
      }
      Ok(Block {
        cell,
        levels,
        frames,
      })
    })
    .collect()
}

/// Lays out `tileset`'s tiles; `Unpadded::Nothing` when it has nothing to
/// draw yet (no tile with a picture, or its atlas image not loaded), and
/// `Unpadded::TooBig` or `TileTooBig` when its padded image would be over
/// `max_side` texels on a side. Laid out first as a texture of its own; an
/// image that comes out small enough for the atlas is laid out again with
/// the atlas's levels, which only shrinks its cells
pub(crate) fn lay_out(
  tileset: &Tileset,
  images: &impl Pixels,
  max_side: u32,
) -> Result<Laid, Unpadded> {
  // Every tile that has a picture, by id
  let ids: Vec<u32> = match &tileset.image {
    TilesetImage::Atlas { image, .. } => {
      images.image(image).ok_or(Unpadded::Nothing)?;
      (0..tileset.tile_count)
        .filter(|&id| tileset.tile_rect(id).is_some())
        .collect()
    }
    TilesetImage::Collection => {
      let mut ids: Vec<u32> = tileset
        .tiles()
        .filter(|(_, d)| d.image.is_some())
        .map(|(id, _)| id)
        .collect();
      ids.sort_unstable();
      ids
    }
  };
  let tiles: Vec<(u32, UVec2)> = ids
    .into_iter()
    .filter(|&id| source(tileset, images, id).is_some())
    .filter_map(|id| Some((id, tile_size(tileset, id)?)))
    .collect();
  if tiles.is_empty() {
    return Err(Unpadded::Nothing);
  }
  let largest = tiles
    .iter()
    .fold(UVec2::ZERO, |largest, (_, size)| largest.max(*size));
  let animations: Vec<Animated> = tiles
    .iter()
    .filter_map(|&(id, size)| {
      let (frames, frame_duration) = animation_cells(tileset, images, id)?;
      let largest = frames
        .iter()
        .filter_map(|&frame| tile_size(tileset, frame))
        .fold(size, UVec2::max);
      Some(Animated {
        id,
        size,
        frames,
        frame_duration,
        largest,
      })
    })
    .collect();

  let mut blocks = blocks_of(&tiles, &animations, false, max_side)?;
  let (mut size, mut places) = arrange(&blocks, max_side)?;
  if !atlas::own_texture(size.x, size.y) {
    let small = blocks_of(&tiles, &animations, true, max_side)?;
    let atlas_side = atlas::MAX_ATLAS_IMAGE.min(max_side);
    (size, places) = arrange(&small, atlas_side).or_else(|_| arrange(&small, max_side))?;
    blocks = small;
  }

  let cell = |i: usize| PaddedCell {
    origin: places[i].0,
    size: blocks[i].cell,
    levels: blocks[i].levels,
  };
  let mut slots: Vec<Option<PaddedTile>> = Vec::new();
  let mut paints = Vec::with_capacity(blocks.len());
  for (i, &(id, size)) in tiles.iter().enumerate() {
    if slots.len() <= id as usize {
      slots.resize(id as usize + 1, None);
    }
    slots[id as usize] = Some(PaddedTile {
      cell: cell(i),
      size,
      animation: None,
      sort_offset: tileset.tile(id).map_or(0.0, |data| data.sort_offset),
    });
    paints.push((id, cell(i), size));
  }
  for (j, animated) in animations.iter().enumerate() {
    let i = tiles.len() + j;
    let animation = PaddedAnimation {
      first: cell(i),
      columns: places[i].1,
      frames: animated.frames.len() as u32,
      frame_duration: animated.frame_duration,
    };
    // Every frame drawn at the tile's own size, as the draw shows it
    for (k, &frame) in animated.frames.iter().enumerate() {
      paints.push((frame, animation.frame(k as u32), animated.size));
    }
    if let Some(Some(tile)) = slots.get_mut(animated.id as usize) {
      tile.animation = Some(animation);
    }
  }
  let layout = PaddedLayout {
    size,
    offset: tileset.tile_offset,
    largest,
    largest_cell: blocks.iter().fold(UVec2::ZERO, |l, b| l.max(b.cell)),
    art_bytes: tiles
      .iter()
      .map(|(_, s)| u64::from(s.x) * u64::from(s.y) * 4)
      .sum(),
    tile_size: tileset.tile_size,
    serial: 0,
    tiles: Arc::new(slots),
  };
  Ok(Laid { layout, paints })
}

/// Pads `tileset`'s tiles ([`lay_out`], then each cell painted); nothing is
/// made when it cannot be laid out
pub(crate) fn pad_tileset(
  tileset: &Tileset,
  images: &impl Pixels,
  max_side: u32,
) -> Result<(Image, PaddedLayout), Unpadded> {
  let Laid { layout, paints } = lay_out(tileset, images, max_side)?;
  let size = layout.size;
  let mut data = vec![0u8; size.x as usize * size.y as usize * 4];
  for (id, cell, drawn) in paints {
    if let Some(from) = source(tileset, images, id) {
      paint(&mut data, size.x, cell, drawn, &from);
    }
  }
  Ok((Image::from_rgba(data, size.x, size.y).borderless(), layout))
}

/// Copies a tile of `drawn` texels into `cell`, centred, its edge texels
/// extruded over the rest of the cell. Texels the source lacks, and texels
/// of the tileset's transparent colour, are transparent
fn paint(data: &mut [u8], stride: u32, cell: PaddedCell, drawn: UVec2, from: &Source) {
  let (origin, side) = (cell.origin, cell.size);
  let offset = side.saturating_sub(drawn) / 2;
  let (src_w, src_h) = (from.image.width(), from.image.height());
  let pixels = from.image.data();
  for y in 0..side.y {
    let sy = y.saturating_sub(offset.y).min(drawn.y - 1) + from.rect.min.y;
    for x in 0..side.x {
      let sx = x.saturating_sub(offset.x).min(drawn.x - 1) + from.rect.min.x;
      if sx >= src_w || sy >= src_h {
        continue;
      }
      let at = (sy as usize * src_w as usize + sx as usize) * 4;
      let to = ((origin.y + y) as usize * stride as usize + (origin.x + x) as usize) * 4;
      if from.key.is_some_and(|key| pixels[at..at + 3] == key) {
        continue;
      }
      data[to..to + 4].copy_from_slice(&pixels[at..at + 4]);
    }
  }
}

/// What a kept padding takes: its tileset's name, the padded image's bytes
/// on the GPU ([`PaddedLayout::gpu_bytes`]) and its tiles' own
/// ([`PaddedLayout::art_bytes`])
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PaddedSize {
  pub name: String,
  pub bytes: u64,
  pub art_bytes: u64,
}

/// Each padding the world keeps, by its tileset's name (tests, the demo's
/// among them)
pub fn padded_sizes(world: &World) -> Vec<PaddedSize> {
  let Some(cache) = world.get_resource::<PaddedCache>() else {
    return Vec::new();
  };
  let padded = cache.lock();
  let mut sizes: Vec<PaddedSize> = padded
    .entries
    .values()
    .map(|entry| PaddedSize {
      name: entry.name.clone(),
      bytes: entry.layout.gpu_bytes(),
      art_bytes: entry.layout.art_bytes,
    })
    .collect();
  sizes.sort_by(|a, b| a.name.cmp(&b.name));
  sizes
}

/// Which tileset a padded image is of
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Key {
  Asset(AssetId<Tileset>),
  /// An embedded tileset, by the address of its `Arc`
  Embedded(usize),
}

struct Entry {
  /// Versions of everything the padded image was made from
  stamp: u64,
  /// The tileset's name
  name: String,
  handle: Handle<Image>,
  layout: PaddedLayout,
  /// The round it was last asked for in
  asked: u64,
  /// An embedded tileset's allocation, held so no other tileset is made at
  /// its address (its key) while the padding is kept
  _holds: Option<Weak<Tileset>>,
}

/// A tileset too big to pad, as it was when refused: not tried again (or
/// warned about again) until it changes
struct Refused {
  stamp: u64,
  /// The round it was last asked for in
  asked: u64,
  /// As [`Entry`]'s
  _holds: Option<Weak<Tileset>>,
}

/// The next [`PaddedLayout::serial`]
static SERIALS: AtomicU64 = AtomicU64::new(1);

/// The padded image of each tileset a map draws, made once and made again
/// when the tileset or an image of it changes, and kept for `KEEP_ROUNDS`
/// rounds after a map last asked for it: a level despawned and another of
/// the same tileset spawned soon after (a level restarted or changed) draws
/// with it, rather than padding again, and making its texture again, in its
/// landing frame. A tileset too big to pad draws nothing, and the dev log
/// warns once for each version of it a map draws.
///
/// Shared through the world ([`PaddedCache`]): a map's landing job pads the
/// tilesets the cache has no padding of yet, with their textures' levels,
/// on its thread (`to_pad`, `ToPad::pad`), and its landing hands them over
/// (`adopt`), so the landing frame only writes them. Padding again a
/// tileset already padded (it changed) is the drawer's, at once and in
/// place
pub struct PaddedTilesets {
  entries: FastMap<Key, Entry>,
  too_big: FastMap<Key, Refused>,
  /// The round under way, counted from 1
  round: u64,
  /// The most texels a padded image may span on a side: the device's
  /// largest texture side, which the engine sets once its device exists
  /// (`set_texture_limit`)
  max_side: u32,
  /// Paddings landings handed over (tests)
  #[cfg(test)]
  adopted: usize,
}

impl Default for PaddedTilesets {
  fn default() -> Self {
    Self {
      entries: FastMap::default(),
      too_big: FastMap::default(),
      round: 0,
      max_side: default_max_side(),
      #[cfg(test)]
      adopted: 0,
    }
  }
}

/// The padded tilesets as the world holds them, which the engine inserts
/// with its world (and gives its device's texture limit), the map drawer
/// locks each frame, and landings lock to look and to hand paddings over: a
/// lock, where taking a resource out of the world and putting it back each
/// frame moves an entity's components twice
#[derive(Resource, Default, Clone)]
pub(crate) struct PaddedCache(Arc<Mutex<PaddedTilesets>>);

impl PaddedCache {
  pub(crate) fn lock(&self) -> MutexGuard<'_, PaddedTilesets> {
    self.0.lock().unwrap_or_else(PoisonError::into_inner)
  }
}

/// A tileset's key, the tileset and its version
fn keyed<'a>(
  source: &'a TilesetSource,
  tilesets: &'a Assets<Tileset>,
) -> Option<(Key, &'a Tileset, u64)> {
  Some(match source {
    TilesetSource::Asset(handle) => (
      Key::Asset(handle.id()),
      tilesets.get(handle)?,
      tilesets.version(handle)?,
    ),
    TilesetSource::Embedded(tileset) => {
      (Key::Embedded(Arc::as_ptr(tileset) as usize), &**tileset, 0)
    }
  })
}

/// A tileset a landing job is to pad, with copies of what it reads
pub(crate) struct ToPad {
  key: Key,
  stamp: u64,
  tileset: Copied,
  pixels: FastMap<AssetId<Image>, Image>,
  /// The most texels its padded image may span on a side
  max_side: u32,
}

/// A tileset as a job holds it: a copy of an asset's, or an embedded one's
/// `Arc` (which also keeps its address, its key, from being reused)
enum Copied {
  Asset(Box<Tileset>),
  Embedded(Arc<Tileset>),
}

/// A padding made ahead, its texture's levels too, for its landing to hand
/// over
pub(crate) struct Prepared {
  key: Key,
  stamp: u64,
  name: String,
  image: Image,
  layout: PaddedLayout,
  holds: Option<Weak<Tileset>>,
}

impl ToPad {
  /// Pads the tileset and makes its texture's levels (the bordered first
  /// level and the mips), so uploading it only copies them; `None` where
  /// there is nothing to draw, or it is too big (the drawer says so)
  pub(crate) fn pad(self) -> Option<Prepared> {
    let tileset = match &self.tileset {
      Copied::Asset(tileset) => &**tileset,
      Copied::Embedded(tileset) => &**tileset,
    };
    let (mut image, mut layout) = pad_tileset(tileset, &self.pixels, self.max_side).ok()?;
    layout.serial = SERIALS.fetch_add(1, Ordering::Relaxed);
    image.prepare_upload();
    let holds = match &self.tileset {
      Copied::Asset(_) => None,
      Copied::Embedded(tileset) => Some(Arc::downgrade(tileset)),
    };
    Some(Prepared {
      key: self.key,
      stamp: self.stamp,
      name: tileset.name.clone(),
      image,
      layout,
      holds,
    })
  }
}

/// A hash of the versions of what `tileset` is padded from
fn stamp(tileset: &Tileset, version: u64, images: &Assets<Image>) -> u64 {
  let mut h = DefaultHasher::new();
  version.hash(&mut h);
  match &tileset.image {
    TilesetImage::Atlas { image, .. } => images.version(image).hash(&mut h),
    TilesetImage::Collection => {
      // Each tile's (id, image version) hashed alone and the hashes summed,
      // so the map's order does not matter and nothing is collected
      let tiles = tileset
        .tiles()
        .filter_map(|(id, data)| Some((id, &data.image.as_ref()?.0)));
      let sum = tiles.fold(0u64, |sum, (id, handle)| {
        let mut one = DefaultHasher::new();
        id.hash(&mut one);
        images.version(handle).hash(&mut one);
        sum.wrapping_add(one.finish())
      });
      sum.hash(&mut h);
    }
  }
  h.finish()
}

impl PaddedTilesets {
  /// Pads for a device whose textures span at most
  /// `max_texture_dimension_2d` texels a side: a tileset whose padded image
  /// would not fit one draws nothing, warned.
  /// Paddings already made are kept
  pub(crate) fn set_texture_limit(&mut self, max_texture_dimension_2d: u32) {
    self.max_side = texture_side_room(max_texture_dimension_2d);
  }

  /// The most texels a padded image may span on a side (tests)
  #[cfg(test)]
  pub(crate) fn max_side(&self) -> u32 {
    self.max_side
  }

  /// How many paddings landings made and handed over (tests)
  #[cfg(test)]
  pub(crate) fn adopted(&self) -> usize {
    self.adopted
  }

  /// Starts a round of `get`s (one a frame): what no `get` has asked for in
  /// `KEEP_ROUNDS` rounds is freed at its end
  pub fn begin_round(&mut self) {
    self.round += 1;
  }

  /// The tilesets of `sources` a landing job is to pad: those loaded with
  /// their images that have no padding (nor a refusal) yet. One padded
  /// before, of another version, is left to the drawer, which pads it
  /// again in place
  pub(crate) fn to_pad(
    &self,
    sources: &[TilesetSource],
    tilesets: &Assets<Tileset>,
    images: &Assets<Image>,
  ) -> Vec<ToPad> {
    let mut out = Vec::new();
    for source in sources {
      let Some((key, tileset, version)) = keyed(source, tilesets) else {
        continue;
      };
      if self.entries.contains_key(&key) || self.too_big.contains_key(&key) {
        continue;
      }
      let mut pixels = FastMap::default();
      let mut copy = |handle: &Handle<Image>| match images.get(handle) {
        Some(image) => {
          pixels.insert(handle.id(), image.clone());
          true
        }
        None => false,
      };
      let ready = match &tileset.image {
        TilesetImage::Atlas { image, .. } => copy(image),
        TilesetImage::Collection => tileset
          .tiles()
          .filter_map(|(_, data)| data.image.as_ref())
          .fold(true, |ready, (handle, _)| copy(handle) && ready),
      };
      if !ready {
        continue;
      }
      out.push(ToPad {
        key,
        stamp: stamp(tileset, version, images),
        tileset: match source {
          TilesetSource::Asset(_) => Copied::Asset(Box::new(tileset.clone())),
          TilesetSource::Embedded(tileset) => Copied::Embedded(Arc::clone(tileset)),
        },
        pixels,
        max_side: self.max_side,
      });
    }
    out
  }

  /// Takes a padding a landing job made: kept as the tileset's, unless one
  /// was made meanwhile (the drawer's, which goes on drawing)
  pub(crate) fn adopt(&mut self, prepared: Prepared, images: &mut Assets<Image>) {
    if self.entries.contains_key(&prepared.key) {
      return;
    }
    let handle = images.add(prepared.image);
    #[cfg(test)]
    {
      self.adopted += 1;
    }
    self.too_big.remove(&prepared.key);
    self.entries.insert(
      prepared.key,
      Entry {
        stamp: prepared.stamp,
        name: prepared.name,
        handle,
        layout: prepared.layout,
        asked: self.round,
        _holds: prepared.holds,
      },
    );
  }

  /// The padded image and layout of `source`; `None` while there is nothing
  /// to draw
  pub fn get(
    &mut self,
    source: &TilesetSource,
    tilesets: &Assets<Tileset>,
    images: &mut Assets<Image>,
  ) -> Option<(Handle<Image>, PaddedLayout)> {
    let (key, tileset, version) = keyed(source, tilesets)?;
    // An embedded tileset is known by its address: what is kept of it holds
    // its allocation, so a tileset made after it is gone cannot take the
    // address and be given its padding
    let holds = || match source {
      TilesetSource::Asset(_) => None,
      TilesetSource::Embedded(tileset) => Some(Arc::downgrade(tileset)),
    };
    let stamp = stamp(tileset, version, images);
    if let Some(entry) = self.entries.get_mut(&key)
      && entry.stamp == stamp
    {
      entry.asked = self.round;
      return Some((entry.handle.clone(), entry.layout.clone()));
    }
    if let Some(refused) = self.too_big.get_mut(&key)
      && refused.stamp == stamp
    {
      refused.asked = self.round;
      return None;
    }
    let (image, mut layout) = match pad_tileset(tileset, images, self.max_side) {
      Ok(made) => made,
      Err(why) => {
        // Nothing to draw (yet), or too big: what was made before is stale
        self.entries.remove(&key);
        let refused = match why {
          Unpadded::Nothing => false,
          Unpadded::TooBig { cells, size } => {
            crate::engine_warn!(
              module: "Tilemap",
              "tileset \"{}\" is too big to pad: its {cells} cells need a padded image of {} x {} texels, over the {} a side it may span (the GPU's largest texture side), so its tiles draw nothing (a tile's cell is the tile with each side rounded up to a multiple of a power of two, by at most a quarter)",
              tileset.name,
              size.0,
              size.1,
              self.max_side
            );
            true
          }
          Unpadded::TileTooBig { size } => {
            crate::engine_warn!(
              module: "Tilemap",
              "tileset \"{}\" is too big to pad: its tile of {} x {} texels needs a padded cell over the {} texels a side it may span (the GPU's largest texture side), so its tiles draw nothing",
              tileset.name,
              size.x,
              size.y,
              self.max_side
            );
            true
          }
        };
        if refused {
          self.too_big.insert(
            key,
            Refused {
              stamp,
              asked: self.round,
              _holds: holds(),
            },
          );
        }
        return None;
      }
    };
    self.too_big.remove(&key);
    layout.serial = SERIALS.fetch_add(1, Ordering::Relaxed);
    // Made again in place, so the draw keeps its texture's identity
    let handle = match self.entries.remove(&key) {
      Some(old) => match images.get_mut(&old.handle) {
        Some(slot) => {
          *slot = image;
          old.handle
        }
        None => images.add(image),
      },
      None => images.add(image),
    };
    self.entries.insert(
      key,
      Entry {
        stamp,
        name: tileset.name.clone(),
        handle: handle.clone(),
        layout: layout.clone(),
        asked: self.round,
        _holds: holds(),
      },
    );
    Some((handle, layout))
  }

  /// Frees what no `get` has asked for in `KEEP_ROUNDS` rounds
  pub fn end_round(&mut self) {
    let round = self.round;
    let kept = |asked: u64| round.saturating_sub(asked) < KEEP_ROUNDS;
    self.entries.retain(|_, e| kept(e.asked));
    self.too_big.retain(|_, refused| kept(refused.asked));
  }

  /// How many padded images are kept (tests)
  #[cfg(test)]
  pub(crate) fn len(&self) -> usize {
    self.entries.len()
  }

  /// The serial of the padding kept for `source`, if one is (tests)
  #[cfg(test)]
  pub(crate) fn serial(&self, source: &TilesetSource, tilesets: &Assets<Tileset>) -> Option<u64> {
    let (key, _, _) = keyed(source, tilesets)?;
    self.entries.get(&key).map(|entry| entry.layout.serial)
  }
}

#[cfg(test)]
pub(crate) mod tests {
  use super::*;
  use crate::core::graphics::atlas::{BORDER, own_texture, upload_border};
  use crate::core::tilemap::{TileData, TileFrame, TilesetImage};
  use std::time::Duration;

  /// A `cols` x `rows` atlas of `tile` px tiles; texel (x, y) of tile `id`
  /// is `[x, y, id, 255]` (x and y within the tile)
  pub(crate) fn marked_atlas(
    images: &mut Assets<Image>,
    tile: u32,
    cols: u32,
    rows: u32,
  ) -> Tileset {
    marked_atlas_of(images, UVec2::splat(tile), cols, rows)
  }

  /// [`marked_atlas`] of tiles `tile` texels wide and tall
  fn marked_atlas_of(images: &mut Assets<Image>, tile: UVec2, cols: u32, rows: u32) -> Tileset {
    let (w, h) = (tile.x * cols, tile.y * rows);
    let mut data = Vec::new();
    for y in 0..h {
      for x in 0..w {
        let id = (y / tile.y) * cols + x / tile.x;
        data.extend_from_slice(&[(x % tile.x) as u8, (y % tile.y) as u8, id as u8, 255]);
      }
    }
    let image = images.add(Image::from_rgba(data, w, h));
    let mut t = Tileset::new(
      "marked",
      tile,
      TilesetImage::Atlas {
        image,
        size: UVec2::new(w, h),
      },
    );
    t.columns = cols;
    t.tile_count = cols * rows;
    t
  }

  /// An atlas of `count` tiles of `size` texels in a row over a stand-in
  /// image: laying out reads no texel
  fn stand_in_atlas(images: &mut Assets<Image>, size: UVec2, count: u32) -> Tileset {
    let image = images.add(Image::solid_rect(1, 1, 0, 0, 0, 255));
    let mut t = Tileset::new(
      "stand-in",
      size,
      TilesetImage::Atlas {
        image,
        size: UVec2::new(size.x * count, size.y),
      },
    );
    t.columns = count;
    t.tile_count = count;
    t
  }

  /// A collection of images of `sizes` (ids from 0), each solid in its
  /// colour of `colours` (by id, wrapping), or a stand-in 1 px image when
  /// `colours` is empty
  fn collection(images: &mut Assets<Image>, sizes: &[UVec2], colours: &[[u8; 4]]) -> Tileset {
    let stand_in = images.add(Image::solid_rect(1, 1, 0, 0, 0, 255));
    let largest = sizes.iter().fold(UVec2::ZERO, |l, s| l.max(*s));
    let mut t = Tileset::new("collection", largest, TilesetImage::Collection);
    for (id, &size) in sizes.iter().enumerate() {
      let image = match colours.get(id % colours.len().max(1)) {
        Some(&[r, g, b, a]) => images.add(Image::solid_rect(size.x, size.y, r, g, b, a)),
        None => stand_in.clone(),
      };
      t.insert_tile(
        id as u32,
        TileData {
          image: Some((image, size)),
          ..TileData::default()
        },
      );
    }
    t.tile_count = sizes.len() as u32;
    t
  }

  /// Tile `id` of `t` animated through `frames` (tile, milliseconds)
  fn animate(t: &mut Tileset, id: u32, frames: &[(u32, u64)]) {
    let mut data = t.tile(id).cloned().unwrap_or_default();
    data.animation = frames
      .iter()
      .map(|&(tile, ms)| TileFrame {
        tile,
        duration: Duration::from_millis(ms),
      })
      .collect();
    t.insert_tile(id, data);
  }

  /// `tileset`'s layout under the engine's texture limit, nothing painted
  fn laid_out(tileset: &Tileset, images: &Assets<Image>) -> PaddedLayout {
    lay_out(tileset, images, default_max_side())
      .expect("laid out")
      .layout
  }

  /// Every cell of `layout` with the size of the tile it holds: each tile's
  /// own, then each of an animated tile's frames
  fn every_cell(layout: &PaddedLayout) -> Vec<(PaddedCell, UVec2)> {
    let mut cells = Vec::new();
    for tile in layout.tiles.iter().flatten() {
      cells.push((tile.cell, tile.size));
      if let Some(a) = tile.animation {
        cells.extend((0..a.frames).map(|k| (a.frame(k), tile.size)));
      }
    }
    cells
  }

  /// Where tile `id`'s picture starts: centred in its cell
  fn picture(layout: &PaddedLayout, id: u32) -> UVec2 {
    let tile = layout.tile(id).expect("tile");
    tile.cell.origin + (tile.cell.size - tile.size) / 2
  }

  fn texel(image: &Image, x: u32, y: u32) -> [u8; 4] {
    let i = ((y * image.width() + x) * 4) as usize;
    image.data()[i..i + 4].try_into().expect("four bytes")
  }

  #[test]
  fn clean_levels_keep_the_rounding_within_a_quarter() {
    let k = |x, y, atlas| clean_levels(UVec2::new(x, y), atlas);
    assert_eq!(k(16, 16, false), 4);
    assert_eq!(k(16, 16, true), 3);
    assert_eq!(k(32, 32, false), 5);
    assert_eq!(k(256, 256, false), 8);
    assert_eq!(k(129, 129, false), 5);
    assert_eq!(k(256, 352, false), 7);
    assert_eq!(k(120, 140, false), 5);
    assert_eq!(k(14, 14, false), 4);
    assert_eq!(k(14, 14, true), 3);
    assert_eq!(k(1, 1, false), 0);
    assert_eq!(cell_for(UVec2::new(256, 352), 7), UVec2::new(256, 384));
    assert_eq!(cell_for(UVec2::new(120, 140), 5), UVec2::new(128, 160));
    assert_eq!(cell_for(UVec2::new(129, 129), 5), UVec2::new(160, 160));
    assert_eq!(cell_for(UVec2::new(14, 14), 3), UVec2::new(16, 16));
  }

  #[test]
  fn every_cell_starts_on_its_own_alignment_and_none_overlap() {
    let mut images = Assets::new();
    let size = UVec2::new;
    let mut sets = vec![
      ("256 x 352", stand_in_atlas(&mut images, size(256, 352), 14)),
      (
        "120 x 140",
        stand_in_atlas(&mut images, size(120, 140), 112),
      ),
    ];
    let mut mixed = vec![size(512, 512)];
    mixed.extend([size(32, 32); 50]);
    mixed.extend([size(100, 60); 3]);
    sets.push(("mixed", collection(&mut images, &mixed, &[])));
    let mut animated = collection(&mut images, &[size(64, 64), size(48, 80)], &[]);
    animate(&mut animated, 0, &[(0, 100), (1, 100)]);
    sets.push(("animated", animated));
    for (name, set) in &sets {
      let layout = laid_out(set, &images);
      let cells = every_cell(&layout);
      let largest = cells
        .iter()
        .map(|(cell, _)| 1u32 << cell.levels)
        .max()
        .expect("cells");
      assert_eq!(
        layout.size % largest,
        UVec2::ZERO,
        "{name}: {}",
        layout.size
      );
      for &(cell, tile) in &cells {
        let step = 1u32 << cell.levels;
        assert_eq!(cell.origin % step, UVec2::ZERO, "{name}: {cell:?}");
        assert_eq!(cell.size % step, UVec2::ZERO, "{name}: {cell:?}");
        assert!(cell.size.cmpge(tile).all(), "{name}: {cell:?} holds {tile}");
        assert!(
          (cell.origin + cell.size).cmple(layout.size).all(),
          "{name}: {cell:?} inside {}",
          layout.size
        );
      }
      for (i, (a, _)) in cells.iter().enumerate() {
        for (b, _) in &cells[i + 1..] {
          let (a_end, b_end) = (a.origin + a.size, b.origin + b.size);
          let apart = a_end.x <= b.origin.x
            || b_end.x <= a.origin.x
            || a_end.y <= b.origin.y
            || b_end.y <= a.origin.y;
          assert!(apart, "{name}: {a:?} overlaps {b:?}");
        }
      }
    }
  }

  #[test]
  fn padded_bytes_stay_near_the_art() {
    let mut images = Assets::new();
    let size = UVec2::new;
    // Sketch Town's pictures: 256 x 384 cells, 7 columns of 2
    let sketch = laid_out(&stand_in_atlas(&mut images, size(256, 352), 14), &images);
    assert_eq!(sketch.size, size(1792, 768));
    assert_eq!(sketch.art_bytes, 14 * 256 * 352 * 4);
    assert!(
      sketch.gpu_bytes() <= 2 * sketch.art_bytes,
      "{} of {}",
      sketch.gpu_bytes(),
      sketch.art_bytes
    );
    // No rounding: the art and its smaller copies
    let square = laid_out(&stand_in_atlas(&mut images, size(256, 256), 4), &images);
    assert!(
      square.gpu_bytes() as f64 <= square.art_bytes as f64 * 4.0 / 3.0 * 1.001,
      "{} of {}",
      square.gpu_bytes(),
      square.art_bytes
    );
    // A castle and fifty flowers, each in a cell of its own size
    let mut sizes = vec![size(512, 512)];
    sizes.extend([size(32, 32); 50]);
    let castle = laid_out(&collection(&mut images, &sizes, &[]), &images);
    assert!(
      2 * castle.gpu_bytes() <= 5 * castle.art_bytes,
      "{} of {}",
      castle.gpu_bytes(),
      castle.art_bytes
    );
    // Small tiles take their own size
    let small = laid_out(&stand_in_atlas(&mut images, size(16, 16), 64), &images);
    for (cell, _) in every_cell(&small) {
      assert_eq!(cell.size, UVec2::splat(16));
    }
  }

  #[test]
  fn an_animation_s_frames_take_cells_of_its_largest_frame_in_one_block() {
    let mut images = Assets::new();
    let (red, green) = ([200, 30, 30, 255], [30, 200, 30, 255]);
    let sizes = [UVec2::new(64, 64), UVec2::new(48, 80)];
    let mut set = collection(&mut images, &sizes, &[red, green]);
    animate(
      &mut set,
      0,
      &[(0, 100), (1, 100), (0, 100), (1, 100), (0, 100)],
    );
    let (image, layout) = pad_tileset(&set, &images, default_max_side()).expect("padded");
    let anim = layout.tile(0).and_then(|t| t.animation).expect("animated");
    assert_eq!(anim.frames, 5);
    let largest = UVec2::new(64, 80);
    let levels = clean_levels(largest, !own_texture(layout.size.x, layout.size.y));
    let cell = cell_for(largest, levels);
    assert_eq!((anim.first.size, anim.first.levels), (cell, levels));
    for k in 0..anim.frames {
      let frame = anim.frame(k);
      let step = UVec2::new(k % anim.columns, k / anim.columns) * cell;
      assert_eq!(frame.origin, anim.first.origin + step, "frame {k}");
      assert_eq!((frame.size, frame.levels), (cell, levels), "frame {k}");
      // Each frame shows its tile
      let centre = frame.origin + frame.size / 2;
      let want = if k % 2 == 0 { red } else { green };
      assert_eq!(texel(&image, centre.x, centre.y), want, "frame {k}");
    }
  }

  #[test]
  fn a_padded_image_that_fits_the_atlas_caps_its_levels_at_three() {
    let mut images = Assets::new();
    // 9 tiles of 64 px: 192 px, in the atlas, whose pages keep four levels
    let (image, layout) = pad_tileset(
      &marked_atlas(&mut images, 64, 3, 3),
      &images,
      default_max_side(),
    )
    .expect("padded");
    assert_eq!(layout.size, UVec2::splat(192));
    assert!(image.is_borderless() && !image.has_own_texture());
    for (cell, _) in every_cell(&layout) {
      assert_eq!((cell.size, cell.levels), (UVec2::splat(64), 3));
    }
    // 25 of them: 320 px, a texture of its own with every level and no
    // border
    let (image, layout) = pad_tileset(
      &marked_atlas(&mut images, 64, 5, 5),
      &images,
      default_max_side(),
    )
    .expect("padded");
    assert_eq!(layout.size, UVec2::splat(320));
    assert!(image.is_borderless() && image.has_own_texture());
    assert_eq!(upload_border(320, 320, true), 0);
    for (cell, _) in every_cell(&layout) {
      assert_eq!((cell.size, cell.levels), (UVec2::splat(64), 6));
    }
  }

  #[test]
  fn a_padded_tileset_extrudes_every_tile_into_its_own_cell() {
    // 14 x 10 tiles take 16 x 12 cells (two clean levels): each tile sits
    // centred, a texel in on every side, its edge texels out to the cell's
    // edges
    let mut images = Assets::new();
    let t = marked_atlas_of(&mut images, UVec2::new(14, 10), 2, 2);
    let (image, layout) = pad_tileset(&t, &images, default_max_side()).expect("padded");
    assert_eq!(layout.size, UVec2::new(32, 24));
    assert_eq!((image.width(), image.height()), (32, 24));
    for id in 0..4u32 {
      let tile = layout.tile(id).expect("tile");
      assert_eq!((tile.cell.size, tile.cell.levels), (UVec2::new(16, 12), 2));
      let o = tile.cell.origin;
      let at = |x, y| texel(&image, o.x + x, o.y + y);
      let id = id as u8;
      // The tile itself
      assert_eq!(at(1, 1), [0, 0, id, 255]);
      assert_eq!(at(14, 10), [13, 9, id, 255]);
      assert_eq!(at(4, 6), [3, 5, id, 255]);
      // Its edges and corners, repeated to the cell's edges
      assert_eq!(at(0, 0), [0, 0, id, 255]);
      assert_eq!(at(15, 0), [13, 0, id, 255]);
      assert_eq!(at(0, 11), [0, 9, id, 255]);
      assert_eq!(at(15, 11), [13, 9, id, 255]);
      assert_eq!(at(0, 5), [0, 4, id, 255]);
      assert_eq!(at(7, 11), [6, 9, id, 255]);
      // Every texel of the cell is this tile's
      for y in 0..12 {
        for x in 0..16 {
          assert_eq!(at(x, y)[2], id);
        }
      }
    }
  }

  #[test]
  fn a_tileset_s_transparent_colour_pads_transparent() {
    use crate::core::ui::Color;
    // Texel (x, y) of tile `id` is [x, y, id, 255]: with [3, 4, 1] the
    // transparent colour, tile 1's texel (3, 4) pads clear, and nothing else
    let mut images = Assets::new();
    let mut t = marked_atlas(&mut images, 16, 2, 2);
    t.transparent = Some(Color::rgb(3, 4, 1));
    let (image, layout) = pad_tileset(&t, &images, default_max_side()).expect("padded");
    let o = picture(&layout, 1);
    assert_eq!(texel(&image, o.x + 3, o.y + 4), [0, 0, 0, 0]);
    assert_eq!(texel(&image, o.x + 4, o.y + 4), [4, 4, 1, 255]);
    let o = picture(&layout, 0);
    assert_eq!(texel(&image, o.x + 3, o.y + 4), [3, 4, 0, 255]);
  }

  #[test]
  fn a_padded_image_of_its_own_texture_starts_its_cells_at_texel_zero() {
    // 289 tiles of 16 px: seventeen 16 px cells a side, 272 px, over the
    // atlas's 256 px, so a texture of its own
    let mut images = Assets::new();
    let t = marked_atlas(&mut images, 16, 17, 17);
    let (image, layout) = pad_tileset(&t, &images, default_max_side()).expect("padded");
    assert!(image.is_borderless() && image.has_own_texture());
    assert_eq!(layout.tile(0).expect("tile").cell.origin, UVec2::ZERO);
    assert_eq!(layout.size, UVec2::splat(17 * 16));
  }

  #[test]
  fn a_small_padded_image_has_no_margin_and_sits_in_an_atlas_page() {
    // One 64 px tile: a cell of its own size, the image 64 px
    let mut images = Assets::new();
    let t = marked_atlas(&mut images, 64, 1, 1);
    let (image, layout) = pad_tileset(&t, &images, default_max_side()).expect("padded");
    assert!(image.is_borderless() && !image.has_own_texture());
    assert_eq!(layout.size, UVec2::splat(64));
    assert_eq!((image.width(), image.height()), (64, 64));
    assert_eq!(upload_border(64, 64, true), BORDER, "a slot's border");
  }

  #[test]
  fn nothing_to_pad_is_none() {
    let images = Assets::new();
    let t = Tileset::new("e", UVec2::splat(16), TilesetImage::Collection);
    assert!(matches!(
      pad_tileset(&t, &images, default_max_side()),
      Err(Unpadded::Nothing)
    ));
  }

  #[test]
  fn unequal_frame_times_repeat_frames_to_a_common_beat() {
    let mut images = Assets::new();
    let mut t = marked_atlas(&mut images, 16, 2, 2);
    animate(&mut t, 0, &[(1, 100), (2, 300)]);
    let (image, layout) = pad_tileset(&t, &images, default_max_side()).expect("padded");
    let anim = layout.tile(0).and_then(|t| t.animation).expect("animated");
    assert_eq!(anim.frames, 4);
    assert!((anim.frame_duration - 0.1).abs() < 1e-6);
    let shown: Vec<u8> = (0..4)
      .map(|k| {
        let o = anim.frame(k).origin;
        texel(&image, o.x + 8, o.y + 8)[2]
      })
      .collect();
    assert_eq!(shown, [1, 2, 2, 2]);
  }

  #[test]
  fn padded_images_are_made_once_remade_on_change_and_freed_unused() {
    let mut tilesets = Assets::<Tileset>::new();
    let mut images = Assets::new();
    let t = marked_atlas(&mut images, 16, 2, 2);
    let TilesetImage::Atlas { image: src, .. } = t.image.clone() else {
      unreachable!()
    };
    let source = TilesetSource::Asset(tilesets.add(t));
    let mut padded = PaddedTilesets::default();
    let count = images.len();

    padded.begin_round();
    let (first, made) = padded.get(&source, &tilesets, &mut images).expect("padded");
    padded.end_round();
    assert_eq!(images.len(), count + 1);
    let v1 = images.version(&first).expect("loaded");
    assert_ne!(made.serial, 0);

    padded.begin_round();
    let (again, kept) = padded.get(&source, &tilesets, &mut images).expect("padded");
    padded.end_round();
    assert_eq!(first, again);
    assert_eq!(images.version(&again), Some(v1));
    assert_eq!(kept.serial, made.serial, "the same padding");

    // The atlas image changes: the padded image is made again, in place
    images.get_mut(&src).expect("image").data_mut()[2] = 99;
    padded.begin_round();
    let (third, layout) = padded.get(&source, &tilesets, &mut images).expect("padded");
    padded.end_round();
    assert_eq!(first, third);
    assert_ne!(images.version(&third), Some(v1));
    assert!(layout.serial > made.serial, "a new padding");
    let o = layout.tile(0).expect("tile").cell.origin;
    let img = images.get(&third).expect("padded image");
    assert_eq!(texel(img, o.x, o.y)[2], 99);

    // Rounds that do not ask free it, once it has been kept a while
    for _ in 0..KEEP_ROUNDS {
      padded.begin_round();
      padded.end_round();
    }
    drop((first, again, third));
    images.free_unheld(&mut Vec::new());
    assert_eq!(images.len(), count);
  }

  #[test]
  fn a_padding_unasked_for_a_while_is_kept_and_then_freed() {
    // A map despawned and another of the same tileset spawned a few frames
    // later (a level restarted or changed) draws with the same padding,
    // not one made again in its landing frame
    let mut tilesets = Assets::<Tileset>::new();
    let mut images = Assets::new();
    let source = TilesetSource::Asset(tilesets.add(marked_atlas(&mut images, 16, 2, 2)));
    let mut padded = PaddedTilesets::default();
    padded.begin_round();
    let (handle, made) = padded.get(&source, &tilesets, &mut images).expect("padded");
    padded.end_round();
    for _ in 0..30 {
      padded.begin_round();
      padded.end_round();
    }
    padded.begin_round();
    let (again, kept) = padded.get(&source, &tilesets, &mut images).expect("padded");
    padded.end_round();
    assert_eq!(
      (again, kept.serial),
      (handle, made.serial),
      "the same padding"
    );
    // Unasked for as long as an undrawn chunk is kept, it goes
    for _ in 0..KEEP_ROUNDS {
      padded.begin_round();
      padded.end_round();
    }
    assert_eq!(padded.len(), 0);
  }

  #[test]
  fn a_gone_embedded_tileset_s_address_is_held_while_its_padding_is_kept() {
    // An inline tileset is known by its `Arc`'s address. A map saved with
    // its inline tileset changed drops the old `Arc`; were its allocation
    // freed while its padding is kept, a new tileset (of another tile size,
    // over the same image) allocated there would get the old padding. So
    // the kept padding holds the allocation (a `Weak`) until it goes
    let tilesets = Assets::<Tileset>::new();
    let mut images = Assets::new();
    let mut padded = PaddedTilesets::default();
    let first = Arc::new(marked_atlas(&mut images, 16, 2, 2));
    let too_big = {
      let image = images.add(Image::solid_rect(4, 4, 0, 0, 0, 255));
      let mut set = Tileset::new(
        "inline-too-big",
        UVec2::splat(64),
        TilesetImage::Atlas {
          image,
          size: UVec2::splat(64 * 130),
        },
      );
      set.columns = 130;
      set.tile_count = 130 * 130;
      Arc::new(set)
    };
    padded.begin_round();
    let source = TilesetSource::Embedded(first.clone());
    assert!(padded.get(&source, &tilesets, &mut images).is_some());
    let source = TilesetSource::Embedded(too_big.clone());
    assert!(padded.get(&source, &tilesets, &mut images).is_none());
    padded.end_round();
    drop(source);
    // The padding and the refusal each hold their tileset's allocation
    assert_eq!(Arc::weak_count(&first), 1, "the padding holds it");
    assert_eq!(Arc::weak_count(&too_big), 1, "the refusal holds it");
    for _ in 0..KEEP_ROUNDS {
      padded.begin_round();
      padded.end_round();
    }
    assert_eq!(padded.len(), 0);
    assert_eq!(Arc::weak_count(&first), 0, "let go with the padding");
    assert_eq!(Arc::weak_count(&too_big), 0, "and with the refusal");
  }

  #[test]
  fn a_padded_image_past_the_device_s_texture_side_is_too_big() {
    // 16,900 tiles of 64 px: 64 px cells, at most 128 a side within the
    // 8,192 texels a texture may span under the limits the engine asks for,
    // so 16,384 of them at most; the atlas would refuse the image and the
    // tiles would draw nothing, unwarned
    let mut tilesets = Assets::<Tileset>::new();
    let mut images = Assets::new();
    let image = images.add(Image::solid_rect(16, 16, 255, 0, 0, 255));
    let mut set = Tileset::new(
      "past-the-texture-3b9e",
      UVec2::splat(64),
      TilesetImage::Atlas {
        image,
        size: UVec2::splat(64 * 130),
      },
    );
    set.columns = 130;
    set.tile_count = 130 * 130;
    let source = TilesetSource::Asset(tilesets.add(set));
    let mut padded = PaddedTilesets::default();
    padded.begin_round();
    assert!(padded.get(&source, &tilesets, &mut images).is_none());
    padded.end_round();
    let warned = crate::core::dev_log::get_entries()
      .iter()
      .filter(|e| e.message.contains("past-the-texture-3b9e"))
      .count();
    assert_eq!(warned, 1, "warned, naming the tileset");
    assert_eq!(images.len(), 1, "no padded image made");
    // A device whose textures span more pads it: 130 cells a side
    let mut padded = PaddedTilesets::default();
    padded.set_texture_limit(16384);
    padded.begin_round();
    let made = padded.get(&source, &tilesets, &mut images);
    assert_eq!(
      made.map(|(_, layout)| layout.size),
      Some(UVec2::splat(8320))
    );
  }

  #[test]
  fn a_tileset_too_big_to_pad_draws_nothing_and_warns_once() {
    // 16,900 tiles of 64 px: 64 px cells, more than the 128 a side that fit
    // the 8,192 texels a padded image may span. The atlas image itself is
    // small: nothing is padded, so nothing reads past it
    let mut tilesets = Assets::<Tileset>::new();
    let mut images = Assets::new();
    let image = images.add(Image::solid_rect(16, 16, 255, 0, 0, 255));
    let mut set = Tileset::new(
      "too-big-5d2a",
      UVec2::splat(64),
      TilesetImage::Atlas {
        image,
        size: UVec2::splat(64 * 130),
      },
    );
    set.columns = 130;
    set.tile_count = 130 * 130;
    let source = TilesetSource::Asset(tilesets.add(set));
    let mut padded = PaddedTilesets::default();
    for _ in 0..3 {
      padded.begin_round();
      assert!(padded.get(&source, &tilesets, &mut images).is_none());
      padded.end_round();
    }
    let warned = crate::core::dev_log::get_entries()
      .iter()
      .filter(|e| e.message.contains("too-big-5d2a"))
      .count();
    assert_eq!(warned, 1, "warned once, naming the tileset");
    // One tile too large for any cell says so, not a size of 2^64 texels
    let image = images.add(Image::solid_rect(4, 4, 0, 0, 0, 255));
    let mut huge = Tileset::new(
      "huge-tile-71c0",
      UVec2::splat(3_000_000_000),
      TilesetImage::Atlas {
        image,
        size: UVec2::splat(3_000_000_000),
      },
    );
    huge.columns = 1;
    huge.tile_count = 1;
    let source = TilesetSource::Asset(tilesets.add(huge));
    padded.begin_round();
    assert!(padded.get(&source, &tilesets, &mut images).is_none());
    padded.end_round();
    let said: Vec<String> = crate::core::dev_log::get_entries()
      .into_iter()
      .filter(|e| e.message.contains("huge-tile-71c0"))
      .map(|e| e.message)
      .collect();
    assert_eq!(said.len(), 1);
    assert!(
      said[0].contains("3000000000 x 3000000000 texels"),
      "{said:?}"
    );
  }
}
