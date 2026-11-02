//! Tiled's XML formats: maps (`.tmx`), tilesets (`.tsx`) and templates
//! (`.tx`), read into the same [`raw`](super::raw) structs the JSON reader
//! fills, so a file loads to what its JSON twin loads to.
//!
//! An attribute that is absent leaves the field `None` (or zero, where the
//! JSON default is zero): the builder owns every default, and a template's
//! instance overrides only what it sets. Errors read `line L, column C: what`
//! and point at the attribute, or at the element when the fault is its own.

use roxmltree::{Document, Node};
use serde_json::{Map, Number, Value};

use super::build::MAX_DEPTH;
use super::raw::{
  RawChunk, RawData, RawFrame, RawLayer, RawMap, RawObject, RawOffset, RawPoint, RawProperty,
  RawTemplate, RawText, RawTile, RawTileset, RawTilesetRef, RawTransformations, RawWangColor,
  RawWangSet, RawWangTile,
};

type Parsed<T> = Result<T, String>;

/// Where an attribute value, or an element, begins
fn at(doc: &Document<'_>, offset: usize) -> String {
  let pos = doc.text_pos_at(offset);
  format!("line {}, column {}", pos.row, pos.col)
}

fn fail<T>(doc: &Document<'_>, node: Node<'_, '_>, what: impl std::fmt::Display) -> Parsed<T> {
  Err(format!("{}: {what}", at(doc, node.range().start)))
}

/// A reader over one parsed file
struct Reader<'a, 'i> {
  doc: &'a Document<'i>,
}

impl<'a, 'i> Reader<'a, 'i> {
  fn bad<T>(&self, node: Node<'a, 'i>, name: &str, what: impl std::fmt::Display) -> Parsed<T> {
    let offset = node
      .attribute_node(name)
      .map_or(node.range().start, |a| a.range().start);
    Err(format!("{}: {name}: {what}", at(self.doc, offset)))
  }

