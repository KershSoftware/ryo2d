//! Tweening system for smooth interpolation of Transform properties.
//!
//! Provides standard easing curves and a `Tween` component for animating
//! position, rotation, and scale over time.
//!
//! # Example
//!
//! ```ignore
//! use ryo2d::prelude::*;
//!
//! // Move to position with easing
//! world.spawn((
//!     Transform::new(100.0, 100.0),
//!     Tween::to_position(400.0, 300.0, 2.0)
//!         .with_easing(Ease::OutBounce),
//! ));
//!
//! // Pulsing scale effect
//! world.spawn((
//!     Transform::new(200.0, 200.0),
//!     Tween::to_scale(1.2, 1.2, 0.3)
//!         .with_easing(Ease::InOutSine)
//!         .with_loop(TweenLoop::PingPong),
//! ));
//! ```

use super::components::Transform;
use std::f32::consts::PI;

/// Easing functions for smooth interpolation.
///
/// Each function maps an input `t` in range [0.0, 1.0] to an output value,
/// where 0.0 represents the start and 1.0 represents the end.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Ease {
  /// Linear interpolation (no easing)
  #[default]
  Linear,

  // Quadratic
  /// Slow start, fast end (quadratic)
  InQuad,
  /// Fast start, slow end (quadratic)
  OutQuad,
  /// Slow start and end (quadratic)
  InOutQuad,

  // Cubic
  /// Slow start, fast end (cubic)
  InCubic,
  /// Fast start, slow end (cubic)
  OutCubic,
  /// Slow start and end (cubic)
  InOutCubic,

  // Quartic
  /// Slow start, fast end (quartic)
  InQuart,
  /// Fast start, slow end (quartic)
  OutQuart,
  /// Slow start and end (quartic)
  InOutQuart,

  // Sine
  /// Slow start, fast end (sine wave)
  InSine,
  /// Fast start, slow end (sine wave)
  OutSine,
  /// Slow start and end (sine wave)
  InOutSine,

  // Exponential
  /// Slow start, fast end (exponential)
  InExpo,
  /// Fast start, slow end (exponential)
  OutExpo,
  /// Slow start and end (exponential)
  InOutExpo,

  // Back (overshoot)
  /// Pulls back before accelerating
  InBack,
  /// Overshoots target before settling
  OutBack,
  /// Pulls back and overshoots
  InOutBack,

  // Elastic
  /// Elastic wobble at start
  InElastic,
  /// Elastic wobble at end
  OutElastic,
  /// Elastic wobble at start and end
  InOutElastic,

  // Bounce
  /// Bounces at start
  InBounce,
  /// Bounces at end
  OutBounce,
  /// Bounces at start and end
  InOutBounce,
}

impl Ease {
  /// Apply the easing function to a normalized time value.
  ///
  /// # Arguments
  /// * `t` - Progress value in range [0.0, 1.0]
  ///
  /// # Returns
  /// The eased value (may exceed [0.0, 1.0] for overshoot effects like Back/Elastic)
  pub fn apply(&self, t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);

