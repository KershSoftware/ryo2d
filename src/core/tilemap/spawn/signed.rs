//! Unbounded layers at negative cells and on chunk edges: every consumer of
//! a spawned map (collision, shadows, class tiles, edits, cell conversions)
//! agrees on which chunk a cell is in and where it is.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use std::time::{Duration, Instant};

use glam::{IVec2, UVec2, Vec2};

use crate::core::asset::Assets;
use crate::core::ecs::{Entity, Transform, World};
use crate::core::lighting::LightOccluder2D;
use crate::core::physics::{
  Collider, ColliderShape, TileCell, TileColliders, TilePart, in_grid_space,
};
use crate::core::tilemap::test_util::{MapWorld, layers_of};
use crate::core::tilemap::{
  MapOccluder, MapRoot, MapTile, MapsView, Tile, TileData, TileFlip, TileGrid, TileLayerData,
  TileMap, TileShape, Tileset, TilesetImage, TilesetSource,
};

/// A cell's side in pixels
const CELL: f32 = 16.0;

/// A plain solid block, and a solid block of the class "crate"
const BLOCK: Tile = Tile {
  tileset: 0,
  id: 0,
  flip: TileFlip::NONE,
};
const CRATE: Tile = Tile {
  tileset: 0,
  id: 1,
  flip: TileFlip::NONE,
};

/// A collection tileset of 16 px tiles: 0 a solid block, 1 a solid block
/// of the class "crate"
fn blocks() -> Tileset {
  let mut set = Tileset::new("blocks", UVec2::splat(16), TilesetImage::Collection);
  set.tile_count = 2;
  let square = || {
    vec![TileShape {
      points: vec![
        Vec2::ZERO,
        Vec2::new(16.0, 0.0),
        Vec2::new(16.0, 16.0),
        Vec2::new(0.0, 16.0),
      ],
      ..TileShape::default()
    }]
  };
  set.insert_tile(
    0,
    TileData {
      shapes: square(),
      ..TileData::default()
    },
  );
  set.insert_tile(
    1,
    TileData {
      class: Some("crate".to_string()),
      shapes: square(),
      ..TileData::default()
    },
  );
  set
}

/// A map built in code whose one layer, "ground", is unbounded and holds
/// `tiles`
fn unbounded_map(tiles: &[(IVec2, Tile)]) -> TileMap {
  let mut map = TileMap::new(UVec2::ZERO, UVec2::splat(16));
  map.add_tileset(TilesetSource::Embedded(Arc::new(blocks())));
  let layer = map.add_tile_layer("ground");
  let Some(grid) = map.tiles_mut(layer) else {
    panic!("ground is a tile layer");
  };
  *grid = TileGrid::unbounded();
  for &(cell, tile) in tiles {
    assert!(grid.set(cell, Some(tile)), "{cell} takes a tile");
  }
  map
}

/// Spawns `map` in `maps` under `root` and lets it land
fn spawn(maps: &mut MapWorld, map: TileMap, root: impl Fn(MapRoot) -> MapRoot) -> Entity {
  let handle = maps.world.resource_mut::<Assets<TileMap>>().add(map);
  maps.spawn(root(MapRoot::new(handle)))
}

fn colliders(world: &World, map: Entity) -> &TileColliders {
  match world.get::<TileColliders>(map) {
    Some(tiles) => tiles,
    None => panic!("{map:?} has no TileColliders"),
  }
}

/// The chunks the map's shadows were built for, each once, in row order
fn occluded_chunks(world: &mut World) -> Vec<IVec2> {
  let mut chunks: Vec<IVec2> = world
    .query::<&MapOccluder>()
    .iter(world)
    .map(|occluder| occluder.chunk)
    .collect();
  chunks.sort_by_key(|c| (c.y, c.x));
  chunks.dedup();
  chunks
}

/// Where the map's collider parts sit (each at its chunk's top-left)
fn part_places(world: &mut World) -> Vec<Vec2> {
  parts(world).into_iter().map(|(at, _)| at).collect()
}

/// The map's collider parts: where each sits, and its collider
fn parts(world: &mut World) -> Vec<(Vec2, Collider)> {
  world
    .query::<(&TilePart, &Transform, &Collider)>()
    .iter(world)
    .map(|(_, at, collider)| (at.translation, collider.clone()))
    .collect()
}

