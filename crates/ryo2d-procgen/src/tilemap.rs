//! Grids as tile maps: a generated level that draws, collides and edits like
//! a designer's map.

use std::collections::HashSet;

use ryo2d::prelude::*;

use crate::config::TileType;
use crate::grid::GenerationGrid;

/// For each Wang position, in Tiled's order (top, top-right, right,
/// bottom-right, bottom, bottom-left, left, top-left), the offsets of the
/// cells that share it
const SHARED: [&[(i32, i32)]; 8] = [
  &[(0, 0), (0, -1)],
  &[(0, 0), (1, 0), (0, -1), (1, -1)],
  &[(0, 0), (1, 0)],
  &[(0, 0), (1, 0), (0, 1), (1, 1)],
  &[(0, 0), (0, 1)],
  &[(0, 0), (-1, 0), (0, 1), (-1, 1)],
  &[(0, 0), (-1, 0)],
  &[(0, 0), (-1, 0), (0, -1), (-1, -1)],
];

/// Where a generated grid's cells go on a map of a shape: each through the
/// shape's square lattice, so cells side by side in the grid stay
/// neighbours on the map, and the map's size, the smallest holding them all
/// from (0, 0)
struct Placement {
  lattice: Lattice,
  /// Added to a grid cell before the lattice takes it
  shift: IVec2,
  /// Added to the map cell the lattice gives
  offset: IVec2,
  size: UVec2,
}

impl Placement {
  /// The placement of a `w` x `h` grid on a map of `shape`; `None` on a
  /// hexagonal map, whose cells make no square lattice
  fn of(shape: GridShape, tile_size: UVec2, (w, h): (u32, u32)) -> Option<Placement> {
    let lattice = MapGrid::new(shape, tile_size).lattice()?;
    let mut placement = Placement {
      lattice,
      shift: IVec2::ZERO,
      offset: IVec2::ZERO,
      size: UVec2::new(w, h),
    };
    // Orthogonal, isometric and oblique maps index their lattice as it is
    let GridShape::Staggered { axis, .. } = shape else {
      return Some(placement);
    };
    if w == 0 || h == 0 {
      return Some(placement);
    }
    // A map cell's line (its row on axis Y, its column on axis X) and its
    // place along the line
    let split = |cell: IVec2| match axis {
      StaggerAxis::Y => (cell.y, cell.x),
      StaggerAxis::X => (cell.x, cell.y),
    };
    // Lines and places only grow or shrink across the grid, so its corners
    // hold their extremes
    let (right, bottom) = (w as i32 - 1, h as i32 - 1);
    let corners = [
      IVec2::ZERO,
      IVec2::new(right, 0),
      IVec2::new(0, bottom),
      IVec2::new(right, bottom),
    ];
    // A step along the lattice's x is a step to the next line on every
    // staggered grid, so shifting along it starts the lines at 0
    let first = corners
      .iter()
      .map(|&corner| split(lattice.grid_cell(corner)).0)
      .min()
      .unwrap_or(0);
    placement.shift = IVec2::new(-first, 0);
    let ends = corners.map(|corner| split(lattice.grid_cell(corner + placement.shift)));
    let last_line = ends.iter().map(|end| end.0).max().unwrap_or(0);
    let (first_place, last_place) = (
      ends.iter().map(|end| end.1).min().unwrap_or(0),
      ends.iter().map(|end| end.1).max().unwrap_or(0),
    );
    // Moving cells along their lines keeps every line's shift
    let (lines, places) = (
      (last_line + 1) as u32,
      (last_place - first_place + 1) as u32,
    );
    (placement.offset, placement.size) = match axis {
      StaggerAxis::Y => (IVec2::new(-first_place, 0), UVec2::new(places, lines)),
      StaggerAxis::X => (IVec2::new(0, -first_place), UVec2::new(lines, places)),
    };
    Some(placement)
  }

