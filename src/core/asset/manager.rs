//! The old asset manager: UI files, until they load through the registry
//!
//! - `AssetManager` - loads and caches UI files by path, and reloads them
//!   when the hot reload watcher sees them change
//! - Images and shaders are registry assets
//!   (`ctx.assets().load_now::<Image>(path)`, `load_now::<Shader>(path)`);
//!   `load_sprite` is a thin call to the registry, kept until the manager goes
//!
//! The engine's manager is a resource (`ctx.assets` in scene callbacks); it
//! applies hot reloads each frame in `PreUpdate`.

use std::collections::HashMap;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::core::ecs::{Resource, World};
use crate::core::error::{ManagerError, ManagerResult};
use crate::core::graphics::Image;
use crate::core::ui::file::build::Placeholders;
use crate::core::ui::file::document::ImportResolver;
use crate::core::ui::file::{UiFile, UiFileError};

use super::hot_reload::{HotReloadEvent, HotReloadWatcher};
use super::{AssetError, AssetsView, Handle};
#[allow(unused_imports)] // engine_info only used in debug builds
use crate::{engine_error, engine_info, engine_warn};

// ============================================================================
// AssetManager
// ============================================================================

/// The old manager for UI files, cached by path and reloaded when they
/// change. Images and shaders load through the asset registry
/// (`ctx.assets()`)
///
/// # Example
///
/// ```rust,ignore
/// let mut assets = AssetManager::new();
/// let menu = assets.load_ui("ui/menu.ui.ron")?;
/// ```
#[derive(Resource)]
pub struct AssetManager {
  /// Base directory for resolving relative paths
  base_path: PathBuf,
  /// Hot reload watcher (optional, for development)
  watcher: Option<HotReloadWatcher>,
  /// Hot reloads since the engine last took them: the path, or the path and
  /// the error
  reload_outcomes: Vec<Result<PathBuf, (PathBuf, String)>>,
  /// Cache of UI files, keyed by resolved path
  ui_files: HashMap<PathBuf, UiFile>,
  /// The UI files being loaded now, to catch imports that go round
  ui_loading: Vec<PathBuf>,
  /// A hash of the last broken text of each UI file the watcher warned about
  ui_warned: HashMap<PathBuf, u64>,
}

impl AssetManager {
  /// Create a new asset manager with the current directory as base path
  pub fn new() -> Self {
    AssetManager {
      base_path: PathBuf::from("."),
      watcher: None,
      reload_outcomes: Vec::new(),
      ui_files: HashMap::new(),
      ui_loading: Vec::new(),
      ui_warned: HashMap::new(),
    }
  }

  /// Create an asset manager with a specific base path
  ///
  /// All relative paths will be resolved from this base directory.
  pub fn with_base_path<P: AsRef<Path>>(base_path: P) -> Self {
    AssetManager {
      base_path: base_path.as_ref().to_path_buf(),
      watcher: None,
      reload_outcomes: Vec::new(),
      ui_files: HashMap::new(),
      ui_loading: Vec::new(),
      ui_warned: HashMap::new(),
    }
  }

  /// Set the base path for asset loading
  pub fn set_base_path<P: AsRef<Path>>(&mut self, path: P) {
    self.base_path = path.as_ref().to_path_buf();
  }

  /// Get the current base path
  pub fn base_path(&self) -> &Path {
    &self.base_path
  }

  /// Resolve a path relative to the base path
  fn resolve_path<P: AsRef<Path>>(&self, path: P) -> PathBuf {
    let path = path.as_ref();
    if path.is_absolute() {
      path.to_path_buf()
    } else {
      self.base_path.join(path)
    }
  }

  /// Cache key for an asset path: the canonical resolved path
  ///
  /// Hot reload events carry absolute, symlink-resolved paths, so keys are
  /// canonicalized to match them. Falls back to the resolved path when the
  /// file does not exist. Handle ids stay the (readable) resolved path.
  fn cache_key<P: AsRef<Path>>(&self, path: P) -> PathBuf {
    let resolved = self.resolve_path(path);
    std::fs::canonicalize(&resolved).unwrap_or(resolved)
  }

  /// Load an image at once through the asset registry of `world`, as
  /// `ctx.assets().load_now::<Image>(path)` does; `path` is relative to the
  /// asset folder. Kept for the old API until the manager goes
  pub fn load_sprite(
    &self,
    world: &mut World,
    path: &str,
  ) -> Result<Handle<Image>, Arc<AssetError>> {
    AssetsView::new(world).load_now::<Image>(path)
  }

