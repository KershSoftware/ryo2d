//! A grid's outline loops and one-way runs packed into colliders by area,
//! not one collider each: Rapier's cost is per collider (about a
//! microsecond to register one), so a map of a thousand islands would take
//! a millisecond to land. The loops whose first corner is in a chunk
//! (`CHUNK` cells a side) are one collider, and so are the flat one-way
//! runs starting in a chunk along one row. A sloped run stays a collider of
//! its own: the one-way rule measures a collider as a whole (its highest
//! point along its direction holds a character, its lowest ends a drop), so
//! only runs on one line can share one. An edit packs again only the chunks
//! whose loops or runs it changed.
//!
//! A hexagonal grid's outlines have no straight runs to merge, so they are
//! not traced into loops: its parts hold the boundary edges along the sides
//! their cells own (`HexParts`), and, in parts of their own, the rings of
//! its shape cells, each part at most `HEX_PART_EDGES` (B, 128) edges where
//! it can, a chunk's sides and its rings each split by their own count: a
//! chunk is one part while it holds at most B, else its 16 x 16 quarters
//! are, each while it holds at most B, else a quarter's 8 x 8 blocks are,
//! whose sides never split, while a block's rings past B split into its
//! 4 x 4 sub-blocks, which never split. An edit rebuilds the parts of sides
//! owning its cell's six sides (its own among them), up to four, so about
//! 4 x B edges where its blocks keep within the budget, and the part of
//! rings holding its cell only where the cell's rings changed, and makes or
//! drops a part where a box gains its first edge or loses its last: a
//! shape costs Rapier about 80 ns an edge to build, a part about 3 ns a
//! step to keep. Of the budgets tried, 128 is the largest that kept an edit
//! at a corner of four parts each just under it within 100 µs at the 99th
//! percentile (the tile rebuild plus the fixed step after it), in five runs
//! of six, 116.1 µs in the sixth; timed again on 2026-11-06 it passed
//! 100 µs in every run, 106.5 to 134.8 µs, and main's tip read the same in
//! an A/B, so the rise came at a later commit than that choice (the physics
//! module notes). Measured in release (`docs/backlog/tilemap.md`): an idle
//! fixed step over the speed guards' cave rock, 2,109 parts, takes 6.08 to
//! 6.25 µs against 1.92 to 2.04 for its 500 square parts, and a static map
//! of it, with its shadow chains, meets the 50 µs a frame target (44.4 to
//! 47.8 µs over an empty frame); the edit guard's loop rebuild (the map's
//! collision sync, the tile rebuild and the collider sync) reads a median
//! 55.4 to 55.9 µs for a cell where four of the islands' chunks meet (four
//! quarter parts), 26.0 to 30.1 µs for part churn (four parts made, or
//! dropped) and 41.5 to 42.7 µs for an edit that takes a chunk past B (its
//! part dropped, seven made), each within its 100 µs target. A block's
//! cells own at most about 190 sides, but its shape cells' rings have no
//! bound, so a sub-block heavy with rings can hold more than B by as much
//! as its art has: a chunk of inset hexagons, six ring edges a cell, holds
//! 96 a sub-block, and its loop rebuild reads a median 11.2 to 12.9 µs for
//! a cell in a block's middle and 15.0 to 16.2 µs for one turned whole
//! where four blocks meet (in the edit guard, whose map is four layers of
//! 1024 x 1024 cells, 30.7 to 30.8 and 35.4 to 35.9 µs), where with each
//! block's rings in its part of sides it read 42.8 to 42.9 and 134.0 to
//! 135.1 µs (`hex_ring_heavy_edit_timing` in the tilemap's occluders,
//! release, three runs).
//! A bounded grid's border belongs to the cells inside it, so no part sits
//! outside the grid: a one-cell bump along a map's edge would otherwise
//! make and drop parts in the chunks outside it at every edit.

use glam::IVec2;

use super::hex::HexCells;
use super::outline::{HexSides, each_hex_owner, hex_owned_edges};
use super::{CellRect, Cells, GRID, Point, TileCell, merge_straight_runs};
use crate::core::ecs::{Entity, EntityHashMap, Vec2};
use crate::core::hash::FastMap;
use crate::core::physics::collider::Collider;

/// Cells a side of the area whose loops (or runs) one collider holds
pub(crate) const CHUNK: i32 = 32;

/// What a collider packs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) enum PackKey {
  /// The outline loops whose first corner is in a chunk
  Loops(i32, i32),
  /// The flat one-way runs starting in a chunk, along one row (an outline
  /// grid y), running one way
  Row {
    chunk: (i32, i32),
    y: i32,
    rightward: bool,
  },
  /// A sloped one-way run, by its id
  Run(u32),
  /// A box of a hexagonal grid's chunk: the chunk's edges, or a quarter's
  /// or a block's of a chunk split for its edge count (`HexParts`); its
  /// sides, or its shape cells' rings (`rings`)
  Hex {
    chunk: (i32, i32),
    part: HexBox,
    rings: bool,
  },
}

