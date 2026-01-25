//! Built-in components for the game engine

use crate::sprite::Sprite;

/// Position and rotation in 2D space
#[derive(Debug, Clone)]
pub struct Transform {
  pub x: f32,
  pub y: f32,
  pub rotation: f32,
  pub scale_x: f32,
  pub scale_y: f32,
}

impl Transform {
  pub fn new(x: f32, y: f32) -> Self {
    Transform {
      x,
      y,
      rotation: 0.0,
      scale_x: 1.0,
      scale_y: 1.0,
    }
  }

  pub fn origin() -> Self {
    Self::new(0.0, 0.0)
  }
}

impl Default for Transform {
  fn default() -> Self {
    Self::origin()
  }
}

/// Movement per second
#[derive(Debug, Clone, Default)]
pub struct Velocity {
  pub dx: f32,
  pub dy: f32,
}

impl Velocity {
  pub fn new(dx: f32, dy: f32) -> Self {
    Velocity { dx, dy }
  }

  pub fn zero() -> Self {
    Self::new(0.0, 0.0)
  }
}

/// Visual representation for rendering
#[derive(Clone)]
pub struct SpriteRenderer {
  pub sprite: Sprite,
  pub visible: bool,
  pub z_order: i32,
}

impl SpriteRenderer {
  pub fn new(sprite: Sprite) -> Self {
    SpriteRenderer {
      sprite,
      visible: true,
      z_order: 0,
    }
  }

  pub fn with_z_order(mut self, z: i32) -> Self {
    self.z_order = z;
    self
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn transform_defaults() {
    let t = Transform::default();
    assert_eq!(t.x, 0.0);
    assert_eq!(t.y, 0.0);
    assert_eq!(t.rotation, 0.0);
    assert_eq!(t.scale_x, 1.0);
    assert_eq!(t.scale_y, 1.0);
  }

  #[test]
  fn velocity_defaults() {
    let v = Velocity::default();
    assert_eq!(v.dx, 0.0);
    assert_eq!(v.dy, 0.0);
  }
}
