//! What a map's cell is, for collision or for light: tilesets read once
//! into `ShapeSource`s (so another thread can read them), a tile's shapes
//! placed where it draws (on a hexagonal grid, turned as it is drawn) and
//! classified as a `TileCell`, IntGrid values by
//! their names, a group of layers' union, and which chunks changed since a
//! group last read its layers. Pure but for the queries' aliases, so a
//! map's first build can run off the main thread.

use std::cell::Cell;
use std::sync::Arc;

use glam::{DAffine2, DVec2, IVec2, Vec2};

use super::spawn::{IntGridKey, TileLayerData};
use super::{
  CHUNK_SIZE, GridShape, Lattice, LayerInfo, LdtkProject, MapGrid, Tile, TileFlip, TileGrid,
  TileMap, Tileset, TilesetImage, TilesetSource,
};
use crate::core::asset::{AssetId, Assets};
use crate::core::ecs::{Allow, DetectChanges, Disabled, Entity, Has, Query, Ref, Res};
use crate::core::hash::FastMap;
use crate::core::physics::{
  Boundary, CellRect, CellShape, HexCells, SlopeCorner, TileCell, TileColliders,
};

/// How near a point must be to a cell corner to count as on it, in tiles
const EPSILON: f32 = 1e-4;

/// Shape points are compared on this grid (a tile's points), so two tiles
/// drawing the same shape share it
const KEY_GRID: f32 = 46080.0;

/// The asset stores a map's collision and occluders read
pub(super) type Stores<'w> = (
  Option<Res<'w, Assets<TileMap>>>,
  Option<Res<'w, Assets<LdtkProject>>>,
  Option<Res<'w, Assets<Tileset>>>,
);

pub(super) type Layers<'w, 's> = Query<
  'w,
  's,
  (
    &'static LayerInfo,
    Ref<'static, TileLayerData>,
    Has<Disabled>,
  ),
  Allow<Disabled>,
>;
/// A tileset as the colliders were built from it: an asset's id and
/// content version, or an embedded one (held, so its address is not
/// reused while the stamp is kept)
#[derive(Clone, Debug)]
pub(super) enum TilesetStamp {
  Asset(AssetId<Tileset>, Option<u64>),
  Embedded(Arc<Tileset>),
}

impl TilesetStamp {
  fn of(source: &TilesetSource, assets: Option<&Assets<Tileset>>) -> TilesetStamp {
    match source {
      TilesetSource::Asset(handle) => {
        TilesetStamp::Asset(handle.id(), assets.and_then(|a| a.version(handle)))
      }
      TilesetSource::Embedded(tileset) => TilesetStamp::Embedded(Arc::clone(tileset)),
    }
  }

  fn matches(&self, source: &TilesetSource, assets: Option<&Assets<Tileset>>) -> bool {
    match (self, source) {
      (TilesetStamp::Asset(id, version), TilesetSource::Asset(handle)) => {
        *id == handle.id() && *version == assets.and_then(|a| a.version(handle))
      }
      (TilesetStamp::Embedded(held), TilesetSource::Embedded(tileset)) => {
        Arc::ptr_eq(held, tileset)
      }
      _ => false,
    }
  }
}

/// The stamps of a map's tilesets
pub(super) fn stamps(map: &TileMap, assets: Option<&Assets<Tileset>>) -> Vec<TilesetStamp> {
  map
    .tilesets
    .iter()
    .map(|source| TilesetStamp::of(source, assets))
    .collect()
}

/// Whether `stamps` are still those of `map`'s tilesets
pub(super) fn stamps_match(
  stamps: &[TilesetStamp],
  map: &TileMap,
  assets: Option<&Assets<Tileset>>,
) -> bool {
  stamps.len() == map.tilesets.len()
    && stamps
      .iter()
      .zip(&map.tilesets)
      .all(|(stamp, source)| stamp.matches(source, assets))
}

/// Whether what was built from `shapes` (and `stairs`, for what reads
/// them) still stands for `map`'s tilesets: their stamps match, or they
/// moved (an image reloaded, or a tileset saved with other tile data) and
/// the tilesets give the same shapes and stairs, so the stamps are brought
/// up to date and nothing is built again
pub(super) fn shapes_kept(
  stamps: &mut Vec<TilesetStamp>,
  (shapes, stairs): (&[Option<ShapeSource>], Option<&[Option<StairSource>]>),
  map: &TileMap,
  assets: Option<&Assets<Tileset>>,
) -> bool {
  if stamps_match(stamps, map, assets) {
    return true;
  }
  if *sources(map, assets) != *shapes {
    return false;
  }
  if stairs.is_some_and(|stairs| *stair_sources(map, assets) != *stairs) {
    return false;
  }
  *stamps = self::stamps(map, assets);
  true
}

/// A tileset's stairs, taken from it once (so another thread can read
/// them): each stair tile's id and the floor it leads up to
#[derive(Debug, Default, PartialEq)]
pub(super) struct StairSource {
  tiles: FastMap<u32, u8>,
}

impl StairSource {
  fn of(tileset: &Tileset) -> StairSource {
    StairSource {
      tiles: tileset
        .tiles()
        .filter_map(|(id, data)| Some((id, data.stairs?)))
        .collect(),
    }
  }
}

/// The stairs of a map's tilesets, by index (`None` for one not in its
/// store)
pub(super) fn stair_sources(
  map: &TileMap,
  assets: Option<&Assets<Tileset>>,
) -> Arc<[Option<StairSource>]> {
  map
    .tilesets
    .iter()
    .map(|source| match source {
      TilesetSource::Asset(handle) => assets.and_then(|a| a.get(handle)).map(StairSource::of),
      TilesetSource::Embedded(tileset) => Some(StairSource::of(tileset)),
    })
    .collect()
}

/// The floor `tile` is stairs up to, by its tileset's stairs (`stairs`)
pub(super) fn stairs_of(stairs: &[Option<StairSource>], tile: Tile) -> Option<u8> {
  let source = stairs.get(usize::from(tile.tileset))?.as_ref()?;
  source.tiles.get(&tile.id).copied()
}

