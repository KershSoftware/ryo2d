//! The demo's pages without a window: each runs two seconds without a
//! warning; `game:spawn` and `game:clear` change the world, refuse a bad
//! argument and a count over the cap; `game:say` shows a phrase;
//! `game:paint` exists on one page only and paints its background; and the Log page's keys log at their
//! levels, from the game and from a library target

use console_demo::{LIBRARY_TARGET, MAX_SPAWN, Spawned, demo};
use ryo2d::prelude::*;
use ryo2d::testing::{HeadlessEngine, LogCapture};

const WORLD: usize = 0;
const WORKSHOP: usize = 1;
const LOG: usize = 2;
const PAGES: usize = 3;

/// Two seconds at the demo's 60 Hz
const FRAMES: u32 = 120;

fn engine_on(page: usize) -> Option<HeadlessEngine> {
  HeadlessEngine::new_or_skip(demo().start_page(page).engine_builder())
}

fn warnings(log: &LogCapture) -> Vec<String> {
  log.warnings_and_errors()
}

/// Every live entity (counted by a query: the entity table's own length
/// also counts slots it has reserved)
fn entities(engine: &mut HeadlessEngine) -> usize {
  let world = engine.world_mut();
  world.query::<Entity>().iter(world).count()
}

fn spawned(engine: &mut HeadlessEngine) -> usize {
  let world = engine.world_mut();
  world.query::<&Spawned>().iter(world).count()
}

fn said(lines: &[String]) -> String {
  lines.join("\n")
}

#[test]
fn every_page_runs_two_seconds_without_a_warning() {
  let log = ryo2d::testing::capture_log();
  for page in 0..PAGES {
    let Some(mut engine) = engine_on(page) else {
      return;
    };
    engine.step_frames(FRAMES).unwrap();
    let warned = warnings(&log);
    assert!(warned.is_empty(), "page {}: {warned:#?}", page + 1);
  }
}

#[test]
fn spawn_adds_entities_and_clear_removes_only_those() {
  let Some(mut engine) = engine_on(WORLD) else {
    return;
  };
  engine.step_frames(3).unwrap();
  let before = entities(&mut engine);
  let lines = engine.console_run("game:spawn slime 3");
  assert!(!said(&lines).is_empty());
  assert_eq!(entities(&mut engine), before + 3, "{lines:?}");
  assert_eq!(spawned(&mut engine), 3);
  // One more with no count
  engine.console_run("game:spawn bat");
  assert_eq!(entities(&mut engine), before + 4);
  engine.console_run("game:clear");
  assert_eq!(
    entities(&mut engine),
    before,
    "the page's own entities stay"
  );
  assert_eq!(spawned(&mut engine), 0);
}

#[test]
fn spawn_refuses_a_bad_argument() {
  let Some(mut engine) = engine_on(WORLD) else {
    return;
  };
  engine.step_frames(3).unwrap();
  let before = entities(&mut engine);
  let lines = said(&engine.console_run("game:spawn slime many"));
  assert!(lines.contains("game:spawn <kind> [count]"), "{lines}");
  let lines = said(&engine.console_run("game:spawn dragon 2"));
  assert!(lines.contains("dragon"), "{lines}");
  let lines = said(&engine.console_run(&format!("game:spawn slime {}", MAX_SPAWN + 1)));
  assert!(lines.contains(&MAX_SPAWN.to_string()), "{lines}");
  let lines = said(&engine.console_run("game:spawn"));
  assert!(lines.contains("missing"), "{lines}");
  assert_eq!(entities(&mut engine), before, "nothing was spawned");
  engine.console_run(&format!("game:spawn rock {MAX_SPAWN}"));
  assert_eq!(entities(&mut engine), before + MAX_SPAWN);
}

#[test]
fn say_shows_a_quoted_phrase() {
  let Some(mut engine) = engine_on(WORLD) else {
    return;
  };
  engine.step_frames(3).unwrap();
  let lines = said(&engine.console_run("game:say \"hello there world\""));
  assert!(lines.contains("hello there world"), "{lines}");
  let lines = said(&engine.console_run("game:say"));
  assert!(lines.contains("game:say"), "usage: {lines}");
  engine.step_frames(3).unwrap();
}

#[test]
fn paint_exists_on_the_workshop_page_only() {
  for page in [WORLD, LOG] {
    let Some(mut engine) = engine_on(page) else {
      return;
    };
    engine.step_frames(3).unwrap();
    let lines = said(&engine.console_run("game:paint 10 20 30"));
    assert!(lines.contains("Unknown command"), "page {page}: {lines}");
  }
  let Some(mut engine) = engine_on(WORKSHOP) else {
    return;
  };
  engine.step_frames(3).unwrap();
  let painted = [10, 20, 30];
  assert!(background_share(&engine, painted) < 0.01, "not painted yet");
  let lines = said(&engine.console_run("game:paint 10 20 30"));
  assert!(!lines.contains("Unknown command"), "{lines}");
  assert!(lines.contains("10"), "{lines}");
  // The other page's commands are not here
  let lines = said(&engine.console_run("game:spawn slime"));
  assert!(lines.contains("Unknown command"), "{lines}");
  // The page's background takes the colour painted
  engine.step_frames(2).unwrap();
  let share = background_share(&engine, painted);
  assert!(share > 0.25, "{share} of the frame is the painted colour");
  let lines = said(&engine.console_run("game:paint 10 20 999"));
  assert!(
    lines.contains(r#"argument 3 ("999") is not a number; usage: game:paint <r> <g> <b>"#),
    "{lines}"
  );
  // A refused paint leaves the background as it was
  engine.step_frames(2).unwrap();
  assert_eq!(background_share(&engine, painted), share);
}

/// The part of the last frame within 2 of `rgb` in each channel
fn background_share(engine: &HeadlessEngine, rgb: [u8; 3]) -> f32 {
  let frame = engine.frame().unwrap();
  let near = frame
    .pixels()
    .filter(|pixel| {
      pixel.0[..3]
        .iter()
        .zip(rgb)
        .all(|(&got, want)| got.abs_diff(want) <= 2)
    })
    .count();
  near as f32 / (frame.width() * frame.height()) as f32
}

#[test]
fn the_log_pages_keys_log_at_their_levels() {
  let log = ryo2d::testing::capture_log();
  let Some(mut engine) = engine_on(LOG) else {
    return;
  };
  engine.step_frames(3).unwrap();
  assert!(warnings(&log).is_empty());
  for key in [
    KeyCode::Digit3,
    KeyCode::Digit4,
    KeyCode::Digit5,
    KeyCode::Digit6,
  ] {
    engine.tap_key(key);
    engine.step_frames(2).unwrap();
  }
  let warned = warnings(&log);
  assert!(
    warned.iter().any(|w| w.contains("game warning")),
    "{warned:#?}"
  );
  assert!(
    warned.iter().any(|w| w.contains("game error")),
    "{warned:#?}"
  );
  let library = log.from_target(LIBRARY_TARGET);
  assert_eq!(library.len(), 1, "{library:?}");
  assert_eq!(library[0].level, log::Level::Warn);
  // One press logs once, not every frame
  engine.step_frames(30).unwrap();
  assert_eq!(warnings(&log).len(), warned.len());
}
