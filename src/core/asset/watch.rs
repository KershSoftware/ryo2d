//! Watching the asset folder (native builds): a saved file reloads once its
//! writes settle.
//!
//! notify's callback maps each changed file to its asset path and sends it,
//! with the time, to the debounce thread. That thread tells the server about
//! a path once [`SETTLE`] has passed since the path's last event, so an
//! editor's save in several writes, or a write to a temp file renamed over
//! the asset, reloads once, with the last bytes. A removed file is not
//! reported: its asset keeps its last value.

use std::path::{Path, PathBuf};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, RecvTimeoutError, Sender, unbounded};
use notify::event::{ModifyKind, RenameMode};
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};

use super::AssetPath;

/// How long a file must go unwritten before it reloads
const SETTLE: Duration = Duration::from_millis(100);

/// How often the debounce thread looks for settled files while any wait
const CHECK_EVERY: Duration = Duration::from_millis(25);

/// A watch on the asset folder: notify's watcher and the debounce thread.
/// Both stop when it drops
pub(crate) struct Watching {
  _watcher: RecommendedWatcher,
  notes: Sender<Note>,
  debounce: Option<JoinHandle<()>>,
}

/// What the debounce thread is told
enum Note {
  /// A file changed at this time
  Changed(AssetPath, Instant),
  /// The watch is over
  Stop,
}

/// The asset root as the file system names it, and as it was given: an
/// event's path starts with one of them
struct Roots {
  canonical: PathBuf,
  given: PathBuf,
}

impl Watching {
  /// Watches `root` and every folder under it, telling `changed` about each
  /// file once its writes settle. None when the folder is missing (noted at
  /// debug level: a game with no assets has none) or, with a warning, when
  /// it cannot be watched
  pub(crate) fn start(root: &Path, changed: Sender<AssetPath>) -> Option<Watching> {
    let roots = Roots {
      // notify reports the names the file system uses: macOS's /var is /private/var
      canonical: std::fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf()),
      given: root.to_path_buf(),
    };
    if !roots.canonical.is_dir() {
      log::debug!(
        "the asset folder {} is missing: changed files will not reload",
        root.display()
      );
      return None;
    }
    let watched = roots.canonical.clone();
    let (notes, noted) = unbounded();
    let to_debounce = notes.clone();
    // notify calls back on a thread of its own: in this thread's scope too
    let scope = crate::core::logging::current_scope();
    let watcher = notify::recommended_watcher(move |result| {
      crate::core::logging::in_scope(scope, || note_changes(result, &roots, &to_debounce));
    })
    .and_then(|mut watcher| {
      watcher
        .watch(&watched, RecursiveMode::Recursive)
        .map(|()| watcher)
    });
    let watcher = match watcher {
      Ok(watcher) => watcher,
      Err(error) => {
        log::warn!(
          "cannot watch the asset folder {}: {error}; changed files will not reload",
          root.display()
        );
        return None;
      }
    };
    let named = root.display().to_string();
    let thread = std::thread::Builder::new()
      .name("ryo2d-asset-watch".to_owned())
      .spawn(crate::core::logging::scoped(move || {
        log::debug!("watching the asset folder {named} for changes");
        debounce(noted, changed);
      }));
    match thread {
      Ok(thread) => Some(Watching {
        _watcher: watcher,
        notes,
        debounce: Some(thread),
      }),
      Err(error) => {
        log::warn!(
          "could not start the asset folder's watch: {error}; changed files will not reload"
        );
        None
      }
    }
  }
}

impl Drop for Watching {
  fn drop(&mut self) {
    // The thread may already have gone, if the server did
    let _ = self.notes.send(Note::Stop);
    if let Some(thread) = self.debounce.take() {
      let _ = thread.join();
    }
  }
}

