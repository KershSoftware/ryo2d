//! Texture atlas: a CPU allocation layout plus GPU pages.
//!
//! Small images (at most [`MAX_ATLAS_IMAGE`] px per side) are packed into
//! shared 2048x2048 pages so many sprites can share one bind group. Every
//! placed image carries an 8-px border repeating its edge pixels and starts
//! on an 8-px grid, so bilinear sampling never bleeds in a neighbour at any
//! of the pages' four mip levels. Larger images get their own standalone
//! texture with the same border and slot semantics and a full mip chain.
//!
//! An [`AtlasSlot`] names its texture with a [`SlotPage`]: either a shared atlas
//! page or a standalone texture.
//!
//! A new version of an image that has a texture, its upload made ahead
//! ([`PreparedUpload`]) and larger than a frame's budget, is written into a
//! new texture a band of rows a frame while the old one draws; once whole,
//! the new one takes the old one's place before a frame draws anything.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use etagere::{AllocId, BucketedAtlasAllocator, size2};

use super::glyph_atlas::{GlyphAtlas, GlyphPage, GlyphSlot};
use super::texture_registry::TextureId;
use super::{Image, RenderTarget};
use crate::core::asset::AssetId;
use crate::core::render::batch::Sampling;
use crate::core::text::{GlyphImage, GlyphKey};

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
/// Border added around every image, in pixels: one texel at the smallest
/// mip level.
pub(crate) const BORDER: u32 = 8;
/// Atlas rects start on and span multiples of this many pixels, so they
/// stay whole texels down to the smallest mip level.
const CELL: u32 = 8;
/// Mip levels of an atlas page (2048 down to 256 px).
pub(crate) const MIP_LEVELS: u32 = 4;
/// How often stale atlas entries are freed and empty pages released
const RELEASE_EVERY: u64 = 60;
/// The most bytes of image levels a frame writes into the textures of
/// images being replaced, counting the frame's other image writes first.
/// Measured on an Apple M5 Pro, a write costs the main thread about 50 µs
/// and 85 to 110 µs a MB more, so 2 MiB is about 0.2 ms of the 0.5 ms a
/// reload may add to a frame; a 2048 px image's 22.7 MB take 11 frames
pub(crate) const UPLOAD_BUDGET: u64 = 2 * 1024 * 1024;
/// The first image being replaced writes at least this share of the
/// budget (a quarter) a frame, whatever else the frame wrote
const MIN_SPREAD_SHARE: u64 = 4;
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
  /// A glyph page
  Glyph(GlyphPage),
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

  /// Number of keys with a slot.
  #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
  pub(crate) fn len(&self) -> usize {
    self.entries.len()
  }

  /// Find a slot for `key`, allocating `width`x`height` plus its border (in
  /// whole cells) if new.
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

  /// The size of the image `key` holds a slot for
  fn size(&self, key: TextureId) -> Option<(u32, u32)> {
    self.entries.get(&key).map(|e| (e.w, e.h))
  }

  /// The slot of `key`, marked used in `frame`
  fn touch(&mut self, key: TextureId, frame: u64) -> Option<AtlasSlot> {
    let entry = self.entries.get_mut(&key)?;
    entry.last_used = frame;
    Some(entry.slot)
  }

  /// The last frame `key`'s slot was used in
  fn last_used(&self, key: TextureId) -> Option<u64> {
    self.entries.get(&key).map(|e| e.last_used)
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

  /// Allocate `bw`x`bh` pixels, rounded up to whole cells; the rect's pixel
  /// origin.
  fn try_allocate(&mut self, bw: u32, bh: u32) -> Option<(usize, AllocId, u32, u32)> {
    let size = size2(bw.div_ceil(CELL) as i32, bh.div_ceil(CELL) as i32);
    for (index, page) in self.pages.iter_mut().enumerate() {
      if let Some(a) = page.allocate(size) {
        return Some((
          index,
          a.id,
          a.rectangle.min.x as u32 * CELL,
          a.rectangle.min.y as u32 * CELL,
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
      let cells = (self.page_size / CELL) as i32;
      self
        .pages
        .push(BucketedAtlasAllocator::new(size2(cells, cells)));
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
pub(crate) struct PageBindGroups {
  nearest: wgpu::BindGroup,
  linear: wgpu::BindGroup,
}

impl PageBindGroups {
  pub(crate) fn get(&self, sampling: Sampling) -> &wgpu::BindGroup {
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

/// What an image being written into a new texture draws until the new one
/// is whole: its old texture, showing an older version at its size then
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Showing {
  /// The image's texture
  pub(crate) key: TextureId,
  pub(crate) image: AssetId<Image>,
  /// The version the old texture shows
  pub(crate) version: u64,
  /// The size the old texture shows it at
  pub(crate) size: (u32, u32),
}

/// An image's new version, written into a texture of its own a band of
/// rows at a time, every level in turn. The texture is made with its first
/// band, so images reloaded together do not all take their GPU memory in
/// one frame
struct Spread {
  texture: Option<(wgpu::Texture, PageBindGroups)>,
  upload: PreparedUpload,
  /// The texture's levels
  levels: u32,
  /// The level the next band is in, and its first row
  level: u32,
  row: u32,
}

/// What a texture is made with: the device, and the bind group layout and
/// samplers its bind groups take
struct Maker<'a> {
  device: &'a wgpu::Device,
  layout: &'a wgpu::BindGroupLayout,
  nearest: &'a wgpu::Sampler,
  linear: &'a wgpu::Sampler,
}

impl Spread {
  fn new(upload: PreparedUpload) -> Spread {
    let (width, height) = upload.size();
    Spread {
      texture: None,
      upload,
      levels: standalone_levels(width + 2 * BORDER, height + 2 * BORDER),
      level: 0,
      row: 0,
    }
  }

  /// Whether every level is written
  fn done(&self) -> bool {
    self.texture.is_some() && self.level >= self.levels
  }

  /// Write the next bands, up to `room` bytes of levels (and at least a
  /// row when `floor`), as `write_mips` would write them whole; the bytes
  /// written. None when the upload lacks a level the texture has, so the
  /// texture can never be whole. The texture is made only when a row goes
  /// in
  fn write(
    &mut self,
    make: &Maker<'_>,
    queue: &wgpu::Queue,
    (room, floor): (u64, bool),
  ) -> Option<u64> {
    let (width, height) = self.upload.size();
    let first_row = u64::from(width + 2 * BORDER) * 4;
    if self.texture.is_none() && !floor && room < first_row {
      return Some(0);
    }
    if self.texture.is_none() {
      crate::profile_scope!("atlas:spread texture");
      let (bw, bh) = (width + 2 * BORDER, height + 2 * BORDER);
      self.texture = Some(new_texture(make, (bw, bh), self.levels, "Atlas Standalone"));
    }
    let Spread {
      texture: Some((texture, _)),
      upload,
      levels,
      level,
      row,
    } = self
    else {
      return None;
    };
    let mut chain = upload.levels().skip(*level as usize);
    let mut written = 0u64;
    while *level < *levels {
      let (width, height, pixels) = chain.next()?;
      // The GPU rounds odd level sizes down: the last column or row (the
      // border's) is left out, as `write_mips` leaves it
      let size = texture
        .size()
        .mip_level_size(*level, wgpu::TextureDimension::D2);
      let (columns, rows) = (width.min(size.width), height.min(size.height));
      let row_bytes = u64::from(width) * 4;
      while *row < rows {
        let fit = room.saturating_sub(written) / row_bytes;
        // The first spread always moves on, at least a row a call
        let fit = if floor && written == 0 {
          fit.max(1)
        } else {
          fit
        };
        let band = fit.min(u64::from(rows - *row));
        if band == 0 {
          return Some(written);
        }
        let start = u64::from(*row) * row_bytes;
        let bytes = band * row_bytes;
        let data = usize::try_from(start)
          .ok()
          .zip(usize::try_from(start + bytes).ok())
          .and_then(|(start, end)| pixels.get(start..end))?;
        let band = band as u32;
        let first = *level == 0 && *row == 0;
        let write = || {
          queue.write_texture(
            wgpu::TexelCopyTextureInfo {
              texture,
              mip_level: *level,
              origin: wgpu::Origin3d {
                x: 0,
                y: *row,
                z: 0,
              },
              aspect: wgpu::TextureAspect::All,
            },
            data,
            wgpu::TexelCopyBufferLayout {
              offset: 0,
              bytes_per_row: Some(width * 4),
              rows_per_image: Some(band),
            },
            wgpu::Extent3d {
              width: columns,
              height: band,
              depth_or_array_layers: 1,
            },
          );
        };
        if first {
          // wgpu clears the level before its first write
          crate::profile_scope!("atlas:spread first band");
          write();
        } else {
          write();
        }
        written += bytes;
        *row += band;
      }
      *level += 1;
      *row = 0;
    }
    Some(written)
  }
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
  glyphs: GlyphAtlas,
  /// Images written so far, to a page or a standalone texture
  writes: u64,
  /// New versions of images with a texture, being written into new
  /// textures over frames, by key (in key order, so the writes are in one
  /// order every run)
  spreads: BTreeMap<TextureId, Spread>,
  /// What the keys of `spreads` draw meanwhile
  showing: Vec<Showing>,
  /// Keys whose new texture took the old one's place, until taken
  swapped: Vec<TextureId>,
  /// Keys whose new texture was dropped unfinished, until taken
  dropped: Vec<TextureId>,
  /// Bytes of image levels written this frame
  spent: u64,
  /// The most a frame writes into `spreads` ([`UPLOAD_BUDGET`])
  budget: u64,
}

impl TextureAtlas {
  /// How many standalone textures it holds
  pub(crate) fn standalone_count(&self) -> usize {
    self.standalone.len()
  }

  /// An empty atlas whose textures bind with `layout` (texture, then
  /// sampler).
  pub(crate) fn new(device: &wgpu::Device, layout: wgpu::BindGroupLayout) -> Self {
    // The atlas border covers any edge overshoot of either filter. Nearest
    // keeps to the full-size level (exact texels); linear blends levels
    let sampler = |label, filter, mipmap_filter, lod_max_clamp| {
      device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some(label),
        address_mode_u: wgpu::AddressMode::ClampToEdge,
        address_mode_v: wgpu::AddressMode::ClampToEdge,
        address_mode_w: wgpu::AddressMode::ClampToEdge,
        mag_filter: filter,
        min_filter: filter,
        mipmap_filter,
        lod_max_clamp,
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
      glyphs: GlyphAtlas::new(),
      writes: 0,
      spreads: BTreeMap::new(),
      showing: Vec::new(),
      swapped: Vec::new(),
      dropped: Vec::new(),
      spent: 0,
      budget: UPLOAD_BUDGET,
      bind_layout: layout,
      nearest: sampler(
        "Sprite Sampler (nearest)",
        wgpu::FilterMode::Nearest,
        wgpu::MipmapFilterMode::Nearest,
        0.0,
      ),
      linear: sampler(
        "Sprite Sampler (linear)",
        wgpu::FilterMode::Linear,
        wgpu::MipmapFilterMode::Linear,
        32.0,
      ),
    }
  }

  /// The linear and nearest samplers every page's bind groups use (atlas
  /// and standalone pages, render targets and glyph pages)
  pub(crate) fn samplers(&self) -> (&wgpu::Sampler, &wgpu::Sampler) {
    (&self.linear, &self.nearest)
  }

  /// The bind groups sampling `view` with each sampler.
  fn bind(&self, device: &wgpu::Device, view: &wgpu::TextureView, label: &str) -> PageBindGroups {
    bind_with(
      device,
      &self.bind_layout,
      view,
      &self.nearest,
      &self.linear,
      label,
    )
  }

  /// The generation of the glyph page `page` (`None`: no such page)
  pub(crate) fn glyph_page_generation(&self, page: GlyphPage) -> Option<u64> {
    self.glyphs.page_generation(page)
  }

  /// Clear the glyph page `page` as an eviction clears it
  #[cfg(test)]
  pub(crate) fn clear_glyph_page(&mut self, page: GlyphPage) {
    self.glyphs.clear_page(page);
  }

  /// The last frame the glyph page `page` was pinned in (tests)
  #[cfg(test)]
  pub(crate) fn glyph_page_pinned(&self, page: GlyphPage) -> Option<u64> {
    self.glyphs.pinned(page)
  }

  /// Keep the glyph page `page` from eviction in `frame`
  pub(crate) fn pin_glyph_page(&mut self, page: GlyphPage, frame: u64) {
    self.glyphs.pin(page, frame);
  }

  /// Glyph pages cleared to make room so far
  pub(crate) fn glyph_evictions(&self) -> u64 {
    self.glyphs.evictions()
  }

  /// Forget the glyphs of faces freed or changed in `fonts` since the last
  /// call, freeing the glyph pages that leaves empty
  pub(crate) fn sync_fonts(&mut self, fonts: &crate::core::text::Fonts) {
    self.glyphs.sync_fonts(fonts);
  }

  /// How many glyphs of `font` are placed (tests)
  // The headless engine reads it, which the web has not
  #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
  pub(crate) fn glyphs_of(&self, font: crate::core::text::FontId) -> usize {
    self.glyphs.glyphs_of(font)
  }

  /// How many glyph pages there are (tests)
  // The headless engine reads it, which the web has not
  #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
  pub(crate) fn glyph_page_count(&self) -> usize {
    self.glyphs.page_count()
  }

  /// Glyphs refused so far for a full atlas
  pub(crate) fn glyphs_refused(&self) -> u64 {
    self.glyphs.refused()
  }

  /// The slot of a glyph image, rasterizing and uploading it on first use.
  /// A key with no image (a space) or one too big for a glyph page is
  /// remembered and not rasterized again.
  pub(crate) fn glyph(
    &mut self,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    key: GlyphKey,
    frame: u64,
    raster: impl FnOnce() -> Option<GlyphImage>,
  ) -> Option<GlyphSlot> {
    if let Some(slot) = self.glyphs.slot(&key, frame) {
      return Some(slot);
    }
    if self.glyphs.has_no_image(&key) {
      return None;
    }
    let Some(image) = raster() else {
      self.glyphs.mark_no_image(key);
      return None;
    };
    let (layout, nearest, linear) = (&self.bind_layout, &self.nearest, &self.linear);
    self
      .glyphs
      .insert(device, queue, key, &image, frame, |view| {
        bind_with(device, layout, view, nearest, linear, "Glyph Page")
      })
  }

  /// Upload (if needed) and return where `image` lives.
  ///
  /// Its pixels are only read when the image has to be written: from its
  /// upload made ahead when there is one, else bordered and halved here.
  /// Images larger than 256 px, or that cannot be placed in the atlas
  /// without evicting more than a bounded number of entries, get a
  /// standalone texture, whose mip levels below the first are the image's
  /// own when its loader made them. While a new version of `key` is being
  /// written into a new texture (`spread`), the old one is returned, or the
  /// new one finished at once should the old be gone. `None` when the pixel
  /// data does not match the size (nothing is registered then) or the
  /// image exceeds the device's texture size limit.
  pub(crate) fn slot(
    &mut self,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    key: TextureId,
    image: ImagePixels<'_>,
    frame: u64,
  ) -> Option<AtlasSlot> {
    if self.spreads.contains_key(&key) {
      if let Some(slot) = self.touch(key, frame) {
        return Some(slot);
      }
      // The old texture is gone: the new one is finished now, or, should
      // it never be whole, dropped and the pixels written below
      let make = Maker {
        device,
        layout: &self.bind_layout,
        nearest: &self.nearest,
        linear: &self.linear,
      };
      let finished = self
        .spreads
        .get_mut(&key)
        .and_then(|spread| spread.write(&make, queue, (u64::MAX, true)));
      match finished {
        Some(bytes) => {
          self.spent += bytes;
          return self.install(key, frame);
        }
        None => self.drop_spread(key),
      }
    }
    let ImagePixels {
      width,
      height,
      rgba,
      mips,
      prepared,
    } = image;
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

    let small = !own_texture(width, height);
    // A resized key falls through, so its new pixels are written
    if small && self.layout.holds(key, width, height) {
      return self.layout.place(key, width, height, frame).map(|p| p.slot);
    }

    // The pixels with their border, made ahead or here. Validated before
    // anything is registered, so a bad upload leaves no entry
    let bordered = match prepared.filter(|prepared| prepared.fits(width, height)) {
      Some(prepared) => Bordered::Ahead(prepared),
      None => Bordered::Here(extrude_border(rgba, width, height, BORDER)?),
    };
    let (bw, bh) = (width + 2 * BORDER, height + 2 * BORDER);
    if small && let Some(p) = self.layout.place(key, width, height, frame) {
      while self.pages.len() < self.layout.page_count() {
        self.pages.push(self.create_page(device));
      }
      let SlotPage::Atlas(index) = p.slot.page else {
        return None;
      };
      let page = self.pages.get(index)?;
      let written = match bordered {
        Bordered::Ahead(prepared) => write_mips(queue, &page.texture, p.x, p.y, prepared.levels()),
        Bordered::Here(bordered) => {
          let chain = mip_chain(bordered, bw, bh, MIP_LEVELS);
          let levels = chain
            .iter()
            .map(|(w, h, pixels)| (*w, *h, pixels.as_slice()));
          write_mips(queue, &page.texture, p.x, p.y, levels)
        }
      };
      self.spent += written;
      self.writes += 1;
      return Some(p.slot);
    }
    // Standalone now: a leftover atlas slot would be returned, unwritten, if
    // the key later shrinks back to its old size
    self.layout.remove(key);
    self.writes += 1;
    let levels = standalone_levels(bw, bh);
    let (texture, bind_groups) = self.create_texture(device, bw, bh, levels, "Atlas Standalone");
    let written = match bordered {
      Bordered::Ahead(prepared) if prepared_mips_fit(prepared.below(), bw, bh, levels) => {
        write_mips(queue, &texture, 0, 0, prepared.levels())
      }
      bordered => {
        // A small image's upload has a page's four levels, too few for a
        // texture of its own (a full atlas's fallback): its levels are
        // made here from its bordered pixels
        let bordered = match bordered {
          Bordered::Ahead(prepared) => prepared.bordered.clone(),
          Bordered::Here(bordered) => bordered,
        };
        match mips.filter(|mips| prepared_mips_fit(mips, bw, bh, levels)) {
          // The loader made the levels below the first on its thread
          Some(mips) => {
            let below = halvings(bw, bh)
              .skip(1)
              .zip(mips)
              .map(|((w, h), pixels)| (w, h, pixels.as_slice()));
            let first = std::iter::once((bw, bh, bordered.as_slice()));
            write_mips(queue, &texture, 0, 0, first.chain(below))
          }
          None => {
            let chain = mip_chain(bordered, bw, bh, levels);
            let chain = chain
              .iter()
              .map(|(w, h, pixels)| (*w, *h, pixels.as_slice()));
            write_mips(queue, &texture, 0, 0, chain)
          }
        }
      }
    };
    self.spent += written;
    Some(self.add_standalone(key, (texture, bind_groups), (width, height), frame))
  }

  /// Whether `upload`, a new version of `key`'s image, is written into a
  /// new texture over frames ([`spread`](Self::spread)) rather than at
  /// once: the image gets a texture of its own, `key` has a texture to
  /// draw meanwhile, and its levels are more than a frame's budget
  pub(crate) fn spreads(
    &self,
    device: &wgpu::Device,
    key: TextureId,
    upload: &PreparedUpload,
  ) -> bool {
    let (width, height) = upload.size();
    let max = u64::from(device.limits().max_texture_dimension_2d);
    let fits = |side: u32| u64::from(side) + u64::from(2 * BORDER) <= max;
    own_texture(width, height)
      && fits(width)
      && fits(height)
      && upload.bytes() > self.budget
      && self
        .lookup(key)
        .is_some_and(|slot| !matches!(slot.page, SlotPage::Target(_)))
  }

  /// Start writing `upload`, the new version of `key`'s image, into a new
  /// texture of its own, a band a frame ([`write_spreads`](Self::write_spreads)),
  /// the texture made with the first.
  /// Until it is whole `key`'s texture draws, `shows` (the image, and the
  /// version that texture shows); then it takes that texture's place
  /// ([`swap_spreads`](Self::swap_spreads)). A spread `key` had is dropped
  pub(crate) fn spread(
    &mut self,
    key: TextureId,
    upload: PreparedUpload,
    (image, version): (AssetId<Image>, u64),
  ) {
    let size = self.image_size(key).unwrap_or(upload.size());
    self.showing.retain(|showing| showing.key != key);
    self.showing.push(Showing {
      key,
      image,
      version,
      size,
    });
    self.spreads.insert(key, Spread::new(upload));
  }

  /// Write as many bands of the images being replaced as the frame's budget
  /// leaves room for, after the frame's other image writes. The first with
  /// bands left gets at least a quarter of the budget (and at least a row)
  /// whatever the frame wrote, so it ends however much else is written
  /// every frame; another starts only with room for a row. One whose upload cannot fill
  /// its texture is dropped ([`take_dropped`](Self::take_dropped))
  pub(crate) fn write_spreads(&mut self, device: &wgpu::Device, queue: &wgpu::Queue) {
    let make = Maker {
      device,
      layout: &self.bind_layout,
      nearest: &self.nearest,
      linear: &self.linear,
    };
    let (budget, spent, dropped) = (self.budget, &mut self.spent, &mut self.dropped);
    let mut first = true;
    self.spreads.retain(|key, spread| {
      // Whole and waiting for its swap: it writes nothing, and leaves the
      // first's share to the next
      if spread.done() {
        return true;
      }
      let left = budget.saturating_sub(*spent);
      let room = if first {
        left.max(budget / MIN_SPREAD_SHARE)
      } else {
        left
      };
      if room == 0 {
        return true;
      }
      // Only the first gets a row whatever its room: a later one with room
      // for less waits, its texture not made, so images reloaded together
      // make their textures (and wgpu's clears) a frame at a time
      let floor = std::mem::replace(&mut first, false);
      match spread.write(&make, queue, (room, floor)) {
        Some(bytes) => {
          *spent += bytes;
          true
        }
        None => {
          dropped.push(*key);
          false
        }
      }
    });
    let dropped = &self.dropped;
    if !dropped.is_empty() {
      self
        .showing
        .retain(|showing| !dropped.contains(&showing.key));
    }
  }

  /// Put each new texture written whole in an earlier frame in its old
  /// one's place (before a frame draws anything, so all of it draws the
  /// same pixels); their keys are taken with
  /// [`take_swapped`](Self::take_swapped)
  pub(crate) fn swap_spreads(&mut self, frame: u64) {
    while let Some(key) = self
      .spreads
      .iter()
      .find(|(_, spread)| spread.done())
      .map(|(key, _)| *key)
    {
      self.install(key, frame);
    }
  }

  /// The keys whose new texture took the old one's place since the last
  /// call
  pub(crate) fn take_swapped(&mut self) -> std::vec::Drain<'_, TextureId> {
    self.swapped.drain(..)
  }

  /// The keys whose new texture could not be written whole, dropped with
  /// it since the last call: their texture shows the old version still
  pub(crate) fn take_dropped(&mut self) -> std::vec::Drain<'_, TextureId> {
    self.dropped.drain(..)
  }

  /// Place nothing on the shared pages from now on, so every image gets a
  /// texture of its own, as when every page is full (tests)
  #[cfg(test)]
  pub(crate) fn refuse_pages(&mut self) {
    self.layout = AtlasLayout::new(PAGE_SIZE, 0);
    self.pages.clear();
  }

  /// What each image being written into a new texture draws meanwhile
  pub(crate) fn showing(&self) -> &[Showing] {
    &self.showing
  }

  /// Whether a new version of `key` is being written into a new texture
  pub(crate) fn is_spreading(&self, key: TextureId) -> bool {
    self.spreads.contains_key(&key)
  }

  /// How many images are being written into new textures (tests)
  #[cfg(test)]
  pub(crate) fn spread_count(&self) -> usize {
    self.spreads.len()
  }

  /// How many of them have made their texture (tests)
  #[cfg(test)]
  pub(crate) fn spread_textures(&self) -> usize {
    self
      .spreads
      .values()
      .filter(|spread| spread.texture.is_some())
      .count()
  }

  /// Write at most `bytes` a frame into the textures of images being
  /// replaced (tests)
  #[cfg(test)]
  pub(crate) fn set_budget(&mut self, bytes: u64) {
    self.budget = bytes;
  }

  /// Put `key`'s new texture, written whole, in its old one's place
  fn install(&mut self, key: TextureId, frame: u64) -> Option<AtlasSlot> {
    if !self.spreads.get(&key).is_some_and(Spread::done) {
      return None;
    }
    let spread = self.spreads.remove(&key)?;
    self.showing.retain(|showing| showing.key != key);
    // It carries on where the old one was used
    let last_used = self.last_used(key).unwrap_or(frame);
    if let Some(id) = self.standalone_ids.get(&key).copied() {
      self.remove_standalone(id);
    }
    self.layout.remove(key);
    self.swapped.push(key);
    let Spread {
      texture, upload, ..
    } = spread;
    // The upload's levels are freed with it
    Some(self.add_standalone(key, texture?, upload.size(), last_used))
  }

  /// Drop `key`'s new texture, which can never be whole: its old texture
  /// shows the old version still
  fn drop_spread(&mut self, key: TextureId) {
    if self.spreads.remove(&key).is_some() {
      self.showing.retain(|showing| showing.key != key);
      self.dropped.push(key);
    }
  }

  /// The slot of the texture `key` draws now, marked used in `frame`
  fn touch(&mut self, key: TextureId, frame: u64) -> Option<AtlasSlot> {
    match self.standalone_ids.get(&key) {
      Some(id) => {
        let standalone = self.standalone.get_mut(id)?;
        standalone.last_used = frame;
        Some(standalone.slot)
      }
      None => self.layout.touch(key, frame),
    }
  }

  /// The size of the image `key`'s texture shows now
  fn image_size(&self, key: TextureId) -> Option<(u32, u32)> {
    match self.standalone_ids.get(&key) {
      Some(id) => self.standalone.get(id).map(|s| s.size),
      None => self.layout.size(key),
    }
  }

  /// The last frame `key`'s texture was used in
  fn last_used(&self, key: TextureId) -> Option<u64> {
    match self.standalone_ids.get(&key) {
      Some(id) => self.standalone.get(id).map(|s| s.last_used),
      None => self.layout.last_used(key),
    }
  }

  /// How many images have been written so far, to a page or a standalone
  /// texture
  pub(crate) fn writes(&self) -> u64 {
    self.writes
  }

  /// How many textures have room now: a rect on a page or a standalone
  /// texture
  #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
  pub(crate) fn rects(&self) -> usize {
    self.layout.len() + self.standalone.len()
  }

  /// Where `key` lives now, without uploading it or marking it used.
  // The batch asks for every sprite: called, it cost a 10,000-sprite frame
  // about 4%
  #[inline]
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

  /// Forget `key`, freeing its atlas rect or standalone texture, and the
  /// new texture a new version of it was being written into.
  pub(crate) fn remove(&mut self, key: TextureId) {
    if let Some(target) = self.target_ids.remove(&key) {
      self.targets.remove(&target);
      return;
    }
    if let Some(id) = self.standalone_ids.get(&key).copied() {
      self.remove_standalone(id);
    }
    self.layout.remove(key);
    if self.spreads.remove(&key).is_some() {
      self.showing.retain(|showing| showing.key != key);
    }
  }

  /// Drop every entry, page and standalone texture.
  pub(crate) fn clear(&mut self) {
    self.layout = AtlasLayout::new(PAGE_SIZE, MAX_PAGES);
    self.pages.clear();
    self.standalone.clear();
    self.standalone_ids.clear();
    self.targets.clear();
    self.target_ids.clear();
    self.spreads.clear();
    self.showing.clear();
    self.swapped.clear();
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
      format: crate::core::render::SCENE_FORMAT,
      // Lighting copies the camera's image before lighting it in place
      usage: wgpu::TextureUsages::RENDER_ATTACHMENT
        | wgpu::TextureUsages::TEXTURE_BINDING
        | wgpu::TextureUsages::COPY_SRC,
      view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    let bind_groups = self.bind(device, &view, "Render Target");
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
      SlotPage::Glyph(page) => return self.glyphs.bind_group(page, sampling),
    };
    Some(groups.get(sampling))
  }

  /// The GPU texture behind `page` (for readback).
  // Asked for every sprite, as `lookup` is
  #[inline]
  pub(crate) fn texture(&self, page: SlotPage) -> Option<&wgpu::Texture> {
    match page {
      SlotPage::Atlas(i) => self.pages.get(i).map(|p| &p.texture),
      SlotPage::Standalone(i) => self.standalone.get(&i).map(|s| &s.texture),
      SlotPage::Target(target) => self.targets.get(&target).map(|t| &t.texture),
      SlotPage::Glyph(page) => self.glyphs.texture(page),
    }
  }

  /// Housekeeping: start the next frame's budget, drop standalone textures
  /// unused for 300 or more frames, and every [`RELEASE_EVERY`] frames free
  /// stale atlas entries and the pages left empty at the end, so the GPU
  /// memory comes back.
  pub(crate) fn end_frame(&mut self, frame: u64) {
    self.spent = 0;
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
    let (texture, bind_groups) =
      self.create_texture(device, PAGE_SIZE, PAGE_SIZE, MIP_LEVELS, "Atlas Page");
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
    mip_levels: u32,
    label: &str,
  ) -> (wgpu::Texture, PageBindGroups) {
    let make = Maker {
      device,
      layout: &self.bind_layout,
      nearest: &self.nearest,
      linear: &self.linear,
    };
    new_texture(&make, (width, height), mip_levels, label)
  }

  /// Register `texture` as `key`'s standalone texture, for an image
  /// `width`x`height` px
  fn add_standalone(
    &mut self,
    key: TextureId,
    (texture, bind_groups): (wgpu::Texture, PageBindGroups),
    (width, height): (u32, u32),
    last_used: u64,
  ) -> AtlasSlot {
    let (bw, bh) = (width + 2 * BORDER, height + 2 * BORDER);
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
        last_used,
      },
    );
    slot
  }
}

/// A page-format texture `width`x`height` with `mip_levels` levels, and its
/// bind groups
fn new_texture(
  make: &Maker<'_>,
  (width, height): (u32, u32),
  mip_levels: u32,
  label: &str,
) -> (wgpu::Texture, PageBindGroups) {
  let texture = make.device.create_texture(&wgpu::TextureDescriptor {
    label: Some(label),
    size: wgpu::Extent3d {
      width,
      height,
      depth_or_array_layers: 1,
    },
    mip_level_count: mip_levels,
    sample_count: 1,
    dimension: wgpu::TextureDimension::D2,
    format: PAGE_FORMAT,
    usage: wgpu::TextureUsages::TEXTURE_BINDING
      | wgpu::TextureUsages::COPY_DST
      | wgpu::TextureUsages::COPY_SRC,
    view_formats: &[],
  });
  let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
  let bind_groups = bind_with(
    make.device,
    make.layout,
    &view,
    make.nearest,
    make.linear,
    label,
  );
  (texture, bind_groups)
}

/// An image's pixels as the atlas takes them: `rgba` (sRGB bytes,
/// `width`x`height`), the levels below the first of its own texture its
/// loader made, and the rest of its upload made ahead, if not yet taken
#[derive(Clone, Copy)]
pub(crate) struct ImagePixels<'p> {
  pub(crate) width: u32,
  pub(crate) height: u32,
  pub(crate) rgba: &'p [u8],
  pub(crate) mips: Option<&'p [Vec<u8>]>,
  pub(crate) prepared: Option<&'p PreparedUpload>,
}

impl<'p> ImagePixels<'p> {
  /// `rgba`, `width`x`height`, with nothing made ahead (tests)
  #[cfg(test)]
  pub(crate) fn plain(width: u32, height: u32, rgba: &'p [u8]) -> Self {
    Self {
      width,
      height,
      rgba,
      mips: None,
      prepared: None,
    }
  }
}

/// An image's pixels with their border: made ahead with its levels, or
/// here
enum Bordered<'p> {
  Ahead(&'p PreparedUpload),
  Here(Vec<u8>),
}

/// What writing an image's pixels into a texture needs besides them: the
/// pixels with their border and the mip levels below, exactly what
/// [`extrude_border`] and [`mip_chain`] make at upload. The image loader
/// makes it on its thread, so an upload only copies it.
pub(crate) struct PreparedUpload {
  /// The image's size, without its border
  width: u32,
  height: u32,
  /// The pixels with their border
  bordered: Vec<u8>,
  /// The levels below: down to one texel for an image with a texture of
  /// its own, the shared pages' three for one placed on a page
  below: Arc<Vec<Vec<u8>>>,
}

impl std::fmt::Debug for PreparedUpload {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    f.debug_struct("PreparedUpload")
      .field("size", &(self.width, self.height))
      .field("levels", &(self.below.len() + 1))
      .field("bytes", &self.bytes())
      .finish()
  }
}

