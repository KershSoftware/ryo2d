//! Asset management system for loading and caching game resources
//!
//! This module provides:
//! - `AssetManager` - Central hub for loading and caching assets
//! - `SpriteHandle` - Shared reference to loaded sprite data
//! - Automatic deduplication - same file loaded once, shared many times
//! - Hot reloading support for development iteration
//!
//! # Example
//!
//! ```rust,ignore
//! // Load assets through the manager
//! let player = assets.load_sprite("sprites/player.png")?;
//! let enemy = assets.load_sprite("sprites/enemy.png")?;
//!
//! // Handles are cheap to clone (Arc-backed)
//! let player2 = player.clone(); // No extra memory allocation
//! ```
//!
//! The engine's manager is a resource (`ctx.assets` in scene callbacks); it
//! finishes async loads and applies hot reloads each frame in `PreUpdate`.

use std::collections::HashMap;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, RwLock};
#[cfg(not(target_arch = "wasm32"))]
use std::thread::{self, JoinHandle};

use crossbeam_channel::{Receiver, Sender, unbounded};

use crate::core::ecs::{Resource, ShaderHandle};
use crate::core::error::{AssetError, AssetResult};
use crate::core::graphics::Sprite;
use crate::core::ui::file::build::{Placeholders, SpriteSource};
use crate::core::ui::file::document::ImportResolver;
use crate::core::ui::file::{UiFile, UiFileError};

use super::hot_reload::{HotReloadEvent, HotReloadWatcher};
#[allow(unused_imports)] // engine_info only used in debug builds
use crate::{engine_error, engine_info, engine_warn};

/// A handle to a loaded sprite asset
///
/// Handles are cheap to clone - they use `Arc` internally for shared ownership.
/// When all handles to an asset are dropped, the asset data is freed.
///
/// The handle uses `RwLock` internally to support hot reloading - the sprite
/// data can be updated while the game is running.
///
/// # Example
///
/// ```rust,ignore
/// let sprite = assets.load_sprite("player.png")?;
///
/// // Clone is cheap - just increments reference count
/// let sprite2 = sprite.clone();
///
/// // Both handles point to the same sprite data
/// assert_eq!(sprite.width(), sprite2.width());
/// ```
#[derive(Clone)]
pub struct SpriteHandle {
  inner: Arc<RwLock<Sprite>>,
  /// The path this sprite was loaded from (for debugging/reloading)
  path: Arc<PathBuf>,
  /// Process-unique identity shared by every clone of this handle
  serial: usize,
  /// Bumped on every write to the sprite, shared by every clone
  generation: Arc<AtomicU64>,
}

/// Source of [`SpriteHandle::serial`] values.
static NEXT_SERIAL: AtomicUsize = AtomicUsize::new(1);

impl SpriteHandle {
  /// Create a new sprite handle from a sprite
  pub fn new(sprite: Sprite, path: PathBuf) -> Self {
    SpriteHandle {
      inner: Arc::new(RwLock::new(sprite)),
      path: Arc::new(path),
      serial: NEXT_SERIAL.fetch_add(1, Ordering::Relaxed),
      generation: Arc::new(AtomicU64::new(0)),
    }
  }

  /// A process-unique identity, shared by clones of this handle.
  ///
  /// Unlike [`id`](Self::id) (the path), two handles made from the same path
  /// have different serials, and a serial is never reused after a handle is
  /// dropped.
  pub(crate) fn serial(&self) -> usize {
    self.serial
  }

  /// How many times the sprite's pixels were replaced (0 for a fresh handle).
  ///
  /// With [`serial`](Self::serial) it names one version of the sprite's
  /// content, so a cache can tell when its copy is stale.
  pub(crate) fn generation(&self) -> u64 {
    self.generation.load(Ordering::Acquire)
  }

  /// Create a sprite handle from an existing sprite with a name
  ///
  /// Convenience method that accepts any path-like type.
  pub fn from_sprite<P: AsRef<Path>>(sprite: Sprite, name: P) -> Self {
    Self::new(sprite, name.as_ref().to_path_buf())
  }

  /// Get the sprite width
  pub fn width(&self) -> u32 {
    self
      .inner
      .read()
      .expect("SpriteHandle RwLock poisoned")
      .width()
  }

  /// Get the sprite height
  pub fn height(&self) -> u32 {
    self
      .inner
      .read()
      .expect("SpriteHandle RwLock poisoned")
      .height()
  }

  /// Get the path this sprite was loaded from
  pub fn path(&self) -> &Path {
    &self.path
  }

  /// Get a unique identifier for this sprite (for caching/batching)
  ///
  /// Returns the path as a string, which is unique for each loaded sprite.
  pub fn id(&self) -> String {
    self.path.to_string_lossy().to_string()
  }

  /// Get a reference to the underlying sprite data
  ///
  /// This acquires a read lock on the sprite. For rendering, prefer
  /// using `with_sprite()` which handles the lock scope properly.
  pub fn sprite(&self) -> std::sync::RwLockReadGuard<'_, Sprite> {
    self.inner.read().expect("SpriteHandle RwLock poisoned")
  }

  /// Execute a closure with read access to the sprite
  ///
  /// This is the preferred way to access sprite data for rendering,
  /// as it ensures the lock is released promptly.
  ///
  /// # Example
  ///
  /// ```rust,ignore
  /// handle.with_sprite(|sprite| {
  ///     sprite.draw(frame, width, height, x, y);
  /// });
  /// ```
  pub fn with_sprite<F, R>(&self, f: F) -> R
  where
    F: FnOnce(&Sprite) -> R,
  {
    let sprite = self.inner.read().expect("SpriteHandle RwLock poisoned");
    f(&sprite)
  }

  /// Update the sprite data (used by hot reload system)
  ///
  /// This replaces the sprite data with new data. All handles
  /// pointing to this sprite will see the updated data.
  pub(crate) fn update(&self, new_sprite: Sprite) {
    let mut sprite = self.inner.write().expect("SpriteHandle RwLock poisoned");
    *sprite = new_sprite;
    // Bumped while the write lock is held, so a reader never sees new pixels
    // under the old generation
    self.generation.fetch_add(1, Ordering::Release);
  }

  /// Check how many handles reference this sprite
  ///
  /// Useful for debugging memory usage.
  pub fn ref_count(&self) -> usize {
    Arc::strong_count(&self.inner)
  }
}

