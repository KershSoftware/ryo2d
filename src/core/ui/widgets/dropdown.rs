//! The dropdown: a trigger showing the selected option, and a list of the
//! options in a popup while it is open

use std::panic::Location;

use glam::Vec2;

use super::common::{WidgetOpts, widget_builders};
use crate::core::text::{RichText, TextLayoutOptions, TextOverflow};
use crate::core::ui::immediate::{UiCommand, UiContext};
use crate::core::ui::layout::{Align, Edges, Flow, Justify, Length, Measure, UiStyle};
use crate::core::ui::memory::Flags;
use crate::core::ui::response::{Response, Sense};
use crate::core::ui::theme::{LookPatch, WidgetKind, WidgetState};
use crate::core::ui::tree::{DrawKind, Node};
use crate::core::ui::{Rect, UiId, popup};

/// Separates the list's id (the scope of its options) from the trigger's
const LIST_SALT: u64 = 0x6c69_7374;
/// The chevron's size
const CHEVRON: Vec2 = Vec2::new(10.0, 6.0);
/// Room between the list's edges and its options
const LIST_PADDING: f32 = 4.0;
/// The label of a dropdown with no options
const EMPTY: &str = "-";

/// Whether a dropdown's list is open, kept under its id
#[derive(Debug, Default)]
pub(crate) struct DropdownMemory {
  pub(crate) open: bool,
}

/// A dropdown being set up. It is drawn by [`show`](Self::show) or when it
/// is dropped.
pub struct Dropdown<'u, 'a> {
  ui: Option<&'u mut UiContext<'a>>,
  site: &'static Location<'static>,
  selected: &'u mut usize,
  options: &'u [&'u str],
  opts: WidgetOpts,
}

impl<'a> UiContext<'a> {
  /// A dropdown showing `options[*selected]`. A click or Confirm opens its
  /// list below it, on the popup layer, where focus moves to the selected
  /// option; picking one (a click or Confirm) sets `*selected` and closes
  /// the list, and the [`Response`] reports `changed()` when that was a
  /// change. Back, or a press anywhere else, closes the list without
  /// picking; Back used that way leaves [`back`](UiContext::back) false.
  /// `*selected` past the end is clamped to the last option; with no
  /// options it shows "-" and never opens.
  ///
  /// ```ignore
  /// ui.dropdown(&mut self.difficulty, &["Easy", "Normal", "Hard"]).show();
  /// ```
  #[track_caller]
  pub fn dropdown<'u>(
    &'u mut self,
    selected: &'u mut usize,
    options: &'u [&'u str],
  ) -> Dropdown<'u, 'a> {
    Dropdown {
      ui: Some(self),
      site: Location::caller(),
      selected,
      options,
      opts: WidgetOpts::default(),
    }
  }
}

widget_builders!(Dropdown<'_, '_>);

impl Dropdown<'_, '_> {
  /// Draw the dropdown (and its list, while open); what happened to it this
  /// frame
  pub fn show(mut self) -> Response {
    self.run().unwrap_or_default()
  }

  /// Record the dropdown, once (`show` and `Drop` both come here)
  fn run(&mut self) -> Option<Response> {
    let ui = self.ui.take()?;
    let id = ui.memory.scopes.make(self.site, self.opts.key);
    let list = id.derived(LIST_SALT);
    let (scene, top) = ui.scene();
    let disabled = self.opts.disabled;
    let count = self.options.len();
    if let Some(last) = count.checked_sub(1) {
      *self.selected = (*self.selected).min(last);
    }
    let before = *self.selected;
    let mut response = ui.widget_response(id, Sense::click(), &self.opts);

    let was_open = ui
      .memory
      .peek_state::<DropdownMemory>(id)
      .is_some_and(|state| state.open);
    let mut open = was_open && count > 0 && !disabled;
    if count > 0 && !disabled && response.clicked() {
      open = !open;
    }
    if open && top {
      // The trigger, the list and its options are the dropdown's
      let memory = &*ui.memory;
      let ours = |who: UiId| {
        who != UiId::NONE
          && (who == id || who == list || memory.prev(who).is_some_and(|r| r.scope == list))
      };
      let i = &memory.interact;
      if (i.back_used && ours(i.focused)) || (i.pressed && !ours(i.press_target)) {
        open = false;
      }
    }

    if open && let Some(picked) = self.list(ui, list, id) {
      *self.selected = picked;
      open = false;
    }
    if *self.selected != before {
      response.mark_changed();
    }
    if let Some(state) = ui.memory.state::<DropdownMemory>(id, scene) {
      state.open = open;
    }

    response.rect = self.trigger(ui, id, open, &response);
    ui.tooltip_tick(self.opts.tooltip.as_ref(), &response);
    Some(response)
  }

