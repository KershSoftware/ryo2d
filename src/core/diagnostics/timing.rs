//! Frame timings: per-phase measurements, their smoothing and the
//! [`FrameCost`] resource.

use std::time::Duration;
use web_time::Instant;

/// Tracks timing for each phase of the game loop.
///
/// Used to display frame timing breakdown when `show_frame_timing` is enabled.
/// Measurements have minimal overhead (just Instant::now() calls).
///
/// Note: Currently, `update` includes physics time (combined in fixed timestep loop).
/// The `physics` field is reserved for future separate measurement.
#[derive(Debug, Clone, Default)]
pub struct FrameTimings {
  /// Time spent in game update logic (currently includes physics)
  pub update: Duration,
  /// Time spent in physics simulation (reserved for future use)
  pub physics: Duration,
  /// Time spent rendering (CPU side)
  pub render: Duration,
  /// Time spent running the render passes and presenting, without
  /// `wait`
  pub present: Duration,
  /// Time spent waiting for the window's next image to draw into, which
  /// vsync paces
  pub wait: Duration,
  /// Total work time (excludes `wait` and the sleep between frames)
  pub total: Duration,
}

/// What the engine's recent frames cost on the CPU, smoothed over about 50
/// frames: a world resource the engine writes at the end of every frame,
/// windowed or headless, for a game's own readout of the numbers the F1
/// frame timing overlay shows. Read it, don't write it: the engine puts it
/// back each frame.
///
/// [`update_ms`](Self::update_ms), [`render_ms`](Self::render_ms) and
/// [`present_ms`](Self::present_ms) do not add up to
/// [`total_ms`](Self::total_ms): the frame's phases outside the fixed steps
/// (`First`, `PreUpdate` with the asset pump and tile map spawning,
/// `Update`, `PostUpdate` with transform propagation and map shadow sync,
/// and `Last`) count in the total alone: their cost is the total less the
/// other three.
///
/// ```
/// use ryo2d::prelude::*;
///
/// fn render_cost(ctx: &SceneContext<'_>) -> String {
///   match ctx.resource::<FrameCost>() {
///     Some(cost) => format!("render {:.2} ms, update {:.2} ms", cost.render_ms(), cost.update_ms()),
///     None => String::new(),
///   }
/// }
/// ```
#[derive(crate::core::ecs::Resource, Debug, Clone, Copy, PartialEq)]
pub struct FrameCost {
  update_ms: f64,
  render_ms: f64,
  present_ms: f64,
  total_ms: f64,
  fps: f64,
  frames: u64,
}

impl FrameCost {
  /// The engine's smoothed timings as they stand
  pub(crate) fn of(timings: &SmoothedFrameTimings) -> FrameCost {
    FrameCost {
      update_ms: timings.update_ms,
      render_ms: timings.render_ms,
      present_ms: timings.present_ms,
      total_ms: timings.total_ms,
      fps: timings.fps,
      frames: timings.frames,
    }
  }

  /// The UI pass and the fixed steps: the scenes' updates, their systems
  /// and physics, in milliseconds. The frame's other phases (the asset
  /// pump, tile map spawning, transform propagation) are not in it; they
  /// show only in [`total_ms`](Self::total_ms)
  pub fn update_ms(&self) -> f64 {
    self.update_ms
  }

  /// Building the frame on the CPU: gathering what it draws (sprites, map
  /// chunks, text, lights), uploading and encoding it, in milliseconds
  pub fn render_ms(&self) -> f64 {
    self.render_ms
  }

  /// Running the render passes and presenting, without the wait for the
  /// window's next image (which vsync paces), in milliseconds
  pub fn present_ms(&self) -> f64 {
    self.present_ms
  }

  /// The frame's work from its start to presenting, every phase included
  /// but not the vsync wait or the sleep between frames, in milliseconds
  pub fn total_ms(&self) -> f64 {
    self.total_ms
  }

  /// Frames a second, from the wall-clock time between frames (which does
  /// count the waits); 0 until the second frame
  pub fn fps(&self) -> f64 {
    self.fps
  }

  /// How many frames have been measured
  pub fn frames(&self) -> u64 {
    self.frames
  }
}

/// Smoothed frame timing values for stable overlay display.
///
/// Uses exponential moving average to prevent jittery numbers.
/// The smoothing factor (alpha) controls responsiveness vs stability.
#[derive(Debug, Clone)]
pub struct SmoothedFrameTimings {
  /// Smoothed update time in milliseconds
  pub update_ms: f64,
  /// Smoothed render time in milliseconds
  pub render_ms: f64,
  /// Smoothed present time in milliseconds
  pub present_ms: f64,
  /// Smoothed total work time in milliseconds (excludes sleep/vsync wait)
  pub total_ms: f64,
  /// FPS from the smoothed wall-clock time between frames (includes
  /// sleep/vsync)
  pub fps: f64,
  /// Smoothed wall-clock time between frames, in seconds (0 before the
  /// second frame)
  frame_secs: f64,
  /// Smoothing factor (0.0 = no change, 1.0 = instant update)
  alpha: f64,
  /// Tracks wall-clock time between update() calls for real FPS measurement
  last_update: Option<Instant>,
  /// How many frames have been measured
  frames: u64,
}

