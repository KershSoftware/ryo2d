use std::collections::HashMap;
use std::path::{Path, PathBuf};

use super::atlas::slice_atlas;
use super::handle::{
  SlotMap, TextureArrayHandle, TextureArrayMarker, TextureHandle, TextureMarker,
};
use crate::core::ecs::Resource;
use crate::core::graphics::Gpu;

/// CPU + GPU data for an individual texture.
#[derive(Debug)]
pub(crate) struct ManagedTexture {
  #[allow(dead_code)] // RAII: dropping this frees the GPU allocation
  pub texture: wgpu::Texture,
  pub view: wgpu::TextureView,
  pub width: u32,
  pub height: u32,
  pub data: Vec<u8>, // CPU copy retained for array building
}

/// GPU data for a texture array.
#[derive(Debug)]
pub(crate) struct ManagedTextureArray {
  #[allow(dead_code)] // RAII: dropping this frees the GPU allocation
  pub texture: wgpu::Texture,
  pub view: wgpu::TextureView,
}

/// Centralized GPU content texture manager.
///
/// Owns all content GPU textures (not render targets). Provides loading,
/// atlas slicing, deduplication, and lifecycle management.
#[derive(Resource)]
pub struct TextureManager {
  textures: SlotMap<TextureMarker, ManagedTexture>,
  arrays: SlotMap<TextureArrayMarker, ManagedTextureArray>,
  path_cache: HashMap<PathBuf, TextureHandle>,
}

impl TextureManager {
  /// Create a new empty TextureManager.
  pub fn new() -> Self {
    Self {
      textures: SlotMap::new(),
      arrays: SlotMap::new(),
      path_cache: HashMap::new(),
    }
  }

  /// Create a GPU texture from raw RGBA8 bytes.
  /// Stores a CPU copy for potential array building later.
  ///
  /// # Panics
  /// Panics if `data.len() != (width * height * 4) as usize`.
  pub fn from_rgba(
    &mut self,
    data: Vec<u8>,
    width: u32,
    height: u32,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
  ) -> TextureHandle {
    assert_eq!(
      data.len(),
      (width as usize) * (height as usize) * 4,
      "RGBA data length {} does not match {}x{}x4 = {}",
      data.len(),
      width,
      height,
      (width as usize) * (height as usize) * 4,
    );

    let texture = device.create_texture(&wgpu::TextureDescriptor {
      label: Some("TextureManager Texture"),
      size: wgpu::Extent3d {
        width,
        height,
        depth_or_array_layers: 1,
      },
      mip_level_count: 1,
      sample_count: 1,
      dimension: wgpu::TextureDimension::D2,
      format: wgpu::TextureFormat::Rgba8UnormSrgb,
      usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
      view_formats: &[],
    });

    queue.write_texture(
      wgpu::ImageCopyTexture {
        texture: &texture,
        mip_level: 0,
        origin: wgpu::Origin3d::ZERO,
        aspect: wgpu::TextureAspect::All,
      },
      &data,
      wgpu::ImageDataLayout {
        offset: 0,
        bytes_per_row: Some(4 * width),
        rows_per_image: Some(height),
      },
      wgpu::Extent3d {
        width,
        height,
        depth_or_array_layers: 1,
      },
    );

    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());

