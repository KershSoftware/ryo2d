//! Wang auto-tiling at runtime: painting a terrain colour into a tile layer
//! and letting the cells around it pick the tiles of a Wang set that fit, as
//! Tiled's terrain brush does (corrections on).
//!
//! What a paint does, in the order of Tiled's results:
//!
//! - The paint asks one color of the positions the set's kind colors (a
//!   cell's corners, edges or both) and every cell sharing one of them
//!   picks again, in rows and columns (the painted cell is not first). A pick must have the
//!   color asked and what the cells that picked before it show beside it,
//!   and among those fits is the one closest to what the cell shows now;
//!   closeness sums the distances between colors, which are steps through
//!   tiles that hold both (`Matcher`).
//! - No color is color 0, and the empty cell is a candidate with only 0.
//! - A cell nothing fits keeps what it has.
//! - A pick that changes a cell passes the change on to the neighbours
//!   across an edge whose colors no longer agree, within a square around
//!   the painted cell as many cells out as the set's longest color chain
//!   plus one (two colors that share a tile are a chain of one: two cells
//!   out; deep water, water, sand, grass: four).
//! - Flips: a cell is read through its flip, and the tileset's
//!   transformations add flipped and turned candidates.
//!
//! Where Tiled draws at random among equals, this draws by a hash of the
//! cell, so the same edits make the same tiles.

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::OnceLock;

use glam::IVec2;

use super::edit::{self, LayerRef};
use super::spawn::{LayerTiles, MapRoot, MapTile, TileLayerData, map_asset};
use super::{
  Tile, TileFlip, TileTransformations, TilesetSource, WangColor, WangKind, WangSet, WangTile,
};
use crate::core::asset::Assets;
use crate::core::ecs::{Entity, Resource, World};
use crate::core::physics::CellRect;
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

/// The position each position swaps with under the diagonal, horizontal and
/// vertical flips
const DIAGONAL: [usize; 8] = [6, 5, 4, 3, 2, 1, 0, 7];
const HORIZONTAL: [usize; 8] = [0, 7, 6, 5, 4, 3, 2, 1];
const VERTICAL: [usize; 8] = [4, 3, 2, 1, 0, 7, 6, 5];

/// The Wang sets that warned of a cell they have no tile for, by tileset name
/// and set name: the tileset's content version then, and how many warnings
/// went out. A tileset that changed may warn again
#[derive(Resource, Default)]
pub(crate) struct WangWarned(pub(crate) HashMap<(String, String), (u64, u32)>);

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

/// `wangid` as a cell with `flip` shows it: the diagonal flip first, then the
/// horizontal, then the vertical
fn flipped(wangid: [u8; 8], flip: TileFlip) -> [u8; 8] {
  let mut id = wangid;
  for (flag, swap) in [
    (TileFlip::DIAGONAL, DIAGONAL),
    (TileFlip::HORIZONTAL, HORIZONTAL),
    (TileFlip::VERTICAL, VERTICAL),
  ] {
    if flip.contains(flag) {
      let before = id;
      id = std::array::from_fn(|i| before[swap[i]]);
    }
  }
  id
}

/// The flips a set may give its tiles: those whose turn of the positions
/// lies in the group the allowed mirrors and quarter turn generate
fn allowed_flips(allowed: TileTransformations) -> Vec<TileFlip> {
  const POSITIONS: [u8; 8] = [0, 1, 2, 3, 4, 5, 6, 7];
  let turn = |flip: TileFlip| flipped(POSITIONS, flip);
  let mut generators = Vec::new();
  if allowed.hflip {
    generators.push(turn(TileFlip::HORIZONTAL));
  }
  if allowed.vflip {
    generators.push(turn(TileFlip::VERTICAL));
  }
  if allowed.rotate {
    generators.push(turn(TileFlip::DIAGONAL | TileFlip::HORIZONTAL));
  }
  let mut group = vec![POSITIONS];
  let mut next = 0;
  while next < group.len() {
    let from = group[next];
    next += 1;
    for g in &generators {
      let moved: [u8; 8] = std::array::from_fn(|i| from[usize::from(g[i])]);
      if !group.contains(&moved) {
        group.push(moved);
      }
    }
  }
  // The turns come before the mirrors, so of two flips that show the same
  // colors the turn is the one a cell gets
  let turns = [
    TileFlip::NONE,
    TileFlip::DIAGONAL | TileFlip::HORIZONTAL,
    TileFlip::HORIZONTAL | TileFlip::VERTICAL,
    TileFlip::VERTICAL | TileFlip::DIAGONAL,
    TileFlip::HORIZONTAL,
    TileFlip::VERTICAL,
    TileFlip::DIAGONAL,
    TileFlip::HORIZONTAL | TileFlip::VERTICAL | TileFlip::DIAGONAL,
  ];
  turns
    .into_iter()
    .filter(|&flip| group.contains(&turn(flip)))
    .collect()
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

/// What a cell can hold in a set: a tile at a flip, or nothing
struct Candidate {
  tile: Option<u32>,
  flip: TileFlip,
  wangid: [u8; 8],
  /// Tile probability times its colors'
  weight: f64,
  transformed: bool,
}

/// A set's candidates and how far apart its colors are
struct Matcher {
  candidates: Vec<Candidate>,
  /// Steps between colors 0 (none) to n, through tiles that hold both; `None`
  /// where no chain of tiles joins them
  distance: Vec<Vec<Option<u32>>>,
  positions: &'static [usize],
  prefer_untransformed: bool,
  /// Each tile's wangid, as the set lists it
  ids: HashMap<u32, [u8; 8]>,
  /// The largest distance between two colors a chain of tiles joins
  longest_chain: i32,
}

/// What a set works out once from its tiles, colors and transformations: its
/// candidates, how far apart its colors are and its tiles' wangids. Built the
/// first time it is asked for, so a set made in code gets it too, and a
/// tileset that loads again brings a new one with its sets. The set's fields
/// are read once built: change them through a new `WangSet`.
#[derive(Default)]
pub(crate) struct WangCache(OnceLock<Matcher>);

impl Clone for WangCache {
  fn clone(&self) -> Self {
    WangCache::default()
  }
}

impl std::fmt::Debug for WangCache {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    f.write_str("WangCache")
  }
}

