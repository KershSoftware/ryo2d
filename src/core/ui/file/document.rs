//! A UI file: its top level, templates and imports, checked at load, and
//! the entry points that build its root or a template into a `UiNode`

use std::collections::HashMap;
use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, PoisonError, RwLock};

use crate::core::asset::AssetManager;
use crate::core::ui::UiNode;
use crate::core::ui::nodes::BindValue;

use super::build::{BuildCx, Placeholders, Scope, SpriteSource, build_node, check_template};
use super::error::{UiFileError, UiFileIssue, describe};
use super::reader::{Body, Field, Pos, Value, ValueKind, read};
use super::vocab::{kind_spec, nearest};

/// Instance ids for built trees, so a reload can tell a file's nodes from
/// those code added; 0 is a node built in code
static NEXT_INSTANCE: AtomicU64 = AtomicU64::new(1);

const TOP_FIELDS: &[&str] = &["root", "templates", "imports", "bindings"];
const TEMPLATE_FIELDS: &[&str] = &["params", "defaults", "node"];

/// A loaded UI file: a cheap, cloneable handle to its checked contents,
/// like a `SpriteHandle`.
///
/// Load one with `assets.load_ui(path)`, or check text with
/// [`UiFile::parse`]. Build its `root`, or an instance of one of its
/// templates, into a [`UiNode`] to spawn. A reload replaces the contents in
/// place, so every clone of the handle sees it.
///
/// ```
/// use ryo2d::prelude::*;
///
/// let file = UiFile::parse(
///   "ui/slots.ui.ron",
///   r#"(templates: {
///     "Item": (params: ["name", "count"], node: Button(label: "{name} x{count}")),
///   })"#,
/// )?;
/// let mut assets = AssetManager::new();
/// let args = UiArgs::new().with("name", "Sword").with("count", 3);
/// let sword = file.template("Item", &args, &mut assets)?;
/// # let _ = sword;
/// # Ok::<(), UiFileError>(())
/// ```
#[derive(Clone)]
pub struct UiFile(Arc<FileCell>);

struct FileCell {
  path: String,
  /// Text the game gave rather than a file on disk: never reloaded from disk
  embedded: bool,
  doc: RwLock<Arc<Document>>,
  generation: AtomicU64,
}

/// A file's checked contents
pub(crate) struct Document {
  /// The text it was read from, to check again when an import changes
  pub(crate) text: String,
  pub(crate) root: Option<Value>,
  /// The file's own templates; its imports hold theirs
  pub(crate) templates: HashMap<String, Arc<TemplateDef>>,
  pub(crate) imports: Vec<UiFile>,
  /// Starting values for names, from `bindings`
  pub(crate) seeds: Vec<(String, BindValue)>,
}

/// A template as its file defines it
pub(crate) struct TemplateDef {
  pub(crate) name: String,
  /// Where the file names it
  pub(crate) pos: Pos,
  /// The required parameters
  pub(crate) params: Vec<String>,
  /// The optional parameters and their values
  pub(crate) defaults: Vec<(String, Value)>,
  pub(crate) node: Value,
}

/// What a built tree came from, so a reload can build it again as the same
/// instance
pub(crate) struct TreeSource {
  pub(crate) file: UiFile,
  /// The template, or `None` for the file's root
  pub(crate) template: Option<String>,
  pub(crate) args: UiArgs,
  pub(crate) instance: u64,
}

/// Where a file's imports come from
pub(crate) trait ImportResolver {
  fn import(&mut self, path: &str) -> Result<UiFile, UiFileError>;
}

/// Refuses every import: for files read with [`UiFile::parse`]
pub(crate) struct NoImports;

impl ImportResolver for NoImports {
  fn import(&mut self, path: &str) -> Result<UiFile, UiFileError> {
    Err(UiFileError::one(
      path,
      "files read with UiFile::parse cannot import; load the file with assets.load_ui",
    ))
  }
}

/// The arguments of a template built from code.
///
/// ```
/// use ryo2d::prelude::*;
///
/// let args = UiArgs::new()
///   .with("label", "Resume")
///   .with("count", 3)
///   .with("tags", vec!["new", "rare"]);
/// # let _ = args;
/// ```
#[derive(Debug, Clone, Default)]
pub struct UiArgs {
  args: Vec<(String, UiArg)>,
}

impl UiArgs {
  pub fn new() -> UiArgs {
    UiArgs::default()
  }

  /// Give the argument `name`; giving it again replaces the value
  pub fn with(mut self, name: &str, value: impl Into<UiArg>) -> UiArgs {
    let value = value.into();
    match self.args.iter_mut().find(|(given, _)| given == name) {
      Some((_, slot)) => *slot = value,
      None => self.args.push((name.to_string(), value)),
    }
    self
  }
}

/// A template argument from code: a number, text, a bool or a list of them
#[derive(Debug, Clone, PartialEq)]
pub enum UiArg {
  Number(f64),
  Text(String),
  Bool(bool),
  List(Vec<UiArg>),
}

impl UiArg {
  /// The argument as a file value at `pos`
  fn value(&self, pos: Pos) -> Value {
    let kind = match self {
      UiArg::Number(n) => ValueKind::Num(*n),
      UiArg::Text(text) => ValueKind::Str(text.clone()),
      UiArg::Bool(on) => ValueKind::Bool(*on),
      UiArg::List(items) => ValueKind::List(items.iter().map(|item| item.value(pos)).collect()),
    };
    Value { kind, pos }
  }
}

