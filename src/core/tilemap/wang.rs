//! Wang auto-tiling at runtime: painting a terrain colour into a tile layer
//! and letting the cells around it pick the tiles of a Wang set that fit.

use std::collections::HashSet;

use glam::IVec2;

use super::edit::{self, LayerRef};
use super::spawn::{MapRoot, TileLayerData, map_asset};
use super::{Tile, TilesetSource, WangKind, WangSet};
use crate::core::asset::Assets;
use crate::core::ecs::{Entity, Resource, World};
use crate::core::tilemap::Tileset;

/// Where each of a tile's eight positions lies from its centre, clockwise
/// from the top edge
const DIRECTIONS: [IVec2; 8] = [
  IVec2::new(0, -1),
  IVec2::new(1, -1),
  IVec2::new(1, 0),
  IVec2::new(1, 1),
  IVec2::new(0, 1),
  IVec2::new(-1, 1),
  IVec2::new(-1, 0),
  IVec2::new(-1, -1),
];

/// The Wang sets that already warned of a pattern they have no tile for
#[derive(Resource, Default)]
pub(crate) struct WangWarned(pub(crate) HashSet<String>);

impl WangKind {
  /// The positions of a wangid this kind of set colours: corners are the odd
  /// ones, edges the even ones
  fn positions(self) -> &'static [usize] {
    match self {
      WangKind::Corner => &[1, 3, 5, 7],
      WangKind::Edge => &[0, 2, 4, 6],
      WangKind::Mixed => &[0, 1, 2, 3, 4, 5, 6, 7],
    }
  }
}

/// Every cell that shares position `k` of a cell, as its offset from that
/// cell and the position it holds there: the cell itself first, then the
/// one across an edge, or the three around a corner
fn shared(k: usize) -> impl Iterator<Item = (IVec2, usize)> {
  let across = (DIRECTIONS[k], (k + 4) % 8);
  let beside = (k % 2 == 1).then(|| {
    [
      (DIRECTIONS[(k + 7) % 8], (k + 2) % 8),
      (DIRECTIONS[(k + 1) % 8], (k + 6) % 8),
    ]
  });
  std::iter::once((IVec2::ZERO, k))
    .chain(std::iter::once(across))
    .chain(beside.into_iter().flatten())
}

/// A stable value in `[0, 1)` for `cell`: the same cell always draws the same
fn cell_unit(cell: IVec2) -> f64 {
  let mut z =
    (u64::from(cell.x as u32) << 32 | u64::from(cell.y as u32)).wrapping_add(0x9e37_79b9_7f4a_7c15);
  z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
  z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
  z ^= z >> 31;
  (z >> 11) as f64 / (1u64 << 53) as f64
}

impl WangSet {
  /// The 1-based index of the color called `name`, as a wangid holds it
  pub fn color_index(&self, name: &str) -> Option<u8> {
    let at = self.colors.iter().position(|c| c.name == name)?;
    u8::try_from(at + 1).ok()
  }

  /// The tile whose wangid is `wanted` at every position the set's kind
  /// colours (corners, edges, or all eight; 0 is no color). Among equals the
  /// pick weighs each tile by its probability times its colors', and
  /// `cell` seeds it, so the same cell always picks the same tile
  pub fn best_tile(&self, wanted: [u8; 8], cell: IVec2) -> Option<u32> {
    self.pick(wanted, self.kind.positions(), cell)
  }

  /// [`best_tile`](Self::best_tile) over just the `positions`
  fn pick(&self, wanted: [u8; 8], positions: &[usize], cell: IVec2) -> Option<u32> {
    let weight = |tile: &super::WangTile| {
      let colors: f32 = tile
        .wangid
        .iter()
        .filter(|&&c| c != 0)
        .map(|&c| {
          self
            .colors
            .get(usize::from(c) - 1)
            .map_or(1.0, |c| c.probability)
        })
        .product();
      (tile.probability * colors).max(0.0)
    };
    let mut fits = self
      .tiles
      .iter()
      .filter(|t| positions.iter().all(|&k| t.wangid[k] == wanted[k]))
      .peekable();
    let first = *fits.peek()?;
    let fits: Vec<_> = fits.collect();
    let total: f64 = fits.iter().map(|t| f64::from(weight(t))).sum();
    if total <= 0.0 {
      return Some(first.tile);
    }
    let mut left = cell_unit(cell) * total;
    for tile in &fits {
      left -= f64::from(weight(tile));
      if left < 0.0 {
        return Some(tile.tile);
      }
    }
    fits.last().map(|t| t.tile)
  }
}