/// A collider's points
fn points(collider: &Collider) -> Vec<Vec2> {
  match &collider.shape {
    ColliderShape::Polyline { points }
    | ColliderShape::Polygon { points }
    | ColliderShape::Segments { points, .. } => points.clone(),
    ColliderShape::Segment { a, b } => vec![*a, *b],
    _ => Vec::new(),
  }
}

/// A chunk's top-left in the layer's pixels
fn chunk_origin(chunk: IVec2) -> Vec2 {
  (chunk * 32).as_vec2() * CELL
}

/// Whole-pixel steps of edges (from, to), each with how many times it is there
type Steps = BTreeMap<((i32, i32), (i32, i32)), usize>;

/// Counts the whole-pixel steps of the axis-aligned edge from `a` to `b`
/// (px, on the pixel grid) in `steps`, so edges compare however chains
/// split and merge them
fn add_steps(steps: &mut Steps, a: Vec2, b: Vec2) {
  let on_grid = |v: Vec2| {
    assert!(
      (v - v.round()).abs().max_element() < 1e-3,
      "{v} off the pixel grid"
    );
    v.round().as_ivec2()
  };
  let (a, b) = (on_grid(a), on_grid(b));
  let delta = b - a;
  assert!(
    delta.x == 0 || delta.y == 0,
    "{a} to {b} is not axis-aligned"
  );
  let unit = delta.signum();
  for i in 0..delta.abs().max_element() {
    let (from, to) = (a + unit * i, a + unit * (i + 1));
    *steps.entry(((from.x, from.y), (to.x, to.y))).or_insert(0) += 1;
  }
}