  /// Record the open list below the trigger `anchor`: a popup
  /// column in the dropdown's normal look (opaque in the built-in themes,
  /// so the page does not show through), a focus scope that traps, of one
  /// option per entry in a scroll area no taller than the room on the side
  /// it opens on, so a long list stays inside the UI and the wheel and
  /// navigation scroll it. The option picked this frame, if any.
  fn list(&self, ui: &mut UiContext<'_>, list: UiId, anchor: UiId) -> Option<usize> {
    let room = popup::room(ui.anchor_rect(anchor), ui.ui_size()) - 2.0 * LIST_PADDING;
    ui.begin_popup(anchor, false);
    let style = UiStyle {
      flow: Flow::Column,
      padding: Edges::all(LIST_PADDING),
      ..UiStyle::default()
    };
    let flags = Flags::FOCUS_SCOPE | Flags::TRAP | Flags::BLOCKING | Flags::POPUP;
    let node = ui.open_node(list, style, None, flags);
    let look = ui.look_of(WidgetKind::Dropdown, &self.opts, WidgetState::Normal);
    ui.push_look_box(node, &look);
    ui.memory.scopes.push(list);
    let current = ui.theme().palette.accent;
    let mut picked = None;
    ui.scroll()
      .fill_width()
      .max_height(room.max(0.0))
      .show(|ui| {
        for (i, option) in self.options.iter().enumerate() {
          let mut row = ui
            .button(*option)
            .id(i)
            .kind(WidgetKind::Choice)
            .also(Flags::USES_BACK)
            .label_left()
            .fill_width();
          if i == *self.selected {
            row = row
              .default_focus()
              .style(LookPatch::default().text(current));
          }
          if row.show().clicked() {
            picked = Some(i);
          }
        }
      });
    ui.memory.scopes.pop();
    ui.close_node(node);
    ui.end_popup();
    picked
  }

  /// Record the trigger: a row with the dropdown look, the selected
  /// option's label and a chevron (pointing up while `open`). Its rect
  /// (this frame's when it was laid out, else last frame's).
  fn trigger(&self, ui: &mut UiContext<'_>, id: UiId, open: bool, response: &Response) -> Rect {
    let disabled = self.opts.disabled;
    let state = ui.widget_state(response, disabled);
    let look = ui.look_of(WidgetKind::Dropdown, &self.opts, state);
    let theme = ui.theme_arc();
    let sizes = &theme.metrics.sizes;
    // Its look's own padding, else the theme's button padding
    let padding = if look.padding == Edges::ZERO {
      Edges::xy(sizes.button_padding[0], sizes.button_padding[1])
    } else {
      look.padding
    };
    let mut style = UiStyle {
      flow: Flow::Row,
      align: Align::Center,
      justify: Justify::SpaceBetween,
      gap: theme.metrics.spacing,
      padding,
      ..self.opts.style.clone()
    };
    // As tall as a button, unless placed or sized
    if self.opts.at.is_none() && style.height == Length::Fit {
      style.min_height = Some(sizes.button_min_height);
    }
    let mut record = self.opts.record(Flags::CLICK, true);
    if open {
      record.flags |= Flags::USES_BACK;
    }
    let mut row = Node::new(id, style, Measure::None);
    row.record = Some(record);
    let row = ui.push_node(row, self.opts.at);
    ui.push_look_box(row, &look);
    ui.memory.scopes.push(id);

    // The label, on one line, in the button role
    let label = self.options.get(*self.selected).copied().unwrap_or(EMPTY);
    let text = RichText::new(label);
    let text_style = theme.text.button.style().color(look.text.with_alpha(255));
    let options = TextLayoutOptions::default()
      .max_lines(1)
      .overflow(TextOverflow::Ellipsis);
    let leaf = ui.leaf_of(text, text_style, options);
    let opacity = f32::from(look.text.a) / 255.0;
    ui.text_leaf(self.site, leaf, (opacity, None), Vec2::ZERO);

    // The chevron: down while closed, up while open
    let chevron = ui.memory.scopes.make(self.site, None);
    let node = ui.push_node(
      Node::new(chevron, UiStyle::default(), Measure::Fixed(CHEVRON)),
      None,
    );
    let (tip, ends) = if open { (0.5, 5.5) } else { (5.5, 0.5) };
    ui.push_draw(
      node,
      DrawKind::Local(UiCommand::Line {
        points: [
          Vec2::new(0.0, ends),
          Vec2::new(CHEVRON.x / 2.0, tip),
          Vec2::new(CHEVRON.x, ends),
        ]
        .into(),
        width: 1.5,
        color: look.text,
        closed: false,
        clip: None,
        layer: 0,
      }),
    );
    ui.close_node(node);
    ui.memory.scopes.pop();

    if response.focused() && ui.focus_visible() {
      ui.focus_ring(row, look.radius);
    }
    ui.close_node(row).rect
  }
}

