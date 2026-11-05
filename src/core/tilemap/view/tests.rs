//! A layer's grid in the world (`MapsView::grid`): picking and edits on the
//! cell under a point, outlines in the world and the board tools, on every
//! grid, on a map moved, turned and scaled; and a navigation grid over a
//! window of a layer's cells (`MapsView::navigation`), its map's collision
//! and a stream's unloaded chunks blocked.

use std::sync::Arc;

use glam::{IVec2, UVec2, Vec2};

use crate::core::asset::Assets;
use crate::core::ecs::kernel::{MessageCursor, Messages};
use crate::core::ecs::{Entity, Transform, World};
use crate::core::navigation::{NavigationGrid, PathResult, Pathfinder};
use crate::core::physics::{CellRect, TileCell, TileColliders};
use crate::core::tilemap::test_util::{MapWorld, whole_hex_set};
use crate::core::tilemap::{
  ChunkEvent, ChunkRequest, ChunkTiles, GridShape, LayerGrid, MapRoot, MapStream, MapsView,
  StaggerAxis, StaggerIndex, StreamAnchor, Tile, TileData, TileMap, TileShape, Tileset,
  TilesetImage, TilesetSource,
};

use StaggerAxis::{X, Y};
use StaggerIndex::{Even, Odd};

/// Every grid the tests walk, with its cell size: each shape and stagger,
/// odd sizes, a hex side of 0 and negative skews
fn shapes() -> Vec<(GridShape, UVec2)> {
  let mut out = vec![
    (GridShape::Orthogonal, UVec2::new(16, 16)),
    (GridShape::Isometric, UVec2::new(64, 32)),
    (GridShape::Isometric, UVec2::new(33, 17)),
    (
      GridShape::Oblique {
        skew: IVec2::new(8, 0),
      },
      UVec2::new(32, 32),
    ),
    (
      GridShape::Oblique {
        skew: IVec2::new(-6, 4),
      },
      UVec2::new(32, 32),
    ),
  ];
  for axis in [X, Y] {
    for index in [Odd, Even] {
      out.push((GridShape::Staggered { axis, index }, UVec2::new(64, 32)));
      out.push((
        GridShape::Hexagonal {
          axis,
          index,
          side: 12,
        },
        UVec2::new(32, 28),
      ));
    }
  }
  out.push((
    GridShape::Hexagonal {
      axis: Y,
      index: Odd,
      side: 0,
    },
    UVec2::new(32, 32),
  ));
  out
}

/// A tileset of no image with room for every tile the tests write
fn tileset() -> TilesetSource {
  let mut set = Tileset::new("plain", UVec2::splat(16), TilesetImage::Collection);
  set.tile_count = 4096;
  TilesetSource::Embedded(Arc::new(set))
}

/// Where the tests put a map: moved, turned and scaled, so every picking
/// goes through the transform
fn placed() -> Transform {
  Transform::from_xy(123.0, -45.0)
    .with_rotation(0.3)
    .with_scale(Vec2::new(1.5, 1.25))
}

/// A map of `shape` with one tile layer, "ground", shifted by an offset:
/// bounded to `size` cells, or infinite with `None`
fn map_of(shape: GridShape, cell: UVec2, size: Option<UVec2>) -> TileMap {
  let mut map = match size {
    Some(size) => TileMap::new(size, cell),
    None => TileMap::infinite(cell),
  }
  .with_shape(shape);
  map.add_tileset(tileset());
  let ground = map.add_tile_layer("ground");
  map.layers[ground].info.offset = Vec2::new(5.0, -3.0);
  map
}

/// Spawns `map` at `transform` and lets it land
fn spawn(maps: &mut MapWorld, map: TileMap, transform: Transform) -> Entity {
  let handle = maps.world.resource_mut::<Assets<TileMap>>().add(map);
  let root = maps.world.spawn((MapRoot::new(handle), transform)).id();
  maps.settle(root);
  root
}

/// The cells of `min..=max`, by row
fn cells(min: IVec2, max: IVec2) -> Vec<IVec2> {
  (min.y..=max.y)
    .flat_map(|y| (min.x..=max.x).map(move |x| IVec2::new(x, y)))
    .collect()
}