/// The set called `name` among the map's tilesets, and its tileset's index
fn find_set<'w>(world: &'w World, root: &MapRoot, name: &str) -> Option<(u16, &'w WangSet)> {
  let map = map_asset(world, root)?;
  for (index, source) in map.tilesets.iter().enumerate() {
    let tileset = match source {
      TilesetSource::Asset(handle) => world.get_resource::<Assets<Tileset>>()?.get(handle)?,
      TilesetSource::Embedded(tileset) => tileset,
    };
    if let Some(set) = tileset.wang_sets.iter().find(|s| s.name == name) {
      return Some((u16::try_from(index).ok()?, set));
    }
  }
  None
}

/// What painting works out before it edits
struct Plan {
  tiles: Vec<(IVec2, Tile)>,
  /// A cell had no tile to fit its colours
  missed: bool,
}

/// The tiles the cells around `cell` take when it is painted `color` (0 for
/// none): its corners and/or edges (by the set's kind) take the color and
/// every cell sharing one picks again, wanting the painted color where it
/// shares a position with `cell` and what its neighbours outside the painted
/// reach have, else what the cell has. Cells holding no tile of the set are
/// left alone
fn plan(
  world: &World,
  layer: Entity,
  (index, set): (u16, &WangSet),
  cell: IVec2,
  color: u8,
) -> Option<Plan> {
  let grid = &world.get::<TileLayerData>(layer)?.grid;
  let wangid_of = |at: IVec2| -> Option<[u8; 8]> {
    let tile = grid.get(at).filter(|t| t.tileset == index)?;
    set
      .tiles
      .iter()
      .find(|t| t.tile == tile.id)
      .map(|t| t.wangid)
  };
  let positions = set.kind.positions();
  // Every cell touching a painted position, with what the paint says of it
  let mut forced: Vec<(IVec2, [Option<u8>; 8])> = Vec::new();
  for &k in positions {
    for (offset, at) in shared(k) {
      let target = cell + offset;
      let slot = match forced.iter().position(|(c, _)| *c == target) {
        Some(slot) => slot,
        None => {
          forced.push((target, [None; 8]));
          forced.len() - 1
        }
      };
      forced[slot].1[at] = Some(color);
    }
  }
  let region: HashSet<IVec2> = forced.iter().map(|(c, _)| *c).collect();
  let mut plan = Plan {
    tiles: Vec::new(),
    missed: false,
  };
  for (at, paint) in &forced {
    let own = wangid_of(*at);
    if *at != cell && own.is_none() {
      continue;
    }
    let mut wanted = [0u8; 8];
    for &k in positions {
      wanted[k] = paint[k]
        .or_else(|| {
          shared(k)
            .skip(1)
            .filter(|(offset, _)| !region.contains(&(*at + *offset)))
            .find_map(|(offset, pos)| wangid_of(*at + offset).map(|w| w[pos]))
        })
        .or(own.map(|w| w[k]))
        .unwrap_or(0);
    }
    match set.pick(wanted, positions, *at) {
      Some(id) => plan.tiles.push((*at, Tile::new(index, id))),
      None => plan.missed = true,
    }
  }
  Some(plan)
}

/// Paints `cell` of a tile layer with the Wang color `color` (`None` for no
/// color) of the set `wang_set`, then has the cells it touches pick the tiles
/// that fit; see [`MapsView::set_terrain`](super::MapsView::set_terrain)
pub(crate) fn set_terrain(
  world: &mut World,
  map: Entity,
  layer: LayerRef<'_>,
  cell: IVec2,
  wang_set: &str,
  color: Option<&str>,
) -> bool {
  let Some(entity) = edit::layer_entity(world, map, layer) else {
    return false;
  };
  let Some(size) = world
    .get::<TileLayerData>(entity)
    .map(|data| data.grid.size().as_ivec2())
  else {
    return false;
  };
  if cell.cmplt(IVec2::ZERO).any() || cell.cmpge(size).any() {
    return false;
  }
  let planned = {
    let Some(root) = world.get::<MapRoot>(map) else {
      return false;
    };
    let Some(found) = find_set(world, root, wang_set) else {
      return false;
    };
    let index = match color {
      None => 0,
      Some(name) => match found.1.color_index(name) {
        Some(index) => index,
        None => return false,
      },
    };
    plan(world, entity, found, cell, index)
  };
  let Some(planned) = planned else {
    return false;
  };
  if planned.missed {
    world.init_resource::<WangWarned>();
    let first = world
      .get_resource_mut::<WangWarned>()
      .is_some_and(|mut warned| warned.0.insert(wang_set.to_string()));
    if first {
      crate::engine_warn!(
        module: "Tilemap",
        "map {map}: the Wang set {wang_set:?} has no tile for a pattern that painting {color:?} at {cell} asks for, so those cells keep their tiles (this set warns once)"
      );
    }
  }
  let mut changed = false;
  for (at, tile) in planned.tiles {
    changed |= edit::set_tile(world, map, layer, at, Some(tile));
  }
  changed
}