impl PackKey {
  /// The chunk whose top-left its part sits at: a loop pack's or a row's
  /// own; a sloped run's (a pack of one run) the chunk of `first`, its
  /// run's first point, which counts for nothing else
  pub(crate) fn chunk(self, first: Point) -> (i32, i32) {
    match self {
      PackKey::Loops(x, y) => (x, y),
      PackKey::Row { chunk, .. } | PackKey::Hex { chunk, .. } => chunk,
      PackKey::Run(_) => point_chunk(first),
    }
  }
}

/// The chunk holding an outline grid point, rounding toward negative
/// infinity.
pub(crate) fn point_chunk((x, y): Point) -> (i32, i32) {
  let side = GRID * CHUNK;
  (x.div_euclid(side), y.div_euclid(side))
}

/// A loop's corners as packed: straight runs merged, open (the edge back
/// to the first corner implied).
pub(crate) fn ring(corners: &[Point]) -> Vec<Point> {
  let mut ring = merge_straight_runs(corners);
  ring.pop();
  ring
}

/// The pack of a loop through `corners` (its points edge by edge, as
/// traced), by its first corner. Not by the first corner of its `ring`: an
/// edit that straightens the first corner (an edge it changed meeting it)
/// would move the loop to the chunk of the next corner that turns, and with
/// it every body on the loop to another collider. A retrace starts the loop
/// where it started while its first edge stands
pub(crate) fn loop_key(corners: &[Point]) -> PackKey {
  let (x, y) = point_chunk(corners.first().copied().unwrap_or_default());
  PackKey::Loops(x, y)
}

/// The pack of one-way run `id` through `points` (its ends are what it
/// collides as).
pub(crate) fn run_key(id: u32, points: &[Point]) -> PackKey {
  let (a, b) = ends(points);
  if a.1 == b.1 {
    PackKey::Row {
      chunk: point_chunk(a),
      y: a.1,
      rightward: b.0 > a.0,
    }
  } else {
    PackKey::Run(id)
  }
}

/// A run's first and last points.
pub(crate) fn ends(points: &[Point]) -> (Point, Point) {
  let first = points.first().copied().unwrap_or_default();
  (first, points.last().copied().unwrap_or(first))
}

/// The points and edges of a pack's collider, in px, as they are gathered.
#[derive(Debug, Default)]
pub(crate) struct Geometry {
  points: Vec<Vec2>,
  segments: Vec<[u32; 2]>,
  /// Loops or runs gathered
  count: usize,
}

impl Geometry {
  /// Adds a closed loop through `ring` (open: the edge back is added).
  pub(crate) fn add_ring(&mut self, ring: impl IntoIterator<Item = Vec2>) {
    let base = self.points.len() as u32;
    self.points.extend(ring);
    let n = self.points.len() as u32 - base;
    self
      .segments
      .extend((0..n).map(|i| [base + i, base + (i + 1) % n]));
    self.count += 1;
  }

  /// Adds an edge from `a` to `b`.
  pub(crate) fn add_segment(&mut self, a: Vec2, b: Vec2) {
    let base = self.points.len() as u32;
    self.points.extend([a, b]);
    self.segments.push([base, base + 1]);
    self.count += 1;
  }

  /// The collider of loops: one loop is a closed polyline, several are
  /// segments between their points.
  pub(crate) fn loops(mut self) -> Collider {
    if self.count == 1 {
      if let Some(&first) = self.points.first() {
        self.points.push(first);
      }
      return Collider::polyline(self.points);
    }
    Collider::segments(self.points, self.segments)
  }

  /// The collider of one-way runs on one line, solid from the side they
  /// face (up, for a flat run running right): one run is a segment.
  pub(crate) fn runs(self) -> Collider {
    let (a, b) = (
      self.points.first().copied().unwrap_or_default(),
      self.points.get(1).copied().unwrap_or_default(),
    );
    let along = b - a;
    let facing = Vec2::new(along.y, -along.x).normalize_or(Vec2::NEG_Y);
    let collider = if self.count == 1 {
      Collider::segment(a, b)
    } else {
      Collider::segments(self.points, self.segments)
    };
    collider.one_way_towards(facing)
  }
}

/// A pack's members (loop or run ids) and the collider built from them.
#[derive(Debug, Default)]
struct Pack {
  members: Vec<u32>,
  part: Option<Entity>,
}

/// Which loops (or runs) each collider packs.
#[derive(Debug, Default)]
pub(crate) struct Packs {
  /// Each member's pack, by its id
  of: Vec<Option<PackKey>>,
  packs: FastMap<PackKey, Pack>,
  /// The pack of each part
  parts: EntityHashMap<PackKey>,
}

impl Packs {
  /// Puts member `id` in pack `key`, out of the one it was in (an id
  /// dropped and given again to a new loop or run); both are `dirty`.
  pub(crate) fn place(&mut self, id: u32, key: PackKey, dirty: &mut Vec<PackKey>) {
    let slot = id as usize;
    if self.of.len() <= slot {
      self.of.resize(slot + 1, None);
    }
    if let Some(old) = self.of[slot].replace(key) {
      if let Some(pack) = self.packs.get_mut(&old) {
        pack.members.retain(|&member| member != id);
      }
      dirty.push(old);
    }
    self.packs.entry(key).or_default().members.push(id);
    dirty.push(key);
  }

