//! Texture atlas: a CPU allocation layout plus GPU pages.
//!
//! Small images (at most [`MAX_ATLAS_IMAGE`] px per side) are packed into
//! shared 2048x2048 pages so many sprites can share one bind group. Every
//! placed image carries a 1-px border repeating its edge pixels, so bilinear
//! sampling and scaling never bleed in a neighbour. Larger images get their
//! own standalone texture with the same border and slot semantics.
//!
//! An [`AtlasSlot`] names its texture with a [`SlotPage`]: either a shared atlas
//! page or a standalone texture.

use std::collections::HashMap;
use std::sync::Arc;

use etagere::{AllocId, BucketedAtlasAllocator, size2};

/// Edge length of an atlas page in pixels.
const PAGE_SIZE: u32 = 2048;
/// Maximum number of atlas pages before entries are evicted.
const MAX_PAGES: usize = 8;
/// Images larger than this on either side are standalone.
const MAX_ATLAS_IMAGE: u32 = 256;
/// Entries unused for this many frames may be freed when a page is full.
const STALE_FRAMES: u64 = 300;
/// Most entries evicted for a single placement before giving up (the image
/// then falls back to a standalone texture rather than flushing the atlas).
const MAX_EVICTIONS: usize = 64;
/// Border added around every image, in pixels.
const BORDER: u32 = 1;
const PAGE_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

/// What an atlas entry is keyed by.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) enum AtlasKey {
  /// An entity sprite: the handle's identity plus its content generation
  Sprite { handle: usize, generation: u64 },
  /// A texture uploaded by id (render_gpu, particles, parallax)
  Id(String),
}

/// Which texture holds an image.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum SlotPage {
  /// Index of a shared atlas page
  Atlas(usize),
  /// Id of a standalone texture (images too large for the atlas)
  Standalone(usize),
}

/// Where an image lives: a page index and its rect in normalized page UVs.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct AtlasSlot {
  pub(crate) page: SlotPage,
  /// u_min, v_min, u_max, v_max of the image (inside the extruded border)
  pub(crate) uv: [f32; 4],
}

/// Result of [`AtlasLayout::place`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Placement {
  pub(crate) slot: AtlasSlot,
  /// Pixel origin of the bordered rect
  pub(crate) x: u32,
  pub(crate) y: u32,
  pub(crate) newly_allocated: bool,
}

struct Entry {
  w: u32,
  h: u32,
  page: usize,
  alloc: AllocId,
  slot: AtlasSlot,
  x: u32,
  y: u32,
  last_used: u64,
}

/// CPU-side allocation bookkeeping (testable without a GPU).
pub(crate) struct AtlasLayout {
  page_size: u32,
  max_pages: usize,
  pages: Vec<BucketedAtlasAllocator>,
  entries: HashMap<AtlasKey, Entry>,
}

impl AtlasLayout {
  pub(crate) fn new(page_size: u32, max_pages: usize) -> Self {
    Self {
      page_size,
      max_pages,
      pages: Vec::new(),
      entries: HashMap::new(),
    }
  }

  /// Number of pages allocated so far.
  pub(crate) fn page_count(&self) -> usize {
    self.pages.len()
  }

