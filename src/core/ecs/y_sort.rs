//! Y-sorting component for Zelda/RPG-style depth ordering
//!
//! The `YSort` component enables automatic depth sorting based on entity Y position,
//! creating the illusion of entities walking in front of or behind each other.
//!
//! # Example
//!
//! ```rust,ignore
//! use ryo2d::prelude::*;
//!
//! // Basic y-sorting (sorts by foot position)
//! world.spawn((
//!     Transform::new(100.0, 50.0),
//!     Renderable::sprite(player_handle),
//!     YSort::new(),
//! ));
//!
//! // With depth scaling for perspective illusion
//! world.spawn((
//!     Transform::new(100.0, 50.0),
//!     Renderable::sprite(npc_handle),
//!     YSort::new().with_depth_scale(0.7, 1.3, 0.0, 180.0),
//! ));
//!
//! // Flying entity with fixed sort offset
//! world.spawn((
//!     Transform::new(100.0, 30.0),
//!     Renderable::sprite(bat_handle),
//!     YSort::new().with_sort_offset(40.0),
//! ));
//! ```
//!
//! # How It Works
//!
//! Y-sorting combines the entity's z_order (from Renderable) with its Y position
//! to create a final sort key:
//!
//! ```text
//! sort_key = (z_order * 10000) + sort_y
//! ```
//!
//! This maintains layer separation (z_order) while enabling Y-based sorting within
//! each layer. Entities at higher Y positions (lower on screen) appear in front.
//!
//! # Foot Offset
//!
//! The `foot_offset` field determines which part of the sprite is used for sorting:
//! - `1.0` (default) - Sort by bottom of sprite (feet)
//! - `0.5` - Sort by center of sprite
//! - `0.0` - Sort by top of sprite
//!
//! # Depth Scaling
//!
//! Optional perspective illusion where entities scale based on Y position:
//! - Entities at `y_min` (far/top) use `scale_min`
//! - Entities at `y_max` (near/bottom) use `scale_max`
//!
//! This creates a pseudo-3D effect common in top-down RPGs.

/// Y-sorting component for automatic depth ordering based on Y position
///
/// Attach this to entities that should be sorted by their vertical position
/// within a render layer. Higher Y values (lower on screen) render in front.
///
/// `z_order` picks the layer; `YSort` orders entities by y within their layer;
/// sprites, particles and trails at the same z draw after the layer's entities.
#[derive(Debug, Clone)]
pub struct YSort {
  /// Where on the sprite to measure Y for sorting (0.0=top, 1.0=bottom)
  ///
  /// Default is 1.0 (foot position) which is standard for character sprites
  /// where the "feet" touch the ground plane.
  pub foot_offset: f32,

  /// Fixed pixel offset added to sort Y calculation
  ///
  /// Useful for flying entities or objects that should sort at a different
  /// Y than their actual position (e.g., a shadow's entity position is on
  /// the ground but the flying creature above it).
  pub sort_offset: f32,

  /// Whether depth scaling is enabled for this entity
  pub depth_scale_enabled: bool,

  /// Minimum scale factor (applied at y_min)
  pub scale_min: f32,

  /// Maximum scale factor (applied at y_max)
  pub scale_max: f32,

  /// Y position that corresponds to scale_min (typically top of play area)
  pub y_min: f32,

  /// Y position that corresponds to scale_max (typically bottom of play area)
  pub y_max: f32,
}

impl Default for YSort {
  fn default() -> Self {
    Self::new()
  }
}

impl YSort {
  /// Create a new YSort component with default settings
  ///
  /// Defaults:
  /// - `foot_offset`: 1.0 (sort by bottom of sprite)
  /// - `sort_offset`: 0.0 (no fixed offset)
  /// - Depth scaling: disabled
  pub fn new() -> Self {
    Self {
      foot_offset: 1.0,
      sort_offset: 0.0,
      depth_scale_enabled: false,
      scale_min: 1.0,
      scale_max: 1.0,
      y_min: 0.0,
      y_max: 100.0,
    }
  }

  /// Set the foot offset (0.0=top, 0.5=center, 1.0=bottom)
  ///
  /// This determines which vertical position of the sprite is used for
  /// Y-sorting calculations.
  pub fn with_foot_offset(mut self, offset: f32) -> Self {
    self.foot_offset = offset.clamp(0.0, 1.0);
    self
  }

