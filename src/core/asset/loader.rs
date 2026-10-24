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

  /// The extensions this loader takes ("png", "ui.ron"), matched without
  /// regard to case. One written with its dot (".png") is taken without it
  fn extensions(&self) -> &[&'static str];

  /// Makes the asset; dependencies are loaded through `cx`. Return an
  /// error for bytes it cannot read rather than panic: in native builds a
  /// panic fails the asset, but the web build aborts on one
  fn load(
    &self,
    bytes: Vec<u8>,
    settings: &Self::Settings,
    cx: &mut LoadContext<'_>,
  ) -> Result<Self::Asset, AssetError>;

  /// Checks the asset against its dependencies, once they are loaded and
  /// whenever one reloads (on the main thread): `cx.world()` holds their
  /// stores. An error fails the load, or a reload, which keeps the last good
  /// value; name the asset in it with `AssetError::decode(cx.path(), error)`.
  /// The default accepts
  fn check(&self, asset: &Self::Asset, cx: &CheckContext<'_>) -> Result<(), AssetError> {
    let _ = (asset, cx);
    Ok(())
  }

  /// Why the asset did not load, a dependency it asked for having failed
  /// (on the workers, or in a reload): `error` is the server's
  /// `AssetError::Dependency`, which names both paths. A loader that knows
  /// where its file names the dependency can give a better error; the
  /// default keeps the server's
  fn dependency_failed(&self, asset: &Self::Asset, error: &AssetError) -> Option<AssetError> {
    let _ = (asset, error);
    None
  }
}

/// What a loader can do while loading
pub struct LoadContext<'a> {
  path: &'a AssetPath,
  server: &'a AssetServer,
  dependencies: Vec<UntypedAssetId>,
  mode: LoadMode<'a>,
  /// The load replaces a loaded value
  reload: bool,
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
      reload: false,
    }
  }

  /// The context of a reload, replacing a loaded value, when `reload`
  pub(crate) fn reloading(mut self, reload: bool) -> Self {
    self.reload = reload;
    self
  }

  /// The dependencies loaded through this context
  pub(crate) fn into_dependencies(self) -> Vec<UntypedAssetId> {
    self.dependencies
  }

  /// The path of the asset being loaded
  pub fn path(&self) -> &AssetPath {
    self.path
  }

  /// Whether the load was queued: on a worker thread (in the pump on the
  /// web), not on the thread that called `load_now`
  pub(crate) fn queued(&self) -> bool {
    matches!(self.mode, LoadMode::Queued)
  }

  /// Whether the load is a reload, replacing a loaded value (a file that
  /// changed, or `AssetServer::reload`)
  pub(crate) fn is_reload(&self) -> bool {
    self.reload
  }

  /// The world `load_now` loads into, which holds the dependencies loaded
  /// so far; none on the workers
  pub(crate) fn world(&self) -> Option<&World> {
    match &self.mode {
      LoadMode::Now { world, .. } => Some(world),
      LoadMode::Queued => None,
    }
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

/// What a loader's `check` can see: the asset's path, and the world with
/// every store, its dependencies' among them
pub struct CheckContext<'a> {
  path: &'a AssetPath,
  world: &'a World,
}

impl<'a> CheckContext<'a> {
  pub(crate) fn new(path: &'a AssetPath, world: &'a World) -> Self {
    Self { path, world }
  }

  /// The path of the asset being checked, to name it in an error
  pub fn path(&self) -> &AssetPath {
    self.path
  }

  /// The world: the stores the asset's dependencies are in
  /// (`cx.world().resource::<Assets<Image>>()`)
  pub fn world(&self) -> &World {
    self.world
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
  /// The loader's `check` of a value it made, the asset at `path`
  fn check(&self, value: &dyn Any, world: &World, path: &AssetPath) -> Result<(), AssetError>;
  /// The loader's `dependency_failed` for a value it made
  fn dependency_failed(&self, value: &dyn Any, error: &AssetError) -> Option<AssetError>;
}

struct Erased<L: AssetLoader> {
  loader: L,
  default_hash: u64,
  /// The loader's extensions without a leading dot, worked out once
  extensions: Vec<&'static str>,
}

/// `loader` with its types erased
pub(crate) fn erase<L: AssetLoader>(loader: L) -> Arc<dyn ErasedLoader> {
  // ".level" is "level": a path's extensions are matched without their dot
  let extensions = loader
    .extensions()
    .iter()
    .map(|extension| extension.trim_start_matches('.'))
    .collect();
  Arc::new(Erased {
    loader,
    default_hash: settings_hash(&L::Settings::default()),
    extensions,
  })
}

/// A settings value's part of an asset's key
pub(crate) fn settings_hash<S: Hash + 'static>(settings: &S) -> u64 {
  fast_hash(&(TypeId::of::<S>(), settings))
}

impl<L: AssetLoader> ErasedLoader for Erased<L> {
  fn extensions(&self) -> &[&'static str] {
    &self.extensions
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

  fn check(&self, value: &dyn Any, world: &World, path: &AssetPath) -> Result<(), AssetError> {
    // The server checks a value only with the loader that made it (or one
    // that makes its type); anything else fails the asset rather than pass
    let asset = value.downcast_ref::<L::Asset>();
    debug_assert!(asset.is_some(), "a loader checks only values it made");
    let Some(asset) = asset else {
      return Err(AssetError::decode(
        path,
        "a broken invariant: its loader was given a value of another type to check",
      ));
    };
    self.loader.check(asset, &CheckContext::new(path, world))
  }

  fn dependency_failed(&self, value: &dyn Any, error: &AssetError) -> Option<AssetError> {
    // A value of another type keeps the server's error
    let asset = value.downcast_ref::<L::Asset>()?;
    self.loader.dependency_failed(asset, error)
  }
}
