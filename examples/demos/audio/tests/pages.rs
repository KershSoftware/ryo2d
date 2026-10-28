//! The demo's pages without a window, on the mock audio backend: each page
//! plays for two seconds without a warning, from the demo's folder and from
//! the files the web build embeds; the limits page keeps each rule's row
//! within its voices while each rule acts; a slider sets its volume
//! setting; the variations page takes a saved fifth take; the music page
//! crossfades; and the hot reload page counts a saved definition

use std::time::{Duration, Instant};

use audio_demo::{Reloads, demo, embedded, limits};
use ryo2d::core::audio::AudioLogKind;
use ryo2d::core::dev_log::{self, LogLevel};
use ryo2d::prelude::*;
use ryo2d::testing::HeadlessEngine;

/// The pages, in the demo's order
const BUSES: usize = 0;
const VARIATIONS: usize = 1;
const LIMITS: usize = 2;
const MUSIC: usize = 5;
const HOT_RELOAD: usize = 6;
const PAGES: usize = 7;

/// Two seconds at the demo's 60 Hz
const FRAMES: u32 = 120;

fn engine_on(page: usize) -> Option<HeadlessEngine> {
  HeadlessEngine::new_or_skip(demo().start_page(page).engine_builder())
}

/// The engine on `page`, its assets read from `files` as on the web
fn engine_over(page: usize, files: MemorySource) -> Option<HeadlessEngine> {
  let builder = demo()
    .start_page(page)
    .configure(move |b| b.asset_source(files))
    .engine_builder();
  HeadlessEngine::new_or_skip(builder)
}

fn audio(engine: &mut HeadlessEngine) -> AudioView<'_> {
  AudioView::new(engine.world_mut())
}

/// Every warning in the dev log. It is the process's, so the other tests'
/// land in it too; none of them expects a warning either
fn warnings() -> Vec<String> {
  dev_log::get_entries()
    .into_iter()
    .filter(|entry| entry.level >= LogLevel::Warn)
    .map(|entry| entry.message)
    .collect()
}

/// The text of the demo's file `path`, as the web build embeds it
fn text_of(path: &str) -> String {
  audio_demo::EMBEDDED
    .iter()
    .find(|(embedded, _)| *embedded == path)
    .map(|(_, bytes)| String::from_utf8_lossy(bytes).into_owned())
    .unwrap_or_else(|| panic!("{path} is not embedded"))
}

/// Step every page for two seconds, checking each plays something on some
/// frame and none warns
fn every_page_plays(mut engine_on: impl FnMut(usize) -> Option<HeadlessEngine>) {
  for page in 0..PAGES {
    let Some(mut engine) = engine_on(page) else {
      return;
    };
    let mut most = 0;
    for _ in 0..FRAMES {
      engine.step().unwrap();
      most = most.max(audio(&mut engine).voices_on("master"));
    }
    assert!(most > 0, "page {} plays nothing", page + 1);
    let warned = warnings();
    assert!(warned.is_empty(), "page {}: {warned:#?}", page + 1);
  }
}

#[test]
fn every_page_plays_for_two_seconds_without_a_warning() {
  every_page_plays(engine_on);
}

#[test]
fn every_page_plays_from_the_files_the_web_build_embeds() {
  every_page_plays(|page| engine_over(page, embedded()));
}

#[test]
fn the_web_build_embeds_every_asset_file() {
  let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets");
  let mut on_disk = Vec::new();
  let mut folders = vec![root.clone()];
  while let Some(folder) = folders.pop() {
    for entry in std::fs::read_dir(&folder).unwrap() {
      let path = entry.unwrap().path();
      if path.is_dir() {
        folders.push(path);
      } else if path.file_name().is_some_and(|name| name != "README.md") {
        let relative = path.strip_prefix(&root).unwrap();
        on_disk.push(relative.to_string_lossy().replace('\\', "/"));
      }
    }
  }
  on_disk.sort();
  let mut embedded: Vec<String> = audio_demo::EMBEDDED
    .iter()
    .map(|(path, _)| path.to_string())
    .collect();
  embedded.sort();
  assert_eq!(embedded, on_disk);
  for (path, bytes) in audio_demo::EMBEDDED {
    assert_eq!(*bytes, std::fs::read(root.join(path)).unwrap(), "{path}");
  }
}

#[test]
fn each_limit_row_keeps_within_its_voices_while_its_rule_acts() {
  let Some(mut engine) = engine_on(LIMITS) else {
    return;
  };
  let mut reached = [false; limits::ROW_BUSES.len()];
  for frame in 0..FRAMES {
    engine.step().unwrap();
    for (row, bus) in limits::ROW_BUSES.into_iter().enumerate() {
      let voices = audio(&mut engine).voices_on(bus);
      assert!(
        voices <= limits::MAX_VOICES,
        "frame {frame}: {voices} voices on {bus}"
      );
      reached[row] |= voices == limits::MAX_VOICES;
    }
  }
  assert_eq!(reached, [true; 5], "each row reaches its limit");
  let page_max = limits::ROW_BUSES.len() * limits::MAX_VOICES;
  let sfx = audio(&mut engine).voices_on("sfx");
  assert!(sfx <= page_max, "{sfx} voices on sfx, past {page_max}");

  // What each rule did past its limit: refused the play, or stopped a voice
  let log = engine.audio_log();
  let count = |bus: &str, kind: AudioLogKind| {
    log
      .iter()
      .filter(|entry| entry.bus == bus && entry.kind == kind)
      .count()
  };
  let acted: Vec<(usize, usize)> = limits::ROW_BUSES
    .into_iter()
    .map(|bus| {
      (
        count(bus, AudioLogKind::Refused),
        count(bus, AudioLogKind::Stolen),
      )
    })
    .collect();
  let [prevent, oldest, quietest, farthest, priority] = acted[..] else {
    panic!("five rows: {acted:?}");
  };
  assert!(
    prevent.0 > 0 && prevent.1 == 0,
    "PreventNew refuses: {prevent:?}"
  );
  for (rule, row) in [
    ("StopOldest", oldest),
    ("StopQuietest", quietest),
    ("StopFarthest", farthest),
  ] {
    assert!(row.0 == 0 && row.1 > 0, "{rule} stops a voice: {row:?}");
  }
  assert!(
    priority.0 > 0 && priority.1 > 0,
    "StopLowestPriority stops a less important voice, and refuses a play less important than all: {priority:?}"
  );
}

