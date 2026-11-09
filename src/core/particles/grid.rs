//! A uniform grid of the colliders and edges a pass gathered, so each
//! particle tests only those near its move. A pass that gathers colliders
//! rebuilds it, into the buffers it keeps; a query lists what lies in the
//! cells a box covers.

use glam::Vec2;

use super::contact::{ColliderGeometry, ParticleCollider};

/// The narrowest and the widest a cell is, in world units
const CELL_SIDES: (f32, f32) = (16.0, 256.0);

/// The most cells a grid holds: colliders spread wider double the cell's
/// side until they fit
const MAX_CELLS: usize = 1 << 16;

/// The most cells one collider or edge goes in: a larger one is found by
/// every query instead, as every collider was before there was a grid
const MAX_ITEM_CELLS: usize = 1 << 10;

/// How much further than its reach a particle's query looks, and a share
/// of its coordinates' size more: so neither the 1e-3 within which
/// `solid_contact` adds a corner's normals nor rounding leaves out an edge
/// a contact reads
const QUERY_SLACK: f32 = 0.5;
const QUERY_SLACK_SHARE: f32 = 1.0 / 65_536.0;

/// An edge's collider when no collider holds it
const NO_OWNER: u32 = u32::MAX;

/// The gathered colliders and edges, cell by cell, so a query finds those
/// near a particle's move. Circles and boxes go in the cells their box
/// covers, an edges collider's edges in the cells each edge's box covers.
/// A collider or edge with no bounds (not a number), or covering more than
/// `MAX_ITEM_CELLS`, is found by every query. Its buffers are kept from one
/// rebuild to the next
#[derive(Debug, Default)]
pub(crate) struct ColliderGrid {
  lattice: Lattice,
  /// The greatest corner of what the cells hold (the least is the
  /// lattice's origin)
  end: Vec2,
  /// The circles and boxes in each cell
  colliders: Cells,
  /// The edges in each cell
  edges: Cells,
  /// The colliders every query finds, ascending
  everywhere_colliders: Vec<u32>,
  /// The edges every query finds, ascending
  everywhere_edges: Vec<u32>,
  /// The collider each edge is one of
  owners: Vec<u32>,
  /// The largest reach of the gathered colliders: a tile part's half cell
  reach: f32,
  /// How many colliders and edges were gathered
  gathered: (u32, u32),
}

/// What a query found: the colliders a particle's move may meet, and the
/// edges of theirs it may meet
#[derive(Debug, Default)]
pub(crate) struct Candidates {
  /// Indices into the gathered colliders: in gathered order, no repeats
  pub(crate) colliders: Vec<u32>,
  /// Indices into the gathered edges: ascending, no repeats
  pub(crate) edges: Vec<u32>,
}

/// Where the cells lie: square, `side` wide, cell (0, 0) at `origin`, and
/// none when `columns` is 0
#[derive(Debug, Default, Clone, Copy)]
struct Lattice {
  origin: Vec2,
  side: f32,
  columns: usize,
  rows: usize,
}

/// The cells a box covers: its first and last column and row
#[derive(Debug, Clone, Copy)]
struct Span {
  columns: (usize, usize),
  rows: (usize, usize),
}

impl Lattice {
  /// The cells covering `min` to `max` (a box past the cells takes the
  /// cells at their edge)
  fn span(self, min: Vec2, max: Vec2) -> Span {
    let column = |x: f32| index(x, self.origin.x, self.side, self.columns);
    let row = |y: f32| index(y, self.origin.y, self.side, self.rows);
    Span {
      columns: (column(min.x), column(max.x)),
      rows: (row(min.y), row(max.y)),
    }
  }

  /// Each cell of `span`, row by row
  fn cells(self, span: Span) -> impl Iterator<Item = usize> {
    let (columns, across) = (span.columns, self.columns);
    (span.rows.0..=span.rows.1)
      .flat_map(move |row| (columns.0..=columns.1).map(move |column| row * across + column))
  }
}

impl Span {
  /// How many cells it covers
  fn len(self) -> usize {
    let across = self.columns.1.saturating_sub(self.columns.0) + 1;
    let down = self.rows.1.saturating_sub(self.rows.0) + 1;
    across.saturating_mul(down)
  }
}