  /// Set a fixed pixel offset for sorting
  ///
  /// This offset is added to the calculated sort Y position. Use positive
  /// values to make the entity sort as if it were lower on screen (in front),
  /// or negative values to sort higher (behind).
  pub fn with_sort_offset(mut self, offset: f32) -> Self {
    self.sort_offset = offset;
    self
  }

  /// Enable depth scaling with the specified parameters
  ///
  /// # Arguments
  ///
  /// * `scale_min` - Scale factor at y_min (far/top of screen)
  /// * `scale_max` - Scale factor at y_max (near/bottom of screen)
  /// * `y_min` - Y position for minimum scale
  /// * `y_max` - Y position for maximum scale
  ///
  /// # Example
  ///
  /// ```rust,ignore
  /// // Entities at y=0 are 70% size, at y=180 are 130% size
  /// YSort::new().with_depth_scale(0.7, 1.3, 0.0, 180.0)
  /// ```
  pub fn with_depth_scale(
    mut self,
    scale_min: f32,
    scale_max: f32,
    y_min: f32,
    y_max: f32,
  ) -> Self {
    self.depth_scale_enabled = true;
    self.scale_min = scale_min;
    self.scale_max = scale_max;
    self.y_min = y_min;
    self.y_max = y_max;
    self
  }

  /// Disable depth scaling
  pub fn without_depth_scale(mut self) -> Self {
    self.depth_scale_enabled = false;
    self
  }

  /// Calculate the Y position used for sorting
  ///
  /// # Arguments
  ///
  /// * `transform_y` - The entity's transform Y position
  /// * `sprite_height` - Height of the sprite in pixels
  ///
  /// # Returns
  ///
  /// The Y value to use for sorting comparisons
  pub fn calculate_sort_y(&self, transform_y: f32, sprite_height: f32) -> f32 {
    transform_y + (sprite_height * self.foot_offset) + self.sort_offset
  }

