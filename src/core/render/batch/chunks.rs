//! Retained tile chunks. Each chunk of a tile layer keeps its instances, a
//! range per tileset in row order (so each row of cells is a run of it, for
//! y-sorted layers), in one arena the GPU holds a copy of: built once from
//! its tiles, written to the GPU once, and built and written again only when
//! what it was built from changes. A draw of a chunk binds a slot (group 2,
//! at a dynamic offset) that places it: the layer's world transform, the
//! camera's parallax offset and where the tileset's padded image sits on its
//! page, so moving the map, moving the camera or the atlas moving the image
//! never rebuilds a chunk.
//!
//! [`ChunkStore`] is the CPU side, filled while the frame is gathered;
//! [`ChunkBuffers`] the GPU side, written in `Batch2D::prepare`.

use std::ops::Range;

use glam::{UVec2, Vec2};

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
/// outgrows it
const FIRST_INSTANCES: usize = 1024;

/// One tileset's tiles of one chunk of a tile layer: a range of the arena
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct ChunkKey {
  pub layer: Entity,
  pub chunk: UVec2,
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

/// One row of cells of one tileset's part of a chunk: a run of the part's
/// instances (they are in row order), and the box they cover in the
/// layer's pixels. A y-sorted layer draws each row on its own
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ChunkRow {
  pub tileset: u16,
  /// The cells' row in the layer
  pub row: u32,
  /// Where the run starts in its part, and how many instances it has
  pub first: u32,
  pub count: u32,
  pub min: Vec2,
  pub max: Vec2,
}

/// A chunk as a view draws it: each tileset's part, and the rows of each
pub(crate) struct BuiltChunk<'a> {
  pub parts: &'a [ChunkPart],
  /// Part by part, rows in order
  pub rows: &'a [ChunkRow],
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
  parts: Vec<ChunkPart>,
  rows: Vec<ChunkRow>,
}

/// The CPU side of the retained chunks: each layer's built chunks, the
/// arena they are in (and what of it changed since the GPU was written),
/// and the frame's slots
#[derive(Default)]
pub(crate) struct ChunkStore {
  layers: FastMap<Entity, FastMap<UVec2, Built>>,
  /// The arena: every chunk's instances, as the GPU is to hold them
  held: Vec<SpriteInstance>,
  /// Free ranges of the arena below its end, by start, none touching
  free: Vec<Range<u32>>,
  /// Ranges written since the GPU last was
  written: Vec<Range<u32>>,
  /// A chunk's instances as built (tileset, row, instance), before they
  /// are split by tileset
  scratch: Vec<(u16, u32, SpriteInstance)>,
  /// The frame's slots, and the texture each one's UVs come from
  slots: Vec<TileSlot>,
  slot_textures: Vec<TextureId>,
  frame: u64,
  views: u64,
  rebuilds: u64,
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

