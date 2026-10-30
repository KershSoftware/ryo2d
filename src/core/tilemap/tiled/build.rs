//! From what a Tiled file says ([`raw`](super::raw)) to a [`TileMap`] or a
//! [`Tileset`], and the checks that need the loaded images and tilesets.

use std::fmt::Display;
use std::sync::Arc;
use std::time::Duration;

use super::data::{decode_gid, decode_layer_data};
use super::objects::{Alignment, ObjectState, TilesetEntry, build_objects};
use super::project::{Props, parse_color};
use super::raw::{RawLayer, RawMap, RawObject, RawTile, RawTileset};
use super::wang::build_wang_sets;
use crate::core::asset::{
  Asset, AssetError, AssetPath, AssetServer, Assets, CheckContext, Handle, LoadContext,
};
use crate::core::graphics::Image;
use crate::core::render::batch::BlendMode;
use crate::core::tilemap::{
  ImageLayer, LayerKind, MapLayer, Tile, TileData, TileFrame, TileGrid, TileMap, TileShape,
  Tileset, TilesetImage, TilesetSource,
};
use crate::core::ui::Color;
use glam::{IVec2, UVec2, Vec2};

/// Most cells a layer or a map holds (8192 x 8192), as for `TileColliders`
const MAX_CELLS: u64 = 1 << 26;
/// Most tiles a tileset holds: what a cell's 20 bits can name
pub(super) const MAX_TILES: u32 = 1_048_575;
const MAX_TILESETS: usize = 256;
const MAX_DEPTH: usize = 64;
/// Points an ellipse is cut into
const ELLIPSE_POINTS: usize = 16;

pub(super) fn fail(path: &AssetPath, place: &str, what: impl Display) -> AssetError {
  AssetError::decode(path, format!("{place}: {what}"))
}

/// `written`, a path in the file at `path`, as a path from the asset root
pub(super) fn join(path: &AssetPath, place: &str, written: &str) -> Result<String, AssetError> {
  if written.starts_with(['/', '\\']) {
    return Err(fail(
      path,
      place,
      format!("{written:?} is an absolute path"),
    ));
  }
  Ok(match path.parent() {
    Some(folder) => format!("{folder}/{written}"),
    None => written.to_string(),
  })
}

/// Loads the file `written` names as a dependency of the one being loaded
fn load_dep<T: Asset>(
  cx: &mut LoadContext<'_>,
  path: &AssetPath,
  place: &str,
  written: &str,
) -> Result<Handle<T>, AssetError> {
  let joined = join(path, place, written)?;
  cx.load::<T>(&joined).map_err(|error| match error {
    AssetError::Dependency { .. } => error,
    refused => fail(path, place, format!("{written:?}: {}", refused.report())),
  })
}

fn multiply(a: Color, b: Color) -> Color {
  let m = |x: u8, y: u8| ((x as u16 * y as u16 + 127) / 255) as u8;
  Color::new(m(a.r, b.r), m(a.g, b.g), m(a.b, b.b), m(a.a, b.a))
}

/// A cell count that is within the limit
fn cell_count(w: u32, h: u32) -> Option<usize> {
  // each side too: a zero side would let the other be anything
  if u64::from(w) > MAX_CELLS || u64::from(h) > MAX_CELLS {
    return None;
  }
  let n = u64::from(w) * u64::from(h);
  (n <= MAX_CELLS).then_some(n as usize)
}

// ---- tilesets -------------------------------------------------------------

