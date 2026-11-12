//! From what a Tiled file says ([`raw`](super::raw)) to a [`TileMap`] or a
//! [`Tileset`], and the checks that need the loaded images and tilesets.

use std::fmt::Display;
use std::sync::Arc;
use std::time::Duration;

use super::data::{decode_gid, decode_layer_data};
use super::objects::{Alignment, ObjectState, TilesetEntry, build_objects};
use super::project::{Props, parse_color};
use super::raw::{RawChunk, RawData, RawLayer, RawMap, RawObject, RawTile, RawTileset};
use super::wang::{build_wang_sets, transformations};
use crate::core::asset::{
  Asset, AssetError, AssetPath, AssetServer, Assets, CheckContext, Handle, LoadContext,
};
use crate::core::graphics::Image;
use crate::core::hash::FastSet;
use crate::core::physics::CellRect;
use crate::core::render::batch::{BlendMode, Filter};
use crate::core::tilemap::map::tiles_bounds;
use crate::core::tilemap::{
  CHUNK_SIZE, GridShape, ImageLayer, LayerKind, MapGrid, MapLayer, Properties, PropertyValue,
  StaggerAxis, StaggerIndex, Tile, TileData, TileFlip, TileFrame, TileGrid, TileMap, TileShape,
  Tileset, TilesetImage, TilesetSource,
};
use crate::core::ui::Color;
use glam::{IVec2, UVec2, Vec2};

/// Most cells a layer or a map holds (8192 x 8192)
const MAX_CELLS: u64 = 1 << 26;
/// Most cells a map's tile layers hold together (and an LDtk level's and
/// project's, an IntGrid layer counting its tiles and its values): 512 MiB
/// of 4-byte cells, an 8192 x 8192 map of two layers or a 1024 x 1024 one of
/// 128. Counted as the engine allocates them, in whole chunks of
/// `CHUNK_SIZE` x `CHUNK_SIZE` (`layer_cells`, `chunked_cells`), from the
/// layers' sizes before any is decoded or allocated, so a small file
/// cannot ask for gigabytes
pub(crate) const MAX_MAP_CELLS: u64 = 1 << 27;
/// Most tiles a tileset holds: what a cell's 20 bits can name
pub(crate) const MAX_TILES: u32 = 1_048_575;
const MAX_TILESETS: usize = 256;
pub(super) const MAX_DEPTH: usize = 64;
/// Points an ellipse is cut into
const ELLIPSE_POINTS: usize = 16;

pub(crate) fn fail(path: &AssetPath, place: &str, what: impl Display) -> AssetError {
  AssetError::decode(path, format!("{place}: {what}"))
}

/// `written`, a path in the file at `path`, as a path from the asset root
pub(crate) fn join(path: &AssetPath, place: &str, written: &str) -> Result<String, AssetError> {
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

/// Where a file names each of its dependencies, kept with what it loads to,
/// so one that fails is reported at its place in the file (a tileset's
/// image names its tileset), however it fails: loaded at once, or on the
/// workers, where the server tells the loader (`AssetLoader::dependency_failed`)
#[derive(Clone, Default)]
pub(crate) struct DependencyPlaces(Vec<(String, String)>);

impl std::fmt::Debug for DependencyPlaces {
  // Paths differ between a file and its twin in the other format, and what
  // a load compares is what it loads to: only how many
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    write!(f, "DependencyPlaces({})", self.0.len())
  }
}

impl DependencyPlaces {
  /// Notes that the file names `dependency` (an asset path) at `place`; the
  /// first place counts
  fn note(&mut self, dependency: &str, place: &str) {
    if !self.0.iter().any(|(d, _)| d == dependency) {
      self.0.push((dependency.to_string(), place.to_string()));
    }
  }

  /// The error of a failed dependency (the server's `AssetError::Dependency`)
  /// at the place the file names it; `None` for another error, or a
  /// dependency the file does not name
  pub(crate) fn explain(&self, error: &AssetError) -> Option<AssetError> {
    let AssetError::Dependency {
      path,
      dependency,
      error,
    } = error
    else {
      return None;
    };
    let (_, place) = self.0.iter().find(|(d, _)| d == dependency.as_str())?;
    Some(dependency_error(path, place, dependency, error))
  }
}

/// The editor features a file uses that load otherwise than the editor
/// shows them, each warned once per file (`warn`), naming its first place
/// and how many more
#[derive(Default)]
pub(crate) struct Unread(Vec<(String, String, usize)>);

impl Unread {
  /// Notes `what` (what the file does and how it loads) at `place`
  pub(crate) fn note(&mut self, what: impl Into<String>, place: &str) {
    let what = what.into();
    match self.0.iter_mut().find(|(w, ..)| *w == what) {
      Some((.., more)) => *more += 1,
      None => self.0.push((what, place.to_string(), 0)),
    }
  }

  /// Warns once for each feature noted, then forgets them
  pub(crate) fn warn(&mut self, path: &AssetPath) {
    for (what, place, more) in self.0.drain(..) {
      let more = match more {
        0 => String::new(),
        1 => " and 1 place more".to_string(),
        n => format!(" and {n} places more"),
      };
      log::warn!("{path}: {what} (at {place}{more})");
    }
  }
}

/// `path`'s dependency `dependency`, named at `place`, failed with `error`
fn dependency_error(
  path: &AssetPath,
  place: &str,
  dependency: &AssetPath,
  error: &AssetError,
) -> AssetError {
  fail(
    path,
    place,
    format!("{:?} did not load: {}", dependency.as_str(), error.report()),
  )
}