  /// The map cell of the grid's cell (`x`, `y`)
  fn cell(&self, x: i32, y: i32) -> IVec2 {
    self.lattice.grid_cell(IVec2::new(x, y) + self.shift) + self.offset
  }
}

fn warn_missing_color(name: &str, set: &str) {
  log::warn!("Wang set \"{set}\" has no colour \"{name}\"; its cells stay empty");
}

impl GenerationGrid {
  /// The grid as a [`TileMap`] of `shape` with one tile layer, `"tiles"`,
  /// of `tile_size` cells, and `tileset` as its only tileset. Each cell takes
  /// the tile `tile_of` gives its type, an id in the tileset; a type it
  /// gives `None` is left empty.
  ///
  /// The generators are square, so the grid is the map's square lattice
  /// ([`MapGrid::lattice`]): on an orthogonal, isometric or oblique map,
  /// as big as the grid, cell `(x, y)` is the map's `(x, y)` (row 0 the
  /// top row; on an isometric map the diamond's top-right side). A
  /// staggered map indexes its lattice otherwise: each cell goes to its
  /// lattice cell's map cell ([`Lattice::grid_cell`]), so the grid's
  /// rectangle stands as a diamond on a map as big as holds it, and cells
  /// side by side in the grid are neighbours on the map as on the others.
  /// `None` for a hexagonal map, whose cells make no square lattice.
  pub fn to_tile_map(
    &self,
    shape: GridShape,
    tile_size: UVec2,
    tileset: TilesetSource,
    tile_of: impl Fn(TileType) -> Option<u32>,
  ) -> Option<TileMap> {
    let placement = Placement::of(shape, tile_size, self.dimensions())?;
    let (mut map, layer) = empty_map(shape, placement.size, tile_size, tileset);
    if let Some(tiles) = map.tiles_mut(layer) {
      for (x, y, kind) in self.iter() {
        if let Some(id) = tile_of(kind) {
          tiles.set(placement.cell(x, y), Some(Tile::new(0, id)));
        }
      }
    }
    Some(map)
  }

  /// The grid as a [`TileMap`] like [`to_tile_map`](Self::to_tile_map), with
  /// each cell's tile and flip picked from `wang_set` (a set of `tileset`,
  /// found with [`Tileset::wang_set`]) so that edges and corners join.
  ///
  /// `color_of` names the set's colour a type is drawn as; `None`, or a name
  /// the set lacks (warned of once), is empty, colour 0. A cell's Wang
  /// positions are not its own colour alone, so neighbours always agree on
  /// what they share: a corner takes the colour of highest precedence among
  /// the up to four cells that meet there, an edge the higher of its two
  /// cells. Precedence is the set's colour order, a higher colour covering a
  /// lower and every colour covering empty; outside the grid is empty. Only
  /// the positions the set's kind uses are asked for. So the higher colour
  /// grows half a cell into the lower one, and a one-cell strip of the lower
  /// colour between two higher ones is covered; order the colours in Tiled
  /// to choose which covers which.
  ///
  /// A cell the set has no exact tile for (an incomplete set) takes the
  /// closest tile that fits, as Tiled's terrain brush would pick it
  /// ([`WangSet::best_tile`]); it is left empty only where no tile fits or
  /// the empty cell fits best.
  ///
  /// The corners and edges are the square lattice's, worked out in the
  /// grid: on an isometric or staggered map a cell's top edge is its
  /// diamond's top-right side, the one it shares with the cell above it in
  /// the grid. `None` for a hexagonal map.
  pub fn to_terrain_map<'a>(
    &self,
    shape: GridShape,
    tile_size: UVec2,
    tileset: TilesetSource,
    wang_set: &WangSet,
    color_of: impl Fn(TileType) -> Option<&'a str>,
  ) -> Option<TileMap> {
    let placement = Placement::of(shape, tile_size, self.dimensions())?;
    let (mut map, layer) = empty_map(shape, placement.size, tile_size, tileset);
    let (w, h) = self.dimensions();
    let mut missing: HashSet<&str> = HashSet::new();
    let colors: Vec<u8> = self
      .iter()
      .map(|(_, _, kind)| match color_of(kind) {
        None => 0,
        Some(name) => wang_set.color_index(name).unwrap_or_else(|| {
          if missing.insert(name) {
            warn_missing_color(name, wang_set.name());
          }
          0
        }),
      })
      .collect();
    let at = |x: i32, y: i32| -> u8 {
      if x < 0 || y < 0 || x >= w as i32 || y >= h as i32 {
        0
      } else {
        colors[(y as u32 * w + x as u32) as usize]
      }
    };
    let (corners, edges) = match wang_set.kind() {
      WangKind::Corner => (true, false),
      WangKind::Edge => (false, true),
      WangKind::Mixed => (true, true),
    };
    let Some(tiles) = map.tiles_mut(layer) else {
      return Some(map);
    };
    for y in 0..h as i32 {
      for x in 0..w as i32 {
        let mut wanted = [WangSet::ANY; 8];
        for (k, shared) in SHARED.iter().enumerate() {
          let used = if k % 2 == 1 { corners } else { edges };
          if used {
            wanted[k] = shared
              .iter()
              .map(|&(dx, dy)| at(x + dx, y + dy))
              .max()
              .unwrap_or(0);
          }
        }
        let cell = placement.cell(x, y);
        if let Some((id, flip)) = wang_set.best_tile(wanted, cell) {
          tiles.set(
            cell,
            Some(Tile {
              tileset: 0,
              id,
              flip,
            }),
          );
        }
      }
    }
    Some(map)
  }
}

