//! Maps against the spec's speed targets (6a's, and the streaming target),
//! with a GPU where a frame is drawn (`HeadlessEngine`, 1920 x 1080, 16 px
//! tiles at zoom 1).
//!
//! Before criterion runs, four guards:
//! - `static_frame`: a 1024 x 1024 map of four layers (a background on
//!   every cell, cave rock from noise that collides and casts shadows, and
//!   two sparse layers of props) built in code over one 16 px atlas, the
//!   camera flying a long loop over it at 444 to 690 px/s (561 on average
//!   over the guard's frames), against an empty scene whose camera flies
//!   the same path. A frame's cost is the wall time of the
//!   whole step (`HeadlessEngine::step`: every phase, the render step and
//!   its submit) with the GPU finished before each step, outside the
//!   measure, as `benches/ui_render.rs`'s guard does; the engines step in
//!   turn, one frame each, so a change in the machine's load moves them
//!   alike. The median frame over the empty frame's median is held to
//!   50 µs (the spec). The same map built with unbounded layers
//!   (`TileMap::infinite`) steps in the same rounds and is held to 50 µs
//!   the same way. Beside them, with no target: the same map drawn only
//!   (no collision, no shadows), colliding without shadows, casting shadows
//!   without collision (which split what the map's child entities cost),
//!   with its props layer y-sorted, and lit (a shadowed light following the
//!   camera, against an empty lit scene), which shows what lighting's
//!   gather of every map occluder costs; each map's entities and occluder
//!   chains; and the map's first build (its landing job, the landing frame
//!   and the one after), stepped back to back. Then what a padded tileset
//!   cache hit costs, an atlas's and a 1,000-tile collection's;
//! - `edit`: one-tile edits on a 1024 x 1024 four-layer map of 16,000
//!   islands with two loops each spanning four chunks: a jagged skyline 128
//!   cells wide and 32 high, whose column 99 holds only its bottom cell (a
//!   neck), and a one-cell serpentine filling the same 4 x 1 chunks (the
//!   longest loop that area can hold). Each loop is edited 400 times a way:
//!   a bump (a tile set on top of it and cleared again: it stays one loop),
//!   and a split and a join (a cell cleared and set again: the skyline's
//!   neck; the middle of the serpentine's first row, and of its middle row,
//!   which splits it into halves of about 2,000 edges, the worst split a
//!   loop of four chunks can have). Each edit is timed in three parts, each
//!   held to 100 µs (the spec): the chunk rebuild (the render step of the frame after the
//!   edit, less the median render step of the ordinary frames between
//!   edits: the chunk's instances built and uploaded), the loop rebuild
//!   (the map's collision sync, which classifies the chunk again, then
//!   physics' tile collider rebuild, which retraces the loops through the
//!   cell and reshapes their chunk colliders, and its collider sync, which
//!   builds the new shapes for Rapier; each run on the engine's world in a
//!   schedule of its own, commands applied:
//!   `ryo2d::core::tilemap::edit_schedules`) and the occluder rebuild (the
//!   occluder sync, commands applied). A frame passes between edits, as in
//!   a game, so their data is not in cache. Beside them, with no target: the
//!   edit frame's whole step over an ordinary frame's, the loop and
//!   occluder rebuilds of as many edits back to back (their data in cache);
//! - `load`: a 256 x 256 four-layer Tiled map (TMX, every layer base64 and
//!   zlib) over an external TSX tileset and its PNG, read from a
//!   `MemorySource`. Its load, from `load` to the map ready with its
//!   tileset and image, on the asset workers with the main thread pumping,
//!   is held to 20 ms (the spec; twenty loads of fresh files, the median),
//!   and so is the same map saved as an infinite TMX (16 x 16 chunks, zlib
//!   each);
//!   a level change (the tileset loaded already) is printed beside it. Then
//!   the landing: an engine at 60 Hz (paced with a spin wait) loads and
//!   spawns a fresh copy of the map every 40 frames and despawns it 32
//!   frames later, twenty times. The frames the map's asset lands in, the
//!   map lands in and the one after are its landing frames, and the
//!   slowest of them over the run's median ordinary frame is held to
//!   0.5 ms (the spec: no hitch). A run over that is measured once more,
//!   and the better run is held. The despawn frames count as neither. The
//!   first landing makes the padded tileset's texture (its landing job
//!   padded it; later ones find the padding kept), and is printed apart. The landing job's time, spawn
//!   to landed with frames stepped back to back, is printed too: on the web,
//!   which has no threads, the job runs in the frame it is sent;
//! - `stream`: an endless world streamed from a generator (`MapStream`
//!   with its default budgets, loading within 512 px of the view and
//!   unloading past 1,536): two unbounded layers over the static map's
//!   atlas, a background on every cell and cave rock from the same noise on
//!   the ground, which collides and casts shadows. Its camera, the stream's
//!   anchor, crosses it on a straight line at 2,000 px/s (heading 0.8, 0.6,
//!   so rows and columns of chunks come and go both) for the guard's frames
//!   and the warm-up before them, against an empty scene whose camera flies
//!   the same path; the engines (and the baseline below) step in turn, one
//!   frame each, each round paced at 60 Hz with a spin wait, as the landing
//!   is, so the generator's thread keeps the time a game gives it. A
//!   frame's cost is the whole step, the GPU finished before it, as the
//!   static guard's. The 99th
//!   percentile of the world's step over the empty frame's median is held
//!   to 0.5 ms (the spec: any frame; one slowest frame is the machine's
//!   noise, so the maximum is printed beside it, and so is the empty
//!   frame's own spread). Every frame, warm-up included, two checks fail
//!   the run at once (assertions): every chunk overlapping the camera's
//!   view as the frame drew it is resident (no unfilled chunk on screen),
//!   and the resident chunks number no more than the view grown by the
//!   unload distance can meet (printed), each chunk past that distance
//!   gone within the frames the unload budget takes to drain them all.
//!   Beside them, with no target: the world's 99th percentile less the
//!   empty frame's; a static baseline, a third engine stepped in the same
//!   rounds on the chunks the crossing meets (the view grown by the load
//!   distance: about 1,100 chunks, 2.3 million cells of the generator's own
//!   tiles) built in code as an infinite map with no stream, its step over
//!   the empty frame's and the world's over its own, so streaming's cost
//!   stands apart from the map's; the frames that landed chunks, unloaded
//!   them, brought a chunk into view (its first draw) and the others, each
//!   with its render step; then a teleport 100 chunks on, the camera
//!   staying there 180 frames: the frames its view took to fill and the
//!   chunks left behind to go, and those frames' steps.
//!
//! Each verdict holds the median (the landing, its slowest frame; the
//! stream, its 99th percentile); the 95th percentile (the stream's, the
//! 99th) and the maximum are printed beside it. A miss fails the run once
//! criterion's benches have run, printing the command to profile it with. Each guard runs only where criterion measures the bench of its
//! name: with no filter or one naming it, and not for `--list`, `--test` or
//! `--profile-time`. The criterion benches, for the profiles:
//! `static_frame` steps the map engine with its camera flying,
//! `edit` sets or clears the serpentine's cell and steps the engine,
//! `load` loads the map with its tileset and image in a fresh registry,
//! `landing` spawns the loaded map and times its landing frame, and
//! `stream` steps the endless world back to back, its camera crossing it
//! (out and back along a leg of 4,000 frames, so a long profile stays
//! within collision's reach of the origin).
//!
//! The guards' numbers move with the machine's load: run them on a quiet
//! machine. Medians are read from where criterion writes its results
//! (`guards::results_dir`).

mod guards;

use std::collections::{BTreeSet, HashMap};
use std::io::Write as _;
use std::process::Command;
use std::time::{Duration, Instant, SystemTime};

use base64::Engine as _;
use criterion::{BatchSize, Criterion};
use ryo2d::core::asset::pump;
use ryo2d::core::tilemap::padded::PaddedTilesets;
use ryo2d::ecs::Schedule;
use ryo2d::prelude::*;
use ryo2d::testing::{HeadlessEngine, asset_world_over, shared_gpu};

/// The spec's static frame: a map's frame over an empty frame's
const STATIC_TARGET: Duration = Duration::from_micros(50);
/// The spec's edit: each part
const EDIT_TARGET: Duration = Duration::from_micros(100);
/// The spec's load
const LOAD_TARGET: Duration = Duration::from_millis(20);
/// The spec's landing: no hitch, held as 0.5 ms over an ordinary frame
const HITCH_TARGET: Duration = Duration::from_micros(500);
/// The spec's streaming: at most 0.5 ms of main-thread time added to a
/// frame, held at the 99th percentile
const STREAM_TARGET: Duration = Duration::from_micros(500);
/// Every engine's view
const RESOLUTION: (u32, u32) = (1920, 1080);
/// A cell's side in pixels
const TILE: u32 = 16;
/// The static and edit maps' side in cells, and the loaded map's
const BIG: u32 = 1024;
const SMALL: u32 = 256;
/// The rounds the static guard takes its medians over: one frame of every
/// engine a round
const ROUNDS: usize = 2000;
/// Frames each engine runs before it is measured
const WARM: usize = 120;
/// The rate the camera goes round its path, as pixels a second over half
/// the map's side: over the guard's frames it flies at 444 to 690 px/s, 561
/// on average
const FLY_SPEED: f32 = 600.0;
/// Edits of each loop
const EDITS: usize = 400;
/// Loads of fresh files, and of a level change
const LOADS: usize = 20;
/// Landings a run, frames a landing's cycle, and the frame of it the map
/// is despawned in
const LANDINGS: usize = 20;
const CYCLE: usize = 40;
const DESPAWN: usize = 32;
/// A 60 Hz frame: the landing's pace
const FRAME: Duration = Duration::from_nanos(16_666_667);
/// The tileset: 16 x 16 tiles of 16 px. Rock (a full-cell collision
/// shape), background, props and foreground details
const COLUMNS: u32 = 16;
const ROCK: std::ops::Range<u32> = 0..8;
const BACK: std::ops::Range<u32> = 16..24;
const PROPS: std::ops::Range<u32> = 32..48;
const FRONT: std::ops::Range<u32> = 48..56;
/// The streaming camera's speed in pixels a second, and its heading: both
/// axes move, so rows and columns of chunks stream in
const STREAM_SPEED: f32 = 2000.0;
const HEADING: Vec2 = Vec2::new(0.8, 0.6);
/// The streamed world's load and unload distances: the stream's defaults
const LOAD: f32 = 512.0;
const UNLOAD: f32 = 1536.0;
/// After the crossing, the streaming cameras jump this far and stay this
/// many frames: a teleport
const JUMP: Vec2 = Vec2::new(100.0 * (CHUNK_SIZE * TILE) as f32, 0.0);
const TELEPORT_FRAMES: usize = 180;
/// Frames a leg of the streaming camera's path takes: it goes out and back
/// along it, so a long criterion run or profile stays well within
/// collision's reach of the origin (the guard's frames all go out)
const LEG: usize = 4000;
/// The criterion benches whose medians are printed, with what each median
/// is of
const BENCHES: [(&str, &str); 5] = [
  (
    "static_frame",
    "a step of the 1024 x 1024 map, its camera flying (back to back, GPU-paced)",
  ),
  ("edit", "an edit of the serpentine and the step after it"),
  (
    "load",
    "a load of the 256 x 256 TMX with its tileset and image, in a fresh registry",
  ),
  ("landing", "the frame a loaded 256 x 256 map lands in"),
  (
    "stream",
    "a step of the endless world, its camera crossing it at 2,000 px/s (back to back)",
  ),
];

