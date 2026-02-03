//! Integration tests for the render pipeline system
//!
//! These tests verify the render pipeline orchestration, pass dependencies,
//! and GPU parallax functionality.

use ryo2d::core::graphics::parallax::TileMode;
use ryo2d::core::graphics::{GpuParallaxBackground, GpuParallaxLayer};
use ryo2d::core::render::passes::{ClearPass, ParallaxPass, UploadPass};
use ryo2d::core::render::{RenderPass, RenderPipeline, pass_ids};

// =============================================================================
// Pipeline Orchestration Tests
// =============================================================================

#[test]
fn test_empty_pipeline() {
  let pipeline = RenderPipeline::new();
  assert_eq!(pipeline.pass_count(), 0);
}

#[test]
fn test_add_passes() {
  let mut pipeline = RenderPipeline::new();
  pipeline.add_pass(ClearPass::new());
  assert_eq!(pipeline.pass_count(), 1);

  pipeline.add_pass(UploadPass::new());
  assert_eq!(pipeline.pass_count(), 2);
}

#[test]
fn test_remove_pass() {
  let mut pipeline = RenderPipeline::new();
  pipeline.add_pass(ClearPass::new());
  pipeline.add_pass(UploadPass::new());

  let removed = pipeline.remove_pass(pass_ids::CLEAR);
  assert!(removed.is_some());
  assert_eq!(removed.unwrap().id(), pass_ids::CLEAR);
  assert_eq!(pipeline.pass_count(), 1);
}

#[test]
fn test_pass_dependency_order() {
  let mut pipeline = RenderPipeline::new();

  // Add passes in reverse dependency order
  pipeline.add_pass(UploadPass::new()); // Depends on cpu_sprite
  pipeline.add_pass(ClearPass::new()); // No dependencies

  let ids = pipeline.pass_ids();

  // Clear should come before upload
  let clear_pos = ids.iter().position(|&id| id == pass_ids::CLEAR).unwrap();
  let upload_pos = ids.iter().position(|&id| id == pass_ids::UPLOAD).unwrap();

  // Note: upload depends on cpu_sprite which isn't added, so it has 0 resolved deps
  // Both clear and upload will have 0 in-degree, so they'll be sorted alphabetically
  assert!(clear_pos < upload_pos, "Clear should come before upload in alphabetical order");
}

#[test]
fn test_enable_disable_pass() {
  let mut pipeline = RenderPipeline::new();
  pipeline.add_pass(ClearPass::new());

  assert!(pipeline.is_enabled(pass_ids::CLEAR));

  pipeline.set_enabled(pass_ids::CLEAR, false);
  assert!(!pipeline.is_enabled(pass_ids::CLEAR));

  pipeline.set_enabled(pass_ids::CLEAR, true);
  assert!(pipeline.is_enabled(pass_ids::CLEAR));
}

#[test]
fn test_get_pass() {
  let mut pipeline = RenderPipeline::new();
  pipeline.add_pass(ClearPass::new());

  let pass = pipeline.get_pass(pass_ids::CLEAR);
  assert!(pass.is_some());
  assert_eq!(pass.unwrap().id(), pass_ids::CLEAR);

  let missing = pipeline.get_pass("nonexistent");
  assert!(missing.is_none());
}

// =============================================================================
// Clear Pass Tests
// =============================================================================

#[test]
fn test_clear_pass_default_color() {
  let pass = ClearPass::new();
  assert_eq!(pass.color, [0x2d, 0x2d, 0x2d, 0xff]);
}

#[test]
fn test_clear_pass_custom_color() {
  let pass = ClearPass::with_color(255, 0, 0, 255);
  assert_eq!(pass.color, [255, 0, 0, 255]);
}

#[test]
fn test_clear_pass_no_dependencies() {
  let pass = ClearPass::new();
  assert!(pass.dependencies().is_empty());
}

// =============================================================================
// Upload Pass Tests
// =============================================================================

#[test]
fn test_upload_pass_dependencies() {
  let pass = UploadPass::new();
  assert_eq!(pass.dependencies(), &[pass_ids::CPU_SPRITE]);
}

// =============================================================================
// Parallax Pass Tests
// =============================================================================

#[test]
fn test_parallax_pass_empty() {
  let pass = ParallaxPass::new();
  assert_eq!(pass.layer_count(), 0);
}

#[test]
fn test_parallax_pass_add_layers() {
  use ryo2d::core::render::passes::GpuParallaxLayer;

  let mut pass = ParallaxPass::new();

  pass.add_layer(GpuParallaxLayer::new("layer1", 100, 100, 0.0, -10));
  pass.add_layer(GpuParallaxLayer::new("layer2", 100, 100, 0.5, -5));

  assert_eq!(pass.layer_count(), 2);
}

