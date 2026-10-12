//! Trail system integration tests
//!
//! These tests verify the Trail2D API behaves correctly, including:
//! - Visibility and enabled state synchronization
//! - Point recording behavior
//! - Edge cases like wrap-around handling
//!
//! REGRESSION CONTEXT (2024-02):
//! - `set_enabled(false)` previously only stopped recording but kept rendering (ghost trails)
//! - Opaque CPU backgrounds covered GPU-rendered trails (layering issue)
//! - Users expected trails to disappear when disabled
//!
//! CRITICAL INVARIANTS:
//! 1. `set_enabled(false)` MUST set `visible = false` (no ghost trails)
//! 2. `set_enabled(true)` MUST set `visible = true` (trails appear)
//! 3. `set_recording()` MUST NOT affect visibility (for fade-out effects)
//! 4. `set_visible()` MUST NOT affect recording (independent control)

use ryo2d::prelude::*;

// ============================================================================
// REGRESSION TESTS - Critical invariants that must never break
// ============================================================================

/// REGRESSION: set_enabled(false) must hide trails immediately
///
/// Previously, set_enabled(false) only stopped recording new points but kept
/// rendering existing ones. This caused "ghost trails" that confused users.
#[test]
fn regression_set_enabled_false_hides_trail() {
  let mut trail = Trail2D::new(Color::WHITE, 5.0);

  // Initially enabled and visible
  assert!(trail.is_enabled());
  assert!(trail.is_visible());

  // Disable the trail
  trail.set_enabled(false);

  // Must be both disabled AND hidden
  assert!(!trail.is_enabled(), "Trail should be disabled");
  assert!(!trail.is_visible(), "Trail should be hidden when disabled");
}

/// REGRESSION: set_enabled(true) must show trails
///
/// When re-enabling a trail, it should become visible again.
#[test]
fn regression_set_enabled_true_shows_trail() {
  let mut trail = Trail2D::new(Color::WHITE, 5.0);

  // Disable then re-enable
  trail.set_enabled(false);
  trail.set_enabled(true);

  // Must be both enabled AND visible
  assert!(trail.is_enabled(), "Trail should be enabled");
  assert!(trail.is_visible(), "Trail should be visible when enabled");
}

/// REGRESSION: Trails must start enabled and visible
///
/// New trails should be immediately usable without extra configuration.
#[test]
fn regression_trail_starts_enabled_and_visible() {
  let trail = Trail2D::new(Color::WHITE, 5.0);

  assert!(trail.is_enabled(), "New trail should start enabled");
  assert!(trail.is_visible(), "New trail should start visible");
}

// ============================================================================
// API BEHAVIOR TESTS
// ============================================================================

/// set_recording() must NOT affect visibility
///
/// This allows fade-out effects where trail stops recording but keeps rendering.
#[test]
fn set_recording_does_not_affect_visibility() {
  let mut trail = Trail2D::new(Color::WHITE, 5.0);

  // Stop recording
  trail.set_recording(false);

  // Recording stopped but still visible
  assert!(!trail.is_enabled(), "Recording should be stopped");
  assert!(
    trail.is_visible(),
    "Should still be visible for fade-out effect"
  );

  // Re-enable recording
  trail.set_recording(true);
  assert!(trail.is_enabled(), "Recording should be active");
  assert!(trail.is_visible(), "Should still be visible");
}

/// set_visible() must NOT affect recording
///
/// This allows hiding a trail temporarily while keeping it recording.
#[test]
fn set_visible_does_not_affect_recording() {
  let mut trail = Trail2D::new(Color::WHITE, 5.0);

  // Hide but keep recording
  trail.set_visible(false);

  assert!(trail.is_enabled(), "Recording should still be active");
  assert!(!trail.is_visible(), "Trail should be hidden");

  // Show again
  trail.set_visible(true);
  assert!(trail.is_enabled(), "Recording should still be active");
  assert!(trail.is_visible(), "Trail should be visible");
}

/// Independent visibility and recording control
///
/// All four combinations should work correctly.
#[test]
fn visibility_and_recording_independent() {
  let mut trail = Trail2D::new(Color::WHITE, 5.0);

  // State 1: recording=true, visible=true (default)
  assert!(trail.is_enabled());
  assert!(trail.is_visible());

  // State 2: recording=false, visible=true (fade-out mode)
  trail.set_recording(false);
  assert!(!trail.is_enabled());
  assert!(trail.is_visible());

  // State 3: recording=false, visible=false (fully off)
  trail.set_visible(false);
  assert!(!trail.is_enabled());
  assert!(!trail.is_visible());

  // State 4: recording=true, visible=false (recording while hidden)
  trail.set_recording(true);
  assert!(trail.is_enabled());
  assert!(!trail.is_visible());

  // Back to State 1
  trail.set_visible(true);
  assert!(trail.is_enabled());
  assert!(trail.is_visible());
}

// ============================================================================
// POINT RECORDING TESTS
// ============================================================================

/// Points are only recorded when enabled
#[test]
fn points_recorded_only_when_enabled() {
  let mut trail = Trail2D::new(Color::WHITE, 5.0).with_min_distance(0.0);

  // Add points while enabled
  trail.update(0.0, 0.0, 0.016);
  trail.update(10.0, 10.0, 0.016);
  assert_eq!(trail.point_count(), 2);

  // Disable and try to add more
  trail.set_enabled(false);
  trail.update(20.0, 20.0, 0.016);
  trail.update(30.0, 30.0, 0.016);

  // Count should not increase
  assert_eq!(
    trail.point_count(),
    2,
    "Points should not be added when disabled"
  );
}

