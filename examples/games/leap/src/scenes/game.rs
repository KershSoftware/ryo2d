//! Game scene with leap mechanics and multiplayer

use std::collections::HashMap;
use std::net::{SocketAddr, UdpSocket};
use std::time::Duration;

use ryo2d::prelude::*;

use crate::components::*;
use crate::constants::*;
use crate::generation::*;
use crate::network::*;

use super::MenuScene;

/// Get the local LAN IP address
fn get_lan_ip() -> Option<String> {
    let socket = UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("8.8.8.8:80").ok()?;
    let local_addr = socket.local_addr().ok()?;
    Some(local_addr.ip().to_string())
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

    // Entity tracking
    players: HashMap<ClientId, Entity>,
    player_lights: HashMap<ClientId, Entity>,
    local_client_id: Option<ClientId>,
    platforms: Vec<Entity>,
    platform_lamps: Vec<Entity>,
    background_tiles: Vec<Entity>,
    bg_tile_height: f32,
    rain_emitter: Option<Entity>,
    rain_sprite: Option<SpriteHandle>,

    // Particle system
    particle_system: ParticleSystem,

    // Platform generation
    seed: u64,
    generator: PlatformGenerator,
    highest_player_y: f32,

    // Camera
    camera_y: f32,

    // Network timing
    broadcast_timer: f32,

    // Animation timing
    elapsed_time: f32,

    // Death/respawn
    respawn_timer: Option<f32>,

    // Connection state
    connected: bool,
    waiting_for_player: bool,
    received_seed: bool,

    // UI state
    show_quit_dialog: bool,
    pending_quit: bool,

    // Cached state for UI rendering
    charge_indicator: ChargeIndicatorState,
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
            players: HashMap::new(),
            player_lights: HashMap::new(),
            local_client_id: Some(ClientId::from(0u64)),
            platforms: Vec::new(),
            platform_lamps: Vec::new(),
            background_tiles: Vec::new(),
            bg_tile_height: 0.0,
            rain_emitter: None,
            rain_sprite: None,
            particle_system: ParticleSystem::new(),
            seed,
            generator: PlatformGenerator::new(seed),
            highest_player_y: PLAYER_SPAWN_Y,
            camera_y: PLAYER_SPAWN_Y - CAMERA_LOOK_AHEAD,
            broadcast_timer: 0.0,
            elapsed_time: 0.0,
            respawn_timer: None,
            connected: true,
            waiting_for_player: false,
            received_seed: true,
            show_quit_dialog: false,
            pending_quit: false,
            charge_indicator: ChargeIndicatorState::default(),
        })
    }

    /// Create host (server) game
    pub fn new_host(room_code: String) -> Result<Self, NetworkError> {
        let server = ListenServer::new(NetworkServerConfig {
            port: DEFAULT_PORT,
            max_clients: 2,
            protocol_id: PROTOCOL_ID,
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
            players: HashMap::new(),
            player_lights: HashMap::new(),
            local_client_id: Some(host_id),
            platforms: Vec::new(),
            platform_lamps: Vec::new(),
            background_tiles: Vec::new(),
            bg_tile_height: 0.0,
            rain_emitter: None,
            rain_sprite: None,
            particle_system: ParticleSystem::new(),
            seed,
            generator: PlatformGenerator::new(seed),
            highest_player_y: PLAYER_SPAWN_Y,
            camera_y: PLAYER_SPAWN_Y - CAMERA_LOOK_AHEAD,
            broadcast_timer: 0.0,
            elapsed_time: 0.0,
            respawn_timer: None,
            connected: true,
            waiting_for_player: true,
            received_seed: true,
            show_quit_dialog: false,
            pending_quit: false,
            charge_indicator: ChargeIndicatorState::default(),
        })
    }

    /// Create client game
    pub fn new_client(room_code: &str, address: &str) -> Result<Self, NetworkError> {
        let server_addr: SocketAddr = if address.contains(':') {
            address
                .parse()
                .map_err(|_| NetworkError::ConnectionError(format!("Invalid address: {}", address)))?
        } else {
            format!("{}:{}", address, DEFAULT_PORT)
                .parse()
                .map_err(|_| NetworkError::ConnectionError(format!("Invalid address: {}", address)))?
        };

        let client = NetworkClient::connect(NetworkClientConfig {
            server_addr,
            protocol_id: PROTOCOL_ID,
        })?;

        Ok(Self {
            mode: GameMode::Client,
            room_code: room_code.to_string(),
            server: None,
            client: Some(client),
            lan_ip: None,
            players: HashMap::new(),
            player_lights: HashMap::new(),
            local_client_id: None,
            platforms: Vec::new(),
            platform_lamps: Vec::new(),
            background_tiles: Vec::new(),
            bg_tile_height: 0.0,
            rain_emitter: None,
            rain_sprite: None,
            particle_system: ParticleSystem::new(),
            seed: 0, // Will receive from server
            generator: PlatformGenerator::new(0),
            highest_player_y: PLAYER_SPAWN_Y,
            camera_y: PLAYER_SPAWN_Y - CAMERA_LOOK_AHEAD,
            broadcast_timer: 0.0,
            elapsed_time: 0.0,
            respawn_timer: None,
            connected: false,
            waiting_for_player: true,
            received_seed: false,
            show_quit_dialog: false,
            pending_quit: false,
            charge_indicator: ChargeIndicatorState::default(),
        })
    }

    fn spawn_platform_entity(
        &mut self,
        ctx: &mut SceneContext,
        x: f32,
        y: f32,
        width: f32,
        chunk_y: i32,
        is_spawn: bool,
    ) -> Entity {
        let color = if is_spawn {
            SPAWN_PLATFORM_COLOR
        } else {
            PLATFORM_COLOR
        };

        let entity = ctx.world.spawn((
            Transform::new(x, y),
            Renderable::sprite(SpriteHandle::from_sprite(
                Sprite::solid_rect(width as u32, PLATFORM_HEIGHT as u32, color.0, color.1, color.2, 255),
                format!("platform_{}_{}", chunk_y, self.platforms.len()),
            ))
            .with_z_order(Z_PLATFORM),
            Collider::box_collider(width, PLATFORM_HEIGHT),
            if is_spawn {
                Platform::spawn_platform()
            } else {
                Platform::new(width, chunk_y)
            },
        ));

        self.platforms.push(entity);

        // Spawn a lamp above the platform (not for spawn platform)
        if !is_spawn {
            self.spawn_platform_lamp(ctx, x, y, width, chunk_y);
        }

        entity
    }

    fn spawn_platform_lamp(
        &mut self,
        ctx: &mut SceneContext,
        platform_x: f32,
        platform_y: f32,
        platform_width: f32,
        chunk_y: i32,
    ) {
        // Use chunk-based seed to determine left or right placement and phase
        let chunk_seed = self.seed.wrapping_add(chunk_y as u64 * 7919 + self.platform_lamps.len() as u64);
        let on_left = (chunk_seed % 2) == 0;
        let phase = (chunk_seed % 628) as f32 / 100.0; // Random phase 0 to ~2π

        let lamp_x = if on_left {
            platform_x + LAMP_OFFSET_X
        } else {
            platform_x + platform_width - LAMP_OFFSET_X
        };
        let lamp_y = platform_y + LAMP_OFFSET_Y;

        // Outer glow light (dimmer, larger)
        let outer_lamp = ctx.world.spawn((
            Transform::new(lamp_x, lamp_y),
            Light2D::point_with_intensity(
                Color::rgb(LAMP_LIGHT_COLOR.0, LAMP_LIGHT_COLOR.1, LAMP_LIGHT_COLOR.2),
                LAMP_LIGHT_RADIUS,
                LAMP_LIGHT_INTENSITY,
            ),
            PlatformLamp::new(lamp_x, lamp_y, phase),
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
            PlatformLamp::new(lamp_x, lamp_y, phase),
        ));
        self.platform_lamps.push(inner_lamp);
    }

    fn update_lamps(&mut self, ctx: &mut SceneContext, dt: f32) {
        // Accumulate time for oscillation
        self.elapsed_time += dt;

        let lamp_entities: Vec<Entity> = self.platform_lamps.clone();

        for entity in lamp_entities {
            let lamp_data = ctx.world.get_component::<PlatformLamp>(entity).map(|l| *l);

            if let Some(lamp) = lamp_data {
                if let Some(mut transform) = ctx.world.get_component_mut::<Transform>(entity) {
                    let angle = self.elapsed_time * LAMP_OSCILLATE_SPEED + lamp.phase;

                    // Circular oscillation
                    let offset_x = angle.cos() * LAMP_OSCILLATE_RADIUS;
                    let offset_y = angle.sin() * LAMP_OSCILLATE_RADIUS;

                    transform.x = lamp.base_x + offset_x;
                    transform.y = lamp.base_y + offset_y;
                }
            }
        }
    }

    fn spawn_spawn_platform(&mut self, ctx: &mut SceneContext) {
        let x = (SCREEN_WIDTH as f32 - SPAWN_PLATFORM_WIDTH) / 2.0;
        self.spawn_platform_entity(ctx, x, SPAWN_PLATFORM_Y, SPAWN_PLATFORM_WIDTH, 0, true);
    }

    fn spawn_rain(&mut self, ctx: &mut SceneContext) {
        // Create a small particle sprite for rain drops using asset manager
        let rain_sprite = ctx.assets.create_sprite(
            Sprite::solid_rect(2, 8, 180, 200, 255, 200), // Blue-white raindrop
            "rain_particle",
        );
        self.rain_sprite = Some(rain_sprite.clone());

        // Create rain emitter - wide enough to cover the screen
        let emitter = ParticleEmitter::rain(rain_sprite)
            .with_mode(EmissionMode::Continuous { rate: 200.0 })
            .with_shape(EmissionShape::Line {
                start_x: -(SCREEN_WIDTH as f32),
                start_y: -50.0,
                end_x: SCREEN_WIDTH as f32,
                end_y: -50.0,
            })
            .with_lifetime(2.0, 3.0)
            .with_gravity(20.0, 500.0); // Slight wind, moderate fall

        let rain_entity = ctx.world.spawn((
            Transform::new(SCREEN_WIDTH as f32 / 2.0, self.camera_y - SCREEN_HEIGHT as f32 / 2.0),
            emitter,
        ));

        self.rain_emitter = Some(rain_entity);
    }

    fn update_rain(&mut self, ctx: &mut SceneContext) {
        // Keep rain emitter above the camera
        if let Some(rain_entity) = self.rain_emitter {
            if let Some(mut transform) = ctx.world.get_component_mut::<Transform>(rain_entity) {
                transform.x = SCREEN_WIDTH as f32 / 2.0;
                transform.y = self.camera_y - SCREEN_HEIGHT as f32 / 2.0;
            }
        }
    }

    fn spawn_background(&mut self, ctx: &mut SceneContext) {
        // Load background image
        let bg_handle = match ctx.assets.load_sprite("assets/bg.png") {
            Ok(handle) => handle,
            Err(_) => return, // No background if asset fails to load
        };

        let bg_width = bg_handle.width() as f32;
        let bg_height = bg_handle.height() as f32;

        // Scale to fit screen width
        let scale = SCREEN_WIDTH as f32 / bg_width;
        let scaled_height = bg_height * scale;
        self.bg_tile_height = scaled_height;

        // Spawn enough tiles to cover visible area plus buffer (3 tiles)
        for i in 0..4 {
            let y = SPAWN_PLATFORM_Y - (i as f32 * scaled_height);
            let mut transform = Transform::new(0.0, y);
            transform.scale_x = scale;
            transform.scale_y = scale;

            let entity = ctx.world.spawn((
                transform,
                Renderable::sprite(bg_handle.clone()).with_z_order(Z_BACKGROUND),
            ));
            self.background_tiles.push(entity);
        }
    }

    fn update_background(&mut self, ctx: &mut SceneContext) {
        if self.bg_tile_height <= 0.0 || self.background_tiles.is_empty() {
            return;
        }

        // Find the camera's visible range
        let visible_top = self.camera_y - SCREEN_HEIGHT as f32;
        let visible_bottom = self.camera_y + SCREEN_HEIGHT as f32;

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
        for &entity in self.players.values() {
            let Some(player) = ctx.world.get_component::<Player>(entity) else {
                continue;
            };

            let (scale_x, scale_y, offset_x) = if player.leap_state.is_charging() {
                // Calculate squish based on charge time (0 to 1)
                let charge_t = (player.leap_state.charge_time() / MAX_CHARGE_TIME).min(1.0);

                // Ease in the squish effect
                let eased_t = charge_t * charge_t; // Quadratic ease-in

                // Interpolate scale: start at 1.0, go to squish values
                let sx = 1.0 + (CHARGE_SQUISH_X - 1.0) * eased_t;
                let sy = 1.0 + (CHARGE_SQUISH_Y - 1.0) * eased_t;

                // Lean away from aim direction
                let lean = CHARGE_LEAN_OFFSET * eased_t * (-player.facing.sign());

                (sx, sy, lean)
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

        // Spawn a light for this player
        let light_entity = ctx.world.spawn((
            Transform::new(spawn_x + PLAYER_WIDTH / 2.0, PLAYER_SPAWN_Y + PLAYER_HEIGHT / 2.0),
            Light2D::point_with_intensity(
                Color::rgb(PLAYER_LIGHT_COLOR.0, PLAYER_LIGHT_COLOR.1, PLAYER_LIGHT_COLOR.2),
                PLAYER_LIGHT_RADIUS,
                PLAYER_LIGHT_INTENSITY,
            ),
            PlayerLight { player_entity: entity },
        ));
        self.player_lights.insert(client_id, light_entity);

        entity
    }

    fn get_local_player_entity(&self) -> Option<Entity> {
        self.local_client_id.and_then(|id| self.players.get(&id).copied())
    }

    fn check_death_and_respawn(&mut self, ctx: &mut SceneContext, dt: f32) {
        // Check if respawn timer is active
        if let Some(timer) = self.respawn_timer.as_mut() {
            *timer -= dt;
            if *timer <= 0.0 {
                self.respawn_timer = None;
                // For clients, send respawn request to server
                // For host/single player, respawn locally
                match self.mode {
                    GameMode::SinglePlayer | GameMode::Host => {
                        self.respawn_local_player(ctx);
                    }
                    GameMode::Client => {
                        self.send_respawn_request();
                        // Also respawn locally for immediate feedback
                        self.respawn_local_player(ctx);
                    }
                }
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

        // Start respawn timer if player fell below death threshold
        if transform.y > DEATH_Y {
            self.respawn_timer = Some(RESPAWN_DELAY);
        }
    }

    fn respawn_local_player(&mut self, ctx: &mut SceneContext) {
        let Some(entity) = self.get_local_player_entity() else {
            return;
        };

        self.respawn_player(ctx, entity);

        // Reset camera to spawn position (only for local player)
        self.camera_y = PLAYER_SPAWN_Y - CAMERA_LOOK_AHEAD;
    }

    fn respawn_player(&mut self, ctx: &mut SceneContext, entity: Entity) {
        // Get player slot for spawn position
        let slot = ctx
            .world
            .get_component::<Player>(entity)
            .map(|p| p.slot)
            .unwrap_or(0);

        let spawn_x = if slot == 0 {
            PLAYER_A_SPAWN_X
        } else {
            PLAYER_B_SPAWN_X
        };

        // Reset position
        if let Some(mut transform) = ctx.world.get_component_mut::<Transform>(entity) {
            transform.x = spawn_x;
            transform.y = PLAYER_SPAWN_Y;
        }

        // Reset velocity
        if let Some(mut vel) = ctx.world.get_component_mut::<PlayerVelocity>(entity) {
            vel.x = 0.0;
            vel.y = 0.0;
        }

        // Reset player state
        if let Some(mut player) = ctx.world.get_component_mut::<Player>(entity) {
            player.grounded = true;
            player.leap_state = LeapState::Idle;
        }
    }

    fn send_respawn_request(&mut self) {
        let Some(client) = self.client.as_mut() else {
            return;
        };

        let respawn_rpc = RespawnRpc;
        if let Ok(serialized) = SerializedRpc::from_rpc(&respawn_rpc, client.client_id(), RpcTarget::Server)
            && let Ok(bytes) = serialized.to_bytes()
        {
            client.send(Channel::ReliableOrdered, &bytes);
        }
    }

    fn handle_input(&mut self, ctx: &mut SceneContext, dt: f32) -> LeapInput {
        let mut input = LeapInput::default();

        let entity = match self.get_local_player_entity() {
            Some(e) => e,
            None => return input,
        };

        // Get current player state
        let player = match ctx.world.get_component::<Player>(entity) {
            Some(p) => *p,
            None => return input,
        };

        // Check for charge button (Space or gamepad South/A button)
        let charge_button_down = ctx.input.key_down(KeyCode::Space)
            || ctx.input.gamepad_button_down(GamepadButton::South);

        // Check directional inputs (keyboard + gamepad)
        let aim_up = ctx.input.key_down(KeyCode::ArrowUp)
            || ctx.input.key_down(KeyCode::KeyW)
            || ctx.input.gamepad_button_down(GamepadButton::DPadUp)
            || ctx.input.left_stick_y() < -0.3;
        let aim_down = ctx.input.key_down(KeyCode::ArrowDown)
            || ctx.input.key_down(KeyCode::KeyS)
            || ctx.input.gamepad_button_down(GamepadButton::DPadDown)
            || ctx.input.left_stick_y() > 0.3;
        let aim_left = ctx.input.key_down(KeyCode::ArrowLeft)
            || ctx.input.key_down(KeyCode::KeyA)
            || ctx.input.gamepad_button_down(GamepadButton::DPadLeft)
            || ctx.input.left_stick_x() < -0.3;
        let aim_right = ctx.input.key_down(KeyCode::ArrowRight)
            || ctx.input.key_down(KeyCode::KeyD)
            || ctx.input.gamepad_button_down(GamepadButton::DPadRight)
            || ctx.input.left_stick_x() > 0.3;

        // Handle charging state
        if player.leap_state.can_leap() && charge_button_down {
            // Start or continue charging
            input.charging = true;
            input.aim_angle = DEFAULT_AIM_ANGLE;
        } else if player.leap_state.is_charging() {
            if charge_button_down {
                // Continue charging
                input.charging = true;
                input.charge_time = player.leap_state.charge_time();
                input.aim_angle = player.leap_state.aim_angle();

                // Aim adjustment (vertical)
                if aim_up {
                    input.aim_delta = AIM_SPEED * dt;
                }
                if aim_down {
                    input.aim_delta = -AIM_SPEED * dt;
                }

                // Analog stick for smooth aim adjustment
                let stick_y = ctx.input.left_stick_y();
                if stick_y.abs() > 0.1 {
                    input.aim_delta = -stick_y * AIM_SPEED * dt;
                }

                // Horizontal aim changes facing (inverted - pull back to aim)
                // Press left to aim RIGHT, press right to aim LEFT
                if aim_left
                    && let Some(mut p) = ctx.world.get_component_mut::<Player>(entity)
                {
                    p.facing = Facing::Right;
                }
                if aim_right
                    && let Some(mut p) = ctx.world.get_component_mut::<Player>(entity)
                {
                    p.facing = Facing::Left;
                }
            } else {
                // Released charge button - trigger leap
                input.release = true;
                input.charge_time = player.leap_state.charge_time();
                input.aim_angle = player.leap_state.aim_angle();
            }
        }

        input
    }

    fn apply_input(&mut self, ctx: &mut SceneContext, entity: Entity, input: &LeapInput, dt: f32) {
        let mut player = match ctx.world.get_component_mut::<Player>(entity) {
            Some(p) => p,
            None => return,
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

                    if let Some(mut vel) = ctx.world.get_component_mut::<PlayerVelocity>(entity) {
                        vel.x = vx;
                        vel.y = vy;
                    }

                    if let Some(mut p) = ctx.world.get_component_mut::<Player>(entity) {
                        p.leap_state = LeapState::Airborne;
                        p.grounded = false;
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

    fn update_physics(&mut self, ctx: &mut SceneContext, dt: f32) {
        let player_entities: Vec<Entity> = self.players.values().copied().collect();

        for entity in player_entities {
            // Check if airborne
            let is_airborne = ctx
                .world
                .get_component::<Player>(entity)
                .map(|p| p.leap_state.is_airborne())
                .unwrap_or(false);

            if !is_airborne {
                continue;
            }

            // Apply gravity
            if let Some(mut vel) = ctx.world.get_component_mut::<PlayerVelocity>(entity) {
                vel.y += GRAVITY * dt;
            }

            // Get velocity
            let vel = ctx
                .world
                .get_component::<PlayerVelocity>(entity)
                .map(|v| (v.x, v.y));

            if let Some((vx, vy)) = vel {
                // Store previous Y for sweep collision
                let prev_y = ctx
                    .world
                    .get_component::<Transform>(entity)
                    .map(|t| t.y)
                    .unwrap_or(0.0);

                // Update position
                if let Some(mut transform) = ctx.world.get_component_mut::<Transform>(entity) {
                    transform.x += vx * dt;
                    transform.y += vy * dt;

                    // Clamp to screen bounds (no wrapping)
                    transform.x = transform.x.clamp(0.0, SCREEN_WIDTH as f32 - PLAYER_WIDTH);
                }

                // Check platform collisions (only when falling)
                if vy > 0.0 {
                    self.check_platform_landing(ctx, entity, prev_y);
                }
            }
        }
    }

    fn check_platform_landing(
        &mut self,
        ctx: &mut SceneContext,
        player_entity: Entity,
        prev_y: f32,
    ) {
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

            if let Some(mut vel) = ctx.world.get_component_mut::<PlayerVelocity>(player_entity) {
                vel.x = 0.0;
                vel.y = 0.0;
            }

            if let Some(mut player) = ctx.world.get_component_mut::<Player>(player_entity) {
                player.grounded = true;
                player.leap_state = LeapState::Landing {
                    timer: LANDING_RECOVERY_TIME,
                };
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
                    self.spawn_platform_entity(ctx, pdata.x, pdata.y, pdata.width, chunk_index, false);
                }
            }
        }
    }

    fn update_camera(&mut self, ctx: &mut SceneContext) {
        // Follow local player - camera looks ABOVE the player so they can see where to jump
        let player_y = match self.get_local_player_entity() {
            Some(entity) => {
                ctx.world
                    .get_component::<Transform>(entity)
                    .map(|t| t.y)
            }
            None => None,
        };

        let Some(player_y) = player_y else { return };

        // Target camera position: above the player by CAMERA_LOOK_AHEAD
        // This puts the player near the bottom of the screen
        let target_camera_y = player_y - CAMERA_LOOK_AHEAD;

        // Always follow the player with smooth lerping
        self.camera_y += (target_camera_y - self.camera_y) * CAMERA_LERP_SPEED;

        // Update camera position (camera_y is the center Y position)
        ctx.camera
            .set_position(SCREEN_WIDTH as f32 / 2.0, self.camera_y);
    }

    fn update_single_player(&mut self, ctx: &mut SceneContext, dt: f32) {
        // Check for death and handle respawn
        self.check_death_and_respawn(ctx, dt);

        let input = self.handle_input(ctx, dt);

        if let Some(entity) = self.get_local_player_entity() {
            self.apply_input(ctx, entity, &input, dt);
        }

        self.update_physics(ctx, dt);
        self.generate_platforms_as_needed(ctx);
        self.update_camera(ctx);
        self.update_rain(ctx);
        self.update_background(ctx);
        self.update_player_lights(ctx);
        self.update_player_squish(ctx);
        self.update_lamps(ctx, dt);
        self.update_charge_indicator(ctx);

        // Update particle system
        self.particle_system.update(ctx.world, ctx.events, dt);
    }

    fn update_host(&mut self, ctx: &mut SceneContext, dt: f32) {
        // Check for death and handle respawn
        self.check_death_and_respawn(ctx, dt);

        let server = self.server.as_mut().unwrap();
        server.update(Duration::from_secs_f32(dt));

        // Process events
        let events: Vec<_> = server.events().collect();
        for event in events {
            match event {
                NetworkEvent::ClientConnected { client_id } => {
                    // Send seed to new client
                    let seed_rpc = GameSeedRpc { seed: self.seed };
                    if let Ok(serialized) = SerializedRpc::from_rpc(&seed_rpc, self.local_client_id.unwrap(), RpcTarget::Client(client_id))
                        && let Ok(bytes) = serialized.to_bytes()
                    {
                        self.server.as_mut().unwrap().send(client_id, Channel::ReliableOrdered, &bytes);
                    }

                    // Spawn player B
                    self.spawn_player(ctx, 1, client_id);
                    self.waiting_for_player = false;
                }
                NetworkEvent::ClientDisconnected { client_id, .. } => {
                    if let Some(entity) = self.players.remove(&client_id) {
                        let _ = ctx.world.despawn(entity);
                    }
                    self.waiting_for_player = true;
                }
                _ => {}
            }
        }

        // Receive input from clients - collect inputs first to avoid borrow issues
        let mut pending_inputs: Vec<(Entity, LeapInput)> = Vec::new();
        let mut pending_respawns: Vec<Entity> = Vec::new();
        {
            let server = self.server.as_mut().unwrap();
            for client_id in server.remote_clients().collect::<Vec<_>>() {
                while let Some(data) = server.receive(client_id, Channel::ReliableOrdered) {
                    if let Ok(serialized) = SerializedRpc::from_bytes(&data) {
                        // Try to parse as LeapInputRpc
                        if let Ok(rpc) = serialized.clone().into_rpc::<LeapInputRpc>()
                            && let Some(&entity) = self.players.get(&client_id)
                        {
                            pending_inputs.push((entity, rpc.payload.input));
                        }
                        // Try to parse as RespawnRpc
                        else if let Ok(_) = serialized.into_rpc::<RespawnRpc>()
                            && let Some(&entity) = self.players.get(&client_id)
                        {
                            pending_respawns.push(entity);
                        }
                    }
                }
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

        self.update_physics(ctx, dt);
        self.generate_platforms_as_needed(ctx);
        self.update_camera(ctx);
        self.update_rain(ctx);
        self.update_background(ctx);
        self.update_player_lights(ctx);
        self.update_player_squish(ctx);
        self.update_lamps(ctx, dt);
        self.update_charge_indicator(ctx);

        // Update particle system
        self.particle_system.update(ctx.world, ctx.events, dt);

        // Broadcast positions
        self.broadcast_timer += dt;
        if self.broadcast_timer >= POSITION_BROADCAST_RATE {
            self.broadcast_timer = 0.0;
            self.broadcast_positions(ctx);
        }
    }

    fn update_client(&mut self, ctx: &mut SceneContext, dt: f32) {
        // Check for death and handle respawn
        self.check_death_and_respawn(ctx, dt);

        let client = self.client.as_mut().unwrap();
        client.update(Duration::from_secs_f32(dt));

        // Check connection
        if !self.connected && client.is_connected() {
            self.connected = true;
            self.local_client_id = Some(client.client_id());
        }

        // Process events
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

        // Receive seed and position updates - collect data first to avoid borrow issues
        let mut received_seed_data: Option<u64> = None;
        let mut position_updates: Vec<PositionUpdate> = Vec::new();
        {
            let client = self.client.as_mut().unwrap();

            // Check for seed (reliable)
            while let Some(data) = client.receive(Channel::ReliableOrdered) {
                if let Ok(serialized) = SerializedRpc::from_bytes(&data)
                    && let Ok(rpc) = serialized.into_rpc::<GameSeedRpc>()
                {
                    received_seed_data = Some(rpc.payload.seed);
                }
            }

            // Check for position updates (unreliable)
            while let Some(data) = client.receive(Channel::Unreliable) {
                if let Ok(update) = deserialize::<PositionUpdate>(&data) {
                    position_updates.push(update);
                }
            }
        }

        // Process seed if received
        if let Some(seed) = received_seed_data {
            self.seed = seed;
            self.generator.reset_with_seed(self.seed);
            self.received_seed = true;
            self.generate_platforms_as_needed(ctx);
        }

        // Process position updates
        // Local player uses client-side prediction - only spawn, don't update position
        let local_id = self.local_client_id;
        for update in position_updates {
            let is_local = Some(update.client_id) == local_id;
            let already_spawned = self.players.contains_key(&update.client_id);

            // Spawn player if not yet spawned
            let entity = if let Some(&e) = self.players.get(&update.client_id) {
                e
            } else {
                self.spawn_player(ctx, update.slot, update.client_id)
            };

            // Skip position updates for local player (we predict locally)
            // But we still needed to spawn them above
            if is_local && already_spawned {
                continue;
            }

            if let Some(mut transform) = ctx.world.get_component_mut::<Transform>(entity) {
                transform.x = update.x;
                transform.y = update.y;
            }

            if let Some(mut vel) = ctx.world.get_component_mut::<PlayerVelocity>(entity) {
                vel.x = update.vx;
                vel.y = update.vy;
            }

            if let Some(mut player) = ctx.world.get_component_mut::<Player>(entity) {
                player.leap_state = update.leap_state.into();
                player.facing = if update.facing_right {
                    Facing::Right
                } else {
                    Facing::Left
                };
            }
        }

        self.waiting_for_player = self.players.len() < 2;

        // Handle local input and apply it locally (client-side prediction)
        let input = self.handle_input(ctx, dt);
        if let Some(entity) = self.get_local_player_entity() {
            self.apply_input(ctx, entity, &input, dt);
        }

        // Send input to server
        let client = self.client.as_mut().unwrap();
        let input_rpc = LeapInputRpc { input };
        if let Ok(serialized) = SerializedRpc::from_rpc(&input_rpc, client.client_id(), RpcTarget::Server)
            && let Ok(bytes) = serialized.to_bytes()
        {
            client.send(Channel::ReliableOrdered, &bytes);
        }

        // Run local physics for smooth movement (client-side prediction)
        self.update_physics(ctx, dt);

        self.generate_platforms_as_needed(ctx);
        self.update_camera(ctx);
        self.update_rain(ctx);
        self.update_background(ctx);
        self.update_player_lights(ctx);
        self.update_player_squish(ctx);
        self.update_lamps(ctx, dt);
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

        for (&client_id, &entity) in &self.players {
            if let Some(transform) = ctx.world.get_component::<Transform>(entity)
                && let Some(vel) = ctx.world.get_component::<PlayerVelocity>(entity)
                && let Some(player) = ctx.world.get_component::<Player>(entity)
            {
                updates.push(PositionUpdate {
                    client_id,
                    slot: player.slot,
                    x: transform.x,
                    y: transform.y,
                    vx: vel.x,
                    vy: vel.y,
                    leap_state: player.leap_state.into(),
                    facing_right: player.facing == Facing::Right,
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
        self.charge_indicator.screen_x = transform.x + PLAYER_WIDTH / 2.0;
        self.charge_indicator.screen_y = transform.y - self.camera_y + SCREEN_HEIGHT as f32 / 2.0;
        self.charge_indicator.charge_pct = (charge_time / MAX_CHARGE_TIME).min(1.0);
        self.charge_indicator.is_overcharging = charge_time > OVERCHARGE_THRESHOLD;
        self.charge_indicator.trajectory_vx = force * angle_rad.cos() * direction;
        self.charge_indicator.trajectory_vy = -force * angle_rad.sin();
    }

    fn draw_charge_indicator(&self, ui: &mut UiContext) {
        if !self.charge_indicator.visible {
            return;
        }

        let screen_x = self.charge_indicator.screen_x;
        let screen_y = self.charge_indicator.screen_y;

        // Draw charge bar
        let bar_width = 40.0;
        let bar_height = 6.0;
        let bar_x = screen_x - bar_width / 2.0;
        let bar_y = screen_y - 20.0;

        // Background
        ui.draw_rect(
            Rect::new(bar_x - 1.0, bar_y - 1.0, bar_width + 2.0, bar_height + 2.0),
            Color::BLACK,
        );

        // Fill
        let fill_color = if self.charge_indicator.is_overcharging {
            Color::rgb(OVERCHARGE_COLOR.0, OVERCHARGE_COLOR.1, OVERCHARGE_COLOR.2)
        } else {
            Color::rgb(
                CHARGE_INDICATOR_COLOR.0,
                CHARGE_INDICATOR_COLOR.1,
                CHARGE_INDICATOR_COLOR.2,
            )
        };
        ui.draw_rect(
            Rect::new(bar_x, bar_y, bar_width * self.charge_indicator.charge_pct, bar_height),
            fill_color,
        );

        // Draw trajectory preview
        let vx = self.charge_indicator.trajectory_vx;
        let vy = self.charge_indicator.trajectory_vy;

        let dot_color = Color::new(
            TRAJECTORY_COLOR.0,
            TRAJECTORY_COLOR.1,
            TRAJECTORY_COLOR.2,
            150,
        );

        for i in 0..15 {
            let t = i as f32 * 0.05;
            let px = screen_x + vx * t;
            let py = screen_y + vy * t + 0.5 * GRAVITY * t * t;

            if py > SCREEN_HEIGHT as f32 || py < 0.0 {
                break;
            }

            ui.draw_rect(Rect::new(px - 2.0, py - 2.0, 4.0, 4.0), dot_color);
        }
    }
}

impl Scene for GameScene {
    fn name(&self) -> &str {
        "GameScene"
    }

    fn on_enter(&mut self, ctx: &mut SceneContext) {
        ctx.world.clear();
        ctx.camera
            .set_position(SCREEN_WIDTH as f32 / 2.0, SCREEN_HEIGHT as f32 / 2.0);

        // Set up dark ambient lighting
        if let Some(lighting) = ctx.lighting {
            lighting.set_ambient(AmbientLight::new(
                Color::rgb(10, 10, 20), // Very dark blue-black
                0.05, // Very low intensity
            ));
            // Keep UI readable in darkness
            lighting.set_ui_affected_by_lighting(false);
        }

        // Spawn rain particles
        self.spawn_rain(ctx);

        // Rain audio is already playing from menu scene (persists between scenes)

        // Spawn spawn platform
        self.spawn_spawn_platform(ctx);

        // Generate initial platforms above spawn
        self.generate_platforms_as_needed(ctx);

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
        }

        // R or Select button to respawn
        if ctx.input.key_pressed(KeyCode::KeyR)
            || ctx.input.gamepad_button_pressed(GamepadButton::Select)
        {
            match self.mode {
                GameMode::SinglePlayer | GameMode::Host => {
                    self.respawn_local_player(ctx);
                }
                GameMode::Client => {
                    // Send respawn request to server and respawn locally
                    self.send_respawn_request();
                    self.respawn_local_player(ctx);
                }
            }
        }

        if !self.show_quit_dialog {
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

            if self.mode == GameMode::Host
                && let Some(ref lan) = self.lan_ip
            {
                let lan_text = format!("{}:{}", lan, DEFAULT_PORT);
                let (tw, _) = ui.text_size(&lan_text);
                ui.draw_text(
                    &lan_text,
                    sw as f32 - tw as f32 - 20.0,
                    40.0,
                    Color::new(100, 200, 100, 150),
                );
            }
        }

        // Mode indicator (top left)
        let mode_text = match self.mode {
            GameMode::SinglePlayer => "SINGLE PLAYER",
            GameMode::Host => "HOST",
            GameMode::Client => "CLIENT",
        };
        ui.draw_text(mode_text, 20.0, 20.0, Color::new(255, 255, 255, 100));

        // Draw charge indicator using cached state
        self.draw_charge_indicator(ui);

        // Connection status
        if self.mode == GameMode::Client && !self.connected {
            ui.draw_text_anchored("Connecting...", Anchor::Center, 0.0, -50.0, Color::YELLOW);
        } else if self.waiting_for_player && self.mode != GameMode::SinglePlayer {
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
            "Space/A: Charge | Aim: Arrows/Stick | R: Respawn",
            20.0,
            sh as f32 - 30.0,
            Color::LIGHT_GRAY,
        );

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

            if ui.button("Yes, Leave", Rect::new(center_x - btn_w - 20.0, btn_y, btn_w, btn_h)) {
                self.pending_quit = true;
            }

            if ui.button("Cancel", Rect::new(center_x + 20.0, btn_y, btn_w, btn_h)) {
                self.show_quit_dialog = false;
            }
        }
    }

    fn render_gpu(&mut self, ctx: &mut GpuRenderContext) {
        // Upload rain sprite texture
        if let Some(ref sprite) = self.rain_sprite {
            ctx.upload_sprite(sprite);
        }

        // Collect particles for rendering
        self.particle_system.collect_for_render(ctx.world, ctx.camera, &mut ctx.batch);
    }
}
