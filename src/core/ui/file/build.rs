//! Building `UiNode`s from the values the reader produced
//!
//! Mistakes are collected with their positions rather than stopping at the
//! first: building goes on past one, skipping what failed. A name that is
//! not a kind is a template: its arguments are substituted into its body,
//! which is built in the template's own file.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use crate::core::asset::SpriteHandle;
use crate::core::graphics::Sprite;
use crate::core::text::TextReveal;
use crate::core::ui::nodes::bindings::parse_hole;
use crate::core::ui::nodes::{BindValue, RowShape};
use crate::core::ui::{Color, Edges, Length, UiNode};

use super::document::{Document, TemplateDef};
use super::error::{UiFileError, UiFileIssue, describe};
use super::reader::{Body, Field, Issue, Pos, Value, ValueKind};
use super::vocab::{Ctor, Form, KINDS, OPTIONS, UNIVERSAL, kind_spec, nearest, option_spec};

/// How deep nodes may nest, templates included, so that templates passing
/// nodes to each other cannot overflow the stack
const MAX_DEPTH: usize = 64;

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

/// A file whose templates a build can use: its own and its imports'
#[derive(Clone)]
pub(crate) struct Scope {
  pub(crate) path: String,
  pub(crate) doc: Arc<Document>,
}

impl Scope {
  /// The template `name`, from the file or else its imports, and the scope
  /// its body builds in
  pub(crate) fn find(&self, name: &str) -> Option<(Arc<TemplateDef>, Scope)> {
    if let Some(def) = self.doc.templates.get(name) {
      return Some((def.clone(), self.clone()));
    }
    self.doc.imports.iter().find_map(|file| {
      let doc = file.doc();
      let def = doc.templates.get(name)?.clone();
      let path = file.path().to_string();
      Some((def, Scope { path, doc }))
    })
  }

  /// Every template name the scope can use, sorted, for suggestions
  pub(crate) fn names(&self) -> Vec<String> {
    let mut names: Vec<String> = self.doc.templates.keys().cloned().collect();
    for file in &self.doc.imports {
      names.extend(file.doc().templates.keys().cloned());
    }
    names.sort_unstable();
    names
  }
}

/// A file list's row shape (`List(row_shape: ..)`): the value as the file
/// wrote it, built for each row in the file it was written in, as that file
/// was when the list's tree was built. A reload builds the tree again, and
/// so gives its list a new one.
pub(crate) struct FileRow {
  /// The file the value is built in; none outside a file
  scope: Option<Scope>,
  /// The files a template's arguments in the value were written in
  pinned: Vec<Scope>,
  value: Value,
}

impl FileRow {
  /// The file the row shape was written in, for messages
  pub(crate) fn path(&self) -> &str {
    self.scope.as_ref().map_or("", |scope| scope.path.as_str())
  }

  /// One row's tree, its images loaded through `sprites`
  pub(crate) fn build(&self, sprites: &mut dyn SpriteSource) -> Result<UiNode, UiFileError> {
    let mut cx = BuildCx {
      scope: self.scope.clone(),
      pinned: self.pinned.clone(),
      ..BuildCx::new(sprites)
    };
    let node = build_node(&self.value, &mut cx);
    let path = self.path();
    let issues = cx
      .issues
      .into_iter()
      .map(|issue| UiFileIssue::at(path, issue));
    UiFileError::check(issues.collect())?;
    node.ok_or_else(|| UiFileError::one(path, "the list's row shape builds no node"))
  }
}

pub(crate) struct BuildCx<'a> {
  pub(crate) sprites: &'a mut dyn SpriteSource,
  pub(crate) issues: Vec<Issue>,
  /// The option or argument being read, for messages
  pub(crate) what: &'static str,
  /// The file template names resolve in; none outside a file
  pub(crate) scope: Option<Scope>,
  /// The templates being expanded, by file path and name, outermost first
  pub(crate) stack: Vec<(String, String)>,
  /// The instance id every node built is given
  pub(crate) instance: u64,
  /// The scopes `InScope` values were written in
  pinned: Vec<Scope>,
  depth: usize,
}