macro_rules! arg_from_number {
  ($($ty:ty),*) => {$(
    impl From<$ty> for UiArg {
      fn from(value: $ty) -> UiArg {
        UiArg::Number(value as f64)
      }
    }
  )*};
}
arg_from_number!(f64, i32, u32, usize);

impl From<f32> for UiArg {
  fn from(value: f32) -> UiArg {
    UiArg::Number(f64::from(value))
  }
}

impl From<bool> for UiArg {
  fn from(value: bool) -> UiArg {
    UiArg::Bool(value)
  }
}

impl From<&str> for UiArg {
  fn from(value: &str) -> UiArg {
    UiArg::Text(value.to_string())
  }
}

impl From<String> for UiArg {
  fn from(value: String) -> UiArg {
    UiArg::Text(value)
  }
}

impl<T: Into<UiArg>> From<Vec<T>> for UiArg {
  fn from(items: Vec<T>) -> UiArg {
    UiArg::List(items.into_iter().map(Into::into).collect())
  }
}

impl From<&[&str]> for UiArg {
  fn from(items: &[&str]) -> UiArg {
    UiArg::List(items.iter().map(|&item| item.into()).collect())
  }
}

impl fmt::Debug for UiFile {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    f.debug_struct("UiFile")
      .field("path", &self.0.path)
      .field("generation", &self.generation())
      .finish()
  }
}

impl UiFile {
  /// Check a UI file's text without an asset manager, for tests and tools.
  /// A file read this way cannot import other files.
  pub fn parse(name: &str, text: &str) -> Result<UiFile, UiFileError> {
    UiFile::load(
      name,
      text,
      &mut NoImports,
      &mut Placeholders::default(),
      true,
    )
  }

  /// The path the file was loaded from, or the name it was parsed with
  pub fn path(&self) -> &str {
    &self.0.path
  }

  /// How many times the file's contents have been replaced by a reload
  pub fn generation(&self) -> u64 {
    self.0.generation.load(Ordering::Acquire)
  }

  /// Build the file's `root`, loading its images through `assets`.
  ///
  /// Each build is a new instance with the file's `bindings` as starting
  /// values; a missing image is an error at its value's position.
  pub fn root(&self, assets: &mut AssetManager) -> Result<UiNode, UiFileError> {
    self.build_root(assets)
  }

  /// Build an instance of the template `name` (the file's or an import's)
  /// with `args`, loading its images through `assets`.
  ///
  /// An argument is one of the template's parameters or an option of the
  /// node it builds, as in a file: `MenuButton(label: .., default_focus: true)`.
  pub fn template(
    &self,
    name: &str,
    args: &UiArgs,
    assets: &mut AssetManager,
  ) -> Result<UiNode, UiFileError> {
    self.build_template(name, args, assets)
  }

  /// Read and check `text`, and hold the result as the file at `path`
  pub(crate) fn load(
    path: &str,
    text: &str,
    imports: &mut dyn ImportResolver,
    sprites: &mut dyn SpriteSource,
    embedded: bool,
  ) -> Result<UiFile, UiFileError> {
    let doc = UiFile::check(path, text, imports, sprites)?;
    Ok(UiFile(Arc::new(FileCell {
      path: path.to_string(),
      embedded,
      doc: RwLock::new(doc),
      generation: AtomicU64::new(0),
    })))
  }

  /// Read and check `text` as the file at `path`: its top level, imports,
  /// templates (each alone, its required parameters unknown) and its root,
  /// with every mistake collected
  pub(crate) fn check(
    path: &str,
    text: &str,
    imports: &mut dyn ImportResolver,
    sprites: &mut dyn SpriteSource,
  ) -> Result<Arc<Document>, UiFileError> {
    let value = read(text).map_err(|issue| UiFileError {
      issues: vec![UiFileIssue::at(path, issue)],
    })?;
    let mut check = Check {
      path,
      issues: Vec::new(),
    };
    let top = check.top_level(&value);
    let imported = check.imports(top.imports, imports);
    let defs = check.templates(top.templates);
    check.duplicates(&defs, &imported);
    let seeds = check.bindings(top.bindings);
    let doc = Arc::new(Document {
      text: text.to_string(),
      root: top.root.cloned(),
      templates: defs
        .iter()
        .filter(|(_, usable)| *usable)
        .map(|(def, _)| (def.name.clone(), def.clone()))
        .collect(),
      imports: imported.into_iter().map(|(file, _)| file).collect(),
      seeds,
    });

    let scope = Scope {
      path: path.to_string(),
      doc: doc.clone(),
    };
    let mut cx = BuildCx::in_file(sprites, scope, 0);
    for (def, _) in &defs {
      check_template(def, &mut cx);
    }
    if let Some(root) = &doc.root {
      build_node(root, &mut cx);
    }
    let built = cx
      .issues
      .into_iter()
      .map(|issue| UiFileIssue::at(path, issue));
    check.issues.extend(built);
    UiFileError::check(check.issues)?;
    Ok(doc)
  }

  pub(crate) fn doc(&self) -> Arc<Document> {
    self
      .0
      .doc
      .read()
      .unwrap_or_else(PoisonError::into_inner)
      .clone()
  }

