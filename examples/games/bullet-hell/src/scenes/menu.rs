//! Menu scene

use crate::save::load_best_time;
use ryo2d::prelude::*;

use super::GameScene;

pub struct MenuScene {
  best_time: f32,
  background: GpuParallaxBackground,
  parallax_sprites: Vec<SpriteHandle>,
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

    // Store sprite handles for GPU upload
    let parallax_sprites = vec![
      sky_sprite.clone(),
      moon_sprite.clone(),
      cloud_sprite.clone(),
      mountain_sprite.clone(),
      dune_mid_sprite.clone(),
      dune_front_sprite.clone(),
    ];

    // Build GPU-accelerated parallax background with desert layers
    let mut background = GpuParallaxBackground::new();

    // Static sky layer
    background.add_layer(GpuParallaxLayer::new(&sky_sprite, 0.0, -100));

    // Auto-scrolling layers with infinite scroll
    background.add_layer(
      GpuParallaxLayer::new(&moon_sprite, 0.0, -90)
        .with_auto_scroll(5.0)
        .with_tile_mode(TileMode::InfiniteScroll),
    );
    background.add_layer(
      GpuParallaxLayer::new(&cloud_sprite, 0.0, -80)
        .with_auto_scroll(15.0)
        .with_tile_mode(TileMode::InfiniteScroll),
    );
    background.add_layer(
      GpuParallaxLayer::new(&mountain_sprite, 0.0, -60)
        .with_auto_scroll(30.0)
        .with_tile_mode(TileMode::InfiniteScroll),
    );
    background.add_layer(
      GpuParallaxLayer::new(&dune_mid_sprite, 0.0, -40)
        .with_auto_scroll(50.0)
        .with_tile_mode(TileMode::InfiniteScroll),
    );
    background.add_layer(
      GpuParallaxLayer::new(&dune_front_sprite, 0.0, -20)
        .with_auto_scroll(80.0)
        .with_tile_mode(TileMode::InfiniteScroll),
    );

    Self {
      best_time: load_best_time(),
      background,
      parallax_sprites,
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

  fn render_background(&mut self, _frame: &mut [u8], _width: u32, _height: u32, _camera: &Camera) {
    // Parallax rendering moved to render_gpu for GPU acceleration
  }

  fn render_gpu(&mut self, ctx: &mut GpuRenderContext) {
    // Upload parallax textures to GPU (only happens once, cached after)
    for sprite in &self.parallax_sprites {
      ctx.upload_sprite(sprite);
    }

    // Render parallax background using GPU instancing
    self.background.render(ctx);
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