/// The middle of an outline's points: the cell's centre, every outline
/// being symmetric about it (a repeated point has its twin opposite)
fn middle(points: &[Vec2]) -> Vec2 {
  points.iter().copied().sum::<Vec2>() / points.len() as f32
}

/// Twice an outline's signed area, positive where it runs clockwise on
/// screen (y down)
fn signed_area(points: &[Vec2]) -> f32 {
  let n = points.len();
  (0..n)
    .map(|i| {
      let (a, b) = (points[i], points[(i + 1) % n]);
      a.x * b.y - b.x * a.y
    })
    .sum()
}

/// The tile standing for the walk's `i`th cell, one no other cell has
fn tile_for(i: usize) -> Tile {
  Tile::new(0, i as u32 + 1)
}

/// The layer's grid of `map`'s "ground"
fn ground(maps: &mut MapWorld, map: Entity) -> LayerGrid {
  match MapsView::new(&mut maps.world).grid(map, "ground") {
    Some(grid) => grid,
    None => panic!("{map} has a ground layer"),
  }
}

/// Picks each of `walk`'s cells by the middle of its outline in the world
/// and puts its own tile there, then checks every cell holds its own tile
/// and no other cell took one
fn pick_and_edit(maps: &mut MapWorld, map: Entity, walk: &[IVec2], label: &str) {
  let layer = ground(maps, map);
  for (i, &cell) in walk.iter().enumerate() {
    let outline = layer.outline_world(cell);
    let points = outline.points();
    // In the world: the layer's outline taken there, still clockwise on
    // screen from its top-most point
    let local = layer.grid.cell_outline(cell);
    assert_eq!(points.len(), local.points().len(), "{label} {cell}");
    for point in local.points() {
      let at = layer.to_world.transform_point2(*point);
      assert!(points.contains(&at), "{label} {cell}: {at} in {points:?}");
    }
    assert!(signed_area(points) > 0.0, "{label} {cell}: {points:?}");
    let top = points
      .iter()
      .fold(f32::INFINITY, |top, point| top.min(point.y));
    assert_eq!(points[0].y, top, "{label} {cell}: {points:?}");

    let at = middle(points);
    let centre = layer.cell_center_world(cell);
    assert!(at.distance(centre) < 0.01, "{label} {cell}: {at} {centre}");
    let mut view = MapsView::new(&mut maps.world);
    assert_eq!(view.cell_to_world(map, "ground", cell), Some(centre));
    assert_eq!(layer.cell_at_world(at), cell, "{label}");
    assert_eq!(layer.cell_at_world(centre), cell, "{label}");
    let picked = view.world_to_cell(map, "ground", at);
    assert_eq!(
      picked,
      Some(cell),
      "{label}: the middle of {cell}'s outline"
    );
    assert!(
      view.set_tile(map, "ground", cell, Some(tile_for(i))),
      "{label} {cell}"
    );
  }
  let view = MapsView::new(&mut maps.world);
  for (i, &cell) in walk.iter().enumerate() {
    assert_eq!(
      view.tile(map, "ground", cell),
      Some(tile_for(i)),
      "{label} {cell}: its own tile and no other's"
    );
  }
  let data = maps
    .world
    .query::<&crate::core::tilemap::TileLayerData>()
    .iter(&maps.world)
    .next()
    .map(|data| {
      let extent = data.tiles.extent();
      cells(extent.min, extent.max - IVec2::ONE)
        .into_iter()
        .filter(|&cell| data.tiles.get(cell).is_some())
        .count()
    });
  assert_eq!(
    data,
    Some(walk.len()),
    "{label}: only the cells walked hold a tile"
  );
}

