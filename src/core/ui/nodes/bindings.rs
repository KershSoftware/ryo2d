//! Named bindings: values a tree's root keeps by name, written into the
//! state components of the widgets bound to each name and formatted into
//! the labels and tooltips that show them. A list node's row is a scope of
//! its own: it keeps its own values, and its root's never reach into it.

use std::collections::HashMap;

use crate::core::ecs::kernel::{Children, Component, Entity, World};
use crate::core::ui::theme::warn_once;

use super::components::{UiChecked, UiIndex, UiLabel, UiTextValue, UiTooltip, UiValue};
use super::list::UiListRow;
use super::node::UiNode;
use super::node::UiNodeKind;
use super::spawn::scope_of;

/// A value a binding holds
#[derive(Debug, Clone, PartialEq)]
pub enum BindValue {
  Number(f64),
  Text(String),
  Bool(bool),
}

impl BindValue {
  /// The text `{name}` shows, or `{name:.N}` with `decimals`
  pub(crate) fn display(&self, decimals: Option<usize>) -> String {
    match self {
      BindValue::Number(n) => match decimals {
        Some(d) => format!("{n:.d$}"),
        None if n.is_finite() && n.fract() == 0.0 && n.abs() < 1e15 => format!("{}", *n as i64),
        None => format!("{}", *n as f32),
      },
      BindValue::Text(text) => text.clone(),
      BindValue::Bool(on) => on.to_string(),
    }
  }

  /// The same value as a widget would hold it: numbers compared as `f32`
  fn same(&self, other: &BindValue) -> bool {
    match (self, other) {
      (BindValue::Number(a), BindValue::Number(b)) => (*a as f32) == (*b as f32),
      (a, b) => a == b,
    }
  }

  /// Whether a widget holding `own` agrees with this stored value. A text
  /// input holds text whatever the name holds, so its text is compared as
  /// the stored value shows, or as the number it parses to.
  fn agrees(&self, own: &BindValue) -> bool {
    match (self, own) {
      (BindValue::Text(_), _) | (_, BindValue::Number(_) | BindValue::Bool(_)) => self.same(own),
      (stored, BindValue::Text(text)) => {
        stored.display(None) == *text
          || matches!(stored, BindValue::Number(n)
            if text.parse::<f64>().is_ok_and(|parsed| parsed as f32 == *n as f32))
      }
    }
  }

  /// What the name stores when a widget holding `own` changes: a text
  /// input's text keeps the type the name already has
  fn adopt(stored: Option<&BindValue>, own: BindValue) -> BindValue {
    let BindValue::Text(text) = &own else {
      return own;
    };
    match stored {
      Some(BindValue::Number(_)) => match text.parse::<f64>() {
        Ok(n) if n.is_finite() => BindValue::Number(n),
        _ => own,
      },
      Some(BindValue::Bool(_)) => match text.as_str() {
        "true" => BindValue::Bool(true),
        "false" => BindValue::Bool(false),
        _ => own,
      },
      _ => own,
    }
  }

  fn type_name(&self) -> &'static str {
    match self {
      BindValue::Number(_) => "number",
      BindValue::Text(_) => "text",
      BindValue::Bool(_) => "bool",
    }
  }
}

macro_rules! from_number {
  ($($ty:ty),*) => {$(
    impl From<$ty> for BindValue {
      fn from(value: $ty) -> BindValue {
        BindValue::Number(value as f64)
      }
    }
  )*};
}
from_number!(f64, i32, i64, u32, u64, usize);

impl From<f32> for BindValue {
  fn from(value: f32) -> BindValue {
    BindValue::Number(f64::from(value))
  }
}

impl From<bool> for BindValue {
  fn from(value: bool) -> BindValue {
    BindValue::Bool(value)
  }
}

impl From<&str> for BindValue {
  fn from(value: &str) -> BindValue {
    BindValue::Text(value.to_string())
  }
}

impl From<String> for BindValue {
  fn from(value: String) -> BindValue {
    BindValue::Text(value)
  }
}

macro_rules! try_float {
  ($($ty:ty),*) => {$(
    impl TryFrom<BindValue> for $ty {
      type Error = BindValue;
      fn try_from(value: BindValue) -> Result<$ty, BindValue> {
        match value {
          BindValue::Number(n) => Ok(n as $ty),
          other => Err(other),
        }
      }
    }
  )*};
}
try_float!(f32, f64);

