//! The `debug:ui` overlay: every widget the UI recorded this frame, outlined
//! with its id.

use crate::core::text::TextStyle;
use crate::core::ui::UiContext;
use crate::core::ui::immediate::OVERLAY_LAYER;
use crate::core::ui::{Color, Rect};

/// Size of the id labels, in UI units
const LABEL_SIZE: f32 = 8.0;

/// Outline each record written this frame on the overlay layer: cyan for the
/// top scene's, grey for a lower scene's; yellow when hot, orange when
/// active, green when focused. The low 16 bits of the id label its corner.
pub(crate) fn draw_debug_overlay(ui: &mut UiContext) {
  let layer = ui.set_layer(OVERLAY_LAYER);
  ui.clear_clips();
  let i = &ui.memory.interact;
  let (hot, active, focused, top) = (i.hot, i.active, i.focused, i.nav_scene);
  let mut marks: Vec<(u64, Rect, Color)> = ui
    .memory
    .current_records()
    .map(|(id, record)| {
      let color = if id == focused {
        Color::rgb(80, 220, 120)
      } else if id == active {
        Color::rgb(255, 150, 40)
      } else if id == hot {
        Color::YELLOW
      } else if Some(record.scene) == top {
        Color::new(80, 200, 255, 200)
      } else {
        Color::new(140, 140, 140, 160)
      };
      (id.0, record.rect, color)
    })
    .collect();
  // Records come in map order: sort so the overlay is stable frame to frame
  marks.sort_by_key(|(id, ..)| *id);
  let style = TextStyle::default().size(LABEL_SIZE);
  for (id, rect, color) in marks {
    ui.draw_rect_outline(rect, color, 1.0);
    let label = format!("{:04x}", id & 0xffff);
    ui.draw_text_styled(
      label,
      rect.x + 1.0,
      rect.y + 1.0,
      &style.clone().color(color),
    );
  }
  ui.set_layer(layer);
}