/// The cell, of `count` from `origin` each `side` wide, that `at` falls in
/// (the first or last for a place before or past them). It never goes
/// down as `at` goes up, so a box meeting another covers a cell of it
fn index(at: f32, origin: f32, side: f32, count: usize) -> usize {
  let cell = ((at - origin) / side).floor();
  cell.clamp(0.0, count.saturating_sub(1) as f32) as usize
}

/// One kind of thing in the cells: each cell's ids, back to back
#[derive(Debug, Default)]
struct Cells {
  /// Where each cell's ids start in `ids`, then where the last one's end
  starts: Vec<usize>,
  /// The ids, ascending within a cell
  ids: Vec<u32>,
}

impl Cells {
  fn clear(&mut self) {
    self.starts.clear();
    self.ids.clear();
  }

  /// The ids in `cell`
  fn of(&self, cell: usize) -> &[u32] {
    match (self.starts.get(cell), self.starts.get(cell + 1)) {
      (Some(&start), Some(&end)) => self.ids.get(start..end).unwrap_or(&[]),
      _ => &[],
    }
  }

  /// Puts each of `items` (an id and its box) in the `lattice`'s cells
  /// its box covers, or in `everywhere` when it covers more than
  /// `MAX_ITEM_CELLS`
  fn fill<I>(&mut self, lattice: Lattice, items: I, everywhere: &mut Vec<u32>)
  where
    I: DoubleEndedIterator<Item = (u32, Vec2, Vec2)> + Clone,
  {
    let count = lattice.columns * lattice.rows;
    let Self { starts, ids } = self;
    starts.clear();
    starts.resize(count + 1, 0);
    for (id, min, max) in items.clone() {
      let span = lattice.span(min, max);
      if span.len() > MAX_ITEM_CELLS {
        everywhere.push(id);
        continue;
      }
      for cell in lattice.cells(span) {
        starts[cell] += 1;
      }
    }
    // Each cell's end, then filled back to front: each cell's ids end up
    // ascending, and its entry at its start
    let mut total = 0;
    for start in &mut starts[..count] {
      total += *start;
      *start = total;
    }
    starts[count] = total;
    ids.clear();
    ids.resize(total, 0);
    for (id, min, max) in items.rev() {
      let span = lattice.span(min, max);
      if span.len() > MAX_ITEM_CELLS {
        continue;
      }
      for cell in lattice.cells(span) {
        starts[cell] -= 1;
        ids[starts[cell]] = id;
      }
    }
  }
}

/// Where a collider goes in the cells
enum Place {
  /// In the cells this box covers
  Box(Vec2, Vec2),
  /// Found by every query: its bounds are not numbers
  Everywhere,
  /// In the cells by its edges
  ByEdges,
}

impl Place {
  fn of(collider: &ParticleCollider) -> Self {
    let bounded = |min: Vec2, max: Vec2| {
      if min.is_finite() && max.is_finite() {
        Place::Box(min.min(max), min.max(max))
      } else {
        Place::Everywhere
      }
    };
    match collider.geometry {
      ColliderGeometry::Box {
        min_x,
        min_y,
        max_x,
        max_y,
      } => bounded(Vec2::new(min_x, min_y), Vec2::new(max_x, max_y)),
      ColliderGeometry::Circle { x, y, radius } => {
        let (centre, radius) = (Vec2::new(x, y), radius.abs());
        bounded(centre - radius, centre + radius)
      }
      ColliderGeometry::Edges { .. } => Place::ByEdges,
    }
  }
}

/// The box of edge `a`-`b`, if a contact reads it: one whose length is 0
/// or not a number never meets a particle
fn edge_box((a, b): (Vec2, Vec2)) -> Option<(Vec2, Vec2)> {
  let length = (b - a).length();
  (length > 0.0 && length.is_finite()).then(|| (a.min(b), a.max(b)))
}

/// An index as an id
fn id(index: usize) -> u32 {
  u32::try_from(index).unwrap_or(u32::MAX)
}

impl ColliderGrid {
  /// Holds nothing, keeping its buffers' room
  pub(crate) fn clear(&mut self) {
    self.lattice = Lattice::default();
    self.end = Vec2::ZERO;
    self.colliders.clear();
    self.edges.clear();
    self.everywhere_colliders.clear();
    self.everywhere_edges.clear();
    self.owners.clear();
    self.reach = 0.0;
    self.gathered = (0, 0);
  }

