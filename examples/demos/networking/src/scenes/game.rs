//! The match: two players on a platform, host-authoritative

use std::collections::HashMap;
use std::net::{SocketAddr, UdpSocket};
use std::time::Duration;

use demo_kit::prelude::*;
use ryo2d::core::network::NetworkError;

use crate::components::*;
use crate::constants::*;

/// Get the local LAN IP address by connecting to a remote address.
/// This doesn't actually send data, just determines which interface would be used.
fn get_lan_ip() -> Option<String> {
  // Connect to Google's DNS to determine local interface IP
  let socket = UdpSocket::bind("0.0.0.0:0").ok()?;
  socket.connect("8.8.8.8:80").ok()?;
  let local_addr = socket.local_addr().ok()?;
  Some(local_addr.ip().to_string())
}

/// Get the public WAN IP address using an external service.
/// This is done synchronously for simplicity - in production you'd want async.
fn get_wan_ip() -> Option<String> {
  // Use a simple HTTP request to get public IP
  // This blocks briefly but only happens once when hosting
  let response = std::process::Command::new("curl")
    .args(["-s", "--max-time", "2", "https://api.ipify.org"])
    .output()
    .ok()?;

  if response.status.success() {
    let ip = String::from_utf8(response.stdout).ok()?;
    let ip = ip.trim().to_string();
    // Basic validation - should look like an IP
    if ip.contains('.') && ip.len() <= 15 {
      return Some(ip);
    }
  }
  None
}

/// Network role
#[derive(Debug, Clone, Copy, PartialEq)]
enum Role {
  Host,
  Client,
}

/// Pending input from a remote player
struct PendingInput {
  entity: Entity,
  move_x: f32,
  jump: bool,
}

/// A running match, as host or client
pub struct Match {
  role: Role,
  room_code: String,

  // Networking
  server: Option<ListenServer>,
  client: Option<NetworkClient>,

  // Host IP addresses (only populated for host)
  lan_ip: Option<String>,
  wan_ip: Option<String>,

  // Entity tracking
  players: HashMap<ClientId, Entity>,
  /// Background and platform, despawned with the players on leaving
  scenery: Vec<Entity>,
  local_client_id: Option<ClientId>,

  // Local player input
  local_move_x: f32,
  local_jump: bool,

  // Connection state
  connected: bool,
  waiting_for_player: bool,

  // Set by the leave key; the page then drops the match
  leave_requested: bool,
}

impl Match {
  /// Create a new host (server) game
  pub fn new_host(room_code: String) -> Result<Self, NetworkError> {
    let server = ListenServer::new(NetworkServerConfig {
      port: DEFAULT_PORT,
      max_clients: 2,
      protocol_id: PROTOCOL_ID,
      ..Default::default()
    })?;

    let host_id = server.host_id();

    // Detect LAN and WAN IPs for display
    let lan_ip = get_lan_ip();
    let wan_ip = get_wan_ip();

    Ok(Self {
      role: Role::Host,
      room_code,
      server: Some(server),
      client: None,
      lan_ip,
      wan_ip,
      players: HashMap::new(),
      scenery: Vec::new(),
      local_client_id: Some(host_id),
      local_move_x: 0.0,
      local_jump: false,
      connected: true,
      waiting_for_player: true,
      leave_requested: false,
    })
  }

  /// Create a new client game
  pub fn new_client(room_code: &str, address: &str) -> Result<Self, NetworkError> {
    // Parse the address (expected format: "ip:port" or just "ip" for default port)
    let server_addr: SocketAddr = if address.contains(':') {
      address
        .parse()
        .map_err(|_| NetworkError::ConnectionError(format!("Invalid address: {}", address)))?
    } else {
      format!("{}:{}", address, DEFAULT_PORT)
        .parse()
        .map_err(|_| NetworkError::ConnectionError(format!("Invalid address: {}", address)))?
    };

    let client = NetworkClient::connect(NetworkClientConfig::udp(server_addr, PROTOCOL_ID))?;

    Ok(Self {
      role: Role::Client,
      room_code: room_code.to_string(),
      server: None,
      client: Some(client),
      lan_ip: None,
      wan_ip: None,
      players: HashMap::new(),
      scenery: Vec::new(),
      local_client_id: None,
      local_move_x: 0.0,
      local_jump: false,
      connected: false,
      waiting_for_player: true,
      leave_requested: false,
    })
  }

