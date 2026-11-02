//! Streaming an infinite map's chunks. A [`MapStream`] on a map's root
//! gives it a generator; each frame `stream_maps` works out the chunks
//! within its load distance of the cameras and the [`StreamAnchor`]s, asks
//! for the missing ones on a thread of its own (where there is none, the
//! web, on the main thread within a time budget): the stream's
//! [`SavedChunks`] first, then the generator. It lands those that came
//! back nearest first within a budget a frame, and unloads the chunks past
//! its unload distance from every anchor, keeping those a player changed
//! and despawning the entities they own ([`ChunkOwned`]). Each arrival and
//! departure is a [`ChunkEvent`].

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use glam::{Affine2, IVec2, UVec2, Vec2};
use serde::ser::SerializeMap;

use super::edit::local_to_world;
use super::grid::CELLS;
use super::jobs::Worker;
use super::spawn::{
  LayerTiles, MapLayerRef, MapLayers, MapRoot, MapTile, TileLayerData, map_asset,
};
use super::{CHUNK_SIZE, LayerInfo, LayerKind, MapGrid, Tile, TileGrid, TileMap};
use crate::core::ecs::kernel::{Allow, Disabled, QueryState};
use crate::core::ecs::{
  Component, DetectChangesMut, Entity, Local, Message, Messages, With, World,
};
use crate::core::graphics::{Camera2d, CameraView};
use crate::core::hash::{FastMap, FastSet};
use crate::core::messages::register_message;
use crate::core::render::tiles::{moved, parallax};
use crate::core::ui::Rect;

/// The most chunks a streamed map has asked for and not landed yet: out on
/// the worker, or back and waiting to land
const MAX_OUT: usize = 64;

/// The most chunks a side of one anchor's load area spans: 4,096 chunks an
/// anchor, 32,768 px a side with 16 px cells. A load distance or a view
/// past it (a camera zoomed far out) loads the middle of its area, warned
/// once a map
pub(crate) const MAX_SPAN: i32 = 64;

/// How many chunks past every unload distance may wait to unload while a
/// stream lands chunks, whatever its unload budget: past that, a frame that
/// lands unloads too
pub(crate) const UNLOAD_BACKLOG: usize = 8;

/// The farthest a distance reaches, in pixels: one past it is read as it,
/// and one that is not a number as 0
const MAX_DISTANCE: f32 = 1.0e9;

/// The chunk worker's thread
const THREAD: &str = "ryo2d chunk builder";

/// Where each stream's epoch comes from, so a stream's late jobs are never
/// taken for those of the stream that replaced it
static EPOCHS: AtomicU64 = AtomicU64::new(1);

#[cfg(test)]
thread_local! {
  /// A test's own cap on the chunks a side of an anchor's load area, lower
  /// than `MAX_SPAN`, so a test of the cap asks for a few chunks past it
  pub(crate) static SPAN_CAP: std::cell::Cell<i32> = const { std::cell::Cell::new(MAX_SPAN) };
  /// Whether a test's chunk worker has no thread, as on the web
  pub(crate) static INLINE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// The most chunks a side of an anchor's load area spans: `MAX_SPAN` (in a
/// test, the thread's `SPAN_CAP`)
fn span_cap() -> i32 {
  #[cfg(test)]
  return SPAN_CAP.with(std::cell::Cell::get).max(1);
  #[cfg(not(test))]
  MAX_SPAN
}

/// One chunk's tiles, layer by layer, as a generator fills it: each layer
/// named as the map names it, its cells from (0, 0), the chunk's first
/// cell, to (31, 31). A layer given nothing lands empty.
///
/// Two are equal when they hold the same tiles in every layer. It
/// serializes as a map from each layer holding a tile to its cells, packed
/// as a Tiled layer's base64 zlib data packs them, so a save game holding
/// it stays small.
#[derive(Clone, Default)]
pub struct ChunkTiles {
  /// Each layer's cells, row by row, as `TileGrid::encode` gives them
  layers: Vec<(String, Box<[u32; CELLS]>)>,
}

impl ChunkTiles {
  /// Puts `tile` (`None` clears) at `cell` of the layer named `layer`, a
  /// cell within the chunk: each coordinate below 32. A cell outside it,
  /// or a tile that does not fit a cell (a tileset past 255, an id past
  /// 1,048,574), changes nothing
  pub fn set(&mut self, layer: &str, cell: UVec2, tile: Option<Tile>) {
    let Some(at) = place(cell) else {
      return;
    };
    let Some(raw) = TileGrid::encode(tile) else {
      return;
    };
    match self.layers.iter_mut().find(|(name, _)| name == layer) {
      Some((_, cells)) => cells[at] = raw,
      None if raw != 0 => {
        let mut cells = Box::new([0; CELLS]);
        cells[at] = raw;
        self.layers.push((layer.to_string(), cells));
      }
      None => {}
    }
  }

  /// The tile at `cell` of the layer named `layer`; `None` if empty or
  /// outside the chunk
  pub fn get(&self, layer: &str, cell: UVec2) -> Option<Tile> {
    let at = place(cell)?;
    let (_, cells) = self.layers.iter().find(|(name, _)| name == layer)?;
    TileGrid::decode(cells[at])
  }

  /// Whether no layer holds a tile
  pub fn is_empty(&self) -> bool {
    self.layers.iter().all(|(_, cells)| empty(cells))
  }

  /// Takes the cells of the layer named `layer` out
  fn take(&mut self, layer: &str) -> Option<Box<[u32; CELLS]>> {
    let at = self.layers.iter().position(|(name, _)| name == layer)?;
    Some(self.layers.swap_remove(at).1)
  }

  /// The cells of the layer named `layer`, if it has any
  fn cells(&self, layer: &str) -> Option<&[u32; CELLS]> {
    self
      .layers
      .iter()
      .find(|(name, _)| name == layer)
      .map(|(_, cells)| &**cells)
  }
}

impl PartialEq for ChunkTiles {
  fn eq(&self, other: &ChunkTiles) -> bool {
    let within = |a: &ChunkTiles, b: &ChunkTiles| {
      a.layers.iter().all(|(name, cells)| match b.cells(name) {
        Some(theirs) => **cells == *theirs,
        None => empty(cells),
      })
    };
    within(self, other) && within(other, self)
  }
}

/// Each layer's tiles with their cells, so a chunk prints as what it holds
impl std::fmt::Debug for ChunkTiles {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    let mut layers = f.debug_map();
    for (name, cells) in &self.layers {
      let tiles: Vec<(UVec2, Tile)> = cells
        .iter()
        .enumerate()
        .filter_map(|(at, &raw)| {
          let cell = UVec2::new(at as u32 % CHUNK_SIZE, at as u32 / CHUNK_SIZE);
          Some((cell, TileGrid::decode(raw)?))
        })
        .collect();
      layers.entry(name, &tiles);
    }
    layers.finish()
  }
}

/// Each layer holding a tile, by name, to its cells packed as base64 zlib
impl serde::Serialize for ChunkTiles {
  fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
    // Formats such as bincode and postcard write the length first
    let held = || self.layers.iter().filter(|(_, cells)| !empty(cells));
    let mut layers = serializer.serialize_map(Some(held().count()))?;
    for (name, cells) in held() {
      let packed = pack(cells).map_err(serde::ser::Error::custom)?;
      layers.serialize_entry(name, &packed)?;
    }
    layers.end()
  }
}

/// A map from layer names to packed cells, as [`ChunkTiles`] serializes:
/// data that is not base64, or does not inflate to a chunk's cells, is an
/// error naming its layer, and a cell holding no tile reads empty
impl<'de> serde::Deserialize<'de> for ChunkTiles {
  fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<ChunkTiles, D::Error> {
    let packed = BTreeMap::<String, String>::deserialize(deserializer)?;
    let mut tiles = ChunkTiles::default();
    for (name, text) in packed {
      let cells = unpack(&text)
        .map_err(|error| serde::de::Error::custom(format!("layer {name:?}: {error}")))?;
      if !empty(&cells) {
        tiles.layers.push((name, cells));
      }
    }
    Ok(tiles)
  }
}

/// A chunk's cells as base64 text of their little-endian bytes, zlib
/// compressed
fn pack(cells: &[u32; CELLS]) -> std::io::Result<String> {
  let mut zlib = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
  for cell in cells {
    zlib.write_all(&cell.to_le_bytes())?;
  }
  Ok(STANDARD.encode(zlib.finish()?))
}

