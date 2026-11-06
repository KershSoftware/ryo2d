//! Map occluders: a spawned map's occluding tile layers cast 2D shadows
//! through `LightOccluder2D` chains, kept in step with their tiles.
//!
//! A layer occludes when its `LayerInfo::occludes` says so, else when it
//! collides (`collides`); a disabled map (or layer) has no occluders, and
//! enabling it builds them from its tiles as they then are. Occluding
//! layers sharing a cell size and offset occlude as one: each cell takes
//! the strongest kind across them, as collision's union does. What a cell
//! occludes with: a tile's shapes marked `occluder`, else its collision
//! shapes (one-way ones too: light does not care which way a platform lets
//! bodies through), all placed where it draws as collision places them; an
//! IntGrid's solid cells whole and its slopes as their half cell.
//!
//! The occluders are the boundary of what occludes, worked out by the tile
//! colliders' outline units (`physics::Boundary`): an edge where one side
//! occludes and the other does not, shapes fitted to their cells on the
//! outline grid, edges two cells share cancelling, across chunk borders too.
//! So an edge inside a solid mass never casts its shadow through the lit
//! art beyond it. Each chunk owns the borders on the left of and above its
//! cells (and the grid's right and bottom ones where it reaches them); its
//! edges are chained into open chains (`LightOccluder2D::chain`), each a
//! child of the group's first layer marked `MapOccluder` with its chunk, its
//! entity at the centre of its corners' box (the light's reach test reads
//! the entity's position less the chain's extent, its farthest corner from
//! there). A chunk whose version moved is built again, and
//! so is the chunk right of it or below it when the cells it shares a
//! border with changed kind (that one only at the border, its other edges
//! as it kept them); every other chunk keeps its occluders. Chunks
//! are signed: a group of bounded layers covers their largest bounds, one
//! with an unbounded layer any chunk, where an unbounded grid has no right
//! or bottom border, so the borders right of and below its last cells are
//! the next chunk's (built as that chunk, though its layers have none).
//!
//! A hexagonal layer casts its shadows along the sides its colliders keep:
//! each side between a cell that occludes whole and one that does not,
//! owned by the earlier of the two in row order (on a bounded grid, by the
//! one inside it), so a chunk owns the sides between its cells and those
//! right of and below them, and an unbounded group's chunks left of and
//! above its cells hold their first sides; and each other tile's own
//! shape, a closed chain of each of its rings, as its colliders hold them.
//! Each chunk keeps what its last build read (`HexKept`), so an edit's
//! build works from the cells that changed: it reads again only the sides
//! they reach, chains again only the chains through their corners, and
//! spawns again only those and the rings of the cells whose shape changed.
//! A rim cell whose solidity changed builds again the chunks whose cells
//! own one of its sides, and only their chains through it.
//!
//! A map's first chains are worked out off the main thread with the rest
//! of its landing (`spawn.rs`: `prepare`), and spawned as its layers do
//! (`land`). What it builds after that builds at once.

use std::collections::BTreeMap;
use std::sync::Arc;

use glam::{Affine2, DVec2, IVec2, Vec2};

use super::collision::LayerView;
use super::kinds::{
  CELLS, ChunkKinds, Context, GridKey, Layers, Part, Purpose, ShapeKey, ShapeSource, Stores,
  TilesetStamp, Tracked, changed_chunks, lattice_of, layer_data, reindexing, scatter, shapes_kept,
  sources, stamps, union_kinds,
};
use super::spawn::{IntGridKey, MapLayers, MapRoot, TileLayerData, root_map};
use super::{CHUNK_SIZE, Lattice, TileGrid};
use crate::core::ecs::{
  Allow, ChildOf, Commands, Component, DetectChangesMut, Disabled, Entity, Has, Local,
  ParentPlaced, Query, Transform, World,
};
use crate::core::hash::{FastMap, FastSet};
use crate::core::lighting::LightOccluder2D;
use crate::core::physics::{
  Boundary, CellRect, GRID, HexCells, HexSides, PlacedEdge, Point, TileCell, chain_points,
};

/// Marks an occluder a map keeps from its tiles: a child of the first layer
/// of the layers occluding as one, rebuilt with its chunk. Games can find a
/// map's shadows by it; the map owns them, so an edit to one lasts until a
/// change to its chunk builds it again. Only the map's walk places it: a change to its
/// `Transform` is not applied until the map next moves, and a rebuild
/// replaces it
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct MapOccluder {
  /// The chunk of the layers it was built from: signed, as
  /// [`TileGrid::chunk_of`] gives it; on a staggered map, a chunk of its
  /// lattice's cells ([`Lattice::lattice_cell`](super::Lattice::lattice_cell))
  pub chunk: IVec2,
}

/// An occluding layer as the occluders were built from it: its place among
/// the map's layers and its entity (a stand-in until the layers spawn)
#[derive(Clone, PartialEq, Debug)]
pub(crate) struct Lit {
  entity: Entity,
  index: usize,
  grid: GridKey,
  offset: [u32; 2],
  /// Its grid's bounds; `None` for an unbounded grid
  bounds: Option<CellRect>,
  intgrid: bool,
}

/// What a chunk's cells along its right and bottom sides were when it was
/// last built: the chunks right of it and below read them. A chunk not
/// kept is one whose sides are all `Empty`
#[derive(Debug, Default, PartialEq)]
struct Sides {
  right: Vec<TileCell>,
  bottom: Vec<TileCell>,
}

/// Whether a side kept as `old` (none: all `Empty`) is `now`
fn same_side(old: Option<&[TileCell]>, now: &[TileCell]) -> bool {
  match old {
    Some(old) => old == now,
    None => now.iter().all(|&c| c == TileCell::Empty),
  }
}

/// The first chunk a group of unbounded layers casts shadows in, on either
/// axis, and one past the last: a chunk's cells, the column left of it and
/// the row above are all cells `i32` holds
const SHADOW_CHUNKS: (i32, i32) = (
  -(i32::MAX / CHUNK_SIZE as i32),
  i32::MAX / CHUNK_SIZE as i32,
);

/// A chain to spawn: where its entity goes, and its vertices around it
type Chain = (Vec2, Vec<Vec2>);

/// The chain through `points` (its layers' space): its entity at the
/// centre of its points' box, its vertices round it
fn chain_at(points: Vec<Vec2>) -> Chain {
  let low = points.iter().copied().fold(Vec2::MAX, Vec2::min);
  let high = points.iter().copied().fold(Vec2::MIN, Vec2::max);
  let centre = (low + high) * 0.5;
  (centre, points.into_iter().map(|p| p - centre).collect())
}

/// A chunk's mark of the chunk `step` from it (each way -1, 0 or 1) to
/// build again, the cells it reads having changed: a bit of the marks a
/// chunk's build returns. On a square lattice the chunks right of it and
/// below it read its cells (a border belongs to the cells right of and
/// below it); on a hexagonal grid, whose sides belong to the earlier of
/// their two cells in row order, the chunks whose cells own a side of a
/// cell of its that changed
const fn mark(step: IVec2) -> u16 {
  1 << ((step.y + 1) * 3 + step.x + 1)
}

/// The cells a chunk's row of solidity holds (`Solidity`)
const SIDE: usize = 32;
const _: () = assert!(CHUNK_SIZE as usize == SIDE);

/// The solidity a hexagonal chunk's sides read: its own cells', a row a
/// word (bit x of word y), and that of the cells across the sides it owns,
/// the column left of it and the column right of it (bit y) and the row
/// below it from the cell left of its first column (bit x + 1). Only
/// `Solid` cells are solid to a hexagonal side
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Solidity {
  cells: [u32; SIDE],
  left: u32,
  right: u32,
  below: u64,
}

impl Solidity {
  /// Whether the cell at `(x, y)` from the chunk's first cell is solid
  /// (outside what it reads, no)
  fn at(&self, x: i32, y: i32) -> bool {
    let inside = |v: i32| (0..SIDE as i32).contains(&v);
    match (inside(x), inside(y)) {
      (true, true) => (self.cells[y as usize] >> x) & 1 != 0,
      (false, true) if x == -1 => (self.left >> y) & 1 != 0,
      (false, true) if x == SIDE as i32 => (self.right >> y) & 1 != 0,
      (_, false) if y == SIDE as i32 && (-1..=SIDE as i32).contains(&x) => {
        (self.below >> (x + 1)) & 1 != 0
      }
      _ => false,
    }
  }

  /// The cells solid in one of `self` and `other` and not the other
  fn changed(&self, other: &Solidity) -> Solidity {
    let mut cells = [0; SIDE];
    for (y, row) in cells.iter_mut().enumerate() {
      *row = self.cells[y] ^ other.cells[y];
    }
    Solidity {
      cells,
      left: self.left ^ other.left,
      right: self.right ^ other.right,
      below: self.below ^ other.below,
    }
  }

  fn count(&self) -> u32 {
    let rows: u32 = self.cells.iter().map(|row| row.count_ones()).sum();
    rows + self.left.count_ones() + self.right.count_ones() + self.below.count_ones()
  }

  /// Whether none of its own cells is solid
  fn none_inside(&self) -> bool {
    self.cells.iter().all(|&row| row == 0)
  }

  /// Each cell it holds solid, from the chunk's first cell: its own by
  /// row, then the column left of it, the column right of it and the row
  /// below it
  fn each(&self, mut f: impl FnMut(IVec2)) {
    let mut bits = |mut word: u64, at: &dyn Fn(i32) -> IVec2| {
      while word != 0 {
        f(at(word.trailing_zeros() as i32));
        word &= word - 1;
      }
    };
    for (y, &row) in self.cells.iter().enumerate() {
      bits(u64::from(row), &|x| IVec2::new(x, y as i32));
    }
    bits(u64::from(self.left), &|y| IVec2::new(-1, y));
    bits(u64::from(self.right), &|y| IVec2::new(SIDE as i32, y));
    bits(self.below, &|x| IVec2::new(x - 1, SIDE as i32));
  }
}

/// What a hexagonal chunk's last build read, and the chains it spawned
/// from that: an edit's build compares the chunk's cells with it, and
/// builds again only what the cells that changed reach. A chunk none of
/// whose cells is solid or shaped keeps none (its next build is whole)
#[derive(Debug, Default)]
struct HexKept {
  read: Solidity,
  /// Its shape cells within the group's bounds, each by its slot (y * 32 +
  /// x) and its shape, in slot order
  shapes: Vec<(u16, u16)>,
  /// The chain each of its edges (`Group::edges`, by their place there)
  /// runs in; none on a grid whose sides have length 0, chained whole at
  /// each build
  chain_of: Vec<u16>,
  /// Each chain's occluder, by its index (`None`: an index free, or a chain
  /// not yet spawned)
  chains: Vec<Option<Entity>>,
  /// How many chains its last whole chaining made
  whole: usize,
  /// Each shape cell's rings' occluders, by `slot << 16 | ring`
  rings: FastMap<u32, Entity>,
}

impl HexKept {
  /// Notes `entity` as the occluder spawned for `slot`
  fn record(&mut self, slot: Slot, entity: Entity) {
    match slot {
      Slot::Side(id) => {
        if let Some(chain) = self.chains.get_mut(usize::from(id)) {
          *chain = Some(entity);
        }
      }
      Slot::Ring(key) => {
        self.rings.insert(key, entity);
      }
    }
  }
}

/// A chain to spawn, with its slot in its hexagonal chunk's `HexKept` (none
/// off a hexagonal grid)
type Slotted = (Option<Slot>, Chain);

/// Where a hexagonal chunk's new chain goes in its `HexKept`: a chain of
/// its sides by index, or a ring by `slot << 16 | ring`
#[derive(Clone, Copy, Debug)]
enum Slot {
  Side(u16),
  Ring(u32),
}

/// What a hexagonal chunk's build changed: the occluders that go, the
/// chains to spawn, each with its slot, and the chunks round it to build
/// again (`mark` bits)
struct HexBuilt {
  gone: Vec<Entity>,
  spawn: Vec<Slotted>,
  marks: u16,
}

/// Changed cells (its own and those it reads round it) past which a
/// hexagonal chunk is chained again whole: such a build reaches most of
/// its chains anyway, as a streamed chunk landing or unloading does
const HEX_WHOLE: u32 = 64;

/// Layers sharing a grid and offset, occluding as one
#[derive(Debug)]
struct Group {
  grid: GridKey,
  offset: [u32; 2],
  /// Its occluders' parent: its first layer
  parent: Entity,
  parent_layer: usize,
  /// The cells its layers hold: their largest bounds (bounded grids start
  /// at (0, 0)), or `None` with an unbounded layer
  bounds: Option<CellRect>,
  layers: Vec<Tracked>,
  /// Each tile's kind (by its cell's value) and the shapes they name
  kinds: FastMap<u32, TileCell>,
  shapes: FastMap<ShapeKey, u16>,
  boundary: Boundary,
  /// Each chunk's occluders in the order they were spawned (so each is
  /// despawned newest first, off the end of its parent's children), a
  /// chunk with none left out
  chunks: FastMap<IVec2, Vec<Entity>>,
  /// Each chunk's boundary edges as it was last built (`Boundary`'s
  /// placed edges, from its top-left), a chunk with none left out: a chunk
  /// whose own cells did not change since rebuilds only its borders on the
  /// left and above from them (on a hexagonal grid its rim cells' sides)
  edges: FastMap<IVec2, Vec<PlacedEdge>>,
  /// Each chunk's sides, a chunk whose sides are all `Empty` left out
  sides: FastMap<IVec2, Sides>,
  /// The chains a first build worked out, chunk by chunk (each once), each
  /// with its slot in its hexagonal chunk's `HexKept`, until they spawn as
  /// it lands
  pending: Vec<(IVec2, Vec<Slotted>)>,
  /// On a staggered grid, whose chunks of cells straddle its lattice's: the
  /// lattice, and the kinds of its cells by lattice chunk (a chunk all
  /// `Empty` left out), which its chunks are built from. Its chunks are the
  /// lattice's, and unbounded
  lattice: Option<(Lattice, FastMap<IVec2, Box<ChunkKinds>>)>,
  /// On a hexagonal grid, its cells (whose sides `boundary` reads, by their
  /// owners) and what each chunk's last build read and spawned (a chunk
  /// none of whose cells is solid or shaped left out). `sides` stays empty
  hex: Option<HexCells>,
  hex_chunks: FastMap<IVec2, HexKept>,
}

impl Group {
  fn new(lit: &Lit) -> Group {
    let grid = lit.grid.grid();
    let lattice = reindexing(&grid).map(|lattice| (lattice, FastMap::default()));
    let hex = grid.hex_cells();
    Group {
      grid: lit.grid,
      offset: lit.offset,
      parent: lit.entity,
      parent_layer: lit.index,
      bounds: lit.bounds.filter(|_| lattice.is_none()),
      layers: Vec::new(),
      kinds: FastMap::default(),
      shapes: FastMap::default(),
      boundary: match hex {
        Some(hex) => Boundary::default().with_hex(hex),
        None => Boundary::default(),
      },
      chunks: FastMap::default(),
      edges: FastMap::default(),
      sides: FastMap::default(),
      pending: Vec::new(),
      lattice,
      hex,
      hex_chunks: FastMap::default(),
    }
  }

  /// Whether `chunk` is one the group builds: one holding cells of its
  /// bounds, or for unbounded layers one within `SHADOW_CHUNKS`
  fn holds_chunk(&self, chunk: IVec2) -> bool {
    match self.bounds {
      Some(bounds) if bounds.is_empty() => false,
      Some(bounds) => {
        let (first, last) = (
          TileGrid::chunk_of(bounds.min),
          TileGrid::chunk_of(bounds.max - IVec2::ONE),
        );
        chunk.cmpge(first).all() && chunk.cmple(last).all()
      }
      None => {
        let (first, end) = SHADOW_CHUNKS;
        chunk.cmpge(IVec2::splat(first)).all() && chunk.cmplt(IVec2::splat(end)).all()
      }
    }
  }

  /// Despawns every occluder, each chunk's newest first
  fn clear(&mut self, commands: &mut Commands) {
    for (_, chunk) in self.chunks.drain() {
      for occluder in chunk.into_iter().rev() {
        commands.entity(occluder).try_despawn();
      }
    }
    self.edges.clear();
    self.hex_chunks.clear();
  }
}

/// A map's occluders: what they were built from and its groups. On the map
/// entity
#[derive(Component, Debug)]
pub(crate) struct MapShadows {
  layers: Vec<Lit>,
  tilesets: Vec<TilesetStamp>,
  overrides: Vec<(IntGridKey, TileCell)>,
  shapes: Arc<[Option<ShapeSource>]>,
  groups: Vec<Group>,
  /// The layers warned about, by place: occluding with hexagons the tracer
  /// does not take
  warned: FastSet<usize>,
}

impl MapShadows {
  fn clear(&mut self, commands: &mut Commands) {
    for group in &mut self.groups {
      group.clear(commands);
    }
  }
}

/// Chains `edges` (directed) into as few open chains as it can greedily:
/// from each point with more edges leaving than arriving, then round what
/// is left (closed loops, each a chain ending where it starts). Straight
/// runs merge into one edge. A point's edges are taken in the order given.
/// The edges are sorted by start and by end once, and every point found
/// by its place in those orders, with no map: an edit's chunk of a dense
/// map has about a thousand edges, and a list a point, maps grown from
/// empty and a chain made for every start tried cost half its rebuild; a
/// map of the points made at its full size was a third of a chunk's
/// shadows once those went
fn chains(edges: &[(Point, Point)]) -> Vec<Vec<Point>> {
  const NONE: u32 = u32::MAX;
  // Edge indices by start point, each point's in the order given
  let mut sorted: Vec<(Point, u32)> = edges
    .iter()
    .enumerate()
    .map(|(i, &(a, _))| (a, i as u32))
    .collect();
  sorted.sort_unstable();
  let order: Vec<u32> = sorted.iter().map(|&(_, i)| i).collect();
  // Each start point (sorted) and its run of `order`, from its first edge
  // not yet used, and the run each edge starts in
  let mut points: Vec<Point> = Vec::new();
  let mut runs: Vec<(u32, u32)> = Vec::new();
  let mut start_run = vec![0u32; edges.len()];
  for (k, &(point, i)) in sorted.iter().enumerate() {
    if points.last() == Some(&point) {
      if let Some(run) = runs.last_mut() {
        run.1 = k as u32 + 1;
      }
    } else {
      points.push(point);
      runs.push((k as u32, k as u32 + 1));
    }
    start_run[i as usize] = (points.len() - 1) as u32;
  }
  // The run each edge ends in (`NONE` where no edge leaves its end): the
  // edges by end walked beside the points
  sorted.clear();
  sorted.extend(edges.iter().enumerate().map(|(i, &(_, b))| (b, i as u32)));
  sorted.sort_unstable();
  let mut end_run = vec![NONE; edges.len()];
  let mut p = 0;
  for &(end, i) in &sorted {
    while p < points.len() && points[p] < end {
      p += 1;
    }
    if points.get(p) == Some(&end) {
      end_run[i as usize] = p as u32;
    }
  }
  drop(sorted);
  // The edges arriving at each start point
  let mut arriving = vec![0u32; points.len()];
  for &run in &end_run {
    if run != NONE {
      arriving[run as usize] += 1;
    }
  }
  let mut used = vec![false; edges.len()];
  // The first edge of run `run` not used yet, taken
  let take = |runs: &mut [(u32, u32)], used: &mut [bool], run: u32| -> Option<usize> {
    let (first, end) = runs.get_mut(run as usize)?;
    // A run's used edges are its first ones: skip them
    while *first < *end && used[order[*first as usize] as usize] {
      *first += 1;
    }
    let found = (*first < *end).then(|| order[*first as usize] as usize)?;
    used[found] = true;
    Some(found)
  };
  // Heads first: the points more edges leave than arrive at, in order,
  // then whatever edges are left, in order
  let heads = (0..points.len() as u32).filter(|&run| {
    let (first, end) = runs[run as usize];
    end - first > arriving[run as usize]
  });
  let starts: Vec<u32> = heads.chain(start_run.iter().copied()).collect();
  let mut out = Vec::new();
  for start in starts {
    // A start with edges left starts chains until it has none
    while let Some(first) = take(&mut runs, &mut used, start) {
      let mut chain = vec![points[start as usize], edges[first].1];
      let mut at = end_run[first];
      while let Some(next) = take(&mut runs, &mut used, at) {
        let to = edges[next].1;
        // A straight run is one edge
        let (p, q) = (chain[chain.len() - 2], chain[chain.len() - 1]);
        let (u, v) = (
          (i64::from(q.0 - p.0), i64::from(q.1 - p.1)),
          (i64::from(to.0 - q.0), i64::from(to.1 - q.1)),
        );
        if u.0 * v.1 - u.1 * v.0 == 0 && u.0 * v.0 + u.1 * v.1 > 0 {
          chain.pop();
        }
        chain.push(to);
        at = end_run[next];
      }
      out.push(chain);
    }
  }
  out
}

