//! Timing system for the game loop.
//!
//! Manages frame timing, fixed timesteps, time scaling, and hitstop effects.
//! Consolidates timing logic that was previously split between `TimeState` (internal)
//! and `TimeControl` (shared with game code).

use std::time::{Duration, Instant};

use super::dev_log::set_frame;
use super::game::TimeControl;
use crate::engine_debug;

/// Internal timing state for the game loop.
///
/// Tracks frame timing, accumulator for fixed timestep, and time control effects.
#[derive(Debug)]
pub struct TimeState {
  /// When the current frame started
  frame_start: Instant,
  /// Time since last frame (variable)
  delta_time: Duration,
  /// Fixed timestep for physics/logic updates
  fixed_timestep: Duration,
  /// Accumulator for fixed timestep updates
  accumulator: Duration,
  /// Target frame duration for FPS limiting
  target_frame_time: Duration,
  /// Frame counter for debugging
  frame_count: u64,
  /// Last calculated FPS (updated each second)
  current_fps: u64,
  /// Last time we printed debug info
  last_debug_print: Instant,
  /// Time scale multiplier (0.0 = frozen, 1.0 = normal, >1.0 = fast)
  time_scale: f32,
  /// Remaining hitstop duration in seconds (freeze frame effect)
  hitstop_remaining: f32,
}

impl TimeState {
  /// Create a new TimeState with the given target FPS and physics rate.
  pub fn new(target_fps: u32, physics_hz: u32) -> Self {
    let now = Instant::now();
    TimeState {
      frame_start: now,
      delta_time: Duration::ZERO,
      fixed_timestep: Duration::from_secs_f64(1.0 / physics_hz as f64),
      accumulator: Duration::ZERO,
      target_frame_time: Duration::from_secs_f64(1.0 / target_fps as f64),
      frame_count: 0,
      current_fps: 0,
      last_debug_print: now,
      time_scale: 1.0,
      hitstop_remaining: 0.0,
    }
  }

  /// Begin a new frame, updating delta time and accumulator.
  pub fn begin_frame(&mut self) {
    let now = Instant::now();
    self.delta_time = now.duration_since(self.frame_start);
    self.frame_start = now;

    // Add delta to accumulator, but cap it to prevent spiral of death
    // If we fall too far behind, we just drop updates rather than freezing
    self.accumulator += self.delta_time;
    let max_accumulator = self.fixed_timestep * 5; // Max 5 physics updates per frame
    if self.accumulator > max_accumulator {
      self.accumulator = max_accumulator;
    }

    self.frame_count += 1;
    // Update dev_log frame number for log entries
    set_frame(self.frame_count);
  }

  /// Check if we have enough accumulated time for a fixed update.
  pub fn should_update(&self) -> bool {
    self.accumulator >= self.fixed_timestep
  }

  /// Consume one fixed timestep from the accumulator.
  pub fn consume_update(&mut self) {
    self.accumulator -= self.fixed_timestep;
  }

  /// Get the variable delta time (time since last frame).
  pub fn dt(&self) -> f32 {
    self.delta_time.as_secs_f32()
  }

  /// Get the fixed delta time (unscaled).
  pub fn fixed_dt(&self) -> f32 {
    self.fixed_timestep.as_secs_f32()
  }

  /// Get the fixed delta time scaled by time_scale, returning 0.0 during hitstop.
  ///
  /// This is used for animations, tweens, and game logic that should be affected
  /// by slow-mo and hitstop effects.
  pub fn scaled_fixed_dt(&self) -> f32 {
    if self.hitstop_remaining > 0.0 {
      0.0
    } else {
      self.fixed_timestep.as_secs_f32() * self.time_scale
    }
  }

  /// Update hitstop timer with real (unscaled) delta time.
  pub fn update_hitstop(&mut self, dt: f32) {
    if self.hitstop_remaining > 0.0 {
      self.hitstop_remaining = (self.hitstop_remaining - dt).max(0.0);
    }
  }

  /// Get the interpolation alpha for rendering between fixed updates.
  pub fn interpolation_alpha(&self) -> f32 {
    (self.accumulator.as_secs_f32() / self.fixed_timestep.as_secs_f32()).min(1.0)
  }

  /// Update the FPS counter and log debug timing info once per second. Returns true if updated.
  pub fn tick_fps_counter(&mut self) -> bool {
    let now = Instant::now();
    if now.duration_since(self.last_debug_print) >= Duration::from_secs(1) {
      self.current_fps = self.frame_count;
      engine_debug!(
        module: "GameLoop",
        "FPS: {}, Delta: {:.2}ms, Fixed DT: {:.2}ms",
        self.current_fps,
        self.delta_time.as_secs_f64() * 1000.0,
        self.fixed_timestep.as_secs_f64() * 1000.0
      );
      self.frame_count = 0;
      self.last_debug_print = now;
      true
    } else {
      false
    }
  }

  /// Wait precisely until target frame time using hybrid sleep/spin.
  ///
  /// This is more accurate than pure `thread::sleep()` which has ~1-15ms
  /// granularity depending on OS scheduler. We sleep for most of the time
  /// to save CPU, then spin-wait for the final microseconds for precision.
  pub fn wait_for_target_frame_time(&self) {
    let elapsed = self.frame_start.elapsed();
    if elapsed >= self.target_frame_time {
      return; // Already past target, no waiting needed
    }

    let remaining = self.target_frame_time - elapsed;

    // Sleep for most of the remaining time (save CPU)
    // Leave 1.5ms margin for spin-waiting to hit exact target
    let spin_margin = Duration::from_micros(1500);
    if remaining > spin_margin {
      std::thread::sleep(remaining - spin_margin);
    }

    // Spin-wait for precise timing (busy loop but very short)
    while self.frame_start.elapsed() < self.target_frame_time {
      std::hint::spin_loop();
    }
  }