  fn spawn_background(&mut self, ctx: &mut SceneContext) {
    let background = ctx
      .world
      .spawn((
        Transform::from_xy(0.0, 0.0),
        Renderable::sprite(SpriteHandle::from_sprite(
          Sprite::solid_rect(
            SCREEN_WIDTH,
            SCREEN_HEIGHT,
            BACKGROUND_COLOR.0,
            BACKGROUND_COLOR.1,
            BACKGROUND_COLOR.2,
            255,
          ),
          "background",
        ))
        .with_anchor(Anchor::TopLeft)
        .with_z_order(Z_BACKGROUND),
      ))
      .id();
    self.scenery.push(background);
  }

  fn spawn_platform(&mut self, ctx: &mut SceneContext) {
    let platform_x = (SCREEN_WIDTH as f32 - PLATFORM_WIDTH) / 2.0;

    let platform = ctx
      .world
      .spawn((
        Transform::from_xy(platform_x, PLATFORM_Y),
        Renderable::sprite(SpriteHandle::from_sprite(
          Sprite::solid_rect(
            PLATFORM_WIDTH as u32,
            PLATFORM_HEIGHT as u32,
            PLATFORM_COLOR.0,
            PLATFORM_COLOR.1,
            PLATFORM_COLOR.2,
            255,
          ),
          "platform",
        ))
        .with_anchor(Anchor::TopLeft)
        .with_z_order(Z_PLATFORM),
        // One-way, so players land on it only from above
        Collider::rect(PLATFORM_WIDTH, PLATFORM_HEIGHT)
          .with_offset(PLATFORM_WIDTH / 2.0, PLATFORM_HEIGHT / 2.0)
          .one_way(),
      ))
      .id();
    self.scenery.push(platform);
  }

  /// Walls just off either side of the screen keep the players on it
  fn spawn_walls(&mut self, ctx: &mut SceneContext) {
    let height = SCREEN_HEIGHT as f32 * 3.0;
    for x in [
      -WALL_THICKNESS / 2.0,
      SCREEN_WIDTH as f32 + WALL_THICKNESS / 2.0,
    ] {
      let wall = ctx
        .world
        .spawn((
          Transform::from_xy(x, SCREEN_HEIGHT as f32 / 2.0),
          Collider::rect(WALL_THICKNESS, height),
        ))
        .id();
      self.scenery.push(wall);
    }
  }

  fn spawn_player(&mut self, ctx: &mut SceneContext, slot: u8, client_id: ClientId) -> Entity {
    let (spawn_x, color) = if slot == 0 {
      (PLAYER_A_SPAWN_X, PLAYER_A_COLOR)
    } else {
      (PLAYER_B_SPAWN_X, PLAYER_B_COLOR)
    };

    // Positions are top-left corners: the collision rules below work in them.
    // Players are on their own layer and collide only with the world: they
    // pass through each other, and `check_player_collision` pushes them apart
    let entity = ctx
      .world
      .spawn((
        Transform::from_xy(spawn_x, PLAYER_SPAWN_Y),
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
        .with_anchor(Anchor::TopLeft)
        .with_z_order(Z_PLAYER),
        Collider::rect(PLAYER_WIDTH, PLAYER_HEIGHT)
          .with_offset(PLAYER_WIDTH / 2.0, PLAYER_HEIGHT / 2.0)
          .with_layer(PLAYER_LAYER)
          .with_mask(WORLD_LAYER),
        Player::new(slot),
        Velocity::zero(),
      ))
      .id();
    // The host simulates both players: the engine's character controller
    // moves them by their `Velocity`. A client only shows the host's
    // positions, so its players stay bare colliders.
    if self.role == Role::Host {
      ctx
        .world
        .entity_mut(entity)
        .insert(CharacterController::platformer());
    }

    self.players.insert(client_id, entity);
    entity
  }

