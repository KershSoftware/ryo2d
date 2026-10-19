//! The inventory slot: a square with an item's icon and count, the usual
//! drag source and drop target

use std::panic::Location;
use std::sync::Arc;

use glam::Vec2;

use super::common::{WidgetOpts, widget_builders};
use crate::core::asset::SpriteHandle;
use crate::core::render::batch::Filter;
use crate::core::text::{RichText, TextLayoutOptions};
use crate::core::ui::Color;
use crate::core::ui::immediate::UiContext;
use crate::core::ui::layout::{Edges, Measure, UiStyle};
use crate::core::ui::memory::Flags;
use crate::core::ui::response::{Response, Sense};
use crate::core::ui::theme::{WidgetKind, WidgetState};
use crate::core::ui::tree::{DrawKind, Node};

/// The icon's inset from the slot's edges
const ICON_INSET: f32 = 6.0;
/// The count's inset from the slot's bottom-right corner
const COUNT_INSET: f32 = 3.0;
/// A selected slot's border
const SELECTED_BORDER: f32 = 2.0;

/// An inventory slot being set up. It is drawn by [`show`](Self::show) or
/// when it is dropped.
pub struct Slot<'u, 'a> {
  ui: Option<&'u mut UiContext<'a>>,
  site: &'static Location<'static>,
  icon: Option<SpriteHandle>,
  count: u32,
  selected: bool,
  filter: Filter,
  opts: WidgetOpts,
}

impl<'a> UiContext<'a> {
  /// An inventory slot: a square of the theme's `Sizes::slot` with the
  /// slot look, an item's [`icon`](Slot::icon) and [`count`](Slot::count).
  /// A click or Confirm reports `clicked()`; as a
  /// [`drag_source`](Slot::drag_source) its payload is dragged with the
  /// mouse or picked up with Confirm, and as a
  /// [`drop_target`](Slot::drop_target) it reports what is dropped on it.
  /// A slot with no icon is empty.
  ///
  /// ```ignore
  /// ui.grid(4).gap(4.0).focus_scope().show(|ui| {
  ///     for (i, item) in self.bag.iter().enumerate() {
  ///         let mut slot = ui.slot().id(i).drop_target::<usize>();
  ///         if let Some(item) = item {
  ///             slot = slot.icon(&item.icon).count(item.count).drag_source(i);
  ///         }
  ///         if let Some(from) = slot.show().dropped::<usize>() {
  ///             self.bag.swap(from, i);
  ///         }
  ///     }
  /// });
  /// ```
  #[track_caller]
  pub fn slot<'u>(&'u mut self) -> Slot<'u, 'a> {
    Slot {
      ui: Some(self),
      site: Location::caller(),
      icon: None,
      count: 0,
      selected: false,
      filter: Filter::Default,
      opts: WidgetOpts::default(),
    }
  }
}

widget_builders!(Slot<'_, '_>);

impl Slot<'_, '_> {
  /// The item's icon, inset from the edges (also its drag ghost, unless
  /// [`drag_ghost`](Self::drag_ghost) gives one)
  pub fn icon(mut self, sprite: &SpriteHandle) -> Self {
    self.icon = Some(sprite.clone());
    self
  }

  /// How many of the item it holds: shown at the bottom-right above 1
  pub fn count(mut self, n: u32) -> Self {
    self.count = n;
    self
  }

  /// Draw it selected: a border in the accent colour
  pub fn selected(mut self, on: bool) -> Self {
    self.selected = on;
    self
  }

  /// How the icon is sampled (`Filter::Default`: nearest in pixel-art
  /// presentation, else linear)
  pub fn filter(mut self, filter: Filter) -> Self {
    self.filter = filter;
    self
  }

  /// Draw the slot; what happened to it this frame
  pub fn show(mut self) -> Response {
    self.run().unwrap_or_default()
  }

