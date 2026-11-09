//! The sprites' textures and game shaders, and the sprite instances the 2D
//! batch draws.
//!
//! - [`SpriteInstance`]: one world-space quad (transform, UV rect, tint,
//!   effect parameters, UV bounds), 128 bytes
//! - [`SpriteDraw`] and [`BatchCollector`]: `Scene::render` and particle
//!   sprites with their z, blend, filter and lighting
//! - [`SpriteGpuRenderer`]: the texture registry and atlas (every texture
//!   drawn, made resident each frame) and game sprite shaders compiled to
//!   WGSL; the batch (`render/batch`) makes the pipelines and draws
//!
//! Textures live in a texture atlas (`Rgba8Unorm` pages holding raw sRGB
//! bytes, mipmapped). The shader tints and applies effects in sRGB space and
//! converts to linear, premultiplied colour on output.

use std::collections::HashMap;
use std::collections::HashSet;

use super::atlas::{Showing, upload_border};
use super::glyph_atlas::{GlyphPage, GlyphSlot};
use super::render_targets::RenderTargets;
use super::{AtlasSlot, SlotPage, TextureAtlas, TextureId, TextureRegistry, TextureSource};
use super::{Image, RenderLayers, RenderTarget};
use crate::core::asset::{AssetId, Assets, Handle};
use crate::core::ecs::{Material, Shader, ShaderParams, SpriteEffect, SpriteShader};
use crate::core::render::batch::{BlendMode, Filter, Sampling};
use crate::core::render::ui_renderer::TexKey;
use crate::core::render::{DrawKind, DrawList, SpriteLook, SpriteShading};
use crate::core::shaders::cache::{Compiled, ShaderCache, Wanted};
use crate::core::shaders::custom::{CompiledShader, ShaderError, ShaderKind};
use crate::core::text::{GlyphImage, GlyphKey};
use crate::core::ui::Color;

/// Per-instance data for GPU sprite rendering
///
/// This struct is passed to the shader for each sprite instance.
#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct SpriteInstance {
  /// Transform matrix (2D affine: scale, rotation, translation)
  /// Row-major 3x3 packed into 3 vec4s (the last lane of row 0 holds
  /// emission - 1; the other last lanes are unused)
  pub transform: [[f32; 4]; 3],
  /// UV rect: x=u_min, y=v_min, z=u_max, w=v_max
  pub uv_rect: [f32; 4],
  /// Tint color (RGBA of the sRGB colour, straight alpha, 0-1), applied in
  /// sRGB space like the CPU renderer
  pub tint: [f32; 4],
  /// Effect parameters (meaning depends on shader)
  /// x=effect_type, y,z,w=effect-specific params
  pub effect_params: [f32; 4],
  /// Additional effect parameters
  /// Used for effects that need more than 4 floats
  pub effect_extra: [f32; 4],
  /// The sprite's own rect in atlas UVs (u_min, v_min, u_max, v_max).
  ///
  /// Set by the renderer at draw time from `uv_rect`; texture samples outside
  /// it are transparent, so effects never pick up atlas neighbours.
  pub uv_bounds: [f32; 4],
}

impl SpriteInstance {
  /// Create a basic instance with identity transform and full UV
  pub fn new() -> Self {
    Self {
      transform: [
        [1.0, 0.0, 0.0, 0.0], // row 0: scale_x, shear_x, translate_x
        [0.0, 1.0, 0.0, 0.0], // row 1: shear_y, scale_y, translate_y
        [0.0, 0.0, 1.0, 0.0], // row 2: (unused in 2D)
      ],
      uv_rect: [0.0, 0.0, 1.0, 1.0],
      tint: [1.0, 1.0, 1.0, 1.0],
      effect_params: [0.0; 4],
      effect_extra: [0.0; 4],
      uv_bounds: [0.0, 0.0, 1.0, 1.0],
    }
  }

  /// Set transform from position, rotation (radians), and scale
  pub fn with_transform(
    mut self,
    x: f32,
    y: f32,
    rotation: f32,
    scale_x: f32,
    scale_y: f32,
  ) -> Self {
    let cos = rotation.cos();
    let sin = rotation.sin();

    // 2D affine transform matrix (column-major for WGSL mat3x3)
    // WGSL mat3x3 constructor takes column vectors:
    // Column 0: X-axis transform [scale_x * cos, scale_x * sin, 0]
    // Column 1: Y-axis transform [-scale_y * sin, scale_y * cos, 0]
    // Column 2: Translation [x, y, 1]
    let emission = self.transform[0][3];
    self.transform[0] = [scale_x * cos, scale_x * sin, 0.0, emission];
    self.transform[1] = [-scale_y * sin, scale_y * cos, 0.0, 0.0];
    self.transform[2] = [x, y, 1.0, 0.0];

    self
  }

  /// Multiply the sprite's light by `emission` (default 1): above 1 it
  /// glows past white and feeds bloom. Kept as `emission - 1` in
  /// `transform[0][3]`, so a zeroed instance means 1. NaN counts as 1; the
  /// value is clamped to 0..=65504 (the most a scene image holds).
  pub fn with_emission(mut self, emission: f32) -> Self {
    let emission = if emission.is_nan() {
      1.0
    } else {
      emission.clamp(0.0, 65504.0)
    };
    self.transform[0][3] = emission - 1.0;
    self
  }

  /// The instance's emission (see [`with_emission`](Self::with_emission)).
  pub fn emission(&self) -> f32 {
    self.transform[0][3] + 1.0
  }

  /// Set UV rectangle (for sprite sheets/atlases)
  pub fn with_uv(mut self, u_min: f32, v_min: f32, u_max: f32, v_max: f32) -> Self {
    self.uv_rect = [u_min, v_min, u_max, v_max];
    self
  }

  /// Set tint color with alpha
  pub fn with_tint(mut self, color: Color, alpha: f32) -> Self {
    let a = (color.a as f32 / 255.0) * alpha;
    self.tint = [
      color.r as f32 / 255.0,
      color.g as f32 / 255.0,
      color.b as f32 / 255.0,
      a,
    ];
    self
  }

  /// Set effect parameters from a SpriteEffect
  pub fn with_effect(mut self, effect: &SpriteEffect) -> Self {
    match effect {
      SpriteEffect::Outline { color, thickness } => {
        self.effect_params = [1.0, *thickness as f32, 0.0, 0.0];
        self.effect_extra = [
          color.r as f32 / 255.0,
          color.g as f32 / 255.0,
          color.b as f32 / 255.0,
          color.a as f32 / 255.0,
        ];
      }
      SpriteEffect::Glow {
        color,
        radius,
        intensity,
      } => {
        self.effect_params = [2.0, *radius, *intensity, 0.0];
        self.effect_extra = [
          color.r as f32 / 255.0,
          color.g as f32 / 255.0,
          color.b as f32 / 255.0,
          color.a as f32 / 255.0,
        ];
      }
      SpriteEffect::Flash { color, intensity } => {
        self.effect_params = [3.0, *intensity, 0.0, 0.0];
        self.effect_extra = [
          color.r as f32 / 255.0,
          color.g as f32 / 255.0,
          color.b as f32 / 255.0,
          color.a as f32 / 255.0,
        ];
      }
      SpriteEffect::ColorShift { color, amount } => {
        self.effect_params = [4.0, *amount, 0.0, 0.0];
        self.effect_extra = [
          color.r as f32 / 255.0,
          color.g as f32 / 255.0,
          color.b as f32 / 255.0,
          color.a as f32 / 255.0,
        ];
      }
      SpriteEffect::Silhouette { color } => {
        self.effect_params = [5.0, 0.0, 0.0, 0.0];
        self.effect_extra = [
          color.r as f32 / 255.0,
          color.g as f32 / 255.0,
          color.b as f32 / 255.0,
          color.a as f32 / 255.0,
        ];
      }
      SpriteEffect::Pixelate { block_size } => {
        self.effect_params = [6.0, *block_size as f32, 0.0, 0.0];
      }
      SpriteEffect::Wave {
        amplitude,
        frequency,
        time,
        ..
      } => {
        self.effect_params = [7.0, *amplitude, *frequency, *time];
      }
      SpriteEffect::Dissolve {
        progress,
        edge_color,
      } => {
        self.effect_params = [8.0, *progress, 0.0, 0.0];
        if let Some(color) = edge_color {
          self.effect_extra = [
            color.r as f32 / 255.0,
            color.g as f32 / 255.0,
            color.b as f32 / 255.0,
            color.a as f32 / 255.0,
          ];
        }
      }
      SpriteEffect::Invert => {
        self.effect_params = [9.0, 0.0, 0.0, 0.0];
      }
      SpriteEffect::Grayscale { intensity } => {
        self.effect_params = [10.0, *intensity, 0.0, 0.0];
      }
      SpriteEffect::DirectionalOutline {
        solid_color,
        passable_color,
        thickness,
      } => {
        // The passable (bottom edge) colour is packed two 8-bit channels per
        // float (exact integers), the solid colour goes in effect_extra
        let p = passable_color;
        self.effect_params = [
          11.0,
          *thickness as f32,
          p.r as f32 * 256.0 + p.g as f32,
          p.b as f32 * 256.0 + p.a as f32,
        ];
        self.effect_extra = [
          solid_color.r as f32 / 255.0,
          solid_color.g as f32 / 255.0,
          solid_color.b as f32 / 255.0,
          solid_color.a as f32 / 255.0,
        ];
      }
    }
    self
  }
}