impl PreparedUpload {
  /// The upload of `rgba`, an image `width`x`height` px: its levels as a
  /// texture of its own or a shared page takes them. None when `rgba` is
  /// not `width * height * 4` bytes
  pub(crate) fn new(rgba: &[u8], width: u32, height: u32) -> Option<PreparedUpload> {
    let bordered = extrude_border(rgba, width, height, BORDER)?;
    let bw = width.checked_add(2 * BORDER)?;
    let bh = height.checked_add(2 * BORDER)?;
    let levels = if own_texture(width, height) {
      standalone_levels(bw, bh)
    } else {
      MIP_LEVELS
    };
    let mut chain = mip_chain(bordered, bw, bh, levels)
      .into_iter()
      .map(|(_, _, pixels)| pixels);
    let bordered = chain.next()?;
    Some(PreparedUpload {
      width,
      height,
      bordered,
      below: Arc::new(chain.collect()),
    })
  }

  /// The image's size, without its border
  pub(crate) fn size(&self) -> (u32, u32) {
    (self.width, self.height)
  }

  /// The levels below the first, which an image with a texture of its own
  /// keeps (see [`prepared_mips_fit`])
  pub(crate) fn below(&self) -> &Arc<Vec<Vec<u8>>> {
    &self.below
  }

  /// The first level: the pixels with their border (tests)
  #[cfg(test)]
  pub(crate) fn first(&self) -> &[u8] {
    &self.bordered
  }

