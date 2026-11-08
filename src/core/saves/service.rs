//! The save service behind `ctx.saves()` and the `Saves` parameter. The main
//! thread checks a save and queues it; the writer's thread encodes and writes
//! it, in call order; a load sees what is queued.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::io;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, PoisonError, RwLock};
use std::time::Duration;

use serde::Serialize;
use serde::de::DeserializeOwned;
use web_time::{Instant, SystemTime};

use super::SaveError;
use super::container::{self, FORMAT, SaveHeader};
use super::slot::{backup_name, check_new, file_name, is_legacy, is_temp_of};
use super::storage::{Completion, Origin, Recovery, Storage};
use super::versions::VersionRegistry;
use super::view::{Loaded, SaveFinished, SaveInfo, SaveOp, SaveSlot};
use super::writer::{Writer, lock};
use crate::core::ecs::Resource;

const SAVE_EXTENSION: &str = ".ryo-save";

/// The engine's saves: where they are kept, the game's data versions, the
/// writer and what is queued on it. Internally synchronized: every method
/// takes `&self`.
#[derive(Resource)]
pub struct SaveService {
  /// `None`: saves are unavailable
  storage: RwLock<Option<Arc<dyn Storage>>>,
  versions: VersionRegistry,
  shared: Arc<Shared>,
  writer: Writer<JobEnd>,
  results: Mutex<Results>,
  /// The slots already warned about loading from their backup
  backup_warned: Mutex<HashSet<String>>,
  /// A flush gave up waiting, so later ones do not wait again
  gave_up: AtomicBool,
}

/// What the main thread and the writer's jobs share: each slot's latest queued
/// save or delete, by file name
#[derive(Default)]
struct Shared {
  slots: Mutex<HashMap<String, Pending>>,
  next_generation: AtomicU64,
}

/// A slot's latest queued save or delete; its generation orders it among the
/// slot's others
struct Pending {
  generation: u64,
  op: PendingOp,
}

enum PendingOp {
  Save {
    header: SaveHeader,
    encoded: Arc<Encoded>,
  },
  Delete,
}

/// A queued save's bytes, published by its job once encoded, for loads
#[derive(Default)]
struct Encoded {
  state: Mutex<Encoding>,
  ready: Condvar,
}

#[derive(Clone, Default)]
enum Encoding {
  #[default]
  Waiting,
  Bytes(Arc<Vec<u8>>),
  /// It will not be written (its data did not serialize, or a newer engine's
  /// save is in the way): the stored copy stands
  Failed,
  /// A later save or delete of the slot took its place
  Superseded,
}

impl Encoded {
  fn set(&self, to: Encoding) {
    *lock(&self.state) = to;
    self.ready.notify_all();
  }

  fn peek(&self) -> Encoding {
    lock(&self.state).clone()
  }

  /// Waits for the job to publish; on the web the job ran as it was sent
  fn wait(&self) -> Encoding {
    let mut state = lock(&self.state);
    while matches!(*state, Encoding::Waiting) {
      state = self
        .ready
        .wait(state)
        .unwrap_or_else(PoisonError::into_inner);
    }
    state.clone()
  }
}

/// Publishes a job's encoding; a job that ends without publishing (it
/// panicked) leaves it `Failed`, so no load waits for it forever
struct Publish {
  encoded: Arc<Encoded>,
  done: bool,
}

impl Publish {
  fn set(&mut self, to: Encoding) {
    self.encoded.set(to);
    self.done = true;
  }
}

impl Drop for Publish {
  fn drop(&mut self) {
    if !self.done {
      self.encoded.set(Encoding::Failed);
    }
  }
}

/// What a load finds queued for a slot
enum Queued {
  Nothing,
  Deleted,
  Bytes(Arc<Vec<u8>>),
}

/// What a header read finds queued for a slot
enum QueuedHeader {
  Nothing,
  Deleted,
  Header(SaveHeader),
}

impl Shared {
  /// Records `op` as the slot's latest, returning its generation
  fn begin(&self, file: &str, op: PendingOp) -> u64 {
    let mut slots = lock(&self.slots);
    let generation = self.next_generation.fetch_add(1, Ordering::Relaxed);
    slots.insert(file.to_string(), Pending { generation, op });
    generation
  }

  /// True when a later save or delete of the slot is queued
  fn superseded(&self, file: &str, generation: u64) -> bool {
    lock(&self.slots)
      .get(file)
      .is_some_and(|pending| pending.generation > generation)
  }

  /// The slot's queued data, waiting for its job to encode it
  fn queued(&self, file: &str) -> Queued {
    loop {
      let encoded = match lock(&self.slots).get(file) {
        None => return Queued::Nothing,
        Some(Pending {
          op: PendingOp::Delete,
          ..
        }) => return Queued::Deleted,
        Some(Pending {
          op: PendingOp::Save { encoded, .. },
          ..
        }) => Arc::clone(encoded),
      };
      match encoded.wait() {
        Encoding::Bytes(bytes) => return Queued::Bytes(bytes),
        Encoding::Failed => return Queued::Nothing,
        // The slot's newer entry has it now
        Encoding::Superseded | Encoding::Waiting => {}
      }
    }
  }

  /// The slot's queued header, without waiting
  fn queued_header(&self, file: &str) -> QueuedHeader {
    match lock(&self.slots).get(file) {
      None => QueuedHeader::Nothing,
      Some(Pending {
        op: PendingOp::Delete,
        ..
      }) => QueuedHeader::Deleted,
      Some(Pending {
        op: PendingOp::Save { header, encoded },
        ..
      }) => match encoded.peek() {
        Encoding::Failed => QueuedHeader::Nothing,
        _ => QueuedHeader::Header(header.clone()),
      },
    }
  }
}

/// What a job queued on the writer is for
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum JobKind {
  /// A slot's save
  Save { slot: String },
  /// A slot's delete
  Delete { slot: String },
  /// A named file's write (`write_named`)
  Named { name: String },
}

/// A job queued on the writer, as `drain` reports it
struct JobMeta {
  kind: JobKind,
  /// The file it writes
  file: String,
  /// A slot job's generation
  generation: Option<u64>,
}

