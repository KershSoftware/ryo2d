//! Separators and spacers

use std::panic::Location;

use crate::core::ui::Edges;
use crate::core::ui::immediate::UiContext;
use crate::core::ui::layout::{Flow, Length, Measure, UiStyle};
use crate::core::ui::response::Response;
use crate::core::ui::tree::{DrawKind, Node};

impl UiContext<'_> {
  /// A one-unit line across its container: horizontal in a column,
  /// vertical in a row.
  ///
  /// ```ignore
  /// ui.column().show(|ui| {
  ///     ui.text("Audio");
  ///     ui.separator();
  ///     ui.text("Video");
  /// });
  /// ```
  #[track_caller]
  pub fn separator(&mut self) -> Response {
    let site = Location::caller();
    let id = self.memory.scopes.make(site, None);
    let vertical = matches!(self.open_flow(), Some(Flow::Row));
    let style = if vertical {
      UiStyle {
        width: Length::Px(1.0),
        height: Length::Fill,
        ..UiStyle::default()
      }
    } else {
      UiStyle {
        width: Length::Fill,
        height: Length::Px(1.0),
        ..UiStyle::default()
      }
    };
    let muted = self.theme().palette.muted;
    let node = self.push_node(Node::new(id, style, Measure::None), None);
    self.push_draw(
      node,
      DrawKind::Fill {
        inset: Edges::ZERO,
        fill: muted.with_alpha(muted.a / 2),
        radius: 0.0,
        border: None,
      },
    );
    self.close_node(node)
  }

  /// Empty room that takes what its container has left. Several share it.
  ///
  /// ```ignore
  /// ui.row().fill_width().show(|ui| {
  ///     ui.text("Score");
  ///     ui.spacer();
  ///     ui.text("1200");
  /// });
  /// ```
  #[track_caller]
  pub fn spacer(&mut self) -> Response {
    let id = self.memory.scopes.make(Location::caller(), None);
    let style = UiStyle {
      grow: 1.0,
      ..UiStyle::default()
    };
    let node = self.push_node(Node::new(id, style, Measure::None), None);
    self.close_node(node)
  }
}

#[cfg(all(test, feature = "default-fonts"))]
mod tests {
  use crate::core::ui::immediate::UiCommand;
  use crate::core::ui::test_util::UiDriver;
  use crate::core::ui::{Rect, UiContext};

  fn rects(d: &UiDriver) -> Vec<Rect> {
    d.commands
      .iter()
      .filter_map(|(_, c)| match c {
        UiCommand::Rect { rect, .. } => Some(*rect),
        _ => None,
      })
      .collect()
  }

  #[test]
  fn a_separator_crosses_its_container() {
    let mut d = UiDriver::new((300.0, 100.0));
    d.frame(|ui: &mut UiContext<'_>| {
      ui.column().at(Rect::new(0.0, 0.0, 200.0, 50.0)).show(|ui| {
        ui.separator();
      });
      ui.row().at(Rect::new(0.0, 60.0, 200.0, 30.0)).show(|ui| {
        ui.separator();
      });
    });
    assert_eq!(
      rects(&d),
      vec![
        Rect::new(0.0, 0.0, 200.0, 1.0),
        Rect::new(0.0, 60.0, 1.0, 30.0)
      ],
      "a line across a column, a line down a row"
    );
  }

  #[test]
  fn a_spacer_pushes_what_follows_to_the_end() {
    let mut d = UiDriver::new((300.0, 100.0));
    d.frame(|ui: &mut UiContext<'_>| {
      ui.row().at(Rect::new(0.0, 60.0, 200.0, 30.0)).show(|ui| {
        ui.spacer();
        ui.separator();
      });
    });
    assert_eq!(rects(&d), vec![Rect::new(199.0, 60.0, 1.0, 30.0)]);
  }
}
