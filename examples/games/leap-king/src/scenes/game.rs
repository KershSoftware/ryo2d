//! Game scene with leap mechanics and multiplayer

use std::collections::HashMap;
use std::fs;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

static CONSOLE_RESPAWN_REQUESTED: AtomicBool = AtomicBool::new(false);

use ryo2d::core::network::NetworkError;
use ryo2d::prelude::*;

use crate::components::*;
use crate::constants::*;
use crate::generation::*;
use crate::network::*;

use super::MenuScene;

/// Connection notification type
#[derive(Debug, Clone)]
struct ConnectionNotification {
  message: String,
  timer: f32,
  is_connect: bool, // true = connect, false = disconnect
}

const NOTIFICATION_DURATION: f32 = 3.0;

/// Get the session cache file path for a room code
fn session_cache_path(room_code: &str) -> PathBuf {
  let mut path = dirs::data_local_dir().unwrap_or_else(|| PathBuf::from("."));
  path.push("leap");
  path.push(format!("session_{}.txt", room_code));
  path
}

/// Load a cached session ID for a room code
fn load_cached_session_id(room_code: &str) -> u64 {
  let path = session_cache_path(room_code);
  if let Ok(contents) = fs::read_to_string(&path) {
    contents.trim().parse().unwrap_or(0)
  } else {
    0
  }
}

/// Save a session ID to cache
fn save_session_id(room_code: &str, session_id: u64) {
  let path = session_cache_path(room_code);
  if let Some(parent) = path.parent() {
    let _ = fs::create_dir_all(parent);
  }
  let _ = fs::write(path, session_id.to_string());
}

/// Linear interpolation for u8 values
fn lerp_u8(a: u8, b: u8, t: f32) -> u8 {
  let a = a as f32;
  let b = b as f32;
  (a + (b - a) * t).clamp(0.0, 255.0) as u8
}

/// Maximum distance for spatial audio (beyond this, volume is at minimum)
const SPATIAL_AUDIO_MAX_DISTANCE: f32 = 600.0;
/// Minimum volume for distant sounds (0.0 = silent at max distance)
const SPATIAL_AUDIO_MIN_VOLUME: f32 = 0.2;
/// How much horizontal offset maps to full panning (pixels)
const SPATIAL_AUDIO_PAN_RANGE: f32 = 400.0;

/// Calculate spatial sound options based on sound source position relative to listener.
/// - Panning: Based on horizontal distance (left/right)
/// - Volume: Based on total distance (attenuates with distance)
fn spatial_sound_options(
  listener_x: f32,
  listener_y: f32,
  source_x: f32,
  source_y: f32,
) -> SoundOptions {
  let dx = source_x - listener_x;
  let dy = source_y - listener_y;
  let distance = (dx * dx + dy * dy).sqrt();

  // Panning: -1.0 (left) to 1.0 (right) based on horizontal offset
  let panning = (dx / SPATIAL_AUDIO_PAN_RANGE).clamp(-1.0, 1.0);

  // Volume: Attenuate with distance (linear falloff)
  let distance_factor = (1.0 - distance / SPATIAL_AUDIO_MAX_DISTANCE).clamp(0.0, 1.0);
  let volume = SPATIAL_AUDIO_MIN_VOLUME + (1.0 - SPATIAL_AUDIO_MIN_VOLUME) * distance_factor;

  SoundOptions::new().panning(panning).volume(volume)
}

/// Game mode
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum GameMode {
  SinglePlayer,
  Host,
  Client,
}

/// Cached charge indicator state for UI rendering
#[derive(Default)]
struct ChargeIndicatorState {
  visible: bool,
  // Screen position (converted from world via camera in update)
  screen_x: f32,
  screen_y: f32,
  charge_pct: f32,
  is_overcharging: bool,
  trajectory_vx: f32,
  trajectory_vy: f32,
}

/// Per-player checkpoint state
#[derive(Debug, Clone)]
struct PlayerCheckpoint {
  center_x: f32,
  platform_width: f32,
  y: f32,
  platform_entity: Option<Entity>,
  last_checkpoint_height: f32,
  last_synced_y: f32,
}

impl PlayerCheckpoint {
  fn new(screen_width: f32) -> Self {
    Self {
      center_x: screen_width / 2.0,
      platform_width: SPAWN_PLATFORM_WIDTH,
      y: PLAYER_SPAWN_Y,
      platform_entity: None,
      last_checkpoint_height: 0.0,
      last_synced_y: PLAYER_SPAWN_Y,
    }
  }

  /// Slot 0's spot on the checkpoint platform, where its `Checkpoint`
  /// trigger puts a player's `Respawn` point.
  fn spawn_point(&self) -> Vec2 {
    let left = self.center_x - self.platform_width / 2.0;
    Vec2::new(left + slot_spawn_offset(0, self.platform_width), self.y)
  }
}

/// How far from a platform's left edge a slot's player stands when spawned
/// on it: the spawn platform's spots, scaled to the platform's width.
fn slot_spawn_offset(slot: usize, platform_width: f32) -> f32 {
  let idx = slot.min(PLAYER_SPAWN_OFFSETS.len() - 1);
  PLAYER_SPAWN_OFFSETS[idx] * platform_width / SPAWN_PLATFORM_WIDTH
}

/// The checkpoint threshold a platform with its top at `platform_top`
/// stands at: the highest threshold at or below its height (checkpoint
/// platforms are generated exactly at the thresholds).
fn checkpoint_height(platform_top: f32) -> f32 {
  let height = (SPAWN_PLATFORM_Y - platform_top) / HEIGHT_SCALE;
  let mut threshold = FIRST_CHECKPOINT_HEIGHT;
  let mut interval = CHECKPOINT_INTERVAL;
  let mut reached = 0.0;
  while threshold <= height + 0.5 {
    reached = threshold;
    threshold += interval;
    interval += CHECKPOINT_INTERVAL_GROWTH;
  }
  reached
}

pub struct GameScene {
  mode: GameMode,
  room_code: String,

  // Networking
  server: Option<ListenServer>,
  client: Option<NetworkClient>,

  // Entity tracking
  players: HashMap<ClientId, Entity>,
  player_lights: HashMap<ClientId, Entity>,
  local_client_id: Option<ClientId>,
  platforms: Vec<Entity>,
  platform_lamps: Vec<Entity>,
  lamp_dust_emitters: Vec<Entity>,
  background_tiles: Vec<Entity>,
  bg_tile_height: f32,
  rain_emitter: Option<Entity>,
  rain_sprite: Option<SpriteHandle>,
  dust_sprite: Option<SpriteHandle>,

  // Particle system
  particle_system: ParticleSystem,

  // Sound effects loaded flag
  sounds_loaded: bool,

  // Track previous leap state for remote players (to detect jumps/lands)
  previous_leap_states: HashMap<ClientId, NetworkLeapState>,

  // Track remote players' best heights and slots for display
  remote_best_heights: HashMap<ClientId, f32>,
  remote_slots: HashMap<ClientId, u8>,

  // Platform generation
  seed: u64,
  generator: PlatformGenerator,
  highest_player_y: f32,

  // Camera
  camera_y: f32,
  camera_fast_lerp: bool, // Rapid lerp after respawn

  // Network timing
  broadcast_timer: f32,

  // Animation timing
  elapsed_time: f32,
  /// Last authoritative elapsed_time received from host (clients only)
  host_elapsed_time: f32,

  // Death/respawn
  respawn_timer: Option<f32>,

  // Per-player checkpoint system
  player_checkpoints: HashMap<ClientId, PlayerCheckpoint>,
  future_checkpoint_platform: Option<Entity>,
  current_height: f32,
  max_height: f32,

  // Connection state
  connected: bool,
  received_seed: bool,

  // Session persistence
  session_id: u64,
  client_sessions: HashMap<u64, SessionData>, // Server-side: session_id -> session data
  client_to_session: HashMap<ClientId, u64>,  // Server-side: client_id -> session_id

  // Connection notifications
  notifications: Vec<ConnectionNotification>,

  // UI state
  show_quit_dialog: bool,
  quit_dialog_selection: usize, // 0 = Yes, 1 = Cancel
  quit_input_cooldown: f32,
  pending_quit: bool,

  // Input delay to prevent menu button from triggering charge
  input_delay: f32,

  // Cached state for UI rendering
  charge_indicator: ChargeIndicatorState,

  // Client-side prediction helper
  prediction: PredictionHelper,

  // Screen dimensions (set from engine in on_enter)
  screen_width: f32,
  screen_height: f32,

  // Mouse drag state for click-and-drag slingshot control
  mouse_drag_start: Option<(f32, f32)>,

  // Ping measurement (client only)
  ping_ms: Option<u32>,
  ping_timer: f32,
  ping_send_time: Option<std::time::Instant>,

  // Slot assignment (server/host only)
  client_slots: HashMap<ClientId, u8>,

  /// Simulated players the game just spawned or moved, and how many more
  /// steps their floor checks wait (see `TELEPORT_SETTLE_STEPS`)
  settling: HashMap<Entity, u8>,
}

impl GameScene {
  /// Create single player game
  pub fn new_single_player() -> Result<Self, NetworkError> {
    let seed: u64 = rand::random();
    Ok(Self {
      mode: GameMode::SinglePlayer,
      room_code: String::new(),
      server: None,
      client: None,
      players: HashMap::new(),
      player_lights: HashMap::new(),
      local_client_id: Some(ClientId::from(0u64)),
      platforms: Vec::new(),
      platform_lamps: Vec::new(),
      lamp_dust_emitters: Vec::new(),
      background_tiles: Vec::new(),
      bg_tile_height: 0.0,
      rain_emitter: None,
      rain_sprite: None,
      dust_sprite: None,
      particle_system: ParticleSystem::new(),
      sounds_loaded: false,
      previous_leap_states: HashMap::new(),
      remote_best_heights: HashMap::new(),
      remote_slots: HashMap::new(),
      seed,
      generator: PlatformGenerator::new(seed, 0.0),
      highest_player_y: PLAYER_SPAWN_Y,
      camera_y: PLAYER_SPAWN_Y - CAMERA_LOOK_AHEAD,
      camera_fast_lerp: false,
      broadcast_timer: 0.0,
      elapsed_time: 0.0,
      host_elapsed_time: 0.0,
      respawn_timer: None,
      player_checkpoints: HashMap::new(),
      future_checkpoint_platform: None,
      current_height: 0.0,
      max_height: 0.0,
      connected: true,
      received_seed: true,
      session_id: 0,
      client_sessions: HashMap::new(),
      client_to_session: HashMap::new(),
      notifications: Vec::new(),
      show_quit_dialog: false,
      quit_dialog_selection: 1, // Default to Cancel
      quit_input_cooldown: 0.0,
      pending_quit: false,
      input_delay: 0.2, // Prevent menu button from triggering charge
      charge_indicator: ChargeIndicatorState::default(),
      prediction: PredictionHelper::new(Some(ClientId::from(0u64))),
      screen_width: 0.0,
      screen_height: 0.0,
      mouse_drag_start: None,
      ping_ms: None,
      ping_timer: 0.0,
      ping_send_time: None,
      client_slots: HashMap::new(),
      settling: HashMap::new(),
    })
  }

  /// Create host (server) game
  pub fn new_host(room_code: String) -> Result<Self, NetworkError> {
    let server = ListenServer::new(NetworkServerConfig {
      port: DEFAULT_PORT,
      max_clients: MAX_PLAYERS - 1,
      protocol_id: PROTOCOL_ID,
      transport: TransportConfig::Udp,
    })?;

    let host_id = server.host_id();
    let seed: u64 = rand::random();

    Ok(Self {
      mode: GameMode::Host,
      room_code,
      server: Some(server),
      client: None,
      players: HashMap::new(),
      player_lights: HashMap::new(),
      local_client_id: Some(host_id),
      platforms: Vec::new(),
      platform_lamps: Vec::new(),
      lamp_dust_emitters: Vec::new(),
      background_tiles: Vec::new(),
      bg_tile_height: 0.0,
      rain_emitter: None,
      rain_sprite: None,
      dust_sprite: None,
      particle_system: ParticleSystem::new(),
      sounds_loaded: false,
      previous_leap_states: HashMap::new(),
      remote_best_heights: HashMap::new(),
      remote_slots: HashMap::new(),
      seed,
      generator: PlatformGenerator::new(seed, 0.0),
      highest_player_y: PLAYER_SPAWN_Y,
      camera_y: PLAYER_SPAWN_Y - CAMERA_LOOK_AHEAD,
      camera_fast_lerp: false,
      broadcast_timer: 0.0,
      elapsed_time: 0.0,
      host_elapsed_time: 0.0,
      respawn_timer: None,
      player_checkpoints: HashMap::new(),
      future_checkpoint_platform: None,
      current_height: 0.0,
      max_height: 0.0,
      connected: true,
      received_seed: true,
      session_id: 0,
      client_sessions: HashMap::new(),
      client_to_session: HashMap::new(),
      notifications: Vec::new(),
      show_quit_dialog: false,
      quit_dialog_selection: 1, // Default to Cancel
      quit_input_cooldown: 0.0,
      pending_quit: false,
      input_delay: 0.2, // Prevent menu button from triggering charge
      charge_indicator: ChargeIndicatorState::default(),
      prediction: PredictionHelper::new(Some(host_id)),
      screen_width: 0.0,
      screen_height: 0.0,
      mouse_drag_start: None,
      ping_ms: None,
      ping_timer: 0.0,
      ping_send_time: None,
      client_slots: HashMap::new(),
      settling: HashMap::new(),
    })
  }

  /// Create host (server) game over WebRTC
  pub fn new_host_webrtc(room_code: String, signal_url: &str) -> Result<Self, NetworkError> {
    let server = ListenServer::new(NetworkServerConfig {
      max_clients: MAX_PLAYERS - 1,
      protocol_id: PROTOCOL_ID,
      transport: TransportConfig::WebRtc(WebRtcConfig::new(signal_url, &room_code)),
      ..Default::default()
    })?;

    let host_id = server.host_id();
    let seed: u64 = rand::random();

    Ok(Self {
      mode: GameMode::Host,
      room_code,
      server: Some(server),
      client: None,
      players: HashMap::new(),
      player_lights: HashMap::new(),
      local_client_id: Some(host_id),
      platforms: Vec::new(),
      platform_lamps: Vec::new(),
      lamp_dust_emitters: Vec::new(),
      background_tiles: Vec::new(),
      bg_tile_height: 0.0,
      rain_emitter: None,
      rain_sprite: None,
      dust_sprite: None,
      particle_system: ParticleSystem::new(),
      sounds_loaded: false,
      previous_leap_states: HashMap::new(),
      remote_best_heights: HashMap::new(),
      remote_slots: HashMap::new(),
      seed,
      generator: PlatformGenerator::new(seed, 0.0),
      highest_player_y: PLAYER_SPAWN_Y,
      camera_y: PLAYER_SPAWN_Y - CAMERA_LOOK_AHEAD,
      camera_fast_lerp: false,
      broadcast_timer: 0.0,
      elapsed_time: 0.0,
      host_elapsed_time: 0.0,
      respawn_timer: None,
      player_checkpoints: HashMap::new(),
      future_checkpoint_platform: None,
      current_height: 0.0,
      max_height: 0.0,
      connected: true,
      received_seed: true,
      session_id: 0,
      client_sessions: HashMap::new(),
      client_to_session: HashMap::new(),
      notifications: Vec::new(),
      show_quit_dialog: false,
      quit_dialog_selection: 1,
      quit_input_cooldown: 0.0,
      pending_quit: false,
      input_delay: 0.2,
      charge_indicator: ChargeIndicatorState::default(),
      prediction: PredictionHelper::new(Some(host_id)),
      screen_width: 0.0,
      screen_height: 0.0,
      mouse_drag_start: None,
      ping_ms: None,
      ping_timer: 0.0,
      ping_send_time: None,
      client_slots: HashMap::new(),
      settling: HashMap::new(),
    })
  }