/// Loads the file `written` names as a dependency of the one being loaded,
/// noting where in `places`
pub(crate) fn load_dep<T: Asset>(
  cx: &mut LoadContext<'_>,
  path: &AssetPath,
  place: &str,
  written: &str,
  places: &mut DependencyPlaces,
) -> Result<Handle<T>, AssetError> {
  let joined = join(path, place, written)?;
  if let Ok(normal) = AssetPath::parse(&joined) {
    places.note(normal.as_str(), place);
  }
  cx.load::<T>(&joined).map_err(|error| match error {
    AssetError::Dependency {
      dependency, error, ..
    } => dependency_error(path, place, &dependency, &error),
    refused => fail(path, place, format!("{written:?}: {}", refused.report())),
  })
}

/// A transparent colour as Tiled writes it (`#ff00ff` in JSON, `ff00ff` in
/// XML); an empty one is none
fn transparent(
  path: &AssetPath,
  place: &str,
  text: Option<&str>,
) -> Result<Option<Color>, AssetError> {
  let Some(text) = text.filter(|t| !t.is_empty()) else {
    return Ok(None);
  };
  parse_color(text).map(Some).ok_or_else(|| {
    fail(
      path,
      place,
      format!("the transparent colour {text:?} is not a color"),
    )
  })
}

fn multiply(a: Color, b: Color) -> Color {
  let m = |x: u8, y: u8| ((x as u16 * y as u16 + 127) / 255) as u8;
  Color::new(m(a.r, b.r), m(a.g, b.g), m(a.b, b.b), m(a.a, b.a))
}

/// The cells the engine allocates for a bounded grid of `size` were it
/// full: every chunk of `CHUNK_SIZE` x `CHUNK_SIZE` it spans, whole, since
/// a grid makes a chunk whole where a tile goes (a 1 x 1 layer takes
/// 1,024 cells, a 1 x 2^26 one 2^31)
pub(crate) fn engine_cells(size: UVec2) -> u64 {
  let chunks = |side: u32| u64::from(side.div_ceil(CHUNK_SIZE));
  chunks(size.x)
    .saturating_mul(chunks(size.y))
    .saturating_mul(u64::from(CHUNK_SIZE * CHUNK_SIZE))
}

/// The cells a bounded grid of `size` counts against the caps: its
/// engine cells ([`engine_cells`]), once its sides and cells are within a
/// layer's (`cell_count`) and so are its engine cells. The error says
/// which is past
pub(crate) fn layer_cells(size: UVec2) -> Result<u64, String> {
  if cell_count(size.x, size.y).is_none() {
    return Err(format!(
      "{} x {} cells is past the {MAX_CELLS} cells a layer holds",
      size.x, size.y
    ));
  }
  let cells = engine_cells(size);
  if cells > MAX_CELLS {
    return Err(format!(
      "{} x {} cells take {cells} cells of the engine's chunks of {CHUNK_SIZE} x {CHUNK_SIZE}, past the {MAX_CELLS} cells a layer holds",
      size.x, size.y
    ));
  }
  Ok(cells)
}

/// A cell count that is within the limit
pub(crate) fn cell_count(w: u32, h: u32) -> Option<usize> {
  // each side too: a zero side would let the other be anything
  if u64::from(w) > MAX_CELLS || u64::from(h) > MAX_CELLS {
    return None;
  }
  let n = u64::from(w) * u64::from(h);
  (n <= MAX_CELLS).then_some(n as usize)
}

// ---- tilesets -------------------------------------------------------------