/// Whether an event of this kind can change a file's bytes: a create, a
/// modify, or a rename to the path. A removed file keeps its last value, and
/// a metadata change (a `touch`) leaves the bytes as they were
fn is_change(kind: &EventKind) -> bool {
  match kind {
    EventKind::Modify(ModifyKind::Name(RenameMode::From) | ModifyKind::Metadata(_)) => false,
    EventKind::Create(_) | EventKind::Modify(_) => true,
    _ => false,
  }
}

/// The asset path of a file an event names, or None when it is outside the
/// root or an editor's temp file (a name starting with `.` or ending in `~`)
fn asset_path(path: &Path, roots: &Roots) -> Option<AssetPath> {
  let relative = path
    .strip_prefix(&roots.canonical)
    .or_else(|_| path.strip_prefix(&roots.given))
    .ok()?;
  let name = relative.file_name()?.to_str()?;
  if name.starts_with('.') || name.ends_with('~') {
    return None;
  }
  AssetPath::parse(relative.to_str()?).ok()
}

/// The callback's side: sends each changed file in `result` on to the
/// debounce thread
fn note_changes(result: notify::Result<Event>, roots: &Roots, notes: &Sender<Note>) {
  let event = match result {
    Ok(event) => event,
    Err(error) => {
      log::warn!("watching the asset folder: {error}");
      return;
    }
  };
  if !is_change(&event.kind) {
    return;
  }
  let at = Instant::now();
  for path in &event.paths {
    // A folder is no file, and neither is a name already gone again: the
    // old side of a rename, or a temp file saved and moved
    if let Some(asset) = asset_path(path, roots)
      && path.is_file()
    {
      // Nothing is listening once the watch has stopped
      let _ = notes.send(Note::Changed(asset, at));
    }
  }
}

/// The debounce thread: tells `changed` about each path once its writes
/// settle, until the watch stops
fn debounce(notes: Receiver<Note>, changed: Sender<AssetPath>) {
  let mut pending = Pending::default();
  loop {
    let note = if pending.is_empty() {
      // Nothing waits to settle: sleep until a file changes
      notes.recv().map_err(|_| RecvTimeoutError::Disconnected)
    } else {
      notes.recv_timeout(CHECK_EVERY)
    };
    match note {
      Ok(Note::Changed(path, at)) => pending.record(path, at),
      Ok(Note::Stop) | Err(RecvTimeoutError::Disconnected) => return,
      Err(RecvTimeoutError::Timeout) => {}
    }
    for path in pending.settled(Instant::now()) {
      // The server has gone: nothing is left to tell
      if changed.send(path).is_err() {
        return;
      }
    }
  }
}

/// Changed files waiting for their writes to settle, in the order of their
/// last event, so the settled ones lead
#[derive(Default)]
struct Pending(Vec<(AssetPath, Instant)>);

impl Pending {
  fn is_empty(&self) -> bool {
    self.0.is_empty()
  }

  /// Notes a change to `path` at `at`, behind every other
  fn record(&mut self, path: AssetPath, at: Instant) {
    self.0.retain(|(waiting, _)| *waiting != path);
    self.0.push((path, at));
  }

  /// Takes the paths unwritten for [`SETTLE`] by `now`, in order
  fn settled(&mut self, now: Instant) -> impl Iterator<Item = AssetPath> + '_ {
    let settled = self
      .0
      .iter()
      .take_while(|(_, at)| now.saturating_duration_since(*at) >= SETTLE)
      .count();
    self.0.drain(..settled).map(|(path, _)| path)
  }
}

#[cfg(test)]
mod tests {
  use crossbeam_channel::{RecvTimeoutError, TryRecvError, unbounded};
  use notify::event::{
    AccessKind, CreateKind, DataChange, MetadataKind, ModifyKind, RemoveKind, RenameMode,
  };

  use super::*;
  use crate::core::asset::{
    Asset, AssetChanges, AssetError, AssetEvent, AssetLoader, AssetServer, AssetSource, Assets,
    DiskSource, Handle, LoadContext, insert_registry, pump,
  };
  use crate::core::ecs::kernel::{MessageCursor, Messages, World};
  use crate::core::graphics::Image;
  use std::sync::{Mutex, PoisonError};

