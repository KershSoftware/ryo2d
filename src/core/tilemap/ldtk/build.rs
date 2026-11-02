//! From what an LDtk file says ([`raw`](super::raw)) to a project of levels,
//! and the checks that need the loaded images.

use std::sync::Arc;

use glam::{IVec2, UVec2, Vec2};
use serde_json::Value;

use super::raw::{
  RawEntity, RawField, RawLayer, RawLayerDef, RawLevel, RawProject, RawRect, RawTile,
  RawTilesetDef, RawWorld,
};
use super::{LdtkLevel, LdtkProject, Neighbour, NeighbourDirection, WorldLayout, WorldLevel};
use crate::core::asset::{AssetError, AssetPath, CheckContext, LoadContext};
use crate::core::graphics::Image;
use crate::core::hash::{FastMap, FastSet};
use crate::core::tilemap::tiled::build::{
  DependencyPlaces, MAX_MAP_CELLS, MAX_TILES, Unread, cell_count, check_map_cells, check_tileset,
  check_used_ids, engine_cells, fail, join, layer_cells, load_dep,
};
use crate::core::tilemap::tiled::json::parse;
use crate::core::tilemap::{
  GridShape, IntGrid, LayerInfo, LayerKind, MapGrid, MapLayer, MapObjectData, ObjectShape,
  Properties, PropertyValue, Tile, TileData, TileFlip, TileGrid, TileMap, Tileset, TilesetImage,
  TilesetSource,
};
use crate::core::ui::Color;

/// The gap LDtk leaves between levels of a linear layout (`World.reorganizeWorld`)
const LINEAR_SPACING: u32 = 48;
const MAX_TILESETS: usize = 256;
/// The name of a single-world project's only world
const DEFAULT_WORLD: &str = "World";

/// A tileset as the layers see it
struct TilesetRef {
  /// Its index in the map's tilesets
  index: u16,
  set: Arc<Tileset>,
}

/// What building a level looks things up in
struct Defs<'a> {
  path: &'a AssetPath,
  tilesets: FastMap<i64, TilesetRef>,
  layers: FastMap<i64, &'a RawLayerDef>,
  /// Each entity definition's tile render mode
  tile_modes: FastMap<i64, &'a str>,
  default_grid: i64,
  default_bg: Option<String>,
  /// What the project uses that loads otherwise than LDtk shows it
  unread: std::cell::RefCell<Unread>,
}

fn to_u32(path: &AssetPath, place: &str, what: &str, v: i64) -> Result<u32, AssetError> {
  u32::try_from(v).map_err(|_| fail(path, place, format!("{what} {v} is out of range")))
}

fn to_i32(path: &AssetPath, place: &str, what: &str, v: i64) -> Result<i32, AssetError> {
  i32::try_from(v).map_err(|_| fail(path, place, format!("{what} {v} is out of range")))
}

fn ceil_div(a: u32, b: u32) -> u32 {
  a.div_ceil(b.max(1))
}

/// `#RRGGBB`
fn parse_color(text: &str) -> Option<Color> {
  let hex = text.strip_prefix('#')?;
  if hex.len() != 6 || !hex.is_ascii() {
    return None;
  }
  let v = u32::from_str_radix(hex, 16).ok()?;
  Some(Color::rgb((v >> 16) as u8, (v >> 8) as u8, v as u8))
}

// ---- tilesets -------------------------------------------------------------

fn build_tilesets(
  raw: &[RawTilesetDef],
  cx: &mut LoadContext<'_>,
  path: &AssetPath,
  places: &mut DependencyPlaces,
) -> Result<Vec<(i64, Arc<Tileset>)>, AssetError> {
  if raw.len() > MAX_TILESETS {
    return Err(fail(
      path,
      "defs",
      format!(
        "{} tilesets is past the {MAX_TILESETS} a map holds",
        raw.len()
      ),
    ));
  }
  let mut out = Vec::with_capacity(raw.len());
  for def in raw {
    let place = format!("tileset {:?}", def.identifier);
    let grid = to_u32(path, &place, "tileGridSize", def.tile_grid_size)?;
    if grid == 0 {
      return Err(fail(path, &place, "the tile size is 0"));
    }
    let spacing = to_u32(path, &place, "spacing", def.spacing)?;
    let padding = to_u32(path, &place, "padding", def.padding)?;
    let size = UVec2::new(
      to_u32(path, &place, "pxWid", def.px_wid)?,
      to_u32(path, &place, "pxHei", def.px_hei)?,
    );
    let fit = |extent: u32| {
      extent
        .saturating_sub(padding.saturating_mul(2))
        .saturating_add(spacing)
        / grid.saturating_add(spacing).max(1)
    };
    let mut columns = to_u32(path, &place, "__cWid", def.c_wid)?;
    let mut rows = to_u32(path, &place, "__cHei", def.c_hei)?;
    let image = match def.rel_path.as_deref().filter(|p| !p.is_empty()) {
      Some(written) => {
        if size.x == 0 || size.y == 0 {
          return Err(fail(path, &place, "the image has no size (pxWid, pxHei)"));
        }
        if columns == 0 {
          columns = fit(size.x);
        }
        if rows == 0 {
          rows = fit(size.y);
        }
        TilesetImage::Atlas {
          image: load_dep::<Image>(cx, path, &place, written, places)?,
          size,
        }
      }
      // LDtk's built-in icons: no file, nothing to draw from
      None => TilesetImage::Collection,
    };
    let count = u64::from(columns) * u64::from(rows);
    if count > u64::from(MAX_TILES) {
      return Err(fail(
        path,
        &place,
        format!("{count} tiles is past the {MAX_TILES} a tileset holds"),
      ));
    }
    let mut set = Tileset::new(def.identifier.clone(), UVec2::splat(grid), image);
    set.columns = columns;
    set.tile_count = count as u32;
    set.spacing = spacing;
    set.margin = padding;
    tile_data(path, &place, def, &mut set)?;
    out.push((def.uid, Arc::new(set)));
  }
  Ok(out)
}

