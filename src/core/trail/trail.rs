//! Trail effect component
//!
//! Trails render a fading path behind moving entities, perfect for
//! projectiles, particles, sword swipes, and motion effects.
//!
//! # Example
//!
//! ```rust,ignore
//! use ryo2d::prelude::*;
//!
//! // Spawn a bullet with a trail
//! let bullet = ctx.world.spawn((
//!     Transform::from_xy(100.0, 100.0),
//!     Renderable::sprite(bullet_sprite),
//!     Trail2D::new(Color::YELLOW, 5.0)
//!         .with_length(20)
//!         .with_fade(true),
//! )).id();
//! ```

use crate::core::asset::SpriteHandle;
use crate::core::ecs::Component;
use crate::core::render::batch::{BlendMode, Filter};
use crate::core::ui::Color;
use std::collections::VecDeque;

/// How a texture is mapped along a trail
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TextureMode {
  /// Stretch the texture to fit the entire trail length (UV.x 0-1 maps to full texture)
  #[default]
  Stretch,
  /// Tile the texture along the trail (texture repeats based on trail length)
  Tile,
}

/// How a trail or line is shaded across its width.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TrailStyle {
  /// Solid color with sharp edges
  #[default]
  Solid,
  /// Soft edges that fade out
  Soft,
  /// Glow effect with bright core
  Glow,
}

/// How line endpoints are drawn
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LineCap {
  /// Flat cut at endpoint (current behavior, zero overhead)
  #[default]
  Butt,
  /// Semicircle at endpoints (~8 extra triangles per cap)
  Round,
  /// Extends by half-width beyond endpoint (2 extra vertices per cap)
  Square,
}

/// How line segments connect at corners
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LineJoin {
  /// Sharp corner; one longer than four times the line's width is bevelled
  #[default]
  Miter,
  /// Flat diagonal cut at corner (1 extra triangle per corner)
  Bevel,
  /// Smooth rounded corner (a few triangles per corner)
  Round,
}

/// GPU-driven trail/line effects applied in the vertex/fragment shader.
///
/// Effects are composable with any `TrailStyle` and work on both
/// Trail2D and Line2D components.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TrailEffect {
  /// Jagged electrical arc displacement
  Lightning {
    /// Max displacement in pixels
    jitter: f32,
    /// Re-randomization rate (Hz)
    speed: f32,
    /// Noise octaves (1-4, higher = more detailed)
    detail: u32,
  },
  /// Perlin noise displacement for organic movement (fire, smoke, wind)
  Noise {
    /// Spatial frequency of the noise
    frequency: f32,
    /// Max displacement in pixels
    amplitude: f32,
    /// Animation scroll speed
    speed: f32,
  },
  /// Scrolling UV coordinates along the trail length
  UvScroll {
    /// Scroll speed in UV units per second
    speed: f32,
  },
  /// Sine wave ribbon displacement
  Ribbon {
    /// Wave height in pixels
    amplitude: f32,
    /// Waves per trail length
    frequency: f32,
    /// Animation speed
    speed: f32,
  },
}

/// Maximum number of points a trail can store
pub const MAX_TRAIL_POINTS: usize = 128;

/// A 2D trail effect that renders a fading path behind an entity.
///
/// Trails automatically record position history and render a smooth
/// line that fades from the current position to older positions.
#[derive(Component, Debug, Clone)]
pub struct Trail2D {
  /// Position history (newest at front)
  points: VecDeque<TrailPoint>,
  /// Maximum number of points to store
  max_points: usize,
  /// Trail color at the head (newest point)
  color: Color,
  /// Optional end color for gradient (if None, fades to transparent)
  end_color: Option<Color>,
  /// Width at the head (newest point) in pixels
  width: f32,
  /// Width at the tail (oldest point) - for tapering
  end_width: f32,
  /// Whether to fade alpha along the trail
  fade_alpha: bool,
  /// Minimum distance between points (to avoid clustering)
  min_distance: f32,
  /// Whether the trail is currently active (recording points)
  enabled: bool,
  /// Whether the trail is visible (rendered). Separate from enabled to allow
  /// fade-out effects where trail stops recording but still renders.
  visible: bool,
  /// Time since last point was added (for time-based emission)
  time_accumulator: f32,
  /// Emit a point every N seconds (0 = distance-based only)
  emit_interval: f32,
  /// Maximum lifetime of trail points in seconds (None = no time-based removal)
  lifetime: Option<f32>,
  /// Distance in pixels over which to fade (None = use position-based fade)
  fade_distance: Option<f32>,
  /// Optional texture to map along the trail
  texture: Option<SpriteHandle>,
  /// How the texture is mapped along the trail
  texture_mode: TextureMode,
  /// Pixels of texture per tile (only used when texture_mode is Tile)
  tile_length: f32,
  /// Rendering style for this trail
  style: TrailStyle,
  /// Optional GPU effect applied to this trail
  effect: Option<TrailEffect>,
  /// How endpoints are drawn
  cap: LineCap,
  /// How the trail turns its corners
  join: LineJoin,
  /// How it combines with what is below
  blend: BlendMode,
  /// How its texture is sampled
  filter: Filter,
  /// Whether scene lighting applies (false: full brightness over the lit scene)
  lighting: bool,
  /// Draw order against sprites and other trails (higher draws on top)
  z_order: i32,
}

