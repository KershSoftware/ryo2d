//! Saves and settings in the browser: IndexedDB, mirrored in memory. Reads
//! come from the mirror; a write changes the mirror at once and reaches the
//! browser in one commit, whose result arrives later. The logic is here,
//! compiled everywhere and tested against a fake browser; the browser's own
//! backend is `idb.rs`.

use std::collections::{BTreeMap, VecDeque};
use std::future::Future;
use std::io;
use std::pin::pin;
use std::sync::{Arc, Mutex};
use std::task::Poll;

use super::slot::backup_name;
use super::storage::{Completion, Origin, Recovery, Storage, Verify};
use super::writer::lock;
use super::{SaveError, SaveService};

/// One change in a browser commit
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum IdbOp {
  /// Keeps the bytes under the name, replacing any
  Put(String, Vec<u8>),
  /// Removes the name; a missing name is not an error
  Delete(String),
}

/// Why a browser commit failed
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum IdbFailure {
  /// The site's storage quota is used up
  Quota,
  /// Anything else, as the browser described it
  Other(String),
}

/// Where the browser store's commits go: IndexedDB on the web, a fake in
/// tests
pub(crate) trait IdbBackend: Send + Sync {
  /// Applies `ops` in order in one transaction, all or none, then calls
  /// `done` with its result. Commits apply in call order. `done` may run
  /// before `commit` returns
  fn commit(&self, ops: Vec<IdbOp>, done: Box<dyn FnOnce(Result<(), IdbFailure>) + Send>);
}

type Bytes = Arc<[u8]>;

/// The browser's store as the saves see it: every file in memory, read at
/// start-up. A write changes the memory at once and commits to the browser
/// in one transaction; its `Completion` is pending until the browser says
/// how it went, and a commit that fails takes its changes back out. The web
/// has no temporary files: a commit is whole or not at all.
pub(crate) struct BrowserStorage<B: IdbBackend> {
  backend: B,
  mirror: Arc<Mutex<Mirror>>,
  /// Held while a commit enters the mirror and goes to the browser, so both
  /// take commits in one order
  order: Mutex<()>,
}

/// The files as reads see them, and as the browser holds them
#[derive(Default)]
struct Mirror {
  /// What reads see: `committed`, with the running commits' changes over
  /// it in call order
  files: BTreeMap<String, Bytes>,
  /// What the browser holds, as far as the finished commits say
  committed: BTreeMap<String, Bytes>,
  /// The commits not yet in `committed`, in call order
  running: VecDeque<Running>,
  next_id: u64,
}

struct Running {
  id: u64,
  /// Each name and its new bytes (`None`: deleted), in order
  changes: Vec<(String, Option<Bytes>)>,
  /// The browser committed it; it joins `committed` once every earlier
  /// commit has finished
  landed: bool,
}

impl Mirror {
  /// Applies a commit's changes to what reads see; returns its id
  fn begin(&mut self, ops: &[IdbOp]) -> u64 {
    let id = self.next_id;
    self.next_id += 1;
    let changes: Vec<_> = ops
      .iter()
      .map(|op| match op {
        IdbOp::Put(name, bytes) => (name.clone(), Some(Bytes::from(bytes.as_slice()))),
        IdbOp::Delete(name) => (name.clone(), None),
      })
      .collect();
    for (name, bytes) in &changes {
      set(&mut self.files, name, bytes.clone());
    }
    self.running.push_back(Running {
      id,
      changes,
      landed: false,
    });
    id
  }

  /// The browser finished commit `id`. One that failed takes its names back
  /// to what the browser holds, with the commits after it still over them
  /// (the browser applies those all the same)
  fn finish(&mut self, id: u64, landed: bool) {
    let Some(index) = self.running.iter().position(|running| running.id == id) else {
      return;
    };
    if landed {
      self.running[index].landed = true;
    } else if let Some(failed) = self.running.remove(index) {
      for (name, _) in &failed.changes {
        let mut bytes = self.committed.get(name).cloned();
        for (changed, new) in self.running.iter().flat_map(|running| &running.changes) {
          if changed == name {
            bytes.clone_from(new);
          }
        }
        set(&mut self.files, name, bytes);
      }
    }
    while self.running.front().is_some_and(|running| running.landed) {
      if let Some(landed) = self.running.pop_front() {
        for (name, bytes) in landed.changes {
          set(&mut self.committed, &name, bytes);
        }
      }
    }
  }
}

