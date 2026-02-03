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
pub const WALL_SLIDE_MAX_SPEED: f32 = 150.0; // Max fall speed when wall sliding (less sticky)
pub const WALL_SLIDE_GRAVITY_SCALE: f32 = 0.5; // Gravity multiplier during wall slide
pub const WALL_SLIDE_GRACE_DURATION: f32 = 0.12; // Seconds to still wall jump after releasing wall
pub const WALL_JUMP_FORCE_X: f32 = 450.0; // Horizontal push away from wall
pub const WALL_JUMP_FORCE_Y: f32 = 500.0; // Vertical jump force (slightly less than normal)
pub const WALL_JUMP_PUSH_DURATION: f32 = 0.2; // Seconds where player can't override horizontal push

// Turret configuration
pub const TURRET_SIZE: f32 = 32.0;
pub const TURRET_MOVE_SPEED: f32 = 80.0;
pub const TURRET_MOVE_SPEED_MAX: f32 = 200.0;
pub const TURRET_SHOOT_INTERVAL: f32 = 1.5; // seconds between bursts
pub const TURRET_BURST_SIZE: u32 = 5; // bullets per burst (starting)
pub const TURRET_BURST_SIZE_MAX: u32 = 12; // bullets per burst (maximum)
pub const TURRET_BURST_DELAY: f32 = 0.1; // seconds between bullets in burst
pub const TURRETS_PER_SIDE: u32 = 1; // starting turrets per side (2 total)
pub const TURRETS_PER_SIDE_MAX: u32 = 6; // maximum turrets per side (12 total)
pub const TURRET_SPAWN_DURATION: f32 = 1.5; // seconds to fade in before becoming active

// Progression configuration
pub const PROGRESSION_INTERVAL: f32 = 10.0; // seconds between difficulty increases
pub const PROGRESSION_STEPS: u32 = 8; // number of difficulty steps to reach maximum

// Bullet configuration
pub const BULLET_SIZE: f32 = 6.0;
pub const BULLET_SPEED: f32 = 250.0;

// Particle configuration
pub const PARTICLE_SIZE: f32 = 4.0;
pub const PARTICLES_PER_EXPLOSION: u32 = 12;
pub const PARTICLE_SPEED_MIN: f32 = 100.0;
pub const PARTICLE_SPEED_MAX: f32 = 300.0;
pub const PARTICLE_LIFETIME: f32 = 1.0; // seconds

// Platform configuration
pub const PLATFORM_THICKNESS: f32 = 20.0;

// Barrier configuration (vertical moving obstacles)
pub const BARRIER_WIDTH: f32 = 20.0;
pub const BARRIER_HEIGHT: f32 = 200.0;
pub const BARRIER_SPEED: f32 = 100.0; // moderate speed

// Drone configuration
pub const DRONE_SIZE: f32 = 28.0;
pub const DRONE_SPEED: f32 = 200.0;
pub const DRONE_AIM_DURATION: f32 = 0.8; // Time spent aiming before firing
pub const DRONE_FIRE_DURATION: f32 = 0.5; // How long the laser stays active
pub const DRONE_COOLDOWN: f32 = 1.5; // Cooldown after firing before moving again
pub const DRONE_LASER_WIDTH: f32 = 6.0;
pub const DRONE_FIRST_SPAWN_TIME: f32 = 30.0; // First drone spawns at 30 seconds
pub const DRONE_SPAWN_INTERVAL: f32 = 20.0; // Seconds between additional drone spawns
pub const DRONE_MAX_COUNT: u32 = 3; // Maximum number of drones

// Z-ordering
pub const Z_BACKGROUND: i32 = 0;
pub const Z_PLATFORM: i32 = 5;
pub const Z_BARRIER: i32 = 6;
pub const Z_TURRET: i32 = 10;
pub const Z_DRONE: i32 = 12;
pub const Z_LASER: i32 = 14;
pub const Z_BULLET: i32 = 15;
pub const Z_PLAYER: i32 = 20;
pub const Z_PARTICLE: i32 = 25;
pub const Z_UI: i32 = 100;
