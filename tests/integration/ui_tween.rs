//! UI Tween system integration tests
//!
//! These tests verify that UiTween works correctly with other UI subsystems
//! and can be used for practical UI animation scenarios.
//!
//! DESIGN CONTEXT:
//! - UiTween is a standalone retained-mode tween system for UI properties
//! - It reuses the Ease enum from the ECS tween system for consistency
//! - It provides type-safe interpolation for UI-specific types (Color, Rect, etc.)

use ryo2d::prelude::*;
use std::sync::Arc;

// ============================================================================
// LERP TRAIT INTEGRATION TESTS
// ============================================================================

/// Verify Lerp trait works correctly with UI Color type
#[test]
fn lerp_integrates_with_ui_color() {
  // Color constants from UI types
  let start = Color::HEALTH_RED;
  let end = Color::STAMINA_GREEN;

  let mid = Color::lerp(&start, &end, 0.5);

  // Should be a blend between the two game colors
  assert!(mid.r > 0);
  assert!(mid.g > 0);
}

/// Verify Lerp trait works correctly with UI Rect type
#[test]
fn lerp_integrates_with_ui_rect() {
  // Common UI pattern: expanding a panel
  let collapsed = Rect::new(100.0, 100.0, 200.0, 50.0);
  let expanded = Rect::new(100.0, 100.0, 200.0, 300.0);

  let mid = Rect::lerp(&collapsed, &expanded, 0.5);

  assert_eq!(mid.x, 100.0);
  assert_eq!(mid.y, 100.0);
  assert_eq!(mid.width, 200.0);
  assert!((mid.height - 175.0).abs() < 0.01);
}

// ============================================================================
// UI TWEEN + EASE INTEGRATION TESTS
// ============================================================================

/// Verify UiTween correctly uses Ease enum from ECS module
#[test]
fn ui_tween_uses_ecs_ease_enum() {
  // All easing functions should be available
  let eases = [
    Ease::Linear,
    Ease::OutQuad,
    Ease::InOutCubic,
    Ease::OutBack,
    Ease::OutElastic,
    Ease::OutBounce,
  ];

  for ease in eases {
    let tween = TweenFloat::new(0.0, 100.0, 1.0).with_ease(ease);
    assert!(tween.is_playing());
  }
}

/// Test that easing curves produce expected behavior in UI context
#[test]
fn ease_curves_behave_correctly_for_ui() {
  // OutBack overshoots - useful for "pop in" effects
  let mut out_back = TweenFloat::new(0.0, 100.0, 1.0).with_ease(Ease::OutBack);
  out_back.update(0.7);
  // OutBack should overshoot the target temporarily
  assert!(
    out_back.value() > 100.0,
    "OutBack should overshoot at t=0.7"
  );

  // OutBounce - useful for dropping/landing effects
  let mut out_bounce = TweenFloat::new(0.0, 100.0, 1.0).with_ease(Ease::OutBounce);
  out_bounce.update(1.0);
  assert!(
    (out_bounce.value() - 100.0).abs() < 0.01,
    "OutBounce should end at target"
  );
}

// ============================================================================
// PRACTICAL UI ANIMATION PATTERNS
// ============================================================================

/// Test fade-in animation pattern for UI panels
#[test]
fn pattern_fade_in_panel() {
  let mut alpha_tween = TweenFloat::new(0.0, 1.0, 0.3).with_ease(Ease::OutQuad);

  // Panel starts invisible
  assert_eq!(alpha_tween.value(), 0.0);

  // Animate to visible
  alpha_tween.update(0.3);
  assert!((alpha_tween.value() - 1.0).abs() < 0.01);
  assert!(alpha_tween.is_finished());
}

/// Test slide-in animation pattern for menus
#[test]
fn pattern_slide_in_menu() {
  // Menu slides in from off-screen left
  let mut slide_tween = TweenVec2::new((-300.0, 100.0), (50.0, 100.0), 0.5).with_ease(Ease::OutCubic);

  // Starts off-screen
  let (x, _y) = slide_tween.value();
  assert_eq!(x, -300.0);

  // Animate to final position
  slide_tween.update(0.5);
  let (x, y) = slide_tween.value();
  assert!((x - 50.0).abs() < 0.01);
  assert!((y - 100.0).abs() < 0.01);
}

