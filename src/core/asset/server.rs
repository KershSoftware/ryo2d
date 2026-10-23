use std::any::{Any, TypeId};
use std::borrow::{Borrow, Cow};
use std::hash::Hash;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, RwLock};

use crossbeam_channel::{Receiver, Sender, unbounded};

use super::assets::Ledger;
use super::event::{AssetStats, TypeStats};
use super::loader::{ErasedLoader, LoadMode, erase, settings_hash};
use super::path::{extensions, normalise};
use super::pump::{Finished, FreeUnheld, Loaded, Typed, free_unheld, insert_value, send_failed};
use super::workers::{INLINE_BUDGET, Workers};
use super::{
  Asset, AssetError, AssetEvent, AssetId, AssetLoader, AssetPath, AssetSettings, AssetSource,
  Assets, Handle, LoadContext, LoadState, Progress, UntypedAssetId,
};
use crate::core::ecs::Resource;
use crate::core::ecs::kernel::World;
use crate::core::hash::FastMap;
use crate::core::messages::register_message;

/// Loads assets by path, through loaders by extension, on worker threads.
/// Cheap to clone (shared inside); a world resource
#[derive(Resource, Clone)]
pub struct AssetServer {
  inner: Arc<ServerInner>,
}

pub(crate) struct ServerInner {
  source: Box<dyn AssetSource>,
  /// Loaders by the asset type they make, the latest registered first
  loaders: RwLock<FastMap<TypeId, Vec<Arc<dyn ErasedLoader>>>>,
  /// The registered asset types, in the order they were registered
  types: RwLock<Vec<AssetType>>,
  table: Mutex<Table>,
  /// Finished loads, for the pump
  finished: Sender<Finished>,
  finished_queue: Receiver<Finished>,
  /// Paths the source says changed. Reloads will read it; until then the pump
  /// empties it, so it cannot grow
  changed: Receiver<AssetPath>,
  workers: Workers,
}

struct AssetType {
  id: TypeId,
  name: &'static str,
  /// The ledger of the type's `Assets<T>`, so ids can be reserved without the world
  ledger: Arc<Mutex<Ledger>>,
  free_unheld: FreeUnheld,
}

/// What the server knows of the assets it loads. Locked before any ledger, never after
#[derive(Default)]
struct Table {
  /// Assets by key: by type and settings hash, then by path (so a lookup
  /// borrows the path and allocates nothing)
  keys: FastMap<(TypeId, u64), FastMap<AssetPath, UntypedAssetId>>,
  /// Loads refused before anything was read (a bad path, no loader, settings
  /// of another type), by type and settings key, then by the text asked for:
  /// asking again gives the same failed asset, so it warns once
  refused: FastMap<(TypeId, u64), FastMap<Box<str>, UntypedAssetId>>,
  /// Every asset asked for through the server, until it is freed
  infos: FastMap<UntypedAssetId, Info>,
  /// The last job number handed out
  last_job: u64,
}

struct Info {
  /// Where its key is, so freeing it can remove the key
  key: Key,
  /// The settings it was loaded with
  // Read by reloads (Task 5)
  #[allow(dead_code)]
  settings: Option<Arc<dyn Any + Send + Sync>>,
  state: LoadState,
  /// The job whose result counts; any other's is stale and dropped
  job: u64,
  /// What it loaded through its `LoadContext`
  // Read by readiness (Task 5)
  #[allow(dead_code)]
  dependencies: Vec<UntypedAssetId>,
}

enum Key {
  /// Loaded by path: its path and settings hash, in `Table::keys`
  Path(AssetPath, u64),
  /// Refused: the text asked for and its settings key, in `Table::refused`
  Refused(Box<str>, u64),
}

/// The settings key of a refused `load` without settings: there was no
/// loader to give the default settings
const DEFAULT_SETTINGS: u64 = 0;

/// A load's path, loader and settings, worked out before anything is made
struct Request<'p> {
  path: Cow<'p, str>,
  loader: Arc<dyn ErasedLoader>,
  settings_hash: u64,
}

/// A load refused before anything was read: the text it is known by (the
/// path as given when it is not an asset path, else the normalised path)
/// and why
struct Refused<'p> {
  text: Cow<'p, str>,
  reason: Refusal,
}

enum Refusal {
  InvalidPath(&'static str),
  NoLoader,
  Settings {
    takes: &'static str,
    given: &'static str,
  },
}

impl Refused<'_> {
  /// The key it is kept under when `load` refuses it: a bad path by its
  /// text alone, anything else by its path and settings
  fn settings_key(&self, given: Option<u64>) -> u64 {
    match self.reason {
      Refusal::InvalidPath(_) => DEFAULT_SETTINGS,
      _ => given.unwrap_or(DEFAULT_SETTINGS),
    }
  }

  fn into_error(self) -> AssetError {
    match self.reason {
      Refusal::InvalidPath(reason) => AssetError::InvalidPath {
        path: self.text.into_owned(),
        reason,
      },
      Refusal::NoLoader => {
        let extension = extensions(&self.text).next().unwrap_or_default().to_owned();
        AssetError::NoLoader {
          path: AssetPath::from_normal(self.text),
          extension,
        }
      }
      Refusal::Settings { takes, given } => AssetError::decode(
        &AssetPath::from_normal(self.text),
        format!("its loader takes `{takes}` settings, not `{given}`"),
      ),
    }
  }
}

/// Why `load_now` (or a dependency it loads) gave no asset
pub(crate) enum NowError {
  /// The request is wrong (a bad path, no loader, a cycle); no asset was made for it
  Request(AssetError),
  /// The asset at `path` failed to load
  Failed {
    path: AssetPath,
    error: Arc<AssetError>,
  },
}

impl NowError {
  fn into_error(self) -> Arc<AssetError> {
    match self {
      NowError::Request(error) => Arc::new(error),
      NowError::Failed { error, .. } => error,
    }
  }

  /// The error a loader sees when loading a dependency failed: a cycle as it
  /// is, so the outermost load names it, and anything else as a `Dependency`
  pub(crate) fn into_dependency_error(self, parent: &AssetPath) -> AssetError {
    match self {
      NowError::Request(error) => error,
      NowError::Failed { path, error } => match &*error {
        AssetError::Cycle { path, chain } => AssetError::Cycle {
          path: path.clone(),
          chain: chain.clone(),
        },
        _ => AssetError::Dependency {
          path: parent.clone(),
          dependency: path,
          error,
        },
      },
    }
  }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
  mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// A type's name without its module path: `Text` for `my_game::assets::Text`
pub(crate) fn short_name<T: ?Sized>() -> &'static str {
  let name = std::any::type_name::<T>();
  let base = name.split('<').next().unwrap_or(name);
  base.rsplit("::").next().unwrap_or(base)
}

