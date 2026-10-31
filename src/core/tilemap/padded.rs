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
//!
//! Public but hidden from the docs: `tests/tilemap_alloc.rs` and
//! `benches/tilemap.rs` drive the cache from outside the crate.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, Weak};

use glam::{IVec2, UVec2};

use super::map::TilesetSource;
use super::tileset::{Tileset, TilesetImage, URect};
use crate::core::asset::{AssetId, Assets, Handle};
use crate::core::ecs::Resource;
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
/// Rounds (frames) a padding no map asked for is kept, as long as the
/// batch keeps an undrawn chunk (`chunks::KEEP_FRAMES`)
const KEEP_ROUNDS: u64 = crate::core::render::batch::KEEP_FRAMES;

/// How a tileset's tiles sit in its padded image
#[derive(Clone, Debug)]
pub struct PaddedLayout {
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
pub struct PaddedTile {
  /// The tile's cell, counted row-major from the top left
  pub slot: u32,
  /// The tile's size in texels, as drawn unflipped
  pub size: UVec2,
  pub animation: Option<PaddedAnimation>,
}

/// An animated tile's frames: `frames` cells from `first_slot`, row-major,
/// wrapping rows, each shown for `frame_duration` seconds
#[derive(Clone, Copy, Debug)]
pub struct PaddedAnimation {
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
}

fn source<'a>(tileset: &Tileset, images: &'a impl Pixels, id: u32) -> Option<Source<'a>> {
  match &tileset.image {
    TilesetImage::Atlas { image, .. } => Some(Source {
      image: images.image(image)?,
      rect: tileset.tile_rect(id)?,
    }),
    TilesetImage::Collection => {
      let (handle, size) = tileset.tile(id)?.image.as_ref()?;
      Some(Source {
        image: images.image(handle)?,
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
  /// Its `cells` cells of `cell` texels would make an image of `size`
  /// texels, over `MAX_SIDE` on a side
  TooBig {
    cells: u64,
    cell: u32,
    size: (u64, u64),
  },
  /// Its largest tile, `side` texels, is too large for a cell at all
  TileTooBig { side: u32 },
}

/// Pads `tileset`'s tiles; `Unpadded::Nothing` when it has nothing to draw
/// yet (no tile with a picture, or its atlas image not loaded), and
/// `Unpadded::TooBig` when its padded image would be over `MAX_SIDE`
/// texels on a side
pub(crate) fn pad_tileset(
  tileset: &Tileset,
  images: &impl Pixels,
) -> Result<(Image, PaddedLayout), Unpadded> {
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
  let ids: Vec<(u32, UVec2)> = ids
    .into_iter()
    .filter(|&id| source(tileset, images, id).is_some())
    .filter_map(|id| Some((id, tile_size(tileset, id)?)))
    .collect();
  let side = ids
    .iter()
    .map(|(_, s)| s.x.max(s.y))
    .max()
    .ok_or(Unpadded::Nothing)?;
  let room = side.max(MIN_ROOM);
  let cell = side
    .checked_add(room)
    .and_then(u32::checked_next_power_of_two)
    .ok_or(Unpadded::TileTooBig { side })?;

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
    return Err(Unpadded::TooBig {
      cells: n,
      cell,
      size: (side(columns), side(rows)),
    });
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
  Ok((Image::from_rgba(data, size.x, size.y), layout))
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
#[derive(Default)]
pub struct PaddedTilesets {
  entries: FastMap<Key, Entry>,
  too_big: FastMap<Key, Refused>,
  /// The round under way, counted from 1
  round: u64,
}

/// The padded tilesets as the world holds them, which the map drawer
/// inserts and locks each frame, and landings lock to look and to hand
/// paddings over: a lock, where taking a resource out of the world and
/// putting it back each frame moves an entity's components twice
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
    let (mut image, mut layout) = pad_tileset(tileset, &self.pixels).ok()?;
    layout.serial = SERIALS.fetch_add(1, Ordering::Relaxed);
    image.prepare_upload();
    let holds = match &self.tileset {
      Copied::Asset(_) => None,
      Copied::Embedded(tileset) => Some(Arc::downgrade(tileset)),
    };
    Some(Prepared {
      key: self.key,
      stamp: self.stamp,
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
    self.too_big.remove(&prepared.key);
    self.entries.insert(
      prepared.key,
      Entry {
        stamp: prepared.stamp,
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
    let (image, mut layout) = match pad_tileset(tileset, images) {
      Ok(made) => made,
      Err(why) => {
        // Nothing to draw (yet), or too big: what was made before is stale
        self.entries.remove(&key);
        let refused = match why {
          Unpadded::Nothing => false,
          Unpadded::TooBig { cells, cell, size } => {
            crate::engine_warn!(
              module: "Tilemap",
              "tileset \"{}\" is too big to pad: its {cells} cells of {cell} texels need a padded image of {} x {} texels, over the {MAX_SIDE} a side it may span, so its tiles draw nothing (a tile's cell is the power of two at or above twice its tileset's largest tile side, and at least that side plus 8)",
              tileset.name,
              size.0,
              size.1
            );
            true
          }
          Unpadded::TileTooBig { side } => {
            crate::engine_warn!(
              module: "Tilemap",
              "tileset \"{}\" is too big to pad: its largest tile is {side} texels a side, too large for a padded cell (twice its side, rounded up to a power of two, past {} texels), so its tiles draw nothing",
              tileset.name,
              u32::MAX
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
    assert!(matches!(pad_tileset(&t, &images), Err(Unpadded::Nothing)));
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
  fn a_tileset_too_big_to_pad_draws_nothing_and_warns_once() {
    // 16,900 tiles of 64 px: cells of 128 texels, 130 a side, over the
    // 16,384 texels a padded image may span. The atlas image itself is
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
    assert!(said[0].contains("3000000000 texels a side"), "{said:?}");
  }
}
