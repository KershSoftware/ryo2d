//! The menu's rain: it starts once its definition has loaded, without the
//! menu waiting for it, plays once however often the menu is entered, and
//! keeps its definition loaded, so saving it retunes the loop

use leap_king::scenes::MenuScene;
use ryo2d::core::audio::{AudioLogEntry, AudioLogKind};
use ryo2d::prelude::*;
use ryo2d::testing::HeadlessEngine;

const RAIN: &str = "rain.sound.ron";

/// The menu, entered afresh `again` more times, 30 frames apart
struct Again {
  menu: MenuScene,
  frames: u32,
  again: u32,
}

impl Again {
  fn new(again: u32) -> Self {
    Self {
      menu: MenuScene::new(),
      frames: 0,
      again,
    }
  }
}

impl Scene for Again {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    self.menu.on_enter(ctx);
  }

  fn on_exit(&mut self, ctx: &mut SceneContext) {
    self.menu.on_exit(ctx);
  }

  fn update(&mut self, ctx: &mut SceneContext, dt: f32) -> SceneAction {
    let action = self.menu.update(ctx, dt);
    self.frames += 1;
    if self.frames == 30 && self.again > 0 {
      return SceneAction::Switch(Box::new(Again::new(self.again - 1)));
    }
    action
  }
}

/// The rain's starts on the ambience bus
fn rain_starts(engine: &HeadlessEngine) -> Vec<AudioLogEntry> {
  engine
    .audio_log()
    .into_iter()
    .filter(|entry| entry.kind == AudioLogKind::Start && entry.sound == RAIN)
    .collect()
}

/// Step `engine` until the rain has started, at most 600 frames
fn until_raining(engine: &mut HeadlessEngine) -> AudioLogEntry {
  for _ in 0..600 {
    if let Some(start) = rain_starts(engine).pop() {
      return start;
    }
    engine.step().unwrap();
  }
  panic!("the rain never started: {:#?}", engine.audio_log());
}

#[test]
fn the_menu_does_not_wait_for_the_rain() {
  let Some(mut engine) = HeadlessEngine::new_or_skip(
    EngineBuilder::new()
      .resolution(1280, 720)
      .start_scene(MenuScene::new()),
  ) else {
    return;
  };
  // The menu has entered and no frame has run: the 24 MB clip is not read
  // on the main thread as it enters
  let clip = engine.assets().load::<AudioClip>("rain.mp3");
  assert!(
    !engine.assets().is_ready(&clip),
    "the menu read the rain as it entered"
  );
  let start = until_raining(&mut engine);
  assert_eq!(start.bus, "ambience");
}

#[test]
fn the_rain_plays_once_however_often_the_menu_is_entered() {
  let Some(mut engine) = HeadlessEngine::new_or_skip(
    EngineBuilder::new()
      .resolution(1280, 720)
      .start_scene(Again::new(3)),
  ) else {
    return;
  };
  until_raining(&mut engine);
  engine.step_frames(120).unwrap();
  assert_eq!(rain_starts(&engine).len(), 1, "one loop");
  let stopped = engine
    .audio_log()
    .into_iter()
    .any(|entry| entry.kind == AudioLogKind::Stop && entry.sound == RAIN);
  assert!(!stopped, "the loop outlives each menu");
}

#[test]
fn saving_the_rain_retunes_its_loop() {
  let rain = |volume: f32| {
    format!(r#"(clips: [(path: "rain.mp3")], bus: "ambience", volume: {volume:.1}, looping: true)"#)
  };
  let clip = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/assets/rain.mp3")).unwrap();
  let files = MemorySource::new();
  files.insert(RAIN, rain(-8.0));
  files.insert("rain.mp3", clip);
  let Some(mut engine) = HeadlessEngine::new_or_skip(
    EngineBuilder::new()
      .resolution(1280, 720)
      .asset_source(files.clone())
      .start_scene(Again::new(1)),
  ) else {
    return;
  };
  let start = until_raining(&mut engine);
  // Past the menu entered again, the definition is still held
  engine.step_frames(60).unwrap();
  files.insert(RAIN, rain(-20.0));
  engine.world().resource::<AssetServer>().reload(RAIN);
  engine.step_frames(60).unwrap();
  let retuned: Vec<f32> = engine
    .audio_log()
    .into_iter()
    .filter(|entry| entry.kind == AudioLogKind::Gain && entry.voice == start.voice)
    .map(|entry| entry.gain_db)
    .collect();
  assert!(
    retuned.iter().any(|gain| (gain + 20.0).abs() < 0.01),
    "the loop took the saved volume: {retuned:?}"
  );
}

#[test]
fn rain_that_does_not_load_is_warned_about_once() {
  // No rain files at all: each entry of the menu asks for the rain
  let Some(mut engine) = HeadlessEngine::new_or_skip(
    EngineBuilder::new()
      .resolution(1280, 720)
      .asset_source(MemorySource::new())
      .start_scene(Again::new(2)),
  ) else {
    return;
  };
  engine.step_frames(120).unwrap();
  let warned = ryo2d::core::dev_log::get_entries()
    .into_iter()
    .filter(|entry| {
      entry.level >= ryo2d::core::dev_log::LogLevel::Warn && entry.message.contains(RAIN)
    })
    .count();
  assert_eq!(warned, 1, "one warning naming the rain");
  assert!(rain_starts(&engine).is_empty());
}