fn set(files: &mut BTreeMap<String, Bytes>, name: &str, bytes: Option<Bytes>) {
  match bytes {
    Some(bytes) => {
      files.insert(name.to_string(), bytes);
    }
    None => {
      files.remove(name);
    }
  }
}

impl<B: IdbBackend> BrowserStorage<B> {
  /// A store holding `files`, every entry read from the browser, committing
  /// to `backend`
  pub(crate) fn new(backend: B, files: impl IntoIterator<Item = (String, Vec<u8>)>) -> Self {
    let files: BTreeMap<String, Bytes> = files
      .into_iter()
      .map(|(name, bytes)| (name, Bytes::from(bytes)))
      .collect();
    BrowserStorage {
      backend,
      mirror: Arc::new(Mutex::new(Mirror {
        committed: files.clone(),
        files,
        ..Mirror::default()
      })),
      order: Mutex::new(()),
    }
  }

  /// Commits the changes `ops` gives for the files as reads see them
  fn commit(
    &self,
    ops: impl FnOnce(&BTreeMap<String, Bytes>) -> io::Result<Vec<IdbOp>>,
  ) -> Completion {
    let _order = lock(&self.order);
    let (id, ops) = {
      let mut mirror = lock(&self.mirror);
      let ops = match ops(&mirror.files) {
        Ok(ops) => ops,
        Err(error) => return Completion::Done(Err(error)),
      };
      (mirror.begin(&ops), ops)
    };
    let (send, receive) = crossbeam_channel::bounded(1);
    let mirror = Arc::clone(&self.mirror);
    // Not under the mirror's lock: `done` may run before `commit` returns
    self.backend.commit(
      ops,
      Box::new(move |result| {
        lock(&mirror).finish(id, result.is_ok());
        let _ = send.send(result.map_err(io_error));
      }),
    );
    Completion::Pending(receive)
  }
}

impl<B: IdbBackend + 'static> Storage for BrowserStorage<B> {
  fn read(&self, name: &str) -> io::Result<Option<Vec<u8>>> {
    Ok(
      lock(&self.mirror)
        .files
        .get(name)
        .map(|bytes| bytes.to_vec()),
    )
  }

  fn names(&self) -> io::Result<Vec<String>> {
    Ok(lock(&self.mirror).files.keys().cloned().collect())
  }

  /// One commit: the new bytes, and the current copy as the backup when it
  /// verifies (a damaged copy never replaces a good backup)
  fn replace(&self, name: &str, bytes: &[u8], verify: Verify<'_>) -> Completion {
    self.commit(|files| {
      let mut ops = Vec::with_capacity(2);
      if let Some(old) = files.get(name).filter(|old| verify(old).is_some()) {
        ops.push(IdbOp::Put(backup_name(name), old.to_vec()));
      }
      ops.push(IdbOp::Put(name.to_string(), bytes.to_vec()));
      Ok(ops)
    })
  }

  fn recover(&self, name: &str, verify: Verify<'_>) -> io::Result<Recovery> {
    let mirror = lock(&self.mirror);
    let current = mirror.files.get(name);
    let backup = mirror.files.get(&backup_name(name));
    for (copy, from) in [(current, Origin::Current), (backup, Origin::Backup)] {
      if let Some(bytes) = copy.filter(|bytes| verify(bytes).is_some()) {
        return Ok(Recovery::Found {
          bytes: bytes.to_vec(),
          from,
        });
      }
    }
    Ok(if current.is_some() || backup.is_some() {
      Recovery::Damaged
    } else {
      Recovery::Missing
    })
  }

  fn remove_all(&self, name: &str) -> Completion {
    self.commit(|_| {
      Ok(vec![
        IdbOp::Delete(backup_name(name)),
        IdbOp::Delete(name.to_string()),
      ])
    })
  }

  /// One commit: the file under `to`, and gone from `name`
  fn set_aside(&self, name: &str, to: &str) -> Completion {
    self.commit(|files| {
      let bytes = files
        .get(name)
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, format!("no file \"{name}\"")))?;
      Ok(vec![
        IdbOp::Put(to.to_string(), bytes.to_vec()),
        IdbOp::Delete(name.to_string()),
      ])
    })
  }
}

