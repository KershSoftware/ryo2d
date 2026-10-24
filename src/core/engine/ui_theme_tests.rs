//! The UI theme following a theme asset: used once it loads, and again when
//! its file is saved or code changes it; a reload that fails keeps it.

use std::sync::Arc;
use std::time::{Duration, Instant};

use super::*;
use crate::core::asset::{AssetError, AssetEvent, Assets, AssetsView, Handle, MemorySource};
use crate::core::ecs::kernel::{MessageCursor, Messages};
use crate::core::graphics::{Image, WgpuRenderer};
use crate::core::scene::{SceneAction, SceneContext};
use crate::core::ui::{Theme, UiTheme};

struct Idle;

impl Scene for Idle {
  fn update(&mut self, _ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    SceneAction::Continue
  }
}

/// A 2 x 2 PNG
fn png() -> Vec<u8> {
  let image = image::RgbaImage::from_pixel(2, 2, image::Rgba([255; 4]));
  let mut bytes = Vec::new();
  if let Err(error) = image.write_to(
    &mut std::io::Cursor::new(&mut bytes),
    image::ImageFormat::Png,
  ) {
    panic!("encoding a png: {error}");
  }
  bytes
}

/// An engine with a GPU (`None`, with a skip line, without one) reading and
/// watching `files`, its UI theme following the theme at `path`, one frame in
fn following(files: &MemorySource, path: &str) -> Option<(Engine, Handle<Theme>)> {
  let Ok((device, queue)) = crate::core::testing::shared_gpu() else {
    eprintln!("skipping: no GPU available for the engine");
    return None;
  };
  let mut engine = EngineBuilder::new()
    .resolution(320, 240)
    .asset_source(files.clone())
    .hot_reload(true)
    .start_scene(Idle)
    .build_headless();
  engine.init_gpu(WgpuRenderer::new_offscreen(device, queue, 320, 240), 1.0);
  let theme = match AssetsView::new(&mut engine.world).load_now::<Theme>(path) {
    Ok(theme) => theme,
    Err(error) => panic!("{path}: {}", error.report()),
  };
  engine.world.resource_mut::<UiTheme>().follow(theme.clone());
  step(&mut engine);
  Some((engine, theme))
}

fn step(engine: &mut Engine) {
  engine.frame(Some(Duration::from_micros(16_667)));
}

/// Steps until `done`, failing after two seconds
fn step_until(engine: &mut Engine, mut done: impl FnMut(&Engine) -> bool) {
  let start = Instant::now();
  while !done(engine) {
    assert!(
      start.elapsed() < Duration::from_secs(2),
      "the reload never landed"
    );
    step(engine);
    std::thread::sleep(Duration::from_millis(2));
  }
}

/// The name of the theme the UI draws with, and its generation
fn in_use(engine: &Engine) -> (String, u64) {
  let theme = engine.world.resource::<UiTheme>();
  (theme.get().name.clone(), theme.generation())
}

/// A cursor over the theme events from now on
fn theme_events(engine: &Engine) -> MessageCursor<AssetEvent<Theme>> {
  engine
    .world
    .resource::<Messages<AssetEvent<Theme>>>()
    .get_cursor_current()
}

/// The errors of the reloads that failed since `events` last read
fn reload_failures(
  engine: &Engine,
  events: &mut MessageCursor<AssetEvent<Theme>>,
) -> Vec<Arc<AssetError>> {
  events
    .read(engine.world.resource::<Messages<AssetEvent<Theme>>>())
    .filter_map(|event| match event {
      AssetEvent::ReloadFailed { error, .. } => Some(Arc::clone(error)),
      _ => None,
    })
    .collect()
}