  fn handle_input(&mut self, ctx: &mut SceneContext) {
    // Horizontal movement
    self.local_move_x = 0.0;
    if ctx.input.key_down(KeyCode::KeyA) {
      self.local_move_x -= 1.0;
    }
    if ctx.input.key_down(KeyCode::KeyD) {
      self.local_move_x += 1.0;
    }

    // Jump
    self.local_jump = ctx.input.key_pressed(KeyCode::Space) || ctx.input.key_pressed(KeyCode::KeyW);
  }

  /// True once the player pressed the leave key
  pub fn leave_requested(&self) -> bool {
    self.leave_requested
  }

  /// Disconnect from the host; a hosted server stops when dropped
  pub fn disconnect(&mut self) {
    if let Some(client) = self.client.as_mut() {
      client.disconnect();
    }
  }

  /// Despawn everything the match spawned
  pub fn despawn(&mut self, ctx: &mut SceneContext) {
    for entity in self.scenery.drain(..) {
      ctx.world.despawn(entity);
    }
    for (_, entity) in self.players.drain() {
      ctx.world.despawn(entity);
    }
  }

  fn update_host(&mut self, ctx: &mut SceneContext, dt: f32) {
    let server = self.server.as_mut().unwrap();
    server.update(Duration::from_secs_f32(dt));

    // Collect events first to avoid borrow issues
    let events: Vec<_> = server.events().collect();

    for event in events {
      match event {
        NetworkEvent::ClientConnected { client_id } => {
          // Remote client joined - spawn player B
          self.spawn_player(ctx, 1, client_id);
          self.waiting_for_player = false;
        }
        NetworkEvent::ClientDisconnected { client_id, .. } => {
          // Remove disconnected player
          if let Some(entity) = self.players.remove(&client_id) {
            ctx.world.despawn(entity);
          }
          self.waiting_for_player = true;
        }
        _ => {}
      }
    }

    // Collect pending inputs from remote clients
    let mut pending_inputs: Vec<PendingInput> = Vec::new();

    let server = self.server.as_mut().unwrap();
    for client_id in server.remote_clients().collect::<Vec<_>>() {
      while let Some(data) = server.receive(client_id, Channel::ReliableOrdered) {
        if let Ok(serialized) = SerializedRpc::from_bytes(&data)
          && let Ok(rpc) = serialized.into_rpc::<PlayerInputRpc>()
          && let Some(&entity) = self.players.get(&client_id)
        {
          pending_inputs.push(PendingInput {
            entity,
            move_x: rpc.payload.move_x,
            jump: rpc.payload.jump,
          });
        }
      }
    }

    // Apply collected remote inputs
    for input in pending_inputs {
      self.apply_input(ctx, input.entity, input.move_x, input.jump);
    }

    // Apply local (host) input
    let host_id = self.server.as_ref().unwrap().host_id();
    if let Some(&entity) = self.players.get(&host_id) {
      self.apply_input(ctx, entity, self.local_move_x, self.local_jump);
    }

    // Update physics for all players
    self.update_physics(ctx, dt);

    // Broadcast player positions to clients
    self.broadcast_positions(ctx);
  }

