//! The Workshop page: `game:paint <r> <g> <b>` is a command of this page
//! only. It sets a field of the page, and leaves with the page: on the other
//! pages the console says it is unknown

use demo_kit::prelude::*;

use crate::shared::{BAD, DIM, GOOD, HOT, OPEN_LINE, draw_footer};

const DEFAULT: [u8; 3] = [40, 70, 40];

pub(crate) struct WorkshopPage {
  /// The background, set by `game:paint`
  paint: [u8; 3],
}

impl WorkshopPage {
  pub(crate) fn new() -> Self {
    Self { paint: DEFAULT }
  }
}

impl DemoPage for WorkshopPage {
  fn name(&self) -> &str {
    "Workshop"
  }

  fn description(&self) -> &str {
    concat!(
      "A command only this page has: game:paint <r> <g> <b> sets the page's own background",
      "\nOpen the console with backquote and try game:paint 140 40 40, game:paint 40 40 140 and game:paint 300 0 0 (an error).",
      "\nThe command is registered with page_command in enter, so it is gone when the page changes: on the other pages the console says it is unknown.",
      "\nThe console is a development tool and is absent from a shipped build (ryo2d build --dev-tools keeps it)."
    )
  }

  fn enter(&mut self, ctx: &mut SceneContext) {
    page_command(
      ctx,
      CommandSpec::new("game:paint", "Set the Workshop page's background")
        .usage("game:paint <r> <g> <b>")
        .help("Three numbers from 0 to 255. Only on the Workshop page."),
      |page: &mut WorkshopPage, _ctx, cmd| {
        let r: u8 = cmd.arg(0)?;
        let g: u8 = cmd.arg(1)?;
        let b: u8 = cmd.arg(2)?;
        page.paint = [r, g, b];
        cmd.print(format!("Background set to {r} {g} {b}"));
        Ok(())
      },
    );
  }

  fn ui(&mut self, ui: &mut UiContext, _ctx: &mut SceneContext) {
    let [r, g, b] = self.paint;
    let (w, h) = ui.screen_size();
    ui.draw_rect(Rect::new(0.0, 0.0, w as f32, h as f32), Color::rgb(r, g, b));

    let area = content_area(ui);
    let x = area.x + 16.0;
    let y = area.y + 8.0;
    ui.draw_text(OPEN_LINE, x, y, HOT);
    ui.draw_text(
      "game:paint <r> <g> <b>   numbers 0 to 255",
      x,
      y + 26.0,
      Color::WHITE,
    );
    ui.draw_text(
      &format!("Background: rgb({r}, {g}, {b})"),
      x,
      y + 56.0,
      GOOD,
    );
    ui.draw_text(
      "game:paint is registered by this page: it is gone on the other pages",
      x,
      y + 86.0,
      BAD,
    );
    ui.draw_text(
      "(there the console prints: Unknown command)",
      x,
      y + 106.0,
      DIM,
    );
    draw_footer(ui, x, area.y + area.height - 24.0);
  }
}