impl Default for SpriteInstance {
  fn default() -> Self {
    Self::new()
  }
}

/// Named pixel textures neither uploaded nor drawn for this many frames are
/// forgotten, with their pixels. An image's texture lives until its asset is
/// freed, or until the standalone cap or a full atlas needs its room
const TEXTURE_TTL: u64 = 600;

/// How often forgotten textures are pruned, and images' textures checked
/// against the image store
const PRUNE_EVERY: u64 = 120;

/// Most images kept as standalone (over 256 px) textures; beyond it the
/// least recently shown are freed (uploaded again when next drawn), so a
/// game holding hundreds of large images does not hold their textures too
const MAX_STANDALONE_ENTITY_SPRITES: usize = 64;

/// Compiled sprite shader sources no shader has used for this many frames
/// are forgotten; a shader's own entry goes when it is freed
const CUSTOM_SHADER_TTL: u64 = 600;

/// The sprites' textures and game shaders: every texture drawn lives in its
/// atlas, and game sprite shaders are compiled here; the 2D batch draws them.
pub struct SpriteGpuRenderer {
  // Game sprite shaders, compiled to WGSL; the batch makes their pipelines
  custom: ShaderCache<()>,

  // Every texture drawn lives in the atlas under its `TextureId`
  atlas: TextureAtlas,
  // The textures and their pixel sources, for (re)uploading
  registry: TextureRegistry,
  // Pixel textures by the name they were uploaded under (the shadow ellipse)
  names: HashMap<String, TextureId>,
  // Atlas frame counter, advanced by `prepare_textures`
  frame: u64,

  // Textures already made resident this frame (scratch, reused across frames)
  resident_scratch: HashSet<TextureId>,
  // The image drawn in place of one that is not loaded (debug builds)
  missing: Option<AssetId<Image>>,
  // Image textures written so far (tests watch it)
  uploads: u64,
  /// The times the standalone cap looked through the textures
  #[cfg(test)]
  standalone_scans: u64,
}

/// What a frame draws for an image: the image itself, or the checker in its
/// place
#[derive(Clone, Copy)]
pub(crate) struct ShownImage<'i> {
  pub(crate) id: AssetId<Image>,
  pub(crate) image: &'i Image,
  version: u64,
  /// The size its texture shows it at: an older version's while a new one
  /// is written into a new texture
  size: (u32, u32),
}

impl ShownImage<'_> {
  /// The size it draws at, in texels
  pub(crate) fn size(&self) -> (u32, u32) {
    self.size
  }
}

impl SpriteGpuRenderer {
  /// Create a sprite renderer: an empty atlas and no shaders yet.
  pub fn new(device: &wgpu::Device, _queue: &wgpu::Queue) -> Self {
    let texture_layout = crate::core::render::batch::texture_layout(device);
    Self {
      custom: ShaderCache::new(ShaderKind::Sprite),
      atlas: TextureAtlas::new(device, texture_layout),
      registry: TextureRegistry::new(),
      names: HashMap::new(),
      frame: 0,
      resident_scratch: HashSet::new(),
      missing: None,
      uploads: 0,
      #[cfg(test)]
      standalone_scans: 0,
    }
  }

  /// Upload a sprite texture to the GPU (cached by id).
  ///
  /// `data` is RGBA8 in sRGB. The texture goes into the texture atlas;
  /// uploading an id that is already cached does nothing, so upload each
  /// frame you draw it: textures neither uploaded nor drawn for 600 frames
  /// are dropped. Data that doesn't match `width`x`height` is ignored.
  pub fn upload_texture(
    &mut self,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    id: &str,
    width: u32,
    height: u32,
    data: &[u8],
  ) {
    self.upload_named(device, queue, id, width, height, data);
  }

  /// [`upload_texture`](Self::upload_texture), returning the texture's id
  /// (`None` when the data cannot be uploaded; nothing is kept then).
  pub(crate) fn upload_named(
    &mut self,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    name: &str,
    width: u32,
    height: u32,
    data: &[u8],
  ) -> Option<TextureId> {
    if let Some(id) = self.named_texture(name) {
      self.touch(id);
      return Some(id);
    }
    let id = self.registry.pixels(name, width, height, data, self.frame);
    if self.resident(device, queue, id).is_none() {
      self.registry.remove(id);
      return None;
    }
    self.names.insert(name.to_string(), id);
    Some(id)
  }

  /// The texture uploaded under `name`, if it is still kept.
  pub(crate) fn named_texture(&self, name: &str) -> Option<TextureId> {
    self
      .names
      .get(name)
      .copied()
      .filter(|id| self.registry.get(*id).is_some())
  }

  fn touch(&mut self, id: TextureId) {
    if let Some(entry) = self.registry.get_mut(id) {
      entry.last_used = self.frame;
    }
  }

  /// Mark the textures `ids` used this frame without making them resident:
  /// a kept UI image shows them (each once: `UiBuild::shown_textures`). One
  /// lookup each beside the registry's slot. A texture that went meanwhile
  /// stays gone until a frame draws it
  pub(crate) fn mark_shown(&mut self, ids: &[TextureId]) {
    for &id in ids {
      self.touch(id);
      self.atlas.mark_used(id, self.frame);
    }
  }

  /// Make `id` resident in the atlas: write its pixels when they are new,
  /// changed (a reloaded image) or were evicted. A large new version made
  /// ahead is written into a new texture over frames instead, the old one
  /// drawn meanwhile (`spread`). `None` when they cannot be uploaded
  /// (empty, mismatched or larger than the device allows).
  fn resident(
    &mut self,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    id: TextureId,
  ) -> Option<AtlasSlot> {
    if self.registry.needs_upload(id) && !self.spread(device, id) {
      // Changed content: free the old pixels' rect, so the new ones are written
      self.atlas.remove(id);
    }
    let entry = self.registry.get(id)?;
    // Render targets are drawn into by cameras, never uploaded
    if matches!(entry.source, TextureSource::Target) {
      return self.atlas.lookup(id);
    }
    let image = matches!(entry.source, TextureSource::Image { .. });
    let written = self.atlas.writes();
    let pixels = entry.source.pixels(entry.width, entry.height);
    let slot = self.atlas.slot(device, queue, id, pixels, self.frame);
    // While its new version is written over frames, the old one shows
    if slot.is_some() && !self.atlas.is_spreading(id) {
      self.registry.mark_uploaded(id);
    }
    if image && self.atlas.writes() != written {
      self.uploads += 1;
    }
    self.take_swaps();
    slot
  }

  /// Start writing texture `id`'s new version into a new texture over
  /// frames, while its texture draws the old one: when its loader made its
  /// upload, it is too large for a frame's budget, and it has a texture to
  /// draw meanwhile (`TextureAtlas::spreads`). Whether it did
  fn spread(&mut self, device: &wgpu::Device, id: TextureId) -> bool {
    let Some(entry) = self.registry.get(id) else {
      return false;
    };
    let TextureSource::Image {
      id: image,
      prepared: Some(upload),
      borderless,
      ..
    } = &entry.source
    else {
      return false;
    };
    let border = upload_border(entry.width, entry.height, *borderless);
    if !upload.fits(entry.width, entry.height, border) || !self.atlas.spreads(device, id, upload) {
      return false;
    }
    let shows = (*image, entry.uploaded_content.unwrap_or_default());
    let Some(upload) = self
      .registry
      .get_mut(id)
      .and_then(|entry| entry.source.take_prepared())
    else {
      return false;
    };
    self.atlas.spread(id, upload, shows);
    self.registry.mark_writing(id);
    true
  }

