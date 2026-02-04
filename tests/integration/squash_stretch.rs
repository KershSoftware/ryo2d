//! SquashStretch system integration tests
//!
//! These tests verify that squash/stretch effects work correctly with the ECS.
//!
//! REGRESSION CONTEXT:
//! - SquashStretch component was not being processed by the engine
//! - Scale changes were never applied to Transform
//! - SquashStretchStarted/SquashStretchFinished events were never fired

use ryo2d::prelude::*;

// ============================================================================
// REGRESSION TESTS - Critical invariants that must never break
// ============================================================================

/// REGRESSION: SquashStretch must decay over time
///
/// Previously, SquashStretch was never updated by the engine, so strength
/// never decayed and entities stayed deformed.
#[test]
fn regression_squash_stretch_decays() {
  let mut ss = SquashStretch::new(0.3).with_decay_rate(4.0);

  assert_eq!(ss.strength(), 1.0, "Initial strength should be 1.0");
  assert!(ss.active, "Should be active initially");

  // Simulate engine update
  ss.update(0.25);

  // Strength should have decayed: 1.0 - 4.0 * 0.25 = 0.0
  assert!(
    ss.strength() <= 0.0,
    "Strength should decay to 0 after 0.25s with decay_rate 4.0"
  );
  assert!(!ss.active, "Should be inactive after full decay");
}

/// REGRESSION: SquashStretch must provide scale values
///
/// The engine uses current_scale() to apply deformation to Transform.
#[test]
fn regression_squash_stretch_provides_scale() {
  let ss = SquashStretch::new(0.3)
    .with_base_scale(1.0, 1.0)
    .with_axis(SquashAxis::Y);

  let (scale_x, scale_y) = ss.current_scale();

  // At full strength with 0.3 intensity on Y axis:
  // scale_y = 1.0 * (1.0 - 0.3) = 0.7 (squashed)
  // scale_x = 1.0 * (1.0 + 0.3) = 1.3 (stretched to preserve volume)
  assert!(
    (scale_x - 1.3).abs() < 0.01,
    "Scale X should be ~1.3, got {}",
    scale_x
  );
  assert!(
    (scale_y - 0.7).abs() < 0.01,
    "Scale Y should be ~0.7, got {}",
    scale_y
  );
}

/// REGRESSION: SquashStretch just_started flag must be set initially
///
/// The engine uses this flag to fire SquashStretchStarted events.
#[test]
fn regression_squash_stretch_just_started_flag() {
  let ss = SquashStretch::new(0.3);
  assert!(ss.just_started, "just_started should be true on creation");
}

/// REGRESSION: SquashStretch just_finished flag must be set when done
///
/// The engine uses this flag to fire SquashStretchFinished events.
/// Note: just_finished is a one-shot flag, true for only ONE frame.
#[test]
fn regression_squash_stretch_just_finished_flag() {
  let mut ss = SquashStretch::new(0.3).with_decay_rate(10.0);

  // With decay_rate 10.0, after 0.1s: strength = 1.0 - 10.0 * 0.1 = 0.0
  // So the first update should trigger just_finished
  ss.update(0.1);

  assert!(
    ss.just_finished,
    "just_finished should be true on the frame effect completes"
  );
  assert!(!ss.active, "Should be inactive when finished");

  // The flag should clear on the next update (one-shot behavior)
  ss.update(0.1);
  assert!(
    !ss.just_finished,
    "just_finished should clear after one frame"
  );
}

// ============================================================================
// SQUASH/STRETCH BUILDER TESTS
// ============================================================================

/// Test squash/stretch creation with intensity
#[test]
fn squash_stretch_new() {
  let ss = SquashStretch::new(0.4);

  assert!(ss.active);
  assert_eq!(ss.intensity(), 0.4);
  assert_eq!(ss.strength(), 1.0);
}

/// Test custom decay rate
#[test]
fn squash_stretch_decay_rate() {
  let ss = SquashStretch::new(0.3).with_decay_rate(12.0);
  assert_eq!(ss.decay_rate(), 12.0);
}

/// Test axis configuration
#[test]
fn squash_stretch_axis() {
  let ss_y = SquashStretch::new(0.3).with_axis(SquashAxis::Y);
  let ss_x = SquashStretch::new(0.3).with_axis(SquashAxis::X);

  // Y-axis squash (default) - squash Y, stretch X
  let (sx, sy) = ss_y.with_base_scale(1.0, 1.0).current_scale();
  assert!(sx > 1.0, "X should stretch");
  assert!(sy < 1.0, "Y should squash");

  // X-axis squash - squash X, stretch Y
  let (sx, sy) = ss_x.with_base_scale(1.0, 1.0).current_scale();
  assert!(sx < 1.0, "X should squash");
  assert!(sy > 1.0, "Y should stretch");
}

/// Test volume preservation setting
#[test]
fn squash_stretch_preserve_volume() {
  let with_volume = SquashStretch::new(0.3)
    .with_base_scale(1.0, 1.0)
    .with_preserve_volume(true);

  let without_volume = SquashStretch::new(0.3)
    .with_base_scale(1.0, 1.0)
    .with_preserve_volume(false);

  let (sx_with, _) = with_volume.current_scale();
  let (sx_without, _) = without_volume.current_scale();

  assert!(sx_with > 1.0, "With volume preservation, X should stretch");
  assert!(
    (sx_without - 1.0).abs() < 0.01,
    "Without volume preservation, X should stay at base"
  );
}