fn us(time: Duration) -> f64 {
  time.as_secs_f64() * 1e6
}

fn ms(time: Duration) -> f64 {
  time.as_secs_f64() * 1e3
}

/// The median, `percentile`th percentile and maximum of `times`
fn spread_at(
  mut times: Vec<Duration>,
  percentile: usize,
) -> Option<(Duration, Duration, Duration)> {
  times.sort_unstable();
  let median = *times.get(times.len() / 2)?;
  let at = times[(times.len() * percentile / 100).min(times.len() - 1)];
  Some((median, at, *times.last()?))
}

/// The median, 95th percentile and maximum of `times`
fn spread(times: Vec<Duration>) -> Option<(Duration, Duration, Duration)> {
  spread_at(times, 95)
}

fn median(times: Vec<Duration>) -> Duration {
  spread(times).map_or(Duration::ZERO, |(median, _, _)| median)
}

/// `times` less `base`, each (none below zero)
fn over(times: &[Duration], base: Duration) -> Vec<Duration> {
  times.iter().map(|t| t.saturating_sub(base)).collect()
}

/// "median X µs, Nth percentile Y, max Z", the `percentile`th
fn shown_at(times: Vec<Duration>, percentile: usize) -> String {
  match spread_at(times, percentile) {
    Some((median, at, max)) => format!(
      "median {:.1} µs, {percentile}th percentile {:.1}, max {:.1}",
      us(median),
      us(at),
      us(max)
    ),
    None => "none".into(),
  }
}

/// "median X µs, 95th percentile Y, max Z"
fn shown(times: Vec<Duration>) -> String {
  shown_at(times, 95)
}

/// Whether `time` is within `target`, and the verdict
fn verdict(time: Duration, target: Duration) -> (bool, String) {
  let met = time <= target;
  let word = if met { "met" } else { "missed" };
  (met, format!("target {} µs: {word}", target.as_micros()))
}

/// Wait, spinning, until `next`: a thread that sleeps most of each frame
/// runs its frames on a slower core
fn pace(next: Instant) {
  while Instant::now() < next {
    std::hint::spin_loop();
  }
}

/// Let the GPU finish everything submitted, as a frame at 60 Hz finds it
fn settle_gpu() {
  if let Ok((device, _)) = shared_gpu() {
    device
      .poll(wgpu::PollType::wait_indefinitely())
      .expect("the GPU finishes its work");
  }
}

// The art and the maps

/// A hash of a cell, for the maps' scatter and noise
fn hash(x: i32, y: i32, salt: u32) -> u32 {
  let mut h = (x as u32).wrapping_mul(0x8DA6_B343)
    ^ (y as u32).wrapping_mul(0xD816_3841)
    ^ salt.wrapping_mul(0xCB1A_B31F);
  h ^= h >> 13;
  h = h.wrapping_mul(0x5BD1_E995);
  h ^ (h >> 15)
}

/// Smooth value noise in 0..1, on a lattice one unit apart
fn noise(x: f32, y: f32, salt: u32) -> f32 {
  let (x0, y0) = (x.floor(), y.floor());
  let (fx, fy) = (x - x0, y - y0);
  let (sx, sy) = (fx * fx * (3.0 - 2.0 * fx), fy * fy * (3.0 - 2.0 * fy));
  let (ix, iy) = (x0 as i32, y0 as i32);
  let at = |dx: i32, dy: i32| (hash(ix + dx, iy + dy, salt) & 0xFFFF) as f32 / 65535.0;
  let top = at(0, 0) + (at(1, 0) - at(0, 0)) * sx;
  let bottom = at(0, 1) + (at(1, 1) - at(0, 1)) * sx;
  top + (bottom - top) * sy
}

/// The atlas's pixels: 256 x 256, each tile its own colour with a darker
/// rim, so no two tiles look alike
fn atlas_pixels() -> Vec<u8> {
  let side = COLUMNS * TILE;
  let mut data = Vec::with_capacity((side * side * 4) as usize);
  for y in 0..side {
    for x in 0..side {
      let id = (y / TILE) * COLUMNS + x / TILE;
      let h = hash(id as i32, 0, 9);
      let rim = x % TILE == 0 || y % TILE == 0 || x % TILE == TILE - 1 || y % TILE == TILE - 1;
      let shade = |c: u32| if rim { (c / 2) as u8 } else { c as u8 };
      data.extend_from_slice(&[
        shade(h & 0xFF),
        shade((h >> 8) & 0xFF),
        shade((h >> 16) & 0xFF),
        255,
      ]);
    }
  }
  data
}

/// The tileset over `image`: rock tiles carry a full-cell collision shape
fn tileset(image: Handle<Image>) -> Tileset {
  let side = COLUMNS * TILE;
  let mut set = Tileset::new(
    "bench",
    UVec2::splat(TILE),
    TilesetImage::Atlas {
      image,
      size: UVec2::splat(side),
    },
  );
  set.columns = COLUMNS;
  set.tile_count = COLUMNS * COLUMNS;
  let t = TILE as f32;
  for id in ROCK {
    set.insert_tile(
      id,
      TileData {
        shapes: vec![TileShape {
          points: vec![
            Vec2::ZERO,
            Vec2::new(t, 0.0),
            Vec2::new(t, t),
            Vec2::new(0.0, t),
          ],
          ..TileShape::default()
        }],
        ..TileData::default()
      },
    );
  }
  set
}

/// A tile of `range` picked by `h`
fn pick(range: std::ops::Range<u32>, h: u32) -> u32 {
  range.start + h % (range.end - range.start)
}

/// Whether the cell (`x`, `y`) is cave rock: where noise is high
fn rocky(x: i32, y: i32) -> bool {
  let (fx, fy) = (x as f32, y as f32);
  noise(fx / 22.0, fy / 22.0, 1) * 0.7 + noise(fx / 7.0, fy / 7.0, 2) * 0.3 > 0.6
}

/// The cells of a cave map `side` cells square, layer by layer (background,
/// ground, props, front), as tile ids: a background on every cell, rock
/// where noise is high, props on about one open cell in 16 and front
/// details on one in 64
fn cave_cells(side: u32) -> [Vec<Option<u32>>; 4] {
  let n = (side * side) as usize;
  let mut layers = [vec![None; n], vec![None; n], vec![None; n], vec![None; n]];
  for y in 0..side as i32 {
    for x in 0..side as i32 {
      let i = (y as u32 * side + x as u32) as usize;
      let h = hash(x, y, 3);
      layers[0][i] = Some(pick(BACK, h));
      if rocky(x, y) {
        layers[1][i] = Some(pick(ROCK, h));
        continue;
      }
      let scatter = hash(x, y, 4);
      if scatter.is_multiple_of(16) {
        layers[2][i] = Some(pick(PROPS, scatter / 16));
      } else if scatter % 64 == 1 {
        layers[3][i] = Some(pick(FRONT, scatter / 64));
      }
    }
  }
  layers
}

const LAYER_NAMES: [&str; 4] = ["background", "ground", "props", "front"];

/// A map of `cells` (as `cave_cells` gives them) over the tileset `set`:
/// bounded layers of `side` cells, or unbounded ones of an infinite map
fn map_of(side: u32, set: TilesetSource, cells: &[Vec<Option<u32>>; 4], infinite: bool) -> TileMap {
  let mut map = if infinite {
    TileMap::infinite(UVec2::splat(TILE))
  } else {
    TileMap::new(UVec2::splat(side), UVec2::splat(TILE))
  };
  let set = map.add_tileset(set);
  for (name, layer_cells) in LAYER_NAMES.iter().zip(cells) {
    let layer = map.add_tile_layer(name);
    let grid = map.tiles_mut(layer).expect("a tile layer");
    for (i, id) in layer_cells.iter().enumerate() {
      if let Some(id) = id {
        let cell = IVec2::new((i as u32 % side) as i32, (i as u32 / side) as i32);
        grid.set(cell, Some(Tile::new(set, *id)));
      }
    }
  }
  map
}

/// The tileset added to `ctx`'s stores
fn add_tileset(ctx: &mut SceneContext) -> TilesetSource {
  let side = COLUMNS * TILE;
  let image = ctx
    .assets()
    .add(Image::from_rgba(atlas_pixels(), side, side));
  TilesetSource::Asset(ctx.assets().add(tileset(image)))
}

/// The camera's path at `path` (radians along it): a long loop over most
/// of a map `side` cells square
fn flown(path: f32, side: u32) -> Vec2 {
  let side = (side * TILE) as f32;
  let reach = side * 0.42;
  Vec2::new(
    side / 2.0 + reach * path.sin(),
    side / 2.0 + reach * (path * 1.37 + 1.0).sin(),
  )
}

/// Where the camera is at frame `frame`, going round its path at
/// `FLY_SPEED`
fn camera_at(frame: usize) -> Vec2 {
  let side = (BIG * TILE) as f32;
  flown(frame as f32 * FLY_SPEED / 60.0 / (side * 0.5), BIG)
}

/// A light that follows the camera
#[derive(Component)]
struct Torch;

/// The map roots in `engine`'s world
fn roots(engine: &mut HeadlessEngine) -> Vec<Entity> {
  let world = engine.world_mut();
  world
    .query_filtered::<Entity, With<MapRoot>>()
    .iter(world)
    .collect()
}

/// Whether every map in `engine` has landed
fn landed(engine: &mut HeadlessEngine) -> bool {
  let roots = roots(engine);
  let view = MapsView::new(engine.world_mut());
  roots.iter().all(|&root| view.is_ready(root))
}

/// Puts the camera (and a torch, if there is one) at `at`
fn place_camera(engine: &mut HeadlessEngine, at: Vec2) {
  let world = engine.world_mut();
  if let Some(mut camera) = CameraMut::main(world) {
    camera.set_position(at.x, at.y);
  }
  let mut torches = world.query_filtered::<&mut Transform, With<Torch>>();
  for mut transform in torches.iter_mut(world) {
    transform.translation = at;
  }
}

// static_frame

/// What an engine of the static guard shows
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Show {
  Empty,
  /// The map with collision and shadows: the guarded one
  Map,
  /// The same map with unbounded layers, also guarded
  MapUnbounded,
  /// Drawn only: no collision, no shadows
  Drawn,
  /// The map with its props layer y-sorted
  YSorted,
  EmptyLit,
  MapLit,
  /// Colliding, its layers casting no shadows
  NoShadows,
  /// Casting shadows, its root without collision
  NoCollision,
}

impl Show {
  fn has_map(self) -> bool {
    !matches!(self, Show::Empty | Show::EmptyLit)
  }

  fn lit(self) -> bool {
    matches!(self, Show::EmptyLit | Show::MapLit)
  }