/// The cells `pack` packed: an error for text that is not base64 or does
/// not inflate to exactly a chunk's cells (inflating no further than one
/// byte past them, so a small text cannot fill memory)
fn unpack(text: &str) -> Result<Box<[u32; CELLS]>, String> {
  let packed = STANDARD
    .decode(text.trim())
    .map_err(|error| format!("the base64 data is broken: {error}"))?;
  let want = CELLS * 4;
  let mut bytes = Vec::with_capacity(want);
  flate2::read::ZlibDecoder::new(packed.as_slice())
    .take(want as u64 + 1)
    .read_to_end(&mut bytes)
    .map_err(|error| format!("the zlib data is broken: {error}"))?;
  if bytes.len() > want {
    return Err(format!("it holds more than a chunk's {CELLS} cells"));
  }
  if bytes.len() < want {
    return Err(format!(
      "it holds {} bytes, short of a chunk's {CELLS} cells of 4",
      bytes.len()
    ));
  }
  let mut cells = Box::new([0; CELLS]);
  for (cell, raw) in cells.iter_mut().zip(bytes.as_chunks::<4>().0) {
    let raw = u32::from_le_bytes(*raw);
    if TileGrid::decode(raw).is_some() {
      *cell = raw;
    }
  }
  Ok(cells)
}

/// The chunks a player changed, kept while they are unloaded so they come
/// back as the player left them; a clone shares the same store. Give it to
/// a stream with [`MapStream::with_saved`]: when a chunk changed since it
/// landed unloads (an edit through `ctx.maps()`, or a class tile the game
/// despawned or an edit placed), its tiles go in, class tiles included,
/// and a chunk the store holds comes back from it instead of the
/// generator. The changed chunks still resident go in too as the map's
/// root goes (despawned, a scene's cleanup among them, or a `MapRoot` put
/// over it), so leaving the scene keeps the player's edits. Serialize it
/// with a save game, and give it back to the stream when the game loads;
/// [`MapsView::save_changed_chunks`](super::MapsView::save_changed_chunks)
/// puts in the changed chunks still resident first.
///
/// ```
/// use ryo2d::prelude::*;
///
/// /// What a save game holds of an endless world
/// #[derive(serde::Serialize, serde::Deserialize)]
/// struct WorldSave {
///   seed: u64,
///   chunks: SavedChunks,
/// }
///
/// fn endless(ctx: &mut SceneContext<'_>, map: Handle<TileMap>, save: &WorldSave) -> Entity {
///   // The stream keeps the chunks a player changed in the save's store
///   let stream =
///     MapStream::new(|_: ChunkRequest| ChunkTiles::default()).with_saved(save.chunks.clone());
///   ctx.world.spawn((MapRoot::new(map), stream)).id()
/// }
///
/// fn write(ctx: &mut SceneContext<'_>, map: Entity, save: &WorldSave) -> serde_json::Result<String> {
///   // The changed chunks round the player, still resident, go in first
///   ctx.maps().save_changed_chunks(map);
///   serde_json::to_string(save)
/// }
/// ```
#[derive(Clone, Default, serde::Deserialize)]
#[serde(from = "Vec<SavedChunk<ChunkTiles>>")]
pub struct SavedChunks {
  chunks: Arc<Mutex<FastMap<IVec2, ChunkTiles>>>,
}

/// A saved chunk as a save game holds it
#[derive(serde::Serialize, serde::Deserialize)]
struct SavedChunk<T> {
  chunk: IVec2,
  tiles: T,
}

impl SavedChunks {
  /// The store, whose lock no one keeps across a panic
  fn lock(&self) -> MutexGuard<'_, FastMap<IVec2, ChunkTiles>> {
    self.chunks.lock().unwrap_or_else(PoisonError::into_inner)
  }

  /// A copy of the tiles kept for `chunk`
  pub fn get(&self, chunk: IVec2) -> Option<ChunkTiles> {
    self.lock().get(&chunk).cloned()
  }

  /// Keeps `tiles` for `chunk`, over what was kept for it
  pub fn insert(&self, chunk: IVec2, tiles: ChunkTiles) {
    self.lock().insert(chunk, tiles);
  }

  /// Forgets `chunk`, so it comes back from the generator
  pub fn remove(&self, chunk: IVec2) -> Option<ChunkTiles> {
    self.lock().remove(&chunk)
  }

  /// Whether tiles are kept for `chunk`
  pub fn contains(&self, chunk: IVec2) -> bool {
    self.lock().contains_key(&chunk)
  }

  /// The chunks kept, by row then column: a copy, as the store is now
  pub fn keys(&self) -> Vec<IVec2> {
    let mut keys: Vec<IVec2> = self.lock().keys().copied().collect();
    keys.sort_unstable_by_key(|chunk| (chunk.y, chunk.x));
    keys
  }

  /// How many chunks are kept
  pub fn len(&self) -> usize {
    self.lock().len()
  }

  /// Whether no chunk is kept
  pub fn is_empty(&self) -> bool {
    self.lock().is_empty()
  }
}

/// The chunks kept, by row, each with its tiles
impl serde::Serialize for SavedChunks {
  fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
    let chunks = self.lock();
    let mut sorted: Vec<(&IVec2, &ChunkTiles)> = chunks.iter().collect();
    sorted.sort_unstable_by_key(|(chunk, _)| (chunk.y, chunk.x));
    serializer.collect_seq(
      sorted
        .into_iter()
        .map(|(&chunk, tiles)| SavedChunk { chunk, tiles }),
    )
  }
}

impl From<Vec<SavedChunk<ChunkTiles>>> for SavedChunks {
  fn from(chunks: Vec<SavedChunk<ChunkTiles>>) -> SavedChunks {
    let store = chunks
      .into_iter()
      .map(|saved| (saved.chunk, saved.tiles))
      .collect();
    SavedChunks {
      chunks: Arc::new(Mutex::new(store)),
    }
  }
}

/// How many chunks are kept
impl std::fmt::Debug for SavedChunks {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    f.debug_struct("SavedChunks")
      .field("chunks", &self.len())
      .finish()
  }
}

/// `cell`'s place among a chunk's cells; `None` outside the chunk
fn place(cell: UVec2) -> Option<usize> {
  (cell.x < CHUNK_SIZE && cell.y < CHUNK_SIZE).then(|| (cell.y * CHUNK_SIZE + cell.x) as usize)
}

/// Whether `cells` hold no tile
fn empty(cells: &[u32; CELLS]) -> bool {
  cells.iter().all(|&cell| cell == 0)
}

/// What a generator is asked for: one chunk of the map
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ChunkRequest {
  /// The chunk, signed as [`TileGrid::chunk_of`] gives it
  pub chunk: IVec2,
  /// Its first cell, the top-left by index: `chunk * 32`. A
  /// [`ChunkTiles`] cell `c` is the map's cell `first_cell + c`
  pub first_cell: IVec2,
  /// The streamed layers' grid (its first unbounded tile layer's): where
  /// each cell is in the layer's pixels, and which cells neighbour it
  pub grid: MapGrid,
}

