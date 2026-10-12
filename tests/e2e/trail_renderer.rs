//! GPU-based trail renderer tests.
//!
//! These tests verify the trail rendering system works correctly at the GPU level,
//! including texture operations, shader execution, and pixel output.
//!
//! Tests gracefully skip when no GPU is available (CI without GPU).

use crate::common::gpu::TestGpu;
use ryo2d::prelude::*;

// ============================================================================
// TRAIL RENDERER CREATION TESTS
// ============================================================================

/// Test that TrailRenderer can be created with headless GPU
#[test]
fn test_trail_renderer_creation() {
  let Some(gpu) = TestGpu::shared() else {
    eprintln!("Skipping test_trail_renderer_creation: no GPU available");
    return;
  };

  let format = wgpu::TextureFormat::Rgba8Unorm;
  let renderer = TrailRenderer::new(&gpu.device, 100, 100, format);

  // TrailRenderer starts enabled by default
  assert!(
    renderer.is_enabled(),
    "TrailRenderer should be enabled by default"
  );
}

/// Test that enabled state can be toggled
#[test]
fn test_trail_renderer_enable_disable() {
  let Some(gpu) = TestGpu::shared() else {
    eprintln!("Skipping test_trail_renderer_enable_disable: no GPU available");
    return;
  };

  let format = wgpu::TextureFormat::Rgba8Unorm;
  let mut renderer = TrailRenderer::new(&gpu.device, 100, 100, format);

  // Starts enabled by default
  assert!(renderer.is_enabled(), "Should start enabled by default");

  renderer.set_enabled(false);
  assert!(
    !renderer.is_enabled(),
    "Should be disabled after set_enabled(false)"
  );

  renderer.set_enabled(true);
  assert!(
    renderer.is_enabled(),
    "Should be enabled after set_enabled(true)"
  );
}

/// Test that TrailRenderer can be resized
#[test]
fn test_trail_renderer_resize() {
  let Some(gpu) = TestGpu::shared() else {
    eprintln!("Skipping test_trail_renderer_resize: no GPU available");
    return;
  };

  let format = wgpu::TextureFormat::Rgba8Unorm;
  let mut renderer = TrailRenderer::new(&gpu.device, 100, 100, format);

  // Resize to new dimensions
  renderer.resize(200, 150);

  // The renderer should accept the new size without panicking
  // (test passes if we reach here without panic)
}

// ============================================================================
// VERTEX ACCUMULATION TESTS
// ============================================================================

/// Test that begin_frame clears accumulated vertices
#[test]
fn test_begin_frame_clears_vertices() {
  let Some(gpu) = TestGpu::shared() else {
    eprintln!("Skipping test_begin_frame_clears_vertices: no GPU available");
    return;
  };

  let format = wgpu::TextureFormat::Rgba8Unorm;
  let mut renderer = TrailRenderer::new(&gpu.device, 100, 100, format);

  // Create a trail with points
  let mut trail = Trail2D::new(Color::WHITE, 5.0).with_min_distance(0.0);
  trail.update(0.0, 0.0, 0.016);
  trail.update(10.0, 10.0, 0.016);
  trail.update(20.0, 20.0, 0.016);

  // Add trail to renderer
  renderer.add_trail(&trail);
  assert!(
    renderer.vertex_count() > 0,
    "Should have vertices after adding trail"
  );

  // Begin new frame
  renderer.begin_frame();
  assert_eq!(
    renderer.vertex_count(),
    0,
    "Vertex count should be 0 after begin_frame"
  );
}

/// Test that invisible trails don't add vertices
#[test]
fn test_invisible_trail_no_vertices() {
  let Some(gpu) = TestGpu::shared() else {
    eprintln!("Skipping test_invisible_trail_no_vertices: no GPU available");
    return;
  };

  let format = wgpu::TextureFormat::Rgba8Unorm;
  let mut renderer = TrailRenderer::new(&gpu.device, 100, 100, format);

  // Create a trail and set it invisible
  let mut trail = Trail2D::new(Color::WHITE, 5.0).with_min_distance(0.0);
  trail.update(0.0, 0.0, 0.016);
  trail.update(10.0, 10.0, 0.016);
  trail.update(20.0, 20.0, 0.016);
  trail.set_visible(false);

  assert!(!trail.is_visible(), "Trail should be invisible");
  assert_eq!(trail.point_count(), 3, "Trail should have 3 points");

  // Add trail to renderer
  renderer.add_trail(&trail);

  assert_eq!(
    renderer.vertex_count(),
    0,
    "Invisible trail should not add vertices"
  );
}