  /// Record the slot, once (`show` and `Drop` both come here)
  fn run(&mut self) -> Option<Response> {
    let ui = self.ui.take()?;
    let id = ui.memory.scopes.make(self.site, self.opts.key);
    let disabled = self.opts.disabled;
    let mut response = ui.response(id, self.opts.drag.sense(Sense::click()), disabled);
    // Its icon is its ghost, unless it has one of its own
    let ghost = (self.icon.as_ref(), self.filter);
    ui.drag_tick(&mut response, &self.opts.drag, disabled, ghost);

    let state = ui.widget_state(&response, disabled);
    let mut look = ui.look_of(WidgetKind::Slot, &self.opts, state);
    let theme = ui.theme_arc();
    if self.selected {
      look.border = theme.palette.accent;
      look.border_width = SELECTED_BORDER;
    }
    let side = theme.metrics.sizes.slot;
    // The padding places the count; the icon has its own inset
    let style = UiStyle {
      padding: Edges::all(COUNT_INSET),
      ..self.opts.style.clone()
    };
    let mut node = Node::new(id, style, Measure::Fixed(Vec2::splat(side)));
    node.record = Some(self.opts.record(Flags::CLICK, true));
    let index = ui.push_node(node, self.opts.at);
    ui.push_look_box(index, &look);

    // Dimmed as far as a disabled button's label is
    let alpha = if disabled {
      ui.look_of(WidgetKind::Button, &self.opts, WidgetState::Disabled)
        .text
        .a
    } else {
      255
    };
    if let Some(sprite) = self.icon.take() {
      ui.push_draw(
        index,
        DrawKind::Sprite {
          inset: Edges::all(ICON_INSET),
          frac: Vec2::splat(0.5),
          size: None,
          sprite,
          uv: [0.0, 0.0, 1.0, 1.0],
          tint: Color::WHITE.with_alpha(alpha),
          filter: self.filter,
        },
      );
    }
    if self.count > 1 {
      let text = RichText::new(self.count.to_string());
      let style = theme.text.caption.style();
      let options = TextLayoutOptions::default();
      let leaf = ui.leaf_of(text, style, options);
      ui.push_draw(
        index,
        DrawKind::Text {
          leaf: Arc::new(leaf),
          frac: Vec2::ONE,
          wrap: false,
          opacity: f32::from(alpha) / 255.0,
          reveal: None,
        },
      );
    }
    if response.focused() && ui.focus_visible() {
      ui.focus_ring(index, look.radius);
    }
    response.rect = ui.close_node(index).rect;
    ui.tooltip_tick(self.opts.tooltip.as_ref(), &response);
    Some(response)
  }
}

impl Drop for Slot<'_, '_> {
  /// A slot not shown is drawn when it is dropped
  fn drop(&mut self) {
    self.run();
  }
}

#[cfg(all(test, feature = "default-fonts"))]
mod tests {
  use crate::core::asset::SpriteHandle;
  use crate::core::graphics::Sprite;
  use crate::core::ui::Rect;
  use crate::core::ui::immediate::UiCommand;
  use crate::core::ui::test_util::UiDriver;

  #[test]
  fn a_slot_draws_its_icon_and_count() {
    let icon = SpriteHandle::from_sprite(Sprite::solid_rect(16, 16, 200, 50, 50, 255), "test-icon");
    let mut d = UiDriver::new((100.0, 100.0));
    d.frame(|ui| {
      ui.slot()
        .at(Rect::new(10.0, 10.0, 48.0, 48.0))
        .icon(&icon)
        .count(12)
        .show();
    });
    let sprites: Vec<Rect> = d
      .commands
      .iter()
      .filter_map(|(_, c)| match c {
        UiCommand::Sprite { rect, .. } => Some(*rect),
        _ => None,
      })
      .collect();
    assert_eq!(
      sprites,
      vec![Rect::new(16.0, 16.0, 36.0, 36.0)],
      "the icon, inset 6"
    );
    let texts: Vec<String> = d
      .commands
      .iter()
      .filter_map(|(_, c)| match c {
        UiCommand::Text { layout, .. } => Some(layout.text().to_string()),
        _ => None,
      })
      .collect();
    assert_eq!(texts, vec!["12".to_string()]);
  }

  #[test]
  fn the_count_sits_inset_at_the_bottom_right_and_one_shows_none() {
    let mut d = UiDriver::new((100.0, 100.0));
    d.frame(|ui| {
      ui.slot()
        .at(Rect::new(10.0, 10.0, 48.0, 48.0))
        .count(3)
        .show();
      ui.slot()
        .at(Rect::new(10.0, 60.0, 48.0, 48.0))
        .count(1)
        .show();
    });
    let corners: Vec<(f32, f32)> = d
      .commands
      .iter()
      .filter_map(|(_, c)| match c {
        UiCommand::Text { layout, origin, .. } => {
          let end = *origin + layout.size;
          Some((end.x, end.y))
        }
        _ => None,
      })
      .collect();
    assert_eq!(corners.len(), 1, "a count of 1 is not shown");
    let (right, bottom) = corners[0];
    assert!(
      (right - 55.0).abs() < 0.01 && (bottom - 55.0).abs() < 0.01,
      "3 in from the corner (58, 58): {corners:?}"
    );
  }

  #[test]
  fn a_selected_slot_has_an_accent_border() {
    use crate::core::ui::Theme;
    let mut d = UiDriver::new((100.0, 100.0));
    d.frame(|ui| {
      ui.slot()
        .at(Rect::new(0.0, 0.0, 48.0, 48.0))
        .selected(true)
        .show();
    });
    let borders: Vec<_> = d
      .commands
      .iter()
      .filter_map(|(_, c)| match c {
        UiCommand::Rect { border, .. } => Some(*border),
        _ => None,
      })
      .collect();
    let accent = Theme::dark().palette.accent;
    assert_eq!(borders, vec![Some((2.0, accent))]);
  }
}