  /// The images whose new texture took the old one's place, which show the
  /// version written and count as uploaded, and those whose new texture
  /// was dropped unfinished, which are written at once when next drawn
  fn take_swaps(&mut self) {
    let Self {
      atlas,
      registry,
      uploads,
      ..
    } = self;
    for texture in atlas.take_swapped() {
      registry.mark_written(texture);
      *uploads += 1;
    }
    for texture in atlas.take_dropped() {
      registry.stop_writing(texture);
    }
  }

  /// Make the textures `ids` resident for this frame: re-upload what the
  /// atlas evicted, mark them used, write the bands of images being
  /// replaced that the frame's budget leaves room for, and run the atlas
  /// housekeeping.
  fn make_resident(
    &mut self,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    ids: impl Iterator<Item = TextureId>,
  ) {
    for id in ids {
      self.touch(id);
      self.resident(device, queue, id);
    }
    self.atlas.write_spreads(device, queue);
    self.atlas.end_frame(self.frame);
    self.prune();
    self.frame += 1;
  }

  /// Make the textures of `list`'s sprites and runs (particles), then
  /// `extra` (the UI's sprites), then those of `list`'s textured trails and
  /// tile chunks resident for this frame, in that order (the order they are
  /// placed in the atlas); the batch and the UI renderer then look up their
  /// [`slot`](Self::slot)s. Call once per frame.
  pub(crate) fn prepare_textures(
    &mut self,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    list: &DrawList,
    extra: &[TextureId],
  ) {
    let mut seen = std::mem::take(&mut self.resident_scratch);
    seen.clear();
    // Consecutive sprites mostly share a texture: skip repeats before hashing
    let mut previous: Option<TextureId> = None;
    let textures = (0..list.sprite_count())
      .filter_map(|i| list.sprite(i))
      .map(|sprite| sprite.texture)
      .chain(list.run_textures())
      .chain(extra.iter().copied())
      .chain(list.triangle_textures())
      .chain(list.chunk_textures())
      .filter(|&texture| {
        let repeat = previous == Some(texture);
        previous = Some(texture);
        !repeat && seen.insert(texture)
      });
    self.make_resident(device, queue, textures);
    self.resident_scratch = seen;
  }

  /// Where `texture` lives in the atlas, and the size of its page.
  // The batch asks for every sprite (see `TextureAtlas::lookup`)
  #[inline]
  pub(crate) fn slot(&self, texture: TextureId) -> Option<(AtlasSlot, (u32, u32))> {
    let slot = self.atlas.lookup(texture)?;
    let page = self.atlas.texture(slot.page)?;
    Some((slot, (page.width(), page.height())))
  }

  /// A glyph image's slot, rasterizing and uploading it on first use
  pub(crate) fn glyph(
    &mut self,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    key: GlyphKey,
    raster: impl FnOnce() -> Option<GlyphImage>,
  ) -> Option<GlyphSlot> {
    self.atlas.glyph(device, queue, key, self.frame, raster)
  }

  /// The generation of the glyph page `page` (`None`: no such page)
  pub(crate) fn glyph_page_generation(&self, page: GlyphPage) -> Option<u64> {
    self.atlas.glyph_page_generation(page)
  }

  /// Keep the glyph page `page` from eviction this frame: glyphs built from
  /// it in an earlier frame are drawn again
  pub(crate) fn pin_glyph_page(&mut self, page: GlyphPage) {
    self.atlas.pin_glyph_page(page, self.frame);
  }

  /// Clear the glyph page `page` as an eviction clears it: its glyphs are
  /// forgotten and its generation changes
  #[cfg(test)]
  pub(crate) fn clear_glyph_page(&mut self, page: GlyphPage) {
    self.atlas.clear_glyph_page(page);
  }

  /// Whether the glyph page `page` is kept from eviction in this frame
  /// (`pin_glyph_page`; tests)
  #[cfg(test)]
  pub(crate) fn glyph_page_pinned(&self, page: GlyphPage) -> bool {
    self.atlas.glyph_page_pinned(page) == Some(self.frame)
  }

  /// Glyph pages cleared to make room so far
  pub(crate) fn glyph_evictions(&self) -> u64 {
    self.atlas.glyph_evictions()
  }

  /// Bring the glyph pages in line with the font registry before a frame
  /// draws text: the glyphs of faces freed or changed since the last frame
  /// go, and a page they alone used goes with its texture
  pub(crate) fn sync_fonts(&mut self, fonts: &crate::core::text::Fonts) {
    self.atlas.sync_fonts(fonts);
  }

  /// How many glyphs of `font` are placed (tests)
  // The headless engine reads it, which the web has not
  #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
  pub(crate) fn glyphs_of(&self, font: crate::core::text::FontId) -> usize {
    self.atlas.glyphs_of(font)
  }

  /// How many glyph pages there are (tests)
  // The headless engine reads it, which the web has not
  #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
  pub(crate) fn glyph_page_count(&self) -> usize {
    self.atlas.glyph_page_count()
  }

  /// Glyphs refused so far for a full atlas
  pub(crate) fn glyphs_refused(&self) -> u64 {
    self.atlas.glyphs_refused()
  }

  /// The texture behind `key`: a glyph page, or an atlas, standalone or
  /// render-target page (`None`: no such page)
  pub(crate) fn page_texture(&self, key: TexKey) -> Option<&wgpu::Texture> {
    self.atlas.texture(match key {
      TexKey::Glyph(page) => SlotPage::Glyph(page),
      TexKey::Sprite(page) => page,
    })
  }

  /// The linear and nearest samplers the pages' bind groups use
  pub(crate) fn samplers(&self) -> (&wgpu::Sampler, &wgpu::Sampler) {
    self.atlas.samplers()
  }

  /// The bind group sampling atlas or target page `page` with `sampling`.
  pub(crate) fn page_bind_group(
    &self,
    page: SlotPage,
    sampling: Sampling,
  ) -> Option<&wgpu::BindGroup> {
    self.atlas.bind_group(page, sampling)
  }

  /// The name and composed WGSL of the game shader with source key `key`
  /// (from [`shade_custom`](Self::shade_custom)), while it is kept.
  pub(crate) fn custom_shader(&self, key: u64) -> Option<(&str, &str)> {
    let compiled = self.custom.source(key)?;
    let shader = compiled.shader.as_ref().ok()?;
    Some((compiled.name.as_str(), shader.wgsl.as_str()))
  }

  /// The GPU accepted a pipeline made from the game shader source `key`
  /// (`Ok`) or rejected it (`Err`, its message): a shader drawing a
  /// rejected source draws its last good version
  pub(crate) fn custom_shader_settled(&mut self, key: u64, outcome: Result<(), String>) {
    match outcome {
      Ok(()) => self.custom.accepted(key),
      Err(message) => self.custom.rejected(key, message),
    }
  }

  /// Whether a game shader waits for a newer version's pipelines, made off
  /// the main thread
  pub(crate) fn custom_shaders_waiting(&self) -> bool {
    self.custom.waiting()
  }

