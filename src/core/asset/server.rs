use std::any::{Any, TypeId};
use std::borrow::{Borrow, Cow};
use std::collections::hash_map::Entry;
use std::hash::Hash;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, RwLock};

use crossbeam_channel::{Receiver, Sender, unbounded};

use super::assets::Ledger;
use super::event::{AssetStats, TypeStats};
use super::loader::{ErasedLoader, LoadMode, erase, settings_hash};
use super::path::{extensions, normalise};
use super::pump::{
  Arrival, Arrived, BROKEN_STORE, Finished, FreeUnheld, Loaded, Typed, Waiting, fail, free_unheld,
  run_check,
};
use super::workers::{INLINE_BUDGET, Workers};
use super::{
  Asset, AssetError, AssetEvent, AssetId, AssetLoader, AssetPath, AssetSettings, AssetSource,
  Assets, Handle, LoadContext, LoadState, Progress, UntypedAssetId,
};
use crate::core::ecs::Resource;
use crate::core::ecs::kernel::World;
use crate::core::hash::{FastMap, FastSet, fast_hash};
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
  /// Paths the source says changed: the pump reloads them
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
  /// Values whose dependencies are still loading, in the order they arrived
  waiting: Vec<Waiting>,
}

struct Info {
  /// Where its key is, so freeing it can remove the key
  key: Key,
  /// The settings it was loaded with; none for a refused load
  settings: Option<Arc<dyn Any + Send + Sync>>,
  /// The loader that makes it; none for a refused load
  loader: Option<Arc<dyn ErasedLoader>>,
  typed: Typed,
  state: LoadState,
  /// The job whose result counts; any other's is stale and dropped
  job: u64,
  /// Whether that job is on the workers, its result not yet in
  in_flight: bool,
  /// Whether its file changed while that job ran: the result is dropped,
  /// and it loads again
  again: bool,
  /// What the value in its store holds (loaded through its `LoadContext`)
  dependencies: Vec<UntypedAssetId>,
  /// The loaded assets whose values hold this one
  dependents: Vec<UntypedAssetId>,
  /// A hash of the last error it warned about, so an error warns once
  warned: Option<u64>,
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

/// A load to run: the asset, its job, and what reading and making it takes
struct LoadJob {
  id: UntypedAssetId,
  job: u64,
  path: AssetPath,
  loader: Arc<dyn ErasedLoader>,
  settings: Arc<dyn Any + Send + Sync>,
}

/// A waiting value the pump can act on, taken out of the waiting list
pub(crate) enum Settled {
  /// Its dependencies are all loaded
  Ready(Waiting),
  /// A dependency failed; the error names it
  Failed(Waiting, Arc<AssetError>),
  /// Freed or overtaken since it arrived: it drops
  Stale(Waiting),
}

/// A waiting value that fails, and why
pub(crate) type Failure = (Waiting, Arc<AssetError>);

/// A loaded asset that holds one that changed, and what checks it
pub(crate) struct Dependent {
  pub(crate) id: UntypedAssetId,
  pub(crate) typed: Typed,
  pub(crate) loader: Arc<dyn ErasedLoader>,
  pub(crate) path: AssetPath,
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
  /// loaded, loading or failed. A failed asset is not tried again until its
  /// file changes, or it is freed and asked for again.
  ///
  /// An asset whose loader loads dependencies is inserted only once they
  /// are all loaded, and its loader's `check` passes; one that fails fails it
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
  /// again; one already failed gives back its error, until it is freed or
  /// its file changes. One loading on a worker loads here instead, and the
  /// worker's result is dropped. Its dependencies are loaded here too, and
  /// its loader's `check` runs before it is inserted. On failure the error
  /// is returned and the state stays `Failed`
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

  /// Loaded, with every dependency loaded too (recursively)
  pub fn is_ready<T: Asset>(&self, id: impl Into<AssetId<T>>) -> bool {
    self.dependency_state(id).is_loaded()
  }

  /// The state counting dependencies: `Loading` until all are loaded,
  /// `Failed` with the first dependency's error wrapped in `Dependency`
  pub fn dependency_state<T: Asset>(&self, id: impl Into<AssetId<T>>) -> LoadState {
    let id = id.into().untyped();
    let own = self.state_of(id);
    if !own.is_loaded() {
      return own;
    }
    let table = self.inner.table();
    // An asset added in code has nothing recorded
    let Some(root) = table
      .infos
      .get(&id)
      .filter(|root| !root.dependencies.is_empty())
    else {
      return own;
    };
    // A safety net: through the server, a loaded asset's dependencies are
    // all loaded (a parent settles only once they are, and a loaded asset
    // never becomes `Loading` or `Failed`), so this walk agrees with `state`
    // unless that breaks. Depth first, in the order the loader asked; each
    // asset once, so a dependency two others share is counted once
    let mut seen = FastSet::default();
    seen.insert(id);
    let mut next: Vec<UntypedAssetId> = root.dependencies.iter().rev().copied().collect();
    let mut loading = false;
    while let Some(dependency) = next.pop() {
      if !seen.insert(dependency) {
        continue;
      }
      // Freed: its parent let go of it, so it holds nothing up
      let Some(info) = table.infos.get(&dependency) else {
        continue;
      };
      match &info.state {
        LoadState::Failed(error) => {
          return LoadState::Failed(Arc::new(AssetError::Dependency {
            path: table.path_of(id),
            dependency: table.path_of(dependency),
            error: Arc::clone(error),
          }));
        }
        LoadState::Loading => loading = true,
        LoadState::Loaded | LoadState::NotLoaded => {}
      }
      next.extend(info.dependencies.iter().rev());
    }
    if loading {
      LoadState::Loading
    } else {
      LoadState::Loaded
    }
  }

  /// Reloads every asset loaded from `path` (each settings variant) on the workers
  ///
  /// Each keeps its id. Once its new dependencies load and its loader's
  /// `check` passes, the new value is swapped in (`Modified`), and each
  /// asset holding it is checked again. A reload that fails keeps the last
  /// good value (`ReloadFailed`). A reload asked for while one is reading
  /// runs again once that one is in, so the last write wins. An asset that
  /// failed to load is loaded anew
  pub fn reload(&self, path: &str) {
    if let Ok(path) = normalise(path) {
      self.reload_normal(&path);
    }
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
    let settings = settings_value(settings, &*request.loader);
    let load = table.start(id, request, settings, Typed::of::<T>(), true);
    drop(table);
    self.queue_job(load);
    Ok(handle)
  }

