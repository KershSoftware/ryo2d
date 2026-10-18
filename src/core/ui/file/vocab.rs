//! The vocabulary of UI files: one table of node kinds and one of options
//!
//! A kind is a `UiNode` constructor in CamelCase and an option a `UiNode`
//! method of the same name, so a file is a spelling of the builder API.

use crate::core::render::batch::Filter;
use crate::core::text::TextAlign;
use crate::core::ui::nodes::TextRole;
use crate::core::ui::{Align, Anchor, Justify, Rect, UiNode};

use super::build::BuildCx;
use super::reader::Value;

/// How a value is written
#[derive(Clone, Copy)]
pub(crate) enum Form {
  Flag,
  Bool,
  Number,
  Whole,
  Length,
  Edges,
  Text,
  Asset,
  Color,
  Pair,
  Rect,
  Quad,
  Numbers,
  Strings,
  Range,
  Typewriter,
  Enum(&'static [&'static str]),
}

impl Form {
  /// What the form takes, for "`gap` takes a number"
  pub(crate) fn describe(self) -> String {
    match self {
      Form::Flag => "`true` or `false`".into(),
      Form::Bool => "a bool".into(),
      Form::Number => "a number".into(),
      Form::Whole => "a whole number".into(),
      Form::Length => "a length (a number, Pct(..), Fill or Fit)".into(),
      Form::Edges => "a number, (x, y) or (left: .., top: .., right: .., bottom: ..)".into(),
      Form::Text => "a string".into(),
      Form::Asset => "an asset path (a string)".into(),
      Form::Color => "a colour (\"#rrggbb\", \"#rrggbbaa\" or (r: .., g: .., b: .., a: ..))".into(),
      Form::Pair => "(x, y)".into(),
      Form::Rect => "(x, y, width, height)".into(),
      Form::Quad => "four numbers".into(),
      Form::Numbers => "a list of numbers".into(),
      Form::Strings => "a list of strings".into(),
      Form::Range => "(min, max)".into(),
      Form::Typewriter => "a number or (per_second: .., punctuation_pause: .., fade: ..)".into(),
      Form::Enum(names) => format!("one of {}", names.join(", ")),
    }
  }
}

pub(crate) struct OptionSpec {
  pub(crate) name: &'static str,
  pub(crate) apply: fn(UiNode, &Value, &mut BuildCx<'_>) -> UiNode,
}

pub(crate) struct ArgSpec {
  pub(crate) name: &'static str,
  pub(crate) required: bool,
}

const fn arg(name: &'static str, required: bool) -> ArgSpec {
  ArgSpec { name, required }
}

/// A constructor's arguments, as the file gave them
pub(crate) struct Ctor<'v> {
  pub(crate) args: Vec<(&'static str, &'v Value)>,
}

impl Ctor<'_> {
  fn read<T>(
    &self,
    cx: &mut BuildCx<'_>,
    name: &'static str,
    read: impl FnOnce(&mut BuildCx<'_>, &Value) -> Option<T>,
  ) -> Option<T> {
    let (_, value) = self.args.iter().find(|(arg, _)| *arg == name)?;
    cx.what = name;
    read(cx, value)
  }

  pub(crate) fn text(&self, cx: &mut BuildCx<'_>, name: &'static str) -> Option<String> {
    self.read(cx, name, |cx, v| cx.text(v))
  }

  pub(crate) fn bool(&self, cx: &mut BuildCx<'_>, name: &'static str) -> Option<bool> {
    self.read(cx, name, |cx, v| cx.bool(v))
  }

  pub(crate) fn number(&self, cx: &mut BuildCx<'_>, name: &'static str) -> Option<f32> {
    self.read(cx, name, |cx, v| cx.number(v))
  }

  pub(crate) fn whole(&self, cx: &mut BuildCx<'_>, name: &'static str, max: u64) -> Option<u64> {
    self.read(cx, name, |cx, v| cx.whole(v, max))
  }

  pub(crate) fn strings(&self, cx: &mut BuildCx<'_>, name: &'static str) -> Option<Vec<String>> {
    self.read(cx, name, |cx, v| cx.strings(v))
  }

  pub(crate) fn range(&self, cx: &mut BuildCx<'_>, name: &'static str) -> Option<(f32, f32)> {
    self.read(cx, name, |cx, v| cx.range(v))
  }

  pub(crate) fn asset(
    &self,
    cx: &mut BuildCx<'_>,
    name: &'static str,
  ) -> Option<crate::core::asset::SpriteHandle> {
    self.read(cx, name, |cx, v| cx.asset(v))
  }
}