/// What placing and classifying a tileset's tiles reads, taken from it once
/// (so another thread can read it): its tiles with shapes, each with the
/// size it draws at, and the tileset's offset
#[derive(Debug, Default, PartialEq)]
pub(super) struct ShapeSource {
  offset: Vec2,
  tiles: FastMap<u32, SourceTile>,
}

#[derive(Debug, PartialEq)]
struct SourceTile {
  /// The size it draws at
  size: Vec2,
  shapes: Vec<SourceShape>,
}

#[derive(Debug, PartialEq)]
struct SourceShape {
  points: Vec<Vec2>,
  one_way: bool,
  occluder: bool,
}

impl ShapeSource {
  fn of(tileset: &Tileset) -> ShapeSource {
    let tiles = tileset
      .tiles()
      .filter(|(_, data)| !data.shapes.is_empty())
      .map(|(id, data)| {
        let shapes = data
          .shapes
          .iter()
          .map(|shape| SourceShape {
            points: shape.points.clone(),
            one_way: shape.one_way,
            occluder: shape.occluder,
          })
          .collect();
        let size = drawn_size(tileset, id);
        (id, SourceTile { size, shapes })
      })
      .collect();
    ShapeSource {
      offset: tileset.tile_offset.as_vec2(),
      tiles,
    }
  }
}

/// The shape sources of a map's tilesets, by index (`None` for one not in
/// its store)
pub(super) fn sources(
  map: &TileMap,
  assets: Option<&Assets<Tileset>>,
) -> Arc<[Option<ShapeSource>]> {
  map
    .tilesets
    .iter()
    .map(|source| match source {
      TilesetSource::Asset(handle) => assets.and_then(|a| a.get(handle)).map(ShapeSource::of),
      TilesetSource::Embedded(tileset) => Some(ShapeSource::of(tileset)),
    })
    .collect()
}

/// The size tile `id` draws at: the tileset's, or a collection tile's own
fn drawn_size(tileset: &Tileset, id: u32) -> Vec2 {
  let own = match tileset.image {
    TilesetImage::Atlas { .. } => None,
    TilesetImage::Collection => tileset
      .tile(id)
      .and_then(|data| data.image.as_ref())
      .map(|(_, size)| *size),
  };
  own.unwrap_or(tileset.tile_size).as_vec2()
}

/// A layer's grid as collision and shadows group layers by it: its shape,
/// cell size and origin (by its bits)
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub(super) struct GridKey {
  shape: GridShape,
  cell: glam::UVec2,
  origin: [u32; 2],
}

impl GridKey {
  pub(super) fn of(grid: &MapGrid) -> GridKey {
    GridKey {
      shape: grid.shape,
      cell: grid.cell,
      origin: [grid.origin.x.to_bits(), grid.origin.y.to_bits()],
    }
  }

  pub(super) fn grid(&self) -> MapGrid {
    let origin = Vec2::new(
      f32::from_bits(self.origin[0]),
      f32::from_bits(self.origin[1]),
    );
    MapGrid::new(self.shape, self.cell).with_origin(origin)
  }
}

/// The lattice a grid places its cells through for collision and shadows,
/// lattice units to the layer's pixels: none for an orthogonal grid at the
/// origin (its cells are rectangles of its cell size from it) and for a
/// hexagonal one (which has none)
pub(super) fn lattice_of(grid: &MapGrid) -> Option<glam::Affine2> {
  if grid.shape == GridShape::Orthogonal && grid.origin == Vec2::ZERO {
    return None;
  }
  grid.lattice().map(|lattice| lattice.to_pixel)
}

/// The lattice of a grid that indexes its cells otherwise than it does (a
/// staggered grid's), whose storage chunks' cells straddle its chunks
pub(super) fn reindexing(grid: &MapGrid) -> Option<Lattice> {
  grid.lattice().filter(|lattice| lattice.reindexes())
}

/// How a layer's grid takes a tile's shapes from its image's pixels into
/// its cell's square (the unit square of collision's grid), or on a
/// hexagonal grid into its cell's box
#[derive(Clone, Copy, Debug)]
pub(super) enum Placement {
  /// Upright rectangles of this size: an orthogonal grid's cells, and an
  /// oblique grid's, whose shapes are taken in the cell unskewed (as
  /// Tiled's collision editor shows them, over the unskewed image) and skew
  /// with the grid
  Cells(Vec2),
  /// Through the grid's lattice: `to_square` takes a point of a tile image
  /// standing in cell (0, 0) (from where its bottom-left goes, before its
  /// tileset's offset) to lattice units from cell (0, 0)'s square's
  /// corner. Every cell is that one moved along the lattice, so a tile's
  /// shapes are the same in every cell
  Lattice(DAffine2),
  /// Into a hexagonal cell's box, whose units a shape's are on a hexagonal
  /// grid: from where `MapGrid::tile_anchor` stands the image, flipped,
  /// then turned as drawing turns it
  Hex(HexPlace),
  /// Nowhere (a lattice that folds its cells flat or is not finite, or
  /// hexagons the collision tracer does not take): its tiles collide as
  /// nothing
  Nothing,
}

/// Where a hexagonal grid's cells take a tile, every cell alike, in pixels
/// from the cell's box's top-left (`HexCells::cell_box`)
#[derive(Clone, Copy, Debug)]
pub(super) struct HexPlace {
  /// Where a tile image's bottom-left stands (`MapGrid::tile_anchor`),
  /// before its tileset's offset
  anchor: DVec2,
  /// The box's size: a shape's tile units
  size: DVec2,
  /// The cell's corners, repeats (a side of length 0's) and points along a
  /// side (a long side's middle, on a grid whose side is the cell's length
  /// or longer) dropped: the first `count`
  corners: [DVec2; 6],
  count: usize,
}

/// Whether `p` lies along the side from `a` to `b`, within `slack` of it
/// and between its ends
fn along((a, b): (DVec2, DVec2), p: DVec2, slack: f64) -> bool {
  let side = b - a;
  let length = side.length_squared();
  if length == 0.0 {
    return true;
  }
  let t = (p - a).dot(side) / length;
  side.perp_dot(p - a).abs() <= slack * length.sqrt() && t > 0.0 && t < 1.0
}