/// Streams an infinite map's chunks: put it on the map's [`MapRoot`](super::MapRoot)
/// entity, with a map whose tile layers are unbounded
/// ([`TileMap::infinite`](super::TileMap::infinite), or a Tiled infinite
/// map).
///
/// The engine asks `generator` for each chunk within `load_distance` of a
/// camera's view or a [`StreamAnchor`], nearest first, on a thread of its
/// own: it must be `Send + Sync`, and should be a pure function of the
/// chunk (a seeded noise, say). A chunk the stream's [`SavedChunks`] hold
/// comes from them instead. Chunks land in every tile layer of the map at
/// most `land_per_frame` a frame, nearest first, each drawing, colliding,
/// casting shadows and spawning its class tiles as a landed map's do, and
/// each sending [`ChunkEvent::Loaded`]. A chunk farther than
/// `unload_distance` from every camera's view and anchor unloads, with the
/// entities it owns ([`ChunkOwned`]), sending [`ChunkEvent::Unloading`]
/// (carrying its tiles, and kept in the `SavedChunks`, when a player
/// changed it), at most `unload_per_frame` a frame, the farthest first, and
/// none in a frame that lands chunks while eight or fewer wait (so a frame
/// rarely both builds and takes down chunks, and at most eight linger past
/// the distance while chunks keep landing): after a teleport the chunks
/// left behind stay resident a few frames as they go, and the resident
/// chunks are within the distances again once they have. Distances are
/// measured in world pixels from the anchor (or the view) to the chunk,
/// along each axis. A camera's view loads and
/// keeps the chunks each tile layer shows, so a layer with parallax, or
/// offset from the map, also has the chunks loaded where it shows them,
/// however far from the view's own (those chunks are resident in every
/// layer). Only the chunks resident take
/// edits: `ctx.maps()` refuses a cell in any other. When the map's root
/// goes (despawned, a scene's cleanup among them, or a `MapRoot` put over
/// it), its changed resident chunks go into the `SavedChunks` as their
/// unloads would, so leaving the scene keeps the player's edits, and the
/// entities its chunks own go with it; no [`ChunkEvent::Unloading`] is
/// sent then.
///
/// A Tiled infinite map's own chunks are the hand-made part of the world:
/// every engine chunk (32 x 32 cells) that any layer's Tiled chunk touches
/// belongs to the file, its empty cells included, and so does every chunk
/// a map built in code holds in its asset. The generator never runs
/// there; those chunks are resident from the start, whatever the
/// distances, take edits, never unload and send no [`ChunkEvent`], so a
/// big hand-made area stays in memory. Their edits are kept through
/// [`MapsView::save_changed_chunks`](super::MapsView::save_changed_chunks)
/// and as the root goes, and a stream with those [`SavedChunks`] puts them
/// back as it starts. A
/// hot reload changes the file's chunks alone: the generated ones keep
/// their tiles, and a save that adds, drops or reshapes a tile layer
/// streams them again.
///
/// On native platforms a generator that panics on a chunk is caught: the
/// chunk lands empty, it is not asked for again, and the dev log warns once
/// a map. A web build aborts on a panic, so there it ends the game: keep
/// the generator from panicking. Where there are no threads (the web) the
/// generator runs on the main thread, `web_budget` of each frame.
///
/// ```
/// use ryo2d::prelude::*;
///
/// fn endless(ctx: &mut SceneContext<'_>, tileset: Handle<Tileset>) -> Entity {
///   let mut map = TileMap::infinite(UVec2::splat(16));
///   let set = map.add_tileset(TilesetSource::Asset(tileset));
///   map.add_tile_layer("ground");
///   let map = ctx.assets().add(map);
///   // A floor along the world's row 20, as far as anyone walks
///   let stream = MapStream::new(move |request: ChunkRequest| {
///     let mut tiles = ChunkTiles::default();
///     let row = 20 - request.first_cell.y;
///     if (0..32).contains(&row) {
///       for x in 0..32 {
///         tiles.set("ground", UVec2::new(x, row as u32), Some(Tile::new(set, 0)));
///       }
///     }
///     tiles
///   });
///   ctx.world.spawn((MapRoot::new(map), stream)).id()
/// }
/// ```
#[derive(Component, Clone)]
pub struct MapStream {
  /// Fills a chunk: called once for each chunk as it first comes near, and
  /// again when it comes back after unloading
  pub generator: Arc<dyn Fn(ChunkRequest) -> ChunkTiles + Send + Sync>,
  /// Chunks within this many pixels of a camera's view, or of an anchor,
  /// load
  pub load_distance: f32,
  /// Chunks farther than this from every camera's view and anchor unload;
  /// one below `load_distance` is raised to it, warned once
  pub unload_distance: f32,
  /// Whether every camera is an anchor (its view, grown by the distances)
  pub cameras: bool,
  /// Landed chunks a frame, at most (at least one)
  pub land_per_frame: u32,
  /// Unloaded chunks a frame, at most (at least one), the farthest from
  /// every camera's view and anchor first: a teleport's unloads spread over
  /// a few frames, the chunks waiting their turn still resident. A frame
  /// that lands chunks unloads none while eight or fewer wait, whatever
  /// this budget, so landing and unloading rarely share a frame's cost and
  /// at most eight linger past the distance while chunks keep landing
  pub unload_per_frame: u32,
  /// Where there are no threads (the web): main-thread time a frame for
  /// generating (at least one chunk a frame)
  pub web_budget: Duration,
  /// Where the chunks a player changed are kept as they unload, and asked
  /// for before the generator; `None` (the default) keeps none, so a
  /// changed chunk comes back as the generator makes it
  pub saved: Option<SavedChunks>,
}

impl MapStream {
  /// A stream of `generator`'s chunks: those within 512 px of a camera's
  /// view or an anchor load, those past 1,536 px of every one unload, two
  /// land and two unload a frame at most (each builds or takes down a
  /// chunk's collision and shadows in that frame; a frame landing chunks
  /// unloads none while eight or fewer wait), the web gives it 2 ms a
  /// frame, and it keeps no changed chunks
  pub fn new(generator: impl Fn(ChunkRequest) -> ChunkTiles + Send + Sync + 'static) -> MapStream {
    MapStream {
      generator: Arc::new(generator),
      load_distance: 512.0,
      unload_distance: 1536.0,
      cameras: true,
      land_per_frame: 2,
      unload_per_frame: 2,
      web_budget: Duration::from_millis(2),
      saved: None,
    }
  }

  /// This stream keeping the chunks a player changed in `saved` (a clone
  /// of the game's, which shares its store), and bringing back those it
  /// holds instead of the generator's
  pub fn with_saved(mut self, saved: SavedChunks) -> MapStream {
    self.saved = Some(saved);
    self
  }
}

impl std::fmt::Debug for MapStream {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    f.debug_struct("MapStream")
      .field("load_distance", &self.load_distance)
      .field("unload_distance", &self.unload_distance)
      .field("cameras", &self.cameras)
      .field("land_per_frame", &self.land_per_frame)
      .field("unload_per_frame", &self.unload_per_frame)
      .field("web_budget", &self.web_budget)
      .field("saved", &self.saved)
      .finish_non_exhaustive()
  }
}

/// Keeps the chunks near this entity loaded, on every streamed map: a
/// player, a companion, a far-off machine that must keep running
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct StreamAnchor;

/// What a streamed map has now, as its stream's last frame left it:
/// [`MapsView::stream_status`](super::MapsView::stream_status) reads it.
/// For a HUD, a loading screen's progress or a test: how many chunks are
/// resident, which of them the file holds and which the stream landed,
/// how many are on their way, and how many wait to unload
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct StreamStatus {
  /// Chunks resident: drawn, colliding and taking edits. The hand-made
  /// chunks and the generated ones together
  pub resident: usize,
  /// Of the resident chunks, those the stream landed (from its generator,
  /// or from its [`SavedChunks`]): the only ones that unload
  pub generated: usize,
  /// Of the resident chunks, the file's: every engine chunk a Tiled
  /// infinite map's chunks touch, and every chunk a map built in code
  /// holds in its asset. Resident from the start, never generated, never
  /// unloaded
  pub hand_made: usize,
  /// Chunks asked for and not landed yet: out on the chunk builder, or
  /// back and waiting their turn to land under `land_per_frame`. At most
  /// 64 are held at once; more wanted wait to be asked
  pub in_flight: usize,
  /// Resident chunks past every unload distance left for later frames by
  /// `unload_per_frame` (after a teleport, the old area going a few a
  /// frame), or by a frame that landed chunks
  pub draining: usize,
}

/// An entity that belongs to a streamed chunk: despawned (its children
/// with it) when the chunk unloads, disabled or not, and when its map's
/// root goes. Put it on the enemies and pickups a game spawns there on
/// [`ChunkEvent::Loaded`]; they go in the unloading frame, before the
/// game's update, and the next `Loaded` of the chunk places them afresh
#[derive(Component, Clone, Copy, Debug)]
pub struct ChunkOwned {
  /// The map's root entity
  pub map: Entity,
  /// The chunk, signed as [`TileGrid::chunk_of`] gives it
  pub chunk: IVec2,
}

/// A streamed map's chunk arriving or leaving. A map's frame lands before
/// it unloads, so in a frame that does both (after a teleport, or under a
/// camera faster than the land budget) its `Loaded` messages come before
/// its `Unloading` ones
#[derive(Message, Clone, Debug)]
pub enum ChunkEvent {
  /// It landed this frame: its tiles draw and collide, and its class tiles
  /// are in, so a game can place its own entities there
  Loaded { map: Entity, chunk: IVec2 },
  /// It unloaded this frame. By the time a game reads it the chunk is gone:
  /// its tiles, its class tiles and the entities it owned ([`ChunkOwned`])
  /// are, and its colliders and shadows go within the frame, so nothing of
  /// it can be read from the map. `changed` is what it held when a player
  /// changed it since it landed
  /// (an edit through `ctx.maps()`, or a class tile the game despawned or
  /// an edit placed): its tiles, class tiles included, as the stream's
  /// [`SavedChunks`] keep them. `None` for a chunk as it landed
  Unloading {
    map: Entity,
    chunk: IVec2,
    changed: Option<ChunkTiles>,
  },
}

/// What a streamed map has: kept on its root by `stream_maps`, and gone
/// with the root (a root put over starts afresh)
#[derive(Component)]
pub(crate) struct StreamState {
  /// Tells this stream's jobs from those of a stream the root had before
  epoch: u64,
  /// The chunks resident: the file's, and those landed and not unloaded
  /// since (in every unbounded tile layer of the map)
  resident: FastSet<IVec2>,
  /// The chunks that are the file's (`file_chunks`): resident from the
  /// stream's first frame, never asked for and never unloaded
  file: FastSet<IVec2>,
  /// The resident chunks that are not the file's: those landed, the only
  /// ones that unload, so a frame's look for chunks to unload never walks
  /// a big hand-made map's chunks
  generated: FastSet<IVec2>,
  /// How many resident chunks no anchor keeps were left for later frames
  /// at the last look, past the unload budget
  draining: usize,
  /// The version the stream started at: one of the file's chunks has
  /// changed since when its version passed it (and its last whole load's)
  since: u64,
  /// The chunks asked of the generator: each one's job, and the flag that
  /// calls it off once the chunk is no longer wanted
  out: FastMap<IVec2, (u64, Arc<AtomicBool>)>,
  /// The chunks generated and waiting to land
  ready: Vec<(IVec2, ChunkTiles)>,
  /// The chunks whose generator panicked: each lands empty, never asked
  /// for again
  panicked: FastSet<IVec2>,
  /// The resident chunks whose class tiles an edit placed, replaced or
  /// took since they landed, which their grids' versions do not show
  touched: FastSet<IVec2>,
  /// How many chunks within the load distance were not resident at the
  /// last look
  waiting: usize,
  warned: Warned,
}