#[test]
fn negative_and_chunk_edge_cells_agree_in_every_consumer() {
  // Cells -1, 0, -32 and -33 on both axes: -1 and -32 are the last and
  // first cells of chunk -1, 0 the first of chunk 0, -33 the last of
  // chunk -2. The crate at (-33, -33) is a class tile: it leaves the grid
  // and spawns as an entity, so it neither collides nor casts a shadow
  let solid = [IVec2::new(-1, -1), IVec2::ZERO, IVec2::new(-32, -32)];
  let crate_cell = IVec2::new(-33, -33);
  let mut tiles: Vec<(IVec2, Tile)> = solid.iter().map(|&cell| (cell, BLOCK)).collect();
  tiles.push((crate_cell, CRATE));
  let mut maps = MapWorld::new();
  let map = spawn(&mut maps, unbounded_map(&tiles), |root| {
    root.with_entity_tiles(&["crate"])
  });
  maps.frame();

  // The grid and edits read each cell where it was put
  let view = MapsView::new(&mut maps.world);
  for cell in solid {
    assert_eq!(view.tile(map, "ground", cell), Some(BLOCK), "{cell}");
  }
  assert_eq!(view.tile(map, "ground", crate_cell), None);

  // Collision: each solid cell, and nothing across its chunk edges
  let tiles = colliders(&maps.world, map);
  assert_eq!(tiles.bounds(), None, "an unbounded layer's grid");
  for cell in solid {
    assert_eq!(tiles.get(cell), TileCell::Solid, "{cell}");
  }
  for cell in [
    crate_cell,
    IVec2::new(-1, 0),
    IVec2::new(0, -1),
    IVec2::new(-2, -1),
    IVec2::new(-1, -2),
    IVec2::new(1, 0),
    IVec2::new(-33, -32),
    IVec2::new(-32, -33),
    IVec2::new(-31, -32),
  ] {
    assert_eq!(tiles.get(cell), TileCell::Empty, "{cell}");
  }
  // Each part sits at the top-left of a chunk, its points relative to it;
  // put back where they are, they are the solid cells' corners
  let mut corners = BTreeSet::new();
  for (place, collider) in parts(&mut maps.world) {
    assert_eq!(
      place % (32.0 * CELL),
      Vec2::ZERO,
      "a chunk's top-left: {place}"
    );
    for point in points(&in_grid_space(collider, place)) {
      corners.insert((point.x.round() as i32, point.y.round() as i32));
    }
  }
  let expected: BTreeSet<(i32, i32)> = solid
    .iter()
    .flat_map(|cell| {
      [IVec2::ZERO, IVec2::X, IVec2::ONE, IVec2::Y].map(|corner| (*cell + corner) * CELL as i32)
    })
    .map(|p| (p.x, p.y))
    .collect();
  assert_eq!(corners, expected);

  // Shadows: each chunk owns the borders on the left of and above its
  // cells, so the right and bottom borders of the solid cell (-1, -1), the
  // last of chunk (-1, -1), are chunk (0, -1)'s and chunk (-1, 0)'s; every
  // other border of the solid cells is their own chunk's. The crate casts
  // none, so chunk (-2, -2) has no shadow
  assert_eq!(
    occluded_chunks(&mut maps.world),
    [
      IVec2::new(-1, -1),
      IVec2::new(0, -1),
      IVec2::new(-1, 0),
      IVec2::ZERO,
    ]
  );

  // and their geometry: each solid cell's outline, wound as lighting winds
  // polygons, each edge once, in the chunk that owns it (a chunk's chains
  // lie within its cells' box, its right and bottom borders included)
  let chains: Vec<(IVec2, Vec2, LightOccluder2D)> = maps
    .world
    .query::<(&MapOccluder, &Transform, &LightOccluder2D)>()
    .iter(&maps.world)
    .map(|(marker, at, occluder)| (marker.chunk, at.translation, occluder.clone()))
    .collect();
  let mut steps = Steps::new();
  for (chunk, at, occluder) in &chains {
    let (low, high) = (chunk_origin(*chunk), chunk_origin(*chunk + IVec2::ONE));
    for (a, b) in occluder.edges() {
      let (a, b) = (a + *at, b + *at);
      for p in [a, b] {
        assert!(
          p.cmpge(low).all() && p.cmple(high).all(),
          "chunk {chunk}'s chain reaches {p}"
        );
      }
      add_steps(&mut steps, a, b);
    }
  }
  let mut rings = BTreeMap::new();
  for cell in solid {
    let (lo, hi) = (cell.as_vec2() * CELL, (cell + IVec2::ONE).as_vec2() * CELL);
    let corners = [lo, Vec2::new(lo.x, hi.y), hi, Vec2::new(hi.x, lo.y)];
    for i in 0..4 {
      add_steps(&mut rings, corners[i], corners[(i + 1) % 4]);
    }
  }
  assert_eq!(steps, rings, "the solid cells' outlines, each edge once");

  // The class tile sits at its cell's centre
  let found: Vec<(Entity, MapTile)> = maps
    .world
    .query::<(Entity, &MapTile)>()
    .iter(&maps.world)
    .map(|(entity, tile)| (entity, tile.clone()))
    .collect();
  assert_eq!(found.len(), 1);
  let (entity, crate_tile) = &found[0];
  assert_eq!((crate_tile.cell, crate_tile.tile), (crate_cell, CRATE));
  let at = maps.world.get::<Transform>(*entity).map(|t| t.translation);
  assert_eq!(at, Some((crate_cell.as_vec2() + 0.5) * CELL));

  // The cell under each cell's centre is that cell, and back
  let view = MapsView::new(&mut maps.world);
  for cell in solid.into_iter().chain([crate_cell]) {
    let centre = (cell.as_vec2() + 0.5) * CELL;
    assert_eq!(view.world_to_cell(map, "ground", centre), Some(cell));
    assert_eq!(view.cell_to_world(map, "ground", cell), Some(centre));
  }
}