#[test]
fn picking_and_edits_land_on_the_cell_under_the_point_on_every_grid() {
  for (shape, cell) in shapes() {
    // A bounded map: on isometric grids its origin is off zero
    let mut maps = MapWorld::new();
    let map = spawn(
      &mut maps,
      map_of(shape, cell, Some(UVec2::new(6, 5))),
      placed(),
    );
    let walk = cells(IVec2::ZERO, IVec2::new(5, 4));
    pick_and_edit(&mut maps, map, &walk, &format!("bounded {shape:?} {cell}"));

    // An infinite map: negative cells, and cells across chunk borders
    let mut maps = MapWorld::new();
    let map = spawn(&mut maps, map_of(shape, cell, None), placed());
    let mut walk = cells(IVec2::splat(-4), IVec2::splat(3));
    walk.extend([
      IVec2::new(-33, 31),
      IVec2::new(32, -32),
      IVec2::new(-32, -1),
    ]);
    pick_and_edit(&mut maps, map, &walk, &format!("infinite {shape:?} {cell}"));
  }
}

#[test]
fn a_layer_grid_is_none_for_a_layer_that_is_not_a_tile_layer() {
  let mut maps = MapWorld::new();
  let mut map = map_of(GridShape::Isometric, UVec2::new(64, 32), None);
  map.add_object_layer("things");
  let map = spawn(&mut maps, map, Transform::IDENTITY);
  let view = MapsView::new(&mut maps.world);
  assert!(view.grid(map, "ground").is_some());
  assert!(view.grid(map, "things").is_none());
  assert!(view.grid(map, "nowhere").is_none());
  assert!(view.grid(Entity::PLACEHOLDER, "ground").is_none());
}

#[test]
fn hex_board_tools_work_through_the_layer() {
  let shape = GridShape::Hexagonal {
    axis: Y,
    index: Odd,
    side: 12,
  };
  let cell = UVec2::new(32, 28);
  let mut maps = MapWorld::new();
  let asset = map_of(shape, cell, Some(UVec2::new(10, 10)));
  let map_grid = asset.grid();
  let map = spawn(&mut maps, asset, placed());
  let layer = ground(&mut maps, map);
  // The layer's grid is the map's, placed where the layer is in the world
  assert_eq!(layer.grid, map_grid);

  // The player clicks a hex: the one under the pointer
  let home = IVec2::new(4, 5);
  let click = layer.cell_center_world(home) + Vec2::new(3.0, -2.0);
  assert_eq!(layer.cell_at_world(click), home);
  let view = MapsView::new(&mut maps.world);
  assert_eq!(view.world_to_cell(map, "ground", click), Some(home));

  // Its six neighbours each share an edge with it in the world
  let around: Vec<IVec2> = layer.grid.neighbours(home).collect();
  assert_eq!(around.len(), 6);
  let outline = layer.outline_world(home);
  for &next in &around {
    let theirs = layer.outline_world(next);
    let shared = theirs
      .points()
      .iter()
      .filter(|point| outline.points().contains(point))
      .count();
    assert_eq!(shared, 2, "{next} shares an edge with {home}");
    assert_eq!(layer.grid.distance(home, next), 1);
  }

  // A move range of 2, its ring, and a line to a hex clicked far off,
  // each step a neighbour, as Red Blob's maths gives them
  let range = layer.grid.range(home, 2);
  assert_eq!(range.len(), 19);
  assert!(range.iter().all(|&c| layer.grid.distance(home, c) <= 2));
  let ring = layer.grid.ring(home, 2);
  assert_eq!(ring.len(), 12);
  assert!(ring.iter().all(|&c| layer.grid.distance(home, c) == 2));
  let far = layer.cell_at_world(layer.cell_center_world(IVec2::new(9, 0)));
  let line = layer.grid.line(home, far);
  assert_eq!(line.len() as u32, layer.grid.distance(home, far) + 1);
  assert_eq!((line[0], line[line.len() - 1]), (home, far));
  for step in line.windows(2) {
    assert!(layer.grid.neighbours(step[0]).any(|n| n == step[1]));
  }

  // Marking the range on the board marks those hexes and no other
  let mut view = MapsView::new(&mut maps.world);
  for &c in &range {
    assert!(view.set_tile(map, "ground", c, Some(Tile::new(0, 1))));
  }
  let marked: Vec<IVec2> = cells(IVec2::ZERO, IVec2::splat(9))
    .into_iter()
    .filter(|&c| view.tile(map, "ground", c).is_some())
    .collect();
  let mut expect = range.clone();
  expect.sort_by_key(|c| (c.y, c.x));
  assert_eq!(marked, expect);
}