impl JobMeta {
  fn finish(self, id: u64, result: Result<(), Arc<SaveError>>) -> Finished {
    Finished {
      id,
      kind: self.kind,
      result,
      file: self.file,
      generation: self.generation,
    }
  }
}

/// A job that has finished, as [`SaveService::drain`] gives it
#[derive(Debug)]
pub(crate) struct Finished {
  /// The id its queueing returned
  #[cfg_attr(not(test), expect(dead_code, reason = "the settings writer's"))]
  pub(crate) id: u64,
  pub(crate) kind: JobKind,
  pub(crate) result: Result<(), Arc<SaveError>>,
  file: String,
  generation: Option<u64>,
}

impl Finished {
  /// The `SaveFinished` of a slot's save or delete; `None` for a named file
  pub(crate) fn message(&self) -> Option<SaveFinished> {
    let (slot, op) = match &self.kind {
      JobKind::Save { slot } => (slot, SaveOp::Save),
      JobKind::Delete { slot } => (slot, SaveOp::Delete),
      JobKind::Named { .. } => return None,
    };
    Some(SaveFinished {
      slot: slot.clone(),
      op,
      result: self.result.clone(),
    })
  }
}

/// How a job on the writer ended
pub(crate) enum JobEnd {
  Done(Result<(), SaveError>),
  /// The storage finishes it later (the browser's commit)
  Committing(Completion),
}

impl JobEnd {
  fn from_completion(completion: Completion, file: &str) -> JobEnd {
    match completion.poll() {
      Ok(result) => JobEnd::Done(result.map_err(|error| io_error(file, error))),
      Err(pending) => JobEnd::Committing(pending),
    }
  }
}

/// The jobs queued and their results, until `drain` takes them
#[derive(Default)]
struct Results {
  jobs: HashMap<u64, JobMeta>,
  /// Jobs whose storage finishes them later
  committing: Vec<(u64, JobMeta, Completion)>,
  ready: Vec<Finished>,
}

impl SaveService {
  /// A service with no storage (saves are `Unavailable` until
  /// [`set_storage`](Self::set_storage)), reading the game's data versions
  /// from `versions`
  pub(crate) fn new(versions: VersionRegistry) -> SaveService {
    SaveService {
      storage: RwLock::new(None),
      versions,
      shared: Arc::default(),
      writer: Writer::named("ryo2d-saves"),
      results: Mutex::default(),
      backup_warned: Mutex::default(),
      gave_up: AtomicBool::new(false),
    }
  }

  /// Keeps saves in `storage` from now on
  pub(crate) fn set_storage(&self, storage: Arc<dyn Storage>) {
    *self.storage.write().unwrap_or_else(PoisonError::into_inner) = Some(storage);
  }

  fn storage(&self) -> Option<Arc<dyn Storage>> {
    self
      .storage
      .read()
      .unwrap_or_else(PoisonError::into_inner)
      .clone()
  }

