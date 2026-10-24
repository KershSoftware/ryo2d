//! A UI file: its top level, templates and imports, checked at load, and
//! the entry points that build its root or a template into a `UiNode`

use std::borrow::Cow;
use std::collections::HashMap;
use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};

use crate::core::asset::path::normalise;
use crate::core::asset::{
  Asset, AssetError, AssetId, AssetPath, AssetServer, Assets, AssetsView, Handle, LoadContext,
};
use crate::core::ecs::kernel::World;
use crate::core::graphics::Image;
use crate::core::ui::UiNode;
use crate::core::ui::nodes::BindValue;

use super::build::{BuildCx, Reach, Scope, build_node, check_template};
use super::error::{UiFileError, UiFileIssue, describe, issues_of, reason};
use super::reader::{Body, Field, Pos, Value, ValueKind, read};
use super::vocab::{kind_spec, nearest};

/// Instance ids for built trees, so a reload can tell a file's nodes from
/// those code added; 0 is a node built in code
static NEXT_INSTANCE: AtomicU64 = AtomicU64::new(1);

/// How many files `version_of` reads through a file's imports at most:
/// imports cannot go round (a load refuses a cycle), so this only bounds
/// the cost of files many others share
const MAX_IMPORT_VISITS: usize = 256;

const TOP_FIELDS: &[&str] = &["root", "templates", "imports", "bindings"];
const TEMPLATE_FIELDS: &[&str] = &["params", "defaults", "node"];

/// A UI file (`.ui.ron`): an asset, checked when it loads.
///
/// Load one through the registry (`ctx.assets().load_now::<UiFile>(path)`,
/// a path under `assets/`), or check text the game holds with
/// [`UiFile::parse`] and add it (`ctx.assets().add(..)`). Build its `root`,
/// or an instance of one of its templates, from the handle, into a
/// [`UiNode`] to spawn. Each tree holds the file's handle, and a reload
/// (the file saved while the game runs, or one it imports) brings the trees
/// in line.
///
/// ```
/// use ryo2d::prelude::*;
///
/// fn sword(ctx: &mut SceneContext<'_>) -> Result<UiNode, UiFileError> {
///   let file = UiFile::parse(
///     "ui/slots.ui.ron",
///     r#"(templates: {
///       "Item": (params: ["name", "count"], node: Button(label: "{name} x{count}")),
///     })"#,
///   )?;
///   let file = ctx.assets().add(file);
///   let args = UiArgs::new().with("name", "Sword").with("count", 3);
///   file.template("Item", &args, &mut ctx.assets())
/// }
/// ```
pub struct UiFile {
  path: String,
  pub(crate) doc: Arc<Document>,
  /// The images the file names: held, so they live while it does
  #[allow(dead_code)]
  images: Vec<Handle<Image>>,
  /// Each of those images' path, and where the file names it
  image_paths: Vec<(String, Pos)>,
  /// The newest version among its imports when the load checked it whole
  /// (under `load_now`, or with no imports): until one changes, `check` has
  /// nothing to build again
  checked: Option<u64>,
}

impl Asset for UiFile {}

/// A file's checked contents
pub(crate) struct Document {
  pub(crate) root: Option<Value>,
  /// The file's own templates; its imports hold theirs
  pub(crate) templates: HashMap<String, Arc<TemplateDef>>,
  /// Every template the file defines, in file order, so a check reports in
  /// that order
  order: Vec<Arc<TemplateDef>>,
  /// The imported files, in the order named: set when the file loads, or,
  /// for text read with `parse`, when it is first built
  imports: OnceLock<Vec<Handle<UiFile>>>,
  /// The path of each import, and where the file names it
  import_paths: Vec<(String, Pos)>,
  /// For text read with `parse` that imports files: how its check against
  /// them came out at its first build, given again at every later one
  parsed_check: OnceLock<Result<(), UiFileError>>,
  /// Starting values for names, from `bindings`
  pub(crate) seeds: Vec<(String, BindValue)>,
}

impl Document {
  /// The imported files; none until they are known
  pub(crate) fn imports(&self) -> &[Handle<UiFile>] {
    self.imports.get().map_or(&[], Vec::as_slice)
  }
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
  /// The file, held while the tree lives
  pub(crate) file: Handle<UiFile>,
  /// The template, or `None` for the file's root
  pub(crate) template: Option<String>,
  pub(crate) args: UiArgs,
  pub(crate) instance: u64,
  /// The file's version when this tree was built, its imports' counted
  /// (`version_of`)
  pub(crate) version: u64,
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
  /// No arguments: a template with no required parameters builds from it
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
  /// A number, as a file's `3` or `0.5`
  Number(f64),
  /// Text, as a file's `"Resume"`
  Text(String),
  /// `true` or `false`
  Bool(bool),
  /// A list, as a file's `["A", "B"]`
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
arg_from_number!(f64, i32, i64, u32, u64, usize);

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
    f.debug_struct("UiFile").field("path", &self.path).finish()
  }
}

