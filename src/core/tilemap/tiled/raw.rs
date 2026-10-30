//! What a Tiled file says, before it becomes a map or a tileset. The JSON
//! reader deserialises straight into these structs, whose field names are
//! Tiled's own; the XML reader fills the same ones by hand. Fields whose
//! default is not zero are `Option`s, so the builder owns every default.

use serde::Deserialize;
use serde_json::Value;

/// One property as written: its Tiled type, its custom type name if any,
/// and the value as JSON
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub(crate) struct RawProperty {
  pub name: String,
  #[serde(rename = "type")]
  pub ty: String,
  pub propertytype: Option<String>,
  pub value: Value,
}

/// A map file
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(crate) struct RawMap {
  pub orientation: String,
  pub infinite: bool,
  pub width: u32,
  pub height: u32,
  pub tilewidth: u32,
  pub tileheight: u32,
  pub class: Option<String>,
  pub backgroundcolor: Option<String>,
  pub properties: Vec<RawProperty>,
  pub tilesets: Vec<RawTilesetRef>,
  pub layers: Vec<RawLayer>,
}

/// A tileset a map uses: an external file by `source`, or written inline
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(crate) struct RawTilesetRef {
  pub firstgid: u32,
  pub source: Option<String>,
  #[serde(flatten)]
  pub inline: RawTileset,
}

/// A tileset file, or a tileset written inside a map
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(crate) struct RawTileset {
  pub name: String,
  pub class: Option<String>,
  pub tilewidth: u32,
  pub tileheight: u32,
  pub spacing: u32,
  pub margin: u32,
  pub tilecount: Option<u32>,
  pub columns: u32,
  pub tileoffset: RawOffset,
  /// Where tile objects sit against their position (`bottomleft`, `center`...)
  pub objectalignment: Option<String>,
  pub image: Option<String>,
  pub imagewidth: u32,
  pub imageheight: u32,
  pub tiles: Vec<RawTile>,
  pub wangsets: Vec<RawWangSet>,
  pub properties: Vec<RawProperty>,
}

/// A Wang set of a tileset
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(crate) struct RawWangSet {
  pub name: String,
  pub class: Option<String>,
  /// `corner`, `edge` or `mixed`
  #[serde(rename = "type")]
  pub ty: String,
  pub colors: Vec<RawWangColor>,
  pub wangtiles: Vec<RawWangTile>,
  pub properties: Vec<RawProperty>,
}

/// A color of a Wang set
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(crate) struct RawWangColor {
  pub name: String,
  pub class: Option<String>,
  pub color: String,
  /// The tile standing for the color; -1 or absent for none
  pub tile: Option<i64>,
  pub probability: Option<f32>,
  pub properties: Vec<RawProperty>,
}

/// A tile of a Wang set with the color index at each of its eight positions,
/// clockwise from the top edge
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(crate) struct RawWangTile {
  pub tileid: u32,
  pub wangid: Vec<u32>,
}

/// A tileset's `tileoffset`, in pixels
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(crate) struct RawOffset {
  pub x: i32,
  pub y: i32,
}

/// One tile's extra data
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(crate) struct RawTile {
  pub id: u32,
  #[serde(alias = "type")]
  pub class: Option<String>,
  pub probability: Option<f32>,
  pub image: Option<String>,
  pub imagewidth: u32,
  pub imageheight: u32,
  pub properties: Vec<RawProperty>,
  /// The collision shapes: an object group
  pub objectgroup: Option<RawLayer>,
  pub animation: Vec<RawFrame>,
}

/// One frame of a tile's animation
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(crate) struct RawFrame {
  pub tileid: u32,
  /// In milliseconds
  pub duration: u32,
}

/// A layer of any kind, told apart by `ty`
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(crate) struct RawLayer {
  pub id: Option<u32>,
  pub name: String,
  pub class: Option<String>,
  /// `tilelayer`, `objectgroup`, `imagelayer` or `group`
  #[serde(rename = "type")]
  pub ty: String,
  pub offsetx: f32,
  pub offsety: f32,
  pub opacity: Option<f32>,
  pub visible: Option<bool>,
  pub tintcolor: Option<String>,
  pub parallaxx: Option<f32>,
  pub parallaxy: Option<f32>,
  /// The blend mode of Tiled 1.12
  pub mode: Option<String>,
  pub width: Option<u32>,
  pub height: Option<u32>,
  pub data: Option<RawData>,
  pub encoding: Option<String>,
  pub compression: Option<String>,
  /// Present only in infinite maps
  pub chunks: Option<Vec<Value>>,
  pub objects: Vec<RawObject>,
  pub image: Option<String>,
  pub repeatx: bool,
  pub repeaty: bool,
  /// The children of a group
  pub layers: Vec<RawLayer>,
  pub properties: Vec<RawProperty>,
}

/// A tile layer's cells: a list of gids, or text to decode (CSV in XML,
/// base64 in both)
#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub(crate) enum RawData {
  Array(Vec<u32>),
  Text(String),
}

/// A point of a polygon or polyline
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub(crate) struct RawPoint {
  pub x: f64,
  pub y: f64,
}

/// The text of a text object
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub(crate) struct RawText {
  pub text: String,
}

/// An object of an object group. Everything an instance of a template can
/// override is an `Option`, so an absent field takes the template's value
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub(crate) struct RawObject {
  pub id: u32,
  pub name: Option<String>,
  #[serde(alias = "type")]
  pub class: Option<String>,
  pub x: Option<f64>,
  pub y: Option<f64>,
  pub width: Option<f64>,
  pub height: Option<f64>,
  /// In degrees, clockwise
  pub rotation: Option<f64>,
  pub visible: Option<bool>,
  /// Set for a tile object
  pub gid: Option<u32>,
  pub ellipse: bool,
  pub point: bool,
  pub capsule: bool,
  pub polygon: Option<Vec<RawPoint>>,
  pub polyline: Option<Vec<RawPoint>>,
  pub text: Option<RawText>,
  /// The template file, if the object comes from one
  pub template: Option<String>,
  pub properties: Vec<RawProperty>,
}

/// A template file: an object, and the tileset its tile comes from
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(crate) struct RawTemplate {
  pub tileset: Option<RawTilesetRef>,
  pub object: RawObject,
}