#[test]
fn a_followed_theme_reloads_when_its_file_changes() {
  let files = MemorySource::new();
  files.insert("ui/followed.theme.ron", "(name: \"first\")");
  let Some((mut engine, theme)) = following(&files, "ui/followed.theme.ron") else {
    return;
  };
  let (name, first) = in_use(&engine);
  assert_eq!(name, "first", "used from the UI pass after `follow`");
  // The UI theme holds the asset while it follows it
  drop(theme);
  step(&mut engine);
  files.insert("ui/followed.theme.ron", "(name: \"second\")");
  step_until(&mut engine, |engine| in_use(engine).0 == "second");
  let (_, second) = in_use(&engine);
  assert_ne!(second, first, "the generation moved on");
  step(&mut engine);
  step(&mut engine);
  assert_eq!(in_use(&engine).1, second, "an idle pass leaves it");
}

#[test]
fn a_broken_theme_save_keeps_the_theme() {
  let path = "ui/broken-save.theme.ron";
  let files = MemorySource::new();
  files.insert(path, "(name: \"kept\")");
  let Some((mut engine, _theme)) = following(&files, path) else {
    return;
  };
  let before = in_use(&engine);
  assert_eq!(before.0, "kept");
  let mut events = theme_events(&engine);
  let mut failed = 0;
  // Saved twice with the same mistake: two failed reloads, one warning
  for times in 1..=2 {
    files.insert(path, "(name: \"broken\", palette: nonsense");
    step_until(&mut engine, |engine| {
      failed += reload_failures(engine, &mut events).len();
      failed == times
    });
  }
  step(&mut engine);
  step(&mut engine);
  assert_eq!(
    in_use(&engine),
    before,
    "the same theme, at the same generation"
  );
  let warnings = crate::core::dev_log::get_entries()
    .iter()
    .filter(|entry| entry.message.contains(path))
    .count();
  assert_eq!(warnings, 1, "warned once for the one mistake");
}

#[test]
fn a_theme_changed_in_code_applies_at_the_next_ui_pass() {
  let files = MemorySource::new();
  files.insert("ui/coded.theme.ron", "(name: \"from the file\")");
  let Some((mut engine, theme)) = following(&files, "ui/coded.theme.ron") else {
    return;
  };
  let (name, before) = in_use(&engine);
  assert_eq!(name, "from the file");
  let mut themes = engine.world.resource_mut::<Assets<Theme>>();
  let Some(value) = themes.get_mut(&theme) else {
    panic!("the theme is in its store");
  };
  value.name = "from code".to_string();
  step(&mut engine);
  let (name, after) = in_use(&engine);
  assert_eq!(name, "from code");
  assert_ne!(after, before, "the generation moved on");
}

#[test]
fn a_theme_whose_skin_image_fails_keeps_its_last_good_value() {
  let path = "ui/skin-fails.theme.ron";
  let skinned = |name: &str, image: &str| {
    format!(
      r#"(name: "{name}", skins: {{"frame": (path: "{image}", insets: (2.0, 2.0, 2.0, 2.0))}})"#
    )
  };
  let files = MemorySource::new();
  files.insert(path, skinned("good", "ui/frame.png"));
  files.insert("ui/frame.png", png());
  let Some((mut engine, _theme)) = following(&files, path) else {
    return;
  };
  let frame = AssetsView::new(&mut engine.world).load::<Image>("ui/frame.png");
  let skin = |engine: &Engine| engine.world.resource::<UiTheme>().get().skins["frame"].clone();
  assert_eq!(skin(&engine).image, Some(frame.clone()));
  let before = in_use(&engine);
  let mut events = theme_events(&engine);
  let mut failures = Vec::new();
  files.insert(path, skinned("bad", "ui/missing.png"));
  step_until(&mut engine, |engine| {
    failures.extend(reload_failures(engine, &mut events));
    !failures.is_empty()
  });
  assert!(
    matches!(&*failures[0], AssetError::Dependency { dependency, .. } if dependency.as_str() == "ui/missing.png"),
    "{}",
    failures[0].report()
  );
  step(&mut engine);
  assert_eq!(in_use(&engine), before, "the last good theme, as it was");
  let kept = skin(&engine);
  assert_eq!(kept.path.as_deref(), Some("ui/frame.png"));
  assert_eq!(kept.image, Some(frame));
}
