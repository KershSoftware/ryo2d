//! Glyph images in their own pages beside the sprite atlas: R8 coverage
//! (raster glyphs), RGBA (colour glyphs, sRGB bytes) and RGBA distances
//! (MSDF). Glyphs are placed on first use and evicted a page at a time,
//! least recently used first, never one drawn this frame. Each glyph takes
//! whole 16x16 blocks of texels and is written whole ([`BLOCK`]). A face
//! that is freed or whose data changes loses its glyphs, and a page that
//! leaves empty is freed with its texture.

use std::collections::HashSet;
use std::sync::atomic::{AtomicU64, Ordering};

use etagere::{AllocId, AllocatorOptions, BucketedAtlasAllocator, DEFAULT_OPTIONS, size2};

use crate::core::graphics::atlas::PageBindGroups;
use crate::core::hash::{FastMap, FastSet};
use crate::core::render::batch::Sampling;
use crate::core::text::{FontId, Fonts, GlyphFormat, GlyphImage, GlyphKey};

/// Page edge in texels
pub(crate) const GLYPH_PAGE_SIZE: u32 = 1024;
/// Pages per format
pub(crate) const GLYPH_PAGES: usize = 8;
/// Transparent texels around each glyph
const PADDING: u32 = 1;
/// Each glyph takes whole blocks of this many texels a side: placed on a
/// grid of them and written whole, transparent around the glyph and its
/// border. An upload over part of a block was seen on Apple GPUs to leave
/// the block sampled wrong in the frame drawing it (UI text under glyph
/// page pressure: tiles of a glyph missing, or of the glyph placed there
/// before), though it read back right afterwards; uploads of whole blocks
/// never showed it
const BLOCK: u32 = 16;

/// The texels written to upload the glyph at `rect`: x, y, width, height,
/// whole blocks from the corner of its border
fn upload_region(rect: TexelRect) -> (u32, u32, u32, u32) {
  (
    rect.x - PADDING,
    rect.y - PADDING,
    (rect.width + 2 * PADDING).div_ceil(BLOCK) * BLOCK,
    (rect.height + 2 * PADDING).div_ceil(BLOCK) * BLOCK,
  )
}
/// Keys without an image (spaces) remembered at most
const MAX_NO_IMAGE: usize = 4096;

/// A glyph page: its format and index within that format
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct GlyphPage {
  pub(crate) format: GlyphFormat,
  pub(crate) index: u16,
}

/// Where a glyph's image is and how it sits on the pen
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct GlyphSlot {
  pub(crate) page: GlyphPage,
  /// u_min, v_min, u_max, v_max of the image (inside its padding)
  pub(crate) uv: [f32; 4],
  pub(crate) width: u32,
  pub(crate) height: u32,
  pub(crate) left: i32,
  pub(crate) top: i32,
  /// The pixel size the image was made at
  pub(crate) base: f32,
}

/// A placed image's texel rectangle
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TexelRect {
  pub(crate) x: u32,
  pub(crate) y: u32,
  pub(crate) width: u32,
  pub(crate) height: u32,
}

#[derive(Debug, Clone)]
pub(crate) struct ShelfEntry {
  pub(crate) format: GlyphFormat,
  pub(crate) page: usize,
  pub(crate) rect: TexelRect,
  left: i32,
  top: i32,
  base: f32,
  // Kept so a placed rectangle can be freed on its own (a forgotten face's
  // glyphs); that changes its page's generation, or texts reused from
  // earlier frames would sample whatever is placed there next
  alloc: AllocId,
  last_used: u64,
}

#[derive(Debug)]
pub(crate) enum Placement {
  Placed {
    page: usize,
    rect: TexelRect,
  },
  /// No page of this format has room and another may be made
  NeedPage,
  /// Every page holds glyphs drawn this frame
  Full,
  /// Bigger than a page
  TooBig,
}

struct ShelfPage {
  allocator: BucketedAtlasAllocator,
  keys: FastSet<GlyphKey>,
  /// Changes whenever the page is cleared, never repeating in the process
  /// (a new atlas's pages never match an old one's): what was built from
  /// its glyphs is good while it is unchanged
  generation: u64,
  /// The last frame something built from the page earlier was drawn
  /// without looking its glyphs up; the page is not evicted in that frame
  pinned: u64,
}

/// A page generation no page has had in this process
fn next_page_generation() -> u64 {
  static NEXT: AtomicU64 = AtomicU64::new(1);
  NEXT.fetch_add(1, Ordering::Relaxed)
}

