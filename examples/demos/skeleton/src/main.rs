//! Skeleton Demo - a cutout hero, page by page (the pages are the
//! `skeleton_demo` library)

fn main() {
  let demo = skeleton_demo::demo();
  let demo = demo.configure(|b| b.project(ryo2d::project!()));
  // The web has no file system: the assets are built into the demo
  #[cfg(target_arch = "wasm32")]
  let demo = demo.configure(|b| b.asset_source(skeleton_demo::embedded()));
  demo.run();
}