macro_rules! try_whole {
  ($($ty:ty),*) => {$(
    impl TryFrom<BindValue> for $ty {
      type Error = BindValue;
      fn try_from(value: BindValue) -> Result<$ty, BindValue> {
        match value {
          BindValue::Number(n)
            if n.fract() == 0.0 && n >= <$ty>::MIN as f64 && n <= <$ty>::MAX as f64 =>
          {
            Ok(n as $ty)
          }
          other => Err(other),
        }
      }
    }
  )*};
}
try_whole!(i32, u32, usize);

impl TryFrom<BindValue> for bool {
  type Error = BindValue;
  fn try_from(value: BindValue) -> Result<bool, BindValue> {
    match value {
      BindValue::Bool(on) => Ok(on),
      other => Err(other),
    }
  }
}

impl TryFrom<BindValue> for String {
  type Error = BindValue;
  fn try_from(value: BindValue) -> Result<String, BindValue> {
    match value {
      BindValue::Text(text) => Ok(text),
      other => Err(other),
    }
  }
}

/// A label or tooltip with `{name}` holes, parsed once
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Pattern(Vec<Piece>);

#[derive(Debug, Clone, PartialEq)]
enum Piece {
  Text(String),
  Value {
    name: String,
    decimals: Option<usize>,
  },
}

impl Pattern {
  pub(crate) fn parse(text: &str) -> Pattern {
    let mut pieces = Vec::new();
    let mut literal = String::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
      match c {
        '{' if chars.peek() == Some(&'{') => {
          chars.next();
          literal.push('{');
        }
        '}' if chars.peek() == Some(&'}') => {
          chars.next();
          literal.push('}');
        }
        '{' => {
          let mut hole = String::new();
          let mut closed = false;
          for c in chars.by_ref() {
            if c == '}' {
              closed = true;
              break;
            }
            hole.push(c);
          }
          match parse_hole(&hole).filter(|_| closed) {
            Some((name, decimals)) => {
              if !literal.is_empty() {
                pieces.push(Piece::Text(std::mem::take(&mut literal)));
              }
              pieces.push(Piece::Value { name, decimals });
            }
            // Not a hole: keep the text as written
            None => {
              literal.push('{');
              literal.push_str(&hole);
              if closed {
                literal.push('}');
              }
            }
          }
        }
        c => literal.push(c),
      }
    }
    if !literal.is_empty() {
      pieces.push(Piece::Text(literal));
    }
    Pattern(pieces)
  }

  /// The names the pattern shows
  pub(crate) fn names(&self) -> impl Iterator<Item = &str> {
    self.0.iter().filter_map(|piece| match piece {
      Piece::Value { name, .. } => Some(name.as_str()),
      Piece::Text(_) => None,
    })
  }

  /// The text with each hole filled from `values`; an unset name shows as nothing
  pub(crate) fn format(&self, values: &HashMap<String, BindValue>) -> String {
    let mut text = String::new();
    for piece in &self.0 {
      match piece {
        Piece::Text(literal) => text.push_str(literal),
        Piece::Value { name, decimals } => {
          if let Some(value) = values.get(name) {
            text.push_str(&value.display(*decimals));
          }
        }
      }
    }
    text
  }
}

/// `name` or `name:.N`
pub(crate) fn parse_hole(hole: &str) -> Option<(String, Option<usize>)> {
  let (name, spec) = match hole.split_once(':') {
    Some((name, spec)) => (name, Some(spec)),
    None => (hole, None),
  };
  let valid = !name.is_empty()
    && name
      .chars()
      .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
  if !valid {
    return None;
  }
  let decimals = match spec {
    None => None,
    // At most nine decimals: a larger precision is not a hole
    Some(spec) => Some(
      spec
        .strip_prefix('.')?
        .parse::<usize>()
        .ok()
        .filter(|&d| d <= 9)?,
    ),
  };
  Some((name.to_string(), decimals))
}

#[cfg_attr(not(debug_assertions), allow(unused_variables))]
fn warn_unset(name: &str) {
  #[cfg(debug_assertions)]
  warn_once(format!("ui-binding-unset:{name}"), || {
    format!("no value is set for the UI binding \"{name}\"; it shows as nothing")
  });
}

/// The binding a value widget is tied to
#[derive(Component, Debug, Clone, PartialEq, Eq)]
pub(crate) struct UiBind(pub(crate) String);

/// A node's label and tooltip patterns
#[derive(Component, Debug, Clone, PartialEq, Default)]
pub(crate) struct UiPattern {
  pub(crate) label: Option<Pattern>,
  pub(crate) tooltip: Option<Pattern>,
}

/// A root's binding values, and an index of the nodes that use each name
#[derive(Component, Debug, Default)]
pub(crate) struct UiBindings {
  pub(crate) values: HashMap<String, BindValue>,
  index: Option<Index>,
  /// Whether the pattern names have been checked for unset values since the
  /// tree last changed
  checked: bool,
}