/// Works out chunk `chunk` of `group` (one it holds) again from its
/// layers' cells: the chains of the boundary edges it owns, in its layers'
/// space. Returns them and whether its right and bottom sides changed kind
/// (the chunks across them read those cells). With `own` its own cells
/// changed and are read whole; without, only the cells across its borders
/// on the left and above did, so it reads its first column and row alone
/// and takes its other edges from those it kept (`Group::edges`), as it
/// does when its own cells are all empty
fn chunk_chains(
  group: &mut Group,
  chunk: IVec2,
  own: bool,
  data: &[Option<&TileLayerData>],
  context: &Context<'_>,
) -> (Vec<Chain>, bool, bool) {
  let side = CHUNK_SIZE as i32;
  // `holds_chunk`: its cells, and the column and row before them, are in
  // an i32
  let min = chunk * side;
  let mut max = min + IVec2::splat(side);
  if let Some(bounds) = group.bounds {
    max = max.min(bounds.max);
  }
  // The cells its borders read, each one kind across the group's layers:
  // its own (or its first column and row), the last column of the chunk
  // left of it and the last row of the chunk above
  let Group {
    layers,
    kinds,
    shapes,
    boundary,
    lattice,
    ..
  } = group;
  let (cells, left, above) = match lattice {
    // A staggered grid's kinds are kept by lattice chunk
    Some((_, kept)) => {
      let read = |at: IVec2| kept.get(&at).map_or([TileCell::Empty; CELLS], |c| **c);
      (read(chunk), read(chunk - IVec2::X), read(chunk - IVec2::Y))
    }
    None => {
      let mut classify = |at: IVec2, part: Part| {
        union_kinds(
          layers,
          data,
          (at, part),
          (&mut *kinds, &mut *shapes),
          &mut *boundary,
          context,
        )
      };
      let column = |x: i32| Part {
        min: IVec2::new(x, 0),
        max: IVec2::new(x + 1, side),
      };
      let row = |y: i32| Part {
        min: IVec2::new(0, y),
        max: IVec2::new(side, y + 1),
      };
      let cells = if own {
        classify(chunk, Part::WHOLE)
      } else {
        let mut first = classify(chunk, column(0));
        let top = classify(chunk, row(0));
        first[..side as usize].copy_from_slice(&top[..side as usize]);
        first
      };
      let left = classify(chunk - IVec2::X, column(side - 1));
      let above = classify(chunk - IVec2::Y, row(side - 1));
      (cells, left, above)
    }
  };
  let at = |x: i32, y: i32| {
    let (lx, ly) = (x - min.x, y - min.y);
    let inside = |v: i32| (0..side).contains(&v);
    if x >= max.x || y >= max.y {
      TileCell::Empty
    } else if inside(lx) && inside(ly) {
      cells[(ly * side + lx) as usize]
    } else if lx == -1 && inside(ly) {
      left[(ly * side + side - 1) as usize]
    } else if ly == -1 && inside(lx) {
      above[((side - 1) * side + lx) as usize]
    } else {
      TileCell::Empty
    }
  };
  let empty = own && cells.iter().all(|&c| c == TileCell::Empty);
  let mut placed = Vec::new();
  let (from, to) = ((min.x, min.y), (max.x, max.y));
  if own && !empty {
    group
      .boundary
      .placed_edges(at, from, to, group.bounds, &mut placed);
  } else {
    // Only its borders on the left and above can have changed: an empty
    // chunk has no other edges
    let kept = if empty { None } else { group.edges.get(&chunk) };
    group.boundary.rebuilt_borders(
      at,
      from,
      to,
      group.bounds,
      kept.map_or(&[][..], Vec::as_slice),
      &mut placed,
    );
  }
  // The outlines run clockwise on screen; lighting's polygons the other way
  // (mapped through a lattice that mirrors them, they run the other way
  // round and still bound what occludes, which is all lighting reads)
  let edges: Vec<(Point, Point)> = placed.iter().map(|&(_, a, b)| (b, a)).collect();
  let grid = group.grid.grid();
  let cell = grid.cell.as_dvec2();
  let to_pixel = lattice_of(&grid);
  let origin = min.as_dvec2();
  let px = |(x, y): Point| {
    let point = DVec2::new(f64::from(x), f64::from(y)) / f64::from(GRID);
    match to_pixel {
      Some(to) => through(to, origin + point),
      None => ((origin + point) * cell).as_vec2(),
    }
  };
  let built = chains(&edges)
    .into_iter()
    .map(|chain| chain_at(chain.into_iter().map(px).collect()))
    .collect();
  if placed.is_empty() {
    group.edges.remove(&chunk);
  } else {
    // Kept for as long as the chunk is: no room past its edges
    placed.shrink_to_fit();
    group.edges.insert(chunk, placed);
  }
  if !own {
    // Its own cells, its sides among them, are as they were
    return (built, false, false);
  }
  let sides = Sides {
    right: (min.y..max.y).map(|y| at(max.x - 1, y)).collect(),
    bottom: (min.x..max.x).map(|x| at(x, max.y - 1)).collect(),
  };
  let old = group.sides.get(&chunk);
  let right = !same_side(old.map(|s| &s.right[..]), &sides.right);
  let bottom = !same_side(old.map(|s| &s.bottom[..]), &sides.bottom);
  if same_side(None, &sides.right) && same_side(None, &sides.bottom) {
    group.sides.remove(&chunk);
  } else {
    group.sides.insert(chunk, sides);
  }
  (built, right, bottom)
}

/// Works out chunk `chunk` of a hexagonal group (its cells `hex`) again
/// from its layers' cells, in its layers' space: the chains of the sides
/// its cells own (`Boundary::with_hex`: each side belongs to the earlier of
/// its two cells in row order, or on a bounded grid to the one inside it,
/// so a chunk owns the sides between its cells and the cells right of and
/// below them, never those left of and above them), each point whole
/// pixels from the box of its first cell (`HexCells::corner_of`) plus the
/// grid's origin, and each of its shape cells' rings a closed chain of its
/// own (`Boundary::hex_rings`, placed as their colliders place them). Both
/// wound for lighting. Its sides read its own cells and the cells across
/// those it owns: the column right of it, the row below it with a cell
/// past it either way, and the column left of it (the cells down and to
/// the left of its first column's). With `own` its own cells changed and
/// are read again; without, only cells round it did, and its own are as it
/// kept them.
///
/// What it reads is compared with what the chunk kept (`HexKept`). The
/// sides its cells own that reach a cell whose solidity changed are read
/// again (`Boundary::hex_cell_edges`), every other edge kept; the chains
/// through a corner of such a cell (one whose sides the chunk owns some
/// of), those that lost or turned an edge among them, are chained again
/// with the new edges (`Boundary::hex_chain_edges`), and only they are
/// spawned again. An edge's next one is decided by the three cells at its
/// end, so every other chain still runs as a whole build would run it, and
/// the chunk holds as many chains as a whole build makes. A shape cell's
/// rings are spawned again only when its shape changed. The chunk is
/// chained whole when it kept nothing, on a grid whose sides have length 0
/// (chained by their points), when more than `HEX_WHOLE` cells changed, or
/// once its chains pass twice what its last whole chaining made. Returns
/// the occluders that go, the chains to spawn and, with `own`, the chunks
/// whose cells own a side of one of its cells whose solidity changed
fn hex_chunk_chains(
  group: &mut Group,
  hex: HexCells,
  chunk: IVec2,
  own: bool,
  data: &[Option<&TileLayerData>],
  context: &Context<'_>,
) -> HexBuilt {
  let side = CHUNK_SIZE as i32;
  let last = side - 1;
  // `holds_chunk`: its cells, and the column and row past them either way,
  // are in an i32
  let min = chunk * side;
  let mut max = min + IVec2::splat(side);
  let bounds = group.bounds;
  if let Some(bounds) = bounds {
    max = max.min(bounds.max);
  }
  let (from, to) = ((min.x, min.y), (max.x, max.y));
  let corner = group.grid.grid().origin.as_dvec2() + hex.corner_of(min).as_dvec2();
  let Group {
    layers,
    kinds,
    shapes,
    boundary,
    chunks,
    edges,
    hex_chunks,
    ..
  } = group;
  let kept = hex_chunks.remove(&chunk);
  let mut classify = |at: IVec2, part: Part| {
    union_kinds(
      layers,
      data,
      (at, part),
      (&mut *kinds, &mut *shapes),
      &mut *boundary,
      context,
    )
  };
  let part = |(x0, y0): (i32, i32), (x1, y1): (i32, i32)| Part {
    min: IVec2::new(x0, y0),
    max: IVec2::new(x1, y1),
  };
  // Its own cells only when they changed, else as it kept them (a chunk
  // that kept nothing has none solid or shaped)
  let cells = own.then(|| classify(chunk, Part::WHOLE));
  let left = classify(chunk - IVec2::X, part((last, 0), (side, side)));
  let right = classify(chunk + IVec2::X, part((0, 0), (1, side)));
  let below = classify(chunk + IVec2::Y, part((0, 0), (side, 1)));
  let below_left = classify(chunk + IVec2::new(-1, 1), part((last, 0), (side, 1)));
  let below_right = classify(chunk + IVec2::ONE, part((0, 0), (1, 1)));
  let slot = |x: i32, y: i32| (y * side + x) as usize;
  let solid = |kind: TileCell| kind == TileCell::Solid;
  let mut now = Solidity::default();
  match (&cells, &kept) {
    (Some(cells), _) => {
      for (y, row) in now.cells.iter_mut().enumerate() {
        for x in 0..side {
          if solid(cells[slot(x, y as i32)]) {
            *row |= 1 << x;
          }
        }
      }
    }
    (None, Some(kept)) => now.cells = kept.read.cells,
    (None, None) => {}
  }
  for y in 0..side {
    if solid(left[slot(last, y)]) {
      now.left |= 1 << y;
    }
    if solid(right[slot(0, y)]) {
      now.right |= 1 << y;
    }
  }
  for x in 0..side {
    if solid(below[slot(x, 0)]) {
      now.below |= 1 << (x + 1);
    }
  }
  if solid(below_left[slot(last, 0)]) {
    now.below |= 1;
  }
  if solid(below_right[slot(0, 0)]) {
    now.below |= 1 << (side + 1);
  }
  // Its shape cells, when its own cells were read
  let now_shapes: Option<Vec<(u16, u16)>> = cells.as_ref().map(|cells| {
    let mut found = Vec::new();
    for y in 0..side {
      for x in 0..side {
        if let TileCell::Shape(shape) = cells[slot(x, y)]
          && bounds.is_none_or(|b| b.contains(min + IVec2::new(x, y)))
        {
          found.push((slot(x, y) as u16, shape));
        }
      }
    }
    found
  });
  let at = |x: i32, y: i32| {
    if now.at(x - min.x, y - min.y) {
      TileCell::Solid
    } else {
      TileCell::Empty
    }
  };
  let sides = HexSides::of(hex, bounds);
  let had = kept.is_some();
  let mut next = kept.unwrap_or_default();
  let changed = next.read.changed(&now);
  let mut gone = Vec::new();
  let mut spawn: Vec<Slotted> = Vec::new();

  // The chunks whose cells own a side of a rim cell whose solidity changed
  let mut marks = 0;
  if own {
    for (y, &row) in changed.cells.iter().enumerate() {
      // Only its rim cells have sides another chunk's cells own
      let rim = if y == 0 || y == SIDE - 1 {
        u32::MAX
      } else {
        1 | (1 << (SIDE - 1))
      };
      let mut bits = row & rim;
      while bits != 0 {
        let cell = min + IVec2::new(bits.trailing_zeros() as i32, y as i32);
        bits &= bits - 1;
        for s in 0..6 {
          let (across, owned) = sides.across(cell, s);
          let step = TileGrid::chunk_of(across) - chunk;
          if !owned && step != IVec2::ZERO {
            marks |= mark(step);
          }
        }
      }
    }
  }

  // The rings of the cells whose shape changed, each a closed chain wound
  // for lighting
  if let Some(now_shapes) = now_shapes {
    let old = std::mem::take(&mut next.shapes);
    let (mut i, mut j) = (0, 0);
    let mut rings = Vec::new();
    loop {
      let (went, came) = match (old.get(i).copied(), now_shapes.get(j).copied()) {
        (None, None) => break,
        (Some(o), Some(n)) if o.0 == n.0 => {
          i += 1;
          j += 1;
          if o.1 == n.1 {
            continue;
          }
          (Some(o.0), Some(n))
        }
        (Some(o), n) if n.is_none_or(|n| o.0 < n.0) => {
          i += 1;
          (Some(o.0), None)
        }
        (_, n) => {
          j += 1;
          (None, n)
        }
      };
      if let Some(went) = went {
        for ring in 0u32.. {
          match next.rings.remove(&((u32::from(went) << 16) | ring)) {
            Some(entity) => gone.push(entity),
            None => break,
          }
        }
      }
      if let Some((at_slot, shape)) = came {
        let place = i32::from(at_slot);
        let cell = min + IVec2::new(place % side, place / side);
        rings.clear();
        boundary.hex_rings(cell, TileCell::Shape(shape), min, &mut rings);
        for (ring, points) in rings.iter().filter(|ring| ring.len() > 1).enumerate() {
          // Its first point, then the others backwards, back to the first
          let n = points.len();
          let chain = chain_at(
            (0..=n)
              .map(|k| (corner + points[(n - k) % n]).as_vec2())
              .collect(),
          );
          let key = (u32::from(at_slot) << 16) | ring as u32;
          spawn.push((Some(Slot::Ring(key)), chain));
        }
      }
    }
    next.shapes = now_shapes;
  }

  // The sides: they run clockwise on screen round their solid cell,
  // lighting's the other way. A chunk's chains are fewer than its edges,
  // at most about 3,200, so an index fits a `u16`
  let px = |(x, y): Point| (corner + DVec2::new(f64::from(x), f64::from(y))).as_vec2();
  let side_chain = |placed: &[PlacedEdge], chain: &[u32]| {
    chain_at(
      chain_points(placed, chain)
        .into_iter()
        .rev()
        .map(px)
        .collect(),
    )
  };
  let live = next.chains.iter().flatten().count();
  let whole =
    !had || hex.point_sides() || changed.count() > HEX_WHOLE || live > 2 * next.whole.max(1);
  if whole {
    // Every chain of its sides goes: those it kept, or with nothing kept
    // every occluder it has (none of its cells was shaped: no rings)
    if had {
      gone.extend(next.chains.drain(..).flatten());
    } else {
      gone.extend(chunks.get(&chunk).into_iter().flatten().copied());
    }
    let empty = own && now.none_inside();
    let mut placed = Vec::new();
    if own && !empty {
      boundary.placed_edges(at, from, to, bounds, &mut placed);
    } else {
      // Only its rim cells' sides can have changed: an empty chunk has no
      // other edges
      let kept_edges = if empty { None } else { edges.get(&chunk) };
      boundary.rebuilt_borders(
        at,
        from,
        to,
        bounds,
        kept_edges.map_or(&[][..], Vec::as_slice),
        &mut placed,
      );
    }
    next.chain_of.clear();
    let count = match boundary.hex_chain_edges(at, from, to, bounds, &placed) {
      // Chained by their cells where three meet at each corner
      Some(found) => {
        next.chain_of.resize(placed.len(), 0);
        for (id, chain) in found.iter().enumerate() {
          for &edge in chain {
            next.chain_of[edge as usize] = id as u16;
          }
          spawn.push((Some(Slot::Side(id as u16)), side_chain(&placed, chain)));
        }
        found.len()
      }
      // else by their points, whole at every build
      None => {
        let lit: Vec<(Point, Point)> = placed.iter().map(|&(_, a, b)| (b, a)).collect();
        let found = chains(&lit);
        for (id, chain) in found.iter().enumerate() {
          let chain = chain_at(chain.iter().map(|&p| px(p)).collect());
          spawn.push((Some(Slot::Side(id as u16)), chain));
        }
        found.len()
      }
    };
    next.chains = vec![None; count];
    next.whole = count;
    keep_edges(edges, chunk, placed);
  } else if changed.count() > 0 {
    let first = hex.corner_of(min);
    let point_of = |p: glam::I64Vec2| -> Option<Point> {
      let p = p.saturating_sub(first);
      Some((i32::try_from(p.x).ok()?, i32::try_from(p.y).ok()?))
    };
    let in_chunk = |cell: IVec2| {
      let local = cell - min;
      (0..side).contains(&local.x)
        && (0..side).contains(&local.y)
        && bounds.is_none_or(|b| b.contains(cell))
    };
    let note = |rows: &mut [u32; SIDE], cell: IVec2| {
      let local = cell - min;
      in_chunk(cell) && {
        rows[local.y as usize] |= 1 << local.x;
        true
      }
    };
    // The cells whose sides to read again (the owners of a changed cell's
    // sides), the cells whose edges can pass a changed cell's corner (it
    // and the cells beside it: three cells meet at each corner) and those
    // corners, of each changed cell one of whose sides the chunk owns
    let (mut dirty, mut near) = ([0u32; SIDE], [0u32; SIDE]);
    let mut corners: Vec<Point> = Vec::new();
    changed.each(|local| {
      let cell = min + local;
      let mut reached = false;
      for s in 0..6 {
        let (across, owned) = sides.across(cell, s);
        reached |= note(&mut dirty, if owned { cell } else { across });
      }
      if reached {
        note(&mut near, cell);
        for s in 0..6 {
          note(&mut near, sides.across(cell, s).0);
        }
        corners.extend(hex.corners(cell).into_iter().filter_map(point_of));
      }
    });
    corners.sort_unstable();
    corners.dedup();
    let marked = |rows: &[u32; SIDE], place: u32| {
      let (x, y) = ((place >> 3) & 0x1FFF, (place >> 16) as usize);
      y < SIDE && x < SIDE as u32 && (rows[y] >> x) & 1 != 0
    };
    let mut fresh = Vec::new();
    boundary.hex_cell_edges(
      at,
      from,
      to,
      bounds,
      |x, y| (0..side).contains(&y) && (0..side).contains(&x) && (dirty[y as usize] >> x) & 1 != 0,
      &mut fresh,
    );
    // The kept edges with those read again put in their place, in the
    // scan's order, each with its chain (`NEW` for an edge read again
    // that differs), and the chains reached: those that lost or turned an
    // edge, and those through a changed cell's corner
    const NEW: u16 = u16::MAX;
    let old = edges.remove(&chunk).unwrap_or_default();
    let mut placed = Vec::with_capacity(old.len() + fresh.len());
    let mut chain_of = Vec::with_capacity(old.len() + fresh.len());
    let mut reached = vec![false; next.chains.len()];
    let mut fresh = fresh.into_iter().peekable();
    for (k, &edge) in old.iter().enumerate() {
      while let Some(new) = fresh.next_if(|new| new.0 < edge.0) {
        placed.push(new);
        chain_of.push(NEW);
      }
      let id = next.chain_of.get(k).copied().unwrap_or(NEW);
      if !marked(&dirty, edge.0) {
        placed.push(edge);
        chain_of.push(id);
        continue;
      }
      match fresh.next_if(|new| new.0 == edge.0) {
        Some(new) if new == edge => {
          placed.push(edge);
          chain_of.push(id);
        }
        other => {
          if let Some(hit) = reached.get_mut(usize::from(id)) {
            *hit = true;
          }
          if let Some(new) = other {
            placed.push(new);
            chain_of.push(NEW);
          }
        }
      }
    }
    for new in fresh {
      placed.push(new);
      chain_of.push(NEW);
    }
    let reaches =
      |id: u16, reached: &[bool]| id == NEW || reached.get(usize::from(id)) != Some(&false);
    for (i, &(place, a, b)) in placed.iter().enumerate() {
      let id = chain_of[i];
      if !reaches(id, &reached)
        && marked(&near, place)
        && (corners.binary_search(&a).is_ok() || corners.binary_search(&b).is_ok())
      {
        reached[usize::from(id)] = true;
      }
    }
    // The chains reached go, and their edges and the new ones are chained
    // again
    let mut sub = Vec::new();
    let mut back = Vec::new();
    for (i, &edge) in placed.iter().enumerate() {
      if reaches(chain_of[i], &reached) {
        sub.push(edge);
        back.push(i);
      }
    }
    for (id, &hit) in reached.iter().enumerate() {
      if hit && let Some(entity) = next.chains[id].take() {
        gone.push(entity);
      }
    }
    // Free indices, the lowest first
    let mut free: Vec<u16> = (0..next.chains.len())
      .rev()
      .filter(|&id| next.chains[id].is_none())
      .map(|id| id as u16)
      .collect();
    let found = boundary
      .hex_chain_edges(at, from, to, bounds, &sub)
      .unwrap_or_default();
    for chain in &found {
      let id = free.pop().unwrap_or_else(|| {
        next.chains.push(None);
        (next.chains.len() - 1) as u16
      });
      for &k in chain {
        chain_of[back[k as usize]] = id;
      }
      spawn.push((Some(Slot::Side(id)), side_chain(&sub, chain)));
    }
    chain_of.shrink_to_fit();
    next.chain_of = chain_of;
    keep_edges(edges, chunk, placed);
  }
  next.read = now;
  if !now.none_inside() || !next.shapes.is_empty() {
    hex_chunks.insert(chunk, next);
  }
  HexBuilt { gone, spawn, marks }
}

/// Keeps `placed` as chunk `chunk`'s edges, for as long as the chunk is: no
/// room past them (a chunk with none left out)
fn keep_edges(
  edges: &mut FastMap<IVec2, Vec<PlacedEdge>>,
  chunk: IVec2,
  mut placed: Vec<PlacedEdge>,
) {
  if placed.is_empty() {
    edges.remove(&chunk);
  } else {
    placed.shrink_to_fit();
    edges.insert(chunk, placed);
  }
}

/// `point` (lattice units) taken through the lattice `to`, worked out in
/// f64 and rounded once
fn through(to: Affine2, point: DVec2) -> Vec2 {
  let (a, b) = (to.matrix2.x_axis.as_dvec2(), to.matrix2.y_axis.as_dvec2());
  (a * point.x + b * point.y + to.translation.as_dvec2()).as_vec2()
}

/// On a staggered grid, the kinds of `changed` (storage chunks of its
/// layers) set at their lattice cells in `group`'s kept kinds; returns the
/// lattice chunks whose kinds changed, in no set order, each once. A
/// lattice chunk left all `Empty` goes
fn rescatter(
  group: &mut Group,
  data: &[Option<&TileLayerData>],
  changed: &[IVec2],
  context: &Context<'_>,
) -> Vec<IVec2> {
  let Group {
    layers,
    kinds,
    shapes,
    boundary,
    lattice: Some((lattice, kept)),
    ..
  } = group
  else {
    return Vec::new();
  };
  let side = IVec2::splat(CHUNK_SIZE as i32);
  let mut due: FastSet<IVec2> = FastSet::default();
  for &chunk in changed {
    let now = union_kinds(
      layers,
      data,
      (chunk, Part::WHOLE),
      (&mut *kinds, &mut *shapes),
      &mut *boundary,
      context,
    );
    for (at, kind) in scatter(lattice, chunk, &now) {
      let (place, local) = (at.div_euclid(side), at.rem_euclid(side));
      let slot = (local.y * side.x + local.x) as usize;
      let cells = match kept.get_mut(&place) {
        Some(cells) => cells,
        None if kind == TileCell::Empty => continue,
        None => kept
          .entry(place)
          .or_insert_with(|| Box::new([TileCell::Empty; CELLS])),
      };
      if cells[slot] != kind {
        cells[slot] = kind;
        due.insert(place);
      }
    }
  }
  for place in &due {
    if kept
      .get(place)
      .is_some_and(|cells| cells.iter().all(|&c| c == TileCell::Empty))
    {
      kept.remove(place);
    }
  }
  due.into_iter().collect()
}

