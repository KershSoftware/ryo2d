//! Saves in a folder on the native file system.

use std::fs::{self, File};
use std::io::{self, Write};
use std::path::PathBuf;

use super::storage::FileOps;

/// A folder of save files
#[derive(Debug)]
pub(crate) struct FileStorage {
  dir: PathBuf,
}

impl FileStorage {
  /// Storage in `dir`, creating the folder and its parents if need be
  pub(crate) fn new(dir: PathBuf) -> io::Result<FileStorage> {
    fs::create_dir_all(&dir)?;
    Ok(FileStorage { dir })
  }

  fn path(&self, name: &str) -> PathBuf {
    self.dir.join(name)
  }
}

impl FileOps for FileStorage {
  fn read(&self, name: &str) -> io::Result<Option<Vec<u8>>> {
    match fs::read(self.path(name)) {
      Ok(bytes) => Ok(Some(bytes)),
      Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
      Err(error) => Err(error),
    }
  }

  fn write_synced(&self, name: &str, bytes: &[u8]) -> io::Result<()> {
    let mut file = File::create(self.path(name))?;
    file.write_all(bytes)?;
    file.sync_all()
  }

  fn rename(&self, from: &str, to: &str) -> io::Result<()> {
    fs::rename(self.path(from), self.path(to))
  }

  fn remove(&self, name: &str) -> io::Result<()> {
    match fs::remove_file(self.path(name)) {
      Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
      result => result,
    }
  }

  fn names(&self) -> io::Result<Vec<String>> {
    let mut names = Vec::new();
    for entry in fs::read_dir(&self.dir)? {
      let entry = entry?;
      // An entry another writer removed mid-listing is skipped
      if !entry.file_type().is_ok_and(|kind| kind.is_file()) {
        continue;
      }
      // A name that is not UTF-8 is no file the saves wrote
      if let Ok(name) = entry.file_name().into_string() {
        names.push(name);
      }
    }
    names.sort();
    Ok(names)
  }

  fn sync_dir(&self) -> io::Result<()> {
    // std cannot open a folder to sync it on Windows
    #[cfg(unix)]
    File::open(&self.dir)?.sync_all()?;
    Ok(())
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::saves::storage::{Origin, Recovery, Storage};

  const NAME: &str = "slot.ryo-save";

  fn verify(bytes: &[u8]) -> Option<u64> {
    match bytes {
      b"old" => Some(1),
      b"new" => Some(2),
      _ => None,
    }
  }

  #[test]
  fn a_replace_survives_on_a_real_folder() {
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join("saves").join("game");
    let storage = FileStorage::new(dir.clone()).unwrap();
    assert!(dir.is_dir());

    storage.replace(NAME, b"old", &verify).wait().unwrap();
    assert_eq!(
      Storage::read(&storage, NAME).unwrap().as_deref(),
      Some(&b"old"[..])
    );
    assert!(!dir.join("slot.ryo-save.bak").exists());

    storage.replace(NAME, b"new", &verify).wait().unwrap();
    assert_eq!(std::fs::read(dir.join(NAME)).unwrap(), b"new");
    assert_eq!(
      std::fs::read(dir.join("slot.ryo-save.bak")).unwrap(),
      b"old"
    );
    assert!(matches!(
      storage.recover(NAME, &verify).unwrap(),
      Recovery::Found { ref bytes, from: Origin::Current } if bytes == b"new"
    ));
    assert_eq!(
      Storage::names(&storage).unwrap(),
      [NAME, "slot.ryo-save.bak"].map(String::from)
    );
    FileOps::sync_dir(&storage).unwrap();
  }

  #[test]
  fn two_writers_on_one_folder_leave_a_verified_save() {
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().to_path_buf();
    let writers: Vec<_> = [b"old", b"new"]
      .into_iter()
      .map(|bytes| {
        let dir = dir.clone();
        std::thread::spawn(move || {
          let storage = FileStorage::new(dir).unwrap();
          for _ in 0..100 {
            // A save may fail when the other writer moves a file under it;
            // what matters is the folder each leaves behind
            let _ = storage.replace(NAME, bytes, &verify).wait();
          }
        })
      })
      .collect();
    for writer in writers {
      writer.join().unwrap();
    }

    let storage = FileStorage::new(dir).unwrap();
    assert!(matches!(
      storage.recover(NAME, &verify).unwrap(),
      Recovery::Found { .. }
    ));
    let names = Storage::names(&storage).unwrap();
    assert!(
      names.iter().all(|name| !name.ends_with(".tmp")),
      "temps left behind: {names:?}"
    );
  }
}
