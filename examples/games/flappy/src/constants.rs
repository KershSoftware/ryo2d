//! Game constants and configuration values

// Screen dimensions
pub const SCREEN_WIDTH: u32 = 400;
pub const SCREEN_HEIGHT: u32 = 600;

// Bird physics
pub const BIRD_GRAVITY: f32 = 800.0;
pub const BIRD_FLAP_VELOCITY: f32 = -300.0;
pub const BIRD_MAX_FALL_SPEED: f32 = 400.0;
pub const BIRD_SIZE: f32 = 64.0;
pub const BIRD_COLLIDER_RATIO: f32 = 0.35; // Collision circle is 35% of sprite size (forgiving for wings)
pub const BIRD_START_X: f32 = 100.0;
pub const BIRD_START_Y: f32 = 200.0;
pub const BIRD_TILT_UP_MAX: f32 = -25.0; // Max upward tilt in degrees when jumping
pub const BIRD_TILT_DOWN_MAX: f32 = 70.0; // Max downward tilt in degrees when falling

// Bird animation
pub const BIRD_FLAP_FRAME_TIME: f32 = 0.15; // 150ms per frame (~6.6 FPS flapping)
pub const BIRD_HURT_FRAME_TIME: f32 = 0.1; // 100ms per frame (10 FPS hurt animation)

// Pipe configuration
pub const PIPE_WIDTH: f32 = 52.0;
pub const PIPE_GAP: f32 = 225.0; // Gap between top and bottom pipes
pub const PIPE_SPAWN_DISTANCE: f32 = 250.0;
pub const PIPE_SCROLL_SPEED: f32 = 120.0;
pub const PIPE_MIN_HEIGHT: f32 = 100.0;
pub const PIPE_MAX_HEIGHT: f32 = 400.0;

// Ground
pub const GROUND_HEIGHT: f32 = 112.0;
pub const GROUND_SCROLL_SPEED: f32 = 120.0;
pub const GROUND_BORDER_COLOR: (u8, u8, u8, u8) = (80, 50, 30, 80); // Dark brown border line

// Background
pub const BACKGROUND_SCROLL_SPEED: f32 = 30.0;
pub const MENU_BACKGROUND_SCROLL_SPEED: f32 = 60.0; // Faster parallax for menu

// Game bounds
pub const CEILING_Y: f32 = 0.0;
pub const GROUND_Y: f32 = SCREEN_HEIGHT as f32 - GROUND_HEIGHT;

// Scoring
pub const SCORE_X: f32 = SCREEN_WIDTH as f32 / 2.0;
pub const SCORE_Y: f32 = 50.0;