impl Default for SmoothedFrameTimings {
  fn default() -> Self {
    Self {
      update_ms: 0.0,
      render_ms: 0.0,
      present_ms: 0.0,
      total_ms: 0.0,
      fps: 0.0,
      frame_secs: 0.0,
      alpha: 0.02, // Smooth over ~50 frames for stable display
      last_update: None,
      frames: 0,
    }
  }
}

impl SmoothedFrameTimings {
  /// Update smoothed values from raw frame timings.
  ///
  /// Work-time fields (update, render, present, total) come from `FrameTimings`.
  /// FPS is measured from wall-clock time between consecutive `update()` calls,
  /// so it includes sleep/vsync wait and reflects actual rendered frames per second.
  pub fn update(&mut self, raw: &FrameTimings) {
    self.update_at(raw, Instant::now());
  }

  /// [`update`](Self::update) with the frame ending at `now`
  fn update_at(&mut self, raw: &FrameTimings, now: Instant) {
    let update_ms = raw.update.as_secs_f64() * 1000.0;
    let render_ms = raw.render.as_secs_f64() * 1000.0;
    let present_ms = raw.present.as_secs_f64() * 1000.0;
    let total_ms = raw.total.as_secs_f64() * 1000.0;

    // Exponential moving average: smoothed = alpha * new + (1 - alpha) * old.
    // Until there are as many frames as the average spans it is their plain
    // mean, so the first frames read as they are rather than rising from 0
    self.frames += 1;
    let alpha = self.alpha.max(1.0 / self.frames as f64);
    self.update_ms = alpha * update_ms + (1.0 - alpha) * self.update_ms;
    self.render_ms = alpha * render_ms + (1.0 - alpha) * self.render_ms;
    self.present_ms = alpha * present_ms + (1.0 - alpha) * self.present_ms;
    self.total_ms = alpha * total_ms + (1.0 - alpha) * self.total_ms;

    // FPS from the real wall-clock time between frames (includes
    // sleep/vsync). The time is averaged, not the rate: vsync often hands
    // out frames in bursts, and averaging 1/dt over a short gap and a long
    // one counts far more frames than were drawn.
    if let Some(last) = self.last_update {
      let real_dt = now.duration_since(last).as_secs_f64();
      self.frame_secs = if self.frame_secs > 0.0 {
        self.alpha * real_dt + (1.0 - self.alpha) * self.frame_secs
      } else {
        real_dt
      };
      if self.frame_secs > 0.0 {
        self.fps = 1.0 / self.frame_secs;
      }
    }
    self.last_update = Some(now);
  }
}

/// Helper to time a code block and return the duration.
#[inline]
pub fn time_block<F, R>(f: F) -> (R, Duration)
where
  F: FnOnce() -> R,
{
  let start = Instant::now();
  let result = f();
  (result, start.elapsed())
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn the_first_frames_are_averaged_rather_than_blended_up_from_zero() {
    let mut timings = SmoothedFrameTimings::default();
    let raw = FrameTimings {
      update: Duration::from_millis(2),
      render: Duration::from_millis(4),
      ..FrameTimings::default()
    };
    let mut now = Instant::now();
    for _ in 0..3 {
      now += Duration::from_millis(16);
      timings.update_at(&raw, now);
    }
    assert!(
      (timings.update_ms - 2.0).abs() < 1e-9,
      "{}",
      timings.update_ms
    );
    assert!(
      (timings.render_ms - 4.0).abs() < 1e-9,
      "{}",
      timings.render_ms
    );
  }

  #[test]
  fn fps_averages_the_frame_time_not_the_frame_rate() {
    // Vsync often hands frames out in pairs: one 1 ms after the last, the
    // next 15.67 ms later. That is two frames every 16.67 ms: 120 a second.
    let mut timings = SmoothedFrameTimings::default();
    let raw = FrameTimings::default();
    let mut now = Instant::now();
    for frame in 0..2000 {
      let gap = if frame % 2 == 0 { 1_000 } else { 15_667 };
      now += Duration::from_micros(gap);
      timings.update_at(&raw, now);
    }
    // The average ripples a little (about 2%) as the gaps alternate
    assert!((timings.fps - 120.0).abs() < 3.0, "fps {}", timings.fps);
  }
}