impl AssetServer {
  /// A server reading from `source`, with its worker threads started
  pub fn new(source: impl AssetSource) -> Self {
    let (finished, finished_queue) = unbounded();
    let (changes, changed) = unbounded();
    if let Err(error) = source.watch(changes) {
      crate::engine_warn!(module: "asset", "the asset source cannot watch: {}", error.report());
    }
    Self {
      inner: Arc::new(ServerInner {
        source: Box::new(source),
        loaders: RwLock::default(),
        types: RwLock::default(),
        table: Mutex::default(),
        finished,
        finished_queue,
        changed,
        workers: Workers::new(),
      }),
    }
  }

  /// Makes `T` storable: inserts `Assets<T>` and registers `AssetEvent<T>`
  pub fn register_asset<T: Asset>(&self, world: &mut World) {
    if !world.contains_resource::<Assets<T>>() {
      world.init_resource::<Assets<T>>();
    }
    register_message::<AssetEvent<T>>(world);
    let Some(ledger) = world.get_resource::<Assets<T>>().map(Assets::ledger) else {
      return;
    };
    let mut types = self
      .inner
      .types
      .write()
      .unwrap_or_else(PoisonError::into_inner);
    match types.iter_mut().find(|known| known.id == TypeId::of::<T>()) {
      // Ids come from the world's store, if it was replaced
      Some(known) => known.ledger = ledger,
      None => types.push(AssetType {
        id: TypeId::of::<T>(),
        name: short_name::<T>(),
        ledger,
        free_unheld: free_unheld::<T>,
      }),
    }
  }

  /// Adds a loader. For an extension two loaders of one type share, the
  /// latest wins, so a game can replace an engine loader
  pub fn register_loader<L: AssetLoader>(&self, loader: L) {
    self
      .inner
      .loaders
      .write()
      .unwrap_or_else(PoisonError::into_inner)
      .entry(TypeId::of::<L::Asset>())
      .or_default()
      .insert(0, erase(loader));
  }

  /// The asset at `path`, loading it if needed; returns at once. A bad path
  /// or a missing loader gives a handle whose state is `Failed`.
  ///
  /// One path (with one settings value) is one asset: while any handle to it
  /// lives, or until the pump frees it, asking again gives the same handle,
  /// loaded, loading or failed. A failed asset is not tried again until it
  /// is freed; asking for it then loads it anew
  pub fn load<T: Asset>(&self, path: &str) -> Handle<T> {
    self.load_request::<T, ()>(path, None)
  }

  /// The asset at `path` loaded with `settings`, as [`AssetServer::load`] does.
  /// Settings of a type the loader does not take fail the load
  pub fn load_with<T: Asset, S: AssetSettings>(&self, path: &str, settings: S) -> Handle<T> {
    self.load_request(path, Some(settings))
  }

  /// Loads at once on this thread (dependencies too) and inserts the result.
  ///
  /// An asset already loaded is given back as it is, without reading it
  /// again; one already failed gives back its error, until it is freed. One
  /// loading on a worker loads here instead, and the worker's result is
  /// dropped. On failure the error is returned and the state stays `Failed`
  pub fn load_now<T: Asset>(
    &self,
    world: &mut World,
    path: &str,
  ) -> Result<Handle<T>, Arc<AssetError>> {
    self
      .load_now_request::<T, ()>(world, path, None, &mut Vec::new())
      .map_err(NowError::into_error)
  }

  /// Loads at once with `settings`, as [`AssetServer::load_now`] does
  pub fn load_now_with<T: Asset, S: AssetSettings>(
    &self,
    world: &mut World,
    path: &str,
    settings: S,
  ) -> Result<Handle<T>, Arc<AssetError>> {
    self
      .load_now_request(world, path, Some(settings), &mut Vec::new())
      .map_err(NowError::into_error)
  }

  /// Where the asset is in loading. An asset added in code is `Loaded`
  /// while it lives
  pub fn state<T: Asset>(&self, id: impl Into<AssetId<T>>) -> LoadState {
    self.state_of(id.into().untyped())
  }

  /// The path the asset was loaded from
  pub fn path<T: Asset>(&self, id: impl Into<AssetId<T>>) -> Option<AssetPath> {
    let table = self.inner.table();
    match &table.infos.get(&id.into().untyped())?.key {
      Key::Path(path, _) => Some(path.clone()),
      Key::Refused(..) => None,
    }
  }

  /// How far a group of assets has loaded. An id neither loaded nor loading
  /// (freed, or never asked for) is not counted
  pub fn progress(&self, ids: &[UntypedAssetId]) -> Progress {
    let mut progress = Progress::default();
    for &id in ids {
      match self.state_of(id) {
        LoadState::Loaded => progress.loaded += 1,
        LoadState::Failed(_) => progress.failed += 1,
        LoadState::Loading => progress.pending += 1,
        LoadState::NotLoaded => {}
      }
    }
    progress
  }

  /// Loaded, loading and failed counts per registered type, over the assets
  /// loaded by path
  pub fn stats(&self) -> AssetStats {
    let mut types: Vec<(TypeId, TypeStats)> = self
      .inner
      .types
      .read()
      .unwrap_or_else(PoisonError::into_inner)
      .iter()
      .map(|known| {
        let stats = TypeStats {
          name: known.name,
          ..TypeStats::default()
        };
        (known.id, stats)
      })
      .collect();
    let table = self.inner.table();
    for (id, info) in &table.infos {
      let Some((_, stats)) = types.iter_mut().find(|(type_id, _)| *type_id == id.type_id) else {
        continue;
      };
      match info.state {
        LoadState::Loaded => stats.loaded += 1,
        LoadState::Loading => stats.loading += 1,
        LoadState::Failed(_) => stats.failed += 1,
        LoadState::NotLoaded => {}
      }
    }
    AssetStats {
      types: types.into_iter().map(|(_, stats)| stats).collect(),
    }
  }

  fn state_of(&self, id: UntypedAssetId) -> LoadState {
    let known = self
      .inner
      .table()
      .infos
      .get(&id)
      .map(|info| info.state.clone());
    if let Some(state) = known {
      return state;
    }
    // Not asked for by path: an asset added in code is loaded while it lives
    let ledger = self.ledger_of(id.type_id);
    match ledger {
      Some(ledger) if lock(&ledger).is_live_slot(id.index, id.generation) => LoadState::Loaded,
      _ => LoadState::NotLoaded,
    }
  }

  fn ledger_of(&self, type_id: TypeId) -> Option<Arc<Mutex<Ledger>>> {
    let types = self
      .inner
      .types
      .read()
      .unwrap_or_else(PoisonError::into_inner);
    let known = types.iter().find(|known| known.id == type_id)?;
    Some(Arc::clone(&known.ledger))
  }

