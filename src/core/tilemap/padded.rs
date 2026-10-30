//! Padded tileset images: every tile in a power-of-two cell of its own, its
//! edge texels extruded to fill the cell, so no filter and no mip level of
//! the image reads a neighbouring tile at any zoom.
//!
//! The padded image is an ordinary `Image` asset of its own (held by
//! [`PaddedTilesets`]), drawn through the renderer's usual image path: one
//! over 256 px gets a texture of its own with a mip chain, a smaller one may
//! sit in the shared atlas, where its slot can move (the draw maps UVs
//! through the slot). UVs written for it are in padded-image space, 0..1
//! over the whole image.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use glam::{IVec2, UVec2};

use super::map::TilesetSource;
use super::tileset::{Tileset, TilesetImage, URect};
use crate::core::asset::{AssetId, Assets, Handle};
use crate::core::graphics::Image;
use crate::core::graphics::atlas::BORDER;
use crate::core::hash::FastMap;

/// The least room a cell keeps past its tile, in texels: the atlas's four
/// mip levels (the coarsest 8 texels a texel) each stay inside the cell
const MIN_ROOM: u32 = 8;
/// The most texels a padded image may span on a side
const MAX_SIDE: u64 = 16384;
/// The most cells one animation may take once its frames are repeated to a
/// common beat; past that it plays on a beat of the mean frame time
const MAX_ANIMATION_CELLS: u64 = 128;

/// How a tileset's tiles sit in its padded image
#[derive(Clone, Debug)]
pub(crate) struct PaddedLayout {
  /// A cell's side in texels: the power of two at or above twice the
  /// largest tile's longer side, and at or above that side plus 8 (R10).
  /// Mip level k samples only a tile's own cell while 2^k is at most the
  /// room the cell has past the tile, so every level a tile drawn a pixel
  /// wide or more uses, and every level of an atlas page, is the tile's own
  pub cell: u32,
  /// Cells per row
  pub columns: u32,
  /// Texels before the first cell and after the last, across and down:
  /// with the 8 texels of border a texture puts around an image
  /// (`atlas::BORDER`), each cell starts on a multiple of its side and the
  /// texture's sides are whole cells, so a texture of its own (with every
  /// mip level) keeps each cell whole texels down to one texel a cell
  pub margin: u32,
  /// The padded image's size in texels
  pub size: UVec2,
  /// The tileset's `tile_offset`: where every tile draws from its cell's
  /// bottom-left corner, in pixels, x right and y down
  pub offset: IVec2,
  /// Which padding this is: [`PaddedTilesets`] gives each image it makes a
  /// number never given before (0 until then), so what was built from a
  /// layout can tell it was padded again
  pub serial: u64,
  tiles: Arc<Vec<Option<PaddedTile>>>,
}

/// Where one tile sits in the padded image
#[derive(Clone, Copy, Debug)]
pub(crate) struct PaddedTile {
  /// The tile's cell, counted row-major from the top left
  pub slot: u32,
  /// The tile's size in texels, as drawn unflipped
  pub size: UVec2,
  pub animation: Option<PaddedAnimation>,
}

/// An animated tile's frames: `frames` cells from `first_slot`, row-major,
/// wrapping rows, each shown for `frame_duration` seconds
#[derive(Clone, Copy, Debug)]
pub(crate) struct PaddedAnimation {
  pub first_slot: u32,
  pub frames: u32,
  pub frame_duration: f32,
}

impl PaddedLayout {
  /// Tile `id`'s place; `None` for an id the tileset has no picture for
  pub fn tile(&self, id: u32) -> Option<&PaddedTile> {
    self.tiles.get(id as usize)?.as_ref()
  }

  /// The top-left texel of cell `slot`
  pub fn cell_origin(&self, slot: u32) -> UVec2 {
    UVec2::new(slot % self.columns, slot / self.columns) * self.cell + self.margin
  }
}

/// Where a tile's texels come from
struct Source<'a> {
  image: &'a Image,
  rect: URect,
}