    match self {
      Ease::Linear => t,

      // Quadratic
      Ease::InQuad => t * t,
      Ease::OutQuad => t * (2.0 - t),
      Ease::InOutQuad => {
        if t < 0.5 {
          2.0 * t * t
        } else {
          -1.0 + (4.0 - 2.0 * t) * t
        }
      }

      // Cubic
      Ease::InCubic => t * t * t,
      Ease::OutCubic => {
        let t1 = t - 1.0;
        t1 * t1 * t1 + 1.0
      }
      Ease::InOutCubic => {
        if t < 0.5 {
          4.0 * t * t * t
        } else {
          let t1 = 2.0 * t - 2.0;
          0.5 * t1 * t1 * t1 + 1.0
        }
      }

      // Quartic
      Ease::InQuart => t * t * t * t,
      Ease::OutQuart => {
        let t1 = t - 1.0;
        1.0 - t1 * t1 * t1 * t1
      }
      Ease::InOutQuart => {
        if t < 0.5 {
          8.0 * t * t * t * t
        } else {
          let t1 = t - 1.0;
          1.0 - 8.0 * t1 * t1 * t1 * t1
        }
      }

      // Sine
      Ease::InSine => 1.0 - (t * PI / 2.0).cos(),
      Ease::OutSine => (t * PI / 2.0).sin(),
      Ease::InOutSine => 0.5 * (1.0 - (PI * t).cos()),

      // Exponential
      Ease::InExpo => {
        if t == 0.0 {
          0.0
        } else {
          2.0_f32.powf(10.0 * (t - 1.0))
        }
      }
      Ease::OutExpo => {
        if t == 1.0 {
          1.0
        } else {
          1.0 - 2.0_f32.powf(-10.0 * t)
        }
      }
      Ease::InOutExpo => {
        if t == 0.0 {
          0.0
        } else if t == 1.0 {
          1.0
        } else if t < 0.5 {
          0.5 * 2.0_f32.powf(20.0 * t - 10.0)
        } else {
          1.0 - 0.5 * 2.0_f32.powf(-20.0 * t + 10.0)
        }
      }

      // Back (overshoot)
      Ease::InBack => {
        const C1: f32 = 1.70158;
        const C3: f32 = C1 + 1.0;
        C3 * t * t * t - C1 * t * t
      }
      Ease::OutBack => {
        const C1: f32 = 1.70158;
        const C3: f32 = C1 + 1.0;
        let t1 = t - 1.0;
        1.0 + C3 * t1 * t1 * t1 + C1 * t1 * t1
      }
      Ease::InOutBack => {
        const C1: f32 = 1.70158;
        const C2: f32 = C1 * 1.525;
        if t < 0.5 {
          0.5 * (2.0 * t) * (2.0 * t) * ((C2 + 1.0) * 2.0 * t - C2)
        } else {
          let t1 = 2.0 * t - 2.0;
          0.5 * (t1 * t1 * ((C2 + 1.0) * t1 + C2) + 2.0)
        }
      }

      // Elastic
      Ease::InElastic => {
        if t == 0.0 {
          0.0
        } else if t == 1.0 {
          1.0
        } else {
          const C4: f32 = 2.0 * PI / 3.0;
          -2.0_f32.powf(10.0 * t - 10.0) * ((10.0 * t - 10.75) * C4).sin()
        }
      }
      Ease::OutElastic => {
        if t == 0.0 {
          0.0
        } else if t == 1.0 {
          1.0
        } else {
          const C4: f32 = 2.0 * PI / 3.0;
          2.0_f32.powf(-10.0 * t) * ((10.0 * t - 0.75) * C4).sin() + 1.0
        }
      }
      Ease::InOutElastic => {
        if t == 0.0 {
          0.0
        } else if t == 1.0 {
          1.0
        } else {
          const C5: f32 = 2.0 * PI / 4.5;
          if t < 0.5 {
            -0.5 * 2.0_f32.powf(20.0 * t - 10.0) * ((20.0 * t - 11.125) * C5).sin()
          } else {
            0.5 * 2.0_f32.powf(-20.0 * t + 10.0) * ((20.0 * t - 11.125) * C5).sin() + 1.0
          }
        }
      }

      // Bounce
      Ease::OutBounce => out_bounce(t),
      Ease::InBounce => 1.0 - out_bounce(1.0 - t),
      Ease::InOutBounce => {
        if t < 0.5 {
          0.5 * (1.0 - out_bounce(1.0 - 2.0 * t))
        } else {
          0.5 * (1.0 + out_bounce(2.0 * t - 1.0))
        }
      }
    }
  }
}