/// A failed commit as the saves report it: a full quota is `StorageFull`
fn io_error(failure: IdbFailure) -> io::Error {
  match failure {
    IdbFailure::Quota => io::Error::new(
      io::ErrorKind::StorageFull,
      "the browser's storage for this site is full",
    ),
    IdbFailure::Other(reason) => io::Error::other(reason),
  }
}

/// Puts the browser's store under the saves; a store that did not open warns
/// once that saves are unavailable and settings last only for this session.
/// True when it was installed
pub(crate) fn install(saves: &SaveService, store: Result<Arc<dyn Storage>, SaveError>) -> bool {
  match store {
    Ok(storage) => {
      saves.set_storage(storage);
      true
    }
    Err(error) => {
      let reason = match std::error::Error::source(&error) {
        Some(cause) => format!("{error}: {cause}"),
        None => error.to_string(),
      };
      log::warn!(
        "the browser's storage did not open ({reason}): saves are unavailable, and settings last only for this session"
      );
      false
    }
  }
}

/// The browser store's start-up: `open` (the database) has until `timeout`
/// to finish, else the store is `Unavailable`; once it has opened, `read`
/// (every file in it) is waited for however long it takes, and the first
/// scene waits with it
pub(crate) async fn open_then_read<D, F, R>(
  open: impl Future<Output = Result<D, SaveError>>,
  timeout: impl Future<Output = ()>,
  read: impl FnOnce(D) -> R,
) -> Result<F, SaveError>
where
  R: Future<Output = Result<F, SaveError>>,
{
  let database = in_time(open, timeout).await?;
  read(database).await
}

/// `work`'s result, or `Unavailable` when `timeout` finishes first
async fn in_time<T>(
  work: impl Future<Output = Result<T, SaveError>>,
  timeout: impl Future<Output = ()>,
) -> Result<T, SaveError> {
  let mut work = pin!(work);
  let mut timeout = pin!(timeout);
  std::future::poll_fn(|context| {
    if let Poll::Ready(result) = work.as_mut().poll(context) {
      return Poll::Ready(result);
    }
    timeout
      .as_mut()
      .poll(context)
      .map(|()| Err(SaveError::Unavailable))
  })
  .await
}

/// The web's start-up: the first scene waits until the GPU and the browser
/// store have both arrived. Each counts once, so a store arriving after the
/// time-out has spoken is dropped
pub(crate) struct StartupGate<G, S> {
  gpu: Option<G>,
  store: Option<S>,
  /// Both were taken
  opened: bool,
}

impl<G, S> StartupGate<G, S> {
  pub(crate) fn new() -> Self {
    StartupGate {
      gpu: None,
      store: None,
      opened: false,
    }
  }

  /// The GPU set-up finished
  pub(crate) fn gpu_ready(&mut self, gpu: G) {
    if !self.opened && self.gpu.is_none() {
      self.gpu = Some(gpu);
    }
  }

  /// The browser store opened, or will not
  pub(crate) fn store_ready(&mut self, store: S) {
    if !self.opened && self.store.is_none() {
      self.store = Some(store);
    }
  }

  /// Whether both have arrived and are not taken yet
  pub(crate) fn ready(&self) -> bool {
    self.gpu.is_some() && self.store.is_some()
  }

  /// Both, once both have arrived; then the gate stays open
  pub(crate) fn take(&mut self) -> Option<(G, S)> {
    if !self.ready() {
      return None;
    }
    self.opened = true;
    self.gpu.take().zip(self.store.take())
  }
}

/// A browser for tests: its commits wait until the test releases them, in
/// call order
#[cfg(test)]
pub(crate) mod fake {
  use std::collections::{BTreeMap, VecDeque};
  use std::sync::{Arc, Mutex};
  use std::time::{Duration, Instant};

  use super::{BrowserStorage, IdbBackend, IdbFailure, IdbOp};

  type Done = Box<dyn FnOnce(Result<(), IdbFailure>) + Send>;

