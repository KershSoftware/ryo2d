//! Parallax tiling modes, used by [`GpuParallaxLayer`](super::GpuParallaxLayer).

/// Controls how a parallax layer tiles horizontally.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TileMode {
  /// No tiling - the sprite is drawn once at its position.
  /// Suitable for static sky backgrounds or non-repeating elements.
  #[default]
  None,

  /// Tile horizontally to fill the screen width.
  /// The sprite repeats seamlessly across the screen.
  Horizontal,

  /// Infinite horizontal scrolling with seamless wrapping.
  /// The sprite tiles continuously as the camera moves,
  /// creating an endless scrolling effect.
  InfiniteScroll,
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn test_tile_mode_default() {
    assert_eq!(TileMode::default(), TileMode::None);
  }
}
