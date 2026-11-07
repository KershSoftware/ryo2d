//! The crash-safe order of replacing, recovering and removing a file, for
//! every [`FileOps`] backend.
//!
//! At every instant a whole, verified copy of either the old or the new bytes
//! exists, and [`recover`] finds it.

use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use super::slot::{backup_name, is_temp_of, temp_name};
use super::storage::{FileOps, Origin, Recovery, Verify};

/// How many times a rename refused because the file is held is tried again,
/// and how long apart: a virus scanner or an indexer may hold a fresh file for
/// a moment (about 100 ms in all)
const RENAME_RETRIES: usize = 10;
const RENAME_RETRY_PAUSE: Duration = Duration::from_millis(10);

/// A folder that cannot be synced warns once, not on every save
static SYNC_WARNED: AtomicBool = AtomicBool::new(false);

/// Replaces `name` with `bytes`:
/// 1. writes and flushes a temporary file;
/// 2. renames the current file to the backup, if it verifies;
/// 3. renames the temporary file over the current one;
/// 4. syncs the folder (a failure warns: the data is in place);
/// 5. removes this name's other temporary files.
///
/// A failed replace removes its own temporary file, so the old copy stays the
/// one found.
pub(crate) fn safe_replace(
  ops: &dyn FileOps,
  name: &str,
  bytes: &[u8],
  verify: Verify<'_>,
) -> io::Result<()> {
  let temp = temp_name(name);
  let result = replace_through(ops, name, &temp, bytes, verify);
  if result.is_err() {
    // A torn temporary file is garbage, and a whole one is a save the caller
    // is told failed; a removal that fails is cleared by the next save
    let _ = ops.remove(&temp);
  }
  result
}

fn replace_through(
  ops: &dyn FileOps,
  name: &str,
  temp: &str,
  bytes: &[u8],
  verify: Verify<'_>,
) -> io::Result<()> {
  ops.write_synced(temp, bytes)?;

  // A current file that does not verify never replaces the backup
  if ops
    .read(name)?
    .is_some_and(|current| verify(&current).is_some())
  {
    match rename_retrying(ops, name, &backup_name(name)) {
      // Another writer moved it to the backup since it was read
      Err(error) if error.kind() == io::ErrorKind::NotFound => {}
      result => result?,
    }
  }

  rename_retrying(ops, temp, name)?;
  sync_or_warn(ops, name);

  // Leftovers of a crash or of another writer; the next save clears any this
  // one could not
  if let Ok(names) = ops.names() {
    for leftover in names.iter().filter(|other| is_temp_of(other, name)) {
      let _ = ops.remove(leftover);
    }
  }
  Ok(())
}

/// The newest whole copy of `name`: the file itself if it verifies, else the
/// verified temporary file with the highest recency, else the backup if it
/// verifies. `Damaged` when the file or the backup is there but nothing
/// verifies; a torn temporary file alone is no save, so that is `Missing`.
pub(crate) fn recover(ops: &dyn FileOps, name: &str, verify: Verify<'_>) -> io::Result<Recovery> {
  let mut damaged = false;
  if let Some(bytes) = ops.read(name)? {
    if verify(&bytes).is_some() {
      return Ok(Recovery::Found {
        bytes,
        from: Origin::Current,
      });
    }
    damaged = true;
  }

  if let Some(bytes) = newest_temp(ops, name, verify)? {
    return Ok(Recovery::Found {
      bytes,
      from: Origin::Temp,
    });
  }

  if let Some(bytes) = ops.read(&backup_name(name))? {
    if verify(&bytes).is_some() {
      return Ok(Recovery::Found {
        bytes,
        from: Origin::Backup,
      });
    }
    damaged = true;
  }

  Ok(if damaged {
    Recovery::Damaged
  } else {
    Recovery::Missing
  })
}

fn newest_temp(ops: &dyn FileOps, name: &str, verify: Verify<'_>) -> io::Result<Option<Vec<u8>>> {
  let mut newest: Option<(u64, Vec<u8>)> = None;
  for temp in ops.names()?.iter().filter(|other| is_temp_of(other, name)) {
    // Another writer may be clearing it
    let Ok(Some(bytes)) = ops.read(temp) else {
      continue;
    };
    if let Some(recency) = verify(&bytes)
      && newest.as_ref().is_none_or(|(best, _)| recency > *best)
    {
      newest = Some((recency, bytes));
    }
  }
  Ok(newest.map(|(_, bytes)| bytes))
}

/// Removes `name`'s temporary files, then its backup, then the file itself,
/// so a removal cut short leaves the whole save or nothing: a leftover backup
/// or temporary file would come back as the save.
pub(crate) fn remove_all(ops: &dyn FileOps, name: &str) -> io::Result<()> {
  for temp in ops.names()?.iter().filter(|other| is_temp_of(other, name)) {
    ops.remove(temp)?;
  }
  ops.remove(&backup_name(name))?;
  // The other copies' removal reaches the disk before the file's does
  sync_or_warn(ops, name);
  ops.remove(name)?;
  sync_or_warn(ops, name);
  Ok(())
}