  /// Find a slot for `key`, allocating `width`x`height` (+2 px border) if new.
  /// `None` when the image must be standalone (larger than 256 px) or nothing can be freed.
  pub(crate) fn place(
    &mut self,
    key: &AtlasKey,
    width: u32,
    height: u32,
    frame: u64,
  ) -> Option<Placement> {
    if width == 0 || height == 0 || width > MAX_ATLAS_IMAGE || height > MAX_ATLAS_IMAGE {
      return None;
    }
    if self
      .entries
      .get(key)
      .is_some_and(|e| (e.w, e.h) != (width, height))
    {
      self.remove(key);
    }
    if let Some(entry) = self.entries.get_mut(key) {
      entry.last_used = frame;
      return Some(Placement {
        slot: entry.slot,
        x: entry.x,
        y: entry.y,
        newly_allocated: false,
      });
    }

    let (bw, bh) = (width + 2 * BORDER, height + 2 * BORDER);
    let (page, alloc, x, y) = self.allocate(bw, bh, frame)?;
    let ps = self.page_size as f32;
    let slot = AtlasSlot {
      page: SlotPage::Atlas(page),
      uv: [
        (x + BORDER) as f32 / ps,
        (y + BORDER) as f32 / ps,
        (x + BORDER + width) as f32 / ps,
        (y + BORDER + height) as f32 / ps,
      ],
    };
    self.entries.insert(
      key.clone(),
      Entry {
        w: width,
        h: height,
        page,
        alloc,
        slot,
        x,
        y,
        last_used: frame,
      },
    );
    Some(Placement {
      slot,
      x,
      y,
      newly_allocated: true,
    })
  }

  /// Whether `key` currently has a slot of exactly `width`x`height`.
  pub(crate) fn holds(&self, key: &AtlasKey, width: u32, height: u32) -> bool {
    self
      .entries
      .get(key)
      .is_some_and(|e| (e.w, e.h) == (width, height))
  }

  /// The slot of `key`, without marking it used.
  pub(crate) fn get(&self, key: &AtlasKey) -> Option<AtlasSlot> {
    self.entries.get(key).map(|e| e.slot)
  }

  /// Free every entry unused for [`STALE_FRAMES`] or more frames.
  pub(crate) fn free_stale(&mut self, frame: u64) {
    let stale: Vec<AtlasKey> = self
      .entries
      .iter()
      .filter(|(_, e)| frame.saturating_sub(e.last_used) >= STALE_FRAMES)
      .map(|(k, _)| k.clone())
      .collect();
    for key in stale {
      self.remove(&key);
    }
  }

  /// Free `key`'s slot, if it has one.
  pub(crate) fn remove(&mut self, key: &AtlasKey) {
    if let Some(entry) = self.entries.remove(key)
      && let Some(page) = self.pages.get_mut(entry.page)
    {
      page.deallocate(entry.alloc);
    }
  }

  fn try_allocate(&mut self, bw: u32, bh: u32) -> Option<(usize, AllocId, u32, u32)> {
    let size = size2(bw as i32, bh as i32);
    for (index, page) in self.pages.iter_mut().enumerate() {
      if let Some(a) = page.allocate(size) {
        return Some((
          index,
          a.id,
          a.rectangle.min.x as u32,
          a.rectangle.min.y as u32,
        ));
      }
    }
    None
  }

  /// Allocation order: existing pages, stale entries, new pages, then LRU eviction.
  fn allocate(&mut self, bw: u32, bh: u32, frame: u64) -> Option<(usize, AllocId, u32, u32)> {
    if bw > self.page_size || bh > self.page_size {
      return None;
    }
    if let Some(found) = self.try_allocate(bw, bh) {
      return Some(found);
    }
    self.free_stale(frame);
    if let Some(found) = self.try_allocate(bw, bh) {
      return Some(found);
    }
    while self.pages.len() < self.max_pages {
      self.pages.push(BucketedAtlasAllocator::new(size2(
        self.page_size as i32,
        self.page_size as i32,
      )));
      if let Some(found) = self.try_allocate(bw, bh) {
        return Some(found);
      }
    }
    // Evict least recently used entries, never ones used this frame, and
    // never more than MAX_EVICTIONS for one placement.
    for _ in 0..MAX_EVICTIONS {
      let lru = self
        .entries
        .iter()
        .min_by_key(|(_, e)| e.last_used)
        .filter(|(_, e)| e.last_used < frame)
        .map(|(k, _)| k.clone())?;
      self.remove(&lru);
      if let Some(found) = self.try_allocate(bw, bh) {
        return Some(found);
      }
    }
    None
  }
}