/// The same image: two handles share one cell (the asset cache returns the
/// same cell for one path)
impl PartialEq for SpriteHandle {
  fn eq(&self, other: &Self) -> bool {
    Arc::ptr_eq(&self.inner, &other.inner)
  }
}

impl Eq for SpriteHandle {}

impl std::fmt::Debug for SpriteHandle {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    f.debug_struct("SpriteHandle")
      .field("path", &self.path)
      .field("width", &self.width())
      .field("height", &self.height())
      .field("ref_count", &self.ref_count())
      .finish()
  }
}

// ============================================================================
// Async Loading Types
// ============================================================================

/// The loading state of an async asset
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoadingState {
  /// Asset is being loaded in the background
  Loading,
  /// Asset loaded successfully
  Loaded,
  /// Asset failed to load
  Failed(String),
}

/// A handle to an asynchronously loading sprite
///
/// This handle is returned immediately when you request an async load.
/// The actual sprite data loads in a background thread.
///
/// # Example
///
/// ```rust,ignore
/// // Request async load - returns immediately
/// let pending = assets.load_sprite_async("large_texture.png");
///
/// // Later, check if loaded
/// match pending.state() {
///     LoadingState::Loading => {
///         // Still loading, maybe show placeholder
///     }
///     LoadingState::Loaded => {
///         // Safe to use the sprite
///         let sprite = pending.get().unwrap();
///     }
///     LoadingState::Failed(reason) => {
///         eprintln!("Load failed: {}", reason);
///     }
/// }
/// ```
#[derive(Clone)]
pub struct AsyncSpriteHandle {
  /// The actual sprite (None until loaded)
  inner: Arc<RwLock<Option<SpriteHandle>>>,
  /// Current loading state
  state: Arc<RwLock<LoadingState>>,
  /// Path being loaded
  path: Arc<PathBuf>,
}

impl AsyncSpriteHandle {
  /// Create a new pending async handle
  fn new_pending(path: PathBuf) -> Self {
    AsyncSpriteHandle {
      inner: Arc::new(RwLock::new(None)),
      state: Arc::new(RwLock::new(LoadingState::Loading)),
      path: Arc::new(path),
    }
  }

  /// Get the current loading state
  pub fn state(&self) -> LoadingState {
    self
      .state
      .read()
      .expect("AsyncSpriteHandle state RwLock poisoned")
      .clone()
  }

  /// Check if the asset is still loading
  pub fn is_loading(&self) -> bool {
    matches!(
      *self
        .state
        .read()
        .expect("AsyncSpriteHandle state RwLock poisoned"),
      LoadingState::Loading
    )
  }

  /// Check if the asset has finished loading (success or failure)
  pub fn is_complete(&self) -> bool {
    !self.is_loading()
  }

  /// Check if the asset loaded successfully
  pub fn is_loaded(&self) -> bool {
    matches!(
      *self
        .state
        .read()
        .expect("AsyncSpriteHandle state RwLock poisoned"),
      LoadingState::Loaded
    )
  }

  /// Check if the asset failed to load
  pub fn is_failed(&self) -> bool {
    matches!(
      *self
        .state
        .read()
        .expect("AsyncSpriteHandle state RwLock poisoned"),
      LoadingState::Failed(_)
    )
  }

  /// Get the loaded sprite, if available
  ///
  /// Returns `None` if still loading or failed.
  pub fn get(&self) -> Option<SpriteHandle> {
    self
      .inner
      .read()
      .expect("AsyncSpriteHandle inner RwLock poisoned")
      .clone()
  }

  /// Get the loaded sprite, or a fallback if not ready
  ///
  /// Useful for rendering while loading.
  pub fn get_or(&self, fallback: &SpriteHandle) -> SpriteHandle {
    self.get().unwrap_or_else(|| fallback.clone())
  }

  /// Get the path being loaded
  pub fn path(&self) -> &Path {
    &self.path
  }

  /// Internal: Mark as loaded with sprite data
  fn complete_success(&self, handle: SpriteHandle) {
    *self
      .inner
      .write()
      .expect("AsyncSpriteHandle inner RwLock poisoned") = Some(handle);
    *self
      .state
      .write()
      .expect("AsyncSpriteHandle state RwLock poisoned") = LoadingState::Loaded;
  }

  /// Internal: Mark as failed with error
  fn complete_failure(&self, reason: String) {
    *self
      .state
      .write()
      .expect("AsyncSpriteHandle state RwLock poisoned") = LoadingState::Failed(reason);
  }
}

impl std::fmt::Debug for AsyncSpriteHandle {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    f.debug_struct("AsyncSpriteHandle")
      .field("path", &self.path)
      .field("state", &self.state())
      .finish()
  }
}

/// Message sent from loader thread to main thread
struct LoadResult {
  path: PathBuf,
  result: Result<Sprite, String>,
}

/// Background loader for async asset loading
#[cfg(not(target_arch = "wasm32"))]
struct AsyncLoader {
  /// Sender for load requests
  request_tx: Sender<PathBuf>,
  /// Receiver for completed loads
  result_rx: Receiver<LoadResult>,
  /// Handle to loader thread
  _thread: JoinHandle<()>,
}

/// The web has no threads: loads run inline and wait in the channel for the
/// next poll
#[cfg(target_arch = "wasm32")]
struct AsyncLoader {
  result_tx: Sender<LoadResult>,
  result_rx: Receiver<LoadResult>,
}