/// A single point in the trail
#[derive(Debug, Clone, Copy)]
pub struct TrailPoint {
  /// X position
  pub x: f32,
  /// Y position
  pub y: f32,
  /// Age of this point in seconds
  pub age: f32,
}

impl TrailPoint {
  /// Create a new trail point
  pub fn new(x: f32, y: f32) -> Self {
    Self { x, y, age: 0.0 }
  }
}

impl Trail2D {
  /// Create a new trail with default settings.
  ///
  /// # Arguments
  /// * `color` - The color of the trail
  /// * `width` - The width of the trail at its head (newest point)
  pub fn new(color: Color, width: f32) -> Self {
    Self {
      points: VecDeque::with_capacity(32),
      max_points: 20,
      color,
      end_color: None,
      width: width.max(0.5),
      end_width: 0.0,
      fade_alpha: true,
      min_distance: 2.0,
      enabled: true,
      visible: true,
      time_accumulator: 0.0,
      emit_interval: 0.0,
      lifetime: None,
      fade_distance: None,
      texture: None,
      texture_mode: TextureMode::Stretch,
      tile_length: 64.0,
      style: TrailStyle::Solid,
      effect: None,
      cap: LineCap::Butt,
      join: LineJoin::Miter,
      blend: BlendMode::Alpha,
      filter: Filter::Default,
      lighting: true,
      z_order: 0,
    }
  }

  /// Create a trail optimized for bullets/projectiles.
  ///
  /// Short, fast-fading trail with tapering width.
  pub fn bullet(color: Color) -> Self {
    Self::new(color, 4.0)
      .with_length(15)
      .with_taper(0.5)
      .with_min_distance(3.0)
  }

  /// Create a trail optimized for sword/weapon swipes.
  ///
  /// Wide trail that tapers to a point.
  pub fn swipe(color: Color, width: f32) -> Self {
    Self::new(color, width)
      .with_length(10)
      .with_taper(0.0)
      .with_min_distance(1.0)
  }

  /// Create a trail optimized for particles.
  ///
  /// Thin, short trail.
  pub fn particle(color: Color) -> Self {
    Self::new(color, 2.0)
      .with_length(8)
      .with_taper(0.0)
      .with_min_distance(2.0)
  }

  /// Create a trail optimized for laser/beam effects.
  ///
  /// Consistent width, no fade.
  pub fn laser(color: Color, width: f32) -> Self {
    Self::new(color, width)
      .with_length(50)
      .with_end_width(width)
      .with_fade(false)
      .with_min_distance(1.0)
  }

  /// Create a trail for fading laser effects.
  ///
  /// Consistent width with time-based fade. Perfect for laser fade-out effects
  /// where you populate the trail with `populate_line()`, then disable recording
  /// and let the lifetime cause the trail to fade and disappear.
  ///
  /// # Arguments
  /// * `color` - Laser color
  /// * `width` - Laser width in pixels
  /// * `fade_time` - Time in seconds for the laser to fade out
  pub fn fading_laser(color: Color, width: f32, fade_time: f32) -> Self {
    Self::new(color, width)
      .with_length(50)
      .with_end_width(width)
      .with_fade(true) // Enable fade for gradient effect
      .with_lifetime(fade_time)
      .with_min_distance(1.0)
  }

  /// Create a lightning/electrical arc trail.
  ///
  /// Glowing trail with jagged GPU-driven displacement.
  pub fn lightning(color: Color, width: f32) -> Self {
    Self::new(color, width)
      .with_length(30)
      .with_style(TrailStyle::Glow)
      .with_effect(TrailEffect::Lightning {
        jitter: 15.0,
        speed: 8.0,
        detail: 3,
      })
      .with_cap(LineCap::Round)
  }

  /// Create a fire/smoke trail.
  ///
  /// Soft trail with noise-based displacement and fade to transparent.
  pub fn fire(color: Color, width: f32) -> Self {
    Self::new(color, width)
      .with_length(30)
      .with_style(TrailStyle::Soft)
      .with_effect(TrailEffect::Noise {
        frequency: 3.0,
        amplitude: 10.0,
        speed: 2.0,
      })
      .with_end_color(Color::new(color.r, color.g / 2, 0, 0))
      .with_taper(0.0)
  }

  /// Create an energy beam trail.
  ///
  /// Glowing trail with scrolling UV for animated texture effects.
  pub fn energy(color: Color, width: f32) -> Self {
    Self::new(color, width)
      .with_length(50)
      .with_style(TrailStyle::Glow)
      .with_effect(TrailEffect::UvScroll { speed: 1.5 })
      .with_end_width(width) // No taper
      .with_fade(false)
  }

  /// Set the maximum number of points in the trail.
  pub fn with_length(mut self, max_points: usize) -> Self {
    self.max_points = max_points.clamp(2, MAX_TRAIL_POINTS);
    self
  }

  /// Set the end width (for tapering). 0.0 = taper to nothing.
  pub fn with_end_width(mut self, end_width: f32) -> Self {
    self.end_width = end_width.max(0.0);
    self
  }

  /// Set taper as a ratio of the start width (0.0 = point, 1.0 = no taper).
  pub fn with_taper(mut self, ratio: f32) -> Self {
    self.end_width = self.width * ratio.clamp(0.0, 1.0);
    self
  }

  /// Set whether alpha should fade along the trail.
  pub fn with_fade(mut self, fade: bool) -> Self {
    self.fade_alpha = fade;
    self
  }

