//! LDtk's projects as registry assets: each level is a [`TileMap`] over the
//! project's tilesets, with the worlds' layouts and neighbours beside them.

mod build;
mod raw;

use std::sync::Arc;

use glam::{IVec2, UVec2, Vec2};

use self::build::{build_project, check_project};
use self::raw::{RawProject, RawVersion};
use super::tiled::json::parse;
use super::{Properties, TileMap, Tileset};
use crate::core::asset::{Asset, AssetError, AssetLoader, CheckContext, LoadContext};

/// Which way a neighbouring level lies from a level, as LDtk works it out
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum NeighbourDirection {
  North,
  South,
  East,
  West,
  NorthEast,
  NorthWest,
  SouthEast,
  SouthWest,
  /// On a greater world depth
  Above,
  /// On a lower world depth
  Below,
  /// Overlapping, on the same depth
  Overlap,
}

/// A level touching another
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Neighbour {
  /// The neighbour's index in the same [`WorldLayout::levels`]
  pub level: usize,
  pub direction: NeighbourDirection,
}

/// Where a level sits in its world
#[derive(Clone, Debug)]
pub struct WorldLevel {
  /// The level's identifier
  pub name: String,
  /// The top-left corner of its rectangle in world pixels
  pub position: Vec2,
  pub size: Vec2,
  /// Where the map's pixel (0, 0) sits in world pixels (its tile origin),
  /// where a root spawning the level puts its `Transform`: `position` for
  /// a bounded map, whose rectangle starts there (except an oblique map
  /// skewed left or up); for an infinite map, cell (0, 0)'s corner, its
  /// rectangle starting at its top-left tile
  pub origin: Vec2,
  /// The world depth: LDtk's `worldDepth`, where each depth is a floor of
  /// its own (a level on another depth is above or below this one, not
  /// beside it); 0 for a Tiled world's levels
  pub depth: i32,
  pub neighbours: Vec<Neighbour>,
}

/// The levels of a world and where they lie
#[derive(Clone, Debug)]
pub struct WorldLayout {
  pub name: String,
  pub levels: Vec<WorldLevel>,
}

/// One level of a project
#[derive(Clone, Debug)]
pub struct LdtkLevel {
  pub identifier: String,
  pub iid: String,
  /// The world's name in a multi-world project; `None` in a single-world one
  pub world: Option<String>,
  /// The top-left corner in world pixels
  pub position: IVec2,
  /// In pixels
  pub size: UVec2,
  /// The world depth (`worldDepth`): levels on other depths are floors
  /// above or below this one
  pub depth: i32,
  /// The level's custom fields
  pub fields: Properties,
  /// Indices into the level's world's [`WorldLayout::levels`]
  pub neighbours: Vec<Neighbour>,
  /// The level's layers, bottom first, over the project's tilesets
  pub map: TileMap,
}

/// A loaded LDtk project
#[derive(Clone, Debug)]
pub struct LdtkProject {
  /// Every level of every world, each world's levels in order
  pub levels: Vec<LdtkLevel>,
  pub worlds: Vec<WorldLayout>,
  /// The project's tilesets, in definition order; the maps embed these
  pub tilesets: Vec<Arc<Tileset>>,
  /// Where the project names its tilesets' images
  pub(crate) dependency_places: super::tiled::build::DependencyPlaces,
}

impl Asset for LdtkProject {}

impl LdtkProject {
  /// The first level whose identifier or iid is `name_or_iid`
  pub fn level(&self, name_or_iid: &str) -> Option<&LdtkLevel> {
    self
      .levels
      .iter()
      .find(|l| l.identifier == name_or_iid || l.iid == name_or_iid)
  }
}

/// Loads LDtk projects (`.ldtk`) as [`LdtkProject`]s. Levels kept in separate
/// `.ldtkl` files are read with the project, so saving one loads it again;
/// tileset images are dependencies
#[derive(Default)]
pub struct LdtkLoader;

/// The newest JSON version this loader reads: LDtk 1.5.x
const SUPPORTED: (u32, u32) = (1, 5);

/// Fails if `version` is a newer schema than [`SUPPORTED`]
fn check_version(path: &crate::core::asset::AssetPath, version: &str) -> Result<(), AssetError> {
  let mut parts = version.split('.').map(|p| p.parse::<u32>());
  let (Some(Ok(major)), Some(Ok(minor))) = (parts.next(), parts.next()) else {
    return Err(AssetError::decode(
      path,
      format!("jsonVersion {version:?} is not a version like \"1.5.3\""),
    ));
  };
  if (major, minor) > SUPPORTED {
    return Err(AssetError::decode(
      path,
      format!(
        "jsonVersion {version:?} is newer than this loader reads ({}.{}.x)",
        SUPPORTED.0, SUPPORTED.1
      ),
    ));
  }
  Ok(())
}

impl AssetLoader for LdtkLoader {
  type Asset = LdtkProject;
  type Settings = ();

  fn extensions(&self) -> &[&'static str] {
    &["ldtk"]
  }

  fn load(
    &self,
    bytes: Vec<u8>,
    _: &(),
    cx: &mut LoadContext<'_>,
  ) -> Result<LdtkProject, AssetError> {
    let path = cx.path().clone();
    let version: RawVersion = parse(&path, &bytes)?;
    check_version(&path, &version.json_version)?;
    let raw: RawProject = parse(&path, &bytes)?;
    build_project(raw, cx)
  }

  fn check(&self, project: &LdtkProject, cx: &CheckContext<'_>) -> Result<(), AssetError> {
    check_project(project, cx)
  }

  /// A tileset image that did not load, named with its tileset
  fn dependency_failed(&self, project: &LdtkProject, error: &AssetError) -> Option<AssetError> {
    project.dependency_places.explain(error)
  }
}

#[cfg(test)]
mod tests {
  use std::path::Path;

  use glam::{IVec2, UVec2, Vec2};

  use super::*;
  use crate::core::asset::{AssetServer, AssetsView, MemorySource, insert_registry};
  use crate::core::ecs::kernel::World;
  use crate::core::tilemap::{
    GridShape, LayerKind, MapGrid, MapLayer, PropertyValue, Tile, TileFlip, TilesetImage,
    TilesetSource,
  };
  use crate::core::ui::Color;

  const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/tilemap/ldtk");

  fn read_tree(root: &Path, dir: &Path, prefix: &str, files: &MemorySource) {
    let Ok(entries) = std::fs::read_dir(dir) else {
      panic!("{dir:?} is not readable")
    };
    for entry in entries.flatten() {
      let path = entry.path();
      if path.is_dir() {
        read_tree(root, &path, prefix, files);
      } else if let (Ok(rel), Ok(bytes)) = (path.strip_prefix(root), std::fs::read(&path)) {
        let rel = rel.to_string_lossy().replace('\\', "/");
        files.insert(&format!("{prefix}{rel}"), bytes);
      }
    }
  }

  /// The fixture files under `prefix`, plus `extra` (path, text) files
  fn files_at(prefix: &str, extra: &[(&str, String)]) -> MemorySource {
    let files = MemorySource::new();
    read_tree(Path::new(FIXTURES), Path::new(FIXTURES), prefix, &files);
    for (path, text) in extra {
      files.insert(path, text.as_bytes());
    }
    files
  }