impl Matcher {
  fn new(set: &WangSet) -> Matcher {
    let positions = set.kind.positions();
    let colors = set.colors.len() + 1;
    let probability = |color: u8| {
      usize::from(color)
        .checked_sub(1)
        .and_then(|i| set.colors.get(i))
        .map_or(1.0, |c| f64::from(c.probability))
    };
    let mut distance = vec![vec![None; colors]; colors];
    for (i, row) in distance.iter_mut().enumerate() {
      row[i] = Some(0);
    }
    let flips = allowed_flips(set.transformations);
    let mut candidates = Vec::new();
    for tile in &set.tiles {
      // A tile with no color at all is the empty cell's twin, and loses to it
      if tile.wangid.iter().all(|&c| c == 0) {
        continue;
      }
      for &a in positions {
        for &b in positions {
          let (a, b) = (usize::from(tile.wangid[a]), usize::from(tile.wangid[b]));
          if a != b && a < colors && b < colors {
            distance[a][b] = Some(1);
          }
        }
      }
      let weight = f64::from(tile.probability).max(0.0)
        * tile
          .wangid
          .iter()
          .filter(|&&c| c != 0)
          .map(|&c| probability(c))
          .product::<f64>();
      let first = candidates.len();
      for &flip in &flips {
        let wangid = flipped(tile.wangid, flip);
        if candidates[first..]
          .iter()
          .any(|c: &Candidate| c.wangid == wangid)
        {
          continue;
        }
        candidates.push(Candidate {
          tile: Some(tile.tile),
          flip,
          wangid,
          weight,
          transformed: flip != TileFlip::NONE,
        });
      }
    }
    candidates.push(Candidate {
      tile: None,
      flip: TileFlip::NONE,
      wangid: [0; 8],
      weight: 1.0,
      transformed: false,
    });
    for via in 0..colors {
      for from in 0..colors {
        for to in 0..colors {
          if let (Some(a), Some(b)) = (distance[from][via], distance[via][to])
            && distance[from][to].is_none_or(|d| a + b < d)
          {
            distance[from][to] = Some(a + b);
          }
        }
      }
    }
    let longest_chain = distance
      .iter()
      .flatten()
      .flatten()
      .map(|&d| i32::try_from(d).unwrap_or(i32::MAX))
      .max()
      .unwrap_or(0);
    Matcher {
      candidates,
      distance,
      positions,
      prefer_untransformed: set.transformations.prefer_untransformed,
      ids: set.tiles.iter().rev().map(|t| (t.tile, t.wangid)).collect(),
      longest_chain,
    }
  }

  /// How far `candidate` is from `wanted` (ANY positions count for nothing):
  /// the sum of its colors' distances; `None` if some color cannot be
  /// reached from the wanted one
  fn penalty(&self, candidate: &Candidate, wanted: &[u8; 8]) -> Option<u32> {
    let mut total = 0u32;
    for &k in self.positions {
      if wanted[k] == WangSet::ANY {
        continue;
      }
      let step = self
        .distance
        .get(usize::from(wanted[k]))?
        .get(usize::from(candidate.wangid[k]))
        .copied()??;
      total = total.saturating_add(step);
    }
    Some(total)
  }

  /// The candidate closest to `hard` (counted first) and then to `soft`, or
  /// with `exact` one that has `hard` exactly and is closest to `soft`;
  /// among equals preferring an untransformed tile if the set says so, and
  /// then weighed by probability and drawn by `cell`'s hash
  fn pick(&self, hard: &[u8; 8], soft: &[u8; 8], cell: IVec2, exact: bool) -> Option<&Candidate> {
    let mut best: Vec<&Candidate> = Vec::new();
    let mut least = None;
    for candidate in &self.candidates {
      let (Some(h), Some(s)) = (self.penalty(candidate, hard), self.penalty(candidate, soft))
      else {
        continue;
      };
      if exact && h != 0 {
        continue;
      }
      match least {
        Some(l) if (h, s) > l => {}
        Some(l) if (h, s) == l => best.push(candidate),
        _ => {
          least = Some((h, s));
          best.clear();
          best.push(candidate);
        }
      }
    }
    if self.prefer_untransformed && best.iter().any(|c| !c.transformed) {
      best.retain(|c| !c.transformed);
    }
    let total: f64 = best.iter().map(|c| c.weight).sum();
    if total <= 0.0 {
      return best.first().copied();
    }
    let mut left = cell_unit(cell) * total;
    for candidate in &best {
      left -= candidate.weight;
      if left < 0.0 {
        return Some(candidate);
      }
    }
    best.last().copied()
  }
}

impl WangSet {
  /// A set of `kind` over `colors` and `tiles`, with the tileset's
  /// `transformations`. A set is read-only once built, so the candidates the
  /// first pick works out stay true
  pub fn new(
    name: impl Into<String>,
    kind: WangKind,
    colors: Vec<WangColor>,
    tiles: Vec<WangTile>,
    transformations: TileTransformations,
  ) -> WangSet {
    WangSet {
      name: name.into(),
      kind,
      colors,
      tiles,
      transformations,
      cache: WangCache::default(),
    }
  }

  /// The set's candidates and distances, built on the first call
  fn matcher(&self) -> &Matcher {
    self.cache.0.get_or_init(|| Matcher::new(self))
  }

  /// In a wanted wangid: any color goes at this position
  pub const ANY: u8 = u8::MAX;

  /// The set's name
  pub fn name(&self) -> &str {
    &self.name
  }

  /// Which parts of a tile the set's colors touch
  pub fn kind(&self) -> WangKind {
    self.kind
  }