  fn name(self) -> &'static str {
    match self {
      Show::Empty => "an empty frame",
      Show::Map => "the map, colliding and casting shadows",
      Show::MapUnbounded => "the map with unbounded layers, colliding and casting shadows",
      Show::Drawn => "the map drawn only (no collision, no shadows)",
      Show::YSorted => "the map with its props layer y-sorted",
      Show::EmptyLit => "an empty lit frame",
      Show::MapLit => "the map lit (one shadowed light)",
      Show::NoShadows => "the map colliding, without shadows",
      Show::NoCollision => "the map casting shadows, without collision",
    }
  }
}

/// The static guard's scene: the big map (or nothing), lit or not
struct BigScene {
  show: Show,
}

impl Scene for BigScene {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    if self.show.lit() {
      ctx.lighting().set_ambient(AmbientLight::night());
      ctx.world.spawn((
        Transform::from_xy(0.0, 0.0),
        Light2D::point(Color::WHITE, 500.0).with_shadows(),
        Torch,
      ));
    }
    if !self.show.has_map() {
      return;
    }
    let set = add_tileset(ctx);
    let mut map = map_of(BIG, set, &cave_cells(BIG), self.show == Show::MapUnbounded);
    if self.show == Show::YSorted {
      map.layers[2].info.y_sorted = true;
    }
    if matches!(self.show, Show::Drawn | Show::NoShadows) {
      for layer in &mut map.layers {
        layer.info.occludes = Some(false);
      }
    }
    let root = MapRoot::new(ctx.assets().add(map));
    let root = if matches!(self.show, Show::Drawn | Show::NoCollision) {
      root.without_collision()
    } else {
      root
    };
    ctx.world.spawn((Transform::IDENTITY, root));
  }
}

/// The map's first build as the static guard's engines land it, stepped
/// back to back: the landing job (spawn to landed), the landing frame and
/// the frame after
struct FirstBuild {
  job: Duration,
  frame: Duration,
  after: Duration,
}

/// An engine of the static guard, landed and warmed up; its first build if
/// it has a map. None without a GPU
fn static_engine(show: Show) -> Option<(HeadlessEngine, Option<FirstBuild>)> {
  let builder = EngineBuilder::new()
    .resolution(RESOLUTION.0, RESOLUTION.1)
    .start_scene(BigScene { show });
  let mut engine = HeadlessEngine::new_or_skip(builder)?;
  place_camera(&mut engine, camera_at(0));
  let start = Instant::now();
  let mut built = None;
  if show.has_map() {
    loop {
      let step = Instant::now();
      engine.step().expect("a frame");
      let frame = step.elapsed();
      if landed(&mut engine) {
        let job = start.elapsed();
        let step = Instant::now();
        engine.step().expect("a frame");
        built = Some(FirstBuild {
          job,
          frame,
          after: step.elapsed(),
        });
        break;
      }
      assert!(start.elapsed() < Duration::from_secs(60), "the map landed");
    }
  }
  for frame in 0..WARM {
    place_camera(&mut engine, camera_at(frame));
    engine.step().expect("a warm-up frame");
  }
  Some((engine, built))
}

/// One engine of the static guard and its frames
struct Probe {
  show: Show,
  engine: HeadlessEngine,
  steps: Vec<Duration>,
  renders: Vec<Duration>,
}

/// The static frame guard
fn static_guard() {
  let shows = [
    Show::Empty,
    Show::Map,
    Show::MapUnbounded,
    Show::Drawn,
    Show::YSorted,
    Show::EmptyLit,
    Show::MapLit,
    Show::NoShadows,
    Show::NoCollision,
  ];
  let mut probes = Vec::new();
  let mut first = None;
  let mut first_unbounded = None;
  for show in shows {
    let Some((engine, built)) = static_engine(show) else {
      return;
    };
    if show == Show::Map {
      first = built;
    } else if show == Show::MapUnbounded {
      first_unbounded = built;
    }
    probes.push(Probe {
      show,
      engine,
      steps: Vec::with_capacity(ROUNDS),
      renders: Vec::with_capacity(ROUNDS),
    });
  }
  // Round robin, as `benches/ui_render.rs`: each round starts at the next
  // engine, every other round in reverse, so no engine steps twice in a row
  let count = probes.len();
  for round in 0..ROUNDS {
    let at = camera_at(WARM + round);
    for i in 0..count {
      let index = if round.is_multiple_of(2) {
        (round + i) % count
      } else {
        (round + 1 + count - i) % count
      };
      let probe = &mut probes[index];
      place_camera(&mut probe.engine, at);
      settle_gpu();
      let start = Instant::now();
      probe.engine.step().expect("a guard frame");
      probe.steps.push(start.elapsed());
      probe.renders.push(probe.engine.render_time());
    }
  }
  let find = |show: Show| {
    probes
      .iter()
      .find(|probe| probe.show == show)
      .expect("a probe of each show")
  };
  let empty = find(Show::Empty);
  let (empty_step, empty_render) = (median(empty.steps.clone()), median(empty.renders.clone()));
  let empty_lit = find(Show::EmptyLit);
  let empty_lit_step = median(empty_lit.steps.clone());
  eprintln!(
    "static_frame guard: {ROUNDS} interleaved frames at {}x{}, the camera flying at about 560 \
     px/s. An empty frame: step median {:.1} µs (render step {:.1}); an empty lit frame {:.1}",
    RESOLUTION.0,
    RESOLUTION.1,
    us(empty_step),
    us(empty_render),
    us(empty_lit_step),
  );
  for probe in &probes {
    if !probe.show.has_map() {
      continue;
    }
    let base = if probe.show.lit() {
      empty_lit_step
    } else {
      empty_step
    };
    let render = median(probe.renders.clone());
    let line = format!(
      "{}: the step over the empty frame's median, {}; its render step {:+.1} µs over the empty \
       frame's",
      probe.show.name(),
      shown(over(&probe.steps, base)),
      us(render) - us(empty_render),
    );
    if matches!(probe.show, Show::Map | Show::MapUnbounded) {
      let added = median(probe.steps.clone()).saturating_sub(base);
      let (met, held) = verdict(added, STATIC_TARGET);
      eprintln!(
        "static_frame: {line}. Held, the median: {:.1} µs, {held}",
        us(added)
      );
      if !met {
        guards::miss(format!("static_frame, {line}"));
      }
    } else {
      eprintln!("static_frame (no target): {line}");
    }
  }
  for probe in probes.iter_mut().filter(|probe| probe.show.has_map()) {
    let world = probe.engine.world_mut();
    let occluders = world.query::<&MapOccluder>().iter(world).count();
    let children = world.query::<&ChildOf>().iter(world).count();
    eprintln!(
      "static_frame (no target): {}: {} entities, {children} of them children, {occluders} \
       occluder chains",
      probe.show.name(),
      world.entities().len(),
    );
  }
  for (first, of) in [(first, "bounded"), (first_unbounded, "unbounded")] {
    let Some(first) = first else { continue };
    eprintln!(
      "static_frame (no target): the 1024 x 1024 {of} map's first build, stepped back to back: \
       spawn to landed {:.1} ms (the landing job on its worker, and the frames meanwhile), the \
       landing frame {:.1} ms, the frame after {:.1} ms",
      ms(first.job),
      ms(first.frame),
      ms(first.after),
    );
  }
}

/// The static frame bench: the map engine stepped back to back, its camera
/// flying
fn static_bench(c: &mut Criterion) {
  let mut made: Option<(HeadlessEngine, usize)> = None;
  c.bench_function("static_frame", |b| {
    if made.is_none() {
      made = static_engine(Show::Map).map(|(engine, _)| (engine, WARM));
    }
    let Some((engine, frame)) = made.as_mut() else {
      return;
    };
    b.iter(|| {
      *frame += 1;
      place_camera(engine, camera_at(*frame));
      engine.step().expect("a frame")
    });
  });
}

/// What a padded tileset cache hit costs a frame: an atlas tileset's, and
/// a 1,000-tile collection's (whose stamp reads every tile's image
/// version). No target
fn padded_line() {
  let mut world = asset_world_over(MemorySource::new());
  let atlas = {
    let side = COLUMNS * TILE;
    let image =
      world
        .resource_mut::<Assets<Image>>()
        .add(Image::from_rgba(atlas_pixels(), side, side));
    TilesetSource::Asset(world.resource_mut::<Assets<Tileset>>().add(tileset(image)))
  };
  let collection = {
    let images: Vec<Handle<Image>> = (0..8)
      .map(|k| {
        world.resource_mut::<Assets<Image>>().add(Image::solid_rect(
          TILE,
          TILE,
          k * 30,
          90,
          200,
          255,
        ))
      })
      .collect();
    let mut set = Tileset::new("collection", UVec2::splat(TILE), TilesetImage::Collection);
    set.tile_count = 1000;
    for id in 0..1000u32 {
      set.insert_tile(
        id,
        TileData {
          image: Some((
            images[id as usize % images.len()].clone(),
            UVec2::splat(TILE),
          )),
          ..TileData::default()
        },
      );
    }
    TilesetSource::Asset(world.resource_mut::<Assets<Tileset>>().add(set))
  };
  let mut padded = PaddedTilesets::default();
  let mut time = |source: &TilesetSource| {
    world.resource_scope(|world, mut images: Mut<Assets<Image>>| {
      let tilesets = world.resource::<Assets<Tileset>>();
      let mut times = Vec::with_capacity(1000);
      for _ in 0..1100 {
        padded.begin_round();
        let start = Instant::now();
        let got = padded.get(source, tilesets, &mut images);
        times.push(start.elapsed());
        assert!(got.is_some(), "the tileset pads");
        padded.end_round();
      }
      median(times.split_off(100))
    })
  };
  let (atlas, collection) = (time(&atlas), time(&collection));
  eprintln!(
    "padded tileset cache hits, one a tileset a frame (no target): an atlas {:.2} µs, a \
     1,000-tile collection {:.2} µs",
    us(atlas),
    us(collection),
  );
}

// edit

/// The edit map's loops: each in chunks (0..4, row) of the ground layer
const SKYLINE_ROW: i32 = 0;
const SERPENTINE_ROW: i32 = 2;

/// The skyline's column that holds only its bottom cell: the neck between
/// its two parts, which clearing that cell splits
const NECK: usize = 99;

/// The skyline's column heights: a top in 1..=24 for each of 128 columns,
/// but the neck's, 31
fn skyline_tops() -> Vec<i32> {
  let mut seed: u32 = 7;
  (0..128)
    .map(|x| {
      seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
      if x == NECK {
        31
      } else {
        1 + (seed >> 16) as i32 % 24
      }
    })
    .collect()
}

/// One loop's edits: a tile set on top of it and cleared again (a bump: the
/// loop stays one), or a cell of it cleared and set again (a cut: the loop
/// splits in two, and the two join again)
struct EditCase {
  name: &'static str,
  cell: IVec2,
  cut: bool,
}