  /// Holds a pass's gathered `colliders` and their `edges`. Its cells are
  /// twice the mean edge's length wide, or, with no edges, the mean of
  /// the circles' and boxes' sides, within 16 to 256 world units (wider
  /// when the colliders spread past `MAX_CELLS` of them)
  pub(crate) fn rebuild(&mut self, colliders: &[ParticleCollider], edges: &[(Vec2, Vec2)]) {
    self.clear();
    self.own(colliders, edges.len());
    let (mut low, mut high) = (Vec2::INFINITY, Vec2::NEG_INFINITY);
    let (mut sides, mut boxes) = (0.0_f64, 0_u32);
    for (k, collider) in colliders.iter().enumerate() {
      match Place::of(collider) {
        Place::Box(min, max) => {
          (low, high) = (low.min(min), high.max(max));
          sides += (f64::from(max.x - min.x) + f64::from(max.y - min.y)) / 2.0;
          boxes += 1;
        }
        Place::Everywhere => self.everywhere_colliders.push(id(k)),
        Place::ByEdges => {}
      }
    }
    let (mut lengths, mut lines) = (0.0_f64, 0_u32);
    let owned = edges.iter().zip(&self.owners);
    for (&edge, _) in owned.filter(|&(_, &owner)| owner != NO_OWNER) {
      if let Some((min, max)) = edge_box(edge) {
        (low, high) = (low.min(min), high.max(max));
        lengths += f64::from((edge.1 - edge.0).length());
        lines += 1;
      }
    }
    if boxes == 0 && lines == 0 {
      return;
    }
    let side = if lines > 0 {
      2.0 * lengths / f64::from(lines)
    } else {
      sides / f64::from(boxes)
    };
    let mut side = (side as f32).clamp(CELL_SIDES.0, CELL_SIDES.1);
    let span = (
      f64::from(high.x) - f64::from(low.x),
      f64::from(high.y) - f64::from(low.y),
    );
    let (columns, rows) = loop {
      let across = (span.0 / f64::from(side)).floor() + 1.0;
      let down = (span.1 / f64::from(side)).floor() + 1.0;
      if across * down <= MAX_CELLS as f64 {
        break (across as usize, down as usize);
      }
      side *= 2.0;
    };
    let lattice = Lattice {
      origin: low,
      side,
      columns,
      rows,
    };
    (self.lattice, self.end) = (lattice, high);
    let boxed =
      colliders
        .iter()
        .enumerate()
        .filter_map(|(k, collider)| match Place::of(collider) {
          Place::Box(min, max) => Some((id(k), min, max)),
          _ => None,
        });
    self
      .colliders
      .fill(lattice, boxed, &mut self.everywhere_colliders);
    let Self {
      owners,
      edges: cells,
      everywhere_edges,
      ..
    } = self;
    let lines = edges
      .iter()
      .zip(owners.iter())
      .enumerate()
      .filter(|&(_, (_, &owner))| owner != NO_OWNER)
      .filter_map(|(e, (&edge, _))| edge_box(edge).map(|(min, max)| (id(e), min, max)));
    cells.fill(lattice, lines, everywhere_edges);
    self.everywhere_colliders.sort_unstable();
    self.everywhere_edges.sort_unstable();
  }

  /// Holds `colliders` and `edges` as every query's finds, as testing
  /// every collider and edge does
  #[cfg(test)]
  pub(crate) fn rebuild_whole(&mut self, colliders: &[ParticleCollider], edges: &[(Vec2, Vec2)]) {
    self.clear();
    self.own(colliders, edges.len());
    self.everywhere_colliders.extend(0..self.gathered.0);
    self.everywhere_edges.extend(0..self.gathered.1);
  }

  /// Counts the gathered colliders and `edges` edges, and notes each
  /// edge's collider and the largest reach
  fn own(&mut self, colliders: &[ParticleCollider], edges: usize) {
    self.gathered = (id(colliders.len()), id(edges));
    self.owners.resize(edges, NO_OWNER);
    for (k, collider) in colliders.iter().enumerate() {
      if let ColliderGeometry::Edges {
        start, end, solid, ..
      } = collider.geometry
      {
        if let Some(reach) = solid {
          self.reach = self.reach.max(reach);
        }
        let own = self
          .owners
          .get_mut(start as usize..(end as usize).min(edges));
        for owner in own.into_iter().flatten() {
          *owner = id(k);
        }
      }
    }
  }

