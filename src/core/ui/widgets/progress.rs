//! The progress bar

use std::panic::Location;

use glam::Vec2;

use super::common::{WidgetOpts, widget_builders};
use crate::core::text::{RichText, TextLayoutOptions};
use crate::core::ui::immediate::{UiContext, text_seed};
use crate::core::ui::layout::{Edges, Measure};
use crate::core::ui::memory::Flags;
use crate::core::ui::response::{Response, Sense};
use crate::core::ui::theme::{WidgetKind, WidgetState};
use crate::core::ui::tree::{DrawKind, Node, TextLeaf};

/// A bar's size unless `.width()` and `.height()` say otherwise
const SIZE: Vec2 = Vec2::new(160.0, 12.0);

/// A progress bar being set up. It is drawn by [`show`](Self::show) or when
/// it is dropped.
pub struct Progress<'u, 'a> {
  ui: Option<&'u mut UiContext<'a>>,
  site: &'static Location<'static>,
  fraction: f32,
  label: Option<RichText>,
  opts: WidgetOpts,
}

impl<'a> UiContext<'a> {
  /// A bar filled `fraction` (0 to 1, clamped; a number that is not one
  /// is 0) of the way. It senses nothing, unless it has a tooltip.
  ///
  /// ```ignore
  /// ui.progress(self.health / self.max_health).label("HP").width(200.0);
  /// ```
  #[track_caller]
  pub fn progress<'u>(&'u mut self, fraction: f32) -> Progress<'u, 'a> {
    Progress {
      ui: Some(self),
      site: Location::caller(),
      fraction,
      label: None,
      opts: WidgetOpts::default(),
    }
  }
}

widget_builders!(Progress<'_, '_>);

impl Progress<'_, '_> {
  /// Text centred over the bar
  pub fn label(mut self, label: impl Into<RichText>) -> Self {
    self.label = Some(label.into());
    self
  }

  /// Draw the bar; what happened to it this frame
  pub fn show(mut self) -> Response {
    self.run().unwrap_or_default()
  }

  /// Record the bar, once (`show` and `Drop` both come here)
  fn run(&mut self) -> Option<Response> {
    let ui = self.ui.take()?;
    let id = ui.memory.scopes.make(self.site, self.opts.key);
    let disabled = self.opts.disabled;
    let mut response = ui.widget_response(id, Sense::hover(), &self.opts);
    let state = if disabled {
      WidgetState::Disabled
    } else {
      WidgetState::Normal
    };
    let look = ui.look_of(WidgetKind::Progress, &self.opts, state);
    let theme = ui.theme_arc();
    let fraction = if self.fraction.is_finite() {
      self.fraction.clamp(0.0, 1.0)
    } else {
      0.0
    };

    let mut node = Node::new(id, self.opts.style.clone(), Measure::Fixed(SIZE));
    if self.opts.tooltip.is_some() || self.opts.drag.is_set() {
      node.record = Some(self.opts.record(Flags::HOVER, false));
    }
    let index = ui.push_node(node, self.opts.at);
    ui.push_draw(
      index,
      DrawKind::Fill {
        inset: Edges::ZERO,
        fill: look.fill,
        radius: look.radius,
        border: None,
      },
    );
    ui.push_draw(
      index,
      DrawKind::Fraction {
        inset: Edges::ZERO,
        to: fraction,
        band: None,
        fill: look.border,
        radius: look.radius,
      },
    );
    if let Some(label) = self.label.take() {
      let style = theme.text.body.style();
      let options = TextLayoutOptions::default();
      let seed = text_seed(&label, &style, &options);
      ui.push_draw(
        index,
        DrawKind::Text {
          leaf: TextLeaf {
            text: label,
            style,
            options,
          },
          frac: Vec2::splat(0.5),
          wrap: false,
          opacity: f32::from(look.text.a) / 255.0,
          reveal: None,
          seed,
        },
      );
    }
    response.rect = ui.close_node(index).rect;
    ui.tooltip_tick(self.opts.tooltip.as_ref(), &response);
    Some(response)
  }
}

impl Drop for Progress<'_, '_> {
  /// A bar not shown is drawn when it is dropped
  fn drop(&mut self) {
    self.run();
  }
}

#[cfg(all(test, feature = "default-fonts"))]
mod tests {
  use crate::core::ui::Rect;
  use crate::core::ui::immediate::UiCommand;
  use crate::core::ui::test_util::UiDriver;

  /// The widths of the rects a 160-wide bar draws at `fraction`: the track, then the fill
  fn widths(fraction: f32) -> Vec<f32> {
    let mut d = UiDriver::new((300.0, 100.0));
    d.frame(|ui| {
      ui.progress(fraction)
        .at(Rect::new(0.0, 0.0, 160.0, 12.0))
        .show();
    });
    d.commands
      .iter()
      .filter_map(|(_, c)| match c {
        UiCommand::Rect { rect, .. } => Some(rect.width),
        _ => None,
      })
      .collect()
  }

  #[test]
  fn progress_clamps_its_fraction() {
    assert_eq!(widths(0.5), vec![160.0, 80.0]);
    assert_eq!(widths(2.0), vec![160.0, 160.0]);
    assert_eq!(
      widths(f32::NAN),
      vec![160.0],
      "NaN is 0, and an empty fill draws nothing"
    );
  }
}
