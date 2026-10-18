//! Building `UiNode`s from the values the reader produced
//!
//! Mistakes are collected with their positions rather than stopping at the
//! first: building goes on past one, skipping what failed.

use std::collections::HashMap;

use crate::core::asset::SpriteHandle;
use crate::core::graphics::Sprite;
use crate::core::text::TextReveal;
use crate::core::ui::{Color, Edges, Length, UiNode};

use super::error::describe;
use super::reader::{Body, Issue, Pos, Value, ValueKind};
use super::vocab::{
  Ctor, Form, KINDS, KindSpec, OPTIONS, UNIVERSAL, kind_spec, nearest, option_spec,
};

/// Where a file's images come from
pub(crate) trait SpriteSource {
  fn sprite(&mut self, path: &str) -> Result<SpriteHandle, String>;
}

/// Stands in for every image: one shared 1x1 handle per path, never failing
#[derive(Default)]
pub(crate) struct Placeholders {
  handles: HashMap<String, SpriteHandle>,
}

impl SpriteSource for Placeholders {
  fn sprite(&mut self, path: &str) -> Result<SpriteHandle, String> {
    let handle = self
      .handles
      .entry(path.to_string())
      .or_insert_with(|| SpriteHandle::new(Sprite::solid_rect(1, 1, 0, 0, 0, 0), path.into()));
    Ok(handle.clone())
  }
}

pub(crate) struct BuildCx<'a> {
  pub(crate) sprites: &'a mut dyn SpriteSource,
  pub(crate) issues: Vec<Issue>,
  /// The option or argument being read, for messages
  pub(crate) what: &'static str,
}