  /// Read a file's text, as an asset error
  fn read_text(full_path: &Path) -> ManagerResult<String> {
    std::fs::read_to_string(full_path).map_err(|e| match e.kind() {
      std::io::ErrorKind::NotFound => ManagerError::NotFound(full_path.to_path_buf()),
      _ => ManagerError::LoadError {
        path: full_path.to_path_buf(),
        reason: e.to_string(),
      },
    })
  }

  /// Load a UI file (`.ui.ron`)
  ///
  /// Cached by resolved path, so loading the same file twice returns the same
  /// [`UiFile`]. Its imports load through the cache too. Every mistake in the
  /// file is reported together in [`ManagerError::UiFile`]. With hot reload
  /// enabled, editing the file updates the `UiFile` in place.
  pub fn load_ui<P: AsRef<Path>>(&mut self, path: P) -> ManagerResult<UiFile> {
    let key = self.cache_key(&path);
    if let Some(file) = self.ui_files.get(&key) {
      return Ok(file.clone());
    }
    let text = Self::read_text(&self.resolve_path(&path))?;
    let name = path.as_ref().display().to_string();
    self.ui_loading.push(key.clone());
    let loaded = UiFile::load(&name, &text, self, &mut Placeholders::default(), false);
    self.ui_loading.pop();
    let file = loaded.map_err(ManagerError::UiFile)?;
    self.ui_files.insert(key, file.clone());
    Ok(file)
  }

  /// Add a UI file from text the game holds (for instance `include_str!`)
  ///
  /// The text is checked like a file on disk. Later loads and imports of
  /// `path` use it, and it is never reloaded from disk.
  pub fn add_ui<P: AsRef<Path>>(&mut self, path: P, text: &str) -> Result<UiFile, UiFileError> {
    let key = self.cache_key(&path);
    let name = path.as_ref().display().to_string();
    self.ui_loading.push(key.clone());
    let loaded = UiFile::load(&name, text, self, &mut Placeholders::default(), true);
    self.ui_loading.pop();
    let file = loaded?;
    self.ui_files.insert(key, file.clone());
    Ok(file)
  }

  /// Read a cached UI file from disk again
  ///
  /// On success the same [`UiFile`] holds the new contents, and the files that
  /// import it are checked again. On a mistake the old contents stay.
  pub fn reload_ui<P: AsRef<Path>>(&mut self, path: P) -> ManagerResult<()> {
    let key = self.cache_key(&path);
    let Some(file) = self.ui_files.get(&key).cloned() else {
      return Err(ManagerError::NotFound(self.resolve_path(&path)));
    };
    if file.is_embedded() {
      return Ok(());
    }
    let text = Self::read_text(&key)?;
    self
      .reload_ui_text(&key, &file, &text)
      .map_err(ManagerError::UiFile)
  }

  /// Check `text` as the new contents of `file`; on success hold it and check
  /// the importers again
  fn reload_ui_text(&mut self, key: &Path, file: &UiFile, text: &str) -> Result<(), UiFileError> {
    self.ui_loading.push(key.to_path_buf());
    let checked = UiFile::check(file.path(), text, self, &mut Placeholders::default());
    self.ui_loading.pop();
    file.replace(checked?);
    self.recheck_importers(file);
    Ok(())
  }

  /// Check every cached file that imports `changed`, directly or through
  /// others, against its own text; a file that no longer checks keeps its old
  /// contents
  fn recheck_importers(&mut self, changed: &UiFile) {
    let mut queue = vec![changed.clone()];
    let mut done: Vec<UiFile> = vec![changed.clone()];
    while let Some(current) = queue.pop() {
      let importers: Vec<(PathBuf, UiFile)> = self
        .ui_files
        .iter()
        .filter(|(_, file)| file.imports_file(&current))
        .filter(|(_, file)| !done.iter().any(|d| d.same_file(file)))
        .map(|(key, file)| (key.clone(), file.clone()))
        .collect();
      for (key, importer) in importers {
        done.push(importer.clone());
        let text = importer.doc().text.clone();
        self.ui_loading.push(key);
        let checked = UiFile::check(importer.path(), &text, self, &mut Placeholders::default());
        self.ui_loading.pop();
        match checked {
          Ok(doc) => {
            importer.replace(doc);
            queue.push(importer);
          }
          Err(error) => {
            engine_warn!(
              module: "ui",
              "could not check {} again after an import changed; keeping the old version:\n{error}",
              importer.path()
            );
          }
        }
      }
    }
  }

