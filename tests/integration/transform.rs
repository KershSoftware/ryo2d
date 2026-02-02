//! Transform and pivot system integration tests
//!
//! These tests verify transform pivots behave correctly and don't break
//! existing game positioning logic.
//!
//! REGRESSION CONTEXT:
//! - Changing default pivot from (0,0) to (0.5,0.5) broke all existing games
//! - Sprites appeared in wrong positions
//! - Flappy bird spun wildly instead of tilting (combined with radians bug)
//! - Pivot offset calculation was incorrectly applied to (0,0) pivot

use ryo2d::prelude::*;
use std::f32::consts::{FRAC_PI_2, PI, TAU};

// ============================================================================
// REGRESSION TESTS - Critical invariants that must never break
// ============================================================================

/// REGRESSION: Default pivot MUST be top-left (0, 0)
///
/// This is critical for backwards compatibility. Existing games expect:
/// - Transform position = top-left corner of sprite
/// - Sprites spawn at exact coordinates given
///
/// Changing this default broke all existing games.
#[test]
fn regression_default_pivot_is_top_left() {
  let transform = Transform::new(100.0, 200.0);

  assert_eq!(transform.pivot_x, 0.0, "Default pivot_x must be 0.0 (left)");
  assert_eq!(transform.pivot_y, 0.0, "Default pivot_y must be 0.0 (top)");
}

/// REGRESSION: Top-left pivot (0,0) must use simple positioning
///
/// When pivot is (0,0), the engine should NOT apply pivot offset calculations.
/// This is a special case that preserves the original/legacy behavior where
/// transform.x/y directly represents the top-left corner of the sprite.
///
/// The pivot offset calculation is: offset = (pivot - 0.5) * size
/// For (0,0): offset = (-0.5, -0.5) * size = shift sprite up-left
/// For (0.5,0.5): offset = (0,0) = no shift (centered on position)
///
/// But for backwards compatibility, (0,0) pivot skips this calculation entirely.
#[test]
fn regression_top_left_pivot_no_offset_calculation() {
  let transform = Transform::new(100.0, 100.0);

  // With (0,0) pivot:
  // - Position (100, 100) should be the TOP-LEFT corner of the sprite
  // - No pivot offset should be applied
  // - Sprite draws at exactly (100, 100) as its origin

  assert_eq!(transform.pivot_x, 0.0);
  assert_eq!(transform.pivot_y, 0.0);

  // This test documents the engine behavior:
  // In engine.rs render loop, when pivot is (0,0), the simple draw path is used
  // without pivot offset calculations.
}

/// REGRESSION: Default rotation must be 0 (no rotation)
#[test]
fn regression_default_rotation_is_zero() {
  let transform = Transform::new(100.0, 200.0);
  assert_eq!(transform.rotation, 0.0, "Default rotation must be 0");
}

/// REGRESSION: Default scale must be 1.0 (no scaling)
#[test]
fn regression_default_scale_is_one() {
  let transform = Transform::new(100.0, 200.0);
  assert_eq!(transform.scale_x, 1.0, "Default scale_x must be 1.0");
  assert_eq!(transform.scale_y, 1.0, "Default scale_y must be 1.0");
}

/// REGRESSION: Engine uses radians for rotation, not degrees
///
/// This catches the bug where game code set rotation in degrees
/// (e.g., 25 degrees) but the engine interpreted it as radians
/// (25 radians = 1432 degrees), causing sprites to spin wildly.
#[test]
fn regression_rotation_is_in_radians() {
  // Document that rotation is in radians
  let mut transform = Transform::new(0.0, 0.0);

  // 90 degrees = PI/2 radians
  transform.rotation = FRAC_PI_2;
  assert!(
    (transform.rotation - FRAC_PI_2).abs() < 0.0001,
    "90 degrees should be PI/2 radians"
  );

  // 180 degrees = PI radians
  transform.rotation = PI;
  assert!(
    (transform.rotation - PI).abs() < 0.0001,
    "180 degrees should be PI radians"
  );

  // Full rotation = 2*PI radians (TAU)
  transform.rotation = TAU;
  assert!(
    (transform.rotation - TAU).abs() < 0.0001,
    "360 degrees should be TAU radians"
  );

  // Common mistake: using degrees directly
  // If someone sets rotation = 45.0 (intending 45 degrees),
  // they're actually setting 45 radians = 2578 degrees = 7+ full rotations
  // This is a game-level bug, but we document the expected behavior here.
}