  /// Queues `data` to be saved in `slot`
  pub(crate) fn save<T: Serialize + Send + 'static>(
    &self,
    slot: &str,
    data: T,
    info: SaveInfo,
  ) -> Result<(), SaveError> {
    check_new(slot)?;
    let storage = self.storage().ok_or(SaveError::Unavailable)?;
    let header = SaveHeader {
      format: FORMAT,
      version: self.versions.current::<T>(),
      saved_at: unix_now(),
      play_time: info.play_time,
      name: info.name,
      labels: info.labels,
      engine_version: env!("CARGO_PKG_VERSION").to_string(),
    };
    let file = file_name(slot);
    let encoded = Arc::new(Encoded::default());
    let generation = self.shared.begin(
      &file,
      PendingOp::Save {
        header: header.clone(),
        encoded: Arc::clone(&encoded),
      },
    );
    let shared = Arc::clone(&self.shared);
    let meta = JobMeta {
      kind: JobKind::Save {
        slot: slot.to_string(),
      },
      file: file.clone(),
      generation: Some(generation),
    };
    self.queue(meta, move || {
      let mut publish = Publish {
        encoded,
        done: false,
      };
      if shared.superseded(&file, generation) {
        publish.set(Encoding::Superseded);
        return JobEnd::Done(Ok(()));
      }
      // A newer engine's save is refused, never overwritten
      if let Some(found) = newer_format(storage.as_ref(), &file) {
        publish.set(Encoding::Failed);
        return JobEnd::Done(Err(newer(found)));
      }
      let bytes = match encode(&header, &data) {
        Ok(bytes) => Arc::new(bytes),
        Err(error) => {
          publish.set(Encoding::Failed);
          return JobEnd::Done(Err(error));
        }
      };
      drop(data);
      publish.set(Encoding::Bytes(Arc::clone(&bytes)));
      JobEnd::from_completion(storage.replace(&file, &bytes, &container::verify), &file)
    });
    Ok(())
  }

  /// The save in `slot`: its queued data if a save of it is queued, else the
  /// newest whole copy kept
  pub(crate) fn load<T: DeserializeOwned + 'static>(
    &self,
    slot: &str,
  ) -> Result<Loaded<T>, SaveError> {
    let storage = self.storage();
    check_reachable(slot, storage.as_deref())?;
    let file = file_name(slot);
    match self.shared.queued(&file) {
      Queued::Deleted => return Err(SaveError::NotFound(slot.to_string())),
      Queued::Bytes(bytes) => return self.decode(slot, &bytes, false),
      Queued::Nothing => {}
    }
    let storage = storage.ok_or(SaveError::Unavailable)?;
    refuse_newer(storage.as_ref(), &file)?;
    match storage
      .recover(&file, &container::verify)
      .map_err(|error| io_error(&file, error))?
    {
      Recovery::Missing => Err(SaveError::NotFound(slot.to_string())),
      Recovery::Damaged => Err(damaged(slot)),
      Recovery::Found { bytes, from } => {
        let from_backup = from == Origin::Backup;
        if from_backup {
          self.warn_backup(slot);
        }
        self.decode(slot, &bytes, from_backup)
      }
    }
  }

  /// The header of the save in `slot`, read without its data
  pub(crate) fn info(&self, slot: &str) -> Result<SaveHeader, SaveError> {
    let storage = self.storage();
    check_reachable(slot, storage.as_deref())?;
    let file = file_name(slot);
    match self.shared.queued_header(&file) {
      QueuedHeader::Deleted => return Err(SaveError::NotFound(slot.to_string())),
      QueuedHeader::Header(header) => return Ok(header),
      QueuedHeader::Nothing => {}
    }
    let storage = storage.ok_or(SaveError::Unavailable)?;
    match stored_header(storage.as_ref(), &file)? {
      Recovery::Missing => Err(SaveError::NotFound(slot.to_string())),
      Recovery::Damaged => Err(damaged(slot)),
      Recovery::Found { bytes, .. } => {
        container::read_header(&bytes).map_err(|error| in_slot(error, slot))
      }
    }
  }

  /// Every slot with its header, sorted by slot; the queued saves among them
  pub(crate) fn list(&self) -> Result<Vec<SaveSlot>, SaveError> {
    let storage = self.storage().ok_or(SaveError::Unavailable)?;
    let names = storage
      .names()
      .map_err(|error| io_error(&format!("*{SAVE_EXTENSION}"), error))?;
    let mut files: BTreeSet<String> = names.iter().filter_map(|name| save_file_of(name)).collect();
    let mut queued = BTreeMap::new();
    for (file, pending) in lock(&self.shared.slots).iter() {
      files.insert(file.clone());
      let header = match &pending.op {
        PendingOp::Delete => None,
        PendingOp::Save { header, encoded } => match encoded.peek() {
          Encoding::Failed => continue,
          _ => Some(header.clone()),
        },
      };
      queued.insert(file.clone(), header);
    }

    let mut slots = Vec::new();
    for file in files {
      let Some(slot) = file.strip_suffix(SAVE_EXTENSION) else {
        continue;
      };
      if check_new(slot).is_err() && !(LEGACY_NAMES && is_legacy(slot)) {
        continue;
      }
      let (header, from_backup) = match queued.remove(&file) {
        Some(Some(header)) => (header, false),
        Some(None) => continue,
        None => match stored_header(storage.as_ref(), &file) {
          Ok(Recovery::Found { bytes, from }) => match container::read_header(&bytes) {
            Ok(header) => (header, from == Origin::Backup),
            Err(_) => continue,
          },
          // A slot with no header to show (damaged, or a newer engine's) is
          // left out; loading it says why
          Ok(Recovery::Missing | Recovery::Damaged) | Err(_) => continue,
        },
      };
      slots.push(SaveSlot {
        slot: slot.to_string(),
        header,
        from_backup,
      });
    }
    // File names order `a-b.ryo-save` before `a.ryo-save`
    slots.sort_by(|a, b| a.slot.cmp(&b.slot));
    Ok(slots)
  }

  /// True when `slot` holds a save, or one is queued for it
  pub(crate) fn exists(&self, slot: &str) -> bool {
    let storage = self.storage();
    if check_reachable(slot, storage.as_deref()).is_err() {
      return false;
    }
    let file = file_name(slot);
    match self.shared.queued_header(&file) {
      QueuedHeader::Deleted => false,
      QueuedHeader::Header(_) => true,
      QueuedHeader::Nothing => storage.is_some_and(|storage| kept(storage.as_ref(), &file)),
    }
  }

  /// Queues the removal of `slot`, its backup and its temporary files, after
  /// any save of it queued before
  pub(crate) fn delete(&self, slot: &str) -> Result<(), SaveError> {
    let storage = self.storage();
    check_reachable(slot, storage.as_deref())?;
    let storage = storage.ok_or(SaveError::Unavailable)?;
    let file = file_name(slot);
    let generation = self.shared.begin(&file, PendingOp::Delete);
    let meta = JobMeta {
      kind: JobKind::Delete {
        slot: slot.to_string(),
      },
      file: file.clone(),
      generation: Some(generation),
    };
    // Never skipped for a later save: the deleted save would become that
    // save's backup
    self.queue(meta, move || {
      JobEnd::from_completion(storage.remove_all(&file), &file)
    });
    Ok(())
  }

  /// True while a save or delete of `slot` is queued or its result not yet
  /// drained
  pub(crate) fn is_writing(&self, slot: &str) -> bool {
    lock(&self.shared.slots).contains_key(&file_name(slot))
  }

  /// Queues a crash-safe write of the file `name` (the settings' file) on
  /// the same writer as the saves; returns the job's id, which its
  /// [`Finished`] carries
  #[cfg_attr(not(test), expect(dead_code, reason = "the settings writer's"))]
  pub(crate) fn write_named(
    &self,
    name: &str,
    bytes: Vec<u8>,
    verify: fn(&[u8]) -> Option<u64>,
  ) -> u64 {
    let storage = self.storage();
    let file = name.to_string();
    let meta = JobMeta {
      kind: JobKind::Named {
        name: name.to_string(),
      },
      file: file.clone(),
      generation: None,
    };
    self.queue(meta, move || match storage {
      Some(storage) => JobEnd::from_completion(storage.replace(&file, &bytes, &verify), &file),
      None => JobEnd::Done(Err(SaveError::Unavailable)),
    })
  }

  /// The newest whole copy of the file `name`; `Missing` with no storage
  pub(crate) fn recover_named(
    &self,
    name: &str,
    verify: fn(&[u8]) -> Option<u64>,
  ) -> io::Result<Recovery> {
    match self.storage() {
      Some(storage) => storage.recover(name, &verify),
      None => Ok(Recovery::Missing),
    }
  }

  /// Renames the file `name` to `to`, replacing `to`
  pub(crate) fn set_aside_named(&self, name: &str, to: &str) -> Completion {
    match self.storage() {
      Some(storage) => storage.set_aside(name, to),
      None => Completion::Done(Err(io::Error::other("save storage is unavailable"))),
    }
  }

  /// The jobs finished since the last call, slot jobs and named files alike;
  /// a slot whose latest job is among them is no longer writing
  pub(crate) fn drain(&self) -> Vec<Finished> {
    let ready = {
      let mut results = lock(&self.results);
      self.collect(&mut results);
      std::mem::take(&mut results.ready)
    };
    if !ready.is_empty() {
      let mut slots = lock(&self.shared.slots);
      for finished in &ready {
        if let Some(generation) = finished.generation
          && slots
            .get(&finished.file)
            .is_some_and(|pending| pending.generation == generation)
        {
          slots.remove(&finished.file);
        }
      }
    }
    ready
  }

  /// Waits for every queued job to finish: warns after `warn_after`, and
  /// gives up after `give_up`, returning the files still being written. Once
  /// it has given up, later calls return at once
  pub(crate) fn flush(&self, warn_after: Duration, give_up: Duration) -> Vec<String> {
    let start = Instant::now();
    let gave_up = self.gave_up.load(Ordering::Relaxed);
    let (warn_at, give_up_at) = if gave_up {
      (start, start)
    } else {
      (start + warn_after, start + give_up)
    };
    if !self.writer.wait_idle(warn_at) && !gave_up {
      crate::engine_warn!(
        module: "Saves",
        "still writing {} after {warn_after:?}; waiting up to {give_up:?}",
        self.writer.unfinished().join(", ")
      );
      self.writer.wait_idle(give_up_at);
    }

    let mut unfinished = self.writer.unfinished();
    {
      let mut results = lock(&self.results);
      self.collect(&mut results);
      for (id, meta, completion) in std::mem::take(&mut results.committing) {
        match completion.wait_until(give_up_at) {
          Ok(result) => {
            let result = result.map_err(|error| Arc::new(io_error(&meta.file, error)));
            results.ready.push(meta.finish(id, result));
          }
          Err(still) => {
            if !unfinished.contains(&meta.file) {
              unfinished.push(meta.file.clone());
            }
            results.committing.push((id, meta, still));
          }
        }
      }
    }
    if !unfinished.is_empty() && !self.gave_up.swap(true, Ordering::Relaxed) {
      crate::engine_warn!(
        module: "Saves",
        "gave up after {give_up:?} waiting to write {}: those writes may be lost",
        unfinished.join(", ")
      );
    }
    unfinished
  }

  /// Sends `job` to the writer and keeps what it is for, under the results'
  /// lock, so `drain` never sees a result before its job is known
  fn queue(&self, meta: JobMeta, job: impl FnOnce() -> JobEnd + Send + 'static) -> u64 {
    let mut results = lock(&self.results);
    let id = self.writer.send(&meta.file, job);
    results.jobs.insert(id, meta);
    id
  }

  /// Moves the writer's results into `ready`, and the storage's late
  /// commits that have landed
  fn collect(&self, results: &mut Results) {
    for (id, end) in self.writer.finished() {
      let Some(meta) = results.jobs.remove(&id) else {
        continue;
      };
      match end {
        Some(JobEnd::Done(result)) => results
          .ready
          .push(meta.finish(id, result.map_err(Arc::new))),
        Some(JobEnd::Committing(completion)) => results.committing.push((id, meta, completion)),
        None => {
          let panicked = SaveError::Serialize("the save job panicked".into());
          results.ready.push(meta.finish(id, Err(Arc::new(panicked))));
        }
      }
    }
    for (id, meta, completion) in std::mem::take(&mut results.committing) {
      match completion.poll() {
        Ok(result) => {
          let result = result.map_err(|error| Arc::new(io_error(&meta.file, error)));
          results.ready.push(meta.finish(id, result));
        }
        Err(still) => results.committing.push((id, meta, still)),
      }
    }
  }

  fn decode<T: DeserializeOwned + 'static>(
    &self,
    slot: &str,
    bytes: &[u8],
    from_backup: bool,
  ) -> Result<Loaded<T>, SaveError> {
    let (header, body) = container::decode(bytes).map_err(|error| in_slot(error, slot))?;
    let (data, upgraded_from) = self
      .versions
      .decode_ron::<T>(header.version, &body)
      .map_err(|error| in_slot(error, slot))?;
    Ok(Loaded {
      data,
      header,
      from_backup,
      upgraded_from,
    })
  }

  fn warn_backup(&self, slot: &str) {
    if lock(&self.backup_warned).insert(slot.to_string()) {
      crate::engine_warn!(
        module: "Saves",
        "save \"{slot}\" is damaged, so its backup (the save before it) was loaded; this warns once per slot"
      );
    }
  }
}

