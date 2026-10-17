//! Tabs: a row of tab buttons, one selected, switched by a click, Confirm or
//! the shoulder buttons

use std::panic::Location;

use super::common::{WidgetOpts, widget_builders};
use crate::core::ui::Length;
use crate::core::ui::immediate::UiContext;
use crate::core::ui::layout::{Flow, UiStyle};
use crate::core::ui::memory::Flags;
use crate::core::ui::response::Response;
use crate::core::ui::theme::WidgetKind;

/// The theme variant the selected tab is drawn in
const SELECTED: &str = "selected";

/// A row of tabs being set up. It is drawn by [`show`](Self::show) or when
/// it is dropped.
pub struct Tabs<'u, 'a> {
  ui: Option<&'u mut UiContext<'a>>,
  site: &'static Location<'static>,
  selected: &'u mut usize,
  labels: &'u [&'u str],
  opts: WidgetOpts,
}

impl<'a> UiContext<'a> {
  /// A row of tabs labelled `labels`, sharing its width equally, with
  /// `labels[*selected]` drawn in the theme's `selected` variant. A click or
  /// Confirm on a tab selects it. The first tabs drawn in the top scene also
  /// follow `ui.tab_next` and `ui.tab_prev` (E and Q, the shoulder buttons),
  /// wrapping around, and move the focus to the newly selected tab; with no
  /// tabs on screen those keys stay the game's. The [`Response`] (the row's)
  /// reports `changed()` when the selection moved. `*selected` past the end
  /// is clamped to the last tab.
  ///
  /// ```ignore
  /// ui.tabs(&mut self.tab, &["Video", "Audio", "Controls"]).fill_width().show();
  /// match self.tab { 0 => video(ui), 1 => audio(ui), _ => controls(ui) }
  /// ```
  #[track_caller]
  pub fn tabs<'u>(&'u mut self, selected: &'u mut usize, labels: &'u [&'u str]) -> Tabs<'u, 'a> {
    Tabs {
      ui: Some(self),
      site: Location::caller(),
      selected,
      labels,
      opts: WidgetOpts::default(),
    }
  }
}

widget_builders!(Tabs<'_, '_>);

impl Tabs<'_, '_> {
  /// Draw the tabs; what happened to them this frame
  pub fn show(mut self) -> Response {
    self.run().unwrap_or_default()
  }

  /// Record the tabs, once (`show` and `Drop` both come here)
  fn run(&mut self) -> Option<Response> {
    let ui = self.ui.take()?;
    let id = ui.memory.scopes.make(self.site, self.opts.key);
    let count = self.labels.len();
    let disabled = self.opts.disabled;
    if let Some(last) = count.checked_sub(1) {
      *self.selected = (*self.selected).min(last);
    }
    let before = *self.selected;
    // The shoulder buttons, for the first tabs to take them
    let step = if count > 0 && !disabled {
      ui.take_tab_step()
    } else {
      0
    };
    if step != 0 {
      let count = count as i64;
      *self.selected = (*self.selected as i64 + i64::from(step)).rem_euclid(count) as usize;
    }
    let stepped = *self.selected != before;

    let style = UiStyle {
      flow: Flow::Row,
      ..self.opts.style.clone()
    };
    let row = ui.open_node(id, style, self.opts.at, Flags::default());
    ui.memory.scopes.push(id);
    let mut clicked = None;
    for (i, label) in self.labels.iter().enumerate() {
      let mut tab = ui
        .button(*label)
        .id(i)
        .kind(WidgetKind::Tab)
        .width(Length::Fill)
        .enabled(!disabled);
      if i == *self.selected {
        tab = tab.variant(SELECTED);
      }
      if let Some(patch) = &self.opts.look_patch {
        tab = tab.style(patch.clone());
      }
      if let Some(tip) = &self.opts.tooltip {
        tab = tab.tooltip(tip.clone());
      }
      let tab = tab.show();
      if tab.clicked() {
        clicked = Some(i);
      }
      // The old tab's contents are gone: focus stays on the tab bar
      if stepped && i == *self.selected {
        ui.request_focus(tab.id);
      }
    }
    ui.memory.scopes.pop();
    let mut response = ui.close_node(row);
    if let Some(i) = clicked {
      *self.selected = i;
    }
    if *self.selected != before {
      response.mark_changed();
    }
    Some(response)
  }
}