struct Page {
  texture: wgpu::Texture,
  bind_group: wgpu::BindGroup,
}

struct Standalone {
  key: AtlasKey,
  texture: wgpu::Texture,
  bind_group: wgpu::BindGroup,
  slot: AtlasSlot,
  size: (u32, u32),
  last_used: u64,
}

/// GPU pages + layout + standalone textures.
///
/// The layout and sampler are shared (`Arc`) because wgpu 22 handles are not
/// `Clone` and bind groups are created lazily as pages and textures appear.
pub(crate) struct TextureAtlas {
  layout: AtlasLayout,
  pages: Vec<Page>,
  standalone: HashMap<usize, Standalone>,
  standalone_ids: HashMap<AtlasKey, usize>,
  next_standalone: usize,
  bind_layout: Arc<wgpu::BindGroupLayout>,
  sampler: Arc<wgpu::Sampler>,
}

impl TextureAtlas {
  pub(crate) fn new(
    _device: &wgpu::Device,
    layout: Arc<wgpu::BindGroupLayout>,
    sampler: Arc<wgpu::Sampler>,
  ) -> Self {
    Self {
      layout: AtlasLayout::new(PAGE_SIZE, MAX_PAGES),
      pages: Vec::new(),
      standalone: HashMap::new(),
      standalone_ids: HashMap::new(),
      next_standalone: 0,
      bind_layout: layout,
      sampler,
    }
  }

  /// Upload (if needed) and return where `rgba` (sRGB bytes) lives.
  ///
  /// `rgba` is only called when the image has to be written. Images larger
  /// than 256 px, or that cannot be placed in the atlas without evicting more
  /// than a bounded number of entries, get a standalone texture. `None` when
  /// the pixel data does not match the size (nothing is registered then) or
  /// the image exceeds the device's texture size limit.
  #[allow(clippy::too_many_arguments)] // signature fixed by the atlas API
  pub(crate) fn slot(
    &mut self,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    key: &AtlasKey,
    width: u32,
    height: u32,
    rgba: impl FnOnce() -> Vec<u8>,
    frame: u64,
  ) -> Option<AtlasSlot> {
    if width == 0 || height == 0 {
      return None;
    }
    let max_dim = device.limits().max_texture_dimension_2d;
    if width + 2 * BORDER > max_dim || height + 2 * BORDER > max_dim {
      return None;
    }

    if let Some(id) = self.standalone_ids.get(key).copied() {
      if let Some(s) = self.standalone.get_mut(&id)
        && s.size == (width, height)
      {
        s.last_used = frame;
        return Some(s.slot);
      }
      self.remove_standalone(id);
    }

    let small = width <= MAX_ATLAS_IMAGE && height <= MAX_ATLAS_IMAGE;
    // A resized key falls through, so its new pixels are written
    if small && self.layout.holds(key, width, height) {
      return self.layout.place(key, width, height, frame).map(|p| p.slot);
    }

    // Validate before registering anything, so a bad upload leaves no entry.
    let pixels = rgba();
    let bordered = extrude_border(&pixels, width, height)?;
    if small && let Some(p) = self.layout.place(key, width, height, frame) {
      while self.pages.len() < self.layout.page_count() {
        self.pages.push(self.create_page(device));
      }
      let SlotPage::Atlas(index) = p.slot.page else {
        return None;
      };
      let page = self.pages.get(index)?;
      write_bordered(queue, &page.texture, p.x, p.y, width, height, &bordered);
      return Some(p.slot);
    }
    // Standalone now: a leftover atlas slot would be returned, unwritten, if
    // the key later shrinks back to its old size
    self.layout.remove(key);
    Some(self.create_standalone(device, queue, key.clone(), width, height, &bordered, frame))
  }

