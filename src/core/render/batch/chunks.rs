//! Retained tile chunks. Each chunk of a tile layer keeps its instances, a
//! range per tileset in Tiled's paint order (so each paint band of cells, a
//! row, is a run of it, for y-sorted layers), in one arena the GPU holds a
//! copy of: built once from its tiles, written to the GPU once, and built
//! and written again only when what it was built from changes. A chunk with
//! a tile that leaves its cell's box keeps its paint order too, as runs of
//! one tileset's instances, so a layer whose tiles overlap can draw them in
//! Tiled's order across tilesets and chunks. A draw of a chunk binds a slot (group 2,
//! at a dynamic offset) that places it: the layer's world transform, the
//! camera's parallax offset and where the tileset's padded image sits on its
//! page, so moving the map, moving the camera or the atlas moving the image
//! never rebuilds a chunk.
//!
//! [`ChunkStore`] is the CPU side, filled while the frame is gathered;
//! [`ChunkBuffers`] the GPU side, written in `Batch2D::prepare`.

use std::ops::Range;

use glam::{IVec2, Vec2};

use crate::core::ecs::Entity;
use crate::core::graphics::{SpriteGpuRenderer, SpriteInstance, TextureId};
use crate::core::hash::FastMap;

/// Frames a chunk stays built while nothing draws it
pub(crate) const KEEP_FRAMES: u64 = 120;

/// Where each slot starts in the slot buffer: WebGPU's largest minimum
/// uniform offset alignment
pub(crate) const SLOT_STRIDE: u32 = 256;

/// Slots the slot buffer holds at first; it doubles when a frame needs more
const FIRST_SLOTS: usize = 16;

/// Instances the arena's buffer holds at first; it doubles when the arena
/// outgrows it, up to the arena's cap
const FIRST_INSTANCES: usize = 1024;

/// The most instances the arena holds: as many as the GPU's largest buffer
/// (`max_buffer_size`) fits, and never past what a `u32` counts
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Cap(usize);

impl Cap {
  /// The cap of a device whose largest buffer is `max_buffer_size` bytes
  fn of(max_buffer_size: u64) -> Self {
    let instances = max_buffer_size / std::mem::size_of::<SpriteInstance>() as u64;
    Self(usize::try_from(instances.min(u64::from(u32::MAX))).unwrap_or(u32::MAX as usize))
  }
}

impl Default for Cap {
  /// The limits the engine asks its device for
  fn default() -> Self {
    Self::of(wgpu::Limits::default().max_buffer_size)
  }
}

/// One tileset's tiles of one chunk of a tile layer: a range of the arena
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct ChunkKey {
  pub layer: Entity,
  /// Signed, as `TileGrid::chunk_of` gives it
  pub chunk: IVec2,
  pub tileset: u16,
}

/// What a chunk was built from: its grid's version of it, and a stamp of
/// everything else its instances depend on (the layer's look and cell size,
/// the tilesets' padded layouts)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ChunkStamp {
  pub version: u64,
  pub look: u64,
}

/// One tileset's instances of a chunk: where they are in the arena, and the
/// box they cover in the layer's pixels
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ChunkPart {
  pub tileset: u16,
  pub start: u32,
  pub count: u32,
  pub min: Vec2,
  pub max: Vec2,
}

/// A cell's place in Tiled's paint order (`MapGrid::paint_key`): its paint
/// band, then its column
pub(crate) type PaintKey = (i32, i32, i32);

/// A paint band: the first two parts of a paint key, the cells' row (on an
/// isometric grid their diagonal, x + y) and, on axis-X staggered and
/// hexagonal grids, 1 for the row's shifted cells (0 for the rest, and on
/// every other grid)
pub(crate) type Band = (i32, i32);

/// One instance a chunk is built from: its tileset, its place in Tiled's
/// paint order, the instance, and where it sorts (in the layer's pixels)
/// when that is not its band's sort point, as on a y-sorted layer
pub(crate) type Planted = (u16, PaintKey, SpriteInstance, Option<f32>);

/// A paint key's band
fn band((row, phase, _): PaintKey) -> Band {
  (row, phase)
}

/// How a chunk's tiles leave their cells' boxes (each cell's outline's
/// bounding box), as pixel art draws them
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Spill {
  /// None does: a tile overlaps nothing but where its cell's box overlaps
  /// its neighbours'
  #[default]
  None,
  /// Some reach above or below their cell's box, none past its sides: a
  /// band's cells' boxes lie side by side, so its tiles never overlap one
  /// another, and only the bands' order shows
  Vertical,
  /// Some reach past their cell's box's sides (wide, moved across, or
  /// turned): a band's tiles overlap one another, and their order shows
  Sideways,
}

/// A run of one tileset's part of a chunk: instances next to each other in
/// the part (which is in paint order), all of one paint band, and the box
/// they cover in the layer's pixels. A chunk's rows are each tileset's band
/// as one run, which a y-sorted layer draws on its own; its runs are its
/// instances in paint order, a run where the tileset changes or the band
/// ends
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ChunkRun {
  pub tileset: u16,
  /// The cells' paint band in the layer (signed)
  pub band: Band,
  /// The paint column of its first cell (the last part of its paint key)
  pub column: i32,
  /// Where the run starts in its part, and how many instances it has
  pub first: u32,
  pub count: u32,
  pub min: Vec2,
  pub max: Vec2,
  /// Where a y-sorted layer sorts the run, in the layer's pixels, when it
  /// is not its band's sort point: the run is then one instance
  pub sort: Option<f32>,
}

/// Where [`ChunkStore::chunk`] copies a chunk as a view draws it: each
/// tileset's part, and, if asked for, its rows (band by band, each band's
/// tilesets in order) and its runs in paint order (a chunk whose tiles stay
/// inside their cells' boxes' sides gives its rows, as a band's tiles then
/// never overlap one another)
pub(crate) struct ChunkCopy<'a> {
  pub parts: &'a mut Vec<ChunkPart>,
  pub rows: Option<&'a mut Vec<ChunkRun>>,
  pub runs: Option<&'a mut Vec<ChunkRun>>,
}