  fn world_over(files: MemorySource) -> World {
    let mut world = World::new();
    insert_registry(&mut world, AssetServer::new(files));
    world
  }

  fn load_in(world: &mut World, path: &str) -> Result<LdtkProject, String> {
    let mut assets = AssetsView::new(world);
    let handle = assets
      .load_now::<LdtkProject>(path)
      .map_err(|e| e.report())?;
    assets
      .get(&handle)
      .cloned()
      .ok_or_else(|| "not in the store".to_string())
  }

  fn load(path: &str) -> LdtkProject {
    let mut world = world_over(files_at("", &[]));
    match load_in(&mut world, path) {
      Ok(project) => project,
      Err(error) => panic!("{path}: {error}"),
    }
  }

  /// The fixture `name` with `from` replaced by `to` in its text, loaded as `m.ldtk`
  fn load_edited(name: &str, from: &str, to: &str) -> Result<LdtkProject, String> {
    let text = std::fs::read_to_string(Path::new(FIXTURES).join(name)).expect("fixture");
    assert!(text.contains(from), "{from:?} is not in {name}");
    let mut world = world_over(files_at("", &[("m.ldtk", text.replace(from, to))]));
    load_in(&mut world, "m.ldtk")
  }

  fn failure(name: &str, from: &str, to: &str) -> String {
    match load_edited(name, from, to) {
      Ok(_) => panic!("{from:?} -> {to:?} loaded"),
      Err(error) => error,
    }
  }

