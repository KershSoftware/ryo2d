//! Game constants

// Player configuration
pub const PLAYER_WIDTH: f32 = 24.0;
pub const PLAYER_HEIGHT: f32 = 32.0;

// Leap physics
pub const GRAVITY: f32 = 1200.0; // px/s²
pub const MIN_LEAP_FORCE: f32 = 300.0;
pub const MAX_LEAP_FORCE: f32 = 900.0;
pub const MAX_CHARGE_TIME: f32 = 1.5; // seconds
pub const OVERCHARGE_THRESHOLD: f32 = 1.2; // seconds
pub const OVERCHARGE_PENALTY: f32 = 0.7; // 70% force on overcharge
pub const LANDING_RECOVERY_TIME: f32 = 0.15; // seconds

// Aim configuration - SLINGSHOT STYLE: pull opposite direction to aim
// Pull down-left to leap up-right, pull down-right to leap up-left
pub const MIN_AIM_ANGLE: f32 = 45.0_f32; // degrees from horizontal (higher = more vertical)
pub const MAX_AIM_ANGLE: f32 = 85.0_f32; // degrees from horizontal
pub const DEFAULT_AIM_ANGLE: f32 = 70.0_f32; // degrees - favors high arc
pub const AIM_SPEED: f32 = 120.0; // degrees per second
pub const MOUSE_DRAG_DEADZONE: f32 = 10.0; // Minimum drag distance (pixels) before direction registers

// Platform configuration - tighter gaps for reliable jumping
pub const CHUNK_HEIGHT: f32 = 400.0;
pub const MIN_PLATFORMS_PER_CHUNK: u32 = 3;
pub const MAX_PLATFORMS_PER_CHUNK: u32 = 5;
pub const MIN_PLATFORM_WIDTH: f32 = 40.0; // Tiny platforms possible
pub const MAX_PLATFORM_WIDTH: f32 = 200.0;
pub const PLATFORM_HEIGHT: f32 = 16.0;
pub const MIN_VERTICAL_GAP: f32 = 60.0;
pub const MAX_VERTICAL_GAP: f32 = 120.0;
pub const MAX_HORIZONTAL_OFFSET: f32 = 250.0; // Max X distance between consecutive platforms
pub const MAX_JUMPABLE_GAP: f32 = 200.0; // Max edge-to-edge gap that's still jumpable
pub const PLATFORM_SPACING: f32 = 10.0; // Minimum gap between platforms on same Y level
pub const SPAWN_PLATFORM_WIDTH: f32 = 300.0;
pub const MOVING_PLATFORM_INTERVAL_MIN: u32 = 10;
pub const MOVING_PLATFORM_INTERVAL_MAX: u32 = 15;
pub const MOVING_PLATFORM_SPEED: f32 = 1.2; // radians/sec, sinusoidal
pub const SPAWN_PLATFORM_Y: f32 = 500.0;

// Spawn positions (on spawn platform)
pub const PLAYER_SPAWN_Y: f32 = SPAWN_PLATFORM_Y - PLAYER_HEIGHT;
/// Horizontal spawn offsets from spawn platform left edge for each player slot
pub const PLAYER_SPAWN_OFFSETS: [f32; 4] = [
  PLAYER_WIDTH / 2.0,                                       // Slot 0: left
  SPAWN_PLATFORM_WIDTH - PLAYER_WIDTH - PLAYER_WIDTH / 2.0, // Slot 1: right
  SPAWN_PLATFORM_WIDTH * 0.33 - PLAYER_WIDTH / 2.0,         // Slot 2: left-center
  SPAWN_PLATFORM_WIDTH * 0.67 - PLAYER_WIDTH / 2.0,         // Slot 3: right-center
];

// Death and respawn
pub const DEATH_Y_OFFSET: f32 = 400.0; // Fall this far below checkpoint to die
pub const RESPAWN_DELAY: f32 = 0.8; // Seconds of falling sensation before respawn

