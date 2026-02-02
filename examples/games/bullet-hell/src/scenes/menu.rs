//! Menu scene

use crate::save::load_best_time;
use ryo2d::prelude::*;

use super::GameScene;

// Fixed design resolution (same as game scene)
const DESIGN_WIDTH: f32 = 1920.0;
const DESIGN_HEIGHT: f32 = 1080.0;

pub struct MenuScene {
  best_time: f32,
  background: ParallaxBackground,
}

impl Default for MenuScene {
  fn default() -> Self {
    Self::new()
  }
}

impl MenuScene {
  pub fn new() -> Self {
    // Load parallax background layers (same as game scene)
    let sky_sprite = Sprite::load("assets/desert_sky.png").expect("Failed to load sky sprite");
    let moon_sprite = Sprite::load("assets/desert_moon.png").expect("Failed to load moon sprite");
    let cloud_sprite =
      Sprite::load("assets/desert_cloud.png").expect("Failed to load cloud sprite");
    let mountain_sprite =
      Sprite::load("assets/desert_mountain.png").expect("Failed to load mountain sprite");
    let dune_mid_sprite =
      Sprite::load("assets/desert_dunemid.png").expect("Failed to load mid dune sprite");
    let dune_front_sprite =
      Sprite::load("assets/desert_dunefrontt.png").expect("Failed to load front dune sprite");

    // Build parallax background with desert layers
    let background = ParallaxBackground::builder(DESIGN_WIDTH as u32, DESIGN_HEIGHT as u32)
      .scale_to_fit()
      .static_layer(SpriteHandle::from_sprite(sky_sprite, "desert_sky"), -100)
      .auto_scroll_layer(
        SpriteHandle::from_sprite(moon_sprite, "desert_moon"),
        5.0,
        -90,
      )
      .auto_scroll_layer(
        SpriteHandle::from_sprite(cloud_sprite, "desert_cloud"),
        15.0,
        -80,
      )
      .auto_scroll_layer(
        SpriteHandle::from_sprite(mountain_sprite, "desert_mountain"),
        30.0,
        -60,
      )
      .auto_scroll_layer(
        SpriteHandle::from_sprite(dune_mid_sprite, "desert_dunemid"),
        50.0,
        -40,
      )
      .auto_scroll_layer(
        SpriteHandle::from_sprite(dune_front_sprite, "desert_dunefront"),
        80.0,
        -20,
      )
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

    // Start background music (only if not already playing - persists across scenes)
    if let Some(audio) = &mut ctx.audio {
      let _ = audio.play_music_once("assets/music.mp3", MusicOptions::new().volume(0.4));
    }
  }

  fn update(&mut self, ctx: &mut SceneContext, dt: f32) -> SceneAction {
    // Update parallax background
    self.background.update(dt);

    // Start game
    if ctx.input.key_pressed(KeyCode::Space) || ctx.input.key_pressed(KeyCode::Enter) {
      return SceneAction::Switch(Box::new(GameScene::new()));
    }
    // Quit to desktop
    if ctx.input.key_pressed(KeyCode::Escape) {
      return SceneAction::Quit;
    }
    SceneAction::Continue
  }

  fn render_background(&mut self, frame: &mut [u8], width: u32, height: u32, camera: &Camera) {
    self.background.render(frame, width, height, camera);
  }

  fn render_ui(&mut self, ui: &mut UiContext) {
    let (screen_w, _screen_h) = ui.screen_size();

    let center_x = screen_w as f32 / 2.0;

    // Title
    let title = "BULLET HELL";
    let (title_w, _) = ui.text_size(title);
    ui.draw_text(
      title,
      center_x - title_w as f32 / 2.0,
      150.0,
      Color::rgb(255, 50, 50),
    );

    // Subtitle
    let subtitle = "GPU Batching Demo";
    let (subtitle_w, _) = ui.text_size(subtitle);
    ui.draw_text(
      subtitle,
      center_x - subtitle_w as f32 / 2.0,
      180.0,
      Color::rgb(150, 150, 150),
    );

    // Controls
    let controls = vec![
      "WASD / Arrow Keys - Move",
      "SPACE - Jump",
      "",
      "Survive as long as you can!",
      "Hide behind platforms to avoid bullets.",
    ];

    let mut y = 280.0;
    for line in controls {
      let (line_w, _) = ui.text_size(line);
      ui.draw_text(
        line,
        center_x - line_w as f32 / 2.0,
        y,
        Color::rgb(200, 200, 200),
      );
      y += 25.0;
    }

    // Best time
    if self.best_time > 0.0 {
      let best_text = format!("Best Time: {:.1}s", self.best_time);
      let (best_w, _) = ui.text_size(&best_text);
      ui.draw_text(
        &best_text,
        center_x - best_w as f32 / 2.0,
        450.0,
        Color::rgb(255, 215, 0),
      );
    }

    // Start prompt
    let prompt = "Press SPACE to Start";
    let (prompt_w, _) = ui.text_size(prompt);
    ui.draw_text(
      prompt,
      center_x - prompt_w as f32 / 2.0,
      520.0,
      Color::rgb(100, 255, 100),
    );

    // Quit hint
    let quit_hint = "Press ESC to Quit";
    let (quit_w, _) = ui.text_size(quit_hint);
    ui.draw_text(
      quit_hint,
      center_x - quit_w as f32 / 2.0,
      550.0,
      Color::rgb(150, 150, 150),
    );
  }
}