  /// Create client game (auto-detects WebRTC vs UDP from address format)
  pub fn new_client(room_code: &str, address: &str) -> Result<Self, NetworkError> {
    let is_webrtc = address.starts_with("ws://") || address.starts_with("wss://");

    let client = if is_webrtc {
      NetworkClient::connect(NetworkClientConfig::webrtc(WebRtcConfig::new(
        address, room_code,
      )))?
    } else {
      let server_addr: SocketAddr = if address.contains(':') {
        address
          .parse()
          .map_err(|_| NetworkError::ConnectionError(format!("Invalid address: {}", address)))?
      } else {
        format!("{}:{}", address, DEFAULT_PORT)
          .parse()
          .map_err(|_| NetworkError::ConnectionError(format!("Invalid address: {}", address)))?
      };
      NetworkClient::connect(NetworkClientConfig {
        server_addr: Some(server_addr),
        protocol_id: PROTOCOL_ID,
        transport: TransportConfig::Udp,
      })?
    };

    Ok(Self {
      mode: GameMode::Client,
      room_code: room_code.to_string(),
      server: None,
      client: Some(client),
      players: HashMap::new(),
      player_lights: HashMap::new(),
      local_client_id: None,
      platforms: Vec::new(),
      platform_lamps: Vec::new(),
      lamp_dust_emitters: Vec::new(),
      background_tiles: Vec::new(),
      bg_tile_height: 0.0,
      rain_emitter: None,
      rain_sprite: None,
      dust_sprite: None,
      particle_system: ParticleSystem::new(),
      sounds_loaded: false,
      previous_leap_states: HashMap::new(),
      remote_best_heights: HashMap::new(),
      remote_slots: HashMap::new(),
      seed: 0, // Will receive from server
      generator: PlatformGenerator::new(0, 0.0),
      highest_player_y: PLAYER_SPAWN_Y,
      camera_y: PLAYER_SPAWN_Y - CAMERA_LOOK_AHEAD,
      camera_fast_lerp: false,
      broadcast_timer: 0.0,
      elapsed_time: 0.0,
      host_elapsed_time: 0.0,
      respawn_timer: None,
      player_checkpoints: HashMap::new(),
      future_checkpoint_platform: None,
      current_height: 0.0,
      max_height: 0.0,
      connected: false,
      received_seed: false,
      session_id: load_cached_session_id(room_code),
      client_sessions: HashMap::new(),
      client_to_session: HashMap::new(),
      notifications: Vec::new(),
      show_quit_dialog: false,
      quit_dialog_selection: 1, // Default to Cancel
      quit_input_cooldown: 0.0,
      pending_quit: false,
      input_delay: 0.2, // Prevent menu button from triggering charge
      charge_indicator: ChargeIndicatorState::default(),
      prediction: PredictionHelper::new(None), // Will set local_client_id when connected
      screen_width: 0.0,
      screen_height: 0.0,
      mouse_drag_start: None,
      ping_ms: None,
      ping_timer: 0.0,
      ping_send_time: None,
      client_slots: HashMap::new(),
      settling: HashMap::new(),
    })
  }

  /// Generate the tower from `seed` instead of a random one (a host sends
  /// its seed to clients; tests use a fixed one).
  pub fn with_seed(mut self, seed: u64) -> Self {
    self.seed = seed;
    self
  }

  /// Returns the lowest unused slot 0-3
  fn next_available_slot(&self) -> u8 {
    let used: std::collections::HashSet<u8> = self.client_slots.values().copied().collect();
    (0..MAX_PLAYERS as u8)
      .find(|s| !used.contains(s))
      .unwrap_or(0)
  }

  /// Compute spawn X position for a player slot, relative to the centered spawn platform
  fn spawn_x_for_slot(&self, slot: u8) -> f32 {
    let platform_left = self.screen_width / 2.0 - SPAWN_PLATFORM_WIDTH / 2.0;
    platform_left + slot_spawn_offset(slot as usize, SPAWN_PLATFORM_WIDTH)
  }

  #[allow(clippy::too_many_arguments)]
  fn spawn_platform_entity(
    &mut self,
    ctx: &mut SceneContext,
    x: f32,
    y: f32,
    width: f32,
    chunk_y: i32,
    is_spawn: bool,
    is_moving: bool,
    is_checkpoint: bool,
    platform_index: u32,
  ) -> Entity {
    let color = if is_spawn {
      SPAWN_PLATFORM_COLOR
    } else if is_moving {
      MOVING_PLATFORM_COLOR
    } else {
      PLATFORM_COLOR
    };

    let platform = if is_spawn {
      Platform::spawn_platform()
    } else if is_checkpoint {
      Platform::checkpoint(width, chunk_y)
    } else {
      Platform::new(width, chunk_y)
    };

    // Platforms are placed and collided by their top-left corner. They are
    // one-way: players leap up through them and land on top
    let entity = ctx
      .world
      .spawn((
        Transform::from_xy(x, y),
        Renderable::sprite(SpriteHandle::from_sprite(
          Sprite::solid_rect(
            width as u32,
            PLATFORM_HEIGHT as u32,
            color.0,
            color.1,
            color.2,
            255,
          ),
          format!("platform_{}_{}", chunk_y, self.platforms.len()),
        ))
        .with_z_order(Z_PLATFORM)
        .with_anchor(Anchor::TopLeft),
        Collider::rect(width, PLATFORM_HEIGHT)
          .with_offset(width / 2.0, PLATFORM_HEIGHT / 2.0)
          .one_way(),
        platform,
      ))
      .id();

    // A moving platform is a kinematic body: `update_moving_platforms` moves
    // its `Transform`, and the players standing on it ride along
    if is_moving {
      let phase_seed = self
        .seed
        .wrapping_add(chunk_y as u64 * 4919 + platform_index as u64);
      let phase = (phase_seed % 628) as f32 / 100.0;
      let moving = MovingPlatform::new(x, width, self.screen_width, MOVING_PLATFORM_SPEED, phase);
      ctx
        .world
        .entity_mut(entity)
        .insert((moving, RigidBody::kinematic()));
    }

    if is_checkpoint {
      self.spawn_checkpoint_zone(ctx, entity, x, width);
    }

    self.platforms.push(entity);

    // Spawn a lamp above the platform (not for spawn platform)
    if !is_spawn {
      self.spawn_platform_lamp(ctx, x, y, width, chunk_y, entity, platform_index);
    }

    entity
  }

  /// A `Checkpoint` trigger at a checkpoint platform's height, one and a
  /// half players tall and spanning the tower wall to wall: a player
  /// reaches the checkpoint by climbing past its height anywhere, not only
  /// on its platform. Its `Respawn` point moves to slot 0's spot on the
  /// platform (the spawn rule of `checkpoint_spawn_x`) and a
  /// `CheckpointReached` is written.
  fn spawn_checkpoint_zone(&self, ctx: &mut SceneContext, platform: Entity, left: f32, width: f32) {
    // The child sits at the platform's top-left corner
    ctx.spawn_child(
      platform,
      (
        Transform::IDENTITY,
        Collider::rect(self.screen_width, CHECKPOINT_ZONE_HEIGHT)
          .with_offset(
            self.screen_width / 2.0 - left,
            -CHECKPOINT_ZONE_HEIGHT / 2.0,
          )
          .as_trigger()
          .with_mask(PLAYER_LAYER),
        Checkpoint::new().with_offset(Vec2::new(slot_spawn_offset(0, width), -PLAYER_HEIGHT)),
      ),
    );
  }

  /// Walls just off either side of the screen: players bounce off them.
  fn spawn_walls(&self, ctx: &mut SceneContext) {
    let center_y = WALL_BOTTOM - WALL_HEIGHT / 2.0;
    for x in [
      -WALL_THICKNESS / 2.0,
      self.screen_width + WALL_THICKNESS / 2.0,
    ] {
      ctx.world.spawn((
        Transform::from_xy(x, center_y),
        Collider::rect(WALL_THICKNESS, WALL_HEIGHT),
      ));
    }
  }

  #[allow(clippy::too_many_arguments)]
  fn spawn_platform_lamp(
    &mut self,
    ctx: &mut SceneContext,
    platform_x: f32,
    platform_y: f32,
    platform_width: f32,
    chunk_y: i32,
    platform_entity: Entity,
    platform_index: u32,
  ) {
    // Use chunk-based seed for random values
    let chunk_seed = self
      .seed
      .wrapping_add(chunk_y as u64 * 7919 + platform_index as u64);

    // Random phase 0 to ~2π for unique movement patterns
    let phase = (chunk_seed % 628) as f32 / 100.0;

    // Random speed multiplier 0.6 to 1.4 for varied movement speeds
    let speed_seed = chunk_seed.wrapping_mul(31337);
    let speed_mult = 0.6 + ((speed_seed % 80) as f32 / 100.0);

    // Lamp starts at center of platform
    let lamp_x = platform_x + platform_width / 2.0;
    let lamp_y = platform_y + LAMP_OFFSET_Y;

    // Outer glow light (dimmer, larger) - with soft shadows
    let outer_lamp = ctx
      .world
      .spawn((
        Transform::from_xy(lamp_x, lamp_y),
        Light2D::point_with_intensity(
          Color::rgb(LAMP_LIGHT_COLOR.0, LAMP_LIGHT_COLOR.1, LAMP_LIGHT_COLOR.2),
          LAMP_LIGHT_RADIUS,
          LAMP_LIGHT_INTENSITY,
        )
        .with_soft_shadows(LAMP_SHADOW_SOFTNESS),
        PlatformLamp::new(lamp_x, lamp_y, phase, platform_width, speed_mult)
          .with_platform_entity(platform_entity),
      ))
      .id();
    self.platform_lamps.push(outer_lamp);

    // Inner bright core light (brighter, smaller)
    let inner_lamp = ctx
      .world
      .spawn((
        Transform::from_xy(lamp_x, lamp_y),
        Light2D::point_with_intensity(
          Color::rgb(LAMP_INNER_COLOR.0, LAMP_INNER_COLOR.1, LAMP_INNER_COLOR.2),
          LAMP_INNER_RADIUS,
          LAMP_INNER_INTENSITY,
        ),
        PlatformLamp::new(lamp_x, lamp_y, phase, platform_width, speed_mult)
          .with_platform_entity(platform_entity),
      ))
      .id();
    self.platform_lamps.push(inner_lamp);

    // Create dust sprite if not already created
    if self.dust_sprite.is_none() {
      self.dust_sprite = Some(ctx.assets.create_sprite(
        Sprite::solid_rect(2, 2, 255, 240, 200, 120), // Warm semi-transparent dust
        "lamp_dust",
      ));
    }

    // Dust mote particle emitter - gentle floating particles in the light
    if let Some(ref dust_sprite) = self.dust_sprite {
      let dust_emitter = ctx
        .world
        .spawn((
          Transform::from_xy(lamp_x, lamp_y),
          ParticleEmitter::new(dust_sprite.clone())
            .with_mode(EmissionMode::Continuous { rate: 3.0 }) // Sparse, gentle
            .with_shape(EmissionShape::Circle {
              radius: LAMP_LIGHT_RADIUS * 0.6,
              hollow: false,
            })
            .with_lifetime(3.0, 6.0)
            .with_size(1.0, 3.0)
            .with_end_size(0.5, 1.5)
            .with_speed(2.0, 8.0) // Very slow drift
            .with_color(
              Color::new(255, 240, 200, 80), // Warm, semi-transparent
              Color::new(255, 220, 180, 0),  // Fade to invisible
            )
            .with_gravity(0.0, -5.0) // Gentle upward drift
            .with_drag(2.0),
          PlatformLamp::new(lamp_x, lamp_y, phase, platform_width, speed_mult)
            .with_platform_entity(platform_entity),
        ))
        .id();
      self.lamp_dust_emitters.push(dust_emitter);
    }
  }

  fn update_lamps(&mut self, ctx: &mut SceneContext) {
    let lamp_entities: Vec<Entity> = self.platform_lamps.clone();

    for entity in lamp_entities {
      let lamp_data = ctx.world.get::<PlatformLamp>(entity).copied();

      if let Some(lamp) = lamp_data
        && let Some(mut transform) = ctx.world.get_mut::<Transform>(entity)
      {
        let t = self.elapsed_time;
        let p = lamp.phase;
        let speed = LAMP_OSCILLATE_SPEED * lamp.speed_mult;

        // Horizontal travel range - lamp travels most of platform width
        let travel_range = (lamp.platform_width - LAMP_OFFSET_X * 2.0) * 0.45;

        // Fairy-like floating motion using Lissajous curves with per-lamp speed
        // Primary smooth figure-8 motion
        let fairy_x = (t * speed * 0.7 + p).sin() * (t * speed * 0.3 + p * 0.5).cos();
        let fairy_y = (t * speed * 0.5 + p * 1.3).sin();

        // Secondary gentle drift overlay
        let drift_x =
          (t * speed * 0.15 + p * 2.1).sin() * 0.5 + (t * speed * 0.08 + p * 0.7).cos() * 0.3;
        let drift_y =
          (t * speed * 0.25 + p * 1.7).cos() * 0.3 + (t * speed * 0.12 + p * 2.3).sin() * 0.2;

        // Horizontal movement scaled to platform width
        let offset_x = travel_range * (fairy_x * 0.7 + drift_x * 0.3);
        let offset_y = LAMP_OSCILLATE_RADIUS * (fairy_y * 0.5 + drift_y);

        // Occasional subtle jitter (reduced frequency and intensity)
        let flicker_seed = ((t * 5.0 * lamp.speed_mult + p * 11.0).sin() * 1000.0).abs() % 1.0;
        let flicker = if flicker_seed > 0.95 {
          // Rare quick jitter
          let jx = ((t * 31.0 + p).sin() * 2.0).signum() * 1.5;
          let jy = ((t * 37.0 + p).cos() * 2.0).signum() * 1.0;
          (jx, jy)
        } else {
          (0.0, 0.0)
        };

        transform.translation.x = lamp.base_x + offset_x + flicker.0;
        transform.translation.y = lamp.base_y + offset_y + flicker.1;
      }
    }

    // Update dust emitter positions to follow their lamps
    for entity in &self.lamp_dust_emitters {
      let lamp_data = ctx.world.get::<PlatformLamp>(*entity).copied();

      if let Some(lamp) = lamp_data
        && let Some(mut transform) = ctx.world.get_mut::<Transform>(*entity)
      {
        let t = self.elapsed_time;
        let p = lamp.phase;
        let speed = LAMP_OSCILLATE_SPEED * lamp.speed_mult;

        // Horizontal travel range - same as lamps
        let travel_range = (lamp.platform_width - LAMP_OFFSET_X * 2.0) * 0.45;

        // Same fairy-like motion as lamps (without jitter)
        let fairy_x = (t * speed * 0.7 + p).sin() * (t * speed * 0.3 + p * 0.5).cos();
        let fairy_y = (t * speed * 0.5 + p * 1.3).sin();

        let drift_x =
          (t * speed * 0.15 + p * 2.1).sin() * 0.5 + (t * speed * 0.08 + p * 0.7).cos() * 0.3;
        let drift_y =
          (t * speed * 0.25 + p * 1.7).cos() * 0.3 + (t * speed * 0.12 + p * 2.3).sin() * 0.2;

        // Horizontal movement scaled to platform width
        let offset_x = travel_range * (fairy_x * 0.7 + drift_x * 0.3);
        let offset_y = LAMP_OSCILLATE_RADIUS * (fairy_y * 0.5 + drift_y);

        transform.translation.x = lamp.base_x + offset_x;
        transform.translation.y = lamp.base_y + offset_y;
      }
    }
  }

