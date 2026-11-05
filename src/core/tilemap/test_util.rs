//! Shared helpers for the tilemap tests.

use std::path::Path;

use crate::core::asset::{AssetServer, AssetsView, Handle, MemorySource, insert_registry, pump};
use crate::core::ecs::{Entity, IntoScheduleConfigs, World};
use crate::core::physics::test_util::world as physics_world;
use crate::core::schedule::{EngineSchedules, Phase};

pub(crate) const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/tilemap");

/// Every file under `dir` at its path relative to `root`, the XML twins left out
fn read_tree(root: &Path, dir: &Path, files: &MemorySource) {
  let Ok(entries) = std::fs::read_dir(dir) else {
    panic!("{dir:?} is not readable")
  };
  for entry in entries.flatten() {
    let path = entry.path();
    if path.is_dir() {
      if path.file_name().is_some_and(|n| n == "xml") {
        continue;
      }
      read_tree(root, &path, files);
    } else if let (Ok(rel), Ok(bytes)) = (path.strip_prefix(root), std::fs::read(&path)) {
      files.insert(&rel.to_string_lossy().replace('\\', "/"), bytes);
    }
  }
}

/// The tilemap fixtures (under `tiled/` and `ldtk/`) plus `extra` files
pub(crate) fn files_with(extra: &[(&str, &str)]) -> MemorySource {
  let files = MemorySource::new();
  read_tree(Path::new(FIXTURES), Path::new(FIXTURES), &files);
  for (path, text) in extra {
    files.insert(path, *text);
  }
  files
}

/// A world with the registry over `files` and nothing else
pub(crate) fn world_over(files: MemorySource) -> World {
  let mut world = World::new();
  insert_registry(&mut world, AssetServer::new(files));
  world
}

/// A physics world over the tilemap fixtures with the engine's map systems:
/// spawning, reloading, streaming and class tiles following chunks in
/// `PreUpdate`,
/// collision in the fixed step, occluders in `PostUpdate`, and physics
/// forgetting what was removed in `Last`
pub(crate) struct MapWorld {
  pub(crate) world: World,
  schedules: EngineSchedules,
}

impl MapWorld {
  pub(crate) fn new() -> MapWorld {
    MapWorld::over(files_with(&[]))
  }

  /// A map world over `files`, watching them: a file put over one already
  /// there reloads what was loaded from it
  pub(crate) fn over(files: MemorySource) -> MapWorld {
    let maps = MapWorld::unwatched(files);
    maps.world.resource::<AssetServer>().start_watching();
    maps
  }

  /// A map world over `files` that does not watch them: a test reloads
  /// what it chooses (`AssetServer::reload`)
  pub(crate) fn unwatched(files: MemorySource) -> MapWorld {
    let (mut world, mut schedules) = physics_world(60);
    insert_registry(&mut world, AssetServer::new(files));
    schedules.add_systems(
      Phase::PreUpdate,
      (
        super::spawn_maps,
        super::reload_maps,
        super::stream_maps,
        super::follow_chunk_swaps,
      )
        .chain(),
    );
    super::register_tilemap_systems(&mut schedules);
    schedules.add_systems(
      Phase::Last,
      (
        crate::core::physics::sync::forget_removed,
        crate::core::physics::sync::track_disabled,
      ),
    );
    MapWorld { world, schedules }
  }

  /// A frame with one fixed step: the pump and the spawn, the step, then
  /// `PostUpdate` and `Last`
  pub(crate) fn frame(&mut self) {
    self.pre_update();
    self.step();
    self.post_update();
    self.last();
  }

  /// The pump and `PreUpdate` (the spawn) alone
  pub(crate) fn pre_update(&mut self) {
    pump(&mut self.world);
    self.schedules.run(Phase::PreUpdate, &mut self.world);
  }

  /// The schedules, for a test's own systems
  pub(crate) fn schedules(&mut self) -> &mut EngineSchedules {
    &mut self.schedules
  }

  /// One fixed step alone
  pub(crate) fn step(&mut self) {
    self.schedules.run(Phase::FixedUpdate, &mut self.world);
  }

  /// `PostUpdate` alone
  pub(crate) fn post_update(&mut self) {
    self.schedules.run(Phase::PostUpdate, &mut self.world);
  }

  /// `Last` alone
  pub(crate) fn last(&mut self) {
    self.schedules.run(Phase::Last, &mut self.world);
  }

  /// Runs frames until `done`, failing after 5 s: reloads and landings are
  /// worked out on other threads
  pub(crate) fn frames_until(&mut self, mut done: impl FnMut(&mut World) -> bool) {
    let start = std::time::Instant::now();
    loop {
      self.frame();
      if done(&mut self.world) {
        return;
      }
      assert!(
        start.elapsed() < std::time::Duration::from_secs(5),
        "timed out waiting"
      );
      std::thread::sleep(std::time::Duration::from_millis(1));
    }
  }

  /// Spawns a map entity over `root` at the origin and runs frames until
  /// it is built (`settle`)
  pub(crate) fn spawn(&mut self, root: super::MapRoot) -> Entity {
    let map = self.world.spawn(root).id();
    self.settle(map);
    map
  }