  /// Marks `dirty` the packs of the parts of dropped loops (or runs); a
  /// part no pack holds is returned, to go.
  pub(crate) fn dropped(
    &self,
    parts: impl IntoIterator<Item = Entity>,
    dirty: &mut Vec<PackKey>,
  ) -> Vec<Entity> {
    let mut stray = Vec::new();
    for part in parts {
      match self.parts.get(&part) {
        Some(&key) => dirty.push(key),
        None => stray.push(part),
      }
    }
    stray
  }

  /// Takes out of pack `key` the members that are gone (`exists` is false)
  /// and its part, which is returned to go; its members, as they now are.
  pub(crate) fn settle(&mut self, key: PackKey, exists: impl Fn(u32) -> bool) -> Vec<u32> {
    let Some(pack) = self.packs.get_mut(&key) else {
      return Vec::new();
    };
    let of = &mut self.of;
    pack.members.retain(|&id| {
      let keep = exists(id);
      if !keep && of.get(id as usize).copied().flatten() == Some(key) {
        of[id as usize] = None;
      }
      keep
    });
    pack.members.clone()
  }

  /// Takes pack `key`'s part, to go; a pack left with no members goes too.
  pub(crate) fn take_part(&mut self, key: PackKey) -> Option<Entity> {
    let pack = self.packs.get_mut(&key)?;
    let part = pack.part.take();
    if pack.members.is_empty() {
      self.packs.remove(&key);
    }
    if let Some(part) = part {
      self.parts.remove(&part);
    }
    part
  }

  /// Pack `key`'s part.
  pub(crate) fn part(&self, key: PackKey) -> Option<Entity> {
    self.packs.get(&key).and_then(|pack| pack.part)
  }

  /// Records `part` as pack `key`'s collider.
  pub(crate) fn set_part(&mut self, key: PackKey, part: Entity) {
    if let Some(pack) = self.packs.get_mut(&key) {
      pack.part = Some(part);
      self.parts.insert(part, key);
    }
  }

  /// Pack `key`'s members.
  pub(crate) fn members(&self, key: PackKey) -> &[u32] {
    self.packs.get(&key).map_or(&[], |pack| &pack.members)
  }

  /// How many packs there are.
  #[cfg(test)]
  pub(crate) fn len(&self) -> usize {
    self.packs.len()
  }
}

/// Cells a side of the smallest box a part of a hexagonal chunk's sides
/// holds: a chunk's quarters (16 x 16) split into these blocks, which
/// never split for their sides
pub(crate) const HEX_BLOCK: i32 = 8;

/// The most edges a hexagonal part holds (B): a chunk's sides, and its
/// rings apart, are one part while they hold at most this many, else its
/// quarters' are, each while they hold at most this many, else that
/// quarter's blocks' are (and a block's rings past it, its sub-blocks')
pub(crate) const HEX_PART_EDGES: usize = 128;

/// Blocks a side of a chunk
const BLOCKS: i32 = CHUNK / HEX_BLOCK;

/// Cells a side of a block's sub-blocks (4 x 4), the smallest box a part
/// of rings holds: a block's rings past the budget split into these, which
/// never split
const HEX_SUB: i32 = 4;

/// Sub-blocks a side of a chunk
const SUBS: i32 = CHUNK / HEX_SUB;

/// Cells a side of a chunk's quarter
const QUARTER: i32 = CHUNK / 2;

/// An edge, from its start to its end
type Edge = (Point, Point);

/// Which box of a hexagonal chunk a part holds the edges of
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) enum HexBox {
  /// The whole chunk (32 x 32 cells)
  Chunk,
  /// A quarter (16 x 16 cells), by index row by row
  Quarter(u8),
  /// A block (8 x 8 cells), by index row by row over the chunk
  Block(u8),
  /// A sub-block (4 x 4 cells), by index row by row over the chunk: parts
  /// of rings only
  Sub(u8),
}

/// The blocks of quarter `q`, by index
fn quarter_blocks(q: usize) -> [usize; 4] {
  let first = q / 2 * 8 + q % 2 * 2;
  [first, first + 1, first + 4, first + 5]
}

/// The quarter of block `b`
fn block_quarter(b: usize) -> usize {
  b / 8 * 2 + b % 4 / 2
}

/// The sub-blocks of block `b`, by index over the chunk, row by row
fn block_subs(b: usize) -> [usize; 4] {
  let first = b / 4 * 16 + b % 4 * 2;
  [first, first + 1, first + 8, first + 9]
}

/// The block of sub-block `s`, and which of that block's four it is (row
/// by row)
fn sub_block(s: usize) -> (usize, usize) {
  (s / 16 * 4 + s % 8 / 2, s / 8 % 2 * 2 + s % 2)
}

/// Which of its block's four sub-blocks `cell` is in (row by row)
fn sub_of(cell: IVec2) -> usize {
  let at = cell.rem_euclid(IVec2::splat(HEX_BLOCK)) / HEX_SUB;
  (at.y * 2 + at.x) as usize
}