  /// The newer versions game shaders wait for, for the batch to make their
  /// pipelines on the shader thread
  pub(crate) fn wanted_custom_shaders(&self) -> impl Iterator<Item = Wanted<'_, ()>> {
    self.custom.wanted()
  }

  /// A game shader source the shader thread compiled came back
  /// (`ShaderCache::landed`): `outcome` says whether the GPU accepted its
  /// pipelines, which the batch keeps; `checking`, whether a pipeline the
  /// main thread made from it still has its check out
  pub(crate) fn custom_shader_landed(
    &mut self,
    key: u64,
    name: &str,
    compiled: Result<std::sync::Arc<CompiledShader>, ShaderError>,
    outcome: Result<(), String>,
    checking: bool,
  ) {
    self
      .custom
      .landed(key, name, compiled, None, outcome, checking);
  }

  /// The job compiling game shader source `key` panicked
  /// (`ShaderCache::panicked`)
  pub(crate) fn custom_shader_panicked(&mut self, key: u64) {
    self.custom.panicked(key);
  }

  /// Moves each game shader waiting for a newer version to it once `ready`
  /// says its pipelines are made (`ShaderCache::advance`)
  pub(crate) fn advance_custom_shaders(
    &mut self,
    ready: impl FnMut(u64, &Compiled<()>, u64) -> bool,
  ) {
    self.custom.advance(ready);
  }

  /// Whether game shader `id` waits to be drawn at its current version
  /// (tests and benches that measure a reload until it is drawn)
  #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
  pub(crate) fn custom_shader_pending(
    &self,
    id: AssetId<Shader>,
    shaders: &Assets<Shader>,
  ) -> bool {
    self.custom.pending(id, shaders)
  }

  /// Whether a game shader still uses the source key `key`: the batch keeps
  /// its pipelines only while one does
  pub(crate) fn custom_shader_in_use(&self, key: u64) -> bool {
    self.custom.in_use(key)
  }

  /// Compile (or reuse) the game shader and pack `params` into `instance`.
  ///
  /// Returns the key to draw the sprite with ([`SpriteShading::Custom`];
  /// shaders with equal sources share it), or `None` when the shader is not
  /// loaded or failed to compile, in which case the sprite draws plain. Call
  /// at emit time, so nothing per-sprite has to be kept until the frame is
  /// laid out.
  pub(crate) fn shade_custom(
    &mut self,
    shader: AssetId<Shader>,
    shaders: &Assets<Shader>,
    params: &ShaderParams,
    instance: &mut SpriteInstance,
  ) -> Option<u64> {
    let (key, compiled) = self.custom.prepare(shader, shaders, self.frame)?;
    compiled.shader.as_ref().ok()?;
    // Eight floats: the instance's two effect lanes
    let mut bytes = [0u8; 32];
    self.custom.pack(shader, shaders, params, &mut bytes)?;
    let floats: [f32; 8] = bytemuck::cast(bytes);
    instance.effect_params = [floats[0], floats[1], floats[2], floats[3]];
    instance.effect_extra = [floats[4], floats[5], floats[6], floats[7]];
    Some(key)
  }

  /// Bring the game shaders in line with the shader store before a frame
  /// draws: the shaders freed since the last frame (`removed`, from
  /// `AssetEvent::Removed`) are forgotten at once, and the batch drops the
  /// pipelines no shader uses any more. Now and then every shader is checked
  /// against `shaders` too, so one whose event was missed does not stay
  pub(crate) fn sync_shaders(&mut self, removed: &[AssetId<Shader>], shaders: &Assets<Shader>) {
    for &shader in removed {
      self.custom.remove(shader);
    }
    if self.frame.is_multiple_of(PRUNE_EVERY) {
      self.custom.sweep(shaders);
    }
  }

  /// Where a texture uploaded by name currently lives in the atlas.
  #[cfg(test)]
  pub(crate) fn lookup(&self, name: &str) -> Option<AtlasSlot> {
    self.atlas.lookup(self.named_texture(name)?)
  }

  /// Where an image's texture currently lives in the atlas.
  #[cfg(test)]
  pub(crate) fn lookup_image(&self, image: AssetId<Image>) -> Option<AtlasSlot> {
    self.atlas.lookup(self.registry.image_texture(image)?)
  }

  /// Make the GPU textures of `targets` match it: create the new ones,
  /// recreate the resized ones and free the removed ones.
  pub(crate) fn sync_targets(&mut self, device: &wgpu::Device, targets: &RenderTargets) {
    for id in self
      .registry
      .retain_targets(|target| targets.contains(target))
    {
      self.atlas.remove(id);
    }
    for (target, size) in targets.iter() {
      let id = self.registry.target(target, size.0, size.1);
      self.atlas.sync_target(device, id, target, size);
    }
  }

  /// The id sprites showing `target` draw with, once its texture exists.
  pub(crate) fn target_texture(&self, target: RenderTarget) -> Option<TextureId> {
    self.registry.target_id(target)
  }

  /// `target`'s texture and the view cameras draw into.
  pub(crate) fn target_image(
    &self,
    target: RenderTarget,
  ) -> Option<(&wgpu::Texture, &wgpu::TextureView)> {
    self.atlas.target(target)
  }

  /// What a frame draws for image `id`: the image while it is loaded; in
  /// debug builds the checker while it is not (not yet, failed or freed),
  /// and in release builds nothing
  pub(crate) fn shown_image<'i>(
    &self,
    id: AssetId<Image>,
    images: &'i Assets<Image>,
  ) -> Option<ShownImage<'i>> {
    let shown = |id: AssetId<Image>| {
      let image = images.get(id)?;
      Some(ShownImage {
        id,
        image,
        version: images.version(id)?,
        size: self
          .showing(id)
          .map_or((image.width(), image.height()), |showing| showing.size),
      })
    };
    shown(id).or_else(|| {
      if cfg!(debug_assertions) {
        shown(self.missing?)
      } else {
        None
      }
    })
  }

  /// The texture image `id` draws with (or the checker, see
  /// [`shown_image`](Self::shown_image)), registered on first use; its
  /// pixels are uploaded when a frame draws it.
  pub(crate) fn texture_of(
    &mut self,
    id: AssetId<Image>,
    images: &Assets<Image>,
  ) -> Option<TextureId> {
    let shown = self.shown_image(id, images)?;
    Some(self.shown_texture(shown))
  }

  /// The texture of an image [`shown_image`](Self::shown_image) chose
  pub(crate) fn shown_texture(&mut self, shown: ShownImage<'_>) -> TextureId {
    self
      .registry
      .image(shown.id, shown.image, shown.version, self.frame)
  }

  /// Whether `id` is still a texture (unused ones are forgotten).
  pub(crate) fn is_texture(&self, id: TextureId) -> bool {
    self.registry.get(id).is_some()
  }

  /// Make an entity's image resident and return the texture to draw it by.
  ///
  /// Every entity sharing an image shares one texture, and a reloaded
  /// image is uploaded again. Returns `None` for an image that cannot be
  /// uploaded (empty, or larger than the device allows).
  pub(crate) fn upload_entity_image(
    &mut self,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    shown: ShownImage<'_>,
  ) -> Option<TextureId> {
    let id = self.shown_texture(shown);
    // Uploaded ones are made resident with the frame's other textures
    if self.registry.needs_upload(id) {
      self.resident(device, queue, id)?;
    }
    Some(id)
  }

  /// Bring the images' textures in line with the image store before a
  /// frame draws: the textures of the images freed since the last frame
  /// (`removed`, from `AssetEvent::Removed`) go at once, with their atlas
  /// space and any new texture being written for them; new textures
  /// written whole take their old ones' places; and an image changed since
  /// (`changed`, from `AssetEvent::Modified`) that has a texture takes its
  /// new version, a large one made ahead starting to be written into a new
  /// texture over frames, so all of this frame draws its old pixels. A
  /// large one with no texture to draw meanwhile keeps no bordered copy of
  /// its pixels: its next draw borders them.
  /// `missing` is the checker drawn in place of an image that is not
  /// loaded (debug builds). Now and then every image texture is checked
  /// against `images` too, so one whose event was missed (a frame not
  /// drawn) does not stay
  pub(crate) fn sync_images(
    &mut self,
    device: &wgpu::Device,
    removed: impl IntoIterator<Item = AssetId<Image>>,
    changed: impl IntoIterator<Item = AssetId<Image>>,
    images: &Assets<Image>,
    missing: Option<AssetId<Image>>,
  ) {
    self.missing = missing;
    for image in removed {
      if let Some(texture) = self.registry.image_texture(image) {
        self.atlas.remove(texture);
        self.registry.remove(texture);
      }
    }
    self.atlas.swap_spreads();
    self.take_swaps();
    for image in changed {
      let (Some(value), Some(version)) = (images.get(image), images.version(image)) else {
        continue;
      };
      let Some(texture) = self.registry.image_texture(image) else {
        // No texture: not drawn yet, or the standalone cap freed it for
        // room (with its entry). A large one's next draw borders it, rather
        // than it keeping a second copy of its pixels until then
        if value.has_own_texture() {
          value.take_upload();
        }
        continue;
      };
      self.registry.refresh(texture, value, version);
      if self.registry.needs_upload(texture) && !self.spread(device, texture) {
        // A large one with no texture to draw meanwhile (it could not be
        // uploaded) is
        // bordered when next drawn, as a first draw is; one within the
        // budget is written at once with what its loader made
        if value.has_own_texture()
          && let Some(entry) = self.registry.get_mut(texture)
          && self.atlas.lookup(texture).is_none()
        {
          entry.source.take_prepared();
        }
      }
    }
    if self.frame.is_multiple_of(PRUNE_EVERY) {
      let gone: Vec<TextureId> = self
        .registry
        .images()
        .filter(|(image, _)| images.get(*image).is_none())
        .map(|(_, texture)| texture)
        .collect();
      for texture in gone {
        self.atlas.remove(texture);
        self.registry.remove(texture);
      }
    }
  }

  /// What each image being written into a new texture draws meanwhile:
  /// its old texture, at an older version and size
  pub(crate) fn showing_old(&self) -> &[Showing] {
    self.atlas.showing()
  }

  /// What image `id`'s texture shows while its new version is written
  fn showing(&self, id: AssetId<Image>) -> Option<&Showing> {
    self
      .atlas
      .showing()
      .iter()
      .find(|showing| showing.image == id)
  }

  /// Forget named pixel textures neither uploaded nor drawn for
  /// [`TEXTURE_TTL`] frames, with their pixels and atlas space, and cap the
  /// standalone images. Games upload what they draw each frame, so a
  /// texture still in use is never dropped.
  fn prune(&mut self) {
    self.cap_standalone_sprites();
    if !self.frame.is_multiple_of(PRUNE_EVERY) {
      return;
    }
    self.custom.prune(self.frame, CUSTOM_SHADER_TTL);
    for id in self.registry.prune(self.frame, TEXTURE_TTL) {
      self.atlas.remove(id);
    }
    let registry = &self.registry;
    self.names.retain(|_, id| registry.get(*id).is_some());
  }

  /// Free the least recently used standalone image textures beyond
  /// [`MAX_STANDALONE_ENTITY_SPRITES`], sparing any used this frame; a
  /// freed one is uploaded again when a frame draws it.
  fn cap_standalone_sprites(&mut self) {
    // Every standalone image is one of the atlas's standalone textures
    if self.atlas.standalone_count() <= MAX_STANDALONE_ENTITY_SPRITES {
      return;
    }
    #[cfg(test)]
    {
      self.standalone_scans += 1;
    }
    let mut standalone = self.standalone_sprites();
    let Some(excess) = standalone
      .len()
      .checked_sub(MAX_STANDALONE_ENTITY_SPRITES)
      .filter(|n| *n > 0)
    else {
      return;
    };
    standalone.sort_by_key(|&(_, last_used)| last_used);
    let frame = self.frame;
    let evict: Vec<TextureId> = standalone
      .into_iter()
      .filter(|&(_, last_used)| last_used < frame)
      .take(excess)
      .map(|(id, _)| id)
      .collect();
    for id in evict {
      self.atlas.remove(id);
      self.registry.remove(id);
    }
  }

  /// Images holding a standalone texture, as `(id, last_used)`.
  fn standalone_sprites(&self) -> Vec<(TextureId, u64)> {
    self
      .registry
      .iter()
      .filter(|(id, entry)| {
        matches!(entry.source, TextureSource::Image { .. })
          && self
            .atlas
            .lookup(*id)
            .is_some_and(|slot| matches!(slot.page, SlotPage::Standalone(_)))
      })
      .map(|(id, entry)| (id, entry.last_used))
      .collect()
  }

  /// How many textures are kept (for tests that watch for leaks).
  // The headless engine reads it, which the web has not
  #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
  pub(crate) fn texture_count(&self) -> usize {
    self.registry.len()
  }

  /// How many times an image's pixels were written to the GPU (for tests
  /// that watch what is uploaded again).
  // The headless engine reads it, which the web has not
  #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
  pub(crate) fn image_uploads(&self) -> u64 {
    self.uploads
  }

  /// Whether image `id`'s new version is being written into a new texture
  /// over frames, its old texture drawn meanwhile
  // The headless engine reads it, which the web has not
  #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
  pub(crate) fn upload_pending(&self, id: AssetId<Image>) -> bool {
    self.showing(id).is_some()
  }

  /// How many images are being written into new textures (tests)
  #[cfg(test)]
  pub(crate) fn spread_uploads(&self) -> usize {
    self.atlas.spread_count()
  }

  /// The bytes a frame writes into textures being replaced (tests)
  #[cfg(test)]
  pub(crate) fn set_upload_budget(&mut self, bytes: u64) {
    self.atlas.set_budget(bytes);
  }

  /// Give every image a texture of its own from now on, as a full atlas
  /// does (tests)
  #[cfg(test)]
  pub(crate) fn refuse_atlas_pages(&mut self) {
    self.atlas.refuse_pages();
  }

  /// Whether image `id`'s texture holds an upload made ahead, not yet
  /// written (tests)
  #[cfg(test)]
  pub(crate) fn holds_prepared(&self, id: AssetId<Image>) -> bool {
    self
      .registry
      .image_texture(id)
      .and_then(|texture| self.registry.get(texture))
      .is_some_and(|entry| entry.source.prepared().is_some())
  }

  /// How many textures have room in the atlas (for tests that watch it
  /// freed).
  // The headless engine reads it, which the web has not
  #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
  pub(crate) fn atlas_rects(&self) -> usize {
    self.atlas.rects()
  }

  /// How many sprites hold a standalone texture.
  #[cfg(test)]
  pub(crate) fn standalone_entity_count(&self) -> usize {
    self.standalone_sprites().len()
  }

  /// Whether the image is uploaded (for tests of what gets uploaded).
  #[cfg(test)]
  pub(crate) fn has_image(&self, image: AssetId<Image>) -> bool {
    self
      .registry
      .image_texture(image)
      .is_some_and(|id| !self.registry.needs_upload(id))
  }

  /// Clear the texture cache
  pub fn clear_cache(&mut self) {
    self.registry = TextureRegistry::new();
    self.names.clear();
    self.atlas.clear();
  }
}