  /// Set a gradient end color (otherwise fades to transparent).
  pub fn with_end_color(mut self, color: Color) -> Self {
    self.end_color = Some(color);
    self
  }

  /// Set the minimum distance between points.
  pub fn with_min_distance(mut self, distance: f32) -> Self {
    self.min_distance = distance.max(0.1);
    self
  }

  /// Set time-based emission interval.
  ///
  /// When > 0, a point is emitted every N seconds regardless of distance.
  pub fn with_emit_interval(mut self, interval: f32) -> Self {
    self.emit_interval = interval.max(0.0);
    self
  }

  /// Set the maximum lifetime for trail points.
  ///
  /// Points older than this are automatically removed. When combined with
  /// `fade_alpha`, points will smoothly fade out as they approach the lifetime.
  ///
  /// # Arguments
  /// * `seconds` - Maximum age in seconds before points are removed
  ///
  /// # Example
  ///
  /// ```ignore
  /// // Trail points live for 0.5 seconds then disappear
  /// let trail = Trail2D::new(Color::YELLOW, 5.0)
  ///     .with_lifetime(0.5);
  /// ```
  pub fn with_lifetime(mut self, seconds: f32) -> Self {
    self.lifetime = Some(seconds.max(0.01));
    self
  }

  /// Set the fade distance in pixels.
  ///
  /// When set, the trail fades based on actual distance from the head rather than
  /// position in the point array. This provides more predictable fading regardless
  /// of entity speed - fast objects won't have stretched-out fades.
  ///
  /// # Arguments
  /// * `pixels` - Distance in pixels over which to fade from opaque to transparent
  ///
  /// # Example
  ///
  /// ```ignore
  /// // Trail fades over 100 pixels of distance
  /// let trail = Trail2D::new(Color::YELLOW, 5.0)
  ///     .with_fade_distance(100.0);
  /// ```
  pub fn with_fade_distance(mut self, pixels: f32) -> Self {
    self.fade_distance = Some(pixels.max(1.0));
    self
  }

  /// Set a texture to map along the trail.
  ///
  /// The texture will be stretched or tiled along the trail's length,
  /// and stretched across the trail's width. The texture color is multiplied
  /// with the trail's color for tinting effects.
  ///
  /// # Example
  ///
  /// ```ignore
  /// // Create a fire trail with a flame texture
  /// let trail = Trail2D::new(Color::WHITE, 32.0)
  ///     .with_texture(flame_sprite)
  ///     .with_texture_mode(TextureMode::Tile)
  ///     .with_tile_length(64.0);
  /// ```
  pub fn with_texture(mut self, texture: SpriteHandle) -> Self {
    self.texture = Some(texture);
    self
  }

  /// Set how the texture is mapped along the trail.
  ///
  /// - `TextureMode::Stretch` - Stretches the texture to fit the entire trail length
  /// - `TextureMode::Tile` - Repeats the texture along the trail based on `tile_length`
  pub fn with_texture_mode(mut self, mode: TextureMode) -> Self {
    self.texture_mode = mode;
    self
  }

  /// Set the length in pixels for each texture tile (when using `TextureMode::Tile`).
  ///
  /// The texture will repeat every `length` pixels along the trail.
  /// Default is 64 pixels.
  pub fn with_tile_length(mut self, length: f32) -> Self {
    self.tile_length = length.max(1.0);
    self
  }

  /// Get the trail color.
  pub fn color(&self) -> Color {
    self.color
  }

  /// Set the trail color.
  pub fn set_color(&mut self, color: Color) {
    self.color = color;
  }

  /// Get the trail width.
  pub fn width(&self) -> f32 {
    self.width
  }

  /// Set the trail width.
  pub fn set_width(&mut self, width: f32) {
    self.width = width.max(0.5);
  }

  /// Check if the trail is enabled (recording new points).
  pub fn is_enabled(&self) -> bool {
    self.enabled
  }

  /// Enable or disable the trail.
  ///
  /// When disabled, the trail stops recording new points AND becomes invisible.
  /// This is the expected behavior for most use cases (toggle trail on/off).
  ///
  /// For fade-out effects where you want to stop recording but keep rendering,
  /// use `set_recording(false)` instead, which only stops point recording.
  pub fn set_enabled(&mut self, enabled: bool) {
    self.enabled = enabled;
    self.visible = enabled; // Sync visibility with enabled state
  }

  /// Check if the trail is visible (will be rendered).
  pub fn is_visible(&self) -> bool {
    self.visible
  }

  /// Set trail visibility independently of recording state.
  ///
  /// Use this for advanced control, e.g., temporarily hiding a trail
  /// while keeping it recording, or showing a trail that's no longer recording.
  pub fn set_visible(&mut self, visible: bool) {
    self.visible = visible;
  }

  /// Enable or disable point recording without affecting visibility.
  ///
  /// Use this for fade-out effects: call `set_recording(false)` to stop
  /// adding new points while the existing trail continues to render and fade.
  pub fn set_recording(&mut self, recording: bool) {
    self.enabled = recording;
  }

  /// Get the current points in the trail.
  pub fn points(&self) -> &VecDeque<TrailPoint> {
    &self.points
  }

  /// Get the number of points currently in the trail.
  pub fn point_count(&self) -> usize {
    self.points.len()
  }

  /// Clear all points from the trail.
  pub fn clear(&mut self) {
    self.points.clear();
  }