impl HexPlace {
  /// The place `grid` (a hexagonal grid, whose cells the tracer takes)
  /// gives
  fn of(grid: &MapGrid) -> Option<HexPlace> {
    let hex = grid.hex_cells().filter(HexCells::traceable)?;
    let (min, size) = hex.cell_box(IVec2::ZERO);
    let corner = min.as_dvec2();
    let anchor = (grid.tile_anchor(IVec2::ZERO) - grid.origin).as_dvec2() - corner;
    let mut kept: Vec<DVec2> = Vec::with_capacity(6);
    for point in hex.corners(IVec2::ZERO) {
      let point = point.as_dvec2() - corner;
      if !kept.contains(&point) {
        kept.push(point);
      }
    }
    // Whole pixels: a point along a side is exactly on it
    while let Some(i) = (0..kept.len()).find(|&i| {
      let n = kept.len();
      n > 3 && along((kept[(i + n - 1) % n], kept[(i + 1) % n]), kept[i], 0.0)
    }) {
      kept.remove(i);
    }
    let mut corners = [DVec2::ZERO; 6];
    corners[..kept.len()].copy_from_slice(&kept);
    let count = kept.len();
    Some(HexPlace {
      anchor,
      size: size.as_dvec2(),
      corners,
      count,
    })
  }

  /// `points` (tile pixels from its image's top-left, y down) where a tile
  /// `size` px whose tileset's offset is `offset` stands in its cell,
  /// flipped by `flip` (its mirrors, not its turns), in pixels from the
  /// box's top-left
  fn flipped(&self, (size, offset): (Vec2, Vec2), flip: TileFlip, points: &[Vec2]) -> Vec<DVec2> {
    let (size, offset) = (size.as_dvec2(), offset.as_dvec2());
    points
      .iter()
      .map(|p| {
        let mut at = p.as_dvec2();
        if flip.contains(TileFlip::HORIZONTAL) {
          at.x = size.x - at.x;
        }
        if flip.contains(TileFlip::VERTICAL) {
          at.y = size.y - at.y;
        }
        self.anchor + offset + DVec2::new(at.x, at.y - size.y)
      })
      .collect()
  }

  /// `flipped` points (as `flipped` gives them) turned by `flip`'s turns
  /// about the tile's quad's middle, where it stands unturned (its
  /// tileset's offset included), as drawing turns it, in the box's units
  fn turned(&self, (size, offset): (Vec2, Vec2), flip: TileFlip, flipped: &[DVec2]) -> Vec<Vec2> {
    let middle = self.anchor + offset.as_dvec2() + DVec2::new(0.5, -0.5) * size.as_dvec2();
    let turn = flip.quad_turn(true).as_dmat2();
    flipped
      .iter()
      .map(|&p| ((middle + turn * (p - middle)) / self.size).as_vec2())
      .collect()
  }

  /// Whether `polygon` (pixels from the box's top-left, flipped and not
  /// turned) is the cell's whole hexagon: without repeated points, and
  /// points along a side (within half a pixel) that are no corner's, its
  /// points are the cell's corners (`corners`), each within half a pixel
  /// of one, and no other
  fn whole(&self, polygon: &[DVec2]) -> bool {
    let near = |a: DVec2, b: DVec2| (a - b).abs().max_element() <= f64::from(EPSILON);
    let mut ring: Vec<DVec2> = Vec::with_capacity(polygon.len());
    for &p in polygon {
      if ring.last().is_none_or(|&q| !near(q, p)) {
        ring.push(p);
      }
    }
    while ring.len() > 1
      && ring
        .first()
        .zip(ring.last())
        .is_some_and(|(&a, &b)| near(a, b))
    {
      ring.pop();
    }
    let corners = &self.corners[..self.count];
    let a_corner = |p: DVec2| corners.iter().any(|c| c.distance(p) <= 0.5);
    while let Some(i) = (0..ring.len()).find(|&i| {
      let n = ring.len();
      let side = (ring[(i + n - 1) % n], ring[(i + 1) % n]);
      n > 3 && !a_corner(ring[i]) && along(side, ring[i], 0.5)
    }) {
      ring.remove(i);
    }
    if ring.len() != self.count {
      return false;
    }
    let mut met = [false; 6];
    ring.iter().all(|&p| {
      let found = (0..self.count).find(|&i| !met[i] && self.corners[i].distance(p) <= 0.5);
      found.inspect(|&i| met[i] = true).is_some()
    })
  }
}

impl Placement {
  /// How `grid` places its tiles' shapes
  pub(super) fn of(grid: &MapGrid) -> Placement {
    if let GridShape::Orthogonal | GridShape::Oblique { .. } = grid.shape {
      return Placement::Cells(grid.cell.as_vec2());
    }
    if let GridShape::Hexagonal { .. } = grid.shape {
      return HexPlace::of(grid).map_or(Placement::Nothing, Placement::Hex);
    }
    let Some(lattice) = grid.lattice() else {
      return Placement::Nothing;
    };
    let to = lattice.to_pixel;
    let to_pixel = DAffine2::from_cols(
      to.matrix2.x_axis.as_dvec2(),
      to.matrix2.y_axis.as_dvec2(),
      to.translation.as_dvec2(),
    );
    if to_pixel.matrix2.determinant() == 0.0 || !to_pixel.is_finite() {
      return Placement::Nothing;
    }
    let anchor = grid.tile_anchor(IVec2::ZERO).as_dvec2();
    let corner = lattice.lattice_cell(IVec2::ZERO).as_dvec2();
    Placement::Lattice(
      DAffine2::from_translation(-corner) * to_pixel.inverse() * DAffine2::from_translation(anchor),
    )
  }
}

/// Whether one-way tiles on `grid` are one-way: a platformer feature for
/// orthogonal maps, which on every other grid collide both ways
fn keeps_one_way(grid: &MapGrid) -> bool {
  grid.shape == GridShape::Orthogonal
}