#[derive(Debug, Default)]
struct Index {
  widgets: HashMap<String, Vec<Entity>>,
  patterns: HashMap<String, Vec<Entity>>,
}

impl UiNode {
  /// Values to give names that have none when the tree is spawned
  pub(crate) fn seed(mut self, name: impl Into<String>, value: BindValue) -> UiNode {
    self.seeds.push((name.into(), value));
    self
  }
}

/// What the bound widget's component holds
fn widget_value(world: &World, node: Entity) -> Option<BindValue> {
  let entity = world.get_entity(node).ok()?;
  if let Some(v) = entity.get::<UiValue>() {
    return Some(BindValue::Number(f64::from(v.0)));
  }
  if let Some(c) = entity.get::<UiChecked>() {
    return Some(BindValue::Bool(c.0));
  }
  if let Some(i) = entity.get::<UiIndex>() {
    return Some(BindValue::Number(i.0 as f64));
  }
  entity
    .get::<UiTextValue>()
    .map(|t| BindValue::Text(t.0.clone()))
}

/// Write `value` into the bound widget's component if it differs, or warn
/// once when its type does not fit the widget; whether it wrote
fn write_widget(world: &mut World, node: Entity, name: &str, value: &BindValue) -> bool {
  let Some(kind) = world.get::<UiNodeKind>(node).map(|k| k.name()) else {
    return false;
  };
  let wrong = || {
    warn_once(format!("ui-bind-type:{name}:{kind}"), || {
      format!(
        "the UI binding \"{name}\" was given a {} but a {kind} node needs another type; \
         nothing is written",
        value.type_name()
      )
    });
  };
  let Ok(mut entity) = world.get_entity_mut(node) else {
    return false;
  };
  match (kind, value) {
    ("slider" | "progress", BindValue::Number(n)) => {
      let n = *n as f32;
      if entity.get::<UiValue>().is_some_and(|v| v.0 != n)
        && let Some(mut v) = entity.get_mut::<UiValue>()
      {
        v.0 = n;
        return true;
      }
    }
    ("checkbox" | "toggle", BindValue::Bool(on)) => {
      if entity.get::<UiChecked>().is_some_and(|c| c.0 != *on)
        && let Some(mut c) = entity.get_mut::<UiChecked>()
      {
        c.0 = *on;
        return true;
      }
    }
    ("radio_group" | "dropdown" | "tabs" | "choice_list", BindValue::Number(n))
      if n.fract() == 0.0 && *n >= 0.0 && *n <= usize::MAX as f64 =>
    {
      let i = *n as usize;
      if entity.get::<UiIndex>().is_some_and(|c| c.0 != i)
        && let Some(mut c) = entity.get_mut::<UiIndex>()
      {
        c.0 = i;
        return true;
      }
    }
    ("text_input", BindValue::Text(_) | BindValue::Number(_) | BindValue::Bool(_)) => {
      let text = value.display(None);
      if entity.get::<UiTextValue>().is_some_and(|t| t.0 != text)
        && let Some(mut t) = entity.get_mut::<UiTextValue>()
      {
        t.0 = text;
        return true;
      }
    }
    _ => wrong(),
  }
  false
}

/// Build the index of a root if it has none
fn ensure_index(world: &mut World, root: Entity) {
  if world
    .get::<UiBindings>(root)
    .is_none_or(|b| b.index.is_some())
  {
    return;
  }
  let mut index = Index::default();
  collect(world, root, &mut index);
  if let Some(mut bindings) = world.get_mut::<UiBindings>(root) {
    bindings.index = Some(index);
  }
}

fn collect(world: &World, node: Entity, index: &mut Index) {
  if let Some(bind) = world.get::<UiBind>(node) {
    index.widgets.entry(bind.0.clone()).or_default().push(node);
  }
  if let Some(pattern) = world.get::<UiPattern>(node) {
    for name in pattern
      .label
      .iter()
      .chain(pattern.tooltip.iter())
      .flat_map(Pattern::names)
    {
      let nodes = index.patterns.entry(name.to_string()).or_default();
      if nodes.last() != Some(&node) {
        nodes.push(node);
      }
    }
  }
  if let Some(kids) = world.get::<Children>(node) {
    for &child in kids.iter().filter(|&&child| !is_row(world, child)) {
      collect(world, child, index);
    }
  }
}

