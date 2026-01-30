//! Game constants and configuration values

// Screen dimensions
pub const SCREEN_WIDTH: u32 = 400;
pub const SCREEN_HEIGHT: u32 = 600;

// Bird physics
pub const BIRD_GRAVITY: f32 = 800.0;
pub const BIRD_FLAP_VELOCITY: f32 = -300.0;
pub const BIRD_MAX_FALL_SPEED: f32 = 400.0;
pub const BIRD_SIZE: f32 = 34.0;
pub const BIRD_START_X: f32 = 100.0;
pub const BIRD_START_Y: f32 = 300.0;

// Pipe configuration
pub const PIPE_WIDTH: f32 = 52.0;
pub const PIPE_GAP: f32 = 200.0; // Gap between top and bottom pipes (increased for easier gameplay)
pub const PIPE_SPAWN_DISTANCE: f32 = 250.0;
pub const PIPE_SCROLL_SPEED: f32 = 120.0;
pub const PIPE_MIN_HEIGHT: f32 = 100.0;
pub const PIPE_MAX_HEIGHT: f32 = 400.0;

// Ground
pub const GROUND_HEIGHT: f32 = 112.0;
pub const GROUND_SCROLL_SPEED: f32 = 120.0;

// Background
pub const BACKGROUND_SCROLL_SPEED: f32 = 30.0;

// Game bounds
pub const CEILING_Y: f32 = 0.0;
pub const GROUND_Y: f32 = SCREEN_HEIGHT as f32 - GROUND_HEIGHT;

// Scoring
pub const SCORE_X: f32 = SCREEN_WIDTH as f32 / 2.0;
pub const SCORE_Y: f32 = 50.0;