/// Despawns chunk `chunk`'s occluders `gone` with `commands`, newest
/// first, so each comes off the end of its parent's children (bevy finds a
/// child from the back), or every one of them for `None`, and spawns
/// `chains` as its occluders, children of the group's parent, each noted at
/// its slot in its hexagonal chunk's `HexKept`. Without `commands` (a first
/// build, which has none to despawn) keeps the chains for the landing
fn place(
  group: &mut Group,
  chunk: IVec2,
  gone: Option<Vec<Entity>>,
  chains: Vec<Slotted>,
  commands: Option<&mut Commands>,
) {
  let Some(commands) = commands else {
    if !chains.is_empty() {
      group.pending.push((chunk, chains));
    }
    return;
  };
  let list = group.chunks.entry(chunk).or_default();
  match gone {
    // A whole chunk of another grid, in the order they were spawned, as
    // ever: newest first measured no better there, within the edit guard's
    // spread on those grids
    None => {
      for occluder in list.drain(..) {
        commands.entity(occluder).try_despawn();
      }
    }
    Some(mut gone) if !gone.is_empty() => {
      gone.sort_unstable();
      for &occluder in list.iter().rev() {
        if gone.binary_search(&occluder).is_ok() {
          commands.entity(occluder).try_despawn();
        }
      }
      list.retain(|occluder| gone.binary_search(occluder).is_err());
    }
    Some(_) => {}
  }
  let mut kept = group.hex_chunks.get_mut(&chunk);
  for (slot, (centre, vertices)) in chains {
    let occluder = commands
      .spawn((
        Transform::from_translation(centre),
        LightOccluder2D::chain(vertices),
        MapOccluder { chunk },
        ParentPlaced,
        ChildOf(group.parent),
      ))
      .id();
    list.push(occluder);
    if let (Some(kept), Some(slot)) = (kept.as_deref_mut(), slot) {
      kept.record(slot, occluder);
    }
  }
  if list.is_empty() {
    group.chunks.remove(&chunk);
  }
}

/// Works out the chunks of `group` that changed since it last looked (and
/// those across a side that changed kind), spawning them with `commands`,
/// or keeping them in `pending` for the landing without
fn update(
  group: &mut Group,
  data: &[Option<&TileLayerData>],
  changed: Vec<IVec2>,
  context: &Context<'_>,
  mut commands: Option<&mut Commands>,
) {
  // A staggered grid's chunks are its lattice's: those its changed chunks'
  // cells changed in
  let changed = if group.lattice.is_some() {
    rescatter(group, data, &changed, context)
  } else {
    changed
  };
  // Row by row, so a chunk marks the ones right of and below it in time;
  // each with whether its own cells changed (else only those across a
  // border it owns did). A hexagonal chunk marks chunks left of and above
  // it too, which come next; one built already in this pass read the cells
  // as they now are, and is not built again
  let mut due: BTreeMap<(i32, i32), bool> = changed
    .into_iter()
    .filter(|&chunk| group.holds_chunk(chunk))
    .map(|chunk| ((chunk.y, chunk.x), true))
    .collect();
  if commands.is_none() {
    group.pending.clear();
  }
  let mut built: FastSet<IVec2> = FastSet::default();
  while let Some(((y, x), own)) = due.pop_first() {
    let chunk = IVec2::new(x, y);
    let marks = match group.hex {
      // Only the chains it changed
      Some(hex) => {
        built.insert(chunk);
        let HexBuilt { gone, spawn, marks } =
          hex_chunk_chains(group, hex, chunk, own, data, context);
        place(group, chunk, Some(gone), spawn, commands.as_deref_mut());
        marks
      }
      // Every chain of the chunk
      None => {
        let (chains, right, bottom) = chunk_chains(group, chunk, own, data, context);
        let chains = chains.into_iter().map(|chain| (None, chain)).collect();
        place(group, chunk, None, chains, commands.as_deref_mut());
        let mut marks = 0;
        if right {
          marks |= mark(IVec2::X);
        }
        if bottom {
          marks |= mark(IVec2::Y);
        }
        marks
      }
    };
    for dy in -1..=1 {
      for dx in -1..=1 {
        let step = IVec2::new(dx, dy);
        let next = chunk + step;
        if marks & mark(step) != 0 && group.holds_chunk(next) && !built.contains(&next) {
          due.entry((next.y, next.x)).or_insert(false);
        }
      }
    }
  }
}

/// The occluding layers among `layers` into `planned`, on every grid. An
/// occluding layer of hexagons the tracer does not take
/// (`HexCells::traceable`: over 16,384 px, or under 2 px) casts no
/// shadows, as it does not collide, and warns once (`warned`)
fn plan<'a>(
  map: Entity,
  layers: impl IntoIterator<Item = LayerView<'a>>,
  planned: &mut Vec<Lit>,
  warned: &mut FastSet<usize>,
) {
  planned.clear();
  for layer in layers {
    if layer.info.occludes.unwrap_or(layer.info.collides) {
      let hex = layer.data.grid.hex_cells();
      if hex.is_some_and(|hex| !hex.traceable()) {
        if warned.insert(layer.index) {
          crate::engine_warn!(
            module: "Tilemap",
            "map {map}: layer \"{}\" has hexagonal cells over 16,384 px or under 2 px, which cast no shadows",
            layer.info.name
          );
        }
        continue;
      }
      planned.push(Lit {
        entity: layer.entity,
        index: layer.index,
        grid: GridKey::of(&layer.data.grid),
        offset: [layer.info.offset.x.to_bits(), layer.info.offset.y.to_bits()],
        bounds: layer.data.tiles.bounds(),
        intgrid: layer.data.intgrid.is_some(),
      });
    }
  }
}

