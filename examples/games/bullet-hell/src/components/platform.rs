//! Platform component for static level geometry

use ryo2d::prelude::*;

#[derive(Component, Debug, Clone, Copy)]
pub struct Platform;

#[derive(Component, Debug, Clone, Copy)]
pub struct Wall;

/// Moving vertical barrier that blocks player and bullets
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