  /// Where `key` lives now, without uploading it or marking it used.
  pub(crate) fn lookup(&self, key: &AtlasKey) -> Option<AtlasSlot> {
    match self.standalone_ids.get(key) {
      Some(id) => self.standalone.get(id).map(|s| s.slot),
      None => self.layout.get(key),
    }
  }

  /// Forget `key`, freeing its atlas rect or standalone texture.
  pub(crate) fn remove(&mut self, key: &AtlasKey) {
    if let Some(id) = self.standalone_ids.get(key).copied() {
      self.remove_standalone(id);
    }
    self.layout.remove(key);
  }

  /// Drop every entry, page and standalone texture.
  pub(crate) fn clear(&mut self) {
    self.layout = AtlasLayout::new(PAGE_SIZE, MAX_PAGES);
    self.pages.clear();
    self.standalone.clear();
    self.standalone_ids.clear();
  }

  pub(crate) fn bind_group(&self, page: SlotPage) -> Option<&wgpu::BindGroup> {
    match page {
      SlotPage::Atlas(i) => self.pages.get(i).map(|p| &p.bind_group),
      SlotPage::Standalone(i) => self.standalone.get(&i).map(|s| &s.bind_group),
    }
  }

  /// The GPU texture behind `page` (for readback).
  pub(crate) fn texture(&self, page: SlotPage) -> Option<&wgpu::Texture> {
    match page {
      SlotPage::Atlas(i) => self.pages.get(i).map(|p| &p.texture),
      SlotPage::Standalone(i) => self.standalone.get(&i).map(|s| &s.texture),
    }
  }

  /// Housekeeping: drop standalone textures unused for 300 or more frames.
  pub(crate) fn end_frame(&mut self, frame: u64) {
    let stale: Vec<usize> = self
      .standalone
      .iter()
      .filter(|(_, s)| frame.saturating_sub(s.last_used) >= STALE_FRAMES)
      .map(|(id, _)| *id)
      .collect();
    for id in stale {
      self.remove_standalone(id);
    }
  }

  fn remove_standalone(&mut self, id: usize) {
    if let Some(s) = self.standalone.remove(&id) {
      self.standalone_ids.remove(&s.key);
    }
  }

  fn create_page(&self, device: &wgpu::Device) -> Page {
    let (texture, bind_group) = self.create_texture(device, PAGE_SIZE, PAGE_SIZE, "Atlas Page");
    Page {
      texture,
      bind_group,
    }
  }

  fn create_texture(
    &self,
    device: &wgpu::Device,
    width: u32,
    height: u32,
    label: &str,
  ) -> (wgpu::Texture, wgpu::BindGroup) {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
      label: Some(label),
      size: wgpu::Extent3d {
        width,
        height,
        depth_or_array_layers: 1,
      },
      mip_level_count: 1,
      sample_count: 1,
      dimension: wgpu::TextureDimension::D2,
      format: PAGE_FORMAT,
      usage: wgpu::TextureUsages::TEXTURE_BINDING
        | wgpu::TextureUsages::COPY_DST
        | wgpu::TextureUsages::COPY_SRC,
      view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
      label: Some(label),
      layout: &self.bind_layout,
      entries: &[
        wgpu::BindGroupEntry {
          binding: 0,
          resource: wgpu::BindingResource::TextureView(&view),
        },
        wgpu::BindGroupEntry {
          binding: 1,
          resource: wgpu::BindingResource::Sampler(&self.sampler),
        },
      ],
    });
    (texture, bind_group)
  }

  #[allow(clippy::too_many_arguments)]
  fn create_standalone(
    &mut self,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    key: AtlasKey,
    width: u32,
    height: u32,
    bordered: &[u8],
    frame: u64,
  ) -> AtlasSlot {
    let (bw, bh) = (width + 2 * BORDER, height + 2 * BORDER);
    let (texture, bind_group) = self.create_texture(device, bw, bh, "Atlas Standalone");
    write_bordered(queue, &texture, 0, 0, width, height, bordered);
    let id = self.next_standalone;
    self.next_standalone += 1;
    let slot = AtlasSlot {
      page: SlotPage::Standalone(id),
      uv: [
        BORDER as f32 / bw as f32,
        BORDER as f32 / bh as f32,
        (BORDER + width) as f32 / bw as f32,
        (BORDER + height) as f32 / bh as f32,
      ],
    };
    self.standalone_ids.insert(key.clone(), id);
    self.standalone.insert(
      id,
      Standalone {
        key,
        texture,
        bind_group,
        slot,
        size: (width, height),
        last_used: frame,
      },
    );
    slot
  }
}