/// The Buses page's sliders start at the game's settings (the demo's
/// `Ryo.toml` sets master to 0.89), a slider sets its setting, and the
/// page puts back what it found as it closes
#[test]
fn a_slider_sets_its_volume_setting() {
  let Some(mut engine) = engine_on(BUSES) else {
    return;
  };
  engine.step_frames(3).unwrap();
  let master = |engine: &mut HeadlessEngine| audio(engine).volume_setting(VolumeKind::Master);
  assert_eq!(master(&mut engine), 0.89, "as the demo's Ryo.toml sets it");
  // The panel has the focus, on its first row: Master, a step of 0.05
  // down
  engine.tap_key(KeyCode::ArrowLeft);
  engine.step_frames(3).unwrap();
  let lowered = master(&mut engine);
  assert!((lowered - 0.85).abs() < 1e-4, "master is {lowered}");
  // The next page starts from the game's mix
  engine.tap_key(KeyCode::PageDown);
  engine.step_frames(3).unwrap();
  assert_eq!(master(&mut engine), 0.89);
}

#[test]
fn space_crossfades_to_the_other_track() {
  let Some(mut engine) = engine_on(MUSIC) else {
    return;
  };
  engine.step_frames(10).unwrap();
  let theme = engine.assets().load::<Music>("music/theme.music.ron");
  let night = engine.assets().load::<Music>("music/night.music.ron");
  assert_eq!(audio(&mut engine).current_music(), Some(theme.id()));
  assert_eq!(audio(&mut engine).voices_on("music"), 1);

  engine.tap_key(KeyCode::Space);
  engine.step_frames(10).unwrap();
  assert_eq!(audio(&mut engine).current_music(), Some(night.id()));
  assert_eq!(
    audio(&mut engine).voices_on("music"),
    2,
    "both play while they crossfade"
  );
  // The crossfade is two seconds
  engine.step_frames(150).unwrap();
  assert_eq!(audio(&mut engine).voices_on("music"), 1);
}

#[test]
fn the_variations_page_takes_a_saved_fifth_take() {
  const FOOTSTEP: &str = "sounds/footstep.sound.ron";
  let files = embedded();
  let Some(mut engine) = engine_over(VARIATIONS, files.clone()) else {
    return;
  };
  engine.step_frames(30).unwrap();
  // As a save in the folder does, with hot reload on: a fifth take
  let saved = text_of(FOOTSTEP).replace(
    r#"(path: "sounds/step-4.wav", weight: 2.0),"#,
    r#"(path: "sounds/step-4.wav", weight: 2.0),
    (path: "sounds/tick.wav", weight: 4.0),"#,
  );
  assert!(saved.contains("tick.wav"), "{saved}");
  files.insert(FOOTSTEP, saved);
  engine.assets().server().reload(FOOTSTEP);
  // The page plays on through the reload, then from a reset, picking the
  // fifth take too
  engine.step_frames(FRAMES).unwrap();
  engine.tap_key(KeyCode::KeyR);
  engine.step_frames(FRAMES).unwrap();
  let fifth = engine
    .audio_log()
    .iter()
    .filter(|entry| entry.kind == AudioLogKind::Start && entry.source == "sounds/tick.wav")
    .count();
  assert!(fifth > 0, "the fifth take plays");
  assert!(warnings().is_empty(), "{:#?}", warnings());
}

/// The hot reload page's count of `path`'s `AssetEvent::Modified`
fn modified(engine: &HeadlessEngine, path: &str) -> Option<u32> {
  engine.world().get_resource::<Reloads>()?.modified(path)
}

#[test]
fn the_hot_reload_page_counts_a_saved_definition() {
  const DRONE: &str = "hot/drone.sound.ron";
  let files = embedded();
  let Some(mut engine) = engine_over(HOT_RELOAD, files.clone()) else {
    return;
  };
  engine.step_frames(3).unwrap();
  assert_eq!(modified(&engine, DRONE), Some(0));

  // As a save in the folder does, with hot reload on
  let saved = text_of(DRONE).replace("volume: -12.0", "volume: -6.0");
  assert!(saved.contains("volume: -6.0"), "{saved}");
  files.insert(DRONE, saved);
  engine.assets().server().reload(DRONE);
  let start = Instant::now();
  while modified(&engine, DRONE) != Some(1) {
    assert!(
      start.elapsed() < Duration::from_secs(10),
      "the reload never counted: {:?}",
      modified(&engine, DRONE)
    );
    engine.step().unwrap();
  }
  assert_eq!(modified(&engine, "sounds/drone.wav"), Some(0));
  assert!(warnings().is_empty(), "{:#?}", warnings());
}
