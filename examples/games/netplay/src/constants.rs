//! Game constants

// Screen dimensions
pub const SCREEN_WIDTH: u32 = 800;
pub const SCREEN_HEIGHT: u32 = 600;

// Player configuration
pub const PLAYER_WIDTH: f32 = 32.0;
pub const PLAYER_HEIGHT: f32 = 48.0;
pub const PLAYER_SPEED: f32 = 200.0;
pub const PLAYER_JUMP_FORCE: f32 = 400.0;

// Physics
pub const GRAVITY: f32 = 800.0;

// Platform configuration
pub const PLATFORM_WIDTH: f32 = 600.0;
pub const PLATFORM_HEIGHT: f32 = 32.0;
pub const PLATFORM_Y: f32 = 500.0;

// Spawn positions
pub const PLAYER_A_SPAWN_X: f32 = 150.0;
pub const PLAYER_B_SPAWN_X: f32 = 650.0;
pub const PLAYER_SPAWN_Y: f32 = 400.0;

// Networking
pub const DEFAULT_PORT: u16 = 7777;
pub const PROTOCOL_ID: u64 = 0x4E455450_4C415921; // "NETPLAY!"
pub const ROOM_CODE_LENGTH: usize = 5;

// Colors
pub const PLAYER_A_COLOR: (u8, u8, u8) = (66, 135, 245); // Blue
pub const PLAYER_B_COLOR: (u8, u8, u8) = (245, 66, 66); // Red
pub const PLATFORM_COLOR: (u8, u8, u8) = (80, 80, 80); // Dark gray
pub const BACKGROUND_COLOR: (u8, u8, u8) = (30, 30, 40); // Dark blue-gray

// Z-ordering
pub const Z_BACKGROUND: i32 = 0;
pub const Z_PLATFORM: i32 = 5;
pub const Z_PLAYER: i32 = 10;
pub const Z_UI: i32 = 100;