  /// Calculate the depth scale factors for an entity at the given Y position
  ///
  /// # Arguments
  ///
  /// * `y` - The entity's Y position (typically transform.y)
  ///
  /// # Returns
  ///
  /// A tuple of (scale_x, scale_y) multipliers to apply to the entity's transform scale.
  /// Returns (1.0, 1.0) if depth scaling is disabled.
  pub fn calculate_depth_scale(&self, y: f32) -> (f32, f32) {
    if !self.depth_scale_enabled {
      return (1.0, 1.0);
    }

    // Avoid division by zero
    let range = self.y_max - self.y_min;
    if range.abs() < 0.001 {
      return (self.scale_min, self.scale_min);
    }

    // Calculate t (0.0 at y_min, 1.0 at y_max), clamped
    let t = ((y - self.y_min) / range).clamp(0.0, 1.0);

    // Linear interpolation between scale_min and scale_max
    let scale = self.scale_min + t * (self.scale_max - self.scale_min);

    (scale, scale)
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn default_values() {
    let ysort = YSort::new();
    assert_eq!(ysort.foot_offset, 1.0);
    assert_eq!(ysort.sort_offset, 0.0);
    assert!(!ysort.depth_scale_enabled);
  }

  #[test]
  fn with_foot_offset() {
    let ysort = YSort::new().with_foot_offset(0.5);
    assert_eq!(ysort.foot_offset, 0.5);

    // Test clamping
    let ysort_clamped = YSort::new().with_foot_offset(2.0);
    assert_eq!(ysort_clamped.foot_offset, 1.0);

    let ysort_clamped_neg = YSort::new().with_foot_offset(-1.0);
    assert_eq!(ysort_clamped_neg.foot_offset, 0.0);
  }

  #[test]
  fn with_sort_offset() {
    let ysort = YSort::new().with_sort_offset(50.0);
    assert_eq!(ysort.sort_offset, 50.0);

    let ysort_neg = YSort::new().with_sort_offset(-30.0);
    assert_eq!(ysort_neg.sort_offset, -30.0);
  }

  #[test]
  fn with_depth_scale() {
    let ysort = YSort::new().with_depth_scale(0.7, 1.3, 0.0, 180.0);
    assert!(ysort.depth_scale_enabled);
    assert_eq!(ysort.scale_min, 0.7);
    assert_eq!(ysort.scale_max, 1.3);
    assert_eq!(ysort.y_min, 0.0);
    assert_eq!(ysort.y_max, 180.0);
  }

  #[test]
  fn without_depth_scale() {
    let ysort = YSort::new()
      .with_depth_scale(0.7, 1.3, 0.0, 180.0)
      .without_depth_scale();
    assert!(!ysort.depth_scale_enabled);
  }

  #[test]
  fn calculate_sort_y_default() {
    let ysort = YSort::new();
    // With foot_offset=1.0 and sort_offset=0.0:
    // sort_y = transform_y + (sprite_height * 1.0) + 0.0
    let sort_y = ysort.calculate_sort_y(100.0, 32.0);
    assert_eq!(sort_y, 132.0);
  }

  #[test]
  fn calculate_sort_y_center() {
    let ysort = YSort::new().with_foot_offset(0.5);
    // sort_y = 100.0 + (32.0 * 0.5) + 0.0 = 116.0
    let sort_y = ysort.calculate_sort_y(100.0, 32.0);
    assert_eq!(sort_y, 116.0);
  }

  #[test]
  fn calculate_sort_y_with_offset() {
    let ysort = YSort::new().with_sort_offset(20.0);
    // sort_y = 100.0 + (32.0 * 1.0) + 20.0 = 152.0
    let sort_y = ysort.calculate_sort_y(100.0, 32.0);
    assert_eq!(sort_y, 152.0);
  }

  #[test]
  fn calculate_depth_scale_disabled() {
    let ysort = YSort::new();
    let (sx, sy) = ysort.calculate_depth_scale(50.0);
    assert_eq!(sx, 1.0);
    assert_eq!(sy, 1.0);
  }

  #[test]
  fn calculate_depth_scale_at_min() {
    let ysort = YSort::new().with_depth_scale(0.5, 1.5, 0.0, 100.0);
    let (sx, sy) = ysort.calculate_depth_scale(0.0);
    assert_eq!(sx, 0.5);
    assert_eq!(sy, 0.5);
  }

  #[test]
  fn calculate_depth_scale_at_max() {
    let ysort = YSort::new().with_depth_scale(0.5, 1.5, 0.0, 100.0);
    let (sx, sy) = ysort.calculate_depth_scale(100.0);
    assert_eq!(sx, 1.5);
    assert_eq!(sy, 1.5);
  }

  #[test]
  fn calculate_depth_scale_at_midpoint() {
    let ysort = YSort::new().with_depth_scale(0.5, 1.5, 0.0, 100.0);
    let (sx, sy) = ysort.calculate_depth_scale(50.0);
    // t = 0.5, scale = 0.5 + 0.5 * (1.5 - 0.5) = 0.5 + 0.5 = 1.0
    assert_eq!(sx, 1.0);
    assert_eq!(sy, 1.0);
  }

  #[test]
  fn calculate_depth_scale_clamped() {
    let ysort = YSort::new().with_depth_scale(0.5, 1.5, 0.0, 100.0);

    // Below y_min - should clamp to scale_min
    let (sx, sy) = ysort.calculate_depth_scale(-50.0);
    assert_eq!(sx, 0.5);
    assert_eq!(sy, 0.5);

    // Above y_max - should clamp to scale_max
    let (sx, sy) = ysort.calculate_depth_scale(150.0);
    assert_eq!(sx, 1.5);
    assert_eq!(sy, 1.5);
  }

  #[test]
  fn calculate_depth_scale_zero_range() {
    let ysort = YSort::new().with_depth_scale(0.8, 1.2, 50.0, 50.0);
    let (sx, sy) = ysort.calculate_depth_scale(50.0);
    // With zero range, should return scale_min
    assert_eq!(sx, 0.8);
    assert_eq!(sy, 0.8);
  }

  #[test]
  fn builder_chaining() {
    let ysort = YSort::new()
      .with_foot_offset(0.5)
      .with_sort_offset(10.0)
      .with_depth_scale(0.7, 1.3, 0.0, 200.0);

    assert_eq!(ysort.foot_offset, 0.5);
    assert_eq!(ysort.sort_offset, 10.0);
    assert!(ysort.depth_scale_enabled);
    assert_eq!(ysort.scale_min, 0.7);
    assert_eq!(ysort.scale_max, 1.3);
    assert_eq!(ysort.y_min, 0.0);
    assert_eq!(ysort.y_max, 200.0);
  }

  // ============================================================================
  // REGRESSION TESTS - Critical invariants
  // ============================================================================

  /// REGRESSION: Default foot_offset must be 1.0 (bottom of sprite)
  ///
  /// This is the standard for RPG-style y-sorting where entities sort by their feet.
  #[test]
  fn regression_default_foot_offset_is_bottom() {
    let ysort = YSort::new();
    assert_eq!(
      ysort.foot_offset, 1.0,
      "Default foot_offset must be 1.0 (bottom of sprite)"
    );
  }

  /// REGRESSION: Depth scaling must be disabled by default
  ///
  /// Users must explicitly opt-in to depth scaling to avoid unexpected behavior.
  #[test]
  fn regression_depth_scale_disabled_by_default() {
    let ysort = YSort::new();
    assert!(
      !ysort.depth_scale_enabled,
      "Depth scaling must be disabled by default"
    );
  }

  /// REGRESSION: Disabled depth scale must return (1.0, 1.0)
  ///
  /// When depth scaling is disabled, calculate_depth_scale must return identity scale.
  #[test]
  fn regression_disabled_scale_returns_identity() {
    let ysort = YSort::new();
    let (sx, sy) = ysort.calculate_depth_scale(999.0);
    assert_eq!(sx, 1.0, "Disabled depth scale must return 1.0 for x");
    assert_eq!(sy, 1.0, "Disabled depth scale must return 1.0 for y");
  }

  // ============================================================================
  // SORT KEY CALCULATION TESTS
  // ============================================================================

  /// Test sort key formula: higher Y = higher sort key = renders in front
  #[test]
  fn sort_y_increases_with_transform_y() {
    let ysort = YSort::new();

    let sort_y_low = ysort.calculate_sort_y(50.0, 32.0);
    let sort_y_high = ysort.calculate_sort_y(100.0, 32.0);

    assert!(
      sort_y_high > sort_y_low,
      "Higher transform Y should produce higher sort Y"
    );
  }

  /// Test that foot_offset=0 sorts by top of sprite
  #[test]
  fn foot_offset_zero_sorts_by_top() {
    let ysort = YSort::new().with_foot_offset(0.0);
    let sort_y = ysort.calculate_sort_y(100.0, 32.0);
    // sort_y = 100.0 + (32.0 * 0.0) + 0.0 = 100.0
    assert_eq!(
      sort_y, 100.0,
      "foot_offset=0 should sort by transform Y (top)"
    );
  }

  /// Test that negative sort_offset moves entity "behind" others
  #[test]
  fn negative_sort_offset_sorts_behind() {
    let ysort_normal = YSort::new();
    let ysort_behind = YSort::new().with_sort_offset(-50.0);

    let sort_normal = ysort_normal.calculate_sort_y(100.0, 32.0);
    let sort_behind = ysort_behind.calculate_sort_y(100.0, 32.0);

    assert!(
      sort_behind < sort_normal,
      "Negative sort_offset should produce lower sort Y (behind)"
    );
  }

  /// Test that positive sort_offset moves entity "in front" of others
  #[test]
  fn positive_sort_offset_sorts_in_front() {
    let ysort_normal = YSort::new();
    let ysort_front = YSort::new().with_sort_offset(50.0);

    let sort_normal = ysort_normal.calculate_sort_y(100.0, 32.0);
    let sort_front = ysort_front.calculate_sort_y(100.0, 32.0);

    assert!(
      sort_front > sort_normal,
      "Positive sort_offset should produce higher sort Y (in front)"
    );
  }

  /// Test flying entity use case: entity at y=30 with sort_offset=40 sorts like y=70
  #[test]
  fn flying_entity_sorts_at_shadow_position() {
    let ysort_flying = YSort::new().with_sort_offset(40.0);
    let ysort_ground = YSort::new();

    // Flying entity at y=30 with offset=40
    let sort_flying = ysort_flying.calculate_sort_y(30.0, 16.0);
    // Ground entity at y=70
    let sort_ground = ysort_ground.calculate_sort_y(70.0, 16.0);

    // Flying: 30 + (16 * 1.0) + 40 = 86
    // Ground: 70 + (16 * 1.0) + 0 = 86
    assert_eq!(sort_flying, 86.0);
    assert_eq!(sort_ground, 86.0);
  }

  // ============================================================================
  // DEPTH SCALE INTERPOLATION TESTS
  // ============================================================================

  /// Test depth scale at 25% position
  #[test]
  fn depth_scale_at_quarter() {
    let ysort = YSort::new().with_depth_scale(0.5, 1.5, 0.0, 100.0);
    let (sx, _) = ysort.calculate_depth_scale(25.0);
    // t = 0.25, scale = 0.5 + 0.25 * (1.5 - 0.5) = 0.5 + 0.25 = 0.75
    assert!((sx - 0.75).abs() < 0.001, "Scale at 25% should be 0.75");
  }

  /// Test depth scale at 75% position
  #[test]
  fn depth_scale_at_three_quarters() {
    let ysort = YSort::new().with_depth_scale(0.5, 1.5, 0.0, 100.0);
    let (sx, _) = ysort.calculate_depth_scale(75.0);
    // t = 0.75, scale = 0.5 + 0.75 * (1.5 - 0.5) = 0.5 + 0.75 = 1.25
    assert!((sx - 1.25).abs() < 0.001, "Scale at 75% should be 1.25");
  }

  /// Test depth scale with inverted range (scale_max < scale_min)
  #[test]
  fn depth_scale_inverted_range() {
    // Larger at top, smaller at bottom (unusual but valid)
    let ysort = YSort::new().with_depth_scale(1.5, 0.5, 0.0, 100.0);

    let (sx_min, _) = ysort.calculate_depth_scale(0.0);
    let (sx_max, _) = ysort.calculate_depth_scale(100.0);

    assert_eq!(sx_min, 1.5, "At y_min should be scale_min (1.5)");
    assert_eq!(sx_max, 0.5, "At y_max should be scale_max (0.5)");
  }

  /// Test depth scale with negative y range
  #[test]
  fn depth_scale_negative_y_range() {
    let ysort = YSort::new().with_depth_scale(0.8, 1.2, -100.0, 100.0);

    let (sx_min, _) = ysort.calculate_depth_scale(-100.0);
    let (sx_mid, _) = ysort.calculate_depth_scale(0.0);
    let (sx_max, _) = ysort.calculate_depth_scale(100.0);

    assert_eq!(sx_min, 0.8);
    assert_eq!(sx_mid, 1.0);
    assert_eq!(sx_max, 1.2);
  }

  /// Test that scale_x equals scale_y (uniform scaling)
  #[test]
  fn depth_scale_is_uniform() {
    let ysort = YSort::new().with_depth_scale(0.5, 2.0, 0.0, 100.0);

    for y in [0.0, 25.0, 50.0, 75.0, 100.0] {
      let (sx, sy) = ysort.calculate_depth_scale(y);
      assert_eq!(sx, sy, "Scale must be uniform (sx == sy) at y={}", y);
    }
  }

  // ============================================================================
  // CLONE AND DEFAULT TESTS
  // ============================================================================

  /// Test that YSort implements Clone correctly
  #[test]
  fn ysort_clone() {
    let original = YSort::new()
      .with_foot_offset(0.7)
      .with_sort_offset(15.0)
      .with_depth_scale(0.6, 1.4, 10.0, 200.0);

    let cloned = original.clone();

    assert_eq!(cloned.foot_offset, original.foot_offset);
    assert_eq!(cloned.sort_offset, original.sort_offset);
    assert_eq!(cloned.depth_scale_enabled, original.depth_scale_enabled);
    assert_eq!(cloned.scale_min, original.scale_min);
    assert_eq!(cloned.scale_max, original.scale_max);
    assert_eq!(cloned.y_min, original.y_min);
    assert_eq!(cloned.y_max, original.y_max);
  }

  /// Test that YSort implements Default correctly
  #[test]
  fn ysort_default() {
    let default: YSort = Default::default();
    let new = YSort::new();

    assert_eq!(default.foot_offset, new.foot_offset);
    assert_eq!(default.sort_offset, new.sort_offset);
    assert_eq!(default.depth_scale_enabled, new.depth_scale_enabled);
  }

  // ============================================================================
  // EDGE CASE TESTS
  // ============================================================================

  /// Test with zero sprite height
  #[test]
  fn sort_y_zero_sprite_height() {
    let ysort = YSort::new();
    let sort_y = ysort.calculate_sort_y(100.0, 0.0);
    // sort_y = 100.0 + (0.0 * 1.0) + 0.0 = 100.0
    assert_eq!(sort_y, 100.0);
  }

  /// Test with very large values
  #[test]
  fn sort_y_large_values() {
    let ysort = YSort::new();
    let sort_y = ysort.calculate_sort_y(10000.0, 1000.0);
    assert_eq!(sort_y, 11000.0);
  }

  /// Test depth scale with very small range (near zero but not zero)
  #[test]
  fn depth_scale_tiny_range() {
    // Range of 0.001 is exactly at the threshold, so it should work
    let ysort = YSort::new().with_depth_scale(0.9, 1.1, 100.0, 100.01);
    // Range is 0.01, which is > 0.001 threshold
    let (sx, _) = ysort.calculate_depth_scale(100.005);
    // t = 0.5, scale = 0.9 + 0.5 * 0.2 = 1.0
    assert!((sx - 1.0).abs() < 0.01);
  }
}