  /// What the fake browser holds, and the commits waiting
  #[derive(Clone, Default)]
  pub(crate) struct FakeIdb {
    state: Arc<Mutex<State>>,
  }

  #[derive(Default)]
  struct State {
    db: BTreeMap<String, Vec<u8>>,
    waiting: VecDeque<(Vec<IdbOp>, Done)>,
  }

  impl FakeIdb {
    /// A browser holding `files`, and the store opened over them
    pub(crate) fn opened(files: &[(&str, &[u8])]) -> (FakeIdb, BrowserStorage<FakeIdb>) {
      let files: BTreeMap<_, _> = files
        .iter()
        .map(|(name, bytes)| (name.to_string(), bytes.to_vec()))
        .collect();
      let browser = FakeIdb::default();
      browser.state.lock().unwrap().db = files.clone();
      (browser.clone(), BrowserStorage::new(browser, files))
    }

    /// The commits waiting, oldest first
    pub(crate) fn waiting(&self) -> Vec<Vec<IdbOp>> {
      let state = self.state.lock().unwrap();
      state.waiting.iter().map(|(ops, _)| ops.clone()).collect()
    }

    /// Waits until `count` commits are waiting (the saves' writer thread
    /// sends them)
    pub(crate) fn wait_for(&self, count: usize) {
      let start = Instant::now();
      while self.state.lock().unwrap().waiting.len() < count {
        assert!(
          start.elapsed() < Duration::from_secs(5),
          "{count} commits never came"
        );
        std::thread::sleep(Duration::from_millis(1));
      }
    }

    /// The oldest waiting commit finishes with `result`
    pub(crate) fn release(&self, result: Result<(), IdbFailure>) {
      let done = {
        let mut state = self.state.lock().unwrap();
        let (ops, done) = state.waiting.pop_front().expect("a commit is waiting");
        if result.is_ok() {
          for op in ops {
            match op {
              IdbOp::Put(name, bytes) => state.db.insert(name, bytes),
              IdbOp::Delete(name) => state.db.remove(&name),
            };
          }
        }
        done
      };
      done(result);
    }

    /// What the browser holds
    pub(crate) fn db(&self) -> BTreeMap<String, Vec<u8>> {
      self.state.lock().unwrap().db.clone()
    }
  }

  impl IdbBackend for FakeIdb {
    fn commit(&self, ops: Vec<IdbOp>, done: Done) {
      self.state.lock().unwrap().waiting.push_back((ops, done));
    }
  }
}

#[cfg(test)]
mod tests {
  use std::time::{Duration, Instant};

  use serde::{Deserialize, Serialize};

  use super::fake::FakeIdb;
  use super::*;
  use crate::core::audio::VolumeKind;
  use crate::core::config::UserPreferenceFilter;
  use crate::core::saves::SaveInfo;
  use crate::core::saves::storage::Origin;
  use crate::core::saves::versions::VersionRegistry;
  use crate::core::settings::file::SettingsFile;
  use crate::core::settings::persist::{DEBOUNCE, SettingsWriter};
  use crate::core::settings::sections::SectionRegistry;
  use crate::core::settings::{EngineValues, Settings};

  #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
  struct Hero {
    level: u32,
  }

  /// A copy is whole unless it starts with "bad"
  fn whole(bytes: &[u8]) -> Option<u64> {
    (!bytes.starts_with(b"bad")).then_some(1)
  }

  fn read(store: &dyn Storage, name: &str) -> Option<Vec<u8>> {
    store.read(name).unwrap()
  }

  /// Every file as reads see it
  fn mirror(store: &dyn Storage) -> BTreeMap<String, Vec<u8>> {
    let names = store.names().unwrap();
    names
      .into_iter()
      .map(|name| {
        let bytes = read(store, &name).unwrap();
        (name, bytes)
      })
      .collect()
  }

  fn put(name: &str, bytes: &[u8]) -> IdbOp {
    IdbOp::Put(name.to_string(), bytes.to_vec())
  }

  fn delete(name: &str) -> IdbOp {
    IdbOp::Delete(name.to_string())
  }

