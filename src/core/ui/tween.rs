//! UI Tweening System
//!
//! A standalone tweening system for UI widget properties with easing functions
//! and callbacks. Follows a retained-mode pattern where state is stored and
//! updated each frame.
//!
//! # Example
//!
//! ```ignore
//! use ryo2d::prelude::*;
//!
//! // Basic fade animation
//! let mut fade = TweenFloat::new(0.0, 1.0, 0.5)
//!     .with_ease(Ease::OutQuad);
//!
//! // Position slide with delay
//! let mut slide = TweenVec2::new((0.0, 0.0), (100.0, 50.0), 0.3)
//!     .with_ease(Ease::OutBack)
//!     .with_delay(0.2);
//!
//! // Looping pulse effect
//! let mut pulse = TweenFloat::new(1.0, 1.2, 0.4)
//!     .with_ease(Ease::InOutSine)
//!     .with_loop(UiTweenLoop::PingPong);
//!
//! // In update():
//! fade.update(dt);
//! slide.update(dt);
//! pulse.update(dt);
//!
//! // In render_ui():
//! let alpha = (fade.value() * 255.0) as u8;
//! let (x, y) = slide.value();
//! ```

use std::sync::Arc;

// Re-export Ease from ECS module (which re-exports it from tween)
pub use crate::core::ecs::Ease;

use super::{Color, Rect};

// Lerp trait and impls (f32, (f32,f32), Color, Rect, Vec2) live in the shared
// ECS module so they can be used by both UI tweens and ECS property tweens.
pub use crate::core::ecs::Lerp;

/// Loop behavior for UI tweens.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum UiTweenLoop {
  /// Play once and stop (default)
  #[default]
  Once,
  /// Loop forever
  Loop,
  /// Play forward then backward, repeat forever
  PingPong,
  /// Loop a specific number of times
  Times(u32),
}

/// Type alias for tween callbacks.
pub type UiTweenCallback = Arc<dyn Fn(f32) + Send + Sync>;

/// Generic UI tween for interpolating values over time.
///
/// `UiTween<T>` provides smooth interpolation between a start and end value
/// using configurable easing functions, delays, and loop modes.
///
/// # Type Parameters
///
/// * `T` - The value type to interpolate. Must implement `Lerp` and `Clone`.
///
/// # Example
///
/// ```ignore
/// let mut tween = UiTween::new(0.0, 100.0, 0.5)
///     .with_ease(Ease::OutQuad)
///     .on_complete(|_| println!("Done!"));
///
/// // Each frame:
/// tween.update(dt);
/// let current = tween.value(); // 0.0 -> 100.0 over 0.5 seconds
/// ```
pub struct UiTween<T: Lerp> {
  start: T,
  target: T,
  current: T,
  duration: f32,
  delay: f32,
  elapsed: f32,
  easing: Ease,
  loop_mode: UiTweenLoop,
  playing: bool,
  ping_pong_reverse: bool,
  loop_count: u32,
  on_start: Option<UiTweenCallback>,
  on_complete: Option<UiTweenCallback>,
  started: bool,
}

impl<T: Lerp> UiTween<T> {
  /// Create a new tween interpolating from `start` to `end` over `duration` seconds.
  pub fn new(start: T, end: T, duration: f32) -> Self {
    let current = start.clone();
    Self {
      start,
      target: end,
      current,
      duration,
      delay: 0.0,
      elapsed: 0.0,
      easing: Ease::Linear,
      loop_mode: UiTweenLoop::Once,
      playing: true,
      ping_pong_reverse: false,
      loop_count: 0,
      on_start: None,
      on_complete: None,
      started: false,
    }
  }

  /// Set the easing function.
  pub fn with_ease(mut self, ease: Ease) -> Self {
    self.easing = ease;
    self
  }

  /// Add a delay before the tween starts.
  pub fn with_delay(mut self, delay: f32) -> Self {
    self.delay = delay;
    self
  }

  /// Set the loop mode.
  pub fn with_loop(mut self, loop_mode: UiTweenLoop) -> Self {
    self.loop_mode = loop_mode;
    self
  }

  /// Set a callback to be called when the tween starts (after delay).
  ///
  /// For looping tweens, this is only called once at the very beginning.
  pub fn on_start<F>(mut self, callback: F) -> Self
  where
    F: Fn(f32) + Send + Sync + 'static,
  {
    self.on_start = Some(Arc::new(callback));
    self
  }

