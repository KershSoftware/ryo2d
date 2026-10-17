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
/// Paddle centre y (its top edge sits at 650)
pub const PADDLE_Y: f32 = 658.0;
pub const PADDLE_SPEED: f32 = 900.0;
/// Paddle acceleration (pixels per second squared)
pub const PADDLE_ACCELERATION: f32 = 6300.0;
/// Paddle deceleration when no input (friction)
pub const PADDLE_DECELERATION: f32 = 8000.0;
/// How much paddle velocity affects ball launch angle (0.0 to 1.0)
pub const PADDLE_LAUNCH_INFLUENCE: f32 = 0.5;

/// Ball configuration
pub const BALL_SIZE: f32 = 12.0;
/// The ball's speed in pixels per second. The ball's speeds are 2.4 times
/// what they were before the physics overhaul, which stepped Rapier 1/60 s
/// at 144 updates a second: the ball really moved this fast then.
pub const BALL_SPEED: f32 = 960.0;

/// Block configuration
pub const BLOCK_WIDTH: f32 = 80.0;
pub const BLOCK_HEIGHT: f32 = 25.0;
pub const BLOCK_PADDING: f32 = 4.0;
/// Centre y of the first block row (its top edge sits at 60)
pub const BLOCK_TOP_OFFSET: f32 = 72.5;
/// Centre x of the first block column (its left edge sits at 120)
pub const BLOCK_LEFT_OFFSET: f32 = 160.0;

/// Wall configuration
pub const WALL_THICKNESS: f32 = 20.0;

/// Gameplay settings
pub const STARTING_LIVES: u32 = 3;
pub const MAX_LEVEL: u32 = 10;

/// Ball physics
pub const BALL_MIN_VERTICAL_SPEED: f32 = 120.0; // Prevents horizontal-only motion
pub const BALL_MAX_HORIZONTAL_RATIO: f32 = 0.7; // Max horizontal as ratio of total speed
pub const PADDLE_SPIN_EFFECT: f32 = 480.0; // Horizontal velocity added based on hit position

/// Multi-ball spawning
pub const MULTIBALL_COOLDOWN: f32 = 5.0; // Seconds between multiball block spawns
pub const MULTIBALL_SURVIVAL_MAX: f32 = 30.0; // Seconds of survival for max spawn chance
pub const MULTIBALL_BASE_CHANCE: f32 = 0.15; // Base spawn chance at max survival time

/// Animation timings
pub const MULTIBALL_GLOW_SPEED: f32 = 8.0; // Pulse speed for multiball blocks
pub const HEART_FADE_SPEED: f32 = 2.0; // Fade cycle speed for heart blocks
pub const PLUS_ONE_DURATION: f32 = 1.5; // Duration of "+1" life animation

/// Input
pub const GAMEPAD_STICK_DEADZONE: f32 = 0.2;
