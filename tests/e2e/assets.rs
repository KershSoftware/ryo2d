//! The asset registry in a running engine: scenes load through
//! `ctx.assets()`, the pump inserts queued loads each frame, and a handle a
//! scene drops frees its asset

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use ryo2d::prelude::*;
use ryo2d::testing::HeadlessEngine;

/// A text file
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

/// A headless engine reading `files`, with texts registered, running `scene`
fn engine(files: MemorySource, scene: impl Scene + 'static) -> Option<HeadlessEngine> {
  HeadlessEngine::new_or_skip(
    EngineBuilder::new()
      .resolution(320, 240)
      .asset_source(files)
      .asset_loader(TextLoader)
      .start_scene(scene),
  )
}

fn files() -> MemorySource {
  let files = MemorySource::new();
  files.insert("a.txt", "hello");
  files.insert("b.txt", "later");
  files
}

/// Loads `a.txt` at once on enter and reads it back on each update
struct ReadsBack {
  text: Option<Handle<Text>>,
  read: Arc<Mutex<Option<String>>>,
}

impl Scene for ReadsBack {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    self.text = Some(ctx.assets().load_now("a.txt").expect("a.txt loads"));
  }

  fn update(&mut self, ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    if let Some(text) = &self.text {
      *self.read.lock().unwrap() = ctx.assets().get(text).map(|text| text.0.clone());
    }
    SceneAction::Continue
  }
}

#[test]
fn a_scene_loads_now_on_enter_and_reads_it_back_in_update() {
  let read = Arc::new(Mutex::new(None));
  let scene = ReadsBack {
    text: None,
    read: read.clone(),
  };
  let Some(mut engine) = engine(files(), scene) else {
    return;
  };
  engine.step().unwrap();
  assert_eq!(read.lock().unwrap().as_deref(), Some("hello"));
}

/// Asks for `b.txt` on its first update, and records each later update
/// whether it is loaded and what it reads
struct LoadsLater {
  text: Option<Handle<Text>>,
  read: Arc<Mutex<Option<String>>>,
}

impl Scene for LoadsLater {
  fn update(&mut self, ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    let assets = ctx.assets();
    match &self.text {
      None => self.text = Some(assets.load("b.txt")),
      Some(text) => {
        if assets.state(text).is_loaded() {
          *self.read.lock().unwrap() = assets.get(text).map(|text| text.0.clone());
        }
      }
    }
    SceneAction::Continue
  }
}

#[test]
fn a_load_from_update_is_loaded_within_ten_frames() {
  let read = Arc::new(Mutex::new(None));
  let scene = LoadsLater {
    text: None,
    read: read.clone(),
  };
  let Some(mut engine) = engine(files(), scene) else {
    return;
  };
  let mut frames = 0;
  while read.lock().unwrap().is_none() && frames < 10 {
    engine.step().unwrap();
    frames += 1;
  }
  assert_eq!(
    read.lock().unwrap().as_deref(),
    Some("later"),
    "loaded within {frames} frames"
  );
}

/// Holds `a.txt` from enter until it leaves (when `leave` is set); its
/// handle drops on exit
struct Holds {
  text: Option<Handle<Text>>,
  id: Arc<Mutex<Option<AssetId<Text>>>>,
  leave: Arc<AtomicBool>,
}

impl Scene for Holds {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    let text: Handle<Text> = ctx.assets().load_now("a.txt").expect("a.txt loads");
    *self.id.lock().unwrap() = Some(text.id());
    self.text = Some(text);
  }

  fn update(&mut self, _ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    if self.leave.load(Ordering::SeqCst) {
      return SceneAction::Switch(Box::new(Empty));
    }
    SceneAction::Continue
  }

  fn on_exit(&mut self, _ctx: &mut SceneContext) {
    self.text = None;
  }
}

struct Empty;
impl Scene for Empty {}

#[test]
fn a_handle_dropped_on_exit_frees_its_asset_within_two_frames() {
  let id = Arc::new(Mutex::new(None));
  let leave = Arc::new(AtomicBool::new(false));
  let scene = Holds {
    text: None,
    id: id.clone(),
    leave: leave.clone(),
  };
  let Some(mut engine) = engine(files(), scene) else {
    return;
  };
  engine.step_frames(2).unwrap();
  let id = id.lock().unwrap().expect("the scene loaded a.txt");
  assert_eq!(
    engine.assets().get(id).map(|text| text.0.as_str()),
    Some("hello")
  );

  let mut events = engine
    .world()
    .resource::<Messages<AssetEvent<Text>>>()
    .get_cursor_current();
  leave.store(true, Ordering::SeqCst);
  let mut removed = false;
  for _ in 0..2 {
    engine.step().unwrap();
    let messages = engine.world().resource::<Messages<AssetEvent<Text>>>();
    removed |= events
      .read(messages)
      .any(|event| matches!(event, AssetEvent::Removed { id: gone } if *gone == id));
  }
  assert!(removed, "Removed within two frames of the scene leaving");
  assert!(
    engine.assets().get(id).is_none(),
    "the store no longer has it"
  );
}
