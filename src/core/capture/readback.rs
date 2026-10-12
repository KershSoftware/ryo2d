//! GPU texture readback into tightly packed RGBA8 images.

use image::RgbaImage;

use crate::core::error::{CaptureError, CaptureResult};

/// Bytes per row padded to wgpu's copy alignment (256).
pub fn padded_bytes_per_row(width: u32) -> u32 {
  let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
  (width * 4).div_ceil(align) * align
}

/// Remove per-row padding from a buffer laid out with `padded_row` bytes per row.
pub fn strip_row_padding(data: &[u8], width: u32, height: u32, padded_row: u32) -> Vec<u8> {
  let row = (width * 4) as usize;
  let padded_row = padded_row as usize;
  let mut out = Vec::with_capacity(row * height as usize);
  for y in 0..height as usize {
    let start = y * padded_row;
    out.extend_from_slice(&data[start..start + row]);
  }
  out
}

/// Swap the red and blue channels of BGRA8 pixels in place.
pub fn bgra_to_rgba_in_place(pixels: &mut [u8]) {
  for px in pixels.as_chunks_mut::<4>().0 {
    px.swap(0, 2);
  }
}

/// Read a texture back to the CPU as an RGBA8 image.
///
/// Blocks until the GPU has finished the copy. The texture needs `COPY_SRC`
/// usage and an 8-bit RGBA or BGRA format; BGRA is converted to RGBA.
pub fn read_texture_rgba(
  device: &wgpu::Device,
  queue: &wgpu::Queue,
  texture: &wgpu::Texture,
) -> CaptureResult<RgbaImage> {
  let swizzle = match texture.format() {
    wgpu::TextureFormat::Rgba8Unorm | wgpu::TextureFormat::Rgba8UnormSrgb => false,
    wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Bgra8UnormSrgb => true,
    other => return Err(CaptureError::UnsupportedFormat(other)),
  };

  let width = texture.width();
  let height = texture.height();
  let padded_row = padded_bytes_per_row(width);

  let buffer = device.create_buffer(&wgpu::BufferDescriptor {
    label: Some("Capture Readback Buffer"),
    size: u64::from(padded_row) * u64::from(height),
    usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
    mapped_at_creation: false,
  });

  let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
    label: Some("Capture Readback Encoder"),
  });
  encoder.copy_texture_to_buffer(
    wgpu::ImageCopyTexture {
      texture,
      mip_level: 0,
      origin: wgpu::Origin3d::ZERO,
      aspect: wgpu::TextureAspect::All,
    },
    wgpu::ImageCopyBuffer {
      buffer: &buffer,
      layout: wgpu::ImageDataLayout {
        offset: 0,
        bytes_per_row: Some(padded_row),
        rows_per_image: Some(height),
      },
    },
    wgpu::Extent3d {
      width,
      height,
      depth_or_array_layers: 1,
    },
  );
  queue.submit(std::iter::once(encoder.finish()));

  let slice = buffer.slice(..);
  let (tx, rx) = std::sync::mpsc::channel();
  slice.map_async(wgpu::MapMode::Read, move |result| {
    let _ = tx.send(result);
  });
  device.poll(wgpu::Maintain::Wait);
  rx.recv()
    .map_err(|e| CaptureError::Readback(e.to_string()))?
    .map_err(|e| CaptureError::Readback(e.to_string()))?;

  let mut pixels = {
    let mapped = slice.get_mapped_range();
    strip_row_padding(&mapped, width, height, padded_row)
  };
  buffer.unmap();

  if swizzle {
    bgra_to_rgba_in_place(&mut pixels);
  }

  RgbaImage::from_raw(width, height, pixels)
    .ok_or_else(|| CaptureError::Readback("pixel buffer size mismatch".to_string()))
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn padded_row_rounds_up_to_256() {
    assert_eq!(padded_bytes_per_row(64), 256);
    assert_eq!(padded_bytes_per_row(37), 256);
    assert_eq!(padded_bytes_per_row(65), 512);
  }

  #[test]
  fn strip_row_padding_keeps_only_pixel_bytes() {
    // 2x2 image, rows padded to 12 bytes (8 pixel bytes + 4 padding)
    let data = [
      1, 2, 3, 4, 5, 6, 7, 8, 0xEE, 0xEE, 0xEE, 0xEE, //
      9, 10, 11, 12, 13, 14, 15, 16, 0xEE, 0xEE, 0xEE, 0xEE,
    ];
    let out = strip_row_padding(&data, 2, 2, 12);
    assert_eq!(out, (1..=16).collect::<Vec<u8>>());
  }

  #[test]
  fn bgra_to_rgba_swaps_red_and_blue() {
    let mut px = [10, 20, 30, 40, 1, 2, 3, 4];
    bgra_to_rgba_in_place(&mut px);
    assert_eq!(px, [30, 20, 10, 40, 3, 2, 1, 4]);
  }
}