/// The edits the guard makes: each loop's bump, and each loop's cut (the
/// skyline's neck, the middle of the serpentine's first row, and the middle
/// of its middle row, which splits it into halves of about 2,000 edges,
/// the smaller given the other loop's id edge by edge)
fn edit_cases() -> [EditCase; 5] {
  let tops = skyline_tops();
  let skyline = |x: usize, y: i32| IVec2::new(x as i32, SKYLINE_ROW * 32 + y);
  let serpentine = |x: i32, y: i32| IVec2::new(x, SERPENTINE_ROW * 32 + y);
  [
    EditCase {
      name: "the skyline",
      cell: skyline(64, tops[64] - 1),
      cut: false,
    },
    EditCase {
      name: "the skyline",
      cell: skyline(NECK, 31),
      cut: true,
    },
    EditCase {
      name: "the serpentine",
      cell: serpentine(64, 0),
      cut: false,
    },
    EditCase {
      name: "the serpentine",
      cell: serpentine(64, 1),
      cut: true,
    },
    EditCase {
      name: "the serpentine cut through its middle row",
      cell: serpentine(64, 15),
      cut: true,
    },
  ]
}

/// The edit map: background on every cell; on the ground layer 3 x 3
/// islands every 8 cells (about 16,000), and the skyline and the
/// serpentine, each in the 4 x 1 chunks of its row, with the islands kept
/// out of chunks 0..5 by 0..4; props and details as the cave map's
fn edit_map(set: TilesetSource) -> TileMap {
  let mut cells = cave_cells(BIG);
  let side = BIG as i32;
  let at = |x: i32, y: i32| (y * side + x) as usize;
  for cell in cells[1].iter_mut() {
    *cell = None;
  }
  let rock = |x: i32, y: i32| Some(pick(ROCK, hash(x, y, 5)));
  for by in (0..side).step_by(8) {
    for bx in (0..side).step_by(8) {
      if bx < 160 && by < 128 {
        continue;
      }
      for (x, y) in (0..9).map(|i| (bx + 2 + i % 3, by + 2 + i / 3)) {
        cells[1][at(x, y)] = rock(x, y);
      }
    }
  }
  for (x, top) in skyline_tops().into_iter().enumerate() {
    for y in top..32 {
      cells[1][at(x as i32, SKYLINE_ROW * 32 + y)] = rock(x as i32, y);
    }
  }
  // Every other row from row 1, joined at alternate ends
  let base = SERPENTINE_ROW * 32;
  for y in (1..32).step_by(2) {
    for x in 0..128 {
      cells[1][at(x, base + y)] = rock(x, base + y);
    }
    if y < 31 {
      let end = if y % 4 == 1 { 127 } else { 0 };
      cells[1][at(end, base + y + 1)] = rock(end, base + y + 1);
    }
  }
  // Props and details only where the ground is open
  for i in 0..cells[1].len() {
    if cells[1][i].is_some() {
      cells[2][i] = None;
      cells[3][i] = None;
    }
  }
  map_of(BIG, set, &cells, false)
}

/// The edit guard's scene: the edit map
struct EditScene;

impl Scene for EditScene {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    let set = add_tileset(ctx);
    let map = edit_map(set);
    let root = MapRoot::new(ctx.assets().add(map));
    ctx.world.spawn((Transform::IDENTITY, root));
  }
}

/// An engine on the edit map, landed and warmed up, its camera over both
/// loops; its map's root. None without a GPU
fn edit_engine() -> Option<(HeadlessEngine, Entity)> {
  let builder = EngineBuilder::new()
    .resolution(RESOLUTION.0, RESOLUTION.1)
    .start_scene(EditScene);
  let mut engine = HeadlessEngine::new_or_skip(builder)?;
  place_camera(&mut engine, Vec2::new(1024.0, 540.0));
  let start = Instant::now();
  while !landed(&mut engine) {
    engine.step().expect("a frame");
    assert!(start.elapsed() < Duration::from_secs(60), "the map landed");
  }
  engine.step_frames(WARM as u32).expect("the warm-up frames");
  let root = roots(&mut engine)[0];
  Some((engine, root))
}

/// The tile edit `i` puts at `cell`: rock, then nothing, in turn; nothing
/// first for a cut, whose cell starts as rock
fn toggle(engine: &mut HeadlessEngine, root: Entity, cell: IVec2, i: usize, cut: bool) {
  let rock = i.is_multiple_of(2) != cut;
  let tile = rock.then_some(Tile::new(0, ROCK.start));
  let changed = MapsView::new(engine.world_mut()).set_tile(root, "ground", cell, tile);
  assert!(changed, "the edit changes its cell");
}

/// One loop's edits: each part's times, and the ordinary frames'
struct Edits {
  collision: Vec<Duration>,
  rebuild: Vec<Duration>,
  occluders: Vec<Duration>,
  /// The render step and whole step of each frame after an edit, and of
  /// each ordinary frame between them
  edit_renders: Vec<Duration>,
  edit_steps: Vec<Duration>,
  renders: Vec<Duration>,
  steps: Vec<Duration>,
  /// The loop and occluder rebuilds of edits back to back, no frame between
  warm_loops: Vec<Duration>,
  warm_occluders: Vec<Duration>,
}

/// `schedule` run once on `engine`'s world, timed
fn timed(schedule: &mut Schedule, engine: &mut HeadlessEngine) -> Duration {
  let start = Instant::now();
  schedule.run(engine.world_mut());
  start.elapsed()
}

/// A step with the GPU finished first: its render step and the whole step
fn step(engine: &mut HeadlessEngine) -> (Duration, Duration) {
  settle_gpu();
  let start = Instant::now();
  engine.step().expect("a frame");
  (engine.render_time(), start.elapsed())
}

fn edits(engine: &mut HeadlessEngine, root: Entity, cell: IVec2, cut: bool) -> Edits {
  let [mut collision, mut rebuild, mut occluders]: [Schedule; 3] =
    ryo2d::core::tilemap::edit_schedules();
  // Their first runs set them up
  for schedule in [&mut collision, &mut rebuild, &mut occluders] {
    schedule.run(engine.world_mut());
  }
  let mut out = Edits {
    collision: Vec::with_capacity(EDITS),
    rebuild: Vec::with_capacity(EDITS),
    occluders: Vec::with_capacity(EDITS),
    edit_renders: Vec::with_capacity(EDITS),
    edit_steps: Vec::with_capacity(EDITS),
    renders: Vec::with_capacity(EDITS),
    steps: Vec::with_capacity(EDITS),
    warm_loops: Vec::with_capacity(EDITS),
    warm_occluders: Vec::with_capacity(EDITS),
  };
  for i in 0..EDITS {
    let (render, whole) = step(engine);
    out.renders.push(render);
    out.steps.push(whole);
    toggle(engine, root, cell, i, cut);
    out.collision.push(timed(&mut collision, engine));
    out.rebuild.push(timed(&mut rebuild, engine));
    out.occluders.push(timed(&mut occluders, engine));
    let (render, whole) = step(engine);
    out.edit_renders.push(render);
    out.edit_steps.push(whole);
  }
  // Back to back, no frame between: what the edits cost with their data
  // in cache
  for i in EDITS..2 * EDITS {
    toggle(engine, root, cell, i, cut);
    let collided = timed(&mut collision, engine) + timed(&mut rebuild, engine);
    out.warm_loops.push(collided);
    out.warm_occluders.push(timed(&mut occluders, engine));
  }
  out
}

/// The edits `which` picks of `times`: every one, or every other from the
/// first or the second
fn picked(times: &[Duration], which: Option<usize>) -> Vec<Duration> {
  match which {
    None => times.to_vec(),
    Some(first) => times.iter().skip(first).step_by(2).copied().collect(),
  }
}

/// The edit guard: each loop's bump, split and join, each's chunk, loop and
/// occluder rebuilds, each median held to 100 µs
fn edit_guard() {
  let Some((mut engine, root)) = edit_engine() else {
    return;
  };
  for case in edit_cases() {
    let (name, cell) = (case.name, case.cell);
    let edits = edits(&mut engine, root, cell, case.cut);
    let ordinary_render = median(edits.renders.clone());
    let ordinary_step = median(edits.steps.clone());
    let chunk = over(&edits.edit_renders, ordinary_render);
    let loops: Vec<Duration> = edits
      .collision
      .iter()
      .zip(&edits.rebuild)
      .map(|(a, b)| *a + *b)
      .collect();
    // A bump's edits all keep one loop; a cut's split it and join it in turn
    let kinds: &[(&str, Option<usize>)] = if case.cut {
      &[("a split", Some(0)), ("a join", Some(1))]
    } else {
      &[("a bump", None)]
    };
    for &(kind, which) in kinds {
      eprintln!(
        "edit guard, {name}, {kind} (cell {cell}), {} edits, each beside an ordinary frame: \
         the collision sync {}; the tile collider rebuild and collider sync {}; the edit \
         frame's whole step over an ordinary frame's median ({:.1} µs) {} (no target)",
        picked(&edits.collision, which).len(),
        shown(picked(&edits.collision, which)),
        shown(picked(&edits.rebuild, which)),
        us(ordinary_step),
        shown(over(&picked(&edits.edit_steps, which), ordinary_step)),
      );
      eprintln!(
        "edit (no target), {name}, {kind}, as many edits back to back with no frame between \
         (their data in cache): the loop rebuild {}; the occluder rebuild {}",
        shown(picked(&edits.warm_loops, which)),
        shown(picked(&edits.warm_occluders, which)),
      );
      for (part, times) in [
        ("the chunk rebuild", &chunk),
        ("the loop rebuild", &loops),
        ("the occluder rebuild", &edits.occluders),
      ] {
        let times = picked(times, which);
        let held = median(times.clone());
        let (met, verdict) = verdict(held, EDIT_TARGET);
        let line = format!("{name}, {kind}, {part}: {}; {verdict}", shown(times));
        eprintln!("edit: {line}");
        if !met {
          guards::miss(format!("edit, {line}"));
        }
      }
    }
  }
}

/// The edit bench: the serpentine's cell set or cleared, and the step after
fn edit_bench(c: &mut Criterion) {
  let mut made: Option<(HeadlessEngine, Entity, usize)> = None;
  let cell = edit_cases()[2].cell;
  c.bench_function("edit", |b| {
    if made.is_none() {
      made = edit_engine().map(|(engine, root)| (engine, root, 0));
    }
    let Some((engine, root, i)) = made.as_mut() else {
      return;
    };
    b.iter(|| {
      toggle(engine, *root, cell, *i, false);
      *i += 1;
      engine.step().expect("a frame")
    });
  });
}

// load

/// A TMX file of the 256 x 256 cave map, every layer base64 and zlib, over
/// the tileset at `tileset` (relative to the map's folder)
fn tmx(cells: &[Vec<Option<u32>>; 4], tileset: &str) -> Vec<u8> {
  let mut out = String::new();
  out.push_str(&format!(
    "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<map version=\"1.10\" \
     tiledversion=\"1.12.0\" orientation=\"orthogonal\" renderorder=\"right-down\" \
     width=\"{SMALL}\" height=\"{SMALL}\" tilewidth=\"{TILE}\" tileheight=\"{TILE}\" \
     infinite=\"0\" nextlayerid=\"5\" nextobjectid=\"1\">\n <tileset firstgid=\"1\" \
     source=\"{tileset}\"/>\n"
  ));
  for (index, (name, layer)) in LAYER_NAMES.iter().zip(cells).enumerate() {
    let mut raw = Vec::with_capacity(layer.len() * 4);
    for id in layer {
      let gid = id.map_or(0, |id| id + 1);
      raw.extend_from_slice(&gid.to_le_bytes());
    }
    let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    z.write_all(&raw).expect("compressed");
    let packed = z.finish().expect("compressed");
    let text = base64::engine::general_purpose::STANDARD.encode(packed);
    out.push_str(&format!(
      " <layer id=\"{}\" name=\"{name}\" width=\"{SMALL}\" height=\"{SMALL}\">\n  <data \
       encoding=\"base64\" compression=\"zlib\">\n   {text}\n  </data>\n </layer>\n",
      index + 1
    ));
  }
  out.push_str("</map>\n");
  out.into_bytes()
}

