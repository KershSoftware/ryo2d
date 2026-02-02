//! Menu scene

use crate::save::load_best_time;
use ryo2d::prelude::*;

use super::GameScene;

pub struct MenuScene {
  best_time: f32,
}

impl Default for MenuScene {
  fn default() -> Self {
    Self::new()
  }
}

impl MenuScene {
  pub fn new() -> Self {
    Self {
      best_time: load_best_time(),
    }
  }
}

impl Scene for MenuScene {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    // Clear the world to remove all entities (including bullets with trails)
    ctx.world.clear();
    self.best_time = load_best_time();
  }

  fn update(&mut self, ctx: &mut SceneContext, _dt: f32) -> SceneAction {
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

  fn render_ui(&mut self, ui: &mut UiContext) {
    let (screen_w, screen_h) = ui.screen_size();

    // Clear to dark background
    ui.draw_rect(
      Rect::new(0.0, 0.0, screen_w as f32, screen_h as f32),
      Color::rgb(20, 20, 30),
    );

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
