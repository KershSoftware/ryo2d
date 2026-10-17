//! The choice list: a column of choices (dialogue answers, shop options),
//! picked by a click, Confirm or a number key

use std::panic::Location;

use winit::keyboard::KeyCode;

use super::common::{WidgetOpts, widget_builders};
use crate::core::ui::immediate::UiContext;
use crate::core::ui::layout::{Flow, UiStyle};
use crate::core::ui::memory::Flags;
use crate::core::ui::theme::WidgetKind;

/// Room between choices
const GAP: f32 = 2.0;
/// The number keys that pick the first nine choices: the row's, then the
/// keypad's
const NUMBER_KEYS: [[KeyCode; 2]; 9] = [
  [KeyCode::Digit1, KeyCode::Numpad1],
  [KeyCode::Digit2, KeyCode::Numpad2],
  [KeyCode::Digit3, KeyCode::Numpad3],
  [KeyCode::Digit4, KeyCode::Numpad4],
  [KeyCode::Digit5, KeyCode::Numpad5],
  [KeyCode::Digit6, KeyCode::Numpad6],
  [KeyCode::Digit7, KeyCode::Numpad7],
  [KeyCode::Digit8, KeyCode::Numpad8],
  [KeyCode::Digit9, KeyCode::Numpad9],
];

/// A choice list being set up. It is drawn by [`show`](Self::show) (the
/// choice picked this frame) or when it is dropped.
pub struct ChoiceList<'u, 'a> {
  ui: Option<&'u mut UiContext<'a>>,
  site: &'static Location<'static>,
  options: &'u [&'u str],
  /// The choice focused first
  first: usize,
  opts: WidgetOpts,
}

impl<'a> UiContext<'a> {
  /// A column of choices, a focus scope whose first choice (or
  /// [`default`](ChoiceList::default)'s) takes the focus when it appears.
  /// `show()` gives the choice picked this frame: by a click, Confirm, or in
  /// the top scene the number keys 1 to 9 (row or keypad), which pick a
  /// choice that exists and are then taken from gameplay.
  ///
  /// ```ignore
  /// if let Some(choice) = ui.choice_list(&["Buy", "Sell", "Leave"]).show() {
  ///     self.answer(choice);
  /// }
  /// ```
  #[track_caller]
  pub fn choice_list<'u>(&'u mut self, options: &'u [&'u str]) -> ChoiceList<'u, 'a> {
    ChoiceList {
      ui: Some(self),
      site: Location::caller(),
      options,
      first: 0,
      opts: WidgetOpts::default(),
    }
  }
}

widget_builders!(ChoiceList<'_, '_>);

impl ChoiceList<'_, '_> {
  /// Focus choice `index` first when the list appears (the first choice
  /// when there is no such choice)
  pub fn default(mut self, index: usize) -> Self {
    self.first = index;
    self
  }

  /// Draw the list; the choice picked this frame
  pub fn show(mut self) -> Option<usize> {
    self.run().flatten()
  }

  /// Record the list, once (`show` and `Drop` both come here); the choice
  /// picked
  fn run(&mut self) -> Option<Option<usize>> {
    let ui = self.ui.take()?;
    let id = ui.memory.scopes.make(self.site, self.opts.key);
    let (_, top) = ui.scene();
    let disabled = self.opts.disabled;
    let mut picked = None;
    // Number keys, unless a field or an open list has the keyboard
    if top && !disabled && !ui.wants_keyboard() {
      for (n, keys) in NUMBER_KEYS.iter().enumerate().take(self.options.len()) {
        for &key in keys {
          if ui.input().key_pressed(key) {
            ui.consume_key(key);
            picked.get_or_insert(n);
          }
        }
      }
    }

    let style = UiStyle {
      flow: Flow::Column,
      gap: GAP,
      ..self.opts.style.clone()
    };
    let node = ui.open_node(id, style, self.opts.at, Flags::FOCUS_SCOPE);
    ui.memory.scopes.push(id);
    let first = if self.first < self.options.len() {
      self.first
    } else {
      0
    };
    for (i, option) in self.options.iter().enumerate() {
      let mut choice = ui
        .button(*option)
        .id(i)
        .kind(WidgetKind::Choice)
        .label_left()
        .fill_width()
        .enabled(!disabled);
      if i == first {
        choice = choice.default_focus();
      }
      if let Some(patch) = &self.opts.look_patch {
        choice = choice.style(patch.clone());
      }
      if let Some(tip) = &self.opts.tooltip {
        choice = choice.tooltip(tip.clone());
      }
      if choice.show().clicked() {
        picked.get_or_insert(i);
      }
    }
    ui.memory.scopes.pop();
    ui.close_node(node);
    Some(picked)
  }
}

impl Drop for ChoiceList<'_, '_> {
  /// A choice list not shown is drawn when it is dropped
  fn drop(&mut self) {
    self.run();
  }
}

#[cfg(all(test, feature = "default-fonts"))]
mod tests {
  use crate::core::input::Direction;
  use crate::core::ui::test_util::UiDriver;
  use crate::core::ui::{Rect, UiContext};
  use winit::keyboard::KeyCode;

  fn choices(ui: &mut UiContext<'_>) -> Option<usize> {
    ui.choice_list(&["Buy", "Sell", "Leave"])
      .at(Rect::new(0.0, 0.0, 200.0, 90.0))
      .show()
  }

  #[test]
  fn number_keys_pick_a_choice() {
    let mut d = UiDriver::new((300.0, 200.0));
    for _ in 0..2 {
      d.frame(choices);
    }
    d.key(KeyCode::Digit2);
    assert_eq!(d.frame(choices), Some(1));
    d.key(KeyCode::Digit9);
    assert_eq!(d.frame(choices), None, "there is no ninth choice");
  }

  #[test]
  fn confirm_picks_the_focused_choice() {
    let mut d = UiDriver::new((300.0, 200.0));
    for _ in 0..2 {
      d.frame(choices);
    }
    d.nav(Direction::Down);
    d.frame(choices);
    d.confirm();
    assert_eq!(d.frame(choices), Some(1));
  }

  #[test]
  fn default_picks_the_first_focus_and_a_numpad_key_is_consumed() {
    let mut d = UiDriver::new((300.0, 200.0));
    let draw = |ui: &mut UiContext<'_>| {
      let picked = ui
        .choice_list(&["Buy", "Sell", "Leave"])
        .default(2)
        .at(Rect::new(0.0, 0.0, 200.0, 90.0))
        .show();
      (picked, ui.input().key_pressed(KeyCode::Numpad1))
    };
    for _ in 0..2 {
      d.frame(draw);
    }
    d.confirm();
    assert_eq!(d.frame(draw), (Some(2), false));
    d.key(KeyCode::Numpad1);
    assert_eq!(d.frame(draw), (Some(0), false), "picked and taken");
  }
}