/// The node's root re-indexes on next use
pub(crate) fn invalidate(world: &mut World, node: Entity) {
  if let Some(root) = scope_of(world, node)
    && let Some(mut bindings) = world.get_mut::<UiBindings>(root)
  {
    bindings.index = None;
    bindings.checked = false;
  }
}

/// The widgets and pattern nodes using `name`
fn users(world: &mut World, root: Entity, name: &str) -> (Vec<Entity>, Vec<Entity>) {
  ensure_index(world, root);
  let Some(index) = world.get::<UiBindings>(root).and_then(|b| b.index.as_ref()) else {
    return (Vec::new(), Vec::new());
  };
  (
    index.widgets.get(name).cloned().unwrap_or_default(),
    index.patterns.get(name).cloned().unwrap_or_default(),
  )
}

/// Write the stored value of `name` to every widget bound to it (but
/// `except`) and format every pattern showing it. Whether anything was
/// written; each node written is pushed to `written`, if given.
fn propagate(
  world: &mut World,
  root: Entity,
  name: &str,
  except: Option<Entity>,
  mut written: Option<&mut Vec<Entity>>,
) -> bool {
  let Some(value) = world
    .get::<UiBindings>(root)
    .and_then(|b| b.values.get(name).cloned())
  else {
    return false;
  };
  let (widgets, patterns) = users(world, root, name);
  let mut wrote = false;
  let mut note = |node: Entity, did: bool| {
    if did {
      wrote = true;
      if let Some(written) = written.as_deref_mut() {
        written.push(node);
      }
    }
  };
  for widget in widgets {
    if Some(widget) != except {
      note(widget, write_widget(world, widget, name, &value));
    }
  }
  for node in patterns {
    note(node, format_node(world, root, node));
  }
  wrote
}

/// Format the node's patterns against the root's values, writing the label
/// and tooltip only when the text differs; whether it wrote
pub(crate) fn format_node(world: &mut World, root: Entity, node: Entity) -> bool {
  let (label, tooltip) = {
    let (Some(pattern), Some(bindings)) =
      (world.get::<UiPattern>(node), world.get::<UiBindings>(root))
    else {
      return false;
    };
    (
      pattern.label.as_ref().map(|p| p.format(&bindings.values)),
      pattern.tooltip.as_ref().map(|p| p.format(&bindings.values)),
    )
  };
  let Ok(mut entity) = world.get_entity_mut(node) else {
    return false;
  };
  let mut wrote = false;
  if let Some(text) = label
    && entity.get::<UiLabel>().is_some_and(|l| l.0 != text)
    && let Some(mut l) = entity.get_mut::<UiLabel>()
  {
    l.0 = text;
    wrote = true;
  }
  if let Some(text) = tooltip {
    match entity.get_mut::<UiTooltip>() {
      Some(mut t) => {
        if t.0 != text {
          t.0 = text;
          wrote = true;
        }
      }
      None => {
        entity.insert(UiTooltip(text));
        wrote = true;
      }
    }
  }
  wrote
}

/// Set `name` on the root `node` belongs to, and update everything that uses it
pub(crate) fn set(world: &mut World, node: Entity, name: &str, value: BindValue) {
  let Some(root) = scope_of(world, node) else {
    warn_once("ui-set:no-root".to_string(), || {
      "ui_set needs a node of a spawned UI tree".to_string()
    });
    return;
  };
  // An unchanged value formats and writes nothing, so a game may set it every
  // frame; a widget the game wrote directly keeps its value, and `sync_root`
  // gives it to the name
  if store(world, root, name, value) {
    propagate(world, root, name, None, None);
  }
}

/// Keep `value` as the root's value of `name`, if it differs; whether it did
fn store(world: &mut World, root: Entity, name: &str, value: BindValue) -> bool {
  if world.get::<UiBindings>(root).is_none() {
    world.entity_mut(root).insert(UiBindings::default());
  }
  let Some(mut bindings) = world.get_mut::<UiBindings>(root) else {
    return false;
  };
  match bindings.values.get_mut(name) {
    Some(stored) if stored.same(&value) => false,
    Some(stored) => {
      *stored = value;
      true
    }
    None => {
      bindings.values.insert(name.to_string(), value);
      true
    }
  }
}

/// The value of `name` on the root `node` belongs to
pub(crate) fn get(world: &World, node: Entity, name: &str) -> Option<BindValue> {
  let root = scope_of(world, node)?;
  world.get::<UiBindings>(root)?.values.get(name).cloned()
}