  /// Tests that turn the watcher's level to debug take turns, so one's
  /// guard cannot put the level back under the other
  static DEBUG_LEVEL: Mutex<()> = Mutex::new(());

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

  /// How long a test waits for the file system and the workers
  const PATIENCE: Duration = Duration::from_secs(3);

  fn path(text: &str) -> AssetPath {
    match AssetPath::parse(text) {
      Ok(path) => path,
      Err(error) => panic!("{text:?}: {error}"),
    }
  }

  fn temp_dir() -> tempfile::TempDir {
    match tempfile::tempdir() {
      Ok(dir) => dir,
      Err(error) => panic!("a temp dir: {error}"),
    }
  }

  fn write(dir: &Path, name: &str, bytes: impl AsRef<[u8]>) {
    if let Err(error) = std::fs::write(dir.join(name), bytes) {
      panic!("writing {name}: {error}");
    }
  }

  /// A `Text` server over a temp folder it watches, in a world made as
  /// every world is (so it loads images too). Other asset types register on
  /// `server` and `world` as `Text` does
  struct Folder {
    dir: tempfile::TempDir,
    world: World,
    server: AssetServer,
  }

  impl Folder {
    /// A new temp folder holding `files`, watched
    fn new(files: &[(&str, &str)]) -> Folder {
      let dir = temp_dir();
      for (name, text) in files {
        write(dir.path(), name, text);
      }
      let root = dir.path().to_path_buf();
      Folder::watched_at(dir, &root)
    }

    /// `dir`, watched through `root`: the folder itself, or a link to it
    fn watched_at(dir: tempfile::TempDir, root: &Path) -> Folder {
      let mut world = World::new();
      let server = AssetServer::new(DiskSource::new(root));
      server.register_asset::<Text>(&mut world);
      server.register_loader(TextLoader);
      insert_registry(&mut world, server.clone());
      server.start_watching();
      Folder { dir, world, server }
    }

    fn write(&self, name: &str, bytes: impl AsRef<[u8]>) {
      write(self.dir.path(), name, bytes);
    }

    fn load_now<T: Asset>(&mut self, path: &str) -> Handle<T> {
      match self.server.load_now::<T>(&mut self.world, path) {
        Ok(handle) => handle,
        Err(error) => panic!("{path}: {}", error.report()),
      }
    }

    fn text(&self, handle: &Handle<Text>) -> Option<&str> {
      self
        .world
        .resource::<Assets<Text>>()
        .get(handle)
        .map(|text| text.0.as_str())
    }

    /// A cursor over the `T` events sent from now on
    fn cursor<T: Asset>(&self) -> MessageCursor<AssetEvent<T>> {
      self
        .world
        .resource::<Messages<AssetEvent<T>>>()
        .get_cursor_current()
    }

    /// Pumps until `done` holds, or fails with `waiting_for`
    fn pump_until(&mut self, waiting_for: &str, done: impl Fn(&Folder) -> bool) {
      let start = Instant::now();
      loop {
        pump(&mut self.world);
        if done(self) {
          return;
        }
        assert!(start.elapsed() < PATIENCE, "timed out: {waiting_for}");
        std::thread::sleep(Duration::from_millis(2));
      }
    }

    /// Pumps until an event `done` accepts is sent; gives back every event
    /// sent meanwhile
    fn pump_until_sent<T: Asset>(
      &mut self,
      cursor: &mut MessageCursor<AssetEvent<T>>,
      done: impl Fn(&AssetEvent<T>) -> bool,
    ) -> Vec<AssetEvent<T>> {
      let start = Instant::now();
      let mut sent = Vec::new();
      loop {
        pump(&mut self.world);
        sent.extend(
          cursor
            .read(self.world.resource::<Messages<AssetEvent<T>>>())
            .cloned(),
        );
        if sent.iter().any(&done) {
          return sent;
        }
        assert!(start.elapsed() < PATIENCE, "timed out: {sent:?}");
        std::thread::sleep(Duration::from_millis(2));
      }
    }
  }