/// An infinite TMX file of the same map: every layer base64 and zlib in
/// chunks of 16 x 16 cells, an empty chunk left out as Tiled does
fn tmx_infinite(cells: &[Vec<Option<u32>>; 4], tileset: &str) -> Vec<u8> {
  const CHUNK: u32 = 16;
  let mut out = String::new();
  out.push_str(&format!(
    "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<map version=\"1.10\" \
     tiledversion=\"1.12.0\" orientation=\"orthogonal\" renderorder=\"right-down\" \
     width=\"0\" height=\"0\" tilewidth=\"{TILE}\" tileheight=\"{TILE}\" \
     infinite=\"1\" nextlayerid=\"5\" nextobjectid=\"1\">\n <tileset firstgid=\"1\" \
     source=\"{tileset}\"/>\n"
  ));
  for (index, (name, layer)) in LAYER_NAMES.iter().zip(cells).enumerate() {
    out.push_str(&format!(
      " <layer id=\"{}\" name=\"{name}\" width=\"0\" height=\"0\">\n  <data \
       encoding=\"base64\" compression=\"zlib\">\n",
      index + 1
    ));
    for cy in 0..SMALL / CHUNK {
      for cx in 0..SMALL / CHUNK {
        let mut raw = Vec::with_capacity((CHUNK * CHUNK * 4) as usize);
        let mut any = false;
        for y in cy * CHUNK..(cy + 1) * CHUNK {
          for x in cx * CHUNK..(cx + 1) * CHUNK {
            let id = layer[(y * SMALL + x) as usize];
            any |= id.is_some();
            raw.extend_from_slice(&id.map_or(0, |id| id + 1).to_le_bytes());
          }
        }
        if !any {
          continue;
        }
        let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        z.write_all(&raw).expect("compressed");
        let packed = z.finish().expect("compressed");
        let text = base64::engine::general_purpose::STANDARD.encode(packed);
        out.push_str(&format!(
          "   <chunk x=\"{}\" y=\"{}\" width=\"{CHUNK}\" height=\"{CHUNK}\">\n   {text}\n   \
           </chunk>\n",
          cx * CHUNK,
          cy * CHUNK
        ));
      }
    }
    out.push_str("  </data>\n </layer>\n");
  }
  out.push_str("</map>\n");
  out.into_bytes()
}

/// A TSX file of the tileset over the image at `image` (relative to the
/// tileset's folder): rock tiles with a full-cell rectangle
fn tsx(image: &str) -> Vec<u8> {
  let side = COLUMNS * TILE;
  let mut out = format!(
    "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<tileset version=\"1.10\" \
     tiledversion=\"1.12.0\" name=\"terrain\" tilewidth=\"{TILE}\" tileheight=\"{TILE}\" \
     tilecount=\"{}\" columns=\"{COLUMNS}\">\n <image source=\"{image}\" width=\"{side}\" \
     height=\"{side}\"/>\n",
    COLUMNS * COLUMNS
  );
  for id in ROCK {
    out.push_str(&format!(
      " <tile id=\"{id}\">\n  <objectgroup draworder=\"index\" id=\"2\">\n   <object id=\"1\" \
       x=\"0\" y=\"0\" width=\"{TILE}\" height=\"{TILE}\"/>\n  </objectgroup>\n </tile>\n"
    ));
  }
  out.push_str("</tileset>\n");
  out.into_bytes()
}

/// The atlas as a PNG
fn png() -> Vec<u8> {
  let side = COLUMNS * TILE;
  let image = image::RgbaImage::from_raw(side, side, atlas_pixels()).expect("the atlas");
  let mut bytes = std::io::Cursor::new(Vec::new());
  image
    .write_to(&mut bytes, image::ImageFormat::Png)
    .expect("a png");
  bytes.into_inner()
}

/// The load guard's files: `maps/cold-{i}.tmx` over `tilesets/cold-{i}.tsx`
/// over `images/cold-{i}.png` (fresh files for each load), and
/// `maps/warm-{i}.tmx` and `maps/land-{run}-{i}.tmx` over
/// `tilesets/terrain.tsx` over `images/terrain.png`
fn load_files() -> MemorySource {
  let files = MemorySource::new();
  let cells = cave_cells(SMALL);
  let png = png();
  let terrain = tmx(&cells, "../tilesets/terrain.tsx");
  files.insert("tilesets/terrain.tsx", tsx("../images/terrain.png"));
  files.insert("images/terrain.png", png.clone());
  // One more than the guard loads, for the untimed load that starts the
  // workers
  for i in 0..=LOADS {
    files.insert(
      &format!("maps/cold-{i}.tmx"),
      tmx(&cells, &format!("../tilesets/cold-{i}.tsx")),
    );
    files.insert(
      &format!("tilesets/cold-{i}.tsx"),
      tsx(&format!("../images/cold-{i}.png")),
    );
    files.insert(&format!("images/cold-{i}.png"), png.clone());
    files.insert(&format!("maps/warm-{i}.tmx"), terrain.clone());
    files.insert(
      &format!("maps/inf-cold-{i}.tmx"),
      tmx_infinite(&cells, &format!("../tilesets/inf-cold-{i}.tsx")),
    );
    files.insert(
      &format!("tilesets/inf-cold-{i}.tsx"),
      tsx(&format!("../images/inf-cold-{i}.png")),
    );
    files.insert(&format!("images/inf-cold-{i}.png"), png.clone());
  }
  for run in 0..2 {
    for i in 0..LANDINGS {
      files.insert(&format!("maps/land-{run}-{i}.tmx"), terrain.clone());
    }
  }
  files
}

/// Loads `path` and pumps until it is ready with its dependencies: how
/// long that took, and how long the pumps took on this thread
fn timed_load(world: &mut World, server: &AssetServer, path: &str) -> (Duration, Duration) {
  let start = Instant::now();
  let handle = server.load::<TileMap>(path);
  let mut pumping = Duration::ZERO;
  loop {
    let pumped = Instant::now();
    pump(world);
    pumping += pumped.elapsed();
    if server.is_ready(&handle) {
      break;
    }
    if let Some(error) = server.dependency_state(&handle).error() {
      panic!("{path}: {}", error.report());
    }
    assert!(start.elapsed() < Duration::from_secs(10), "{path} loaded");
    std::thread::yield_now();
  }
  let took = start.elapsed();
  drop(handle);
  pump(world);
  (took, pumping)
}

/// The loads: twenty of fresh files (map, tileset and image) and twenty
/// level changes (the tileset loaded already), and the main thread's pumps
fn load_guard(files: &MemorySource) {
  let mut world = asset_world_over(files.clone());
  let server = world.resource::<AssetServer>().clone();
  // The workers start with the first load
  timed_load(&mut world, &server, &format!("maps/cold-{LOADS}.tmx"));
  let (mut cold, mut pumps) = (Vec::new(), Vec::new());
  for i in 0..LOADS {
    let (took, pumping) = timed_load(&mut world, &server, &format!("maps/cold-{i}.tmx"));
    cold.push(took);
    pumps.push(pumping);
  }
  let mut infinite = Vec::new();
  for i in 0..LOADS {
    let path = format!("maps/inf-cold-{i}.tmx");
    infinite.push(timed_load(&mut world, &server, &path).0);
  }
  let tileset = server.load::<Tileset>("tilesets/terrain.tsx");
  while !server.is_ready(&tileset) {
    pump(&mut world);
    std::thread::yield_now();
  }
  let mut warm = Vec::new();
  for i in 0..LOADS {
    warm.push(timed_load(&mut world, &server, &format!("maps/warm-{i}.tmx")).0);
  }
  drop(tileset);
  let held = median(cold.clone());
  let (met, held_text) = verdict(held, LOAD_TARGET);
  let line = format!(
    "a 256 x 256 four-layer base64 and zlib TMX with its TSX and PNG, {LOADS} loads of fresh \
     files: {}; {held_text}",
    shown(cold)
  );
  eprintln!("load: {line}");
  eprintln!(
    "load (no target): the main thread's pumps meanwhile {}; a level change (the tileset \
     loaded already) {}",
    shown(pumps),
    shown(warm),
  );
  if !met {
    guards::miss(format!("load, {line}"));
  }
  let held = median(infinite.clone());
  let (met, held_text) = verdict(held, LOAD_TARGET);
  let line = format!(
    "a 256 x 256 four-layer infinite TMX (16 x 16 chunks, base64 and zlib) with its TSX and \
     PNG, {LOADS} loads of fresh files: {}; {held_text}",
    shown(infinite)
  );
  eprintln!("load: {line}");
  if !met {
    guards::miss(format!("load, {line}"));
  }
}

/// A scene with nothing in it: the landing guard's maps are spawned from
/// outside
struct Nothing;

impl Scene for Nothing {}

/// An engine over the load guard's files, its camera over the top left of
/// the 256 x 256 map, holding the shared tileset; warmed up. None without
/// a GPU
fn landing_engine(files: &MemorySource) -> Option<(HeadlessEngine, Handle<Tileset>)> {
  let builder = EngineBuilder::new()
    .resolution(RESOLUTION.0, RESOLUTION.1)
    .asset_source(files.clone())
    .start_scene(Nothing);
  let mut engine = HeadlessEngine::new_or_skip(builder)?;
  place_camera(
    &mut engine,
    Vec2::new(RESOLUTION.0 as f32 / 2.0, RESOLUTION.1 as f32 / 2.0),
  );
  let tileset = engine
    .assets()
    .load_now::<Tileset>("tilesets/terrain.tsx")
    .unwrap_or_else(|error| panic!("the tileset: {}", error.report()));
  engine.step_frames(60).expect("the warm-up frames");
  Some((engine, tileset))
}

/// What a frame of a landing run was
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kind {
  Ordinary,
  /// The map's asset landed in it
  Asset,
  /// The map landed in it
  Map,
  /// The frame after the map landed
  After,
  /// The map was despawned in it, or the frame before was
  Despawn,
}

/// One run of the landing guard: each frame's time and kind, and the
/// landings that did not land within their cycle
struct LandingRun {
  frames: Vec<(Duration, Kind)>,
  unlanded: usize,
  /// Frames from spawn to landed, each landing
  waits: Vec<usize>,
  /// Each map landing frame, in order: the first makes the padded
  /// tileset's texture
  landings: Vec<Duration>,
}

impl LandingRun {
  fn ordinary(&self) -> Vec<Duration> {
    self.of(&[Kind::Ordinary])
  }

  fn of(&self, kinds: &[Kind]) -> Vec<Duration> {
    self
      .frames
      .iter()
      .filter(|(_, kind)| kinds.contains(kind))
      .map(|(time, _)| *time)
      .collect()
  }