/// A map of `shape` and `size` cells, the one tileset and an empty
/// `"tiles"` layer
fn empty_map(
  shape: GridShape,
  size: UVec2,
  tile_size: UVec2,
  tileset: TilesetSource,
) -> (TileMap, usize) {
  let mut map = TileMap::new(size, tile_size).with_shape(shape);
  map.add_tileset(tileset);
  let layer = map.add_tile_layer("tiles");
  (map, layer)
}

#[cfg(test)]
mod tests {
  use crate::prelude::*;
  use ryo2d::prelude::*;
  use std::sync::Arc;

  fn tileset() -> TilesetSource {
    TilesetSource::Embedded(Arc::new(Tileset::new(
      "t",
      UVec2::splat(16),
      TilesetImage::Collection,
    )))
  }

  fn color(name: &str) -> WangColor {
    WangColor {
      name: name.into(),
      color: Color::WHITE,
      tile: None,
      probability: 1.0,
      properties: Default::default(),
    }
  }

  /// A corner set over `names` (colours 1..): one tile for every
  /// combination of corner colours, whose id is the corners as base-3
  /// digits, top-right lowest
  fn corner_set(names: &[&str]) -> WangSet {
    let base = names.len() as u32 + 1;
    let mut tiles = Vec::new();
    for id in 0..base.pow(4) {
      let mut wangid = [0u8; 8];
      for (n, slot) in [1, 3, 5, 7].into_iter().enumerate() {
        wangid[slot] = ((id / base.pow(n as u32)) % base) as u8;
      }
      tiles.push(WangTile {
        tile: id,
        wangid,
        probability: 1.0,
      });
    }
    WangSet::new(
      "ground",
      WangKind::Corner,
      names.iter().map(|n| color(n)).collect(),
      tiles,
      TileTransformations::default(),
    )
  }

  fn tile_at(map: &TileMap, x: i32, y: i32) -> Option<Tile> {
    map
      .tiles(map.layer_index("tiles").unwrap())?
      .get(IVec2::new(x, y))
  }

