//! Audio in a running engine: a headless engine plays on kira's mock
//! backend, silently, and its audio log records what played

use std::sync::{Arc, Mutex};

use ryo2d::core::audio::AudioLogKind;
use ryo2d::prelude::*;
use ryo2d::testing::HeadlessEngine;

const TONE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/audio/tone.wav");

/// Plays `tone.wav` as it enters, keeping the voice where the test sees it
struct PlaysOnEnter {
  voice: Arc<Mutex<Option<Voice>>>,
}

impl Scene for PlaysOnEnter {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    let clip = ctx
      .assets()
      .load_now::<AudioClip>("tone.wav")
      .expect("tone.wav loads");
    *self.voice.lock().unwrap() = Some(ctx.audio().play(&clip));
  }
}

#[test]
fn a_headless_engine_plays_on_the_mock() {
  let files = MemorySource::new();
  files.insert("tone.wav", std::fs::read(TONE).expect("the fixture reads"));
  let voice = Arc::new(Mutex::new(None));
  let scene = PlaysOnEnter {
    voice: voice.clone(),
  };
  let Some(mut engine) = HeadlessEngine::new_or_skip(
    EngineBuilder::new()
      .resolution(320, 240)
      .asset_source(files)
      .start_scene(scene),
  ) else {
    return;
  };
  engine.step().unwrap();
  let log = engine.audio_log();
  assert!(
    log.iter().any(|entry| entry.kind == AudioLogKind::Start
      && entry.source == "tone.wav"
      && entry.bus == "sfx"),
    "{log:#?}"
  );
  let voice = voice.lock().unwrap().clone().expect("the scene played");
  assert!(voice.is_playing());
  // The 250 ms tone ends as the engine's frames render it on the mock
  engine.step_frames(30).unwrap();
  assert!(voice.is_stopped(), "{voice:?}");
}