  /// Set a callback to be called when the tween completes.
  ///
  /// For looping tweens (`Loop`, `PingPong`), this is called each time
  /// a cycle completes. For `Times(n)`, it's called after each iteration.
  pub fn on_complete<F>(mut self, callback: F) -> Self
  where
    F: Fn(f32) + Send + Sync + 'static,
  {
    self.on_complete = Some(Arc::new(callback));
    self
  }

  /// Update the tween with the elapsed delta time.
  ///
  /// Call this every frame to advance the animation.
  pub fn update(&mut self, dt: f32) {
    if !self.playing {
      return;
    }

    // Handle delay
    if self.delay > 0.0 {
      self.delay -= dt;
      if self.delay > 0.0 {
        return;
      }
      // Delay finished, continue with remaining time
      let overshoot = -self.delay;
      self.delay = 0.0;

      // Fire on_start callback
      if !self.started {
        self.started = true;
        if let Some(ref cb) = self.on_start {
          cb(0.0);
        }
      }

      self.elapsed = overshoot;
    } else {
      // Fire on_start if this is the first update
      if !self.started {
        self.started = true;
        if let Some(ref cb) = self.on_start {
          cb(0.0);
        }
      }
      self.elapsed += dt;
    }

    // Calculate progress
    let raw_progress = if self.duration <= 0.0 {
      1.0
    } else {
      (self.elapsed / self.duration).clamp(0.0, 1.0)
    };

    // Handle ping-pong direction
    let progress = if self.ping_pong_reverse {
      1.0 - raw_progress
    } else {
      raw_progress
    };

    // Apply easing and interpolate
    let eased = self.easing.apply(progress);
    self.current = T::lerp(&self.start, &self.target, eased);

    // Handle completion
    if self.elapsed >= self.duration {
      match self.loop_mode {
        UiTweenLoop::Once => {
          self.playing = false;
          if let Some(ref cb) = self.on_complete {
            cb(1.0);
          }
        }
        UiTweenLoop::Loop => {
          self.elapsed -= self.duration;
          if let Some(ref cb) = self.on_complete {
            cb(1.0);
          }
        }
        UiTweenLoop::PingPong => {
          self.elapsed -= self.duration;
          self.ping_pong_reverse = !self.ping_pong_reverse;
          if let Some(ref cb) = self.on_complete {
            cb(1.0);
          }
        }
        UiTweenLoop::Times(n) => {
          self.loop_count += 1;
          if self.loop_count >= n {
            self.playing = false;
          } else {
            self.elapsed -= self.duration;
          }
          if let Some(ref cb) = self.on_complete {
            cb(1.0);
          }
        }
      }
    }
  }

  /// Get the current interpolated value.
  pub fn value(&self) -> T {
    self.current.clone()
  }

  /// Get the current progress (0.0 to 1.0).
  pub fn progress(&self) -> f32 {
    if self.duration <= 0.0 {
      1.0
    } else {
      (self.elapsed / self.duration).clamp(0.0, 1.0)
    }
  }

  /// Check if the tween has finished.
  ///
  /// Only meaningful for `Once` and `Times` modes.
  pub fn is_finished(&self) -> bool {
    match self.loop_mode {
      UiTweenLoop::Once => !self.playing && self.elapsed >= self.duration,
      UiTweenLoop::Times(n) => self.loop_count >= n,
      UiTweenLoop::Loop | UiTweenLoop::PingPong => false,
    }
  }

  /// Check if the tween is currently playing.
  pub fn is_playing(&self) -> bool {
    self.playing
  }

  /// Restart the tween from the beginning.
  pub fn restart(&mut self) {
    self.elapsed = 0.0;
    self.playing = true;
    self.ping_pong_reverse = false;
    self.loop_count = 0;
    self.started = false;
    self.current = self.start.clone();
  }

  /// Pause the tween.
  pub fn pause(&mut self) {
    self.playing = false;
  }

  /// Resume a paused tween.
  pub fn resume(&mut self) {
    self.playing = true;
  }

