//! The demo's pages without a window: each runs for 120 frames with
//! particles alive and no warning logged, and a click on the
//! Explosions page plays an explosion whose emitter despawns itself once
//! its last particle dies, and the Files page plays the effects of its files

use particles_demo::{demo, embedded};
use ryo2d::prelude::*;
use ryo2d::testing::{HeadlessEngine, LogCapture};

const EXPLOSIONS: usize = 2;
const PAGES: usize = 14;
const FILES: usize = 13;

/// The frames each page runs
const FRAMES: u32 = 120;

fn engine_on(page: usize) -> Option<HeadlessEngine> {
  HeadlessEngine::new_or_skip(demo().start_page(page).engine_builder())
}

/// Every warning and error `log` captured
fn warnings(log: &LogCapture) -> Vec<String> {
  log.warnings_and_errors()
}

/// The emitter entities in the engine's world
fn emitters(engine: &mut HeadlessEngine) -> usize {
  let world = engine.world_mut();
  world.query::<&ParticleEmitter>().iter(world).count()
}

#[test]
fn every_page_runs_with_particles_and_without_a_warning() {
  let log = ryo2d::testing::capture_log();
  for page in 0..PAGES {
    let Some(mut engine) = engine_on(page) else {
      return;
    };
    engine.step_frames(FRAMES).unwrap();
    let live = ParticlesView::new(engine.world_mut()).count();
    assert!(live > 0, "page {}: no particles alive", page + 1);
    let warned = warnings(&log);
    assert!(warned.is_empty(), "page {}: {warned:#?}", page + 1);
  }
}

#[test]
fn a_click_plays_an_explosion_that_despawns_when_it_finishes() {
  let Some(mut engine) = engine_on(EXPLOSIONS) else {
    return;
  };
  engine.step_frames(3).unwrap();
  let before = emitters(&mut engine);
  engine.mouse_move(640.0, 300.0);
  engine.press_mouse(MouseButton::Left);
  engine.step().unwrap();
  engine.release_mouse(MouseButton::Left);
  engine.step_frames(2).unwrap();
  assert_eq!(emitters(&mut engine), before + 1, "the click's explosion");
  // Its one cycle is a second, and its sparks live at most 0.8 s, at 144 Hz
  engine.step_frames(2 * 144 + 20).unwrap();
  assert_eq!(emitters(&mut engine), before, "the explosion despawned");
}

#[test]
fn the_files_page_plays_the_effects_of_its_files() {
  let Some(mut engine) = engine_on(FILES) else {
    return;
  };
  engine.step_frames(FRAMES).unwrap();
  let world = engine.world_mut();
  let effects: Vec<Handle<ParticleEffect>> = world
    .query::<&ParticleEmitter>()
    .iter(world)
    .map(|emitter| emitter.effect.clone())
    .collect();
  assert!(effects.len() >= 4, "an emitter per file: {}", effects.len());
  let loaded = world.resource::<Assets<ParticleEffect>>();
  for effect in &effects {
    assert!(loaded.get(effect).is_some(), "{effect:?} is loaded");
  }
}

#[test]
fn the_embedded_files_play_the_files_page_as_the_web_does() {
  let log = ryo2d::testing::capture_log();
  let builder = demo().start_page(FILES).engine_builder();
  let Some(mut engine) = HeadlessEngine::new_or_skip(builder.asset_source(embedded())) else {
    return;
  };
  engine.step_frames(FRAMES).unwrap();
  assert!(ParticlesView::new(engine.world_mut()).count() > 0);
  let warned = warnings(&log);
  assert!(warned.is_empty(), "{warned:#?}");
}