/// The polygon of a collision object, in tile pixels; `None` for objects that
/// are no closed shape (points, polylines, text, tile objects). Ellipses and
/// capsules are cut into polygons
fn collision_shape(
  px: &mut Props<'_>,
  place: &str,
  o: &RawObject,
) -> Result<Option<TileShape>, AssetError> {
  let place = format!("{place}, object {}", o.id);
  if o.polyline.is_some() {
    px.unread.note(
      "a polyline among a tile's collision shapes has no inside, so it does not collide or cast a shadow (draw a polygon)",
      &place,
    );
    return Ok(None);
  }
  if o.point || o.text.is_some() || o.gid.is_some() {
    return Ok(None);
  }
  let (width, height) = (o.width.unwrap_or(0.0), o.height.unwrap_or(0.0));
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
  } else if o.capsule {
    if width <= 0.0 || height <= 0.0 {
      return Ok(None);
    }
    capsule_points(width, height)
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

/// A capsule of `width` x `height` from its top-left: a rectangle whose two
/// short ends are half circles, each cut into `ELLIPSE_POINTS / 2` edges,
/// clockwise on screen (y down) from the right end's top
fn capsule_points(width: f64, height: f64) -> Vec<(f64, f64)> {
  let r = width.min(height) / 2.0;
  let half = ELLIPSE_POINTS / 2;
  let arc = |centre: (f64, f64), from: f64| {
    (0..=half).map(move |i| {
      let t = from + std::f64::consts::PI * i as f64 / half as f64;
      (centre.0 + r * t.cos(), centre.1 + r * t.sin())
    })
  };
  let quarter = std::f64::consts::FRAC_PI_2;
  if width >= height {
    // ends left and right: the right half circle from its top, then the left
    arc((width - r, r), -quarter)
      .chain(arc((r, r), quarter))
      .collect()
  } else {
    // ends top and bottom: the bottom half circle from its right, then the top
    arc((r, height - r), 0.0)
      .chain(arc((r, r), std::f64::consts::PI))
      .collect()
  }
}

fn tile_data(
  cx: &mut LoadContext<'_>,
  px: &mut Props<'_>,
  path: &AssetPath,
  place: &str,
  raw: RawTile,
) -> Result<TileData, AssetError> {
  let properties = px.read(place, &raw.properties, raw.class.as_deref())?;
  let mut data = TileData {
    class: raw.class.clone().filter(|c| !c.is_empty()),
    sort_offset: properties
      .float("sort_offset")
      .filter(|offset| offset.is_finite())
      .map_or(0.0, |offset| offset as f32),
    stairs: storey("stairs", properties.get("stairs")).map_err(|what| fail(path, place, what))?,
    properties,
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
    let handle = load_dep::<Image>(cx, path, place, written, &mut px.places)?;
    data.image = Some((handle, UVec2::new(raw.imagewidth, raw.imageheight)));
    let part = (
      raw.x.unwrap_or(0),
      raw.y.unwrap_or(0),
      raw.width.unwrap_or(raw.imagewidth),
      raw.height.unwrap_or(raw.imageheight),
    );
    if part != (0, 0, raw.imagewidth, raw.imageheight) {
      px.unread.note(
        "a collection tile's part of its image (x, y, width, height) is not read: the tile draws its whole image",
        place,
      );
    }
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
      let image = load_dep::<Image>(cx, &path, &place, written, &mut px.places)?;
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
  if raw.tilerendersize.as_deref() == Some("grid") {
    px.unread.note(
      "a tileset's tile render size \"grid\" (tiles scaled to the map's cells as its fill mode says) is not read: its tiles draw at their own size",
      &place,
    );
  }
  let isometric = raw
    .grid
    .as_ref()
    .is_some_and(|grid| grid.orientation == "isometric");
  let shaped = raw.tiles.iter().any(|tile| {
    tile
      .objectgroup
      .as_ref()
      .is_some_and(|group| !group.objects.is_empty())
  });
  if isometric && shaped {
    // Measured: Tiled keeps a tile's shapes as written whatever the grid,
    // and its own exporters read them in the image's pixels; its collision
    // editor, which scripting cannot read, draws them on this grid
    px.unread.note(
      "a tileset's isometric grid is not read: its tiles' collision shapes are read in their images' pixels, as Tiled's collision editor shows them on an orthogonal grid (set the tileset's orientation to orthogonal in Tiled to see them as they collide)",
      &place,
    );
  }
  set.transparent = transparent(&path, &place, raw.transparentcolor.as_deref())?;
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
  // after the tiles, whose probabilities the sets' tiles carry
  set.transformations = transformations(&raw.transformations);
  let wang_sets = build_wang_sets(px, &place, raw.wangsets, set.transformations, |id| {
    set.tile(id).map_or(1.0, |data| data.probability)
  })?;
  set.wang_sets = wang_sets;
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
  /// The nearest group's floor that names one
  floor: Option<u8>,
}

struct Builder<'p, 'a> {
  px: &'p mut Props<'a>,
  objects: ObjectState,
  path: AssetPath,
  size: UVec2,
  /// Whether the map is infinite: its tile layers' cells are in chunks
  infinite: bool,
  /// The map's grid, which every tile layer copies
  grid: MapGrid,
  /// Each tileset's first gid and its index, by first gid
  first_gids: Vec<(u32, u16)>,
  /// The id for a layer the file gave none; `None` once the ids ran out
  next_id: Option<u32>,
  layers: Vec<MapLayer>,
  used_ids: Vec<(usize, u16, u32)>,
  /// The engine chunks an infinite map's Tiled chunks touch, any layer's
  file_chunks: FastSet<IVec2>,
}

/// The cells the tile layers of `layers` (in groups too) take, as the
/// engine allocates them: a bounded layer's as its size says (a layer
/// without one is the map's `size`; [`layer_cells`]), an infinite map's as
/// [`chunked_cells`] counts them; a layer past what one holds fails,
/// naming it
fn tile_cells(
  path: &AssetPath,
  layers: &[RawLayer],
  size: UVec2,
  grid: &MapGrid,
  depth: usize,
) -> Result<u64, AssetError> {
  let mut total = 0u64;
  // deeper groups fail as they are built
  if depth > MAX_DEPTH {
    return Ok(total);
  }
  for l in layers {
    let place = || format!("layer {:?}", l.name);
    let cells = match (l.ty.as_str(), &l.chunks) {
      ("tilelayer", Some(chunks)) => chunked_cells(path, &place(), chunks, grid)?,
      ("tilelayer", None) => {
        let (w, h) = (l.width.unwrap_or(size.x), l.height.unwrap_or(size.y));
        layer_cells(UVec2::new(w, h)).map_err(|what| fail(path, &place(), what))?
      }
      ("group", _) => tile_cells(path, &l.layers, size, grid, depth + 1)?,
      _ => 0,
    };
    total = total.saturating_add(cells);
  }
  Ok(total)
}

/// The cells an infinite map's layer of `chunks`, at `place`, counts
/// against the caps: the more of the cells its chunks hold (what is read
/// and decoded) and the cells of the engine's chunks they reach (what is
/// allocated: each engine chunk a tile goes in holds `CHUNK_SIZE` x
/// `CHUNK_SIZE` cells, however few of them the file writes, so a 1 x 1
/// chunk takes 1,024 and a 16 x 16 one off the engine's grid reaches
/// four). Each chunk is checked first (`check_chunk`); either count past a
/// layer's cells fails naming the layer. The engine chunks are found
/// without decoding anything, keeping no more of them than a layer holds
fn chunked_cells(
  path: &AssetPath,
  place: &str,
  chunks: &[RawChunk],
  grid: &MapGrid,
) -> Result<u64, AssetError> {
  let per_chunk = u64::from(CHUNK_SIZE * CHUNK_SIZE);
  let mut read = 0u64;
  let mut reached: FastSet<IVec2> = FastSet::default();
  for (index, chunk) in chunks.iter().enumerate() {
    let (origin, size) = check_chunk(chunk, grid)
      .map_err(|what| fail(path, &chunk_place(place, index, chunk), what))?;
    read = read.saturating_add(u64::from(size.x) * u64::from(size.y));
    if read > MAX_CELLS {
      return Err(fail(
        path,
        place,
        format!("its chunks hold {read} cells, past the {MAX_CELLS} cells a layer holds"),
      ));
    }
    // Its first and last cells are cells an `i32` holds (`check_chunk`)
    let last = origin.saturating_add((size - UVec2::ONE).as_ivec2());
    let (first, last) = (TileGrid::chunk_of(origin), TileGrid::chunk_of(last));
    for y in first.y..=last.y {
      for x in first.x..=last.x {
        if reached.insert(IVec2::new(x, y)) && reached.len() as u64 * per_chunk > MAX_CELLS {
          return Err(fail(
            path,
            place,
            format!(
              "its chunks take {} cells of the engine's chunks of {CHUNK_SIZE} x {CHUNK_SIZE}, past the {MAX_CELLS} cells a layer holds",
              reached.len() as u64 * per_chunk
            ),
          ));
        }
      }
    }
  }
  Ok(read.max(reached.len() as u64 * per_chunk))
}

/// An infinite map's tile layer `raw` (at `place`) as an unbounded grid of
/// its chunks: each checked (`check_chunk`; the map's cells were counted
/// before), decoded in the layer's encoding and compression, turned into
/// cells by `cells` (the chunk's gids, its first cell and its width) and
/// written where it says. A layer of data, not chunks, fails
fn chunked_grid(
  path: &AssetPath,
  place: &str,
  raw: &RawLayer,
  grid: &MapGrid,
  mut cells: impl FnMut(&mut [u32], IVec2, u32) -> Result<(), String>,
) -> Result<TileGrid, AssetError> {
  if raw.chunks.is_none() && raw.data.as_ref().is_some_and(holds_data) {
    return Err(fail(
      path,
      place,
      "an infinite map's layer keeps its cells in chunks, not data",
    ));
  }
  let mut tiles = TileGrid::unbounded();
  for (index, chunk) in raw.chunks.iter().flatten().enumerate() {
    let at = chunk_place(place, index, chunk);
    // Checked with the map's cells before anything was decoded: this only
    // reads its place and size again
    let (origin, size) = check_chunk(chunk, grid).map_err(|e| fail(path, &at, e))?;
    let expected = (size.x as usize) * (size.y as usize);
    // Its data as long as its size before any is decoded
    let data = match &chunk.data {
      None => return Err(fail(path, &at, "the chunk has no data")),
      Some(RawData::Array(gids)) if gids.len() != expected => {
        return Err(fail(
          path,
          &at,
          format!("has {} cells, expected {expected}", gids.len()),
        ));
      }
      Some(data) => data,
    };
    let mut decoded = decode_layer_data(
      data,
      raw.encoding.as_deref(),
      raw.compression.as_deref(),
      expected,
    )
    .map_err(|e| fail(path, &at, e))?;
    cells(&mut decoded, origin, size.x).map_err(|e| fail(path, &at, e))?;
    tiles.write_rows(origin, size.x as usize, &decoded);
  }
  Ok(tiles)
}

/// The cells an infinite map's tile layers (in groups too) hold tiles in,
/// read from what its file says (`raw`, on `grid`, the map's) without
/// loading it, for a world file's level rectangle: the map's cells counted
/// against the caps, then its chunks checked, decoded and placed as its
/// load does, and their rectangle found by the function
/// [`TileMap::bounds`] uses, so the two agree. Empty when the map holds no
/// tile. A chunk or count its load refuses fails here too
pub(crate) fn infinite_bounds(
  path: &AssetPath,
  raw: &RawMap,
  grid: &MapGrid,
) -> Result<CellRect, AssetError> {
  if grid.cell.min_element() == 0 {
    return Err(fail(path, "map", "the tile size is 0"));
  }
  let cells = tile_cells(path, &raw.layers, UVec2::ZERO, grid, 0)?;
  check_map_cells(path, "map", cells)?;
  // Any tile is a tile here: which one is the map load's business
  let Some(tile) = TileGrid::encode(Some(Tile::new(0, 0))) else {
    return Ok(CellRect::default());
  };
  let mut grids = Vec::new();
  let mut pending: Vec<(&[RawLayer], usize)> = vec![(&raw.layers, 0)];
  while let Some((layers, depth)) = pending.pop() {
    // deeper groups fail as the map is built
    if depth > MAX_DEPTH {
      continue;
    }
    for layer in layers {
      match layer.ty.as_str() {
        "group" => pending.push((&layer.layers, depth + 1)),
        "tilelayer" => {
          let place = format!("layer {:?}", layer.name);
          grids.push(chunked_grid(path, &place, layer, grid, |gids, _, _| {
            for gid in gids.iter_mut() {
              *gid = if decode_gid(*gid).0 == 0 { 0 } else { tile };
            }
            Ok(())
          })?);
        }
        _ => {}
      }
    }
  }
  Ok(tiles_bounds(&grids))
}

/// Where the chunk `index` (counting from 0) of the layer at `layer` is,
/// for its errors: by its place, or by its index when it states no place
fn chunk_place(layer: &str, index: usize, chunk: &RawChunk) -> String {
  match (chunk.x, chunk.y) {
    (Some(x), Some(y)) => format!("{layer}, chunk ({x}, {y})"),
    _ => format!("{layer}, chunk {index} (counting from 0)"),
  }
}

/// Where `chunk`'s first cell is and its size in cells, once it is a
/// chunk the engine can hold: its `x`, `y`, `width` and `height` all
/// stated (Tiled writes each; one left out is a broken file, not a 0), a
/// size of a cell or more, within what a layer holds, every cell one an
/// `i32` holds, and every cell's pixels (its outline on `grid`, the map's)
/// numbers an `i32` holds. Checked before anything is decoded or
/// allocated, so a chunk far past the cells and pixels the engine counts
/// in fails naming it, and a map's cells are counted from its chunks'
/// places and sizes alone
fn check_chunk(chunk: &RawChunk, grid: &MapGrid) -> Result<(IVec2, UVec2), String> {
  let stated =
    |value: Option<i64>, name: &str| value.ok_or_else(|| format!("it does not state its `{name}`"));
  let (x, y) = (stated(chunk.x, "x")?, stated(chunk.y, "y")?);
  let (w, h) = (
    stated(chunk.width, "width")?,
    stated(chunk.height, "height")?,
  );
  if w <= 0 || h <= 0 {
    return Err(format!("its size, {w} x {h}, holds no cell"));
  }
  let Some(size) = u32::try_from(w)
    .ok()
    .zip(u32::try_from(h).ok())
    .filter(|&(w, h)| cell_count(w, h).is_some())
    .map(|(w, h)| UVec2::new(w, h))
  else {
    return Err(format!(
      "{w} x {h} cells is past the {MAX_CELLS} cells a layer holds"
    ));
  };
  // Its first and last cells, each one an i32 holds: on a grid whose step
  // is under a pixel (a staggered map of 1-pixel tiles) a cell past one can
  // still sit at a pixel within it
  let cell = |x: i64, y: i64| Some(IVec2::new(i32::try_from(x).ok()?, i32::try_from(y).ok()?));
  let ends = cell(x, y).zip(cell(x.saturating_add(w - 1), y.saturating_add(h - 1)));
  // and the box of their outlines on the grid, in exact pixels
  let fits = |(first, last): &(IVec2, IVec2)| {
    let (min, max) = grid.exact_box(*first, *last);
    min.min_element() >= f64::from(i32::MIN) && max.max_element() <= f64::from(i32::MAX)
  };
  let Some((first, _)) = ends.filter(fits) else {
    return Err(
      "its cells reach past the pixels a map holds (a cell's place in pixels on the map's grid must fit a 32-bit integer)"
        .to_string(),
    );
  };
  Ok((first, size))
}

/// Fails if layers holding `cells` together are past [`MAX_MAP_CELLS`]
pub(crate) fn check_map_cells(path: &AssetPath, place: &str, cells: u64) -> Result<(), AssetError> {
  if cells > MAX_MAP_CELLS {
    return Err(fail(
      path,
      place,
      format!("its layers hold {cells} cells, past the {MAX_MAP_CELLS} a map holds"),
    ));
  }
  Ok(())
}

fn max_layer_id(layers: &[RawLayer]) -> u32 {
  layers
    .iter()
    .map(|l| l.id.unwrap_or(0).max(max_layer_id(&l.layers)))
    .max()
    .unwrap_or(0)
}

/// A tile or object layer's `filter` property: "linear" or "nearest" (in
/// any case). Absent, or any other value or type, keeps the default, as a
/// reserved property of the wrong type does
fn layer_filter(props: &Properties) -> Filter {
  match props.string("filter") {
    Some(value) if value.eq_ignore_ascii_case("linear") => Filter::Linear,
    Some(value) if value.eq_ignore_ascii_case("nearest") => Filter::Nearest,
    _ => Filter::Default,
  }
}

/// The reserved property `name` naming a floor (a layer's `floor`, a
/// tile's `stairs`), `value`: a whole number from 0 to 255, a float with no
/// fraction counting; `None` left out. Any other value is an error, saying
/// what is wrong
fn storey(name: &str, value: Option<&PropertyValue>) -> Result<Option<u8>, String> {
  let Some(value) = value else {
    return Ok(None);
  };
  match value.whole().and_then(|n| u8::try_from(n).ok()) {
    Some(floor) => Ok(Some(floor)),
    None => Err(format!(
      "property {name:?}: {} is not a whole number from 0 to 255",
      as_tiled_shows(value)
    )),
  }
}

/// A property's value as Tiled shows it, for an error
fn as_tiled_shows(value: &PropertyValue) -> String {
  match value {
    PropertyValue::Int(n) => n.to_string(),
    PropertyValue::Float(f) => f.to_string(),
    PropertyValue::Bool(b) => b.to_string(),
    PropertyValue::String(text) | PropertyValue::File(text) => format!("{text:?}"),
    PropertyValue::Color(c) => format!("#{:02x}{:02x}{:02x}{:02x}", c.a, c.r, c.g, c.b),
    PropertyValue::Object(id) => format!("object {id}"),
    PropertyValue::Enum { value, .. } => value.clone(),
    PropertyValue::Class { ty, .. } if ty.is_empty() => "a class".to_string(),
    PropertyValue::Class { ty, .. } => format!("a {ty}"),
    PropertyValue::List(items) => format!(
      "[{}]",
      items
        .iter()
        .map(as_tiled_shows)
        .collect::<Vec<_>>()
        .join(", ")
    ),
  }
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

  /// Whether the map's grid is hexagonal
  fn hex(&self) -> bool {
    matches!(self.grid.shape, GridShape::Hexagonal { .. })
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
    let group = raw.ty == "group";
    // A group's floor (its own, or its class's default) passes to its
    // layers that name none; its other properties are its own, not read
    let group_floor = if group {
      let floor = self
        .px
        .read_one(&place, &raw.properties, raw.class.as_deref(), "floor")?;
      storey("floor", floor.as_ref()).map_err(|what| fail(&path, &place, what))?
    } else {
      None
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
      floor: group_floor.or(inherit.floor),
    };
    if group {
      return self.layers(raw.layers, here, depth + 1, cx);
    }
    let id = self.layer_id(&raw, &place)?;
    let props = self
      .px
      .read(&place, &raw.properties, raw.class.as_deref())?;
    let floor = storey("floor", props.get("floor")).map_err(|what| fail(&path, &place, what))?;
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
        let mut objects = build_objects(
          &mut self.objects,
          &self.first_gids,
          cx,
          self.px,
          &place,
          std::mem::take(&mut raw.objects),
          &mut highest,
        )?;
        // Tiled ignores the 120 degree turn off hexagonal maps
        if !self.hex() {
          for tile in objects.iter_mut().filter_map(|o| o.tile.as_mut()) {
            tile.flip = tile.flip.without(TileFlip::HEX_ROTATE_120);
          }
        }
        used = highest
          .into_iter()
          .enumerate()
          .filter_map(|(tileset, max)| Some((tileset as u16, max?)))
          .collect();
        // Tiled draws a top-down layer's (its default) objects by their y:
        // its tile objects draw in file order here, which differs only
        // where two overlap in the other order
        let top_down = matches!(raw.draworder.as_deref(), None | Some("topdown"));
        if top_down && drawn_out_of_order(&objects) {
          self.px.unread.note(
            "an object layer drawn top down (Tiled's default) draws its tile objects in their order in the file, not by their y (set the layer's draw order to manual, or order them)",
            &place,
          );
        }
        LayerKind::Objects(objects)
      }
      "imagelayer" => {
        // Tiled saves a new image layer with no image: it draws nothing
        let image = match raw.image.as_deref().filter(|i| !i.is_empty()) {
          Some(written) => Some(load_dep::<Image>(
            cx,
            &path,
            &place,
            written,
            &mut self.px.places,
          )?),
          None => None,
        };
        LayerKind::Image(ImageLayer {
          image,
          repeat_x: raw.repeatx,
          repeat_y: raw.repeaty,
          transparent: transparent(&path, &place, raw.transparentcolor.as_deref())?,
        })
      }
      other => return Err(fail(&path, &place, format!("unknown layer type {other:?}"))),
    };
    let mut info = crate::core::tilemap::LayerInfo::new(id, raw.name);
    info.class = raw.class.filter(|c| !c.is_empty());
    info.offset = here.offset;
    info.floor = floor.or(here.floor).unwrap_or(0);
    info.opacity = here.opacity;
    info.tint = here.tint;
    info.parallax = here.parallax;
    info.visible = here.visible;
    info.blend = here.blend;
    if !matches!(kind, LayerKind::Image(_)) {
      info.filter = layer_filter(&props);
    }
    info.collides = props.bool("collides").unwrap_or(is_tiles);
    info.occludes = props.bool("occludes");
    info.self_shadow = match props.get("self_shadow") {
      Some(PropertyValue::Bool(on)) => *on,
      Some(_) => {
        self.px.unread.note(
          "the reserved layer property \"self_shadow\" is not a bool, so the layer's shadows do not shadow themselves",
          &place,
        );
        false
      }
      None => false,
    };
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

  /// The layer's tiles, and the highest id it uses of each tileset: a
  /// bounded grid of its data, or an infinite map's unbounded grid of its
  /// chunks, each chunk as large as it says, where it says
  fn tile_layer(
    &mut self,
    raw: &RawLayer,
    place: &str,
  ) -> Result<(LayerKind, Vec<(u16, u32)>), AssetError> {
    let path = self.path.clone();
    let mut highest: Vec<Option<u32>> = vec![None; self.first_gids.len()];
    // Each Tiled chunk's first and last engine chunk
    let mut touched: Vec<(IVec2, IVec2)> = Vec::new();
    let map_grid = self.grid;
    let mut grid = match (self.infinite, &raw.chunks) {
      (true, _) => chunked_grid(&path, place, raw, &map_grid, |gids, origin, width| {
        // A chunk of a cell or more, its last cell one an `i32` holds
        // (`check_chunk`)
        let rows = (gids.len() as u32).checked_div(width).unwrap_or(0);
        let last = origin.saturating_add(UVec2::new(width, rows).as_ivec2() - IVec2::ONE);
        touched.push((TileGrid::chunk_of(origin), TileGrid::chunk_of(last)));
        self.cells(gids, &mut highest, origin, width)
      })?,
      (false, Some(_)) => {
        return Err(fail(
          &path,
          place,
          "the layer keeps its cells in chunks, which only an infinite map's layers do",
        ));
      }
      (false, None) => {
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
        let mut cells = decode_layer_data(
          data,
          raw.encoding.as_deref(),
          raw.compression.as_deref(),
          expected,
        )
        .map_err(|e| fail(&path, place, e))?;
        self
          .cells(&mut cells, &mut highest, IVec2::ZERO, w)
          .map_err(|e| fail(&path, place, e))?;
        let mut grid = TileGrid::new(UVec2::new(w, h));
        grid.write_rows(IVec2::ZERO, w as usize, &cells);
        grid
      }
    };
    // Counted against the caps with the map's cells, so no more of them
    // than a layer holds
    for (first, last) in touched {
      for y in first.y..=last.y {
        self
          .file_chunks
          .extend((first.x..=last.x).map(|x| IVec2::new(x, y)));
      }
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
        tiles: grid,
        intgrid: None,
        grid: self.grid,
      },
      used,
    ))
  }

  /// Turns `gids` (rows of `width`, the first at cell `origin`) into the
  /// cells a grid holds, in place, noting the highest id each tileset is
  /// used for in `highest`. The error names the cell
  fn cells(
    &self,
    gids: &mut [u32],
    highest: &mut [Option<u32>],
    origin: IVec2,
    width: u32,
  ) -> Result<(), String> {
    let width = (width as usize).max(1);
    let at = |i: usize| {
      let cell = origin + IVec2::new((i % width) as i32, (i / width) as i32);
      format!("cell ({}, {})", cell.x, cell.y)
    };
    // The tileset of the last gid, which the next is most likely in: its
    // first gid, the first of the next tileset, and its index
    let mut last: Option<(u32, u32, u16)> = None;
    let hex = self.hex();
    for (i, slot) in gids.iter_mut().enumerate() {
      let (gid, mut flip) = decode_gid(*slot);
      if gid == 0 {
        *slot = 0;
        continue;
      }
      // Tiled ignores the 120 degree turn off hexagonal maps
      if !hex {
        flip = flip.without(TileFlip::HEX_ROTATE_120);
      }
      let (first, tileset) = match last {
        Some((first, end, tileset)) if (first..end).contains(&gid) => (first, tileset),
        _ => {
          let k = self.first_gids.partition_point(|&(first, _)| first <= gid);
          let Some(&(first, tileset)) = k.checked_sub(1).and_then(|k| self.first_gids.get(k))
          else {
            return Err(format!("{}: gid {gid} is outside every tileset", at(i)));
          };
          let end = self.first_gids.get(k).map_or(u32::MAX, |&(next, _)| next);
          last = Some((first, end, tileset));
          (first, tileset)
        }
      };
      let id = gid - first;
      let Some(cell) = TileGrid::encode(Some(Tile { tileset, id, flip })) else {
        return Err(format!(
          "{}: tile id {id} is past the {MAX_TILES} tiles a tileset holds",
          at(i)
        ));
      };
      *slot = cell;
      let max = &mut highest[tileset as usize];
      *max = Some(max.map_or(id, |m| m.max(id)));
    }
    Ok(())
  }
}

