//! Generic property tweening component.
//!
//! [`PropertyTween<T>`] interpolates any [`Lerp`]-implementing type over time,
//! using the same easing curves, loop modes, and callbacks as the Transform-based
//! [`super::Tween`].
//!
//! # Example
//!
//! ```ignore
//! use ryo2d::prelude::*;
//!
//! // Tween a float from 0 to 100 over 2 seconds
//! let mut tween = PropertyTween::new(0.0_f32, 100.0, 2.0)
//!     .with_easing(Ease::OutQuad);
//!
//! // Each frame:
//! tween.update(dt);
//! let val = tween.value(); // smoothly goes 0.0 -> 100.0
//!
//! // Tween a Vec2
//! let mut pos = PropertyTween::new(Vec2::ZERO, Vec2::new(400.0, 300.0), 1.5)
//!     .with_easing(Ease::OutBack)
//!     .with_loop(TweenLoop::PingPong);
//! ```

use std::sync::Arc;

use super::lerp::Lerp;
use super::tween::{Ease, TweenCallback, TweenLoop};

/// A generic tween component that interpolates any [`Lerp`]-implementing type.
///
/// Unlike [`super::Tween`] which operates on Transform fields, `PropertyTween<T>`
/// stores its own start/end/current values and can be used for arbitrary data.
///
/// Create with [`PropertyTween::new`] and advance each frame with [`PropertyTween::update`].
/// Read the interpolated value with [`PropertyTween::value`].
pub struct PropertyTween<T: Lerp> {
  start: T,
  end: T,
  current: T,
  duration: f32,
  elapsed: f32,
  delay: f32,
  easing: Ease,
  loop_mode: TweenLoop,
  speed: f32,
  playing: bool,
  started: bool,
  just_started: bool,
  just_finished: bool,
  ping_pong_reverse: bool,
  loop_count: u32,
  on_start: Option<TweenCallback>,
  on_update: Option<TweenCallback>,
  on_complete: Option<TweenCallback>,
}

impl<T: Lerp> PropertyTween<T> {
  /// Create a new property tween interpolating from `start` to `end` over `duration` seconds.
  ///
  /// The tween starts playing immediately. `current` is initialized to `start`.
  pub fn new(start: T, end: T, duration: f32) -> Self {
    let current = start.clone();
    Self {
      start,
      end,
      current,
      duration,
      elapsed: 0.0,
      delay: 0.0,
      easing: Ease::Linear,
      loop_mode: TweenLoop::Once,
      speed: 1.0,
      playing: true,
      started: false,
      just_started: false,
      just_finished: false,
      ping_pong_reverse: false,
      loop_count: 0,
      on_start: None,
      on_update: None,
      on_complete: None,
    }
  }

  // ===== Builder methods =====

  /// Set the easing function.
  pub fn with_easing(mut self, ease: Ease) -> Self {
    self.easing = ease;
    self
  }

  /// Set a delay before the tween starts (in seconds).
  pub fn with_delay(mut self, seconds: f32) -> Self {
    self.delay = seconds;
    self
  }

  /// Set the loop mode.
  pub fn with_loop(mut self, mode: TweenLoop) -> Self {
    self.loop_mode = mode;
    self
  }

