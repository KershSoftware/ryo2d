//! Game Constants
//!
//! Centralized configuration values for the Breakout game.
//! Separating these makes tuning and balancing easier.

/// Screen dimensions
pub const SCREEN_WIDTH: u32 = 1280;
pub const SCREEN_HEIGHT: u32 = 720;

/// Paddle configuration
pub const PADDLE_WIDTH: f32 = 120.0;
pub const PADDLE_HEIGHT: f32 = 16.0;
pub const PADDLE_Y: f32 = 650.0;
pub const PADDLE_SPEED: f32 = 600.0;

/// Ball configuration
pub const BALL_SIZE: f32 = 12.0;
pub const BALL_SPEED: f32 = 400.0;

/// Block configuration
pub const BLOCK_WIDTH: f32 = 80.0;
pub const BLOCK_HEIGHT: f32 = 25.0;
pub const BLOCK_PADDING: f32 = 4.0;
pub const BLOCK_TOP_OFFSET: f32 = 60.0;
pub const BLOCK_LEFT_OFFSET: f32 = 120.0;

/// Wall configuration
pub const WALL_THICKNESS: f32 = 20.0;

/// Gameplay settings
pub const STARTING_LIVES: u32 = 3;