  /// An upload of an image `width`x`height` px made of these levels, fit
  /// or not (tests)
  #[cfg(test)]
  pub(crate) fn with_levels(
    (width, height): (u32, u32),
    bordered: Vec<u8>,
    below: Vec<Vec<u8>>,
  ) -> PreparedUpload {
    PreparedUpload {
      width,
      height,
      bordered,
      below: Arc::new(below),
    }
  }

  /// Every level with its size, the bordered first one first
  fn levels(&self) -> impl Iterator<Item = (u32, u32, &[u8])> {
    let (bw, bh) = (self.width + 2 * BORDER, self.height + 2 * BORDER);
    std::iter::once((bw, bh, self.bordered.as_slice())).chain(
      halvings(bw, bh)
        .skip(1)
        .zip(self.below.iter())
        .map(|((w, h), pixels)| (w, h, pixels.as_slice())),
    )
  }

  /// Whether it is the upload of an image `width`x`height` px: made for
  /// that size, each level the size `mip_chain` halves to
  pub(crate) fn fits(&self, width: u32, height: u32) -> bool {
    let (bw, bh) = (
      width.saturating_add(2 * BORDER),
      height.saturating_add(2 * BORDER),
    );
    let levels = if own_texture(width, height) {
      standalone_levels(bw, bh)
    } else {
      MIP_LEVELS
    };
    (self.width, self.height) == (width, height)
      && self.bordered.len() as u64 == u64::from(bw) * u64::from(bh) * 4
      && prepared_mips_fit(&self.below, bw, bh, levels)
  }