/// `points` (tile pixels from the tile image's top-left, y down) where the
/// tile draws in its cell, in units of the cell's square from its corner
/// (an orthogonal cell's top-left, in tiles): the tile stands on the cell's
/// bottom edge from the left however large it is (on an oblique grid in
/// the cell unskewed, which skews with the grid; on isometric and
/// staggered grids where `MapGrid::tile_anchor` puts it, taken back
/// through the lattice), the tileset's offset moves it, and its
/// flips (diagonal first, then horizontal, then vertical, as Tiled composes
/// them) turn it, as `render::tiles` draws it; on a hexagonal grid in the
/// units of its cell's box, its mirrors then its turns (`HexPlace`).
/// `size` is the size the tile draws at and `offset` its tileset's
fn placed(
  size: Vec2,
  offset: Vec2,
  tile: Tile,
  points: &[Vec2],
  placement: Placement,
) -> Vec<Vec2> {
  if let Placement::Hex(place) = placement {
    let flipped = place.flipped((size, offset), tile.flip, points);
    return place.turned((size, offset), tile.flip, &flipped);
  }
  let diagonal = tile.flip.contains(TileFlip::DIAGONAL);
  let drawn = if diagonal {
    Vec2::new(size.y, size.x)
  } else {
    size
  };
  points
    .iter()
    .map(|p| {
      let mut at = if diagonal { Vec2::new(p.y, p.x) } else { *p };
      if tile.flip.contains(TileFlip::HORIZONTAL) {
        at.x = drawn.x - at.x;
      }
      if tile.flip.contains(TileFlip::VERTICAL) {
        at.y = drawn.y - at.y;
      }
      match placement {
        Placement::Cells(cell) => {
          let px = Vec2::new(at.x + offset.x, cell.y - drawn.y + at.y + offset.y);
          px / cell
        }
        Placement::Lattice(to_square) => {
          // From the image's bottom-left, moved by the tileset's offset
          let from = DVec2::new(
            f64::from(at.x) + f64::from(offset.x),
            f64::from(at.y) - f64::from(drawn.y) + f64::from(offset.y),
          );
          to_square.transform_point2(from).as_vec2()
        }
        Placement::Hex(_) | Placement::Nothing => Vec2::ZERO,
      }
    })
    .collect()
}

/// `polygon` without repeated points (its closing point included)
pub(super) fn cleaned(polygon: &[Vec2]) -> Vec<Vec2> {
  let mut out: Vec<Vec2> = Vec::with_capacity(polygon.len());
  for &p in polygon {
    if out
      .last()
      .is_none_or(|q| (*q - p).abs().max_element() > EPSILON)
    {
      out.push(p);
    }
  }
  while out.len() > 1
    && out
      .first()
      .zip(out.last())
      .is_some_and(|(a, b)| (*a - *b).abs().max_element() <= EPSILON)
  {
    out.pop();
  }
  out
}

/// The cell corner a point is on: 0 top-left, 1 top-right, 2 bottom-right,
/// 3 bottom-left
fn corner_of(p: Vec2) -> Option<u8> {
  let near = |v: f32, to: f32| (v - to).abs() <= EPSILON;
  match (
    near(p.x, 0.0),
    near(p.x, 1.0),
    near(p.y, 0.0),
    near(p.y, 1.0),
  ) {
    (true, _, true, _) => Some(0),
    (_, true, true, _) => Some(1),
    (_, true, _, true) => Some(2),
    (true, _, _, true) => Some(3),
    _ => None,
  }
}

/// The distinct corners `polygon` is made of, if it is made of corners only
fn corners_only(polygon: &[Vec2]) -> Option<Vec<u8>> {
  let corners: Vec<u8> = polygon
    .iter()
    .map(|&p| corner_of(p))
    .collect::<Option<_>>()?;
  let mut distinct = corners.clone();
  distinct.sort_unstable();
  distinct.dedup();
  (distinct.len() == corners.len()).then_some(corners)
}

/// Twice the signed area, y down
pub(super) fn twice_area(points: &[Vec2]) -> f32 {
  (0..points.len())
    .map(|i| points[i].perp_dot(points[(i + 1) % points.len()]))
    .sum()
}

/// Whether `polygon` (tiles) is exactly its cell
pub(super) fn full_cell(polygon: &[Vec2]) -> bool {
  let ring = cleaned(polygon);
  ring.len() == 4
    && corners_only(&ring).is_some()
    && (twice_area(&ring).abs() - 2.0).abs() <= 4.0 * EPSILON
}

/// The filled corner of `polygon` (tiles), if it is a half cell cut along
/// a diagonal: three of the cell's corners
fn slope_corner(polygon: &[Vec2]) -> Option<SlopeCorner> {
  let ring = cleaned(polygon);
  if ring.len() != 3 {
    return None;
  }
  let corners = corners_only(&ring)?;
  let missing = (0..4).find(|c| !corners.contains(c))?;
  // The right angle is opposite the corner the triangle leaves out
  Some(match (missing + 2) % 4 {
    0 => SlopeCorner::TopLeft,
    1 => SlopeCorner::TopRight,
    2 => SlopeCorner::BottomRight,
    _ => SlopeCorner::BottomLeft,
  })
}

/// A shape's geometry on a fine grid, so equal shapes share an index
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub(super) struct ShapeKey {
  one_way: bool,
  polygons: Vec<Vec<(i64, i64)>>,
}

impl ShapeKey {
  fn of(polygons: &[Vec<Vec2>], one_way: bool) -> ShapeKey {
    let snap = |v: f32| (v * KEY_GRID).round() as i64;
    ShapeKey {
      one_way,
      polygons: polygons
        .iter()
        .map(|p| p.iter().map(|v| (snap(v.x), snap(v.y))).collect())
        .collect(),
    }
  }
}

/// Where new shapes go: the grid being built's list, or the grid itself
pub(super) trait ShapeSink {
  fn add(&mut self, shape: CellShape) -> u16;
}

impl ShapeSink for TileColliders {
  fn add(&mut self, shape: CellShape) -> u16 {
    self.add_shape(shape)
  }
}

impl ShapeSink for Boundary {
  fn add(&mut self, shape: CellShape) -> u16 {
    self.add_shape(shape)
  }
}

/// What a tile's shapes are read for
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Purpose {
  /// Its collision shapes (those not marked `occluder`): its solid ones,
  /// else its one-way ones
  Collision,
  /// What blocks light: its shapes marked `occluder`, else its collision
  /// shapes, one-way ones too (light does not care which way a platform
  /// lets bodies through), all as one solid shape; an IntGrid cell occludes
  /// if it is solid or a slope (a one-way cell is solid only along its top,
  /// which has no area)
  Occlusion,
}