/// Test that disabled trails (via set_enabled) don't add vertices
#[test]
fn test_disabled_trail_no_vertices() {
  let Some(gpu) = TestGpu::shared() else {
    eprintln!("Skipping test_disabled_trail_no_vertices: no GPU available");
    return;
  };

  let format = wgpu::TextureFormat::Rgba8Unorm;
  let mut renderer = TrailRenderer::new(&gpu.device, 100, 100, format);

  // Create a trail with points, then disable it
  let mut trail = Trail2D::new(Color::WHITE, 5.0).with_min_distance(0.0);
  trail.update(0.0, 0.0, 0.016);
  trail.update(10.0, 10.0, 0.016);
  trail.update(20.0, 20.0, 0.016);
  trail.set_enabled(false); // This also sets visible=false

  assert!(!trail.is_visible(), "Disabled trail should be invisible");

  // Add trail to renderer
  renderer.add_trail(&trail);

  assert_eq!(
    renderer.vertex_count(),
    0,
    "Disabled trail should not add vertices"
  );
}

/// Test that a single trail with sufficient points generates vertices
#[test]
fn test_single_trail_generates_vertices() {
  let Some(gpu) = TestGpu::shared() else {
    eprintln!("Skipping test_single_trail_generates_vertices: no GPU available");
    return;
  };

  let format = wgpu::TextureFormat::Rgba8Unorm;
  let mut renderer = TrailRenderer::new(&gpu.device, 100, 100, format);

  // Create a trail with 3 points (minimum for visible trail)
  let mut trail = Trail2D::new(Color::WHITE, 5.0).with_min_distance(0.0);
  trail.update(0.0, 0.0, 0.016);
  trail.update(10.0, 10.0, 0.016);
  trail.update(20.0, 20.0, 0.016);

  // Add trail to renderer
  renderer.add_trail(&trail);

  // 3 points should generate 6 vertices (2 per point for triangle strip)
  assert_eq!(
    renderer.vertex_count(),
    6,
    "3-point trail should generate 6 vertices"
  );
}

/// Test that multiple trails are properly separated by degenerate triangles
///
/// REGRESSION TEST: This verifies the fix for the "double tail" bug where
/// trails would connect to each other due to missing degenerate triangles.
#[test]
fn test_multiple_trails_degenerate_separation() {
  let Some(gpu) = TestGpu::shared() else {
    eprintln!("Skipping test_multiple_trails_degenerate_separation: no GPU available");
    return;
  };

  let format = wgpu::TextureFormat::Rgba8Unorm;
  let mut renderer = TrailRenderer::new(&gpu.device, 100, 100, format);

  // Create first trail with 3 points
  let mut trail1 = Trail2D::new(Color::WHITE, 5.0).with_min_distance(0.0);
  trail1.update(0.0, 0.0, 0.016);
  trail1.update(10.0, 0.0, 0.016);
  trail1.update(20.0, 0.0, 0.016);

  // Create second trail with 4 points
  let mut trail2 = Trail2D::new(Color::RED, 5.0).with_min_distance(0.0);
  trail2.update(50.0, 50.0, 0.016);
  trail2.update(60.0, 50.0, 0.016);
  trail2.update(70.0, 50.0, 0.016);
  trail2.update(80.0, 50.0, 0.016);

  // Add both trails
  renderer.add_trail(&trail1);
  renderer.add_trail(&trail2);

  // Expected vertex count:
  // Trail 1: 3 points * 2 = 6 vertices
  // Trail 2: 2 degenerate + 4 points * 2 = 10 vertices
  // Total: 16 vertices
  assert_eq!(
    renderer.vertex_count(),
    16,
    "Two trails (3 + 4 points) should generate 16 vertices (including degenerates)"
  );
}

