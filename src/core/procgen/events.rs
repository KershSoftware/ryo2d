//! Procedural generation messages
//!
//! Messages for monitoring generation and for game logic, written with
//! `ctx.write_message` and read with `ctx.messages`.

use std::time::Duration;

use crate::core::ecs::Message;

/// Emitted when a generation algorithm starts
///
/// # Example
///
/// ```ignore
/// use ryo2d::prelude::*;
///
/// fn update(&mut self, ctx: &mut GameContext) {
///     for event in ctx.messages::<GenerationStarted>() {
///         println!("Starting {} with seed {}", event.algorithm, event.seed);
///     }
/// }
/// ```
#[derive(Message, Debug, Clone)]
pub struct GenerationStarted {
  /// Name of the algorithm being used
  pub algorithm: String,
  /// Seed for reproducibility
  pub seed: u64,
  /// Grid width
  pub width: u32,
  /// Grid height
  pub height: u32,
}

impl GenerationStarted {
  pub fn new(algorithm: impl Into<String>, seed: u64, width: u32, height: u32) -> Self {
    GenerationStarted {
      algorithm: algorithm.into(),
      seed,
      width,
      height,
    }
  }
}

/// Emitted when a generation algorithm completes successfully
///
/// # Example
///
/// ```ignore
/// use ryo2d::prelude::*;
///
/// fn update(&mut self, ctx: &mut GameContext) {
///     for event in ctx.messages::<GenerationCompleted>() {
///         println!(
///             "{} generated {} regions in {}ms",
///             event.algorithm, event.region_count, event.duration_ms
///         );
///     }
/// }
/// ```
#[derive(Message, Debug, Clone)]
pub struct GenerationCompleted {
  /// Name of the algorithm used
  pub algorithm: String,
  /// Seed used for generation
  pub seed: u64,
  /// Number of regions created
  pub region_count: usize,
  /// Generation time in milliseconds
  pub duration_ms: u64,
}

impl GenerationCompleted {
  pub fn new(algorithm: impl Into<String>, seed: u64, region_count: usize) -> Self {
    GenerationCompleted {
      algorithm: algorithm.into(),
      seed,
      region_count,
      duration_ms: 0,
    }
  }

  pub fn with_duration(mut self, duration: Duration) -> Self {
    self.duration_ms = duration.as_millis() as u64;
    self
  }

  pub fn with_duration_ms(mut self, duration_ms: u64) -> Self {
    self.duration_ms = duration_ms;
    self
  }
}

/// Reason for generation failure
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GenerationFailureReason {
  /// Grid dimensions too small
  GridTooSmall,
  /// Configuration invalid
  InvalidConfig,
  /// Algorithm couldn't produce valid output
  NoValidOutput,
  /// WFC contradiction (no valid tile placement)
  WfcContradiction,
  /// Timeout during generation
  Timeout,
  /// Custom reason
  Custom(String),
}

/// Emitted when a generation algorithm fails
///
/// # Example
///
/// ```ignore
/// use ryo2d::prelude::*;
///
/// fn update(&mut self, ctx: &mut GameContext) {
///     for event in ctx.messages::<GenerationFailed>() {
///         eprintln!("Generation failed: {:?}", event.reason);
///     }
/// }
/// ```
#[derive(Message, Debug, Clone)]
pub struct GenerationFailed {
  /// Name of the algorithm that failed
  pub algorithm: String,
  /// Reason for failure
  pub reason: GenerationFailureReason,
}

impl GenerationFailed {
  pub fn new(algorithm: impl Into<String>, reason: GenerationFailureReason) -> Self {
    GenerationFailed {
      algorithm: algorithm.into(),
      reason,
    }
  }
}

/// Emitted when a new region (room, cave, etc.) is created
///
/// # Example
///
/// ```ignore
/// use ryo2d::prelude::*;
///
/// fn update(&mut self, ctx: &mut GameContext) {
///     for event in ctx.messages::<RegionCreated>() {
///         // Spawn entities in the new room
///         spawn_enemies_in_region(ctx, &event.bounds);
///     }
/// }
/// ```
#[derive(Message, Debug, Clone, Copy)]
pub struct RegionCreated {
  /// Region ID
  pub region_id: u32,
  /// Bounding box x
  pub x: i32,
  /// Bounding box y
  pub y: i32,
  /// Bounding box width
  pub width: u32,
  /// Bounding box height
  pub height: u32,
  /// Center x
  pub center_x: i32,
  /// Center y
  pub center_y: i32,
}

impl RegionCreated {
  pub fn new(
    region_id: u32,
    x: i32,
    y: i32,
    width: u32,
    height: u32,
    center_x: i32,
    center_y: i32,
  ) -> Self {
    RegionCreated {
      region_id,
      x,
      y,
      width,
      height,
      center_x,
      center_y,
    }
  }

  pub fn from_region(region: &super::grid::Region) -> Self {
    RegionCreated {
      region_id: region.id,
      x: region.bounds.x,
      y: region.bounds.y,
      width: region.bounds.width,
      height: region.bounds.height,
      center_x: region.center.0,
      center_y: region.center.1,
    }
  }
}

/// Emitted when two regions are connected
///
/// # Example
///
/// ```ignore
/// use ryo2d::prelude::*;
///
/// fn update(&mut self, ctx: &mut GameContext) {
///     for event in ctx.messages::<RegionsConnected>() {
///         // Maybe spawn a door between regions
///         spawn_door(ctx, event.region_a, event.region_b);
///     }
/// }
/// ```
#[derive(Message, Debug, Clone, Copy)]
pub struct RegionsConnected {
  /// ID of the first region
  pub region_a: u32,
  /// ID of the second region
  pub region_b: u32,
  /// Length of the connecting path
  pub path_length: f32,
}

impl RegionsConnected {
  pub fn new(region_a: u32, region_b: u32, path_length: f32) -> Self {
    RegionsConnected {
      region_a,
      region_b,
      path_length,
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn generation_started_creation() {
    let event = GenerationStarted::new("BSP", 12345, 64, 64);
    assert_eq!(event.algorithm, "BSP");
    assert_eq!(event.seed, 12345);
    assert_eq!(event.width, 64);
    assert_eq!(event.height, 64);
  }

  #[test]
  fn generation_completed_with_duration() {
    let event =
      GenerationCompleted::new("Cellular", 12345, 5).with_duration(Duration::from_millis(150));
    assert_eq!(event.duration_ms, 150);
  }

  #[test]
  fn generation_failed_creation() {
    let event = GenerationFailed::new("WFC", GenerationFailureReason::WfcContradiction);
    assert_eq!(event.algorithm, "WFC");
    assert_eq!(event.reason, GenerationFailureReason::WfcContradiction);
  }

  #[test]
  fn region_created_creation() {
    let event = RegionCreated::new(0, 10, 20, 30, 40, 25, 40);
    assert_eq!(event.region_id, 0);
    assert_eq!(event.x, 10);
    assert_eq!(event.y, 20);
    assert_eq!(event.center_x, 25);
  }

  #[test]
  fn regions_connected_creation() {
    let event = RegionsConnected::new(0, 1, 15.5);
    assert_eq!(event.region_a, 0);
    assert_eq!(event.region_b, 1);
    assert!((event.path_length - 15.5).abs() < 0.01);
  }
}