  fn modified<T: Asset>(handle: &Handle<T>) -> impl Fn(&AssetEvent<T>) -> bool + use<T> {
    let id = handle.id();
    move |event| matches!(event, AssetEvent::Modified { id: changed } if *changed == id)
  }

  fn about(handle: &Handle<Text>) -> impl Fn(&AssetEvent<Text>) -> bool + use<> {
    let id = handle.id();
    move |event| match event {
      AssetEvent::Added { id: about }
      | AssetEvent::Modified { id: about }
      | AssetEvent::Removed { id: about }
      | AssetEvent::Failed { id: about, .. }
      | AssetEvent::ReloadFailed { id: about, .. } => *about == id,
    }
  }

  #[test]
  fn a_saved_file_reloads_under_the_same_handle() {
    let mut folder = Folder::new(&[("a.txt", "one")]);
    let a = folder.load_now("a.txt");
    let id = a.id();
    let mut cursor = folder.cursor();
    folder.write("a.txt", "two");
    let sent = folder.pump_until_sent(&mut cursor, modified(&a));
    assert_eq!(folder.text(&a), Some("two"), "{sent:?}");
    assert_eq!(a.id(), id);
    assert!(folder.server.state(&a).is_loaded());
  }

  #[test]
  fn two_writes_in_the_debounce_reload_once_with_the_last() {
    let mut folder = Folder::new(&[("a.txt", "one"), ("later.txt", "wait")]);
    let a = folder.load_now("a.txt");
    let later = folder.load_now("later.txt");
    let mut cursor = folder.cursor();
    folder.write("a.txt", "a");
    // Inside the debounce, as an editor's truncate and write are
    std::thread::sleep(Duration::from_millis(30));
    folder.write("a.txt", "b");
    let burst = folder.pump_until_sent(&mut cursor, modified(&a));
    assert_eq!(
      folder.text(&a),
      Some("b"),
      "the first reload has the last write: {burst:?}"
    );
    // A file saved after the burst reloads after anything the burst caused
    folder.write("later.txt", "go");
    let after = folder.pump_until_sent(&mut cursor, modified(&later));
    let of_a = modified(&a);
    let reloads = burst.iter().chain(&after).filter(|&event| of_a(event));
    assert_eq!(reloads.count(), 1, "{burst:?} then {after:?}");
    assert_eq!(folder.text(&a), Some("b"));
  }

  #[test]
  fn a_half_written_text_keeps_the_last_good_one() {
    let mut folder = Folder::new(&[("a.txt", "one")]);
    let a = folder.load_now("a.txt");
    let id = a.id();
    let mut cursor = folder.cursor();
    // Cut off inside a two-byte character
    folder.write("a.txt", [b't', b'w', 0xC3]);
    let sent = folder.pump_until_sent(
      &mut cursor,
      |event| matches!(event, AssetEvent::ReloadFailed { id: failed, .. } if *failed == id),
    );
    assert_eq!(folder.text(&a), Some("one"), "{sent:?}");
    assert!(folder.server.state(&a).is_loaded());
    assert!(!sent.iter().any(modified(&a)), "{sent:?}");
    // The rest of the write lands
    folder.write("a.txt", "twø");
    folder.pump_until_sent(&mut cursor, modified(&a));
    assert_eq!(folder.text(&a), Some("twø"));
  }

  /// `width`x`height` pixels of one colour, as a PNG file
  fn png(width: u32, height: u32, colour: [u8; 4]) -> Vec<u8> {
    let pixels = image::RgbaImage::from_pixel(width, height, image::Rgba(colour));
    let mut file = std::io::Cursor::new(Vec::new());
    if let Err(error) = pixels.write_to(&mut file, image::ImageFormat::Png) {
      panic!("encoding a PNG: {error}");
    }
    file.into_inner()
  }

