//! The checkbox

use std::panic::Location;

use super::common::{WidgetOpts, widget_builders};
use super::mark_row::{Mark, MarkRow};
use crate::core::text::RichText;
use crate::core::ui::immediate::UiContext;
use crate::core::ui::response::Response;
use crate::core::ui::theme::WidgetKind;

/// A checkbox being set up. It is drawn by [`show`](Self::show) or when it is
/// dropped.
pub struct Checkbox<'u, 'a> {
  ui: Option<&'u mut UiContext<'a>>,
  site: &'static Location<'static>,
  checked: &'u mut bool,
  label: RichText,
  opts: WidgetOpts,
}

impl<'a> UiContext<'a> {
  /// A checkbox with a label, writing `checked`. A click, or Confirm while
  /// it has the focus, flips it and the [`Response`] reports `changed()`.
  ///
  /// ```ignore
  /// if ui.checkbox(&mut self.fullscreen, "Fullscreen").show().changed() {
  ///     apply_video_settings(self);
  /// }
  /// ```
  #[track_caller]
  pub fn checkbox<'u>(
    &'u mut self,
    checked: &'u mut bool,
    label: impl Into<RichText>,
  ) -> Checkbox<'u, 'a> {
    Checkbox {
      ui: Some(self),
      site: Location::caller(),
      checked,
      label: label.into(),
      opts: WidgetOpts::default(),
    }
  }
}

widget_builders!(Checkbox<'_, '_>);

impl Checkbox<'_, '_> {
  /// Draw the checkbox; what happened to it this frame
  pub fn show(mut self) -> Response {
    self.run().unwrap_or_default()
  }

  /// Record the checkbox, once (`show` and `Drop` both come here)
  fn run(&mut self) -> Option<Response> {
    let ui = self.ui.take()?;
    let spec = MarkRow {
      kind: WidgetKind::Checkbox,
      mark: Mark::Check,
      label: std::mem::take(&mut self.label),
    };
    let checked = &mut *self.checked;
    Some(ui.mark_row(self.site, &self.opts, spec, |clicked| {
      if clicked {
        *checked = !*checked;
      }
      (*checked, clicked)
    }))
  }
}

impl Drop for Checkbox<'_, '_> {
  /// A checkbox not shown is drawn when it is dropped
  fn drop(&mut self) {
    self.run();
  }
}

#[cfg(all(test, feature = "default-fonts"))]
mod tests {
  use crate::core::ui::test_util::UiDriver;
  use crate::core::ui::{Rect, UiContext};

  #[test]
  fn clicking_a_checkbox_toggles_it_and_reports_changed() {
    let mut d = UiDriver::new((200.0, 100.0));
    let mut on = false;
    let draw = |ui: &mut UiContext<'_>, on: &mut bool| {
      ui.checkbox(on, "Sound")
        .at(Rect::new(10.0, 10.0, 120.0, 24.0))
        .show()
        .changed()
    };
    d.frame(|ui| draw(ui, &mut on));
    d.move_to(15.0, 20.0);
    d.press();
    d.frame(|ui| draw(ui, &mut on));
    d.release();
    assert!(d.frame(|ui| draw(ui, &mut on)));
    assert!(on);
  }

  #[test]
  fn confirm_toggles_a_focused_checkbox() {
    let mut d = UiDriver::new((200.0, 100.0));
    let mut on = true;
    let draw = |ui: &mut UiContext<'_>, on: &mut bool| {
      ui.column().focus_scope().show(|ui| {
        ui.checkbox(on, "Music").default_focus().show();
      });
    };
    d.frame(|ui| draw(ui, &mut on));
    d.frame(|ui| draw(ui, &mut on));
    d.confirm();
    d.frame(|ui| draw(ui, &mut on));
    assert!(!on);
  }

  #[test]
  fn a_disabled_checkbox_ignores_clicks_and_focus() {
    let mut d = UiDriver::new((200.0, 100.0));
    let mut on = false;
    let draw = |ui: &mut UiContext<'_>, on: &mut bool| {
      ui.column().focus_scope().show(|ui| {
        ui.checkbox(on, "Locked")
          .enabled(false)
          .default_focus()
          .show();
      });
    };
    d.frame(|ui| draw(ui, &mut on));
    d.move_to(10.0, 10.0);
    d.press();
    d.frame(|ui| draw(ui, &mut on));
    d.release();
    d.confirm();
    d.frame(|ui| draw(ui, &mut on));
    assert!(!on);
    assert_eq!(
      d.memory.interact.focused,
      crate::core::ui::UiId::NONE,
      "nothing to focus"
    );
  }

  #[test]
  fn a_checked_box_draws_a_check_mark_in_the_accent() {
    use crate::core::ui::Color;
    use crate::core::ui::immediate::UiCommand;
    let mut d = UiDriver::new((200.0, 100.0));
    let mut on = true;
    d.frame(|ui| {
      ui.checkbox(&mut on, "")
        .at(Rect::new(0.0, 0.0, 20.0, 20.0))
        .show();
    });
    let fills: Vec<Color> = d
      .commands
      .iter()
      .filter_map(|(_, c)| match c {
        UiCommand::Rect { fill, .. } => Some(*fill),
        _ => None,
      })
      .collect();
    assert_eq!(fills, vec![Color::rgb(80, 150, 255)]);
    let lines = d
      .commands
      .iter()
      .filter(|(_, c)| matches!(c, UiCommand::Line { .. }))
      .count();
    assert_eq!(lines, 2, "the two strokes of the check");
  }
}