/// The blocked cells of `grid`'s window, by row
fn blocked(grid: &NavigationGrid) -> Vec<IVec2> {
  let window = grid.window();
  cells(window.min, window.max - IVec2::ONE)
    .into_iter()
    .filter(|&cell| !grid.is_walkable(cell.into()))
    .collect()
}

/// `cells` by row, then column
fn by_row(mut cells: Vec<IVec2>) -> Vec<IVec2> {
  cells.sort_by_key(|cell| (cell.y, cell.x));
  cells
}

/// `map`'s navigation grid over `window` of `layer`
fn navigation(maps: &mut MapWorld, map: Entity, layer: &str, window: CellRect) -> NavigationGrid {
  match MapsView::new(&mut maps.world).navigation(map, layer, window) {
    Some(grid) => grid,
    None => panic!("{map} has landed with a tile layer {layer}"),
  }
}

/// The cells of the path `grid` finds from `from` to `to`, if it finds one
fn path_cells(grid: &NavigationGrid, from: IVec2, to: IVec2) -> Option<Vec<IVec2>> {
  match Pathfinder::new().find_path_grid(grid, from.into(), to.into()) {
    PathResult::Found(path) => Some(
      path
        .waypoints()
        .iter()
        .map(|node| node.grid_coord.into())
        .collect(),
    ),
    _ => None,
  }
}

#[test]
fn a_map_s_navigation_blocks_its_colliding_layers() {
  let cell = UVec2::new(32, 28);
  let mut map = TileMap::new(UVec2::new(10, 8), cell).with_shape(GridShape::Hexagonal {
    axis: Y,
    index: Odd,
    side: 12,
  });
  // Tile 0 is its whole hexagon, tile 1 a floor of no shape
  let mut set = whole_hex_set(&map.grid());
  set.insert_tile(1, TileData::default());
  set.tile_count = 2;
  let set = map.add_tileset(TilesetSource::Embedded(Arc::new(set)));
  let floor = map.add_tile_layer("floor");
  map.layers[floor].info.collides = false;
  let walls = map.add_tile_layer("walls");
  // One column across, so it collides as a grid of its own on its layer
  let rocks = map.add_tile_layer("rocks");
  map.layers[rocks].info.offset = Vec2::new(cell.x as f32, 0.0);
  map.add_object_layer("things");
  let solid = Some(Tile::new(set, 0));
  if let Some(grid) = map.tiles_mut(floor) {
    grid.fill(IVec2::ZERO, IVec2::new(9, 7), Some(Tile::new(set, 1)));
  }
  // Column 4 from row 1 to row 6, each hex sharing a side with the next
  let wall: Vec<IVec2> = (1..=6).map(|y| IVec2::new(4, y)).collect();
  if let Some(grid) = map.tiles_mut(walls) {
    for &at in &wall {
      grid.set(at, solid);
    }
  }
  if let Some(grid) = map.tiles_mut(rocks) {
    grid.set(IVec2::new(1, 2), solid);
  }
  let mut maps = MapWorld::new();
  let root = spawn(&mut maps, map, Transform::IDENTITY);

  let view = MapsView::new(&mut maps.world);
  let (Some(bounds), Some(floor), Some(rocks), Some(rocks_layer)) = (
    view.bounds(root),
    view.grid(root, "floor"),
    view.grid(root, "rocks"),
    view.layer_entity(root, "rocks"),
  ) else {
    panic!("the map landed with its layers")
  };
  // The walls collide on the map entity, the rock on its own layer, where
  // its hex lies over the floor's (2, 2)
  assert!(maps.world.get::<TileColliders>(root).is_some());
  assert!(maps.world.get::<TileColliders>(rocks_layer).is_some());
  let rock = floor.cell_at_world(rocks.cell_center_world(IVec2::new(1, 2)));
  assert_eq!(rock, IVec2::new(2, 2));

  let grid = navigation(&mut maps, root, "floor", bounds);
  assert_eq!(grid.window(), bounds);
  assert_eq!(grid.layer(), Some(floor));
  let expected = by_row([wall, vec![rock]].concat());
  assert_eq!(blocked(&grid), expected);

  // A path across the wall goes round one of its ends, a hex at a time
  let Some(path) = path_cells(&grid, IVec2::new(1, 4), IVec2::new(7, 4)) else {
    panic!("a way round the wall")
  };
  assert!(path.iter().all(|&at| grid.is_walkable(at.into())));
  for step in path.windows(2) {
    assert!(
      floor.grid.neighbours(step[0]).any(|next| next == step[1]),
      "{path:?}"
    );
  }
  assert!(path.iter().any(|at| at.y == 0 || at.y == 7), "{path:?}");

  // The map moved and scaled this frame, before any transform propagates:
  // its tiles move with its layers' cells, and block the same cells
  let Some(mut at) = maps.world.get_mut::<Transform>(root) else {
    panic!("the map has a transform")
  };
  *at = Transform::from_xy(300.0, -120.0).with_scale(Vec2::splat(2.0));
  let moved = navigation(&mut maps, root, "floor", bounds);
  let now = MapsView::new(&mut maps.world).grid(root, "floor");
  assert_eq!(moved.layer(), now);
  assert_ne!(moved.layer(), Some(floor));
  assert_eq!(blocked(&moved), expected);
}