  fn update_client(&mut self, ctx: &mut SceneContext, dt: f32) {
    let client = self.client.as_mut().unwrap();
    client.update(Duration::from_secs_f32(dt));

    // Check connection status
    if !self.connected && client.is_connected() {
      self.connected = true;
      self.local_client_id = Some(client.client_id());
    }

    // Process connection events
    let events: Vec<_> = client.events().collect();
    for event in events {
      match event {
        NetworkEvent::Connected => {
          self.connected = true;
          self.local_client_id = Some(self.client.as_ref().unwrap().client_id());
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

    // Send input to server
    let client = self.client.as_mut().unwrap();
    let input_rpc = PlayerInputRpc {
      move_x: self.local_move_x,
      jump: self.local_jump,
    };
    if let Ok(serialized) =
      SerializedRpc::from_rpc(&input_rpc, client.client_id(), RpcTarget::Server)
      && let Ok(bytes) = serialized.to_bytes()
    {
      client.send(Channel::ReliableOrdered, &bytes);
    }

    // Collect position updates from server
    let mut updates: Vec<PositionUpdate> = Vec::new();
    let client = self.client.as_mut().unwrap();
    while let Some(data) = client.receive(Channel::Unreliable) {
      if let Ok(update) = bincode_deserialize::<PositionUpdate>(&data) {
        updates.push(update);
      }
    }

    // Process collected updates
    for update in updates {
      // Find or spawn player
      let entity = if let Some(&e) = self.players.get(&update.client_id) {
        e
      } else {
        // Spawn new player
        self.spawn_player(ctx, update.slot, update.client_id)
      };

      // Update position
      if let Some(mut transform) = ctx.world.get_mut::<Transform>(entity) {
        transform.translation = Vec2::new(update.x, update.y);
      }
    }

    self.waiting_for_player = self.players.len() < 2;
  }

  /// Record a player's input; `update_physics` acts on it.
  fn apply_input(&mut self, ctx: &mut SceneContext, entity: Entity, move_x: f32, jump: bool) {
    if let Some(mut player) = ctx.world.get_mut::<Player>(entity) {
      player.move_x = move_x;
      player.jump |= jump;
    }
  }

  /// The host's platformer rules. The engine's character controller moves
  /// the players by their `Velocity` after `update` (in the fixed step, like
  /// this), stopping them on the platform and at the walls; this sets the
  /// run, jumps, adds gravity and settles overlapping players.
  fn update_physics(&mut self, ctx: &mut SceneContext, dt: f32) {
    let player_entities: Vec<Entity> = self.players.values().copied().collect();
    let mut jumped = Vec::new();

    for &entity in &player_entities {
      let Some(mut player) = ctx.world.get_mut::<Player>(entity) else {
        continue;
      };
      let jump = std::mem::take(&mut player.jump) && player.grounded;
      let move_x = player.move_x;
      if jump {
        jumped.push(entity);
      }
      if let Some(mut vel) = ctx.world.get_mut::<Velocity>(entity) {
        vel.dx = move_x * PLAYER_SPEED;
        if jump {
          vel.dy = -PLAYER_JUMP_FORCE;
        }
        vel.dy += GRAVITY * dt;
      }
    }

    // On the platform (as the last move found it) and not jumping off it
    for &entity in &player_entities {
      let on_floor = ctx
        .world
        .get::<CharacterState>(entity)
        .is_some_and(|state| state.on_floor);
      let falling = ctx
        .world
        .get::<Velocity>(entity)
        .is_some_and(|v| v.dy >= 0.0);
      if let Some(mut player) = ctx.world.get_mut::<Player>(entity) {
        player.grounded = on_floor && falling;
      }
    }

    // Player-player collision
    if let [a, b] = player_entities[..] {
      self.check_player_collision(ctx, a, b, &jumped, dt);
    }
  }

  fn broadcast_positions(&mut self, ctx: &mut SceneContext) {
    let server = match self.server.as_mut() {
      Some(s) => s,
      None => return,
    };

    // Collect position data first
    let mut updates: Vec<(ClientId, PositionUpdate)> = Vec::new();
    for (&client_id, &entity) in &self.players {
      if let Some(transform) = ctx.world.get::<Transform>(entity)
        && let Some(player) = ctx.world.get::<Player>(entity)
      {
        updates.push((
          client_id,
          PositionUpdate {
            client_id,
            slot: player.slot,
            x: transform.translation.x,
            y: transform.translation.y,
          },
        ));
      }
    }

    // Broadcast to all remote clients
    let remote_clients: Vec<_> = server.remote_clients().collect();
    for update in &updates {
      if let Ok(bytes) = bincode_serialize(&update.1) {
        for &remote_id in &remote_clients {
          server.send(remote_id, Channel::Unreliable, &bytes);
        }
      }
    }
  }

  /// Players pass through each other, so overlaps are settled here, through
  /// their velocities for this step (writing a character's `Transform`
  /// would teleport it, skipping its move): one on the other's head moves
  /// onto it and stands there unless it `jumped` this step, and side by side
  /// they are pushed apart.
  fn check_player_collision(
    &mut self,
    ctx: &mut SceneContext,
    a: Entity,
    b: Entity,
    jumped: &[Entity],
    dt: f32,
  ) {
    let (ax, ay) = {
      let t = ctx.world.get::<Transform>(a);
      match t {
        Some(t) => (t.translation.x, t.translation.y),
        None => return,
      }
    };

    let (bx, by) = {
      let t = ctx.world.get::<Transform>(b);
      match t {
        Some(t) => (t.translation.x, t.translation.y),
        None => return,
      }
    };

    // AABB collision; standing exactly on the other's head counts, so a
    // player resting there stays grounded
    let overlap_x = (ax < bx + PLAYER_WIDTH) && (ax + PLAYER_WIDTH > bx);
    let overlap_y = (ay < by + PLAYER_HEIGHT + HEAD_REST) && (ay + PLAYER_HEIGHT + HEAD_REST > by);

    if overlap_x && overlap_y {
      // Calculate overlap amounts
      let overlap_left = (ax + PLAYER_WIDTH) - bx;
      let overlap_right = (bx + PLAYER_WIDTH) - ax;
      let overlap_top = (ay + PLAYER_HEIGHT) - by;
      let overlap_bottom = (by + PLAYER_HEIGHT) - ay;

      let min_overlap_x = overlap_left.min(overlap_right);
      let min_overlap_y = overlap_top.min(overlap_bottom);

      if min_overlap_y < min_overlap_x {
        // Vertical collision (jumping on head): the one on top moves onto
        // the other's head this step and stands there
        let (top, overlap) = if ay < by {
          (a, overlap_top)
        } else {
          (b, overlap_bottom)
        };
        if !jumped.contains(&top) {
          if let Some(mut v) = ctx.world.get_mut::<Velocity>(top) {
            v.dy = -overlap / dt;
          }
          if let Some(mut p) = ctx.world.get_mut::<Player>(top) {
            p.grounded = true;
          }
        }
      } else {
        // Horizontal collision (push apart), each by `push` this step
        let push = (min_overlap_x / 2.0 + 1.0) / dt;
        let (left, right) = if ax < bx { (a, b) } else { (b, a) };
        if let Some(mut v) = ctx.world.get_mut::<Velocity>(left) {
          v.dx -= push;
        }
        if let Some(mut v) = ctx.world.get_mut::<Velocity>(right) {
          v.dx += push;
        }
      }
    }
  }
}

impl Match {
  /// Build the match's world: background, platform and the host's player
  pub fn enter(&mut self, ctx: &mut SceneContext) {
    ctx
      .camera()
      .set_position(SCREEN_WIDTH as f32 / 2.0, SCREEN_HEIGHT as f32 / 2.0);

    self.spawn_background(ctx);
    self.spawn_platform(ctx);
    self.spawn_walls(ctx);

    if self.role == Role::Host {
      let host_id = self.server.as_ref().unwrap().host_id();
      self.spawn_player(ctx, 0, host_id);
    }
  }

  pub fn update(&mut self, ctx: &mut SceneContext, dt: f32) {
    if ctx.input.key_pressed(KeyCode::KeyL) {
      self.leave_requested = true;
      return;
    }

    self.handle_input(ctx);

    match self.role {
      Role::Host => self.update_host(ctx, dt),
      Role::Client => self.update_client(ctx, dt),
    }
  }

  pub fn ui(&mut self, ui: &mut UiContext) {
    let area = content_area(ui);
    let right = area.x + area.width;

    // Room code (top right, faded)
    let code_text = format!("Room: {}", self.room_code);
    let (tw, _) = ui.text_size(&code_text);
    ui.draw_text(
      &code_text,
      right - tw as f32,
      area.y,
      Color::new(255, 255, 255, 100),
    );

    // Show host IP addresses below room code (host only)
    if self.role == Role::Host {
      let mut y_pos = area.y + 20.0;

      if let Some(ref lan) = self.lan_ip {
        let lan_text = format!("LAN: {}:{}", lan, DEFAULT_PORT);
        let (tw, _) = ui.text_size(&lan_text);
        ui.draw_text(
          &lan_text,
          right - tw as f32,
          y_pos,
          Color::new(100, 200, 100, 150), // Green tint
        );
        y_pos += 16.0;
      }

      if let Some(ref wan) = self.wan_ip {
        let wan_text = format!("WAN: {}:{}", wan, DEFAULT_PORT);
        let (tw, _) = ui.text_size(&wan_text);
        ui.draw_text(
          &wan_text,
          right - tw as f32,
          y_pos,
          Color::new(200, 150, 100, 150), // Orange tint
        );
      }
    }

    // Debug info panel (top left)
    let role_str = if self.role == Role::Host {
      "HOST"
    } else {
      "CLIENT"
    };
    let connected_str = if self.connected {
      "Connected"
    } else {
      "Disconnected"
    };
    let player_count = self.players.len();

    ui.draw_text(
      &format!("[{}] {}", role_str, connected_str),
      area.x,
      area.y,
      if self.role == Role::Host {
        Color::rgb(66, 135, 245)
      } else {
        Color::rgb(245, 66, 66)
      },
    );
    ui.draw_text(
      &format!("Players: {}/2", player_count),
      area.x,
      area.y + 20.0,
      Color::WHITE,
    );

    // Show player positions
    let mut y_offset = area.y + 40.0;
    for (&client_id, &entity) in &self.players {
      let pos_str = format!("  P{}: entity {:?}", client_id, entity);
      ui.draw_text(&pos_str, area.x, y_offset, Color::GRAY);
      y_offset += 20.0;
    }

    // Connection status (center)
    if self.role == Role::Client && !self.connected {
      ui.draw_text_anchored("Connecting...", Anchor::Center, 0.0, -50.0, Color::YELLOW);
    } else if self.waiting_for_player {
      ui.draw_text_anchored(
        "Waiting for player...",
        Anchor::Center,
        0.0,
        -50.0,
        Color::YELLOW,
      );
    }
  }
}

/// Position update sent from server to clients
#[derive(serde::Serialize, serde::Deserialize)]
struct PositionUpdate {
  client_id: ClientId,
  slot: u8,
  x: f32,
  y: f32,
}

/// Helper to serialize using bincode-style encoding
fn bincode_serialize<T: serde::Serialize>(value: &T) -> Result<Vec<u8>, NetworkError> {
  serde_json::to_vec(value).map_err(|e| NetworkError::SendError(e.to_string()))
}

/// Helper to deserialize using bincode-style encoding
fn bincode_deserialize<T: serde::de::DeserializeOwned>(bytes: &[u8]) -> Result<T, NetworkError> {
  serde_json::from_slice(bytes).map_err(|e| NetworkError::ReceiveError(e.to_string()))
}
