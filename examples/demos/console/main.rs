//! Console Demo - Ryo2D Example
//!
//! Demonstrates console commands that run in the game: each page registers
//! its commands as it enters and they leave with it, so `game:paint` only
//! exists on the Workshop page, where it sets the page's own colour. Runs on
//! the demo-kit shell; switch pages with `[` and `]`.

use demo_kit::prelude::*;

const PAINT_DEFAULT: (u8, u8, u8) = (40, 70, 40);

fn main() {
  Demo::new("Console")
    .resolution(800, 600)
    .page(|| HubPage)
    .page(|| WorkshopPage {
      paint: PAINT_DEFAULT,
    })
    .run();
}

/// Register `game:hello` for page `P` (every page has its own)
fn register_hello<P: DemoPage>(ctx: &mut SceneContext) {
  page_command(
    ctx,
    CommandSpec::new("game:hello", "Print a greeting")
      .usage("game:hello [name]")
      .help("Prints 'Hello, <name>!', or 'Hello, World!' with no name. Quotes keep a name with spaces together."),
    |_: &mut P, _ctx, cmd| {
      let name = cmd.rest(0);
      let name = if name.is_empty() { "World" } else { name.as_str() };
      cmd.print(format!("Hello, {name}!"));
      Ok(())
    },
  );
}

const CONSOLE_HINT: &str = "Open the developer console with the backquote key (`).\n\
  Run `help` to list commands; game:hello [name] works on both pages, game:paint <color> only on Workshop.";

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
    register_hello::<HubPage>(ctx);
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
      "Try: help, game:hello, game:paint red",
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
      "game:hello — on every page",
      panel.x + 10.0,
      panel.y + 35.0,
      Color::rgb(100, 255, 100),
    );
    ui.draw_text(
      "game:paint — not on this page (Workshop only)",
      panel.x + 10.0,
      panel.y + 55.0,
      Color::rgb(255, 100, 100),
    );
  }
}

// ============================================================================
// Page 2 — Workshop
// ============================================================================

struct WorkshopPage {
  /// The background, set by `game:paint`
  paint: (u8, u8, u8),
}

const COLORS: &str = "Colors: red, green, blue, yellow, purple, reset";

impl DemoPage for WorkshopPage {
  fn name(&self) -> &str {
    "Workshop"
  }

  fn description(&self) -> &str {
    CONSOLE_HINT
  }

  fn enter(&mut self, ctx: &mut SceneContext) {
    register_hello::<WorkshopPage>(ctx);

    // This page's own command, gone when the page is left
    page_command(
      ctx,
      CommandSpec::new(
        "game:paint",
        "Change the background color (Workshop page only)",
      )
      .usage("game:paint <color>")
      .help(COLORS),
      |page: &mut WorkshopPage, _ctx, cmd| {
        let color: String = cmd.arg(0)?;
        let paint = match color.to_ascii_lowercase().as_str() {
          "red" => (140, 40, 40),
          "green" => (40, 140, 40),
          "blue" => (40, 40, 140),
          "yellow" => (140, 140, 40),
          "purple" => (120, 40, 140),
          "reset" => PAINT_DEFAULT,
          other => return Err(format!("Unknown color: {other}. {COLORS}")),
        };
        page.paint = paint;
        cmd.print(format!("Background set to {color}"));
        Ok(())
      },
    );
  }

  fn ui(&mut self, ui: &mut UiContext, _ctx: &mut SceneContext) {
    // Painted background (set via game:paint console command)
    let (r, g, b) = self.paint;
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
      "game:hello — on every page",
      panel.x + 10.0,
      panel.y + 35.0,
      Color::rgb(100, 255, 100),
    );
    ui.draw_text(
      "game:paint — on this page only",
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