/// Older names stay reachable where the old save manager could write them
const LEGACY_NAMES: bool = cfg!(not(target_arch = "wasm32"));

/// Checks a slot name for reading or deleting: a new name, or (on native) an
/// older name with a save kept under it
fn check_reachable(slot: &str, storage: Option<&dyn Storage>) -> Result<(), SaveError> {
  let Err(error) = check_new(slot) else {
    return Ok(());
  };
  let kept_legacy = LEGACY_NAMES
    && is_legacy(slot)
    && storage.is_some_and(|storage| kept(storage, &file_name(slot)));
  if kept_legacy { Ok(()) } else { Err(error) }
}

/// True when the storage holds a copy of `file`: itself, its backup or a
/// temporary file
fn kept(storage: &dyn Storage, file: &str) -> bool {
  let backup = backup_name(file);
  storage.names().is_ok_and(|names| {
    names
      .iter()
      .any(|name| *name == file || *name == backup || is_temp_of(name, file))
  })
}

/// The save file a stored name is a copy of: the file, its backup or one of
/// its temporary files
fn save_file_of(name: &str) -> Option<String> {
  if name.ends_with(SAVE_EXTENSION) {
    return Some(name.to_string());
  }
  if let Some(file) = name.strip_suffix(".bak")
    && file.ends_with(SAVE_EXTENSION)
  {
    return Some(file.to_string());
  }
  let (file, _) = name.strip_suffix(".tmp")?.rsplit_once('.')?;
  (file.ends_with(SAVE_EXTENSION) && is_temp_of(name, file)).then(|| file.to_string())
}

/// The format of `file` as stored, when a newer engine wrote it
fn newer_format(storage: &dyn Storage, file: &str) -> Option<u32> {
  let bytes = storage.read(file).ok().flatten()?;
  container::peek_format(&bytes).filter(|found| *found > FORMAT)
}

