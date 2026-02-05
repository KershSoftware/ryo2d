//! Menu Scene - Starting screen for the game

use ryo2d::prelude::*;
use std::time::Duration;

use super::{DebugScene, GameScene};
use crate::components::Scrolling;
use crate::constants::*;
use crate::save::load_high_score;

/// Pre-loaded sprite handles for game assets.
/// Loaded once in MenuScene::new() before the window appears.
struct PreloadedAssets {
  background: Sprite,
  ground: Sprite,
  pipe_cap: Sprite,
  pipe_body: Sprite,
}

impl PreloadedAssets {
  /// Load all PNG assets upfront during engine initialization.
  /// This prevents the white flash that would occur if we loaded during on_enter.
  fn load() -> Self {
    Self {
      background: Sprite::load("assets/bg.png").expect("Failed to load bg.png"),
      ground: Sprite::load("assets/bg-ground.png").expect("Failed to load bg-ground.png"),
      pipe_cap: Sprite::load("assets/pipe_cap.png").expect("Failed to load pipe_cap.png"),
      pipe_body: Sprite::load("assets/pipe_body.png").expect("Failed to load pipe_body.png"),
    }
  }

  /// Register preloaded sprites with the AssetManager for caching.
  /// Call this in on_enter to populate the cache so other scenes can use ctx.assets.load_sprite().
  fn register_with_cache(self, assets: &mut AssetManager) {
    // Create handles and register them - these will be cached by path
    let bg_handle = SpriteHandle::from_sprite(self.background, "assets/bg.png");
    assets.cache_sprite_handle(bg_handle);

    let ground_handle = SpriteHandle::from_sprite(self.ground, "assets/bg-ground.png");
    assets.cache_sprite_handle(ground_handle);

    let cap_handle = SpriteHandle::from_sprite(self.pipe_cap, "assets/pipe_cap.png");
    assets.cache_sprite_handle(cap_handle);

    let body_handle = SpriteHandle::from_sprite(self.pipe_body, "assets/pipe_body.png");
    assets.cache_sprite_handle(body_handle);
  }
}

pub struct MenuScene {
  high_score: u32,
  start_clicked: bool,
  debug_clicked: bool,
  debug_enabled: bool,
  bird_flap_up: Sprite,
  bird_flap_down: Sprite,
  anim_timer: f32,
  anim_frame: bool,
  background_entities: Vec<Entity>,
  /// Pre-loaded PNG assets (loaded before window appears, registered on first on_enter)
  preloaded_assets: Option<PreloadedAssets>,
}

impl MenuScene {
  pub fn new() -> Self {
    // Check if debug mode is enabled in Ryo.toml
    let debug_enabled = Workspace::discover()
      .ok()
      .and_then(|ws| ws.config().env_bool("debug_scene"))
      .unwrap_or(false);

    // Load bird sprites for animation (SVG - not cacheable via AssetManager)
    let bird_flap_up =
      Sprite::load_svg_fit("assets/bird-flap-up.svg", 64, 64).expect("Failed to load bird sprite");
    let bird_flap_down = Sprite::load_svg_fit("assets/bird-flap-down.svg", 64, 64)
      .expect("Failed to load bird sprite");

    // Pre-load all PNG assets before window appears (prevents long white flash)
    let preloaded_assets = PreloadedAssets::load();

    Self {
      high_score: load_high_score(),
      start_clicked: false,
      debug_clicked: false,
      debug_enabled,
      bird_flap_up,
      bird_flap_down,
      anim_timer: 0.0,
      anim_frame: false,
      background_entities: Vec::new(),
      preloaded_assets: Some(preloaded_assets),
    }
  }
}

impl Default for MenuScene {
  fn default() -> Self {
    Self::new()
  }
}

impl Scene for MenuScene {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    ctx.world.clear();
    ctx
      .camera
      .set_position(SCREEN_WIDTH as f32 / 2.0, SCREEN_HEIGHT as f32 / 2.0);

    // Register pre-loaded assets with the cache (loaded in new(), before window appeared)
    // This makes load_sprite() calls instant for other scenes
    if let Some(preloaded) = self.preloaded_assets.take() {
      preloaded.register_with_cache(ctx.assets);
    }

    // Start background music (persists across scenes)
    if let Some(audio) = &mut ctx.audio {
      audio.set_master_volume(0.02);

      let options = MusicOptions::new().looping(true);
      if let Err(e) = audio.play_music_once("assets/8bit-bg.mp3", options) {
        eprintln!("Failed to play background music: {}", e);
      }
    }

