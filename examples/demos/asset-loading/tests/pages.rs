//! The demo's pages without a window, over the demo's own asset folder: the
//! loading screen loads every asset on the worker threads once asked and
//! settles each one, and the hot reload page counts each reload

use std::time::{Duration, Instant};

use asset_loading_demo::demo;
use demo_kit::prelude::*;
use ryo2d::testing::HeadlessEngine;

/// The Loading screen page
const LOADING: usize = 0;
/// The Hot reload page
const HOT_RELOAD: usize = 1;

/// The ten images in the demo's folder
const IMAGES: usize = 10;

fn engine_on(page: usize) -> Option<HeadlessEngine> {
  HeadlessEngine::new_or_skip(demo().start_page(page).engine_builder())
}

/// The registry's (loaded, loading, failed) counts of images, fonts and UI
/// files, the assets loaded by path
fn counts(engine: &mut HeadlessEngine) -> [(usize, usize, usize); 3] {
  let stats = engine.assets().stats();
  ["Image", "Font", "UiFile"].map(|name| {
    stats
      .types
      .iter()
      .find(|kind| kind.name == name)
      .map_or((0, 0, 0), |kind| (kind.loaded, kind.loading, kind.failed))
  })
}

/// Nothing of the demo's is still loading
fn settled(engine: &mut HeadlessEngine) -> bool {
  counts(engine).iter().all(|&(_, loading, _)| loading == 0)
}

/// Step until `done` holds, for at most ten seconds
fn step_until(
  engine: &mut HeadlessEngine,
  what: &str,
  mut done: impl FnMut(&mut HeadlessEngine) -> bool,
) {
  let start = Instant::now();
  while !done(engine) {
    assert!(
      start.elapsed() < Duration::from_secs(10),
      "timed out waiting for {what}"
    );
    engine.step().unwrap();
    std::thread::sleep(Duration::from_millis(1));
  }
}

/// The texts the last frame drew, in the order it drew them
fn texts(engine: &HeadlessEngine) -> Vec<String> {
  const MARK: &str = "TextLayout { text: \"";
  let snapshot = engine.ui_snapshot();
  let mut rest = snapshot.lines().next().unwrap_or_default();
  let mut texts = Vec::new();
  while let Some(at) = rest.find(MARK) {
    rest = &rest[at + MARK.len()..];
    let mut text = String::new();
    let mut end = rest.len();
    let mut chars = rest.char_indices();
    while let Some((i, c)) = chars.next() {
      match c {
        '\\' => {
          if let Some((_, escaped)) = chars.next() {
            text.push(escaped);
          }
        }
        '"' => {
          end = i;
          break;
        }
        _ => text.push(c),
      }
    }
    rest = &rest[end..];
    texts.push(text);
  }
  texts
}

/// The size, in UI units, of the first text drawn that starts with `start`
fn drawn_size(engine: &HeadlessEngine, start: &str) -> Option<(f32, f32)> {
  let snapshot = engine.ui_snapshot();
  let commands = snapshot.lines().next().unwrap_or_default();
  let escaped = format!("{start:?}");
  let at = commands.find(&escaped[..escaped.len() - 1])?;
  let rest = &commands[at..];
  let size = &rest[rest.find("size: Vec2(")? + "size: Vec2(".len()..];
  let (width, rest) = size.split_once(", ")?;
  let height = &rest[..rest.find(')')?];
  Some((width.parse().ok()?, height.parse().ok()?))
}

/// The `len` texts drawn from the first `label` on: one row of a page's table
fn row<'a>(texts: &'a [String], label: &str, len: usize) -> Vec<&'a str> {
  let Some(at) = texts.iter().position(|text| text == label) else {
    return Vec::new();
  };
  texts[at..].iter().take(len).map(String::as_str).collect()
}

