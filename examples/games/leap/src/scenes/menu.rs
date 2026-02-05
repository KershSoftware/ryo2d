//! Menu scene with single player, host, and join options

use rand::Rng;
use ryo2d::prelude::*;

use crate::constants::*;

use super::GameScene;

/// Generate a random 5-character room code
fn generate_room_code() -> String {
    const CHARS: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";
    let mut rng = rand::rng();
    (0..ROOM_CODE_LENGTH)
        .map(|_| CHARS[rng.random_range(0..CHARS.len())] as char)
        .collect()
}

pub struct MenuScene {
    room_code_input: TextInputState,
    address_input: TextInputState,
    error_message: Option<String>,
    last_dt: f32,
    pending_scene: Option<Box<dyn Scene>>,

    // Rain particles
    rain_emitter: Option<Entity>,
    rain_sprite: Option<SpriteHandle>,
    particle_system: ParticleSystem,
}

impl MenuScene {
    pub fn new() -> Self {
        let mut address_input = TextInputState::new();
        address_input.set_text(DEFAULT_ADDRESS);
        Self {
            room_code_input: TextInputState::new(),
            address_input,
            error_message: None,
            last_dt: 0.016,
            pending_scene: None,
            rain_emitter: None,
            rain_sprite: None,
            particle_system: ParticleSystem::new(),
        }
    }

    fn spawn_rain(&mut self, ctx: &mut SceneContext) {
        // Create a small particle sprite for rain drops
        let rain_sprite = ctx.assets.create_sprite(
            Sprite::solid_rect(2, 8, 180, 200, 255, 200),
            "rain_particle_menu",
        );
        self.rain_sprite = Some(rain_sprite.clone());

        // Create rain emitter covering the screen
        let emitter = ParticleEmitter::rain(rain_sprite)
            .with_mode(EmissionMode::Continuous { rate: 200.0 })
            .with_shape(EmissionShape::Line {
                start_x: -(SCREEN_WIDTH as f32),
                start_y: -50.0,
                end_x: SCREEN_WIDTH as f32 * 2.0,
                end_y: -50.0,
            })
            .with_lifetime(2.0, 3.0)
            .with_gravity(20.0, 500.0);

        let rain_entity = ctx.world.spawn((
            Transform::new(SCREEN_WIDTH as f32 / 2.0, 0.0),
            emitter,
        ));

        self.rain_emitter = Some(rain_entity);
    }

    fn try_single_player(&mut self) {
        match GameScene::new_single_player() {
            Ok(scene) => {
                self.pending_scene = Some(Box::new(scene));
            }
            Err(e) => {
                self.error_message = Some(format!("Failed to start: {}", e));
            }
        }
    }

    fn try_create_room(&mut self) {
        let room_code = generate_room_code();
        match GameScene::new_host(room_code.clone()) {
            Ok(scene) => {
                self.pending_scene = Some(Box::new(scene));
            }
            Err(e) => {
                self.error_message = Some(format!("Failed to create: {}", e));
            }
        }
    }

    fn try_join_room(&mut self) {
        let room_code = self.room_code_input.text().trim().to_uppercase();
        let address = self.address_input.text().trim().to_string();

        if room_code.len() != ROOM_CODE_LENGTH {
            self.error_message = Some(format!("Room code must be {} characters", ROOM_CODE_LENGTH));
            return;
        }

        if address.is_empty() {
            self.error_message = Some("Enter server address".to_string());
            return;
        }

        match GameScene::new_client(&room_code, &address) {
            Ok(scene) => {
                self.pending_scene = Some(Box::new(scene));
            }
            Err(e) => {
                self.error_message = Some(format!("Failed to join: {}", e));
            }
        }
    }
}

impl Scene for MenuScene {
    fn name(&self) -> &str {
        "MenuScene"
    }

    fn on_enter(&mut self, ctx: &mut SceneContext) {
        // Set up dark ambient lighting
        if let Some(lighting) = ctx.lighting {
            lighting.set_ambient(AmbientLight::new(
                Color::rgb(10, 10, 20),
                0.05,
            ));
            lighting.set_ui_affected_by_lighting(false);
        }

        // Spawn rain particles
        self.spawn_rain(ctx);

        // Play rain ambient sound (persists between scenes with play_music_once)
        if let Some(audio) = &mut ctx.audio {
            let _ = audio.play_music_once(
                "assets/rain.mp3",
                MusicOptions::new().volume(0.4).looping(true),
            );
        }
    }