#[test]
fn a_map_s_navigation_is_none_until_it_lands_and_off_tile_layers() {
  let mut map = map_of(
    GridShape::Isometric,
    UVec2::new(64, 32),
    Some(UVec2::new(4, 4)),
  );
  map.add_object_layer("things");
  let mut maps = MapWorld::new();
  let handle = maps.world.resource_mut::<Assets<TileMap>>().add(map);
  let root = maps.world.spawn(MapRoot::new(handle)).id();
  let window = CellRect::from_size(UVec2::splat(4));
  let view = MapsView::new(&mut maps.world);
  assert!(view.navigation(root, "ground", window).is_none());

  maps.settle(root);
  let view = MapsView::new(&mut maps.world);
  assert!(view.navigation(root, "ground", window).is_some());
  assert!(view.navigation(root, 0_usize, window).is_some());
  assert!(view.navigation(root, "things", window).is_none());
  assert!(view.navigation(root, 1_usize, window).is_none());
  assert!(view.navigation(root, 2_usize, window).is_none());
  assert!(view.navigation(root, "nowhere", window).is_none());
  assert!(
    view
      .navigation(Entity::PLACEHOLDER, "ground", window)
      .is_none()
  );
}

/// A streamed map's cell side, and its chunk's, in pixels
const CELL: f32 = 16.0;
const CHUNK: f32 = 32.0 * CELL;

/// A tileset of 16 px tiles: tile 0 a solid block
fn blocks() -> TilesetSource {
  let mut set = Tileset::new("blocks", UVec2::splat(16), TilesetImage::Collection);
  set.tile_count = 1;
  set.insert_tile(
    0,
    TileData {
      shapes: vec![TileShape {
        points: vec![
          Vec2::ZERO,
          Vec2::new(16.0, 0.0),
          Vec2::new(16.0, 16.0),
          Vec2::new(0.0, 16.0),
        ],
        ..TileShape::default()
      }],
      ..TileData::default()
    },
  );
  TilesetSource::Embedded(Arc::new(set))
}

/// A generator walling every chunk's "ground" down its column 16 from row
/// 0 to row 20, so rows 21 to 31 pass under the wall
fn walled(_: ChunkRequest) -> ChunkTiles {
  let mut tiles = ChunkTiles::default();
  for y in 0..=20 {
    tiles.set("ground", UVec2::new(16, y), Some(Tile::new(0, 0)));
  }
  tiles
}

/// The cells `walled` blocks in the chunks (x, 0) for each of `xs`
fn walls_in(xs: &[i32]) -> Vec<IVec2> {
  xs.iter()
    .flat_map(|&x| (0..=20).map(move |y| IVec2::new(x * 32 + 16, y)))
    .collect()
}