impl ChunkCopy<'_> {
  /// Copies `built`'s parts, and rows and runs if asked for, over what was
  /// there; how its tiles leave their cells' boxes
  fn take(self, built: &Built) -> Spill {
    self.parts.clear();
    self.parts.extend_from_slice(&built.parts);
    if let Some(rows) = self.rows {
      rows.clear();
      rows.extend_from_slice(&built.rows);
    }
    if let Some(runs) = self.runs {
      runs.clear();
      runs.extend_from_slice(if built.spill == Spill::Sideways {
        &built.runs
      } else {
        &built.rows
      });
    }
    built.spill
  }
}

/// One draw's slot, as `tile.wgsl` reads it (`TileSlot`)
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Default, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct TileSlot {
  /// The layer's world transform: its x and y axes, then its origin
  pub x_axis: [f32; 2],
  pub y_axis: [f32; 2],
  pub origin: [f32; 2],
  /// The camera's parallax offset for the layer, in world units
  pub parallax: [f32; 2],
  /// Where the padded image sits on its page: UV offset, then scale
  /// (filled in `prepare`, once the image is resident)
  pub uv: [f32; 4],
}

/// A slot as the slot buffer holds it, one per [`SLOT_STRIDE`] bytes
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct SlotBlock {
  slot: TileSlot,
  _pad: [f32; 52],
}

const _: () = assert!(std::mem::size_of::<SlotBlock>() == SLOT_STRIDE as usize);

/// A chunk as built
#[derive(Debug, Default)]
struct Built {
  /// `None` until first built
  stamp: Option<ChunkStamp>,
  /// The last frame it was drawn in
  drawn: u64,
  /// The last view it was handed to
  view: u64,
  /// What it was last refused room for: the stamp it was to be built from
  /// and the instances that made. It is not built again from that stamp
  /// until the arena has room for them
  refused: Option<(ChunkStamp, u32)>,
  parts: Vec<ChunkPart>,
  /// Each tileset's bands, band by band
  rows: Vec<ChunkRun>,
  /// How its tiles leave their cells' boxes
  spill: Spill,
  /// Its instances in paint order, as runs (only when a tile of it leaves
  /// its cell's box's sides)
  runs: Vec<ChunkRun>,
}

/// The CPU side of the retained chunks: each layer's built chunks, the
/// arena they are in (and what of it changed since the GPU was written),
/// and the frame's slots.
///
/// The arena holds at most as many instances as the GPU's largest buffer
/// fits (its cap: about two million under the engine's limits). A chunk
/// that would take it past that frees the chunks drawn least recently
/// (none drawn this frame); when that is not enough it is not built, and
/// draws nothing until there is room, and the dev log warns once
#[derive(Default)]
pub(crate) struct ChunkStore {
  layers: FastMap<Entity, FastMap<IVec2, Built>>,
  /// The arena: every chunk's instances, as the GPU is to hold them
  held: Vec<SpriteInstance>,
  /// Free ranges of the arena below its end, by start, none touching
  free: Vec<Range<u32>>,
  /// Ranges written since the GPU last was
  written: Vec<Range<u32>>,
  /// A chunk's instances as built (tileset, paint key, instance), in
  /// paint order, before they are split by tileset
  scratch: Vec<Planted>,
  /// The frame's slots, and the texture each one's UVs come from
  slots: Vec<TileSlot>,
  slot_textures: Vec<TextureId>,
  frame: u64,
  views: u64,
  rebuilds: u64,
  cap: Cap,
  /// Chunks that may be freed to make room, newest first (each a chunk not
  /// drawn in the frame `evictable_frame`, when the list was made), as
  /// `(last drawn, layer, chunk)`
  evictable: Vec<(u64, Entity, IVec2)>,
  evictable_frame: u64,
  /// Whether the dev log was told the arena is full
  warned: bool,
}

impl ChunkStore {
  /// Starts a frame's chunks: the last frame's slots go
  pub(crate) fn begin_frame(&mut self) {
    self.frame += 1;
    self.slots.clear();
    self.slot_textures.clear();
  }

  /// A new view: one draw of a layer, through one slot, seen by one or more
  /// cameras. [`chunk`](Self::chunk) hands a chunk to a view once
  pub(crate) fn begin_view(&mut self) -> u64 {
    self.views += 1;
    self.views
  }

  /// The store of a device whose largest buffer is `max_buffer_size`
  /// bytes: its arena holds no more than that buffer does
  pub(crate) fn for_device(max_buffer_size: u64) -> Self {
    Self {
      cap: Cap::of(max_buffer_size),
      ..Self::default()
    }
  }

