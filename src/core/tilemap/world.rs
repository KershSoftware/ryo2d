//! Worlds: a Tiled `.world` file or an LDtk world as levels with positions
//! and neighbours, and spawning the levels in an area.

use glam::Vec2;

use super::spawn::{MapRoot, MapSource};
use super::{LdtkProject, Neighbour, NeighbourDirection, TileMap, WorldLayout, WorldLevel};
use crate::core::asset::{
  Asset, AssetPath, AssetServer, Assets, Handle, LoadState, UntypedAssetId,
};
use crate::core::ecs::{Component, Entity, Transform, World};
use crate::core::ui::Rect;

/// A Tiled world file's levels: each map with its place in the world, its
/// neighbours worked out from the edges it shares with others (an edge in
/// common is a cardinal neighbour, a corner a diagonal one, an overlap
/// [`NeighbourDirection::Overlap`]). Loaded from a `.world` file; its maps
/// are dependencies, so the world is ready when they are.
///
/// A map is listed (`maps`), or named by a pattern. The registry cannot list
/// a folder, so a pattern is supported only as literal text with up to two
/// numbers, `(\d+)` (or `(-?\d+)`), in the file name, as in
/// `"room_(\d+)_(\d+)\.tmx"`: one number is x, two are x then y, and the
/// loader tries 0, 1, 2... row by row (y = 0, 1, ...; in each row x from 0)
/// until the first missing file, stopping at the first row whose x = 0 file
/// is missing, in at most 1,024 tries. Each map sits at
/// (x * multiplierX + offsetX, y * multiplierY + offsetY). Other regular
/// expression features are a load error, and a pattern no file matches
/// fails the load; negative numbers and gaps in the numbering are not
/// reached, so a world that uses them lists its maps. A listed map without
/// a `width` and `height` has them read from its file.
///
/// Spawn the levels near the player with
/// [`MapsView::spawn_levels_in`](super::MapsView::spawn_levels_in).
#[derive(Debug)]
pub struct TileWorld {
  /// The levels and their neighbours, in the order the file lists them
  /// (listed maps first, then each pattern's)
  pub layout: WorldLayout,
  maps: Vec<Handle<TileMap>>,
}

impl Asset for TileWorld {}

impl TileWorld {
  pub(crate) fn new(layout: WorldLayout, maps: Vec<Handle<TileMap>>) -> TileWorld {
    TileWorld { layout, maps }
  }

  /// The maps, by level index
  pub fn maps(&self) -> &[Handle<TileMap>] {
    &self.maps
  }

  /// The map of level `level`
  pub fn map(&self, level: usize) -> Option<&Handle<TileMap>> {
    self.maps.get(level)
  }
}

/// A world a game spawns levels of: a Tiled world file's asset, or one of
/// an LDtk project's worlds
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WorldRef {
  Tiled(Handle<TileWorld>),
  /// The world named `world` of `project` (a single-world project's is
  /// named as the project names it)
  Ldtk {
    project: Handle<LdtkProject>,
    world: String,
  },
}

impl From<&Handle<TileWorld>> for WorldRef {
  fn from(world: &Handle<TileWorld>) -> Self {
    WorldRef::Tiled(world.clone())
  }
}

