use std::collections::HashMap;
use std::fmt;
use std::path::Path;

use serde::Deserialize;

use super::layer::TilemapLayer;
use super::map::{Tilemap, TilemapOrientation};
use super::tileset::{TileAnimation, TileId, TileProperties, Tileset};

// ---------------------------------------------------------------------------
// Public error type
// ---------------------------------------------------------------------------

/// Errors that can occur when loading a Tiled map.
#[derive(Debug)]
pub enum TilemapError {
  /// An I/O error occurred reading a file.
  Io(std::io::Error),
  /// The JSON could not be parsed or contained unexpected structure.
  Parse(String),
  /// A required tileset was missing or could not be resolved.
  MissingTileset(String),
  /// The map uses a feature that is not supported by this loader.
  UnsupportedFeature(String),
}

impl fmt::Display for TilemapError {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    match self {
      TilemapError::Io(err) => write!(f, "I/O error: {}", err),
      TilemapError::Parse(msg) => write!(f, "parse error: {}", msg),
      TilemapError::MissingTileset(name) => write!(f, "missing tileset: {}", name),
      TilemapError::UnsupportedFeature(feat) => {
        write!(f, "unsupported feature: {}", feat)
      }
    }
  }
}

impl std::error::Error for TilemapError {
  fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
    match self {
      TilemapError::Io(err) => Some(err),
      _ => None,
    }
  }
}

impl From<std::io::Error> for TilemapError {
  fn from(err: std::io::Error) -> Self {
    TilemapError::Io(err)
  }
}

impl From<serde_json::Error> for TilemapError {
  fn from(err: serde_json::Error) -> Self {
    TilemapError::Parse(err.to_string())
  }
}

// ---------------------------------------------------------------------------
// Public object types
// ---------------------------------------------------------------------------

/// An object parsed from a Tiled object layer.
#[derive(Debug, Clone)]
pub struct TiledObject {
  /// Name of the object (may be empty).
  pub name: String,
  /// Type/class string of the object.
  pub obj_type: String,
  /// X position in pixels.
  pub x: f32,
  /// Y position in pixels.
  pub y: f32,
  /// Width in pixels (0 for points).
  pub width: f32,
  /// Height in pixels (0 for points).
  pub height: f32,
  /// Rotation in degrees (clockwise).
  pub rotation: f32,
  /// The geometric shape of this object.
  pub shape: TiledShape,
  /// Custom properties defined in the Tiled editor.
  pub properties: HashMap<String, TiledValue>,
}

/// The geometric shape of a [`TiledObject`].
#[derive(Debug, Clone)]
pub enum TiledShape {
  /// Axis-aligned rectangle (default shape).
  Rect,
  /// Ellipse inscribed in the object's bounding box.
  Ellipse,
  /// A single point (width and height are 0).
  Point,
  /// A closed polygon defined by vertex offsets from the object's position.
  Polygon(Vec<(f32, f32)>),
  /// An open polyline defined by vertex offsets from the object's position.
  Polyline(Vec<(f32, f32)>),
}

/// A typed property value from the Tiled editor.
#[derive(Debug, Clone)]
pub enum TiledValue {
  /// Boolean value.
  Bool(bool),
  /// Integer value.
  Int(i64),
  /// Floating-point value.
  Float(f64),
  /// String value.
  String(String),
}

// ---------------------------------------------------------------------------
// Internal serde types for the raw Tiled JSON format
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct RawMap {
  #[serde(default)]
  orientation: String,
  #[serde(default)]
  width: u32,
  #[serde(default)]
  height: u32,
  #[serde(default)]
  tilewidth: u32,
  #[serde(default)]
  tileheight: u32,
  #[serde(default)]
  infinite: bool,
  #[serde(default)]
  tilesets: Vec<RawTilesetRef>,
  #[serde(default)]
  layers: Vec<RawLayer>,
}

#[derive(Deserialize)]
struct RawTilesetRef {
  firstgid: u32,
  #[serde(default)]
  source: Option<String>,
  // Inline tileset data (when no external source).
  #[serde(flatten)]
  inline: RawTileset,
}

#[derive(Deserialize, Default)]
struct RawTileset {
  #[serde(default)]
  name: String,
  #[serde(default)]
  image: Option<String>,
  #[serde(default)]
  tilewidth: u32,
  #[serde(default)]
  tileheight: u32,
  #[serde(default)]
  columns: u32,
  #[serde(default)]
  tilecount: u32,
  #[serde(default)]
  spacing: u32,
  #[serde(default)]
  margin: u32,
  #[serde(default)]
  imagewidth: u32,
  #[serde(default)]
  imageheight: u32,
  #[serde(default)]
  tiles: Vec<RawTile>,
}

#[derive(Deserialize)]
struct RawTile {
  id: u32,
  #[serde(default)]
  properties: Vec<RawProperty>,
  #[serde(default)]
  animation: Vec<RawAnimationFrame>,
}

#[derive(Deserialize)]
struct RawProperty {
  #[serde(default)]
  name: String,
  #[serde(rename = "type", default)]
  prop_type: String,
  #[serde(default)]
  value: serde_json::Value,
}