/// Test button hover scale effect
#[test]
fn pattern_button_hover_scale() {
  // Button scales up on hover
  let mut scale_tween = TweenFloat::new(1.0, 1.1, 0.15).with_ease(Ease::OutQuad);

  scale_tween.update(0.15);
  assert!((scale_tween.value() - 1.1).abs() < 0.01);

  // Scale back down on unhover using retarget
  scale_tween.retarget(1.0);
  scale_tween.update(0.15);
  assert!((scale_tween.value() - 1.0).abs() < 0.01);
}

/// Test notification popup animation
#[test]
fn pattern_notification_popup() {
  // Notification pops in and floats up while fading
  let mut position = TweenVec2::new((400.0, 500.0), (400.0, 450.0), 1.0).with_ease(Ease::OutQuad);
  let mut alpha = TweenFloat::new(1.0, 0.0, 1.5)
    .with_ease(Ease::InQuad)
    .with_delay(0.5);

  // Update both
  position.update(0.5);
  alpha.update(0.5);

  // Position should be moving
  let (_x, y) = position.value();
  assert!(y < 500.0);

  // Alpha should still be full (in delay)
  assert!((alpha.value() - 1.0).abs() < 0.01);
}

/// Test pulsing indicator animation
#[test]
fn pattern_pulsing_indicator() {
  let mut pulse = TweenFloat::new(0.8, 1.2, 0.5)
    .with_ease(Ease::InOutSine)
    .with_loop(UiTweenLoop::PingPong);

  // Run several cycles
  for _ in 0..10 {
    pulse.update(0.1);
    let value = pulse.value();
    assert!(value >= 0.79 && value <= 1.21, "Pulse should stay in range");
  }

  // Should never finish
  assert!(!pulse.is_finished());
  assert!(pulse.is_playing());
}

/// Test color flash effect for damage/highlight
#[test]
fn pattern_color_flash() {
  let mut flash = TweenColor::new(Color::WHITE, Color::RED, 0.1)
    .with_ease(Ease::Linear)
    .with_loop(UiTweenLoop::PingPong);

  // Flash to red
  flash.update(0.1);
  let color = flash.value();
  assert_eq!(color.r, 255);

  // Flash back to white
  flash.update(0.1);
  let color = flash.value();
  assert_eq!(color.r, 255);
  assert_eq!(color.g, 255);
  assert_eq!(color.b, 255);
}

// ============================================================================
// STAGGERED ANIMATION TESTS
// ============================================================================

/// Test staggered menu item animations using delays
#[test]
fn pattern_staggered_menu_items() {
  let mut items: Vec<TweenFloat> = (0..5)
    .map(|i| {
      TweenFloat::new(-200.0, 0.0, 0.3)
        .with_ease(Ease::OutBack)
        .with_delay(i as f32 * 0.1)
    })
    .collect();

  // First item has no delay (i=0), so it starts immediately
  // Items 1-4 have delays of 0.1, 0.2, 0.3, 0.4 seconds

  // After 0.05s, first item is already animating (no delay), others waiting
  for item in &mut items {
    item.update(0.05);
  }
  assert!(items[0].value() > -200.0, "First item (no delay) should be animating");
  assert!(items[1].value() == -200.0, "Second item should still be in delay");

  // After 0.1s more (0.15s total), items 0 and 1 should be animating
  for item in &mut items {
    item.update(0.1);
  }
  assert!(items[0].value() > -200.0, "First item should be animating");
  assert!(items[1].value() > -200.0, "Second item should now be animating");
  assert!(items[2].value() == -200.0, "Third item should still be waiting");
}

// ============================================================================
// CALLBACK INTEGRATION TESTS
// ============================================================================

/// Test chaining animations with callbacks
#[test]
fn pattern_chained_animations() {
  use std::sync::atomic::{AtomicBool, Ordering};

  let phase2_started = Arc::new(AtomicBool::new(false));
  let phase2_clone = phase2_started.clone();

  let mut phase1 = TweenFloat::new(0.0, 100.0, 0.5).on_complete(move || {
    phase2_clone.store(true, Ordering::SeqCst);
  });

  // Phase 1 running
  phase1.update(0.25);
  assert!(!phase2_started.load(Ordering::SeqCst));

  // Phase 1 completes, triggers phase 2
  phase1.update(0.25);
  assert!(phase2_started.load(Ordering::SeqCst));
}