/// Give names that have no value the `seeds`, and propagate those
pub(crate) fn seed(world: &mut World, node: Entity, seeds: Vec<(String, BindValue)>) {
  if seeds.is_empty() {
    return;
  }
  let Some(root) = scope_of(world, node) else {
    return;
  };
  for (name, value) in seeds {
    let unset = world
      .get::<UiBindings>(root)
      .is_none_or(|b| !b.values.contains_key(&name));
    if unset {
      store(world, root, &name, value);
      propagate(world, root, &name, None, None);
    }
  }
}

/// The bound widgets and pattern nodes of a subtree, depth first
fn subtree(world: &World, node: Entity, widgets: &mut Vec<Entity>, patterns: &mut Vec<Entity>) {
  if world.get::<UiBind>(node).is_some() {
    widgets.push(node);
  }
  if world.get::<UiPattern>(node).is_some() {
    patterns.push(node);
  }
  if let Some(kids) = world.get::<Children>(node) {
    for &child in kids.iter().filter(|&&child| !is_row(world, child)) {
      subtree(world, child, widgets, patterns);
    }
  }
}

/// Whether `node` is a list's row, a binding scope of its own
fn is_row(world: &World, node: Entity) -> bool {
  world.get::<UiListRow>(node).is_some()
}

/// Join a freshly spawned subtree to its root's values
pub(crate) fn apply_subtree(world: &mut World, top: Entity, seeds: Vec<(String, BindValue)>) {
  let Some(root) = scope_of(world, top) else {
    return;
  };
  let (mut widgets, mut patterns) = (Vec::new(), Vec::new());
  subtree(world, top, &mut widgets, &mut patterns);
  if seeds.is_empty() && widgets.is_empty() && patterns.is_empty() {
    return;
  }
  if world.get::<UiBindings>(root).is_none() {
    world.entity_mut(root).insert(UiBindings::default());
  }
  invalidate(world, top);
  seed(world, top, seeds);

  let mut fresh = Vec::new();
  for widget in widgets {
    let Some(name) = world.get::<UiBind>(widget).map(|b| b.0.clone()) else {
      continue;
    };
    let stored = world
      .get::<UiBindings>(root)
      .and_then(|b| b.values.get(&name).cloned());
    match stored {
      Some(value) => {
        write_widget(world, widget, &name, &value);
      }
      None => {
        if let Some(own) = widget_value(world, widget) {
          store(world, root, &name, own);
          fresh.push(name);
        }
      }
    }
  }
  for node in patterns {
    format_node(world, root, node);
  }
  for name in fresh {
    propagate(world, root, &name, None, None);
  }
}

/// Before a root is drawn: a bound widget a game changed sets its name.
/// Whether that wrote another widget or a pattern.
pub(crate) fn sync_root(world: &mut World, root: Entity) -> bool {
  if world.get::<UiBindings>(root).is_none() {
    return false;
  }
  ensure_index(world, root);
  warn_unset_names(world, root);
  let mut changes = Vec::new();
  if let Some(bindings) = world.get::<UiBindings>(root)
    && let Some(index) = &bindings.index
  {
    for (name, widgets) in &index.widgets {
      let stored = bindings.values.get(name);
      for &widget in widgets {
        let Some(own) = widget_value(world, widget) else {
          continue;
        };
        if stored.is_none_or(|stored| !stored.agrees(&own)) {
          changes.push((name.clone(), widget, own));
          break;
        }
      }
    }
  }
  let mut wrote = false;
  for (name, widget, value) in changes {
    let value = adopted(world, root, &name, value);
    store(world, root, &name, value);
    wrote |= propagate(world, root, &name, Some(widget), None);
  }
  wrote
}

/// What `name` stores for a widget's `own` value
fn adopted(world: &World, root: Entity, name: &str, own: BindValue) -> BindValue {
  let stored = world
    .get::<UiBindings>(root)
    .and_then(|b| b.values.get(name));
  BindValue::adopt(stored, own)
}

/// Once per index build: warn about pattern names that have no value
fn warn_unset_names(world: &mut World, root: Entity) {
  let Some(mut bindings) = world.get_mut::<UiBindings>(root) else {
    return;
  };
  if bindings.checked {
    return;
  }
  bindings.checked = true;
  if let Some(index) = &bindings.index {
    for name in index.patterns.keys() {
      if !bindings.values.contains_key(name) {
        warn_unset(name);
      }
    }
  }
}