#[derive(Deserialize)]
struct RawAnimationFrame {
  tileid: u32,
  duration: u32,
}

#[derive(Deserialize)]
struct RawLayer {
  #[serde(rename = "type", default)]
  layer_type: String,
  #[serde(default)]
  name: String,
  #[serde(default)]
  visible: bool,
  #[serde(default = "default_opacity")]
  opacity: f32,
  #[serde(default)]
  data: Vec<u32>,
  #[serde(default)]
  width: u32,
  #[serde(default)]
  height: u32,
  #[serde(default)]
  objects: Vec<RawObject>,
  #[serde(default = "default_parallax")]
  parallaxx: f32,
  #[serde(default = "default_parallax")]
  parallaxy: f32,
}

fn default_opacity() -> f32 {
  1.0
}

fn default_parallax() -> f32 {
  1.0
}

#[derive(Deserialize)]
struct RawObject {
  #[serde(default)]
  name: String,
  #[serde(rename = "type", default)]
  obj_type: String,
  #[serde(default)]
  x: f32,
  #[serde(default)]
  y: f32,
  #[serde(default)]
  width: f32,
  #[serde(default)]
  height: f32,
  #[serde(default)]
  rotation: f32,
  #[serde(default)]
  point: bool,
  #[serde(default)]
  ellipse: bool,
  #[serde(default)]
  polygon: Option<Vec<RawPoint>>,
  #[serde(default)]
  polyline: Option<Vec<RawPoint>>,
  #[serde(default)]
  properties: Vec<RawProperty>,
}