/// Test that trails with insufficient points (<2) don't add vertices
#[test]
fn test_insufficient_points_no_vertices() {
  let Some(gpu) = TestGpu::shared() else {
    eprintln!("Skipping test_insufficient_points_no_vertices: no GPU available");
    return;
  };

  let format = wgpu::TextureFormat::Rgba8Unorm;
  let mut renderer = TrailRenderer::new(&gpu.device, 100, 100, format);

  // Create a trail with only 1 point
  let mut trail = Trail2D::new(Color::WHITE, 5.0).with_min_distance(0.0);
  trail.update(0.0, 0.0, 0.016);

  assert_eq!(trail.point_count(), 1, "Trail should have 1 point");

  // Add trail to renderer
  renderer.add_trail(&trail);

  assert_eq!(
    renderer.vertex_count(),
    0,
    "Single-point trail should not generate vertices"
  );
}

// ============================================================================
// RENDER TESTS
// ============================================================================

/// Test that render doesn't crash with empty vertex buffer
#[test]
fn test_render_empty_trails() {
  let Some(gpu) = TestGpu::shared() else {
    eprintln!("Skipping test_render_empty_trails: no GPU available");
    return;
  };

  let format = wgpu::TextureFormat::Rgba8Unorm;
  let renderer = TrailRenderer::new(&gpu.device, 100, 100, format);
  let target = gpu.create_target(100, 100);

  // Clear target first
  target.clear(gpu, 0.0, 0.0, 0.0, 1.0);

  let mut encoder = gpu
    .device
    .create_command_encoder(&wgpu::CommandEncoderDescriptor {
      label: Some("Empty Trail Render Test"),
    });

  // Render with no trails accumulated
  renderer.render(
    &gpu.queue,
    &mut encoder,
    &target.view,
    Vec2::ZERO,
    1.0,
    0.0,
    TrailStyle::Solid,
  );

  gpu.queue.submit(std::iter::once(encoder.finish()));
  // Test passes if we reach here without panic
}

/// Test that disabled renderer skips render
#[test]
fn test_render_when_disabled() {
  let Some(gpu) = TestGpu::shared() else {
    eprintln!("Skipping test_render_when_disabled: no GPU available");
    return;
  };

  let format = wgpu::TextureFormat::Rgba8Unorm;
  let mut renderer = TrailRenderer::new(&gpu.device, 100, 100, format);
  let target = gpu.create_target(100, 100);

  // Create and add a trail
  let mut trail = Trail2D::new(Color::WHITE, 5.0).with_min_distance(0.0);
  trail.update(0.0, 0.0, 0.016);
  trail.update(50.0, 50.0, 0.016);
  trail.update(100.0, 100.0, 0.016);
  renderer.add_trail(&trail);

  // Disable the renderer
  renderer.set_enabled(false);

  // Clear target to black
  target.clear(gpu, 0.0, 0.0, 0.0, 1.0);

  let mut encoder = gpu
    .device
    .create_command_encoder(&wgpu::CommandEncoderDescriptor {
      label: Some("Disabled Renderer Test"),
    });

  // Render with disabled renderer
  renderer.render(
    &gpu.queue,
    &mut encoder,
    &target.view,
    Vec2::ZERO,
    1.0,
    0.0,
    TrailStyle::Solid,
  );

  gpu.queue.submit(std::iter::once(encoder.finish()));

  // Target should remain black since renderer was disabled
  let center = target.get_pixel(gpu, 50, 50);
  assert_eq!(
    center.0, 0,
    "Disabled renderer should not render (red channel)"
  );
  assert_eq!(
    center.1, 0,
    "Disabled renderer should not render (green channel)"
  );
  assert_eq!(
    center.2, 0,
    "Disabled renderer should not render (blue channel)"
  );
}