  /// Stop and reset the tween.
  pub fn stop(&mut self) {
    self.playing = false;
    self.elapsed = 0.0;
    self.started = false;
    self.ping_pong_reverse = false;
    self.loop_count = 0;
    self.current = self.start.clone();
  }

  /// Set a new target value while keeping the current value as the new start.
  ///
  /// Useful for chaining animations or responding to user input.
  pub fn retarget(&mut self, new_target: T) {
    self.start = self.current.clone();
    self.target = new_target;
    self.elapsed = 0.0;
    self.playing = true;
    self.started = false;
    self.ping_pong_reverse = false;
    self.loop_count = 0;
  }

  /// Get the start value.
  pub fn start_value(&self) -> &T {
    &self.start
  }

  /// Get the target value.
  pub fn target_value(&self) -> &T {
    &self.target
  }
}

impl<T: Lerp + std::fmt::Debug> std::fmt::Debug for UiTween<T> {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    f.debug_struct("UiTween")
      .field("start", &self.start)
      .field("target", &self.target)
      .field("current", &self.current)
      .field("duration", &self.duration)
      .field("delay", &self.delay)
      .field("elapsed", &self.elapsed)
      .field("easing", &self.easing)
      .field("loop_mode", &self.loop_mode)
      .field("playing", &self.playing)
      .field("on_start", &self.on_start.as_ref().map(|_| "Fn(f32)"))
      .field("on_complete", &self.on_complete.as_ref().map(|_| "Fn(f32)"))
      .finish()
  }
}

impl<T: Lerp> Clone for UiTween<T> {
  fn clone(&self) -> Self {
    Self {
      start: self.start.clone(),
      target: self.target.clone(),
      current: self.current.clone(),
      duration: self.duration,
      delay: self.delay,
      elapsed: self.elapsed,
      easing: self.easing,
      loop_mode: self.loop_mode,
      playing: self.playing,
      ping_pong_reverse: self.ping_pong_reverse,
      loop_count: self.loop_count,
      on_start: self.on_start.clone(),
      on_complete: self.on_complete.clone(),
      started: self.started,
    }
  }
}

/// Type alias for floating-point tweens.
pub type TweenFloat = UiTween<f32>;

/// Type alias for 2D vector tweens (position, size, etc.).
pub type TweenVec2 = UiTween<(f32, f32)>;

/// Type alias for color tweens.
pub type TweenColor = UiTween<Color>;