/// The tiles' data from the tileset's enum tags and custom data, read as
/// Tiled's tile classes and properties are: a tagged tile's class is its
/// first tag in the enum's order and each of its tags a bool property true
/// (a tile may have several), and its custom data (LDtk's one free text a
/// tile) the string property `custom_data`
fn tile_data(
  path: &AssetPath,
  place: &str,
  def: &RawTilesetDef,
  set: &mut Tileset,
) -> Result<(), AssetError> {
  let id_of = |id: i64| {
    u32::try_from(id)
      .ok()
      .filter(|&id| id < set.tile_count)
      .ok_or_else(|| {
        fail(
          path,
          place,
          format!("tile {id} is not one of its {} tiles", set.tile_count),
        )
      })
  };
  let mut data: FastMap<u32, TileData> = FastMap::default();
  for tag in &def.enum_tags {
    for &id in &tag.tile_ids {
      let tile = data.entry(id_of(id)?).or_default();
      if tile.class.is_none() {
        tile.class = Some(tag.enum_value_id.clone());
      }
      tile
        .properties
        .insert(tag.enum_value_id.clone(), PropertyValue::Bool(true));
    }
  }
  for custom in def.custom_data.iter().filter(|c| !c.data.is_empty()) {
    data
      .entry(id_of(custom.tile_id)?)
      .or_default()
      .properties
      .insert("custom_data", PropertyValue::String(custom.data.clone()));
  }
  for (id, tile) in data {
    set.insert_tile(id, tile);
  }
  Ok(())
}

// ---- fields ---------------------------------------------------------------

fn field_fail(
  path: &AssetPath,
  place: &str,
  field: &RawField,
  what: impl std::fmt::Display,
) -> AssetError {
  fail(
    path,
    &format!("{place}, field {:?}", field.identifier),
    what,
  )
}

fn grid_point(v: &Value) -> Option<PropertyValue> {
  let cx = v.get("cx")?.as_i64()?;
  let cy = v.get("cy")?.as_i64()?;
  let mut fields = Properties::default();
  fields.insert("cx", PropertyValue::Int(cx));
  fields.insert("cy", PropertyValue::Int(cy));
  Some(PropertyValue::Class {
    ty: "Point".into(),
    fields,
  })
}

/// One value of type `ty` (arrays are cut apart by the caller); `Ok(None)` for
/// a value that is unset
fn scalar(defs: &Defs<'_>, ty: &str, v: &Value) -> Result<Option<PropertyValue>, String> {
  if v.is_null() {
    return Ok(None);
  }
  let bad = |what: &str| format!("the value {v} is not {what}");
  Ok(Some(match ty {
    "Int" => match v.as_i64() {
      Some(i) => PropertyValue::Int(i),
      None => match v.as_f64() {
        Some(f) if f.fract() == 0.0 && f.abs() < 9.0e15 => PropertyValue::Int(f as i64),
        _ => return Err(bad("an integer")),
      },
    },
    "Float" => PropertyValue::Float(v.as_f64().ok_or_else(|| bad("a number"))?),
    "Bool" => PropertyValue::Bool(v.as_bool().ok_or_else(|| bad("a bool"))?),
    "String" | "Multilines" | "Text" => {
      PropertyValue::String(v.as_str().ok_or_else(|| bad("a string"))?.to_string())
    }
    "FilePath" => PropertyValue::File(v.as_str().ok_or_else(|| bad("a string"))?.to_string()),
    "Color" => PropertyValue::Color(
      v.as_str()
        .and_then(parse_color)
        .ok_or_else(|| bad("a color like \"#D181E8\""))?,
    ),
    "Point" => grid_point(v).ok_or_else(|| bad("a point ({\"cx\", \"cy\"})"))?,
    "Tile" => {
      let rect: RawRect = serde_json::from_value(v.clone()).map_err(|_| bad("a tile rectangle"))?;
      let mut fields = Properties::default();
      let name = defs
        .tilesets
        .get(&rect.tileset_uid)
        .map_or_else(|| rect.tileset_uid.to_string(), |t| t.set.name.clone());
      fields.insert("tileset", PropertyValue::String(name));
      for (key, n) in [("x", rect.x), ("y", rect.y), ("w", rect.w), ("h", rect.h)] {
        fields.insert(key, PropertyValue::Int(n as i64));
      }
      PropertyValue::Class {
        ty: "Tile".into(),
        fields,
      }
    }
    "EntityRef" => match v.get("entityIid").and_then(Value::as_str) {
      Some(iid) if !iid.is_empty() => PropertyValue::Object(iid.to_string()),
      _ => return Ok(None),
    },
    other => {
      let named = other
        .strip_prefix("LocalEnum.")
        .or_else(|| other.strip_prefix("ExternEnum."))
        .or_else(|| {
          other
            .strip_prefix("Enum(")
            .or_else(|| other.strip_prefix("ExternEnum("))
            .and_then(|r| r.strip_suffix(')'))
        });
      let Some(name) = named else {
        return Err(format!("the field type {other:?} is not known"));
      };
      PropertyValue::Enum {
        ty: name.to_string(),
        value: v.as_str().ok_or_else(|| bad("a string"))?.to_string(),
      }
    }
  }))
}