/// The cell kind of `polygons` (tiles, already placed), solid or one-way
fn classify(
  polygons: Vec<Vec<Vec2>>,
  one_way: bool,
  ids: &mut FastMap<ShapeKey, u16>,
  sink: &mut dyn ShapeSink,
) -> TileCell {
  if !one_way && polygons.iter().any(|p| full_cell(p)) {
    return TileCell::Solid;
  }
  if let [polygon] = &polygons[..] {
    if one_way && full_cell(polygon) {
      return TileCell::OneWay;
    }
    if !one_way && let Some(corner) = slope_corner(polygon) {
      return TileCell::Slope(corner);
    }
  }
  shape_of(polygons, one_way, ids, sink)
}

/// `polygons` as a shape's cell: the shape of the same polygons named
/// already, else a new one
fn shape_of(
  polygons: Vec<Vec<Vec2>>,
  one_way: bool,
  ids: &mut FastMap<ShapeKey, u16>,
  sink: &mut dyn ShapeSink,
) -> TileCell {
  let key = ShapeKey::of(&polygons, one_way);
  if let Some(&index) = ids.get(&key) {
    return TileCell::Shape(index);
  }
  let index = sink.add(CellShape { polygons, one_way });
  ids.insert(key, index);
  TileCell::Shape(index)
}

/// The cell kind `tile` is on cells its grid places as `placement`, for
/// `purpose`. Off an orthogonal grid (`one_way_kept` false) a one-way tile
/// collides both ways, noted in `both_ways`. On a hexagonal grid a tile
/// with a polygon that is its cell's whole hexagon, as it stands flipped
/// before its turns, is solid however turned; any other is its shape
fn tile_kind(
  source: Option<&ShapeSource>,
  tile: Tile,
  (placement, one_way_kept): (Placement, bool),
  (purpose, both_ways): (Purpose, &Cell<bool>),
  ids: &mut FastMap<ShapeKey, u16>,
  sink: &mut dyn ShapeSink,
) -> TileCell {
  let Some((source, data)) = source.and_then(|s| Some((s, s.tiles.get(&tile.id)?))) else {
    return TileCell::Empty;
  };
  if let Placement::Nothing = placement {
    return TileCell::Empty;
  }
  // The shapes read: for collision its solid ones, else its one-way ones
  // (those not marked `occluder`); for light those marked, else all
  let marked = data.shapes.iter().any(|s| s.occluder);
  let read = |s: &SourceShape, one_way: bool| match purpose {
    Purpose::Collision => !s.occluder && s.one_way == one_way,
    Purpose::Occlusion => !one_way && s.occluder == marked,
  };
  let one_way_shapes = !data.shapes.iter().any(|s| read(s, false));
  let shapes: Vec<&SourceShape> = data
    .shapes
    .iter()
    .filter(|s| read(s, one_way_shapes))
    .collect();
  if shapes.is_empty() {
    return TileCell::Empty;
  }
  if one_way_shapes && !one_way_kept {
    both_ways.set(true);
  }
  let one_way = one_way_shapes && one_way_kept;
  let polygons: Vec<Vec<Vec2>> = shapes
    .iter()
    .map(|s| placed(data.size, source.offset, tile, &s.points, placement))
    .collect();
  let Placement::Hex(place) = placement else {
    return classify(polygons, one_way, ids, sink);
  };
  let drawn = (data.size, source.offset);
  let whole = shapes
    .iter()
    .any(|s| place.whole(&place.flipped(drawn, tile.flip, &s.points)));
  if whole && !one_way {
    return TileCell::Solid;
  }
  shape_of(polygons, one_way, ids, sink)
}

/// The cell kind an IntGrid value's name gives by default
fn default_kind(name: &str) -> TileCell {
  const NAMES: [(&str, TileCell); 6] = [
    ("solid", TileCell::Solid),
    ("one_way", TileCell::OneWay),
    (
      "slope_bottom_left",
      TileCell::Slope(SlopeCorner::BottomLeft),
    ),
    (
      "slope_bottom_right",
      TileCell::Slope(SlopeCorner::BottomRight),
    ),
    ("slope_top_left", TileCell::Slope(SlopeCorner::TopLeft)),
    ("slope_top_right", TileCell::Slope(SlopeCorner::TopRight)),
  ];
  NAMES
    .iter()
    .find(|(known, _)| known.eq_ignore_ascii_case(name))
    .map_or(TileCell::Empty, |(_, kind)| *kind)
}

/// The cell kind of each IntGrid value: an override by value, else one by
/// name, else the name's default (names compare ignoring ASCII case). An
/// override to a `Shape` names no shape the map has, so it collides as
/// nothing
pub(super) fn intgrid_table(
  names: &FastMap<i32, String>,
  overrides: &[(IntGridKey, TileCell)],
) -> FastMap<i32, TileCell> {
  let usable = |kind: TileCell| match kind {
    TileCell::Shape(_) => TileCell::Empty,
    kind => kind,
  };
  let by_value = |value: i32| {
    overrides
      .iter()
      .find(|(key, _)| *key == IntGridKey::Value(value))
      .map(|(_, kind)| usable(*kind))
  };
  let by_name = |name: &str| {
    overrides
      .iter()
      .find(|(key, _)| matches!(key, IntGridKey::Name(n) if n.eq_ignore_ascii_case(name)))
      .map(|(_, kind)| usable(*kind))
  };
  let mut table: FastMap<i32, TileCell> = names
    .iter()
    .map(|(&value, name)| {
      let kind = by_value(value)
        .or_else(|| by_name(name))
        .unwrap_or_else(|| default_kind(name));
      (value, kind)
    })
    .collect();
  for (key, kind) in overrides {
    if let IntGridKey::Value(value) = key {
      table.entry(*value).or_insert(usable(*kind));
    }
  }
  table
}

/// How strong a cell kind is in a group's union
fn rank(kind: TileCell) -> u8 {
  match kind {
    TileCell::Empty => 0,
    TileCell::OneWay => 1,
    TileCell::Slope(_) => 2,
    TileCell::Shape(_) => 3,
    TileCell::Solid => 4,
  }
}