/// Renames `name` to `to`, replacing `to`
pub(crate) fn set_aside(ops: &dyn FileOps, name: &str, to: &str) -> io::Result<()> {
  rename_retrying(ops, name, to)?;
  sync_or_warn(ops, name);
  Ok(())
}

fn rename_retrying(ops: &dyn FileOps, from: &str, to: &str) -> io::Result<()> {
  let mut retries = 0;
  loop {
    match ops.rename(from, to) {
      Err(error) if is_held(&error) && retries < RENAME_RETRIES => {
        retries += 1;
        pause(RENAME_RETRY_PAUSE);
      }
      result => return result,
    }
  }
}

/// True when the error is another process holding the file
fn is_held(error: &io::Error) -> bool {
  // Windows reports a file open without delete sharing as a sharing violation
  const ERROR_SHARING_VIOLATION: i32 = 32;
  error.kind() == io::ErrorKind::PermissionDenied
    || (cfg!(windows) && error.raw_os_error() == Some(ERROR_SHARING_VIOLATION))
}

fn sync_or_warn(ops: &dyn FileOps, name: &str) {
  if let Err(error) = ops.sync_dir()
    && !SYNC_WARNED.swap(true, Ordering::Relaxed)
  {
    crate::engine_warn!(
      module: "Saves",
      "could not sync the save folder after changing \"{name}\" ({error}); the change is in place, but a power cut may undo it (this warns once)"
    );
  }
}

/// Waits a moment; the browser cannot block, so there it returns at once
pub(super) fn pause(duration: Duration) {
  #[cfg(not(target_arch = "wasm32"))]
  std::thread::sleep(duration);
  #[cfg(target_arch = "wasm32")]
  let _ = duration;
}

#[cfg(test)]
mod tests {
  use std::sync::atomic::AtomicUsize;

  use super::*;
  use crate::core::saves::memory::{FaultOp, MemorySaves};

  const NAME: &str = "slot.ryo-save";
  const BACKUP: &str = "slot.ryo-save.bak";

  /// Accepts only the two whole test saves; the new one is the more recent
  fn verify(bytes: &[u8]) -> Option<u64> {
    match bytes {
      b"old" => Some(1),
      b"new" => Some(2),
      _ => None,
    }
  }

  fn store_with_old() -> MemorySaves {
    let store = MemorySaves::new();
    store.put(NAME, b"old");
    store
  }

  fn found(store: &MemorySaves) -> (Vec<u8>, Origin) {
    match recover(store, NAME, &verify) {
      Ok(Recovery::Found { bytes, from }) => (bytes, from),
      other => panic!("expected a save, got {other:?}"),
    }
  }

