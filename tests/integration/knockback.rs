//! Knockback system integration tests
//!
//! These tests verify that knockback works correctly with the ECS.
//!
//! REGRESSION CONTEXT:
//! - Knockback component was not being processed by the engine
//! - Knockback velocity was never applied to Transform or Velocity
//! - KnockbackStarted/KnockbackFinished events were never fired

use ryo2d::prelude::*;

// ============================================================================
// REGRESSION TESTS - Critical invariants that must never break
// ============================================================================

/// REGRESSION: Knockback must decay over time
///
/// Previously, Knockback was never updated by the engine, so strength
/// never decayed and entities stayed static.
#[test]
fn regression_knockback_decays() {
  let mut kb = Knockback::new(Vec2::new(1.0, 0.0), 400.0).with_decay_rate(4.0);

  assert_eq!(kb.strength(), 1.0, "Initial strength should be 1.0");
  assert!(kb.active, "Should be active initially");

  // Simulate engine update
  kb.update(0.25);

  // Strength should have decayed: 1.0 - 4.0 * 0.25 = 0.0
  assert!(
    kb.strength() <= 0.0,
    "Strength should decay to 0 after 0.25s with decay_rate 4.0"
  );
  assert!(!kb.active, "Should be inactive after full decay");
}

/// REGRESSION: Knockback velocity must be calculable
///
/// The engine uses current_velocity() to apply movement.
#[test]
fn regression_knockback_provides_velocity() {
  let kb = Knockback::new(Vec2::new(1.0, 0.0), 400.0);
  let (vx, vy) = kb.current_velocity();

  assert_eq!(vx, 400.0, "Velocity X should equal force at full strength");
  assert_eq!(vy, 0.0, "Velocity Y should be 0 for horizontal knockback");
}

/// REGRESSION: Knockback just_started flag must be set initially
///
/// The engine uses this flag to fire KnockbackStarted events.
#[test]
fn regression_knockback_just_started_flag() {
  let kb = Knockback::new(Vec2::new(1.0, 0.0), 400.0);
  assert!(kb.just_started, "just_started should be true on creation");
}

/// REGRESSION: Knockback just_finished flag must be set when done
///
/// The engine uses this flag to fire KnockbackFinished events.
/// Note: just_finished is a one-shot flag, true for only ONE frame.
#[test]
fn regression_knockback_just_finished_flag() {
  let mut kb = Knockback::new(Vec2::new(1.0, 0.0), 400.0).with_decay_rate(10.0);

  // With decay_rate 10.0, after 0.1s: strength = 1.0 - 10.0 * 0.1 = 0.0
  // So the first update should trigger just_finished
  kb.update(0.1);

  assert!(
    kb.just_finished,
    "just_finished should be true on the frame knockback completes"
  );
  assert!(!kb.active, "Should be inactive when finished");

  // The flag should clear on the next update (one-shot behavior)
  kb.update(0.1);
  assert!(
    !kb.just_finished,
    "just_finished should clear after one frame"
  );
}

// ============================================================================
// KNOCKBACK BUILDER TESTS
// ============================================================================

/// Test knockback creation with direction and force
#[test]
fn knockback_new() {
  let kb = Knockback::new(Vec2::new(-1.0, 0.0), 300.0);

  assert!(kb.active);
  assert_eq!(kb.force(), 300.0);
  assert!(kb.direction().x < 0.0, "Direction should be left");
}

/// Test knockback from angle
#[test]
fn knockback_from_angle() {
  use std::f32::consts::PI;

  // Angle 0 = right
  let kb = Knockback::from_angle(0.0, 100.0);
  assert!((kb.direction().x - 1.0).abs() < 0.01);

  // Angle PI = left
  let kb = Knockback::from_angle(PI, 100.0);
  assert!((kb.direction().x + 1.0).abs() < 0.01);
}

/// Test custom decay rate
#[test]
fn knockback_decay_rate() {
  let kb = Knockback::new(Vec2::new(1.0, 0.0), 400.0).with_decay_rate(8.0);
  assert_eq!(kb.decay_rate(), 8.0);
}

/// Test accumulation setting
#[test]
fn knockback_accumulation() {
  let kb = Knockback::new(Vec2::new(1.0, 0.0), 400.0).with_accumulate(false);
  assert!(!kb.accumulate);
}

// ============================================================================
// KNOCKBACK UPDATE TESTS
// ============================================================================

/// Test velocity decreases as strength decays
#[test]
fn knockback_velocity_decays_with_strength() {
  let mut kb = Knockback::new(Vec2::new(1.0, 0.0), 400.0).with_decay_rate(2.0);

  let (initial_vx, _) = kb.current_velocity();
  assert_eq!(initial_vx, 400.0);

  // Decay to 50% strength
  kb.update(0.25); // 1.0 - 2.0 * 0.25 = 0.5

  let (decayed_vx, _) = kb.current_velocity();
  // Velocity uses strength² for punchy feel: 0.5² = 0.25, so velocity = 100
  assert!(
    (decayed_vx - 100.0).abs() < 1.0,
    "Velocity should be ~100 at 50% strength, got {}",
    decayed_vx
  );
}

/// Test just_started clears after first update
#[test]
fn knockback_just_started_clears() {
  let mut kb = Knockback::new(Vec2::new(1.0, 0.0), 400.0);
  assert!(kb.just_started);

  kb.update(0.016);
  assert!(!kb.just_started, "just_started should clear after update");
}

/// Test stop() immediately ends knockback
#[test]
fn knockback_stop() {
  let mut kb = Knockback::new(Vec2::new(1.0, 0.0), 400.0);

  kb.stop();

  assert!(!kb.active);
  assert!(kb.just_finished);
  assert_eq!(kb.strength(), 0.0);
}

