//! Pipe component - obstacles that scroll across the screen

use ryo2d::prelude::*;

/// Marks an entity as a pipe obstacle
#[derive(Component, Debug, Clone, Copy)]
pub struct Pipe {
  pub is_top: bool,
  pub has_scored: bool,
}

impl Pipe {
  pub fn new_top() -> Self {
    Self {
      is_top: true,
      has_scored: false,
    }
  }

  pub fn new_bottom() -> Self {
    Self {
      is_top: false,
      has_scored: false,
    }
  }
}