/// Type alias for rectangle tweens.
pub type TweenRect = UiTween<Rect>;

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn lerp_f32() {
    assert_eq!(f32::lerp(&0.0, &100.0, 0.0), 0.0);
    assert_eq!(f32::lerp(&0.0, &100.0, 0.5), 50.0);
    assert_eq!(f32::lerp(&0.0, &100.0, 1.0), 100.0);
  }

  #[test]
  fn lerp_vec2() {
    let start = (0.0, 0.0);
    let end = (100.0, 200.0);
    let mid = <(f32, f32)>::lerp(&start, &end, 0.5);
    assert_eq!(mid, (50.0, 100.0));
  }

  #[test]
  fn lerp_color() {
    let start = Color::BLACK;
    let end = Color::WHITE;
    let mid = Color::lerp(&start, &end, 0.5);
    assert_eq!(mid.r, 128);
    assert_eq!(mid.g, 128);
    assert_eq!(mid.b, 128);
  }

  #[test]
  fn lerp_rect() {
    let start = Rect::new(0.0, 0.0, 100.0, 100.0);
    let end = Rect::new(100.0, 100.0, 200.0, 200.0);
    let mid = Rect::lerp(&start, &end, 0.5);
    assert_eq!(mid.x, 50.0);
    assert_eq!(mid.y, 50.0);
    assert_eq!(mid.width, 150.0);
    assert_eq!(mid.height, 150.0);
  }

  #[test]
  fn tween_basic() {
    let mut tween = TweenFloat::new(0.0, 100.0, 1.0);

    // At start
    assert_eq!(tween.value(), 0.0);
    assert_eq!(tween.progress(), 0.0);
    assert!(tween.is_playing());
    assert!(!tween.is_finished());

    // Halfway
    tween.update(0.5);
    assert!((tween.value() - 50.0).abs() < 0.01);
    assert!((tween.progress() - 0.5).abs() < 0.01);

    // Complete
    tween.update(0.5);
    assert!((tween.value() - 100.0).abs() < 0.01);
    assert!(!tween.is_playing());
    assert!(tween.is_finished());
  }

  #[test]
  fn tween_with_delay() {
    let mut tween = TweenFloat::new(0.0, 100.0, 1.0).with_delay(0.5);

    // During delay
    tween.update(0.3);
    assert_eq!(tween.value(), 0.0);

    // After delay
    tween.update(0.3); // 0.1s into tween
    assert!(tween.value() > 0.0);
  }

  #[test]
  fn tween_with_easing() {
    let mut tween = TweenFloat::new(0.0, 100.0, 1.0).with_ease(Ease::InQuad);

    tween.update(0.5);
    // InQuad at t=0.5 gives 0.25, so value should be 25
    assert!((tween.value() - 25.0).abs() < 0.01);
  }

  #[test]
  fn tween_loop() {
    let mut tween = TweenFloat::new(0.0, 100.0, 1.0).with_loop(UiTweenLoop::Loop);

    tween.update(1.0);
    assert!(tween.is_playing()); // Still playing

    tween.update(0.5);
    assert!((tween.value() - 50.0).abs() < 0.01); // Back to interpolating
  }

  #[test]
  fn tween_ping_pong() {
    let mut tween = TweenFloat::new(0.0, 100.0, 1.0).with_loop(UiTweenLoop::PingPong);

    // Forward
    tween.update(1.0);
    assert!((tween.value() - 100.0).abs() < 0.01);

    // Backward
    tween.update(0.5);
    assert!((tween.value() - 50.0).abs() < 0.01);

    tween.update(0.5);
    assert!((tween.value() - 0.0).abs() < 0.01);
  }

  #[test]
  fn tween_times() {
    let mut tween = TweenFloat::new(0.0, 100.0, 1.0).with_loop(UiTweenLoop::Times(2));

    tween.update(1.0);
    assert!(tween.is_playing());

    tween.update(1.0);
    assert!(!tween.is_playing());
    assert!(tween.is_finished());
  }

  #[test]
  fn tween_restart() {
    let mut tween = TweenFloat::new(0.0, 100.0, 1.0);

    tween.update(1.0);
    assert!(tween.is_finished());

    tween.restart();
    assert!(tween.is_playing());
    assert!(!tween.is_finished());
    assert_eq!(tween.value(), 0.0);
  }

  #[test]
  fn tween_pause_resume() {
    let mut tween = TweenFloat::new(0.0, 100.0, 1.0);

    tween.update(0.5);
    assert!((tween.value() - 50.0).abs() < 0.01);

    tween.pause();
    tween.update(0.5);
    assert!((tween.value() - 50.0).abs() < 0.01); // No change

    tween.resume();
    tween.update(0.5);
    assert!((tween.value() - 100.0).abs() < 0.01);
  }

  #[test]
  fn tween_retarget() {
    let mut tween = TweenFloat::new(0.0, 100.0, 1.0);

    tween.update(0.5);
    assert!((tween.value() - 50.0).abs() < 0.01);

    tween.retarget(200.0);
    assert_eq!(*tween.start_value(), 50.0);
    assert_eq!(*tween.target_value(), 200.0);
  }

  #[test]
  fn tween_callback_on_complete() {
    use std::sync::atomic::{AtomicBool, Ordering};

    let completed = Arc::new(AtomicBool::new(false));
    let completed_clone = completed.clone();

    let mut tween = TweenFloat::new(0.0, 100.0, 1.0).on_complete(move |_| {
      completed_clone.store(true, Ordering::SeqCst);
    });

    tween.update(0.5);
    assert!(!completed.load(Ordering::SeqCst));

    tween.update(0.5);
    assert!(completed.load(Ordering::SeqCst));
  }

  #[test]
  fn tween_callback_on_start() {
    use std::sync::atomic::{AtomicBool, Ordering};

    let started = Arc::new(AtomicBool::new(false));
    let started_clone = started.clone();

    let mut tween = TweenFloat::new(0.0, 100.0, 1.0).on_start(move |_| {
      started_clone.store(true, Ordering::SeqCst);
    });

    assert!(!started.load(Ordering::SeqCst));

    tween.update(0.1);
    assert!(started.load(Ordering::SeqCst));
  }

  #[test]
  fn tween_vec2() {
    let mut tween = TweenVec2::new((0.0, 0.0), (100.0, 200.0), 1.0);

    tween.update(0.5);
    let (x, y) = tween.value();
    assert!((x - 50.0).abs() < 0.01);
    assert!((y - 100.0).abs() < 0.01);
  }

  #[test]
  fn tween_color() {
    let mut tween = TweenColor::new(Color::BLACK, Color::WHITE, 1.0);

    tween.update(0.5);
    let color = tween.value();
    assert_eq!(color.r, 128);
    assert_eq!(color.g, 128);
    assert_eq!(color.b, 128);
  }

  #[test]
  fn tween_rect() {
    let start = Rect::new(0.0, 0.0, 100.0, 50.0);
    let end = Rect::new(100.0, 100.0, 200.0, 100.0);
    let mut tween = TweenRect::new(start, end, 1.0);

    tween.update(0.5);
    let rect = tween.value();
    assert!((rect.x - 50.0).abs() < 0.01);
    assert!((rect.y - 50.0).abs() < 0.01);
    assert!((rect.width - 150.0).abs() < 0.01);
    assert!((rect.height - 75.0).abs() < 0.01);
  }

  // ============================================================================
  // EDGE CASE TESTS
  // ============================================================================

  #[test]
  fn tween_zero_duration() {
    let mut tween = TweenFloat::new(0.0, 100.0, 0.0);

    tween.update(0.016);
    assert!((tween.value() - 100.0).abs() < 0.01);
    assert!(tween.is_finished());
  }

  #[test]
  fn tween_negative_values() {
    let mut tween = TweenFloat::new(-100.0, 100.0, 1.0);

    tween.update(0.5);
    assert!((tween.value() - 0.0).abs() < 0.01);

    tween.update(0.5);
    assert!((tween.value() - 100.0).abs() < 0.01);
  }

  #[test]
  fn tween_large_dt_overshoot() {
    let mut tween = TweenFloat::new(0.0, 100.0, 1.0);

    // Large dt that overshoots duration
    tween.update(5.0);
    assert!((tween.value() - 100.0).abs() < 0.01);
    assert!(tween.is_finished());
  }

  #[test]
  fn tween_stop_resets_completely() {
    let mut tween = TweenFloat::new(0.0, 100.0, 1.0).with_loop(UiTweenLoop::PingPong);

    tween.update(1.5); // Into ping-pong reverse
    tween.stop();

    assert!(!tween.is_playing());
    assert_eq!(tween.value(), 0.0);
    assert_eq!(tween.progress(), 0.0);
  }

  #[test]
  fn tween_clone_is_independent() {
    let mut tween1 = TweenFloat::new(0.0, 100.0, 1.0);
    tween1.update(0.5);

    let mut tween2 = tween1.clone();
    tween2.update(0.5);

    // tween1 should still be at 50%
    assert!((tween1.value() - 50.0).abs() < 0.01);
    // tween2 should be at 100%
    assert!((tween2.value() - 100.0).abs() < 0.01);
  }

  #[test]
  fn tween_delay_with_overshoot() {
    let mut tween = TweenFloat::new(0.0, 100.0, 1.0).with_delay(0.5);

    // Update with dt that exceeds delay
    tween.update(0.7); // 0.2s into tween after 0.5s delay
    assert!(tween.value() > 0.0);
    assert!((tween.value() - 20.0).abs() < 1.0);
  }

  #[test]
  fn tween_on_start_only_called_once() {
    use std::sync::atomic::{AtomicUsize, Ordering};

    let count = Arc::new(AtomicUsize::new(0));
    let count_clone = count.clone();

    let mut tween = TweenFloat::new(0.0, 100.0, 1.0)
      .with_loop(UiTweenLoop::Loop)
      .on_start(move |_| {
        count_clone.fetch_add(1, Ordering::SeqCst);
      });

    // Multiple updates including loop restarts
    tween.update(0.5);
    tween.update(0.5);
    tween.update(0.5);
    tween.update(0.5);

    // on_start should only be called once
    assert_eq!(count.load(Ordering::SeqCst), 1);
  }

  #[test]
  fn tween_on_complete_called_each_loop() {
    use std::sync::atomic::{AtomicUsize, Ordering};

    let count = Arc::new(AtomicUsize::new(0));
    let count_clone = count.clone();

    let mut tween = TweenFloat::new(0.0, 100.0, 0.5)
      .with_loop(UiTweenLoop::Times(3))
      .on_complete(move |_| {
        count_clone.fetch_add(1, Ordering::SeqCst);
      });

    tween.update(0.5); // Complete loop 1
    tween.update(0.5); // Complete loop 2
    tween.update(0.5); // Complete loop 3

    assert_eq!(count.load(Ordering::SeqCst), 3);
  }

  // ============================================================================
  // EASING INTEGRATION TESTS
  // ============================================================================

  #[test]
  fn tween_all_easing_functions() {
    let easings = [
      Ease::Linear,
      Ease::InQuad,
      Ease::OutQuad,
      Ease::InOutQuad,
      Ease::InCubic,
      Ease::OutCubic,
      Ease::InOutCubic,
      Ease::InQuart,
      Ease::OutQuart,
      Ease::InOutQuart,
      Ease::InSine,
      Ease::OutSine,
      Ease::InOutSine,
      Ease::InExpo,
      Ease::OutExpo,
      Ease::InOutExpo,
      Ease::InBack,
      Ease::OutBack,
      Ease::InOutBack,
      Ease::InElastic,
      Ease::OutElastic,
      Ease::InOutElastic,
      Ease::InBounce,
      Ease::OutBounce,
      Ease::InOutBounce,
    ];

    for ease in easings {
      let mut tween = TweenFloat::new(0.0, 100.0, 1.0).with_ease(ease);

      // All tweens should start at 0
      assert!(
        (tween.value() - 0.0).abs() < 0.01,
        "Ease {:?} should start at 0",
        ease
      );

      // Update to completion
      tween.update(1.0);

      // All tweens should end at target (within tolerance for overshoot easings)
      // Back and Elastic can overshoot temporarily but should end at target
      assert!(
        (tween.value() - 100.0).abs() < 1.0,
        "Ease {:?} should end near 100, got {}",
        ease,
        tween.value()
      );
    }
  }

  // ============================================================================
  // COLOR INTERPOLATION EDGE CASES
  // ============================================================================

  #[test]
  fn lerp_color_with_alpha() {
    let start = Color::new(0, 0, 0, 0);
    let end = Color::new(255, 255, 255, 255);
    let mid = Color::lerp(&start, &end, 0.5);

    assert_eq!(mid.r, 128);
    assert_eq!(mid.g, 128);
    assert_eq!(mid.b, 128);
    assert_eq!(mid.a, 128);
  }

  #[test]
  fn tween_color_red_to_blue() {
    let mut tween = TweenColor::new(Color::RED, Color::BLUE, 1.0);

    tween.update(0.5);
    let color = tween.value();

    // RED (255, 0, 0) -> BLUE (0, 0, 255)
    // At 50%: R=128, G=0, B=128
    assert!((color.r as i32 - 128).abs() <= 1);
    assert_eq!(color.g, 0);
    assert!((color.b as i32 - 128).abs() <= 1);
  }

  // ============================================================================
  // RECT INTERPOLATION EDGE CASES
  // ============================================================================

  #[test]
  fn lerp_rect_negative_coords() {
    let start = Rect::new(-100.0, -100.0, 50.0, 50.0);
    let end = Rect::new(100.0, 100.0, 150.0, 150.0);
    let mid = Rect::lerp(&start, &end, 0.5);

    assert!((mid.x - 0.0).abs() < 0.01);
    assert!((mid.y - 0.0).abs() < 0.01);
    assert!((mid.width - 100.0).abs() < 0.01);
    assert!((mid.height - 100.0).abs() < 0.01);
  }

  #[test]
  fn tween_rect_shrinking() {
    let start = Rect::new(0.0, 0.0, 200.0, 200.0);
    let end = Rect::new(50.0, 50.0, 100.0, 100.0);
    let mut tween = TweenRect::new(start, end, 1.0);

    tween.update(1.0);
    let rect = tween.value();

    assert!((rect.x - 50.0).abs() < 0.01);
    assert!((rect.y - 50.0).abs() < 0.01);
    assert!((rect.width - 100.0).abs() < 0.01);
    assert!((rect.height - 100.0).abs() < 0.01);
  }
}