/// Packing and eviction, GPU-free
pub(crate) struct GlyphShelves {
  size: u32,
  max_pages: usize,
  /// By format and index; a freed page leaves a hole, which the next page
  /// of its format takes, so the other pages keep their indices
  pages: FastMap<GlyphFormat, Vec<Option<ShelfPage>>>,
  /// The hot map: looked up for every glyph every frame
  entries: FastMap<GlyphKey, ShelfEntry>,
  warned: HashSet<(u32, u32)>,
  /// Formats found full of this frame's glyphs: warned once per run, as a
  /// sustained overflow runs out every frame
  warned_full: HashSet<GlyphFormat>,
  /// Pages cleared to make room, over the atlas's life
  evictions: u64,
  /// Glyphs refused because every page held glyphs drawn this frame
  refused: u64,
}

impl GlyphShelves {
  pub(crate) fn new(size: u32, max_pages: usize) -> Self {
    Self {
      size,
      max_pages,
      pages: FastMap::default(),
      entries: FastMap::default(),
      warned: HashSet::new(),
      warned_full: HashSet::new(),
      evictions: 0,
      refused: 0,
    }
  }

  /// The entry for `key`, marked used in `frame`
  pub(crate) fn lookup(&mut self, key: &GlyphKey, frame: u64) -> Option<&ShelfEntry> {
    let entry = self.entries.get_mut(key)?;
    entry.last_used = frame;
    Some(entry)
  }

  /// A new empty page of `format`, in the first freed place or after the
  /// others; its index
  pub(crate) fn add_page(&mut self, format: GlyphFormat) -> usize {
    let side = self.size as i32;
    let page = ShelfPage {
      // Sizes rounded to whole blocks, so every place is on the block grid
      allocator: BucketedAtlasAllocator::with_options(
        size2(side, side),
        &AllocatorOptions {
          alignment: size2(BLOCK as i32, BLOCK as i32),
          ..DEFAULT_OPTIONS
        },
      ),
      keys: FastSet::default(),
      generation: next_page_generation(),
      pinned: 0,
    };
    let pages = self.pages.entry(format).or_default();
    match pages.iter().position(Option::is_none) {
      Some(hole) => {
        if let Some(slot) = pages.get_mut(hole) {
          *slot = Some(page);
        }
        hole
      }
      None => {
        pages.push(Some(page));
        pages.len() - 1
      }
    }
  }

  fn page(&self, format: GlyphFormat, index: usize) -> Option<&ShelfPage> {
    self.pages.get(&format)?.get(index)?.as_ref()
  }

  fn page_mut(&mut self, format: GlyphFormat, index: usize) -> Option<&mut ShelfPage> {
    self.pages.get_mut(&format)?.get_mut(index)?.as_mut()
  }

  /// The pages of `format` there are, freed ones not counted
  fn live_pages(&self, format: GlyphFormat) -> usize {
    self
      .pages
      .get(&format)
      .map_or(0, |pages| pages.iter().flatten().count())
  }

  /// The pages there are, of every format (tests)
  #[cfg(test)]
  pub(crate) fn page_count(&self) -> usize {
    self.pages.values().flatten().flatten().count()
  }

  /// Forget every glyph of `font`: their room is free again, and each page
  /// they were on changes its generation. A page left with no glyph is
  /// freed; those are returned
  pub(crate) fn forget_font(&mut self, font: FontId) -> Vec<GlyphPage> {
    let pages = &mut self.pages;
    let mut touched: Vec<(GlyphFormat, usize)> = Vec::new();
    self.entries.retain(|key, entry| {
      if key.font != font {
        return true;
      }
      if let Some(page) = pages
        .get_mut(&entry.format)
        .and_then(|pages| pages.get_mut(entry.page))
        .and_then(Option::as_mut)
      {
        page.allocator.deallocate(entry.alloc);
        page.keys.remove(key);
      }
      if !touched.contains(&(entry.format, entry.page)) {
        touched.push((entry.format, entry.page));
      }
      false
    });
    let mut freed = Vec::new();
    for (format, index) in touched {
      let Some(slot) = pages
        .get_mut(&format)
        .and_then(|pages| pages.get_mut(index))
      else {
        continue;
      };
      match slot {
        Some(page) if page.keys.is_empty() => {
          *slot = None;
          freed.push(GlyphPage {
            format,
            index: u16::try_from(index).unwrap_or(u16::MAX),
          });
        }
        Some(page) => page.generation = next_page_generation(),
        None => {}
      }
    }
    // Trailing holes go, so a format's page list stays as long as it needs
    for pages in pages.values_mut() {
      while pages.last().is_some_and(Option::is_none) {
        pages.pop();
      }
    }
    freed
  }

