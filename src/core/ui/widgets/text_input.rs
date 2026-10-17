//! The text field

use std::borrow::Cow;
use std::panic::Location;

use glam::Vec2;
use winit::keyboard::KeyCode;

use super::common::{WidgetOpts, widget_builders};
use super::text_edit::TextEdit;
use crate::core::text::{RichText, TextLayoutOptions, TextStyle};
use crate::core::ui::immediate::{UiCommand, UiContext, text_seed};
use crate::core::ui::layout::{Edges, Flow, Length, Measure, UiStyle};
use crate::core::ui::memory::Flags;
use crate::core::ui::response::{Response, Sense};
use crate::core::ui::theme::{WidgetKind, WidgetState};
use crate::core::ui::tree::{DrawKind, Node};
use crate::core::ui::{Color, Rect};

/// The caret's width in UI units
const CARET_WIDTH: f32 = 1.5;
/// A password field shows this once per character
const BULLET: char = '\u{2022}';
/// The selection's opacity over the text
const SELECTION_ALPHA: u8 = 100;

/// A text field being set up over the caller's `String`. It is drawn by
/// [`show`](Self::show) or when it is dropped.
pub struct TextInput<'u, 'a> {
  ui: Option<&'u mut UiContext<'a>>,
  site: &'static Location<'static>,
  text: &'u mut String,
  placeholder: String,
  max_len: Option<usize>,
  password: bool,
  opts: WidgetOpts,
}

impl<'a> UiContext<'a> {
  /// A one-line text field editing `text`, the theme's `text_input` size
  /// unless told. A click focuses it (outside any focus scope too), and so
  /// does navigation inside a scope; while it has the focus it takes every
  /// key: typing, the arrows, Home, End, Backspace, Delete, and with Ctrl
  /// (Cmd) A, C, X and V. Enter submits, Escape (B on a pad) leaves it and
  /// Tab moves on.
  ///
  /// ```ignore
  /// let name = ui.text_input(&mut self.name).placeholder("Name").max_len(16).show();
  /// if name.submitted() {
  ///     self.start(&self.name);
  /// }
  /// ```
  #[track_caller]
  pub fn text_input<'u>(&'u mut self, text: &'u mut String) -> TextInput<'u, 'a> {
    TextInput {
      ui: Some(self),
      site: Location::caller(),
      text,
      placeholder: String::new(),
      max_len: None,
      password: false,
      opts: WidgetOpts::default(),
    }
  }
}

widget_builders!(TextInput<'_, '_>);

impl TextInput<'_, '_> {
  /// Shown, dimmed, while the text is empty and the field is not focused
  pub fn placeholder(mut self, placeholder: impl Into<String>) -> Self {
    self.placeholder = placeholder.into();
    self
  }

  /// At most `chars` characters: typing or pasting past it is cut off
  pub fn max_len(mut self, chars: usize) -> Self {
    self.max_len = Some(chars);
    self
  }

  /// Show a bullet for each character instead of the text
  pub fn password(mut self) -> Self {
    self.password = true;
    self
  }

  /// Draw the field; what happened to it this frame: `changed()` when the
  /// text changed, `submitted()` on Enter, `focused()` while it has the
  /// keyboard
  pub fn show(mut self) -> Response {
    self.run().unwrap_or_default()
  }

