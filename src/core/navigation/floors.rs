//! Navigation across floors: a grid for each floor over the same cells,
//! joined where stairs lead from one floor to another.

use super::grid::{GridCoord, NavigationGrid};
use super::path::PathNode;
use super::pathfinder::{Graph, distance, tie_break};
use crate::core::ecs::Floor;

/// Stairs joining two floors at one cell: a path steps from the cell on one
/// floor to the same cell on the other at no cost, since it is the same
/// ground. A tile map's stairs give one for each of their cells
/// ([`MapsView::floor_navigation`](crate::core::tilemap::MapsView::floor_navigation)).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StairLink {
  /// The cell, on both floors' grids
  pub cell: GridCoord,
  /// The floor the stairs lead up from
  pub lower: Floor,
  /// The floor they lead up to
  pub upper: Floor,
}

/// A navigation grid for each floor, over the same cells, joined at stair
/// cells ([`StairLink`]): what [`Pathfinder::find_floor_path`](super::Pathfinder::find_floor_path)
/// searches, so a path can climb from one floor to another.
///
/// [`MapsView::floor_navigation`](crate::core::tilemap::MapsView::floor_navigation)
/// takes one from a tile map with floors. A game can make one without a
/// map ([`new`](Self::new)), from grids it puts on their floors
/// ([`NavigationGrid::on_floor`]), for a dungeon of levels joined by
/// ladders, say.
///
/// ```
/// use ryo2d::prelude::*;
///
/// /// Two levels of 20 x 15 cells of 16 px, the upper open only over its
/// /// gallery, joined by a ladder at (4, 7)
/// fn two_levels() -> FloorNavigation {
///   let ground = NavigationGrid::new(320.0, 240.0, 16.0);
///   let mut gallery = NavigationGrid::new(320.0, 240.0, 16.0).on_floor(Floor::On(1));
///   for y in 0..15 {
///     for x in 0..20 {
///       if !(4..12).contains(&x) || !(5..9).contains(&y) {
///         gallery.set(GridCoord::new(x, y), CellState::Blocked);
///       }
///     }
///   }
///   let ladder = StairLink {
///     cell: GridCoord::new(4, 7),
///     lower: Floor::GROUND,
///     upper: Floor::On(1),
///   };
///   FloorNavigation::new(vec![ground, gallery], vec![ladder])
/// }
/// ```
#[derive(Clone, Debug)]
pub struct FloorNavigation {
  /// One grid for each floor, by floor
  grids: Vec<NavigationGrid>,
  /// The links between its floors, by row
  links: Vec<StairLink>,
  /// Each link both ways, by the grids' places: its cell, the grid it
  /// leaves and the grid it enters, by row, then the grid it leaves
  ways: Vec<(GridCoord, u8, u8)>,
}

impl FloorNavigation {
  /// The floors of `grids`, joined at `links`. The grids are kept by their
  /// floor ([`NavigationGrid::floor`]): the first for each floor number; a
  /// grid on `Floor::Every` is left out. A link between floors it has no
  /// grid for, or from a floor to itself, is left out.
  ///
  /// The grids must cover the same cells, as those `floor_navigation`
  /// takes from a map do: the search measures every floor's cells on the
  /// lowest floor's grid, so it finds the cheapest path only when the
  /// floors' cells lie where the lowest floor's do.
  pub fn new(grids: Vec<NavigationGrid>, links: Vec<StairLink>) -> FloorNavigation {
    let mut grids: Vec<NavigationGrid> = grids
      .into_iter()
      .filter(|grid| grid.floor().number().is_some())
      .collect();
    // Stable, so the first of a floor's grids leads its run
    grids.sort_by_key(|grid| grid.floor().number());
    grids.dedup_by_key(|grid| grid.floor());
    let place = |floor: Floor| {
      grids
        .iter()
        .position(|grid| grid.floor() == floor)
        .and_then(|at| u8::try_from(at).ok())
    };
    let mut links: Vec<StairLink> = links
      .into_iter()
      .filter(|link| {
        link.lower != link.upper && place(link.lower).is_some() && place(link.upper).is_some()
      })
      .collect();
    links.sort_by_key(|link| {
      (
        link.cell.y,
        link.cell.x,
        link.lower.number(),
        link.upper.number(),
      )
    });
    links.dedup();
    let mut ways = Vec::with_capacity(links.len() * 2);
    for link in &links {
      if let (Some(lower), Some(upper)) = (place(link.lower), place(link.upper)) {
        ways.push((link.cell, lower, upper));
        ways.push((link.cell, upper, lower));
      }
    }
    ways.sort_by_key(|&(cell, from, to)| (cell.y, cell.x, from, to));
    ways.dedup();
    FloorNavigation { grids, links, ways }
  }

