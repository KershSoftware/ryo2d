//! The rebinding row: captures the next key or pad button for an action

use std::panic::Location;

use winit::keyboard::KeyCode;

use super::common::{WidgetOpts, widget_builders};
use crate::core::input::Binding;
use crate::core::text::{RichText, TextLayoutOptions};
use crate::core::ui::immediate::{PendingRebind, UiContext, text_seed};
use crate::core::ui::layout::{Align, Edges, Flow, Measure, UiStyle};
use crate::core::ui::memory::{Flags, UiMemory, UiSound};
use crate::core::ui::response::{Response, Sense};
use crate::core::ui::theme::{WidgetKind, WidgetState, warn_once};
use crate::core::ui::tree::{Node, TextLeaf};

/// A capture with no press cancels after this long (seconds)
const CAPTURE_TIMEOUT: f64 = 5.0;
/// A swap's note shows under the row this long (seconds)
const NOTE_SECONDS: f64 = 2.0;
/// Room between the row and its note
const NOTE_GAP: f32 = 2.0;

/// What a rebinding row remembers: whether it waits for a press (since
/// when), and the note a swap left (with when)
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct RebindState {
  pub(crate) capturing: bool,
  pub(crate) since: f64,
  pub(crate) note: Option<(String, f64)>,
}

/// The engine bound `rebind` and it swapped with the action `other`: its
/// row says so for a moment
pub(crate) fn note_swap(memory: &mut UiMemory, rebind: &PendingRebind, other: &str, now: f64) {
  if let Some(state) = memory.state::<RebindState>(rebind.id, rebind.scene) {
    state.note = Some((format!("Swapped with {other}"), now));
  }
}

/// A rebinding row being set up. It is drawn by [`show`](Self::show) or when
/// it is dropped.
pub struct Rebind<'u, 'a> {
  ui: Option<&'u mut UiContext<'a>>,
  site: &'static Location<'static>,
  action: String,
  label: Option<RichText>,
  slot: usize,
  opts: WidgetOpts,
}

impl<'a> UiContext<'a> {
  /// A row that rebinds `action`: its label on the left and its bindings as
  /// prompt chips on the right. A click, or Confirm while it has the focus,
  /// makes it wait for the next key or pad button (every one, Back's
  /// included), which replaces the binding in its [`slot`](Rebind::slot)
  /// once the UI pass ends; the [`Response`] reports `changed()` that
  /// frame. Escape, five seconds with no press, a click or the focus moving
  /// away cancels. A binding another action of its group (`ui.*`, or the
  /// game's) has swaps with it, and the row says so for a moment. Mouse
  /// buttons are not captured.
  ///
  /// ```ignore
  /// ui.column().focus_scope().show(|ui| {
  ///     ui.rebind("jump").label("Jump").fill_width();
  ///     ui.rebind("jump").label("Jump (pad)").slot(1).fill_width();
  /// });
  /// ```
  #[track_caller]
  pub fn rebind<'u>(&'u mut self, action: &str) -> Rebind<'u, 'a> {
    Rebind {
      ui: Some(self),
      site: Location::caller(),
      action: action.to_string(),
      label: None,
      slot: 0,
      opts: WidgetOpts::default(),
    }
  }
}

widget_builders!(Rebind<'_, '_>);

impl Rebind<'_, '_> {
  /// The row's label (default: the action's name)
  pub fn label(mut self, label: impl Into<RichText>) -> Self {
    self.label = Some(label.into());
    self
  }

  /// Which of the action's bindings a capture replaces (default 0, the
  /// first); past the last, it adds one
  pub fn slot(mut self, slot: usize) -> Self {
    self.slot = slot;
    self
  }

  /// Draw the row; `changed()` the frame it captured a press
  pub fn show(mut self) -> Response {
    self.run().unwrap_or_default()
  }

