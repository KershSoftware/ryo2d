//! Ball Component

/// Marker component for the ball
#[derive(Debug, Clone)]
pub struct Ball {
  /// Whether the ball is currently frozen (waiting for launch)
  pub frozen: bool,
}

impl Ball {
  /// Create a new ball that starts frozen
  pub fn new() -> Self {
    Self { frozen: true }
  }
}

impl Default for Ball {
  fn default() -> Self {
    Self::new()
  }
}