#[cfg(target_arch = "wasm32")]
impl AsyncLoader {
  fn new() -> Self {
    let (result_tx, result_rx) = unbounded::<LoadResult>();
    AsyncLoader {
      result_tx,
      result_rx,
    }
  }

  fn request_load(&self, path: PathBuf) {
    let result = Sprite::load(&path).map_err(|e| e.to_string());
    let _ = self.result_tx.send(LoadResult { path, result });
  }
}

#[cfg(not(target_arch = "wasm32"))]
impl AsyncLoader {
  fn new() -> Self {
    let (request_tx, request_rx) = unbounded::<PathBuf>();
    let (result_tx, result_rx) = unbounded::<LoadResult>();

    let thread = thread::spawn(move || {
      // Process load requests until channel closes
      while let Ok(path) = request_rx.recv() {
        let result = Sprite::load(&path).map_err(|e| e.to_string());
        let _ = result_tx.send(LoadResult { path, result });
      }
    });

    AsyncLoader {
      request_tx,
      result_rx,
      _thread: thread,
    }
  }

  fn request_load(&self, path: PathBuf) {
    let _ = self.request_tx.send(path);
  }
}

impl AsyncLoader {
  fn poll_results(&self) -> Vec<LoadResult> {
    let mut results = Vec::new();
    while let Ok(result) = self.result_rx.try_recv() {
      results.push(result);
    }
    results
  }
}

// ============================================================================
// LoadingProgress
// ============================================================================

/// Progress information for batch asset loading.
///
/// Use this to display loading screens with progress bars.
///
/// # Example
///
/// ```rust,ignore
/// let progress = ctx.assets.loading_progress();
/// if progress.is_loading() {
///     let pct = progress.percentage();
///     ui.draw_progress_bar(rect, pct, Color::GREEN, Color::DARK_GRAY);
///     ui.draw_text(&format!("Loading: {}", progress.current_asset()), x, y, Color::WHITE);
/// }
/// ```
#[derive(Debug, Clone)]
pub struct LoadingProgress {
  /// Total number of assets queued for loading
  pub total: usize,
  /// Number of assets that have finished loading (success or failure)
  pub completed: usize,
  /// Number of assets that loaded successfully
  pub succeeded: usize,
  /// Number of assets that failed to load
  pub failed: usize,
  /// Path of the asset currently being loaded (if any)
  pub current: Option<String>,
  /// Paths of assets that failed to load
  pub failures: Vec<String>,
}

impl LoadingProgress {
  /// Create empty progress (nothing loading)
  pub fn none() -> Self {
    Self {
      total: 0,
      completed: 0,
      succeeded: 0,
      failed: 0,
      current: None,
      failures: Vec::new(),
    }
  }

  /// Check if any assets are currently loading
  pub fn is_loading(&self) -> bool {
    self.total > 0 && self.completed < self.total
  }

  /// Check if all assets have finished loading
  pub fn is_complete(&self) -> bool {
    self.total > 0 && self.completed >= self.total
  }

  /// Get loading progress as a percentage (0.0 to 1.0)
  ///
  /// Returns 1.0 if nothing is loading or all loads are complete.
  pub fn percentage(&self) -> f32 {
    if self.total == 0 {
      1.0
    } else {
      self.completed as f32 / self.total as f32
    }
  }

  /// Get the name of the asset currently being loaded
  ///
  /// Returns the filename without the full path for cleaner display.
  pub fn current_asset(&self) -> &str {
    self
      .current
      .as_ref()
      .map(|p| p.rsplit(['/', '\\']).next().unwrap_or(p))
      .unwrap_or("")
  }

  /// Check if any assets failed to load
  pub fn has_failures(&self) -> bool {
    self.failed > 0
  }
}

impl Default for LoadingProgress {
  fn default() -> Self {
    Self::none()
  }
}

// ============================================================================
// AssetManager
// ============================================================================

/// Central manager for loading and caching game assets
///
/// The `AssetManager` provides:
/// - Automatic caching - loading the same file twice returns the cached version
/// - Reference counting - assets stay loaded while handles exist
/// - Type-safe loading - different methods for different asset types
/// - Async loading - non-blocking loads for large assets
///
/// # Example
///
/// ```rust,ignore
/// let mut assets = AssetManager::new();
///
/// // Synchronous load (blocks until complete)
/// let player = assets.load_sprite("player.png")?;
///
/// // Async load (returns immediately)
/// let boss = assets.load_sprite_async("large_boss.png");
///
/// // Later, check if async load is complete
/// if boss.is_loaded() {
///     let sprite = boss.get().unwrap();
/// }
/// ```
#[derive(Resource)]
pub struct AssetManager {
  /// Base directory for resolving relative paths
  base_path: PathBuf,
  /// Cache of loaded sprites, keyed by canonical path
  sprites: HashMap<PathBuf, SpriteHandle>,
  /// Cache of loaded shaders, keyed by resolved path
  shaders: HashMap<PathBuf, ShaderHandle>,
  /// Hot reload watcher (optional, for development)
  watcher: Option<HotReloadWatcher>,
  /// Background loader for async loading
  async_loader: Option<AsyncLoader>,
  /// Pending async load handles, keyed by path
  pending_loads: HashMap<PathBuf, AsyncSpriteHandle>,
  /// Batch loading tracking: total assets requested in current batch
  batch_total: usize,
  /// Batch loading tracking: assets completed (success + failure)
  batch_completed: usize,
  /// Batch loading tracking: assets that succeeded
  batch_succeeded: usize,
  /// Batch loading tracking: paths that failed
  batch_failures: Vec<String>,
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
      sprites: HashMap::new(),
      shaders: HashMap::new(),
      watcher: None,
      async_loader: None,
      pending_loads: HashMap::new(),
      batch_total: 0,
      batch_completed: 0,
      batch_succeeded: 0,
      batch_failures: Vec::new(),
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
      sprites: HashMap::new(),
      shaders: HashMap::new(),
      watcher: None,
      async_loader: None,
      pending_loads: HashMap::new(),
      batch_total: 0,
      batch_completed: 0,
      batch_succeeded: 0,
      batch_failures: Vec::new(),
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

