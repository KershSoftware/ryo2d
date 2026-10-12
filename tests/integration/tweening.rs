//! Tweening system integration tests
//!
//! These tests verify that tweens work correctly with the ECS and rendering systems.
//!
//! REGRESSION CONTEXT:
//! - TweenSequence was not being processed by the engine (only Tween was)
//! - Color/alpha tweening values were calculated but never applied to Renderable
//! - Renderer used draw_rotated() instead of draw_rotated_tinted(), ignoring alpha/tint

use ryo2d::prelude::*;

// ============================================================================
// REGRESSION TESTS - Critical invariants that must never break
// ============================================================================

/// REGRESSION: TweenSequence must advance through tweens
///
/// Previously, TweenSequence components were never processed by the engine.
/// Only Tween components were updated, so sequences never progressed.
#[test]
fn regression_tween_sequence_advances() {
  let mut seq = TweenSequence::new()
    .then(Tween::to_position(100.0, 0.0, 0.5))
    .then(Tween::to_position(200.0, 0.0, 0.5));

  assert_eq!(seq.current_index(), 0, "Should start at first tween");
  assert!(!seq.is_finished(), "Should not be finished initially");

  // Advance to next tween
  seq.advance();
  assert_eq!(seq.current_index(), 1, "Should advance to second tween");

  // Advance past last tween
  seq.advance();
  assert!(seq.is_finished(), "Should be finished after all tweens");
}

/// REGRESSION: TweenSequence with loop should restart
///
/// Looping sequences should restart from the beginning after completing.
#[test]
fn regression_tween_sequence_loops() {
  let mut seq = TweenSequence::new()
    .then(Tween::to_position(100.0, 0.0, 0.5))
    .then(Tween::to_position(200.0, 0.0, 0.5))
    .with_loop();

  seq.advance(); // Complete first tween
  seq.advance(); // Complete second tween - should loop

  assert_eq!(seq.current_index(), 0, "Should loop back to first tween");
  assert!(!seq.is_finished(), "Looping sequence should never finish");
  assert!(seq.playing, "Should still be playing");
}

/// REGRESSION: Tween must capture start alpha from Renderable
///
/// Previously, tweens with alpha targets never captured the starting alpha
/// value from the Renderable component, so interpolation started from default (1.0).
#[test]
fn regression_tween_captures_start_alpha() {
  let mut tween = Tween::fade_to(0.0, 1.0);
  let mut transform = Transform::from_xy(0.0, 0.0);

  // Simulate Renderable with initial alpha of 0.5
  tween.capture_start_renderable(0.5, Color::WHITE);

  // Update to 50% progress
  tween.update(0.5, &mut transform);

  // Alpha should interpolate from 0.5 to 0.0
  // At 50%: 0.5 + (0.0 - 0.5) * 0.5 = 0.25
  let alpha = tween.current_alpha().expect("Should have alpha");
  assert!(
    (alpha - 0.25).abs() < 0.01,
    "Alpha should be ~0.25, got {}",
    alpha
  );
}

/// REGRESSION: Tween must capture start tint from Renderable
///
/// Previously, tweens with tint targets never captured the starting tint
/// value from the Renderable component, so interpolation started from WHITE.
#[test]
fn regression_tween_captures_start_tint() {
  let mut tween = Tween::tint_to(Color::RED, 1.0);
  let mut transform = Transform::from_xy(0.0, 0.0);

  // Simulate Renderable with initial tint of blue
  tween.capture_start_renderable(1.0, Color::BLUE);

  // Update to 50% progress
  tween.update(0.5, &mut transform);

  // Tint should interpolate from BLUE to RED
  let tint = tween.current_tint().expect("Should have tint");

  // BLUE (0, 0, 255) -> RED (255, 0, 0)
  // At 50%: R = 0 + (255-0)*0.5 = 128, B = 255 + (0-255)*0.5 = 128
  assert!(
    (tint.r as i32 - 128).abs() <= 1,
    "R should be ~128, got {}",
    tint.r
  );
  assert_eq!(tint.g, 0, "G should be 0");
  assert!(
    (tint.b as i32 - 128).abs() <= 1,
    "B should be ~128, got {}",
    tint.b
  );
}

