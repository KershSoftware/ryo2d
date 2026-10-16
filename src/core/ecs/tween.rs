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
//!     Transform::from_xy(100.0, 100.0),
//!     Tween::to_position(400.0, 300.0, 2.0)
//!         .with_easing(Ease::OutBounce),
//! ));
//!
//! // Pulsing scale effect
//! world.spawn((
//!     Transform::from_xy(200.0, 200.0),
//!     Tween::to_scale(1.2, 1.2, 0.3)
//!         .with_easing(Ease::InOutSine)
//!         .with_loop(TweenLoop::PingPong),
//! ));
//! ```

use super::kernel::Component;
use super::transform::Transform;
use crate::core::ui::Color;
use std::f32::consts::PI;
use std::sync::Arc;

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

/// Type alias for tween callbacks.
///
/// Callbacks are `Arc`-wrapped to allow cloning the Tween while sharing the callback.
/// Use `Fn` (not `FnMut`) since callbacks may be called multiple times for looping tweens.
pub type TweenCallback = Arc<dyn Fn(f32) + Send + Sync>;

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
/// The engine updates active tweens automatically each fixed step (`FixedSet::Juice`).
///
/// # Example
///
/// ```ignore
/// // Move and rotate simultaneously
/// world.spawn((
///     Transform::from_xy(0.0, 0.0),
///     Tween::new(1.5)
///         .to_x(200.0)
///         .to_y(100.0)
///         .to_rotation_value(std::f32::consts::PI)
///         .with_easing(Ease::OutCubic),
/// ));
/// ```
#[derive(Component, Clone)]
pub struct Tween {
  // Target values (None = don't tween that property)
  pub target_x: Option<f32>,
  pub target_y: Option<f32>,
  pub target_rotation: Option<f32>,
  pub target_scale_x: Option<f32>,
  pub target_scale_y: Option<f32>,
  /// Target alpha (0.0-1.0): a `Renderable`'s alpha, or a `Text2d`'s opacity
  pub target_alpha: Option<f32>,
  /// Target tint color - requires Renderable component
  pub target_tint: Option<Color>,

  // Start values (captured when tween starts)
  start_x: f32,
  start_y: f32,
  start_rotation: f32,
  start_scale_x: f32,
  start_scale_y: f32,
  start_alpha: f32,
  start_tint: Color,

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
  /// Playback speed multiplier (1.0 = normal, 2.0 = double speed, 0.5 = half speed)
  pub speed: f32,

  // State
  started: bool,
  /// Whether the tween is currently playing
  pub playing: bool,
  /// True for one fixed step when the tween starts (after delay)
  pub just_started: bool,
  /// True for one fixed step when the tween finishes (or each loop iteration for Loop/Times)
  pub just_finished: bool,
  /// For PingPong: true when going backward
  ping_pong_reverse: bool,
  /// For Times: current loop count
  loop_count: u32,

  // Callbacks
  on_start: Option<TweenCallback>,
  on_update: Option<TweenCallback>,
  on_complete: Option<TweenCallback>,
}

impl Default for Tween {
  fn default() -> Self {
    Self {
      target_x: None,
      target_y: None,
      target_rotation: None,
      target_scale_x: None,
      target_scale_y: None,
      target_alpha: None,
      target_tint: None,
      start_x: 0.0,
      start_y: 0.0,
      start_rotation: 0.0,
      start_scale_x: 1.0,
      start_scale_y: 1.0,
      start_alpha: 1.0,
      start_tint: Color::WHITE,
      duration: 1.0,
      delay: 0.0,
      elapsed: 0.0,
      easing: Ease::Linear,
      loop_mode: TweenLoop::Once,
      speed: 1.0,
      started: false,
      playing: true,
      just_started: false,
      just_finished: false,
      ping_pong_reverse: false,
      loop_count: 0,
      on_start: None,
      on_update: None,
      on_complete: None,
    }
  }
}

impl std::fmt::Debug for Tween {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    f.debug_struct("Tween")
      .field("target_x", &self.target_x)
      .field("target_y", &self.target_y)
      .field("target_rotation", &self.target_rotation)
      .field("target_scale_x", &self.target_scale_x)
      .field("target_scale_y", &self.target_scale_y)
      .field("target_alpha", &self.target_alpha)
      .field("target_tint", &self.target_tint)
      .field("duration", &self.duration)
      .field("delay", &self.delay)
      .field("elapsed", &self.elapsed)
      .field("easing", &self.easing)
      .field("loop_mode", &self.loop_mode)
      .field("speed", &self.speed)
      .field("playing", &self.playing)
      .field("just_started", &self.just_started)
      .field("just_finished", &self.just_finished)
      .field("on_start", &self.on_start.as_ref().map(|_| "Fn(f32)"))
      .field("on_update", &self.on_update.as_ref().map(|_| "Fn(f32)"))
      .field("on_complete", &self.on_complete.as_ref().map(|_| "Fn(f32)"))
      .finish()
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