  /// The bytes of its levels
  pub(crate) fn bytes(&self) -> u64 {
    let below: usize = self.below.iter().map(Vec::len).sum();
    (self.bordered.len() + below) as u64
  }
}

/// Whether an image `width`x`height` px gets a texture of its own: over
/// [`MAX_ATLAS_IMAGE`] px on a side
pub(crate) fn own_texture(width: u32, height: u32) -> bool {
  width > MAX_ATLAS_IMAGE || height > MAX_ATLAS_IMAGE
}

/// The sizes of a mip chain from `width`x`height`, halving with `mip_chain`
fn halvings(width: u32, height: u32) -> impl Iterator<Item = (u32, u32)> {
  std::iter::successors(Some((width, height)), |&(w, h)| {
    Some((w.div_ceil(2).max(1), h.div_ceil(2).max(1)))
  })
}

/// Whether `mips` are the levels below the first of a standalone texture
/// `bw`x`bh` px (border included) with `levels` levels: one per level, each
/// the size `mip_chain` halves to. Anything else is made again at upload
fn prepared_mips_fit(mips: &[Vec<u8>], bw: u32, bh: u32, levels: u32) -> bool {
  mips.len() as u64 + 1 == u64::from(levels)
    && halvings(bw, bh)
      .skip(1)
      .zip(mips)
      .all(|((w, h), pixels)| pixels.len() as u64 == u64::from(w) * u64::from(h) * 4)
}