  fn str(&self, node: Node<'a, 'i>, name: &str) -> Option<&'a str> {
    node.attribute(name)
  }

  /// The first of `names` the element has; Tiled 1.9 renamed `type` to `class`
  fn class(&self, node: Node<'a, 'i>) -> Option<String> {
    ["class", "type"]
      .iter()
      .find_map(|n| node.attribute(*n))
      .map(str::to_string)
  }

  fn num<T: std::str::FromStr>(&self, node: Node<'a, 'i>, name: &str) -> Parsed<Option<T>> {
    let Some(text) = node.attribute(name) else {
      return Ok(None);
    };
    match text.trim().parse::<T>() {
      Ok(n) => Ok(Some(n)),
      Err(_) => self.bad(node, name, format!("{text:?} is not a valid number")),
    }
  }

  fn float(&self, node: Node<'a, 'i>, name: &str) -> Parsed<Option<f64>> {
    match self.num::<f64>(node, name)? {
      Some(f) if !f.is_finite() => self.bad(node, name, "is not a finite number"),
      other => Ok(other),
    }
  }

  fn flag(&self, node: Node<'a, 'i>, name: &str) -> Parsed<Option<bool>> {
    match node.attribute(name).map(str::trim) {
      None => Ok(None),
      Some("1" | "true") => Ok(Some(true)),
      Some("0" | "false") => Ok(Some(false)),
      Some(other) => self.bad(node, name, format!("{other:?} is not 0, 1, true or false")),
    }
  }

  fn children(
    &self,
    node: Node<'a, 'i>,
    name: &'static str,
  ) -> impl Iterator<Item = Node<'a, 'i>> + use<'a, 'i> {
    node
      .children()
      .filter(move |c| c.is_element() && c.tag_name().name() == name)
  }

  fn child(&self, node: Node<'a, 'i>, name: &'static str) -> Option<Node<'a, 'i>> {
    self.children(node, name).next()
  }

  // ---- properties ---------------------------------------------------------

  fn properties(&self, node: Node<'a, 'i>) -> Parsed<Vec<RawProperty>> {
    self.properties_at(node, 0)
  }

  fn properties_at(&self, node: Node<'a, 'i>, depth: usize) -> Parsed<Vec<RawProperty>> {
    let Some(list) = self.child(node, "properties") else {
      return Ok(Vec::new());
    };
    if depth > MAX_DEPTH {
      return fail(self.doc, list, "classes are nested too deep");
    }
    let mut out = Vec::new();
    for p in self.children(list, "property") {
      let ty = p.attribute("type").unwrap_or("string").to_string();
      let value = self.property_value(p, &ty, depth)?;
      out.push(RawProperty {
        name: p.attribute("name").unwrap_or_default().to_string(),
        ty,
        propertytype: p.attribute("propertytype").map(str::to_string),
        value,
      });
    }
    Ok(out)
  }

  /// The property's value as the JSON file would hold it: a number for `int`,
  /// `float` and `object`, a flag for `bool`, an object for a class, an
  /// array of typed items for a list
  fn property_value(&self, p: Node<'a, 'i>, ty: &str, depth: usize) -> Parsed<Value> {
    // a string may sit between the tags instead, for several lines
    let written = match p.attribute("value") {
      Some(v) => v.to_string(),
      None if ty == "string" || ty == "color" || ty == "file" => text_of(p),
      None => String::new(),
    };
    let text = written.trim();
    Ok(match ty {
      "int" => match text {
        "" => Value::from(0),
        _ => match text.parse::<i64>() {
          Ok(n) => Value::from(n),
          Err(_) => return self.bad(p, "value", format!("{text:?} is not an int")),
        },
      },
      "float" => match text {
        "" => Value::from(0),
        _ => match text.parse::<f64>().ok().and_then(json_number) {
          Some(n) => n,
          None => return self.bad(p, "value", format!("{text:?} is not a float")),
        },
      },
      "bool" => match text {
        "" | "false" | "0" => Value::Bool(false),
        "true" | "1" => Value::Bool(true),
        _ => return self.bad(p, "value", format!("{text:?} is not true or false")),
      },
      "object" => match text.parse::<i64>() {
        Ok(n) => Value::from(n),
        Err(_) => Value::String(written),
      },
      "class" => {
        let mut members = Map::new();
        for m in self.properties_at(p, depth + 1)? {
          members.insert(m.name, m.value);
        }
        Value::Object(members)
      }
      // Tiled 1.12's lists: `<item type=".." propertytype=".." value=".."/>`,
      // each read as a property's value is (a class item holds its members,
      // a list item its items), into the JSON file's `{type, value}` items
      "list" => {
        if depth > MAX_DEPTH {
          return fail(self.doc, p, "lists are nested too deep");
        }
        let mut items = Vec::new();
        for item in self.children(p, "item") {
          let ty = item.attribute("type").unwrap_or("string").to_string();
          let value = self.property_value(item, &ty, depth + 1)?;
          let mut entry = Map::new();
          if let Some(custom) = item.attribute("propertytype") {
            entry.insert("propertytype".into(), Value::String(custom.to_string()));
          }
          entry.insert("type".into(), Value::String(ty));
          entry.insert("value".into(), value);
          items.push(Value::Object(entry));
        }
        Value::Array(items)
      }
      _ => Value::String(written),
    })
  }

  // ---- tilesets -----------------------------------------------------------

  fn tileset_ref(&self, node: Node<'a, 'i>) -> Parsed<RawTilesetRef> {
    let source = self.str(node, "source").map(str::to_string);
    let inline = if source.is_some() {
      RawTileset::default()
    } else {
      self.tileset(node)?
    };
    Ok(RawTilesetRef {
      firstgid: self.num(node, "firstgid")?.unwrap_or(0),
      source,
      inline,
    })
  }

  fn tileset(&self, node: Node<'a, 'i>) -> Parsed<RawTileset> {
    let mut set = RawTileset {
      name: self.str(node, "name").unwrap_or_default().to_string(),
      class: self.class(node),
      tilewidth: self.num(node, "tilewidth")?.unwrap_or(0),
      tileheight: self.num(node, "tileheight")?.unwrap_or(0),
      spacing: self.num(node, "spacing")?.unwrap_or(0),
      margin: self.num(node, "margin")?.unwrap_or(0),
      tilecount: self.num(node, "tilecount")?,
      columns: self.num(node, "columns")?.unwrap_or(0),
      objectalignment: self.str(node, "objectalignment").map(str::to_string),
      tilerendersize: self.str(node, "tilerendersize").map(str::to_string),
      properties: self.properties(node)?,
      ..RawTileset::default()
    };
    if let Some(offset) = self.child(node, "tileoffset") {
      set.tileoffset = RawOffset {
        x: self.num(offset, "x")?.unwrap_or(0),
        y: self.num(offset, "y")?.unwrap_or(0),
      };
    }
    if let Some(image) = self.child(node, "image") {
      set.image = self.str(image, "source").map(str::to_string);
      set.imagewidth = self.num(image, "width")?.unwrap_or(0);
      set.imageheight = self.num(image, "height")?.unwrap_or(0);
      set.transparentcolor = self.str(image, "trans").map(str::to_string);
    }
    if let Some(node) = self.child(node, "transformations") {
      let flag = |name: &str| {
        self
          .str(node, name)
          .is_some_and(|v| v == "1" || v == "true")
      };
      set.transformations = RawTransformations {
        hflip: flag("hflip"),
        vflip: flag("vflip"),
        rotate: flag("rotate"),
        preferuntransformed: flag("preferuntransformed"),
      };
    }
    for tile in self.children(node, "tile") {
      set.tiles.push(self.tile(tile)?);
    }
    if let Some(sets) = self.child(node, "wangsets") {
      for wang in self.children(sets, "wangset") {
        set.wangsets.push(self.wang_set(wang)?);
      }
    }
    Ok(set)
  }

  fn tile(&self, node: Node<'a, 'i>) -> Parsed<RawTile> {
    let mut tile = RawTile {
      id: self.num(node, "id")?.unwrap_or(0),
      class: self.class(node),
      probability: self.num(node, "probability")?,
      x: self.num(node, "x")?,
      y: self.num(node, "y")?,
      width: self.num(node, "width")?,
      height: self.num(node, "height")?,
      properties: self.properties(node)?,
      ..RawTile::default()
    };
    if let Some(image) = self.child(node, "image") {
      tile.image = self.str(image, "source").map(str::to_string);
      tile.imagewidth = self.num(image, "width")?.unwrap_or(0);
      tile.imageheight = self.num(image, "height")?.unwrap_or(0);
    }
    if let Some(group) = self.child(node, "objectgroup") {
      tile.objectgroup = Some(self.layer(group, 0)?);
    }
    if let Some(animation) = self.child(node, "animation") {
      for frame in self.children(animation, "frame") {
        tile.animation.push(RawFrame {
          tileid: self.num(frame, "tileid")?.unwrap_or(0),
          duration: self.num(frame, "duration")?.unwrap_or(0),
        });
      }
    }
    Ok(tile)
  }

  fn wang_set(&self, node: Node<'a, 'i>) -> Parsed<RawWangSet> {
    let mut set = RawWangSet {
      name: self.str(node, "name").unwrap_or_default().to_string(),
      class: self.class(node),
      ty: self.str(node, "type").unwrap_or_default().to_string(),
      properties: self.properties(node)?,
      ..RawWangSet::default()
    };
    for color in self.children(node, "wangcolor") {
      set.colors.push(RawWangColor {
        name: self.str(color, "name").unwrap_or_default().to_string(),
        class: self.class(color),
        color: self.str(color, "color").unwrap_or_default().to_string(),
        tile: self.num(color, "tile")?,
        probability: self.num(color, "probability")?,
        properties: self.properties(color)?,
      });
    }
    for tile in self.children(node, "wangtile") {
      let mut wangid = Vec::new();
      for part in self.str(tile, "wangid").unwrap_or_default().split(',') {
        match part.trim().parse::<u32>() {
          Ok(n) => wangid.push(n),
          Err(_) => {
            return self.bad(tile, "wangid", format!("{part:?} is not a color index"));
          }
        }
      }
      set.wangtiles.push(RawWangTile {
        tileid: self.num(tile, "tileid")?.unwrap_or(0),
        wangid,
      });
    }
    Ok(set)
  }

  // ---- layers -------------------------------------------------------------

  /// A `layer`, `objectgroup`, `imagelayer` or `group` element
  fn layer(&self, node: Node<'a, 'i>, depth: usize) -> Parsed<RawLayer> {
    if depth > MAX_DEPTH {
      return fail(self.doc, node, "groups are nested too deep");
    }
    let ty = match node.tag_name().name() {
      "layer" => "tilelayer",
      "objectgroup" => "objectgroup",
      "imagelayer" => "imagelayer",
      _ => "group",
    };
    let mut layer = RawLayer {
      id: self.num(node, "id")?,
      name: self.str(node, "name").unwrap_or_default().to_string(),
      class: self.class(node),
      ty: ty.to_string(),
      offsetx: self.num(node, "offsetx")?.unwrap_or(0.0),
      offsety: self.num(node, "offsety")?.unwrap_or(0.0),
      opacity: self.num(node, "opacity")?,
      visible: self.flag(node, "visible")?,
      tintcolor: self.str(node, "tintcolor").map(str::to_string),
      parallaxx: self.num(node, "parallaxx")?,
      parallaxy: self.num(node, "parallaxy")?,
      mode: self.str(node, "mode").map(str::to_string),
      width: self.num(node, "width")?,
      height: self.num(node, "height")?,
      repeatx: self.flag(node, "repeatx")?.unwrap_or(false),
      repeaty: self.flag(node, "repeaty")?.unwrap_or(false),
      draworder: self.str(node, "draworder").map(str::to_string),
      properties: self.properties(node)?,
      ..RawLayer::default()
    };
    match ty {
      "tilelayer" => {
        if let Some(data) = self.child(node, "data") {
          self.layer_data(data, &mut layer)?;
        }
      }
      "objectgroup" => {
        for object in self.children(node, "object") {
          layer.objects.push(self.object(object)?);
        }
      }
      "imagelayer" => {
        let image = self.child(node, "image");
        layer.image = image
          .and_then(|i| self.str(i, "source"))
          .map(str::to_string);
        layer.transparentcolor = image.and_then(|i| self.str(i, "trans")).map(str::to_string);
      }
      _ => {
        for child in node.children().filter(Node::is_element) {
          if matches!(
            child.tag_name().name(),
            "layer" | "objectgroup" | "imagelayer" | "group"
          ) {
            layer.layers.push(self.layer(child, depth + 1)?);
          }
        }
      }
    }
    Ok(layer)
  }

  /// A tile layer's `<data>`: its cells, or an infinite map's `<chunk>`s
  /// of them, each in the layer's encoding
  fn layer_data(&self, data: Node<'a, 'i>, layer: &mut RawLayer) -> Parsed<()> {
    layer.encoding = self.str(data, "encoding").map(str::to_string);
    layer.compression = self.str(data, "compression").map(str::to_string);
    let plain = layer.encoding.is_none();
    if self.child(data, "chunk").is_some() {
      let mut chunks = Vec::new();
      for chunk in self.children(data, "chunk") {
        // An attribute left out stays `None`: the build refuses it,
        // naming the chunk
        chunks.push(RawChunk {
          x: self.num(chunk, "x")?,
          y: self.num(chunk, "y")?,
          width: self.num(chunk, "width")?,
          height: self.num(chunk, "height")?,
          data: Some(self.cells(chunk, plain)?),
        });
      }
      layer.chunks = Some(chunks);
      return Ok(());
    }
    layer.data = Some(self.cells(data, plain)?);
    Ok(())
  }

  /// The cells of a `<data>` or `<chunk>`: its `<tile>` elements' gids
  /// where there is no encoding (`plain`), else its text to decode
  fn cells(&self, node: Node<'a, 'i>, plain: bool) -> Parsed<RawData> {
    let tiles: Vec<Node<'a, 'i>> = self.children(node, "tile").collect();
    Ok(if plain && !tiles.is_empty() {
      let mut gids = Vec::with_capacity(tiles.len());
      for tile in tiles {
        gids.push(self.num(tile, "gid")?.unwrap_or(0));
      }
      RawData::Array(gids)
    } else {
      RawData::Text(text_of(node))
    })
  }

  fn object(&self, node: Node<'a, 'i>) -> Parsed<RawObject> {
    let mut object = RawObject {
      id: self.num(node, "id")?.unwrap_or(0),
      name: self.str(node, "name").map(str::to_string),
      class: self.class(node),
      x: self.float(node, "x")?,
      y: self.float(node, "y")?,
      width: self.float(node, "width")?,
      height: self.float(node, "height")?,
      rotation: self.float(node, "rotation")?,
      visible: self.flag(node, "visible")?,
      gid: self.num(node, "gid")?,
      template: self.str(node, "template").map(str::to_string),
      properties: self.properties(node)?,
      ..RawObject::default()
    };
    object.ellipse = self.child(node, "ellipse").is_some();
    object.point = self.child(node, "point").is_some();
    object.capsule = self.child(node, "capsule").is_some();
    if let Some(p) = self.child(node, "polygon") {
      object.polygon = Some(self.points(p)?);
    }
    if let Some(p) = self.child(node, "polyline") {
      object.polyline = Some(self.points(p)?);
    }
    if let Some(t) = self.child(node, "text") {
      object.text = Some(RawText { text: text_of(t) });
    }
    Ok(object)
  }

  /// The `points="x,y x,y"` of a polygon or polyline
  fn points(&self, node: Node<'a, 'i>) -> Parsed<Vec<RawPoint>> {
    let mut out = Vec::new();
    for pair in self
      .str(node, "points")
      .unwrap_or_default()
      .split_whitespace()
    {
      let parsed = pair.split_once(',').and_then(|(x, y)| {
        let (x, y) = (x.parse::<f64>().ok()?, y.parse::<f64>().ok()?);
        (x.is_finite() && y.is_finite()).then_some(RawPoint { x, y })
      });
      match parsed {
        Some(point) => out.push(point),
        None => return self.bad(node, "points", format!("{pair:?} is not an x,y pair")),
      }
    }
    Ok(out)
  }

  // ---- files --------------------------------------------------------------

  fn map(&self, node: Node<'a, 'i>) -> Parsed<RawMap> {
    let mut map = RawMap {
      orientation: self
        .str(node, "orientation")
        .unwrap_or_default()
        .to_string(),
      staggeraxis: self.str(node, "staggeraxis").map(str::to_string),
      staggerindex: self.str(node, "staggerindex").map(str::to_string),
      hexsidelength: self.num(node, "hexsidelength")?,
      skewx: self.num(node, "skewx")?.unwrap_or(0),
      skewy: self.num(node, "skewy")?.unwrap_or(0),
      renderorder: self
        .str(node, "renderorder")
        .unwrap_or_default()
        .to_string(),
      infinite: self.flag(node, "infinite")?.unwrap_or(false),
      width: self.num(node, "width")?.unwrap_or(0),
      height: self.num(node, "height")?.unwrap_or(0),
      tilewidth: self.num(node, "tilewidth")?.unwrap_or(0),
      tileheight: self.num(node, "tileheight")?.unwrap_or(0),
      class: self.class(node),
      backgroundcolor: self.str(node, "backgroundcolor").map(str::to_string),
      parallaxoriginx: self.num(node, "parallaxoriginx")?.unwrap_or(0.0),
      parallaxoriginy: self.num(node, "parallaxoriginy")?.unwrap_or(0.0),
      properties: self.properties(node)?,
      ..RawMap::default()
    };
    for child in node.children().filter(Node::is_element) {
      match child.tag_name().name() {
        "tileset" => map.tilesets.push(self.tileset_ref(child)?),
        "layer" | "objectgroup" | "imagelayer" | "group" => {
          map.layers.push(self.layer(child, 0)?);
        }
        _ => {}
      }
    }
    Ok(map)
  }

  fn template(&self, node: Node<'a, 'i>) -> Parsed<RawTemplate> {
    let Some(object) = self.child(node, "object") else {
      return fail(self.doc, node, "the template has no <object>");
    };
    Ok(RawTemplate {
      tileset: match self.child(node, "tileset") {
        Some(t) => Some(self.tileset_ref(t)?),
        None => None,
      },
      object: self.object(object)?,
    })
  }
}