  /// `T`'s ledger, or an error naming `path` when `T` is not registered
  fn registered<T: Asset>(&self, path: &str) -> Result<Arc<Mutex<Ledger>>, AssetError> {
    self.ledger_of(TypeId::of::<T>()).ok_or_else(|| {
      AssetError::decode(
        &AssetPath::from_normal(Cow::Borrowed(path)),
        format!("`{}` is not a registered asset type", short_name::<T>()),
      )
    })
  }

  /// The loader that makes `T` from `path`: the longest extension with one wins
  fn loader<T: Asset>(&self, path: &str) -> Option<Arc<dyn ErasedLoader>> {
    let loaders = self
      .inner
      .loaders
      .read()
      .unwrap_or_else(PoisonError::into_inner);
    let candidates = loaders.get(&TypeId::of::<T>())?;
    extensions(path).find_map(|extension| {
      candidates
        .iter()
        .find(|loader| {
          loader
            .extensions()
            .iter()
            .any(|known| known.eq_ignore_ascii_case(extension))
        })
        .cloned()
    })
  }

  fn resolve<'p, T: Asset, S: AssetSettings>(
    &self,
    raw: &'p str,
    settings: Option<&S>,
  ) -> Result<Request<'p>, Refused<'p>> {
    let path = normalise(raw).map_err(|reason| Refused {
      text: Cow::Borrowed(raw),
      reason: Refusal::InvalidPath(reason),
    })?;
    let Some(loader) = self.loader::<T>(&path) else {
      return Err(Refused {
        text: path,
        reason: Refusal::NoLoader,
      });
    };
    let settings_hash = match settings {
      None => loader.default_settings_hash(),
      Some(settings) => {
        let (takes, name) = loader.settings_type();
        if takes != TypeId::of::<S>() {
          return Err(Refused {
            text: path,
            reason: Refusal::Settings {
              takes: name,
              given: short_name::<S>(),
            },
          });
        }
        settings_hash(settings)
      }
    };
    Ok(Request {
      path,
      loader,
      settings_hash,
    })
  }

  fn load_request<T: Asset, S: AssetSettings>(&self, path: &str, settings: Option<S>) -> Handle<T> {
    match self.resolve::<T, S>(path, settings.as_ref()) {
      Ok(request) => match self.queue_resolved(request, settings) {
        Ok(handle) => handle,
        Err(error) => unregistered(error),
      },
      Err(refused) => {
        let key = refused.settings_key(settings.as_ref().map(settings_hash));
        self.refused(refused, key)
      }
    }
  }

  /// The asset at `path`, its load queued unless it is loaded or loading; an
  /// error if the path is bad, no loader makes a `T` from it or `T` is not registered
  pub(crate) fn queue_load<T: Asset, S: AssetSettings>(
    &self,
    path: &str,
    settings: Option<S>,
  ) -> Result<Handle<T>, AssetError> {
    let request = self
      .resolve::<T, S>(path, settings.as_ref())
      .map_err(Refused::into_error)?;
    self.queue_resolved(request, settings)
  }

  /// [`AssetServer::queue_load`] once the request is resolved
  fn queue_resolved<T: Asset, S: AssetSettings>(
    &self,
    request: Request<'_>,
    settings: Option<S>,
  ) -> Result<Handle<T>, AssetError> {
    let ledger = self.registered::<T>(&request.path)?;
    let mut table = self.inner.table();
    if let Some(handle) = table.find(&ledger, &request) {
      return Ok(handle);
    }
    let handle = lock(&ledger).reserve::<T>();
    let id = handle.id().untyped();
    let path = AssetPath::from_normal(request.path);
    let settings = settings_value(settings, &*request.loader);
    let job = table.start(
      id,
      path.clone(),
      request.settings_hash,
      Arc::clone(&settings),
    );
    drop(table);
    let server = Arc::downgrade(&self.inner);
    let loader = request.loader;
    self.inner.workers.queue(Box::new(move || {
      // A server dropped before the job ran has no one to give the result to
      let Some(inner) = server.upgrade() else {
        return;
      };
      let server = AssetServer { inner };
      let outcome = server
        .run_load(&path, &*loader, &*settings, LoadMode::Queued)
        .map_err(Arc::new);
      let _ = server.inner.finished.send(Finished {
        id,
        job,
        outcome,
        typed: Typed::of::<T>(),
      });
    }));
    Ok(handle)
  }

  /// The failed asset for a refused load: the one already kept under its
  /// key, or a new one whose `Failed` event goes at the next pump
  fn refused<T: Asset>(&self, refused: Refused<'_>, settings_key: u64) -> Handle<T> {
    let Some(ledger) = self.ledger_of(TypeId::of::<T>()) else {
      return unregistered(refused.into_error());
    };
    let key = (TypeId::of::<T>(), settings_key);
    let mut table = self.inner.table();
    if let Some(handle) = table.find_refused(&ledger, key, &refused.text) {
      return handle;
    }
    let text: Box<str> = Box::from(&*refused.text);
    let error = Arc::new(refused.into_error());
    let handle = lock(&ledger).reserve::<T>();
    let id = handle.id().untyped();
    let job = table.fail(id, Key::Refused(text, settings_key), Arc::clone(&error));
    drop(table);
    let _ = self.inner.finished.send(Finished {
      id,
      job,
      outcome: Err(error),
      typed: Typed::of::<T>(),
    });
    handle
  }

  /// `load_now` for `T` at `path`, inside any `load_now` already running on
  /// this thread: `stack` holds their paths, outermost first
  pub(crate) fn load_now_request<T: Asset, S: AssetSettings>(
    &self,
    world: &mut World,
    path: &str,
    settings: Option<S>,
    stack: &mut Vec<AssetPath>,
  ) -> Result<Handle<T>, NowError> {
    let request = self
      .resolve::<T, S>(path, settings.as_ref())
      .map_err(|refused| NowError::Request(refused.into_error()))?;
    if let Some(at) = stack.iter().position(|p| p.as_str() == request.path) {
      return Err(NowError::Request(cycle(
        stack.get(at..).unwrap_or_default(),
        &request.path,
      )));
    }
    let ledger = self
      .registered::<T>(&request.path)
      .map_err(NowError::Request)?;
    let mut table = self.inner.table();
    let handle = match table.find::<T>(&ledger, &request) {
      Some(handle) => match table
        .infos
        .get(&handle.id().untyped())
        .map(|info| &info.state)
      {
        Some(LoadState::Loaded) => return Ok(handle),
        Some(LoadState::Failed(error)) => {
          return Err(NowError::Failed {
            path: AssetPath::from_normal(request.path),
            error: Arc::clone(error),
          });
        }
        // Loading on a worker: it loads here instead, and that job's result is dropped
        _ => handle,
      },
      None => lock(&ledger).reserve::<T>(),
    };
    let id = handle.id().untyped();
    let path = AssetPath::from_normal(request.path);
    let settings = settings_value(settings, &*request.loader);
    let job = table.start(
      id,
      path.clone(),
      request.settings_hash,
      Arc::clone(&settings),
    );
    drop(table);

    stack.push(path.clone());
    let mode = LoadMode::Now {
      world: &mut *world,
      stack: &mut *stack,
    };
    let outcome = self.run_load(&path, &*request.loader, &*settings, mode);
    stack.pop();
    match outcome {
      Ok(Loaded {
        value,
        dependencies,
      }) => {
        if insert_value::<T>(world, id, value) {
          self.set_loaded(id, job, dependencies);
        }
        Ok(handle)
      }
      Err(error) => {
        let error = Arc::new(error);
        self.set_failed(id, job, Arc::clone(&error));
        crate::engine_warn!(module: "asset", "{}", error.report());
        send_failed::<T>(world, id, Arc::clone(&error));
        Err(NowError::Failed { path, error })
      }
    }
  }

  /// Reads and loads one asset. A panic in the source or the loader is
  /// caught, and fails the asset
  fn run_load(
    &self,
    path: &AssetPath,
    loader: &dyn ErasedLoader,
    settings: &(dyn Any + Send + Sync),
    mode: LoadMode<'_>,
  ) -> Result<Loaded, AssetError> {
    let bytes = catch_unwind(AssertUnwindSafe(|| self.inner.source.read(path)))
      .map_err(|_| AssetError::decode(path, "the source panicked"))??;
    let mut cx = LoadContext::new(path, self, mode);
    let value = catch_unwind(AssertUnwindSafe(|| loader.load(bytes, settings, &mut cx)))
      .map_err(|_| AssetError::decode(path, "the loader panicked"))??;
    Ok(Loaded {
      value,
      dependencies: cx.into_dependencies(),
    })
  }

  // The pump's side

  pub(crate) fn run_inline_jobs(&self) {
    self.inner.workers.run_inline(INLINE_BUDGET);
  }

  pub(crate) fn drain_changed(&self) {
    while self.inner.changed.try_recv().is_ok() {}
  }

  pub(crate) fn next_finished(&self) -> Option<Finished> {
    self.inner.finished_queue.try_recv().ok()
  }

  /// Whether `job` is still the one whose result counts for `id`
  pub(crate) fn is_current(&self, id: UntypedAssetId, job: u64) -> bool {
    self
      .inner
      .table()
      .infos
      .get(&id)
      .is_some_and(|info| info.job == job)
  }

  pub(crate) fn set_loaded(&self, id: UntypedAssetId, job: u64, dependencies: Vec<UntypedAssetId>) {
    let mut table = self.inner.table();
    if let Some(info) = table.infos.get_mut(&id).filter(|info| info.job == job) {
      info.state = LoadState::Loaded;
      info.dependencies = dependencies;
    }
  }

  pub(crate) fn set_failed(&self, id: UntypedAssetId, job: u64, error: Arc<AssetError>) {
    let mut table = self.inner.table();
    if let Some(info) = table.infos.get_mut(&id).filter(|info| info.job == job) {
      info.state = LoadState::Failed(error);
    }
  }

  /// The free step of the `index`th registered type
  pub(crate) fn free_step(&self, index: usize) -> Option<FreeUnheld> {
    let types = self
      .inner
      .types
      .read()
      .unwrap_or_else(PoisonError::into_inner);
    types.get(index).map(|known| known.free_unheld)
  }

  /// Drops what the server knows of freed assets: their keys and states
  pub(crate) fn forget(&self, freed: &[UntypedAssetId]) {
    let mut table = self.inner.table();
    for &id in freed {
      table.forget(id);
    }
  }
}

