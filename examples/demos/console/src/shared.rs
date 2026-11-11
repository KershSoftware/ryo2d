//! What the pages share: the colours and the line every page tells about
//! the console key and the build without tools

use demo_kit::prelude::*;

/// Labels and headers
pub(crate) const DIM: Color = Color::rgb(150, 150, 165);
/// A value that matters now
pub(crate) const HOT: Color = Color::XP_GOLD;
/// Something refused or lost
pub(crate) const BAD: Color = Color::rgb(255, 110, 100);
/// Something that works
pub(crate) const GOOD: Color = Color::rgb(120, 220, 130);

/// The most `game:spawn` adds at once
pub const MAX_SPAWN: usize = 500;

/// Marks what `game:spawn` added, so `game:clear` removes only that
#[derive(Component)]
pub struct Spawned;

/// The first line of every page: how to open the console
pub(crate) const OPEN_LINE: &str =
  "Backquote (`) opens the console, and closes it while its line is empty; Esc always closes it";

/// Drawn on every page: what a shipped game has
pub(crate) const SHIPPED_LINE: &str = "The console is a development tool: a shipped build has none, and these commands do nothing there";

/// The line every page ends with
pub(crate) fn draw_footer(ui: &mut UiContext, x: f32, y: f32) {
  ui.draw_text(SHIPPED_LINE, x, y, DIM);
}