  #[test]
  fn a_dungeon_becomes_a_map_of_its_tiles() {
    let mut grid = GenerationGrid::filled(4, 3, TileType::Wall);
    grid.set(1, 1, TileType::Floor);
    grid.set(2, 1, TileType::Water);
    let map = grid
      .to_tile_map(
        GridShape::Orthogonal,
        UVec2::splat(16),
        tileset(),
        |t| match t {
          TileType::Wall => Some(7),
          TileType::Floor => Some(2),
          _ => None,
        },
      )
      .expect("a square grid makes a map");
    assert_eq!(map.size, UVec2::new(4, 3));
    assert_eq!(map.tile_size, UVec2::splat(16));
    assert_eq!(map.tilesets.len(), 1);
    assert_eq!(map.layers.len(), 1);
    assert_eq!(map.layers[0].info.name, "tiles");
    assert_eq!(tile_at(&map, 0, 0), Some(Tile::new(0, 7)));
    assert_eq!(tile_at(&map, 1, 1), Some(Tile::new(0, 2)));
    assert_eq!(tile_at(&map, 2, 1), None, "tile_of said None");
  }

  #[test]
  fn a_terrain_map_picks_wang_edge_tiles() {
    // TR = 1, BR = 2, BL = 4, TL = 8 for the one colour
    let set = corner_set(&["floor"]);
    let mut grid = GenerationGrid::new(5, 5);
    grid.set(2, 2, TileType::Floor);
    let map = grid
      .to_terrain_map(
        GridShape::Orthogonal,
        UVec2::splat(16),
        tileset(),
        &set,
        |t| (t == TileType::Floor).then_some("floor"),
      )
      .expect("a square grid makes a map");
    let id = |x, y| tile_at(&map, x, y).map(|t| t.id);
    assert_eq!(id(2, 2), Some(15), "all four corners floor");
    assert_eq!(id(1, 1), Some(2), "bottom-right corner touches the room");
    assert_eq!(id(3, 1), Some(4));
    assert_eq!(id(1, 3), Some(1));
    assert_eq!(id(3, 3), Some(8));
    assert_eq!(id(2, 1), Some(6), "bottom edge");
    assert_eq!(id(2, 3), Some(9), "top edge");
    assert_eq!(id(0, 0), None, "no corner is floor: left empty");
  }

  #[test]
  fn the_higher_colour_covers_the_lower_at_a_shared_corner() {
    // colours water 1, floor 2; digits base 3: TR 1, BR 3, BL 9, TL 27
    let set = corner_set(&["water", "floor"]);
    let mut grid = GenerationGrid::new(2, 2);
    grid.set(0, 0, TileType::Water);
    grid.set(1, 0, TileType::Floor);
    let map = grid
      .to_terrain_map(
        GridShape::Orthogonal,
        UVec2::splat(16),
        tileset(),
        &set,
        |t| match t {
          TileType::Water => Some("water"),
          TileType::Floor => Some("floor"),
          _ => None,
        },
      )
      .expect("a square grid makes a map");
    let id = |x, y| tile_at(&map, x, y).map(|t| t.id);
    // (0,0): top-right, bottom-right: floor covers water; bottom-left and
    // top-left: only water
    assert_eq!(id(0, 0), Some(2 + 2 * 3 + 9 + 27));
    // (1,0): every corner touches the floor cell
    assert_eq!(id(1, 0), Some(2 * (1 + 3 + 9 + 27)));
  }

  #[test]
  fn a_colour_the_set_lacks_is_empty() {
    let set = corner_set(&["floor"]);
    let mut grid = GenerationGrid::new(2, 2);
    grid.set(0, 0, TileType::Floor);
    let map = grid
      .to_terrain_map(
        GridShape::Orthogonal,
        UVec2::splat(16),
        tileset(),
        &set,
        |_| Some("lava"),
      )
      .expect("a square grid makes a map");
    assert_eq!(tile_at(&map, 0, 0), None);
    assert_eq!(tile_at(&map, 1, 1), None);
  }

