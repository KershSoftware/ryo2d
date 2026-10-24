//! The old asset manager, until it goes: everything it loaded (images,
//! shaders, UI files) is a registry asset now
//! (`ctx.assets().load_now::<Image>(path)`, `load_now::<UiFile>(path)`);
//! `load_sprite` is a thin call to the registry, kept until the manager goes
//!
//! The engine's manager is a resource (`ctx.assets` in scene callbacks).

use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::core::ecs::{Resource, World};
use crate::core::error::{ManagerError, ManagerResult};
use crate::core::graphics::Image;

use super::hot_reload::{HotReloadEvent, HotReloadWatcher};
use super::{AssetError, AssetsView, Handle};
use crate::{engine_info, engine_warn};

// ============================================================================
// AssetManager
// ============================================================================

/// The old manager, kept until it goes. Images, shaders and UI files load
/// through the asset registry (`ctx.assets()`)
#[derive(Resource)]
pub struct AssetManager {
  /// Base directory for resolving relative paths
  base_path: PathBuf,
  /// Hot reload watcher (optional, for development)
  watcher: Option<HotReloadWatcher>,
  /// Hot reloads since the engine last took them: the path, or the path and
  /// the error
  reload_outcomes: Vec<Result<PathBuf, (PathBuf, String)>>,
}

impl AssetManager {
  /// Create a new asset manager with the current directory as base path
  pub fn new() -> Self {
    AssetManager {
      base_path: PathBuf::from("."),
      watcher: None,
      reload_outcomes: Vec::new(),
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

  /// Take the watcher's events. The registry reloads every asset the
  /// manager used to, so none reloads here and this returns 0; a watcher
  /// error is logged
  pub fn process_reloads(&mut self) -> usize {
    let Some(watcher) = &self.watcher else {
      return 0;
    };
    for event in watcher.poll() {
      if let HotReloadEvent::Error(e) = event {
        engine_warn!(module: "HotReload", "Watcher error: {}", e);
      }
    }
    0
  }

  /// The hot reloads since the last call: each reloaded path, or the path
  /// and why it failed.
  pub(crate) fn take_reload_outcomes(&mut self) -> Vec<Result<PathBuf, (PathBuf, String)>> {
    std::mem::take(&mut self.reload_outcomes)
  }
}

impl Default for AssetManager {
  fn default() -> Self {
    Self::new()
  }
}

#[cfg(test)]
mod tests {
  use super::*;

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