impl ServerInner {
  fn table(&self) -> MutexGuard<'_, Table> {
    lock(&self.table)
  }
}

impl Table {
  /// A new strong handle to the live asset with the request's key
  fn find<T: Asset>(&self, ledger: &Mutex<Ledger>, request: &Request<'_>) -> Option<Handle<T>> {
    let paths = self.keys.get(&(TypeId::of::<T>(), request.settings_hash))?;
    let id = paths.get(&*request.path)?.typed::<T>()?;
    // None when the slot was freed and the pump has not forgotten it yet:
    // the key is loaded anew, under a new id
    lock(ledger).strong(id)
  }

  /// A new strong handle to the live failed asset kept for a refused load
  fn find_refused<T: Asset>(
    &self,
    ledger: &Mutex<Ledger>,
    key: (TypeId, u64),
    text: &str,
  ) -> Option<Handle<T>> {
    let id = self.refused.get(&key)?.get(text)?.typed::<T>()?;
    lock(ledger).strong(id)
  }

  fn next_job(&mut self) -> u64 {
    self.last_job += 1;
    self.last_job
  }

  /// Records a load of `id` under its key, and gives its job number
  fn start(
    &mut self,
    id: UntypedAssetId,
    path: AssetPath,
    settings_hash: u64,
    settings: Arc<dyn Any + Send + Sync>,
  ) -> u64 {
    let job = self.next_job();
    self
      .keys
      .entry((id.type_id, settings_hash))
      .or_default()
      .insert(path.clone(), id);
    self.infos.insert(
      id,
      Info {
        key: Key::Path(path, settings_hash),
        settings: Some(settings),
        state: LoadState::Loading,
        job,
        dependencies: Vec::new(),
      },
    );
    job
  }

  /// Records `id` as refused under `key`, failed before it could load, and
  /// gives its job number
  fn fail(&mut self, id: UntypedAssetId, key: Key, error: Arc<AssetError>) -> u64 {
    let job = self.next_job();
    if let Key::Refused(text, settings_key) = &key {
      self
        .refused
        .entry((id.type_id, *settings_key))
        .or_default()
        .insert(text.clone(), id);
    }
    self.infos.insert(
      id,
      Info {
        key,
        settings: None,
        state: LoadState::Failed(error),
        job,
        dependencies: Vec::new(),
      },
    );
    job
  }

  fn forget(&mut self, id: UntypedAssetId) {
    let Some(info) = self.infos.remove(&id) else {
      return;
    };
    match info.key {
      Key::Path(path, settings_hash) => {
        remove_key(
          &mut self.keys,
          (id.type_id, settings_hash),
          path.as_str(),
          id,
        );
      }
      Key::Refused(text, settings_key) => {
        remove_key(&mut self.refused, (id.type_id, settings_key), &text, id);
      }
    }
  }
}

