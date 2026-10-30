//! What a map's cell is, for collision or for light: tilesets read once
//! into `ShapeSource`s (so another thread can read them), a tile's shapes
//! placed where it draws and classified as a `TileCell`, IntGrid values by
//! their names, a group of layers' union, and which chunks changed since a
//! group last read its layers. Pure but for the queries' aliases, so a
//! map's first build can run off the main thread.

use std::sync::Arc;

use glam::{IVec2, UVec2, Vec2};

use super::spawn::{IntGridKey, TileLayerData};
use super::{
  LayerInfo, LdtkProject, Tile, TileFlip, TileMap, Tileset, TilesetImage, TilesetSource,
};
use crate::core::asset::{AssetId, Assets};
use crate::core::ecs::{Allow, DetectChanges, Disabled, Entity, Has, Query, Ref, Res};
use crate::core::hash::FastMap;
use crate::core::physics::{Boundary, CellShape, SlopeCorner, TileCell, TileColliders};

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

/// Whether what was built from `shapes` still stands for `map`'s tilesets:
/// their stamps match, or they moved (an image reloaded, or a tileset saved
/// with other tile data) and the tilesets give the same shapes, so the
/// stamps are brought up to date and nothing is built again
pub(super) fn shapes_kept(
  stamps: &mut Vec<TilesetStamp>,
  shapes: &[Option<ShapeSource>],
  map: &TileMap,
  assets: Option<&Assets<Tileset>>,
) -> bool {
  if stamps_match(stamps, map, assets) {
    return true;
  }
  if *sources(map, assets) != *shapes {
    return false;
  }
  *stamps = self::stamps(map, assets);
  true
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

/// `points` (tile pixels from the tile image's top-left, y down) where the
/// tile draws in its cell, in tiles from the cell's top-left: the tile
/// stands on the cell's bottom edge from the left however large it is, the
/// tileset's offset moves it, and its flips (diagonal first, then
/// horizontal, then vertical, as Tiled composes them) turn it, as
/// `render::tiles` draws it. `size` is the size the tile draws at and
/// `offset` its tileset's
fn placed(size: Vec2, offset: Vec2, tile: Tile, points: &[Vec2], cell: Vec2) -> Vec<Vec2> {
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
      let px = Vec2::new(at.x + offset.x, cell.y - drawn.y + at.y + offset.y);
      px / cell
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
  let key = ShapeKey::of(&polygons, one_way);
  if let Some(&index) = ids.get(&key) {
    return TileCell::Shape(index);
  }
  let index = sink.add(CellShape { polygons, one_way });
  ids.insert(key, index);
  TileCell::Shape(index)
}

/// The cell kind `tile` is on cells of `cell` pixels, for `purpose`
fn tile_kind(
  source: Option<&ShapeSource>,
  tile: Tile,
  cell: Vec2,
  purpose: Purpose,
  ids: &mut FastMap<ShapeKey, u16>,
  sink: &mut dyn ShapeSink,
) -> TileCell {
  let Some((source, data)) = source.and_then(|s| Some((s, s.tiles.get(&tile.id)?))) else {
    return TileCell::Empty;
  };
  let collected = |keep: &dyn Fn(&SourceShape) -> bool| -> Vec<Vec<Vec2>> {
    data
      .shapes
      .iter()
      .filter(|s| keep(s))
      .map(|s| placed(data.size, source.offset, tile, &s.points, cell))
      .collect()
  };
  let (solid, one_way) = match purpose {
    Purpose::Collision => (
      collected(&|s| !s.occluder && !s.one_way),
      collected(&|s| !s.occluder && s.one_way),
    ),
    Purpose::Occlusion => {
      let marked = data.shapes.iter().any(|s| s.occluder);
      (collected(&|s| s.occluder == marked), Vec::new())
    }
  };
  if !solid.is_empty() {
    return classify(solid, false, ids, sink);
  }
  if !one_way.is_empty() {
    return classify(one_way, true, ids, sink);
  }
  TileCell::Empty
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

/// A layer of a group: the chunk versions it was last read at
#[derive(Debug)]
pub(super) struct Tracked {
  /// Its place among the map's layers, and its entity (a stand-in until
  /// the layers spawn, when `relink` sets it)
  pub(super) index: usize,
  pub(super) entity: Entity,
  versions: Vec<u64>,
  grid_version: u64,
  /// The kind of each value, for a layer read by its IntGrid
  intgrid: Option<FastMap<i32, TileCell>>,
}

impl Tracked {
  /// A layer read as its tiles are now
  pub(super) fn new(
    (index, entity): (usize, Entity),
    data: &TileLayerData,
    overrides: &[(IntGridKey, TileCell)],
  ) -> Tracked {
    let grid = &data.grid;
    let chunks = grid.chunks();
    let versions = (0..chunks.y)
      .flat_map(|y| (0..chunks.x).map(move |x| UVec2::new(x, y)))
      .map(|chunk| grid.chunk_version(chunk))
      .collect();
    Tracked {
      index,
      entity,
      versions,
      grid_version: grid.version(),
      intgrid: data
        .intgrid
        .as_ref()
        .map(|intgrid| intgrid_table(&intgrid.names, overrides)),
    }
  }

  /// A layer not read yet: every chunk that ever changed counts as changed
  pub(super) fn unread(
    (index, entity): (usize, Entity),
    data: &TileLayerData,
    overrides: &[(IntGridKey, TileCell)],
  ) -> Tracked {
    let chunks = data.grid.chunks();
    Tracked {
      index,
      entity,
      versions: vec![0; chunks.x as usize * chunks.y as usize],
      grid_version: 0,
      intgrid: data
        .intgrid
        .as_ref()
        .map(|intgrid| intgrid_table(&intgrid.names, overrides)),
    }
  }
}

impl Tracked {
  /// Points it at its layer's entity, from the map's layer entities
  pub(super) fn relink(&mut self, layers: &[Entity]) {
    if let Some(&entity) = layers.get(self.index) {
      self.entity = entity;
    }
  }

  /// Marks the layer read as `data` is (a copy of it, off the main thread);
  /// returns every chunk of it that ever changed, which a first build reads
  pub(super) fn read_all(&mut self, data: &TileLayerData) -> Vec<UVec2> {
    let grid = &data.grid;
    let chunks = grid.chunks();
    self.versions.clear();
    let mut changed = Vec::new();
    for y in 0..chunks.y {
      for x in 0..chunks.x {
        let version = grid.chunk_version(UVec2::new(x, y));
        self.versions.push(version);
        if version != 0 {
          changed.push(UVec2::new(x, y));
        }
      }
    }
    self.grid_version = grid.version();
    changed
  }
}

/// The chunks of the layers of a group whose version moved since each was
/// last read (all of a layer's, when its IntGrid's names now name other
/// kinds), sorted, each once; the layers are marked read
pub(super) fn changed_chunks(
  group: &mut [Tracked],
  layers: &Layers,
  overrides: &[(IntGridKey, TileCell)],
) -> Vec<UVec2> {
  let mut changed: Vec<UVec2> = Vec::new();
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
    let grid = &data.grid;
    if !all && grid.version() == tracked.grid_version {
      continue;
    }
    let chunks = grid.chunks();
    tracked
      .versions
      .resize(chunks.x as usize * chunks.y as usize, 0);
    for y in 0..chunks.y {
      for x in 0..chunks.x {
        let chunk = UVec2::new(x, y);
        let version = grid.chunk_version(chunk);
        let seen = &mut tracked.versions[(y * chunks.x + x) as usize];
        if all || *seen != version {
          *seen = version;
          changed.push(chunk);
        }
      }
    }
    tracked.grid_version = grid.version();
  }
  changed.sort_unstable_by_key(|c| (c.y, c.x));
  changed.dedup();
  changed
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
}

/// The kind `cell` of a layer is, for the context's purpose
pub(super) fn layer_kind(
  tracked: &Tracked,
  data: &TileLayerData,
  cell: IVec2,
  (kinds, shapes): (&mut FastMap<Tile, TileCell>, &mut FastMap<ShapeKey, u16>),
  sink: &mut dyn ShapeSink,
  context: &Context<'_>,
) -> TileCell {
  match &tracked.intgrid {
    Some(table) => {
      let kind = data
        .intgrid
        .as_ref()
        .and_then(|intgrid| intgrid.get(cell))
        .and_then(|value| table.get(&value).copied())
        .unwrap_or(TileCell::Empty);
      match (context.purpose, kind) {
        (Purpose::Occlusion, TileCell::OneWay) => TileCell::Empty,
        (_, kind) => kind,
      }
    }
    None => match data.grid.get(cell) {
      None => TileCell::Empty,
      Some(tile) => *kinds.entry(tile).or_insert_with(|| {
        let source = context
          .shapes
          .get(usize::from(tile.tileset))
          .and_then(Option::as_ref);
        tile_kind(
          source,
          tile,
          data.cell_size.as_vec2(),
          context.purpose,
          shapes,
          sink,
        )
      }),
    },
  }
}

/// `kind` over `below` in a group's union: the stronger, the upper on a tie
pub(super) fn over(below: TileCell, kind: TileCell) -> TileCell {
  if kind != TileCell::Empty && rank(kind) >= rank(below) {
    kind
  } else {
    below
  }
}