#[cfg(test)]
mod tests {
  use glam::IVec2;

  use super::WangWarned;
  use crate::core::asset::{Assets, AssetsView};
  use crate::core::ecs::{Entity, World};
  use crate::core::tilemap::test_util::{MapWorld, load_map};
  use crate::core::tilemap::{MapRoot, MapsView, Tile, TileMap, Tileset, WangSet};

  /// The 5x5 map of all-dirt tiles (id 15 of the ground set), landed
  fn dirt_map(mw: &mut MapWorld) -> Entity {
    let handle = load_map(&mut mw.world, "tiled/maps/wang-dirt.tmj");
    mw.spawn(MapRoot::new(handle))
  }

  fn tile_ids(world: &mut World, map: Entity) -> Vec<Vec<Option<u32>>> {
    let maps = MapsView::new(world);
    (0..5)
      .map(|y| {
        (0..5)
          .map(|x| maps.tile(map, "ground", IVec2::new(x, y)).map(|t| t.id))
          .collect()
      })
      .collect()
  }

  fn wang_set(world: &mut World, name: &str) -> WangSet {
    let handle = AssetsView::new(world)
      .load_now::<Tileset>("tiled/tilesets/wang.tsj")
      .expect("loads");
    let sets = world.resource::<Assets<Tileset>>();
    match sets
      .get(&handle)
      .and_then(|t| t.wang_sets.iter().find(|s| s.name == name))
    {
      Some(set) => set.clone(),
      None => panic!("no wang set {name}"),
    }
  }

  #[test]
  fn painting_grass_into_dirt_picks_edge_and_corner_tiles() {
    let mut mw = MapWorld::new();
    let map = dirt_map(&mut mw);
    // painted by Tiled's own brush (scripts/paint-wang.js)
    let handle = load_map(&mut mw.world, "tiled/maps/wang-grass.tmj");
    let expected: Vec<Vec<Option<u32>>> = {
      let asset = mw.world.resource::<Assets<TileMap>>().get(&handle);
      let Some(grid) = asset.and_then(|m| m.tiles(0)) else {
        panic!("no expected layer")
      };
      (0..5)
        .map(|y| {
          (0..5)
            .map(|x| grid.get(IVec2::new(x, y)).map(|t| t.id))
            .collect()
        })
        .collect()
    };
    let mut maps = MapsView::new(&mut mw.world);
    assert!(maps.set_terrain(map, "ground", IVec2::new(2, 2), "ground", Some("grass")));
    assert_eq!(tile_ids(&mut mw.world, map), expected);
    assert_eq!(expected[2][2], Some(0), "all grass");
    // the same paint again changes nothing
    let mut maps = MapsView::new(&mut mw.world);
    assert!(!maps.set_terrain(map, "ground", IVec2::new(2, 2), "ground", Some("grass")));
  }

