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
pub const PADDLE_SPEED: f32 = 900.0;
/// Paddle acceleration (pixels per second squared)
pub const PADDLE_ACCELERATION: f32 = 6300.0;
/// Paddle deceleration when no input (friction)
pub const PADDLE_DECELERATION: f32 = 8000.0;
/// How much paddle velocity affects ball launch angle (0.0 to 1.0)
pub const PADDLE_LAUNCH_INFLUENCE: f32 = 0.5;

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