  /// Get the current FPS (updated once per second).
  pub fn current_fps(&self) -> u64 {
    self.current_fps
  }

  /// Get the current frame count.
  pub fn frame_count(&self) -> u64 {
    self.frame_count
  }

  /// Get the frame start instant.
  pub fn frame_start(&self) -> Instant {
    self.frame_start
  }

  /// Get the target frame time.
  pub fn target_frame_time(&self) -> Duration {
    self.target_frame_time
  }

  /// Sync time scale from TimeControl (called before updates).
  ///
  /// This picks up any time_scale changes from game code.
  pub fn sync_from_time_control(&mut self, time_control: &TimeControl) {
    self.time_scale = time_control.time_scale;
    // Only use time_control hitstop if it's greater (allows game to trigger new hitstops)
    if time_control.hitstop_remaining > self.hitstop_remaining {
      self.hitstop_remaining = time_control.hitstop_remaining;
    }
  }

  /// Sync hitstop back to TimeControl (called after updates).
  ///
  /// This allows game code to read is_hitstop_active() correctly.
  pub fn sync_to_time_control(&self, time_control: &mut TimeControl) {
    time_control.hitstop_remaining = self.hitstop_remaining;
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn test_time_state_new() {
    let time = TimeState::new(60, 120);
    assert_eq!(time.time_scale, 1.0);
    assert_eq!(time.hitstop_remaining, 0.0);
    assert_eq!(time.frame_count, 0);
  }

  #[test]
  fn test_fixed_dt() {
    let time = TimeState::new(60, 120);
    // 120 Hz = ~8.33ms
    assert!((time.fixed_dt() - 1.0 / 120.0).abs() < 0.0001);
  }

  #[test]
  fn test_scaled_fixed_dt_normal() {
    let time = TimeState::new(60, 120);
    assert!((time.scaled_fixed_dt() - time.fixed_dt()).abs() < 0.0001);
  }

  #[test]
  fn test_scaled_fixed_dt_with_time_scale() {
    let mut time = TimeState::new(60, 120);
    time.time_scale = 0.5;
    assert!((time.scaled_fixed_dt() - time.fixed_dt() * 0.5).abs() < 0.0001);
  }

  #[test]
  fn test_scaled_fixed_dt_during_hitstop() {
    let mut time = TimeState::new(60, 120);
    time.hitstop_remaining = 0.1;
    assert_eq!(time.scaled_fixed_dt(), 0.0);
  }

  #[test]
  fn test_update_hitstop() {
    let mut time = TimeState::new(60, 120);
    time.hitstop_remaining = 0.1;
    time.update_hitstop(0.05);
    assert!((time.hitstop_remaining - 0.05).abs() < 0.0001);
    time.update_hitstop(0.1);
    assert_eq!(time.hitstop_remaining, 0.0);
  }

  #[test]
  fn test_should_update_and_consume() {
    let mut time = TimeState::new(60, 120);
    // Manually add to accumulator
    time.accumulator = time.fixed_timestep * 2;
    assert!(time.should_update());
    time.consume_update();
    assert!(time.should_update());
    time.consume_update();
    assert!(!time.should_update());
  }

  #[test]
  fn test_sync_from_time_control() {
    let mut time = TimeState::new(60, 120);
    let tc = TimeControl {
      time_scale: 0.5,
      hitstop_remaining: 0.2,
    };

    time.sync_from_time_control(&tc);
    assert_eq!(time.time_scale, 0.5);
    assert_eq!(time.hitstop_remaining, 0.2);
  }

  #[test]
  fn test_sync_from_time_control_hitstop_max() {
    let mut time = TimeState::new(60, 120);
    time.hitstop_remaining = 0.5; // Already higher

    let tc = TimeControl {
      hitstop_remaining: 0.2, // Lower value
      ..Default::default()
    };

    time.sync_from_time_control(&tc);
    // Should keep the higher value
    assert_eq!(time.hitstop_remaining, 0.5);
  }

  #[test]
  fn test_sync_to_time_control() {
    let mut time = TimeState::new(60, 120);
    time.hitstop_remaining = 0.1;

    let mut tc = TimeControl::default();
    time.sync_to_time_control(&mut tc);
    assert_eq!(tc.hitstop_remaining, 0.1);
  }

  #[test]
  fn test_interpolation_alpha() {
    let mut time = TimeState::new(60, 120);
    time.accumulator = Duration::ZERO;
    assert_eq!(time.interpolation_alpha(), 0.0);

    time.accumulator = time.fixed_timestep / 2;
    assert!((time.interpolation_alpha() - 0.5).abs() < 0.01);

    time.accumulator = time.fixed_timestep;
    assert!((time.interpolation_alpha() - 1.0).abs() < 0.01);

    // Clamped to 1.0
    time.accumulator = time.fixed_timestep * 2;
    assert_eq!(time.interpolation_alpha(), 1.0);
  }
}
