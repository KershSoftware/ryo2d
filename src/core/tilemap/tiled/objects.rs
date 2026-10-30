//! The objects of an object layer: every shape, tile objects placed by their
//! tileset's alignment, and instances of templates.
//!
//! Every object is stored the same way: `position` is the top-left corner of
//! its rectangle (the origin of its points, for a polygon or polyline) in map
//! pixels, and `rotation` turns it about that corner. Tiled rotates about the
//! object's own position, which for a tile object is the alignment point
//! (bottom-left unless the tileset says otherwise), so a rotated tile object's
//! position moves to where its top-left corner ends up: the same picture,
//! and one rule for every object.

use std::collections::HashMap;
use std::sync::Arc;

use glam::Vec2;

use super::build::{MAX_TILES, join};
use super::data::decode_gid;
use super::project::Props;
use super::raw::{RawObject, RawPoint, RawProperty, RawTemplate, RawTileset};
use crate::core::asset::{AssetError, AssetPath, LoadContext};
use crate::core::tilemap::{MapObjectData, ObjectShape, Tile};

/// Where a tile object's position sits on its picture
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Alignment {
  TopLeft,
  Top,
  TopRight,
  Left,
  Center,
  Right,
  BottomLeft,
  Bottom,
  BottomRight,
}

impl Alignment {
  /// The alignment Tiled's `objectalignment` names; an unspecified one is
  /// the bottom left, which is Tiled's for orthogonal maps
  pub(super) fn parse(text: Option<&str>) -> Option<Alignment> {
    Some(match text {
      None | Some("" | "unspecified" | "bottomleft") => Alignment::BottomLeft,
      Some("topleft") => Alignment::TopLeft,
      Some("top") => Alignment::Top,
      Some("topright") => Alignment::TopRight,
      Some("left") => Alignment::Left,
      Some("center") => Alignment::Center,
      Some("right") => Alignment::Right,
      Some("bottom") => Alignment::Bottom,
      Some("bottomright") => Alignment::BottomRight,
      _ => return None,
    })
  }

  /// The point as a fraction of the picture's width and height
  fn anchor(self) -> Vec2 {
    match self {
      Alignment::TopLeft => Vec2::new(0.0, 0.0),
      Alignment::Top => Vec2::new(0.5, 0.0),
      Alignment::TopRight => Vec2::new(1.0, 0.0),
      Alignment::Left => Vec2::new(0.0, 0.5),
      Alignment::Center => Vec2::new(0.5, 0.5),
      Alignment::Right => Vec2::new(1.0, 0.5),
      Alignment::BottomLeft => Vec2::new(0.0, 1.0),
      Alignment::Bottom => Vec2::new(0.5, 1.0),
      Alignment::BottomRight => Vec2::new(1.0, 1.0),
    }
  }
}

/// What the objects need to know of one of the map's tilesets
pub(super) struct TilesetEntry {
  /// Where the tileset's file is, for an external one
  pub path: Option<String>,
  /// Known at once for an embedded tileset, read from the file for an
  /// external one the first time a tile object needs it
  pub alignment: Option<Alignment>,
}

/// A map's objects as they are read: its tilesets, and the templates met so far
pub(super) struct ObjectState {
  pub tilesets: Vec<TilesetEntry>,
  templates: HashMap<String, Arc<RawTemplate>>,
}

impl ObjectState {
  pub(super) fn new(tilesets: Vec<TilesetEntry>) -> ObjectState {
    ObjectState {
      tilesets,
      templates: HashMap::new(),
    }
  }
}

/// The alignment `objectalignment` of the tileset file at `path`, which the
/// map holds as a dependency and so cannot look inside while it loads
fn read_alignment(
  cx: &LoadContext<'_>,
  from: &AssetPath,
  place: &str,
  path: &str,
) -> Result<Alignment, AssetError> {
  let Ok(asset) = AssetPath::parse(path) else {
    return Ok(Alignment::BottomLeft);
  };
  // only the JSON tileset can be looked into so far
  if !asset.as_str().ends_with(".tsj") {
    return Ok(Alignment::BottomLeft);
  }
  let bytes = cx
    .read(&asset)
    .map_err(|e| AssetError::decode(from, format!("{place}: {}", e.report())))?;
  let raw: RawTileset = serde_json::from_slice(&bytes)
    .map_err(|e| AssetError::decode(from, format!("{place}: {path:?}: {e}")))?;
  Alignment::parse(raw.objectalignment.as_deref()).ok_or_else(|| {
    AssetError::decode(
      from,
      format!(
        "{place}: {path:?}: objectalignment {:?} is not one Tiled has",
        raw.objectalignment.unwrap_or_default()
      ),
    )
  })
}

