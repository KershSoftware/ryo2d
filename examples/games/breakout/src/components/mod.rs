//! Game Components
//!
//! ECS components specific to the Breakout game.
//! These are marker and data components for game entities.

use ryo2d::prelude::*;

mod ball;
mod block;
mod paddle;

pub use ball::Ball;
pub use block::{Block, BlockColor};
pub use paddle::Paddle;

/// Marker for walls
#[derive(Component, Debug, Clone)]
pub struct Wall;

/// Marker for the death zone (bottom of screen)
#[derive(Component, Debug, Clone)]
pub struct DeathZone;