/// Removes `text` from `map` under `key` if it still names `id`: it may name
/// a newer asset already, asked for while this one was being freed
fn remove_key<K: Borrow<str> + Hash + Eq>(
  map: &mut FastMap<(TypeId, u64), FastMap<K, UntypedAssetId>>,
  key: (TypeId, u64),
  text: &str,
  id: UntypedAssetId,
) {
  let Some(texts) = map.get_mut(&key) else {
    return;
  };
  if texts.get(text) == Some(&id) {
    texts.remove(text);
  }
  if texts.is_empty() {
    map.remove(&key);
  }
}

/// `T` was never registered, so there is nowhere to keep the asset or its
/// state: say why now, and hand out a handle that finds nothing
fn unregistered<T: Asset>(error: AssetError) -> Handle<T> {
  crate::engine_warn!(module: "asset", "{}", error.report());
  Handle::detached()
}

fn settings_value<S: AssetSettings>(
  settings: Option<S>,
  loader: &dyn ErasedLoader,
) -> Arc<dyn Any + Send + Sync> {
  match settings {
    Some(settings) => Arc::new(settings),
    None => loader.default_settings(),
  }
}

/// A cycle: `stack` runs from the path that is asked for again to the load asking
fn cycle(stack: &[AssetPath], path: &str) -> AssetError {
  let mut chain = String::new();
  for step in stack {
    chain.push_str(step.as_str());
    chain.push_str(" → ");
  }
  chain.push_str(path);
  let path = stack
    .first()
    .cloned()
    .unwrap_or_else(|| AssetPath::from_normal(Cow::Borrowed(path)));
  AssetError::Cycle { path, chain }
}

#[cfg(test)]
mod tests {
  use std::time::Duration;

  use super::*;
  use crate::core::asset::event::TypeStats;
  use crate::core::asset::{AssetEvent, Assets, LoadContext, MemorySource, pump};
  use crate::core::ecs::kernel::{
    MessageCursor, MessageReader, Messages, ResMut, Schedule, ScheduleLabel,
  };

  #[derive(Debug)]
  struct Text(String);
  impl Asset for Text {}

  struct TextLoader;
  impl AssetLoader for TextLoader {
    type Asset = Text;
    type Settings = ();
    fn extensions(&self) -> &[&'static str] {
      &["txt"]
    }
    fn load(&self, bytes: Vec<u8>, _: &(), cx: &mut LoadContext<'_>) -> Result<Text, AssetError> {
      String::from_utf8(bytes)
        .map(Text)
        .map_err(|e| AssetError::decode(cx.path(), e))
    }
  }

  struct BoomLoader;
  impl AssetLoader for BoomLoader {
    type Asset = Text;
    type Settings = ();
    fn extensions(&self) -> &[&'static str] {
      &["boom"]
    }
    fn load(&self, _: Vec<u8>, _: &(), _: &mut LoadContext<'_>) -> Result<Text, AssetError> {
      panic!("boom")
    }
  }

  #[derive(Default, Clone, Hash, PartialEq, Eq)]
  struct Upper(bool);

  struct UpperLoader;
  impl AssetLoader for UpperLoader {
    type Asset = Text;
    type Settings = Upper;
    fn extensions(&self) -> &[&'static str] {
      &["up"]
    }
    fn load(
      &self,
      bytes: Vec<u8>,
      settings: &Upper,
      cx: &mut LoadContext<'_>,
    ) -> Result<Text, AssetError> {
      let text = String::from_utf8(bytes).map_err(|e| AssetError::decode(cx.path(), e))?;
      Ok(Text(if settings.0 {
        text.to_uppercase()
      } else {
        text
      }))
    }
  }

  /// A file whose text names another file, loaded first as its dependency
  struct Link(Next);
  impl Asset for Link {}

  // Held so the dependency lives as long as the link
  #[allow(dead_code)]
  enum Next {
    Text(Handle<Text>),
    Link(Handle<Link>),
  }

  struct LinkLoader;
  impl AssetLoader for LinkLoader {
    type Asset = Link;
    type Settings = ();
    fn extensions(&self) -> &[&'static str] {
      &["link"]
    }
    fn load(&self, bytes: Vec<u8>, _: &(), cx: &mut LoadContext<'_>) -> Result<Link, AssetError> {
      let text = String::from_utf8(bytes).map_err(|e| AssetError::decode(cx.path(), e))?;
      let next = text.trim();
      Ok(Link(if next.ends_with(".link") {
        Next::Link(cx.load::<Link>(next)?)
      } else {
        Next::Text(cx.load::<Text>(next)?)
      }))
    }
  }

  fn world_with(source: impl AssetSource) -> (World, AssetServer) {
    let mut world = World::new();
    let server = AssetServer::new(source);
    server.register_asset::<Text>(&mut world);
    server.register_asset::<Link>(&mut world);
    server.register_loader(TextLoader);
    server.register_loader(BoomLoader);
    server.register_loader(UpperLoader);
    server.register_loader(LinkLoader);
    world.insert_resource(server.clone());
    (world, server)
  }

  /// How long a test waits for the workers before it fails
  const PATIENCE: Duration = Duration::from_secs(5);

  fn pump_until(world: &mut World, done: impl Fn(&World) -> bool) {
    let start = std::time::Instant::now();
    loop {
      pump(world);
      if done(world) {
        return;
      }
      assert!(start.elapsed() < PATIENCE, "timed out waiting");
      std::thread::sleep(Duration::from_millis(1));
    }
  }

  /// Waits until `count` results have finished on the workers, unapplied
  fn wait_finished(server: &AssetServer, count: usize) {
    let start = std::time::Instant::now();
    while server.inner.finished_queue.len() < count {
      assert!(start.elapsed() < PATIENCE, "timed out waiting");
      std::thread::sleep(Duration::from_millis(1));
    }
  }

  fn text(world: &World, id: impl Into<AssetId<Text>>) -> Option<&str> {
    world
      .resource::<Assets<Text>>()
      .get(id)
      .map(|t| t.0.as_str())
  }

  fn events(world: &World, cursor: &mut MessageCursor<AssetEvent<Text>>) -> Vec<AssetEvent<Text>> {
    cursor
      .read(world.resource::<Messages<AssetEvent<Text>>>())
      .cloned()
      .collect()
  }

  /// A source that holds back "slow.txt" until the test lets it go
  struct HeldSource {
    files: MemorySource,
    release: crossbeam_channel::Receiver<()>,
  }
  impl AssetSource for HeldSource {
    fn read(&self, path: &AssetPath) -> Result<Vec<u8>, AssetError> {
      if path.as_str() == "slow.txt" {
        // Returns when the test sends, or drops its sender
        let _ = self.release.recv();
      }
      self.files.read(path)
    }
  }

