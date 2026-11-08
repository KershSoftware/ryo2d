//! Saves through a headless engine: `ctx.saves()`, `SaveFinished`, the
//! memory store and the flush when the engine goes.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use super::*;
use crate::core::saves::{MemorySaves, SaveFinished, SaveOp, SavesView};
use crate::core::scene::{SceneAction, SceneContext};
use crate::core::testing::HeadlessEngine;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Progress {
  level: u32,
}

/// What the scene saw
#[derive(Default)]
struct Seen {
  loaded: Option<u32>,
  finished: Vec<SaveFinished>,
}

/// Saves `slot1` as it enters and loads it back at once; then gathers the
/// `SaveFinished` messages
struct Saver {
  seen: Arc<Mutex<Seen>>,
}

impl Scene for Saver {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    ctx
      .saves()
      .save("slot1", Progress { level: 3 })
      .expect("the save is queued");
    let loaded = ctx.saves().load::<Progress>("slot1").ok();
    self.seen.lock().unwrap().loaded = loaded.map(|loaded| loaded.data.level);
  }

  fn update(&mut self, ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    let finished = ctx.messages::<SaveFinished>();
    self.seen.lock().unwrap().finished.extend(finished);
    SceneAction::Continue
  }
}

fn saver() -> (Saver, Arc<Mutex<Seen>>) {
  let seen = Arc::new(Mutex::new(Seen::default()));
  let scene = Saver {
    seen: Arc::clone(&seen),
  };
  (scene, seen)
}

#[test]
fn a_scene_saves_and_loads_through_ctx_saves_and_reads_save_finished() {
  let (scene, seen) = saver();
  let Some(mut engine) = HeadlessEngine::new_or_skip(EngineBuilder::new().start_scene(scene))
  else {
    return;
  };
  assert_eq!(
    seen.lock().unwrap().loaded,
    Some(3),
    "the queued data loads"
  );

  let deadline = std::time::Instant::now() + Duration::from_secs(5);
  while seen.lock().unwrap().finished.is_empty() && std::time::Instant::now() < deadline {
    engine.step().unwrap();
    std::thread::sleep(Duration::from_millis(1));
  }
  let finished = seen.lock().unwrap().finished.clone();
  assert_eq!(finished.len(), 1, "{finished:?}");
  assert_eq!(finished[0].slot, "slot1");
  assert_eq!(finished[0].op, SaveOp::Save);
  assert!(finished[0].result.is_ok());

  let saves = SavesView::new(engine.world());
  assert!(!saves.is_writing("slot1"));
  let loaded = saves.load::<Progress>("slot1").unwrap();
  assert_eq!(loaded.data, Progress { level: 3 });
  assert!(
    engine
      .saves()
      .names()
      .contains(&"slot1.ryo-save".to_string())
  );
}

#[test]
fn dropping_the_engine_finishes_pending_saves() {
  let store = MemorySaves::new();
  store.slow(Duration::from_millis(100));
  let (scene, _) = saver();
  let Some(engine) = HeadlessEngine::new_or_skip(
    EngineBuilder::new()
      .memory_saves(store.clone())
      .start_scene(scene),
  ) else {
    return;
  };
  assert!(store.get("slot1.ryo-save").is_none(), "still being written");
  drop(engine);
  let bytes = store
    .get("slot1.ryo-save")
    .expect("written before the drop returned");
  assert!(crate::core::saves::container::verify(&bytes).is_some());
}

/// Set in the copy of the test binary run from a project folder
const CHILD: &str = "RYO_SAVES_TEST_IN_PROJECT";

#[test]
fn a_headless_engine_keeps_saves_in_memory() {
  if std::env::var_os(CHILD).is_none() {
    // Run this test again in a process of its own whose working folder holds a
    // Ryo.toml, so no other test sees the change of folder
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("Ryo.toml"), "").unwrap();
    let data = dir.path().join("data");
    std::fs::create_dir(&data).unwrap();
    std::fs::write(data.join("keep.txt"), "x").unwrap();
    let name = concat!(module_path!(), "::a_headless_engine_keeps_saves_in_memory");
    let name = name.strip_prefix("ryo2d::").unwrap_or(name);
    let output = std::process::Command::new(std::env::current_exe().unwrap())
      .args(["--exact", name, "--nocapture", "--test-threads=1"])
      .env(CHILD, "1")
      .current_dir(dir.path())
      .output()
      .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
      output.status.success(),
      "{stdout}\n{}",
      String::from_utf8_lossy(&output.stderr)
    );
    assert!(stdout.contains("1 passed"), "{stdout}");
    let files: Vec<_> = std::fs::read_dir(&data)
      .unwrap()
      .map(|entry| entry.unwrap().file_name())
      .collect();
    assert_eq!(
      files,
      ["keep.txt"],
      "the project's data folder is unchanged"
    );
    return;
  }

  let (scene, _) = saver();
  let Some(mut engine) = HeadlessEngine::new_or_skip(EngineBuilder::new().start_scene(scene))
  else {
    return;
  };
  let deadline = std::time::Instant::now() + Duration::from_secs(5);
  while !engine
    .saves()
    .names()
    .contains(&"slot1.ryo-save".to_string())
    && std::time::Instant::now() < deadline
  {
    engine.step().unwrap();
    std::thread::sleep(Duration::from_millis(1));
  }
  assert_eq!(engine.saves().names(), ["slot1.ryo-save"]);
}