pub(crate) struct KindSpec {
  pub(crate) name: &'static str,
  /// The option a single value stands for: `Button("Go")`
  pub(crate) main: Option<&'static str>,
  pub(crate) args: &'static [ArgSpec],
  pub(crate) make: fn(&Ctor<'_>, &mut BuildCx<'_>) -> Option<UiNode>,
}

macro_rules! plain {
  ($name:literal, $ctor:ident) => {
    KindSpec {
      name: $name,
      main: None,
      args: &[],
      make: |_, _| Some(UiNode::$ctor()),
    }
  };
}

/// A radio group, dropdown or tabs: options and the selected index
macro_rules! choices {
  ($name:literal, $ctor:ident) => {
    KindSpec {
      name: $name,
      main: None,
      args: &[arg("options", true), arg("selected", false)],
      make: |c, cx| {
        let options = c.strings(cx, "options").unwrap_or_default();
        let options: Vec<&str> = options.iter().map(String::as_str).collect();
        let selected = c.whole(cx, "selected", usize::MAX as u64).unwrap_or(0) as usize;
        Some(UiNode::$ctor(&options, selected))
      },
    }
  };
}

pub(crate) static KINDS: &[KindSpec] = &[
  plain!("Column", column),
  plain!("Row", row),
  plain!("Stack", stack),
  plain!("Panel", panel),
  plain!("Scroll", scroll),
  plain!("Slot", slot),
  plain!("Separator", separator),
  plain!("Spacer", spacer),
  KindSpec {
    name: "Grid",
    main: Some("columns"),
    args: &[arg("columns", true)],
    make: |c, cx| {
      let columns = c.whole(cx, "columns", u16::MAX as u64).unwrap_or(1) as u16;
      Some(UiNode::grid(columns))
    },
  },
  KindSpec {
    name: "Popup",
    main: None,
    args: &[arg("anchor_node", true)],
    make: |c, cx| Some(UiNode::popup(c.text(cx, "anchor_node").unwrap_or_default())),
  },
  KindSpec {
    name: "Text",
    main: Some("text"),
    args: &[arg("text", false)],
    make: |c, cx| Some(UiNode::text(c.text(cx, "text").unwrap_or_default())),
  },
  KindSpec {
    name: "Button",
    main: Some("label"),
    args: &[arg("label", false)],
    make: |c, cx| Some(UiNode::button(c.text(cx, "label").unwrap_or_default())),
  },
  KindSpec {
    name: "Image",
    main: Some("sprite"),
    args: &[arg("sprite", true)],
    make: |c, cx| c.asset(cx, "sprite").map(UiNode::image),
  },
  KindSpec {
    name: "Checkbox",
    main: Some("label"),
    args: &[arg("label", false), arg("checked", false)],
    make: |c, cx| {
      let label = c.text(cx, "label").unwrap_or_default();
      Some(UiNode::checkbox(
        label,
        c.bool(cx, "checked").unwrap_or(false),
      ))
    },
  },
  KindSpec {
    name: "Toggle",
    main: Some("label"),
    args: &[arg("label", false), arg("on", false)],
    make: |c, cx| {
      let label = c.text(cx, "label").unwrap_or_default();
      Some(UiNode::toggle(label, c.bool(cx, "on").unwrap_or(false)))
    },
  },
  choices!("RadioGroup", radio_group),
  choices!("Dropdown", dropdown),
  choices!("Tabs", tabs),
  KindSpec {
    name: "ChoiceList",
    main: None,
    args: &[arg("options", true)],
    make: |c, cx| {
      let options = c.strings(cx, "options").unwrap_or_default();
      let options: Vec<&str> = options.iter().map(String::as_str).collect();
      Some(UiNode::choice_list(&options))
    },
  },
  KindSpec {
    name: "Slider",
    main: None,
    args: &[arg("range", true), arg("value", false)],
    make: |c, cx| {
      let (min, max) = c.range(cx, "range").unwrap_or((0.0, 1.0));
      let value = c.number(cx, "value").unwrap_or(min);
      Some(UiNode::slider(min..=max, value))
    },
  },
  KindSpec {
    name: "Progress",
    main: Some("fraction"),
    args: &[arg("fraction", false)],
    make: |c, cx| Some(UiNode::progress(c.number(cx, "fraction").unwrap_or(0.0))),
  },
  KindSpec {
    name: "TextInput",
    main: Some("text"),
    args: &[arg("text", false)],
    make: |c, cx| Some(UiNode::text_input(c.text(cx, "text").unwrap_or_default())),
  },
  KindSpec {
    name: "Prompt",
    main: Some("input"),
    args: &[arg("input", true)],
    make: |c, cx| Some(UiNode::prompt(&c.text(cx, "input").unwrap_or_default())),
  },
  KindSpec {
    name: "Rebind",
    main: Some("input"),
    args: &[arg("input", true)],
    make: |c, cx| Some(UiNode::rebind(&c.text(cx, "input").unwrap_or_default())),
  },
  KindSpec {
    name: "Confirm",
    main: None,
    args: &[
      arg("title", true),
      arg("message", false),
      arg("yes", false),
      arg("no", false),
    ],
    make: |c, cx| {
      let title = c.text(cx, "title").unwrap_or_default();
      let message = c.text(cx, "message").unwrap_or_default();
      let yes = c.text(cx, "yes").unwrap_or_else(|| "Yes".into());
      let no = c.text(cx, "no").unwrap_or_else(|| "No".into());
      Some(UiNode::confirm(&title, &message, &yes, &no))
    },
  },
];

