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

use super::{GRID, Point, merge_straight_runs};
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
}

impl PackKey {
  /// The chunk whose top-left its part sits at: a loop pack's or a row's
  /// own; a sloped run's (a pack of one run) the chunk of `first`, its
  /// run's first point, which counts for nothing else
  pub(crate) fn chunk(self, first: Point) -> (i32, i32) {
    match self {
      PackKey::Loops(x, y) => (x, y),
      PackKey::Row { chunk, .. } => chunk,
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