  /// Record the field, once (`show` and `Drop` both come here)
  fn run(&mut self) -> Option<Response> {
    let ui = self.ui.take()?;
    let id = ui.memory.scopes.make(self.site, self.opts.key);
    let disabled = self.opts.disabled;
    let mut response = ui.widget_response(id, Sense::click_and_drag(), &self.opts);
    let (scene, _) = ui.scene();
    let theme = ui.theme_arc();
    let sizes = &theme.metrics.sizes;

    let mut edit = ui
      .memory
      .state::<TextEdit>(id, scene)
      .map(std::mem::take)
      .unwrap_or_default();
    edit.clamp_to(self.text);

    // The size it draws at: its own, its `at` rect's, or, when it fills,
    // where layout put it last frame
    let mut style = self.opts.style.clone();
    style.flow = Flow::Column;
    let last = ui.memory.prev(id).map(|record| record.rect);
    let fills = style.grow > 0.0;
    let along = |length: Length, own: f32, last: Option<f32>| match length {
      Length::Fit if !fills => own,
      Length::Px(v) if !fills && v.is_finite() && v >= 0.0 => v,
      _ => last.unwrap_or(own),
    };
    let size = match self.opts.at {
      Some(at) => Vec2::new(at.width, at.height),
      None => Vec2::new(
        along(style.width, sizes.text_input[0], last.map(|r| r.width)),
        along(style.height, sizes.text_input[1], last.map(|r| r.height)),
      ),
    };
    let finite = |v: f32| if v.is_finite() { v.max(0.0) } else { 0.0 };
    let size = Vec2::new(finite(size.x), finite(size.y));
    if style.width == Length::Fit {
      style.width = Length::Px(sizes.text_input[0]);
    }
    if style.height == Length::Fit {
      style.height = Length::Px(sizes.text_input[1]);
    }

    let focused = response.focused() && !disabled;
    let state = if disabled {
      WidgetState::Disabled
    } else if focused {
      WidgetState::Focused
    } else {
      ui.widget_state(&response, false)
    };
    let look = ui.look_of(WidgetKind::TextInput, &self.opts, state);
    let pad = clean_edges(look.padding);
    // Laid out opaque and faded through its opacity, so every look shares
    // one cached layout
    let text_style = theme.text.body.style().color(look.text.with_alpha(255));
    let opacity = f32::from(look.text.a) / 255.0;

    if focused {
      let input = ui.input();
      let shift = input.key_down(KeyCode::ShiftLeft) || input.key_down(KeyCode::ShiftRight);
      let pressed_now = response.pressed() && ui.memory.interact.pressed;
      let left_button = ui.memory.interact.active_button == 0;
      if response.gained_focus() && !pressed_now {
        // Reached by navigation or a request: type at the end
        edit.set_cursor(self.text, self.text.len(), false);
      }
      // A press puts the caret under the pointer; held, it selects
      if response.pressed()
        && left_button
        && let (Some(pointer), Some(rect)) = (response.pointer, last)
      {
        let shown = shown_text(self.text, self.password);
        let x = pointer.x - (rect.x + pad.left) + edit.scroll();
        let at = ui.byte_at(&shown, &text_style, x);
        let select = !pressed_now || shift;
        if self.password {
          // A bullet per character: the character index is the same
          edit.set_cursor_char(self.text, at / BULLET.len_utf8(), select);
        } else {
          edit.set_cursor(self.text, at, select);
        }
      }
      edit.tick(ui.dt());
      // Escape, or Back from a pad (the field uses it: `USES_BACK`)
      if ui.input().key_pressed(KeyCode::Escape) || ui.memory.interact.back_used {
        // Leaving the field: Back is its, not the scene's
        ui.release_focus(id);
        response.mark_lost_focus();
      } else {
        let out = edit.handle_input(self.text, ui.input(), self.max_len);
        if out.changed {
          response.mark_changed();
        }
        if out.submitted {
          response.mark_submitted();
        }
      }
      // A focused field owns the keyboard: nothing typed is gameplay's
      ui.consume_keyboard();
    }
    let focused = response.focused();

    let mut flags = Flags::CLICK | Flags::DRAG | Flags::TEXT_FIELD | Flags::TAKES_HORIZONTAL;
    if focused {
      flags |= Flags::USES_BACK;
    }
    let mut node = Node::new(
      id,
      UiStyle {
        padding: pad,
        ..style
      },
      Measure::None,
    );
    node.record = Some(self.opts.record(flags, true));
    let field = ui.push_node(node, self.opts.at);
    ui.push_look_box(field, &look);

    // The text's view: the field's content box, clipping what is drawn in it
    let fill = UiStyle {
      width: Length::Fill,
      height: Length::Fill,
      ..UiStyle::default()
    };
    let mut view = Node::new(id.derived(1), fill.clone(), Measure::None);
    view.clips = true;
    let view = ui.push_node(view, None);
    let line = ui.push_node(Node::new(id.derived(2), fill, Measure::None), None);

    let inner = Vec2::new(
      (size.x - pad.width()).max(0.0),
      (size.y - pad.height()).max(0.0),
    );
    let shown = shown_text(self.text, self.password);
    let preedit = if focused {
      ui.input().ime_preedit().to_owned()
    } else {
      String::new()
    };
    let preedit_width = if preedit.is_empty() {
      0.0
    } else {
      ui.measure_text(preedit.as_str(), &text_style).x
    };
    let caret_at = shown_byte(self.text, self.password, edit.cursor());
    let caret_x = ui.caret_x(&shown, &text_style, caret_at);
    edit.ensure_caret_visible(caret_x + preedit_width, inner.x);
    let scroll = edit.scroll();
    let line_height = ui.line_height(&text_style);
    let y = (inner.y - line_height) / 2.0;
    let rect = |x: f32, y: f32, w: f32, h: f32, fill: Color| {
      DrawKind::Local(UiCommand::Rect {
        rect: Rect::new(x, y, w, h),
        fill,
        radius: 0.0,
        border: None,
        clip: None,
        layer: 0,
      })
    };

    if let Some(range) = edit.selection().filter(|_| focused) {
      let range = shown_byte(self.text, self.password, range.start)
        ..shown_byte(self.text, self.password, range.end);
      let (left, right) = ui.selection_x(&shown, &text_style, range);
      let color = theme.palette.accent.with_alpha(SELECTION_ALPHA);
      ui.push_draw(
        line,
        rect(left - scroll, y, right - left, line_height, color),
      );
    }
    if shown.is_empty() && !focused {
      if !self.placeholder.is_empty() {
        let muted = text_style.clone().color(theme.palette.muted);
        let placeholder = std::mem::take(&mut self.placeholder);
        push_text(ui, line, placeholder, &muted, Vec2::new(0.0, y), opacity);
      }
    } else if !shown.is_empty() {
      push_text(
        ui,
        line,
        shown.into_owned(),
        &text_style,
        Vec2::new(-scroll, y),
        opacity,
      );
    }
    if focused {
      let at = caret_x - scroll;
      if !preedit.is_empty() {
        // The composition, underlined at the caret, before it is committed
        push_text(ui, line, preedit, &text_style, Vec2::new(at, y), opacity);
        let underline = rect(at, y + line_height - 1.0, preedit_width, 1.0, look.text);
        ui.push_draw(line, underline);
      }
      if edit.caret_shown() && edit.selection().is_none() {
        let caret = rect(at + preedit_width, y, CARET_WIDTH, line_height, look.text);
        ui.push_draw(line, caret);
      }
    }

    ui.close_node(line);
    ui.close_node(view);
    if focused && ui.focus_visible() {
      ui.focus_ring(field, look.radius);
    }
    response.rect = ui.close_node(field).rect;
    if let Some(slot) = ui.memory.state::<TextEdit>(id, scene) {
      *slot = edit;
    }
    ui.tooltip_tick(self.opts.tooltip.as_ref(), &response);
    Some(response)
  }
}