/// A layer of a group: what it was last read at
#[derive(Debug)]
pub(super) struct Tracked {
  /// Its place among the map's layers, and its entity (a stand-in until
  /// the layers spawn, when `relink` sets it)
  pub(super) index: usize,
  pub(super) entity: Entity,
  /// The grid it was last read from (`TileGrid::id`; 0 before any), and
  /// that grid's version then: while the grid is the same one, the chunks
  /// changed since are those of a later version
  grid_id: u64,
  grid_version: u64,
  /// The version each chunk was read at, for a grid swapped for another,
  /// whose chunks are compared one by one
  versions: FastMap<IVec2, u64>,
  /// The kind of each value, for a layer read by its IntGrid
  intgrid: Option<FastMap<i32, TileCell>>,
}

impl Tracked {
  /// A layer read as its tiles are now
  pub(super) fn new(
    place: (usize, Entity),
    data: &TileLayerData,
    overrides: &[(IntGridKey, TileCell)],
  ) -> Tracked {
    let mut tracked = Tracked::unread(place, data, overrides);
    tracked.read_all(data);
    tracked
  }

  /// A layer not read yet: every chunk counts as changed
  pub(super) fn unread(
    (index, entity): (usize, Entity),
    data: &TileLayerData,
    overrides: &[(IntGridKey, TileCell)],
  ) -> Tracked {
    Tracked {
      index,
      entity,
      grid_id: 0,
      grid_version: 0,
      versions: FastMap::default(),
      intgrid: data
        .intgrid
        .as_ref()
        .map(|intgrid| intgrid_table(&intgrid.names, overrides)),
    }
  }

  /// Points it at its layer's entity, from the map's layer entities
  pub(super) fn relink(&mut self, layers: &[Entity]) {
    if let Some(&entity) = layers.get(self.index) {
      self.entity = entity;
    }
  }

  /// Marks the layer read as `data` is (a copy of it, off the main thread);
  /// returns the chunks a build reads (`build_chunks`)
  pub(super) fn read_all(&mut self, data: &TileLayerData) -> Vec<IVec2> {
    let grid = &data.tiles;
    self.versions.clear();
    self.versions.reserve(grid.chunk_count());
    self.versions.extend(grid.chunk_versions());
    self.grid_id = grid.id();
    self.grid_version = grid.version();
    build_chunks(data)
  }
}

/// The chunks a build of a layer reads: every chunk its grid has and, for
/// an IntGrid layer, every chunk holding cells of its IntGrid that its grid
/// holds (`chunk_kinds` reads an IntGrid's values in any chunk, and a grid
/// a game swapped for a new one has only the chunks its tiles went in), in
/// no set order and possibly twice
pub(super) fn build_chunks(data: &TileLayerData) -> Vec<IVec2> {
  let grid = &data.tiles;
  let mut chunks: Vec<IVec2> = grid.chunk_coords().collect();
  if let Some(intgrid) = &data.intgrid {
    let mut held = CellRect::from_size(intgrid.size);
    if let Some(bounds) = grid.bounds() {
      held = CellRect::new(held.min.max(bounds.min), held.max.min(bounds.max));
    }
    if !held.is_empty() {
      let first = TileGrid::chunk_of(held.min);
      let last = TileGrid::chunk_of(held.max - IVec2::ONE);
      for y in first.y..=last.y {
        chunks.extend((first.x..=last.x).map(|x| IVec2::new(x, y)));
      }
    }
  }
  chunks
}

/// The chunks of the layers of a group whose version moved since each was
/// last read (all of a layer's, when its IntGrid's names now name other
/// kinds), or that its grid no longer has, in row order, each once; the
/// layers are marked read (`changed_in`)
pub(super) fn changed_chunks(
  group: &mut [Tracked],
  layers: &Layers,
  overrides: &[(IntGridKey, TileCell)],
) -> Vec<IVec2> {
  let mut changed: Vec<IVec2> = Vec::new();
  for tracked in group {
    let Ok((_, data, _)) = layers.get(tracked.entity) else {
      continue;
    };
    // An IntGrid's names may have changed with its layer
    let mut all = false;
    if data.is_changed()
      && let (Some(table), Some(intgrid)) = (&mut tracked.intgrid, &data.intgrid)
    {
      let now = intgrid_table(&intgrid.names, overrides);
      if now != *table {
        *table = now;
        all = true;
      }
    }
    changed_in(tracked, &data.tiles, all, &mut changed);
  }
  changed.sort_unstable_by_key(|c| (c.y, c.x));
  changed.dedup();
  changed
}

/// Pushes onto `changed` the chunks of `grid` that changed since `tracked`
/// last read it (every one with `all`), and marks it read. While the grid
/// is the one it read it looks only for later versions; a grid swapped for
/// another has every chunk compared. Either way a chunk it read that the
/// grid no longer has (the old grid's, or one unloaded since) counts as
/// changed, to empty, and is forgotten. Read so, its chunks always hold
/// every chunk of the grid, so it holds more exactly when the grid lost
/// some: unloads need no record on the grid, and each consumer finds them
/// whenever it reads
fn changed_in(tracked: &mut Tracked, grid: &TileGrid, all: bool, changed: &mut Vec<IVec2>) {
  let same = grid.id() == tracked.grid_id;
  if !all && same && grid.version() == tracked.grid_version {
    return;
  }
  for (chunk, version) in grid.chunk_versions() {
    if same && !all && version <= tracked.grid_version {
      continue;
    }
    let before = tracked.versions.insert(chunk, version);
    if all || before != Some(version) {
      changed.push(chunk);
    }
  }
  if tracked.versions.len() > grid.chunk_count() {
    tracked.versions.retain(|&chunk, _| {
      let kept = grid.has_chunk(chunk);
      if !kept {
        changed.push(chunk);
      }
      kept
    });
  }
  tracked.grid_id = grid.id();
  tracked.grid_version = grid.version();
}

/// The tile data of a group's layers, in its order (`None` for one gone)
pub(super) fn layer_data<'w>(
  group: &[Tracked],
  layers: &'w Layers,
) -> Vec<Option<Ref<'w, TileLayerData>>> {
  group
    .iter()
    .map(|t| layers.get(t.entity).ok().map(|(_, data, _)| data))
    .collect()
}

