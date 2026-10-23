use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use crossbeam_channel::Sender;

use super::{AssetError, AssetPath};

/// Where asset bytes come from
pub trait AssetSource: Send + Sync + 'static {
  /// The bytes at `path`
  fn read(&self, path: &AssetPath) -> Result<Vec<u8>, AssetError>;

  /// Starts telling `changed` about paths whose bytes changed; a source that
  /// cannot watch returns Ok and never sends
  fn watch(&self, changed: Sender<AssetPath>) -> Result<(), AssetError> {
    let _ = changed;
    Ok(())
  }
}

/// Files under a folder (Task 7 adds watching)
pub struct DiskSource {
  root: PathBuf,
}

impl DiskSource {
  /// Files under `root`
  pub fn new(root: impl Into<PathBuf>) -> Self {
    Self { root: root.into() }
  }

  /// The folder asset paths are relative to
  pub fn root(&self) -> &Path {
    &self.root
  }
}

impl AssetSource for DiskSource {
  #[cfg(not(target_arch = "wasm32"))]
  fn read(&self, path: &AssetPath) -> Result<Vec<u8>, AssetError> {
    std::fs::read(self.root.join(path.as_str())).map_err(|source| match source.kind() {
      std::io::ErrorKind::NotFound => AssetError::NotFound { path: path.clone() },
      _ => AssetError::Io {
        path: path.clone(),
        source,
      },
    })
  }

  #[cfg(target_arch = "wasm32")]
  fn read(&self, path: &AssetPath) -> Result<Vec<u8>, AssetError> {
    Err(AssetError::Io {
      path: path.clone(),
      source: std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "the web build has no file system",
      ),
    })
  }
}

/// Bytes in memory: tests and tools. Clones share their files; `insert` over
/// an existing path tells a watcher it changed
#[derive(Clone, Default)]
pub struct MemorySource {
  files: Arc<Mutex<MemoryFiles>>,
}

#[derive(Default)]
struct MemoryFiles {
  bytes: HashMap<AssetPath, Arc<[u8]>>,
  watcher: Option<Sender<AssetPath>>,
}

impl MemorySource {
  /// A source with no files
  pub fn new() -> Self {
    Self::default()
  }

  /// Puts `bytes` at `path`, replacing what was there
  pub fn insert(&self, path: &str, bytes: impl Into<Vec<u8>>) {
    let path = match AssetPath::parse(path) {
      Ok(path) => path,
      Err(error) => {
        crate::engine_warn!(module: "asset", "MemorySource::insert: {error}");
        return;
      }
    };
    let bytes: Arc<[u8]> = bytes.into().into();
    let mut files = self.files();
    let replaced = files.bytes.insert(path.clone(), bytes).is_some();
    if replaced && let Some(watcher) = &files.watcher {
      // A watcher that has gone away has nothing left to tell
      let _ = watcher.send(path);
    }
  }

  /// Takes the file at `path` away
  pub fn remove(&self, path: &str) {
    if let Ok(path) = AssetPath::parse(path) {
      self.files().bytes.remove(&path);
    }
  }

  fn files(&self) -> MutexGuard<'_, MemoryFiles> {
    self.files.lock().unwrap_or_else(PoisonError::into_inner)
  }
}

impl AssetSource for MemorySource {
  fn read(&self, path: &AssetPath) -> Result<Vec<u8>, AssetError> {
    // Copied outside the lock, so a big file does not hold up other readers
    let bytes = self.files().bytes.get(path).cloned();
    bytes
      .map(|bytes| bytes.to_vec())
      .ok_or_else(|| AssetError::NotFound { path: path.clone() })
  }

  fn watch(&self, changed: Sender<AssetPath>) -> Result<(), AssetError> {
    self.files().watcher = Some(changed);
    Ok(())
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  fn path(text: &str) -> AssetPath {
    match AssetPath::parse(text) {
      Ok(path) => path,
      Err(error) => panic!("{text:?}: {error}"),
    }
  }

  #[test]
  fn a_memory_source_reads_what_was_inserted() {
    let source = MemorySource::new();
    source.insert("a/./b.txt", "hello");
    assert_eq!(source.read(&path("a/b.txt")).ok(), Some(b"hello".to_vec()));
    source.remove("a/b.txt");
    assert!(matches!(
      source.read(&path("a/b.txt")),
      Err(AssetError::NotFound { .. })
    ));
  }

  #[test]
  fn a_memory_source_tells_its_watcher_when_a_file_is_replaced() {
    let source = MemorySource::new();
    let (changed, seen) = crossbeam_channel::unbounded();
    assert!(source.watch(changed).is_ok());
    source.insert("a.txt", "one");
    assert!(seen.try_recv().is_err(), "a new file is not a change");
    source.clone().insert("a.txt", "two");
    assert_eq!(seen.try_recv().ok(), Some(path("a.txt")));
  }

  #[cfg(not(target_arch = "wasm32"))]
  #[test]
  fn a_disk_source_reads_under_its_root() {
    let dir = tempfile::tempdir().map_err(|e| e.to_string());
    let Ok(dir) = dir else {
      panic!("a temp dir: {dir:?}");
    };
    let written = std::fs::create_dir(dir.path().join("ui"))
      .and_then(|()| std::fs::write(dir.path().join("ui/a.txt"), "hello"));
    assert!(written.is_ok());
    let source = DiskSource::new(dir.path());
    assert_eq!(source.root(), dir.path());
    assert_eq!(source.read(&path("ui/a.txt")).ok(), Some(b"hello".to_vec()));
    assert!(matches!(
      source.read(&path("ui/b.txt")),
      Err(AssetError::NotFound { .. })
    ));
    assert!(matches!(
      source.read(&path("ui")),
      Err(AssetError::Io { .. })
    ));
  }
}
