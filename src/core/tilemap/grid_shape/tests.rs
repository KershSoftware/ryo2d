//! `MapGrid` against what Tiled measured (`measured.json`, written by
//! `make-fixtures.js`), and its picking, neighbours, board tools, bands and
//! chunk ranges on every grid. Pure: no world

use std::collections::{HashMap, HashSet, VecDeque};

use glam::{IVec2, UVec2, Vec2};
use serde_json::Value;

use super::{GridShape, MapGrid, StaggerAxis, StaggerIndex};
use crate::core::physics::{CellRect, HexCells};
use crate::core::tilemap::grid::{CHUNK_SIZE, TileGrid};
use crate::core::tilemap::test_util::FIXTURES;
use crate::core::ui::Rect;

use StaggerAxis::{X, Y};
use StaggerIndex::{Even, Odd};

/// A small xorshift, so the random runs are the same on every machine
struct Rng(u64);

impl Rng {
  fn next(&mut self) -> u64 {
    let mut x = self.0;
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    self.0 = x;
    x
  }

  fn int(&mut self, lo: i32, hi: i32) -> i32 {
    let span = (i64::from(hi) - i64::from(lo) + 1) as u64;
    (i64::from(lo) + (self.next() % span) as i64) as i32
  }

  fn float(&mut self, lo: f32, hi: f32) -> f32 {
    lo + (self.next() >> 40) as f32 / (1u64 << 24) as f32 * (hi - lo)
  }

  fn chance(&mut self, percent: u64) -> bool {
    self.next() % 100 < percent
  }
}

fn stag(axis: StaggerAxis, index: StaggerIndex) -> GridShape {
  GridShape::Staggered { axis, index }
}

fn hex(axis: StaggerAxis, index: StaggerIndex, side: u32) -> GridShape {
  GridShape::Hexagonal { axis, index, side }
}

fn oblique(x: i32, y: i32) -> GridShape {
  GridShape::Oblique {
    skew: IVec2::new(x, y),
  }
}

fn grid(shape: GridShape, width: u32, height: u32) -> MapGrid {
  MapGrid::new(shape, UVec2::new(width, height))
}

fn is_hex(grid: &MapGrid) -> bool {
  matches!(grid.shape, GridShape::Hexagonal { .. })
}

/// A hexagonal grid whose side is 0: its hexagons are diamonds, and the
/// cells beside each other along its rows or columns meet at a point
fn side_zero(grid: &MapGrid) -> bool {
  matches!(grid.shape, GridShape::Hexagonal { side: 0, .. })
}

/// Every grid the tests walk: each shape and stagger, odd sizes (half
/// cells on half pixels), side 0, negative skews, and origins off zero
fn grids() -> Vec<MapGrid> {
  let mut out = vec![
    grid(GridShape::Orthogonal, 32, 32),
    grid(GridShape::Orthogonal, 33, 17),
    grid(GridShape::Isometric, 64, 32),
    grid(GridShape::Isometric, 48, 32),
    grid(GridShape::Isometric, 33, 17),
    grid(oblique(8, 12), 32, 32),
    grid(oblique(-10, 0), 32, 32),
    grid(oblique(0, -12), 32, 32),
    grid(oblique(7, -5), 33, 17),
  ];
  for axis in [X, Y] {
    for index in [Odd, Even] {
      out.push(grid(stag(axis, index), 64, 32));
      out.push(grid(stag(axis, index), 33, 17));
      out.push(grid(hex(axis, index, 12), 32, 32));
      out.push(grid(hex(axis, index, 0), 32, 32));
      out.push(grid(hex(axis, index, 13), 33, 17));
    }
  }
  // Sides longer than the tile: the rows spread apart, and the hexagons'
  // slants are gone
  out.extend([
    grid(hex(Y, Odd, 40), 32, 32),
    grid(hex(X, Even, 41), 32, 32),
  ]);
  let away = Vec2::new(40.5, -24.0);
  out.extend([
    grid(GridShape::Orthogonal, 32, 32).with_origin(away),
    MapGrid::for_map(
      GridShape::Isometric,
      UVec2::new(33, 17),
      UVec2::new(5, 3),
      false,
    ),
    grid(oblique(8, 12), 32, 32).with_origin(away),
    grid(stag(Y, Odd), 64, 32).with_origin(away),
    grid(hex(X, Even, 12), 32, 32).with_origin(away),
  ]);
  out
}

fn cells_around(center: IVec2, reach: i32) -> impl Iterator<Item = IVec2> {
  (-reach..=reach).flat_map(move |y| (-reach..=reach).map(move |x| center + IVec2::new(x, y)))
}

// ---- measured.json ----------------------------------------------------------

/// One of the sample grids Tiled measured
struct Sample {
  name: String,
  grid: MapGrid,
  size: Option<UVec2>,
  /// `tileToScreen` of the cells -3..3 a side
  screen: HashMap<IVec2, Vec2>,
  /// Where `toImage()` drew a tile image the cell's size: its bottom-left,
  /// in the render's pixels
  drawn: Vec<(IVec2, Vec2)>,
  /// What `screenToTile` picked at points
  picks: Vec<(Vec2, IVec2)>,
}

struct Measured {
  samples: Vec<Sample>,
  /// Skew, render order, and the tile drawn over the other: of the tall
  /// pair at (3, 4) and (3, 3), then of the wide pair at (4, 3) and (3, 3)
  oblique_order: Vec<(IVec2, String, [IVec2; 2])>,
}

fn int(v: &Value) -> i32 {
  v.as_i64().expect("an integer") as i32
}

fn num(v: &Value) -> f32 {
  v.as_f64().expect("a number") as f32
}

fn pair(v: &Value) -> IVec2 {
  IVec2::new(int(&v[0]), int(&v[1]))
}

fn measured() -> Measured {
  let path = format!("{FIXTURES}/tiled/measured.json");
  let text = std::fs::read_to_string(&path).expect("measured.json");
  let json: Value = serde_json::from_str(&text).expect("measured.json is JSON");
  assert_eq!(json["tiled"], "1.12.2");
  let rows = |g: &Value, key: &str| {
    g.get(key)
      .and_then(Value::as_array)
      .cloned()
      .unwrap_or_default()
  };
  let samples = json["grids"]
    .as_array()
    .expect("grids")
    .iter()
    .map(|g| {
      let axis = if g["staggeraxis"] == "x" { X } else { Y };
      let index = if g["staggerindex"] == "odd" {
        Odd
      } else {
        Even
      };
      let shape = match g["orientation"].as_str().expect("orientation") {
        "orthogonal" => GridShape::Orthogonal,
        "isometric" => GridShape::Isometric,
        "staggered" => stag(axis, index),
        "hexagonal" => hex(axis, index, int(&g["hexsidelength"]) as u32),
        "oblique" => GridShape::Oblique {
          skew: pair(&g["skew"]),
        },
        other => panic!("orientation {other}"),
      };
      let size = g.get("size").map(|s| pair(s).as_uvec2());
      let at = |r: &Value| (pair(r), Vec2::new(num(&r[2]), num(&r[3])));
      Sample {
        name: g["name"].as_str().expect("name").to_string(),
        grid: MapGrid::for_map(
          shape,
          pair(&g["tile"]).as_uvec2(),
          size.unwrap_or(UVec2::ZERO),
          size.is_none(),
        ),
        size,
        screen: rows(g, "screen").iter().map(at).collect(),
        drawn: rows(g, "drawn").iter().map(at).collect(),
        picks: rows(g, "picks")
          .iter()
          .map(|r| {
            (
              Vec2::new(num(&r[0]), num(&r[1])),
              IVec2::new(int(&r[2]), int(&r[3])),
            )
          })
          .collect(),
      }
    })
    .collect();
  let oblique_order = rows(&json, "oblique_order")
    .iter()
    .map(|o| {
      let over = &o["over"];
      (
        pair(&o["skew"]),
        o["renderorder"].as_str().expect("renderorder").to_string(),
        [pair(&over[0]), pair(&over[1])],
      )
    })
    .collect();
  Measured {
    samples,
    oblique_order,
  }
}