/// Copy `rgba` into a `(w+2)x(h+2)` buffer whose border repeats the edge pixels.
/// `None` when `rgba` is not `width * height * 4` bytes.
fn extrude_border(rgba: &[u8], width: u32, height: u32) -> Option<Vec<u8>> {
  let (w, h) = (width as usize, height as usize);
  if rgba.len() != w * h * 4 {
    return None;
  }
  let bw = w + 2;
  let mut out = vec![0u8; bw * (h + 2) * 4];
  for by in 0..h + 2 {
    let sy = by.saturating_sub(1).min(h - 1);
    for bx in 0..bw {
      let sx = bx.saturating_sub(1).min(w - 1);
      let src = (sy * w + sx) * 4;
      let dst = (by * bw + bx) * 4;
      out[dst..dst + 4].copy_from_slice(&rgba[src..src + 4]);
    }
  }
  Some(out)
}

fn write_bordered(
  queue: &wgpu::Queue,
  texture: &wgpu::Texture,
  x: u32,
  y: u32,
  width: u32,
  height: u32,
  bordered: &[u8],
) {
  let (bw, bh) = (width + 2 * BORDER, height + 2 * BORDER);
  queue.write_texture(
    wgpu::ImageCopyTexture {
      texture,
      mip_level: 0,
      origin: wgpu::Origin3d { x, y, z: 0 },
      aspect: wgpu::TextureAspect::All,
    },
    bordered,
    wgpu::ImageDataLayout {
      offset: 0,
      bytes_per_row: Some(bw * 4),
      rows_per_image: Some(bh),
    },
    wgpu::Extent3d {
      width: bw,
      height: bh,
      depth_or_array_layers: 1,
    },
  );
}

#[cfg(test)]
mod tests {
  use super::*;

  struct Lcg(u64);
  impl Lcg {
    fn next(&mut self, max: u32) -> u32 {
      self.0 = self
        .0
        .wrapping_mul(6364136223846793005)
        .wrapping_add(1442695040888963407);
      ((self.0 >> 33) as u32) % max
    }
  }

  fn id(n: usize) -> AtlasKey {
    AtlasKey::Id(format!("k{n}"))
  }

  #[test]
  fn same_key_gets_the_same_slot() {
    let mut layout = AtlasLayout::new(2048, 2);
    let a = layout.place(&id(0), 32, 16, 0).unwrap();
    let b = layout.place(&id(0), 32, 16, 1).unwrap();
    assert!(a.newly_allocated);
    assert!(!b.newly_allocated);
    assert_eq!(a.slot, b.slot);
    assert_eq!((a.x, a.y), (b.x, b.y));
  }

  #[test]
  fn different_keys_get_disjoint_rects() {
    let mut layout = AtlasLayout::new(2048, 4);
    let mut rng = Lcg(7);
    let mut rects = Vec::new();
    for n in 0..50 {
      let (w, h) = (1 + rng.next(120), 1 + rng.next(120));
      let p = layout.place(&id(n), w, h, 0).unwrap();
      rects.push((p.slot.page, p.x, p.y, w + 2, h + 2));
    }
    for (i, a) in rects.iter().enumerate() {
      for b in &rects[i + 1..] {
        if a.0 != b.0 {
          continue;
        }
        let apart = a.1 + a.3 <= b.1 || b.1 + b.3 <= a.1 || a.2 + a.4 <= b.2 || b.2 + b.4 <= a.2;
        assert!(apart, "{a:?} overlaps {b:?}");
      }
    }
  }

