//! Allocations of the asset registry's pump, counted by a global allocator
//! in this test binary. Only the test's own thread counts: the pump runs
//! there, and the workers' reads and loads are not the pump's
// This test binary holds one test, so nothing else runs on its thread

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::time::{Duration, Instant};

use ryo2d::core::asset::pump;
use ryo2d::prelude::*;

struct Counting;
static ALLOCATIONS: AtomicUsize = AtomicUsize::new(0);

thread_local! {
  /// Whether this thread's allocations are being counted
  static COUNTING: Cell<bool> = const { Cell::new(false) };
}

fn note() {
  if COUNTING.try_with(Cell::get).unwrap_or(false) {
    ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
  }
}

unsafe impl GlobalAlloc for Counting {
  unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
    note();
    unsafe { System.alloc(layout) }
  }
  unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
    unsafe { System.dealloc(ptr, layout) }
  }
  unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
    note();
    unsafe { System.realloc(ptr, layout, size) }
  }
}

#[global_allocator]
static GLOBAL: Counting = Counting;

/// The allocations `f` makes on this thread
fn allocations(f: impl FnOnce()) -> usize {
  let before = ALLOCATIONS.load(Ordering::Relaxed);
  COUNTING.with(|counting| counting.set(true));
  f();
  COUNTING.with(|counting| counting.set(false));
  ALLOCATIONS.load(Ordering::Relaxed) - before
}

struct Text(#[allow(dead_code)] String);
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

/// A file of text paths, one a line, each loaded as a dependency
struct Bundle(#[allow(dead_code)] Vec<Handle<Text>>);
impl Asset for Bundle {}

struct BundleLoader;
impl AssetLoader for BundleLoader {
  type Asset = Bundle;
  type Settings = ();
  fn extensions(&self) -> &[&'static str] {
    &["bundle"]
  }
  fn load(&self, bytes: Vec<u8>, _: &(), cx: &mut LoadContext<'_>) -> Result<Bundle, AssetError> {
    let text = String::from_utf8(bytes).map_err(|e| AssetError::decode(cx.path(), e))?;
    let parts = text.lines().map(|line| cx.load(line.trim()));
    Ok(Bundle(parts.collect::<Result<_, _>>()?))
  }
}

/// Files in memory; a read of `held` waits until the gate opens
#[derive(Clone)]
struct Held {
  files: MemorySource,
  held: &'static str,
  open: Arc<(Mutex<bool>, Condvar)>,
}

impl Held {
  fn open(&self) {
    let (open, opened) = &*self.open;
    *open.lock().unwrap_or_else(PoisonError::into_inner) = true;
    opened.notify_all();
  }
}

impl AssetSource for Held {
  fn read(&self, path: &AssetPath) -> Result<Vec<u8>, AssetError> {
    if path.as_str() == self.held {
      let (open, opened) = &*self.open;
      let mut open = open.lock().unwrap_or_else(PoisonError::into_inner);
      while !*open {
        open = opened.wait(open).unwrap_or_else(PoisonError::into_inner);
      }
    }
    self.files.read(path)
  }
}

/// Pumps until `done`, failing with `what` after five seconds
fn pump_until(world: &mut World, what: &str, mut done: impl FnMut() -> bool) {
  let start = Instant::now();
  loop {
    pump(world);
    if done() {
      return;
    }
    assert!(
      start.elapsed() < Duration::from_secs(5),
      "timed out: {what}"
    );
    std::thread::sleep(Duration::from_millis(1));
  }
}

/// A hundred frames' pumps
fn hundred_pumps(world: &mut World) -> usize {
  allocations(|| {
    for _ in 0..100 {
      pump(world);
    }
  })
}

#[test]
fn the_pump_allocates_nothing_with_nothing_to_load_reload_or_free() {
  let files = MemorySource::new();
  files.insert("a.txt", "a");
  files.insert("parent.bundle", "slow.txt");
  files.insert("slow.txt", "slow");
  let source = Held {
    files,
    held: "slow.txt",
    open: Arc::default(),
  };
  let mut world = World::new();
  let server = AssetServer::new(source.clone());
  server.register_asset::<Text>(&mut world);
  server.register_asset::<Bundle>(&mut world);
  server.register_loader(TextLoader);
  server.register_loader(BundleLoader);
  world.insert_resource(server.clone());
  for _ in 0..10 {
    pump(&mut world);
  }

  let idle = hundred_pumps(&mut world);
  assert_eq!(
    idle, 0,
    "100 pumps with nothing loaded allocated {idle} times"
  );

  // A loaded asset whose handle is cloned and its clones dropped, which
  // frees nothing: the last handle lives on (a last drop is a free, and may
  // allocate on the drop channel)
  let text = server
    .load_now::<Text>(&mut world, "a.txt")
    .expect("a.txt loads");
  for _ in 0..10 {
    pump(&mut world);
  }
  let held = allocations(|| {
    for _ in 0..100 {
      let clone = text.clone();
      pump(&mut world);
      drop(clone);
      pump(&mut world);
    }
  });
  assert_eq!(
    held, 0,
    "100 frames cloning and dropping a held handle allocated {held} times"
  );

  // A parent whose dependency is still reading waits in the pump: pumped
  // until the server holds it back, then measured. Allocations are only
  // measured here, never waited on
  let parent = server.load::<Bundle>("parent.bundle");
  pump_until(&mut world, "the parent never waited on slow.txt", || {
    server.waiting_count() == 1
  });
  // The parent's dependency, found by its key: no new load
  let slow = server.load::<Text>("slow.txt");
  let waiting = hundred_pumps(&mut world);
  // Every measured pump ran with the parent held back
  assert_eq!(server.waiting_count(), 1, "the parent still waits");
  assert!(matches!(server.state(&parent), LoadState::Loading));
  assert!(matches!(server.state(&slow), LoadState::Loading));
  assert_eq!(
    waiting, 0,
    "100 pumps with a parent waiting allocated {waiting} times"
  );

  source.open();
  pump_until(&mut world, "the parent never loaded after slow.txt", || {
    server.state(&parent).is_loaded()
  });
  assert!(
    server.is_ready(&parent),
    "it waited for slow.txt, and loaded"
  );
}