/// What a stream has warned about, each once
#[derive(Default)]
struct Warned {
  distances: bool,
  span: bool,
  panic: bool,
  unbounded: bool,
  /// The layer names a generator gave that the map lacks
  layers: FastSet<String>,
}

impl StreamState {
  fn new() -> StreamState {
    StreamState {
      epoch: EPOCHS.fetch_add(1, Ordering::Relaxed),
      resident: FastSet::default(),
      file: FastSet::default(),
      generated: FastSet::default(),
      draining: 0,
      since: 0,
      out: FastMap::default(),
      ready: Vec::new(),
      panicked: FastSet::default(),
      touched: FastSet::default(),
      waiting: 0,
      warned: Warned::default(),
    }
  }
}

/// What a job hands back
enum Generated {
  Tiles(ChunkTiles),
  /// The chunk was no longer wanted when the job's turn came
  CalledOff,
}

/// A world rectangle the chunks near it load around: an anchor's point,
/// or a camera's view (and the point at its centre, which parallax is
/// measured from)
struct Anchor {
  min: Vec2,
  max: Vec2,
  center: Vec2,
  camera: bool,
}

/// An anchor in a map's space: the box holding its rectangle, in map
/// pixels, and the chunks it loads and keeps
struct Area {
  min: Vec2,
  max: Vec2,
  load: Option<(IVec2, IVec2)>,
  keep: Option<(IVec2, IVec2)>,
}

/// What the stream keeps between frames: the chunk worker, which map and
/// chunk each job out is for, and lists used each frame (kept, so a frame
/// that lands and unloads nothing allocates nothing)
#[derive(Default)]
pub(crate) struct StreamWork {
  worker: Option<Worker<Generated>>,
  /// Each job out: its map, that map's stream's epoch, and the chunk
  jobs: FastMap<u64, (Entity, u64, IVec2)>,
  anchors: Vec<Anchor>,
  maps: Vec<(Entity, MapStream)>,
  /// The map's unbounded tile layers
  layers: Vec<Entity>,
  /// Where those drawn apart from the map are placed: each distinct world
  /// placement and parallax (`apart_layers`)
  placements: Vec<(Affine2, Vec2)>,
  areas: Vec<Area>,
  wanted: Vec<((f32, f32), IVec2)>,
  gone: Vec<IVec2>,
  arrived: Vec<(u64, Option<Generated>)>,
  /// The class tiles of the chunks unloading
  class_tiles: Vec<ClassEntry>,
  /// Each map and chunk unloaded this frame, then the entities they owned
  unloaded: Vec<(Entity, IVec2)>,
  owned: Vec<Entity>,
}

/// A class tile listed in a chunk: its chunk, its layer's place among the
/// map's unbounded tile layers, its cell and its entity (which the game may
/// have despawned)
type ClassEntry = (IVec2, usize, IVec2, Entity);