/// A field's value; `None` when it is unset, which leaves the property out
fn field_value(
  defs: &Defs<'_>,
  place: &str,
  field: &RawField,
) -> Result<Option<PropertyValue>, AssetError> {
  let fail_with = |what: String| field_fail(defs.path, place, field, what);
  if let Some(inner) = field
    .ty
    .strip_prefix("Array<")
    .and_then(|t| t.strip_suffix('>'))
  {
    let items = match &field.value {
      Value::Null => return Ok(None),
      Value::Array(items) => items,
      other => return Err(fail_with(format!("the value {other} is not an array"))),
    };
    let mut list = Vec::with_capacity(items.len());
    for item in items {
      if let Some(value) = scalar(defs, inner, item).map_err(fail_with)? {
        list.push(value);
      }
    }
    return Ok(Some(PropertyValue::List(list)));
  }
  scalar(defs, &field.ty, &field.value).map_err(fail_with)
}

fn fields_of(defs: &Defs<'_>, place: &str, fields: &[RawField]) -> Result<Properties, AssetError> {
  let mut out = Properties::default();
  for field in fields {
    if let Some(value) = field_value(defs, place, field)? {
      out.insert(field.identifier.clone(), value);
    }
  }
  Ok(out)
}

// ---- layers ---------------------------------------------------------------

/// The tile an entity shows: its rectangle, if it is exactly one tile of its
/// tileset
fn entity_tile(defs: &Defs<'_>, rect: &RawRect) -> Option<Tile> {
  let t = defs.tilesets.get(&rect.tileset_uid)?;
  let set = &t.set;
  let whole = |n: f64| n.fract() == 0.0 && (0.0..4.0e9).contains(&n);
  if ![rect.x, rect.y, rect.w, rect.h].into_iter().all(whole)
    || rect.w as u32 != set.tile_size.x
    || rect.h as u32 != set.tile_size.y
    || set.columns == 0
  {
    return None;
  }
  let (x, y) = (rect.x as u32, rect.y as u32);
  let step = UVec2::new(set.tile_size.x + set.spacing, set.tile_size.y + set.spacing);
  let (ox, oy) = (x.checked_sub(set.margin)?, y.checked_sub(set.margin)?);
  if ox % step.x != 0 || oy % step.y != 0 {
    return None;
  }
  let id = (oy / step.y)
    .checked_mul(set.columns)?
    .checked_add(ox / step.x)?;
  (id < set.tile_count).then(|| Tile::new(t.index, id))
}

fn entity(defs: &Defs<'_>, place: &str, raw: &RawEntity) -> Result<MapObjectData, AssetError> {
  let place = format!("{place}, entity {}", raw.iid);
  let size = Vec2::new(raw.width as f32, raw.height as f32);
  let tile = raw.tile.as_ref().and_then(|r| entity_tile(defs, r));
  // A tile object draws stretched over its box: as LDtk draws `Stretch`,
  // and any mode when the box is the tile's size (or, fitting or covering,
  // its shape)
  if let (Some(_), Some(rect)) = (tile, raw.tile.as_ref()) {
    let mode = defs
      .tile_modes
      .get(&raw.def_uid)
      .copied()
      .unwrap_or("FitInside");
    let drawn = Vec2::new(rect.w as f32, rect.h as f32);
    let same_shape = (size.x * drawn.y - size.y * drawn.x).abs() < 1e-3;
    let alike = match mode {
      "Stretch" => true,
      "FitInside" | "Cover" => same_shape,
      _ => size == drawn,
    };
    if !alike {
      defs.unread.borrow_mut().note(
        format!("an entity's tile drawn {mode:?} in LDtk draws stretched over the entity's box"),
        &place,
      );
    }
  }
  // LDtk's px is where the pivot lies; the map wants the top-left
  let position = Vec2::new(raw.px[0] as f32, raw.px[1] as f32)
    - Vec2::new(raw.pivot[0] as f32, raw.pivot[1] as f32) * size;
  Ok(MapObjectData {
    id: raw.iid.clone(),
    name: raw.identifier.clone(),
    class: Some(raw.identifier.clone()).filter(|c| !c.is_empty()),
    position,
    size,
    rotation: 0.0,
    shape: ObjectShape::Rect,
    tile,
    visible: true,
    properties: fields_of(defs, &place, &raw.field_instances)?,
  })
}

/// Most layers one LDtk layer splits into: its own and the sub-layers its
/// stacked, offset and translucent tiles need. Each sub-layer is a grid the
/// size of the level, drawn chunk by chunk, so the cap bounds a layer's
/// cells and draws at 16 times a plain layer's (a 1024 x 1024 layer's at
/// 64 MiB). LDtk's bundled samples ask for up to 44 in a rule-heavy layer;
/// past 16 the tiles left over are at most 28 of a layer's thousands, the
/// deepest stacks and rarest offsets, which then draw in the nearest
/// sub-layer
const MAX_SUBLAYERS: usize = 16;

/// `1.0f32`'s bits: an opaque tile
const OPAQUE: u32 = 0x3F80_0000;

/// What puts a tile in a sub-layer of its own: how deep it is stacked in its
/// cell (the first tile LDtk lists there is 0), where in the cell it is
/// drawn (a rule's offset), and its alpha (a rule's opacity)
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
struct SubKey {
  depth: u32,
  offset: IVec2,
  alpha: u32,
}

impl SubKey {
  /// The layer's own: the first tile in a cell, on the grid, opaque
  const BASE: SubKey = SubKey {
    depth: 0,
    offset: IVec2::ZERO,
    alpha: OPAQUE,
  };

  fn alpha(self) -> f32 {
    f32::from_bits(self.alpha)
  }
}