/// What a block of a hexagonal chunk holds: what its cells own
#[derive(Debug, Clone, Default)]
struct HexBlock {
  /// The boundary edges along its cells' sides, in row order (each cell's
  /// sides in turn)
  edges: Vec<Edge>,
  /// Its shape cells with rings, in row order, each with its shape: each
  /// collides as its shape's rings (`ShapeBuild::rings`)
  rings: Vec<(IVec2, u16)>,
  /// How many edges those rings hold in each of its four sub-blocks (row
  /// by row)
  ring_edges: [usize; 4],
}

impl HexBlock {
  /// How many edges its sides hold, or its rings
  fn len(&self, rings: bool) -> usize {
    if rings {
      self.ring_edges.iter().sum()
    } else {
      self.edges.len()
    }
  }

  fn is_empty(&self) -> bool {
    self.edges.is_empty() && self.rings.is_empty()
  }

  fn clear(&mut self) {
    self.edges.clear();
    self.rings.clear();
    self.ring_edges = [0; 4];
  }

  /// Its shape cells with rings in its sub-block `s` (of its four, row by
  /// row)
  fn sub_rings(&self, s: usize) -> impl Iterator<Item = (IVec2, u16)> + '_ {
    self
      .rings
      .iter()
      .copied()
      .filter(move |&(cell, _)| sub_of(cell) == s)
  }

  /// Its sub-blocks whose rings are not `other`'s, a bit each
  fn subs_changed(&self, other: &Self) -> u8 {
    if self.rings == other.rings && self.ring_edges == other.ring_edges {
      return 0;
    }
    (0..4)
      .filter(|&s| {
        self.ring_edges[s] != other.ring_edges[s] || !self.sub_rings(s).eq(other.sub_rings(s))
      })
      .fold(0, |subs, s| subs | 1 << s)
  }

  /// Takes in what `cell` owns: the boundary edges along its sides (each
  /// side with one solid cell, read through `solid`), and, a shape cell
  /// that collides (`reaches`), its rings
  fn read(
    &mut self,
    sides: &HexSides,
    cell: IVec2,
    (cells, read): (Cells, &impl Fn(i32, i32) -> TileCell),
    solid: &impl Fn(i32, i32) -> bool,
  ) {
    hex_owned_edges(sides, cell, solid, &mut self.edges);
    let kind = read(cell.x, cell.y);
    if let TileCell::Shape(shape) = kind {
      let n = cells.ring_edges(kind);
      if n > 0 && sides.hex.reaches(cell) {
        self.rings.push((cell, shape));
        self.ring_edges[sub_of(cell)] += n;
      }
    }
  }
}

/// How a hexagonal chunk's sides, or its rings, are split into parts: each
/// level splits once it holds more than `HEX_PART_EDGES`, and stays split
/// until a build afresh, or until the chunk holds none of them
#[derive(Debug, Default)]
struct HexSplit {
  /// How many edges in all
  edges: usize,
  /// Its parts are its quarters'
  chunk: bool,
  /// Each quarter's parts are its blocks'
  quarters: [bool; 4],
  /// Each block's parts are its sub-blocks' (rings only)
  blocks: [bool; (BLOCKS * BLOCKS) as usize],
}

/// A hexagonal chunk's boundary edges and rings, by the block of the cell
/// owning each, and how each kind is split into parts
#[derive(Debug)]
struct HexChunk {
  /// Each block's, row by row of blocks
  blocks: Vec<HexBlock>,
  /// Its sides' parts
  sides: HexSplit,
  /// Its rings' parts
  rings: HexSplit,
}

impl HexChunk {
  fn new() -> Self {
    Self {
      blocks: vec![HexBlock::default(); (BLOCKS * BLOCKS) as usize],
      sides: HexSplit::default(),
      rings: HexSplit::default(),
    }
  }

  /// How its sides, or its rings, are split
  fn split(&mut self, rings: bool) -> &mut HexSplit {
    if rings {
      &mut self.rings
    } else {
      &mut self.sides
    }
  }

  /// How many edges of its sides, or its rings, quarter `q` holds
  fn quarter_len(&self, rings: bool, q: usize) -> usize {
    quarter_blocks(q)
      .iter()
      .map(|&b| self.blocks[b].len(rings))
      .sum()
  }

  /// The boxes of its parts of sides, or of rings, each holding an edge:
  /// the chunk, or its quarters and the blocks of those split (and the
  /// sub-blocks of a block whose rings split)
  fn boxes(&self, rings: bool, out: &mut Vec<HexBox>) {
    let split = if rings { &self.rings } else { &self.sides };
    if split.edges == 0 {
      return;
    }
    if !split.chunk {
      out.push(HexBox::Chunk);
      return;
    }
    for q in 0..4 {
      if split.quarters[q] {
        for b in quarter_blocks(q) {
          self.block_boxes(rings, b, out);
        }
      } else if self.quarter_len(rings, q) > 0 {
        out.push(HexBox::Quarter(q as u8));
      }
    }
  }

