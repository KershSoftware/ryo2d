//! Shared helpers for the tilemap tests.

use std::path::Path;

use crate::core::asset::{AssetServer, AssetsView, Handle, MemorySource, insert_registry, pump};
use crate::core::ecs::{Entity, World};
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
/// spawning in `PreUpdate`, collision in the fixed step, occluders in
/// `PostUpdate`, and physics forgetting what was removed in `Last`
pub(crate) struct MapWorld {
  pub(crate) world: World,
  schedules: EngineSchedules,
}

impl MapWorld {
  pub(crate) fn new() -> MapWorld {
    let (mut world, mut schedules) = physics_world(60);
    insert_registry(&mut world, AssetServer::new(files_with(&[])));
    schedules.add_systems(Phase::PreUpdate, super::spawn_maps);
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
    self.schedules.run(Phase::PostUpdate, &mut self.world);
    self.schedules.run(Phase::Last, &mut self.world);
  }

  /// The pump and `PreUpdate` (the spawn) alone
  pub(crate) fn pre_update(&mut self) {
    pump(&mut self.world);
    self.schedules.run(Phase::PreUpdate, &mut self.world);
  }

  /// One fixed step alone
  pub(crate) fn step(&mut self) {
    self.schedules.run(Phase::FixedUpdate, &mut self.world);
  }

  /// `PostUpdate` alone
  pub(crate) fn post_update(&mut self) {
    self.schedules.run(Phase::PostUpdate, &mut self.world);
  }

  /// Spawns a map entity over `root` at the origin and runs frames until
  /// it is built (`settle`)
  pub(crate) fn spawn(&mut self, root: super::MapRoot) -> Entity {
    let map = self.world.spawn(root).id();
    self.settle(map);
    map
  }

  /// Runs frames until `map` collides and its shadows are all spawned: its
  /// first builds come back from another thread, and spawn over frames
  pub(crate) fn settle(&mut self, map: Entity) {
    let start = std::time::Instant::now();
    loop {
      self.frame();
      let collides = super::MapsView::new(&mut self.world).collides(map);
      let shadows = self
        .world
        .get::<super::occluders::MapShadows>(map)
        .is_some_and(super::occluders::MapShadows::is_built);
      if collides && shadows {
        return;
      }
      assert!(
        start.elapsed() < std::time::Duration::from_secs(5),
        "map {map} was not built in 5 s"
      );
      std::thread::sleep(std::time::Duration::from_millis(1));
    }
  }
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