#[test]
fn the_loading_screen_loads_every_asset_on_the_workers_once_asked() {
  let Some(mut engine) = engine_on(LOADING) else {
    return;
  };
  engine.step_frames(3).unwrap();
  assert_eq!(
    counts(&mut engine),
    [(0, 0, 0); 3],
    "nothing loads before Space"
  );
  let idle = texts(&engine);
  assert_eq!(row(&idle, "bg.png", 3), ["bg.png", "Image", "not loaded"]);
  assert!(idle.iter().any(|text| text == "0 / 14"), "{idle:?}");

  engine.tap_key(KeyCode::Space);
  engine.step().unwrap();
  // `load` only queues: a load lands in a pump, the next frame's at the soonest
  assert_eq!(
    counts(&mut engine),
    [(0, IMAGES + 1, 0), (0, 2, 0), (0, 1, 0)],
    "every asset is on its way, the missing one too"
  );

  step_until(&mut engine, "every asset to settle", settled);
  engine.step_frames(2).unwrap();
  assert_eq!(
    counts(&mut engine),
    [(IMAGES, 0, 1), (2, 0, 0), (1, 0, 0)],
    "every file loaded but the missing one"
  );
  let done = texts(&engine);
  assert_eq!(
    row(&done, "missing.png", 3),
    [
      "missing.png",
      "Image",
      "failed: no asset at \"missing.png\""
    ],
    "the failure's error, naming the asset path only"
  );
  assert_eq!(
    row(&done, "bg.png", 3),
    ["bg.png", "Image", "loaded 800×450"]
  );
  assert_eq!(
    row(&done, "fonts/PressStart2P-Regular.ttf", 3),
    [
      "fonts/PressStart2P-Regular.ttf",
      "Font",
      "loaded Press Start 2P"
    ]
  );
  assert_eq!(
    row(&done, "ui/card.ui.ron", 3),
    ["ui/card.ui.ron", "UI file", "loaded"]
  );
  assert!(done.iter().any(|text| text == "14 / 14"), "{done:?}");
}

#[test]
fn a_long_load_error_wraps_inside_the_page() {
  // A broken image: its error names the decoder's cause, too long for a line
  let files = MemorySource::new();
  files.insert("bg.png", "not a png");
  let builder = demo()
    .start_page(LOADING)
    .configure(move |b| b.asset_source(files))
    .engine_builder();
  let Some(mut engine) = HeadlessEngine::new_or_skip(builder) else {
    return;
  };
  engine.step_frames(3).unwrap();
  engine.tap_key(KeyCode::Space);
  engine.step().unwrap();
  step_until(&mut engine, "every asset to settle", settled);
  engine.step_frames(2).unwrap();
  let error = "failed: could not load \"bg.png\": ";
  let Some((width, height)) = drawn_size(&engine, error) else {
    panic!("the error is drawn: {:?}", texts(&engine));
  };
  // The state column starts 370 units in, on an 800-unit page
  assert!(width <= 800.0 - 370.0, "it stays on the page: {width} wide");
  assert!(height > 30.0, "it wraps onto a second line: {height} high");
}

#[test]
fn space_on_a_finished_load_reads_every_file_again() {
  let Some(mut engine) = engine_on(LOADING) else {
    return;
  };
  engine.step_frames(3).unwrap();
  engine.tap_key(KeyCode::Space);
  engine.step().unwrap();
  step_until(&mut engine, "the first load to settle", settled);
  engine.step_frames(2).unwrap();

  engine.tap_key(KeyCode::Space);
  // The page lets go of the assets, waits for the pump to free them, then
  // loads them again: none is kept from the first load
  let mut loading_again = false;
  for _ in 0..10 {
    engine.step().unwrap();
    let [(images, loading, _), _, _] = counts(&mut engine);
    if images == 0 && loading == IMAGES + 1 {
      loading_again = true;
      break;
    }
  }
  assert!(
    loading_again,
    "the images are read again: {:?}",
    counts(&mut engine)
  );
  step_until(&mut engine, "the second load to settle", settled);
  assert_eq!(counts(&mut engine), [(IMAGES, 0, 1), (2, 0, 0), (1, 0, 0)]);
}