  /// A source whose first read of each path takes the bytes, says so, and
  /// holds them back until the test lets it go; later reads are not held
  struct FirstReadHeld {
    files: MemorySource,
    seen: Mutex<std::collections::HashSet<AssetPath>>,
    reading: crossbeam_channel::Sender<()>,
    release: crossbeam_channel::Receiver<()>,
  }
  impl AssetSource for FirstReadHeld {
    fn read(&self, path: &AssetPath) -> Result<Vec<u8>, AssetError> {
      let bytes = self.files.read(path);
      if lock(&self.seen).insert(path.clone()) {
        let _ = self.reading.send(());
        let _ = self.release.recv();
      }
      bytes
    }
  }

  #[test]
  fn a_load_returns_at_once_and_the_pump_inserts_the_asset() {
    let source = MemorySource::new();
    source.insert("a.txt", "hello");
    let (mut world, server) = world_with(source);
    let handle: Handle<Text> = server.load("a.txt");
    assert!(matches!(server.state(&handle), LoadState::Loading));
    pump_until(&mut world, |w| {
      w.resource::<Assets<Text>>().get(&handle).is_some()
    });
    assert!(server.state(&handle).is_loaded());
    assert_eq!(
      world
        .resource::<Assets<Text>>()
        .get(&handle)
        .map(|t| t.0.as_str()),
      Some("hello")
    );
    assert_eq!(
      server.path(&handle).as_ref().map(AssetPath::as_str),
      Some("a.txt")
    );
  }

  #[test]
  fn loading_one_path_twice_gives_one_asset() {
    let source = MemorySource::new();
    source.insert("a.txt", "hello");
    let (_world, server) = world_with(source);
    let a: Handle<Text> = server.load("a.txt");
    let b: Handle<Text> = server.load("./a.txt");
    assert_eq!(a, b);
  }

  #[test]
  fn settings_are_part_of_the_key() {
    let source = MemorySource::new();
    source.insert("a.up", "hi");
    let (mut world, server) = world_with(source);
    let plain: Handle<Text> = server.load_with("a.up", Upper(false));
    let upper: Handle<Text> = server.load_with("a.up", Upper(true));
    assert_ne!(plain, upper);
    assert_eq!(
      server.load::<Text>("a.up"),
      plain,
      "default settings are Upper(false)"
    );
    pump_until(&mut world, |w| w.resource::<Assets<Text>>().len() == 2);
    assert_eq!(
      world
        .resource::<Assets<Text>>()
        .get(&upper)
        .map(|t| t.0.as_str()),
      Some("HI")
    );
  }

  #[test]
  fn settings_of_another_type_fail_the_load() {
    let source = MemorySource::new();
    source.insert("a.up", "hi");
    let (_world, server) = world_with(source);
    let handle: Handle<Text> = server.load_with("a.up", 7u8);
    assert!(matches!(
      server.state(&handle).error().map(|e| &**e),
      Some(AssetError::Decode { .. })
    ));
  }

  #[test]
  fn a_missing_file_fails_with_not_found() {
    let (mut world, server) = world_with(MemorySource::new());
    let handle: Handle<Text> = server.load("nope.txt");
    pump_until(&mut world, |_| server.state(&handle).is_failed());
    assert!(matches!(
      server.state(&handle).error().map(|e| &**e),
      Some(AssetError::NotFound { path }) if path.as_str() == "nope.txt"
    ));
  }

  #[test]
  fn a_path_with_no_loader_fails_with_no_loader() {
    let (mut world, server) = world_with(MemorySource::new());
    let mut cursor = world.resource::<Messages<AssetEvent<Text>>>().get_cursor();
    let handle: Handle<Text> = server.load("a.xyz");
    assert!(matches!(
      server.state(&handle).error().map(|e| &**e),
      Some(AssetError::NoLoader { extension, .. }) if extension == "xyz"
    ));
    pump(&mut world);
    let sent = events(&world, &mut cursor);
    assert!(
      matches!(sent.as_slice(), [AssetEvent::Failed { id, .. }] if *id == handle.id()),
      "{sent:?}"
    );
    assert!(matches!(
      server.load_now::<Text>(&mut world, "b.xyz").map_err(|e| e.to_string()),
      Err(message) if message.contains("no loader")
    ));
  }

  #[test]
  fn a_refused_load_asked_again_is_the_same_failed_asset() {
    let (mut world, server) = world_with(MemorySource::new());
    let mut cursor = world.resource::<Messages<AssetEvent<Text>>>().get_cursor();
    let no_loader: Handle<Text> = server.load("a.xyz");
    assert_eq!(server.load::<Text>("./a.xyz"), no_loader);
    let invalid: Handle<Text> = server.load("../a.txt");
    assert_eq!(server.load::<Text>("../a.txt"), invalid);
    let wrong: Handle<Text> = server.load_with("a.up", 7u8);
    assert_eq!(server.load_with::<Text, u8>("a.up", 7), wrong);
    let other: Handle<Text> = server.load_with("a.xyz", Upper(true));
    assert_ne!(other, no_loader, "other settings are another asset");
    // One failure each, so one warning each
    pump(&mut world);
    let sent = events(&world, &mut cursor);
    assert_eq!(sent.len(), 4, "{sent:?}");
    assert!(sent.iter().all(|e| matches!(e, AssetEvent::Failed { .. })));
    // Once freed, it is refused anew
    let id = no_loader.id();
    drop(no_loader);
    pump(&mut world);
    let again: Handle<Text> = server.load("a.xyz");
    assert_ne!(again.id(), id);
    assert!(server.state(&again).is_failed());
  }

  #[test]
  fn a_handle_from_before_its_type_was_registered_finds_nothing() {
    let mut world = World::new();
    let server = AssetServer::new(MemorySource::new());
    server.register_loader(TextLoader);
    world.insert_resource(server.clone());
    let early: Handle<Text> = server.load("a.txt");
    server.register_asset::<Text>(&mut world);
    let real = world
      .resource_mut::<Assets<Text>>()
      .add(Text("real".into()));
    assert_ne!(early, real);
    assert_eq!(text(&world, &early), None);
    assert!(matches!(server.state(&early), LoadState::NotLoaded));
  }

  #[test]
  fn a_path_leaving_the_root_is_invalid() {
    let (mut world, server) = world_with(MemorySource::new());
    for path in ["../a.txt", "/a.txt"] {
      let handle: Handle<Text> = server.load(path);
      assert!(
        matches!(
          server.state(&handle).error().map(|e| &**e),
          Some(AssetError::InvalidPath { .. })
        ),
        "{path}"
      );
      assert!(
        matches!(
          server.load_now::<Text>(&mut world, path).err().as_deref(),
          Some(AssetError::InvalidPath { .. })
        ),
        "{path}"
      );
    }
  }

