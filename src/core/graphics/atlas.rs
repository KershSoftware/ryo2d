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

use etagere::{AllocId, BucketedAtlasAllocator, size2};

use super::RenderTarget;
use super::texture_registry::TextureId;
use crate::core::render::batch::Sampling;

/// Edge length of an atlas page in pixels.
const PAGE_SIZE: u32 = 2048;
/// Maximum number of atlas pages before entries are evicted.
const MAX_PAGES: usize = 8;
/// Images larger than this on either side are standalone.
const MAX_ATLAS_IMAGE: u32 = 256;
/// Entries unused for this many frames may be freed when a page is full, and
/// are freed every [`RELEASE_EVERY`] frames.
const STALE_FRAMES: u64 = 300;
/// Most entries evicted for a single placement before giving up (the image
/// then falls back to a standalone texture rather than flushing the atlas).
const MAX_EVICTIONS: usize = 64;
/// Border added around every image, in pixels.
const BORDER: u32 = 1;
/// How often stale atlas entries are freed and empty pages released
const RELEASE_EVERY: u64 = 60;
const PAGE_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

/// Which texture holds an image.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum SlotPage {
  /// Index of a shared atlas page
  Atlas(usize),
  /// Id of a standalone texture (images too large for the atlas)
  Standalone(usize),
  /// A render target's texture
  Target(RenderTarget),
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
  entries: HashMap<TextureId, Entry>,
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
    key: TextureId,
    width: u32,
    height: u32,
    frame: u64,
  ) -> Option<Placement> {
    if width == 0 || height == 0 || width > MAX_ATLAS_IMAGE || height > MAX_ATLAS_IMAGE {
      return None;
    }
    if self
      .entries
      .get(&key)
      .is_some_and(|e| (e.w, e.h) != (width, height))
    {
      self.remove(key);
    }
    if let Some(entry) = self.entries.get_mut(&key) {
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
      key,
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
  pub(crate) fn holds(&self, key: TextureId, width: u32, height: u32) -> bool {
    self
      .entries
      .get(&key)
      .is_some_and(|e| (e.w, e.h) == (width, height))
  }

  /// The slot of `key`, without marking it used.
  pub(crate) fn get(&self, key: TextureId) -> Option<AtlasSlot> {
    self.entries.get(&key).map(|e| e.slot)
  }

  /// Free every entry unused for [`STALE_FRAMES`] or more frames.
  pub(crate) fn free_stale(&mut self, frame: u64) {
    let stale: Vec<TextureId> = self
      .entries
      .iter()
      .filter(|(_, e)| frame.saturating_sub(e.last_used) >= STALE_FRAMES)
      .map(|(k, _)| *k)
      .collect();
    for key in stale {
      self.remove(key);
    }
  }

  /// Free entries unused for [`STALE_FRAMES`] frames, then release empty
  /// pages at the end (earlier pages keep their index)
  pub(crate) fn release_stale(&mut self, frame: u64) {
    self.free_stale(frame);
    let used = self.entries.values().map(|e| e.page + 1).max().unwrap_or(0);
    self.pages.truncate(used);
  }

  /// Free `key`'s slot, if it has one.
  pub(crate) fn remove(&mut self, key: TextureId) {
    if let Some(entry) = self.entries.remove(&key)
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
    // Clear the least recently used page that nothing drew from this frame:
    // one eviction sure to make room, where scattered entries rarely free a
    // large enough rect (the allocator reclaims space a bucket at a time)
    if let Some(page) = self.least_recent_page(frame) {
      self.clear_page(page);
      if let Some(found) = self.try_allocate(bw, bh) {
        return Some(found);
      }
    }
    // Every page has something drawn this frame: evict single entries, least
    // recently used first, never ones used this frame, and never more than
    // MAX_EVICTIONS for one placement.
    let mut candidates: Vec<(u64, TextureId)> = self
      .entries
      .iter()
      .filter(|(_, e)| e.last_used < frame)
      .map(|(k, e)| (e.last_used, *k))
      .collect();
    candidates.sort_by_key(|(last_used, _)| *last_used);
    for (_, key) in candidates.into_iter().take(MAX_EVICTIONS) {
      self.remove(key);
      if let Some(found) = self.try_allocate(bw, bh) {
        return Some(found);
      }
    }
    None
  }

  /// The page whose most recently used entry is the oldest, among pages with
  /// entries and none used this frame
  fn least_recent_page(&self, frame: u64) -> Option<usize> {
    let mut newest: Vec<Option<u64>> = vec![None; self.pages.len()];
    for entry in self.entries.values() {
      let slot = &mut newest[entry.page];
      *slot = Some(slot.map_or(entry.last_used, |n| n.max(entry.last_used)));
    }
    newest
      .iter()
      .enumerate()
      .filter_map(|(page, newest)| newest.map(|n| (page, n)))
      .filter(|(_, newest)| *newest < frame)
      .min_by_key(|(_, newest)| *newest)
      .map(|(page, _)| page)
  }

  /// Free every entry on `page`
  fn clear_page(&mut self, page: usize) {
    self.entries.retain(|_, e| e.page != page);
    if let Some(allocator) = self.pages.get_mut(page) {
      allocator.clear();
    }
  }
}

struct Page {
  texture: wgpu::Texture,
  bind_groups: PageBindGroups,
}

/// A texture's bind groups, one per sampling.
struct PageBindGroups {
  nearest: wgpu::BindGroup,
  linear: wgpu::BindGroup,
}

impl PageBindGroups {
  fn get(&self, sampling: Sampling) -> &wgpu::BindGroup {
    match sampling {
      Sampling::Nearest => &self.nearest,
      Sampling::Linear => &self.linear,
    }
  }
}

/// A render target's texture: cameras draw into `view` (sRGB, blending in
/// linear); sprites sample it through `bind_groups` as raw sRGB bytes, like
/// the pages.
struct TargetPage {
  key: TextureId,
  texture: wgpu::Texture,
  view: wgpu::TextureView,
  bind_groups: PageBindGroups,
}

struct Standalone {
  key: TextureId,
  texture: wgpu::Texture,
  bind_groups: PageBindGroups,
  slot: AtlasSlot,
  size: (u32, u32),
  last_used: u64,
}

/// GPU pages + layout + standalone textures.
///
/// Every page, standalone texture and render target gets a bind group per
/// sampling when it is created.
pub(crate) struct TextureAtlas {
  layout: AtlasLayout,
  pages: Vec<Page>,
  standalone: HashMap<usize, Standalone>,
  standalone_ids: HashMap<TextureId, usize>,
  next_standalone: usize,
  targets: HashMap<RenderTarget, TargetPage>,
  target_ids: HashMap<TextureId, RenderTarget>,
  bind_layout: wgpu::BindGroupLayout,
  nearest: wgpu::Sampler,
  linear: wgpu::Sampler,
}

impl TextureAtlas {
  /// An empty atlas whose textures bind with `layout` (texture, then
  /// sampler).
  pub(crate) fn new(device: &wgpu::Device, layout: wgpu::BindGroupLayout) -> Self {
    // The atlas border covers any edge overshoot of either filter
    let sampler = |label, filter| {
      device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some(label),
        address_mode_u: wgpu::AddressMode::ClampToEdge,
        address_mode_v: wgpu::AddressMode::ClampToEdge,
        address_mode_w: wgpu::AddressMode::ClampToEdge,
        mag_filter: filter,
        min_filter: filter,
        mipmap_filter: wgpu::MipmapFilterMode::Nearest,
        ..Default::default()
      })
    };
    Self {
      layout: AtlasLayout::new(PAGE_SIZE, MAX_PAGES),
      pages: Vec::new(),
      standalone: HashMap::new(),
      standalone_ids: HashMap::new(),
      next_standalone: 0,
      targets: HashMap::new(),
      target_ids: HashMap::new(),
      bind_layout: layout,
      nearest: sampler("Sprite Sampler (nearest)", wgpu::FilterMode::Nearest),
      linear: sampler("Sprite Sampler (linear)", wgpu::FilterMode::Linear),
    }
  }

  /// The bind groups sampling `view` with each sampler.
  fn bind(&self, device: &wgpu::Device, view: &wgpu::TextureView, label: &str) -> PageBindGroups {
    let group = |sampler: &wgpu::Sampler| {
      crate::core::graphics::bind_groups::create(
        device,
        &wgpu::BindGroupDescriptor {
          label: Some(label),
          layout: &self.bind_layout,
          entries: &[
            wgpu::BindGroupEntry {
              binding: 0,
              resource: wgpu::BindingResource::TextureView(view),
            },
            wgpu::BindGroupEntry {
              binding: 1,
              resource: wgpu::BindingResource::Sampler(sampler),
            },
          ],
        },
      )
    };
    PageBindGroups {
      nearest: group(&self.nearest),
      linear: group(&self.linear),
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
    key: TextureId,
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

    if let Some(id) = self.standalone_ids.get(&key).copied() {
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
    Some(self.create_standalone(device, queue, key, width, height, &bordered, frame))
  }

  /// Where `key` lives now, without uploading it or marking it used.
  pub(crate) fn lookup(&self, key: TextureId) -> Option<AtlasSlot> {
    if let Some(&target) = self.target_ids.get(&key) {
      return Some(AtlasSlot {
        page: SlotPage::Target(target),
        uv: [0.0, 0.0, 1.0, 1.0],
      });
    }
    match self.standalone_ids.get(&key) {
      Some(id) => self.standalone.get(id).map(|s| s.slot),
      None => self.layout.get(key),
    }
  }

  /// Forget `key`, freeing its atlas rect or standalone texture.
  pub(crate) fn remove(&mut self, key: TextureId) {
    if let Some(target) = self.target_ids.remove(&key) {
      self.targets.remove(&target);
      return;
    }
    if let Some(id) = self.standalone_ids.get(&key).copied() {
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
    self.targets.clear();
    self.target_ids.clear();
  }

  /// Give `target` (registered as `key`) a `width`x`height` texture,
  /// recreated when its size changed.
  pub(crate) fn sync_target(
    &mut self,
    device: &wgpu::Device,
    key: TextureId,
    target: RenderTarget,
    (width, height): (u32, u32),
  ) {
    if let Some(page) = self.targets.get(&target)
      && page.key == key
      && (page.texture.width(), page.texture.height()) == (width, height)
    {
      return;
    }
    let texture = device.create_texture(&wgpu::TextureDescriptor {
      label: Some("Render Target"),
      size: wgpu::Extent3d {
        width,
        height,
        depth_or_array_layers: 1,
      },
      mip_level_count: 1,
      sample_count: 1,
      dimension: wgpu::TextureDimension::D2,
      format: wgpu::TextureFormat::Rgba8UnormSrgb,
      // Lighting copies the camera's image before lighting it in place
      usage: wgpu::TextureUsages::RENDER_ATTACHMENT
        | wgpu::TextureUsages::TEXTURE_BINDING
        | wgpu::TextureUsages::COPY_SRC,
      view_formats: &[PAGE_FORMAT],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    let sampled = texture.create_view(&wgpu::TextureViewDescriptor {
      format: Some(PAGE_FORMAT),
      ..Default::default()
    });
    let bind_groups = self.bind(device, &sampled, "Render Target");
    self.target_ids.insert(key, target);
    self.targets.insert(
      target,
      TargetPage {
        key,
        texture,
        view,
        bind_groups,
      },
    );
  }

  /// `target`'s texture and the view cameras draw into.
  pub(crate) fn target(
    &self,
    target: RenderTarget,
  ) -> Option<(&wgpu::Texture, &wgpu::TextureView)> {
    self
      .targets
      .get(&target)
      .map(|page| (&page.texture, &page.view))
  }

  /// The bind group sampling `page` with `sampling`.
  pub(crate) fn bind_group(&self, page: SlotPage, sampling: Sampling) -> Option<&wgpu::BindGroup> {
    let groups = match page {
      SlotPage::Atlas(i) => &self.pages.get(i)?.bind_groups,
      SlotPage::Standalone(i) => &self.standalone.get(&i)?.bind_groups,
      SlotPage::Target(target) => &self.targets.get(&target)?.bind_groups,
    };
    Some(groups.get(sampling))
  }

  /// The GPU texture behind `page` (for readback).
  pub(crate) fn texture(&self, page: SlotPage) -> Option<&wgpu::Texture> {
    match page {
      SlotPage::Atlas(i) => self.pages.get(i).map(|p| &p.texture),
      SlotPage::Standalone(i) => self.standalone.get(&i).map(|s| &s.texture),
      SlotPage::Target(target) => self.targets.get(&target).map(|t| &t.texture),
    }
  }

  /// Housekeeping: drop standalone textures unused for 300 or more frames,
  /// and every [`RELEASE_EVERY`] frames free stale atlas entries and the
  /// pages left empty at the end, so the GPU memory comes back.
  pub(crate) fn end_frame(&mut self, frame: u64) {
    if frame.is_multiple_of(RELEASE_EVERY) {
      self.layout.release_stale(frame);
      self.pages.truncate(self.layout.page_count());
    }
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
    let (texture, bind_groups) = self.create_texture(device, PAGE_SIZE, PAGE_SIZE, "Atlas Page");
    Page {
      texture,
      bind_groups,
    }
  }

  fn create_texture(
    &self,
    device: &wgpu::Device,
    width: u32,
    height: u32,
    label: &str,
  ) -> (wgpu::Texture, PageBindGroups) {
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
    let bind_groups = self.bind(device, &view, label);
    (texture, bind_groups)
  }

  #[allow(clippy::too_many_arguments)]
  fn create_standalone(
    &mut self,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    key: TextureId,
    width: u32,
    height: u32,
    bordered: &[u8],
    frame: u64,
  ) -> AtlasSlot {
    let (bw, bh) = (width + 2 * BORDER, height + 2 * BORDER);
    let (texture, bind_groups) = self.create_texture(device, bw, bh, "Atlas Standalone");
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
    self.standalone_ids.insert(key, id);
    self.standalone.insert(
      id,
      Standalone {
        key,
        texture,
        bind_groups,
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
    wgpu::TexelCopyTextureInfo {
      texture,
      mip_level: 0,
      origin: wgpu::Origin3d { x, y, z: 0 },
      aspect: wgpu::TextureAspect::All,
    },
    bordered,
    wgpu::TexelCopyBufferLayout {
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

  fn id(n: usize) -> TextureId {
    TextureId::for_test(n as u32)
  }

  #[test]
  fn same_key_gets_the_same_slot() {
    let mut layout = AtlasLayout::new(2048, 2);
    let a = layout.place(id(0), 32, 16, 0).unwrap();
    let b = layout.place(id(0), 32, 16, 1).unwrap();
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
      let p = layout.place(id(n), w, h, 0).unwrap();
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
    assert!(layout.place(id(0), 257, 10, 0).is_none());
    assert!(layout.place(id(1), 10, 257, 0).is_none());
    assert!(layout.place(id(2), 256, 256, 0).is_some());
  }

  #[test]
  fn new_pages_before_eviction_then_lru() {
    let mut layout = AtlasLayout::new(128, 2);
    let placed: Vec<_> = (0..8)
      .map(|n| layout.place(id(n), 62, 62, n as u64 + 1).unwrap())
      .collect();
    assert_eq!(placed[0].slot.page, SlotPage::Atlas(0));
    assert_eq!(placed[4].slot.page, SlotPage::Atlas(1));
    assert!(placed.iter().all(|p| p.newly_allocated));
    let ninth = layout.place(id(8), 62, 62, 20).unwrap();
    assert_eq!(
      (ninth.slot, ninth.x, ninth.y),
      (placed[0].slot, placed[0].x, placed[0].y)
    );
    assert!(layout.place(id(0), 62, 62, 21).unwrap().newly_allocated);
  }

  #[test]
  fn a_full_atlas_frees_a_whole_page_instead_of_scattered_entries() {
    let mut layout = AtlasLayout::new(512, 1);
    let mut n = 0;
    while n < 2000 && layout.place(id(n), 14, 14, 1).is_some() {
      n += 1;
    }
    assert!(n > 500, "only {n} small images fit");
    // Scattered small evictions would rarely free a 66 px square in one go
    let big = layout.place(id(9999), 64, 64, 2);
    assert!(big.is_some(), "a 64 px image fell back to standalone");
  }

  #[test]
  fn freeing_a_page_spares_pages_used_this_frame() {
    let mut layout = AtlasLayout::new(256, 2);
    for n in 0..16 {
      assert_eq!(
        layout.place(id(n), 62, 62, 1).map(|p| p.slot.page),
        Some(SlotPage::Atlas(0))
      );
    }
    for n in 16..32 {
      assert_eq!(
        layout.place(id(n), 62, 62, 2).map(|p| p.slot.page),
        Some(SlotPage::Atlas(1))
      );
    }
    // Page 0 is older, but one of its images is drawn this frame
    layout.place(id(0), 62, 62, 3);
    let big = layout.place(id(100), 126, 126, 3).expect("room is made");
    assert_eq!(big.slot.page, SlotPage::Atlas(1));
    assert!((0..16).all(|n| layout.holds(id(n), 62, 62)), "page 0 kept");
  }

  #[test]
  fn stale_entries_are_freed_and_empty_trailing_pages_released() {
    let mut layout = AtlasLayout::new(128, 3);
    for n in 0..8 {
      layout.place(id(n), 62, 62, 1);
    }
    assert_eq!(layout.page_count(), 2);
    // Only one image on page 0 is still drawn
    layout.place(id(0), 62, 62, 200);
    layout.release_stale(301);
    assert!(layout.holds(id(0), 62, 62));
    assert!(!(1..8).any(|n| layout.holds(id(n), 62, 62)));
    assert_eq!(layout.page_count(), 1, "the emptied last page is released");
  }

  #[test]
  fn a_removed_id_is_placed_afresh() {
    // A changed sprite is removed and placed again, so its pixels are rewritten
    let mut layout = AtlasLayout::new(2048, 2);
    assert!(layout.place(id(5), 16, 16, 0).unwrap().newly_allocated);
    layout.remove(id(5));
    assert!(layout.place(id(5), 16, 16, 1).unwrap().newly_allocated);
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
      .map(|n| layout.place(id(n), 62, 62, 0).unwrap())
      .collect();
    let p = layout.place(id(9), 62, 62, 300).unwrap();
    assert_eq!(layout.page_count(), 1);
    assert_eq!(p.slot.page, SlotPage::Atlas(0));
    assert!(old.iter().any(|o| (o.x, o.y) == (p.x, p.y)));
    assert_live_disjoint(&layout);
  }

  #[test]
  fn fresh_entries_are_kept_and_a_page_is_opened() {
    let mut layout = AtlasLayout::new(128, 2);
    let old: Vec<_> = (0..4)
      .map(|n| layout.place(id(n), 62, 62, 0).unwrap())
      .collect();
    let p = layout.place(id(9), 62, 62, 299).unwrap();
    assert_eq!(layout.page_count(), 2);
    assert_eq!(p.slot.page, SlotPage::Atlas(1));
    for (n, o) in old.iter().enumerate() {
      let again = layout.place(id(n), 62, 62, 299).unwrap();
      assert!(!again.newly_allocated);
      assert_eq!(again.slot, o.slot);
    }
  }

  #[test]
  fn nothing_used_this_frame_is_evicted() {
    let mut layout = AtlasLayout::new(128, 1);
    let old: Vec<_> = (0..4)
      .map(|n| layout.place(id(n), 62, 62, 5).unwrap())
      .collect();
    assert!(layout.place(id(9), 62, 62, 5).is_none());
    for (n, o) in old.iter().enumerate() {
      let again = layout.place(id(n), 62, 62, 5).unwrap();
      assert!(!again.newly_allocated);
      assert_eq!(again.slot, o.slot);
    }
  }

  #[test]
  fn lru_eviction_keeps_live_rects_disjoint() {
    let mut layout = AtlasLayout::new(128, 2);
    for n in 0..8 {
      layout.place(id(n), 62, 62, n as u64 + 1).unwrap();
    }
    for n in 8..12 {
      let p = layout.place(id(n), 62, 62, 20 + n as u64).unwrap();
      assert!(p.newly_allocated);
      assert_live_disjoint(&layout);
    }
    assert_eq!(layout.entries.len(), 8);
  }

  fn gpu_atlas(device: &wgpu::Device) -> TextureAtlas {
    TextureAtlas::new(device, crate::core::render::batch::texture_layout(device))
  }

  #[test]
  fn failed_upload_does_not_register_the_key() {
    let Ok((device, queue)) = crate::core::testing::shared_gpu() else {
      return;
    };
    let mut atlas = gpu_atlas(&device);
    assert!(
      atlas
        .slot(&device, &queue, id(0), 2, 2, || vec![0; 7], 0)
        .is_none()
    );
    let px: Vec<u8> = (0..4u8).flat_map(|i| [10 * (i + 1), 0, 0, 255]).collect();
    let slot = atlas
      .slot(&device, &queue, id(0), 2, 2, || px.clone(), 1)
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
      .slot(&device, &queue, id(0), 300, 4, || vec![9; 300 * 4 * 4], 0)
      .unwrap();
    let b = atlas
      .slot(&device, &queue, id(0), 301, 4, || vec![9; 301 * 4 * 4], 1)
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
      .slot(&device, &queue, id(0), 2, 2, || vec![10; 2 * 2 * 4], 0)
      .unwrap();
    // Too big for the atlas, so standalone
    let big = atlas
      .slot(&device, &queue, id(0), 300, 4, || vec![20; 300 * 4 * 4], 1)
      .unwrap();
    assert!(matches!(big.page, SlotPage::Standalone(_)));
    // Back at the original size, the new pixels must be written
    let small = atlas
      .slot(&device, &queue, id(0), 2, 2, || vec![77; 2 * 2 * 4], 2)
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
      .slot(&device, &queue, id(0), 2, 2, || vec![10; 2 * 2 * 4], 0)
      .unwrap();
    let resized = atlas
      .slot(&device, &queue, id(0), 3, 2, || vec![50; 3 * 2 * 4], 1)
      .unwrap();
    assert_ne!(small.uv, resized.uv);
    assert_eq!(atlas.lookup(id(0)), Some(resized));
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
        .slot(&device, &queue, id(0), too_big, 1, || unreachable!(), 0)
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
      .slot(&device, &queue, id(0), 2, 2, || px.clone(), 0)
      .unwrap();
    assert!(atlas.bind_group(slot.page, Sampling::Nearest).is_some());
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
