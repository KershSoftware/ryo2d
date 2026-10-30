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
  /// The top-left corner in world pixels
  pub position: Vec2,
  pub size: Vec2,
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
}

#[cfg(test)]
mod tests {
  use std::path::Path;

  use glam::{IVec2, UVec2, Vec2};

  use super::*;
  use crate::core::asset::{AssetServer, AssetsView, MemorySource, insert_registry};
  use crate::core::ecs::kernel::World;
  use crate::core::tilemap::{
    LayerKind, MapLayer, PropertyValue, Tile, TileFlip, TilesetImage, TilesetSource,
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
      LayerKind::Tiles { grid, .. } => grid.get(IVec2::new(x, y)),
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
          grid,
          intgrid,
          cell_size,
        } => {
          out += &format!("cell {cell_size:?}\n");
          for y in 0..grid.size().y as i32 {
            for x in 0..grid.size().x as i32 {
              out += &format!("{:?};", grid.get(IVec2::new(x, y)));
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
    assert_eq!(names, ["Decor", "Collision", "Props", "Entities"]);
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
    // auto-tiles; the last tile stacked in a cell is the one on top
    assert_eq!(cell(level, "Collision", 0, 0), Some(Tile::new(0, 5)));
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
    // `<` is a level below
    let (_, upper) = world_level(over, "Upper");
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
    // a linear layout has no positions in the file: the levels follow one another
    let hall = &project.worlds[2];
    assert_eq!(world_level(hall, "Hall_0").1.position, Vec2::ZERO);
    assert_eq!(world_level(hall, "Hall_1").1.position, Vec2::new(0.0, 48.0));
    assert_eq!(
      project.level("Hall_1").expect("hall").position,
      IVec2::new(0, 48)
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
    assert_eq!(external.levels[0].map.layers.len(), 4);
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
    let cell_size = |name: &str| match &layer(level, name).kind {
      LayerKind::Tiles { cell_size, .. } => *cell_size,
      _ => panic!("{name} is no tile layer"),
    };
    assert_eq!(cell_size("Collision"), UVec2::splat(8));
    assert_eq!(cell_size("Decor"), UVec2::splat(16));
    assert_eq!(cell_size("Props"), UVec2::splat(16));
    // the 8 px layer has twice the cells across and down
    let LayerKind::Tiles { grid, intgrid, .. } = &layer(level, "Collision").kind else {
      panic!("no tiles")
    };
    assert_eq!(grid.size(), UVec2::new(8, 6));
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
    let LayerKind::Tiles { grid, .. } = &layer(level, "Decor").kind else {
      panic!("no tiles")
    };
    assert_eq!(grid.size(), UVec2::new(4, 3));
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
      error.contains("tileset \"Tiles\"") && error.contains("too small"),
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
}
