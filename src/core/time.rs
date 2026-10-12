//! The engine's clocks.
//!
//! `real` is the wall clock. `game` is real time scaled, pausable and capped;
//! everything the player perceives as game time reads it. `fixed` accumulates
//! game time into constant steps that drive `FixedUpdate`.

use std::time::{Duration, Instant};

use crate::core::ecs::kernel::Resource;

/// Wall-clock time: never scaled, paused or capped.
#[derive(Debug, Clone, Copy, Default)]
pub struct RealClock {
  delta: Duration,
  elapsed: Duration,
}

impl RealClock {
  pub fn delta(&self) -> f32 {
    self.delta.as_secs_f32()
  }

  pub fn delta_duration(&self) -> Duration {
    self.delta
  }

  pub fn elapsed(&self) -> f32 {
    self.elapsed.as_secs_f32()
  }
}

/// Game time: real time times `speed`, zero while paused or in hitstop,
/// and never more than `max_delta` (or one fixed timestep, if longer) of
/// real time per frame.
#[derive(Debug, Clone)]
pub struct GameClock {
  delta: Duration,
  elapsed: Duration,
  speed: f32,
  paused: bool,
  hitstop: Duration,
  max_delta: Duration,
}

impl Default for GameClock {
  fn default() -> Self {
    Self {
      delta: Duration::ZERO,
      elapsed: Duration::ZERO,
      speed: 1.0,
      paused: false,
      hitstop: Duration::ZERO,
      max_delta: Duration::from_millis(250),
    }
  }
}

impl GameClock {
  /// The fastest `set_speed` allows.
  pub const MAX_SPEED: f32 = 100.0;

  pub fn delta(&self) -> f32 {
    self.delta.as_secs_f32()
  }

  pub fn delta_duration(&self) -> Duration {
    self.delta
  }

  pub fn elapsed(&self) -> f32 {
    self.elapsed.as_secs_f32()
  }

  /// Multiplier on real time: 1 is normal, 0.5 is slow-mo, 0 freezes.
  pub fn speed(&self) -> f32 {
    self.speed
  }

  /// Speeds clamp to `0..=MAX_SPEED`; a non-finite speed resets to 1.
  pub fn set_speed(&mut self, speed: f32) {
    self.speed = if speed.is_finite() {
      speed.clamp(0.0, Self::MAX_SPEED)
    } else {
      1.0
    };
  }

  pub fn pause(&mut self) {
    self.paused = true;
  }

  pub fn resume(&mut self) {
    self.paused = false;
  }

  pub fn is_paused(&self) -> bool {
    self.paused
  }

  /// Freeze game time for `seconds` of real time (a longer hitstop replaces a
  /// shorter one). One too long to represent lasts until `clear_hitstop`.
  pub fn hitstop(&mut self, seconds: f32) {
    let requested = Duration::try_from_secs_f32(seconds.max(0.0)).unwrap_or(Duration::MAX);
    self.hitstop = self.hitstop.max(requested);
  }

  pub fn is_hitstop(&self) -> bool {
    !self.hitstop.is_zero()
  }

  /// End any hitstop now.
  pub fn clear_hitstop(&mut self) {
    self.hitstop = Duration::ZERO;
  }

  /// The most real time one frame may add to game time. A fixed timestep
  /// longer than this raises the cap to one step.
  pub fn max_delta(&self) -> Duration {
    self.max_delta
  }

  pub fn set_max_delta(&mut self, max_delta: Duration) {
    self.max_delta = max_delta;
  }

  fn advance(&mut self, real_delta: Duration, min_cap: Duration) {
    if !self.hitstop.is_zero() {
      self.hitstop = self.hitstop.saturating_sub(real_delta);
      self.delta = Duration::ZERO;
      return;
    }
    if self.paused {
      self.delta = Duration::ZERO;
      return;
    }
    let capped = real_delta.min(self.max_delta.max(min_cap));
    self.delta = capped.mul_f32(self.speed);
    self.elapsed += self.delta;
  }
}

/// Constant-step time for `FixedUpdate`.
#[derive(Debug, Clone)]
pub struct FixedClock {
  timestep: Duration,
  accumulator: Duration,
  steps: u64,
}

impl FixedClock {
  fn with_hz(hz: u32) -> Self {
    Self {
      timestep: Self::timestep_for(hz),
      accumulator: Duration::ZERO,
      steps: 0,
    }
  }