  /// Forwards to the store until `left` ops have run, then fails every op, as
  /// a process that stopped there leaves the store. A write that is cut off
  /// stores its first `torn` bytes.
  struct CutOff<'a> {
    store: &'a MemorySaves,
    left: AtomicUsize,
    torn: usize,
  }

  impl CutOff<'_> {
    fn step(&self) -> io::Result<()> {
      self
        .left
        .try_update(Ordering::SeqCst, Ordering::SeqCst, |n| n.checked_sub(1))
        .map(|_| ())
        .map_err(|_| io::Error::other("cut off"))
    }
  }

  impl FileOps for CutOff<'_> {
    fn read(&self, name: &str) -> io::Result<Option<Vec<u8>>> {
      self.step()?;
      FileOps::read(self.store, name)
    }

    fn write_synced(&self, name: &str, bytes: &[u8]) -> io::Result<()> {
      if let Err(cut) = self.step() {
        let torn = &bytes[..self.torn.min(bytes.len())];
        FileOps::write_synced(self.store, name, torn)?;
        return Err(cut);
      }
      FileOps::write_synced(self.store, name, bytes)
    }

    fn rename(&self, from: &str, to: &str) -> io::Result<()> {
      self.step()?;
      FileOps::rename(self.store, from, to)
    }

    fn remove(&self, name: &str) -> io::Result<()> {
      self.step()?;
      FileOps::remove(self.store, name)
    }

    fn names(&self) -> io::Result<Vec<String>> {
      self.step()?;
      FileOps::names(self.store)
    }

    fn sync_dir(&self) -> io::Result<()> {
      self.step()?;
      FileOps::sync_dir(self.store)
    }
  }

  fn assert_old_or_new(store: &MemorySaves, label: &str) {
    match recover(store, NAME, &verify) {
      Ok(Recovery::Found { bytes, .. }) => assert!(
        bytes == b"old" || bytes == b"new",
        "{label}: recovered {bytes:?}"
      ),
      other => panic!("{label}: expected the old or the new save, got {other:?}"),
    }
  }

  #[test]
  fn a_crash_at_any_step_leaves_the_old_or_the_new_save() {
    // The old save alone, and with a backup and a torn temporary file left by
    // an earlier crash, so the cleanup's removals are among the steps too
    let starts: [fn() -> MemorySaves; 2] = [store_with_old, || {
      let store = store_with_old();
      store.put(BACKUP, b"old");
      store.put("slot.ryo-save.1-0.tmp", b"ne");
      store
    }];

    for (start, fresh) in starts.into_iter().enumerate() {
      let clean = fresh();
      safe_replace(&clean, NAME, b"new", &verify).unwrap();
      let steps = clean.ops();
      assert!(
        steps >= 4,
        "a replace takes at least four ops, took {steps}"
      );

      for k in 0..steps {
        for op in [
          FaultOp::Write,
          FaultOp::Rename,
          FaultOp::Remove,
          FaultOp::SyncDir,
        ] {
          let store = fresh();
          store.fail(op, k);
          let _ = safe_replace(&store, NAME, b"new", &verify);
          assert_old_or_new(&store, &format!("start {start}, fail({op:?}, {k})"));
        }

        for keep in [0, 1, 2] {
          let store = fresh();
          store.crash_write(k, keep);
          let _ = safe_replace(&store, NAME, b"new", &verify);
          assert_old_or_new(&store, &format!("start {start}, crash_write({k}, {keep})"));
        }

        let store = fresh();
        let cut = CutOff {
          store: &store,
          left: AtomicUsize::new(k),
          torn: 2,
        };
        let _ = safe_replace(&cut, NAME, b"new", &verify);
        assert_old_or_new(&store, &format!("start {start}, cut off after {k} ops"));
      }
    }
  }

  #[test]
  fn a_damaged_current_falls_back_to_the_backup() {
    let store = MemorySaves::new();
    store.put(NAME, b"ne");
    store.put(BACKUP, b"old");
    assert_eq!(found(&store), (b"old".to_vec(), Origin::Backup));
  }

  #[test]
  fn a_damaged_current_never_becomes_the_backup() {
    let store = MemorySaves::new();
    store.put(NAME, b"damaged");
    store.put(BACKUP, b"old");
    safe_replace(&store, NAME, b"new", &verify).unwrap();
    assert_eq!(store.get(BACKUP).as_deref(), Some(&b"old"[..]));
    assert_eq!(store.get(NAME).as_deref(), Some(&b"new"[..]));
  }

  #[test]
  fn a_newer_complete_temp_wins_over_the_backup() {
    let store = store_with_old();
    // The first rename makes the backup; the second, the temp over the slot,
    // fails, and so does every cleanup, as if the run stopped after it
    store.fail(FaultOp::Rename, 1);
    store.fail_from(FaultOp::Remove, 0);
    assert!(safe_replace(&store, NAME, b"new", &verify).is_err());
    assert_eq!(store.get(NAME), None);
    assert_eq!(store.get(BACKUP).as_deref(), Some(&b"old"[..]));
    assert_eq!(found(&store), (b"new".to_vec(), Origin::Temp));
  }

  #[test]
  fn the_newest_verified_temp_is_chosen() {
    let store = MemorySaves::new();
    store.put("slot.ryo-save.7-3.tmp", b"new");
    store.put("slot.ryo-save.7-9.tmp", b"old");
    store.put("slot.ryo-save.8-0.tmp", b"ne");
    assert_eq!(found(&store), (b"new".to_vec(), Origin::Temp));
  }

  #[test]
  fn both_copies_damaged_is_damaged() {
    let store = MemorySaves::new();
    store.put(NAME, b"ne");
    store.put(BACKUP, b"ol");
    assert!(matches!(
      recover(&store, NAME, &verify),
      Ok(Recovery::Damaged)
    ));
  }

  #[test]
  fn no_copy_is_missing_and_a_torn_temp_alone_is_not_a_save() {
    let store = MemorySaves::new();
    assert!(matches!(
      recover(&store, NAME, &verify),
      Ok(Recovery::Missing)
    ));
    store.put("slot.ryo-save.7-0.tmp", b"ne");
    assert!(matches!(
      recover(&store, NAME, &verify),
      Ok(Recovery::Missing)
    ));
  }

  #[test]
  fn a_held_file_is_renamed_after_a_short_retry() {
    let store = store_with_old();
    store.deny_rename(3);
    safe_replace(&store, NAME, b"new", &verify).unwrap();
    assert_eq!(found(&store), (b"new".to_vec(), Origin::Current));

    let store = store_with_old();
    store.deny_rename(20);
    let error = safe_replace(&store, NAME, b"new", &verify).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
    assert_eq!(found(&store), (b"old".to_vec(), Origin::Current));
  }

  #[test]
  fn leftover_temps_are_cleared_by_the_next_save() {
    let store = store_with_old();
    store.put("slot.ryo-save.999-0.tmp", b"ne");
    store.put("slot.ryo-save.999-1.tmp", b"new");
    store.put("other.ryo-save.999-0.tmp", b"new");
    safe_replace(&store, NAME, b"new", &verify).unwrap();
    assert_eq!(
      store.names(),
      ["other.ryo-save.999-0.tmp", NAME, BACKUP].map(String::from)
    );
  }

  #[test]
  fn a_failed_write_leaves_no_temp_behind() {
    let store = store_with_old();
    store.crash_write(0, 2);
    assert!(safe_replace(&store, NAME, b"new", &verify).is_err());
    assert_eq!(store.names(), [NAME].map(String::from));
  }
}