/// The groups of a map's occluding layers (`planned`, read through
/// `views`), every chunk of each worked out: spawned with `commands`, kept
/// for the landing without
fn build(
  planned: &[Lit],
  views: &[LayerView<'_>],
  context: &Context<'_>,
  mut commands: Option<&mut Commands>,
) -> Vec<Group> {
  let view = |index: usize| views.iter().find(|v| v.index == index);
  let mut groups: Vec<Group> = Vec::new();
  for lit in planned {
    let Some(view) = view(lit.index) else {
      continue;
    };
    let tracked = Tracked::unread((lit.index, lit.entity), view.data, context.overrides);
    match groups
      .iter_mut()
      .find(|g| g.grid == lit.grid && g.offset == lit.offset)
    {
      Some(group) => {
        // The largest bounds, or none with an unbounded layer
        group.bounds = group.bounds.zip(lit.bounds).map(|(a, b)| a.union(b));
        group.layers.push(tracked);
      }
      None => {
        let mut group = Group::new(lit);
        group.layers.push(tracked);
        groups.push(group);
      }
    }
  }
  for group in &mut groups {
    let data: Vec<Option<&TileLayerData>> = group
      .layers
      .iter()
      .map(|t| view(t.index).map(|v| v.data))
      .collect();
    let mut changed = Vec::new();
    for (tracked, data) in group.layers.iter_mut().zip(&data) {
      if let Some(data) = data {
        changed.extend(tracked.read_all(data));
      }
    }
    changed.sort_unstable_by_key(|c| (c.y, c.x));
    changed.dedup();
    update(group, &data, changed, context, commands.as_deref_mut());
  }
  groups
}

/// What a map's first shadows read besides its layers
pub(crate) struct Inputs {
  pub(crate) overrides: Vec<(IntGridKey, TileCell)>,
  pub(crate) shapes: Arc<[Option<ShapeSource>]>,
  pub(crate) tilesets: Vec<TilesetStamp>,
}

/// A map's shadows worked out off the main thread, to land with its layers
pub(crate) struct Prepared {
  state: MapShadows,
}

/// The shadows of the map `map` as its layers (each with its place among
/// them) will spawn: every chunk's chains. Pure: any thread
pub(crate) fn prepare(map: Entity, layers: &[LayerView<'_>], inputs: Inputs) -> Prepared {
  let mut planned = Vec::new();
  let mut warned = FastSet::default();
  plan(map, layers.iter().copied(), &mut planned, &mut warned);
  let context = Context::new(&inputs.shapes, &inputs.overrides, Purpose::Occlusion);
  let groups = build(&planned, layers, &context, None);
  Prepared {
    state: MapShadows {
      layers: planned,
      tilesets: inputs.tilesets,
      overrides: inputs.overrides,
      shapes: inputs.shapes,
      groups,
      warned,
    },
  }
}

/// Spawns a map's prepared shadows under its layers (`layers`, the entities
/// its layers spawned as, by place), as the map lands
pub(crate) fn land(world: &mut World, map: Entity, layers: &[Entity], prepared: Prepared) {
  let Prepared { mut state } = prepared;
  for lit in &mut state.layers {
    if let Some(&entity) = layers.get(lit.index) {
      lit.entity = entity;
    }
  }
  for group in &mut state.groups {
    let Some(&parent) = layers.get(group.parent_layer) else {
      continue;
    };
    group.parent = parent;
    for tracked in &mut group.layers {
      tracked.relink(layers);
    }
    let pending = std::mem::take(&mut group.pending);
    let counts: Vec<(IVec2, usize)> = pending
      .iter()
      .map(|(chunk, chains)| (*chunk, chains.len()))
      .collect();
    // On a hexagonal grid, where each goes in its chunk's `HexKept`
    let slots: Vec<Option<Slot>> = match group.hex {
      Some(_) => pending
        .iter()
        .flat_map(|(_, chains)| chains.iter().map(|(slot, _)| *slot))
        .collect(),
      None => Vec::new(),
    };
    let bundles = pending.into_iter().flat_map(|(chunk, chains)| {
      chains.into_iter().map(move |(_, (centre, vertices))| {
        (
          Transform::from_translation(centre),
          LightOccluder2D::chain(vertices),
          MapOccluder { chunk },
          ParentPlaced,
          ChildOf(parent),
        )
      })
    });
    let mut spawned = world
      .spawn_batch(bundles)
      .collect::<Vec<Entity>>()
      .into_iter();
    let mut slots = slots.into_iter();
    for (chunk, count) in counts {
      let occluders: Vec<Entity> = spawned.by_ref().take(count).collect();
      match group.hex_chunks.get_mut(&chunk) {
        Some(kept) => {
          for (&occluder, slot) in occluders.iter().zip(slots.by_ref()) {
            if let Some(slot) = slot {
              kept.record(slot, occluder);
            }
          }
        }
        None => slots.by_ref().take(count).for_each(drop),
      }
      group.chunks.insert(chunk, occluders);
    }
  }
  if let Ok(mut entity) = world.get_entity_mut(map) {
    entity.insert(state);
  }
}

type Maps<'w, 's> = Query<
  'w,
  's,
  (
    Entity,
    &'static MapRoot,
    Option<&'static MapLayers>,
    Option<&'static mut MapShadows>,
    Has<Disabled>,
  ),
  Allow<Disabled>,
>;

/// The map's enabled tile layers, as planning and a build read them
fn views<'a>(
  map_layers: &'a MapLayers,
  layers: &'a Layers,
) -> impl Iterator<Item = LayerView<'a>> + 'a {
  map_layers
    .0
    .iter()
    .enumerate()
    .filter_map(|(index, &entity)| {
      let (info, data, disabled) = layers.get(entity).ok()?;
      (!disabled).then(|| LayerView {
        index,
        entity,
        info,
        data: data.into_inner(),
      })
    })
}

/// `PostUpdate`, before transforms propagate: every spawned map's occluding
/// layers keep their boundary chains, a chunk built again when its version
/// moves (or a side it shares changed kind). A disabled map, or one whose
/// root went, has none. An idle frame costs a look at each map's occluding
/// layers and tilesets
pub(crate) fn sync_map_occluders(
  mut maps: Maps,
  layers: Layers,
  (tile_maps, projects, tilesets): Stores,
  mut planned: Local<Vec<Lit>>,
  mut commands: Commands,
) {
  let tilesets = tilesets.as_deref();
  for (map, root, map_layers, mut state, disabled) in &mut maps {
    let Some(map_layers) = map_layers else {
      // The root was put over or taken away, and its layers with it
      if let Some(mut state) = state {
        state.clear(&mut commands);
        commands.entity(map).try_remove::<MapShadows>();
      }
      continue;
    };
    if disabled {
      // Its shadows go; enabled, it builds them again
      if let Some(mut state) = state.filter(|s| !s.groups.is_empty()) {
        let state = state.bypass_change_detection();
        state.clear(&mut commands);
        state.groups.clear();
        state.layers.clear();
      }
      continue;
    }
    // Without its map's asset the tiles cannot be read: what is there stays
    let Some(tile_map) = root_map(root, tile_maps.as_deref(), projects.as_deref()) else {
      continue;
    };
    let mut first_warnings = FastSet::default();
    let warned = match state.as_mut() {
      Some(state) => &mut state.bypass_change_detection().warned,
      None => &mut first_warnings,
    };
    plan(map, views(map_layers, &layers), &mut planned, warned);
    let same = state.as_mut().is_some_and(|s| {
      let s = s.bypass_change_detection();
      s.layers == *planned
        && s.overrides == root.intgrid_collision
        && shapes_kept(&mut s.tilesets, &s.shapes, tile_map, tilesets)
    });
    if same && let Some(mut state) = state {
      let state = state.bypass_change_detection();
      let context = Context::new(&state.shapes, &root.intgrid_collision, Purpose::Occlusion);
      for group in &mut state.groups {
        let changed = changed_chunks(&mut group.layers, &layers, context.overrides);
        if !changed.is_empty() {
          let data = layer_data(&group.layers, &layers);
          let data: Vec<Option<&TileLayerData>> = data.iter().map(|d| d.as_deref()).collect();
          update(group, &data, changed, &context, Some(&mut commands));
        }
      }
      continue;
    }
    // Built at once
    let shapes = sources(tile_map, tilesets);
    let context = Context::new(&shapes, &root.intgrid_collision, Purpose::Occlusion);
    let views: Vec<LayerView<'_>> = views(map_layers, &layers).collect();
    let groups = build(&planned, &views, &context, Some(&mut commands));
    let warned = match &state {
      Some(old) => old.warned.clone(),
      None => first_warnings,
    };
    let fresh = MapShadows {
      layers: planned.clone(),
      tilesets: stamps(tile_map, tilesets),
      overrides: root.intgrid_collision.clone(),
      shapes: Arc::clone(&shapes),
      groups,
      warned,
    };
    match state {
      Some(mut state) => {
        state.clear(&mut commands);
        *state = fresh;
      }
      None => {
        commands.entity(map).try_insert(fresh);
      }
    }
  }
}

#[cfg(test)]
mod tests {
  use std::collections::BTreeMap;
  use std::sync::Arc;

  use glam::{IVec2, UVec2, Vec2};

  use super::*;
  use crate::core::asset::{Assets, Handle};
  use crate::core::ecs::{
    ChildOf, Disabled, Entity, GlobalTransform, Has, Or, Transform, With, Without, World,
  };
  use crate::core::lighting::{Light2D, LightOccluder2D, push_shadow_volumes};
  use crate::core::physics::{Collider, Physics, TileCell, TilePart};
  use crate::core::render::batch::Triangles;
  use crate::core::tilemap::test_util::{MapWorld, layers_of, load_map as load};
  use crate::core::tilemap::{
    IntGrid, LayerKind, MapRoot, MapsView, Tile, TileData, TileMap, TileShape, Tileset,
    TilesetImage, TilesetSource,
  };

  /// An occluder: its entity, chunk, where its entity sits in its parent's
  /// space and its vertices around it
  #[derive(Debug, Clone)]
  struct Found {
    entity: Entity,
    chunk: IVec2,
    at: Vec2,
    occluder: LightOccluder2D,
  }

  fn occluders_of(world: &mut World, parent: Entity) -> Vec<Found> {
    let mut found: Vec<Found> = world
      .query::<(Entity, &ChildOf, &MapOccluder, &Transform, &LightOccluder2D)>()
      .iter(world)
      .filter(|(_, child_of, ..)| child_of.parent() == parent)
      .map(|(entity, _, marker, transform, occluder)| Found {
        entity,
        chunk: marker.chunk,
        at: transform.translation,
        occluder: occluder.clone(),
      })
      .collect();
    found.sort_by_key(|f| f.entity);
    found
  }

  fn count<C: Component>(world: &mut World) -> usize {
    world.query::<&C>().iter(world).count()
  }

  type Edge = ((i32, i32), (i32, i32));

  /// Every edge of `found` in its parent's space, cut into whole-pixel
  /// steps (so edges compare however chains split and merge them), each
  /// with how many times it is there
  fn steps(found: &[Found]) -> BTreeMap<Edge, usize> {
    let mut all = BTreeMap::new();
    for f in found {
      for (a, b) in f.occluder.edges() {
        let (a, b) = (a + f.at, b + f.at);
        let rounded = |v: Vec2| {
          assert!(
            (v - v.round()).abs().max_element() < 1e-3,
            "{v} off the pixel grid"
          );
          (v.x.round() as i32, v.y.round() as i32)
        };
        add_steps(&mut all, rounded(a), rounded(b));
      }
    }
    all
  }

  fn gcd(a: i32, b: i32) -> i32 {
    if b == 0 { a.abs() } else { gcd(b, a % b) }
  }

  fn add_steps(all: &mut BTreeMap<Edge, usize>, a: (i32, i32), b: (i32, i32)) {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let n = gcd(dx, dy).max(1);
    for i in 0..n {
      let from = (a.0 + dx * i / n, a.1 + dy * i / n);
      let to = (a.0 + dx * (i + 1) / n, a.1 + dy * (i + 1) / n);
      *all.entry((from, to)).or_insert(0) += 1;
    }
  }

  /// The steps of a ring of corners (px) as lighting winds them: each edge
  /// from a corner to the next and the last back to the first
  fn ring(corners: &[(i32, i32)]) -> BTreeMap<Edge, usize> {
    let mut all = BTreeMap::new();
    for (i, &a) in corners.iter().enumerate() {
      add_steps(&mut all, a, corners[(i + 1) % corners.len()]);
    }
    all
  }

  fn merged(rings: &[BTreeMap<Edge, usize>]) -> BTreeMap<Edge, usize> {
    let mut all = BTreeMap::new();
    for ring in rings {
      for (edge, n) in ring {
        *all.entry(*edge).or_insert(0) += n;
      }
    }
    all
  }

  /// Every occluder is an open chain whose entity sits at the centre of its
  /// corners' box
  fn assert_placed(found: &[Found]) {
    for f in found {
      let vertices = f.occluder.vertices();
      assert!(!f.occluder.is_closed(), "a chain: {f:?}");
      let min = vertices.iter().copied().fold(Vec2::MAX, Vec2::min);
      let max = vertices.iter().copied().fold(Vec2::MIN, Vec2::max);
      assert!(
        ((min + max) * 0.5).abs().max_element() < 1e-3,
        "off its geometry: {f:?}"
      );
    }
  }

  /// 16 px tiles: a solid block (0), a half block (1), a slope filled at its
  /// bottom right (2), a one-way block (3), and a block whose only shape is a
  /// small occluder (4)
  fn blocks() -> Tileset {
    let mut set = Tileset::new("blocks", UVec2::splat(16), TilesetImage::Collection);
    let shaped = |points: Vec<Vec2>, one_way: bool, occluder: bool| TileData {
      shapes: vec![TileShape {
        points,
        one_way,
        occluder,
        ..TileShape::default()
      }],
      ..TileData::default()
    };
    let rect = |x: f32, y: f32, w: f32, h: f32| {
      vec![
        Vec2::new(x, y),
        Vec2::new(x + w, y),
        Vec2::new(x + w, y + h),
        Vec2::new(x, y + h),
      ]
    };
    set.insert_tile(0, shaped(rect(0.0, 0.0, 16.0, 16.0), false, false));
    set.insert_tile(1, shaped(rect(0.0, 8.0, 16.0, 8.0), false, false));
    let slope = vec![
      Vec2::new(16.0, 0.0),
      Vec2::new(16.0, 16.0),
      Vec2::new(0.0, 16.0),
    ];
    set.insert_tile(2, shaped(slope, false, false));
    set.insert_tile(3, shaped(rect(0.0, 0.0, 16.0, 16.0), true, false));
    set.insert_tile(4, shaped(rect(4.0, 2.0, 8.0, 12.0), false, true));
    set
  }

  /// A map of `size` 16 px cells over `blocks`, one layer called "ground"
  /// with a solid floor along its bottom row, then `edit`
  fn floor_map(world: &mut World, size: UVec2, edit: impl FnOnce(&mut TileMap)) -> Handle<TileMap> {
    let mut map = TileMap::new(size, UVec2::splat(16));
    let set = map.add_tileset(TilesetSource::Embedded(Arc::new(blocks())));
    let layer = map.add_tile_layer("ground");
    if let Some(grid) = map.tiles_mut(layer) {
      for x in 0..size.x as i32 {
        grid.set(IVec2::new(x, size.y as i32 - 1), Some(Tile::new(set, 0)));
      }
    }
    edit(&mut map);
    world.resource_mut::<Assets<TileMap>>().add(map)
  }

  fn put(map: &mut TileMap, layer: usize, cell: (i32, i32), id: u32) {
    if let Some(grid) = map.tiles_mut(layer) {
      grid.set(IVec2::new(cell.0, cell.1), Some(Tile::new(0, id)));
    }
  }

  /// `chains` as it was, each point found through a map (the reference)
  fn chains_through_a_map(edges: &[(Point, Point)]) -> Vec<Vec<Point>> {
    // Edge indices by start point, each point's in the order given
    let mut sorted: Vec<(Point, u32)> = edges
      .iter()
      .enumerate()
      .map(|(i, &(a, _))| (a, i as u32))
      .collect();
    sorted.sort_unstable();
    let order: Vec<u32> = sorted.iter().map(|&(_, i)| i).collect();
    // Each start point (sorted) and its run of `order`, from its first edge
    // not yet used
    let mut points: Vec<Point> = Vec::new();
    let mut runs: Vec<(u32, u32)> = Vec::new();
    for (k, &(point, _)) in sorted.iter().enumerate() {
      if points.last() == Some(&point) {
        if let Some(run) = runs.last_mut() {
          run.1 = k as u32 + 1;
        }
      } else {
        points.push(point);
        runs.push((k as u32, k as u32 + 1));
      }
    }
    drop(sorted);
    let mut run_of: FastMap<Point, u32> = FastMap::default();
    run_of.reserve(points.len());
    for (run, &point) in points.iter().enumerate() {
      run_of.insert(point, run as u32);
    }
    // The edges arriving at each start point
    let mut arriving = vec![0u32; points.len()];
    for &(_, b) in edges {
      if let Some(&run) = run_of.get(&b) {
        arriving[run as usize] += 1;
      }
    }
    let mut used = vec![false; edges.len()];
    // The first edge leaving `at` not used yet, taken
    let take = |runs: &mut [(u32, u32)], used: &mut [bool], at: Point| -> Option<usize> {
      let run = *run_of.get(&at)? as usize;
      let (first, end) = &mut runs[run];
      // A run's used edges are its first ones: skip them
      while *first < *end && used[order[*first as usize] as usize] {
        *first += 1;
      }
      let found = (*first..*end)
        .map(|j| order[j as usize] as usize)
        .find(|&i| !used[i])?;
      used[found] = true;
      Some(found)
    };
    // Heads first: the points more edges leave than arrive at, in order
    let heads: Vec<Point> = (0..points.len())
      .filter(|&run| runs[run].1 - runs[run].0 > arriving[run])
      .map(|run| points[run])
      .collect();
    // then whatever edges are left, in order
    let starts = heads.into_iter().chain(edges.iter().map(|&(a, _)| a));
    let mut out = Vec::new();
    for start in starts {
      // A start with edges left starts chains until it has none
      while let Some(first) = take(&mut runs, &mut used, start) {
        let mut chain = vec![start, edges[first].1];
        let mut at = edges[first].1;
        while let Some(next) = take(&mut runs, &mut used, at) {
          let to = edges[next].1;
          // A straight run is one edge
          let (p, q) = (chain[chain.len() - 2], chain[chain.len() - 1]);
          let (u, v) = (
            (i64::from(q.0 - p.0), i64::from(q.1 - p.1)),
            (i64::from(to.0 - q.0), i64::from(to.1 - q.1)),
          );
          if u.0 * v.1 - u.1 * v.0 == 0 && u.0 * v.0 + u.1 * v.1 > 0 {
            chain.pop();
          }
          chain.push(to);
          at = to;
        }
        out.push(chain);
      }
    }
    out
  }

  #[test]
  fn chains_found_by_place_are_those_found_through_a_map() {
    // Random directed edges on a small lattice (repeated, branching, closed
    // and open), and the boundary edges of random blocks of cells
    let mut seed = 0x2545_f491_u32;
    let mut next = |n: u32| {
      seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
      (seed >> 8) % n
    };
    for round in 0..300 {
      let count = 1 + next(80) as usize;
      let side = 2 + next(6) as i32;
      let edges: Vec<(Point, Point)> = (0..count)
        .map(|_| {
          let p = |next: &mut dyn FnMut(u32) -> u32| {
            (next(side as u32) as i32 * 7, next(side as u32) as i32 * 7)
          };
          (p(&mut next), p(&mut next))
        })
        .filter(|(a, b)| a != b)
        .collect();
      assert_eq!(
        chains(&edges),
        chains_through_a_map(&edges),
        "round {round}"
      );
    }
    let boundary = Boundary::default();
    let kinds = [
      TileCell::Solid,
      TileCell::Empty,
      TileCell::Empty,
      TileCell::OneWay,
    ];
    for round in 0..100 {
      let cells: Vec<TileCell> = (0..64).map(|_| kinds[next(4) as usize]).collect();
      let at = |x: i32, y: i32| {
        if (0..8).contains(&x) && (0..8).contains(&y) {
          cells[(y * 8 + x) as usize]
        } else {
          TileCell::Empty
        }
      };
      let mut placed = Vec::new();
      boundary.placed_edges(at, (0, 0), (9, 9), None, &mut placed);
      let edges: Vec<(Point, Point)> = placed.iter().map(|&(_, a, b)| (b, a)).collect();
      assert!(!edges.is_empty());
      assert_eq!(
        chains(&edges),
        chains_through_a_map(&edges),
        "block {round}"
      );
    }
  }

  #[test]
  fn occluders_are_the_boundary_of_what_occludes() {
    let mut maps = MapWorld::new();
    // A floor with a half block and a slope standing on it, a one-way block
    // in the air, and a block that only occludes with its small shape
    let handle = floor_map(&mut maps.world, UVec2::new(6, 3), |map| {
      put(map, 0, (1, 1), 1);
      put(map, 0, (3, 1), 2);
      put(map, 0, (5, 0), 3);
      put(map, 0, (0, 0), 4);
    });
    let map = maps.spawn(MapRoot::new(handle));
    let layer = layers_of(&mut maps.world, map)[0];
    let found = occluders_of(&mut maps.world, layer);
    assert_placed(&found);
    assert!(found.iter().all(|f| f.chunk == IVec2::ZERO));
    // Counter-clockwise on screen, as lighting winds polygons: the edges
    // between the floor and what stands on it cancel
    let ground = ring(&[
      (0, 32),
      (0, 48),
      (96, 48),
      (96, 32),
      (64, 32),
      (64, 16),
      (48, 32),
      (32, 32),
      (32, 24),
      (16, 24),
      (16, 32),
    ]);
    let platform = ring(&[(80, 0), (80, 16), (96, 16), (96, 0)]);
    let light_only = ring(&[(4, 2), (4, 14), (12, 14), (12, 2)]);
    assert_eq!(steps(&found), merged(&[ground, platform, light_only]));
  }

  #[test]
  fn occluders_meet_across_layers_and_chunk_borders() {
    let mut maps = MapWorld::new();
    let level = load(&mut maps.world, "tiled/maps/level.tmj");
    let map = maps.spawn(MapRoot::new(level));
    let layers = layers_of(&mut maps.world, map);
    // The floor's two layers occlude as one; decor does not occlude
    let found = occluders_of(&mut maps.world, layers[0]);
    assert_placed(&found);
    assert!(occluders_of(&mut maps.world, layers[1]).is_empty());
    assert!(occluders_of(&mut maps.world, layers[2]).is_empty());
    let all = steps(&found);
    let along = |x0: i32, x1: i32, y: i32| {
      all
        .keys()
        .any(|&((ax, ay), (bx, by))| ay == y && by == y && ax.min(bx) >= x0 && ax.max(bx) <= x1)
    };
    let down = |x: i32, y0: i32, y1: i32| {
      all
        .keys()
        .any(|&((ax, ay), (bx, by))| ax == x && bx == x && ay.min(by) >= y0 && ay.max(by) <= y1)
    };
    assert!(
      !down(192, 176, 192),
      "no seam where the floor's layers meet"
    );
    assert!(!along(288, 304, 176), "none under the half block");
    assert!(!along(320, 352, 176), "none under the peak of two slopes");
    assert!(!along(368, 384, 176), "none under the turned slope");
    assert!(down(0, 176, 192) && down(384, 176, 192), "the floor's ends");
    // Across a chunk border: a floor of 64 cells is one boundary
    let handle = floor_map(&mut maps.world, UVec2::new(64, 4), |_| {});
    let wide = maps.spawn(MapRoot::new(handle));
    let layer = layers_of(&mut maps.world, wide)[0];
    let found = occluders_of(&mut maps.world, layer);
    assert_placed(&found);
    assert_eq!(
      steps(&found),
      ring(&[(0, 48), (0, 64), (1024, 64), (1024, 48)])
    );
    let mut chunks: Vec<IVec2> = found.iter().map(|f| f.chunk).collect();
    chunks.sort_by_key(|c| (c.y, c.x));
    assert_eq!(chunks, [IVec2::ZERO, IVec2::new(1, 0)]);
  }

  /// Whether `point` is in a shadow `occluders` (each at its position) cast
  /// from a light at `from`
  fn shadowed(point: Vec2, from: Vec2, occluders: &[Found]) -> bool {
    shadowed_by(400.0, point, from, occluders)
  }

  /// Whether `point` is in a shadow `occluders` (each at its position) cast
  /// from a light of `radius` at `from`
  fn shadowed_by(radius: f32, point: Vec2, from: Vec2, occluders: &[Found]) -> bool {
    let light = Light2D::point(crate::core::ui::Color::WHITE, radius).with_shadows();
    let placed: Vec<(GlobalTransform, &LightOccluder2D)> = occluders
      .iter()
      .map(|f| {
        let at = GlobalTransform::from_transform(&Transform::from_translation(f.at));
        (at, &f.occluder)
      })
      .collect();
    let mut triangles = Triangles::default();
    push_shadow_volumes(from, &light, &placed, &mut triangles);
    triangles.indices.chunks(3).any(|t| {
      let corner = |i: u32| Vec2::from(triangles.vertices[i as usize].position);
      let (a, b, c) = (corner(t[0]), corner(t[1]), corner(t[2]));
      let side = |p: Vec2, q: Vec2| (q - p).perp_dot(point - p);
      let (s1, s2, s3) = (side(a, b), side(b, c), side(c, a));
      (s1 > 1e-3 && s2 > 1e-3 && s3 > 1e-3) || (s1 < -1e-3 && s2 < -1e-3 && s3 < -1e-3)
    })
  }

  #[test]
  fn a_small_light_at_the_end_of_a_chunk_long_wall_casts_its_shadow() {
    let mut maps = MapWorld::new();
    // A wall one cell thick along a whole chunk (x 0 to 512, y 32 to 48):
    // one chain, its entity at its middle, 256 px from either end
    let handle = floor_map(&mut maps.world, UVec2::new(32, 3), |_| {});
    let map = maps.spawn(MapRoot::new(handle));
    let layer = layers_of(&mut maps.world, map)[0];
    let found = occluders_of(&mut maps.world, layer);
    assert_eq!(found.len(), 1, "{found:?}");
    // A torch of 64 px just above its left end: the art under the wall is
    // in its shadow
    let torch = Vec2::new(20.0, 10.0);
    assert!(shadowed_by(64.0, Vec2::new(20.0, 56.0), torch, &found));
    assert!(!shadowed_by(64.0, Vec2::new(20.0, 24.0), torch, &found));
  }

  #[test]
  fn a_slope_beside_ground_casts_no_shadow_into_the_ground() {
    let mut maps = MapWorld::new();
    // A slope (x 48 to 64) and a half block (x 96 to 112) standing on a
    // floor (y 32 to 48)
    let handle = floor_map(&mut maps.world, UVec2::new(8, 3), |map| {
      put(map, 0, (3, 1), 2);
      put(map, 0, (6, 1), 1);
    });
    let map = maps.spawn(MapRoot::new(handle));
    let layer = layers_of(&mut maps.world, map)[0];
    let found = occluders_of(&mut maps.world, layer);
    // The floor's art under each stays lit from above them and from above
    // either side: no edge between a shape and the floor casts into it
    for (points, lights) in [
      (
        [Vec2::new(52.0, 36.0), Vec2::new(60.0, 44.0)],
        [
          Vec2::new(56.0, -40.0),
          Vec2::new(20.0, -60.0),
          Vec2::new(90.0, -60.0),
        ],
      ),
      (
        [Vec2::new(100.0, 34.0), Vec2::new(106.0, 44.0)],
        [
          Vec2::new(104.0, -40.0),
          Vec2::new(70.0, -60.0),
          Vec2::new(140.0, -60.0),
        ],
      ),
    ] {
      for point in points {
        for from in lights {
          assert!(
            !shadowed(point, from, &found),
            "{point} lit from {from} is in shadow"
          );
        }
      }
    }
    // A sanity check: behind the ground, away from a light above, is shadow
    assert!(shadowed(
      Vec2::new(60.0, 60.0),
      Vec2::new(60.0, -40.0),
      &found
    ));
  }

  #[test]
  fn map_occluders_cast_their_shadows_away_from_a_light() {
    let mut maps = MapWorld::new();
    let level = load(&mut maps.world, "tiled/maps/level.tmj");
    let map = maps.spawn(MapRoot::new(level));
    let layer = layers_of(&mut maps.world, map)[0];
    let found = occluders_of(&mut maps.world, layer);
    // Lit from above, the floor's top casts nothing and its bottom casts:
    // under the floor (y 192 on) is shadow, above it is not
    let above = Vec2::new(24.0, 100.0);
    assert!(shadowed(Vec2::new(24.0, 200.0), above, &found));
    assert!(!shadowed(Vec2::new(24.0, 170.0), above, &found));
    assert!(
      !shadowed(Vec2::new(24.0, 184.0), above, &found),
      "the floor itself"
    );
  }

  #[test]
  fn a_layer_occludes_as_it_says_else_as_it_collides() {
    let mut maps = MapWorld::new();
    let lit_only = floor_map(&mut maps.world, UVec2::new(4, 2), |map| {
      map.layers[0].info.collides = false;
      map.layers[0].info.occludes = Some(true);
    });
    let lit_only = maps.spawn(MapRoot::new(lit_only));
    let solid_only = floor_map(&mut maps.world, UVec2::new(4, 2), |map| {
      map.layers[0].info.occludes = Some(false);
    });
    let solid_only = maps.spawn(MapRoot::new(solid_only));
    let lit_only = layers_of(&mut maps.world, lit_only)[0];
    assert_eq!(occluders_of(&mut maps.world, lit_only).len(), 1);
    let solid_only = layers_of(&mut maps.world, solid_only)[0];
    assert!(occluders_of(&mut maps.world, solid_only).is_empty());
  }

  #[test]
  fn an_int_grid_occludes_with_its_solid_cells_and_slopes() {
    let mut maps = MapWorld::new();
    let handle = floor_map(&mut maps.world, UVec2::new(4, 2), |map| {
      if let LayerKind::Tiles { intgrid, .. } = &mut map.layers[0].kind {
        let names = [
          (1, "Solid"),
          (2, "one_way"),
          (3, "slope_bottom_left"),
          (4, "Wall"),
        ];
        *intgrid = Some(IntGrid {
          size: UVec2::new(4, 2),
          values: vec![1, 3, 2, 4, 0, 0, 0, 0],
          names: names.iter().map(|(v, n)| (*v, n.to_string())).collect(),
        });
      }
    });
    let root = MapRoot::new(handle).with_intgrid_collision("wall", TileCell::Solid);
    let map = maps.spawn(root);
    let layer = layers_of(&mut maps.world, map)[0];
    let found = occluders_of(&mut maps.world, layer);
    assert_placed(&found);
    // Its IntGrid, not its tiles (the floor along the bottom row): the solid
    // cell and the slope beside it as one, and the wall it was told about;
    // the one-way cell has no area to occlude with
    let solid_and_slope = ring(&[(0, 0), (0, 16), (32, 16), (16, 0)]);
    let wall = ring(&[(48, 0), (48, 16), (64, 16), (64, 0)]);
    assert_eq!(steps(&found), merged(&[solid_and_slope, wall]));
  }

  #[test]
  fn the_edges_kept_a_chunk_take_no_room_past_them() {
    // A floor across two chunks, with blocks scattered over it: each
    // chunk's kept edges are shrunk to fit, as they live with the chunk
    let mut maps = MapWorld::new();
    let handle = floor_map(&mut maps.world, UVec2::new(64, 8), |map| {
      for x in (0..64).step_by(3) {
        put(map, 0, (x, x % 5), 0);
      }
    });
    let map = maps.spawn(MapRoot::new(handle));
    let layer = layers_of(&mut maps.world, map)[0];
    let block = Some(Tile::new(0, 0));
    assert!(MapsView::new(&mut maps.world).set_tile(map, 0, IVec2::new(33, 6), block));
    maps.frame();
    let Some(shadows) = maps.world.get::<MapShadows>(map) else {
      panic!("{map} casts shadows");
    };
    let kept: Vec<(usize, usize)> = shadows
      .groups
      .iter()
      .flat_map(|group| group.edges.values())
      .map(|edges| (edges.len(), edges.capacity()))
      .collect();
    assert!(
      kept.len() >= 2 && kept.iter().all(|&(len, _)| len > 4),
      "{kept:?}"
    );
    assert!(kept.iter().all(|&(len, room)| len == room), "{kept:?}");
    assert!(!occluders_of(&mut maps.world, layer).is_empty());
  }

  #[test]
  fn an_edit_rebuilds_only_its_chunk_s_occluders() {
    let mut maps = MapWorld::new();
    let handle = floor_map(&mut maps.world, UVec2::new(64, 32), |_| {});
    let map = maps.spawn(MapRoot::new(handle));
    let layer = layers_of(&mut maps.world, map)[0];
    let before = occluders_of(&mut maps.world, layer);
    assert_eq!(before.len(), 2, "{before:#?}");
    let far = before
      .iter()
      .find(|f| f.chunk == IVec2::new(1, 0))
      .expect("the far chunk's")
      .clone();
    assert!(MapsView::new(&mut maps.world).set_tile(
      map,
      "ground",
      IVec2::new(5, 10),
      Some(Tile::new(0, 1))
    ));
    maps.post_update();
    let after = occluders_of(&mut maps.world, layer);
    assert_placed(&after);
    assert!(
      after.iter().any(|f| f.entity == far.entity),
      "the far chunk kept its occluder"
    );
    let near: Vec<&Found> = after.iter().filter(|f| f.chunk == IVec2::ZERO).collect();
    assert_eq!(
      near.len(),
      2,
      "the floor's and the half block's: {after:#?}"
    );
    assert!(
      near
        .iter()
        .all(|f| before.iter().all(|b| b.entity != f.entity)),
      "the edited chunk's occluders were built again"
    );
    let half = ring(&[(80, 168), (80, 176), (96, 176), (96, 168)]);
    let floor = ring(&[(0, 496), (0, 512), (1024, 512), (1024, 496)]);
    assert_eq!(steps(&after), merged(&[half, floor]));
    // An edit on the chunk border rebuilds the chunk across it too: the
    // floor's last cell of the near chunk gone, the far chunk's floor has a
    // left end
    assert!(MapsView::new(&mut maps.world).set_tile(map, "ground", IVec2::new(31, 31), None));
    maps.post_update();
    let edged = occluders_of(&mut maps.world, layer);
    assert!(
      edged.iter().all(|f| f.entity != far.entity),
      "the far chunk was built again"
    );
    let half = ring(&[(80, 168), (80, 176), (96, 176), (96, 168)]);
    let near = ring(&[(0, 496), (0, 512), (496, 512), (496, 496)]);
    let far = ring(&[(512, 496), (512, 512), (1024, 512), (1024, 496)]);
    assert_eq!(steps(&edged), merged(&[half, near, far]));
  }

  #[test]
  fn a_disabled_map_casts_no_shadows_until_enabled() {
    let mut maps = MapWorld::new();
    let handle = floor_map(&mut maps.world, UVec2::new(4, 2), |_| {});
    let map = maps.spawn(MapRoot::new(handle));
    assert_eq!(count::<MapOccluder>(&mut maps.world), 1);
    maps.world.entity_mut(map).insert(Disabled);
    maps.frame();
    assert_eq!(count::<MapOccluder>(&mut maps.world), 0);
    assert!(MapsView::new(&mut maps.world).set_tile(
      map,
      "ground",
      IVec2::new(1, 0),
      Some(Tile::new(0, 1))
    ));
    maps.frame();
    assert_eq!(count::<MapOccluder>(&mut maps.world), 0);
    maps.world.entity_mut(map).remove::<Disabled>();
    maps.frame();
    // The half block put on the floor while disabled is part of its outline
    let layer = layers_of(&mut maps.world, map)[0];
    let found = occluders_of(&mut maps.world, layer);
    assert_eq!(
      steps(&found),
      ring(&[
        (0, 16),
        (0, 32),
        (64, 32),
        (64, 16),
        (32, 16),
        (32, 8),
        (16, 8),
        (16, 16)
      ])
    );
  }

  #[test]
  fn a_despawned_map_leaves_no_colliders_or_occluders() {
    let mut maps = MapWorld::new();
    let level = load(&mut maps.world, "tiled/maps/level.tmj");
    let map = maps.spawn(MapRoot::new(level));
    // A second map offset onto layer-level colliders
    let shifted = floor_map(&mut maps.world, UVec2::new(4, 2), |map| {
      map.layers[0].info.offset = Vec2::new(0.0, 4.0);
    });
    let other = maps.spawn(MapRoot::new(shifted));
    maps.frame();
    assert!(count::<TilePart>(&mut maps.world) > 0);
    assert!(count::<MapOccluder>(&mut maps.world) > 0);
    assert!(!maps.world.resource::<Physics>().collider_entries.is_empty());
    maps.world.despawn(map);
    maps.world.despawn(other);
    maps.frame();
    assert_eq!(count::<TilePart>(&mut maps.world), 0);
    assert_eq!(count::<Collider>(&mut maps.world), 0);
    assert_eq!(count::<MapOccluder>(&mut maps.world), 0);
    assert_eq!(count::<LightOccluder2D>(&mut maps.world), 0);
    assert!(maps.world.resource::<Physics>().collider_entries.is_empty());
  }

  /// A white floor over the window, no ambient light, then `setup`
  struct Lit {
    size: (u32, u32),
    setup: Box<dyn FnMut(&mut crate::core::scene::SceneContext) + Send>,
  }

  impl crate::core::scene::Scene for Lit {
    fn on_enter(&mut self, ctx: &mut crate::core::scene::SceneContext) {
      use crate::core::ecs::{Anchor, Renderable};
      let (w, h) = self.size;
      let floor = ctx.assets().add(crate::core::graphics::Image::solid_rect(
        w, h, 255, 255, 255, 255,
      ));
      ctx.world.spawn((
        Transform::from_xy(0.0, 0.0),
        Renderable::image(floor).with_anchor(Anchor::TopLeft),
      ));
      ctx
        .lighting()
        .set_ambient(crate::core::lighting::AmbientLight::with_intensity(0.0));
      (self.setup)(ctx);
    }
  }

  /// The linear red of each of `pixels` in a frame of a wall two 8 px cells
  /// thick across 64 cells (two chunks, the seam at x 256; y 160 to 176),
  /// lit from above right of the seam, its shadows of `alpha` and
  /// `softness`; the wall's occluders the map's, or one rectangle
  fn wall_frame(
    map_occludes: bool,
    alpha: u8,
    softness: Option<f32>,
    pixels: &[(u32, u32)],
  ) -> Option<Vec<f32>> {
    use crate::core::engine::EngineBuilder;
    use crate::core::testing::HeadlessEngine;
    use crate::core::ui::Color;
    let setup = move |ctx: &mut crate::core::scene::SceneContext| {
      let light = Light2D::point(Color::WHITE, 300.0).with_intensity(0.5);
      let light = match softness {
        Some(softness) => light.with_soft_shadows(softness),
        None => light.with_shadows(),
      };
      let light = light.with_shadow_color(Color::new(0, 0, 0, alpha));
      ctx.world.spawn((Transform::from_xy(304.0, 120.0), light));
      let mut set = Tileset::new("blocks", UVec2::splat(8), TilesetImage::Collection);
      let block = vec![
        Vec2::new(0.0, 0.0),
        Vec2::new(8.0, 0.0),
        Vec2::new(8.0, 8.0),
        Vec2::new(0.0, 8.0),
      ];
      set.insert_tile(
        0,
        TileData {
          shapes: vec![TileShape {
            points: block,
            ..TileShape::default()
          }],
          ..TileData::default()
        },
      );
      let mut map = TileMap::new(UVec2::new(64, 32), UVec2::splat(8));
      let set = map.add_tileset(TilesetSource::Embedded(Arc::new(set)));
      let layer = map.add_tile_layer("wall");
      if let Some(grid) = map.tiles_mut(layer) {
        grid.fill(
          IVec2::new(0, 20),
          IVec2::new(63, 21),
          Some(Tile::new(set, 0)),
        );
      }
      if !map_occludes {
        map.layers[layer].info.occludes = Some(false);
        ctx.world.spawn((
          Transform::from_xy(256.0, 168.0),
          LightOccluder2D::rect(512.0, 16.0),
        ));
      }
      let handle = ctx.assets().add(map);
      ctx.world.spawn(MapRoot::new(handle));
    };
    let scene = Lit {
      size: (512, 256),
      setup: Box::new(setup),
    };
    let builder = EngineBuilder::new().resolution(512, 256).start_scene(scene);
    let mut engine = HeadlessEngine::new_or_skip(builder)?;
    // The map lands once its landing comes back from the worker; then a
    // few frames for the shadows to draw
    engine.step_frames(2).expect("frames");
    crate::core::tilemap::test_util::step_until_maps_land(&mut engine);
    engine.step_frames(3).expect("frames");
    let frame = engine.frame().expect("a frame");
    let linear = |byte: u8| {
      let c = f32::from(byte) / 255.0;
      if c <= 0.04045 {
        c / 12.92
      } else {
        ((c + 0.055) / 1.055).powf(2.4)
      }
    };
    Some(
      pixels
        .iter()
        .map(|&(x, y)| linear(frame.get_pixel(x, y).0[0]))
        .collect(),
    )
  }

  #[test]
  fn abutting_map_occluders_cast_no_seam_shadows() {
    // In the wall where the seam's wedge would fall, in the wall away from
    // it, and under the wall behind the seam and beside it
    let pixels = [(250, 172), (220, 172), (226, 196), (200, 196)];
    for (alpha, softness) in [(255, None), (128, None), (255, Some(0.6)), (128, Some(0.6))] {
      let (Some(one), Some(map)) = (
        wall_frame(false, alpha, softness, &pixels),
        wall_frame(true, alpha, softness, &pixels),
      ) else {
        return;
      };
      assert!(one[0] > 0.05, "the wall's art is lit: {one:?}");
      for (i, (expected, got)) in one.iter().zip(&map).enumerate() {
        assert!(
          (expected - got).abs() <= 0.01 + expected * 0.02,
          "alpha {alpha}, softness {softness:?}, pixel {:?}: one rectangle {expected}, the map's occluders {got}",
          pixels[i]
        );
      }
    }
  }

  // ---- grids of a square lattice ------------------------------------------

  /// Tiles the size of `cell`: a diamond filling its box (0), a rectangle
  /// filling it (1) and the diamond's left half (2): a whole isometric or
  /// staggered cell, a whole oblique cell, and half an isometric or
  /// staggered cell cut along the lattice's diagonal
  fn lattice_blocks(cell: UVec2) -> Tileset {
    let (w, h) = (cell.x as f32, cell.y as f32);
    let mut set = Tileset::new("lattice", cell, TilesetImage::Collection);
    let shaped = |points: Vec<Vec2>| TileData {
      shapes: vec![TileShape {
        points,
        ..TileShape::default()
      }],
      ..TileData::default()
    };
    let (top, right, bottom, left) = (
      Vec2::new(w / 2.0, 0.0),
      Vec2::new(w, h / 2.0),
      Vec2::new(w / 2.0, h),
      Vec2::new(0.0, h / 2.0),
    );
    set.insert_tile(0, shaped(vec![top, right, bottom, left]));
    set.insert_tile(
      1,
      shaped(vec![
        Vec2::ZERO,
        Vec2::new(w, 0.0),
        Vec2::new(w, h),
        Vec2::new(0.0, h),
      ]),
    );
    set.insert_tile(2, shaped(vec![top, bottom, left]));
    set
  }

  /// A map of `shape` and `cell`-sized cells over `lattice_blocks` (`size`
  /// cells, or infinite for none), its layer "ground" holding `tiles`
  fn lattice_map(
    shape: crate::core::tilemap::GridShape,
    cell: UVec2,
    size: Option<UVec2>,
    tiles: &[(IVec2, u32)],
  ) -> TileMap {
    let mut map = match size {
      Some(size) => TileMap::new(size, cell),
      None => TileMap::infinite(cell),
    }
    .with_shape(shape);
    let set = map.add_tileset(TilesetSource::Embedded(Arc::new(lattice_blocks(cell))));
    let layer = map.add_tile_layer("ground");
    if let Some(grid) = map.tiles_mut(layer) {
      for &(cell, id) in tiles {
        grid.set(cell, Some(Tile::new(set, id)));
      }
    }
    map
  }

  /// The steps of the edges of the collider parts under `parent`, in its
  /// space, each turned round (as lighting winds what outlines bound)
  fn collision_steps(world: &mut World, parent: Entity) -> BTreeMap<Edge, usize> {
    use crate::core::physics::{ColliderShape, in_grid_space};
    let parts: Vec<Collider> = world
      .query::<(&ChildOf, &Collider, &Transform)>()
      .iter(world)
      .filter(|(child_of, ..)| child_of.parent() == parent)
      .map(|(_, c, at)| in_grid_space(c.clone(), at.translation))
      .collect();
    let rounded = |v: Vec2| {
      assert!(
        (v - v.round()).abs().max_element() < 1e-3,
        "{v} off the pixel grid"
      );
      (v.x.round() as i32, v.y.round() as i32)
    };
    let mut all = BTreeMap::new();
    for part in parts {
      let edges: Vec<(Vec2, Vec2)> = match &part.shape {
        ColliderShape::Polyline { points } => points.windows(2).map(|w| (w[0], w[1])).collect(),
        ColliderShape::Segments { points, segments } => segments
          .iter()
          .map(|&[a, b]| (points[a as usize], points[b as usize]))
          .collect(),
        other => panic!("an outline: {other:?}"),
      };
      for (a, b) in edges {
        add_steps(&mut all, rounded(b), rounded(a));
      }
    }
    all
  }

  #[test]
  fn shadows_follow_the_same_outlines_on_every_square_lattice_grid() {
    use crate::core::tilemap::{GridShape, StaggerAxis, StaggerIndex};
    let staggered = |axis, index| GridShape::Staggered { axis, index };
    // A blob of whole cells with a half cell beside it, a lone cell, and
    // (infinite maps) cells across chunk borders and at negative cells
    let blob = [(1, 1, 0), (2, 1, 0), (1, 2, 0), (2, 2, 2), (4, 4, 0)];
    let far = [(31, 6, 0), (32, 6, 0), (32, 7, 0), (-1, -1, 0), (-2, -1, 0)];
    let cases = [
      (
        GridShape::Isometric,
        UVec2::new(64, 32),
        Some(UVec2::new(6, 6)),
        false,
      ),
      (GridShape::Isometric, UVec2::new(32, 16), None, false),
      (
        staggered(StaggerAxis::Y, StaggerIndex::Odd),
        UVec2::new(64, 32),
        None,
        false,
      ),
      (
        staggered(StaggerAxis::X, StaggerIndex::Even),
        UVec2::new(32, 64),
        None,
        false,
      ),
      (
        staggered(StaggerAxis::Y, StaggerIndex::Even),
        UVec2::new(64, 32),
        Some(UVec2::new(6, 6)),
        false,
      ),
      (
        GridShape::Oblique {
          skew: IVec2::new(8, 12),
        },
        UVec2::splat(32),
        Some(UVec2::new(6, 6)),
        true,
      ),
      (
        GridShape::Oblique {
          skew: IVec2::new(-10, 0),
        },
        UVec2::splat(32),
        None,
        true,
      ),
    ];
    for (shape, cell, size, oblique) in cases {
      let label = format!("{shape:?} {cell} {size:?}");
      let tiles: Vec<(IVec2, u32)> = blob
        .iter()
        .chain(if size.is_none() { &far[..] } else { &[][..] })
        // An oblique cell is whole as a rectangle (its half a diamond is
        // not half the cell: it stays a shape)
        .map(|&(x, y, id)| (IVec2::new(x, y), if oblique && id == 0 { 1 } else { id }))
        .collect();
      let map = lattice_map(shape, cell, size, &tiles);
      let grid = map.grid();
      let mut maps = MapWorld::new();
      let handle = maps.world.resource_mut::<Assets<TileMap>>().add(map);
      let map = maps.spawn(MapRoot::new(handle));
      maps.frame();
      let layer = layers_of(&mut maps.world, map)[0];
      let found = occluders_of(&mut maps.world, layer);
      assert_placed(&found);
      let lit = steps(&found);
      assert!(!lit.is_empty(), "{label}");
      assert_eq!(lit, collision_steps(&mut maps.world, map), "{label}");
      // The lone cell's outline, wound for lighting, is its cell's on the
      // grid
      let outline = grid.cell_outline(IVec2::new(4, 4));
      let corners: Vec<(i32, i32)> = outline
        .points()
        .iter()
        .rev()
        .map(|p| (p.x.round() as i32, p.y.round() as i32))
        .collect();
      for (edge, n) in ring(&corners) {
        assert_eq!(
          lit.get(&edge),
          Some(&n),
          "{label}: cell (4, 4)'s edge {edge:?}"
        );
      }
    }
  }

  #[test]
  fn a_streamed_staggered_chunk_unloads_cleanly() {
    use crate::core::physics::TileColliders;
    use crate::core::tilemap::{GridShape, MapGrid, StaggerAxis, StaggerIndex, TileLayerData};
    let shape = GridShape::Staggered {
      axis: StaggerAxis::Y,
      index: StaggerIndex::Odd,
    };
    let cell = UVec2::new(64, 32);
    // A wall across the border of chunks (0, 0) and (1, 0), and blocks in
    // each: the second chunk's cells straddle lattice chunks
    let mut tiles: Vec<(IVec2, u32)> = (26..38).map(|x| (IVec2::new(x, 4), 0)).collect();
    tiles.extend([(3, 3), (40, 10), (45, 30), (33, 0)].map(|(x, y)| (IVec2::new(x, y), 0)));
    let left: Vec<(IVec2, u32)> = tiles.iter().copied().filter(|(c, _)| c.x < 32).collect();
    let tiles_list = tiles.clone();
    let mut maps = MapWorld::new();
    let mut add = |map: TileMap| maps.world.resource_mut::<Assets<TileMap>>().add(map);
    let (whole, half) = (
      add(lattice_map(shape, cell, None, &tiles)),
      add(lattice_map(shape, cell, None, &left)),
    );
    let streamed = maps.spawn(MapRoot::new(whole));
    let without = maps.spawn(MapRoot::new(half));
    maps.frame();
    // What a map holds: its colliders' cells and storage, its parts' edges,
    // and its occluders' edges and chunks
    type State = (
      Vec<(IVec2, TileCell)>,
      usize,
      BTreeMap<Edge, usize>,
      BTreeMap<Edge, usize>,
      Vec<IVec2>,
    );
    let state = |world: &mut World, map: Entity| -> State {
      let tiles = world.get::<TileColliders>(map).cloned().expect("colliders");
      let mut cells: Vec<(IVec2, TileCell)> = tiles.cells_in(tiles.extent()).collect();
      cells.sort_by_key(|(c, _)| (c.y, c.x));
      let layer = layers_of(world, map)[0];
      let found = occluders_of(world, layer);
      let mut chunks: Vec<IVec2> = found.iter().map(|f| f.chunk).collect();
      chunks.sort_by_key(|c| (c.y, c.x));
      chunks.dedup();
      (
        cells,
        tiles.chunk_count(),
        collision_steps(world, map),
        steps(&found),
        chunks,
      )
    };
    let loaded = state(&mut maps.world, streamed);
    let reference = state(&mut maps.world, without);
    assert_ne!(loaded, reference);
    // Its cells collide at their lattice cells
    let lattice = MapGrid::new(shape, cell)
      .lattice()
      .expect("a staggered lattice");
    let tiles = maps
      .world
      .get::<TileColliders>(streamed)
      .cloned()
      .expect("colliders");
    for (at, _) in &tiles_list {
      assert_eq!(
        tiles.get(lattice.lattice_cell(*at)),
        TileCell::Solid,
        "{at}"
      );
    }

    // The stream unloads chunk (1, 0) of the layer
    let layer = layers_of(&mut maps.world, streamed)[0];
    let cells = maps
      .world
      .get_mut::<TileLayerData>(layer)
      .and_then(|mut data| data.tiles.unload_chunk(IVec2::new(1, 0)))
      .expect("chunk (1, 0) was there");
    maps.frame();
    maps.frame();
    assert_eq!(
      state(&mut maps.world, streamed),
      reference,
      "as if never loaded"
    );

    // and loads it back
    if let Some(mut data) = maps.world.get_mut::<TileLayerData>(layer) {
      assert!(data.tiles.load_chunk(IVec2::new(1, 0), cells));
    }
    maps.frame();
    maps.frame();
    assert_eq!(state(&mut maps.world, streamed), loaded, "as it was");
  }

  #[test]
  fn a_streamed_hex_chunk_unloads_cleanly() {
    use crate::core::physics::TileColliders;
    use crate::core::tilemap::TileLayerData;
    // A wall across the border of chunks (0, 0) and (1, 0), and hexagons
    // in each (whole ones: tile 0 of `whole_hex_set`)
    let mut tiles: Vec<IVec2> = (26..38).map(|x| IVec2::new(x, 4)).collect();
    tiles.extend([(3, 3), (40, 10), (45, 30), (33, 0)].map(|(x, y)| IVec2::new(x, y)));
    let left: Vec<IVec2> = tiles.iter().copied().filter(|c| c.x < 32).collect();
    let mut maps = MapWorld::new();
    let mut add = |map: TileMap| maps.world.resource_mut::<Assets<TileMap>>().add(map);
    let (whole, half) = (add(hex_map(&tiles)), add(hex_map(&left)));
    let streamed = maps.spawn(MapRoot::new(whole));
    let without = maps.spawn(MapRoot::new(half));
    maps.frame();
    // What a map holds: its colliders' cells and storage, its parts'
    // edges, and its occluders' edges and chunks
    type State = (
      Vec<(IVec2, TileCell)>,
      usize,
      BTreeMap<Edge, usize>,
      BTreeMap<Edge, usize>,
      Vec<IVec2>,
    );
    let state = |world: &mut World, map: Entity| -> State {
      let tiles = world.get::<TileColliders>(map).cloned().expect("colliders");
      let mut cells: Vec<(IVec2, TileCell)> = tiles.cells_in(tiles.extent()).collect();
      cells.sort_by_key(|(c, _)| (c.y, c.x));
      let layer = layers_of(world, map)[0];
      let found = occluders_of(world, layer);
      assert_placed(&found);
      let mut chunks: Vec<IVec2> = found.iter().map(|f| f.chunk).collect();
      chunks.sort_by_key(|c| (c.y, c.x));
      chunks.dedup();
      (
        cells,
        tiles.chunk_count(),
        collision_steps(world, map),
        steps(&found),
        chunks,
      )
    };
    let loaded = state(&mut maps.world, streamed);
    let reference = state(&mut maps.world, without);
    assert_ne!(loaded, reference);
    assert!(!loaded.2.is_empty());
    for at in &tiles {
      assert!(loaded.0.contains(&(*at, TileCell::Solid)), "{at}");
    }
    // Its shadows are its colliders' edges, wound for lighting
    assert_eq!(loaded.3, loaded.2);
    assert_eq!(reference.3, reference.2);

    // The stream unloads chunk (1, 0) of the layer: the wall's edges close
    // at the chunk's edge, as if its cells were never there
    let layer = layers_of(&mut maps.world, streamed)[0];
    let cells = maps
      .world
      .get_mut::<TileLayerData>(layer)
      .and_then(|mut data| data.tiles.unload_chunk(IVec2::new(1, 0)))
      .expect("chunk (1, 0) was there");
    maps.frame();
    maps.frame();
    assert_eq!(
      state(&mut maps.world, streamed),
      reference,
      "as if never loaded"
    );

    // and loads it back
    if let Some(mut data) = maps.world.get_mut::<TileLayerData>(layer) {
      assert!(data.tiles.load_chunk(IVec2::new(1, 0), cells));
    }
    maps.frame();
    maps.frame();
    assert_eq!(state(&mut maps.world, streamed), loaded, "as it was");
  }

  // ---- hexagonal grids --------------------------------------------------------

  /// The hexagonal collision fixtures (`make-fixtures.js`' `collideHex`):
  /// each map's files (JSON, then XML) and its XML tilesets, which the
  /// fixture source leaves out
  const HEX_FIXTURES: [([&str; 2], [&str; 2]); 2] = [
    (
      [
        "tiled/maps/collide-hex-y-odd.tmj",
        "tiled/xml/maps/collide-hex-y-odd.tmx",
      ],
      [
        "tiled/xml/tilesets/collide-hex-y.tsx",
        "tiled/xml/tilesets/collide-hex-y-tall.tsx",
      ],
    ),
    (
      [
        "tiled/maps/collide-hex-x-even.tmj",
        "tiled/xml/maps/collide-hex-x-even.tmx",
      ],
      [
        "tiled/xml/tilesets/collide-hex-x.tsx",
        "tiled/xml/tilesets/collide-hex-x-tall.tsx",
      ],
    ),
  ];

  /// The hexagonal grid the infinite hex maps here are on: 120 x 140 px
  /// hexagons with sides of 70 in rows, the odd ones shifted
  fn hex_shape() -> crate::core::tilemap::GridShape {
    use crate::core::tilemap::{GridShape, StaggerAxis, StaggerIndex};
    GridShape::Hexagonal {
      axis: StaggerAxis::Y,
      index: StaggerIndex::Odd,
      side: 70,
    }
  }

  /// An infinite map on `hex_shape` over `whole_hex_set`, its layer
  /// "ground" holding whole hexagons (tile 0) at `cells`
  fn hex_map(cells: &[IVec2]) -> TileMap {
    use crate::core::tilemap::test_util::whole_hex_set;
    let mut map = TileMap::infinite(UVec2::new(120, 140)).with_shape(hex_shape());
    let set = whole_hex_set(&map.grid());
    let set = map.add_tileset(TilesetSource::Embedded(Arc::new(set)));
    let layer = map.add_tile_layer("ground");
    if let Some(grid) = map.tiles_mut(layer) {
      for &at in cells {
        grid.set(at, Some(Tile::new(set, 0)));
      }
    }
    map
  }

  /// Where `entity`'s space sits in the world: its translation and its
  /// ancestors' (a map's layers and parts are moved, never turned)
  fn world_offset(world: &World, entity: Entity) -> Vec2 {
    let mut at = Vec2::ZERO;
    let mut next = Some(entity);
    while let Some(e) = next {
      at += world
        .get::<Transform>(e)
        .map_or(Vec2::ZERO, |t| t.translation);
      next = world.get::<ChildOf>(e).map(ChildOf::parent);
    }
    at
  }

  /// Every edge of the occluders under `parent` (each chain's consecutive
  /// points), in the world, turned back round: lighting winds what the
  /// outlines bound the other way
  fn lit_edges(world: &mut World, parent: Entity) -> Vec<(Vec2, Vec2)> {
    let base = world_offset(world, parent);
    occluders_of(world, parent)
      .iter()
      .flat_map(|f| {
        let at = base + f.at;
        f.occluder
          .edges()
          .map(move |(a, b)| (at + b, at + a))
          .collect::<Vec<_>>()
      })
      .collect()
  }

  /// Every edge of the collider parts under `parent`, in the world
  fn part_edges(world: &mut World, parent: Entity) -> Vec<(Vec2, Vec2)> {
    use crate::core::physics::{ColliderShape, in_grid_space};
    let base = world_offset(world, parent);
    let parts: Vec<Collider> = world
      .query::<(&ChildOf, &Collider, &Transform)>()
      .iter(world)
      .filter(|(child_of, ..)| child_of.parent() == parent)
      .map(|(_, c, at)| in_grid_space(c.clone(), at.translation))
      .collect();
    let mut all = Vec::new();
    for part in parts {
      match &part.shape {
        ColliderShape::Polyline { points } => {
          all.extend(points.windows(2).map(|w| (base + w[0], base + w[1])));
        }
        ColliderShape::Segments { points, segments } => all.extend(
          segments
            .iter()
            .map(|&[a, b]| (base + points[a as usize], base + points[b as usize])),
        ),
        other => panic!("an outline: {other:?}"),
      }
    }
    all
  }

  /// `a` and `b` hold the same edges, as a multiset, and the same points,
  /// each within 1/100 px of its match (a shape's ring need not lie on
  /// whole pixels)
  fn assert_same_edges(a: &[(Vec2, Vec2)], b: &[(Vec2, Vec2)], label: &str) {
    const NEAR: f32 = 1e-2;
    let near = |p: Vec2, q: Vec2| (p - q).abs().max_element() < NEAR;
    assert_eq!(a.len(), b.len(), "{label}: {a:?} against {b:?}");
    // `b` by its edges' first x, each of `a`'s looked for among those
    // within reach of its own (a dense map has thousands of edges)
    let mut order: Vec<usize> = (0..b.len()).collect();
    order.sort_by(|&i, &j| b[i].0.x.total_cmp(&b[j].0.x));
    let mut used = vec![false; b.len()];
    for &(p, q) in a {
      let start = order.partition_point(|&i| b[i].0.x <= p.x - NEAR);
      let found = order[start..]
        .iter()
        .copied()
        .take_while(|&i| b[i].0.x < p.x + NEAR)
        .find(|&i| !used[i] && near(b[i].0, p) && near(b[i].1, q));
      let Some(i) = found else {
        panic!("{label}: the edge {p} to {q} is not among {b:?}");
      };
      used[i] = true;
    }
    // Each edge matched one of the other's as many, so every point of each
    // is near one of the other's
  }

  /// The occluders among `found` that are closed chains (ending where they
  /// start), by their edge counts, sorted
  fn closed_chains(found: &[Found]) -> Vec<usize> {
    let mut closed: Vec<usize> = found
      .iter()
      .filter_map(|f| {
        let vertices = f.occluder.vertices();
        let (first, last) = (vertices.first()?, vertices.last()?);
        ((*first - *last).abs().max_element() < 1e-3).then(|| vertices.len() - 1)
      })
      .collect();
    closed.sort_unstable();
    closed
  }

  /// `entity`'s global transform as its ancestors' locals give it, from
  /// the root's stored global (a body's write-back lands after its step's
  /// propagation, so the root's `Transform` can be a step ahead of it)
  fn composed(world: &World, entity: Entity) -> GlobalTransform {
    match world.get::<ChildOf>(entity) {
      Some(child_of) => {
        let local = *world.get::<Transform>(entity).expect("a Transform");
        composed(world, child_of.parent()).mul_transform(&local)
      }
      None => *world
        .get::<GlobalTransform>(entity)
        .expect("a GlobalTransform"),
    }
  }

  /// Every part's and chain's `GlobalTransform` is its root's global times
  /// its ancestors' locals (1e-3); how many of each there are
  fn assert_carried(world: &mut World) -> (usize, usize) {
    let placed: Vec<(Entity, bool)> = world
      .query_filtered::<(Entity, Has<TilePart>), Or<(With<TilePart>, With<MapOccluder>)>>()
      .iter(world)
      .collect();
    let (mut parts, mut chains) = (0, 0);
    for (entity, is_part) in placed {
      let global = *world.get::<GlobalTransform>(entity).unwrap();
      let want = composed(world, entity);
      assert!(
        global.affine().abs_diff_eq(want.affine(), 1e-3),
        "{entity}: {global:?} against {want:?}"
      );
      if is_part {
        parts += 1;
      } else {
        chains += 1;
      }
    }
    (parts, chains)
  }

  #[test]
  fn a_map_on_a_kinematic_body_carries_its_parts_and_chains() {
    use crate::core::ecs::Velocity;
    use crate::core::physics::RigidBody;
    let clumps: Vec<IVec2> = [(5, 5), (6, 5), (6, 6), (40, 9), (41, 9)]
      .into_iter()
      .map(|(x, y)| IVec2::new(x, y))
      .collect();
    let mut maps = MapWorld::new();
    let handle = maps
      .world
      .resource_mut::<Assets<TileMap>>()
      .add(hex_map(&clumps));
    let map = maps.spawn(MapRoot::new(handle));
    maps.frame();
    maps
      .world
      .entity_mut(map)
      .insert((RigidBody::kinematic(), Velocity::new(60.0, 0.0)));
    for _ in 0..30 {
      maps.frame();
    }
    let moved = maps.world.get::<Transform>(map).unwrap().translation.x;
    assert!(moved > 10.0, "physics moved the map root: {moved}");
    let (parts, chains) = assert_carried(&mut maps.world);
    assert!(parts > 0 && chains > 0, "{parts} parts, {chains} chains");
    // The parts' colliders in Rapier sit where their parts do, within a
    // step's travel (1 px): the globals follow the body's write-back a step
    // behind its colliders (pre-existing: the same without the marker)
    let physics = maps.world.resource::<Physics>();
    let mut checked = 0;
    for (part, entry) in physics.collider_entries.iter() {
      let Some(global) = maps.world.get::<GlobalTransform>(*part) else {
        continue;
      };
      if maps.world.get::<TilePart>(*part).is_none() {
        continue;
      }
      let at = physics
        .colliders
        .get(entry.handle)
        .unwrap()
        .position()
        .translation;
      let want = global.translation();
      assert!(
        (at.x - want.x).abs() < 1.5 && (at.y - want.y).abs() < 1.5,
        "{part}: the collider at {at:?}, the part at {want:?}"
      );
      checked += 1;
    }
    assert!(checked > 0);
  }

  #[test]
  fn a_moved_map_carries_its_hex_parts_and_chains() {
    let clumps: Vec<IVec2> = [(5, 5), (6, 5), (6, 6), (40, 9), (41, 9)]
      .into_iter()
      .map(|(x, y)| IVec2::new(x, y))
      .collect();
    for offset in [Vec2::ZERO, Vec2::new(13.0, -7.0)] {
      let mut map = hex_map(&clumps);
      map.layers[0].info.offset = offset;
      let mut maps = MapWorld::new();
      let handle = maps.world.resource_mut::<Assets<TileMap>>().add(map);
      let map = maps.spawn(MapRoot::new(handle));
      maps.frame();
      let layer = layers_of(&mut maps.world, map)[0];
      let before = lit_edges(&mut maps.world, layer);
      assert!(!before.is_empty());

      // Moved, turned and scaled in one frame
      let root = Transform::from_xy(100.0, 50.0)
        .with_rotation_degrees(30.0)
        .with_uniform_scale(2.0);
      *maps.world.get_mut::<Transform>(map).expect("root") = root;
      maps.frame();
      let at = GlobalTransform::from_transform(&root);
      assert_eq!(*maps.world.get::<GlobalTransform>(map).unwrap(), at);

      let (parts, chains) = assert_carried(&mut maps.world);
      assert!(parts > 0 && chains > 0, "{parts} parts, {chains} chains");

      let after: Vec<(Vec2, Vec2)> = maps
        .world
        .query::<(&MapOccluder, &GlobalTransform, &LightOccluder2D)>()
        .iter(&maps.world)
        .flat_map(|(_, global, occluder)| {
          let affine = global.affine();
          occluder
            .edges()
            .map(|(a, b)| (affine.transform_point2(b), affine.transform_point2(a)))
            .collect::<Vec<_>>()
        })
        .collect();
      let want: Vec<(Vec2, Vec2)> = before
        .iter()
        .map(|&(a, b)| {
          let to = at.affine();
          (to.transform_point2(a), to.transform_point2(b))
        })
        .collect();
      assert_same_edges(&after, &want, "moved shadows");
    }
  }

  #[test]
  fn an_interpolated_map_s_rebuilt_chains_get_previous_transforms() {
    use crate::core::ecs::{Interpolated, PreviousGlobalTransform};
    let clumps: Vec<IVec2> = [(5, 5), (6, 5), (6, 6)]
      .into_iter()
      .map(|(x, y)| IVec2::new(x, y))
      .collect();
    let mut maps = MapWorld::new();
    let handle = maps
      .world
      .resource_mut::<Assets<TileMap>>()
      .add(hex_map(&clumps));
    let map = maps.spawn(MapRoot::new(handle));
    maps.world.entity_mut(map).insert(Interpolated);
    maps.frame();
    let unplaced = |world: &mut World| {
      world
        .query_filtered::<Entity, (With<MapOccluder>, Without<PreviousGlobalTransform>)>()
        .iter(world)
        .count()
    };
    assert_eq!(unplaced(&mut maps.world), 0, "the landed chains");
    // A lone hexagon builds its chunk's chains again
    assert!(MapsView::new(&mut maps.world).set_tile(
      map,
      "ground",
      IVec2::new(10, 10),
      Some(Tile::new(0, 0))
    ));
    maps.frame();
    assert!(count::<MapOccluder>(&mut maps.world) > 1);
    assert_eq!(unplaced(&mut maps.world), 0, "the rebuilt chains");
  }

  #[test]
  fn shadows_follow_hex_outlines() {
    use crate::core::tilemap::test_util::world_with_xml;
    for (files, [set, tall]) in HEX_FIXTURES {
      for file in files {
        let (mut maps, map, _) = world_with_xml(&[files[1], set, tall], file);
        let layer = layers_of(&mut maps.world, map)[0];
        let found = occluders_of(&mut maps.world, layer);
        assert_placed(&found);
        assert!(found.iter().all(|f| f.chunk == IVec2::ZERO), "{file}");
        // The clump of whole hexagons and each shape tile's ring: the
        // colliders' edges, wound for lighting
        let parts = part_edges(&mut maps.world, map);
        assert_eq!(parts.len(), 12 + 5 * 3, "{file}");
        assert_same_edges(&lit_edges(&mut maps.world, layer), &parts, file);
        // The clump's border is one closed chain, and so is each ring
        assert_eq!(closed_chains(&found), [3, 3, 3, 3, 3, 12], "{file}");
        assert_eq!(found.len(), 6, "{file}");
      }
    }
  }

  #[test]
  fn an_edit_rebuilds_only_its_chunk_s_occluders_on_a_hex_map() {
    // A clump well inside each of chunks (-1, -1), (0, 0) and (1, 0)
    let clumps: Vec<IVec2> = [(5, 5), (6, 5), (6, 6)]
      .into_iter()
      .flat_map(|(x, y)| [(x, y), (x + 35, y), (x - 32, y - 32)])
      .map(|(x, y)| IVec2::new(x, y))
      .collect();
    let map = hex_map(&clumps);
    let grid = map.grid();
    let mut maps = MapWorld::new();
    let handle = maps.world.resource_mut::<Assets<TileMap>>().add(map);
    let map = maps.spawn(MapRoot::new(handle));
    let layer = layers_of(&mut maps.world, map)[0];
    let in_chunk = |found: &[Found], chunk: (i32, i32)| -> Vec<Entity> {
      let chunk = IVec2::new(chunk.0, chunk.1);
      let mut entities: Vec<Entity> = found
        .iter()
        .filter(|f| f.chunk == chunk)
        .map(|f| f.entity)
        .collect();
      entities.sort();
      entities
    };
    let before = occluders_of(&mut maps.world, layer);
    assert_placed(&before);
    let mut chunks: Vec<IVec2> = before.iter().map(|f| f.chunk).collect();
    chunks.sort_by_key(|c| (c.y, c.x));
    assert_eq!(
      chunks,
      [IVec2::new(-1, -1), IVec2::ZERO, IVec2::new(1, 0)],
      "a chain a clump"
    );
    assert_same_edges(
      &lit_edges(&mut maps.world, layer),
      &part_edges(&mut maps.world, map),
      "before",
    );
    // A lone hexagon in chunk (0, 0): only that chunk is built again, and
    // in it only the hexagon's chain is new
    assert!(MapsView::new(&mut maps.world).set_tile(
      map,
      "ground",
      IVec2::new(10, 10),
      Some(Tile::new(0, 0))
    ));
    maps.frame();
    let after = occluders_of(&mut maps.world, layer);
    assert_placed(&after);
    for far in [(-1, -1), (1, 0)] {
      assert_eq!(
        in_chunk(&after, far),
        in_chunk(&before, far),
        "chunk {far:?} kept its occluders"
      );
    }
    let near = in_chunk(&after, (0, 0));
    assert_eq!(near.len(), 2, "the clump's and the hexagon's: {after:#?}");
    let was = in_chunk(&before, (0, 0));
    assert_eq!(
      near.iter().filter(|e| was.contains(e)).count(),
      1,
      "the clump's chain kept its entity, the hexagon's is new"
    );
    assert_same_edges(
      &lit_edges(&mut maps.world, layer),
      &part_edges(&mut maps.world, map),
      "after an edit",
    );
    // A hexagon in chunk (1, 0)'s first column: its side on the left, and
    // the one on the upper left, belong to cells of chunk (0, 0), which is
    // built again with it: each chunk gains the hexagon's chain of the
    // sides it owns and keeps the chains it had
    let edge_cell = IVec2::new(32, 10);
    assert!(MapsView::new(&mut maps.world).set_tile(
      map,
      "ground",
      edge_cell,
      Some(Tile::new(0, 0))
    ));
    maps.frame();
    let edged = occluders_of(&mut maps.world, layer);
    assert_placed(&edged);
    assert_eq!(
      in_chunk(&edged, (-1, -1)),
      in_chunk(&before, (-1, -1)),
      "chunk (-1, -1) kept its occluders"
    );
    for built in [(0, 0), (1, 0)] {
      let (now, was) = (in_chunk(&edged, built), in_chunk(&after, built));
      assert!(
        was.iter().all(|e| now.contains(e)),
        "chunk {built:?} kept its chains"
      );
      assert_eq!(
        now.len(),
        was.len() + 1,
        "chunk {built:?} gained the hexagon's"
      );
    }
    let left = grid
      .cell_outline(edge_cell)
      .points()
      .iter()
      .map(|p| p.x)
      .fold(f32::MAX, f32::min);
    let base = world_offset(&maps.world, layer);
    let owned = edged.iter().filter(|f| f.chunk == IVec2::ZERO).any(|f| {
      f.occluder.edges().any(|(a, b)| {
        let (a, b) = (base + f.at + a, base + f.at + b);
        (a.x - left).abs() < 1e-3 && (b.x - left).abs() < 1e-3
      })
    });
    assert!(owned, "chunk (0, 0) casts the hexagon's left side");
    assert_same_edges(
      &lit_edges(&mut maps.world, layer),
      &part_edges(&mut maps.world, map),
      "after an edit on a chunk's border",
    );
  }

  #[test]
  fn hex_shadows_edited_at_random_round_chunk_corners_match_a_fresh_build() {
    use crate::core::tilemap::test_util::whole_hex_set;
    use crate::core::tilemap::{GridShape, StaggerAxis, StaggerIndex};
    let mut seed = 0x1b87_3593_u32;
    let mut next = |n: u32| {
      seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
      (seed >> 8) % n
    };
    // Cells round the corners where chunks (-1, -1), (0, -1), (-1, 0),
    // (0, 0), (1, 0) and (0, 1) meet, so edits change every rim; on grids
    // of either axis and index, and of sides of 0 (diamonds, where four
    // cells meet at a point) and longer than the cell (whose sides run on
    // straight)
    let near = [-2, -1, 0, 1, 30, 31, 32, 33];
    let grids = [StaggerAxis::Y, StaggerAxis::X]
      .into_iter()
      .flat_map(|axis| [StaggerIndex::Odd, StaggerIndex::Even].map(|index| (axis, index, 8)))
      .chain([
        (StaggerAxis::Y, StaggerIndex::Odd, 0),
        (StaggerAxis::X, StaggerIndex::Even, 30),
      ]);
    for (axis, index, side) in grids {
      let shape = GridShape::Hexagonal { axis, index, side };
      let cell = UVec2::new(16, 18);
      let label = format!("{axis:?} {index:?} side {side}");
      let map_of = |cells: &[IVec2]| {
        let mut map = TileMap::infinite(cell).with_shape(shape);
        let set = whole_hex_set(&map.grid());
        let set = map.add_tileset(TilesetSource::Embedded(Arc::new(set)));
        let layer = map.add_tile_layer("ground");
        if let Some(grid) = map.tiles_mut(layer) {
          for &at in cells {
            grid.set(at, Some(Tile::new(set, 0)));
          }
        }
        map
      };
      let pick = |next: &mut dyn FnMut(u32) -> u32| {
        let n = near.len() as u32;
        IVec2::new(near[next(n) as usize], near[next(n) as usize])
      };
      let mut solid: Vec<IVec2> = (0..12).map(|_| pick(&mut next)).collect();
      solid.sort_by_key(|c| (c.y, c.x));
      solid.dedup();
      let mut maps = MapWorld::new();
      let handle = maps
        .world
        .resource_mut::<Assets<TileMap>>()
        .add(map_of(&solid));
      let map = maps.spawn(MapRoot::new(handle));
      let layer = layers_of(&mut maps.world, map)[0];
      for edit in 0..40 {
        let at = pick(&mut next);
        let tile = if solid.contains(&at) {
          solid.retain(|&c| c != at);
          None
        } else {
          solid.push(at);
          Some(Tile::new(0, 0))
        };
        assert!(MapsView::new(&mut maps.world).set_tile(map, "ground", at, tile));
        maps.frame();
        let found = occluders_of(&mut maps.world, layer);
        assert_placed(&found);
        assert_eq!(
          steps(&found),
          collision_steps(&mut maps.world, map),
          "{label}: edit {edit} at {at}"
        );
      }
      // As a map built afresh from the same cells casts them, chunk for
      // chunk
      let handle = maps
        .world
        .resource_mut::<Assets<TileMap>>()
        .add(map_of(&solid));
      let fresh = maps.spawn(MapRoot::new(handle));
      let fresh_layer = layers_of(&mut maps.world, fresh)[0];
      let by_chunk = |found: &[Found]| -> BTreeMap<(i32, i32), BTreeMap<Edge, usize>> {
        let mut chunks: BTreeMap<(i32, i32), Vec<Found>> = BTreeMap::new();
        for f in found {
          chunks
            .entry((f.chunk.y, f.chunk.x))
            .or_default()
            .push(f.clone());
        }
        chunks
          .into_iter()
          .map(|(chunk, found)| (chunk, steps(&found)))
          .collect()
      };
      let edited = occluders_of(&mut maps.world, layer);
      let built = occluders_of(&mut maps.world, fresh_layer);
      assert!(!built.is_empty(), "{label}");
      assert_eq!(by_chunk(&edited), by_chunk(&built), "{label}");
    }
  }

  #[test]
  fn occluder_marked_shapes_cast_the_shadow_on_hex() {
    let cell = UVec2::new(120, 140);
    let mut map = TileMap::new(UVec2::new(4, 4), cell).with_shape(hex_shape());
    let grid = map.grid();
    // A tile its cell's size that collides with a triangle and casts its
    // shadow with a square, both clockwise on screen in its pixels
    let triangle = [
      Vec2::new(30.0, 40.0),
      Vec2::new(90.0, 60.0),
      Vec2::new(50.0, 100.0),
    ];
    let square = [
      Vec2::new(40.0, 50.0),
      Vec2::new(80.0, 50.0),
      Vec2::new(80.0, 90.0),
      Vec2::new(40.0, 90.0),
    ];
    let mut set = Tileset::new("rocks", cell, TilesetImage::Collection);
    set.insert_tile(
      0,
      TileData {
        shapes: vec![
          TileShape {
            points: triangle.to_vec(),
            ..TileShape::default()
          },
          TileShape {
            points: square.to_vec(),
            occluder: true,
            ..TileShape::default()
          },
        ],
        ..TileData::default()
      },
    );
    let set = map.add_tileset(TilesetSource::Embedded(Arc::new(set)));
    let layer = map.add_tile_layer("rocks");
    let at = IVec2::new(1, 2);
    if let Some(grid) = map.tiles_mut(layer) {
      grid.set(at, Some(Tile::new(set, 0)));
    }
    let mut maps = MapWorld::new();
    let handle = maps.world.resource_mut::<Assets<TileMap>>().add(map);
    let map = maps.spawn(MapRoot::new(handle));
    let layer = layers_of(&mut maps.world, map)[0];
    // Each point where the tile draws it: from its image's top-left
    let top_left = grid.tile_anchor(at) - Vec2::new(0.0, cell.y as f32);
    let ring = |points: &[Vec2]| -> Vec<(Vec2, Vec2)> {
      (0..points.len())
        .map(|i| {
          (
            top_left + points[i],
            top_left + points[(i + 1) % points.len()],
          )
        })
        .collect()
    };
    let found = occluders_of(&mut maps.world, layer);
    assert_placed(&found);
    assert_eq!(closed_chains(&found), [4], "{found:?}");
    assert_same_edges(&lit_edges(&mut maps.world, layer), &ring(&square), "light");
    // while it collides with the triangle alone
    assert_same_edges(
      &part_edges(&mut maps.world, map),
      &ring(&triangle),
      "collision",
    );
  }

  // ---- hexagonal edits that keep the chains they don't reach --------------

  /// What a test paints a hexagonal cell with: nothing, its whole hexagon
  /// (a solid cell), or one of two shapes of its own (tiles 1 and 2 of
  /// `shaped_hex_set`)
  #[derive(Clone, Copy, Debug, PartialEq, Eq)]
  enum Paint {
    Empty,
    Solid,
    Inset,
    Wedge,
  }

  impl Paint {
    const ALL: [Paint; 4] = [Paint::Empty, Paint::Solid, Paint::Inset, Paint::Wedge];

    fn tile(self) -> Option<Tile> {
      match self {
        Paint::Empty => None,
        Paint::Solid => Some(Tile::new(0, 0)),
        Paint::Inset => Some(Tile::new(0, 1)),
        Paint::Wedge => Some(Tile::new(0, 2)),
      }
    }
  }

  /// `whole_hex_set` for `grid` with tile 1 an inset hexagon and tile 2 a
  /// wedge, each a shape of its own, in fractions of the tile
  fn shaped_hex_set(grid: &crate::core::tilemap::MapGrid) -> Tileset {
    use crate::core::tilemap::test_util::whole_hex_set;
    const INSET: [(f32, f32); 6] = [
      (0.5, 0.125),
      (0.875, 0.3125),
      (0.875, 0.6875),
      (0.5, 0.875),
      (0.125, 0.6875),
      (0.125, 0.3125),
    ];
    const WEDGE: [(f32, f32); 3] = [(0.25, 0.25), (0.75, 0.5), (0.25, 0.75)];
    let mut set = whole_hex_set(grid);
    let size = grid.cell.as_vec2();
    for (id, points) in [(1, &INSET[..]), (2, &WEDGE[..])] {
      set.insert_tile(
        id,
        TileData {
          shapes: vec![TileShape {
            points: points
              .iter()
              .map(|&(x, y)| Vec2::new(x, y) * size)
              .collect(),
            ..TileShape::default()
          }],
          ..TileData::default()
        },
      );
    }
    set.tile_count = 3;
    set
  }

  /// A map of `shape` hexagons of `cell` px, bounded to `size` cells or
  /// infinite, its layer "ground" painted at `cells`
  fn painted_map(
    shape: crate::core::tilemap::GridShape,
    cell: UVec2,
    size: Option<UVec2>,
    cells: &BTreeMap<(i32, i32), Paint>,
  ) -> TileMap {
    let map = match size {
      Some(size) => TileMap::new(size, cell),
      None => TileMap::infinite(cell),
    };
    let mut map = map.with_shape(shape);
    let set = shaped_hex_set(&map.grid());
    map.add_tileset(TilesetSource::Embedded(Arc::new(set)));
    let layer = map.add_tile_layer("ground");
    if let Some(grid) = map.tiles_mut(layer) {
      for (&(y, x), paint) in cells {
        grid.set(IVec2::new(x, y), paint.tile());
      }
    }
    map
  }

  /// Spawns `map` and returns it with its layer
  fn spawn_map(maps: &mut MapWorld, map: TileMap) -> (Entity, Entity) {
    let handle = maps.world.resource_mut::<Assets<TileMap>>().add(map);
    let map = maps.spawn(MapRoot::new(handle));
    let layer = layers_of(&mut maps.world, map)[0];
    (map, layer)
  }

  /// Paints `cell` of `map`'s "ground" with `paint`, noted in `cells`, and
  /// runs a frame
  fn paint(
    maps: &mut MapWorld,
    map: Entity,
    cells: &mut BTreeMap<(i32, i32), Paint>,
    cell: IVec2,
    paint: Paint,
  ) {
    assert!(
      MapsView::new(&mut maps.world).set_tile(map, "ground", cell, paint.tile()),
      "{cell} becomes {paint:?}"
    );
    if paint == Paint::Empty {
      cells.remove(&(cell.y, cell.x));
    } else {
      cells.insert((cell.y, cell.x), paint);
    }
    maps.frame();
  }

  /// That the shadows of `layer` are what a map built afresh as `fresh`
  /// casts, edge for edge, and each chunk holds at most twice the chains
  /// its fresh build does (spawned, compared and despawned)
  fn assert_as_fresh(maps: &mut MapWorld, layer: Entity, fresh: TileMap, label: &str) {
    let (built, built_layer) = spawn_map(maps, fresh);
    assert_same_edges(
      &lit_edges(&mut maps.world, layer),
      &lit_edges(&mut maps.world, built_layer),
      label,
    );
    let per_chunk = |found: &[Found]| -> BTreeMap<(i32, i32), usize> {
      let mut counts = BTreeMap::new();
      for f in found {
        *counts.entry((f.chunk.y, f.chunk.x)).or_insert(0) += 1;
      }
      counts
    };
    let edited = per_chunk(&occluders_of(&mut maps.world, layer));
    let fresh = per_chunk(&occluders_of(&mut maps.world, built_layer));
    for (chunk, &n) in &edited {
      let most = 2 * fresh.get(chunk).copied().unwrap_or(0);
      assert!(
        n <= most,
        "{label}: chunk {chunk:?} holds {n} chains, a fresh build {most} / 2"
      );
    }
    maps.world.despawn(built);
    maps.frame();
  }

  /// Whether a chain of `found` (its parent at `base` in the world) passes
  /// through `point` (the world), at a vertex or along a segment
  fn touches(found: &Found, base: Vec2, point: Vec2) -> bool {
    found.occluder.edges().any(|(a, b)| {
      let (a, b) = (base + found.at + a, base + found.at + b);
      let (ab, ap) = (b - a, point - a);
      let t = ap.dot(ab) / ab.length_squared().max(1e-6);
      (-1e-4..=1.0 + 1e-4).contains(&t) && (a + ab * t - point).length() < 1e-3
    })
  }

  /// The occluders under `layer` by entity
  fn by_entity(world: &mut World, layer: Entity) -> BTreeMap<Entity, Found> {
    occluders_of(world, layer)
      .into_iter()
      .map(|f| (f.entity, f))
      .collect()
  }

  /// That every chain of `before` that passes through none of `corners`
  /// (the world) is in `after` as it was: the same entity, place and
  /// vertices
  fn assert_kept_away_from(
    before: &BTreeMap<Entity, Found>,
    after: &BTreeMap<Entity, Found>,
    base: Vec2,
    corners: &[Vec2],
    label: &str,
  ) {
    for (entity, was) in before {
      if corners.iter().any(|&c| touches(was, base, c)) {
        continue;
      }
      let Some(now) = after.get(entity) else {
        panic!(
          "{label}: chain {entity} (chunk {}) passes through no corner of the edit, yet went",
          was.chunk
        );
      };
      assert_eq!(now.at, was.at, "{label}: {entity}");
      assert_eq!(
        now.occluder.vertices(),
        was.occluder.vertices(),
        "{label}: {entity}"
      );
    }
  }

  /// Every map occluder carries `ParentPlaced`: its map's walk places it
  fn assert_parent_placed(world: &mut World) {
    let unplaced = world
      .query_filtered::<Entity, (With<MapOccluder>, Without<ParentPlaced>)>()
      .iter(world)
      .count();
    assert_eq!(unplaced, 0, "chains without ParentPlaced");
  }

  /// The hexagonal grids of 16 x 18 px cells and sides of 8 the edits run
  /// on: either axis, either index
  fn hex_grids() -> Vec<crate::core::tilemap::GridShape> {
    use crate::core::tilemap::{GridShape, StaggerAxis, StaggerIndex};
    [StaggerAxis::Y, StaggerAxis::X]
      .into_iter()
      .flat_map(|axis| {
        [StaggerIndex::Odd, StaggerIndex::Even].map(|index| GridShape::Hexagonal {
          axis,
          index,
          side: 8,
        })
      })
      .collect()
  }

  /// The guards' grid: 16 px hexagons of side 8 in rows, the odd ones
  /// shifted
  fn guard_hex() -> crate::core::tilemap::GridShape {
    use crate::core::tilemap::{GridShape, StaggerAxis, StaggerIndex};
    GridShape::Hexagonal {
      axis: StaggerAxis::Y,
      index: StaggerIndex::Odd,
      side: 8,
    }
  }

  /// The serpentine's rows: every odd row of chunk (0, 0) full, from
  /// column `from` to column 31
  fn full_rows(from: i32) -> BTreeMap<(i32, i32), Paint> {
    (1..32)
      .step_by(2)
      .flat_map(|y| (from..32).map(move |x| ((y, x), Paint::Solid)))
      .collect()
  }

  #[test]
  fn a_hex_edit_respawns_only_the_chains_through_its_cell() {
    use crate::core::tilemap::MapGrid;
    let (shape, cell) = (guard_hex(), UVec2::splat(16));
    let mut cells = full_rows(0);
    let mut maps = MapWorld::new();
    let (map, layer) = spawn_map(&mut maps, painted_map(shape, cell, None, &cells));
    let before = by_entity(&mut maps.world, layer);
    assert!(before.len() >= 16, "a chain a row: {}", before.len());
    // A cell in the middle of a row goes: only that row's chain passes
    // through its corners
    let edited = IVec2::new(16, 15);
    paint(&mut maps, map, &mut cells, edited, Paint::Empty);
    let after = by_entity(&mut maps.world, layer);
    let corners = MapGrid::new(shape, cell)
      .cell_outline(edited)
      .points()
      .to_vec();
    let base = world_offset(&maps.world, layer);
    assert_kept_away_from(&before, &after, base, &corners, "a row cut");
    let kept = after.keys().filter(|e| before.contains_key(e)).count();
    assert!(kept >= before.len() - 2, "kept {kept} of {}", before.len());
    assert_parent_placed(&mut maps.world);
    assert_as_fresh(
      &mut maps,
      layer,
      painted_map(shape, cell, None, &cells),
      "a row cut",
    );
  }

  #[test]
  fn a_hex_rim_edit_rebuilds_only_the_neighbour_chains_it_touches() {
    use crate::core::tilemap::MapGrid;
    let (shape, cell) = (guard_hex(), UVec2::splat(16));
    // The rows run on through the chunk left of chunk (0, 0)
    let mut cells = full_rows(-32);
    let mut maps = MapWorld::new();
    let (map, layer) = spawn_map(&mut maps, painted_map(shape, cell, None, &cells));
    let before = by_entity(&mut maps.world, layer);
    let left =
      |found: &BTreeMap<Entity, Found>| found.values().filter(|f| f.chunk == IVec2::NEG_X).count();
    assert!(left(&before) >= 16, "the left chunk's rows");
    // Cell (0, 15)'s side on the left belongs to (-1, 15), in the chunk
    // left of it: that chunk is built again, but only its row 15's chain
    // passes through the cell's corners
    let edited = IVec2::new(0, 15);
    paint(&mut maps, map, &mut cells, edited, Paint::Empty);
    let after = by_entity(&mut maps.world, layer);
    let corners = MapGrid::new(shape, cell)
      .cell_outline(edited)
      .points()
      .to_vec();
    let base = world_offset(&maps.world, layer);
    assert_kept_away_from(&before, &after, base, &corners, "a rim cell");
    let kept_left = after
      .iter()
      .filter(|(e, f)| f.chunk == IVec2::NEG_X && before.contains_key(e))
      .count();
    assert!(
      kept_left + 2 >= left(&before),
      "the left chunk kept {kept_left} of {}",
      left(&before)
    );
    assert_parent_placed(&mut maps.world);
    assert_as_fresh(
      &mut maps,
      layer,
      painted_map(shape, cell, None, &cells),
      "a rim cell",
    );
  }

  #[test]
  fn ring_chains_of_unchanged_cells_keep_their_entities() {
    let (shape, cell) = (guard_hex(), UVec2::splat(16));
    // Chunk (0, 0) full of tiles whose shape is an inset hexagon, each a
    // closed chain of its own
    let mut cells: BTreeMap<(i32, i32), Paint> = (0..32)
      .flat_map(|y| (0..32).map(move |x| ((y, x), Paint::Inset)))
      .collect();
    let mut maps = MapWorld::new();
    let (map, layer) = spawn_map(&mut maps, painted_map(shape, cell, None, &cells));
    let before = by_entity(&mut maps.world, layer);
    assert_eq!(before.len(), 1024, "a ring a cell");
    // A cell cleared: its ring goes, and nothing else
    paint(&mut maps, map, &mut cells, IVec2::new(10, 10), Paint::Empty);
    let cleared = by_entity(&mut maps.world, layer);
    assert_eq!(cleared.len(), 1023);
    assert!(
      cleared.keys().all(|e| before.contains_key(e)),
      "every other ring kept its entity"
    );
    // A cell given another shape: its ring alone is spawned again
    paint(&mut maps, map, &mut cells, IVec2::new(20, 20), Paint::Wedge);
    let turned = by_entity(&mut maps.world, layer);
    assert_eq!(turned.len(), 1023);
    let new: Vec<&Found> = turned
      .values()
      .filter(|f| !cleared.contains_key(&f.entity))
      .collect();
    assert_eq!(new.len(), 1, "one ring respawned");
    assert_eq!(new[0].occluder.vertices().len(), 4, "the wedge, closed");
    assert_eq!(
      cleared.keys().filter(|e| !turned.contains_key(e)).count(),
      1,
      "one ring gone"
    );
    assert_parent_placed(&mut maps.world);
    assert_as_fresh(
      &mut maps,
      layer,
      painted_map(shape, cell, None, &cells),
      "rings",
    );
  }

  #[test]
  fn hex_neighbour_marks_follow_side_ownership_on_every_variant() {
    let cell = UVec2::new(16, 18);
    // Four cells in ten solid over the chunks round chunk (0, 0), so every
    // chunk has chains along its rims
    let hash = |x: i32, y: i32| {
      let h = (x as u32).wrapping_mul(0x9E37_79B9) ^ (y as u32).wrapping_mul(0x85EB_CA6B);
      (h ^ (h >> 15)).wrapping_mul(0x2C1B_3C6D) >> 16
    };
    let start: BTreeMap<(i32, i32), Paint> = (-32..64)
      .flat_map(|y| (-32..64).map(move |x| (x, y)))
      .filter(|&(x, y)| hash(x, y) % 10 < 4)
      .map(|(x, y)| ((y, x), Paint::Solid))
      .collect();
    // Each kind of rim cell: the corners, the first and last rows' and
    // columns' middles, on lines of either parity
    let rim = [
      (0, 0),
      (31, 0),
      (0, 31),
      (31, 31),
      (15, 0),
      (16, 0),
      (15, 31),
      (16, 31),
      (0, 15),
      (0, 16),
      (31, 15),
      (31, 16),
    ];
    for shape in hex_grids() {
      let grid = crate::core::tilemap::MapGrid::new(shape, cell);
      let Some(hex) = grid.hex_cells() else {
        panic!("{shape:?} is hexagonal");
      };
      let mut cells = start.clone();
      let mut maps = MapWorld::new();
      let (map, layer) = spawn_map(&mut maps, painted_map(shape, cell, None, &cells));
      for (x, y) in rim {
        let at = IVec2::new(x, y);
        let label = format!("{shape:?}, cell {at}");
        // The chunks owning one of its sides: each side belongs to the
        // earlier of its cells in row order
        let owners: Vec<IVec2> = (0..6)
          .map(|side| {
            let across = hex.across(at, side);
            let owner = if (across.y, across.x) < (at.y, at.x) {
              across
            } else {
              at
            };
            TileGrid::chunk_of(owner)
          })
          .collect();
        let was = cells.get(&(y, x)).copied().unwrap_or(Paint::Empty);
        let flipped = if was == Paint::Solid {
          Paint::Empty
        } else {
          Paint::Solid
        };
        for paint_now in [flipped, was] {
          let before = by_entity(&mut maps.world, layer);
          paint(&mut maps, map, &mut cells, at, paint_now);
          let after = by_entity(&mut maps.world, layer);
          for dy in -1..=1 {
            for dx in -1..=1 {
              let chunk = IVec2::new(dx, dy);
              if chunk == IVec2::ZERO || owners.contains(&chunk) {
                continue;
              }
              let of = |found: &BTreeMap<Entity, Found>| -> Vec<Entity> {
                found
                  .values()
                  .filter(|f| f.chunk == chunk)
                  .map(|f| f.entity)
                  .collect()
              };
              assert!(!of(&before).is_empty(), "{label}: chunk {chunk} has chains");
              assert_eq!(
                of(&after),
                of(&before),
                "{label} made {paint_now:?}: chunk {chunk} owns none of its sides"
              );
            }
          }
          // and every chunk that did own one built what a fresh build casts
          assert_as_fresh(
            &mut maps,
            layer,
            painted_map(shape, cell, None, &cells),
            &format!("{label} made {paint_now:?}"),
          );
        }
      }
    }
  }

  #[test]
  fn a_shape_cell_painted_on_a_chunk_edge_rebuilds_no_neighbour() {
    let (shape, cell) = (guard_hex(), UVec2::splat(16));
    // The chunk left of chunk (0, 0) solid along its last columns, and a
    // clump in chunk (0, 0)
    let mut cells: BTreeMap<(i32, i32), Paint> = (4..16)
      .flat_map(|y| (-4..0).map(move |x| ((y, x), Paint::Solid)))
      .chain([((20, 5), Paint::Solid), ((20, 6), Paint::Solid)])
      .collect();
    let mut maps = MapWorld::new();
    let (map, layer) = spawn_map(&mut maps, painted_map(shape, cell, None, &cells));
    let left = |found: &BTreeMap<Entity, Found>| -> Vec<Entity> {
      found
        .values()
        .filter(|f| f.chunk == IVec2::NEG_X)
        .map(|f| f.entity)
        .collect()
    };
    let start = by_entity(&mut maps.world, layer);
    assert!(!left(&start).is_empty());
    // A first-column cell beside the solid ones becomes a shape, then
    // another: no cell's solidity changes, so only its ring is spawned
    let edited = IVec2::new(0, 10);
    let mut before = start.clone();
    for shaped in [Paint::Inset, Paint::Wedge] {
      paint(&mut maps, map, &mut cells, edited, shaped);
      let after = by_entity(&mut maps.world, layer);
      assert_eq!(
        left(&after),
        left(&start),
        "{shaped:?}: the left chunk kept its chains"
      );
      let gone = before.keys().filter(|e| !after.contains_key(e)).count();
      let new = after.keys().filter(|e| !before.contains_key(e)).count();
      assert_eq!(new, 1, "{shaped:?}: its ring");
      assert_eq!(
        gone,
        usize::from(shaped == Paint::Wedge),
        "{shaped:?}: the ring before"
      );
      before = after;
    }
    assert_as_fresh(
      &mut maps,
      layer,
      painted_map(shape, cell, None, &cells),
      "a shape on a chunk's edge",
    );
  }

  /// Random edits at `near` (cells' columns and rows) of a map of `shape`
  /// (bounded to `size`, or infinite): each cell becomes empty, solid or
  /// one of two shapes, and after each edit the shadows are a fresh
  /// build's
  fn edit_at_random(
    shape: crate::core::tilemap::GridShape,
    size: Option<UVec2>,
    near: &[i32],
    seed: u32,
    edits: usize,
  ) {
    let cell = UVec2::new(16, 18);
    let mut seed = seed;
    let mut next = |n: u32| {
      seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
      (seed >> 8) % n
    };
    let pick = |next: &mut dyn FnMut(u32) -> u32| {
      let n = near.len() as u32;
      IVec2::new(near[next(n) as usize], near[next(n) as usize])
    };
    let mut cells = BTreeMap::new();
    for _ in 0..16 {
      let at = pick(&mut next);
      cells.insert((at.y, at.x), Paint::ALL[1 + next(3) as usize]);
    }
    let mut maps = MapWorld::new();
    let (map, layer) = spawn_map(&mut maps, painted_map(shape, cell, size, &cells));
    for edit in 0..edits {
      let at = pick(&mut next);
      let was = cells.get(&(at.y, at.x)).copied().unwrap_or(Paint::Empty);
      let now = loop {
        let now = Paint::ALL[next(4) as usize];
        if now != was {
          break now;
        }
      };
      paint(&mut maps, map, &mut cells, at, now);
      assert_kept_as_spawned(&mut maps.world, map);
      let label = format!("{shape:?} {size:?}: edit {edit}, {at} {was:?} to {now:?}");
      assert_as_fresh(
        &mut maps,
        layer,
        painted_map(shape, cell, size, &cells),
        &label,
      );
    }
    assert_parent_placed(&mut maps.world);
  }

  #[test]
  fn hex_shadows_edited_at_random_match_a_fresh_build_on_bounded_maps_and_every_variant() {
    for (i, shape) in hex_grids().into_iter().enumerate() {
      // Round a bounded map's first cells, the corner of its first four
      // chunks and its far border
      let seed = 0x2545_f491 + i as u32;
      edit_at_random(
        shape,
        Some(UVec2::splat(40)),
        &[0, 1, 31, 32, 33, 38, 39],
        seed,
        24,
      );
      // and round the chunks that meet at (0, 0) and (32, 32) of an
      // infinite one
      edit_at_random(shape, None, &[-2, -1, 0, 1, 31, 32, 33], seed, 24);
    }
  }

  #[test]
  fn a_side_zero_hex_map_edited_at_random_matches_a_fresh_build() {
    use crate::core::tilemap::{GridShape, StaggerAxis, StaggerIndex};
    // Diamonds: four cells meet at a point, and the chains go by points
    for (axis, index) in [
      (StaggerAxis::Y, StaggerIndex::Odd),
      (StaggerAxis::X, StaggerIndex::Even),
    ] {
      let shape = GridShape::Hexagonal {
        axis,
        index,
        side: 0,
      };
      edit_at_random(shape, None, &[-2, -1, 0, 1, 31, 32, 33], 0x0bad_5eed, 24);
      edit_at_random(
        shape,
        Some(UVec2::splat(40)),
        &[0, 1, 31, 32, 33, 38, 39],
        0x0bad_5eed,
        12,
      );
    }
  }

  /// Paints each of `edits` on `map`'s "ground" (a cell outside a map
  /// bounded to `size` left out), noted in `cells`, and runs one frame for
  /// them all
  fn paint_many(
    maps: &mut MapWorld,
    map: Entity,
    cells: &mut BTreeMap<(i32, i32), Paint>,
    size: Option<UVec2>,
    edits: &[(IVec2, Paint)],
  ) {
    for &(cell, paint) in edits {
      let outside =
        size.is_some_and(|size| cell.cmplt(IVec2::ZERO).any() || cell.cmpge(size.as_ivec2()).any());
      if outside {
        continue;
      }
      MapsView::new(&mut maps.world).set_tile(map, "ground", cell, paint.tile());
      if paint == Paint::Empty {
        cells.remove(&(cell.y, cell.x));
      } else {
        cells.insert((cell.y, cell.x), paint);
      }
    }
    maps.frame();
  }

  /// Seeded frames each painting 2 to 8 cells at once round the chunk
  /// corners and rims at `near` (cells' columns and rows; `rims` those on a
  /// chunk's edge) of a map of `shape`, bounded to `size` or infinite:
  /// clusters of cells beside each other, and every other frame a rim cell
  /// with its neighbour across the chunk's edge, each made empty, solid or
  /// one of two shapes. After each frame the shadows are a fresh build's
  fn paint_many_at_random(
    shape: crate::core::tilemap::GridShape,
    size: Option<UVec2>,
    (near, rims): (&[i32], &[i32]),
    seed: u32,
    frames: usize,
  ) {
    let cell = UVec2::new(16, 18);
    let Some(hex) = crate::core::tilemap::MapGrid::new(shape, cell).hex_cells() else {
      panic!("{shape:?} is hexagonal");
    };
    let mut seed = seed;
    let mut next = |n: u32| {
      seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
      (seed >> 8) % n
    };
    let pick =
      |from: &[i32], next: &mut dyn FnMut(u32) -> u32| from[next(from.len() as u32) as usize];
    let mut cells = BTreeMap::new();
    for _ in 0..24 {
      let at = IVec2::new(pick(near, &mut next), pick(near, &mut next));
      cells.insert((at.y, at.x), Paint::ALL[1 + next(3) as usize]);
    }
    let mut maps = MapWorld::new();
    let (map, layer) = spawn_map(&mut maps, painted_map(shape, cell, size, &cells));
    for frame in 0..frames {
      let count = 2 + next(7) as usize;
      let across_rim = frame % 2 == 0;
      let anchor = match (across_rim, next(2)) {
        (true, 0) => IVec2::new(pick(rims, &mut next), pick(near, &mut next)),
        (true, _) => IVec2::new(pick(near, &mut next), pick(rims, &mut next)),
        (false, _) => IVec2::new(pick(near, &mut next), pick(near, &mut next)),
      };
      let mut picked = vec![anchor];
      if across_rim {
        let chunk = TileGrid::chunk_of(anchor);
        let first = next(6) as usize;
        let beyond = (0..6)
          .map(|s| hex.across(anchor, (first + s) % 6))
          .find(|&c| TileGrid::chunk_of(c) != chunk);
        picked.extend(beyond);
      }
      // The rest beside cells already picked
      for _ in 0..64 {
        if picked.len() >= count {
          break;
        }
        let from = picked[next(picked.len() as u32) as usize];
        let beside = hex.across(from, next(6) as usize);
        if !picked.contains(&beside) {
          picked.push(beside);
        }
      }
      let edits: Vec<(IVec2, Paint)> = picked
        .iter()
        .map(|&at| (at, Paint::ALL[next(4) as usize]))
        .collect();
      paint_many(&mut maps, map, &mut cells, size, &edits);
      assert_kept_as_spawned(&mut maps.world, map);
      let label = format!("{shape:?} {size:?}: frame {frame}, {edits:?}");
      assert_as_fresh(
        &mut maps,
        layer,
        painted_map(shape, cell, size, &cells),
        &label,
      );
    }
    assert_parent_placed(&mut maps.world);
  }

  #[test]
  fn hex_shadows_painted_several_cells_a_frame_round_chunk_rims_match_a_fresh_build() {
    for (i, shape) in hex_grids().into_iter().enumerate() {
      let seed = 0x5bd1_e995 + i as u32;
      // Round the chunks that meet at (0, 0) and (32, 32) of an infinite
      // map
      paint_many_at_random(
        shape,
        None,
        (&[-2, -1, 0, 1, 30, 31, 32, 33], &[-1, 0, 31, 32]),
        seed,
        40,
      );
      // and of a bounded one, to its far border
      paint_many_at_random(
        shape,
        Some(UVec2::splat(40)),
        (&[0, 1, 30, 31, 32, 33, 38, 39], &[0, 31, 32, 39]),
        seed,
        20,
      );
    }
  }

  #[test]
  fn a_hex_edit_of_more_than_hex_whole_cells_in_a_kept_chunk_matches_a_fresh_build() {
    let (shape, cell) = (guard_hex(), UVec2::splat(16));
    let mut cells = full_rows(0);
    let mut maps = MapWorld::new();
    let (map, layer) = spawn_map(&mut maps, painted_map(shape, cell, None, &cells));
    let kept = |world: &World| {
      world.get::<MapShadows>(map).is_some_and(|s| {
        s.groups
          .iter()
          .any(|g| g.hex_chunks.contains_key(&IVec2::ZERO))
      })
    };
    assert!(kept(&maps.world), "chunk (0, 0) keeps what it read");
    let before = by_entity(&mut maps.world, layer);
    // Five rows of sixteen cells turned over in one frame, well inside the
    // chunk: more cells than `HEX_WHOLE`
    let edits: Vec<(IVec2, Paint)> = (8..13)
      .flat_map(|y| (4..20).map(move |x| IVec2::new(x, y)))
      .map(|at| {
        let now = match cells.get(&(at.y, at.x)) {
          Some(Paint::Solid) => Paint::Empty,
          _ => Paint::Solid,
        };
        (at, now)
      })
      .collect();
    assert!(edits.len() as u32 > HEX_WHOLE);
    paint_many(&mut maps, map, &mut cells, None, &edits);
    // Chained whole: every chain of the chunk is new
    let after = by_entity(&mut maps.world, layer);
    let chunk_chains: Vec<&Found> = after.values().filter(|f| f.chunk == IVec2::ZERO).collect();
    assert!(!chunk_chains.is_empty());
    assert!(
      chunk_chains.iter().all(|f| !before.contains_key(&f.entity)),
      "the chunk was chained whole"
    );
    assert!(kept(&maps.world), "and keeps what it read again");
    assert_kept_as_spawned(&mut maps.world, map);
    assert_parent_placed(&mut maps.world);
    assert_as_fresh(
      &mut maps,
      layer,
      painted_map(shape, cell, None, &cells),
      "more than HEX_WHOLE cells at once",
    );
  }

  #[test]
  fn a_hex_chunk_edited_many_times_keeps_its_chain_count_near_a_fresh_build() {
    use crate::core::physics::Boundary;
    use std::collections::BTreeSet;
    let (shape, cell) = (guard_hex(), UVec2::splat(16));
    let Some(hex) = crate::core::tilemap::MapGrid::new(shape, cell).hex_cells() else {
      panic!("hexagonal");
    };
    let mut seed = 0x6a09_e667_u32;
    let mut next = |n: u32| {
      seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
      (seed >> 8) % n
    };
    // Chunk (0, 0) four cells in ten solid
    let mut solid: BTreeSet<(i32, i32)> = BTreeSet::new();
    for y in 0..32 {
      for x in 0..32 {
        if next(10) < 4 {
          solid.insert((y, x));
        }
      }
    }
    let mut cells: BTreeMap<(i32, i32), Paint> = solid.iter().map(|&c| (c, Paint::Solid)).collect();
    let mut maps = MapWorld::new();
    let (map, layer) = spawn_map(&mut maps, painted_map(shape, cell, None, &cells));
    // What a fresh build of chunk (0, 0)'s cells chains its sides into
    let boundary = Boundary::default().with_hex(hex);
    let fresh = |solid: &BTreeSet<(i32, i32)>| {
      let at = |x: i32, y: i32| {
        if solid.contains(&(y, x)) {
          TileCell::Solid
        } else {
          TileCell::Empty
        }
      };
      let mut placed = Vec::new();
      boundary.placed_edges(at, (0, 0), (32, 32), None, &mut placed);
      boundary
        .hex_chains(at, (0, 0), (32, 32), None, &placed)
        .map_or(0, |chains| chains.len())
    };
    for edit in 0..500 {
      let at = IVec2::new(next(32) as i32, next(32) as i32);
      let now = if solid.remove(&(at.y, at.x)) {
        Paint::Empty
      } else {
        solid.insert((at.y, at.x));
        Paint::Solid
      };
      paint(&mut maps, map, &mut cells, at, now);
      let chains = occluders_of(&mut maps.world, layer)
        .iter()
        .filter(|f| f.chunk == IVec2::ZERO)
        .count();
      let most = 2 * fresh(&solid);
      assert!(
        chains <= most,
        "edit {edit} at {at}: {chains} chains, a fresh build {}",
        most / 2
      );
      // and in fact as many: every chain stays a whole run of edges
      assert_eq!(chains, most / 2, "edit {edit} at {at}");
    }
    assert_as_fresh(
      &mut maps,
      layer,
      painted_map(shape, cell, None, &cells),
      "500 edits",
    );
  }

  #[test]
  fn a_reloaded_hex_map_after_edits_casts_what_a_fresh_build_does() {
    use crate::core::ecs::kernel::Messages;
    use crate::core::tilemap::MapEvent;
    use crate::core::tilemap::test_util::{FIXTURES, files_with};
    let files = files_with(&[]);
    let mut maps = MapWorld::over(files.clone());
    let handle = load(&mut maps.world, "tiled/maps/collide-hex-y-odd.tmj");
    let map = maps.spawn(MapRoot::new(handle.clone()));
    maps.frame();
    let layer = layers_of(&mut maps.world, map)[0];
    // Edits that build its chunk's chains again, piece by piece
    let whole = Some(Tile::new(0, 0));
    for (x, y, tile) in [(0, 0, whole), (5, 5, whole), (3, 2, None), (6, 4, whole)] {
      assert!(MapsView::new(&mut maps.world).set_tile(map, "walls", IVec2::new(x, y), tile));
      maps.frame();
    }
    let all = |world: &mut World| -> Vec<Entity> {
      let mut found: Vec<Entity> = world
        .query_filtered::<Entity, With<MapOccluder>>()
        .iter(world)
        .collect();
      found.sort();
      found
    };
    let before = all(&mut maps.world);
    assert!(!before.is_empty());
    // The tileset saved with its triangle tile's shape made the whole
    // hexagon its tile 0 has
    let path = format!("{FIXTURES}/tiled/tilesets/collide-hex-y.tsj");
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
    let mut json: serde_json::Value =
      serde_json::from_str(&text).unwrap_or_else(|e| panic!("{path}: {e}"));
    let hexagon = json["tiles"][0]["objectgroup"]["objects"][0]["polygon"].clone();
    assert!(hexagon.is_array());
    json["tiles"][1]["objectgroup"]["objects"][0]["polygon"] = hexagon;
    let mut cursor = maps
      .world
      .resource::<Messages<MapEvent>>()
      .get_cursor_current();
    files.insert("tiled/tilesets/collide-hex-y.tsj", json.to_string());
    maps.frames_until(|world| {
      let messages = world.resource::<Messages<MapEvent>>();
      cursor
        .read(messages)
        .any(|event| matches!(event, MapEvent::Reloaded { map: m } if *m == map))
    });
    for _ in 0..3 {
      maps.frame();
    }
    let after = all(&mut maps.world);
    assert!(!after.is_empty());
    assert!(
      after.iter().all(|e| !before.contains(e)),
      "no chain from before the reload survives"
    );
    // As the same files spawned afresh cast them
    let fresh = maps.spawn(MapRoot::new(handle));
    maps.frame();
    let fresh_layer = layers_of(&mut maps.world, fresh)[0];
    let cells = |world: &World, layer: Entity| {
      let Some(data) = world.get::<crate::core::tilemap::TileLayerData>(layer) else {
        panic!("{layer} is a tile layer");
      };
      let extent = data.tiles.extent();
      (extent.min.y..extent.max.y)
        .flat_map(|y| (extent.min.x..extent.max.x).map(move |x| IVec2::new(x, y)))
        .map(|cell| data.tiles.get(cell))
        .collect::<Vec<_>>()
    };
    assert_eq!(
      cells(&maps.world, layer),
      cells(&maps.world, fresh_layer),
      "the reload took the file's cells"
    );
    assert_same_edges(
      &lit_edges(&mut maps.world, layer),
      &lit_edges(&mut maps.world, fresh_layer),
      "reloaded",
    );
    assert_parent_placed(&mut maps.world);
  }

  /// Every occluder the groups of `map` keep is in the world under its
  /// chunk, and every map occluder in the world is kept; a hexagonal
  /// chunk's kept chains and rings are among its occluders, and its edges
  /// each have their chain
  fn assert_kept_as_spawned(world: &mut World, map: Entity) {
    let Some(shadows) = world.get::<MapShadows>(map) else {
      panic!("{map} casts shadows");
    };
    for group in &shadows.groups {
      for (chunk, kept) in &group.hex_chunks {
        let listed = group.chunks.get(chunk).map_or(&[][..], Vec::as_slice);
        for occluder in kept.chains.iter().flatten().chain(kept.rings.values()) {
          assert!(
            listed.contains(occluder),
            "chunk {chunk}: {occluder} kept, not listed"
          );
        }
        // (Sides of length 0 are chained by their points, whole each time)
        let edges = group.edges.get(chunk).map_or(0, Vec::len);
        if group.hex.is_some_and(|hex| !hex.point_sides()) {
          assert_eq!(kept.chain_of.len(), edges, "chunk {chunk}: a chain an edge");
        }
      }
    }
    let mut kept: Vec<(IVec2, Entity)> = shadows
      .groups
      .iter()
      .flat_map(|g| {
        g.chunks
          .iter()
          .flat_map(|(chunk, entities)| entities.iter().map(|e| (*chunk, *e)))
      })
      .collect();
    kept.sort_by_key(|&(c, e)| (c.y, c.x, e));
    let mut spawned: Vec<(IVec2, Entity)> = world
      .query::<(Entity, &MapOccluder)>()
      .iter(world)
      .map(|(e, o)| (o.chunk, e))
      .collect();
    spawned.sort_by_key(|&(c, e)| (c.y, c.x, e));
    assert_eq!(kept, spawned);
  }

  #[test]
  fn a_streamed_hex_chunk_landed_again_keeps_no_stale_chains() {
    use crate::core::tilemap::TileLayerData;
    // A wall across the border of chunks (0, 0) and (1, 0), hexagons in
    // each, and a patch in chunk (1, 0)
    let mut tiles: Vec<IVec2> = (26..38).map(|x| IVec2::new(x, 4)).collect();
    tiles.extend([(3, 3), (40, 10), (45, 30), (33, 0)].map(|(x, y)| IVec2::new(x, y)));
    tiles.extend((14..20).flat_map(|y| (40..48).map(move |x| IVec2::new(x, y))));
    let mut maps = MapWorld::new();
    let handle = maps
      .world
      .resource_mut::<Assets<TileMap>>()
      .add(hex_map(&tiles));
    let map = maps.spawn(MapRoot::new(handle));
    maps.frame();
    let layer = layers_of(&mut maps.world, map)[0];
    let by_chunk = |found: &[Found]| -> BTreeMap<(i32, i32), BTreeMap<Edge, usize>> {
      let mut chunks: BTreeMap<(i32, i32), Vec<Found>> = BTreeMap::new();
      for f in found {
        chunks
          .entry((f.chunk.y, f.chunk.x))
          .or_default()
          .push(f.clone());
      }
      chunks
        .into_iter()
        .map(|(chunk, found)| (chunk, steps(&found)))
        .collect()
    };
    let landed = occluders_of(&mut maps.world, layer);
    let first_life: Vec<Entity> = landed
      .iter()
      .filter(|f| f.chunk == IVec2::X)
      .map(|f| f.entity)
      .collect();
    assert!(!first_life.is_empty());
    assert_kept_as_spawned(&mut maps.world, map);

    // The stream unloads chunk (1, 0): none of its chains outlives it
    let cells = maps
      .world
      .get_mut::<TileLayerData>(layer)
      .and_then(|mut data| data.tiles.unload_chunk(IVec2::X))
      .expect("chunk (1, 0) was there");
    maps.frame();
    maps.frame();
    let away = occluders_of(&mut maps.world, layer);
    assert!(away.iter().all(|f| !first_life.contains(&f.entity)));
    assert_kept_as_spawned(&mut maps.world, map);
    let Some(shadows) = maps.world.get::<MapShadows>(map) else {
      panic!("{map} casts shadows");
    };
    assert!(
      shadows
        .groups
        .iter()
        .all(|g| !g.hex_chunks.contains_key(&IVec2::X)),
      "nothing kept of its cells while it is away"
    );

    // and lands it again: its chains are a fresh build's, none of them
    // from its first life
    if let Some(mut data) = maps.world.get_mut::<TileLayerData>(layer) {
      assert!(data.tiles.load_chunk(IVec2::X, cells));
    }
    maps.frame();
    maps.frame();
    let back = occluders_of(&mut maps.world, layer);
    assert!(back.iter().all(|f| !first_life.contains(&f.entity)));
    assert_eq!(by_chunk(&back), by_chunk(&landed), "as it first landed");
    assert_kept_as_spawned(&mut maps.world, map);
    assert_parent_placed(&mut maps.world);
  }

  /// One-tile edits in a chunk of a hexagonal map full of tiles whose shape
  /// is not their cell's hexagon (an inset hexagon, a ring of six edges a
  /// cell, all counted toward a part's budget of 128 edges, so each 8 x 8
  /// block holds 384 and none splits further), timed as the speed guards'
  /// edit guard times its parts (`benches/tilemap.rs`, `edit_schedules`): a
  /// frame before each edit, then the loop rebuild (the collision sync, the
  /// tile collider rebuild and the collider sync) and the occluder rebuild
  /// (the occluder sync), each on its own. The cases: a ring cell cleared
  /// and set back in the middle of a block (its block's part rebuilt), and
  /// one turned whole and back where four blocks meet (its sides' owners'
  /// four blocks rebuilt). Prints each part's median, 99th percentile and
  /// longest, the edges of the parts each edit rebuilt and the most a part
  /// of the chunk holds. 16 px hexagons of side 8 in odd rows, as the
  /// guards'. Run in release:
  /// `cargo test --release -p ryo2d --lib hex_ring_heavy_edit_timing -- --ignored --nocapture`
  #[test]
  #[ignore = "a timing: run in release with --ignored --nocapture"]
  fn hex_ring_heavy_edit_timing() {
    use crate::core::ecs::{Schedule, With};
    use crate::core::physics::ColliderShape;
    use crate::core::tilemap::test_util::whole_hex_set;
    use crate::core::tilemap::{GridShape, StaggerAxis, StaggerIndex};
    use std::collections::HashMap;
    use std::time::{Duration, Instant};
    let cell = UVec2::splat(16);
    let mut map = TileMap::new(UVec2::splat(128), cell).with_shape(GridShape::Hexagonal {
      axis: StaggerAxis::Y,
      index: StaggerIndex::Odd,
      side: 8,
    });
    // Tile 0 is its whole hexagon (a solid cell); tile 1 an inset hexagon,
    // clockwise on screen in its pixels, a shape of its own
    let mut set = whole_hex_set(&map.grid());
    let inset = [
      (8.0, 2.0),
      (14.0, 5.0),
      (14.0, 11.0),
      (8.0, 14.0),
      (2.0, 11.0),
      (2.0, 5.0),
    ];
    set.insert_tile(
      1,
      TileData {
        shapes: vec![TileShape {
          points: inset.iter().map(|&(x, y)| Vec2::new(x, y)).collect(),
          ..TileShape::default()
        }],
        ..TileData::default()
      },
    );
    set.tile_count = 2;
    let set = map.add_tileset(TilesetSource::Embedded(Arc::new(set)));
    let layer = map.add_tile_layer("ground");
    // Chunk (1, 1) full of the inset tile
    if let Some(grid) = map.tiles_mut(layer) {
      grid.fill(IVec2::splat(32), IVec2::splat(63), Some(Tile::new(set, 1)));
    }
    let mut maps = MapWorld::new();
    let handle = maps.world.resource_mut::<Assets<TileMap>>().add(map);
    let map = maps.spawn(MapRoot::new(handle));
    maps.frame();
    // Each part's segments, by entity
    let parts = |world: &mut World| -> HashMap<Entity, (usize, i64)> {
      world
        .query_filtered::<(Entity, &Collider), With<TilePart>>()
        .iter(world)
        .filter_map(|(entity, collider)| match &collider.shape {
          ColliderShape::Segments { points, segments } => {
            let sum: i64 = points.iter().map(|p| (p.x * 7.0 + p.y * 13.0) as i64).sum();
            Some((entity, (segments.len(), sum)))
          }
          _ => None,
        })
        .collect()
    };
    let most = parts(&mut maps.world)
      .values()
      .map(|&(n, _)| n)
      .max()
      .unwrap_or(0);
    println!(
      "a chunk of 1,024 inset hexagons: {} parts, the most edges in one {most} (the budget 128)",
      parts(&mut maps.world).len()
    );
    println!(
      "| case | loop rebuild median / p99 / max | occluder rebuild median / p99 / max | edges rebuilt median / max |"
    );
    let cases = [
      (
        "a ring cell cleared and set back in a block's middle (36, 36)",
        IVec2::new(36, 36),
        None,
      ),
      (
        "a ring cell turned whole and back where four blocks meet (40, 40)",
        IVec2::new(40, 40),
        Some(Tile::new(set, 0)),
      ),
    ];
    for (name, at, edited) in cases {
      let [mut collision, mut rebuild, mut occluders]: [Schedule; 3] =
        crate::core::tilemap::edit_schedules();
      for schedule in [&mut collision, &mut rebuild, &mut occluders] {
        schedule.run(&mut maps.world);
      }
      let (mut loops, mut lights, mut edges) = (Vec::new(), Vec::new(), Vec::new());
      for i in 0..400 {
        maps.frame();
        let before = parts(&mut maps.world);
        let tile = if i % 2 == 0 {
          edited
        } else {
          Some(Tile::new(set, 1))
        };
        assert!(
          MapsView::new(&mut maps.world).set_tile(map, "ground", at, tile),
          "{name}: the edit changes its cell"
        );
        let start = Instant::now();
        collision.run(&mut maps.world);
        rebuild.run(&mut maps.world);
        let middle = Instant::now();
        occluders.run(&mut maps.world);
        let end = Instant::now();
        loops.push(middle - start);
        lights.push(end - middle);
        let after = parts(&mut maps.world);
        edges.push(
          after
            .iter()
            .filter(|&(entity, shape)| before.get(entity) != Some(shape))
            .map(|(_, &(n, _))| n)
            .sum::<usize>(),
        );
      }
      let stats = |mut times: Vec<Duration>| {
        times.sort();
        let p99 = times[(times.len() * 99).div_ceil(100) - 1];
        (times[times.len() / 2], p99, times[times.len() - 1])
      };
      let (loop_mid, loop_99, loop_max) = stats(loops);
      let (light_mid, light_99, light_max) = stats(lights);
      edges.sort_unstable();
      println!(
        "| {name} | {loop_mid:.1?} / {loop_99:.1?} / {loop_max:.1?} | {light_mid:.1?} / {light_99:.1?} / {light_max:.1?} | {} / {} |",
        edges[edges.len() / 2],
        edges[edges.len() - 1]
      );
    }
  }
}