  #[test]
  fn large_images_are_standalone() {
    let mut layout = AtlasLayout::new(2048, 2);
    assert!(layout.place(&id(0), 257, 10, 0).is_none());
    assert!(layout.place(&id(1), 10, 257, 0).is_none());
    assert!(layout.place(&id(2), 256, 256, 0).is_some());
  }

  #[test]
  fn new_pages_before_eviction_then_lru() {
    let mut layout = AtlasLayout::new(128, 2);
    let placed: Vec<_> = (0..8)
      .map(|n| layout.place(&id(n), 62, 62, n as u64 + 1).unwrap())
      .collect();
    assert_eq!(placed[0].slot.page, SlotPage::Atlas(0));
    assert_eq!(placed[4].slot.page, SlotPage::Atlas(1));
    assert!(placed.iter().all(|p| p.newly_allocated));
    let ninth = layout.place(&id(8), 62, 62, 20).unwrap();
    assert_eq!(
      (ninth.slot, ninth.x, ninth.y),
      (placed[0].slot, placed[0].x, placed[0].y)
    );
    assert!(layout.place(&id(0), 62, 62, 21).unwrap().newly_allocated);
  }

  #[test]
  fn changed_sprite_reuploads() {
    let mut layout = AtlasLayout::new(2048, 2);
    let k1 = AtlasKey::Sprite {
      handle: 5,
      generation: 1,
    };
    let k2 = AtlasKey::Sprite {
      handle: 5,
      generation: 2,
    };
    let a = layout.place(&k1, 16, 16, 0).unwrap();
    let b = layout.place(&k2, 16, 16, 1).unwrap();
    assert!(a.newly_allocated && b.newly_allocated);
    assert_ne!((a.x, a.y), (b.x, b.y));
  }

  fn assert_live_disjoint(layout: &AtlasLayout) {
    let rects: Vec<_> = layout
      .entries
      .values()
      .map(|e| (e.slot.page, e.x, e.y, e.w + 2, e.h + 2))
      .collect();
    for (i, a) in rects.iter().enumerate() {
      for b in &rects[i + 1..] {
        if a.0 != b.0 {
          continue;
        }
        let apart = a.1 + a.3 <= b.1 || b.1 + b.3 <= a.1 || a.2 + a.4 <= b.2 || b.2 + b.4 <= a.2;
        assert!(apart, "{a:?} overlaps {b:?}");
      }
    }
  }

  #[test]
  fn full_page_frees_entries_unused_for_300_frames() {
    let mut layout = AtlasLayout::new(128, 2);
    let old: Vec<_> = (0..4)
      .map(|n| layout.place(&id(n), 62, 62, 0).unwrap())
      .collect();
    let p = layout.place(&id(9), 62, 62, 300).unwrap();
    assert_eq!(layout.page_count(), 1);
    assert_eq!(p.slot.page, SlotPage::Atlas(0));
    assert!(old.iter().any(|o| (o.x, o.y) == (p.x, p.y)));
    assert_live_disjoint(&layout);
  }

  #[test]
  fn fresh_entries_are_kept_and_a_page_is_opened() {
    let mut layout = AtlasLayout::new(128, 2);
    let old: Vec<_> = (0..4)
      .map(|n| layout.place(&id(n), 62, 62, 0).unwrap())
      .collect();
    let p = layout.place(&id(9), 62, 62, 299).unwrap();
    assert_eq!(layout.page_count(), 2);
    assert_eq!(p.slot.page, SlotPage::Atlas(1));
    for (n, o) in old.iter().enumerate() {
      let again = layout.place(&id(n), 62, 62, 299).unwrap();
      assert!(!again.newly_allocated);
      assert_eq!(again.slot, o.slot);
    }
  }