  /// Enable hot reloading for development
  ///
  /// When enabled, a background thread watches for file changes and
  /// automatically reloads modified assets. The engine enables it for the
  /// assets folder in dev builds and calls `process_reloads()` each frame.
  ///
  /// # Arguments
  ///
  /// * `watch_paths` - Directories to watch for changes
  ///
  /// # Example
  ///
  /// ```rust,ignore
  /// assets.enable_hot_reload(vec!["assets/sprites", "assets/textures"])?;
  ///
  /// // Each reload then arrives as a message
  /// for reloaded in ctx.messages::<AssetReloaded>() {
  ///     println!("Reloaded {}", reloaded.path.display());
  /// }
  /// ```
  pub fn enable_hot_reload<P, I>(&mut self, watch_paths: I) -> ManagerResult<()>
  where
    P: AsRef<Path>,
    I: IntoIterator<Item = P>,
  {
    use super::hot_reload::HotReloadConfig;

    // Files cannot be watched on the web
    if cfg!(target_arch = "wasm32") {
      let _ = watch_paths;
      engine_info!(module: "HotReload", "hot reload is not available on the web");
      return Ok(());
    }

    let paths: Vec<PathBuf> = watch_paths
      .into_iter()
      .map(|p| self.resolve_path(p))
      .collect();

    let config = HotReloadConfig {
      watch_paths: paths,
      ..HotReloadConfig::default()
    };

    let watcher =
      HotReloadWatcher::new(config).map_err(|e| ManagerError::WatchError(e.to_string()))?;

    self.watcher = Some(watcher);
    Ok(())
  }

  /// Disable hot reloading
  ///
  /// Stops the background watcher thread.
  pub fn disable_hot_reload(&mut self) {
    self.watcher = None;
  }

  /// Check if hot reloading is enabled
  pub fn is_hot_reload_enabled(&self) -> bool {
    self.watcher.is_some()
  }

  /// Process pending hot reload events
  ///
  /// Checks for and applies asset changes. Returns the number of assets that
  /// were reloaded. The engine calls it once per frame in `PreUpdate` (dev
  /// builds) and writes an `AssetReloaded` or `AssetLoadFailed` message for
  /// each outcome.
  pub fn process_reloads(&mut self) -> usize {
    let events = match &self.watcher {
      Some(w) => w.poll(),
      None => return 0,
    };

    events
      .into_iter()
      .filter(|event| self.handle_reload_event(event.clone()))
      .count()
  }

  /// The hot reloads since the last call: each reloaded path, or the path
  /// and why it failed.
  pub(crate) fn take_reload_outcomes(&mut self) -> Vec<Result<PathBuf, (PathBuf, String)>> {
    std::mem::take(&mut self.reload_outcomes)
  }

  /// The watcher saw a cached UI file change: reload it if its text changed
  /// and checks, else keep the old version and warn once per broken text
  fn reload_ui_from_event(&mut self, key: &Path, file: &UiFile) -> bool {
    let text = match std::fs::read_to_string(key) {
      Ok(text) => text,
      Err(e) => {
        engine_error!(module: "HotReload", "Failed to reload {:?}: {}", key, e);
        self
          .reload_outcomes
          .push(Err((key.to_path_buf(), e.to_string())));
        return false;
      }
    };
    if text == file.doc().text {
      // Back to the loaded text: a mistake saved again is reported again
      self.ui_warned.remove(key);
      return false;
    }
    match self.reload_ui_text(key, file, &text) {
      Ok(()) => {
        self.ui_warned.remove(key);
        self.reload_outcomes.push(Ok(key.to_path_buf()));
        #[cfg(debug_assertions)]
        engine_info!(module: "HotReload", "Reloaded UI file: {:?}", key);
        true
      }
      Err(error) => {
        let mut hasher = DefaultHasher::new();
        text.hash(&mut hasher);
        let hash = hasher.finish();
        if self.ui_warned.insert(key.to_path_buf(), hash) != Some(hash) {
          engine_warn!(
            module: "ui",
            "could not reload {}; keeping the old version:\n{error}",
            file.path()
          );
        }
        self
          .reload_outcomes
          .push(Err((key.to_path_buf(), error.to_string())));
        false
      }
    }
  }

