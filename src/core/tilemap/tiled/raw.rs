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
  /// The lines a staggered or hexagonal map shifts by half a cell: its
  /// rows (`y`) or columns (`x`)
  pub staggeraxis: Option<String>,
  /// Which of those lines: the `odd` or the `even` ones
  pub staggerindex: Option<String>,
  /// A hexagonal map's side along the stagger axis, in pixels (Tiled takes
  /// a negative one, which the builder refuses)
  pub hexsidelength: Option<i64>,
  /// An oblique map's shift across per row and down per column, in
  /// pixels; Tiled leaves out a 0
  pub skewx: i64,
  pub skewy: i64,
  /// The order Tiled draws tiles in (`right-down` unless set)
  pub renderorder: String,
  pub infinite: bool,
  pub width: u32,
  pub height: u32,
  pub tilewidth: u32,
  pub tileheight: u32,
  pub class: Option<String>,
  pub backgroundcolor: Option<String>,
  /// Where parallax is measured from, in pixels
  pub parallaxoriginx: f32,
  pub parallaxoriginy: f32,
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
  /// The size tiles draw at (`tile`, or `grid`: scaled to the map's cells
  /// as `fillmode` says)
  pub tilerendersize: Option<String>,
  /// The grid Tiled's tileset editor draws its overlays on (terrain and
  /// collision shapes): `orthogonal` or `isometric`
  pub grid: Option<RawTilesetGrid>,
  pub image: Option<String>,
  pub imagewidth: u32,
  pub imageheight: u32,
  /// The colour the image draws clear
  pub transparentcolor: Option<String>,
  pub tiles: Vec<RawTile>,
  pub wangsets: Vec<RawWangSet>,
  pub transformations: RawTransformations,
  pub properties: Vec<RawProperty>,
}

/// A tileset's editor grid
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(crate) struct RawTilesetGrid {
  pub orientation: String,
}

/// What a tileset's Wang sets may do to a tile
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(crate) struct RawTransformations {
  pub hflip: bool,
  pub vflip: bool,
  pub rotate: bool,
  pub preferuntransformed: bool,
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
  /// The part of its image a collection tile draws (Tiled 1.9): left out,
  /// the whole image
  pub x: Option<u32>,
  pub y: Option<u32>,
  pub width: Option<u32>,
  pub height: Option<u32>,
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
  /// An infinite map's tile layer: its cells in chunks, in the layer's
  /// `encoding` and `compression`
  pub chunks: Option<Vec<RawChunk>>,
  /// An object layer's draw order: `topdown` (Tiled's default) or `index`
  pub draworder: Option<String>,
  pub objects: Vec<RawObject>,
  pub image: Option<String>,
  /// The colour an image layer's picture draws clear
  pub transparentcolor: Option<String>,
  pub repeatx: bool,
  pub repeaty: bool,
  /// The children of a group
  pub layers: Vec<RawLayer>,
  pub properties: Vec<RawProperty>,
}

/// A chunk of an infinite map's tile layer: where its first cell is, its
/// size in cells and its gids, row by row. Its numbers are read wider than
/// a cell's, so a chunk past what a cell holds is refused naming it, and
/// each is `None` where the file leaves it out, which the build refuses
/// naming it (`check_chunk`)
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(crate) struct RawChunk {
  pub x: Option<i64>,
  pub y: Option<i64>,
  pub width: Option<i64>,
  pub height: Option<i64>,
  pub data: Option<RawData>,
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
