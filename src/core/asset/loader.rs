use std::any::{Any, TypeId};
use std::hash::Hash;
use std::sync::Arc;

use super::server::short_name;
use super::{Asset, AssetError, AssetPath, AssetServer, Handle, UntypedAssetId};
use crate::core::ecs::kernel::World;
use crate::core::hash::fast_hash;

/// A loader's settings: part of the asset's key, so one file loaded with two
/// settings is two assets
pub trait AssetSettings: Default + Clone + Hash + Eq + Send + Sync + 'static {}
impl<S: Default + Clone + Hash + Eq + Send + Sync + 'static> AssetSettings for S {}

/// Turns a file's bytes into an asset
pub trait AssetLoader: Send + Sync + 'static {
  /// What it makes
  type Asset: Asset;
  /// What it can be told; `()` if nothing
  type Settings: AssetSettings;

  /// The extensions this loader takes, without the leading dot ("png", "ui.ron")
  fn extensions(&self) -> &[&'static str];

  /// Makes the asset; dependencies are loaded through `cx`
  fn load(
    &self,
    bytes: Vec<u8>,
    settings: &Self::Settings,
    cx: &mut LoadContext<'_>,
  ) -> Result<Self::Asset, AssetError>;

  /// Checks the asset against its dependencies, once they are loaded and
  /// whenever one reloads (on the main thread). An error fails the load, or
  /// a reload, which keeps the last good value. The default accepts
  fn check(&self, asset: &Self::Asset, world: &World) -> Result<(), AssetError> {
    let _ = (asset, world);
    Ok(())
  }
}

/// What a loader can do while loading
pub struct LoadContext<'a> {
  path: &'a AssetPath,
  server: &'a AssetServer,
  dependencies: Vec<UntypedAssetId>,
  mode: LoadMode<'a>,
}

/// How a load runs: queued on the workers, or at once on the calling thread
pub(crate) enum LoadMode<'a> {
  /// On a worker: dependencies are queued too
  Queued,
  /// In `load_now`: dependencies load at once too, into `world`. `stack`
  /// holds the paths being loaded, outermost first, to catch a cycle
  Now {
    world: &'a mut World,
    stack: &'a mut Vec<AssetPath>,
  },
}

impl<'a> LoadContext<'a> {
  pub(crate) fn new(path: &'a AssetPath, server: &'a AssetServer, mode: LoadMode<'a>) -> Self {
    Self {
      path,
      server,
      dependencies: Vec::new(),
      mode,
    }
  }

  /// The dependencies loaded through this context
  pub(crate) fn into_dependencies(self) -> Vec<UntypedAssetId> {
    self.dependencies
  }

  /// The path of the asset being loaded
  pub fn path(&self) -> &AssetPath {
    self.path
  }

  /// Loads a dependency: the asset being loaded holds the handle and is ready
  /// only once this is. While `load_now` runs, dependencies load now too
  pub fn load<T: Asset>(&mut self, path: &str) -> Result<Handle<T>, AssetError> {
    self.load_request::<T, ()>(path, None)
  }

  /// Loads a dependency with settings, as [`LoadContext::load`] does
  pub fn load_with<T: Asset, S: AssetSettings>(
    &mut self,
    path: &str,
    settings: S,
  ) -> Result<Handle<T>, AssetError> {
    self.load_request(path, Some(settings))
  }

  fn load_request<T: Asset, S: AssetSettings>(
    &mut self,
    path: &str,
    settings: Option<S>,
  ) -> Result<Handle<T>, AssetError> {
    let handle = match &mut self.mode {
      LoadMode::Queued => self.server.queue_load::<T, S>(path, settings)?,
      LoadMode::Now { world, stack } => self
        .server
        .load_now_request::<T, S>(world, path, settings, stack)
        .map_err(|error| error.into_dependency_error(self.path))?,
    };
    self.dependencies.push(handle.id().untyped());
    Ok(handle)
  }
}

/// A loader with its types erased, for the server's table
pub(crate) trait ErasedLoader: Send + Sync + 'static {
  fn extensions(&self) -> &[&'static str];
  /// The settings type, and its name for errors
  fn settings_type(&self) -> (TypeId, &'static str);
  /// The key hash of the default settings, worked out once
  fn default_settings_hash(&self) -> u64;
  fn default_settings(&self) -> Arc<dyn Any + Send + Sync>;
  fn load(
    &self,
    bytes: Vec<u8>,
    settings: &(dyn Any + Send + Sync),
    cx: &mut LoadContext<'_>,
  ) -> Result<Box<dyn Any + Send>, AssetError>;
  /// The loader's `check` of a value it made
  fn check(&self, value: &dyn Any, world: &World) -> Result<(), AssetError>;
}

struct Erased<L: AssetLoader> {
  loader: L,
  default_hash: u64,
}

/// `loader` with its types erased
pub(crate) fn erase<L: AssetLoader>(loader: L) -> Arc<dyn ErasedLoader> {
  Arc::new(Erased {
    loader,
    default_hash: settings_hash(&L::Settings::default()),
  })
}

/// A settings value's part of an asset's key
pub(crate) fn settings_hash<S: Hash + 'static>(settings: &S) -> u64 {
  fast_hash(&(TypeId::of::<S>(), settings))
}

impl<L: AssetLoader> ErasedLoader for Erased<L> {
  fn extensions(&self) -> &[&'static str] {
    self.loader.extensions()
  }

  fn settings_type(&self) -> (TypeId, &'static str) {
    (TypeId::of::<L::Settings>(), short_name::<L::Settings>())
  }

  fn default_settings_hash(&self) -> u64 {
    self.default_hash
  }

  fn default_settings(&self) -> Arc<dyn Any + Send + Sync> {
    Arc::new(L::Settings::default())
  }

  fn load(
    &self,
    bytes: Vec<u8>,
    settings: &(dyn Any + Send + Sync),
    cx: &mut LoadContext<'_>,
  ) -> Result<Box<dyn Any + Send>, AssetError> {
    // The server checks the settings' type before it queues a load
    let Some(settings) = settings.downcast_ref::<L::Settings>() else {
      return Err(AssetError::decode(
        cx.path(),
        "the loader was given settings of another type",
      ));
    };
    let asset = self.loader.load(bytes, settings, cx)?;
    Ok(Box::new(asset))
  }

  fn check(&self, value: &dyn Any, world: &World) -> Result<(), AssetError> {
    // A value of another type is not one this loader made: nothing to check
    match value.downcast_ref::<L::Asset>() {
      Some(asset) => self.loader.check(asset, world),
      None => Ok(()),
    }
  }
}