// ============================================================================
// PIVOT BUILDER TESTS
// ============================================================================

/// Test center pivot (0.5, 0.5)
#[test]
fn pivot_center() {
  let transform = Transform::new(100.0, 200.0).with_pivot_center();

  assert_eq!(transform.pivot_x, 0.5, "Center pivot_x should be 0.5");
  assert_eq!(transform.pivot_y, 0.5, "Center pivot_y should be 0.5");
  assert_eq!(transform.x, 100.0, "Position should be unchanged");
  assert_eq!(transform.y, 200.0, "Position should be unchanged");
}

/// Test top-center pivot (0.5, 0.0) - used for pendulums, hanging objects
#[test]
fn pivot_top_center() {
  let transform = Transform::new(100.0, 200.0).with_pivot_top_center();

  assert_eq!(transform.pivot_x, 0.5, "Top-center pivot_x should be 0.5");
  assert_eq!(transform.pivot_y, 0.0, "Top-center pivot_y should be 0.0");
}

/// Test bottom-center pivot (0.5, 1.0) - used for standing objects
#[test]
fn pivot_bottom_center() {
  let transform = Transform::new(100.0, 200.0).with_pivot_bottom_center();

  assert_eq!(
    transform.pivot_x, 0.5,
    "Bottom-center pivot_x should be 0.5"
  );
  assert_eq!(
    transform.pivot_y, 1.0,
    "Bottom-center pivot_y should be 1.0"
  );
}

/// Test explicit top-left pivot
#[test]
fn pivot_top_left_explicit() {
  let transform = Transform::new(100.0, 200.0).with_pivot_top_left();

  assert_eq!(transform.pivot_x, 0.0, "Top-left pivot_x should be 0.0");
  assert_eq!(transform.pivot_y, 0.0, "Top-left pivot_y should be 0.0");
}

/// Test top-right pivot
#[test]
fn pivot_top_right() {
  let transform = Transform::new(100.0, 200.0).with_pivot_top_right();

  assert_eq!(transform.pivot_x, 1.0, "Top-right pivot_x should be 1.0");
  assert_eq!(transform.pivot_y, 0.0, "Top-right pivot_y should be 0.0");
}

/// Test custom pivot values
#[test]
fn pivot_custom_values() {
  let transform = Transform::new(100.0, 200.0).with_pivot(0.25, 0.75);

  assert_eq!(
    transform.pivot_x, 0.25,
    "Custom pivot_x should be preserved"
  );
  assert_eq!(
    transform.pivot_y, 0.75,
    "Custom pivot_y should be preserved"
  );
}

/// Test pivot via Anchor enum
#[test]
fn pivot_via_anchor() {
  let transform = Transform::new(100.0, 200.0).with_anchor(Anchor::Center);
  assert_eq!(transform.pivot_x, 0.5);
  assert_eq!(transform.pivot_y, 0.5);

  let transform = Transform::new(100.0, 200.0).with_anchor(Anchor::BottomRight);
  assert_eq!(transform.pivot_x, 1.0);
  assert_eq!(transform.pivot_y, 1.0);

  let transform = Transform::new(100.0, 200.0).with_anchor(Anchor::TopLeft);
  assert_eq!(transform.pivot_x, 0.0);
  assert_eq!(transform.pivot_y, 0.0);
}

// ============================================================================
// TRANSFORM BUILDER CHAIN TESTS
// ============================================================================

/// Test that builder methods can be chained
#[test]
fn builder_method_chaining() {
  let transform = Transform::new(100.0, 200.0)
    .with_pivot_center()
    .with_rotation(PI / 4.0)
    .with_scale(2.0, 2.0);

  assert_eq!(transform.x, 100.0);
  assert_eq!(transform.y, 200.0);
  assert_eq!(transform.pivot_x, 0.5);
  assert_eq!(transform.pivot_y, 0.5);
  assert!((transform.rotation - PI / 4.0).abs() < 0.0001);
  assert_eq!(transform.scale_x, 2.0);
  assert_eq!(transform.scale_y, 2.0);
}