/// Whether `data` holds any cell: an infinite map's empty layer may be
/// written with empty data
fn holds_data(data: &RawData) -> bool {
  match data {
    RawData::Array(gids) => !gids.is_empty(),
    RawData::Text(text) => !text.trim().is_empty(),
  }
}

/// Whether two of `objects`' tile objects overlap and are in the file in
/// the other order from the one Tiled's top-down layers draw them in: by
/// the bottom edge (a tile object's y in Tiled), equal ones in file order
fn drawn_out_of_order(objects: &[crate::core::tilemap::MapObjectData]) -> bool {
  // each tile object's place in the file and its rectangle, by its top
  let mut tiles: Vec<(usize, Vec2, Vec2)> = objects
    .iter()
    .enumerate()
    .filter(|(_, o)| o.tile.is_some())
    .map(|(i, o)| (i, o.position, o.position + o.size))
    .collect();
  tiles.sort_by(|a, b| a.1.y.total_cmp(&b.1.y));
  for (k, a) in tiles.iter().enumerate() {
    for b in &tiles[k + 1..] {
      // this and every later one starts below `a`'s bottom
      if b.1.y >= a.2.y {
        break;
      }
      if a.1.x < b.2.x && b.1.x < a.2.x {
        let tiled = a.2.y < b.2.y || (a.2.y == b.2.y && a.0 < b.0);
        if tiled != (a.0 < b.0) {
          return true;
        }
      }
    }
  }
  false
}