/// The template at `written`, read and kept
fn template(
  state: &mut ObjectState,
  cx: &LoadContext<'_>,
  px: &Props<'_>,
  place: &str,
  written: &str,
) -> Result<(Arc<RawTemplate>, String), AssetError> {
  let joined = join(px.path(), place, written)?;
  let asset = AssetPath::parse(&joined)
    .map_err(|e| px.fail(place, format!("the template {written:?}: {}", e.report())))?;
  let key = asset.as_str().to_string();
  if let Some(found) = state.templates.get(&key) {
    return Ok((found.clone(), key));
  }
  if !key.ends_with(".tj") && !key.ends_with(".json") {
    return Err(px.fail(
      place,
      format!(
        "the template {written:?} is not a .tj file; Tiled's XML templates load with the XML reader"
      ),
    ));
  }
  let bytes = cx
    .read(&asset)
    .map_err(|e| px.fail(place, format!("the template {written:?}: {}", e.report())))?;
  let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(&bytes);
  let raw: RawTemplate = serde_json::from_slice(bytes).map_err(|e| {
    px.fail(
      place,
      format!(
        "the template {written:?}: line {}, column {}: {e}",
        e.line(),
        e.column()
      ),
    )
  })?;
  let raw = Arc::new(raw);
  state.templates.insert(key.clone(), raw.clone());
  Ok((raw, key))
}

fn points(raw: &[RawPoint]) -> Vec<Vec2> {
  raw
    .iter()
    .map(|p| Vec2::new(p.x as f32, p.y as f32))
    .collect()
}

/// `base`'s properties with `over`'s laid on them by name
fn merge_properties(base: &[RawProperty], over: &[RawProperty]) -> Vec<RawProperty> {
  let mut out: Vec<RawProperty> = base.to_vec();
  for p in over {
    match out.iter_mut().find(|q| q.name == p.name) {
      Some(slot) => *slot = p.clone(),
      None => out.push(p.clone()),
    }
  }
  out
}

fn has_shape(o: &RawObject) -> bool {
  o.ellipse
    || o.point
    || o.capsule
    || o.polygon.is_some()
    || o.polyline.is_some()
    || o.text.is_some()
}

/// The objects of the layer `place` names. `first_gids` are the map's
/// tilesets' first gids and indexes, sorted; `highest` collects the highest
/// tile id used of each tileset
pub(super) fn build_objects(
  state: &mut ObjectState,
  first_gids: &[(u32, u16)],
  cx: &LoadContext<'_>,
  px: &mut Props<'_>,
  place: &str,
  raw: Vec<RawObject>,
  highest: &mut [Option<u32>],
) -> Result<Vec<MapObjectData>, AssetError> {
  let mut out = Vec::with_capacity(raw.len());
  for object in raw {
    out.push(build_object(
      state, first_gids, cx, px, place, object, highest,
    )?);
  }
  Ok(out)
}

