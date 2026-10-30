//! Grids as tile maps: a generated level that draws, collides and edits like
//! a designer's map.

use std::collections::HashSet;

use ryo2d::core::dev_log::{self, LogContext, LogLevel};
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

fn warn_missing_color(name: &str, set: &str) {
  dev_log::log(
    LogLevel::Warn,
    LogContext::Game,
    Some("procgen"),
    format!("Wang set \"{set}\" has no colour \"{name}\"; its cells stay empty"),
  );
}

impl GenerationGrid {
  /// The grid as a [`TileMap`] of one tile layer, `"tiles"`, as big as the
  /// grid with `tile_size` cells and `tileset` as its only tileset. Cell
  /// `(x, y)` takes the tile `tile_of` gives its type, an id in the tileset;
  /// a type it gives `None` is left empty. Row 0 is the map's top row.
  pub fn to_tile_map(
    &self,
    tile_size: UVec2,
    tileset: TilesetSource,
    tile_of: impl Fn(TileType) -> Option<u32>,
  ) -> TileMap {
    let (mut map, layer) = self.empty_map(tile_size, tileset);
    if let Some(tiles) = map.tiles_mut(layer) {
      for (x, y, kind) in self.iter() {
        if let Some(id) = tile_of(kind) {
          tiles.set(IVec2::new(x, y), Some(Tile::new(0, id)));
        }
      }
    }
    map
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
  /// A cell the set has no tile for (see [`WangSet::best_tile`]) is left
  /// empty.
  pub fn to_terrain_map<'a>(
    &self,
    tile_size: UVec2,
    tileset: TilesetSource,
    wang_set: &WangSet,
    color_of: impl Fn(TileType) -> Option<&'a str>,
  ) -> TileMap {
    let (mut map, layer) = self.empty_map(tile_size, tileset);
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
      return map;
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
        let cell = IVec2::new(x, y);
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
    map
  }

  /// A map with the grid's size, the one tileset and an empty `"tiles"` layer
  fn empty_map(&self, tile_size: UVec2, tileset: TilesetSource) -> (TileMap, usize) {
    let mut map = TileMap::new(UVec2::new(self.width(), self.height()), tile_size);
    map.add_tileset(tileset);
    let layer = map.add_tile_layer("tiles");
    (map, layer)
  }
}

#[cfg(test)]
mod tests {
  use crate::prelude::*;
  use ryo2d::core::dev_log::LogLevel;
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
    let map = grid.to_tile_map(UVec2::splat(16), tileset(), |t| match t {
      TileType::Wall => Some(7),
      TileType::Floor => Some(2),
      _ => None,
    });
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
    let map = grid.to_terrain_map(UVec2::splat(16), tileset(), &set, |t| {
      (t == TileType::Floor).then_some("floor")
    });
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
    let map = grid.to_terrain_map(UVec2::splat(16), tileset(), &set, |t| match t {
      TileType::Water => Some("water"),
      TileType::Floor => Some("floor"),
      _ => None,
    });
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
    let map = grid.to_terrain_map(UVec2::splat(16), tileset(), &set, |_| Some("lava"));
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
    let map = room(5).to_terrain_map(UVec2::splat(16), tileset(), &set, floor_of);
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
    let map = room(5).to_terrain_map(UVec2::splat(16), tileset(), &set, floor_of);
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
    let map = room(5).to_terrain_map(UVec2::splat(16), tileset(), &set, floor_of);
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
    let set = corner_set(&["floor"]);
    let grid = GenerationGrid::filled(3, 3, TileType::Floor);
    let warnings = || {
      ryo2d::core::dev_log::get_entries()
        .iter()
        .filter(|e| e.level == LogLevel::Warn && e.message.contains("lava-warn-once"))
        .count()
    };
    let before = warnings();
    grid.to_terrain_map(UVec2::splat(16), tileset(), &set, |_| {
      Some("lava-warn-once")
    });
    assert_eq!(warnings(), before + 1, "nine cells, one warning");
    grid.to_terrain_map(UVec2::splat(16), tileset(), &set, |_| {
      Some("lava-warn-once")
    });
    assert_eq!(warnings(), before + 2, "another conversion warns again");
  }
}