/// The grid a map's file says it has (`raw`, the file at `path`): its
/// orientation (orthogonal when not written), and the parameters Tiled
/// writes for it, each defaulting where it is left out as Tiled's do
/// (measured): a staggered or hexagonal map's stagger axis and index (`y`
/// and `odd`), a hexagonal map's side (0), an oblique map's skews (0). An
/// orientation, axis or index Tiled has none of, a negative side, or a skew
/// an `i32` does not hold fails naming it. A parameter of another grid's is
/// not read
pub(crate) fn grid_shape(path: &AssetPath, raw: &RawMap) -> Result<GridShape, AssetError> {
  let place = "map";
  let axis = || match raw.staggeraxis.as_deref() {
    None | Some("") | Some("y") => Ok(StaggerAxis::Y),
    Some("x") => Ok(StaggerAxis::X),
    Some(other) => Err(fail(
      path,
      place,
      format!("staggeraxis {other:?} is not one Tiled has (x or y)"),
    )),
  };
  let index = || match raw.staggerindex.as_deref() {
    None | Some("") | Some("odd") => Ok(StaggerIndex::Odd),
    Some("even") => Ok(StaggerIndex::Even),
    Some(other) => Err(fail(
      path,
      place,
      format!("staggerindex {other:?} is not one Tiled has (odd or even)"),
    )),
  };
  let skew = |value: i64, name: &str| {
    i32::try_from(value).map_err(|_| {
      fail(
        path,
        place,
        format!("{name} {value} is past the pixels a skew can be (a 32-bit integer)"),
      )
    })
  };
  Ok(match raw.orientation.as_str() {
    "" | "orthogonal" => GridShape::Orthogonal,
    "isometric" => GridShape::Isometric,
    "staggered" => GridShape::Staggered {
      axis: axis()?,
      index: index()?,
    },
    "hexagonal" => {
      let side = raw.hexsidelength.unwrap_or(0);
      if side < 0 {
        return Err(fail(
          path,
          place,
          format!("hexsidelength {side} is negative: a hexagon's side is 0 or more pixels"),
        ));
      }
      let Ok(side) = u32::try_from(side) else {
        return Err(fail(
          path,
          place,
          format!("hexsidelength {side} is past the pixels a side can be (a 32-bit integer)"),
        ));
      };
      GridShape::Hexagonal {
        axis: axis()?,
        index: index()?,
        side,
      }
    }
    "oblique" => GridShape::Oblique {
      skew: IVec2::new(skew(raw.skewx, "skewx")?, skew(raw.skewy, "skewy")?),
    },
    other => {
      return Err(fail(
        path,
        place,
        format!(
          "orientation {other:?} is not one Tiled has (orthogonal, isometric, staggered, hexagonal or oblique)"
        ),
      ));
    }
  })
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
  let shape = grid_shape(&path, &raw)?;
  if raw.tilewidth == 0 || raw.tileheight == 0 {
    return Err(fail(&path, place, "the tile size is 0"));
  }
  // Tiled draws orthogonal and oblique maps in their render order, and
  // every other grid right-down whatever it says (measured)
  let ordered = matches!(shape, GridShape::Orthogonal | GridShape::Oblique { .. });
  if ordered && !matches!(raw.renderorder.as_str(), "" | "right-down") {
    px.unread.note(
      format!(
        "the render order {:?} is not read: tiles draw right-down, which differs where tiles reach past their cells",
        raw.renderorder
      ),
      place,
    );
  }
  // An infinite map's size means nothing (Tiled writes 0): its layers'
  // chunks say where its cells are
  let size = if raw.infinite {
    UVec2::ZERO
  } else {
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
    UVec2::new(raw.width, raw.height)
  };
  let mut map = TileMap::new(size, UVec2::new(raw.tilewidth, raw.tileheight));
  map.infinite = raw.infinite;
  map.shape = shape;
  let cells = tile_cells(&path, &raw.layers, map.size, &map.grid(), 0)?;
  check_map_cells(&path, place, cells)?;
  map.class = raw.class.clone().filter(|c| !c.is_empty());
  map.parallax_origin = Vec2::new(raw.parallaxoriginx, raw.parallaxoriginy);
  if !map.parallax_origin.is_finite() {
    return Err(fail(
      &path,
      place,
      "the parallax origin is not a finite point",
    ));
  }
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
        TilesetSource::Asset(load_dep::<Tileset>(
          cx,
          &path,
          &place,
          written,
          &mut px.places,
        )?)
      }
      None => {
        let alignment = Alignment::parse(entry.inline.objectalignment.as_deref(), map.shape)
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
    objects: ObjectState::new(entries, map.grid()),
    path: path.clone(),
    size: map.size,
    infinite: map.infinite,
    grid: map.grid(),
    first_gids,
    next_id: max_layer_id(&raw.layers).checked_add(1),
    layers: Vec::new(),
    used_ids: Vec::new(),
    file_chunks: FastSet::default(),
  };
  let inherit = Inherit {
    offset: Vec2::ZERO,
    opacity: 1.0,
    tint: Color::WHITE,
    parallax: Vec2::ONE,
    visible: true,
    blend: BlendMode::Alpha,
    floor: None,
  };
  builder.layers(raw.layers, inherit, 0, cx)?;
  map.layers = builder.layers;
  map.used_ids = builder.used_ids;
  map.file_chunks = builder.file_chunks.into_iter().collect();
  map
    .file_chunks
    .sort_unstable_by_key(|chunk| (chunk.y, chunk.x));
  map.dependency_places = std::mem::take(&mut px.places);
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
  check_used_ids(map, cx)
}