  /// The boxes of block `b`'s parts of sides, or of rings, each holding an
  /// edge: its own, or its sub-blocks' where its rings split
  fn block_boxes(&self, rings: bool, b: usize, out: &mut Vec<HexBox>) {
    let block = &self.blocks[b];
    if rings && self.rings.blocks[b] {
      let held = block_subs(b).into_iter().zip(block.ring_edges);
      out.extend(
        held
          .filter(|&(_, n)| n > 0)
          .map(|(s, _)| HexBox::Sub(s as u8)),
      );
    } else if block.len(rings) > 0 {
      out.push(HexBox::Block(b as u8));
    }
  }

  /// Decides quarter `q`'s parts of sides, or of rings, afresh: its
  /// blocks' if it holds more than `HEX_PART_EDGES`, else its own if any
  fn settle_quarter(&mut self, rings: bool, q: usize, rebuilt: &mut Vec<HexBox>) {
    let edges = self.quarter_len(rings, q);
    if edges > HEX_PART_EDGES {
      self.split(rings).quarters[q] = true;
      for b in quarter_blocks(q) {
        self.settle_block(rings, b, rebuilt);
      }
    } else if edges > 0 {
      rebuilt.push(HexBox::Quarter(q as u8));
    }
  }

  /// Decides block `b`'s parts afresh: its sides are one part, and its
  /// rings are its sub-blocks' if they hold more than `HEX_PART_EDGES`,
  /// else one part
  fn settle_block(&mut self, rings: bool, b: usize, rebuilt: &mut Vec<HexBox>) {
    if rings && self.blocks[b].len(true) > HEX_PART_EDGES {
      self.rings.blocks[b] = true;
    }
    self.block_boxes(rings, b, rebuilt);
  }

  /// Decides each box's parts of sides, or of rings, by its count, as a
  /// build afresh does
  fn settle(&mut self, rings: bool, scratch: &mut Vec<HexBox>) {
    let split = self.split(rings);
    split.chunk = split.edges > HEX_PART_EDGES;
    if split.chunk {
      for q in 0..4 {
        self.settle_quarter(rings, q, scratch);
      }
    }
    scratch.clear();
  }

  /// Decides what an edit changed of its parts of sides, or of rings:
  /// `moved` are the blocks whose sides (or rings) changed, in quarter
  /// order, each with the sub-blocks whose rings changed, a bit each. The
  /// parts to build again, or make, go in `rebuilt`, those to drop in
  /// `gone` (all of a box's new parts as it splits)
  fn update(
    &mut self,
    rings: bool,
    moved: &[(usize, u8)],
    rebuilt: &mut Vec<HexBox>,
    gone: &mut Vec<HexBox>,
  ) {
    if moved.is_empty() {
      return;
    }
    let split = self.split(rings);
    if split.edges == 0 {
      // Its parts go, and its splits with them
      *split = HexSplit::default();
      gone.push(HexBox::Chunk);
      gone.extend((0..4).map(HexBox::Quarter));
      gone.extend((0..BLOCKS * BLOCKS).map(|b| HexBox::Block(b as u8)));
      if rings {
        gone.extend((0..SUBS * SUBS).map(|s| HexBox::Sub(s as u8)));
      }
    } else if !split.chunk && split.edges <= HEX_PART_EDGES {
      rebuilt.push(HexBox::Chunk);
    } else if !split.chunk {
      // Past the budget: the chunk's part goes, its quarters' come
      split.chunk = true;
      gone.push(HexBox::Chunk);
      for q in 0..4 {
        self.settle_quarter(rings, q, rebuilt);
      }
    } else {
      for quarter in moved.chunk_by(|a, b| block_quarter(a.0) == block_quarter(b.0)) {
        let q = block_quarter(quarter[0].0);
        if !self.split(rings).quarters[q] {
          // Gone empty, or past the budget (its blocks' parts come): the
          // quarter's part goes
          let edges = self.quarter_len(rings, q);
          if edges == 0 || edges > HEX_PART_EDGES {
            gone.push(HexBox::Quarter(q as u8));
          }
          self.settle_quarter(rings, q, rebuilt);
          continue;
        }
        for &(b, subs) in quarter {
          if rings && self.rings.blocks[b] {
            let changed = block_subs(b)
              .into_iter()
              .enumerate()
              .filter(|&(i, _)| subs & 1 << i != 0);
            for (i, s) in changed {
              let part = HexBox::Sub(s as u8);
              if self.blocks[b].ring_edges[i] == 0 {
                gone.push(part);
              } else {
                rebuilt.push(part);
              }
            }
          } else {
            // Gone empty, or its rings past the budget (its sub-blocks'
            // parts come): the block's part goes
            let edges = self.blocks[b].len(rings);
            if edges == 0 || (rings && edges > HEX_PART_EDGES) {
              gone.push(HexBox::Block(b as u8));
            }
            self.settle_block(rings, b, rebuilt);
          }
        }
      }
    }
  }
}