impl BuildCx<'_> {
  pub(crate) fn new(sprites: &mut dyn SpriteSource) -> BuildCx<'_> {
    BuildCx {
      sprites,
      issues: Vec::new(),
      what: "",
      scope: None,
      stack: Vec::new(),
      instance: 0,
      pinned: Vec::new(),
      depth: 0,
    }
  }

  /// A build in `scope`'s file, giving every node `instance`
  pub(crate) fn in_file(
    sprites: &mut dyn SpriteSource,
    scope: Scope,
    instance: u64,
  ) -> BuildCx<'_> {
    BuildCx {
      scope: Some(scope),
      instance,
      ..BuildCx::new(sprites)
    }
  }

  /// The index of the current scope among the scopes arguments were
  /// written in
  fn pin_scope(&mut self) -> Option<usize> {
    let scope = self.scope.as_ref()?;
    if let Some(index) = self
      .pinned
      .iter()
      .position(|pinned| Arc::ptr_eq(&pinned.doc, &scope.doc))
    {
      return Some(index);
    }
    let scope = scope.clone();
    self.pinned.push(scope);
    Some(self.pinned.len() - 1)
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

  /// A number as `f32`, which must stay finite: a finite `f64` such as
  /// `1e40` is not one
  fn single(&mut self, n: f64, pos: Pos) -> Option<f32> {
    let single = n as f32;
    if single.is_finite() {
      return Some(single);
    }
    let message = format!("\"{}\" is too large", self.what);
    self.issue(pos, message);
    None
  }

  pub(crate) fn number(&mut self, value: &Value) -> Option<f32> {
    if !self.readable(value) {
      return None;
    }
    match value.kind {
      ValueKind::Num(n) => self.single(n, value.pos),
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
    match &unpinned(value).kind {
      ValueKind::Num(n) => self.single(*n, value.pos).map(Length::Px),
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
      ValueKind::Num(n) => self.single(*n, value.pos).map(Edges::from),
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
    // A template parameter while the template is checked: a stand-in, so
    // the rest of its node is checked too
    if matches!(value.kind, ValueKind::Any) {
      let blank = Sprite::solid_rect(1, 1, 0, 0, 0, 0);
      return Some(SpriteHandle::new(blank, PathBuf::new()));
    }
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
        ValueKind::Num(n) => *slot = self.single(*n, item.pos)?,
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
        ValueKind::Num(n) => out.push(self.single(*n, item.pos)?),
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
      ValueKind::Num(n) => self.single(*n, value.pos).map(TextReveal::new),
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

  /// A list's row shape: a template's name (bare, or as a string) or a node.
  /// It is built once here, so its mistakes are found (and its images
  /// loaded) with the rest of the tree, then again for each row, in the file
  /// it was written in.
  pub(crate) fn row_shape(&mut self, value: &Value) -> Option<RowShape> {
    let value = match &value.kind {
      // A template parameter while the template is checked
      ValueKind::Any => return None,
      ValueKind::Str(name) => {
        if !name.chars().next().is_some_and(char::is_uppercase) {
          let message = format!("\"{name}\" is not a template name: template names are CamelCase");
          self.issue(value.pos, message);
          return None;
        }
        Value {
          kind: ValueKind::Named {
            name: name.clone(),
            body: Body::Unit,
          },
          pos: value.pos,
        }
      }
      ValueKind::Named { .. } | ValueKind::InScope { .. } => value.clone(),
      _ => return self.mismatch(value, Form::Node),
    };
    let _ = build_node(&value, self)?;
    Some(RowShape::File(Arc::new(FileRow {
      scope: self.scope.clone(),
      pinned: self.pinned.clone(),
      value,
    })))
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
    } = &unpinned(value).kind
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

/// The value an argument's wrapper holds: an enum name or `Fill` passed to
/// a template reads as itself
fn unpinned(value: &Value) -> &Value {
  match &value.kind {
    ValueKind::InScope { value, .. } => value,
    _ => value,
  }
}

/// Mark a node and everything under it as built from a file: labels and
/// tooltips are binding patterns, and every node carries the build's
/// instance (a kind such as `Confirm` builds children of its own)
fn from_file(mut node: UiNode, instance: u64) -> UiNode {
  node = node.bound();
  node.instance = instance;
  node.children = node
    .children
    .into_iter()
    .map(|child| from_file(child, instance))
    .collect();
  node
}

/// A field a node's file value gave, with where
struct Entry<'v> {
  name: &'v str,
  pos: Pos,
  value: &'v Value,
}

fn entries(fields: &[Field]) -> Vec<Entry<'_>> {
  fields
    .iter()
    .map(|field| Entry {
      name: &field.name,
      pos: field.pos,
      value: &field.value,
    })
    .collect()
}

/// Who a node's fields are given to, for messages: a kind, or a template
/// and its parameters
struct Owner<'a> {
  name: &'a str,
  params: Option<&'a [&'a str]>,
}