  fn spawn_starting_platform(&mut self, ctx: &mut SceneContext) {
    let x = (self.screen_width - SPAWN_PLATFORM_WIDTH) / 2.0;
    self.spawn_platform_entity(
      ctx,
      x,
      SPAWN_PLATFORM_Y,
      SPAWN_PLATFORM_WIDTH,
      0,
      true,
      false,
      false,
      0,
    );
  }

  fn spawn_rain(&mut self, ctx: &mut SceneContext) {
    // Create particle sprite
    let rain_sprite = ctx.assets.create_sprite(
      Sprite::solid_circle(8, 255, 255, 255, 255), // White circle
      "rain_particle",
    );
    self.rain_sprite = Some(rain_sprite.clone());

    // Use the built-in rain preset with wider coverage
    // Add glow effect so rain is visible even with dark lighting
    let emitter = ParticleEmitter::rain(rain_sprite)
      .with_mode(EmissionMode::Continuous { rate: 300.0 })
      .with_lifetime(2.5, 3.0) // Long enough to fall the full screen height
      .with_shape(EmissionShape::Line {
        start_x: -self.screen_width,
        start_y: -50.0, // Above emitter
        end_x: self.screen_width,
        end_y: -50.0,
      })
      .with_z_order(Z_PLATFORM - 1) // Above background, below platforms
      .with_effect(SpriteEffect::glow_with_intensity(
        Color::new(180, 200, 255, 255), // Blue-white glow
        3.0,
        0.8,
      ));

    // Position emitter at top of visible camera area
    let rain_entity = ctx
      .world
      .spawn((
        Transform::from_xy(
          self.screen_width / 2.0,
          self.camera_y - self.screen_height / 2.0,
        ),
        emitter,
      ))
      .id();

    self.rain_emitter = Some(rain_entity);
  }

  fn update_rain(&mut self, ctx: &mut SceneContext) {
    // Keep rain emitter above the camera
    if let Some(rain_entity) = self.rain_emitter
      && let Some(mut transform) = ctx.world.get_mut::<Transform>(rain_entity)
    {
      transform.translation.x = self.screen_width / 2.0;
      transform.translation.y = self.camera_y - self.screen_height / 2.0; // Top of visible area
    }
  }

  fn update_background(&mut self, ctx: &mut SceneContext) {
    if self.bg_tile_height <= 0.0 || self.background_tiles.is_empty() {
      return;
    }

    // Find the camera's visible range
    let visible_top = self.camera_y - self.screen_height;
    let visible_bottom = self.camera_y + self.screen_height;

    for &entity in &self.background_tiles {
      if let Some(mut transform) = ctx.world.get_mut::<Transform>(entity) {
        // If tile is below visible area, move it up
        if transform.translation.y > visible_bottom {
          // Move up by total height of all tiles
          transform.translation.y -= self.bg_tile_height * self.background_tiles.len() as f32;
        }
        // If tile is above visible area, move it down
        else if transform.translation.y + self.bg_tile_height < visible_top {
          transform.translation.y += self.bg_tile_height * self.background_tiles.len() as f32;
        }
      }
    }
  }

  /// Update ambient lighting based on player height - gets brighter as you climb
  fn update_ambient_by_height(&self, ctx: &mut SceneContext) {
    // Calculate height factor (0.0 at bottom, 1.0 at max height)
    let height_factor = (self.current_height / HEIGHT_FOR_MAX_AMBIENT).clamp(0.0, 1.0);

    // Lerp between dark cave and brighter sky
    let r = lerp_u8(AMBIENT_DARK_COLOR.0, AMBIENT_BRIGHT_COLOR.0, height_factor);
    let g = lerp_u8(AMBIENT_DARK_COLOR.1, AMBIENT_BRIGHT_COLOR.1, height_factor);
    let b = lerp_u8(AMBIENT_DARK_COLOR.2, AMBIENT_BRIGHT_COLOR.2, height_factor);
    let intensity =
      AMBIENT_DARK_INTENSITY + (AMBIENT_BRIGHT_INTENSITY - AMBIENT_DARK_INTENSITY) * height_factor;

    ctx
      .lighting()
      .set_ambient(AmbientLight::new(Color::rgb(r, g, b), intensity));
  }

  fn update_player_lights(&mut self, ctx: &mut SceneContext) {
    // Update each player light to follow its player
    for (&client_id, &light_entity) in &self.player_lights {
      let Some(&player_entity) = self.players.get(&client_id) else {
        continue;
      };

      let Some(player_transform) = ctx.world.get::<Transform>(player_entity) else {
        continue;
      };

      // Center the light on the player
      let light_x = player_transform.translation.x + PLAYER_WIDTH / 2.0;
      let light_y = player_transform.translation.y + PLAYER_HEIGHT / 2.0;

      if let Some(mut light_transform) = ctx.world.get_mut::<Transform>(light_entity) {
        light_transform.translation.x = light_x;
        light_transform.translation.y = light_y;
      }
    }
  }

  fn update_player_squish(&mut self, ctx: &mut SceneContext) {
    // Update player visual squish based on charge state
    let time = self.elapsed_time;

    for &entity in self.players.values() {
      let Some(player) = ctx.world.get::<Player>(entity) else {
        continue;
      };

      let (scale_x, scale_y, offset_x) = if player.leap_state.is_charging() {
        // Calculate squish based on charge time (0 to 1)
        let charge_t = (player.leap_state.charge_time() / MAX_CHARGE_TIME).min(1.0);
        let is_overcharging = player.leap_state.charge_time() > OVERCHARGE_THRESHOLD;

        // Ease in the squish effect with smooth cubic
        let eased_t = charge_t * charge_t * (3.0 - 2.0 * charge_t); // Smoothstep

        // Add pulsing effect that increases with charge
        let pulse_intensity = eased_t * CHARGE_PULSE_AMOUNT;
        let pulse = (time * CHARGE_PULSE_SPEED).sin() * pulse_intensity;

        // Interpolate scale: start at 1.0, go to squish values, add pulse
        let base_sx = 1.0 + (CHARGE_SQUISH_X - 1.0) * eased_t;
        let base_sy = 1.0 + (CHARGE_SQUISH_Y - 1.0) * eased_t;

        // Pulse affects both axes inversely (wider = shorter, maintaining volume feel)
        let sx = base_sx + pulse;
        let sy = base_sy - pulse * 0.5;

        // Shake effect that builds up as charge increases
        // Starts subtle, gets more intense near full charge
        let shake_intensity = if is_overcharging {
          // Intense shake when overcharging
          CHARGE_SHAKE_MAX * 1.5
        } else {
          // Gradual buildup: exponential curve so it's subtle at start, strong at end
          let shake_t = (charge_t * charge_t * charge_t).min(1.0);
          CHARGE_SHAKE_MAX * shake_t
        };

        // High frequency shake using multiple sine waves for organic feel
        let shake_x = shake_intensity
          * ((time * CHARGE_SHAKE_SPEED).sin() * 0.6
            + (time * CHARGE_SHAKE_SPEED * 1.7).cos() * 0.4);
        let shake_y = shake_intensity
          * 0.3
          * ((time * CHARGE_SHAKE_SPEED * 1.3).cos() * 0.7
            + (time * CHARGE_SHAKE_SPEED * 2.1).sin() * 0.3);

        // Lean away from aim direction + shake
        let lean = CHARGE_LEAN_OFFSET * eased_t * (-player.facing.sign()) + shake_x;

        (sx, sy + shake_y * 0.01, lean)
      } else {
        // Reset to normal
        (1.0, 1.0, 0.0)
      };

      // The sprite is a child anchored at the player's bottom-left corner, so
      // squishing keeps the feet grounded; the lean only moves the sprite
      // (it doesn't affect collision).
      let Some(sprite) = ctx
        .world
        .get::<Children>(entity)
        .and_then(|children| children.first().copied())
      else {
        continue;
      };
      if let Some(mut transform) = ctx.world.get_mut::<Transform>(sprite) {
        transform.scale = Vec2::new(scale_x, scale_y);
        transform.translation.x = offset_x;
      }
    }
  }

  fn spawn_player(&mut self, ctx: &mut SceneContext, slot: u8, client_id: ClientId) -> Entity {
    let spawn_x = self.spawn_x_for_slot(slot);
    let idx = (slot as usize).min(PLAYER_COLORS.len() - 1);
    let color = PLAYER_COLORS[idx];

    // Determine if this is the local player (already spawned means we can predict)
    let is_local = self.local_client_id == Some(client_id);
    let predicted = if is_local {
      // Local player starts as spawned for immediate prediction
      Predicted::new_spawned(client_id)
    } else {
      // Remote players start unspawned, will be marked spawned on first server update
      Predicted::new(client_id)
    };

    // The player's position is its top-left corner; the sprite is a child
    // standing on the bottom-left corner so it can squish in place. Players
    // are on their own layer and collide only with the world, so they pass
    // through each other and `check_player_collisions` decides what happens.
    let entity = ctx
      .world
      .spawn((
        Transform::from_xy(spawn_x, PLAYER_SPAWN_Y),
        Collider::rect(PLAYER_WIDTH, PLAYER_HEIGHT)
          .with_offset(PLAYER_WIDTH / 2.0, PLAYER_HEIGHT / 2.0)
          .with_layer(PLAYER_LAYER)
          .with_mask(WORLD_LAYER),
        Player::new(slot),
        Velocity::zero(),
        predicted,
      ))
      .id();
    // Whoever simulates a player gives it a controller: single player and
    // the host simulate everyone, a client only its own player. A client's
    // remote players stay bare colliders moved by the host's updates.
    if self.mode != GameMode::Client || is_local {
      self.make_controlled(ctx, entity);
    }
    // Checkpoints are decided where everyone is simulated; a client's
    // respawn point comes from the host's checkpoint
    if self.mode != GameMode::Client {
      ctx.world.entity_mut(entity).insert(Respawn::here());
    }
    ctx.spawn_child(
      entity,
      (
        Transform::from_xy(0.0, PLAYER_HEIGHT),
        Renderable::sprite(SpriteHandle::from_sprite(
          Sprite::solid_rect(
            PLAYER_WIDTH as u32,
            PLAYER_HEIGHT as u32,
            color.0,
            color.1,
            color.2,
            255,
          ),
          format!("player_{}", slot),
        ))
        .with_z_order(Z_PLAYER)
        .with_anchor(Anchor::BottomLeft),
      ),
    );

    self.players.insert(client_id, entity);

    // Spawn a light for this player - with subtle volumetric glow
    let light_entity = ctx
      .world
      .spawn((
        Transform::from_xy(
          spawn_x + PLAYER_WIDTH / 2.0,
          PLAYER_SPAWN_Y + PLAYER_HEIGHT / 2.0,
        ),
        Light2D::point_with_intensity(
          Color::rgb(
            PLAYER_LIGHT_COLOR.0,
            PLAYER_LIGHT_COLOR.1,
            PLAYER_LIGHT_COLOR.2,
          ),
          PLAYER_LIGHT_RADIUS,
          PLAYER_LIGHT_INTENSITY,
        )
        .with_volumetric_settings(VolumetricSettings::subtle_beam()),
        PlayerLight {
          player_entity: entity,
        },
      ))
      .id();
    self.player_lights.insert(client_id, light_entity);

    entity
  }

  fn get_local_player_entity(&self) -> Option<Entity> {
    self
      .local_client_id
      .and_then(|id| self.players.get(&id).copied())
  }

  /// The players this side simulates: everyone in single player and on the
  /// host, only its own player on a client.
  fn simulated_players(&self) -> Vec<Entity> {
    if self.mode == GameMode::Client {
      self.get_local_player_entity().into_iter().collect()
    } else {
      self.players.values().copied().collect()
    }
  }

  /// Give a player the engine's character controller: the physics step
  /// moves it by its `Velocity` and reports what it touched.
  fn make_controlled(&mut self, ctx: &mut SceneContext, entity: Entity) {
    ctx
      .world
      .entity_mut(entity)
      .insert(CharacterController::platformer());
    self.settle(entity, SPAWN_SETTLE_STEPS);
  }

  /// The game just put this player somewhere: wait `steps` before trusting
  /// its `CharacterState` (see `TELEPORT_SETTLE_STEPS`).
  fn settle(&mut self, entity: Entity, steps: u8) {
    if steps > 0 {
      self.settling.insert(entity, steps);
    }
  }

  /// Count one step of a player's wait; true while it is still waiting.
  fn take_settle_step(&mut self, entity: Entity) -> bool {
    let Some(steps) = self.settling.get_mut(&entity) else {
      return false;
    };
    *steps -= 1;
    if *steps == 0 {
      self.settling.remove(&entity);
    }
    true
  }

  /// Put a simulated player at `to`. For its controller a `Transform` write
  /// is a teleport: it moves on from there in this step's physics, but this
  /// step's floor checks would read where it was, so they wait.
  fn teleport(&mut self, ctx: &mut SceneContext, entity: Entity, to: Vec2) {
    if let Some(mut transform) = ctx.world.get_mut::<Transform>(entity) {
      transform.translation = to;
    }
    self.settle(entity, TELEPORT_SETTLE_STEPS);
  }

  /// Get a clone of the local player's checkpoint (or a default if not yet set)
  fn local_checkpoint(&self) -> PlayerCheckpoint {
    self
      .local_client_id
      .and_then(|id| self.player_checkpoints.get(&id))
      .cloned()
      .unwrap_or_else(|| PlayerCheckpoint::new(self.screen_width))
  }

  /// Get or initialize a player's checkpoint entry
  fn get_or_init_checkpoint(&mut self, client_id: ClientId) -> &mut PlayerCheckpoint {
    let screen_width = self.screen_width;
    self
      .player_checkpoints
      .entry(client_id)
      .or_insert_with(|| PlayerCheckpoint::new(screen_width))
  }

  fn check_death_and_respawn(&mut self, ctx: &mut SceneContext, dt: f32) {
    // Check if respawn timer is active
    if let Some(timer) = self.respawn_timer.as_mut() {
      *timer -= dt;
      if *timer <= 0.0 {
        self.respawn_timer = None;
        self.execute_respawn(ctx);
      }
      return;
    }

    // Check if local player has fallen to death
    let Some(entity) = self.get_local_player_entity() else {
      return;
    };

    let Some(transform) = ctx.world.get::<Transform>(entity) else {
      return;
    };

    // Death threshold is relative to local player's checkpoint position
    let death_y = self.local_checkpoint().y + DEATH_Y_OFFSET;

    // Start respawn timer if player fell below death threshold
    if transform.translation.y > death_y {
      self.respawn_timer = Some(RESPAWN_DELAY);
    }
  }

  /// Update height tracking and check for new checkpoints
  fn update_height_and_checkpoints(&mut self, ctx: &mut SceneContext) {
    // Update local player's height display
    if let Some(entity) = self.get_local_player_entity() {
      let (player_y, is_grounded) = {
        let Some(transform) = ctx.world.get::<Transform>(entity) else {
          return;
        };
        let Some(player) = ctx.world.get::<Player>(entity) else {
          return;
        };
        (transform.translation.y, player.grounded)
      };

      let raw_height = (PLAYER_SPAWN_Y - player_y) / HEIGHT_SCALE;
      self.current_height = raw_height.max(0.0);

      if is_grounded && self.current_height > self.max_height {
        self.max_height = self.current_height;
      }
    }
  }