  fn still_pending(completion: Completion) -> Completion {
    match completion.poll() {
      Err(pending) => pending,
      Ok(result) => panic!("finished before the browser committed: {result:?}"),
    }
  }

  fn saves_over(store: BrowserStorage<FakeIdb>) -> SaveService {
    let saves = SaveService::new(VersionRegistry::default());
    saves.set_storage(Arc::new(store));
    saves
  }

  /// The slots' results, once the writer has sent every job's (their
  /// commits released, so the flush waits for nothing else): a job's result
  /// goes out only after its commit is made, and `drain` never waits
  fn settled_results(saves: &SaveService) -> Vec<Result<(), Arc<SaveError>>> {
    let unfinished = saves.flush(Duration::from_secs(5), Duration::from_secs(5));
    assert!(unfinished.is_empty(), "still writing {unfinished:?}");
    saves
      .drain()
      .into_iter()
      .filter_map(|finished| finished.message())
      .map(|message| message.result)
      .collect()
  }

  #[test]
  fn writes_are_readable_at_once_and_finish_when_the_browser_commits() {
    let (browser, store) = FakeIdb::opened(&[]);
    let done = store.replace("a", b"one", &whole);
    assert_eq!(read(&store, "a").as_deref(), Some(&b"one"[..]));
    assert!(browser.db().is_empty(), "not committed yet");
    let done = still_pending(done);
    browser.release(Ok(()));
    assert!(matches!(done.poll(), Ok(Ok(()))));
    assert_eq!(browser.db(), mirror(&store));

    // Through the saves: a load reads the save at once, and SaveFinished
    // waits for the commit
    let (browser, store) = FakeIdb::opened(&[]);
    let saves = saves_over(store);
    saves
      .save("browser-hero", Hero { level: 3 }, SaveInfo::new())
      .unwrap();
    browser.wait_for(1);
    assert!(saves.drain().is_empty());
    assert!(saves.is_writing("browser-hero"));
    assert_eq!(
      saves.load::<Hero>("browser-hero").unwrap().data,
      Hero { level: 3 }
    );
    browser.release(Ok(()));
    let results = settled_results(&saves);
    assert!(matches!(results[..], [Ok(())]), "{results:?}");
    assert!(!saves.is_writing("browser-hero"));
    assert_eq!(
      saves.load::<Hero>("browser-hero").unwrap().data,
      Hero { level: 3 },
      "from the mirror"
    );
  }

  #[test]
  fn commits_apply_in_call_order() {
    let (browser, store) = FakeIdb::opened(&[("b", b"old")]);
    let first = store.replace("a", b"one", &whole);
    let second = store.replace("a", b"two", &whole);
    let removed = store.remove_all("b");
    assert_eq!(
      browser.waiting(),
      [
        vec![put("a", b"one")],
        vec![put("a.bak", b"one"), put("a", b"two")],
        vec![delete("b.bak"), delete("b")],
      ]
    );
    assert_eq!(read(&store, "a").as_deref(), Some(&b"two"[..]));
    assert_eq!(read(&store, "b"), None);
    for _ in 0..3 {
      browser.release(Ok(()));
    }
    for completion in [first, second, removed] {
      assert!(matches!(completion.poll(), Ok(Ok(()))));
    }
    assert_eq!(browser.db(), mirror(&store));

    // An earlier commit failing leaves a later one standing: the browser
    // still applies it
    let (browser, store) = FakeIdb::opened(&[("a", b"zero")]);
    let first = store.replace("a", b"one", &whole);
    let second = store.replace("a", b"two", &whole);
    browser.release(Err(IdbFailure::Other("aborted".into())));
    assert!(matches!(first.poll(), Ok(Err(_))));
    assert_eq!(read(&store, "a").as_deref(), Some(&b"two"[..]));
    assert_eq!(read(&store, "a.bak").as_deref(), Some(&b"one"[..]));
    browser.release(Ok(()));
    assert!(matches!(second.poll(), Ok(Ok(()))));
    assert_eq!(browser.db(), mirror(&store));
  }