  /// The grid of `floor`, if it has one
  pub fn grid(&self, floor: Floor) -> Option<&NavigationGrid> {
    self.grids.iter().find(|grid| grid.floor() == floor)
  }

  /// Its grids, one for each floor, the lowest first
  pub fn grids(&self) -> impl Iterator<Item = &NavigationGrid> {
    self.grids.iter()
  }

  /// The links between its floors, by row
  pub fn links(&self) -> &[StairLink] {
    &self.links
  }

  /// The search's node for `floor`'s `cell`, if it has that floor
  pub(super) fn key(&self, (floor, cell): (Floor, GridCoord)) -> Option<FloorCell> {
    let at = self.grids.iter().position(|grid| grid.floor() == floor)?;
    Some(FloorCell {
      grid: u8::try_from(at).ok()?,
      cell,
    })
  }

  /// The grid `at`'s floor has, which every key it made names
  fn grid_at(&self, at: FloorCell) -> Option<&NavigationGrid> {
    self.grids.get(usize::from(at.grid))
  }

  /// The ways out of `at` by its links: the places of the grids they enter
  fn ways_from(&self, at: FloorCell) -> impl Iterator<Item = u8> + '_ {
    let key = (at.cell.y, at.cell.x, at.grid);
    let first = self
      .ways
      .partition_point(|&(cell, from, _)| (cell.y, cell.x, from) < key);
    self.ways[first..]
      .iter()
      .take_while(move |&&(cell, from, _)| (cell.y, cell.x, from) == key)
      .map(|&(.., to)| to)
  }
}

/// A node of a search across floors: a cell of one floor's grid, by the
/// grid's place
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) struct FloorCell {
  grid: u8,
  cell: GridCoord,
}

impl Graph for FloorNavigation {
  type Key = FloorCell;

  fn neighbors(&self, at: FloorCell) -> Vec<(FloorCell, f32)> {
    let Some(grid) = self.grid_at(at) else {
      return Vec::new();
    };
    let mut out: Vec<(FloorCell, f32)> = grid
      .neighbors(at.cell)
      .into_iter()
      .map(|(cell, cost)| (FloorCell { cell, ..at }, cost))
      .collect();
    // The same ground on the floor a link leads to, at no cost
    for to in self.ways_from(at) {
      let onto = FloorCell { grid: to, ..at };
      if self.grid_at(onto).is_some_and(|g| g.is_walkable(at.cell)) {
        out.push((onto, 0.0));
      }
    }
    out
  }

  fn distance(&self, from: FloorCell, to: FloorCell) -> f32 {
    self
      .grids
      .first()
      .map_or(0.0, |lowest| distance(lowest, from.cell, to.cell))
  }

  fn tie_break(&self, at: FloorCell, start: FloorCell, goal: FloorCell) -> f32 {
    self.grids.first().map_or(0.0, |lowest| {
      tie_break(lowest, at.cell, start.cell, goal.cell)
    })
  }

  fn node(&self, at: FloorCell) -> PathNode {
    match self.grid_at(at) {
      Some(grid) => Graph::node(grid, at.cell),
      // Not met: its keys name its grids
      None => PathNode::new(f32::NAN, f32::NAN, at.cell),
    }
  }

  fn generation(&self) -> u64 {
    self
      .grids
      .iter()
      .fold(0, |sum, grid| sum.wrapping_add(grid.generation()))
  }

  fn grid_of(&self, floor: Floor) -> Option<&NavigationGrid> {
    self.grid(floor)
  }
}