  /// How many cells it has
  #[cfg(test)]
  pub(crate) fn cells(&self) -> usize {
    self.lattice.columns * self.lattice.rows
  }

  /// The colliders and edges in the cells the box `min` to `max` covers,
  /// with those every query finds, and each found edge's collider: every
  /// one whose box meets it (and some near it). A box that is not all
  /// numbers finds every collider and edge
  pub(crate) fn query(&self, min: Vec2, max: Vec2, out: &mut Candidates) {
    if !(min.is_finite() && max.is_finite()) {
      self.all(out);
      return;
    }
    out.colliders.clear();
    out.edges.clear();
    out.colliders.extend_from_slice(&self.everywhere_colliders);
    out.edges.extend_from_slice(&self.everywhere_edges);
    let lattice = self.lattice;
    let meets = lattice.columns > 0
      && min.cmple(max).all()
      && min.cmple(self.end).all()
      && max.cmpge(lattice.origin).all();
    if meets {
      for cell in lattice.cells(lattice.span(min, max)) {
        out.colliders.extend_from_slice(self.colliders.of(cell));
        out.edges.extend_from_slice(self.edges.of(cell));
      }
    }
    out.edges.sort_unstable();
    out.edges.dedup();
    // Each edge's collider, once: the edges come collider by collider
    let mut last = NO_OWNER;
    for &edge in &out.edges {
      let owner = self.owners.get(edge as usize).copied().unwrap_or(NO_OWNER);
      if owner != last && owner != NO_OWNER {
        out.colliders.push(owner);
      }
      last = owner;
    }
    out.colliders.sort_unstable();
    out.colliders.dedup();
  }

  /// What a particle of `radius` moving from `from` to `to` may meet: the
  /// query of its move's box grown by its radius or the largest reach
  /// (a tile part's half cell), whichever is more, and a little more. A
  /// move or radius that is not all numbers finds every collider and edge
  pub(crate) fn query_move(&self, from: Vec2, to: Vec2, radius: f32, out: &mut Candidates) {
    if !(from.is_finite() && to.is_finite() && radius.is_finite()) {
      self.all(out);
      return;
    }
    let size = from.abs().max(to.abs()).max_element();
    let grow = radius.abs().max(self.reach) + QUERY_SLACK + size * QUERY_SLACK_SHARE;
    self.query(from.min(to) - grow, from.max(to) + grow, out);
  }

