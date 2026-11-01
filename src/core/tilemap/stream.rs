//! Streaming an infinite map's chunks. A [`MapStream`] on a map's root
//! gives it a generator; each frame `stream_maps` works out the chunks
//! within its load distance of the cameras and the [`StreamAnchor`]s, asks
//! the generator for the missing ones on a thread of its own (where there
//! is none, the web, on the main thread within a time budget), lands those
//! that came back nearest first within a budget a frame, and unloads the
//! chunks past its unload distance from every anchor. Each arrival and
//! departure is a [`ChunkEvent`].

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

use glam::{Affine2, IVec2, UVec2, Vec2};

use super::edit::local_to_world;
use super::grid::CELLS;
use super::jobs::Worker;
use super::spawn::{MapLayerRef, MapLayers, TileLayerData};
use super::{CHUNK_SIZE, Tile, TileGrid};
use crate::core::ecs::kernel::QueryState;
use crate::core::ecs::{
  Component, DetectChangesMut, Entity, Local, Message, Messages, With, World,
};
use crate::core::graphics::{Camera2d, CameraView};
use crate::core::hash::{FastMap, FastSet};
use crate::core::messages::register_message;
use crate::core::ui::Rect;

/// The most chunks a streamed map has asked for and not landed yet: out on
/// the worker, or back and waiting to land
const MAX_OUT: usize = 64;

/// The most chunks a side of one anchor's load area spans: 4,096 chunks an
/// anchor, 32,768 px a side with 16 px cells. A load distance or a view
/// past it (a camera zoomed far out) loads the middle of its area, warned
/// once a map
pub(crate) const MAX_SPAN: i32 = 64;

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
/// Two are equal when they hold the same tiles in every layer.
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

/// `cell`'s place among a chunk's cells; `None` outside the chunk
fn place(cell: UVec2) -> Option<usize> {
  (cell.x < CHUNK_SIZE && cell.y < CHUNK_SIZE).then(|| (cell.y * CHUNK_SIZE + cell.x) as usize)
}

/// Whether `cells` hold no tile
fn empty(cells: &[u32; CELLS]) -> bool {
  cells.iter().all(|&cell| cell == 0)
}

/// What a generator is asked for: one chunk of the map
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChunkRequest {
  /// The chunk, signed as [`TileGrid::chunk_of`] gives it
  pub chunk: IVec2,
  /// Its first cell, the top-left: `chunk * 32`. A [`ChunkTiles`] cell
  /// `c` is the map's cell `first_cell + c`
  pub first_cell: IVec2,
  /// A cell's size in pixels
  pub tile_size: UVec2,
}

/// Streams an infinite map's chunks: put it on the map's [`MapRoot`](super::MapRoot)
/// entity, with a map whose tile layers are unbounded
/// ([`TileMap::infinite`](super::TileMap::infinite)).
///
/// The engine asks `generator` for each chunk within `load_distance` of a
/// camera's view or a [`StreamAnchor`], nearest first, on a thread of its
/// own: it must be `Send + Sync`, and should be a pure function of the
/// chunk (a seeded noise, say). Chunks land in every tile layer of the map
/// at most `land_per_frame` a frame, nearest first, each drawing,
/// colliding, casting shadows and spawning its class tiles as a landed
/// map's do, and each sending [`ChunkEvent::Loaded`]. A chunk farther than
/// `unload_distance` from every camera's view and anchor unloads, sending
/// [`ChunkEvent::Unloading`] first. Distances are measured in world pixels
/// from the anchor (or the view) to the chunk, along each axis. Only the
/// chunks resident take edits: `ctx.maps()` refuses a cell in any other.
///
/// A generator that panics on a chunk is caught: the chunk lands empty, it
/// is not asked for again, and the dev log warns once a map. Where there
/// are no threads (the web) the generator runs on the main thread,
/// `web_budget` of each frame.
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
  /// Where there are no threads (the web): main-thread time a frame for
  /// generating (at least one chunk a frame)
  pub web_budget: Duration,
}

impl MapStream {
  /// A stream of `generator`'s chunks: those within 512 px of a camera's
  /// view or an anchor load, those past 1,536 px of every one unload, four
  /// land a frame, and the web gives it 2 ms a frame
  pub fn new(generator: impl Fn(ChunkRequest) -> ChunkTiles + Send + Sync + 'static) -> MapStream {
    MapStream {
      generator: Arc::new(generator),
      load_distance: 512.0,
      unload_distance: 1536.0,
      cameras: true,
      land_per_frame: 4,
      web_budget: Duration::from_millis(2),
    }
  }
}

impl std::fmt::Debug for MapStream {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    f.debug_struct("MapStream")
      .field("load_distance", &self.load_distance)
      .field("unload_distance", &self.unload_distance)
      .field("cameras", &self.cameras)
      .field("land_per_frame", &self.land_per_frame)
      .field("web_budget", &self.web_budget)
      .finish_non_exhaustive()
  }
}

/// Keeps the chunks near this entity loaded, on every streamed map: a
/// player, a companion, a far-off machine that must keep running
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct StreamAnchor;