/// The text inside an element, as written
fn text_of(node: Node<'_, '_>) -> String {
  node
    .descendants()
    .filter(Node::is_text)
    .filter_map(|n| n.text())
    .collect()
}

/// A float as the JSON file would write it: a whole one as an integer, which
/// is how Tiled's own JSON export writes `1.0`
fn json_number(f: f64) -> Option<Value> {
  if f.fract() == 0.0 && f.abs() < 9.0e15 {
    return Some(Value::from(f as i64));
  }
  Number::from_f64(f).map(Value::Number)
}

/// Parses `text`, whose root element must be `root`
fn read<T>(
  text: &str,
  root: &str,
  fill: impl for<'a, 'i> FnOnce(&Reader<'a, 'i>, Node<'a, 'i>) -> Parsed<T>,
) -> Parsed<T> {
  let text = text.strip_prefix('\u{feff}').unwrap_or(text);
  let doc = Document::parse(text).map_err(|e| {
    let pos = e.pos();
    format!("line {}, column {}: {e}", pos.row, pos.col)
  })?;
  let top = doc.root_element();
  if top.tag_name().name() != root {
    return fail(
      &doc,
      top,
      format!(
        "expected a <{root}> element, found <{}>",
        top.tag_name().name()
      ),
    );
  }
  fill(&Reader { doc: &doc }, top)
}