impl BuildCx<'_> {
  pub(crate) fn new(sprites: &mut dyn SpriteSource) -> BuildCx<'_> {
    BuildCx {
      sprites,
      issues: Vec::new(),
      what: "",
    }
  }

  fn issue(&mut self, pos: Pos, message: impl Into<String>) {
    self.issues.push(Issue {
      pos,
      message: message.into(),
    });
  }

  /// Whether the value can be read at all: an `Any` is accepted and applies
  /// nothing, and a lowercase bare name is a parameter used outside a template
  fn readable(&mut self, value: &Value) -> bool {
    match &value.kind {
      ValueKind::Any => false,
      ValueKind::Named {
        name,
        body: Body::Unit,
      } if is_lowercase(name) => {
        let message =
          format!("\"{name}\" is not a value here; parameters exist only inside templates");
        self.issue(value.pos, message);
        false
      }
      _ => true,
    }
  }

  /// Report a value of the wrong shape for `form`
  fn mismatch<T>(&mut self, value: &Value, form: Form) -> Option<T> {
    let message = format!(
      "\"{}\" takes {}, not {}",
      self.what,
      form.describe(),
      describe(&value.kind)
    );
    self.issue(value.pos, message);
    None
  }

  pub(crate) fn flag(&mut self, value: &Value) -> bool {
    if !self.readable(value) {
      return false;
    }
    match value.kind {
      ValueKind::Bool(on) => on,
      _ => {
        self.mismatch::<()>(value, Form::Flag);
        false
      }
    }
  }

  pub(crate) fn bool(&mut self, value: &Value) -> Option<bool> {
    if !self.readable(value) {
      return None;
    }
    match value.kind {
      ValueKind::Bool(on) => Some(on),
      _ => self.mismatch(value, Form::Bool),
    }
  }

  pub(crate) fn number(&mut self, value: &Value) -> Option<f32> {
    if !self.readable(value) {
      return None;
    }
    match value.kind {
      ValueKind::Num(n) => Some(n as f32),
      _ => self.mismatch(value, Form::Number),
    }
  }

  /// A non-negative whole number no larger than `max`
  pub(crate) fn whole(&mut self, value: &Value, max: u64) -> Option<u64> {
    if !self.readable(value) {
      return None;
    }
    let ValueKind::Num(n) = value.kind else {
      return self.mismatch(value, Form::Whole);
    };
    if n < 0.0 || n.fract() != 0.0 {
      let message = format!("\"{}\" takes a whole number, not {n}", self.what);
      self.issue(value.pos, message);
      return None;
    }
    if n > max as f64 {
      let message = format!(
        "\"{}\" takes a whole number up to {max}, not {n}",
        self.what
      );
      self.issue(value.pos, message);
      return None;
    }
    Some(n as u64)
  }

  pub(crate) fn length(&mut self, value: &Value) -> Option<Length> {
    if !self.readable(value) {
      return None;
    }
    match &value.kind {
      ValueKind::Num(n) => Some(Length::Px(*n as f32)),
      ValueKind::Named { name, body } => match (name.as_str(), body) {
        ("Fill", Body::Unit) => Some(Length::Fill),
        ("Fit", Body::Unit) => Some(Length::Fit),
        ("Pct", Body::Single(inner)) => self.number(inner).map(Length::Pct),
        _ => self.mismatch(value, Form::Length),
      },
      _ => self.mismatch(value, Form::Length),
    }
  }

  pub(crate) fn edges(&mut self, value: &Value) -> Option<Edges> {
    if !self.readable(value) {
      return None;
    }
    match &value.kind {
      ValueKind::Num(n) => Some(Edges::from(*n as f32)),
      ValueKind::Tuple(_) => {
        let (x, y) = self.tuple::<2>(value, Form::Edges).map(|[x, y]| (x, y))?;
        Some(Edges::from((x, y)))
      }
      ValueKind::Fields(fields) => {
        let mut edges = Edges::ZERO;
        let mut ok = true;
        for field in fields {
          let slot = match field.name.as_str() {
            "left" => &mut edges.left,
            "top" => &mut edges.top,
            "right" => &mut edges.right,
            "bottom" => &mut edges.bottom,
            other => {
              let message = format!(
                "\"{}\" has no field \"{other}\"; it takes left, top, right and bottom",
                self.what
              );
              self.issue(field.pos, message);
              ok = false;
              continue;
            }
          };
          match self.number(&field.value) {
            Some(n) => *slot = n,
            None => ok = false,
          }
        }
        ok.then_some(edges)
      }
      _ => self.mismatch(value, Form::Edges),
    }
  }

  pub(crate) fn text(&mut self, value: &Value) -> Option<String> {
    if !self.readable(value) {
      return None;
    }
    match &value.kind {
      ValueKind::Str(text) => Some(text.clone()),
      _ => self.mismatch(value, Form::Text),
    }
  }

  pub(crate) fn asset(&mut self, value: &Value) -> Option<SpriteHandle> {
    if !self.readable(value) {
      return None;
    }
    let ValueKind::Str(path) = &value.kind else {
      return self.mismatch(value, Form::Asset);
    };
    match self.sprites.sprite(path) {
      Ok(handle) => Some(handle),
      Err(message) => {
        self.issue(
          value.pos,
          format!("cannot load the image \"{path}\": {message}"),
        );
        None
      }
    }
  }

  pub(crate) fn color(&mut self, value: &Value) -> Option<Color> {
    if !self.readable(value) {
      return None;
    }
    match &value.kind {
      ValueKind::Str(text) => match parse_hex(text) {
        Some(color) => Some(color),
        None => self.mismatch(value, Form::Color),
      },
      ValueKind::Fields(fields) => {
        let mut channels = [None, None, None, Some(255u8)];
        let mut ok = true;
        for field in fields {
          let index = match field.name.as_str() {
            "r" => 0,
            "g" => 1,
            "b" => 2,
            "a" => 3,
            other => {
              let message = format!(
                "\"{}\" has no field \"{other}\"; it takes r, g, b and a",
                self.what
              );
              self.issue(field.pos, message);
              ok = false;
              continue;
            }
          };
          match self.whole(&field.value, 255) {
            Some(n) => channels[index] = Some(n as u8),
            None => ok = false,
          }
        }
        if !ok {
          return None;
        }
        let [Some(r), Some(g), Some(b), Some(a)] = channels else {
          let message = format!("\"{}\" needs r, g and b", self.what);
          self.issue(value.pos, message);
          return None;
        };
        Some(Color::new(r, g, b, a))
      }
      _ => self.mismatch(value, Form::Color),
    }
  }

  /// A tuple of exactly `N` numbers
  fn tuple<const N: usize>(&mut self, value: &Value, form: Form) -> Option<[f32; N]> {
    if !self.readable(value) {
      return None;
    }
    let ValueKind::Tuple(items) = &value.kind else {
      return self.mismatch(value, form);
    };
    if items.len() != N {
      return self.mismatch(value, form);
    }
    let mut out = [0.0; N];
    for (slot, item) in out.iter_mut().zip(items) {
      match &item.kind {
        ValueKind::Num(n) => *slot = *n as f32,
        _ if !self.readable(item) => return None,
        _ => return self.mismatch(item, form),
      }
    }
    Some(out)
  }

  pub(crate) fn pair(&mut self, value: &Value) -> Option<(f32, f32)> {
    self.tuple::<2>(value, Form::Pair).map(|[x, y]| (x, y))
  }

  pub(crate) fn rect(&mut self, value: &Value) -> Option<[f32; 4]> {
    self.tuple::<4>(value, Form::Rect)
  }

  pub(crate) fn quad(&mut self, value: &Value) -> Option<[f32; 4]> {
    self.tuple::<4>(value, Form::Quad)
  }

  pub(crate) fn range(&mut self, value: &Value) -> Option<(f32, f32)> {
    self
      .tuple::<2>(value, Form::Range)
      .map(|[min, max]| (min, max))
  }

  pub(crate) fn numbers(&mut self, value: &Value) -> Option<Vec<f32>> {
    if !self.readable(value) {
      return None;
    }
    let ValueKind::List(items) = &value.kind else {
      return self.mismatch(value, Form::Numbers);
    };
    let mut out = Vec::with_capacity(items.len());
    for item in items {
      match &item.kind {
        ValueKind::Num(n) => out.push(*n as f32),
        _ if !self.readable(item) => return None,
        _ => return self.mismatch(item, Form::Numbers),
      }
    }
    Some(out)
  }

  pub(crate) fn strings(&mut self, value: &Value) -> Option<Vec<String>> {
    if !self.readable(value) {
      return None;
    }
    let ValueKind::List(items) = &value.kind else {
      return self.mismatch(value, Form::Strings);
    };
    let mut out = Vec::with_capacity(items.len());
    for item in items {
      match &item.kind {
        ValueKind::Str(text) => out.push(text.clone()),
        _ if !self.readable(item) => return None,
        _ => return self.mismatch(item, Form::Strings),
      }
    }
    Some(out)
  }

  pub(crate) fn typewriter(&mut self, value: &Value) -> Option<TextReveal> {
    if !self.readable(value) {
      return None;
    }
    match &value.kind {
      ValueKind::Num(n) => Some(TextReveal::new(*n as f32)),
      ValueKind::Fields(fields) => {
        let mut per_second = None;
        let mut punctuation_pause = None;
        let mut fade = None;
        let mut ok = true;
        for field in fields {
          let slot = match field.name.as_str() {
            "per_second" => &mut per_second,
            "punctuation_pause" => &mut punctuation_pause,
            "fade" => &mut fade,
            other => {
              let message = format!(
                "\"{}\" has no field \"{other}\"; it takes per_second, punctuation_pause and fade",
                self.what
              );
              self.issue(field.pos, message);
              ok = false;
              continue;
            }
          };
          match self.number(&field.value) {
            Some(n) => *slot = Some(n),
            None => ok = false,
          }
        }
        if !ok {
          return None;
        }
        let Some(per_second) = per_second else {
          let message = format!("\"{}\" needs \"per_second\"", self.what);
          self.issue(value.pos, message);
          return None;
        };
        let mut reveal = TextReveal::new(per_second);
        if let Some(pause) = punctuation_pause {
          reveal.punctuation_pause = pause;
        }
        if let Some(fade) = fade {
          reveal.fade = fade;
        }
        Some(reveal)
      }
      _ => self.mismatch(value, Form::Typewriter),
    }
  }

  /// One of `names`, written as a bare name
  pub(crate) fn enum_name(
    &mut self,
    value: &Value,
    names: &'static [&'static str],
  ) -> Option<&'static str> {
    if !self.readable(value) {
      return None;
    }
    let form = Form::Enum(names);
    let ValueKind::Named {
      name,
      body: Body::Unit,
    } = &value.kind
    else {
      return self.mismatch(value, form);
    };
    if let Some(found) = names.iter().find(|candidate| **candidate == name) {
      return Some(found);
    }
    let mut message = format!(
      "\"{}\" takes {}; \"{name}\" is not one",
      self.what,
      form.describe()
    );
    if let Some(close) = nearest(name, names.iter().copied()) {
      message.push_str(&format!("; did you mean \"{close}\"?"));
    }
    self.issue(value.pos, message);
    None
  }
}