impl UiFile {
  /// Read and check text the game holds (for instance `include_str!`), as
  /// the file `name`; add it to the registry (`ctx.assets().add(..)`) to
  /// build from it. Every mistake comes back together, each with its line
  /// and column.
  ///
  /// It is never reloaded. Its imports are asset paths, loaded at once when
  /// it is first built (they must be files the registry can load), and it
  /// is checked against them then; its images load when it is built.
  pub fn parse(name: &str, text: &str) -> Result<UiFile, UiFileError> {
    let read = read_file(name, text)?;
    let mut issues = read.issues;
    // Checked against its imports once they load, at its first build
    let deferred = !read.imports.is_empty();
    let imports = if deferred {
      OnceLock::new()
    } else {
      OnceLock::from(Vec::new())
    };
    let doc = Arc::new(Document::new(read.parts, read.imports, imports));
    if !deferred {
      issues.extend(check_built(name, &doc, Reach::check(None, None)));
    }
    UiFileError::check(issues)?;
    Ok(UiFile {
      path: name.to_string(),
      doc,
      images: Vec::new(),
      image_paths: Vec::new(),
      checked: None,
    })
  }

  /// The path the file was loaded from, or the name it was parsed with
  pub fn path(&self) -> &str {
    &self.path
  }

  /// Read `text` as the file at `path` and check its top level; load its
  /// imports and the images it names through `cx`, as its dependencies.
  /// While `load_now` runs, the imports are loaded by now, so the file's
  /// templates and root are checked here too, and every mistake comes back
  /// together; on the workers that waits for `check`
  pub(crate) fn load(
    path: &AssetPath,
    text: &str,
    cx: &mut LoadContext<'_>,
  ) -> Result<UiFile, AssetError> {
    let name = path.as_str();
    let read = read_file(name, text).map_err(|error| AssetError::decode(path, error))?;
    let mut issues = read.issues;
    let sees_imports = cx.world().is_some() || read.imports.is_empty();
    let mut imports = Vec::with_capacity(read.imports.len());
    let mut loaded = Vec::with_capacity(read.imports.len());
    for (import, pos) in read.imports {
      match cx.load::<UiFile>(&import) {
        Ok(file) => {
          imports.push(file);
          loaded.push((import, pos));
        }
        // As it is, so the load that started it names it
        Err(error @ AssetError::Cycle { .. }) => return Err(error),
        Err(error) => {
          issues.push(issue_at(name, pos, format!("cannot import \"{import}\"")));
          issues.extend(issues_of(&error, &import));
        }
      }
    }
    let doc = Arc::new(Document::new(read.parts, loaded, OnceLock::from(imports)));
    // Built once to find the images it names. With its imports in the world
    // (or none to read) that is the whole check, so its mistakes count now
    let mut named = Vec::new();
    let built = check_built(name, &doc, Reach::check(cx.world(), Some(&mut named)));
    let checked = sees_imports.then(|| imports_version(&doc, cx.world()));
    if sees_imports {
      issues.extend(built);
    }
    let mut images = Vec::with_capacity(named.len());
    let mut image_paths = Vec::with_capacity(named.len());
    for (image, pos) in named {
      match cx.load::<Image>(&image) {
        Ok(handle) => {
          images.push(handle);
          image_paths.push((image, pos));
        }
        Err(error) => {
          let message = format!("cannot load the image \"{image}\": {}", reason(&error));
          issues.push(issue_at(name, pos, message));
        }
      }
    }
    UiFileError::check(issues).map_err(|error| AssetError::decode(path, error))?;
    Ok(UiFile {
      path: name.to_string(),
      doc,
      images,
      image_paths,
      checked,
    })
  }

  /// The mistake a dependency that did not load (`error`) makes in the file,
  /// at the position the file names it, as the load reports it at once;
  /// none if the file does not name it
  pub(crate) fn dependency_issues(
    &self,
    dependency: &AssetPath,
    error: &AssetError,
  ) -> Option<UiFileError> {
    let names = |raw: &str| normalise(raw).is_ok_and(|path| path == dependency.as_str());
    if let Some((import, pos)) = self.doc.import_paths.iter().find(|(raw, _)| names(raw)) {
      let mut issues = vec![issue_at(
        &self.path,
        *pos,
        format!("cannot import \"{import}\""),
      )];
      issues.extend(issues_of(error, import));
      return Some(UiFileError { issues });
    }
    let (image, pos) = self.image_paths.iter().find(|(raw, _)| names(raw))?;
    let message = format!("cannot load the image \"{image}\": {}", reason(error));
    Some(UiFileError {
      issues: vec![issue_at(&self.path, *pos, message)],
    })
  }

  /// Build the root and every template once against the imports' documents
  /// in `world`, as the file's loader checks it. A file the load checked
  /// whole is not built again while its imports are as they were then
  pub(crate) fn check(&self, world: &World) -> Result<(), AssetError> {
    if self.checked.is_some() && self.checked == Some(imports_version(&self.doc, Some(world))) {
      return Ok(());
    }
    let issues = check_built(&self.path, &self.doc, Reach::check(Some(world), None));
    UiFileError::check(issues).map_err(|error| {
      AssetError::decode(&AssetPath::from_normal(Cow::Borrowed(&self.path)), error)
    })
  }
}