/// A layer's tiles sorted into sub-layers, before any grid is allocated
struct TilePlan {
  /// The sub-layers' keys in drawing order, the layer's own first
  keys: Vec<SubKey>,
  /// Each tile in the level, in the order LDtk lists them: its index in the
  /// layer's tiles, its cell and its sub-layer
  placed: Vec<(usize, IVec2, usize)>,
  /// The sub-layers the tiles asked for, and how many tiles past the cap
  /// went to the nearest kept one
  needed: usize,
  snapped: usize,
}

/// Sorts `tiles` (in LDtk's display order, the first beneath the next) into
/// the sub-layers of a layer of `size` cells of `grid` pixels: one per
/// distinct (stack depth, offset in the cell, alpha), the layer's own first,
/// then by depth and, within a depth, as LDtk lists them, so each tile draws
/// over the ones listed before it in its cell. Past [`MAX_SUBLAYERS`] the
/// keys holding the most tiles are kept, and each other tile goes to the
/// kept sub-layer whose cell is free and whose offset, depth and alpha are
/// nearest. Tiles whose cell is outside the level (auto-tiles of stamps
/// reach past its edge) are left out
fn plan_tiles(size: UVec2, grid: u32, tiles: &[&RawTile]) -> TilePlan {
  let g = i64::from(grid.max(1));
  let mut depth: FastMap<IVec2, u32> = FastMap::default();
  let mut keyed = Vec::with_capacity(tiles.len());
  // each key's tiles and the index of its first
  let mut counts: FastMap<SubKey, (usize, usize)> = FastMap::default();
  for (i, tile) in tiles.iter().enumerate() {
    let (px, py) = (tile.px[0], tile.px[1]);
    let (cx, cy) = (px.div_euclid(g), py.div_euclid(g));
    if cx < 0 || cy < 0 || cx >= i64::from(size.x) || cy >= i64::from(size.y) {
      continue;
    }
    let cell = IVec2::new(cx as i32, cy as i32);
    let stacked = depth.entry(cell).or_insert(0);
    let alpha = if tile.a.is_finite() {
      tile.a.clamp(0.0, 1.0) as f32
    } else {
      1.0
    };
    let key = SubKey {
      depth: *stacked,
      offset: IVec2::new(px.rem_euclid(g) as i32, py.rem_euclid(g) as i32),
      alpha: alpha.to_bits(),
    };
    *stacked = stacked.saturating_add(1);
    counts.entry(key).or_insert((0, i)).0 += 1;
    keyed.push((i, cell, key));
  }
  let needed =
    counts.len().max(1) + usize::from(!counts.is_empty() && !counts.contains_key(&SubKey::BASE));
  let mut others: Vec<(SubKey, usize, usize)> = counts
    .iter()
    .filter(|(key, _)| **key != SubKey::BASE)
    .map(|(key, &(count, first))| (*key, count, first))
    .collect();
  others.sort_by(|a, b| b.1.cmp(&a.1).then(a.2.cmp(&b.2)));
  others.truncate(MAX_SUBLAYERS - 1);
  others.sort_by_key(|&(key, _, first)| (key.depth, first));
  let mut keys = vec![SubKey::BASE];
  keys.extend(others.iter().map(|&(key, ..)| key));
  let index: FastMap<SubKey, usize> = keys.iter().enumerate().map(|(i, k)| (*k, i)).collect();
  let mut placed = Vec::with_capacity(keyed.len());
  let mut taken: FastSet<(usize, IVec2)> = FastSet::default();
  let mut rest = Vec::new();
  for &(i, cell, key) in &keyed {
    match index.get(&key) {
      Some(&sub) => {
        taken.insert((sub, cell));
        placed.push((i, cell, sub));
      }
      None => rest.push((i, cell, key)),
    }
  }
  let snapped = rest.len();
  for (i, cell, key) in rest {
    let sub = nearest(&keys, key, |sub| taken.contains(&(sub, cell)));
    taken.insert((sub, cell));
    placed.push((i, cell, sub));
  }
  // as LDtk lists them, so where two share a cell the later wins
  placed.sort_unstable_by_key(|&(i, ..)| i);
  TilePlan {
    keys,
    placed,
    needed,
    snapped,
  }
}

/// The kept sub-layer nearest `key`: one whose cell is free first, then the
/// nearest offset (where the tile draws), depth (what it draws over) and
/// alpha
fn nearest(keys: &[SubKey], key: SubKey, taken: impl Fn(usize) -> bool) -> usize {
  (0..keys.len())
    .min_by_key(|&sub| {
      let k = keys[sub];
      let offset = (k.offset - key.offset).abs().element_sum();
      let alpha = ((k.alpha() - key.alpha()).abs() * 255.0) as u32;
      (taken(sub), offset, k.depth.abs_diff(key.depth), alpha)
    })
    .unwrap_or(0)
}

/// A sub-layer's editor id: its layer's uid and its key, hashed (FNV-1a),
/// with the top bit set, which no LDtk uid has, so a reload matches it to
/// the sub-layer of the same tiles wherever it now sits
fn sub_layer_id(uid: u32, key: SubKey) -> u32 {
  let mut h: u32 = 0x811c_9dc5;
  for word in [
    uid,
    key.depth,
    key.offset.x as u32,
    key.offset.y as u32,
    key.alpha,
  ] {
    for byte in word.to_le_bytes() {
      h ^= u32::from(byte);
      h = h.wrapping_mul(0x0100_0193);
    }
  }
  h | 0x8000_0000
}