  #[test]
  fn a_quota_failure_rolls_back_and_reports_storage_full() {
    let (browser, store) = FakeIdb::opened(&[("a", b"one")]);
    let done = store.replace("a", b"two", &whole);
    assert_eq!(read(&store, "a.bak").as_deref(), Some(&b"one"[..]));
    browser.release(Err(IdbFailure::Quota));
    match done.poll() {
      Ok(Err(error)) => assert_eq!(error.kind(), io::ErrorKind::StorageFull),
      other => panic!("not StorageFull: {other:?}"),
    }
    assert_eq!(mirror(&store), browser.db());
    assert_eq!(store.names().unwrap(), ["a"]);

    // A set-aside that fails is rolled back too
    let aside = store.set_aside("a", "a.old");
    assert_eq!(read(&store, "a"), None);
    browser.release(Err(IdbFailure::Other("aborted".into())));
    assert!(matches!(aside.poll(), Ok(Err(_))));
    assert_eq!(mirror(&store), browser.db());

    // Through the saves: SaveFinished carries StorageFull, and the save
    // before it still loads
    let (browser, store) = FakeIdb::opened(&[]);
    let saves = saves_over(store);
    saves
      .save("browser-full", Hero { level: 1 }, SaveInfo::new())
      .unwrap();
    browser.wait_for(1);
    browser.release(Ok(()));
    assert!(matches!(settled_results(&saves)[..], [Ok(())]));
    saves
      .save("browser-full", Hero { level: 2 }, SaveInfo::new())
      .unwrap();
    browser.wait_for(1);
    browser.release(Err(IdbFailure::Quota));
    let results = settled_results(&saves);
    assert!(
      matches!(results[..], [Err(ref error)] if matches!(**error, SaveError::StorageFull)),
      "{results:?}"
    );
    assert_eq!(
      saves.load::<Hero>("browser-full").unwrap().data,
      Hero { level: 1 }
    );
  }

  #[test]
  fn a_replace_keeps_the_previous_copy_as_backup() {
    let (browser, store) = FakeIdb::opened(&[]);
    let _ = store.replace("a", b"one", &whole);
    browser.release(Ok(()));
    let _ = store.replace("a", b"two", &whole);
    browser.release(Ok(()));
    assert_eq!(
      browser.db(),
      BTreeMap::from([
        ("a".to_string(), b"two".to_vec()),
        ("a.bak".to_string(), b"one".to_vec()),
      ])
    );
    assert_eq!(
      store.recover("a", &whole).unwrap(),
      Recovery::Found {
        bytes: b"two".to_vec(),
        from: Origin::Current
      }
    );

    // A damaged copy never replaces a good backup, and recover falls back to
    // the backup
    let (browser, store) = FakeIdb::opened(&[("a", b"bad"), ("a.bak", b"one")]);
    assert_eq!(
      store.recover("a", &whole).unwrap(),
      Recovery::Found {
        bytes: b"one".to_vec(),
        from: Origin::Backup
      }
    );
    let _ = store.replace("a", b"three", &whole);
    assert_eq!(browser.waiting(), [vec![put("a", b"three")]]);
    browser.release(Ok(()));
    assert_eq!(read(&store, "a.bak").as_deref(), Some(&b"one"[..]));

    let (_browser, store) = FakeIdb::opened(&[("a", b"bad")]);
    assert_eq!(store.recover("a", &whole).unwrap(), Recovery::Damaged);
    assert_eq!(store.recover("b", &whole).unwrap(), Recovery::Missing);
  }

  #[test]
  fn the_first_scene_waits_for_both_the_gpu_and_the_store() {
    let mut gate = StartupGate::new();
    assert!(!gate.ready());
    gate.gpu_ready("gpu");
    assert!(!gate.ready(), "the GPU alone does not open it");
    assert!(gate.take().is_none());
    gate.store_ready(Ok::<u32, SaveError>(1));
    assert!(gate.ready());
    assert!(matches!(gate.take(), Some(("gpu", Ok(1)))));
    assert!(gate.take().is_none(), "it opens once");

    // The store first; only its first arrival counts
    let mut gate = StartupGate::new();
    gate.store_ready(Err(SaveError::Unavailable));
    gate.store_ready(Ok(2));
    assert!(!gate.ready(), "the store alone does not open it");
    gate.gpu_ready("gpu");
    assert!(matches!(
      gate.take(),
      Some(("gpu", Err(SaveError::Unavailable)))
    ));
    gate.store_ready(Ok(3));
    assert!(!gate.ready());
  }