/// A TMX map
pub(crate) fn parse_tmx(text: &str) -> Parsed<RawMap> {
  read(text, "map", |r, node| r.map(node))
}

/// A TSX tileset
pub(crate) fn parse_tsx(text: &str) -> Parsed<RawTileset> {
  read(text, "tileset", |r, node| r.tileset(node))
}

/// A TX object template
pub(crate) fn parse_tx(text: &str) -> Parsed<RawTemplate> {
  read(text, "template", |r, node| r.template(node))
}

#[cfg(test)]
mod tests {
  use std::path::Path;

  use glam::{IVec2, UVec2, Vec2};

  use crate::core::asset::{
    Asset, AssetServer, Assets, AssetsView, Handle, MemorySource, insert_registry,
  };
  use crate::core::ecs::kernel::World;
  use crate::core::physics::CellRect;
  use crate::core::tilemap::{
    GridShape, ImageLayer, LayerKind, MapGrid, StaggerAxis, StaggerIndex, Tile, TileFlip, TileGrid,
    TileMap, Tileset, TilesetImage, TilesetSource,
  };

  const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/tilemap/tiled");

  fn read_tree(root: &Path, dir: &Path, files: &MemorySource) {
    let Ok(entries) = std::fs::read_dir(dir) else {
      panic!("{dir:?} is not readable")
    };
    for entry in entries.flatten() {
      let path = entry.path();
      if path.is_dir() {
        read_tree(root, &path, files);
      } else if let (Ok(rel), Ok(bytes)) = (path.strip_prefix(root), std::fs::read(&path)) {
        files.insert(&rel.to_string_lossy().replace('\\', "/"), bytes);
      }
    }
  }

  /// Every fixture, the XML ones under `xml/`, plus `extra` (path, text) files
  fn world_with(extra: &[(&str, &str)]) -> World {
    let files = MemorySource::new();
    read_tree(Path::new(FIXTURES), Path::new(FIXTURES), &files);
    for (path, text) in extra {
      files.insert(path, *text);
    }
    let mut world = World::new();
    insert_registry(&mut world, AssetServer::new(files));
    world
  }

  fn load_map(world: &mut World, path: &str) -> Result<TileMap, String> {
    let mut assets = AssetsView::new(world);
    let handle = assets.load_now::<TileMap>(path).map_err(|e| e.report())?;
    assets
      .get(&handle)
      .cloned()
      .ok_or_else(|| "not in the store".to_string())
  }

  fn load_set(world: &mut World, path: &str) -> Result<Tileset, String> {
    let mut assets = AssetsView::new(world);
    let handle = assets.load_now::<Tileset>(path).map_err(|e| e.report())?;
    assets
      .get(&handle)
      .cloned()
      .ok_or_else(|| "not in the store".to_string())
  }

  /// A file's path with what differs between a JSON file and its XML twin
  /// taken away: the `xml/` folder and the XML extensions
  fn twin_name(path: &str) -> String {
    let path = path.strip_prefix("xml/").unwrap_or(path);
    for (xml, json) in [(".tsx", ".tsj"), (".tx", ".tj"), (".tmx", ".tmj")] {
      if let Some(stem) = path.strip_suffix(xml) {
        return format!("{stem}{json}");
      }
    }
    path.to_string()
  }