  #[test]
  fn a_panicking_loader_fails_its_asset_and_the_workers_live_on() {
    let source = MemorySource::new();
    source.insert("a.boom", "x");
    source.insert("b.txt", "fine");
    source.insert("c.boom", "y");
    let (mut world, server) = world_with(source);
    let boom: Handle<Text> = server.load("a.boom");
    let fine: Handle<Text> = server.load("b.txt");
    pump_until(&mut world, |_| {
      server.state(&boom).is_failed() && server.state(&fine).is_loaded()
    });
    assert!(matches!(
      server.state(&boom).error().map(|e| &**e),
      Some(AssetError::Decode { .. })
    ));
    assert!(server.state(&fine).is_loaded());
    assert!(matches!(
      server
        .load_now::<Text>(&mut world, "c.boom")
        .err()
        .as_deref(),
      Some(AssetError::Decode { .. })
    ));
  }

  #[test]
  fn load_now_inserts_at_once_and_wins_over_a_queued_load() {
    let source = MemorySource::new();
    source.insert("a.txt", "one");
    let (mut world, server) = world_with(source.clone());
    let queued: Handle<Text> = server.load("a.txt");
    // The queued load has read "one" and finished; the pump has not applied it
    wait_finished(&server, 1);
    source.insert("a.txt", "two");
    let now = server
      .load_now::<Text>(&mut world, "a.txt")
      .map_err(|e| e.to_string());
    assert_eq!(now.as_ref().ok(), Some(&queued), "one asset");
    assert_eq!(
      world
        .resource::<Assets<Text>>()
        .get(&queued)
        .map(|t| t.0.as_str()),
      Some("two")
    );
    pump(&mut world);
    assert_eq!(
      world
        .resource::<Assets<Text>>()
        .get(&queued)
        .map(|t| t.0.as_str()),
      Some("two"),
      "the queued load's older result is dropped"
    );
    assert!(server.state(&queued).is_loaded());
  }

  #[test]
  fn load_now_wins_over_a_queued_load_still_reading() {
    let files = MemorySource::new();
    files.insert("a.txt", "one");
    let (reading, started) = crossbeam_channel::unbounded();
    let (release, held) = crossbeam_channel::unbounded();
    let (mut world, server) = world_with(FirstReadHeld {
      files: files.clone(),
      seen: Mutex::default(),
      reading,
      release: held,
    });
    let queued: Handle<Text> = server.load("a.txt");
    // The worker has read "one" and holds it
    assert!(started.recv_timeout(PATIENCE).is_ok(), "timed out waiting");
    files.insert("a.txt", "two");
    let now = server.load_now::<Text>(&mut world, "a.txt").ok();
    assert_eq!(now.as_ref(), Some(&queued), "one asset");
    assert_eq!(text(&world, &queued), Some("two"));
    let _ = release.send(());
    wait_finished(&server, 1);
    pump(&mut world);
    assert_eq!(
      text(&world, &queued),
      Some("two"),
      "the queued load's older result is dropped"
    );
  }

  #[test]
  fn load_now_of_a_loaded_asset_gives_it_back() {
    let source = MemorySource::new();
    source.insert("a.txt", "one");
    let (mut world, server) = world_with(source.clone());
    let first = server.load_now::<Text>(&mut world, "a.txt").ok();
    source.insert("a.txt", "two");
    let again = server.load_now::<Text>(&mut world, "a.txt").ok();
    assert!(first.is_some());
    assert_eq!(first, again);
    assert_eq!(
      again
        .and_then(|h| text(&world, &h).map(str::to_owned))
        .as_deref(),
      Some("one")
    );
  }

  #[test]
  fn load_now_names_an_import_cycle() {
    let source = MemorySource::new();
    source.insert("a.link", "b.link");
    source.insert("b.link", "a.link");
    let (mut world, server) = world_with(source);
    let error = server.load_now::<Link>(&mut world, "a.link").err();
    assert!(
      matches!(
        error.as_deref(),
        Some(AssetError::Cycle { path, chain })
          if path.as_str() == "a.link" && chain == "a.link → b.link → a.link"
      ),
      "{error:?}"
    );
  }

  #[test]
  fn load_now_wraps_a_failed_dependency() {
    let source = MemorySource::new();
    source.insert("a.link", "gone.txt");
    let (mut world, server) = world_with(source);
    let error = server.load_now::<Link>(&mut world, "a.link").err();
    assert!(
      matches!(
        error.as_deref(),
        Some(AssetError::Dependency { path, dependency, error })
          if path.as_str() == "a.link"
            && dependency.as_str() == "gone.txt"
            && matches!(**error, AssetError::NotFound { .. })
      ),
      "{error:?}"
    );
  }

  #[test]
  fn a_queued_load_queues_its_dependencies() {
    let source = MemorySource::new();
    source.insert("a.link", "b.txt");
    source.insert("b.txt", "bee");
    let (mut world, server) = world_with(source);
    let link: Handle<Link> = server.load("a.link");
    pump_until(&mut world, |w| {
      w.resource::<Assets<Link>>().len() == 1 && w.resource::<Assets<Text>>().len() == 1
    });
    let next = match world.resource::<Assets<Link>>().get(&link) {
      Some(Link(Next::Text(next))) => text(&world, next),
      _ => None,
    };
    assert_eq!(next, Some("bee"));
  }

  #[derive(Resource, Default)]
  struct Seen(Vec<AssetEvent<Text>>);

  #[derive(ScheduleLabel, Debug, Clone, Copy, PartialEq, Eq, Hash)]
  struct Read;

  /// The events a `MessageReader` system sees since it last ran
  fn read_events(world: &mut World, reader: &mut Schedule) -> Vec<AssetEvent<Text>> {
    reader.run(world);
    std::mem::take(&mut world.resource_mut::<Seen>().0)
  }

  #[test]
  fn events_tell_added_failed_and_removed() {
    let source = MemorySource::new();
    source.insert("a.txt", "hello");
    let (mut world, server) = world_with(source);
    world.init_resource::<Seen>();
    let mut reader = Schedule::new(Read);
    reader.add_systems(
      |mut events: MessageReader<AssetEvent<Text>>, mut seen: ResMut<Seen>| {
        seen.0.extend(events.read().cloned());
      },
    );
    let loaded: Handle<Text> = server.load("a.txt");
    let missing: Handle<Text> = server.load("nope.txt");
    pump_until(&mut world, |_| {
      server.state(&loaded).is_loaded() && server.state(&missing).is_failed()
    });
    let sent = read_events(&mut world, &mut reader);
    assert_eq!(sent.len(), 2, "{sent:?}");
    assert!(
      sent
        .iter()
        .any(|e| matches!(e, AssetEvent::Added { id } if *id == loaded.id()))
    );
    assert!(
      sent
        .iter()
        .any(|e| matches!(e, AssetEvent::Failed { id, .. } if *id == missing.id()))
    );
    let id = loaded.id();
    drop(loaded);
    pump(&mut world);
    let sent = read_events(&mut world, &mut reader);
    assert!(
      matches!(sent.as_slice(), [AssetEvent::Removed { id: removed }] if *removed == id),
      "{sent:?}"
    );
    assert!(matches!(server.state(id), LoadState::NotLoaded));
    assert_eq!(server.path(id), None);
  }