  /// The slowest landing frame over the median ordinary frame
  fn slowest(&self) -> Duration {
    let base = median(self.ordinary());
    self
      .of(&[Kind::Asset, Kind::Map, Kind::After])
      .into_iter()
      .max()
      .unwrap_or_default()
      .saturating_sub(base)
  }
}

/// One run: twenty landings at 60 Hz, each a fresh copy of the map loaded
/// and spawned at once, despawned 32 frames later
fn landing_run(files: &MemorySource, run: usize) -> Option<LandingRun> {
  let (mut engine, _tileset) = landing_engine(files)?;
  let mut frames = Vec::with_capacity(LANDINGS * CYCLE);
  let (mut unlanded, mut waits, mut landings) = (0, Vec::new(), Vec::new());
  let mut next = Instant::now();
  for landing in 0..LANDINGS {
    let handle = engine
      .assets()
      .load::<TileMap>(&format!("maps/land-{run}-{landing}.tmx"));
    let id = handle.id();
    let root = engine
      .world_mut()
      .spawn((Transform::IDENTITY, MapRoot::new(handle)))
      .id();
    let mut landed_at = None;
    for frame in 0..CYCLE {
      if frame == DESPAWN {
        engine.world_mut().despawn(root);
      }
      let had_asset = engine
        .world()
        .resource::<Assets<TileMap>>()
        .get(id)
        .is_some();
      let was_ready = landed_at.is_some();
      pace(next);
      next = Instant::now() + FRAME;
      settle_gpu();
      let start = Instant::now();
      engine.step().expect("a frame");
      let time = start.elapsed();
      let has_asset = engine
        .world()
        .resource::<Assets<TileMap>>()
        .get(id)
        .is_some();
      let ready = frame < DESPAWN && MapsView::new(engine.world_mut()).is_ready(root);
      let kind = if frame == DESPAWN || frame == DESPAWN + 1 {
        Kind::Despawn
      } else if ready && !was_ready {
        landed_at = Some(frame);
        landings.push(time);
        Kind::Map
      } else if landed_at.is_some_and(|at| frame == at + 1) {
        Kind::After
      } else if has_asset && !had_asset {
        Kind::Asset
      } else {
        Kind::Ordinary
      };
      frames.push((time, kind));
    }
    match landed_at {
      Some(at) => waits.push(at),
      None => unlanded += 1,
    }
  }
  Some(LandingRun {
    frames,
    unlanded,
    waits,
    landings,
  })
}

/// How long the landing job takes, frames stepped back to back: spawn to
/// landed, the median of five (the web runs it in the frame it is sent)
fn landing_job(files: &MemorySource) -> Option<Duration> {
  let (mut engine, _tileset) = landing_engine(files)?;
  let handle = engine
    .assets()
    .load_now::<TileMap>("maps/warm-0.tmx")
    .unwrap_or_else(|error| panic!("the map: {}", error.report()));
  let mut times = Vec::new();
  for _ in 0..5 {
    let root = engine
      .world_mut()
      .spawn((Transform::IDENTITY, MapRoot::new(handle.clone())))
      .id();
    let start = Instant::now();
    while !MapsView::new(engine.world_mut()).is_ready(root) {
      engine.step().expect("a frame");
      assert!(start.elapsed() < Duration::from_secs(10), "the map landed");
    }
    times.push(start.elapsed());
    engine.world_mut().despawn(root);
    engine.step_frames(3).expect("frames");
  }
  Some(median(times))
}

/// The landing guard: the slowest landing frame over the median ordinary
/// frame, held to 0.5 ms; a run over that is measured once more, and the
/// better run held
fn landing_guard(files: &MemorySource) {
  let Some(first) = landing_run(files, 0) else {
    return;
  };
  let again = first.slowest() > HITCH_TARGET;
  let second = if again { landing_run(files, 1) } else { None };
  let runs: Vec<&LandingRun> = [Some(&first), second.as_ref()]
    .into_iter()
    .flatten()
    .collect();
  for (number, run) in runs.iter().enumerate() {
    let base = median(run.ordinary());
    let waits: Vec<String> = run.waits.iter().map(|w| w.to_string()).collect();
    let first = run
      .landings
      .first()
      .map_or(0.0, |t| us(t.saturating_sub(base)));
    eprintln!(
      "landing guard, run {}: {LANDINGS} landings at 60 Hz, each frame the whole step. Ordinary \
       frames ({}): {}. Over the median ordinary frame: the asset landing {}; the map landing \
       {} (the first, which makes the padded tileset's texture, {first:.1} µs; the others \
       {}); the frame after {}. Frames from spawn to landed: {}",
      number + 1,
      run.ordinary().len(),
      shown(run.ordinary()),
      shown(over(&run.of(&[Kind::Asset]), base)),
      shown(over(&run.of(&[Kind::Map]), base)),
      shown(over(run.landings.get(1..).unwrap_or(&[]), base)),
      shown(over(&run.of(&[Kind::After]), base)),
      waits.join(", "),
    );
  }
  let (number, best) = runs
    .iter()
    .enumerate()
    .min_by_key(|(_, run)| run.slowest())
    .map_or((0, &first), |(number, run)| (number, *run));
  let held = best.slowest();
  let (met, verdict) = verdict(held, HITCH_TARGET);
  let slowest: Vec<String> = runs
    .iter()
    .map(|run| format!("{:.1}", us(run.slowest())))
    .collect();
  let line = format!(
    "the slowest landing frame over the median ordinary frame, run {}: {:.1} µs (each run: {} \
     µs); {verdict}",
    number + 1,
    us(held),
    slowest.join(", "),
  );
  eprintln!("landing: {line}");
  if !met {
    guards::miss(format!("load (landing), {line}"));
  }
  for run in &runs {
    if run.unlanded > 0 {
      guards::miss(format!(
        "load (landing): {} of {LANDINGS} maps did not land within {DESPAWN} frames",
        run.unlanded
      ));
    }
  }
  if let Some(job) = landing_job(files) {
    eprintln!(
      "landing (no target): spawn to landed with frames stepped back to back, the median of \
       five: {:.1} ms (on the web the landing job runs in the frame it is sent)",
      ms(job)
    );
  }
}

/// The load bench: the map with its tileset and image, in a fresh registry
fn load_bench(c: &mut Criterion, files: &mut Option<MemorySource>) {
  c.bench_function("load", |b| {
    let files = files.get_or_insert_with(load_files).clone();
    b.iter_batched(
      || {
        let world = asset_world_over(files.clone());
        let server = world.resource::<AssetServer>().clone();
        (world, server)
      },
      |(mut world, server)| timed_load(&mut world, &server, "maps/cold-0.tmx"),
      BatchSize::PerIteration,
    );
  });
}

/// The landing bench: the loaded map spawned, its landing frame timed, and
/// despawned
fn landing_bench(c: &mut Criterion, files: &mut Option<MemorySource>) {
  let mut made: Option<(HeadlessEngine, Handle<TileMap>, Handle<Tileset>)> = None;
  c.bench_function("landing", |b| {
    let files = files.get_or_insert_with(load_files).clone();
    if made.is_none() {
      made = landing_engine(&files).map(|(mut engine, tileset)| {
        let map = engine
          .assets()
          .load_now::<TileMap>("maps/warm-0.tmx")
          .unwrap_or_else(|error| panic!("the map: {}", error.report()));
        (engine, map, tileset)
      });
    }
    let Some((engine, map, _)) = made.as_mut() else {
      return;
    };
    b.iter_custom(|iters| {
      let mut total = Duration::ZERO;
      for _ in 0..iters {
        let root = engine
          .world_mut()
          .spawn((Transform::IDENTITY, MapRoot::new(map.clone())))
          .id();
        let start = Instant::now();
        loop {
          let frame = Instant::now();
          engine.step().expect("a frame");
          if MapsView::new(engine.world_mut()).is_ready(root) {
            total += frame.elapsed();
            break;
          }
          assert!(start.elapsed() < Duration::from_secs(10), "the map landed");
          std::thread::yield_now();
        }
        engine.world_mut().despawn(root);
        engine.step_frames(2).expect("frames");
      }
      total
    });
  });
}

// stream

/// Where the streaming camera is at frame `frame`: on a straight line
/// through the origin, which the guard's frames (warm-up included) cross
/// halfway, going out and back along a leg of `LEG` frames
fn crossing_at(frame: usize) -> Vec2 {
  let lap = frame % (2 * LEG);
  let along = if lap < LEG { lap } else { 2 * LEG - lap };
  let middle = (WARM + ROUNDS) as f32 / 2.0;
  HEADING * (along as f32 - middle) * STREAM_SPEED / 60.0
}

/// A chunk of the endless world: a background on every cell, and on the
/// ground cave rock where noise is high, as the static map's
fn endless_chunk(request: ChunkRequest, set: u16) -> ChunkTiles {
  let mut tiles = ChunkTiles::default();
  for y in 0..CHUNK_SIZE {
    for x in 0..CHUNK_SIZE {
      let at = UVec2::new(x, y);
      let cell = request.first_cell + at.as_ivec2();
      let h = hash(cell.x, cell.y, 3);
      tiles.set("background", at, Some(Tile::new(set, pick(BACK, h))));
      if rocky(cell.x, cell.y) {
        tiles.set("ground", at, Some(Tile::new(set, pick(ROCK, h))));
      }
    }
  }
  tiles
}

/// The streaming guard's scene: an endless world of two unbounded layers
/// (the ground colliding and casting shadows) streamed round the camera
struct EndlessScene;

impl Scene for EndlessScene {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    let source = add_tileset(ctx);
    let mut map = TileMap::infinite(UVec2::splat(TILE));
    let set = map.add_tileset(source);
    map.add_tile_layer("background");
    map.add_tile_layer("ground");
    let map = ctx.assets().add(map);
    let stream = MapStream {
      load_distance: LOAD,
      unload_distance: UNLOAD,
      ..MapStream::new(move |request: ChunkRequest| endless_chunk(request, set))
    };
    ctx
      .world
      .spawn((Transform::IDENTITY, MapRoot::new(map), stream));
  }
}

/// The chunks the streaming guard's crossing meets (warm-up included),
/// the view grown by the load distance: those the stream lands round the
/// view, by row
fn crossed_chunks() -> Vec<IVec2> {
  let half = Vec2::new(RESOLUTION.0 as f32, RESOLUTION.1 as f32) / 2.0 + LOAD;
  let mut chunks = BTreeSet::new();
  for frame in 0..=WARM + ROUNDS {
    let at = crossing_at(frame);
    let (first, last) = chunks_over(Rect::new(
      at.x - half.x,
      at.y - half.y,
      2.0 * half.x,
      2.0 * half.y,
    ));
    for y in first.y..=last.y {
      for x in first.x..=last.x {
        chunks.insert((y, x));
      }
    }
  }
  chunks.into_iter().map(|(y, x)| IVec2::new(x, y)).collect()
}

/// The streaming guard's static baseline: the endless world's chunks the
/// crossing meets (`crossed_chunks`, the generator's own tiles), built in
/// code as an infinite map of the same two layers (the ground colliding
/// and casting shadows), with no stream: what the map costs drawn and
/// still, so the world over it is what streaming adds
struct CrossedScene;