/// The odd sizes (half cells on half pixels), hexagons of side 0 or longer
/// than the tile, and negative skews Tiled saves
fn odd_or_degenerate(sample: &Sample) -> bool {
  let cell = sample.grid.cell;
  cell.x % 2 == 1
    || cell.y % 2 == 1
    || matches!(sample.grid.shape, GridShape::Hexagonal { side, .. } if side == 0 || side > cell.max_element())
    || matches!(sample.grid.shape, GridShape::Oblique { skew } if skew.cmplt(IVec2::ZERO).any())
}

/// Where Tiled's renderer puts a tile image whose bottom-left is `anchor`:
/// there, except on isometric maps of odd sizes, whose half pixels it
/// rounds up, and whose odd tile heights put a tile a pixel lower where its
/// bottom is a whole pixel (measured; the formats note has it)
fn rastered(grid: &MapGrid, anchor: Vec2) -> Vec2 {
  if grid.shape != GridShape::Isometric {
    assert_eq!(
      anchor,
      anchor.round(),
      "{grid:?} places tiles on whole pixels"
    );
    return anchor;
  }
  let y = if grid.cell.y % 2 == 1 {
    anchor.y.floor() + 1.0
  } else {
    anchor.y
  };
  Vec2::new(anchor.x.ceil(), y)
}

/// Checks a sample grid against Tiled: the corner `tileToScreen` names of
/// every cell, where each cell's tile stands, and the renders
fn check_placement(sample: &Sample) {
  let (grid, name) = (sample.grid, &sample.name);
  let cell = grid.cell.as_vec2();
  assert_eq!(sample.screen.len(), 49, "{name}");
  for (&c, &at) in &sample.screen {
    match grid.shape {
      // Tiled names the diamond's top vertex; a tile stands on the
      // bottom-left of the diamond's box
      GridShape::Isometric => {
        assert_eq!(grid.cell_outline(c).points()[0], at, "{name} {c}");
        assert_eq!(
          grid.tile_anchor(c),
          at + Vec2::new(-cell.x / 2.0, cell.y),
          "{name} {c}"
        );
      }
      // Tiled names the cell's skewed top-left corner, which is where the
      // tile of the cell above it stands
      GridShape::Oblique { .. } => {
        assert!(grid.cell_outline(c).points().contains(&at), "{name} {c}");
        assert_eq!(grid.tile_anchor(c - IVec2::Y), at, "{name} {c}");
      }
      // The cell's box, its stagger in
      _ => {
        let bounds = grid.cell_bounds(c);
        assert_eq!(Vec2::new(bounds.x, bounds.y), at, "{name} {c}");
      }
    }
  }
  if !matches!(grid.shape, GridShape::Isometric | GridShape::Oblique { .. }) {
    // A tile stands on the box's bottom, as high above it as Tiled draws
    // it: the cell's height, or on odd sizes the even height Tiled steps by
    let (c, d) = sample.drawn[0];
    let height = d.y - sample.screen[&c].y;
    for (&c, &at) in &sample.screen {
      assert_eq!(
        grid.tile_anchor(c),
        at + Vec2::new(0.0, height),
        "{name} {c}"
      );
    }
  }
  // The renders: each tile where it stands, moved by the render's offset
  // (an oblique map skewed to negative x or y is drawn from its leftmost and
  // top-most point) and rounded as Tiled's renderer rounds
  let offset = match (grid.shape, sample.size) {
    (GridShape::Oblique { skew }, Some(size)) => Vec2::new(
      (-skew.x * size.y as i32).max(0) as f32,
      (-skew.y * size.x as i32).max(0) as f32,
    ),
    _ => Vec2::ZERO,
  };
  assert_eq!(sample.drawn.is_empty(), sample.size.is_none(), "{name}");
  for &(c, d) in &sample.drawn {
    assert_eq!(
      d,
      rastered(&grid, grid.tile_anchor(c)) + offset,
      "{name} {c}"
    );
  }
  // Pixel art draws a tile the cell's size where Tiled's raster does
  for &c in sample.screen.keys() {
    let anchor = grid.tile_anchor(c);
    assert_eq!(
      anchor + grid.raster_offset(anchor, grid.cell),
      rastered(&grid, anchor),
      "{name} {c}"
    );
  }
}

#[test]
fn pixel_art_draws_tiles_where_tileds_raster_does_on_odd_isometric_grids() {
  // Tiled's renders of one tile alone on isometric maps of odd cells
  // (`measured-raster.json`): atlas tiles and image collections read from
  // a file, of tilesets whose tile size is below, at and past the cell's
  // width, across and down, at whole and half anchors. Where the anchor is
  // and where the render puts the tile's bottom-left
  let path = format!("{FIXTURES}/tiled/measured-raster.json");
  let text = std::fs::read_to_string(&path).expect("measured-raster.json");
  let json: Value = serde_json::from_str(&text).expect("measured-raster.json is JSON");
  assert_eq!(json["tiled"], "1.12.2");
  let rows = json["rows"].as_array().expect("rows");
  assert_eq!(rows.len(), 108);
  let mut late = 0;
  for row in rows {
    let v = |i: usize| num(&row[i]);
    let cell = UVec2::new(v(0) as u32, v(1) as u32);
    let tileset = UVec2::new(v(3) as u32, v(4) as u32);
    let at = IVec2::new(v(7) as i32, v(8) as i32);
    let grid = MapGrid::for_map(GridShape::Isometric, cell, UVec2::splat(12), false);
    let anchor = grid.tile_anchor(at);
    assert_eq!(anchor, Vec2::new(v(9), v(10)), "{row}");
    let drawn = anchor + grid.raster_offset(anchor, tileset);
    assert_eq!(drawn, Vec2::new(v(11), v(12)), "{row}");
    late += usize::from(drawn.x == anchor.x + 1.0);
  }
  // Tiles of tilesets past the cell's width, at a whole x on odd widths:
  // three atlases and two collections a grid, on five grids of odd widths
  assert_eq!(late, 25);
}

#[test]
fn tile_anchors_match_tileds_measurements() {
  let measured = measured();
  let checked = measured
    .samples
    .iter()
    .filter(|s| !odd_or_degenerate(s))
    .inspect(|s| check_placement(s))
    .count();
  assert_eq!(checked, 13);
  // The formats note's examples
  let iso = MapGrid::for_map(
    GridShape::Isometric,
    UVec2::new(64, 32),
    UVec2::new(4, 4),
    false,
  );
  assert_eq!(iso.origin, Vec2::new(128.0, 0.0));
  assert_eq!(iso.tile_anchor(IVec2::new(1, 2)), Vec2::new(64.0, 80.0));
  let stag_x = grid(stag(X, Odd), 64, 32).cell_bounds(IVec2::new(1, 2));
  assert_eq!((stag_x.x, stag_x.y), (32.0, 80.0));
  let hex_y = grid(hex(Y, Odd, 12), 32, 32).cell_bounds(IVec2::new(2, 1));
  assert_eq!((hex_y.x, hex_y.y), (80.0, 22.0));
  let skewed = grid(oblique(16, 0), 32, 32);
  assert_eq!(skewed.tile_anchor(IVec2::new(1, 2)), Vec2::new(80.0, 96.0));
  // An infinite isometric map's origin is cell (0, 0)'s top vertex, at 0
  let endless = MapGrid::for_map(GridShape::Isometric, UVec2::new(64, 32), UVec2::ZERO, true);
  assert_eq!(endless.origin, Vec2::ZERO);
  assert_eq!(
    endless.cell_outline(IVec2::new(-2, -1)).points()[0],
    Vec2::new(-32.0, -48.0)
  );
}