#[test]
fn writing_a_tile_far_away_on_an_unbounded_layer_collides_and_casts_shadows() {
  let mut maps = MapWorld::new();
  let map = spawn(&mut maps, unbounded_map(&[(IVec2::ZERO, BLOCK)]), |r| r);
  let cell = IVec2::new(-500, 300);
  let chunk = IVec2::new(-16, 9);
  assert_eq!(TileGrid::chunk_of(cell), chunk);
  assert!(MapsView::new(&mut maps.world).set_tile(map, "ground", cell, Some(BLOCK)));
  assert_eq!(
    MapsView::new(&mut maps.world).tile(map, "ground", cell),
    Some(BLOCK)
  );
  maps.frame();
  maps.frame();
  assert_eq!(colliders(&maps.world, map).get(cell), TileCell::Solid);
  let places = part_places(&mut maps.world);
  assert!(
    places.contains(&chunk_origin(chunk)),
    "a part at chunk {chunk}'s top-left: {places:?}"
  );
  assert!(
    occluded_chunks(&mut maps.world).contains(&chunk),
    "a shadow for chunk {chunk}"
  );
  // Filling a row there makes the chunks it writes, each colliding
  let changed = MapsView::new(&mut maps.world).fill(
    map,
    "ground",
    IVec2::new(-1000, -40),
    IVec2::new(-961, -40),
    Some(BLOCK),
  );
  assert_eq!(changed, 40);
  maps.frame();
  let tiles = colliders(&maps.world, map);
  for x in [-1000, -993, -992, -961] {
    assert_eq!(tiles.get(IVec2::new(x, -40)), TileCell::Solid, "{x}");
  }
}

#[test]
fn a_fill_making_more_chunks_than_a_map_holds_is_refused_and_warns_once() {
  use crate::core::tilemap::grid::{FILL_CAP, MAX_FILL_CHUNKS};
  // The cap lowered to 4 chunks for the test (131,072 in a game), so were
  // the guard gone the refused fills would make a few chunks, not gigabytes
  FILL_CAP.with(|cap| cap.set(4));
  let mut maps = MapWorld::new();
  let mut map = infinite_map();
  map.layers[0].info.name = "huge-fill-5d2a".to_string();
  let map = spawn(&mut maps, map, |r| r.with_entity_tiles(&["crate"]));
  let layer = "huge-fill-5d2a";
  let mut view = MapsView::new(&mut maps.world);
  // A row of 5 chunks, a plain tile and a class tile: refused before
  // anything is made
  let five = IVec2::new(5 * 32 - 1, 0);
  assert_eq!(view.fill(map, layer, IVec2::ZERO, five, Some(BLOCK)), 0);
  assert_eq!(view.fill(map, layer, IVec2::ZERO, five, Some(CRATE)), 0);
  let Some(ground) = view.layer_entity(map, layer) else {
    panic!("no layer");
  };
  let chunks = maps
    .world
    .get::<TileLayerData>(ground)
    .map_or(usize::MAX, |data| data.grid.chunk_coords().count());
  assert_eq!(chunks, 0, "nothing was made");
  assert_eq!(maps.world.query::<&MapTile>().iter(&maps.world).count(), 0);
  let warned = crate::core::dev_log::get_entries()
    .iter()
    .filter(|e| e.message.contains(layer) && e.message.contains("fill"))
    .count();
  assert_eq!(warned, 1, "warned once, naming the layer");
  // A fill within the cap writes; clearing every cell reads the chunks
  // there are
  let mut view = MapsView::new(&mut maps.world);
  assert_eq!(
    view.fill(
      map,
      layer,
      IVec2::new(-40, 0),
      IVec2::new(40, 0),
      Some(BLOCK)
    ),
    81
  );
  assert_eq!(view.fill(map, layer, IVec2::MIN, IVec2::MAX, None), 81);
  FILL_CAP.with(|cap| cap.set(MAX_FILL_CHUNKS));
}

#[test]
fn an_unbounded_layer_with_a_tile_a_million_cells_out_lands_holding_only_its_chunks() {
  // The rectangle the two tiles span holds about 976 million chunks:
  // nothing may be sized from it
  let far = IVec2::splat(1_000_000);
  let started = Instant::now();
  let mut maps = MapWorld::new();
  let map = spawn(
    &mut maps,
    unbounded_map(&[(IVec2::ZERO, BLOCK), (far, BLOCK)]),
    |r| r,
  );
  maps.frame();
  assert!(
    started.elapsed() < Duration::from_secs(5),
    "landed in {:?}",
    started.elapsed()
  );
  let layer = layers_of(&mut maps.world, map)[0];
  let chunks: Vec<IVec2> = maps
    .world
    .get::<TileLayerData>(layer)
    .map(|data| data.grid.chunk_coords().collect())
    .unwrap_or_default();
  let far_chunk = TileGrid::chunk_of(far);
  assert_eq!(chunks, [IVec2::ZERO, far_chunk]);
  // Both cast shadows; the far one is past the collision lattice's reach,
  // so it draws and casts a shadow but does not collide
  assert_eq!(occluded_chunks(&mut maps.world), [IVec2::ZERO, far_chunk]);
  let tiles = colliders(&maps.world, map);
  assert_eq!(tiles.get(IVec2::ZERO), TileCell::Solid);
  assert_eq!(tiles.get(far), TileCell::Empty);
  assert_eq!(
    tiles.extent(),
    crate::core::physics::CellRect::new(IVec2::ZERO, IVec2::splat(32)),
    "the colliders hold chunk (0, 0) alone"
  );
  // An edit there is followed in its chunk alone
  assert!(MapsView::new(&mut maps.world).set_tile(map, "ground", far + IVec2::X, Some(BLOCK)));
  maps.frame();
  assert_eq!(occluded_chunks(&mut maps.world), [IVec2::ZERO, far_chunk]);
}