/// Right after a bound widget is drawn: its change sets its name. Each other
/// node that wrote is pushed to `written`.
pub(crate) fn widget_wrote(world: &mut World, node: Entity, written: &mut Vec<Entity>) {
  let Some(root) = scope_of(world, node) else {
    return;
  };
  let Some(name) = world.get::<UiBind>(node).map(|b| b.0.clone()) else {
    return;
  };
  let Some(own) = widget_value(world, node) else {
    return;
  };
  let same = world
    .get::<UiBindings>(root)
    .and_then(|b| b.values.get(&name))
    .is_some_and(|stored| stored.agrees(&own));
  if !same {
    let own = adopted(world, root, &name, own);
    store(world, root, &name, own);
    propagate(world, root, &name, Some(node), Some(written));
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::testing::UiHarness;
  use crate::core::ui::nodes::{UiChecked, UiIndex, UiLabel, UiTextValue, UiTooltip, UiValue};
  use crate::prelude::*;

  fn label(h: &UiHarness, node: Entity) -> String {
    h.world()
      .get::<UiLabel>(node)
      .map(|l| l.0.clone())
      .unwrap_or_default()
  }

  #[test]
  fn patterns_parse_holes_decimals_and_escapes() {
    let mut values = HashMap::new();
    values.insert("score".to_string(), BindValue::from(120));
    values.insert("volume".to_string(), BindValue::from(0.8_f32));
    values.insert("on".to_string(), BindValue::from(true));
    let format = |text: &str| Pattern::parse(text).format(&values);
    assert_eq!(format("Score: {score}"), "Score: 120");
    assert_eq!(format("{volume}"), "0.8", "an f32's shortest text");
    assert_eq!(format("{volume:.2}"), "0.80");
    assert_eq!(format("{on}"), "true");
    assert_eq!(format("{{score}}"), "{score}");
    assert_eq!(format("{not a hole}"), "{not a hole}");
    assert_eq!(format("{missing}!"), "!");
  }

  #[test]
  fn ui_set_writes_bound_widgets_and_patterns() {
    let mut h = UiHarness::new(400.0, 300.0);
    let root = h.spawn_ui(
      UiNode::column()
        .child(UiNode::text("Score: {score}").bound().name("score"))
        .child(UiNode::slider(0.0..=1.0, 0.5).bind("volume").name("slider"))
        .child(UiNode::checkbox("Mute", false).bind("mute").name("mute"))
        .child(
          UiNode::dropdown(&["A", "B", "C"], 0)
            .bind("pick")
            .name("pick"),
        )
        .child(UiNode::text_input("").bind("who").name("who")),
    );
    h.ui_set(root, "score", 120);
    h.ui_set(root, "volume", 0.25_f32);
    h.ui_set(root, "mute", true);
    h.ui_set(root, "pick", 2_usize);
    h.ui_set(root, "who", "Kai");
    let find = |h: &UiHarness, name| h.find_ui(name).unwrap();
    assert_eq!(label(&h, find(&h, "score")), "Score: 120");
    assert_eq!(
      h.world().get::<UiValue>(find(&h, "slider")).unwrap().0,
      0.25
    );
    assert!(h.world().get::<UiChecked>(find(&h, "mute")).unwrap().0);
    assert_eq!(h.world().get::<UiIndex>(find(&h, "pick")).unwrap().0, 2);
    assert_eq!(
      h.world().get::<UiTextValue>(find(&h, "who")).unwrap().0,
      "Kai"
    );
    assert_eq!(h.ui_get::<u32>(root, "score"), Some(120));
    assert_eq!(h.ui_get::<String>(root, "who"), Some("Kai".to_string()));
  }

  #[test]
  fn a_widget_seeds_its_name_and_a_label_follows_it_the_same_frame() {
    let mut h = UiHarness::new(400.0, 300.0);
    let root = h.spawn_ui(
      UiNode::column()
        .focus_scope()
        .child(
          UiNode::slider(0.0..=1.0, 0.5)
            .step(0.1)
            .bind("volume")
            .default_focus(),
        )
        .child(UiNode::text("Volume: {volume:.1}").bound().name("shown")),
    );
    let shown = h.find_ui("shown").unwrap();
    assert_eq!(
      label(&h, shown),
      "Volume: 0.5",
      "seeded from the slider at spawn"
    );
    h.frame(|_| {});
    h.action("ui.right");
    h.frame(|_| {});
    assert_eq!(
      label(&h, shown),
      "Volume: 0.6",
      "drawn after the slider: same frame"
    );
    assert_eq!(h.ui_get::<f32>(root, "volume"), Some(0.6));
  }

  #[test]
  fn a_game_write_to_a_bound_component_reaches_the_name() {
    let mut h = UiHarness::new(400.0, 300.0);
    let root = h.spawn_ui(
      UiNode::column()
        .child(UiNode::text("{hp:.0}").bound().name("shown"))
        .child(UiNode::progress(1.0).bind("hp").name("bar").hidden()),
    );
    let bar = h.find_ui("bar").unwrap();
    h.world_mut().get_mut::<UiValue>(bar).unwrap().0 = 0.4;
    h.frame(|_| {});
    assert_eq!(
      h.ui_get::<f32>(root, "hp"),
      Some(0.4),
      "synced though hidden"
    );
    h.frame(|_| {});
    assert_eq!(label(&h, h.find_ui("shown").unwrap()), "0");
  }

  #[test]
  fn two_widgets_on_one_name_follow_each_other() {
    let mut h = UiHarness::new(400.0, 300.0);
    let root = h.spawn_ui(
      UiNode::column()
        .child(UiNode::checkbox("A", false).bind("on").name("a"))
        .child(UiNode::toggle("B", false).bind("on").name("b")),
    );
    let a = h.find_ui("a").unwrap();
    h.world_mut().get_mut::<UiChecked>(a).unwrap().0 = true;
    h.frame(|_| {});
    assert!(
      h.world()
        .get::<UiChecked>(h.find_ui("b").unwrap())
        .unwrap()
        .0
    );
    assert_eq!(h.ui_get::<bool>(root, "on"), Some(true));
  }

  #[test]
  fn values_belong_to_each_root() {
    let mut h = UiHarness::new(400.0, 300.0);
    let hud = || UiNode::column().child(UiNode::text("{score}").bound());
    let one = h.spawn_ui(hud());
    let two = h.spawn_ui(hud());
    h.ui_set(one, "score", 1);
    h.ui_set(two, "score", 2);
    assert_eq!(h.ui_get::<i32>(one, "score"), Some(1));
    assert_eq!(h.ui_get::<i32>(two, "score"), Some(2));
  }

  #[test]
  fn a_wrong_type_warns_once_and_writes_nothing() {
    let mut h = UiHarness::new(400.0, 300.0);
    let root =
      h.spawn_ui(UiNode::column().child(UiNode::slider(0.0..=1.0, 0.5).bind("level").name("s")));
    h.ui_set(root, "level", true);
    h.ui_set(root, "level", false);
    assert_eq!(
      h.world().get::<UiValue>(h.find_ui("s").unwrap()).unwrap().0,
      0.5
    );
    assert_eq!(
      crate::core::ui::test_util::warnings_containing("\"level\""),
      1
    );
  }

  #[test]
  fn seeds_fill_only_names_without_a_value_and_tooltips_are_patterns() {
    let mut h = UiHarness::new(400.0, 300.0);
    let root = h.spawn_ui(
      UiNode::column().seed("gold", BindValue::from(5)).child(
        UiNode::button("Buy")
          .tooltip("You have {gold} gold")
          .bound()
          .name("buy"),
      ),
    );
    let buy = h.find_ui("buy").unwrap();
    assert_eq!(
      h.world().get::<UiTooltip>(buy).unwrap().0,
      "You have 5 gold"
    );
    h.ui_set(root, "gold", 9);
    assert_eq!(
      h.world().get::<UiTooltip>(buy).unwrap().0,
      "You have 9 gold"
    );
  }

  #[test]
  fn a_subtree_added_later_joins_the_roots_values() {
    let mut h = UiHarness::new(400.0, 300.0);
    let root = h.spawn_ui(UiNode::column().name("list"));
    h.ui_set(root, "count", 3);
    let list = h.find_ui("list").unwrap();
    let added = crate::core::ui::nodes::spawn_ui_in(
      h.world_mut(),
      list,
      UiNode::text("{count} items").bound(),
    )
    .unwrap();
    assert_eq!(label(&h, added), "3 items");
    h.ui_set(root, "count", 4);
    assert_eq!(label(&h, added), "4 items");
  }

  #[test]
  fn bind_on_a_kind_without_a_value_warns() {
    let mut h = UiHarness::new(400.0, 300.0);
    h.spawn_ui(UiNode::column().child(UiNode::button("Go").bind("x")));
    assert_eq!(
      crate::core::ui::test_util::warnings_containing("UiNode::bind does nothing on a button node"),
      1
    );
  }

  #[test]
  fn a_large_precision_is_not_a_hole() {
    let mut values = HashMap::new();
    values.insert("hp".to_string(), BindValue::from(0.5));
    let format = |text: &str| Pattern::parse(text).format(&values);
    assert_eq!(format("{hp:.70000}"), "{hp:.70000}");
    assert_eq!(format("{hp:.9}"), "0.500000000");
  }

  #[test]
  fn a_text_input_keeps_the_names_type() {
    let mut h = UiHarness::new(400.0, 300.0);
    let root = h.spawn_ui(
      UiNode::column()
        .child(UiNode::text("{age:.1}").bound().name("shown"))
        .child(UiNode::text_input("").bind("age")),
    );
    h.ui_set(root, "age", 30);
    h.frame(|_| {});
    h.frame(|_| {});
    assert_eq!(h.ui_get::<u32>(root, "age"), Some(30));
    assert_eq!(label(&h, h.find_ui("shown").unwrap()), "30.0");
  }

  #[test]
  fn a_slider_and_a_text_input_on_one_name_settle() {
    use crate::core::ecs::kernel::Changed;
    let mut h = UiHarness::new(400.0, 300.0);
    h.spawn_ui(
      UiNode::column()
        .child(UiNode::slider(0.0..=1.0, 0.5).bind("v").name("s"))
        .child(UiNode::text_input("").bind("v").name("t"))
        .child(UiNode::text("{v}").bound().name("shown")),
    );
    for _ in 0..4 {
      h.frame(|_| {});
    }
    h.world_mut().clear_trackers();
    h.frame(|_| {});
    let w = h.world_mut();
    let mut changed = w.query_filtered::<Entity, Changed<UiValue>>();
    assert_eq!(changed.iter(w).count(), 0);
    let mut changed = w.query_filtered::<Entity, Changed<UiTextValue>>();
    assert_eq!(changed.iter(w).count(), 0);
    let mut changed = w.query_filtered::<Entity, Changed<UiLabel>>();
    assert_eq!(changed.iter(w).count(), 0);
  }

  #[test]
  fn ui_set_of_an_unchanged_value_writes_and_formats_nothing() {
    use crate::core::ecs::kernel::Changed;
    let mut h = UiHarness::new(400.0, 300.0);
    let root = h.spawn_ui(
      UiNode::column()
        .child(UiNode::text("Score {score}").bound().name("shown"))
        .child(UiNode::progress(0.5).bind("hp").name("bar")),
    );
    h.ui_set(root, "score", 3);
    h.ui_set(root, "hp", 0.5);
    let (shown, bar) = (h.find_ui("shown").unwrap(), h.find_ui("bar").unwrap());
    // Written directly, so a set that formats or writes shows
    h.world_mut().get_mut::<UiLabel>(shown).unwrap().0 = "Game wrote".into();
    h.world_mut().get_mut::<UiValue>(bar).unwrap().0 = 0.9;
    h.world_mut().clear_trackers();
    h.ui_set(root, "score", 3);
    h.ui_set(root, "hp", 0.5);
    let w = h.world_mut();
    let mut changed = w.query_filtered::<Entity, Changed<UiLabel>>();
    assert_eq!(changed.iter(w).count(), 0, "no pattern formatted");
    let mut changed = w.query_filtered::<Entity, Changed<UiValue>>();
    assert_eq!(changed.iter(w).count(), 0, "no widget written");
    // The widget the game wrote sets its name at the next sync
    h.frame(|_| {});
    assert_eq!(h.ui_get::<f32>(root, "hp"), Some(0.9));
    h.ui_set(root, "score", 4);
    assert_eq!(label(&h, shown), "Score 4", "a change formats");
  }

  #[test]
  fn text_written_to_an_input_moves_the_slider_on_its_name() {
    let mut h = UiHarness::new(400.0, 300.0);
    h.spawn_ui(
      UiNode::column()
        .child(UiNode::slider(0.0..=1.0, 0.5).bind("v").name("s"))
        .child(UiNode::text_input("").bind("v").name("t")),
    );
    let t = h.find_ui("t").unwrap();
    h.world_mut().get_mut::<UiTextValue>(t).unwrap().0 = "0.7".to_string();
    h.frame(|_| {});
    let value = h.world().get::<UiValue>(h.find_ui("s").unwrap()).unwrap().0;
    assert_eq!(value, 0.7);
  }

  #[test]
  fn an_unset_name_warns_when_first_drawn_not_at_spawn() {
    let mut h = UiHarness::new(400.0, 300.0);
    let root = h.spawn_ui(
      UiNode::column()
        .child(UiNode::text("{late}").bound())
        .child(UiNode::text("{never}").bound()),
    );
    h.ui_set(root, "late", 1);
    assert_eq!(
      crate::core::ui::test_util::warnings_containing("\"never\""),
      0
    );
    h.frame(|_| {});
    h.frame(|_| {});
    assert_eq!(
      crate::core::ui::test_util::warnings_containing("\"late\""),
      0
    );
    assert_eq!(
      crate::core::ui::test_util::warnings_containing("\"never\""),
      1
    );
  }
}