/// The mip levels of a standalone texture `width`x`height` px, border
/// included: a full chain, down to one texel
fn standalone_levels(width: u32, height: u32) -> u32 {
  32 - width.max(height).leading_zeros()
}

/// Copy `rgba` into a `(w+2)x(h+2)` buffer whose border repeats the edge pixels.
/// `None` when `rgba` is not `width * height * 4` bytes.
pub(crate) fn extrude_border(rgba: &[u8], width: u32, height: u32, border: u32) -> Option<Vec<u8>> {
  let (w, h, d) = (width as usize, height as usize, border as usize);
  if w == 0 || h == 0 || rgba.len() != w * h * 4 {
    return None;
  }
  let bw = w + 2 * d;
  let mut out = vec![0u8; bw * (h + 2 * d) * 4];
  for by in 0..h + 2 * d {
    let sy = by.saturating_sub(d).min(h - 1);
    for bx in 0..bw {
      let sx = bx.saturating_sub(d).min(w - 1);
      let src = (sy * w + sx) * 4;
      let dst = (by * bw + bx) * 4;
      out[dst..dst + 4].copy_from_slice(&rgba[src..src + 4]);
    }
  }
  Some(out)
}

/// `rgba` (`width`x`height`, sRGB bytes, kept as the first level) and up
/// to `levels - 1` smaller levels, each halving the one before (an odd side
/// keeps its last row or column), down to one texel at most.
///
/// Texels average in linear light, weighted by alpha, so transparent
/// surroundings neither darken nor tint an edge; alpha averages as it is.
pub(crate) fn mip_chain(
  rgba: Vec<u8>,
  width: u32,
  height: u32,
  levels: u32,
) -> Vec<(u32, u32, Vec<u8>)> {
  let decode: [f32; 256] = std::array::from_fn(|byte| {
    let c = byte as f32 / 255.0;
    if c <= 0.04045 {
      c / 12.92
    } else {
      ((c + 0.055) / 1.055).powf(2.4)
    }
  });
  let encode = |linear: f32| {
    let c = linear.clamp(0.0, 1.0);
    let srgb = if c <= 0.003_130_8 {
      c * 12.92
    } else {
      1.055 * c.powf(1.0 / 2.4) - 0.055
    };
    (srgb * 255.0).round() as u8
  };
  let mut chain = vec![(width, height, rgba)];
  while (chain.len() as u32) < levels {
    let Some((w, h, pixels)) = chain.last() else {
      break;
    };
    let (w, h) = (*w as usize, *h as usize);
    if w <= 1 && h <= 1 {
      break;
    }
    let (nw, nh) = (w.div_ceil(2), h.div_ceil(2));
    let mut next = vec![0u8; nw * nh * 4];
    for y in 0..nh {
      for x in 0..nw {
        let rows = [2 * y, (2 * y + 1).min(h - 1)];
        let cols = [2 * x, (2 * x + 1).min(w - 1)];
        let (mut rgb, mut alpha) = ([0.0f32; 3], 0.0f32);
        let mut plain = [0.0f32; 3];
        for row in rows {
          for col in cols {
            let texel = &pixels[(row * w + col) * 4..][..4];
            let a = f32::from(texel[3]) / 255.0;
            for c in 0..3 {
              let linear = decode[usize::from(texel[c])];
              rgb[c] += linear * a;
              plain[c] += linear;
            }
            alpha += a;
          }
        }
        let out = &mut next[(y * nw + x) * 4..][..4];
        for c in 0..3 {
          // Fully transparent texels keep their plain average
          out[c] = encode(if alpha > 0.0 {
            rgb[c] / alpha
          } else {
            plain[c] / 4.0
          });
        }
        out[3] = (alpha / 4.0 * 255.0).round() as u8;
      }
    }
    chain.push((nw as u32, nh as u32, next));
  }
  chain
}