/// Whether the tilesets hold the ids the map's layers use
pub(crate) fn check_used_ids(map: &TileMap, cx: &CheckContext<'_>) -> Result<(), AssetError> {
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

#[cfg(test)]
mod tests {
  use super::*;

  fn tile_layer(width: u32, height: u32) -> RawLayer {
    RawLayer {
      name: format!("{width} x {height}"),
      ty: "tilelayer".to_string(),
      width: Some(width),
      height: Some(height),
      ..RawLayer::default()
    }
  }

  /// The cells a map of `layers` counts against the caps, or why it fails
  fn count(layers: &[RawLayer]) -> Result<u64, String> {
    let path = AssetPath::parse("m.tmj").map_err(|e| e.report())?;
    let grid = MapGrid::new(GridShape::Orthogonal, UVec2::splat(16));
    let cells = tile_cells(&path, layers, UVec2::ZERO, &grid, 0).map_err(|e| e.report())?;
    check_map_cells(&path, "map", cells).map_err(|e| e.report())?;
    Ok(cells)
  }

  #[test]
  fn bounded_layers_count_the_engine_chunks_they_span_and_the_spec_s_maps_fit() {
    // An 8192 x 8192 layer is 2^26 cells either way, at a layer's cap
    assert_eq!(count(&[tile_layer(8192, 8192)]), Ok(1 << 26));
    // A 1024 x 1024 map of 128 layers is 2^27, at a map's cap; one more
    // layer passes it
    let mut many: Vec<RawLayer> = (0..128).map(|_| tile_layer(1024, 1024)).collect();
    assert_eq!(count(&many), Ok(1 << 27));
    many.push(tile_layer(1, 1));
    let error = count(&many).expect_err("a 129th layer");
    assert!(
      error.contains("layers hold 134218752 cells, past the 134217728 a map holds"),
      "{error}"
    );
    // A 10,000 x 64 runner takes 313 x 2 chunks of 1,024 cells
    assert_eq!(count(&[tile_layer(10_000, 64)]), Ok(313 * 2 * 1024));
    // a side short of a chunk takes the whole chunk
    assert_eq!(count(&[tile_layer(33, 1)]), Ok(2 * 1024));
    // A thin layer within a layer's cells spans more chunks than a layer
    // holds: 1 x 2^26 is 2^21 chunks, 2^31 cells
    let error = count(&[tile_layer(1, 1 << 26)]).expect_err("a thread");
    assert!(
      error.contains("layer \"1 x 67108864\"")
        && error.contains("2147483648 cells")
        && error.contains("past the 67108864 cells a layer holds"),
      "{error}"
    );
  }
}