/// A map built infinite in code, over the solid blocks, with one tile layer
/// "ground"
fn infinite_map() -> TileMap {
  let mut map = TileMap::infinite(UVec2::splat(16));
  map.add_tileset(TilesetSource::Embedded(Arc::new(blocks())));
  map.add_tile_layer("ground");
  map
}

#[test]
fn a_map_built_infinite_in_code_grows_where_a_game_writes() {
  use crate::core::physics::RigidBody;

  let mut maps = MapWorld::new();
  let map = spawn(&mut maps, infinite_map(), |r| r);
  assert_eq!(
    MapsView::new(&mut maps.world).fill(
      map,
      "ground",
      IVec2::new(-100, 5),
      IVec2::new(-90, 5),
      Some(BLOCK)
    ),
    11
  );
  assert!(MapsView::new(&mut maps.world).set_tile(
    map,
    "ground",
    IVec2::new(5000, -5000),
    Some(BLOCK)
  ));
  // A body over the floor's cell (-95, 4), well above it
  let body = maps
    .world
    .spawn((
      Transform::from_xy(-95.0 * CELL + CELL / 2.0, 40.0),
      RigidBody::dynamic(),
      Collider::rect(8.0, 8.0),
    ))
    .id();
  let y_of = |world: &World| world.get::<Transform>(body).map(|t| t.translation.y);
  let mut last = f32::MAX;
  let mut still = 0;
  for _ in 0..120 {
    maps.frame();
    let y = y_of(&maps.world).unwrap_or(f32::NAN);
    still = if (y - last).abs() < 1e-3 {
      still + 1
    } else {
      0
    };
    last = y;
    if still >= 5 {
      break;
    }
  }
  // The floor's top is at y 80: the 8 px body rests with its centre 4 above
  let y = y_of(&maps.world).unwrap_or(f32::NAN);
  assert!(
    (y - 76.0).abs() < 1.0,
    "the body is at y {y}, not on the floor"
  );
  // The layer holds the chunks of both writes: from chunk (-4, -157) to
  // chunk (156, 0)
  let layer = layers_of(&mut maps.world, map)[0];
  let extent = maps
    .world
    .get::<TileLayerData>(layer)
    .map(|data| data.grid.extent());
  assert_eq!(
    extent,
    Some(crate::core::physics::CellRect::new(
      IVec2::new(-128, -5024),
      IVec2::new(5024, 32)
    ))
  );
}