impl Drop for Dropdown<'_, '_> {
  /// A dropdown not shown is drawn when it is dropped
  fn drop(&mut self) {
    self.run();
  }
}

#[cfg(all(test, feature = "default-fonts"))]
mod tests {
  use crate::core::input::Direction;
  use crate::core::ui::test_util::UiDriver;
  use crate::core::ui::{Rect, UiContext};

  const OPTIONS: [&str; 3] = ["Easy", "Medium", "Hard"];
  const TRIGGER: Rect = Rect::new(10.0, 10.0, 120.0, 28.0);

  fn drop_down(ui: &mut UiContext<'_>, sel: &mut usize) -> bool {
    let mut changed = false;
    ui.column().focus_scope().show(|ui| {
      changed = ui
        .dropdown(sel, &OPTIONS)
        .at(TRIGGER)
        .default_focus()
        .show()
        .changed()
    });
    changed
  }

  fn click(d: &mut UiDriver, x: f32, y: f32, sel: &mut usize) -> bool {
    d.move_to(x, y);
    d.press();
    d.frame(|ui| drop_down(ui, sel));
    d.release();
    d.frame(|ui| drop_down(ui, sel))
  }

  #[test]
  fn a_dropdown_opens_picks_and_closes() {
    let mut d = UiDriver::new((300.0, 200.0));
    let mut sel = 0;
    d.frame(|ui| drop_down(ui, &mut sel));
    click(&mut d, 20.0, 20.0, &mut sel);
    d.frame(|ui| drop_down(ui, &mut sel));
    // The list opens below the trigger: options of about 28 each from y 40
    assert!(click(&mut d, 20.0, 40.0 + 28.0 * 2.0 + 10.0, &mut sel));
    assert_eq!(sel, 2);
    d.frame(|ui| drop_down(ui, &mut sel));
    assert!(
      !d.frame(|ui| {
        drop_down(ui, &mut sel);
        ui.wants_keyboard()
      }),
      "closed"
    );
  }

  #[test]
  fn confirm_opens_a_focused_dropdown_and_the_d_pad_picks() {
    let mut d = UiDriver::new((300.0, 200.0));
    let mut sel = 0;
    for _ in 0..2 {
      d.frame(|ui| drop_down(ui, &mut sel));
    }
    d.confirm();
    d.frame(|ui| drop_down(ui, &mut sel));
    d.frame(|ui| drop_down(ui, &mut sel));
    d.nav(Direction::Down);
    d.frame(|ui| drop_down(ui, &mut sel));
    d.confirm();
    d.frame(|ui| drop_down(ui, &mut sel));
    assert_eq!(sel, 1);
  }

  #[test]
  fn back_closes_an_open_dropdown_before_anything_else() {
    let mut d = UiDriver::new((300.0, 200.0));
    let mut sel = 1;
    d.frame(|ui| drop_down(ui, &mut sel));
    click(&mut d, 20.0, 20.0, &mut sel);
    d.frame(|ui| drop_down(ui, &mut sel));
    d.back();
    let back = d.frame(|ui| {
      drop_down(ui, &mut sel);
      ui.back()
    });
    assert!(!back, "the list used Back");
    assert_eq!(sel, 1);
    d.back();
    assert!(
      d.frame(|ui| {
        drop_down(ui, &mut sel);
        ui.back()
      }),
      "closed now: Back is free"
    );
  }