// ============================================================================
// SQUASH/STRETCH UPDATE TESTS
// ============================================================================

/// Test scale changes as strength decays
#[test]
fn squash_stretch_scale_decays_with_strength() {
  let mut ss = SquashStretch::new(0.4)
    .with_base_scale(1.0, 1.0)
    .with_decay_rate(2.0);

  let (_, initial_y) = ss.current_scale();
  // At full strength: 1.0 - 0.4 = 0.6
  assert!((initial_y - 0.6).abs() < 0.01);

  // Decay to 50% strength
  ss.update(0.25); // 1.0 - 2.0 * 0.25 = 0.5

  let (_, decayed_y) = ss.current_scale();
  // At 50% strength, t = 0.25 (squared), deform = 0.4 * 0.25 = 0.1
  // scale_y = 1.0 - 0.1 = 0.9
  assert!(
    (decayed_y - 0.9).abs() < 0.01,
    "Scale Y should be ~0.9 at 50% strength, got {}",
    decayed_y
  );
}

/// Test just_started clears after first update
#[test]
fn squash_stretch_just_started_clears() {
  let mut ss = SquashStretch::new(0.3);
  assert!(ss.just_started);

  ss.update(0.016);
  assert!(!ss.just_started, "just_started should clear after update");
}

/// Test stop() immediately ends effect
#[test]
fn squash_stretch_stop() {
  let mut ss = SquashStretch::new(0.3).with_base_scale(1.0, 1.0);

  ss.stop();

  assert!(!ss.active);
  assert!(ss.just_finished);
  assert_eq!(ss.strength(), 0.0);

  // Scale should return to base
  let (sx, sy) = ss.current_scale();
  assert!((sx - 1.0).abs() < 0.01);
  assert!((sy - 1.0).abs() < 0.01);
}

/// Test apply() reactivates effect
#[test]
fn squash_stretch_apply() {
  let mut ss = SquashStretch::new(0.3);
  ss.stop();

  ss.apply(0.5);

  assert!(ss.active);
  assert!(ss.just_started);
  assert_eq!(ss.strength(), 1.0);
  assert_eq!(ss.intensity(), 0.5);
}

// ============================================================================
// ECS INTEGRATION TESTS
// ============================================================================

/// Test SquashStretch with Transform in ECS
#[test]
fn squash_stretch_with_transform_ecs() {
  let mut world = World::new();

  let entity = world.spawn((
    Transform::new(100.0, 100.0),
    SquashStretch::new(0.3),
  ));

  assert!(world.get::<Transform>(entity).is_some());
  assert!(world.get::<SquashStretch>(entity).is_some());

  let ss = world.get::<SquashStretch>(entity).unwrap();
  assert!(ss.active);
  assert_eq!(ss.intensity(), 0.3);
}

/// Test multiple squash/stretch entities
#[test]
fn multiple_squash_stretch_entities() {
  let mut world = World::new();

  // Landing impact (Y-axis)
  let e1 = world.spawn((
    Transform::new(0.0, 0.0),
    SquashStretch::new(0.3).with_axis(SquashAxis::Y),
  ));

  // Wall hit (X-axis)
  let e2 = world.spawn((
    Transform::new(100.0, 0.0),
    SquashStretch::new(0.4).with_axis(SquashAxis::X),
  ));

  // Fast recovery
  let e3 = world.spawn((
    Transform::new(200.0, 0.0),
    SquashStretch::new(0.5).with_decay_rate(12.0),
  ));

  // Verify all entities have squash/stretch
  assert!(world.get::<SquashStretch>(e1).is_some());
  assert!(world.get::<SquashStretch>(e2).is_some());
  assert!(world.get::<SquashStretch>(e3).is_some());

  // Verify different configurations
  let ss1 = world.get::<SquashStretch>(e1).unwrap();
  let ss2 = world.get::<SquashStretch>(e2).unwrap();
  let ss3 = world.get::<SquashStretch>(e3).unwrap();

  assert_eq!(ss1.intensity(), 0.3);
  assert_eq!(ss2.intensity(), 0.4);
  assert_eq!(ss3.decay_rate(), 12.0);
}

/// Test base scale capture
#[test]
fn squash_stretch_captures_base_scale() {
  let mut ss = SquashStretch::new(0.3);

  // Initially no base scale captured
  let (bx, by) = ss.base_scale();
  assert!((bx - 1.0).abs() < 0.01); // Default
  assert!((by - 1.0).abs() < 0.01);

  // Capture from transform
  ss.capture_base_scale(2.0, 1.5);

  let (bx, by) = ss.base_scale();
  assert!((bx - 2.0).abs() < 0.01);
  assert!((by - 1.5).abs() < 0.01);

  // Second capture should not override
  ss.capture_base_scale(3.0, 3.0);
  let (bx, by) = ss.base_scale();
  assert!((bx - 2.0).abs() < 0.01, "Base scale X should not change");
  assert!((by - 1.5).abs() < 0.01, "Base scale Y should not change");
}