  /// Hold `doc` as the file's contents from now on
  pub(crate) fn replace(&self, doc: Arc<Document>) {
    *self.0.doc.write().unwrap_or_else(PoisonError::into_inner) = doc;
    self.0.generation.fetch_add(1, Ordering::AcqRel);
  }

  /// Whether both are the same file (not just equal)
  pub(crate) fn same_file(&self, other: &UiFile) -> bool {
    Arc::ptr_eq(&self.0, &other.0)
  }

  pub(crate) fn is_embedded(&self) -> bool {
    self.0.embedded
  }

  /// `root()` with any sprite source
  pub(crate) fn build_root(&self, sprites: &mut dyn SpriteSource) -> Result<UiNode, UiFileError> {
    self.build(None, &UiArgs::default(), next_instance(), sprites)
  }

  /// `template()` with any sprite source
  pub(crate) fn build_template(
    &self,
    name: &str,
    args: &UiArgs,
    sprites: &mut dyn SpriteSource,
  ) -> Result<UiNode, UiFileError> {
    self.build(Some(name), args, next_instance(), sprites)
  }

  /// Build again what `source` built, as the same instance
  pub(crate) fn build_source(
    &self,
    source: &TreeSource,
    sprites: &mut dyn SpriteSource,
  ) -> Result<UiNode, UiFileError> {
    self.build(
      source.template.as_deref(),
      &source.args,
      source.instance,
      sprites,
    )
  }

  /// Whether the file imports `other` itself (not through another file)
  pub(crate) fn imports_file(&self, other: &UiFile) -> bool {
    self
      .doc()
      .imports
      .iter()
      .any(|file| Arc::ptr_eq(&file.0, &other.0))
  }

  fn build(
    &self,
    template: Option<&str>,
    args: &UiArgs,
    instance: u64,
    sprites: &mut dyn SpriteSource,
  ) -> Result<UiNode, UiFileError> {
    let path = self.path();
    let doc = self.doc();
    let scope = Scope {
      path: path.to_string(),
      doc: doc.clone(),
    };
    let built = match template {
      None => {
        let Some(root) = &doc.root else {
          return Err(UiFileError::one(path, "the file has no root"));
        };
        let mut cx = BuildCx::in_file(sprites, scope, instance);
        let node = build_node(root, &mut cx);
        (node, cx.issues, path.to_string())
      }
      Some(name) => {
        let Some((def, defined)) = scope.find(name) else {
          let mut message = format!("no template \"{name}\" in this file or its imports");
          if let Some(close) = nearest(name, scope.names().iter().map(String::as_str)) {
            message.push_str(&format!("; did you mean \"{close}\"?"));
          }
          return Err(UiFileError::one(path, message));
        };
        // The arguments are the fields of a use at the template's position
        let fields = args
          .args
          .iter()
          .map(|(name, arg)| Field {
            name: name.clone(),
            pos: def.pos,
            value: arg.value(def.pos),
          })
          .collect();
        let used = Value {
          kind: ValueKind::Named {
            name: def.name.clone(),
            body: Body::Fields(fields),
          },
          pos: def.pos,
        };
        let mut cx = BuildCx::in_file(sprites, scope, instance);
        let node = build_node(&used, &mut cx);
        // The positions are in the text of the file that defines it
        (node, cx.issues, defined.path)
      }
    };
    let (node, issues, at) = built;
    UiFileError::check(
      issues
        .into_iter()
        .map(|issue| UiFileIssue::at(&at, issue))
        .collect(),
    )?;
    let Some(mut node) = node else {
      let what = template.unwrap_or("the root");
      return Err(UiFileError::one(path, format!("{what} builds no node")));
    };
    if template.is_none() {
      for (name, value) in &doc.seeds {
        node = node.seed(name.clone(), value.clone());
      }
    }
    node.source = Some(Arc::new(TreeSource {
      file: self.clone(),
      template: template.map(str::to_string),
      args: args.clone(),
      instance,
    }));
    Ok(node)
  }
}

fn next_instance() -> u64 {
  NEXT_INSTANCE.fetch_add(1, Ordering::Relaxed)
}

/// The top level's fields
#[derive(Default)]
struct Top<'v> {
  root: Option<&'v Value>,
  templates: Option<&'v Value>,
  imports: Option<&'v Value>,
  bindings: Option<&'v Value>,
}

/// The checks of a file's top level, with the mistakes they find
struct Check<'a> {
  path: &'a str,
  issues: Vec<UiFileIssue>,
}