  /// Manually add a point to the front (newest) of the trail.
  ///
  /// This is useful for creating static trails like laser fade-out effects
  /// where you want to populate the trail with specific points rather than
  /// recording position history.
  pub fn push_point(&mut self, x: f32, y: f32) {
    self.points.push_front(TrailPoint::new(x, y));
    while self.points.len() > self.max_points {
      self.points.pop_back();
    }
  }

  /// Manually add a point to the back (oldest) of the trail.
  ///
  /// Useful for building trails from start to end when populating
  /// a laser or line effect.
  pub fn push_point_back(&mut self, x: f32, y: f32) {
    self.points.push_back(TrailPoint::new(x, y));
    while self.points.len() > self.max_points {
      self.points.pop_front();
    }
  }

  /// Populate the trail with evenly spaced points along a line.
  ///
  /// Creates a trail from `start` to `end` with the specified number of points.
  /// The trail will have `start` at the head (newest) and `end` at the tail (oldest).
  /// This is perfect for creating laser fade-out effects.
  ///
  /// # Arguments
  /// * `start_x`, `start_y` - Starting position (head of trail)
  /// * `end_x`, `end_y` - Ending position (tail of trail)
  /// * `num_points` - Number of points to create (minimum 2)
  pub fn populate_line(
    &mut self,
    start_x: f32,
    start_y: f32,
    end_x: f32,
    end_y: f32,
    num_points: usize,
  ) {
    self.points.clear();
    let num_points = num_points.max(2).min(self.max_points);

    for i in 0..num_points {
      let t = i as f32 / (num_points - 1) as f32;
      let x = start_x + (end_x - start_x) * t;
      let y = start_y + (end_y - start_y) * t;
      self.points.push_back(TrailPoint::new(x, y));
    }
  }

  /// Update the trail with a new position.
  ///
  /// The engine calls this every fixed step with the entity's world position
  /// (`FixedSet::Trails`); call it yourself only for a trail that follows
  /// something else. Points are automatically added based on distance and time thresholds,
  /// and removed when they exceed the lifetime (if set).
  ///
  /// When `enabled` is false, points are still aged and removed (for fade-out effects),
  /// but no new points are recorded. This allows trails to fade out gracefully after
  /// an entity stops moving or a laser stops firing.
  pub fn update(&mut self, x: f32, y: f32, dt: f32) {
    // Age existing points (always, even when disabled for fade-out effects)
    for point in &mut self.points {
      point.age += dt;
    }

    // Remove points that exceed lifetime (always, even when disabled)
    if let Some(max_age) = self.lifetime {
      while self.points.back().is_some_and(|point| point.age > max_age) {
        self.points.pop_back();
      }
    }

    // Only record new points when enabled
    if !self.enabled {
      return;
    }

    // Check if we should add a new point
    let should_add = if let Some(front) = self.points.front() {
      let dx = x - front.x;
      let dy = y - front.y;
      let dist = (dx * dx + dy * dy).sqrt();

      // Distance-based emission
      let dist_emit = dist >= self.min_distance;

      // Time-based emission
      self.time_accumulator += dt;
      let time_emit = self.emit_interval > 0.0 && self.time_accumulator >= self.emit_interval;

      if time_emit {
        self.time_accumulator = 0.0;
      }

      dist_emit || time_emit
    } else {
      true // Always add first point
    };

    if should_add {
      // Add new point at front
      self.points.push_front(TrailPoint::new(x, y));

      // Remove excess points (max_points limit)
      while self.points.len() > self.max_points {
        self.points.pop_back();
      }
    }
  }

  /// Get interpolated color at a position along the trail (0.0 = head, 1.0 = tail).
  pub fn color_at(&self, t: f32) -> Color {
    let t = t.clamp(0.0, 1.0);

    let end = self
      .end_color
      .unwrap_or(Color::new(self.color.r, self.color.g, self.color.b, 0));

    let r = lerp(self.color.r as f32, end.r as f32, t) as u8;
    let g = lerp(self.color.g as f32, end.g as f32, t) as u8;
    let b = lerp(self.color.b as f32, end.b as f32, t) as u8;
    let a = if self.fade_alpha {
      lerp(self.color.a as f32, end.a as f32, t) as u8
    } else {
      self.color.a
    };

    Color::new(r, g, b, a)
  }

  /// Get interpolated width at a position along the trail (0.0 = head, 1.0 = tail).
  pub fn width_at(&self, t: f32) -> f32 {
    lerp(self.width, self.end_width, t.clamp(0.0, 1.0))
  }

  /// Get the maximum number of points.
  pub fn max_points(&self) -> usize {
    self.max_points
  }

  /// Check if alpha fading is enabled.
  pub fn fades_alpha(&self) -> bool {
    self.fade_alpha
  }

  /// Get the end width.
  pub fn end_width(&self) -> f32 {
    self.end_width
  }

  /// Get the lifetime setting.
  pub fn lifetime(&self) -> Option<f32> {
    self.lifetime
  }

  /// Set the lifetime for trail points.
  ///
  /// Pass `Some(seconds)` to enable time-based removal, or `None` to disable.
  pub fn set_lifetime(&mut self, lifetime: Option<f32>) {
    self.lifetime = lifetime.map(|l| l.max(0.01));
  }

  /// Get the age-based alpha multiplier for a point.
  ///
  /// When lifetime is set, returns a value from 1.0 (new) to 0.0 (about to expire).
  /// When lifetime is not set, always returns 1.0.
  pub fn age_alpha(&self, age: f32) -> f32 {
    match self.lifetime {
      Some(max_age) => (1.0 - (age / max_age)).clamp(0.0, 1.0),
      None => 1.0,
    }
  }

