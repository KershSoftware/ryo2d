//! A platformer page whose level file is missing says so: the load error is
//! logged

use character_demo::{EMBEDDED, demo};
use ryo2d::prelude::*;
use ryo2d::testing::HeadlessEngine;

#[test]
fn a_page_without_its_level_file_logs_why() {
  let log = ryo2d::testing::capture_log();
  let files = MemorySource::new();
  for (path, bytes) in EMBEDDED {
    if *path != "ldtk/platformer.ldtk" {
      files.insert(path, *bytes);
    }
  }
  let builder = demo()
    .start_page(0)
    .configure(move |b| b.asset_source(files))
    .engine_builder();
  let Some(mut engine) = HeadlessEngine::new_or_skip(builder) else {
    return;
  };
  engine.step_frames(10).unwrap();
  let logged = log.warnings_and_errors();
  assert!(
    logged
      .iter()
      .any(|message| message.contains("ldtk/platformer.ldtk") && message.contains("did not load")),
    "{logged:#?}"
  );
}
