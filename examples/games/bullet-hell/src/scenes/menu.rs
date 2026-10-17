//! Menu scene

use crate::look::use_game_look;
use crate::save::load_best_time;
use crate::shared;
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
    use_game_look(ctx);

    self.best_time = load_best_time();

    // Declare game commands (idempotent - can be called multiple times)
    ctx.console().declare(
      "game:reset",
      "Reset the game to initial state",
      Some("Resets all game state including score, player position, and enemies. Only available during gameplay."),
    );

    // The backdrop carries on from the shared parallax time
    shared::spawn_background(ctx);

    // Start background music (only if not already playing - persists across scenes)
    let _ = ctx
      .audio
      .play_music_once("assets/music.mp3", MusicOptions::new().volume(0.4));
  }

  fn update(&mut self, ctx: &mut SceneContext, dt: f32) -> SceneAction {
    // The shared parallax time carries the backdrop into the next scene
    shared::add_parallax_time(dt);

    // Start game: Confirm (Space, Enter, A), or Start on a pad
    if ctx.input.action_pressed("ui.confirm")
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

  fn ui(&mut self, ui: &mut UiContext, _ctx: &mut SceneContext) -> SceneAction {
    // Title
    ui.draw_text_anchored(
      "BULLET HELL",
      Anchor::TopCenter,
      0.0,
      150.0,
      Color::rgb(255, 50, 50),
    );

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
      ui.draw_text_anchored(
        &best_text,
        Anchor::TopCenter,
        0.0,
        y + 20.0,
        Color::rgb(255, 215, 0),
      );
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