/// What classifying a map's cells reads
pub(super) struct Context<'a> {
  /// The map's tilesets' shapes, by index
  pub(super) shapes: &'a [Option<ShapeSource>],
  pub(super) overrides: &'a [(IntGridKey, TileCell)],
  pub(super) purpose: Purpose,
  /// The map's tilesets' stairs, by index, for the floors' footprints
  /// (`footprint_kinds`); none unless given (`with_stairs`)
  pub(super) stairs: &'a [Option<StairSource>],
  /// Set when a one-way tile or IntGrid cell off an orthogonal grid was
  /// made to collide both ways
  pub(super) both_ways: Cell<bool>,
}

impl<'a> Context<'a> {
  pub(super) fn new(
    shapes: &'a [Option<ShapeSource>],
    overrides: &'a [(IntGridKey, TileCell)],
    purpose: Purpose,
  ) -> Context<'a> {
    Context {
      shapes,
      overrides,
      purpose,
      stairs: &[],
      both_ways: Cell::new(false),
    }
  }

  /// The context reading `stairs`, the map's tilesets' stairs
  pub(super) fn with_stairs(self, stairs: &'a [Option<StairSource>]) -> Context<'a> {
    Context { stairs, ..self }
  }
}

/// The cells of `chunk`'s kinds (a storage chunk of a layer on a grid
/// whose `lattice` reindexes its cells) at their lattice cells, row by row
/// from the chunk's first cell; nothing for a chunk past what an `i32`
/// holds
pub(super) fn scatter<'k>(
  lattice: &'k Lattice,
  chunk: IVec2,
  kinds: &'k ChunkKinds,
) -> impl Iterator<Item = (IVec2, TileCell)> + 'k {
  let origin = chunk_origin(chunk);
  origin.into_iter().flat_map(move |origin| {
    kinds.iter().enumerate().map(move |(i, &kind)| {
      let at = IVec2::new(i as i32 % SIDE, i as i32 / SIDE);
      (lattice.lattice_cell(origin + at), kind)
    })
  })
}

/// The cells of a chunk, row by row
pub(super) type ChunkKinds = [TileCell; CELLS];

/// The cells of a chunk, `CHUNK_SIZE` rows of `CHUNK_SIZE`
pub(super) const CELLS: usize = (CHUNK_SIZE * CHUNK_SIZE) as usize;

/// A chunk's side as a cell coordinate
const SIDE: i32 = CHUNK_SIZE as i32;

/// The part of a chunk a classification reads: its cells from `min`
/// (included) to `max` (excluded), counted from the chunk's top-left
#[derive(Clone, Copy, Debug)]
pub(super) struct Part {
  pub(super) min: IVec2,
  pub(super) max: IVec2,
}

impl Part {
  /// All of a chunk
  pub(super) const WHOLE: Part = Part {
    min: IVec2::ZERO,
    max: IVec2::splat(SIDE),
  };
}

/// The first cell of `chunk`, if `i32` holds it
pub(super) fn chunk_origin(chunk: IVec2) -> Option<IVec2> {
  Some(IVec2::new(
    chunk.x.checked_mul(SIDE)?,
    chunk.y.checked_mul(SIDE)?,
  ))
}

/// Lays the kinds of `part` of `chunk` of a layer, for the context's
/// purpose, over `out` (a chunk's cells: each cell the stronger of what is
/// there and the layer's, the layer's on a tie). A tile layer's cells are
/// read where they lie in the chunk, each tile's kind worked out once
/// (`kinds`, by the cell's value); an IntGrid layer's are its values in the
/// cells both its IntGrid and its grid hold
pub(super) fn chunk_kinds(
  tracked: &Tracked,
  data: &TileLayerData,
  (chunk, part): (IVec2, Part),
  (kinds, shapes): (&mut FastMap<u32, TileCell>, &mut FastMap<ShapeKey, u16>),
  sink: &mut dyn ShapeSink,
  context: &Context<'_>,
  out: &mut ChunkKinds,
) {
  let local = |x: i32, y: i32| (y * SIDE + x) as usize;
  let placement = Placement::of(&data.grid);
  let one_way_kept = keeps_one_way(&data.grid);
  if let Placement::Nothing = placement {
    // No cells to collide or cast shadows on
    return;
  }
  match &tracked.intgrid {
    Some(table) => {
      let (Some(origin), Some(intgrid)) = (chunk_origin(chunk), data.intgrid.as_ref()) else {
        return;
      };
      let mut held = CellRect::from_size(intgrid.size);
      if let Some(bounds) = data.tiles.bounds() {
        held = CellRect::new(held.min.max(bounds.min), held.max.min(bounds.max));
      }
      let lo = origin.saturating_add(part.min).max(held.min);
      let hi = origin.saturating_add(part.max).min(held.max);
      let width = intgrid.size.x as usize;
      for y in lo.y..hi.y {
        let row = y as usize * width;
        for x in lo.x..hi.x {
          let value = intgrid.values.get(row + x as usize).copied().unwrap_or(0);
          if value == 0 {
            continue;
          }
          let kind = match (context.purpose, table.get(&value).copied()) {
            (_, None) | (Purpose::Occlusion, Some(TileCell::OneWay)) => continue,
            (_, Some(TileCell::OneWay)) if !one_way_kept => {
              context.both_ways.set(true);
              TileCell::Solid
            }
            (_, Some(kind)) => kind,
          };
          let slot = &mut out[local(x - origin.x, y - origin.y)];
          *slot = over(*slot, kind);
        }
      }
    }
    None => {
      let Some(cells) = data.tiles.chunk_cells(chunk) else {
        return;
      };
      // The tiles met last, by a hash of their value, in front of the map
      // of every tile's kind: a chunk holds few tiles, often over and over
      let mut met = [(0u32, TileCell::Empty); 32];
      for y in part.min.y..part.max.y {
        for x in part.min.x..part.max.x {
          let at = local(x, y);
          let raw = cells[at];
          if raw == 0 {
            continue;
          }
          let slot = &mut met[(raw.wrapping_mul(0x9E37_79B9) >> 27) as usize];
          let kind = if slot.0 == raw {
            slot.1
          } else {
            let kind = *kinds.entry(raw).or_insert_with(|| {
              let Some(tile) = TileGrid::decode(raw) else {
                return TileCell::Empty;
              };
              let source = context
                .shapes
                .get(usize::from(tile.tileset))
                .and_then(Option::as_ref);
              let purpose = (context.purpose, &context.both_ways);
              let place = (placement, one_way_kept);
              tile_kind(source, tile, place, purpose, shapes, sink)
            });
            *slot = (raw, kind);
            kind
          };
          out[at] = over(out[at], kind);
        }
      }
    }
  }
}