/// The chunk and block (by its index) of `cell`
fn hex_block(cell: IVec2) -> (IVec2, usize) {
  let chunk = cell.div_euclid(IVec2::splat(CHUNK));
  let at = cell.rem_euclid(IVec2::splat(CHUNK)) / HEX_BLOCK;
  (chunk, (at.y * BLOCKS + at.x) as usize)
}

/// The first cell (its top-left) of hexagonal part `key`: its chunk's, its
/// quarter's, its block's or its sub-block's
pub(crate) fn hex_first_cell(key: PackKey) -> IVec2 {
  let PackKey::Hex { chunk, part, .. } = key else {
    return IVec2::ZERO;
  };
  let first = IVec2::new(chunk.0, chunk.1) * CHUNK;
  match part {
    HexBox::Chunk => first,
    HexBox::Quarter(q) => first + IVec2::new(i32::from(q) % 2, i32::from(q) / 2) * QUARTER,
    HexBox::Block(b) => {
      first + IVec2::new(i32::from(b) % BLOCKS, i32::from(b) / BLOCKS) * HEX_BLOCK
    }
    HexBox::Sub(s) => first + IVec2::new(i32::from(s) % SUBS, i32::from(s) / SUBS) * HEX_SUB,
  }
}

/// A hexagonal grid's colliders: the boundary edges along the sides each
/// cell owns (the earlier of a side's two cells in row order; of a side on
/// a bounded grid's border, the cell inside it), and each shape cell's
/// rings, its own, kept by chunk and block, and each part's entity. A
/// chunk's sides and its rings are parts apart, each holding at most
/// `HEX_PART_EDGES` (B) edges where it can, its sides' and its rings' split
/// each by their own count: a chunk's are one part while they hold at most
/// B; past that its quarters' are, each while they hold at most B; past
/// that the quarter's blocks' are. A block's sides never split: its cells
/// own at most about 190 (a dense pattern about B, a little more along the
/// block's right column and last row). A block's rings past B split into
/// its four sub-blocks of 4 x 4 cells, which never split, so a sub-block
/// heavy with rings holds more than B by as much as its art has (a block
/// of inset hexagons, six ring edges a cell, holds 384, each of its
/// sub-blocks 96). Landing decides each box by its count, and an edit that
/// takes a part past B splits it (its part dropped and its box's parts
/// made); nothing merges back until a build afresh, or until the chunk
/// holds none of that kind. An edit rebuilds the parts of sides of the
/// blocks owning its cells' sides, which lie in up to four chunks round
/// its cell, so at most four parts of about B edges where their blocks
/// keep within it (more as one splits), and the part of rings holding a
/// cell only where that cell's rings changed, so an edit of sides beside
/// cells heavy with rings rebuilds none of them; it makes a part where a
/// box held no edge of its kind till then, or drops one it empties.
#[derive(Debug, Default)]
pub(crate) struct HexParts {
  chunks: FastMap<IVec2, HexChunk>,
  parts: FastMap<PackKey, Entity>,
}

/// What an edit changed of a hexagonal grid's parts.
#[derive(Debug, Default)]
pub(crate) struct HexChanges {
  /// The parts to build again, or make: each holds an edge
  pub(crate) rebuilt: Vec<PackKey>,
  /// The parts gone, to despawn
  pub(crate) gone: Vec<Entity>,
}

impl HexParts {
  /// Every edge of the grid `cells` of `hex`'s cells, by chunk and block,
  /// each box's parts decided by its count. `bounds` are the grid's
  /// (`TileColliders::bounds`): on a bounded grid a side between a cell
  /// inside them and one outside belongs to the inside cell, so no part
  /// sits outside the bounds. A cell whose corners or its neighbours' pass
  /// an `i32` (`HexCells::reaches`) is empty here, and a grid the tracer
  /// does not take has none
  pub(crate) fn build(cells: Cells, hex: HexCells, bounds: Option<CellRect>) -> Self {
    let mut parts = Self::default();
    if !hex.traceable() {
      return parts;
    }
    let sides = HexSides::of(hex, bounds);
    let read = cells.cached();
    let solid = |x: i32, y: i32| read(x, y) == TileCell::Solid && hex.reaches(IVec2::new(x, y));
    let mut owned = HexBlock::default();
    each_hex_owner(cells, bounds, |cell| {
      owned.clear();
      owned.read(&sides, cell, (cells, &read), &solid);
      if !owned.is_empty() {
        let (chunk, block) = hex_block(cell);
        let held = parts.chunks.entry(chunk).or_insert_with(HexChunk::new);
        held.sides.edges += owned.len(false);
        held.rings.edges += owned.len(true);
        let into = &mut held.blocks[block];
        into.edges.extend_from_slice(&owned.edges);
        into.rings.extend_from_slice(&owned.rings);
        for (n, more) in into.ring_edges.iter_mut().zip(owned.ring_edges) {
          *n += more;
        }
      }
    });
    let mut scratch = Vec::new();
    for held in parts.chunks.values_mut() {
      held.settle(false, &mut scratch);
      held.settle(true, &mut scratch);
    }
    parts
  }