  /// Load a sprite from a file
  ///
  /// If the sprite was previously loaded, returns the cached handle.
  /// Otherwise, loads from disk and caches the result.
  ///
  /// # Arguments
  ///
  /// * `path` - Path to the image file (PNG, JPEG, etc.)
  ///
  /// # Returns
  ///
  /// A handle to the loaded sprite, or an error if loading failed.
  ///
  /// # Example
  ///
  /// ```rust,ignore
  /// let sprite = assets.load_sprite("sprites/player.png")?;
  /// println!("Loaded {}x{} sprite", sprite.width(), sprite.height());
  /// ```
  pub fn load_sprite<P: AsRef<Path>>(&mut self, path: P) -> AssetResult<SpriteHandle> {
    let full_path = self.resolve_path(&path);
    let key = self.cache_key(&path);

    // Check cache first
    if let Some(handle) = self.sprites.get(&key) {
      return Ok(handle.clone());
    }

    // Load from disk
    let sprite = Sprite::load(&full_path).map_err(AssetError::Sprite)?;

    // Create handle and cache it
    let handle = SpriteHandle::new(sprite, full_path.clone());
    self.sprites.insert(key, handle.clone());

    Ok(handle)
  }

  /// Load a custom WGSL shader from a file
  ///
  /// Cached by resolved path, so loading the same file twice returns clones of
  /// one handle. With hot reload enabled, editing the file updates the handle
  /// in place and every sprite using it picks up the new code.
  ///
  /// The source is compiled lazily by the renderer; WGSL errors are reported
  /// in the dev log with the line numbers of the game's own file.
  ///
  /// # Example
  ///
  /// ```rust,ignore
  /// let dissolve = assets.load_shader("shaders/dissolve.wgsl")?;
  /// let material = Material::custom(dissolve).with_param("progress", 0.5);
  /// ```
  pub fn load_shader<P: AsRef<Path>>(&mut self, path: P) -> AssetResult<ShaderHandle> {
    let full_path = self.resolve_path(&path);
    let key = self.cache_key(&path);

    if let Some(handle) = self.shaders.get(&key) {
      return Ok(handle.clone());
    }

    let source = std::fs::read_to_string(&full_path).map_err(|e| match e.kind() {
      std::io::ErrorKind::NotFound => AssetError::NotFound(full_path.clone()),
      _ => AssetError::LoadError {
        path: full_path.clone(),
        reason: e.to_string(),
      },
    })?;

    let handle = ShaderHandle::from_file(&full_path, source);
    self.shaders.insert(key, handle.clone());
    Ok(handle)
  }

  /// Read a file's text, as an asset error
  fn read_text(full_path: &Path) -> AssetResult<String> {
    std::fs::read_to_string(full_path).map_err(|e| match e.kind() {
      std::io::ErrorKind::NotFound => AssetError::NotFound(full_path.to_path_buf()),
      _ => AssetError::LoadError {
        path: full_path.to_path_buf(),
        reason: e.to_string(),
      },
    })
  }

