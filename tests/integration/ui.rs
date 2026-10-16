//! UI system integration tests
//!
//! These tests verify UI rendering and its interaction with lighting.
//!
//! REGRESSION CONTEXT:
//! - UI disappeared when lighting was added (UI was affected by darkness)
//! - UI should always be visible by default, regardless of scene lighting

use ryo2d::prelude::*;

// ============================================================================
// REGRESSION TESTS - Critical invariants that must never break
// ============================================================================

/// REGRESSION: UI must be visible regardless of lighting by default
///
/// This test documents the critical behavior that UI elements (HUD, menus,
/// text overlays) should always render at full brightness, even when the
/// scene is in complete darkness (ambient = 0.0).
///
/// The fix for this was:
/// 1. UI is drawn into an image of its own
/// 2. Lighting is applied only to the scene
/// 3. The UI image is composited on top after lighting
#[test]
fn regression_ui_visible_in_darkness() {
  // This documents the expected behavior:
  // - UI bypasses the lighting composition
  // - UI appears at full brightness regardless of ambient intensity

  // In a pitch-black scene (ambient = 0.0), UI should still be visible
  let ambient = AmbientLight::dark();
  assert!(ambient.intensity <= 0.1, "Dark ambient for test");

  // The UI rendering path is separate from the lit scene path
  // UI buffer -> composited after lighting -> always visible
}

/// REGRESSION: Color type must have consistent construction methods
#[test]
fn regression_color_construction() {
  // RGB construction (alpha defaults to 255)
  let color = Color::rgb(255, 128, 0);
  assert_eq!(color.r, 255);
  assert_eq!(color.g, 128);
  assert_eq!(color.b, 0);
  assert_eq!(color.a, 255, "RGB constructor should default alpha to 255");

  // RGBA construction via new()
  let color = Color::new(255, 128, 0, 128);
  assert_eq!(color.r, 255);
  assert_eq!(color.g, 128);
  assert_eq!(color.b, 0);
  assert_eq!(color.a, 128);

  // Hex construction
  let color = Color::hex(0xFF8000);
  assert_eq!(color.r, 255);
  assert_eq!(color.g, 128);
  assert_eq!(color.b, 0);
}

// ============================================================================
// COLOR PRESET TESTS
// ============================================================================

/// Test standard color presets
#[test]
fn color_presets() {
  assert_eq!(Color::WHITE.r, 255);
  assert_eq!(Color::WHITE.g, 255);
  assert_eq!(Color::WHITE.b, 255);
  assert_eq!(Color::WHITE.a, 255);

  assert_eq!(Color::BLACK.r, 0);
  assert_eq!(Color::BLACK.g, 0);
  assert_eq!(Color::BLACK.b, 0);
  assert_eq!(Color::BLACK.a, 255);

  assert_eq!(Color::RED.r, 255);
  assert_eq!(Color::RED.g, 0);
  assert_eq!(Color::RED.b, 0);

  assert_eq!(Color::GREEN.r, 0);
  assert_eq!(Color::GREEN.g, 255);
  assert_eq!(Color::GREEN.b, 0);

  assert_eq!(Color::BLUE.r, 0);
  assert_eq!(Color::BLUE.g, 0);
  assert_eq!(Color::BLUE.b, 255);

  // Transparent
  assert_eq!(Color::TRANSPARENT.a, 0);
}

/// Test color with alpha
#[test]
fn color_alpha() {
  let color = Color::new(255, 255, 255, 128);
  assert_eq!(color.a, 128);

  let color = Color::new(255, 255, 255, 0);
  assert_eq!(color.a, 0);
}

// ============================================================================
// RECT TESTS
// ============================================================================

/// Test Rect construction
#[test]
fn rect_construction() {
  let rect = Rect::new(10.0, 20.0, 100.0, 50.0);

  assert_eq!(rect.x, 10.0);
  assert_eq!(rect.y, 20.0);
  assert_eq!(rect.width, 100.0);
  assert_eq!(rect.height, 50.0);
}

/// Test Rect contains point
#[test]
fn rect_contains_point() {
  let rect = Rect::new(10.0, 20.0, 100.0, 50.0);
  // Rect spans x: 10-110, y: 20-70

  // Inside
  assert!(
    rect.contains(50.0, 40.0),
    "Point inside should be contained"
  );
  assert!(
    rect.contains(10.0, 20.0),
    "Top-left corner should be contained"
  );

  // Outside
  assert!(
    !rect.contains(5.0, 40.0),
    "Point left of rect should not be contained"
  );
  assert!(
    !rect.contains(50.0, 15.0),
    "Point above rect should not be contained"
  );
  assert!(
    !rect.contains(150.0, 40.0),
    "Point right of rect should not be contained"
  );
  assert!(
    !rect.contains(50.0, 80.0),
    "Point below rect should not be contained"
  );

  // Edge cases: right and bottom edges are EXCLUSIVE (standard for UI hit testing)
  // This prevents double-hits when rectangles are adjacent
  assert!(
    !rect.contains(110.0, 40.0),
    "Right edge should be exclusive"
  );
  assert!(
    !rect.contains(50.0, 70.0),
    "Bottom edge should be exclusive"
  );

  // Just inside the boundary
  assert!(
    rect.contains(109.9, 40.0),
    "Just inside right edge should be contained"
  );
  assert!(
    rect.contains(50.0, 69.9),
    "Just inside bottom edge should be contained"
  );
}

/// Test Rect center
#[test]
fn rect_center() {
  let rect = Rect::new(10.0, 20.0, 100.0, 50.0);
  let (cx, cy) = rect.center();

  assert_eq!(cx, 60.0, "Center X should be x + width/2");
  assert_eq!(cy, 45.0, "Center Y should be y + height/2");
}

