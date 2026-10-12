//! Small shared components.

use serde::{Deserialize, Serialize};

use super::kernel::Component;

/// Common render layers for organising draw order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum RenderLayer {
  Background = -1000,
  Default = 0,
  Foreground = 1000,
  UI = 2000,
}

impl RenderLayer {
  pub fn as_z_order(self) -> i32 {
    self as i32
  }
}

/// Movement per second, in pixels.
#[derive(Component, Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct Velocity {
  pub dx: f32,
  pub dy: f32,
}

impl Velocity {
  pub fn new(dx: f32, dy: f32) -> Self {
    Self { dx, dy }
  }

  pub fn zero() -> Self {
    Self::new(0.0, 0.0)
  }

  pub fn from_vec2(v: glam::Vec2) -> Self {
    Self::new(v.x, v.y)
  }

  pub fn as_vec2(&self) -> glam::Vec2 {
    glam::Vec2::new(self.dx, self.dy)
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn velocity_round_trips_through_vec2() {
    let v = Velocity::from_vec2(glam::Vec2::new(3.0, -4.0));
    assert_eq!(v, Velocity::new(3.0, -4.0));
    assert_eq!(v.as_vec2(), glam::Vec2::new(3.0, -4.0));
    assert_eq!(Velocity::zero(), Velocity::default());
  }

  #[test]
  fn render_layers_order_by_z() {
    assert!(RenderLayer::Background.as_z_order() < RenderLayer::Default.as_z_order());
    assert!(RenderLayer::Foreground.as_z_order() < RenderLayer::UI.as_z_order());
  }
}
