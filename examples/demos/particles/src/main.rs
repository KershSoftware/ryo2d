//! Particles Demo - every preset effect, emitters and attractors (the pages
//! are the `particles_demo` library)

fn main() {
  let demo = particles_demo::demo();
  let demo = demo.configure(|b| b.project(ryo2d::project!()));
  // The web has no file system: the assets are built into the demo
  #[cfg(target_arch = "wasm32")]
  let demo = demo.configure(|b| b.asset_source(particles_demo::embedded()));
  demo.run();
}