/// Fills a grid of `size` cells for each of the plan's sub-layers from
/// LDtk's tiles. Returns the grids and the highest id each uses of the
/// tileset
fn fill_grids(
  defs: &Defs<'_>,
  place: &str,
  size: UVec2,
  tileset: Option<&TilesetRef>,
  (tiles, plan): (&[&RawTile], &TilePlan),
) -> Result<(Vec<TileGrid>, Vec<Option<u32>>), AssetError> {
  let Some(tileset) = tileset else {
    if tiles.is_empty() {
      return Ok((vec![TileGrid::new(size)], vec![None]));
    }
    return Err(fail(defs.path, place, "the layer has tiles but no tileset"));
  };
  let mut grids: Vec<TileGrid> = plan.keys.iter().map(|_| TileGrid::new(size)).collect();
  let mut highest = vec![None; plan.keys.len()];
  for &(i, cell, sub) in &plan.placed {
    let tile = tiles[i];
    let id = u32::try_from(tile.t)
      .ok()
      .filter(|&id| id < MAX_TILES)
      .ok_or_else(|| {
        fail(
          defs.path,
          place,
          format!(
            "tile at pixel ({}, {}): tile id {} is past the {MAX_TILES} tiles a tileset holds",
            tile.px[0], tile.px[1], tile.t
          ),
        )
      })?;
    // bit 0 mirrors left to right, bit 1 top to bottom
    let mut flip = TileFlip::NONE;
    if tile.f & 1 != 0 {
      flip = flip | TileFlip::HORIZONTAL;
    }
    if tile.f & 2 != 0 {
      flip = flip | TileFlip::VERTICAL;
    }
    if let (Some(grid), Some(slot)) = (grids.get_mut(sub), highest.get_mut(sub)) {
      grid.set(cell, Some(Tile::new(tileset.index, id).with_flip(flip)));
      *slot = Some(slot.map_or(id, |m: u32| m.max(id)));
    }
  }
  // a loaded grid starts clean
  for grid in &mut grids {
    grid.drain_dirty(&mut Vec::new());
  }
  Ok((grids, highest))
}

struct LayerOut {
  layer: MapLayer,
  /// (tileset, highest id) the layer uses
  used: Option<(u16, u32)>,
}

/// A layer checked and its tiles sorted into sub-layers, before any grid is
/// allocated, so a level's cells can be counted first
struct PlannedLayer<'d> {
  raw: &'d RawLayer,
  def: &'d RawLayerDef,
  info: LayerInfo,
  place: String,
  /// Cells across and down, and a cell's pixels (zero for entities)
  size: UVec2,
  cell_size: UVec2,
  tileset: Option<&'d TilesetRef>,
  /// What it draws: its auto-tiles, or its grid tiles
  tiles: Vec<&'d RawTile>,
  plan: TilePlan,
}

impl PlannedLayer<'_> {
  /// The cells its grids take, as the engine allocates them: each grid
  /// (the layer's own and a sub-layer each, level-sized) its whole chunks
  /// (`engine_cells`), and an IntGrid's values, one a cell, once more
  fn cells(&self) -> u64 {
    if self.raw.ty == "Entities" {
      return 0;
    }
    let grids = engine_cells(self.size).saturating_mul(self.plan.keys.len() as u64);
    let values = if self.raw.ty == "IntGrid" {
      u64::from(self.size.x) * u64::from(self.size.y)
    } else {
      0
    };
    grids.saturating_add(values)
  }
}

fn plan_layer<'d>(
  defs: &'d Defs<'_>,
  size_px: UVec2,
  raw: &'d RawLayer,
) -> Result<PlannedLayer<'d>, AssetError> {
  let place = format!("layer {:?}", raw.identifier);
  let def: &RawLayerDef = defs
    .layers
    .get(&raw.layer_def_uid)
    .copied()
    .ok_or_else(|| {
      fail(
        defs.path,
        &place,
        format!(
          "its definition {} is not in the project's defs",
          raw.layer_def_uid
        ),
      )
    })?;
  let id = u32::try_from(raw.layer_def_uid)
    .map_err(|_| fail(defs.path, &place, "its definition uid is out of range"))?;
  let mut info = LayerInfo::new(id, raw.identifier.clone());
  info.offset = Vec2::new(raw.offset_x as f32, raw.offset_y as f32);
  info.opacity = raw.opacity as f32;
  info.visible = raw.visible;
  // LDtk's factor is 0 for a layer that scrolls with the world
  info.parallax = Vec2::new((1.0 - def.parallax_x) as f32, (1.0 - def.parallax_y) as f32);
  if def.parallax_scaling && (def.parallax_x != 0.0 || def.parallax_y != 0.0) {
    defs.unread.borrow_mut().note(
      "a parallax layer's scaling (LDtk zooms it by its factor) is not drawn: it moves by its factor and keeps its size",
      &place,
    );
  }
  info.collides = raw.ty == "IntGrid";
  if !matches!(
    raw.ty.as_str(),
    "IntGrid" | "AutoLayer" | "Tiles" | "Entities"
  ) {
    return Err(fail(
      defs.path,
      &place,
      format!("unknown layer type {:?}", raw.ty),
    ));
  }
  let empty = TilePlan {
    keys: vec![SubKey::BASE],
    placed: Vec::new(),
    needed: 1,
    snapped: 0,
  };
  if raw.ty == "Entities" {
    return Ok(PlannedLayer {
      raw,
      def,
      info,
      place,
      size: UVec2::ZERO,
      cell_size: UVec2::ZERO,
      tileset: None,
      tiles: Vec::new(),
      plan: empty,
    });
  }
  // each layer counts cells of its own grid
  let g = to_u32(defs.path, &place, "__gridSize", raw.grid_size)?;
  if g == 0 {
    return Err(fail(defs.path, &place, "the grid size is 0"));
  }
  let size = UVec2::new(ceil_div(size_px.x, g), ceil_div(size_px.y, g));
  layer_cells(size).map_err(|what| fail(defs.path, &place, what))?;
  if raw.c_wid != i64::from(size.x) || raw.c_hei != i64::from(size.y) {
    return Err(fail(
      defs.path,
      &place,
      format!(
        "it is {} x {} cells but the level's {} x {} px make {} x {} of {g} px",
        raw.c_wid, raw.c_hei, size_px.x, size_px.y, size.x, size.y
      ),
    ));
  }
  let tileset = raw
    .override_tileset_uid
    .or(raw.tileset_uid)
    .and_then(|uid| defs.tilesets.get(&uid));
  let tiles: Vec<&RawTile> = match raw.ty.as_str() {
    "Tiles" => raw.grid_tiles.iter().collect(),
    _ => raw.auto_layer_tiles.iter().collect(),
  };
  let plan = plan_tiles(size, g, &tiles);
  Ok(PlannedLayer {
    raw,
    def,
    info,
    place,
    size,
    cell_size: UVec2::splat(g),
    tileset,
    tiles,
    plan,
  })
}