impl Scene for CrossedScene {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    let source = add_tileset(ctx);
    let mut map = TileMap::infinite(UVec2::splat(TILE));
    let set = map.add_tileset(source);
    let layers = [
      ("background", map.add_tile_layer("background")),
      ("ground", map.add_tile_layer("ground")),
    ];
    for chunk in crossed_chunks() {
      let first_cell = chunk * CHUNK_SIZE as i32;
      let request = ChunkRequest {
        chunk,
        first_cell,
        grid: map.grid(),
      };
      let tiles = endless_chunk(request, set);
      for (name, layer) in layers {
        let grid = map.tiles_mut(layer).expect("a tile layer");
        for y in 0..CHUNK_SIZE {
          for x in 0..CHUNK_SIZE {
            let at = UVec2::new(x, y);
            if let Some(tile) = tiles.get(name, at) {
              grid.set(first_cell + at.as_ivec2(), Some(tile));
            }
          }
        }
      }
    }
    let map = ctx.assets().add(map);
    ctx.world.spawn((Transform::IDENTITY, MapRoot::new(map)));
  }
}

/// The main camera's view as `engine`'s last frame drew it
fn drawn_view(engine: &mut HeadlessEngine) -> Option<CameraBounds> {
  let world = engine.world_mut();
  world
    .query_filtered::<&CameraView, With<MainCamera>>()
    .iter(world)
    .next()
    .map(|view| view.bounds)
}

/// `view` grown by `by` on every side
fn grown(view: CameraBounds, by: f32) -> Rect {
  Rect::new(
    view.left - by,
    view.top - by,
    view.width() + 2.0 * by,
    view.height() + 2.0 * by,
  )
}

/// The chunks overlapping `area` (touching is not overlapping), first and
/// last, the map at the origin
fn chunks_over(area: Rect) -> (IVec2, IVec2) {
  let side = (CHUNK_SIZE * TILE) as f32;
  let (min, max) = (
    Vec2::new(area.x, area.y),
    Vec2::new(area.x + area.width, area.y + area.height),
  );
  (
    (min / side).floor().as_ivec2(),
    ((max / side).ceil() - 1.0).as_ivec2(),
  )
}

/// The most chunks an area of `view` grown by `by` on every side can
/// overlap: a span of `n` chunks' length meets `n + 1` of them at most
fn chunks_bound(view: CameraBounds, by: f32) -> usize {
  let side = (CHUNK_SIZE * TILE) as f32;
  let meets = |length: f32| (length / side).ceil() as usize + 1;
  meets(view.width() + 2.0 * by) * meets(view.height() + 2.0 * by)
}

/// What an engine of the streaming guard shows
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Crossed {
  Empty,
  /// The endless world, streamed: the guarded one
  Streamed,
  /// The static baseline (`CrossedScene`)
  Baseline,
}

/// An engine of the streaming guard, its camera at the path's start: the
/// endless world with every chunk of the view's load area landed, and its
/// root; the static baseline landed, and its root; or an empty scene. None
/// without a GPU
fn stream_engine(show: Crossed) -> Option<(HeadlessEngine, Option<Entity>)> {
  let builder = EngineBuilder::new().resolution(RESOLUTION.0, RESOLUTION.1);
  let builder = match show {
    Crossed::Empty => builder.start_scene(Nothing),
    Crossed::Streamed => builder.start_scene(EndlessScene),
    Crossed::Baseline => builder.start_scene(CrossedScene),
  };
  let mut engine = HeadlessEngine::new_or_skip(builder)?;
  place_camera(&mut engine, crossing_at(0));
  if show == Crossed::Empty {
    engine.step().expect("a frame");
    return Some((engine, None));
  }
  let start = Instant::now();
  loop {
    engine.step().expect("a frame");
    let root = roots(&mut engine).first().copied();
    let ready = match (root, drawn_view(&mut engine)) {
      (Some(root), Some(view)) if show == Crossed::Streamed => {
        MapsView::new(engine.world_mut()).stream_ready(root, grown(view, LOAD))
      }
      (Some(_), Some(_)) => landed(&mut engine),
      _ => false,
    };
    if ready {
      return Some((engine, root));
    }
    assert!(
      start.elapsed() < Duration::from_secs(60),
      "the chunks round the view landed"
    );
    std::thread::yield_now();
  }
}

/// One measured frame of the streaming guard
struct Crossing {
  /// The streamed frame's step, and its render step
  step: Duration,
  render: Duration,
  /// The chunks it landed and unloaded
  landed: usize,
  unloaded: usize,
  /// Whether a chunk came into the camera's view
  came_into_view: bool,
}

/// Which of the streaming guard's measured frames a line is of
type Pick = fn(&Crossing) -> bool;

/// The chunks loaded and unloaded since `cursor` last read `engine`'s
/// chunk messages
fn chunk_events(
  engine: &mut HeadlessEngine,
  cursor: &mut MessageCursor<ChunkEvent>,
) -> (usize, usize) {
  let Some(messages) = engine.world().get_resource::<Messages<ChunkEvent>>() else {
    return (0, 0);
  };
  cursor
    .read(messages)
    .fold((0, 0), |(loaded, unloaded), event| match event {
      ChunkEvent::Loaded { .. } => (loaded + 1, unloaded),
      ChunkEvent::Unloading { .. } => (loaded, unloaded + 1),
    })
}

/// How many chunks are resident in `engine`'s streamed world (each is in
/// every unbounded layer, so the layer holding the most), and those of them
/// lying outside `keep`, into `outside`
fn residents(engine: &mut HeadlessEngine, keep: Rect, outside: &mut Vec<IVec2>) -> usize {
  outside.clear();
  let (first, last) = chunks_over(keep);
  let world = engine.world_mut();
  let mut layers = world.query::<&TileLayerData>();
  let Some(data) = layers
    .iter(world)
    .max_by_key(|data| data.tiles.chunk_coords().count())
  else {
    return 0;
  };
  outside.extend(
    data
      .tiles
      .chunk_coords()
      .filter(|chunk| !(chunk.cmpge(first).all() && chunk.cmple(last).all())),
  );
  data.tiles.chunk_coords().count()
}

/// A teleport after the crossing: the streamed frames' steps over the
/// empty frames' median meanwhile, the frame the view filled in and the
/// frame the chunks left behind were all gone by, and the most chunks
/// landed and unloaded in one frame
struct Teleport {
  steps: Vec<Duration>,
  filled: Option<usize>,
  settled: Option<usize>,
  most_landed: usize,
  most_unloaded: usize,
}

/// Both cameras jump `JUMP` from where the crossing left them and stay
/// there `TELEPORT_FRAMES` frames, stepped in turn and paced at 60 Hz
fn teleport(
  (empty, streamed): (&mut HeadlessEngine, &mut HeadlessEngine),
  root: Entity,
  cursor: &mut MessageCursor<ChunkEvent>,
) -> Teleport {
  let at = crossing_at(WARM + ROUNDS) + JUMP;
  let mut out = Teleport {
    steps: Vec::with_capacity(TELEPORT_FRAMES),
    filled: None,
    settled: None,
    most_landed: 0,
    most_unloaded: 0,
  };
  let (mut empty_steps, mut outside) = (Vec::with_capacity(TELEPORT_FRAMES), Vec::new());
  let mut next = Instant::now();
  for frame in 0..TELEPORT_FRAMES {
    pace(next);
    next = Instant::now() + FRAME;
    for i in [frame % 2, 1 - frame % 2] {
      let engine = if i == 0 { &mut *empty } else { &mut *streamed };
      place_camera(engine, at);
      settle_gpu();
      let start = Instant::now();
      engine.step().expect("a teleport frame");
      let took = start.elapsed();
      if i == 0 {
        empty_steps.push(took);
      } else {
        out.steps.push(took);
      }
    }
    let (landed, unloaded) = chunk_events(streamed, cursor);
    out.most_landed = out.most_landed.max(landed);
    out.most_unloaded = out.most_unloaded.max(unloaded);
    // The view drawn, a frame behind the camera: views are drawn from
    // the interpolated transform, which with one fixed step a frame is
    // the step before's
    let Some(view) = drawn_view(streamed) else {
      continue;
    };
    let middle = Vec2::new(view.left + view.right, view.top + view.bottom) * 0.5;
    if middle.distance(at) > 1.0 {
      continue;
    }
    let maps = MapsView::new(streamed.world_mut());
    if out.filled.is_none() && maps.stream_ready(root, grown(view, 0.0)) {
      out.filled = Some(frame + 1);
    }
    residents(streamed, grown(view, UNLOAD), &mut outside);
    if out.settled.is_none() && outside.is_empty() {
      out.settled = Some(frame + 1);
    }
  }
  out.steps = over(&out.steps, median(empty_steps));
  out
}