    self.textures.insert(ManagedTexture {
      texture,
      view,
      width,
      height,
      data,
    })
  }

  /// Load an image from disk. Deduplicates by exact path (not canonicalized).
  ///
  /// Uses the `image` crate to decode PNG/JPEG to RGBA8.
  /// Note: `"assets/wall.png"` and `"./assets/wall.png"` are treated as
  /// different paths. Use consistent paths to benefit from deduplication.
  pub fn load(&mut self, path: &Path, device: &wgpu::Device, queue: &wgpu::Queue) -> TextureHandle {
    // Check dedup cache
    if let Some(&handle) = self.path_cache.get(path) {
      if self.textures.get(handle).is_some() {
        return handle;
      }
      // Handle was dropped, remove stale cache entry
      self.path_cache.remove(path);
    }

    // Load image via image crate
    let img = image::open(path).unwrap_or_else(|e| panic!("Failed to load texture: {path:?}: {e}"));
    let rgba = img.into_rgba8();
    let width = rgba.width();
    let height = rgba.height();
    let data = rgba.into_raw();

    let handle = self.from_rgba(data, width, height, device, queue);
    self.path_cache.insert(path.to_path_buf(), handle);
    handle
  }

  /// Load an atlas image from disk and slice into individual tile textures.
  /// Returns handles in row-major order.
  pub fn load_atlas(
    &mut self,
    path: &Path,
    tile_w: u32,
    tile_h: u32,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
  ) -> Vec<TextureHandle> {
    let img = image::open(path).unwrap_or_else(|e| panic!("Failed to load atlas: {path:?}: {e}"));
    let rgba = img.into_rgba8();
    let width = rgba.width();
    let height = rgba.height();
    let data = rgba.into_raw();

    let tiles = slice_atlas(&data, width, height, tile_w, tile_h);
    tiles
      .into_iter()
      .map(|tile_data| self.from_rgba(tile_data, tile_w, tile_h, device, queue))
      .collect()
  }

  /// Build a texture_2d_array from individual texture handles.
  /// All referenced textures must have the same dimensions.
  pub fn create_array(
    &mut self,
    handles: &[TextureHandle],
    device: &wgpu::Device,
    queue: &wgpu::Queue,
  ) -> TextureArrayHandle {
    assert!(
      !handles.is_empty(),
      "Cannot create array from zero textures"
    );

    let first = self
      .textures
      .get(handles[0])
      .expect("First texture handle is invalid");
    let width = first.width;
    let height = first.height;
    let layer_count = handles.len() as u32;

    // Verify all textures share the same dimensions
    for (i, &handle) in handles.iter().enumerate().skip(1) {
      let managed = self
        .textures
        .get(handle)
        .unwrap_or_else(|| panic!("Texture handle at index {i} is invalid"));
      assert_eq!(
        (managed.width, managed.height),
        (width, height),
        "Texture at index {i} has dimensions {}x{}, expected {width}x{height}",
        managed.width,
        managed.height,
      );
    }

    let texture = device.create_texture(&wgpu::TextureDescriptor {
      label: Some("TextureManager Array"),
      size: wgpu::Extent3d {
        width,
        height,
        depth_or_array_layers: layer_count,
      },
      mip_level_count: 1,
      sample_count: 1,
      dimension: wgpu::TextureDimension::D2,
      format: wgpu::TextureFormat::Rgba8UnormSrgb,
      usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
      view_formats: &[],
    });

    // Upload each layer
    for (i, &handle) in handles.iter().enumerate() {
      let managed = self
        .textures
        .get(handle)
        .expect("Texture handle became invalid during array creation");
      queue.write_texture(
        wgpu::ImageCopyTexture {
          texture: &texture,
          mip_level: 0,
          origin: wgpu::Origin3d {
            x: 0,
            y: 0,
            z: i as u32,
          },
          aspect: wgpu::TextureAspect::All,
        },
        &managed.data,
        wgpu::ImageDataLayout {
          offset: 0,
          bytes_per_row: Some(4 * width),
          rows_per_image: Some(height),
        },
        wgpu::Extent3d {
          width,
          height,
          depth_or_array_layers: 1,
        },
      );
    }

    let view = texture.create_view(&wgpu::TextureViewDescriptor {
      dimension: Some(wgpu::TextureViewDimension::D2Array),
      ..Default::default()
    });

    self.arrays.insert(ManagedTextureArray { texture, view })
  }

  /// Get the texture view for binding.
  pub fn get_view(&self, handle: TextureHandle) -> Option<&wgpu::TextureView> {
    self.textures.get(handle).map(|m| &m.view)
  }

  /// Get the texture array view for binding.
  pub fn get_array_view(&self, handle: TextureArrayHandle) -> Option<&wgpu::TextureView> {
    self.arrays.get(handle).map(|m| &m.view)
  }

  /// Get texture dimensions.
  pub fn dimensions(&self, handle: TextureHandle) -> Option<(u32, u32)> {
    self.textures.get(handle).map(|m| (m.width, m.height))
  }

  /// Remove an individual texture, freeing GPU memory.
  pub fn drop_texture(&mut self, handle: TextureHandle) {
    self.textures.remove(handle);
    // Clean up path_cache (remove any entry pointing to this handle)
    self.path_cache.retain(|_, &mut h| h != handle);
  }

  /// Remove a texture array, freeing GPU memory.
  pub fn drop_array(&mut self, handle: TextureArrayHandle) {
    self.arrays.remove(handle);
  }
}

impl Default for TextureManager {
  fn default() -> Self {
    Self::new()
  }
}

/// Convenience wrapper that bundles TextureManager with device/queue references.
///
/// Obtained via `ctx.textures()` in scene callbacks; it holds the `Gpu`
/// resource so texture calls need no device or queue.
pub struct TextureContext<'a> {
  manager: &'a mut TextureManager,
  gpu: Gpu,
}

impl<'a> TextureContext<'a> {
  /// Create a new TextureContext.
  pub fn new(manager: &'a mut TextureManager, gpu: Gpu) -> Self {
    Self { manager, gpu }
  }

  /// Load an image from disk. Deduplicates by path.
  pub fn load(&mut self, path: impl AsRef<std::path::Path>) -> TextureHandle {
    self
      .manager
      .load(path.as_ref(), &self.gpu.device, &self.gpu.queue)
  }

  /// Load an atlas image and slice into individual tile textures.
  pub fn load_atlas(
    &mut self,
    path: impl AsRef<std::path::Path>,
    tile_w: u32,
    tile_h: u32,
  ) -> Vec<TextureHandle> {
    self.manager.load_atlas(
      path.as_ref(),
      tile_w,
      tile_h,
      &self.gpu.device,
      &self.gpu.queue,
    )
  }

  /// Create from raw RGBA bytes.
  pub fn from_rgba(&mut self, data: Vec<u8>, width: u32, height: u32) -> TextureHandle {
    self
      .manager
      .from_rgba(data, width, height, &self.gpu.device, &self.gpu.queue)
  }

  /// Build a texture_2d_array from individual texture handles.
  pub fn create_array(&mut self, handles: &[TextureHandle]) -> TextureArrayHandle {
    self
      .manager
      .create_array(handles, &self.gpu.device, &self.gpu.queue)
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn test_new_manager_default() {
    let _mgr = TextureManager::default();
  }
}