  /// Set the playback speed multiplier.
  ///
  /// - `1.0` = normal speed
  /// - `2.0` = double speed (completes in half the time)
  /// - `0.5` = half speed (takes twice as long)
  pub fn with_speed(mut self, speed: f32) -> Self {
    self.speed = speed;
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

  /// Set a callback to be called every frame while the tween is active.
  ///
  /// Receives the eased progress value (0.0 to 1.0).
  pub fn on_update<F>(mut self, callback: F) -> Self
  where
    F: Fn(f32) + Send + Sync + 'static,
  {
    self.on_update = Some(Arc::new(callback));
    self
  }

  /// Set a callback to be called when the tween completes a cycle.
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

  // ===== Getters =====

  /// Returns the current interpolated value.
  pub fn value(&self) -> T {
    self.current.clone()
  }

  /// Returns raw progress (0.0 to 1.0), computed as `elapsed / duration`.
  pub fn progress(&self) -> f32 {
    if self.duration <= 0.0 {
      1.0
    } else {
      (self.elapsed / self.duration).clamp(0.0, 1.0)
    }
  }

  /// Whether the tween is currently playing.
  pub fn playing(&self) -> bool {
    self.playing
  }

  /// Whether the tween has completed (not playing AND has started).
  pub fn is_finished(&self) -> bool {
    !self.playing && self.started
  }

  /// True for exactly one frame when the tween starts (after delay).
  pub fn just_started(&self) -> bool {
    self.just_started
  }

  /// True for exactly one frame when the tween finishes a cycle.
  pub fn just_finished(&self) -> bool {
    self.just_finished
  }

  // ===== Control =====

  /// Pause the tween (stops advancing but not finished).
  pub fn pause(&mut self) {
    self.playing = false;
  }

  /// Resume a paused tween.
  pub fn resume(&mut self) {
    self.playing = true;
  }

  /// Stop and mark as done. The current value is preserved.
  ///
  /// Unlike [`Tween::stop`] which resets to the beginning,
  /// this freezes the tween at its current interpolated value.
  pub fn stop(&mut self) {
    self.playing = false;
    self.started = true;
  }

  /// Reset to beginning and start playing.
  pub fn restart(&mut self) {
    self.elapsed = 0.0;
    self.playing = true;
    self.started = false;
    self.just_started = false;
    self.just_finished = false;
    self.ping_pong_reverse = false;
    self.loop_count = 0;
    self.current = self.start.clone();
  }

  /// Change destination mid-flight.
  ///
  /// Sets `start` to the current interpolated value, resets elapsed to 0,
  /// and updates `end` to the new target. The `on_start` callback will fire
  /// again on the next update since the tween is effectively restarting.
  pub fn retarget(&mut self, new_end: T) {
    self.start = self.current.clone();
    self.end = new_end;
    self.elapsed = 0.0;
    self.playing = true;
    self.started = false;
    self.ping_pong_reverse = false;
    self.loop_count = 0;
  }

  // ===== Core update =====

  /// Advance the tween by `dt` seconds.
  ///
  /// This handles delay, speed, easing, interpolation, callbacks, and loop modes.
  pub fn update(&mut self, dt: f32) {
    // Reset one-frame flags from the previous frame
    self.just_started = false;
    self.just_finished = false;

    if !self.playing {
      return;
    }

    // Apply speed multiplier to delta time
    let dt = dt * self.speed;

    // Handle delay (delay uses unscaled time, like Tween does)
    if self.delay > 0.0 {
      self.delay -= dt / self.speed.max(0.001); // Use unscaled dt for delay
      if self.delay > 0.0 {
        return;
      }
      // Delay finished — capture overshoot time scaled by speed
      let overshoot = -self.delay * self.speed;
      self.delay = 0.0;

      // Mark as started
      if !self.started {
        self.started = true;
        self.just_started = true;
        if let Some(ref cb) = self.on_start {
          cb(0.0);
        }
      }

      self.elapsed = overshoot;
    } else {
      // Normal update: mark started if needed, then add dt
      if !self.started {
        self.started = true;
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

    // Compute interpolated value
    self.current = T::lerp(&self.start, &self.end, eased);

    // Fire on_update callback with eased progress
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

      // Fire on_complete callback
      if self.just_finished
        && let Some(ref cb) = self.on_complete
      {
        cb(1.0);
      }
    }
  }
}

impl<T: Lerp + std::fmt::Debug> std::fmt::Debug for PropertyTween<T> {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    f.debug_struct("PropertyTween")
      .field("start", &self.start)
      .field("end", &self.end)
      .field("current", &self.current)
      .field("duration", &self.duration)
      .field("elapsed", &self.elapsed)
      .field("delay", &self.delay)
      .field("easing", &self.easing)
      .field("loop_mode", &self.loop_mode)
      .field("speed", &self.speed)
      .field("playing", &self.playing)
      .field("started", &self.started)
      .field("just_started", &self.just_started)
      .field("just_finished", &self.just_finished)
      .field("on_start", &self.on_start.as_ref().map(|_| "Fn(f32)"))
      .field("on_update", &self.on_update.as_ref().map(|_| "Fn(f32)"))
      .field("on_complete", &self.on_complete.as_ref().map(|_| "Fn(f32)"))
      .finish()
  }
}

impl<T: Lerp> Clone for PropertyTween<T> {
  fn clone(&self) -> Self {
    Self {
      start: self.start.clone(),
      end: self.end.clone(),
      current: self.current.clone(),
      duration: self.duration,
      elapsed: self.elapsed,
      delay: self.delay,
      easing: self.easing,
      loop_mode: self.loop_mode,
      speed: self.speed,
      playing: self.playing,
      started: self.started,
      just_started: self.just_started,
      just_finished: self.just_finished,
      ping_pong_reverse: self.ping_pong_reverse,
      loop_count: self.loop_count,
      on_start: self.on_start.clone(),
      on_update: self.on_update.clone(),
      on_complete: self.on_complete.clone(),
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use glam::Vec2;
  use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
  use std::sync::{Arc, Mutex};

  // 1. new_starts_playing — new tween is playing, value equals start
  #[test]
  fn new_starts_playing() {
    let tween = PropertyTween::new(0.0_f32, 100.0, 1.0);
    assert!(tween.playing());
    assert!(!tween.is_finished());
    assert_eq!(tween.value(), 0.0);
  }

  // 2. value_interpolates_linearly — at halfway, value is midpoint
  #[test]
  fn value_interpolates_linearly() {
    let mut tween = PropertyTween::new(0.0_f32, 100.0, 1.0);
    tween.update(0.5);
    assert!((tween.value() - 50.0).abs() < 0.01);
  }

  // 3. value_with_easing — InQuad at 0.5 should give 0.25 progress
  #[test]
  fn value_with_easing() {
    let mut tween = PropertyTween::new(0.0_f32, 100.0, 1.0).with_easing(Ease::InQuad);
    tween.update(0.5);
    // InQuad at t=0.5 gives 0.25, so value should be 25.0
    assert!((tween.value() - 25.0).abs() < 0.01);
  }

  // 4. completes_after_duration — value equals end, not playing, is_finished
  #[test]
  fn completes_after_duration() {
    let mut tween = PropertyTween::new(0.0_f32, 100.0, 1.0);
    tween.update(1.0);
    assert!((tween.value() - 100.0).abs() < 0.01);
    assert!(!tween.playing());
    assert!(tween.is_finished());
  }

  // 5. delay_postpones_start — value stays at start during delay
  #[test]
  fn delay_postpones_start() {
    let mut tween = PropertyTween::new(0.0_f32, 100.0, 1.0).with_delay(0.5);

    // During delay, value should remain at start
    tween.update(0.3);
    assert_eq!(tween.value(), 0.0);

    // After delay completes, tween starts with overshoot
    tween.update(0.3);
    // 0.6 total - 0.5 delay = 0.1 seconds of tween time -> 10% progress
    assert!((tween.value() - 10.0).abs() < 0.5);
  }

  // 6. loop_mode_restarts — keeps playing after duration
  #[test]
  fn loop_mode_restarts() {
    let mut tween = PropertyTween::new(0.0_f32, 100.0, 1.0).with_loop(TweenLoop::Loop);

    tween.update(1.0);
    assert!(tween.playing());
    assert!(tween.just_finished());

    // After looping, continues interpolating
    tween.update(0.5);
    assert!((tween.value() - 50.0).abs() < 0.01);
  }

  // 7. ping_pong_reverses — goes backward after completion
  #[test]
  fn ping_pong_reverses() {
    let mut tween = PropertyTween::new(0.0_f32, 100.0, 1.0).with_loop(TweenLoop::PingPong);

    // Forward to end
    tween.update(1.0);
    assert!((tween.value() - 100.0).abs() < 0.01);

    // Backward — at 0.5s into reverse, should be at 50
    tween.update(0.5);
    assert!((tween.value() - 50.0).abs() < 0.01);

    // Back to start
    tween.update(0.5);
    assert!((tween.value() - 0.0).abs() < 0.01);
  }

  // 8. times_mode — stops after N iterations
  #[test]
  fn times_mode() {
    let mut tween = PropertyTween::new(0.0_f32, 100.0, 1.0).with_loop(TweenLoop::Times(2));

    // First iteration
    tween.update(1.0);
    assert!(tween.playing());

    // Second iteration
    tween.update(1.0);
    assert!(!tween.playing());
    assert!(tween.is_finished());
  }

  // 9. speed_multiplier — double speed reaches halfway in half time
  #[test]
  fn speed_multiplier() {
    let mut tween = PropertyTween::new(0.0_f32, 100.0, 1.0).with_speed(2.0);

    // At 2x speed, 0.25 seconds should give 50% progress
    tween.update(0.25);
    assert!((tween.value() - 50.0).abs() < 0.01);

    // Completes in half the time
    tween.update(0.25);
    assert!((tween.value() - 100.0).abs() < 0.01);
    assert!(tween.just_finished());
  }

  // 10. pause_and_resume — paused tween doesn't advance
  #[test]
  fn pause_and_resume() {
    let mut tween = PropertyTween::new(0.0_f32, 100.0, 1.0);

    tween.update(0.5);
    assert!((tween.value() - 50.0).abs() < 0.01);

    tween.pause();
    tween.update(0.5);
    // Should not have advanced
    assert!((tween.value() - 50.0).abs() < 0.01);

    tween.resume();
    tween.update(0.5);
    assert!((tween.value() - 100.0).abs() < 0.01);
  }

  // 11. retarget_changes_destination — new end from current position
  #[test]
  fn retarget_changes_destination() {
    let mut tween = PropertyTween::new(0.0_f32, 100.0, 1.0);

    tween.update(0.5);
    assert!((tween.value() - 50.0).abs() < 0.01);

    // Retarget to 200 from current value (50)
    tween.retarget(200.0);

    // Now halfway through the new tween should be at 125 (midpoint of 50..200)
    tween.update(0.5);
    assert!((tween.value() - 125.0).abs() < 0.01);
  }

  // 12. progress_reports_correctly — matches elapsed/duration
  #[test]
  fn progress_reports_correctly() {
    let mut tween = PropertyTween::new(0.0_f32, 100.0, 2.0);

    assert_eq!(tween.progress(), 0.0);

    tween.update(1.0);
    assert!((tween.progress() - 0.5).abs() < 0.01);

    tween.update(1.0);
    assert!((tween.progress() - 1.0).abs() < 0.01);
  }

  // 13. vec2_lerp — works with Vec2 type
  #[test]
  fn vec2_lerp() {
    let mut tween = PropertyTween::new(Vec2::ZERO, Vec2::new(100.0, 200.0), 1.0);

    tween.update(0.5);
    let val = tween.value();
    assert!((val.x - 50.0).abs() < 0.01);
    assert!((val.y - 100.0).abs() < 0.01);
  }

  // 14. callbacks_receive_progress — on_update gets eased progress value
  #[test]
  fn callbacks_receive_progress() {
    let received = Arc::new(Mutex::new(Vec::<f32>::new()));
    let received_clone = received.clone();

    let mut tween = PropertyTween::new(0.0_f32, 100.0, 1.0).on_update(move |progress| {
      received_clone.lock().unwrap().push(progress);
    });

    tween.update(0.5);
    tween.update(0.5);

    let values = received.lock().unwrap();
    assert_eq!(values.len(), 2);
    // Linear easing: eased == raw progress
    assert!((values[0] - 0.5).abs() < 0.01);
    assert!((values[1] - 1.0).abs() < 0.01);
  }

  // 15. just_started_and_just_finished_flags — true for exactly one frame
  #[test]
  fn just_started_and_just_finished_flags() {
    let mut tween = PropertyTween::new(0.0_f32, 100.0, 1.0);

    assert!(!tween.just_started());
    assert!(!tween.just_finished());

    // First update: just_started should be true
    tween.update(0.1);
    assert!(tween.just_started());
    assert!(!tween.just_finished());

    // Second update: just_started should be false
    tween.update(0.1);
    assert!(!tween.just_started());
    assert!(!tween.just_finished());

    // Complete: just_finished should be true
    tween.update(1.0);
    assert!(!tween.just_started());
    assert!(tween.just_finished());

    // Next frame: just_finished should be cleared
    tween.update(0.1);
    assert!(!tween.just_finished());
  }

  // 16. stop_method — stops and marks finished
  #[test]
  fn stop_method() {
    let mut tween = PropertyTween::new(0.0_f32, 100.0, 1.0);

    tween.update(0.5);
    assert!(tween.playing());

    tween.stop();
    assert!(!tween.playing());
    assert!(tween.is_finished()); // started=true, playing=false

    // Should not advance after stop
    tween.update(0.5);
    assert!((tween.value() - 50.0).abs() < 0.01); // value unchanged
  }

  // 17. restart_method — resets and replays
  #[test]
  fn restart_method() {
    let mut tween = PropertyTween::new(0.0_f32, 100.0, 1.0);

    tween.update(1.0);
    assert!(tween.is_finished());

    tween.restart();
    assert!(tween.playing());
    assert!(!tween.is_finished());
    assert_eq!(tween.value(), 0.0); // reset to start
    assert_eq!(tween.progress(), 0.0);

    // Can play again
    tween.update(0.5);
    assert!((tween.value() - 50.0).abs() < 0.01);
  }

  // Additional: on_start callback fires after delay
  #[test]
  fn on_start_fires_after_delay() {
    let started = Arc::new(AtomicBool::new(false));
    let started_clone = started.clone();

    let mut tween = PropertyTween::new(0.0_f32, 100.0, 1.0)
      .with_delay(0.5)
      .on_start(move |_| {
        started_clone.store(true, Ordering::SeqCst);
      });

    tween.update(0.3);
    assert!(!started.load(Ordering::SeqCst));

    tween.update(0.3);
    assert!(started.load(Ordering::SeqCst));
  }

  // Additional: on_complete fires with 1.0
  #[test]
  fn on_complete_fires_with_one() {
    let received = Arc::new(Mutex::new(None::<f32>));
    let received_clone = received.clone();

    let mut tween = PropertyTween::new(0.0_f32, 100.0, 1.0).on_complete(move |progress| {
      *received_clone.lock().unwrap() = Some(progress);
    });

    tween.update(1.0);
    assert_eq!(*received.lock().unwrap(), Some(1.0));
  }

  // Additional: on_start only fires once
  #[test]
  fn on_start_only_fires_once() {
    let count = Arc::new(AtomicUsize::new(0));
    let count_clone = count.clone();

    let mut tween = PropertyTween::new(0.0_f32, 100.0, 1.0).on_start(move |_| {
      count_clone.fetch_add(1, Ordering::SeqCst);
    });

    tween.update(0.1);
    tween.update(0.1);
    tween.update(0.1);

    assert_eq!(count.load(Ordering::SeqCst), 1);
  }

  // Additional: loop on_complete fires each iteration
  #[test]
  fn loop_on_complete_fires_each_iteration() {
    let count = Arc::new(AtomicUsize::new(0));
    let count_clone = count.clone();

    let mut tween = PropertyTween::new(0.0_f32, 100.0, 0.5)
      .with_loop(TweenLoop::Times(3))
      .on_complete(move |_| {
        count_clone.fetch_add(1, Ordering::SeqCst);
      });

    tween.update(0.5); // iteration 1
    tween.update(0.5); // iteration 2
    tween.update(0.5); // iteration 3

    assert_eq!(count.load(Ordering::SeqCst), 3);
  }

  #[test]
  fn zero_duration_completes_instantly() {
    let mut tween = PropertyTween::new(0.0f32, 100.0, 0.0);
    tween.update(0.016);
    assert!(tween.is_finished());
    assert!(!tween.playing());
    assert!((tween.value() - 100.0).abs() < 0.01);
  }
}
