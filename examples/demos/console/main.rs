//! Console Demo - Ryo2D Example
//!
//! Demonstrates the two-phase command system, scene switching, and command scoping.
//!
//! ## Controls
//!
//! - `~`: Open developer console
//! - Try `help game` to see all game commands
//! - `game:hello [name]`: Prints a greeting (works in both scenes)
//! - `game:scene1` / `game:scene2`: Switch scenes
//! - `game:paint <color>`: Change background color (Scene 2 only)

use std::sync::atomic::{AtomicU8, Ordering};

use ryo2d::prelude::*;

// Cross-scene communication via atomics (console handlers can't return SceneAction)
static SCENE_REQUEST: AtomicU8 = AtomicU8::new(0); // 0=none, 1=scene1, 2=scene2
static PAINT_R: AtomicU8 = AtomicU8::new(40);
static PAINT_G: AtomicU8 = AtomicU8::new(70);
static PAINT_B: AtomicU8 = AtomicU8::new(40);

fn main() {
  let demo = ConsoleDemo {
    scenes: SceneGame::new(Box::new(HubScene)),
  };

  EngineBuilder::new()
    .title("Console Demo")
    .resolution(800, 600)
    .with_game(demo)
    .build()
    .run()
    .unwrap();
}

// ============================================================================
// ConsoleDemo — thin Game wrapper around SceneGame
// ============================================================================

struct ConsoleDemo {
  scenes: SceneGame,
}

impl Game for ConsoleDemo {
  fn init(&mut self, ctx: &mut GameContext) {
    // Declare ALL commands up front (visible in help immediately)
    ctx.declare_console_command(
      "game:hello",
      "Print a greeting",
      Some(
        "Usage: game:hello [name]\n\nPrints 'Hello, <name>!' or 'Hello, World!' if no name given.",
      ),
    );
    ctx.declare_console_command("game:scene1", "Switch to Scene 1 (Hub)", None::<&str>);
    ctx.declare_console_command("game:scene2", "Switch to Scene 2 (Workshop)", None::<&str>);
    ctx.declare_console_command(
      "game:paint",
      "Change background color (Scene 2 only)",
      Some("Usage: game:paint <color>\n\nColors: red, green, blue, yellow, purple, reset\nOnly has effect in Scene 2."),
    );

    // Register PERMANENT handlers (scope 0) for commands that work everywhere
    ctx.register_console_command("game:hello", |args, cmd_ctx| {
      let name = if args.is_empty() { "World" } else { args[0] };
      cmd_ctx.console.print(&format!("Hello, {}!", name));
      CommandResult::Ok
    });

    ctx.register_console_command("game:scene1", |_args, cmd_ctx| {
      SCENE_REQUEST.store(1, Ordering::Relaxed);
      cmd_ctx.console.print("Switching to Scene 1...");
      CommandResult::Ok
    });

    ctx.register_console_command("game:scene2", |_args, cmd_ctx| {
      SCENE_REQUEST.store(2, Ordering::Relaxed);
      cmd_ctx.console.print("Switching to Scene 2...");
      CommandResult::Ok
    });

    // Note: game:paint is declared but NOT registered here.
    // It will be registered as a scoped handler in Scene 2's on_enter.

    // Delegate to SceneGame for scene setup
    self.scenes.init(ctx);
  }

  fn update(&mut self, ctx: &mut GameContext, dt: f32) {
    self.scenes.update(ctx, dt);
  }

  fn render_background(&mut self, frame: &mut [u8], width: u32, height: u32, camera: &Camera) {
    self.scenes.render_background(frame, width, height, camera);
  }

  fn render_ui(&mut self, ui: &mut UiContext) {
    self.scenes.render_ui(ui);
  }

  fn get_overlay(&self) -> Option<(f32, [u8; 4])> {
    self.scenes.get_overlay()
  }

  fn is_physics_paused(&self) -> bool {
    self.scenes.is_physics_paused()
  }

  fn should_quit(&self) -> bool {
    self.scenes.should_quit()
  }
}

// ============================================================================
// Scene 1 — Hub
// ============================================================================