fn source<'a>(tileset: &Tileset, images: &'a Assets<Image>, id: u32) -> Option<Source<'a>> {
  match &tileset.image {
    TilesetImage::Atlas { image, .. } => Some(Source {
      image: images.get(image)?,
      rect: tileset.tile_rect(id)?,
    }),
    TilesetImage::Collection => {
      let (handle, size) = tileset.tile(id)?.image.as_ref()?;
      Some(Source {
        image: images.get(handle)?,
        rect: URect::new(0, 0, size.x, size.y),
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
fn animation_cells(tileset: &Tileset, images: &Assets<Image>, id: u32) -> Option<(Vec<u32>, f32)> {
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

/// Pads `tileset`'s tiles, or `None` when it has nothing to draw yet: no
/// tile with a picture, or its atlas image not loaded
pub(crate) fn pad_tileset(
  tileset: &Tileset,
  images: &Assets<Image>,
) -> Option<(Image, PaddedLayout)> {
  // Every tile that has a picture, by id
  let ids: Vec<u32> = match &tileset.image {
    TilesetImage::Atlas { image, .. } => {
      images.get(image)?;
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
  let ids: Vec<(u32, UVec2)> = ids
    .into_iter()
    .filter(|&id| source(tileset, images, id).is_some())
    .filter_map(|id| Some((id, tile_size(tileset, id)?)))
    .collect();
  let side = ids.iter().map(|(_, s)| s.x.max(s.y)).max()?;
  let room = side.max(MIN_ROOM);
  let cell = side.checked_add(room)?.checked_next_power_of_two()?;

  // The cells: the tiles by id, then each animation's frames in turn
  let mut cells: Vec<(u32, UVec2)> = ids.clone();
  let mut tiles: Vec<Option<PaddedTile>> = Vec::new();
  for (slot, &(id, size)) in ids.iter().enumerate() {
    if tiles.len() <= id as usize {
      tiles.resize(id as usize + 1, None);
    }
    tiles[id as usize] = Some(PaddedTile {
      slot: slot as u32,
      size,
      animation: None,
    });
  }
  for &(id, size) in &ids {
    let Some((frames, frame_duration)) = animation_cells(tileset, images, id) else {
      continue;
    };
    let first_slot = cells.len() as u32;
    cells.extend(frames.iter().map(|&f| (f, size)));
    if let Some(Some(tile)) = tiles.get_mut(id as usize) {
      tile.animation = Some(PaddedAnimation {
        first_slot,
        frames: frames.len() as u32,
        frame_duration,
      });
    }
  }

  let n = cells.len() as u64;
  let columns = (n as f64).sqrt().ceil().max(1.0) as u64;
  let rows = n.div_ceil(columns);
  // A texture of its own has the border around the image and a level for
  // every halving, which the GPU sizes by rounding down and spreads over
  // the whole texture. A margin on every side makes border and margin
  // together whole cells, so the cells start on whole texels of each level
  // down to one texel a cell, and the texture's sides halve exactly that
  // far. (An atlas page's four levels line up anyway: its slots, and 8
  // texels, are whole texels of all four)
  let margin = (cell - BORDER % cell) % cell;
  let side = |count: u64| count * u64::from(cell) + 2 * u64::from(margin);
  if side(columns) > MAX_SIDE || side(rows) > MAX_SIDE {
    return None;
  }
  let (columns, rows) = (columns as u32, rows as u32);
  let size = UVec2::new(columns, rows) * cell + 2 * margin;
  let mut layout = PaddedLayout {
    cell,
    columns,
    margin,
    size,
    offset: tileset.tile_offset,
    serial: 0,
    tiles: Arc::new(Vec::new()),
  };
  let mut data = vec![0u8; size.x as usize * size.y as usize * 4];
  for (slot, &(id, drawn)) in cells.iter().enumerate() {
    let Some(from) = source(tileset, images, id) else {
      continue;
    };
    let origin = layout.cell_origin(slot as u32);
    paint(&mut data, size.x, origin, cell, drawn, &from);
  }
  layout.tiles = Arc::new(tiles);
  Some((Image::from_rgba(data, size.x, size.y), layout))
}

/// Copies a tile of `drawn` texels into the cell at `origin`, centred, its
/// edge texels extruded over the rest of the cell. Texels the source lacks
/// stay transparent
fn paint(data: &mut [u8], stride: u32, origin: UVec2, cell: u32, drawn: UVec2, from: &Source) {
  let offset = UVec2::new((cell - drawn.x) / 2, (cell - drawn.y) / 2);
  let (src_w, src_h) = (from.image.width(), from.image.height());
  let pixels = from.image.data();
  for y in 0..cell {
    let sy = y.saturating_sub(offset.y).min(drawn.y - 1) + from.rect.min.y;
    for x in 0..cell {
      let sx = x.saturating_sub(offset.x).min(drawn.x - 1) + from.rect.min.x;
      if sx >= src_w || sy >= src_h {
        continue;
      }
      let at = (sy as usize * src_w as usize + sx as usize) * 4;
      let to = ((origin.y + y) as usize * stride as usize + (origin.x + x) as usize) * 4;
      data[to..to + 4].copy_from_slice(&pixels[at..at + 4]);
    }
  }
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
  handle: Handle<Image>,
  layout: PaddedLayout,
  asked: bool,
}

/// The next [`PaddedLayout::serial`]
static SERIALS: AtomicU64 = AtomicU64::new(1);

/// The padded image of each tileset a map draws, made once and made again
/// when the tileset or an image of it changes
#[derive(Default)]
pub(crate) struct PaddedTilesets {
  entries: FastMap<Key, Entry>,
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
  /// Starts a round of `get`s: what no `get` asks for before `end_round` is
  /// freed
  pub fn begin_round(&mut self) {
    for e in self.entries.values_mut() {
      e.asked = false;
    }
  }

  /// The padded image and layout of `source`; `None` while there is nothing
  /// to draw
  pub fn get(
    &mut self,
    source: &TilesetSource,
    tilesets: &Assets<Tileset>,
    images: &mut Assets<Image>,
  ) -> Option<(Handle<Image>, PaddedLayout)> {
    let (key, tileset, version) = match source {
      TilesetSource::Asset(handle) => (
        Key::Asset(handle.id()),
        tilesets.get(handle)?,
        tilesets.version(handle)?,
      ),
      TilesetSource::Embedded(tileset) => {
        (Key::Embedded(Arc::as_ptr(tileset) as usize), &**tileset, 0)
      }
    };
    let stamp = stamp(tileset, version, images);
    if let Some(entry) = self.entries.get_mut(&key)
      && entry.stamp == stamp
    {
      entry.asked = true;
      return Some((entry.handle.clone(), entry.layout.clone()));
    }
    let made = pad_tileset(tileset, images);
    let Some((image, mut layout)) = made else {
      // Nothing to draw (yet): what was made before is stale
      self.entries.remove(&key);
      return None;
    };
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
        handle: handle.clone(),
        layout: layout.clone(),
        asked: true,
      },
    );
    Some((handle, layout))
  }

  /// Frees what the round's `get`s did not ask for
  pub fn end_round(&mut self) {
    self.entries.retain(|_, e| e.asked);
  }

  /// How many padded images are kept (tests)
  #[cfg(test)]
  pub fn len(&self) -> usize {
    self.entries.len()
  }
}

#[cfg(test)]
pub(crate) mod tests {
  use super::*;
  use crate::core::tilemap::{TileData, TileFrame, TilesetImage};
  use std::alloc::{GlobalAlloc, Layout, System};
  use std::cell::Cell;
  use std::time::Duration;

  thread_local! {
    static ALLOCATIONS: Cell<usize> = const { Cell::new(0) };
  }

  /// Counts the allocations each thread makes
  struct Counting;

  // SAFETY: every call goes to `System`; the counter is a const-initialised
  // thread local without a destructor, so touching it never allocates
  unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
      ALLOCATIONS.with(|c| c.set(c.get() + 1));
      unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
      unsafe { System.dealloc(ptr, layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new: usize) -> *mut u8 {
      ALLOCATIONS.with(|c| c.set(c.get() + 1));
      unsafe { System.realloc(ptr, layout, new) }
    }
  }

  #[global_allocator]
  static COUNTING: Counting = Counting;

  fn allocations() -> usize {
    ALLOCATIONS.with(Cell::get)
  }

  /// A `cols` x `rows` atlas of `tile` px tiles; texel (x, y) of tile `id`
  /// is `[x, y, id, 255]` (x and y within the tile)
  pub(crate) fn marked_atlas(
    images: &mut Assets<Image>,
    tile: u32,
    cols: u32,
    rows: u32,
  ) -> Tileset {
    let (w, h) = (tile * cols, tile * rows);
    let mut data = Vec::new();
    for y in 0..h {
      for x in 0..w {
        let id = (y / tile) * cols + x / tile;
        data.extend_from_slice(&[(x % tile) as u8, (y % tile) as u8, id as u8, 255]);
      }
    }
    let image = images.add(Image::from_rgba(data, w, h));
    let mut t = Tileset::new(
      "marked",
      UVec2::splat(tile),
      TilesetImage::Atlas {
        image,
        size: UVec2::new(w, h),
      },
    );
    t.columns = cols;
    t.tile_count = cols * rows;
    t
  }

  fn texel(image: &Image, x: u32, y: u32) -> [u8; 4] {
    let i = ((y * image.width() + x) * 4) as usize;
    image.data()[i..i + 4].try_into().expect("four bytes")
  }

  #[test]
  fn a_padded_tileset_extrudes_every_tile_into_its_own_cell() {
    let mut images = Assets::new();
    let t = marked_atlas(&mut images, 16, 2, 2);
    let (image, layout) = pad_tileset(&t, &images).expect("padded");
    assert_eq!(layout.cell, 32);
    assert_eq!(layout.columns, 2);
    // Two cells a side, and 24 texels on each side (24 and a texture's 8
    // of border make a whole cell)
    assert_eq!(layout.margin, 24);
    assert_eq!(layout.size, UVec2::splat(112));
    assert_eq!((image.width(), image.height()), (112, 112));
    for id in 0..4u32 {
      let slot = layout.tile(id).expect("tile").slot;
      let o = layout.cell_origin(slot);
      // The tile sits centred: 8 texels in
      assert_eq!(texel(&image, o.x + 8, o.y + 8), [0, 0, id as u8, 255]);
      assert_eq!(texel(&image, o.x + 23, o.y + 23), [15, 15, id as u8, 255]);
      // The border repeats the edge texels, corners included
      assert_eq!(texel(&image, o.x, o.y), [0, 0, id as u8, 255]);
      assert_eq!(texel(&image, o.x + 31, o.y), [15, 0, id as u8, 255]);
      assert_eq!(texel(&image, o.x, o.y + 31), [0, 15, id as u8, 255]);
      assert_eq!(texel(&image, o.x + 31, o.y + 31), [15, 15, id as u8, 255]);
      assert_eq!(texel(&image, o.x + 3, o.y + 12), [0, 4, id as u8, 255]);
      // Every texel of the cell belongs to this tile alone
      for y in 0..32 {
        for x in 0..32 {
          assert_eq!(texel(&image, o.x + x, o.y + y)[2], id as u8);
        }
      }
    }
  }

  #[test]
  fn cells_are_whole_texels_of_every_mip_level_behind_a_texture_s_border() {
    // A texture of its own puts the atlas's border around the image: each
    // cell must still start on a whole texel of every level down to one
    // texel a cell, or zoomed out the levels mix neighbours into a tile.
    // And the texture's sides must be whole cells: the GPU sizes a level
    // by rounding down, so a level of another size would not line up with
    // the full-size one
    let mut images = Assets::new();
    for (tile, cols) in [(16, 10), (8, 3), (30, 2), (2, 2)] {
      let t = marked_atlas(&mut images, tile, cols, cols);
      let (image, layout) = pad_tileset(&t, &images).expect("padded");
      let size = UVec2::new(image.width(), image.height());
      assert_eq!(size, layout.size);
      let texture = size + 2 * BORDER;
      assert_eq!(
        texture % layout.cell,
        UVec2::ZERO,
        "{tile} px tiles: {size}"
      );
      for slot in 0..cols * cols {
        let o = layout.cell_origin(slot);
        let behind = o + UVec2::splat(BORDER);
        assert_eq!(
          behind % layout.cell,
          UVec2::ZERO,
          "{tile} px tiles, cell {slot} at {o}"
        );
        assert!((o + UVec2::splat(layout.cell)).cmple(size).all());
      }
    }
  }

  #[test]
  fn a_tile_of_any_size_gets_a_cell_with_room_for_every_mip_level() {
    // Level k of the image samples only a tile's own cell while 2^k is at
    // most the cell's room past the tile: at least the tile's side (every
    // level a tile drawn a pixel wide or more uses) and at least 8 (every
    // level of an atlas page)
    let mut images = Assets::new();
    for side in 1..=70u32 {
      let t = marked_atlas(&mut images, side, 1, 1);
      let (_, layout) = pad_tileset(&t, &images).expect("padded");
      let room = layout.cell - side;
      assert!(layout.cell.is_power_of_two(), "{side} px");
      assert!(
        room >= side && room >= 8,
        "{side} px tiles in {} px cells",
        layout.cell
      );
      // And no bigger than that asks
      assert!(layout.cell / 2 < (2 * side).max(side + 8), "{side} px");
    }
    // Powers of two from 8 up keep cells twice their side, as before
    for (side, cell) in [
      (14, 32),
      (15, 32),
      (16, 32),
      (30, 64),
      (32, 64),
      (8, 16),
      (4, 16),
    ] {
      let t = marked_atlas(&mut images, side, 1, 1);
      let (_, layout) = pad_tileset(&t, &images).expect("padded");
      assert_eq!(layout.cell, cell, "{side} px");
    }
  }

  #[test]
  fn a_collection_packs_each_tile_image_the_same_way() {
    let mut images = Assets::new();
    let a = images.add(Image::solid_rect(16, 16, 255, 0, 0, 255));
    let b = images.add(Image::solid_rect(8, 24, 0, 255, 0, 255));
    let mut t = Tileset::new("c", UVec2::splat(16), TilesetImage::Collection);
    t.tile_count = 6;
    t.insert_tile(
      2,
      TileData {
        image: Some((a, UVec2::splat(16))),
        ..TileData::default()
      },
    );
    t.insert_tile(
      5,
      TileData {
        image: Some((b, UVec2::new(8, 24))),
        ..TileData::default()
      },
    );
    let (image, layout) = pad_tileset(&t, &images).expect("padded");
    // The longest side, 24, takes a 64 px cell (twice it, rounded up)
    assert_eq!(layout.cell, 64);
    assert!(layout.tile(0).is_none() && layout.tile(3).is_none());
    assert_eq!(layout.tile(5).expect("tile").size, UVec2::new(8, 24));
    let o = layout.cell_origin(layout.tile(5).expect("tile").slot);
    assert_eq!(texel(&image, o.x, o.y), [0, 255, 0, 255]);
    assert_eq!(texel(&image, o.x + 63, o.y + 63), [0, 255, 0, 255]);
    let o = layout.cell_origin(layout.tile(2).expect("tile").slot);
    assert_eq!(texel(&image, o.x + 63, o.y), [255, 0, 0, 255]);
  }

  #[test]
  fn nothing_to_pad_is_none() {
    let images = Assets::new();
    let t = Tileset::new("e", UVec2::splat(16), TilesetImage::Collection);
    assert!(pad_tileset(&t, &images).is_none());
  }

  #[test]
  fn an_animation_lays_its_frames_out_in_consecutive_cells() {
    let mut images = Assets::new();
    let mut t = marked_atlas(&mut images, 16, 2, 2);
    let ms = |tile, ms| TileFrame {
      tile,
      duration: Duration::from_millis(ms),
    };
    t.insert_tile(
      0,
      TileData {
        animation: vec![ms(1, 100), ms(2, 100), ms(3, 100)],
        ..TileData::default()
      },
    );
    let (image, layout) = pad_tileset(&t, &images).expect("padded");
    let anim = layout.tile(0).and_then(|t| t.animation).expect("animated");
    assert_eq!(anim.frames, 3);
    assert!((anim.frame_duration - 0.1).abs() < 1e-6);
    for (k, want) in [1u8, 2, 3].into_iter().enumerate() {
      let o = layout.cell_origin(anim.first_slot + k as u32);
      assert_eq!(texel(&image, o.x + 8, o.y + 8)[2], want);
    }
    // The static tiles are still there, and the image holds every cell
    assert!(layout.size.x * layout.size.y >= 7 * 32 * 32);
  }

  #[test]
  fn unequal_frame_times_repeat_frames_to_a_common_beat() {
    let mut images = Assets::new();
    let mut t = marked_atlas(&mut images, 16, 2, 2);
    let ms = |tile, ms| TileFrame {
      tile,
      duration: Duration::from_millis(ms),
    };
    t.insert_tile(
      0,
      TileData {
        animation: vec![ms(1, 100), ms(2, 300)],
        ..TileData::default()
      },
    );
    let (image, layout) = pad_tileset(&t, &images).expect("padded");
    let anim = layout.tile(0).and_then(|t| t.animation).expect("animated");
    assert_eq!(anim.frames, 4);
    assert!((anim.frame_duration - 0.1).abs() < 1e-6);
    let shown: Vec<u8> = (0..4)
      .map(|k| {
        let o = layout.cell_origin(anim.first_slot + k);
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
    let o = layout.cell_origin(layout.tile(0).expect("tile").slot);
    let img = images.get(&third).expect("padded image");
    assert_eq!(texel(img, o.x, o.y)[2], 99);

    // A round that does not ask frees it
    padded.begin_round();
    padded.end_round();
    drop((first, again, third));
    images.free_unheld(&mut Vec::new());
    assert_eq!(images.len(), count);
  }

  #[test]
  fn a_cache_hit_allocates_nothing() {
    let mut tilesets = Assets::<Tileset>::new();
    let mut images = Assets::new();
    let atlas = marked_atlas(&mut images, 16, 2, 2);
    let a = images.add(Image::solid_rect(16, 16, 255, 0, 0, 255));
    let b = images.add(Image::solid_rect(8, 24, 0, 255, 0, 255));
    let mut collection = Tileset::new("c", UVec2::splat(16), TilesetImage::Collection);
    collection.tile_count = 40;
    for id in 0..40 {
      let (handle, size) = if id % 2 == 0 {
        (a.clone(), UVec2::splat(16))
      } else {
        (b.clone(), UVec2::new(8, 24))
      };
      collection.insert_tile(
        id,
        TileData {
          image: Some((handle, size)),
          ..TileData::default()
        },
      );
    }
    let sources = [
      TilesetSource::Asset(tilesets.add(atlas)),
      TilesetSource::Asset(tilesets.add(collection.clone())),
      TilesetSource::Embedded(Arc::new(collection)),
    ];
    let mut padded = PaddedTilesets::default();
    for round in 0..2 {
      padded.begin_round();
      for source in &sources {
        assert!(
          padded.get(source, &tilesets, &mut images).is_some(),
          "round {round}"
        );
      }
      padded.end_round();
    }
    // Warm: every further round is a hit
    let before = allocations();
    for _ in 0..10 {
      padded.begin_round();
      for source in &sources {
        // Handle and layout clones are reference counts
        drop(padded.get(source, &tilesets, &mut images));
      }
      padded.end_round();
    }
    assert_eq!(allocations(), before, "a cache hit allocated");
  }
}