/// The streamed maps: landed, and enabled
pub(crate) type StreamedMaps = QueryState<(Entity, &'static MapStream), With<MapLayers>>;

/// The entities owned by streamed chunks, disabled ones too
pub(crate) type OwnedEntities = QueryState<(Entity, &'static ChunkOwned), Allow<Disabled>>;

/// `PreUpdate`, after the maps land and reload and before class tiles
/// follow the chunks: each streamed map unloads the chunks past its unload
/// distance from every anchor (keeping those a player changed, and
/// despawning the entities they own), asks for the chunks within its load
/// distance it lacks, nearest first, and lands those that came back,
/// nearest first, within its budget. A frame where nothing lands or
/// unloads allocates nothing
pub(crate) fn stream_maps(
  world: &mut World,
  streamed: &mut StreamedMaps,
  anchors: &mut QueryState<Entity, With<StreamAnchor>>,
  cameras: &mut QueryState<&'static CameraView, With<Camera2d>>,
  owned: &mut OwnedEntities,
  mut work: Local<StreamWork>,
) {
  let work = &mut *work;
  work.maps.extend(
    streamed
      .iter(world)
      .map(|(map, stream)| (map, stream.clone())),
  );
  if work.maps.is_empty() && work.jobs.is_empty() {
    return;
  }
  work.anchors.clear();
  for entity in anchors.iter(world) {
    if let Some(at) = local_to_world(world, entity).map(|affine| affine.translation)
      && at.is_finite()
    {
      work.anchors.push(Anchor {
        min: at,
        max: at,
        center: at,
        camera: false,
      });
    }
  }
  for view in cameras.iter(world) {
    let bounds = view.bounds;
    let (min, max) = (
      Vec2::new(bounds.left, bounds.top),
      Vec2::new(bounds.right, bounds.bottom),
    );
    if min.is_finite() && max.is_finite() && view.center.is_finite() {
      work.anchors.push(Anchor {
        min,
        max,
        center: view.center,
        camera: true,
      });
    }
  }
  // Jobs of maps gone or no longer streamed come back to nothing
  work.route(world);
  let maps = std::mem::take(&mut work.maps);
  for (map, stream) in &maps {
    work.stream(world, *map, stream);
  }
  work.maps = maps;
  work.maps.clear();
  work.despawn_owned(world, owned);
}

impl StreamWork {
  /// One streamed map's frame: unload, ask, land
  fn stream(&mut self, world: &mut World, map: Entity, stream: &MapStream) {
    unbounded_layers(world, map, &mut self.layers);
    if world.get::<StreamState>(map).is_none() && !self.start(world, map, stream) {
      return;
    }
    if self.layers.is_empty() {
      if let Some(mut state) = world.get_mut::<StreamState>(map)
        && !std::mem::replace(&mut state.warned.unbounded, true)
      {
        crate::engine_warn!(
          module: "Tilemap",
          "map {map}: a MapStream streams the unbounded tile layers of an infinite map (TileMap::infinite, or a Tiled infinite map), and this map has none, so it streams nothing (this map warns once)"
        );
      }
      return;
    }
    // A map scaled to nothing streams nothing until it is not
    let Some(space) = map_space(world, map) else {
      return;
    };
    let (grid, chunk_px) = (space.grid, space.chunk_px);
    let load_px = distance(stream.load_distance);
    let mut unload_px = distance(stream.unload_distance);
    if unload_px < load_px {
      if let Some(mut state) = world.get_mut::<StreamState>(map)
        && !std::mem::replace(&mut state.warned.distances, true)
      {
        crate::engine_warn!(
          module: "Tilemap",
          "map {map}: its MapStream's unload_distance ({unload_px}) is below its load_distance ({load_px}), so chunks would unload as they land; it is raised to the load distance (this map warns once)"
        );
      }
      unload_px = load_px;
    }
    self.areas.clear();
    let (cap, mut capped) = (span_cap(), false);
    let reach = (load_px, unload_px, chunk_px);
    for anchor in &self.anchors {
      if anchor.camera && !stream.cameras {
        continue;
      }
      let area = area_of(anchor, space.to_map, reach, (cap, &mut capped));
      self.areas.push(area);
    }
    // A camera's view also loads and keeps the chunks each unbounded layer
    // drawn apart from the map shows there (its parallax, or an offset),
    // as the renderer places it: residency is shared by the layers, so
    // their areas join the map's. A map with no such layer has none
    apart_layers(world, &self.layers, space.to_world, &mut self.placements);
    if stream.cameras && !self.placements.is_empty() {
      let origin = world
        .get::<MapRoot>(map)
        .and_then(|root| map_asset(world, root))
        .map_or(Vec2::ZERO, |asset| asset.parallax_origin);
      for anchor in self.anchors.iter().filter(|anchor| anchor.camera) {
        for &layer in &self.placements {
          if let Some(to_layer) = layer_view((space.to_world, origin), layer, anchor.center) {
            let area = area_of(anchor, to_layer, reach, (cap, &mut capped));
            self.areas.push(area);
          }
        }
      }
    }
    if capped
      && let Some(mut state) = world.get_mut::<StreamState>(map)
      && !std::mem::replace(&mut state.warned.span, true)
    {
      crate::engine_warn!(
        module: "Tilemap",
        "map {map}: an anchor's load area spans more than {cap} chunks a side (its stream's load_distance, or a camera zoomed far out), so only the middle {cap} x {cap} load (this map warns once)"
      );
    }
    self.call_off(world, map);
    self.request(world, map, stream, (grid, chunk_px));
    self.route(world);
    let landed = self.land(world, map, stream.land_per_frame, chunk_px);
    self.unload(world, map, stream, chunk_px, landed > 0);
  }

  /// A stream's first frame on `map` (its unbounded tile layers found):
  /// its state, the file's chunks resident from the start, and those of
  /// them the stream's `SavedChunks` hold put back as a player left them.
  /// False for a root gone
  fn start(&mut self, world: &mut World, map: Entity, stream: &MapStream) -> bool {
    let mut state = StreamState::new();
    if !self.layers.is_empty() {
      if let Some(asset) = world
        .get::<MapRoot>(map)
        .and_then(|root| map_asset(world, root))
      {
        state.file = file_chunks(asset);
      }
      state.resident.extend(state.file.iter().copied());
      state.since = self
        .layers
        .iter()
        .filter_map(|&layer| world.get::<TileLayerData>(layer))
        .map(|data| data.tiles.version())
        .max()
        .unwrap_or(0);
    }
    let mut kept: Vec<(IVec2, ChunkTiles)> = match &stream.saved {
      Some(saved) if !state.file.is_empty() => saved
        .lock()
        .iter()
        .filter(|(chunk, _)| state.file.contains(chunk))
        .map(|(&chunk, tiles)| (chunk, tiles.clone()))
        .collect(),
      _ => Vec::new(),
    };
    let Ok(mut entity) = world.get_entity_mut(map) else {
      return false;
    };
    entity.insert(state);
    kept.sort_unstable_by_key(|(chunk, _)| (chunk.y, chunk.x));
    for (chunk, tiles) in kept {
      self.put_chunk(world, map, chunk, tiles);
    }
    true
  }

  /// Calls off what `map`'s stream asked for or holds past every anchor's
  /// unload distance, so none of it lands
  fn call_off(&mut self, world: &mut World, map: Entity) {
    let areas = &self.areas;
    let kept = |chunk: IVec2| {
      areas
        .iter()
        .any(|area| area.keep.is_some_and(|range| within(range, chunk)))
    };
    let Some(mut state) = world.get_mut::<StreamState>(map) else {
      return;
    };
    let state = &mut *state;
    state.out.retain(|&chunk, (_, called_off)| {
      let keep = kept(chunk);
      if !keep {
        called_off.store(true, Ordering::Relaxed);
      }
      keep
    });
    state.ready.retain(|(chunk, _)| kept(*chunk));
  }

  /// Unloads `map`'s chunks past every anchor's unload distance (never the
  /// file's), at most the stream's `unload_per_frame`, the farthest first
  /// (the others wait, resident, for the next frames), telling the game and
  /// keeping in its `SavedChunks` those a player changed since they landed.
  /// A frame that `landed` chunks unloads none, unless more than
  /// `UNLOAD_BACKLOG` chunks wait: unloading waits out a burst of landings,
  /// so the two rarely build and take down chunks in one frame, and no more
  /// than that many linger past the distance while chunks keep landing
  fn unload(
    &mut self,
    world: &mut World,
    map: Entity,
    stream: &MapStream,
    chunk_px: Vec2,
    landed: bool,
  ) {
    let areas = &self.areas;
    let kept = |chunk: IVec2| {
      areas
        .iter()
        .any(|area| area.keep.is_some_and(|range| within(range, chunk)))
    };
    self.gone.clear();
    {
      let Some(state) = world.get::<StreamState>(map) else {
        return;
      };
      self.gone.extend(
        state
          .generated
          .iter()
          .copied()
          .filter(|chunk| !kept(*chunk)),
      );
    }
    let budget = stream.unload_per_frame.max(1) as usize;
    let backlog = self.gone.len() > UNLOAD_BACKLOG;
    let budget = if landed && !backlog { 0 } else { budget };
    let draining = self.gone.len().saturating_sub(budget);
    if budget == 0 {
      self.gone.clear();
    } else if draining > 0 {
      let StreamWork { gone, areas, .. } = self;
      gone.sort_unstable_by(|a, b| {
        nearer(nearness(areas, *b, chunk_px), nearness(areas, *a, chunk_px))
          .then((a.y, a.x).cmp(&(b.y, b.x)))
      });
      gone.truncate(budget);
    }
    if let Some(mut state) = world.get_mut::<StreamState>(map) {
      state.draining = draining;
    }
    self.unload_gone(world, map, stream.saved.as_ref(), true);
  }

  /// Unloads the chunks `gone` lists from `map`: each leaves every
  /// unbounded tile layer, and the entities it owns go after every map's
  /// unloads. A chunk a player changed since it landed is kept in `saved`;
  /// with `tell`, each sends `ChunkEvent::Unloading` first
  fn unload_gone(
    &mut self,
    world: &mut World,
    map: Entity,
    saved: Option<&SavedChunks>,
    tell: bool,
  ) {
    if self.gone.is_empty() {
      return;
    }
    self.gone.sort_unstable_by_key(|chunk| (chunk.y, chunk.x));
    let read = tell || saved.is_some();
    if read {
      class_tiles_in(world, &self.layers, &self.gone, &mut self.class_tiles);
    }
    for &chunk in &self.gone {
      if read {
        // What it holds now, class tiles still in, if a player changed it
        let tiles = entries_of(&self.class_tiles, chunk);
        let (touched, since) = world.get::<StreamState>(map).map_or((false, 0), |state| {
          (state.touched.contains(&chunk), state.since_for(chunk))
        });
        let changed = changed_since_landing(world, &self.layers, chunk, (touched, since), tiles)
          .then(|| holdings(world, &self.layers, chunk, tiles));
        if let (Some(saved), Some(tiles)) = (saved, &changed) {
          saved.insert(chunk, tiles.clone());
        }
        if tell {
          send(
            world,
            ChunkEvent::Unloading {
              map,
              chunk,
              changed,
            },
          );
        }
      }
      self.unloaded.push((map, chunk));
      for &layer in &self.layers {
        if let Some(mut data) = world.get_mut::<TileLayerData>(layer)
          && data
            .bypass_change_detection()
            .tiles
            .unload_chunk(chunk)
            .is_some()
        {
          data.set_changed();
        }
      }
    }
    // Each grid's extent worked out once for all the chunks that went
    for &layer in &self.layers {
      if let Some(mut data) = world.get_mut::<TileLayerData>(layer) {
        data.bypass_change_detection().tiles.settle_extent();
      }
    }
    if let Some(mut state) = world.get_mut::<StreamState>(map) {
      for chunk in &self.gone {
        state.resident.remove(chunk);
        state.generated.remove(chunk);
        state.touched.remove(chunk);
      }
    }
  }

  /// Despawns the entities owned by the chunks unloaded this frame, their
  /// children with them
  fn despawn_owned(&mut self, world: &mut World, owned: &mut OwnedEntities) {
    if self.unloaded.is_empty() {
      return;
    }
    let key = |map: Entity, chunk: IVec2| (map, chunk.y, chunk.x);
    self
      .unloaded
      .sort_unstable_by_key(|&(map, chunk)| key(map, chunk));
    let unloaded = &self.unloaded;
    self.owned.extend(
      owned
        .iter(world)
        .filter(|(_, of)| {
          unloaded
            .binary_search_by_key(&key(of.map, of.chunk), |&(map, chunk)| key(map, chunk))
            .is_ok()
        })
        .map(|(entity, _)| entity),
    );
    for entity in self.owned.drain(..) {
      let _ = world.try_despawn(entity);
    }
    self.unloaded.clear();
  }

  /// Asks the generator for `map`'s chunks within the load distance that
  /// it lacks, nearest first, while it has fewer than `MAX_OUT` out (with
  /// no thread, within the stream's time budget)
  fn request(
    &mut self,
    world: &mut World,
    map: Entity,
    stream: &MapStream,
    (grid, chunk_px): (MapGrid, Vec2),
  ) {
    self.wanted.clear();
    let (epoch, room) = {
      let Some(mut state) = world.get_mut::<StreamState>(map) else {
        return;
      };
      let state = &mut *state;
      let mut waiting = 0;
      for (i, area) in self.areas.iter().enumerate() {
        let Some((first, last)) = area.load else {
          continue;
        };
        for y in first.y..=last.y {
          for x in first.x..=last.x {
            let chunk = IVec2::new(x, y);
            // Each chunk counted once, by the first area wanting it
            let earlier = self.areas[..i]
              .iter()
              .any(|area| area.load.is_some_and(|range| within(range, chunk)));
            if earlier || state.resident.contains(&chunk) {
              continue;
            }
            waiting += 1;
            if state.out.contains_key(&chunk) || state.ready.iter().any(|(c, _)| *c == chunk) {
              continue;
            }
            if state.panicked.contains(&chunk) {
              // Empty, unless the player changed it and it was kept
              let kept = stream.saved.as_ref().and_then(|saved| saved.get(chunk));
              state.ready.push((chunk, kept.unwrap_or_default()));
              continue;
            }
            self
              .wanted
              .push((nearness(&self.areas, chunk, chunk_px), chunk));
          }
        }
      }
      state.waiting = waiting;
      let held = state.out.len() + state.ready.len();
      (state.epoch, MAX_OUT.saturating_sub(held))
    };
    if room == 0 || self.wanted.is_empty() {
      return;
    }
    self
      .wanted
      .sort_unstable_by(|a, b| nearer(a.0, b.0).then((a.1.y, a.1.x).cmp(&(b.1.y, b.1.x))));
    let worker = self.worker.get_or_insert_with(new_worker);
    let started = web_time::Instant::now();
    for &(_, chunk) in self.wanted.iter().take(room) {
      let request = ChunkRequest {
        chunk,
        first_cell: chunk * CHUNK_SIZE as i32,
        grid,
      };
      let called_off = Arc::new(AtomicBool::new(false));
      let (generator, off) = (Arc::clone(&stream.generator), Arc::clone(&called_off));
      let saved = stream.saved.clone();
      let job = worker.send(move || {
        if off.load(Ordering::Relaxed) {
          return Generated::CalledOff;
        }
        // A chunk the player changed comes back as they left it
        match saved.and_then(|saved| saved.get(request.chunk)) {
          Some(tiles) => Generated::Tiles(tiles),
          None => Generated::Tiles(generator(request)),
        }
      });
      self.jobs.insert(job, (map, epoch, chunk));
      if let Some(mut state) = world.get_mut::<StreamState>(map) {
        state.out.insert(chunk, (job, called_off));
      }
      if worker.runs_inline() && started.elapsed() >= stream.web_budget {
        break;
      }
    }
  }

  /// Hands each job that came back to its map, if its map still streams
  /// and still wants the chunk: a generator that panicked leaves the chunk
  /// empty, never asked for again, warned once a map
  fn route(&mut self, world: &mut World) {
    let StreamWork {
      worker,
      jobs,
      arrived,
      ..
    } = self;
    let Some(worker) = worker else {
      return;
    };
    arrived.extend(worker.finished());
    for (job, made) in arrived.drain(..) {
      let Some((map, epoch, chunk)) = jobs.remove(&job) else {
        continue;
      };
      let Some(mut state) = world.get_mut::<StreamState>(map) else {
        continue;
      };
      let wanted = state.out.get(&chunk).map(|(id, _)| *id) == Some(job);
      if state.epoch != epoch || !wanted {
        continue;
      }
      state.out.remove(&chunk);
      match made {
        Some(Generated::Tiles(tiles)) => state.ready.push((chunk, tiles)),
        Some(Generated::CalledOff) => {}
        None => {
          state.panicked.insert(chunk);
          state.ready.push((chunk, ChunkTiles::default()));
          if !std::mem::replace(&mut state.warned.panic, true) {
            crate::engine_warn!(
              module: "Tilemap",
              "map {map}: its stream's generator panicked at chunk ({}, {}), which lands empty and is not asked for again (this map warns once)",
              chunk.x,
              chunk.y
            );
          }
        }
      }
    }
  }

  /// Lands up to `budget` (at least one) of `map`'s generated chunks,
  /// nearest the anchors first
  fn land(&mut self, world: &mut World, map: Entity, budget: u32, chunk_px: Vec2) -> usize {
    {
      let Some(mut state) = world.get_mut::<StreamState>(map) else {
        return 0;
      };
      if state.ready.is_empty() {
        return 0;
      }
      // The nearest last, to pop
      let areas = &self.areas;
      state.ready.sort_unstable_by(|(a, _), (b, _)| {
        nearer(nearness(areas, *b, chunk_px), nearness(areas, *a, chunk_px))
      });
    }
    let mut landed = 0;
    for _ in 0..budget.max(1) {
      let next = world
        .get_mut::<StreamState>(map)
        .and_then(|mut state| state.ready.pop());
      let Some((chunk, tiles)) = next else {
        break;
      };
      self.land_chunk(world, map, chunk, tiles);
      landed += 1;
    }
    landed
  }

  /// Lands `chunk` on `map` (`put_chunk`), resident from now, and tells
  /// the game
  fn land_chunk(&mut self, world: &mut World, map: Entity, chunk: IVec2, tiles: ChunkTiles) {
    if !self.put_chunk(world, map, chunk, tiles) {
      return;
    }
    if let Some(mut state) = world.get_mut::<StreamState>(map) {
      state.resident.insert(chunk);
      state.generated.insert(chunk);
    }
    send(world, ChunkEvent::Loaded { map, chunk });
  }

  /// Puts `chunk` into every unbounded tile layer of `map` whole, each
  /// layer the cells `tiles` gives it by name (an empty chunk where it
  /// gives none), warning once of each name the map has no tile layer of.
  /// False for a map that does not stream
  fn put_chunk(
    &mut self,
    world: &mut World,
    map: Entity,
    chunk: IVec2,
    mut tiles: ChunkTiles,
  ) -> bool {
    for &layer in &self.layers {
      let cells = world
        .get::<MapLayerRef>(layer)
        .and_then(|of| tiles.take(&of.name))
        .unwrap_or_else(|| Box::new([0; CELLS]));
      if let Some(mut data) = world.get_mut::<TileLayerData>(layer)
        && data
          .bypass_change_detection()
          .tiles
          .load_chunk(chunk, cells)
      {
        data.set_changed();
      }
    }
    let Some(mut state) = world.get_mut::<StreamState>(map) else {
      return false;
    };
    for (name, cells) in &tiles.layers {
      if !empty(cells) && !state.warned.layers.contains(name) {
        state.warned.layers.insert(name.clone());
        crate::engine_warn!(
          module: "Tilemap",
          "map {map}: its stream's generator gave tiles for the layer {name:?}, which is not one of the map's tile layers, so they are left out (this layer warns once)"
        );
      }
    }
    true
  }
}

impl StreamState {
  /// The version `chunk`'s changes are told from besides its last whole
  /// load: the file's chunks', 0 for every other (made by its load)
  fn since_for(&self, chunk: IVec2) -> u64 {
    if self.file.contains(&chunk) {
      self.since
    } else {
      0
    }
  }
}

/// The chunks a streamed map leaves to its asset: every engine chunk any
/// of a Tiled infinite map's layers' Tiled chunks touches, and every chunk
/// the asset's unbounded tile layers hold (so a map built in code keeps
/// what its asset was given). The generator never runs there, and they
/// stay resident
pub(crate) fn file_chunks(map: &TileMap) -> FastSet<IVec2> {
  let mut chunks: FastSet<IVec2> = map.file_chunks.iter().copied().collect();
  for layer in &map.layers {
    if let LayerKind::Tiles { tiles, .. } = &layer.kind
      && tiles.bounds().is_none()
    {
      chunks.extend(tiles.chunk_coords());
    }
  }
  chunks
}

/// Readies the streamed map `map` for a reload whose file holds the
/// chunks `file` (`file_chunks`), before its layers take the file's cells,
/// which a reload then gives the file's chunks alone: the file's chunks
/// the save dropped unload without a message (the stream fills them as it
/// fills any other); the generated chunks the file now holds unload as
/// the stream unloads a chunk, not kept (the file's cells take their
/// place); and when the save adds, takes away or reshapes one of the
/// map's unbounded tile layers (`reshaped`), every generated chunk unloads
/// as the stream unloads a chunk, kept when a player changed it, to stream
/// back into the layers as they are now. The file's chunks are then
/// `file`, every one resident, and none is asked for
pub(crate) fn before_reload(world: &mut World, map: Entity, file: &FastSet<IVec2>, reshaped: bool) {
  let Some(state) = world.get::<StreamState>(map) else {
    return;
  };
  let (mut dropped, mut claimed, mut restream) = (Vec::new(), Vec::new(), Vec::new());
  for &chunk in &state.resident {
    match (state.file.contains(&chunk), file.contains(&chunk)) {
      (true, false) => dropped.push(chunk),
      (false, true) => claimed.push(chunk),
      (false, false) if reshaped => restream.push(chunk),
      _ => {}
    }
  }
  let saved = world
    .get::<MapStream>(map)
    .and_then(|stream| stream.saved.clone());
  let mut work = StreamWork::default();
  unbounded_layers(world, map, &mut work.layers);
  work.gone = restream;
  work.unload_gone(world, map, saved.as_ref(), true);
  work.gone = claimed;
  work.unload_gone(world, map, None, true);
  work.gone = dropped;
  work.unload_gone(world, map, None, false);
  let mut owned = world.query_filtered::<(Entity, &ChunkOwned), Allow<Disabled>>();
  work.despawn_owned(world, &mut owned);
  if let Some(mut state) = world.get_mut::<StreamState>(map) {
    let state = &mut *state;
    state.out.retain(|chunk, (_, called_off)| {
      let keep = !file.contains(chunk);
      if !keep {
        called_off.store(true, Ordering::Relaxed);
      }
      keep
    });
    state.ready.retain(|(chunk, _)| !file.contains(chunk));
    state.resident.extend(file.iter().copied());
    state.file.clone_from(file);
  }
}

/// `map`'s unbounded tile layers, bottom first, into `out`
fn unbounded_layers(world: &World, map: Entity, out: &mut Vec<Entity>) {
  out.clear();
  if let Some(layers) = world.get::<MapLayers>(map) {
    let unbounded = |layer: &Entity| {
      world
        .get::<TileLayerData>(*layer)
        .is_some_and(|data| data.tiles.bounds().is_none())
    };
    out.extend(layers.0.iter().copied().filter(unbounded));
  }
}

/// The class tiles `layers` list in `chunks` (sorted by row), into `out`
/// by chunk, then layer, then cell: one look at each layer's list
fn class_tiles_in(world: &World, layers: &[Entity], chunks: &[IVec2], out: &mut Vec<ClassEntry>) {
  out.clear();
  let row = |chunk: &IVec2| (chunk.y, chunk.x);
  for (at, &layer) in layers.iter().enumerate() {
    let Some(table) = world.get::<LayerTiles>(layer) else {
      continue;
    };
    for (&cell, &entity) in &table.0 {
      let chunk = TileGrid::chunk_of(cell);
      if chunks.binary_search_by_key(&row(&chunk), row).is_ok() {
        out.push((chunk, at, cell, entity));
      }
    }
  }
  out.sort_unstable_by_key(|&(chunk, at, cell, _)| (row(&chunk), at, row(&cell)));
}

/// The class tiles of `chunk` among `entries` (sorted by chunk)
fn entries_of(entries: &[ClassEntry], chunk: IVec2) -> &[ClassEntry] {
  let key = (chunk.y, chunk.x);
  let from = entries.partition_point(|entry| (entry.0.y, entry.0.x) < key);
  let to = entries.partition_point(|entry| (entry.0.y, entry.0.x) <= key);
  &entries[from..to]
}

/// The class tile `entity`, if it still lives at `cell`: the game has not
/// despawned it or moved it to another cell
fn living(world: &World, entity: Entity, cell: IVec2) -> Option<&MapTile> {
  world
    .get::<MapTile>(entity)
    .filter(|tile| tile.cell == cell)
}

/// Whether `chunk` changed since it landed (one of the file's chunks:
/// since the version `since` its stream started at): an edit changed its
/// cells in
/// one of `layers`, or placed, replaced or took one of its class tiles
/// (`touched`), or the game despawned one of the class tiles listed in it
/// (`tiles`, a collected coin)
fn changed_since_landing(
  world: &World,
  layers: &[Entity],
  chunk: IVec2,
  (touched, since): (bool, u64),
  tiles: &[ClassEntry],
) -> bool {
  touched
    || layers.iter().any(|&layer| {
      world
        .get::<TileLayerData>(layer)
        .is_some_and(|data| data.tiles.changed_since(chunk, since))
    })
    || tiles
      .iter()
      .any(|&(_, _, cell, entity)| living(world, entity, cell).is_none())
}

/// What `chunk` holds now, as a generator would give it: each of `layers`'
/// cells under its name, with its class tiles still living (`tiles`) back
/// in their cells. A layer whose name an earlier one has is left out, as a
/// landing gives the chunk's cells of a name to the first layer so named
fn holdings(world: &World, layers: &[Entity], chunk: IVec2, tiles: &[ClassEntry]) -> ChunkTiles {
  let mut held = ChunkTiles::default();
  // A chunk holds cells of `i32`: its first is one
  let first = chunk * CHUNK_SIZE as i32;
  for (at, &layer) in layers.iter().enumerate() {
    let (Some(of), Some(data)) = (
      world.get::<MapLayerRef>(layer),
      world.get::<TileLayerData>(layer),
    ) else {
      continue;
    };
    if held.layers.iter().any(|(name, _)| *name == of.name) {
      continue;
    }
    let mut cells = Box::new([0; CELLS]);
    if let Some(grid) = data.tiles.chunk_cells(chunk) {
      cells.copy_from_slice(grid);
    }
    for &(_, _, cell, entity) in tiles.iter().filter(|entry| entry.1 == at) {
      let Some(tile) = living(world, entity, cell) else {
        continue;
      };
      if let (Some(slot), Some(raw)) = (
        place((cell - first).as_uvec2()),
        TileGrid::encode(Some(tile.tile)),
      ) {
        cells[slot] = raw;
      }
    }
    held.layers.push((of.name.clone(), cells));
  }
  held.layers.retain(|(_, cells)| !empty(cells));
  held
}

/// Notes an edit placing, replacing or taking a class tile at `cell` of
/// `map`: on a streamed map, a change of its chunk, if resident, that its
/// grids' versions do not show
pub(crate) fn note_class_edit(world: &mut World, map: Entity, cell: IVec2) {
  let chunk = TileGrid::chunk_of(cell);
  if let Some(mut state) = world.get_mut::<StreamState>(map)
    && state.resident.contains(&chunk)
  {
    state.touched.insert(chunk);
  }
}

/// Puts each chunk resident on the streamed map `map` that a player
/// changed since it landed into its stream's [`SavedChunks`], as its unload
/// would; how many it put (0 for a map with no stream, or whose stream
/// keeps no chunks). `MapsView::save_changed_chunks`, and the root's
/// discard hook as the map goes
pub(crate) fn save_changed(world: &World, map: Entity) -> usize {
  let Some(saved) = world
    .get::<MapStream>(map)
    .and_then(|stream| stream.saved.as_ref())
  else {
    return 0;
  };
  let Some(state) = world.get::<StreamState>(map) else {
    return 0;
  };
  let mut layers = Vec::new();
  unbounded_layers(world, map, &mut layers);
  let mut chunks: Vec<IVec2> = state.resident.iter().copied().collect();
  chunks.sort_unstable_by_key(|chunk| (chunk.y, chunk.x));
  let mut entries = Vec::new();
  class_tiles_in(world, &layers, &chunks, &mut entries);
  let mut put = 0;
  for &chunk in &chunks {
    let tiles = entries_of(&entries, chunk);
    let touched = (state.touched.contains(&chunk), state.since_for(chunk));
    if changed_since_landing(world, &layers, chunk, touched, tiles) {
      saved.insert(chunk, holdings(world, &layers, chunk, tiles));
      put += 1;
    }
  }
  put
}

/// Despawns the entities owned by `map`'s chunks (`ChunkOwned`), as its
/// root goes
pub(crate) fn despawn_chunk_owned(world: &mut World, map: Entity) {
  let owned: Vec<Entity> = world
    .query_filtered::<(Entity, &ChunkOwned), Allow<Disabled>>()
    .iter(world)
    .filter(|(_, of)| of.map == map)
    .map(|(entity, _)| entity)
    .collect();
  for entity in owned {
    let _ = world.try_despawn(entity);
  }
}

/// The chunk worker: a thread of its own (in a test that asks, none)
fn new_worker() -> Worker<Generated> {
  #[cfg(test)]
  if INLINE.with(std::cell::Cell::get) {
    return Worker::without_thread();
  }
  Worker::named(THREAD)
}

/// Writes a chunk message
fn send(world: &mut World, event: ChunkEvent) {
  register_message::<ChunkEvent>(world);
  world.resource_mut::<Messages<ChunkEvent>>().write(event);
}

/// A distance in pixels as the stream reads it: not a number is 0, and it
/// lies from 0 to `MAX_DISTANCE`
fn distance(pixels: f32) -> f32 {
  if pixels.is_nan() {
    0.0
  } else {
    pixels.clamp(0.0, MAX_DISTANCE)
  }
}

/// A streamed map's space
struct MapSpace {
  /// Its tile layers' grid (its first unbounded layer's)
  grid: MapGrid,
  /// A chunk's side in its pixels
  chunk_px: Vec2,
  /// Where it lies in the world: its root's transform and its ancestors'
  /// as they are now
  to_world: Affine2,
  /// The world to its space: `to_world` undone
  to_map: Affine2,
}

/// `map`'s space; `None` for a map with no unbounded tile layer, or scaled
/// to nothing
fn map_space(world: &World, map: Entity) -> Option<MapSpace> {
  let grid = world
    .get::<MapLayers>(map)?
    .0
    .iter()
    .filter_map(|&layer| world.get::<TileLayerData>(layer))
    .find(|data| data.tiles.bounds().is_none())?
    .grid;
  let cell = grid.cell;
  if cell.cmpeq(UVec2::ZERO).any() {
    return None;
  }
  let to_world = local_to_world(world, map).unwrap_or(Affine2::IDENTITY);
  let det = to_world.matrix2.determinant();
  if !det.is_finite() || det.abs() < 1e-12 {
    return None;
  }
  Some(MapSpace {
    grid,
    chunk_px: cell.as_vec2() * CHUNK_SIZE as f32,
    to_world,
    to_map: to_world.inverse(),
  })
}

/// `anchor` taken into a space by `to_space` (the map's, or a layer's as a
/// camera sees it): the box holding it there, and the chunks it loads and
/// keeps, its rectangle grown by the load and unload distances
/// (`reach`, with a chunk's side); a load area past `cap` chunks a side
/// loads its middle, noted in `capped`
fn area_of(
  anchor: &Anchor,
  to_space: Affine2,
  (load_px, unload_px, chunk_px): (f32, f32, Vec2),
  (cap, capped): (i32, &mut bool),
) -> Area {
  let (min, max) = map_rect(to_space, anchor.min, anchor.max);
  let grown = |by: f32| map_rect(to_space, anchor.min - by, anchor.max + by);
  let load = chunk_range(grown(load_px), chunk_px).map(|range| cap_range(range, cap, capped));
  let keep = chunk_range(grown(unload_px), chunk_px);
  Area {
    min,
    max,
    load,
    keep,
  }
}

/// The placements of `layers` drawn apart from their map, which lies at
/// `to_world`, into `out`: each distinct pair of a layer's world placement
/// (the map's transform and its own, which holds its offset) and its
/// parallax, leaving out those with no parallax placed as the map is
fn apart_layers(
  world: &World,
  layers: &[Entity],
  to_world: Affine2,
  out: &mut Vec<(Affine2, Vec2)>,
) {
  out.clear();
  for &layer in layers {
    let factor = world
      .get::<LayerInfo>(layer)
      .map_or(Vec2::ONE, |info| info.parallax);
    let Some(placed) = local_to_world(world, layer) else {
      continue;
    };
    let as_map = factor == Vec2::ONE && placed == to_world;
    if !as_map && !out.contains(&(placed, factor)) {
      out.push((placed, factor));
    }
  }
}

/// The world to the space of a layer placed at `placed` with parallax
/// `factor` (one of `apart_layers`), as a camera centred on `center` shows
/// it: the renderer's mapping, through `render::tiles::parallax` from the
/// map's placement and parallax origin (`map`). `None` for a layer scaled
/// to nothing
fn layer_view(
  map: (Affine2, Vec2),
  (placed, factor): (Affine2, Vec2),
  center: Vec2,
) -> Option<Affine2> {
  // As the renderer's views: a layer with no parallax draws unmoved
  let offset = if factor == Vec2::ONE {
    Vec2::ZERO
  } else {
    parallax(map, factor, center)?
  };
  let placed = moved(placed, offset);
  let det = placed.matrix2.determinant();
  (det.is_finite() && det.abs() >= 1e-12).then(|| placed.inverse())
}

/// The box in map space holding the world rectangle `min..max`
fn map_rect(to_map: Affine2, min: Vec2, max: Vec2) -> (Vec2, Vec2) {
  let corners = [min, Vec2::new(max.x, min.y), max, Vec2::new(min.x, max.y)];
  corners
    .iter()
    .fold((Vec2::INFINITY, Vec2::NEG_INFINITY), |(lo, hi), &corner| {
      let at = to_map.transform_point2(corner);
      (lo.min(at), hi.max(at))
    })
}

/// The chunks, first and last (both included), overlapping the map-space
/// box `min..max` (touching its edge is not overlapping), within the
/// chunks of `i32`'s cells; on an axis where the box has no size (a point,
/// or a line), the chunk holding it there, so a point on a chunk boundary
/// is in the chunk past it. `None` for a box that is not a number
fn chunk_range((min, max): (Vec2, Vec2), chunk_px: Vec2) -> Option<(IVec2, IVec2)> {
  let (a, b) = (min / chunk_px, max / chunk_px);
  if !(a.is_finite() && b.is_finite()) {
    return None;
  }
  let (lo, hi) = (
    TileGrid::chunk_of(IVec2::MIN),
    TileGrid::chunk_of(IVec2::MAX),
  );
  let first = a.floor();
  let last = Vec2::select(a.cmpeq(b), first, b.ceil() - 1.0);
  // `as` saturates, and the clamp keeps to the chunks a map holds
  let (first, last) = (
    first.as_ivec2().clamp(lo, hi),
    last.as_ivec2().clamp(lo, hi),
  );
  first.cmple(last).all().then_some((first, last))
}

/// `range` cut to the middle `cap` chunks a side where it spans more,
/// noting in `capped` that it did
fn cap_range((first, last): (IVec2, IVec2), cap: i32, capped: &mut bool) -> (IVec2, IVec2) {
  let mut side = |lo: i32, hi: i32| {
    if i64::from(hi) - i64::from(lo) < i64::from(cap) {
      return (lo, hi);
    }
    *capped = true;
    let middle = (i64::from(lo) + i64::from(hi)).div_euclid(2);
    let from = middle - i64::from(cap - 1) / 2;
    // Within the range, so within `i32`
    (from as i32, (from + i64::from(cap) - 1) as i32)
  };
  let (x, y) = (side(first.x, last.x), side(first.y, last.y));
  (IVec2::new(x.0, y.0), IVec2::new(x.1, y.1))
}

/// Whether `chunk` is in `range`, both ends included
fn within((first, last): (IVec2, IVec2), chunk: IVec2) -> bool {
  chunk.cmpge(first).all() && chunk.cmple(last).all()
}

/// How near `chunk` is to the anchors: its gap to the nearest anchor's
/// box (0 inside it), then the distance from its middle to that box's
/// middle, in map pixels
fn nearness(areas: &[Area], chunk: IVec2, chunk_px: Vec2) -> (f32, f32) {
  let min = chunk.as_vec2() * chunk_px;
  let max = min + chunk_px;
  let middle = (min + max) * 0.5;
  areas
    .iter()
    .map(|area| {
      let gap = (area.min - max)
        .max(min - area.max)
        .max(Vec2::ZERO)
        .length();
      (gap, middle.distance((area.min + area.max) * 0.5))
    })
    .min_by(|a, b| nearer(*a, *b))
    .unwrap_or((f32::INFINITY, f32::INFINITY))
}

/// Which of two nearnesses is nearer
fn nearer(a: (f32, f32), b: (f32, f32)) -> std::cmp::Ordering {
  a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1))
}