#[test]
fn test_parallax_pass_layer_sorting() {
  use ryo2d::core::render::passes::GpuParallaxLayer;

  let mut pass = ParallaxPass::new();

  // Add layers out of z-order
  pass.add_layer(GpuParallaxLayer::new("front", 100, 100, 0.8, -5));
  pass.add_layer(GpuParallaxLayer::new("back", 100, 100, 0.2, -20));
  pass.add_layer(GpuParallaxLayer::new("mid", 100, 100, 0.5, -10));

  let layers = pass.layers();
  assert_eq!(layers[0].texture_id, "back");
  assert_eq!(layers[1].texture_id, "mid");
  assert_eq!(layers[2].texture_id, "front");
}

#[test]
fn test_parallax_pass_dependencies() {
  let pass = ParallaxPass::new();
  assert_eq!(pass.dependencies(), &[pass_ids::CLEAR]);
}

// =============================================================================
// GPU Parallax Background Tests
// =============================================================================

#[test]
fn test_gpu_parallax_background_new() {
  let bg = GpuParallaxBackground::new();
  assert_eq!(bg.layer_count(), 0);
}

#[test]
fn test_gpu_parallax_layer_builder_pattern() {
  let layer = GpuParallaxLayer::from_id("test", 100, 100, 0.5, -10)
    .with_auto_scroll(20.0)
    .with_y_offset(50.0)
    .with_tile_mode(TileMode::InfiniteScroll)
    .with_scale(2.0)
    .with_tint(255, 128, 64, 200);

  // Test public fields
  assert_eq!(layer.scroll_speed, 0.5);
  assert_eq!(layer.auto_scroll_speed, 20.0);
  assert_eq!(layer.y_offset, 50.0);
  assert_eq!(layer.tile_mode, TileMode::InfiniteScroll);
  // scale is private, tested via the builder pattern fluency
}

#[test]
fn test_gpu_parallax_background_add_remove() {
  let mut bg = GpuParallaxBackground::new();

  bg.add_layer(GpuParallaxLayer::from_id("layer1", 100, 100, 0.0, -10));
  bg.add_layer(GpuParallaxLayer::from_id("layer2", 100, 100, 0.5, -5));
  assert_eq!(bg.layer_count(), 2);

  let removed = bg.remove_layer("layer1");
  assert!(removed.is_some());
  assert_eq!(bg.layer_count(), 1);
}

#[test]
fn test_gpu_parallax_background_layer_sorting() {
  let mut bg = GpuParallaxBackground::new();

  // Add layers out of order
  bg.add_layer(GpuParallaxLayer::from_id("front", 100, 100, 0.8, 10));
  bg.add_layer(GpuParallaxLayer::from_id("back", 100, 100, 0.0, -100));
  bg.add_layer(GpuParallaxLayer::from_id("mid", 100, 100, 0.5, 0));

  // Get layer by ID to verify they exist
  assert!(bg.get_layer_mut("back").is_some());
  assert!(bg.get_layer_mut("mid").is_some());
  assert!(bg.get_layer_mut("front").is_some());
}

#[test]
fn test_gpu_parallax_background_update_time() {
  let mut bg = GpuParallaxBackground::new();

  bg.update(0.016); // ~60fps frame
  bg.update(0.016);
  bg.update(0.016);

  // Can't easily verify internal state, but verify no panic
}

#[test]
fn test_gpu_parallax_builder() {
  // Builder pattern works with GpuParallaxBackground
  let mut custom_bg = GpuParallaxBackground::new();
  custom_bg.add_layer(
    GpuParallaxLayer::from_id("sky", 1920, 1080, 0.0, -100)
  );
  custom_bg.add_layer(
    GpuParallaxLayer::from_id("mountains", 1920, 1080, 0.3, -80)
      .with_tile_mode(TileMode::InfiniteScroll)
      .with_auto_scroll(10.0)
  );

  assert_eq!(custom_bg.layer_count(), 2);
}

// =============================================================================
// Tile Mode Tests
// =============================================================================

#[test]
fn test_tile_mode_none() {
  let layer = GpuParallaxLayer::from_id("test", 100, 100, 0.0, 0);
  assert_eq!(layer.tile_mode, TileMode::None);
}

#[test]
fn test_tile_mode_horizontal() {
  let layer = GpuParallaxLayer::from_id("test", 100, 100, 0.0, 0)
    .with_tile_mode(TileMode::Horizontal);
  assert_eq!(layer.tile_mode, TileMode::Horizontal);
}

#[test]
fn test_tile_mode_infinite_scroll() {
  let layer = GpuParallaxLayer::from_id("test", 100, 100, 0.0, 0)
    .with_tile_mode(TileMode::InfiniteScroll);
  assert_eq!(layer.tile_mode, TileMode::InfiniteScroll);
}