/// The polygon of a collision object, in tile pixels; `None` for objects that
/// are no closed shape (points, polylines, text, tile objects)
fn collision_shape(
  px: &mut Props<'_>,
  place: &str,
  o: &RawObject,
) -> Result<Option<TileShape>, AssetError> {
  if o.point || o.capsule || o.polyline.is_some() || o.text.is_some() || o.gid.is_some() {
    return Ok(None);
  }
  let (width, height) = (o.width.unwrap_or(0.0), o.height.unwrap_or(0.0));
  let place = format!("{place}, object {}", o.id);
  // points relative to the object's position, before it is turned
  let local: Vec<(f64, f64)> = if let Some(points) = &o.polygon {
    if points.len() < 3 {
      return Err(px.fail(&place, "a polygon needs 3 points or more"));
    }
    points.iter().map(|p| (p.x, p.y)).collect()
  } else if o.ellipse {
    (0..ELLIPSE_POINTS)
      .map(|i| {
        let t = std::f64::consts::TAU * i as f64 / ELLIPSE_POINTS as f64;
        (
          width / 2.0 * (1.0 + t.cos()),
          height / 2.0 * (1.0 + t.sin()),
        )
      })
      .collect()
  } else {
    if width <= 0.0 || height <= 0.0 {
      return Ok(None);
    }
    vec![(0.0, 0.0), (width, 0.0), (width, height), (0.0, height)]
  };
  let (sin, cos) = o.rotation.unwrap_or(0.0).to_radians().sin_cos();
  let (ox, oy) = (o.x.unwrap_or(0.0), o.y.unwrap_or(0.0));
  let points = local
    .into_iter()
    .map(|(x, y)| {
      // clockwise on screen, with y down
      Vec2::new(
        (ox + x * cos - y * sin) as f32,
        (oy + x * sin + y * cos) as f32,
      )
    })
    .collect();
  let properties = px.read(&place, &o.properties, o.class.as_deref())?;
  Ok(Some(TileShape {
    points,
    one_way: properties.bool("one_way").unwrap_or(false),
    occluder: properties.bool("occluder").unwrap_or(false),
    properties,
  }))
}

fn tile_data(
  cx: &mut LoadContext<'_>,
  px: &mut Props<'_>,
  path: &AssetPath,
  place: &str,
  raw: RawTile,
) -> Result<TileData, AssetError> {
  let mut data = TileData {
    class: raw.class.clone().filter(|c| !c.is_empty()),
    properties: px.read(place, &raw.properties, raw.class.as_deref())?,
    probability: raw.probability.unwrap_or(1.0),
    ..TileData::default()
  };
  if let Some(group) = &raw.objectgroup {
    for object in &group.objects {
      if let Some(shape) = collision_shape(px, place, object)? {
        data.shapes.push(shape);
      }
    }
  }
  for frame in &raw.animation {
    if frame.tileid >= MAX_TILES {
      return Err(fail(
        path,
        place,
        format!(
          "animation frame {} is past the {MAX_TILES} tiles a tileset holds",
          frame.tileid
        ),
      ));
    }
    data.animation.push(TileFrame {
      tile: frame.tileid,
      duration: Duration::from_millis(u64::from(frame.duration)),
    });
  }
  if let Some(written) = &raw.image {
    let handle = load_dep::<Image>(cx, path, place, written)?;
    data.image = Some((handle, UVec2::new(raw.imagewidth, raw.imageheight)));
  }
  Ok(data)
}

/// A tileset from what its file says. Images load as dependencies of the file
/// being loaded, from the folder it is in
pub(crate) fn build_tileset(
  raw: RawTileset,
  cx: &mut LoadContext<'_>,
  px: &mut Props<'_>,
) -> Result<Tileset, AssetError> {
  let path = cx.path().clone();
  let place = format!("tileset {:?}", raw.name);
  if raw.tilewidth == 0 || raw.tileheight == 0 {
    return Err(fail(&path, &place, "the tile size is 0"));
  }
  let tile_size = UVec2::new(raw.tilewidth, raw.tileheight);
  let mut columns = raw.columns;
  let mut computed_count = 0;
  let image = match &raw.image {
    Some(written) => {
      let size = UVec2::new(raw.imagewidth, raw.imageheight);
      if size.x == 0 || size.y == 0 {
        return Err(fail(
          &path,
          &place,
          "the image has no size (imagewidth, imageheight)",
        ));
      }
      let image = load_dep::<Image>(cx, &path, &place, written)?;
      // how many tiles the image holds, if the file does not say
      let fit = |extent: u32, tile: u32| {
        extent
          .saturating_sub(raw.margin.saturating_mul(2))
          .saturating_add(raw.spacing)
          / tile.saturating_add(raw.spacing).max(1)
      };
      if columns == 0 {
        columns = fit(size.x, tile_size.x);
      }
      computed_count = columns.saturating_mul(fit(size.y, tile_size.y));
      TilesetImage::Atlas { image, size }
    }
    None => TilesetImage::Collection,
  };
  let mut set = Tileset::new(raw.name.clone(), tile_size, image);
  set.class = raw.class.clone().filter(|c| !c.is_empty());
  set.spacing = raw.spacing;
  set.margin = raw.margin;
  set.columns = columns;
  set.tile_offset = IVec2::new(raw.tileoffset.x, raw.tileoffset.y);
  let declared = raw.tilecount.unwrap_or(computed_count);
  if declared > MAX_TILES {
    return Err(fail(
      &path,
      &place,
      format!("tilecount {declared} is past the {MAX_TILES} tiles a tileset holds"),
    ));
  }
  let mut count = declared;
  set.properties = px.read(&place, &raw.properties, raw.class.as_deref())?;
  set.wang_sets = build_wang_sets(px, &place, raw.wangsets)?;
  for tile in raw.tiles {
    let id = tile.id;
    let tile_place = format!("{place}, tile {id}");
    if id >= MAX_TILES {
      return Err(fail(
        &path,
        &tile_place,
        format!("the id is past the {MAX_TILES} tiles a tileset holds"),
      ));
    }
    // a collection's ids may skip, and its gid range runs to its highest
    count = count.max(id + 1);
    let data = tile_data(cx, px, &path, &tile_place, tile)?;
    set.insert_tile(id, data);
  }
  set.tile_count = count;
  Ok(set)
}

