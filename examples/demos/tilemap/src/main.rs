//! Tilemap Demo - Tiled and LDtk levels, flips, animated tiles, a top-down
//! map, collision, objects, editing, a procgen dungeon, a world of rooms, a
//! big map and hot reload (the pages are the `tilemap_demo` library)

fn main() {
  let demo = tilemap_demo::demo();
  // The web has no file system: the assets are built into the demo
  #[cfg(target_arch = "wasm32")]
  let demo = demo.configure(|b| b.asset_source(tilemap_demo::embedded()));
  demo.run();
}
