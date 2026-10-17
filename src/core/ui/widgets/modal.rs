//! Modals and confirm dialogs: a dim over the whole UI that blocks the
//! pointer, and a centred dialog that traps the focus; and `ui.popup`, a
//! panel below a widget (a context menu)

use std::panic::Location;

use crate::core::ui::containers::Container;
use crate::core::ui::immediate::UiContext;
use crate::core::ui::{Justify, UiId};

/// The widest a confirm dialog's message grows before it wraps
const MESSAGE_WIDTH: f32 = 320.0;

/// A modal being set up: a [`Container`] (the dialog, a column with the
/// panel look) whose `.show(|ui| ..)` dims the whole UI under it
pub type Modal<'u, 'a> = Container<'u, 'a>;

impl<'a> UiContext<'a> {
  /// A popup below widget `anchor` (where it was last drawn; centred on the
  /// UI when it was not): a column with the panel look, on the popup layer
  /// above everything the scene draws, that traps the focus while it shows.
  /// It moves above its anchor when there is no room below, stays inside
  /// the UI and is at least as wide as the anchor. The game decides when it
  /// shows, and typically closes it on `ui.back()` or a pick.
  ///
  /// ```ignore
  /// let more = ui.button("More").show();
  /// if more.clicked() { self.menu = !self.menu; }
  /// if self.menu {
  ///     ui.popup(more.id).show(|ui| {
  ///         if ui.button("Rename").clicked() { self.menu = false; }
  ///     });
  ///     if ui.back() { self.menu = false; }
  /// }
  /// ```
  #[track_caller]
  pub fn popup<'u>(&'u mut self, anchor: UiId) -> Container<'u, 'a> {
    let rect = self
      .memory
      .current(anchor)
      .or_else(|| self.memory.prev(anchor))
      .map(|record| record.rect);
    self.popup_panel(Location::caller(), rect)
  }

  /// A modal: a dialog (a column with the panel look) centred over a dim
  /// of the whole UI, on the popup layer. Nothing under it takes the
  /// pointer, focus moves into it when it appears and back when it goes,
  /// and navigation stays inside it. It does not close itself: the game
  /// stops showing it, typically on `ui.back()` or a button.
  ///
  /// ```ignore
  /// if self.paused {
  ///     ui.modal().show(|ui| {
  ///         ui.text("Paused");
  ///         if ui.button("Resume").default_focus().clicked() { self.paused = false; }
  ///     });
  ///     if ui.back() { self.paused = false; }
  /// }
  /// ```
  #[track_caller]
  pub fn modal<'u>(&'u mut self) -> Modal<'u, 'a> {
    self.modal_panel(Location::caller())
  }

  /// A yes/no modal: `title`, `message` and Yes and No buttons, No focused
  /// first. `Some(true)` or `Some(false)` when one is chosen, `Some(false)`
  /// on Back, `None` otherwise. The game shows it while it waits for an
  /// answer.
  ///
  /// Its ids come from its own code, not the line that calls it, so it is
  /// the same dialog wherever it is drawn from; two drawn in one frame are
  /// told apart by their order, as a helper called twice is.
  ///
  /// ```ignore
  /// if self.quitting {
  ///     match ui.confirm("Quit?", "Unsaved progress is lost.") {
  ///         Some(true) => return SceneAction::Quit,
  ///         Some(false) => self.quitting = false,
  ///         None => {}
  ///     }
  /// }
  /// ```
  pub fn confirm(&mut self, title: &str, message: &str) -> Option<bool> {
    self.confirm_with(title, message, "Yes", "No")
  }

  /// [`confirm`](Self::confirm) with its buttons labelled `yes` and `no`
  pub fn confirm_with(&mut self, title: &str, message: &str, yes: &str, no: &str) -> Option<bool> {
    let theme = self.theme_arc();
    let heading = theme.text.heading.style();
    let body = theme.text.body.style();
    let gap = theme.metrics.spacing;
    let mut answer = None;
    self.modal().gap(gap).show(|ui| {
      ui.text(title).style(&heading);
      ui.text(message).style(&body).wrap(MESSAGE_WIDTH);
      ui.row().gap(gap).justify(Justify::End).show(|ui| {
        if ui.button(yes).clicked() {
          answer = Some(true);
        }
        if ui.button(no).default_focus().clicked() {
          answer = Some(false);
        }
      });
    });
    if answer.is_none() && self.back() {
      answer = Some(false);
    }
    answer
  }
}

#[cfg(all(test, feature = "default-fonts"))]
mod tests {
  use crate::core::input::Direction;
  use crate::core::ui::test_util::UiDriver;
  use crate::core::ui::{Rect, UiContext};