  /// Find room for a `(w, h)` image of `key`, evicting the least recently
  /// used page of `format` if need be (never one drawn in `frame`)
  pub(crate) fn place(
    &mut self,
    key: &GlyphKey,
    format: GlyphFormat,
    (w, h): (u32, u32),
    frame: u64,
  ) -> Placement {
    let (pw, ph) = (w + 2 * PADDING, h + 2 * PADDING);
    if pw > self.size || ph > self.size {
      if self.warned.insert((w, h)) {
        log::warn!("a {w}x{h} glyph is larger than a glyph page; it is not drawn");
      }
      return Placement::TooBig;
    }
    let want = size2(pw as i32, ph as i32);
    let page_count = self.pages.get(&format).map_or(0, Vec::len);
    for index in 0..page_count {
      if let Some(placed) = self.allocate(key, format, index, want, (w, h), frame) {
        return placed;
      }
    }
    if self.live_pages(format) < self.max_pages {
      return Placement::NeedPage;
    }
    // The page whose newest glyph is oldest, if that is before this frame
    let victim = (0..page_count)
      .filter_map(|index| {
        let page = self.page(format, index)?;
        let newest = page
          .keys
          .iter()
          .filter_map(|k| self.entries.get(k))
          .map(|e| e.last_used)
          .max()
          .unwrap_or(0)
          .max(page.pinned);
        (newest < frame).then_some((newest, index))
      })
      .min();
    let Some((_, index)) = victim else {
      if self.warned_full.insert(format) {
        log::warn!(
          "the {format:?} glyph pages are full of glyphs drawn this frame; some text is not drawn"
        );
      }
      self.refused += 1;
      return Placement::Full;
    };
    self.clear_page(format, index);
    self.evictions += 1;
    self
      .allocate(key, format, index, want, (w, h), frame)
      .unwrap_or(Placement::Full)
  }

  /// Empty page `index` of `format`: its glyphs are forgotten and its
  /// generation changes
  fn clear_page(&mut self, format: GlyphFormat, index: usize) {
    if let Some(page) = self
      .pages
      .get_mut(&format)
      .and_then(|p| p.get_mut(index))
      .and_then(Option::as_mut)
    {
      page.allocator.clear();
      for evicted in page.keys.drain() {
        self.entries.remove(&evicted);
      }
      page.generation = next_page_generation();
    }
  }

  /// The generation of page `index` of `format` (`None`: no such page)
  pub(crate) fn page_generation(&self, format: GlyphFormat, index: usize) -> Option<u64> {
    Some(self.page(format, index)?.generation)
  }

  /// The last frame page `index` of `format` was pinned in (tests)
  #[cfg(test)]
  fn page_pinned(&self, format: GlyphFormat, index: usize) -> Option<u64> {
    Some(self.page(format, index)?.pinned)
  }

  /// How many glyphs of `font` are placed
  // The headless engine reads it, which the web has not
  #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
  pub(crate) fn glyphs_of(&self, font: FontId) -> usize {
    self.entries.keys().filter(|key| key.font == font).count()
  }

  /// Keep page `index` of `format` from eviction in `frame`: something
  /// built from its glyphs earlier is drawn in it
  pub(crate) fn pin(&mut self, format: GlyphFormat, index: usize, frame: u64) {
    if let Some(page) = self.page_mut(format, index) {
      page.pinned = page.pinned.max(frame);
    }
  }

  /// Pages cleared to make room so far
  pub(crate) fn evictions(&self) -> u64 {
    self.evictions
  }

  /// Glyphs refused so far because the pages were full of glyphs drawn in
  /// their frame
  pub(crate) fn refused(&self) -> u64 {
    self.refused
  }

  fn allocate(
    &mut self,
    key: &GlyphKey,
    format: GlyphFormat,
    index: usize,
    want: etagere::Size,
    (w, h): (u32, u32),
    frame: u64,
  ) -> Option<Placement> {
    let page = self.page_mut(format, index)?;
    let alloc = page.allocator.allocate(want)?;
    let rect = TexelRect {
      x: alloc.rectangle.min.x as u32 + PADDING,
      y: alloc.rectangle.min.y as u32 + PADDING,
      width: w,
      height: h,
    };
    page.keys.insert(*key);
    self.entries.insert(
      *key,
      ShelfEntry {
        format,
        page: index,
        rect,
        left: 0,
        top: 0,
        base: 0.0,
        alloc: alloc.id,
        last_used: frame,
      },
    );
    Some(Placement::Placed { page: index, rect })
  }
}

struct GpuPage {
  texture: wgpu::Texture,
  bind_groups: PageBindGroups,
}

/// The glyph pages on the GPU
pub(crate) struct GlyphAtlas {
  shelves: GlyphShelves,
  /// By format and index, as the shelves' pages (a freed page is a hole)
  pages: FastMap<GlyphFormat, Vec<Option<GpuPage>>>,
  /// Keys that have no image (spaces, refused glyphs): not asked for again
  empty: FastSet<GlyphKey>,
  /// The registry's faces and their serials as of the last `sync_fonts`,
  /// and its generation then: what the placed glyphs were made from
  faces: Vec<(FontId, u64)>,
  fonts_generation: Option<u64>,
}