/// Helper function for bounce easing
fn out_bounce(t: f32) -> f32 {
  const N1: f32 = 7.5625;
  const D1: f32 = 2.75;

  if t < 1.0 / D1 {
    N1 * t * t
  } else if t < 2.0 / D1 {
    let t = t - 1.5 / D1;
    N1 * t * t + 0.75
  } else if t < 2.5 / D1 {
    let t = t - 2.25 / D1;
    N1 * t * t + 0.9375
  } else {
    let t = t - 2.625 / D1;
    N1 * t * t + 0.984375
  }
}

/// Loop behavior for tweens.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TweenLoop {
  /// Play once and stop
  #[default]
  Once,
  /// Loop forever
  Loop,
  /// Play forward then backward, repeat forever
  PingPong,
  /// Loop a specific number of times
  Times(u32),
}

/// Component for tweening Transform properties over time.
///
/// Attach to an entity with a `Transform` to animate its position, rotation, or scale.
/// The engine updates active tweens automatically each frame.
///
/// # Example
///
/// ```ignore
/// // Move and rotate simultaneously
/// world.spawn((
///     Transform::new(0.0, 0.0),
///     Tween::new(1.5)
///         .to_x(200.0)
///         .to_y(100.0)
///         .to_rotation(std::f32::consts::PI)
///         .with_easing(Ease::OutCubic),
/// ));
/// ```
#[derive(Debug, Clone)]
pub struct Tween {
  // Target values (None = don't tween that property)
  pub target_x: Option<f32>,
  pub target_y: Option<f32>,
  pub target_rotation: Option<f32>,
  pub target_scale_x: Option<f32>,
  pub target_scale_y: Option<f32>,

  // Start values (captured when tween starts)
  start_x: f32,
  start_y: f32,
  start_rotation: f32,
  start_scale_x: f32,
  start_scale_y: f32,

  // Timing
  /// Total duration in seconds
  pub duration: f32,
  /// Delay before starting in seconds
  pub delay: f32,
  elapsed: f32,

  // Config
  /// Easing function to use
  pub easing: Ease,
  /// Loop behavior
  pub loop_mode: TweenLoop,

  // State
  started: bool,
  /// Whether the tween is currently playing
  pub playing: bool,
  /// True for one frame when the tween finishes (or each loop iteration for Loop/Times)
  pub just_finished: bool,
  /// For PingPong: true when going backward
  ping_pong_reverse: bool,
  /// For Times: current loop count
  loop_count: u32,
}

impl Default for Tween {
  fn default() -> Self {
    Self {
      target_x: None,
      target_y: None,
      target_rotation: None,
      target_scale_x: None,
      target_scale_y: None,
      start_x: 0.0,
      start_y: 0.0,
      start_rotation: 0.0,
      start_scale_x: 1.0,
      start_scale_y: 1.0,
      duration: 1.0,
      delay: 0.0,
      elapsed: 0.0,
      easing: Ease::Linear,
      loop_mode: TweenLoop::Once,
      started: false,
      playing: true,
      just_finished: false,
      ping_pong_reverse: false,
      loop_count: 0,
    }
  }
}

impl Tween {
  /// Create a new tween with the given duration.
  ///
  /// Use the builder methods to specify which properties to animate.
  pub fn new(duration: f32) -> Self {
    Self {
      duration,
      ..Default::default()
    }
  }

  /// Create a tween that moves to a position.
  pub fn to_position(x: f32, y: f32, duration: f32) -> Self {
    Self::new(duration).to_x(x).to_y(y)
  }

  /// Create a tween that rotates to an angle (radians).
  pub fn to_rotation(rotation: f32, duration: f32) -> Self {
    Self::new(duration).to_rotation_value(rotation)
  }

  /// Create a tween that scales to a size.
  pub fn to_scale(scale_x: f32, scale_y: f32, duration: f32) -> Self {
    Self::new(duration).to_scale_x(scale_x).to_scale_y(scale_y)
  }

  /// Create a tween that scales uniformly.
  pub fn to_uniform_scale(scale: f32, duration: f32) -> Self {
    Self::to_scale(scale, scale, duration)
  }