  /// Take in the checkpoints players reached last step. `update` calls this
  /// before anything else (the quit dialog, R, the death check): a zone
  /// fallen through moves a `Respawn` point down for a step, and this is
  /// what puts it back. Respawns use `PlayerCheckpoint`, which only goes up.
  fn read_checkpoints(&mut self, ctx: &mut SceneContext) {
    // The client relies on the host for checkpoints (its players have no
    // `Respawn`, so its zones report nothing)
    let reached = ctx.messages::<CheckpointReached>();
    if self.mode == GameMode::Client {
      return;
    }

    // Host/SinglePlayer: a player entering a checkpoint's zone
    for reached in reached {
      let Some(client_id) = self
        .players
        .iter()
        .find_map(|(&id, &entity)| (entity == reached.entity).then_some(id))
      else {
        continue;
      };
      let Some(platform) = ctx
        .world
        .get::<ChildOf>(reached.checkpoint)
        .map(|child_of| child_of.parent())
      else {
        continue;
      };
      let (Some(transform), Some(&Platform { width, .. })) = (
        ctx.world.get::<Transform>(platform),
        ctx.world.get::<Platform>(platform),
      ) else {
        continue;
      };
      let (left, top) = (transform.translation.x, transform.translation.y);
      let height = checkpoint_height(top);

      // Checkpoints only go up: a lower one (fallen onto on the way to the
      // death line) puts the respawn point back
      let current = self
        .player_checkpoints
        .get(&client_id)
        .cloned()
        .unwrap_or_else(|| PlayerCheckpoint::new(self.screen_width));
      if height <= current.last_checkpoint_height {
        if let Some(mut respawn) = ctx.world.get_mut::<Respawn>(reached.entity) {
          respawn.point = current.spawn_point();
        }
        continue;
      }

      // Only mark visuals (green color) for the local player
      if Some(client_id) == self.local_client_id {
        Self::mark_checkpoint_platform(ctx, current.platform_entity, platform);
      }
      let cp = self.get_or_init_checkpoint(client_id);
      cp.center_x = left + width / 2.0;
      cp.platform_width = width;
      cp.y = top - PLAYER_HEIGHT;
      cp.last_checkpoint_height = height;
      cp.platform_entity = Some(platform);
    }
  }

  /// Colour a platform's sprite (`kind` names the sprite).
  fn paint_platform(ctx: &mut SceneContext, entity: Entity, color: (u8, u8, u8), kind: &str) {
    let Some(width) = ctx.world.get::<Platform>(entity).map(|p| p.width) else {
      return;
    };
    if let Some(mut renderable) = ctx.world.get_mut::<Renderable>(entity) {
      renderable.sprite = SpriteHandle::from_sprite(
        Sprite::solid_rect(
          width as u32,
          PLATFORM_HEIGHT as u32,
          color.0,
          color.1,
          color.2,
          255,
        ),
        format!("platform_{kind}_{}", entity.to_bits()),
      );
    }
  }

  /// Turn the local player's checkpoint platform green, and its old one
  /// back to grey (checkpoint platforms never move; the spawn platform
  /// keeps its colour).
  fn mark_checkpoint_platform(ctx: &mut SceneContext, old: Option<Entity>, new: Entity) {
    if let Some(old) = old
      && old != new
      && ctx.world.get::<Platform>(old).is_some_and(|p| !p.is_spawn)
    {
      Self::paint_platform(ctx, old, PLATFORM_COLOR, "normal");
    }
    Self::paint_platform(ctx, new, CHECKPOINT_PLATFORM_COLOR, "checkpoint");
  }

  /// The checkpoint platform with its top nearest `top`, within a platform
  /// gap (the client finds the platform of the checkpoint the host sent).
  fn checkpoint_platform_at(&self, ctx: &SceneContext, top: f32) -> Option<Entity> {
    self
      .platforms
      .iter()
      .filter(|&&entity| {
        ctx
          .world
          .get::<Platform>(entity)
          .is_some_and(|p| p.is_checkpoint)
      })
      .filter_map(|&entity| {
        let distance = (ctx.world.get::<Transform>(entity)?.translation.y - top).abs();
        (distance <= MIN_VERTICAL_GAP).then_some((entity, distance))
      })
      .min_by(|a, b| a.1.total_cmp(&b.1))
      .map(|(entity, _)| entity)
  }

  /// Sync checkpoint visual on the client side.
  /// When the host sends a new checkpoint_y for the local player, find the matching platform and color it green.
  fn sync_checkpoint_visual(&mut self, ctx: &mut SceneContext) {
    let cp = self.local_checkpoint();
    if (cp.y - cp.last_synced_y).abs() < 0.1 {
      return;
    }

    let platform = self.checkpoint_platform_at(ctx, cp.y + PLAYER_HEIGHT);
    if let Some(platform) = platform {
      Self::mark_checkpoint_platform(ctx, cp.platform_entity, platform);
    }
    if let Some(local_id) = self.local_client_id {
      let cp = self.get_or_init_checkpoint(local_id);
      if platform.is_some() {
        cp.platform_entity = platform;
      }
      cp.last_synced_y = cp.y;
    }
  }

  /// Compute the next checkpoint threshold above the given height.
  fn next_checkpoint_threshold(last_height: f32) -> f32 {
    let mut threshold = FIRST_CHECKPOINT_HEIGHT;
    let mut interval = CHECKPOINT_INTERVAL;
    while threshold <= last_height {
      threshold += interval;
      interval += CHECKPOINT_INTERVAL_GROWTH;
    }
    threshold
  }

  /// Update the yellow "future checkpoint" platform marker for the local player.
  /// Finds the platform closest to (but at or above) the next checkpoint threshold.
  fn update_future_checkpoint_visual(&mut self, ctx: &mut SceneContext) {
    let cp = self.local_checkpoint();
    let next_threshold = Self::next_checkpoint_threshold(cp.last_checkpoint_height);

    // Convert threshold to world Y: standing on a platform at this height means
    // player_y = platform_y - PLAYER_HEIGHT, and height = (PLAYER_SPAWN_Y - player_y) / HEIGHT_SCALE
    // So platform_y = PLAYER_SPAWN_Y + PLAYER_HEIGHT - threshold * HEIGHT_SCALE
    let cutoff_y = PLAYER_SPAWN_Y + PLAYER_HEIGHT - next_threshold * HEIGHT_SCALE;

    // Find the is_checkpoint platform closest to the cutoff Y
    let mut best: Option<(Entity, f32)> = None;
    for &platform_entity in &self.platforms {
      // Skip the current checkpoint platform (it's already green)
      if cp.platform_entity == Some(platform_entity) {
        continue;
      }
      let Some(transform) = ctx.world.get::<Transform>(platform_entity) else {
        continue;
      };
      let Some(platform) = ctx.world.get::<Platform>(platform_entity) else {
        continue;
      };
      if !platform.is_checkpoint {
        continue;
      }
      // Pick the platform closest to the cutoff Y (which is the threshold position)
      let dist = (transform.translation.y - cutoff_y).abs();
      let is_closer = best.as_ref().is_none_or(|b| dist < (b.1 - cutoff_y).abs());
      if is_closer {
        best = Some((platform_entity, transform.translation.y));
      }
    }

    let new_entity = best.map(|(e, _)| e);

    // Nothing changed
    if new_entity == self.future_checkpoint_platform {
      return;
    }

    // Revert old yellow checkpoint platform to its normal color
    // Skip if it's now the current checkpoint (already colored green)
    if let Some(old_entity) = self.future_checkpoint_platform
      && cp.platform_entity != Some(old_entity)
      && ctx
        .world
        .get::<Platform>(old_entity)
        .is_some_and(|p| p.is_checkpoint)
    {
      Self::paint_platform(ctx, old_entity, PLATFORM_COLOR, "normal");
    }

    // Color new future checkpoint platform yellow
    if let Some(new_entity) = new_entity {
      Self::paint_platform(ctx, new_entity, FUTURE_CHECKPOINT_COLOR, "future");
    }

    self.future_checkpoint_platform = new_entity;
  }

  /// Compute per-slot spawn X on a checkpoint platform.
  /// Slot 0 spawns on the far left, slot 1 on the far right.
  /// If the other player overlaps the intended spawn, swap to the opposite side.
  fn checkpoint_spawn_x(&self, ctx: &SceneContext, slot: u8, cp: &PlayerCheckpoint) -> f32 {
    let platform_left = cp.center_x - cp.platform_width / 2.0;

    // Compute 4 candidate positions scaled proportionally to the checkpoint platform width
    let positions: [f32; 4] =
      std::array::from_fn(|i| platform_left + slot_spawn_offset(i, cp.platform_width));

    let idx = (slot as usize).min(positions.len() - 1);
    let preferred_x = positions[idx];

    // In single player, no anti-overlap needed
    if self.mode == GameMode::SinglePlayer {
      return preferred_x;
    }

    // Helper: check if a position overlaps any other player
    let overlaps = |x: f32| -> bool {
      for (&_cid, &other_entity) in &self.players {
        let other_slot = ctx.world.get::<Player>(other_entity).map(|p| p.slot);
        if other_slot == Some(slot) {
          continue;
        }
        if let Some(other_transform) = ctx.world.get::<Transform>(other_entity)
          && x < other_transform.translation.x + PLAYER_WIDTH
          && x + PLAYER_WIDTH > other_transform.translation.x
        {
          return true;
        }
      }
      false
    };

    // Try preferred position first
    if !overlaps(preferred_x) {
      return preferred_x;
    }

    // Try all 4 positions left-to-right to find the first unoccupied spot
    for &pos in &positions {
      if !overlaps(pos) {
        return pos;
      }
    }

    preferred_x
  }

  /// Where a player comes back: on its checkpoint's platform, at slot 0's
  /// spot in single player (`PlayerCheckpoint::spawn_point`, where the
  /// checkpoint's trigger also puts the `Respawn` point), and at its slot's
  /// own spot, or the first free one, in multiplayer. `PlayerCheckpoint`
  /// only goes up, while the `Respawn` point can sit on a lower checkpoint
  /// for the step after its zone is fallen through.
  fn respawn_point(&self, ctx: &SceneContext, slot: u8, cp: &PlayerCheckpoint) -> Vec2 {
    Vec2::new(self.checkpoint_spawn_x(ctx, slot, cp), cp.y)
  }

  /// Execute respawn using NetworkAction pattern
  /// - For clients: applies locally AND sends to server (dual execution)
  /// - For host/single player: applies locally only
  fn execute_respawn(&mut self, ctx: &mut SceneContext) {
    let Some(entity) = self.get_local_player_entity() else {
      return;
    };

    let slot = ctx.world.get::<Player>(entity).map(|p| p.slot).unwrap_or(0);

    let cp = self.local_checkpoint();
    let point = self.respawn_point(ctx, slot, &cp);

    // Apply respawn locally: reset position, velocity, and state.
    // Do NOT use RespawnAction (which calls force_correction) — the prediction
    // system preserves local positions for the local entity, so the respawn sticks
    // until the server catches up.
    self.teleport(ctx, entity, point);
    if let Some(mut vel) = ctx.world.get_mut::<Velocity>(entity) {
      vel.dx = 0.0;
      vel.dy = 0.0;
    }
    if let Some(mut player) = ctx.world.get_mut::<Player>(entity) {
      player.grounded = true;
      player.leap_state = LeapState::Idle;
    }

    // Notify server so it can reset the player on its end
    if self.mode == GameMode::Client
      && let Some(client) = self.client.as_mut()
    {
      let rpc = RespawnRpc;
      if let Ok(serialized) = SerializedRpc::from_rpc(&rpc, client.client_id(), RpcTarget::Server)
        && let Ok(bytes) = serialized.to_bytes()
      {
        client.send(Channel::ReliableOrdered, &bytes);
      }
    }

    // Trigger fast camera lerp to checkpoint position (instead of instant snap)
    self.camera_fast_lerp = true;
  }

  /// Respawn a player entity (used by server when receiving respawn requests)
  fn respawn_player(&mut self, ctx: &mut SceneContext, entity: Entity, client_id: ClientId) {
    let slot = ctx.world.get::<Player>(entity).map(|p| p.slot).unwrap_or(0);

    let cp = self
      .player_checkpoints
      .get(&client_id)
      .cloned()
      .unwrap_or_else(|| PlayerCheckpoint::new(self.screen_width));
    let point = self.respawn_point(ctx, slot, &cp);

    let action = RespawnAction {
      slot,
      spawn_x: point.x,
      spawn_y: point.y,
    };

    let mut action_ctx = ActionContext::new(ctx.world);
    action.apply(&mut action_ctx);
    // The action wrote the player's `Transform`: a teleport
    self.settle(entity, TELEPORT_SETTLE_STEPS);
  }

  fn handle_input(&mut self, ctx: &mut SceneContext, dt: f32) -> LeapInput {
    // Default input for early returns - using builder ensures required fields are set
    let default_input = || {
      LeapInput::builder()
        .charging(false)
        .facing_right(true)
        .build()
    };

    // Decrement input delay (prevents menu button from triggering charge)
    if self.input_delay > 0.0 {
      self.input_delay -= dt;
    }

    let entity = match self.get_local_player_entity() {
      Some(e) => e,
      None => return default_input(),
    };

    // Get current player state
    let player = match ctx.world.get::<Player>(entity) {
      Some(p) => *p,
      None => return default_input(),
    };

    // Start with current facing direction
    let mut facing_right = player.facing == Facing::Right;

    // --- Mouse input (click-and-drag slingshot) ---
    if !self.show_quit_dialog {
      // Clear drag if player can't charge (airborne, landing, etc.)
      if self.mouse_drag_start.is_some()
        && !player.leap_state.can_leap()
        && !player.leap_state.is_charging()
      {
        self.mouse_drag_start = None;
      }

      // Start drag on click when player is idle
      if ctx.input.mouse_pressed(MouseButton::Left)
        && player.leap_state.can_leap()
        && self.input_delay <= 0.0
      {
        self.mouse_drag_start = Some((ctx.input.mouse_x(), ctx.input.mouse_y()));
      }

      if let Some((start_x, start_y)) = self.mouse_drag_start {
        let mx = ctx.input.mouse_x();
        let my = ctx.input.mouse_y();
        let drag_dx = mx - start_x;
        let drag_dy = my - start_y;
        let drag_dist = (drag_dx * drag_dx + drag_dy * drag_dy).sqrt();

        if !ctx.input.mouse_down(MouseButton::Left) {
          // Released mouse — fire the leap
          self.mouse_drag_start = None;

          let release_facing = if drag_dist > MOUSE_DRAG_DEADZONE && drag_dx.abs() > 5.0 {
            drag_dx < 0.0 // Slingshot: drag left = face right
          } else {
            facing_right
          };

          return LeapInput::builder()
            .charging(false)
            .facing_right(release_facing)
            .release(player.leap_state.is_charging())
            .aim_angle(player.leap_state.aim_angle())
            .charge_time(player.leap_state.charge_time())
            .build();
        }

        // Still holding — continue charging with mouse aim
        let desired_angle = if drag_dist > MOUSE_DRAG_DEADZONE {
          // Slingshot: pulling down = higher arc, horizontal = flatter
          let effective_dy = drag_dy.max(0.0);
          effective_dy
            .atan2(drag_dx.abs().max(0.001))
            .to_degrees()
            .clamp(MIN_AIM_ANGLE, MAX_AIM_ANGLE)
        } else {
          DEFAULT_AIM_ANGLE
        };

        let current_angle = if player.leap_state.is_charging() {
          player.leap_state.aim_angle()
        } else {
          DEFAULT_AIM_ANGLE
        };

        let mouse_facing = if drag_dist > MOUSE_DRAG_DEADZONE && drag_dx.abs() > 5.0 {
          drag_dx < 0.0 // Slingshot: drag left = face right
        } else {
          facing_right
        };

        return LeapInput::builder()
          .charging(true)
          .facing_right(mouse_facing)
          .aim_angle(if player.leap_state.is_charging() {
            current_angle
          } else {
            desired_angle
          })
          .aim_delta(desired_angle - current_angle)
          .build();
      }
    }

    // Check for charge button (Space or gamepad South/A button)
    // Ignore if input_delay is active (prevents menu selection from triggering charge)
    let charge_button_down = self.input_delay <= 0.0
      && (ctx.input.key_down(KeyCode::Space)
        || ctx.input.gamepad_button_down(GamepadButton::South));

    // Check directional inputs (keyboard + DPad only - analog stick handled separately)
    let key_aim_up = ctx.input.key_down(KeyCode::ArrowUp)
      || ctx.input.key_down(KeyCode::KeyW)
      || ctx.input.gamepad_button_down(GamepadButton::DPadUp);
    let key_aim_down = ctx.input.key_down(KeyCode::ArrowDown)
      || ctx.input.key_down(KeyCode::KeyS)
      || ctx.input.gamepad_button_down(GamepadButton::DPadDown);
    let key_aim_left = ctx.input.key_down(KeyCode::ArrowLeft)
      || ctx.input.key_down(KeyCode::KeyA)
      || ctx.input.gamepad_button_down(GamepadButton::DPadLeft);
    let key_aim_right = ctx.input.key_down(KeyCode::ArrowRight)
      || ctx.input.key_down(KeyCode::KeyD)
      || ctx.input.gamepad_button_down(GamepadButton::DPadRight);

    // Analog stick - use vector magnitude for smooth diagonal input
    let stick_x = ctx.input.left_stick_x();
    let stick_y = ctx.input.left_stick_y();
    let stick_magnitude = (stick_x * stick_x + stick_y * stick_y).sqrt();
    let stick_active = stick_magnitude > 0.1; // Low deadzone for responsive diagonal input

    // Collect input values
    let mut charging = false;
    let mut aim_angle = DEFAULT_AIM_ANGLE;
    let mut release = false;
    let mut charge_time = 0.0;
    let mut aim_delta = 0.0;

    // Handle charging state
    if player.leap_state.can_leap() && charge_button_down {
      // Start or continue charging
      charging = true;
      aim_angle = DEFAULT_AIM_ANGLE;
    } else if player.leap_state.is_charging() {
      if charge_button_down {
        // Continue charging
        charging = true;
        charge_time = player.leap_state.charge_time();
        aim_angle = player.leap_state.aim_angle();

        // SLINGSHOT AIMING - pull opposite direction to aim
        // Analog stick takes priority for smooth diagonal control
        if stick_active {
          // Normalize X for direction detection
          let norm_x = stick_x / stick_magnitude;

          // Horizontal: pull left = launch right, pull right = launch left
          // Very low threshold - even slight horizontal bias sets direction
          // This makes bottom-left/bottom-right corners much more forgiving
          if norm_x.abs() > 0.05 {
            facing_right = norm_x < 0.0; // Pull left (negative) = face right
          }

          // Vertical: map stick Y to arc angle (slingshot inverted)
          // Pull down (negative Y) = higher arc
          // Use raw stick_y for more direct control, scaled by how much Y input there is
          // This makes diagonals feel more responsive
          let y_factor = stick_y.abs().max(0.3); // Ensure some response even at corners
          let vertical_influence = -stick_y * y_factor;
          aim_delta = vertical_influence * AIM_SPEED * dt * 2.0;
        } else {
          // Keyboard/DPad fallback (discrete input)
          // Vertical aim (slingshot: down = higher arc)
          if key_aim_down {
            aim_delta = AIM_SPEED * dt;
          }
          if key_aim_up {
            aim_delta = -AIM_SPEED * dt;
          }

          // Horizontal facing (slingshot: left = face right)
          if key_aim_left {
            facing_right = true;
          }
          if key_aim_right {
            facing_right = false;
          }
        }
      } else {
        // Released charge button - trigger leap
        release = true;
        charge_time = player.leap_state.charge_time();
        aim_angle = player.leap_state.aim_angle();
      }
    }

    // Build input using the builder pattern - compiler ensures required fields are set
    LeapInput::builder()
      .charging(charging)
      .facing_right(facing_right) // Required - can't forget this now!
      .aim_angle(aim_angle)
      .release(release)
      .charge_time(charge_time)
      .aim_delta(aim_delta)
      .build()
  }

