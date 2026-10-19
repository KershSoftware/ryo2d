//! Tooltips: a widget's `.tooltip(text)` shows after the pointer or focus
//! rests on it; the scene's UI draws the last one asked for when it ends

use std::sync::Arc;

use glam::Vec2;

use crate::core::text::{RichText, TextLayoutOptions};
use crate::core::ui::immediate::{POPUP_LAYER, UiContext};
use crate::core::ui::layout::{Edges, Measure, UiStyle};
use crate::core::ui::response::Response;
use crate::core::ui::theme::{WidgetKind, WidgetState};
use crate::core::ui::tree::{DrawKind, Node};
use crate::core::ui::{Rect, UiId};

/// The widest a tooltip grows before its text wraps
const MAX_WIDTH: f32 = 280.0;
/// Room between the widget and its tooltip
const GAP: f32 = 6.0;
/// Separates a tooltip's state and node ids from the widget's own
const SALT: u64 = 0x0074_6970;

/// How long the pointer or focus has rested on a widget with a tooltip
#[derive(Debug, Default)]
pub(super) struct TooltipTimer {
  seconds: f32,
}

impl UiContext<'_> {
  /// Advance the tooltip timer of the widget `response` belongs to and, once
  /// the theme's delay is reached, ask for its `tooltip` to be drawn. It runs
  /// while the widget is hovered, or focused with the ring showing; a press
  /// or losing both starts it over.
  pub(crate) fn tooltip_tick(&mut self, tooltip: Option<&RichText>, response: &Response) {
    let Some(text) = tooltip else {
      return;
    };
    let (scene, _) = self.scene();
    let resting =
      (response.hovered() || (response.focused() && self.focus_visible())) && !response.pressed();
    let dt = self.dt();
    let key = response.id.derived(SALT);
    let mut scratch = TooltipTimer::default();
    let timer = self
      .memory
      .state::<TooltipTimer>(key, scene)
      .unwrap_or(&mut scratch);
    timer.seconds = if resting { timer.seconds + dt } else { 0.0 };
    let seconds = timer.seconds;
    if resting && seconds >= self.theme().metrics.tooltip_delay {
      self.taint();
      self.pending_tooltip = Some((text.clone(), response.rect));
    }
  }

  /// Draw the tooltip asked for this scene, if it is the top scene's, on the
  /// popup layer below its widget (above it when it would leave the UI)
  pub(crate) fn flush_tooltip(&mut self) {
    let Some((text, near)) = self.pending_tooltip.take() else {
      return;
    };
    let (scene, top) = self.scene();
    if !top {
      return;
    }
    let theme = self.theme_arc();
    let look = theme.look(WidgetKind::Tooltip, None, WidgetState::Normal);
    let pad = if look.padding == Edges::ZERO {
      Edges::all(6.0)
    } else {
      look.padding
    };
    let style = theme.text.caption.style().color(look.text.with_alpha(255));
    let options = TextLayoutOptions::default().wrap((MAX_WIDTH - pad.width()).max(1.0));
    let leaf = self.leaf_of(text, style, options);
    let inner = self.measure_leaf(&leaf);
    let size = Vec2::new(inner.x + pad.width(), inner.y + pad.height());
    if !size.is_finite() {
      return;
    }

    let ui = self.ui_size();
    let below = near.bottom() + GAP;
    let y = if below + size.y > ui.y {
      near.y - GAP - size.y
    } else {
      below
    };
    let x = near.x.min(ui.x - size.x);
    let at = Rect::new(x.max(0.0), y.max(0.0), size.x, size.y);

    let id = UiId::root(scene).derived(SALT);
    let page = self.set_layer(POPUP_LAYER);
    self.with_no_clip(|ui| {
      let node_style = UiStyle {
        padding: pad,
        ..UiStyle::default()
      };
      let node = Node::new(id, node_style, Measure::Fixed(inner));
      let index = ui.push_node(node, Some(at));
      ui.push_look_box(index, &look);
      ui.push_draw(
        index,
        DrawKind::Text {
          leaf: Arc::new(leaf),
          frac: Vec2::ZERO,
          wrap: false,
          opacity: f32::from(look.text.a) / 255.0,
          reveal: None,
        },
      );
      ui.close_node(index);
    });
    self.set_layer(page);
  }
}

#[cfg(all(test, feature = "default-fonts"))]
mod tests {
  use crate::core::ui::immediate::{POPUP_LAYER, UiCommand};
  use crate::core::ui::test_util::UiDriver;
  use crate::core::ui::{Rect, UiContext};