  /// The set's colors; a wangid's color indices count from 1 into this
  pub fn colors(&self) -> &[WangColor] {
    &self.colors
  }

  /// The set's tiles with their wangids and probabilities
  pub fn tiles(&self) -> &[WangTile] {
    &self.tiles
  }

  /// What the set's tileset lets it do to a tile
  pub fn transformations(&self) -> TileTransformations {
    self.transformations
  }

  /// The 1-based index of the color called `name`, as a wangid holds it
  pub fn color_index(&self, name: &str) -> Option<u8> {
    let at = self.colors.iter().position(|c| c.name == name)?;
    u8::try_from(at + 1).ok()
  }

  /// The tile, and the flip it takes, whose wangid is closest to `wanted`
  /// at the positions the set's kind colors (corners, edges, or all eight).
  /// A wanted position holds a color index, 0 for none, or [`ANY`](Self::ANY).
  /// Closeness is Tiled's: the colors' distances summed, a color no chain of
  /// tiles reaches rules a tile out, the set's flips widen the choice and an
  /// untransformed tile wins a tie if the set prefers it. Among equals the
  /// pick weighs each tile by its probability times its colors' and `cell`
  /// seeds it, so the same cell always picks the same tile. `None` if
  /// nothing fits, or if the closest is no tile at all (an empty cell fits
  /// an all-0 `wanted` best)
  pub fn best_tile(&self, wanted: [u8; 8], cell: IVec2) -> Option<(u32, TileFlip)> {
    let candidate = self.matcher().pick(&wanted, &[Self::ANY; 8], cell, false)?;
    Some((candidate.tile?, candidate.flip))
  }
}

/// The set called `name` among the map's tilesets, its tileset's index in
/// the map, and the tileset's name and content version
fn find_set<'w>(
  world: &'w World,
  root: &MapRoot,
  name: &str,
) -> Option<(u16, &'w WangSet, (String, u64))> {
  let map = map_asset(world, root)?;
  for (index, source) in map.tilesets.iter().enumerate() {
    let (tileset, version) = match source {
      TilesetSource::Asset(handle) => {
        let Some(assets) = world.get_resource::<Assets<Tileset>>() else {
          continue;
        };
        let Some(tileset) = assets.get(handle) else {
          continue;
        };
        (tileset, assets.version(handle).unwrap_or(0))
      }
      TilesetSource::Embedded(tileset) => (&**tileset, 0),
    };
    if let Some(set) = tileset.wang_sets.iter().find(|s| s.name == name) {
      return Some((
        u16::try_from(index).ok()?,
        set,
        (tileset.name.clone(), version),
      ));
    }
  }
  None
}

/// What painting works out before it edits
struct Plan {
  edits: Vec<(IVec2, Option<Tile>)>,
  /// A cell holding a tile of the set had nothing to fit its colors
  missed: bool,
}

/// One painting: the cells it has settled and the ones waiting
struct Painting<'w> {
  world: &'w World,
  layer: Entity,
  index: u16,
  /// The cells the layer's grid holds: a bounded grid's bounds, `None`
  /// for an unbounded one, which holds any cell (a paint there makes the
  /// chunks it writes in, as `set_tile` does)
  bounds: Option<CellRect>,
  matcher: &'w Matcher,
  /// The colors the painting asks of each cell touching it (`ANY` where it
  /// asks none)
  paint: HashMap<IVec2, [u8; 8]>,
  /// The cell painted
  painted: IVec2,
  decided: HashMap<IVec2, [u8; 8]>,
  pending: HashSet<IVec2>,
  /// Cells that had nothing to fit: left as they are, and not asked again
  failed: HashSet<IVec2>,
  queue: VecDeque<IVec2>,
  /// The cells a change may reach
  reach: HashSet<IVec2>,
  plan: Plan,
}

impl Painting<'_> {
  fn in_bounds(&self, at: IVec2) -> bool {
    self.bounds.is_none_or(|bounds| bounds.contains(at))
  }

  /// A class tile lives on the cell: not ours to change
  fn locked(&self, at: IVec2) -> bool {
    self
      .world
      .get::<LayerTiles>(self.layer)
      .and_then(|table| table.0.get(&at))
      .is_some_and(|&entity| {
        self
          .world
          .get::<MapTile>(entity)
          .is_some_and(|t| t.cell == at)
      })
  }

  fn tile_at(&self, at: IVec2) -> Option<Tile> {
    self.world.get::<TileLayerData>(self.layer)?.tiles.get(at)
  }

  /// The colors the grid shows at `at`, through the cell's flips; zeros for an
  /// empty cell and for a tile outside the set
  fn colors_at(&self, at: IVec2) -> [u8; 8] {
    match self.tile_at(at) {
      Some(tile) if tile.tileset == self.index => self
        .matcher
        .ids
        .get(&tile.id)
        .map_or([0; 8], |&id| flipped(id, tile.flip)),
      _ => [0; 8],
    }
  }

  fn holds_set_tile(&self, at: IVec2) -> bool {
    self
      .tile_at(at)
      .is_some_and(|t| t.tileset == self.index && self.matcher.ids.contains_key(&t.id))
  }

  /// Settles `at` on `candidate`, and queues the neighbours whose colors at a
  /// shared position now differ
  fn place(&mut self, at: IVec2, candidate: usize) {
    let (tile, flip, wangid) = {
      let c = &self.matcher.candidates[candidate];
      (c.tile, c.flip, c.wangid)
    };
    let tile = tile.map(|id| Tile::new(self.index, id).with_flip(flip));
    if self.tile_at(at) != tile {
      self.plan.edits.push((at, tile));
    }
    self.decided.insert(at, wangid);
    for &k in self.matcher.positions {
      for (offset, pos) in shared(k).skip(1) {
        let other = at + offset;
        if offset.abs().element_sum() != 1
          || !self.in_bounds(other)
          || !self.reach.contains(&other)
          || self.decided.contains_key(&other)
          || self.pending.contains(&other)
          || (self.failed.contains(&other) && self.paint.contains_key(&other))
          || self.locked(other)
        {
          continue;
        }
        if self.colors_at(other)[pos] != wangid[k] {
          self.pending.insert(other);
          self.queue.push_back(other);
        }
      }
    }
  }

  /// The index of the candidate with the `hard` colors that is closest to
  /// the `soft` ones
  fn choose(&self, at: IVec2, hard: &[u8; 8], soft: &[u8; 8]) -> Option<usize> {
    let picked = self.matcher.pick(hard, soft, at, true)?;
    self
      .matcher
      .candidates
      .iter()
      .position(|c| std::ptr::eq(c, picked))
  }

  /// Settles the next waiting cell: the color painted and what the cells that
  /// settled beside it show are a must, what the cell shows otherwise a wish
  fn settle_next(&mut self) -> bool {
    let Some(at) = self.queue.pop_front() else {
      return false;
    };
    self.pending.remove(&at);
    let own = self.colors_at(at);
    let mut hard = self.paint.get(&at).copied().unwrap_or([WangSet::ANY; 8]);
    let mut soft = [WangSet::ANY; 8];
    for &k in self.matcher.positions {
      if hard[k] == WangSet::ANY {
        for (offset, pos) in shared(k).skip(1) {
          if let Some(wangid) = self.decided.get(&(at + offset)) {
            hard[k] = wangid[pos];
            break;
          }
        }
      }
      if hard[k] == WangSet::ANY {
        soft[k] = own[k];
      }
    }
    match self.choose(at, &hard, &soft) {
      Some(candidate) => self.place(at, candidate),
      None => {
        self.failed.insert(at);
        self.plan.missed |= at == self.painted || self.holds_set_tile(at);
      }
    }
    true
  }
}