#[test]
fn maps_view_reads_a_grown_map_s_bounds_and_each_layer_s() {
  use crate::core::physics::CellRect;
  let mut maps = MapWorld::new();
  let mut map = infinite_map();
  map.add_tile_layer("decor");
  map.add_object_layer("things");
  let map = spawn(&mut maps, map, |r| r.with_entity_tiles(&["crate"]));
  let view = MapsView::new(&mut maps.world);
  let empty = Some(CellRect::default());
  assert_eq!(view.bounds(map), empty, "no tile yet");
  assert_eq!(view.layer_bounds(map, "ground"), empty);
  // Writes grow it, negative cells and class tiles included; the asset's
  // bounds stay what it held when it was added
  let mut view = MapsView::new(&mut maps.world);
  assert!(view.set_tile(map, "ground", IVec2::new(-40, 3), Some(BLOCK)));
  assert!(view.set_tile(map, "ground", IVec2::new(7, -9), Some(BLOCK)));
  assert!(view.set_tile(map, "decor", IVec2::new(100, 20), Some(CRATE)));
  assert_eq!(
    view.layer_bounds(map, "ground"),
    Some(CellRect::new(IVec2::new(-40, -9), IVec2::new(8, 4)))
  );
  assert_eq!(
    view.layer_bounds(map, 1),
    Some(CellRect::new(IVec2::new(100, 20), IVec2::new(101, 21))),
    "the class tile's cell"
  );
  assert_eq!(
    view.bounds(map),
    Some(CellRect::new(IVec2::new(-40, -9), IVec2::new(101, 21)))
  );
  // Clearing a tile shrinks it
  assert!(view.set_tile(map, "ground", IVec2::new(-40, 3), None));
  assert_eq!(
    view.layer_bounds(map, "ground"),
    Some(CellRect::new(IVec2::new(7, -9), IVec2::new(8, -8)))
  );
  // A layer that is not a tile layer, or one the map lacks, has none
  assert_eq!(view.layer_bounds(map, "things"), None);
  assert_eq!(view.layer_bounds(map, "nope"), None);

  // A bounded map's are its size, its layers' their bounds
  let mut bounded = TileMap::new(UVec2::new(40, 8), UVec2::splat(16));
  bounded.add_tileset(TilesetSource::Embedded(Arc::new(blocks())));
  bounded.add_tile_layer("ground");
  let bounded = spawn(&mut maps, bounded, |r| r);
  let whole = Some(CellRect::from_size(UVec2::new(40, 8)));
  let view = MapsView::new(&mut maps.world);
  assert_eq!(view.bounds(bounded), whole);
  assert_eq!(view.layer_bounds(bounded, "ground"), whole);
  // A root that has not landed has none
  let waiting = maps.world.spawn_empty().id();
  assert_eq!(MapsView::new(&mut maps.world).bounds(waiting), None);
}

/// Chunk A, (-1, 0), and chunk B, (0, 0), of `two_chunks`
const A: IVec2 = IVec2::new(-1, 0);
const B: IVec2 = IVec2::ZERO;
/// The crate in chunk A: column 7, row 12 of it
const CRATE_CELL: IVec2 = IVec2::new(-25, 12);

/// A solid block of 4 x 4 cells from `corner`
fn block(corner: IVec2) -> Vec<IVec2> {
  (0..4)
    .flat_map(|y| (0..4).map(move |x| corner + IVec2::new(x, y)))
    .collect()
}

/// The solid blocks in chunks A and B: away from their shared border and
/// every other, so neither's colliders or shadows read the other's cells
fn blocks_of_a_and_b() -> (Vec<IVec2>, Vec<IVec2>) {
  (block(IVec2::new(-20, 5)), block(IVec2::new(10, 5)))
}

/// An unbounded map of a solid block in chunk A and one in chunk B, with a
/// crate (a class tile) in chunk A, landed
fn two_chunks(maps: &mut MapWorld) -> Entity {
  let (in_a, in_b) = blocks_of_a_and_b();
  let mut tiles: Vec<(IVec2, Tile)> = in_a.iter().chain(&in_b).map(|&c| (c, BLOCK)).collect();
  tiles.push((CRATE_CELL, CRATE));
  let map = spawn(maps, unbounded_map(&tiles), |root| {
    root.with_entity_tiles(&["crate"])
  });
  maps.frame();
  map
}

/// The map's collider parts by the chunk each sits at, with their entities
fn parts_by_chunk(world: &mut World) -> Vec<(IVec2, Entity)> {
  let mut found: Vec<(IVec2, Entity)> = world
    .query::<(Entity, &TilePart, &Transform)>()
    .iter(world)
    .map(|(entity, _, at)| {
      let chunk = (at.translation / (32.0 * CELL)).round().as_ivec2();
      (chunk, entity)
    })
    .collect();
  found.sort_by_key(|(chunk, entity)| (chunk.y, chunk.x, *entity));
  found
}

/// The map's occluders of `chunk`
fn occluders_of(world: &mut World, chunk: IVec2) -> Vec<Entity> {
  let mut found: Vec<Entity> = world
    .query::<(Entity, &MapOccluder)>()
    .iter(world)
    .filter(|(_, occluder)| occluder.chunk == chunk)
    .map(|(entity, _)| entity)
    .collect();
  found.sort();
  found
}

