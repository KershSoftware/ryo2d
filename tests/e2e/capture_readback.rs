//! E2E tests for frame capture GPU readback.

use crate::common::gpu::TestGpu;
use ryo2d::core::capture::read_texture_rgba;
use ryo2d::core::error::CaptureError;

fn upload_texture(
  gpu: &TestGpu,
  format: wgpu::TextureFormat,
  bytes_per_pixel: u32,
  width: u32,
  height: u32,
  bytes: &[u8],
) -> wgpu::Texture {
  let size = wgpu::Extent3d {
    width,
    height,
    depth_or_array_layers: 1,
  };
  let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
    label: Some("Capture Test Texture"),
    size,
    mip_level_count: 1,
    sample_count: 1,
    dimension: wgpu::TextureDimension::D2,
    format,
    usage: wgpu::TextureUsages::COPY_SRC | wgpu::TextureUsages::COPY_DST,
    view_formats: &[],
  });
  gpu.queue.write_texture(
    wgpu::ImageCopyTexture {
      texture: &texture,
      mip_level: 0,
      origin: wgpu::Origin3d::ZERO,
      aspect: wgpu::TextureAspect::All,
    },
    bytes,
    wgpu::ImageDataLayout {
      offset: 0,
      bytes_per_row: Some(width * bytes_per_pixel),
      rows_per_image: Some(height),
    },
    size,
  );
  texture
}

/// Each pixel's channels encode its position so row/column mix-ups show up.
fn pattern(width: u32, height: u32) -> Vec<u8> {
  let mut bytes = Vec::with_capacity((width * height * 4) as usize);
  for y in 0..height {
    for x in 0..width {
      bytes.extend_from_slice(&[x as u8, y as u8, (x + y) as u8, 255]);
    }
  }
  bytes
}

#[test]
fn readback_rgba_texture_with_row_padding() {
  let Some(gpu) = TestGpu::shared() else {
    eprintln!("Skipping readback_rgba_texture_with_row_padding: no GPU available");
    return;
  };
  // 37 px wide = 148 bytes per row, padded to 256 for the copy
  let (width, height) = (37, 5);
  let bytes = pattern(width, height);
  let texture = upload_texture(
    gpu,
    wgpu::TextureFormat::Rgba8UnormSrgb,
    4,
    width,
    height,
    &bytes,
  );

  let image = read_texture_rgba(&gpu.device, &gpu.queue, &texture).unwrap();

  assert_eq!(image.dimensions(), (width, height));
  assert_eq!(image.into_raw(), bytes);
}

#[test]
fn readback_bgra_texture_is_swizzled_to_rgba() {
  let Some(gpu) = TestGpu::shared() else {
    eprintln!("Skipping readback_bgra_texture_is_swizzled_to_rgba: no GPU available");
    return;
  };
  let (width, height) = (37, 5);
  let bgra = pattern(width, height);
  let texture = upload_texture(
    gpu,
    wgpu::TextureFormat::Bgra8UnormSrgb,
    4,
    width,
    height,
    &bgra,
  );

  let image = read_texture_rgba(&gpu.device, &gpu.queue, &texture).unwrap();

  let expected: Vec<u8> = bgra
    .chunks_exact(4)
    .flat_map(|px| [px[2], px[1], px[0], px[3]])
    .collect();
  assert_eq!(image.into_raw(), expected);
}

#[test]
fn readback_rejects_unsupported_format() {
  let Some(gpu) = TestGpu::shared() else {
    eprintln!("Skipping readback_rejects_unsupported_format: no GPU available");
    return;
  };
  let texture = upload_texture(gpu, wgpu::TextureFormat::R8Unorm, 1, 4, 4, &[0u8; 16]);

  let result = read_texture_rgba(&gpu.device, &gpu.queue, &texture);

  assert!(matches!(
    result,
    Err(CaptureError::UnsupportedFormat(
      wgpu::TextureFormat::R8Unorm
    ))
  ));
}