/// The cells a paint at `cell` may change: the square as many cells out as
/// the set's colors chain, plus one (a chain of one step, two colors that
/// share a tile, reaches two cells; each further step reaches one more)
fn reach(cell: IVec2, longest_chain: i32) -> HashSet<IVec2> {
  let out = longest_chain.max(1) + 1;
  (-out..=out)
    .flat_map(|y| (-out..=out).map(move |x| cell + IVec2::new(x, y)))
    .collect()
}

/// Whether every cell a paint at `cell` reads lies in `i32`: its `reach`,
/// and the neighbours a cell at the reach's edge looks at
fn reach_fits(cell: IVec2, longest_chain: i32) -> bool {
  let margin = longest_chain.max(1).saturating_add(2);
  let fits = |v: i32| v.checked_sub(margin).is_some() && v.checked_add(margin).is_some();
  fits(cell.x) && fits(cell.y)
}

/// The tiles the cells around `cell` take when it is painted `color` (0 for
/// none). Each cell sharing a painted position settles in rows and columns
/// (the painted cell too) on the tile, or none, that has the color asked and
/// what the cells that settled beside it show, closest to what the cell
/// shows now; a cell that changed has the neighbours across an edge whose
/// colors now differ settle too, within `reach`, and so on until the colors
/// agree or nothing fits. `None` for a cell whose reach leaves `i32` (an
/// unbounded layer's cell near its ends)
fn plan(
  world: &World,
  layer: Entity,
  (index, set): (u16, &WangSet),
  cell: IVec2,
  color: u8,
) -> Option<Plan> {
  if !reach_fits(cell, set.matcher().longest_chain) {
    return None;
  }
  let bounds = world.get::<TileLayerData>(layer)?.tiles.bounds();
  let mut painting = Painting {
    world,
    layer,
    index,
    bounds,
    matcher: set.matcher(),
    paint: HashMap::new(),
    painted: cell,
    decided: HashMap::new(),
    pending: HashSet::new(),
    failed: HashSet::new(),
    queue: VecDeque::new(),
    reach: reach(cell, set.matcher().longest_chain),
    plan: Plan {
      edits: Vec::new(),
      missed: false,
    },
  };
  let mut touching = vec![cell];
  for &k in set.kind.positions() {
    for (offset, pos) in shared(k) {
      let at = cell + offset;
      painting.paint.entry(at).or_insert([WangSet::ANY; 8])[pos] = color;
      if !touching.contains(&at) {
        touching.push(at);
      }
    }
  }
  touching.sort_by_key(|at| (at.y, at.x));
  for at in touching {
    if painting.in_bounds(at) && (at == cell || !painting.locked(at)) {
      painting.pending.insert(at);
      painting.queue.push_back(at);
    }
  }
  while painting.settle_next() {}
  Some(painting.plan)
}

/// Whether the set `key` (tileset name, set name) has yet to warn at the
/// tileset's content `version`; counts the warning if so
fn warn_first(warned: &mut WangWarned, key: (String, String), version: u64) -> bool {
  let entry = warned.0.entry(key).or_insert((version, 0));
  let first = entry.1 == 0 || entry.0 != version;
  if first {
    *entry = (version, entry.1 + 1);
  }
  first
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
  let Some(bounds) = world
    .get::<TileLayerData>(entity)
    .map(|data| data.tiles.bounds())
  else {
    return false;
  };
  if bounds.is_some_and(|bounds| !bounds.contains(cell)) {
    return false;
  }
  // A streamed map's painted cell must be in a resident chunk; the cells
  // the paint reaches past them are left out, as `set_tile` leaves them
  if bounds.is_none() && !edit::streamed_holds(world, map, cell) {
    return false;
  }
  let (planned, warn_key) = {
    let Some(root) = world.get::<MapRoot>(map) else {
      return false;
    };
    let Some((index, set, key)) = find_set(world, root, wang_set) else {
      return false;
    };
    let index_of_color = match color {
      None => 0,
      Some(name) => match set.color_index(name) {
        Some(found) => found,
        None => return false,
      },
    };
    (
      plan(world, entity, (index, set), cell, index_of_color),
      (key.0, wang_set.to_string(), key.1),
    )
  };
  let Some(planned) = planned else {
    return false;
  };
  if planned.missed {
    world.init_resource::<WangWarned>();
    let (tileset, set, version) = warn_key;
    let first = world
      .get_resource_mut::<WangWarned>()
      .is_some_and(|mut warned| warn_first(&mut warned, (tileset, set), version));
    if first {
      crate::engine_warn!(
        module: "Tilemap",
        "map {map}: the Wang set {wang_set:?} has no tile to fit what painting {color:?} at {cell} asks of a cell, so that cell keeps its tile (this set warns once)"
      );
    }
  }
  let mut changed = false;
  for (at, tile) in planned.edits {
    changed |= edit::set_tile(world, map, layer, at, tile);
  }
  changed
}