  fn timestep_for(hz: u32) -> Duration {
    Duration::from_secs_f64(1.0 / f64::from(hz.max(1)))
  }

  pub fn timestep(&self) -> f32 {
    self.timestep.as_secs_f32()
  }

  pub fn timestep_duration(&self) -> Duration {
    self.timestep
  }

  /// Change the fixed rate; a rate of 0 clamps to 1 Hz.
  pub fn set_hz(&mut self, hz: u32) {
    self.timestep = Self::timestep_for(hz);
  }

  pub fn accumulator(&self) -> f32 {
    self.accumulator.as_secs_f32()
  }

  /// How far the next fixed step is through: the render interpolation alpha, in `[0, 1)`.
  pub fn overstep_fraction(&self) -> f32 {
    (self.accumulator.as_secs_f32() / self.timestep.as_secs_f32()).clamp(0.0, 0.999_999)
  }

  /// Fixed steps run since the engine started.
  pub fn step_count(&self) -> u64 {
    self.steps
  }
}

/// The engine's clocks, a resource and `SceneContext::time`.
#[derive(Resource, Debug, Clone)]
pub struct Time {
  pub real: RealClock,
  pub game: GameClock,
  pub fixed: FixedClock,
  frame_count: u64,
  frame_start: Option<Instant>,
  in_fixed: bool,
}

impl Time {
  pub fn new(fixed_hz: u32) -> Self {
    Self {
      real: RealClock::default(),
      game: GameClock::default(),
      fixed: FixedClock::with_hz(fixed_hz),
      frame_count: 0,
      frame_start: None,
      in_fixed: false,
    }
  }

  /// The delta the current code should integrate with: the fixed timestep
  /// inside `FixedUpdate`, the game clock's frame delta elsewhere.
  pub fn delta(&self) -> f32 {
    if self.in_fixed {
      self.fixed.timestep()
    } else {
      self.game.delta()
    }
  }

  pub fn frame_count(&self) -> u64 {
    self.frame_count
  }

  /// Advance by the wall-clock time since the previous call (zero on the first).
  pub fn begin_frame(&mut self) {
    let now = Instant::now();
    let delta = self
      .frame_start
      .map(|start| now.duration_since(start))
      .unwrap_or(Duration::ZERO);
    self.frame_start = Some(now);
    self.advance(delta);
  }

  /// Advance by an explicit real duration (headless runs and tests).
  pub fn advance(&mut self, real_delta: Duration) {
    self.real.delta = real_delta;
    self.real.elapsed += real_delta;
    self.game.advance(real_delta, self.fixed.timestep);
    self.fixed.accumulator += self.game.delta;
    self.frame_count += 1;
  }

  /// Consume one fixed step if the accumulator holds one.
  pub(crate) fn try_expend_fixed_step(&mut self) -> bool {
    if self.fixed.accumulator >= self.fixed.timestep {
      self.fixed.accumulator -= self.fixed.timestep;
      self.fixed.steps += 1;
      true
    } else {
      false
    }
  }

  pub(crate) fn set_in_fixed(&mut self, in_fixed: bool) {
    self.in_fixed = in_fixed;
  }

  /// The instant the current frame started, for frame pacing.
  pub fn frame_start(&self) -> Option<Instant> {
    self.frame_start
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use std::time::Duration;

  fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
  }

  #[test]
  fn fixed_steps_accumulate_across_uneven_frames() {
    let mut t = Time::new(60);
    t.advance(ms(10));
    assert!(!t.try_expend_fixed_step());
    t.advance(ms(10));
    assert!(t.try_expend_fixed_step());
    assert!(!t.try_expend_fixed_step());
    assert_eq!(t.fixed.step_count(), 1);
  }

  #[test]
  fn max_delta_caps_a_stall() {
    let mut t = Time::new(60);
    t.advance(Duration::from_secs(10));
    let mut steps = 0;
    while t.try_expend_fixed_step() {
      steps += 1;
    }
    // 250 ms at 60 Hz is 15 steps; the timestep rounds to whole nanoseconds,
    // so allow one step of slack either way. The point is that it is not 600.
    assert!((14..=15).contains(&steps), "{steps}");
    assert!(
      (t.real.delta() - 10.0).abs() < 1e-6,
      "real clock is never capped"
    );
  }

