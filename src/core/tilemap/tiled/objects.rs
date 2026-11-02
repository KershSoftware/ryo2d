//! The objects of an object layer: every shape, tile objects placed by their
//! tileset's alignment, and instances of templates.
//!
//! Every object is stored the same way: `position` is the top-left corner of
//! its rectangle (the origin of its points, for a polygon or polyline) in the
//! layer's pixels, and `rotation` turns it about that corner. Tiled stores an
//! isometric or oblique map's objects in a space of its own, which the
//! grid projects on the way in. Tiled rotates about the
//! object's own position, which for a tile object is the alignment point
//! (bottom-left unless the tileset says otherwise), so a rotated tile object's
//! position moves to where its top-left corner ends up: the same picture,
//! and one rule for every object.

use std::collections::HashMap;
use std::sync::Arc;

use glam::{DVec2, Vec2};

use super::build::{MAX_TILES, join};
use super::data::decode_gid;
use super::project::Props;
use super::raw::{RawObject, RawPoint, RawProperty, RawTemplate, RawTileset};
use super::xml::{parse_tsx, parse_tx};
use crate::core::asset::{AssetError, AssetPath, LoadContext};
use crate::core::tilemap::{GridShape, MapGrid, MapObjectData, ObjectShape, Tile};

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
  /// The alignment Tiled's `objectalignment` names on a map of `shape`; an
  /// unspecified one is the bottom centre on an isometric map and the bottom
  /// left on every other (measured)
  pub(super) fn parse(text: Option<&str>, shape: GridShape) -> Option<Alignment> {
    Some(match text {
      None | Some("" | "unspecified") => Alignment::unspecified(shape),
      Some("bottomleft") => Alignment::BottomLeft,
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

  /// What an unspecified `objectalignment` means on a map of `shape`
  pub(super) fn unspecified(shape: GridShape) -> Alignment {
    match shape {
      GridShape::Isometric => Alignment::Bottom,
      _ => Alignment::BottomLeft,
    }
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
  /// The map's grid, whose object space positions are read through
  grid: MapGrid,
  templates: HashMap<String, Arc<RawTemplate>>,
}

impl ObjectState {
  pub(super) fn new(tilesets: Vec<TilesetEntry>, grid: MapGrid) -> ObjectState {
    ObjectState {
      tilesets,
      grid,
      templates: HashMap::new(),
    }
  }
}

/// The alignment `objectalignment` of the tileset file at `path`, which the
/// map holds as a dependency and so cannot look inside while it loads
fn read_alignment(
  cx: &LoadContext<'_>,
  shape: GridShape,
  from: &AssetPath,
  place: &str,
  path: &str,
) -> Result<Alignment, AssetError> {
  let Ok(asset) = AssetPath::parse(path) else {
    return Ok(Alignment::unspecified(shape));
  };
  let name = asset.as_str().to_ascii_lowercase();
  let xml = name.ends_with(".tsx");
  if !xml && !name.ends_with(".tsj") {
    return Ok(Alignment::unspecified(shape));
  }
  let bytes = cx
    .read(&asset)
    .map_err(|e| AssetError::decode(from, format!("{place}: {}", e.report())))?;
  let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(&bytes);
  let raw: RawTileset = if xml {
    std::str::from_utf8(bytes)
      .map_err(|e| e.to_string())
      .and_then(parse_tsx)
  } else {
    serde_json::from_slice(bytes).map_err(|e| e.to_string())
  }
  .map_err(|e| AssetError::decode(from, format!("{place}: {path:?}: {e}")))?;
  Alignment::parse(raw.objectalignment.as_deref(), shape).ok_or_else(|| {
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
  let lower = key.to_ascii_lowercase();
  let xml = lower.ends_with(".tx");
  if !xml && !lower.ends_with(".tj") && !lower.ends_with(".json") {
    return Err(px.fail(
      place,
      format!("the template {written:?} is not a .tj or .tx file"),
    ));
  }
  let bytes = cx
    .read(&asset)
    .map_err(|e| px.fail(place, format!("the template {written:?}: {}", e.report())))?;
  let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(&bytes);
  let raw: RawTemplate = if xml {
    std::str::from_utf8(bytes)
      .map_err(|e| e.to_string())
      .and_then(parse_tx)
      .map_err(|e| px.fail(place, format!("the template {written:?}: {e}")))?
  } else {
    serde_json::from_slice(bytes).map_err(|e| {
      px.fail(
        place,
        format!(
          "the template {written:?}: line {}, column {}: {e}",
          e.line(),
          e.column()
        ),
      )
    })?
  };
  let raw = Arc::new(raw);
  state.templates.insert(key.clone(), raw.clone());
  Ok((raw, key))
}

/// Points round an ellipse of an isometric or oblique map, which Tiled
/// draws as the outline of its projection
const ELLIPSE_POINTS: usize = 32;

/// Points of each of a capsule's two caps
const CAP_POINTS: usize = 9;

/// The outline of the rectangle `size` as its corners, in the object space
fn rect_outline(size: Vec2) -> Vec<DVec2> {
  let (w, h) = (f64::from(size.x), f64::from(size.y));
  vec![
    DVec2::ZERO,
    DVec2::new(w, 0.0),
    DVec2::new(w, h),
    DVec2::new(0.0, h),
  ]
}

/// The ellipse that fills the rectangle `size`, as `ELLIPSE_POINTS` points
fn ellipse_outline(size: Vec2) -> Vec<DVec2> {
  let (rx, ry) = (f64::from(size.x) / 2.0, f64::from(size.y) / 2.0);
  (0..ELLIPSE_POINTS)
    .map(|k| {
      let t = std::f64::consts::TAU * k as f64 / ELLIPSE_POINTS as f64;
      DVec2::new(rx + rx * t.cos(), ry + ry * t.sin())
    })
    .collect()
}

/// The capsule that fills the rectangle `size`: its caps are half circles
/// at the ends of the longer side
fn capsule_outline(size: Vec2) -> Vec<DVec2> {
  let (w, h) = (f64::from(size.x), f64::from(size.y));
  let radius = w.min(h) / 2.0;
  let mut out = Vec::with_capacity(CAP_POINTS * 2);
  // a cap's points from `from` degrees turning half a circle clockwise
  let mut cap = |centre: DVec2, from: f64| {
    for k in 0..CAP_POINTS {
      let t = (from + 180.0 * k as f64 / (CAP_POINTS - 1) as f64).to_radians();
      out.push(centre + DVec2::new(t.cos(), t.sin()) * radius);
    }
  };
  if w >= h {
    cap(DVec2::new(w - radius, radius), -90.0);
    cap(DVec2::new(radius, radius), 90.0);
  } else {
    cap(DVec2::new(radius, h - radius), 0.0);
    cap(DVec2::new(radius, radius), 180.0);
  }
  out
}

/// An object's shape on an isometric or oblique map, where Tiled stores
/// it in the grid's object space: rectangles, ellipses and capsules become
/// polygons of their projected outline, and a polygon's or polyline's points
/// are projected. Its points are steps from the object's projected position,
/// and the rotation (Tiled measured: in screen space, about that position)
/// turns them as it does any polygon. Points and text keep their box, drawn
/// unprojected at the projected position (measured). The size of a shape
/// that became points is zero
fn project_shape(grid: &MapGrid, shape: ObjectShape, size: Vec2) -> (ObjectShape, Vec2) {
  let step = |p: DVec2| grid.object_step(p).as_vec2();
  let steps = |outline: Vec<DVec2>| outline.into_iter().map(step).collect();
  match shape {
    ObjectShape::Rect => (ObjectShape::Polygon(steps(rect_outline(size))), Vec2::ZERO),
    ObjectShape::Ellipse => (
      ObjectShape::Polygon(steps(ellipse_outline(size))),
      Vec2::ZERO,
    ),
    ObjectShape::Capsule => (
      ObjectShape::Polygon(steps(capsule_outline(size))),
      Vec2::ZERO,
    ),
    ObjectShape::Polygon(points) => (
      ObjectShape::Polygon(points.iter().map(|p| step(p.as_dvec2())).collect()),
      Vec2::ZERO,
    ),
    ObjectShape::Polyline(points) => (
      ObjectShape::Polyline(points.iter().map(|p| step(p.as_dvec2())).collect()),
      Vec2::ZERO,
    ),
    other => (other, size),
  }
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

  let grid = state.grid;
  let stored = DVec2::new(o.x.unwrap_or(0.0), o.y.unwrap_or(0.0));
  let mut position = if grid.projects_objects() {
    grid.object_point(stored)
  } else {
    stored.as_vec2()
  };
  let mut size = Vec2::new(
    o.width.unwrap_or(0.0) as f32,
    o.height.unwrap_or(0.0) as f32,
  );
  // a tile object's picture is sized in pixels; every other shape of an
  // isometric or oblique map is drawn through the grid
  let mut shape = shape;
  if grid.projects_objects() && tile.is_none() {
    (shape, size) = project_shape(&grid, shape, size);
  }
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
        let a = read_alignment(cx, state.grid.shape, px.path(), &place, &path)?;
        if let Some(entry) = state.tilesets.get_mut(tileset_index) {
          entry.alignment = Some(a);
        }
        a
      }
      _ => Alignment::unspecified(state.grid.shape),
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

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn an_unspecified_alignment_is_bottom_centre_only_on_isometric_maps() {
    assert_eq!(
      Alignment::parse(None, GridShape::Isometric),
      Some(Alignment::Bottom)
    );
    assert_eq!(
      Alignment::parse(Some("unspecified"), GridShape::Isometric),
      Some(Alignment::Bottom)
    );
    assert_eq!(
      Alignment::parse(Some("bottomleft"), GridShape::Isometric),
      Some(Alignment::BottomLeft)
    );
    for shape in [
      GridShape::Orthogonal,
      GridShape::Oblique {
        skew: (8, 12).into(),
      },
    ] {
      assert_eq!(Alignment::parse(None, shape), Some(Alignment::BottomLeft));
    }
  }

  #[test]
  fn a_capsule_outline_fills_its_box_with_round_ends() {
    for size in [Vec2::new(64.0, 32.0), Vec2::new(20.0, 50.0)] {
      let outline = capsule_outline(size);
      assert_eq!(outline.len(), CAP_POINTS * 2);
      let (mut lo, mut hi) = (DVec2::MAX, DVec2::MIN);
      for p in &outline {
        lo = lo.min(*p);
        hi = hi.max(*p);
      }
      assert!(lo.abs_diff_eq(DVec2::ZERO, 1e-9), "{lo}");
      assert!(hi.abs_diff_eq(size.as_dvec2(), 1e-9), "{hi}");
    }
  }
}