#[cfg(test)]
mod tests {
  use glam::{IVec2, UVec2};

  use super::{WangWarned, warn_first};
  use crate::core::asset::{Assets, AssetsView};
  use crate::core::ecs::{Entity, World};
  use crate::core::tilemap::test_util::{MapWorld, load_map};
  use crate::core::tilemap::{
    MapRoot, MapsView, Tile, TileFlip, TileGrid, TileLayerData, TileMap, Tileset, WangKind, WangSet,
  };

  /// The ground layer of a spawned map, row by row
  fn layer(world: &mut World, map: Entity, size: IVec2) -> Vec<Option<Tile>> {
    let maps = MapsView::new(world);
    (0..size.y)
      .flat_map(|y| (0..size.x).map(move |x| IVec2::new(x, y)))
      .map(|cell| maps.tile(map, "ground", cell))
      .collect()
  }

  /// The ground layer of the map asset at `path`, row by row, and its size
  fn recorded(mw: &mut MapWorld, path: &str) -> (Vec<Option<Tile>>, IVec2) {
    let handle = load_map(&mut mw.world, &format!("tiled/maps/{path}"));
    let asset = mw.world.resource::<Assets<TileMap>>().get(&handle);
    let Some(grid) = asset.and_then(|m| m.tiles(0)) else {
      panic!("{path} has no tile layer")
    };
    let size = grid.extent().size().as_ivec2();
    let cells = (0..size.y)
      .flat_map(|y| (0..size.x).map(move |x| IVec2::new(x, y)))
      .map(|cell| grid.get(cell))
      .collect();
    (cells, size)
  }

  /// `from` painted `colour` (none erases) of `set` at `cell` by
  /// `set_terrain`, against `to` as Tiled's own brush painted it
  /// (`scripts/paint-wang.js`)
  fn assert_as_tiled(from: &str, to: &str, set: &str, cell: IVec2, colour: Option<&str>) {
    let mut mw = MapWorld::new();
    let handle = load_map(&mut mw.world, &format!("tiled/maps/{from}"));
    let map = mw.spawn(MapRoot::new(handle));
    let (expected, size) = recorded(&mut mw, to);
    MapsView::new(&mut mw.world).set_terrain(map, "ground", cell, set, colour);
    let got = layer(&mut mw.world, map, size);
    let show = |cells: &[Option<Tile>]| -> Vec<String> {
      cells
        .chunks(size.x as usize)
        .map(|row| {
          row
            .iter()
            .map(|t| match t {
              None => ".".to_string(),
              Some(t) => format!("{}{:?}", t.id, t.flip),
            })
            .collect::<Vec<_>>()
            .join(" ")
        })
        .collect()
    };
    assert_eq!(show(&got), show(&expected), "{from} -> {to}");
  }

  fn wang_set(world: &mut World, tileset: &str, name: &str) -> WangSet {
    let handle = AssetsView::new(world)
      .load_now::<Tileset>(&format!("tiled/tilesets/{tileset}"))
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
    assert_as_tiled(
      "wang-dirt.tmj",
      "wang-grass.tmj",
      "ground",
      IVec2::new(2, 2),
      Some("grass"),
    );
    // the same paint again changes nothing
    let mut mw = MapWorld::new();
    let handle = load_map(&mut mw.world, "tiled/maps/wang-grass.tmj");
    let map = mw.spawn(MapRoot::new(handle));
    let mut maps = MapsView::new(&mut mw.world);
    assert!(!maps.set_terrain(map, "ground", IVec2::new(2, 2), "ground", Some("grass")));
  }

  #[test]
  fn painting_on_the_map_s_right_and_bottom_edges_keeps_to_the_map() {
    // The cell's corners on the map's border take the colour, and the cells
    // inside pick their transitions, as Tiled's brush paints them
    assert_as_tiled(
      "wang-dirt.tmj",
      "wang-grass-edge.tmj",
      "ground",
      IVec2::new(4, 2),
      Some("grass"),
    );
    assert_as_tiled(
      "wang-dirt.tmj",
      "wang-grass-far-corner.tmj",
      "ground",
      IVec2::new(4, 4),
      Some("grass"),
    );
  }

  #[test]
  fn erasing_empties_the_cell_and_keeps_what_has_no_tile_to_fit() {
    // the neighbours would face a hole, and a two-colour set has no tile
    // with a bare corner, so they stay as they are
    assert_as_tiled(
      "wang-grass.tmj",
      "wang-erased.tmj",
      "ground",
      IVec2::new(2, 2),
      None,
    );
  }

  #[test]
  fn a_one_colour_set_paints_transitions_into_empty_cells_and_erases_them() {
    assert_as_tiled(
      "wang-solo-empty.tmj",
      "wang-solo-painted.tmj",
      "solo",
      IVec2::new(2, 2),
      Some("ground"),
    );
    assert_as_tiled(
      "wang-solo-full.tmj",
      "wang-solo-erased.tmj",
      "solo",
      IVec2::new(2, 2),
      None,
    );
  }