struct HubScene;

impl Scene for HubScene {
  fn update(&mut self, ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    // Check for scene switch request from console command
    let req = SCENE_REQUEST.swap(0, Ordering::Relaxed);
    if req == 2 {
      return SceneAction::Switch(Box::new(WorkshopScene));
    }

    if ctx.input.key_pressed(KeyCode::Escape) {
      return SceneAction::Quit;
    }

    SceneAction::Continue
  }

  fn render_background(&mut self, frame: &mut [u8], width: u32, height: u32, _camera: &Camera) {
    // Dark blue background
    let r: u8 = 20;
    let g: u8 = 30;
    let b: u8 = 60;
    for y in 0..height {
      for x in 0..width {
        let idx = ((y * width + x) * 4) as usize;
        frame[idx] = r;
        frame[idx + 1] = g;
        frame[idx + 2] = b;
        frame[idx + 3] = 255;
      }
    }
  }

  fn render_ui(&mut self, ui: &mut UiContext) {
    // Title
    ui.draw_text("Scene 1 — Hub", 30.0, 20.0, Color::WHITE);

    // Instructions
    ui.draw_text(
      "Press ~ to open the developer console",
      30.0,
      60.0,
      Color::rgb(150, 170, 200),
    );
    ui.draw_text(
      "Try: help game, game:hello, game:scene2",
      30.0,
      85.0,
      Color::rgb(120, 140, 170),
    );

    // Scene 2 button
    let btn_rect = Rect::new(30.0, 130.0, 200.0, 40.0);
    if ui.button("Go to Scene 2", btn_rect) {
      SCENE_REQUEST.store(2, Ordering::Relaxed);
    }

    // Status panel
    let panel = Rect::new(30.0, 200.0, 400.0, 100.0);
    ui.draw_rect(panel, Color::rgb(15, 20, 40));
    ui.draw_rect_outline(panel, Color::rgb(50, 60, 90), 1.0);

    ui.draw_text("Active Commands", 40.0, 210.0, Color::rgb(150, 200, 255));
    ui.draw_text(
      "game:hello — active (permanent)",
      40.0,
      235.0,
      Color::rgb(100, 255, 100),
    );
    ui.draw_text(
      "game:scene1 / game:scene2 — active (permanent)",
      40.0,
      255.0,
      Color::rgb(100, 255, 100),
    );
    ui.draw_text(
      "game:paint — inactive (Scene 2 only)",
      40.0,
      275.0,
      Color::rgb(255, 100, 100),
    );

    // Footer
    ui.draw_text("Press ESC to exit", 30.0, 560.0, Color::rgb(100, 100, 100));
  }

  fn name(&self) -> &str {
    "Hub"
  }
}

// ============================================================================
// Scene 2 — Workshop
// ============================================================================

struct WorkshopScene;