  /// Record the row, once (`show` and `Drop` both come here)
  fn run(&mut self) -> Option<Response> {
    let ui = self.ui.take()?;
    let site = self.site;
    let id = ui.memory.scopes.make(site, self.opts.key);
    let disabled = self.opts.disabled;
    let mut response = ui.widget_response(id, Sense::click(), &self.opts);
    let (scene, top) = ui.scene();
    let now = ui.memory.interact.now;
    // An action the game never bound has nothing to rebind
    let known = ui.prompts().bindings.contains_key(&self.action);
    if !known {
      let action = &self.action;
      warn_once(format!("rebind:{action}"), || {
        format!("no action named {action:?} to rebind; its row shows \"?\" and captures nothing")
      });
    }
    let mut state = ui
      .memory
      .state::<RebindState>(id, scene)
      .map(|state| state.clone())
      .unwrap_or_default();

    // A capture that started on an earlier frame takes this frame's press
    // (never the click or Confirm that started it)
    if state.capturing {
      let cancelled = disabled
        || !known
        || !top
        || response.lost_focus()
        || response.clicked()
        || now - state.since >= CAPTURE_TIMEOUT;
      if cancelled {
        state.capturing = false;
        // A row that cannot capture any more is no one's cancel
        if !disabled && known && top {
          ui.play(UiSound::Error);
        }
      } else if ui.input().key_pressed(KeyCode::Escape) {
        ui.consume_key(KeyCode::Escape);
        state.capturing = false;
        ui.play(UiSound::Error);
      } else if let Some(binding) = captured_press(ui) {
        match binding {
          Binding::Key(key) => ui.consume_key(key),
          Binding::Pad(button) => ui.consume_gamepad(button),
          _ => {}
        }
        ui.pending_rebinds.push(PendingRebind {
          id,
          scene,
          action: self.action.clone(),
          slot: self.slot,
          binding,
        });
        state.capturing = false;
        response.mark_changed();
      }
    } else if response.clicked() && !disabled && known {
      state.capturing = true;
      state.since = now;
    }
    if state
      .note
      .as_ref()
      .is_some_and(|(_, at)| now - at >= NOTE_SECONDS || now < *at)
    {
      state.note = None;
    }
    if let Some(slot) = ui.memory.state::<RebindState>(id, scene) {
      *slot = state.clone();
    }

    let widget_state = if state.capturing && !disabled {
      WidgetState::Pressed
    } else {
      ui.widget_state(&response, disabled)
    };
    let look = ui.look_of(WidgetKind::Rebind, &self.opts, widget_state);
    let theme = ui.theme_arc();
    let sizes = &theme.metrics.sizes;
    let pad = if look.padding == Edges::ZERO {
      Edges::xy(sizes.button_padding[0], sizes.button_padding[1])
    } else {
      look.padding
    };
    let opacity = f32::from(look.text.a) / 255.0;

    // A column: the row, then the note under it
    ui.memory.scopes.push(id);
    let outer = ui.memory.scopes.make(site, None);
    let outer_style = UiStyle {
      flow: Flow::Column,
      gap: NOTE_GAP,
      ..self.opts.style.clone()
    };
    let outer = ui.push_node(Node::new(outer, outer_style, Measure::None), self.opts.at);

    let mut flags = Flags::CLICK;
    if state.capturing {
      flags |= Flags::CAPTURING | Flags::USES_BACK;
    }
    let row_style = UiStyle {
      flow: Flow::Row,
      align: Align::Center,
      gap: theme.metrics.spacing,
      padding: pad,
      min_height: Some(sizes.button_min_height),
      ..UiStyle::default()
    };
    let mut row = Node::new(id, row_style, Measure::None);
    row.record = Some(self.opts.record(flags, true));
    let row = ui.push_node(row, None);
    ui.push_look_box(row, &look);

    // The label takes the room the chips leave
    let label_box = ui.memory.scopes.make(site, None);
    let grow = UiStyle {
      grow: 1.0,
      ..UiStyle::default()
    };
    let label_box = ui.push_node(Node::new(label_box, grow, Measure::None), None);
    let label = self
      .label
      .take()
      .unwrap_or_else(|| RichText::new(self.action.as_str()));
    let body = theme.text.body.style().color(look.text.with_alpha(255));
    push_text(ui, site, label, body, opacity);
    ui.close_node(label_box);

    if state.capturing {
      let caption = theme.text.caption.style().color(look.text.with_alpha(255));
      let asking = RichText::new("Press a key or button…");
      push_text(ui, site, asking, caption, opacity);
    } else {
      let bindings = ui
        .prompts()
        .bindings
        .get(&self.action)
        .cloned()
        .unwrap_or_default();
      let chip = WidgetOpts {
        disabled,
        ..WidgetOpts::default()
      };
      if bindings.is_empty() {
        let id = ui.memory.scopes.make(site, None);
        ui.prompt_chip(id, &chip, None);
      }
      for binding in bindings {
        let id = ui.memory.scopes.make(site, None);
        ui.prompt_chip(id, &chip, Some(binding));
      }
    }
    if response.focused() && ui.focus_visible() {
      ui.focus_ring(row, look.radius);
    }
    response.rect = ui.close_node(row).rect;

    if let Some((note, _)) = state.note {
      let caption = theme.text.caption.style();
      push_text(ui, site, RichText::new(note), caption, 1.0);
    }
    ui.memory.scopes.pop();
    ui.close_node(outer);
    ui.tooltip_tick(self.opts.tooltip.as_ref(), &response);
    Some(response)
  }
}

impl Drop for Rebind<'_, '_> {
  /// A row not shown is drawn when it is dropped
  fn drop(&mut self) {
    self.run();
  }
}