// ---- maps -----------------------------------------------------------------

/// What a layer takes from the groups around it
#[derive(Clone, Copy)]
struct Inherit {
  offset: Vec2,
  opacity: f32,
  tint: Color,
  parallax: Vec2,
  visible: bool,
  blend: BlendMode,
}

struct Builder<'p, 'a> {
  px: &'p mut Props<'a>,
  objects: ObjectState,
  path: AssetPath,
  size: UVec2,
  /// Each tileset's first gid and its index, by first gid
  first_gids: Vec<(u32, u16)>,
  /// The id for a layer the file gave none; `None` once the ids ran out
  next_id: Option<u32>,
  layers: Vec<MapLayer>,
  used_ids: Vec<(usize, u16, u32)>,
}

fn max_layer_id(layers: &[RawLayer]) -> u32 {
  layers
    .iter()
    .map(|l| l.id.unwrap_or(0).max(max_layer_id(&l.layers)))
    .max()
    .unwrap_or(0)
}

fn blend_of(path: &AssetPath, place: &str, mode: Option<&str>) -> Result<BlendMode, AssetError> {
  match mode {
    None | Some("" | "normal") => Ok(BlendMode::Alpha),
    Some("add") => Ok(BlendMode::Additive),
    Some("multiply") => Ok(BlendMode::Multiply),
    Some("screen") => Ok(BlendMode::Screen),
    Some(other) => Err(fail(
      path,
      place,
      format!("the blend mode {other:?} is not supported (normal, add, multiply and screen are)"),
    )),
  }
}

impl Builder<'_, '_> {
  fn layer_id(&mut self, raw: &RawLayer, place: &str) -> Result<u32, AssetError> {
    if let Some(id) = raw.id {
      return Ok(id);
    }
    let id = self.next_id.ok_or_else(|| {
      fail(
        &self.path,
        place,
        "the layer ids ran out: no id is left for a layer without one",
      )
    })?;
    self.next_id = id.checked_add(1);
    Ok(id)
  }

  fn layers(
    &mut self,
    raw: Vec<RawLayer>,
    inherit: Inherit,
    depth: usize,
    cx: &mut LoadContext<'_>,
  ) -> Result<(), AssetError> {
    for layer in raw {
      self.layer(layer, inherit, depth, cx)?;
    }
    Ok(())
  }

