//! A platformer page whose level file is missing says so: the load error is
//! in the dev log. Its own test binary, as the dev log is the process's and
//! `pages.rs` expects it to hold no warning

use character_demo::{EMBEDDED, demo};
use ryo2d::core::dev_log::{self, LogLevel};
use ryo2d::prelude::*;
use ryo2d::testing::HeadlessEngine;

#[test]
fn a_page_without_its_level_file_logs_why() {
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
  let logged: Vec<String> = dev_log::get_entries()
    .into_iter()
    .filter(|entry| entry.level >= LogLevel::Warn)
    .map(|entry| entry.message)
    .collect();
  assert!(
    logged
      .iter()
      .any(|message| message.contains("ldtk/platformer.ldtk") && message.contains("did not load")),
    "{logged:#?}"
  );
}