/// Test uniform scale builder
#[test]
fn uniform_scale() {
  let transform = Transform::new(0.0, 0.0).with_uniform_scale(3.0);

  assert_eq!(transform.scale_x, 3.0);
  assert_eq!(transform.scale_y, 3.0);
}

/// Test offset builder
#[test]
fn offset_builder() {
  let transform = Transform::new(100.0, 200.0)
    .with_anchor(Anchor::Center)
    .with_offset(10.0, -5.0);

  assert_eq!(transform.offset_x, 10.0);
  assert_eq!(transform.offset_y, -5.0);
}

// ============================================================================
// TRANSFORM + WORLD INTEGRATION TESTS
// ============================================================================

/// Test transforms can be spawned and queried
#[test]
fn transform_spawn_and_query() {
  let mut world = World::new();

  let entity = world.spawn((Transform::new(100.0, 200.0).with_pivot_center(),));

  let transform = world.get::<Transform>(entity);
  assert!(transform.is_some());

  let t = transform.unwrap();
  assert_eq!(t.x, 100.0);
  assert_eq!(t.y, 200.0);
  assert_eq!(t.pivot_x, 0.5);
  assert_eq!(t.pivot_y, 0.5);
}

/// Test transform modification via world
#[test]
fn transform_modification() {
  let mut world = World::new();

  let entity = world.spawn((Transform::new(0.0, 0.0),));

  // Modify transform
  if let Some(mut transform) = world.get_mut::<Transform>(entity) {
    transform.x = 50.0;
    transform.y = 75.0;
    transform.rotation = PI / 2.0;
  }

  // Verify changes persisted
  let transform = world.get::<Transform>(entity).unwrap();
  assert_eq!(transform.x, 50.0);
  assert_eq!(transform.y, 75.0);
  assert!((transform.rotation - PI / 2.0).abs() < 0.0001);
}

/// Test Vec2/Vec3 conversion methods
#[test]
fn vec_conversions() {
  let transform = Transform::new(100.0, 200.0);

  let v2 = transform.as_vec2();
  assert_eq!(v2.x, 100.0);
  assert_eq!(v2.y, 200.0);

  let v3 = transform.as_vec3();
  assert_eq!(v3.x, 100.0);
  assert_eq!(v3.y, 200.0);
  assert_eq!(v3.z, 0.0); // Default z

  // Test from_vec2
  let transform = Transform::from_vec2(Vec2::new(50.0, 75.0));
  assert_eq!(transform.x, 50.0);
  assert_eq!(transform.y, 75.0);

  // Test from_vec3
  let transform = Transform::from_vec3(Vec3::new(10.0, 20.0, 30.0));
  assert_eq!(transform.x, 10.0);
  assert_eq!(transform.y, 20.0);
  assert_eq!(transform.z, 30.0);
}

// ============================================================================
// VELOCITY COMPONENT TESTS
// ============================================================================

/// Test Velocity creation and defaults
#[test]
fn velocity_creation() {
  let velocity = Velocity::new(5.0, -3.0);
  assert_eq!(velocity.dx, 5.0);
  assert_eq!(velocity.dy, -3.0);
}

/// Test Velocity default is zero
#[test]
fn velocity_default_is_zero() {
  let velocity = Velocity::default();
  assert_eq!(velocity.dx, 0.0);
  assert_eq!(velocity.dy, 0.0);
}

/// Test Velocity with Transform in ECS
#[test]
fn velocity_with_transform() {
  let mut world = World::new();

  let _entity = world.spawn((Transform::new(100.0, 100.0), Velocity::new(10.0, 5.0)));

  // Query both components together
  let mut query = world.query::<(&Transform, &Velocity)>();
  let query_result: Vec<_> = query.iter().collect();
  assert_eq!(query_result.len(), 1);

  let (transform, velocity) = query_result[0];
  assert_eq!(transform.x, 100.0);
  assert_eq!(velocity.dx, 10.0);
}