/// Where a map takes edits: `None` for a map with no `MapStream`, which
/// takes them in any chunk; on a streamed map, the chunks resident on it
/// (none before its stream first looks)
pub(crate) fn resident_chunks(world: &World, map: Entity) -> Option<Resident<'_>> {
  world.get::<MapStream>(map)?;
  Some(Resident(
    world.get::<StreamState>(map).map(|state| &state.resident),
  ))
}

/// The chunks resident on a streamed map
pub(crate) struct Resident<'a>(Option<&'a FastSet<IVec2>>);

impl Resident<'_> {
  /// Whether `chunk` is resident
  pub(crate) fn holds(&self, chunk: IVec2) -> bool {
    self.0.is_some_and(|resident| resident.contains(&chunk))
  }

  /// The resident chunks from `first` to `last` (both included), by row:
  /// found from the resident chunks, however far apart the two are
  pub(crate) fn within(&self, first: IVec2, last: IVec2) -> Vec<IVec2> {
    let mut found: Vec<IVec2> = self
      .0
      .into_iter()
      .flatten()
      .copied()
      .filter(|&chunk| within((first, last), chunk))
      .collect();
    found.sort_unstable_by_key(|chunk| (chunk.y, chunk.x));
    found
  }
}

/// The status of the streamed map `map` (`MapsView::stream_status`):
/// `None` with no `MapStream`, or before its stream's first frame
pub(crate) fn stream_status(world: &World, map: Entity) -> Option<StreamStatus> {
  world.get::<MapStream>(map)?;
  let state = world.get::<StreamState>(map)?;
  Some(StreamStatus {
    resident: state.resident.len(),
    generated: state.generated.len(),
    hand_made: state.file.len(),
    in_flight: state.out.len() + state.ready.len(),
    draining: state.draining,
  })
}