/// An infinite map of 16 px cells with a colliding "ground" layer,
/// streamed by `stream` with the cameras left out (or not streamed, for
/// `None`), at the origin and landed
fn endless(maps: &mut MapWorld, stream: Option<MapStream>) -> Entity {
  let mut map = TileMap::infinite(UVec2::splat(16));
  map.add_tileset(blocks());
  map.add_tile_layer("ground");
  let handle = maps.world.resource_mut::<Assets<TileMap>>().add(map);
  let root = maps.world.spawn(MapRoot::new(handle)).id();
  if let Some(stream) = stream {
    maps.world.entity_mut(root).insert(MapStream {
      cameras: false,
      ..stream
    });
  }
  maps.settle(root);
  root
}

/// An anchor in the middle of chunk (x, 0)
fn anchor_in(world: &mut World, x: i32) -> Entity {
  let at = Vec2::new((x as f32 + 0.5) * CHUNK, 0.5 * CHUNK);
  world
    .spawn((Transform::from_translation(at), StreamAnchor))
    .id()
}

/// Runs frames until every chunk (x, 0) of `map`, for each of `xs`, has
/// landed since `cursor` last read
fn until_landed(
  maps: &mut MapWorld,
  map: Entity,
  cursor: &mut MessageCursor<ChunkEvent>,
  xs: &[i32],
) {
  let mut landed = Vec::new();
  maps.frames_until(|world| {
    if let Some(messages) = world.get_resource::<Messages<ChunkEvent>>() {
      for event in cursor.read(messages) {
        if let ChunkEvent::Loaded { map: of, chunk } = event
          && *of == map
        {
          landed.push(*chunk);
        }
      }
    }
    xs.iter().all(|&x| landed.contains(&IVec2::new(x, 0)))
  });
}

#[test]
fn a_streamed_map_s_navigation_blocks_unloaded_chunks_until_they_land() {
  // One chunk tall, across chunks 0 to 3 of row 0
  let window = CellRect::new(IVec2::ZERO, IVec2::new(128, 32));

  // Not streamed: a chunk the map has no cells in blocks nothing
  let mut maps = MapWorld::new();
  let plain = endless(&mut maps, None);
  assert_eq!(blocked(&navigation(&mut maps, plain, "ground", window)), []);

  // A bounded map given a stream streams nothing: its layer's cells are
  // all there, and only its tile blocks
  let mut bounded = TileMap::new(UVec2::new(128, 32), UVec2::splat(16));
  bounded.add_tileset(blocks());
  let ground = bounded.add_tile_layer("ground");
  if let Some(grid) = bounded.tiles_mut(ground) {
    grid.set(IVec2::new(40, 5), Some(Tile::new(0, 0)));
  }
  let handle = maps.world.resource_mut::<Assets<TileMap>>().add(bounded);
  let stream = MapStream {
    cameras: false,
    ..MapStream::new(walled)
  };
  let bounded = maps.world.spawn((MapRoot::new(handle), stream)).id();
  maps.settle(bounded);
  assert!(
    MapsView::new(&mut maps.world)
      .stream_status(bounded)
      .is_some()
  );
  let grid = navigation(&mut maps, bounded, "ground", window);
  assert_eq!(blocked(&grid), [IVec2::new(40, 5)]);

  // Anchors in chunks 0 and 3, each loading only the chunk it is in
  let mut maps = MapWorld::new();
  let stream = MapStream {
    load_distance: 100.0,
    ..MapStream::new(walled)
  };
  let map = endless(&mut maps, Some(stream));
  let mut cursor = MessageCursor::<ChunkEvent>::default();
  let first = anchor_in(&mut maps.world, 0);
  let last = anchor_in(&mut maps.world, 3);
  until_landed(&mut maps, map, &mut cursor, &[0, 3]);
  let status = MapsView::new(&mut maps.world).stream_status(map);
  assert_eq!(status.map(|status| status.resident), Some(2));

  // Every cell of chunks 1 and 2 is blocked, and nothing crosses them
  let grid = navigation(&mut maps, map, "ground", window);
  let gap = cells(IVec2::new(32, 0), IVec2::new(95, 31));
  assert_eq!(blocked(&grid), by_row([walls_in(&[0, 3]), gap].concat()));
  let (from, to) = (IVec2::new(2, 2), IVec2::new(125, 2));
  assert_eq!(
    Pathfinder::new().find_path_grid(&grid, from.into(), to.into()),
    PathResult::NoPath
  );

  // The anchors move into chunks 1 and 2, which land: a grid taken again
  // opens them, and a path crosses every chunk under each wall
  for (anchor, x) in [(first, 1), (last, 2)] {
    if let Some(mut at) = maps.world.get_mut::<Transform>(anchor) {
      at.translation = Vec2::new((x as f32 + 0.5) * CHUNK, 0.5 * CHUNK);
    }
  }
  until_landed(&mut maps, map, &mut cursor, &[1, 2]);
  let grid = navigation(&mut maps, map, "ground", window);
  assert_eq!(blocked(&grid), by_row(walls_in(&[0, 1, 2, 3])));
  let Some(path) = path_cells(&grid, from, to) else {
    panic!("a way under the walls")
  };
  assert!(path.iter().all(|&at| grid.is_walkable(at.into())));
  assert!(path.iter().any(|at| at.y > 20), "{path:?}");
}