  /// Every collider and edge
  fn all(&self, out: &mut Candidates) {
    out.colliders.clear();
    out.edges.clear();
    out.colliders.extend(0..self.gathered.0);
    out.edges.extend(0..self.gathered.1);
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::ecs::{Entity, Transform};
  use crate::core::particles::testing;
  use crate::core::physics::Collider;

  /// Whether a query of `min` to `max` finds nothing
  fn finds_nothing(grid: &ColliderGrid, min: Vec2, max: Vec2) -> bool {
    let mut near = Candidates {
      colliders: vec![7],
      edges: vec![3],
    };
    grid.query(min, max, &mut near);
    near.colliders.is_empty() && near.edges.is_empty()
  }

  #[test]
  fn an_empty_world_builds_an_empty_grid() {
    // A grid holding a floor from an earlier gather finds it
    let mut world = testing::world();
    world.spawn((Transform::from_xy(100.0, 60.0), Collider::rect(200.0, 20.0)));
    testing::step(&mut world);
    let (colliders, edges) = testing::gathered(&mut world);
    let mut grid = ColliderGrid::default();
    grid.rebuild(&colliders, &edges);
    let mut near = Candidates::default();
    grid.query(Vec2::new(90.0, 40.0), Vec2::new(110.0, 55.0), &mut near);
    assert_eq!(near.colliders, [0]);
    assert!(grid.cells() > 0);

    // Then an empty world's gather leaves it empty: no cells, and no query
    // finds anything, whatever its box
    let (colliders, edges) = testing::gathered(&mut testing::world());
    assert!(colliders.is_empty() && edges.is_empty());
    grid.rebuild(&colliders, &edges);
    assert_eq!(grid.cells(), 0);
    let boxes = [
      (Vec2::new(90.0, 40.0), Vec2::new(110.0, 55.0)),
      (Vec2::splat(-1e30), Vec2::splat(1e30)),
      (Vec2::NEG_INFINITY, Vec2::INFINITY),
      (Vec2::NAN, Vec2::NAN),
    ];
    for (min, max) in boxes {
      assert!(finds_nothing(&grid, min, max), "{min} to {max}");
    }
    near.colliders.push(0);
    grid.query_move(
      Vec2::new(100.0, 40.0),
      Vec2::new(100.0, 52.0),
      2.0,
      &mut near,
    );
    assert!(near.colliders.is_empty() && near.edges.is_empty());
  }

  #[test]
  fn a_collider_or_a_move_with_no_bounds_meets_everything() {
    // A circle at no number meets every particle, as without a grid; so
    // does every collider a particle at no number meets
    let mut world = testing::world();
    world.spawn((Transform::from_xy(100.0, 60.0), Collider::rect(200.0, 20.0)));
    world.spawn((Transform::from_xy(f32::NAN, 0.0), Collider::circle(8.0)));
    world.spawn((
      Transform::IDENTITY,
      Collider::segment(Vec2::new(0.0, 300.0), Vec2::new(40.0, 300.0)),
    ));
    testing::step(&mut world);
    let (colliders, edges) = testing::gathered(&mut world);
    assert_eq!((colliders.len(), edges.len()), (3, 1));
    let mut grid = ColliderGrid::default();
    grid.rebuild(&colliders, &edges);
    let mut near = Candidates::default();
    grid.query_move(
      Vec2::new(900.0, 900.0),
      Vec2::new(901.0, 900.0),
      2.0,
      &mut near,
    );
    assert_eq!(near.colliders, [1], "the circle at no number, alone");
    grid.query_move(
      Vec2::new(10.0, 300.0),
      Vec2::new(10.0, 302.0),
      2.0,
      &mut near,
    );
    assert_eq!(
      (near.colliders.as_slice(), near.edges.as_slice()),
      ([1, 2].as_slice(), [0].as_slice())
    );
    for (from, to, radius) in [
      (Vec2::new(10.0, 300.0), Vec2::NAN, 2.0),
      (Vec2::new(f32::INFINITY, 0.0), Vec2::ZERO, 2.0),
      (Vec2::ZERO, Vec2::ZERO, f32::NAN),
    ] {
      grid.query_move(from, to, radius, &mut near);
      assert_eq!(
        (near.colliders.as_slice(), near.edges.as_slice()),
        ([0, 1, 2].as_slice(), [0].as_slice()),
        "from {from} to {to} at radius {radius}"
      );
    }
  }

  #[test]
  fn a_corner_a_hair_past_the_reach_is_still_read() {
    // A tile part's corner where two edges almost meet: the particle is
    // just inside the reach of one edge's end, and the other's end is 6e-4
    // further, just past the reach and across a cell's side. A contact
    // adds the normals of both ends (they are within 1e-3), so the query
    // must find the far edge too, or the particle would leave the ground
    // by another normal
    let edges = vec![
      (Vec2::new(107.9998, 54.0), Vec2::new(107.9998, 50.0)),
      (Vec2::new(108.0004, 50.0), Vec2::new(112.0, 50.0)),
      // Far off, setting the cells' origin so a cell's side falls between
      // the two ends
      (Vec2::new(92.0004, 300.0), Vec2::new(96.0004, 300.0)),
    ];
    let part = ParticleCollider {
      entity: Entity::PLACEHOLDER,
      geometry: ColliderGeometry::Edges {
        start: 0,
        end: 3,
        min: Vec2::new(92.0004, 50.0),
        max: Vec2::new(112.0, 300.0),
        solid: Some(8.0),
      },
      layer: 1,
    };
    let mut grid = ColliderGrid::default();
    grid.rebuild(&[part], &edges);
    let at = Vec2::new(100.0, 50.0);
    let mut near = Candidates::default();
    grid.query_move(at, at, 2.0, &mut near);
    assert_eq!(near.edges, [0, 1]);
    let all = part.contact(&edges, at, (at.x, at.y), 2.0);
    let found = part.contact_among(&edges, &near.edges, at, (at.x, at.y), 2.0);
    let normal = |c: Option<crate::core::particles::contact::Contact>| {
      c.map(|c| (c.normal_x.to_bits(), c.normal_y.to_bits()))
    };
    assert!(all.is_some_and(|c| c.normal_x > 0.7 && c.normal_y > 0.7));
    assert_eq!(normal(found), normal(all));
  }
}
