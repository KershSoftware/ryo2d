//! UI files in the engine: a file spawned by a scene, edited on disk and
//! reloaded while the game runs

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use ryo2d::prelude::*;
use ryo2d::testing::HeadlessEngine;

/// Spawns its file on enter, and reloads it once `reload` is set, setting
/// `failed` if the new file has mistakes
struct FileScene {
  path: PathBuf,
  reload: Arc<AtomicBool>,
  failed: Arc<AtomicBool>,
}

impl Scene for FileScene {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    ctx
      .spawn_ui_file(self.path.to_str().unwrap())
      .expect("the file loads");
  }

  fn update(&mut self, ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    if self.reload.swap(false, Ordering::SeqCst) && ctx.assets.reload_ui(&self.path).is_err() {
      self.failed.store(true, Ordering::SeqCst);
    }
    SceneAction::Continue
  }
}

fn menu(width: u32) -> String {
  format!(
    r#"(root: Column(focus_scope: true, anchor: Center, children: [Button(label: "Play", width: {width}, default_focus: true)]))"#
  )
}

/// An engine running a `FileScene` over `menu(100)`, four frames in, with
/// its file's path and the scene's two flags
fn start(
  dir: &tempfile::TempDir,
) -> Option<(HeadlessEngine, PathBuf, Arc<AtomicBool>, Arc<AtomicBool>)> {
  let path = dir.path().join("menu.ui.ron");
  std::fs::write(&path, menu(100)).unwrap();
  let reload = Arc::new(AtomicBool::new(false));
  let failed = Arc::new(AtomicBool::new(false));
  let scene = FileScene {
    path: path.clone(),
    reload: reload.clone(),
    failed: failed.clone(),
  };
  let mut engine =
    HeadlessEngine::new_or_skip(EngineBuilder::new().resolution(640, 480).start_scene(scene))?;
  engine.step_frames(4).unwrap();
  Some((engine, path, reload, failed))
}

#[test]
fn an_edited_file_reloads_in_the_running_engine_and_keeps_its_ids() {
  let dir = tempfile::tempdir().unwrap();
  let Some((mut engine, path, reload, failed)) = start(&dir) else {
    return;
  };
  let (id, before) = engine.ui_focusables()[0];
  std::fs::write(&path, menu(200)).unwrap();
  reload.store(true, Ordering::SeqCst);
  engine.step_frames(3).unwrap();
  assert!(!failed.load(Ordering::SeqCst), "the new file loads");
  let (same, after) = engine.ui_focusables()[0];
  assert_eq!(same, id, "the button kept its entity, so its id");
  assert!(after.width > before.width + 50.0, "{before:?} -> {after:?}");
}

#[test]
fn an_edit_with_mistakes_keeps_the_running_tree() {
  let dir = tempfile::tempdir().unwrap();
  let Some((mut engine, path, reload, failed)) = start(&dir) else {
    return;
  };
  let before = engine.ui_focusables()[0];
  std::fs::write(&path, menu(200).replace("Button(", "Buton(")).unwrap();
  reload.store(true, Ordering::SeqCst);
  engine.step_frames(3).unwrap();
  assert!(
    failed.load(Ordering::SeqCst),
    "the reload reports the mistake"
  );
  assert_eq!(
    engine.ui_focusables()[0],
    before,
    "the same button, as it was"
  );
}