/// Whether every chunk overlapping `area` (world pixels) is resident on the
/// streamed map `map` (an area of no size on an axis: the chunks holding
/// it there); false for a map not streamed or not landed
pub(crate) fn stream_ready(world: &World, map: Entity, area: Rect) -> bool {
  if world.get::<MapStream>(map).is_none() {
    return false;
  }
  let Some(state) = world.get::<StreamState>(map) else {
    return false;
  };
  let Some(MapSpace {
    chunk_px, to_map, ..
  }) = map_space(world, map)
  else {
    return false;
  };
  let (a, b) = (
    Vec2::new(area.x, area.y),
    Vec2::new(area.x + area.width, area.y + area.height),
  );
  if !(a.is_finite() && b.is_finite()) {
    return false;
  }
  let Some((first, last)) = chunk_range(map_rect(to_map, a.min(b), a.max(b)), chunk_px) else {
    return false;
  };
  // More chunks than are resident cannot all be: never walk past them
  let count =
    (i64::from(last.x) - i64::from(first.x) + 1) * (i64::from(last.y) - i64::from(first.y) + 1);
  if count > state.resident.len() as i64 {
    return false;
  }
  (first.y..=last.y).all(|y| (first.x..=last.x).all(|x| state.resident.contains(&IVec2::new(x, y))))
}

#[cfg(test)]
mod tests;