/// Write each level of `chain` into `texture`'s matching mip level, at
/// (`x`, `y`) scaled to that level; the bytes of the levels written.
///
/// The GPU rounds odd level sizes down where the chain rounds them up: a
/// level that would overrun its texture loses its last column or row (the
/// border's, repeating the edge).
fn write_mips<'p>(
  queue: &wgpu::Queue,
  texture: &wgpu::Texture,
  x: u32,
  y: u32,
  chain: impl IntoIterator<Item = (u32, u32, &'p [u8])>,
) -> u64 {
  let mut written = 0;
  for (level, (width, height, pixels)) in (0u32..).zip(chain) {
    if level >= texture.mip_level_count() {
      break;
    }
    let (x, y) = (x >> level, y >> level);
    let room = texture
      .size()
      .mip_level_size(level, wgpu::TextureDimension::D2);
    let copy = (
      width.min(room.width.saturating_sub(x)),
      height.min(room.height.saturating_sub(y)),
    );
    if copy.0 == 0 || copy.1 == 0 {
      continue;
    }
    queue.write_texture(
      wgpu::TexelCopyTextureInfo {
        texture,
        mip_level: level,
        origin: wgpu::Origin3d { x, y, z: 0 },
        aspect: wgpu::TextureAspect::All,
      },
      pixels,
      wgpu::TexelCopyBufferLayout {
        offset: 0,
        bytes_per_row: Some(width * 4),
        rows_per_image: Some(height),
      },
      wgpu::Extent3d {
        width: copy.0,
        height: copy.1,
        depth_or_array_layers: 1,
      },
    );
    written += u64::from(width) * u64::from(copy.1) * 4;
  }
  written
}