impl Drop for Tabs<'_, '_> {
  /// Tabs not shown are drawn when they are dropped
  fn drop(&mut self) {
    self.run();
  }
}

#[cfg(all(test, feature = "default-fonts"))]
mod tests {
  use crate::core::ui::test_util::UiDriver;
  use crate::core::ui::{Rect, UiContext};

  const LABELS: [&str; 3] = ["Audio", "Video", "Controls"];

  fn draw(ui: &mut UiContext<'_>, sel: &mut usize) {
    ui.column().focus_scope().show(|ui| {
      ui.tabs(sel, &LABELS)
        .at(Rect::new(0.0, 0.0, 300.0, 30.0))
        .show();
      ui.button("Inside").default_focus().show();
    });
  }

  /// The focused widget's rect, as last drawn
  fn focus_rect(d: &UiDriver) -> Option<Rect> {
    d.memory.prev(d.memory.interact.focused).map(|r| r.rect)
  }

  #[test]
  fn shoulder_buttons_switch_tabs_and_clicks_select() {
    let mut d = UiDriver::new((300.0, 200.0));
    let mut sel = 0;
    for _ in 0..2 {
      d.frame(|ui| draw(ui, &mut sel));
    }
    for expected in [1, 2, 0] {
      d.tab_next();
      d.frame(|ui| draw(ui, &mut sel));
      assert_eq!(sel, expected);
      d.frame(|ui| draw(ui, &mut sel));
      let third = Rect::new(100.0 * expected as f32, 0.0, 100.0, 30.0);
      let focus = focus_rect(&d);
      assert!(
        focus.is_some_and(|f| {
          let (x, y) = f.center();
          third.contains(x, y)
        }),
        "focus is on tab {expected}: {focus:?}"
      );
    }
    d.move_to(150.0, 15.0);
    d.press();
    d.frame(|ui| draw(ui, &mut sel));
    d.release();
    d.frame(|ui| draw(ui, &mut sel));
    assert_eq!(sel, 1, "a click selects");
  }

  #[test]
  fn tab_keys_are_left_alone_without_tabs() {
    let mut d = UiDriver::new((300.0, 200.0));
    let draw = |ui: &mut UiContext<'_>| {
      ui.column().focus_scope().show(|ui| {
        ui.button("Only").default_focus().show();
      });
    };
    for _ in 0..2 {
      d.frame(draw);
    }
    d.tab_next();
    d.frame(draw);
    assert!(!d.memory.interact.tab_taken);
    assert!(!d.last_outcome().used.contains(&"ui.tab_next"));
  }

  #[test]
  fn the_selected_tab_is_drawn_in_the_selected_look_and_tabs_share_the_row() {
    use crate::core::ui::immediate::UiCommand;
    use crate::core::ui::theme::{Theme, WidgetKind, WidgetState};
    let mut d = UiDriver::new((300.0, 200.0));
    let mut sel = 1;
    d.frame(|ui| {
      ui.column().focus_scope().show(|ui| {
        ui.tabs(&mut sel, &LABELS)
          .at(Rect::new(0.0, 0.0, 300.0, 30.0))
          .show();
      });
    });
    let theme = Theme::dark();
    let selected = theme.look(WidgetKind::Tab, Some("selected"), WidgetState::Normal);
    let plain = theme.look(WidgetKind::Tab, None, WidgetState::Normal);
    assert_ne!(selected.fill, plain.fill);
    let tabs: Vec<(Rect, crate::core::ui::Color)> = d
      .commands
      .iter()
      .filter_map(|(_, c)| match c {
        UiCommand::Rect { rect, fill, .. } => Some((*rect, *fill)),
        _ => None,
      })
      .collect();
    assert_eq!(
      tabs.iter().map(|(r, _)| r.width).collect::<Vec<_>>(),
      vec![100.0; 3]
    );
    assert_eq!(
      tabs.iter().map(|(_, f)| *f).collect::<Vec<_>>(),
      vec![plain.fill, selected.fill, plain.fill]
    );
  }
}