  /// Queues `load` on the workers; its result goes to the pump
  fn queue_job(&self, load: LoadJob) {
    let server = Arc::downgrade(&self.inner);
    self.inner.workers.queue(Box::new(move || {
      // A server dropped before the job ran has no one to give the result to
      let Some(inner) = server.upgrade() else {
        return;
      };
      let server = AssetServer { inner };
      let LoadJob {
        id,
        job,
        path,
        loader,
        settings,
      } = load;
      let outcome = server
        .run_load(&path, &*loader, &*settings, LoadMode::Queued)
        .map_err(Arc::new);
      let _ = server.inner.finished.send(Finished { id, job, outcome });
    }));
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
    let job = table.fail(
      id,
      Key::Refused(text, settings_key),
      Arc::clone(&error),
      Typed::of::<T>(),
    );
    drop(table);
    let _ = self.inner.finished.send(Finished {
      id,
      job,
      outcome: Err(error),
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
    let settings = settings_value(settings, &*request.loader);
    let LoadJob {
      job,
      path,
      loader,
      settings,
      ..
    } = table.start(id, request, settings, Typed::of::<T>(), false);
    drop(table);

    stack.push(path.clone());
    let mode = LoadMode::Now {
      world: &mut *world,
      stack: &mut *stack,
    };
    let outcome = self.run_load(&path, &*loader, &*settings, mode);
    stack.pop();
    let arrived = Arrived {
      id,
      job,
      arrival: Arrival::First,
      typed: Typed::of::<T>(),
    };
    let settled = match outcome {
      Ok(Loaded {
        value,
        dependencies,
      }) => {
        let waiting = Waiting {
          arrived,
          value,
          dependencies,
        };
        self.settle_now(world, waiting, &*loader, &path)
      }
      Err(error) => Err(Arc::new(error)),
    };
    match settled {
      Ok(()) => Ok(handle),
      Err(error) => {
        fail(world, self, arrived, Arc::clone(&error));
        Err(NowError::Failed { path, error })
      }
    }
  }

  /// Puts a value `load_now` made in its store, its dependencies loaded
  /// already; an error if they lead back to it or its loader's check fails
  fn settle_now(
    &self,
    world: &mut World,
    waiting: Waiting,
    loader: &dyn ErasedLoader,
    path: &AssetPath,
  ) -> Result<(), Arc<AssetError>> {
    let Waiting {
      arrived,
      value,
      dependencies,
    } = waiting;
    // A cycle through a waiting reload, which `load_now`'s own stack cannot see
    let cycle = self.inner.table().cycle_through(arrived.id, &dependencies);
    if let Some((own, others)) = cycle {
      for (waiting, error) in others {
        let other = waiting.arrived;
        drop(waiting);
        fail(world, self, other, error);
      }
      return Err(own);
    }
    run_check(loader, &*value, world, path).map_err(Arc::new)?;
    let inserted = (arrived.typed.insert)(world, arrived.id, value, Arrival::First);
    // Its slot was reserved or found live just now, and `T` is registered
    debug_assert!(inserted, "a value load_now made goes in its store");
    if !inserted {
      return Err(Arc::new(AssetError::decode(path, BROKEN_STORE)));
    }
    self.set_loaded(arrived.id, arrived.job, dependencies);
    Ok(())
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

  /// Reloads each path the source says changed
  pub(crate) fn reload_changed(&self) {
    while let Ok(path) = self.inner.changed.try_recv() {
      self.reload_normal(path.as_str());
    }
  }

  /// [`AssetServer::reload`] of a normalised path
  fn reload_normal(&self, path: &str) {
    let mut jobs = Vec::new();
    {
      let mut table = self.inner.table();
      let ids: Vec<UntypedAssetId> = table
        .keys
        .values()
        .filter_map(|paths| paths.get(path).copied())
        .collect();
      for id in ids {
        let Some(info) = table.infos.get_mut(&id) else {
          continue;
        };
        // What that job reads may be older than the change
        if info.in_flight {
          info.again = true;
          continue;
        }
        jobs.extend(table.restart(id));
      }
    }
    for job in jobs {
      self.queue_job(job);
    }
  }

  pub(crate) fn next_finished(&self) -> Option<Finished> {
    self.inner.finished_queue.try_recv().ok()
  }

  /// The result of `job` for `id` is in: how it counts, or None if it does
  /// not (freed, overtaken, or read before a change, which loads it again)
  pub(crate) fn arrive(&self, id: UntypedAssetId, job: u64) -> Option<Arrived> {
    let mut table = self.inner.table();
    let info = table.infos.get_mut(&id).filter(|info| info.job == job)?;
    info.in_flight = false;
    let arrived = Arrived {
      id,
      job,
      arrival: if info.state.is_loaded() {
        Arrival::Reload
      } else {
        Arrival::First
      },
      typed: info.typed,
    };
    if std::mem::take(&mut info.again) {
      let again = table.restart(id);
      drop(table);
      if let Some(again) = again {
        self.queue_job(again);
      }
      return None;
    }
    Some(arrived)
  }

  /// Holds a value until its dependencies load. If they lead back to it, it
  /// fails with `Cycle` instead, and so does each other waiting value on the
  /// cycle: those failures come back, its own first
  pub(crate) fn wait(&self, waiting: Waiting) -> Vec<Failure> {
    let mut table = self.inner.table();
    match table.cycle_through(waiting.arrived.id, &waiting.dependencies) {
      None => {
        table.waiting.push(waiting);
        Vec::new()
      }
      Some((own, mut others)) => {
        others.insert(0, (waiting, own));
        others
      }
    }
  }

  /// The first waiting value the pump can act on, taken out of the list
  pub(crate) fn next_settled(&self) -> Option<Settled> {
    let mut table = self.inner.table();
    let (index, verdict) = table
      .waiting
      .iter()
      .enumerate()
      .find_map(|(index, waiting)| table.verdict(waiting).map(|verdict| (index, verdict)))?;
    let waiting = table.waiting.remove(index);
    Some(match verdict {
      Verdict::Ready => Settled::Ready(waiting),
      Verdict::Failed(error) => Settled::Failed(waiting, error),
      Verdict::Stale => Settled::Stale(waiting),
    })
  }

  /// The loader that checks `id`'s values, and its path
  pub(crate) fn checker(&self, id: UntypedAssetId) -> Option<(Arc<dyn ErasedLoader>, AssetPath)> {
    let table = self.inner.table();
    let info = table.infos.get(&id)?;
    let Key::Path(path, _) = &info.key else {
      return None;
    };
    Some((Arc::clone(info.loader.as_ref()?), path.clone()))
  }

  /// The path `id` is known by, for an error: its path, the text a refused
  /// load asked for, or a placeholder once it is forgotten
  pub(crate) fn known_path(&self, id: UntypedAssetId) -> AssetPath {
    self.inner.table().path_of(id)
  }

  /// `id`'s value of `job` is in its store, holding `dependencies`
  pub(crate) fn set_loaded(&self, id: UntypedAssetId, job: u64, dependencies: Vec<UntypedAssetId>) {
    let mut table = self.inner.table();
    let Some(info) = table.infos.get_mut(&id).filter(|info| info.job == job) else {
      return;
    };
    info.state = LoadState::Loaded;
    info.warned = None;
    let old = std::mem::replace(&mut info.dependencies, dependencies);
    table.relink(id, &old);
  }

  /// `id`'s first load, `job`, failed
  pub(crate) fn set_failed(&self, id: UntypedAssetId, job: u64, error: Arc<AssetError>) {
    let mut table = self.inner.table();
    if let Some(info) = table.infos.get_mut(&id).filter(|info| info.job == job) {
      info.state = LoadState::Failed(error);
    }
  }

  /// The loaded assets that hold `id`, directly or through others, each
  /// once, nearest first: breadth first over `dependents`, so by the fewest
  /// steps from `id` (not outermost last)
  pub(crate) fn dependents_of(&self, id: UntypedAssetId) -> Vec<Dependent> {
    let table = self.inner.table();
    let mut order = Vec::new();
    if table
      .infos
      .get(&id)
      .is_none_or(|info| info.dependents.is_empty())
    {
      return order;
    }
    let mut seen = FastSet::default();
    seen.insert(id);
    // Breadth first: `reached` is the queue, `at` its head
    let mut reached = vec![id];
    let mut at = 0;
    while let Some(&held) = reached.get(at) {
      at += 1;
      let Some(info) = table.infos.get(&held) else {
        continue;
      };
      for &dependent in &info.dependents {
        if !seen.insert(dependent) {
          continue;
        }
        let Some(holder) = table.infos.get(&dependent).filter(|i| i.state.is_loaded()) else {
          continue;
        };
        let (Key::Path(path, _), Some(loader)) = (&holder.key, &holder.loader) else {
          continue;
        };
        reached.push(dependent);
        order.push(Dependent {
          id: dependent,
          typed: holder.typed,
          loader: Arc::clone(loader),
          path: path.clone(),
        });
      }
    }
    order
  }

  /// Warns about `error` for `id`, unless it was the last error `id` warned about
  pub(crate) fn warn_once(&self, id: UntypedAssetId, error: &AssetError) {
    let report = error.report();
    let hash = fast_hash(&report);
    let fresh = self
      .inner
      .table()
      .infos
      .get_mut(&id)
      .is_none_or(|info| info.warned.replace(hash) != Some(hash));
    if fresh {
      crate::engine_warn!(module: "asset", "{report}");
    }
  }

  /// `id` passed again: its next error warns, even one it warned about before
  pub(crate) fn clear_warning(&self, id: UntypedAssetId) {
    if let Some(info) = self.inner.table().infos.get_mut(&id) {
      info.warned = None;
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

  /// Records a load of `id` under its request's key, and gives the job to
  /// run; `in_flight` if it runs on the workers
  fn start(
    &mut self,
    id: UntypedAssetId,
    request: Request<'_>,
    settings: Arc<dyn Any + Send + Sync>,
    typed: Typed,
    in_flight: bool,
  ) -> LoadJob {
    let job = self.next_job();
    let path = AssetPath::from_normal(request.path);
    self
      .keys
      .entry((id.type_id, request.settings_hash))
      .or_default()
      .insert(path.clone(), id);
    // `load_now` starts again an asset loading on a worker: what holds it stays
    let dependents = self
      .infos
      .remove(&id)
      .map(|info| info.dependents)
      .unwrap_or_default();
    self.infos.insert(
      id,
      Info {
        key: Key::Path(path.clone(), request.settings_hash),
        settings: Some(Arc::clone(&settings)),
        loader: Some(Arc::clone(&request.loader)),
        typed,
        state: LoadState::Loading,
        job,
        in_flight,
        again: false,
        dependencies: Vec::new(),
        dependents,
        warned: None,
      },
    );
    LoadJob {
      id,
      job,
      path,
      loader: request.loader,
      settings,
    }
  }

  /// Starts `id`'s load again under a new job, from its path, loader and
  /// settings: a reload when it is loaded, else a load anew. A result of
  /// an older job is stale from now on
  fn restart(&mut self, id: UntypedAssetId) -> Option<LoadJob> {
    let job = self.last_job + 1;
    let info = self.infos.get_mut(&id)?;
    let (Key::Path(path, _), Some(loader), Some(settings)) =
      (&info.key, &info.loader, &info.settings)
    else {
      return None;
    };
    let load = LoadJob {
      id,
      job,
      path: path.clone(),
      loader: Arc::clone(loader),
      settings: Arc::clone(settings),
    };
    info.job = job;
    info.in_flight = true;
    info.again = false;
    if !info.state.is_loaded() {
      info.state = LoadState::Loading;
    }
    self.last_job = job;
    Some(load)
  }

  /// Records `id` as refused under `key`, failed before it could load, and
  /// gives its job number
  fn fail(&mut self, id: UntypedAssetId, key: Key, error: Arc<AssetError>, typed: Typed) -> u64 {
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
        loader: None,
        typed,
        state: LoadState::Failed(error),
        job,
        in_flight: false,
        again: false,
        dependencies: Vec::new(),
        dependents: Vec::new(),
        warned: None,
      },
    );
    job
  }

  /// Tells what `id`'s value holds that `id` holds it, and what it held
  /// before (`old`) and no longer does that it does not
  fn relink(&mut self, id: UntypedAssetId, old: &[UntypedAssetId]) {
    let Some(info) = self.infos.get_mut(&id) else {
      return;
    };
    let dependencies = std::mem::take(&mut info.dependencies);
    for dependency in old.iter().filter(|d| !dependencies.contains(d)) {
      if let Some(held) = self.infos.get_mut(dependency) {
        held.dependents.retain(|&dependent| dependent != id);
      }
    }
    for dependency in &dependencies {
      if let Some(held) = self.infos.get_mut(dependency)
        && !held.dependents.contains(&id)
      {
        held.dependents.push(id);
      }
    }
    if let Some(info) = self.infos.get_mut(&id) {
      info.dependencies = dependencies;
    }
  }

  /// Whether `job` is still the one whose result counts for `id`
  fn is_current(&self, id: UntypedAssetId, job: u64) -> bool {
    self.infos.get(&id).is_some_and(|info| info.job == job)
  }

  /// Where `id`'s current waiting value is in the list
  fn waiting_index(&self, id: UntypedAssetId) -> Option<usize> {
    self
      .waiting
      .iter()
      .position(|waiting| waiting.arrived.id == id && self.is_current(id, waiting.arrived.job))
  }

  /// What a waiting value can do now; None while a dependency is loading
  fn verdict(&self, waiting: &Waiting) -> Option<Verdict> {
    let Arrived { id, job, .. } = waiting.arrived;
    if !self.is_current(id, job) {
      return Some(Verdict::Stale);
    }
    let mut loading = false;
    for &dependency in &waiting.dependencies {
      match self.infos.get(&dependency).map(|info| &info.state) {
        Some(LoadState::Failed(error)) => {
          return Some(Verdict::Failed(Arc::new(AssetError::Dependency {
            path: self.path_of(id),
            dependency: self.path_of(dependency),
            error: Arc::clone(error),
          })));
        }
        Some(LoadState::Loading) => loading = true,
        // Loaded, or freed: a dependency its parent let go of holds nothing up
        _ => {}
      }
    }
    (!loading).then_some(Verdict::Ready)
  }

  /// What `id` will hold: its waiting value's dependencies if it has one,
  /// else those of the value in its store
  fn edges(&self, id: UntypedAssetId) -> &[UntypedAssetId] {
    match self
      .waiting_index(id)
      .and_then(|index| self.waiting.get(index))
    {
      Some(waiting) => &waiting.dependencies,
      None => self
        .infos
        .get(&id)
        .map(|info| info.dependencies.as_slice())
        .unwrap_or_default(),
    }
  }

  /// The assets on a cycle through `id`, `id` first, each holding the next
  /// and the last holding `id`, if `dependencies` (what `id` will hold) lead
  /// back to it
  fn find_cycle(
    &self,
    id: UntypedAssetId,
    dependencies: &[UntypedAssetId],
  ) -> Option<Vec<UntypedAssetId>> {
    // Each asset reached, and the one it was reached from
    let mut reached: FastMap<UntypedAssetId, UntypedAssetId> = FastMap::default();
    let mut next = Vec::new();
    for &dependency in dependencies {
      if dependency == id {
        return Some(vec![id]);
      }
      if let Entry::Vacant(slot) = reached.entry(dependency) {
        slot.insert(id);
        next.push(dependency);
      }
    }
    while let Some(at) = next.pop() {
      for &held in self.edges(at) {
        if held == id {
          let mut members = vec![at];
          let mut step = at;
          while let Some(&from) = reached.get(&step).filter(|&&from| from != id) {
            members.push(from);
            step = from;
          }
          members.push(id);
          members.reverse();
          return Some(members);
        }
        if let Entry::Vacant(slot) = reached.entry(held) {
          slot.insert(at);
          next.push(held);
        }
      }
    }
    None
  }

  /// If `dependencies` (what `id` will hold) lead back to `id`: its `Cycle`
  /// error, and each other waiting value on the cycle, taken out of the
  /// list, with its own. Each error names the chain from its asset
  fn cycle_through(
    &mut self,
    id: UntypedAssetId,
    dependencies: &[UntypedAssetId],
  ) -> Option<(Arc<AssetError>, Vec<Failure>)> {
    let members = self.find_cycle(id, dependencies)?;
    let paths: Vec<AssetPath> = members.iter().map(|&member| self.path_of(member)).collect();
    let mut own = None;
    let mut others = Vec::new();
    for (at, (&member, path)) in members.iter().zip(&paths).enumerate() {
      let chain = paths
        .iter()
        .cycle()
        .skip(at)
        .take(paths.len() + 1)
        .map(AssetPath::as_str)
        .collect::<Vec<_>>()
        .join(" → ");
      let error = Arc::new(AssetError::Cycle {
        path: path.clone(),
        chain,
      });
      if member == id {
        own = Some(error);
      } else if let Some(index) = self.waiting_index(member) {
        others.push((self.waiting.remove(index), error));
      }
    }
    // `members` starts with `id`, so `own` is always there
    own.map(|own| (own, others))
  }

  /// The path `id` is known by: its path, or the text a refused load asked for
  fn path_of(&self, id: UntypedAssetId) -> AssetPath {
    let text = match self.infos.get(&id).map(|info| &info.key) {
      Some(Key::Path(path, _)) => return path.clone(),
      Some(Key::Refused(text, _)) => &**text,
      None => "(freed)",
    };
    AssetPath::from_normal(Cow::Borrowed(text))
  }

  fn forget(&mut self, id: UntypedAssetId) {
    let Some(info) = self.infos.remove(&id) else {
      return;
    };
    for dependency in &info.dependencies {
      if let Some(held) = self.infos.get_mut(dependency) {
        held.dependents.retain(|&dependent| dependent != id);
      }
    }
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

/// What a waiting value can do now
enum Verdict {
  /// Be checked and put in its store
  Ready,
  /// Fail, a dependency having failed
  Failed(Arc<AssetError>),
  /// Drop: freed or overtaken
  Stale,
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
  use std::time::{Duration, Instant};

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

  /// A file of paths, one a line, each loaded as a dependency: another
  /// bundle for a ".bundle" path, else a `Text`
  struct Bundle {
    path: AssetPath,
    parts: Vec<Part>,
  }
  impl Asset for Bundle {}

  // Held so the dependencies live as long as the bundle
  #[allow(dead_code)]
  enum Part {
    Text(Handle<Text>),
    Bundle(Handle<Bundle>),
  }

  struct BundleLoader;
  impl AssetLoader for BundleLoader {
    type Asset = Bundle;
    type Settings = ();
    fn extensions(&self) -> &[&'static str] {
      &["bundle"]
    }
    fn load(&self, bytes: Vec<u8>, _: &(), cx: &mut LoadContext<'_>) -> Result<Bundle, AssetError> {
      let text = String::from_utf8(bytes).map_err(|e| AssetError::decode(cx.path(), e))?;
      let mut parts = Vec::new();
      for line in text.lines().map(str::trim).filter(|line| !line.is_empty()) {
        parts.push(if line.ends_with(".bundle") {
          Part::Bundle(cx.load(line)?)
        } else {
          Part::Text(cx.load(line)?)
        });
      }
      Ok(Bundle {
        path: cx.path().clone(),
        parts,
      })
    }
    /// Fails when a text it holds reads "bad"
    fn check(&self, bundle: &Bundle, world: &World) -> Result<(), AssetError> {
      let texts = world.resource::<Assets<Text>>();
      let bad = bundle.parts.iter().any(
        |part| matches!(part, Part::Text(text) if texts.get(text).is_some_and(|t| t.0 == "bad")),
      );
      if bad {
        return Err(AssetError::decode(&bundle.path, "it holds a bad text"));
      }
      Ok(())
    }
  }

  fn world_with(source: impl AssetSource) -> (World, AssetServer) {
    let mut world = World::new();
    let server = AssetServer::new(source);
    server.register_asset::<Text>(&mut world);
    server.register_asset::<Link>(&mut world);
    server.register_asset::<Bundle>(&mut world);
    server.register_loader(TextLoader);
    server.register_loader(BoomLoader);
    server.register_loader(UpperLoader);
    server.register_loader(LinkLoader);
    server.register_loader(BundleLoader);
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

  fn events<T: Asset>(
    world: &World,
    cursor: &mut MessageCursor<AssetEvent<T>>,
  ) -> Vec<AssetEvent<T>> {
    cursor
      .read(world.resource::<Messages<AssetEvent<T>>>())
      .cloned()
      .collect()
  }

  /// A cursor that reads only the events sent from now on
  fn cursor<T: Asset>(world: &World) -> MessageCursor<AssetEvent<T>> {
    world
      .resource::<Messages<AssetEvent<T>>>()
      .get_cursor_current()
  }

  /// Pumps until an event `done` accepts is sent; gives back every event
  /// sent meanwhile
  fn pump_until_sent<T: Asset>(
    world: &mut World,
    cursor: &mut MessageCursor<AssetEvent<T>>,
    done: impl Fn(&AssetEvent<T>) -> bool,
  ) -> Vec<AssetEvent<T>> {
    let start = Instant::now();
    let mut sent = Vec::new();
    loop {
      pump(world);
      sent.extend(events(world, cursor));
      if sent.iter().any(&done) {
        return sent;
      }
      assert!(start.elapsed() < PATIENCE, "timed out waiting: {sent:?}");
      std::thread::sleep(Duration::from_millis(1));
    }
  }

  /// How many parts the bundle holds, once it is in its store
  fn parts(world: &World, id: impl Into<AssetId<Bundle>>) -> Option<usize> {
    world
      .resource::<Assets<Bundle>>()
      .get(id)
      .map(|bundle| bundle.parts.len())
  }

  fn version<T: Asset>(world: &World, id: impl Into<AssetId<T>>) -> Option<u64> {
    world.resource::<Assets<T>>().version(id)
  }

  /// Results whose dependencies are still loading
  fn waiting(server: &AssetServer) -> usize {
    server.inner.table().waiting.len()
  }

  /// The asset's `Cycle` error, as (path, chain)
  fn cycle_of(error: Option<&AssetError>) -> Option<(String, String)> {
    match error {
      Some(AssetError::Cycle { path, chain }) => Some((path.to_string(), chain.clone())),
      _ => None,
    }
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

  /// Files in memory that tell no one when they change: only `reload` reloads
  struct Unwatched(MemorySource);
  impl AssetSource for Unwatched {
    fn read(&self, path: &AssetPath) -> Result<Vec<u8>, AssetError> {
      self.0.read(path)
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

  /// A source whose every read takes the bytes, says so, then waits until
  /// the test lets it go
  struct Gated {
    files: MemorySource,
    reading: crossbeam_channel::Sender<()>,
    release: crossbeam_channel::Receiver<()>,
  }
  impl AssetSource for Gated {
    fn read(&self, path: &AssetPath) -> Result<Vec<u8>, AssetError> {
      let bytes = self.files.read(path);
      let _ = self.reading.send(());
      let _ = self.release.recv();
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
    // Unwatched: a reload of the change would land "two" as well, hiding a
    // stale "one" applied
    let (mut world, server) = world_with(Unwatched(source.clone()));
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
          TypeStats {
            name: "Bundle",
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

  // Dependencies, readiness and reloads

  #[test]
  fn a_parent_is_added_only_once_its_dependencies_are_loaded() {
    let files = MemorySource::new();
    files.insert("x.bundle", "a.txt\nslow.txt");
    files.insert("a.txt", "a");
    files.insert("slow.txt", "s");
    let (release, held) = crossbeam_channel::unbounded();
    let (mut world, server) = world_with(HeldSource {
      files,
      release: held,
    });
    let mut texts = cursor::<Text>(&world);
    let mut bundles = cursor::<Bundle>(&world);
    let bundle: Handle<Bundle> = server.load("x.bundle");
    let a: Handle<Text> = server.load("a.txt");
    // The bundle's result is in, and waits for slow.txt
    pump_until(&mut world, |_| {
      server.state(&a).is_loaded() && waiting(&server) == 1
    });
    assert_eq!(parts(&world, &bundle), None, "not added while one loads");
    assert!(matches!(server.state(&bundle), LoadState::Loading));
    assert!(!server.is_ready(&bundle));
    assert!(matches!(
      server.dependency_state(&bundle),
      LoadState::Loading
    ));
    assert!(events(&world, &mut bundles).is_empty());
    let _ = release.send(());
    // By the pump that sends Added<Bundle>, both texts were added
    let start = Instant::now();
    let mut added_texts = 0;
    loop {
      pump(&mut world);
      added_texts += events(&world, &mut texts)
        .iter()
        .filter(|e| matches!(e, AssetEvent::Added { .. }))
        .count();
      let sent = events(&world, &mut bundles);
      if !sent.is_empty() {
        assert!(
          matches!(sent.as_slice(), [AssetEvent::Added { id }] if *id == bundle.id()),
          "{sent:?}"
        );
        break;
      }
      assert!(start.elapsed() < PATIENCE, "timed out waiting");
      std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(added_texts, 2);
    assert_eq!(parts(&world, &bundle), Some(2));
    assert!(server.is_ready(&bundle));
    assert!(server.dependency_state(&bundle).is_loaded());
  }

  #[test]
  fn a_failed_dependency_fails_its_parent_with_dependency() {
    let source = MemorySource::new();
    source.insert("x.bundle", "a.txt\nmissing.txt");
    source.insert("a.txt", "a");
    let (mut world, server) = world_with(source);
    let mut bundles = cursor::<Bundle>(&world);
    let bundle: Handle<Bundle> = server.load("x.bundle");
    pump_until(&mut world, |_| server.state(&bundle).is_failed());
    let error = server.state(&bundle).error().cloned();
    assert!(
      matches!(
        error.as_deref(),
        Some(AssetError::Dependency { path, dependency, error })
          if path.as_str() == "x.bundle"
            && dependency.as_str() == "missing.txt"
            && matches!(**error, AssetError::NotFound { .. })
      ),
      "{error:?}"
    );
    assert!(server.dependency_state(&bundle).is_failed());
    assert!(!server.is_ready(&bundle));
    assert_eq!(parts(&world, &bundle), None);
    let sent = events(&world, &mut bundles);
    assert!(
      matches!(sent.as_slice(), [AssetEvent::Failed { id, .. }] if *id == bundle.id()),
      "{sent:?}"
    );
  }

  #[test]
  fn a_parent_whose_check_fails_is_failed() {
    let source = MemorySource::new();
    source.insert("x.bundle", "a.txt");
    source.insert("y.bundle", "a.txt");
    source.insert("a.txt", "bad");
    let (mut world, server) = world_with(source);
    let mut bundles = cursor::<Bundle>(&world);
    let queued: Handle<Bundle> = server.load("x.bundle");
    pump_until(&mut world, |_| server.state(&queued).is_failed());
    let error = server.state(&queued).error().map(|e| e.to_string());
    assert_eq!(error.as_deref(), Some("could not load \"x.bundle\""));
    let sent = events(&world, &mut bundles);
    assert!(
      matches!(sent.as_slice(), [AssetEvent::Failed { id, .. }] if *id == queued.id()),
      "{sent:?}"
    );
    let now = server.load_now::<Bundle>(&mut world, "y.bundle").err();
    assert!(
      matches!(now.as_deref(), Some(AssetError::Decode { path, .. }) if path.as_str() == "y.bundle"),
      "{now:?}"
    );
  }

  #[test]
  fn a_reload_keeps_the_id_and_moves_the_version_on() {
    let source = MemorySource::new();
    source.insert("a.txt", "one");
    let (mut world, server) = world_with(source.clone());
    let handle = server.load_now::<Text>(&mut world, "a.txt").expect("loads");
    let before = world.resource::<Assets<Text>>().version(&handle);
    source.insert("a.txt", "two"); // MemorySource tells the server
    pump_until(&mut world, |w| {
      w.resource::<Assets<Text>>()
        .get(&handle)
        .map(|t| t.0.as_str())
        == Some("two")
    });
    assert!(world.resource::<Assets<Text>>().version(&handle) > before);
  }

  #[test]
  fn a_broken_reload_keeps_the_last_good_value() {
    let source = MemorySource::new();
    source.insert("a.txt", "one");
    let (mut world, server) = world_with(source.clone());
    let handle = server.load_now::<Text>(&mut world, "a.txt").expect("loads");
    let mut texts = cursor::<Text>(&world);
    source.insert("a.txt", [0xff, 0xfe]);
    let sent = pump_until_sent(&mut world, &mut texts, |e| {
      matches!(e, AssetEvent::ReloadFailed { .. })
    });
    assert!(
      matches!(
        sent.as_slice(),
        [AssetEvent::ReloadFailed { id, error }]
          if *id == handle.id() && matches!(**error, AssetError::Decode { .. })
      ),
      "{sent:?}"
    );
    assert_eq!(text(&world, &handle), Some("one"));
    assert!(server.state(&handle).is_loaded());
  }

  #[test]
  fn a_broken_reload_warns_once_per_error() {
    use crate::core::ui::test_util::warnings_containing;
    let warned = || warnings_containing("warn-once.txt");
    let source = MemorySource::new();
    source.insert("warn-once.txt", "one");
    let (mut world, server) = world_with(source.clone());
    let handle = server
      .load_now::<Text>(&mut world, "warn-once.txt")
      .expect("loads");
    let mut texts = cursor::<Text>(&world);
    let reload_failed = |e: &AssetEvent<Text>| matches!(e, AssetEvent::ReloadFailed { .. });
    source.insert("warn-once.txt", [0xff]);
    pump_until_sent(&mut world, &mut texts, reload_failed);
    assert_eq!(warned(), 1);
    source.insert("warn-once.txt", [0xff]);
    pump_until_sent(&mut world, &mut texts, reload_failed);
    assert_eq!(warned(), 1, "the same error warns once");
    source.insert("warn-once.txt", "two");
    pump_until_sent(&mut world, &mut texts, |e| {
      matches!(e, AssetEvent::Modified { .. })
    });
    source.insert("warn-once.txt", [0xff]);
    pump_until_sent(&mut world, &mut texts, reload_failed);
    assert_eq!(warned(), 2, "broken again after a good reload");
    assert_eq!(text(&world, &handle), Some("two"));
  }

  #[test]
  fn a_reloaded_dependency_tells_its_dependents() {
    let source = MemorySource::new();
    source.insert("x.bundle", "a.txt\nb.txt");
    source.insert("a.txt", "a");
    source.insert("b.txt", "b");
    let (mut world, server) = world_with(source.clone());
    let bundle = server
      .load_now::<Bundle>(&mut world, "x.bundle")
      .expect("loads");
    let a: Handle<Text> = server.load("a.txt");
    let before = version(&world, &bundle);
    let mut texts = cursor::<Text>(&world);
    let mut bundles = cursor::<Bundle>(&world);
    source.insert("a.txt", "changed");
    let sent = pump_until_sent(&mut world, &mut bundles, |e| {
      matches!(e, AssetEvent::Modified { .. })
    });
    assert!(
      matches!(sent.as_slice(), [AssetEvent::Modified { id }] if *id == bundle.id()),
      "{sent:?}"
    );
    let sent = events(&world, &mut texts);
    assert!(
      matches!(sent.as_slice(), [AssetEvent::Modified { id }] if *id == a.id()),
      "{sent:?}"
    );
    assert_eq!(text(&world, &a), Some("changed"));
    assert!(version(&world, &bundle) > before);
  }

  #[test]
  fn a_dependency_reload_that_fails_its_dependents_check_is_reported() {
    let source = MemorySource::new();
    source.insert("x.bundle", "a.txt\nb.txt");
    source.insert("a.txt", "a");
    source.insert("b.txt", "b");
    let (mut world, server) = world_with(source.clone());
    let bundle = server
      .load_now::<Bundle>(&mut world, "x.bundle")
      .expect("loads");
    let a: Handle<Text> = server.load("a.txt");
    let before = version(&world, &bundle);
    let mut texts = cursor::<Text>(&world);
    let mut bundles = cursor::<Bundle>(&world);
    source.insert("a.txt", "bad");
    let sent = pump_until_sent(&mut world, &mut bundles, |e| {
      matches!(e, AssetEvent::ReloadFailed { .. })
    });
    assert!(
      matches!(
        sent.as_slice(),
        [AssetEvent::ReloadFailed { id, error }]
          if *id == bundle.id() && matches!(**error, AssetError::Decode { .. })
      ),
      "{sent:?}"
    );
    let sent = events(&world, &mut texts);
    assert!(
      matches!(sent.as_slice(), [AssetEvent::Modified { id }] if *id == a.id()),
      "{sent:?}"
    );
    assert_eq!(version(&world, &bundle), before, "its value is as it was");
    assert!(server.state(&bundle).is_loaded());
  }

  #[test]
  fn freeing_a_parent_frees_dependencies_nothing_else_holds() {
    let source = MemorySource::new();
    source.insert("x.bundle", "a.txt\nb.txt");
    source.insert("a.txt", "a");
    source.insert("b.txt", "b");
    let (mut world, server) = world_with(source);
    let bundle = server
      .load_now::<Bundle>(&mut world, "x.bundle")
      .expect("loads");
    let a = server.load::<Text>("a.txt").id();
    let b: Handle<Text> = server.load("b.txt");
    let mut texts = cursor::<Text>(&world);
    let mut bundles = cursor::<Bundle>(&world);
    let id = bundle.id();
    drop(bundle);
    pump(&mut world);
    pump(&mut world);
    let sent = events(&world, &mut bundles);
    assert!(
      matches!(sent.as_slice(), [AssetEvent::Removed { id: removed }] if *removed == id),
      "{sent:?}"
    );
    let sent = events(&world, &mut texts);
    assert!(
      matches!(sent.as_slice(), [AssetEvent::Removed { id: removed }] if *removed == a),
      "only the text nothing else holds: {sent:?}"
    );
    assert_eq!(text(&world, &b), Some("b"));
    assert!(matches!(server.state(a), LoadState::NotLoaded));
  }

  #[test]
  fn load_now_loads_dependencies_now_and_rejects_cycles() {
    let source = MemorySource::new();
    source.insert("x.bundle", "a.txt\nb.txt");
    source.insert("a.txt", "a");
    source.insert("b.txt", "b");
    source.insert("a.bundle", "b.bundle");
    source.insert("b.bundle", "a.bundle");
    let (mut world, server) = world_with(source);
    let bundle = server
      .load_now::<Bundle>(&mut world, "x.bundle")
      .expect("loads");
    assert_eq!(parts(&world, &bundle), Some(2));
    assert_eq!(world.resource::<Assets<Text>>().len(), 2);
    assert!(server.is_ready(&bundle));
    let error = server.load_now::<Bundle>(&mut world, "a.bundle").err();
    assert_eq!(
      cycle_of(error.as_deref()),
      Some((
        "a.bundle".to_owned(),
        "a.bundle → b.bundle → a.bundle".to_owned()
      )),
      "{error:?}"
    );
  }

  #[test]
  fn a_cycle_through_queued_loads_fails_with_cycle() {
    let source = MemorySource::new();
    source.insert("a.bundle", "b.bundle");
    source.insert("b.bundle", "a.bundle");
    source.insert("me.bundle", "me.bundle");
    let (mut world, server) = world_with(source);
    let mut bundles = cursor::<Bundle>(&world);
    let a: Handle<Bundle> = server.load("a.bundle");
    let me: Handle<Bundle> = server.load("me.bundle");
    pump_until(&mut world, |_| {
      server.state(&a).is_failed() && server.state(&me).is_failed()
    });
    let chain = |handle: &Handle<Bundle>| cycle_of(server.state(handle).error().map(|e| &**e));
    assert_eq!(
      chain(&a),
      Some((
        "a.bundle".to_owned(),
        "a.bundle → b.bundle → a.bundle".to_owned()
      ))
    );
    assert_eq!(
      chain(&me),
      Some(("me.bundle".to_owned(), "me.bundle → me.bundle".to_owned()))
    );
    // Every asset on a cycle fails, each naming the chain from itself
    let sent = events(&world, &mut bundles);
    let mut chains: Vec<String> = sent
      .iter()
      .filter_map(|e| match e {
        AssetEvent::Failed { error, .. } => cycle_of(Some(error)).map(|(_, chain)| chain),
        _ => None,
      })
      .collect();
    chains.sort();
    assert_eq!(
      chains,
      [
        "a.bundle → b.bundle → a.bundle",
        "b.bundle → a.bundle → b.bundle",
        "me.bundle → me.bundle"
      ],
      "{sent:?}"
    );
    // Nothing on a cycle holds another now, so all three are freed
    drop(a);
    drop(me);
    pump(&mut world);
    pump(&mut world);
    let removed = events(&world, &mut bundles)
      .iter()
      .chain(&sent)
      .filter(|e| matches!(e, AssetEvent::Removed { .. }))
      .count();
    assert_eq!(removed, 3);
  }

  #[test]
  fn a_reload_that_would_make_a_cycle_fails_and_keeps_the_last_value() {
    let source = MemorySource::new();
    source.insert("x.bundle", "a.txt");
    source.insert("y.bundle", "x.bundle");
    source.insert("a.txt", "a");
    let (mut world, server) = world_with(source.clone());
    let y = server
      .load_now::<Bundle>(&mut world, "y.bundle")
      .expect("loads");
    let x: Handle<Bundle> = server.load("x.bundle");
    let mut bundles = cursor::<Bundle>(&world);
    source.insert("x.bundle", "y.bundle");
    let sent = pump_until_sent(&mut world, &mut bundles, |e| {
      matches!(e, AssetEvent::ReloadFailed { .. })
    });
    let error = match sent.as_slice() {
      [AssetEvent::ReloadFailed { id, error }] if *id == x.id() => Some(&**error),
      _ => None,
    };
    assert_eq!(
      cycle_of(error),
      Some((
        "x.bundle".to_owned(),
        "x.bundle → y.bundle → x.bundle".to_owned()
      )),
      "{sent:?}"
    );
    let held = world
      .resource::<Assets<Bundle>>()
      .get(&x)
      .map(|bundle| matches!(bundle.parts.as_slice(), [Part::Text(_)]));
    assert_eq!(held, Some(true), "the last value stays");
    assert!(server.state(&x).is_loaded());
    // No cycle was made, so both are freed once unheld
    drop(x);
    drop(y);
    pump(&mut world);
    pump(&mut world);
    assert!(world.resource::<Assets<Bundle>>().is_empty());
  }

  #[test]
  fn a_reload_with_a_new_dependency_waits_for_it() {
    let files = MemorySource::new();
    files.insert("x.bundle", "a.txt");
    files.insert("a.txt", "a");
    files.insert("slow.txt", "s");
    let (release, held) = crossbeam_channel::unbounded();
    let (mut world, server) = world_with(HeldSource {
      files: files.clone(),
      release: held,
    });
    let bundle = server
      .load_now::<Bundle>(&mut world, "x.bundle")
      .expect("loads");
    let before = version(&world, &bundle);
    let mut bundles = cursor::<Bundle>(&world);
    files.insert("x.bundle", "a.txt\nslow.txt");
    server.reload("x.bundle");
    pump_until(&mut world, |_| waiting(&server) == 1);
    assert_eq!(
      parts(&world, &bundle),
      Some(1),
      "the old value stays meanwhile"
    );
    assert_eq!(version(&world, &bundle), before);
    assert!(server.is_ready(&bundle));
    let _ = release.send(());
    let sent = pump_until_sent(&mut world, &mut bundles, |e| {
      matches!(e, AssetEvent::Modified { .. })
    });
    assert!(
      matches!(sent.as_slice(), [AssetEvent::Modified { id }] if *id == bundle.id()),
      "{sent:?}"
    );
    assert_eq!(parts(&world, &bundle), Some(2));
    assert!(server.is_ready(&bundle));
  }

  #[test]
  fn reload_reloads_every_settings_variant() {
    let files = MemorySource::new();
    files.insert("a.up", "hi");
    let (mut world, server) = world_with(Unwatched(files.clone()));
    let plain = server
      .load_now_with::<Text, _>(&mut world, "a.up", Upper(false))
      .expect("loads");
    let upper = server
      .load_now_with::<Text, _>(&mut world, "a.up", Upper(true))
      .expect("loads");
    files.insert("a.up", "bye");
    server.reload("./a.up");
    pump_until(&mut world, |w| {
      text(w, &plain) == Some("bye") && text(w, &upper) == Some("BYE")
    });
  }

  #[test]
  fn a_change_during_a_reload_loads_again_when_it_finishes() {
    let files = MemorySource::new();
    files.insert("a.txt", "one");
    let (reading, read) = crossbeam_channel::unbounded();
    let (release, held) = crossbeam_channel::unbounded();
    let (mut world, server) = world_with(Gated {
      files: files.clone(),
      reading,
      release: held,
    });
    let _ = release.send(());
    let handle = server.load_now::<Text>(&mut world, "a.txt").expect("loads");
    let mut texts = cursor::<Text>(&world);
    files.insert("a.txt", "two");
    server.reload("a.txt");
    // The reload has read "two" (after load_now's read) and is held
    for _ in 0..2 {
      assert!(read.recv_timeout(PATIENCE).is_ok(), "timed out waiting");
    }
    files.insert("a.txt", "three");
    server.reload("a.txt");
    // The held read goes on; the next is not started until its result is in
    let _ = release.send(());
    wait_finished(&server, 1);
    assert!(read.try_recv().is_err(), "one read at a time");
    let _ = release.send(());
    let sent = pump_until_sent(&mut world, &mut texts, |e| {
      matches!(e, AssetEvent::Modified { .. })
    });
    assert_eq!(text(&world, &handle), Some("three"), "the last write wins");
    assert!(
      matches!(sent.as_slice(), [AssetEvent::Modified { id }] if *id == handle.id()),
      "the outdated read was dropped: {sent:?}"
    );
  }

  #[test]
  fn a_failed_asset_loads_when_its_file_changes() {
    let source = MemorySource::new();
    source.insert("a.txt", [0xff]);
    let (mut world, server) = world_with(source.clone());
    let handle: Handle<Text> = server.load("a.txt");
    pump_until(&mut world, |_| server.state(&handle).is_failed());
    let mut texts = cursor::<Text>(&world);
    source.insert("a.txt", "fixed");
    let sent = pump_until_sent(&mut world, &mut texts, |e| {
      matches!(e, AssetEvent::Added { .. })
    });
    assert!(
      matches!(sent.as_slice(), [AssetEvent::Added { id }] if *id == handle.id()),
      "{sent:?}"
    );
    assert_eq!(text(&world, &handle), Some("fixed"));
    assert!(server.state(&handle).is_loaded());
  }

  #[test]
  fn a_reload_tells_dependents_nearest_first() {
    let source = MemorySource::new();
    source.insert("outer.bundle", "inner.bundle");
    source.insert("inner.bundle", "a.txt");
    source.insert("a.txt", "a");
    let (mut world, server) = world_with(source.clone());
    let outer = server
      .load_now::<Bundle>(&mut world, "outer.bundle")
      .expect("loads");
    let inner: Handle<Bundle> = server.load("inner.bundle");
    let mut bundles = cursor::<Bundle>(&world);
    source.insert("a.txt", "changed");
    let sent = pump_until_sent(
      &mut world,
      &mut bundles,
      |e| matches!(e, AssetEvent::Modified { id } if *id == outer.id()),
    );
    assert!(
      matches!(
        sent.as_slice(),
        [AssetEvent::Modified { id: first }, AssetEvent::Modified { id: last }]
          if *first == inner.id() && *last == outer.id()
      ),
      "{sent:?}"
    );
  }

  #[test]
  fn a_load_now_closing_a_cycle_through_a_waiting_reload_fails_both() {
    let files = MemorySource::new();
    files.insert("x.bundle", "a.txt");
    files.insert("a.txt", "a");
    files.insert("p.bundle", "x.bundle");
    let (reading, read) = crossbeam_channel::unbounded();
    let (release, held) = crossbeam_channel::unbounded();
    let (mut world, server) = world_with(FirstReadHeld {
      files: files.clone(),
      seen: Mutex::default(),
      reading,
      release: held,
    });
    // load_now's first reads of x.bundle and a.txt are held too: let them go
    let _ = release.send(());
    let _ = release.send(());
    let x = server
      .load_now::<Bundle>(&mut world, "x.bundle")
      .expect("loads");
    let mut bundles = cursor::<Bundle>(&world);
    // x's reload now holds p.bundle, whose first read (on a worker) is held
    files.insert("x.bundle", "a.txt\np.bundle");
    server.reload("x.bundle");
    for _ in 0..3 {
      assert!(read.recv_timeout(PATIENCE).is_ok(), "timed out waiting");
    }
    pump_until(&mut world, |_| waiting(&server) == 1);
    // p.bundle, loaded now, holds x.bundle, whose waiting reload holds p.bundle
    let error = server.load_now::<Bundle>(&mut world, "p.bundle").err();
    assert_eq!(
      cycle_of(error.as_deref()),
      Some((
        "p.bundle".to_owned(),
        "p.bundle → x.bundle → p.bundle".to_owned()
      )),
      "{error:?}"
    );
    let sent = events(&world, &mut bundles);
    let reload_failed = sent.iter().find_map(|e| match e {
      AssetEvent::ReloadFailed { id, error } if *id == x.id() => cycle_of(Some(error)),
      _ => None,
    });
    assert_eq!(
      reload_failed,
      Some((
        "x.bundle".to_owned(),
        "x.bundle → p.bundle → x.bundle".to_owned()
      )),
      "{sent:?}"
    );
    assert_eq!(parts(&world, &x), Some(1), "x keeps its last value");
    assert_eq!(waiting(&server), 0);
    let _ = release.send(());
  }

  #[test]
  fn a_reload_overtaken_while_it_waits_is_dropped() {
    let files = MemorySource::new();
    files.insert("x.bundle", "a.txt");
    files.insert("a.txt", "a");
    files.insert("b.txt", "b");
    files.insert("slow.txt", "s");
    let (release, held) = crossbeam_channel::unbounded();
    let (mut world, server) = world_with(HeldSource {
      files: files.clone(),
      release: held,
    });
    let x = server
      .load_now::<Bundle>(&mut world, "x.bundle")
      .expect("loads");
    // Held by the test too, so it outlives the stale value
    let slow: Handle<Text> = server.load("slow.txt");
    files.insert("x.bundle", "a.txt\nslow.txt");
    server.reload("x.bundle");
    pump_until(&mut world, |_| waiting(&server) == 1);
    // A newer reload: the value waiting on slow.txt is stale from now on
    files.insert("x.bundle", "a.txt\nb.txt");
    server.reload("x.bundle");
    let b: Handle<Text> = server.load("b.txt");
    let holds_b = |w: &World| {
      w.resource::<Assets<Bundle>>().get(&x).is_some_and(
        |bundle| matches!(bundle.parts.as_slice(), [_, Part::Text(t)] if t.id() == b.id()),
      )
    };
    pump_until(&mut world, |w| holds_b(w));
    let _ = release.send(());
    pump_until(&mut world, |_| server.state(&slow).is_loaded());
    pump(&mut world);
    assert!(holds_b(&world), "the newer reload's value stays");
  }

  #[test]
  fn a_first_load_overtaken_by_load_now_while_it_waits_is_dropped() {
    let files = MemorySource::new();
    files.insert("x.bundle", "slow.txt");
    files.insert("slow.txt", "s");
    let (reading, read) = crossbeam_channel::unbounded();
    let (release, held) = crossbeam_channel::unbounded();
    let (mut world, server) = world_with(FirstReadHeld {
      files: files.clone(),
      seen: Mutex::default(),
      reading,
      release: held,
    });
    // The worker's first read of x.bundle goes; its first read of slow.txt is held
    let _ = release.send(());
    let queued: Handle<Bundle> = server.load("x.bundle");
    for _ in 0..2 {
      assert!(read.recv_timeout(PATIENCE).is_ok(), "timed out waiting");
    }
    pump_until(&mut world, |_| waiting(&server) == 1);
    // load_now's reads are second reads, so not held; slow.txt loads now too
    files.insert("x.bundle", "slow.txt\nslow.txt");
    let now = server
      .load_now::<Bundle>(&mut world, "x.bundle")
      .expect("loads");
    assert_eq!(now, queued, "one asset");
    assert_eq!(parts(&world, &now), Some(2));
    // slow.txt is loaded now, so the stale value would settle if it were kept
    pump(&mut world);
    assert_eq!(parts(&world, &now), Some(2), "load_now's value stays");
    assert_eq!(waiting(&server), 0);
    let _ = release.send(());
  }

  #[test]
  fn freeing_a_parent_frees_its_dependencies_in_the_same_pump() {
    let source = MemorySource::new();
    source.insert("x.bundle", "a.txt\nb.txt");
    source.insert("a.txt", "a");
    source.insert("b.txt", "b");
    let (mut world, server) = world_with(source);
    let bundle = server
      .load_now::<Bundle>(&mut world, "x.bundle")
      .expect("loads");
    let mut texts = cursor::<Text>(&world);
    drop(bundle);
    // Text's store is freed before Bundle's in a round, so only a second
    // round frees the texts the bundle held
    pump(&mut world);
    let removed = events(&world, &mut texts)
      .iter()
      .filter(|e| matches!(e, AssetEvent::Removed { .. }))
      .count();
    assert_eq!(removed, 2);
    assert!(world.resource::<Assets<Text>>().is_empty());
  }

  #[test]
  fn a_diamond_checks_its_top_once() {
    let source = MemorySource::new();
    source.insert("top.bundle", "left.bundle\nright.bundle");
    source.insert("left.bundle", "a.txt");
    source.insert("right.bundle", "a.txt");
    source.insert("a.txt", "a");
    let (mut world, server) = world_with(source.clone());
    let top = server
      .load_now::<Bundle>(&mut world, "top.bundle")
      .expect("loads");
    let mut bundles = cursor::<Bundle>(&world);
    source.insert("a.txt", "changed");
    let sent = pump_until_sent(
      &mut world,
      &mut bundles,
      |e| matches!(e, AssetEvent::Modified { id } if *id == top.id()),
    );
    pump(&mut world);
    let modified = sent
      .iter()
      .chain(&events(&world, &mut bundles))
      .filter(|e| matches!(e, AssetEvent::Modified { .. }))
      .count();
    assert_eq!(modified, 3, "left, right and top, each once: {sent:?}");
  }

  #[test]
  fn a_dependents_error_repeated_after_a_fix_warns_again() {
    use crate::core::ui::test_util::warnings_containing;
    let warned = || warnings_containing("warn-dependent.bundle");
    let source = MemorySource::new();
    source.insert("warn-dependent.bundle", "warn-dependency.txt");
    source.insert("warn-dependency.txt", "a");
    let (mut world, server) = world_with(source.clone());
    let _bundle = server
      .load_now::<Bundle>(&mut world, "warn-dependent.bundle")
      .expect("loads");
    let mut bundles = cursor::<Bundle>(&world);
    let reload_failed = |e: &AssetEvent<Bundle>| matches!(e, AssetEvent::ReloadFailed { .. });
    source.insert("warn-dependency.txt", "bad");
    pump_until_sent(&mut world, &mut bundles, reload_failed);
    assert_eq!(warned(), 1);
    source.insert("warn-dependency.txt", "bad");
    pump_until_sent(&mut world, &mut bundles, reload_failed);
    assert_eq!(warned(), 1, "the same error warns once");
    source.insert("warn-dependency.txt", "good");
    pump_until_sent(&mut world, &mut bundles, |e| {
      matches!(e, AssetEvent::Modified { .. })
    });
    source.insert("warn-dependency.txt", "bad");
    pump_until_sent(&mut world, &mut bundles, reload_failed);
    assert_eq!(warned(), 2, "broken again after it passed");
  }

  #[test]
  fn dependency_state_walks_the_recorded_dependencies() {
    let source = MemorySource::new();
    source.insert("top.bundle", "mid.bundle");
    source.insert("mid.bundle", "a.txt");
    source.insert("a.txt", "a");
    let (mut world, server) = world_with(source);
    let top = server
      .load_now::<Bundle>(&mut world, "top.bundle")
      .expect("loads");
    let a = server.load::<Text>("a.txt").id().untyped();
    assert!(server.is_ready(&top));
    // States the server never reaches, set by hand: the walk is a safety net
    let set_state = |state: LoadState| {
      if let Some(info) = server.inner.table().infos.get_mut(&a) {
        info.state = state;
      }
    };
    set_state(LoadState::Loading);
    assert!(server.state(&top).is_loaded());
    assert!(matches!(server.dependency_state(&top), LoadState::Loading));
    assert!(!server.is_ready(&top));
    let missing = AssetError::NotFound {
      path: AssetPath::parse("a.txt").expect("a path"),
    };
    set_state(LoadState::Failed(Arc::new(missing)));
    let state = server.dependency_state(&top);
    assert!(
      matches!(
        state.error().map(|e| &**e),
        Some(AssetError::Dependency { path, dependency, error })
          if path.as_str() == "top.bundle"
            && dependency.as_str() == "a.txt"
            && matches!(**error, AssetError::NotFound { .. })
      ),
      "{state:?}"
    );
  }
}