    // Spawn scrolling background (uses cached assets)
    self.spawn_background(ctx);
  }

  fn on_exit(&mut self, _ctx: &mut SceneContext) {
    self.background_entities.clear();
  }

  fn update(&mut self, ctx: &mut SceneContext, dt: f32) -> SceneAction {
    // Update scrolling background
    for &entity in &self.background_entities {
      if let Some(scrolling) = ctx.world.get::<Scrolling>(entity)
        && let Some(mut transform) = ctx.world.get_mut::<Transform>(entity)
      {
        let new_x = scrolling.update(transform.x, dt);
        transform.x = new_x;
      }
    }

    // Animate bird preview
    self.anim_timer += dt;
    if self.anim_timer >= BIRD_FLAP_FRAME_TIME {
      self.anim_timer = 0.0;
      self.anim_frame = !self.anim_frame;
    }

    // Start game on Space, Enter, or button click
    if ctx.input.key_pressed(KeyCode::Space)
      || ctx.input.key_pressed(KeyCode::Enter)
      || self.start_clicked
    {
      self.start_clicked = false;
      return SceneAction::SwitchWith(
        Box::new(GameScene::new(self.high_score)),
        Transition::fade(Duration::from_millis(300)),
      );
    }

    // Debug mode on D key or debug button click (only if enabled)
    if self.debug_enabled && (ctx.input.key_pressed(KeyCode::KeyD) || self.debug_clicked) {
      self.debug_clicked = false;
      return SceneAction::SwitchWith(
        Box::new(DebugScene::new()),
        Transition::fade(Duration::from_millis(300)),
      );
    }

    SceneAction::Continue
  }

  fn render_ui(&mut self, ui: &mut UiContext) {
    let (screen_w, screen_h) = ui.screen_size();

    // Title
    let title = "FLAPPY";
    let (title_w, _) = ui.text_size(title);
    ui.draw_text(
      title,
      screen_w as f32 / 2.0 - title_w as f32 / 2.0,
      100.0,
      Color::WHITE,
    );

    // High score (upper right)
    if self.high_score > 0 {
      let high_score_text = format!("Best: {}", self.high_score);
      let (text_w, _) = ui.text_size(&high_score_text);
      ui.draw_text(
        &high_score_text,
        screen_w as f32 - text_w as f32 - 16.0,
        16.0,
        Color::rgb(80, 80, 80),
      );
    }

    // Bird preview (centered below title, animated)
    let bird_sprite = if self.anim_frame {
      &self.bird_flap_up
    } else {
      &self.bird_flap_down
    };
    let bird_w = bird_sprite.width() as f32;
    let bird_x = screen_w as f32 / 2.0 - bird_w / 2.0;
    let bird_y = 180.0;
    ui.draw_sprite(bird_sprite, bird_x, bird_y);

    // Start button
    let button_w = 200.0;
    let button_h = 60.0;
    let button_x = screen_w as f32 / 2.0 - button_w / 2.0;
    let button_y = screen_h as f32 / 2.0;

    if ui.button("START", Rect::new(button_x, button_y, button_w, button_h)) {
      self.start_clicked = true;
    }

    // Debug button (smaller, below start) - only if debug mode enabled
    if self.debug_enabled {
      let debug_button_w = 120.0;
      let debug_button_h = 40.0;
      let debug_button_x = screen_w as f32 / 2.0 - debug_button_w / 2.0;
      let debug_button_y = button_y + button_h + 20.0;

      if ui.button(
        "DEBUG",
        Rect::new(
          debug_button_x,
          debug_button_y,
          debug_button_w,
          debug_button_h,
        ),
      ) {
        self.debug_clicked = true;
      }
    }

    // Instructions
    let instructions = if self.debug_enabled {
      "Press SPACE or CLICK to flap | D for debug"
    } else {
      "Press SPACE or CLICK to flap"
    };
    let (text_w, _) = ui.text_size(instructions);
    ui.draw_text(
      instructions,
      screen_w as f32 / 2.0 - text_w as f32 / 2.0,
      screen_h as f32 - 100.0,
      Color::rgb(100, 100, 100),
    );
  }
}

impl MenuScene {
  fn spawn_background(&mut self, ctx: &mut SceneContext) {
    // Spawn two background segments for seamless parallax scrolling
    // Menu background scrolls faster than game for visual appeal
    let bg_handle = ctx
      .assets
      .load_sprite("assets/bg.png")
      .expect("Failed to load background sprite");
    let bg_width = bg_handle.width() as f32;
    let bg_height = bg_handle.height() as f32;
    let tile_width = bg_width;
    let wrap_distance = bg_width * 2.0;

    // Scale background to fill screen height while maintaining aspect ratio
    let screen_height = SCREEN_HEIGHT as f32;
    let scale = screen_height / bg_height;

    for i in 0..2 {
      let x = i as f32 * bg_width * scale;
      let mut transform = Transform::new(x, 0.0);
      transform.scale_x = scale;
      transform.scale_y = scale;
      let background = ctx.world.spawn((
        transform,
        Renderable::sprite(bg_handle.clone()).with_z_order(0), // Render behind everything
        Scrolling::new(
          MENU_BACKGROUND_SCROLL_SPEED,
          tile_width * scale,
          wrap_distance * scale,
        ),
      ));
      self.background_entities.push(background);
    }
  }
}