/// REGRESSION: Renderable default alpha must be 1.0 (fully opaque)
///
/// This ensures sprites are visible by default.
#[test]
fn regression_renderable_default_alpha() {
  let sprite = Sprite::solid_rect(10, 10, 255, 255, 255, 255);
  let handle = SpriteHandle::from_sprite(sprite, "test");
  let renderable = Renderable::sprite(handle);

  assert_eq!(
    renderable.alpha, 1.0,
    "Default alpha must be 1.0 (fully opaque)"
  );
}

/// REGRESSION: Renderable default tint must be WHITE (no tinting)
///
/// WHITE tint means no color modification to the sprite.
#[test]
fn regression_renderable_default_tint() {
  let sprite = Sprite::solid_rect(10, 10, 255, 255, 255, 255);
  let handle = SpriteHandle::from_sprite(sprite, "test");
  let renderable = Renderable::sprite(handle);

  assert_eq!(
    renderable.tint,
    Color::WHITE,
    "Default tint must be WHITE (no tinting)"
  );
}

/// REGRESSION: Renderable alpha/tint can be modified
///
/// The rendering system must use these values when drawing.
#[test]
fn regression_renderable_alpha_tint_modifiable() {
  let sprite = Sprite::solid_rect(10, 10, 255, 255, 255, 255);
  let handle = SpriteHandle::from_sprite(sprite, "test");
  let mut renderable = Renderable::sprite(handle);

  renderable.alpha = 0.5;
  renderable.tint = Color::RED;

  assert_eq!(renderable.alpha, 0.5, "Alpha should be modifiable");
  assert_eq!(renderable.tint, Color::RED, "Tint should be modifiable");
}

// ============================================================================
// TWEEN BUILDER TESTS
// ============================================================================

/// Test alpha tween builder methods
#[test]
fn tween_alpha_builders() {
  let fade_out = Tween::fade_out(1.0);
  assert_eq!(fade_out.target_alpha, Some(0.0));

  let fade_in = Tween::fade_in(1.0);
  assert_eq!(fade_in.target_alpha, Some(1.0));

  let fade_to = Tween::fade_to(0.75, 1.0);
  assert_eq!(fade_to.target_alpha, Some(0.75));
}

/// Test tint tween builder methods
#[test]
fn tween_tint_builders() {
  let tint = Tween::tint_to(Color::GREEN, 1.0);
  assert_eq!(tint.target_tint, Some(Color::GREEN));
}

/// Test combined position and alpha tween
#[test]
fn tween_combined_position_alpha() {
  let tween = Tween::new(1.0).to_x(100.0).to_y(200.0).to_alpha(0.5);

  assert_eq!(tween.target_x, Some(100.0));
  assert_eq!(tween.target_y, Some(200.0));
  assert_eq!(tween.target_alpha, Some(0.5));
  assert!(tween.has_color_targets());
}

/// Test has_color_targets detection
#[test]
fn tween_has_color_targets() {
  let position_only = Tween::to_position(100.0, 100.0, 1.0);
  assert!(!position_only.has_color_targets());

  let with_alpha = Tween::new(1.0).to_alpha(0.5);
  assert!(with_alpha.has_color_targets());

  let with_tint = Tween::new(1.0).to_tint(Color::RED);
  assert!(with_tint.has_color_targets());
}

// ============================================================================
// TWEEN UPDATE TESTS
// ============================================================================

/// Test alpha interpolation during tween update
#[test]
fn tween_alpha_interpolation() {
  let mut tween = Tween::fade_to(0.0, 1.0);
  let mut transform = Transform::from_xy(0.0, 0.0);

  // Capture start from "full opacity"
  tween.capture_start_renderable(1.0, Color::WHITE);

  // At start (0% progress)
  let alpha = tween.current_alpha().expect("Should have alpha");
  assert!(
    (alpha - 1.0).abs() < 0.01,
    "At start, alpha should be 1.0, got {}",
    alpha
  );

  // Update to 25%
  tween.update(0.25, &mut transform);
  let alpha = tween.current_alpha().expect("Should have alpha");
  assert!(
    (alpha - 0.75).abs() < 0.01,
    "At 25%, alpha should be ~0.75, got {}",
    alpha
  );

  // Update to 100%
  tween.update(0.75, &mut transform);
  let alpha = tween.current_alpha().expect("Should have alpha");
  assert!(
    (alpha - 0.0).abs() < 0.01,
    "At 100%, alpha should be ~0.0, got {}",
    alpha
  );
}