  fn apply_input(&mut self, ctx: &mut SceneContext, entity: Entity, input: &LeapInput, dt: f32) {
    let mut player = match ctx.world.get_mut::<Player>(entity) {
      Some(p) => p,
      None => return,
    };

    // Update facing direction from input
    player.facing = if input.facing_right {
      Facing::Right
    } else {
      Facing::Left
    };

    match player.leap_state {
      LeapState::Idle => {
        if input.charging {
          player.leap_state = LeapState::Charging {
            charge_time: 0.0,
            aim_angle: input.aim_angle,
          };
        }
      }
      LeapState::Charging {
        charge_time,
        aim_angle,
      } => {
        if input.release {
          // Calculate leap velocity
          let force = player.calculate_leap_force();
          let angle_rad = aim_angle.to_radians();
          let direction = player.facing.sign();

          let vx = force * angle_rad.cos() * direction;
          let vy = -force * angle_rad.sin(); // Negative because Y is down

          if let Some(mut vel) = ctx.world.get_mut::<Velocity>(entity) {
            vel.dx = vx;
            vel.dy = vy;
          }

          if let Some(mut p) = ctx.world.get_mut::<Player>(entity) {
            p.leap_state = LeapState::Airborne;
            p.grounded = false;
          }

          // Play jump sound
          let _ = ctx
            .audio
            .play_sound_with("jump", SoundOptions::new().volume(0.05));
        } else if input.charging {
          // Update charge
          let new_charge = charge_time + dt;
          let new_angle = (aim_angle + input.aim_delta).clamp(MIN_AIM_ANGLE, MAX_AIM_ANGLE);
          player.leap_state = LeapState::Charging {
            charge_time: new_charge,
            aim_angle: new_angle,
          };
        }
      }
      LeapState::Landing { timer } => {
        let new_timer = timer - dt;
        if new_timer <= 0.0 {
          player.leap_state = LeapState::Idle;
        } else {
          player.leap_state = LeapState::Landing { timer: new_timer };
        }
      }
      LeapState::Airborne => {
        // Handled by physics
      }
    }
  }

  fn update_moving_platforms(&mut self, ctx: &mut SceneContext) {
    // Collect moving platform entities and their data
    let moving_platforms: Vec<(Entity, MovingPlatform)> = self
      .platforms
      .iter()
      .filter_map(|&entity| {
        ctx
          .world
          .get::<MovingPlatform>(entity)
          .map(|m| (entity, *m))
      })
      .collect();

    // Moving platforms are kinematic bodies: each travels to its new
    // `Transform` over the physics step, carrying whoever stands on it
    for (plat_entity, moving) in &moving_platforms {
      let new_x = moving.x_at(self.elapsed_time);
      if let Some(mut transform) = ctx.world.get_mut::<Transform>(*plat_entity) {
        transform.translation.x = new_x;
      }
    }

    // Update lamp base_x for lamps on moving platforms
    let lamp_entities: Vec<Entity> = self.platform_lamps.clone();
    for entity in lamp_entities {
      let lamp_data = ctx.world.get::<PlatformLamp>(entity).copied();
      if let Some(lamp) = lamp_data
        && let Some(plat_entity) = lamp.platform_entity
        && let Some(plat_transform) = ctx.world.get::<Transform>(plat_entity)
        && let Some(plat) = ctx.world.get::<Platform>(plat_entity)
        && ctx.world.get::<MovingPlatform>(plat_entity).is_some()
      {
        let new_base_x = plat_transform.translation.x + plat.width / 2.0;
        if let Some(mut lamp_mut) = ctx.world.get_mut::<PlatformLamp>(entity) {
          lamp_mut.base_x = new_base_x;
        }
      }
    }

    // Update dust emitter base_x too
    for &entity in &self.lamp_dust_emitters {
      let lamp_data = ctx.world.get::<PlatformLamp>(entity).copied();
      if let Some(lamp) = lamp_data
        && let Some(plat_entity) = lamp.platform_entity
        && let Some(plat_transform) = ctx.world.get::<Transform>(plat_entity)
        && let Some(plat) = ctx.world.get::<Platform>(plat_entity)
        && ctx.world.get::<MovingPlatform>(plat_entity).is_some()
      {
        let new_base_x = plat_transform.translation.x + plat.width / 2.0;
        if let Some(mut lamp_mut) = ctx.world.get_mut::<PlatformLamp>(entity) {
          lamp_mut.base_x = new_base_x;
        }
      }
    }
  }

  /// Leap physics for the players this side simulates. The engine's
  /// character controller moves them by their `Velocity` after `update`
  /// (in the fixed step, like this), so this reads what their last move
  /// found and adds gravity while they are in the air.
  fn update_physics(&mut self, ctx: &mut SceneContext, dt: f32) {
    for entity in self.simulated_players() {
      let (Some(mut player), Some(mut vel)) = (
        ctx.world.get::<Player>(entity).copied(),
        ctx.world.get::<Velocity>(entity).copied(),
      ) else {
        continue;
      };

      let Some(state) = ctx.world.get::<CharacterState>(entity) else {
        continue;
      };

      // The wall stopped the run; bounce back off it. A wall the last move
      // hit was hit wherever the game has put the player since
      if state.on_wall && player.leap_state.is_airborne() {
        vel.dx = -state.velocity_before.x * WALL_BOUNCE_FACTOR;
      }

      // Just spawned, respawned or corrected: the floor checks would read
      // the old place (a correction while standing would read as leaving
      // the floor and landing again)
      if !self.take_settle_step(entity) {
        // Landing is the game's own state meeting the floor: on the floor
        // while airborne and not rising. That is `landed` on the step it
        // happens, and a landing during the wait above is not lost.
        if state.on_floor && player.leap_state.is_airborne() && vel.dy >= 0.0 {
          vel.dx = 0.0;
          player.grounded = true;
          player.leap_state = LeapState::Landing {
            timer: LANDING_RECOVERY_TIME,
          };
          let _ = ctx
            .audio
            .play_sound_with("land", SoundOptions::new().volume(0.05));
        }
        // Off the floor without leaping: pushed or carried off an edge, or
        // put in the air by a correction
        else if !state.on_floor && !player.leap_state.is_airborne() {
          player.grounded = false;
          player.leap_state = LeapState::Airborne;
        }
      }

      if player.leap_state.is_airborne() {
        vel.dy += GRAVITY * dt;
      }

      if let Some(mut stored) = ctx.world.get_mut::<Player>(entity) {
        *stored = player;
      }
      if let Some(mut stored) = ctx.world.get_mut::<Velocity>(entity) {
        *stored = vel;
      }
    }
  }

  /// Check for player-to-player collisions
  /// - Subtle horizontal push when players overlap
  /// - Head bounce when landing on another player
  ///   Returns true if any collision was detected (used by host to trigger
  ///   an immediate position broadcast so clients learn about the collision
  ///   without waiting for the next 20Hz tick).
  fn check_player_collisions(&mut self, ctx: &mut SceneContext) -> bool {
    // Only check in multiplayer with 2+ players
    if self.players.len() < 2 {
      return false;
    }

    let mut any_collision = false;

    // Collect player data to avoid borrow issues
    let player_data: Vec<(Entity, (Vec2, Vec2), f32, bool)> = self
      .players
      .values()
      .filter_map(|&entity| {
        let transform = ctx.world.get::<Transform>(entity)?;
        let collider = ctx.world.get::<Collider>(entity)?;
        let vel = ctx.world.get::<Velocity>(entity)?;
        let player = ctx.world.get::<Player>(entity)?;
        let bounds = collider.world_bounds(&GlobalTransform::from_transform(transform));
        Some((entity, bounds, vel.dy, player.leap_state.is_airborne()))
      })
      .collect();

    // Check each pair of players
    for i in 0..player_data.len() {
      for j in (i + 1)..player_data.len() {
        let (entity_a, ref a_bounds, avy, a_airborne) = player_data[i];
        let (entity_b, ref b_bounds, bvy, b_airborne) = player_data[j];

        let (a_left, a_top) = (a_bounds.0.x, a_bounds.0.y);
        let (a_right, a_bottom) = (a_bounds.1.x, a_bounds.1.y);

        let (b_left, b_top) = (b_bounds.0.x, b_bounds.0.y);
        let (b_right, b_bottom) = (b_bounds.1.x, b_bounds.1.y);

        // Check horizontal overlap
        let h_overlap = a_right > b_left && a_left < b_right;
        // Check vertical overlap
        let v_overlap = a_bottom > b_top && a_top < b_bottom;

        if !h_overlap || !v_overlap {
          continue; // No collision
        }

        // Calculate overlap amounts
        let h_overlap_amount = (a_right.min(b_right) - a_left.max(b_left)).max(0.0);
        let v_overlap_amount = (a_bottom.min(b_bottom) - a_top.max(b_top)).max(0.0);

        // Head bounce check: is one player landing on top of another?
        // Player A landing on B's head
        if a_airborne && avy > 0.0 && a_bottom <= b_top + HEAD_BOUNCE_ZONE && a_bottom >= b_top {
          // A is landing on B's head - bounce A up
          if let Some(mut vel) = ctx.world.get_mut::<Velocity>(entity_a) {
            vel.dy = HEAD_BOUNCE_VELOCITY;
          }
          // Play a subtle sound
          let _ = ctx
            .audio
            .play_sound_with("land", SoundOptions::new().volume(0.03));
          any_collision = true;
          continue; // Don't apply push collision for head bounce
        }

        // Player B landing on A's head
        if b_airborne && bvy > 0.0 && b_bottom <= a_top + HEAD_BOUNCE_ZONE && b_bottom >= a_top {
          // B is landing on A's head - bounce B up
          if let Some(mut vel) = ctx.world.get_mut::<Velocity>(entity_b) {
            vel.dy = HEAD_BOUNCE_VELOCITY;
          }
          let _ = ctx
            .audio
            .play_sound_with("land", SoundOptions::new().volume(0.03));
          any_collision = true;
          continue;
        }

        // Air-to-air knockback: both players airborne and overlapping — push apart
        if a_airborne && b_airborne {
          let a_center_x = (a_left + a_right) / 2.0;
          let b_center_x = (b_left + b_right) / 2.0;
          let push_dir = if a_center_x <= b_center_x {
            -1.0_f32
          } else {
            1.0
          };

          if let Some(mut vel) = ctx.world.get_mut::<Velocity>(entity_a) {
            vel.dx = AIR_KNOCKBACK_HORIZONTAL * push_dir;
            vel.dy = vel.dy.min(AIR_KNOCKBACK_VERTICAL);
          }
          if let Some(mut vel) = ctx.world.get_mut::<Velocity>(entity_b) {
            vel.dx = AIR_KNOCKBACK_HORIZONTAL * -push_dir;
            vel.dy = vel.dy.min(AIR_KNOCKBACK_VERTICAL);
          }
          any_collision = true;
          continue;
        }

        // Subtle horizontal push when overlapping (only if not a head bounce situation)
        // Only push if there's more horizontal than vertical overlap (side collision)
        if h_overlap_amount < v_overlap_amount * 2.0 {
          // Determine push direction based on visual center positions
          let a_center = (a_left + a_right) / 2.0;
          let b_center = (b_left + b_right) / 2.0;

          // Push away from each other using knockback component
          let (dir_a, dir_b) = if a_center < b_center {
            // A is left of B - push A left, B right
            (Vec2::new(-1.0, 0.0), Vec2::new(1.0, 0.0))
          } else {
            // A is right of B - push A right, B left
            (Vec2::new(1.0, 0.0), Vec2::new(-1.0, 0.0))
          };

          Knockback::apply_with_decay(
            ctx.world,
            entity_a,
            dir_a,
            PLAYER_PUSH_FORCE,
            PLAYER_PUSH_DECAY,
          );
          Knockback::apply_with_decay(
            ctx.world,
            entity_b,
            dir_b,
            PLAYER_PUSH_FORCE,
            PLAYER_PUSH_DECAY,
          );
          any_collision = true;
        }
      }
    }
    any_collision
  }

