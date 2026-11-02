//! Worlds: a Tiled `.world` file or an LDtk world as levels with positions
//! and neighbours, and spawning the levels in an area.

use glam::Vec2;

use super::spawn::{MapRoot, MapSource};
use super::{LdtkProject, Neighbour, NeighbourDirection, TileMap, WorldLayout, WorldLevel};
use crate::core::asset::{
  Asset, AssetPath, AssetServer, Assets, Handle, LoadState, UntypedAssetId,
};
use crate::core::ecs::{Allow, Component, Disabled, Entity, Transform, World};
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
/// expression features are a load error (a bare `.` is a dot, since names
/// are built, never matched; `.*` fails on its `*`), and a pattern no file matches
/// fails the load. Names are built, so only these files are found:
/// - numbers written plainly, with no leading zeros: `room_1_2.tmx`, never
///   `room_01_02.tmx` (a literal `0` in the pattern, `room_0(\d+)\.tmx`,
///   reaches `room_00` to `room_09` and stops, since 10 is built `room_010`);
/// - numbering from 0 in every row: maps numbered from 1 are not found (the
///   load fails, no map matching), and neither are negative numbers nor the
///   maps past a gap.
///
/// A world named any other way lists its maps. Every map is placed by its
/// tile origin: the world's x and y are where its pixel (0, 0) sits
/// ([`WorldLevel::origin`]), and its rectangle is its cells' box on its
/// grid ([`MapGrid::bounds_of`](super::MapGrid::bounds_of)) from there,
/// read from its file: a bounded map's every cell (an isometric map's
/// diamond's box), or the size the world lists it with; an infinite map's
/// tiles' bounds, as [`TileMap::bounds`] gives them, whatever size the
/// world lists it with, and one with no tile takes its origin cell (Tiled
/// documents no rule for infinite or non-orthogonal maps in worlds, and
/// cannot load one headless to measure; this is the engine's reading).
/// The world reads every map it lists or a pattern finds, each of which
/// then loads again as a dependency, so saving one loads the world again.
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
  /// its [`WorldLevel::origin`] with the root's `Transform`
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
  /// The levels at world depth `depth` whose rectangle overlaps `area`,
  /// with their indices, in layout order. A level that only touches the
  /// area's edge does not count. A Tiled world's levels are all at depth 0;
  /// an LDtk world's are at their own ([`WorldLevel::depth`]), so a floor
  /// above or below one is not in its area
  pub fn levels_in(&self, area: Rect, depth: i32) -> impl Iterator<Item = (usize, &WorldLevel)> {
    self.levels.iter().enumerate().filter(move |(_, level)| {
      level.depth == depth
        && level.position.x < area.right()
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

/// Spawns the levels of `source` at world depth `depth` that overlap `area`
/// and are not spawned, each root set up by `configure`
pub(crate) fn spawn_levels_in(
  world: &mut World,
  source: WorldRef,
  area: Rect,
  depth: i32,
  mut configure: impl FnMut(&WorldLevel, MapRoot) -> MapRoot,
) -> Vec<Entity> {
  // A disabled root (a level the game parked) still holds its level
  let present: Vec<usize> = world
    .query_filtered::<&WorldLevelOf, Allow<Disabled>>()
    .iter(world)
    .filter(|of| of.world == source)
    .map(|of| of.level)
    .collect();
  let Some(layout) = source.layout(world) else {
    return Vec::new();
  };
  let roots: Vec<(usize, Vec2, MapRoot)> = layout
    .levels_in(area, depth)
    .filter(|(index, _)| !present.contains(index))
    .map(|(index, level)| {
      let source = MapSource::WorldLevel {
        world: source.clone(),
        level: index,
      };
      // The game sets the root up; the map stays the level's
      let mut root = configure(level, MapRoot::from_source(source.clone()));
      root.source = source;
      (index, level.origin, root)
    })
    .collect();
  roots
    .into_iter()
    .map(|(level, position, root)| {
      let of = WorldLevelOf {
        world: source.clone(),
        level,
      };
      world
        .spawn((root, of, Transform::from_xy(position.x, position.y)))
        .id()
    })
    .collect()
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
  use glam::{IVec2, Vec2};

  use super::*;
  use crate::core::asset::{Assets, AssetsView};
  use crate::core::ecs::{Entity, Transform};
  use crate::core::tilemap::test_util::{MapWorld, files_with, maps_landed, world_over};
  use crate::core::tilemap::{
    GridShape, LdtkProject, MapRoot, MapsView, NeighbourDirection, StaggerAxis, StaggerIndex,
  };
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
        .levels_in(area, 0)
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

  #[test]
  fn a_world_places_an_infinite_map_by_its_tile_origin() {
    let files = files_with(&[(
      "tiled/worlds/far.world",
      r#"{"maps":[{"fileName":"../maps/infinite.tmj","x":1000,"y":500},
         {"fileName":"a.tmj","x":-1000,"y":-1000}],"type":"world"}"#,
    )]);
    let mut maps = MapWorld::over(files);
    let far = load_world(&mut maps.world, "tiled/worlds/far.world").expect("loads");
    let layout = layout_of(&maps.world, &far);
    let handle = AssetsView::new(&mut maps.world)
      .load_now::<TileMap>("tiled/maps/infinite.tmj")
      .expect("the map loads");
    let bounds = match maps.world.resource::<Assets<TileMap>>().get(&handle) {
      Some(map) => map.bounds(),
      None => panic!("the map is not in the store"),
    };
    assert!(bounds.min.x < 0 && bounds.min.y < 0, "{bounds:?}");
    // The world's x and y are where the map's cell (0, 0) sits; its
    // rectangle is its tiles' bounds from there
    let tile = Vec2::splat(16.0);
    let (index, far_level) = level(&layout, "../maps/infinite.tmj");
    assert_eq!(far_level.origin, Vec2::new(1000.0, 500.0));
    assert_eq!(
      far_level.position,
      Vec2::new(1000.0, 500.0) + bounds.min.as_vec2() * tile
    );
    assert_eq!(far_level.size, bounds.size().as_vec2() * tile);
    // a bounded map's origin is its rectangle's top-left
    let (_, a) = level(&layout, "a.tmj");
    assert_eq!(
      (a.origin, a.position),
      (Vec2::splat(-1000.0), Vec2::splat(-1000.0))
    );

    // Spawned, its root (the map's cell (0, 0)) sits at the tile origin
    let near = Rect::new(
      far_level.position.x + 1.0,
      far_level.position.y + 1.0,
      4.0,
      4.0,
    );
    assert!(
      !near.contains(far_level.origin.x, far_level.origin.y),
      "the area is away from the origin"
    );
    let spawned = MapsView::new(&mut maps.world).spawn_levels_in(&far, near);
    assert_eq!(spawned.len(), 1);
    maps.frames_until(maps_landed);
    let roots = spawned_roots(&mut maps.world);
    assert_eq!(
      roots.iter().map(|(_, l, p)| (*l, *p)).collect::<Vec<_>>(),
      [(index, Vec2::new(1000.0, 500.0))]
    );
    let cell =
      MapsView::new(&mut maps.world).cell_to_world(roots[0].0, "ground", IVec2::new(-20, -3));
    assert_eq!(
      cell,
      Some(Vec2::new(
        1000.0 - 20.0 * 16.0 + 8.0,
        500.0 - 3.0 * 16.0 + 8.0
      ))
    );
  }

  #[test]
  fn an_infinite_map_listed_with_a_size_takes_its_tiles_rectangle() {
    // Tiled writes a width and height for every map a world lists: an
    // infinite map's rectangle is its tiles' from its origin, whatever the
    // entry says
    let files = files_with(&[(
      "tiled/worlds/sized.world",
      r#"{"maps":[{"fileName":"../maps/infinite.tmj","x":1000,"y":500,"width":320,"height":160},
         {"fileName":"a.tmj","x":-1000,"y":-1000,"width":64,"height":48}],"type":"world"}"#,
    )]);
    let mut maps = MapWorld::over(files);
    let sized = load_world(&mut maps.world, "tiled/worlds/sized.world").expect("loads");
    let layout = layout_of(&maps.world, &sized);
    let handle = AssetsView::new(&mut maps.world)
      .load_now::<TileMap>("tiled/maps/infinite.tmj")
      .expect("the map loads");
    let bounds = match maps.world.resource::<Assets<TileMap>>().get(&handle) {
      Some(map) => map.bounds(),
      None => panic!("the map is not in the store"),
    };
    let tile = Vec2::splat(16.0);
    let (index, far) = level(&layout, "../maps/infinite.tmj");
    assert_eq!(far.origin, Vec2::new(1000.0, 500.0));
    assert_eq!(
      (far.position, far.size),
      (
        far.origin + bounds.min.as_vec2() * tile,
        bounds.size().as_vec2() * tile
      )
    );
    // a bounded map keeps the size it is listed with
    let (_, a) = level(&layout, "a.tmj");
    assert_eq!(
      (a.origin, a.position, a.size),
      (
        Vec2::splat(-1000.0),
        Vec2::splat(-1000.0),
        Vec2::new(64.0, 48.0)
      )
    );
    // An area over the map's negative part, away from its origin, finds it
    // and spawns it
    let west = Rect::new(far.position.x + 1.0, far.position.y + 1.0, 4.0, 4.0);
    assert!(west.right() < far.origin.x && west.bottom() < far.origin.y);
    let found: Vec<usize> = layout.levels_in(west, 0).map(|(i, _)| i).collect();
    assert_eq!(found, [index]);
    let spawned = MapsView::new(&mut maps.world).spawn_levels_in(&sized, west);
    assert_eq!(spawned.len(), 1);
  }

  #[test]
  fn an_infinite_map_s_world_rectangle_holds_its_grouped_layers_and_straddling_chunks() {
    // The world reads the file itself: a tile layer in a group, and Tiled's
    // chunks of 24 x 24 across the engine's, give the loaded map's bounds
    let files = files_with(&[(
      "tiled/worlds/chunks24.world",
      r#"{"maps":[{"fileName":"../maps/infinite-chunks24.tmj","x":160,"y":-80,"width":16,"height":16}],"type":"world"}"#,
    )]);
    let mut maps = MapWorld::over(files);
    let world = load_world(&mut maps.world, "tiled/worlds/chunks24.world").expect("loads");
    let layout = layout_of(&maps.world, &world);
    let handle = AssetsView::new(&mut maps.world)
      .load_now::<TileMap>("tiled/maps/infinite-chunks24.tmj")
      .expect("the map loads");
    let bounds = match maps.world.resource::<Assets<TileMap>>().get(&handle) {
      Some(map) => map.bounds(),
      None => panic!("the map is not in the store"),
    };
    assert_eq!(bounds.min, IVec2::new(-40, -49), "the grouped layer's tile");
    let level = &layout.levels[0];
    let tile = Vec2::splat(16.0);
    assert_eq!(
      (level.origin, level.position, level.size),
      (
        Vec2::new(160.0, -80.0),
        Vec2::new(160.0, -80.0) + bounds.min.as_vec2() * tile,
        bounds.size().as_vec2() * tile
      )
    );
  }

  #[test]
  fn a_world_places_isometric_and_infinite_hex_levels_by_their_grid_bounds() {
    // `shape.world` lists a bounded isometric map at (-400, -200) and an
    // infinite hexagonal one at (1000, 500): each level's origin (where its
    // root, the map's pixel (0, 0), spawns) is where the world lists it, and
    // its rectangle is its cells' box on its grid from there
    let mut maps = MapWorld::new();
    let world = load_world(&mut maps.world, "tiled/worlds/shape.world").expect("loads");
    let layout = layout_of(&maps.world, &world);
    for (name, at, shape) in [
      (
        "../maps/shape-iso.tmj",
        Vec2::new(-400.0, -200.0),
        GridShape::Isometric,
      ),
      (
        "../maps/shape-infinite-hex.tmj",
        Vec2::new(1000.0, 500.0),
        GridShape::Hexagonal {
          axis: StaggerAxis::Y,
          index: StaggerIndex::Odd,
          side: 12,
        },
      ),
    ] {
      let path = format!("tiled/{}", name.trim_start_matches("../"));
      let handle = AssetsView::new(&mut maps.world)
        .load_now::<TileMap>(&path)
        .expect("the map loads");
      let Some(map) = maps.world.resource::<Assets<TileMap>>().get(&handle) else {
        panic!("{name} is not in the store")
      };
      assert_eq!(map.shape, shape, "{name}");
      let rect = map.grid().bounds_of(map.bounds());
      let (_, level) = level(&layout, name);
      assert_eq!(level.origin, at, "{name}");
      assert_eq!(
        (level.position, level.size),
        (
          at + Vec2::new(rect.x, rect.y),
          Vec2::new(rect.width, rect.height)
        ),
        "{name}"
      );
    }
    // The isometric map's box is its diamond's, (6 + 4) half cells a side,
    // from its top-left: not 6 x 4 cells of 64 x 32
    let (_, iso) = level(&layout, "../maps/shape-iso.tmj");
    assert_eq!(
      (iso.position, iso.size),
      (Vec2::new(-400.0, -200.0), Vec2::new(320.0, 160.0))
    );
    // The hexagonal one's rows step 22 pixels, not 32: its top row, -40, is
    // 880 above its origin
    let (_, hex) = level(&layout, "../maps/shape-infinite-hex.tmj");
    assert_eq!(hex.position.y, 500.0 - 40.0 * 22.0);
  }

  #[test]
  fn an_empty_infinite_map_in_a_world_takes_its_origin_cell() {
    let empty = r#"{"type":"map","orientation":"orthogonal","infinite":true,"width":0,"height":0,
      "tilewidth":16,"tileheight":8,"tilesets":[],
      "layers":[{"id":1,"name":"ground","type":"tilelayer","chunks":[]}]}"#;
    let files = files_with(&[
      ("tiled/maps/empty-7c1e.tmj", empty),
      (
        "tiled/worlds/empty.world",
        r#"{"maps":[{"fileName":"../maps/empty-7c1e.tmj","x":64,"y":-32,"width":320,"height":160},
           {"fileName":"../maps/empty-7c1e.tmj","x":-64,"y":32}],"type":"world"}"#,
      ),
    ]);
    let mut maps = MapWorld::over(files);
    let world = load_world(&mut maps.world, "tiled/worlds/empty.world").expect("loads");
    let layout = layout_of(&maps.world, &world);
    let rects: Vec<(Vec2, Vec2, Vec2)> = layout
      .levels
      .iter()
      .map(|l| (l.origin, l.position, l.size))
      .collect();
    let tile = Vec2::new(16.0, 8.0);
    assert_eq!(
      rects,
      [
        (Vec2::new(64.0, -32.0), Vec2::new(64.0, -32.0), tile),
        (Vec2::new(-64.0, 32.0), Vec2::new(-64.0, 32.0), tile)
      ]
    );
    // so it can still spawn
    let area = Rect::new(66.0, -30.0, 2.0, 2.0);
    assert_eq!(
      MapsView::new(&mut maps.world)
        .spawn_levels_in(&world, area)
        .len(),
      1
    );
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
  fn a_parked_level_is_not_spawned_again() {
    use crate::core::ecs::Disabled;
    let mut maps = MapWorld::new();
    let rooms = load_world(&mut maps.world, "tiled/worlds/rooms.world").expect("loads");
    let area = Rect::new(10.0, 10.0, 4.0, 4.0);
    let first = MapsView::new(&mut maps.world).spawn_levels_in(&rooms, area);
    assert_eq!(first.len(), 1);
    maps.frames_until(maps_landed);
    // The game parks the room: disabled, not despawned
    maps.world.entity_mut(first[0]).insert(Disabled);
    assert!(
      MapsView::new(&mut maps.world)
        .spawn_levels_in(&rooms, area)
        .is_empty(),
      "a disabled root still holds its level"
    );
    // Parked while its landing is out, too
    let other = Rect::new(40.0, 10.0, 4.0, 4.0);
    let second = MapsView::new(&mut maps.world).spawn_levels_in(&rooms, other);
    assert_eq!(second.len(), 1);
    maps.world.entity_mut(second[0]).insert(Disabled);
    assert!(
      MapsView::new(&mut maps.world)
        .spawn_levels_in(&rooms, other)
        .is_empty()
    );
  }

  #[test]
  fn levels_spawn_with_the_roots_the_game_sets_up() {
    use crate::core::physics::TileCell;
    use crate::core::tilemap::IntGridKey;
    use crate::core::tilemap::spawn::MapSource;
    use crate::core::tilemap::test_util::load_map;
    let mut maps = MapWorld::new();
    let rooms = load_world(&mut maps.world, "tiled/worlds/rooms.world").expect("loads");
    let other = load_map(&mut maps.world, "tiled/worlds/d.tmj");
    let mut seen = Vec::new();
    let spawned = MapsView::new(&mut maps.world).spawn_levels_in_with(
      &rooms,
      Rect::new(30.0, 0.0, 10.0, 10.0),
      0,
      |level, _| {
        seen.push(level.name.clone());
        // A root naming another map: only its settings count
        MapRoot::new(other.clone())
          .with_z(-10)
          .with_entity_tiles(&["coin"])
          .with_intgrid_collision(3, TileCell::OneWay)
          .without_collision()
      },
    );
    assert_eq!(seen, ["a.tmj", "b.tmj"]);
    assert_eq!(spawned.len(), 2);
    for (index, &entity) in spawned.iter().enumerate() {
      let Some(root) = maps.world.get::<MapRoot>(entity) else {
        panic!("no root");
      };
      assert_eq!(root.z, -10);
      assert_eq!(root.entity_tiles, ["coin"]);
      assert_eq!(
        root.intgrid_collision,
        [(IntGridKey::Value(3), TileCell::OneWay)]
      );
      assert!(!root.collision);
      match &root.source {
        MapSource::WorldLevel { world, level } => {
          assert_eq!((world, *level), (&WorldRef::Tiled(rooms.clone()), index));
        }
        other => panic!("the level's map was replaced: {other:?}"),
      }
    }
    maps.frames_until(maps_landed);
    for &entity in &spawned {
      assert!(MapsView::new(&mut maps.world).is_ready(entity));
      assert!(
        maps
          .world
          .get::<crate::core::physics::TileColliders>(entity)
          .is_none(),
        "collision off"
      );
    }
  }

  #[test]
  fn an_ldtk_world_streams_one_depth_at_a_time() {
    let mut maps = MapWorld::new();
    let project = AssetsView::new(&mut maps.world)
      .load_now::<LdtkProject>("ldtk/worlds.ldtk")
      .expect("worlds.ldtk loads");
    let overworld = WorldRef::Ldtk {
      project,
      world: "Overworld".into(),
    };
    // Inside C (depth 0) and Over (depth 0), under Upper (depth 1)
    let area = Rect::new(90.0, 64.0, 4.0, 4.0);
    let names = |world: &crate::core::ecs::World, depth: i32| -> Vec<String> {
      let Some(layout) = overworld.layout(world) else {
        panic!("not loaded");
      };
      layout
        .levels_in(area, depth)
        .map(|(_, l)| l.name.clone())
        .collect()
    };
    assert_eq!(names(&maps.world, 0), ["C", "Over"]);
    assert_eq!(names(&maps.world, 1), ["Upper"]);
    assert!(names(&maps.world, -1).is_empty());
    let level_names = |maps: &mut MapWorld, roots: &[Entity]| -> Vec<String> {
      let Some(layout) = overworld.layout(&maps.world) else {
        panic!("not loaded");
      };
      roots
        .iter()
        .filter_map(|&root| maps.world.get::<WorldLevelOf>(root))
        .map(|of| layout.levels[of.level].name.clone())
        .collect()
    };
    // The ground floor by default, the floor above when asked
    let ground = MapsView::new(&mut maps.world).spawn_levels_in(overworld.clone(), area);
    assert_eq!(level_names(&mut maps, &ground), ["C", "Over"]);
    let upper =
      MapsView::new(&mut maps.world)
        .spawn_levels_in_with(overworld.clone(), area, 1, |_, root| root);
    assert_eq!(level_names(&mut maps, &upper), ["Upper"]);
    maps.frames_until(maps_landed);
    assert!(
      MapsView::new(&mut maps.world)
        .spawn_levels_in_with(overworld, area, 1, |_, root| root)
        .is_empty()
    );
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

  const MAP: &str = r#"{"type":"map","orientation":"orthogonal","width":2,"height":2,"tilewidth":16,"tileheight":16,"tilesets":[],"layers":[]}"#;

  fn world_from(
    extra: &[(&str, &str)],
    path: &str,
  ) -> (crate::core::ecs::World, Result<Handle<TileWorld>, String>) {
    let mut world = world_over(files_with(extra));
    let loaded = load_world(&mut world, path);
    (world, loaded)
  }

  fn positions(layout: &WorldLayout) -> Vec<(String, Vec2)> {
    layout
      .levels
      .iter()
      .map(|l| (l.name.clone(), l.position))
      .collect()
  }

  #[test]
  fn huge_multipliers_and_offsets_fail_the_load_instead_of_panicking() {
    for fields in [
      r#""multiplierX":2000000000,"offsetX":2000000000"#,
      r#""multiplierY":2000000000,"multiplierX":1,"offsetY":2000000000"#,
      r#""multiplierX":-2000000000,"offsetX":-2000000000"#,
    ] {
      let json = format!(r#"{{"patterns":[{{"regexp":"grid_(\\d+)_(\\d+)\\.tmj",{fields}}}]}}"#);
      let (_, loaded) = world_from(
        &[("tiled/worlds/huge.world", &json)],
        "tiled/worlds/huge.world",
      );
      let error = loaded.expect_err("fails");
      assert!(
        error.contains("huge.world") && error.contains("grid_(\\d+)_(\\d+)"),
        "{error}"
      );
    }
  }

  #[test]
  fn a_bare_dot_in_a_pattern_is_a_dot() {
    let json =
      r#"{"patterns":[{"regexp":"grid_(\\d+)_(\\d+).tmj","multiplierX":32,"multiplierY":32}]}"#;
    let (world, loaded) = world_from(
      &[("tiled/worlds/dot.world", json)],
      "tiled/worlds/dot.world",
    );
    let layout = layout_of(&world, &loaded.expect("loads"));
    assert_eq!(layout.levels.len(), 3);
    for regexp in [r"grid_.*", r"grid_.+"] {
      let json = format!(r#"{{"patterns":[{{"regexp":"{regexp}"}}]}}"#);
      let (_, loaded) = world_from(&[("tiled/worlds/q.world", &json)], "tiled/worlds/q.world");
      assert!(loaded.expect_err("fails").contains("not supported"));
    }
  }

  #[test]
  fn a_trailing_escaped_dollar_is_a_dollar() {
    use crate::core::tilemap::tiled::world_file::{Part, parse_pattern};
    assert_eq!(parse_pattern(r"foo\$"), Ok(vec![Part::Text("foo$".into())]));
    assert_eq!(parse_pattern("^foo$"), Ok(vec![Part::Text("foo".into())]));
  }

  #[test]
  fn a_one_number_pattern_runs_along_x() {
    let json = r#"{"patterns":[{"regexp":"one_(\\d+)\\.tmj","multiplierX":40,"multiplierY":99,"offsetX":5,"offsetY":7}]}"#;
    let (world, loaded) = world_from(
      &[
        ("tiled/worlds/one.world", json),
        ("tiled/worlds/one_0.tmj", MAP),
        ("tiled/worlds/one_1.tmj", MAP),
        ("tiled/worlds/one_2.tmj", MAP),
        ("tiled/worlds/one_4.tmj", MAP),
      ],
      "tiled/worlds/one.world",
    );
    let layout = layout_of(&world, &loaded.expect("loads"));
    // one row (y = 0), stopping at the gap
    assert_eq!(
      positions(&layout),
      [
        ("one_0.tmj".to_string(), Vec2::new(5.0, 7.0)),
        ("one_1.tmj".to_string(), Vec2::new(45.0, 7.0)),
        ("one_2.tmj".to_string(), Vec2::new(85.0, 7.0)),
      ]
    );
  }

  #[test]
  fn a_pattern_finds_numbers_from_zero_written_plainly() {
    let pattern = |regexp: &str| format!(r#"{{"patterns":[{{"regexp":"{regexp}"}}]}}"#);
    // Zero-padded or numbered from 1: no map matches
    for (regexp, files) in [
      (r"pad_(\\d+)\\.tmj", ["pad_00.tmj", "pad_01.tmj"]),
      (r"one_(\\d+)\\.tmj", ["one_1.tmj", "one_2.tmj"]),
    ] {
      let json = pattern(regexp);
      let extra: Vec<(String, &str)> = files
        .iter()
        .map(|f| (format!("tiled/worlds/{f}"), MAP))
        .chain([("tiled/worlds/w.world".to_string(), json.as_str())])
        .collect();
      let extra: Vec<(&str, &str)> = extra.iter().map(|(p, t)| (p.as_str(), *t)).collect();
      let (_, loaded) = world_from(&extra, "tiled/worlds/w.world");
      assert!(loaded.expect_err("fails").contains("no map"), "{regexp}");
    }
    // A literal 0 reaches 00 to 09 only: 10 is built as 010
    let mut extra: Vec<(String, String)> = (0..12)
      .map(|n| (format!("tiled/worlds/z_{n:02}.tmj"), MAP.to_string()))
      .collect();
    extra.push(("tiled/worlds/z.world".into(), pattern(r"z_0(\\d+)\\.tmj")));
    let extra: Vec<(&str, &str)> = extra
      .iter()
      .map(|(p, t)| (p.as_str(), t.as_str()))
      .collect();
    let (world, loaded) = world_from(&extra, "tiled/worlds/z.world");
    let layout = layout_of(&world, &loaded.expect("loads"));
    let names: Vec<String> = layout.levels.iter().map(|l| l.name.clone()).collect();
    let expected: Vec<String> = (0..10).map(|n| format!("z_{n:02}.tmj")).collect();
    assert_eq!(names, expected);
  }

  #[test]
  fn a_pattern_without_numbers_is_one_file() {
    let json = r#"{"patterns":[{"regexp":"only\\.tmj","offsetX":3,"offsetY":4}]}"#;
    let (world, loaded) = world_from(
      &[
        ("tiled/worlds/zero.world", json),
        ("tiled/worlds/only.tmj", MAP),
      ],
      "tiled/worlds/zero.world",
    );
    let layout = layout_of(&world, &loaded.expect("loads"));
    assert_eq!(
      positions(&layout),
      [("only.tmj".to_string(), Vec2::new(3.0, 4.0))]
    );
  }

  #[test]
  fn north_east_and_south_west_neighbours() {
    use NeighbourDirection::*;
    let json = r#"{"maps":[{"fileName":"a.tmj","x":0,"y":32,"width":32,"height":32},
      {"fileName":"b.tmj","x":32,"y":0,"width":32,"height":32}]}"#;
    let (world, loaded) = world_from(
      &[("tiled/worlds/diag.world", json)],
      "tiled/worlds/diag.world",
    );
    let layout = layout_of(&world, &loaded.expect("loads"));
    assert_eq!(neighbours(&layout, "a.tmj"), [("b.tmj".into(), NorthEast)]);
    assert_eq!(neighbours(&layout, "b.tmj"), [("a.tmj".into(), SouthWest)]);
  }

  #[test]
  fn a_broken_map_reached_by_a_probe_fails_the_world_naming_it() {
    let json = r#"{"patterns":[{"regexp":"bad_(\\d+)\\.tmj"}]}"#;
    let (_, loaded) = world_from(
      &[
        ("tiled/worlds/bad.world", json),
        ("tiled/worlds/bad_0.tmj", "{"),
      ],
      "tiled/worlds/bad.world",
    );
    let error = loaded.expect_err("fails");
    assert!(
      error.contains("bad.world") && error.contains("bad_0.tmj"),
      "{error}"
    );
  }

  #[test]
  fn two_tiled_worlds_keep_their_levels_apart() {
    let rooms = std::fs::read_to_string(format!(
      "{}/tiled/worlds/rooms.world",
      crate::core::tilemap::test_util::FIXTURES
    ))
    .expect("fixture");
    let mut maps = MapWorld::over(files_with(&[("tiled/worlds/copy.world", &rooms)]));
    let first = load_world(&mut maps.world, "tiled/worlds/rooms.world").expect("loads");
    let second = load_world(&mut maps.world, "tiled/worlds/copy.world").expect("loads");
    let area = Rect::new(0.0, 0.0, 10.0, 10.0);
    assert_eq!(
      MapsView::new(&mut maps.world)
        .spawn_levels_in(&first, area)
        .len(),
      1
    );
    assert_eq!(
      MapsView::new(&mut maps.world)
        .spawn_levels_in(&second, area)
        .len(),
      1
    );
    assert!(
      MapsView::new(&mut maps.world)
        .spawn_levels_in(&first, area)
        .is_empty()
    );
  }
}