impl Drop for TextInput<'_, '_> {
  /// A field not shown is drawn when it is dropped
  fn drop(&mut self) {
    self.run();
  }
}

/// What the field shows of `text`: itself, or a bullet per character
fn shown_text(text: &str, password: bool) -> Cow<'_, str> {
  if password {
    Cow::Owned(std::iter::repeat_n(BULLET, text.chars().count()).collect())
  } else {
    Cow::Borrowed(text)
  }
}

/// Where byte `byte` of `text` falls in what the field shows
fn shown_byte(text: &str, password: bool, byte: usize) -> usize {
  if password {
    let chars = text.get(..byte).map_or(0, |before| before.chars().count());
    chars * BULLET.len_utf8()
  } else {
    byte
  }
}

/// `text` in `style` with its top-left at `at`, in `node`'s units
fn push_text(
  ui: &mut UiContext<'_>,
  node: usize,
  text: String,
  style: &TextStyle,
  at: Vec2,
  opacity: f32,
) {
  let text = RichText::new(text);
  let options = TextLayoutOptions::default();
  let scale = ui.ui_scale();
  let Some(layout) = ui.text_engine().layout(&text, style, &options, scale) else {
    return;
  };
  let seed = text_seed(&text, style, &options);
  ui.push_draw(
    node,
    DrawKind::Local(UiCommand::Text {
      layout,
      origin: at,
      reveal: None,
      opacity,
      seed,
      clip: None,
      layer: 0,
    }),
  );
}

/// `edges` with every side that is not a finite, non-negative number at 0
fn clean_edges(edges: Edges) -> Edges {
  let clean = |v: f32| if v.is_finite() { v.max(0.0) } else { 0.0 };
  Edges::new(
    clean(edges.left),
    clean(edges.top),
    clean(edges.right),
    clean(edges.bottom),
  )
}

#[cfg(all(test, feature = "default-fonts"))]
mod tests {
  use crate::core::ui::test_util::UiDriver;
  use crate::core::ui::{Rect, UiContext};
  use winit::keyboard::KeyCode;

  const FIELD: Rect = Rect::new(10.0, 10.0, 160.0, 28.0);

  fn field(ui: &mut UiContext<'_>, text: &mut String) -> crate::core::ui::Response {
    ui.text_input(text)
      .at(FIELD)
      .placeholder("Name")
      .max_len(8)
      .show()
  }

  fn focus(d: &mut UiDriver, text: &mut String) {
    d.frame(|ui| field(ui, text));
    d.move_to(20.0, 20.0);
    d.press();
    d.frame(|ui| field(ui, text));
    d.release();
    d.frame(|ui| field(ui, text));
  }

  #[test]
  fn a_field_takes_focus_on_click_outside_any_scope_and_types() {
    let mut d = UiDriver::new((300.0, 100.0));
    let mut name = String::new();
    focus(&mut d, &mut name);
    assert!(d.frame(|ui| {
      field(ui, &mut name);
      ui.wants_keyboard()
    }));
    d.type_text("Ryo");
    assert!(d.frame(|ui| field(ui, &mut name)).changed());
    assert_eq!(name, "Ryo");
  }