/// Defines the names of an enum option and the function from a name to its
/// value, from one list
macro_rules! choice {
  ($names:ident, $from:ident, $ty:ty, [$($name:literal => $value:expr),* $(,)?]) => {
    const $names: &[&str] = &[$($name),*];

    fn $from(name: &str) -> Option<$ty> {
      match name {
        $($name => Some($value),)*
        _ => None,
      }
    }
  };
}

choice!(ALIGNS, align_of, Align, [
  "Start" => Align::Start, "Center" => Align::Center, "End" => Align::End,
  "Stretch" => Align::Stretch,
]);
choice!(JUSTIFIES, justify_of, Justify, [
  "Start" => Justify::Start, "Center" => Justify::Center, "End" => Justify::End,
  "SpaceBetween" => Justify::SpaceBetween, "SpaceAround" => Justify::SpaceAround,
  "SpaceEvenly" => Justify::SpaceEvenly,
]);
choice!(ANCHORS, anchor_of, Anchor, [
  "TopLeft" => Anchor::TopLeft, "TopCenter" => Anchor::TopCenter,
  "TopRight" => Anchor::TopRight, "CenterLeft" => Anchor::CenterLeft,
  "Center" => Anchor::Center, "CenterRight" => Anchor::CenterRight,
  "BottomLeft" => Anchor::BottomLeft, "BottomCenter" => Anchor::BottomCenter,
  "BottomRight" => Anchor::BottomRight,
]);
choice!(TEXT_ALIGNS, text_align_of, TextAlign, [
  "Start" => TextAlign::Start, "Center" => TextAlign::Center, "End" => TextAlign::End,
  "Justify" => TextAlign::Justify,
]);
choice!(FILTERS, filter_of, Filter, [
  "Nearest" => Filter::Nearest, "Linear" => Filter::Linear,
]);
choice!(ROLES, role_of, TextRole, [
  "Body" => TextRole::Body, "Heading" => TextRole::Heading, "Button" => TextRole::Button,
  "Caption" => TextRole::Caption, "Mono" => TextRole::Mono,
]);

