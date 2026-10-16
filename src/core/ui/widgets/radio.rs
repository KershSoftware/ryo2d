//! The radio button

use std::panic::Location;

use super::common::{WidgetOpts, widget_builders};
use super::mark_row::{Mark, MarkRow};
use crate::core::text::RichText;
use crate::core::ui::immediate::UiContext;
use crate::core::ui::response::Response;
use crate::core::ui::theme::WidgetKind;

/// A radio button being set up. It is drawn by [`show`](Self::show) or when
/// it is dropped.
pub struct Radio<'u, 'a, T: PartialEq + Copy> {
  ui: Option<&'u mut UiContext<'a>>,
  site: &'static Location<'static>,
  current: &'u mut T,
  value: T,
  label: RichText,
  opts: WidgetOpts,
}

impl<'a> UiContext<'a> {
  /// A radio button for `value`, selected while `*current == value`. A
  /// click, or Confirm while it has the focus, sets `*current = value`; the
  /// [`Response`] reports `changed()` when that was a change.
  ///
  /// ```ignore
  /// for (size, name) in [(Size::Small, "Small"), (Size::Large, "Large")] {
  ///     ui.radio(&mut self.size, size, name).id(name).show();
  /// }
  /// ```
  #[track_caller]
  pub fn radio<'u, T: PartialEq + Copy>(
    &'u mut self,
    current: &'u mut T,
    value: T,
    label: impl Into<RichText>,
  ) -> Radio<'u, 'a, T> {
    Radio {
      ui: Some(self),
      site: Location::caller(),
      current,
      value,
      label: label.into(),
      opts: WidgetOpts::default(),
    }
  }
}

widget_builders!([T: PartialEq + Copy] Radio<'_, '_, T>);

impl<T: PartialEq + Copy> Radio<'_, '_, T> {
  /// Draw the radio button; what happened to it this frame
  pub fn show(mut self) -> Response {
    self.run().unwrap_or_default()
  }

  /// Record the radio button, once (`show` and `Drop` both come here)
  fn run(&mut self) -> Option<Response> {
    let ui = self.ui.take()?;
    let spec = MarkRow {
      kind: WidgetKind::Radio,
      mark: Mark::Dot,
      label: std::mem::take(&mut self.label),
    };
    let (current, value) = (&mut *self.current, self.value);
    Some(ui.mark_row(self.site, &self.opts, spec, |clicked| {
      let changed = clicked && *current != value;
      if clicked {
        *current = value;
      }
      (*current == value, changed)
    }))
  }
}

impl<T: PartialEq + Copy> Drop for Radio<'_, '_, T> {
  /// A radio button not shown is drawn when it is dropped
  fn drop(&mut self) {
    self.run();
  }
}

#[cfg(all(test, feature = "default-fonts"))]
mod tests {
  use crate::core::ui::test_util::UiDriver;
  use crate::core::ui::{Rect, UiContext};

  #[derive(Debug, Clone, Copy, PartialEq)]
  enum Size {
    Small,
    Medium,
    Large,
  }

  /// Three 30-tall radios from y 0; whether any changed
  fn radios(ui: &mut UiContext<'_>, size: &mut Size) -> bool {
    let mut changed = false;
    ui.column()
      .gap(0.0)
      .at(Rect::new(0.0, 0.0, 200.0, 90.0))
      .show(|ui| {
        for (value, label) in [
          (Size::Small, "Small"),
          (Size::Medium, "Medium"),
          (Size::Large, "Large"),
        ] {
          changed |= ui
            .radio(size, value, label)
            .id(label)
            .height(30.0)
            .show()
            .changed();
        }
      });
    changed
  }

  fn click_third(d: &mut UiDriver, size: &mut Size) -> bool {
    d.move_to(10.0, 75.0);
    d.press();
    d.frame(|ui| radios(ui, size));
    d.release();
    d.frame(|ui| radios(ui, size))
  }

  #[test]
  fn a_radio_sets_its_value() {
    let mut d = UiDriver::new((200.0, 100.0));
    let mut size = Size::Small;
    d.frame(|ui| radios(ui, &mut size));
    assert!(click_third(&mut d, &mut size));
    assert_eq!(size, Size::Large);
    assert!(!click_third(&mut d, &mut size), "already Large: no change");
  }
}
