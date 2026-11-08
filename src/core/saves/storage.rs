//! The storage seam: what the saves need from a place to keep files.

use std::io;

#[cfg(not(target_arch = "wasm32"))]
use crossbeam_channel::RecvTimeoutError;
use crossbeam_channel::{Receiver, TryRecvError};
use web_time::Instant;

use super::replace;

/// Checks a copy of a file: `Some(recency)` for a whole, valid copy, where a
/// larger recency is a newer save, and `None` otherwise
pub(crate) type Verify<'a> = &'a dyn Fn(&[u8]) -> Option<u64>;

/// The file operations a folder-like store offers. Every `FileOps` backend is
/// a [`Storage`] through [`replace::safe_replace`] and [`replace::recover`].
pub(crate) trait FileOps: Send + Sync {
  /// The file's bytes, or `None` when there is no such file
  fn read(&self, name: &str) -> io::Result<Option<Vec<u8>>>;
  /// Writes the file whole, replacing any file of that name, and flushes it to
  /// the disk before returning
  fn write_synced(&self, name: &str, bytes: &[u8]) -> io::Result<()>;
  /// Renames `from` to `to` in one atomic step, replacing `to`
  fn rename(&self, from: &str, to: &str) -> io::Result<()>;
  /// Removes the file; a missing file is not an error
  fn remove(&self, name: &str) -> io::Result<()>;
  /// Every file name in the store
  fn names(&self) -> io::Result<Vec<String>>;
  /// Makes the store's renames and removals durable
  fn sync_dir(&self) -> io::Result<()>;
}

/// The outcome of a write, a removal or a rename: finished, or still running
/// in a backend that commits asynchronously (the browser)
#[derive(Debug)]
#[must_use = "a write's completion carries its result"]
pub(crate) enum Completion {
  /// The work has finished
  Done(io::Result<()>),
  /// The work finishes later and sends its result here
  Pending(Receiver<io::Result<()>>),
}

impl Completion {
  /// The result once the work has finished, else the completion back to poll
  /// again
  pub(crate) fn poll(self) -> Result<io::Result<()>, Completion> {
    match self {
      Completion::Done(result) => Ok(result),
      Completion::Pending(receiver) => match receiver.try_recv() {
        Ok(result) => Ok(result),
        Err(TryRecvError::Empty) => Err(Completion::Pending(receiver)),
        Err(TryRecvError::Disconnected) => Ok(Err(dropped())),
      },
    }
  }

  /// The result once the work has finished, waiting for it until
  /// `deadline`, else the completion back. The web cannot block, so there it
  /// polls once
  pub(crate) fn wait_until(self, deadline: Instant) -> Result<io::Result<()>, Completion> {
    #[cfg(target_arch = "wasm32")]
    {
      let _ = deadline;
      self.poll()
    }
    #[cfg(not(target_arch = "wasm32"))]
    match self {
      Completion::Done(result) => Ok(result),
      Completion::Pending(receiver) => {
        match receiver.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
          Ok(result) => Ok(result),
          Err(RecvTimeoutError::Timeout) => Err(Completion::Pending(receiver)),
          Err(RecvTimeoutError::Disconnected) => Ok(Err(dropped())),
        }
      }
    }
  }

  /// Blocks until the work has finished
  #[cfg(test)]
  pub(crate) fn wait(self) -> io::Result<()> {
    match self {
      Completion::Done(result) => result,
      Completion::Pending(receiver) => receiver.recv().unwrap_or_else(|_| Err(dropped())),
    }
  }
}

fn dropped() -> io::Error {
  io::Error::other("the storage dropped the work before it finished")
}

/// What [`Storage::recover`] found for a file
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Recovery {
  /// There is no copy of the file
  Missing,
  /// The file or its backup is there, but no copy verifies
  Damaged,
  /// A whole, verified copy
  Found {
    /// The copy's bytes
    bytes: Vec<u8>,
    /// Which copy it is
    from: Origin,
  },
}

/// Which copy of a file [`Storage::recover`] found
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Origin {
  /// The file itself
  Current,
  /// A temporary file a replace wrote but did not finish renaming
  Temp,
  /// The backup the last replace made of the previous copy
  Backup,
}

