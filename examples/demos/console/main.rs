//! Console Demo - Ryo2D Example
//!
//! Demonstrates the two-phase command system and command scoping: commands are
//! declared once and handlers are registered per page, so `game:paint` only
//! works on the Workshop page. Runs on the demo-kit shell; switch pages with
//! `[` and `]`.

use std::sync::atomic::{AtomicU8, Ordering};

use demo_kit::prelude::*;

// Console handlers can't touch the page, so the paint color is shared via atomics
const PAINT_DEFAULT: (u8, u8, u8) = (40, 70, 40);
static PAINT_R: AtomicU8 = AtomicU8::new(PAINT_DEFAULT.0);
static PAINT_G: AtomicU8 = AtomicU8::new(PAINT_DEFAULT.1);
static PAINT_B: AtomicU8 = AtomicU8::new(PAINT_DEFAULT.2);

fn main() {
  Demo::new("Console")
    .resolution(800, 600)
    .page(|| HubPage)
    .page(|| WorkshopPage)
    .run();
}

/// Declare all commands and register the shared handlers.
/// Declarations are global and idempotent; handlers are scoped to the active page.
fn register_shared_commands(ctx: &mut SceneContext) {
  let mut console = ctx.console();

  // Declare ALL commands up front (visible in help immediately)
  console.declare(
    "game:hello",
    "Print a greeting",
    Some(
      "Usage: game:hello [name]\n\nPrints 'Hello, <name>!' or 'Hello, World!' if no name given.",
    ),
  );
  console.declare(
    "game:paint",
    "Change background color (Workshop page only)",
    Some("Usage: game:paint <color>\n\nColors: red, green, blue, yellow, purple, reset\nOnly has effect on the Workshop page."),
  );

  // Register handlers that work on every page (scoped, re-registered on each enter)
  console.register("game:hello", |args, cmd_ctx| {
    let name = if args.is_empty() { "World" } else { args[0] };
    cmd_ctx.console.print(format!("Hello, {}!", name));
    CommandResult::Ok
  });
}

const CONSOLE_HINT: &str = "Open the developer console with the backquote key (`).\n\
  Run `help game` to list commands; game:hello [name] works on both pages, game:paint <color> only on Workshop.";

// ============================================================================
// Page 1 — Hub
// ============================================================================

struct HubPage;

impl DemoPage for HubPage {
  fn name(&self) -> &str {
    "Hub"
  }

  fn description(&self) -> &str {
    CONSOLE_HINT
  }

  fn enter(&mut self, ctx: &mut SceneContext) {
    register_shared_commands(ctx);
  }

  fn ui(&mut self, ui: &mut UiContext, _ctx: &mut SceneContext) {
    let area = content_area(ui);
    let x = area.x + 10.0;
    let y = area.y + 10.0;

    ui.draw_text(
      "Press ` to open the developer console",
      x,
      y,
      Color::rgb(150, 170, 200),
    );
    ui.draw_text(
      "Try: help game, game:hello, game:paint red",
      x,
      y + 25.0,
      Color::rgb(120, 140, 170),
    );

    // Status panel
    let panel = Rect::new(x, y + 70.0, 400.0, 100.0);
    ui.draw_rect(panel, Color::rgb(15, 20, 40));
    ui.draw_rect_outline(panel, Color::rgb(50, 60, 90), 1.0);

    ui.draw_text(
      "Active Commands",
      panel.x + 10.0,
      panel.y + 10.0,
      Color::rgb(150, 200, 255),
    );
    ui.draw_text(
      "game:hello — active (permanent)",
      panel.x + 10.0,
      panel.y + 35.0,
      Color::rgb(100, 255, 100),
    );
    ui.draw_text(
      "game:paint — inactive (Workshop page only)",
      panel.x + 10.0,
      panel.y + 55.0,
      Color::rgb(255, 100, 100),
    );
  }
}

// ============================================================================
// Page 2 — Workshop
// ============================================================================

struct WorkshopPage;

impl DemoPage for WorkshopPage {
  fn name(&self) -> &str {
    "Workshop"
  }

  fn description(&self) -> &str {
    CONSOLE_HINT
  }

  fn enter(&mut self, ctx: &mut SceneContext) {
    PAINT_R.store(PAINT_DEFAULT.0, Ordering::Relaxed);
    PAINT_G.store(PAINT_DEFAULT.1, Ordering::Relaxed);
    PAINT_B.store(PAINT_DEFAULT.2, Ordering::Relaxed);

    // Register shared commands (hello)
    register_shared_commands(ctx);

    // Register scoped handler for game:paint (auto-cleaned on page exit)
    ctx.console().register("game:paint", |args, cmd_ctx| {
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
          cmd_ctx.console.print(format!("Unknown color: {}", other));
          cmd_ctx
            .console
            .print("Colors: red, green, blue, yellow, purple, reset");
        }
      }

      CommandResult::Ok
    });
  }

  fn ui(&mut self, ui: &mut UiContext, _ctx: &mut SceneContext) {
    // Painted background (set via game:paint console command)
    let r = PAINT_R.load(Ordering::Relaxed);
    let g = PAINT_G.load(Ordering::Relaxed);
    let b = PAINT_B.load(Ordering::Relaxed);
    let (w, h) = ui.screen_size();
    ui.draw_rect(Rect::new(0.0, 0.0, w as f32, h as f32), Color::rgb(r, g, b));

    let area = content_area(ui);
    let x = area.x + 10.0;
    let y = area.y + 10.0;

    ui.draw_text(
      "Press ` to open the developer console",
      x,
      y,
      Color::rgb(150, 200, 170),
    );
    ui.draw_text(
      "Try: game:paint red, game:paint green, game:paint reset",
      x,
      y + 25.0,
      Color::rgb(120, 170, 140),
    );

    // Status panel
    let panel = Rect::new(x, y + 70.0, 400.0, 120.0);
    ui.draw_rect(panel, Color::rgb(15, 30, 20));
    ui.draw_rect_outline(panel, Color::rgb(50, 90, 60), 1.0);

    ui.draw_text(
      "Active Commands",
      panel.x + 10.0,
      panel.y + 10.0,
      Color::rgb(150, 255, 200),
    );
    ui.draw_text(
      "game:hello — active (permanent)",
      panel.x + 10.0,
      panel.y + 35.0,
      Color::rgb(100, 255, 100),
    );
    ui.draw_text(
      "game:paint — active (scoped to this page)",
      panel.x + 10.0,
      panel.y + 55.0,
      Color::rgb(100, 255, 100),
    );
    ui.draw_text(
      &format!("Background: rgb({}, {}, {})", r, g, b),
      panel.x + 10.0,
      panel.y + 85.0,
      Color::rgb(200, 200, 200),
    );
  }
}