/// Test that trails render visible output
#[test]
fn test_render_produces_visible_output() {
  let Some(gpu) = TestGpu::shared() else {
    eprintln!("Skipping test_render_produces_visible_output: no GPU available");
    return;
  };

  let format = wgpu::TextureFormat::Rgba8Unorm;
  let mut renderer = TrailRenderer::new(&gpu.device, 64, 64, format);
  let target = gpu.create_target(64, 64);

  // Create a wide white trail through the center
  let mut trail = Trail2D::new(Color::WHITE, 20.0) // Wide trail
    .with_min_distance(0.0);
  trail.update(0.0, 32.0, 0.016);
  trail.update(32.0, 32.0, 0.016);
  trail.update(64.0, 32.0, 0.016);

  renderer.add_trail(&trail);

  // Clear target to black
  target.clear(gpu, 0.0, 0.0, 0.0, 1.0);

  let mut encoder = gpu
    .device
    .create_command_encoder(&wgpu::CommandEncoderDescriptor {
      label: Some("Trail Render Output Test"),
    });

  // Render the trail (camera centered on target)
  renderer.render(
    &gpu.queue,
    &mut encoder,
    &target.view,
    Vec2::new(32.0, 32.0),
    1.0,
    0.0,
    TrailStyle::Solid,
  );

  gpu.queue.submit(std::iter::once(encoder.finish()));

  // The center should have visible trail pixels
  let center = target.get_pixel(gpu, 32, 32);
  let brightness = (center.0 as u32 + center.1 as u32 + center.2 as u32) / 3;

  // Trail should be visible (not completely black)
  assert!(
    brightness > 50,
    "Trail should render visible output at center, got brightness {}",
    brightness
  );
}

/// Test different trail styles render without crashing
#[test]
fn test_render_all_styles() {
  let Some(gpu) = TestGpu::shared() else {
    eprintln!("Skipping test_render_all_styles: no GPU available");
    return;
  };

  let format = wgpu::TextureFormat::Rgba8Unorm;
  let mut renderer = TrailRenderer::new(&gpu.device, 64, 64, format);
  let target = gpu.create_target(64, 64);

  let styles = [TrailStyle::Solid, TrailStyle::Soft, TrailStyle::Glow];

  for style in styles {
    renderer.begin_frame();

    // Create a trail
    let mut trail = Trail2D::new(Color::WHITE, 10.0).with_min_distance(0.0);
    trail.update(10.0, 32.0, 0.016);
    trail.update(32.0, 32.0, 0.016);
    trail.update(54.0, 32.0, 0.016);

    renderer.add_trail(&trail);

    target.clear(gpu, 0.0, 0.0, 0.0, 1.0);

    let mut encoder = gpu
      .device
      .create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some(&format!("Trail Style {:?} Test", style)),
      });

    renderer.render(
      &gpu.queue,
      &mut encoder,
      &target.view,
      Vec2::new(32.0, 32.0),
      1.0,
      0.0,
      style,
    );

    gpu.queue.submit(std::iter::once(encoder.finish()));
    // Test passes if we reach here without panic for each style
  }
}

// ============================================================================
// REGRESSION TESTS
// ============================================================================

/// REGRESSION: Verify visibility check is applied before adding vertices
///
/// This test verifies that the renderer respects the trail's visibility state,
/// fixing the "ghost trails" bug where trails would remain visible after being
/// disabled via set_enabled(false).
#[test]
fn regression_visibility_check_in_add_trail() {
  let Some(gpu) = TestGpu::shared() else {
    eprintln!("Skipping regression_visibility_check_in_add_trail: no GPU available");
    return;
  };

  let format = wgpu::TextureFormat::Rgba8Unorm;
  let mut renderer = TrailRenderer::new(&gpu.device, 100, 100, format);

  // Create trail and add points while enabled
  let mut trail = Trail2D::new(Color::WHITE, 5.0).with_min_distance(0.0);
  trail.update(0.0, 0.0, 0.016);
  trail.update(10.0, 10.0, 0.016);
  trail.update(20.0, 20.0, 0.016);

  // Verify it has points
  assert_eq!(trail.point_count(), 3);
  assert!(trail.is_visible());

  // Disable the trail (should set visible=false)
  trail.set_enabled(false);
  assert!(!trail.is_visible(), "set_enabled(false) should hide trail");

  // Try to add to renderer
  renderer.add_trail(&trail);

  // Should have no vertices because trail is not visible
  assert_eq!(
    renderer.vertex_count(),
    0,
    "Hidden trail should not add vertices - ghost trail bug!"
  );
}