#[derive(Deserialize)]
struct RawPoint {
  x: f32,
  y: f32,
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Given a Tiled GID, find which tileset it belongs to and return
/// `(tileset_index, local_tile_id)`.
///
/// Tilesets are sorted by `firstgid` ascending. A GID belongs to the tileset
/// with the largest `firstgid` that is <= the GID.
fn resolve_tileset_index(firstgids: &[(u32, usize)], gid: u32) -> Option<(usize, TileId)> {
  if gid == 0 {
    return None;
  }
  // firstgids is sorted ascending by firstgid.
  // Walk backwards to find the last entry where firstgid <= gid.
  let mut result: Option<(usize, TileId)> = None;
  for &(firstgid, tileset_idx) in firstgids {
    if firstgid <= gid {
      result = Some((tileset_idx, gid - firstgid));
    } else {
      break;
    }
  }
  result
}

fn parse_orientation(s: &str) -> Result<TilemapOrientation, TilemapError> {
  match s {
    "orthogonal" | "" => Ok(TilemapOrientation::Orthogonal),
    "isometric" => Ok(TilemapOrientation::Isometric),
    "hexagonal" => Ok(TilemapOrientation::HexagonalFlat),
    other => Err(TilemapError::UnsupportedFeature(format!(
      "orientation '{}'",
      other
    ))),
  }
}

fn parse_object(raw: &RawObject) -> TiledObject {
  let shape = if raw.point {
    TiledShape::Point
  } else if raw.ellipse {
    TiledShape::Ellipse
  } else if let Some(ref pts) = raw.polygon {
    TiledShape::Polygon(pts.iter().map(|p| (p.x, p.y)).collect())
  } else if let Some(ref pts) = raw.polyline {
    TiledShape::Polyline(pts.iter().map(|p| (p.x, p.y)).collect())
  } else {
    TiledShape::Rect
  };

  let properties = parse_properties(&raw.properties);

  TiledObject {
    name: raw.name.clone(),
    obj_type: raw.obj_type.clone(),
    x: raw.x,
    y: raw.y,
    width: raw.width,
    height: raw.height,
    rotation: raw.rotation,
    shape,
    properties,
  }
}

fn parse_properties(raw_props: &[RawProperty]) -> HashMap<String, TiledValue> {
  let mut props = HashMap::new();
  for rp in raw_props {
    let value = match rp.prop_type.as_str() {
      "bool" => match rp.value.as_bool() {
        Some(b) => TiledValue::Bool(b),
        None => continue,
      },
      "int" => match rp.value.as_i64() {
        Some(i) => TiledValue::Int(i),
        None => continue,
      },
      "float" => match rp.value.as_f64() {
        Some(f) => TiledValue::Float(f),
        None => continue,
      },
      "string" | "" => match rp.value.as_str() {
        Some(s) => TiledValue::String(s.to_string()),
        None => {
          // Fallback: store as string representation.
          TiledValue::String(rp.value.to_string())
        }
      },
      _ => {
        // Unknown property type, store as string.
        TiledValue::String(rp.value.to_string())
      }
    };
    props.insert(rp.name.clone(), value);
  }
  props
}

fn parse_tile_properties(raw_tileset: &RawTileset) -> HashMap<TileId, TileProperties> {
  let mut props_map = HashMap::new();

  for raw_tile in &raw_tileset.tiles {
    let mut tile_props = TileProperties::default();

    // Parse animation frames.
    if !raw_tile.animation.is_empty() {
      tile_props.animation = Some(TileAnimation {
        frames: raw_tile
          .animation
          .iter()
          .map(|f| (f.tileid, f.duration))
          .collect(),
      });
    }

    // Parse per-tile properties.
    for rp in &raw_tile.properties {
      match rp.name.as_str() {
        "solid" => {
          if let Some(b) = rp.value.as_bool() {
            tile_props.solid = b;
          }
        }
        "platform" => {
          if let Some(b) = rp.value.as_bool() {
            tile_props.platform = b;
          }
        }
        _ => {
          // Store as custom property.
          let val = match rp.value.as_str() {
            Some(s) => s.to_string(),
            None => rp.value.to_string(),
          };
          tile_props.custom.insert(rp.name.clone(), val);
        }
      }
    }

    props_map.insert(raw_tile.id, tile_props);
  }

  props_map
}

fn build_tileset(raw: &RawTileset) -> Tileset {
  let properties = parse_tile_properties(raw);
  Tileset {
    name: raw.name.clone(),
    image_path: raw.image.clone().unwrap_or_default(),
    tile_width: raw.tilewidth,
    tile_height: raw.tileheight,
    columns: raw.columns,
    tile_count: raw.tilecount,
    spacing: raw.spacing,
    margin: raw.margin,
    image_width: raw.imagewidth,
    image_height: raw.imageheight,
    properties,
  }
}

// ---------------------------------------------------------------------------
// Public loader
// ---------------------------------------------------------------------------

/// Load a Tiled JSON map file (.tmj) and return a [`Tilemap`] plus any
/// objects found in object layers.
///
/// External tilesets (.tsj) are resolved relative to the map file's directory.
///
/// # Errors
///
/// Returns [`TilemapError`] if:
/// - The file cannot be read ([`TilemapError::Io`]).
/// - The JSON is malformed ([`TilemapError::Parse`]).
/// - The map is infinite ([`TilemapError::UnsupportedFeature`]).
/// - A tileset cannot be loaded ([`TilemapError::MissingTileset`]).
pub fn load_tiled_map(path: &str) -> Result<(Tilemap, Vec<TiledObject>), TilemapError> {
  let map_path = Path::new(path);
  let map_dir = map_path.parent().unwrap_or_else(|| Path::new("."));

  let contents = std::fs::read_to_string(map_path)?;
  load_tiled_map_str(&contents, map_dir)
}

/// Parse a Tiled JSON map from a string, resolving external tilesets relative
/// to `base_dir`.
fn load_tiled_map_str(
  json: &str,
  base_dir: &Path,
) -> Result<(Tilemap, Vec<TiledObject>), TilemapError> {
  let raw_map: RawMap = serde_json::from_str(json)?;

  // Reject infinite maps.
  if raw_map.infinite {
    return Err(TilemapError::UnsupportedFeature(
      "infinite maps are not supported".to_string(),
    ));
  }

  let orientation = parse_orientation(&raw_map.orientation)?;

  // Load tilesets (external or inline).
  let mut tilesets: Vec<Tileset> = Vec::new();
  // Sorted (firstgid, tileset_index) pairs for GID resolution.
  let mut firstgids: Vec<(u32, usize)> = Vec::new();

  for ts_ref in &raw_map.tilesets {
    let tileset = if let Some(ref source) = ts_ref.source {
      let ts_path = base_dir.join(source);
      let ts_contents = std::fs::read_to_string(&ts_path).map_err(|e| {
        TilemapError::MissingTileset(format!(
          "failed to read tileset '{}': {}",
          ts_path.display(),
          e
        ))
      })?;
      let raw_ts: RawTileset = serde_json::from_str(&ts_contents)?;
      build_tileset(&raw_ts)
    } else {
      build_tileset(&ts_ref.inline)
    };
    let idx = tilesets.len();
    tilesets.push(tileset);
    firstgids.push((ts_ref.firstgid, idx));
  }

  // Ensure firstgids are sorted by firstgid (they usually already are).
  firstgids.sort_by_key(|&(fgid, _)| fgid);

  // Build the tilemap.
  let mut tilemap = Tilemap::new(
    raw_map.tilewidth,
    raw_map.tileheight,
    raw_map.width,
    raw_map.height,
  );
  tilemap.orientation = orientation;
  for ts in tilesets {
    tilemap.add_tileset(ts);
  }

  let mut objects: Vec<TiledObject> = Vec::new();
  let mut z_order: i32 = 0;

  for raw_layer in &raw_map.layers {
    match raw_layer.layer_type.as_str() {
      "tilelayer" => {
        let mut layer = TilemapLayer::new(&raw_layer.name, raw_layer.width, raw_layer.height);
        layer.visible = raw_layer.visible;
        layer.opacity = raw_layer.opacity;
        layer.z_order = z_order;
        layer.parallax_x = raw_layer.parallaxx;
        layer.parallax_y = raw_layer.parallaxy;

        // Parse tile data: GID 0 = empty, otherwise resolve to
        // (tileset_index, local_tile_id).
        for (i, &gid) in raw_layer.data.iter().enumerate() {
          if gid == 0 {
            continue;
          }
          if let Some((ts_idx, local_id)) = resolve_tileset_index(&firstgids, gid) {
            let x = (i as u32) % raw_layer.width;
            let y = (i as u32) / raw_layer.width;
            layer.tileset_index = ts_idx;
            layer.tiles[i] = Some(local_id);
            let _ = (x, y); // used for bounds only
          }
        }

        tilemap.add_layer(layer);
        z_order += 1;
      }
      "objectgroup" => {
        for raw_obj in &raw_layer.objects {
          objects.push(parse_object(raw_obj));
        }
        z_order += 1;
      }
      _ => {
        // Ignore unknown layer types (image layers, group layers, etc.).
        z_order += 1;
      }
    }
  }

  Ok((tilemap, objects))
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
  use super::*;

  // -- resolve_tileset_index tests --

  #[test]
  fn resolve_single_tileset() {
    // One tileset starting at GID 1.
    let firstgids = vec![(1, 0)];

    // GID 0 is always empty.
    assert!(resolve_tileset_index(&firstgids, 0).is_none());

    // GID 1 -> tileset 0, local 0.
    assert_eq!(resolve_tileset_index(&firstgids, 1), Some((0, 0)));

    // GID 5 -> tileset 0, local 4.
    assert_eq!(resolve_tileset_index(&firstgids, 5), Some((0, 4)));
  }

  #[test]
  fn resolve_multiple_tilesets() {
    // Two tilesets: first at GID 1 (16 tiles), second at GID 17.
    let firstgids = vec![(1, 0), (17, 1)];

    // GID 1 -> tileset 0, local 0.
    assert_eq!(resolve_tileset_index(&firstgids, 1), Some((0, 0)));

    // GID 16 -> tileset 0, local 15.
    assert_eq!(resolve_tileset_index(&firstgids, 16), Some((0, 15)));

    // GID 17 -> tileset 1, local 0.
    assert_eq!(resolve_tileset_index(&firstgids, 17), Some((1, 0)));

    // GID 20 -> tileset 1, local 3.
    assert_eq!(resolve_tileset_index(&firstgids, 20), Some((1, 3)));
  }

  #[test]
  fn resolve_three_tilesets() {
    let firstgids = vec![(1, 0), (33, 1), (65, 2)];

    assert_eq!(resolve_tileset_index(&firstgids, 1), Some((0, 0)));
    assert_eq!(resolve_tileset_index(&firstgids, 32), Some((0, 31)));
    assert_eq!(resolve_tileset_index(&firstgids, 33), Some((1, 0)));
    assert_eq!(resolve_tileset_index(&firstgids, 64), Some((1, 31)));
    assert_eq!(resolve_tileset_index(&firstgids, 65), Some((2, 0)));
    assert_eq!(resolve_tileset_index(&firstgids, 100), Some((2, 35)));
  }

  // -- RawMap parsing tests --

  #[test]
  fn parse_minimal_map_json() {
    let json = r#"{
            "orientation": "orthogonal",
            "width": 10,
            "height": 8,
            "tilewidth": 32,
            "tileheight": 32,
            "infinite": false,
            "tilesets": [],
            "layers": []
        }"#;
    let raw: RawMap = serde_json::from_str(json).unwrap();
    assert_eq!(raw.orientation, "orthogonal");
    assert_eq!(raw.width, 10);
    assert_eq!(raw.height, 8);
    assert_eq!(raw.tilewidth, 32);
    assert_eq!(raw.tileheight, 32);
    assert!(!raw.infinite);
    assert!(raw.tilesets.is_empty());
    assert!(raw.layers.is_empty());
  }