  fn generate_platforms_as_needed(&mut self, ctx: &mut SceneContext) {
    // Find highest player Y
    let mut highest_y = self.highest_player_y;
    for &entity in self.players.values() {
      if let Some(transform) = ctx.world.get::<Transform>(entity)
        && transform.translation.y < highest_y
      {
        highest_y = transform.translation.y;
      }
    }
    self.highest_player_y = highest_y;

    // Generate chunks as needed
    let required_chunk = PlatformGenerator::required_chunk_for_y(highest_y);

    // Start from chunk 0 (directly above spawn platform)
    for chunk_index in 0..=required_chunk {
      if let Some(platforms) = self.generator.generate_chunk(chunk_index) {
        for pdata in platforms {
          self.spawn_platform_entity(
            ctx,
            pdata.x,
            pdata.y,
            pdata.width,
            chunk_index,
            false,
            pdata.moving,
            pdata.is_checkpoint,
            pdata.platform_index,
          );
        }
      }
    }
  }

  fn update_camera(&mut self, ctx: &mut SceneContext) {
    // Follow local player - camera looks ABOVE the player so they can see where to jump
    let player_y = match self.get_local_player_entity() {
      Some(entity) => ctx.world.get::<Transform>(entity).map(|t| t.translation.y),
      None => None,
    };

    let Some(player_y) = player_y else { return };

    // Target camera position: above the player by CAMERA_LOOK_AHEAD
    // This puts the player near the bottom of the screen
    let target_camera_y = player_y - CAMERA_LOOK_AHEAD;

    // Use faster lerp after respawn for rapid but smooth camera movement
    let lerp_speed = if self.camera_fast_lerp {
      0.15 // Faster lerp for respawn (3x normal speed)
    } else {
      CAMERA_LERP_SPEED
    };

    // Always follow the player with smooth lerping
    self.camera_y += (target_camera_y - self.camera_y) * lerp_speed;

    // Reset fast lerp once camera is close to target
    if self.camera_fast_lerp && (target_camera_y - self.camera_y).abs() < 10.0 {
      self.camera_fast_lerp = false;
    }

    // Update camera position (camera_y is the center Y position)
    ctx
      .camera()
      .set_position(self.screen_width / 2.0, self.camera_y);
  }

  fn update_single_player(&mut self, ctx: &mut SceneContext, dt: f32) {
    self.elapsed_time += dt;

    // Update notifications
    self.update_notifications(dt);

    // Check for death and handle respawn
    self.check_death_and_respawn(ctx, dt);

    // Update height tracking and checkpoints
    self.update_height_and_checkpoints(ctx);
    self.update_future_checkpoint_visual(ctx);

    let input = self.handle_input(ctx, dt);

    if let Some(entity) = self.get_local_player_entity() {
      self.apply_input(ctx, entity, &input, dt);
    }

    self.update_moving_platforms(ctx);
    self.update_physics(ctx, dt);
    self.check_player_collisions(ctx);
    self.generate_platforms_as_needed(ctx);
    self.update_camera(ctx);
    self.update_rain(ctx);
    self.update_background(ctx);
    self.update_ambient_by_height(ctx);
    self.update_player_lights(ctx);
    self.update_player_squish(ctx);
    self.update_lamps(ctx);
    self.update_charge_indicator(ctx);

    // Update particle system
    self.particle_system.update(ctx.world, dt);
  }

  fn update_host(&mut self, ctx: &mut SceneContext, dt: f32) {
    self.elapsed_time += dt;

    // Update notifications
    self.update_notifications(dt);

    // Check for death and handle respawn
    self.check_death_and_respawn(ctx, dt);

    // Update height tracking and checkpoints
    self.update_height_and_checkpoints(ctx);
    self.update_future_checkpoint_visual(ctx);

    let server = self.server.as_mut().unwrap();
    server.update(Duration::from_secs_f32(dt));

    // Process events
    let events: Vec<_> = server.events().collect();
    for event in events {
      match event {
        NetworkEvent::ClientConnected { client_id } => {
          // Reject if server is full
          if self.players.len() >= MAX_PLAYERS {
            self.server.as_mut().unwrap().disconnect(client_id);
            continue;
          }

          // Generate new session ID for this client
          let session_id: u64 = rand::random();
          self.client_to_session.insert(client_id, session_id);

          // Assign dynamic slot
          let slot = self.next_available_slot();
          self.client_slots.insert(client_id, slot);
          self.spawn_player(ctx, slot, client_id);

          // Send game init with seed, session, and server-assigned client ID
          let init_rpc = GameInitRpc {
            seed: self.seed,
            session_id,
            client_id,
            slot,
            checkpoint_x: None,
            checkpoint_y: None,
            checkpoint_height: None,
            elapsed_time: self.elapsed_time,
          };
          if let Ok(serialized) = SerializedRpc::from_rpc(
            &init_rpc,
            self.local_client_id.unwrap(),
            RpcTarget::Client(client_id),
          ) && let Ok(bytes) = serialized.to_bytes()
          {
            self
              .server
              .as_mut()
              .unwrap()
              .send(client_id, Channel::ReliableOrdered, &bytes);
          }

          // Show connection notification
          self.notifications.push(ConnectionNotification {
            message: format!("Player {} connected", slot + 1),
            timer: NOTIFICATION_DURATION,
            is_connect: true,
          });
        }
        NetworkEvent::ClientDisconnected { client_id, .. } => {
          // Save session data before removing player
          if let Some(session_id) = self.client_to_session.remove(&client_id) {
            // Use the disconnecting player's own checkpoint data
            let cp = self
              .player_checkpoints
              .get(&client_id)
              .cloned()
              .unwrap_or_else(|| PlayerCheckpoint::new(self.screen_width));
            if cp.last_checkpoint_height > 0.0 {
              self.client_sessions.insert(
                session_id,
                SessionData {
                  checkpoint_x: cp.center_x,
                  checkpoint_y: cp.y,
                  checkpoint_height: cp.last_checkpoint_height,
                  max_height: cp.last_checkpoint_height,
                },
              );
            }
          }

          let slot = self.client_slots.remove(&client_id).unwrap_or(0);

          if let Some(entity) = self.players.remove(&client_id) {
            self.settling.remove(&entity);
            let _ = ctx.world.despawn(entity);
          }
          if let Some(light_entity) = self.player_lights.remove(&client_id) {
            let _ = ctx.world.despawn(light_entity);
          }
          self.remote_best_heights.remove(&client_id);
          self.remote_slots.remove(&client_id);
          self.player_checkpoints.remove(&client_id);

          // Show disconnection notification
          self.notifications.push(ConnectionNotification {
            message: format!("Player {} disconnected", slot + 1),
            timer: NOTIFICATION_DURATION,
            is_connect: false,
          });
        }
        _ => {}
      }
    }

    // Receive input from clients - collect inputs first to avoid borrow issues
    let mut pending_inputs: Vec<(Entity, LeapInput)> = Vec::new();
    let mut pending_respawns: Vec<(Entity, ClientId)> = Vec::new();
    let mut pending_reconnects: Vec<(ClientId, u64)> = Vec::new();
    let mut pending_pongs: Vec<(ClientId, u32)> = Vec::new();
    {
      let server = self.server.as_mut().unwrap();
      for client_id in server.remote_clients().collect::<Vec<_>>() {
        while let Some(data) = server.receive(client_id, Channel::ReliableOrdered) {
          if let Ok(serialized) = SerializedRpc::from_bytes(&data) {
            // Try to parse as LeapInputRpc
            if let Ok(rpc) = serialized.clone().into_rpc::<LeapInputRpc>() {
              if let Some(&entity) = self.players.get(&client_id) {
                pending_inputs.push((entity, rpc.payload.input));
              }
            }
            // Try to parse as RespawnRpc
            else if serialized.clone().into_rpc::<RespawnRpc>().is_ok() {
              if let Some(&entity) = self.players.get(&client_id) {
                pending_respawns.push((entity, client_id));
              }
            }
            // Try to parse as SessionReconnectRpc
            else if let Ok(rpc) = serialized.clone().into_rpc::<SessionReconnectRpc>() {
              pending_reconnects.push((client_id, rpc.payload.session_id));
            }
            // Try to parse as PingRpc - echo back as PongRpc
            else if let Ok(rpc) = serialized.into_rpc::<PingRpc>() {
              pending_pongs.push((client_id, rpc.payload.token));
            }
          }
        }
      }
    }

    // Send pong responses
    for (client_id, token) in pending_pongs {
      let pong = PongRpc { token };
      if let Ok(serialized) = SerializedRpc::from_rpc(
        &pong,
        self.local_client_id.unwrap(),
        RpcTarget::Client(client_id),
      ) && let Ok(bytes) = serialized.to_bytes()
      {
        self
          .server
          .as_mut()
          .unwrap()
          .send(client_id, Channel::ReliableOrdered, &bytes);
      }
    }

    // Handle session reconnects - send checkpoint data if we have it
    for (client_id, session_id) in pending_reconnects {
      if let Some(session_data) = self.client_sessions.get(&session_id).cloned() {
        // Update client_to_session mapping to use the reconnected session
        self.client_to_session.insert(client_id, session_id);

        // Assign a fresh slot
        let slot = self.next_available_slot();
        self.client_slots.insert(client_id, slot);

        // Send updated init with checkpoint data
        let init_rpc = GameInitRpc {
          seed: self.seed,
          session_id,
          client_id,
          slot,
          checkpoint_x: Some(session_data.checkpoint_x),
          checkpoint_y: Some(session_data.checkpoint_y),
          checkpoint_height: Some(session_data.checkpoint_height),
          elapsed_time: self.elapsed_time,
        };
        if let Ok(serialized) = SerializedRpc::from_rpc(
          &init_rpc,
          self.local_client_id.unwrap(),
          RpcTarget::Client(client_id),
        ) && let Ok(bytes) = serialized.to_bytes()
        {
          self
            .server
            .as_mut()
            .unwrap()
            .send(client_id, Channel::ReliableOrdered, &bytes);
        }

        // Show reconnect notification
        self.notifications.push(ConnectionNotification {
          message: format!("Player {} reconnected", slot + 1),
          timer: NOTIFICATION_DURATION,
          is_connect: true,
        });
      }
    }

    for (entity, input) in pending_inputs {
      self.apply_input(ctx, entity, &input, dt);
    }
    for (entity, client_id) in pending_respawns {
      self.respawn_player(ctx, entity, client_id);
    }

    // Handle local input
    let input = self.handle_input(ctx, dt);
    if let Some(entity) = self.get_local_player_entity() {
      self.apply_input(ctx, entity, &input, dt);
    }

    self.update_moving_platforms(ctx);
    self.update_physics(ctx, dt);
    let had_collision = self.check_player_collisions(ctx);
    self.generate_platforms_as_needed(ctx);
    self.update_camera(ctx);
    self.update_rain(ctx);
    self.update_background(ctx);
    self.update_ambient_by_height(ctx);
    self.update_player_lights(ctx);
    self.update_player_squish(ctx);
    self.update_lamps(ctx);
    self.update_charge_indicator(ctx);

    // Update particle system
    self.particle_system.update(ctx.world, dt);

    // Track remote players' best heights and slots on the host side
    let local_id = self.local_client_id;
    for (&client_id, &entity) in &self.players {
      if Some(client_id) == local_id {
        continue;
      }
      if let Some(transform) = ctx.world.get::<Transform>(entity)
        && let Some(player) = ctx.world.get::<Player>(entity)
      {
        self.remote_slots.insert(client_id, player.slot);
        let height = ((PLAYER_SPAWN_Y - transform.translation.y) / HEIGHT_SCALE).max(0.0);
        if player.grounded {
          let best = self.remote_best_heights.entry(client_id).or_insert(0.0);
          if height > *best {
            *best = height;
          }
        }
      }
    }

    // Broadcast positions — immediately after collisions so clients learn
    // about velocity/state changes without waiting for the next 20Hz tick.
    // Otherwise fall back to the normal 20Hz broadcast rate.
    self.broadcast_timer += dt;
    if had_collision || self.broadcast_timer >= POSITION_BROADCAST_RATE {
      self.broadcast_timer = 0.0;
      self.broadcast_positions(ctx);
    }
  }