  #[test]
  fn an_incomplete_mixed_set_resolves_as_tiled_does() {
    // the set holds the cells left and right of the paint but not above or
    // below: Tiled carries the paint up and down two cells
    assert_as_tiled(
      "wang-mix-a.tmj",
      "wang-mix-b.tmj",
      "mix",
      IVec2::new(2, 2),
      Some("b"),
    );
  }

  #[test]
  fn a_correction_stops_two_cells_out_in_a_set_of_two_colours() {
    // b at the corner of a layer of a: the column of b carries two cells down
    // and no further, where Tiled leaves it
    assert_as_tiled(
      "wang-mix-a.tmj",
      "wang-mix-corner.tmj",
      "mix",
      IVec2::ZERO,
      Some("b"),
    );
    let (cells, _) = recorded(&mut MapWorld::new(), "wang-mix-corner.tmj");
    let tile = |x: usize, y: usize| cells[y * 5 + x].map(|t| t.id);
    assert_eq!(
      (tile(0, 2), tile(0, 3)),
      (Some(1), Some(0)),
      "the fixture stops at row 2"
    );
  }

  #[test]
  fn a_missing_transition_is_made_of_the_middle_colour() {
    assert_as_tiled(
      "wang-tri-grass.tmj",
      "wang-tri-water.tmj",
      "shore",
      IVec2::new(3, 3),
      Some("water"),
    );
  }

  #[test]
  fn painting_terrain_outside_every_chunk_of_an_unbounded_layer_makes_its_chunks() {
    // The one-colour set painted into an unbounded layer, far from the one
    // chunk it has: the cells around the paint take what Tiled's brush gave
    // the same paint into an empty layer, in chunks made for them
    let mut mw = MapWorld::new();
    let empty = load_map(&mut mw.world, "tiled/maps/wang-solo-empty.tmj");
    let tilesets = mw
      .world
      .resource::<Assets<TileMap>>()
      .get(&empty)
      .map(|m| m.tilesets.clone())
      .unwrap_or_default();
    let mut map = TileMap::new(UVec2::ZERO, UVec2::splat(16));
    map.tilesets = tilesets;
    let layer = map.add_tile_layer("ground");
    if let Some(grid) = map.tiles_mut(layer) {
      *grid = TileGrid::unbounded();
      grid.set(IVec2::ZERO, Some(Tile::new(0, 0)));
    }
    let handle = mw.world.resource_mut::<Assets<TileMap>>().add(map);
    let map = mw.spawn(MapRoot::new(handle));
    let painted = IVec2::new(-100, 70);
    let mut maps = MapsView::new(&mut mw.world);
    assert!(maps.set_terrain(map, "ground", painted, "solo", Some("ground")));
    let (expected, size) = recorded(&mut mw, "wang-solo-painted.tmj");
    let corner = painted - IVec2::splat(2);
    let maps = MapsView::new(&mut mw.world);
    let got: Vec<Option<Tile>> = (0..size.y)
      .flat_map(|y| (0..size.x).map(move |x| IVec2::new(x, y)))
      .map(|cell| maps.tile(map, "ground", corner + cell))
      .collect();
    assert_eq!(got, expected);
    let Some(ground) = maps.layer_entity(map, "ground") else {
      panic!("no ground layer");
    };
    let chunks: Vec<IVec2> = mw
      .world
      .get::<TileLayerData>(ground)
      .map(|data| data.tiles.chunk_coords().collect())
      .unwrap_or_default();
    assert_eq!(chunks, [IVec2::ZERO, TileGrid::chunk_of(painted)]);
  }

  #[test]
  fn painting_terrain_at_the_ends_of_i32_is_refused() {
    // An unbounded layer takes any cell, but a paint changes the cells
    // around it: one whose reach leaves `i32` is refused, not wrapped to
    // the far end of the world (or a panic in a dev build)
    let mut mw = MapWorld::new();
    let empty = load_map(&mut mw.world, "tiled/maps/wang-solo-empty.tmj");
    let tilesets = mw
      .world
      .resource::<Assets<TileMap>>()
      .get(&empty)
      .map(|m| m.tilesets.clone())
      .unwrap_or_default();
    let mut map = TileMap::infinite(UVec2::splat(16));
    map.tilesets = tilesets;
    map.add_tile_layer("ground");
    let handle = mw.world.resource_mut::<Assets<TileMap>>().add(map);
    let map = mw.spawn(MapRoot::new(handle));
    let (lo, hi) = (i32::MIN, i32::MAX);
    let mut maps = MapsView::new(&mut mw.world);
    for cell in [
      IVec2::new(hi, 0),
      IVec2::new(lo, 0),
      IVec2::new(0, hi),
      IVec2::new(0, lo),
      IVec2::new(hi - 2, lo + 2),
      IVec2::new(lo + 1, hi - 1),
    ] {
      assert!(
        !maps.set_terrain(map, "ground", cell, "solo", Some("ground")),
        "{cell} is refused"
      );
    }
    let Some(ground) = maps.layer_entity(map, "ground") else {
      panic!("no ground layer");
    };
    let chunks = mw
      .world
      .get::<TileLayerData>(ground)
      .map_or(0, |data| data.tiles.chunk_coords().count());
    assert_eq!(chunks, 0, "nothing was written");
    // A cell whose reach fits paints as anywhere else
    let near = IVec2::new(hi - 4, lo + 4);
    let mut maps = MapsView::new(&mut mw.world);
    assert!(maps.set_terrain(map, "ground", near, "solo", Some("ground")));
    assert!(maps.tile(map, "ground", near).is_some());
  }

  #[test]
  fn a_flipped_neighbour_is_read_through_its_flip() {
    assert_as_tiled(
      "wang-flipped.tmj",
      "wang-flipped-painted.tmj",
      "ground",
      IVec2::new(2, 2),
      Some("grass"),
    );
  }