  /// Apply one watcher event, returning whether an asset was reloaded
  ///
  /// The event path is canonicalized before the cache lookup, because the OS
  /// reports absolute (and symlink-resolved) paths while cache keys come from
  /// the possibly relative base path.
  pub(crate) fn handle_reload_event(&mut self, event: HotReloadEvent) -> bool {
    let mut reloaded = false;
    match event {
      HotReloadEvent::FileModified(path) | HotReloadEvent::FileCreated(path) => {
        let key = std::fs::canonicalize(&path).unwrap_or(path);
        if let Some(file) = self.ui_files.get(&key).cloned()
          && !file.is_embedded()
        {
          reloaded |= self.reload_ui_from_event(&key, &file);
        }
      }
      HotReloadEvent::FileDeleted(_path) => {
        // Just log - don't remove from cache as handles may still be in use
        #[cfg(debug_assertions)]
        engine_info!(module: "HotReload", "Asset removed (still cached): {:?}", _path);
      }
      HotReloadEvent::Error(e) => {
        engine_warn!(module: "HotReload", "Watcher error: {}", e);
      }
    }
    reloaded
  }
}

impl Default for AssetManager {
  fn default() -> Self {
    Self::new()
  }
}

/// UI files import through the cache, refusing imports that go round
impl ImportResolver for AssetManager {
  fn import(&mut self, path: &str) -> Result<UiFile, UiFileError> {
    let key = self.cache_key(path);
    let name = |k: &Path| {
      k.file_name().map_or_else(
        || k.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
      )
    };
    let circle = |chain: Vec<String>| {
      UiFileError::one(
        path,
        format!("the imports go round in a circle: {}", chain.join(" -> ")),
      )
    };
    if let Some(start) = self.ui_loading.iter().position(|k| *k == key) {
      let chain = self.ui_loading[start..]
        .iter()
        .map(|k| name(k))
        .chain(std::iter::once(name(&key)))
        .collect();
      return Err(circle(chain));
    }
    // A cached file that already imports a file being loaded would close a circle
    if let Some(cached) = self.ui_files.get(&key) {
      let mut seen: Vec<UiFile> = Vec::new();
      let mut stack = vec![cached.clone()];
      while let Some(file) = stack.pop() {
        if seen.iter().any(|f| f.same_file(&file)) {
          continue;
        }
        let hit = self.ui_loading.iter().position(|k| {
          self
            .ui_files
            .get(k)
            .is_some_and(|loading| loading.same_file(&file))
        });
        if let Some(start) = hit {
          let chain = self.ui_loading[start..]
            .iter()
            .map(|k| name(k))
            .chain([name(&key), name(&self.ui_loading[start])])
            .collect();
          return Err(circle(chain));
        }
        stack.extend(file.doc().imports.iter().cloned());
        seen.push(file);
      }
    }
    self.load_ui(path).map_err(|e| match e {
      ManagerError::UiFile(error) => error,
      other => UiFileError::one(path, other.to_string()),
    })
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  fn event_fixture() -> (tempfile::TempDir, AssetManager) {
    let dir = tempfile::tempdir().expect("temp dir");
    let mut assets = AssetManager::new();
    assets.set_base_path(dir.path());
    (dir, assets)
  }

  fn write(dir: &Path, name: &str, text: &str) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, text).unwrap();
    path
  }

  #[test]
  fn load_ui_caches_by_path_and_imports_through_the_cache() {
    let (dir, mut assets) = event_fixture();
    write(
      dir.path(),
      "common.ui.ron",
      r#"(templates: {"T": (node: Text("t"))})"#,
    );
    write(
      dir.path(),
      "menu.ui.ron",
      r#"(imports: ["common.ui.ron"], root: T)"#,
    );
    let menu = assets.load_ui("menu.ui.ron").unwrap();
    let again = assets.load_ui("menu.ui.ron").unwrap();
    assert_eq!(menu.generation(), again.generation());
    let common = assets.load_ui("common.ui.ron").unwrap();
    assert!(menu.imports_file(&common), "the import came from the cache");
  }

  #[test]
  fn a_broken_or_missing_ui_file_is_an_error() {
    let (dir, mut assets) = event_fixture();
    write(dir.path(), "bad.ui.ron", "(root: Buton)");
    assert!(matches!(
      assets.load_ui("bad.ui.ron"),
      Err(ManagerError::UiFile(_))
    ));
    assert!(matches!(
      assets.load_ui("nope.ui.ron"),
      Err(ManagerError::NotFound(_))
    ));
  }

  #[test]
  fn add_ui_serves_later_loads_and_imports() {
    let (dir, mut assets) = event_fixture();
    assets
      .add_ui(
        "embedded/common.ui.ron",
        r#"(templates: {"T": (node: Text("t"))})"#,
      )
      .unwrap();
    write(
      dir.path(),
      "menu.ui.ron",
      r#"(imports: ["embedded/common.ui.ron"], root: T)"#,
    );
    assert!(assets.load_ui("menu.ui.ron").is_ok());
  }

  #[test]
  fn reload_ui_replaces_the_document_and_a_broken_reload_keeps_the_old_one() {
    let (dir, mut assets) = event_fixture();
    let path = write(dir.path(), "hud.ui.ron", "(root: Text(\"a\"))");
    let file = assets.load_ui("hud.ui.ron").unwrap();
    let first = file.generation();
    std::fs::write(&path, "(root: Text(\"b\"))").unwrap();
    assets.reload_ui("hud.ui.ron").unwrap();
    assert!(
      file.generation() > first,
      "the same handle sees the new document"
    );
    let good = file.generation();
    std::fs::write(&path, "(root: Text(").unwrap();
    assert!(assets.reload_ui("hud.ui.ron").is_err());
    assert_eq!(file.generation(), good, "kept");
  }

  #[test]
  fn reloading_an_import_bumps_its_importers() {
    let (dir, mut assets) = event_fixture();
    let common = write(
      dir.path(),
      "common.ui.ron",
      r#"(templates: {"T": (node: Text("t"))})"#,
    );
    write(
      dir.path(),
      "menu.ui.ron",
      r#"(imports: ["common.ui.ron"], root: T)"#,
    );
    let menu = assets.load_ui("menu.ui.ron").unwrap();
    let before = menu.generation();
    std::fs::write(&common, r#"(templates: {"T": (node: Text("u"))})"#).unwrap();
    assets.reload_ui("common.ui.ron").unwrap();
    assert!(menu.generation() > before);
  }

  #[test]
  fn ui_imports_in_a_circle_are_an_error() {
    let (dir, mut assets) = event_fixture();
    write(
      dir.path(),
      "a.ui.ron",
      r#"(imports: ["b.ui.ron"], templates: {"A": (node: Column)})"#,
    );
    write(
      dir.path(),
      "b.ui.ron",
      r#"(imports: ["a.ui.ron"], templates: {"B": (node: Column)})"#,
    );
    let Err(ManagerError::UiFile(error)) = assets.load_ui("a.ui.ron") else {
      panic!()
    };
    assert!(error.to_string().contains("circle"), "{error}");
  }

  #[test]
  fn a_reload_that_closes_a_circle_is_refused_and_keeps_the_old_document() {
    let (dir, mut assets) = event_fixture();
    write(
      dir.path(),
      "a.ui.ron",
      r#"(imports: ["b.ui.ron"], templates: {"A": (node: Column)})"#,
    );
    let b = write(
      dir.path(),
      "b.ui.ron",
      r#"(templates: {"B": (node: Column)})"#,
    );
    assets.load_ui("a.ui.ron").unwrap();
    let file = assets.load_ui("b.ui.ron").unwrap();
    let good = file.generation();
    std::fs::write(
      &b,
      r#"(imports: ["a.ui.ron"], templates: {"B": (node: Column)})"#,
    )
    .unwrap();
    let Err(ManagerError::UiFile(error)) = assets.reload_ui("b.ui.ron") else {
      panic!()
    };
    assert!(error.to_string().contains("circle"), "{error}");
    assert_eq!(file.generation(), good, "kept");
  }

  #[test]
  fn the_watcher_refuses_a_circle_and_warns() {
    let (dir, mut assets) = event_fixture();
    write(
      dir.path(),
      "ca.ui.ron",
      r#"(imports: ["cb.ui.ron"], templates: {"A": (node: Column)})"#,
    );
    let b = write(
      dir.path(),
      "cb.ui.ron",
      r#"(templates: {"B": (node: Column)})"#,
    );
    assets.load_ui("ca.ui.ron").unwrap();
    let file = assets.load_ui("cb.ui.ron").unwrap();
    let good = file.generation();
    std::fs::write(
      &b,
      r#"(imports: ["ca.ui.ron"], templates: {"B": (node: Column)})"#,
    )
    .unwrap();
    assert!(!assets.handle_reload_event(HotReloadEvent::FileModified(b)));
    assert_eq!(file.generation(), good);
    assert_eq!(
      crate::core::ui::test_util::warnings_containing("cb.ui.ron"),
      1
    );
  }

  #[test]
  fn a_half_saved_file_warns_once_and_the_finished_save_reloads() {
    let (dir, mut assets) = event_fixture();
    let path = write(dir.path(), "halfsaved.ui.ron", "(root: Text(\"a\"))");
    let file = assets.load_ui("halfsaved.ui.ron").unwrap();
    let start = file.generation();
    std::fs::write(&path, "(root: Te").unwrap();
    for _ in 0..3 {
      assert!(!assets.handle_reload_event(HotReloadEvent::FileModified(path.clone())));
    }
    assert_eq!(
      crate::core::ui::test_util::warnings_containing("halfsaved.ui.ron"),
      1,
      "once per broken text"
    );
    std::fs::write(&path, "(root: Text(\"b\"))").unwrap();
    assert!(assets.handle_reload_event(HotReloadEvent::FileModified(path.clone())));
    assert!(file.generation() > start);
    assert!(
      !assets.handle_reload_event(HotReloadEvent::FileModified(path)),
      "the same text again does nothing"
    );
  }

  #[test]
  fn the_same_mistake_saved_again_after_a_revert_warns_again() {
    let (dir, mut assets) = event_fixture();
    let good = "(root: Text(\"a\"))";
    let path = write(dir.path(), "rebroken.ui.ron", good);
    assets.load_ui("rebroken.ui.ron").unwrap();
    let warned = || crate::core::ui::test_util::warnings_containing("rebroken.ui.ron");
    std::fs::write(&path, "(root: Te").unwrap();
    assert!(!assets.handle_reload_event(HotReloadEvent::FileModified(path.clone())));
    assert_eq!(warned(), 1);
    std::fs::write(&path, good).unwrap();
    assert!(
      !assets.handle_reload_event(HotReloadEvent::FileModified(path.clone())),
      "the loaded text: nothing to reload"
    );
    std::fs::write(&path, "(root: Te").unwrap();
    assert!(!assets.handle_reload_event(HotReloadEvent::FileModified(path)));
    assert_eq!(warned(), 2, "reported again");
  }

  #[test]
  fn load_sprite_loads_through_the_registry() {
    let files = super::super::MemorySource::new();
    let mut png = Vec::new();
    image::RgbaImage::new(3, 2)
      .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
      .expect("encode png");
    files.insert("ship.png", png);
    let mut world = World::new();
    super::super::insert_registry(&mut world, super::super::AssetServer::new(files));
    let assets = AssetManager::new();
    let ship = assets.load_sprite(&mut world, "ship.png").expect("load");
    let image = AssetsView::new(&mut world)
      .get(&ship)
      .map(|i| (i.width(), i.height()));
    assert_eq!(image, Some((3, 2)));
  }

  #[test]
  fn asset_manager_resolves_paths() {
    let assets = AssetManager::with_base_path("assets");
    let resolved = assets.resolve_path("sprites/player.png");
    assert_eq!(resolved, PathBuf::from("assets/sprites/player.png"));
  }

  #[test]
  fn asset_manager_resolves_absolute_paths() {
    let assets = AssetManager::with_base_path("assets");
    let absolute = PathBuf::from("/absolute/path/sprite.png");
    let resolved = assets.resolve_path(&absolute);
    assert_eq!(resolved, absolute);
  }

  #[test]
  fn asset_manager_set_base_path() {
    let mut assets = AssetManager::new();
    assets.set_base_path("new_assets");
    assert_eq!(assets.base_path(), Path::new("new_assets"));

    let resolved = assets.resolve_path("sprite.png");
    assert_eq!(resolved, PathBuf::from("new_assets/sprite.png"));
  }

  #[test]
  fn asset_manager_default() {
    let assets = AssetManager::default();
    assert_eq!(assets.base_path(), Path::new("."));
  }
}