impl WorldRef {
  /// The world's layout, once its asset is loaded
  pub(crate) fn layout<'w>(&self, world: &'w World) -> Option<&'w WorldLayout> {
    match self {
      WorldRef::Tiled(handle) => Some(
        &world
          .get_resource::<Assets<TileWorld>>()?
          .get(handle)?
          .layout,
      ),
      WorldRef::Ldtk {
        project,
        world: name,
      } => world
        .get_resource::<Assets<LdtkProject>>()?
        .get(project)?
        .worlds
        .iter()
        .find(|w| w.name == *name),
    }
  }

  pub(crate) fn asset_id(&self) -> UntypedAssetId {
    match self {
      WorldRef::Tiled(handle) => handle.id().untyped(),
      WorldRef::Ldtk { project, .. } => project.id().untyped(),
    }
  }

  pub(crate) fn state(&self, server: &AssetServer) -> LoadState {
    match self {
      WorldRef::Tiled(handle) => server.state(handle),
      WorldRef::Ldtk { project, .. } => server.state(project),
    }
  }

  pub(crate) fn path(&self, server: &AssetServer) -> Option<AssetPath> {
    match self {
      WorldRef::Tiled(handle) => server.path(handle),
      WorldRef::Ldtk { project, .. } => server.path(project),
    }
  }

  pub(crate) fn version(&self, world: &World) -> Option<u64> {
    match self {
      WorldRef::Tiled(handle) => world.get_resource::<Assets<TileWorld>>()?.version(handle),
      WorldRef::Ldtk { project, .. } => world
        .get_resource::<Assets<LdtkProject>>()?
        .version(project),
    }
  }

  /// What the map of level `level` is, once the world is loaded and has the level
  pub(crate) fn resolve(&self, world: &World, level: usize) -> Option<MapSource> {
    match self {
      WorldRef::Tiled(handle) => {
        let map = world
          .get_resource::<Assets<TileWorld>>()?
          .get(handle)?
          .map(level)?;
        Some(MapSource::Map(map.clone()))
      }
      WorldRef::Ldtk { project, .. } => {
        let name = &self.layout(world)?.levels.get(level)?.name;
        Some(MapSource::Ldtk {
          project: project.clone(),
          level: name.clone(),
        })
      }
    }
  }
}

/// Marks the root of a level spawned by
/// [`MapsView::spawn_levels_in`](super::MapsView::spawn_levels_in): which
/// world, and which of its levels. While the entity lives, that level is
/// spawned; despawn it and the level can be spawned again.
#[derive(Component, Clone, Debug, PartialEq, Eq)]
pub struct WorldLevelOf {
  pub world: WorldRef,
  /// The index in the world's [`WorldLayout::levels`]
  pub level: usize,
}

impl MapRoot {
  /// Level `level` of the Tiled world `world` (an index in its
  /// [`WorldLayout::levels`]). The root names the map once the world is
  /// loaded; a level the world does not have fails the root
  /// ([`MapEvent::Failed`](super::MapEvent::Failed)). Put the level at
  /// its [`WorldLevel::position`] with the root's `Transform`
  pub fn world_level(world: &Handle<TileWorld>, level: usize) -> MapRoot {
    MapRoot::from_source(MapSource::WorldLevel {
      world: WorldRef::Tiled(world.clone()),
      level,
    })
  }

  /// Level `level` of the world `world` of an LDtk project, as
  /// [`world_level`](Self::world_level) does for a Tiled world
  pub fn ldtk_world_level(project: Handle<LdtkProject>, world: &str, level: usize) -> MapRoot {
    MapRoot::from_source(MapSource::WorldLevel {
      world: WorldRef::Ldtk {
        project,
        world: world.to_string(),
      },
      level,
    })
  }
}

impl WorldLayout {
  /// The levels whose rectangle overlaps `area`, with their indices, in
  /// layout order. A level that only touches the area's edge does not count
  pub fn levels_in(&self, area: Rect) -> impl Iterator<Item = (usize, &WorldLevel)> {
    self.levels.iter().enumerate().filter(move |(_, level)| {
      level.position.x < area.right()
        && level.position.x + level.size.x > area.x
        && level.position.y < area.bottom()
        && level.position.y + level.size.y > area.y
    })
  }
}

