//! Scrolling component - for parallax effects

/// Makes an entity scroll horizontally at a given speed
#[derive(Debug, Clone, Copy)]
pub struct Scrolling {
  pub speed: f32,
  /// Width of a single tile - when x <= -tile_width, the tile wraps
  pub tile_width: f32,
  /// Total width to wrap by (typically 2 * tile_width for seamless 2-tile scrolling)
  pub wrap_distance: f32,
}

impl Scrolling {
  /// Create a new scrolling component for seamless looping
  ///
  /// - `speed`: How fast to scroll (pixels per second)
  /// - `tile_width`: Width of the sprite/tile
  /// - `wrap_distance`: How far to jump when wrapping (usually 2 * tile_width for 2 tiles)
  pub fn new(speed: f32, tile_width: f32, wrap_distance: f32) -> Self {
    Self {
      speed,
      tile_width,
      wrap_distance,
    }
  }

  /// Create scrolling for a single tile that doesn't wrap (just scrolls off screen)
  pub fn no_wrap(speed: f32) -> Self {
    Self {
      speed,
      tile_width: f32::MAX,
      wrap_distance: 0.0,
    }
  }

  pub fn update(&self, current_x: f32, dt: f32) -> f32 {
    let new_x = current_x - self.speed * dt;
    // Wrap when the tile has scrolled completely off the left edge
    if new_x <= -self.tile_width {
      new_x + self.wrap_distance
    } else {
      new_x
    }
  }
}