#[test]
fn odd_and_degenerate_grids_place_as_tiled_measures() {
  let measured = measured();
  let checked = measured
    .samples
    .iter()
    .filter(|s| odd_or_degenerate(s))
    .inspect(|s| check_placement(s))
    .count();
  assert_eq!(checked, 18);
  // A bounded isometric map's origin rounds down, as Tiled's does: 3 rows of
  // 33-pixel cells put cell (0, 0)'s top vertex at 49, not 49.5
  let odd = MapGrid::for_map(
    GridShape::Isometric,
    UVec2::new(33, 17),
    UVec2::new(5, 3),
    false,
  );
  assert_eq!(odd.origin, Vec2::new(49.0, 0.0));
  // Staggered and hexagonal grids of odd sizes step by whole pixels, half a
  // cell rounded down: 33 x 17 staggered cells are 32 x 16 diamonds
  let stag_y = grid(stag(Y, Even), 33, 17);
  assert_eq!(
    stag_y.cell_bounds(IVec2::new(1, 1)),
    Rect::new(32.0, 8.0, 32.0, 16.0)
  );
  assert_eq!(stag_y.tile_anchor(IVec2::new(1, 1)), Vec2::new(32.0, 24.0));
  // A hexagonal grid of side 0 is the staggered grid of its stagger, but
  // keeps the six neighbours of a hexagon
  for axis in [X, Y] {
    for index in [Odd, Even] {
      for size in [UVec2::new(32, 32), UVec2::new(33, 17)] {
        let (diamonds, hexagons) = (
          MapGrid::new(stag(axis, index), size),
          MapGrid::new(hex(axis, index, 0), size),
        );
        for c in cells_around(IVec2::ZERO, 6) {
          assert_eq!(hexagons.tile_anchor(c), diamonds.tile_anchor(c));
          assert_eq!(hexagons.cell_center(c), diamonds.cell_center(c));
          assert_eq!(hexagons.cell_bounds(c), diamonds.cell_bounds(c));
          let mut points = hexagons.cell_outline(c).points().to_vec();
          points.dedup();
          assert_eq!(points, diamonds.cell_outline(c).points());
          let four: HashSet<IVec2> = diamonds.neighbours(c).collect();
          let six: HashSet<IVec2> = hexagons.neighbours(c).collect();
          assert!(
            four.is_subset(&six) && six.len() == 6,
            "{axis:?} {index:?} {c}"
          );
          let p = diamonds.cell_center(c) + Vec2::new(5.25, -3.5);
          assert_eq!(hexagons.cell_at(p), diamonds.cell_at(p));
        }
      }
    }
  }
}

// ---- orthogonal -------------------------------------------------------------

#[test]
fn an_orthogonal_grid_is_6a_s_rectangles() {
  let mut rng = Rng(0x0b_5e55);
  for size in [UVec2::new(16, 16), UVec2::new(33, 17), UVec2::new(24, 40)] {
    let grid = MapGrid::new(GridShape::Orthogonal, size);
    let s = size.as_vec2();
    assert_eq!(
      MapGrid::for_map(GridShape::Orthogonal, size, UVec2::new(9, 7), false),
      grid
    );
    let lattice = grid.lattice().expect("a square lattice");
    assert_eq!(lattice.to_pixel, glam::Affine2::from_scale(s));
    for i in 0..4000 {
      let reach = if i % 2 == 0 { 50 } else { 3_000_000 };
      let c = IVec2::new(rng.int(-reach, reach), rng.int(-reach, reach));
      // 6a's maths, to the bit
      assert_eq!(grid.cell_center(c), (c.as_vec2() + 0.5) * s, "{c}");
      assert_eq!(
        grid.tile_anchor(c),
        Vec2::new(c.x as f32 * s.x, (c.y as f32 + 1.0) * s.y),
        "{c}"
      );
      assert_eq!(
        grid.cell_bounds(c),
        Rect::new(c.x as f32 * s.x, c.y as f32 * s.y, s.x, s.y),
        "{c}"
      );
      assert_eq!(grid.sort_band(c), c.y);
      assert_eq!(
        grid.band_y(grid.sort_band(c)),
        (c.y as f32 + 1.0) * s.y,
        "{c}"
      );
      assert_eq!(grid.paint_key(c), (c.y, 0, c.x));
      assert_eq!(lattice.lattice_cell(c), c);
      assert_eq!(lattice.grid_cell(c), c);
      let p = Vec2::new(rng.float(-1e6, 1e6), rng.float(-1e6, 1e6));
      assert_eq!(grid.cell_at(p), (p / s).floor().as_ivec2(), "{p}");
      if i < 100 {
        let around: Vec<IVec2> = grid.neighbours(c).collect();
        assert_eq!(
          around,
          [c - IVec2::Y, c + IVec2::X, c + IVec2::Y, c - IVec2::X]
        );
        let other = c + IVec2::new(rng.int(-9, 9), rng.int(-9, 9));
        let d = (other - c).abs();
        assert_eq!(grid.distance(c, other), (d.x + d.y) as u32);
      }
    }
  }
}

// ---- picking ----------------------------------------------------------------

fn cross(a: Vec2, b: Vec2, p: Vec2) -> f64 {
  let (e, v) = ((b - a).as_dvec2(), (p - a).as_dvec2());
  e.x * v.y - e.y * v.x
}

/// The outline's edges that have a length, as (from, to)
fn edges(outline: &[Vec2]) -> impl Iterator<Item = (Vec2, Vec2)> + '_ {
  (0..outline.len())
    .map(|i| (outline[i], outline[(i + 1) % outline.len()]))
    .filter(|(a, b)| a != b)
}

/// Whether `p` is inside the outline (clockwise on screen) and not on it
fn strictly_inside(outline: &[Vec2], p: Vec2) -> bool {
  edges(outline).all(|(a, b)| cross(a, b, p) > 0.0)
}

fn inside_or_on(outline: &[Vec2], p: Vec2) -> bool {
  edges(outline).all(|(a, b)| cross(a, b, p) >= 0.0)
}

/// The edge two outlines share, as `a`'s from-index and `b`'s: `a`'s edge i
/// to i + 1 is `b`'s j + 1 to j (both run clockwise)
fn shared_edge(a: &[Vec2], b: &[Vec2]) -> Option<(usize, usize)> {
  let (n, m) = (a.len(), b.len());
  (0..n).find_map(|i| {
    (0..m)
      .find(|&j| a[i] == b[(j + 1) % m] && a[(i + 1) % n] == b[j])
      .map(|j| (i, j))
  })
}

