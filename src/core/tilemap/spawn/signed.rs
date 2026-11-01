//! Unbounded layers at negative cells and on chunk edges: every consumer of
//! a spawned map (collision, shadows, class tiles, edits, cell conversions)
//! agrees on which chunk a cell is in and where it is.

use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::{Duration, Instant};

use glam::{IVec2, UVec2, Vec2};

use crate::core::asset::Assets;
use crate::core::ecs::{Entity, Transform, World};
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