fn is_lowercase(name: &str) -> bool {
  name.chars().next().is_some_and(char::is_lowercase)
}

/// `#rrggbb` or `#rrggbbaa`, either case
fn parse_hex(text: &str) -> Option<Color> {
  let digits = text.strip_prefix('#')?;
  if !matches!(digits.len(), 6 | 8) || !digits.chars().all(|c| c.is_ascii_hexdigit()) {
    return None;
  }
  let byte = |i: usize| u8::from_str_radix(&digits[i..i + 2], 16).ok();
  let alpha = if digits.len() == 8 { byte(6)? } else { 255 };
  Some(Color::new(byte(0)?, byte(2)?, byte(4)?, alpha))
}

/// Mark a node and everything under it as binding patterns: in a file,
/// labels and tooltips are patterns
fn bound_all(mut node: UiNode) -> UiNode {
  node = node.bound();
  node.children = node.children.into_iter().map(bound_all).collect();
  node
}

/// A field a node's file value gave, with where
struct Entry<'v> {
  name: &'v str,
  pos: Pos,
  value: &'v Value,
}

/// Build a node from a `Kind(..)` value
pub(crate) fn build_node(value: &Value, cx: &mut BuildCx<'_>) -> Option<UiNode> {
  let (name, body) = match &value.kind {
    ValueKind::Any => return None,
    ValueKind::Named { name, body } => (name, body),
    other => {
      let message = format!(
        "expected a node such as Button(..), not {}",
        describe(other)
      );
      cx.issue(value.pos, message);
      return None;
    }
  };
  if matches!(body, Body::Unit) && is_lowercase(name) {
    cx.readable(value);
    return None;
  }
  let Some(spec) = kind_spec(name) else {
    let mut message = format!("unknown node kind \"{name}\"");
    if let Some(close) = nearest(name, KINDS.iter().map(|kind| kind.name)) {
      message.push_str(&format!("; did you mean \"{close}\"?"));
    }
    cx.issue(value.pos, message);
    return None;
  };
  let entries: Vec<Entry<'_>> = match body {
    Body::Unit => Vec::new(),
    Body::Single(inner) => {
      let Some(main) = spec.main else {
        cx.issue(
          inner.pos,
          format!("{} has no short form; name its options", spec.name),
        );
        return None;
      };
      vec![Entry {
        name: main,
        pos: inner.pos,
        value: inner,
      }]
    }
    Body::Fields(fields) => fields
      .iter()
      .map(|field| Entry {
        name: &field.name,
        pos: field.pos,
        value: &field.value,
      })
      .collect(),
  };

  let mut ctor = Ctor { args: Vec::new() };
  let mut missing = false;
  for arg in spec.args {
    match entries.iter().find(|entry| entry.name == arg.name) {
      Some(entry) => ctor.args.push((arg.name, entry.value)),
      None if arg.required => {
        cx.issue(value.pos, format!("{} needs \"{}\"", spec.name, arg.name));
        missing = true;
      }
      None => {}
    }
  }
  if missing {
    return None;
  }
  let mut node = bound_all((spec.make)(&ctor, cx)?);

  let mut keys: Vec<String> = Vec::new();
  for entry in entries {
    if spec.args.iter().any(|arg| arg.name == entry.name) {
      continue;
    }
    node = apply_field(node, spec, &entry, cx, &mut keys);
  }
  Some(node)
}