  fn layer(
    &mut self,
    mut raw: RawLayer,
    inherit: Inherit,
    depth: usize,
    cx: &mut LoadContext<'_>,
  ) -> Result<(), AssetError> {
    let path = self.path.clone();
    let place = format!("layer {:?}", raw.name);
    if depth > MAX_DEPTH {
      return Err(fail(&path, &place, "groups are nested too deep"));
    }
    let own_blend = blend_of(&path, &place, raw.mode.as_deref())?;
    let tint = match raw.tintcolor.as_deref().filter(|t| !t.is_empty()) {
      Some(text) => parse_color(text)
        .ok_or_else(|| fail(&path, &place, format!("tintcolor {text:?} is not a color")))?,
      None => Color::WHITE,
    };
    let here = Inherit {
      offset: inherit.offset + Vec2::new(raw.offsetx, raw.offsety),
      opacity: inherit.opacity * raw.opacity.unwrap_or(1.0),
      tint: multiply(inherit.tint, tint),
      parallax: inherit.parallax
        * Vec2::new(raw.parallaxx.unwrap_or(1.0), raw.parallaxy.unwrap_or(1.0)),
      visible: inherit.visible && raw.visible.unwrap_or(true),
      blend: if own_blend == BlendMode::Alpha {
        inherit.blend
      } else {
        own_blend
      },
    };
    if raw.ty == "group" {
      return self.layers(raw.layers, here, depth + 1, cx);
    }
    let id = self.layer_id(&raw, &place)?;
    let props = self
      .px
      .read(&place, &raw.properties, raw.class.as_deref())?;
    let is_tiles = raw.ty == "tilelayer";
    let mut used = Vec::new();
    let kind = match raw.ty.as_str() {
      "tilelayer" => {
        let (kind, highest) = self.tile_layer(&raw, &place)?;
        used = highest;
        kind
      }
      "objectgroup" => {
        let mut highest = vec![None; self.first_gids.len()];
        let objects = build_objects(
          &mut self.objects,
          &self.first_gids,
          cx,
          self.px,
          &place,
          std::mem::take(&mut raw.objects),
          &mut highest,
        )?;
        used = highest
          .into_iter()
          .enumerate()
          .filter_map(|(tileset, max)| Some((tileset as u16, max?)))
          .collect();
        LayerKind::Objects(objects)
      }
      "imagelayer" => {
        let Some(written) = raw.image.as_deref().filter(|i| !i.is_empty()) else {
          return Err(fail(&path, &place, "the image layer has no image"));
        };
        LayerKind::Image(ImageLayer {
          image: load_dep::<Image>(cx, &path, &place, written)?,
          repeat_x: raw.repeatx,
          repeat_y: raw.repeaty,
        })
      }
      other => return Err(fail(&path, &place, format!("unknown layer type {other:?}"))),
    };
    let mut info = crate::core::tilemap::LayerInfo::new(id, raw.name);
    info.class = raw.class.filter(|c| !c.is_empty());
    info.offset = here.offset;
    info.opacity = here.opacity;
    info.tint = here.tint;
    info.parallax = here.parallax;
    info.visible = here.visible;
    info.blend = here.blend;
    info.collides = props.bool("collides").unwrap_or(is_tiles);
    info.occludes = props.bool("occludes");
    info.y_sorted = props.bool("y_sort").unwrap_or(false);
    info.lit = props.bool("lit").unwrap_or(true);
    info.properties = props;
    let index = self.layers.len();
    self
      .used_ids
      .extend(used.into_iter().map(|(tileset, max)| (index, tileset, max)));
    self.layers.push(MapLayer { info, kind });
    Ok(())
  }

