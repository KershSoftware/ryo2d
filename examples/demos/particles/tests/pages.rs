//! The demo's pages without a window: each runs for 120 frames with
//! particles alive and no warning in the dev log, and a click on the
//! Explosions page plays an explosion whose emitter despawns itself once
//! its last particle dies

use particles_demo::demo;
use ryo2d::core::dev_log::{self, LogLevel};
use ryo2d::prelude::*;
use ryo2d::testing::HeadlessEngine;

const EXPLOSIONS: usize = 2;
const PAGES: usize = 10;

/// The frames each page runs
const FRAMES: u32 = 120;

fn engine_on(page: usize) -> Option<HeadlessEngine> {
  HeadlessEngine::new_or_skip(demo().start_page(page).engine_builder())
}

/// Every warning in the dev log. The log is the process's, so the tests
/// share it
fn warnings() -> Vec<String> {
  dev_log::get_entries()
    .into_iter()
    .filter(|entry| entry.level >= LogLevel::Warn)
    .map(|entry| format!("{}: {}", entry.module.unwrap_or_default(), entry.message))
    .collect()
}

/// The emitter entities in the engine's world
fn emitters(engine: &mut HeadlessEngine) -> usize {
  let world = engine.world_mut();
  world.query::<&ParticleEmitter>().iter(world).count()
}

#[test]
fn every_page_runs_with_particles_and_without_a_warning() {
  for page in 0..PAGES {
    let Some(mut engine) = engine_on(page) else {
      return;
    };
    engine.step_frames(FRAMES).unwrap();
    let live = ParticlesView::new(engine.world_mut()).count();
    assert!(live > 0, "page {}: no particles alive", page + 1);
    let warned = warnings();
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