/// Atlas padding (texels on every side) of the effect in `params`: outline,
/// glow and directional outline grow the sprite as the CPU renderer did.
pub(crate) fn effect_padding(params: &[f32; 4]) -> f32 {
  match params[0] as i32 {
    // Outline, DirectionalOutline: thickness
    1 | 11 => params[1].max(0.0),
    // Glow: the radius in whole texels, at least 1
    2 => params[1].floor().max(1.0),
    _ => 0.0,
  }
}

/// Map an instance into its atlas slot.
///
/// `uv_rect` (normalized over the texture) is remapped into the slot and
/// `uv_bounds` becomes the sprite's own rect, clipped to the slot. When
/// `effects` is set and the effect grows the sprite, the quad and its UVs are
/// enlarged by the effect's padding around the same centre, so the effect can
/// draw outside the sprite. `page_size` is the size of the slot's texture.
pub(crate) fn place_in_slot(
  instance: &SpriteInstance,
  slot: &AtlasSlot,
  page_size: (u32, u32),
  effects: bool,
) -> SpriteInstance {
  let [s0, t0, s1, t1] = slot.uv;
  let [u0, v0, u1, v1] = instance.uv_rect;
  let (du, dv) = (s1 - s0, t1 - t0);
  let mut uv = [s0 + u0 * du, t0 + v0 * dv, s0 + u1 * du, t0 + v1 * dv];
  let mut out = *instance;
  out.uv_bounds = [
    uv[0].min(uv[2]).max(s0),
    uv[1].min(uv[3]).max(t0),
    uv[0].max(uv[2]).min(s1),
    uv[1].max(uv[3]).min(t1),
  ];

  let pad = if effects {
    effect_padding(&instance.effect_params)
  } else {
    0.0
  };
  let texel_u = 1.0 / page_size.0.max(1) as f32;
  let texel_v = 1.0 / page_size.1.max(1) as f32;
  // Sprite size in texels
  let width = (uv[2] - uv[0]).abs() / texel_u;
  let height = (uv[3] - uv[1]).abs() / texel_v;
  if pad > 0.0 && width > 0.0 && height > 0.0 {
    let grow_x = (width + 2.0 * pad) / width;
    let grow_y = (height + 2.0 * pad) / height;
    for c in 0..2 {
      out.transform[0][c] *= grow_x;
      out.transform[1][c] *= grow_y;
    }
    // Grow outward, also when the UVs are flipped
    let pad_u = (uv[2] - uv[0]).signum() * pad * texel_u;
    let pad_v = (uv[3] - uv[1]).signum() * pad * texel_v;
    uv[0] -= pad_u;
    uv[2] += pad_u;
    uv[1] -= pad_v;
    uv[3] += pad_v;
  }
  out.uv_rect = uv;
  out
}

/// The shader that applies to a sprite: its `Material`'s, else the inline one.
pub(crate) fn effective_shader<'a>(
  material: Option<&'a Material>,
  inline_shader: Option<&'a SpriteShader>,
) -> Option<&'a SpriteShader> {
  material.map(|m| &m.shader).or(inline_shader)
}

/// Get the shader effect from a Material or inline shader
pub fn get_sprite_effect<'a>(
  material: Option<&'a Material>,
  inline_shader: Option<&'a SpriteShader>,
) -> Option<&'a SpriteEffect> {
  effective_shader(material, inline_shader).and_then(SpriteShader::as_builtin)
}

/// The WGSL shader for sprite rendering (loaded from external file)
pub const SPRITE_SHADER: &str = crate::core::shaders::compose::SPRITE;

// =============================================================================
// Sprite Batching System
// =============================================================================