/// `NewerVersion` when a newer engine wrote `file`: its backup is not taken
/// in its place
fn refuse_newer(storage: &dyn Storage, file: &str) -> Result<(), SaveError> {
  let bytes = storage.read(file).map_err(|error| io_error(file, error))?;
  match bytes.and_then(|bytes| container::peek_format(&bytes)) {
    Some(found) if found > FORMAT => Err(newer(found)),
    _ => Ok(()),
  }
}

/// The newest copy of `file` whose header reads (a slot list checks no more)
fn stored_header(storage: &dyn Storage, file: &str) -> Result<Recovery, SaveError> {
  refuse_newer(storage, file)?;
  storage
    .recover(file, &|bytes| {
      container::read_header(bytes)
        .ok()
        .map(|header| header.saved_at)
    })
    .map_err(|error| io_error(file, error))
}

fn encode<T: Serialize>(header: &SaveHeader, data: &T) -> Result<Vec<u8>, SaveError> {
  let ron = ron::to_string(data).map_err(|error| SaveError::Serialize(Box::new(error)))?;
  container::encode(header, &ron)
}

fn newer(found: u32) -> SaveError {
  SaveError::NewerVersion {
    found,
    current: FORMAT,
  }
}

fn damaged(slot: &str) -> SaveError {
  SaveError::Corrupt {
    slot: slot.to_string(),
    reason: "the save and its backup are damaged".to_string(),
  }
}

/// A storage error: `StorageFull` for a full store, else `Io` naming the file
fn io_error(file: &str, error: io::Error) -> SaveError {
  if error.kind() == io::ErrorKind::StorageFull {
    SaveError::StorageFull
  } else {
    SaveError::Io {
      path: file.to_string(),
      source: error,
    }
  }
}

/// Names the slot in an error the container or the versions left unnamed
fn in_slot(error: SaveError, slot: &str) -> SaveError {
  match error {
    SaveError::Corrupt {
      slot: unnamed,
      reason,
    } if unnamed.is_empty() => SaveError::Corrupt {
      slot: slot.to_string(),
      reason,
    },
    SaveError::Deserialize {
      slot: unnamed,
      source,
    } if unnamed.is_empty() => SaveError::Deserialize {
      slot: slot.to_string(),
      source,
    },
    other => other,
  }
}

/// Seconds since the Unix epoch
fn unix_now() -> u64 {
  SystemTime::now()
    .duration_since(SystemTime::UNIX_EPOCH)
    .map_or(0, |since| since.as_secs())
}

#[cfg(test)]
mod tests {
  use std::io;
  use std::sync::{Arc, Mutex};
  use std::time::{Duration, Instant};

  use crossbeam_channel::Sender;
  use serde::ser::Error as _;
  use serde::{Deserialize, Serialize, Serializer};

  use super::*;
  use crate::core::saves::container::fixtures::{LEGACY_SAVE, LegacyFixture};
  use crate::core::saves::files::FileStorage;
  use crate::core::saves::memory::{FaultOp, MemorySaves};
  use crate::core::saves::storage::{Completion, Recovery, Storage, Verify};
  use crate::core::saves::{SaveError, SaveFinished, SaveInfo, SaveOp, SaveVersions};

  #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
  struct Hero {
    level: u32,
  }

  fn hero(level: u32) -> Hero {
    Hero { level }
  }

  fn service_over(storage: Arc<dyn Storage>, versions: VersionRegistry) -> SaveService {
    let service = SaveService::new(versions);
    service.set_storage(storage);
    service
  }

  fn service(store: &MemorySaves) -> SaveService {
    service_over(Arc::new(store.clone()), VersionRegistry::default())
  }

  /// Waits for every queued job, then the slot results since the last drain
  fn settle(service: &SaveService) -> Vec<SaveFinished> {
    let unfinished = service.flush(Duration::from_secs(5), Duration::from_secs(10));
    assert!(unfinished.is_empty(), "still writing {unfinished:?}");
    service
      .drain()
      .into_iter()
      .filter_map(|finished| finished.message())
      .collect()
  }

  fn warnings_containing(needle: &str) -> usize {
    crate::core::dev_log::get_entries()
      .iter()
      .filter(|entry| entry.level == crate::core::dev_log::LogLevel::Warn)
      .filter(|entry| entry.message.contains(needle))
      .count()
  }

  /// A save of `level` whose container format is 3, as a newer engine
  /// would write it
  fn newer_save(level: u32) -> Vec<u8> {
    let store = MemorySaves::new();
    let service = service(&store);
    service.save("x", hero(level), SaveInfo::new()).unwrap();
    settle(&service);
    let mut bytes = store.get("x.ryo-save").unwrap();
    bytes[4..8].copy_from_slice(&3u32.to_le_bytes());
    bytes
  }

  #[test]
  fn a_load_while_a_write_is_queued_returns_the_queued_data() {
    let store = MemorySaves::new();
    let service = service(&store);
    service.save("a", hero(1), SaveInfo::new()).unwrap();
    settle(&service);

    store.slow(Duration::from_millis(50));
    service.save("a", hero(2), SaveInfo::new()).unwrap();
    assert!(service.is_writing("a"));
    let loaded = service.load::<Hero>("a").unwrap();
    assert_eq!(loaded.data, hero(2));
    assert!(!loaded.from_backup);
    assert_eq!(loaded.upgraded_from, None);

    let finished = settle(&service);
    assert_eq!(finished.len(), 1);
    assert!(finished[0].result.is_ok());
    assert!(!service.is_writing("a"));
  }

  #[test]
  fn two_saves_to_one_slot_write_the_later_and_report_both() {
    let store = MemorySaves::new();
    let service = service(&store);
    store.slow(Duration::from_millis(20));
    // Keeps the writer busy while both saves of `a` are queued
    service.write_named("blocker", b"x".to_vec(), |_| Some(0));
    service.save("a", hero(1), SaveInfo::new()).unwrap();
    service.save("a", hero(2), SaveInfo::new()).unwrap();

    let finished = settle(&service);
    assert_eq!(store.writes(), 2, "the blocker, then one save of a");
    assert_eq!(service.load::<Hero>("a").unwrap().data, hero(2));
    assert_eq!(finished.len(), 2);
    for result in &finished {
      assert_eq!((result.slot.as_str(), result.op), ("a", SaveOp::Save));
      assert!(result.result.is_ok(), "{result:?}");
    }
  }

