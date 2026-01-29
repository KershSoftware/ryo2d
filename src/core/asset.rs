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
//!
//! // Hot reload: call each frame to apply pending changes
//! assets.process_reloads();
//! ```

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, RwLock};
use std::thread::{self, JoinHandle};

use crate::core::error::{AssetError, AssetResult};
use crate::core::hot_reload::{HotReloadEvent, HotReloadWatcher};
use crate::core::sprite::Sprite;

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
}

impl SpriteHandle {
  /// Create a new sprite handle from a sprite
  pub fn new(sprite: Sprite, path: PathBuf) -> Self {
    SpriteHandle {
      inner: Arc::new(RwLock::new(sprite)),
      path: Arc::new(path),
    }
  }

  /// Get the sprite width
  pub fn width(&self) -> u32 {
    self.inner.read().expect("SpriteHandle RwLock poisoned").width()
  }

  /// Get the sprite height
  pub fn height(&self) -> u32 {
    self.inner.read().expect("SpriteHandle RwLock poisoned").height()
  }

  /// Get the path this sprite was loaded from
  pub fn path(&self) -> &Path {
    &self.path
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
  }

  /// Check how many handles reference this sprite
  ///
  /// Useful for debugging memory usage.
  pub fn ref_count(&self) -> usize {
    Arc::strong_count(&self.inner)
  }
}

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
    self.state.read().expect("AsyncSpriteHandle state RwLock poisoned").clone()
  }

  /// Check if the asset is still loading
  pub fn is_loading(&self) -> bool {
    matches!(*self.state.read().expect("AsyncSpriteHandle state RwLock poisoned"), LoadingState::Loading)
  }

  /// Check if the asset has finished loading (success or failure)
  pub fn is_complete(&self) -> bool {
    !self.is_loading()
  }

  /// Check if the asset loaded successfully
  pub fn is_loaded(&self) -> bool {
    matches!(*self.state.read().expect("AsyncSpriteHandle state RwLock poisoned"), LoadingState::Loaded)
  }

  /// Check if the asset failed to load
  pub fn is_failed(&self) -> bool {
    matches!(*self.state.read().expect("AsyncSpriteHandle state RwLock poisoned"), LoadingState::Failed(_))
  }

  /// Get the loaded sprite, if available
  ///
  /// Returns `None` if still loading or failed.
  pub fn get(&self) -> Option<SpriteHandle> {
    self.inner.read().expect("AsyncSpriteHandle inner RwLock poisoned").clone()
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
    *self.inner.write().expect("AsyncSpriteHandle inner RwLock poisoned") = Some(handle);
    *self.state.write().expect("AsyncSpriteHandle state RwLock poisoned") = LoadingState::Loaded;
  }

  /// Internal: Mark as failed with error
  fn complete_failure(&self, reason: String) {
    *self.state.write().expect("AsyncSpriteHandle state RwLock poisoned") = LoadingState::Failed(reason);
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
struct AsyncLoader {
  /// Sender for load requests
  request_tx: Sender<PathBuf>,
  /// Receiver for completed loads
  result_rx: Receiver<LoadResult>,
  /// Handle to loader thread
  _thread: JoinHandle<()>,
}

impl AsyncLoader {
  fn new() -> Self {
    let (request_tx, request_rx) = channel::<PathBuf>();
    let (result_tx, result_rx) = channel::<LoadResult>();

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

  fn poll_results(&self) -> Vec<LoadResult> {
    let mut results = Vec::new();
    while let Ok(result) = self.result_rx.try_recv() {
      results.push(result);
    }
    results
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
pub struct AssetManager {
  /// Base directory for resolving relative paths
  base_path: PathBuf,
  /// Cache of loaded sprites, keyed by canonical path
  sprites: HashMap<PathBuf, SpriteHandle>,
  /// Hot reload watcher (optional, for development)
  watcher: Option<HotReloadWatcher>,
  /// Background loader for async loading
  async_loader: Option<AsyncLoader>,
  /// Pending async load handles, keyed by path
  pending_loads: HashMap<PathBuf, AsyncSpriteHandle>,
}

impl AssetManager {
  /// Create a new asset manager with the current directory as base path
  pub fn new() -> Self {
    AssetManager {
      base_path: PathBuf::from("."),
      sprites: HashMap::new(),
      watcher: None,
      async_loader: None,
      pending_loads: HashMap::new(),
    }
  }

  /// Create an asset manager with a specific base path
  ///
  /// All relative paths will be resolved from this base directory.
  pub fn with_base_path<P: AsRef<Path>>(base_path: P) -> Self {
    AssetManager {
      base_path: base_path.as_ref().to_path_buf(),
      sprites: HashMap::new(),
      watcher: None,
      async_loader: None,
      pending_loads: HashMap::new(),
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

    // Check cache first
    if let Some(handle) = self.sprites.get(&full_path) {
      return Ok(handle.clone());
    }

    // Load from disk
    let sprite = Sprite::load(&full_path).map_err(AssetError::Sprite)?;

    // Create handle and cache it
    let handle = SpriteHandle::new(sprite, full_path.clone());
    self.sprites.insert(full_path, handle.clone());

    Ok(handle)
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
  /// **Important:** You must call `process_async_loads()` each frame to
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
  /// // In update loop:
  /// assets.process_async_loads();
  ///
  /// // Check if loaded before using
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
    if let Some(handle) = self.sprites.get(&full_path) {
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

    async_handle
  }

  /// Process completed async loads
  ///
  /// Call this once per frame to check for completed background loads
  /// and update their handles. Returns the number of loads completed.
  ///
  /// This method is separate from `process_reloads()` - call both if
  /// using both async loading and hot reloading.
  ///
  /// # Example
  ///
  /// ```rust,ignore
  /// fn update(&mut self, ctx: &mut GameContext, _dt: f32) {
  ///     // Process async loads
  ///     let loaded = ctx.assets.process_async_loads();
  ///     if loaded > 0 {
  ///         println!("Loaded {} assets", loaded);
  ///     }
  ///     
  ///     // ... rest of update
  /// }
  /// ```
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
            self.sprites.insert(result.path, handle.clone());

            // Update the async handle
            pending.complete_success(handle);
          }
          Err(reason) => {
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
    let full_path = self.resolve_path(path);
    self.sprites.contains_key(&full_path)
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
  /// automatically reloads modified assets. Call `process_reloads()`
  /// each frame to apply pending changes.
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
  /// // In your game loop:
  /// let reloaded = assets.process_reloads();
  /// if reloaded > 0 {
  ///     println!("Reloaded {} assets", reloaded);
  /// }
  /// ```
  pub fn enable_hot_reload<P, I>(&mut self, watch_paths: I) -> AssetResult<()>
  where
    P: AsRef<Path>,
    I: IntoIterator<Item = P>,
  {
    use crate::core::hot_reload::HotReloadConfig;

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
  /// Call this once per frame to check for and apply asset changes.
  /// Returns the number of assets that were reloaded.
  ///
  /// # Example
  ///
  /// ```rust,ignore
  /// fn update(&mut self, ctx: &mut GameContext) {
  ///     // Process any pending hot reloads
  ///     ctx.assets.process_reloads();
  ///     
  ///     // ... rest of update
  /// }
  /// ```
  pub fn process_reloads(&mut self) -> usize {
    let watcher = match &self.watcher {
      Some(w) => w,
      None => return 0,
    };

    let mut reloaded = 0;

    // Process all pending events
    for event in watcher.poll() {
      match event {
        HotReloadEvent::FileModified(path) | HotReloadEvent::FileCreated(path) => {
          if let Some(handle) = self.sprites.get(&path) {
            // Reload the sprite from disk
            match Sprite::load(&path) {
              Ok(new_sprite) => {
                handle.update(new_sprite);
                reloaded += 1;
                #[cfg(debug_assertions)]
                eprintln!("[HotReload] Reloaded: {:?}", path);
              }
              Err(e) => {
                eprintln!("[HotReload] Failed to reload {:?}: {}", path, e);
              }
            }
          }
        }
        HotReloadEvent::FileDeleted(path) => {
          // Just log - don't remove from cache as handles may still be in use
          #[cfg(debug_assertions)]
          eprintln!("[HotReload] Asset removed (still cached): {:?}", path);
        }
        HotReloadEvent::Error(e) => {
          eprintln!("[HotReload] Watcher error: {}", e);
        }
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
}
