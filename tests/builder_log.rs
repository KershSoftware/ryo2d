//! The builder installs the engine's log. A process of its own: the log
//! installs once per process, so nothing else may run here first.

#[test]
fn the_builder_installs_the_log() {
  use ryo2d::logging::log;
  assert_eq!(
    log::max_level(),
    log::LevelFilter::Off,
    "nothing installed yet"
  );
  let _builder = ryo2d::prelude::EngineBuilder::new();
  assert_ne!(log::max_level(), log::LevelFilter::Off);
}
