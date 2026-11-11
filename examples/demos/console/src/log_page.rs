//! The Log page: keys that log at each level, from the game and from a
//! library target. The log's levels are set per target (`log:level`), and
//! the console shows warnings and errors by default (`log:show`)

use demo_kit::prelude::*;

use crate::shared::{DIM, GOOD, HOT, OPEN_LINE, draw_footer};

/// The target of the library records the page logs
pub const LIBRARY_TARGET: &str = "some_library";

/// Key, what it logs, and the colour of its row
const ROWS: [(KeyCode, &str, &str); 7] = [
  (KeyCode::Digit1, "1", "game trace"),
  (KeyCode::Digit2, "2", "game debug"),
  (KeyCode::Digit3, "3", "game info"),
  (KeyCode::Digit4, "4", "game warning"),
  (KeyCode::Digit5, "5", "game error"),
  (KeyCode::Digit6, "6", "library warning"),
  (KeyCode::Digit7, "7", "library debug"),
];

pub(crate) struct LogPage {
  /// How many records each key has sent
  sent: [u32; 7],
}

impl LogPage {
  pub(crate) fn new() -> Self {
    Self { sent: [0; 7] }
  }

  fn log(&mut self, row: usize) {
    self.sent[row] += 1;
    let n = self.sent[row];
    match row {
      0 => trace!("game trace #{n}"),
      1 => debug!("game debug #{n}"),
      2 => info!("game info #{n}"),
      3 => warn!("game warning #{n}"),
      4 => error!("game error #{n}"),
      5 => log::warn!(target: LIBRARY_TARGET, "library warning #{n}"),
      _ => log::debug!(target: LIBRARY_TARGET, "library debug #{n}"),
    }
  }
}

impl DemoPage for LogPage {
  fn name(&self) -> &str {
    "Log"
  }

  fn description(&self) -> &str {
    concat!(
      "Log at every level, from the game and from a library: keys 1 to 7, and the console shows the warnings",
      "\nThe game and the engine log from info up by default and libraries from warn up, so trace, debug and the library's debug are dropped until you ask for them.",
      "\nIn the console try log:level game debug (then key 2 shows), log:level some_library debug (then key 7), log:show info (the console shows info too) and log:show off.",
      "\nThe console is a development tool and is absent from a shipped build (ryo2d build --dev-tools keeps it)."
    )
  }

  fn controls(&self) -> Vec<Control> {
    vec![
      Control::keys(
        "1-5",
        &[
          KeyCode::Digit1,
          KeyCode::Digit2,
          KeyCode::Digit3,
          KeyCode::Digit4,
          KeyCode::Digit5,
        ],
        "Log trace, debug, info, warning, error",
      ),
      Control::keys(
        "6 / 7",
        &[KeyCode::Digit6, KeyCode::Digit7],
        "Log a library warning / debug",
      ),
    ]
  }

  fn update(&mut self, ctx: &mut SceneContext, _dt: f32) {
    for (row, (key, ..)) in ROWS.iter().enumerate() {
      if ctx.input.key_pressed(*key) {
        self.log(row);
      }
    }
  }

  fn ui(&mut self, ui: &mut UiContext, _ctx: &mut SceneContext) {
    let area = content_area(ui);
    let x = area.x + 16.0;
    let mut y = area.y + 8.0;
    ui.draw_text(OPEN_LINE, x, y, HOT);
    y += 30.0;
    for (row, (_, key, what)) in ROWS.iter().enumerate() {
      let color = if row < 5 {
        Color::WHITE
      } else {
        Color::rgb(150, 200, 255)
      };
      ui.draw_text(&format!("[{key}]  {what}"), x, y, color);
      ui.draw_text(&format!("sent {}", self.sent[row]), x + 220.0, y, DIM);
      y += 22.0;
    }
    y += 10.0;
    ui.draw_text(
      "Try in the console:  log:level game debug   log:show info",
      x,
      y,
      GOOD,
    );
    ui.draw_text(
      "log:level lists the levels; the library's target is some_library",
      x,
      y + 22.0,
      DIM,
    );
    draw_footer(ui, x, area.y + area.height - 24.0);
  }
}