  fn layer<'a>(level: &'a LdtkLevel, name: &str) -> &'a MapLayer {
    match level.map.layers.iter().find(|l| l.info.name == name) {
      Some(layer) => layer,
      None => panic!("no layer {name}"),
    }
  }

  fn cell(level: &LdtkLevel, layer_name: &str, x: i32, y: i32) -> Option<Tile> {
    match &layer(level, layer_name).kind {
      LayerKind::Tiles { tiles: grid, .. } => grid.get(IVec2::new(x, y)),
      _ => panic!("{layer_name} is not a tile layer"),
    }
  }

  /// Everything a level holds, as text, to compare two loads
  fn describe(level: &LdtkLevel) -> String {
    let mut out = format!(
      "{} {} {:?} {:?} {:?} {:?} {:?} {:?}\n",
      level.identifier,
      level.iid,
      level.position,
      level.size,
      level.fields,
      level.neighbours,
      level.map.background,
      level.map.size
    );
    for l in &level.map.layers {
      out += &format!("{:?}\n", l.info);
      match &l.kind {
        LayerKind::Tiles {
          tiles,
          intgrid,
          grid,
        } => {
          out += &format!("grid {grid:?}\n");
          let extent = tiles.extent();
          for y in extent.min.y..extent.max.y {
            for x in extent.min.x..extent.max.x {
              out += &format!("{:?};", tiles.get(IVec2::new(x, y)));
            }
          }
          if let Some(ig) = intgrid {
            let mut names: Vec<_> = ig.names.iter().collect();
            names.sort();
            out += &format!("\n{:?} {:?}", ig.values, names);
          }
        }
        LayerKind::Objects(objects) => {
          for o in objects {
            out += &format!("{o:?}\n");
          }
        }
        LayerKind::Image(_) => out += "image",
      }
      out.push('\n');
    }
    out
  }

  #[test]
  fn internal_and_external_levels_load_alike() {
    let internal = load("internal.ldtk");
    let external = load("external.ldtk");
    assert_eq!(internal.levels.len(), 2);
    assert_eq!(external.levels.len(), 2);
    for (a, b) in internal.levels.iter().zip(&external.levels) {
      assert_eq!(describe(a), describe(b));
    }
    let level = &internal.levels[0];
    assert_eq!(level.identifier, "Level_0");
    assert_eq!(level.iid, "11111111-0000-4000-8000-000000000001");
    assert_eq!(level.position, IVec2::ZERO);
    assert_eq!(level.size, UVec2::new(64, 48));
    assert_eq!(level.map.size, UVec2::new(4, 3));
    assert_eq!(level.map.tile_size, UVec2::splat(16));
    assert_eq!(level.map.background, Some(Color::rgb(0x39, 0x86, 0x66)));
    assert_eq!(level.fields.string("title"), Some("Start"));
    assert_eq!(level.fields.bool("dark"), Some(true));
    assert_eq!(level.map.properties, level.fields);
    assert_eq!(internal.levels[1].position, IVec2::new(64, 0));
    assert!(internal.levels[1].fields.is_empty());
    assert_eq!(internal.worlds.len(), 1);
    assert!(internal.levels.iter().all(|l| l.world.is_none()));
    assert!(std::ptr::eq(
      internal.level("Level_1").expect("by name"),
      internal
        .level("11111111-0000-4000-8000-000000000002")
        .expect("by iid")
    ));
    assert!(internal.level("nope").is_none());
  }

  #[test]
  fn layers_draw_in_the_order_ldtk_lists_them_reversed() {
    // LDtk lists the top layer first; the map's layer 0 draws first
    let project = load("internal.ldtk");
    let names: Vec<_> = project.levels[0]
      .map
      .layers
      .iter()
      .map(|l| l.info.name.as_str())
      .collect();
    // Collision stacks a tile on another: its sub-layer, part of it, comes
    // after the editor's layers
    assert_eq!(
      names,
      ["Decor", "Collision", "Props", "Entities", "Collision#1"]
    );
  }

  #[test]
  fn int_grid_values_keep_their_identifiers() {
    let project = load("internal.ldtk");
    let level = &project.levels[0];
    let collision = layer(level, "Collision");
    let LayerKind::Tiles {
      intgrid: Some(ig), ..
    } = &collision.kind
    else {
      panic!("Collision holds no int grid")
    };
    assert_eq!(ig.size, UVec2::new(4, 3));
    assert_eq!(ig.values, [1, 1, 1, 1, 1, 0, 2, 1, 3, 1, 1, 1]);
    assert_eq!(ig.names.get(&1).map(String::as_str), Some("Wall"));
    assert_eq!(ig.names.get(&2).map(String::as_str), Some("Water"));
    assert_eq!(ig.names.get(&3), None);
    assert_eq!(ig.get(IVec2::new(2, 1)), Some(2));
    assert_eq!(ig.get(IVec2::new(1, 1)), None);
    assert!(collision.info.collides);
    assert!(!layer(level, "Decor").info.collides);
    assert!(!layer(level, "Entities").info.collides);
    assert!(matches!(
      layer(level, "Decor").kind,
      LayerKind::Tiles { intgrid: None, .. }
    ));
    assert_eq!(collision.info.id, 1);
  }

  #[test]
  fn auto_tiles_and_grid_tiles_keep_their_flips() {
    let project = load("internal.ldtk");
    let level = &project.levels[0];
    let (h, v) = (TileFlip::HORIZONTAL, TileFlip::VERTICAL);
    // auto-tiles; a tile stacked on another in its cell is a sub-layer up
    assert_eq!(cell(level, "Collision", 0, 0), Some(Tile::new(0, 0)));
    assert_eq!(cell(level, "Collision#1", 0, 0), Some(Tile::new(0, 5)));
    assert_eq!(
      cell(level, "Collision", 1, 0),
      Some(Tile::new(0, 1).with_flip(h))
    );
    assert_eq!(
      cell(level, "Collision", 2, 0),
      Some(Tile::new(0, 2).with_flip(v))
    );
    assert_eq!(
      cell(level, "Collision", 3, 0),
      Some(Tile::new(0, 3).with_flip(h | v))
    );
    assert_eq!(cell(level, "Collision", 0, 2), None);
    // grid tiles
    assert_eq!(
      cell(level, "Decor", 1, 1),
      Some(Tile::new(0, 4).with_flip(h))
    );
    assert_eq!(
      cell(level, "Decor", 2, 1),
      Some(Tile::new(0, 6).with_flip(h | v))
    );
    assert_eq!(
      cell(level, "Decor", 3, 2),
      Some(Tile::new(0, 7).with_flip(v))
    );
    assert_eq!(cell(level, "Decor", 0, 0), None);
    // the second tileset
    assert_eq!(cell(level, "Props", 0, 2), Some(Tile::new(1, 1)));
    // every tileset is embedded, shared with the project
    assert_eq!(level.map.tilesets.len(), project.tilesets.len());
    for (source, set) in level.map.tilesets.iter().zip(&project.tilesets) {
      match source {
        TilesetSource::Embedded(own) => assert!(Arc::ptr_eq(own, set)),
        TilesetSource::Asset(_) => panic!("a level's tileset is embedded"),
      }
    }
    let tiles = &project.tilesets[0];
    assert_eq!(tiles.name, "Tiles");
    assert_eq!((tiles.columns, tiles.tile_count), (4, 8));
    assert_eq!(tiles.tile_size, UVec2::splat(16));
    let props = &project.tilesets[1];
    assert_eq!((props.columns, props.tile_count, props.spacing), (2, 2, 1));
    assert!(matches!(tiles.image, TilesetImage::Atlas { size, .. } if size == UVec2::new(64, 32)));
    // a tileset with no image of its own (LDtk's built-in icons) has none to draw
    assert!(matches!(
      project.tilesets[2].image,
      TilesetImage::Collection
    ));
  }

  #[test]
  fn tile_enum_tags_and_custom_data_read_as_class_and_properties() {
    // The Tiles tileset tags its tiles with SomeEnum's values: tile 1 A,
    // tile 2 A and B; tiles 1 and 3 carry custom data
    let project = load("internal.ldtk");
    let tiles = &project.tilesets[0];
    let one = tiles.tile(1).expect("tile 1 has data");
    assert_eq!(one.class.as_deref(), Some("A"));
    assert_eq!(one.properties.bool("A"), Some(true));
    assert_eq!(one.properties.string("custom_data"), Some("slippery"));
    let two = tiles.tile(2).expect("tile 2 has data");
    assert_eq!(
      two.class.as_deref(),
      Some("A"),
      "the first tag in the enum's order"
    );
    assert_eq!(
      (two.properties.bool("A"), two.properties.bool("B")),
      (Some(true), Some(true))
    );
    assert_eq!(two.properties.get("custom_data"), None);
    let three = tiles.tile(3).expect("tile 3 has data");
    assert_eq!(three.class, None);
    assert_eq!(
      three.properties.string("custom_data"),
      Some("{\"speed\": 2}")
    );
    assert!(tiles.tile(0).is_none());
    // a tag on a tile the tileset does not have fails, naming the tileset
    let error = json_failure(|v| {
      v["defs"]["tilesets"][0]["enumTags"][0]["tileIds"] = serde_json::json!([8]);
    });
    assert!(
      error.contains("tileset \"Tiles\"") && error.contains("tile 8"),
      "{error}"
    );
  }

  /// A loaded tile layer, as the sub-layer tests compare them
  struct SubLayer {
    name: String,
    id: u32,
    offset: Vec2,
    opacity: f32,
    collides: bool,
    intgrid: bool,
    /// Its tiles as (cell, id)
    tiles: Vec<(IVec2, u32)>,
  }

  fn sub_layers(level: &LdtkLevel) -> Vec<SubLayer> {
    level
      .map
      .layers
      .iter()
      .map(|l| {
        let LayerKind::Tiles {
          tiles: grid,
          intgrid,
          ..
        } = &l.kind
        else {
          panic!("{} is not a tile layer", l.info.name)
        };
        let mut tiles = Vec::new();
        let extent = grid.extent();
        for y in extent.min.y..extent.max.y {
          for x in extent.min.x..extent.max.x {
            if let Some(t) = grid.get(IVec2::new(x, y)) {
              tiles.push((IVec2::new(x, y), t.id));
            }
          }
        }
        SubLayer {
          name: l.info.name.clone(),
          id: l.info.id,
          offset: l.info.offset,
          opacity: l.info.opacity,
          collides: l.info.collides,
          intgrid: intgrid.is_some(),
          tiles,
        }
      })
      .collect()
  }

  #[test]
  fn stacked_offset_and_translucent_tiles_load_as_sub_layers() {
    // Shaped like LDtk's saved output: Ground stacks a tile on another,
    // moves one 4 px by a rule's offset and draws one at half alpha; the
    // IntGrid Collision stacks two
    let project = load("stacked.ldtk");
    let level = &project.levels[0];
    let layers = sub_layers(level);
    let names: Vec<&str> = layers.iter().map(|l| l.name.as_str()).collect();
    // the editor's layers in its order, then the sub-layers, each part of
    // its layer
    assert_eq!(
      names,
      [
        "Collision",
        "Ground",
        "Collision#1",
        "Ground#1",
        "Ground#2",
        "Ground#3"
      ]
    );
    let parts: Vec<Option<u32>> = level.map.layers.iter().map(|l| l.info.part_of).collect();
    assert_eq!(parts, [None, None, Some(31), Some(30), Some(30), Some(30)]);
    let c = IVec2::new;
    // the IntGrid and collision stay on the layer's own
    assert_eq!(layers[0].id, 31);
    assert!(layers[0].collides && layers[0].intgrid);
    assert_eq!(layers[0].tiles, [(c(1, 1), 2)]);
    assert_eq!((layers[2].collides, layers[2].intgrid), (false, false));
    assert_eq!(layers[2].tiles, [(c(1, 1), 1)], "stacked above");
    // Ground's own holds the plain tiles
    assert_eq!(layers[1].id, 30);
    assert_eq!(layers[1].tiles, [(c(0, 0), 0), (c(3, 2), 4)]);
    // then by depth, each depth as LDtk lists them: the moved tile, the
    // translucent one, and the stacked one over the red, each at its own
    // offset and opacity over its layer's
    assert_eq!(
      (layers[3].offset, layers[3].opacity),
      (Vec2::new(4.0, 4.0), 1.0)
    );
    assert_eq!(layers[3].tiles, [(c(2, 0), 2)]);
    assert_eq!((layers[4].offset, layers[4].opacity), (Vec2::ZERO, 0.5));
    assert_eq!(layers[4].tiles, [(c(0, 2), 3)]);
    assert_eq!((layers[5].offset, layers[5].opacity), (Vec2::ZERO, 1.0));
    assert_eq!(layers[5].tiles, [(c(0, 0), 1)]);
    for sub in [2, 3, 4, 5] {
      assert!(
        !layers[sub].collides && !layers[sub].intgrid,
        "{}",
        layers[sub].name
      );
    }
    // sub-layer ids are their own, and the same each load
    let ids: Vec<u32> = layers.iter().map(|l| l.id).collect();
    let mut unique = ids.clone();
    unique.sort_unstable();
    unique.dedup();
    assert_eq!(unique.len(), ids.len(), "{ids:?}");
    let again: Vec<u32> = sub_layers(&load("stacked.ldtk").levels[0])
      .iter()
      .map(|l| l.id)
      .collect();
    assert_eq!(again, ids);
  }

  #[test]
  fn a_later_layer_keeps_its_place_whatever_stacking_a_save_adds() {
    // internal.ldtk's Collision stacks a tile: its sub-layer is listed after
    // the editor's layers, part of Collision, so Props is still third
    let project = load("internal.ldtk");
    let level = &project.levels[0];
    let names: Vec<&str> = level
      .map
      .layers
      .iter()
      .map(|l| l.info.name.as_str())
      .collect();
    assert_eq!(
      names,
      ["Decor", "Collision", "Props", "Entities", "Collision#1"]
    );
    assert_eq!(level.map.layers[4].info.part_of, Some(1));
    assert!(
      level.map.layers[..4]
        .iter()
        .all(|l| l.info.part_of.is_none())
    );
    // a save stacks a tile in Decor too: every editor layer keeps its place
    let edited = load_json(|v| {
      let layers = v["levels"][0]["layerInstances"]
        .as_array_mut()
        .expect("layers");
      let decor = layers
        .iter_mut()
        .find(|l| l["__identifier"] == "Decor")
        .expect("decor");
      let first = decor["gridTiles"][0].clone();
      decor["gridTiles"]
        .as_array_mut()
        .expect("tiles")
        .push(first);
    })
    .expect("loads");
    let level = &edited.levels[0];
    for (index, name) in ["Decor", "Collision", "Props", "Entities"]
      .iter()
      .enumerate()
    {
      assert_eq!(level.map.layer_index(name), Some(index), "{name}");
    }
    let decor_id = level.map.layers[0].info.id;
    assert!(
      level.map.layers[4..]
        .iter()
        .any(|l| l.info.name == "Decor#1" && l.info.part_of == Some(decor_id))
    );
  }

  #[test]
  fn a_sub_layer_keeps_its_id_when_a_save_adds_another_before_it() {
    // A save adds a tile at another offset, listed first: the sub-layers'
    // places and names move up, their ids (the layer's uid and their key)
    // do not, so a reload keeps each one's entity for the same tiles
    let before = sub_layers(&load("stacked.ldtk").levels[0]);
    let text = std::fs::read_to_string(Path::new(FIXTURES).join("stacked.ldtk")).expect("fixture");
    let mut value: serde_json::Value = serde_json::from_str(&text).expect("json");
    let ground = &mut value["levels"][0]["layerInstances"][0];
    assert_eq!(ground["__identifier"], "Ground");
    let tiles = ground["autoLayerTiles"].as_array_mut().expect("tiles");
    tiles.insert(
      0,
      serde_json::json!({"px": [33, 17], "src": [0, 0], "f": 0, "t": 0, "d": [100, 6], "a": 1}),
    );
    let mut world = world_over(files_at("", &[("m.ldtk", value.to_string())]));
    let edited = load_in(&mut world, "m.ldtk").expect("loads");
    let after = sub_layers(&edited.levels[0]);
    let id_of = |layers: &[SubLayer], offset: Vec2, opacity: f32| {
      layers
        .iter()
        .find(|l| {
          l.name.starts_with("Ground")
            && l.offset == offset
            && l.opacity == opacity
            && !l.tiles.is_empty()
        })
        .map(|l| l.id)
    };
    assert_eq!(after.len(), before.len() + 1);
    for (offset, opacity) in [(Vec2::new(4.0, 4.0), 1.0), (Vec2::ZERO, 0.5)] {
      assert!(id_of(&before, offset, opacity).is_some());
      assert_eq!(
        id_of(&after, offset, opacity),
        id_of(&before, offset, opacity)
      );
    }
  }

  #[test]
  fn a_layer_needing_more_sub_layers_than_it_may_have_puts_the_rest_in_the_nearest_and_warns_once()
  {
    // Twenty tiles in a row of cells, each a pixel further into its cell:
    // twenty sub-layers asked for, sixteen made, four tiles snapped
    let project = load_json(|v| {
      let layers = v["levels"][0]["layerInstances"]
        .as_array_mut()
        .expect("layers");
      let decor = layers
        .iter_mut()
        .find(|l| l["__identifier"] == "Decor")
        .expect("decor");
      decor["__identifier"] = "Decor-9f4a".into();
      let tiles: Vec<serde_json::Value> = (0..20)
        .map(|i| {
          let (cx, cy) = (i % 4, (i / 4) % 3);
          serde_json::json!({"px": [cx * 16 + i % 16, cy * 16], "src": [0, 0], "f": 0, "t": 1, "d": [i], "a": 1})
        })
        .collect();
      decor["gridTiles"] = tiles.into();
    })
    .expect("loads");
    let level = &project.levels[0];
    let decor = level
      .map
      .layers
      .iter()
      .filter(|l| l.info.name.starts_with("Decor-9f4a"))
      .count();
    assert_eq!(decor, 16);
    let warned: Vec<String> = crate::core::dev_log::get_entries()
      .into_iter()
      .filter(|e| e.message.contains("Decor-9f4a"))
      .map(|e| e.message)
      .collect();
    assert_eq!(warned.len(), 1, "warned once, naming the layer: {warned:?}");
    assert!(warned[0].contains("4 tiles"), "{}", warned[0]);
  }

  fn door(level: &LdtkLevel) -> &crate::core::tilemap::MapObjectData {
    let LayerKind::Objects(objects) = &layer(level, "Entities").kind else {
      panic!("Entities holds no objects")
    };
    match objects
      .iter()
      .find(|o| o.class.as_deref() == Some("EntityFieldsTest"))
    {
      Some(o) => o,
      None => panic!("no door"),
    }
  }

  #[test]
  fn entities_load_with_every_field_type() {
    let project = load("internal.ldtk");
    let level = &project.levels[0];
    let o = door(level);
    assert_eq!(o.id, "11111111-0000-4000-8000-00000000d00a");
    assert_eq!(o.name, "EntityFieldsTest");
    assert_eq!(o.size, Vec2::new(16.0, 32.0));
    // px is the pivot (0.5, 0.5): the top-left is half the size back
    assert_eq!(o.position, Vec2::new(16.0, 8.0));
    assert_eq!(o.rotation, 0.0);
    assert_eq!(o.shape, crate::core::tilemap::ObjectShape::Rect);
    assert_eq!(o.tile, Some(Tile::new(0, 5)));
    assert!(o.visible);
    let p = &o.properties;
    assert_eq!(p.get("Integer"), Some(&PropertyValue::Int(1)));
    assert_eq!(p.get("Float"), Some(&PropertyValue::Float(1.5)));
    assert_eq!(p.get("Boolean"), Some(&PropertyValue::Bool(false)));
    assert_eq!(p.string("String_singleLine"), Some("foo"));
    assert_eq!(p.string("String_multiLines"), Some("foo bar"));
    assert_eq!(
      p.get("Enum"),
      Some(&PropertyValue::Enum {
        ty: "SomeEnum".into(),
        value: "A".into()
      })
    );
    assert_eq!(
      p.get("ExternEnum"),
      Some(&PropertyValue::Enum {
        ty: "AnExternEnum".into(),
        value: "Value1".into()
      })
    );
    assert_eq!(p.color("Color"), Some(Color::rgb(0xD1, 0x81, 0xE8)));
    let point = |cx: i64, cy: i64| {
      let mut fields = Properties::default();
      fields.insert("cx", PropertyValue::Int(cx));
      fields.insert("cy", PropertyValue::Int(cy));
      PropertyValue::Class {
        ty: "Point".into(),
        fields,
      }
    };
    assert_eq!(p.get("Point"), Some(&point(4, 17)));
    assert_eq!(
      p.get("FilePath"),
      Some(&PropertyValue::File("README.md".into()))
    );
    assert_eq!(
      p.get("Array_Integer"),
      Some(&PropertyValue::List(vec![
        PropertyValue::Int(1),
        PropertyValue::Int(2),
        PropertyValue::Int(3)
      ]))
    );
    let Some(PropertyValue::List(items)) = p.get("Array_Enum") else {
      panic!("Array_Enum is no list")
    };
    assert_eq!(items.len(), 3);
    assert_eq!(
      items[2],
      PropertyValue::Enum {
        ty: "SomeEnum".into(),
        value: "C".into()
      }
    );
    assert_eq!(
      p.get("Array_points"),
      Some(&PropertyValue::List(vec![
        point(14, 19),
        point(14, 17),
        point(18, 17),
        point(18, 19)
      ]))
    );
    assert_eq!(
      p.get("Array_multilines"),
      Some(&PropertyValue::List(vec![
        PropertyValue::String("<foo/> <bar>5</bar>".into()),
        PropertyValue::String("<hello>world</hello>".into())
      ]))
    );
    let Some(PropertyValue::Class { ty, fields }) = p.get("Tile") else {
      panic!("Tile is no class")
    };
    assert_eq!(ty, "Tile");
    assert_eq!(fields.string("tileset"), Some("Tiles"));
    assert_eq!(
      [
        fields.int("x"),
        fields.int("y"),
        fields.int("w"),
        fields.int("h")
      ],
      [Some(32), Some(16), Some(16), Some(16)]
    );
    // a field with no value is left out
    assert!(p.get("Unset_int").is_none());
    assert!(p.get("Unset_enum").is_none());
  }

  #[test]
  fn entities_sit_by_their_pivot() {
    let project = load("internal.ldtk");
    let LayerKind::Objects(objects) = &layer(&project.levels[0], "Entities").kind else {
      panic!("no objects")
    };
    let chest = objects
      .iter()
      .find(|o| o.id.ends_with("c0de"))
      .expect("chest");
    // pivot (0, 1) at (32, 48), 16 x 16: the top-left is (32, 32)
    assert_eq!(chest.position, Vec2::new(32.0, 32.0));
    assert_eq!(chest.tile, None);
    assert_eq!(objects.len(), 3);
  }

  #[test]
  fn entity_references_name_the_iid() {
    let project = load("internal.ldtk");
    let LayerKind::Objects(objects) = &layer(&project.levels[0], "Entities").kind else {
      panic!("no objects")
    };
    let chest = objects
      .iter()
      .find(|o| o.id.ends_with("c0de"))
      .expect("chest");
    assert_eq!(
      chest.properties.get("target"),
      Some(&PropertyValue::Object(
        "11111111-0000-4000-8000-00000000d00a".into()
      ))
    );
    // an unset reference is no reference, not an empty id
    let lone = objects
      .iter()
      .find(|o| o.id.ends_with("01ee"))
      .expect("lone");
    assert!(lone.properties.get("target").is_none());
  }

  fn world_level<'a>(layout: &'a WorldLayout, name: &str) -> (usize, &'a WorldLevel) {
    match layout
      .levels
      .iter()
      .enumerate()
      .find(|(_, l)| l.name == name)
    {
      Some(found) => found,
      None => panic!("no level {name}"),
    }
  }

  #[test]
  fn worlds_keep_level_positions_and_neighbours() {
    use NeighbourDirection::*;
    let project = load("worlds.ldtk");
    let names: Vec<_> = project.worlds.iter().map(|w| w.name.as_str()).collect();
    assert_eq!(names, ["Overworld", "Caves", "Hall"]);
    assert_eq!(project.levels.len(), 11 + 2 + 2);
    let over = &project.worlds[0];
    let (c, centre) = world_level(over, "C");
    assert_eq!(centre.position, Vec2::new(64.0, 48.0));
    assert_eq!(centre.size, Vec2::new(64.0, 48.0));
    let mut found: Vec<(String, NeighbourDirection)> = centre
      .neighbours
      .iter()
      .map(|n| (over.levels[n.level].name.clone(), n.direction))
      .collect();
    found.sort_by_key(|(name, _)| name.clone());
    let expected = [
      ("E", East),
      ("N", North),
      ("NE", NorthEast),
      ("NW", NorthWest),
      ("Over", Overlap),
      ("S", South),
      ("SE", SouthEast),
      ("SW", SouthWest),
      ("Upper", Above),
      ("W", West),
    ];
    let expected: Vec<(String, NeighbourDirection)> =
      expected.iter().map(|(n, d)| (n.to_string(), *d)).collect();
    assert_eq!(found, expected);
    // `<` is a level below; each level keeps its world depth
    let (_, upper) = world_level(over, "Upper");
    assert_eq!((centre.depth, upper.depth), (0, 1));
    assert_eq!(project.level("Upper").map(|l| l.depth), Some(1));
    assert_eq!(
      upper.neighbours,
      [Neighbour {
        level: c,
        direction: Below
      }]
    );
    // the level of the project carries the same, and its world
    let level = project.level("C").expect("C");
    assert_eq!(level.world.as_deref(), Some("Overworld"));
    assert_eq!(level.neighbours, centre.neighbours);
    assert_eq!(level.position, IVec2::new(64, 48));
    // a free layout keeps negative positions
    let caves = &project.worlds[1];
    assert_eq!(
      world_level(caves, "Cave_B").1.position,
      Vec2::new(-64.0, 16.0)
    );
    assert_eq!(world_level(caves, "Cave_B").1.size, Vec2::new(32.0, 32.0));
    assert_eq!(
      project.level("Cave_A").expect("A").world.as_deref(),
      Some("Caves")
    );
    // a linear layout has no positions in the file: the levels follow one another, 48 px apart
    let hall = &project.worlds[2];
    assert_eq!(world_level(hall, "Hall_0").1.position, Vec2::ZERO);
    assert_eq!(world_level(hall, "Hall_1").1.position, Vec2::new(0.0, 96.0));
    assert_eq!(
      project.level("Hall_1").expect("hall").position,
      IVec2::new(0, 96)
    );
    // a level's map is as big as its pixels say
    assert_eq!(
      project.level("Over").expect("over").map.size,
      UVec2::new(3, 3)
    );
  }

  #[test]
  fn single_world_levels_have_neighbours_too() {
    let project = load("internal.ldtk");
    assert_eq!(
      project.levels[0].neighbours,
      [Neighbour {
        level: 1,
        direction: NeighbourDirection::East
      }]
    );
    assert_eq!(
      project.worlds[0].levels[1].neighbours,
      [Neighbour {
        level: 0,
        direction: NeighbourDirection::West
      }]
    );
  }

  #[test]
  fn a_newer_schema_than_supported_fails_naming_both_versions() {
    let error = failure(
      "internal.ldtk",
      "\"jsonVersion\": \"1.5.3\"",
      "\"jsonVersion\": \"1.6.0\"",
    );
    assert!(error.starts_with("could not load \"m.ldtk\""), "{error}");
    assert!(error.contains("1.6.0") && error.contains("1.5"), "{error}");
    // an older one, and a later patch, still load
    assert!(
      load_edited(
        "internal.ldtk",
        "\"jsonVersion\": \"1.5.3\"",
        "\"jsonVersion\": \"1.4.1\""
      )
      .is_ok()
    );
    assert!(
      load_edited(
        "internal.ldtk",
        "\"jsonVersion\": \"1.5.3\"",
        "\"jsonVersion\": \"1.5.9\""
      )
      .is_ok()
    );
    let error = failure(
      "internal.ldtk",
      "\"jsonVersion\": \"1.5.3\"",
      "\"jsonVersion\": \"x\"",
    );
    assert!(error.contains("jsonVersion"), "{error}");
  }

  #[test]
  fn tileset_images_resolve_against_the_project_s_folder() {
    let files = files_at("game/maps/", &[]);
    let mut world = world_over(files);
    let project = load_in(&mut world, "game/maps/internal.ldtk").expect("loads");
    let TilesetImage::Atlas { image, .. } = &project.tilesets[0].image else {
      panic!("no atlas")
    };
    let server = world.resource::<AssetServer>();
    let path = server.path(image).expect("the image has a path");
    assert_eq!(path.as_str(), "game/maps/images/tiles.png");
    // and the external levels, read from beside the project
    let external = load_in(&mut world, "game/maps/external.ldtk").expect("external");
    assert_eq!(external.levels[0].map.layers.len(), 5);
  }

  /// `internal.ldtk` with `edit` applied to its JSON, loaded as `m.ldtk`
  fn load_json(edit: impl FnOnce(&mut serde_json::Value)) -> Result<LdtkProject, String> {
    let text = std::fs::read_to_string(Path::new(FIXTURES).join("internal.ldtk")).expect("fixture");
    let mut value: serde_json::Value = serde_json::from_str(&text).expect("json");
    edit(&mut value);
    let mut world = world_over(files_at("", &[("m.ldtk", value.to_string())]));
    load_in(&mut world, "m.ldtk")
  }

  fn json_failure(edit: impl FnOnce(&mut serde_json::Value)) -> String {
    match load_json(edit) {
      Ok(_) => panic!("loaded"),
      Err(error) => error,
    }
  }

  #[test]
  fn tiles_past_the_level_s_edge_are_left_out() {
    let project = load_json(|v| {
      let layers = v["levels"][0]["layerInstances"]
        .as_array_mut()
        .expect("layers");
      let decor = layers
        .iter_mut()
        .find(|l| l["__identifier"] == "Decor")
        .expect("decor");
      let tiles = decor["gridTiles"].as_array_mut().expect("tiles");
      tiles
        .push(serde_json::json!({"px": [64, 0], "src": [0, 0], "f": 0, "t": 0, "d": [0], "a": 1}));
      tiles.push(
        serde_json::json!({"px": [-16, 16], "src": [0, 0], "f": 0, "t": 0, "d": [0], "a": 1}),
      );
    })
    .expect("loads");
    assert_eq!(
      cell(&project.levels[0], "Decor", 1, 1).map(|t| t.id),
      Some(4)
    );
  }

  /// Level_0 with its Collision layer on an 8 px grid under the 16 px art
  fn mixed_grid_project() -> LdtkProject {
    load_json(|v| {
      let layers = v["levels"][0]["layerInstances"]
        .as_array_mut()
        .expect("layers");
      let col = layers
        .iter_mut()
        .find(|l| l["__identifier"] == "Collision")
        .expect("collision");
      col["__gridSize"] = 8.into();
      col["__cWid"] = 8.into();
      col["__cHei"] = 6.into();
      let mut csv = vec![0; 48];
      csv[0] = 1;
      csv[9] = 2;
      csv[47] = 1;
      col["intGridCsv"] = csv.into();
      col["autoLayerTiles"] = serde_json::json!([
        {"px": [8, 8], "src": [16, 0], "f": 0, "t": 1, "d": [0], "a": 1},
        {"px": [56, 40], "src": [0, 16], "f": 1, "t": 4, "d": [0], "a": 1}
      ]);
    })
    .expect("loads")
  }

  #[test]
  fn a_level_mixing_grid_sizes_keeps_each_layer_s_cell_size() {
    let project = mixed_grid_project();
    let level = &project.levels[0];
    // Each layer's grid square, of its own cell size
    let grid = |name: &str| match &layer(level, name).kind {
      LayerKind::Tiles { grid, .. } => *grid,
      _ => panic!("{name} is no tile layer"),
    };
    let square = |side: u32| MapGrid::new(GridShape::Orthogonal, UVec2::splat(side));
    assert_eq!(grid("Collision"), square(8));
    assert_eq!(grid("Decor"), square(16));
    assert_eq!(grid("Props"), square(16));
    // the 8 px layer has twice the cells across and down
    let LayerKind::Tiles {
      tiles: grid,
      intgrid,
      ..
    } = &layer(level, "Collision").kind
    else {
      panic!("no tiles")
    };
    assert_eq!(
      grid.bounds(),
      Some(crate::core::physics::CellRect::from_size(UVec2::new(8, 6)))
    );
    assert_eq!(intgrid.as_ref().map(|g| g.size), Some(UVec2::new(8, 6)));
    assert_eq!(
      intgrid.as_ref().and_then(|g| g.get(IVec2::new(1, 1))),
      Some(2)
    );
    assert_eq!(cell(level, "Collision", 1, 1), Some(Tile::new(0, 1)));
    assert_eq!(
      cell(level, "Collision", 7, 5),
      Some(Tile::new(0, 4).with_flip(TileFlip::HORIZONTAL))
    );
    let LayerKind::Tiles { tiles: grid, .. } = &layer(level, "Decor").kind else {
      panic!("no tiles")
    };
    assert_eq!(
      grid.bounds(),
      Some(crate::core::physics::CellRect::from_size(UVec2::new(4, 3)))
    );
    // the map's own cell is the bottom layer's
    assert_eq!(level.map.tile_size, UVec2::splat(16));
    assert_eq!(level.map.size, UVec2::new(4, 3));
    // a layer whose cell count disagrees with its grid size still fails
    let error = json_failure(|v| {
      let layers = v["levels"][0]["layerInstances"]
        .as_array_mut()
        .expect("layers");
      let col = layers
        .iter_mut()
        .find(|l| l["__identifier"] == "Collision")
        .expect("collision");
      col["__gridSize"] = 8.into();
    });
    assert!(
      error.contains("layer \"Collision\"") && error.contains("cells"),
      "{error}"
    );
  }

  #[test]
  fn a_level_s_background_colour_loads() {
    let project = load("internal.ldtk");
    assert_eq!(
      project.levels[0].map.background,
      Some(Color::rgb(0x39, 0x86, 0x66))
    );
    // the default colour, which LDtk fills in for a level of none
    assert_eq!(
      project.levels[1].map.background,
      Some(Color::rgb(0x6E, 0x6F, 0x8B))
    );
    // without `__bgColor`: the level's own, then the project's default
    let own = load_json(|v| {
      v["levels"][1]
        .as_object_mut()
        .expect("level")
        .remove("__bgColor");
      v["levels"][1]["bgColor"] = "#102030".into();
    })
    .expect("loads");
    assert_eq!(
      own.levels[1].map.background,
      Some(Color::rgb(0x10, 0x20, 0x30))
    );
    let default = load_json(|v| {
      v["levels"][1]
        .as_object_mut()
        .expect("level")
        .remove("__bgColor");
      v["defaultLevelBgColor"] = "#405060".into();
    })
    .expect("loads");
    assert_eq!(
      default.levels[1].map.background,
      Some(Color::rgb(0x40, 0x50, 0x60))
    );
    let error = json_failure(|v| {
      v["levels"][1]["__bgColor"] = "red".into();
    });
    assert!(
      error.contains("level \"Level_1\"") && error.contains("red"),
      "{error}"
    );
  }

  #[test]
  fn bad_files_fail_with_their_place() {
    let mut world = world_over(files_at(
      "",
      &[(
        "bad.ldtk",
        "{\"jsonVersion\": \"1.5.3\",\n  \"levels\": [".into(),
      )],
    ));
    let error = load_in(&mut world, "bad.ldtk").expect_err("truncated");
    assert!(error.starts_with("could not load \"bad.ldtk\""), "{error}");
    assert!(error.contains("line 2, column"), "{error}");
    // an int grid of the wrong length
    let error = json_failure(|v| {
      let layers = v["levels"][0]["layerInstances"]
        .as_array_mut()
        .expect("layers");
      let col = layers
        .iter_mut()
        .find(|l| l["__identifier"] == "Collision")
        .expect("collision");
      col["intGridCsv"].as_array_mut().expect("csv").pop();
    });
    assert!(
      error.contains("layer \"Collision\"") && error.contains("intGridCsv"),
      "{error}"
    );
    // an unknown field type
    let error = json_failure(|v| {
      let layers = v["levels"][0]["layerInstances"]
        .as_array_mut()
        .expect("layers");
      let ents = layers
        .iter_mut()
        .find(|l| l["__identifier"] == "Entities")
        .expect("entities");
      ents["entityInstances"][0]["fieldInstances"][0]["__type"] = "Quaternion".into();
    });
    assert!(
      error.contains("entity 11111111-0000-4000-8000-00000000d00a"),
      "{error}"
    );
    assert!(
      error.contains("field \"Integer\"") && error.contains("Quaternion"),
      "{error}"
    );
    // a neighbour that is not a level
    let error = json_failure(|v| {
      v["levels"][0]["__neighbours"][0]["levelIid"] = "ghost".into();
    });
    assert!(
      error.contains("level \"Level_0\"") && error.contains("ghost"),
      "{error}"
    );
    let error = json_failure(|v| {
      v["levels"][0]["__neighbours"][0]["dir"] = "up".into();
    });
    assert!(error.contains("\"up\""), "{error}");
  }

  #[test]
  fn tilesets_that_cannot_hold_the_tiles_fail() {
    // a tile past the tileset's eight
    let error = json_failure(|v| {
      let layers = v["levels"][0]["layerInstances"]
        .as_array_mut()
        .expect("layers");
      let decor = layers
        .iter_mut()
        .find(|l| l["__identifier"] == "Decor")
        .expect("decor");
      decor["gridTiles"][0]["t"] = 40.into();
    });
    assert!(
      error.contains("layer \"Decor\"") && error.contains("tile id 40"),
      "{error}"
    );
    // an image smaller than the tileset says
    let error = json_failure(|v| {
      v["defs"]["tilesets"][0]["relPath"] = "images/props.png".into();
    });
    assert!(
      error.contains("tileset \"Tiles\"")
        && error.contains("too small")
        && error.contains("images/props.png"),
      "{error}"
    );
    // a path out of the asset root
    let error = json_failure(|v| {
      v["defs"]["tilesets"][0]["relPath"] = "../../x.png".into();
    });
    assert!(error.contains("tileset \"Tiles\""), "{error}");
    let error = json_failure(|v| {
      v["defs"]["tilesets"][0]["relPath"] = "/abs/x.png".into();
    });
    assert!(error.contains("absolute"), "{error}");
  }

  #[test]
  fn a_tileset_image_that_does_not_load_fails_the_project_naming_it_and_the_tileset() {
    let text = std::fs::read_to_string(Path::new(FIXTURES).join("internal.ldtk")).expect("fixture");
    let mut value: serde_json::Value = serde_json::from_str(&text).expect("json");
    value["defs"]["tilesets"][0]["relPath"] = "images/broken.png".into();
    let files = files_at("", &[("m.ldtk", value.to_string())]);
    files.insert("images/broken.png", b"not a png".as_slice());
    // at once: through the loader's own load of the image
    let mut world = world_over(files.clone());
    let now = load_in(&mut world, "m.ldtk").expect_err("loaded");
    // on the workers: through the server, which asks the loader where
    let mut world = world_over(files);
    let server = world.resource::<AssetServer>().clone();
    let project = server.load::<LdtkProject>("m.ldtk");
    let start = std::time::Instant::now();
    let queued = loop {
      crate::core::asset::pump(&mut world);
      if let crate::core::asset::LoadState::Failed(error) = server.state(&project) {
        break error.report();
      }
      assert!(start.elapsed().as_secs() < 5, "timed out");
      std::thread::sleep(std::time::Duration::from_millis(1));
    };
    for error in [now, queued] {
      assert!(
        error.contains("tileset \"Tiles\"") && error.contains("images/broken.png"),
        "{error}"
      );
    }
  }

  #[test]
  fn a_level_whose_layers_hold_too_many_cells_together_is_refused_before_allocating() {
    // Level_0 made 8192 x 8192 cells of 16 px, its layers empty but each
    // counted (the IntGrid twice, its tiles and its values): four layers'
    // worth of 2^26 cells
    let error = json_failure(|v| {
      let level = &mut v["levels"][0];
      level["pxWid"] = (8192 * 16).into();
      level["pxHei"] = (8192 * 16).into();
      let layers = level["layerInstances"].as_array_mut().expect("layers");
      for layer in layers.iter_mut() {
        if layer["__type"] != "Entities" {
          layer["__cWid"] = 8192.into();
          layer["__cHei"] = 8192.into();
          layer["intGridCsv"] = serde_json::json!([]);
          layer["autoLayerTiles"] = serde_json::json!([]);
          layer["gridTiles"] = serde_json::json!([]);
        }
      }
    });
    assert!(
      error.contains("level \"Level_0\"") && error.contains("past the 134217728"),
      "{error}"
    );
  }

  #[test]
  fn a_thin_level_counts_the_engine_chunks_its_layers_span_before_allocating() {
    // Level_0 made 1 x 2^26 cells of 16 px with one tile layer, empty: a
    // layer's cells, but 2^21 of the engine's 32 x 32 chunks (2^31 cells)
    // were it full, so it is refused before any grid is allocated
    let error = json_failure(|v| {
      let level = &mut v["levels"][0];
      level["pxWid"] = 16.into();
      level["pxHei"] = ((1i64 << 26) * 16).into();
      let layers = level["layerInstances"].as_array_mut().expect("layers");
      layers
        .retain(|layer| layer["__identifier"] == "Entities" || layer["__identifier"] == "Props");
      for layer in layers.iter_mut() {
        if layer["__type"] != "Entities" {
          layer["__cWid"] = 1.into();
          layer["__cHei"] = (1i64 << 26).into();
          layer["gridTiles"] = serde_json::json!([]);
        }
      }
    });
    assert!(
      error.contains("layer \"Props\"")
        && error.contains("2147483648 cells")
        && error.contains("past the 67108864 cells a layer holds"),
      "{error}"
    );
  }

  #[test]
  fn ldtk_features_drawn_otherwise_warn_once_naming_each() {
    // The door's tile drawn repeated over its box, and two layers with
    // parallax that LDtk scales
    let text = std::fs::read_to_string(Path::new(FIXTURES).join("internal.ldtk")).expect("fixture");
    let mut value: serde_json::Value = serde_json::from_str(&text).expect("json");
    let mut tiled_entities = 0;
    for def in value["defs"]["entities"].as_array_mut().expect("entities") {
      def["tileRenderMode"] = "Repeat".into();
      tiled_entities += 1;
    }
    assert!(tiled_entities > 0);
    for layer in value["defs"]["layers"].as_array_mut().expect("layers") {
      if layer["identifier"] == "Decor" || layer["identifier"] == "Props" {
        layer["parallaxFactorX"] = 0.5.into();
        layer["parallaxScaling"] = true.into();
      }
    }
    let mut world = world_over(files_at("", &[("warn-e5f6.ldtk", value.to_string())]));
    load_in(&mut world, "warn-e5f6.ldtk").expect("loads");
    let warned: Vec<String> = crate::core::dev_log::get_entries()
      .into_iter()
      .filter(|e| e.message.contains("warn-e5f6.ldtk"))
      .map(|e| e.message)
      .collect();
    let count = |what: &str| warned.iter().filter(|w| w.contains(what)).count();
    assert_eq!(count("parallax layer's scaling"), 1, "{warned:#?}");
    assert_eq!(count("\"Repeat\""), 1, "{warned:#?}");
  }

  #[test]
  fn sizes_that_would_overflow_are_refused() {
    let error = json_failure(|v| {
      v["levels"][1]["pxWid"] = 4_000_000_000i64.into();
      v["levels"][1]["pxHei"] = 4_000_000_000i64.into();
    });
    assert!(
      error.contains("level \"Level_1\"") && error.contains("cells"),
      "{error}"
    );
    let error = json_failure(|v| {
      v["levels"][1]["pxWid"] = 0.into();
    });
    assert!(error.contains("no size"), "{error}");
  }

  #[test]
  fn a_missing_external_level_names_the_file() {
    let files = files_at("", &[]);
    files.remove("external/Level_1.ldtkl");
    let mut world = world_over(files);
    let error = load_in(&mut world, "external.ldtk").expect_err("missing");
    assert!(
      error.contains("level \"Level_1\"") && error.contains("external/Level_1.ldtkl"),
      "{error}"
    );
    // and a level file that is another level
    let files = files_at("", &[]);
    let one = std::fs::read("tests/fixtures/tilemap/ldtk/external/Level_0.ldtkl").expect("file");
    files.insert("external/Level_1.ldtkl", one);
    let mut world = world_over(files);
    let error = load_in(&mut world, "external.ldtk").expect_err("wrong level");
    assert!(error.contains("not"), "{error}");
  }

  #[test]
  fn linear_levels_are_placed_as_ldtk_places_them() {
    // LDtk's `World.reorganizeWorld` puts each level after the last one's
    // width (or height) plus a spacing of 48 px, and the file writes -1
    let horizontal = load_json(|v| {
      v["worldLayout"] = "LinearHorizontal".into();
      for l in v["levels"].as_array_mut().expect("levels") {
        l["worldX"] = (-1).into();
        l["worldY"] = (-1).into();
      }
    })
    .expect("loads");
    assert_eq!(horizontal.levels[0].position, IVec2::ZERO);
    assert_eq!(horizontal.levels[1].position, IVec2::new(64 + 48, 0));
    assert_eq!(
      horizontal.worlds[0].levels[1].position,
      Vec2::new(112.0, 0.0)
    );
    let vertical = load("worlds.ldtk");
    assert_eq!(vertical.level("Hall_0").expect("0").position, IVec2::ZERO);
    assert_eq!(
      vertical.level("Hall_1").expect("1").position,
      IVec2::new(0, 48 + 48)
    );
    // a free or GridVania layout is as the file says
    assert_eq!(
      vertical.level("Cave_B").expect("B").position,
      IVec2::new(-64, 16)
    );
  }

  #[test]
  fn parallax_factors_map_to_layer_parallax() {
    // LDtk's editor shifts a layer by camera * factor (Camera.getParallaxOffsetX),
    // so on screen it follows (1 - factor) of the camera: Tiled's parallax
    let project = load_json(|v| {
      let layers = v["defs"]["layers"].as_array_mut().expect("layers");
      for l in layers.iter_mut() {
        if l["identifier"] == "Decor" {
          l["parallaxFactorX"] = 0.25.into();
          l["parallaxFactorY"] = (-0.5).into();
          l["parallaxScaling"] = false.into();
        }
        if l["identifier"] == "Props" {
          l["parallaxFactorX"] = 1.into();
        }
      }
    })
    .expect("loads");
    let level = &project.levels[0];
    assert_eq!(layer(level, "Decor").info.parallax, Vec2::new(0.75, 1.5));
    assert_eq!(layer(level, "Props").info.parallax, Vec2::new(0.0, 1.0));
    // the default is a layer fixed to the world
    assert_eq!(layer(level, "Collision").info.parallax, Vec2::ONE);
  }
}