/// The map's class tiles: each one's entity and cell
fn class_tiles(world: &mut World) -> Vec<(Entity, IVec2)> {
  world
    .query::<(Entity, &MapTile)>()
    .iter(world)
    .map(|(entity, tile)| (entity, tile.cell))
    .collect()
}

/// The layer's grid, to unload and load chunks of
fn grid_of(world: &mut World, map: Entity) -> crate::core::ecs::Mut<'_, TileLayerData> {
  let layer = layers_of(world, map)[0];
  match world.get_mut::<TileLayerData>(layer) {
    Some(data) => data,
    None => panic!("{layer:?} is not a tile layer"),
  }
}

#[test]
fn an_unloaded_chunk_leaves_collision_shadows_and_class_tiles() {
  let mut maps = MapWorld::new();
  let map = two_chunks(&mut maps);
  let (in_a, in_b) = blocks_of_a_and_b();
  let parts = parts_by_chunk(&mut maps.world);
  assert_eq!(parts.iter().map(|p| p.0).collect::<Vec<_>>(), [A, B]);
  let b_occluders = occluders_of(&mut maps.world, B);
  assert!(!occluders_of(&mut maps.world, A).is_empty());
  assert!(!b_occluders.is_empty());
  let tiles = class_tiles(&mut maps.world);
  assert_eq!(tiles.len(), 1);
  let (crate_tile, _) = tiles[0];

  // A leaves the layer's grid, as a stream unloads it
  let unloaded = grid_of(&mut maps.world, map).grid.unload_chunk(A);
  assert!(unloaded.is_some());
  maps.frame();

  // Collision: A's cells are empty and its storage gone; B's part is the
  // one it was
  let tiles = colliders(&maps.world, map);
  for &cell in &in_a {
    assert_eq!(tiles.get(cell), TileCell::Empty, "{cell}");
  }
  for &cell in &in_b {
    assert_eq!(tiles.get(cell), TileCell::Solid, "{cell}");
  }
  assert!(
    tiles.chunk_cells(A).is_none(),
    "A's colliders' storage goes"
  );
  assert_eq!(parts_by_chunk(&mut maps.world), [parts[1]]);
  // Shadows: none for A; B's are the ones it had
  assert!(occluders_of(&mut maps.world, A).is_empty());
  assert_eq!(occluders_of(&mut maps.world, B), b_occluders);
  // The crate goes with its chunk
  assert!(maps.world.get_entity(crate_tile).is_err(), "despawned");
  assert!(class_tiles(&mut maps.world).is_empty());
  let view = MapsView::new(&mut maps.world);
  assert_eq!(view.tile(map, "ground", in_a[0]), None);
  assert_eq!(view.tile(map, "ground", in_b[0]), Some(BLOCK));
}

#[test]
fn a_floor_across_an_unloaded_chunk_closes_at_its_edge_in_collision_and_shadows() {
  // A floor on row 5 from cell -32 to 63, through chunks -1, 0 and 1:
  // unloading chunk 1 leaves a floor from -32 to 31, one closed outline
  // ending at x = 32 cells
  let floor: Vec<(IVec2, Tile)> = (-32..64).map(|x| (IVec2::new(x, 5), BLOCK)).collect();
  let mut maps = MapWorld::new();
  let map = spawn(&mut maps, unbounded_map(&floor), |r| r);
  maps.frame();
  let unloaded = grid_of(&mut maps.world, map)
    .grid
    .unload_chunk(IVec2::new(1, 0));
  assert!(unloaded.is_some());
  maps.frame();

  let (lo, hi) = (Vec2::new(-32.0, 5.0) * CELL, Vec2::new(32.0, 6.0) * CELL);
  let corners = [lo, Vec2::new(lo.x, hi.y), hi, Vec2::new(hi.x, lo.y)];
  // Collision: one part, a closed loop round the floor left
  let found = parts(&mut maps.world);
  assert_eq!(found.len(), 1, "no part left in chunk 1");
  let (place, collider) = found[0].clone();
  let ColliderShape::Polyline { points } = &in_grid_space(collider, place).shape else {
    panic!("one loop is a closed polyline");
  };
  assert_eq!(points.first(), points.last(), "closed");
  let mut seen: Vec<(i32, i32)> = points
    .iter()
    .map(|p| (p.x.round() as i32, p.y.round() as i32))
    .collect();
  seen.sort_unstable();
  seen.dedup();
  let mut expected: Vec<(i32, i32)> = corners.iter().map(|p| (p.x as i32, p.y as i32)).collect();
  expected.sort_unstable();
  assert_eq!(seen, expected);
  // Shadows: the same outline, each edge once; chunk 1 owns its right
  // border, the left border of its own first column
  let mut steps = Steps::new();
  let chains: Vec<(Vec2, LightOccluder2D)> = maps
    .world
    .query::<(&MapOccluder, &Transform, &LightOccluder2D)>()
    .iter(&maps.world)
    .map(|(_, at, occluder)| (at.translation, occluder.clone()))
    .collect();
  for (at, occluder) in &chains {
    for (a, b) in occluder.edges() {
      add_steps(&mut steps, a + *at, b + *at);
    }
  }
  let mut ring = Steps::new();
  for i in 0..4 {
    add_steps(&mut ring, corners[i], corners[(i + 1) % 4]);
  }
  assert_eq!(steps, ring, "the floor's outline, closed at x = 32 cells");
  assert_eq!(
    occluded_chunks(&mut maps.world),
    [IVec2::new(-1, 0), IVec2::ZERO, IVec2::new(1, 0)]
  );
}