  /// Takes in the cells at `changed` (now as `cells` holds them; `bounds`
  /// as `build` took them): the blocks owning their sides (and their own,
  /// which holds a shape cell's rings) are read again, and only the parts
  /// whose boxes hold what changed of those blocks are rebuilt: the parts
  /// of sides of the blocks whose sides changed, and the parts of rings of
  /// the blocks (or sub-blocks) whose rings changed (all of a box's new
  /// parts as it splits)
  pub(crate) fn apply(
    &mut self,
    cells: Cells,
    hex: HexCells,
    bounds: Option<CellRect>,
    changed: &[(IVec2, TileCell)],
  ) -> HexChanges {
    let mut changes = HexChanges::default();
    if !hex.traceable() || changed.is_empty() {
      return changes;
    }
    let sides = HexSides::of(hex, bounds);
    let mut dirty: Vec<(IVec2, usize)> = Vec::with_capacity(changed.len() * 7);
    for &(cell, _) in changed {
      dirty.push(hex_block(cell));
      for side in 0..6 {
        let (across, owned) = sides.across(cell, side);
        dirty.push(hex_block(if owned { cell } else { across }));
      }
    }
    dirty.sort_unstable_by_key(|&(chunk, block)| (chunk.y, chunk.x, block_quarter(block), block));
    dirty.dedup();
    let read = cells.cached();
    let solid = |x: i32, y: i32| read(x, y) == TileCell::Solid && hex.reaches(IVec2::new(x, y));
    let Self { chunks, parts } = self;
    let mut owned = HexBlock::default();
    let mut rebuilt: Vec<HexBox> = Vec::new();
    let mut gone: Vec<HexBox> = Vec::new();
    // The blocks whose sides, and whose rings, the edit changed
    let mut moved_sides: Vec<(usize, u8)> = Vec::with_capacity(dirty.len());
    let mut moved_rings: Vec<(usize, u8)> = Vec::new();
    for group in dirty.chunk_by(|a, b| a.0 == b.0) {
      let chunk = group[0].0;
      let held = chunks.entry(chunk).or_insert_with(HexChunk::new);
      moved_sides.clear();
      moved_rings.clear();
      for &(_, block) in group {
        owned.clear();
        let first = hex_first_cell(PackKey::Hex {
          chunk: (chunk.x, chunk.y),
          part: HexBox::Block(block as u8),
          rings: false,
        });
        for y in 0..HEX_BLOCK {
          for x in 0..HEX_BLOCK {
            owned.read(&sides, first + IVec2::new(x, y), (cells, &read), &solid);
          }
        }
        let was = &mut held.blocks[block];
        let new_sides = was.edges != owned.edges;
        let subs = was.subs_changed(&owned);
        if new_sides {
          held.sides.edges = held.sides.edges - was.len(false) + owned.len(false);
          moved_sides.push((block, 0));
        }
        if subs != 0 {
          held.rings.edges = held.rings.edges - was.len(true) + owned.len(true);
          moved_rings.push((block, subs));
        }
        if new_sides || subs != 0 {
          was.clone_from(&owned);
        }
      }
      for (rings, moved) in [(false, &moved_sides), (true, &moved_rings)] {
        rebuilt.clear();
        gone.clear();
        held.update(rings, moved, &mut rebuilt, &mut gone);
        let key = |part: HexBox| PackKey::Hex {
          chunk: (chunk.x, chunk.y),
          part,
          rings,
        };
        changes
          .gone
          .extend(gone.iter().filter_map(|&part| parts.remove(&key(part))));
        changes
          .rebuilt
          .extend(rebuilt.iter().map(|&part| key(part)));
      }
      if held.sides.edges == 0 && held.rings.edges == 0 {
        chunks.remove(&chunk);
      }
    }
    changes
  }

  /// Every part's key, chunk by chunk in row order, each chunk's parts of
  /// sides, then its parts of rings, box by box
  pub(crate) fn keys(&self) -> Vec<PackKey> {
    let mut chunks: Vec<(&IVec2, &HexChunk)> = self.chunks.iter().collect();
    chunks.sort_unstable_by_key(|(chunk, _)| (chunk.y, chunk.x));
    let (mut keys, mut boxes) = (Vec::new(), Vec::new());
    for (chunk, held) in chunks {
      let chunk = (chunk.x, chunk.y);
      for rings in [false, true] {
        boxes.clear();
        held.boxes(rings, &mut boxes);
        keys.extend(
          boxes
            .iter()
            .map(|&part| PackKey::Hex { chunk, part, rings }),
        );
      }
    }
    keys
  }

  /// The blocks part `key`'s box covers, and, for a sub-block, which of its
  /// block's four it is
  fn part_blocks(&self, key: PackKey) -> (Vec<&HexBlock>, Option<usize>) {
    let PackKey::Hex { chunk, part, .. } = key else {
      return (Vec::new(), None);
    };
    let Some(held) = self.chunks.get(&IVec2::new(chunk.0, chunk.1)) else {
      return (Vec::new(), None);
    };
    let block = |b: usize| held.blocks.get(b);
    match part {
      HexBox::Chunk => (held.blocks.iter().collect(), None),
      HexBox::Quarter(q) => (
        quarter_blocks(usize::from(q))
          .into_iter()
          .filter_map(block)
          .collect(),
        None,
      ),
      HexBox::Block(b) => (block(usize::from(b)).into_iter().collect(), None),
      HexBox::Sub(s) => {
        let (b, i) = sub_block(usize::from(s));
        (block(b).into_iter().collect(), Some(i))
      }
    }
  }