  /// Copies into `out` the parts, rows and runs of `layer`'s chunk `chunk`
  /// for `view`, built by `build` first (it fills the instances, as
  /// `(tileset, paint key, instance)` in paint order, and says how they
  /// leave their cells' boxes) when `stamp` is not what it was built from;
  /// then how its tiles leave their cells' boxes. `None`, copying nothing,
  /// when the view had it already, or the arena has no room for it. The
  /// chunk counts as drawn this frame
  pub(crate) fn chunk(
    &mut self,
    (layer, chunk): (Entity, IVec2),
    stamp: ChunkStamp,
    view: u64,
    build: impl FnOnce(&mut Vec<Planted>) -> Spill,
    out: ChunkCopy<'_>,
  ) -> Option<Spill> {
    let frame = self.frame;
    let built = self
      .layers
      .entry(layer)
      .or_default()
      .entry(chunk)
      .or_default();
    if built.view == view {
      return None;
    }
    built.view = view;
    built.drawn = frame;
    if built.stamp == Some(stamp) {
      return Some(out.take(built));
    }
    built.stamp = None;
    for part in built.parts.drain(..) {
      release(
        &mut self.held,
        &mut self.free,
        part.start..part.start + part.count,
      );
    }
    built.rows.clear();
    built.runs.clear();
    built.spill = Spill::None;
    // Refused room for this stamp before: not built again until there is
    // room for as many instances
    if let Some((_, count)) = built.refused.filter(|(refused, _)| *refused == stamp)
      && !self.make_room(count)
    {
      return None;
    }
    self.scratch.clear();
    let spill = build(&mut self.scratch);
    // A chunk makes at most an instance a cell
    let count = u32::try_from(self.scratch.len()).unwrap_or(u32::MAX);
    let Some(start) = self.allocate(count) else {
      self.refuse(layer, chunk, (stamp, count));
      return None;
    };
    let Self {
      layers,
      held,
      written,
      scratch,
      rebuilds,
      ..
    } = self;
    // Drawn this frame, so no room made took it
    let built = layers
      .get_mut(&layer)
      .and_then(|chunks| chunks.get_mut(&chunk))?;
    built.stamp = Some(stamp);
    built.refused = None;
    built.spill = spill;
    *rebuilds += 1;
    // A chunk whose tiles overlap within a band keeps its paint order: runs
    // of one tileset's instances, each counted from its part's start
    if spill == Spill::Sideways {
      paint_runs(scratch, &mut built.runs);
    }
    // Stable, so each tileset keeps its paint order. A chunk of one tileset
    // (or already in order) needs none: the sort would take a scratch buffer
    // of the chunk's size (about 140 KB for a full chunk) to move nothing
    if !scratch.is_sorted_by_key(|(tileset, ..)| *tileset) {
      scratch.sort_by_key(|(tileset, ..)| *tileset);
    }
    let mut at = start;
    for run in scratch.chunk_by(|a, b| a.0 == b.0) {
      let tileset = run[0].0;
      let count = run.len() as u32;
      let start = at;
      at += count;
      for (slot, (_, _, instance, _)) in held[start as usize..].iter_mut().zip(run) {
        *slot = *instance;
      }
      // Each band's run of the part, and the box of each
      let (mut min, mut max) = (Vec2::splat(f32::MAX), Vec2::splat(f32::MIN));
      let mut first = 0;
      // An instance with a sort point of its own is a run alone
      for cells in run.chunk_by(|a, b| band(a.1) == band(b.1) && a.3.is_none() && b.3.is_none()) {
        let (mut low, mut high) = (Vec2::splat(f32::MAX), Vec2::splat(f32::MIN));
        for (_, _, instance, _) in cells {
          let (a, b) = reach(instance);
          low = low.min(a);
          high = high.max(b);
        }
        built.rows.push(ChunkRun {
          tileset,
          band: band(cells[0].1),
          column: cells[0].1.2,
          first,
          count: cells.len() as u32,
          min: low,
          max: high,
          sort: cells[0].3,
        });
        first += cells.len() as u32;
        min = min.min(low);
        max = max.max(high);
      }
      written.push(start..start + count);
      built.parts.push(ChunkPart {
        tileset,
        start,
        count,
        min,
        max,
      });
    }
    // Band by band, so a band's runs are together (a chunk of one tileset
    // has them so already)
    if !built
      .rows
      .is_sorted_by_key(|row| (row.band, row.tileset, row.first))
    {
      built
        .rows
        .sort_unstable_by_key(|row| (row.band, row.tileset, row.first));
    }
    Some(out.take(built))
  }

  /// Room for `count` instances in the arena, its start, freeing the chunks
  /// drawn least recently until there is: `None` when freeing every chunk
  /// not drawn this frame is not enough
  fn allocate(&mut self, count: u32) -> Option<u32> {
    loop {
      if let Some(start) = allocate(&mut self.held, &mut self.free, count, self.cap) {
        return Some(start);
      }
      if !self.evict_oldest() {
        return None;
      }
    }
  }

  /// Whether the arena has room for `count` instances, once the chunks drawn
  /// least recently are freed until it does
  fn make_room(&mut self, count: u32) -> bool {
    loop {
      if has_room(&self.held, &self.free, count, self.cap) {
        return true;
      }
      if !self.evict_oldest() {
        return false;
      }
    }
  }

  /// Frees the chunk drawn least recently, of those not drawn this frame
  /// (whose draws are already made); false when there is none
  fn evict_oldest(&mut self) -> bool {
    let frame = self.frame;
    if self.evictable_frame != frame {
      self.evictable_frame = frame;
      self.evictable.clear();
      for (&layer, chunks) in &self.layers {
        for (&chunk, built) in chunks {
          if built.drawn < frame && !built.parts.is_empty() {
            self.evictable.push((built.drawn, layer, chunk));
          }
        }
      }
      // Newest first, so the oldest comes off the end
      self
        .evictable
        .sort_unstable_by_key(|&(drawn, ..)| std::cmp::Reverse(drawn));
    }
    while let Some((_, layer, chunk)) = self.evictable.pop() {
      let Some(chunks) = self.layers.get_mut(&layer) else {
        continue;
      };
      // Drawn again since the list was made, or gone
      if chunks.get(&chunk).is_none_or(|built| built.drawn >= frame) {
        continue;
      }
      if let Some(built) = chunks.remove(&chunk) {
        for part in built.parts {
          release(
            &mut self.held,
            &mut self.free,
            part.start..part.start + part.count,
          );
        }
      }
      return true;
    }
    false
  }

  /// Records that `layer`'s chunk `chunk` found no room for `wanted` (its
  /// stamp, its instances), and warns the first time the arena is full
  fn refuse(&mut self, layer: Entity, chunk: IVec2, wanted: (ChunkStamp, u32)) {
    if let Some(built) = self
      .layers
      .get_mut(&layer)
      .and_then(|chunks| chunks.get_mut(&chunk))
    {
      built.refused = Some(wanted);
    }
    if !self.warned {
      self.warned = true;
      let size = std::mem::size_of::<SpriteInstance>();
      crate::engine_warn!(
        module: "Tilemap",
        "the tile chunk arena is full: the chunks in view, and those drawn in the last {KEEP_FRAMES} frames, need more than the {} tile instances ({} MiB) the GPU's largest buffer holds, so the chunks past that draw nothing until there is room (every camera's chunks share this arena: zoom in or show a smaller view, show fewer tile layers to the wide camera with RenderLayers, or draw an overview from a picture of the map)",
        self.cap.0,
        self.cap.0 * size / (1024 * 1024)
      );
    }
  }

