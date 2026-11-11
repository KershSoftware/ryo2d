//! Headless tests of the lighting demo's pages

use ryo2d::testing::{HeadlessEngine, capture_log};

const PAGES: usize = 1;

#[test]
fn every_page_builds_and_runs_without_a_warning() {
  let log = capture_log();
  for page in 0..PAGES {
    let Some(mut engine) =
      HeadlessEngine::new_or_skip(lighting_demo::demo().start_page(page).engine_builder())
    else {
      return;
    };
    engine.step_frames(120).unwrap();
    let frame = engine.frame().unwrap();
    assert!(frame.width() > 0, "page {page} drew a frame");
  }
  let warnings = log.warnings_and_errors();
  assert!(warnings.is_empty(), "{warnings:?}");
}