#[test]
fn cell_at_round_trips_and_splits_shared_edges_one_way() {
  for grid in grids() {
    for c in cells_around(IVec2::ZERO, 40) {
      assert_eq!(grid.cell_at(grid.cell_center(c)), c, "{grid:?} {c}");
    }
    for a in cells_around(IVec2::ZERO, 6) {
      let outline = grid.cell_outline(a);
      let points = outline.points();
      let n = points.len();
      // What draws there: every point inside the outline picks it
      let centre = grid.cell_center(a);
      for i in 0..n {
        for p in [points[i], (points[i] + points[(i + 1) % n]) * 0.5] {
          let inner = centre + (p - centre) * 0.9;
          assert!(strictly_inside(points, inner), "{grid:?} {a} {inner}");
          assert_eq!(grid.cell_at(inner), a, "{grid:?} {a} {inner}");
        }
      }
      // A shared edge goes to one of its cells, the whole edge to the same
      // one, from whichever cell's outline the point is taken
      for b in grid.neighbours(a) {
        let other = grid.cell_outline(b);
        let (i, j) = shared_edge(points, other.points()).expect("neighbours share an edge");
        let (p0, p1) = (points[i], points[(i + 1) % n]);
        if p0 == p1 {
          continue;
        }
        let m = other.points().len();
        let (q1, q0) = (other.points()[j], other.points()[(j + 1) % m]);
        let along =
          |from: Vec2, to: Vec2| [1.0, 2.0, 3.0].map(|k| (from * (4.0 - k) + to * k) / 4.0);
        let (on_a, on_b) = (along(p0, p1), along(q0, q1));
        assert_eq!(on_a, on_b, "{grid:?} {a} {b}");
        let picked = on_a.map(|p| grid.cell_at(p));
        assert!(
          picked[0] == a || picked[0] == b,
          "{grid:?} {a} {b} {picked:?}"
        );
        assert!(
          picked.iter().all(|&p| p == picked[0]),
          "{grid:?} {a} {b} {picked:?}"
        );
      }
      // A corner goes to one of the cells meeting there
      for &v in points {
        let meeting: HashSet<IVec2> = cells_around(a, 2)
          .filter(|&c| grid.cell_outline(c).points().contains(&v))
          .collect();
        assert!(
          meeting.contains(&a) && meeting.len() >= 3,
          "{grid:?} {a} {v} {meeting:?}"
        );
        assert!(
          meeting.contains(&grid.cell_at(v)),
          "{grid:?} {a} {v} {meeting:?}"
        );
      }
    }
  }
  // Tiled picks a hexagon by the nearest centre, which on hexagons of other
  // than the regular side hands a sliver by each vertex to a neighbour; the
  // engine picks the outline the point is in
  let measured = measured();
  let board = measured
    .samples
    .iter()
    .find(|s| !s.picks.is_empty())
    .expect("Tiled's picks");
  assert_eq!(board.grid, grid(hex(Y, Odd, 12), 32, 32));
  for &(p, tiled) in &board.picks {
    let picked = board.grid.cell_at(p);
    assert!(
      inside_or_on(board.grid.cell_outline(picked).points(), p),
      "{p}"
    );
    if picked != tiled {
      assert!(
        !strictly_inside(board.grid.cell_outline(tiled).points(), p),
        "{p}"
      );
    }
  }
  let sliver = Vec2::new(31.0, 22.5);
  assert!(board.picks.contains(&(sliver, IVec2::new(0, 1))));
  assert_eq!(board.grid.cell_at(sliver), IVec2::ZERO);
  // Which cell an edge goes to: on a hexagonal or staggered grid, a slanted
  // side to the later row (column on axis X), any other side to the later
  // cell along its row; elsewhere, the later cell along the axis crossed
  let pick = |shape: GridShape, x: f32, y: f32| grid(shape, 32, 32).cell_at(Vec2::new(x, y));
  assert_eq!(pick(hex(Y, Odd, 12), 24.0, 27.0), IVec2::new(0, 1));
  assert_eq!(pick(hex(Y, Odd, 12), 32.0, 16.0), IVec2::new(1, 0));
  assert_eq!(pick(hex(X, Odd, 12), 27.0, 24.0), IVec2::new(1, 0));
  assert_eq!(pick(hex(X, Odd, 12), 16.0, 32.0), IVec2::new(0, 1));
  assert_eq!(pick(stag(Y, Odd), 8.0, 24.0), IVec2::new(-1, 1));
  assert_eq!(pick(GridShape::Isometric, 8.0, 8.0), IVec2::new(0, 0));
  assert_eq!(pick(GridShape::Isometric, 8.0, 24.0), IVec2::new(1, 0));
  assert_eq!(pick(GridShape::Isometric, -8.0, 24.0), IVec2::new(0, 1));
  assert_eq!(pick(oblique(8, 12), 36.0, 28.0), IVec2::new(1, 0));
}

// ---- neighbours and distance ------------------------------------------------

/// Clockwise from straight up on screen, in degrees
fn bearing(d: Vec2) -> f64 {
  f64::from(d.x)
    .atan2(-f64::from(d.y))
    .to_degrees()
    .rem_euclid(360.0)
}

/// Whether the bearings turn clockwise, each past the one before, less than
/// a whole turn in all
fn clockwise(bearings: &[f64]) -> bool {
  let turned: Vec<f64> = bearings
    .iter()
    .map(|b| (b - bearings[0]).rem_euclid(360.0))
    .collect();
  turned.windows(2).all(|w| w[0] < w[1])
}

/// Steps from `a` to `b` through `neighbours`, found by a search
fn steps(grid: &MapGrid, a: IVec2, b: IVec2) -> u32 {
  let mut seen = HashSet::from([a]);
  let mut queue = VecDeque::from([(a, 0)]);
  while let Some((at, n)) = queue.pop_front() {
    if at == b {
      return n;
    }
    for next in grid.neighbours(at) {
      if seen.insert(next) {
        queue.push_back((next, n + 1));
      }
    }
  }
  unreachable!("every cell is reachable")
}

#[test]
fn neighbours_share_an_edge_and_distance_counts_steps() {
  let mut rng = Rng(0xd15_7a9ce);
  for grid in grids() {
    let count = if is_hex(&grid) { 6 } else { 4 };
    for c in cells_around(IVec2::ZERO, 5) {
      assert_eq!(grid.neighbours(c).len(), count, "{grid:?}");
      let around: Vec<IVec2> = grid.neighbours(c).collect();
      assert_eq!(
        around.iter().collect::<HashSet<_>>().len(),
        count,
        "{grid:?} {c}"
      );
      let outline = grid.cell_outline(c);
      for &n in &around {
        let other = grid.cell_outline(n);
        assert!(
          shared_edge(outline.points(), other.points()).is_some(),
          "{grid:?} {c} {n}"
        );
        if !side_zero(&grid) {
          let shared: HashSet<_> = outline
            .points()
            .iter()
            .filter(|p| other.points().contains(p))
            .map(|p| p.to_array().map(f32::to_bits))
            .collect();
          assert_eq!(shared.len(), 2, "{grid:?} {c} {n}");
        }
        assert_eq!(grid.distance(c, n), 1);
        assert_eq!(grid.distance(n, c), 1);
      }
      // And every cell sharing an edge is one of them
      for other in cells_around(c, 2).filter(|&o| o != c) {
        if shared_edge(outline.points(), grid.cell_outline(other).points()).is_some() {
          assert!(around.contains(&other), "{grid:?} {c} {other}");
        }
      }
      // Clockwise from the top: the cell across the top edge, or where the
      // top is a vertex the edge right of it; oblique grids keep the order
      // of the grid unskewed
      let bearings: Vec<f64> = around
        .iter()
        .map(|&n| bearing(grid.cell_center(n) - grid.cell_center(c)))
        .collect();
      assert!(clockwise(&bearings), "{grid:?} {c} {bearings:?}");
      if let GridShape::Oblique { .. } = grid.shape {
        assert_eq!(
          around,
          [c - IVec2::Y, c + IVec2::X, c + IVec2::Y, c - IVec2::X]
        );
      } else {
        assert!(bearings[0] < 90.0, "{grid:?} {c} {bearings:?}");
      }
    }
    for _ in 0..200 {
      let a = IVec2::new(rng.int(-8, 8), rng.int(-8, 8));
      let b = a + IVec2::new(rng.int(-6, 6), rng.int(-6, 6));
      assert_eq!(grid.distance(a, b), steps(&grid, a, b), "{grid:?} {a} {b}");
      assert_eq!(grid.distance(a, b), grid.distance(b, a));
    }
  }
}