  #[test]
  fn a_half_written_image_keeps_the_last_good_one() {
    let red = [200, 0, 0, 255];
    let blue = [0, 0, 200, 255];
    let dir = temp_dir();
    write(dir.path(), "a.png", png(32, 32, red));
    let root = dir.path().to_path_buf();
    let mut folder = Folder::watched_at(dir, &root);
    let a = folder.load_now::<Image>("a.png");
    let id = a.id();
    let first_pixel = |folder: &Folder| {
      let images = folder.world.resource::<Assets<Image>>();
      images
        .get(&a)
        .and_then(|image| image.data().first_chunk::<4>().copied())
    };
    let mut cursor = folder.cursor::<Image>();
    // An editor's save cut off halfway, with new pixels
    let blue_png = png(32, 32, blue);
    folder.write("a.png", &blue_png[..blue_png.len() / 2]);
    let sent = folder.pump_until_sent(
      &mut cursor,
      |event| matches!(event, AssetEvent::ReloadFailed { id: failed, .. } if *failed == id),
    );
    assert_eq!(first_pixel(&folder), Some(red), "{sent:?}");
    assert!(folder.server.state(&a).is_loaded());
    assert!(!sent.iter().any(modified(&a)), "{sent:?}");
    // The rest of the write lands
    folder.write("a.png", &blue_png);
    folder.pump_until_sent(&mut cursor, modified(&a));
    assert_eq!(first_pixel(&folder), Some(blue));
  }

  #[cfg(unix)]
  #[test]
  fn a_change_under_a_symlinked_root_reloads() {
    let dir = temp_dir();
    write(dir.path(), "a.txt", "one");
    let links = temp_dir();
    let root = links.path().join("assets");
    if let Err(error) = std::os::unix::fs::symlink(dir.path(), &root) {
      panic!("a link to the folder: {error}");
    }
    let mut folder = Folder::watched_at(dir, &root);
    let a = folder.load_now("a.txt");
    let mut cursor = folder.cursor();
    write(&root, "a.txt", "two");
    let sent = folder.pump_until_sent(&mut cursor, modified(&a));
    assert_eq!(folder.text(&a), Some("two"), "{sent:?}");
  }

  #[test]
  fn a_deleted_file_keeps_its_last_value() {
    let mut folder = Folder::new(&[("a.txt", "one"), ("later.txt", "wait")]);
    let a = folder.load_now("a.txt");
    let later = folder.load_now("later.txt");
    let mut cursor = folder.cursor();
    if let Err(error) = std::fs::remove_file(folder.dir.path().join("a.txt")) {
      panic!("removing a.txt: {error}");
    }
    // Saved after the removal, so it reloads after anything the removal caused
    folder.write("later.txt", "go");
    let sent = folder.pump_until_sent(&mut cursor, modified(&later));
    assert_eq!(folder.text(&a), Some("one"));
    assert!(folder.server.state(&a).is_loaded());
    assert!(!sent.iter().any(about(&a)), "{sent:?}");
  }

  #[test]
  fn editor_temp_files_are_ignored() {
    // Checked at the source: the server would reload nothing for these
    // paths anyway, as no asset is loaded from them
    let dir = temp_dir();
    let source = DiskSource::new(dir.path());
    let (changes, changed) = unbounded();
    assert!(source.watch(AssetChanges::new(changes)).is_ok());
    write(dir.path(), ".a.txt.swp", "swap");
    write(dir.path(), "a.txt~", "backup");
    write(dir.path(), "a.txt", "saved");
    // Paths go out in the order of their last write, so a temp file let
    // through would come before a.txt
    let start = Instant::now();
    let mut sent = Vec::new();
    while !sent.contains(&path("a.txt")) {
      match changed.recv_timeout(Duration::from_millis(10)) {
        Ok(path) => sent.push(path),
        Err(RecvTimeoutError::Timeout) => {}
        Err(RecvTimeoutError::Disconnected) => panic!("the watch stopped: {sent:?}"),
      }
      assert!(start.elapsed() < PATIENCE, "timed out: {sent:?}");
    }
    assert_eq!(sent, [path("a.txt")]);
  }