/// The kinds of `part` of `chunk` across a group's layers (`data` in the
/// group's order, `None` for a layer gone), each cell the strongest, the
/// upper layer's on a tie; cells outside `part` are `Empty`
pub(super) fn union_kinds(
  layers: &[Tracked],
  data: &[Option<&TileLayerData>],
  at: (IVec2, Part),
  kinds: (&mut FastMap<u32, TileCell>, &mut FastMap<ShapeKey, u16>),
  sink: &mut dyn ShapeSink,
  context: &Context<'_>,
) -> ChunkKinds {
  let (tile_kinds, shapes) = kinds;
  let mut out = [TileCell::Empty; CELLS];
  for (layer, data) in layers.iter().zip(data) {
    if let Some(data) = data {
      chunk_kinds(
        layer,
        data,
        at,
        (&mut *tile_kinds, &mut *shapes),
        sink,
        context,
        &mut out,
      );
    }
  }
  out
}

/// What a layer gives a floor's footprint (`footprint_kinds`)
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Footing {
  /// Every cell holding a tile: a layer on that floor
  Tiles,
  /// The cells of its tiles that are stairs up to that floor: a lower
  /// floor's layer
  Stairs,
}

/// The cells of `chunk` in floor `floor`'s footprint, across its layers
/// (`data` in the group's order, `None` for a layer gone, each giving what
/// `footing` says): `Solid` where a layer on the floor holds a tile, or a
/// lower floor's layer a tile that is stairs up to it (the context's
/// stairs); every other cell `Empty`. A layer whose grid places no cells
/// (`Placement::Nothing`) gives none
pub(super) fn footprint_kinds(
  data: &[Option<&TileLayerData>],
  footing: &[Footing],
  (chunk, floor): (IVec2, u8),
  context: &Context<'_>,
) -> ChunkKinds {
  let mut out = [TileCell::Empty; CELLS];
  for (data, &footing) in data.iter().zip(footing) {
    let Some(data) = data else {
      continue;
    };
    if let Placement::Nothing = Placement::of(&data.grid) {
      continue;
    }
    let Some(cells) = data.tiles.chunk_cells(chunk) else {
      continue;
    };
    // The cell value met last, and whether it is stairs up to the floor: a
    // chunk holds few tiles, often over and over
    let mut met = (0u32, false);
    for (slot, &raw) in out.iter_mut().zip(cells) {
      if raw == 0 {
        continue;
      }
      let held = match footing {
        Footing::Tiles => true,
        Footing::Stairs if met.0 == raw => met.1,
        Footing::Stairs => {
          let up = TileGrid::decode(raw).and_then(|tile| stairs_of(context.stairs, tile));
          met = (raw, up == Some(floor));
          met.1
        }
      };
      if held {
        *slot = TileCell::Solid;
      }
    }
  }
  out
}

/// `kind` over `below` in a group's union: the stronger, the upper on a tie
pub(super) fn over(below: TileCell, kind: TileCell) -> TileCell {
  if kind != TileCell::Empty && rank(kind) >= rank(below) {
    kind
  } else {
    below
  }
}

#[cfg(test)]
mod tests {
  use glam::{IVec2, UVec2};

  use super::*;

  /// The chunks `tracked` finds changed in `data`'s grid, in row order
  fn read(tracked: &mut Tracked, data: &TileLayerData) -> Vec<IVec2> {
    let mut changed = Vec::new();
    changed_in(tracked, &data.tiles, false, &mut changed);
    changed.sort_unstable_by_key(|c| (c.y, c.x));
    changed
  }

  #[test]
  fn consumers_reading_at_different_times_each_see_an_unloaded_chunk_go_once() {
    // Chunks (-1, 0), (0, 0) and (1, 0), each holding a tile
    let mut data = TileLayerData {
      tiles: TileGrid::unbounded(),
      intgrid: None,
      grid: crate::core::tilemap::MapGrid::new(
        crate::core::tilemap::GridShape::Orthogonal,
        UVec2::splat(16),
      ),
    };
    for x in [-5, 4, 40] {
      data.tiles.set(IVec2::new(x, 3), Some(Tile::new(0, 1)));
    }
    let (a, b, c) = (IVec2::new(-1, 0), IVec2::ZERO, IVec2::new(1, 0));
    let place = (0, Entity::PLACEHOLDER);
    let mut early = Tracked::new(place, &data, &[]);
    let mut late = Tracked::new(place, &data, &[]);

    // One reads just after chunk A is unloaded: changed (to empty), once,
    // then forgotten
    assert!(data.tiles.unload_chunk(a).is_some());
    assert_eq!(read(&mut early, &data), [a]);
    assert_eq!(read(&mut early, &data), []);
    assert!(!early.versions.contains_key(&a));

    // Before the other reads, C is edited and B unloaded and loaded back
    data.tiles.set(IVec2::new(41, 3), Some(Tile::new(0, 2)));
    let Some(cells) = data.tiles.unload_chunk(b) else {
      panic!("chunk {b} is there");
    };
    assert!(data.tiles.load_chunk(b, cells));
    // The late one sees A gone, B and C changed, each once
    assert_eq!(read(&mut late, &data), [a, b, c]);
    assert_eq!(read(&mut late, &data), []);
    assert_eq!(late.versions.len(), 2, "A forgotten, B kept");
    // and the early one sees B and C, never A again
    assert_eq!(read(&mut early, &data), [b, c]);
    assert_eq!(early.versions.len(), 2);

    // A chunk unloaded and loaded back between two reads is changed, kept
    let Some(cells) = data.tiles.unload_chunk(c) else {
      panic!("chunk {c} is there");
    };
    assert!(data.tiles.load_chunk(c, cells));
    assert_eq!(read(&mut early, &data), [c]);
    assert!(early.versions.contains_key(&c));
  }
}
