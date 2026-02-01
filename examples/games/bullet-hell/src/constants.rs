//! Game constants and configuration
//!
//! Note: Screen dimensions are obtained from the engine at runtime via
//! `ctx.screen_width()` and `ctx.screen_height()`. Configure resolution
//! in Ryo.toml [system] section or via EngineBuilder::resolution().

// Player configuration
pub const PLAYER_SIZE: f32 = 24.0;
pub const PLAYER_SPEED: f32 = 600.0; // 2x speed
pub const PLAYER_JUMP_FORCE: f32 = 550.0; // Higher jump (~154px height)
pub const PLAYER_MAX_LIVES: u32 = 3;
pub const INVINCIBLE_DURATION: f32 = 1.5; // seconds of invincibility after hit

// Physics
pub const GRAVITY: f32 = 980.0; // pixels per second squared

// Turret configuration
pub const TURRET_SIZE: f32 = 32.0;
pub const TURRET_MOVE_SPEED: f32 = 80.0;
pub const TURRET_MOVE_SPEED_MAX: f32 = 200.0;
pub const TURRET_SHOOT_INTERVAL: f32 = 1.5; // seconds between bursts
pub const TURRET_BURST_SIZE: u32 = 5; // bullets per burst (starting)
pub const TURRET_BURST_SIZE_MAX: u32 = 12; // bullets per burst (maximum)
pub const TURRET_BURST_DELAY: f32 = 0.1; // seconds between bullets in burst
pub const TURRETS_PER_SIDE: u32 = 2; // starting turrets per side
pub const TURRETS_PER_SIDE_MAX: u32 = 6; // maximum turrets per side

// Progression configuration
pub const PROGRESSION_INTERVAL: f32 = 15.0; // seconds between difficulty increases
pub const PROGRESSION_STEPS: u32 = 8; // number of difficulty steps to reach maximum

// Bullet configuration
pub const BULLET_SIZE: f32 = 8.0;
pub const BULLET_SPEED: f32 = 250.0;

// Particle configuration
pub const PARTICLE_SIZE: f32 = 4.0;
pub const PARTICLES_PER_EXPLOSION: u32 = 12;
pub const PARTICLE_SPEED_MIN: f32 = 100.0;
pub const PARTICLE_SPEED_MAX: f32 = 300.0;
pub const PARTICLE_LIFETIME: f32 = 1.0; // seconds

// Platform configuration
pub const PLATFORM_THICKNESS: f32 = 20.0;

// Z-ordering
pub const Z_BACKGROUND: i32 = 0;
pub const Z_PLATFORM: i32 = 5;
pub const Z_TURRET: i32 = 10;
pub const Z_BULLET: i32 = 15;
pub const Z_PLAYER: i32 = 20;
pub const Z_PARTICLE: i32 = 25;
pub const Z_UI: i32 = 100;
