//! A layer's grid in the world (`MapsView::grid`): picking and edits on the
//! cell under a point, outlines in the world and the board tools, on every
//! grid, on a map moved, turned and scaled.

use std::sync::Arc;

use glam::{IVec2, UVec2, Vec2};

use crate::core::asset::Assets;
use crate::core::ecs::{Entity, Transform};
use crate::core::tilemap::test_util::MapWorld;
use crate::core::tilemap::{
  GridShape, LayerGrid, MapRoot, MapsView, StaggerAxis, StaggerIndex, Tile, TileMap, Tileset,
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