impl Handle<UiFile> {
  /// Build the file's `root`, loading its images at once through the asset
  /// registry (`&mut ctx.assets()`); their paths are asset paths.
  ///
  /// Each build is a new instance with the file's `bindings` as starting
  /// values; a missing image is an error at its value's position. A file
  /// still loading on the workers builds nothing yet: an error says so.
  pub fn root(&self, assets: &mut AssetsView<'_>) -> Result<UiNode, UiFileError> {
    build(
      self,
      None,
      &UiArgs::default(),
      next_instance(),
      assets.world_mut(),
    )
  }

  /// Build an instance of the template `name` (the file's or an import's)
  /// with `args`, loading its images at once through the asset registry
  /// (`&mut ctx.assets()`).
  ///
  /// An argument is one of the template's parameters or an option of the
  /// node it builds, as in a file: `MenuButton(label: .., default_focus: true)`.
  pub fn template(
    &self,
    name: &str,
    args: &UiArgs,
    assets: &mut AssetsView<'_>,
  ) -> Result<UiNode, UiFileError> {
    build(self, Some(name), args, next_instance(), assets.world_mut())
  }

  /// Build again what `source` built, as the same instance
  pub(crate) fn build_source(
    &self,
    source: &TreeSource,
    world: &mut World,
  ) -> Result<UiNode, UiFileError> {
    build(
      self,
      source.template.as_deref(),
      &source.args,
      source.instance,
      world,
    )
  }
}

/// The newest content version among `file` and the files it imports,
/// through every import: it moves on whenever any of them changes, since a
/// store's versions only grow and each change takes the next one
pub(crate) fn version_of(file: &Handle<UiFile>, files: &Assets<UiFile>) -> u64 {
  fn newest(id: AssetId<UiFile>, files: &Assets<UiFile>, visits: &mut usize) -> u64 {
    let own = files.version(id).unwrap_or(0);
    if *visits == 0 {
      return own;
    }
    *visits -= 1;
    files.get(id).map_or(own, |file| {
      file.doc.imports().iter().fold(own, |newest_yet, import| {
        newest_yet.max(newest(import.id(), files, visits))
      })
    })
  }
  let mut visits = MAX_IMPORT_VISITS;
  newest(file.id(), files, &mut visits)
}

/// The newest version among the files `doc` imports, through every import
fn imports_version(doc: &Document, world: Option<&World>) -> u64 {
  let Some(files) = world.and_then(|world| world.get_resource::<Assets<UiFile>>()) else {
    return 0;
  };
  doc
    .imports()
    .iter()
    .map(|import| version_of(import, files))
    .max()
    .unwrap_or(0)
}