#[test]
fn load_again_finishes_while_another_holder_keeps_one_of_its_assets() {
  let Some(mut engine) = engine_on(LOADING) else {
    return;
  };
  engine.step_frames(3).unwrap();
  // Held outside the page, as another scene might hold it
  let kept = engine
    .assets()
    .load_now::<Image>("back_decor.png")
    .expect("it loads");
  engine.tap_key(KeyCode::Space);
  engine.step().unwrap();
  step_until(&mut engine, "the first load to settle", settled);
  engine.step_frames(2).unwrap();

  engine.tap_key(KeyCode::Space);
  let done = |engine: &mut HeadlessEngine| texts(engine).iter().any(|t| t.starts_with("Done:"));
  step_until(&mut engine, "the page to load again", |engine| {
    !done(engine)
  });
  // The kept image is not freed, and the page does not wait for it
  step_until(&mut engine, "the second load to finish", done);
  let shown = texts(&engine);
  assert!(shown.iter().any(|text| text == "14 / 14"), "{shown:?}");
  assert_eq!(
    row(&shown, "back_decor.png", 3),
    ["back_decor.png", "Image", "loaded 3840×2160"]
  );
  drop(kept);
}

#[test]
fn the_hot_reload_page_counts_each_reload_as_modified() {
  let Some(mut engine) = engine_on(HOT_RELOAD) else {
    return;
  };
  engine.step_frames(3).unwrap();
  // A `load_now` that fails gives its error and no handle, so the registry
  // keeps nothing of the missing file
  assert_eq!(
    counts(&mut engine),
    [(IMAGES, 0, 0), (2, 0, 0), (1, 0, 0)],
    "everything is loaded at once"
  );
  let before = texts(&engine);
  assert_eq!(row(&before, "bg.png", 2), ["bg.png", "modified 0"]);
  assert_eq!(
    row(&before, "missing.png", 3),
    [
      "missing.png",
      "modified 0",
      "failed: no asset at \"missing.png\""
    ]
  );

  // As a save in the folder does, with hot reload on
  engine.assets().server().reload("bg.png");
  engine.assets().server().reload("ui/card.ui.ron");
  step_until(&mut engine, "the reloads to count", |engine| {
    let shown = texts(engine);
    row(&shown, "bg.png", 2) == ["bg.png", "modified 1"]
      && row(&shown, "ui/card.ui.ron", 2) == ["ui/card.ui.ron", "modified 1"]
  });
  let after = texts(&engine);
  assert_eq!(
    row(&after, "pipe_cap.png", 2),
    ["pipe_cap.png", "modified 0"]
  );
  assert_eq!(
    row(&after, "fonts/m5x7.ttf", 2),
    ["fonts/m5x7.ttf", "modified 0"]
  );
}

#[test]
fn a_reload_that_fails_shows_its_error_and_keeps_the_asset() {
  // The demo's files in memory, so the test can break one
  let files = MemorySource::new();
  let folder = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets");
  for path in ["bg.png", "fonts/m5x7.ttf", "ui/card.ui.ron"] {
    files.insert(path, std::fs::read(folder.join(path)).unwrap());
  }
  let source = files.clone();
  let builder = demo()
    .start_page(HOT_RELOAD)
    .configure(move |b| b.asset_source(source))
    .engine_builder();
  let Some(mut engine) = HeadlessEngine::new_or_skip(builder) else {
    return;
  };
  engine.step_frames(3).unwrap();
  files.insert("bg.png", "not a png");
  engine.assets().server().reload("bg.png");
  step_until(&mut engine, "the failed reload to show", |engine| {
    let shown = texts(engine);
    row(&shown, "bg.png", 3)
      .get(2)
      .is_some_and(|note| note.starts_with("reload failed"))
  });
  let shown = texts(&engine);
  let bg = row(&shown, "bg.png", 3);
  assert_eq!(
    bg[..2],
    ["bg.png", "modified 0"],
    "a failed reload is no change"
  );
  assert!(
    bg[2].starts_with("reload failed: could not load \"bg.png\": "),
    "the error and its cause: {bg:?}"
  );
  let [(images, _, _), _, _] = counts(&mut engine);
  assert_eq!(images, 1, "the last good bg.png stays loaded");
}