    fn update(&mut self, ctx: &mut SceneContext, dt: f32) -> SceneAction {
        self.last_dt = dt;

        // Update particle system
        self.particle_system.update(ctx.world, ctx.events, dt);

        // Check for pending scene switch
        if let Some(scene) = self.pending_scene.take() {
            return SceneAction::Switch(scene);
        }

        // ESC or gamepad B/Start to quit
        if ctx.input.key_pressed(KeyCode::Escape)
            || ctx.input.gamepad_button_pressed(GamepadButton::East)
            || ctx.input.gamepad_button_pressed(GamepadButton::Start)
        {
            return SceneAction::Quit;
        }

        // Gamepad A button for quick single player start (when no text input focused)
        if !self.room_code_input.is_focused()
            && !self.address_input.is_focused()
            && ctx.input.gamepad_button_pressed(GamepadButton::South)
        {
            self.try_single_player();
        }

        // Enter to join if inputs have content
        let enter_pressed = ctx.input.key_pressed(KeyCode::Enter);
        let either_focused = self.room_code_input.is_focused() || self.address_input.is_focused();
        let has_code = self.room_code_input.text().len() == ROOM_CODE_LENGTH;
        let has_address = !self.address_input.text().trim().is_empty();

        if enter_pressed && either_focused && has_code && has_address {
            self.try_join_room();
        }

        SceneAction::Continue
    }

    fn render_ui(&mut self, ui: &mut UiContext) {
        let dt = self.last_dt;
        let (sw, sh) = ui.screen_size();
        let center_x = sw as f32 / 2.0;
        let center_y = sh as f32 / 2.0;

        // Background (black)
        ui.draw_rect(
            Rect::new(0.0, 0.0, sw as f32, sh as f32),
            Color::rgb(BACKGROUND_COLOR.0, BACKGROUND_COLOR.1, BACKGROUND_COLOR.2),
        );

        // Title
        ui.draw_text_anchored("LEAP", Anchor::TopCenter, 0.0, 60.0, Color::WHITE);
        ui.draw_text_anchored(
            "Charge. Aim. Release.",
            Anchor::TopCenter,
            0.0,
            90.0,
            Color::GRAY,
        );

        // Single Player button
        let button_width = 200.0;
        let button_height = 40.0;
        let button_x = center_x - button_width / 2.0;

        let single_rect = Rect::new(button_x, center_y - 80.0, button_width, button_height);
        if ui.button("Single Player", single_rect) {
            self.try_single_player();
        }

        // Create Room button
        let create_rect = Rect::new(button_x, center_y - 30.0, button_width, button_height);
        if ui.button("Create Room", create_rect) {
            self.try_create_room();
        }

        // Separator
        ui.draw_text_anchored("- or join -", Anchor::Center, 0.0, 25.0, Color::GRAY);

        // Join section - Room Code
        ui.draw_text_anchored("Room Code:", Anchor::Center, -80.0, 55.0, Color::WHITE);

        let code_input_width = 100.0;
        let code_input_rect = Rect::new(
            center_x - 80.0 - code_input_width / 2.0,
            center_y + 75.0,
            code_input_width,
            32.0,
        );
        ui.text_input(&mut self.room_code_input, code_input_rect, "XXXXX", dt);

        // Join section - Server Address
        ui.draw_text_anchored("Address:", Anchor::Center, 80.0, 55.0, Color::WHITE);

        let addr_input_width = 160.0;
        let addr_input_rect = Rect::new(
            center_x + 80.0 - addr_input_width / 2.0,
            center_y + 75.0,
            addr_input_width,
            32.0,
        );
        ui.text_input(&mut self.address_input, addr_input_rect, DEFAULT_ADDRESS, dt);

        // Join button
        let join_rect = Rect::new(button_x, center_y + 125.0, button_width, button_height);
        if ui.button("Join Room", join_rect) {
            self.try_join_room();
        }

        // Error message
        if let Some(ref err) = self.error_message {
            ui.draw_text_anchored(err, Anchor::Center, 0.0, 190.0, Color::RED);
        }

        // Controls hint
        ui.draw_text_anchored(
            "Controls: Space/A = Charge, Arrows/Stick = Aim",
            Anchor::BottomCenter,
            0.0,
            -40.0,
            Color::DARK_GRAY,
        );

        // ESC hint
        ui.draw_text_anchored("ESC to quit", Anchor::BottomCenter, 0.0, -20.0, Color::DARK_GRAY);
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