/// The layers a planned layer makes: its own (with the IntGrid and the
/// collision), then its sub-layers, named after it with `#1`, `#2` and on in
/// drawing order, each at its tiles' offset and alpha over the layer's
fn build_planned(defs: &Defs<'_>, planned: PlannedLayer<'_>) -> Result<Vec<LayerOut>, AssetError> {
  let PlannedLayer {
    raw,
    def,
    info,
    place,
    size,
    cell_size,
    tileset,
    tiles,
    plan,
  } = planned;
  if raw.ty == "Entities" {
    let mut objects = Vec::with_capacity(raw.entity_instances.len());
    for e in &raw.entity_instances {
      objects.push(entity(defs, &place, e)?);
    }
    return Ok(vec![LayerOut {
      layer: MapLayer {
        info,
        kind: LayerKind::Objects(objects),
      },
      used: None,
    }]);
  }
  if plan.snapped > 0 {
    crate::engine_warn!(
      module: "LDtk",
      "{}: {place}: its stacked, offset and translucent tiles ask for {} layers, past the {MAX_SUBLAYERS} a layer splits into, so {} {} in the nearest of them",
      defs.path,
      plan.needed,
      plan.snapped,
      if plan.snapped == 1 { "tile draws" } else { "tiles draw" }
    );
  }
  if raw.ty == "IntGrid" {
    let want = cell_count(size.x, size.y).unwrap_or(0);
    if raw.int_grid_csv.len() != want {
      return Err(fail(
        defs.path,
        &place,
        format!(
          "intGridCsv holds {} values for {} x {} cells",
          raw.int_grid_csv.len(),
          size.x,
          size.y
        ),
      ));
    }
  }
  let (grids, highest) = fill_grids(defs, &place, size, tileset, (&tiles, &plan))?;
  let mut out = Vec::with_capacity(grids.len());
  for (k, (grid, max)) in grids.into_iter().zip(highest).enumerate() {
    let key = plan.keys.get(k).copied().unwrap_or(SubKey::BASE);
    let mut sub = info.clone();
    let intgrid = if k == 0 && raw.ty == "IntGrid" {
      let mut names = FastMap::default();
      for value in &def.int_grid_values {
        if let Some(name) = value.identifier.as_deref().filter(|n| !n.is_empty()) {
          names.insert(value.value, name.to_string());
        }
      }
      Some(IntGrid {
        size,
        values: raw.int_grid_csv.clone(),
        names,
      })
    } else {
      None
    };
    if k > 0 {
      // Part of its layer, which it draws with: its offset and opacity are
      // its own, over the layer's
      sub.name = format!("{}#{k}", info.name);
      sub.id = sub_layer_id(info.id, key);
      sub.part_of = Some(info.id);
      sub.offset = key.offset.as_vec2();
      sub.opacity = key.alpha();
      sub.collides = false;
      sub.occludes = Some(false);
    }
    out.push(LayerOut {
      layer: MapLayer {
        info: sub,
        kind: LayerKind::Tiles {
          tiles: grid,
          intgrid,
          // LDtk's grids are square, each layer's of its own size
          grid: MapGrid::new(GridShape::Orthogonal, cell_size),
        },
      },
      used: tileset.zip(max).map(|(t, max)| (t.index, max)),
    });
  }
  Ok(out)
}

// ---- levels ---------------------------------------------------------------

/// The level's background colour: `__bgColor` is the one LDtk resolved; the
/// level's own `bgColor` and the project's default are the fallbacks
fn background(defs: &Defs<'_>, place: &str, raw: &RawLevel) -> Result<Option<Color>, AssetError> {
  let written = [
    raw.bg_color.as_deref(),
    raw.own_bg_color.as_deref(),
    defs.default_bg.as_deref(),
  ]
  .into_iter()
  .flatten()
  .find(|t| !t.is_empty());
  written
    .map(|text| {
      parse_color(text).ok_or_else(|| {
        fail(
          defs.path,
          place,
          format!("the background colour {text:?} is not a color like \"#6E6F8B\""),
        )
      })
    })
    .transpose()
}