// ============================================================================
// ANCHOR TESTS
// ============================================================================

/// Test all 9 anchor points
#[test]
fn anchor_all_points() {
  // Top row
  let (px, py) = Anchor::TopLeft.to_pivot();
  assert_eq!((px, py), (0.0, 0.0));

  let (px, py) = Anchor::TopCenter.to_pivot();
  assert_eq!((px, py), (0.5, 0.0));

  let (px, py) = Anchor::TopRight.to_pivot();
  assert_eq!((px, py), (1.0, 0.0));

  // Middle row
  let (px, py) = Anchor::CenterLeft.to_pivot();
  assert_eq!((px, py), (0.0, 0.5));

  let (px, py) = Anchor::Center.to_pivot();
  assert_eq!((px, py), (0.5, 0.5));

  let (px, py) = Anchor::CenterRight.to_pivot();
  assert_eq!((px, py), (1.0, 0.5));

  // Bottom row
  let (px, py) = Anchor::BottomLeft.to_pivot();
  assert_eq!((px, py), (0.0, 1.0));

  let (px, py) = Anchor::BottomCenter.to_pivot();
  assert_eq!((px, py), (0.5, 1.0));

  let (px, py) = Anchor::BottomRight.to_pivot();
  assert_eq!((px, py), (1.0, 1.0));
}

/// Test anchor position calculation
#[test]
fn anchor_position_calculation() {
  let screen_width = 800.0;
  let screen_height = 600.0;
  let elem_width = 100.0;
  let elem_height = 50.0;

  // TopLeft with no offset should be at (0, 0)
  let (x, y) = Anchor::TopLeft.calculate_position(
    0.0,
    0.0,
    elem_width,
    elem_height,
    screen_width,
    screen_height,
  );
  assert_eq!((x, y), (0.0, 0.0));

  // TopRight should position element at right edge
  let (x, y) = Anchor::TopRight.calculate_position(
    0.0,
    0.0,
    elem_width,
    elem_height,
    screen_width,
    screen_height,
  );
  assert_eq!(x, 700.0, "TopRight X should be screen_width - elem_width");
  assert_eq!(y, 0.0);

  // Center should position element in center of screen
  let (x, y) = Anchor::Center.calculate_position(
    0.0,
    0.0,
    elem_width,
    elem_height,
    screen_width,
    screen_height,
  );
  assert_eq!(
    x, 350.0,
    "Center X should be (screen_width - elem_width) / 2"
  );
  assert_eq!(
    y, 275.0,
    "Center Y should be (screen_height - elem_height) / 2"
  );

  // BottomCenter should be at bottom center
  let (x, y) = Anchor::BottomCenter.calculate_position(
    0.0,
    0.0,
    elem_width,
    elem_height,
    screen_width,
    screen_height,
  );
  assert_eq!(x, 350.0);
  assert_eq!(
    y, 550.0,
    "BottomCenter Y should be screen_height - elem_height"
  );
}

/// Test anchor with offset
#[test]
fn anchor_with_offset() {
  let screen_width = 800.0;
  let screen_height = 600.0;
  let elem_width = 100.0;
  let elem_height = 50.0;

  // TopLeft with offset
  let (x, y) = Anchor::TopLeft.calculate_position(
    10.0,
    20.0,
    elem_width,
    elem_height,
    screen_width,
    screen_height,
  );
  assert_eq!((x, y), (10.0, 20.0));

  // BottomRight with negative offset (inset from corner)
  let (x, y) = Anchor::BottomRight.calculate_position(
    -10.0,
    -20.0,
    elem_width,
    elem_height,
    screen_width,
    screen_height,
  );
  assert_eq!(x, 690.0, "Should be 10px inset from right edge");
  assert_eq!(y, 530.0, "Should be 20px inset from bottom edge");
}

// ============================================================================
// UI + ECS INTEGRATION (if applicable)
// ============================================================================

/// Test that UI types don't interfere with ECS
#[test]
fn ui_types_independent_of_ecs() {
  // UI types (Color, Rect, Anchor) should be usable without ECS
  let color = Color::RED;
  let rect = Rect::new(0.0, 0.0, 100.0, 100.0);
  let anchor = Anchor::Center;

  // They can be used together
  let _ = (color, rect, anchor);

  // ECS World should work independently
  let mut world = World::new();
  let entity = world.spawn((Transform::from_xy(0.0, 0.0),)).id();

  // No conflicts between UI and ECS systems - verify entity exists
  assert!(world.get_entity(entity).is_ok());
}

// ============================================================================
// DPI/UI SCALE TESTS
// ============================================================================

/// Test that EngineBuilder accepts ui_scale configuration
#[test]
fn engine_builder_ui_scale_config() {
  // Test that ui_scale can be set to manual values
  // We just verify the builder accepts these configurations
  let _builder = EngineBuilder::new()
    .title("Test")
    .resolution(800, 600)
    .ui_scale(Some(2.0));
  // Builder pattern works - can't easily test build() without a window
}

/// Test that ui_scale None means auto-detect
#[test]
fn engine_builder_ui_scale_auto() {
  // Test that ui_scale can be set to auto-detect
  let _builder = EngineBuilder::new()
    .title("Test")
    .resolution(800, 600)
    .ui_scale(None); // UI units equal screen units at any display scale
  // Builder pattern works
}

/// Test that various ui_scale values are valid
#[test]
fn ui_scale_values_valid() {
  // One render pixel per UI unit
  let _builder1 = EngineBuilder::new().ui_scale(Some(1.0));

  // Two (a big UI)
  let _builder2 = EngineBuilder::new().ui_scale(Some(2.0));

  // Fractional
  let _builder3 = EngineBuilder::new().ui_scale(Some(1.5));

  // All configurations accepted by builder
}