  /// The layer's tiles, and the highest id it uses of each tileset
  fn tile_layer(
    &mut self,
    raw: &RawLayer,
    place: &str,
  ) -> Result<(LayerKind, Vec<(u16, u32)>), AssetError> {
    let path = self.path.clone();
    if raw.chunks.is_some() {
      return Err(fail(&path, place, "infinite maps arrive in sub-project 6b"));
    }
    let (w, h) = (
      raw.width.unwrap_or(self.size.x),
      raw.height.unwrap_or(self.size.y),
    );
    let Some(expected) = cell_count(w, h) else {
      return Err(fail(
        &path,
        place,
        format!("{w} x {h} cells is past the {MAX_CELLS} cells a layer holds"),
      ));
    };
    let Some(data) = &raw.data else {
      return Err(fail(&path, place, "the layer has no data"));
    };
    let gids = decode_layer_data(
      data,
      raw.encoding.as_deref(),
      raw.compression.as_deref(),
      expected,
    )
    .map_err(|e| fail(&path, place, e))?;
    let mut grid = TileGrid::new(UVec2::new(w, h));
    let mut highest: Vec<Option<u32>> = vec![None; self.first_gids.len()];
    let width = (w as usize).max(1);
    for (i, &raw_gid) in gids.iter().enumerate() {
      let (gid, flip) = decode_gid(raw_gid);
      if gid == 0 {
        continue;
      }
      let cell = IVec2::new((i % width) as i32, (i / width) as i32);
      let at = || format!("cell ({}, {})", cell.x, cell.y);
      let k = self.first_gids.partition_point(|&(first, _)| first <= gid);
      let Some(&(first, tileset)) = k.checked_sub(1).and_then(|k| self.first_gids.get(k)) else {
        return Err(fail(
          &path,
          place,
          format!("{}: gid {gid} is outside every tileset", at()),
        ));
      };
      let id = gid - first;
      if !grid.set(cell, Some(Tile { tileset, id, flip })) {
        return Err(fail(
          &path,
          place,
          format!(
            "{}: tile id {id} is past the {MAX_TILES} tiles a tileset holds",
            at()
          ),
        ));
      }
      let slot = &mut highest[tileset as usize];
      *slot = Some(slot.map_or(id, |m| m.max(id)));
    }
    // a loaded grid starts clean
    grid.drain_dirty(&mut Vec::new());
    let used = highest
      .into_iter()
      .enumerate()
      .filter_map(|(tileset, max)| Some((tileset as u16, max?)))
      .collect();
    Ok((
      LayerKind::Tiles {
        grid,
        intgrid: None,
      },
      used,
    ))
  }
}

/// A map from what its file says. Tilesets named by `source` and images load
/// as dependencies of the file being loaded, resolved against its folder
pub(crate) fn build_map(
  raw: RawMap,
  cx: &mut LoadContext<'_>,
  px: &mut Props<'_>,
) -> Result<TileMap, AssetError> {
  let path = cx.path().clone();
  let place = "map";
  match raw.orientation.as_str() {
    "" | "orthogonal" => {}
    other => {
      return Err(fail(
        &path,
        place,
        format!("{other} maps are not supported yet; they arrive in sub-project 6b"),
      ));
    }
  }
  if raw.infinite {
    return Err(fail(&path, place, "infinite maps arrive in sub-project 6b"));
  }
  if raw.tilewidth == 0 || raw.tileheight == 0 {
    return Err(fail(&path, place, "the tile size is 0"));
  }
  if cell_count(raw.width, raw.height).is_none() {
    return Err(fail(
      &path,
      place,
      format!(
        "{} x {} cells is past the {MAX_CELLS} cells a map holds",
        raw.width, raw.height
      ),
    ));
  }
  let mut map = TileMap::new(
    UVec2::new(raw.width, raw.height),
    UVec2::new(raw.tilewidth, raw.tileheight),
  );
  map.class = raw.class.clone().filter(|c| !c.is_empty());
  map.properties = px.read(place, &raw.properties, raw.class.as_deref())?;
  if let Some(text) = raw.backgroundcolor.as_deref().filter(|t| !t.is_empty()) {
    map.background = Some(parse_color(text).ok_or_else(|| {
      fail(
        &path,
        place,
        format!("backgroundcolor {text:?} is not a color"),
      )
    })?);
  }
  if raw.tilesets.len() > MAX_TILESETS {
    return Err(fail(
      &path,
      place,
      format!(
        "{} tilesets is past the {MAX_TILESETS} a map holds",
        raw.tilesets.len()
      ),
    ));
  }
  let mut first_gids = Vec::with_capacity(raw.tilesets.len());
  let mut entries = Vec::with_capacity(raw.tilesets.len());
  for (index, entry) in raw.tilesets.into_iter().enumerate() {
    let place = format!("tileset {index}");
    if entry.firstgid == 0 {
      return Err(fail(
        &path,
        &place,
        "firstgid is 0, which is the empty cell",
      ));
    }
    first_gids.push((entry.firstgid, index as u16));
    let source = match &entry.source {
      Some(written) => {
        let joined = join(&path, &place, written)?;
        entries.push(TilesetEntry {
          path: AssetPath::parse(&joined)
            .ok()
            .map(|p| p.as_str().to_string()),
          alignment: None,
        });
        TilesetSource::Asset(load_dep::<Tileset>(cx, &path, &place, written)?)
      }
      None => {
        let alignment = Alignment::parse(entry.inline.objectalignment.as_deref())
          .ok_or_else(|| fail(&path, &place, "objectalignment is not one Tiled has"))?;
        entries.push(TilesetEntry {
          path: None,
          alignment: Some(alignment),
        });
        TilesetSource::Embedded(Arc::new(build_tileset(entry.inline, cx, px)?))
      }
    };
    map.tilesets.push(source);
  }
  first_gids.sort_unstable();
  if let Some(pair) = first_gids.windows(2).find(|w| w[0].0 == w[1].0) {
    return Err(fail(
      &path,
      place,
      format!("two tilesets start at gid {}", pair[0].0),
    ));
  }
  let mut builder = Builder {
    px,
    objects: ObjectState::new(entries),
    path: path.clone(),
    size: map.size,
    first_gids,
    next_id: max_layer_id(&raw.layers).checked_add(1),
    layers: Vec::new(),
    used_ids: Vec::new(),
  };
  let inherit = Inherit {
    offset: Vec2::ZERO,
    opacity: 1.0,
    tint: Color::WHITE,
    parallax: Vec2::ONE,
    visible: true,
    blend: BlendMode::Alpha,
  };
  builder.layers(raw.layers, inherit, 0, cx)?;
  map.layers = builder.layers;
  map.used_ids = builder.used_ids;
  Ok(map)
}