  // Builder methods for individual properties

  /// Set target X position.
  pub fn to_x(mut self, x: f32) -> Self {
    self.target_x = Some(x);
    self
  }

  /// Set target Y position.
  pub fn to_y(mut self, y: f32) -> Self {
    self.target_y = Some(y);
    self
  }

  /// Set target rotation (radians).
  pub fn to_rotation_value(mut self, rotation: f32) -> Self {
    self.target_rotation = Some(rotation);
    self
  }

  /// Set target X scale.
  pub fn to_scale_x(mut self, scale_x: f32) -> Self {
    self.target_scale_x = Some(scale_x);
    self
  }

  /// Set target Y scale.
  pub fn to_scale_y(mut self, scale_y: f32) -> Self {
    self.target_scale_y = Some(scale_y);
    self
  }

  /// Set the easing function.
  pub fn with_easing(mut self, easing: Ease) -> Self {
    self.easing = easing;
    self
  }

  /// Set a delay before the tween starts.
  pub fn with_delay(mut self, delay: f32) -> Self {
    self.delay = delay;
    self
  }

  /// Set the loop mode.
  pub fn with_loop(mut self, loop_mode: TweenLoop) -> Self {
    self.loop_mode = loop_mode;
    self
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
  }

  /// Restart the tween from the beginning.
  pub fn restart(&mut self) {
    self.stop();
    self.playing = true;
  }

  /// Check if the tween has completed (only meaningful for Once and Times modes).
  pub fn is_finished(&self) -> bool {
    match self.loop_mode {
      TweenLoop::Once => !self.playing && self.elapsed >= self.duration,
      TweenLoop::Times(n) => self.loop_count >= n,
      TweenLoop::Loop | TweenLoop::PingPong => false, // Never finishes
    }
  }

  /// Get the current progress (0.0 to 1.0).
  pub fn progress(&self) -> f32 {
    if self.duration <= 0.0 {
      1.0
    } else {
      (self.elapsed / self.duration).clamp(0.0, 1.0)
    }
  }

  /// Capture start values from the transform (called internally when tween starts).
  fn capture_start(&mut self, transform: &Transform) {
    self.start_x = transform.x;
    self.start_y = transform.y;
    self.start_rotation = transform.rotation;
    self.start_scale_x = transform.scale_x;
    self.start_scale_y = transform.scale_y;
    self.started = true;
  }

