//! Map collision: a spawned map's colliding tile layers become
//! `TileColliders`, kept in step with their tiles.
//!
//! A tile layer collides when its `LayerInfo::collides` is set and its
//! root has not turned collision off. Colliding layers are grouped by their
//! cell size, offset and collision material (`collision_layer`,
//! `collision_mask`, `friction`, `restitution`: a layer's properties over
//! its map's): the group of the map's own tile size, no offset and the
//! map's material unions into one `TileColliders` on the map entity, and
//! any other group (an LDtk IntGrid of 8 px cells under 16 px art, a layer
//! shifted by an offset, an ice layer) gets its own on its first layer's
//! entity, whose `Transform` carries the offset. A cell takes the strongest kind across its group:
//! `Solid`, then `Shape`, then `Slope`, then `OneWay`; of two shapes (or
//! two slopes) the upper layer's wins.
//!
//! A layer with an IntGrid collides by its values' names (`intgrid_table`);
//! any other by its tiles' collision shapes (`TileShape`s not marked
//! `occluder`), placed where the tile draws (`placed`: a tile larger than
//! its cell stands on the cell's bottom-left, the tileset's `tile_offset`
//! moves it, flips turn it) and classified (`classify`): one full-cell
//! rectangle is `Solid` (`OneWay` if it is one-way), a half-cell right
//! triangle on the cell's diagonal is `Slope`, anything else a `Shape`,
//! one per distinct geometry. A tile with both solid and one-way shapes
//! collides with its solid ones.
//!
//! Each step, every chunk whose version moved since the group last
//! looked is classified again and only the cells whose kind changed are
//! set, so the tile colliders rebuild only the outlines an edit touched. A
//! change of what the colliders are built from (which layers collide, their
//! cell sizes, offsets or sizes, the tilesets' content, the IntGrid
//! overrides) builds every group again.

use std::sync::Arc;

use glam::{IVec2, UVec2, Vec2};

use super::spawn::{IntGridKey, MapLayers, MapRoot, TileLayerData, root_map};
use super::{
  CHUNK_SIZE, LayerInfo, LdtkProject, Properties, PropertyValue, Tile, TileFlip, TileMap,
  TileShape, Tileset, TilesetImage, TilesetSource,
};
use crate::core::asset::{AssetId, Assets};
use crate::core::ecs::{
  Allow, Commands, Component, DetectChanges, Disabled, Entity, Has, Local, Query, Ref, Res, Without,
};
use crate::core::hash::{FastMap, FastSet};
use crate::core::physics::{CellShape, SlopeCorner, TileCell, TileColliders};

/// The most cells a `TileColliders` holds a side
const MAX_SIDE: u32 = 8192;

/// How near a point must be to a cell corner to count as on it, in tiles
const EPSILON: f32 = 1e-4;

/// Shape points are compared on this grid (a tile's points), so two tiles
/// drawing the same shape share it
const KEY_GRID: f32 = 46080.0;

/// What a group of layers shares: a cell size, an offset and a material
/// (floats by their bits)
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct GroupKey {
  cell: UVec2,
  offset: [u32; 2],
  material: [u32; 4],
}

impl GroupKey {
  fn of(cell: UVec2, offset: Vec2, material: Material) -> GroupKey {
    GroupKey {
      cell,
      offset: [offset.x.to_bits(), offset.y.to_bits()],
      material: [
        material.layer,
        material.mask,
        material.friction.to_bits(),
        material.restitution.to_bits(),
      ],
    }
  }

  fn material(&self) -> Material {
    Material {
      layer: self.material[0],
      mask: self.material[1],
      friction: f32::from_bits(self.material[2]),
      restitution: f32::from_bits(self.material[3]),
    }
  }
}

/// A colliding layer as the colliders were built from it
#[derive(Clone, PartialEq, Debug)]
pub(crate) struct Planned {
  entity: Entity,
  key: GroupKey,
  size: UVec2,
  intgrid: bool,
}

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