/// Points are recorded when using set_recording(true) even if visibility is false
#[test]
fn points_recorded_when_recording_enabled() {
  let mut trail = Trail2D::new(Color::WHITE, 5.0).with_min_distance(0.0);

  // Hide but keep recording
  trail.set_visible(false);

  trail.update(0.0, 0.0, 0.016);
  trail.update(10.0, 10.0, 0.016);

  assert_eq!(
    trail.point_count(),
    2,
    "Points should be recorded even when hidden"
  );
  assert!(!trail.is_visible(), "Trail should still be hidden");
}

/// clear() removes all points
#[test]
fn clear_removes_all_points() {
  let mut trail = Trail2D::new(Color::WHITE, 5.0).with_min_distance(0.0);

  trail.update(0.0, 0.0, 0.016);
  trail.update(10.0, 10.0, 0.016);
  trail.update(20.0, 20.0, 0.016);
  assert_eq!(trail.point_count(), 3);

  trail.clear();
  assert_eq!(trail.point_count(), 0, "clear() should remove all points");
}

/// min_distance prevents clustering
#[test]
fn min_distance_prevents_clustering() {
  let mut trail = Trail2D::new(Color::WHITE, 5.0).with_min_distance(10.0);

  trail.update(0.0, 0.0, 0.016);
  trail.update(5.0, 0.0, 0.016); // Only 5 pixels away - should NOT add
  trail.update(15.0, 0.0, 0.016); // 15 pixels from first - should add

  assert_eq!(
    trail.point_count(),
    2,
    "Points closer than min_distance should be skipped"
  );
}

// ============================================================================
// ECS INTEGRATION TESTS
// ============================================================================

/// Trail2D works as an ECS component
#[test]
fn trail_as_ecs_component() {
  let mut world = World::new();

  let trail = Trail2D::new(Color::RED, 3.0).with_length(20);

  let entity = world.spawn((Transform::from_xy(100.0, 100.0), trail)).id();

  // Can retrieve and modify
  {
    let mut trail = world.get_mut::<Trail2D>(entity).unwrap();
    trail.set_enabled(false);
  }

  // Changes persist
  let trail = world.get::<Trail2D>(entity).unwrap();
  assert!(!trail.is_enabled());
  assert!(!trail.is_visible());
}

/// Multiple entities with trails
#[test]
fn multiple_entities_with_trails() {
  let mut world = World::new();

  let e1 = world
    .spawn((Transform::from_xy(0.0, 0.0), Trail2D::new(Color::RED, 3.0)))
    .id();

  let e2 = world
    .spawn((
      Transform::from_xy(100.0, 100.0),
      Trail2D::new(Color::BLUE, 5.0),
    ))
    .id();

  // Disable one trail
  {
    let mut trail = world.get_mut::<Trail2D>(e1).unwrap();
    trail.set_enabled(false);
  }

  // Check they're independent
  let t1 = world.get::<Trail2D>(e1).unwrap();
  let t2 = world.get::<Trail2D>(e2).unwrap();

  assert!(!t1.is_enabled(), "First trail should be disabled");
  assert!(t2.is_enabled(), "Second trail should still be enabled");
}

// ============================================================================
// BUILDER PATTERN TESTS
// ============================================================================

/// Builder methods chain correctly
#[test]
fn builder_pattern_chaining() {
  let trail = Trail2D::new(Color::WHITE, 5.0)
    .with_length(30)
    .with_fade(true)
    .with_min_distance(2.0)
    .with_taper(0.5)
    .with_end_color(Color::new(255, 0, 0, 0));

  // Should compile and create valid trail
  assert!(trail.is_enabled());
  assert!(trail.is_visible());
}

/// Preset constructors work
#[test]
fn preset_constructors() {
  let bullet = Trail2D::bullet(Color::YELLOW);
  assert!(bullet.is_enabled());

  let swipe = Trail2D::swipe(Color::WHITE, 20.0);
  assert!(swipe.is_enabled());

  let particle = Trail2D::particle(Color::RED);
  assert!(particle.is_enabled());

  let laser = Trail2D::laser(Color::GREEN, 10.0);
  assert!(laser.is_enabled());
}

// ============================================================================
// EDGE CASES
// ============================================================================

/// Trail with zero points doesn't crash
#[test]
fn empty_trail_is_safe() {
  let trail = Trail2D::new(Color::WHITE, 5.0);

  assert_eq!(trail.point_count(), 0);
  // These should not panic
  let _ = trail.points();
  let _ = trail.is_enabled();
  let _ = trail.is_visible();
}

/// Trail handles rapid enable/disable toggling
#[test]
fn rapid_toggle_is_safe() {
  let mut trail = Trail2D::new(Color::WHITE, 5.0);

  for _ in 0..100 {
    trail.set_enabled(true);
    trail.set_enabled(false);
  }

  assert!(!trail.is_enabled());
  assert!(!trail.is_visible());
}

/// Trail visibility can be toggled while disabled
#[test]
fn visibility_toggle_while_disabled() {
  let mut trail = Trail2D::new(Color::WHITE, 5.0);

  trail.set_enabled(false);
  assert!(!trail.is_visible());

  // Can manually set visible even when disabled
  trail.set_visible(true);
  assert!(!trail.is_enabled(), "Should still be disabled");
  assert!(trail.is_visible(), "Should be manually visible");

  trail.set_visible(false);
  assert!(!trail.is_visible());
}