/// Test tint interpolation during tween update
#[test]
fn tween_tint_interpolation() {
  let mut tween = Tween::tint_to(Color::new(0, 0, 0, 255), 1.0);
  let mut transform = Transform::from_xy(0.0, 0.0);

  // Capture start from WHITE
  tween.capture_start_renderable(1.0, Color::WHITE);

  // Update to 50%
  tween.update(0.5, &mut transform);
  let tint = tween.current_tint().expect("Should have tint");

  // WHITE (255,255,255) -> BLACK (0,0,0) at 50% = (128,128,128)
  assert!(
    (tint.r as i32 - 128).abs() <= 1,
    "At 50%, R should be ~128, got {}",
    tint.r
  );
  assert!(
    (tint.g as i32 - 128).abs() <= 1,
    "At 50%, G should be ~128, got {}",
    tint.g
  );
  assert!(
    (tint.b as i32 - 128).abs() <= 1,
    "At 50%, B should be ~128, got {}",
    tint.b
  );
}

// ============================================================================
// TWEEN SEQUENCE TESTS
// ============================================================================

/// Test TweenSequence creation and chaining
#[test]
fn tween_sequence_creation() {
  let seq = TweenSequence::new()
    .then(Tween::to_position(100.0, 0.0, 0.5))
    .then(Tween::to_position(200.0, 0.0, 0.5))
    .then(Tween::to_position(300.0, 0.0, 0.5));

  assert_eq!(seq.len(), 3);
  assert!(!seq.is_empty());
  assert_eq!(seq.current_index(), 0);
}

/// Test TweenSequence current tween access
#[test]
fn tween_sequence_current_access() {
  let seq = TweenSequence::new()
    .then(Tween::to_position(100.0, 0.0, 0.5))
    .then(Tween::to_position(200.0, 0.0, 0.5));

  let current = seq.current().expect("Should have current tween");
  assert_eq!(current.target_x, Some(100.0));
}

/// Test TweenSequence pause/resume
#[test]
fn tween_sequence_pause_resume() {
  let mut seq = TweenSequence::new().then(Tween::to_position(100.0, 0.0, 0.5));

  assert!(seq.playing);

  seq.pause();
  assert!(!seq.playing);

  seq.resume();
  assert!(seq.playing);
}

/// Test TweenSequence restart
#[test]
fn tween_sequence_restart() {
  let mut seq = TweenSequence::new()
    .then(Tween::to_position(100.0, 0.0, 0.5))
    .then(Tween::to_position(200.0, 0.0, 0.5));

  seq.advance();
  assert_eq!(seq.current_index(), 1);

  seq.restart();
  assert_eq!(seq.current_index(), 0);
  assert!(seq.playing);
}

// ============================================================================
// RENDERABLE BUILDER TESTS
// ============================================================================

/// Test Renderable with_alpha builder
#[test]
fn renderable_with_alpha() {
  let sprite = Sprite::solid_rect(10, 10, 255, 255, 255, 255);
  let handle = SpriteHandle::from_sprite(sprite, "test");
  let renderable = Renderable::sprite(handle).with_alpha(0.5);

  assert_eq!(renderable.alpha, 0.5);
}

/// Test Renderable with_tint builder
#[test]
fn renderable_with_tint() {
  let sprite = Sprite::solid_rect(10, 10, 255, 255, 255, 255);
  let handle = SpriteHandle::from_sprite(sprite, "test");
  let renderable = Renderable::sprite(handle).with_tint(Color::GREEN);

  assert_eq!(renderable.tint, Color::GREEN);
}

// ============================================================================
// ECS INTEGRATION TESTS
// ============================================================================