impl GlyphAtlas {
  pub(crate) fn new() -> Self {
    Self {
      shelves: GlyphShelves::new(GLYPH_PAGE_SIZE, GLYPH_PAGES),
      pages: FastMap::default(),
      empty: FastSet::default(),
      faces: Vec::new(),
      fonts_generation: None,
    }
  }

  /// Bring the glyphs in line with `fonts` before a frame draws text: every
  /// glyph of a face freed or changed since the last call is forgotten
  /// (also the keys found to have no image), and a page left empty is
  /// freed with its texture. Runs between frames, so no glyph a frame drew
  /// or pinned goes while it draws. Costs a comparison when the registry has
  /// not changed
  pub(crate) fn sync_fonts(&mut self, fonts: &Fonts) {
    if self.fonts_generation == Some(fonts.generation()) {
      return;
    }
    self.fonts_generation = Some(fonts.generation());
    let faces = std::mem::take(&mut self.faces);
    for &(font, serial) in &faces {
      if fonts.face_serial(font) != Some(serial) {
        self.forget_font(font);
      }
    }
    // The snapshot holds every face a glyph can be placed from until the
    // next call. That rests on an invariant: nothing follows the font stores
    // (`fonts_follow_assets`) while a frame is drawn. The follows run after
    // the pump, at the start of the UI pass and in `ctx.fonts()`, all before
    // `present_frame`, and `Scene::render` gets no `SceneContext`. A follow
    // inside `present_frame` would let a face registered after this snapshot
    // place glyphs that a later change in place leaves stale
    self.faces = faces;
    self.faces.clear();
    self
      .faces
      .extend(fonts.faces().map(|(font, face)| (font, face.serial)));
  }

  /// Forget every glyph of `font`, and free the pages that leaves empty
  fn forget_font(&mut self, font: FontId) {
    self.empty.retain(|key| key.font != font);
    for page in self.shelves.forget_font(font) {
      if let Some(slot) = self
        .pages
        .get_mut(&page.format)
        .and_then(|pages| pages.get_mut(usize::from(page.index)))
      {
        *slot = None;
      }
    }
    for pages in self.pages.values_mut() {
      while pages.last().is_some_and(Option::is_none) {
        pages.pop();
      }
    }
  }

  /// How many glyphs of `font` are placed (tests)
  // The headless engine reads it, which the web has not
  #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
  pub(crate) fn glyphs_of(&self, font: FontId) -> usize {
    self.shelves.glyphs_of(font)
  }

  /// How many glyph pages have textures (tests)
  // The headless engine reads it, which the web has not
  #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
  pub(crate) fn page_count(&self) -> usize {
    self.pages.values().flatten().flatten().count()
  }

  /// `key` was found to have no image
  pub(crate) fn has_no_image(&self, key: &GlyphKey) -> bool {
    self.empty.contains(key)
  }

  /// Remember that `key` has no image. The set is cleared when it is full
  /// (sizes that animate make new keys forever); a forgotten key is only
  /// rasterized again.
  pub(crate) fn mark_no_image(&mut self, key: GlyphKey) {
    if self.empty.len() >= MAX_NO_IMAGE {
      self.empty.clear();
    }
    self.empty.insert(key);
  }

  /// The generation of the glyph page `page` (`None`: no such page)
  pub(crate) fn page_generation(&self, page: GlyphPage) -> Option<u64> {
    self
      .shelves
      .page_generation(page.format, usize::from(page.index))
  }

  /// Clear `page` as an eviction clears it
  #[cfg(test)]
  pub(crate) fn clear_page(&mut self, page: GlyphPage) {
    self
      .shelves
      .clear_page(page.format, usize::from(page.index));
  }

  /// The last frame `page` was pinned in (tests)
  #[cfg(test)]
  pub(crate) fn pinned(&self, page: GlyphPage) -> Option<u64> {
    self
      .shelves
      .page_pinned(page.format, usize::from(page.index))
  }

  /// Keep `page` from eviction in `frame`
  pub(crate) fn pin(&mut self, page: GlyphPage, frame: u64) {
    self
      .shelves
      .pin(page.format, usize::from(page.index), frame);
  }

  /// Pages cleared to make room so far
  pub(crate) fn evictions(&self) -> u64 {
    self.shelves.evictions()
  }

  /// Glyphs refused so far for a full atlas
  pub(crate) fn refused(&self) -> u64 {
    self.shelves.refused()
  }

  fn slot_of(entry: &ShelfEntry) -> GlyphSlot {
    let side = GLYPH_PAGE_SIZE as f32;
    let TexelRect {
      x,
      y,
      width,
      height,
    } = entry.rect;
    GlyphSlot {
      page: GlyphPage {
        format: entry.format,
        index: entry.page as u16,
      },
      uv: [
        x as f32 / side,
        y as f32 / side,
        (x + width) as f32 / side,
        (y + height) as f32 / side,
      ],
      width,
      height,
      left: entry.left,
      top: entry.top,
      base: entry.base,
    }
  }