  /// The parts and rows of `layer`'s chunk `chunk` for `view`, built by
  /// `build` first (it fills the instances, as `(tileset, row, instance)`
  /// grouped by tileset, in row order within one) when `stamp` is not what
  /// it was built from. `None` when the view had it already. The chunk
  /// counts as drawn this frame
  pub(crate) fn chunk(
    &mut self,
    layer: Entity,
    chunk: UVec2,
    stamp: ChunkStamp,
    view: u64,
    build: impl FnOnce(&mut Vec<(u16, u32, SpriteInstance)>),
  ) -> Option<BuiltChunk<'_>> {
    let Self {
      layers,
      held,
      free,
      written,
      scratch,
      frame,
      rebuilds,
      ..
    } = self;
    let built = layers.entry(layer).or_default().entry(chunk).or_default();
    if built.view == view {
      return None;
    }
    built.view = view;
    built.drawn = *frame;
    if built.stamp != Some(stamp) {
      built.stamp = Some(stamp);
      *rebuilds += 1;
      for part in built.parts.drain(..) {
        release(held, free, part.start..part.start + part.count);
      }
      built.rows.clear();
      scratch.clear();
      build(scratch);
      for run in scratch.chunk_by(|a, b| a.0 == b.0) {
        let tileset = run[0].0;
        let count = run.len() as u32;
        let start = allocate(held, free, count);
        for (slot, (_, _, instance)) in held[start as usize..].iter_mut().zip(run) {
          *slot = *instance;
        }
        // Each row's run of the part, and the box of each
        let (mut min, mut max) = (Vec2::splat(f32::MAX), Vec2::splat(f32::MIN));
        let mut first = 0;
        for cells in run.chunk_by(|a, b| a.1 == b.1) {
          let (mut low, mut high) = (Vec2::splat(f32::MAX), Vec2::splat(f32::MIN));
          for (_, _, instance) in cells {
            let (a, b) = reach(instance);
            low = low.min(a);
            high = high.max(b);
          }
          built.rows.push(ChunkRow {
            tileset,
            row: cells[0].1,
            first,
            count: cells.len() as u32,
            min: low,
            max: high,
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
    }
    Some(BuiltChunk {
      parts: &built.parts,
      rows: &built.rows,
    })
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

  /// How many times a chunk has been built (again)
  #[cfg(test)]
  pub(crate) fn rebuilds(&self) -> u64 {
    self.rebuilds
  }

  /// How many chunks are built
  #[cfg(test)]
  pub(crate) fn chunk_count(&self) -> usize {
    self.layers.values().map(FastMap::len).sum()
  }

  /// How many instances the built chunks hold
  #[cfg(test)]
  pub(crate) fn instances_in_use(&self) -> usize {
    let free: usize = self.free.iter().map(|r| r.len()).sum();
    self.held.len() - free
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
/// them, else past its end
fn allocate(held: &mut Vec<SpriteInstance>, free: &mut Vec<Range<u32>>, count: u32) -> u32 {
  if let Some(i) = free.iter().position(|r| r.len() as u32 >= count) {
    let start = free[i].start;
    free[i].start += count;
    if free[i].is_empty() {
      free.remove(i);
    }
    return start;
  }
  let start = held.len() as u32;
  held.resize(held.len() + count as usize, bytemuck::Zeroable::zeroed());
  start
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
      let capacity = held.len().next_power_of_two().max(FIRST_INSTANCES);
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
  fn builder(tiles: &[(u16, f32)]) -> impl FnOnce(&mut Vec<(u16, u32, SpriteInstance)>) + '_ {
    move |out| {
      out.clear();
      out.extend(
        tiles
          .iter()
          .map(|&(set, x)| (set, 0, tile(x, 8.0, 16.0, 16.0))),
      );
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
      .chunk(layer, UVec2::ZERO, stamp, view, builder(tiles))
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
      chunk: UVec2::ZERO,
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

  #[test]
  fn a_chunk_s_rows_are_runs_of_each_part() {
    let [layer] = layers(1)[..] else { return };
    let mut store = ChunkStore::default();
    store.begin_frame();
    let view = store.begin_view();
    // (tileset, row, centre x): rows 0 and 2 of tileset 0, row 1 of 1
    let tiles = [(0, 0, 8.0), (0, 0, 24.0), (0, 2, 8.0), (1, 1, 40.0)];
    let build = |out: &mut Vec<(u16, u32, SpriteInstance)>| {
      out.extend(tiles.iter().map(|&(set, row, x)| {
        let y = row as f32 * 16.0 + 8.0;
        (set, row, tile(x, y, 16.0, 16.0))
      }));
    };
    let built = store
      .chunk(layer, UVec2::ZERO, STAMP, view, build)
      .expect("built");
    assert_eq!(built.parts.len(), 2);
    let rows: Vec<(u16, u32, u32, u32)> = built
      .rows
      .iter()
      .map(|r| (r.tileset, r.row, r.first, r.count))
      .collect();
    assert_eq!(rows, [(0, 0, 0, 2), (0, 2, 2, 1), (1, 1, 0, 1)]);
    let boxes: Vec<(Vec2, Vec2)> = built.rows.iter().map(|r| (r.min, r.max)).collect();
    assert_eq!(
      boxes,
      [
        (Vec2::ZERO, Vec2::new(32.0, 16.0)),
        (Vec2::new(0.0, 32.0), Vec2::new(16.0, 48.0)),
        (Vec2::new(32.0, 16.0), Vec2::new(48.0, 32.0)),
      ]
    );
    // Drawn again unchanged, the rows are as built
    let again = store.begin_view();
    let kept = store
      .chunk(layer, UVec2::ZERO, STAMP, again, |_| {})
      .expect("kept");
    assert_eq!(kept.rows.len(), 3);
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
        .chunk(layer, UVec2::ZERO, STAMP, view, builder(&[(0, 8.0)]))
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
      chunk: UVec2::new(1, 0),
      tileset: 0,
    };
    let view = store.begin_view();
    store.chunk(
      b,
      UVec2::new(1, 0),
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