  /// Load a UI file (`.ui.ron`)
  ///
  /// Cached by resolved path, so loading the same file twice returns the same
  /// [`UiFile`]. Its imports load through the cache too. Every mistake in the
  /// file is reported together in [`AssetError::UiFile`]. With hot reload
  /// enabled, editing the file updates the `UiFile` in place.
  pub fn load_ui<P: AsRef<Path>>(&mut self, path: P) -> AssetResult<UiFile> {
    let key = self.cache_key(&path);
    if let Some(file) = self.ui_files.get(&key) {
      return Ok(file.clone());
    }
    let text = Self::read_text(&self.resolve_path(&path))?;
    let name = path.as_ref().display().to_string();
    self.ui_loading.push(key.clone());
    let loaded = UiFile::load(&name, &text, self, &mut Placeholders::default(), false);
    self.ui_loading.pop();
    let file = loaded.map_err(AssetError::UiFile)?;
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
  pub fn reload_ui<P: AsRef<Path>>(&mut self, path: P) -> AssetResult<()> {
    let key = self.cache_key(&path);
    let Some(file) = self.ui_files.get(&key).cloned() else {
      return Err(AssetError::NotFound(self.resolve_path(&path)));
    };
    if file.is_embedded() {
      return Ok(());
    }
    let text = Self::read_text(&key)?;
    self
      .reload_ui_text(&key, &file, &text)
      .map_err(AssetError::UiFile)
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

  /// Create a sprite handle from an existing sprite (not from a file)
  ///
  /// This is useful for programmatically generated sprites.
  /// The sprite will NOT be cached since it has no file path.
  ///
  /// # Example
  ///
  /// ```rust,ignore
  /// let sprite = Sprite::solid_rect(32, 32, 255, 0, 0, 255);
  /// let handle = assets.create_sprite(sprite, "generated/red_square");
  /// ```
  pub fn create_sprite<P: AsRef<Path>>(&self, sprite: Sprite, name: P) -> SpriteHandle {
    SpriteHandle::new(sprite, name.as_ref().to_path_buf())
  }

  // ==========================================================================
  // Async Loading
  // ==========================================================================

  /// Load a sprite asynchronously in a background thread
  ///
  /// This method returns immediately with an `AsyncSpriteHandle` that can
  /// be polled for completion. The actual loading happens in a background
  /// thread, preventing frame hitches for large assets.
  ///
  /// The engine calls `process_async_loads()` each frame (`PreUpdate`) to
  /// complete pending loads and update handles.
  ///
  /// # Example
  ///
  /// ```rust,ignore
  /// // Request async load - returns immediately
  /// let boss = assets.load_sprite_async("large_boss_texture.png");
  ///
  /// // Store the handle
  /// self.boss_sprite = Some(boss);
  ///
  /// // In update: check if loaded before using
  /// if let Some(boss) = &self.boss_sprite {
  ///     if boss.is_loaded() {
  ///         let sprite = boss.get().unwrap();
  ///         // Use sprite for rendering
  ///     } else if boss.is_loading() {
  ///         // Show loading indicator
  ///     }
  /// }
  /// ```
  pub fn load_sprite_async<P: AsRef<Path>>(&mut self, path: P) -> AsyncSpriteHandle {
    let full_path = self.resolve_path(&path);

    // If already cached, return immediately as loaded
    if let Some(handle) = self.sprites.get(&self.cache_key(&path)) {
      let async_handle = AsyncSpriteHandle::new_pending(full_path);
      async_handle.complete_success(handle.clone());
      return async_handle;
    }

    // If already pending, return existing handle
    if let Some(pending) = self.pending_loads.get(&full_path) {
      return pending.clone();
    }

    // Create async loader if needed
    if self.async_loader.is_none() {
      self.async_loader = Some(AsyncLoader::new());
    }

    // Create pending handle and request load
    let async_handle = AsyncSpriteHandle::new_pending(full_path.clone());
    self
      .pending_loads
      .insert(full_path.clone(), async_handle.clone());

    if let Some(loader) = &self.async_loader {
      loader.request_load(full_path);
    }
    // The web loaded it inline: hand back a completed handle
    #[cfg(target_arch = "wasm32")]
    self.process_async_loads();

    async_handle
  }

  /// Process completed async loads
  ///
  /// Checks for completed background loads and updates their handles.
  /// Returns the number of loads completed. The engine calls it once per
  /// frame in `PreUpdate`, so games only need it when driving an
  /// `AssetManager` of their own.
  pub fn process_async_loads(&mut self) -> usize {
    let loader = match &self.async_loader {
      Some(l) => l,
      None => return 0,
    };

    let results = loader.poll_results();
    let count = results.len();

    for result in results {
      if let Some(pending) = self.pending_loads.remove(&result.path) {
        match result.result {
          Ok(sprite) => {
            // Create handle and cache it
            let handle = SpriteHandle::new(sprite, result.path.clone());
            let key = std::fs::canonicalize(&result.path).unwrap_or(result.path);
            self.sprites.insert(key, handle.clone());

            // Update the async handle
            pending.complete_success(handle);

            // Update batch tracking
            self.batch_completed += 1;
            self.batch_succeeded += 1;
          }
          Err(reason) => {
            // Track failure for batch progress
            self.batch_completed += 1;
            self
              .batch_failures
              .push(result.path.to_string_lossy().to_string());

            pending.complete_failure(reason);
          }
        }
      }
    }

    count
  }

  /// Get the number of assets currently loading
  pub fn pending_load_count(&self) -> usize {
    self.pending_loads.len()
  }

  /// Check if any async loads are in progress
  pub fn has_pending_loads(&self) -> bool {
    !self.pending_loads.is_empty()
  }

  // ==========================================================================
  // Cache Management
  // ==========================================================================

  /// Check if a sprite is already cached
  pub fn is_sprite_cached<P: AsRef<Path>>(&self, path: P) -> bool {
    self.sprites.contains_key(&self.cache_key(path))
  }

  /// Get the number of cached sprites
  pub fn sprite_count(&self) -> usize {
    self.sprites.len()
  }

  /// Remove unused sprites from the cache
  ///
  /// A sprite is considered "unused" if the only reference to it
  /// is the cache itself (ref_count == 1).
  ///
  /// Returns the number of sprites removed.
  pub fn gc_sprites(&mut self) -> usize {
    let before = self.sprites.len();
    self.sprites.retain(|_, handle| handle.ref_count() > 1);
    before - self.sprites.len()
  }

  /// Clear all cached sprites
  ///
  /// Note: Existing handles will still work, but reloading the same
  /// path will trigger a new disk read.
  pub fn clear_sprite_cache(&mut self) {
    self.sprites.clear();
  }

  /// Add a pre-loaded sprite handle to the cache.
  ///
  /// This is useful when you've loaded a sprite outside the normal
  /// asset manager flow and want to register it for cache lookups.
  ///
  /// # Example
  ///
  /// ```rust,ignore
  /// // Load sprite manually (e.g., in a loading scene)
  /// let handle = SpriteHandle::from_sprite(sprite, "assets/bg.png");
  ///
  /// // Register with asset manager for later lookups
  /// assets.cache_sprite_handle(handle);
  ///
  /// // Now load_sprite will return the cached version instantly
  /// let bg = assets.load_sprite("assets/bg.png")?; // No disk I/O!
  /// ```
  pub fn cache_sprite_handle(&mut self, handle: SpriteHandle) {
    let key = self.cache_key(handle.path());
    self.sprites.insert(key, handle);
  }

  // ==========================================================================
  // Batch Loading
  // ==========================================================================

  /// Start loading multiple assets as a batch.
  ///
  /// This queues all provided paths for async loading and initializes
  /// the progress tracking. Use `loading_progress()` to monitor progress
  /// and display loading screens.
  ///
  /// The engine completes loads each frame (`process_async_loads()` in
  /// `PreUpdate`).
  ///
  /// # Example
  ///
  /// ```rust,ignore
  /// // In your loading scene's on_enter:
  /// ctx.assets.start_loading(&[
  ///     "assets/bg.png",
  ///     "assets/player.png",
  ///     "assets/enemy.png",
  /// ]);
  ///
  /// // In update:
  /// let progress = ctx.assets.loading_progress();
  /// if progress.is_complete() {
  ///     return SceneAction::Switch(Box::new(GameScene::new()));
  /// }
  /// ```
  pub fn start_loading<P, I>(&mut self, paths: I)
  where
    P: AsRef<Path>,
    I: IntoIterator<Item = P>,
  {
    // Reset batch tracking
    self.batch_total = 0;
    self.batch_completed = 0;
    self.batch_succeeded = 0;
    self.batch_failures.clear();

    // Queue all assets for loading
    for path in paths {
      // Skip already-cached assets (they're already loaded)
      if self.sprites.contains_key(&self.cache_key(&path)) {
        // Count as already completed
        self.batch_total += 1;
        self.batch_completed += 1;
        self.batch_succeeded += 1;
        continue;
      }

      // Queue for async loading
      self.load_sprite_async(path);
      self.batch_total += 1;
    }
  }

  /// Get current loading progress.
  ///
  /// Returns a `LoadingProgress` struct with information about the current
  /// batch loading operation. Use this to display loading screens.
  pub fn loading_progress(&self) -> LoadingProgress {
    // Find currently loading asset (first pending)
    let current = self
      .pending_loads
      .keys()
      .next()
      .map(|p| p.to_string_lossy().to_string());

    LoadingProgress {
      total: self.batch_total,
      completed: self.batch_completed,
      succeeded: self.batch_succeeded,
      failed: self.batch_failures.len(),
      current,
      failures: self.batch_failures.clone(),
    }
  }

  /// Check if batch loading is complete.
  ///
  /// Shorthand for `loading_progress().is_complete()`.
  pub fn is_loading_complete(&self) -> bool {
    self.batch_total > 0 && self.batch_completed >= self.batch_total
  }

  /// Reset batch loading tracking.
  ///
  /// Call this when starting a new loading sequence or when you want
  /// to clear the progress state.
  pub fn reset_loading_progress(&mut self) {
    self.batch_total = 0;
    self.batch_completed = 0;
    self.batch_succeeded = 0;
    self.batch_failures.clear();
  }

  // ==========================================================================
  // Statistics
  // ==========================================================================

  /// Get cache statistics for debugging
  pub fn stats(&self) -> AssetStats {
    AssetStats {
      sprites_cached: self.sprites.len(),
      sprites_total_refs: self.sprites.values().map(|h| h.ref_count()).sum(),
      sprites_loading: self.pending_loads.len(),
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
  pub fn enable_hot_reload<P, I>(&mut self, watch_paths: I) -> AssetResult<()>
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
      HotReloadWatcher::new(config).map_err(|e| AssetError::WatchError(e.to_string()))?;

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
        if let Some(handle) = self.shaders.get(&key) {
          match std::fs::read_to_string(&key) {
            Ok(source) => {
              handle.replace_source(source);
              reloaded = true;
              self.reload_outcomes.push(Ok(key.clone()));
              #[cfg(debug_assertions)]
              engine_info!(module: "HotReload", "Reloaded shader: {:?}", key);
            }
            Err(e) => {
              engine_error!(module: "HotReload", "Failed to reload {:?}: {}", key, e);
              self.reload_outcomes.push(Err((key.clone(), e.to_string())));
            }
          }
        }
        if let Some(handle) = self.sprites.get(&key) {
          match Sprite::load(&key) {
            Ok(new_sprite) => {
              handle.update(new_sprite);
              reloaded = true;
              self.reload_outcomes.push(Ok(key.clone()));
              #[cfg(debug_assertions)]
              engine_info!(module: "HotReload", "Reloaded: {:?}", key);
            }
            Err(e) => {
              engine_error!(module: "HotReload", "Failed to reload {:?}: {}", key, e);
              self.reload_outcomes.push(Err((key.clone(), e.to_string())));
            }
          }
        }
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
      AssetError::UiFile(error) => error,
      other => UiFileError::one(path, other.to_string()),
    })
  }
}

/// UI files load their images as sprites, resolved and cached as any other
impl SpriteSource for AssetManager {
  fn sprite(&mut self, path: &str) -> Result<SpriteHandle, String> {
    self.load_sprite(path).map_err(|e| e.to_string())
  }
}

/// Statistics about cached assets
#[derive(Debug, Clone, Copy)]
pub struct AssetStats {
  /// Number of sprites in the cache
  pub sprites_cached: usize,
  /// Total reference count across all cached sprites
  pub sprites_total_refs: usize,
  /// Number of sprites currently loading asynchronously
  pub sprites_loading: usize,
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
      Err(AssetError::UiFile(_))
    ));
    assert!(matches!(
      assets.load_ui("nope.ui.ron"),
      Err(AssetError::NotFound(_))
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
    let Err(AssetError::UiFile(error)) = assets.load_ui("a.ui.ron") else {
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
    let Err(AssetError::UiFile(error)) = assets.reload_ui("b.ui.ron") else {
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
  fn shader_reload_matches_an_absolute_event_path() {
    let (dir, mut assets) = event_fixture();
    std::fs::create_dir_all(dir.path().join("assets/shaders")).expect("mkdir");
    let file = dir.path().join("assets/shaders/x.wgsl");
    std::fs::write(&file, "old").expect("write");
    let handle = assets.load_shader("assets/shaders/x.wgsl").expect("load");
    let before = handle.generation();

    std::fs::write(&file, "new").expect("rewrite");
    let absolute = std::fs::canonicalize(&file).expect("canonical");

    assert!(assets.handle_reload_event(HotReloadEvent::FileModified(absolute)));
    assert_eq!(handle.source(), "new");
    assert!(handle.generation() > before);
  }

  #[test]
  fn sprite_reload_matches_an_absolute_event_path() {
    let (dir, mut assets) = event_fixture();
    std::fs::create_dir_all(dir.path().join("assets")).expect("mkdir");
    let file = dir.path().join("assets/s.png");
    image::RgbaImage::new(2, 2).save(&file).expect("save png");
    let handle = assets.load_sprite("assets/s.png").expect("load");
    let before = handle.generation();

    image::RgbaImage::new(4, 3).save(&file).expect("resave png");
    let absolute = std::fs::canonicalize(&file).expect("canonical");

    assert!(assets.handle_reload_event(HotReloadEvent::FileModified(absolute)));
    assert_eq!((handle.width(), handle.height()), (4, 3));
    assert!(handle.generation() > before);
  }

  #[test]
  fn reload_also_matches_a_relative_event_path() {
    let (dir, mut assets) = event_fixture();
    let file = dir.path().join("r.wgsl");
    std::fs::write(&file, "old").expect("write");
    let handle = assets.load_shader("r.wgsl").expect("load");

    std::fs::write(&file, "new").expect("rewrite");

    assert!(assets.handle_reload_event(HotReloadEvent::FileModified(file)));
    assert_eq!(handle.source(), "new");
  }

  #[test]
  fn load_shader_caches_by_path() {
    let dir = tempfile::tempdir().expect("temp dir");
    std::fs::write(dir.path().join("a.wgsl"), "fn sprite() {}").expect("write shader");
    let mut assets = AssetManager::with_base_path(dir.path());

    let first = assets.load_shader("a.wgsl").expect("load shader");
    let second = assets.load_shader("a.wgsl").expect("load shader again");

    assert_eq!(first, second);
    assert_eq!(first.source(), "fn sprite() {}");
    assert!(first.id().ends_with("a.wgsl"));
    first.replace_source("changed");
    assert_eq!(second.source(), "changed");
  }

  #[test]
  fn missing_shader_file_is_an_error() {
    let dir = tempfile::tempdir().expect("temp dir");
    let mut assets = AssetManager::with_base_path(dir.path());

    assert!(matches!(
      assets.load_shader("nope.wgsl"),
      Err(AssetError::NotFound(_))
    ));
  }

  #[test]
  fn sprite_handle_clone_shares_data() {
    let sprite = Sprite::solid_rect(10, 10, 255, 0, 0, 255);
    let handle1 = SpriteHandle::new(sprite, PathBuf::from("test.png"));

    assert_eq!(handle1.ref_count(), 1);

    let handle2 = handle1.clone();
    assert_eq!(handle1.ref_count(), 2);
    assert_eq!(handle2.ref_count(), 2);

    drop(handle2);
    assert_eq!(handle1.ref_count(), 1);
  }

  #[test]
  fn sprite_handle_dimensions() {
    let sprite = Sprite::solid_rect(64, 32, 255, 128, 0, 255);
    let handle = SpriteHandle::new(sprite, PathBuf::from("test.png"));

    assert_eq!(handle.width(), 64);
    assert_eq!(handle.height(), 32);
    assert_eq!(handle.path(), Path::new("test.png"));
  }

  #[test]
  fn sprite_handle_update() {
    let sprite1 = Sprite::solid_rect(10, 10, 255, 0, 0, 255);
    let handle = SpriteHandle::new(sprite1, PathBuf::from("test.png"));
    assert_eq!(handle.width(), 10);

    let sprite2 = Sprite::solid_rect(20, 20, 0, 255, 0, 255);
    handle.update(sprite2);
    assert_eq!(handle.width(), 20);
    assert_eq!(handle.height(), 20);
  }

  #[test]
  fn sprite_handle_identity_and_generation() {
    let make = || {
      SpriteHandle::new(
        Sprite::solid_rect(4, 4, 1, 2, 3, 255),
        PathBuf::from("same"),
      )
    };
    let handle = make();
    let other = make();
    let clone = handle.clone();

    // Same path, different sprites; clones share the identity
    assert_ne!(handle.serial(), other.serial());
    assert_eq!(handle.serial(), clone.serial());

    assert_eq!(handle.generation(), 0);
    handle.update(Sprite::solid_rect(4, 4, 9, 9, 9, 255));
    // Every clone sees the write; other handles do not
    assert_eq!(handle.generation(), 1);
    assert_eq!(clone.generation(), 1);
    assert_eq!(other.generation(), 0);
  }

  #[test]
  fn sprite_handle_from_sprite() {
    let sprite = Sprite::solid_rect(50, 50, 100, 100, 100, 255);
    let handle = SpriteHandle::from_sprite(sprite, "named_sprite");

    assert_eq!(handle.path(), Path::new("named_sprite"));
    assert_eq!(handle.width(), 50);
  }

  #[test]
  fn sprite_handle_id_matches_path() {
    let sprite = Sprite::solid_rect(10, 10, 255, 0, 0, 255);
    let handle = SpriteHandle::new(sprite, PathBuf::from("assets/test.png"));

    assert_eq!(handle.id(), "assets/test.png");
  }

  #[test]
  fn asset_manager_creates_sprite_handle() {
    let assets = AssetManager::new();
    let sprite = Sprite::solid_rect(32, 32, 0, 255, 0, 255);
    let handle = assets.create_sprite(sprite, "generated/green");

    assert_eq!(handle.width(), 32);
    assert_eq!(handle.height(), 32);
    assert_eq!(handle.path(), Path::new("generated/green"));
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
    assert_eq!(assets.sprite_count(), 0);
  }

  #[test]
  fn asset_manager_stats() {
    let assets = AssetManager::new();
    let sprite1 = Sprite::solid_rect(10, 10, 255, 0, 0, 255);
    let sprite2 = Sprite::solid_rect(20, 20, 0, 255, 0, 255);

    let handle1 = assets.create_sprite(sprite1, "test1");
    let _handle2 = assets.create_sprite(sprite2, "test2");
    let _handle1_clone = handle1.clone();

    let stats = assets.stats();
    assert_eq!(stats.sprites_cached, 0); // create_sprite doesn't cache
    assert_eq!(stats.sprites_loading, 0);
  }

  #[test]
  fn asset_manager_gc_removes_unused() {
    let mut assets = AssetManager::new();
    let sprite = Sprite::solid_rect(10, 10, 255, 0, 0, 255);
    let handle = SpriteHandle::new(sprite, PathBuf::from("test.png"));

    // Manually insert into cache
    assets
      .sprites
      .insert(PathBuf::from("test.png"), handle.clone());
    assert_eq!(assets.sprite_count(), 1);

    // With external handle, ref_count = 2, should not be removed
    assert_eq!(assets.gc_sprites(), 0);
    assert_eq!(assets.sprite_count(), 1);

    // Drop external handle, ref_count = 1, should be removed
    drop(handle);
    assert_eq!(assets.gc_sprites(), 1);
    assert_eq!(assets.sprite_count(), 0);
  }

  #[test]
  fn asset_manager_gc_threshold() {
    let mut assets = AssetManager::new();
    let sprite = Sprite::solid_rect(10, 10, 255, 0, 0, 255);

    // Add sprites at ref_count = 1
    for i in 0..5 {
      let handle = SpriteHandle::new(sprite.clone(), PathBuf::from(format!("test{}.png", i)));
      assets
        .sprites
        .insert(PathBuf::from(format!("test{}.png", i)), handle);
    }

    assert_eq!(assets.sprite_count(), 5);

    // Default threshold is 1 - should remove all
    assert_eq!(assets.gc_sprites(), 5);
    assert_eq!(assets.sprite_count(), 0);
  }

  #[test]
  fn asset_manager_clear_cache() {
    let mut assets = AssetManager::new();
    let sprite = Sprite::solid_rect(10, 10, 255, 0, 0, 255);

    let handle = SpriteHandle::new(sprite, PathBuf::from("test.png"));
    assets
      .sprites
      .insert(PathBuf::from("test.png"), handle.clone());

    assert_eq!(assets.sprite_count(), 1);
    assets.clear_sprite_cache();
    assert_eq!(assets.sprite_count(), 0);

    // Handle still works after cache clear
    assert_eq!(handle.width(), 10);
  }

  #[test]
  fn asset_manager_create_doesnt_cache() {
    let assets = AssetManager::new();
    let sprite = Sprite::solid_rect(10, 10, 255, 0, 0, 255);

    let _handle1 = assets.create_sprite(sprite.clone(), "test1");
    let _handle2 = assets.create_sprite(sprite.clone(), "test2");

    // create_sprite doesn't cache, only load_sprite does
    assert_eq!(assets.sprite_count(), 0);
  }

  #[test]
  fn async_sprite_handle_states() {
    let handle = AsyncSpriteHandle::new_pending(PathBuf::from("loading.png"));

    assert!(handle.is_loading());
    assert!(!handle.is_complete());
    assert!(!handle.is_loaded());
    assert!(!handle.is_failed());
    assert!(handle.get().is_none());
  }

  #[test]
  fn async_sprite_handle_success() {
    let handle = AsyncSpriteHandle::new_pending(PathBuf::from("test.png"));
    let sprite = Sprite::solid_rect(10, 10, 255, 0, 0, 255);
    let sprite_handle = SpriteHandle::new(sprite, PathBuf::from("test.png"));

    handle.complete_success(sprite_handle.clone());

    assert!(!handle.is_loading());
    assert!(handle.is_complete());
    assert!(handle.is_loaded());
    assert!(!handle.is_failed());
    assert!(handle.get().is_some());
    assert_eq!(handle.get().unwrap().width(), 10);
  }

  #[test]
  fn async_sprite_handle_failure() {
    let handle = AsyncSpriteHandle::new_pending(PathBuf::from("missing.png"));
    handle.complete_failure("File not found".to_string());

    assert!(!handle.is_loading());
    assert!(handle.is_complete());
    assert!(!handle.is_loaded());
    assert!(handle.is_failed());
    assert!(handle.get().is_none());

    match handle.state() {
      LoadingState::Failed(msg) => assert_eq!(msg, "File not found"),
      _ => panic!("Expected Failed state"),
    }
  }

  #[test]
  fn async_sprite_handle_get_or_fallback() {
    let loading = AsyncSpriteHandle::new_pending(PathBuf::from("loading.png"));
    let fallback = SpriteHandle::new(
      Sprite::solid_rect(5, 5, 128, 128, 128, 255),
      PathBuf::from("fallback.png"),
    );

    let result = loading.get_or(&fallback);
    assert_eq!(result.width(), 5); // Got fallback

    // Complete loading
    let loaded_sprite = SpriteHandle::new(
      Sprite::solid_rect(20, 20, 255, 255, 255, 255),
      PathBuf::from("loading.png"),
    );
    loading.complete_success(loaded_sprite);

    let result = loading.get_or(&fallback);
    assert_eq!(result.width(), 20); // Got loaded sprite
  }

  #[test]
  fn async_sprite_handle_path() {
    let handle = AsyncSpriteHandle::new_pending(PathBuf::from("assets/sprite.png"));
    assert_eq!(handle.path(), Path::new("assets/sprite.png"));
  }

  #[test]
  fn loading_state_clone() {
    let state1 = LoadingState::Failed("test".to_string());
    let state2 = state1.clone();

    match (state1, state2) {
      (LoadingState::Failed(msg1), LoadingState::Failed(msg2)) => {
        assert_eq!(msg1, msg2);
      }
      _ => panic!("Clone didn't preserve state"),
    }
  }

  #[test]
  fn loading_state_pattern_matching() {
    let loading = LoadingState::Loading;
    let loaded = LoadingState::Loaded;
    let failed = LoadingState::Failed("error".to_string());

    assert!(matches!(loading, LoadingState::Loading));
    assert!(matches!(loaded, LoadingState::Loaded));
    if let LoadingState::Failed(msg) = failed {
      assert_eq!(msg, "error");
    } else {
      panic!("Expected Failed state");
    }
  }
}
