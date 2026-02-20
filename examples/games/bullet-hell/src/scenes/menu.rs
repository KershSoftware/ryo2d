//! Menu scene

use crate::save::load_best_time;
use crate::shared;
use ryo2d::prelude::*;

use super::GameScene;

pub struct MenuScene {
  best_time: f32,
  background: GpuParallaxBackground,
}

impl Default for MenuScene {
  fn default() -> Self {
    Self::new()
  }
}

impl MenuScene {
  pub fn new() -> Self {
    // Load parallax background layers as sprite handles (for GPU upload)
    let sky_sprite = SpriteHandle::from_sprite(
      Sprite::load("assets/desert_sky.png").expect("Failed to load sky sprite"),
      "desert_sky",
    );
    let moon_sprite = SpriteHandle::from_sprite(
      Sprite::load("assets/desert_moon.png").expect("Failed to load moon sprite"),
      "desert_moon",
    );
    let cloud_sprite = SpriteHandle::from_sprite(
      Sprite::load("assets/desert_cloud.png").expect("Failed to load cloud sprite"),
      "desert_cloud",
    );
    let mountain_sprite = SpriteHandle::from_sprite(
      Sprite::load("assets/desert_mountain.png").expect("Failed to load mountain sprite"),
      "desert_mountain",
    );
    let dune_mid_sprite = SpriteHandle::from_sprite(
      Sprite::load("assets/desert_dunemid.png").expect("Failed to load mid dune sprite"),
      "desert_dunemid",
    );
    let dune_front_sprite = SpriteHandle::from_sprite(
      Sprite::load("assets/desert_dunefrontt.png").expect("Failed to load front dune sprite"),
      "desert_dunefront",
    );

    // Scale factor to fit 1000px sprites to 1080px screen height
    let scale = 1080.0 / 1000.0;

    // Build GPU-accelerated parallax background with desert layers
    let background = GpuParallaxBuilder::new()
      .static_layer(&sky_sprite, -100)
      .with_scale(scale)
      .layer(&moon_sprite, 0.0, -90)
      .with_scale(scale)
      .with_auto_scroll(10.0)
      .with_tile_mode(TileMode::InfiniteScroll)
      .layer(&cloud_sprite, 0.0, -80)
      .with_scale(scale)
      .with_auto_scroll(30.0)
      .with_tile_mode(TileMode::InfiniteScroll)
      .layer(&mountain_sprite, 0.0, -60)
      .with_scale(scale)
      .with_auto_scroll(60.0)
      .with_tile_mode(TileMode::InfiniteScroll)
      .layer(&dune_mid_sprite, 0.0, -40)
      .with_scale(scale)
      .with_auto_scroll(100.0)
      .with_tile_mode(TileMode::InfiniteScroll)
      .layer(&dune_front_sprite, 0.0, -20)
      .with_scale(scale)
      .with_auto_scroll(160.0)
      .with_tile_mode(TileMode::InfiniteScroll)
      .build();

    Self {
      best_time: load_best_time(),
      background,
    }
  }
}

impl Scene for MenuScene {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    // Clear the world to remove all entities (including bullets with trails)
    ctx.world.clear();
    self.best_time = load_best_time();

    // Declare game commands (idempotent - can be called multiple times)
    ctx.declare_console_command(
      "game:reset",
      "Reset the game to initial state",
      Some("Resets all game state including score, player position, and enemies. Only available during gameplay."),
    );

    // Sync parallax background with shared time (persists across scenes)
    self
      .background
      .set_elapsed_time(shared::get_parallax_time());

    // Start background music (only if not already playing - persists across scenes)
    if let Some(audio) = &mut ctx.audio {
      let _ = audio.play_music_once("assets/music.mp3", MusicOptions::new().volume(0.4));
    }
  }

  fn update(&mut self, ctx: &mut SceneContext, dt: f32) -> SceneAction {
    // Update shared parallax time (persists between menu and game)
    let parallax_time = shared::add_parallax_time(dt);
    self.background.set_elapsed_time(parallax_time);

    // Start game (keyboard or gamepad)
    if ctx.input.key_pressed(KeyCode::Space)
      || ctx.input.key_pressed(KeyCode::Enter)
      || ctx.input.gamepad_button_pressed(GamepadButton::South)
      || ctx.input.gamepad_button_pressed(GamepadButton::Start)
    {
      return SceneAction::Switch(Box::new(GameScene::new()));
    }
    // Quit to desktop
    if ctx.input.key_pressed(KeyCode::Escape) {
      return SceneAction::Quit;
    }
    SceneAction::Continue
  }

  fn render_gpu(&mut self, ctx: &mut GpuRenderContext) {
    self.background.render(ctx);
  }

  fn render_ui(&mut self, ui: &mut UiContext) -> SceneAction {
    // Title
    ui.draw_text_anchored("BULLET HELL", Anchor::TopCenter, 0.0, 150.0, Color::rgb(255, 50, 50));

    // Controls - Keyboard
    let keyboard_controls = vec![
      ("CONTROLS", Color::rgb(255, 200, 100)),
      ("", Color::rgb(200, 200, 200)),
      ("Keyboard:", Color::rgb(150, 200, 255)),
      (
        "WASD / Arrows - Move    SPACE - Jump",
        Color::rgb(200, 200, 200),
      ),
      ("", Color::rgb(200, 200, 200)),
      ("Controller:", Color::rgb(150, 200, 255)),
      (
        "Left Stick / D-Pad - Move    A / X - Jump",
        Color::rgb(200, 200, 200),
      ),
      ("", Color::rgb(200, 200, 200)),
      ("Survive as long as you can!", Color::rgb(200, 200, 200)),
      (
        "Hide behind platforms to avoid bullets.",
        Color::rgb(200, 200, 200),
      ),
    ];

    let mut y = 250.0;
    for (line, color) in keyboard_controls {
      ui.draw_text_anchored(line, Anchor::TopCenter, 0.0, y, color);
      y += 22.0;
    }

    // Best time
    if self.best_time > 0.0 {
      let best_text = format!("Best Time: {:.1}s", self.best_time);
      ui.draw_text_anchored(&best_text, Anchor::TopCenter, 0.0, y + 20.0, Color::rgb(255, 215, 0));
    }

    // Start prompt
    ui.draw_text_anchored(
      "Press SPACE / A to Start",
      Anchor::TopCenter,
      0.0,
      520.0,
      Color::rgb(100, 255, 100),
    );

    // Quit hint
    ui.draw_text_anchored(
      "ESC to Quit",
      Anchor::TopCenter,
      0.0,
      550.0,
      Color::rgb(150, 150, 150),
    );
    SceneAction::Continue
  }
}