  fn update_client(&mut self, ctx: &mut SceneContext, dt: f32) {
    // Note: elapsed_time is advanced AFTER network sync below (not here)
    // so the host's authoritative time is adopted before adding this frame's dt.

    // Update notifications
    self.update_notifications(dt);

    // Check for death and handle respawn
    self.check_death_and_respawn(ctx, dt);

    // Update height tracking and checkpoints
    self.update_height_and_checkpoints(ctx);

    let client = self.client.as_mut().unwrap();
    client.update(Duration::from_secs_f32(dt));

    // Check connection
    if !self.connected && client.is_connected() {
      self.connected = true;
      let client_id = client.client_id();
      self.local_client_id = Some(client_id);
      self.prediction.set_local_client_id(Some(client_id));
    }

    // Process events
    let events: Vec<_> = client.events().collect();
    for event in events {
      match event {
        NetworkEvent::Connected => {
          self.connected = true;
          let client_id = self.client.as_ref().unwrap().client_id();
          self.local_client_id = Some(client_id);
          self.prediction.set_local_client_id(Some(client_id));

          // Send session reconnect request if we have a cached session
          if self.session_id != 0 {
            let reconnect_rpc = SessionReconnectRpc {
              session_id: self.session_id,
            };
            if let Some(client) = self.client.as_mut()
              && let Ok(serialized) =
                SerializedRpc::from_rpc(&reconnect_rpc, client_id, RpcTarget::Server)
              && let Ok(bytes) = serialized.to_bytes()
            {
              client.send(Channel::ReliableOrdered, &bytes);
            }
          }
        }
        NetworkEvent::Disconnected { .. } => {
          self.connected = false;
        }
        _ => {}
      }
    }

    if !self.connected {
      return;
    }

    // Receive seed and position updates - collect data first to avoid borrow issues
    let mut received_init: Option<GameInitRpc> = None;
    let mut position_updates: Vec<PositionUpdate> = Vec::new();
    {
      let client = self.client.as_mut().unwrap();

      // Check for game init/seed/pong (reliable)
      while let Some(data) = client.receive(Channel::ReliableOrdered) {
        if let Ok(serialized) = SerializedRpc::from_bytes(&data) {
          // Try new GameInitRpc first
          if let Ok(rpc) = serialized.clone().into_rpc::<GameInitRpc>() {
            received_init = Some(rpc.payload);
          }
          // Fall back to old GameSeedRpc for backwards compatibility
          else if let Ok(rpc) = serialized.clone().into_rpc::<GameSeedRpc>() {
            received_init = Some(GameInitRpc {
              seed: rpc.payload.seed,
              session_id: 0,
              client_id: 0,
              slot: 1,
              checkpoint_x: None,
              checkpoint_y: None,
              checkpoint_height: None,
              elapsed_time: 0.0,
            });
          }
          // Pong response for ping measurement
          else if serialized.into_rpc::<PongRpc>().is_ok()
            && let Some(sent_at) = self.ping_send_time.take()
          {
            self.ping_ms = Some(sent_at.elapsed().as_millis() as u32);
          }
        }
      }

      // Check for position updates (unreliable)
      while let Some(data) = client.receive(Channel::Unreliable) {
        if let Ok(update) = deserialize::<PositionUpdate>(&data) {
          position_updates.push(update);
        }
      }
    }

    // Process game init if received
    if let Some(init) = received_init {
      // Use the server-assigned client ID so we match position updates
      if init.client_id != 0 {
        self.local_client_id = Some(ClientId::from(init.client_id));
        self
          .prediction
          .set_local_client_id(Some(ClientId::from(init.client_id)));
      }

      self.seed = init.seed;

      // Despawn any platforms/lamps generated before receiving the real seed
      for &entity in &self.platforms {
        let _ = ctx.world.despawn(entity);
      }
      self.platforms.clear();
      for &entity in &self.platform_lamps {
        let _ = ctx.world.despawn(entity);
      }
      self.platform_lamps.clear();
      for &entity in &self.lamp_dust_emitters {
        let _ = ctx.world.despawn(entity);
      }
      self.lamp_dust_emitters.clear();
      self.highest_player_y = PLAYER_SPAWN_Y;

      self.generator.reset_with_seed(self.seed);
      self.received_seed = true;
      self.spawn_starting_platform(ctx);
      self.generate_platforms_as_needed(ctx);

      // Sync elapsed_time from host so moving platforms start at correct phase
      if init.elapsed_time > 0.0 {
        self.elapsed_time = init.elapsed_time;
        self.host_elapsed_time = init.elapsed_time;
      }

      // Save session ID for future reconnects
      if init.session_id != 0 {
        self.session_id = init.session_id;
        save_session_id(&self.room_code, init.session_id);
      }

      // Restore checkpoint if provided (reconnect case)
      if let (Some(cx), Some(cy), Some(ch)) =
        (init.checkpoint_x, init.checkpoint_y, init.checkpoint_height)
      {
        if let Some(local_id) = self.local_client_id {
          let cp = self.get_or_init_checkpoint(local_id);
          cp.center_x = cx;
          cp.y = cy;
          cp.last_checkpoint_height = ch;
        }
        // Trigger a respawn to checkpoint position after player spawns
        self.respawn_timer = Some(0.1);
      }
    }

    // Process position updates using PredictionHelper
    for update in position_updates {
      // Sync elapsed time from host for deterministic moving platform positions.
      // Replace local time with host's authoritative value (skip out-of-order packets
      // that would move time backward).
      if update.host_elapsed_time > self.host_elapsed_time {
        self.host_elapsed_time = update.host_elapsed_time;
        self.elapsed_time = update.host_elapsed_time;
      }

      // Sync checkpoint state from host — only apply for local player's update
      if Some(update.client_id) == self.local_client_id {
        let cp = self.get_or_init_checkpoint(update.client_id);
        cp.center_x = update.checkpoint_center_x;
        cp.platform_width = update.checkpoint_platform_width;
        cp.y = update.checkpoint_y;
        // Derive checkpoint height from Y position so HUD displays correctly
        let height = (PLAYER_SPAWN_Y - update.checkpoint_y) / HEIGHT_SCALE;
        if height > cp.last_checkpoint_height {
          cp.last_checkpoint_height = height;
        }
      }

      // Spawn player if not yet spawned
      let entity = if let Some(&e) = self.players.get(&update.client_id) {
        e
      } else {
        self.spawn_player(ctx, update.slot, update.client_id)
      };

      // Convert to ServerPositionUpdate for PredictionHelper
      let server_update = ServerPositionUpdate::new(update.client_id, update.x, update.y, 0)
        .with_velocity(update.vx, update.vy);

      // Use PredictionHelper to handle the update correctly
      match self
        .prediction
        .handle_position_update(ctx.world, entity, &server_update)
      {
        UpdateResult::Applied | UpdateResult::Spawned => {
          // Remote player or first spawn - also update velocity and game state
          if let Some(mut vel) = ctx.world.get_mut::<Velocity>(entity) {
            vel.dx = update.vx;
            vel.dy = update.vy;
          }
          if let Some(mut player) = ctx.world.get_mut::<Player>(entity) {
            player.leap_state = update.leap_state.into();
            player.grounded = update.grounded;
            player.facing = if update.facing_right {
              Facing::Right
            } else {
              Facing::Left
            };
          }

          // Play sounds for remote player state changes (with spatial audio)
          let prev_state = self
            .previous_leap_states
            .get(&update.client_id)
            .copied()
            .unwrap_or(NetworkLeapState::Idle);
          let new_state = update.leap_state;

          // Get local player position for spatial audio
          let local_pos = self
            .get_local_player_entity()
            .and_then(|e| ctx.world.get::<Transform>(e))
            .map(|t| (t.translation.x, t.translation.y));

          // Detect jump (transition to Airborne)
          if !matches!(prev_state, NetworkLeapState::Airborne)
            && matches!(new_state, NetworkLeapState::Airborne)
          {
            let options = if let Some((lx, ly)) = local_pos {
              spatial_sound_options(lx, ly, update.x, update.y).volume(0.05)
            } else {
              SoundOptions::new().volume(0.05)
            };
            let _ = ctx.audio.play_sound_with("jump", options);
          }

          // Detect land (transition to Landing)
          if !matches!(prev_state, NetworkLeapState::Landing { .. })
            && matches!(new_state, NetworkLeapState::Landing { .. })
          {
            let options = if let Some((lx, ly)) = local_pos {
              spatial_sound_options(lx, ly, update.x, update.y).volume(0.05)
            } else {
              SoundOptions::new().volume(0.05)
            };
            let _ = ctx.audio.play_sound_with("land", options);
          }

          // Store current state for next comparison
          self
            .previous_leap_states
            .insert(update.client_id, new_state);

          // Track remote player's best height and slot
          self
            .remote_best_heights
            .insert(update.client_id, update.best_height);
          self.remote_slots.insert(update.client_id, update.slot);
        }
        UpdateResult::StoredForReconciliation => {
          // Local player — reconcile predicted state with server authority.
          // Read the client's current predicted state to compare.
          let (client_grounded, client_airborne) = ctx
            .world
            .get::<Player>(entity)
            .map(|p| (p.grounded, p.leap_state.is_airborne()))
            .unwrap_or((false, false));

          let server_grounded = update.grounded;
          let server_airborne = matches!(update.leap_state, NetworkLeapState::Airborne);

          // Critical state correction: server and client disagree on
          // grounded/airborne state (e.g. knocked off a platform by another
          // player, or landed on host but client hasn't detected it yet).
          // Trust the server — snap position, velocity, and state.
          // (Each `Transform` write below teleports the local player's
          // controller; `teleport` makes the landing checks wait for it.)
          if (server_airborne && client_grounded) || (server_grounded && client_airborne) {
            self.teleport(ctx, entity, Vec2::new(update.x, update.y));
            if let Some(mut vel) = ctx.world.get_mut::<Velocity>(entity) {
              vel.dx = update.vx;
              vel.dy = update.vy;
            }
            if let Some(mut player) = ctx.world.get_mut::<Player>(entity) {
              player.leap_state = update.leap_state.into();
              player.grounded = update.grounded;
            }
          }
          // Position drift correction: same state but positions diverged
          // significantly. Lerp toward the server position to prevent
          // gradual drift without jarring snaps.
          else {
            let client_pos = ctx
              .world
              .get::<Transform>(entity)
              .map(|t| (t.translation.x, t.translation.y));
            if let Some((cx, cy)) = client_pos {
              let dx = update.x - cx;
              let dy = update.y - cy;
              let dist_sq = dx * dx + dy * dy;
              // Large discrepancy (>48px, ~1.5 player heights): snap
              if dist_sq > 48.0 * 48.0 {
                self.teleport(ctx, entity, Vec2::new(update.x, update.y));
                if let Some(mut vel) = ctx.world.get_mut::<Velocity>(entity) {
                  vel.dx = update.vx;
                  vel.dy = update.vy;
                }
              }
              // Moderate discrepancy (>8px): smooth correction
              else if dist_sq > 8.0 * 8.0 {
                let blend = 0.3; // Blend 30% toward server each update (20Hz)
                self.teleport(ctx, entity, Vec2::new(cx + dx * blend, cy + dy * blend));
                // Also nudge velocity toward server to prevent re-diverging
                if let Some(mut vel) = ctx.world.get_mut::<Velocity>(entity) {
                  vel.dx += (update.vx - vel.dx) * blend;
                  vel.dy += (update.vy - vel.dy) * blend;
                }
              }
            }
          }
        }
        UpdateResult::Skipped => {}
      }
    }

    // A player spawned before the host said which client this is (its
    // position can beat the init message) became a remote one: once it is
    // known to be ours, simulate it
    if let Some(entity) = self.get_local_player_entity()
      && ctx.world.get::<CharacterController>(entity).is_none()
    {
      self.make_controlled(ctx, entity);
    }

    // Sync checkpoint visual after processing all position updates
    self.sync_checkpoint_visual(ctx);
    self.update_future_checkpoint_visual(ctx);

    // Handle local input and apply it locally (client-side prediction)
    let input = self.handle_input(ctx, dt);
    if let Some(entity) = self.get_local_player_entity() {
      self.apply_input(ctx, entity, &input, dt);
    }

    // Send input to server
    let client = self.client.as_mut().unwrap();
    let client_id = client.client_id();
    let input_rpc = LeapInputRpc { input };
    if let Ok(serialized) = SerializedRpc::from_rpc(&input_rpc, client_id, RpcTarget::Server)
      && let Ok(bytes) = serialized.to_bytes()
    {
      client.send(Channel::ReliableOrdered, &bytes);
    }

    // Send ping every second
    self.ping_timer += dt;
    if self.ping_timer >= 1.0 {
      self.ping_timer = 0.0;
      let ping = PingRpc {
        token: self.ping_ms.unwrap_or(0),
      };
      if let Ok(serialized) = SerializedRpc::from_rpc(&ping, client_id, RpcTarget::Server)
        && let Ok(bytes) = serialized.to_bytes()
      {
        let client = self.client.as_mut().unwrap();
        client.send(Channel::ReliableOrdered, &bytes);
        self.ping_send_time = Some(std::time::Instant::now());
      }
    }

    // Dead-reckon remote players for smooth movement between 20Hz host updates.
    // Apply game-specific gravity to airborne remotes first, then let the engine
    // extrapolate positions from velocity.
    let local_id = self.local_client_id;
    for (&cid, &entity) in &self.players {
      if Some(cid) == local_id {
        continue;
      }
      if ctx
        .world
        .get::<Player>(entity)
        .is_some_and(|p| p.leap_state.is_airborne())
        && let Some(mut vel) = ctx.world.get_mut::<Velocity>(entity)
      {
        vel.dy += GRAVITY * dt;
      }
    }
    self.prediction.dead_reckon_remotes(ctx.world, dt);

    // Interpolate elapsed_time between host ticks for smooth platform motion.
    // host_elapsed_time is the last authoritative value; we add local dt to it
    // but cap so we never drift more than one broadcast interval ahead.
    self.elapsed_time =
      (self.elapsed_time + dt).min(self.host_elapsed_time + POSITION_BROADCAST_RATE);

    // Run local physics for smooth movement (client-side prediction)
    self.update_moving_platforms(ctx);
    self.update_physics(ctx, dt);
    // NOTE: Player-to-player collisions are NOT run on the client.
    // The host is authoritative for all collision results — running them here
    // against dead-reckoned remote positions produces different outcomes than
    // the host, causing permanent desync (especially on moving platforms).

    self.generate_platforms_as_needed(ctx);
    self.update_camera(ctx);
    self.update_rain(ctx);
    self.update_background(ctx);
    self.update_ambient_by_height(ctx);
    self.update_player_lights(ctx);
    self.update_player_squish(ctx);
    self.update_lamps(ctx);
    self.update_charge_indicator(ctx);

    // Update particle system
    self.particle_system.update(ctx.world, dt);
  }

  fn broadcast_positions(&mut self, ctx: &mut SceneContext) {
    let server = match self.server.as_mut() {
      Some(s) => s,
      None => return,
    };

    let mut updates: Vec<PositionUpdate> = Vec::new();

    // Get local client's best height
    let local_id = self.local_client_id;

    for (&client_id, &entity) in &self.players {
      if let Some(transform) = ctx.world.get::<Transform>(entity)
        && let Some(vel) = ctx.world.get::<Velocity>(entity)
        && let Some(player) = ctx.world.get::<Player>(entity)
      {
        // Use local max_height for local player, remote_best_heights for others
        let best_height = if Some(client_id) == local_id {
          self.max_height
        } else {
          *self.remote_best_heights.get(&client_id).unwrap_or(&0.0)
        };

        let default_cp = PlayerCheckpoint::new(self.screen_width);
        let cp = self
          .player_checkpoints
          .get(&client_id)
          .unwrap_or(&default_cp);
        updates.push(PositionUpdate {
          client_id,
          slot: player.slot,
          x: transform.translation.x,
          y: transform.translation.y,
          vx: vel.dx,
          vy: vel.dy,
          leap_state: player.leap_state.into(),
          facing_right: player.facing == Facing::Right,
          grounded: player.grounded,
          best_height,
          checkpoint_center_x: cp.center_x,
          checkpoint_platform_width: cp.platform_width,
          checkpoint_y: cp.y,
          host_elapsed_time: self.elapsed_time,
        });
      }
    }

    let remote_clients: Vec<_> = server.remote_clients().collect();
    for update in &updates {
      if let Ok(bytes) = serialize(update) {
        for &remote_id in &remote_clients {
          server.send(remote_id, Channel::Unreliable, &bytes);
        }
      }
    }
  }

  fn update_notifications(&mut self, dt: f32) {
    // Update notification timers and remove expired ones
    for notif in &mut self.notifications {
      notif.timer -= dt;
    }
    self.notifications.retain(|n| n.timer > 0.0);
  }

  fn update_charge_indicator(&mut self, ctx: &mut SceneContext) {
    self.charge_indicator.visible = false;

    let entity = match self.get_local_player_entity() {
      Some(e) => e,
      None => return,
    };

    let player = match ctx.world.get::<Player>(entity) {
      Some(p) => p,
      None => return,
    };

    if !player.leap_state.is_charging() {
      return;
    }

    let transform = match ctx.world.get::<Transform>(entity) {
      Some(t) => t,
      None => return,
    };

    let charge_time = player.leap_state.charge_time();
    let aim_angle = player.leap_state.aim_angle();

    // Calculate trajectory velocity
    let force = player.calculate_leap_force();
    let angle_rad = aim_angle.to_radians();
    let direction = player.facing.sign();

    self.charge_indicator.visible = true;
    // Convert world position to screen position via camera
    let world_x = transform.translation.x + PLAYER_WIDTH / 2.0;
    let world_y = transform.translation.y + PLAYER_HEIGHT / 2.0;
    let Vec2 { x: sx, y: sy } = ctx.camera().world_to_screen(Vec2::new(world_x, world_y));
    self.charge_indicator.screen_x = sx;
    self.charge_indicator.screen_y = sy;
    self.charge_indicator.charge_pct = (charge_time / MAX_CHARGE_TIME).min(1.0);
    self.charge_indicator.is_overcharging = charge_time > OVERCHARGE_THRESHOLD;
    self.charge_indicator.trajectory_vx = force * angle_rad.cos() * direction;
    self.charge_indicator.trajectory_vy = -force * angle_rad.sin();
  }

