//! The `debug:ui` overlay: every widget the UI recorded this frame, outlined
//! with its id.

use crate::core::ecs::kernel::World;
use crate::core::text::TextStyle;
use crate::core::ui::UiContext;
use crate::core::ui::immediate::OVERLAY_LAYER;
use crate::core::ui::nodes::{UiInteraction, cache_stats};
use crate::core::ui::{Color, Rect, UiId};

/// Size of the id labels, in UI units
const LABEL_SIZE: f32 = 8.0;

/// Outline each record written this frame on the overlay layer: cyan for the
/// top scene's, grey for a lower scene's; yellow when hot, orange when
/// active, green when focused. The low 16 bits of the id label its corner.
pub(crate) fn draw_debug_overlay(ui: &mut UiContext) {
  let layer = ui.set_layer(OVERLAY_LAYER);
  // Unclipped, and the caller's clips as they were afterwards
  ui.with_no_clip(|ui| {
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
  });
  ui.set_layer(layer);
}

/// How long a rebuilt node's outline stays, in seconds
const FADE: f32 = 0.5;

/// The `debug:ui-rebuilds` overlay's state: the nodes rebuilt lately, each
/// outlined for half a second, fading.
///
/// A rebuilt node is outlined in orange. The nodes the debug spot check
/// built to compare with their records are not outlined, since nothing
/// changed in them; they show only in the counter, apart.
#[derive(Debug, Default)]
pub(crate) struct RebuildsOverlay {
  /// The ids outlined and the seconds each has left
  marks: Vec<(UiId, f32)>,
}

impl RebuildsOverlay {
  /// How many outlines are showing
  #[cfg(test)]
  pub(crate) fn outlined(&self) -> usize {
    self.marks.len()
  }

  /// Age the outlines by `dt`, add the nodes the last node walk rebuilt,
  /// draw them on the overlay layer and write the counter in the top right
  /// corner. Call it once a pass, after the node trees drew; `dt` is zero
  /// for a pass that reruns the frame.
  pub(crate) fn draw(&mut self, ui: &mut UiContext, world: &World, dt: f32) {
    for (_, left) in &mut self.marks {
      *left -= dt;
    }
    self.marks.retain(|&(_, left)| left > 0.0);
    let stats = cache_stats(world).cloned().unwrap_or_default();
    for node in &stats.rebuilt {
      let Some(id) = world
        .get::<UiInteraction>(*node)
        .map(|interaction| interaction.id)
        .filter(|id| *id != UiId::NONE)
      else {
        continue;
      };
      match self.marks.iter_mut().find(|(marked, _)| *marked == id) {
        Some((_, left)) => *left = FADE,
        None => self.marks.push((id, FADE)),
      }
    }
    let layer = ui.set_layer(OVERLAY_LAYER);
    ui.with_no_clip(|ui| {
      for &(id, left) in &self.marks {
        let record = ui.memory.current(id).or_else(|| ui.memory.prev(id));
        let Some(rect) = record.map(|record| record.rect) else {
          continue;
        };
        let alpha = (left / FADE).clamp(0.0, 1.0);
        ui.draw_rect_outline(rect, Color::new(255, 150, 40, (alpha * 255.0) as u8), 2.0);
      }
      let mut text = format!(
        "rebuilt {} / replayed {} nodes",
        stats.built, stats.replayed
      );
      if stats.spot_built > 0 {
        text.push_str(&format!(" (spot check {})", stats.spot_built));
      }
      let style = TextStyle::default().size(LABEL_SIZE * 1.5);
      let width = ui.measure_text(text.as_str(), &style).x;
      let (screen, _) = ui.screen_size();
      ui.draw_text_styled(text, screen as f32 - width - 4.0, 4.0, &style);
    });
    ui.set_layer(layer);
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::testing::UiHarness;
  use crate::core::ui::immediate::UiCommands;
  use crate::core::ui::nodes::{UiLabel, UiNode};
  use crate::core::ui::test_util::{context, text_engine};

  #[test]
  fn the_overlay_leaves_the_callers_clips() {
    let mut commands = UiCommands::default();
    let mut text = text_engine();
    let mut ui = context(&mut commands, &mut text, (200.0, 100.0));
    let clip = Rect::new(10.0, 10.0, 50.0, 20.0);
    ui.push_clip(clip);
    draw_debug_overlay(&mut ui);
    assert_eq!(ui.clip_depth(), 1);
    assert_eq!(ui.current_clip(), Some(clip));
  }

  #[test]
  fn a_rebuilt_node_is_outlined_and_the_outline_fades() {
    let mut h = UiHarness::new(640.0, 480.0);
    h.spawn_ui(
      UiNode::column()
        .child(UiNode::text("Menu"))
        .child(UiNode::button("Play"))
        .child(UiNode::button("A").name("a")),
    );
    for _ in 0..3 {
      h.frame(|_| ());
    }
    let a = h.find_ui("a").unwrap();
    h.world_mut().get_mut::<UiLabel>(a).unwrap().0 = "B".into();
    let mut overlay = RebuildsOverlay::default();
    h.frame_with_world(|ui, world| overlay.draw(ui, world, 1.0 / 60.0));
    assert!(overlay.outlined() >= 1, "the rebuilt path is outlined");
    for _ in 0..40 {
      h.frame_with_world(|ui, world| overlay.draw(ui, world, 1.0 / 60.0));
    }
    assert_eq!(overlay.outlined(), 0, "faded after half a second");
  }
}