/// Tileset `index` of a map
pub(super) fn tileset_of<'a>(
  sources: &'a [TilesetSource],
  index: u16,
  assets: Option<&'a Assets<Tileset>>,
) -> Option<&'a Tileset> {
  match sources.get(usize::from(index))? {
    TilesetSource::Asset(handle) => assets?.get(handle),
    TilesetSource::Embedded(tileset) => Some(tileset),
  }
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
/// `render::tiles` draws it
pub(super) fn placed(tileset: &Tileset, tile: Tile, points: &[Vec2], cell: Vec2) -> Vec<Vec2> {
  let size = drawn_size(tileset, tile.id);
  let diagonal = tile.flip.contains(TileFlip::DIAGONAL);
  let drawn = if diagonal {
    Vec2::new(size.y, size.x)
  } else {
    size
  };
  let offset = tileset.tile_offset.as_vec2();
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
struct ShapeKey {
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
trait ShapeSink {
  fn add(&mut self, shape: CellShape) -> u16;
}

/// A grid's shapes before the grid exists, numbered as `add_shape` will
struct Pending(Vec<CellShape>);

impl ShapeSink for Pending {
  fn add(&mut self, shape: CellShape) -> u16 {
    // `add_shape` names none past 65,535 (and logs it)
    let index = u16::try_from(self.0.len()).unwrap_or(u16::MAX);
    self.0.push(shape);
    index
  }
}

impl ShapeSink for TileColliders {
  fn add(&mut self, shape: CellShape) -> u16 {
    self.add_shape(shape)
  }
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

/// The cell kind `tile` collides as on cells of `cell` pixels
fn tile_kind(
  tileset: Option<&Tileset>,
  tile: Tile,
  cell: Vec2,
  ids: &mut FastMap<ShapeKey, u16>,
  sink: &mut dyn ShapeSink,
) -> TileCell {
  let Some((tileset, data)) = tileset.and_then(|t| Some((t, t.tile(tile.id)?))) else {
    return TileCell::Empty;
  };
  let collected = |one_way: bool| -> Vec<Vec<Vec2>> {
    data
      .shapes
      .iter()
      .filter(|s: &&TileShape| !s.occluder && s.one_way == one_way)
      .map(|s| placed(tileset, tile, &s.points, cell))
      .collect()
  };
  let solid = collected(false);
  if !solid.is_empty() {
    return classify(solid, false, ids, sink);
  }
  let one_way = collected(true);
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
struct Tracked {
  entity: Entity,
  versions: Vec<u64>,
  grid_version: u64,
  /// The kind of each value, for a layer that collides by its IntGrid
  intgrid: Option<FastMap<i32, TileCell>>,
}

impl Tracked {
  fn new(entity: Entity, data: &TileLayerData, overrides: &[(IntGridKey, TileCell)]) -> Tracked {
    let grid = &data.grid;
    let chunks = grid.chunks();
    let versions = (0..chunks.y)
      .flat_map(|y| (0..chunks.x).map(move |x| UVec2::new(x, y)))
      .map(|chunk| grid.chunk_version(chunk))
      .collect();
    Tracked {
      entity,
      versions,
      grid_version: grid.version(),
      intgrid: data
        .intgrid
        .as_ref()
        .map(|intgrid| intgrid_table(&intgrid.names, overrides)),
    }
  }
}

/// Layers sharing a cell size and offset, and the grid they collide in
#[derive(Debug)]
struct Group {
  key: GroupKey,
  /// Where its `TileColliders` is
  target: Entity,
  size: UVec2,
  /// Over the most a `TileColliders` holds: it has none
  too_large: bool,
  layers: Vec<Tracked>,
  /// Each tile's kind, its shapes numbered in this group's grid
  kinds: FastMap<Tile, TileCell>,
  shapes: FastMap<ShapeKey, u16>,
}

/// The collision material properties give
#[derive(Clone, Copy, PartialEq, Debug)]
struct Material {
  layer: u32,
  mask: u32,
  friction: f32,
  restitution: f32,
}

impl Material {
  /// `TileColliders`' own defaults
  const DEFAULT: Material = Material {
    layer: 1,
    mask: u32::MAX,
    friction: 0.5,
    restitution: 0.0,
  };

  /// From `collision_layer`, `collision_mask`, `friction` and
  /// `restitution` in `properties`, each over the same in `under` (a
  /// layer's over its map's); a property of the wrong type is left out
  /// (what is under it counts) and `warn` hears of it
  fn of(
    properties: &Properties,
    under: Material,
    mut warn: impl FnMut(&'static str, &PropertyValue, &'static str),
  ) -> Material {
    let mut bits = |name: &'static str, under: u32| match properties.get(name) {
      None => under,
      Some(value) => match properties.int(name) {
        // A mask of every bit is -1 in a 32-bit int
        Some(int) if (i64::from(i32::MIN)..=i64::from(u32::MAX)).contains(&int) => int as u32,
        _ => {
          warn(name, value, "a whole number of 32 bits");
          under
        }
      },
    };
    let layer = bits("collision_layer", under.layer);
    let mask = bits("collision_mask", under.mask);
    let mut float = |name: &'static str, under: f32| match properties.get(name) {
      None => under,
      Some(value) => match properties.float(name) {
        Some(f) if f.is_finite() => f as f32,
        _ => {
          warn(name, value, "a number");
          under
        }
      },
    };
    let friction = float("friction", under.friction);
    let restitution = float("restitution", under.restitution);
    Material {
      layer,
      mask,
      friction,
      restitution,
    }
  }

  fn apply(&self, tiles: &mut TileColliders) {
    tiles.layer = self.layer;
    tiles.mask = self.mask;
    tiles.friction = self.friction;
    tiles.restitution = self.restitution;
  }
}

/// A map's collision: what it was built from, its groups, and what it has
/// warned about. On the map entity
#[derive(Component, Debug)]
pub(crate) struct MapCollision {
  layers: Vec<Planned>,
  tile_size: UVec2,
  tilesets: Vec<TilesetStamp>,
  overrides: Vec<(IntGridKey, TileCell)>,
  groups: Vec<Group>,
  /// Its colliders were taken away (the map disabled, or one went missing):
  /// the next step builds them all
  torn: bool,
  warned_layers: FastSet<Entity>,
  /// Properties of the wrong type warned about: the layer's (none for the
  /// map's) and the name
  warned_properties: FastSet<(Option<Entity>, &'static str)>,
}

impl MapCollision {
  /// The entities holding a `TileColliders` of this map's
  fn targets(&self) -> impl Iterator<Item = Entity> + '_ {
    self
      .groups
      .iter()
      .filter(|g| !g.too_large)
      .map(|g| g.target)
  }

  /// Takes every `TileColliders` this map put on
  fn tear_down(&mut self, commands: &mut Commands) {
    for target in self.targets() {
      commands.entity(target).try_remove::<TileColliders>();
    }
    self.torn = true;
  }
}

/// The asset stores a map's collision reads
type Stores<'w> = (
  Option<Res<'w, Assets<TileMap>>>,
  Option<Res<'w, Assets<LdtkProject>>>,
  Option<Res<'w, Assets<Tileset>>>,
);

type Maps<'w, 's> = Query<
  'w,
  's,
  (
    Entity,
    &'static MapRoot,
    Option<&'static MapLayers>,
    Option<&'static mut MapCollision>,
    Has<Disabled>,
  ),
  Allow<Disabled>,
>;
type Unrooted<'w, 's> =
  Query<'w, 's, (Entity, &'static mut MapCollision), (Without<MapRoot>, Allow<Disabled>)>;
type Layers<'w, 's> = Query<
  'w,
  's,
  (
    &'static LayerInfo,
    Ref<'static, TileLayerData>,
    Has<Disabled>,
  ),
  Allow<Disabled>,
>;
type Grids<'w, 's> = Query<'w, 's, &'static mut TileColliders, Allow<Disabled>>;

/// What classifying a map's cells reads
struct Context<'a> {
  tile_map: &'a TileMap,
  tilesets: Option<&'a Assets<Tileset>>,
  overrides: &'a [(IntGridKey, TileCell)],
}

/// The kind `cell` of a layer collides as
fn layer_kind(
  tracked: &Tracked,
  data: &TileLayerData,
  cell: IVec2,
  (kinds, shapes): (&mut FastMap<Tile, TileCell>, &mut FastMap<ShapeKey, u16>),
  sink: &mut dyn ShapeSink,
  context: &Context<'_>,
) -> TileCell {
  match &tracked.intgrid {
    Some(table) => data
      .intgrid
      .as_ref()
      .and_then(|intgrid| intgrid.get(cell))
      .and_then(|value| table.get(&value).copied())
      .unwrap_or(TileCell::Empty),
    None => match data.grid.get(cell) {
      None => TileCell::Empty,
      Some(tile) => *kinds.entry(tile).or_insert_with(|| {
        let tileset = tileset_of(&context.tile_map.tilesets, tile.tileset, context.tilesets);
        tile_kind(tileset, tile, data.cell_size.as_vec2(), shapes, sink)
      }),
    },
  }
}

/// `kind` over `below` in a group's union: the stronger, the upper on a tie
fn over(below: TileCell, kind: TileCell) -> TileCell {
  if kind != TileCell::Empty && rank(kind) >= rank(below) {
    kind
  } else {
    below
  }
}

/// Every group of a map's colliding layers, built from their tiles as they
/// are, with the grid each puts on its target (none for a group too large)
fn build(
  map: Entity,
  context: &Context<'_>,
  planned: &[Planned],
  layers: &Layers,
  map_material: Material,
  mut warned_layers: FastSet<Entity>,
) -> (MapCollision, Vec<(Entity, TileColliders)>) {
  let map_key = GroupKey::of(context.tile_map.tile_size, Vec2::ZERO, map_material);
  let mut groups: Vec<Group> = Vec::new();
  for layer in planned {
    let Ok((_, data, _)) = layers.get(layer.entity) else {
      continue;
    };
    let tracked = Tracked::new(layer.entity, &data, context.overrides);
    match groups.iter_mut().find(|g| g.key == layer.key) {
      Some(group) => {
        group.size = group.size.max(layer.size);
        group.layers.push(tracked);
      }
      None => groups.push(Group {
        key: layer.key,
        target: if layer.key == map_key {
          map
        } else {
          layer.entity
        },
        size: layer.size,
        too_large: false,
        layers: vec![tracked],
        kinds: FastMap::default(),
        shapes: FastMap::default(),
      }),
    }
  }
  let mut grids = Vec::new();
  for group in &mut groups {
    let size = group.size;
    if size.x > MAX_SIDE || size.y > MAX_SIDE {
      group.too_large = true;
      let first = group.layers.first().map(|t| t.entity);
      if first.is_some_and(|first| warned_layers.insert(first)) {
        let names: Vec<String> = group
          .layers
          .iter()
          .filter_map(|t| layers.get(t.entity).ok())
          .map(|(info, ..)| format!("\"{}\"", info.name))
          .collect();
        let named = if names.len() == 1 { "layer" } else { "layers" };
        crate::engine_warn!(
          module: "Tilemap",
          "map {map}: {named} {} would collide in a grid of {} x {} cells, over the {MAX_SIDE} a side tile colliders hold, so nothing there collides",
          names.join(", "),
          size.x,
          size.y
        );
      }
      continue;
    }
    let mut cells = vec![TileCell::Empty; size.x as usize * size.y as usize];
    let mut pending = Pending(Vec::new());
    let Group {
      layers: tracked,
      kinds,
      shapes,
      ..
    } = group;
    for layer in tracked.iter() {
      let Ok((_, data, _)) = layers.get(layer.entity) else {
        continue;
      };
      let reach = data.grid.size().min(size);
      for y in 0..reach.y {
        for x in 0..reach.x {
          let cell = IVec2::new(x as i32, y as i32);
          let kind = layer_kind(layer, &data, cell, (kinds, shapes), &mut pending, context);
          let slot = &mut cells[y as usize * size.x as usize + x as usize];
          *slot = over(*slot, kind);
        }
      }
    }
    let mut tiles = TileColliders::from_cells(size.x, size.y, group.key.cell.as_vec2(), cells);
    group.key.material().apply(&mut tiles);
    for shape in pending.0 {
      tiles.add_shape(shape);
    }
    grids.push((group.target, tiles));
  }
  let state = MapCollision {
    layers: planned.to_vec(),
    tile_size: context.tile_map.tile_size,
    tilesets: stamps(context.tile_map, context.tilesets),
    overrides: context.overrides.to_vec(),
    groups,
    torn: false,
    warned_layers,
    warned_properties: FastSet::default(),
  };
  (state, grids)
}

/// Brings a group's grid up to its layers' tiles: the cells of every chunk
/// whose version moved since it last looked, setting only those whose kind
/// changed
fn update(group: &mut Group, tiles: &mut TileColliders, layers: &Layers, context: &Context<'_>) {
  let mut changed: Vec<UVec2> = Vec::new();
  for tracked in &mut group.layers {
    let Ok((_, data, _)) = layers.get(tracked.entity) else {
      continue;
    };
    // An IntGrid's names may have changed with its layer: its cells all
    // count as changed if what they name did
    let mut all = false;
    if data.is_changed()
      && let (Some(table), Some(intgrid)) = (&mut tracked.intgrid, &data.intgrid)
    {
      let now = intgrid_table(&intgrid.names, context.overrides);
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
  if changed.is_empty() {
    return;
  }
  changed.sort_unstable_by_key(|c| (c.y, c.x));
  changed.dedup();
  let data: Vec<Option<Ref<TileLayerData>>> = group
    .layers
    .iter()
    .map(|t| layers.get(t.entity).ok().map(|(_, data, _)| data))
    .collect();
  let Group {
    layers: tracked,
    kinds,
    shapes,
    size,
    ..
  } = group;
  for chunk in changed {
    let min = chunk * CHUNK_SIZE;
    let max = (min + UVec2::splat(CHUNK_SIZE)).min(*size);
    for y in min.y..max.y {
      for x in min.x..max.x {
        let cell = IVec2::new(x as i32, y as i32);
        let mut kind = TileCell::Empty;
        for (layer, data) in tracked.iter().zip(&data) {
          if let Some(data) = data {
            let below = kind;
            kind = over(
              below,
              layer_kind(layer, data, cell, (kinds, shapes), tiles, context),
            );
          }
        }
        if tiles.get(x, y) != kind {
          tiles.set(x, y, kind);
        }
      }
    }
  }
}

/// `FixedSet::Movement`, before the tile colliders rebuild: every spawned
/// map's colliding layers into its `TileColliders`, as their tiles are now.
/// A disabled map has none (enabled, it builds them from the tiles as they
/// then are); a map whose root went takes its colliders away. An idle step
/// costs a look at each map's colliding layers, tilesets and properties
pub(crate) fn sync_map_colliders(
  mut maps: Maps,
  mut unrooted: Unrooted,
  layers: Layers,
  mut grids: Grids,
  (tile_maps, projects, tilesets): Stores,
  mut planned: Local<Vec<Planned>>,
  mut commands: Commands,
) {
  let tilesets = tilesets.as_deref();
  for (map, mut state) in &mut unrooted {
    state.tear_down(&mut commands);
    commands.entity(map).remove::<MapCollision>();
  }
  for (map, root, map_layers, mut state, disabled) in &mut maps {
    let Some(map_layers) = map_layers else {
      // The root was put over or taken away, and its layers with it
      if let Some(mut state) = state {
        state.tear_down(&mut commands);
        commands.entity(map).remove::<MapCollision>();
      }
      continue;
    };
    if disabled {
      if let Some(mut state) = state.filter(|s| !s.torn) {
        state.tear_down(&mut commands);
      }
      continue;
    }
    let Some(tile_map) = root_map(root, tile_maps.as_deref(), projects.as_deref()) else {
      continue;
    };
    // A property of the wrong type warns once a map
    let mut first_warnings = FastSet::default();
    let warned = match state.as_mut() {
      Some(state) => &mut state.warned_properties,
      None => &mut first_warnings,
    };
    let mut warn = |layer: Option<(Entity, &str)>,
                    name: &'static str,
                    value: &PropertyValue,
                    wanted: &str| {
      if warned.insert((layer.map(|(entity, _)| entity), name)) {
        let whose = match layer {
          Some((_, layer)) => format!("layer \"{layer}\""),
          None => "the map".to_string(),
        };
        crate::engine_warn!(
          module: "Tilemap",
          "map {map}: {whose}'s property {name} is {value:?}, not {wanted}; its colliders take what is under it (the map's, else the default)"
        );
      }
    };
    let map_material = Material::of(
      &tile_map.properties,
      Material::DEFAULT,
      |name, value, wanted| warn(None, name, value, wanted),
    );
    planned.clear();
    if root.collision {
      for &entity in &map_layers.0 {
        let Ok((info, data, layer_disabled)) = layers.get(entity) else {
          continue;
        };
        if info.collides && !layer_disabled {
          let material = Material::of(&info.properties, map_material, |name, value, wanted| {
            warn(Some((entity, &info.name)), name, value, wanted)
          });
          planned.push(Planned {
            entity,
            key: GroupKey::of(data.cell_size, info.offset, material),
            size: data.grid.size(),
            intgrid: data.intgrid.is_some(),
          });
        }
      }
    }
    let context = Context {
      tile_map,
      tilesets,
      overrides: &root.intgrid_collision,
    };
    let same = state.as_ref().is_some_and(|s| {
      !s.torn
        && s.layers == *planned
        && s.tile_size == tile_map.tile_size
        && s.overrides == root.intgrid_collision
        && stamps_match(&s.tilesets, tile_map, tilesets)
    });
    match state {
      Some(mut state) if same => {
        let state = &mut *state;
        for group in &mut state.groups {
          if group.too_large {
            continue;
          }
          let Ok(mut tiles) = grids.get_mut(group.target) else {
            // Taken away by something else: all of it is built next step
            state.torn = true;
            continue;
          };
          update(group, &mut tiles, &layers, &context);
        }
      }
      state => {
        let (warned_layers, warned_properties) = match &state {
          Some(old) => (old.warned_layers.clone(), old.warned_properties.clone()),
          None => (FastSet::default(), first_warnings),
        };
        let (mut fresh, built) = build(
          map,
          &context,
          &planned,
          &layers,
          map_material,
          warned_layers,
        );
        fresh.warned_properties = warned_properties;
        if let Some(old) = &state {
          for target in old.targets() {
            if !built.iter().any(|(t, _)| *t == target) {
              commands.entity(target).try_remove::<TileColliders>();
            }
          }
        }
        for (target, tiles) in built {
          commands.entity(target).try_insert(tiles);
        }
        match state {
          Some(mut state) => *state = fresh,
          None => {
            commands.entity(map).insert(fresh);
          }
        }
      }
    }
  }
}

#[cfg(test)]
mod tests {
  use std::sync::{Arc, Mutex};

  use glam::{IVec2, UVec2, Vec2};

  use crate::core::asset::{Assets, AssetsView, Handle};
  use crate::core::ecs::Transform;
  use crate::core::ecs::{ChildOf, Disabled, Entity, World};
  use crate::core::engine::EngineBuilder;
  use crate::core::gameplay::{MoveIntent, PlatformerMovement};
  use crate::core::physics::CharacterState;
  use crate::core::physics::{
    Collider, ColliderShape, SlopeCorner, TileCell, TileColliders, TilePart,
  };
  use crate::core::scene::{Scene, SceneContext};
  use crate::core::testing::HeadlessEngine;
  use crate::core::tilemap::test_util::{MapWorld, files_with};
  use crate::core::tilemap::{
    IntGrid, LayerKind, MapLayerRef, MapRoot, MapsView, PropertyValue, Tile, TileData, TileFlip,
    TileMap, TileShape, Tileset, TilesetImage, TilesetSource,
  };

  /// Tile ids of `shapes`
  const SOLID: u32 = 0;
  const SLOPE: u32 = 1;
  const LIGHT_ONLY: u32 = 2;
  const HALF: u32 = 3;
  const PLATFORM: u32 = 4;

  fn rect(x: f32, y: f32, w: f32, h: f32) -> Vec<Vec2> {
    vec![
      Vec2::new(x, y),
      Vec2::new(x + w, y),
      Vec2::new(x + w, y + h),
      Vec2::new(x, y + h),
    ]
  }

  fn shape(points: Vec<Vec2>, occluder: bool) -> TileShape {
    TileShape {
      points,
      occluder,
      ..TileShape::default()
    }
  }

  /// 16 px tiles: a solid block, a slope filled at its bottom right, a
  /// shape that only blocks light, and a half block
  fn shapes() -> Tileset {
    let mut set = Tileset::new("shapes", UVec2::splat(16), TilesetImage::Collection);
    let with = |shapes: Vec<TileShape>| TileData {
      shapes,
      ..TileData::default()
    };
    set.insert_tile(SOLID, with(vec![shape(rect(0.0, 0.0, 16.0, 16.0), false)]));
    let slope = vec![
      Vec2::new(16.0, 0.0),
      Vec2::new(16.0, 16.0),
      Vec2::new(0.0, 16.0),
    ];
    set.insert_tile(SLOPE, with(vec![shape(slope, false)]));
    set.insert_tile(
      LIGHT_ONLY,
      with(vec![shape(rect(4.0, 2.0, 8.0, 12.0), true)]),
    );
    set.insert_tile(HALF, with(vec![shape(rect(0.0, 8.0, 16.0, 8.0), false)]));
    let platform = TileShape {
      one_way: true,
      ..shape(rect(0.0, 0.0, 16.0, 16.0), false)
    };
    set.insert_tile(PLATFORM, with(vec![platform]));
    set
  }

  /// A map of `size` 16 px cells over `shapes`, with a tile layer of each
  /// name holding the tiles listed
  fn code_map(size: UVec2, layers: &[(&str, &[(IVec2, u32)])]) -> TileMap {
    let mut map = TileMap::new(size, UVec2::splat(16));
    let set = map.add_tileset(TilesetSource::Embedded(Arc::new(shapes())));
    for (name, tiles) in layers {
      let layer = map.add_tile_layer(name);
      let grid = map.tiles_mut(layer).expect("a tile layer");
      for &(cell, id) in *tiles {
        grid.set(cell, Some(Tile::new(set, id)));
      }
    }
    map
  }

  fn add(world: &mut World, map: TileMap) -> Handle<TileMap> {
    world.resource_mut::<Assets<TileMap>>().add(map)
  }

  fn load(world: &mut World, path: &str) -> Handle<TileMap> {
    match AssetsView::new(world).load_now::<TileMap>(path) {
      Ok(handle) => handle,
      Err(error) => panic!("{path}: {}", error.report()),
    }
  }

  fn colliders(world: &World, entity: Entity) -> TileColliders {
    match world.get::<TileColliders>(entity) {
      Some(tiles) => tiles.clone(),
      None => panic!("{entity:?} has no TileColliders"),
    }
  }

  /// The map's layer entities, by index
  fn layers_of(world: &mut World, map: Entity) -> Vec<Entity> {
    let mut found: Vec<(usize, Entity)> = world
      .query::<(Entity, &MapLayerRef)>()
      .iter(world)
      .filter(|(_, l)| l.map == map)
      .map(|(e, l)| (l.index, e))
      .collect();
    found.sort();
    found.into_iter().map(|(_, e)| e).collect()
  }

  fn count_parts(world: &mut World) -> usize {
    world.query::<&TilePart>().iter(world).count()
  }

  /// The parts under `parent`, by entity
  fn parts_of(world: &mut World, parent: Entity) -> Vec<(Entity, Collider)> {
    let mut found: Vec<(Entity, Collider)> = world
      .query::<(Entity, &ChildOf, &Collider)>()
      .iter(world)
      .filter(|(_, child_of, _)| child_of.parent() == parent)
      .map(|(e, _, c)| (e, c.clone()))
      .collect();
    found.sort_by_key(|(e, _)| *e);
    found
  }

  /// The one part with a point in `min..=max` (px)
  fn part_in(parts: &[(Entity, Collider)], min: Vec2, max: Vec2) -> Entity {
    let points = |c: &Collider| match &c.shape {
      ColliderShape::Polyline { points } | ColliderShape::Polygon { points } => points.clone(),
      ColliderShape::Segment { a, b } => vec![*a, *b],
      _ => Vec::new(),
    };
    let inside = |p: &Vec2| p.cmpge(min).all() && p.cmple(max).all();
    let found: Vec<Entity> = parts
      .iter()
      .filter(|(_, c)| points(c).iter().any(inside))
      .map(|(e, _)| *e)
      .collect();
    assert_eq!(found.len(), 1, "one part in {min}..{max}: {parts:?}");
    found[0]
  }

  /// Whether `polygon` has exactly the points `expected`, in any order
  fn same_points(polygon: &[Vec2], expected: &[(f32, f32)]) -> bool {
    polygon.len() == expected.len()
      && expected.iter().all(|&(x, y)| {
        polygon
          .iter()
          .any(|p| (*p - Vec2::new(x, y)).abs().max_element() < 1e-4)
      })
  }

  #[test]
  fn tiled_collision_shapes_become_cells_after_flips() {
    use SlopeCorner::{BottomLeft, BottomRight, TopRight};
    let mut maps = MapWorld::new();
    let level = load(&mut maps.world, "tiled/maps/level.tmj");
    let map = maps.spawn(MapRoot::new(level));
    let tiles = colliders(&maps.world, map);
    assert_eq!((tiles.width(), tiles.height()), (24, 12));
    assert_eq!(tiles.tile_size, Vec2::splat(16.0));
    // The floor is two layers' tiles, in one grid
    assert!((0..24).all(|x| tiles.get(x, 11) == TileCell::Solid));
    // One slope tile, as drawn, mirrored and turned
    assert_eq!(tiles.get(20, 10), TileCell::Slope(BottomRight));
    assert_eq!(tiles.get(21, 10), TileCell::Slope(BottomLeft), "mirrored");
    assert_eq!(tiles.get(22, 10), TileCell::Slope(TopRight), "upside down");
    assert_eq!(tiles.get(23, 10), TileCell::Slope(BottomLeft), "turned");
    assert!((4..8).all(|x| tiles.get(x, 7) == TileCell::OneWay));
    let TileCell::Shape(half) = tiles.get(18, 10) else {
      panic!("the half block is {:?}", tiles.get(18, 10))
    };
    let half = tiles.shape(half).expect("the half block's shape");
    assert!(!half.one_way);
    assert_eq!(half.polygons.len(), 1);
    assert!(
      same_points(
        &half.polygons[0],
        &[(0.0, 0.5), (1.0, 0.5), (1.0, 1.0), (0.0, 1.0)]
      ),
      "{:?}",
      half.polygons
    );
    assert_eq!(tiles.get(14, 10), TileCell::Empty, "decor does not collide");
    assert_eq!(
      tiles.get(10, 5),
      TileCell::Empty,
      "an occluder only occludes"
    );
    // The material is the map's properties (its layers say none)
    assert_eq!((tiles.layer, tiles.mask), (2, u32::MAX));
    assert_eq!((tiles.friction, tiles.restitution), (0.25, 0.0));
    for layer in layers_of(&mut maps.world, map) {
      assert!(maps.world.get::<TileColliders>(layer).is_none());
    }
    // and physics builds it in that step
    assert!(!parts_of(&mut maps.world, map).is_empty());
  }

  #[test]
  fn flips_and_offsets_place_shapes_where_the_tile_draws() {
    use SlopeCorner::{BottomLeft, BottomRight, TopLeft, TopRight};
    let h = TileFlip::HORIZONTAL;
    let v = TileFlip::VERTICAL;
    let d = TileFlip::DIAGONAL;
    let mut maps = MapWorld::new();
    let flips = [
      (TileFlip::NONE, BottomRight),
      (h, BottomLeft),
      (v, TopRight),
      (h | v, TopLeft),
      // the diagonal mirrors this one onto itself
      (d, BottomRight),
      (d | h, BottomLeft),
      (d | v, TopRight),
      (d | h | v, TopLeft),
    ];
    let mut map = code_map(UVec2::new(8, 2), &[("ground", &[])]);
    for (x, (flip, _)) in flips.iter().enumerate() {
      let tile = Tile::new(0, SLOPE).with_flip(*flip);
      map
        .tiles_mut(0)
        .map(|g| g.set(IVec2::new(x as i32, 0), Some(tile)));
    }
    // A half block in a tileset drawn 8 px lower: its shape leaves the cell
    // through the bottom, and the cell keeps the top half of what is left
    let mut low = shapes();
    low.tile_offset = IVec2::new(0, -8);
    let low = map.add_tileset(TilesetSource::Embedded(Arc::new(low)));
    map
      .tiles_mut(0)
      .map(|g| g.set(IVec2::new(0, 1), Some(Tile::new(low, HALF))));
    let handle = add(&mut maps.world, map);
    let map = maps.spawn(MapRoot::new(handle));
    let tiles = colliders(&maps.world, map);
    for (x, (flip, corner)) in flips.iter().enumerate() {
      assert_eq!(
        tiles.get(x as u32, 0),
        TileCell::Slope(*corner),
        "flip {flip:?}"
      );
    }
    let TileCell::Shape(raised) = tiles.get(0, 1) else {
      panic!("the raised half block is {:?}", tiles.get(0, 1))
    };
    let raised = tiles.shape(raised).expect("its shape");
    assert!(
      same_points(
        &raised.polygons[0],
        &[(0.0, 0.0), (1.0, 0.0), (1.0, 0.5), (0.0, 0.5)]
      ),
      "{:?}",
      raised.polygons
    );
  }

  #[test]
  fn a_tile_larger_than_its_cell_collides_where_it_draws() {
    // A 32 x 32 tile on 16 px cells stands on its cell's bottom edge from
    // the left: its bottom-left quarter is the cell
    let mut big = Tileset::new("big", UVec2::splat(32), TilesetImage::Collection);
    big.insert_tile(
      0,
      TileData {
        shapes: vec![shape(rect(0.0, 16.0, 16.0, 16.0), false)],
        ..TileData::default()
      },
    );
    big.insert_tile(
      1,
      TileData {
        shapes: vec![shape(rect(16.0, 16.0, 16.0, 16.0), false)],
        ..TileData::default()
      },
    );
    let mut map = TileMap::new(UVec2::new(2, 1), UVec2::splat(16));
    let set = map.add_tileset(TilesetSource::Embedded(Arc::new(big)));
    let layer = map.add_tile_layer("trees");
    if let Some(grid) = map.tiles_mut(layer) {
      grid.set(IVec2::new(0, 0), Some(Tile::new(set, 0)));
      grid.set(IVec2::new(1, 0), Some(Tile::new(set, 1)));
    }
    let mut maps = MapWorld::new();
    let handle = add(&mut maps.world, map);
    let map = maps.spawn(MapRoot::new(handle));
    let tiles = colliders(&maps.world, map);
    assert_eq!(
      tiles.get(0, 0),
      TileCell::Solid,
      "the quarter over its cell"
    );
    // the other tile's square is right of its cell: nothing of it is inside
    match tiles.get(1, 0) {
      TileCell::Empty => {}
      TileCell::Shape(index) => {
        let polygon = &tiles.shape(index).expect("its shape").polygons[0];
        assert!(polygon.iter().all(|p| p.x >= 1.0 - 1e-4), "{polygon:?}");
      }
      other => panic!("the square right of its cell is {other:?}"),
    }
  }

  #[test]
  fn int_grid_names_collide_by_default_and_overrides_apply() {
    use SlopeCorner::{BottomLeft, TopRight};
    use TileCell::{Empty, OneWay, Slope, Solid};
    let mut map = code_map(
      UVec2::new(8, 1),
      &[("collision", &[(IVec2::new(7, 0), SOLID)])],
    );
    let names = [
      (1, "Solid"),
      (2, "one_way"),
      (3, "SLOPE_BOTTOM_LEFT"),
      (4, "slope_top_right"),
      (5, "Wall"),
      (6, "lava"),
      (7, "water"),
    ];
    if let LayerKind::Tiles { intgrid, .. } = &mut map.layers[0].kind {
      *intgrid = Some(IntGrid {
        size: UVec2::new(8, 1),
        values: vec![1, 2, 3, 4, 5, 6, 7, 0],
        names: names.iter().map(|(v, n)| (*v, n.to_string())).collect(),
      });
    }
    let mut maps = MapWorld::new();
    let root = MapRoot::new(add(&mut maps.world, map))
      .with_intgrid_collision("wall", Solid)
      .with_intgrid_collision("lava", Solid)
      .with_intgrid_collision(6, OneWay)
      .with_intgrid_collision(2, Empty);
    let map = maps.spawn(root);
    let tiles = colliders(&maps.world, map);
    let row: Vec<TileCell> = (0..8).map(|x| tiles.get(x, 0)).collect();
    // by name (any case), a value over a name, nothing for an unknown name;
    // the layer's tiles (its auto-tiles) do not collide, its IntGrid does
    assert_eq!(
      row,
      [
        Solid,
        Empty,
        Slope(BottomLeft),
        Slope(TopRight),
        Solid,
        OneWay,
        Empty,
        Empty
      ]
    );
  }

  #[test]
  fn an_ldtk_int_grid_collides_by_its_names() {
    let mut maps = MapWorld::new();
    let project = match AssetsView::new(&mut maps.world)
      .load_now::<crate::core::tilemap::LdtkProject>("ldtk/internal.ldtk")
    {
      Ok(project) => project,
      Err(error) => panic!("{}", error.report()),
    };
    let level = maps
      .world
      .resource::<Assets<crate::core::tilemap::LdtkProject>>()
      .get(&project)
      .and_then(|p| p.levels.first().map(|l| l.identifier.clone()))
      .expect("a level");
    let root = MapRoot::ldtk(project, &level).with_intgrid_collision("Wall", TileCell::Solid);
    let map = maps.spawn(root);
    // The IntGrid layer is the one that collides; its walls are solid
    let walls = maps
      .world
      .get::<TileColliders>(map)
      .map(|tiles| {
        (0..tiles.height())
          .flat_map(|y| (0..tiles.width()).map(move |x| (x, y)))
          .filter(|&(x, y)| tiles.get(x, y) == TileCell::Solid)
          .count()
      })
      .unwrap_or(0);
    assert_eq!(walls, 9, "the level's walls collide");
  }

  #[test]
  fn a_layer_with_collides_false_adds_nothing() {
    let floor: Vec<(IVec2, u32)> = (0..4).map(|x| (IVec2::new(x, 0), SOLID)).collect();
    let mut maps = MapWorld::new();
    let mut off = code_map(UVec2::new(4, 1), &[("walls", &floor)]);
    off.layers[0].info.collides = false;
    let handle = add(&mut maps.world, off);
    let off = maps.spawn(MapRoot::new(handle));
    let on = code_map(UVec2::new(4, 1), &[("walls", &floor)]);
    let on = add(&mut maps.world, on);
    let quiet = maps.spawn(MapRoot::new(on.clone()).without_collision());
    maps.frame();
    for map in [off, quiet] {
      assert!(maps.world.get::<TileColliders>(map).is_none());
      for layer in layers_of(&mut maps.world, map) {
        assert!(maps.world.get::<TileColliders>(layer).is_none());
      }
    }
    assert_eq!(count_parts(&mut maps.world), 0);
    // The same layer colliding builds its floor
    let loud = maps.spawn(MapRoot::new(on));
    assert_eq!(colliders(&maps.world, loud).get(3, 0), TileCell::Solid);
    assert_eq!(count_parts(&mut maps.world), 1);
  }

  /// A floor 40 cells long on row 3 and an island at (20, 0), on 16 px cells
  fn floor_and_island(maps: &mut MapWorld) -> Entity {
    let mut tiles: Vec<(IVec2, u32)> = (0..40).map(|x| (IVec2::new(x, 3), SOLID)).collect();
    tiles.push((IVec2::new(20, 0), SOLID));
    let map = code_map(UVec2::new(40, 4), &[("ground", &tiles)]);
    let handle = add(&mut maps.world, map);
    maps.spawn(MapRoot::new(handle))
  }

  #[test]
  fn an_edit_updates_its_cells_before_the_next_step() {
    let mut maps = MapWorld::new();
    let map = floor_and_island(&mut maps);
    let parts = parts_of(&mut maps.world, map);
    let island_area = (Vec2::new(320.0, 0.0), Vec2::new(336.0, 16.0));
    let island = part_in(&parts, island_area.0, island_area.1);
    let mut view = MapsView::new(&mut maps.world);
    assert!(view.set_tile(map, "ground", IVec2::new(10, 1), Some(Tile::new(0, SOLID))));
    assert!(view.set_tile(map, "ground", IVec2::new(39, 3), None));
    // One fixed step: the cells change and physics builds them
    maps.step();
    let tiles = colliders(&maps.world, map);
    assert_eq!(tiles.get(10, 1), TileCell::Solid);
    assert_eq!(tiles.get(39, 3), TileCell::Empty);
    let parts = parts_of(&mut maps.world, map);
    part_in(&parts, Vec2::new(160.0, 16.0), Vec2::new(176.0, 32.0));
    let floor = part_in(&parts, Vec2::new(0.0, 48.0), Vec2::new(0.0, 64.0));
    let right = parts
      .iter()
      .find(|(e, _)| *e == floor)
      .map(|(_, c)| match &c.shape {
        ColliderShape::Polyline { points } => points.iter().map(|p| p.x).fold(0.0, f32::max),
        _ => 0.0,
      });
    assert_eq!(right, Some(39.0 * 16.0), "the floor lost its last cell");
    // The island's outline was not touched
    assert_eq!(part_in(&parts, island_area.0, island_area.1), island);
  }

  #[test]
  fn an_edit_that_changes_no_cell_kind_changes_no_cell() {
    let mut maps = MapWorld::new();
    let map = floor_and_island(&mut maps);
    let before = colliders(&maps.world, map);
    // The same block mirrored, where one was: still a solid cell
    assert!(MapsView::new(&mut maps.world).set_tile(
      map,
      "ground",
      IVec2::new(5, 3),
      Some(Tile::new(0, SOLID).with_flip(TileFlip::HORIZONTAL))
    ));
    maps.step();
    assert_eq!(colliders(&maps.world, map), before);
  }

  #[test]
  fn the_strongest_kind_wins_a_cell_and_the_upper_layer_a_tie() {
    let at = |x: i32| IVec2::new(x, 0);
    let flipped = Tile::new(0, HALF).with_flip(TileFlip::VERTICAL);
    let mut map = code_map(
      UVec2::new(5, 1),
      &[
        (
          "below",
          &[
            (at(0), SLOPE),
            (at(1), SOLID),
            (at(2), PLATFORM),
            (at(3), HALF),
          ],
        ),
        (
          "above",
          &[
            (at(0), HALF),
            (at(1), SLOPE),
            (at(2), SLOPE),
            (at(4), PLATFORM),
          ],
        ),
      ],
    );
    map.tiles_mut(1).map(|g| g.set(at(3), Some(flipped)));
    let mut maps = MapWorld::new();
    let handle = add(&mut maps.world, map);
    let map = maps.spawn(MapRoot::new(handle));
    let tiles = colliders(&maps.world, map);
    let shape_of = |cell: TileCell| match cell {
      TileCell::Shape(index) => tiles.shape(index).map(|s| s.polygons[0].clone()),
      _ => None,
    };
    // A shape over a slope, a solid cell over a slope, a slope over a
    // one-way cell, a one-way cell alone
    let bottom_half = [(0.0, 0.5), (1.0, 0.5), (1.0, 1.0), (0.0, 1.0)];
    let top_half = [(0.0, 0.0), (1.0, 0.0), (1.0, 0.5), (0.0, 0.5)];
    assert!(shape_of(tiles.get(0, 0)).is_some_and(|p| same_points(&p, &bottom_half)));
    assert_eq!(tiles.get(1, 0), TileCell::Solid);
    assert_eq!(tiles.get(2, 0), TileCell::Slope(SlopeCorner::BottomRight));
    assert_eq!(tiles.get(4, 0), TileCell::OneWay);
    // Of two shapes, the upper layer's
    assert!(shape_of(tiles.get(3, 0)).is_some_and(|p| same_points(&p, &top_half)));
    // and an edit to the layer below changes nothing the one above covers
    assert!(MapsView::new(&mut maps.world).set_tile(map, "below", at(3), None));
    maps.step();
    assert!(
      shape_of(colliders(&maps.world, map).get(3, 0)).is_some_and(|p| same_points(&p, &top_half))
    );
  }

  #[test]
  fn layers_of_another_cell_size_or_offset_collide_on_their_own_entity() {
    let mut map = code_map(
      UVec2::new(4, 2),
      &[
        ("art", &[(IVec2::new(0, 1), SOLID)]),
        ("more art", &[(IVec2::new(1, 1), SOLID)]),
        ("shifted", &[(IVec2::new(3, 0), SOLID)]),
      ],
    );
    map.layers[2].info.offset = Vec2::new(4.0, 0.0);
    // An 8 px IntGrid under the 16 px art, as LDtk levels mix them
    let fine = map.add_tile_layer("fine");
    map.layers[fine].kind = LayerKind::Tiles {
      grid: crate::core::tilemap::TileGrid::new(UVec2::new(8, 4)),
      intgrid: Some(IntGrid {
        size: UVec2::new(8, 4),
        values: (0..32).map(|i| i32::from(i == 31)).collect(),
        names: [(1, "solid".to_string())].into_iter().collect(),
      }),
      cell_size: UVec2::splat(8),
    };
    let mut maps = MapWorld::new();
    let handle = add(&mut maps.world, map);
    let map = maps.spawn(MapRoot::new(handle));
    let layers = layers_of(&mut maps.world, map);
    // The two layers sharing the map's cells and no offset are one grid
    let shared = colliders(&maps.world, map);
    assert_eq!((shared.width(), shared.height()), (4, 2));
    assert_eq!(shared.get(0, 1), TileCell::Solid);
    assert_eq!(shared.get(1, 1), TileCell::Solid);
    assert_eq!(shared.get(3, 0), TileCell::Empty);
    for layer in &layers[..2] {
      assert!(maps.world.get::<TileColliders>(*layer).is_none());
    }
    let shifted = colliders(&maps.world, layers[2]);
    assert_eq!(shifted.get(3, 0), TileCell::Solid);
    assert_eq!(shifted.tile_size, Vec2::splat(16.0));
    let fine = colliders(&maps.world, layers[3]);
    assert_eq!(
      (fine.width(), fine.height(), fine.tile_size),
      (8, 4, Vec2::splat(8.0))
    );
    assert_eq!(fine.get(7, 3), TileCell::Solid);
    assert_eq!(fine.get(0, 0), TileCell::Empty);
    // Each built its parts
    assert_eq!(parts_of(&mut maps.world, layers[2]).len(), 1);
    assert_eq!(parts_of(&mut maps.world, layers[3]).len(), 1);
  }

  #[test]
  fn a_disabled_map_collides_with_nothing_until_enabled() {
    let mut map = code_map(
      UVec2::new(4, 2),
      &[
        ("ground", &[(IVec2::new(0, 1), SOLID)]),
        ("shifted", &[(IVec2::new(3, 0), SOLID)]),
      ],
    );
    map.layers[1].info.offset = Vec2::new(0.0, 4.0);
    let mut maps = MapWorld::new();
    let handle = add(&mut maps.world, map);
    let map = maps.spawn(MapRoot::new(handle));
    maps.frame();
    assert_eq!(count_parts(&mut maps.world), 2);
    maps.world.entity_mut(map).insert(Disabled);
    maps.frame();
    assert_eq!(
      count_parts(&mut maps.world),
      0,
      "a disabled map's layers too"
    );
    // An edit while disabled is there when it comes back
    assert!(MapsView::new(&mut maps.world).set_tile(
      map,
      "shifted",
      IVec2::new(0, 0),
      Some(Tile::new(0, SOLID))
    ));
    maps.frame();
    assert_eq!(count_parts(&mut maps.world), 0);
    maps.world.entity_mut(map).remove::<Disabled>();
    maps.frame();
    assert_eq!(count_parts(&mut maps.world), 3);
    let shifted = layers_of(&mut maps.world, map)[1];
    assert_eq!(colliders(&maps.world, shifted).get(0, 0), TileCell::Solid);
  }

  #[test]
  fn a_colliding_layer_too_wide_for_tile_colliders_warns_once_naming_it() {
    let mut map = code_map(
      UVec2::new(8193, 1),
      &[("too-wide-7c1e", &[(IVec2::ZERO, SOLID)])],
    );
    // and a layer within the limit, offset onto its own grid, still collides
    let small = map.add_tile_layer("small");
    map.layers[small].kind = LayerKind::Tiles {
      grid: {
        let mut grid = crate::core::tilemap::TileGrid::new(UVec2::new(2, 1));
        grid.set(IVec2::ZERO, Some(Tile::new(0, SOLID)));
        grid
      },
      intgrid: None,
      cell_size: UVec2::splat(16),
    };
    map.layers[small].info.offset = Vec2::new(0.0, 32.0);
    let mut maps = MapWorld::new();
    let handle = add(&mut maps.world, map);
    let map = maps.spawn(MapRoot::new(handle));
    maps.frame();
    maps.frame();
    assert!(maps.world.get::<TileColliders>(map).is_none());
    let warned = crate::core::dev_log::get_entries()
      .iter()
      .filter(|e| e.message.contains("too-wide-7c1e"))
      .count();
    assert_eq!(warned, 1, "warned once, naming the layer");
    let small = layers_of(&mut maps.world, map)[1];
    assert_eq!(colliders(&maps.world, small).get(0, 0), TileCell::Solid);
  }

  #[test]
  fn map_properties_set_the_material_and_a_wrong_type_warns_once() {
    let mut map = code_map(UVec2::new(2, 1), &[("ground", &[(IVec2::ZERO, SOLID)])]);
    map
      .properties
      .insert("collision_layer", PropertyValue::Int(4));
    map
      .properties
      .insert("collision_mask", PropertyValue::Int(-1));
    map.properties.insert("friction", PropertyValue::Int(1));
    map.properties.insert(
      "restitution",
      PropertyValue::String("bouncy-3f9a".to_string()),
    );
    let mut maps = MapWorld::new();
    let handle = add(&mut maps.world, map);
    let map = maps.spawn(MapRoot::new(handle));
    maps.frame();
    maps.frame();
    let tiles = colliders(&maps.world, map);
    assert_eq!((tiles.layer, tiles.mask), (4, u32::MAX));
    assert_eq!((tiles.friction, tiles.restitution), (1.0, 0.0));
    let warned = crate::core::dev_log::get_entries()
      .iter()
      .filter(|e| e.message.contains("restitution") && e.message.contains("bouncy-3f9a"))
      .count();
    assert_eq!(warned, 1, "a wrong type warns once");
    let parts = parts_of(&mut maps.world, map);
    assert!(parts.iter().all(|(_, c)| c.layer == 4 && c.friction == 1.0));
  }

  #[test]
  fn a_layer_s_own_material_overrides_the_map_s() {
    let mut map = code_map(
      UVec2::new(4, 1),
      &[
        ("ground", &[(IVec2::new(0, 0), SOLID)]),
        ("ice", &[(IVec2::new(1, 0), SOLID)]),
        ("same", &[(IVec2::new(2, 0), SOLID)]),
      ],
    );
    map
      .properties
      .insert("friction", PropertyValue::Float(0.25));
    map
      .properties
      .insert("collision_layer", PropertyValue::Int(2));
    // Ice is slippery and on its own collision layer; "same" says what the
    // map says
    let ice = &mut map.layers[1].info.properties;
    ice.insert("friction", PropertyValue::Float(0.0));
    ice.insert("collision_layer", PropertyValue::Int(8));
    map.layers[2]
      .info
      .properties
      .insert("friction", PropertyValue::Float(0.25));
    let mut maps = MapWorld::new();
    let handle = add(&mut maps.world, map);
    let map = maps.spawn(MapRoot::new(handle));
    let layers = layers_of(&mut maps.world, map);
    let shared = colliders(&maps.world, map);
    assert_eq!((shared.friction, shared.layer), (0.25, 2));
    assert_eq!(shared.get(0, 0), TileCell::Solid);
    assert_eq!(
      shared.get(2, 0),
      TileCell::Solid,
      "a layer saying what the map says"
    );
    assert_eq!(shared.get(1, 0), TileCell::Empty);
    let ice = colliders(&maps.world, layers[1]);
    assert_eq!((ice.friction, ice.layer, ice.mask), (0.0, 8, u32::MAX));
    assert_eq!(ice.get(1, 0), TileCell::Solid);
    assert_eq!(ice.get(0, 0), TileCell::Empty);
    assert!(maps.world.get::<TileColliders>(layers[2]).is_none());
    let parts = parts_of(&mut maps.world, layers[1]);
    assert!(!parts.is_empty() && parts.iter().all(|(_, c)| c.friction == 0.0 && c.layer == 8));
  }

  #[test]
  fn a_map_whose_root_goes_keeps_no_colliders() {
    let mut maps = MapWorld::new();
    let map = floor_and_island(&mut maps);
    assert!(count_parts(&mut maps.world) > 0);
    maps.world.entity_mut(map).remove::<MapRoot>();
    maps.frame();
    maps.frame();
    assert!(maps.world.get::<TileColliders>(map).is_none());
    assert_eq!(count_parts(&mut maps.world), 0);
  }

  /// Spawns the fixture level as it enters
  struct Level {
    map: Arc<Mutex<Option<Entity>>>,
  }

  impl Scene for Level {
    fn on_enter(&mut self, ctx: &mut SceneContext) {
      let level = ctx
        .assets()
        .load_now::<TileMap>("tiled/maps/level.tmj")
        .expect("level.tmj loads");
      let map = ctx.world.spawn(MapRoot::new(level)).id();
      if let Ok(mut held) = self.map.lock() {
        *held = Some(map);
      }
    }
  }

  #[test]
  fn a_character_runs_and_lands_on_a_loaded_tiled_level() {
    let map = Arc::new(Mutex::new(None));
    let Some(mut engine) = HeadlessEngine::new_or_skip(
      EngineBuilder::new()
        .resolution(160, 120)
        .asset_source(files_with(&[]))
        .start_scene(Level { map: map.clone() }),
    ) else {
      return;
    };
    engine.step().expect("a frame");
    let map = map.lock().ok().and_then(|m| *m).expect("the level spawned");
    assert!(engine.world_mut().get::<TileColliders>(map).is_some());
    let player = engine
      .world_mut()
      .spawn((
        Transform::from_xy(40.0, 120.0),
        Collider::rect(12.0, 24.0),
        PlatformerMovement::default(),
      ))
      .id();
    let state = |engine: &mut HeadlessEngine| {
      let world = engine.world_mut();
      let at = world.get::<Transform>(player).map(|t| t.translation);
      let state = world.get::<CharacterState>(player).cloned();
      (at.unwrap_or(Vec2::NAN), state.unwrap_or_default())
    };
    // It falls onto the floor, whose top is at y = 176
    let mut landed = false;
    for _ in 0..90 {
      engine.step().expect("a frame");
      if state(&mut engine).1.on_floor {
        landed = true;
        break;
      }
    }
    let (at, _) = state(&mut engine);
    assert!(landed, "it never landed: at {at}");
    assert!((at.y - 164.0).abs() < 1.0, "standing at {at}");
    // It runs right, over the seam where the floor passes from one layer to
    // the next (x = 192) and through the decor layer's block (x 224 to 240),
    // which does not collide, until the half block's side (x = 288), which
    // it cannot step up
    let mut stopped = None;
    for step in 0..120 {
      if let Some(mut intent) = engine.world_mut().get_mut::<MoveIntent>(player) {
        intent.direction = Vec2::X;
      }
      engine.step().expect("a frame");
      let (at, state) = state(&mut engine);
      assert!(state.on_floor, "step {step}: at {at}, {state:?}");
      if state.on_wall {
        stopped = Some((at, state));
        break;
      }
    }
    let Some((at, state)) = stopped else {
      panic!("it ran on to {}", state(&mut engine).0)
    };
    assert!((at.x - 282.0).abs() < 0.5, "stopped at {at}, {state:?}");
    assert!((state.wall_normal - Vec2::NEG_X).length() < 1e-3);
  }

  /// Times one tile's edit on a 1024 x 1024 map of four colliding layers:
  /// islands of solid tiles on one, and every cell of the others holding a
  /// tile with no shape (the classification's worst case). Run it in
  /// release: `cargo test --release -p ryo2d --lib one_tile_edit_sync_timing
  /// -- --ignored --nocapture`
  #[test]
  #[ignore = "timing; run in release by hand"]
  fn one_tile_edit_sync_timing() {
    use std::time::{Duration, Instant};

    use crate::core::ecs::Schedule;

    const SIDE: u32 = 1024;
    let mut map = code_map(UVec2::splat(SIDE), &[]);
    for name in ["ground", "decor 1", "decor 2", "decor 3"] {
      let layer = map.add_tile_layer(name);
      let grid = map.tiles_mut(layer).expect("a tile layer");
      if layer == 0 {
        for y in (0..SIDE as i32).step_by(8) {
          for x in (0..SIDE as i32).step_by(8) {
            grid.fill(
              IVec2::new(x + 2, y + 2),
              IVec2::new(x + 4, y + 4),
              Some(Tile::new(0, SOLID)),
            );
          }
        }
      } else {
        grid.fill(
          IVec2::ZERO,
          IVec2::splat(SIDE as i32 - 1),
          Some(Tile::new(0, 9)),
        );
      }
    }
    let mut maps = MapWorld::new();
    let handle = add(&mut maps.world, map);
    let map = maps.world.spawn(MapRoot::new(handle)).id();
    let mut sync = Schedule::default();
    sync.add_systems(super::sync_map_colliders);
    let timed = |maps: &mut MapWorld, what: &str, run: &mut dyn FnMut(&mut MapWorld)| {
      let start = Instant::now();
      run(maps);
      println!("{what}: {:?}", start.elapsed());
    };
    timed(&mut maps, "the spawn", &mut |maps| maps.pre_update());
    timed(&mut maps, "the first collision build", &mut |maps| {
      sync.run(&mut maps.world)
    });
    timed(&mut maps, "physics building it, and a step", &mut |maps| {
      maps.step()
    });
    timed(&mut maps, "the first occluders", &mut |maps| {
      maps.post_update()
    });
    let median = |mut times: Vec<Duration>| {
      times.sort();
      (times[times.len() / 2], times[times.len() - 1])
    };
    let (mut colliders, mut occluders) = (Vec::new(), Vec::new());
    for i in 0..400 {
      let tile = (i % 2 == 0).then_some(Tile::new(0, SOLID));
      MapsView::new(&mut maps.world).set_tile(map, "ground", IVec2::new(517, 517), tile);
      let start = Instant::now();
      sync.run(&mut maps.world);
      colliders.push(start.elapsed());
      maps.step();
      let start = Instant::now();
      maps.post_update();
      occluders.push(start.elapsed());
    }
    let mut idle = Vec::new();
    for _ in 0..400 {
      let start = Instant::now();
      sync.run(&mut maps.world);
      idle.push(start.elapsed());
    }
    let mut idle_occluders = Vec::new();
    for _ in 0..400 {
      let start = Instant::now();
      maps.post_update();
      idle_occluders.push(start.elapsed());
    }
    println!(
      "an edit's collision sync (median, max): {:?}",
      median(colliders)
    );
    println!(
      "an edit's occluder sync, commands applied: {:?}",
      median(occluders)
    );
    println!("an idle step's collision sync: {:?}", median(idle));
    println!(
      "an idle frame's occluder sync: {:?}",
      median(idle_occluders)
    );
  }
}