  #[test]
  fn a_set_with_transformations_picks_flipped_and_rotated_tiles() {
    assert_as_tiled(
      "wang-turn-dirt.tmj",
      "wang-turn-painted.tmj",
      "turn",
      IVec2::new(2, 2),
      Some("grass"),
    );
    let (cells, _) = recorded(&mut MapWorld::new(), "wang-turn-painted.tmj");
    assert!(
      cells.iter().flatten().any(|t| t.flip != TileFlip::NONE),
      "the fixture does use flips"
    );
  }

  #[test]
  fn a_chain_of_four_colours_is_corrected_ring_by_ring() {
    // deep water into grass: sand and shallows between, three rings out
    assert_as_tiled(
      "wang-chain-grass.tmj",
      "wang-chain-deep.tmj",
      "depth",
      IVec2::new(5, 5),
      Some("deep"),
    );
  }

  #[test]
  fn an_untransformed_tile_wins_when_the_set_prefers_it() {
    // a complete set that may also turn its tiles: every tile fits twice
    assert_as_tiled(
      "wang-rot-dirt.tmj",
      "wang-rot-painted.tmj",
      "ground",
      IVec2::new(2, 2),
      Some("grass"),
    );
    let (cells, _) = recorded(&mut MapWorld::new(), "wang-rot-painted.tmj");
    assert!(cells.iter().flatten().all(|t| t.flip == TileFlip::NONE));
  }

  #[test]
  fn an_edge_set_picks_its_tiles() {
    let mut mw = MapWorld::new();
    let handle = load_map(&mut mw.world, "tiled/maps/wang-dirt.tmj");
    let map = mw.spawn(MapRoot::new(handle));
    // brick at (2, 2) of a layer of plank (as Tiled paints it: the four
    // cells across an edge pick again, the diagonals share none)
    let mut maps = MapsView::new(&mut mw.world);
    maps.fill(
      map,
      "ground",
      IVec2::ZERO,
      IVec2::splat(4),
      Some(Tile::new(0, 15)),
    );
    assert!(maps.set_terrain(map, "ground", IVec2::new(2, 2), "walls", Some("brick")));
    let ids: Vec<u32> = layer(&mut mw.world, map, IVec2::splat(5))
      .iter()
      .flatten()
      .map(|t| t.id)
      .collect();
    let at = |x: usize, y: usize| ids[y * 5 + x];
    assert_eq!(at(2, 2), 0, "brick on every edge");
    assert_eq!(at(2, 1), 13, "above: its bottom edge is brick");
    assert_eq!(at(3, 2), 14, "right: its left edge");
    assert_eq!(at(2, 3), 7, "below: its top edge");
    assert_eq!(at(1, 2), 11, "left: its right edge");
    for (x, y) in [(1, 1), (3, 1), (1, 3), (3, 3)] {
      assert_eq!(at(x, y), 15, "diagonal ({x}, {y}) is unchanged");
    }
  }

  #[test]
  fn ties_pick_by_probability_and_are_stable() {
    let mut mw = MapWorld::new();
    let tied = wang_set(&mut mw.world, "wang.tsj", "tied");
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
            .map_or(u32::MAX, |(tile, _)| tile)
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

    // the same edits to two maps give the same tiles
    let paint = |mw: &mut MapWorld| {
      let handle = load_map(&mut mw.world, "tiled/maps/wang-dirt.tmj");
      let map = mw.spawn(MapRoot::new(handle));
      let mut maps = MapsView::new(&mut mw.world);
      for x in 0..5 {
        for y in 0..5 {
          maps.set_terrain(map, "ground", IVec2::new(x, y), "tied", Some("x"));
        }
      }
      layer(&mut mw.world, map, IVec2::splat(5))
    };
    let (first, second) = (paint(&mut mw), paint(&mut mw));
    assert_eq!(first, second);
    let flat: Vec<u32> = first.iter().flatten().map(|t| t.id).collect();
    assert!(
      flat.contains(&4) && flat.contains(&5),
      "both tiles are used"
    );
  }

  #[test]
  fn a_colors_probability_weighs_the_pick() {
    let mut mw = MapWorld::new();
    let set = wang_set(&mut mw.world, "wang.tsj", "weighted");
    assert_eq!(set.color_index("heavy"), Some(2));
    // any color at every position: the two tiles and an empty cell all fit
    let mut tiles = [0usize; 2];
    for i in 0..400 {
      let cell = IVec2::new(i % 20, i / 20);
      if let Some((tile, _)) = set.best_tile([WangSet::ANY; 8], cell) {
        tiles[(tile - 6) as usize] += 1;
      }
    }
    // a tile weighs its colors' probabilities at each of its four edges:
    // 1 (light), 1.5^4 (heavy) and 1 (empty), so 57, 287 and 57 of 400
    assert!((35..=85).contains(&tiles[0]), "light took {}", tiles[0]);
    assert!((250..=320).contains(&tiles[1]), "heavy took {}", tiles[1]);
  }