#[test]
fn navigation_on_an_isometric_map_matches_its_collision() {
  let cell = UVec2::new(64, 32);
  let (w, h) = (64.0, 32.0);
  let with = |points: &[(f32, f32)]| TileData {
    shapes: vec![TileShape {
      points: points.iter().map(|&(x, y)| Vec2::new(x, y)).collect(),
      ..TileShape::default()
    }],
    ..TileData::default()
  };
  // A whole diamond, a slope on three of its corners, a small diamond round
  // its centre, and a sliver by its top corner, clear of the centre
  let mut set = Tileset::new("lattice", cell, TilesetImage::Collection);
  set.tile_count = 4;
  set.insert_tile(
    0,
    with(&[(w / 2.0, 0.0), (w, h / 2.0), (w / 2.0, h), (0.0, h / 2.0)]),
  );
  set.insert_tile(1, with(&[(w / 2.0, 0.0), (w, h / 2.0), (w / 2.0, h)]));
  set.insert_tile(
    2,
    with(&[
      (w / 2.0, 8.0),
      (48.0, h / 2.0),
      (w / 2.0, 24.0),
      (16.0, h / 2.0),
    ]),
  );
  set.insert_tile(3, with(&[(w / 2.0, 0.0), (40.0, 4.0), (24.0, 4.0)]));
  let mut map = TileMap::new(UVec2::new(8, 6), cell).with_shape(GridShape::Isometric);
  let set = map.add_tileset(TilesetSource::Embedded(Arc::new(set)));
  let walls = map.add_tile_layer("walls");
  let placed_tiles = [
    ((1, 1), 0),
    ((2, 1), 0),
    ((3, 1), 0),
    ((5, 2), 1),
    ((2, 4), 2),
    ((6, 4), 3),
  ];
  if let Some(grid) = map.tiles_mut(walls) {
    for ((x, y), id) in placed_tiles {
      grid.set(IVec2::new(x, y), Some(Tile::new(set, id)));
    }
  }
  let mut maps = MapWorld::new();
  let root = spawn(&mut maps, map, placed());
  let Some(tiles) = maps.world.get::<TileColliders>(root).cloned() else {
    panic!("the walls collide on the map")
  };
  assert_eq!(tiles.get(IVec2::new(1, 1)), TileCell::Solid);
  assert!(matches!(tiles.get(IVec2::new(5, 2)), TileCell::Slope(_)));
  assert!(matches!(tiles.get(IVec2::new(2, 4)), TileCell::Shape(_)));
  assert!(matches!(tiles.get(IVec2::new(6, 4)), TileCell::Shape(_)));

  // A window wider than the map, cells round it included
  let window = CellRect::new(IVec2::splat(-2), IVec2::new(10, 8));
  let grid = navigation(&mut maps, root, "walls", window);
  let blocking: Vec<IVec2> = tiles
    .cells_in(window)
    .filter(|&(_, kind)| match kind {
      TileCell::Solid | TileCell::Slope(_) => true,
      TileCell::Shape(shape) => tiles.shape_blocks(shape),
      TileCell::Empty | TileCell::OneWay => false,
    })
    .map(|(at, _)| at)
    .collect();
  let blocking = by_row(blocking);
  let expected = [(1, 1), (2, 1), (3, 1), (5, 2), (2, 4)];
  assert_eq!(
    blocking,
    by_row(expected.map(|(x, y)| IVec2::new(x, y)).to_vec())
  );
  assert_eq!(blocked(&grid), blocking);
}