  #[test]
  fn a_popup_sits_below_its_anchor_and_traps_focus() {
    let mut d = UiDriver::new((300.0, 200.0));
    let draw = |ui: &mut UiContext<'_>| {
      let mut inner = None;
      let anchor = ui
        .button("More")
        .at(Rect::new(10.0, 10.0, 80.0, 24.0))
        .show();
      ui.popup(anchor.id)
        .show(|ui| inner = Some(ui.button("Rename").show()));
      inner
    };
    d.frame(draw);
    d.frame(draw);
    let Some(inner) = d.frame(draw) else {
      panic!("the popup drew no button");
    };
    assert!(inner.rect.y >= 34.0, "below the anchor: {:?}", inner.rect);
    assert_eq!(
      d.frame(|ui| {
        draw(ui);
        ui.focused()
      }),
      Some(inner.id),
      "the popup took focus"
    );
  }

  #[test]
  fn a_modal_blocks_the_page_under_it() {
    let mut d = UiDriver::new((300.0, 200.0));
    let page = Rect::new(10.0, 10.0, 80.0, 30.0);
    let draw = |ui: &mut UiContext<'_>| {
      let clicked = ui.button("Page").at(page).clicked();
      ui.modal().show(|ui| {
        ui.text("Paused");
      });
      clicked
    };
    d.frame(draw);
    d.move_to(20.0, 20.0);
    d.press();
    d.frame(draw);
    d.release();
    assert!(!d.frame(draw));
  }

  #[test]
  fn a_modal_traps_focus_and_returns_it_when_closed() {
    let mut d = UiDriver::new((300.0, 200.0));
    let mut open = false;
    let draw = |ui: &mut UiContext<'_>, open: bool| {
      let mut page = None;
      ui.column()
        .focus_scope()
        .show(|ui| page = Some(ui.button("Settings").default_focus().show().id));
      let mut inner = None;
      if open {
        ui.modal()
          .show(|ui| inner = Some(ui.button("Close").show().id));
      }
      (page, inner)
    };
    let (page, _) = d.frame(|ui| draw(ui, open));
    d.frame(|ui| draw(ui, open));
    open = true;
    let (_, inner) = d.frame(|ui| draw(ui, open));
    d.frame(|ui| draw(ui, open));
    assert_eq!(d.frame(|ui| ui.focused()), inner);
    d.nav(Direction::Up);
    d.frame(|ui| draw(ui, open));
    assert_eq!(d.frame(|ui| ui.focused()), inner, "trapped");
    open = false;
    d.frame(|ui| draw(ui, open));
    d.frame(|ui| draw(ui, open));
    assert_eq!(d.frame(|ui| ui.focused()), page);
  }

  #[test]
  fn back_closes_the_innermost_popup_first() {
    let mut d = UiDriver::new((400.0, 300.0));
    let mut sel = 0;
    let draw = |ui: &mut UiContext<'_>, sel: &mut usize| {
      let mut back = false;
      ui.modal().show(|ui| {
        ui.dropdown(sel, &["a", "b"]).default_focus().show();
      });
      back |= ui.back();
      back
    };
    for _ in 0..2 {
      d.frame(|ui| draw(ui, &mut sel));
    }
    d.confirm(); // opens the focused dropdown
    d.frame(|ui| draw(ui, &mut sel));
    d.frame(|ui| draw(ui, &mut sel));
    d.back();
    assert!(
      !d.frame(|ui| draw(ui, &mut sel)),
      "the list took the first Back"
    );
    d.back();
    assert!(
      d.frame(|ui| draw(ui, &mut sel)),
      "the game gets the second, to close the modal"
    );
  }

  #[test]
  fn confirm_returns_the_choice_and_back_means_no() {
    let mut d = UiDriver::new((400.0, 300.0));
    for _ in 0..2 {
      d.frame(|ui| ui.confirm("Quit?", "Unsaved progress is lost."));
    }
    d.back();
    assert_eq!(
      d.frame(|ui| ui.confirm("Quit?", "Unsaved progress is lost.")),
      Some(false)
    );
    for _ in 0..2 {
      d.frame(|ui| ui.confirm("Quit?", "Unsaved progress is lost."));
    }
    d.nav(Direction::Left); // from No to Yes
    d.frame(|ui| ui.confirm("Quit?", "Unsaved progress is lost."));
    d.confirm();
    assert_eq!(
      d.frame(|ui| ui.confirm("Quit?", "Unsaved progress is lost.")),
      Some(true)
    );
  }

  #[test]
  fn a_modal_dims_the_whole_ui_and_centres_its_dialog() {
    use crate::core::ui::Color;
    use crate::core::ui::immediate::UiCommand;
    let mut d = UiDriver::new((300.0, 200.0));
    let draw = |ui: &mut UiContext<'_>| {
      ui.modal().show(|ui| {
        ui.button("Close").show();
      })
    };
    d.frame(draw);
    let dialog = d.frame(draw).rect;
    let dim = d.commands.iter().find_map(|(_, c)| match c {
      UiCommand::Rect {
        rect, fill, layer, ..
      } if *layer == 1 => Some((*rect, *fill)),
      _ => None,
    });
    assert_eq!(
      dim,
      Some((Rect::new(0.0, 0.0, 300.0, 200.0), Color::new(0, 0, 0, 160)))
    );
    let (cx, cy) = dialog.center();
    assert!(
      (cx - 150.0).abs() < 1.0 && (cy - 100.0).abs() < 1.0,
      "centred: {dialog:?}"
    );
  }

  #[test]
  fn a_modal_does_not_want_the_keyboard_but_a_popup_does() {
    let mut d = UiDriver::new((300.0, 200.0));
    let modal = |ui: &mut UiContext<'_>| {
      ui.modal().show(|ui| {
        ui.button("Close").show();
      });
      ui.wants_keyboard()
    };
    for _ in 0..3 {
      assert!(!d.frame(modal));
    }
    let mut d = UiDriver::new((300.0, 200.0));
    let popup = |ui: &mut UiContext<'_>| {
      let anchor = ui
        .button("More")
        .at(Rect::new(10.0, 10.0, 80.0, 24.0))
        .show();
      ui.popup(anchor.id).show(|ui| {
        ui.button("Rename").show();
      });
      ui.wants_keyboard()
    };
    for _ in 0..3 {
      assert!(d.frame(popup));
    }
  }
}
