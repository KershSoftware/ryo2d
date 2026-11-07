use std::sync::Arc;

use glam::{IVec2, UVec2, Vec2};

use super::{Climber, FloorChanged, StairRun};
use crate::core::asset::Assets;
use crate::core::ecs::{
  ChildOf, Entity, Floor, Interpolated, MessageCursor, Messages, Transform, World,
};
use crate::core::engine::EngineBuilder;
use crate::core::gameplay::{MoveIntent, TopDownMovement};
use crate::core::physics::Collider;
use crate::core::scene::Scene;
use crate::core::testing::HeadlessEngine;
use crate::core::tilemap::test_util::MapWorld;
use crate::core::tilemap::{
  GridShape, MapFloors, MapRoot, MapsView, StaggerAxis, StaggerIndex, Tile, TileData, TileFlip,
  TileMap, TileShape, Tileset, TilesetImage, TilesetSource,
};
use crate::core::time::Time;
use crate::core::trail::Trail2D;
use crate::core::ui::Color;

/// Tiles of `steps`
const PLAIN: Tile = tile(0);
const WALL: Tile = tile(1);
const UP_1: Tile = tile(2);
const UP_2: Tile = tile(3);
const UP_5: Tile = tile(4);
/// A plain tile that a tileset saved later makes stairs up to floor 1
const CRATE: Tile = tile(5);

const fn tile(id: u32) -> Tile {
  Tile {
    tileset: 0,
    id,
    flip: TileFlip::NONE,
  }
}

/// Floor 1's height in the maps here, and floor 2's
const UP: Vec2 = Vec2::new(0.0, -32.0);
const HIGH: Vec2 = Vec2::new(0.0, -64.0);

/// 16 px tiles: plain ground, a solid block, and stairs up to floors 1, 2
/// and 5
fn steps() -> Tileset {
  let mut set = Tileset::new("steps", UVec2::splat(16), TilesetImage::Collection);
  let block = vec![
    Vec2::ZERO,
    Vec2::new(16.0, 0.0),
    Vec2::splat(16.0),
    Vec2::new(0.0, 16.0),
  ];
  let wall = TileData {
    shapes: vec![TileShape {
      points: block,
      ..TileShape::default()
    }],
    ..TileData::default()
  };
  set.insert_tile(WALL.id, wall);
  for (stairs, to) in [(UP_1, 1), (UP_2, 2), (UP_5, 5)] {
    let data = TileData {
      stairs: Some(to),
      ..TileData::default()
    };
    set.insert_tile(stairs.id, data);
  }
  set
}

/// A tile layer: its name, floor, offset and tiles
type Storey<'a> = (&'a str, u8, Vec2, &'a [(IVec2, Tile)]);

