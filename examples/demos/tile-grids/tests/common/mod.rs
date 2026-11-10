//! What the page tests share: an engine on a page, a capture's warnings,
//! the maps that have landed and waiting for one, a layer's tiles, and a
//! click

use ryo2d::prelude::*;
use ryo2d::testing::{HeadlessEngine, LogCapture};
use tile_grids_demo::demo;

pub fn engine_on(page: usize) -> Option<HeadlessEngine> {
  HeadlessEngine::new_or_skip(demo().start_page(page).engine_builder())
}

/// Every warning and error `log` captured
pub fn warnings(log: &LogCapture) -> Vec<String> {
  log.warnings_and_errors()
}

/// The layer names of each map that has landed, bottom first, and how many
/// roots there are
pub fn landed_maps(engine: &mut HeadlessEngine) -> (Vec<Vec<String>>, usize) {
  let world = engine.world_mut();
  let mut roots = world.query::<(Entity, Has<MapIds>, &MapRoot)>();
  let roots: Vec<(Entity, bool)> = roots
    .iter(world)
    .map(|(entity, landed, _)| (entity, landed))
    .collect();
  let mut layers = world.query::<&MapLayerRef>();
  let landed = roots
    .iter()
    .filter(|(_, landed)| *landed)
    .map(|(root, _)| {
      // By index, bottom first: a query's order is the world's layout
      let mut found: Vec<(usize, String)> = layers
        .iter(world)
        .filter(|layer| layer.map == *root)
        .map(|layer| (layer.index, layer.name.clone()))
        .collect();
      found.sort();
      found.into_iter().map(|(_, name)| name).collect()
    })
    .collect();
  (landed, roots.len())
}

/// A step, then a millisecond's rest: a map's padded tilesets are made off
/// the main thread, a few hundred milliseconds for big pictures, which a
/// headless run stepping as fast as it can outpaces
pub fn step_resting(engine: &mut HeadlessEngine) {
  engine.step().unwrap();
  std::thread::sleep(std::time::Duration::from_millis(1));
}

/// The most steps a page's map may take to land (about 20 s)
pub const LANDING: u32 = 15_000;

/// The root of the page's one map, once it has landed
pub fn spawned_map(log: &LogCapture, engine: &mut HeadlessEngine) -> Entity {
  for _ in 0..LANDING {
    step_resting(engine);
    let world = engine.world_mut();
    let mut roots = world.query_filtered::<Entity, (With<MapRoot>, With<MapIds>)>();
    if let Some(root) = roots.iter(world).next() {
      return root;
    }
  }
  panic!("the page's map never landed: {:#?}", warnings(log));
}

/// A copy of the spawned layer `name`'s grid
pub fn layer_grid(engine: &mut HeadlessEngine, map: Entity, name: &str) -> TileGrid {
  let layer = MapsView::new(engine.world_mut())
    .layer_entity(map, name)
    .unwrap_or_else(|| panic!("no layer {name:?}"));
  engine
    .world()
    .get::<TileLayerData>(layer)
    .map(|data| data.tiles.clone())
    .unwrap_or_else(|| panic!("layer {name:?} has no tiles"))
}

pub fn click(engine: &mut HeadlessEngine, at: Vec2, button: MouseButton) {
  engine.mouse_move(at.x, at.y);
  engine.step().unwrap();
  engine.press_mouse(button);
  engine.step_frames(2).unwrap();
  engine.release_mouse(button);
  engine.step_frames(2).unwrap();
}