  #[test]
  fn a_dropped_asset_loaded_again_before_the_pump_keeps_its_id() {
    let source = MemorySource::new();
    source.insert("a.txt", "hello");
    let (mut world, server) = world_with(source);
    let id = server.load_now::<Text>(&mut world, "a.txt").map(|h| h.id());
    let Ok(id) = id else {
      panic!("loads: {id:?}");
    };
    // The handle is gone; before the pump sees that, the next scene asks again
    let again: Handle<Text> = server.load("a.txt");
    assert_eq!(again.id(), id);
    pump(&mut world);
    assert_eq!(text(&world, &again), Some("hello"));
    assert!(server.state(&again).is_loaded());
  }

  #[test]
  fn a_freed_asset_loads_anew() {
    let source = MemorySource::new();
    source.insert("a.txt", "one");
    let (mut world, server) = world_with(source.clone());
    let first = server.load_now::<Text>(&mut world, "a.txt").map(|h| h.id());
    pump(&mut world);
    source.insert("a.txt", "two");
    let second = server.load_now::<Text>(&mut world, "a.txt");
    assert!(first.is_ok());
    assert_ne!(second.as_ref().map(Handle::id).ok(), first.ok());
    assert_eq!(
      second
        .ok()
        .and_then(|h| text(&world, &h).map(str::to_owned))
        .as_deref(),
      Some("two")
    );
  }

  #[test]
  fn a_load_dropped_before_it_finishes_is_discarded() {
    let files = MemorySource::new();
    files.insert("slow.txt", "late");
    let (release, held) = crossbeam_channel::unbounded();
    let (mut world, server) = world_with(HeldSource {
      files,
      release: held,
    });
    let mut cursor = world.resource::<Messages<AssetEvent<Text>>>().get_cursor();
    let slow: Handle<Text> = server.load("slow.txt");
    let id = slow.id();
    drop(slow);
    pump(&mut world);
    let _ = release.send(());
    wait_finished(&server, 1);
    pump(&mut world);
    assert!(
      server.inner.finished_queue.is_empty(),
      "the result was applied"
    );
    let sent = events(&world, &mut cursor);
    assert!(
      matches!(sent.as_slice(), [AssetEvent::Removed { id: removed }] if *removed == id),
      "{sent:?}"
    );
    assert!(world.resource::<Assets<Text>>().is_empty());
    assert!(matches!(server.state(id), LoadState::NotLoaded));
    assert_eq!(server.path(id), None);
  }

  #[test]
  fn errors_keep_their_source() {
    let dir = tempfile::tempdir().map_err(|e| e.to_string());
    let Ok(dir) = dir else {
      panic!("a temp dir: {dir:?}");
    };
    let made = std::fs::create_dir(dir.path().join("folder.txt"))
      .and_then(|()| std::fs::write(dir.path().join("bad.txt"), [0xff, 0xfe]));
    assert!(made.is_ok());
    let (mut world, server) = world_with(crate::core::asset::DiskSource::new(dir.path()));
    let folder: Handle<Text> = server.load("folder.txt");
    let bad: Handle<Text> = server.load("bad.txt");
    pump_until(&mut world, |_| {
      server.state(&folder).is_failed() && server.state(&bad).is_failed()
    });
    let io = server.state(&folder).error().cloned();
    assert!(
      matches!(io.as_deref(), Some(AssetError::Io { .. })),
      "{io:?}"
    );
    let io_source = io.as_deref().and_then(std::error::Error::source);
    assert!(io_source.is_some_and(|s| s.is::<std::io::Error>()));
    let decode = server.state(&bad).error().cloned();
    assert!(
      matches!(decode.as_deref(), Some(AssetError::Decode { .. })),
      "{decode:?}"
    );
    let decode_source = decode.as_deref().and_then(std::error::Error::source);
    assert!(decode_source.is_some_and(|s| s.is::<std::string::FromUtf8Error>()));
  }

  #[test]
  fn progress_and_stats_count_a_group() {
    let files = MemorySource::new();
    files.insert("a.txt", "a");
    files.insert("b.txt", "b");
    files.insert("slow.txt", "s");
    let (release, held) = crossbeam_channel::unbounded();
    let (mut world, server) = world_with(HeldSource {
      files,
      release: held,
    });
    let a: Handle<Text> = server.load("a.txt");
    let b: Handle<Text> = server.load("b.txt");
    let missing: Handle<Text> = server.load("nope.txt");
    pump_until(&mut world, |_| {
      server.state(&a).is_loaded()
        && server.state(&b).is_loaded()
        && server.state(&missing).is_failed()
    });
    // Loaded last, so it holds back at most one worker
    let slow: Handle<Text> = server.load("slow.txt");
    pump(&mut world);
    let group = [
      a.id().untyped(),
      b.id().untyped(),
      missing.id().untyped(),
      slow.id().untyped(),
    ];
    let progress = server.progress(&group);
    assert_eq!(
      progress,
      Progress {
        loaded: 2,
        failed: 1,
        pending: 1
      }
    );
    assert!(!progress.is_done());
    let stats = server.stats();
    assert!(
      matches!(
        stats.types.as_slice(),
        [
          TypeStats {
            name: "Text",
            loaded: 2,
            loading: 1,
            failed: 1
          },
          TypeStats {
            name: "Link",
            loaded: 0,
            loading: 0,
            failed: 0
          },
        ]
      ),
      "{stats:?}"
    );
    let _ = release.send(());
    pump_until(&mut world, |_| server.state(&slow).is_loaded());
    assert!(server.progress(&group).is_done());
  }

  #[test]
  fn an_asset_added_in_code_counts_as_loaded() {
    let (mut world, server) = world_with(MemorySource::new());
    let added = world
      .resource_mut::<Assets<Text>>()
      .add(Text("made".into()));
    assert!(server.state(&added).is_loaded());
    assert_eq!(server.path(&added), None);
    assert_eq!(
      server.progress(&[added.id().untyped()]),
      Progress {
        loaded: 1,
        failed: 0,
        pending: 0
      }
    );
  }
}