// ---- checks ---------------------------------------------------------------

/// Whether the atlas image holds the tiles the tileset declares
pub(crate) fn check_tileset(set: &Tileset, cx: &CheckContext<'_>) -> Result<(), AssetError> {
  let TilesetImage::Atlas { image, .. } = &set.image else {
    return Ok(());
  };
  if set.tile_count == 0 {
    return Ok(());
  }
  let world = cx.world();
  let Some(actual) = world
    .get_resource::<Assets<Image>>()
    .and_then(|images| images.get(image))
    .map(|image| UVec2::new(image.width(), image.height()))
  else {
    return Ok(());
  };
  // the far edges of the last column and the last row
  let last_column = set.columns.min(set.tile_count).checked_sub(1);
  let needs = last_column
    .and_then(|c| set.tile_rect(c))
    .zip(set.tile_rect(set.tile_count - 1))
    .map(|(across, down)| UVec2::new(across.max.x, down.max.y));
  if needs.is_some_and(|n| n.x <= actual.x && n.y <= actual.y) {
    return Ok(());
  }
  let name = world
    .get_resource::<AssetServer>()
    .and_then(|server| server.path(image))
    .map_or_else(String::new, |p| format!(" {p:?}"));
  let needed = match needs {
    Some(n) => format!("it needs at least {} x {}", n.x, n.y),
    None => "not one tile fits".to_string(),
  };
  Err(fail(
    cx.path(),
    &format!("tileset {:?}", set.name),
    format!(
      "the image{name} is {} x {}, too small for {} tiles of {} x {} ({needed})",
      actual.x, actual.y, set.tile_count, set.tile_size.x, set.tile_size.y
    ),
  ))
}

/// Whether the tilesets hold what the map's layers use, and the embedded
/// ones their images
pub(crate) fn check_map(map: &TileMap, cx: &CheckContext<'_>) -> Result<(), AssetError> {
  for source in &map.tilesets {
    if let TilesetSource::Embedded(set) = source {
      check_tileset(set, cx)?;
    }
  }
  let sets = cx.world().get_resource::<Assets<Tileset>>();
  for &(layer, tileset, max) in &map.used_ids {
    let count = match map.tilesets.get(tileset as usize) {
      Some(TilesetSource::Embedded(set)) => Some((set.name.as_str(), set.tile_count)),
      Some(TilesetSource::Asset(handle)) => sets
        .and_then(|sets| sets.get(handle))
        .map(|set| (set.name.as_str(), set.tile_count)),
      None => None,
    };
    if let Some((name, count)) = count
      && max >= count
    {
      let layer = map.layers.get(layer).map_or("", |l| l.info.name.as_str());
      return Err(fail(
        cx.path(),
        &format!("layer {layer:?}"),
        format!("tile id {max} is past the {count} tiles of tileset {name:?}"),
      ));
    }
  }
  Ok(())
}