#[test]
fn loading_a_chunk_back_rebuilds_it_as_new() {
  let mut maps = MapWorld::new();
  let map = two_chunks(&mut maps);
  let (in_a, in_b) = blocks_of_a_and_b();
  let b_part = parts_by_chunk(&mut maps.world)[1];
  let b_occluders = occluders_of(&mut maps.world, B);
  let (old_crate, _) = class_tiles(&mut maps.world)[0];
  let b_untouched = |world: &mut World, when: &str| {
    assert!(parts_by_chunk(world).contains(&b_part), "{when}: B's part");
    assert_eq!(occluders_of(world, B), b_occluders, "{when}: B's shadows");
  };

  let Some(mut cells) = grid_of(&mut maps.world, map).grid.unload_chunk(A) else {
    panic!("chunk A is there");
  };
  maps.frame();
  b_untouched(&mut maps.world, "unloaded");
  // A comes back with its tiles as the map had them, the crate included
  let local = CRATE_CELL - A * 32;
  cells[(local.y * 32 + local.x) as usize] = TileGrid::encode(Some(CRATE)).expect("fits");
  assert!(grid_of(&mut maps.world, map).grid.load_chunk(A, cells));
  maps.frame();

  let tiles = colliders(&maps.world, map);
  for &cell in in_a.iter().chain(&in_b) {
    assert_eq!(tiles.get(cell), TileCell::Solid, "{cell}");
  }
  assert_eq!(
    tiles.get(CRATE_CELL),
    TileCell::Empty,
    "a class tile collides not"
  );
  let parts = parts_by_chunk(&mut maps.world);
  assert_eq!(parts.iter().map(|p| p.0).collect::<Vec<_>>(), [A, B]);
  assert!(
    !occluders_of(&mut maps.world, A).is_empty(),
    "A casts again"
  );
  b_untouched(&mut maps.world, "loaded back");
  // The crate is a new entity, and out of the grid again
  let tiles = class_tiles(&mut maps.world);
  assert_eq!(tiles.len(), 1);
  let (new_crate, cell) = tiles[0];
  assert_eq!(cell, CRATE_CELL);
  assert_ne!(new_crate, old_crate);
  let view = MapsView::new(&mut maps.world);
  assert_eq!(view.tile(map, "ground", CRATE_CELL), None);
  assert_eq!(view.tile(map, "ground", in_a[0]), Some(BLOCK));
  let at = maps
    .world
    .get::<Transform>(new_crate)
    .map(|t| t.translation);
  assert_eq!(at, Some((CRATE_CELL.as_vec2() + 0.5) * CELL));
  // An edit to the crate's cell finds the new entity, as a landed one's
  assert!(MapsView::new(&mut maps.world).set_tile(map, "ground", CRATE_CELL, None));
  assert!(maps.world.get_entity(new_crate).is_err());
}
