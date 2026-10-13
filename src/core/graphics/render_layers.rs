//! Which cameras see what.

use crate::core::ecs::Component;

/// The layers an entity is on, or a camera sees (32 of them, 0..=31). A
/// camera draws an entity when they share a layer.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RenderLayers(u32);

impl RenderLayers {
  /// Only layer `n` (above 31 is 31).
  pub const fn layer(n: u8) -> Self {
    let n = if n > 31 { 31 } else { n };
    Self(1 << n)
  }

  /// Every layer.
  pub const fn all() -> Self {
    Self(u32::MAX)
  }

  /// No layer: nothing sees it.
  pub const fn none() -> Self {
    Self(0)
  }

  /// These layers and layer `n`.
  pub const fn with(self, n: u8) -> Self {
    Self(self.0 | Self::layer(n).0)
  }

  /// Whether the two share a layer.
  pub const fn intersects(&self, other: &RenderLayers) -> bool {
    self.0 & other.0 != 0
  }
}

impl Default for RenderLayers {
  fn default() -> Self {
    Self::layer(0)
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn layers_intersect_when_they_share_one() {
    assert_eq!(RenderLayers::default(), RenderLayers::layer(0));
    assert!(
      RenderLayers::layer(3)
        .with(5)
        .intersects(&RenderLayers::layer(5))
    );
    assert!(!RenderLayers::layer(3).intersects(&RenderLayers::layer(4)));
    assert!(RenderLayers::all().intersects(&RenderLayers::layer(31)));
    assert!(!RenderLayers::none().intersects(&RenderLayers::all()));
    assert_eq!(
      RenderLayers::layer(40),
      RenderLayers::layer(31),
      "out-of-range layers clamp to 31"
    );
  }
}