/// A streamed map's chunk arriving or leaving
#[derive(Message, Clone, Debug)]
pub enum ChunkEvent {
  /// It landed this frame: its tiles draw and collide, and its class tiles
  /// are in, so a game can place its own entities there
  Loaded { map: Entity, chunk: IVec2 },
  /// Sent before the chunk goes. `changed` is for its tiles when an edit
  /// changed it since it landed; the engine does not track a chunk's
  /// changes, so it is `None`
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
  /// The chunks landed and not unloaded since, in every unbounded tile
  /// layer of the map
  resident: FastSet<IVec2>,
  /// The chunks asked of the generator: each one's job, and the flag that
  /// calls it off once the chunk is no longer wanted
  out: FastMap<IVec2, (u64, Arc<AtomicBool>)>,
  /// The chunks generated and waiting to land
  ready: Vec<(IVec2, ChunkTiles)>,
  /// The chunks whose generator panicked: each lands empty, never asked
  /// for again
  panicked: FastSet<IVec2>,
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
      out: FastMap::default(),
      ready: Vec::new(),
      panicked: FastSet::default(),
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
/// or a camera's view
struct Anchor {
  min: Vec2,
  max: Vec2,
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
  areas: Vec<Area>,
  wanted: Vec<((f32, f32), IVec2)>,
  gone: Vec<IVec2>,
  arrived: Vec<(u64, Option<Generated>)>,
}

/// The streamed maps: landed, and enabled
pub(crate) type StreamedMaps = QueryState<(Entity, &'static MapStream), With<MapLayers>>;

/// `PreUpdate`, after the maps land and reload and before class tiles
/// follow the chunks: each streamed map unloads the chunks past its unload
/// distance from every anchor, asks for the chunks within its load distance
/// it lacks, nearest first, and lands those that came back, nearest first,
/// within its budget. A frame where nothing lands or unloads allocates
/// nothing
pub(crate) fn stream_maps(
  world: &mut World,
  streamed: &mut StreamedMaps,
  anchors: &mut QueryState<Entity, With<StreamAnchor>>,
  cameras: &mut QueryState<&'static CameraView, With<Camera2d>>,
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
    if min.is_finite() && max.is_finite() {
      work.anchors.push(Anchor {
        min,
        max,
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
}

impl StreamWork {
  /// One streamed map's frame: unload, ask, land
  fn stream(&mut self, world: &mut World, map: Entity, stream: &MapStream) {
    if world.get::<StreamState>(map).is_none() {
      let Ok(mut entity) = world.get_entity_mut(map) else {
        return;
      };
      entity.insert(StreamState::new());
    }
    self.layers.clear();
    if let Some(layers) = world.get::<MapLayers>(map) {
      let unbounded = |layer: &Entity| {
        world
          .get::<TileLayerData>(*layer)
          .is_some_and(|data| data.grid.bounds().is_none())
      };
      self
        .layers
        .extend(layers.0.iter().copied().filter(unbounded));
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
    let Some((cell, chunk_px, to_map)) = map_space(world, map) else {
      return;
    };
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
    for anchor in &self.anchors {
      if anchor.camera && !stream.cameras {
        continue;
      }
      let (min, max) = map_rect(to_map, anchor.min, anchor.max);
      let grown = |by: f32| map_rect(to_map, anchor.min - by, anchor.max + by);
      let load = chunk_range(grown(load_px), chunk_px);
      let load = load.map(|range| cap_range(range, cap, &mut capped));
      let keep = chunk_range(grown(unload_px), chunk_px);
      self.areas.push(Area {
        min,
        max,
        load,
        keep,
      });
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
    self.unload(world, map);
    self.request(world, map, stream, (cell, chunk_px));
    self.route(world);
    self.land(world, map, stream.land_per_frame, chunk_px);
  }

  /// Unloads `map`'s chunks past every anchor's unload distance, telling
  /// the game first, and calls off what it asked for or holds there
  fn unload(&mut self, world: &mut World, map: Entity) {
    let areas = &self.areas;
    let kept = |chunk: IVec2| {
      areas
        .iter()
        .any(|area| area.keep.is_some_and(|range| within(range, chunk)))
    };
    self.gone.clear();
    {
      let Some(mut state) = world.get_mut::<StreamState>(map) else {
        return;
      };
      let state = &mut *state;
      self
        .gone
        .extend(state.resident.iter().copied().filter(|&chunk| !kept(chunk)));
      state.out.retain(|&chunk, (_, called_off)| {
        let keep = kept(chunk);
        if !keep {
          called_off.store(true, Ordering::Relaxed);
        }
        keep
      });
      state.ready.retain(|(chunk, _)| kept(*chunk));
    }
    if self.gone.is_empty() {
      return;
    }
    self.gone.sort_unstable_by_key(|chunk| (chunk.y, chunk.x));
    for &chunk in &self.gone {
      send(
        world,
        ChunkEvent::Unloading {
          map,
          chunk,
          changed: None,
        },
      );
      for &layer in &self.layers {
        if let Some(mut data) = world.get_mut::<TileLayerData>(layer)
          && data
            .bypass_change_detection()
            .grid
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
        data.bypass_change_detection().grid.settle_extent();
      }
    }
    if let Some(mut state) = world.get_mut::<StreamState>(map) {
      for chunk in &self.gone {
        state.resident.remove(chunk);
      }
    }
  }

  /// Asks the generator for `map`'s chunks within the load distance that
  /// it lacks, nearest first, while it has fewer than `MAX_OUT` out (with
  /// no thread, within the stream's time budget)
  fn request(
    &mut self,
    world: &mut World,
    map: Entity,
    stream: &MapStream,
    (cell, chunk_px): (UVec2, Vec2),
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
              state.ready.push((chunk, ChunkTiles::default()));
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
        tile_size: cell,
      };
      let called_off = Arc::new(AtomicBool::new(false));
      let (generator, off) = (Arc::clone(&stream.generator), Arc::clone(&called_off));
      let job = worker.send(move || {
        if off.load(Ordering::Relaxed) {
          Generated::CalledOff
        } else {
          Generated::Tiles(generator(request))
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
  fn land(&mut self, world: &mut World, map: Entity, budget: u32, chunk_px: Vec2) {
    {
      let Some(mut state) = world.get_mut::<StreamState>(map) else {
        return;
      };
      if state.ready.is_empty() {
        return;
      }
      // The nearest last, to pop
      let areas = &self.areas;
      state.ready.sort_unstable_by(|(a, _), (b, _)| {
        nearer(nearness(areas, *b, chunk_px), nearness(areas, *a, chunk_px))
      });
    }
    for _ in 0..budget.max(1) {
      let next = world
        .get_mut::<StreamState>(map)
        .and_then(|mut state| state.ready.pop());
      let Some((chunk, tiles)) = next else {
        break;
      };
      self.land_chunk(world, map, chunk, tiles);
    }
  }

  /// Puts `chunk` into every unbounded tile layer of `map`, each layer the
  /// cells `tiles` gives it by name (an empty chunk where it gives none),
  /// and tells the game
  fn land_chunk(&mut self, world: &mut World, map: Entity, chunk: IVec2, mut tiles: ChunkTiles) {
    for &layer in &self.layers {
      let cells = world
        .get::<MapLayerRef>(layer)
        .and_then(|of| tiles.take(&of.name))
        .unwrap_or_else(|| Box::new([0; CELLS]));
      if let Some(mut data) = world.get_mut::<TileLayerData>(layer)
        && data.bypass_change_detection().grid.load_chunk(chunk, cells)
      {
        data.set_changed();
      }
    }
    let Some(mut state) = world.get_mut::<StreamState>(map) else {
      return;
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
    state.resident.insert(chunk);
    send(world, ChunkEvent::Loaded { map, chunk });
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

/// A streamed map's space: its tile layers' cell size (its first unbounded
/// layer's), a chunk's side in its pixels, and the world to its space (its
/// root's transform and its ancestors' as they are now, undone). `None` for
/// a map with no unbounded tile layer, or scaled to nothing
fn map_space(world: &World, map: Entity) -> Option<(UVec2, Vec2, Affine2)> {
  let cell = world
    .get::<MapLayers>(map)?
    .0
    .iter()
    .filter_map(|&layer| world.get::<TileLayerData>(layer))
    .find(|data| data.grid.bounds().is_none())?
    .cell_size;
  if cell.cmpeq(UVec2::ZERO).any() {
    return None;
  }
  let to_world = local_to_world(world, map).unwrap_or(Affine2::IDENTITY);
  let det = to_world.matrix2.determinant();
  if !det.is_finite() || det.abs() < 1e-12 {
    return None;
  }
  Some((cell, cell.as_vec2() * CHUNK_SIZE as f32, to_world.inverse()))
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
/// chunks of `i32`'s cells; `None` for none, or a box that is not a number
fn chunk_range((min, max): (Vec2, Vec2), chunk_px: Vec2) -> Option<(IVec2, IVec2)> {
  let (a, b) = (min / chunk_px, max / chunk_px);
  if !(a.is_finite() && b.is_finite()) {
    return None;
  }
  let (lo, hi) = (
    TileGrid::chunk_of(IVec2::MIN),
    TileGrid::chunk_of(IVec2::MAX),
  );
  // `as` saturates, and the clamp keeps to the chunks a map holds
  let first = a.floor().as_ivec2().clamp(lo, hi);
  let last = (b.ceil() - 1.0).as_ivec2().clamp(lo, hi);
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

/// Whether every chunk overlapping `area` (world pixels) is resident on the
/// streamed map `map`; false for a map not streamed or not landed
pub(crate) fn stream_ready(world: &World, map: Entity, area: Rect) -> bool {
  if world.get::<MapStream>(map).is_none() {
    return false;
  }
  let Some(state) = world.get::<StreamState>(map) else {
    return false;
  };
  let Some((_, chunk_px, to_map)) = map_space(world, map) else {
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
    return true;
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