  #[test]
  fn a_delete_after_a_queued_save_leaves_no_slot() {
    let store = MemorySaves::new();
    let service = service(&store);
    store.slow(Duration::from_millis(20));
    service.save("a", hero(1), SaveInfo::new()).unwrap();
    service.delete("a").unwrap();
    assert!(!service.exists("a"));
    assert!(matches!(service.load::<Hero>("a"), Err(SaveError::NotFound(slot)) if slot == "a"));

    let finished = settle(&service);
    let ops: Vec<_> = finished.iter().map(|f| (f.op, f.result.is_ok())).collect();
    assert_eq!(ops, [(SaveOp::Save, true), (SaveOp::Delete, true)]);
    assert!(!service.exists("a"));
    assert!(matches!(
      service.load::<Hero>("a"),
      Err(SaveError::NotFound(_))
    ));
    assert!(store.names().is_empty(), "{:?}", store.names());
  }

  #[test]
  fn a_save_after_a_delete_leaves_no_backup_of_the_deleted_save() {
    let store = MemorySaves::new();
    let service = service(&store);
    service.save("s", hero(1), SaveInfo::new()).unwrap();
    settle(&service);

    store.slow(Duration::from_millis(20));
    // Keeps the writer busy while the delete and the new save are queued
    service.write_named("blocker", b"x".to_vec(), |_| Some(0));
    service.delete("s").unwrap();
    service.save("s", hero(2), SaveInfo::new()).unwrap();
    let finished = settle(&service);
    let ops: Vec<_> = finished.iter().map(|f| (f.op, f.result.is_ok())).collect();
    assert_eq!(ops, [(SaveOp::Delete, true), (SaveOp::Save, true)]);

    assert_eq!(
      store.get("s.ryo-save.bak"),
      None,
      "the deleted save is gone"
    );
    let loaded = service.load::<Hero>("s").unwrap();
    assert_eq!(loaded.data, hero(2));
    assert!(!loaded.from_backup);
    assert_eq!(store.names(), ["blocker", "s.ryo-save"]);
  }

  #[test]
  fn pending_writes_finish_before_flush_returns() {
    let store = MemorySaves::new();
    let service = service(&store);
    store.slow(Duration::from_millis(5));
    for slot in ["a", "b", "c"] {
      service.save(slot, hero(1), SaveInfo::new()).unwrap();
    }
    assert!(
      service
        .flush(Duration::from_secs(5), Duration::from_secs(10))
        .is_empty()
    );
    assert_eq!(
      store.names(),
      ["a.ryo-save", "b.ryo-save", "c.ryo-save"].map(String::from)
    );
    // Nothing is left to wait for
    let started = Instant::now();
    assert!(
      service
        .flush(Duration::from_secs(5), Duration::from_secs(10))
        .is_empty()
    );
    assert!(started.elapsed() < Duration::from_millis(100));
  }

  #[test]
  fn flush_warns_then_gives_up() {
    let store = MemorySaves::new();
    let service = service(&store);
    store.slow(Duration::from_secs(1));
    service.save("gives-up", hero(1), SaveInfo::new()).unwrap();

    let started = Instant::now();
    let unfinished = service.flush(Duration::from_millis(30), Duration::from_millis(100));
    assert_eq!(unfinished, ["gives-up.ryo-save"]);
    assert!(started.elapsed() < Duration::from_millis(900));
    assert_eq!(
      warnings_containing("gives-up.ryo-save"),
      2,
      "one warning while waiting, one on giving up"
    );

    // Once given up, a later flush does not wait again
    let started = Instant::now();
    assert_eq!(
      service.flush(Duration::from_secs(3), Duration::from_secs(10)),
      ["gives-up.ryo-save"]
    );
    assert!(started.elapsed() < Duration::from_millis(900));
  }

  #[test]
  fn saving_one_slot_every_frame_writes_a_bounded_number_of_times() {
    let store = MemorySaves::new();
    let service = service(&store);
    store.slow(Duration::from_millis(5));
    for level in 0..1000 {
      service
        .save("every-frame", hero(level), SaveInfo::new())
        .unwrap();
    }
    let finished = settle(&service);
    assert!(store.writes() <= 10, "{} writes", store.writes());
    assert_eq!(service.load::<Hero>("every-frame").unwrap().data, hero(999));
    assert_eq!(finished.len(), 1000);
    assert!(finished.iter().all(|f| f.result.is_ok()));
    assert!(lock(&service.shared.slots).is_empty());
  }

  #[test]
  fn a_store_that_refuses_writes_reports_each_save_and_keeps_the_last_good_one() {
    let store = MemorySaves::new();
    let service = service(&store);
    service.save("a", hero(1), SaveInfo::new()).unwrap();
    settle(&service);

    store.fail_from(FaultOp::Write, 0);
    for level in 2..5 {
      service.save("a", hero(level), SaveInfo::new()).unwrap();
      let finished = settle(&service);
      assert_eq!(finished.len(), 1);
      match finished[0].result.as_ref().map_err(|e| &**e) {
        Err(SaveError::Io { path, .. }) => assert_eq!(path, "a.ryo-save"),
        other => panic!("expected an Io error, got {other:?}"),
      }
    }
    assert_eq!(service.load::<Hero>("a").unwrap().data, hero(1));
  }

  #[test]
  fn loading_a_slot_as_another_type_is_a_deserialize_error() {
    #[derive(Serialize)]
    struct A {
      x: u32,
    }
    #[derive(Debug, Deserialize)]
    struct B {
      #[allow(dead_code)]
      name: String,
    }

    let store = MemorySaves::new();
    let service = service(&store);
    service.save("a", A { x: 1 }, SaveInfo::new()).unwrap();
    // From the queued bytes, then from the store
    for _ in 0..2 {
      match service.load::<B>("a") {
        Err(SaveError::Deserialize { slot, .. }) => assert_eq!(slot, "a"),
        other => panic!("expected Deserialize, got {other:?}"),
      }
      settle(&service);
    }
  }