macro_rules! option {
  ($name:literal, Flag, $method:ident) => {
    OptionSpec {
      name: $name,
      apply: |node, value, cx| if cx.flag(value) { node.$method() } else { node },
    }
  };
  ($name:literal, Whole, $ty:ty, $method:ident) => {
    OptionSpec {
      name: $name,
      apply: |node, value, cx| match cx.whole(value, <$ty>::MAX as u64) {
        Some(v) => node.$method(v as $ty),
        None => node,
      },
    }
  };
  ($name:literal, Enum, $names:ident, $from:ident, $method:ident) => {
    OptionSpec {
      name: $name,
      apply: |node, value, cx| match cx.enum_name(value, $names).and_then($from) {
        Some(v) => node.$method(v),
        None => node,
      },
    }
  };
  ($name:literal, $read:ident, $method:ident) => {
    OptionSpec {
      name: $name,
      apply: |node, value, cx| match cx.$read(value) {
        Some(v) => node.$method(v),
        None => node,
      },
    }
  };
}

pub(crate) static OPTIONS: &[OptionSpec] = &[
  option!("fill_width", Flag, fill_width),
  option!("fill_height", Flag, fill_height),
  option!("fill", Flag, fill),
  option!("center", Flag, center),
  option!("wrap_items", Flag, wrap_items),
  option!("focus_scope", Flag, focus_scope),
  option!("trap", Flag, trap),
  option!("wrap_focus", Flag, wrap_focus),
  option!("block_pointer", Flag, block_pointer),
  option!("default_focus", Flag, default_focus),
  option!("nav_skip", Flag, nav_skip),
  option!("drag_source", Flag, drag_source),
  option!("drop_target", Flag, drop_target),
  option!("modal", Flag, modal),
  option!("hidden", Flag, hidden),
  option!("embedded", Flag, embedded),
  option!("markup", Flag, markup),
  option!("heading", Flag, heading),
  option!("muted", Flag, muted),
  option!("ellipsis", Flag, ellipsis),
  option!("show_value", Flag, show_value),
  option!("password", Flag, password),
  option!("horizontal", Flag, horizontal),
  option!("both", Flag, both),
  option!("no_bar", Flag, no_bar),
  option!("enabled", bool, enabled),
  option!("focusable", bool, focusable),
  option!("selected", bool, selected),
  option!("min_width", number, min_width),
  option!("max_width", number, max_width),
  option!("min_height", number, min_height),
  option!("max_height", number, max_height),
  option!("gap", number, gap),
  option!("grow", number, grow),
  option!("aspect", number, aspect),
  option!("size", number, size),
  option!("wrap", number, wrap),
  option!("step", number, step),
  option!("max_lines", Whole, u32, max_lines),
  option!("max_len", Whole, usize, max_len),
  option!("default_choice", Whole, usize, default_choice),
  option!("count", Whole, u32, count),
  option!("binding_slot", Whole, usize, binding_slot),
  option!("width", length, width),
  option!("height", length, height),
  option!("padding", edges, padding),
  option!("margin", edges, margin),
  option!("align", Enum, ALIGNS, align_of, align),
  option!("justify", Enum, JUSTIFIES, justify_of, justify),
  option!("anchor", Enum, ANCHORS, anchor_of, anchor),
  option!("text_align", Enum, TEXT_ALIGNS, text_align_of, text_align),
  option!("filter", Enum, FILTERS, filter_of, filter),
  option!("role", Enum, ROLES, role_of, role),
  OptionSpec {
    name: "offset",
    apply: |node, value, cx| match cx.pair(value) {
      Some((x, y)) => node.offset(x, y),
      None => node,
    },
  },
  OptionSpec {
    name: "image_size",
    apply: |node, value, cx| match cx.pair(value) {
      Some((width, height)) => node.image_size(width, height),
      None => node,
    },
  },
  OptionSpec {
    name: "at",
    apply: |node, value, cx| match cx.rect(value) {
      Some([x, y, width, height]) => node.at(Rect::new(x, y, width, height)),
      None => node,
    },
  },
  option!("uv", quad, uv),
  OptionSpec {
    name: "tracks",
    apply: |node, value, cx| match cx.numbers(value) {
      Some(tracks) => node.tracks(&tracks),
      None => node,
    },
  },
  option!("background", color, background),
  option!("color", color, color),
  option!("tint", color, tint),
  option!("tooltip", text, tooltip),
  OptionSpec {
    name: "variant",
    apply: |node, value, cx| match cx.text(value) {
      Some(v) => node.variant(&v),
      None => node,
    },
  },
  option!("action", text, action),
  option!("name", text, name),
  option!("key", text, key),
  option!("bind", text, bind),
  option!("nav_up", text, nav_up),
  option!("nav_down", text, nav_down),
  option!("nav_left", text, nav_left),
  option!("nav_right", text, nav_right),
  option!("label", text, label),
  option!("placeholder", text, placeholder),
  option!("icon", asset, icon),
  OptionSpec {
    name: "typewriter",
    apply: |node, value, cx| match cx.typewriter(value) {
      Some(reveal) => node.typewriter(reveal),
      None => node,
    },
  },
];