  /// Adds a slot for this frame, its UVs to come from `texture`'s place,
  /// and returns its index
  pub(crate) fn push_slot(&mut self, slot: TileSlot, texture: TextureId) -> u32 {
    self.slots.push(slot);
    self.slot_textures.push(texture);
    (self.slots.len() - 1) as u32
  }

  /// Ends the frame's chunks: every chunk of a layer `alive` turns down,
  /// and every chunk not drawn for [`KEEP_FRAMES`], is freed
  pub(crate) fn end_frame(&mut self, mut alive: impl FnMut(Entity) -> bool) {
    let Self {
      layers,
      held,
      free,
      frame,
      ..
    } = self;
    let frame = *frame;
    layers.retain(|&layer, chunks| {
      let alive = alive(layer);
      chunks.retain(|_, built| {
        let keep = alive && frame - built.drawn <= KEEP_FRAMES;
        if !keep {
          for part in built.parts.drain(..) {
            release(held, free, part.start..part.start + part.count);
          }
        }
        keep
      });
      !chunks.is_empty()
    });
  }

  /// The arena range of `key`'s instances
  pub(crate) fn range(&self, key: ChunkKey) -> Option<Range<u32>> {
    let built = self.layers.get(&key.layer)?.get(&key.chunk)?;
    let part = built.parts.iter().find(|p| p.tileset == key.tileset)?;
    Some(part.start..part.start + part.count)
  }

  /// The chunks built, each with its layer, in no set order (the headless
  /// engine's reads)
  #[cfg(not(target_arch = "wasm32"))]
  pub(crate) fn built_chunks(&self) -> impl Iterator<Item = (Entity, IVec2)> + '_ {
    self.layers.iter().flat_map(|(&layer, chunks)| {
      chunks
        .iter()
        .filter(|(_, built)| built.stamp.is_some())
        .map(move |(&chunk, _)| (layer, chunk))
    })
  }

  /// How many times a chunk has been built (again)
  #[cfg(test)]
  pub(crate) fn rebuilds(&self) -> u64 {
    self.rebuilds
  }

  /// How many chunks are built
  #[cfg(test)]
  pub(crate) fn chunk_count(&self) -> usize {
    self
      .layers
      .values()
      .flat_map(FastMap::values)
      .filter(|built| built.stamp.is_some())
      .count()
  }

  /// Sets the most instances the arena may hold (tests)
  #[cfg(test)]
  pub(crate) fn set_cap(&mut self, instances: usize) {
    self.cap = Cap(instances);
  }

  /// How many times the dev log was told the arena is full (tests)
  #[cfg(test)]
  pub(crate) fn full_warnings(&self) -> usize {
    usize::from(self.warned)
  }

  /// How many instances the built chunks hold
  #[cfg(test)]
  pub(crate) fn instances_in_use(&self) -> usize {
    let free: usize = self.free.iter().map(|r| r.len()).sum();
    self.held.len() - free
  }
}

/// `scratch`'s instances, in paint order, as runs into `runs`: a run ends
/// where the tileset changes or the band does, and starts where its first
/// instance will be in its tileset's part (the parts keep paint order)
fn paint_runs(scratch: &[Planted], runs: &mut Vec<ChunkRun>) {
  // How many of each tileset's instances come before; a map holds at most
  // 256 tilesets
  let mut placed = [0u32; 256];
  for &(tileset, key, ref instance, _) in scratch {
    let Some(at) = placed.get_mut(usize::from(tileset)) else {
      continue;
    };
    let first = *at;
    *at += 1;
    let (low, high) = reach(instance);
    match runs.last_mut() {
      Some(run) if run.tileset == tileset && run.band == band(key) => {
        run.count += 1;
        run.min = run.min.min(low);
        run.max = run.max.max(high);
      }
      _ => runs.push(ChunkRun {
        tileset,
        band: band(key),
        column: key.2,
        first,
        count: 1,
        min: low,
        max: high,
        sort: None,
      }),
    }
  }
}

/// The box an instance's quad covers: its centre give or take half of each
/// axis' reach
fn reach(instance: &SpriteInstance) -> (Vec2, Vec2) {
  let [x, y, at] = [
    instance.transform[0],
    instance.transform[1],
    instance.transform[2],
  ];
  let half = Vec2::new(x[0].abs() + y[0].abs(), x[1].abs() + y[1].abs()) * 0.5;
  let centre = Vec2::new(at[0], at[1]);
  (centre - half, centre + half)
}

/// Room for `count` instances in the arena: the first free range that holds
/// them, else past its end; `None` when that would take the arena past
/// `cap`
fn allocate(
  held: &mut Vec<SpriteInstance>,
  free: &mut Vec<Range<u32>>,
  count: u32,
  cap: Cap,
) -> Option<u32> {
  if let Some(i) = free.iter().position(|r| r.len() as u32 >= count) {
    let start = free[i].start;
    free[i].start += count;
    if free[i].is_empty() {
      free.remove(i);
    }
    return Some(start);
  }
  let end = held.len().checked_add(count as usize)?;
  if end > cap.0 {
    return None;
  }
  // Doubling, but never holding room past the cap
  if end > held.capacity() {
    let room = end.max(held.capacity() * 2).min(cap.0);
    held.reserve_exact(room - held.len());
  }
  let start = held.len() as u32;
  held.resize(end, bytemuck::Zeroable::zeroed());
  Some(start)
}

/// Whether [`allocate`] would find room for `count` instances
fn has_room(held: &[SpriteInstance], free: &[Range<u32>], count: u32, cap: Cap) -> bool {
  free.iter().any(|r| r.len() as u32 >= count)
    || held
      .len()
      .checked_add(count as usize)
      .is_some_and(|end| end <= cap.0)
}

/// Gives `range` of the arena back: joined with the free ranges it touches,
/// and cut off the arena if it ends it
fn release(held: &mut Vec<SpriteInstance>, free: &mut Vec<Range<u32>>, range: Range<u32>) {
  if range.is_empty() {
    return;
  }
  let at = free.partition_point(|r| r.start < range.start);
  free.insert(at, range);
  if at + 1 < free.len() && free[at].end == free[at + 1].start {
    free[at].end = free[at + 1].end;
    free.remove(at + 1);
  }
  if at > 0 && free[at - 1].end == free[at].start {
    free[at - 1].end = free[at].end;
    free.remove(at);
  }
  if let Some(last) = free.last()
    && last.end as usize == held.len()
  {
    held.truncate(last.start as usize);
    free.pop();
  }
}