  #[test]
  fn the_open_list_draws_over_the_page() {
    let mut d = UiDriver::new((300.0, 200.0));
    let mut sel = 0;
    let draw = |ui: &mut UiContext<'_>, sel: &mut usize| {
      drop_down(ui, sel);
      ui.draw_rect(
        Rect::new(0.0, 0.0, 300.0, 200.0),
        crate::core::ui::Color::BLUE,
      );
    };
    d.frame(|ui| draw(ui, &mut sel));
    d.move_to(20.0, 20.0);
    d.press();
    d.frame(|ui| draw(ui, &mut sel));
    d.release();
    d.frame(|ui| draw(ui, &mut sel));
    d.frame(|ui| draw(ui, &mut sel));
    let layers: Vec<u8> = d.commands.iter().map(|(_, c)| c.draw_layer()).collect();
    assert!(layers.contains(&1), "the list is on the popup layer");
  }

  /// Whether the list is open: anything is drawn on the popup layer
  fn list_shown(d: &UiDriver) -> bool {
    d.commands.iter().any(|(_, c)| c.draw_layer() == 1)
  }

  #[test]
  fn a_press_outside_closes_the_list_and_one_inside_does_not() {
    let mut d = UiDriver::new((300.0, 200.0));
    let mut sel = 0;
    d.frame(|ui| drop_down(ui, &mut sel));
    click(&mut d, 20.0, 20.0, &mut sel);
    assert!(list_shown(&d), "open");
    // A press on an option, held: still open
    d.move_to(20.0, 50.0);
    d.press();
    d.frame(|ui| drop_down(ui, &mut sel));
    assert!(list_shown(&d), "a press on an option keeps it open");
    d.move_to(250.0, 20.0);
    d.release();
    d.frame(|ui| drop_down(ui, &mut sel));
    assert!(list_shown(&d), "released off the option: nothing picked");
    assert_eq!(sel, 0);
    // A press beside it
    d.press();
    d.frame(|ui| drop_down(ui, &mut sel));
    assert!(!list_shown(&d), "a press outside closes it");
    assert_eq!(sel, 0);
  }

  #[test]
  fn an_empty_dropdown_shows_a_dash_and_never_opens() {
    let mut d = UiDriver::new((300.0, 200.0));
    let mut sel = 3;
    let draw = |ui: &mut UiContext<'_>, sel: &mut usize| {
      ui.dropdown(sel, &[]).at(TRIGGER).show();
    };
    d.frame(|ui| draw(ui, &mut sel));
    d.move_to(20.0, 20.0);
    d.press();
    d.frame(|ui| draw(ui, &mut sel));
    d.release();
    d.frame(|ui| draw(ui, &mut sel));
    d.frame(|ui| draw(ui, &mut sel));
    assert!(!list_shown(&d));
    let texts: Vec<String> = d
      .commands
      .iter()
      .filter_map(|(_, c)| match c {
        crate::core::ui::immediate::UiCommand::Text { layout, .. } => {
          Some(layout.text().to_string())
        }
        _ => None,
      })
      .collect();
    assert_eq!(texts, vec!["-".to_string()]);
  }

  #[test]
  fn a_selection_out_of_range_is_clamped() {
    let mut d = UiDriver::new((300.0, 200.0));
    let mut sel = 9;
    d.frame(|ui| drop_down(ui, &mut sel));
    assert_eq!(sel, 2);
  }

  #[test]
  fn an_open_list_wants_the_keyboard_from_the_frame_it_opens() {
    let mut d = UiDriver::new((300.0, 200.0));
    let mut sel = 0;
    let draw = |ui: &mut UiContext<'_>, sel: &mut usize| {
      drop_down(ui, sel);
      ui.wants_keyboard()
    };
    assert!(!d.frame(|ui| draw(ui, &mut sel)));
    d.move_to(20.0, 20.0);
    d.press();
    assert!(!d.frame(|ui| draw(ui, &mut sel)));
    d.release();
    assert!(d.frame(|ui| draw(ui, &mut sel)), "opened this frame");
    assert!(d.frame(|ui| draw(ui, &mut sel)), "and still open");
  }

  #[test]
  fn back_closes_the_list_after_the_pointer_left_it() {
    let mut d = UiDriver::new((300.0, 200.0));
    let mut sel = 0;
    let draw = |ui: &mut UiContext<'_>, sel: &mut usize| {
      ui.column().focus_scope().show(|ui| {
        ui.dropdown(sel, &OPTIONS)
          .at(TRIGGER)
          .default_focus()
          .show();
        ui.button("Other")
          .at(Rect::new(180.0, 10.0, 80.0, 28.0))
          .show();
      });
      ui.back()
    };
    d.frame(|ui| draw(ui, &mut sel));
    d.move_to(20.0, 20.0);
    d.press();
    d.frame(|ui| draw(ui, &mut sel));
    d.release();
    d.frame(|ui| draw(ui, &mut sel));
    d.frame(|ui| draw(ui, &mut sel));
    // The pointer wanders onto a widget outside the list: focus stays in it
    d.move_to(200.0, 20.0);
    d.frame(|ui| draw(ui, &mut sel));
    d.back();
    assert!(!d.frame(|ui| draw(ui, &mut sel)), "the list used Back");
    assert!(!list_shown(&d), "and closed");
  }

  /// Whether `inner` lies inside `outer` (to a hair, for rounding)
  fn within(inner: Rect, outer: Rect) -> bool {
    inner.x >= outer.x - 0.5
      && inner.y >= outer.y - 0.5
      && inner.right() <= outer.right() + 0.5
      && inner.bottom() <= outer.bottom() + 0.5
  }

  #[test]
  fn a_long_list_stays_inside_the_ui_and_scrolls_to_its_last_option() {
    use crate::core::ui::memory::Flags;

    let labels: Vec<String> = (0..40).map(|i| format!("Option {i}")).collect();
    let options: Vec<&str> = labels.iter().map(String::as_str).collect();
    let ui_rect = Rect::new(0.0, 0.0, 320.0, 180.0);
    let mut d = UiDriver::new((320.0, 180.0));
    let mut sel = 0;
    // The trigger's id
    let draw = |d: &mut UiDriver, sel: &mut usize| {
      d.frame(|ui| {
        let mut trigger = crate::core::ui::UiId::NONE;
        ui.column().focus_scope().show(|ui| {
          trigger = ui
            .dropdown(sel, &options)
            .at(Rect::new(10.0, 60.0, 120.0, 28.0))
            .default_focus()
            .show()
            .id;
        });
        trigger
      })
    };
    for _ in 0..2 {
      draw(&mut d, &mut sel);
    }
    d.confirm();
    let mut trigger = draw(&mut d, &mut sel);
    for _ in 0..2 {
      trigger = draw(&mut d, &mut sel);
    }
    let list = trigger.derived(super::LIST_SALT);
    let Some(shown) = d.memory.drawn(list).map(|r| r.rect) else {
      panic!("the list opened");
    };
    assert!(
      within(shown, ui_rect),
      "the list {shown:?} stays inside the UI"
    );
    // The options, by order
    let mut rows: Vec<_> = d
      .memory
      .drawn_records()
      .filter(|(_, r)| r.scope == list && r.flags.contains(Flags::FOCUSABLE))
      .map(|(id, r)| (r.order, id))
      .collect();
    rows.sort_unstable();
    assert_eq!(rows.len(), 40);
    let last = rows[39].1;
    for _ in 0..39 {
      d.nav(Direction::Down);
      draw(&mut d, &mut sel);
    }
    for _ in 0..30 {
      draw(&mut d, &mut sel);
    }
    assert_eq!(d.focused(), Some(last), "Down reaches the last option");
    let Some(row) = d.memory.drawn(last).copied() else {
      panic!("the last option is drawn");
    };
    assert!(
      within(row.rect, ui_rect) && row.clip.is_some_and(|clip| within(row.rect, clip)),
      "the last option {:?} is scrolled into view (clip {:?})",
      row.rect,
      row.clip
    );
  }

  #[test]
  fn a_list_reopens_on_the_selected_option() {
    let mut d = UiDriver::new((300.0, 200.0));
    let mut sel = 1;
    for _ in 0..2 {
      d.frame(|ui| drop_down(ui, &mut sel));
    }
    d.confirm();
    d.frame(|ui| drop_down(ui, &mut sel));
    d.frame(|ui| drop_down(ui, &mut sel));
    // The mouse brushes option 2, so the list remembers it
    d.move_to(20.0, 40.0 + 4.0 + 28.0 * 2.0 + 10.0);
    d.frame(|ui| drop_down(ui, &mut sel));
    d.back();
    d.frame(|ui| drop_down(ui, &mut sel));
    d.frame(|ui| drop_down(ui, &mut sel));
    // Opened again, Confirm picks what has the focus
    d.confirm();
    d.frame(|ui| drop_down(ui, &mut sel));
    d.frame(|ui| drop_down(ui, &mut sel));
    d.confirm();
    d.frame(|ui| drop_down(ui, &mut sel));
    assert_eq!(sel, 1, "it reopened on the selected option");
  }
}
