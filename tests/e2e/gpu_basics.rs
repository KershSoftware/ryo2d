//! Basic GPU context tests.
//!
//! These tests verify the headless GPU testing infrastructure works correctly.

use crate::common::gpu::TestGpu;

/// Test that we can create a headless GPU context.
///
/// This is the foundation for all GPU-based e2e tests.
/// Returns early if no GPU is available (CI without GPU).
#[test]
fn test_headless_gpu_creation() {
  let Some(gpu) = TestGpu::new() else {
    eprintln!("Skipping test_headless_gpu_creation: no GPU available");
    return;
  };

  // Verify we got valid handles
  assert!(
    gpu.format == wgpu::TextureFormat::Rgba8Unorm,
    "Expected Rgba8Unorm format"
  );

  // Device and queue should be usable (this would panic if invalid)
  let _encoder = gpu
    .device
    .create_command_encoder(&wgpu::CommandEncoderDescriptor {
      label: Some("Test Encoder"),
    });
}

/// Test that we can create render targets of various sizes.
#[test]
fn test_render_target_creation() {
  let Some(gpu) = TestGpu::new() else {
    eprintln!("Skipping test_render_target_creation: no GPU available");
    return;
  };

  // Test common sizes
  let sizes = [(100, 100), (320, 240), (1, 1), (512, 512)];

  for (width, height) in sizes {
    let target = gpu.create_target(width, height);
    assert_eq!(
      target.width(),
      width,
      "Width mismatch for {}x{}",
      width,
      height
    );
    assert_eq!(
      target.height(),
      height,
      "Height mismatch for {}x{}",
      width,
      height
    );
  }
}

/// Test that we can read pixels back from a cleared render target.
#[test]
fn test_pixel_readback() {
  let Some(gpu) = TestGpu::new() else {
    eprintln!("Skipping test_pixel_readback: no GPU available");
    return;
  };

  let target = gpu.create_target(100, 100);

  // Clear to red
  target.clear(&gpu, 1.0, 0.0, 0.0, 1.0);

  // Read pixels back
  let pixels = target.read_pixels_sync(&gpu);

  // Verify size (100 * 100 * 4 bytes = 40000)
  assert_eq!(pixels.len(), 100 * 100 * 4, "Unexpected pixel buffer size");

  // Verify center pixel is red
  let (r, g, b, a) = target.get_pixel(&gpu, 50, 50);
  assert_eq!(r, 255, "Red channel should be 255");
  assert_eq!(g, 0, "Green channel should be 0");
  assert_eq!(b, 0, "Blue channel should be 0");
  assert_eq!(a, 255, "Alpha channel should be 255");
}

/// Test that different clear colors produce correct pixel values.
#[test]
fn test_clear_colors() {
  let Some(gpu) = TestGpu::new() else {
    eprintln!("Skipping test_clear_colors: no GPU available");
    return;
  };

  let target = gpu.create_target(10, 10);

  // Helper to check pixel with tolerance
  fn check_pixel(actual: u8, expected: u8, channel: &str, color_name: &str) {
    let diff = (i32::from(actual) - i32::from(expected)).abs();
    assert!(
      diff <= 1,
      "{}: {} expected ~{}, got {}",
      color_name,
      channel,
      expected,
      actual
    );
  }

  // Test red
  target.clear(&gpu, 1.0, 0.0, 0.0, 1.0);
  let (r, g, b, a) = target.get_pixel(&gpu, 5, 5);
  check_pixel(r, 255, "red", "red");
  check_pixel(g, 0, "green", "red");
  check_pixel(b, 0, "blue", "red");
  check_pixel(a, 255, "alpha", "red");

  // Test green
  target.clear(&gpu, 0.0, 1.0, 0.0, 1.0);
  let (r, g, b, a) = target.get_pixel(&gpu, 5, 5);
  check_pixel(r, 0, "red", "green");
  check_pixel(g, 255, "green", "green");
  check_pixel(b, 0, "blue", "green");
  check_pixel(a, 255, "alpha", "green");

  // Test blue
  target.clear(&gpu, 0.0, 0.0, 1.0, 1.0);
  let (r, g, b, a) = target.get_pixel(&gpu, 5, 5);
  check_pixel(r, 0, "red", "blue");
  check_pixel(g, 0, "green", "blue");
  check_pixel(b, 255, "blue", "blue");
  check_pixel(a, 255, "alpha", "blue");

  // Test white
  target.clear(&gpu, 1.0, 1.0, 1.0, 1.0);
  let (r, g, b, a) = target.get_pixel(&gpu, 5, 5);
  check_pixel(r, 255, "red", "white");
  check_pixel(g, 255, "green", "white");
  check_pixel(b, 255, "blue", "white");
  check_pixel(a, 255, "alpha", "white");

  // Test black
  target.clear(&gpu, 0.0, 0.0, 0.0, 1.0);
  let (r, g, b, a) = target.get_pixel(&gpu, 5, 5);
  check_pixel(r, 0, "red", "black");
  check_pixel(g, 0, "green", "black");
  check_pixel(b, 0, "blue", "black");
  check_pixel(a, 255, "alpha", "black");

  // Test gray (may be 127 or 128 due to rounding)
  target.clear(&gpu, 0.5, 0.5, 0.5, 1.0);
  let (r, g, b, a) = target.get_pixel(&gpu, 5, 5);
  check_pixel(r, 128, "red", "gray");
  check_pixel(g, 128, "green", "gray");
  check_pixel(b, 128, "blue", "gray");
  check_pixel(a, 255, "alpha", "gray");
}

/// Test that corner pixels are correctly addressed.
#[test]
fn test_pixel_addressing() {
  let Some(gpu) = TestGpu::new() else {
    eprintln!("Skipping test_pixel_addressing: no GPU available");
    return;
  };

  let target = gpu.create_target(10, 10);

  // Clear to white
  target.clear(&gpu, 1.0, 1.0, 1.0, 1.0);

  // All corners should be white
  let corners = [(0, 0), (9, 0), (0, 9), (9, 9)];
  for (x, y) in corners {
    let (r, g, b, a) = target.get_pixel(&gpu, x, y);
    assert_eq!(
      (r, g, b, a),
      (255, 255, 255, 255),
      "Corner ({}, {}) should be white",
      x,
      y
    );
  }
}