/// This frame's press for a capture: a key (Escape aside) or a pad button
/// that survives a save; mouse buttons are not captured
fn captured_press(ui: &UiContext<'_>) -> Option<Binding> {
  let input = ui.input();
  let key = input
    .key_pressed_any()
    .map(Binding::Key)
    .filter(Binding::is_saveable);
  key.or_else(|| {
    input
      .gamepad_pressed_any()
      .map(Binding::Pad)
      .filter(Binding::is_saveable)
  })
}

/// Record `text` as a leaf in `style`, faded by `opacity`
fn push_text(
  ui: &mut UiContext<'_>,
  site: &'static Location<'static>,
  text: RichText,
  style: crate::core::text::TextStyle,
  opacity: f32,
) {
  let options = TextLayoutOptions::default();
  let seed = text_seed(&text, &style, &options);
  let leaf = TextLeaf {
    text,
    style,
    options,
  };
  ui.text_leaf(site, leaf, (opacity, None, seed), glam::Vec2::ZERO);
}

#[cfg(all(test, feature = "default-fonts"))]
mod tests {
  use crate::core::input::{Binding, Direction, GamepadButton, InputDevice};
  use crate::core::ui::UiContext;
  use crate::core::ui::test_util::{UiDriver, prompts_with};
  use winit::keyboard::KeyCode;

  fn row(ui: &mut UiContext<'_>) -> bool {
    let mut changed = false;
    ui.column().focus_scope().show(|ui| {
      changed = ui
        .rebind("jump")
        .label("Jump")
        .default_focus()
        .show()
        .changed()
    });
    changed
  }

  fn driver() -> UiDriver {
    UiDriver::new((300.0, 100.0)).prompts(prompts_with(
      InputDevice::Keyboard,
      &[("jump", vec![Binding::Key(KeyCode::Space)])],
    ))
  }

  #[test]
  fn rebinding_captures_the_next_key() {
    let mut d = driver();
    for _ in 0..2 {
      d.frame(row);
    }
    d.confirm();
    d.frame(row);
    d.key(KeyCode::KeyJ);
    assert!(d.frame(row));
    assert_eq!(
      d.pending_rebinds(),
      vec![("jump".to_string(), 0, Binding::Key(KeyCode::KeyJ))]
    );
  }

  #[test]
  fn arrows_and_the_east_button_are_captured_not_navigated() {
    let mut d = driver();
    for _ in 0..2 {
      d.frame(row);
    }
    d.confirm();
    d.frame(row);
    d.nav(Direction::Down);
    d.key(KeyCode::ArrowDown);
    d.frame(row);
    assert_eq!(
      d.pending_rebinds().last().map(|r| r.2),
      Some(Binding::Key(KeyCode::ArrowDown))
    );
    d.confirm();
    d.frame(row);
    d.pad(GamepadButton::East);
    d.back();
    d.frame(row);
    assert_eq!(
      d.pending_rebinds().last().map(|r| r.2),
      Some(Binding::Pad(GamepadButton::East))
    );
  }

  #[test]
  fn escape_cancels_and_a_capture_times_out() {
    let mut d = driver();
    for _ in 0..2 {
      d.frame(row);
    }
    d.confirm();
    d.frame(row);
    d.key(KeyCode::Escape);
    assert!(!d.frame(row));
    assert!(d.pending_rebinds().is_empty());
    assert!(
      d.last_sounds()
        .contains(&crate::core::ui::memory::UiSound::Error),
      "a cancelled capture sounds an error"
    );
    d.confirm();
    d.frame(row);
    for _ in 0..(5 * 60 + 2) {
      d.frame(row);
    }
    d.key(KeyCode::KeyK);
    d.frame(row);
    assert!(d.pending_rebinds().is_empty(), "timed out before K");
  }

  /// The last frame's texts
  fn texts(d: &UiDriver) -> Vec<String> {
    use crate::core::ui::immediate::UiCommand;
    d.commands
      .iter()
      .filter_map(|(_, c)| match c {
        UiCommand::Text { layout, .. } => Some(layout.text().to_string()),
        _ => None,
      })
      .collect()
  }

