//! Platform component for static level geometry

use ryo2d::prelude::*;

/// A platform: the floating ones are one-way (landed on from above), the
/// ground is solid
#[derive(Component, Debug, Clone, Copy)]
pub struct Platform;

/// A screen edge: a collider just outside the screen the player stops at
/// and slides down
#[derive(Component, Debug, Clone, Copy)]
pub struct Wall;

/// Moving vertical barrier that blocks bullets; the player rides its top and
/// slides down its sides. A kinematic body moved through its `Transform`
#[derive(Component, Debug, Clone, Copy)]
pub struct Barrier {
  pub velocity_y: f32,
  pub min_y: f32,
  pub max_y: f32,
}

impl Barrier {
  pub fn new(velocity_y: f32, min_y: f32, max_y: f32) -> Self {
    Self {
      velocity_y,
      min_y,
      max_y,
    }
  }
}