  #[test]
  fn nothing_used_this_frame_is_evicted() {
    let mut layout = AtlasLayout::new(128, 1);
    let old: Vec<_> = (0..4)
      .map(|n| layout.place(&id(n), 62, 62, 5).unwrap())
      .collect();
    assert!(layout.place(&id(9), 62, 62, 5).is_none());
    for (n, o) in old.iter().enumerate() {
      let again = layout.place(&id(n), 62, 62, 5).unwrap();
      assert!(!again.newly_allocated);
      assert_eq!(again.slot, o.slot);
    }
  }

  #[test]
  fn lru_eviction_keeps_live_rects_disjoint() {
    let mut layout = AtlasLayout::new(128, 2);
    for n in 0..8 {
      layout.place(&id(n), 62, 62, n as u64 + 1).unwrap();
    }
    for n in 8..12 {
      let p = layout.place(&id(n), 62, 62, 20 + n as u64).unwrap();
      assert!(p.newly_allocated);
      assert_live_disjoint(&layout);
    }
    assert_eq!(layout.entries.len(), 8);
  }

  fn gpu_atlas(device: &wgpu::Device) -> TextureAtlas {
    let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
      label: None,
      entries: &[
        wgpu::BindGroupLayoutEntry {
          binding: 0,
          visibility: wgpu::ShaderStages::FRAGMENT,
          ty: wgpu::BindingType::Texture {
            multisampled: false,
            view_dimension: wgpu::TextureViewDimension::D2,
            sample_type: wgpu::TextureSampleType::Float { filterable: true },
          },
          count: None,
        },
        wgpu::BindGroupLayoutEntry {
          binding: 1,
          visibility: wgpu::ShaderStages::FRAGMENT,
          ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
          count: None,
        },
      ],
    });
    let sampler = device.create_sampler(&wgpu::SamplerDescriptor::default());
    TextureAtlas::new(device, Arc::new(bgl), Arc::new(sampler))
  }

  #[test]
  fn failed_upload_does_not_register_the_key() {
    let Ok((device, queue)) = crate::core::testing::shared_gpu() else {
      return;
    };
    let mut atlas = gpu_atlas(&device);
    assert!(
      atlas
        .slot(&device, &queue, &id(0), 2, 2, || vec![0; 7], 0)
        .is_none()
    );
    let px: Vec<u8> = (0..4u8).flat_map(|i| [10 * (i + 1), 0, 0, 255]).collect();
    let slot = atlas
      .slot(&device, &queue, &id(0), 2, 2, || px.clone(), 1)
      .unwrap();
    let page = atlas.texture(slot.page).unwrap();
    let img = crate::core::capture::read_texture_rgba(&device, &queue, page).unwrap();
    let x = (slot.uv[0] * 2048.0).round() as u32;
    let y = (slot.uv[1] * 2048.0).round() as u32;
    assert_eq!(img.get_pixel(x, y).0[0], 10);
  }

  #[test]
  fn standalone_reupload_with_new_size_recreates() {
    let Ok((device, queue)) = crate::core::testing::shared_gpu() else {
      return;
    };
    let mut atlas = gpu_atlas(&device);
    let a = atlas
      .slot(&device, &queue, &id(0), 300, 4, || vec![9; 300 * 4 * 4], 0)
      .unwrap();
    let b = atlas
      .slot(&device, &queue, &id(0), 301, 4, || vec![9; 301 * 4 * 4], 1)
      .unwrap();
    assert_ne!(a.uv, b.uv);
    let tex = atlas.texture(b.page).unwrap();
    assert_eq!(tex.width(), 303);
  }

  #[test]
  fn key_growing_standalone_then_shrinking_rewrites_its_pixels() {
    let Ok((device, queue)) = crate::core::testing::shared_gpu() else {
      return;
    };
    let mut atlas = gpu_atlas(&device);
    atlas
      .slot(&device, &queue, &id(0), 2, 2, || vec![10; 2 * 2 * 4], 0)
      .unwrap();
    // Too big for the atlas, so standalone
    let big = atlas
      .slot(&device, &queue, &id(0), 300, 4, || vec![20; 300 * 4 * 4], 1)
      .unwrap();
    assert!(matches!(big.page, SlotPage::Standalone(_)));
    // Back at the original size, the new pixels must be written
    let small = atlas
      .slot(&device, &queue, &id(0), 2, 2, || vec![77; 2 * 2 * 4], 2)
      .unwrap();
    assert!(matches!(small.page, SlotPage::Atlas(_)));
    let page = atlas.texture(small.page).unwrap();
    let img = crate::core::capture::read_texture_rgba(&device, &queue, page).unwrap();
    let x = (small.uv[0] * 2048.0).round() as u32;
    let y = (small.uv[1] * 2048.0).round() as u32;
    assert_eq!(img.get_pixel(x, y).0[0], 77);
  }

  #[test]
  fn resized_key_rewrites_pixels() {
    let Ok((device, queue)) = crate::core::testing::shared_gpu() else {
      return;
    };
    let mut atlas = gpu_atlas(&device);
    let small = atlas
      .slot(&device, &queue, &id(0), 2, 2, || vec![10; 2 * 2 * 4], 0)
      .unwrap();
    let resized = atlas
      .slot(&device, &queue, &id(0), 3, 2, || vec![50; 3 * 2 * 4], 1)
      .unwrap();
    assert_ne!(small.uv, resized.uv);
    assert_eq!(atlas.lookup(&id(0)), Some(resized));
    let page = atlas.texture(resized.page).unwrap();
    let img = crate::core::capture::read_texture_rgba(&device, &queue, page).unwrap();
    let x = (resized.uv[0] * 2048.0).round() as u32;
    let y = (resized.uv[1] * 2048.0).round() as u32;
    assert_eq!(img.get_pixel(x, y).0[0], 50);
  }

  #[test]
  fn images_beyond_the_device_limit_are_rejected() {
    let Ok((device, queue)) = crate::core::testing::shared_gpu() else {
      return;
    };
    let mut atlas = gpu_atlas(&device);
    let too_big = device.limits().max_texture_dimension_2d;
    assert!(
      atlas
        .slot(&device, &queue, &id(0), too_big, 1, || unreachable!(), 0)
        .is_none()
    );
  }

  #[test]
  fn border_repeats_edge_pixels() {
    let Ok((device, queue)) = crate::core::testing::shared_gpu() else {
      return;
    };
    let mut atlas = gpu_atlas(&device);
    let px: Vec<u8> = (0..4u8).flat_map(|i| [10 * (i + 1), 0, 0, 255]).collect();
    let slot = atlas
      .slot(&device, &queue, &id(0), 2, 2, || px.clone(), 0)
      .unwrap();
    assert!(atlas.bind_group(slot.page).is_some());
    let page = atlas.texture(slot.page).unwrap();
    let img = crate::core::capture::read_texture_rgba(&device, &queue, page).unwrap();
    let ps = 2048.0;
    let x0 = (slot.uv[0] * ps).round() as u32 - 1;
    let y0 = (slot.uv[1] * ps).round() as u32 - 1;
    // Expected red per bordered cell, rows top to bottom.
    let r = [
      [10, 10, 20, 20],
      [10, 10, 20, 20],
      [30, 30, 40, 40],
      [30, 30, 40, 40],
    ];
    for (dy, row) in r.iter().enumerate() {
      for (dx, want) in row.iter().enumerate() {
        let p = img.get_pixel(x0 + dx as u32, y0 + dy as u32);
        assert_eq!(p.0[0], *want, "pixel ({dx},{dy})");
      }
    }
  }
}