  /// The slot of an already placed glyph, marked used in `frame`
  pub(crate) fn slot(&mut self, key: &GlyphKey, frame: u64) -> Option<GlyphSlot> {
    self.shelves.lookup(key, frame).map(Self::slot_of)
  }

  /// Place and upload `image`; `None` when it is bigger than a page (the
  /// key is then remembered as having no image) or the atlas is full of
  /// glyphs drawn this frame
  pub(crate) fn insert(
    &mut self,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    key: GlyphKey,
    image: &GlyphImage,
    frame: u64,
    bind: impl Fn(&wgpu::TextureView) -> PageBindGroups,
  ) -> Option<GlyphSlot> {
    let format = image.format;
    let size = (image.width, image.height);
    let mut placement = self.shelves.place(&key, format, size, frame);
    if matches!(placement, Placement::NeedPage) {
      let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("Glyph Page"),
        size: wgpu::Extent3d {
          width: GLYPH_PAGE_SIZE,
          height: GLYPH_PAGE_SIZE,
          depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: match format {
          GlyphFormat::Mask => wgpu::TextureFormat::R8Unorm,
          GlyphFormat::Color | GlyphFormat::Msdf => wgpu::TextureFormat::Rgba8Unorm,
        },
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
      });
      let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
      let bind_groups = bind(&view);
      let index = self.shelves.add_page(format);
      let pages = self.pages.entry(format).or_default();
      if pages.len() <= index {
        pages.resize_with(index + 1, || None);
      }
      if let Some(slot) = pages.get_mut(index) {
        *slot = Some(GpuPage {
          texture,
          bind_groups,
        });
      }
      placement = self.shelves.place(&key, format, size, frame);
    }
    let (page, rect) = match placement {
      Placement::Placed { page, rect } => (page, rect),
      // Refused for good: remembered, so it is never rasterized again
      Placement::TooBig => {
        self.mark_no_image(key);
        return None;
      }
      Placement::NeedPage | Placement::Full => return None,
    };
    let texture = &self.pages.get(&format)?.get(page)?.as_ref()?.texture;

    // One write of the image's whole blocks, transparent around it
    let bpp = if format == GlyphFormat::Mask { 1 } else { 4 };
    let (w, h) = (image.width as usize, image.height as usize);
    let (x, y, width, height) = upload_region(rect);
    let stride = width as usize * bpp;
    let border = PADDING as usize;
    let mut padded = vec![0u8; stride * height as usize];
    if image.data.len() >= w * h * bpp {
      for row in 0..h {
        let from = row * w * bpp;
        let to = (row + border) * stride + border * bpp;
        padded[to..to + w * bpp].copy_from_slice(&image.data[from..from + w * bpp]);
      }
    }
    queue.write_texture(
      wgpu::TexelCopyTextureInfo {
        texture,
        mip_level: 0,
        origin: wgpu::Origin3d { x, y, z: 0 },
        aspect: wgpu::TextureAspect::All,
      },
      &padded,
      wgpu::TexelCopyBufferLayout {
        offset: 0,
        bytes_per_row: Some(stride as u32),
        rows_per_image: Some(height),
      },
      wgpu::Extent3d {
        width,
        height,
        depth_or_array_layers: 1,
      },
    );

