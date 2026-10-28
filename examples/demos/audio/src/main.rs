//! Audio Demo - buses, variations, voice limits, spatial sound, the mix,
//! music and hot reload (the pages are the `audio_demo` library)

fn main() {
  let demo = audio_demo::demo();
  // The web has no file system: the assets are built into the demo
  #[cfg(target_arch = "wasm32")]
  let demo = demo.configure(|b| b.asset_source(audio_demo::embedded()));
  demo.run();
}
