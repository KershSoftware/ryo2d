//! Tiled's maps and tilesets as registry assets.

pub(crate) mod build;
mod data;
pub(crate) mod json;
mod objects;
mod project;
pub(crate) mod raw;
mod wang;
pub(crate) mod world_file;
mod xml;

pub use json::{TiledMapLoader, TiledTilesetLoader};
pub use world_file::TileWorldLoader;

#[cfg(test)]
mod tests {
  use std::path::Path;
  use std::time::{Duration, Instant};

  use glam::{IVec2, UVec2, Vec2};

  use crate::core::asset::{AssetServer, Assets, AssetsView, MemorySource, insert_registry, pump};
  use crate::core::ecs::kernel::World;
  use crate::core::graphics::Image;
  use crate::core::render::batch::{BlendMode, Filter};
  use crate::core::tilemap::{
    GridShape, ImageLayer, LayerInfo, LayerKind, PropertyValue, StaggerAxis, StaggerIndex, Tile,
    TileFlip, TileMap, Tileset, TilesetImage, TilesetSource,
  };
  use crate::core::ui::Color;

  const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/tilemap/tiled");

  /// Every file under `dir` at its path relative to `root`
  fn read_tree(root: &Path, dir: &Path, files: &MemorySource) {
    let Ok(entries) = std::fs::read_dir(dir) else {
      panic!("{dir:?} is not readable")
    };
    for entry in entries.flatten() {
      let path = entry.path();
      if path.is_dir() {
        if path.file_name().is_some_and(|n| n == "xml") {
          continue;
        }
        read_tree(root, &path, files);
      } else if let (Ok(rel), Ok(bytes)) = (path.strip_prefix(root), std::fs::read(&path)) {
        files.insert(&rel.to_string_lossy().replace('\\', "/"), bytes);
      }
    }
  }

  /// The fixture files plus `extra` (path, text) files
  fn files_with(extra: &[(&str, &str)]) -> MemorySource {
    let files = MemorySource::new();
    read_tree(Path::new(FIXTURES), Path::new(FIXTURES), &files);
    for (path, text) in extra {
      files.insert(path, *text);
    }
    files
  }

  fn world_over(files: MemorySource) -> World {
    let mut world = World::new();
    insert_registry(&mut world, AssetServer::new(files));
    world
  }

  /// Loads the map at `path` at once and runs its check
  fn load_in(world: &mut World, path: &str) -> Result<TileMap, String> {
    let mut assets = AssetsView::new(world);
    let handle = assets.load_now::<TileMap>(path).map_err(|e| e.report())?;
    assets
      .get(&handle)
      .cloned()
      .ok_or_else(|| "not in the store".to_string())
  }

  fn load(path: &str) -> TileMap {
    let mut world = world_over(files_with(&[]));
    match load_in(&mut world, path) {
      Ok(map) => map,
      Err(error) => panic!("{path}: {error}"),
    }
  }

  /// The map written as `json` at `m.tmj`, over the fixture files
  fn load_json(json: &str) -> Result<TileMap, String> {
    let mut world = world_over(files_with(&[("m.tmj", json)]));
    load_in(&mut world, "m.tmj")
  }

  fn failure(json: &str) -> String {
    match load_json(json) {
      Ok(_) => panic!("{json} loaded"),
      Err(error) => error,
    }
  }