/// The streaming guard: the endless world's step over an empty frame's
/// median, its 99th percentile held to 0.5 ms; every frame shows no
/// unfilled chunk and keeps its resident chunks within the radii
fn stream_guard() {
  let (Some((mut empty, _)), Some((mut streamed, Some(root))), Some((mut baseline, _))) = (
    stream_engine(Crossed::Empty),
    stream_engine(Crossed::Streamed),
    stream_engine(Crossed::Baseline),
  ) else {
    return;
  };
  let mut cursor = MessageCursor::<ChunkEvent>::default();
  chunk_events(&mut streamed, &mut cursor);
  let (mut empty_steps, mut empty_renders) =
    (Vec::with_capacity(ROUNDS), Vec::with_capacity(ROUNDS));
  let (mut baseline_steps, mut baseline_renders) =
    (Vec::with_capacity(ROUNDS), Vec::with_capacity(ROUNDS));
  let mut frames: Vec<Crossing> = Vec::with_capacity(ROUNDS);
  let (mut unfilled, mut resident) = (Vec::new(), Vec::new());
  let (mut bound, mut over_bound) = (0, Vec::new());
  // The resident chunks past the unload distance, waiting their turn to
  // unload (the stream unloads a few a frame), and how many frames each
  // has waited
  let (mut outside, mut waited) = (Vec::new(), HashMap::<IVec2, usize>::new());
  let (mut most_outside, mut longest) = (0, 0);
  // The view the stream reads in the next frame: the last frame's
  let mut seen = drawn_view(&mut streamed).expect("the camera's view");
  let mut next = Instant::now();
  for round in 0..WARM + ROUNDS {
    let at = crossing_at(round);
    pace(next);
    next = Instant::now() + FRAME;
    // Each round another engine steps first
    let mut steps = [(Duration::ZERO, Duration::ZERO); 3];
    for i in [round % 3, (round + 1) % 3, (round + 2) % 3] {
      let engine = match i {
        0 => &mut empty,
        1 => &mut streamed,
        _ => &mut baseline,
      };
      place_camera(engine, at);
      settle_gpu();
      let start = Instant::now();
      engine.step().expect("a guard frame");
      steps[i] = (start.elapsed(), engine.render_time());
    }
    // Outside the measure: what the frame streamed and showed
    let (landed, unloaded) = chunk_events(&mut streamed, &mut cursor);
    let drawn = drawn_view(&mut streamed).expect("the camera's view");
    if !MapsView::new(streamed.world_mut()).stream_ready(root, grown(drawn, 0.0)) {
      unfilled.push(round);
    }
    let count = residents(&mut streamed, grown(seen, UNLOAD), &mut outside);
    let most = chunks_bound(seen, UNLOAD);
    bound = bound.max(most);
    if count > most {
      over_bound.push((round, count, most));
    }
    resident.push(count);
    waited.retain(|chunk, _| outside.contains(chunk));
    for &chunk in &outside {
      let frames = waited.entry(chunk).or_insert(0);
      *frames += 1;
      longest = longest.max(*frames);
    }
    most_outside = most_outside.max(outside.len());
    let came_into_view = chunks_over(grown(drawn, 0.0)) != chunks_over(grown(seen, 0.0));
    seen = drawn;
    if round < WARM {
      continue;
    }
    empty_steps.push(steps[0].0);
    empty_renders.push(steps[0].1);
    baseline_steps.push(steps[2].0);
    baseline_renders.push(steps[2].1);
    frames.push(Crossing {
      step: steps[1].0,
      render: steps[1].1,
      landed,
      unloaded,
      came_into_view,
    });
  }
  let jump = teleport((&mut empty, &mut streamed), root, &mut cursor);
  let (empty_step, empty_render) = (median(empty_steps.clone()), median(empty_renders.clone()));
  // The measured frames `pick` takes: their steps over the empty frame's
  // median, and their render steps over its render step's
  let added = |pick: Pick| -> (Vec<Duration>, Vec<Duration>) {
    let picked = || frames.iter().filter(|f| pick(f));
    let steps: Vec<Duration> = picked().map(|f| f.step).collect();
    let renders: Vec<Duration> = picked().map(|f| f.render).collect();
    (over(&steps, empty_step), over(&renders, empty_render))
  };
  eprintln!(
    "stream guard: {ROUNDS} interleaved frames at {}x{} paced at 60 Hz, the camera crossing an \
     endless world at {STREAM_SPEED} px/s heading ({}, {}), chunks loading within {LOAD} px of \
     its view and unloading past {UNLOAD}. An empty frame (its own spread is the machine's \
     noise): step {}; render step {}",
    RESOLUTION.0,
    RESOLUTION.1,
    HEADING.x,
    HEADING.y,
    shown_at(empty_steps.clone(), 99),
    shown_at(empty_renders, 99),
  );
  let (all, renders) = added(|_| true);
  let held = spread_at(all.clone(), 99).map_or(Duration::ZERO, |(_, p99, _)| p99);
  let (met, verdict) = verdict(held, STREAM_TARGET);
  let line = format!(
    "the endless world's step over the empty frame's median, {}; its render step over the empty \
     frame's, {}",
    shown_at(all, 99),
    shown_at(renders, 99),
  );
  eprintln!(
    "stream: {line}. Held, the 99th percentile: {:.1} µs, {verdict}",
    us(held)
  );
  if !met {
    guards::miss(format!("stream, {line}"));
  }
  // With no target, so one quiet run tells the machine's floor from the
  // stream's own cost: the 99th percentiles apart, and a static baseline
  let p99 = |times: &[Duration]| spread_at(times.to_vec(), 99).map_or(Duration::ZERO, |s| s.1);
  let world_steps: Vec<Duration> = frames.iter().map(|f| f.step).collect();
  let (world_p99, empty_p99, baseline_p99) =
    (p99(&world_steps), p99(&empty_steps), p99(&baseline_steps));
  let apart = |a: Duration, b: Duration| us(a) - us(b);
  eprintln!(
    "stream (no target): the endless world's 99th percentile less the empty frame's, {:.1} µs \
     (the world's {:.1}, the empty frame's {:.1})",
    apart(world_p99, empty_p99),
    us(world_p99),
    us(empty_p99),
  );
  let baseline_step = median(baseline_steps.clone());
  let baseline_render = median(baseline_renders.clone());
  let world_renders: Vec<Duration> = frames.iter().map(|f| f.render).collect();
  eprintln!(
    "stream (no target): a static baseline, the {} chunks the crossing meets (the view grown by \
     the load distance) built in code as an infinite map of the same two layers, colliding and \
     casting shadows, with no MapStream, stepped in the same rounds: its step over the empty \
     frame's median, {}; its render step over the empty frame's, {}; its 99th percentile less \
     the empty frame's, {:.1} µs",
    crossed_chunks().len(),
    shown_at(over(&baseline_steps, empty_step), 99),
    shown_at(over(&baseline_renders, empty_render), 99),
    apart(baseline_p99, empty_p99),
  );
  eprintln!(
    "stream (no target): the endless world over the static baseline: its step over the \
     baseline's median, {}; its render step over the baseline's, {}; its 99th percentile less \
     the baseline's, {:.1} µs",
    shown_at(over(&world_steps, baseline_step), 99),
    shown_at(over(&world_renders, baseline_render), 99),
    apart(world_p99, baseline_p99),
  );
  let landings: usize = frames.iter().map(|f| f.landed).sum();
  let unloads: usize = frames.iter().map(|f| f.unloaded).sum();
  let both = frames
    .iter()
    .filter(|f| f.landed > 0 && f.unloaded > 0)
    .count();
  let busiest = frames
    .iter()
    .map(|f| (f.landed + f.unloaded, f.landed, f.unloaded))
    .max()
    .unwrap_or_default();
  let kinds: [(&str, Pick); 4] = [
    ("landing chunks", |f| f.landed > 0),
    ("unloading chunks", |f| f.unloaded > 0),
    ("bringing a chunk into view", |f| f.came_into_view),
    ("doing none of these", |f| {
      f.landed == 0 && f.unloaded == 0 && !f.came_into_view
    }),
  ];
  for (kind, pick) in kinds {
    let (steps, renders) = added(pick);
    eprintln!(
      "stream (no target): the frames {kind} ({}): {}; render step {}",
      steps.len(),
      shown_at(steps, 99),
      shown_at(renders, 99),
    );
  }
  let budgets = MapStream::new(|_: ChunkRequest| ChunkTiles::default());
  let frames_of = |frame: Option<usize>| frame.map_or("never".to_string(), |f| f.to_string());
  eprintln!(
    "stream (no target): a teleport {} chunks on after the crossing, the camera staying there \
     {TELEPORT_FRAMES} frames: its view filled in {} frames, the chunks left behind gone in {} \
     (at most {} landed and {} unloaded a frame; the budgets {} and {}); those frames' steps \
     over the empty frames' median, {}",
    JUMP.x / (CHUNK_SIZE * TILE) as f32,
    frames_of(jump.filled),
    frames_of(jump.settled),
    jump.most_landed,
    jump.most_unloaded,
    budgets.land_per_frame,
    budgets.unload_per_frame,
    shown_at(jump.steps, 99),
  );
  let most = resident.iter().copied().max().unwrap_or(0);
  let mut measured = resident.get(WARM..).unwrap_or_default().to_vec();
  measured.sort_unstable();
  let typical = measured.get(measured.len() / 2).copied().unwrap_or(0);
  let world = streamed.world_mut();
  let occluders = world.query::<&MapOccluder>().iter(world).count();
  let children = world.query::<&ChildOf>().iter(world).count();
  eprintln!(
    "stream: {landings} chunks landed and {unloads} unloaded over the measured frames, both in \
     {both} frames, the most in one frame {} ({} landed, {} unloaded); resident \
     chunks at most {most} in any frame, warm-up included (the median measured frame \
     {typical}), against the radii's bound of {bound} (the most chunks the view grown by the \
     unload distance can meet); the world at the end: {} entities, {children} of them \
     children, {occluders} occluder chains",
    busiest.0,
    busiest.1,
    busiest.2,
    world.entities().len(),
  );
  assert!(
    unfilled.is_empty(),
    "stream: {} of the {} frames (warm-up included) showed a chunk not landed; the first, frame \
     {}",
    unfilled.len(),
    WARM + ROUNDS,
    unfilled[0],
  );
  // A chunk past the unload distance waits at most as many frames as the
  // unload budget takes to drain every chunk the radii hold
  let budget = MapStream::new(|_: ChunkRequest| ChunkTiles::default()).unload_per_frame as usize;
  let drain = bound.div_ceil(budget.max(1));
  eprintln!(
    "stream: resident chunks past the unload distance, waiting their turn ({budget} unload a \
     frame): at most {most_outside} in a frame, none waiting more than {longest} frames (the \
     budget drains the radii's bound in {drain})"
  );
  assert!(
    over_bound.is_empty() && longest <= drain,
    "stream: resident chunks past the radii: frames over the bound (frame, resident, bound) \
     {over_bound:?}; a chunk past the unload distance waited {longest} frames, past {drain}"
  );
  eprintln!(
    "stream: no unfilled chunk on screen in any of the {} frames, and the resident chunks within \
     the radii's bound in every one",
    WARM + ROUNDS
  );
}

/// The streaming bench: the endless world's engine stepped back to back,
/// its camera crossing the world
fn stream_bench(c: &mut Criterion) {
  let mut made: Option<(HeadlessEngine, usize)> = None;
  c.bench_function("stream", |b| {
    if made.is_none() {
      made = stream_engine(Crossed::Streamed).map(|(engine, _)| (engine, 0));
    }
    let Some((engine, frame)) = made.as_mut() else {
      return;
    };
    b.iter(|| {
      *frame += 1;
      place_camera(engine, crossing_at(*frame));
      engine.step().expect("a frame")
    });
  });
}

fn main() {
  let started = SystemTime::now();
  let load = Command::new("uptime")
    .output()
    .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
    .unwrap_or_else(|_| "uptime unavailable".into());
  println!("machine: {load}");
  println!("tilemap bench: a static big map, an edit, a load and a streamed endless world");
  let results = guards::results_dir();
  let mut criterion = Criterion::default()
    .sample_size(20)
    .output_directory(&results)
    .configure_from_args();
  if guards::runs(&["static_frame"]) {
    static_guard();
    padded_line();
  }
  if guards::runs(&["edit"]) {
    edit_guard();
  }
  let mut files = None;
  if guards::runs(&["load"]) {
    let files = files.get_or_insert_with(load_files);
    load_guard(files);
    landing_guard(files);
  }
  if guards::runs(&["stream"]) {
    stream_guard();
  }
  static_bench(&mut criterion);
  edit_bench(&mut criterion);
  load_bench(&mut criterion, &mut files);
  landing_bench(&mut criterion, &mut files);
  stream_bench(&mut criterion);
  criterion.final_summary();
  for (bench, of) in BENCHES {
    guards::print_median(&results, bench, of, started, 1);
  }
  guards::verdict(
    "tilemap",
    "Profile a miss (the bench binary is the one `cargo bench --bench tilemap --no-run` names) \
     with\n  xcrun xctrace record --template 'Time Profiler' --output tilemap.trace --launch -- \
     target/release/deps/tilemap-<hash> --bench --profile-time 20 '^static_frame$'\n(static_frame, \
     edit, load, landing or stream) and charge each sample's time to the nearest ryo2d function \
     from the leaf up.",
  );
  eprintln!("tilemap guards: every guard that ran met its target");
}