/// A grid's neighbours, from a grid made here and dropped as this returns:
/// the iterator holds the cells, not the grid
fn neighbours_of_a_passing_grid(cell: IVec2) -> impl ExactSizeIterator<Item = IVec2> {
  MapGrid::new(GridShape::Isometric, UVec2::new(64, 32)).neighbours(cell)
}

#[test]
fn neighbours_outlive_the_grid_they_came_from() {
  let around: Vec<IVec2> = neighbours_of_a_passing_grid(IVec2::new(2, 3)).collect();
  assert_eq!(around, [(2, 2), (3, 3), (2, 4), (1, 3)].map(IVec2::from));
  let held = grid(hex(Y, Odd, 12), 32, 32).neighbours(IVec2::ZERO);
  assert_eq!(held.len(), 6);
}

// ---- lines, ranges and rings ------------------------------------------------

fn is_chain(grid: &MapGrid, cells: &[IVec2]) -> bool {
  cells
    .windows(2)
    .all(|w| grid.neighbours(w[0]).any(|n| n == w[1]))
}

#[test]
fn hex_lines_ranges_and_rings_match_red_blob() {
  let board = grid(hex(Y, Odd, 12), 32, 32);
  let (from, to) = (IVec2::ZERO, IVec2::new(4, -3));
  let line = board.line(from, to);
  assert!(line.len() >= 5, "{line:?}");
  assert_eq!(line.len() as u32, board.distance(from, to) + 1);
  assert_eq!((line[0], line[line.len() - 1]), (from, to));
  assert!(is_chain(&board, &line), "{line:?}");
  let center = IVec2::new(3, 5);
  assert_eq!(board.range(center, 2).len(), 19);
  assert_eq!(board.ring(center, 2).len(), 12);

  let mut rng = Rng(0x7ed_b10b);
  for grid in grids() {
    let hex = is_hex(&grid);
    for center in [IVec2::ZERO, IVec2::new(3, -2), IVec2::new(-5, 4)] {
      for n in 0..=4u32 {
        let r = n as i32;
        let range = grid.range(center, n);
        assert_eq!(range[0], center, "{grid:?}");
        let within: HashSet<IVec2> = range.iter().copied().collect();
        assert_eq!(
          within.len(),
          range.len(),
          "{grid:?} {center} {n}: a cell twice"
        );
        let expect: HashSet<IVec2> = cells_around(center, r + 2)
          .filter(|&c| grid.distance(center, c) <= n)
          .collect();
        assert_eq!(within, expect, "{grid:?} {center} {n}");
        let count = if hex {
          1 + 3 * n * (n + 1)
        } else {
          1 + 2 * n * (n + 1)
        };
        assert_eq!(range.len() as u32, count, "{grid:?} {center} {n}");

        let ring = grid.ring(center, n);
        let around: HashSet<IVec2> = ring.iter().copied().collect();
        assert_eq!(
          around.len(),
          ring.len(),
          "{grid:?} {center} {n}: a cell twice"
        );
        let expect: HashSet<IVec2> = cells_around(center, r + 2)
          .filter(|&c| grid.distance(center, c) == n)
          .collect();
        assert_eq!(around, expect, "{grid:?} {center} {n}");
        if n == 0 {
          assert_eq!(ring, [center]);
          continue;
        }
        assert_eq!(ring.len() as u32, if hex { 6 * n } else { 4 * n });
        let middle = grid.cell_center(center);
        let bearings: Vec<f64> = ring
          .iter()
          .map(|&c| bearing(grid.cell_center(c) - middle))
          .collect();
        assert!(clockwise(&bearings), "{grid:?} {center} {n} {bearings:?}");
        // From the cell n steps along the first neighbour's way
        let first = grid.neighbours(center).next().expect("a neighbour");
        assert_eq!(
          grid.distance(first, ring[0]) + 1,
          n,
          "{grid:?} {center} {n}"
        );
      }
    }
    for _ in 0..60 {
      let a = IVec2::new(rng.int(-20, 20), rng.int(-20, 20));
      let b = IVec2::new(rng.int(-20, 20), rng.int(-20, 20));
      let line = grid.line(a, b);
      assert_eq!(
        line.len() as u32,
        grid.distance(a, b) + 1,
        "{grid:?} {a} {b}"
      );
      assert_eq!((line[0], line[line.len() - 1]), (a, b), "{grid:?}");
      assert!(is_chain(&grid, &line), "{grid:?} {a} {b} {line:?}");
      assert_eq!(grid.line(a, b), line, "the same every time");
    }
  }
  // Square lines break ties one way: across a diagonal, the step along x first
  let square = grid(GridShape::Orthogonal, 32, 32);
  let diagonal = [(0, 0), (1, 0), (1, 1), (2, 1), (2, 2)].map(IVec2::from);
  assert_eq!(square.line(IVec2::ZERO, IVec2::new(2, 2)), diagonal);
}

// ---- chunk ranges and bounds ------------------------------------------------

/// Whether a convex outline and a rectangle overlap over some area (by
/// separating axes: touching is not overlapping)
fn overlaps(outline: &[Vec2], (lo, hi): (Vec2, Vec2)) -> bool {
  if lo.x >= hi.x || lo.y >= hi.y {
    return false;
  }
  let corners = [lo, Vec2::new(hi.x, lo.y), hi, Vec2::new(lo.x, hi.y)];
  let mut axes = vec![Vec2::X, Vec2::Y];
  axes.extend(edges(outline).map(|(a, b)| Vec2::new(a.y - b.y, b.x - a.x)));
  axes.iter().all(|axis| {
    let span = |points: &mut dyn Iterator<Item = Vec2>| {
      points.fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), p| {
        let d = f64::from(p.x) * f64::from(axis.x) + f64::from(p.y) * f64::from(axis.y);
        (lo.min(d), hi.max(d))
      })
    };
    let (a, b) = (
      span(&mut outline.iter().copied()),
      span(&mut corners.iter().copied()),
    );
    a.1 > b.0 && b.1 > a.0
  })
}

/// A random view: a rectangle anywhere (negative coordinates too), some a
/// point or a line, some snapped to whole cells, some a turned camera's box
fn random_rect(rng: &mut Rng) -> Rect {
  let (x, y) = (rng.float(-2500.0, 2500.0), rng.float(-2500.0, 2500.0));
  let (mut w, mut h) = (rng.float(0.0, 900.0), rng.float(0.0, 900.0));
  if rng.chance(10) {
    w = 0.0;
  }
  if rng.chance(10) {
    h = 0.0;
  }
  if rng.chance(20) {
    let snap = |v: f32| (v / 16.0).round() * 16.0;
    return Rect::new(snap(x), snap(y), snap(w), snap(h));
  }
  if rng.chance(30) {
    let turn = rng.float(0.0, std::f32::consts::TAU);
    let (centre, half) = (Vec2::new(x, y), Vec2::new(w, h) * 0.5);
    let rotate = glam::Mat2::from_angle(turn);
    let corners = [
      half,
      Vec2::new(-half.x, half.y),
      -half,
      Vec2::new(half.x, -half.y),
    ]
    .map(|c| centre + rotate * c);
    let lo = corners.iter().fold(Vec2::INFINITY, |m, &c| m.min(c));
    let hi = corners.iter().fold(Vec2::NEG_INFINITY, |m, &c| m.max(c));
    return Rect::new(lo.x, lo.y, hi.x - lo.x, hi.y - lo.y);
  }
  Rect::new(x, y, w, h)
}