/// Where level `b` lies from level `a`, if they touch or overlap
fn direction(a: &WorldLevel, b: &WorldLevel) -> Option<NeighbourDirection> {
  use NeighbourDirection::*;
  let (a_end, b_end) = (a.position + a.size, b.position + b.size);
  let overlap_x = a_end.x.min(b_end.x) - a.position.x.max(b.position.x);
  let overlap_y = a_end.y.min(b_end.y) - a.position.y.max(b.position.y);
  if overlap_x < 0.0 || overlap_y < 0.0 {
    return None;
  }
  let east = b.position.x >= a_end.x;
  let south = b.position.y >= a_end.y;
  Some(match (overlap_x > 0.0, overlap_y > 0.0) {
    (true, true) => Overlap,
    (false, true) => {
      if east {
        East
      } else {
        West
      }
    }
    (true, false) => {
      if south {
        South
      } else {
        North
      }
    }
    (false, false) => match (east, south) {
      (true, true) => SouthEast,
      (true, false) => NorthEast,
      (false, true) => SouthWest,
      (false, false) => NorthWest,
    },
  })
}

/// A layout of `levels` with each one's neighbours worked out from the
/// edges they share
pub(crate) fn layout_of_rects(name: &str, mut levels: Vec<WorldLevel>) -> WorldLayout {
  let found: Vec<Vec<Neighbour>> = levels
    .iter()
    .enumerate()
    .map(|(i, a)| {
      levels
        .iter()
        .enumerate()
        .filter(|(j, _)| *j != i)
        .filter_map(|(j, b)| {
          direction(a, b).map(|direction| Neighbour {
            level: j,
            direction,
          })
        })
        .collect()
    })
    .collect();
  for (level, neighbours) in levels.iter_mut().zip(found) {
    level.neighbours = neighbours;
  }
  WorldLayout {
    name: name.to_string(),
    levels,
  }
}

/// Spawns the levels of `world` that overlap `area` and are not spawned
pub(crate) fn spawn_levels_in(world: &mut World, source: WorldRef, area: Rect) -> Vec<Entity> {
  let Some(layout) = source.layout(world) else {
    return Vec::new();
  };
  let wanted: Vec<(usize, Vec2)> = layout
    .levels_in(area)
    .map(|(i, l)| (i, l.position))
    .collect();
  let mut have = world.query::<&WorldLevelOf>();
  let present: Vec<usize> = have
    .iter(world)
    .filter(|of| of.world == source)
    .map(|of| of.level)
    .collect();
  let mut spawned = Vec::new();
  for (level, position) in wanted {
    if present.contains(&level) {
      continue;
    }
    let root = MapRoot::from_source(MapSource::WorldLevel {
      world: source.clone(),
      level,
    });
    let of = WorldLevelOf {
      world: source.clone(),
      level,
    };
    let entity = world
      .spawn((root, of, Transform::from_xy(position.x, position.y)))
      .id();
    spawned.push(entity);
  }
  spawned
}

/// Gives each waiting root that names a level of a world the map it
/// stands for, once the world has loaded, so the rest of the map systems
/// only see maps and LDtk levels
pub(crate) fn resolve_world_levels(world: &mut World, waiting: &mut super::spawn::WaitingMaps) {
  let resolved: Vec<(Entity, MapSource)> = waiting
    .iter(world)
    .filter_map(|(entity, root, ..)| match &root.source {
      MapSource::WorldLevel {
        world: source,
        level,
      } => source.resolve(world, *level).map(|map| (entity, map)),
      _ => None,
    })
    .collect();
  for (entity, source) in resolved {
    if let Some(mut root) = world.get_mut::<MapRoot>(entity) {
      root.source = source;
    }
  }
}

#[cfg(test)]
mod tests {
  use glam::Vec2;

  use super::*;
  use crate::core::asset::{Assets, AssetsView};
  use crate::core::ecs::{Entity, Transform};
  use crate::core::tilemap::test_util::{MapWorld, files_with, maps_landed, world_over};
  use crate::core::tilemap::{LdtkProject, MapRoot, MapsView, NeighbourDirection};
  use crate::core::ui::Rect;