/// Test Tween and Renderable together in ECS
#[test]
fn tween_renderable_ecs_integration() {
  let mut world = World::new();

  let sprite = Sprite::solid_rect(10, 10, 255, 255, 255, 255);
  let handle = SpriteHandle::from_sprite(sprite, "test");

  let entity = world
    .spawn((
      Transform::from_xy(0.0, 0.0),
      Renderable::sprite(handle).with_alpha(0.8),
      Tween::fade_to(0.2, 1.0),
    ))
    .id();

  // Verify all components exist
  assert!(world.get::<Transform>(entity).is_some());
  assert!(world.get::<Renderable>(entity).is_some());
  assert!(world.get::<Tween>(entity).is_some());

  // Verify initial values
  let renderable = world.get::<Renderable>(entity).unwrap();
  assert_eq!(renderable.alpha, 0.8);

  let tween = world.get::<Tween>(entity).unwrap();
  assert_eq!(tween.target_alpha, Some(0.2));
}

/// Test TweenSequence with Renderable in ECS
#[test]
fn tween_sequence_renderable_ecs_integration() {
  let mut world = World::new();

  let sprite = Sprite::solid_rect(10, 10, 255, 255, 255, 255);
  let handle = SpriteHandle::from_sprite(sprite, "test");

  let entity = world
    .spawn((
      Transform::from_xy(0.0, 0.0),
      Renderable::sprite(handle),
      TweenSequence::new()
        .then(Tween::fade_to(0.5, 0.5))
        .then(Tween::tint_to(Color::RED, 0.5))
        .with_loop(),
    ))
    .id();

  // Verify components exist
  assert!(world.get::<TweenSequence>(entity).is_some());

  let seq = world.get::<TweenSequence>(entity).unwrap();
  assert_eq!(seq.len(), 2);
  assert!(seq.loop_sequence);
}

/// Test multiple entities with different tween types
#[test]
fn multiple_tween_entities() {
  let mut world = World::new();

  let sprite = Sprite::solid_rect(10, 10, 255, 255, 255, 255);
  let handle1 = SpriteHandle::from_sprite(sprite.clone(), "test1");
  let handle2 = SpriteHandle::from_sprite(sprite.clone(), "test2");
  let handle3 = SpriteHandle::from_sprite(sprite, "test3");

  // Entity with Tween (alpha)
  let e1 = world
    .spawn((
      Transform::from_xy(0.0, 0.0),
      Renderable::sprite(handle1),
      Tween::fade_out(1.0),
    ))
    .id();

  // Entity with Tween (tint)
  let e2 = world
    .spawn((
      Transform::from_xy(100.0, 0.0),
      Renderable::sprite(handle2),
      Tween::tint_to(Color::BLUE, 1.0),
    ))
    .id();

  // Entity with TweenSequence
  let e3 = world
    .spawn((
      Transform::from_xy(200.0, 0.0),
      Renderable::sprite(handle3),
      TweenSequence::new()
        .then(Tween::to_position(300.0, 0.0, 0.5))
        .then(Tween::fade_to(0.5, 0.5)),
    ))
    .id();

  // Verify all entities exist with correct components
  assert!(world.get::<Tween>(e1).is_some());
  assert!(world.get::<Tween>(e2).is_some());
  assert!(world.get::<TweenSequence>(e3).is_some());

  // Verify Tween types are correct
  let t1 = world.get::<Tween>(e1).unwrap();
  assert_eq!(t1.target_alpha, Some(0.0));

  let t2 = world.get::<Tween>(e2).unwrap();
  assert_eq!(t2.target_tint, Some(Color::BLUE));
}

// ============================================================================
// ENGINE SYSTEM
// ============================================================================

/// The engine's fixed update runs a tween to its end and reports it
#[test]
fn the_fixed_update_drives_tweens_and_reports_them() {
  // Ten 0.1 s steps cover the whole one-second tween
  let (mut world, mut schedules) = crate::support::juice_world(10);
  let entity = world
    .spawn((
      Transform::from_xy(0.0, 0.0),
      Tween::to_position(100.0, 0.0, 1.0),
    ))
    .id();

  crate::support::run_fixed(&mut world, &mut schedules, 10);

  let x = world
    .get::<Transform>(entity)
    .expect("transform")
    .translation
    .x;
  assert!(
    (x - 100.0).abs() < 1e-3,
    "the tween ended at x = 100, got {x}"
  );
  assert_eq!(
    crate::support::messages::<TweenStarted>(&world),
    [TweenStarted { entity }]
  );
  assert_eq!(
    crate::support::messages::<TweenCompleted>(&world),
    [TweenCompleted {
      entity,
      finished: true
    }]
  );
}