  /// Part `key`'s edges, in the grid's points: a part of sides' (a part of
  /// rings holds none)
  pub(crate) fn edges(&self, key: PackKey) -> impl Iterator<Item = Edge> + '_ {
    let blocks = match key {
      PackKey::Hex { rings: false, .. } => self.part_blocks(key).0,
      _ => Vec::new(),
    };
    blocks
      .into_iter()
      .flat_map(|block| block.edges.iter().copied())
  }

  /// Part `key`'s shape cells with rings, each with its shape: a part of
  /// rings' (a part of sides holds none)
  pub(crate) fn rings(&self, key: PackKey) -> impl Iterator<Item = (IVec2, u16)> + '_ {
    let (blocks, sub) = match key {
      PackKey::Hex { rings: true, .. } => self.part_blocks(key),
      _ => (Vec::new(), None),
    };
    blocks
      .into_iter()
      .flat_map(|block| block.rings.iter().copied())
      .filter(move |&(cell, _)| sub.is_none_or(|s| sub_of(cell) == s))
  }

  /// Part `key`'s entity
  pub(crate) fn part(&self, key: PackKey) -> Option<Entity> {
    self.parts.get(&key).copied()
  }

  /// Takes part `key`'s entity out (to go)
  pub(crate) fn take_part(&mut self, key: PackKey) -> Option<Entity> {
    self.parts.remove(&key)
  }

  /// Records `part` as part `key`'s entity
  pub(crate) fn set_part(&mut self, key: PackKey, part: Entity) {
    self.parts.insert(key, part);
  }

  /// Every part with its entity
  #[cfg(test)]
  pub(crate) fn parts(&self) -> impl Iterator<Item = (PackKey, Entity)> + '_ {
    self.parts.iter().map(|(&key, &part)| (key, part))
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn a_loop_belongs_to_the_chunk_of_its_first_corner() {
    let side = GRID * CHUNK;
    assert_eq!(loop_key(&[(0, 0), (GRID, 0)]), PackKey::Loops(0, 0));
    assert_eq!(loop_key(&[(side - 1, side)]), PackKey::Loops(0, 1));
    assert_eq!(loop_key(&[(side, 2 * side + 5)]), PackKey::Loops(1, 2));
  }

  #[test]
  fn flat_runs_share_a_row_and_sloped_runs_are_alone() {
    let flat = run_key(3, &[(0, GRID), (GRID, GRID), (2 * GRID, GRID)]);
    assert_eq!(
      flat,
      PackKey::Row {
        chunk: (0, 0),
        y: GRID,
        rightward: true
      }
    );
    assert_eq!(run_key(4, &[(5 * GRID, GRID), (6 * GRID, GRID)]), flat);
    assert_eq!(run_key(5, &[(0, GRID), (GRID, 0)]), PackKey::Run(5));
  }

  #[test]
  fn a_member_given_again_leaves_its_old_pack() {
    let (a, b) = (PackKey::Loops(0, 0), PackKey::Loops(1, 0));
    let mut packs = Packs::default();
    let mut dirty = Vec::new();
    packs.place(0, a, &mut dirty);
    packs.place(1, a, &mut dirty);
    assert_eq!(dirty, [a, a]);
    dirty.clear();
    packs.place(1, b, &mut dirty);
    assert_eq!(dirty, [a, b], "both packs change");
    assert_eq!(packs.members(a), [0]);
    assert_eq!(packs.members(b), [1]);
    // A member gone leaves its pack, and the pack with none goes
    assert_eq!(packs.settle(b, |_| false), Vec::<u32>::new());
    assert_eq!(packs.take_part(b), None);
    assert_eq!(packs.len(), 1);
  }

  #[test]
  fn several_loops_are_segments_and_one_is_a_closed_polyline() {
    let square = [Vec2::ZERO, Vec2::X, Vec2::ONE, Vec2::Y];
    let mut one = Geometry::default();
    one.add_ring(square);
    assert_eq!(
      one.loops().shape,
      Collider::polyline(square.into_iter().chain([Vec2::ZERO])).shape
    );
    let mut two = Geometry::default();
    two.add_ring(square);
    two.add_ring(square.map(|p| p + Vec2::splat(3.0)));
    let collider = two.loops();
    let crate::core::physics::ColliderShape::Segments { points, segments } = collider.shape else {
      panic!("{collider:?}");
    };
    assert_eq!(points.len(), 8, "no point twice");
    assert_eq!(
      segments,
      [
        [0, 1],
        [1, 2],
        [2, 3],
        [3, 0],
        [4, 5],
        [5, 6],
        [6, 7],
        [7, 4]
      ]
    );
  }
}