  #[test]
  fn best_tile_follows_tileds_fit_rules() {
    let mut mw = MapWorld::new();
    let ground = wang_set(&mut mw.world, "wang.tsj", "ground");
    let cell = IVec2::new(1, 1);
    // exact
    assert_eq!(
      ground.best_tile([0, 1, 0, 2, 0, 2, 0, 2], cell),
      Some((11, TileFlip::NONE))
    );
    assert_eq!(
      ground
        .best_tile([0, 2, 0, 2, 0, 2, 0, 2], cell)
        .map(|t| t.0),
      Some(15)
    );
    // don't care where it does not matter
    assert_eq!(
      ground
        .best_tile(
          [
            WangSet::ANY,
            1,
            WangSet::ANY,
            1,
            WangSet::ANY,
            1,
            WangSet::ANY,
            1
          ],
          cell
        )
        .map(|t| t.0),
      Some(0)
    );
    // a color the set does not have, or no color in a set with no bare corner
    assert_eq!(ground.best_tile([0, 9, 0, 9, 0, 9, 0, 9], cell), None);
    assert_eq!(ground.best_tile([0; 8], cell), None);
    assert_eq!(ground.color_index("dirt"), Some(2));
    assert_eq!(ground.color_index("nope"), None);
    assert_eq!(ground.kind, WangKind::Corner);

    // empty is a color: a one-colour set with bare corners has a tile for it,
    // and an all-0 wanted is the empty cell, not a tile
    let solo = wang_set(&mut mw.world, "wang-solo.tsj", "solo");
    assert_eq!(
      solo.best_tile([0, 1, 0, 0, 0, 0, 0, 0], cell).map(|t| t.0),
      Some(0)
    );
    assert_eq!(solo.best_tile([0; 8], cell), None);

    // the set's flips widen the choice: a tile turned fits, and it is told how
    let turn = wang_set(&mut mw.world, "wang-turn.tsj", "turn");
    let (tile, flip) = turn
      .best_tile([0, 2, 0, 2, 0, 1, 0, 1], cell)
      .expect("a turned tile fits");
    assert_eq!(tile, 3);
    assert_ne!(flip, TileFlip::NONE);
    // untransformed first when the set prefers it
    assert_eq!(
      turn.best_tile([0, 1, 0, 1, 0, 1, 0, 1], cell),
      Some((0, TileFlip::NONE))
    );
  }

  #[test]
  fn an_unmatched_pattern_keeps_the_tile_and_warns_once() {
    let mut mw = MapWorld::new();
    let handle = load_map(&mut mw.world, "tiled/maps/wang-dirt.tmj");
    let map = mw.spawn(MapRoot::new(handle));
    let before = layer(&mut mw.world, map, IVec2::splat(5));
    let warnings = || {
      crate::core::dev_log::get_entries()
        .iter()
        .filter(|e| e.message.contains("Wang set \"mix\"") && e.message.contains("Some(\"c\")"))
        .count()
    };
    let mut maps = MapsView::new(&mut mw.world);
    // color c has no tile in the mixed set
    assert!(!maps.set_terrain(map, "ground", IVec2::new(2, 2), "mix", Some("c")));
    assert!(!maps.set_terrain(map, "ground", IVec2::new(1, 1), "mix", Some("c")));
    assert!(!maps.set_terrain(map, "ground", IVec2::new(3, 3), "mix", Some("c")));
    assert_eq!(layer(&mut mw.world, map, IVec2::splat(5)), before);
    assert_eq!(
      warnings(),
      1,
      "one warning for the set, however many misses"
    );
    let counted: u32 = mw
      .world
      .resource::<WangWarned>()
      .0
      .values()
      .map(|v| v.1)
      .sum();
    assert_eq!(counted, 1);

    // a set, color, layer or cell that is not there changes nothing and does
    // not warn
    let mut maps = MapsView::new(&mut mw.world);
    assert!(!maps.set_terrain(map, "ground", IVec2::new(2, 2), "nope", Some("a")));
    assert!(!maps.set_terrain(map, "ground", IVec2::new(2, 2), "mix", Some("nope")));
    assert!(!maps.set_terrain(map, "nope", IVec2::new(2, 2), "mix", Some("a")));
    assert!(!maps.set_terrain(map, "ground", IVec2::new(9, 9), "mix", Some("a")));
    assert_eq!(warnings(), 1);
  }

  #[test]
  fn a_set_warns_again_once_its_tileset_changes() {
    let mut warned = WangWarned::default();
    let key = || ("terrain".to_string(), "ground".to_string());
    assert!(warn_first(&mut warned, key(), 1));
    assert!(!warn_first(&mut warned, key(), 1));
    assert!(warn_first(&mut warned, key(), 2), "the tileset changed");
    assert!(!warn_first(&mut warned, key(), 2));
    // another tileset's set of the same name is its own
    assert!(warn_first(
      &mut warned,
      ("other".to_string(), "ground".to_string()),
      1
    ));
  }

  /// A 255-tile mixed set with every flip allowed, as procgen would call it
  fn big_set() -> WangSet {
    use crate::core::tilemap::{TileTransformations, WangColor, WangTile};
    let colors = (1..=3)
      .map(|i| WangColor {
        name: format!("c{i}"),
        color: crate::core::ui::Color::WHITE,
        tile: None,
        probability: 1.0,
        properties: Default::default(),
      })
      .collect();
    let mut tiles = Vec::new();
    let mut seed = 12345u32;
    for tile in 0..255 {
      let wangid = std::array::from_fn(|_| {
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        ((seed >> 24) % 4) as u8
      });
      tiles.push(WangTile {
        tile,
        wangid,
        probability: 1.0,
      });
    }
    WangSet::new(
      "big",
      WangKind::Mixed,
      colors,
      tiles,
      TileTransformations {
        hflip: true,
        vflip: true,
        rotate: true,
        prefer_untransformed: true,
      },
    )
  }

  #[test]
  fn a_set_builds_its_candidates_once_and_a_copy_builds_its_own() {
    let set = big_set();
    let first: *const super::Matcher = set.matcher();
    assert!(std::ptr::eq(first, set.matcher()), "built once per set");
    let copy = set.clone();
    assert!(!std::ptr::eq(first, copy.matcher()), "a copy has its own");
    // the same picks either way
    let wanted = [1, 2, 3, 1, 2, 3, 1, 2];
    for i in 0..20 {
      let cell = IVec2::new(i, 3);
      assert_eq!(set.best_tile(wanted, cell), copy.best_tile(wanted, cell));
    }
  }

  #[test]
  #[ignore = "timing; run by hand"]
  fn best_tile_timing_on_a_255_tile_set() {
    let set = big_set();
    let start = std::time::Instant::now();
    let mut found = 0;
    for i in 0..2000 {
      if set
        .best_tile([1, 2, 3, 1, 2, 3, 1, 2], IVec2::new(i, i))
        .is_some()
      {
        found += 1;
      }
    }
    println!(
      "best_tile on 255 tiles with flips: {:?} per call ({found} found)",
      start.elapsed() / 2000
    );
  }
}
