//! The demo's pages without a window, on the mock audio backend: each page
//! plays for two seconds without a warning, from the demo's folder and from
//! the files the web build embeds; the limits page keeps each rule's row
//! within its voices; a slider sets its volume setting; the music page
//! crossfades; and the hot reload page counts a saved definition

use std::time::{Duration, Instant};

use audio_demo::{demo, embedded, limits};
use ryo2d::core::dev_log::{self, LogLevel};
use ryo2d::prelude::*;
use ryo2d::testing::HeadlessEngine;

/// The pages, in the demo's order
const BUSES: usize = 0;
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

/// Step every page for two seconds, checking each plays something and none
/// warns
fn every_page_plays(mut engine_on: impl FnMut(usize) -> Option<HeadlessEngine>) {
  for page in 0..PAGES {
    let Some(mut engine) = engine_on(page) else {
      return;
    };
    engine.step_frames(FRAMES).unwrap();
    let playing = audio(&mut engine).voices_on("master");
    assert!(playing > 0, "page {} plays nothing", page + 1);
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
fn each_limit_row_keeps_within_its_voices() {
  let Some(mut engine) = engine_on(LIMITS) else {
    return;
  };
  for frame in 0..FRAMES {
    engine.step().unwrap();
    for bus in limits::ROW_BUSES {
      let voices = audio(&mut engine).voices_on(bus);
      assert!(
        voices <= limits::MAX_VOICES,
        "frame {frame}: {voices} voices on {bus}"
      );
    }
  }
  let page_max = limits::ROW_BUSES.len() * limits::MAX_VOICES;
  let sfx = audio(&mut engine).voices_on("sfx");
  assert!(sfx <= page_max, "{sfx} voices on sfx, past {page_max}");
  for bus in limits::ROW_BUSES {
    assert!(
      audio(&mut engine).voices_on(bus) > 0,
      "nothing plays on {bus}"
    );
  }
}

#[test]
fn a_slider_sets_its_volume_setting() {
  let Some(mut engine) = engine_on(BUSES) else {
    return;
  };
  engine.step_frames(3).unwrap();
  let master = VolumeKind::Master.default_value();
  assert_eq!(
    audio(&mut engine).volume_setting(VolumeKind::Master),
    master
  );
  // The panel has the focus, on its first row: Master
  engine.tap_key(KeyCode::ArrowLeft);
  engine.step_frames(3).unwrap();
  let lowered = audio(&mut engine).volume_setting(VolumeKind::Master);
  assert!((lowered - 0.85).abs() < 1e-4, "master is {lowered}");
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

/// The text drawn after `label`: a row's count
fn after<'a>(texts: &'a [String], label: &str) -> Option<&'a str> {
  let at = texts.iter().position(|text| text == label)?;
  texts.get(at + 1).map(String::as_str)
}

#[test]
fn the_hot_reload_page_counts_a_saved_definition() {
  const DRONE: &str = "hot/drone.sound.ron";
  let files = embedded();
  let Some(mut engine) = engine_over(HOT_RELOAD, files.clone()) else {
    return;
  };
  engine.step_frames(3).unwrap();
  assert_eq!(after(&texts(&engine), DRONE), Some("modified 0"));

  // As a save in the folder does, with hot reload on
  let saved = audio_demo::EMBEDDED
    .iter()
    .find(|(path, _)| *path == DRONE)
    .map(|(_, bytes)| String::from_utf8_lossy(bytes).replace("volume: -12.0", "volume: -6.0"))
    .unwrap();
  assert!(saved.contains("volume: -6.0"), "{saved}");
  files.insert(DRONE, saved);
  engine.assets().server().reload(DRONE);
  let start = Instant::now();
  while after(&texts(&engine), DRONE) != Some("modified 1") {
    assert!(
      start.elapsed() < Duration::from_secs(10),
      "the reload never counted: {:?}",
      texts(&engine)
    );
    engine.step().unwrap();
  }
  assert_eq!(
    after(&texts(&engine), "sounds/drone.wav"),
    Some("modified 0")
  );
  assert!(warnings().is_empty(), "{:#?}", warnings());
}
