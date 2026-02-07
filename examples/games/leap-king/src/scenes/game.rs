//! Game scene with leap mechanics and multiplayer

use std::collections::HashMap;
use std::fs;
use std::net::{SocketAddr, UdpSocket};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

static CONSOLE_RESPAWN_REQUESTED: AtomicBool = AtomicBool::new(false);

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

/// Get the local LAN IP address
fn get_lan_ip() -> Option<String> {
  let socket = UdpSocket::bind("0.0.0.0:0").ok()?;
  socket.connect("8.8.8.8:80").ok()?;
  let local_addr = socket.local_addr().ok()?;
  Some(local_addr.ip().to_string())
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

pub struct GameScene {
  mode: GameMode,
  room_code: String,

  // Networking
  server: Option<ListenServer>,
  client: Option<NetworkClient>,

  // Host IP (for display)
  lan_ip: Option<String>,

  // Signal server URL (for WebRTC/Internet play display)
  signal_url: Option<String>,

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

  // Track remote players' best heights for display
  remote_best_heights: HashMap<ClientId, f32>,

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

  // Death/respawn
  respawn_timer: Option<f32>,

  // Checkpoint system
  checkpoint_center_x: f32,
  checkpoint_platform_width: f32,
  checkpoint_y: f32,
  checkpoint_platform: Option<Entity>,
  last_checkpoint_height: f32,
  last_synced_checkpoint_y: f32,
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
      lan_ip: None,
      signal_url: None,
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
      seed,
      generator: PlatformGenerator::new(seed, 0.0),
      highest_player_y: PLAYER_SPAWN_Y,
      camera_y: PLAYER_SPAWN_Y - CAMERA_LOOK_AHEAD,
      camera_fast_lerp: false,
      broadcast_timer: 0.0,
      elapsed_time: 0.0,
      respawn_timer: None,
      checkpoint_center_x: 0.0, // Set properly in on_enter when screen_width is known
      checkpoint_platform_width: SPAWN_PLATFORM_WIDTH,
      checkpoint_y: PLAYER_SPAWN_Y,
      checkpoint_platform: None,
      last_checkpoint_height: 0.0,
      last_synced_checkpoint_y: PLAYER_SPAWN_Y,
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
    })
  }

  /// Create host (server) game
  pub fn new_host(room_code: String) -> Result<Self, NetworkError> {
    let server = ListenServer::new(NetworkServerConfig {
      port: DEFAULT_PORT,
      max_clients: 2,
      protocol_id: PROTOCOL_ID,
      transport: TransportConfig::Udp,
    })?;

    let host_id = server.host_id();
    let lan_ip = get_lan_ip();
    let seed: u64 = rand::random();

    Ok(Self {
      mode: GameMode::Host,
      room_code,
      server: Some(server),
      client: None,
      lan_ip,
      signal_url: None,
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
      seed,
      generator: PlatformGenerator::new(seed, 0.0),
      highest_player_y: PLAYER_SPAWN_Y,
      camera_y: PLAYER_SPAWN_Y - CAMERA_LOOK_AHEAD,
      camera_fast_lerp: false,
      broadcast_timer: 0.0,
      elapsed_time: 0.0,
      respawn_timer: None,
      checkpoint_center_x: 0.0, // Set properly in on_enter when screen_width is known
      checkpoint_platform_width: SPAWN_PLATFORM_WIDTH,
      checkpoint_y: PLAYER_SPAWN_Y,
      checkpoint_platform: None,
      last_checkpoint_height: 0.0,
      last_synced_checkpoint_y: PLAYER_SPAWN_Y,
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
    })
  }

  /// Create host (server) game over WebRTC
  pub fn new_host_webrtc(room_code: String, signal_url: &str) -> Result<Self, NetworkError> {
    let server = ListenServer::new(NetworkServerConfig {
      max_clients: 2,
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
      lan_ip: None,
      signal_url: Some(signal_url.to_string()),
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
      seed,
      generator: PlatformGenerator::new(seed, 0.0),
      highest_player_y: PLAYER_SPAWN_Y,
      camera_y: PLAYER_SPAWN_Y - CAMERA_LOOK_AHEAD,
      camera_fast_lerp: false,
      broadcast_timer: 0.0,
      elapsed_time: 0.0,
      respawn_timer: None,
      checkpoint_center_x: 0.0,
      checkpoint_platform_width: SPAWN_PLATFORM_WIDTH,
      checkpoint_y: PLAYER_SPAWN_Y,
      checkpoint_platform: None,
      last_checkpoint_height: 0.0,
      last_synced_checkpoint_y: PLAYER_SPAWN_Y,
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

    let signal_url = if is_webrtc {
      Some(address.to_string())
    } else {
      None
    };

    Ok(Self {
      mode: GameMode::Client,
      room_code: room_code.to_string(),
      server: None,
      client: Some(client),
      lan_ip: None,
      signal_url,
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
      seed: 0, // Will receive from server
      generator: PlatformGenerator::new(0, 0.0),
      highest_player_y: PLAYER_SPAWN_Y,
      camera_y: PLAYER_SPAWN_Y - CAMERA_LOOK_AHEAD,
      camera_fast_lerp: false,
      broadcast_timer: 0.0,
      elapsed_time: 0.0,
      respawn_timer: None,
      checkpoint_center_x: 0.0, // Set properly in on_enter when screen_width is known
      checkpoint_platform_width: SPAWN_PLATFORM_WIDTH,
      checkpoint_y: PLAYER_SPAWN_Y,
      checkpoint_platform: None,
      last_checkpoint_height: 0.0,
      last_synced_checkpoint_y: PLAYER_SPAWN_Y,
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
    })
  }

  /// Compute spawn X position for a player slot, centered on the spawn platform
  fn spawn_x_for_slot(&self, slot: u8) -> f32 {
    let center = self.screen_width / 2.0;
    if slot == 0 {
      center - PLAYER_SPAWN_OFFSET
    } else {
      center + PLAYER_SPAWN_OFFSET
    }
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
    platform_index: u32,
  ) -> Entity {
    let color = if is_spawn {
      SPAWN_PLATFORM_COLOR
    } else if is_moving {
      MOVING_PLATFORM_COLOR
    } else {
      PLATFORM_COLOR
    };

    let entity = ctx.world.spawn((
      Transform::new(x, y),
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
      .with_z_order(Z_PLATFORM),
      Collider::box_collider(width, PLATFORM_HEIGHT),
      // Note: Shadow volume occluders removed - the geometry-based shadow projection
      // creates harsh triangular artifacts. The soft light falloff looks better.
      if is_spawn {
        Platform::spawn_platform()
      } else {
        Platform::new(width, chunk_y)
      },
    ));

    // Attach MovingPlatform component if this is a moving platform
    if is_moving {
      let phase_seed = self
        .seed
        .wrapping_add(chunk_y as u64 * 4919 + platform_index as u64);
      let phase = (phase_seed % 628) as f32 / 100.0;
      let moving = MovingPlatform::new(x, width, self.screen_width, MOVING_PLATFORM_SPEED, phase);
      let _ = ctx.world.insert_one(entity, moving);
    }

    self.platforms.push(entity);

    // Spawn a lamp above the platform (not for spawn platform)
    if !is_spawn {
      self.spawn_platform_lamp(ctx, x, y, width, chunk_y, entity, platform_index);
    }

    entity
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
    let outer_lamp = ctx.world.spawn((
      Transform::new(lamp_x, lamp_y),
      Light2D::point_with_intensity(
        Color::rgb(LAMP_LIGHT_COLOR.0, LAMP_LIGHT_COLOR.1, LAMP_LIGHT_COLOR.2),
        LAMP_LIGHT_RADIUS,
        LAMP_LIGHT_INTENSITY,
      )
      .with_soft_shadows(LAMP_SHADOW_SOFTNESS),
      PlatformLamp::new(lamp_x, lamp_y, phase, platform_width, speed_mult)
        .with_platform_entity(platform_entity),
    ));
    self.platform_lamps.push(outer_lamp);

    // Inner bright core light (brighter, smaller)
    let inner_lamp = ctx.world.spawn((
      Transform::new(lamp_x, lamp_y),
      Light2D::point_with_intensity(
        Color::rgb(LAMP_INNER_COLOR.0, LAMP_INNER_COLOR.1, LAMP_INNER_COLOR.2),
        LAMP_INNER_RADIUS,
        LAMP_INNER_INTENSITY,
      ),
      PlatformLamp::new(lamp_x, lamp_y, phase, platform_width, speed_mult)
        .with_platform_entity(platform_entity),
    ));
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
      let dust_emitter = ctx.world.spawn((
        Transform::new(lamp_x, lamp_y),
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
      ));
      self.lamp_dust_emitters.push(dust_emitter);
    }
  }

  fn update_lamps(&mut self, ctx: &mut SceneContext) {
    let lamp_entities: Vec<Entity> = self.platform_lamps.clone();

    for entity in lamp_entities {
      let lamp_data = ctx.world.get_component::<PlatformLamp>(entity).map(|l| *l);

      if let Some(lamp) = lamp_data
        && let Some(mut transform) = ctx.world.get_component_mut::<Transform>(entity)
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

        transform.x = lamp.base_x + offset_x + flicker.0;
        transform.y = lamp.base_y + offset_y + flicker.1;
      }
    }

    // Update dust emitter positions to follow their lamps
    for entity in &self.lamp_dust_emitters {
      let lamp_data = ctx.world.get_component::<PlatformLamp>(*entity).map(|l| *l);

      if let Some(lamp) = lamp_data
        && let Some(mut transform) = ctx.world.get_component_mut::<Transform>(*entity)
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

        transform.x = lamp.base_x + offset_x;
        transform.y = lamp.base_y + offset_y;
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
    let rain_entity = ctx.world.spawn((
      Transform::new(
        self.screen_width / 2.0,
        self.camera_y - self.screen_height / 2.0,
      ),
      emitter,
    ));

    self.rain_emitter = Some(rain_entity);
  }

  fn update_rain(&mut self, ctx: &mut SceneContext) {
    // Keep rain emitter above the camera
    if let Some(rain_entity) = self.rain_emitter
      && let Some(mut transform) = ctx.world.get_component_mut::<Transform>(rain_entity)
    {
      transform.x = self.screen_width / 2.0;
      transform.y = self.camera_y - self.screen_height / 2.0; // Top of visible area
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
      if let Some(mut transform) = ctx.world.get_component_mut::<Transform>(entity) {
        // If tile is below visible area, move it up
        if transform.y > visible_bottom {
          // Move up by total height of all tiles
          transform.y -= self.bg_tile_height * self.background_tiles.len() as f32;
        }
        // If tile is above visible area, move it down
        else if transform.y + self.bg_tile_height < visible_top {
          transform.y += self.bg_tile_height * self.background_tiles.len() as f32;
        }
      }
    }
  }

  /// Update ambient lighting based on player height - gets brighter as you climb
  fn update_ambient_by_height(&self, ctx: &mut SceneContext) {
    let Some(lighting) = ctx.lighting else {
      return;
    };

    // Calculate height factor (0.0 at bottom, 1.0 at max height)
    let height_factor = (self.current_height / HEIGHT_FOR_MAX_AMBIENT).clamp(0.0, 1.0);

    // Lerp between dark cave and brighter sky
    let r = lerp_u8(AMBIENT_DARK_COLOR.0, AMBIENT_BRIGHT_COLOR.0, height_factor);
    let g = lerp_u8(AMBIENT_DARK_COLOR.1, AMBIENT_BRIGHT_COLOR.1, height_factor);
    let b = lerp_u8(AMBIENT_DARK_COLOR.2, AMBIENT_BRIGHT_COLOR.2, height_factor);
    let intensity =
      AMBIENT_DARK_INTENSITY + (AMBIENT_BRIGHT_INTENSITY - AMBIENT_DARK_INTENSITY) * height_factor;

    lighting.set_ambient(AmbientLight::new(Color::rgb(r, g, b), intensity));
  }

  fn update_player_lights(&mut self, ctx: &mut SceneContext) {
    // Update each player light to follow its player
    for (&client_id, &light_entity) in &self.player_lights {
      let Some(&player_entity) = self.players.get(&client_id) else {
        continue;
      };

      let Some(player_transform) = ctx.world.get_component::<Transform>(player_entity) else {
        continue;
      };

      // Center the light on the player
      let light_x = player_transform.x + PLAYER_WIDTH / 2.0;
      let light_y = player_transform.y + PLAYER_HEIGHT / 2.0;

      if let Some(mut light_transform) = ctx.world.get_component_mut::<Transform>(light_entity) {
        light_transform.x = light_x;
        light_transform.y = light_y;
      }
    }
  }

  fn update_player_squish(&mut self, ctx: &mut SceneContext) {
    // Update player visual squish based on charge state
    let time = self.elapsed_time;

    for &entity in self.players.values() {
      let Some(player) = ctx.world.get_component::<Player>(entity) else {
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

      if let Some(mut transform) = ctx.world.get_component_mut::<Transform>(entity) {
        transform.scale_x = scale_x;
        transform.scale_y = scale_y;
        // Visual lean effect (doesn't affect collision)
        transform.offset_x = offset_x;
        // Offset Y to keep feet grounded when squishing
        // When scale_y < 1, the sprite gets shorter, so offset down to keep feet in place
        transform.offset_y = PLAYER_HEIGHT * (1.0 - scale_y);
      }
    }
  }

  fn spawn_player(&mut self, ctx: &mut SceneContext, slot: u8, client_id: ClientId) -> Entity {
    let spawn_x = self.spawn_x_for_slot(slot);
    let color = if slot == 0 {
      PLAYER_A_COLOR
    } else {
      PLAYER_B_COLOR
    };

    // Determine if this is the local player (already spawned means we can predict)
    let is_local = self.local_client_id == Some(client_id);
    let predicted = if is_local {
      // Local player starts as spawned for immediate prediction
      Predicted::new_spawned(client_id)
    } else {
      // Remote players start unspawned, will be marked spawned on first server update
      Predicted::new(client_id)
    };

    let entity = ctx.world.spawn((
      Transform::new(spawn_x, PLAYER_SPAWN_Y),
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
      .with_z_order(Z_PLAYER),
      Collider::box_collider(PLAYER_WIDTH, PLAYER_HEIGHT),
      Player::new(slot),
      Velocity::zero(),
      predicted,
    ));

    self.players.insert(client_id, entity);

    // Spawn a light for this player - with subtle volumetric glow
    let light_entity = ctx.world.spawn((
      Transform::new(
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
    ));
    self.player_lights.insert(client_id, light_entity);

    entity
  }

  fn get_local_player_entity(&self) -> Option<Entity> {
    self
      .local_client_id
      .and_then(|id| self.players.get(&id).copied())
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

    let Some(transform) = ctx.world.get_component::<Transform>(entity) else {
      return;
    };

    // Death threshold is relative to checkpoint position
    let death_y = self.checkpoint_y + DEATH_Y_OFFSET;

    // Start respawn timer if player fell below death threshold
    if transform.y > death_y {
      self.respawn_timer = Some(RESPAWN_DELAY);
    }
  }

  /// Update height tracking and check for new checkpoints
  fn update_height_and_checkpoints(&mut self, ctx: &mut SceneContext) {
    let Some(entity) = self.get_local_player_entity() else {
      return;
    };

    // Extract values from components first to avoid borrow issues
    let (player_x, player_y, is_grounded) = {
      let Some(transform) = ctx.world.get_component::<Transform>(entity) else {
        return;
      };
      let Some(player) = ctx.world.get_component::<Player>(entity) else {
        return;
      };
      (transform.x, transform.y, player.grounded)
    };

    // Calculate height (player spawn Y is baseline 0, going up is positive)
    // Y decreases as we go up, so we invert it
    let raw_height = (PLAYER_SPAWN_Y - player_y) / HEIGHT_SCALE;
    self.current_height = raw_height.max(0.0);

    // Track max height (only when grounded on a platform)
    if is_grounded && self.current_height > self.max_height {
      self.max_height = self.current_height;
    }

    // Client relies on host for checkpoint detection — skip local detection
    if self.mode == GameMode::Client {
      return;
    }

    // Check if player is grounded on a platform and has reached a new checkpoint height
    if is_grounded && self.current_height >= FIRST_CHECKPOINT_HEIGHT {
      // Walk progressive checkpoint thresholds (interval grows each time)
      let mut threshold = FIRST_CHECKPOINT_HEIGHT;
      let mut interval = CHECKPOINT_INTERVAL;
      let mut current_checkpoint_height = 0.0_f32;
      while threshold <= self.current_height {
        current_checkpoint_height = threshold;
        threshold += interval;
        interval += CHECKPOINT_INTERVAL_GROWTH;
      }

      // Set new checkpoint if we've passed a new threshold
      if current_checkpoint_height > self.last_checkpoint_height {
        // Find the platform the player is standing on and set checkpoint to its center
        let player_bottom = player_y + PLAYER_HEIGHT;
        if let Some((plat_center_x, plat_top, plat_width)) =
          self.find_and_mark_checkpoint_platform(ctx, player_x, player_bottom)
        {
          self.checkpoint_center_x = plat_center_x;
          self.checkpoint_platform_width = plat_width;
          self.checkpoint_y = plat_top - PLAYER_HEIGHT;
          self.last_checkpoint_height = current_checkpoint_height;
        }
      }
    }
  }

  /// Find the platform under the player and mark it as checkpoint (colors it green)
  /// Returns (platform center X, top Y, platform width) if found
  fn find_and_mark_checkpoint_platform(
    &mut self,
    ctx: &mut SceneContext,
    player_x: f32,
    player_bottom: f32,
  ) -> Option<(f32, f32, f32)> {
    // Revert old checkpoint platform to normal color (if it exists and isn't spawn platform)
    if let Some(old_platform) = self.checkpoint_platform {
      // Get platform info first (releases borrow before mutable borrow)
      let platform_width = ctx
        .world
        .get_component::<Platform>(old_platform)
        .filter(|p| !p.is_spawn)
        .map(|p| p.width);
      let is_moving = ctx
        .world
        .get_component::<MovingPlatform>(old_platform)
        .is_some();

      if let Some(width) = platform_width
        && let Some(mut renderable) = ctx.world.get_component_mut::<Renderable>(old_platform)
      {
        let color = if is_moving {
          MOVING_PLATFORM_COLOR
        } else {
          PLATFORM_COLOR
        };
        let new_sprite = SpriteHandle::from_sprite(
          Sprite::solid_rect(
            width as u32,
            PLATFORM_HEIGHT as u32,
            color.0,
            color.1,
            color.2,
            255,
          ),
          format!("platform_normal_{}", old_platform.id()),
        );
        renderable.sprite = new_sprite;
      }
    }

    // Find the platform the player is standing on
    for &platform_entity in &self.platforms {
      let Some(platform_transform) = ctx.world.get_component::<Transform>(platform_entity) else {
        continue;
      };
      let Some(platform) = ctx.world.get_component::<Platform>(platform_entity) else {
        continue;
      };

      // Skip spawn platform (already green)
      if platform.is_spawn {
        continue;
      }

      // Check if player is on this platform
      let plat_left = platform_transform.x;
      let plat_right = platform_transform.x + platform.width;
      let plat_top = platform_transform.y;

      // Player center X should be within platform bounds, and player bottom should be near platform top
      let player_center_x = player_x + PLAYER_WIDTH / 2.0;
      if player_center_x >= plat_left
        && player_center_x <= plat_right
        && (player_bottom - plat_top).abs() < 5.0
      {
        // Found the platform - color it green
        if let Some(mut renderable) = ctx.world.get_component_mut::<Renderable>(platform_entity) {
          let new_sprite = SpriteHandle::from_sprite(
            Sprite::solid_rect(
              platform.width as u32,
              PLATFORM_HEIGHT as u32,
              CHECKPOINT_PLATFORM_COLOR.0,
              CHECKPOINT_PLATFORM_COLOR.1,
              CHECKPOINT_PLATFORM_COLOR.2,
              255,
            ),
            format!("platform_checkpoint_{}", platform_entity.id()),
          );
          renderable.sprite = new_sprite;
        }
        self.checkpoint_platform = Some(platform_entity);

        // Return platform center X, top Y, and width
        let center_x = platform_transform.x + platform.width / 2.0;
        return Some((center_x, plat_top, platform.width));
      }
    }

    None
  }

  /// Sync checkpoint visual on the client side.
  /// When the host sends a new checkpoint_y, find the matching platform and color it green.
  fn sync_checkpoint_visual(&mut self, ctx: &mut SceneContext) {
    if (self.checkpoint_y - self.last_synced_checkpoint_y).abs() < 0.1 {
      return;
    }

    // Revert old checkpoint platform color
    if let Some(old_platform) = self.checkpoint_platform {
      let platform_info = ctx
        .world
        .get_component::<Platform>(old_platform)
        .filter(|p| !p.is_spawn)
        .map(|p| p.width);
      let is_moving = ctx
        .world
        .get_component::<MovingPlatform>(old_platform)
        .is_some();

      if let Some(width) = platform_info
        && let Some(mut renderable) = ctx.world.get_component_mut::<Renderable>(old_platform)
      {
        let color = if is_moving {
          MOVING_PLATFORM_COLOR
        } else {
          PLATFORM_COLOR
        };
        let new_sprite = SpriteHandle::from_sprite(
          Sprite::solid_rect(
            width as u32,
            PLATFORM_HEIGHT as u32,
            color.0,
            color.1,
            color.2,
            255,
          ),
          format!("platform_normal_{}", old_platform.id()),
        );
        renderable.sprite = new_sprite;
      }
    }

    // Find platform at the new checkpoint position
    let target_plat_y = self.checkpoint_y + PLAYER_HEIGHT;
    for &platform_entity in &self.platforms {
      let Some(transform) = ctx.world.get_component::<Transform>(platform_entity) else {
        continue;
      };
      let Some(platform) = ctx.world.get_component::<Platform>(platform_entity) else {
        continue;
      };
      if platform.is_spawn {
        continue;
      }

      // Match by Y position (player stands on top of platform)
      if (transform.y - target_plat_y).abs() > 2.0 {
        continue;
      }

      // Match by X: checkpoint_center_x should be within the platform bounds
      let plat_left = transform.x;
      let plat_right = transform.x + platform.width;
      if self.checkpoint_center_x >= plat_left && self.checkpoint_center_x <= plat_right {
        // Color it green
        if let Some(mut renderable) = ctx.world.get_component_mut::<Renderable>(platform_entity) {
          let new_sprite = SpriteHandle::from_sprite(
            Sprite::solid_rect(
              platform.width as u32,
              PLATFORM_HEIGHT as u32,
              CHECKPOINT_PLATFORM_COLOR.0,
              CHECKPOINT_PLATFORM_COLOR.1,
              CHECKPOINT_PLATFORM_COLOR.2,
              255,
            ),
            format!("platform_checkpoint_{}", platform_entity.id()),
          );
          renderable.sprite = new_sprite;
        }
        self.checkpoint_platform = Some(platform_entity);
        break;
      }
    }

    self.last_synced_checkpoint_y = self.checkpoint_y;
  }

  /// Compute per-slot spawn X on the current checkpoint platform.
  /// Slot 0 spawns on the far left, slot 1 on the far right.
  /// If the other player overlaps the intended spawn, swap to the opposite side.
  fn checkpoint_spawn_x(&self, ctx: &SceneContext, slot: u8) -> f32 {
    let half_width = self.checkpoint_platform_width / 2.0;
    let left_x = self.checkpoint_center_x - half_width;
    let right_x = (self.checkpoint_center_x + half_width - PLAYER_WIDTH).max(left_x);

    let preferred_x = if slot == 0 { left_x } else { right_x };
    let opposite_x = if slot == 0 { right_x } else { left_x };

    // In single player, no anti-overlap needed
    if self.mode == GameMode::SinglePlayer {
      return preferred_x;
    }

    // Check if any other player overlaps the preferred spawn position
    for (&_cid, &other_entity) in &self.players {
      let other_slot = ctx
        .world
        .get_component::<Player>(other_entity)
        .map(|p| p.slot);
      if other_slot == Some(slot) {
        continue; // Skip self
      }
      if let Some(other_transform) = ctx.world.get_component::<Transform>(other_entity) {
        let other_left = other_transform.x;
        let other_right = other_transform.x + PLAYER_WIDTH;
        let spawn_left = preferred_x;
        let spawn_right = preferred_x + PLAYER_WIDTH;
        // Check horizontal overlap (they share the same Y on the platform)
        if spawn_left < other_right && spawn_right > other_left {
          return opposite_x;
        }
      }
    }

    preferred_x
  }

  /// Execute respawn using NetworkAction pattern
  /// - For clients: applies locally AND sends to server (dual execution)
  /// - For host/single player: applies locally only
  fn execute_respawn(&mut self, ctx: &mut SceneContext) {
    let Some(entity) = self.get_local_player_entity() else {
      return;
    };

    let slot = ctx
      .world
      .get_component::<Player>(entity)
      .map(|p| p.slot)
      .unwrap_or(0);

    let spawn_x = self.checkpoint_spawn_x(ctx, slot);
    let spawn_y = self.checkpoint_y;

    // Apply respawn locally: reset position, velocity, and state.
    // Do NOT use RespawnAction (which calls force_correction) — the prediction
    // system preserves local positions for the local entity, so the respawn sticks
    // until the server catches up.
    if let Some(mut transform) = ctx.world.get_component_mut::<Transform>(entity) {
      transform.x = spawn_x;
      transform.y = spawn_y;
    }
    if let Some(mut vel) = ctx.world.get_component_mut::<Velocity>(entity) {
      vel.dx = 0.0;
      vel.dy = 0.0;
    }
    if let Some(mut player) = ctx.world.get_component_mut::<Player>(entity) {
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
  fn respawn_player(&mut self, ctx: &mut SceneContext, entity: Entity) {
    let slot = ctx
      .world
      .get_component::<Player>(entity)
      .map(|p| p.slot)
      .unwrap_or(0);

    let spawn_x = self.checkpoint_spawn_x(ctx, slot);
    let spawn_y = self.checkpoint_y;

    let action = RespawnAction {
      slot,
      spawn_x,
      spawn_y,
    };

    let mut action_ctx = ActionContext::new(ctx.world);
    action.apply(&mut action_ctx);
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
    let player = match ctx.world.get_component::<Player>(entity) {
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
    let mut player = match ctx.world.get_component_mut::<Player>(entity) {
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

          drop(player);

          if let Some(mut vel) = ctx.world.get_component_mut::<Velocity>(entity) {
            vel.dx = vx;
            vel.dy = vy;
          }

          if let Some(mut p) = ctx.world.get_component_mut::<Player>(entity) {
            p.leap_state = LeapState::Airborne;
            p.grounded = false;
          }

          // Play jump sound
          if let Some(audio) = &mut ctx.audio {
            let _ = audio.play_sound_with("jump", SoundOptions::new().volume(0.05));
          }
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
          .get_component::<MovingPlatform>(entity)
          .map(|m| (entity, *m))
      })
      .collect();

    // Determine which player entities to move (same logic as update_physics)
    let local_entity = self.get_local_player_entity();
    let player_entities: Vec<Entity> = if self.mode == GameMode::Client {
      local_entity.into_iter().collect()
    } else {
      self.players.values().copied().collect()
    };

    for (plat_entity, moving) in &moving_platforms {
      let old_x = ctx
        .world
        .get_component::<Transform>(*plat_entity)
        .map(|t| t.x)
        .unwrap_or(0.0);

      let new_x = moving.x_at(self.elapsed_time);
      let delta_x = new_x - old_x;

      // Update platform position
      if let Some(mut transform) = ctx.world.get_component_mut::<Transform>(*plat_entity) {
        transform.x = new_x;
      }

      // Get platform dimensions for rider check
      let plat_width = ctx
        .world
        .get_component::<Platform>(*plat_entity)
        .map(|p| p.width)
        .unwrap_or(0.0);
      let plat_top = ctx
        .world
        .get_component::<Transform>(*plat_entity)
        .map(|t| t.y)
        .unwrap_or(0.0);

      // Move grounded players standing on this platform
      if delta_x.abs() > 0.001 {
        for &player_entity in &player_entities {
          let is_grounded = ctx
            .world
            .get_component::<Player>(player_entity)
            .map(|p| p.grounded)
            .unwrap_or(false);
          if !is_grounded {
            continue;
          }

          if let Some(pt) = ctx.world.get_component::<Transform>(player_entity) {
            let player_left = pt.x;
            let player_right = pt.x + PLAYER_WIDTH;
            let player_bottom = pt.y + PLAYER_HEIGHT;

            // Check if player is standing on this platform
            let on_platform = player_right > new_x
              && player_left < new_x + plat_width
              && (player_bottom - plat_top).abs() < 5.0;

            if on_platform {
              // Drop the immutable borrow before taking mutable
              drop(pt);
              if let Some(mut pt_mut) = ctx.world.get_component_mut::<Transform>(player_entity) {
                pt_mut.x = (pt_mut.x + delta_x).clamp(0.0, self.screen_width - PLAYER_WIDTH);
              }
            }
          }
        }
      }
    }

    // Update lamp base_x for lamps on moving platforms
    let lamp_entities: Vec<Entity> = self.platform_lamps.clone();
    for entity in lamp_entities {
      let lamp_data = ctx.world.get_component::<PlatformLamp>(entity).map(|l| *l);
      if let Some(lamp) = lamp_data
        && let Some(plat_entity) = lamp.platform_entity
        && let Some(plat_transform) = ctx.world.get_component::<Transform>(plat_entity)
        && let Some(plat) = ctx.world.get_component::<Platform>(plat_entity)
        && ctx
          .world
          .get_component::<MovingPlatform>(plat_entity)
          .is_some()
      {
        let new_base_x = plat_transform.x + plat.width / 2.0;
        if let Some(mut lamp_mut) = ctx.world.get_component_mut::<PlatformLamp>(entity) {
          lamp_mut.base_x = new_base_x;
        }
      }
    }

    // Update dust emitter base_x too
    for &entity in &self.lamp_dust_emitters {
      let lamp_data = ctx.world.get_component::<PlatformLamp>(entity).map(|l| *l);
      if let Some(lamp) = lamp_data
        && let Some(plat_entity) = lamp.platform_entity
        && let Some(plat_transform) = ctx.world.get_component::<Transform>(plat_entity)
        && let Some(plat) = ctx.world.get_component::<Platform>(plat_entity)
        && ctx
          .world
          .get_component::<MovingPlatform>(plat_entity)
          .is_some()
      {
        let new_base_x = plat_transform.x + plat.width / 2.0;
        if let Some(mut lamp_mut) = ctx.world.get_component_mut::<PlatformLamp>(entity) {
          lamp_mut.base_x = new_base_x;
        }
      }
    }
  }

  fn update_physics(&mut self, ctx: &mut SceneContext, dt: f32) {
    // On client, only simulate the local player — remote entities get position from network.
    // On host/single player, simulate all entities.
    let local_entity = self.get_local_player_entity();
    let player_entities: Vec<Entity> = if self.mode == GameMode::Client {
      local_entity.into_iter().collect()
    } else {
      self.players.values().copied().collect()
    };

    for entity in player_entities {
      // Check if airborne
      let is_airborne = ctx
        .world
        .get_component::<Player>(entity)
        .map(|p| p.leap_state.is_airborne())
        .unwrap_or(false);

      // Grounded players: verify they still have platform support.
      // Knockback or moving platforms can push a player off an edge while
      // they remain flagged as grounded, causing them to float.
      if !is_airborne {
        if let Some(transform) = ctx.world.get_component::<Transform>(entity) {
          let player_left = transform.x;
          let player_right = transform.x + PLAYER_WIDTH;
          let player_bottom = transform.y + PLAYER_HEIGHT;

          let mut supported = false;
          for &plat_entity in &self.platforms {
            if let Some(pt) = ctx.world.get_component::<Transform>(plat_entity)
              && let Some(plat) = ctx.world.get_component::<Platform>(plat_entity)
            {
              let plat_left = pt.x;
              let plat_right = pt.x + plat.width;
              let plat_top = pt.y;

              if player_right > plat_left
                && player_left < plat_right
                && (player_bottom - plat_top).abs() < 5.0
              {
                supported = true;
                break;
              }
            }
          }

          if !supported {
            if let Some(mut player) = ctx.world.get_component_mut::<Player>(entity) {
              player.grounded = false;
              player.leap_state = LeapState::Airborne;
            }
            // Don't skip — fall through to airborne physics below
          } else {
            continue;
          }
        } else {
          continue;
        }
      }

      // Apply gravity
      if let Some(mut vel) = ctx.world.get_component_mut::<Velocity>(entity) {
        vel.dy += GRAVITY * dt;
      }

      // Get velocity
      let vel = ctx
        .world
        .get_component::<Velocity>(entity)
        .map(|v| (v.dx, v.dy));

      if let Some((vx, vy)) = vel {
        // Store previous Y for sweep collision
        let prev_y = ctx
          .world
          .get_component::<Transform>(entity)
          .map(|t| t.y)
          .unwrap_or(0.0);

        // Update position
        let mut wall_hit = false;
        if let Some(mut transform) = ctx.world.get_component_mut::<Transform>(entity) {
          transform.x += vx * dt;
          transform.y += vy * dt;

          // Check wall collision
          if transform.x < 0.0 {
            transform.x = 0.0;
            wall_hit = true;
          } else if transform.x > self.screen_width - PLAYER_WIDTH {
            transform.x = self.screen_width - PLAYER_WIDTH;
            wall_hit = true;
          }
        }

        // Bounce off walls — reflect horizontal velocity so player doesn't slide down
        if wall_hit && let Some(mut vel) = ctx.world.get_component_mut::<Velocity>(entity) {
          vel.dx = -vel.dx * WALL_BOUNCE_FACTOR;
        }

        // Check platform collisions (only when falling)
        if vy > 0.0 {
          self.check_platform_landing(ctx, entity, prev_y);
        }
      }
    }
  }

  fn check_platform_landing(&mut self, ctx: &mut SceneContext, player_entity: Entity, prev_y: f32) {
    let player_transform = match ctx.world.get_component::<Transform>(player_entity) {
      Some(t) => (t.x, t.y),
      None => return,
    };

    // Current and previous bottom positions for sweep test
    let current_bottom = player_transform.1 + PLAYER_HEIGHT;
    let prev_bottom = prev_y + PLAYER_HEIGHT;
    let player_left = player_transform.0;
    let player_right = player_transform.0 + PLAYER_WIDTH;

    // Find the highest platform we should land on (in case we passed through multiple)
    let mut best_landing: Option<(f32, Entity)> = None;

    for &platform_entity in &self.platforms {
      let platform = match ctx.world.get_component::<Platform>(platform_entity) {
        Some(p) => *p,
        None => continue,
      };

      let plat_transform = match ctx.world.get_component::<Transform>(platform_entity) {
        Some(t) => (t.x, t.y),
        None => continue,
      };

      let plat_top = plat_transform.1;
      let plat_left = plat_transform.0;
      let plat_right = plat_transform.0 + platform.width;

      // Check horizontal overlap
      let horizontally_overlapping = player_right > plat_left && player_left < plat_right;
      if !horizontally_overlapping {
        continue;
      }

      // Sweep test: check if player's bottom crossed the platform top
      // Player was above platform and is now at or below it
      let was_above = prev_bottom <= plat_top + 1.0; // Small tolerance
      let now_below_or_at = current_bottom >= plat_top;

      if was_above && now_below_or_at {
        // Found a valid landing - pick the highest one (smallest Y)
        match best_landing {
          None => best_landing = Some((plat_top, platform_entity)),
          Some((best_y, _)) if plat_top < best_y => {
            best_landing = Some((plat_top, platform_entity));
          }
          _ => {}
        }
      }
    }

    // Apply the landing if we found one
    if let Some((plat_top, _)) = best_landing {
      if let Some(mut transform) = ctx.world.get_component_mut::<Transform>(player_entity) {
        transform.y = plat_top - PLAYER_HEIGHT;
      }

      if let Some(mut vel) = ctx.world.get_component_mut::<Velocity>(player_entity) {
        vel.dx = 0.0;
        vel.dy = 0.0;
      }

      if let Some(mut player) = ctx.world.get_component_mut::<Player>(player_entity) {
        player.grounded = true;
        player.leap_state = LeapState::Landing {
          timer: LANDING_RECOVERY_TIME,
        };
      }

      // Play land sound
      if let Some(audio) = &mut ctx.audio {
        let _ = audio.play_sound_with("land", SoundOptions::new().volume(0.05));
      }
    }
  }

  /// Check for player-to-player collisions
  /// - Subtle horizontal push when players overlap
  /// - Head bounce when landing on another player
  fn check_player_collisions(&mut self, ctx: &mut SceneContext) {
    // Only check in multiplayer with 2+ players
    if self.players.len() < 2 {
      return;
    }

    // Collect player data to avoid borrow issues
    let player_data: Vec<(Entity, AABB, f32, bool)> = self
      .players
      .values()
      .filter_map(|&entity| {
        let transform = ctx.world.get_component::<Transform>(entity)?;
        let collider = ctx.world.get_component::<Collider>(entity)?;
        let vel = ctx.world.get_component::<Velocity>(entity)?;
        let player = ctx.world.get_component::<Player>(entity)?;
        let aabb = collider.world_aabb(&transform);
        Some((entity, aabb, vel.dy, player.leap_state.is_airborne()))
      })
      .collect();

    // Check each pair of players
    for i in 0..player_data.len() {
      for j in (i + 1)..player_data.len() {
        let (entity_a, ref a_bounds, avy, a_airborne) = player_data[i];
        let (entity_b, ref b_bounds, bvy, b_airborne) = player_data[j];

        let a_left = a_bounds.min_x;
        let a_right = a_bounds.max_x;
        let a_top = a_bounds.min_y;
        let a_bottom = a_bounds.max_y;

        let b_left = b_bounds.min_x;
        let b_right = b_bounds.max_x;
        let b_top = b_bounds.min_y;
        let b_bottom = b_bounds.max_y;

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
          if let Some(mut vel) = ctx.world.get_component_mut::<Velocity>(entity_a) {
            vel.dy = HEAD_BOUNCE_VELOCITY;
          }
          // Play a subtle sound
          if let Some(audio) = &mut ctx.audio {
            let _ = audio.play_sound_with("land", SoundOptions::new().volume(0.03));
          }
          continue; // Don't apply push collision for head bounce
        }

        // Player B landing on A's head
        if b_airborne && bvy > 0.0 && b_bottom <= a_top + HEAD_BOUNCE_ZONE && b_bottom >= a_top {
          // B is landing on A's head - bounce B up
          if let Some(mut vel) = ctx.world.get_component_mut::<Velocity>(entity_b) {
            vel.dy = HEAD_BOUNCE_VELOCITY;
          }
          if let Some(audio) = &mut ctx.audio {
            let _ = audio.play_sound_with("land", SoundOptions::new().volume(0.03));
          }
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

          if let Some(mut vel) = ctx.world.get_component_mut::<Velocity>(entity_a) {
            vel.dx = AIR_KNOCKBACK_HORIZONTAL * push_dir;
            vel.dy = vel.dy.min(AIR_KNOCKBACK_VERTICAL);
          }
          if let Some(mut vel) = ctx.world.get_component_mut::<Velocity>(entity_b) {
            vel.dx = AIR_KNOCKBACK_HORIZONTAL * -push_dir;
            vel.dy = vel.dy.min(AIR_KNOCKBACK_VERTICAL);
          }
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

          // Apply knockback to player A
          let has_kb_a = ctx.world.get_component::<Knockback>(entity_a).is_some();
          if has_kb_a {
            if let Some(mut kb) = ctx.world.get_component_mut::<Knockback>(entity_a) {
              kb.apply(dir_a, PLAYER_PUSH_FORCE);
            }
          } else {
            let _ = ctx.world.insert_one(
              entity_a,
              Knockback::new(dir_a, PLAYER_PUSH_FORCE).with_decay_rate(PLAYER_PUSH_DECAY),
            );
          }

          // Apply knockback to player B
          let has_kb_b = ctx.world.get_component::<Knockback>(entity_b).is_some();
          if has_kb_b {
            if let Some(mut kb) = ctx.world.get_component_mut::<Knockback>(entity_b) {
              kb.apply(dir_b, PLAYER_PUSH_FORCE);
            }
          } else {
            let _ = ctx.world.insert_one(
              entity_b,
              Knockback::new(dir_b, PLAYER_PUSH_FORCE).with_decay_rate(PLAYER_PUSH_DECAY),
            );
          }
        }
      }
    }
  }

  fn generate_platforms_as_needed(&mut self, ctx: &mut SceneContext) {
    // Find highest player Y
    let mut highest_y = self.highest_player_y;
    for &entity in self.players.values() {
      if let Some(transform) = ctx.world.get_component::<Transform>(entity)
        && transform.y < highest_y
      {
        highest_y = transform.y;
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
            pdata.platform_index,
          );
        }
      }
    }
  }

  fn update_camera(&mut self, ctx: &mut SceneContext) {
    // Follow local player - camera looks ABOVE the player so they can see where to jump
    let player_y = match self.get_local_player_entity() {
      Some(entity) => ctx.world.get_component::<Transform>(entity).map(|t| t.y),
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
      .camera
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
    self.particle_system.update(ctx.world, ctx.events, dt);
  }

  fn update_host(&mut self, ctx: &mut SceneContext, dt: f32) {
    self.elapsed_time += dt;

    // Update notifications
    self.update_notifications(dt);

    // Check for death and handle respawn
    self.check_death_and_respawn(ctx, dt);

    // Update height tracking and checkpoints
    self.update_height_and_checkpoints(ctx);

    let server = self.server.as_mut().unwrap();
    server.update(Duration::from_secs_f32(dt));

    // Process events
    let events: Vec<_> = server.events().collect();
    for event in events {
      match event {
        NetworkEvent::ClientConnected { client_id } => {
          // Generate new session ID for this client
          let session_id: u64 = rand::random();
          self.client_to_session.insert(client_id, session_id);

          // Spawn player B
          self.spawn_player(ctx, 1, client_id);

          // Send game init with seed, session, and server-assigned client ID
          let init_rpc = GameInitRpc {
            seed: self.seed,
            session_id,
            client_id,
            checkpoint_x: None,
            checkpoint_y: None,
            checkpoint_height: None,
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
            message: "Player 2 connected".to_string(),
            timer: NOTIFICATION_DURATION,
            is_connect: true,
          });
        }
        NetworkEvent::ClientDisconnected { client_id, .. } => {
          // Save session data before removing player
          if let Some(session_id) = self.client_to_session.remove(&client_id) {
            // Get remote player's best height
            let best_height = self
              .remote_best_heights
              .get(&client_id)
              .copied()
              .unwrap_or(0.0);
            if best_height > 0.0 {
              // Calculate checkpoint position from height
              // Height is (PLAYER_SPAWN_Y - y) / HEIGHT_SCALE, so y = PLAYER_SPAWN_Y - height * HEIGHT_SCALE
              let checkpoint_y = PLAYER_SPAWN_Y - best_height * HEIGHT_SCALE;
              self.client_sessions.insert(
                session_id,
                SessionData {
                  checkpoint_x: self.checkpoint_center_x,
                  checkpoint_y: checkpoint_y - PLAYER_HEIGHT,
                  checkpoint_height: best_height,
                  max_height: best_height,
                },
              );
            }
          }

          if let Some(entity) = self.players.remove(&client_id) {
            let _ = ctx.world.despawn(entity);
          }
          if let Some(light_entity) = self.player_lights.remove(&client_id) {
            let _ = ctx.world.despawn(light_entity);
          }
          self.remote_best_heights.remove(&client_id);

          // Show disconnection notification
          self.notifications.push(ConnectionNotification {
            message: "Player 2 disconnected".to_string(),
            timer: NOTIFICATION_DURATION,
            is_connect: false,
          });
        }
        _ => {}
      }
    }

    // Receive input from clients - collect inputs first to avoid borrow issues
    let mut pending_inputs: Vec<(Entity, LeapInput)> = Vec::new();
    let mut pending_respawns: Vec<Entity> = Vec::new();
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
                pending_respawns.push(entity);
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

        // Send updated init with checkpoint data
        let init_rpc = GameInitRpc {
          seed: self.seed,
          session_id,
          client_id,
          checkpoint_x: Some(session_data.checkpoint_x),
          checkpoint_y: Some(session_data.checkpoint_y),
          checkpoint_height: Some(session_data.checkpoint_height),
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
          message: "Player 2 reconnected".to_string(),
          timer: NOTIFICATION_DURATION,
          is_connect: true,
        });
      }
    }

    for (entity, input) in pending_inputs {
      self.apply_input(ctx, entity, &input, dt);
    }
    for entity in pending_respawns {
      self.respawn_player(ctx, entity);
    }

    // Handle local input
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
    self.particle_system.update(ctx.world, ctx.events, dt);

    // Track remote players' best heights on the host side
    let local_id = self.local_client_id;
    for (&client_id, &entity) in &self.players {
      if Some(client_id) == local_id {
        continue;
      }
      if let Some(transform) = ctx.world.get_component::<Transform>(entity)
        && let Some(player) = ctx.world.get_component::<Player>(entity)
      {
        let height = ((PLAYER_SPAWN_Y - transform.y) / HEIGHT_SCALE).max(0.0);
        if player.grounded {
          let best = self.remote_best_heights.entry(client_id).or_insert(0.0);
          if height > *best {
            *best = height;
          }
        }
      }
    }

    // Broadcast positions
    self.broadcast_timer += dt;
    if self.broadcast_timer >= POSITION_BROADCAST_RATE {
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
              checkpoint_x: None,
              checkpoint_y: None,
              checkpoint_height: None,
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

      // Save session ID for future reconnects
      if init.session_id != 0 {
        self.session_id = init.session_id;
        save_session_id(&self.room_code, init.session_id);
      }

      // Restore checkpoint if provided (reconnect case)
      if let (Some(cx), Some(cy), Some(ch)) =
        (init.checkpoint_x, init.checkpoint_y, init.checkpoint_height)
      {
        self.checkpoint_center_x = cx;
        self.checkpoint_y = cy;
        self.last_checkpoint_height = ch;
        // Trigger a respawn to checkpoint position after player spawns
        self.respawn_timer = Some(0.1);
      }
    }

    // Process position updates using PredictionHelper
    for update in position_updates {
      // Sync elapsed time from host for deterministic moving platform positions
      // Use max() to prevent backward jumps from out-of-order packets
      if update.host_elapsed_time > 0.0 {
        self.elapsed_time = self.elapsed_time.max(update.host_elapsed_time);
      }

      // Sync checkpoint state from host (every update carries it)
      self.checkpoint_center_x = update.checkpoint_center_x;
      self.checkpoint_platform_width = update.checkpoint_platform_width;
      self.checkpoint_y = update.checkpoint_y;

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
          if let Some(mut vel) = ctx.world.get_component_mut::<Velocity>(entity) {
            vel.dx = update.vx;
            vel.dy = update.vy;
          }
          if let Some(mut player) = ctx.world.get_component_mut::<Player>(entity) {
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
            .and_then(|e| ctx.world.get_component::<Transform>(e))
            .map(|t| (t.x, t.y));

          // Detect jump (transition to Airborne)
          if !matches!(prev_state, NetworkLeapState::Airborne)
            && matches!(new_state, NetworkLeapState::Airborne)
            && let Some(audio) = &mut ctx.audio
          {
            let options = if let Some((lx, ly)) = local_pos {
              spatial_sound_options(lx, ly, update.x, update.y).volume(0.05)
            } else {
              SoundOptions::new().volume(0.05)
            };
            let _ = audio.play_sound_with("jump", options);
          }

          // Detect land (transition to Landing)
          if !matches!(prev_state, NetworkLeapState::Landing { .. })
            && matches!(new_state, NetworkLeapState::Landing { .. })
            && let Some(audio) = &mut ctx.audio
          {
            let options = if let Some((lx, ly)) = local_pos {
              spatial_sound_options(lx, ly, update.x, update.y).volume(0.05)
            } else {
              SoundOptions::new().volume(0.05)
            };
            let _ = audio.play_sound_with("land", options);
          }

          // Store current state for next comparison
          self
            .previous_leap_states
            .insert(update.client_id, new_state);

          // Track remote player's best height
          self
            .remote_best_heights
            .insert(update.client_id, update.best_height);
        }
        UpdateResult::StoredForReconciliation => {
          // Local player - we're predicting, don't apply server position
          // (reconciliation would happen here if we had input buffering)
        }
        UpdateResult::Skipped => {}
      }
    }

    // Sync checkpoint visual after processing all position updates
    self.sync_checkpoint_visual(ctx);

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
        .get_component::<Player>(entity)
        .is_some_and(|p| p.leap_state.is_airborne())
        && let Some(mut vel) = ctx.world.get_component_mut::<Velocity>(entity)
      {
        vel.dy += GRAVITY * dt;
      }
    }
    self.prediction.dead_reckon_remotes(ctx.world, dt);

    // Advance elapsed_time AFTER network sync so host's authoritative time is used
    self.elapsed_time += dt;

    // Run local physics for smooth movement (client-side prediction)
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
    self.particle_system.update(ctx.world, ctx.events, dt);
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
      if let Some(transform) = ctx.world.get_component::<Transform>(entity)
        && let Some(vel) = ctx.world.get_component::<Velocity>(entity)
        && let Some(player) = ctx.world.get_component::<Player>(entity)
      {
        // Use local max_height for local player, remote_best_heights for others
        let best_height = if Some(client_id) == local_id {
          self.max_height
        } else {
          *self.remote_best_heights.get(&client_id).unwrap_or(&0.0)
        };

        updates.push(PositionUpdate {
          client_id,
          slot: player.slot,
          x: transform.x,
          y: transform.y,
          vx: vel.dx,
          vy: vel.dy,
          leap_state: player.leap_state.into(),
          facing_right: player.facing == Facing::Right,
          grounded: player.grounded,
          best_height,
          checkpoint_center_x: self.checkpoint_center_x,
          checkpoint_platform_width: self.checkpoint_platform_width,
          checkpoint_y: self.checkpoint_y,
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

  fn update_charge_indicator(&mut self, ctx: &SceneContext) {
    self.charge_indicator.visible = false;

    let entity = match self.get_local_player_entity() {
      Some(e) => e,
      None => return,
    };

    let player = match ctx.world.get_component::<Player>(entity) {
      Some(p) => p,
      None => return,
    };

    if !player.leap_state.is_charging() {
      return;
    }

    let transform = match ctx.world.get_component::<Transform>(entity) {
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
    let world_x = transform.x + PLAYER_WIDTH / 2.0;
    let world_y = transform.y + PLAYER_HEIGHT / 2.0;
    let (sx, sy) = ctx.camera.world_to_screen(world_x, world_y);
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

  fn on_enter(&mut self, ctx: &mut SceneContext) {
    self.screen_width = ctx.screen_width();
    self.screen_height = ctx.screen_height();
    self.generator = PlatformGenerator::new(self.seed, self.screen_width);
    self.checkpoint_center_x = self.screen_width / 2.0;

    ctx
      .camera
      .set_position(self.screen_width / 2.0, self.screen_height / 2.0);

    // Set up atmospheric lighting
    if let Some(lighting) = ctx.lighting {
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
      // Keep UI readable in darkness
      lighting.set_ui_affected_by_lighting(false);
    }

    // Enable ambient occlusion for corner darkening
    ctx.effects.set_ambient_occlusion(0.35, 6.0);

    // Spawn rain particles
    self.spawn_rain(ctx);

    // Rain audio is already playing from menu scene (persists between scenes)

    // Register console command handler (auto-cleaned on scene exit)
    ctx.register_console_command("game:respawn", |_args, cmd_ctx| {
      CONSOLE_RESPAWN_REQUESTED.store(true, Ordering::SeqCst);
      cmd_ctx.console.print_system("Respawning at checkpoint...");
      CommandResult::Ok
    });

    // Load jump and land sound effects
    if let Some(audio) = &mut ctx.audio {
      let _ = audio.load_sound("jump", "assets/jump.mp3");
      let _ = audio.load_sound("land", "assets/land.wav");
      self.sounds_loaded = true;
    }

    // Spawn spawn platform
    self.spawn_starting_platform(ctx);

    // Generate initial platforms above spawn (skip for client — wait for seed from host)
    if self.mode != GameMode::Client {
      self.generate_platforms_as_needed(ctx);
    }

    // Spawn local player
    match self.mode {
      GameMode::SinglePlayer => {
        let id = self.local_client_id.unwrap();
        self.spawn_player(ctx, 0, id);
      }
      GameMode::Host => {
        let id = self.local_client_id.unwrap();
        self.spawn_player(ctx, 0, id);
      }
      GameMode::Client => {
        // Wait for position updates from server
      }
    }
  }

  fn update(&mut self, ctx: &mut SceneContext, dt: f32) -> SceneAction {
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

  fn render_ui(&mut self, ui: &mut UiContext) {
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

      let right_y = 40.0;

      if self.mode == GameMode::Host {
        if let Some(ref signal) = self.signal_url {
          let signal_text = format!("Signal: {}", signal);
          let (tw, _) = ui.text_size(&signal_text);
          ui.draw_text(
            &signal_text,
            sw as f32 - tw as f32 - 20.0,
            right_y,
            Color::new(100, 180, 255, 150),
          );
        } else if let Some(ref lan) = self.lan_ip {
          let lan_text = format!("{}:{}", lan, DEFAULT_PORT);
          let (tw, _) = ui.text_size(&lan_text);
          ui.draw_text(
            &lan_text,
            sw as f32 - tw as f32 - 20.0,
            right_y,
            Color::new(100, 200, 100, 150),
          );
        }
      }

      // Ping display for client (under room code)
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

    // Show rival's best height below local best - multiplayer only
    if self.mode != GameMode::SinglePlayer {
      let local_id = self.local_client_id;
      for (client_id, &best_height) in &self.remote_best_heights {
        if Some(*client_id) != local_id && best_height > 0.0 {
          let rival_text = format!("Rival: {:.0}m", best_height);
          ui.draw_text(
            &rival_text,
            20.0,
            70.0,
            Color::rgb(PLAYER_B_COLOR.0, PLAYER_B_COLOR.1, PLAYER_B_COLOR.2),
          );
          break;
        }
      }
    }

    // Checkpoint indicator (shift down if rival text is showing)
    if self.last_checkpoint_height > 0.0 {
      let checkpoint_y = if self.mode != GameMode::SinglePlayer {
        95.0
      } else {
        70.0
      };
      let checkpoint_text = format!("Checkpoint: {:.0}m", self.last_checkpoint_height);
      ui.draw_text(
        &checkpoint_text,
        20.0,
        checkpoint_y,
        Color::rgb(
          CHECKPOINT_PLATFORM_COLOR.0,
          CHECKPOINT_PLATFORM_COLOR.1,
          CHECKPOINT_PLATFORM_COLOR.2,
        ),
      );
    }

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
      let highlight = Color::rgb(PLAYER_A_COLOR.0, PLAYER_A_COLOR.1, PLAYER_A_COLOR.2);

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
  }

  fn render_gpu(&mut self, ctx: &mut GpuRenderContext) {
    // Upload particle sprite textures
    if let Some(ref sprite) = self.rain_sprite {
      ctx.upload_sprite(sprite);
    }
    if let Some(ref sprite) = self.dust_sprite {
      ctx.upload_sprite(sprite);
    }

    // Collect particles for rendering
    self
      .particle_system
      .collect_for_render(ctx.world, ctx.camera, ctx.batch);
  }
}