  /// Runs frames until `map` has landed: its landing is worked out on
  /// another thread
  pub(crate) fn settle(&mut self, map: Entity) {
    let start = std::time::Instant::now();
    loop {
      self.frame();
      if super::MapsView::new(&mut self.world).is_ready(map) {
        return;
      }
      assert!(
        start.elapsed() < std::time::Duration::from_secs(5),
        "map {map} did not land in 5 s"
      );
      std::thread::yield_now();
    }
  }
}

/// A map world over the fixtures with `xml` (fixture files under
/// `tiled/xml/`, which the fixtures leave out) added, and the map at `file`
/// spawned and landed, with its grid
pub(crate) fn world_with_xml(xml: &[&str], file: &str) -> (MapWorld, Entity, super::MapGrid) {
  let texts: Vec<(&str, String)> = xml
    .iter()
    .map(|&path| {
      let full = format!("{FIXTURES}/{path}");
      let text = std::fs::read_to_string(&full).unwrap_or_else(|e| panic!("{full}: {e}"));
      (path, text)
    })
    .collect();
  let extra: Vec<(&str, &str)> = texts
    .iter()
    .map(|(path, text)| (*path, text.as_str()))
    .collect();
  let mut maps = MapWorld::over(files_with(&extra));
  let handle = load_map(&mut maps.world, file);
  let grid = maps
    .world
    .resource::<crate::core::asset::Assets<super::TileMap>>()
    .get(&handle)
    .map(super::TileMap::grid)
    .expect("the map loaded");
  let map = maps.spawn(super::MapRoot::new(handle));
  maps.frame();
  (maps, map, grid)
}

/// The map at `path`, loaded at once
pub(crate) fn load_map(world: &mut World, path: &str) -> Handle<super::TileMap> {
  match AssetsView::new(world).load_now::<super::TileMap>(path) {
    Ok(handle) => handle,
    Err(error) => panic!("{path}: {}", error.report()),
  }
}

/// The map's layer entities, by index
pub(crate) fn layers_of(world: &mut World, map: Entity) -> Vec<Entity> {
  let mut found: Vec<(usize, Entity)> = world
    .query::<(Entity, &super::MapLayerRef)>()
    .iter(world)
    .filter(|(_, l)| l.map == map)
    .map(|(e, l)| (l.index, e))
    .collect();
  found.sort();
  found.into_iter().map(|(_, e)| e).collect()
}

/// Whether every map root in `world` has landed (or reported a failure)
pub(crate) fn maps_landed(world: &mut World) -> bool {
  world
    .query_filtered::<
      (Option<&super::MapIds>, Option<&super::spawn::MapFailed>),
      crate::core::ecs::With<super::MapRoot>,
    >()
    .iter(world)
    .all(|(ids, failed)| ids.is_some() || failed.is_some())
}

/// Steps `engine` until every map root has landed: a map's landing is
/// worked out on another thread
pub(crate) fn step_until_maps_land(engine: &mut crate::core::testing::HeadlessEngine) {
  let start = std::time::Instant::now();
  while !maps_landed(engine.world_mut()) {
    engine.step().expect("a frame");
    assert!(
      start.elapsed() < std::time::Duration::from_secs(5),
      "a map did not land in 5 s"
    );
    std::thread::yield_now();
  }
}

/// A tileset for `grid` (a hexagonal one) of tiles its cell's size: tile 0
/// whose collision shape is its whole hexagon, in the image's pixels as a
/// tile standing in its cell draws it
pub(crate) fn whole_hex_set(grid: &super::MapGrid) -> super::Tileset {
  use glam::{IVec2, Vec2};
  let mut set = super::Tileset::new("hexes", grid.cell, super::TilesetImage::Collection);
  // The image's top-left as it stands in cell (0, 0)
  let top_left = grid.tile_anchor(IVec2::ZERO) - Vec2::new(0.0, grid.cell.y as f32);
  let points: Vec<Vec2> = grid
    .cell_outline(IVec2::ZERO)
    .points()
    .iter()
    .map(|&p| p - top_left)
    .collect();
  set.insert_tile(
    0,
    super::TileData {
      shapes: vec![super::TileShape {
        points,
        ..super::TileShape::default()
      }],
      ..super::TileData::default()
    },
  );
  set
}

/// A point of whole pixels
pub(crate) type Px = (i32, i32);

/// `p` as whole pixels, if it is one
pub(crate) fn whole_px(p: glam::Vec2) -> Option<Px> {
  ((p - p.round()).abs().max_element() < 1e-3).then(|| (p.x.round() as i32, p.y.round() as i32))
}

/// The border of the union of `cells` on `grid` (a grid whose corners are
/// whole pixels), from their outlines: each side no other of them shares,
/// clockwise round them, sorted
pub(crate) fn union_border(grid: &super::MapGrid, cells: &[glam::IVec2]) -> Vec<(Px, Px)> {
  let mut sides = Vec::new();
  for &cell in cells {
    let outline = grid.cell_outline(cell);
    let points: Vec<Px> = outline
      .points()
      .iter()
      .map(|&p| whole_px(p).expect("a corner on a whole pixel"))
      .collect();
    for i in 0..points.len() {
      let (a, b) = (points[i], points[(i + 1) % points.len()]);
      if a != b {
        sides.push((a, b));
      }
    }
  }
  let mut border: Vec<(Px, Px)> = sides
    .iter()
    .copied()
    .filter(|&(a, b)| !sides.contains(&(b, a)))
    .collect();
  border.sort_unstable();
  border
}