  #[test]
  fn parse_map_with_inline_tileset() {
    let json = r#"{
            "orientation": "orthogonal",
            "width": 4,
            "height": 4,
            "tilewidth": 16,
            "tileheight": 16,
            "infinite": false,
            "tilesets": [
                {
                    "firstgid": 1,
                    "name": "terrain",
                    "image": "terrain.png",
                    "tilewidth": 16,
                    "tileheight": 16,
                    "columns": 8,
                    "tilecount": 64,
                    "spacing": 0,
                    "margin": 0,
                    "imagewidth": 128,
                    "imageheight": 128,
                    "tiles": []
                }
            ],
            "layers": []
        }"#;
    let raw: RawMap = serde_json::from_str(json).unwrap();
    assert_eq!(raw.tilesets.len(), 1);
    assert_eq!(raw.tilesets[0].firstgid, 1);
    assert!(raw.tilesets[0].source.is_none());
    assert_eq!(raw.tilesets[0].inline.name, "terrain");
    assert_eq!(raw.tilesets[0].inline.columns, 8);
    assert_eq!(raw.tilesets[0].inline.tilecount, 64);
  }

  #[test]
  fn parse_map_with_tile_layer() {
    let json = r#"{
            "orientation": "orthogonal",
            "width": 3,
            "height": 2,
            "tilewidth": 16,
            "tileheight": 16,
            "infinite": false,
            "tilesets": [
                {
                    "firstgid": 1,
                    "name": "tiles",
                    "tilewidth": 16,
                    "tileheight": 16,
                    "columns": 4,
                    "tilecount": 16,
                    "imagewidth": 64,
                    "imageheight": 64,
                    "tiles": []
                }
            ],
            "layers": [
                {
                    "type": "tilelayer",
                    "name": "ground",
                    "visible": true,
                    "opacity": 0.8,
                    "width": 3,
                    "height": 2,
                    "data": [1, 0, 3, 0, 2, 0]
                }
            ]
        }"#;
    let raw: RawMap = serde_json::from_str(json).unwrap();
    assert_eq!(raw.layers.len(), 1);
    assert_eq!(raw.layers[0].layer_type, "tilelayer");
    assert_eq!(raw.layers[0].name, "ground");
    assert!(raw.layers[0].visible);
    assert_eq!(raw.layers[0].opacity, 0.8);
    assert_eq!(raw.layers[0].data, vec![1, 0, 3, 0, 2, 0]);
  }

  // -- Object shape parsing tests --

  #[test]
  fn parse_rect_object() {
    let json = r#"{
            "name": "spawn",
            "type": "player_spawn",
            "x": 100.0,
            "y": 200.0,
            "width": 32.0,
            "height": 32.0,
            "rotation": 0.0,
            "properties": []
        }"#;
    let raw: RawObject = serde_json::from_str(json).unwrap();
    let obj = parse_object(&raw);
    assert_eq!(obj.name, "spawn");
    assert_eq!(obj.obj_type, "player_spawn");
    assert_eq!(obj.x, 100.0);
    assert_eq!(obj.y, 200.0);
    assert!(matches!(obj.shape, TiledShape::Rect));
  }

  #[test]
  fn parse_point_object() {
    let json = r#"{
            "name": "waypoint",
            "type": "path",
            "x": 50.0,
            "y": 75.0,
            "width": 0.0,
            "height": 0.0,
            "rotation": 0.0,
            "point": true,
            "properties": []
        }"#;
    let raw: RawObject = serde_json::from_str(json).unwrap();
    let obj = parse_object(&raw);
    assert!(matches!(obj.shape, TiledShape::Point));
  }

  #[test]
  fn parse_ellipse_object() {
    let json = r#"{
            "name": "zone",
            "type": "trigger",
            "x": 10.0,
            "y": 20.0,
            "width": 64.0,
            "height": 48.0,
            "rotation": 0.0,
            "ellipse": true,
            "properties": []
        }"#;
    let raw: RawObject = serde_json::from_str(json).unwrap();
    let obj = parse_object(&raw);
    assert!(matches!(obj.shape, TiledShape::Ellipse));
  }

  #[test]
  fn parse_polygon_object() {
    let json = r#"{
            "name": "collider",
            "type": "collision",
            "x": 0.0,
            "y": 0.0,
            "width": 0.0,
            "height": 0.0,
            "rotation": 0.0,
            "polygon": [
                {"x": 0.0, "y": 0.0},
                {"x": 32.0, "y": 0.0},
                {"x": 16.0, "y": 32.0}
            ],
            "properties": []
        }"#;
    let raw: RawObject = serde_json::from_str(json).unwrap();
    let obj = parse_object(&raw);
    match &obj.shape {
      TiledShape::Polygon(pts) => {
        assert_eq!(pts.len(), 3);
        assert_eq!(pts[0], (0.0, 0.0));
        assert_eq!(pts[1], (32.0, 0.0));
        assert_eq!(pts[2], (16.0, 32.0));
      }
      _ => panic!("expected Polygon shape"),
    }
  }

  #[test]
  fn parse_polyline_object() {
    let json = r#"{
            "name": "path",
            "type": "route",
            "x": 5.0,
            "y": 5.0,
            "width": 0.0,
            "height": 0.0,
            "rotation": 45.0,
            "polyline": [
                {"x": 0.0, "y": 0.0},
                {"x": 10.0, "y": 20.0}
            ],
            "properties": []
        }"#;
    let raw: RawObject = serde_json::from_str(json).unwrap();
    let obj = parse_object(&raw);
    assert_eq!(obj.rotation, 45.0);
    match &obj.shape {
      TiledShape::Polyline(pts) => {
        assert_eq!(pts.len(), 2);
        assert_eq!(pts[0], (0.0, 0.0));
        assert_eq!(pts[1], (10.0, 20.0));
      }
      _ => panic!("expected Polyline shape"),
    }
  }

  // -- Object property parsing tests --

  #[test]
  fn parse_object_properties() {
    let json = r#"{
            "name": "obj",
            "type": "",
            "x": 0.0,
            "y": 0.0,
            "width": 0.0,
            "height": 0.0,
            "rotation": 0.0,
            "properties": [
                {"name": "active", "type": "bool", "value": true},
                {"name": "count", "type": "int", "value": 42},
                {"name": "speed", "type": "float", "value": 3.14},
                {"name": "label", "type": "string", "value": "hello"}
            ]
        }"#;
    let raw: RawObject = serde_json::from_str(json).unwrap();
    let obj = parse_object(&raw);
    assert_eq!(obj.properties.len(), 4);

    match &obj.properties["active"] {
      TiledValue::Bool(b) => assert!(*b),
      _ => panic!("expected Bool"),
    }
    match &obj.properties["count"] {
      TiledValue::Int(i) => assert_eq!(*i, 42),
      _ => panic!("expected Int"),
    }
    match &obj.properties["speed"] {
      TiledValue::Float(f) => assert!((f - 3.14).abs() < 1e-6),
      _ => panic!("expected Float"),
    }
    match &obj.properties["label"] {
      TiledValue::String(s) => assert_eq!(s, "hello"),
      _ => panic!("expected String"),
    }
  }

  // -- Error display tests --

  #[test]
  fn error_display_io() {
    let err = TilemapError::Io(std::io::Error::new(
      std::io::ErrorKind::NotFound,
      "file not found",
    ));
    let msg = format!("{}", err);
    assert!(msg.contains("I/O error"));
    assert!(msg.contains("file not found"));
  }

  #[test]
  fn error_display_parse() {
    let err = TilemapError::Parse("unexpected token".to_string());
    let msg = format!("{}", err);
    assert!(msg.contains("parse error"));
    assert!(msg.contains("unexpected token"));
  }

  #[test]
  fn error_display_missing_tileset() {
    let err = TilemapError::MissingTileset("terrain.tsj".to_string());
    let msg = format!("{}", err);
    assert!(msg.contains("missing tileset"));
    assert!(msg.contains("terrain.tsj"));
  }

  #[test]
  fn error_display_unsupported() {
    let err = TilemapError::UnsupportedFeature("infinite maps".to_string());
    let msg = format!("{}", err);
    assert!(msg.contains("unsupported feature"));
    assert!(msg.contains("infinite maps"));
  }

  #[test]
  fn error_from_io() {
    let io_err = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "denied");
    let err: TilemapError = io_err.into();
    assert!(matches!(err, TilemapError::Io(_)));
  }

  #[test]
  fn error_from_serde_json() {
    let bad_json = "{not valid json}}}";
    let serde_err = serde_json::from_str::<serde_json::Value>(bad_json).unwrap_err();
    let err: TilemapError = serde_err.into();
    assert!(matches!(err, TilemapError::Parse(_)));
  }

  // -- load_tiled_map_str integration tests --

  #[test]
  fn load_rejects_infinite_map() {
    let json = r#"{
            "orientation": "orthogonal",
            "width": 10,
            "height": 10,
            "tilewidth": 16,
            "tileheight": 16,
            "infinite": true,
            "tilesets": [],
            "layers": []
        }"#;
    let result = load_tiled_map_str(json, Path::new("."));
    assert!(result.is_err());
    match result.unwrap_err() {
      TilemapError::UnsupportedFeature(msg) => {
        assert!(msg.contains("infinite"));
      }
      other => panic!("expected UnsupportedFeature, got {:?}", other),
    }
  }

  #[test]
  fn load_empty_map() {
    let json = r#"{
            "orientation": "orthogonal",
            "width": 5,
            "height": 3,
            "tilewidth": 16,
            "tileheight": 16,
            "infinite": false,
            "tilesets": [],
            "layers": []
        }"#;
    let (tilemap, objects) = load_tiled_map_str(json, Path::new(".")).unwrap();
    assert_eq!(tilemap.map_width, 5);
    assert_eq!(tilemap.map_height, 3);
    assert_eq!(tilemap.tile_width, 16);
    assert_eq!(tilemap.tile_height, 16);
    assert!(tilemap.layers.is_empty());
    assert!(tilemap.tilesets.is_empty());
    assert!(objects.is_empty());
  }

  #[test]
  fn load_map_with_tile_layer_and_objects() {
    let json = r#"{
            "orientation": "orthogonal",
            "width": 3,
            "height": 2,
            "tilewidth": 16,
            "tileheight": 16,
            "infinite": false,
            "tilesets": [
                {
                    "firstgid": 1,
                    "name": "tiles",
                    "image": "tiles.png",
                    "tilewidth": 16,
                    "tileheight": 16,
                    "columns": 4,
                    "tilecount": 16,
                    "imagewidth": 64,
                    "imageheight": 64,
                    "tiles": [
                        {
                            "id": 0,
                            "properties": [
                                {"name": "solid", "type": "bool", "value": true}
                            ]
                        }
                    ]
                }
            ],
            "layers": [
                {
                    "type": "tilelayer",
                    "name": "ground",
                    "visible": true,
                    "opacity": 1.0,
                    "width": 3,
                    "height": 2,
                    "data": [1, 2, 0, 0, 3, 1]
                },
                {
                    "type": "objectgroup",
                    "name": "objects",
                    "visible": true,
                    "opacity": 1.0,
                    "objects": [
                        {
                            "name": "spawn",
                            "type": "player",
                            "x": 16.0,
                            "y": 32.0,
                            "width": 16.0,
                            "height": 16.0,
                            "rotation": 0.0,
                            "properties": []
                        }
                    ]
                }
            ]
        }"#;
    let (tilemap, objects) = load_tiled_map_str(json, Path::new(".")).unwrap();

    // Tileset loaded.
    assert_eq!(tilemap.tilesets.len(), 1);
    assert_eq!(tilemap.tilesets[0].name, "tiles");

    // Tile properties parsed from tileset.
    let props = tilemap.tilesets[0].get_properties(0);
    assert!(props.solid);

    // Tile layer created correctly.
    assert_eq!(tilemap.layers.len(), 1);
    let layer = &tilemap.layers[0];
    assert_eq!(layer.name, "ground");
    assert_eq!(layer.width, 3);
    assert_eq!(layer.height, 2);
    assert_eq!(layer.z_order, 0);

    // GID 1 -> local 0, GID 2 -> local 1, GID 3 -> local 2.
    assert_eq!(layer.get_tile(0, 0), Some(0)); // GID 1
    assert_eq!(layer.get_tile(1, 0), Some(1)); // GID 2
    assert_eq!(layer.get_tile(2, 0), None); // GID 0
    assert_eq!(layer.get_tile(0, 1), None); // GID 0
    assert_eq!(layer.get_tile(1, 1), Some(2)); // GID 3
    assert_eq!(layer.get_tile(2, 1), Some(0)); // GID 1

    // Objects parsed.
    assert_eq!(objects.len(), 1);
    assert_eq!(objects[0].name, "spawn");
    assert_eq!(objects[0].obj_type, "player");
    assert_eq!(objects[0].x, 16.0);
    assert_eq!(objects[0].y, 32.0);
  }

  #[test]
  fn load_isometric_orientation() {
    let json = r#"{
            "orientation": "isometric",
            "width": 10,
            "height": 10,
            "tilewidth": 64,
            "tileheight": 32,
            "infinite": false,
            "tilesets": [],
            "layers": []
        }"#;
    let (tilemap, _) = load_tiled_map_str(json, Path::new(".")).unwrap();
    assert_eq!(tilemap.orientation, TilemapOrientation::Isometric);
  }

  #[test]
  fn load_hexagonal_orientation() {
    let json = r#"{
            "orientation": "hexagonal",
            "width": 10,
            "height": 10,
            "tilewidth": 32,
            "tileheight": 32,
            "infinite": false,
            "tilesets": [],
            "layers": []
        }"#;
    let (tilemap, _) = load_tiled_map_str(json, Path::new(".")).unwrap();
    assert_eq!(tilemap.orientation, TilemapOrientation::HexagonalFlat);
  }

  #[test]
  fn z_order_increments_per_layer() {
    let json = r#"{
            "orientation": "orthogonal",
            "width": 2,
            "height": 2,
            "tilewidth": 16,
            "tileheight": 16,
            "infinite": false,
            "tilesets": [
                {
                    "firstgid": 1,
                    "name": "t",
                    "tilewidth": 16,
                    "tileheight": 16,
                    "columns": 4,
                    "tilecount": 16,
                    "imagewidth": 64,
                    "imageheight": 64,
                    "tiles": []
                }
            ],
            "layers": [
                {
                    "type": "tilelayer",
                    "name": "bg",
                    "visible": true,
                    "opacity": 1.0,
                    "width": 2,
                    "height": 2,
                    "data": [1, 1, 1, 1]
                },
                {
                    "type": "objectgroup",
                    "name": "objs",
                    "visible": true,
                    "opacity": 1.0,
                    "objects": []
                },
                {
                    "type": "tilelayer",
                    "name": "fg",
                    "visible": true,
                    "opacity": 1.0,
                    "width": 2,
                    "height": 2,
                    "data": [1, 1, 1, 1]
                }
            ]
        }"#;
    let (tilemap, _) = load_tiled_map_str(json, Path::new(".")).unwrap();
    // Two tile layers (bg at z_order=0, fg at z_order=2 because object layer is in between).
    assert_eq!(tilemap.layers.len(), 2);
    assert_eq!(tilemap.layers[0].z_order, 0);
    assert_eq!(tilemap.layers[0].name, "bg");
    assert_eq!(tilemap.layers[1].z_order, 2);
    assert_eq!(tilemap.layers[1].name, "fg");
  }

  #[test]
  fn parallax_factors_parsed() {
    let json = r#"{
            "orientation": "orthogonal",
            "width": 2,
            "height": 2,
            "tilewidth": 16,
            "tileheight": 16,
            "infinite": false,
            "tilesets": [
                {
                    "firstgid": 1,
                    "name": "t",
                    "tilewidth": 16,
                    "tileheight": 16,
                    "columns": 4,
                    "tilecount": 16,
                    "imagewidth": 64,
                    "imageheight": 64,
                    "tiles": []
                }
            ],
            "layers": [
                {
                    "type": "tilelayer",
                    "name": "parallax_bg",
                    "visible": true,
                    "opacity": 1.0,
                    "width": 2,
                    "height": 2,
                    "data": [1, 1, 1, 1],
                    "parallaxx": 0.5,
                    "parallaxy": 0.75
                }
            ]
        }"#;
    let (tilemap, _) = load_tiled_map_str(json, Path::new(".")).unwrap();
    assert_eq!(tilemap.layers.len(), 1);
    assert_eq!(tilemap.layers[0].parallax_x, 0.5);
    assert_eq!(tilemap.layers[0].parallax_y, 0.75);
  }

  #[test]
  fn tile_animation_parsed() {
    let json = r#"{
            "orientation": "orthogonal",
            "width": 2,
            "height": 2,
            "tilewidth": 16,
            "tileheight": 16,
            "infinite": false,
            "tilesets": [
                {
                    "firstgid": 1,
                    "name": "animated",
                    "image": "anim.png",
                    "tilewidth": 16,
                    "tileheight": 16,
                    "columns": 4,
                    "tilecount": 16,
                    "imagewidth": 64,
                    "imageheight": 64,
                    "tiles": [
                        {
                            "id": 0,
                            "animation": [
                                {"tileid": 0, "duration": 100},
                                {"tileid": 1, "duration": 100},
                                {"tileid": 2, "duration": 200}
                            ]
                        }
                    ]
                }
            ],
            "layers": []
        }"#;
    let (tilemap, _) = load_tiled_map_str(json, Path::new(".")).unwrap();
    let props = tilemap.tilesets[0].get_properties(0);
    let anim = props.animation.unwrap();
    assert_eq!(anim.frames.len(), 3);
    assert_eq!(anim.frames[0], (0, 100));
    assert_eq!(anim.frames[1], (1, 100));
    assert_eq!(anim.frames[2], (2, 200));
  }

  #[test]
  fn parse_tile_custom_properties() {
    let json = r#"{
            "orientation": "orthogonal",
            "width": 1,
            "height": 1,
            "tilewidth": 16,
            "tileheight": 16,
            "infinite": false,
            "tilesets": [
                {
                    "firstgid": 1,
                    "name": "props",
                    "tilewidth": 16,
                    "tileheight": 16,
                    "columns": 4,
                    "tilecount": 16,
                    "imagewidth": 64,
                    "imageheight": 64,
                    "tiles": [
                        {
                            "id": 5,
                            "properties": [
                                {"name": "solid", "type": "bool", "value": true},
                                {"name": "platform", "type": "bool", "value": true},
                                {"name": "damage", "type": "string", "value": "10"}
                            ]
                        }
                    ]
                }
            ],
            "layers": []
        }"#;
    let (tilemap, _) = load_tiled_map_str(json, Path::new(".")).unwrap();
    let props = tilemap.tilesets[0].get_properties(5);
    assert!(props.solid);
    assert!(props.platform);
    assert_eq!(props.custom.get("damage").unwrap(), "10");
  }

  #[test]
  fn multiple_tilesets_gid_resolution() {
    let json = r#"{
            "orientation": "orthogonal",
            "width": 4,
            "height": 1,
            "tilewidth": 16,
            "tileheight": 16,
            "infinite": false,
            "tilesets": [
                {
                    "firstgid": 1,
                    "name": "ground",
                    "tilewidth": 16,
                    "tileheight": 16,
                    "columns": 4,
                    "tilecount": 4,
                    "imagewidth": 64,
                    "imageheight": 16,
                    "tiles": []
                },
                {
                    "firstgid": 5,
                    "name": "walls",
                    "tilewidth": 16,
                    "tileheight": 16,
                    "columns": 4,
                    "tilecount": 4,
                    "imagewidth": 64,
                    "imageheight": 16,
                    "tiles": []
                }
            ],
            "layers": [
                {
                    "type": "tilelayer",
                    "name": "mixed",
                    "visible": true,
                    "opacity": 1.0,
                    "width": 4,
                    "height": 1,
                    "data": [1, 4, 5, 8]
                }
            ]
        }"#;
    let (tilemap, _) = load_tiled_map_str(json, Path::new(".")).unwrap();
    assert_eq!(tilemap.tilesets.len(), 2);
    assert_eq!(tilemap.layers.len(), 1);
    let layer = &tilemap.layers[0];

    // GID 1 -> tileset 0, local 0.
    assert_eq!(layer.get_tile(0, 0), Some(0));
    // GID 4 -> tileset 0, local 3.
    assert_eq!(layer.get_tile(1, 0), Some(3));
    // GID 5 -> tileset 1, local 0.
    assert_eq!(layer.get_tile(2, 0), Some(0));
    // GID 8 -> tileset 1, local 3.
    assert_eq!(layer.get_tile(3, 0), Some(3));
  }

  #[test]
  fn layer_visibility_preserved() {
    let json = r#"{
            "orientation": "orthogonal",
            "width": 2,
            "height": 2,
            "tilewidth": 16,
            "tileheight": 16,
            "infinite": false,
            "tilesets": [
                {
                    "firstgid": 1,
                    "name": "t",
                    "tilewidth": 16,
                    "tileheight": 16,
                    "columns": 4,
                    "tilecount": 16,
                    "imagewidth": 64,
                    "imageheight": 64,
                    "tiles": []
                }
            ],
            "layers": [
                {
                    "type": "tilelayer",
                    "name": "hidden",
                    "visible": false,
                    "opacity": 0.5,
                    "width": 2,
                    "height": 2,
                    "data": [1, 1, 1, 1]
                }
            ]
        }"#;
    let (tilemap, _) = load_tiled_map_str(json, Path::new(".")).unwrap();
    assert!(!tilemap.layers[0].visible);
    assert_eq!(tilemap.layers[0].opacity, 0.5);
  }
}