  /// Update the tween and apply to transform.
  ///
  /// Called by the engine's tween system each frame.
  pub fn update(&mut self, dt: f32, transform: &mut Transform) {
    self.just_finished = false;

    if !self.playing {
      return;
    }

    // Handle delay
    if self.delay > 0.0 {
      self.delay -= dt;
      if self.delay > 0.0 {
        return;
      }
      // Delay finished, capture any overshoot time and DON'T add dt below
      // since the overshoot already accounts for time past delay
      let overshoot = -self.delay;
      self.delay = 0.0;

      // Capture start values before using overshoot
      if !self.started {
        self.capture_start(transform);
      }

      self.elapsed = overshoot;
    } else {
      // Normal update: capture start values if needed, then add dt
      if !self.started {
        self.capture_start(transform);
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

    // Apply easing
    let eased = self.easing.apply(progress);

    // Interpolate and apply each property
    if let Some(target) = self.target_x {
      transform.x = lerp(self.start_x, target, eased);
    }
    if let Some(target) = self.target_y {
      transform.y = lerp(self.start_y, target, eased);
    }
    if let Some(target) = self.target_rotation {
      transform.rotation = lerp(self.start_rotation, target, eased);
    }
    if let Some(target) = self.target_scale_x {
      transform.scale_x = lerp(self.start_scale_x, target, eased);
    }
    if let Some(target) = self.target_scale_y {
      transform.scale_y = lerp(self.start_scale_y, target, eased);
    }

    // Handle completion
    if self.elapsed >= self.duration {
      match self.loop_mode {
        TweenLoop::Once => {
          self.playing = false;
          self.just_finished = true;
        }
        TweenLoop::Loop => {
          self.elapsed -= self.duration;
          self.just_finished = true;
        }
        TweenLoop::PingPong => {
          self.elapsed -= self.duration;
          self.ping_pong_reverse = !self.ping_pong_reverse;
          self.just_finished = true;
        }
        TweenLoop::Times(n) => {
          self.loop_count += 1;
          if self.loop_count >= n {
            self.playing = false;
            self.just_finished = true;
          } else {
            self.elapsed -= self.duration;
            self.just_finished = true;
          }
        }
      }
    }
  }
}

/// Linear interpolation between two values.
#[inline]
fn lerp(start: f32, end: f32, t: f32) -> f32 {
  start + (end - start) * t
}

#[cfg(test)]
mod tests {
  use super::*;

  // ===== Easing Function Tests =====

  #[test]
  fn ease_linear() {
    assert_eq!(Ease::Linear.apply(0.0), 0.0);
    assert_eq!(Ease::Linear.apply(0.5), 0.5);
    assert_eq!(Ease::Linear.apply(1.0), 1.0);
  }

  #[test]
  fn ease_in_quad() {
    assert_eq!(Ease::InQuad.apply(0.0), 0.0);
    assert_eq!(Ease::InQuad.apply(0.5), 0.25); // 0.5^2
    assert_eq!(Ease::InQuad.apply(1.0), 1.0);
  }

  #[test]
  fn ease_out_quad() {
    assert_eq!(Ease::OutQuad.apply(0.0), 0.0);
    assert_eq!(Ease::OutQuad.apply(0.5), 0.75); // 0.5 * (2 - 0.5)
    assert_eq!(Ease::OutQuad.apply(1.0), 1.0);
  }

  #[test]
  fn ease_in_out_quad() {
    assert_eq!(Ease::InOutQuad.apply(0.0), 0.0);
    assert_eq!(Ease::InOutQuad.apply(0.5), 0.5);
    assert_eq!(Ease::InOutQuad.apply(1.0), 1.0);
  }

  #[test]
  fn ease_cubic_endpoints() {
    for ease in [Ease::InCubic, Ease::OutCubic, Ease::InOutCubic] {
      assert_eq!(ease.apply(0.0), 0.0);
      assert!((ease.apply(1.0) - 1.0).abs() < 0.001);
    }
  }

  #[test]
  fn ease_quart_endpoints() {
    for ease in [Ease::InQuart, Ease::OutQuart, Ease::InOutQuart] {
      assert_eq!(ease.apply(0.0), 0.0);
      assert!((ease.apply(1.0) - 1.0).abs() < 0.001);
    }
  }

  #[test]
  fn ease_sine_endpoints() {
    for ease in [Ease::InSine, Ease::OutSine, Ease::InOutSine] {
      assert!((ease.apply(0.0) - 0.0).abs() < 0.001);
      assert!((ease.apply(1.0) - 1.0).abs() < 0.001);
    }
  }

  #[test]
  fn ease_expo_endpoints() {
    for ease in [Ease::InExpo, Ease::OutExpo, Ease::InOutExpo] {
      assert_eq!(ease.apply(0.0), 0.0);
      assert_eq!(ease.apply(1.0), 1.0);
    }
  }

  #[test]
  fn ease_back_overshoots() {
    // OutBack should overshoot 1.0 before settling
    let mid = Ease::OutBack.apply(0.7);
    assert!(mid > 1.0, "OutBack should overshoot: {}", mid);
    assert_eq!(Ease::OutBack.apply(1.0), 1.0);
  }

  #[test]
  fn ease_elastic_endpoints() {
    for ease in [Ease::InElastic, Ease::OutElastic, Ease::InOutElastic] {
      assert_eq!(ease.apply(0.0), 0.0);
      assert_eq!(ease.apply(1.0), 1.0);
    }
  }

  #[test]
  fn ease_bounce_endpoints() {
    for ease in [Ease::InBounce, Ease::OutBounce, Ease::InOutBounce] {
      assert!((ease.apply(0.0) - 0.0).abs() < 0.001);
      assert!((ease.apply(1.0) - 1.0).abs() < 0.001);
    }
  }

  #[test]
  fn ease_clamps_input() {
    // Should clamp negative values
    assert_eq!(Ease::Linear.apply(-0.5), 0.0);
    // Should clamp values > 1
    assert_eq!(Ease::Linear.apply(1.5), 1.0);
  }

  // ===== Tween Builder Tests =====

  #[test]
  fn tween_new() {
    let t = Tween::new(2.0);
    assert_eq!(t.duration, 2.0);
    assert!(t.target_x.is_none());
    assert!(t.target_y.is_none());
    assert!(t.playing);
  }

  #[test]
  fn tween_to_position() {
    let t = Tween::to_position(100.0, 200.0, 1.5);
    assert_eq!(t.target_x, Some(100.0));
    assert_eq!(t.target_y, Some(200.0));
    assert_eq!(t.duration, 1.5);
  }

  #[test]
  fn tween_to_rotation() {
    let t = Tween::to_rotation(PI, 0.5);
    assert_eq!(t.target_rotation, Some(PI));
    assert_eq!(t.duration, 0.5);
  }

  #[test]
  fn tween_to_scale() {
    let t = Tween::to_scale(2.0, 3.0, 1.0);
    assert_eq!(t.target_scale_x, Some(2.0));
    assert_eq!(t.target_scale_y, Some(3.0));
  }

  #[test]
  fn tween_to_uniform_scale() {
    let t = Tween::to_uniform_scale(1.5, 1.0);
    assert_eq!(t.target_scale_x, Some(1.5));
    assert_eq!(t.target_scale_y, Some(1.5));
  }

  #[test]
  fn tween_builder_chain() {
    let t = Tween::new(1.0)
      .to_x(100.0)
      .to_y(200.0)
      .to_rotation_value(PI)
      .with_easing(Ease::OutBounce)
      .with_delay(0.5)
      .with_loop(TweenLoop::PingPong);

    assert_eq!(t.target_x, Some(100.0));
    assert_eq!(t.target_y, Some(200.0));
    assert_eq!(t.target_rotation, Some(PI));
    assert_eq!(t.easing, Ease::OutBounce);
    assert_eq!(t.delay, 0.5);
    assert_eq!(t.loop_mode, TweenLoop::PingPong);
  }

  // ===== Tween Update Tests =====

  #[test]
  fn tween_updates_position() {
    let mut t = Tween::to_position(100.0, 100.0, 1.0);
    let mut transform = Transform::new(0.0, 0.0);

    // First update captures start values
    t.update(0.5, &mut transform);

    assert!(t.started);
    assert_eq!(transform.x, 50.0);
    assert_eq!(transform.y, 50.0);

    // Complete the tween
    t.update(0.5, &mut transform);

    assert_eq!(transform.x, 100.0);
    assert_eq!(transform.y, 100.0);
    assert!(!t.playing);
    assert!(t.just_finished);
  }

  #[test]
  fn tween_with_delay() {
    let mut t = Tween::to_position(100.0, 0.0, 1.0).with_delay(0.5);
    let mut transform = Transform::new(0.0, 0.0);

    // During delay, position shouldn't change
    t.update(0.3, &mut transform);
    assert_eq!(transform.x, 0.0);
    assert!(!t.started);

    // Delay completes, tween starts
    t.update(0.3, &mut transform);
    assert!(t.started);
    // 0.1 seconds of actual tween time (0.6 total - 0.5 delay)
    assert!((transform.x - 10.0).abs() < 0.01);
  }

  #[test]
  fn tween_loop_mode() {
    let mut t = Tween::to_position(100.0, 0.0, 1.0).with_loop(TweenLoop::Loop);
    let mut transform = Transform::new(0.0, 0.0);

    // Complete one loop
    t.update(1.0, &mut transform);
    assert!(t.just_finished);
    assert!(t.playing); // Still playing because it loops

    // Reset happens
    t.update(0.5, &mut transform);
    assert_eq!(transform.x, 50.0); // Back to interpolating from start
  }

  #[test]
  fn tween_ping_pong() {
    let mut t = Tween::to_position(100.0, 0.0, 1.0).with_loop(TweenLoop::PingPong);
    let mut transform = Transform::new(0.0, 0.0);

    // Forward to end
    t.update(1.0, &mut transform);
    assert_eq!(transform.x, 100.0);
    assert!(t.ping_pong_reverse);

    // Backward
    t.update(0.5, &mut transform);
    assert_eq!(transform.x, 50.0);

    t.update(0.5, &mut transform);
    assert_eq!(transform.x, 0.0);
    assert!(!t.ping_pong_reverse); // Back to forward
  }

  #[test]
  fn tween_times() {
    let mut t = Tween::to_position(100.0, 0.0, 1.0).with_loop(TweenLoop::Times(2));
    let mut transform = Transform::new(0.0, 0.0);

    // First loop
    t.update(1.0, &mut transform);
    assert!(t.playing);
    assert_eq!(t.loop_count, 1);

    // Second loop
    t.update(1.0, &mut transform);
    assert!(!t.playing);
    assert_eq!(t.loop_count, 2);
    assert!(t.is_finished());
  }

  #[test]
  fn tween_pause_resume() {
    let mut t = Tween::to_position(100.0, 0.0, 1.0);
    let mut transform = Transform::new(0.0, 0.0);

    t.update(0.5, &mut transform);
    assert_eq!(transform.x, 50.0);

    t.pause();
    t.update(0.5, &mut transform);
    assert_eq!(transform.x, 50.0); // No change while paused

    t.resume();
    t.update(0.5, &mut transform);
    assert_eq!(transform.x, 100.0);
  }

  #[test]
  fn tween_stop_restart() {
    let mut t = Tween::to_position(100.0, 0.0, 1.0);
    let mut transform = Transform::new(0.0, 0.0);

    t.update(0.5, &mut transform);
    t.stop();

    assert!(!t.playing);
    assert!(!t.started);
    assert_eq!(t.elapsed, 0.0);

    t.restart();
    assert!(t.playing);
  }

  #[test]
  fn tween_progress() {
    let mut t = Tween::to_position(100.0, 0.0, 2.0);
    let mut transform = Transform::new(0.0, 0.0);

    assert_eq!(t.progress(), 0.0);

    t.update(1.0, &mut transform);
    assert_eq!(t.progress(), 0.5);

    t.update(1.0, &mut transform);
    assert_eq!(t.progress(), 1.0);
  }

  #[test]
  fn tween_zero_duration() {
    let mut t = Tween::to_position(100.0, 0.0, 0.0);
    let mut transform = Transform::new(0.0, 0.0);

    t.update(0.016, &mut transform);

    // Should immediately reach target
    assert_eq!(transform.x, 100.0);
    assert!(t.is_finished());
  }

  #[test]
  fn tween_only_tweens_specified_properties() {
    let mut t = Tween::new(1.0).to_x(100.0); // Only x, not y
    let mut transform = Transform::new(50.0, 50.0);

    t.update(1.0, &mut transform);

    assert_eq!(transform.x, 100.0); // x was tweened
    assert_eq!(transform.y, 50.0); // y unchanged
  }

  #[test]
  fn tween_with_easing() {
    let mut t = Tween::to_position(100.0, 0.0, 1.0).with_easing(Ease::InQuad);
    let mut transform = Transform::new(0.0, 0.0);

    t.update(0.5, &mut transform);

    // InQuad at t=0.5 gives 0.25, so x should be 25
    assert_eq!(transform.x, 25.0);
  }
}