    let entry = self.shelves.entries.get_mut(&key)?;
    entry.left = image.left;
    entry.top = image.top;
    entry.base = image.base;
    Some(Self::slot_of(entry))
  }

  /// The bind group sampling `page` with `sampling`
  pub(crate) fn bind_group(&self, page: GlyphPage, sampling: Sampling) -> Option<&wgpu::BindGroup> {
    let gpu = self
      .pages
      .get(&page.format)?
      .get(usize::from(page.index))?
      .as_ref()?;
    Some(gpu.bind_groups.get(sampling))
  }

  /// The GPU texture behind `page` (for readback)
  pub(crate) fn texture(&self, page: GlyphPage) -> Option<&wgpu::Texture> {
    self
      .pages
      .get(&page.format)?
      .get(usize::from(page.index))?
      .as_ref()
      .map(|p| &p.texture)
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::text::{FontId, GlyphFormat, GlyphKey};

  fn key(glyph: u16) -> GlyphKey {
    GlyphKey::fill(FontId(0), glyph, 16.0, 0.0)
  }

  /// Two pages of four 12x12 glyphs each: keys 0..4 drawn in frame 1 fill
  /// page 0, keys 4..8 drawn in frame 2 fill page 1
  fn two_full_pages() -> GlyphShelves {
    let mut shelves = GlyphShelves::new(32, 2);
    shelves.add_page(GlyphFormat::Mask);
    shelves.add_page(GlyphFormat::Mask);
    for glyph in 0..8 {
      let frame = if glyph < 4 { 1 } else { 2 };
      let placed = shelves.place(&key(glyph), GlyphFormat::Mask, (12, 12), frame);
      assert!(matches!(placed, Placement::Placed { .. }), "{placed:?}");
    }
    shelves
  }

  #[test]
  fn a_pinned_page_is_not_evicted_in_its_frame() {
    let mut shelves = two_full_pages();
    // Page 0 is the least recently used, but pinned in frame 3
    shelves.pin(GlyphFormat::Mask, 0, 3);
    let placed = shelves.place(&key(100), GlyphFormat::Mask, (12, 12), 3);
    let Placement::Placed { page, .. } = placed else {
      panic!("{placed:?}")
    };
    assert_eq!(page, 1, "the unpinned page is cleared instead");
    assert!(
      shelves.lookup(&key(0), 3).is_some(),
      "the pinned page's glyphs stay"
    );
    assert!(shelves.lookup(&key(4), 3).is_none());
  }

  #[test]
  fn an_eviction_changes_the_page_generation_and_is_counted() {
    let mut shelves = two_full_pages();
    let before = (
      shelves.page_generation(GlyphFormat::Mask, 0),
      shelves.page_generation(GlyphFormat::Mask, 1),
    );
    assert_eq!(shelves.evictions(), 0);
    // Page 0 is the least recently used: cleared for the new glyph
    shelves.place(&key(100), GlyphFormat::Mask, (12, 12), 3);
    assert_eq!(shelves.evictions(), 1);
    assert_ne!(shelves.page_generation(GlyphFormat::Mask, 0), before.0);
    assert_eq!(shelves.page_generation(GlyphFormat::Mask, 1), before.1);
    assert_eq!(shelves.page_generation(GlyphFormat::Mask, 2), None);
  }

  #[test]
  fn glyphs_are_placed_in_whole_blocks() {
    let mut shelves = GlyphShelves::new(GLYPH_PAGE_SIZE, 1);
    shelves.add_page(GlyphFormat::Mask);
    for (glyph, size) in [(5, 7), (13, 13), (14, 30), (1, 1), (40, 9), (254, 195)]
      .into_iter()
      .enumerate()
    {
      let placed = shelves.place(&key(glyph as u16), GlyphFormat::Mask, size, 1);
      let Placement::Placed { rect, .. } = placed else {
        panic!("{placed:?}")
      };
      let (x, y, ..) = upload_region(rect);
      assert_eq!((x % BLOCK, y % BLOCK), (0, 0), "{size:?} at {rect:?}");
    }
  }

  #[test]
  fn an_upload_covers_whole_blocks_around_its_glyph() {
    // 254x195 and its border: 256x197, so 256x208
    let rect = TexelRect {
      x: 721,
      y: 737,
      width: 254,
      height: 195,
    };
    assert_eq!(upload_region(rect), (720, 736, 256, 208));
    let small = TexelRect {
      x: 17,
      y: 1,
      width: 5,
      height: 7,
    };
    assert_eq!(upload_region(small), (16, 0, 16, 16));
  }

  #[test]
  fn page_generations_never_repeat_across_atlases() {
    let mut first = GlyphShelves::new(32, 1);
    let mut second = GlyphShelves::new(32, 1);
    first.add_page(GlyphFormat::Mask);
    second.add_page(GlyphFormat::Mask);
    assert_ne!(
      first.page_generation(GlyphFormat::Mask, 0),
      second.page_generation(GlyphFormat::Mask, 0)
    );
  }

  #[test]
  fn a_glyph_refused_for_a_full_atlas_is_counted() {
    let mut shelves = two_full_pages();
    shelves.pin(GlyphFormat::Mask, 0, 2);
    // Frame 2: page 1 was drawn in it and page 0 is pinned in it
    assert!(matches!(
      shelves.place(&key(100), GlyphFormat::Mask, (12, 12), 2),
      Placement::Full
    ));
    assert_eq!(shelves.refused(), 1);
  }

  #[test]
  fn the_keys_without_images_stay_bounded() {
    let mut atlas = GlyphAtlas::new();
    for glyph in 0..10_000 {
      atlas.mark_no_image(key(glyph));
      assert!(
        atlas.empty.len() <= MAX_NO_IMAGE,
        "{} keys",
        atlas.empty.len()
      );
    }
    assert!(atlas.has_no_image(&key(9_999)), "the newest is kept");
  }

  #[test]
  fn a_placed_glyph_is_found_again() {
    let mut shelves = GlyphShelves::new(64, 2);
    shelves.add_page(GlyphFormat::Mask);
    let placed = shelves.place(&key(1), GlyphFormat::Mask, (10, 12), 0);
    let Placement::Placed { page, rect } = placed else {
      panic!("{placed:?}")
    };
    assert_eq!(page, 0);
    assert_eq!((rect.width, rect.height), (10, 12));
    assert_eq!(shelves.lookup(&key(1), 1).map(|e| e.rect), Some(rect));
    assert!(shelves.lookup(&key(2), 1).is_none());
  }

  #[test]
  fn a_new_page_is_asked_for_until_the_cap() {
    let mut shelves = GlyphShelves::new(32, 2);
    let mut asked = 0;
    for glyph in 0..40 {
      match shelves.place(&key(glyph), GlyphFormat::Mask, (12, 12), 0) {
        Placement::NeedPage => {
          asked += 1;
          shelves.add_page(GlyphFormat::Mask);
          assert!(matches!(
            shelves.place(&key(glyph), GlyphFormat::Mask, (12, 12), 0),
            Placement::Placed { .. }
          ));
        }
        Placement::Placed { .. } => {}
        Placement::Full => break,
        Placement::TooBig => panic!("not too big"),
      }
    }
    assert_eq!(
      asked, 2,
      "one page per format to start, then one more, up to two"
    );
  }

  #[test]
  fn a_glyph_larger_than_a_page_is_refused_once() {
    let mut shelves = GlyphShelves::new(64, 2);
    assert!(matches!(
      shelves.place(&key(9), GlyphFormat::Mask, (100, 10), 0),
      Placement::TooBig
    ));
    assert!(matches!(
      shelves.place(&key(9), GlyphFormat::Mask, (100, 10), 1),
      Placement::TooBig
    ));
    let warnings = crate::core::dev_log::get_entries()
      .iter()
      .filter(|e| e.message.contains("glyph") && e.message.contains("100x10"))
      .count();
    assert_eq!(warnings, 1);
  }

  #[test]
  fn a_full_atlas_keeps_this_frames_glyphs() {
    let mut shelves = GlyphShelves::new(32, 1);
    shelves.add_page(GlyphFormat::Mask);
    let mut placed = Vec::new();
    for glyph in 0.. {
      match shelves.place(&key(glyph), GlyphFormat::Mask, (12, 12), 5) {
        Placement::Placed { .. } => placed.push(glyph),
        Placement::Full => break,
        other => panic!("{other:?}"),
      }
    }
    assert!(!placed.is_empty());
    // Same frame: nothing drawn this frame is evicted
    assert!(matches!(
      shelves.place(&key(1000), GlyphFormat::Mask, (12, 12), 5),
      Placement::Full
    ));
    for glyph in &placed {
      assert!(shelves.lookup(&key(*glyph), 5).is_some());
    }
    // A later frame: the least recently used page is cleared for it
    assert!(matches!(
      shelves.place(&key(1000), GlyphFormat::Mask, (12, 12), 6),
      Placement::Placed { .. }
    ));
    assert!(shelves.lookup(&key(placed[0]), 6).is_none());
  }

  #[test]
  fn a_sustained_overflow_warns_once() {
    // One page of four glyphs, and every frame draws six: each frame clears
    // the page, fills it and runs out again
    let mut shelves = GlyphShelves::new(32, 1);
    shelves.add_page(GlyphFormat::Msdf);
    for frame in 1..=10 {
      for glyph in 0..6 {
        let _ = shelves.place(
          &key(frame * 10 + glyph),
          GlyphFormat::Msdf,
          (12, 12),
          u64::from(frame),
        );
      }
    }
    let warnings = crate::core::dev_log::get_entries()
      .iter()
      .filter(|e| e.message.contains("Msdf glyph pages are full"))
      .count();
    assert_eq!(warnings, 1, "warned once, not once a frame");
  }

  fn of(font: u32, glyph: u16) -> GlyphKey {
    GlyphKey::fill(FontId(font), glyph, 16.0, 0.0)
  }

  fn place(shelves: &mut GlyphShelves, key: GlyphKey) -> usize {
    match shelves.place(&key, GlyphFormat::Mask, (12, 12), 1) {
      Placement::Placed { page, .. } => page,
      other => panic!("{key:?}: {other:?}"),
    }
  }

  #[test]
  fn forgetting_a_font_frees_its_glyphs_and_keeps_the_others() {
    // One page of sixteen 12x12 glyphs: eight of font 0, then eight of font 1
    let mut shelves = GlyphShelves::new(64, 1);
    shelves.add_page(GlyphFormat::Mask);
    for font in [0, 1] {
      for glyph in 0..8 {
        place(&mut shelves, of(font, glyph));
      }
    }
    assert!(matches!(
      shelves.place(&of(2, 0), GlyphFormat::Mask, (12, 12), 1),
      Placement::Full
    ));
    let freed = shelves.forget_font(FontId(1));
    assert!(freed.is_empty(), "font 0's glyphs keep the page");
    for glyph in 0..8 {
      assert!(shelves.lookup(&of(1, glyph), 2).is_none(), "{glyph}");
      assert!(shelves.lookup(&of(0, glyph), 2).is_some(), "{glyph}");
    }
    assert_eq!(shelves.glyphs_of(FontId(1)), 0);
    // Its room is free again (the allocator takes back space a bucket at a
    // time, and these were placed together): eight more fit, nothing evicted
    for glyph in 0..8 {
      place(&mut shelves, of(2, glyph));
    }
    assert_eq!(shelves.evictions(), 0);
    assert_eq!(shelves.glyphs_of(FontId(0)), 8);
  }

  #[test]
  fn forgetting_a_font_changes_the_generation_of_the_pages_it_was_on() {
    // Pages of four: page 0 holds font 0's glyphs; page 1 font 1's, then font 0's
    let mut shelves = GlyphShelves::new(32, 2);
    shelves.add_page(GlyphFormat::Mask);
    shelves.add_page(GlyphFormat::Mask);
    for glyph in 0..4 {
      assert_eq!(place(&mut shelves, of(0, glyph)), 0);
    }
    for glyph in 0..2 {
      assert_eq!(place(&mut shelves, of(1, glyph)), 1);
    }
    assert_eq!(place(&mut shelves, of(0, 9)), 1);
    let before = (
      shelves.page_generation(GlyphFormat::Mask, 0),
      shelves.page_generation(GlyphFormat::Mask, 1),
    );
    assert!(shelves.forget_font(FontId(1)).is_empty());
    assert_eq!(shelves.page_generation(GlyphFormat::Mask, 0), before.0);
    assert!(shelves.page_generation(GlyphFormat::Mask, 1).is_some());
    assert_ne!(
      shelves.page_generation(GlyphFormat::Mask, 1),
      before.1,
      "what was built from page 1 is built again"
    );
  }

  #[test]
  fn a_page_a_forgotten_font_empties_is_freed_and_its_place_reused() {
    let mut shelves = GlyphShelves::new(32, 2);
    shelves.add_page(GlyphFormat::Mask);
    shelves.add_page(GlyphFormat::Mask);
    for glyph in 0..4 {
      assert_eq!(place(&mut shelves, of(0, glyph)), 0);
    }
    for glyph in 0..4 {
      assert_eq!(place(&mut shelves, of(1, glyph)), 1);
    }
    let freed = shelves.forget_font(FontId(1));
    assert_eq!(
      freed,
      vec![GlyphPage {
        format: GlyphFormat::Mask,
        index: 1,
      }]
    );
    assert_eq!(shelves.page_generation(GlyphFormat::Mask, 1), None, "gone");
    assert!(shelves.page_generation(GlyphFormat::Mask, 0).is_some());
    assert_eq!(shelves.page_count(), 1);
    // Page 0 is full: a new page is asked for, and takes the freed place
    assert!(matches!(
      shelves.place(&of(2, 0), GlyphFormat::Mask, (12, 12), 2),
      Placement::NeedPage
    ));
    assert_eq!(shelves.add_page(GlyphFormat::Mask), 1);
    assert_eq!(place(&mut shelves, of(2, 0)), 1);
    assert_eq!(shelves.page_count(), 2);
  }

  #[test]
  fn a_freed_page_in_the_middle_keeps_the_others_indices() {
    let mut shelves = GlyphShelves::new(32, 3);
    for _ in 0..3 {
      shelves.add_page(GlyphFormat::Mask);
    }
    for (font, page) in [(0, 0), (1, 1), (2, 2)] {
      for glyph in 0..4 {
        assert_eq!(place(&mut shelves, of(font, glyph)), page);
      }
    }
    let last = shelves.page_generation(GlyphFormat::Mask, 2);
    assert_eq!(shelves.forget_font(FontId(1)).len(), 1);
    assert_eq!(shelves.page_generation(GlyphFormat::Mask, 2), last);
    assert_eq!(
      shelves.lookup(&of(2, 0), 2).map(|entry| entry.page),
      Some(2),
      "page 2 is still page 2"
    );
    assert_eq!(shelves.add_page(GlyphFormat::Mask), 1, "the hole is filled");
  }

  #[test]
  fn formats_keep_separate_pages() {
    let mut shelves = GlyphShelves::new(64, 2);
    shelves.add_page(GlyphFormat::Mask);
    shelves.add_page(GlyphFormat::Msdf);
    let Placement::Placed { page: mask, .. } = shelves.place(&key(1), GlyphFormat::Mask, (8, 8), 0)
    else {
      panic!()
    };
    let Placement::Placed { page: msdf, .. } = shelves.place(&key(2), GlyphFormat::Msdf, (8, 8), 0)
    else {
      panic!()
    };
    assert_eq!((mask, msdf), (0, 0), "index within each format");
    assert_eq!(
      shelves.lookup(&key(2), 0).map(|e| e.format),
      Some(GlyphFormat::Msdf)
    );
  }
}