  #[test]
  fn an_edge_set_and_a_mixed_set_pick_their_tiles() {
    let mut mw = MapWorld::new();
    let map = dirt_map(&mut mw);
    let all = (IVec2::ZERO, IVec2::splat(4));

    // edges: brick at (2, 2) of a layer of plank; only the four cells across
    // an edge pick again, the diagonals share none
    let mut maps = MapsView::new(&mut mw.world);
    maps.fill(map, "ground", all.0, all.1, Some(Tile::new(0, 15)));
    assert!(maps.set_terrain(map, "ground", IVec2::new(2, 2), "walls", Some("brick")));
    let ids = tile_ids(&mut mw.world, map);
    let rows: Vec<Vec<u32>> = ids
      .iter()
      .map(|r| r.iter().flatten().copied().collect())
      .collect();
    assert_eq!(rows[2][2], 0, "brick on every edge");
    assert_eq!(rows[1][2], 13, "above: its bottom edge is brick");
    assert_eq!(rows[2][3], 14, "right: its left edge");
    assert_eq!(rows[3][2], 7, "below: its top edge");
    assert_eq!(rows[2][1], 11, "left: its right edge");
    for (x, y) in [(1, 1), (3, 1), (1, 3), (3, 3)] {
      assert_eq!(rows[y][x], 15, "diagonal ({x}, {y}) is unchanged");
    }

    // mixed: b at (2, 2) of a layer of a (tile 0); the set has the
    // transitions of the cells to the left and right only
    let mut maps = MapsView::new(&mut mw.world);
    maps.fill(map, "ground", all.0, all.1, Some(Tile::new(0, 0)));
    assert!(maps.set_terrain(map, "ground", IVec2::new(2, 2), "mix", Some("b")));
    let ids = tile_ids(&mut mw.world, map);
    assert_eq!(ids[2][2], Some(1), "all b");
    assert_eq!(ids[2][3], Some(2), "right: b along its left");
    assert_eq!(ids[2][1], Some(3), "left: b along its right");
    assert_eq!(
      ids[1][2],
      Some(0),
      "above has no transition and keeps its tile"
    );
  }

  #[test]
  fn ties_pick_by_probability_and_are_stable() {
    let mut mw = MapWorld::new();
    let tied = wang_set(&mut mw.world, "tied");
    assert_eq!(
      tied.tiles.iter().map(|t| t.probability).collect::<Vec<_>>(),
      [1.0, 3.0],
      "the tiles' probabilities came with the set"
    );
    let wanted = [1, 0, 1, 0, 1, 0, 1, 0];
    let pick_all = || -> Vec<u32> {
      (0..400)
        .map(|i| {
          tied
            .best_tile(wanted, IVec2::new(i % 20, i / 20))
            .unwrap_or(u32::MAX)
        })
        .collect()
    };
    let picks = pick_all();
    assert!(picks.iter().all(|&t| t == 4 || t == 5));
    let heavy = picks.iter().filter(|&&t| t == 5).count();
    assert!(
      (270..=330).contains(&heavy),
      "a tile three times as likely takes about 300 of 400, took {heavy}"
    );
    assert_eq!(picks, pick_all(), "the same cell picks the same tile");
    assert_eq!(tied.color_index("x"), Some(1));
    assert_eq!(tied.color_index("nope"), None);

    // the same edits to two maps give the same tiles
    let paint = |mw: &mut MapWorld| {
      let map = dirt_map(mw);
      let mut maps = MapsView::new(&mut mw.world);
      for x in 0..5 {
        for y in 0..5 {
          maps.set_terrain(map, "ground", IVec2::new(x, y), "tied", Some("x"));
        }
      }
      tile_ids(&mut mw.world, map)
    };
    let (first, second) = (paint(&mut mw), paint(&mut mw));
    assert_eq!(first, second);
    let flat: Vec<u32> = first.iter().flatten().flatten().copied().collect();
    assert!(
      flat.contains(&4) && flat.contains(&5),
      "both tiles are used"
    );
  }

  #[test]
  fn an_unmatched_pattern_keeps_the_tile_and_warns_once() {
    let mut mw = MapWorld::new();
    let map = dirt_map(&mut mw);
    let before = tile_ids(&mut mw.world, map);
    let mut maps = MapsView::new(&mut mw.world);
    // color c has no tile in the mixed set
    assert!(!maps.set_terrain(map, "ground", IVec2::new(2, 2), "mix", Some("c")));
    assert!(!maps.set_terrain(map, "ground", IVec2::new(1, 1), "mix", Some("c")));
    assert_eq!(tile_ids(&mut mw.world, map), before);
    let warned = mw.world.resource::<WangWarned>();
    assert_eq!(warned.0.len(), 1, "one warning for the set");
    assert!(warned.0.contains("mix"));

    // a set, color, layer or cell that is not there changes nothing and does
    // not warn
    let mut maps = MapsView::new(&mut mw.world);
    assert!(!maps.set_terrain(map, "ground", IVec2::new(2, 2), "nope", Some("a")));
    assert!(!maps.set_terrain(map, "ground", IVec2::new(2, 2), "mix", Some("nope")));
    assert!(!maps.set_terrain(map, "nope", IVec2::new(2, 2), "mix", Some("a")));
    assert!(!maps.set_terrain(map, "ground", IVec2::new(9, 9), "mix", Some("a")));
    assert_eq!(mw.world.resource::<WangWarned>().0.len(), 1);
  }
}