  #[test]
  fn a_save_whose_data_cannot_serialize_reports_it_and_keeps_the_previous_save() {
    struct Unserializable;
    impl Serialize for Unserializable {
      fn serialize<S: Serializer>(&self, _: S) -> Result<S::Ok, S::Error> {
        Err(S::Error::custom("no"))
      }
    }

    let store = MemorySaves::new();
    let service = service(&store);
    service.save("a", hero(1), SaveInfo::new()).unwrap();
    settle(&service);

    service.save("a", Unserializable, SaveInfo::new()).unwrap();
    assert_eq!(service.load::<Hero>("a").unwrap().data, hero(1));
    let finished = settle(&service);
    assert_eq!(finished.len(), 1);
    assert!(matches!(
      finished[0].result.as_ref().map_err(|e| &**e),
      Err(SaveError::Serialize(_))
    ));
    assert_eq!(service.load::<Hero>("a").unwrap().data, hero(1));
  }

  #[test]
  fn a_slot_damaged_on_disk_loads_from_its_backup_with_a_warning() {
    let store = MemorySaves::new();
    let service = service(&store);
    for level in [1, 2] {
      service
        .save("damaged-slot", hero(level), SaveInfo::new())
        .unwrap();
      settle(&service);
    }
    let damage = |name: &str| {
      let mut bytes = store.get(name).unwrap();
      let last = bytes.len() - 1;
      bytes[last] ^= 0xff;
      store.put(name, bytes);
    };

    damage("damaged-slot.ryo-save");
    let loaded = service.load::<Hero>("damaged-slot").unwrap();
    assert_eq!(loaded.data, hero(1));
    assert!(loaded.from_backup);
    assert_eq!(warnings_containing("damaged-slot"), 1);
    service.load::<Hero>("damaged-slot").unwrap();
    assert_eq!(
      warnings_containing("damaged-slot"),
      1,
      "warned once per slot"
    );

    damage("damaged-slot.ryo-save.bak");
    match service.load::<Hero>("damaged-slot") {
      Err(SaveError::Corrupt { slot, .. }) => assert_eq!(slot, "damaged-slot"),
      other => panic!("expected Corrupt, got {other:?}"),
    }
  }

