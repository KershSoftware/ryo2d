//! UI files in the engine: a file spawned by a scene, saved again while the
//! game runs, and the trees built from it brought in line

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use ryo2d::core::dev_log;
use ryo2d::prelude::*;
use ryo2d::testing::HeadlessEngine;

/// What the scene saw: the `UiReloaded` messages, and the reloads of its
/// files that failed
#[derive(Default)]
struct Seen {
  reloaded: Vec<UiReloaded>,
  failed: usize,
}

/// Spawns its file on enter, and records what each update reads
struct FileScene {
  path: &'static str,
  seen: Arc<Mutex<Seen>>,
}

impl Scene for FileScene {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    ctx.spawn_ui_file(self.path).expect("the file loads");
  }

  fn update(&mut self, ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    let reloaded = ctx.messages::<UiReloaded>();
    let failed = ctx
      .messages::<AssetEvent<UiFile>>()
      .iter()
      .filter(|event| matches!(event, AssetEvent::ReloadFailed { .. }))
      .count();
    let mut seen = self.seen.lock().unwrap();
    seen.reloaded.extend(reloaded);
    seen.failed += failed;
    SceneAction::Continue
  }
}

fn menu(width: u32) -> String {
  format!(
    r#"(root: Column(focus_scope: true, anchor: Center, children: [Button(label: "Play", width: {width}, default_focus: true)]))"#
  )
}

/// An engine whose scene spawns `path` from `files`, watching them, four
/// frames in
fn start(files: &MemorySource, path: &'static str) -> Option<(HeadlessEngine, Arc<Mutex<Seen>>)> {
  let seen = Arc::new(Mutex::new(Seen::default()));
  let scene = FileScene {
    path,
    seen: seen.clone(),
  };
  let builder = EngineBuilder::new()
    .resolution(640, 480)
    .asset_source(files.clone())
    .hot_reload(true)
    .start_scene(scene);
  let mut engine = HeadlessEngine::new_or_skip(builder)?;
  engine.step_frames(4).unwrap();
  Some((engine, seen))
}

/// Steps until `done`, failing after two seconds
fn step_until(engine: &mut HeadlessEngine, seen: &Mutex<Seen>, done: impl Fn(&Seen) -> bool) {
  let start = Instant::now();
  while !done(&seen.lock().unwrap()) {
    assert!(
      start.elapsed() < Duration::from_secs(2),
      "the reload never landed"
    );
    engine.step().unwrap();
    std::thread::sleep(Duration::from_millis(2));
  }
}

#[test]
fn a_saved_file_reloads_its_trees_in_place() {
  let files = MemorySource::new();
  files.insert("ui/saved.ui.ron", menu(100));
  let Some((mut engine, seen)) = start(&files, "ui/saved.ui.ron") else {
    return;
  };
  let (id, before) = engine.ui_focusables()[0];
  files.insert("ui/saved.ui.ron", menu(200));
  step_until(&mut engine, &seen, |seen| !seen.reloaded.is_empty());
  engine.step().unwrap();
  let (same, after) = engine.ui_focusables()[0];
  assert_eq!(same, id, "the button kept its entity, so its id");
  assert!(after.width > before.width + 50.0, "{before:?} -> {after:?}");
  let seen = seen.lock().unwrap();
  assert_eq!(seen.reloaded.len(), 1, "{:?}", seen.reloaded);
  assert_eq!(seen.reloaded[0].replaced, None);
}

#[test]
fn a_broken_save_keeps_the_trees_and_warns_once() {
  let files = MemorySource::new();
  files.insert("ui/broken-save.ui.ron", menu(100));
  let Some((mut engine, seen)) = start(&files, "ui/broken-save.ui.ron") else {
    return;
  };
  let before = engine.ui_focusables()[0];
  let broken = menu(200).replace("Button(", "Buton(");
  // Saved twice with the same mistake: two failed reloads, one warning
  files.insert("ui/broken-save.ui.ron", broken.clone());
  step_until(&mut engine, &seen, |seen| seen.failed == 1);
  files.insert("ui/broken-save.ui.ron", broken);
  step_until(&mut engine, &seen, |seen| seen.failed == 2);
  engine.step_frames(2).unwrap();
  assert_eq!(
    engine.ui_focusables()[0],
    before,
    "the same button, as it was"
  );
  assert!(seen.lock().unwrap().reloaded.is_empty());
  let warnings = dev_log::get_entries()
    .iter()
    .filter(|entry| entry.message.contains("ui/broken-save.ui.ron"))
    .count();
  assert_eq!(warnings, 1, "warned once for the one mistake");
}

#[test]
fn an_import_reload_rebuilds_its_importers_trees() {
  let files = MemorySource::new();
  let common = |width: u32| {
    format!(
      r#"(templates: {{"Play": (node: Button(label: "Play", width: {width}, default_focus: true))}})"#
    )
  };
  files.insert("ui/common-play.ui.ron", common(100));
  files.insert(
    "ui/importer.ui.ron",
    r#"(imports: ["ui/common-play.ui.ron"], root: Column(focus_scope: true, anchor: Center, children: [Play]))"#,
  );
  let Some((mut engine, seen)) = start(&files, "ui/importer.ui.ron") else {
    return;
  };
  let (id, before) = engine.ui_focusables()[0];
  files.insert("ui/common-play.ui.ron", common(200));
  step_until(&mut engine, &seen, |seen| !seen.reloaded.is_empty());
  engine.step().unwrap();
  let (same, after) = engine.ui_focusables()[0];
  assert_eq!(same, id, "the button kept its entity");
  assert!(after.width > before.width + 50.0, "{before:?} -> {after:?}");
  assert_eq!(seen.lock().unwrap().reloaded.len(), 1);
}