/// The GPU side of the retained chunks: the arena's buffer and the slot
/// buffer with its bind group, each made when first needed and again when
/// outgrown (the arena's let go once it is empty)
pub(crate) struct ChunkBuffers {
  slot_layout: wgpu::BindGroupLayout,
  instances: Option<(wgpu::Buffer, usize)>,
  slots: Option<(wgpu::Buffer, wgpu::BindGroup, usize)>,
  /// The slots last written
  held_slots: Vec<TileSlot>,
  staging: Vec<SlotBlock>,
}

/// The layout tile slots are bound with (group 2): one slot of the slot
/// buffer, at a dynamic offset, read by the vertex stage
pub(crate) fn slot_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
  device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
    label: Some("Tile Slot Layout"),
    entries: &[wgpu::BindGroupLayoutEntry {
      binding: 0,
      visibility: wgpu::ShaderStages::VERTEX,
      ty: wgpu::BindingType::Buffer {
        ty: wgpu::BufferBindingType::Uniform,
        has_dynamic_offset: true,
        min_binding_size: wgpu::BufferSize::new(std::mem::size_of::<TileSlot>() as u64),
      },
      count: None,
    }],
  })
}

impl ChunkBuffers {
  pub(crate) fn new(device: &wgpu::Device) -> Self {
    Self {
      slot_layout: slot_layout(device),
      instances: None,
      slots: None,
      held_slots: Vec::new(),
      staging: Vec::new(),
    }
  }

  /// The layout tile slots are bound with (group 2)
  pub(crate) fn slot_layout(&self) -> &wgpu::BindGroupLayout {
    &self.slot_layout
  }

