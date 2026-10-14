//! Pipe component - obstacles that scroll across the screen

use ryo2d::prelude::*;

/// Marks an entity as a pipe obstacle
#[derive(Component, Debug, Clone, Copy)]
pub struct Pipe {
  pub is_top: bool,
}

impl Pipe {
  pub fn new_top() -> Self {
    Self { is_top: true }
  }

  pub fn new_bottom() -> Self {
    Self { is_top: false }
  }
}

/// The trigger in a pipe pair's gap: the bird entering it scores a point
#[derive(Component, Debug, Clone, Copy)]
pub struct ScoreGate;
