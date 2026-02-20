//! Game scene with multiplayer gameplay

use std::collections::HashMap;
use std::net::{SocketAddr, UdpSocket};
use std::time::Duration;

use ryo2d::prelude::*;

use crate::components::*;
use crate::constants::*;

use super::MenuScene;

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

pub struct GameScene {
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
  local_client_id: Option<ClientId>,

  // Local player input
  local_move_x: f32,
  local_jump: bool,

  // Connection state
  connected: bool,
  waiting_for_player: bool,

  // UI state
  show_quit_dialog: bool,
  pending_quit: bool,
}

impl GameScene {
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
      local_client_id: Some(host_id),
      local_move_x: 0.0,
      local_jump: false,
      connected: true,
      waiting_for_player: true,
      show_quit_dialog: false,
      pending_quit: false,
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
      local_client_id: None,
      local_move_x: 0.0,
      local_jump: false,
      connected: false,
      waiting_for_player: true,
      show_quit_dialog: false,
      pending_quit: false,
    })
  }

  fn spawn_background(&mut self, ctx: &mut SceneContext) {
    ctx.world.spawn((
      Transform::new(0.0, 0.0),
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
      .with_z_order(Z_BACKGROUND),
    ));
  }

  fn spawn_platform(&mut self, ctx: &mut SceneContext) {
    let platform_x = (SCREEN_WIDTH as f32 - PLATFORM_WIDTH) / 2.0;

    ctx.world.spawn((
      Transform::new(platform_x, PLATFORM_Y),
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
      .with_z_order(Z_PLATFORM),
      Collider::box_collider(PLATFORM_WIDTH, PLATFORM_HEIGHT),
    ));
  }

  fn spawn_player(&mut self, ctx: &mut SceneContext, slot: u8, client_id: ClientId) -> Entity {
    let (spawn_x, color) = if slot == 0 {
      (PLAYER_A_SPAWN_X, PLAYER_A_COLOR)
    } else {
      (PLAYER_B_SPAWN_X, PLAYER_B_COLOR)
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
      PlayerVelocity::zero(),
    ));

    self.players.insert(client_id, entity);
    entity
  }

  fn handle_input(&mut self, ctx: &mut SceneContext) {
    // Horizontal movement
    self.local_move_x = 0.0;
    if ctx.input.key_down(KeyCode::KeyA) || ctx.input.key_down(KeyCode::ArrowLeft) {
      self.local_move_x -= 1.0;
    }
    if ctx.input.key_down(KeyCode::KeyD) || ctx.input.key_down(KeyCode::ArrowRight) {
      self.local_move_x += 1.0;
    }

    // Jump
    self.local_jump = ctx.input.key_pressed(KeyCode::Space) || ctx.input.key_pressed(KeyCode::KeyW);
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
            let _ = ctx.world.despawn(entity);
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
      if let Some(mut transform) = ctx.world.get_component_mut::<Transform>(entity) {
        transform.x = update.x;
        transform.y = update.y;
      }
    }

    self.waiting_for_player = self.players.len() < 2;
  }

  fn apply_input(&mut self, ctx: &mut SceneContext, entity: Entity, move_x: f32, jump: bool) {
    // Get player grounded state
    let grounded = ctx
      .world
      .get_component::<Player>(entity)
      .map(|p| p.grounded)
      .unwrap_or(false);

    // Apply horizontal movement
    if let Some(mut vel) = ctx.world.get_component_mut::<PlayerVelocity>(entity) {
      vel.x = move_x * PLAYER_SPEED;

      // Jump if grounded
      if jump && grounded {
        vel.y = -PLAYER_JUMP_FORCE;
      }
    }

    // Update grounded state if jumped
    if jump
      && grounded
      && let Some(mut player) = ctx.world.get_component_mut::<Player>(entity)
    {
      player.grounded = false;
    }
  }

  fn update_physics(&mut self, ctx: &mut SceneContext, dt: f32) {
    // Collect entities to update
    let player_entities: Vec<Entity> = self.players.values().copied().collect();

    for entity in player_entities {
      // Apply gravity
      if let Some(mut vel) = ctx.world.get_component_mut::<PlayerVelocity>(entity) {
        vel.y += GRAVITY * dt;
      }

      // Get velocity for position update
      let vel = ctx
        .world
        .get_component::<PlayerVelocity>(entity)
        .map(|v| (v.x, v.y));

      if let Some((vx, vy)) = vel {
        // Update position
        if let Some(mut transform) = ctx.world.get_component_mut::<Transform>(entity) {
          transform.x += vx * dt;
          transform.y += vy * dt;

          // Screen bounds
          transform.x = transform.x.clamp(0.0, SCREEN_WIDTH as f32 - PLAYER_WIDTH);

          // Check platform collision
          let player_bottom = transform.y + PLAYER_HEIGHT;
          let platform_top = PLATFORM_Y;
          let platform_left = (SCREEN_WIDTH as f32 - PLATFORM_WIDTH) / 2.0;
          let platform_right = platform_left + PLATFORM_WIDTH;

          // Check if landing on platform
          if player_bottom >= platform_top
            && transform.x + PLAYER_WIDTH > platform_left
            && transform.x < platform_right
            && vy > 0.0
          {
            transform.y = platform_top - PLAYER_HEIGHT;
          }
        }

        // Set grounded and stop velocity if on platform
        let on_platform = ctx
          .world
          .get_component::<Transform>(entity)
          .map(|t| {
            let player_bottom = t.y + PLAYER_HEIGHT;
            let platform_top = PLATFORM_Y;
            let platform_left = (SCREEN_WIDTH as f32 - PLATFORM_WIDTH) / 2.0;
            let platform_right = platform_left + PLATFORM_WIDTH;
            (player_bottom - platform_top).abs() < 1.0
              && t.x + PLAYER_WIDTH > platform_left
              && t.x < platform_right
          })
          .unwrap_or(false);

        if on_platform && vy > 0.0 {
          if let Some(mut vel) = ctx.world.get_component_mut::<PlayerVelocity>(entity) {
            vel.y = 0.0;
          }
          if let Some(mut player) = ctx.world.get_component_mut::<Player>(entity) {
            player.grounded = true;
          }
        }
      }
    }

    // Player-player collision
    let entities: Vec<_> = self.players.values().copied().collect();
    if entities.len() == 2 {
      self.check_player_collision(ctx, entities[0], entities[1]);
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
      if let Some(transform) = ctx.world.get_component::<Transform>(entity)
        && let Some(player) = ctx.world.get_component::<Player>(entity)
      {
        updates.push((
          client_id,
          PositionUpdate {
            client_id,
            slot: player.slot,
            x: transform.x,
            y: transform.y,
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

  fn check_player_collision(&mut self, ctx: &mut SceneContext, a: Entity, b: Entity) {
    let (ax, ay) = {
      let t = ctx.world.get_component::<Transform>(a);
      match t {
        Some(t) => (t.x, t.y),
        None => return,
      }
    };

    let (bx, by) = {
      let t = ctx.world.get_component::<Transform>(b);
      match t {
        Some(t) => (t.x, t.y),
        None => return,
      }
    };

    // AABB collision
    let overlap_x = (ax < bx + PLAYER_WIDTH) && (ax + PLAYER_WIDTH > bx);
    let overlap_y = (ay < by + PLAYER_HEIGHT) && (ay + PLAYER_HEIGHT > by);

    if overlap_x && overlap_y {
      // Calculate overlap amounts
      let overlap_left = (ax + PLAYER_WIDTH) - bx;
      let overlap_right = (bx + PLAYER_WIDTH) - ax;
      let overlap_top = (ay + PLAYER_HEIGHT) - by;
      let overlap_bottom = (by + PLAYER_HEIGHT) - ay;

      let min_overlap_x = overlap_left.min(overlap_right);
      let min_overlap_y = overlap_top.min(overlap_bottom);

      if min_overlap_y < min_overlap_x {
        // Vertical collision (jumping on head)
        if ay < by {
          // A is on top of B
          if let Some(mut t) = ctx.world.get_component_mut::<Transform>(a) {
            t.y = by - PLAYER_HEIGHT;
          }
          if let Some(mut v) = ctx.world.get_component_mut::<PlayerVelocity>(a) {
            v.y = 0.0;
          }
          if let Some(mut p) = ctx.world.get_component_mut::<Player>(a) {
            p.grounded = true;
          }
        } else {
          // B is on top of A
          if let Some(mut t) = ctx.world.get_component_mut::<Transform>(b) {
            t.y = ay - PLAYER_HEIGHT;
          }
          if let Some(mut v) = ctx.world.get_component_mut::<PlayerVelocity>(b) {
            v.y = 0.0;
          }
          if let Some(mut p) = ctx.world.get_component_mut::<Player>(b) {
            p.grounded = true;
          }
        }
      } else {
        // Horizontal collision (push apart)
        let push = min_overlap_x / 2.0 + 1.0;
        if ax < bx {
          if let Some(mut t) = ctx.world.get_component_mut::<Transform>(a) {
            t.x -= push;
          }
          if let Some(mut t) = ctx.world.get_component_mut::<Transform>(b) {
            t.x += push;
          }
        } else {
          if let Some(mut t) = ctx.world.get_component_mut::<Transform>(a) {
            t.x += push;
          }
          if let Some(mut t) = ctx.world.get_component_mut::<Transform>(b) {
            t.x -= push;
          }
        }
      }
    }
  }
}

impl Scene for GameScene {
  fn name(&self) -> &str {
    "GameScene"
  }

  fn on_enter(&mut self, ctx: &mut SceneContext) {
    ctx.world.clear();
    ctx
      .camera
      .set_position(SCREEN_WIDTH as f32 / 2.0, SCREEN_HEIGHT as f32 / 2.0);

    // Spawn background and platform
    self.spawn_background(ctx);
    self.spawn_platform(ctx);

    // If host, spawn player A immediately
    if self.role == Role::Host {
      let host_id = self.server.as_ref().unwrap().host_id();
      self.spawn_player(ctx, 0, host_id);
    }
  }

  fn update(&mut self, ctx: &mut SceneContext, dt: f32) -> SceneAction {
    // Check for pending quit (confirmed in dialog)
    if self.pending_quit {
      // Gracefully disconnect
      if let Some(client) = self.client.as_mut() {
        client.disconnect();
      }
      // Server will clean up on drop
      return SceneAction::Switch(Box::new(MenuScene::new()));
    }

    // ESC toggles quit dialog
    if ctx.input.key_pressed(KeyCode::Escape) {
      self.show_quit_dialog = !self.show_quit_dialog;
    }

    // Don't process game input while dialog is open
    if !self.show_quit_dialog {
      self.handle_input(ctx);

      // Update based on role
      match self.role {
        Role::Host => self.update_host(ctx, dt),
        Role::Client => self.update_client(ctx, dt),
      }
    }

    SceneAction::Continue
  }

  fn render_ui(&mut self, ui: &mut UiContext) -> SceneAction {
    let (sw, sh) = ui.screen_size();

    // NOTE: Don't draw background here - it covers game sprites!
    // Background is set via clear color in on_enter

    // Room code (top right, faded)
    let code_text = format!("Room: {}", self.room_code);
    let (tw, _) = ui.text_size(&code_text);
    ui.draw_text(
      &code_text,
      sw as f32 - tw as f32 - 20.0,
      20.0,
      Color::new(255, 255, 255, 100),
    );

    // Show host IP addresses below room code (host only)
    if self.role == Role::Host {
      let mut y_pos = 40.0;

      if let Some(ref lan) = self.lan_ip {
        let lan_text = format!("LAN: {}:{}", lan, DEFAULT_PORT);
        let (tw, _) = ui.text_size(&lan_text);
        ui.draw_text(
          &lan_text,
          sw as f32 - tw as f32 - 20.0,
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
          sw as f32 - tw as f32 - 20.0,
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
      20.0,
      20.0,
      if self.role == Role::Host {
        Color::rgb(66, 135, 245)
      } else {
        Color::rgb(245, 66, 66)
      },
    );
    ui.draw_text(
      &format!("Players: {}/2", player_count),
      20.0,
      40.0,
      Color::WHITE,
    );

    // Show player positions
    let mut y_offset = 60.0;
    for (&client_id, &entity) in &self.players {
      let pos_str = format!("  P{}: entity {:?}", client_id, entity);
      ui.draw_text(&pos_str, 20.0, y_offset, Color::GRAY);
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

    // Controls hint
    ui.draw_text(
      "A/D: Move | Space: Jump | ESC: Menu",
      20.0,
      sh as f32 - 30.0,
      Color::LIGHT_GRAY,
    );

    // Quit confirmation dialog
    if self.show_quit_dialog {
      let center_x = sw as f32 / 2.0;
      let center_y = sh as f32 / 2.0;

      // Semi-transparent overlay
      ui.draw_rect(
        Rect::new(0.0, 0.0, sw as f32, sh as f32),
        Color::new(0, 0, 0, 180),
      );

      // Dialog box
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

      // Dialog text
      ui.draw_text_anchored("Leave Game?", Anchor::Center, 0.0, -30.0, Color::WHITE);

      // Buttons
      let btn_w = 100.0;
      let btn_h = 36.0;
      let btn_y = center_y + 25.0;

      let yes_rect = Rect::new(center_x - btn_w - 20.0, btn_y, btn_w, btn_h);
      let no_rect = Rect::new(center_x + 20.0, btn_y, btn_w, btn_h);

      if ui.button("Yes, Leave", yes_rect) {
        self.pending_quit = true;
      }

      if ui.button("Cancel", no_rect) {
        self.show_quit_dialog = false;
      }
    }
    SceneAction::Continue
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