  #[test]
  fn a_frame_one_timestep_long_runs_a_step_at_any_rate() {
    let mut t = Time::new(2);
    t.advance(t.fixed.timestep_duration());
    assert!(t.try_expend_fixed_step(), "500 ms is longer than max_delta");
  }

  #[test]
  fn half_speed_halves_the_step_count() {
    let mut full = Time::new(60);
    let mut half = Time::new(60);
    half.game.set_speed(0.5);
    let (mut a, mut b) = (0, 0);
    for _ in 0..120 {
      full.advance(ms(16));
      half.advance(ms(16));
      while full.try_expend_fixed_step() {
        a += 1;
      }
      while half.try_expend_fixed_step() {
        b += 1;
      }
    }
    assert!((a as f32 / b as f32 - 2.0).abs() < 0.1, "{a} vs {b}");
  }

  #[test]
  fn pause_and_hitstop_run_no_steps_and_hitstop_ends_in_real_time() {
    let mut t = Time::new(60);
    t.game.pause();
    t.advance(ms(100));
    assert!(!t.try_expend_fixed_step());
    assert_eq!(t.game.delta(), 0.0);
    t.game.resume();

    t.game.hitstop(0.05);
    assert!(t.game.is_hitstop());
    t.advance(ms(30));
    assert!(!t.try_expend_fixed_step());
    t.advance(ms(30));
    assert!(!t.game.is_hitstop(), "60 ms of real time passed");
    t.advance(ms(20));
    assert!(t.try_expend_fixed_step());
  }

  #[test]
  fn overstep_fraction_is_in_unit_range() {
    let mut t = Time::new(60);
    t.advance(ms(8));
    let a = t.fixed.overstep_fraction();
    assert!((0.0..1.0).contains(&a) && a > 0.4, "{a}");
    t.advance(ms(9)); // 17 ms accumulated: one step and a sliver
    assert!(t.try_expend_fixed_step());
    assert!(t.fixed.overstep_fraction() < 0.05);
  }

  #[test]
  fn delta_is_the_timestep_inside_fixed_and_the_game_delta_outside() {
    let mut t = Time::new(50);
    t.advance(ms(30));
    assert!((t.delta() - 0.030).abs() < 1e-6);
    t.set_in_fixed(true);
    assert!((t.delta() - 0.020).abs() < 1e-6);
    t.set_in_fixed(false);
    assert!((t.delta() - 0.030).abs() < 1e-6);
  }

  #[test]
  fn frame_count_and_elapsed_advance() {
    let mut t = Time::new(60);
    t.advance(ms(16));
    t.advance(ms(16));
    assert_eq!(t.frame_count(), 2);
    assert!((t.real.elapsed() - 0.032).abs() < 1e-6);
    assert!((t.game.elapsed() - 0.032).abs() < 1e-6);
  }

  #[test]
  fn invalid_rates_and_speeds_are_clamped() {
    let t = Time::new(0);
    assert!(
      (t.fixed.timestep() - 1.0).abs() < 1e-6,
      "0 Hz clamps to 1 Hz"
    );
    let mut t = Time::new(60);
    t.game.set_speed(-3.0);
    assert_eq!(t.game.speed(), 0.0);
    t.game.set_speed(f32::NAN);
    assert_eq!(t.game.speed(), 1.0, "a non-finite speed resets to normal");
    t.game.set_speed(f32::INFINITY);
    assert_eq!(t.game.speed(), 1.0);
  }

  #[test]
  fn a_huge_speed_is_capped() {
    let mut t = Time::new(60);
    t.game.set_speed(f32::MAX);
    assert_eq!(t.game.speed(), GameClock::MAX_SPEED);
    t.advance(ms(250));
    assert!((t.game.delta() - 0.25 * GameClock::MAX_SPEED).abs() < 1e-3);
  }

  #[test]
  fn an_infinite_hitstop_lasts_until_cleared() {
    let mut t = Time::new(60);
    t.game.hitstop(f32::INFINITY);
    t.advance(Duration::from_secs(3600));
    assert!(t.game.is_hitstop());
    t.game.clear_hitstop();
    t.advance(ms(20));
    assert!(t.try_expend_fixed_step());
  }

  #[test]
  fn clear_hitstop_ends_it_at_once() {
    let mut t = Time::new(60);
    t.game.hitstop(1.0);
    t.game.clear_hitstop();
    assert!(!t.game.is_hitstop());
    t.advance(ms(20));
    assert!(t.try_expend_fixed_step());
  }
}