fn apply_field(
  node: UiNode,
  spec: &KindSpec,
  entry: &Entry<'_>,
  cx: &mut BuildCx<'_>,
  keys: &mut Vec<String>,
) -> UiNode {
  let name = entry.name;
  if name == "children" {
    if !node.kind.takes("child") {
      cx.issue(
        entry.pos,
        format!("{} does not take \"children\"", spec.name),
      );
      return node;
    }
    return add_children(node, entry, cx, keys);
  }
  let Some(option) = option_spec(name) else {
    let mut message = format!("{} has no option \"{name}\"", spec.name);
    let takes = OPTIONS
      .iter()
      .map(|option| option.name)
      .filter(|option| UNIVERSAL.contains(option) || node.kind.takes(option))
      .chain(node.kind.takes("child").then_some("children"));
    if let Some(close) = nearest(name, takes) {
      message.push_str(&format!("; did you mean \"{close}\"?"));
    }
    cx.issue(entry.pos, message);
    return node;
  };
  if !UNIVERSAL.contains(&name) && !node.kind.takes(name) {
    cx.issue(entry.pos, format!("{} does not take \"{name}\"", spec.name));
    return node;
  }
  cx.what = option.name;
  (option.apply)(node, entry.value, cx)
}

fn add_children(
  mut node: UiNode,
  entry: &Entry<'_>,
  cx: &mut BuildCx<'_>,
  keys: &mut Vec<String>,
) -> UiNode {
  let items = match &entry.value.kind {
    ValueKind::Any => return node,
    ValueKind::List(items) => items,
    other => {
      let message = format!(
        "\"children\" takes a list of nodes, not {}",
        describe(other)
      );
      cx.issue(entry.value.pos, message);
      return node;
    }
  };
  for item in items {
    let Some(child) = build_node(item, cx) else {
      continue;
    };
    if let Some(key) = child.key.as_ref().or(child.name.as_ref()) {
      if keys.contains(key) {
        let message = format!("two children are keyed \"{key}\"; give one a different key or name");
        cx.issue(item.pos, message);
      } else {
        keys.push(key.clone());
      }
    }
    node = node.child(child);
  }
  node
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::graphics::Sprite;
  use crate::core::ui::file::reader::read;
  use crate::core::ui::nodes::fingerprint;
  use crate::prelude::*;

  fn build(text: &str) -> (Option<UiNode>, Vec<Issue>) {
    let value = read(text).expect("reads");
    let mut sprites = Placeholders::default();
    let mut cx = BuildCx::new(&mut sprites);
    let node = build_node(&value, &mut cx);
    (node, cx.issues)
  }

  fn same(text: &str, code: UiNode) {
    let (node, issues) = build(text);
    assert!(issues.is_empty(), "{text}: {issues:?}");
    assert_eq!(
      fingerprint(&node.expect("built")),
      fingerprint(&bound_all(code)),
      "{text}"
    );
  }

  /// A code node as a file builds it: every node bound
  fn bound_all(mut node: UiNode) -> UiNode {
    node.bound = true;
    node.children = node.children.into_iter().map(bound_all).collect();
    node
  }

  fn sprite(path: &str) -> SpriteHandle {
    SpriteHandle::new(Sprite::solid_rect(1, 1, 0, 0, 0, 0), path.into())
  }

  fn messages(text: &str) -> Vec<String> {
    build(text).1.into_iter().map(|i| i.message).collect()
  }

  #[test]
  fn every_kind_builds_what_its_constructor_builds() {
    same("Column", UiNode::column());
    same("Row", UiNode::row());
    same("Stack", UiNode::stack());
    same("Panel", UiNode::panel());
    same("Scroll", UiNode::scroll());
    same("Grid(3)", UiNode::grid(3));
    same("Popup(anchor_node: \"sort\")", UiNode::popup("sort"));
    same("Text(\"Hi\")", UiNode::text("Hi"));
    same("Button(label: \"Go\")", UiNode::button("Go"));
    same(
      "Image(\"items/sword.png\")",
      UiNode::image(sprite("items/sword.png")),
    );
    same(
      "Checkbox(label: \"Mute\", checked: true)",
      UiNode::checkbox("Mute", true),
    );
    same("Toggle(\"Full\")", UiNode::toggle("Full", false));
    same(
      "RadioGroup(options: [\"S\", \"M\"], selected: 1)",
      UiNode::radio_group(&["S", "M"], 1),
    );
    same(
      "Dropdown(options: [\"A\", \"B\"])",
      UiNode::dropdown(&["A", "B"], 0),
    );
    same(
      "Tabs(options: [\"A\", \"B\"], selected: 1)",
      UiNode::tabs(&["A", "B"], 1),
    );
    same(
      "ChoiceList(options: [\"Yes\", \"No\"])",
      UiNode::choice_list(&["Yes", "No"]),
    );
    same(
      "Slider(range: (0, 10), value: 4)",
      UiNode::slider(0.0..=10.0, 4.0),
    );
    same("Slider(range: (2, 10))", UiNode::slider(2.0..=10.0, 2.0));
    same("Progress(0.25)", UiNode::progress(0.25));
    same("TextInput(text: \"Kai\")", UiNode::text_input("Kai"));
    same("Slot", UiNode::slot());
    same("Prompt(\"jump\")", UiNode::prompt("jump"));
    same("Rebind(input: \"jump\")", UiNode::rebind("jump"));
    same("Separator", UiNode::separator());
    same("Spacer", UiNode::spacer());
    same(
      "Confirm(title: \"Quit?\")",
      UiNode::confirm("Quit?", "", "Yes", "No"),
    );
  }

  #[test]
  fn every_option_applies_the_method_of_its_name() {
    same(
      "Column(width: 240, height: Pct(0.5), min_width: 1, max_width: 2, min_height: 3, max_height: 4, padding: (8, 12), margin: (left: 1, top: 2, right: 3, bottom: 4), gap: 6, align: Center, justify: SpaceBetween, grow: 1, wrap_items: true, aspect: 2, anchor: BottomRight, offset: (5, 6), background: \"#ff8800\", focus_scope: true, trap: true, wrap_focus: true, block_pointer: true, drag_source: true, drop_target: true, modal: true, embedded: true, name: \"n\", action: \"a\", key: \"k\", hidden: true)",
      UiNode::column()
        .width(240.0)
        .height(Length::Pct(0.5))
        .min_width(1.0)
        .max_width(2.0)
        .min_height(3.0)
        .max_height(4.0)
        .padding((8.0, 12.0))
        .margin(Edges {
          left: 1.0,
          top: 2.0,
          right: 3.0,
          bottom: 4.0,
        })
        .gap(6.0)
        .align(Align::Center)
        .justify(Justify::SpaceBetween)
        .grow(1.0)
        .wrap_items()
        .aspect(2.0)
        .anchor(Anchor::BottomRight)
        .offset(5.0, 6.0)
        .background(Color::new(255, 136, 0, 255))
        .focus_scope()
        .trap()
        .wrap_focus()
        .block_pointer()
        .drag_source()
        .drop_target()
        .modal()
        .embedded()
        .name("n")
        .action("a")
        .key("k")
        .hidden(),
    );
    same(
      "Column(fill: true, center: true)",
      UiNode::column().fill().center(),
    );
    same(
      "Row(fill_width: true, fill_height: true, at: (1, 2, 3, 4))",
      UiNode::row()
        .fill_width()
        .fill_height()
        .at(Rect::new(1.0, 2.0, 3.0, 4.0)),
    );
    same(
      "Grid(columns: 3, tracks: [1, 2, 1])",
      UiNode::grid(3).tracks(&[1.0, 2.0, 1.0]),
    );
    same(
      "Scroll(horizontal: true, both: true, no_bar: true)",
      UiNode::scroll().horizontal().both().no_bar(),
    );
    same(
      "Text(text: \"t\", markup: true, role: Caption, heading: true, size: 20, color: (r: 1, g: 2, b: 3), muted: true, text_align: Justify, wrap: 200, max_lines: 2, ellipsis: true, typewriter: (per_second: 40, punctuation_pause: 0.15))",
      UiNode::text("t")
        .markup()
        .role(TextRole::Caption)
        .heading()
        .size(20.0)
        .color(Color::new(1, 2, 3, 255))
        .muted()
        .text_align(TextAlign::Justify)
        .wrap(200.0)
        .max_lines(2)
        .ellipsis()
        .typewriter(TextReveal::new(40.0).with_punctuation_pause(0.15)),
    );
    same(
      "Button(label: \"b\", icon: \"i.png\", default_focus: true, nav_skip: true, focusable: false, nav_up: \"u\", nav_down: \"d\", nav_left: \"l\", nav_right: \"r\", tooltip: \"tip\", enabled: false, variant: \"danger\")",
      UiNode::button("b")
        .icon(sprite("i.png"))
        .default_focus()
        .nav_skip()
        .focusable(false)
        .nav_up("u")
        .nav_down("d")
        .nav_left("l")
        .nav_right("r")
        .tooltip("tip")
        .enabled(false)
        .variant("danger"),
    );
    same(
      "Image(sprite: \"a.png\", image_size: (16, 16), tint: \"#11223344\", filter: Nearest, uv: (0, 0, 0.5, 0.5))",
      UiNode::image(sprite("a.png"))
        .image_size(16.0, 16.0)
        .tint(Color::new(0x11, 0x22, 0x33, 0x44))
        .filter(Filter::Nearest)
        .uv([0.0, 0.0, 0.5, 0.5]),
    );
    same(
      "Slider(range: (0, 1), step: 0.1, show_value: true, label: \"Vol\", bind: \"v\")",
      UiNode::slider(0.0..=1.0, 0.0)
        .step(0.1)
        .show_value()
        .label("Vol")
        .bind("v"),
    );
    same(
      "TextInput(placeholder: \"name\", max_len: 12, password: true)",
      UiNode::text_input("")
        .placeholder("name")
        .max_len(12)
        .password(),
    );
    same(
      "ChoiceList(options: [\"a\"], default_choice: 0)",
      UiNode::choice_list(&["a"]).default_choice(0),
    );
    same(
      "Slot(icon: \"s.png\", count: 3, selected: true, filter: Linear)",
      UiNode::slot()
        .icon(sprite("s.png"))
        .count(3)
        .selected(true)
        .filter(Filter::Linear),
    );
    same(
      "Rebind(input: \"jump\", label: \"Jump\", binding_slot: 1)",
      UiNode::rebind("jump").label("Jump").binding_slot(1),
    );
    same(
      "Progress(fraction: 0.5, label: \"HP\")",
      UiNode::progress(0.5).label("HP"),
    );
    same(
      "RadioGroup(options: [\"a\", \"b\"], horizontal: true)",
      UiNode::radio_group(&["a", "b"], 0).horizontal(),
    );
    same(
      "Column(width: Fill, height: Fit)",
      UiNode::column().width(Length::Fill).height(Length::Fit),
    );
  }

  #[test]
  fn children_build_in_order() {
    same(
      "Column(gap: 8, children: [Text(\"a\"), Button(\"b\"), Separator])",
      UiNode::column()
        .gap(8.0)
        .child(UiNode::text("a"))
        .child(UiNode::button("b"))
        .child(UiNode::separator()),
    );
  }

  #[test]
  fn mistakes_name_the_fix() {
    assert_eq!(
      messages("Buton"),
      ["unknown node kind \"Buton\"; did you mean \"Button\"?"]
    );
    assert_eq!(
      messages("Button(label: \"x\", fil_width: true)"),
      ["Button has no option \"fil_width\"; did you mean \"fill_width\"?"]
    );
    assert_eq!(
      messages("Button(label: \"x\", step: 1)"),
      ["Button does not take \"step\""]
    );
    assert_eq!(
      messages("Column(gap: \"8\")"),
      ["\"gap\" takes a number, not a string"]
    );
    assert_eq!(messages("Slider(value: 1)"), ["Slider needs \"range\""]);
    assert_eq!(
      messages("Slider(1)"),
      ["Slider has no short form; name its options"]
    );
    assert_eq!(
      messages("Button(label: \"x\", children: [])"),
      ["Button does not take \"children\""]
    );
    assert!(messages("Column(anchor: Middle)")[0].contains("\"Middle\" is not one"));
    assert_eq!(
      messages("Column(children: [Buton, Button(name: \"r\"), Button(name: \"r\")])").len(),
      2,
      "a broken child and a duplicate key, both reported"
    );
    assert!(
      messages("Column(children: [Button(name: \"r\"), Button(name: \"r\")])")[0]
        .contains("two children are keyed \"r\"")
    );
    assert!(messages("Column(width: label)")[0].contains("parameters exist only inside templates"));
    assert!(messages("Button(label: \"x\", insert: 1)")[0].contains("has no option \"insert\""));
  }

  #[test]
  fn an_error_skips_its_option_and_keeps_building() {
    let (node, issues) = build("Column(gap: \"x\", width: 100)");
    assert_eq!(issues.len(), 1);
    assert_eq!(node.unwrap().layout.width, Some(Length::Px(100.0)));
  }

  #[test]
  fn positions_point_at_the_mistake() {
    let (_, issues) = build("Column(\n  gap: 4,\n  wdth: 1,\n)");
    assert_eq!(
      issues[0].pos,
      crate::core::ui::file::reader::Pos { line: 3, column: 3 }
    );
  }

  #[test]
  fn a_failing_image_is_an_issue_at_its_value() {
    struct Missing;
    impl SpriteSource for Missing {
      fn sprite(&mut self, path: &str) -> Result<SpriteHandle, String> {
        Err(format!("no file {path}"))
      }
    }
    let value = read("Image(sprite: \"gone.png\")").unwrap();
    let mut missing = Missing;
    let mut cx = BuildCx::new(&mut missing);
    build_node(&value, &mut cx);
    assert_eq!(cx.issues.len(), 1);
    assert!(
      cx.issues[0]
        .message
        .contains("cannot load the image \"gone.png\"")
    );
    assert_eq!(cx.issues[0].pos.column, 15);
  }

  #[test]
  fn nearest_suggests_only_close_names() {
    assert_eq!(
      nearest("fil_width", ["fill_width", "width"]),
      Some("fill_width")
    );
    assert_eq!(nearest("colour", ["color", "size"]), Some("color"));
    assert_eq!(nearest("zzz", ["fill_width", "width"]), None);
  }
}