impl Scene for WorkshopScene {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    // Register scoped handler for game:paint (auto-cleaned on scene exit)
    ctx.register_console_command("game:paint", |args, cmd_ctx| {
      if args.is_empty() {
        cmd_ctx.console.print("Usage: game:paint <color>");
        cmd_ctx
          .console
          .print("Colors: red, green, blue, yellow, purple, reset");
        return CommandResult::Ok;
      }

      match args[0] {
        "red" => {
          PAINT_R.store(140, Ordering::Relaxed);
          PAINT_G.store(40, Ordering::Relaxed);
          PAINT_B.store(40, Ordering::Relaxed);
          cmd_ctx.console.print("Background set to red");
        }
        "green" => {
          PAINT_R.store(40, Ordering::Relaxed);
          PAINT_G.store(140, Ordering::Relaxed);
          PAINT_B.store(40, Ordering::Relaxed);
          cmd_ctx.console.print("Background set to green");
        }
        "blue" => {
          PAINT_R.store(40, Ordering::Relaxed);
          PAINT_G.store(40, Ordering::Relaxed);
          PAINT_B.store(140, Ordering::Relaxed);
          cmd_ctx.console.print("Background set to blue");
        }
        "yellow" => {
          PAINT_R.store(140, Ordering::Relaxed);
          PAINT_G.store(140, Ordering::Relaxed);
          PAINT_B.store(40, Ordering::Relaxed);
          cmd_ctx.console.print("Background set to yellow");
        }
        "purple" => {
          PAINT_R.store(120, Ordering::Relaxed);
          PAINT_G.store(40, Ordering::Relaxed);
          PAINT_B.store(140, Ordering::Relaxed);
          cmd_ctx.console.print("Background set to purple");
        }
        "reset" => {
          PAINT_R.store(40, Ordering::Relaxed);
          PAINT_G.store(70, Ordering::Relaxed);
          PAINT_B.store(40, Ordering::Relaxed);
          cmd_ctx.console.print("Background reset to default");
        }
        other => {
          cmd_ctx.console.print(&format!("Unknown color: {}", other));
          cmd_ctx
            .console
            .print("Colors: red, green, blue, yellow, purple, reset");
        }
      }

      CommandResult::Ok
    });
  }

  fn update(&mut self, ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    // Check for scene switch request from console command
    let req = SCENE_REQUEST.swap(0, Ordering::Relaxed);
    if req == 1 {
      return SceneAction::Switch(Box::new(HubScene));
    }

    if ctx.input.key_pressed(KeyCode::Escape) {
      return SceneAction::Quit;
    }

    SceneAction::Continue
  }

  fn render_background(&mut self, frame: &mut [u8], width: u32, height: u32, _camera: &Camera) {
    let r = PAINT_R.load(Ordering::Relaxed);
    let g = PAINT_G.load(Ordering::Relaxed);
    let b = PAINT_B.load(Ordering::Relaxed);

    for y in 0..height {
      for x in 0..width {
        let idx = ((y * width + x) * 4) as usize;
        frame[idx] = r;
        frame[idx + 1] = g;
        frame[idx + 2] = b;
        frame[idx + 3] = 255;
      }
    }
  }

  fn render_ui(&mut self, ui: &mut UiContext) {
    // Title
    ui.draw_text("Scene 2 — Workshop", 30.0, 20.0, Color::WHITE);

    // Instructions
    ui.draw_text(
      "Press ~ to open the developer console",
      30.0,
      60.0,
      Color::rgb(150, 200, 170),
    );
    ui.draw_text(
      "Try: game:paint red, game:paint green, game:paint reset",
      30.0,
      85.0,
      Color::rgb(120, 170, 140),
    );

    // Back button
    let btn_rect = Rect::new(30.0, 130.0, 200.0, 40.0);
    if ui.button("Back to Scene 1", btn_rect) {
      SCENE_REQUEST.store(1, Ordering::Relaxed);
    }

    // Status panel
    let panel = Rect::new(30.0, 200.0, 400.0, 120.0);
    ui.draw_rect(panel, Color::rgb(15, 30, 20));
    ui.draw_rect_outline(panel, Color::rgb(50, 90, 60), 1.0);

    ui.draw_text("Active Commands", 40.0, 210.0, Color::rgb(150, 255, 200));
    ui.draw_text(
      "game:hello — active (permanent)",
      40.0,
      235.0,
      Color::rgb(100, 255, 100),
    );
    ui.draw_text(
      "game:scene1 / game:scene2 — active (permanent)",
      40.0,
      255.0,
      Color::rgb(100, 255, 100),
    );
    ui.draw_text(
      "game:paint — active (scoped to this scene)",
      40.0,
      275.0,
      Color::rgb(100, 255, 100),
    );

    // Color display
    let r = PAINT_R.load(Ordering::Relaxed);
    let g = PAINT_G.load(Ordering::Relaxed);
    let b = PAINT_B.load(Ordering::Relaxed);
    ui.draw_text(
      &format!("Background: rgb({}, {}, {})", r, g, b),
      40.0,
      295.0,
      Color::rgb(200, 200, 200),
    );

    // Footer
    ui.draw_text("Press ESC to exit", 30.0, 560.0, Color::rgb(100, 100, 100));
  }

  fn name(&self) -> &str {
    "Workshop"
  }
}
