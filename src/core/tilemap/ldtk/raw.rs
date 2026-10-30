//! What an LDtk file says, named as LDtk's JSON names it. Only what the loader
//! reads is here; serde skips the rest. Fields a file may omit have defaults.

use serde::Deserialize;
use serde_json::Value;

/// Just enough of a project to read its version before the rest is trusted
#[derive(Deserialize)]
pub(super) struct RawVersion {
  #[serde(rename = "jsonVersion", default)]
  pub json_version: String,
}

#[derive(Deserialize)]
pub(super) struct RawProject {
  #[serde(default)]
  pub defs: RawDefs,
  #[serde(default)]
  pub levels: Vec<RawLevel>,
  #[serde(default)]
  pub worlds: Vec<RawWorld>,
  #[serde(rename = "worldLayout", default)]
  pub world_layout: Option<String>,
  #[serde(rename = "defaultGridSize", default)]
  pub default_grid_size: Option<i64>,
}

#[derive(Deserialize, Default)]
pub(super) struct RawDefs {
  #[serde(default)]
  pub layers: Vec<RawLayerDef>,
  #[serde(default)]
  pub tilesets: Vec<RawTilesetDef>,
}

#[derive(Deserialize)]
pub(super) struct RawLayerDef {
  pub uid: i64,
  #[serde(default, rename = "intGridValues")]
  pub int_grid_values: Vec<RawIntGridValue>,
  #[serde(default, rename = "parallaxFactorX")]
  pub parallax_x: f64,
  #[serde(default, rename = "parallaxFactorY")]
  pub parallax_y: f64,
}

#[derive(Deserialize)]
pub(super) struct RawIntGridValue {
  pub value: i32,
  #[serde(default)]
  pub identifier: Option<String>,
}

#[derive(Deserialize)]
pub(super) struct RawTilesetDef {
  pub uid: i64,
  #[serde(default)]
  pub identifier: String,
  #[serde(default, rename = "relPath")]
  pub rel_path: Option<String>,
  #[serde(default, rename = "pxWid")]
  pub px_wid: i64,
  #[serde(default, rename = "pxHei")]
  pub px_hei: i64,
  #[serde(default, rename = "tileGridSize")]
  pub tile_grid_size: i64,
  #[serde(default)]
  pub spacing: i64,
  #[serde(default)]
  pub padding: i64,
  #[serde(default, rename = "__cWid")]
  pub c_wid: i64,
  #[serde(default, rename = "__cHei")]
  pub c_hei: i64,
}

#[derive(Deserialize)]
pub(super) struct RawWorld {
  #[serde(default)]
  pub identifier: String,
  #[serde(rename = "worldLayout", default)]
  pub world_layout: Option<String>,
  #[serde(default)]
  pub levels: Vec<RawLevel>,
}

#[derive(Deserialize)]
pub(super) struct RawLevel {
  #[serde(default)]
  pub identifier: String,
  #[serde(default)]
  pub iid: String,
  #[serde(default, rename = "worldX")]
  pub world_x: i64,
  #[serde(default, rename = "worldY")]
  pub world_y: i64,
  #[serde(default, rename = "pxWid")]
  pub px_wid: i64,
  #[serde(default, rename = "pxHei")]
  pub px_hei: i64,
  #[serde(default, rename = "__bgColor")]
  pub bg_color: Option<String>,
  #[serde(default, rename = "externalRelPath")]
  pub external_rel_path: Option<String>,
  #[serde(default, rename = "fieldInstances")]
  pub field_instances: Vec<RawField>,
  #[serde(default, rename = "layerInstances")]
  pub layer_instances: Option<Vec<RawLayer>>,
  #[serde(default, rename = "__neighbours")]
  pub neighbours: Vec<RawNeighbour>,
}

#[derive(Deserialize)]
pub(super) struct RawNeighbour {
  #[serde(rename = "levelIid")]
  pub level_iid: String,
  pub dir: String,
}

#[derive(Deserialize)]
pub(super) struct RawLayer {
  #[serde(default, rename = "__identifier")]
  pub identifier: String,
  #[serde(rename = "__type")]
  pub ty: String,
  #[serde(default, rename = "__cWid")]
  pub c_wid: i64,
  #[serde(default, rename = "__cHei")]
  pub c_hei: i64,
  #[serde(default, rename = "__gridSize")]
  pub grid_size: i64,
  #[serde(default = "one", rename = "__opacity")]
  pub opacity: f64,
  #[serde(default, rename = "__pxTotalOffsetX")]
  pub offset_x: f64,
  #[serde(default, rename = "__pxTotalOffsetY")]
  pub offset_y: f64,
  #[serde(default, rename = "__tilesetDefUid")]
  pub tileset_uid: Option<i64>,
  #[serde(default, rename = "overrideTilesetUid")]
  pub override_tileset_uid: Option<i64>,
  #[serde(default, rename = "layerDefUid")]
  pub layer_def_uid: i64,
  #[serde(default = "yes")]
  pub visible: bool,
  #[serde(default, rename = "intGridCsv")]
  pub int_grid_csv: Vec<i32>,
  #[serde(default, rename = "autoLayerTiles")]
  pub auto_layer_tiles: Vec<RawTile>,
  #[serde(default, rename = "gridTiles")]
  pub grid_tiles: Vec<RawTile>,
  #[serde(default, rename = "entityInstances")]
  pub entity_instances: Vec<RawEntity>,
}

fn one() -> f64 {
  1.0
}

fn yes() -> bool {
  true
}

#[derive(Deserialize)]
pub(super) struct RawTile {
  pub px: [i64; 2],
  #[serde(default)]
  pub f: u8,
  pub t: i64,
}

#[derive(Deserialize)]
pub(super) struct RawEntity {
  #[serde(default, rename = "__identifier")]
  pub identifier: String,
  #[serde(default)]
  pub iid: String,
  #[serde(default)]
  pub width: f64,
  #[serde(default)]
  pub height: f64,
  #[serde(default)]
  pub px: [f64; 2],
  #[serde(default, rename = "__pivot")]
  pub pivot: [f64; 2],
  #[serde(default, rename = "__tile")]
  pub tile: Option<RawRect>,
  #[serde(default, rename = "fieldInstances")]
  pub field_instances: Vec<RawField>,
}

#[derive(Deserialize)]
pub(super) struct RawRect {
  #[serde(rename = "tilesetUid")]
  pub tileset_uid: i64,
  pub x: f64,
  pub y: f64,
  pub w: f64,
  pub h: f64,
}

#[derive(Deserialize)]
pub(super) struct RawField {
  #[serde(rename = "__identifier")]
  pub identifier: String,
  #[serde(rename = "__type")]
  pub ty: String,
  #[serde(default, rename = "__value")]
  pub value: Value,
}