#[test]
fn chunks_over_covers_every_cell_a_rect_touches() {
  let mut rng = Rng(0xc4_0c4);
  for grid in grids() {
    for _ in 0..100 {
      let rect = random_rect(&mut rng);
      let (lo, hi) = (
        Vec2::new(rect.x, rect.y),
        Vec2::new(rect.right(), rect.bottom()),
      );
      let (first, last) = grid.chunks_over(rect);
      let mut needed: Option<(IVec2, IVec2)> = None;
      let mut note = |cell: IVec2| {
        let chunk = TileGrid::chunk_of(cell);
        assert!(
          chunk.cmpge(first).all() && chunk.cmple(last).all(),
          "{grid:?} {rect:?}: {cell} in {chunk}, not in {first}..={last}"
        );
        needed = Some(needed.map_or((chunk, chunk), |(a, b)| (a.min(chunk), b.max(chunk))));
      };
      // The cells holding its corners and its middle (a point's or a line's
      // cells), and every cell overlapping it
      let points = [
        lo,
        Vec2::new(hi.x, lo.y),
        hi,
        Vec2::new(lo.x, hi.y),
        (lo + hi) * 0.5,
      ];
      let picked = points.map(|p| grid.cell_at(p));
      picked.iter().for_each(|&c| note(c));
      let reach_lo = picked.iter().fold(IVec2::MAX, |m, &c| m.min(c)) - 3;
      let reach_hi = picked.iter().fold(IVec2::MIN, |m, &c| m.max(c)) + 3;
      for y in reach_lo.y..=reach_hi.y {
        for x in reach_lo.x..=reach_hi.x {
          let c = IVec2::new(x, y);
          if overlaps(grid.cell_outline(c).points(), (lo, hi)) {
            note(c);
          }
        }
      }
      // A small over-reach: no more than a chunk past those, each side
      let (low, high) = needed.expect("a cell");
      assert!(
        first.cmpge(low - 1).all() && last.cmple(high + 1).all(),
        "{grid:?} {rect:?}: {first}..={last} for {low}..={high}"
      );
    }
    // Not finite: no chunks
    for rect in [
      Rect::new(f32::NAN, 0.0, 10.0, 10.0),
      Rect::new(0.0, 0.0, f32::NAN, 10.0),
      Rect::new(0.0, 0.0, 10.0, f32::INFINITY),
    ] {
      let (first, last) = grid.chunks_over(rect);
      assert!(first.x > last.x || first.y > last.y, "{grid:?} {rect:?}");
    }
  }
}

/// The box holding every cell of `cells`' outline, worked out cell by cell
fn union_of(grid: &MapGrid, cells: CellRect) -> Rect {
  let (mut lo, mut hi) = (Vec2::INFINITY, Vec2::NEG_INFINITY);
  for y in cells.min.y..cells.max.y {
    for x in cells.min.x..cells.max.x {
      let b = grid.cell_bounds(IVec2::new(x, y));
      lo = lo.min(Vec2::new(b.x, b.y));
      hi = hi.max(Vec2::new(b.right(), b.bottom()));
    }
  }
  Rect::new(lo.x, lo.y, hi.x - lo.x, hi.y - lo.y)
}

#[test]
fn chunk_bounds_and_bounds_of_hold_their_cells_exactly() {
  let mut rng = Rng(0xb0_0d5);
  let side = CHUNK_SIZE as i32;
  for grid in grids() {
    for chunk in [IVec2::ZERO, IVec2::new(-1, 2), IVec2::new(3, -4)] {
      let cells = CellRect::new(chunk * side, chunk * side + side);
      let union = union_of(&grid, cells);
      assert_eq!(grid.chunk_bounds(chunk), union, "{grid:?} {chunk}");
      assert_eq!(grid.bounds_of(cells), union, "{grid:?} {chunk}");
    }
    for _ in 0..40 {
      let min = IVec2::new(rng.int(-30, 30), rng.int(-30, 30));
      let cells = CellRect::new(min, min + IVec2::new(rng.int(1, 6), rng.int(1, 6)));
      assert_eq!(
        grid.bounds_of(cells),
        union_of(&grid, cells),
        "{grid:?} {cells:?}"
      );
    }
    let one = IVec2::new(-3, 7);
    assert_eq!(
      grid.bounds_of(CellRect::new(one, one + 1)),
      grid.cell_bounds(one)
    );
    let empty = grid.bounds_of(CellRect::new(IVec2::ONE, IVec2::ONE));
    assert_eq!((empty.width, empty.height), (0.0, 0.0));
  }
}

#[test]
fn chunk_boxes_give_each_chunk_s_exact_box() {
  let side = CHUNK_SIZE as i32;
  let far = TileGrid::chunk_of(IVec2::MAX);
  let near_end = TileGrid::chunk_of(IVec2::MIN);
  let mut rng = Rng(0xc4_b0c5);
  for grid in grids() {
    let boxes = grid.chunk_boxes();
    let mut chunks = vec![
      IVec2::ZERO,
      IVec2::new(-1, 2),
      far,
      near_end,
      IVec2::new(far.x, near_end.y),
    ];
    chunks.extend((0..40).map(|_| IVec2::new(rng.int(-5000, 5000), rng.int(-5000, 5000))));
    for chunk in chunks {
      let first = chunk * side;
      let exact = grid.exact_box(first, first + IVec2::splat(side - 1));
      assert_eq!(boxes.of(chunk), exact, "{grid:?} {chunk}");
    }
    // And the box worked out from a chunk's cells (`chunk_bounds`), where
    // f32 holds it exactly
    for chunk in [IVec2::ZERO, IVec2::new(-1, 2), IVec2::new(3, -4)] {
      let (lo, hi) = boxes.of(chunk);
      let size = (hi - lo).as_vec2();
      let from_steps = Rect::new(lo.x as f32, lo.y as f32, size.x, size.y);
      assert_eq!(from_steps, grid.chunk_bounds(chunk), "{grid:?} {chunk}");
    }
    // Meeting a box counts touching it
    let (lo, hi) = boxes.of(IVec2::new(2, -1));
    assert!(boxes.meets(IVec2::new(2, -1), (hi, hi + 1.0)));
    assert!(boxes.meets(IVec2::new(2, -1), (lo - 1.0, lo)));
    assert!(!boxes.meets(IVec2::new(2, -1), (hi + 0.5, hi + 1.0)));
  }
}

// ---- the lattice ------------------------------------------------------------

#[test]
fn a_staggered_lattice_agrees_with_its_cells() {
  let sorted = |mut points: Vec<Vec2>| {
    points.sort_by(|a, b| a.x.total_cmp(&b.x).then(a.y.total_cmp(&b.y)));
    points
  };
  for grid in grids() {
    let Some(lattice) = grid.lattice() else {
      assert!(is_hex(&grid), "{grid:?}");
      continue;
    };
    let staggered = matches!(grid.shape, GridShape::Staggered { .. });
    for c in cells_around(IVec2::ZERO, 20) {
      let l = lattice.lattice_cell(c);
      assert_eq!(lattice.grid_cell(l), c, "{grid:?}");
      if !staggered {
        assert_eq!(l, c);
      }
      // Lattice cell (x, y) is the unit square at (x, y): the cell's outline
      assert_eq!(
        lattice.to_pixel.transform_point2(l.as_vec2() + 0.5),
        grid.cell_center(c),
        "{grid:?} {c}"
      );
      let square = [(0, 0), (1, 0), (1, 1), (0, 1)].map(|d| {
        lattice
          .to_pixel
          .transform_point2((l + IVec2::from(d)).as_vec2())
      });
      assert_eq!(
        sorted(square.to_vec()),
        sorted(grid.cell_outline(c).points().to_vec()),
        "{grid:?} {c}"
      );
      // Its neighbours are the lattice's
      let through: Vec<IVec2> = [(0, -1), (1, 0), (0, 1), (-1, 0)]
        .map(|d| lattice.grid_cell(l + IVec2::from(d)))
        .to_vec();
      assert_eq!(
        grid.neighbours(c).collect::<Vec<_>>(),
        through,
        "{grid:?} {c}"
      );
    }
    for l in cells_around(IVec2::new(4, -9), 20) {
      assert_eq!(lattice.lattice_cell(lattice.grid_cell(l)), l, "{grid:?}");
    }
  }
}