  /// Get the fade distance setting.
  pub fn fade_distance(&self) -> Option<f32> {
    self.fade_distance
  }

  /// Set the fade distance.
  ///
  /// Pass `Some(pixels)` to enable distance-based fade, or `None` to disable.
  pub fn set_fade_distance(&mut self, distance: Option<f32>) {
    self.fade_distance = distance.map(|d| d.max(1.0));
  }

  /// Get the distance-based alpha multiplier.
  ///
  /// When fade_distance is set, returns a value from 1.0 (at head) to 0.0 (at fade_distance).
  /// When fade_distance is not set, always returns 1.0.
  pub fn distance_alpha(&self, distance_from_head: f32) -> f32 {
    match self.fade_distance {
      Some(max_dist) => (1.0 - (distance_from_head / max_dist)).clamp(0.0, 1.0),
      None => 1.0,
    }
  }

  /// Calculate cumulative distances from head for each point.
  ///
  /// Returns a Vec where index 0 is always 0.0 (the head), and each subsequent
  /// index contains the total distance traveled from the head to that point.
  pub fn cumulative_distances(&self) -> Vec<f32> {
    let mut distances = Vec::with_capacity(self.points.len());
    let mut total = 0.0;

    for (i, point) in self.points.iter().enumerate() {
      if i == 0 {
        distances.push(0.0);
      } else {
        let prev = &self.points[i - 1];
        let dx = point.x - prev.x;
        let dy = point.y - prev.y;
        total += (dx * dx + dy * dy).sqrt();
        distances.push(total);
      }
    }

    distances
  }

  /// Get the total length of the trail in pixels.
  pub fn total_length(&self) -> f32 {
    if self.points.len() < 2 {
      return 0.0;
    }

    let mut total = 0.0;
    for i in 1..self.points.len() {
      let prev = &self.points[i - 1];
      let curr = &self.points[i];
      let dx = curr.x - prev.x;
      let dy = curr.y - prev.y;
      total += (dx * dx + dy * dy).sqrt();
    }
    total
  }

  /// Get the texture assigned to this trail, if any.
  pub fn texture(&self) -> Option<&SpriteHandle> {
    self.texture.as_ref()
  }

  /// Set or clear the texture for this trail.
  pub fn set_texture(&mut self, texture: Option<SpriteHandle>) {
    self.texture = texture;
  }

  /// Get the texture mapping mode.
  pub fn texture_mode(&self) -> TextureMode {
    self.texture_mode
  }

  /// Set the texture mapping mode.
  pub fn set_texture_mode(&mut self, mode: TextureMode) {
    self.texture_mode = mode;
  }

  /// Get the tile length (pixels per texture repeat in Tile mode).
  pub fn tile_length(&self) -> f32 {
    self.tile_length
  }

  /// Set the tile length.
  pub fn set_tile_length(&mut self, length: f32) {
    self.tile_length = length.max(1.0);
  }

  /// Set the rendering style for this trail.
  pub fn with_style(mut self, style: TrailStyle) -> Self {
    self.style = style;
    self
  }

  /// Set a GPU effect for this trail.
  pub fn with_effect(mut self, effect: TrailEffect) -> Self {
    self.effect = Some(effect);
    self
  }

  /// Set the line cap style for endpoints.
  pub fn with_cap(mut self, cap: LineCap) -> Self {
    self.cap = cap;
    self
  }

  /// Get the rendering style.
  pub fn style(&self) -> TrailStyle {
    self.style
  }

  /// Set the rendering style.
  pub fn set_style(&mut self, style: TrailStyle) {
    self.style = style;
  }

  /// Get the GPU effect, if any.
  pub fn effect(&self) -> Option<&TrailEffect> {
    self.effect.as_ref()
  }

  /// Set or clear the GPU effect.
  pub fn set_effect(&mut self, effect: Option<TrailEffect>) {
    self.effect = effect;
  }

  /// Get the line cap style.
  pub fn cap(&self) -> LineCap {
    self.cap
  }

  /// Set the line cap style.
  pub fn set_cap(&mut self, cap: LineCap) {
    self.cap = cap;
  }

  /// Set how the trail turns its corners (default `Miter`).
  pub fn with_join(mut self, join: LineJoin) -> Self {
    self.join = join;
    self
  }

  /// How the trail turns its corners.
  pub fn join(&self) -> LineJoin {
    self.join
  }

  /// Set how the trail turns its corners.
  pub fn set_join(&mut self, join: LineJoin) {
    self.join = join;
  }

  /// Set how the trail combines with what is below (default `Alpha`;
  /// `Additive` for glowing trails).
  pub fn with_blend(mut self, blend: BlendMode) -> Self {
    self.blend = blend;
    self
  }

  /// How the trail combines with what is below.
  pub fn blend(&self) -> BlendMode {
    self.blend
  }

  /// Set how the trail's texture is sampled (default: the presentation's).
  pub fn with_filter(mut self, filter: Filter) -> Self {
    self.filter = filter;
    self
  }

  /// How the trail's texture is sampled.
  pub fn filter(&self) -> Filter {
    self.filter
  }