/// The options every node takes, whatever its kind
pub(crate) const UNIVERSAL: &[&str] = &["name", "action", "hidden", "key"];

pub(crate) fn kind_spec(name: &str) -> Option<&'static KindSpec> {
  KINDS.iter().find(|kind| kind.name == name)
}

pub(crate) fn option_spec(name: &str) -> Option<&'static OptionSpec> {
  OPTIONS.iter().find(|option| option.name == name)
}

/// The candidate nearest to `word` by edit distance, ignoring case, if it is
/// close enough to be a typo (the first wins a tie)
pub(crate) fn nearest<'a>(
  word: &str,
  candidates: impl IntoIterator<Item = &'a str>,
) -> Option<&'a str> {
  let word: Vec<char> = word.to_lowercase().chars().collect();
  let allowed = (word.len() / 3).max(1);
  let mut best: Option<(usize, &'a str)> = None;
  for candidate in candidates {
    let other: Vec<char> = candidate.to_lowercase().chars().collect();
    let distance = edit_distance(&word, &other);
    if distance <= allowed && best.is_none_or(|(best, _)| distance < best) {
      best = Some((distance, candidate));
    }
  }
  best.map(|(_, name)| name)
}

/// Edits between two words: inserting, deleting or changing a character,
/// or swapping two neighbours (`gpa` is one edit from `gap`)
fn edit_distance(a: &[char], b: &[char]) -> usize {
  let width = b.len() + 1;
  let mut d: Vec<usize> = vec![0; (a.len() + 1) * width];
  for (i, row) in d.chunks_mut(width).enumerate() {
    row[0] = i;
  }
  for (j, cell) in d.iter_mut().take(width).enumerate() {
    *cell = j;
  }
  for i in 1..=a.len() {
    for j in 1..=b.len() {
      let change = usize::from(a[i - 1] != b[j - 1]);
      let mut best = (d[(i - 1) * width + j] + 1)
        .min(d[i * width + j - 1] + 1)
        .min(d[(i - 1) * width + j - 1] + change);
      if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
        best = best.min(d[(i - 2) * width + j - 2] + 1);
      }
      d[i * width + j] = best;
    }
  }
  d[a.len() * width + b.len()]
}