// ---- drawing order and sorting ----------------------------------------------

#[test]
fn paint_keys_order_as_tiled_draws() {
  let mut rng = Rng(0x9a1_7e5);
  // Rows top to bottom, then columns left to right; on isometric grids
  // the diagonals (x + y) top to bottom, each left to right
  for grid in grids() {
    let iso = grid.shape == GridShape::Isometric;
    for _ in 0..500 {
      let a = IVec2::new(rng.int(-50, 50), rng.int(-50, 50));
      let b = IVec2::new(rng.int(-50, 50), rng.int(-50, 50));
      let (row_a, row_b) = if iso {
        (a.x + a.y, b.x + b.y)
      } else {
        (a.y, b.y)
      };
      if row_a < row_b {
        assert!(grid.paint_key(a) < grid.paint_key(b), "{grid:?} {a} {b}");
      }
      if row_a == row_b
        && a.x < b.x
        && !matches!(
          grid.shape,
          GridShape::Staggered { axis: X, .. } | GridShape::Hexagonal { axis: X, .. }
        )
      {
        assert!(grid.paint_key(a) < grid.paint_key(b), "{grid:?} {a} {b}");
      }
    }
  }
  // Axis-X staggered and hexagonal maps draw a row's unshifted columns
  // first, then its shifted ones (measured: the formats note)
  let key = |shape: GridShape, x: i32, y: i32| grid(shape, 32, 32).paint_key(IVec2::new(x, y));
  for shape in [stag(X, Odd), hex(X, Odd, 12)] {
    assert!(key(shape, 2, 2) < key(shape, 3, 2));
    assert!(key(shape, 4, 2) < key(shape, 3, 2));
  }
  for shape in [stag(X, Even), hex(X, Even, 12)] {
    assert!(key(shape, 3, 2) < key(shape, 2, 2));
    assert!(key(shape, 3, 2) < key(shape, 4, 2));
  }
  // Oblique maps draw as orthogonal ones do, whatever the skew (measured):
  // right-down, the engine's order, draws the lower of a tall pair and the
  // right of a wide pair over the other
  let measured = measured();
  let right_down: Vec<_> = measured
    .oblique_order
    .iter()
    .filter(|o| o.1 == "right-down")
    .collect();
  assert_eq!(right_down.len(), 3);
  for (skew, _, over) in right_down {
    let shape = GridShape::Oblique { skew: *skew };
    assert_eq!(*over, [IVec2::new(3, 4), IVec2::new(4, 3)]);
    assert!(key(shape, 3, 4) > key(shape, 3, 3));
    assert!(key(shape, 4, 3) > key(shape, 3, 3));
  }
}

/// Tiled's overlapping pairs (`measured-order.json`): the grid of each,
/// both cells and the one drawn over the other
fn measured_order() -> Vec<(MapGrid, IVec2, IVec2, IVec2)> {
  let path = format!("{FIXTURES}/tiled/measured-order.json");
  let text = std::fs::read_to_string(&path).expect("measured-order.json");
  let json: Value = serde_json::from_str(&text).expect("measured-order.json is JSON");
  assert_eq!(json["tiled"], "1.12.2");
  let grids: Vec<(String, MapGrid)> = json["grids"]
    .as_array()
    .expect("grids")
    .iter()
    .map(|g| {
      let tile = pair(&g["tile"]).as_uvec2();
      let stagger = || {
        let axis = if g["staggeraxis"] == "x" { X } else { Y };
        let index = if g["staggerindex"] == "odd" {
          Odd
        } else {
          Even
        };
        (axis, index)
      };
      let shape = match g["orientation"].as_str() {
        Some("isometric") => GridShape::Isometric,
        Some("staggered") => {
          let (axis, index) = stagger();
          stag(axis, index)
        }
        Some("hexagonal") => {
          let (axis, index) = stagger();
          hex(axis, index, int(&g["hexsidelength"]) as u32)
        }
        other => panic!("measured-order.json: orientation {other:?}"),
      };
      let name = g["name"].as_str().expect("a name").to_string();
      (name, MapGrid::new(shape, tile))
    })
    .collect();
  json["pairs"]
    .as_array()
    .expect("pairs")
    .iter()
    .map(|p| {
      let name = p["grid"].as_str().expect("a grid");
      let Some((_, grid)) = grids.iter().find(|(n, _)| n == name) else {
        panic!("measured-order.json: no grid {name}");
      };
      (*grid, pair(&p["a"]), pair(&p["b"]), pair(&p["over"]))
    })
    .collect()
}

#[test]
fn paint_keys_order_overlapping_tiles_as_tileds_renders_do() {
  // Pairs of tiles larger than their cells, rendered by Tiled: the one it
  // draws over the other has the larger paint key. On an isometric map
  // that is the later diagonal, though its row is the earlier: (7, 9) over
  // (9, 6), (9, 7) over (7, 8), and in one diagonal the cell further right,
  // (8, 7) over (7, 8)
  let pairs = measured_order();
  assert_eq!(pairs.len(), 13);
  for (grid, a, b, over) in pairs {
    let under = if over == a { b } else { a };
    assert!(
      grid.paint_key(over) > grid.paint_key(under),
      "{grid:?}: Tiled draws {over} over {under}"
    );
  }
}

#[test]
fn sort_bands_share_their_cells_sort_point() {
  for grid in grids() {
    for c in cells_around(IVec2::ZERO, 12) {
      let band = grid.sort_band(c);
      let shifted = |line: i32| (line.rem_euclid(2) == 1) as i32;
      let expect = match grid.shape {
        GridShape::Isometric => c.x + c.y,
        GridShape::Staggered { axis: X, index } | GridShape::Hexagonal { axis: X, index, .. } => {
          2 * c.y
            + if index == Odd {
              shifted(c.x)
            } else {
              1 - shifted(c.x)
            }
        }
        _ => c.y,
      };
      assert_eq!(band, expect, "{grid:?} {c}");
      match grid.shape {
        // 6a's rule: a row sorts at its bottom edge
        GridShape::Orthogonal => {
          assert_eq!(
            grid.band_y(band),
            (c.y as f32 + 1.0) * grid.cell.y as f32 + grid.origin.y
          );
        }
        // A row skewed down by its columns has no one sort point
        GridShape::Oblique { skew } if skew.y != 0 => {}
        _ => assert_eq!(grid.band_y(band), grid.cell_center(c).y, "{grid:?} {c}"),
      }
    }
  }
}

// ---- degenerate grids -------------------------------------------------------