// Height tracking and checkpoints
pub const HEIGHT_SCALE: f32 = 10.0; // Pixels per height unit (lower = faster height gain)
pub const CHECKPOINT_INTERVAL: f32 = 150.0; // Height units between first checkpoints
pub const CHECKPOINT_INTERVAL_GROWTH: f32 = 25.0; // Each checkpoint adds this to the interval
pub const FIRST_CHECKPOINT_HEIGHT: f32 = 50.0; // Minimum height before first checkpoint
pub const CHECKPOINT_PLATFORM_COLOR: (u8, u8, u8) = (80, 120, 80); // Green like spawn platform
pub const FUTURE_CHECKPOINT_COLOR: (u8, u8, u8) = (200, 180, 50); // Yellow for next checkpoint

// Camera
pub const CAMERA_DEAD_ZONE: f32 = 100.0;
pub const CAMERA_LERP_SPEED: f32 = 0.1;
// How far above the player the camera centers (puts player near bottom of screen)
pub const CAMERA_LOOK_AHEAD: f32 = 200.0;

// Networking
pub const MAX_PLAYERS: usize = 4;
pub const DEFAULT_PORT: u16 = 7778;
pub const PROTOCOL_ID: u64 = 0x4C454150_47414D45; // "LEAPGAME"
pub const ROOM_CODE_LENGTH: usize = 5;
pub const DEFAULT_ADDRESS: &str = "127.0.0.1:7778";
pub const POSITION_BROADCAST_RATE: f32 = 1.0 / 20.0; // 20 Hz
pub const DEFAULT_SIGNAL_PORT: u16 = 3536;
pub const DEFAULT_SIGNAL_URL: &str = "ws://127.0.0.1:3536";

// Colors (RGB)
pub const PLAYER_COLORS: [(u8, u8, u8); 4] = [
  (66, 200, 135),  // Slot 0: Teal
  (245, 130, 66),  // Slot 1: Orange
  (160, 100, 220), // Slot 2: Purple
  (230, 200, 60),  // Slot 3: Yellow
];
pub const PLATFORM_COLOR: (u8, u8, u8) = (100, 100, 120); // Gray-blue
pub const MOVING_PLATFORM_COLOR: (u8, u8, u8) = (100, 110, 140); // Blue-ish tint
pub const SPAWN_PLATFORM_COLOR: (u8, u8, u8) = (80, 120, 80); // Green-gray
pub const BACKGROUND_COLOR: (u8, u8, u8) = (0, 0, 0); // Pure black
pub const CHARGE_INDICATOR_COLOR: (u8, u8, u8) = (140, 140, 130); // Muted grey
pub const OVERCHARGE_COLOR: (u8, u8, u8) = (160, 100, 100); // Muted reddish grey

// Z-ordering
pub const Z_BACKGROUND: i32 = 0;
pub const Z_PLATFORM: i32 = 5;
pub const Z_PLAYER: i32 = 10;
pub const Z_CHARGE_INDICATOR: i32 = 15;
pub const Z_UI: i32 = 100;

// Charge squish effect
pub const CHARGE_SQUISH_X: f32 = 1.5; // Wider when fully charged
pub const CHARGE_SQUISH_Y: f32 = 0.55; // Shorter when fully charged
pub const CHARGE_LEAN_OFFSET: f32 = 10.0; // Horizontal lean away from aim direction
pub const CHARGE_PULSE_SPEED: f32 = 12.0; // Speed of squish pulse oscillation
pub const CHARGE_PULSE_AMOUNT: f32 = 0.08; // How much the squish pulses
pub const CHARGE_SHAKE_MAX: f32 = 3.0; // Maximum shake offset at full charge
pub const CHARGE_SHAKE_SPEED: f32 = 45.0; // Shake oscillation speed