  /// Debug text with every handle replaced by the path it loaded from, so two
  /// loads compare by what they point at
  struct Canon<'w> {
    world: &'w World,
    swaps: Vec<(String, String)>,
  }

  impl Canon<'_> {
    fn handle<T: Asset>(&mut self, handle: &Handle<T>) {
      let name = match self.world.resource::<AssetServer>().path(handle) {
        Some(path) => twin_name(path.as_str()),
        None => "(no path)".to_string(),
      };
      self
        .swaps
        .push((format!("{handle:?}"), format!("<{name}>")));
    }

    fn text(&self, mut text: String) -> String {
      for (from, to) in &self.swaps {
        text = text.replace(from, to);
      }
      text
    }

    fn set(&mut self, set: &Tileset) -> String {
      if let TilesetImage::Atlas { image, .. } = &set.image {
        self.handle(image);
      }
      let mut out = format!(
        "{} {:?} {:?} {} {} {} {} {:?} {:?} {:?} {:?}\n",
        set.name,
        set.class,
        set.tile_size,
        set.columns,
        set.tile_count,
        set.spacing,
        set.margin,
        set.tile_offset,
        set.image,
        set.wang_sets,
        set.properties
      );
      for id in 0..set.tile_count {
        if let Some(tile) = set.tile(id) {
          if let Some((image, _)) = &tile.image {
            self.handle(image);
          }
          out.push_str(&format!("tile {id}: {tile:?}\n"));
        }
      }
      self.text(out)
    }

    fn map(&mut self, map: &TileMap) -> String {
      let mut out = String::new();
      for source in &map.tilesets {
        match source {
          TilesetSource::Asset(handle) => {
            self.handle(handle);
            match self.world.resource::<Assets<Tileset>>().get(handle) {
              Some(set) => out.push_str(&self.set(set)),
              None => out.push_str("(tileset missing)\n"),
            }
          }
          TilesetSource::Embedded(set) => out.push_str(&self.set(set)),
        }
      }
      for layer in &map.layers {
        if let LayerKind::Image(ImageLayer {
          image: Some(image), ..
        }) = &layer.kind
        {
          self.handle(image);
        }
      }
      let tilesets: Vec<String> = map
        .tilesets
        .iter()
        .map(|s| match s {
          TilesetSource::Asset(h) => format!("{h:?}"),
          TilesetSource::Embedded(_) => "embedded".to_string(),
        })
        .collect();
      let mut rest = format!("{map:#?}");
      for t in tilesets {
        rest = rest.replace(&t, "tileset");
      }
      out.push_str(&self.text(rest));
      out
    }
  }

  fn canon_map(world: &World, map: &TileMap) -> String {
    Canon {
      world,
      swaps: Vec::new(),
    }
    .map(map)
  }

  fn canon_set(world: &World, set: &Tileset) -> String {
    Canon {
      world,
      swaps: Vec::new(),
    }
    .set(set)
  }

  /// Panics naming the first line where the two texts differ
  fn assert_same(name: &str, xml: &str, json: &str) {
    if xml == json {
      return;
    }
    for (n, (a, b)) in xml.lines().zip(json.lines()).enumerate() {
      assert_eq!(a, b, "{name}: line {n} differs (xml left, json right)");
    }
    panic!("{name}: one has more lines than the other");
  }

  const MAP_PAIRS: [&str; 50] = [
    "image-layers",
    "lists",
    "tile-objects",
    "mixed",
    "mixed-embedded",
    "nested",
    "objects",
    "enc-csv",
    "enc-base64",
    "enc-gzip",
    "enc-zlib",
    "enc-zstd",
    "level",
    "wang-dirt",
    "wang-grass",
    "infinite",
    "infinite-chunks8",
    "infinite-csv",
    "infinite-base64",
    "infinite-zlib",
    "infinite-gzip",
    "infinite-zstd",
    "shape-iso",
    "shape-iso-48x32",
    "shape-stag-x-odd",
    "shape-stag-x-even",
    "shape-stag-y-odd",
    "shape-stag-y-even",
    "shape-hex-x-odd",
    "shape-hex-x-even",
    "shape-hex-y-odd",
    "shape-hex-y-even",
    "shape-hex-x-side0",
    "shape-hex-y-side0",
    "shape-hex-long-side",
    "shape-oblique",
    "shape-oblique-neg-x",
    "shape-oblique-neg-y",
    "shape-odd-ortho",
    "shape-odd-iso",
    "shape-odd-stag-x-odd",
    "shape-odd-stag-y-even",
    "shape-odd-hex-x-odd",
    "shape-odd-hex-y-odd",
    "shape-odd-oblique",
    "shape-infinite-iso",
    "shape-infinite-stag",
    "shape-infinite-hex",
    "shape-infinite-oblique",
    "shape-hex-rotations",
  ];
  const SET_PAIRS: [&str; 9] = [
    "loot",
    "tiles",
    "items",
    "offset",
    "aligned",
    "terrain",
    "collision",
    "wang",
    "wang-turn",
  ];

  #[test]
  fn every_fixture_loads_the_same_from_xml_and_json() {
    let mut world = world_with(&[]);
    for name in MAP_PAIRS {
      let xml = load_map(&mut world, &format!("xml/maps/{name}.tmx"))
        .unwrap_or_else(|e| panic!("{name}.tmx: {e}"));
      let json = load_map(&mut world, &format!("maps/{name}.tmj"))
        .unwrap_or_else(|e| panic!("{name}.tmj: {e}"));
      assert_same(name, &canon_map(&world, &xml), &canon_map(&world, &json));
    }
    for name in SET_PAIRS {
      let xml = load_set(&mut world, &format!("xml/tilesets/{name}.tsx"))
        .unwrap_or_else(|e| panic!("{name}.tsx: {e}"));
      let json = load_set(&mut world, &format!("tilesets/{name}.tsj"))
        .unwrap_or_else(|e| panic!("{name}.tsj: {e}"));
      assert_same(name, &canon_set(&world, &xml), &canon_set(&world, &json));
    }
  }

  #[test]
  fn the_comparison_notices_a_difference() {
    let mut world = world_with(&[]);
    let a = load_map(&mut world, "xml/maps/mixed.tmx").expect("loads");
    let b = load_map(&mut world, "maps/nested.tmj").expect("loads");
    assert_ne!(canon_map(&world, &a), canon_map(&world, &b));
  }

  fn failure(text: &str) -> String {
    let mut world = world_with(&[("m.tmx", text)]);
    match load_map(&mut world, "m.tmx") {
      Ok(_) => panic!("{text} loaded"),
      Err(error) => error,
    }
  }

  const HEAD: &str =
    r#"<map orientation="orthogonal" width="2" height="2" tilewidth="16" tileheight="16">"#;

  /// An infinite TMX map of 16 px tiles holding one tile layer "ground"
  /// whose `<data>` (CSV) holds `chunks`
  fn infinite_tmx(chunks: &str) -> String {
    format!(
      "<map orientation=\"orthogonal\" width=\"0\" height=\"0\" tilewidth=\"16\" tileheight=\"16\" infinite=\"1\">\n <layer id=\"1\" name=\"ground\" width=\"0\" height=\"0\">\n  <data encoding=\"csv\">\n{chunks}\n  </data>\n </layer>\n</map>"
    )
  }

  #[test]
  fn a_tmx_chunk_that_does_not_state_its_place_or_size_fails_naming_it() {
    let full = "<chunk x=\"0\" y=\"0\" width=\"2\" height=\"2\">0,0,\n0,0</chunk>";
    let cases = [
      ("y=\"0\" width=\"2\" height=\"2\"", "chunk 1", "`x`"),
      ("x=\"0\" width=\"2\" height=\"2\"", "chunk 1", "`y`"),
      ("x=\"-4\" y=\"2\" height=\"2\"", "chunk (-4, 2)", "`width`"),
      ("x=\"-4\" y=\"2\" width=\"2\"", "chunk (-4, 2)", "`height`"),
    ];
    for (attributes, at, what) in cases {
      let chunks = format!("{full}\n<chunk {attributes}>0,0,\n0,0</chunk>");
      let error = failure(&infinite_tmx(&chunks));
      assert!(
        error.contains("m.tmx")
          && error.contains(r#"layer "ground""#)
          && error.contains(at)
          && error.contains(what)
          && error.contains("does not state"),
        "{attributes}: {error}"
      );
    }
  }

  #[test]
  fn hostile_tmx_chunks_are_refused_naming_their_place() {
    // The TMX reader's own path to the chunks the JSON's hostile tests
    // cover: each fails its load naming the file and the place, never a
    // panic or an allocation of what the chunk claims
    let refused = |chunk: &str, wanted: &[&str]| {
      let error = failure(&infinite_tmx(chunk));
      assert!(
        error.contains("m.tmx") && wanted.iter().all(|w| error.contains(w)),
        "{chunk}: {error}"
      );
    };
    // CSV shorter than the chunk's cells
    refused(
      "<chunk x=\"0\" y=\"16\" width=\"2\" height=\"2\">0,0,\n0</chunk>",
      &[
        r#"layer "ground""#,
        "chunk (0, 16)",
        "has 3 cells, expected 4",
      ],
    );
    // A negative width
    refused(
      "<chunk x=\"16\" y=\"0\" width=\"-16\" height=\"16\">0</chunk>",
      &[r#"layer "ground""#, "chunk (16, 0)", "-16 x 16"],
    );
    // An x past what an i64 holds: the reader names its line and column
    refused(
      "<chunk x=\"9223372036854775808\" y=\"0\" width=\"1\" height=\"1\">0</chunk>",
      &["line 4, column", "x"],
    );
  }

  /// A `shape-*` fixture's name, grid, tile size and size in cells
  type ShapeFixture = (&'static str, GridShape, (u32, u32), (u32, u32));

  /// Each bounded `shape-*` fixture
  fn shape_fixtures() -> Vec<ShapeFixture> {
    use StaggerAxis::{X, Y};
    use StaggerIndex::{Even, Odd};
    let stag = |axis, index| GridShape::Staggered { axis, index };
    let hex = |axis, index, side| GridShape::Hexagonal { axis, index, side };
    let oblique = |x, y| GridShape::Oblique {
      skew: IVec2::new(x, y),
    };
    vec![
      ("shape-iso", GridShape::Isometric, (64, 32), (6, 4)),
      ("shape-iso-48x32", GridShape::Isometric, (48, 32), (6, 4)),
      ("shape-stag-x-odd", stag(X, Odd), (64, 32), (6, 6)),
      ("shape-stag-x-even", stag(X, Even), (64, 32), (6, 6)),
      ("shape-stag-y-odd", stag(Y, Odd), (64, 32), (6, 6)),
      ("shape-stag-y-even", stag(Y, Even), (64, 32), (6, 6)),
      ("shape-hex-x-odd", hex(X, Odd, 12), (32, 32), (6, 6)),
      ("shape-hex-x-even", hex(X, Even, 12), (32, 32), (6, 6)),
      ("shape-hex-y-odd", hex(Y, Odd, 12), (32, 32), (6, 6)),
      ("shape-hex-y-even", hex(Y, Even, 12), (32, 32), (6, 6)),
      ("shape-hex-x-side0", hex(X, Even, 0), (32, 32), (6, 6)),
      ("shape-hex-y-side0", hex(Y, Odd, 0), (32, 32), (6, 6)),
      ("shape-hex-long-side", hex(Y, Odd, 40), (32, 32), (6, 6)),
      ("shape-oblique", oblique(8, 12), (32, 32), (6, 6)),
      ("shape-oblique-neg-x", oblique(-10, 0), (32, 32), (6, 6)),
      ("shape-oblique-neg-y", oblique(0, -12), (32, 32), (6, 6)),
      ("shape-odd-ortho", GridShape::Orthogonal, (33, 17), (6, 6)),
      ("shape-odd-iso", GridShape::Isometric, (33, 17), (6, 4)),
      ("shape-odd-stag-x-odd", stag(X, Odd), (33, 17), (6, 6)),
      ("shape-odd-stag-y-even", stag(Y, Even), (33, 17), (6, 6)),
      ("shape-odd-hex-x-odd", hex(X, Odd, 13), (33, 17), (6, 6)),
      ("shape-odd-hex-y-odd", hex(Y, Odd, 13), (33, 17), (6, 6)),
      ("shape-odd-oblique", oblique(7, -5), (33, 17), (6, 6)),
    ]
  }

  #[test]
  fn every_grid_loads_in_both_formats_with_its_parameters() {
    let mut world = world_with(&[]);
    for (name, shape, (tw, th), (w, h)) in shape_fixtures() {
      for path in [format!("maps/{name}.tmj"), format!("xml/maps/{name}.tmx")] {
        let map = loaded(&mut world, &path);
        assert_eq!(map.shape, shape, "{path}");
        assert_eq!(map.tile_size, UVec2::new(tw, th), "{path}");
        assert_eq!(
          (map.size, map.infinite),
          (UVec2::new(w, h), false),
          "{path}"
        );
        assert_eq!(
          map.grid(),
          MapGrid::for_map(shape, map.tile_size, map.size, false),
          "{path}"
        );
        // Every tile layer is on the map's grid
        for layer in &map.layers {
          if let LayerKind::Tiles { grid, .. } = &layer.kind {
            assert_eq!(*grid, map.grid(), "{path}: {}", layer.info.name);
          }
        }
        // Tile (x + 2y) mod 4 at every cell but the outer ring
        let ground = grid_of(&map, "ground");
        for y in 0..h as i32 {
          for x in 0..w as i32 {
            let inside = x > 0 && y > 0 && x < w as i32 - 1 && y < h as i32 - 1;
            let wanted = inside.then(|| Tile::new(0, (x + 2 * y).rem_euclid(4) as u32));
            assert_eq!(ground.get(IVec2::new(x, y)), wanted, "{path}: ({x}, {y})");
          }
        }
      }
    }
  }

  #[test]
  fn tmx_grid_values_load_and_unknown_ones_fail_naming_them() {
    let map_with = |attributes: &str| {
      format!(
        "<map {attributes} width=\"1\" height=\"1\" tilewidth=\"16\" tileheight=\"16\"></map>"
      )
    };
    let refused = [
      ("orientation=\"hexagon\"", "\"hexagon\""),
      (
        "orientation=\"staggered\" staggeraxis=\"z\"",
        "staggeraxis \"z\"",
      ),
      (
        "orientation=\"hexagonal\" staggerindex=\"middle\"",
        "staggerindex \"middle\"",
      ),
      (
        "orientation=\"hexagonal\" hexsidelength=\"-5\"",
        "hexsidelength -5",
      ),
      (
        "orientation=\"oblique\" skewy=\"3000000000\"",
        "skewy 3000000000",
      ),
    ];
    for (attributes, named) in refused {
      let error = failure(&map_with(attributes));
      assert!(error.contains("m.tmx") && error.contains(named), "{error}");
    }
    let mut world = world_with(&[(
      "m.tmx",
      &map_with(
        "orientation=\"hexagonal\" staggeraxis=\"x\" staggerindex=\"even\" hexsidelength=\"7\"",
      ),
    )]);
    let map = loaded(&mut world, "m.tmx");
    assert_eq!(
      map.shape,
      GridShape::Hexagonal {
        axis: StaggerAxis::X,
        index: StaggerIndex::Even,
        side: 7
      }
    );
    let mut world = world_with(&[("m.tmx", &map_with("orientation=\"oblique\" skewx=\"-10\""))]);
    let map = loaded(&mut world, "m.tmx");
    assert_eq!(
      map.shape,
      GridShape::Oblique {
        skew: IVec2::new(-10, 0)
      }
    );
    // A hexagonal layer that collides is refused from TMX as from JSON
    let mut world = world_with(&[]);
    let error = load_map(&mut world, "xml/maps/shape-hex-collides.tmx")
      .err()
      .unwrap_or_default();
    assert!(
      error.contains("shape-hex-collides.tmx")
        && error.contains("layer \"walls\" on a hexagonal map collides or casts shadows"),
      "{error}"
    );
  }

  #[test]
  fn a_broken_tmx_names_its_line_and_column() {
    let error = failure(&format!("{HEAD}\n <layer id=\"1\" name=\"a\"\n</map>"));
    assert!(error.contains("line 3, column"), "{error}");
  }

  #[test]
  fn a_bad_attribute_names_where_it_is() {
    let error = failure(&format!(
      "{HEAD}\n <layer id=\"1\" name=\"a\" width=\"two\" height=\"2\"/>\n</map>"
    ));
    assert!(
      error.contains("line 2, column") && error.contains("width"),
      "{error}"
    );
  }

  // ---- infinite maps ------------------------------------------------------

  /// The grid of `map`'s tile layer `name`
  fn grid_of<'a>(map: &'a TileMap, name: &str) -> &'a TileGrid {
    match map.layer_index(name).and_then(|index| map.tiles(index)) {
      Some(grid) => grid,
      None => panic!("no tile layer {name}"),
    }
  }

  /// Every tile of `grid` with its cell, row by row
  fn tiles_of(grid: &TileGrid) -> Vec<(IVec2, Tile)> {
    let extent = grid.extent();
    (extent.min.y..extent.max.y)
      .flat_map(|y| (extent.min.x..extent.max.x).map(move |x| IVec2::new(x, y)))
      .filter_map(|cell| Some((cell, grid.get(cell)?)))
      .collect()
  }

  /// What `make-fixtures.js` painted into each tile layer of `infinite`,
  /// row by row (the ground layer is every infinite fixture's)
  fn painted(layer: &str) -> Vec<(IVec2, Tile)> {
    let (h, v, d) = (TileFlip::HORIZONTAL, TileFlip::VERTICAL, TileFlip::DIAGONAL);
    let tile = |x: i32, y: i32, id: u32, flip: TileFlip| {
      (IVec2::new(x, y), Tile::new(0, id).with_flip(flip))
    };
    let mut tiles = match layer {
      "ground" => vec![
        tile(-20, -3, 0, TileFlip::NONE),
        tile(-1, -1, 1, h),
        tile(0, 0, 2, TileFlip::NONE),
        tile(5, 2, 3, h | v | d),
        tile(40, 10, 5, TileFlip::NONE),
        tile(-17, 18, 6, v),
      ],
      "walls" => vec![
        tile(-50, 20, 7, TileFlip::NONE),
        tile(17, -17, 8, TileFlip::NONE),
        tile(18, -17, 8, TileFlip::NONE),
      ],
      "decor" => vec![tile(3, 3, 9, TileFlip::NONE)],
      _ => panic!("no layer {layer} was painted"),
    };
    tiles.sort_by_key(|(cell, _)| (cell.y, cell.x));
    tiles
  }

  fn loaded(world: &mut World, path: &str) -> TileMap {
    load_map(world, path).unwrap_or_else(|e| panic!("{path}: {e}"))
  }

  #[test]
  fn an_infinite_map_s_chunks_land_where_tiled_put_them() {
    let mut world = world_with(&[]);
    let json = loaded(&mut world, "maps/infinite.tmj");
    let xml = loaded(&mut world, "xml/maps/infinite.tmx");
    for (map, format) in [(&json, "tmj"), (&xml, "tmx")] {
      assert!(map.infinite, "{format}");
      assert_eq!(map.size, UVec2::ZERO, "{format}");
      let mut union = CellRect::default();
      for layer in ["ground", "walls", "decor"] {
        let grid = grid_of(map, layer);
        assert_eq!(grid.bounds(), None, "{format} {layer} is unbounded");
        // every tile the script placed, at its cell, and no other
        assert_eq!(tiles_of(grid), painted(layer), "{format} {layer}");
        union = union.union(grid.tile_extent());
      }
      // the engine's chunks of 32 hold Tiled's chunks of 16
      let chunks = |layer: &str| grid_of(map, layer).chunk_coords().collect::<Vec<_>>();
      let at = |x: i32, y: i32| IVec2::new(x, y);
      assert_eq!(
        chunks("ground"),
        [at(-1, -1), at(-1, 0), at(0, 0), at(1, 0)],
        "{format}"
      );
      assert_eq!(chunks("walls"), [at(0, -1), at(-2, 0)], "{format}");
      // decor's tile at (70, -40), written and erased, left an empty chunk
      // Tiled did not save: nothing of it is made
      assert_eq!(chunks("decor"), [at(0, 0)], "{format}");
      assert!(!grid_of(map, "decor").has_chunk(TileGrid::chunk_of(at(70, -40))));
      // the map's bounds hold every layer's tiles, exactly
      assert_eq!(map.bounds(), union, "{format}");
      assert_eq!(
        map.bounds(),
        CellRect::new(at(-50, -17), at(41, 21)),
        "{format}"
      );
      // objects keep their negative coordinates
      let LayerKind::Objects(objects) = &map.layers[3].kind else {
        panic!("{format}: no object layer")
      };
      let placed: Vec<(&str, Vec2)> = objects
        .iter()
        .map(|o| (o.name.as_str(), o.position))
        .collect();
      assert_eq!(
        placed,
        [
          ("crate", Vec2::new(-40.0, -24.0)),
          ("spot", Vec2::new(-8.5, -100.0))
        ],
        "{format}"
      );
    }
    assert_same(
      "infinite",
      &canon_map(&world, &xml),
      &canon_map(&world, &json),
    );
  }

  #[test]
  fn chunks_of_any_size_and_every_encoding_load() {
    let mut world = world_with(&[]);
    let reference = painted("ground");
    let mut paths = vec!["xml/maps/infinite-xml.tmx".to_string()];
    for name in [
      "infinite-chunks8",
      "infinite-csv",
      "infinite-base64",
      "infinite-zlib",
      "infinite-gzip",
      "infinite-zstd",
    ] {
      paths.push(format!("maps/{name}.tmj"));
      paths.push(format!("xml/maps/{name}.tmx"));
    }
    let first = loaded(&mut world, "maps/infinite-csv.tmj");
    let chunks: Vec<IVec2> = grid_of(&first, "ground").chunk_coords().collect();
    for path in paths {
      let map = loaded(&mut world, &path);
      assert!(map.infinite, "{path}");
      let grid = grid_of(&map, "ground");
      assert_eq!(grid.bounds(), None, "{path}");
      assert_eq!(tiles_of(grid), reference, "{path}");
      assert_eq!(grid.chunk_coords().collect::<Vec<_>>(), chunks, "{path}");
    }
  }

  #[test]
  fn chunks_straddling_the_engine_s_chunks_and_a_grouped_layer_land_where_tiled_put_them() {
    // Tiled's chunks of 24 x 24 (`make-fixtures.js`, `chunks24`) straddle
    // the engine's borders at x 32 and -32, and a tile layer sits inside a
    // group: every tile at the cell the script painted, in both formats
    let mut world = world_with(&[]);
    let (h, v) = (TileFlip::HORIZONTAL, TileFlip::VERTICAL);
    let tile = |x: i32, y: i32, id: u32, flip: TileFlip| {
      (IVec2::new(x, y), Tile::new(0, id).with_flip(flip))
    };
    let mut ground = painted("ground");
    ground.extend([
      tile(30, 5, 4, TileFlip::NONE),
      tile(33, 5, 7, TileFlip::NONE),
      tile(-33, -25, 8, TileFlip::NONE),
      tile(-31, -25, 9, h),
    ]);
    ground.sort_by_key(|(cell, _)| (cell.y, cell.x));
    let inner = vec![
      tile(31, -49, 12, v),
      tile(32, -49, 13, TileFlip::NONE),
      tile(-40, 47, 11, TileFlip::NONE),
    ];
    let at = |x: i32, y: i32| IVec2::new(x, y);
    for path in [
      "maps/infinite-chunks24.tmj",
      "xml/maps/infinite-chunks24.tmx",
    ] {
      let map = loaded(&mut world, path);
      assert!(map.infinite, "{path}");
      // the group flattens: its tile layer is the map's second
      assert_eq!(map.layer_index("inner"), Some(1), "{path}");
      for (name, expected) in [("ground", &ground), ("inner", &inner)] {
        let grid = grid_of(&map, name);
        assert_eq!(grid.bounds(), None, "{path} {name}");
        assert_eq!(tiles_of(grid), *expected, "{path} {name}");
      }
      // a straddling chunk's tiles in the engine chunks either side
      assert_eq!(
        grid_of(&map, "ground").chunk_coords().collect::<Vec<_>>(),
        [at(-2, -1), at(-1, -1), at(-1, 0), at(0, 0), at(1, 0)],
        "{path}"
      );
      assert_eq!(
        grid_of(&map, "inner").chunk_coords().collect::<Vec<_>>(),
        [at(0, -2), at(1, -2), at(-2, 1)],
        "{path}"
      );
      assert_eq!(
        map.bounds(),
        CellRect::new(at(-40, -49), at(41, 48)),
        "{path}"
      );
    }
  }

  #[test]
  fn tile_elements_without_an_encoding_fill_a_layer() {
    let tiles = r#"<tile gid="0"/><tile gid="0"/><tile gid="0"/><tile gid="0"/>"#;
    let mut world = world_with(&[(
      "m.tmx",
      &format!(
        "{HEAD}<tileset firstgid=\"1\" source=\"tilesets/tiles.tsx\"/><layer id=\"1\" name=\"a\" width=\"2\" height=\"2\"><data>{tiles}</data></layer></map>"
      ),
    )]);
    // the fixture tileset lives under xml/, so this one is not found
    let error = load_map(&mut world, "m.tmx").expect_err("loaded");
    assert!(error.contains("tilesets/tiles.tsx"), "{error}");
    let mut world = world_with(&[(
      "m.tmx",
      &format!(
        "{HEAD}<layer id=\"1\" name=\"a\" width=\"2\" height=\"2\"><data>{tiles}</data></layer></map>"
      ),
    )]);
    assert!(load_map(&mut world, "m.tmx").is_ok());
  }

  #[test]
  fn a_multi_line_string_property_keeps_its_lines() {
    let mut world = world_with(&[(
      "m.tmx",
      &format!(
        "{HEAD}<properties><property name=\"note\">first\nsecond</property><property name=\"one\" value=\"x\"/></properties></map>"
      ),
    )]);
    let map = load_map(&mut world, "m.tmx").expect("loads");
    assert_eq!(
      map.properties.get("note"),
      Some(&crate::core::tilemap::PropertyValue::String(
        "first\nsecond".into()
      ))
    );
  }

  #[test]
  fn group_nesting_is_bounded() {
    let depth = 200;
    let mut text = HEAD.to_string();
    for _ in 0..depth {
      text.push_str(r#"<group id="1" name="g">"#);
    }
    for _ in 0..depth {
      text.push_str("</group>");
    }
    text.push_str("</map>");
    let error = failure(&text);
    assert!(error.contains("nested too deep"), "{error}");
  }

  #[test]
  fn an_unknown_root_names_what_it_expected() {
    let error = failure("<tileset name=\"x\"/>");
    assert!(
      error.contains("line 1, column 1") && error.contains("<map>"),
      "{error}"
    );
  }

  #[test]
  fn a_broken_template_names_its_file_and_line() {
    let object = r#"{"type":"map","orientation":"orthogonal","width":1,"height":1,"tilewidth":16,"tileheight":16,"tilesets":[],"layers":[{"id":1,"name":"o","type":"objectgroup","objects":[{"id":1,"template":"t.tx","x":0,"y":0}]}]}"#;
    let mut world = world_with(&[
      ("m.tmj", object),
      (
        "t.tx",
        "<template>\n <object id=\"1\" x=\"q\"/>\n</template>",
      ),
    ]);
    let error = load_map(&mut world, "m.tmj").expect_err("loaded");
    assert!(
      error.contains("t.tx") && error.contains("line 2, column") && error.contains("object 1"),
      "{error}"
    );
  }
}