  fn popup_texts(d: &UiDriver) -> Vec<String> {
    d.commands
      .iter()
      .filter_map(|(_, c)| match c {
        UiCommand::Text { layout, layer, .. } if *layer == POPUP_LAYER => {
          Some(layout.text().to_string())
        }
        _ => None,
      })
      .collect()
  }

  fn draw(ui: &mut UiContext<'_>) {
    ui.button("Save")
      .at(Rect::new(10.0, 10.0, 80.0, 30.0))
      .tooltip("Saves the game")
      .show();
  }

  #[test]
  fn a_tooltip_appears_after_the_delay_and_hides_on_press() {
    let mut d = UiDriver::new((300.0, 200.0));
    d.frame(draw);
    d.move_to(20.0, 20.0);
    for _ in 0..28 {
      d.frame(draw);
    }
    assert!(popup_texts(&d).is_empty(), "not before 0.5 s");
    for _ in 0..4 {
      d.frame(draw);
    }
    assert_eq!(popup_texts(&d), vec!["Saves the game".to_string()]);
    d.press();
    d.frame(draw);
    d.release();
    d.frame(draw);
    assert!(
      popup_texts(&d).is_empty(),
      "a press hides it and restarts the delay"
    );
  }

  /// The first rect the last frame drew on the popup layer
  fn popup_rect(d: &UiDriver) -> Option<Rect> {
    d.commands.iter().find_map(|(_, c)| match c {
      UiCommand::Rect { rect, layer, .. } if *layer == POPUP_LAYER => Some(*rect),
      _ => None,
    })
  }

  fn rest_on(d: &mut UiDriver, at: Rect, tip: &str, frames: usize) {
    let draw = |ui: &mut UiContext<'_>| {
      ui.button("X").at(at).tooltip(tip).show();
    };
    d.frame(draw);
    d.move_to(at.x + 2.0, at.y + 2.0);
    for _ in 0..frames {
      d.frame(draw);
    }
  }

  #[test]
  fn a_tooltip_sits_below_its_widget_and_flips_above_at_the_bottom() {
    let mut d = UiDriver::new((300.0, 200.0));
    rest_on(&mut d, Rect::new(10.0, 10.0, 80.0, 30.0), "Tip", 40);
    let below = popup_rect(&d).expect("a tooltip");
    assert_eq!((below.x, below.y), (10.0, 46.0), "6 units under the widget");

    let mut d = UiDriver::new((300.0, 200.0));
    rest_on(&mut d, Rect::new(10.0, 170.0, 80.0, 30.0), "Tip", 40);
    let above = popup_rect(&d).expect("a tooltip");
    assert!(above.bottom() <= 164.5, "above the widget: {above:?}");
  }

  #[test]
  fn a_tooltip_stays_on_screen_and_wraps_long_text() {
    let mut d = UiDriver::new((300.0, 200.0));
    let long =
      "A long explanation that goes on and on and on so that it cannot fit in one line of text";
    rest_on(&mut d, Rect::new(250.0, 10.0, 40.0, 30.0), long, 40);
    let tip = popup_rect(&d).expect("a tooltip");
    assert!(tip.right() <= 300.0, "{tip:?}");
    assert!(tip.width <= 280.5, "{tip:?}");
    assert!(tip.height > 30.0, "wrapped onto lines: {tip:?}");
  }

  #[test]
  fn a_progress_bar_with_a_tooltip_shows_it_on_hover() {
    let mut d = UiDriver::new((300.0, 200.0));
    let draw = |ui: &mut UiContext<'_>| {
      ui.progress(0.5)
        .at(Rect::new(10.0, 10.0, 160.0, 12.0))
        .tooltip("Health")
        .show();
    };
    d.frame(draw);
    d.move_to(20.0, 15.0);
    for _ in 0..40 {
      d.frame(draw);
    }
    assert_eq!(popup_texts(&d), vec!["Health".to_string()]);
  }

  #[test]
  fn a_focused_widget_shows_its_tooltip_with_the_ring() {
    use crate::core::input::InputDevice;
    let mut d = UiDriver::new((300.0, 200.0));
    d.device(InputDevice::Gamepad);
    let draw = |ui: &mut UiContext<'_>| {
      ui.column().focus_scope().show(|ui| {
        ui.button("Save")
          .tooltip("Saves the game")
          .default_focus()
          .show();
      });
    };
    for _ in 0..40 {
      d.frame(draw);
    }
    assert_eq!(popup_texts(&d), vec!["Saves the game".to_string()]);
  }
}