  /// Set whether scene lighting applies (default `true`).
  ///
  /// With `false` the trail is drawn at full brightness on top of the lit scene
  /// (before post-processing), e.g. for glowing magic in the dark. Without
  /// lighting the setting has no effect.
  pub fn with_lighting(mut self, enabled: bool) -> Self {
    self.lighting = enabled;
    self
  }

  /// Whether scene lighting applies to this trail.
  pub fn lighting(&self) -> bool {
    self.lighting
  }

  /// Set whether scene lighting applies to this trail.
  pub fn set_lighting(&mut self, enabled: bool) {
    self.lighting = enabled;
  }

  /// Set the draw order (default 0). Trails sort with entities, `Scene::render`
  /// sprites and lines by z: a trail draws over anything with a lower z and
  /// under anything with a higher one. At equal z, trails draw after
  /// entities and sprites, so give a trail its entity's z to keep it in front.
  pub fn with_z_order(mut self, z_order: i32) -> Self {
    self.z_order = z_order;
    self
  }

  /// The draw order of this trail (see [`Trail2D::with_z_order`]).
  pub fn z_order(&self) -> i32 {
    self.z_order
  }

  /// Set the draw order of this trail (see [`Trail2D::with_z_order`]).
  pub fn set_z_order(&mut self, z_order: i32) {
    self.z_order = z_order;
  }
}

impl Default for Trail2D {
  fn default() -> Self {
    Self::new(Color::WHITE, 3.0)
  }
}