  /// A set of `kind` with one colour "floor" and the given tiles, as
  /// (id, wangid)
  fn one_colour_set(
    kind: WangKind,
    transformations: TileTransformations,
    tiles: &[(u32, [u8; 8])],
  ) -> WangSet {
    WangSet::new(
      "ground",
      kind,
      vec![color("floor")],
      tiles
        .iter()
        .map(|&(tile, wangid)| WangTile {
          tile,
          wangid,
          probability: 1.0,
        })
        .collect(),
      transformations,
    )
  }

  fn room(width: u32) -> GenerationGrid {
    let mut grid = GenerationGrid::new(width, width);
    grid.set(2, 2, TileType::Floor);
    grid
  }

  fn floor_of(t: TileType) -> Option<&'static str> {
    (t == TileType::Floor).then_some("floor")
  }

  #[test]
  fn an_incomplete_set_draws_the_closest_tile() {
    // Only the full tile (15) and the top-right corner (1): the cell left of
    // the floor wants its top-right and bottom-right corners (3), which the
    // set lacks, and the top-right corner is the one closest
    let corners = |mask: u8| {
      let mut wangid = [0u8; 8];
      for (n, slot) in [1, 3, 5, 7].into_iter().enumerate() {
        wangid[slot] = (mask >> n) & 1;
      }
      wangid
    };
    let set = one_colour_set(
      WangKind::Corner,
      TileTransformations::default(),
      &[(15, corners(15)), (1, corners(1))],
    );
    let mut grid = GenerationGrid::new(3, 1);
    grid.set(1, 0, TileType::Floor);
    let map = grid
      .to_terrain_map(
        GridShape::Orthogonal,
        UVec2::splat(16),
        tileset(),
        &set,
        floor_of,
      )
      .expect("a square grid makes a map");
    let id = |x, y| tile_at(&map, x, y).map(|t| t.id);
    assert_eq!(id(1, 0), Some(15));
    assert_eq!(id(0, 0), Some(1), "the closest fit, not an empty cell");
  }

  #[test]
  fn an_edge_set_picks_by_edges_alone() {
    // top 1, right 2, bottom 4, left 8: one tile for each combination
    let tiles: Vec<(u32, [u8; 8])> = (1..16u32)
      .map(|id| {
        let mut wangid = [0u8; 8];
        for (n, slot) in [0, 2, 4, 6].into_iter().enumerate() {
          wangid[slot] = ((id >> n) & 1) as u8;
        }
        (id, wangid)
      })
      .collect();
    let set = one_colour_set(WangKind::Edge, TileTransformations::default(), &tiles);
    let map = room(5)
      .to_terrain_map(
        GridShape::Orthogonal,
        UVec2::splat(16),
        tileset(),
        &set,
        floor_of,
      )
      .expect("a square grid makes a map");
    let id = |x, y| tile_at(&map, x, y).map(|t| t.id);
    assert_eq!(id(2, 2), Some(15));
    assert_eq!(id(2, 1), Some(4), "its bottom edge is the room's top");
    assert_eq!(id(1, 2), Some(2));
    assert_eq!(id(3, 2), Some(8));
    assert_eq!(id(2, 3), Some(1));
    assert_eq!(id(1, 1), None, "a diagonal neighbour shares no edge");
  }

  #[test]
  fn a_mixed_set_picks_by_edges_and_corners() {
    // wangid order: top, TR, right, BR, bottom, BL, left, TL
    let tiles = [
      (1, [1; 8]),
      (2, [0, 0, 0, 1, 1, 1, 0, 0]),
      (3, [0, 0, 0, 1, 0, 0, 0, 0]),
      (4, [0, 0, 0, 0, 1, 0, 0, 0]),
      (5, [0, 0, 0, 1, 0, 1, 0, 0]),
    ];
    let set = one_colour_set(WangKind::Mixed, TileTransformations::default(), &tiles);
    let map = room(5)
      .to_terrain_map(
        GridShape::Orthogonal,
        UVec2::splat(16),
        tileset(),
        &set,
        floor_of,
      )
      .expect("a square grid makes a map");
    let id = |x, y| tile_at(&map, x, y).map(|t| t.id);
    assert_eq!(id(2, 2), Some(1));
    assert_eq!(id(2, 1), Some(2), "the bottom edge and both bottom corners");
    assert_eq!(id(1, 1), Some(3), "a corner alone");
    assert_eq!(id(0, 0), None);
  }

  #[test]
  fn a_terrain_map_draws_a_tile_flipped_where_only_a_flip_fits() {
    // the one tile has floor at its top-right corner only
    let tiles = [(0, [0, 1, 0, 0, 0, 0, 0, 0])];
    let allowed = TileTransformations {
      hflip: true,
      ..Default::default()
    };
    let set = one_colour_set(WangKind::Corner, allowed, &tiles);
    let map = room(5)
      .to_terrain_map(
        GridShape::Orthogonal,
        UVec2::splat(16),
        tileset(),
        &set,
        floor_of,
      )
      .expect("a square grid makes a map");
    let at = |x, y| tile_at(&map, x, y).map(|t| (t.id, t.flip));
    assert_eq!(
      at(1, 3),
      Some((0, TileFlip::NONE)),
      "floor at its top-right"
    );
    assert_eq!(
      at(3, 3),
      Some((0, TileFlip::HORIZONTAL)),
      "floor at its top-left"
    );
  }

  #[test]
  fn a_colour_the_set_lacks_warns_once_per_conversion() {
    let log = ryo2d::testing::capture_log();
    let set = corner_set(&["floor"]);
    let grid = GenerationGrid::filled(3, 3, TileType::Floor);
    let warnings = || {
      log
        .warnings()
        .iter()
        .filter(|message| message.contains("lava-warn-once"))
        .count()
    };
    grid.to_terrain_map(
      GridShape::Orthogonal,
      UVec2::splat(16),
      tileset(),
      &set,
      |_| Some("lava-warn-once"),
    );
    assert_eq!(warnings(), 1, "nine cells, one warning");
    grid.to_terrain_map(
      GridShape::Orthogonal,
      UVec2::splat(16),
      tileset(),
      &set,
      |_| Some("lava-warn-once"),
    );
    assert_eq!(warnings(), 2, "another conversion warns again");
  }

  /// The map cell each cell of a `w` x `h` grid goes to on a map of
  /// `shape`, row by row, and the map's size: found one cell at a time, as
  /// the one tile of a grid whose only floor is that cell
  fn placed(shape: GridShape, (w, h): (u32, u32)) -> (Vec<IVec2>, UVec2) {
    let mut cells = Vec::new();
    let mut sizes = Vec::new();
    for y in 0..h as i32 {
      for x in 0..w as i32 {
        let mut grid = GenerationGrid::filled(w, h, TileType::Wall);
        grid.set(x, y, TileType::Floor);
        let map = grid
          .to_tile_map(shape, UVec2::new(64, 32), tileset(), |t| {
            (t == TileType::Floor).then_some(1)
          })
          .unwrap_or_else(|| panic!("{shape:?} makes a map"));
        assert_eq!(map.shape, shape);
        let tiles = map.tiles(0).expect("a tile layer");
        let size = map.size.as_ivec2();
        let found: Vec<IVec2> = (0..size.y)
          .flat_map(|y| (0..size.x).map(move |x| IVec2::new(x, y)))
          .filter(|&cell| tiles.get(cell).is_some())
          .collect();
        assert_eq!(found.len(), 1, "{shape:?} ({x}, {y}): one tile, on the map");
        cells.push(found[0]);
        sizes.push(map.size);
      }
    }
    sizes.dedup();
    assert_eq!(sizes.len(), 1, "{shape:?}: one size");
    (cells, sizes[0])
  }

  /// The `ground` corner set of the Tiled fixtures with its colours swapped
  /// (dirt 1, grass 2), so that grass covers dirt where a generated grid
  /// puts them side by side
  fn fixture_ground_set() -> WangSet {
    let path = concat!(
      env!("CARGO_MANIFEST_DIR"),
      "/../../tests/fixtures/tilemap/tiled/tilesets/wang.tsj"
    );
    let json: serde_json::Value =
      serde_json::from_str(&std::fs::read_to_string(path).expect("the fixture")).expect("json");
    let ground = &json["wangsets"][0];
    assert_eq!(ground["name"], "ground");
    let swap = |c: u64| match c {
      1 => 2,
      2 => 1,
      other => other,
    };
    let tiles = ground["wangtiles"]
      .as_array()
      .expect("tiles")
      .iter()
      .map(|t| {
        let mut wangid = [0u8; 8];
        for (slot, c) in t["wangid"].as_array().expect("wangid").iter().enumerate() {
          wangid[slot] = swap(c.as_u64().expect("colour")) as u8;
        }
        WangTile {
          tile: t["tileid"].as_u64().expect("id") as u32,
          wangid,
          probability: 1.0,
        }
      })
      .collect();
    WangSet::new(
      "ground",
      WangKind::Corner,
      vec![color("dirt"), color("grass")],
      tiles,
      TileTransformations::default(),
    )
  }

  #[test]
  fn staggered_terrain_matches_what_tileds_brush_paints() {
    // One grass cell in dirt, against `wang-stag-*-grass.tmj`, in which
    // Tiled's brush painted one cell of dirt grass (`paint-wang.js`): the
    // nine cells around the painted one, on the lattice, hold the same tiles
    let set = fixture_ground_set();
    let (w, h) = (7u32, 7u32);
    let mut grid = GenerationGrid::filled(w, h, TileType::Wall);
    grid.set(3, 3, TileType::Floor);
    let tile_size = UVec2::new(32, 16);
    for (name, axis, index) in [
      ("stag-y-odd", StaggerAxis::Y, StaggerIndex::Odd),
      ("stag-y-even", StaggerAxis::Y, StaggerIndex::Even),
      ("stag-x-odd", StaggerAxis::X, StaggerIndex::Odd),
      ("stag-x-even", StaggerAxis::X, StaggerIndex::Even),
    ] {
      let shape = GridShape::Staggered { axis, index };
      let path = format!(
        "{}/../../tests/fixtures/tilemap/tiled/maps/wang-{name}-grass.tmj",
        env!("CARGO_MANIFEST_DIR")
      );
      let json: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(path).expect("the fixture")).expect("json");
      let data = json["layers"][0]["data"].as_array().expect("data");
      let painted = |cell: IVec2| data[(cell.y * 9 + cell.x) as usize].as_u64().expect("gid") - 1;
      let lattice = MapGrid::new(shape, tile_size).lattice().expect("lattice");
      let middle = lattice.lattice_cell(IVec2::splat(4));

      let map = grid
        .to_terrain_map(shape, tile_size, tileset(), &set, |t| {
          (t == TileType::Floor).then_some("grass").or(Some("dirt"))
        })
        .expect("a staggered grid makes a map");
      let (cells, _) = placed(shape, (w, h));
      for dy in -1..=1 {
        for dx in -1..=1 {
          let d = IVec2::new(dx, dy);
          let (x, y) = (3 + dx, 3 + dy);
          let cell = cells[(y as u32 * w + x as u32) as usize];
          let got = tile_at(&map, cell.x, cell.y).map(|t| u64::from(t.id));
          let expected = painted(lattice.grid_cell(middle + d));
          assert_eq!(got, Some(expected), "{name}: lattice offset {d}");
        }
      }
    }
  }

  #[test]
  fn procgen_builds_isometric_and_staggered_maps_from_its_square_grid() {
    let (w, h) = (7u32, 5u32);
    let grid = GenerationGrid::filled(w, h, TileType::Floor);
    let index = |x: u32, y: u32| (y * w + x) as usize;
    let mut shapes = vec![
      GridShape::Orthogonal,
      GridShape::Isometric,
      GridShape::Oblique {
        skew: IVec2::new(6, -3),
      },
    ];
    for axis in [StaggerAxis::X, StaggerAxis::Y] {
      for index in [StaggerIndex::Odd, StaggerIndex::Even] {
        shapes.push(GridShape::Staggered { axis, index });
      }
    }
    for shape in shapes {
      let (cells, size) = placed(shape, (w, h));
      let board = TileMap::new(size, UVec2::new(64, 32))
        .with_shape(shape)
        .grid();
      // Cells side by side in the generator's grid are neighbours on the map
      for y in 0..h {
        for x in 0..w {
          let here = cells[index(x, y)];
          if x + 1 < w {
            let right = cells[index(x + 1, y)];
            assert!(
              board.neighbours(here).any(|n| n == right),
              "{shape:?}: ({x}, {y}) at {here} and the cell right of it at {right}"
            );
          }
          if y + 1 < h {
            let below = cells[index(x, y + 1)];
            assert!(
              board.neighbours(here).any(|n| n == below),
              "{shape:?}: ({x}, {y}) at {here} and the cell below it at {below}"
            );
          }
        }
      }
      // The square lattice's cells keep their steps: two cells n steps
      // apart in the generator's grid are n steps apart on the map
      let far = (cells[index(0, 0)], cells[index(w - 1, h - 1)]);
      assert_eq!(board.distance(far.0, far.1), w - 1 + h - 1, "{shape:?}");
      match shape {
        GridShape::Staggered { .. } => {
          // A diamond of the generator's rectangle, on a map no bigger
          // than holds it
          assert!(size.x <= w + h && size.y <= w + h, "{shape:?}: {size}");
          let lo = cells.iter().fold(IVec2::MAX, |lo, &c| lo.min(c));
          let hi = cells.iter().fold(IVec2::MIN, |hi, &c| hi.max(c));
          assert_eq!((lo, hi), (IVec2::ZERO, size.as_ivec2() - 1), "{shape:?}");
        }
        _ => {
          assert_eq!(size, UVec2::new(w, h), "{shape:?}");
          assert_eq!(cells[index(3, 2)], IVec2::new(3, 2), "{shape:?}: in place");
        }
      }
    }

    // Terrain picks its tiles in the generator's grid, so a staggered map
    // shows each cell the tile the square map shows it
    let set = corner_set(&["floor"]);
    let mut room = GenerationGrid::new(w, h);
    room.set(3, 2, TileType::Floor);
    room.set(4, 2, TileType::Floor);
    let square = room
      .to_terrain_map(
        GridShape::Orthogonal,
        UVec2::new(64, 32),
        tileset(),
        &set,
        floor_of,
      )
      .expect("a square grid makes a map");
    let shape = GridShape::Staggered {
      axis: StaggerAxis::X,
      index: StaggerIndex::Odd,
    };
    let (cells, size) = placed(shape, (w, h));
    let staggered = room
      .to_terrain_map(shape, UVec2::new(64, 32), tileset(), &set, floor_of)
      .expect("a staggered grid makes a map");
    assert_eq!(staggered.size, size);
    for y in 0..h {
      for x in 0..w {
        let cell = cells[index(x, y)];
        assert_eq!(
          tile_at(&staggered, cell.x, cell.y).map(|t| (t.id, t.flip)),
          tile_at(&square, x as i32, y as i32).map(|t| (t.id, t.flip)),
          "({x}, {y}) at {cell}"
        );
      }
    }

    // The generators are square: a hexagonal map has no square lattice
    let hex = GridShape::Hexagonal {
      axis: StaggerAxis::Y,
      index: StaggerIndex::Odd,
      side: 12,
    };
    assert!(
      grid
        .to_tile_map(hex, UVec2::splat(32), tileset(), |_| Some(1))
        .is_none()
    );
    assert!(
      room
        .to_terrain_map(hex, UVec2::splat(32), tileset(), &set, floor_of)
        .is_none()
    );
  }
}