#[test]
fn degenerate_grids_answer_without_panicking() {
  let shapes = [
    GridShape::Orthogonal,
    GridShape::Isometric,
    stag(X, Odd),
    hex(Y, Even, 40),
    hex(X, Odd, u32::MAX),
    // Folded flat, and folded over
    oblique(32, 32),
    oblique(40, 40),
    oblique(i32::MIN, i32::MAX),
  ];
  let sizes = [
    UVec2::ZERO,
    UVec2::ONE,
    UVec2::new(0, 32),
    UVec2::new(32, 32),
    UVec2::MAX,
  ];
  let cells = [
    IVec2::ZERO,
    IVec2::MIN,
    IVec2::MAX,
    IVec2::new(i32::MIN, i32::MAX),
  ];
  let points = [
    Vec2::ZERO,
    Vec2::NAN,
    Vec2::INFINITY,
    Vec2::splat(-1e30),
    Vec2::splat(f32::MAX),
  ];
  for shape in shapes {
    for size in sizes {
      let grid = MapGrid::new(shape, size).with_origin(Vec2::new(-7.5, 3.0));
      let _ = MapGrid::for_map(shape, size, UVec2::MAX, false);
      for &c in &cells {
        let _ = (
          grid.cell_center(c),
          grid.cell_outline(c),
          grid.cell_bounds(c),
          grid.tile_anchor(c),
        );
        let _ = (
          grid.neighbours(c).count(),
          grid.distance(c, IVec2::ZERO),
          grid.distance(IVec2::MIN, IVec2::MAX),
        );
        let _ = (
          grid.sort_band(c),
          grid.band_y(grid.sort_band(c)),
          grid.paint_key(c),
        );
        let _ = (grid.line(c, c), grid.range(c, 1), grid.ring(c, 1));
        let _ = grid.chunk_bounds(TileGrid::chunk_of(c));
        if let Some(lattice) = grid.lattice() {
          let _ = lattice.grid_cell(lattice.lattice_cell(c));
        }
      }
      for &p in &points {
        let _ = grid.cell_at(p);
        let _ = grid.chunks_over(Rect::new(p.x, p.y, 10.0, 10.0));
      }
      let _ = grid.chunks_over(Rect::new(-1e30, -1e30, 2e30, 2e30));
      let _ = grid.bounds_of(CellRect::new(IVec2::MIN, IVec2::MAX));
      let _ = grid.bounds_of(CellRect::new(IVec2::ONE, IVec2::ZERO));
    }
  }
}

/// The hexagonal grids the integer geometry is held to: every axis and
/// index, at the cell sizes and sides that matter
fn hex_variants() -> Vec<MapGrid> {
  let mut out = Vec::new();
  for (cell, side) in [
    ((16, 16), 8),
    ((120, 140), 70),
    ((15, 17), 7),
    ((16, 16), 0),
    ((16, 16), 24),
  ] {
    for axis in [X, Y] {
      for index in [Odd, Even] {
        for origin in [Vec2::ZERO, Vec2::new(5.0, -3.0)] {
          out.push(grid(hex(axis, index, side), cell.0, cell.1).with_origin(origin));
        }
      }
    }
  }
  out
}

/// A hexagon's corners in the grid's pixels worked out from Tiled's
/// hexagonal renderer (`sideOffset`, `columnWidth`, `rowHeight`, the
/// tile-to-pixel steps and the shift on staggered lines), with this
/// engine's truncation: half a cell and the slant round toward zero, the
/// slant clamped at 0. Built for axis Y, swapped and turned back clockwise
/// for axis X. In `f64`, from the origin
fn tiled_hex_corners(grid: &MapGrid, cell: IVec2) -> [glam::DVec2; 6] {
  let GridShape::Hexagonal { axis, index, side } = grid.shape else {
    unreachable!("a hexagonal grid");
  };
  let (w, h) = (i64::from(grid.cell.x), i64::from(grid.cell.y));
  let (across, along) = if axis == Y { (w, h) } else { (h, w) };
  let side = i64::from(side);
  let side_offset = (along - side) / 2;
  let row_height = side_offset + side;
  let column_width = across / 2;
  let (line, pos) = if axis == Y {
    (i64::from(cell.y), i64::from(cell.x))
  } else {
    (i64::from(cell.x), i64::from(cell.y))
  };
  let shifted = (line.rem_euclid(2) == 1) == (index == Odd);
  let u = pos * 2 * column_width + if shifted { column_width } else { 0 };
  let v = line * row_height;
  let (hw, slant) = (column_width, side_offset.max(0));
  let frame = [
    (hw, 0),
    (2 * hw, slant),
    (2 * hw, row_height),
    (hw, row_height + slant),
    (0, row_height),
    (0, slant),
  ];
  let mut out = frame.map(|(du, dv)| {
    let (a, b) = ((u + du) as f64, (v + dv) as f64);
    if axis == Y {
      glam::DVec2::new(a, b)
    } else {
      glam::DVec2::new(b, a)
    }
  });
  if axis == X {
    out.reverse();
  }
  out
}

#[test]
fn hex_cells_agree_with_cell_outlines_on_every_variant() {
  for grid in hex_variants() {
    let GridShape::Hexagonal { axis, index, side } = grid.shape else {
      continue;
    };
    let hex = HexCells::new(axis, index, grid.cell, side);
    let far: &[IVec2] = if grid.cell.x == 16 {
      &[IVec2::new(i32::MAX / 16 - 1, 5)]
    } else if grid.cell.x == 120 {
      &[IVec2::new(-(i32::MAX / 120), i32::MAX / 140)]
    } else {
      &[]
    };
    let near = (-40..40).flat_map(|y| (-40..40).map(move |x| IVec2::new(x, y)));
    for cell in near.chain(far.iter().copied()) {
      let want: Vec<Vec2> = tiled_hex_corners(&grid, cell)
        .iter()
        .map(|c| (*c + grid.origin.as_dvec2()).as_vec2())
        .collect();
      assert_eq!(
        grid.cell_outline(cell).points(),
        want,
        "{:?} {cell}",
        grid.shape
      );
      let exact: Vec<glam::DVec2> = hex.corners(cell).iter().map(|c| c.as_dvec2()).collect();
      assert_eq!(
        exact,
        tiled_hex_corners(&grid, cell),
        "{:?} {cell}",
        grid.shape
      );
    }
  }
}

#[test]
fn each_hex_side_is_its_neighbours_side_reversed() {
  for grid in hex_variants() {
    let hex = grid.hex_cells().expect("hex");
    let flat = matches!(grid.shape, GridShape::Hexagonal { side: 0, .. });
    for y in -20..20 {
      for x in -20..20 {
        let c = IVec2::new(x, y);
        let corners = hex.corners(c);
        for s in 0..6 {
          let n = hex.across(c, s);
          assert_eq!(
            Some(n),
            grid.neighbours(c).nth(s),
            "{:?} {c} side {s}",
            grid.shape
          );
          let back = hex.corners(n);
          let t = (s + 3) % 6;
          if flat {
            continue;
          }
          assert_eq!(
            (corners[s], corners[(s + 1) % 6]),
            (back[(t + 1) % 6], back[t]),
            "{:?} {c} side {s}",
            grid.shape
          );
        }
      }
    }
  }
}

#[test]
fn hex_cells_trace_only_cells_up_to_16384_px() {
  let cells = |x, y| HexCells::new(Y, Odd, UVec2::new(x, y), 8);
  assert!(!cells(16_385, 16).traceable());
  assert!(!cells(16, 16_385).traceable());
  assert!(cells(16_384, 16_384).traceable());
  // Past that the corners are still exact, and `points` says where they don't fit i32
  let big = cells(40_000, 30_000);
  assert_eq!(
    big.corners(IVec2::new(1, 1))[0],
    glam::I64Vec2::new(80_000, 15_004)
  );
  assert!(big.points(IVec2::new(1, 1)).is_some());
  assert!(big.points(IVec2::new(i32::MAX, 1)).is_none());
}