/// A level's map, from its layers (LDtk lists the top one first). `cells`
/// is the project's levels' cells so far, which this level's join before
/// any is allocated
fn build_map(
  defs: &Defs<'_>,
  tilesets: &[Arc<Tileset>],
  raw: &RawLevel,
  (layers, cells): (&[RawLayer], &mut u64),
  size_px: UVec2,
  fields: Properties,
) -> Result<TileMap, AssetError> {
  let place = format!("level {:?}", raw.identifier);
  // every layer checked and its tiles sorted first, so its cells are
  // counted (every tile layer, empty or not, a sub-layer each, an IntGrid
  // twice) before any grid is allocated
  let planned = layers
    .iter()
    .rev()
    .map(|layer| plan_layer(defs, size_px, layer))
    .collect::<Result<Vec<_>, _>>()?;
  let own = planned
    .iter()
    .map(PlannedLayer::cells)
    .fold(0, u64::saturating_add);
  check_map_cells(defs.path, &place, own)?;
  *cells = cells.saturating_add(own);
  if *cells > MAX_MAP_CELLS {
    return Err(fail(
      defs.path,
      &place,
      format!(
        "the project's levels hold {} cells up to this one, past the {MAX_MAP_CELLS} a project holds",
        *cells
      ),
    ));
  }
  // the map's own cell is the bottom tile layer's; each layer keeps its grid
  let grid_size = layers
    .iter()
    .rev()
    .find(|l| l.ty != "Entities")
    .map(|l| {
      to_u32(
        defs.path,
        &format!("layer {:?}", l.identifier),
        "__gridSize",
        l.grid_size,
      )
    })
    .transpose()?;
  let tile = match grid_size {
    Some(g) => g,
    None => to_u32(defs.path, &place, "defaultGridSize", defs.default_grid)?,
  };
  if tile == 0 {
    return Err(fail(defs.path, &place, "the grid size is 0"));
  }
  let size = UVec2::new(ceil_div(size_px.x, tile), ceil_div(size_px.y, tile));
  if cell_count(size.x, size.y).is_none() {
    return Err(fail(
      defs.path,
      &place,
      format!(
        "{} x {} cells is past the cells a map holds",
        size.x, size.y
      ),
    ));
  }
  let mut map = TileMap::new(size, UVec2::splat(tile));
  for set in tilesets {
    map.tilesets.push(TilesetSource::Embedded(set.clone()));
  }
  map.background = background(defs, &place, raw)?;
  map.properties = fields;
  // The editor's layers in its order, then the sub-layers, so a layer's
  // place (and z) stays LDtk's whatever stacking a save adds. A sub-layer's
  // id is a hash: kept apart from the layers' and each other's
  let mut ids: FastSet<u32> = planned.iter().map(|p| p.info.id).collect();
  let mut parts = Vec::new();
  for layer in planned {
    for mut out in build_planned(defs, layer)? {
      if out.layer.info.part_of.is_none() {
        if let Some((tileset, max)) = out.used {
          map.used_ids.push((map.layers.len(), tileset, max));
        }
        map.layers.push(out.layer);
        continue;
      }
      while !ids.insert(out.layer.info.id) {
        out.layer.info.id = (out.layer.info.id.wrapping_add(1) & 0x7FFF_FFFF) | 0x8000_0000;
      }
      parts.push(out);
    }
  }
  for out in parts {
    if let Some((tileset, max)) = out.used {
      map.used_ids.push((map.layers.len(), tileset, max));
    }
    map.layers.push(out.layer);
  }
  Ok(map)
}

/// The layers of `raw`: its own, or those of its `.ldtkl` file, read with the
/// project so that saving the file loads the project again
fn level_layers(
  cx: &mut LoadContext<'_>,
  path: &AssetPath,
  raw: &mut RawLevel,
) -> Result<Vec<RawLayer>, AssetError> {
  let place = format!("level {:?}", raw.identifier);
  if let Some(layers) = raw.layer_instances.take() {
    return Ok(layers);
  }
  let Some(written) = raw.external_rel_path.as_deref().filter(|p| !p.is_empty()) else {
    return Err(fail(
      path,
      &place,
      "the level has no layerInstances and no externalRelPath",
    ));
  };
  let joined = join(path, &place, written)?;
  let file = AssetPath::parse(&joined)
    .map_err(|e| fail(path, &place, format!("{written:?}: {}", e.report())))?;
  let bytes = cx
    .read(&file)
    .map_err(|e| fail(path, &place, format!("{written:?}: {}", e.report())))?;
  let mut external: RawLevel = parse(&file, &bytes)?;
  if external.iid != raw.iid {
    return Err(fail(
      path,
      &place,
      format!(
        "{written:?} is the level {:?}, not {:?}",
        external.iid, raw.iid
      ),
    ));
  }
  external
    .layer_instances
    .take()
    .ok_or_else(|| fail(path, &place, format!("{written:?} holds no layerInstances")))
}

fn direction(path: &AssetPath, place: &str, dir: &str) -> Result<NeighbourDirection, AssetError> {
  use NeighbourDirection::*;
  Ok(match dir {
    "n" => North,
    "s" => South,
    "e" => East,
    "w" => West,
    "ne" => NorthEast,
    "nw" => NorthWest,
    "se" => SouthEast,
    "sw" => SouthWest,
    ">" => Above,
    "<" => Below,
    "o" => Overlap,
    other => {
      return Err(fail(
        path,
        place,
        format!("the neighbour direction {other:?} is not known"),
      ));
    }
  })
}