/// `map` over `source` (its tileset 0), with a tile layer for each of
/// `layers`, each tile set in order
fn with_layers(mut map: TileMap, source: TilesetSource, layers: &[Storey<'_>]) -> TileMap {
  map.add_tileset(source);
  for &(name, floor, offset, tiles) in layers {
    let layer = map.add_tile_layer(name);
    map.layers[layer].info.floor = floor;
    map.layers[layer].info.offset = offset;
    let grid = map.tiles_mut(layer).expect("a tile layer");
    for &(cell, tile) in tiles {
      grid.set(cell, Some(tile));
    }
  }
  map
}

/// A map of 20 x 10 cells of 16 px over `steps`, with `layers`
fn storeyed(layers: &[Storey<'_>]) -> TileMap {
  let map = TileMap::new(UVec2::new(20, 10), UVec2::splat(16));
  with_layers(map, TilesetSource::Embedded(Arc::new(steps())), layers)
}

/// The cells from `min` to `max` (both included), each holding `tile`
fn block(min: (i32, i32), max: (i32, i32), tile: Tile) -> Vec<(IVec2, Tile)> {
  (min.1..=max.1)
    .flat_map(|y| (min.0..=max.0).map(move |x| (IVec2::new(x, y), tile)))
    .collect()
}

/// A layer's tiles, by cell
type Tiles = Vec<(IVec2, Tile)>;

/// The ground over the whole map, its stairs up to floor 1 at `stairs`, and
/// floor 1, 32 px up, over (8, 3) to (13, 6)
fn hall_with(stairs: &[(i32, i32)]) -> (Tiles, Tiles) {
  let mut ground = block((0, 0), (19, 9), PLAIN);
  ground.extend(stairs.iter().map(|&(x, y)| (IVec2::new(x, y), UP_1)));
  (ground, block((8, 3), (13, 6), PLAIN))
}

/// `hall_with` a run of two stairs at (6, 4) and (7, 4): its foot is its
/// left side at x 96, its top where it meets floor 1 at x 128, both at y 72
fn hall() -> TileMap {
  let (ground, upper) = hall_with(&[(6, 4), (7, 4)]);
  storeyed(&[("ground", 0, Vec2::ZERO, &ground), ("upper", 1, UP, &upper)])
}

/// `map` spawned and landed
fn land(maps: &mut MapWorld, map: TileMap) -> Entity {
  let handle = maps.world.resource_mut::<Assets<TileMap>>().add(map);
  let map = maps.spawn(MapRoot::new(handle));
  maps.frame();
  map
}

/// A run as the tests compare it: its floors, cells, bottom and top
type Summary = (u8, u8, Vec<IVec2>, Vec2, Vec2);

/// The runs of `map`, by their floors and first cell
fn runs(world: &World, map: Entity) -> Vec<StairRun> {
  let mut runs = world
    .get::<MapFloors>(map)
    .map(|floors| floors.runs.clone())
    .unwrap_or_default();
  runs.sort_by_key(|run| (run.from, run.to, run.cells.first().map(|c| (c.y, c.x))));
  runs
}

fn summaries(world: &World, map: Entity) -> Vec<Summary> {
  runs(world, map)
    .into_iter()
    .map(|run| (run.from, run.to, run.cells, run.bottom, run.top))
    .collect()
}

/// Whether two lists of runs agree, their points within a thousandth
fn same_runs(got: &[Summary], want: &[Summary]) -> bool {
  got.len() == want.len()
    && got
      .iter()
      .zip(want)
      .all(|(a, b)| a.0 == b.0 && a.1 == b.1 && a.2 == b.2 && near(a.3, b.3) && near(a.4, b.4))
}

fn near(a: Vec2, b: Vec2) -> bool {
  (a - b).length() < 1e-3
}

fn cells(list: &[(i32, i32)]) -> Vec<IVec2> {
  list.iter().map(|&(x, y)| IVec2::new(x, y)).collect()
}

/// A climber standing at `at`, on the ground floor
fn climber_at(maps: &mut MapWorld, at: Vec2) -> Entity {
  maps
    .world
    .spawn((Transform::from_translation(at), Climber))
    .id()
}

/// Puts `entity` at `at`, and runs the two frames a climber's floor takes
/// to follow (the climb reads where the last step left it)
fn move_to(maps: &mut MapWorld, entity: Entity, at: Vec2) {
  if let Some(mut transform) = maps.world.get_mut::<Transform>(entity) {
    transform.translation = at;
  }
  maps.frame();
  maps.frame();
}

fn floor_of(world: &World, entity: Entity) -> Option<Floor> {
  world.get::<Floor>(entity).copied()
}

fn lift_of(world: &mut World, entity: Entity) -> Vec2 {
  MapsView::new(world).drawn_lift(entity)
}

/// The floor changes written since `cursor` last read
fn changes(world: &World, cursor: &mut MessageCursor<FloorChanged>) -> Vec<FloorChanged> {
  cursor
    .read(world.resource::<Messages<FloorChanged>>())
    .copied()
    .collect()
}

/// The warnings in the dev log holding every one of `words`
fn warned(words: &[&str]) -> usize {
  crate::core::dev_log::get_entries()
    .iter()
    .filter(|entry| words.iter().all(|word| entry.message.contains(word)))
    .count()
}

struct Empty;

impl Scene for Empty {}

/// An engine with `map` spawned and landed, and the map's entity
fn engine_with(map: TileMap) -> Option<(HeadlessEngine, Entity)> {
  let builder = EngineBuilder::new().resolution(320, 180).start_scene(Empty);
  let mut engine = HeadlessEngine::new_or_skip(builder)?;
  let handle = engine
    .world_mut()
    .resource_mut::<Assets<TileMap>>()
    .add(map);
  let map = engine.world_mut().spawn(MapRoot::new(handle)).id();
  crate::core::tilemap::test_util::step_until_maps_land(&mut engine);
  Some((engine, map))
}

#[test]
fn stairs_join_into_runs_with_a_bottom_and_a_top() {
  // Two runs up to floor 1 on the ground: two cells leading right into it
  // at row 4, and a block of four above it leading down into its top row;
  // one up to floor 2 on floor 1, a cell leading right into it
  let (mut ground, mut upper) = hall_with(&[(6, 4), (7, 4)]);
  ground.extend(block((10, 1), (11, 2), UP_1));
  upper.push((IVec2::new(9, 5), UP_2));
  let roof = block((10, 4), (12, 5), PLAIN);
  let mut maps = MapWorld::new();
  let map = land(
    &mut maps,
    storeyed(&[
      ("ground", 0, Vec2::ZERO, &ground),
      ("upper", 1, UP, &upper),
      ("roof", 2, HIGH, &roof),
    ]),
  );
  // Each run's top is the middle of the sides it shares with its upper
  // floor's cells; its bottom is its foot, the side farthest from its top
  // along the run (not the middle of every other side, which takes in its
  // sides along it)
  let want: Vec<Summary> = vec![
    (
      0,
      1,
      cells(&[(10, 1), (11, 1), (10, 2), (11, 2)]),
      Vec2::new(176.0, 16.0),
      Vec2::new(176.0, 48.0),
    ),
    (
      0,
      1,
      cells(&[(6, 4), (7, 4)]),
      Vec2::new(96.0, 72.0),
      Vec2::new(128.0, 72.0),
    ),
    (
      1,
      2,
      cells(&[(9, 5)]),
      Vec2::new(144.0, 88.0),
      Vec2::new(160.0, 88.0),
    ),
  ];
  let got = summaries(&maps.world, map);
  assert!(same_runs(&got, &want), "{got:?}");
  // A map without floors has none, nor floors to keep them in
  let plain = land(&mut maps, storeyed(&[("ground", 0, Vec2::ZERO, &ground)]));
  assert!(maps.world.get::<MapFloors>(plain).is_none());
  assert!(maps.world.get::<super::MapStairs>(plain).is_none());
}

#[test]
fn a_climber_changes_floor_halfway_up_and_halfway_down() {
  // The hall, with walls on the ground where its stairs arrive (the cliff
  // under floor 1) and down column 2
  let (ground, upper) = hall_with(&[(6, 4), (7, 4)]);
  let mut walls = block((8, 3), (13, 6), WALL);
  walls.extend(block((2, 0), (2, 9), WALL));
  let map = storeyed(&[
    ("ground", 0, Vec2::ZERO, &ground),
    ("walls", 0, Vec2::ZERO, &walls),
    ("upper", 1, UP, &upper),
  ]);
  let Some((mut engine, _)) = engine_with(map) else {
    return;
  };
  // A top-down climber 10 px across on the ground, left of the stairs
  let hero = engine
    .world_mut()
    .spawn((
      Transform::from_xy(64.0, 72.0),
      Collider::circle(5.0),
      TopDownMovement::default(),
      Climber,
    ))
    .id();
  let mut cursor = MessageCursor::<FloorChanged>::default();
  // `steps` frames heading along `direction`: each change with where the
  // hero stood after its step, and where it ends
  let mut walk = |engine: &mut HeadlessEngine, direction: Vec2, steps: usize| {
    let mut seen = Vec::new();
    for _ in 0..steps {
      if let Some(mut intent) = engine.world_mut().get_mut::<MoveIntent>(hero) {
        intent.direction = direction;
      }
      engine.step().expect("a frame");
      let world = engine.world();
      let at = world
        .get::<Transform>(hero)
        .map_or(Vec2::NAN, |t| t.translation);
      seen.extend(changes(world, &mut cursor).into_iter().map(|c| (c, at.x)));
    }
    let at = engine
      .world()
      .get::<Transform>(hero)
      .map_or(Vec2::NAN, |t| t.translation);
    (seen, at)
  };

  // Up: one change, halfway up the run (x 112, the step after the one
  // that crossed it), and the edge holds it on floor 1 at its far side
  let (seen, at) = walk(&mut engine, Vec2::X, 100);
  let up = FloorChanged {
    entity: hero,
    from: Floor::GROUND,
    to: Floor::On(1),
  };
  assert_eq!(seen.iter().map(|(c, _)| *c).collect::<Vec<_>>(), [up]);
  assert!(
    (112.0..118.0).contains(&seen[0].1),
    "changed at {}",
    seen[0].1
  );
  assert!((at.x - 219.0).abs() < 1.0, "on floor 1 at {at}");
  assert_eq!(floor_of(engine.world(), hero), Some(Floor::On(1)));
  // Down: one change, halfway down, and the ground's wall stops it after
  let (seen, at) = walk(&mut engine, Vec2::NEG_X, 110);
  let down = FloorChanged {
    entity: hero,
    from: Floor::On(1),
    to: Floor::GROUND,
  };
  assert_eq!(seen.iter().map(|(c, _)| *c).collect::<Vec<_>>(), [down]);
  assert!(
    (106.0..112.0).contains(&seen[0].1),
    "changed at {}",
    seen[0].1
  );
  assert!((at.x - 53.0).abs() < 1.0, "on the ground at {at}");
  assert_eq!(floor_of(engine.world(), hero), Some(Floor::GROUND));
}

#[test]
fn a_climber_s_lift_eases_along_the_stairs() {
  let mut maps = MapWorld::new();
  land(&mut maps, hall());
  // A climber at the run's foot, its sprite a child
  let hero = climber_at(&mut maps, Vec2::new(96.0, 72.0));
  let sprite = maps
    .world
    .spawn((Transform::from_xy(0.0, -8.0), ChildOf(hero)))
    .id();
  maps.frame();
  // t = 0: on the ground floor, where it stands
  assert_eq!(floor_of(&maps.world, hero), Some(Floor::GROUND));
  assert!(near(lift_of(&mut maps.world, hero), Vec2::ZERO));
  // t = 0.5: on floor 1 now, halfway up
  move_to(&mut maps, hero, Vec2::new(112.0, 72.0));
  assert_eq!(floor_of(&maps.world, hero), Some(Floor::On(1)));
  for entity in [hero, sprite] {
    let lift = lift_of(&mut maps.world, entity);
    assert!(near(lift, Vec2::new(0.0, -16.0)), "{entity}: {lift}");
  }
  // t = 1, its top: floor 1's height
  move_to(&mut maps, hero, Vec2::new(128.0, 72.0));
  assert!(near(lift_of(&mut maps.world, hero), UP));
  // A quarter up, coming down: on the ground floor again, a quarter up
  move_to(&mut maps, hero, Vec2::new(104.0, 72.0));
  assert_eq!(floor_of(&maps.world, hero), Some(Floor::GROUND));
  for entity in [hero, sprite] {
    let lift = lift_of(&mut maps.world, entity);
    assert!(near(lift, Vec2::new(0.0, -8.0)), "{entity}: {lift}");
  }

  // Between fixed steps the lift follows where the climber draws
  let Some((mut engine, _)) = engine_with(hall()) else {
    return;
  };
  let hero = engine
    .world_mut()
    .spawn((
      Transform::from_xy(100.0, 72.0),
      Climber,
      Interpolated,
      Trail2D::new(Color::WHITE, 2.0),
    ))
    .id();
  engine.step_frames(2).expect("frames");
  // An eighth up
  assert!(near(
    lift_of(engine.world_mut(), hero),
    Vec2::new(0.0, -4.0)
  ));
  // Moved to x 108 in a frame whose one step leaves half a step over: it
  // draws halfway between, at x 104, a quarter up
  if let Some(mut transform) = engine.world_mut().get_mut::<Transform>(hero) {
    transform.translation.x = 108.0;
  }
  let half = engine.world().resource::<Time>().fixed.timestep_duration() / 2;
  engine.world_mut().resource_mut::<Time>().advance(half);
  engine.step().expect("a frame");
  let alpha = engine.world().resource::<Time>().fixed.overstep_fraction();
  assert!((alpha - 0.5).abs() < 1e-3, "{alpha}");
  let lift = lift_of(engine.world_mut(), hero);
  assert!(near(lift, Vec2::new(0.0, -8.0)), "{lift}");
  // while the step itself lifted it where it stood, x 108, three eighths
  // up: its trail's newest point
  let newest = engine
    .world()
    .get::<Trail2D>(hero)
    .and_then(|trail| trail.points().front().map(|p| Vec2::new(p.x, p.y)));
  assert!(
    newest.is_some_and(|p| near(p, Vec2::new(108.0, 60.0))),
    "{newest:?}"
  );
}

#[test]
fn a_run_s_lift_eases_over_its_whole_length() {
  // A run of three: its foot at x 80, its top at x 128
  let (ground, upper) = hall_with(&[(5, 4), (6, 4), (7, 4)]);
  let mut maps = MapWorld::new();
  let map = land(
    &mut maps,
    storeyed(&[("ground", 0, Vec2::ZERO, &ground), ("upper", 1, UP, &upper)]),
  );
  let want: Vec<Summary> = vec![(
    0,
    1,
    cells(&[(5, 4), (6, 4), (7, 4)]),
    Vec2::new(80.0, 72.0),
    Vec2::new(128.0, 72.0),
  )];
  let got = summaries(&maps.world, map);
  assert!(same_runs(&got, &want), "{got:?}");
  // The lift rises evenly from its foot to its top, from the first step
  let hero = climber_at(&mut maps, Vec2::new(80.0, 72.0));
  for x in [80.0, 88.0, 96.0, 104.0, 112.0, 120.0, 128.0] {
    move_to(&mut maps, hero, Vec2::new(x, 72.0));
    let lift = lift_of(&mut maps.world, hero);
    let rise = -32.0 * (x - 80.0) / 48.0;
    assert!(near(lift, Vec2::new(0.0, rise)), "at {x}: {lift}");
  }
}

#[test]
fn a_non_climber_keeps_its_floor_on_stairs() {
  let mut maps = MapWorld::new();
  land(&mut maps, hall());
  let mut cursor = MessageCursor::<FloorChanged>::default();
  // One on the ground three quarters up, one there with a `Floor` of its
  // own, and one on floor 1 a quarter up
  let low = maps.world.spawn(Transform::from_xy(120.0, 72.0)).id();
  let ground = maps
    .world
    .spawn((Transform::from_xy(120.0, 72.0), Floor::GROUND))
    .id();
  let high = maps
    .world
    .spawn((Transform::from_xy(104.0, 72.0), Floor::On(1)))
    .id();
  for _ in 0..3 {
    maps.frame();
  }
  assert_eq!(floor_of(&maps.world, low), None);
  assert_eq!(floor_of(&maps.world, ground), Some(Floor::GROUND));
  assert_eq!(floor_of(&maps.world, high), Some(Floor::On(1)));
  // Each draws at its own floor's height, not eased
  assert!(near(lift_of(&mut maps.world, low), Vec2::ZERO));
  assert!(near(lift_of(&mut maps.world, ground), Vec2::ZERO));
  assert!(near(lift_of(&mut maps.world, high), UP));
  assert!(changes(&maps.world, &mut cursor).is_empty());
}

#[test]
fn stairs_to_a_missing_floor_or_down_warn_once_and_stay_plain() {
  // Floors 0, 1 and 2: stairs up to floor 5 (which the map lacks) on the
  // ground, up to floor 1 on floor 1 (its own) and on floor 2 (leading
  // down), each beside the floor above it
  let mut ground = block((0, 0), (19, 9), PLAIN);
  ground.push((IVec2::new(7, 4), UP_5));
  let mut upper = block((8, 3), (13, 6), PLAIN);
  upper.push((IVec2::new(9, 4), UP_1));
  let mut roof = block((10, 4), (12, 5), PLAIN);
  roof.push((IVec2::new(11, 4), UP_1));
  let mut maps = MapWorld::new();
  let map = land(
    &mut maps,
    storeyed(&[
      ("ground-w5p", 0, Vec2::ZERO, &ground),
      ("upper-w5p", 1, UP, &upper),
      ("roof-w5p", 2, HIGH, &roof),
    ]),
  );
  assert!(runs(&maps.world, map).is_empty());
  assert_eq!(warned(&["ground-w5p", "plain tiles"]), 1);
  // A climber on each keeps its floor
  let on = |maps: &mut MapWorld, x: f32, y: f32, floor: Floor| {
    maps
      .world
      .spawn((Transform::from_xy(x, y), Climber, floor))
      .id()
  };
  let climbers = [
    (on(&mut maps, 120.0, 72.0, Floor::GROUND), Floor::GROUND),
    (on(&mut maps, 152.0, 72.0, Floor::On(1)), Floor::On(1)),
    (on(&mut maps, 184.0, 72.0, Floor::On(2)), Floor::On(2)),
  ];
  for _ in 0..3 {
    maps.frame();
  }
  for (climber, floor) in climbers {
    assert_eq!(floor_of(&maps.world, climber), Some(floor), "{climber}");
  }
  // More of them, painted later, say nothing more
  MapsView::new(&mut maps.world).set_tile(map, "ground-w5p", IVec2::new(7, 5), Some(UP_5));
  MapsView::new(&mut maps.world).set_tile(map, "upper-w5p", IVec2::new(9, 5), Some(UP_1));
  for _ in 0..3 {
    maps.frame();
  }
  assert!(runs(&maps.world, map).is_empty());
  assert_eq!(warned(&["w5p", "plain tiles"]), 1);
}

#[test]
fn a_run_that_leads_nowhere_is_warned_once_and_ignored() {
  // Floor 1 over (8, 3) to (13, 6), and two cells more at (14, 8) and
  // (16, 8). Stairs up to it on the ground: at (2, 2), touching none of its
  // cells (no top); at (10, 5), under it, every side on its cells (no
  // bottom); at (15, 8), between its two cells, its top as far along the
  // run as the middle of its other sides (no way up); and on a ground layer
  // standing 8 px right, beside it at (7, 4) (another place)
  let (ground, mut upper) = hall_with(&[(2, 2), (10, 5), (15, 8)]);
  upper.extend([(IVec2::new(14, 8), PLAIN), (IVec2::new(16, 8), PLAIN)]);
  let aside = [(IVec2::new(7, 4), UP_1)];
  let mut maps = MapWorld::new();
  let map = land(
    &mut maps,
    storeyed(&[
      ("ground-n4z", 0, Vec2::ZERO, &ground),
      ("aside-n4z", 0, Vec2::new(8.0, 0.0), &aside),
      ("upper-n4z", 1, UP, &upper),
    ]),
  );
  assert!(runs(&maps.world, map).is_empty());
  assert_eq!(warned(&["n4z", "lead nowhere"]), 1);
  // A climber on any of them keeps its floor
  let walkers = [(40.0, 40.0), (168.0, 88.0), (248.0, 136.0), (128.0, 72.0)];
  let walkers: Vec<Entity> = walkers
    .into_iter()
    .map(|(x, y)| climber_at(&mut maps, Vec2::new(x, y)))
    .collect();
  for _ in 0..3 {
    maps.frame();
  }
  for walker in walkers {
    assert_eq!(
      floor_of(&maps.world, walker),
      Some(Floor::GROUND),
      "{walker}"
    );
    assert!(near(lift_of(&mut maps.world, walker), Vec2::ZERO));
  }
  // Another, painted later, says nothing more
  MapsView::new(&mut maps.world).set_tile(map, "ground-n4z", IVec2::new(2, 7), Some(UP_1));
  for _ in 0..3 {
    maps.frame();
  }
  assert_eq!(warned(&["n4z", "lead nowhere"]), 1);
}

#[test]
fn runs_follow_edits_to_the_stairs() {
  // The hall, its tileset an asset
  let mut maps = MapWorld::new();
  let set = maps.world.resource_mut::<Assets<Tileset>>().add(steps());
  let (ground, upper) = hall_with(&[(6, 4), (7, 4)]);
  let layers: [Storey<'_>; 2] = [("ground", 0, Vec2::ZERO, &ground), ("upper", 1, UP, &upper)];
  let level = with_layers(
    TileMap::new(UVec2::new(20, 10), UVec2::splat(16)),
    TilesetSource::Asset(set.clone()),
    &layers,
  );
  let handle = maps.world.resource_mut::<Assets<TileMap>>().add(level);
  let map = maps.spawn(MapRoot::new(handle.clone()));
  maps.frame();
  let two: Vec<Summary> = vec![(
    0,
    1,
    cells(&[(6, 4), (7, 4)]),
    Vec2::new(96.0, 72.0),
    Vec2::new(128.0, 72.0),
  )];
  let got = summaries(&maps.world, map);
  assert!(same_runs(&got, &two), "{got:?}");
  // A climber halfway up
  let hero = climber_at(&mut maps, Vec2::new(112.0, 72.0));
  maps.frame();
  maps.frame();
  assert_eq!(floor_of(&maps.world, hero), Some(Floor::On(1)));
  assert!(near(lift_of(&mut maps.world, hero), Vec2::new(0.0, -16.0)));

  // A step painted at its foot: the run takes it in, and its foot moves
  // down a cell, so the climber is now two thirds up
  MapsView::new(&mut maps.world).set_tile(map, "ground", IVec2::new(5, 4), Some(UP_1));
  maps.frame();
  let three: Vec<Summary> = vec![(
    0,
    1,
    cells(&[(5, 4), (6, 4), (7, 4)]),
    Vec2::new(80.0, 72.0),
    Vec2::new(128.0, 72.0),
  )];
  let got = summaries(&maps.world, map);
  assert!(same_runs(&got, &three), "{got:?}");
  let lift = lift_of(&mut maps.world, hero);
  assert!(near(lift, Vec2::new(0.0, -64.0 / 3.0)), "{lift}");

  // Its stairs painted over with crates: no run, and the climber, still
  // on floor 1, draws at its height
  MapsView::new(&mut maps.world).fill(
    map,
    "ground",
    IVec2::new(5, 4),
    IVec2::new(7, 4),
    Some(CRATE),
  );
  maps.frame();
  assert!(runs(&maps.world, map).is_empty());
  assert_eq!(floor_of(&maps.world, hero), Some(Floor::On(1)));
  assert!(near(lift_of(&mut maps.world, hero), UP));

  // The tileset saved with its crates as stairs up to floor 1
  if let Some(set) = maps.world.resource_mut::<Assets<Tileset>>().get_mut(&set) {
    let data = TileData {
      stairs: Some(1),
      ..TileData::default()
    };
    set.insert_tile(CRATE.id, data);
  }
  maps.frame();
  let got = summaries(&maps.world, map);
  assert!(same_runs(&got, &three), "{got:?}");

  // The map saved with its ground standing 8 px right (a map made in code
  // reloads when told to; a reload brings its cells back to the asset's
  // too): its stairs stand elsewhere than floor 1 and lead nowhere; saved
  // back, the asset's two lead up again
  let save = |maps: &mut MapWorld, layer: usize, offset: Vec2| {
    if let Some(level) = maps
      .world
      .resource_mut::<Assets<TileMap>>()
      .get_mut(&handle)
    {
      level.layers[layer].info.offset = offset;
    }
    crate::core::tilemap::reload::apply_reload(&mut maps.world, map);
    maps.frame();
  };
  save(&mut maps, 0, Vec2::new(8.0, 0.0));
  assert!(runs(&maps.world, map).is_empty());
  save(&mut maps, 0, Vec2::ZERO);
  let got = summaries(&maps.world, map);
  assert!(same_runs(&got, &two), "{got:?}");
  // and with floor 1 raised to 48 px up, the climber halfway up eases to
  // half its new height
  save(&mut maps, 1, Vec2::new(0.0, -48.0));
  let lifted = maps
    .world
    .get::<MapFloors>(map)
    .and_then(|f| f.lift(Floor::On(1)));
  assert_eq!(lifted, Some(Vec2::new(0.0, -48.0)));
  let got = summaries(&maps.world, map);
  assert!(same_runs(&got, &two), "{got:?}");
  let lift = lift_of(&mut maps.world, hero);
  assert!(near(lift, Vec2::new(0.0, -24.0)), "{lift}");
}

#[test]
fn a_climber_climbs_stairs_on_a_hexagonal_map() {
  // Pointy hexagons, odd rows shifted right: stairs up to floor 1 at
  // (2, 3) and (3, 3), leading right into floor 1 over (4, 1) to (6, 5)
  let shape = GridShape::Hexagonal {
    axis: StaggerAxis::Y,
    index: StaggerIndex::Odd,
    side: 12,
  };
  let mut ground = block((0, 0), (7, 6), PLAIN);
  ground.extend([(IVec2::new(2, 3), UP_1), (IVec2::new(3, 3), UP_1)]);
  let upper = block((4, 1), (6, 5), PLAIN);
  let level = TileMap::new(UVec2::new(8, 7), UVec2::splat(32)).with_shape(shape);
  let grid = level.grid();
  let level = with_layers(
    level,
    TilesetSource::Embedded(Arc::new(steps())),
    &[("ground", 0, Vec2::ZERO, &ground), ("upper", 1, UP, &upper)],
  );
  let mut maps = MapWorld::new();
  let map = land(&mut maps, level);
  let runs = runs(&maps.world, map);
  assert_eq!(runs.len(), 1, "{runs:?}");
  let run = &runs[0];
  assert_eq!((run.from, run.to), (0, 1));
  assert_eq!(run.cells, cells(&[(2, 3), (3, 3)]));
  // Its foot is the side it shares with (1, 3), and it climbs along its row
  let foot = (grid.cell_center(IVec2::new(1, 3)) + grid.cell_center(IVec2::new(2, 3))) / 2.0;
  assert!(near(run.bottom, foot), "{run:?}");
  assert!(
    (run.top.y - foot.y).abs() < 1e-3 && run.top.x > foot.x + 32.0,
    "{run:?}"
  );

  // A climber walked up it and back changes floor once each way, halfway,
  // its lift eased all along (never at exactly halfway, t stepping by
  // ninths)
  let mut cursor = MessageCursor::<FloorChanged>::default();
  let hero = climber_at(&mut maps, run.bottom);
  let span = run.top.x - run.bottom.x;
  let mut path: Vec<f32> = (0..=9)
    .map(|i| run.bottom.x + span * i as f32 / 9.0)
    .collect();
  path.extend(path.clone().into_iter().rev());
  for x in path {
    move_to(&mut maps, hero, Vec2::new(x, run.bottom.y));
    let t = (x - run.bottom.x) / span;
    let floor = floor_of(&maps.world, hero);
    let want = if t >= 0.5 {
      Floor::On(1)
    } else {
      Floor::GROUND
    };
    assert_eq!(floor, Some(want), "at t {t}");
    let lift = lift_of(&mut maps.world, hero);
    assert!(near(lift, UP * t), "at t {t}: {lift}");
  }
  let floors: Vec<(Floor, Floor)> = changes(&maps.world, &mut cursor)
    .iter()
    .map(|change| (change.from, change.to))
    .collect();
  assert_eq!(
    floors,
    [(Floor::GROUND, Floor::On(1)), (Floor::On(1), Floor::GROUND)]
  );
}