  /// A one-layer 2x2 map around `layer` fields, with the `tilesets` JSON
  fn map_json(extra: &str, layer: &str, tilesets: &str) -> String {
    format!(
      r#"{{"type":"map","orientation":"orthogonal","width":2,"height":2,"tilewidth":16,"tileheight":16,{extra}
        "tilesets":[{tilesets}],"layers":[{layer}]}}"#
    )
  }

  fn tile_layer(fields: &str) -> String {
    format!(
      r#"{{"id":1,"name":"ground","type":"tilelayer","width":2,"height":2,"data":[0,0,0,0]{fields}}}"#
    )
  }

  const SET_REF: &str = r#"{"firstgid":1,"source":"tilesets/tiles.tsj"}"#;

  fn grid_of<'a>(map: &'a TileMap, name: &str) -> &'a crate::core::tilemap::TileGrid {
    let Some(index) = map.layer_index(name) else {
      panic!("no layer {name}")
    };
    match map.tiles(index) {
      Some(grid) => grid,
      None => panic!("{name} is not a tile layer"),
    }
  }

  fn cell(map: &TileMap, layer: &str, x: i32, y: i32) -> Option<Tile> {
    grid_of(map, layer).get(IVec2::new(x, y))
  }

  fn layer<'a>(map: &'a TileMap, name: &str) -> &'a crate::core::tilemap::MapLayer {
    match map.layers.iter().find(|l| l.info.name == name) {
      Some(layer) => layer,
      None => panic!("no layer {name}"),
    }
  }

  fn info<'a>(map: &'a TileMap, name: &str) -> &'a LayerInfo {
    &layer(map, name).info
  }

  #[test]
  fn tiled_tile_layers_use_the_map_s_cell_size() {
    let map = load("maps/mixed.tmj");
    let mut tile_layers = 0;
    for l in &map.layers {
      if let LayerKind::Tiles { grid, .. } = &l.kind {
        tile_layers += 1;
        assert_eq!(*grid, map.grid(), "{}", l.info.name);
        assert_eq!(grid.cell, map.tile_size, "{}", l.info.name);
      }
    }
    assert!(tile_layers > 0);
  }

  #[test]
  fn flipped_tiles_keep_their_flips() {
    let map = load("maps/mixed.tmj");
    let (h, v, d) = (TileFlip::HORIZONTAL, TileFlip::VERTICAL, TileFlip::DIAGONAL);
    assert_eq!(
      cell(&map, "flips", 0, 0),
      Some(Tile::new(0, 0).with_flip(h))
    );
    assert_eq!(
      cell(&map, "flips", 1, 0),
      Some(Tile::new(0, 0).with_flip(v))
    );
    assert_eq!(
      cell(&map, "flips", 2, 0),
      Some(Tile::new(0, 0).with_flip(d))
    );
    assert_eq!(
      cell(&map, "flips", 3, 0),
      Some(Tile::new(0, 0).with_flip(h | v | d))
    );
    // bit 28 is the hexagonal rotation, ignored here
    assert_eq!(cell(&map, "flips", 0, 2), Some(Tile::new(0, 0)));
    assert_eq!(cell(&map, "flips", 1, 1), None);
  }

  #[test]
  fn gids_split_into_id_and_flips() {
    use super::data::decode_gid;
    assert_eq!(decode_gid(7), (7, TileFlip::NONE));
    assert_eq!(decode_gid(0x8000_0007), (7, TileFlip::HORIZONTAL));
    assert_eq!(decode_gid(0x4000_0007), (7, TileFlip::VERTICAL));
    assert_eq!(decode_gid(0x2000_0007), (7, TileFlip::DIAGONAL));
    // the hexagonal 120 degree turn, which a map off a hexagonal grid drops
    assert_eq!(decode_gid(0x1000_0007), (7, TileFlip::HEX_ROTATE_120));
    assert_eq!(
      decode_gid(0xF000_0007),
      (
        7,
        TileFlip::HORIZONTAL | TileFlip::VERTICAL | TileFlip::DIAGONAL | TileFlip::HEX_ROTATE_120
      )
    );
  }

  #[test]
  fn a_layer_mixing_tilesets_keeps_each_tile_s_tileset() {
    let map = load("maps/mixed.tmj");
    assert_eq!(map.tilesets.len(), 2);
    assert_eq!(cell(&map, "ground", 0, 0), Some(Tile::new(0, 0)));
    assert_eq!(cell(&map, "ground", 3, 0), Some(Tile::new(0, 3)));
    assert_eq!(cell(&map, "ground", 0, 1), Some(Tile::new(1, 0)));
    assert_eq!(cell(&map, "ground", 1, 1), Some(Tile::new(1, 1)));
    assert_eq!(cell(&map, "ground", 2, 1), None);
    assert_eq!(cell(&map, "ground", 3, 1), Some(Tile::new(0, 4)));
    assert_eq!(map.size, UVec2::new(4, 3));
    assert_eq!(map.tile_size, UVec2::splat(16));
    assert_eq!(map.class.as_deref(), Some("level"));
    assert_eq!(map.background, Some(Color::rgb(0x10, 0x20, 0x30)));
    assert_eq!(map.properties.string("title"), Some("Mixed"));
    assert_eq!(map.properties.float("gravity"), Some(9.5));
  }

  #[test]
  fn reserved_layer_properties_fill_the_layer_info() {
    let map = load("maps/mixed.tmj");
    let ground = info(&map, "ground");
    assert_eq!(ground.class.as_deref(), Some("floor"));
    assert!(!ground.collides);
    assert_eq!(ground.occludes, Some(true));
    assert!(ground.y_sorted);
    assert!(!ground.lit);
    assert_eq!(ground.properties.bool("collides"), Some(false));
    let flips = info(&map, "flips");
    assert!(flips.collides && flips.lit && !flips.y_sorted);
    assert_eq!(flips.occludes, None);
    assert!(!info(&map, "things").collides);
    assert!(matches!(layer(&map, "things").kind, LayerKind::Objects(_)));
  }

  #[test]
  fn a_layer_s_filter_property_picks_how_its_tiles_are_sampled_in_both_formats() {
    // A tile layer's and an object layer's `filter`, as a TMJ and a TMX
    // file write it: "linear" and "nearest" (in any case), nothing, a value
    // it does not know and a bool, which keep the default as a bad reserved
    // property does
    let cases: [(Option<(&str, &str)>, Filter); 6] = [
      (Some(("string", "linear")), Filter::Linear),
      (Some(("string", "nearest")), Filter::Nearest),
      (Some(("string", "Linear")), Filter::Linear),
      (None, Filter::Default),
      (Some(("string", "smooth")), Filter::Default),
      (Some(("bool", "true")), Filter::Default),
    ];
    for (property, expected) in cases {
      let (json_props, xml_props) = match property {
        Some((ty, value)) => {
          let json_value = if ty == "string" {
            format!("\"{value}\"")
          } else {
            value.to_string()
          };
          (
            format!(r#","properties":[{{"name":"filter","type":"{ty}","value":{json_value}}}]"#),
            format!(
              r#"<properties><property name="filter" type="{ty}" value="{value}"/></properties>"#
            ),
          )
        }
        None => (String::new(), String::new()),
      };
      let json = format!(
        r#"{{"type":"map","orientation":"orthogonal","width":2,"height":2,"tilewidth":16,"tileheight":16,"tilesets":[],
          "layers":[{{"id":1,"name":"ground","type":"tilelayer","width":2,"height":2,"data":[0,0,0,0]{json_props}}},
          {{"id":2,"name":"things","type":"objectgroup","objects":[]{json_props}}}]}}"#
      );
      let xml = format!(
        r#"<map orientation="orthogonal" width="2" height="2" tilewidth="16" tileheight="16">
 <layer id="1" name="ground" width="2" height="2">{xml_props}<data encoding="csv">0,0,0,0</data></layer>
 <objectgroup id="2" name="things">{xml_props}</objectgroup>
</map>"#
      );
      let mut world = world_over(files_with(&[("m.tmj", &json), ("m.tmx", &xml)]));
      for path in ["m.tmj", "m.tmx"] {
        let map = load_in(&mut world, path).unwrap_or_else(|e| panic!("{path}: {e}"));
        for name in ["ground", "things"] {
          assert_eq!(
            info(&map, name).filter,
            expected,
            "{path}, layer {name}, filter {property:?}"
          );
        }
      }
    }
  }

  fn tileset_of(map: &TileMap, index: usize, world: &World) -> Tileset {
    match &map.tilesets[index] {
      TilesetSource::Embedded(set) => (**set).clone(),
      TilesetSource::Asset(handle) => match world.resource::<Assets<Tileset>>().get(handle) {
        Some(set) => set.clone(),
        None => panic!("tileset {index} is not in the store"),
      },
    }
  }

  #[test]
  fn every_encoding_decodes_the_same_layer() {
    let names = ["csv", "base64", "gzip", "zlib", "zstd"];
    let mut world = world_over(files_with(&[]));
    let maps: Vec<TileMap> = names
      .iter()
      .map(
        |n| match load_in(&mut world, &format!("maps/enc-{n}.tmj")) {
          Ok(map) => map,
          Err(error) => panic!("{n}: {error}"),
        },
      )
      .collect();
    let first = grid_of(&maps[0], "ground");
    assert_eq!(
      first.bounds(),
      Some(crate::core::physics::CellRect::from_size(UVec2::new(8, 4)))
    );
    for (map, name) in maps.iter().zip(names) {
      let grid = grid_of(map, "ground");
      for y in 0..4 {
        for x in 0..8 {
          let at = IVec2::new(x, y);
          assert_eq!(grid.get(at), first.get(at), "{name} at {at}");
        }
      }
    }
    let (h, v, d) = (TileFlip::HORIZONTAL, TileFlip::VERTICAL, TileFlip::DIAGONAL);
    assert_eq!(
      first.get(IVec2::new(3, 0)),
      Some(Tile::new(0, 0).with_flip(h))
    );
    assert_eq!(
      first.get(IVec2::new(4, 0)),
      Some(Tile::new(0, 1).with_flip(v))
    );
    assert_eq!(
      first.get(IVec2::new(5, 0)),
      Some(Tile::new(0, 2).with_flip(d))
    );
    assert_eq!(
      first.get(IVec2::new(6, 0)),
      Some(Tile::new(0, 3).with_flip(h | v | d))
    );
    assert_eq!(
      first.get(IVec2::new(1, 1)),
      Some(Tile::new(1, 1).with_flip(h))
    );
  }

  #[test]
  fn a_wrong_cell_count_is_an_error_naming_the_layer() {
    let layer = r#"{"id":1,"name":"walls","type":"tilelayer","width":2,"height":2,"data":[0,0,0]}"#;
    let error = failure(&map_json("", layer, ""));
    assert!(error.contains(r#"layer "walls""#), "{error}");
    assert!(error.contains('3') && error.contains('4'), "{error}");
  }

  #[test]
  fn group_layers_flatten_with_their_offset_opacity_tint_and_parallax() {
    let map = load("maps/nested.tmj");
    let names: Vec<&str> = map.layers.iter().map(|l| l.info.name.as_str()).collect();
    assert_eq!(names, ["sky", "shallow", "deep", "secret", "blended"]);
    let tint = Color::new(0xff, 0x80, 0x00, 0x80);
    let shallow = info(&map, "shallow");
    assert_eq!(shallow.offset, Vec2::new(11.0, 21.0));
    assert_eq!(shallow.opacity, 0.25);
    assert_eq!(shallow.tint, tint);
    assert_eq!(shallow.parallax, Vec2::new(0.5, 0.25));
    assert!(shallow.visible);
    let deep = info(&map, "deep");
    assert_eq!(deep.offset, Vec2::new(15.0, 26.0));
    assert_eq!(deep.opacity, 0.5);
    assert_eq!(deep.tint, tint);
    assert_eq!(deep.parallax, Vec2::new(0.25, 0.25));
    assert_eq!(cell(&map, "deep", 0, 0), Some(Tile::new(0, 3)));
    assert!(!info(&map, "secret").visible);
    let blended = info(&map, "blended");
    assert_eq!(blended.offset, Vec2::ZERO);
    assert_eq!(blended.opacity, 1.0);
    assert_eq!(blended.tint, Color::WHITE);
    assert_eq!(blended.parallax, Vec2::ONE);
    assert_eq!(blended.blend, BlendMode::Additive);
    let ids: Vec<u32> = map.layers.iter().map(|l| l.info.id).collect();
    assert_eq!(ids, [1, 3, 5, 7, 8]);
  }

  #[test]
  fn image_layers_load_their_image() {
    let mut world = world_over(files_with(&[]));
    let map = match load_in(&mut world, "maps/nested.tmj") {
      Ok(map) => map,
      Err(error) => panic!("{error}"),
    };
    let sky = layer(&map, "sky");
    assert_eq!(sky.info.offset, Vec2::new(3.0, 4.0));
    let LayerKind::Image(ImageLayer {
      image,
      repeat_x,
      repeat_y,
      ..
    }) = &sky.kind
    else {
      panic!("sky is not an image layer")
    };
    assert!(*repeat_x && !*repeat_y);
    let Some(image) = image
      .as_ref()
      .and_then(|image| world.resource::<Assets<Image>>().get(image))
    else {
      panic!("the image is not loaded")
    };
    assert_eq!((image.width(), image.height()), (32, 32));
  }

  #[test]
  fn an_image_layer_tiled_left_without_an_image_loads_empty() {
    // Saved by Tiled 1.12.2 as `"image": ""` (and `<imagelayer/>` in XML)
    let map = load("maps/image-layers.tmj");
    let LayerKind::Image(empty) = &layer(&map, "empty").kind else {
      panic!("empty is not an image layer")
    };
    assert!(empty.image.is_none());
    let LayerKind::Image(keyed) = &layer(&map, "keyed").kind else {
      panic!("keyed is not an image layer")
    };
    assert!(keyed.image.is_some());
  }

  #[test]
  fn transparent_colours_load_on_tilesets_and_image_layers() {
    let magenta = Some(Color::rgb(0xff, 0x00, 0xff));
    let mut world = world_over(files_with(&[]));
    let handle = AssetsView::new(&mut world)
      .load_now::<Tileset>("tilesets/loot.tsj")
      .expect("loads");
    let set = world
      .resource::<Assets<Tileset>>()
      .get(&handle)
      .expect("stored");
    assert_eq!(set.transparent, magenta);
    let map = load("maps/image-layers.tmj");
    let LayerKind::Image(keyed) = &layer(&map, "keyed").kind else {
      panic!("keyed is not an image layer")
    };
    assert_eq!(keyed.transparent, magenta);
    let LayerKind::Image(empty) = &layer(&map, "empty").kind else {
      panic!("empty is not an image layer")
    };
    assert_eq!(empty.transparent, None);
    let mixed = load_in(&mut world, "maps/mixed.tmj").expect("loads");
    assert_eq!(tileset_of(&mixed, 0, &world).transparent, None);
  }

  #[test]
  fn the_map_s_parallax_origin_loads() {
    let map = load("maps/image-layers.tmj");
    assert_eq!(map.parallax_origin, Vec2::new(40.0, 24.0));
    assert_eq!(info(&map, "far").parallax, Vec2::new(0.5, 0.25));
    assert_eq!(load("maps/mixed.tmj").parallax_origin, Vec2::ZERO);
  }

  #[test]
  fn layer_visible_defaults_to_true() {
    let layer =
      r#"{"id":1,"name":"ground","type":"tilelayer","width":2,"height":2,"data":[0,0,0,0]}"#;
    let map = match load_json(&map_json("", layer, "")) {
      Ok(map) => map,
      Err(error) => panic!("{error}"),
    };
    let ground = info(&map, "ground");
    assert!(ground.visible);
    assert_eq!(ground.opacity, 1.0);
    assert_eq!(ground.parallax, Vec2::ONE);
  }

  #[test]
  fn external_and_inline_tilesets_load_alike() {
    let mut world = world_over(files_with(&[]));
    let external = load_in(&mut world, "maps/mixed.tmj").expect("external");
    let inline = load_in(&mut world, "maps/mixed-embedded.tmj").expect("inline");
    assert!(matches!(external.tilesets[0], TilesetSource::Asset(_)));
    assert!(matches!(inline.tilesets[0], TilesetSource::Embedded(_)));
    for i in 0..2 {
      let (a, b) = (
        tileset_of(&external, i, &world),
        tileset_of(&inline, i, &world),
      );
      assert_eq!(a.name, b.name);
      assert_eq!(
        (a.tile_size, a.columns, a.tile_count, a.spacing, a.margin),
        (b.tile_size, b.columns, b.tile_count, b.spacing, b.margin)
      );
      for id in 0..a.tile_count {
        let (ta, tb) = (a.tile(id), b.tile(id));
        assert_eq!(ta.is_some(), tb.is_some(), "tile {id}");
        if let (Some(ta), Some(tb)) = (ta, tb) {
          assert_eq!(ta.class, tb.class);
          assert_eq!(ta.shapes, tb.shapes);
          assert_eq!(ta.animation, tb.animation);
          assert_eq!(ta.probability, tb.probability);
          assert_eq!(ta.properties, tb.properties);
          assert_eq!(
            ta.image.as_ref().map(|i| i.1),
            tb.image.as_ref().map(|i| i.1)
          );
        }
      }
    }
    let atlas = tileset_of(&inline, 0, &world);
    let TilesetImage::Atlas { size, .. } = atlas.image else {
      panic!("not an atlas")
    };
    assert_eq!(size, UVec2::splat(64));
    let items = tileset_of(&external, 1, &world);
    assert!(matches!(items.image, TilesetImage::Collection));
    assert_eq!(
      items.tile(1).and_then(|t| t.image.as_ref()).map(|i| i.1),
      Some(UVec2::splat(8))
    );
    for name in ["ground", "flips"] {
      for y in 0..3 {
        for x in 0..4 {
          assert_eq!(cell(&external, name, x, y), cell(&inline, name, x, y));
        }
      }
    }
  }

  #[test]
  fn tileset_paths_resolve_against_the_tileset_s_folder() {
    let png = std::fs::read(format!("{FIXTURES}/images/gem.png")).expect("gem.png");
    let files = MemorySource::new();
    files.insert("a/b/pic.png", png);
    files.insert(
      "a/b/set.tsj",
      r#"{"name":"s","tilewidth":16,"tileheight":16,"tilecount":1,"columns":1,
          "image":"pic.png","imagewidth":16,"imageheight":16}"#,
    );
    files.insert(
      "a/map.tmj",
      r#"{"type":"map","orientation":"orthogonal","width":1,"height":1,"tilewidth":16,"tileheight":16,
          "tilesets":[{"firstgid":1,"source":"b/set.tsj"}],
          "layers":[{"id":1,"name":"g","type":"tilelayer","width":1,"height":1,"data":[1]}]}"#,
    );
    let mut world = world_over(files);
    let map = load_in(&mut world, "a/map.tmj").expect("loads");
    let set = tileset_of(&map, 0, &world);
    let TilesetImage::Atlas { image, .. } = set.image else {
      panic!("not an atlas")
    };
    let server = world.resource::<AssetServer>();
    assert_eq!(
      server.path(&image).map(|p| p.to_string()).as_deref(),
      Some("a/b/pic.png")
    );
    // the fixture's tilesets live in tilesets/ and name ../images/
    let fixture = load("maps/mixed.tmj");
    assert_eq!(fixture.tilesets.len(), 2);
  }

  #[test]
  fn a_path_leaving_the_asset_root_fails_the_load() {
    let set = r#"{"firstgid":1,"name":"s","tilewidth":16,"tileheight":16,"tilecount":1,"columns":1,
        "image":"../escape.png","imagewidth":16,"imageheight":16}"#;
    let error = failure(&map_json("", &tile_layer(""), set));
    assert!(error.contains("escape.png"), "{error}");
  }

  #[test]
  fn a_tileset_image_too_small_for_its_tiles_fails_the_load() {
    let set = r#"{"firstgid":1,"name":"big","tilewidth":16,"tileheight":16,"tilecount":16,"columns":4,
        "image":"images/gem.png","imagewidth":64,"imageheight":64}"#;
    let error = failure(&map_json("", &tile_layer(""), set));
    assert!(error.contains("too small"), "{error}");
    assert!(error.contains("big"), "{error}");
    assert!(error.contains("images/gem.png"), "{error}");
  }

  /// The map `m.tmj` over the fixtures and `extra`, loaded on the workers
  /// (a failed dependency then reaches the map through the server) and at
  /// once (through the loader's own `cx.load`): the two errors
  fn both_failures(extra: &[(&str, &[u8])]) -> [String; 2] {
    let files = files_with(&[]);
    for (path, bytes) in extra {
      files.insert(path, *bytes);
    }
    let mut world = world_over(files.clone());
    let server = world.resource::<AssetServer>().clone();
    let map = server.load::<TileMap>("m.tmj");
    pump_until(&mut world, |_| {
      matches!(server.state(&map), crate::core::asset::LoadState::Failed(_))
    });
    let queued = match server.state(&map) {
      crate::core::asset::LoadState::Failed(error) => error.report(),
      other => panic!("{other:?}"),
    };
    let mut world = world_over(files);
    let now = load_in(&mut world, "m.tmj").expect_err("loaded");
    [queued, now]
  }

  #[test]
  fn a_tileset_image_that_does_not_load_fails_the_map_naming_it_and_the_tileset() {
    let broken: &[u8] = b"not a png";
    // an inline tileset
    let set = r#"{"firstgid":1,"name":"cracked","tilewidth":16,"tileheight":16,"tilecount":16,"columns":4,
        "image":"images/broken.png","imagewidth":64,"imageheight":64}"#;
    let map = map_json("", &tile_layer(""), set);
    for error in both_failures(&[("m.tmj", map.as_bytes()), ("images/broken.png", broken)]) {
      assert!(
        error.contains("tileset \"cracked\"") && error.contains("images/broken.png"),
        "{error}"
      );
    }
    // an external one: the map names the file, the file its image
    let set = r#"{"name":"cracked","tilewidth":16,"tileheight":16,"tilecount":16,"columns":4,
        "image":"../images/broken.png","imagewidth":64,"imageheight":64}"#;
    let map = map_json(
      "",
      &tile_layer(""),
      r#"{"firstgid":1,"source":"sets/cracked.tsj"}"#,
    );
    let files: [(&str, &[u8]); 3] = [
      ("m.tmj", map.as_bytes()),
      ("sets/cracked.tsj", set.as_bytes()),
      ("images/broken.png", broken),
    ];
    for error in both_failures(&files) {
      assert!(
        error.contains("tileset \"cracked\"")
          && error.contains("images/broken.png")
          && error.contains("sets/cracked.tsj"),
        "{error}"
      );
    }
    // an image layer's picture
    let layer = r#"{"id":1,"name":"sky","type":"imagelayer","image":"images/broken.png"}"#;
    let map = map_json("", layer, "");
    for error in both_failures(&[("m.tmj", map.as_bytes()), ("images/broken.png", broken)]) {
      assert!(
        error.contains("layer \"sky\"") && error.contains("images/broken.png"),
        "{error}"
      );
    }
  }

  #[test]
  fn a_gid_outside_every_tileset_fails_with_its_layer_and_cell() {
    let layer =
      r#"{"id":1,"name":"walls","type":"tilelayer","width":2,"height":2,"data":[0,3,0,0]}"#;
    let set = r#"{"firstgid":5,"source":"tilesets/tiles.tsj"}"#;
    let error = failure(&map_json("", layer, set));
    assert!(error.contains(r#"layer "walls""#), "{error}");
    assert!(error.contains("(1, 0)"), "{error}");
    // past the end of the last tileset: found when the tileset is loaded
    let layer =
      r#"{"id":1,"name":"walls","type":"tilelayer","width":2,"height":2,"data":[0,0,0,40]}"#;
    let error = failure(&map_json("", layer, SET_REF));
    assert!(error.contains(r#"layer "walls""#), "{error}");
    assert!(error.contains("39"), "{error}");
  }

  #[test]
  fn a_tile_id_past_the_cell_limit_fails() {
    let set = r#"{"firstgid":1,"name":"huge","tilewidth":16,"tileheight":16,"tilecount":2000000,"columns":1000,
        "image":"images/tiles.png","imagewidth":64,"imageheight":64}"#;
    let error = failure(&map_json("", &tile_layer(""), set));
    assert!(error.contains("1048575"), "{error}");
    assert!(error.contains("huge"), "{error}");
    let set = r#"{"firstgid":1,"name":"sparse","tilewidth":16,"tileheight":16,"tilecount":1,"columns":0,
        "tiles":[{"id":1048575,"image":"images/gem.png","imagewidth":16,"imageheight":16}]}"#;
    let error = failure(&map_json("", &tile_layer(""), set));
    assert!(error.contains("1048575"), "{error}");
  }

  #[test]
  fn hostile_sizes_are_errors_not_allocations() {
    let layer =
      r#"{"id":1,"name":"vast","type":"tilelayer","width":100000,"height":100000,"data":[]}"#;
    let error = failure(&map_json("", layer, ""));
    assert!(error.contains(r#"layer "vast""#), "{error}");
    let huge = r#"{"type":"map","orientation":"orthogonal","width":100000,"height":100000,"tilewidth":16,"tileheight":16,"tilesets":[],"layers":[]}"#;
    let error = failure(huge);
    assert!(error.contains("cells"), "{error}");
    let layers = r#"{"id":4294967295,"name":"a","type":"objectgroup","objects":[]},{"name":"b","type":"objectgroup","objects":[]}"#;
    let error = failure(&map_json("", layers, ""));
    assert!(error.contains("id"), "{error}");
  }

  #[test]
  fn a_map_whose_layers_hold_too_many_cells_together_is_refused_before_decoding_them() {
    // Three layers of 8192 x 8192 cells, each within a layer's limit: their
    // cells are counted before any is decoded (or allocated), so the map
    // fails on the count, not on their data
    let layers: Vec<String> = (1..=3)
      .map(|id| {
        format!(
          r#"{{"id":{id},"name":"l{id}","type":"tilelayer","width":8192,"height":8192,"data":[]}}"#
        )
      })
      .collect();
    let json = format!(
      r#"{{"type":"map","orientation":"orthogonal","width":8192,"height":8192,"tilewidth":16,"tileheight":16,"tilesets":[],"layers":[{{"id":9,"name":"g","type":"group","layers":[{}]}}]}}"#,
      layers.join(",")
    );
    let error = failure(&json);
    assert!(
      error.contains("201326592 cells") && error.contains("past the 134217728 a map holds"),
      "{error}"
    );
  }

  /// The dev log's warnings naming `file`
  fn warnings_about(file: &str) -> Vec<String> {
    crate::core::dev_log::get_entries()
      .into_iter()
      .filter(|e| e.message.contains(file))
      .map(|e| e.message)
      .collect()
  }

  #[test]
  fn editor_features_drawn_otherwise_warn_once_a_file_naming_each() {
    // A right-up render order; an inline tileset whose tiles render at the
    // grid's size, with two tiles drawing parts of their images and one
    // with a polyline collision shape; two top-down object layers whose
    // tile objects overlap out of order by their y
    let set = r#"{"firstgid":1,"name":"parts","tilewidth":16,"tileheight":16,"tilecount":3,"columns":0,
        "tilerendersize":"grid","fillmode":"preserve-aspect-fit","tiles":[
        {"id":0,"image":"images/tiles.png","imagewidth":64,"imageheight":64,"x":16,"y":0,"width":16,"height":16},
        {"id":1,"image":"images/tiles.png","imagewidth":64,"imageheight":64,"x":32,"y":0,"width":16,"height":16},
        {"id":2,"image":"images/gem.png","imagewidth":16,"imageheight":16,
         "objectgroup":{"type":"objectgroup","objects":[{"id":1,"x":0,"y":0,"polyline":[{"x":0,"y":0},{"x":16,"y":16}]}]}}]}"#;
    let objects = |id: u32| {
      format!(
        r#"{{"id":{id},"name":"o{id}","type":"objectgroup","draworder":"topdown","objects":[
          {{"id":{a},"gid":1,"x":0,"y":32,"width":16,"height":16}},
          {{"id":{b},"gid":2,"x":8,"y":24,"width":16,"height":16}}]}}"#,
        a = id * 10,
        b = id * 10 + 1
      )
    };
    let json = map_json(
      r#""renderorder":"right-up","#,
      &format!("{},{}", objects(1), objects(2)),
      set,
    );
    let mut world = world_over(files_with(&[("warn-b7e1.tmj", &json)]));
    load_in(&mut world, "warn-b7e1.tmj").expect("loads");
    let warned = warnings_about("warn-b7e1.tmj");
    let count = |what: &str| warned.iter().filter(|w| w.contains(what)).count();
    assert_eq!(count("render order \"right-up\""), 1, "{warned:#?}");
    assert_eq!(count("tile render size"), 1, "{warned:#?}");
    assert_eq!(count("part of its image"), 1, "{warned:#?}");
    assert!(
      warned
        .iter()
        .any(|w| w.contains("part of its image") && w.contains("1 place more"))
    );
    assert_eq!(count("polyline"), 1, "{warned:#?}");
    assert_eq!(count("top down"), 1, "{warned:#?}");
    assert!(
      warned
        .iter()
        .any(|w| w.contains("top down") && w.contains("layer \"o1\""))
    );
    assert_eq!(warned.len(), 5, "{warned:#?}");
    // in order, a top-down layer draws as Tiled does: no warning
    let json = map_json(
      "",
      &objects(1).replace(r#""x":0,"y":32"#, r#""x":0,"y":16"#),
      set.replace(r#""tilerendersize":"grid","#, "").as_str(),
    );
    let mut world = world_over(files_with(&[("warn-c2d4.tmj", &json)]));
    load_in(&mut world, "warn-c2d4.tmj").expect("loads");
    let warned = warnings_about("warn-c2d4.tmj");
    assert!(
      !warned.iter().any(|w| w.contains("top down")),
      "{warned:#?}"
    );
  }

  #[test]
  fn a_tileset_s_isometric_grid_warns_once_where_its_tiles_have_shapes() {
    // Measured (measured-collision.json): Tiled keeps a tile's shapes as
    // written whatever its tileset's grid, and its exporters read them in
    // the image's pixels; its collision editor draws them on the grid
    let set = |grid: &str, shapes: bool| {
      let group = if shapes {
        r#","objectgroup":{"type":"objectgroup","objects":[{"id":1,"x":0,"y":0,"polygon":[{"x":32,"y":0},{"x":64,"y":16},{"x":32,"y":32},{"x":0,"y":16}]}]}"#
      } else {
        ""
      };
      format!(
        r#"{{"firstgid":1,"name":"diamonds","tilewidth":64,"tileheight":32,"tilecount":1,"columns":0,
          "grid":{{"orientation":"{grid}","width":64,"height":32}},"tiles":[
          {{"id":0,"image":"images/gem.png","imagewidth":64,"imageheight":32{group}}}]}}"#
      )
    };
    let cases = [
      ("grid-a1c3.tmj", set("isometric", true), 1),
      ("grid-b2d4.tmj", set("isometric", false), 0),
      ("grid-c3e5.tmj", set("orthogonal", true), 0),
    ];
    for (file, set, expected) in cases {
      let json = map_json("", &tile_layer(""), &set);
      let mut world = world_over(files_with(&[(file, &json)]));
      let map = load_in(&mut world, file).expect("loads");
      let warned = warnings_about(file);
      let count = warned
        .iter()
        .filter(|w| w.contains("isometric grid is not read"))
        .count();
      assert_eq!(count, expected, "{file}: {warned:#?}");
      // The shape is read in the image's pixels
      let TilesetSource::Embedded(set) = &map.tilesets[0] else {
        panic!("not inline")
      };
      if let Some(shape) = set.tile(0).and_then(|t| t.shapes.first()) {
        assert_eq!(shape.points[1], Vec2::new(64.0, 16.0), "{file}");
      }
    }
  }

  #[test]
  fn a_capsule_among_a_tile_s_collision_shapes_collides_as_its_outline() {
    let set = r#"{"firstgid":1,"name":"pill","tilewidth":16,"tileheight":16,"tilecount":1,"columns":0,"tiles":[
        {"id":0,"image":"images/gem.png","imagewidth":16,"imageheight":16,
         "objectgroup":{"type":"objectgroup","objects":[{"id":1,"x":2,"y":4,"width":12,"height":8,"capsule":true}]}}]}"#;
    let map = load_json(&map_json("", &tile_layer(""), set)).expect("loads");
    let TilesetSource::Embedded(set) = &map.tilesets[0] else {
      panic!("not inline")
    };
    let shape = &set.tile(0).expect("tile 0").shapes[0];
    // two half circles of radius 4 on a 12 x 8 box from (2, 4)
    assert_eq!(shape.points.len(), 18);
    let (min, max) = shape.points.iter().fold(
      (Vec2::splat(f32::MAX), Vec2::splat(f32::MIN)),
      |(lo, hi), p| (lo.min(*p), hi.max(*p)),
    );
    assert!(
      (min - Vec2::new(2.0, 4.0)).abs().max_element() < 1e-4,
      "{min}"
    );
    assert!(
      (max - Vec2::new(14.0, 12.0)).abs().max_element() < 1e-4,
      "{max}"
    );
    // its corners are cut: the box's corner is outside it
    assert!(
      shape
        .points
        .iter()
        .all(|p| p.distance(Vec2::new(2.0, 4.0)) > 1.0)
    );
  }

  #[test]
  fn tile_collision_shapes_animation_class_and_probability_load() {
    let mut world = world_over(files_with(&[]));
    let map = load_in(&mut world, "maps/mixed.tmj").expect("loads");
    let set = tileset_of(&map, 0, &world);
    assert_eq!((set.columns, set.tile_count), (4, 16));
    let brick = set.tile(1).expect("tile 1");
    assert_eq!(brick.class.as_deref(), Some("brick"));
    assert_eq!(brick.probability, 0.5);
    assert_eq!(brick.properties.int("hardness"), Some(3));
    assert_eq!(brick.properties.string("label"), Some("a brick"));
    assert_eq!(brick.shapes.len(), 1);
    assert_eq!(
      brick.shapes[0].points,
      [
        Vec2::new(0.0, 8.0),
        Vec2::new(16.0, 8.0),
        Vec2::new(16.0, 16.0),
        Vec2::new(0.0, 16.0)
      ]
    );
    assert!(brick.shapes[0].one_way && !brick.shapes[0].occluder);
    let tri = set.tile(2).expect("tile 2");
    assert_eq!(
      tri.shapes[0].points,
      [
        Vec2::new(2.0, 2.0),
        Vec2::new(14.0, 2.0),
        Vec2::new(8.0, 14.0)
      ]
    );
    assert_eq!(tri.probability, 1.0);
    let disc = &set.tile(3).expect("tile 3").shapes[0];
    assert_eq!(disc.points.len(), 16);
    assert!(disc.occluder && !disc.one_way);
    for p in &disc.points {
      assert!(
        p.x >= -1e-4 && p.x <= 16.0001 && p.y >= -1e-4 && p.y <= 16.0001,
        "{p}"
      );
    }
    let xs = disc.points.iter().map(|p| p.x);
    assert!((xs.clone().fold(f32::MAX, f32::min)).abs() < 1e-4);
    assert!((xs.fold(f32::MIN, f32::max) - 16.0).abs() < 1e-4);
    let moving = set.tile(4).expect("tile 4");
    assert_eq!(moving.animation.len(), 2);
    assert_eq!(moving.animation[0].tile, 4);
    assert_eq!(moving.animation[0].duration, Duration::from_millis(100));
    assert_eq!(moving.animation[1].tile, 5);
    assert_eq!(moving.animation[1].duration, Duration::from_millis(250));
    assert!(set.tile(0).is_none());
  }

  #[test]
  fn properties_keep_their_types() {
    let props = r##"[{"name":"b","type":"bool","value":true},{"name":"i","type":"int","value":4},
      {"name":"f","type":"float","value":1.5},{"name":"c","type":"color","value":"#ff0a0b0c"},
      {"name":"p","type":"file","value":"x/y.png"},{"name":"o","type":"object","value":12},
      {"name":"e","type":"string","propertytype":"Mood","value":"happy"},
      {"name":"k","type":"class","propertytype":"Stats","value":{"hp":3,"name":"x"}}]"##;
    let map = load_json(&map_json(&format!(r#""properties":{props},"#), "", ""));
    let map = match map {
      Ok(map) => map,
      Err(error) => panic!("{error}"),
    };
    let p = &map.properties;
    assert_eq!(p.bool("b"), Some(true));
    assert_eq!(p.int("i"), Some(4));
    assert_eq!(p.float("f"), Some(1.5));
    assert_eq!(p.color("c"), Some(Color::new(0x0a, 0x0b, 0x0c, 0xff)));
    assert_eq!(p.get("p"), Some(&PropertyValue::File("x/y.png".into())));
    assert_eq!(p.get("o"), Some(&PropertyValue::Object("12".into())));
    assert_eq!(
      p.get("e"),
      Some(&PropertyValue::Enum {
        ty: "Mood".into(),
        value: "happy".into()
      })
    );
    let Some(PropertyValue::Class { ty, fields }) = p.get("k") else {
      panic!("not a class")
    };
    assert_eq!(ty, "Stats");
    assert_eq!(fields.int("hp"), Some(3));
    assert_eq!(fields.string("name"), Some("x"));
  }

  /// An empty 1 x 1 map of `orientation` with `extra` fields
  fn grid_json(orientation: &str, extra: &str) -> String {
    format!(
      r#"{{"type":"map","orientation":"{orientation}",{extra}"width":1,"height":1,"tilewidth":16,"tileheight":16,"tilesets":[],"layers":[]}}"#
    )
  }

  #[test]
  fn unknown_grid_values_and_a_negative_hex_side_fail_naming_them() {
    let refused = [
      ("hexagon", "", "\"hexagon\""),
      ("staggered", r#""staggeraxis":"z","#, "staggeraxis \"z\""),
      (
        "hexagonal",
        r#""staggerindex":"middle","#,
        "staggerindex \"middle\"",
      ),
      ("hexagonal", r#""hexsidelength":-5,"#, "hexsidelength -5"),
      ("oblique", r#""skewx":4294967296,"#, "skewx 4294967296"),
      ("oblique", r#""skewy":-4294967296,"#, "skewy -4294967296"),
    ];
    for (orientation, extra, named) in refused {
      let error = failure(&grid_json(orientation, extra));
      assert!(
        error.contains("m.tmj") && error.contains(named) && !error.contains("6b"),
        "{error}"
      );
    }
    // Fields left out take Tiled's defaults (measured): rows shifted, the
    // odd ones, a side of 0, no skew
    let shapes = [
      ("orthogonal", GridShape::Orthogonal),
      ("", GridShape::Orthogonal),
      ("isometric", GridShape::Isometric),
      (
        "staggered",
        GridShape::Staggered {
          axis: StaggerAxis::Y,
          index: StaggerIndex::Odd,
        },
      ),
      (
        "hexagonal",
        GridShape::Hexagonal {
          axis: StaggerAxis::Y,
          index: StaggerIndex::Odd,
          side: 0,
        },
      ),
      ("oblique", GridShape::Oblique { skew: IVec2::ZERO }),
    ];
    for (orientation, shape) in shapes {
      let map = load_json(&grid_json(orientation, "")).unwrap_or_else(|e| panic!("{e}"));
      assert_eq!(map.shape, shape, "{orientation}");
    }
    // a stagger written on a grid without one means nothing, as in Tiled
    let map = load_json(&grid_json("isometric", r#""staggeraxis":"x","#)).expect("loads");
    assert_eq!(map.shape, GridShape::Isometric);
  }

  /// The colours of the shape fixtures' tiles 0 to 3
  const SHAPE_COLOURS: [[u8; 4]; 4] = [
    [255, 0, 0, 255],
    [0, 255, 0, 255],
    [0, 0, 255, 255],
    [255, 255, 0, 255],
  ];

  /// Loads `renders/{name}.png`, Tiled's `toImage()` of `maps/{name}.tmj`,
  /// and checks the loaded map against it: the render is the size of the
  /// map's box on its grid, and the pixel at the centre of each cell of its
  /// "ground" layer holding a tile is that tile's colour
  fn assert_placed_as_tileds_render(name: &str) {
    assert_placed_as_tileds_render_within(name, 0.0);
  }

  /// `assert_placed_as_tileds_render` where the cells' outlines reach
  /// `below` pixels past the bottom of the tiles Tiled renders
  fn assert_placed_as_tileds_render_within(name: &str, below: f32) {
    let map = load(&format!("maps/{name}.tmj"));
    let grid = map.grid();
    let path = format!("{FIXTURES}/renders/{name}.png");
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
    let render = match image::load_from_memory(&bytes) {
      Ok(image) => image.to_rgba8(),
      Err(e) => panic!("{path}: {e}"),
    };
    let bounds = grid.bounds_of(map.bounds());
    assert_eq!(
      (render.width() as f32, render.height() as f32),
      (bounds.width, bounds.height - below),
      "{name}: the render's size"
    );
    // Tiled draws from the box's top-left (a negative skew's content is
    // left of or above the origin)
    let offset = -Vec2::new(bounds.x, bounds.y);
    let mut tiles = 0;
    for y in 0..map.size.y as i32 {
      for x in 0..map.size.x as i32 {
        let Some(tile) = cell(&map, "ground", x, y) else {
          continue;
        };
        let at = (grid.cell_center(IVec2::new(x, y)) + offset).floor();
        let pixel = render.get_pixel(at.x as u32, at.y as u32).0;
        assert_eq!(
          pixel, SHAPE_COLOURS[tile.id as usize],
          "{name}: cell ({x}, {y}), its centre at {at}"
        );
        tiles += 1;
      }
    }
    assert!(tiles >= 4, "{name}: {tiles} tiles");
  }

  #[test]
  fn odd_and_degenerate_grids_place_as_tiled_measures() {
    let hex = |axis, index, side| GridShape::Hexagonal { axis, index, side };
    let (x, y, odd, even) = (
      StaggerAxis::X,
      StaggerAxis::Y,
      StaggerIndex::Odd,
      StaggerIndex::Even,
    );
    let odd_and_degenerate = [
      // a hexagonal map of side 0, a staggered one
      ("shape-hex-x-side0", hex(x, even, 0), (32, 32)),
      ("shape-hex-y-side0", hex(y, odd, 0), (32, 32)),
      // a side longer than the tile, whose rows spread apart
      ("shape-hex-long-side", hex(y, odd, 40), (32, 32)),
      // negative skews
      (
        "shape-oblique-neg-x",
        GridShape::Oblique {
          skew: IVec2::new(-10, 0),
        },
        (32, 32),
      ),
      (
        "shape-oblique-neg-y",
        GridShape::Oblique {
          skew: IVec2::new(0, -12),
        },
        (32, 32),
      ),
      // an isometric ratio other than 2:1
      ("shape-iso-48x32", GridShape::Isometric, (48, 32)),
      // odd sizes, whose half cells fall on half pixels
      ("shape-odd-ortho", GridShape::Orthogonal, (33, 17)),
      ("shape-odd-iso", GridShape::Isometric, (33, 17)),
      (
        "shape-odd-stag-x-odd",
        GridShape::Staggered {
          axis: x,
          index: odd,
        },
        (33, 17),
      ),
      (
        "shape-odd-stag-y-even",
        GridShape::Staggered {
          axis: y,
          index: even,
        },
        (33, 17),
      ),
      ("shape-odd-hex-x-odd", hex(x, odd, 13), (33, 17)),
      ("shape-odd-hex-y-odd", hex(y, odd, 13), (33, 17)),
      (
        "shape-odd-oblique",
        GridShape::Oblique {
          skew: IVec2::new(7, -5),
        },
        (33, 17),
      ),
    ];
    for (name, shape, (w, h)) in odd_and_degenerate {
      let map = load(&format!("maps/{name}.tmj"));
      assert_eq!(map.shape, shape, "{name}");
      assert_eq!(map.tile_size, UVec2::new(w, h), "{name}");
      // A hexagon's side longer than its tile: rows step 36 pixels (the
      // slant, (32 - 40) / 2, and the side), and Tiled stands each tile on
      // a box 32 tall (the slant twice and the side), so the last row's
      // outline, the step tall, reaches 4 pixels below Tiled's render
      let below = if name == "shape-hex-long-side" {
        4.0
      } else {
        0.0
      };
      assert_placed_as_tileds_render_within(name, below);
    }
    // and the plain grids alike
    for name in [
      "shape-iso",
      "shape-stag-x-odd",
      "shape-stag-x-even",
      "shape-stag-y-odd",
      "shape-stag-y-even",
      "shape-hex-x-odd",
      "shape-hex-x-even",
      "shape-hex-y-odd",
      "shape-hex-y-even",
      "shape-oblique",
    ] {
      assert_placed_as_tileds_render(name);
    }
  }

  /// The cells and tiles of the shape fixtures' infinite maps
  const SHAPE_INFINITE_CELLS: [(i32, i32); 7] = [
    (-20, -3),
    (-1, -1),
    (0, 0),
    (5, 2),
    (40, 10),
    (-17, 18),
    (-33, -40),
  ];

  #[test]
  fn infinite_maps_of_every_grid_load_their_chunks() {
    let maps = [
      ("shape-infinite-iso", GridShape::Isometric, (64, 32)),
      (
        "shape-infinite-stag",
        GridShape::Staggered {
          axis: StaggerAxis::Y,
          index: StaggerIndex::Odd,
        },
        (64, 32),
      ),
      (
        "shape-infinite-hex",
        GridShape::Hexagonal {
          axis: StaggerAxis::Y,
          index: StaggerIndex::Odd,
          side: 12,
        },
        (32, 32),
      ),
      (
        "shape-infinite-oblique",
        GridShape::Oblique {
          skew: IVec2::new(8, 12),
        },
        (32, 32),
      ),
    ];
    for (name, shape, (w, h)) in maps {
      let map = load(&format!("maps/{name}.tmj"));
      assert!(map.infinite && map.size == UVec2::ZERO, "{name}");
      assert_eq!(map.shape, shape, "{name}");
      assert_eq!(map.tile_size, UVec2::new(w, h), "{name}");
      // an infinite map's grid has its (0, 0) at the layer's, isometric too
      assert_eq!(map.grid().origin, Vec2::ZERO, "{name}");
      for (x, y) in SHAPE_INFINITE_CELLS {
        let id = (x + 2 * y).rem_euclid(4) as u32;
        assert_eq!(
          cell(&map, "ground", x, y),
          Some(Tile::new(0, id)),
          "{name}: ({x}, {y})"
        );
      }
      let ground = grid_of(&map, "ground");
      let held: usize = ground
        .chunk_coords()
        .filter_map(|chunk| ground.chunk_cells(chunk))
        .map(|cells| cells.iter().filter(|&&c| c != 0).count())
        .sum();
      assert_eq!(held, SHAPE_INFINITE_CELLS.len(), "{name}");
      // bounds hold every tile, and nothing more
      assert_eq!(
        map.bounds(),
        crate::core::physics::CellRect::new(IVec2::new(-33, -40), IVec2::new(41, 19)),
        "{name}"
      );
    }
    // A chunk's pixels are its cells' on the map's grid: one far up and
    // right, whose cells' pixels an i32 holds as squares of 64 x 32, is past
    // it as diamonds, whose x is the column less the row in half cells
    let far = chunked_layer(1, "far", &chunk(33_000_000, -67_000_000, (16, 16), 256));
    let square = infinite_json(&far).replace(
      r#""tilewidth":16,"tileheight":16"#,
      r#""tilewidth":64,"tileheight":32"#,
    );
    load_json(&square).unwrap_or_else(|e| panic!("{e}"));
    let diamond = square.replace(
      r#""orientation":"orthogonal""#,
      r#""orientation":"isometric""#,
    );
    let error = failure(&diamond);
    assert!(
      error.contains(r#"layer "far""#) && error.contains("pixels"),
      "{error}"
    );
  }

  #[test]
  fn hex_rotation_flags_survive_loading_and_are_dropped_off_hex_maps() {
    let map = load("maps/shape-hex-rotations.tmj");
    assert_eq!(
      map.shape,
      GridShape::Hexagonal {
        axis: StaggerAxis::Y,
        index: StaggerIndex::Odd,
        side: 32
      }
    );
    // Cell (1 + i % 4, 1 + i / 4) holds the marker with flags i: 1 flips it
    // horizontally (0x8...), 2 vertically (0x4...), 4 turns it 60 degrees
    // (0x2...) and 8 120 (0x1...)
    for i in 0..16 {
      let mut flip = TileFlip::NONE;
      for (bit, f) in [
        (1, TileFlip::HORIZONTAL),
        (2, TileFlip::VERTICAL),
        (4, TileFlip::HEX_ROTATE_60),
        (8, TileFlip::HEX_ROTATE_120),
      ] {
        if i & bit != 0 {
          flip = flip | f;
        }
      }
      assert_eq!(
        cell(&map, "markers", 1 + i % 4, 1 + i / 4),
        Some(Tile::new(0, 0).with_flip(flip)),
        "flags {i}"
      );
    }
    // Off hexagonal maps Tiled ignores the 120 degree bit, and so does the
    // load, in layers and tile objects alike
    let (h, v, d) = (TileFlip::HORIZONTAL, TileFlip::VERTICAL, TileFlip::DIAGONAL);
    let mixed = load("maps/mixed.tmj");
    assert_eq!(cell(&mixed, "flips", 0, 2), Some(Tile::new(0, 0)));
    let layers = format!(
      "{},{}",
      tile_layer("").replace("[0,0,0,0]", "[268435457,4026531841,0,0]"),
      r#"{"id":2,"name":"things","type":"objectgroup","objects":[{"id":1,"gid":268435457,"x":0,"y":16,"width":16,"height":16}]}"#
    );
    for (orientation, kept) in [
      ("orthogonal", false),
      ("isometric", false),
      ("staggered", false),
      ("oblique", false),
      ("hexagonal", true),
    ] {
      let json = map_json("", &layers, SET_REF).replace(
        r#""orientation":"orthogonal""#,
        &format!(r#""orientation":"{orientation}""#),
      );
      let map = load_json(&json).unwrap_or_else(|e| panic!("{orientation}: {e}"));
      let r120 = if kept {
        TileFlip::HEX_ROTATE_120
      } else {
        TileFlip::NONE
      };
      assert_eq!(
        cell(&map, "ground", 0, 0),
        Some(Tile::new(0, 0).with_flip(r120)),
        "{orientation}"
      );
      assert_eq!(
        cell(&map, "ground", 1, 0),
        Some(Tile::new(0, 0).with_flip(h | v | d | r120)),
        "{orientation}"
      );
      let LayerKind::Objects(objects) = &layer(&map, "things").kind else {
        panic!("not objects")
      };
      assert_eq!(
        objects[0].tile,
        Some(Tile::new(0, 0).with_flip(r120)),
        "{orientation}"
      );
    }
  }

  /// The map at `path` written as `text`, over the fixture files
  fn load_text(path: &str, text: &str) -> Result<TileMap, String> {
    let mut world = world_over(files_with(&[(path, text)]));
    load_in(&mut world, path)
  }

  #[test]
  fn a_hex_layer_that_collides_loads() {
    // A hexagonal map's layer that says it collides loads colliding, and
    // one that says nothing of it collides and casts shadows, as on any
    // grid
    let map = load_in(
      &mut world_over(files_with(&[])),
      "maps/shape-hex-collides.tmj",
    )
    .unwrap_or_else(|e| panic!("{e}"));
    assert!(matches!(map.shape, GridShape::Hexagonal { .. }));
    for name in ["walls", "ground"] {
      let info = info(&map, name);
      assert!(
        info.collides && info.occludes.unwrap_or(info.collides),
        "{name}"
      );
    }
    let path = format!("{FIXTURES}/maps/shape-hex-collides.tmj");
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
    assert_eq!(text.matches(r#""name":"collides""#).count(), 1);
    let at = "maps/hex-walls.tmj";
    // One that casts shadows loads too
    let occludes = text.replace(r#""name":"collides""#, r#""name":"occludes""#);
    let map = load_text(at, &occludes).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(info(&map, "walls").occludes, Some(true));
    // and one that says it does neither does neither
    let neither = text.replace(r#""value":true"#, r#""value":false"#);
    let map = load_text(at, &neither).unwrap_or_else(|e| panic!("{e}"));
    assert!(!info(&map, "walls").collides);
    // The maps whose tiles have collision shapes load, their layer colliding
    for name in ["collide-hex-y-odd", "collide-hex-x-even"] {
      let map = load_in(
        &mut world_over(files_with(&[])),
        &format!("maps/{name}.tmj"),
      )
      .unwrap_or_else(|e| panic!("{name}: {e}"));
      assert!(info(&map, "walls").collides, "{name}");
    }
    // A tile layer added to a hexagonal map in code collides, as on any
    // other grid
    let mut hex = TileMap::new(UVec2::splat(4), UVec2::splat(32)).with_shape(map.shape);
    let walls = hex.add_tile_layer("walls");
    assert!(hex.layers[walls].info.collides);
    let mut iso =
      TileMap::new(UVec2::splat(4), UVec2::new(64, 32)).with_shape(GridShape::Isometric);
    let walls = iso.add_tile_layer("walls");
    assert!(iso.layers[walls].info.collides);
  }

  #[test]
  fn render_order_warns_only_on_orthogonal_and_oblique_maps() {
    // Tiled draws orthogonal and oblique maps in their render order and
    // every other grid right-down whatever it says (measured)
    for (orientation, warns) in [
      ("orthogonal", true),
      ("oblique", true),
      ("isometric", false),
      ("staggered", false),
      ("hexagonal", false),
    ] {
      let file = format!("order-{orientation}-5c1e.tmj");
      let json = grid_json(orientation, r#""renderorder":"left-up","#);
      let mut world = world_over(files_with(&[(&file, &json)]));
      load_in(&mut world, &file).unwrap_or_else(|e| panic!("{e}"));
      let warned = warnings_about(&file);
      let count = warned
        .iter()
        .filter(|w| w.contains("render order \"left-up\""))
        .count();
      assert_eq!(count, usize::from(warns), "{orientation}: {warned:#?}");
    }
  }

  /// An infinite map of 16 px tiles around `layers`
  fn infinite_json(layers: &str) -> String {
    format!(
      r#"{{"type":"map","orientation":"orthogonal","infinite":true,"width":0,"height":0,"tilewidth":16,"tileheight":16,"tilesets":[],"layers":[{layers}]}}"#
    )
  }

  /// An infinite tile layer called `name` of `chunks` (JSON objects)
  fn chunked_layer(id: u32, name: &str, chunks: &str) -> String {
    format!(r#"{{"id":{id},"name":"{name}","type":"tilelayer","chunks":[{chunks}]}}"#)
  }

  /// A chunk's JSON: `cells` empty gids at (`x`, `y`), `width` x `height`
  fn chunk(x: i64, y: i64, (width, height): (i64, i64), cells: usize) -> String {
    let data = vec!["0"; cells].join(",");
    format!(r#"{{"x":{x},"y":{y},"width":{width},"height":{height},"data":[{data}]}}"#)
  }

  #[test]
  fn hostile_infinite_chunks_are_refused_before_allocating() {
    let refused = |name: &str, chunk_json: String, at: &str, what: &str| {
      let error = failure(&infinite_json(&chunked_layer(1, name, &chunk_json)));
      assert!(
        error.contains("m.tmj")
          && error.contains(&format!("layer {name:?}"))
          && error.contains(&format!("chunk {at}"))
          && error.contains(what),
        "{name}: {error}"
      );
    };
    // A chunk whose cells' pixels pass what an i32 holds
    refused(
      "far",
      chunk(2_147_483_600, 0, (16, 16), 256),
      "(2147483600, 0)",
      "pixels",
    );
    // and one past the cells an i32 holds, whatever the tile size
    refused(
      "beyond",
      chunk(0, -4_294_967_296, (16, 16), 256),
      "(0, -4294967296)",
      "pixels",
    );
    // Data shorter than the chunk's cells
    refused(
      "short",
      chunk(0, 16, (16, 16), 255),
      "(0, 16)",
      "has 255 cells, expected 256",
    );
    // A chunk with no width, or a negative one
    refused("thin", chunk(0, 0, (0, 16), 0), "(0, 0)", "0 x 16");
    refused(
      "inside out",
      chunk(16, 0, (-16, 16), 0),
      "(16, 0)",
      "-16 x 16",
    );
    // 2^20 chunks of 16 x 16 (one layer's limit, 2^26 cells, in each of
    // four layers) are 2^28 cells to read, past the 2^27 a map holds:
    // counted from the chunks' sizes before any is decoded, so a 25 MB file
    // asking for a GiB of cells fails on the count
    let chunks = vec![r#"{"x":0,"y":0,"width":16,"height":16}"#; 1 << 18].join(",");
    let layers: Vec<String> = (1..=4)
      .map(|id| chunked_layer(id, &format!("l{id}"), &chunks))
      .collect();
    let error = failure(&infinite_json(&layers.join(",")));
    assert!(
      error.contains("m.tmj")
        && error.contains("layers hold 268435456 cells, past the 134217728 a map holds"),
      "{error}"
    );
    // one layer holding more than a layer holds names it
    let chunks = vec![r#"{"x":0,"y":0,"width":16,"height":16}"#; (1 << 18) + 1].join(",");
    let error = failure(&infinite_json(&chunked_layer(1, "vast", &chunks)));
    assert!(
      error.contains(r#"layer "vast""#) && error.contains("past the 67108864 cells a layer holds"),
      "{error}"
    );
  }

  #[test]
  fn a_chunk_that_does_not_state_its_place_or_size_fails_naming_it() {
    // Tiled writes all four; one left out is a broken file, not a chunk at
    // 0 or of no size. The error names the layer, the chunk (by its place,
    // or by its order in the layer when its place is what is missing) and
    // what it lacks
    let data = ["0"; 4].join(",");
    let cases = [
      (r#""y":0,"width":2,"height":2"#, "chunk 1", "`x`"),
      (r#""x":0,"width":2,"height":2"#, "chunk 1", "`y`"),
      (r#""x":-4,"y":2,"height":2"#, "chunk (-4, 2)", "`width`"),
      (r#""x":-4,"y":2,"width":2"#, "chunk (-4, 2)", "`height`"),
    ];
    for (fields, at, what) in cases {
      let chunks = format!(
        r#"{{"x":0,"y":0,"width":2,"height":2,"data":[{data}]}},{{{fields},"data":[{data}]}}"#
      );
      let error = failure(&infinite_json(&chunked_layer(1, "ground", &chunks)));
      assert!(
        error.contains("m.tmj")
          && error.contains(r#"layer "ground""#)
          && error.contains(at)
          && error.contains(what)
          && error.contains("does not state"),
        "{fields}: {error}"
      );
    }
  }

  /// `count` chunks of `size` cells, each full of tile 1, the `i`th at
  /// `at(i)`
  fn tiled_chunks(count: usize, size: i64, at: impl Fn(usize) -> (i64, i64)) -> String {
    let data = vec!["1"; (size * size) as usize].join(",");
    (0..count)
      .map(|i| {
        let (x, y) = at(i);
        format!(r#"{{"x":{x},"y":{y},"width":{size},"height":{size},"data":[{data}]}}"#)
      })
      .collect::<Vec<_>>()
      .join(",")
  }

  #[test]
  fn infinite_chunks_count_the_engine_chunks_they_take_before_allocating() {
    // The engine keeps a layer's cells in chunks of 32 x 32, each made
    // whole where a tile goes: a 1 x 1 chunk takes 1,024 cells, and a
    // 16 x 16 chunk at (24, 24) reaches four of them. So a layer counts
    // the cells of the engine's chunks its chunks reach (when more than
    // the cells they hold), checked before anything is decoded or
    // allocated.
    let set = r#""tilesets":[{"firstgid":1,"source":"tilesets/tiles.tsj"}]"#;
    let with_set = |layers: &str| infinite_json(layers).replace(r#""tilesets":[]"#, set);
    // One layer of 65,537 one-cell chunks 32 cells apart: 65,537 cells
    // read, far under a layer's 2^26, but 65,537 engine chunks, past it
    let scattered = tiled_chunks(65_537, 1, |i| (32 * i as i64, 0));
    let error = failure(&with_set(&chunked_layer(1, "dust", &scattered)));
    assert!(
      error.contains(r#"layer "dust""#)
        && error.contains("67109888 cells")
        && error.contains("past the 67108864 cells a layer holds"),
      "{error}"
    );
    // A map of one layer at the limit (65,536 one-cell chunks), one of
    // 16,384 chunks of 16 x 16 each reaching four engine chunks (65,536,
    // at the limit too), and one more cell: 131,073 engine chunks, past
    // the 2^27 cells a map holds, from 4,259,841 cells read
    let dust = tiled_chunks(65_536, 1, |i| (32 * i as i64, 0));
    let straddling = tiled_chunks(16_384, 16, |i| (24 + 64 * i as i64, 24));
    let last = tiled_chunks(1, 1, |_| (0, -100));
    let layers = [
      chunked_layer(1, "dust", &dust),
      chunked_layer(2, "straddling", &straddling),
      chunked_layer(3, "last", &last),
    ];
    let error = failure(&with_set(&layers.join(",")));
    assert!(
      error.contains("m.tmj")
        && error.contains("layers hold 134218752 cells, past the 134217728 a map holds"),
      "{error}"
    );
  }

  /// `cells` empty cells as base64 of zlib, packed as they are written so
  /// the test holds none of them at once
  fn packed_zeros(cells: usize) -> String {
    use base64::Engine;
    use std::io::Write;
    let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::fast());
    let block = [0u8; 1 << 16];
    let mut left = cells * 4;
    while left > 0 {
      let n = left.min(block.len());
      z.write_all(&block[..n]).expect("write");
      left -= n;
    }
    base64::engine::general_purpose::STANDARD.encode(z.finish().expect("finish"))
  }

  /// A bounded map of `width` x `height` cells of 16 px whose layers are
  /// `count` empty tile layers of its size, in zlib
  fn empty_layers_json(width: u32, height: u32, count: u32) -> String {
    let data = packed_zeros(width as usize * height as usize);
    let layers: Vec<String> = (1..=count)
      .map(|id| {
        format!(
          r#"{{"id":{id},"name":"l{id}","type":"tilelayer","width":{width},"height":{height},"encoding":"base64","compression":"zlib","data":"{data}"}}"#
        )
      })
      .collect();
    format!(
      r#"{{"type":"map","orientation":"orthogonal","width":{width},"height":{height},"tilewidth":16,"tileheight":16,"tilesets":[],"layers":[{}]}}"#,
      layers.join(",")
    )
  }

  #[test]
  fn a_thin_bounded_layer_counts_the_engine_chunks_it_spans_before_allocating() {
    // 1 x 2^26 cells, a layer's cells, but 2^21 of the engine's 32 x 32
    // chunks: 2^31 cells (8 GiB) were it full. A few hundred KB of zlib,
    // refused on its count before any is decoded
    let error = failure(&empty_layers_json(1, 1 << 26, 1));
    assert!(
      error.contains(r#"layer "l1""#)
        && error.contains("2147483648 cells")
        && error.contains("past the 67108864 cells a layer holds"),
      "{error}"
    );
    // Three layers of 1 x 1,500,000: each 46,875 chunks, under a layer's
    // cap; together 144,000,000 cells, past a map's, from 4,500,000 cells
    let error = failure(&empty_layers_json(1, 1_500_000, 3));
    assert!(
      error.contains("layers hold 144000000 cells, past the 134217728 a map holds"),
      "{error}"
    );
  }

  #[test]
  fn chunks_are_an_infinite_map_s_and_its_empty_layers_load_empty() {
    // A bounded map's layer of chunks, and an infinite map's of data
    let chunked = chunked_layer(1, "ground", &chunk(0, 0, (2, 2), 4));
    let error = failure(&map_json("", &chunked, ""));
    assert!(
      error.contains(r#"layer "ground""#) && error.contains("only an infinite map"),
      "{error}"
    );
    let error = failure(&infinite_json(&tile_layer("")));
    assert!(
      error.contains(r#"layer "ground""#) && error.contains("in chunks"),
      "{error}"
    );
    // A layer of no chunks, or of empty data, holds nothing anywhere; and
    // a map of nothing has no bounds
    let empty = format!(
      "{},{}",
      chunked_layer(1, "none", ""),
      r#"{"id":2,"name":"blank","type":"tilelayer","data":[]}"#
    );
    let map = load_json(&infinite_json(&empty)).expect("loads");
    for name in ["none", "blank"] {
      let grid = grid_of(&map, name);
      assert_eq!(grid.bounds(), None, "{name}");
      assert_eq!(grid.chunk_coords().count(), 0, "{name}");
    }
    assert!(map.bounds().is_empty());
    // A chunk's cells land at its place, whatever its size; its gids are
    // matched to tilesets, flips kept
    let set = r#"{"firstgid":1,"source":"tilesets/tiles.tsj"},{"firstgid":17,"source":"tilesets/items.tsj"}"#;
    let layer = chunked_layer(
      1,
      "ground",
      r#"{"x":-3,"y":-1,"width":3,"height":2,"data":[0,2,0,0,0,2147483666]}"#,
    );
    let json = infinite_json(&layer).replace(r#""tilesets":[]"#, &format!(r#""tilesets":[{set}]"#));
    let map = load_json(&json).expect("loads");
    assert_eq!(cell(&map, "ground", -2, -1), Some(Tile::new(0, 1)));
    assert_eq!(
      cell(&map, "ground", -1, 0),
      Some(Tile::new(1, 1).with_flip(TileFlip::HORIZONTAL))
    );
    assert_eq!(cell(&map, "ground", -3, 0), None);
    // its tiles in two of the engine's chunks of 32, the bounds round them
    // exactly
    assert_eq!(
      grid_of(&map, "ground").chunk_coords().collect::<Vec<_>>(),
      [IVec2::splat(-1), IVec2::new(-1, 0)]
    );
    assert_eq!(
      map.bounds(),
      crate::core::physics::CellRect::new(IVec2::new(-2, -1), IVec2::new(0, 1))
    );
    // a gid no tileset holds (the map has none) names its cell in the chunk
    let layer = chunked_layer(
      1,
      "ground",
      r#"{"x":-3,"y":-1,"width":1,"height":1,"data":[99]}"#,
    );
    let error = failure(&infinite_json(&layer));
    assert!(
      error.contains("chunk (-3, -1)") && error.contains("cell (-3, -1)") && error.contains("99"),
      "{error}"
    );
  }

  #[test]
  fn a_file_s_chunk_a_million_cells_out_loads_and_lands_holding_only_its_chunks() {
    use crate::core::tilemap::test_util::{self, MapWorld, layers_of};
    use crate::core::tilemap::{MapRoot, TileGrid, TileLayerData};
    // Tiles at (1, 1) and (1,000,001, -999,999): the rectangle between
    // them holds about 976 million of the engine's chunks, and nothing may
    // be sized from it, loading or landing
    let mut cells = vec!["0"; 256];
    cells[17] = "1";
    let data = cells.join(",");
    let layer = chunked_layer(
      1,
      "ground",
      &format!(
        r#"{{"x":0,"y":0,"width":16,"height":16,"data":[{data}]}},{{"x":1000000,"y":-1000000,"width":16,"height":16,"data":[{data}]}}"#
      ),
    );
    let json = infinite_json(&layer).replace(
      r#""tilesets":[]"#,
      r#""tilesets":[{"firstgid":1,"source":"../tilesets/tiles.tsj"}]"#,
    );
    let started = Instant::now();
    let mut maps = MapWorld::over(test_util::files_with(&[("tiled/maps/far.tmj", &json)]));
    let handle = test_util::load_map(&mut maps.world, "tiled/maps/far.tmj");
    let far = IVec2::new(1_000_001, -999_999);
    let far_chunk = TileGrid::chunk_of(far);
    let loaded = match maps.world.resource::<Assets<TileMap>>().get(&handle) {
      Some(map) => map.clone(),
      None => panic!("not in the store"),
    };
    let grid = grid_of(&loaded, "ground");
    assert_eq!(
      grid.chunk_coords().collect::<Vec<_>>(),
      [far_chunk, IVec2::ZERO]
    );
    assert_eq!(grid.get(far), Some(Tile::new(0, 0)));
    let map = maps.spawn(MapRoot::new(handle));
    maps.frame();
    assert!(
      started.elapsed() < Duration::from_secs(5),
      "loaded and landed in {:?}",
      started.elapsed()
    );
    let layer = layers_of(&mut maps.world, map)[0];
    let chunks: Vec<IVec2> = maps
      .world
      .get::<TileLayerData>(layer)
      .map(|data| data.tiles.chunk_coords().collect())
      .unwrap_or_default();
    assert_eq!(chunks, [far_chunk, IVec2::ZERO]);
  }

  #[test]
  fn an_unsupported_blend_mode_fails_naming_it() {
    let error = failure(&map_json("", &tile_layer(r#","mode":"overlay""#), ""));
    assert!(error.contains("overlay"), "{error}");
    assert!(error.contains(r#"layer "ground""#), "{error}");
  }

  #[test]
  fn normal_add_multiply_and_screen_map_to_blend_modes() {
    for (mode, expected) in [
      ("normal", BlendMode::Alpha),
      ("add", BlendMode::Additive),
      ("multiply", BlendMode::Multiply),
      ("screen", BlendMode::Screen),
    ] {
      let json = map_json("", &tile_layer(&format!(r#","mode":"{mode}""#)), "");
      let map = match load_json(&json) {
        Ok(map) => map,
        Err(error) => panic!("{mode}: {error}"),
      };
      assert_eq!(info(&map, "ground").blend, expected, "{mode}");
    }
  }

  fn pump_until(world: &mut World, done: impl Fn(&World) -> bool) {
    let start = Instant::now();
    loop {
      pump(world);
      if done(world) {
        return;
      }
      assert!(
        start.elapsed() < Duration::from_secs(5),
        "timed out waiting"
      );
      std::thread::sleep(Duration::from_millis(1));
    }
  }

  #[test]
  fn a_map_is_ready_only_when_its_tilesets_and_images_are() {
    let mut world = world_over(files_with(&[]));
    let server = world.resource::<AssetServer>().clone();
    server.limit_workers(1);
    let map = server.load::<TileMap>("maps/nested.tmj");
    assert!(!server.is_ready(&map));
    pump_until(&mut world, |_| server.is_ready(&map));
    let Some(loaded) = world.resource::<Assets<TileMap>>().get(&map) else {
      panic!("the map is ready but not stored")
    };
    let LayerKind::Image(sky) = &layer(loaded, "sky").kind else {
      panic!("no sky")
    };
    assert!(
      sky
        .image
        .as_ref()
        .is_some_and(|image| server.is_ready(image))
    );
    let TilesetSource::Asset(set) = &loaded.tilesets[0] else {
      panic!("not external")
    };
    assert!(server.is_ready(set));
    let tileset = world
      .resource::<Assets<Tileset>>()
      .get(set)
      .expect("tileset");
    let TilesetImage::Atlas { image, .. } = &tileset.image else {
      panic!("not an atlas")
    };
    assert!(server.is_ready(image));
  }

  #[test]
  fn an_external_tileset_image_too_small_fails_the_map() {
    let set = r#"{"name":"cramped","tilewidth":16,"tileheight":16,"tilecount":16,"columns":4,
        "image":"../images/gem.png","imagewidth":64,"imageheight":64}"#;
    let files = files_with(&[("sets/cramped.tsj", set)]);
    let map = map_json(
      "",
      &tile_layer(""),
      r#"{"firstgid":1,"source":"sets/cramped.tsj"}"#,
    );
    files.insert("m.tmj", map);
    let mut world = world_over(files);
    let error = load_in(&mut world, "m.tmj").expect_err("loaded");
    assert!(
      error.contains("too small") && error.contains("cramped") && error.contains("images/gem.png"),
      "{error}"
    );
  }

  #[test]
  fn a_layer_without_an_id_gets_the_next_free_one() {
    let layers = r#"{"id":7,"name":"a","type":"objectgroup","objects":[]},{"name":"b","type":"objectgroup","objects":[]}"#;
    let map = load_json(&map_json("", layers, "")).expect("loads");
    let ids: Vec<u32> = map.layers.iter().map(|l| l.info.id).collect();
    assert_eq!(ids, [7, 8]);
  }

  #[test]
  fn broken_json_names_its_line_and_column() {
    let error = failure("{\n \"width\": }");
    assert!(error.contains("line 2, column"), "{error}");
  }

  #[test]
  fn a_tile_s_sort_offset_property_loads_in_pixels() {
    let inline = r#"{"firstgid":1,"name":"o","tilewidth":16,"tileheight":16,"tilecount":16,"columns":4,
        "image":"images/tiles.png","imagewidth":64,"imageheight":64,
        "tiles":[
          {"id":1,"properties":[{"name":"sort_offset","type":"float","value":-12.5}]},
          {"id":2,"properties":[{"name":"sort_offset","type":"int","value":16}]},
          {"id":3,"properties":[{"name":"sort_offset","type":"string","value":"far"}]},
          {"id":4,"properties":[{"name":"hits","type":"int","value":1}]}]}"#;
    let map = load_json(&map_json("", &tile_layer(""), inline)).expect("loads");
    let TilesetSource::Embedded(set) = &map.tilesets[0] else {
      panic!("not inline")
    };
    let offset = |id| set.tile(id).map(|tile| tile.sort_offset);
    assert_eq!(offset(1), Some(-12.5));
    assert_eq!(offset(2), Some(16.0));
    // Not a number, or not given: the tile sorts where it stands
    assert_eq!(offset(3), Some(0.0));
    assert_eq!(offset(4), Some(0.0));
  }

  #[test]
  fn a_tileset_s_tile_offset_loads() {
    let mut world = world_over(files_with(&[]));
    let handle = AssetsView::new(&mut world)
      .load_now::<Tileset>("tilesets/offset.tsj")
      .expect("loads");
    let set = world
      .resource::<Assets<Tileset>>()
      .get(&handle)
      .expect("stored");
    assert_eq!(set.tile_offset, IVec2::new(3, -5));
    let inline = r#"{"firstgid":1,"name":"o","tilewidth":16,"tileheight":16,"tilecount":16,"columns":4,
        "tileoffset":{"x":-2,"y":7},"image":"images/tiles.png","imagewidth":64,"imageheight":64}"#;
    let map = load_json(&map_json("", &tile_layer(""), inline)).expect("loads");
    let TilesetSource::Embedded(set) = &map.tilesets[0] else {
      panic!("not inline")
    };
    assert_eq!(set.tile_offset, IVec2::new(-2, 7));
    let mut other = world_over(files_with(&[]));
    let plain = load_in(&mut other, "maps/mixed.tmj").expect("loads");
    assert_eq!(tileset_of(&plain, 0, &other).tile_offset, IVec2::ZERO);
  }

  #[test]
  fn a_zero_dimension_with_a_huge_one_is_an_error_not_an_overflow() {
    let layer =
      r#"{"id":1,"name":"thin","type":"tilelayer","width":4294967295,"height":0,"data":[]}"#;
    let error = failure(&map_json("", layer, ""));
    assert!(error.contains(r#"layer "thin""#), "{error}");
    for (w, h) in [(4294967295u64, 0u64), (0, 4294967295)] {
      let json = format!(
        r#"{{"type":"map","orientation":"orthogonal","width":{w},"height":{h},"tilewidth":16,"tileheight":16,"tilesets":[],"layers":[]}}"#
      );
      let error = failure(&json);
      assert!(error.contains("cells"), "{error}");
    }
  }

  // ---- objects, templates, Wang sets and the project ----------------------

  use std::f32::consts::FRAC_PI_2;

  use crate::core::tilemap::{
    MapObjectData, ObjectShape, Properties, TileTransformations, TiledMapLoader,
    TiledTilesetLoader, WangKind, WangSet, WangTile,
  };

  fn objects<'a>(map: &'a TileMap, name: &str) -> &'a [MapObjectData] {
    match &layer(map, name).kind {
      LayerKind::Objects(objects) => objects,
      _ => panic!("{name} is not an object layer"),
    }
  }

  fn object<'a>(map: &'a TileMap, id: &str) -> &'a MapObjectData {
    match objects(map, "things").iter().find(|o| o.id == id) {
      Some(o) => o,
      None => panic!("no object {id}"),
    }
  }

  fn enum_of(ty: &str, value: &str) -> PropertyValue {
    PropertyValue::Enum {
      ty: ty.into(),
      value: value.into(),
    }
  }

  /// The fixture map with its project, unless `project` says otherwise
  fn objects_map() -> TileMap {
    load("maps/objects.tmj")
  }

  /// Loads `json` as `m.tmj` over the fixtures and a `project.tiled-project`
  /// holding `project`
  fn load_with_project(json: &str, project: &str) -> Result<TileMap, String> {
    let mut world = world_over(files_with(&[
      ("m.tmj", json),
      ("project.tiled-project", project),
    ]));
    load_in(&mut world, "m.tmj")
  }

  fn objects_json(objects: &str) -> String {
    map_json(
      "",
      &format!(r#"{{"id":1,"name":"things","type":"objectgroup","objects":[{objects}]}}"#),
      SET_REF,
    )
  }

  #[test]
  fn objects_of_every_shape_load_with_class_and_typed_properties() {
    let map = objects_map();
    assert_eq!(objects(&map, "things").len(), 17);

    let crate_ = object(&map, "1");
    assert_eq!(crate_.shape, ObjectShape::Rect);
    assert_eq!(crate_.name, "crate");
    assert_eq!(crate_.class.as_deref(), Some("Crate"));
    assert_eq!(crate_.position, Vec2::new(16.0, 32.0));
    assert_eq!(crate_.size, Vec2::new(32.0, 16.0));
    assert_eq!(crate_.rotation, 0.0);
    assert!(crate_.visible && crate_.tile.is_none());
    assert_eq!(crate_.properties.int("hp"), Some(25));
    assert_eq!(
      crate_.properties.get("target"),
      Some(&PropertyValue::Object("3".into()))
    );

    assert_eq!(object(&map, "2").shape, ObjectShape::Ellipse);
    assert_eq!(object(&map, "2").size, Vec2::new(20.0, 10.0));
    assert_eq!(object(&map, "3").shape, ObjectShape::Point);
    assert_eq!(object(&map, "3").position, Vec2::new(5.0, 6.0));
    assert_eq!(
      object(&map, "4").shape,
      ObjectShape::Polygon(vec![
        Vec2::new(0.0, 0.0),
        Vec2::new(16.0, 0.0),
        Vec2::new(8.0, 12.0)
      ])
    );
    assert_eq!(object(&map, "4").position, Vec2::new(40.0, 60.0));
    assert_eq!(
      object(&map, "5").shape,
      ObjectShape::Polyline(vec![
        Vec2::new(0.0, 0.0),
        Vec2::new(10.0, 5.0),
        Vec2::new(20.0, 0.0)
      ])
    );
    assert_eq!(object(&map, "6").shape, ObjectShape::Capsule);
    assert_eq!(object(&map, "6").size, Vec2::new(12.0, 30.0));
    let sign = object(&map, "7");
    assert_eq!(sign.shape, ObjectShape::Text("Hello there".into()));
    assert_eq!(
      (sign.position, sign.size),
      (Vec2::new(2.0, 2.0), Vec2::new(60.0, 20.0))
    );
    assert!(!object(&map, "13").visible);
    let gem = object(&map, "8");
    assert_eq!(gem.tile, Some(Tile::new(1, 0)));
    assert_eq!(gem.shape, ObjectShape::Rect);
    let flipped = object(&map, "12");
    assert_eq!(
      flipped.tile,
      Some(Tile::new(1, 0).with_flip(TileFlip::HORIZONTAL))
    );
  }

  #[test]
  fn a_class_member_left_at_its_default_reads_the_project_default() {
    let map = objects_map();
    // the crate wrote only hp and target; every other member of Crate is the project's
    let p = &object(&map, "1").properties;
    assert_eq!(p.float("speed"), Some(1.5));
    assert_eq!(p.string("label"), Some("plain"));
    assert_eq!(p.color("tint"), Some(Color::new(0x33, 0x66, 0x99, 0xff)));
    assert_eq!(p.get("material"), Some(&enum_of("Material", "wood")));
    assert_eq!(p.get("resist"), Some(&PropertyValue::List(vec![])));
    assert_eq!(p.int("hp"), Some(25), "what the file wrote still wins");

    // the barrel wrote a metal material, some resistances and half a Drop
    let barrel = &object(&map, "14").properties;
    assert_eq!(barrel.int("hp"), Some(10));
    assert_eq!(barrel.get("material"), Some(&enum_of("Material", "metal")));
    assert_eq!(
      barrel.get("resist"),
      Some(&PropertyValue::List(vec![
        enum_of("Elements", "fire"),
        enum_of("Elements", "poison")
      ]))
    );
    let Some(PropertyValue::Class { ty, fields }) = barrel.get("loot") else {
      panic!("loot is not a class")
    };
    assert_eq!(ty, "Drop");
    assert_eq!(fields.bool("rare"), Some(true));
    // count is not in the file: the Crate member's own default (3) beats Drop's (1)
    assert_eq!(fields.int("count"), Some(3));
  }

  #[test]
  fn nested_class_and_enum_properties_resolve() {
    let project = r#"{"propertyTypes":[
      {"name":"Dir","type":"enum","storageType":"int","values":["up","down","left"],"valuesAsFlags":false},
      {"name":"Tags","type":"enum","storageType":"string","values":["a","b","c"],"valuesAsFlags":true},
      {"name":"Leaf","type":"class","members":[{"name":"n","type":"int","value":7},{"name":"dir","type":"int","propertyType":"Dir","value":1}]},
      {"name":"Mid","type":"class","members":[{"name":"leaf","type":"class","propertyType":"Leaf","value":{"n":8}},{"name":"on","type":"bool","value":true}]},
      {"name":"Top","type":"class","members":[{"name":"mid","type":"class","propertyType":"Mid","value":{}},{"name":"tags","type":"string","propertyType":"Tags","value":"a"}]}
    ]}"#;
    let objects = r#"{"id":1,"name":"o","type":"Top","x":0,"y":0,"properties":[
      {"name":"mid","type":"class","propertytype":"Mid","value":{"leaf":{"dir":2}}},
      {"name":"tags","type":"string","propertytype":"Tags","value":"b, c"}]}"#;
    let map = load_with_project(&objects_json(objects), project).expect("loads");
    let p = &object(&map, "1").properties;
    let Some(PropertyValue::Class { ty, fields: mid }) = p.get("mid") else {
      panic!("mid")
    };
    assert_eq!(ty, "Mid");
    assert_eq!(mid.bool("on"), Some(true));
    let Some(PropertyValue::Class { fields: leaf, .. }) = mid.get("leaf") else {
      panic!("leaf")
    };
    assert_eq!(
      leaf.int("n"),
      Some(8),
      "Mid's default for leaf beats Leaf's own"
    );
    assert_eq!(
      leaf.get("dir"),
      Some(&enum_of("Dir", "left")),
      "an int enum reads by name"
    );
    assert_eq!(
      p.get("tags"),
      Some(&PropertyValue::List(vec![
        enum_of("Tags", "b"),
        enum_of("Tags", "c")
      ]))
    );

    // nothing written: the class fills every member, nested ones included
    let bare = r#"{"id":2,"name":"b","type":"Top","x":0,"y":0}"#;
    let map = load_with_project(&objects_json(bare), project).expect("loads");
    let p = &object(&map, "2").properties;
    assert_eq!(
      p.get("tags"),
      Some(&PropertyValue::List(vec![enum_of("Tags", "a")]))
    );
    let Some(PropertyValue::Class { fields: mid, .. }) = p.get("mid") else {
      panic!("mid")
    };
    let Some(PropertyValue::Class { fields: leaf, .. }) = mid.get("leaf") else {
      panic!("leaf")
    };
    assert_eq!(
      (leaf.int("n"), leaf.get("dir")),
      (Some(8), Some(&enum_of("Dir", "down")))
    );
  }

  fn ints(values: &[i64]) -> PropertyValue {
    PropertyValue::List(values.iter().map(|&v| PropertyValue::Int(v)).collect())
  }

  fn drop_of(count: i64, rare: bool) -> PropertyValue {
    let mut fields = Properties::default();
    fields.insert("count", PropertyValue::Int(count));
    fields.insert("rare", PropertyValue::Bool(rare));
    PropertyValue::Class {
      ty: "Drop".into(),
      fields,
    }
  }

  #[test]
  fn list_properties_load_with_their_items_typed() {
    // Saved by Tiled 1.12.2 from the fixture project, whose class Bag has
    // two list members with defaults
    let map = load("maps/lists.tmj");
    let p = &map.properties;
    assert_eq!(
      p.get("items"),
      Some(&PropertyValue::List(vec![
        PropertyValue::Int(7),
        ints(&[8, 9])
      ]))
    );
    assert_eq!(p.get("empty"), Some(&PropertyValue::List(vec![])));
    assert_eq!(
      p.get("mixed"),
      Some(&PropertyValue::List(vec![
        PropertyValue::Int(1),
        PropertyValue::Float(2.5),
        PropertyValue::String("three".into()),
        PropertyValue::Bool(true),
        PropertyValue::Color(Color::new(0x10, 0x20, 0x30, 0xff)),
        PropertyValue::File("a.png".into()),
        PropertyValue::Object("2".into()),
        enum_of("Material", "wood"),
        PropertyValue::List(vec![
          enum_of("Elements", "fire"),
          enum_of("Elements", "ice")
        ]),
        drop_of(4, false),
      ]))
    );
    // A class's list member the file leaves out takes the project's default,
    // its items resolved against their own classes
    let default_loot = PropertyValue::List(vec![drop_of(1, true), enum_of("Material", "metal")]);
    assert_eq!(p.get("loot"), Some(&default_loot));
    let plain = &object(&map, "1").properties;
    assert_eq!(plain.get("items"), Some(&ints(&[1, 2])));
    assert_eq!(plain.get("loot"), Some(&default_loot));
    let own = &object(&map, "2").properties;
    assert_eq!(
      own.get("loot"),
      Some(&PropertyValue::List(vec![drop_of(9, false)])),
      "a written list replaces the default whole"
    );
    assert_eq!(own.get("items"), Some(&ints(&[1, 2])));
    let Some(PropertyValue::Class { ty, fields }) = object(&map, "3").properties.get("bag") else {
      panic!("bag is not a class")
    };
    assert_eq!(ty, "Bag");
    assert_eq!(fields.get("items"), Some(&ints(&[5])));
    assert_eq!(fields.get("loot"), Some(&default_loot));
  }

  #[test]
  fn without_a_project_class_typed_properties_keep_what_is_written_and_warn_once() {
    let files = files_with(&[]);
    files.remove("project.tiled-project");
    let mut world = world_over(files);
    let map = load_in(&mut world, "maps/objects.tmj").expect("loads without a project");
    let barrel = &object(&map, "14").properties;
    assert_eq!(barrel.int("hp"), None, "no project: no defaults");
    let Some(PropertyValue::Class { ty, fields }) = barrel.get("loot") else {
      panic!("loot is not a class")
    };
    assert_eq!(ty, "Drop");
    assert_eq!(fields.bool("rare"), Some(true));
    assert_eq!(fields.len(), 1);
    // an enum keeps the written value
    assert_eq!(barrel.get("material"), Some(&enum_of("Material", "metal")));
    assert_eq!(barrel.get("resist"), Some(&enum_of("Elements", "5")));
  }

  #[test]
  fn a_template_instance_overrides_only_what_it_sets() {
    let map = objects_map();
    // all from the template but the position
    let plain = object(&map, "15");
    assert_eq!(plain.name, "template crate");
    assert_eq!(plain.class.as_deref(), Some("Crate"));
    assert_eq!(plain.position, Vec2::new(20.0, 20.0));
    assert_eq!(plain.size, Vec2::new(16.0, 16.0));
    assert_eq!(plain.properties.int("hp"), Some(50));
    assert_eq!(
      plain.properties.float("speed"),
      Some(1.5),
      "the class default fills the rest"
    );
    // a name and a size of its own
    let own = object(&map, "16");
    assert_eq!(own.name, "renamed");
    assert_eq!(own.size, Vec2::new(24.0, 24.0));
    assert_eq!(own.position, Vec2::new(30.0, 20.0));
    assert_eq!(own.properties.int("hp"), Some(50));
    // a template with a tile: its tileset is the map's items tileset
    let gem = object(&map, "17");
    assert_eq!(gem.name, "template gem");
    assert_eq!(gem.tile, Some(Tile::new(1, 0)));
    assert_eq!(
      gem.position,
      Vec2::new(48.0, 32.0),
      "bottom left at (48, 48)"
    );
  }

  #[test]
  fn a_template_instance_overrides_a_property_and_keeps_the_others() {
    let files = files_with(&[
      (
        "t.tj",
        r#"{"type":"template","object":{"id":1,"name":"t","type":"Crate","x":0,"y":0,"width":4,"height":4,"properties":[{"name":"hp","type":"int","value":50},{"name":"label","type":"string","value":"tmpl"}]}}"#,
      ),
      (
        "m.tmj",
        &objects_json(
          r#"{"id":5,"template":"t.tj","x":1,"y":2,"properties":[{"name":"hp","type":"int","value":9}]}"#,
        ),
      ),
    ]);
    let mut world = world_over(files);
    let map = load_in(&mut world, "m.tmj").expect("loads");
    let p = &object(&map, "5").properties;
    assert_eq!((p.int("hp"), p.string("label")), (Some(9), Some("tmpl")));
    assert_eq!(object(&map, "5").size, Vec2::splat(4.0));
  }

  #[test]
  fn a_tile_object_sits_by_its_tileset_alignment() {
    let map = objects_map();
    // items has no alignment: Tiled's bottom-left, so the position is the bottom-left corner
    assert_eq!(object(&map, "8").position, Vec2::new(64.0, 48.0));
    // aligned.tsj says center
    assert_eq!(object(&map, "9").position, Vec2::new(92.0, 32.0));
    assert_eq!(object(&map, "9").tile, Some(Tile::new(2, 0)));
    // a rectangle's position is its top-left corner already; it turns about it
    let turned = object(&map, "10");
    assert_eq!(turned.position, Vec2::new(50.0, 50.0));
    assert!((turned.rotation - FRAC_PI_2).abs() < 1e-6);
    // a tile object turned 90 degrees about its bottom-left corner (30, 30):
    // its top-left corner ends up at (46, 30)
    let gem = object(&map, "11");
    assert!(
      (gem.position - Vec2::new(46.0, 30.0)).length() < 1e-4,
      "{:?}",
      gem.position
    );
    assert!((gem.rotation - FRAC_PI_2).abs() < 1e-6);

    // an embedded tileset carries its alignment in the map
    let inline = r#"{"firstgid":1,"name":"e","tilewidth":16,"tileheight":16,"tilecount":16,"columns":4,
        "objectalignment":"bottomright","image":"images/tiles.png","imagewidth":64,"imageheight":64}"#;
    let json = map_json(
      "",
      r#"{"id":1,"name":"things","type":"objectgroup","objects":[{"id":1,"gid":2,"x":100,"y":100,"width":10,"height":20}]}"#,
      inline,
    );
    let map = load_json(&json).expect("loads");
    assert_eq!(object(&map, "1").position, Vec2::new(90.0, 80.0));
    assert_eq!(object(&map, "1").tile, Some(Tile::new(0, 1)));
  }

  #[test]
  fn wang_sets_load_with_their_colours_and_tile_ids() {
    let mut world = world_over(files_with(&[]));
    let handle = AssetsView::new(&mut world)
      .load_now::<Tileset>("tilesets/terrain.tsj")
      .expect("loads");
    let set = world
      .resource::<Assets<Tileset>>()
      .get(&handle)
      .expect("stored");
    let wang_tile = |tile, wangid| WangTile {
      tile,
      wangid,
      probability: 1.0,
    };
    let by_name = |name: &str| -> &WangSet {
      match set.wang_sets.iter().find(|w| w.name == name) {
        Some(w) => w,
        None => panic!("no wang set {name}"),
      }
    };
    assert_eq!(set.wang_sets.len(), 3);

    let ground = by_name("ground");
    assert_eq!(ground.kind, WangKind::Corner);
    assert_eq!(ground.colors.len(), 2);
    assert_eq!(ground.colors[0].name, "grass");
    assert_eq!(ground.colors[0].color, Color::rgb(0x00, 0xaa, 0x00));
    assert_eq!(ground.colors[0].tile, Some(0));
    assert_eq!(ground.colors[1].tile, None);
    assert_eq!(ground.colors[1].probability, 0.5);
    assert_eq!(ground.colors[1].properties.float("speed"), Some(0.5));
    assert_eq!(
      ground.tiles,
      [
        wang_tile(0, [0, 1, 0, 1, 0, 1, 0, 1]),
        wang_tile(1, [0, 2, 0, 1, 0, 1, 0, 1]),
        wang_tile(2, [0, 2, 0, 2, 0, 2, 0, 2])
      ]
    );

    let walls = by_name("walls");
    assert_eq!(walls.kind, WangKind::Edge);
    assert_eq!(walls.colors[0].tile, Some(3));
    assert_eq!(walls.tiles[1], wang_tile(4, [1, 0, 0, 0, 1, 0, 0, 0]));

    let mix = by_name("mix");
    assert_eq!(mix.kind, WangKind::Mixed);
    assert_eq!(mix.colors.len(), 2);
    assert_eq!(mix.colors[1].color, Color::rgb(0, 0, 255));
    assert_eq!(mix.tiles[1], wang_tile(6, [2, 2, 1, 1, 0, 0, 0, 1]));
  }

  #[test]
  fn a_tileset_s_transformations_load_into_it_and_its_wang_sets() {
    let mut world = world_over(files_with(&[]));
    let mut load = |path: &str| {
      let handle = AssetsView::new(&mut world)
        .load_now::<Tileset>(path)
        .expect("loads");
      world
        .resource::<Assets<Tileset>>()
        .get(&handle)
        .expect("stored")
        .clone()
    };
    let turn = load("tilesets/wang-turn.tsj");
    let all = TileTransformations {
      hflip: true,
      vflip: true,
      rotate: true,
      prefer_untransformed: true,
    };
    assert_eq!(turn.transformations, all);
    assert_eq!(turn.wang_sets[0].transformations, all);
    // none written: none allowed
    let plain = load("tilesets/wang.tsj");
    assert_eq!(plain.transformations, TileTransformations::default());
    assert_eq!(
      plain.wang_sets[0].transformations,
      TileTransformations::default()
    );
  }

  #[test]
  fn the_project_path_comes_from_the_loader_then_the_default() {
    let project = r#"{"propertyTypes":[{"name":"Crate","type":"class","members":[{"name":"hp","type":"int","value":77}]}]}"#;
    let json = objects_json(r#"{"id":1,"name":"c","type":"Crate","x":0,"y":0}"#);
    // the default project at the asset root
    let map = load_with_project(&json, project).expect("loads");
    assert_eq!(object(&map, "1").properties.int("hp"), Some(77));
    // a loader told another file reads that one, not the default
    let files = files_with(&[("m.tmj", &json), ("kit/other.tiled-project", project)]);
    files.remove("project.tiled-project");
    let mut world = world_over(files);
    let server = world.resource::<AssetServer>().clone();
    server.register_loader(TiledMapLoader::with_project("kit/other.tiled-project"));
    server.register_loader(TiledTilesetLoader::with_project("kit/other.tiled-project"));
    let map = load_in(&mut world, "m.tmj").expect("loads");
    assert_eq!(object(&map, "1").properties.int("hp"), Some(77));
    // and one that names a file that is not there fails, naming it
    let files = files_with(&[("m.tmj", &json)]);
    let mut world = world_over(files);
    let server = world.resource::<AssetServer>().clone();
    server.register_loader(TiledMapLoader::with_project("kit/missing.tiled-project"));
    let error = load_in(&mut world, "m.tmj").expect_err("loaded");
    assert!(error.contains("kit/missing.tiled-project"), "{error}");
  }

  #[test]
  fn a_saved_project_loads_its_maps_again() {
    let project = |hp: i32| {
      format!(
        r#"{{"propertyTypes":[{{"name":"Crate","type":"class","members":[{{"name":"hp","type":"int","value":{hp}}}]}}]}}"#
      )
    };
    let json = objects_json(r#"{"id":1,"name":"c","type":"Crate","x":0,"y":0}"#);
    let files = files_with(&[("m.tmj", &json), ("project.tiled-project", &project(1))]);
    let mut world = world_over(files.clone());
    let handle = AssetsView::new(&mut world)
      .load_now::<TileMap>("m.tmj")
      .expect("loads");
    let hp = |world: &World| {
      let map = world
        .resource::<Assets<TileMap>>()
        .get(&handle)
        .expect("stored");
      object(map, "1").properties.int("hp")
    };
    assert_eq!(hp(&world), Some(1));
    files.insert("project.tiled-project", project(2));
    let server = world.resource::<AssetServer>().clone();
    server.reload("m.tmj");
    pump_until(&mut world, |w| hp(w) == Some(2));
    assert_eq!(hp(&world), Some(2));
  }

  #[test]
  fn project_and_object_errors_name_their_place() {
    let json = objects_json(r#"{"id":1,"name":"c","x":0,"y":0}"#);
    let error = load_with_project(&json, "{ nope").expect_err("loaded");
    assert!(
      error.contains("project.tiled-project") && error.contains("line 1"),
      "{error}"
    );

    let after_first = map_json(
      "",
      r#"{"id":1,"name":"things","type":"objectgroup","objects":[{"id":4,"gid":2,"x":0,"y":0}]}"#,
      r#"{"firstgid":5,"source":"tilesets/tiles.tsj"}"#,
    );
    let error = failure(&after_first);
    assert!(
      error.contains(r#"layer "things""#) && error.contains("object 4"),
      "{error}"
    );
    // a tile id past its tileset is found once the tileset is in
    let error = failure(&objects_json(r#"{"id":4,"gid":99,"x":0,"y":0}"#));
    assert!(
      error.contains(r#"layer "things""#) && error.contains("tile id 98"),
      "{error}"
    );
    let error = failure(&objects_json(
      r#"{"id":4,"template":"nope.tj","x":0,"y":0}"#,
    ));
    assert!(
      error.contains("object 4") && error.contains("nope.tj"),
      "{error}"
    );
    let error = failure(&objects_json(
      r#"{"id":4,"template":"../../x.tj","x":0,"y":0}"#,
    ));
    assert!(error.contains("object 4"), "{error}");

    let files = files_with(&[
      (
        "t.tj",
        r#"{"type":"template","tileset":{"firstgid":1,"source":"tilesets/items.tsj"},"object":{"id":1,"gid":1,"width":1,"height":1}}"#,
      ),
      (
        "m.tmj",
        &objects_json(r#"{"id":4,"template":"t.tj","x":0,"y":0}"#),
      ),
    ]);
    let mut world = world_over(files);
    let error = load_in(&mut world, "m.tmj").expect_err("loaded");
    assert!(
      error.contains("object 4") && error.contains("not one of the map's tilesets"),
      "{error}"
    );

    let bad_wang = |wang: &str| {
      format!(
        r#"{{"name":"w","tilewidth":16,"tileheight":16,"tilecount":4,"columns":2,"image":"images/tiles.png","imagewidth":32,"imageheight":32,"wangsets":[{wang}]}}"#
      )
    };
    for (wang, what) in [
      (
        r#"{"name":"s","type":"hex","colors":[],"wangtiles":[]}"#,
        "corner, edge or mixed",
      ),
      (
        r##"{"name":"s","type":"edge","colors":[{"name":"a","color":"#ff0000"}],"wangtiles":[{"tileid":0,"wangid":[1,0,0]}]}"##,
        "not 8",
      ),
      (
        r##"{"name":"s","type":"edge","colors":[{"name":"a","color":"#ff0000"}],"wangtiles":[{"tileid":0,"wangid":[2,0,0,0,0,0,0,0]}]}"##,
        "color 2",
      ),
    ] {
      let files = files_with(&[("w.tsj", &bad_wang(wang))]);
      let mut world = world_over(files);
      let error = AssetsView::new(&mut world)
        .load_now::<Tileset>("w.tsj")
        .expect_err("loaded")
        .report();
      assert!(
        error.contains(what) && error.contains("wang set"),
        "{error}"
      );
    }
    let _ = Properties::default();
  }

  // ---- objects on every grid -----------------------------------------------

  /// The object named `name` of the layer "things"
  fn named<'a>(map: &'a TileMap, name: &str) -> &'a MapObjectData {
    match objects(map, "things").iter().find(|o| o.name == name) {
      Some(o) => o,
      None => panic!("no object named {name}"),
    }
  }

  /// What Tiled measured for `map`'s object of id `id`: its anchor, and the
  /// box its render covers, if it draws one
  fn measured_object(map: &str, id: u64) -> (Vec2, Option<[f32; 4]>) {
    let text = std::fs::read_to_string(format!("{FIXTURES}/measured-objects.json"))
      .expect("measured-objects.json");
    let json: serde_json::Value = serde_json::from_str(&text).expect("it is JSON");
    let list = json["objects"].as_array().expect("objects");
    let row = list
      .iter()
      .find(|r| r["map"] == map && r["id"] == id)
      .unwrap_or_else(|| panic!("{map}, object {id} was not measured"));
    let n = |v: &serde_json::Value| v.as_f64().expect("a number") as f32;
    let anchor = Vec2::new(n(&row["anchor"][0]), n(&row["anchor"][1]));
    let box_ = row["box"]
      .as_array()
      .map(|b| [n(&b[0]), n(&b[1]), n(&b[2]), n(&b[3])]);
    (anchor, box_)
  }

  /// The map `name` of the fixtures (the TMX twins load the same: the xml
  /// module's tests hold them to it)
  fn fixture(name: &str) -> TileMap {
    load(&format!("maps/{name}.tmj"))
  }

  /// An object's outline in its layer's pixels, as it draws: a polygon's
  /// points, or a rectangle's corners (a tile object's picture, a rectangle
  /// or an ellipse's box), turned about its position
  fn outline(o: &MapObjectData) -> Vec<Vec2> {
    let local: Vec<Vec2> = match &o.shape {
      ObjectShape::Polygon(points) => points.clone(),
      _ => vec![
        Vec2::ZERO,
        Vec2::new(o.size.x, 0.0),
        o.size,
        Vec2::new(0.0, o.size.y),
      ],
    };
    let turn = Vec2::from_angle(o.rotation);
    local
      .into_iter()
      .map(|p| o.position + turn.rotate(p))
      .collect()
  }

  fn bounds(points: &[Vec2]) -> [f32; 4] {
    let mut b = [f32::MAX, f32::MAX, f32::MIN, f32::MIN];
    for p in points {
      b = [b[0].min(p.x), b[1].min(p.y), b[2].max(p.x), b[3].max(p.y)];
    }
    b
  }

  fn assert_close(got: Vec2, want: Vec2, what: &str) {
    assert!(got.distance(want) < 1e-3, "{what}: {got} is not {want}");
  }

  #[test]
  fn isometric_objects_project_from_tileds_stored_space() {
    {
      let map = fixture("shape-objects-iso");
      assert_eq!(map.shape, GridShape::Isometric);
      // 6 rows of 64-wide cells: the origin is 192 across. A stored point
      // is in tile-height units along the diamond's axes
      let point = named(&map, "point");
      assert_eq!(point.shape, ObjectShape::Point);
      assert_close(point.position, Vec2::new(224.0, 64.0), "point (80, 48)");
      // the rectangle is its four projected corners, steps from its top
      // vertex
      let rect = named(&map, "rect");
      assert_close(rect.position, Vec2::new(192.0, 32.0), "rect position");
      assert_eq!(
        rect.shape,
        ObjectShape::Polygon(vec![
          Vec2::new(0.0, 0.0),
          Vec2::new(32.0, 16.0),
          Vec2::new(-32.0, 48.0),
          Vec2::new(-64.0, 32.0),
        ])
      );
      assert_eq!(rect.size, Vec2::ZERO);
      // the ellipse is 32 points round its projected box
      let ellipse = named(&map, "ellipse");
      let ObjectShape::Polygon(points) = &ellipse.shape else {
        panic!("{:?}", ellipse.shape)
      };
      assert_eq!(points.len(), 32);
      // the stored ellipse's right-most point, (64, 16) from its corner,
      // projects to (48, 40); the whole curve stays in the projected box
      // (x -32..64, y 0..48 about the anchor), which Tiled draws as well
      assert_close(points[0], Vec2::new(48.0, 40.0), "ellipse point");
      let [l, t, r, b] = bounds(points);
      assert!(
        l >= -32.0 && r <= 64.0 && t >= 0.0 && b <= 48.0,
        "{l} {t} {r} {b}"
      );
      // the polygon's points are projected as steps
      let polygon = named(&map, "polygon");
      assert_close(polygon.position, Vec2::new(128.0, 64.0), "polygon position");
      assert_eq!(
        polygon.shape,
        ObjectShape::Polygon(vec![
          Vec2::new(0.0, 0.0),
          Vec2::new(16.0, 8.0),
          Vec2::new(0.0, 16.0),
          Vec2::new(-8.0, 4.0),
        ])
      );
      // a tile object's picture stands on its anchor, bottom-centre:
      // 64 x 64 at stored (64, 96), anchor (160, 80), covers 128..192 and 16..80
      let tile = named(&map, "tile");
      assert!(tile.tile.is_some());
      assert_eq!(tile.size, Vec2::new(64.0, 64.0));
      assert_close(tile.position, Vec2::new(128.0, 16.0), "tile position");
    }
  }

  #[test]
  fn oblique_objects_skew_and_staggered_and_hex_ones_do_not() {
    {
      let map = fixture("shape-objects-oblique");
      // 32 x 32 cells, skew (8, 12): a stored point is unskewed pixels
      let point = named(&map, "point");
      assert_close(
        point.position,
        Vec2::new(92.0, 78.0),
        "oblique point (80, 48)",
      );
      let rect = named(&map, "rect");
      assert_close(
        rect.position,
        Vec2::new(40.0, 44.0),
        "oblique rect position",
      );
      assert_eq!(
        rect.shape,
        ObjectShape::Polygon(vec![
          Vec2::new(0.0, 0.0),
          Vec2::new(32.0, 12.0),
          Vec2::new(48.0, 76.0),
          Vec2::new(16.0, 64.0),
        ])
      );
      let tile = named(&map, "tile");
      // bottom-left on the skewed anchor: (64, 96) is (88, 120)
      assert_close(
        tile.position,
        Vec2::new(88.0, 56.0),
        "oblique tile position",
      );
    }
    for name in ["shape-objects-stag", "shape-objects-hex"] {
      {
        let map = fixture(name);
        assert_close(named(&map, "point").position, Vec2::new(80.0, 48.0), name);
        let rect = named(&map, "rect");
        assert_eq!(rect.shape, ObjectShape::Rect, "{name}");
        assert_close(rect.position, Vec2::new(32.0, 32.0), name);
        assert_eq!(rect.size, Vec2::new(32.0, 64.0), "{name}");
        assert_eq!(named(&map, "ellipse").shape, ObjectShape::Ellipse, "{name}");
        // bottom-left, as on orthogonal maps: 64 x 64 at (64, 96)
        assert_close(named(&map, "tile").position, Vec2::new(64.0, 32.0), name);
      }
    }
  }

  #[test]
  fn a_rotated_object_matches_tileds_corners_on_every_grid() {
    // The measurements are of Tiled's render of each object alone: a tile
    // object's picture is exact, an outline has a pen about 3 pixels wide, more at a sharp corner
    for name in [
      "shape-objects-iso",
      "shape-objects-oblique",
      "shape-objects-stag",
      "shape-objects-hex",
    ] {
      {
        let map = fixture(name);
        for (object, id, pen) in [
          ("rect", 2, 3.5),
          ("ellipse", 3, 3.5),
          ("polygon", 4, 5.0),
          ("tile", 5, 1.0),
          ("turned rect", 6, 5.0),
          ("turned tile", 7, 1.0),
        ] {
          let o = named(&map, object);
          let (anchor, measured) = measured_object(name, id);
          let [l, t, r, b] = measured.expect("drawn");
          let got = bounds(&outline(o));
          // Tiled draws an ellipse of these grids inside its projected
          // box, and the box too: the curve stays in what it covers
          let curved = object == "ellipse" && matches!(o.shape, ObjectShape::Polygon(_));
          if curved {
            assert!(
              got[0] >= l - pen && got[1] >= t - pen && got[2] <= r + pen && got[3] <= b + pen,
              "{name}, {object}: {got:?} is not inside {:?}",
              [l, t, r, b]
            );
          }
          for (mine, theirs, what) in [
            (got[0], l, "left"),
            (got[1], t, "top"),
            (got[2], r, "right"),
            (got[3], b, "bottom"),
          ]
          .into_iter()
          .filter(|_| !curved)
          {
            assert!(
              (mine - theirs).abs() <= pen,
              "{name}, {object}: {what} is {mine}, Tiled draws {theirs} ({got:?} against {:?})",
              [l, t, r, b]
            );
          }
          // a tile object's position is its corner, whose alignment point
          // is Tiled's anchor; every other shape's position is the anchor
          if o.tile.is_none() {
            assert_close(o.position, anchor, &format!("{name}, {object} anchor"));
          }
        }
        assert_close(
          named(&map, "point").position,
          measured_object(name, 1).0,
          name,
        );
      }
    }
  }

  #[test]
  fn an_isometric_tile_object_honours_its_tilesets_alignment() {
    // 64 x 64 at stored (64, 96): the anchor is (160, 80) whichever way
    // it is aligned (Tiled's measure)
    for (name, id, offset) in [
      // unspecified in a tileset file, aligned bottom-centre by Tiled
      ("shape-objects-iso-external", 5, Vec2::new(-32.0, -64.0)),
      ("shape-objects-iso-bottomleft", 5, Vec2::new(0.0, -64.0)),
      ("shape-objects-iso-center", 5, Vec2::new(-32.0, -32.0)),
    ] {
      {
        let map = fixture(name);
        let tile = named(&map, "tile");
        let (anchor, measured) = measured_object(name, id);
        assert_close(anchor, Vec2::new(160.0, 80.0), name);
        assert_close(tile.position, anchor + offset, name);
        let [l, t, r, b] = measured.expect("drawn");
        assert_eq!(
          bounds(&outline(tile)),
          [l, t, r, b],
          "{name}: Tiled draws its picture there"
        );
      }
    }
    // an inline tileset that says nothing is bottom-centre on an isometric
    // map and bottom-left on any other
    let map = load("maps/shape-objects-iso.tmj");
    assert_close(
      named(&map, "tile").position,
      Vec2::new(128.0, 16.0),
      "inline",
    );
    let map = load("maps/shape-objects-oblique.tmj");
    assert_close(
      named(&map, "tile").position,
      Vec2::new(88.0, 56.0),
      "oblique",
    );
  }
}