/// One world's levels, built. `cells` counts the project's levels' cells
fn build_world(
  cx: &mut LoadContext<'_>,
  defs: &Defs<'_>,
  (tilesets, cells): (&[Arc<Tileset>], &mut u64),
  name: &str,
  multi: bool,
  layout: Option<&str>,
  levels: Vec<RawLevel>,
) -> Result<(WorldLayout, Vec<LdtkLevel>), AssetError> {
  let path = defs.path;
  let mut by_iid: FastMap<&str, usize> = FastMap::default();
  for (i, level) in levels.iter().enumerate() {
    by_iid.insert(level.iid.as_str(), i);
  }
  let mut neighbours: Vec<Vec<Neighbour>> = Vec::with_capacity(levels.len());
  for level in &levels {
    let place = format!("level {:?}", level.identifier);
    let mut list = Vec::with_capacity(level.neighbours.len());
    for n in &level.neighbours {
      let Some(&index) = by_iid.get(n.level_iid.as_str()) else {
        return Err(fail(
          path,
          &place,
          format!(
            "its neighbour {:?} is not a level of the world {name:?}",
            n.level_iid
          ),
        ));
      };
      list.push(Neighbour {
        level: index,
        direction: direction(path, &place, &n.dir)?,
      });
    }
    neighbours.push(list);
  }
  let mut world_levels = Vec::with_capacity(levels.len());
  let mut out = Vec::with_capacity(levels.len());
  // where the next level of a linear layout starts
  let mut next = IVec2::ZERO;
  for (mut raw, neighbours) in levels.into_iter().zip(neighbours) {
    let place = format!("level {:?}", raw.identifier);
    let size = UVec2::new(
      to_u32(path, &place, "pxWid", raw.px_wid)?,
      to_u32(path, &place, "pxHei", raw.px_hei)?,
    );
    if size.x == 0 || size.y == 0 {
      return Err(fail(path, &place, "the level has no size (pxWid, pxHei)"));
    }
    let position = match layout {
      Some("LinearHorizontal") => next,
      Some("LinearVertical") => next,
      _ => IVec2::new(
        to_i32(path, &place, "worldX", raw.world_x)?,
        to_i32(path, &place, "worldY", raw.world_y)?,
      ),
    };
    let step = |extent: u32| i32::try_from(extent).ok();
    match layout {
      Some("LinearHorizontal") => {
        next.x = step(size.x.saturating_add(LINEAR_SPACING))
          .and_then(|s| next.x.checked_add(s))
          .ok_or_else(|| {
            fail(
              path,
              &place,
              "the levels are laid out past the world's range",
            )
          })?;
      }
      Some("LinearVertical") => {
        next.y = step(size.y.saturating_add(LINEAR_SPACING))
          .and_then(|s| next.y.checked_add(s))
          .ok_or_else(|| {
            fail(
              path,
              &place,
              "the levels are laid out past the world's range",
            )
          })?;
      }
      _ => {}
    }
    let depth = to_i32(path, &place, "worldDepth", raw.world_depth)?;
    let layers = level_layers(cx, path, &mut raw)?;
    let fields = fields_of(defs, &place, &raw.field_instances)?;
    let map = build_map(
      defs,
      tilesets,
      &raw,
      (&layers, &mut *cells),
      size,
      fields.clone(),
    )?;
    world_levels.push(WorldLevel {
      name: raw.identifier.clone(),
      position: position.as_vec2(),
      size: size.as_vec2(),
      origin: position.as_vec2(),
      depth,
      neighbours: neighbours.clone(),
    });
    out.push(LdtkLevel {
      identifier: raw.identifier,
      iid: raw.iid,
      world: multi.then(|| name.to_string()),
      position,
      size,
      depth,
      fields,
      neighbours,
      map,
    });
  }
  Ok((
    WorldLayout {
      name: name.to_string(),
      levels: world_levels,
    },
    out,
  ))
}

pub(super) fn build_project(
  raw: RawProject,
  cx: &mut LoadContext<'_>,
) -> Result<LdtkProject, AssetError> {
  let path = cx.path().clone();
  let mut places = DependencyPlaces::default();
  let built = build_tilesets(&raw.defs.tilesets, cx, &path, &mut places)?;
  let tilesets: Vec<Arc<Tileset>> = built.iter().map(|(_, s)| s.clone()).collect();
  let defs = Defs {
    path: &path,
    tilesets: built
      .into_iter()
      .enumerate()
      .map(|(i, (uid, set))| {
        (
          uid,
          TilesetRef {
            index: i as u16,
            set,
          },
        )
      })
      .collect(),
    layers: raw.defs.layers.iter().map(|l| (l.uid, l)).collect(),
    tile_modes: raw
      .defs
      .entities
      .iter()
      .filter_map(|e| Some((e.uid, e.tile_render_mode.as_deref()?)))
      .collect(),
    default_grid: raw.default_grid_size.unwrap_or(16),
    default_bg: raw.default_level_bg_color.clone(),
    unread: Default::default(),
  };
  let multi = !raw.worlds.is_empty();
  let sources: Vec<(String, Option<String>, Vec<RawLevel>)> = if multi {
    raw
      .worlds
      .into_iter()
      .map(|w: RawWorld| (w.identifier, w.world_layout, w.levels))
      .collect()
  } else {
    vec![(DEFAULT_WORLD.to_string(), raw.world_layout, raw.levels)]
  };
  let mut worlds = Vec::with_capacity(sources.len());
  let mut levels = Vec::new();
  let mut cells = 0u64;
  for (name, layout, world_levels) in sources {
    let (layout, built) = build_world(
      cx,
      &defs,
      (&tilesets, &mut cells),
      &name,
      multi,
      layout.as_deref(),
      world_levels,
    )?;
    worlds.push(layout);
    levels.extend(built);
  }
  defs.unread.borrow_mut().warn(&path, "LDtk");
  Ok(LdtkProject {
    levels,
    worlds,
    tilesets,
    dependency_places: places,
  })
}

/// Whether the tilesets' images hold the tiles declared, and the tilesets the
/// ids the layers use
pub(super) fn check_project(
  project: &LdtkProject,
  cx: &CheckContext<'_>,
) -> Result<(), AssetError> {
  for set in &project.tilesets {
    check_tileset(set, cx)?;
  }
  for level in &project.levels {
    check_used_ids(&level.map, cx)?;
  }
  Ok(())
}