  /// Ready with `value` at its `polls`th poll, waking itself until then
  fn after<T: Unpin>(polls: usize, value: T) -> impl Future<Output = T> {
    let (mut left, mut value) = (polls, Some(value));
    std::future::poll_fn(move |context| {
      left = left.saturating_sub(1);
      if left == 0 {
        Poll::Ready(value.take().expect("polled once ready"))
      } else {
        context.waker().wake_by_ref();
        Poll::Pending
      }
    })
  }

  #[test]
  fn a_store_that_opens_in_time_is_read_however_long_the_read_takes() {
    let store = pollster::block_on(open_then_read(
      after(1, Ok("db")),
      after(3, ()),
      |database| after(50, Ok(vec![database])),
    ));
    assert_eq!(store.unwrap(), ["db"]);
  }

  #[test]
  fn a_store_that_does_not_open_in_time_is_unavailable_and_never_read() {
    let read = std::cell::Cell::new(false);
    let store = pollster::block_on(open_then_read(
      std::future::pending::<Result<&str, SaveError>>(),
      after(2, ()),
      |database| {
        read.set(true);
        std::future::ready(Ok(database))
      },
    ));
    assert!(matches!(store, Err(SaveError::Unavailable)), "{store:?}");
    assert!(!read.get());

    // An open or a read that fails says why
    let failed = |path: &str| SaveError::Io {
      path: path.into(),
      source: io::Error::other("refused"),
    };
    let store = pollster::block_on(open_then_read(
      after(1, Err::<&str, _>(failed("open"))),
      std::future::pending(),
      |database| std::future::ready(Ok(database)),
    ));
    assert!(matches!(store, Err(SaveError::Io { path, .. }) if path == "open"));
    let store = pollster::block_on(open_then_read(after(1, Ok("db")), after(2, ()), |_| {
      after(5, Err::<&str, _>(failed("read")))
    }));
    assert!(matches!(store, Err(SaveError::Io { path, .. }) if path == "read"));
  }

  #[test]
  fn a_store_that_does_not_open_leaves_saves_unavailable() {
    let saves = SaveService::new(VersionRegistry::default());
    let refused = SaveError::Io {
      path: "refused-browser-db".into(),
      source: io::Error::other("blocked"),
    };
    assert!(!install(&saves, Err(refused)));
    let warned = crate::core::dev_log::get_entries()
      .iter()
      .filter(|entry| entry.level == crate::core::dev_log::LogLevel::Warn)
      .filter(|entry| entry.message.contains("refused-browser-db"))
      .count();
    assert_eq!(warned, 1);
    assert!(matches!(
      saves.save("browser-none", Hero { level: 1 }, SaveInfo::new()),
      Err(SaveError::Unavailable)
    ));
    assert!(matches!(
      saves.load::<Hero>("browser-none"),
      Err(SaveError::Unavailable)
    ));

    // Settings apply, but their write finds nowhere to go
    let mut settings = Settings::new(
      EngineValues::default(),
      UserPreferenceFilter::all(),
      SectionRegistry::default(),
    );
    let mut writer = SettingsWriter::new(SettingsFile::default(), false, &settings);
    settings.set_volume(VolumeKind::Music, 0.5);
    let now = Instant::now();
    writer.observe(now, &settings);
    assert!(writer.tick(now + DEBOUNCE, &settings, &saves));
    saves.flush(Duration::from_secs(5), Duration::from_secs(10));
    let finished = saves.drain();
    assert!(
      matches!(&finished[..], [one] if matches!(one.result.as_ref().map_err(|error| &**error), Err(SaveError::Unavailable))),
      "{finished:?}"
    );
    assert_eq!(settings.volume(VolumeKind::Music), 0.5);

    // A store that opens is installed
    let (browser, store) = FakeIdb::opened(&[]);
    assert!(install(&saves, Ok(Arc::new(store))));
    saves
      .save("browser-none", Hero { level: 1 }, SaveInfo::new())
      .unwrap();
    browser.wait_for(1);
    browser.release(Ok(()));
  }
}
