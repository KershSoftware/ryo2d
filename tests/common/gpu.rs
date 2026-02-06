//! Headless GPU context for e2e testing.
//!
//! Provides GPU initialization without requiring a window, enabling
//! automated testing of rendering and GPU-based systems.

use std::sync::{Arc, OnceLock};

/// Headless GPU context for e2e testing.
///
/// Creates a wgpu Device/Queue without requiring a window surface,
/// enabling GPU-based testing in CI environments.
///
/// # Example
///
/// ```ignore
/// let gpu = TestGpu::shared()?;
/// let target = gpu.create_target(100, 100);
/// // ... render to target ...
/// let pixels = pollster::block_on(target.read_pixels(&gpu));
/// ```
pub struct TestGpu {
  pub device: Arc<wgpu::Device>,
  pub queue: Arc<wgpu::Queue>,
  pub format: wgpu::TextureFormat,
}

/// Shared GPU instance to avoid concurrent initialization deadlocks on Windows.
static SHARED_GPU: OnceLock<Option<TestGpu>> = OnceLock::new();

impl TestGpu {
  /// Get a shared headless GPU context.
  ///
  /// Initializes the GPU once and reuses it across all tests, avoiding
  /// concurrent GPU initialization which deadlocks on Windows.
  ///
  /// Returns `None` if no GPU adapter is available (e.g., in CI without GPU).
  pub fn shared() -> Option<&'static Self> {
    SHARED_GPU.get_or_init(|| Self::new()).as_ref()
  }

  /// Create a headless GPU context.
  ///
  /// Returns `None` if no GPU adapter is available (e.g., in CI without GPU).
  /// Prefer `shared()` in tests to avoid concurrent initialization deadlocks.
  fn new() -> Option<Self> {
    pollster::block_on(Self::new_async())
  }

  async fn new_async() -> Option<Self> {
    // Create wgpu instance
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
      backends: wgpu::Backends::all(),
      ..Default::default()
    });

    // Request adapter WITHOUT a surface - this is the key for headless rendering
    let adapter = instance
      .request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::default(),
        compatible_surface: None, // No window needed!
        force_fallback_adapter: false,
      })
      .await?;

    // Request device and queue
    let (device, queue) = adapter
      .request_device(
        &wgpu::DeviceDescriptor {
          label: Some("TestGpu Device"),
          required_features: wgpu::Features::empty(),
          required_limits: wgpu::Limits::default(),
          memory_hints: Default::default(),
        },
        None,
      )
      .await
      .ok()?;

    Some(Self {
      device: Arc::new(device),
      queue: Arc::new(queue),
      format: wgpu::TextureFormat::Rgba8Unorm,
    })
  }

  /// Create a render target texture that can be rendered to and read back.
  pub fn create_target(&self, width: u32, height: u32) -> TestRenderTarget {
    TestRenderTarget::new(&self.device, width, height, self.format)
  }
}

/// A texture that can be rendered to and read back to CPU.
///
/// Used for verifying GPU rendering output in tests.
pub struct TestRenderTarget {
  pub texture: wgpu::Texture,
  pub view: wgpu::TextureView,
  staging_buffer: wgpu::Buffer,
  width: u32,
  height: u32,
  bytes_per_row: u32,
  padded_bytes_per_row: u32,
}

impl TestRenderTarget {
  fn new(device: &wgpu::Device, width: u32, height: u32, format: wgpu::TextureFormat) -> Self {
    // Create render target texture
    let texture = device.create_texture(&wgpu::TextureDescriptor {
      label: Some("Test Render Target"),
      size: wgpu::Extent3d {
        width,
        height,
        depth_or_array_layers: 1,
      },
      mip_level_count: 1,
      sample_count: 1,
      dimension: wgpu::TextureDimension::D2,
      format,
      usage: wgpu::TextureUsages::RENDER_ATTACHMENT
        | wgpu::TextureUsages::COPY_SRC
        | wgpu::TextureUsages::COPY_DST,
      view_formats: &[],
    });

    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());

    // Calculate buffer size with proper alignment
    // wgpu requires COPY_BYTES_PER_ROW_ALIGNMENT (256) alignment for buffer copies
    let bytes_per_pixel = 4u32; // RGBA8
    let bytes_per_row = width * bytes_per_pixel;
    let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let padded_bytes_per_row = bytes_per_row.div_ceil(align) * align;
    let buffer_size = (padded_bytes_per_row * height) as u64;