impl Check<'_> {
  fn issue(&mut self, pos: Pos, message: impl Into<String>) {
    self.issues.push(UiFileIssue {
      path: self.path.to_string(),
      line: pos.line,
      column: pos.column,
      message: message.into(),
    });
  }

  /// `"x" is not one`, with the nearest of `names`
  fn not_one(&mut self, pos: Pos, what: &str, name: &str, names: &[&str]) {
    let mut message = format!("{what}; \"{name}\" is not one");
    if let Some(close) = nearest(name, names.iter().copied()) {
      message.push_str(&format!("; did you mean \"{close}\"?"));
    }
    self.issue(pos, message);
  }

  fn top_level<'v>(&mut self, value: &'v Value) -> Top<'v> {
    let mut top = Top::default();
    let fields: &[Field] = match &value.kind {
      ValueKind::Fields(fields) => fields,
      // `()` reads as an empty tuple: a file with no fields
      ValueKind::Tuple(items) if items.is_empty() => &[],
      other => {
        let message = format!(
          "a UI file is written (root: .., templates: ..), not {}",
          describe(other)
        );
        self.issue(value.pos, message);
        return top;
      }
    };
    for field in fields {
      let slot = match field.name.as_str() {
        "root" => &mut top.root,
        "templates" => &mut top.templates,
        "imports" => &mut top.imports,
        "bindings" => &mut top.bindings,
        other => {
          let what = "a UI file has root, templates, imports and bindings";
          self.not_one(field.pos, what, other, TOP_FIELDS);
          continue;
        }
      };
      *slot = Some(&field.value);
    }
    if top.root.is_none() && top.templates.is_none() {
      let start = Pos { line: 1, column: 1 };
      self.issue(start, "a UI file needs a root or templates");
    }
    top
  }

  /// The imported files, each with where it is named
  fn imports(
    &mut self,
    value: Option<&Value>,
    resolver: &mut dyn ImportResolver,
  ) -> Vec<(UiFile, Pos)> {
    let Some(value) = value else {
      return Vec::new();
    };
    let ValueKind::List(items) = &value.kind else {
      let message = format!(
        "\"imports\" takes a list of file paths, not {}",
        describe(&value.kind)
      );
      self.issue(value.pos, message);
      return Vec::new();
    };
    let mut files: Vec<(UiFile, Pos)> = Vec::new();
    let mut named: Vec<&str> = Vec::new();
    for item in items {
      let ValueKind::Str(path) = &item.kind else {
        let message = format!(
          "an import is a file path (a string), not {}",
          describe(&item.kind)
        );
        self.issue(item.pos, message);
        continue;
      };
      if named.contains(&path.as_str()) {
        self.issue(item.pos, format!("\"{path}\" is imported twice"));
        continue;
      }
      named.push(path);
      match resolver.import(path) {
        Ok(file) => files.push((file, item.pos)),
        Err(error) => {
          self.issue(item.pos, format!("cannot import \"{path}\""));
          self.issues.extend(error.issues);
        }
      }
    }
    files
  }

  /// The templates in file order, each with whether its name can be used
  fn templates(&mut self, value: Option<&Value>) -> Vec<(Arc<TemplateDef>, bool)> {
    let Some(value) = value else {
      return Vec::new();
    };
    let ValueKind::Map(entries) = &value.kind else {
      let message = format!(
        "\"templates\" takes a map from template names to templates, not {}",
        describe(&value.kind)
      );
      self.issue(value.pos, message);
      return Vec::new();
    };
    entries
      .iter()
      .map(|(name, pos, template)| {
        let usable = self.template_name(name, *pos);
        (Arc::new(self.template(name, *pos, template)), usable)
      })
      .collect()
  }

  fn template_name(&mut self, name: &str, pos: Pos) -> bool {
    let camel = name.chars().next().is_some_and(char::is_uppercase)
      && name.chars().all(|c| c.is_alphanumeric() || c == '_');
    if !camel {
      let message = format!("\"{name}\" is not a template name: template names are CamelCase");
      self.issue(pos, message);
      return false;
    }
    if kind_spec(name).is_some() {
      let message = format!("\"{name}\" is a node kind; give the template another name");
      self.issue(pos, message);
      return false;
    }
    true
  }

  fn template(&mut self, name: &str, pos: Pos, value: &Value) -> TemplateDef {
    // A template without a usable node builds nothing where it is used,
    // rather than adding a mistake at every use
    let mut def = TemplateDef {
      name: name.to_string(),
      pos,
      params: Vec::new(),
      defaults: Vec::new(),
      node: Value {
        kind: ValueKind::Any,
        pos,
      },
    };
    let fields: &[Field] = match &value.kind {
      ValueKind::Fields(fields) => fields,
      ValueKind::Tuple(items) if items.is_empty() => &[],
      other => {
        let message = format!(
          "a template is written (params: [..], defaults: {{..}}, node: ..), not {}",
          describe(other)
        );
        self.issue(value.pos, message);
        return def;
      }
    };
    let mut node = None;
    let mut defaults = Vec::new();
    for field in fields {
      match field.name.as_str() {
        "params" => def.params = self.params(&field.value),
        "defaults" => defaults = self.defaults(&field.value),
        "node" => node = Some(field.value.clone()),
        other => {
          let what = "a template has params, defaults and node";
          self.not_one(field.pos, what, other, TEMPLATE_FIELDS);
        }
      }
    }
    match node {
      Some(node) => def.node = node,
      None => self.issue(value.pos, format!("the template \"{name}\" needs a node")),
    }
    for (param, at, default) in defaults {
      if def.params.contains(&param) {
        let message = format!(
          "\"{param}\" is in params and defaults; a parameter is required or has a default"
        );
        self.issue(at, message);
        continue;
      }
      def.defaults.push((param, default));
    }
    def
  }

  fn params(&mut self, value: &Value) -> Vec<String> {
    let ValueKind::List(items) = &value.kind else {
      let message = format!(
        "\"params\" takes a list of parameter names, not {}",
        describe(&value.kind)
      );
      self.issue(value.pos, message);
      return Vec::new();
    };
    let mut params: Vec<String> = Vec::new();
    for item in items {
      let ValueKind::Str(name) = &item.kind else {
        let message = format!("a parameter name is a string, not {}", describe(&item.kind));
        self.issue(item.pos, message);
        continue;
      };
      if !self.param_name(name, item.pos) {
        continue;
      }
      if params.contains(name) {
        self.issue(item.pos, format!("\"{name}\" is listed twice"));
        continue;
      }
      params.push(name.clone());
    }
    params
  }

  /// The optional parameters, each with where it is named
  fn defaults(&mut self, value: &Value) -> Vec<(String, Pos, Value)> {
    let ValueKind::Map(entries) = &value.kind else {
      let message = format!(
        "\"defaults\" takes a map from parameter names to values, not {}",
        describe(&value.kind)
      );
      self.issue(value.pos, message);
      return Vec::new();
    };
    entries
      .iter()
      .filter(|(name, pos, _)| self.param_name(name, *pos))
      .map(|(name, pos, default)| (name.clone(), *pos, default.clone()))
      .collect()
  }

  /// A parameter is referred to by a bare lowercase name
  fn param_name(&mut self, name: &str, pos: Pos) -> bool {
    let lower = name.chars().next().is_some_and(char::is_lowercase)
      && name.chars().all(|c| c.is_alphanumeric() || c == '_');
    if !lower {
      let message = format!(
        "\"{name}\" is not a parameter name: parameter names start with a lowercase letter"
      );
      self.issue(pos, message);
    }
    lower
  }

  /// A template name defined in the file and an import, or in two imports
  fn duplicates(&mut self, defs: &[(Arc<TemplateDef>, bool)], imported: &[(UiFile, Pos)]) {
    // Each imported name, and the file that first defined it
    let mut defined: HashMap<String, String> = HashMap::new();
    for (file, pos) in imported {
      let doc = file.doc();
      let mut names: Vec<&String> = doc.templates.keys().collect();
      names.sort_unstable();
      for name in names {
        match defined.get(name) {
          Some(first) => {
            let message =
              format!("the template \"{name}\" is defined twice (here and in \"{first}\")");
            self.issue(*pos, message);
          }
          None => {
            defined.insert(name.clone(), file.path().to_string());
          }
        }
      }
    }
    for (def, usable) in defs {
      if let (true, Some(import)) = (*usable, defined.get(&def.name)) {
        let message = format!(
          "the template \"{}\" is defined twice (here and in \"{import}\")",
          def.name
        );
        self.issue(def.pos, message);
      }
    }
  }

  fn bindings(&mut self, value: Option<&Value>) -> Vec<(String, BindValue)> {
    let Some(value) = value else {
      return Vec::new();
    };
    let ValueKind::Map(entries) = &value.kind else {
      let message = format!(
        "\"bindings\" takes a map from names to starting values, not {}",
        describe(&value.kind)
      );
      self.issue(value.pos, message);
      return Vec::new();
    };
    let mut seeds = Vec::new();
    for (name, _, start) in entries {
      let seed = match &start.kind {
        ValueKind::Str(text) => BindValue::Text(text.clone()),
        ValueKind::Num(n) if n.is_finite() => BindValue::Number(*n),
        ValueKind::Num(_) => {
          self.issue(start.pos, format!("the binding \"{name}\" is too large"));
          continue;
        }
        ValueKind::Bool(on) => BindValue::Bool(*on),
        other => {
          let message = format!(
            "the binding \"{name}\" starts as a string, number or bool, not {}",
            describe(other)
          );
          self.issue(start.pos, message);
          continue;
        }
      };
      seeds.push((name.clone(), seed));
    }
    seeds
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::asset::AssetManager;
  use crate::core::ui::file::build::Placeholders;
  use crate::core::ui::nodes::fingerprint;
  use crate::prelude::*;

  /// Imports from an in-memory set of files
  struct Files(HashMap<String, String>, Vec<String>);
  impl ImportResolver for Files {
    fn import(&mut self, path: &str) -> Result<UiFile, UiFileError> {
      if self.1.iter().any(|p| p == path) {
        return Err(UiFileError::one(path, "the imports go round in a circle"));
      }
      let text = self
        .0
        .get(path)
        .cloned()
        .ok_or_else(|| UiFileError::one(path, "no such file"))?;
      self.1.push(path.to_string());
      let file = UiFile::load(path, &text, self, &mut Placeholders::default(), false);
      self.1.pop();
      file
    }
  }

  fn files(list: &[(&str, &str)]) -> Files {
    Files(
      list
        .iter()
        .map(|(p, t)| (p.to_string(), t.to_string()))
        .collect(),
      Vec::new(),
    )
  }

  fn load(text: &str, imports: &mut Files) -> Result<UiFile, UiFileError> {
    UiFile::load(
      "ui/test.ui.ron",
      text,
      imports,
      &mut Placeholders::default(),
      false,
    )
  }

  fn root(file: &UiFile) -> UiNode {
    file
      .build_root(&mut Placeholders::default())
      .expect("builds")
  }

  fn messages(result: Result<UiFile, UiFileError>) -> Vec<String> {
    result
      .expect_err("should fail")
      .issues
      .into_iter()
      .map(|i| i.message)
      .collect()
  }

  const MENU: &str = r#"(
    templates: {
      "MenuButton": (
        params: ["label", "action"],
        defaults: { "wide": true },
        node: Button(label: "[{label}]", action: action, fill_width: wide),
      ),
    },
    root: Column(children: [
      MenuButton(label: "Play", action: "play", default_focus: true),
      MenuButton(label: "Quit", action: "quit", wide: false),
    ]),
  )"#;

  #[test]
  fn a_template_expands_with_arguments_defaults_and_root_options() {
    let file = load(MENU, &mut files(&[])).unwrap();
    let built = root(&file);
    let play = &built.children[0];
    assert_eq!(
      play.state.label.as_deref(),
      Some("[Play]"),
      "{{label}} in a string"
    );
    assert_eq!(play.action.as_deref(), Some("play"));
    assert!(play.flags.default_focus, "a root option over the template");
    assert_eq!(play.template.as_deref(), Some("MenuButton"));
    assert_eq!(play.layout.width, Some(Length::Fill), "the default");
    assert_eq!(built.children[1].layout.width, None, "wide: false");
  }

  #[test]
  fn keys_of_template_instances_use_the_template_name() {
    let file = load(MENU, &mut files(&[])).unwrap();
    let built = root(&file);
    assert_eq!(
      crate::core::ui::nodes::child_keys(&built.children),
      ["MenuButton#0", "MenuButton#1"]
    );
  }

  #[test]
  fn a_string_hole_that_is_not_a_parameter_stays_a_binding() {
    let file = load(
      r#"(templates: {"Gold": (params: ["who"], node: Text("{who} has {gold}"))}, root: Gold(who: "Kai"))"#,
      &mut files(&[]),
    )
    .unwrap();
    assert_eq!(root(&file).state.label.as_deref(), Some("Kai has {gold}"));
  }

  #[test]
  fn templates_come_from_imports_and_resolve_in_their_own_file() {
    let mut imports = files(&[(
      "ui/common.ui.ron",
      r#"(templates: {
        "Inner": (params: ["t"], node: Text(t)),
        "Outer": (params: ["t"], node: Column(children: [Inner(t: t)])),
      })"#,
    )]);
    let file = load(
      r#"(imports: ["ui/common.ui.ron"], root: Outer(t: "hi"))"#,
      &mut imports,
    )
    .unwrap();
    assert_eq!(root(&file).children[0].state.label.as_deref(), Some("hi"));
  }

  #[test]
  fn template_mistakes_are_reported() {
    let none = || files(&[]);
    assert_eq!(
      messages(load("(root: Column, other: 1)", &mut none())),
      ["a UI file has root, templates, imports and bindings; \"other\" is not one"]
    );
    assert_eq!(
      messages(load("()", &mut none())),
      ["a UI file needs a root or templates"]
    );
    assert!(
      messages(load(
        r#"(templates: {"button": (node: Column)})"#,
        &mut none()
      ))[0]
        .contains("template names are CamelCase")
    );
    assert!(
      messages(load(
        r#"(templates: {"Button": (node: Column)})"#,
        &mut none()
      ))[0]
        .contains("is a node kind")
    );
    assert!(
      messages(load(
        r#"(templates: {"A": (params: ["x"], node: Text(x))}, root: A())"#,
        &mut none()
      ))[0]
        .contains("A needs \"x\"")
    );
    assert!(
      messages(load(
        r#"(templates: {"A": (params: ["x"], node: Text(x))}, root: A(x: "1", y: 2))"#,
        &mut none()
      ))[0]
        .contains("A has no parameter or option \"y\"")
    );
    assert!(
      messages(load(
        r#"(templates: {"A": (params: ["label"], node: Text(labl))})"#,
        &mut none()
      ))[0]
        .contains("did you mean \"label\"")
    );
    assert!(
      messages(load(
        r#"(templates: {"A": (node: B), "B": (node: A)})"#,
        &mut none()
      ))[0]
        .contains("uses itself")
    );
    assert!(
      messages(load(r#"(templates: {"A": (node: Buton)})"#, &mut none()))[0]
        .contains("did you mean \"Button\""),
      "bodies are checked at load"
    );
  }

  #[test]
  fn import_mistakes_are_reported() {
    let mut cycle = files(&[
      (
        "a.ui.ron",
        r#"(imports: ["b.ui.ron"], templates: {"A": (node: Column)})"#,
      ),
      (
        "b.ui.ron",
        r#"(imports: ["a.ui.ron"], templates: {"B": (node: Column)})"#,
      ),
    ]);
    let issues = messages(load(r#"(imports: ["a.ui.ron"], root: Column)"#, &mut cycle));
    assert!(issues.iter().any(|m| m.contains("circle")), "{issues:?}");
    let mut twice = files(&[("c.ui.ron", r#"(templates: {"X": (node: Column)})"#)]);
    let issues = messages(load(
      r#"(imports: ["c.ui.ron"], templates: {"X": (node: Row)}, root: X)"#,
      &mut twice,
    ));
    assert!(issues[0].contains("defined twice"), "{issues:?}");
    let issues = messages(load(
      r#"(imports: ["missing.ui.ron"], root: Column)"#,
      &mut files(&[]),
    ));
    assert!(
      issues[0].contains("cannot import \"missing.ui.ron\""),
      "{issues:?}"
    );
  }

  #[test]
  fn parse_refuses_imports_and_errors_print_with_positions() {
    let error = UiFile::parse("ui/x.ui.ron", "(imports: [\"y.ui.ron\"], root: Column)")
      .err()
      .unwrap();
    assert!(error.to_string().contains("cannot import"), "{error}");
    let error = UiFile::parse("ui/x.ui.ron", "(root: Column(\n  gpa: 1))")
      .err()
      .unwrap();
    assert_eq!(
      error.to_string(),
      "ui/x.ui.ron:2:3: Column has no option \"gpa\"; did you mean \"gap\"?"
    );
  }

  #[test]
  fn a_template_built_from_code_takes_args_and_carries_its_source() {
    let file = UiFile::parse(
      "ui/slot.ui.ron",
      r#"(templates: {"Item": (params: ["name", "count"], node: Button(label: "{name} x{count}"))})"#,
    )
    .unwrap();
    let mut assets = AssetManager::new();
    let node = file
      .template(
        "Item",
        &UiArgs::new().with("name", "Sword").with("count", 3),
        &mut assets,
      )
      .unwrap();
    assert_eq!(node.state.label.as_deref(), Some("Sword x3"));
    let source = node
      .source
      .as_ref()
      .expect("the top node carries its source");
    assert_eq!(source.template.as_deref(), Some("Item"));
    assert_ne!(node.instance, 0);
    let error = file
      .template(
        "Item",
        &UiArgs::new().with("name", "S").with("cnt", 1),
        &mut assets,
      )
      .err()
      .unwrap();
    assert!(error.to_string().contains("\"cnt\""), "{error}");
    let other = file
      .template(
        "Item",
        &UiArgs::new().with("name", "A").with("count", 1),
        &mut assets,
      )
      .unwrap();
    assert_ne!(
      other.instance, node.instance,
      "each build is its own instance"
    );
  }

  #[test]
  fn the_root_seeds_its_bindings_and_rebuilds_with_the_same_instance() {
    let file = UiFile::parse(
      "ui/hud.ui.ron",
      r#"(bindings: {"score": 0}, root: Text("{score}"))"#,
    )
    .unwrap();
    let built = root(&file);
    assert_eq!(built.seeds, [("score".to_string(), BindValue::Number(0.0))]);
    let source = built.source.clone().unwrap();
    let again = file
      .build_source(&source, &mut Placeholders::default())
      .unwrap();
    assert_eq!(again.instance, built.instance);
    assert_eq!(fingerprint(&again), fingerprint(&built));
  }

  #[test]
  fn an_image_whose_sprite_is_a_parameter_is_still_checked() {
    let issues = messages(load(
      r#"(templates: {"Pic": (params: ["src"], node: Image(sprite: src, image_sze: (1, 1)))})"#,
      &mut files(&[]),
    ));
    assert_eq!(
      issues,
      ["Image has no option \"image_sze\"; did you mean \"image_size\"?"]
    );
  }

  #[test]
  fn a_parameter_can_stand_for_a_node_written_in_the_using_file() {
    let mut imports = files(&[(
      "ui/common.ui.ron",
      r#"(templates: {
        "Labelled": (params: ["label", "widget"], node: Row(children: [Text(label), widget])),
      })"#,
    )]);
    let file = load(
      r#"(
        imports: ["ui/common.ui.ron"],
        templates: {"Volume": (node: Slider(range: (0, 1), bind: "volume"))},
        root: Column(children: [
          Labelled(label: "Music", widget: Slider(range: (0, 1))),
          Labelled(label: "Master", widget: Volume),
        ]),
      )"#,
      &mut imports,
    )
    .unwrap();
    let built = root(&file);
    let music = &built.children[0];
    assert_eq!(music.children[0].state.label.as_deref(), Some("Music"));
    assert_eq!(music.children[1].kind.name(), "slider");
    let master = &built.children[1].children[1];
    assert_eq!(
      master.template.as_deref(),
      Some("Volume"),
      "built in the file that wrote it, though an imported template placed it"
    );
    assert_eq!(master.bind.as_deref(), Some("volume"));
  }

  #[test]
  fn string_holes_format_numbers_and_leave_escapes_and_other_holes() {
    let file = load(
      r#"(templates: {"Hp": (params: ["hp", "n"], node: Text("{hp:.2}/{n} {{hp}} {hp"))}, root: Hp(hp: 2.5, n: 3))"#,
      &mut files(&[]),
    )
    .unwrap();
    assert_eq!(
      root(&file).state.label.as_deref(),
      Some("2.50/3 {{hp}} {hp")
    );
  }

  #[test]
  fn a_default_is_checked_where_its_template_places_it() {
    let issues = messages(load(
      r#"(templates: {"Gap": (defaults: {"g": "wide"}, node: Column(gap: g))})"#,
      &mut files(&[]),
    ));
    assert_eq!(issues, ["\"gap\" takes a number, not a string"]);
  }

  #[test]
  fn a_mistake_in_a_template_body_is_reported_once_however_often_it_is_used() {
    let issues = messages(load(
      r#"(templates: {"A": (node: Text(text: "a", sise: 3))}, root: Column(children: [A, A]))"#,
      &mut files(&[]),
    ));
    assert_eq!(issues.len(), 1, "{issues:?}");
  }

  #[test]
  fn templates_nested_too_deeply_are_an_error_not_a_crash() {
    let chain: Vec<String> = (0..80)
      .map(|i| format!("\"T{i}\": (node: Column(children: [T{}]))", i + 1))
      .collect();
    let text = format!(
      "(templates: {{{}, \"T80\": (node: Text(\"end\"))}}, root: T0)",
      chain.join(", ")
    );
    let issues = messages(load(&text, &mut files(&[])));
    assert!(
      issues.iter().any(|m| m.contains("nest more than 64 deep")),
      "{issues:?}"
    );
  }

  #[test]
  fn an_error_prints_one_issue_per_line_and_no_position_at_line_0() {
    let error = load(
      r#"(imports: ["missing.ui.ron"], root: Column)"#,
      &mut files(&[]),
    )
    .err()
    .unwrap();
    assert_eq!(
      error.to_string(),
      "ui/test.ui.ron:1:12: cannot import \"missing.ui.ron\"\nmissing.ui.ron: no such file"
    );
  }

  #[test]
  fn an_unknown_template_from_code_is_named_with_a_suggestion() {
    let file = UiFile::parse("ui/slot.ui.ron", r#"(templates: {"Item": (node: Slot)})"#).unwrap();
    let error = file
      .template("Itme", &UiArgs::new(), &mut AssetManager::new())
      .err()
      .unwrap();
    assert!(
      error.to_string().contains("did you mean \"Item\""),
      "{error}"
    );
  }

  #[test]
  fn args_replace_a_repeated_name_and_convert_lists() {
    let args = UiArgs::new()
      .with("a", 1)
      .with("a", "x")
      .with("b", vec![1.5_f32, 2.0])
      .with("c", &["p", "q"][..]);
    assert_eq!(
      args.args,
      [
        ("a".to_string(), UiArg::Text("x".into())),
        (
          "b".to_string(),
          UiArg::List(vec![UiArg::Number(1.5), UiArg::Number(2.0)])
        ),
        (
          "c".to_string(),
          UiArg::List(vec![UiArg::Text("p".into()), UiArg::Text("q".into())])
        ),
      ]
    );
  }

  #[test]
  fn a_template_used_inside_its_own_argument_is_not_a_cycle() {
    let file = load(
      r#"(
        templates: {"Card": (params: ["content"], node: Panel(children: [content]))},
        root: Card(content: Card(content: Text("x"))),
      )"#,
      &mut files(&[]),
    )
    .unwrap();
    let built = root(&file);
    assert_eq!(
      built.children[0].children[0].state.label.as_deref(),
      Some("x")
    );

    let mut imports = files(&[(
      "ui/common.ui.ron",
      r#"(templates: {
        "Labelled": (params: ["label", "widget"], node: Row(children: [Text(label), widget])),
      })"#,
    )]);
    let file = load(
      r#"(
        imports: ["ui/common.ui.ron"],
        templates: {"Volume": (node: Labelled(label: "v", widget: Slider(range: (0, 1))))},
        root: Labelled(label: "Master", widget: Volume),
      )"#,
      &mut imports,
    );
    assert!(file.is_ok(), "{:?}", file.err());

    let issues = messages(load(
      r#"(templates: {"A": (defaults: {"c": A}, node: Panel(children: [c]))})"#,
      &mut files(&[]),
    ));
    assert!(
      issues[0].contains("uses itself"),
      "a default is the template's own: {issues:?}"
    );
  }

  #[test]
  fn imports_are_not_transitive_even_in_arguments() {
    let mut imports = files(&[
      (
        "a.ui.ron",
        r#"(imports: ["c.ui.ron"], templates: {"A": (params: ["w"], node: Column(children: [w]))})"#,
      ),
      ("c.ui.ron", r#"(templates: {"Foo": (node: Text("foo"))})"#),
    ]);
    let error = load(
      "(imports: [\"a.ui.ron\"],\n  root: A(w: Foo))",
      &mut imports,
    )
    .expect_err("Foo is a.ui.ron's import, not this file's");
    assert_eq!(error.issues.len(), 1, "{error}");
    let issue = &error.issues[0];
    assert_eq!(
      (issue.path.as_str(), issue.line, issue.column),
      ("ui/test.ui.ron", 2, 14)
    );
    assert!(
      issue.message.starts_with("unknown node kind \"Foo\""),
      "{error}"
    );
  }

  #[test]
  fn an_imported_template_built_from_code_reports_issues_in_its_own_file() {
    let mut imports = files(&[(
      "ui/common.ui.ron",
      r#"(templates: {"Item": (params: ["name"], node: Button(label: name))})"#,
    )]);
    let file = load(
      r#"(imports: ["ui/common.ui.ron"], root: Column)"#,
      &mut imports,
    )
    .unwrap();
    let args = UiArgs::new().with("name", "S").with("cnt", 1);
    let error = file
      .build_template("Item", &args, &mut Placeholders::default())
      .expect_err("cnt is not an argument of Item");
    assert_eq!(error.issues.len(), 1, "{error}");
    assert_eq!(
      error.issues[0].path, "ui/common.ui.ron",
      "the position is in the text of the file that defines Item"
    );
    assert!(error.issues[0].message.contains("\"cnt\""), "{error}");
  }
}
