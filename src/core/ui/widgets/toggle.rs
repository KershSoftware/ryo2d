//! The toggle switch

use std::panic::Location;

use super::common::{WidgetOpts, widget_builders};
use super::mark_row::{Mark, MarkRow};
use crate::core::text::RichText;
use crate::core::ui::immediate::UiContext;
use crate::core::ui::response::Response;
use crate::core::ui::theme::WidgetKind;

/// A toggle being set up. It is drawn by [`show`](Self::show) or when it is
/// dropped.
pub struct Toggle<'u, 'a> {
  ui: Option<&'u mut UiContext<'a>>,
  site: &'static Location<'static>,
  on: &'u mut bool,
  label: RichText,
  opts: WidgetOpts,
}

impl<'a> UiContext<'a> {
  /// A switch with a label, writing `on`. A click, or Confirm while it has
  /// the focus, flips it and the [`Response`] reports `changed()`.
  ///
  /// ```ignore
  /// ui.toggle(&mut self.music, "Music").show();
  /// ```
  #[track_caller]
  pub fn toggle<'u>(&'u mut self, on: &'u mut bool, label: impl Into<RichText>) -> Toggle<'u, 'a> {
    Toggle {
      ui: Some(self),
      site: Location::caller(),
      on,
      label: label.into(),
      opts: WidgetOpts::default(),
    }
  }
}

widget_builders!(Toggle<'_, '_>);

impl Toggle<'_, '_> {
  /// Draw the toggle; what happened to it this frame
  pub fn show(mut self) -> Response {
    self.run().unwrap_or_default()
  }

  /// Record the toggle, once (`show` and `Drop` both come here)
  fn run(&mut self) -> Option<Response> {
    let ui = self.ui.take()?;
    let spec = MarkRow {
      kind: WidgetKind::Toggle,
      mark: Mark::Switch,
      label: std::mem::take(&mut self.label),
    };
    let on = &mut *self.on;
    Some(ui.mark_row(self.site, &self.opts, spec, |clicked| {
      if clicked {
        *on = !*on;
      }
      (*on, clicked)
    }))
  }
}

impl Drop for Toggle<'_, '_> {
  /// A toggle not shown is drawn when it is dropped
  fn drop(&mut self) {
    self.run();
  }
}

#[cfg(all(test, feature = "default-fonts"))]
mod tests {
  use crate::core::ui::test_util::UiDriver;
  use crate::core::ui::{Rect, UiContext};

  /// One call site, so it is the same toggle every frame
  fn draw(ui: &mut UiContext<'_>, on: &mut bool) -> bool {
    ui.toggle(on, "Music")
      .at(Rect::new(10.0, 10.0, 120.0, 24.0))
      .show()
      .changed()
  }

  fn click(d: &mut UiDriver, on: &mut bool) -> bool {
    d.move_to(20.0, 20.0);
    d.press();
    d.frame(|ui| draw(ui, on));
    d.release();
    d.frame(|ui| draw(ui, on))
  }

  #[test]
  fn a_toggle_flips_on_click() {
    let mut d = UiDriver::new((200.0, 100.0));
    let mut on = false;
    d.frame(|ui| draw(ui, &mut on));
    assert!(click(&mut d, &mut on));
    assert!(on);
    assert!(click(&mut d, &mut on));
    assert!(!on);
  }
}