  #[test]
  fn a_missing_file_that_appears_loads() {
    let mut folder = Folder::new(&[]);
    let b = folder.server.load::<Text>("b.txt");
    folder.pump_until("b.txt to fail", |folder| {
      folder.server.state(&b).is_failed()
    });
    let mut cursor = folder.cursor();
    folder.write("b.txt", "here");
    let id = b.id();
    let sent = folder.pump_until_sent(
      &mut cursor,
      |event| matches!(event, AssetEvent::Added { id: added } if *added == id),
    );
    assert_eq!(folder.text(&b), Some("here"), "{sent:?}");
    assert!(folder.server.state(&b).is_loaded());
  }

  #[test]
  fn a_disk_source_watches_only_when_asked() {
    let dir = temp_dir();
    let source = DiskSource::new(dir.path());
    assert!(!source.is_watching(), "a new source watches nothing");
    let (changes, _changed) = unbounded();
    assert!(source.watch(AssetChanges::new(changes)).is_ok());
    assert!(source.is_watching());
  }

  #[test]
  fn a_missing_root_is_not_watched() {
    let _turn = DEBUG_LEVEL.lock().unwrap_or_else(PoisonError::into_inner);
    let log = crate::core::testing::capture_log();
    let _debug =
      crate::core::logging::level_for_test("ryo2d::core::asset::watch", log::LevelFilter::Debug);
    let dir = temp_dir();
    let missing = dir.path().join("missing");
    let source = DiskSource::new(&missing);
    let (changes, changed) = unbounded();
    assert!(
      source.watch(AssetChanges::new(changes)).is_ok(),
      "a missing folder fails nothing"
    );
    assert!(!source.is_watching());
    assert_eq!(
      changed.try_recv(),
      Err(TryRecvError::Disconnected),
      "nothing will ever be sent"
    );
    // A game with no assets has no folder: that is normal, and no warning
    let named = missing.display().to_string();
    let said = |level: log::Level| {
      log
        .records()
        .iter()
        .filter(|r| r.level == level && r.message.contains(&named))
        .count()
    };
    assert_eq!(said(log::Level::Warn), 0);
    assert_eq!(said(log::Level::Debug), 1);
  }

  #[test]
  fn the_file_watcher_logs_into_its_engine_s_scope() {
    let _turn = DEBUG_LEVEL.lock().unwrap_or_else(PoisonError::into_inner);
    // Opened before the server starts watching, so its threads join it
    let log = crate::core::testing::capture_log();
    let _debug =
      crate::core::logging::level_for_test("ryo2d::core::asset::watch", log::LevelFilter::Debug);
    let folder = Folder::new(&[("a.txt", "one")]);
    let named = folder.dir.path().display().to_string();
    let start = Instant::now();
    while log.containing(&named) == 0 {
      assert!(
        start.elapsed() < PATIENCE,
        "the watch never said it started"
      );
      std::thread::sleep(Duration::from_millis(2));
    }
    let said = log.from_target("ryo2d::core::asset::watch");
    assert!(
      said
        .iter()
        .any(|r| r.message.contains("watching the asset folder")),
      "{said:?}"
    );
    assert!(
      said
        .iter()
        .all(|r| r.scope == log.scope() && r.frame.is_none())
    );
  }