/// Test restart() reactivates knockback
#[test]
fn knockback_restart() {
  let mut kb = Knockback::new(Vec2::new(1.0, 0.0), 400.0);
  kb.stop();

  kb.restart();

  assert!(kb.active);
  assert!(kb.just_started);
  assert_eq!(kb.strength(), 1.0);
}

// ============================================================================
// KNOCKBACK ACCUMULATION TESTS
// ============================================================================

/// Test knockback accumulates when enabled
#[test]
fn knockback_accumulates_force() {
  let mut kb = Knockback::new(Vec2::new(1.0, 0.0), 100.0);

  // Apply more force in same direction
  kb.push(Vec2::new(1.0, 0.0), 100.0);

  // Force should accumulate (capped at 2000)
  assert!(kb.force() > 100.0, "Force should accumulate");
}

/// Test knockback blends directions when accumulating
#[test]
fn knockback_blends_directions() {
  let mut kb = Knockback::new(Vec2::new(1.0, 0.0), 100.0);

  // Apply force at 90 degrees
  kb.push(Vec2::new(0.0, 1.0), 100.0);

  // Direction should be blended (roughly 45 degrees)
  let dir = kb.direction();
  assert!(dir.x > 0.3 && dir.x < 0.8);
  assert!(dir.y > 0.3 && dir.y < 0.8);
}

/// Test knockback replaces when accumulation disabled
#[test]
fn knockback_replaces_when_no_accumulate() {
  let mut kb = Knockback::new(Vec2::new(1.0, 0.0), 100.0).with_accumulate(false);

  kb.push(Vec2::new(0.0, -1.0), 200.0);

  // Should be completely replaced
  let dir = kb.direction();
  assert!(dir.x.abs() < 0.01, "X should be ~0");
  assert!((dir.y + 1.0).abs() < 0.01, "Y should be ~-1");
  assert_eq!(kb.force(), 200.0);
}

// ============================================================================
// ECS INTEGRATION TESTS
// ============================================================================

/// Test Knockback with Transform in ECS
#[test]
fn knockback_with_transform_ecs() {
  let mut world = World::new();

  let entity = world
    .spawn((
      Transform::from_xy(100.0, 100.0),
      Knockback::new(Vec2::new(1.0, 0.0), 400.0),
    ))
    .id();

  assert!(world.get::<Transform>(entity).is_some());
  assert!(world.get::<Knockback>(entity).is_some());

  let kb = world.get::<Knockback>(entity).unwrap();
  assert!(kb.active);
  assert_eq!(kb.force(), 400.0);
}

/// Test Knockback with Velocity in ECS (physics entity)
#[test]
fn knockback_with_velocity_ecs() {
  let mut world = World::new();

  let entity = world
    .spawn((
      Transform::from_xy(100.0, 100.0),
      Velocity::new(0.0, 0.0),
      Knockback::new(Vec2::new(-1.0, 0.5), 300.0),
    ))
    .id();

  assert!(world.get::<Velocity>(entity).is_some());
  assert!(world.get::<Knockback>(entity).is_some());
}

/// Test multiple knockback entities
#[test]
fn multiple_knockback_entities() {
  let mut world = World::new();

  // Simple entity (Transform only)
  let e1 = world
    .spawn((
      Transform::from_xy(0.0, 0.0),
      Knockback::new(Vec2::new(1.0, 0.0), 400.0),
    ))
    .id();

  // Physics entity (with Velocity)
  let e2 = world
    .spawn((
      Transform::from_xy(100.0, 0.0),
      Velocity::new(0.0, 0.0),
      Knockback::new(Vec2::new(-1.0, 0.0), 300.0),
    ))
    .id();

  // Different decay rate
  let e3 = world
    .spawn((
      Transform::from_xy(200.0, 0.0),
      Knockback::new(Vec2::new(0.0, 1.0), 500.0).with_decay_rate(8.0),
    ))
    .id();

  // Verify all entities have knockback
  assert!(world.get::<Knockback>(e1).is_some());
  assert!(world.get::<Knockback>(e2).is_some());
  assert!(world.get::<Knockback>(e3).is_some());

  // Verify different configurations
  let kb1 = world.get::<Knockback>(e1).unwrap();
  let kb2 = world.get::<Knockback>(e2).unwrap();
  let kb3 = world.get::<Knockback>(e3).unwrap();

  assert_eq!(kb1.force(), 400.0);
  assert_eq!(kb2.force(), 300.0);
  assert_eq!(kb3.decay_rate(), 8.0);
}

// ============================================================================
// ENGINE SYSTEM
// ============================================================================

/// REGRESSION: the engine's fixed update moves a knocked-back entity and
/// reports the impulse starting and finishing
#[test]
fn the_fixed_update_drives_knockback_and_reports_it() {
  let (mut world, mut schedules) = crate::support::juice_world(60);
  let entity = world.spawn(Transform::from_xy(0.0, 0.0)).id();
  Knockback::apply(&mut world, entity, Vec2::new(1.0, 0.0), 300.0);

  crate::support::run_fixed(&mut world, &mut schedules, 1);
  let x = world
    .get::<Transform>(entity)
    .expect("transform")
    .translation
    .x;
  assert!(x > 0.0, "knockback moved the entity along +x, got {x}");
  assert_eq!(
    crate::support::messages::<KnockbackStarted>(&world),
    [KnockbackStarted { entity }]
  );

  crate::support::run_fixed(&mut world, &mut schedules, 120);
  assert_eq!(
    crate::support::messages::<KnockbackFinished>(&world),
    [KnockbackFinished { entity }]
  );
}