/// Build `file`'s root (no `template`) or a template, as `instance`
fn build(
  file: &Handle<UiFile>,
  template: Option<&str>,
  args: &UiArgs,
  instance: u64,
  world: &mut World,
) -> Result<UiNode, UiFileError> {
  let loaded = world
    .get_resource::<Assets<UiFile>>()
    .and_then(|files| files.get(file))
    .map(|loaded| (loaded.path.clone(), Arc::clone(&loaded.doc)));
  let Some((path, doc)) = loaded else {
    let path = world
      .get_resource::<AssetServer>()
      .and_then(|server| server.path(file))
      .map(|path| path.as_str().to_string())
      .unwrap_or_default();
    return Err(UiFileError::one(
      &path,
      "the file is not loaded (loading on the workers, failed or freed)",
    ));
  };
  resolve_imports(&path, &doc, world)?;
  let version = world
    .get_resource::<Assets<UiFile>>()
    .map_or(0, |files| version_of(file, files));
  let scope = Scope {
    path: path.clone(),
    doc: Arc::clone(&doc),
  };
  let built = match template {
    None => {
      let Some(root) = &doc.root else {
        return Err(UiFileError::one(&path, "the file has no root"));
      };
      let mut cx = BuildCx::in_file(Reach::Build(world), scope, instance);
      let node = build_node(root, &mut cx);
      (node, cx.issues, path.clone())
    }
    Some(name) => {
      let reach = Reach::Build(world);
      let Some((def, defined)) = scope.find(name, &reach) else {
        let mut message = format!("no template \"{name}\" in this file or its imports");
        let names = scope.names(&reach);
        if let Some(close) = nearest(name, names.iter().map(String::as_str)) {
          message.push_str(&format!("; did you mean \"{close}\"?"));
        }
        return Err(UiFileError::one(&path, message));
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
      let mut cx = BuildCx::in_file(reach, scope, instance);
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
    return Err(UiFileError::one(&path, format!("{what} builds no node")));
  };
  if template.is_none() {
    for (name, value) in &doc.seeds {
      node = node.seed(name.clone(), value.clone());
    }
  }
  node.source = Some(Arc::new(TreeSource {
    file: file.clone(),
    template: template.map(str::to_string),
    args: args.clone(),
    instance,
    version,
  }));
  Ok(node)
}

/// A file read with `parse` loads its imports at its first build, and is
/// checked against them then, which every later build reports again; a
/// file that loaded has them already
fn resolve_imports(path: &str, doc: &Arc<Document>, world: &mut World) -> Result<(), UiFileError> {
  if let Some(checked) = doc.parsed_check.get() {
    return checked.clone();
  }
  if doc.imports.get().is_some() {
    return Ok(());
  }
  let mut imports = Vec::with_capacity(doc.import_paths.len());
  let mut issues = Vec::new();
  for (import, pos) in &doc.import_paths {
    match AssetsView::new(world).load_now::<UiFile>(import) {
      Ok(file) => imports.push(file),
      Err(error) => {
        issues.push(issue_at(path, *pos, format!("cannot import \"{import}\"")));
        issues.extend(issues_of(&error, import));
      }
    }
  }
  // Not held: the next build tries again, and says why again
  UiFileError::check(issues)?;
  let _ = doc.imports.set(imports);
  let checked = UiFileError::check(check_built(path, doc, Reach::check(Some(world), None)));
  let _ = doc.parsed_check.set(checked.clone());
  checked
}

/// A file's mistakes in building: its templates defined twice across it
/// and its imports, then its root and every template built once, each
/// template alone (its required parameters unknown)
fn check_built(path: &str, doc: &Arc<Document>, reach: Reach<'_>) -> Vec<UiFileIssue> {
  let mut issues = duplicates(path, doc, &reach);
  let scope = Scope {
    path: path.to_string(),
    doc: Arc::clone(doc),
  };
  let mut cx = BuildCx::in_file(reach, scope, 0);
  for def in &doc.order {
    check_template(def, &mut cx);
  }
  if let Some(root) = &doc.root {
    build_node(root, &mut cx);
  }
  issues.extend(
    cx.issues
      .into_iter()
      .map(|issue| UiFileIssue::at(path, issue)),
  );
  issues
}

/// A template name defined in the file and an import, or in two imports
fn duplicates(path: &str, doc: &Document, reach: &Reach<'_>) -> Vec<UiFileIssue> {
  let mut issues = Vec::new();
  // Each imported name, and the file that first defined it
  let mut defined: HashMap<&str, &str> = HashMap::new();
  for (file, (_, pos)) in doc.imports().iter().zip(&doc.import_paths) {
    let Some(file) = reach.file(file) else {
      continue;
    };
    let mut names: Vec<&String> = file.doc.templates.keys().collect();
    names.sort_unstable();
    for name in names {
      match defined.get(name.as_str()) {
        Some(first) => {
          let message =
            format!("the template \"{name}\" is defined twice (here and in \"{first}\")");
          issues.push(issue_at(path, *pos, message));
        }
        None => {
          defined.insert(name, file.path());
        }
      }
    }
  }
  for def in &doc.order {
    if let (true, Some(import)) = (
      doc.templates.contains_key(&def.name),
      defined.get(def.name.as_str()),
    ) {
      let message = format!(
        "the template \"{}\" is defined twice (here and in \"{import}\")",
        def.name
      );
      issues.push(issue_at(path, def.pos, message));
    }
  }
  issues
}

fn issue_at(path: &str, pos: Pos, message: String) -> UiFileIssue {
  UiFileIssue {
    path: path.to_string(),
    line: pos.line,
    column: pos.column,
    message,
  }
}

fn next_instance() -> u64 {
  NEXT_INSTANCE.fetch_add(1, Ordering::Relaxed)
}

/// What a file's text holds once read, before its imports are known
struct Parts {
  root: Option<Value>,
  /// Every template, in file order, with whether its name can be used
  defs: Vec<(Arc<TemplateDef>, bool)>,
  seeds: Vec<(String, BindValue)>,
}

/// A file's text read and its top level checked
struct Read {
  parts: Parts,
  /// Each import's path, and where it is named
  imports: Vec<(String, Pos)>,
  /// The mistakes found so far
  issues: Vec<UiFileIssue>,
}

impl Document {
  fn new(
    parts: Parts,
    import_paths: Vec<(String, Pos)>,
    imports: OnceLock<Vec<Handle<UiFile>>>,
  ) -> Document {
    let Parts { root, defs, seeds } = parts;
    Document {
      root,
      templates: defs
        .iter()
        .filter(|(_, usable)| *usable)
        .map(|(def, _)| (def.name.clone(), Arc::clone(def)))
        .collect(),
      order: defs.into_iter().map(|(def, _)| def).collect(),
      imports,
      import_paths,
      parsed_check: OnceLock::new(),
      seeds,
    }
  }
}

/// Read `text` as the file at `path`: its top level, its imports' paths,
/// its templates and its bindings, with every mistake collected. A file
/// that cannot be read at all is an error
fn read_file(path: &str, text: &str) -> Result<Read, UiFileError> {
  let value = read(text).map_err(|issue| UiFileError {
    issues: vec![UiFileIssue::at(path, issue)],
  })?;
  let mut check = Check {
    path,
    issues: Vec::new(),
  };
  let top = check.top_level(&value);
  let imports = check.imports(top.imports);
  let defs = check.templates(top.templates);
  let seeds = check.bindings(top.bindings);
  Ok(Read {
    parts: Parts {
      root: top.root.cloned(),
      defs,
      seeds,
    },
    imports,
    issues: check.issues,
  })
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

  /// The imported files' paths, each with where it is named
  fn imports(&mut self, value: Option<&Value>) -> Vec<(String, Pos)> {
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
    let mut paths: Vec<(String, Pos)> = Vec::new();
    for item in items {
      let ValueKind::Str(path) = &item.kind else {
        let message = format!(
          "an import is a file path (a string), not {}",
          describe(&item.kind)
        );
        self.issue(item.pos, message);
        continue;
      };
      if paths.iter().any(|(named, _)| named == path) {
        self.issue(item.pos, format!("\"{path}\" is imported twice"));
        continue;
      }
      paths.push((path.clone(), item.pos));
    }
    paths
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
  use crate::core::asset::{MemorySource, insert_registry};
  use crate::core::ui::nodes::fingerprint;
  use crate::prelude::*;

  /// A registry over `files`
  fn world_over(files: &[(&str, &str)]) -> World {
    let source = MemorySource::new();
    for (path, text) in files {
      source.insert(path, *text);
    }
    let mut world = World::new();
    insert_registry(&mut world, AssetServer::new(source));
    world
  }

  /// A file in a registry of its own
  struct Loaded {
    world: World,
    file: Handle<UiFile>,
  }

  impl Loaded {
    fn root(&mut self) -> UiNode {
      self
        .file
        .root(&mut AssetsView::new(&mut self.world))
        .expect("builds")
    }

    fn template(&mut self, name: &str, args: &UiArgs) -> Result<UiNode, UiFileError> {
      self
        .file
        .template(name, args, &mut AssetsView::new(&mut self.world))
    }
  }

  /// `text` loaded as `ui/test.ui.ron`, beside `files`
  fn load(text: &str, files: &[(&str, &str)]) -> Result<Loaded, UiFileError> {
    let mut all = files.to_vec();
    all.push(("ui/test.ui.ron", text));
    let mut world = world_over(&all);
    let file = AssetsView::new(&mut world).load_now::<UiFile>("ui/test.ui.ron")?;
    Ok(Loaded { world, file })
  }

  /// Text read with `parse` and added to a registry over `files`
  fn parsed(name: &str, text: &str, files: &[(&str, &str)]) -> Loaded {
    let mut world = world_over(files);
    let file = UiFile::parse(name, text).expect("parses");
    let file = AssetsView::new(&mut world).add(file);
    Loaded { world, file }
  }

  fn error(result: Result<Loaded, UiFileError>) -> UiFileError {
    match result {
      Ok(_) => panic!("should fail"),
      Err(error) => error,
    }
  }

  fn messages(result: Result<Loaded, UiFileError>) -> Vec<String> {
    error(result)
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
    let built = load(MENU, &[]).unwrap().root();
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
  fn false_on_a_use_leaves_a_flag_the_template_sets() {
    let mut file = load(
      r#"(templates: {"Wide": (node: Button(label: "a", fill_width: true))}, root: Wide(fill_width: false))"#,
      &[],
    )
    .unwrap();
    assert_eq!(file.root().layout.width, Some(Length::Fill));
  }

  #[test]
  fn keys_of_template_instances_use_the_template_name() {
    let built = load(MENU, &[]).unwrap().root();
    assert_eq!(
      crate::core::ui::nodes::child_keys(&built.children),
      ["MenuButton#0", "MenuButton#1"]
    );
  }

  #[test]
  fn a_string_hole_that_is_not_a_parameter_stays_a_binding() {
    let mut file = load(
      r#"(templates: {"Gold": (params: ["who"], node: Text("{who} has {gold}"))}, root: Gold(who: "Kai"))"#,
      &[],
    )
    .unwrap();
    assert_eq!(file.root().state.label.as_deref(), Some("Kai has {gold}"));
  }

  const COMMON: (&str, &str) = (
    "ui/common.ui.ron",
    r#"(templates: {
      "Inner": (params: ["t"], node: Text(t)),
      "Outer": (params: ["t"], node: Column(children: [Inner(t: t)])),
    })"#,
  );

  #[test]
  fn templates_come_from_imports_and_resolve_in_their_own_file() {
    let mut file = load(
      r#"(imports: ["ui/common.ui.ron"], root: Outer(t: "hi"))"#,
      &[COMMON],
    )
    .unwrap();
    assert_eq!(file.root().children[0].state.label.as_deref(), Some("hi"));
  }

  #[test]
  fn template_mistakes_are_reported() {
    assert_eq!(
      messages(load("(root: Column, other: 1)", &[])),
      ["a UI file has root, templates, imports and bindings; \"other\" is not one"]
    );
    assert_eq!(
      messages(load("()", &[])),
      ["a UI file needs a root or templates"]
    );
    assert!(
      messages(load(r#"(templates: {"button": (node: Column)})"#, &[]))[0]
        .contains("template names are CamelCase")
    );
    assert!(
      messages(load(r#"(templates: {"Button": (node: Column)})"#, &[]))[0]
        .contains("is a node kind")
    );
    assert!(
      messages(load(
        r#"(templates: {"A": (params: ["x"], node: Text(x))}, root: A())"#,
        &[]
      ))[0]
        .contains("A needs \"x\"")
    );
    assert!(
      messages(load(
        r#"(templates: {"A": (params: ["x"], node: Text(x))}, root: A(x: "1", y: 2))"#,
        &[]
      ))[0]
        .contains("A has no parameter or option \"y\"")
    );
    assert!(
      messages(load(
        r#"(templates: {"A": (params: ["label"], node: Text(labl))})"#,
        &[]
      ))[0]
        .contains("did you mean \"label\"")
    );
    assert!(
      messages(load(
        r#"(templates: {"A": (node: B), "B": (node: A)})"#,
        &[]
      ))[0]
        .contains("uses itself")
    );
    assert!(
      messages(load(r#"(templates: {"A": (node: Buton)})"#, &[]))[0]
        .contains("did you mean \"Button\""),
      "bodies are checked at load"
    );
  }

  #[test]
  fn a_load_reports_its_top_level_and_build_mistakes_together() {
    assert_eq!(
      messages(load("(other: 1, root: Column(\n  gpa: 1))", &[])),
      [
        "a UI file has root, templates, imports and bindings; \"other\" is not one",
        "Column has no option \"gpa\"; did you mean \"gap\"?",
      ]
    );
  }

  #[test]
  fn import_mistakes_are_reported() {
    let cycle = [
      (
        "a.ui.ron",
        r#"(imports: ["b.ui.ron"], templates: {"A": (node: Column)})"#,
      ),
      (
        "b.ui.ron",
        r#"(imports: ["a.ui.ron"], templates: {"B": (node: Column)})"#,
      ),
    ];
    let issues = messages(load(r#"(imports: ["a.ui.ron"], root: Column)"#, &cycle));
    assert_eq!(
      issues,
      ["\"a.ui.ron\" imports itself: a.ui.ron → b.ui.ron → a.ui.ron"]
    );
    let twice = [("c.ui.ron", r#"(templates: {"X": (node: Column)})"#)];
    let issues = messages(load(
      r#"(imports: ["c.ui.ron"], templates: {"X": (node: Row)}, root: X)"#,
      &twice,
    ));
    assert!(issues[0].contains("defined twice"), "{issues:?}");
    let issues = messages(load(r#"(imports: ["missing.ui.ron"], root: Column)"#, &[]));
    assert!(
      issues[0].contains("cannot import \"missing.ui.ron\""),
      "{issues:?}"
    );
  }

  #[test]
  fn an_import_with_mistakes_lists_them_after_the_import() {
    let broken = [("ui/broken.ui.ron", "(templates: {\"T\": (node: Buton)})")];
    let error = error(load(
      r#"(imports: ["ui/broken.ui.ron"], root: Column)"#,
      &broken,
    ));
    assert_eq!(
      error.to_string(),
      "ui/test.ui.ron:1:12: cannot import \"ui/broken.ui.ron\"\n\
       ui/broken.ui.ron:1:26: unknown node kind \"Buton\"; did you mean \"Button\"?"
    );
  }

  #[test]
  fn errors_print_with_positions() {
    let error = UiFile::parse("ui/x.ui.ron", "(root: Column(\n  gpa: 1))")
      .err()
      .unwrap();
    assert_eq!(
      error.to_string(),
      "ui/x.ui.ron:2:3: Column has no option \"gpa\"; did you mean \"gap\"?"
    );
  }

  #[test]
  fn parsed_text_loads_its_imports_at_its_first_build_and_is_checked_then() {
    let mut file = parsed(
      "ui/x.ui.ron",
      r#"(imports: ["ui/common.ui.ron"], root: Outer(t: "hi"))"#,
      &[COMMON],
    );
    assert_eq!(file.root().children[0].state.label.as_deref(), Some("hi"));

    let mut missing = parsed(
      "ui/x.ui.ron",
      r#"(imports: ["ui/gone.ui.ron"], root: Column)"#,
      &[],
    );
    let error = missing
      .file
      .root(&mut AssetsView::new(&mut missing.world))
      .err()
      .unwrap();
    assert!(
      error
        .to_string()
        .starts_with("ui/x.ui.ron:1:12: cannot import \"ui/gone.ui.ron\""),
      "{error}"
    );

    let mut unknown = parsed(
      "ui/x.ui.ron",
      r#"(imports: ["ui/common.ui.ron"], templates: {"Mine": (node: Outr(t: "a"))}, root: Column)"#,
      &[COMMON],
    );
    let error = unknown.template("Mine", &UiArgs::new()).err().unwrap();
    assert!(
      error.to_string().contains("did you mean \"Outer\""),
      "checked against its imports: {error}"
    );
  }

  #[test]
  fn parsed_text_that_fails_its_check_against_its_imports_fails_every_build() {
    // A template it does not build, named twice across it and its import
    let mut file = parsed(
      "ui/x.ui.ron",
      r#"(imports: ["ui/common.ui.ron"], templates: {"Inner": (node: Text("mine"))}, root: Column)"#,
      &[COMMON],
    );
    for build in ["first", "second"] {
      let error = file
        .file
        .root(&mut AssetsView::new(&mut file.world))
        .err()
        .unwrap_or_else(|| panic!("the {build} build succeeds"));
      assert!(error.to_string().contains("defined twice"), "{error}");
    }
  }

  #[test]
  fn a_literal_image_path_with_a_brace_outside_a_template_is_loaded() {
    let issues = messages(load(r#"(root: Image("x{y}.png"))"#, &[]));
    assert_eq!(
      issues,
      ["cannot load the image \"x{y}.png\": no asset at \"x{y}.png\""]
    );
  }

  #[test]
  fn a_template_built_from_code_takes_args_and_carries_its_source() {
    let mut file = parsed(
      "ui/slot.ui.ron",
      r#"(templates: {"Item": (params: ["name", "count"], node: Button(label: "{name} x{count}"))})"#,
      &[],
    );
    let node = file
      .template(
        "Item",
        &UiArgs::new().with("name", "Sword").with("count", 3),
      )
      .unwrap();
    assert_eq!(node.state.label.as_deref(), Some("Sword x3"));
    let source = node
      .source
      .as_ref()
      .expect("the top node carries its source");
    assert_eq!(source.template.as_deref(), Some("Item"));
    assert_eq!(source.file, file.file, "it holds the file");
    assert_ne!(node.instance, 0);
    let error = file
      .template("Item", &UiArgs::new().with("name", "S").with("cnt", 1))
      .err()
      .unwrap();
    assert!(error.to_string().contains("\"cnt\""), "{error}");
    let other = file
      .template("Item", &UiArgs::new().with("name", "A").with("count", 1))
      .unwrap();
    assert_ne!(
      other.instance, node.instance,
      "each build is its own instance"
    );
  }

  #[test]
  fn the_root_seeds_its_bindings_and_rebuilds_with_the_same_instance() {
    let mut file = parsed(
      "ui/hud.ui.ron",
      r#"(bindings: {"score": 0}, root: Text("{score}"))"#,
      &[],
    );
    let built = file.root();
    assert_eq!(built.seeds, [("score".to_string(), BindValue::Number(0.0))]);
    let source = built.source.clone().unwrap();
    let again = source.file.build_source(&source, &mut file.world).unwrap();
    assert_eq!(again.instance, built.instance);
    assert_eq!(fingerprint(&again), fingerprint(&built));
  }

  #[test]
  fn an_image_whose_sprite_is_a_parameter_is_still_checked() {
    let issues = messages(load(
      r#"(templates: {"Pic": (params: ["src"], node: Image(sprite: src, image_sze: (1, 1)))})"#,
      &[],
    ));
    assert_eq!(
      issues,
      ["Image has no option \"image_sze\"; did you mean \"image_size\"?"]
    );
  }

  #[test]
  fn a_missing_image_is_an_issue_at_its_value() {
    let issues = error(load(
      r#"(templates: {"Icon": (params: ["name"], node: Image("icons/{name}.png"))}, root: Column(children: [Image("gone.png")]))"#,
      &[],
    ))
    .issues;
    assert_eq!(
      issues.len(),
      1,
      "a path with a parameter's hole is not loaded: {issues:?}"
    );
    assert_eq!((issues[0].line, issues[0].column), (1, 106));
    assert_eq!(
      issues[0].message,
      "cannot load the image \"gone.png\": no asset at \"gone.png\""
    );
  }

  #[test]
  fn a_parameter_can_stand_for_a_node_written_in_the_using_file() {
    let labelled = (
      "ui/common.ui.ron",
      r#"(templates: {
        "Labelled": (params: ["label", "widget"], node: Row(children: [Text(label), widget])),
      })"#,
    );
    let built = load(
      r#"(
        imports: ["ui/common.ui.ron"],
        templates: {"Volume": (node: Slider(range: (0, 1), bind: "volume"))},
        root: Column(children: [
          Labelled(label: "Music", widget: Slider(range: (0, 1))),
          Labelled(label: "Master", widget: Volume),
        ]),
      )"#,
      &[labelled],
    )
    .unwrap()
    .root();
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
    let mut file = load(
      r#"(templates: {"Hp": (params: ["hp", "n"], node: Text("{hp:.2}/{n} {{hp}} {hp"))}, root: Hp(hp: 2.5, n: 3))"#,
      &[],
    )
    .unwrap();
    assert_eq!(
      file.root().state.label.as_deref(),
      Some("2.50/3 {{hp}} {hp")
    );
  }

  #[test]
  fn a_default_is_checked_where_its_template_places_it() {
    let issues = messages(load(
      r#"(templates: {"Gap": (defaults: {"g": "wide"}, node: Column(gap: g))})"#,
      &[],
    ));
    assert_eq!(issues, ["\"gap\" takes a number, not a string"]);
  }

  #[test]
  fn a_mistake_in_a_template_body_is_reported_once_however_often_it_is_used() {
    let issues = messages(load(
      r#"(templates: {"A": (node: Text(text: "a", sise: 3))}, root: Column(children: [A, A]))"#,
      &[],
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
    let issues = messages(load(&text, &[]));
    assert!(
      issues.iter().any(|m| m.contains("nest more than 64 deep")),
      "{issues:?}"
    );
  }

  #[test]
  fn an_error_prints_one_issue_per_line_and_no_position_at_line_0() {
    let error = error(load(r#"(imports: ["missing.ui.ron"], root: Column)"#, &[]));
    assert_eq!(
      error.to_string(),
      "ui/test.ui.ron:1:12: cannot import \"missing.ui.ron\"\nmissing.ui.ron: no asset at \"missing.ui.ron\""
    );
  }

  #[test]
  fn an_unknown_template_from_code_is_named_with_a_suggestion() {
    let mut file = parsed(
      "ui/slot.ui.ron",
      r#"(templates: {"Item": (node: Slot)})"#,
      &[],
    );
    let error = file.template("Itme", &UiArgs::new()).err().unwrap();
    assert!(
      error.to_string().contains("did you mean \"Item\""),
      "{error}"
    );
  }

  #[test]
  fn a_file_still_loading_builds_nothing_yet() {
    let mut world = world_over(&[("ui/late.ui.ron", "(root: Column)")]);
    let server = world.resource::<AssetServer>().clone();
    let file = server.load::<UiFile>("ui/late.ui.ron");
    let error = file.root(&mut AssetsView::new(&mut world)).err().unwrap();
    assert!(error.to_string().contains("not loaded"), "{error}");
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
  fn args_take_the_whole_number_types_bind_values_do() {
    assert_eq!(UiArg::from(-3_i64), UiArg::Number(-3.0));
    assert_eq!(UiArg::from(7_u64), UiArg::Number(7.0));
  }

  #[test]
  fn a_template_used_inside_its_own_argument_is_not_a_cycle() {
    let built = load(
      r#"(
        templates: {"Card": (params: ["content"], node: Panel(children: [content]))},
        root: Card(content: Card(content: Text("x"))),
      )"#,
      &[],
    )
    .unwrap()
    .root();
    assert_eq!(
      built.children[0].children[0].state.label.as_deref(),
      Some("x")
    );

    let labelled = (
      "ui/common.ui.ron",
      r#"(templates: {
        "Labelled": (params: ["label", "widget"], node: Row(children: [Text(label), widget])),
      })"#,
    );
    let file = load(
      r#"(
        imports: ["ui/common.ui.ron"],
        templates: {"Volume": (node: Labelled(label: "v", widget: Slider(range: (0, 1))))},
        root: Labelled(label: "Master", widget: Volume),
      )"#,
      &[labelled],
    );
    assert!(file.is_ok(), "{:?}", file.err());

    let issues = messages(load(
      r#"(templates: {"A": (defaults: {"c": A}, node: Panel(children: [c]))})"#,
      &[],
    ));
    assert!(
      issues[0].contains("uses itself"),
      "a default is the template's own: {issues:?}"
    );
  }

  #[test]
  fn imports_are_not_transitive_even_in_arguments() {
    let files = [
      (
        "a.ui.ron",
        r#"(imports: ["c.ui.ron"], templates: {"A": (params: ["w"], node: Column(children: [w]))})"#,
      ),
      ("c.ui.ron", r#"(templates: {"Foo": (node: Text("foo"))})"#),
    ];
    let error = error(load(
      "(imports: [\"a.ui.ron\"],\n  root: A(w: Foo))",
      &files,
    ));
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
    let common = (
      "ui/common.ui.ron",
      r#"(templates: {"Item": (params: ["name"], node: Button(label: name))})"#,
    );
    let mut file = load(
      r#"(imports: ["ui/common.ui.ron"], root: Column)"#,
      &[common],
    )
    .unwrap();
    let args = UiArgs::new().with("name", "S").with("cnt", 1);
    let error = file
      .template("Item", &args)
      .expect_err("cnt is not an argument of Item");
    assert_eq!(error.issues.len(), 1, "{error}");
    assert_eq!(
      error.issues[0].path, "ui/common.ui.ron",
      "the position is in the text of the file that defines Item"
    );
    assert!(error.issues[0].message.contains("\"cnt\""), "{error}");
  }

  #[test]
  fn the_version_counts_every_import_through_imports() {
    let mut file = load(
      r#"(imports: ["ui/a.ui.ron"], root: Column)"#,
      &[
        (
          "ui/a.ui.ron",
          r#"(imports: ["ui/b.ui.ron"], templates: {"A": (node: Column)})"#,
        ),
        ("ui/b.ui.ron", r#"(templates: {"B": (node: Column)})"#),
      ],
    )
    .unwrap();
    let b = AssetsView::new(&mut file.world)
      .load_now::<UiFile>("ui/b.ui.ron")
      .unwrap();
    let before = version_of(&file.file, file.world.resource::<Assets<UiFile>>());
    let _ = file.world.resource_mut::<Assets<UiFile>>().get_mut(&b);
    let after = version_of(&file.file, file.world.resource::<Assets<UiFile>>());
    assert!(after > before, "{before} -> {after}");
  }
}