fn build_object(
  state: &mut ObjectState,
  first_gids: &[(u32, u16)],
  cx: &LoadContext<'_>,
  px: &mut Props<'_>,
  layer: &str,
  mut o: RawObject,
  highest: &mut [Option<u32>],
) -> Result<MapObjectData, AssetError> {
  let place = format!("{layer}, object {}", o.id);
  // an object of a template starts as the template's, and its own fields win
  let mut template_tile: Option<(u32, String)> = None;
  let mut template_set = None;
  if let Some(written) = o.template.take().filter(|t| !t.is_empty()) {
    let (tpl, key) = template(state, cx, px, &place, &written)?;
    let base = &tpl.object;
    if let Some(entry) = &tpl.tileset {
      let Some(source) = entry.source.as_deref() else {
        return Err(px.fail(
          &place,
          format!("the template {written:?} holds its tileset inline, which is not supported"),
        ));
      };
      let from = AssetPath::parse(&key).map_err(|e| px.fail(&place, e.report()))?;
      let joined = join(&from, &place, source)?;
      let normal = AssetPath::parse(&joined)
        .map_err(|e| px.fail(&place, format!("{source:?}: {}", e.report())))?;
      template_set = Some((entry.firstgid, normal.as_str().to_string()));
    }
    o.name = o.name.or_else(|| base.name.clone());
    o.class = o.class.or_else(|| base.class.clone());
    o.x = o.x.or(base.x);
    o.y = o.y.or(base.y);
    o.width = o.width.or(base.width);
    o.height = o.height.or(base.height);
    o.rotation = o.rotation.or(base.rotation);
    o.visible = o.visible.or(base.visible);
    if !has_shape(&o) {
      o.ellipse = base.ellipse;
      o.point = base.point;
      o.capsule = base.capsule;
      o.polygon = base.polygon.clone();
      o.polyline = base.polyline.clone();
      o.text = base.text.clone();
    }
    if o.gid.is_none() {
      template_tile = base.gid.map(|g| (g, written));
    }
    o.properties = merge_properties(&base.properties, &o.properties);
  }

  let shape = if let Some(text) = &o.text {
    ObjectShape::Text(text.text.clone())
  } else if let Some(pts) = &o.polygon {
    ObjectShape::Polygon(points(pts))
  } else if let Some(pts) = &o.polyline {
    ObjectShape::Polyline(points(pts))
  } else if o.point {
    ObjectShape::Point
  } else if o.ellipse {
    ObjectShape::Ellipse
  } else if o.capsule {
    ObjectShape::Capsule
  } else {
    ObjectShape::Rect
  };

  // which tile, and which tileset it is in, for a tile object
  let mut tile = None;
  let mut tileset_index = 0;
  if let Some(raw_gid) = o.gid {
    let (gid, flip) = decode_gid(raw_gid);
    let k = first_gids.partition_point(|&(first, _)| first <= gid);
    let Some(&(first, index)) = k.checked_sub(1).and_then(|k| first_gids.get(k)) else {
      return Err(px.fail(&place, format!("gid {gid} is outside every tileset")));
    };
    tile = Some((Tile::new(index, gid - first).with_flip(flip), index));
  } else if let Some((raw_gid, written)) = template_tile {
    let Some((first, set_path)) = &template_set else {
      return Err(px.fail(
        &place,
        format!("the template {written:?} has a tile but no tileset"),
      ));
    };
    let (gid, flip) = decode_gid(raw_gid);
    let Some(index) = state
      .tilesets
      .iter()
      .position(|t| t.path.as_deref() == Some(set_path.as_str()))
    else {
      return Err(px.fail(
        &place,
        format!("the template's tileset {set_path:?} is not one of the map's tilesets"),
      ));
    };
    let Some(id) = gid.checked_sub(*first) else {
      return Err(px.fail(
        &place,
        format!("gid {gid} is below its tileset's first gid {first}"),
      ));
    };
    tile = Some((Tile::new(index as u16, id).with_flip(flip), index as u16));
  }
  if let Some((t, index)) = &mut tile {
    if t.id >= MAX_TILES {
      return Err(px.fail(
        &place,
        format!(
          "tile id {} is past the {MAX_TILES} tiles a tileset holds",
          t.id
        ),
      ));
    }
    if let Some(slot) = highest.get_mut(usize::from(*index)) {
      *slot = Some(slot.map_or(t.id, |m| m.max(t.id)));
    }
    tileset_index = usize::from(*index);
  }

  let mut position = Vec2::new(o.x.unwrap_or(0.0) as f32, o.y.unwrap_or(0.0) as f32);
  let size = Vec2::new(
    o.width.unwrap_or(0.0) as f32,
    o.height.unwrap_or(0.0) as f32,
  );
  let rotation = (o.rotation.unwrap_or(0.0) as f32).to_radians();
  if tile.is_some() {
    // the position is the alignment point: move it to the top-left corner
    let alignment = match state.tilesets.get(tileset_index) {
      Some(TilesetEntry {
        alignment: Some(a), ..
      }) => *a,
      Some(TilesetEntry {
        path: Some(path),
        alignment: None,
      }) => {
        let path = path.clone();
        let a = read_alignment(cx, px.path(), &place, &path)?;
        if let Some(entry) = state.tilesets.get_mut(tileset_index) {
          entry.alignment = Some(a);
        }
        a
      }
      _ => Alignment::BottomLeft,
    };
    let from_top_left = -alignment.anchor() * size;
    let (sin, cos) = rotation.sin_cos();
    position += Vec2::new(
      from_top_left.x * cos - from_top_left.y * sin,
      from_top_left.x * sin + from_top_left.y * cos,
    );
  }

  let name = o.name.unwrap_or_default();
  let class = o.class.filter(|c| !c.is_empty());
  let properties = px.read(&place, &o.properties, class.as_deref())?;
  Ok(MapObjectData {
    id: o.id.to_string(),
    name,
    class,
    position,
    size,
    rotation,
    shape,
    tile: tile.map(|(t, _)| t),
    visible: o.visible.unwrap_or(true),
    properties,
  })
}