  #[test]
  fn watching_stops_when_the_source_drops() {
    let dir = temp_dir();
    let source = DiskSource::new(dir.path());
    let (changes, changed) = unbounded();
    assert!(source.watch(AssetChanges::new(changes)).is_ok());
    assert!(source.is_watching());
    drop(source);
    // The debounce thread held the only sender: it has ended
    assert_eq!(changed.try_recv(), Err(TryRecvError::Disconnected));
  }

  #[test]
  fn creates_modifies_and_renames_to_are_changes() {
    assert!(is_change(&EventKind::Create(CreateKind::File)));
    assert!(is_change(&EventKind::Modify(ModifyKind::Data(
      DataChange::Content
    ))));
    assert!(is_change(&EventKind::Modify(ModifyKind::Name(
      RenameMode::To
    ))));
    // FSEvents does not say which side of a rename a path is: the old name
    // is gone by then, and is skipped as no file
    assert!(is_change(&EventKind::Modify(ModifyKind::Name(
      RenameMode::Any
    ))));
    assert!(!is_change(&EventKind::Modify(ModifyKind::Name(
      RenameMode::From
    ))));
    assert!(!is_change(&EventKind::Remove(RemoveKind::File)));
    assert!(!is_change(&EventKind::Access(AccessKind::Read)));
  }

  #[test]
  fn metadata_changes_are_not_changes() {
    // A `touch`, a permission or an attribute change leaves the bytes as
    // they were; a write that does change them also sends a data event
    for kind in [
      MetadataKind::Any,
      MetadataKind::AccessTime,
      MetadataKind::WriteTime,
      MetadataKind::Permissions,
      MetadataKind::Ownership,
      MetadataKind::Extended,
      MetadataKind::Other,
    ] {
      assert!(
        !is_change(&EventKind::Modify(ModifyKind::Metadata(kind))),
        "{kind:?}"
      );
    }
    assert!(is_change(&EventKind::Modify(ModifyKind::Any)));
  }

  #[test]
  fn event_paths_become_asset_paths_under_either_root() {
    let roots = Roots {
      canonical: PathBuf::from("/private/var/game/assets"),
      given: PathBuf::from("/var/game/assets"),
    };
    let of = |event: &str| asset_path(Path::new(event), &roots);
    assert_eq!(
      of("/private/var/game/assets/ui/menu.ui.ron"),
      Some(path("ui/menu.ui.ron"))
    );
    assert_eq!(of("/var/game/assets/a.png"), Some(path("a.png")));
    assert_eq!(of("/var/game/other/a.png"), None);
    assert_eq!(of("/private/var/game/assets"), None, "the root itself");
    assert_eq!(of("/var/game/assets/ui/.menu.ui.ron.swp"), None);
    assert_eq!(of("/var/game/assets/ui/menu.ui.ron~"), None);
  }

  #[test]
  fn a_path_goes_out_once_its_writes_settle() {
    let start = Instant::now();
    let at = |ms| start + Duration::from_millis(ms);
    let mut pending = Pending::default();
    pending.record(path("a.txt"), at(0));
    pending.record(path("a.txt"), at(30));
    assert_eq!(
      pending.settled(at(100)).count(),
      0,
      "70 ms after the last write is too soon"
    );
    assert_eq!(
      pending.settled(at(130)).collect::<Vec<_>>(),
      [path("a.txt")]
    );
    assert!(pending.is_empty(), "it goes out once");
  }

  #[test]
  fn settled_paths_go_out_in_the_order_of_their_last_write() {
    let start = Instant::now();
    let at = |ms| start + Duration::from_millis(ms);
    let mut pending = Pending::default();
    pending.record(path("x.txt"), at(0));
    pending.record(path("y.txt"), at(10));
    pending.record(path("x.txt"), at(20));
    assert_eq!(
      pending.settled(at(115)).collect::<Vec<_>>(),
      [path("y.txt")]
    );
    assert_eq!(
      pending.settled(at(120)).collect::<Vec<_>>(),
      [path("x.txt")]
    );
  }
}