    // Create staging buffer for readback
    let staging_buffer = device.create_buffer(&wgpu::BufferDescriptor {
      label: Some("Test Staging Buffer"),
      size: buffer_size,
      usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
      mapped_at_creation: false,
    });

    Self {
      texture,
      view,
      staging_buffer,
      width,
      height,
      bytes_per_row,
      padded_bytes_per_row,
    }
  }

  /// Get the width of the render target.
  pub fn width(&self) -> u32 {
    self.width
  }

  /// Get the height of the render target.
  pub fn height(&self) -> u32 {
    self.height
  }

  /// Read rendered pixels back to CPU.
  ///
  /// Returns RGBA pixel data as a flat Vec<u8> with no row padding.
  /// Pixel at (x, y) is at index `(y * width + x) * 4`.
  pub async fn read_pixels(&self, gpu: &TestGpu) -> Vec<u8> {
    // Create command encoder for the copy
    let mut encoder = gpu
      .device
      .create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("Pixel Readback Encoder"),
      });

    // Copy texture to staging buffer
    encoder.copy_texture_to_buffer(
      wgpu::ImageCopyTexture {
        texture: &self.texture,
        mip_level: 0,
        origin: wgpu::Origin3d::ZERO,
        aspect: wgpu::TextureAspect::All,
      },
      wgpu::ImageCopyBuffer {
        buffer: &self.staging_buffer,
        layout: wgpu::ImageDataLayout {
          offset: 0,
          bytes_per_row: Some(self.padded_bytes_per_row),
          rows_per_image: Some(self.height),
        },
      },
      wgpu::Extent3d {
        width: self.width,
        height: self.height,
        depth_or_array_layers: 1,
      },
    );

    // Submit the copy command
    gpu.queue.submit(std::iter::once(encoder.finish()));

    // Map the buffer for reading
    let buffer_slice = self.staging_buffer.slice(..);
    let (tx, rx) = std::sync::mpsc::channel();
    buffer_slice.map_async(wgpu::MapMode::Read, move |result| {
      tx.send(result).unwrap();
    });

    // Wait for the GPU to finish
    gpu.device.poll(wgpu::Maintain::Wait);
    rx.recv().unwrap().expect("Failed to map buffer");

    // Read the data, removing row padding
    let mapped = buffer_slice.get_mapped_range();
    let mut pixels = Vec::with_capacity((self.width * self.height * 4) as usize);

    for y in 0..self.height {
      let start = (y * self.padded_bytes_per_row) as usize;
      let end = start + self.bytes_per_row as usize;
      pixels.extend_from_slice(&mapped[start..end]);
    }

    drop(mapped);
    self.staging_buffer.unmap();

    pixels
  }

  /// Read pixels synchronously using pollster.
  pub fn read_pixels_sync(&self, gpu: &TestGpu) -> Vec<u8> {
    pollster::block_on(self.read_pixels(gpu))
  }

  /// Get pixel color at (x, y).
  ///
  /// Returns (r, g, b, a) tuple with 8-bit components.
  pub fn get_pixel(&self, gpu: &TestGpu, x: u32, y: u32) -> (u8, u8, u8, u8) {
    let pixels = self.read_pixels_sync(gpu);
    let idx = ((y * self.width + x) * 4) as usize;
    (
      pixels[idx],
      pixels[idx + 1],
      pixels[idx + 2],
      pixels[idx + 3],
    )
  }

  /// Clear the render target to a solid color.
  pub fn clear(&self, gpu: &TestGpu, r: f64, g: f64, b: f64, a: f64) {
    let mut encoder = gpu
      .device
      .create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("Clear Encoder"),
      });

    {
      let _render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("Clear Pass"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
          view: &self.view,
          resolve_target: None,
          ops: wgpu::Operations {
            load: wgpu::LoadOp::Clear(wgpu::Color { r, g, b, a }),
            store: wgpu::StoreOp::Store,
          },
        })],
        depth_stencil_attachment: None,
        occlusion_query_set: None,
        timestamp_writes: None,
      });
    }

    gpu.queue.submit(std::iter::once(encoder.finish()));
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn test_gpu_creation() {
    // This test verifies the basic structure works
    // It may return None in CI without GPU, which is fine
    let _gpu = TestGpu::shared();
  }
}