  #[test]
  fn navigation_stands_aside_while_a_row_captures() {
    let mut d = driver();
    // The row and a button under it; whether the scene got Back
    let page = |ui: &mut UiContext<'_>| {
      let mut below = crate::core::ui::Rect::default();
      ui.column().focus_scope().show(|ui| {
        ui.rebind("jump").default_focus().show();
        below = ui.button("Below").show().rect;
      });
      (ui.back(), below)
    };
    for _ in 0..2 {
      d.frame(page);
    }
    d.confirm();
    let (_, below) = d.frame(page);
    let row = d.memory.interact.focused;
    d.nav(Direction::Down);
    d.confirm();
    d.back();
    d.tab(false);
    d.tab_next();
    d.move_to(below.x + 4.0, below.y + 4.0);
    let (back, _) = d.frame(page);
    assert!(!back, "Back is the row's, not the scene's");
    assert!(d.last_outcome().used.is_empty(), "{:?}", d.last_outcome());
    assert_eq!(d.memory.interact.tab_step, 0);
    assert_eq!(
      d.memory.interact.focused, row,
      "the focus stays, the pointer passing over the button too"
    );
    assert!(d.pending_rebinds().is_empty(), "no key was pressed");
    assert!(texts(&d).contains(&"Press a key or button…".to_string()));
  }

  #[test]
  fn the_press_that_starts_a_capture_is_not_captured() {
    let mut d = driver();
    for _ in 0..2 {
      d.frame(row);
    }
    // Confirm's own key is in the frame that starts the capture
    d.confirm();
    d.key(KeyCode::Enter);
    assert!(!d.frame(row));
    assert!(d.pending_rebinds().is_empty());
    d.key(KeyCode::KeyJ);
    assert!(d.frame(row));
  }

  #[test]
  fn a_row_shows_its_label_and_bindings_then_asks_for_a_press() {
    let mut d = driver();
    for _ in 0..2 {
      d.frame(row);
    }
    let shown = texts(&d);
    assert!(shown.contains(&"Jump".to_string()), "{shown:?}");
    assert!(shown.contains(&"Space".to_string()), "{shown:?}");
    d.confirm();
    d.frame(row);
    let shown = texts(&d);
    assert!(shown.contains(&"Press a key or button…".to_string()));
    assert!(!shown.contains(&"Space".to_string()), "{shown:?}");
  }

  #[test]
  fn a_click_starts_a_capture_and_another_cancels_it() {
    let mut d = driver();
    let click_row = |ui: &mut UiContext<'_>| ui.rebind("jump").show().rect;
    let rect = d.frame(click_row);
    d.move_to(rect.x + 4.0, rect.y + 4.0);
    d.press();
    d.frame(click_row);
    d.release();
    d.frame(click_row);
    assert!(texts(&d).contains(&"Press a key or button…".to_string()));
    d.press();
    d.frame(click_row);
    d.release();
    d.frame(click_row);
    assert!(texts(&d).contains(&"Space".to_string()), "cancelled");
  }

  #[test]
  fn a_capturing_row_wants_the_keyboard() {
    let mut d = driver();
    let wants = |ui: &mut UiContext<'_>| {
      let before = ui.wants_keyboard();
      row(ui);
      (before, ui.wants_keyboard())
    };
    for _ in 0..2 {
      assert_eq!(d.frame(wants), (false, false));
    }
    d.confirm();
    assert_eq!(d.frame(wants), (false, true), "from the frame it starts");
    assert_eq!(d.frame(wants), (true, true));
    d.key(KeyCode::KeyJ);
    assert_eq!(d.frame(wants), (true, false), "until it captures");
  }

  #[test]
  fn a_swap_is_noted_under_the_row_for_two_seconds() {
    let mut d = driver();
    for _ in 0..2 {
      d.frame(row);
    }
    d.confirm();
    d.frame(row);
    d.key(KeyCode::KeyJ);
    d.frame(row);
    let rebind = d.rebinds()[0].clone();
    let now = d.memory.interact.now;
    super::note_swap(&mut d.memory, &rebind, "fire", now);
    d.frame(row);
    assert!(texts(&d).contains(&"Swapped with fire".to_string()));
    for _ in 0..121 {
      d.frame(row);
    }
    assert!(!texts(&d).contains(&"Swapped with fire".to_string()));
  }

  #[test]
  fn a_rebind_row_for_an_unknown_action_warns_once_and_never_captures() {
    let mut d = driver();
    let unknown = |ui: &mut UiContext<'_>| {
      let mut changed = false;
      ui.column().focus_scope().show(|ui| {
        changed = ui
          .rebind("t14_unknown_rebind_action")
          .default_focus()
          .show()
          .changed()
      });
      changed
    };
    for _ in 0..2 {
      d.frame(unknown);
    }
    d.confirm();
    d.frame(unknown);
    assert!(!texts(&d).contains(&"Press a key or button…".to_string()));
    d.key(KeyCode::KeyJ);
    assert!(!d.frame(unknown));
    assert!(d.pending_rebinds().is_empty());
    let warnings = crate::core::dev_log::get_entries()
      .iter()
      .filter(|e| e.message.contains("t14_unknown_rebind_action"))
      .count();
    assert_eq!(warnings, 1);
  }
}