// Lighting. Light and ambient colours are sRGB like sprite colours; these were
// tuned when they were read as linear, so each is that value encoded to sRGB
pub const PLAYER_LIGHT_RADIUS: f32 = 180.0;
pub const PLAYER_LIGHT_INTENSITY: f32 = 0.7;
pub const PLAYER_LIGHT_COLOR: (u8, u8, u8) = (255, 248, 239); // Warm white
pub const LAMP_LIGHT_RADIUS: f32 = 120.0; // Outer glow
pub const LAMP_LIGHT_INTENSITY: f32 = 0.6;
pub const LAMP_INNER_RADIUS: f32 = 50.0; // Inner bright core
pub const LAMP_INNER_INTENSITY: f32 = 1.0;
pub const LAMP_LIGHT_COLOR: (u8, u8, u8) = (255, 229, 168); // Orange-yellow
pub const LAMP_INNER_COLOR: (u8, u8, u8) = (255, 248, 229); // Brighter warm white
pub const LAMP_OFFSET_Y: f32 = -30.0; // Above platform
pub const LAMP_OFFSET_X: f32 = 20.0; // Offset from edge
pub const LAMP_OSCILLATE_RADIUS: f32 = 8.0; // How much lamps move (larger = more movement)
pub const LAMP_OSCILLATE_SPEED: f32 = 3.0; // Oscillation speed (radians/sec)

// Enhanced lighting - atmosphere & ambient
pub const AMBIENT_DARK_COLOR: (u8, u8, u8) = (50, 50, 75); // Deep cave blue-black
pub const AMBIENT_DARK_INTENSITY: f32 = 0.03;
pub const AMBIENT_BRIGHT_COLOR: (u8, u8, u8) = (110, 122, 152); // Dawn sky hint
pub const AMBIENT_BRIGHT_INTENSITY: f32 = 0.12;
pub const HEIGHT_FOR_MAX_AMBIENT: f32 = 300.0; // Height units for full transition

// Shadow settings
pub const LAMP_SHADOW_SOFTNESS: f32 = 0.4;
pub const PLAYER_BLOB_SHADOW_WIDTH: f32 = 20.0;
pub const PLAYER_BLOB_SHADOW_HEIGHT: f32 = 6.0;

// Platform occluder
pub const PLATFORM_OCCLUDER_HEIGHT: f32 = 10.0;

// Walls either side of the tower, just off screen
pub const WALL_BOUNCE_FACTOR: f32 = 0.4; // Reflect 40% of horizontal velocity on wall hit
pub const WALL_THICKNESS: f32 = 20.0;
pub const WALL_HEIGHT: f32 = 200_000.0; // From below the spawn up past any climb
pub const WALL_BOTTOM: f32 = SPAWN_PLATFORM_Y + 2.0 * DEATH_Y_OFFSET;

// Collision layers (bits): players collide with the world, never each other
pub const WORLD_LAYER: u32 = 1;
pub const PLAYER_LAYER: u32 = 2;

// Checkpoints: a trigger standing on each checkpoint platform
pub const CHECKPOINT_ZONE_HEIGHT: f32 = PLAYER_HEIGHT * 1.5;
/// Steps a player's floor and wall checks wait after the game moves it (a
/// respawn, a server correction, a spawn): its `CharacterState` describes
/// the old place until the controller has moved it once from the new one
pub const SETTLE_STEPS: u8 = 3;

// Player collision
pub const PLAYER_PUSH_FORCE: f32 = 80.0; // Subtle horizontal push when overlapping
pub const PLAYER_PUSH_DECAY: f32 = 8.0; // Fast decay for subtle effect
pub const HEAD_BOUNCE_VELOCITY: f32 = -350.0; // Upward velocity when bouncing off head
pub const HEAD_BOUNCE_ZONE: f32 = 8.0; // Pixels from top of player that count as "head"
pub const AIR_KNOCKBACK_HORIZONTAL: f32 = 200.0; // Horizontal push when two airborne players collide
pub const AIR_KNOCKBACK_VERTICAL: f32 = -80.0; // Small upward bump on air collision