/// Linear interpolation helper
fn lerp(a: f32, b: f32, t: f32) -> f32 {
  a + (b - a) * t
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn trail_creation() {
    let trail = Trail2D::new(Color::RED, 5.0);
    assert_eq!(trail.width(), 5.0);
    assert_eq!(trail.color(), Color::RED);
    assert!(trail.is_enabled());
    assert_eq!(trail.point_count(), 0);
  }

  #[test]
  fn trail_update_adds_points() {
    let mut trail = Trail2D::new(Color::WHITE, 3.0).with_min_distance(1.0);

    trail.update(0.0, 0.0, 0.016);
    assert_eq!(trail.point_count(), 1);

    trail.update(10.0, 0.0, 0.016);
    assert_eq!(trail.point_count(), 2);
  }

  #[test]
  fn trail_respects_min_distance() {
    let mut trail = Trail2D::new(Color::WHITE, 3.0).with_min_distance(10.0);

    trail.update(0.0, 0.0, 0.016);
    trail.update(5.0, 0.0, 0.016); // Less than min_distance
    assert_eq!(trail.point_count(), 1); // Should not add
  }

  #[test]
  fn trail_respects_max_points() {
    let mut trail = Trail2D::new(Color::WHITE, 3.0)
      .with_length(5)
      .with_min_distance(1.0);

    for i in 0..10 {
      trail.update(i as f32 * 10.0, 0.0, 0.016);
    }

    assert_eq!(trail.point_count(), 5);
  }

  #[test]
  fn trail_color_interpolation() {
    let trail = Trail2D::new(Color::WHITE, 3.0);

    let head_color = trail.color_at(0.0);
    assert_eq!(head_color.r, 255);

    let tail_color = trail.color_at(1.0);
    assert_eq!(tail_color.a, 0); // Fades to transparent
  }

  #[test]
  fn trail_width_interpolation() {
    let trail = Trail2D::new(Color::WHITE, 10.0).with_end_width(2.0);

    assert!((trail.width_at(0.0) - 10.0).abs() < 0.01);
    assert!((trail.width_at(1.0) - 2.0).abs() < 0.01);
    assert!((trail.width_at(0.5) - 6.0).abs() < 0.01);
  }

  #[test]
  fn bullet_preset() {
    let trail = Trail2D::bullet(Color::YELLOW);
    assert_eq!(trail.max_points(), 15);
    assert!(trail.fades_alpha());
  }

  #[test]
  fn laser_preset() {
    let trail = Trail2D::laser(Color::RED, 8.0);
    assert!(!trail.fades_alpha());
    assert!((trail.width() - trail.end_width()).abs() < 0.01);
  }

  #[test]
  fn trail_clear() {
    let mut trail = Trail2D::new(Color::WHITE, 3.0);
    trail.update(0.0, 0.0, 0.016);
    trail.update(10.0, 0.0, 0.016);
    assert!(trail.point_count() > 0);

    trail.clear();
    assert_eq!(trail.point_count(), 0);
  }

  #[test]
  fn trail_disabled() {
    let mut trail = Trail2D::new(Color::WHITE, 3.0);
    trail.set_enabled(false);

    trail.update(0.0, 0.0, 0.016);
    trail.update(10.0, 0.0, 0.016);
    assert_eq!(trail.point_count(), 0);
  }

  #[test]
  fn trail_lifetime_removes_old_points() {
    let mut trail = Trail2D::new(Color::WHITE, 3.0)
      .with_lifetime(0.1) // 100ms lifetime
      .with_min_distance(1.0);

    // Add some points
    trail.update(0.0, 0.0, 0.0);
    trail.update(10.0, 0.0, 0.02);
    trail.update(20.0, 0.0, 0.02);
    assert_eq!(trail.point_count(), 3);

    // Age points past lifetime
    trail.update(30.0, 0.0, 0.1); // First point now at 0.14s age
    // Oldest point should be removed (age > 0.1)
    assert!(trail.point_count() < 4);
  }

  #[test]
  fn trail_lifetime_default_is_none() {
    let trail = Trail2D::new(Color::WHITE, 3.0);
    assert!(trail.lifetime().is_none());
  }

  #[test]
  fn trail_with_lifetime_builder() {
    let trail = Trail2D::new(Color::WHITE, 3.0).with_lifetime(0.5);
    assert_eq!(trail.lifetime(), Some(0.5));
  }

  #[test]
  fn trail_age_alpha_with_lifetime() {
    let trail = Trail2D::new(Color::WHITE, 3.0).with_lifetime(1.0);

    // New point (age 0) = full alpha
    assert!((trail.age_alpha(0.0) - 1.0).abs() < 0.01);
    // Half-life point = half alpha
    assert!((trail.age_alpha(0.5) - 0.5).abs() < 0.01);
    // Expired point = zero alpha
    assert!((trail.age_alpha(1.0) - 0.0).abs() < 0.01);
  }

  #[test]
  fn trail_age_alpha_without_lifetime() {
    let trail = Trail2D::new(Color::WHITE, 3.0); // No lifetime

    // Without lifetime, age_alpha always returns 1.0
    assert!((trail.age_alpha(0.0) - 1.0).abs() < 0.01);
    assert!((trail.age_alpha(100.0) - 1.0).abs() < 0.01);
  }

  #[test]
  fn trail_distance_alpha_with_fade_distance() {
    let trail = Trail2D::new(Color::WHITE, 3.0).with_fade_distance(100.0);

    // At head (distance 0) = full alpha
    assert!((trail.distance_alpha(0.0) - 1.0).abs() < 0.01);
    // Half distance = half alpha
    assert!((trail.distance_alpha(50.0) - 0.5).abs() < 0.01);
    // At fade distance = zero alpha
    assert!((trail.distance_alpha(100.0) - 0.0).abs() < 0.01);
    // Beyond fade distance = clamped to zero
    assert!((trail.distance_alpha(150.0) - 0.0).abs() < 0.01);
  }

  #[test]
  fn trail_distance_alpha_without_fade_distance() {
    let trail = Trail2D::new(Color::WHITE, 3.0); // No fade_distance

    // Without fade_distance, distance_alpha always returns 1.0
    assert!((trail.distance_alpha(0.0) - 1.0).abs() < 0.01);
    assert!((trail.distance_alpha(100.0) - 1.0).abs() < 0.01);
  }

  #[test]
  fn trail_cumulative_distances() {
    let mut trail = Trail2D::new(Color::WHITE, 3.0).with_min_distance(1.0);

    // Add points in a straight line: 0, 10, 25
    // Points are stored newest-first, so after all updates:
    // Index 0 = (25, 0) - head/newest
    // Index 1 = (10, 0)
    // Index 2 = (0, 0) - tail/oldest
    trail.update(0.0, 0.0, 0.016);
    trail.update(10.0, 0.0, 0.016);
    trail.update(25.0, 0.0, 0.016);

    let distances = trail.cumulative_distances();
    assert_eq!(distances.len(), 3);
    assert!((distances[0] - 0.0).abs() < 0.01); // Head is always 0
    assert!((distances[1] - 15.0).abs() < 0.01); // 15 pixels from head (25 -> 10)
    assert!((distances[2] - 25.0).abs() < 0.01); // 25 pixels from head (25 -> 10 -> 0)
  }

  #[test]
  fn trail_total_length() {
    let mut trail = Trail2D::new(Color::WHITE, 3.0).with_min_distance(1.0);

    // Add points: 0 -> 10 -> 25 (total length = 25)
    trail.update(0.0, 0.0, 0.016);
    trail.update(10.0, 0.0, 0.016);
    trail.update(25.0, 0.0, 0.016);

    assert!((trail.total_length() - 25.0).abs() < 0.01);
  }

  // ========== Texture Mapping Tests ==========

  #[test]
  fn trail_texture_mode_default() {
    let trail = Trail2D::new(Color::WHITE, 3.0);
    assert!(trail.texture().is_none());
    assert_eq!(trail.texture_mode(), TextureMode::Stretch);
    assert!((trail.tile_length() - 64.0).abs() < 0.01); // Default tile length
  }

  #[test]
  fn trail_with_texture_mode_stretch() {
    let trail = Trail2D::new(Color::WHITE, 3.0).with_texture_mode(TextureMode::Stretch);
    assert_eq!(trail.texture_mode(), TextureMode::Stretch);
  }

  #[test]
  fn trail_with_texture_mode_tile() {
    let trail = Trail2D::new(Color::WHITE, 3.0).with_texture_mode(TextureMode::Tile);
    assert_eq!(trail.texture_mode(), TextureMode::Tile);
  }

  #[test]
  fn trail_with_tile_length() {
    let trail = Trail2D::new(Color::WHITE, 3.0).with_tile_length(64.0);
    assert!((trail.tile_length() - 64.0).abs() < 0.01);
  }

  #[test]
  fn trail_tile_length_minimum() {
    // Tile length should be clamped to at least 1.0
    let trail = Trail2D::new(Color::WHITE, 3.0).with_tile_length(0.0);
    assert!(trail.tile_length() >= 1.0);

    let trail2 = Trail2D::new(Color::WHITE, 3.0).with_tile_length(-10.0);
    assert!(trail2.tile_length() >= 1.0);
  }

  #[test]
  fn trail_texture_mode_enum_equality() {
    assert_eq!(TextureMode::Stretch, TextureMode::Stretch);
    assert_eq!(TextureMode::Tile, TextureMode::Tile);
    assert_ne!(TextureMode::Stretch, TextureMode::Tile);
  }

  #[test]
  fn trail_texture_mode_default_is_stretch() {
    assert_eq!(TextureMode::default(), TextureMode::Stretch);
  }

  // ========== New Type Tests ==========

  #[test]
  fn line_cap_defaults_to_butt() {
    assert_eq!(LineCap::default(), LineCap::Butt);
  }

  #[test]
  fn line_join_defaults_to_miter() {
    assert_eq!(LineJoin::default(), LineJoin::Miter);
  }

  #[test]
  fn a_trail_takes_a_join() {
    let trail = Trail2D::new(Color::WHITE, 4.0).with_join(LineJoin::Round);
    assert_eq!(trail.join(), LineJoin::Round);
  }

  #[test]
  fn trail_effect_lightning_clone() {
    let effect = TrailEffect::Lightning {
      jitter: 15.0,
      speed: 8.0,
      detail: 3,
    };
    let cloned = effect;
    assert_eq!(effect, cloned);
  }

  #[test]
  fn trail_effect_noise_partial_eq() {
    let a = TrailEffect::Noise {
      frequency: 2.0,
      amplitude: 10.0,
      speed: 1.5,
    };
    let b = TrailEffect::Noise {
      frequency: 2.0,
      amplitude: 10.0,
      speed: 1.5,
    };
    assert_eq!(a, b);
  }

  #[test]
  fn trail_effect_uv_scroll() {
    let effect = TrailEffect::UvScroll { speed: 1.5 };
    if let TrailEffect::UvScroll { speed } = effect {
      assert!((speed - 1.5).abs() < 0.01);
    } else {
      panic!("Wrong variant");
    }
  }

  #[test]
  fn trail_effect_ribbon() {
    let effect = TrailEffect::Ribbon {
      amplitude: 10.0,
      frequency: 3.0,
      speed: 2.0,
    };
    if let TrailEffect::Ribbon {
      amplitude,
      frequency,
      speed,
    } = effect
    {
      assert!((amplitude - 10.0).abs() < 0.01);
      assert!((frequency - 3.0).abs() < 0.01);
      assert!((speed - 2.0).abs() < 0.01);
    } else {
      panic!("Wrong variant");
    }
  }

  // ========== Trail2D Style/Effect/Cap/Join Tests ==========

  #[test]
  fn trail_default_style_is_solid() {
    let trail = Trail2D::new(Color::WHITE, 3.0);
    assert_eq!(trail.style(), TrailStyle::Solid);
  }

  #[test]
  fn trail_with_style() {
    let trail = Trail2D::new(Color::WHITE, 3.0).with_style(TrailStyle::Glow);
    assert_eq!(trail.style(), TrailStyle::Glow);
  }

  #[test]
  fn trail_set_style() {
    let mut trail = Trail2D::new(Color::WHITE, 3.0);
    trail.set_style(TrailStyle::Soft);
    assert_eq!(trail.style(), TrailStyle::Soft);
  }

  #[test]
  fn trail_default_effect_is_none() {
    let trail = Trail2D::new(Color::WHITE, 3.0);
    assert!(trail.effect().is_none());
  }

  #[test]
  fn trail_with_effect() {
    let trail = Trail2D::new(Color::WHITE, 3.0).with_effect(TrailEffect::Lightning {
      jitter: 15.0,
      speed: 8.0,
      detail: 3,
    });
    assert!(trail.effect().is_some());
  }

  #[test]
  fn trail_set_effect_none() {
    let mut trail =
      Trail2D::new(Color::WHITE, 3.0).with_effect(TrailEffect::UvScroll { speed: 1.0 });
    trail.set_effect(None);
    assert!(trail.effect().is_none());
  }

  #[test]
  fn trail_default_cap_is_butt() {
    let trail = Trail2D::new(Color::WHITE, 3.0);
    assert_eq!(trail.cap(), LineCap::Butt);
  }

  #[test]
  fn trail_with_cap() {
    let trail = Trail2D::new(Color::WHITE, 3.0).with_cap(LineCap::Round);
    assert_eq!(trail.cap(), LineCap::Round);
  }

  // ========== New Preset Tests ==========

  #[test]
  fn lightning_preset() {
    let trail = Trail2D::lightning(Color::CYAN, 6.0);
    assert_eq!(trail.style(), TrailStyle::Glow);
    assert!(trail.effect().is_some());
    assert_eq!(trail.cap(), LineCap::Round);
    if let Some(TrailEffect::Lightning { .. }) = trail.effect() {
      // Correct variant
    } else {
      panic!("Expected Lightning effect");
    }
  }

  #[test]
  fn fire_preset() {
    let trail = Trail2D::fire(Color::new(255, 120, 20, 255), 16.0);
    assert_eq!(trail.style(), TrailStyle::Soft);
    assert!(trail.effect().is_some());
    if let Some(TrailEffect::Noise { .. }) = trail.effect() {
      // Correct variant
    } else {
      panic!("Expected Noise effect");
    }
  }

  #[test]
  fn energy_preset() {
    let trail = Trail2D::energy(Color::WHITE, 12.0);
    assert_eq!(trail.style(), TrailStyle::Glow);
    assert!(trail.effect().is_some());
    if let Some(TrailEffect::UvScroll { .. }) = trail.effect() {
      // Correct variant
    } else {
      panic!("Expected UvScroll effect");
    }
  }
}