  fn load_world(
    world: &mut crate::core::ecs::World,
    path: &str,
  ) -> Result<Handle<TileWorld>, String> {
    AssetsView::new(world)
      .load_now::<TileWorld>(path)
      .map_err(|e| e.report())
  }

  fn layout_of(world: &crate::core::ecs::World, handle: &Handle<TileWorld>) -> WorldLayout {
    match world.resource::<Assets<TileWorld>>().get(handle) {
      Some(w) => w.layout.clone(),
      None => panic!("world not loaded"),
    }
  }

  fn level<'a>(layout: &'a WorldLayout, name: &str) -> (usize, &'a WorldLevel) {
    match layout
      .levels
      .iter()
      .enumerate()
      .find(|(_, l)| l.name == name)
    {
      Some(found) => found,
      None => panic!("no level {name}"),
    }
  }

  fn neighbours(layout: &WorldLayout, name: &str) -> Vec<(String, NeighbourDirection)> {
    let mut found: Vec<_> = level(layout, name)
      .1
      .neighbours
      .iter()
      .map(|n| (layout.levels[n.level].name.clone(), n.direction))
      .collect();
    found.sort_by_key(|(n, _)| n.clone());
    found
  }

  #[test]
  fn a_tiled_world_places_its_maps_and_finds_neighbours() {
    use NeighbourDirection::*;
    let mut world = world_over(files_with(&[]));
    let rooms = load_world(&mut world, "tiled/worlds/rooms.world").expect("rooms.world loads");
    let layout = layout_of(&world, &rooms);
    assert_eq!(layout.levels.len(), 4);
    // a gives no size: it is read from the map
    let (_, a) = level(&layout, "a.tmj");
    assert_eq!((a.position, a.size), (Vec2::ZERO, Vec2::new(32.0, 32.0)));
    assert_eq!(level(&layout, "b.tmj").1.position, Vec2::new(32.0, 0.0));
    assert_eq!(
      neighbours(&layout, "a.tmj"),
      [("b.tmj".into(), East), ("c.tmj".into(), SouthEast)]
    );
    assert_eq!(
      neighbours(&layout, "b.tmj"),
      [("a.tmj".into(), West), ("c.tmj".into(), South)]
    );
    assert_eq!(
      neighbours(&layout, "c.tmj"),
      [("a.tmj".into(), NorthWest), ("b.tmj".into(), North)]
    );
    assert!(neighbours(&layout, "d.tmj").is_empty());

    // a pattern numbers its maps: x then y, at the multipliers
    let grid = load_world(&mut world, "tiled/worlds/grid.world").expect("grid.world loads");
    let layout = layout_of(&world, &grid);
    let at: Vec<_> = layout
      .levels
      .iter()
      .map(|l| (l.name.clone(), l.position))
      .collect();
    assert_eq!(
      at,
      [
        ("grid_0_0.tmj".to_string(), Vec2::new(0.0, 0.0)),
        ("grid_1_0.tmj".to_string(), Vec2::new(32.0, 0.0)),
        ("grid_0_1.tmj".to_string(), Vec2::new(0.0, 32.0)),
      ]
    );
    assert_eq!(
      neighbours(&layout, "grid_0_0.tmj"),
      [
        ("grid_0_1.tmj".into(), South),
        ("grid_1_0.tmj".into(), East)
      ]
    );

    // the maps are dependencies: held by the world
    let maps = world
      .resource::<Assets<TileWorld>>()
      .get(&grid)
      .map(|w| w.maps().len());
    assert_eq!(maps, Some(3));
  }

  #[test]
  fn overlapping_maps_neighbour_as_overlap() {
    let files = files_with(&[(
      "o.world",
      r#"{"maps":[{"fileName":"tiled/worlds/a.tmj","x":0,"y":0},
         {"fileName":"tiled/worlds/b.tmj","x":16,"y":16}]}"#,
    )]);
    let mut world = world_over(files);
    let o = load_world(&mut world, "o.world").expect("loads");
    let layout = layout_of(&world, &o);
    assert_eq!(
      neighbours(&layout, "tiled/worlds/a.tmj"),
      [("tiled/worlds/b.tmj".into(), NeighbourDirection::Overlap)]
    );
  }

  #[test]
  fn a_pattern_with_no_match_fails() {
    let files = files_with(&[(
      "tiled/worlds/none.world",
      r#"{"patterns":[{"regexp":"nothing_(\\d+)\\.tmj","multiplierX":32,"multiplierY":32}]}"#,
    )]);
    let mut world = world_over(files);
    let error = load_world(&mut world, "tiled/worlds/none.world").expect_err("fails");
    assert!(error.contains("none.world"), "{error}");
    assert!(
      error.contains("nothing_(\\d+)\\.tmj") && error.contains("no map"),
      "{error}"
    );
  }

  #[test]
  fn patterns_beyond_a_numbered_sequence_are_not_supported() {
    for regexp in [
      r"a_(.*)\\.tmj",
      r"(\\d+)(\\d+)(\\d+)",
      r"[ab]_(\\d+)",
      r"a\\d_(\\d+)",
    ] {
      let json = format!(r#"{{"patterns":[{{"regexp":"{regexp}"}}]}}"#);
      let mut world = world_over(files_with(&[("p.world", &json)]));
      let error = load_world(&mut world, "p.world").expect_err("fails");
      assert!(error.contains("not supported"), "{regexp}: {error}");
    }
  }

  #[test]
  fn a_missing_map_fails_the_world_naming_it() {
    let files = files_with(&[(
      "m.world",
      r#"{"maps":[{"fileName":"tiled/worlds/zzz.tmj","x":0,"y":0,"width":1,"height":1}]}"#,
    )]);
    let mut world = world_over(files);
    let error = load_world(&mut world, "m.world").expect_err("fails");
    assert!(error.contains("zzz.tmj"), "{error}");
  }

  #[test]
  fn levels_in_counts_overlap_not_touching() {
    let mut world = world_over(files_with(&[]));
    let rooms = load_world(&mut world, "tiled/worlds/rooms.world").expect("loads");
    let layout = layout_of(&world, &rooms);
    let names = |area: Rect| -> Vec<String> {
      layout
        .levels_in(area)
        .map(|(_, l)| l.name.clone())
        .collect()
    };
    assert_eq!(names(Rect::new(10.0, 10.0, 4.0, 4.0)), ["a.tmj"]);
    // touching a's right edge only: b, not a
    assert_eq!(names(Rect::new(32.0, 0.0, 10.0, 10.0)), ["b.tmj"]);
    assert_eq!(
      names(Rect::new(30.0, 30.0, 4.0, 4.0)),
      ["a.tmj", "b.tmj", "c.tmj"]
    );
    // an area ending where a begins, or above it, touches only
    assert!(names(Rect::new(-10.0, 0.0, 10.0, 10.0)).is_empty());
    assert!(names(Rect::new(0.0, -10.0, 10.0, 10.0)).is_empty());
    assert!(names(Rect::new(500.0, 500.0, 4.0, 4.0)).is_empty());
  }

  fn spawned_roots(world: &mut crate::core::ecs::World) -> Vec<(Entity, usize, Vec2)> {
    let mut found: Vec<_> = world
      .query::<(Entity, &WorldLevelOf, &Transform)>()
      .iter(world)
      .map(|(e, l, t)| (e, l.level, t.translation))
      .collect();
    found.sort_by_key(|(_, level, _)| *level);
    found
  }

  #[test]
  fn levels_in_an_area_spawn_once() {
    let mut maps = MapWorld::new();
    let rooms = load_world(&mut maps.world, "tiled/worlds/rooms.world").expect("loads");
    let area = Rect::new(30.0, 0.0, 10.0, 10.0);
    let first = MapsView::new(&mut maps.world).spawn_levels_in(&rooms, area);
    assert_eq!(first.len(), 2);
    maps.frames_until(maps_landed);
    let roots = spawned_roots(&mut maps.world);
    assert_eq!(
      roots.iter().map(|(_, l, p)| (*l, *p)).collect::<Vec<_>>(),
      [(0, Vec2::new(0.0, 0.0)), (1, Vec2::new(32.0, 0.0))]
    );
    assert!(roots.iter().all(|(e, ..)| first.contains(e)));
    for (map, ..) in &roots {
      assert!(MapsView::new(&mut maps.world).is_ready(*map));
    }

    // the same area again spawns nothing; a wider one only the new level
    assert!(
      MapsView::new(&mut maps.world)
        .spawn_levels_in(&rooms, area)
        .is_empty()
    );
    let more =
      MapsView::new(&mut maps.world).spawn_levels_in(&rooms, Rect::new(30.0, 0.0, 10.0, 40.0));
    assert_eq!(more.len(), 1);
    maps.frames_until(maps_landed);
    assert_eq!(spawned_roots(&mut maps.world).len(), 3);

    // a level the game despawned can be spawned again
    maps.world.despawn(first[0]);
    let again = MapsView::new(&mut maps.world).spawn_levels_in(&rooms, area);
    assert_eq!(again.len(), 1);
    maps.frames_until(maps_landed);
    assert_eq!(spawned_roots(&mut maps.world).len(), 3);
  }

  #[test]
  fn ldtk_worlds_place_levels() {
    let mut maps = MapWorld::new();
    let project = AssetsView::new(&mut maps.world)
      .load_now::<LdtkProject>("ldtk/worlds.ldtk")
      .expect("worlds.ldtk loads");
    let caves = WorldRef::Ldtk {
      project: project.clone(),
      world: "Caves".into(),
    };
    let spawned = MapsView::new(&mut maps.world)
      .spawn_levels_in(caves.clone(), Rect::new(-100.0, 0.0, 500.0, 100.0));
    assert_eq!(spawned.len(), 2);
    maps.frames_until(maps_landed);
    let roots = spawned_roots(&mut maps.world);
    assert_eq!(
      roots.iter().map(|(_, l, p)| (*l, *p)).collect::<Vec<_>>(),
      [(0, Vec2::new(320.0, 32.0)), (1, Vec2::new(-64.0, 16.0))]
    );
    for (map, ..) in &roots {
      assert!(MapsView::new(&mut maps.world).is_ready(*map));
    }
    // each is a level of the project, by the world's name for it
    let root = MapRoot::ldtk_world_level(project.clone(), "Caves", 1);
    let spawned_b = maps.world.spawn(root).id();
    maps.settle(spawned_b);
    // only the Caves' levels spawn for Caves; spawning again does nothing
    assert!(
      MapsView::new(&mut maps.world)
        .spawn_levels_in(caves, Rect::new(-100.0, 0.0, 500.0, 100.0))
        .is_empty()
    );
    // another world's area spawns its own
    let hall = WorldRef::Ldtk {
      project,
      world: "Hall".into(),
    };
    assert_eq!(
      MapsView::new(&mut maps.world)
        .spawn_levels_in(hall, Rect::new(0.0, 0.0, 10.0, 200.0))
        .len(),
      2
    );
  }

  #[test]
  fn a_level_that_is_not_there_fails_its_root() {
    let mut maps = MapWorld::new();
    let rooms = load_world(&mut maps.world, "tiled/worlds/rooms.world").expect("loads");
    let bad = maps.world.spawn(MapRoot::world_level(&rooms, 9)).id();
    maps.frames_until(maps_landed);
    assert!(!MapsView::new(&mut maps.world).is_ready(bad));
    assert!(
      maps
        .world
        .get::<crate::core::tilemap::spawn::MapFailed>(bad)
        .is_some()
    );
  }
}