/// Build a node from a `Kind(..)` value, or a template's use
pub(crate) fn build_node(value: &Value, cx: &mut BuildCx<'_>) -> Option<UiNode> {
  if cx.depth >= MAX_DEPTH {
    let message = format!("nodes nest more than {MAX_DEPTH} deep here");
    cx.issue(value.pos, message);
    return None;
  }
  cx.depth += 1;
  let node = build_value(value, cx);
  cx.depth -= 1;
  node
}

fn build_value(value: &Value, cx: &mut BuildCx<'_>) -> Option<UiNode> {
  let (name, body) = match &value.kind {
    ValueKind::Any => return None,
    ValueKind::InScope {
      scope,
      stack,
      value,
    } => {
      let written = cx.pinned.get(*scope)?.clone();
      let outer = cx.scope.replace(written);
      // Only the templates being expanded where it was written: a template
      // used inside its own argument is not using itself
      let inner = cx.stack.split_off((*stack).min(cx.stack.len()));
      let node = build_node(value, cx);
      cx.stack.extend(inner);
      cx.scope = outer;
      return node;
    }
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
    if let Some((def, scope)) = cx.scope.as_ref().and_then(|scope| scope.find(name)) {
      return expand(value, body, &def, scope, cx);
    }
    let templates = cx.scope.as_ref().map(Scope::names).unwrap_or_default();
    let mut message = format!("unknown node kind \"{name}\"");
    let known = KINDS
      .iter()
      .map(|kind| kind.name)
      .chain(templates.iter().map(String::as_str));
    if let Some(close) = nearest(name, known) {
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
    Body::Fields(fields) => entries(fields),
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
  let mut node = from_file((spec.make)(&ctor, cx)?, cx.instance);

  let owner = Owner {
    name: spec.name,
    params: None,
  };
  let mut keys: Vec<String> = Vec::new();
  for entry in entries {
    if spec.args.iter().any(|arg| arg.name == entry.name) {
      continue;
    }
    node = apply_field(node, &owner, &entry, cx, &mut keys);
  }
  Some(node)
}

/// A use of a template: its arguments substituted into its body, built in
/// the template's own file, then the other fields applied as options of the
/// node that built
fn expand(
  value: &Value,
  body: &Body,
  def: &TemplateDef,
  scope: Scope,
  cx: &mut BuildCx<'_>,
) -> Option<UiNode> {
  let name = def.name.as_str();
  let cycle = cx
    .stack
    .iter()
    .position(|(path, used)| *path == scope.path && used == name);
  if let Some(start) = cycle {
    let chain: Vec<&str> = cx.stack[start..]
      .iter()
      .map(|(_, used)| used.as_str())
      .chain([name])
      .collect();
    let message = format!(
      "the template \"{name}\" uses itself ({})",
      chain.join(" -> ")
    );
    cx.issue(value.pos, message);
    return None;
  }
  let entries = match body {
    Body::Unit => Vec::new(),
    Body::Single(inner) => {
      let message = format!("{name} has no short form; name its parameters");
      cx.issue(inner.pos, message);
      return None;
    }
    Body::Fields(fields) => entries(fields),
  };
  let params: Vec<&str> = def
    .params
    .iter()
    .chain(def.defaults.iter().map(|(param, _)| param))
    .map(String::as_str)
    .collect();

  // Arguments are built where they were written; defaults are the
  // template's own and are built in its body
  let written = cx.pin_scope().map(|index| (index, cx.stack.len()));
  let mut args: HashMap<String, Value> = def.defaults.iter().cloned().collect();
  let mut options = Vec::new();
  for entry in entries {
    if !params.contains(&entry.name) {
      options.push(entry);
      continue;
    }
    let mut arg = entry.value.clone();
    if let Some((index, stack)) = written {
      pin(&mut arg, index, stack);
    }
    args.insert(entry.name.to_string(), arg);
  }
  for param in &def.params {
    if !args.contains_key(param) {
      cx.issue(value.pos, format!("{name} needs \"{param}\""));
      // Built on with the value unknown, so the rest is still checked
      let unknown = Value {
        kind: ValueKind::Any,
        pos: value.pos,
      };
      args.insert(param.clone(), unknown);
    }
  }

  cx.stack.push((scope.path.clone(), def.name.clone()));
  let outer = cx.scope.replace(scope);
  let body = substitute(&def.node, &args, name, cx);
  let built = build_node(&body, cx);
  cx.scope = outer;
  cx.stack.pop();

  let mut node = built?;
  let mut keys: Vec<String> = node
    .children
    .iter()
    .filter_map(|child| child.key.clone().or_else(|| child.name.clone()))
    .collect();
  let owner = Owner {
    name,
    params: Some(&params),
  };
  for entry in options {
    node = apply_field(node, &owner, &entry, cx, &mut keys);
  }
  node.template = Some(def.name.clone());
  Some(node)
}

/// Check a template's body as it stands alone: each required parameter
/// unknown, each optional one at its default
pub(crate) fn check_template(def: &TemplateDef, cx: &mut BuildCx<'_>) {
  let Some(path) = cx.scope.as_ref().map(|scope| scope.path.clone()) else {
    return;
  };
  cx.stack.push((path, def.name.clone()));
  let mut args: HashMap<String, Value> = def.defaults.iter().cloned().collect();
  for param in &def.params {
    let unknown = Value {
      kind: ValueKind::Any,
      pos: def.pos,
    };
    args.insert(param.clone(), unknown);
  }
  let body = substitute(&def.node, &args, &def.name, cx);
  build_node(&body, cx);
  cx.stack.pop();
}

/// Wrap the names in an argument that are not kinds (template uses, and
/// unknown names) so they are built where the argument was written,
/// wherever the template places them: in that file, which sees only its own
/// templates and its imports', with that file's templates being expanded
fn pin(value: &mut Value, scope: usize, stack: usize) {
  match &mut value.kind {
    ValueKind::Named { name, body } => {
      let parameter = matches!(body, Body::Unit) && is_lowercase(name);
      if !parameter && kind_spec(name).is_none() {
        let pos = value.pos;
        let blank = Value {
          kind: ValueKind::Any,
          pos,
        };
        let inner = std::mem::replace(value, blank);
        value.kind = ValueKind::InScope {
          scope,
          stack,
          value: Box::new(inner),
        };
        return;
      }
      match body {
        Body::Unit => {}
        Body::Single(inner) => pin(inner, scope, stack),
        Body::Fields(fields) => {
          for field in fields {
            pin(&mut field.value, scope, stack);
          }
        }
      }
    }
    ValueKind::List(items) | ValueKind::Tuple(items) => {
      for item in items {
        pin(item, scope, stack);
      }
    }
    ValueKind::Map(entries) => {
      for (_, _, item) in entries {
        pin(item, scope, stack);
      }
    }
    ValueKind::Fields(fields) => {
      for field in fields {
        pin(&mut field.value, scope, stack);
      }
    }
    _ => {}
  }
}

/// A template body with its parameters replaced by the arguments: a bare
/// lowercase name is a parameter, and so is a `{param}` hole in a string.
/// The arguments themselves are not substituted again.
pub(crate) fn substitute(
  value: &Value,
  args: &HashMap<String, Value>,
  template: &str,
  cx: &mut BuildCx<'_>,
) -> Value {
  let kind = match &value.kind {
    ValueKind::Named {
      name,
      body: Body::Unit,
    } if is_lowercase(name) => {
      if let Some(arg) = args.get(name) {
        return arg.clone();
      }
      let mut message = format!("\"{name}\" is not a parameter of {template}");
      let mut params: Vec<&str> = args.keys().map(String::as_str).collect();
      params.sort_unstable();
      if let Some(close) = nearest(name, params) {
        message.push_str(&format!("; did you mean \"{close}\"?"));
      }
      cx.issue(value.pos, message);
      ValueKind::Any
    }
    ValueKind::Str(text) => ValueKind::Str(fill_holes(text, value.pos, args, cx)),
    ValueKind::List(items) => ValueKind::List(
      items
        .iter()
        .map(|item| substitute(item, args, template, cx))
        .collect(),
    ),
    ValueKind::Tuple(items) => ValueKind::Tuple(
      items
        .iter()
        .map(|item| substitute(item, args, template, cx))
        .collect(),
    ),
    ValueKind::Map(entries) => ValueKind::Map(
      entries
        .iter()
        .map(|(key, pos, item)| (key.clone(), *pos, substitute(item, args, template, cx)))
        .collect(),
    ),
    ValueKind::Fields(fields) => ValueKind::Fields(substitute_fields(fields, args, template, cx)),
    ValueKind::Named { name, body } => ValueKind::Named {
      name: name.clone(),
      body: match body {
        Body::Unit => Body::Unit,
        Body::Single(inner) => Body::Single(Box::new(substitute(inner, args, template, cx))),
        Body::Fields(fields) => Body::Fields(substitute_fields(fields, args, template, cx)),
      },
    },
    other => other.clone(),
  };
  Value {
    kind,
    pos: value.pos,
  }
}

fn substitute_fields(
  fields: &[Field],
  args: &HashMap<String, Value>,
  template: &str,
  cx: &mut BuildCx<'_>,
) -> Vec<Field> {
  fields
    .iter()
    .map(|field| Field {
      name: field.name.clone(),
      pos: field.pos,
      value: substitute(&field.value, args, template, cx),
    })
    .collect()
}

/// A string with each `{param}` and `{param:.N}` hole replaced by the
/// argument's text; `{{`, `}}` and other holes stay for bindings, and so
/// does a hole whose argument is unknown
fn fill_holes(text: &str, pos: Pos, args: &HashMap<String, Value>, cx: &mut BuildCx<'_>) -> String {
  if !text.contains('{') {
    return text.to_string();
  }
  let mut out = String::with_capacity(text.len());
  let mut chars = text.chars().peekable();
  while let Some(c) = chars.next() {
    match c {
      '{' | '}' if chars.peek() == Some(&c) => {
        chars.next();
        out.push(c);
        out.push(c);
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
        let filled = parse_hole(&hole)
          .filter(|_| closed)
          .and_then(|(name, decimals)| {
            let shown = match &args.get(&name)?.kind {
              ValueKind::Str(text) => BindValue::Text(text.clone()),
              ValueKind::Num(n) => BindValue::Number(*n),
              ValueKind::Bool(on) => BindValue::Bool(*on),
              ValueKind::Any => return None,
              other => {
                let message = format!(
                  "the parameter \"{name}\" is {}, which text cannot show",
                  describe(other)
                );
                cx.issue(pos, message);
                return None;
              }
            };
            Some(shown.display(decimals))
          });
        match filled {
          Some(shown) => out.push_str(&shown),
          None => {
            out.push('{');
            out.push_str(&hole);
            if closed {
              out.push('}');
            }
          }
        }
      }
      c => out.push(c),
    }
  }
  out
}

fn apply_field(
  node: UiNode,
  owner: &Owner<'_>,
  entry: &Entry<'_>,
  cx: &mut BuildCx<'_>,
  keys: &mut Vec<String>,
) -> UiNode {
  let name = entry.name;
  let who = owner.name;
  if name == "children" {
    if !node.kind.takes("child") {
      cx.issue(entry.pos, format!("{who} does not take \"children\""));
      return node;
    }
    return add_children(node, entry, cx, keys);
  }
  let Some(option) = option_spec(name) else {
    let mut message = match owner.params {
      Some(_) => format!("{who} has no parameter or option \"{name}\""),
      None => format!("{who} has no option \"{name}\""),
    };
    let takes = owner
      .params
      .unwrap_or_default()
      .iter()
      .copied()
      .chain(
        OPTIONS
          .iter()
          .map(|option| option.name)
          .filter(|option| UNIVERSAL.contains(option) || node.kind.takes(option)),
      )
      .chain(node.kind.takes("child").then_some("children"));
    if let Some(close) = nearest(name, takes) {
      message.push_str(&format!("; did you mean \"{close}\"?"));
    }
    cx.issue(entry.pos, message);
    return node;
  };
  if !UNIVERSAL.contains(&name) && !node.kind.takes(name) {
    cx.issue(entry.pos, format!("{who} does not take \"{name}\""));
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
  use crate::core::ui::nodes::{RowShape, fingerprint};
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
    same(
      "List(count: 3, row_height: 20, row_shape: Row)",
      UiNode::list(3, 20.0),
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
  fn every_node_a_kind_builds_carries_the_build_instance() {
    let value = read("Confirm(title: \"Quit?\", message: \"Sure?\")").unwrap();
    let mut sprites = Placeholders::default();
    let mut cx = BuildCx::new(&mut sprites);
    cx.instance = 7;
    let node = build_node(&value, &mut cx).expect("builds");
    fn instances(node: &UiNode, out: &mut Vec<u64>) {
      out.push(node.instance);
      for child in &node.children {
        instances(child, out);
      }
    }
    let mut all = Vec::new();
    instances(&node, &mut all);
    assert!(
      all.len() > 3,
      "the dialog builds children of its own: {all:?}"
    );
    assert!(all.iter().all(|&instance| instance == 7), "{all:?}");
  }

  #[test]
  fn a_number_too_large_for_f32_is_an_issue_at_the_value() {
    let (node, issues) = build("Column(width: 1e40)");
    assert_eq!(issues.len(), 1);
    assert_eq!(issues[0].message, "\"width\" is too large");
    assert_eq!(issues[0].pos.column, 15);
    assert_eq!(node.unwrap().layout.width, None, "no infinite width");
    assert_eq!(
      messages("Column(offset: (1, -1e39))"),
      ["\"offset\" is too large"]
    );
    assert_eq!(
      messages("Grid(columns: 2, tracks: [1, 1e300])"),
      ["\"tracks\" is too large"]
    );
  }

  #[test]
  fn a_list_builds_with_its_row_and_needs_both() {
    let (node, issues) =
      build("List(count: 5, row_height: 20, gap: 2, columns: 2, row_shape: Text(\"x\"))");
    assert!(issues.is_empty(), "{issues:?}");
    let node = node.unwrap();
    assert_eq!(node.kind.name(), "list");
    assert!(node.row_shape.is_some());
    assert_eq!(
      messages("List(count: 5, row_shape: Text(\"x\"))"),
      ["List needs \"row_height\""]
    );
    assert_eq!(
      messages("List(count: 5, row_height: 20)"),
      ["List needs \"row_shape\""]
    );
  }

  #[test]
  fn a_list_s_options_apply_the_methods_of_their_names() {
    same(
      "List(row_height: 28, row_shape: Row)",
      UiNode::list(0, 28.0),
    );
    same(
      "List(count: 10000, row_height: 28, row_shape: Row, columns: 4, overscan: 3, gap: 2, padding: 4, no_bar: true, fill: true, name: \"board\", embedded: true)",
      UiNode::list(10_000, 28.0)
        .columns(4)
        .overscan(3)
        .gap(2.0)
        .padding(4.0)
        .no_bar()
        .fill()
        .name("board")
        .embedded(),
    );
    assert_eq!(
      messages("List(row_height: 20, row_shape: 3)"),
      ["\"row_shape\" takes a template name or a node such as Row(..), not a number"]
    );
    assert_eq!(
      messages("List(row_height: 20, row_shape: \"leader\")"),
      ["\"leader\" is not a template name: template names are CamelCase"]
    );
    assert_eq!(
      messages("List(row_height: 20, row_shape: Row(gpa: 1))"),
      ["Row has no option \"gpa\"; did you mean \"gap\"?"],
      "the row is checked where it is written"
    );
    assert_eq!(
      messages("Column(columns: 2)"),
      ["Column does not take \"columns\""]
    );
  }

  #[test]
  fn a_list_s_row_shape_names_a_template_bare_or_as_a_string_or_is_a_node() {
    let templates = r#""Leader": (node: Row(children: [Text("{rank}")])), "Needy": (params: ["x"], node: Text(x))"#;
    let file = |row: &str| {
      UiFile::parse(
        "ui/board.ui.ron",
        &format!("(templates: {{{templates}}}, root: List(row_height: 20, row_shape: {row}))"),
      )
    };
    for row in ["Leader", "\"Leader\"", "Column(children: [Leader])"] {
      let file = file(row).unwrap_or_else(|e| panic!("{row}: {e}"));
      let list = file.build_root(&mut Placeholders::default()).unwrap();
      let Some(RowShape::File(shape)) = &list.row_shape else {
        panic!("{row}: a file row shape")
      };
      let mut built = shape.build(&mut Placeholders::default()).unwrap();
      while built.kind.name() == "column" {
        built = built.children.remove(0);
      }
      assert_eq!(built.template.as_deref(), Some("Leader"), "{row}");
      assert_eq!(
        built.children[0].state.label.as_deref(),
        Some("{rank}"),
        "{row}: a binding pattern"
      );
      assert!(built.children[0].bound, "{row}");
    }
    let messages = |row: &str| -> Vec<String> {
      file(row)
        .err()
        .unwrap()
        .issues
        .into_iter()
        .map(|i| i.message)
        .collect()
    };
    assert_eq!(
      messages("Leadr"),
      ["unknown node kind \"Leadr\"; did you mean \"Leader\"?"]
    );
    assert_eq!(
      messages("Needy"),
      ["Needy needs \"x\""],
      "a row is built with no arguments"
    );
  }

  #[test]
  fn nearest_suggests_only_close_names() {
    assert_eq!(
      nearest("fil_width", ["fill_width", "width"]),
      Some("fill_width")
    );
    assert_eq!(nearest("colour", ["color", "size"]), Some("color"));
    assert_eq!(
      nearest("gpa", ["grow", "gap"]),
      Some("gap"),
      "a swap is one edit"
    );
    assert_eq!(nearest("zzz", ["fill_width", "width"]), None);
  }
}