/// Times taking a navigation grid over a 256 x 256 window: of a 512 x 512
/// hexagonal map whose colliding walls (3 x 3 clumps every 8 cells) share
/// its floor's cells, and of an endless map streamed round one anchor, most
/// of the window's chunks not landed. Run it in release: `cargo test
/// --release -p ryo2d --lib navigation_window_timing -- --ignored --nocapture`
#[test]
#[ignore = "timing; run in release by hand"]
fn navigation_window_timing() {
  use std::time::{Duration, Instant};

  let median = |mut times: Vec<Duration>| {
    times.sort();
    (times[times.len() / 2], times[times.len() - 1])
  };
  let time = |maps: &mut MapWorld, map: Entity, layer: &str, window: CellRect| {
    let mut times = Vec::new();
    for _ in 0..200 {
      let start = Instant::now();
      let grid = MapsView::new(&mut maps.world).navigation(map, layer, window);
      times.push(start.elapsed());
      assert!(grid.is_some());
    }
    median(times)
  };

  const SIDE: i32 = 512;
  let cell = UVec2::new(32, 28);
  let mut map = TileMap::new(UVec2::splat(SIDE as u32), cell).with_shape(GridShape::Hexagonal {
    axis: Y,
    index: Odd,
    side: 12,
  });
  let mut set = whole_hex_set(&map.grid());
  set.insert_tile(1, TileData::default());
  set.tile_count = 2;
  let set = map.add_tileset(TilesetSource::Embedded(Arc::new(set)));
  let floor = map.add_tile_layer("floor");
  let walls = map.add_tile_layer("walls");
  if let Some(grid) = map.tiles_mut(floor) {
    grid.fill(IVec2::ZERO, IVec2::splat(SIDE - 1), Some(Tile::new(set, 1)));
  }
  if let Some(grid) = map.tiles_mut(walls) {
    for y in (0..SIDE).step_by(8) {
      for x in (0..SIDE).step_by(8) {
        grid.fill(
          IVec2::new(x + 2, y + 2),
          IVec2::new(x + 4, y + 4),
          Some(Tile::new(set, 0)),
        );
      }
    }
  }
  let mut maps = MapWorld::new();
  let hexes = spawn(&mut maps, map, Transform::IDENTITY);
  let window = CellRect::new(IVec2::splat(128), IVec2::splat(384));
  // 32 x 32 clumps of nine hexes in the window
  let grid = navigation(&mut maps, hexes, "floor", window);
  assert_eq!(blocked(&grid).len(), 32 * 32 * 9);
  let (mid, max) = time(&mut maps, hexes, "floor", window);
  println!("a hex map's 256 x 256 window (median, max): {mid:?}, {max:?}");

  let mut maps = MapWorld::new();
  let map = endless(&mut maps, Some(MapStream::new(walled)));
  // The chunks round the anchor's: (-1, -1) to (1, 1)
  anchor_in(&mut maps.world, 0);
  maps.frames_until(|world| {
    MapsView::new(world)
      .stream_status(map)
      .is_some_and(|status| status.resident == 9 && status.in_flight == 0)
  });
  let window = CellRect::new(IVec2::splat(-96), IVec2::splat(160));
  // 55 chunks not landed, and a wall of 21 cells in each of the 9 landed
  let grid = navigation(&mut maps, map, "ground", window);
  assert_eq!(blocked(&grid).len(), 55 * 32 * 32 + 9 * 21);
  let (mid, max) = time(&mut maps, map, "ground", window);
  println!(
    "a streamed map's 256 x 256 window, 9 of its 64 chunks landed (median, max): {mid:?}, {max:?}"
  );
}
