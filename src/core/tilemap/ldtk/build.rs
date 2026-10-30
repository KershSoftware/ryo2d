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
use crate::core::hash::FastMap;
use crate::core::tilemap::tiled::build::{
  MAX_TILES, cell_count, check_tileset, check_used_ids, fail, join, load_dep,
};
use crate::core::tilemap::tiled::json::parse;
use crate::core::tilemap::{
  IntGrid, LayerInfo, LayerKind, MapLayer, MapObjectData, ObjectShape, Properties, PropertyValue,
  Tile, TileFlip, TileGrid, TileMap, Tileset, TilesetImage, TilesetSource,
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
  default_grid: i64,
  default_bg: Option<String>,
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
          image: load_dep::<Image>(cx, path, &place, written)?,
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
    out.push((def.uid, Arc::new(set)));
  }
  Ok(out)
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
    tile: raw.tile.as_ref().and_then(|r| entity_tile(defs, r)),
    visible: true,
    properties: fields_of(defs, &place, &raw.field_instances)?,
  })
}

/// Fills a grid of `size` cells from LDtk's tiles. `grid_size` is the layer's
/// cell in pixels. Returns the highest id used of each tileset
fn fill_grid(
  defs: &Defs<'_>,
  place: &str,
  size: UVec2,
  grid_size: u32,
  tileset: Option<&TilesetRef>,
  tiles: &[&RawTile],
) -> Result<(TileGrid, Option<u32>), AssetError> {
  let mut grid = TileGrid::new(size);
  let mut highest = None;
  for tile in tiles {
    let Some(tileset) = tileset else {
      return Err(fail(defs.path, place, "the layer has tiles but no tileset"));
    };
    let at = || format!("tile at pixel ({}, {})", tile.px[0], tile.px[1]);
    let (px, py) = (tile.px[0], tile.px[1]);
    // auto-tiles of stamps and rules may reach past the level's edge: those
    // cells are not in the map
    let outside = px < 0
      || py < 0
      || px / i64::from(grid_size) >= i64::from(size.x)
      || py / i64::from(grid_size) >= i64::from(size.y);
    if outside {
      continue;
    }
    let cell = IVec2::new(
      (px / i64::from(grid_size)) as i32,
      (py / i64::from(grid_size)) as i32,
    );
    let id = u32::try_from(tile.t)
      .ok()
      .filter(|&id| id < MAX_TILES)
      .ok_or_else(|| {
        fail(
          defs.path,
          place,
          format!(
            "{}: tile id {} is past the {MAX_TILES} tiles a tileset holds",
            at(),
            tile.t
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
    // tiles stacked in a cell come in drawing order: the last one wins
    grid.set(cell, Some(Tile::new(tileset.index, id).with_flip(flip)));
    highest = Some(highest.map_or(id, |m: u32| m.max(id)));
  }
  // a loaded grid starts clean
  grid.drain_dirty(&mut Vec::new());
  Ok((grid, highest))
}

struct LayerOut {
  layer: MapLayer,
  /// (tileset, highest id) the layer uses
  used: Option<(u16, u32)>,
}

fn build_layer(defs: &Defs<'_>, size_px: UVec2, raw: &RawLayer) -> Result<LayerOut, AssetError> {
  let place = format!("layer {:?}", raw.identifier);
  let def = defs
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
  info.collides = false;
  // each layer counts cells of its own grid
  let (size, cell_size) = if raw.ty == "Entities" {
    (UVec2::ZERO, UVec2::ZERO)
  } else {
    let g = to_u32(defs.path, &place, "__gridSize", raw.grid_size)?;
    if g == 0 {
      return Err(fail(defs.path, &place, "the grid size is 0"));
    }
    let size = UVec2::new(ceil_div(size_px.x, g), ceil_div(size_px.y, g));
    if cell_count(size.x, size.y).is_none() {
      return Err(fail(
        defs.path,
        &place,
        format!(
          "{} x {} cells is past the cells a layer holds",
          size.x, size.y
        ),
      ));
    }
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
    (size, UVec2::splat(g))
  };
  let tileset = raw
    .override_tileset_uid
    .or(raw.tileset_uid)
    .and_then(|uid| defs.tilesets.get(&uid));
  let tiled = |tiles: &[&RawTile], used: &mut Option<(u16, u32)>| -> Result<TileGrid, AssetError> {
    let grid_size = to_u32(defs.path, &place, "__gridSize", raw.grid_size)?;
    let (grid, highest) = fill_grid(defs, &place, size, grid_size, tileset, tiles)?;
    *used = tileset.zip(highest).map(|(t, max)| (t.index, max));
    Ok(grid)
  };
  let mut used = None;
  let kind = match raw.ty.as_str() {
    "IntGrid" => {
      info.collides = true;
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
      let auto: Vec<&RawTile> = raw.auto_layer_tiles.iter().collect();
      let grid = tiled(&auto, &mut used)?;
      let mut names = FastMap::default();
      for value in &def.int_grid_values {
        if let Some(name) = value.identifier.as_deref().filter(|n| !n.is_empty()) {
          names.insert(value.value, name.to_string());
        }
      }
      LayerKind::Tiles {
        grid,
        intgrid: Some(IntGrid {
          size,
          values: raw.int_grid_csv.clone(),
          names,
        }),
        cell_size,
      }
    }
    "AutoLayer" => {
      let auto: Vec<&RawTile> = raw.auto_layer_tiles.iter().collect();
      LayerKind::Tiles {
        grid: tiled(&auto, &mut used)?,
        intgrid: None,
        cell_size,
      }
    }
    "Tiles" => {
      let tiles: Vec<&RawTile> = raw.grid_tiles.iter().collect();
      LayerKind::Tiles {
        grid: tiled(&tiles, &mut used)?,
        intgrid: None,
        cell_size,
      }
    }
    "Entities" => {
      let mut objects = Vec::with_capacity(raw.entity_instances.len());
      for e in &raw.entity_instances {
        objects.push(entity(defs, &place, e)?);
      }
      LayerKind::Objects(objects)
    }
    other => {
      return Err(fail(
        defs.path,
        &place,
        format!("unknown layer type {other:?}"),
      ));
    }
  };
  Ok(LayerOut {
    layer: MapLayer { info, kind },
    used,
  })
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

/// A level's map, from its layers (LDtk lists the top one first)
fn build_map(
  defs: &Defs<'_>,
  tilesets: &[Arc<Tileset>],
  raw: &RawLevel,
  layers: &[RawLayer],
  size_px: UVec2,
  fields: Properties,
) -> Result<TileMap, AssetError> {
  let place = format!("level {:?}", raw.identifier);
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
  for layer in layers.iter().rev() {
    let out = build_layer(defs, size_px, layer)?;
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

/// One world's levels, built
fn build_world(
  cx: &mut LoadContext<'_>,
  defs: &Defs<'_>,
  tilesets: &[Arc<Tileset>],
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
    let layers = level_layers(cx, path, &mut raw)?;
    let fields = fields_of(defs, &place, &raw.field_instances)?;
    let map = build_map(defs, tilesets, &raw, &layers, size, fields.clone())?;
    world_levels.push(WorldLevel {
      name: raw.identifier.clone(),
      position: position.as_vec2(),
      size: size.as_vec2(),
      neighbours: neighbours.clone(),
    });
    out.push(LdtkLevel {
      identifier: raw.identifier,
      iid: raw.iid,
      world: multi.then(|| name.to_string()),
      position,
      size,
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
  let built = build_tilesets(&raw.defs.tilesets, cx, &path)?;
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
    default_grid: raw.default_grid_size.unwrap_or(16),
    default_bg: raw.default_level_bg_color.clone(),
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
  for (name, layout, world_levels) in sources {
    let (layout, built) = build_world(
      cx,
      &defs,
      &tilesets,
      &name,
      multi,
      layout.as_deref(),
      world_levels,
    )?;
    worlds.push(layout);
    levels.extend(built);
  }
  Ok(LdtkProject {
    levels,
    worlds,
    tilesets,
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
