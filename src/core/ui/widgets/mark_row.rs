//! The row a checkbox, a toggle and a radio share: a mark, a gap and a label

use std::panic::Location;

use glam::Vec2;

use super::common::WidgetOpts;
use crate::core::text::{RichText, TextLayoutOptions};
use crate::core::ui::Color;
use crate::core::ui::immediate::{UiCommand, UiContext};
use crate::core::ui::layout::{Align, Edges, Flow, Measure, UiStyle};
use crate::core::ui::memory::Flags;
use crate::core::ui::response::{Response, Sense};
use crate::core::ui::theme::WidgetKind;
use crate::core::ui::tree::{DrawKind, Node};

/// The mark at the row's start
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Mark {
  /// A square with a check mark when on
  Check,
  /// A track with a knob that sits right when on
  Switch,
  /// A circle with a dot when on
  Dot,
}

/// What differs between the widgets that use the row
pub(crate) struct MarkRow {
  pub(crate) kind: WidgetKind,
  pub(crate) mark: Mark,
  pub(crate) label: RichText,
}

/// `color` with its alpha scaled by `look_alpha` (a disabled look's)
fn dimmed(color: Color, look_alpha: u8) -> Color {
  let alpha = u16::from(color.a) * u16::from(look_alpha) / 255;
  color.with_alpha(alpha as u8)
}

impl UiContext<'_> {
  /// Record a mark row. A click or Confirm calls `apply(true)`; the row
  /// draws the state `apply` returns: (`on`, `changed`).
  pub(crate) fn mark_row(
    &mut self,
    site: &'static Location<'static>,
    opts: &WidgetOpts,
    spec: MarkRow,
    apply: impl FnOnce(bool) -> (bool, bool),
  ) -> Response {
    let MarkRow { kind, mark, label } = spec;
    let id = self.memory.scopes.make(site, opts.key);
    let disabled = opts.disabled;
    let mut response = self.widget_response(id, Sense::click(), opts);
    let (on, changed) = apply(response.clicked());
    if changed {
      response.mark_changed();
    }

    let state = self.widget_state(&response, disabled);
    let look = self.look_of(kind, opts, state);
    let theme = self.theme_arc();
    let sizes = &theme.metrics.sizes;
    let size = match mark {
      Mark::Switch => Vec2::new(sizes.toggle[0], sizes.toggle[1]),
      _ => Vec2::splat(sizes.checkbox),
    };
    let has_label = !label.text.is_empty() || !label.icons.is_empty();

    let style = UiStyle {
      flow: Flow::Row,
      align: Align::Center,
      gap: theme.metrics.spacing,
      ..opts.style.clone()
    };
    let mut row = Node::new(id, style, Measure::None);
    row.record = Some(opts.record(Flags::CLICK, true));
    let row = self.push_node(row, opts.at);
    self.memory.scopes.push(id);

    // The mark
    let mark_id = self.memory.scopes.make(site, None);
    let node = self.push_node(
      Node::new(mark_id, UiStyle::default(), Measure::Fixed(size)),
      None,
    );
    let mut box_look = look.clone();
    let radius = match mark {
      Mark::Check => look.radius.min(size.y / 2.0),
      Mark::Switch | Mark::Dot => size.y / 2.0,
    };
    box_look.radius = radius;
    if on && mark != Mark::Dot {
      box_look.fill = dimmed(theme.palette.accent, look.text.a);
    }
    self.push_look_box(node, &box_look);
    let ink = dimmed(Color::WHITE, look.text.a);
    match (mark, on) {
      (Mark::Check, true) => {
        for points in [
          [Vec2::new(0.24, 0.52) * size, Vec2::new(0.42, 0.70) * size],
          [Vec2::new(0.42, 0.70) * size, Vec2::new(0.77, 0.32) * size],
        ] {
          self.push_draw(
            node,
            DrawKind::Local(UiCommand::Line {
              points: points.into(),
              at: Vec2::ZERO,
              width: 2.0,
              color: ink,
              closed: false,
              clip: None,
              layer: 0,
            }),
          );
        }
      }
      (Mark::Switch, on) => {
        let knob = (size.y - 6.0).max(0.0);
        self.push_draw(
          node,
          DrawKind::Box {
            inset: Edges::all(3.0),
            frac: Vec2::new(if on { 1.0 } else { 0.0 }, 0.5),
            size: Vec2::splat(knob),
            fill: ink,
            radius: knob / 2.0,
            border: None,
          },
        );
      }
      (Mark::Dot, true) => {
        self.push_draw(
          node,
          DrawKind::Box {
            inset: Edges::ZERO,
            frac: Vec2::splat(0.5),
            size: Vec2::splat(size.x * 0.5),
            fill: dimmed(theme.palette.accent, look.text.a),
            radius: size.x * 0.25,
            border: None,
          },
        );
      }
      _ => {}
    }
    self.close_node(node);

    // The label, in the body role
    if has_label {
      let text_style = theme.text.body.style();
      let leaf = self.leaf_of(label, text_style, TextLayoutOptions::default());
      let opacity = f32::from(look.text.a) / 255.0;
      self.text_leaf(site, leaf, (opacity, None), Vec2::ZERO);
    }

    self.memory.scopes.pop();
    if response.focused() && self.focus_visible() {
      self.focus_ring(row, look.radius);
    }
    response.rect = self.close_node(row).rect;
    self.tooltip_tick(opts.tooltip.as_ref(), &response);
    response
  }
}