  fn draw_charge_indicator(&self, ui: &mut UiContext) {
    if !self.charge_indicator.visible {
      return;
    }

    // Screen position was already computed from world coords via camera
    let screen_x = self.charge_indicator.screen_x;
    let screen_y = self.charge_indicator.screen_y;

    // Get direction from velocity
    let vx = self.charge_indicator.trajectory_vx;
    let vy = self.charge_indicator.trajectory_vy;

    // Normalize direction vector
    let len = (vx * vx + vy * vy).sqrt();
    if len < 0.01 {
      return;
    }
    let dir_x = vx / len;
    let dir_y = vy / len;

    // Line grows from nothing with charge
    let charge_pct = self.charge_indicator.charge_pct;
    let max_len = 35.0; // Shorter max length
    let line_len = max_len * charge_pct;

    // Don't draw if too short
    if line_len < 2.0 {
      return;
    }

    let (base_r, base_g, base_b) = if self.charge_indicator.is_overcharging {
      OVERCHARGE_COLOR
    } else {
      CHARGE_INDICATOR_COLOR
    };

    // Draw the main line (thinner)
    let start_offset = 14.0; // Start just outside player sprite
    let thickness = 1.5; // Thinner line
    let step = 1.0;
    let end_pos = start_offset + line_len;

    // Draw line segments with slight fade at the start
    let mut t = start_offset;
    while t <= end_pos {
      let progress = (t - start_offset) / line_len; // 0 at start, 1 at end
      let alpha = (progress * 255.0).min(255.0) as u8;
      let line_color = Color::new(base_r, base_g, base_b, alpha);

      let px = screen_x + dir_x * t;
      let py = screen_y + dir_y * t;
      ui.draw_rect(
        Rect::new(
          px - thickness / 2.0,
          py - thickness / 2.0,
          thickness,
          thickness,
        ),
        line_color,
      );
      t += step;
    }

    // Arrowhead at the tip that grows with charge
    let tip_x = screen_x + dir_x * end_pos;
    let tip_y = screen_y + dir_y * end_pos;

    // Perpendicular vector for arrow wings
    let perp_x = -dir_y;
    let perp_y = dir_x;

    // Arrow size grows with charge (smaller)
    let arrow_size = 4.0 * charge_pct;
    let arrow_length = 5.0 * charge_pct;

    // Arrow colors
    let arrow_color = Color::new(base_r, base_g, base_b, 220);
    let glow_color = Color::new(base_r, base_g, base_b, (100.0 * charge_pct) as u8);

    // Draw glow behind arrow (smaller)
    ui.draw_rect(Rect::new(tip_x - 2.0, tip_y - 2.0, 4.0, 4.0), glow_color);

    // Draw chevron arrowhead using small rectangles
    // The arrow points in the direction of (dir_x, dir_y)
    // Draw two wings going backward from tip
    let wing_steps = 4;
    for i in 0..wing_steps {
      let t = (i as f32 + 1.0) / wing_steps as f32;
      let back_dist = arrow_length * t;
      let spread = arrow_size * t;

      // Position along the backward direction
      let back_x = tip_x - dir_x * back_dist;
      let back_y = tip_y - dir_y * back_dist;

      // Left wing point
      let left_x = back_x + perp_x * spread;
      let left_y = back_y + perp_y * spread;
      ui.draw_rect(Rect::new(left_x - 0.5, left_y - 0.5, 1.5, 1.5), arrow_color);

      // Right wing point
      let right_x = back_x - perp_x * spread;
      let right_y = back_y - perp_y * spread;
      ui.draw_rect(
        Rect::new(right_x - 0.5, right_y - 0.5, 1.5, 1.5),
        arrow_color,
      );
    }

    // Tip point (muted)
    let tip_color = Color::new(200, 200, 190, (200.0 * charge_pct) as u8);
    ui.draw_rect(Rect::new(tip_x - 0.75, tip_y - 0.75, 1.5, 1.5), tip_color);
  }
}

impl Scene for GameScene {
  fn name(&self) -> &str {
    "GameScene"
  }

  /// The quit dialog stops the game: `update` returns early while it is up,
  /// and the physics step, which moves the players, stops with it.
  fn is_physics_paused(&self) -> bool {
    self.show_quit_dialog
  }

  fn on_enter(&mut self, ctx: &mut SceneContext) {
    self.screen_width = ctx.screen_width();
    self.screen_height = ctx.screen_height();
    self.generator = PlatformGenerator::new(self.seed, self.screen_width);

    ctx
      .camera()
      .set_position(self.screen_width / 2.0, self.screen_height / 2.0);

    // Set up atmospheric lighting
    {
      let mut lighting = ctx.lighting();
      // Dark cave ambient - will transition as player climbs
      lighting.set_ambient(AmbientLight::new(
        Color::rgb(
          AMBIENT_DARK_COLOR.0,
          AMBIENT_DARK_COLOR.1,
          AMBIENT_DARK_COLOR.2,
        ),
        AMBIENT_DARK_INTENSITY,
      ));
      // Dusty atmosphere for visible light beams
      lighting.set_atmosphere(Atmosphere::dusty());
    }

    // Enable ambient occlusion for corner darkening
    ctx.effects().set_ambient_occlusion(0.35, 6.0);

    // Spawn rain particles
    self.spawn_rain(ctx);

    // Rain audio is already playing from menu scene (persists between scenes)

    // Register console command handler (auto-cleaned on scene exit)
    ctx.console().register("game:respawn", |_args, cmd_ctx| {
      CONSOLE_RESPAWN_REQUESTED.store(true, Ordering::SeqCst);
      cmd_ctx.console.print_system("Respawning at checkpoint...");
      CommandResult::Ok
    });

    // Load jump and land sound effects
    let _ = ctx.audio.load_sound("jump", "assets/jump.mp3");
    let _ = ctx.audio.load_sound("land", "assets/land.wav");
    self.sounds_loaded = true;

    // Spawn spawn platform and the walls either side
    self.spawn_starting_platform(ctx);
    self.spawn_walls(ctx);

    // Generate initial platforms above spawn (skip for client — wait for seed from host)
    if self.mode != GameMode::Client {
      self.generate_platforms_as_needed(ctx);
    }

    // Spawn local player
    match self.mode {
      GameMode::SinglePlayer => {
        let id = self.local_client_id.unwrap();
        self.client_slots.insert(id, 0);
        self.spawn_player(ctx, 0, id);
      }
      GameMode::Host => {
        let id = self.local_client_id.unwrap();
        self.client_slots.insert(id, 0);
        self.spawn_player(ctx, 0, id);
      }
      GameMode::Client => {
        // Wait for position updates from server
      }
    }
  }

  fn update(&mut self, ctx: &mut SceneContext, dt: f32) -> SceneAction {
    // First, so nothing below sees a respawn point a lower zone moved, and
    // the quit dialog cannot let the messages age out unread
    self.read_checkpoints(ctx);

    if self.pending_quit {
      if let Some(client) = self.client.as_mut() {
        client.disconnect();
      }
      return SceneAction::Switch(Box::new(MenuScene::new()));
    }

    if ctx.input.key_pressed(KeyCode::Escape)
      || ctx.input.gamepad_button_pressed(GamepadButton::Start)
    {
      self.show_quit_dialog = !self.show_quit_dialog;
      self.quit_dialog_selection = 1; // Default to Cancel when opening
      self.quit_input_cooldown = 0.2;
    }

    // Handle quit dialog input
    if self.show_quit_dialog {
      self.quit_input_cooldown = (self.quit_input_cooldown - dt).max(0.0);

      // B button cancels
      if ctx.input.gamepad_button_pressed(GamepadButton::East) {
        self.show_quit_dialog = false;
        self.input_delay = 0.15; // Prevent button from triggering charge
      }

      // Navigation
      if self.quit_input_cooldown <= 0.0 {
        let nav_left = ctx.input.key_pressed(KeyCode::ArrowLeft)
          || ctx.input.key_pressed(KeyCode::KeyA)
          || ctx.input.gamepad_button_pressed(GamepadButton::DPadLeft)
          || ctx.input.left_stick_x() < -0.5;
        let nav_right = ctx.input.key_pressed(KeyCode::ArrowRight)
          || ctx.input.key_pressed(KeyCode::KeyD)
          || ctx.input.gamepad_button_pressed(GamepadButton::DPadRight)
          || ctx.input.left_stick_x() > 0.5;

        if nav_left || nav_right {
          self.quit_dialog_selection = 1 - self.quit_dialog_selection;
          self.quit_input_cooldown = 0.15;
        }
      }

      // Confirm with Enter, Space, or A button
      let confirm = ctx.input.key_pressed(KeyCode::Enter)
        || ctx.input.key_pressed(KeyCode::Space)
        || ctx.input.gamepad_button_pressed(GamepadButton::South);

      if confirm {
        if self.quit_dialog_selection == 0 {
          self.pending_quit = true;
        } else {
          self.show_quit_dialog = false;
          self.input_delay = 0.15; // Prevent button from triggering charge
        }
      }

      return SceneAction::Continue;
    }

    // R or Y/Triangle (North button) or console command to respawn
    if ctx.input.key_pressed(KeyCode::KeyR)
      || ctx.input.gamepad_button_pressed(GamepadButton::North)
      || CONSOLE_RESPAWN_REQUESTED.swap(false, Ordering::SeqCst)
    {
      self.execute_respawn(ctx);
    }

    if true {
      match self.mode {
        GameMode::SinglePlayer => self.update_single_player(ctx, dt),
        GameMode::Host => self.update_host(ctx, dt),
        GameMode::Client => self.update_client(ctx, dt),
      }
    }

    SceneAction::Continue
  }

  fn ui(&mut self, ui: &mut UiContext, _ctx: &mut SceneContext) -> SceneAction {
    let (sw, sh) = ui.screen_size();

    // Room info (top right)
    if !self.room_code.is_empty() {
      let code_text = format!("Room: {}", self.room_code);
      let (tw, _) = ui.text_size(&code_text);
      ui.draw_text(
        &code_text,
        sw as f32 - tw as f32 - 20.0,
        20.0,
        Color::new(255, 255, 255, 100),
      );

      let mut right_y = 40.0;

      // Player count (both host and client)
      let player_text = format!("Players: {}/{}", self.players.len(), MAX_PLAYERS);
      let (tw, _) = ui.text_size(&player_text);
      ui.draw_text(
        &player_text,
        sw as f32 - tw as f32 - 20.0,
        right_y,
        Color::new(200, 200, 200, 150),
      );
      right_y += 20.0;

      // Ping display for client
      if self.mode == GameMode::Client
        && let Some(ping) = self.ping_ms
      {
        let ping_text = format!("Ping {}ms", ping);
        let (tw, _) = ui.text_size(&ping_text);
        let ping_color = if ping < 80 {
          Color::new(100, 200, 100, 150)
        } else if ping < 150 {
          Color::new(200, 200, 100, 150)
        } else {
          Color::new(200, 100, 100, 150)
        };
        ui.draw_text(
          &ping_text,
          sw as f32 - tw as f32 - 20.0,
          right_y,
          ping_color,
        );
      }
    }

    // Height display (top left)
    let height_text = format!("{:.0}m", self.current_height);
    ui.draw_text(&height_text, 20.0, 20.0, Color::WHITE);

    // Always show local best height
    let max_text = format!("Best: {:.0}m", self.max_height);
    ui.draw_text(&max_text, 20.0, 45.0, Color::new(150, 150, 150, 200));

    // Show remote players' best heights below local best - multiplayer only
    let mut rival_line_count = 0u32;
    if self.mode != GameMode::SinglePlayer {
      let local_id = self.local_client_id;
      // Collect remote players with their slots for sorted display
      let mut remotes: Vec<(u8, f32)> = Vec::new();
      for (client_id, &best_height) in &self.remote_best_heights {
        if Some(*client_id) != local_id && best_height > 0.0 {
          let slot = self.remote_slots.get(client_id).copied().unwrap_or(1);
          remotes.push((slot, best_height));
        }
      }
      remotes.sort_by_key(|(slot, _)| *slot);

      for (i, (slot, best_height)) in remotes.iter().enumerate() {
        let idx = (*slot as usize).min(PLAYER_COLORS.len() - 1);
        let color = PLAYER_COLORS[idx];
        let rival_text = format!("P{}: {:.0}m", slot + 1, best_height);
        ui.draw_text(
          &rival_text,
          20.0,
          70.0 + i as f32 * 25.0,
          Color::rgb(color.0, color.1, color.2),
        );
        rival_line_count += 1;
      }
    }

    // Checkpoint indicator (shift down based on number of rival lines)
    let local_cp = self.local_checkpoint();
    let checkpoint_base_y = 70.0 + rival_line_count as f32 * 25.0;
    if local_cp.last_checkpoint_height > 0.0 {
      let checkpoint_text = format!("Checkpoint: {:.0}m", local_cp.last_checkpoint_height);
      ui.draw_text(
        &checkpoint_text,
        20.0,
        checkpoint_base_y,
        Color::rgb(
          CHECKPOINT_PLATFORM_COLOR.0,
          CHECKPOINT_PLATFORM_COLOR.1,
          CHECKPOINT_PLATFORM_COLOR.2,
        ),
      );
    }

    // Next checkpoint indicator
    let next_threshold = Self::next_checkpoint_threshold(local_cp.last_checkpoint_height);
    let next_text = format!("Next Checkpoint: {:.0}m", next_threshold);
    ui.draw_text(
      &next_text,
      20.0,
      checkpoint_base_y + 25.0,
      Color::rgb(
        FUTURE_CHECKPOINT_COLOR.0,
        FUTURE_CHECKPOINT_COLOR.1,
        FUTURE_CHECKPOINT_COLOR.2,
      ),
    );

    // Respawn hint (bottom right)
    let respawn_text = "R / Y / Triangle : Respawn";
    let (rtw, _) = ui.text_size(respawn_text);
    ui.draw_text(
      respawn_text,
      sw as f32 - rtw as f32 - 20.0,
      sh as f32 - 30.0,
      Color::new(150, 150, 150, 150),
    );

    // Draw charge indicator using cached state
    self.draw_charge_indicator(ui);

    // Connection status (only show for client when not connected)
    if self.mode == GameMode::Client && !self.connected {
      ui.draw_text_anchored("Connecting...", Anchor::Center, 0.0, -50.0, Color::YELLOW);
    }

    // Connection notifications (stacked at top center)
    let mut notif_y = 80.0;
    for notif in &self.notifications {
      let alpha = ((notif.timer / NOTIFICATION_DURATION) * 255.0).min(255.0) as u8;
      let color = if notif.is_connect {
        Color::new(100, 200, 100, alpha) // Green for connect
      } else {
        Color::new(200, 150, 100, alpha) // Orange for disconnect
      };
      ui.draw_text_anchored(&notif.message, Anchor::TopCenter, 0.0, notif_y, color);
      notif_y += 25.0;
    }

    // Quit dialog
    if self.show_quit_dialog {
      let center_x = sw as f32 / 2.0;
      let center_y = sh as f32 / 2.0;

      ui.draw_rect(
        Rect::new(0.0, 0.0, sw as f32, sh as f32),
        Color::new(0, 0, 0, 180),
      );

      let dialog_w = 300.0;
      let dialog_h = 150.0;
      let dialog_x = center_x - dialog_w / 2.0;
      let dialog_y = center_y - dialog_h / 2.0;

      ui.draw_rect(
        Rect::new(dialog_x, dialog_y, dialog_w, dialog_h),
        Color::rgb(40, 40, 50),
      );
      ui.draw_rect_outline(
        Rect::new(dialog_x, dialog_y, dialog_w, dialog_h),
        Color::WHITE,
        2.0,
      );

      ui.draw_text_anchored("Leave Game?", Anchor::Center, 0.0, -30.0, Color::WHITE);

      let btn_w = 100.0;
      let btn_h = 36.0;
      let btn_y = center_y + 25.0;
      let highlight = Color::rgb(PLAYER_COLORS[0].0, PLAYER_COLORS[0].1, PLAYER_COLORS[0].2);

      // Yes button
      let yes_rect = Rect::new(center_x - btn_w - 20.0, btn_y, btn_w, btn_h);
      if self.quit_dialog_selection == 0 {
        ui.draw_rect(
          Rect::new(
            yes_rect.x - 3.0,
            yes_rect.y - 3.0,
            yes_rect.width + 6.0,
            yes_rect.height + 6.0,
          ),
          highlight,
        );
      }
      if ui.button("Yes, Leave", yes_rect) {
        self.pending_quit = true;
      }

      // Cancel button
      let cancel_rect = Rect::new(center_x + 20.0, btn_y, btn_w, btn_h);
      if self.quit_dialog_selection == 1 {
        ui.draw_rect(
          Rect::new(
            cancel_rect.x - 3.0,
            cancel_rect.y - 3.0,
            cancel_rect.width + 6.0,
            cancel_rect.height + 6.0,
          ),
          highlight,
        );
      }
      if ui.button("Cancel", cancel_rect) {
        self.show_quit_dialog = false;
      }
    }
    SceneAction::Continue
  }

  fn render(&mut self, gpu: &mut GpuRenderContext) {
    // Collect particles for rendering
    self
      .particle_system
      .collect_for_render(gpu.world, gpu.batch);
  }
}