/// The bind groups sampling `view` with each sampler.
fn bind_with(
  device: &wgpu::Device,
  layout: &wgpu::BindGroupLayout,
  view: &wgpu::TextureView,
  nearest: &wgpu::Sampler,
  linear: &wgpu::Sampler,
  label: &str,
) -> PageBindGroups {
  let group = |sampler: &wgpu::Sampler| {
    crate::core::graphics::bind_groups::create(
      device,
      &wgpu::BindGroupDescriptor {
        label: Some(label),
        layout,
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
    nearest: group(nearest),
    linear: group(linear),
  }
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

  /// An image whose bordered rect is a quarter of a 128 px page (64 px)
  const Q: u32 = 64 - 2 * BORDER;
  /// An image whose bordered rect is half of a 256 px page (128 px)
  const H: u32 = 128 - 2 * BORDER;

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
      rects.push((p.slot.page, p.x, p.y, w + 2 * BORDER, h + 2 * BORDER));
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
      .map(|n| layout.place(id(n), Q, Q, n as u64 + 1).unwrap())
      .collect();
    assert_eq!(placed[0].slot.page, SlotPage::Atlas(0));
    assert_eq!(placed[4].slot.page, SlotPage::Atlas(1));
    assert!(placed.iter().all(|p| p.newly_allocated));
    let ninth = layout.place(id(8), Q, Q, 20).unwrap();
    assert_eq!(
      (ninth.slot, ninth.x, ninth.y),
      (placed[0].slot, placed[0].x, placed[0].y)
    );
    assert!(layout.place(id(0), Q, Q, 21).unwrap().newly_allocated);
  }

  #[test]
  fn a_full_atlas_frees_a_whole_page_instead_of_scattered_entries() {
    let mut layout = AtlasLayout::new(1024, 1);
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
        layout.place(id(n), Q, Q, 1).map(|p| p.slot.page),
        Some(SlotPage::Atlas(0))
      );
    }
    for n in 16..32 {
      assert_eq!(
        layout.place(id(n), Q, Q, 2).map(|p| p.slot.page),
        Some(SlotPage::Atlas(1))
      );
    }
    // Page 0 is older, but one of its images is drawn this frame
    layout.place(id(0), Q, Q, 3);
    let big = layout.place(id(100), H, H, 3).expect("room is made");
    assert_eq!(big.slot.page, SlotPage::Atlas(1));
    assert!((0..16).all(|n| layout.holds(id(n), Q, Q)), "page 0 kept");
  }

  #[test]
  fn stale_entries_are_freed_and_empty_trailing_pages_released() {
    let mut layout = AtlasLayout::new(128, 3);
    for n in 0..8 {
      layout.place(id(n), Q, Q, 1);
    }
    assert_eq!(layout.page_count(), 2);
    // Only one image on page 0 is still drawn
    layout.place(id(0), Q, Q, 200);
    layout.release_stale(301);
    assert!(layout.holds(id(0), Q, Q));
    assert!(!(1..8).any(|n| layout.holds(id(n), Q, Q)));
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
      .map(|e| (e.slot.page, e.x, e.y, e.w + 2 * BORDER, e.h + 2 * BORDER))
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
      .map(|n| layout.place(id(n), Q, Q, 0).unwrap())
      .collect();
    let p = layout.place(id(9), Q, Q, 300).unwrap();
    assert_eq!(layout.page_count(), 1);
    assert_eq!(p.slot.page, SlotPage::Atlas(0));
    assert!(old.iter().any(|o| (o.x, o.y) == (p.x, p.y)));
    assert_live_disjoint(&layout);
  }

  #[test]
  fn fresh_entries_are_kept_and_a_page_is_opened() {
    let mut layout = AtlasLayout::new(128, 2);
    let old: Vec<_> = (0..4)
      .map(|n| layout.place(id(n), Q, Q, 0).unwrap())
      .collect();
    let p = layout.place(id(9), Q, Q, 299).unwrap();
    assert_eq!(layout.page_count(), 2);
    assert_eq!(p.slot.page, SlotPage::Atlas(1));
    for (n, o) in old.iter().enumerate() {
      let again = layout.place(id(n), Q, Q, 299).unwrap();
      assert!(!again.newly_allocated);
      assert_eq!(again.slot, o.slot);
    }
  }

  #[test]
  fn nothing_used_this_frame_is_evicted() {
    let mut layout = AtlasLayout::new(128, 1);
    let old: Vec<_> = (0..4)
      .map(|n| layout.place(id(n), Q, Q, 5).unwrap())
      .collect();
    assert!(layout.place(id(9), Q, Q, 5).is_none());
    for (n, o) in old.iter().enumerate() {
      let again = layout.place(id(n), Q, Q, 5).unwrap();
      assert!(!again.newly_allocated);
      assert_eq!(again.slot, o.slot);
    }
  }

  #[test]
  fn lru_eviction_keeps_live_rects_disjoint() {
    let mut layout = AtlasLayout::new(128, 2);
    for n in 0..8 {
      layout.place(id(n), Q, Q, n as u64 + 1).unwrap();
    }
    for n in 8..12 {
      let p = layout.place(id(n), Q, Q, 20 + n as u64).unwrap();
      assert!(p.newly_allocated);
      assert_live_disjoint(&layout);
    }
    assert_eq!(layout.entries.len(), 8);
  }

  #[test]
  fn mip_levels_average_in_linear_light() {
    // A black and white 2x2 checker averages to linear 0.5: sRGB 188
    let rgba = [
      0, 0, 0, 255, 255, 255, 255, 255, 255, 255, 255, 255, 0, 0, 0, 255,
    ];
    let chain = mip_chain(rgba.to_vec(), 2, 2, 2);
    assert_eq!(chain.len(), 2);
    let (w, h, pixels) = &chain[1];
    assert_eq!((*w, *h), (1, 1));
    assert!((187..=189).contains(&pixels[0]), "{pixels:?}");
    assert_eq!(pixels[3], 255);
  }

  #[test]
  fn slots_line_up_with_the_smallest_mip_level() {
    let mut layout = AtlasLayout::new(PAGE_SIZE, MAX_PAGES);
    for (i, (w, h)) in [(5, 9), (16, 16), (31, 3), (1, 1)].into_iter().enumerate() {
      let placed = layout
        .place(TextureId::for_test(i as u32), w, h, 0)
        .unwrap();
      assert_eq!((placed.x % CELL, placed.y % CELL), (0, 0), "{w}x{h}");
      let [u0, v0, ..] = placed.slot.uv;
      let inset = (
        (u0 * PAGE_SIZE as f32).round() as u32 - placed.x,
        (v0 * PAGE_SIZE as f32).round() as u32 - placed.y,
      );
      assert_eq!(inset, (BORDER, BORDER));
    }
  }

  #[test]
  fn borders_repeat_edge_pixels_eight_deep() {
    let rgba = [10, 20, 30, 255];
    let bordered = extrude_border(&rgba, 1, 1, BORDER).unwrap();
    let side = (1 + 2 * BORDER) as usize;
    assert_eq!(bordered.len(), side * side * 4);
    assert!(bordered.chunks(4).all(|p| p == [10, 20, 30, 255]));
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
        .slot(&device, &queue, id(0), ImagePixels::plain(2, 2, &[0; 7]), 0)
        .is_none()
    );
    let px: Vec<u8> = (0..4u8).flat_map(|i| [10 * (i + 1), 0, 0, 255]).collect();
    let slot = atlas
      .slot(&device, &queue, id(0), ImagePixels::plain(2, 2, &px), 1)
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
      .slot(
        &device,
        &queue,
        id(0),
        ImagePixels::plain(300, 4, &[9; 300 * 4 * 4]),
        0,
      )
      .unwrap();
    let b = atlas
      .slot(
        &device,
        &queue,
        id(0),
        ImagePixels::plain(301, 4, &[9; 301 * 4 * 4]),
        1,
      )
      .unwrap();
    assert_ne!(a.uv, b.uv);
    let tex = atlas.texture(b.page).unwrap();
    assert_eq!(tex.width(), 301 + 2 * BORDER);
  }

  #[test]
  fn key_growing_standalone_then_shrinking_rewrites_its_pixels() {
    let Ok((device, queue)) = crate::core::testing::shared_gpu() else {
      return;
    };
    let mut atlas = gpu_atlas(&device);
    atlas
      .slot(
        &device,
        &queue,
        id(0),
        ImagePixels::plain(2, 2, &[10; 2 * 2 * 4]),
        0,
      )
      .unwrap();
    // Too big for the atlas, so standalone
    let big = atlas
      .slot(
        &device,
        &queue,
        id(0),
        ImagePixels::plain(300, 4, &[20; 300 * 4 * 4]),
        1,
      )
      .unwrap();
    assert!(matches!(big.page, SlotPage::Standalone(_)));
    // Back at the original size, the new pixels must be written
    let small = atlas
      .slot(
        &device,
        &queue,
        id(0),
        ImagePixels::plain(2, 2, &[77; 2 * 2 * 4]),
        2,
      )
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
      .slot(
        &device,
        &queue,
        id(0),
        ImagePixels::plain(2, 2, &[10; 2 * 2 * 4]),
        0,
      )
      .unwrap();
    let resized = atlas
      .slot(
        &device,
        &queue,
        id(0),
        ImagePixels::plain(3, 2, &[50; 3 * 2 * 4]),
        1,
      )
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
  fn a_spread_that_cannot_fill_its_texture_is_dropped_not_swapped_in() {
    let Ok((device, queue)) = crate::core::testing::shared_gpu() else {
      return;
    };
    let mut atlas = gpu_atlas(&device);
    let pixels = [9; 300 * 4 * 4];
    let old = atlas
      .slot(
        &device,
        &queue,
        id(0),
        ImagePixels::plain(300, 4, &pixels),
        0,
      )
      .unwrap();
    // A first level for the 316x20 texture, and too little below it
    let broken = PreparedUpload::with_levels((300, 4), vec![1; 316 * 20 * 4], vec![vec![2; 4]]);
    let image = crate::core::graphics::image::test_handle(Image::solid_rect(1, 1, 0, 0, 0, 255));
    atlas.spread(id(0), broken, (image.id(), 1));
    for frame in 1..4 {
      atlas.swap_spreads(frame);
      atlas.write_spreads(&device, &queue);
      atlas.end_frame(frame);
    }
    atlas.swap_spreads(4);
    assert_eq!(atlas.spread_count(), 0, "dropped");
    assert_eq!(
      atlas.take_swapped().count(),
      0,
      "never in the old one's place"
    );
    assert_eq!(atlas.take_dropped().collect::<Vec<_>>(), vec![id(0)]);
    assert_eq!(atlas.lookup(id(0)), Some(old), "the old texture draws");
    assert!(atlas.showing().is_empty());
  }

  #[test]
  fn a_finished_spread_waiting_for_its_swap_leaves_the_minimum_to_the_next() {
    let Ok((device, queue)) = crate::core::testing::shared_gpu() else {
      return;
    };
    let mut atlas = gpu_atlas(&device);
    atlas.set_budget(4 * 1024);
    let image = crate::core::graphics::image::test_handle(Image::solid_rect(1, 1, 0, 0, 0, 255));
    // A 316x20 texture of 1,264-byte rows, 33 KB in all
    let upload = |shade| PreparedUpload::new(&[shade; 300 * 4 * 4], 300, 4).unwrap();
    atlas.spread(id(0), upload(1), (image.id(), 1));
    let mut frame = 0;
    while !atlas.spreads[&id(0)].done() {
      atlas.write_spreads(&device, &queue);
      atlas.end_frame(frame);
      frame += 1;
    }
    // Whole, it waits for a swap that does not come; a second is started
    atlas.spread(id(1), upload(2), (image.id(), 1));
    let other = [9; 64 * 64 * 4];
    let start = frame;
    while !atlas.spreads[&id(1)].done() {
      assert!(frame - start < 200, "the second never ends");
      // Other writes use the whole budget: a 64 px image written again
      atlas.remove(id(9));
      atlas.slot(
        &device,
        &queue,
        id(9),
        ImagePixels::plain(64, 64, &other),
        frame,
      );
      let before = (atlas.spreads[&id(1)].level, atlas.spreads[&id(1)].row);
      atlas.write_spreads(&device, &queue);
      let after = (atlas.spreads[&id(1)].level, atlas.spreads[&id(1)].row);
      assert!(after > before, "frame {frame}: the second moves on");
      atlas.end_frame(frame);
      frame += 1;
    }
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
        .slot(
          &device,
          &queue,
          id(0),
          ImagePixels::plain(too_big, 1, &[]),
          0
        )
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
      .slot(&device, &queue, id(0), ImagePixels::plain(2, 2, &px), 0)
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