  #[test]
  fn old_slot_names_stay_reachable_on_native() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("My Save.ryo-save"), LEGACY_SAVE).unwrap();
    let storage = FileStorage::new(dir.path().to_path_buf()).unwrap();
    let service = service_over(Arc::new(storage), VersionRegistry::default());

    let loaded = service.load::<LegacyFixture>("My Save").unwrap();
    assert_eq!(loaded.data, LegacyFixture::written());
    assert!(service.exists("My Save"));
    let slots: Vec<_> = service
      .list()
      .unwrap()
      .into_iter()
      .map(|slot| slot.slot)
      .collect();
    assert_eq!(slots, ["My Save"]);
    assert!(matches!(
      service.save("My Save", LegacyFixture::written(), SaveInfo::new()),
      Err(SaveError::InvalidSlot(_))
    ));
    assert!(matches!(
      service.load::<LegacyFixture>("Other Save"),
      Err(SaveError::InvalidSlot(_))
    ));

    service.delete("My Save").unwrap();
    settle(&service);
    assert!(!dir.path().join("My Save.ryo-save").exists());
  }

  #[test]
  fn the_legacy_fixture_loads_through_the_service() {
    let store = MemorySaves::new();
    store.put("slot1.ryo-save", LEGACY_SAVE);
    let service = service(&store);

    let loaded = service.load::<LegacyFixture>("slot1").unwrap();
    assert_eq!(loaded.data, LegacyFixture::written());
    assert_eq!(loaded.header.format, 1);
    assert_eq!(loaded.upgraded_from, None);
    assert!(!loaded.from_backup);
    let header = service.info("slot1").unwrap();
    assert_eq!(header.name.as_deref(), Some("Before the update"));

    // The next save of the slot writes today's format
    service.save("slot1", loaded.data, SaveInfo::new()).unwrap();
    settle(&service);
    let bytes = store.get("slot1.ryo-save").unwrap();
    assert_eq!(container::peek_format(&bytes), Some(2));
    assert_eq!(
      service.load::<LegacyFixture>("slot1").unwrap().data,
      LegacyFixture::written()
    );
  }

  #[test]
  fn a_save_from_a_newer_engine_is_refused_not_recovered() {
    let store = MemorySaves::new();
    let service = service(&store);
    for level in [1, 2] {
      service.save("a", hero(level), SaveInfo::new()).unwrap();
      settle(&service);
    }
    assert!(store.get("a.ryo-save.bak").is_some());
    store.put("a.ryo-save", newer_save(3));

    assert!(matches!(
      service.load::<Hero>("a"),
      Err(SaveError::NewerVersion {
        found: 3,
        current: 2
      })
    ));
    assert!(matches!(
      service.info("a"),
      Err(SaveError::NewerVersion {
        found: 3,
        current: 2
      })
    ));
  }

  #[test]
  fn a_save_over_a_newer_file_is_refused() {
    let store = MemorySaves::new();
    let service = service(&store);
    let newer = newer_save(3);
    store.put("a.ryo-save", newer.clone());

    service.save("a", hero(1), SaveInfo::new()).unwrap();
    let finished = settle(&service);
    assert_eq!(finished.len(), 1);
    assert!(matches!(
      finished[0].result.as_ref().map_err(|e| &**e),
      Err(SaveError::NewerVersion {
        found: 3,
        current: 2
      })
    ));
    assert_eq!(store.get("a.ryo-save"), Some(newer));
    assert_eq!(store.names(), ["a.ryo-save"]);
  }

  #[test]
  fn a_bad_slot_name_or_no_storage_is_refused_at_once() {
    let store = MemorySaves::new();
    let service = service(&store);
    for bad in ["", "Slot1", "../x", "con"] {
      assert!(matches!(
        service.save(bad, hero(1), SaveInfo::new()),
        Err(SaveError::InvalidSlot(_))
      ));
    }

    let none = SaveService::new(VersionRegistry::default());
    assert!(matches!(
      none.save("a", hero(1), SaveInfo::new()),
      Err(SaveError::Unavailable)
    ));
    assert!(matches!(
      none.load::<Hero>("a"),
      Err(SaveError::Unavailable)
    ));
    assert!(matches!(none.list(), Err(SaveError::Unavailable)));
    assert!(matches!(none.delete("a"), Err(SaveError::Unavailable)));
    assert!(!none.exists("a"));
  }

  #[test]
  fn list_gives_every_slot_with_its_header_sorted() {
    let store = MemorySaves::new();
    let service = service(&store);
    service
      .save(
        "b",
        hero(2),
        SaveInfo::new()
          .name("Chapter two")
          .play_time(90)
          .label("area", "caves"),
      )
      .unwrap();
    service.save("a", hero(1), SaveInfo::new()).unwrap();
    // `-` sorts before the `.` of `.ryo-save`, so file names alone would put
    // `a-b` first
    service.save("a-b", hero(1), SaveInfo::new()).unwrap();
    settle(&service);
    // A save still queued is listed too
    store.slow(Duration::from_millis(20));
    service.save("c", hero(3), SaveInfo::new()).unwrap();

    let slots = service.list().unwrap();
    let names: Vec<_> = slots.iter().map(|slot| slot.slot.as_str()).collect();
    assert_eq!(names, ["a", "a-b", "b", "c"]);
    let b = &slots[2];
    assert_eq!(b.header.name.as_deref(), Some("Chapter two"));
    assert_eq!(b.header.play_time, 90);
    assert_eq!(b.header.labels["area"], "caves");
    assert_eq!(b.header.engine_version, env!("CARGO_PKG_VERSION"));
    assert!(slots.iter().all(|slot| !slot.from_backup));
    assert_eq!(service.info("b").unwrap(), b.header);
    settle(&service);
  }

  #[test]
  fn a_registered_type_saves_its_version_and_upgrades_older_data() {
    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct HeroV2 {
      level: u32,
      gold: u32,
    }

    let store = MemorySaves::new();
    let old = service(&store);
    old.save("a", hero(4), SaveInfo::new()).unwrap();
    settle(&old);

    let mut versions = VersionRegistry::default();
    versions.register(SaveVersions::<HeroV2>::new(2).step(1, |old: Hero| HeroV2 {
      level: old.level,
      gold: 0,
    }));
    let service = service_over(Arc::new(store.clone()), versions);
    let loaded = service.load::<HeroV2>("a").unwrap();
    assert_eq!(loaded.data, HeroV2 { level: 4, gold: 0 });
    assert_eq!(loaded.upgraded_from, Some(1));

    service.save("a", loaded.data, SaveInfo::new()).unwrap();
    settle(&service);
    assert_eq!(service.info("a").unwrap().version, 2);
    assert_eq!(service.load::<HeroV2>("a").unwrap().upgraded_from, None);
  }

  #[test]
  fn named_files_are_written_recovered_and_set_aside() {
    fn verify(bytes: &[u8]) -> Option<u64> {
      bytes.starts_with(b"{").then_some(1)
    }

    let store = MemorySaves::new();
    let service = service(&store);
    let id = service.write_named("system.json", b"{}".to_vec(), verify);
    service.flush(Duration::from_secs(5), Duration::from_secs(10));
    let finished = service.drain();
    assert_eq!(finished.len(), 1);
    assert_eq!(finished[0].id, id);
    assert!(finished[0].result.is_ok());
    assert!(
      finished[0].message().is_none(),
      "no SaveFinished for a named file"
    );

    assert!(matches!(
      service.recover_named("system.json", verify).unwrap(),
      Recovery::Found { ref bytes, .. } if bytes == b"{}"
    ));
    service
      .set_aside_named("system.json", "system.json.damaged")
      .wait()
      .unwrap();
    assert_eq!(store.names(), ["system.json.damaged"]);
  }

  /// Memory storage whose replaces commit when the test says, as the
  /// browser's do
  struct Deferred {
    files: MemorySaves,
    commits: Mutex<Vec<Sender<io::Result<()>>>>,
  }

  impl Storage for Deferred {
    fn read(&self, name: &str) -> io::Result<Option<Vec<u8>>> {
      Storage::read(&self.files, name)
    }
    fn names(&self) -> io::Result<Vec<String>> {
      Storage::names(&self.files)
    }
    fn replace(&self, name: &str, bytes: &[u8], verify: Verify<'_>) -> Completion {
      let _ = self.files.replace(name, bytes, verify).wait();
      let (send, receive) = crossbeam_channel::bounded(1);
      self.commits.lock().unwrap().push(send);
      Completion::Pending(receive)
    }
    fn recover(&self, name: &str, verify: Verify<'_>) -> io::Result<Recovery> {
      self.files.recover(name, verify)
    }
    fn remove_all(&self, name: &str) -> Completion {
      self.files.remove_all(name)
    }
    fn set_aside(&self, name: &str, to: &str) -> Completion {
      self.files.set_aside(name, to)
    }
  }

  #[test]
  fn a_write_that_commits_later_is_reported_when_it_commits() {
    let storage = Arc::new(Deferred {
      files: MemorySaves::new(),
      commits: Mutex::new(Vec::new()),
    });
    let service = service_over(storage.clone(), VersionRegistry::default());
    service.save("later", hero(1), SaveInfo::new()).unwrap();

    let unfinished = service.flush(Duration::from_millis(10), Duration::from_millis(50));
    assert_eq!(unfinished, ["later.ryo-save"]);
    assert!(service.drain().is_empty());
    assert!(service.is_writing("later"));

    for commit in storage.commits.lock().unwrap().drain(..) {
      commit
        .send(Err(io::Error::from(io::ErrorKind::StorageFull)))
        .unwrap();
    }
    let finished: Vec<_> = service
      .drain()
      .into_iter()
      .filter_map(|finished| finished.message())
      .collect();
    assert_eq!(finished.len(), 1);
    assert!(matches!(
      finished[0].result.as_ref().map_err(|e| &**e),
      Err(SaveError::StorageFull)
    ));
    assert!(!service.is_writing("later"));
  }
}