  #[test]
  fn max_len_stops_typing() {
    let mut d = UiDriver::new((300.0, 100.0));
    let mut name = String::new();
    focus(&mut d, &mut name);
    d.type_text("abcdefghijk");
    d.frame(|ui| field(ui, &mut name));
    assert_eq!(name, "abcdefgh");
  }

  #[test]
  fn escape_leaves_the_field_and_back_is_not_offered() {
    let mut d = UiDriver::new((300.0, 100.0));
    let mut name = String::from("x");
    focus(&mut d, &mut name);
    d.key(KeyCode::Escape);
    d.back();
    let back = d.frame(|ui| {
      field(ui, &mut name);
      ui.back()
    });
    assert!(!back, "the field used Escape");
    assert!(!d.frame(|ui| {
      field(ui, &mut name);
      ui.wants_keyboard()
    }));
  }

  #[test]
  fn enter_submits_and_keeps_focus() {
    let mut d = UiDriver::new((300.0, 100.0));
    let mut name = String::from("ok");
    focus(&mut d, &mut name);
    d.key(KeyCode::Enter);
    assert!(d.frame(|ui| field(ui, &mut name)).submitted());
    assert!(d.frame(|ui| field(ui, &mut name)).focused());
  }

  #[test]
  fn tab_moves_to_the_next_field_in_the_scope() {
    let mut d = UiDriver::new((300.0, 200.0));
    let (mut a, mut b) = (String::new(), String::new());
    let draw = |ui: &mut UiContext<'_>, a: &mut String, b: &mut String| {
      let mut ids = Vec::new();
      ui.column().focus_scope().show(|ui| {
        ids.push(ui.text_input(a).default_focus().show().id);
        ids.push(ui.text_input(b).show().id);
      });
      ids
    };
    let ids = d.frame(|ui| draw(ui, &mut a, &mut b));
    d.frame(|ui| draw(ui, &mut a, &mut b));
    d.tab(false);
    d.frame(|ui| draw(ui, &mut a, &mut b));
    assert_eq!(d.frame(|ui| ui.focused()), Some(ids[1]));
    d.type_text("2");
    d.frame(|ui| draw(ui, &mut a, &mut b));
    assert_eq!((a.as_str(), b.as_str()), ("", "2"));
  }

  #[test]
  fn a_password_field_draws_bullets() {
    use crate::core::ui::immediate::UiCommand;
    let mut d = UiDriver::new((300.0, 100.0));
    let mut secret = String::from("hunter");
    d.frame(|ui| ui.text_input(&mut secret).at(FIELD).password().show());
    let shown: Vec<String> = d
      .commands
      .iter()
      .filter_map(|(_, c)| match c {
        UiCommand::Text { layout, .. } => Some(layout.text().to_string()),
        _ => None,
      })
      .collect();
    assert!(shown.iter().any(|t| t == "••••••"), "{shown:?}");
    assert!(!shown.iter().any(|t| t.contains("hunter")));
  }

  #[test]
  fn a_lower_scenes_focused_field_gets_no_keys() {
    let mut d = UiDriver::new((300.0, 100.0));
    let mut name = String::new();
    focus(&mut d, &mut name);
    d.type_text("z");
    d.frame(|ui| {
      ui.end_scene();
      ui.begin_scene(1, false); // the field's scene is no longer the top one
      field(ui, &mut name);
      ui.end_scene();
      ui.begin_scene(2, true);
    });
    assert_eq!(name, "", "a scene under the top one does not type");
  }

  // ==========================================================================
  // Geometry: the caret, the selection, the text and its clip
  // ==========================================================================

  use super::super::text_edit::TextEdit;
  use crate::core::text::TextStyle;
  use crate::core::ui::immediate::UiCommand;
  use crate::core::ui::{Color, Theme, UiId};

  /// The style a field's text is laid out in (colour aside)
  fn body() -> TextStyle {
    Theme::dark().text.body.style()
  }

  /// Where a field at `FIELD` puts its text: inside its look's padding
  fn inner() -> Rect {
    let pad = Theme::dark().widgets.text_input.normal.padding;
    Rect::new(
      FIELD.x + pad.left,
      FIELD.y + pad.top,
      FIELD.width - pad.width(),
      FIELD.height - pad.height(),
    )
  }

  /// A field at `FIELD` with no limit
  fn plain(ui: &mut UiContext<'_>, text: &mut String) -> crate::core::ui::Response {
    ui.text_input(text).at(FIELD).show()
  }

  /// The caret of field `id`
  fn caret(d: &UiDriver, id: UiId) -> usize {
    d.memory
      .peek_state::<TextEdit>(id)
      .map_or(usize::MAX, TextEdit::cursor)
  }

  /// The rect commands of the last frame
  fn rects(d: &UiDriver) -> Vec<(Rect, Color)> {
    d.commands
      .iter()
      .filter_map(|(_, c)| match c {
        UiCommand::Rect { rect, fill, .. } => Some((*rect, *fill)),
        _ => None,
      })
      .collect()
  }

  /// The texts of the last frame, with their clips
  fn texts(d: &UiDriver) -> Vec<(String, Option<Rect>)> {
    d.commands
      .iter()
      .filter_map(|(_, c)| match c {
        UiCommand::Text { layout, clip, .. } => Some((layout.text().to_string(), *clip)),
        _ => None,
      })
      .collect()
  }

  /// Press at `x` (UI units) in the field's middle, held
  fn press_at(d: &mut UiDriver, x: f32, text: &mut String) -> UiId {
    d.frame(|ui| plain(ui, text));
    d.move_to(x, FIELD.y + FIELD.height / 2.0);
    d.press();
    d.frame(|ui| plain(ui, text)).id
  }

  #[test]
  fn a_click_puts_the_caret_at_the_nearest_character_boundary() {
    for content in ["hello world", "héllo wörld"] {
      let mut d = UiDriver::new((320.0, 100.0));
      let byte = content.char_indices().nth(5).map(|(b, _)| b).unwrap();
      let x = d.text.caret_x(content, &body(), byte, 1.0);
      let mut text = content.to_string();
      let id = press_at(&mut d, inner().x + x + 0.4, &mut text);
      assert_eq!(caret(&d, id), byte, "{content}");
    }
  }

  #[test]
  fn the_caret_is_drawn_at_the_text_engines_caret_x() {
    let mut d = UiDriver::new((320.0, 100.0));
    let content = "Wide caret";
    let expected = d.text.caret_x(content, &body(), 4, 1.0);
    let line = d.text.line_height(&body(), 1.0);
    let mut text = content.to_string();
    press_at(&mut d, inner().x + expected, &mut text);
    d.release();
    d.frame(|ui| plain(ui, &mut text));
    let carets: Vec<Rect> = rects(&d)
      .into_iter()
      .map(|(r, _)| r)
      .filter(|r| r.width == 1.5)
      .collect();
    assert_eq!(carets.len(), 1, "{carets:?}");
    let caret = carets[0];
    assert!((caret.x - (inner().x + expected)).abs() < 0.01, "{caret:?}");
    assert_eq!(caret.height, line);
    let centred = inner().y + (inner().height - line) / 2.0;
    assert!((caret.y - centred).abs() < 0.01, "{caret:?}");
  }

  #[test]
  fn a_drag_selects_and_the_selection_spans_the_text_engines_selection_x() {
    let mut d = UiDriver::new((320.0, 100.0));
    let content = "select these words";
    let (left, right) = d.text.selection_x(content, &body(), 7..12, 1.0);
    let mut text = content.to_string();
    let start = d.text.caret_x(content, &body(), 7, 1.0);
    let id = press_at(&mut d, inner().x + start, &mut text);
    d.move_to(inner().x + right, FIELD.y + 14.0);
    d.frame(|ui| plain(ui, &mut text));
    let edit = d.memory.peek_state::<TextEdit>(id).cloned().unwrap();
    assert_eq!(edit.selection(), Some(7..12));
    let accent = Theme::dark().palette.accent;
    let selection = rects(&d)
      .into_iter()
      .find(|(_, fill)| fill.r == accent.r && fill.g == accent.g && fill.a < 255)
      .map(|(r, _)| r)
      .expect("a selection rect");
    assert!((selection.x - (inner().x + left)).abs() < 0.01);
    assert!((selection.width - (right - left)).abs() < 0.01);
  }

  #[test]
  fn the_text_is_clipped_to_the_fields_inner_rect() {
    let mut d = UiDriver::new((320.0, 100.0));
    let mut text = String::from("a long line of text that runs past the field's edge");
    d.frame(|ui| plain(ui, &mut text));
    let drawn = texts(&d);
    assert_eq!(drawn.len(), 1);
    assert_eq!(drawn[0].1, Some(inner()));
  }

  #[test]
  fn the_placeholder_shows_while_the_field_is_empty_and_unfocused() {
    let mut d = UiDriver::new((300.0, 100.0));
    let mut name = String::new();
    d.frame(|ui| field(ui, &mut name));
    assert!(
      texts(&d).iter().any(|(t, _)| t == "Name"),
      "the placeholder"
    );
    focus(&mut d, &mut name);
    assert!(
      !texts(&d).iter().any(|(t, _)| t == "Name"),
      "focused: no placeholder"
    );
  }

  #[test]
  fn a_password_field_maps_the_caret_by_character() {
    let mut d = UiDriver::new((320.0, 100.0));
    let mut secret = String::from("héllo");
    let draw =
      |ui: &mut UiContext<'_>, text: &mut String| ui.text_input(text).at(FIELD).password().show();
    // The bullets' caret x before the 3rd
    let bullets = "•••••";
    let x = d.text.caret_x(bullets, &body(), 3 * '•'.len_utf8(), 1.0);
    d.frame(|ui| draw(ui, &mut secret));
    d.move_to(inner().x + x, FIELD.y + 14.0);
    d.press();
    let id = d.frame(|ui| draw(ui, &mut secret)).id;
    assert_eq!(caret(&d, id), "hél".len(), "the 3rd character of the text");
    d.release();
    d.type_text("X");
    d.frame(|ui| draw(ui, &mut secret));
    assert_eq!(secret, "hélXlo");
  }

  #[test]
  fn the_ime_composition_is_drawn_underlined_after_the_caret() {
    let mut d = UiDriver::new((300.0, 100.0));
    let mut name = String::from("ab");
    focus(&mut d, &mut name);
    d.preedit("cd");
    d.frame(|ui| field(ui, &mut name));
    assert!(texts(&d).iter().any(|(t, _)| t == "cd"), "{:?}", texts(&d));
    let width = d.text.measure(
      &crate::core::text::RichText::new("cd"),
      &body(),
      &crate::core::text::TextLayoutOptions::default(),
      1.0,
    );
    let underline = rects(&d)
      .into_iter()
      .find(|(r, _)| r.height == 1.0 && (r.width - width.x).abs() < 0.01);
    assert!(underline.is_some(), "{:?}", rects(&d));
    assert_eq!(name, "ab", "a composition is not typed yet");
  }

  #[test]
  fn it_is_the_themes_size_unless_told() {
    let mut d = UiDriver::new((400.0, 200.0));
    let (mut a, mut b) = (String::new(), String::new());
    // Inside a column a response has last frame's rect: draw twice
    let mut draw = || {
      d.frame(|ui| {
        let mut rects = (Rect::default(), Rect::default());
        ui.column().show(|ui| {
          rects.0 = ui.text_input(&mut a).show().rect;
          rects.1 = ui.text_input(&mut b).width(300.0).show().rect;
        });
        rects
      })
    };
    draw();
    let (ra, rb) = draw();
    let sizes = Theme::dark().metrics.sizes.text_input;
    assert_eq!((ra.width, ra.height), (sizes[0], sizes[1]));
    assert_eq!((rb.width, rb.height), (300.0, sizes[1]));
  }

  #[test]
  fn a_disabled_field_takes_no_focus_and_no_keys() {
    let mut d = UiDriver::new((300.0, 100.0));
    let mut name = String::new();
    let draw = |ui: &mut UiContext<'_>, text: &mut String| {
      ui.text_input(text).at(FIELD).enabled(false).show()
    };
    d.frame(|ui| draw(ui, &mut name));
    d.move_to(20.0, 20.0);
    d.press();
    d.frame(|ui| draw(ui, &mut name));
    d.release();
    d.type_text("x");
    assert!(!d.frame(|ui| draw(ui, &mut name)).focused());
    assert_eq!(name, "");
  }

  #[test]
  fn the_caret_blinks_at_one_hertz() {
    let mut d = UiDriver::new((300.0, 100.0));
    let mut name = String::new();
    focus(&mut d, &mut name);
    d.type_text("a");
    let mut shown = Vec::new();
    for _ in 0..90 {
      d.frame(|ui| field(ui, &mut name));
      shown.push(rects(&d).iter().any(|(r, _)| r.width == 1.5));
    }
    // Typed at frame 0: shown for half a second, hidden for half, shown again
    assert!(shown[..29].iter().all(|s| *s), "{shown:?}");
    assert!(shown[31..59].iter().all(|s| !*s), "{shown:?}");
    assert!(shown[61..89].iter().all(|s| *s), "{shown:?}");
  }

  #[test]
  fn navigation_focus_puts_the_caret_at_the_end() {
    let mut d = UiDriver::new((300.0, 100.0));
    let mut name = String::from("abc");
    let draw = |ui: &mut UiContext<'_>, text: &mut String| {
      ui.column().focus_scope().show(|ui| {
        ui.text_input(text).show();
      });
    };
    d.frame(|ui| draw(ui, &mut name));
    d.frame(|ui| draw(ui, &mut name));
    d.type_text("d");
    d.frame(|ui| draw(ui, &mut name));
    assert_eq!(name, "abcd");
  }

  #[test]
  fn escape_is_taken_from_the_scene() {
    let mut d = UiDriver::new((300.0, 100.0));
    let mut name = String::new();
    focus(&mut d, &mut name);
    d.key(KeyCode::Escape);
    let (focused, consumed) = d.frame(|ui| {
      let response = field(ui, &mut name);
      (
        response.focused(),
        ui.input().is_key_consumed(KeyCode::Escape),
      )
    });
    assert!(!focused, "it reports leaving at once");
    assert!(consumed);
  }

  // ==========================================================================
  // A focused field keeps the focus from the pointer and the scopes around it
  // ==========================================================================

  #[test]
  fn moving_the_mouse_over_a_button_keeps_a_typing_field_focused() {
    let mut d = UiDriver::new((300.0, 200.0));
    let mut name = String::new();
    let draw = |ui: &mut UiContext<'_>, name: &mut String| {
      let mut out = (UiId::NONE, UiId::NONE, Rect::default());
      ui.column().focus_scope().show(|ui| {
        out.0 = ui.text_input(name).show().id;
        let go = ui.button("Go").show();
        (out.1, out.2) = (go.id, go.rect);
      });
      out
    };
    d.frame(|ui| draw(ui, &mut name));
    let (field_id, _, go) = d.frame(|ui| draw(ui, &mut name));
    assert_eq!(
      d.frame(|ui| ui.focused()),
      Some(field_id),
      "its scope's default"
    );
    // The pointer passes over the button: the field keeps the keys
    d.move_to(go.x + 5.0, go.y + 5.0);
    d.frame(|ui| draw(ui, &mut name));
    d.move_to(go.x + 10.0, go.y + 6.0);
    d.type_text("a");
    let (_, go_id, _) = d.frame(|ui| draw(ui, &mut name));
    assert_eq!(d.frame(|ui| ui.focused()), Some(field_id));
    assert_eq!(name, "a");
    // A press on it still takes the focus
    d.press();
    d.frame(|ui| draw(ui, &mut name));
    assert_eq!(d.frame(|ui| ui.focused()), Some(go_id));
  }

  /// A field outside any scope at `FIELD`, and a menu with one button below it
  fn field_over_a_menu(ui: &mut UiContext<'_>, text: &mut String) -> UiId {
    let id = ui.text_input(text).at(FIELD).show().id;
    ui.column()
      .focus_scope()
      .at(Rect::new(10.0, 60.0, 120.0, 40.0))
      .show(|ui| {
        ui.button("Go");
      });
    id
  }

  #[test]
  fn tab_in_a_scope_less_field_keeps_it_focused() {
    let mut d = UiDriver::new((300.0, 200.0));
    let mut name = String::new();
    d.frame(|ui| field_over_a_menu(ui, &mut name));
    d.move_to(20.0, 20.0);
    d.press();
    let id = d.frame(|ui| field_over_a_menu(ui, &mut name));
    d.release();
    d.frame(|ui| field_over_a_menu(ui, &mut name));
    assert_eq!(d.frame(|ui| ui.focused()), Some(id));
    d.tab(false);
    d.frame(|ui| field_over_a_menu(ui, &mut name));
    assert_eq!(
      d.frame(|ui| ui.focused()),
      Some(id),
      "no tab order to follow"
    );
    assert!(!d.last_outcome().used.contains(&"ui.next"));
    d.type_text("t");
    d.frame(|ui| field_over_a_menu(ui, &mut name));
    assert_eq!(name, "t");
  }

  #[test]
  fn arrows_in_a_scope_less_field_keep_it_focused() {
    use crate::core::input::Direction;
    let mut d = UiDriver::new((300.0, 200.0));
    let mut name = String::new();
    d.frame(|ui| field_over_a_menu(ui, &mut name));
    d.move_to(20.0, 20.0);
    d.press();
    let id = d.frame(|ui| field_over_a_menu(ui, &mut name));
    d.release();
    d.frame(|ui| field_over_a_menu(ui, &mut name));
    for direction in [Direction::Down, Direction::Up, Direction::Right] {
      d.nav(direction);
      d.frame(|ui| field_over_a_menu(ui, &mut name));
      assert_eq!(d.frame(|ui| ui.focused()), Some(id), "{direction:?}");
    }
  }

  #[test]
  fn pad_back_leaves_the_field_and_is_not_offered() {
    let mut d = UiDriver::new((300.0, 100.0));
    let mut name = String::from("x");
    focus(&mut d, &mut name);
    // B on a pad: `ui.back` without the Escape key
    d.back();
    let (focused, back) = d.frame(|ui| {
      let response = field(ui, &mut name);
      (response.focused(), ui.back())
    });
    assert!(!focused, "B leaves the field");
    assert!(!back, "the field used it");
    assert!(!d.frame(|ui| {
      field(ui, &mut name);
      ui.wants_keyboard()
    }));
  }

  #[test]
  fn hovering_a_field_does_not_take_the_focus() {
    let mut d = UiDriver::new((300.0, 200.0));
    let mut name = String::new();
    let draw = |ui: &mut UiContext<'_>, name: &mut String| {
      let mut out = (UiId::NONE, Rect::default());
      ui.column().focus_scope().show(|ui| {
        out.0 = ui.button("Go").default_focus().show().id;
        out.1 = ui.text_input(name).show().rect;
      });
      out
    };
    d.frame(|ui| draw(ui, &mut name));
    let (go, at) = d.frame(|ui| draw(ui, &mut name));
    assert_eq!(d.frame(|ui| ui.focused()), Some(go));
    // The pointer crosses the field: the button keeps the keys
    d.move_to(at.x + 5.0, at.y + 5.0);
    d.frame(|ui| draw(ui, &mut name));
    d.move_to(at.x + 20.0, at.y + 8.0);
    d.type_text(" ");
    d.frame(|ui| draw(ui, &mut name));
    assert_eq!(d.frame(|ui| ui.focused()), Some(go));
    assert_eq!(name, "", "nothing typed into it");
  }

  #[test]
  fn a_space_or_enter_typed_in_the_field_never_clicks_it() {
    let mut d = UiDriver::new((300.0, 100.0));
    let mut name = String::new();
    focus(&mut d, &mut name);
    // Space and Enter are Confirm too
    d.confirm();
    d.key(KeyCode::Space);
    d.type_text(" ");
    let space = d.frame(|ui| field(ui, &mut name));
    assert!(!space.clicked());
    assert_eq!(name, " ");
    d.confirm();
    d.key(KeyCode::Enter);
    let enter = d.frame(|ui| field(ui, &mut name));
    assert!(!enter.clicked());
    assert!(enter.submitted());
    assert!(
      d.last_outcome().used.contains(&"ui.confirm"),
      "still the UI's"
    );
  }

  #[test]
  fn shift_with_an_arrow_selects_and_typing_replaces_the_selection() {
    let mut d = UiDriver::new((300.0, 100.0));
    let mut name = String::from("abc");
    focus(&mut d, &mut name);
    d.key(KeyCode::End);
    d.frame(|ui| field(ui, &mut name));
    d.shift(true);
    for _ in 0..2 {
      d.key(KeyCode::ArrowLeft);
      d.frame(|ui| field(ui, &mut name));
    }
    d.shift(false);
    d.type_text("X");
    d.frame(|ui| field(ui, &mut name));
    assert_eq!(name, "aX");
  }

  #[test]
  fn sizes_that_are_not_numbers_draw_only_finite_rects() {
    let mut d = UiDriver::new((300.0, 100.0));
    let mut text = String::from("abc");
    let draw = |ui: &mut UiContext<'_>, text: &mut String| {
      ui.text_input(text)
        .at(Rect::new(f32::NAN, 10.0, f32::INFINITY, -5.0))
        .show();
      ui.column().show(|ui| {
        ui.text_input(text).width(f32::NAN).height(-3.0).show();
      });
    };
    for _ in 0..3 {
      d.frame(|ui| draw(ui, &mut text));
      for (rect, _) in rects(&d) {
        assert!(
          [rect.x, rect.y, rect.width, rect.height]
            .iter()
            .all(|v| v.is_finite()),
          "{rect:?}"
        );
      }
    }
  }

  #[test]
  fn clicking_a_second_field_moves_focus_to_it() {
    let mut d = UiDriver::new((400.0, 100.0));
    let (mut a, mut b) = (String::new(), String::new());
    let (at_a, at_b) = (
      Rect::new(10.0, 10.0, 150.0, 28.0),
      Rect::new(200.0, 10.0, 150.0, 28.0),
    );
    // B drawn before A: the order they are drawn in does not matter
    let draw = |ui: &mut UiContext<'_>, a: &mut String, b: &mut String| {
      let b = ui.text_input(b).at(at_b).show().focused();
      let a = ui.text_input(a).at(at_a).show().focused();
      (a, b)
    };
    d.frame(|ui| draw(ui, &mut a, &mut b));
    for (x, expected) in [(20.0, (true, false)), (250.0, (false, true))] {
      d.move_to(x, 20.0);
      d.press();
      d.frame(|ui| draw(ui, &mut a, &mut b));
      d.release();
      assert_eq!(d.frame(|ui| draw(ui, &mut a, &mut b)), expected, "at {x}");
    }
    d.type_text("b");
    d.frame(|ui| draw(ui, &mut a, &mut b));
    assert_eq!((a.as_str(), b.as_str()), ("", "b"));
  }
}