  /// Writes what the frame's chunks changed: the slots, their UVs from
  /// where `textures` keeps their images, and the ranges of the arena
  /// built since the last write (all of it into a new buffer once it
  /// outgrew the old one). Call in `prepare`, after the frame's textures
  /// are resident
  pub(crate) fn upload(
    &mut self,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    store: &mut ChunkStore,
    textures: &SpriteGpuRenderer,
  ) {
    // The slots, with their images' places now that they are resident
    for (slot, texture) in store.slots.iter_mut().zip(&store.slot_textures) {
      if let Some((place, _)) = textures.slot(*texture) {
        let [u0, v0, u1, v1] = place.uv;
        slot.uv = [u0, v0, u1 - u0, v1 - v0];
      }
    }
    let wanted = store.slots.len();
    let grown = match &self.slots {
      Some((_, _, capacity)) => wanted > *capacity,
      None => wanted > 0,
    };
    if grown {
      let capacity = wanted.next_power_of_two().max(FIRST_SLOTS);
      let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("Tile Slots"),
        size: capacity as u64 * u64::from(SLOT_STRIDE),
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
      });
      let group = crate::core::graphics::bind_groups::create(
        device,
        &wgpu::BindGroupDescriptor {
          label: Some("Tile Slots"),
          layout: &self.slot_layout,
          entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
              buffer: &buffer,
              offset: 0,
              size: wgpu::BufferSize::new(std::mem::size_of::<TileSlot>() as u64),
            }),
          }],
        },
      );
      self.slots = Some((buffer, group, capacity));
      self.held_slots.clear();
    }
    // A still frame writes no slots
    if let Some((buffer, _, _)) = &self.slots
      && wanted > 0
      && self.held_slots != store.slots
    {
      self.staging.clear();
      self
        .staging
        .extend(store.slots.iter().map(|&slot| SlotBlock {
          slot,
          _pad: [0.0; 52],
        }));
      queue.write_buffer(buffer, 0, bytemuck::cast_slice(&self.staging));
      self.held_slots.clone_from(&store.slots);
    }

    // The arena: nothing built lets its buffer go; a grown one is written
    // whole into a new buffer, else what was built since the last write
    let held = &store.held;
    if held.is_empty() {
      self.instances = None;
      store.written.clear();
      return;
    }
    let size = std::mem::size_of::<SpriteInstance>();
    let fits = matches!(&self.instances, Some((_, capacity)) if held.len() <= *capacity);
    if !fits {
      // Within the GPU's largest buffer: the arena never holds more
      let capacity = held
        .len()
        .next_power_of_two()
        .max(FIRST_INSTANCES)
        .min(store.cap.0)
        .max(held.len());
      let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("Tile Chunks"),
        size: (capacity * size) as u64,
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
      });
      queue.write_buffer(&buffer, 0, bytemuck::cast_slice(held));
      self.instances = Some((buffer, capacity));
      store.written.clear();
      return;
    }
    let Some((buffer, _)) = &self.instances else {
      return;
    };
    // Ranges that touch go in one write (a map's first chunks are built
    // end to end)
    store.written.sort_unstable_by_key(|range| range.start);
    let mut ranges = store.written.drain(..).peekable();
    while let Some(mut range) = ranges.next() {
      while let Some(next) = ranges.next_if(|next| next.start <= range.end) {
        range.end = range.end.max(next.end);
      }
      let end = (range.end as usize).min(held.len());
      let start = range.start as usize;
      if start < end {
        let bytes = bytemuck::cast_slice(&held[start..end]);
        queue.write_buffer(buffer, (start * size) as u64, bytes);
      }
    }
  }

  /// How many instances the arena's buffer holds, 0 with none (tests)
  #[cfg(test)]
  pub(crate) fn capacity(&self) -> usize {
    self.instances.as_ref().map_or(0, |(_, capacity)| *capacity)
  }

  /// The arena's buffer, while any chunk is built
  pub(crate) fn instances(&self) -> Option<&wgpu::Buffer> {
    self.instances.as_ref().map(|(buffer, _)| buffer)
  }

  /// The slots' bind group, once a frame had slots
  pub(crate) fn slot_group(&self) -> Option<&wgpu::BindGroup> {
    self.slots.as_ref().map(|(_, group, _)| group)
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::ecs::World;

  /// A chunk's parts, rows and runs as copied out, and how its tiles
  /// leave their cells' boxes
  struct Copied {
    parts: Vec<ChunkPart>,
    rows: Vec<ChunkRun>,
    runs: Vec<ChunkRun>,
    spill: Spill,
  }

  /// What a build fills a chunk with
  type Scratch = Vec<Planted>;

  impl ChunkStore {
    /// [`chunk`](ChunkStore::chunk), its parts, rows and runs copied out
    fn chunk_copied(
      &mut self,
      layer: Entity,
      chunk: IVec2,
      stamp: ChunkStamp,
      view: u64,
      build: impl FnOnce(&mut Scratch) -> Spill,
    ) -> Option<Copied> {
      let (mut parts, mut rows, mut runs) = (Vec::new(), Vec::new(), Vec::new());
      let out = ChunkCopy {
        parts: &mut parts,
        rows: Some(&mut rows),
        runs: Some(&mut runs),
      };
      let spill = self.chunk((layer, chunk), stamp, view, build, out)?;
      Some(Copied {
        parts,
        rows,
        runs,
        spill,
      })
    }
  }

  fn layers(n: usize) -> Vec<Entity> {
    let mut world = World::new();
    (0..n).map(|_| world.spawn_empty().id()).collect()
  }

  const STAMP: ChunkStamp = ChunkStamp {
    version: 1,
    look: 7,
  };

  /// An instance centred at `(x, y)`, `w` by `h`
  fn tile(x: f32, y: f32, w: f32, h: f32) -> SpriteInstance {
    let mut i = SpriteInstance::new();
    i.transform[0][..2].copy_from_slice(&[w, 0.0]);
    i.transform[1][..2].copy_from_slice(&[0.0, h]);
    i.transform[2][..2].copy_from_slice(&[x, y]);
    i
  }

  /// Builds `tiles` (tileset, centre x) as 16 px tiles on row 0
  fn builder(tiles: &[(u16, f32)]) -> impl FnOnce(&mut Scratch) -> Spill + '_ {
    move |out| {
      out.clear();
      out.extend(tiles.iter().map(|&(set, x)| {
        (
          set,
          (0, 0, (x / 16.0) as i32),
          tile(x, 8.0, 16.0, 16.0),
          None,
        )
      }));
      Spill::None
    }
  }

  fn parts(
    store: &mut ChunkStore,
    layer: Entity,
    stamp: ChunkStamp,
    tiles: &[(u16, f32)],
  ) -> Vec<ChunkPart> {
    let view = store.begin_view();
    store
      .chunk_copied(layer, IVec2::ZERO, stamp, view, builder(tiles))
      .map(|built| built.parts.to_vec())
      .unwrap_or_default()
  }

  #[test]
  fn a_chunk_is_built_once_split_by_tileset_with_its_box() {
    let [layer] = layers(1)[..] else { return };
    let mut store = ChunkStore::default();
    store.begin_frame();
    let built = parts(&mut store, layer, STAMP, &[(0, 8.0), (0, 24.0), (2, 56.0)]);
    assert_eq!(built.len(), 2);
    assert_eq!((built[0].tileset, built[0].count), (0, 2));
    assert_eq!((built[1].tileset, built[1].count), (2, 1));
    assert_eq!(
      (built[0].min, built[0].max),
      (Vec2::ZERO, Vec2::new(32.0, 16.0))
    );
    assert_eq!(
      (built[1].min, built[1].max),
      (Vec2::new(48.0, 0.0), Vec2::new(64.0, 16.0))
    );
    assert_eq!(store.rebuilds(), 1);
    assert_eq!(store.instances_in_use(), 3);
    let key = |tileset| ChunkKey {
      layer,
      chunk: IVec2::ZERO,
      tileset,
    };
    let first = store.range(key(0)).expect("tileset 0");
    assert_eq!(first.len(), 2);
    assert_eq!(store.range(key(2)).map(|r| r.len()), Some(1));
    assert_eq!(store.range(key(1)), None);

    // The same stamp draws what was built
    store.begin_frame();
    let again = parts(&mut store, layer, STAMP, &[]);
    assert_eq!(again, built);
    assert_eq!(store.rebuilds(), 1);
  }

  /// A build of `tiles` (tileset, row, centre x), 16 px tiles in paint
  /// order, saying how they leave their cells' boxes
  fn rows_of(tiles: &[(u16, i32, f32)], spill: Spill) -> impl FnOnce(&mut Scratch) -> Spill + '_ {
    move |out| {
      out.extend(tiles.iter().map(|&(set, row, x)| {
        let y = row as f32 * 16.0 + 8.0;
        (
          set,
          (row, 0, (x / 16.0) as i32),
          tile(x, y, 16.0, 16.0),
          None,
        )
      }));
      spill
    }
  }

  #[test]
  fn an_instance_with_a_sort_point_of_its_own_is_a_row_alone() {
    let [layer] = layers(1)[..] else { return };
    let mut store = ChunkStore::default();
    store.begin_frame();
    let view = store.begin_view();
    // Four tiles of one tileset in row 0, the third sorting at 3.5
    let built = store
      .chunk_copied(layer, IVec2::ZERO, STAMP, view, |out| {
        out.extend((0..4).map(|i| {
          let x = 8.0 + 16.0 * i as f32;
          (
            0,
            (0, 0, i),
            tile(x, 8.0, 16.0, 16.0),
            (i == 2).then_some(3.5),
          )
        }));
        Spill::None
      })
      .expect("built");
    let rows: Vec<(u32, u32, Option<f32>)> = built
      .rows
      .iter()
      .map(|r| (r.first, r.count, r.sort))
      .collect();
    assert_eq!(
      rows,
      [(0, 2, None), (2, 1, Some(3.5)), (3, 1, None)],
      "the tile with a point of its own splits the band"
    );
  }

  #[test]
  fn a_chunk_s_rows_are_runs_of_each_part_band_by_band() {
    let [layer] = layers(1)[..] else { return };
    let mut store = ChunkStore::default();
    store.begin_frame();
    let view = store.begin_view();
    // (tileset, row, centre x): rows 0 and 2 of tileset 0, row 1 of 1
    let tiles = [(0, 0, 8.0), (0, 0, 24.0), (1, 1, 40.0), (0, 2, 8.0)];
    let built = store
      .chunk_copied(
        layer,
        IVec2::ZERO,
        STAMP,
        view,
        rows_of(&tiles, Spill::Vertical),
      )
      .expect("built");
    assert_eq!(built.parts.len(), 2);
    let rows: Vec<(u16, i32, u32, u32)> = built
      .rows
      .iter()
      .map(|r| (r.tileset, r.band.0, r.first, r.count))
      .collect();
    assert_eq!(rows, [(0, 0, 0, 2), (1, 1, 0, 1), (0, 2, 2, 1)]);
    let boxes: Vec<(Vec2, Vec2)> = built.rows.iter().map(|r| (r.min, r.max)).collect();
    assert_eq!(
      boxes,
      [
        (Vec2::ZERO, Vec2::new(32.0, 16.0)),
        (Vec2::new(32.0, 16.0), Vec2::new(48.0, 32.0)),
        (Vec2::new(0.0, 32.0), Vec2::new(16.0, 48.0)),
      ]
    );
    // No tile leaves its cell's sides, so a band's tiles never overlap and
    // its runs are its rows
    assert_eq!(built.spill, Spill::Vertical);
    assert_eq!(built.runs, built.rows);
    // Drawn again unchanged, the rows are as built
    let again = store.begin_view();
    let kept = store
      .chunk_copied(layer, IVec2::ZERO, STAMP, again, |_| Spill::Sideways)
      .expect("kept");
    assert_eq!(kept.rows.len(), 3);
    assert_eq!(kept.spill, Spill::Vertical);
  }

  #[test]
  fn a_spilling_chunk_keeps_its_paint_order_as_runs_of_its_parts() {
    let [layer] = layers(1)[..] else { return };
    let mut store = ChunkStore::default();
    store.begin_frame();
    let view = store.begin_view();
    // Row 0: tilesets 0, 0, 1, 0; row 1: 1, 1, 0 (paint order)
    let tiles = [
      (0, 0, 8.0),
      (0, 0, 24.0),
      (1, 0, 40.0),
      (0, 0, 56.0),
      (1, 1, 8.0),
      (1, 1, 24.0),
      (0, 1, 40.0),
    ];
    let built = store
      .chunk_copied(
        layer,
        IVec2::ZERO,
        STAMP,
        view,
        rows_of(&tiles, Spill::Sideways),
      )
      .expect("built");
    assert_eq!(built.spill, Spill::Sideways);
    let parts: Vec<(u16, u32)> = built.parts.iter().map(|p| (p.tileset, p.count)).collect();
    assert_eq!(parts, [(0, 4), (1, 3)]);
    // Each run: its tileset, band, and place in its part
    let runs: Vec<(u16, i32, u32, u32)> = built
      .runs
      .iter()
      .map(|r| (r.tileset, r.band.0, r.first, r.count))
      .collect();
    assert_eq!(
      runs,
      [
        (0, 0, 0, 2),
        (1, 0, 0, 1),
        (0, 0, 2, 1),
        (1, 1, 1, 2),
        (0, 1, 3, 1)
      ]
    );
    assert_eq!(
      (built.runs[3].min, built.runs[3].max),
      (Vec2::new(0.0, 16.0), Vec2::new(32.0, 32.0))
    );
    // The rows are each tileset's band, band by band
    let rows: Vec<(u16, i32, u32, u32)> = built
      .rows
      .iter()
      .map(|r| (r.tileset, r.band.0, r.first, r.count))
      .collect();
    assert_eq!(
      rows,
      [(0, 0, 0, 3), (1, 0, 0, 1), (0, 1, 3, 1), (1, 1, 1, 2)]
    );
    // Drawn again unchanged, the runs are as built
    let again = store.begin_view();
    let kept = store
      .chunk_copied(layer, IVec2::ZERO, STAMP, again, |_| Spill::None)
      .expect("kept");
    assert_eq!(kept.spill, Spill::Sideways);
    assert_eq!(kept.runs, built.runs);
  }

  #[test]
  fn a_new_version_or_look_rebuilds_the_chunk() {
    let [layer] = layers(1)[..] else { return };
    let mut store = ChunkStore::default();
    store.begin_frame();
    parts(&mut store, layer, STAMP, &[(0, 8.0)]);
    let edited = ChunkStamp {
      version: 2,
      ..STAMP
    };
    let built = parts(&mut store, layer, edited, &[(1, 8.0), (1, 24.0)]);
    assert_eq!(store.rebuilds(), 2);
    assert_eq!(built.len(), 1);
    assert_eq!((built[0].tileset, built[0].count), (1, 2));
    // The old part's room was given back
    assert_eq!(store.instances_in_use(), 2);
    let restyled = ChunkStamp { look: 8, ..edited };
    parts(&mut store, layer, restyled, &[(1, 8.0)]);
    assert_eq!(store.rebuilds(), 3);
    assert_eq!(store.instances_in_use(), 1);
  }

  #[test]
  fn a_view_gets_a_chunk_once() {
    let [layer] = layers(1)[..] else { return };
    let mut store = ChunkStore::default();
    store.begin_frame();
    let view = store.begin_view();
    let get = |store: &mut ChunkStore, view| {
      store
        .chunk_copied(layer, IVec2::ZERO, STAMP, view, builder(&[(0, 8.0)]))
        .is_some()
    };
    assert!(get(&mut store, view));
    assert!(!get(&mut store, view), "twice in one view");
    let other = store.begin_view();
    assert!(
      get(&mut store, other),
      "another view (another camera's parallax)"
    );
    assert_eq!(store.rebuilds(), 1);
  }

  #[test]
  fn freed_room_is_reused_and_the_end_given_back() {
    let [a, b, c] = layers(3)[..] else { return };
    let mut store = ChunkStore::default();
    store.begin_frame();
    parts(&mut store, a, STAMP, &[(0, 8.0), (0, 24.0)]);
    parts(&mut store, b, STAMP, &[(0, 8.0)]);
    parts(&mut store, c, STAMP, &[(0, 8.0), (0, 24.0), (0, 40.0)]);
    assert_eq!(store.instances_in_use(), 6);
    // Layer a goes: its two instances' room is free, below b's
    store.end_frame(|layer| layer != a);
    assert_eq!(store.instances_in_use(), 4);
    assert_eq!(store.chunk_count(), 2);
    store.begin_frame();
    let key = |layer| ChunkKey {
      layer,
      chunk: IVec2::new(1, 0),
      tileset: 0,
    };
    let view = store.begin_view();
    store.chunk_copied(
      b,
      IVec2::new(1, 0),
      STAMP,
      view,
      builder(&[(0, 8.0), (0, 24.0)]),
    );
    assert_eq!(store.range(key(b)), Some(0..2), "a's room, reused");
    // c goes: the arena's end is given back
    store.end_frame(|layer| layer != c);
    assert_eq!(store.instances_in_use(), 3);
    assert_eq!(store.held.len(), 3);
  }

  #[test]
  fn chunks_not_drawn_for_a_while_are_freed() {
    let [layer] = layers(1)[..] else { return };
    let mut store = ChunkStore::default();
    store.begin_frame();
    parts(&mut store, layer, STAMP, &[(0, 8.0)]);
    store.end_frame(|_| true);
    for _ in 0..KEEP_FRAMES {
      store.begin_frame();
      store.end_frame(|_| true);
    }
    assert_eq!(store.chunk_count(), 1, "kept {KEEP_FRAMES} frames undrawn");
    store.begin_frame();
    store.end_frame(|_| true);
    assert_eq!(store.chunk_count(), 0);
    assert_eq!(store.instances_in_use(), 0);
    assert!(store.held.is_empty());
  }

  #[test]
  fn a_full_arena_frees_the_chunks_drawn_least_recently() {
    let [layer] = layers(1)[..] else { return };
    let mut store = ChunkStore::default();
    store.set_cap(6);
    let at = |x| IVec2::new(x, 0);
    let draw = |store: &mut ChunkStore, x: i32, tiles: &[(u16, f32)]| {
      let view = store.begin_view();
      store
        .chunk_copied(layer, at(x), STAMP, view, builder(tiles))
        .is_some()
    };
    // Chunks 0 and 1, then 1 alone: 0 is the least recently drawn
    store.begin_frame();
    assert!(draw(&mut store, 0, &[(0, 8.0), (0, 24.0), (0, 40.0)]));
    store.end_frame(|_| true);
    store.begin_frame();
    assert!(draw(&mut store, 1, &[(0, 8.0), (1, 24.0)]));
    store.end_frame(|_| true);
    // Chunk 2 needs three: 0 is freed for it, not 1 (drawn later)
    store.begin_frame();
    assert!(draw(&mut store, 2, &[(0, 8.0), (0, 24.0), (0, 40.0)]));
    assert_eq!(store.chunk_count(), 2);
    assert_eq!(store.instances_in_use(), 5);
    let key = |x| ChunkKey {
      layer,
      chunk: at(x),
      tileset: 0,
    };
    assert_eq!(store.range(key(0)), None, "freed");
    assert!(store.range(key(1)).is_some());
    assert!(store.held.len() <= 6 && store.held.capacity() <= 6);
    assert_eq!(store.full_warnings(), 0, "room was made");
  }

  #[test]
  fn a_chunk_without_room_is_not_built_until_there_is_and_warns_once() {
    use std::cell::Cell;
    let [layer] = layers(1)[..] else { return };
    let mut store = ChunkStore::default();
    store.set_cap(3);
    let builds = Cell::new(0);
    let draw = |store: &mut ChunkStore, x: i32, count: usize| {
      let view = store.begin_view();
      let build = |out: &mut Scratch| {
        builds.set(builds.get() + 1);
        out.extend((0..count).map(|i| {
          (
            0,
            (0, 0, i as i32),
            tile(8.0 + 16.0 * i as f32, 8.0, 16.0, 16.0),
            None,
          )
        }));
        Spill::None
      };
      store
        .chunk_copied(layer, IVec2::new(x, 0), STAMP, view, build)
        .is_some()
    };
    // Both drawn in one frame: the second has no room, and none can be made
    store.begin_frame();
    assert!(draw(&mut store, 0, 2));
    assert!(!draw(&mut store, 1, 2), "no room");
    assert_eq!((store.full_warnings(), builds.get()), (1, 2));
    store.end_frame(|_| true);
    // Again: it is not built again, as there is still no room
    store.begin_frame();
    assert!(draw(&mut store, 0, 2));
    assert!(!draw(&mut store, 1, 2));
    assert_eq!(builds.get(), 2, "not built again without room");
    store.end_frame(|_| true);
    // Drawn alone, it takes the other's room
    store.begin_frame();
    assert!(draw(&mut store, 1, 2));
    assert_eq!(builds.get(), 3);
    assert_eq!(store.chunk_count(), 1);
    assert_eq!(store.full_warnings(), 1, "warned once");
  }

  #[test]
  fn written_ranges_name_what_was_built() {
    let [layer] = layers(1)[..] else { return };
    let mut store = ChunkStore::default();
    store.begin_frame();
    parts(&mut store, layer, STAMP, &[(0, 8.0), (3, 24.0)]);
    assert_eq!(store.written, [0..1, 1..2]);
    let held: &[u8] = bytemuck::cast_slice(&store.held);
    let second = tile(24.0, 8.0, 16.0, 16.0);
    assert_eq!(&held[128..256], bytemuck::bytes_of(&second));
  }

  #[test]
  fn slots_last_a_frame() {
    let mut store = ChunkStore::default();
    store.begin_frame();
    let texture = TextureId::for_test(3);
    assert_eq!(store.push_slot(TileSlot::default(), texture), 0);
    assert_eq!(store.push_slot(TileSlot::default(), texture), 1);
    store.begin_frame();
    assert_eq!(store.push_slot(TileSlot::default(), texture), 0);
  }
}
