// Windows: Hide the console window in release builds.
// In debug builds, the console remains visible for println! output.
// Remove this line if you need console output in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! My Ryo2D Game
//!
//! A game built with the Ryo2D engine.

use ryo2d::prelude::*;

/// Main game scene
struct GameScene;

impl Scene for GameScene {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    // Initialize your game here
    // Spawn entities, load assets, etc.
    
    // Example: Spawn a simple colored square at center
    let sprite = ctx.assets.create_sprite(
      Sprite::solid_rect(64, 64, 0, 255, 255, 255),
      "square"
    );
    ctx.world.spawn((
      Transform::new(640.0, 360.0),
      Renderable::sprite(sprite).with_z_order(0),
    ));
  }

  fn update(&mut self, ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    // Update game logic here
    
    // Example: Quit on Escape
    if ctx.input.key_pressed(KeyCode::Escape) {
      return SceneAction::Quit;
    }

    SceneAction::Continue
  }

  fn render_ui(&mut self, ui: &mut UiContext) {
    // Draw UI here
    ui.label(
      "My Ryo2D Game",
      10.0,
      10.0,
      Color::WHITE,
      Some(Color::new(0, 0, 0, 180)),
    );
  }
}

fn main() {
  let engine = EngineBuilder::new()
    .title("My Ryo2D Game")
    .resolution(1280, 720)
    .fps(60)
    .with_scene(GameScene)
    .build();

  if let Err(e) = engine.run() {
    eprintln!("Engine error: {}", e);
    std::process::exit(1);
  }
}