/// Where the saves keep their files: a native folder, the browser's store or
/// memory. Files are named, flat, and replaced whole.
pub(crate) trait Storage: Send + Sync + 'static {
  /// The file's bytes as they are, or `None` when there is no such file
  fn read(&self, name: &str) -> io::Result<Option<Vec<u8>>>;
  /// Every file name in the store, backups and temporary files included
  fn names(&self) -> io::Result<Vec<String>>;
  /// Replaces the file so that a crash at any point leaves a whole copy of
  /// either the old or the new bytes for [`Storage::recover`] to find
  fn replace(&self, name: &str, bytes: &[u8], verify: Verify<'_>) -> Completion;
  /// The newest whole copy of the file: the file itself if it verifies, else
  /// the newest verified temporary file, else the backup
  fn recover(&self, name: &str, verify: Verify<'_>) -> io::Result<Recovery>;
  /// Removes the file, its backup and its temporary files
  fn remove_all(&self, name: &str) -> Completion;
  /// Renames the file to `to`, replacing `to`
  fn set_aside(&self, name: &str, to: &str) -> Completion;
}

impl<T: FileOps + 'static> Storage for T {
  fn read(&self, name: &str) -> io::Result<Option<Vec<u8>>> {
    FileOps::read(self, name)
  }

  fn names(&self) -> io::Result<Vec<String>> {
    FileOps::names(self)
  }

  fn replace(&self, name: &str, bytes: &[u8], verify: Verify<'_>) -> Completion {
    Completion::Done(replace::safe_replace(self, name, bytes, verify))
  }

  fn recover(&self, name: &str, verify: Verify<'_>) -> io::Result<Recovery> {
    replace::recover(self, name, verify)
  }

  fn remove_all(&self, name: &str) -> Completion {
    Completion::Done(replace::remove_all(self, name))
  }

  fn set_aside(&self, name: &str, to: &str) -> Completion {
    Completion::Done(replace::set_aside(self, name, to))
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::saves::memory::MemorySaves;

  #[test]
  fn a_pending_completion_resolves_when_its_work_lands() {
    let (send, receive) = crossbeam_channel::bounded(1);
    let pending = Completion::Pending(receive);
    let pending = match pending.poll() {
      Err(still) => still,
      Ok(result) => panic!("resolved before its work landed: {result:?}"),
    };
    send.send(Ok(())).unwrap();
    assert!(matches!(pending.poll(), Ok(Ok(()))));

    let (send, receive) = crossbeam_channel::bounded(1);
    send.send(Err(io::Error::other("full"))).unwrap();
    assert!(Completion::Pending(receive).wait().is_err());
    assert!(Completion::Done(Ok(())).wait().is_ok());
  }

  #[test]
  fn waiting_for_a_pending_completion_stops_at_its_deadline() {
    let (send, receive) = crossbeam_channel::bounded(1);
    let soon = Instant::now() + std::time::Duration::from_millis(10);
    let pending = match Completion::Pending(receive).wait_until(soon) {
      Err(still) => still,
      Ok(result) => panic!("resolved before its work landed: {result:?}"),
    };
    send.send(Ok(())).unwrap();
    assert!(matches!(pending.wait_until(Instant::now()), Ok(Ok(()))));
    assert!(matches!(
      Completion::Done(Ok(())).wait_until(Instant::now()),
      Ok(Ok(()))
    ));
  }

  #[test]
  fn a_pending_completion_whose_work_is_dropped_fails() {
    let (send, receive) = crossbeam_channel::bounded::<io::Result<()>>(1);
    drop(send);
    assert!(matches!(
      Completion::Pending(receive.clone()).poll(),
      Ok(Err(_))
    ));
    assert!(Completion::Pending(receive).wait().is_err());
  }

  #[test]
  fn removing_all_takes_the_file_its_backup_and_its_temps() {
    let store = MemorySaves::new();
    for name in [
      "slot.ryo-save",
      "slot.ryo-save.bak",
      "slot.ryo-save.4-0.tmp",
      "other.ryo-save",
    ] {
      store.put(name, b"x");
    }
    store.remove_all("slot.ryo-save").wait().unwrap();
    assert_eq!(store.names(), ["other.ryo-save"].map(String::from));
    assert!(store.remove_all("slot.ryo-save").wait().is_ok());
  }

  #[test]
  fn setting_aside_replaces_the_target() {
    let store = MemorySaves::new();
    store.put("system.json", b"damaged");
    store.put("system.json.damaged", b"older");
    store
      .set_aside("system.json", "system.json.damaged")
      .wait()
      .unwrap();
    assert_eq!(store.get("system.json"), None);
    assert_eq!(
      store.get("system.json.damaged").as_deref(),
      Some(&b"damaged"[..])
    );
  }
}