/// How a `Scene::render`, particle or parallax sprite is drawn.
///
/// ```ignore
/// let glow = SpriteDraw { z: 5, blend: BlendMode::Additive, ..Default::default() };
/// gpu.add_sprite(&spark, instance, glow);
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpriteDraw {
  /// Draw order against entities, sprites and trails (higher draws on top)
  pub z: i32,
  /// How its colour combines with what is below
  pub blend: BlendMode,
  /// How its texture is sampled when drawn larger or smaller
  pub filter: Filter,
  /// Lit by 2D lighting; `false` draws it after lighting, at full brightness
  pub lit: bool,
  /// Its instance carries a built-in effect (`SpriteInstance::with_effect`)
  pub effects: bool,
  /// Multiplies the instance's emission (default 1)
  pub emission: f32,
}

impl Default for SpriteDraw {
  /// At z 0, blended over what is below, with the presentation's filter,
  /// lit, without effects.
  fn default() -> Self {
    Self {
      z: 0,
      blend: BlendMode::Alpha,
      filter: Filter::Default,
      lit: true,
      effects: false,
      emission: 1.0,
    }
  }
}

impl SpriteDraw {
  /// Multiply the instance's emission by `emission`.
  pub fn with_emission(mut self, emission: f32) -> Self {
    self.emission = emission;
    self
  }

  /// How the draw list takes a sprite drawn with `shading`.
  fn look(&self, shading: SpriteShading) -> SpriteLook {
    SpriteLook {
      shading,
      blend: self.blend,
      filter: self.filter,
      lit: self.lit,
    }
  }
}

/// Collects the frame's `Scene::render`, particle and parallax sprites.
///
/// Sprites are kept in submission order and join the frame's z-sorted draw
/// list, where equal-z sprites draw in submission order:
///
/// ```ignore
/// let mut collector = BatchCollector::new();
/// collector.add(&image, instance, SpriteDraw { z: 2, ..Default::default() });
/// ```
#[derive(Debug, Default)]
pub struct BatchCollector {
  /// Every sprite in submission order
  items: Vec<CollectedSprite>,
  /// The images this frame's sprites draw, each once (an index in
  /// `CollectedSprite`)
  images: Vec<AssetId<Image>>,
  image_index: HashMap<AssetId<Image>, usize>,
  /// Each image's texture while pushing to the draw list (reused)
  textures: Vec<Option<TextureId>>,
}

/// One collected sprite, as the draw list takes it.
#[derive(Debug)]
struct CollectedSprite {
  /// Index of its image in `images`
  image: usize,
  look: SpriteLook,
  z_order: i32,
  instance: SpriteInstance,
}

impl BatchCollector {
  /// Create a new, empty collector.
  pub fn new() -> Self {
    Self::default()
  }

  /// Create a collector with room for `sprites` sprites.
  pub fn with_capacity(sprites: usize) -> Self {
    Self {
      items: Vec::with_capacity(sprites),
      ..Self::default()
    }
  }

  /// Add a sprite drawing `image` as `draw` says. An image that is not
  /// loaded draws the checker in debug builds, and nothing in release ones.
  pub fn add(&mut self, image: &Handle<Image>, instance: SpriteInstance, draw: SpriteDraw) {
    self.add_image(image.id(), instance, draw);
  }

  /// [`add`](Self::add) by the image's id
  pub(crate) fn add_image(
    &mut self,
    image: AssetId<Image>,
    instance: SpriteInstance,
    draw: SpriteDraw,
  ) {
    let shading = if draw.effects {
      SpriteShading::Effect
    } else {
      SpriteShading::Plain
    };
    self.add_shaded(image, shading, instance, draw);
  }

  /// Add a sprite with its shading already decided.
  pub(crate) fn add_shaded(
    &mut self,
    image: AssetId<Image>,
    shading: SpriteShading,
    instance: SpriteInstance,
    draw: SpriteDraw,
  ) {
    let instance = instance.with_emission(instance.emission() * draw.emission);
    let index = match self.image_index.get(&image) {
      Some(&index) => index,
      None => {
        self.images.push(image);
        self.image_index.insert(image, self.images.len() - 1);
        self.images.len() - 1
      }
    };
    self.items.push(CollectedSprite {
      image: index,
      look: draw.look(shading),
      z_order: draw.z,
      instance,
    });
  }

  /// Push every collected sprite (world-space instances) onto `list` as
  /// `kind`, in submission order, with its image's texture registered in
  /// `gpu`.
  pub(crate) fn push_to(
    &mut self,
    list: &mut DrawList,
    kind: DrawKind,
    gpu: &mut SpriteGpuRenderer,
    images: &Assets<Image>,
  ) {
    self.textures.clear();
    let textures = self
      .images
      .iter()
      .map(|&image| gpu.texture_of(image, images));
    self.textures.extend(textures);
    for item in &self.items {
      if let Some(&Some(texture)) = self.textures.get(item.image) {
        let everyone = (RenderLayers::default(), None, None);
        list.push_sprite_in(
          kind,
          item.z_order,
          texture,
          item.look,
          item.instance,
          everyone,
        );
      }
    }
  }

  /// The collected instances, in submission order (tests).
  #[cfg(test)]
  pub(crate) fn instances(&self) -> impl Iterator<Item = &SpriteInstance> {
    self.items.iter().map(|item| &item.instance)
  }

  /// Get the total number of sprites collected
  pub fn sprite_count(&self) -> usize {
    self.items.len()
  }

