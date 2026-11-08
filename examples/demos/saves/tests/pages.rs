//! The demo's pages without a window: each runs for two seconds without a
//! warning; the Slots page saves, lists and deletes; the Upgrades page loads
//! a version 1 save as version 3; the Recovery page loads the backup of a
//! damaged slot; and a slider on the Settings page changes the settings.
//! Saves and settings live in the headless engine's memory store, never on
//! disk

use std::time::{Duration, Instant};

use ryo2d::core::dev_log::{self, LogLevel};
use ryo2d::prelude::*;
use ryo2d::testing::HeadlessEngine;
use saves_demo::{DemoOptions, HeroV3, OLD_SLOT, SLOTS, demo};

const SLOTS_PAGE: usize = 0;
const UPGRADES: usize = 1;
const RECOVERY: usize = 2;
const SETTINGS: usize = 3;
const PAGES: usize = 4;

/// Two seconds at the demo's 60 Hz
const FRAMES: u32 = 120;

fn engine_on(page: usize) -> Option<HeadlessEngine> {
  HeadlessEngine::new_or_skip(demo().start_page(page).engine_builder())
}

/// Every warning in the dev log except the backup warning the Recovery test
/// causes on purpose. The log is the process's, so the tests share it
fn warnings() -> Vec<String> {
  dev_log::get_entries()
    .into_iter()
    .filter(|entry| entry.level >= LogLevel::Warn)
    .filter(|entry| !entry.message.contains("its backup"))
    .map(|entry| entry.message)
    .collect()
}

/// Steps until `done`, giving the save writer's thread up to five seconds
fn step_until(engine: &mut HeadlessEngine, mut done: impl FnMut(&mut HeadlessEngine) -> bool) {
  let start = Instant::now();
  while !done(engine) {
    assert!(start.elapsed() < Duration::from_secs(5), "timed out");
    engine.step_frames(2).unwrap();
    std::thread::sleep(Duration::from_millis(5));
  }
}

fn has_file(engine: &HeadlessEngine, name: &str) -> bool {
  engine.saves().names().iter().any(|n| n == name)
}

fn load(engine: &HeadlessEngine, slot: &str) -> Result<Loaded<HeroV3>, SaveError> {
  SavesView::new(engine.world()).load::<HeroV3>(slot)
}

#[test]
fn every_page_runs_two_seconds_without_a_warning() {
  for page in 0..PAGES {
    let Some(mut engine) = engine_on(page) else {
      return;
    };
    engine.step_frames(FRAMES).unwrap();
    let warned = warnings();
    assert!(warned.is_empty(), "page {}: {warned:#?}", page + 1);
  }
}

#[test]
fn the_slots_page_saves_lists_and_deletes() {
  let Some(mut engine) = engine_on(SLOTS_PAGE) else {
    return;
  };
  engine.step_frames(3).unwrap();
  assert!(engine.saves().names().is_empty());
  // The panel's Slot row starts on slot-a; Space saves it
  engine.tap_key(KeyCode::Space);
  step_until(&mut engine, |e| has_file(e, "slot-a.ryo-save"));
  let listed = SavesView::new(engine.world()).list().unwrap();
  assert_eq!(listed.len(), 1);
  assert_eq!(listed[0].slot, SLOTS[0]);
  assert!(listed[0].header.name.is_some());
  assert!(load(&engine, "slot-a").is_ok());
  // Down to the next slot row value: Right on the panel's first row
  engine.tap_key(KeyCode::ArrowRight);
  engine.step_frames(3).unwrap();
  engine.tap_key(KeyCode::Space);
  step_until(&mut engine, |e| has_file(e, "slot-b.ryo-save"));
  engine.tap_key(KeyCode::KeyX);
  step_until(&mut engine, |e| !has_file(e, "slot-b.ryo-save"));
  assert!(has_file(&engine, "slot-a.ryo-save"));
  assert!(warnings().is_empty(), "{:#?}", warnings());
}

#[test]
fn the_upgrades_page_loads_version_three_from_version_one() {
  let Some(mut engine) = engine_on(UPGRADES) else {
    return;
  };
  step_until(&mut engine, |e| has_file(e, "hero-old.ryo-save"));
  let header = SavesView::new(engine.world()).info(OLD_SLOT).unwrap();
  assert_eq!(header.version, 1);
  let loaded = load(&engine, OLD_SLOT).unwrap();
  assert_eq!(loaded.upgraded_from, Some(1));
  assert_eq!(loaded.data.name, "Rowan");
  assert_eq!(loaded.data.max_hp, 30);
  assert_eq!(loaded.data.class, "Wanderer");
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn the_recovery_page_loads_the_backup_after_damage() {
  let Some(mut engine) = engine_on(RECOVERY) else {
    return;
  };
  engine.step_frames(3).unwrap();
  // S saves slot-a twice, so the second replace leaves a backup
  engine.tap_key(KeyCode::KeyS);
  step_until(&mut engine, |e| has_file(e, "slot-a.ryo-save.bak"));
  engine.saves().put("slot-a.ryo-save", b"garbage".to_vec());
  engine.tap_key(KeyCode::KeyL);
  engine.step_frames(3).unwrap();
  let loaded = load(&engine, "slot-a").unwrap();
  assert!(loaded.from_backup);
}

#[test]
fn the_settings_page_slider_changes_settings() {
  let Some(mut engine) = engine_on(SETTINGS) else {
    return;
  };
  engine.step_frames(3).unwrap();
  let master = |engine: &HeadlessEngine| {
    engine
      .world()
      .resource::<Settings>()
      .volume(VolumeKind::Master)
  };
  let before = master(&engine);
  // The panel has the focus on its first row, Master, in steps of 0.05
  engine.tap_key(KeyCode::ArrowLeft);
  engine.step_frames(3).unwrap();
  let lowered = master(&engine);
  assert!((lowered - 0.85).abs() < 1e-4, "{before} -> {lowered}");
  // The game's own section: Subtitles is the seventh row
  for _ in 0..6 {
    engine.tap_key(KeyCode::ArrowDown);
    engine.step_frames(3).unwrap();
  }
  engine.tap_key(KeyCode::ArrowRight);
  engine.step_frames(3).unwrap();
  let settings = engine.world().resource::<Settings>();
  assert!(settings.game::<DemoOptions>().subtitles);
  assert!((settings.volume(VolumeKind::Master) - lowered).abs() < 1e-6);
}