/// REGRESSION: Multiple trails must not connect to each other
///
/// This tests the degenerate triangle fix that prevents the "double tail" bug
/// where separate trails would appear connected by a line across the screen.
#[test]
fn regression_no_cross_trail_connections() {
  let Some(gpu) = TestGpu::shared() else {
    eprintln!("Skipping regression_no_cross_trail_connections: no GPU available");
    return;
  };

  let format = wgpu::TextureFormat::Rgba8Unorm;
  let mut renderer = TrailRenderer::new(&gpu.device, 200, 200, format);
  let target = gpu.create_target(200, 200);

  // Create two trails at opposite corners
  let mut trail1 = Trail2D::new(Color::WHITE, 10.0).with_min_distance(0.0);
  trail1.update(10.0, 10.0, 0.016);
  trail1.update(30.0, 30.0, 0.016);
  trail1.update(50.0, 50.0, 0.016);

  let mut trail2 = Trail2D::new(Color::WHITE, 10.0).with_min_distance(0.0);
  trail2.update(150.0, 150.0, 0.016);
  trail2.update(170.0, 170.0, 0.016);
  trail2.update(190.0, 190.0, 0.016);

  renderer.add_trail(&trail1);
  renderer.add_trail(&trail2);

  target.clear(gpu, 0.0, 0.0, 0.0, 1.0);

  let mut encoder = gpu
    .device
    .create_command_encoder(&wgpu::CommandEncoderDescriptor {
      label: Some("Trail Connection Test"),
    });

  renderer.render(
    &gpu.queue,
    &mut encoder,
    &target.view,
    Vec2::new(100.0, 100.0),
    1.0,
    0.0,
    TrailStyle::Solid,
  );

  gpu.queue.submit(std::iter::once(encoder.finish()));

  // The center of the screen (100, 100) should be black/empty
  // because neither trail passes through there
  let center = target.get_pixel(gpu, 100, 100);
  let brightness = (center.0 as u32 + center.1 as u32 + center.2 as u32) / 3;

  assert!(
    brightness < 30,
    "Center should be dark (no cross-trail connection), got brightness {}",
    brightness
  );
}

// ============================================================================
// LINE2D TESTS
// ============================================================================

/// Test that Line2D renders without crashing
#[test]
fn test_line_rendering() {
  let Some(gpu) = TestGpu::shared() else {
    eprintln!("Skipping test_line_rendering: no GPU available");
    return;
  };

  let format = wgpu::TextureFormat::Rgba8Unorm;
  let mut renderer = TrailRenderer::new(&gpu.device, 100, 100, format);
  let target = gpu.create_target(100, 100);

  // Create a simple line
  let line = Line2D::new(
    vec![
      Vec2::new(10.0, 50.0),
      Vec2::new(50.0, 50.0),
      Vec2::new(90.0, 50.0),
    ],
    Color::WHITE,
    5.0,
  );

  renderer.add_line(&line, Vec2::new(0.0, 0.0));

  assert!(renderer.vertex_count() > 0, "Line should generate vertices");

  target.clear(gpu, 0.0, 0.0, 0.0, 1.0);

  let mut encoder = gpu
    .device
    .create_command_encoder(&wgpu::CommandEncoderDescriptor {
      label: Some("Line Render Test"),
    });

  renderer.render(
    &gpu.queue,
    &mut encoder,
    &target.view,
    Vec2::new(50.0, 50.0),
    1.0,
    0.0,
    TrailStyle::Solid,
  );

  gpu.queue.submit(std::iter::once(encoder.finish()));
  // Test passes if no panic
}

/// Test that invisible lines don't add vertices
#[test]
fn test_invisible_line_no_vertices() {
  let Some(gpu) = TestGpu::shared() else {
    eprintln!("Skipping test_invisible_line_no_vertices: no GPU available");
    return;
  };

  let format = wgpu::TextureFormat::Rgba8Unorm;
  let mut renderer = TrailRenderer::new(&gpu.device, 100, 100, format);

  // Create an invisible line
  let mut line = Line2D::new(
    vec![Vec2::new(10.0, 50.0), Vec2::new(90.0, 50.0)],
    Color::WHITE,
    5.0,
  );
  line.set_visible(false);

  assert!(!line.is_visible(), "Line should be invisible");

  renderer.add_line(&line, Vec2::new(0.0, 0.0));

  assert_eq!(
    renderer.vertex_count(),
    0,
    "Invisible line should not add vertices"
  );
}