  /// Forget this frame's sprites, keeping the allocations.
  pub fn clear(&mut self) {
    self.items.clear();
    self.images.clear();
    self.image_index.clear();
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::graphics::SlotPage;
  use crate::core::shaders::custom;

  #[test]
  fn emission_lives_in_the_unused_transform_lane() {
    let zeroed: SpriteInstance = bytemuck::Zeroable::zeroed();
    assert_eq!(zeroed.emission(), 1.0, "a zeroed lane means no glow");
    let glowing = SpriteInstance::new()
      .with_emission(3.0)
      .with_transform(10.0, 20.0, 0.5, 2.0, 2.0);
    assert_eq!(glowing.emission(), 3.0, "with_transform keeps it");
    assert_eq!(
      SpriteInstance::new().with_emission(f32::NAN).emission(),
      1.0
    );
    assert_eq!(SpriteInstance::new().with_emission(-2.0).emission(), 0.0);
    assert_eq!(
      SpriteInstance::new()
        .with_emission(f32::INFINITY)
        .emission(),
      65504.0
    );
  }

  #[test]
  fn sprite_instance_size() {
    // Verify instance size matches expected (for GPU buffer layout)
    assert_eq!(std::mem::size_of::<SpriteInstance>(), 128);
  }

  #[test]
  fn sprite_instance_default() {
    let instance = SpriteInstance::new();
    assert_eq!(instance.transform[0][0], 1.0);
    assert_eq!(instance.transform[1][1], 1.0);
    assert_eq!(instance.uv_rect, [0.0, 0.0, 1.0, 1.0]);
    assert_eq!(instance.tint, [1.0, 1.0, 1.0, 1.0]);
  }

  #[test]
  fn sprite_instance_with_transform() {
    let instance = SpriteInstance::new().with_transform(100.0, 200.0, 0.0, 2.0, 3.0);
    // Column-major layout: translation is in column 2
    assert_eq!(instance.transform[2][0], 100.0); // x translation
    assert_eq!(instance.transform[2][1], 200.0); // y translation
    assert_eq!(instance.transform[0][0], 2.0); // scale_x (column 0, row 0)
    assert_eq!(instance.transform[1][1], 3.0); // scale_y (column 1, row 1)
  }

  #[test]
  fn sprite_instance_with_effect_flash() {
    let effect = SpriteEffect::flash(Color::RED, 0.8);
    let instance = SpriteInstance::new().with_effect(&effect);
    assert_eq!(instance.effect_params[0], 3.0); // Flash effect type
    assert_eq!(instance.effect_params[1], 0.8); // intensity
  }

  #[test]
  fn directional_outline_packs_both_colours() {
    let effect =
      SpriteEffect::directional_outline(Color::new(1, 2, 3, 4), Color::new(120, 121, 130, 255), 2);
    let instance = SpriteInstance::new().with_effect(&effect);
    assert_eq!(
      instance.effect_params,
      [11.0, 2.0, 120.0 * 256.0 + 121.0, 130.0 * 256.0 + 255.0]
    );
    assert_eq!(
      instance.effect_extra,
      [1.0 / 255.0, 2.0 / 255.0, 3.0 / 255.0, 4.0 / 255.0]
    );
  }

  // =========================================================================
  // Atlas placement tests
  // =========================================================================

  const PAGE: (u32, u32) = (2048, 2048);

  fn solid(images: &mut Assets<Image>) -> Handle<Image> {
    images.add(Image::solid_rect(2, 2, 255, 255, 255, 255))
  }

  fn texels(v: [f32; 4]) -> [f32; 4] {
    v.map(|c| c * 2048.0)
  }

  /// A 4x4 image at texel (8, 8) of a 2048 page.
  fn slot_4x4() -> AtlasSlot {
    AtlasSlot {
      page: SlotPage::Atlas(0),
      uv: [8.0, 8.0, 12.0, 12.0].map(|c| c / 2048.0),
    }
  }

  #[test]
  fn uvs_are_remapped_into_the_slot() {
    let sub = SpriteInstance::new().with_uv(0.5, 0.0, 1.0, 0.5);
    let placed = place_in_slot(&sub, &slot_4x4(), PAGE, false);
    assert_eq!(texels(placed.uv_rect), [10.0, 8.0, 12.0, 10.0]);
    assert_eq!(placed.uv_bounds, placed.uv_rect);
    assert_eq!(placed.transform, sub.transform);
  }

  #[test]
  fn flipped_uvs_keep_ordered_bounds() {
    let flipped = SpriteInstance::new().with_uv(1.0, 0.0, 0.0, 1.0);
    let placed = place_in_slot(&flipped, &slot_4x4(), PAGE, false);
    assert_eq!(texels(placed.uv_rect), [12.0, 8.0, 8.0, 12.0]);
    assert_eq!(texels(placed.uv_bounds), [8.0, 8.0, 12.0, 12.0]);
  }

  #[test]
  fn bounds_are_clipped_to_the_slot() {
    let beyond = SpriteInstance::new().with_uv(-1.0, 0.0, 2.0, 1.0);
    let placed = place_in_slot(&beyond, &slot_4x4(), PAGE, false);
    assert_eq!(texels(placed.uv_bounds), [8.0, 8.0, 12.0, 12.0]);
  }

  #[test]
  fn growing_effects_enlarge_quad_and_uvs_around_the_centre() {
    let outlined = SpriteInstance::new()
      .with_transform(32.0, 32.0, 0.0, 16.0, 16.0)
      .with_effect(&SpriteEffect::outline(Color::WHITE, 2));
    let placed = place_in_slot(&outlined, &slot_4x4(), PAGE, true);
    // 4 texels + 2 on each side = 8 texels, 4 px each
    assert_eq!(placed.transform[0][0], 32.0);
    assert_eq!(placed.transform[1][1], 32.0);
    assert_eq!(placed.transform[2], outlined.transform[2]);
    assert_eq!(texels(placed.uv_rect), [6.0, 6.0, 14.0, 14.0]);
    assert_eq!(texels(placed.uv_bounds), [8.0, 8.0, 12.0, 12.0]);
  }

  #[test]
  fn growth_follows_flipped_uvs() {
    let outlined = SpriteInstance::new()
      .with_uv(1.0, 0.0, 0.0, 1.0)
      .with_effect(&SpriteEffect::outline(Color::WHITE, 1));
    let placed = place_in_slot(&outlined, &slot_4x4(), PAGE, true);
    assert_eq!(texels(placed.uv_rect), [13.0, 7.0, 7.0, 13.0]);
  }

  #[test]
  fn nothing_grows_without_the_effect_pipeline_or_a_growing_effect() {
    let outlined = SpriteInstance::new().with_effect(&SpriteEffect::outline(Color::WHITE, 2));
    let base = place_in_slot(&outlined, &slot_4x4(), PAGE, false);
    assert_eq!(texels(base.uv_rect), [8.0, 8.0, 12.0, 12.0]);
    let flashed = SpriteInstance::new().with_effect(&SpriteEffect::flash(Color::WHITE, 1.0));
    let flash = place_in_slot(&flashed, &slot_4x4(), PAGE, true);
    assert_eq!(flash.transform, flashed.transform);
  }

  #[test]
  fn effect_padding_matches_the_cpu_growth() {
    let pad =
      |e: SpriteEffect| effect_padding(&SpriteInstance::new().with_effect(&e).effect_params);
    assert_eq!(pad(SpriteEffect::outline(Color::WHITE, 3)), 3.0);
    let directional = SpriteEffect::directional_outline(Color::WHITE, Color::BLACK, 2);
    assert_eq!(pad(directional), 2.0);
    // The CPU grows a glow by `radius as usize`, at least 1
    assert_eq!(
      pad(SpriteEffect::glow_with_intensity(Color::WHITE, 2.7, 1.0)),
      2.0
    );
    assert_eq!(pad(SpriteEffect::glow(Color::WHITE, 1.0)), 1.0);
    assert_eq!(pad(SpriteEffect::invert()), 0.0);
  }

  fn world_list(
    renderer: &mut SpriteGpuRenderer,
    images: &Assets<Image>,
    entries: &[(&Handle<Image>, i32)],
  ) -> DrawList {
    let mut collector = BatchCollector::new();
    for (image, z) in entries {
      let draw = SpriteDraw {
        z: *z,
        ..Default::default()
      };
      collector.add(image, SpriteInstance::new(), draw);
    }
    let mut list = DrawList::default();
    collector.push_to(&mut list, DrawKind::Sprite, renderer, images);
    list.sorted();
    list
  }

  #[test]
  fn the_standalone_cap_looks_through_nothing_while_it_is_not_reached() {
    let Ok((device, queue)) = crate::core::testing::shared_gpu() else {
      return;
    };
    let mut renderer = SpriteGpuRenderer::new(&device, &queue);
    let mut images = Assets::<Image>::new();
    // More textures than the cap, all in the atlas's pages
    let small: Vec<_> = (0..MAX_STANDALONE_ENTITY_SPRITES + 6)
      .map(|_| solid(&mut images))
      .collect();
    let entries: Vec<_> = small.iter().map(|image| (image, 0)).collect();
    for _ in 0..3 {
      let list = world_list(&mut renderer, &images, &entries);
      renderer.prepare_textures(&device, &queue, &list, &[]);
    }
    assert!(renderer.registry.len() > MAX_STANDALONE_ENTITY_SPRITES);
    assert_eq!(renderer.standalone_scans, 0);
  }

  #[test]
  fn prepared_textures_reupload_when_evicted() {
    let Ok((device, queue)) = crate::core::testing::shared_gpu() else {
      return;
    };
    let mut renderer = SpriteGpuRenderer::new(&device, &queue);
    let mut images = Assets::<Image>::new();
    let a = solid(&mut images);
    let list = world_list(&mut renderer, &images, &[(&a, 0)]);
    renderer.prepare_textures(&device, &queue, &list, &[]);
    assert!(renderer.lookup_image(a.id()).is_some());
    // As if the atlas had evicted it
    renderer.atlas.clear();
    assert!(renderer.lookup_image(a.id()).is_none());

    let list = world_list(&mut renderer, &images, &[(&a, 0)]);
    renderer.prepare_textures(&device, &queue, &list, &[]);
    assert!(renderer.lookup_image(a.id()).is_some());
  }

  #[test]
  fn mismatched_texture_data_is_not_cached() {
    let Ok((device, queue)) = crate::core::testing::shared_gpu() else {
      return;
    };
    let mut renderer = SpriteGpuRenderer::new(&device, &queue);
    renderer.upload_texture(&device, &queue, "short", 2, 2, &[255; 15]);
    assert!(renderer.named_texture("short").is_none());
  }

  #[test]
  fn named_textures_neither_uploaded_nor_drawn_are_dropped_and_images_stay() {
    let Ok((device, queue)) = crate::core::testing::shared_gpu() else {
      return;
    };
    let mut renderer = SpriteGpuRenderer::new(&device, &queue);
    let mut images = Assets::<Image>::new();
    renderer.upload_texture(&device, &queue, "gone", 2, 2, &[255; 16]);
    let drawn = solid(&mut images);
    let undrawn = solid(&mut images);
    let list = world_list(&mut renderer, &images, &[(&undrawn, 0)]);
    renderer.prepare_textures(&device, &queue, &list, &[]);
    for _ in 0..=TEXTURE_TTL {
      // The engine uploads what it draws each frame (a no-op once cached)
      renderer.upload_texture(&device, &queue, "uploaded", 2, 2, &[255; 16]);
      let list = world_list(&mut renderer, &images, &[(&drawn, 0)]);
      renderer.prepare_textures(&device, &queue, &list, &[]);
    }
    assert!(renderer.named_texture("gone").is_none());
    assert!(renderer.lookup("gone").is_none(), "its atlas rect is freed");
    assert!(
      renderer.registry.image_texture(undrawn.id()).is_some(),
      "an image's texture lives while its asset does"
    );
    assert!(renderer.named_texture("uploaded").is_some());
    assert!(renderer.lookup_image(drawn.id()).is_some());
  }

  #[test]
  fn a_freed_image_loses_its_texture_at_once() {
    let Ok((device, queue)) = crate::core::testing::shared_gpu() else {
      return;
    };
    let mut renderer = SpriteGpuRenderer::new(&device, &queue);
    let mut images = Assets::<Image>::new();
    let kept = solid(&mut images);
    let freed = solid(&mut images);
    let list = world_list(&mut renderer, &images, &[(&kept, 0), (&freed, 0)]);
    renderer.prepare_textures(&device, &queue, &list, &[]);
    let id = freed.id();
    drop(freed);
    let mut removed = Vec::new();
    images.free_unheld(&mut removed);
    assert_eq!(removed, vec![(id, true)]);
    renderer.sync_images(&device, [id], [], &images, None);
    assert!(renderer.lookup_image(id).is_none());
    assert_eq!(renderer.texture_count(), 1);
    assert!(renderer.lookup_image(kept.id()).is_some());
  }

  #[test]
  fn an_image_freed_unseen_goes_at_the_next_check() {
    let Ok((device, queue)) = crate::core::testing::shared_gpu() else {
      return;
    };
    let mut renderer = SpriteGpuRenderer::new(&device, &queue);
    let mut images = Assets::<Image>::new();
    let freed = solid(&mut images);
    let list = world_list(&mut renderer, &images, &[(&freed, 0)]);
    renderer.prepare_textures(&device, &queue, &list, &[]);
    let id = freed.id();
    drop(freed);
    images.free_unheld(&mut Vec::new());
    // Its `Removed` was never read; the store no longer has it
    while !renderer.frame.is_multiple_of(PRUNE_EVERY) {
      renderer.sync_images(&device, [], [], &images, None);
      assert!(renderer.lookup_image(id).is_some(), "kept until the check");
      renderer.prepare_textures(&device, &queue, &DrawList::default(), &[]);
    }
    renderer.sync_images(&device, [], [], &images, None);
    assert!(renderer.lookup_image(id).is_none());
  }

  /// A 300 px image, too large for the atlas's pages, its upload made as
  /// a reload's is
  fn reloaded(shade: u8) -> Image {
    let mut image = Image::solid_rect(300, 300, shade, 0, 0, 255);
    image.prepare_upload();
    image
  }

  #[test]
  fn a_reload_of_an_image_never_drawn_keeps_no_bordered_copy() {
    let Ok((device, queue)) = crate::core::testing::shared_gpu() else {
      return;
    };
    let mut renderer = SpriteGpuRenderer::new(&device, &queue);
    let mut images = Assets::<Image>::new();
    let handle = images.add(reloaded(10));
    *images.get_mut(&handle).unwrap() = reloaded(20);
    renderer.sync_images(&device, [], [handle.id()], &images, None);
    let image = images.get(&handle).unwrap();
    assert!(
      image.take_upload().is_none(),
      "dropped: a first draw borders it"
    );
    assert!(image.mips().is_some(), "its levels stay");
  }

  #[test]
  fn a_reload_of_an_image_whose_texture_is_gone_keeps_no_bordered_copy() {
    let Ok((device, queue)) = crate::core::testing::shared_gpu() else {
      return;
    };
    let mut renderer = SpriteGpuRenderer::new(&device, &queue);
    let mut images = Assets::<Image>::new();
    let handle = images.add(reloaded(10));
    let list = world_list(&mut renderer, &images, &[(&handle, 0)]);
    renderer.prepare_textures(&device, &queue, &list, &[]);
    let texture = renderer.registry.image_texture(handle.id()).unwrap();
    // As if its texture had gone
    renderer.atlas.remove(texture);
    *images.get_mut(&handle).unwrap() = reloaded(20);
    renderer.sync_images(&device, [], [handle.id()], &images, None);
    assert!(
      !renderer.upload_pending(handle.id()),
      "nothing to draw meanwhile"
    );
    assert!(
      !renderer.holds_prepared(handle.id()),
      "and no bordered copy kept until it is drawn"
    );
  }

  #[test]
  fn a_reload_within_the_budget_keeps_its_upload_until_it_is_written() {
    let Ok((device, queue)) = crate::core::testing::shared_gpu() else {
      return;
    };
    let mut renderer = SpriteGpuRenderer::new(&device, &queue);
    let mut images = Assets::<Image>::new();
    // 300 px: a texture of its own, its levels well under a frame's budget
    let handle = images.add(reloaded(10));
    let list = world_list(&mut renderer, &images, &[(&handle, 0)]);
    renderer.prepare_textures(&device, &queue, &list, &[]);
    *images.get_mut(&handle).unwrap() = reloaded(20);
    renderer.sync_images(&device, [], [handle.id()], &images, None);
    assert!(!renderer.upload_pending(handle.id()), "written at once");
    assert!(
      renderer.holds_prepared(handle.id()),
      "with what its loader made, at its next draw"
    );
    let list = world_list(&mut renderer, &images, &[(&handle, 0)]);
    renderer.prepare_textures(&device, &queue, &list, &[]);
    assert!(!renderer.holds_prepared(handle.id()), "freed once written");
  }

  #[test]
  fn two_large_reloads_landing_together_make_one_new_texture_a_frame() {
    let Ok((device, queue)) = crate::core::testing::shared_gpu() else {
      return;
    };
    let mut renderer = SpriteGpuRenderer::new(&device, &queue);
    // 51 of a 316 px texture's 1,264-byte rows, and 1,072 bytes over
    renderer.set_upload_budget(64 * 1024);
    let mut images = Assets::<Image>::new();
    let (a, b) = (images.add(reloaded(10)), images.add(reloaded(20)));
    let list = world_list(&mut renderer, &images, &[(&a, 0), (&b, 0)]);
    renderer.prepare_textures(&device, &queue, &list, &[]);
    *images.get_mut(&a).unwrap() = reloaded(30);
    *images.get_mut(&b).unwrap() = reloaded(40);
    renderer.sync_images(&device, [], [a.id(), b.id()], &images, None);
    assert_eq!(
      renderer.atlas.spread_count(),
      2,
      "both are written over frames"
    );
    let list = world_list(&mut renderer, &images, &[(&a, 0), (&b, 0)]);
    renderer.prepare_textures(&device, &queue, &list, &[]);
    assert_eq!(
      renderer.atlas.spread_textures(),
      1,
      "the second waits for room for a row before it makes its texture"
    );
  }

  #[test]
  fn shading_is_copy_so_sprites_carry_no_heap_data() {
    fn assert_copy<T: Copy>() {}
    assert_copy::<SpriteShading>();
    assert!(std::mem::size_of::<SpriteShading>() <= 16);
  }

  #[test]
  fn packing_correct_names_reports_nothing() {
    let src = "struct Params { a: f32 }\nfn sprite(in: SpriteIn, params: Params) -> vec4<f32> { return vec4<f32>(params.a); }";
    let layout = custom::compile(ShaderKind::Sprite, "pack.wgsl", src)
      .map(|c| c.layout)
      .unwrap_or_else(|e| panic!("{e}"));
    let mut bytes = [0u8; 32];
    let mut problems = 0;
    let params = ShaderParams::new().with("a", 2.0);
    custom::pack_with(&layout, &params, &mut bytes, |_, _| problems += 1);
    assert_eq!(problems, 0);
    assert_eq!(&bytes[..4], &2.0f32.to_le_bytes());

    let params = params.with("typo", 1.0);
    custom::pack_with(&layout, &params, &mut bytes, |_, _| problems += 1);
    assert_eq!(problems, 1);
  }

  // =========================================================================
  // Collector tests
  // =========================================================================

  #[test]
  fn collector_keeps_submission_order_and_counts() {
    let mut images = Assets::<Image>::new();
    let (c, a) = (solid(&mut images), solid(&mut images));
    let mut collector = BatchCollector::new();
    let at = |z| SpriteDraw {
      z,
      ..Default::default()
    };
    collector.add(&c, SpriteInstance::new(), at(100));
    let effects = SpriteDraw {
      effects: true,
      ..Default::default()
    };
    collector.add(&a, SpriteInstance::new(), effects);
    collector.add(&a.clone(), SpriteInstance::new(), at(50));
    assert_eq!(collector.sprite_count(), 3);

    // One entry per image, shared by its sprites
    assert_eq!(collector.images.len(), 2);
    assert_eq!(collector.items[1].image, collector.items[2].image);
    collector.clear();
    assert_eq!(collector.sprite_count(), 0);
    assert!(
      collector.images.is_empty(),
      "nothing is kept past the frame"
    );
  }
}
