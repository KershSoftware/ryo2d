//! Character Demo - the controller and its presets on a tile level,
//! top-down movement, and custom movement (the pages are the
//! `character_demo` library)

fn main() {
  let demo = character_demo::demo();
  // The web has no file system: the assets are built into the demo
  #[cfg(target_arch = "wasm32")]
  let demo = demo.configure(|b| b.asset_source(character_demo::embedded()));
  demo.run();
}