  /// Set target alpha: a `Renderable`'s alpha, or a `Text2d`'s opacity
  /// (world text fades without being laid out again).
  ///
  /// Alpha will be clamped to 0.0-1.0 range.
  pub fn to_alpha(mut self, alpha: f32) -> Self {
    self.target_alpha = Some(alpha.clamp(0.0, 1.0));
    self
  }

  /// Set target tint color (requires Renderable component on entity).
  ///
  /// Use `Color::WHITE` for no tint, or any color to tint the sprite.
  pub fn to_tint(mut self, tint: Color) -> Self {
    self.target_tint = Some(tint);
    self
  }

  /// Create a tween that fades alpha to a target value.
  pub fn fade_to(alpha: f32, duration: f32) -> Self {
    Self::new(duration).to_alpha(alpha)
  }

  /// Create a tween that fades out (alpha to 0).
  pub fn fade_out(duration: f32) -> Self {
    Self::fade_to(0.0, duration)
  }

  /// Create a tween that fades in (alpha to 1).
  pub fn fade_in(duration: f32) -> Self {
    Self::fade_to(1.0, duration)
  }

  /// Create a tween that transitions to a tint color.
  pub fn tint_to(tint: Color, duration: f32) -> Self {
    Self::new(duration).to_tint(tint)
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

  /// Set the playback speed multiplier.
  ///
  /// - `1.0` = normal speed
  /// - `2.0` = double speed (completes in half the time)
  /// - `0.5` = half speed (takes twice as long)
  /// - `0.0` = paused (use `pause()` instead for explicit pausing)
  pub fn with_speed(mut self, speed: f32) -> Self {
    self.speed = speed;
    self
  }

  /// Set the playback speed at runtime.
  pub fn set_speed(&mut self, speed: f32) {
    self.speed = speed;
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

  /// Set a callback to be called every frame while the tween is active.
  ///
  /// Note: This is called frequently, keep the callback lightweight.
  pub fn on_update<F>(mut self, callback: F) -> Self
  where
    F: Fn(f32) + Send + Sync + 'static,
  {
    self.on_update = Some(Arc::new(callback));
    self
  }

  /// Set a callback to be called when the tween completes.
  ///
  /// For looping tweens (`Loop`, `PingPong`), this is called each time
  /// a cycle completes. For `Times(n)`, it's called after the final iteration.
  pub fn on_complete<F>(mut self, callback: F) -> Self
  where
    F: Fn(f32) + Send + Sync + 'static,
  {
    self.on_complete = Some(Arc::new(callback));
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
    self.just_started = false;
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

  /// Capture start values from the transform, if any (called when the tween starts).
  fn capture_start(&mut self, transform: Option<&Transform>) {
    if let Some(transform) = transform {
      self.start_x = transform.translation.x;
      self.start_y = transform.translation.y;
      self.start_rotation = transform.rotation;
      self.start_scale_x = transform.scale.x;
      self.start_scale_y = transform.scale.y;
    }
    self.started = true;
  }

  /// Whether the tween writes any `Transform` field.
  pub fn has_transform_targets(&self) -> bool {
    self.target_x.is_some()
      || self.target_y.is_some()
      || self.target_rotation.is_some()
      || self.target_scale_x.is_some()
      || self.target_scale_y.is_some()
  }

  /// Write the current alpha and tint targets to `renderable`.
  pub fn apply_color(&self, renderable: &mut super::Renderable) {
    if let Some(alpha) = self.current_alpha() {
      renderable.alpha = alpha;
    }
    if let Some(tint) = self.current_tint() {
      renderable.tint = tint;
    }
  }

  /// Capture start values from renderable (called by engine when tween has color targets).
  pub fn capture_start_renderable(&mut self, alpha: f32, tint: Color) {
    self.start_alpha = alpha;
    self.start_tint = tint;
  }

  /// Check if this tween has any color/alpha targets (needs Renderable).
  pub fn has_color_targets(&self) -> bool {
    self.target_alpha.is_some() || self.target_tint.is_some()
  }

  /// Check if the tween has started (start values have been captured).
  pub fn has_started(&self) -> bool {
    self.started
  }

  /// Get interpolated alpha value at current progress.
  ///
  /// Returns None if no alpha target is set.
  pub fn current_alpha(&self) -> Option<f32> {
    self.target_alpha.map(|target| {
      let progress = self.progress();
      let direction_progress = if self.ping_pong_reverse {
        1.0 - progress
      } else {
        progress
      };
      let eased = self.easing.apply(direction_progress);
      lerp(self.start_alpha, target, eased)
    })
  }

  /// Get interpolated tint color at current progress.
  ///
  /// Returns None if no tint target is set.
  pub fn current_tint(&self) -> Option<Color> {
    self.target_tint.map(|target| {
      let progress = self.progress();
      let direction_progress = if self.ping_pong_reverse {
        1.0 - progress
      } else {
        progress
      };
      let eased = self.easing.apply(direction_progress);
      lerp_color(self.start_tint, target, eased)
    })
  }

  /// Update the tween and apply to transform.
  ///
  /// Called by the engine's tween system each fixed step.
  pub fn update(&mut self, dt: f32, transform: &mut Transform) {
    self.step(dt, Some(transform));
  }

  /// Update a tween with no `Transform` to write (one that only targets
  /// alpha or tint, or an entity without a `Transform`).
  pub fn update_without_transform(&mut self, dt: f32) {
    self.step(dt, None);
  }

  fn step(&mut self, dt: f32, transform: Option<&mut Transform>) {
    self.just_finished = false;
    self.just_started = false;

    if !self.playing {
      return;
    }

    // Apply speed multiplier to delta time
    let dt = dt * self.speed;

    // Handle delay (delay is NOT affected by speed - it's real time)
    if self.delay > 0.0 {
      self.delay -= dt / self.speed.max(0.001); // Use unscaled dt for delay
      if self.delay > 0.0 {
        return;
      }
      // Delay finished, capture any overshoot time and DON'T add dt below
      // since the overshoot already accounts for time past delay
      let overshoot = -self.delay * self.speed; // Scale overshoot by speed
      self.delay = 0.0;

      // Capture start values before using overshoot
      if !self.started {
        self.capture_start(transform.as_deref());
        self.just_started = true;
        if let Some(ref cb) = self.on_start {
          cb(0.0);
        }
      }

      self.elapsed = overshoot;
    } else {
      // Normal update: capture start values if needed, then add dt
      if !self.started {
        self.capture_start(transform.as_deref());
        self.just_started = true;
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

    // Apply easing
    let eased = self.easing.apply(progress);

    // Interpolate and apply each property
    if let Some(transform) = transform {
      if let Some(target) = self.target_x {
        transform.translation.x = lerp(self.start_x, target, eased);
      }
      if let Some(target) = self.target_y {
        transform.translation.y = lerp(self.start_y, target, eased);
      }
      if let Some(target) = self.target_rotation {
        transform.rotation = lerp(self.start_rotation, target, eased);
      }
      if let Some(target) = self.target_scale_x {
        transform.scale.x = lerp(self.start_scale_x, target, eased);
      }
      if let Some(target) = self.target_scale_y {
        transform.scale.y = lerp(self.start_scale_y, target, eased);
      }
    }

    // Call on_update callback
    if let Some(ref cb) = self.on_update {
      cb(eased);
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

      // Call on_complete callback when finished
      if self.just_finished
        && let Some(ref cb) = self.on_complete
      {
        cb(1.0);
      }
    }
  }
}

/// Linear interpolation between two values.
#[inline]
fn lerp(start: f32, end: f32, t: f32) -> f32 {
  start + (end - start) * t
}

/// Linear interpolation between two colors.
#[inline]
fn lerp_color(start: Color, end: Color, t: f32) -> Color {
  Color::new(
    lerp(start.r as f32, end.r as f32, t).round() as u8,
    lerp(start.g as f32, end.g as f32, t).round() as u8,
    lerp(start.b as f32, end.b as f32, t).round() as u8,
    lerp(start.a as f32, end.a as f32, t).round() as u8,
  )
}

/// A sequence of tweens that play one after another.
///
/// When one tween completes, the next one automatically starts.
/// Useful for complex multi-stage animations.
///
/// # Example
///
/// ```ignore
/// use ryo2d::prelude::*;
///
/// // Create a sequence: move right, then up, then fade out
/// let sequence = TweenSequence::new()
///     .then(Tween::to_position(200.0, 100.0, 0.5).with_easing(Ease::OutQuad))
///     .then(Tween::to_position(200.0, 50.0, 0.3).with_easing(Ease::OutQuad))
///     .then(Tween::fade_out(0.2));
///
/// world.spawn((
///     Transform::from_xy(100.0, 100.0),
///     Renderable::sprite(handle),
///     sequence,
/// ));
/// ```
#[derive(Component, Debug, Clone)]
pub struct TweenSequence {
  /// The tweens in order
  tweens: Vec<Tween>,
  /// Index of currently active tween
  current_index: usize,
  /// Whether the sequence is playing
  pub playing: bool,
  /// True for one fixed step when the entire sequence finishes
  pub just_finished: bool,
  /// Loop the entire sequence
  pub loop_sequence: bool,
  /// Playback speed multiplier (default: 1.0)
  pub speed: f32,
}

impl Default for TweenSequence {
  fn default() -> Self {
    Self::new()
  }
}

impl TweenSequence {
  /// Create a new empty tween sequence.
  pub fn new() -> Self {
    Self {
      tweens: Vec::new(),
      current_index: 0,
      playing: true,
      just_finished: false,
      loop_sequence: false,
      speed: 1.0,
    }
  }

  /// Add a tween to the sequence.
  ///
  /// Returns self for chaining.
  pub fn then(mut self, tween: Tween) -> Self {
    self.tweens.push(tween);
    self
  }

  /// Add multiple tweens at once.
  pub fn then_all(mut self, tweens: impl IntoIterator<Item = Tween>) -> Self {
    self.tweens.extend(tweens);
    self
  }

  /// Set the sequence to loop forever.
  pub fn with_loop(mut self) -> Self {
    self.loop_sequence = true;
    self
  }

  /// Set the playback speed multiplier for the entire sequence.
  ///
  /// This multiplies the delta time passed to child tweens.
  /// `2.0` = double speed, `0.5` = half speed.
  pub fn with_speed(mut self, speed: f32) -> Self {
    self.speed = speed;
    self
  }

  /// Set the playback speed at runtime.
  pub fn set_speed(&mut self, speed: f32) {
    self.speed = speed;
  }

  /// Get the current tween (if any).
  pub fn current(&self) -> Option<&Tween> {
    self.tweens.get(self.current_index)
  }

  /// Get the current tween mutably (if any).
  pub fn current_mut(&mut self) -> Option<&mut Tween> {
    self.tweens.get_mut(self.current_index)
  }

  /// Check if the sequence is finished.
  pub fn is_finished(&self) -> bool {
    !self.loop_sequence && self.current_index >= self.tweens.len()
  }

  /// Get the total number of tweens in the sequence.
  pub fn len(&self) -> usize {
    self.tweens.len()
  }

  /// Check if the sequence is empty.
  pub fn is_empty(&self) -> bool {
    self.tweens.is_empty()
  }

  /// Get the index of the current tween.
  pub fn current_index(&self) -> usize {
    self.current_index
  }

  /// Pause the sequence.
  pub fn pause(&mut self) {
    self.playing = false;
    if let Some(tween) = self.current_mut() {
      tween.pause();
    }
  }

  /// Resume the sequence.
  pub fn resume(&mut self) {
    self.playing = true;
    if let Some(tween) = self.current_mut() {
      tween.resume();
    }
  }

  /// Stop and reset the sequence.
  pub fn stop(&mut self) {
    self.playing = false;
    self.current_index = 0;
    for tween in &mut self.tweens {
      tween.stop();
    }
  }

  /// Restart the sequence from the beginning.
  pub fn restart(&mut self) {
    self.stop();
    self.playing = true;
    if let Some(tween) = self.tweens.first_mut() {
      tween.restart();
    }
  }

  /// Advance to the next tween in the sequence.
  ///
  /// Called by the engine when current tween finishes.
  pub fn advance(&mut self) {
    self.current_index += 1;

    if self.current_index >= self.tweens.len() {
      if self.loop_sequence {
        // Reset all tweens and start over
        for tween in &mut self.tweens {
          tween.restart();
        }
        self.current_index = 0;
      } else {
        self.just_finished = true;
        self.playing = false;
      }
    } else if let Some(tween) = self.tweens.get_mut(self.current_index) {
      tween.restart();
    }
  }
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
    let mut transform = Transform::from_xy(0.0, 0.0);

    // First update captures start values
    t.update(0.5, &mut transform);

    assert!(t.started);
    assert_eq!(transform.translation.x, 50.0);
    assert_eq!(transform.translation.y, 50.0);

    // Complete the tween
    t.update(0.5, &mut transform);

    assert_eq!(transform.translation.x, 100.0);
    assert_eq!(transform.translation.y, 100.0);
    assert!(!t.playing);
    assert!(t.just_finished);
  }

  #[test]
  fn tween_with_delay() {
    let mut t = Tween::to_position(100.0, 0.0, 1.0).with_delay(0.5);
    let mut transform = Transform::from_xy(0.0, 0.0);

    // During delay, position shouldn't change
    t.update(0.3, &mut transform);
    assert_eq!(transform.translation.x, 0.0);
    assert!(!t.started);

    // Delay completes, tween starts
    t.update(0.3, &mut transform);
    assert!(t.started);
    // 0.1 seconds of actual tween time (0.6 total - 0.5 delay)
    assert!((transform.translation.x - 10.0).abs() < 0.01);
  }

  #[test]
  fn tween_loop_mode() {
    let mut t = Tween::to_position(100.0, 0.0, 1.0).with_loop(TweenLoop::Loop);
    let mut transform = Transform::from_xy(0.0, 0.0);

    // Complete one loop
    t.update(1.0, &mut transform);
    assert!(t.just_finished);
    assert!(t.playing); // Still playing because it loops

    // Reset happens
    t.update(0.5, &mut transform);
    assert_eq!(transform.translation.x, 50.0); // Back to interpolating from start
  }

  #[test]
  fn tween_ping_pong() {
    let mut t = Tween::to_position(100.0, 0.0, 1.0).with_loop(TweenLoop::PingPong);
    let mut transform = Transform::from_xy(0.0, 0.0);

    // Forward to end
    t.update(1.0, &mut transform);
    assert_eq!(transform.translation.x, 100.0);
    assert!(t.ping_pong_reverse);

    // Backward
    t.update(0.5, &mut transform);
    assert_eq!(transform.translation.x, 50.0);

    t.update(0.5, &mut transform);
    assert_eq!(transform.translation.x, 0.0);
    assert!(!t.ping_pong_reverse); // Back to forward
  }

  #[test]
  fn tween_times() {
    let mut t = Tween::to_position(100.0, 0.0, 1.0).with_loop(TweenLoop::Times(2));
    let mut transform = Transform::from_xy(0.0, 0.0);

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
    let mut transform = Transform::from_xy(0.0, 0.0);

    t.update(0.5, &mut transform);
    assert_eq!(transform.translation.x, 50.0);

    t.pause();
    t.update(0.5, &mut transform);
    assert_eq!(transform.translation.x, 50.0); // No change while paused

    t.resume();
    t.update(0.5, &mut transform);
    assert_eq!(transform.translation.x, 100.0);
  }

  #[test]
  fn tween_stop_restart() {
    let mut t = Tween::to_position(100.0, 0.0, 1.0);
    let mut transform = Transform::from_xy(0.0, 0.0);

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
    let mut transform = Transform::from_xy(0.0, 0.0);

    assert_eq!(t.progress(), 0.0);

    t.update(1.0, &mut transform);
    assert_eq!(t.progress(), 0.5);

    t.update(1.0, &mut transform);
    assert_eq!(t.progress(), 1.0);
  }

  #[test]
  fn tween_zero_duration() {
    let mut t = Tween::to_position(100.0, 0.0, 0.0);
    let mut transform = Transform::from_xy(0.0, 0.0);

    t.update(0.016, &mut transform);

    // Should immediately reach target
    assert_eq!(transform.translation.x, 100.0);
    assert!(t.is_finished());
  }

  #[test]
  fn tween_only_tweens_specified_properties() {
    let mut t = Tween::new(1.0).to_x(100.0); // Only x, not y
    let mut transform = Transform::from_xy(50.0, 50.0);

    t.update(1.0, &mut transform);

    assert_eq!(transform.translation.x, 100.0); // x was tweened
    assert_eq!(transform.translation.y, 50.0); // y unchanged
  }

  #[test]
  fn tween_with_easing() {
    let mut t = Tween::to_position(100.0, 0.0, 1.0).with_easing(Ease::InQuad);
    let mut transform = Transform::from_xy(0.0, 0.0);

    t.update(0.5, &mut transform);

    // InQuad at t=0.5 gives 0.25, so x should be 25
    assert_eq!(transform.translation.x, 25.0);
  }

  // ===== Speed Control Tests =====

  #[test]
  fn tween_with_speed_double() {
    let mut t = Tween::to_position(100.0, 0.0, 1.0).with_speed(2.0);
    let mut transform = Transform::from_xy(0.0, 0.0);

    // At 2x speed, 0.25 seconds should give 50% progress
    t.update(0.25, &mut transform);
    assert_eq!(transform.translation.x, 50.0);

    // Complete in half the time
    t.update(0.25, &mut transform);
    assert_eq!(transform.translation.x, 100.0);
    assert!(t.just_finished);
  }

  #[test]
  fn tween_with_speed_half() {
    let mut t = Tween::to_position(100.0, 0.0, 1.0).with_speed(0.5);
    let mut transform = Transform::from_xy(0.0, 0.0);

    // At 0.5x speed, 1 second should give 50% progress
    t.update(1.0, &mut transform);
    assert_eq!(transform.translation.x, 50.0);

    // Takes twice as long
    t.update(1.0, &mut transform);
    assert_eq!(transform.translation.x, 100.0);
    assert!(t.just_finished);
  }

  #[test]
  fn tween_set_speed_runtime() {
    let mut t = Tween::to_position(100.0, 0.0, 1.0);
    let mut transform = Transform::from_xy(0.0, 0.0);

    t.update(0.5, &mut transform);
    assert_eq!(transform.translation.x, 50.0);

    // Double the speed mid-tween
    t.set_speed(2.0);
    t.update(0.25, &mut transform);
    assert_eq!(transform.translation.x, 100.0);
  }

  #[test]
  fn tween_speed_zero_pauses() {
    let mut t = Tween::to_position(100.0, 0.0, 1.0).with_speed(0.0);
    let mut transform = Transform::from_xy(0.0, 0.0);

    // With speed 0, no progress should be made
    t.update(1.0, &mut transform);
    assert_eq!(transform.translation.x, 0.0);
    assert!(t.playing); // Still playing, just not progressing
  }

  // ===== Callback Tests =====

  #[test]
  fn tween_on_start_callback() {
    use std::sync::atomic::{AtomicBool, Ordering};

    let started = Arc::new(AtomicBool::new(false));
    let started_clone = started.clone();

    let mut t = Tween::to_position(100.0, 0.0, 1.0).on_start(move |_| {
      started_clone.store(true, Ordering::SeqCst);
    });
    let mut transform = Transform::from_xy(0.0, 0.0);

    assert!(!started.load(Ordering::SeqCst));

    t.update(0.1, &mut transform);

    assert!(started.load(Ordering::SeqCst));
    assert!(t.just_started);
  }

  #[test]
  fn tween_on_start_only_once() {
    use std::sync::atomic::{AtomicUsize, Ordering};

    let count = Arc::new(AtomicUsize::new(0));
    let count_clone = count.clone();

    let mut t = Tween::to_position(100.0, 0.0, 1.0).on_start(move |_| {
      count_clone.fetch_add(1, Ordering::SeqCst);
    });
    let mut transform = Transform::from_xy(0.0, 0.0);

    t.update(0.1, &mut transform);
    t.update(0.1, &mut transform);
    t.update(0.1, &mut transform);

    // on_start should only be called once
    assert_eq!(count.load(Ordering::SeqCst), 1);
  }

  #[test]
  fn tween_on_update_callback() {
    use std::sync::atomic::{AtomicUsize, Ordering};

    let count = Arc::new(AtomicUsize::new(0));
    let count_clone = count.clone();

    let mut t = Tween::to_position(100.0, 0.0, 1.0).on_update(move |_| {
      count_clone.fetch_add(1, Ordering::SeqCst);
    });
    let mut transform = Transform::from_xy(0.0, 0.0);

    t.update(0.1, &mut transform);
    t.update(0.1, &mut transform);
    t.update(0.1, &mut transform);

    // on_update called each frame
    assert_eq!(count.load(Ordering::SeqCst), 3);
  }

  #[test]
  fn tween_on_complete_callback() {
    use std::sync::atomic::{AtomicBool, Ordering};

    let completed = Arc::new(AtomicBool::new(false));
    let completed_clone = completed.clone();

    let mut t = Tween::to_position(100.0, 0.0, 1.0).on_complete(move |_| {
      completed_clone.store(true, Ordering::SeqCst);
    });
    let mut transform = Transform::from_xy(0.0, 0.0);

    t.update(0.5, &mut transform);
    assert!(!completed.load(Ordering::SeqCst));

    t.update(0.5, &mut transform);
    assert!(completed.load(Ordering::SeqCst));
    assert!(t.just_finished);
  }

  #[test]
  fn tween_on_complete_loop_calls_each_iteration() {
    use std::sync::atomic::{AtomicUsize, Ordering};

    let count = Arc::new(AtomicUsize::new(0));
    let count_clone = count.clone();

    let mut t = Tween::to_position(100.0, 0.0, 0.5)
      .with_loop(TweenLoop::Times(3))
      .on_complete(move |_| {
        count_clone.fetch_add(1, Ordering::SeqCst);
      });
    let mut transform = Transform::from_xy(0.0, 0.0);

    // Complete 3 iterations
    t.update(0.5, &mut transform); // iteration 1
    t.update(0.5, &mut transform); // iteration 2
    t.update(0.5, &mut transform); // iteration 3

    assert_eq!(count.load(Ordering::SeqCst), 3);
  }

  #[test]
  fn tween_just_started_flag() {
    let mut t = Tween::to_position(100.0, 0.0, 1.0);
    let mut transform = Transform::from_xy(0.0, 0.0);

    assert!(!t.just_started);

    t.update(0.1, &mut transform);
    assert!(t.just_started);

    t.update(0.1, &mut transform);
    assert!(!t.just_started); // Only true for one frame
  }

  #[test]
  fn tween_callbacks_with_delay() {
    use std::sync::atomic::{AtomicBool, Ordering};

    let started = Arc::new(AtomicBool::new(false));
    let started_clone = started.clone();

    let mut t = Tween::to_position(100.0, 0.0, 1.0)
      .with_delay(0.5)
      .on_start(move |_| {
        started_clone.store(true, Ordering::SeqCst);
      });
    let mut transform = Transform::from_xy(0.0, 0.0);

    // During delay, on_start not called
    t.update(0.3, &mut transform);
    assert!(!started.load(Ordering::SeqCst));

    // After delay, on_start called
    t.update(0.3, &mut transform);
    assert!(started.load(Ordering::SeqCst));
  }

  // ===== Color/Alpha Tween Tests =====

  #[test]
  fn tween_to_alpha() {
    let t = Tween::fade_to(0.5, 1.0);
    assert_eq!(t.target_alpha, Some(0.5));
    assert_eq!(t.duration, 1.0);
  }

  #[test]
  fn tween_fade_out() {
    let t = Tween::fade_out(0.5);
    assert_eq!(t.target_alpha, Some(0.0));
  }

  #[test]
  fn tween_fade_in() {
    let t = Tween::fade_in(0.5);
    assert_eq!(t.target_alpha, Some(1.0));
  }

  #[test]
  fn tween_to_tint() {
    let t = Tween::tint_to(Color::RED, 1.0);
    assert_eq!(t.target_tint, Some(Color::RED));
  }

  #[test]
  fn tween_has_color_targets() {
    let t1 = Tween::to_position(100.0, 100.0, 1.0);
    assert!(!t1.has_color_targets());

    let t2 = Tween::fade_to(0.5, 1.0);
    assert!(t2.has_color_targets());

    let t3 = Tween::tint_to(Color::RED, 1.0);
    assert!(t3.has_color_targets());
  }

  #[test]
  fn tween_current_alpha() {
    let mut t = Tween::fade_to(0.0, 1.0);
    let mut transform = Transform::from_xy(0.0, 0.0);

    // Capture start values
    t.capture_start_renderable(1.0, Color::WHITE);
    t.update(0.5, &mut transform);

    let alpha = t.current_alpha().expect("Should have alpha");
    assert!(
      (alpha - 0.5).abs() < 0.01,
      "Alpha should be ~0.5, got {}",
      alpha
    );
  }

  #[test]
  fn tween_current_tint() {
    let mut t = Tween::tint_to(Color::RED, 1.0);
    let mut transform = Transform::from_xy(0.0, 0.0);

    // Capture start values (WHITE to RED)
    t.capture_start_renderable(1.0, Color::WHITE);
    t.update(0.5, &mut transform);

    let tint = t.current_tint().expect("Should have tint");
    // At 50%, should be halfway between WHITE(255,255,255) and RED(255,0,0)
    // R stays 255, G and B go from 255 to 0, so at 50% should be ~128
    assert_eq!(tint.r, 255);
    assert!(
      (tint.g as i32 - 128).abs() <= 1,
      "G should be ~128, got {}",
      tint.g
    );
    assert!(
      (tint.b as i32 - 128).abs() <= 1,
      "B should be ~128, got {}",
      tint.b
    );
  }

  #[test]
  fn lerp_color_works() {
    let start = Color::new(0, 0, 0, 255);
    let end = Color::new(100, 200, 50, 255);

    let mid = lerp_color(start, end, 0.5);
    assert_eq!(mid.r, 50);
    assert_eq!(mid.g, 100);
    assert_eq!(mid.b, 25);
    assert_eq!(mid.a, 255);
  }

  #[test]
  fn tween_alpha_clamps() {
    let t = Tween::new(1.0).to_alpha(2.0);
    assert_eq!(t.target_alpha, Some(1.0));

    let t2 = Tween::new(1.0).to_alpha(-0.5);
    assert_eq!(t2.target_alpha, Some(0.0));
  }

  // ===== TweenSequence Tests =====

  #[test]
  fn sequence_new() {
    let seq = TweenSequence::new();
    assert!(seq.is_empty());
    assert_eq!(seq.len(), 0);
    assert!(seq.playing);
  }

  #[test]
  fn sequence_then_adds_tweens() {
    let seq = TweenSequence::new()
      .then(Tween::to_position(100.0, 0.0, 1.0))
      .then(Tween::to_position(200.0, 0.0, 1.0));

    assert_eq!(seq.len(), 2);
    assert!(!seq.is_empty());
  }

  #[test]
  fn sequence_current() {
    let seq = TweenSequence::new()
      .then(Tween::to_position(100.0, 0.0, 1.0))
      .then(Tween::to_position(200.0, 0.0, 1.0));

    let current = seq.current().expect("Should have current tween");
    assert_eq!(current.target_x, Some(100.0));
    assert_eq!(seq.current_index(), 0);
  }

  #[test]
  fn sequence_advance() {
    let mut seq = TweenSequence::new()
      .then(Tween::to_position(100.0, 0.0, 1.0))
      .then(Tween::to_position(200.0, 0.0, 1.0));

    assert_eq!(seq.current_index(), 0);

    seq.advance();
    assert_eq!(seq.current_index(), 1);
    let current = seq.current().expect("Should have current tween");
    assert_eq!(current.target_x, Some(200.0));

    seq.advance();
    assert!(seq.is_finished());
    assert!(seq.just_finished);
    assert!(!seq.playing);
  }

  #[test]
  fn sequence_loop() {
    let mut seq = TweenSequence::new()
      .then(Tween::to_position(100.0, 0.0, 1.0))
      .then(Tween::to_position(200.0, 0.0, 1.0))
      .with_loop();

    seq.advance();
    seq.advance();

    // Should loop back to start
    assert_eq!(seq.current_index(), 0);
    assert!(!seq.is_finished());
    assert!(seq.playing);
  }

  #[test]
  fn sequence_pause_resume() {
    let mut seq = TweenSequence::new().then(Tween::to_position(100.0, 0.0, 1.0));

    assert!(seq.playing);

    seq.pause();
    assert!(!seq.playing);

    seq.resume();
    assert!(seq.playing);
  }

  #[test]
  fn test_tween_sequence_speed_default() {
    let seq = TweenSequence::new();
    assert_eq!(seq.speed, 1.0);
  }

  #[test]
  fn test_tween_sequence_with_speed() {
    let seq = TweenSequence::new().with_speed(2.0);
    assert_eq!(seq.speed, 2.0);
  }

  #[test]
  fn test_tween_sequence_set_speed() {
    let mut seq = TweenSequence::new();
    seq.set_speed(0.5);
    assert_eq!(seq.speed, 0.5);
  }

  #[test]
  fn sequence_stop_restart() {
    let mut seq = TweenSequence::new()
      .then(Tween::to_position(100.0, 0.0, 1.0))
      .then(Tween::to_position(200.0, 0.0, 1.0));

    seq.advance();
    assert_eq!(seq.current_index(), 1);

    seq.stop();
    assert_eq!(seq.current_index(), 0);
    assert!(!seq.playing);

    seq.restart();
    assert_eq!(seq.current_index(), 0);
    assert!(seq.playing);
  }
}