/// Test callback with looping tweens
#[test]
fn pattern_loop_counter() {
  use std::sync::atomic::{AtomicUsize, Ordering};

  let loop_count = Arc::new(AtomicUsize::new(0));
  let loop_clone = loop_count.clone();

  let mut tween = TweenFloat::new(0.0, 100.0, 0.2)
    .with_loop(UiTweenLoop::Loop)
    .on_complete(move || {
      loop_clone.fetch_add(1, Ordering::SeqCst);
    });

  // Each update call handles at most one loop completion
  // (prevents callback spam in a single frame)
  // To complete 5 loops, we need 5 separate update calls that each complete a loop
  for _ in 0..5 {
    tween.update(0.2);
  }

  assert_eq!(loop_count.load(Ordering::SeqCst), 5);
}

// ============================================================================
// RETARGETING TESTS (smooth interruption)
// ============================================================================

/// Test smooth retargeting for responsive UI
#[test]
fn pattern_smooth_retarget() {
  let mut tween = TweenFloat::new(0.0, 100.0, 1.0);

  // Start moving to 100
  tween.update(0.3);
  let mid_value = tween.value();
  assert!(mid_value > 0.0 && mid_value < 100.0);

  // User changes target to 200 - should continue smoothly from current
  tween.retarget(200.0);
  assert_eq!(*tween.start_value(), mid_value);
  assert_eq!(*tween.target_value(), 200.0);

  // Should still be playing, now towards new target
  tween.update(1.0);
  assert!((tween.value() - 200.0).abs() < 0.01);
}

/// Test retarget for hover state changes
#[test]
fn pattern_hover_retarget() {
  let mut scale = TweenFloat::new(1.0, 1.0, 0.15);

  // Mouse enters - scale up
  scale.retarget(1.1);
  scale.update(0.1);
  let during_hover = scale.value();
  assert!(during_hover > 1.0);

  // Mouse leaves mid-animation - scale back down
  scale.retarget(1.0);
  scale.update(0.15);
  assert!((scale.value() - 1.0).abs() < 0.01);
}

// ============================================================================
// TYPE ALIAS CONVENIENCE TESTS
// ============================================================================

/// Verify all type aliases work correctly
#[test]
fn type_aliases_work() {
  let _float: TweenFloat = UiTween::new(0.0, 1.0, 1.0);
  let _vec2: TweenVec2 = UiTween::new((0.0, 0.0), (100.0, 100.0), 1.0);
  let _color: TweenColor = UiTween::new(Color::BLACK, Color::WHITE, 1.0);
  let _rect: TweenRect = UiTween::new(
    Rect::new(0.0, 0.0, 10.0, 10.0),
    Rect::new(100.0, 100.0, 200.0, 200.0),
    1.0,
  );
}

// ============================================================================
// EDGE CASE INTEGRATION TESTS
// ============================================================================

/// Test very short duration tweens (for snappy UI)
#[test]
fn short_duration_tween() {
  let mut tween = TweenFloat::new(0.0, 100.0, 0.016); // One frame at 60fps

  tween.update(0.016);
  assert!(tween.is_finished());
  assert!((tween.value() - 100.0).abs() < 0.01);
}

/// Test very long duration tweens
#[test]
fn long_duration_tween() {
  let mut tween = TweenFloat::new(0.0, 100.0, 60.0); // 1 minute

  // After 30 seconds, should be at 50%
  tween.update(30.0);
  assert!((tween.value() - 50.0).abs() < 0.01);
  assert!(!tween.is_finished());
}

/// Test multiple small updates vs one large update
#[test]
fn update_granularity_consistency() {
  let mut tween_many = TweenFloat::new(0.0, 100.0, 1.0);
  let mut tween_one = TweenFloat::new(0.0, 100.0, 1.0);

  // Many small updates
  for _ in 0..60 {
    tween_many.update(1.0 / 60.0);
  }

  // One large update
  tween_one.update(1.0);

  // Both should end at approximately the same value
  assert!(
    (tween_many.value() - tween_one.value()).abs() < 1.0,
    "Update granularity should not significantly affect final value"
  );
}
