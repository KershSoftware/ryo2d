//! Maps against the spec's speed targets (6a's, and the streaming target),
//! on every grid, with a GPU where a frame is drawn (`HeadlessEngine`,
//! 1920 x 1080, at zoom 1).
//!
//! The grids (`Grid`): orthogonal cells of 16 px, and cells of as large an
//! area on the others, so a 1080p view holds about as many tiles on each:
//! isometric diamonds of 32 x 16, staggered ones of 32 x 16 (rows shifted,
//! the odd ones), hexagons of 16 x 16 with sides of 8 (pointy-topped, in
//! rows, the odd ones shifted) and oblique 16 px cells skewed 4 px across a
//! row. Each map's tileset has tiles of its cell's size, the rock's
//! collision shape a whole cell (the tile's rectangle, on isometric and
//! staggered grids its diamond, on the hexagonal grid its hexagon), so on
//! every grid the map's layers collide and cast shadows.
//!
//! Before criterion runs, four guards:
//! - `static_frame`: a 1024 x 1024 map of four layers (a background on
//!   every cell, cave rock from noise that collides and casts shadows, and
//!   two sparse layers of props) built in code over one atlas, the camera
//!   flying a long loop over it at 444 to 690 px/s (561 on average over the
//!   guard's frames), against an empty scene. A frame's cost is the wall
//!   time of the whole step (`HeadlessEngine::step`: every phase, the render
//!   step and its submit) with the GPU finished before each step, outside
//!   the measure, as `benches/ui_render.rs`'s guard does; the engines step
//!   in turn, one frame each, so a change in the machine's load moves them
//!   alike. The median frame over the empty frame's median is held to
//!   50 µs (the spec). Two rounds of nine engines (`Round`), each against
//!   an empty frame of its own: an engine stepping evicts the others' data,
//!   so every held map steps in the same company. The orthogonal round: the
//!   map; the same map built with unbounded layers (`TileMap::infinite`),
//!   held to 50 µs the same way; and with no target, the same map drawn
//!   only (no collision, no shadows), colliding without shadows, casting
//!   shadows without collision (which split what the map's child entities
//!   cost), with its props layer y-sorted, and lit (a shadowed light
//!   following the camera, against an empty lit scene), which shows what
//!   lighting's gather of every map occluder costs. The grids' round: the
//!   map on each other grid, held to 50 µs, and on the isometric grid with
//!   tall props too (32 x 48 tiles of a tileset of their own on its plain
//!   props layer, which leave their cells' tops, so the layer draws in
//!   Tiled's order, checked: every props chunk it draws was built knowing
//!   its tiles leave their cells, as only that path builds them), held the
//!   same way; beside them, so the grids compare in one round and the round
//!   holds nine engines as the first, the orthogonal map, the same with
//!   unbounded layers (held in the first round) and drawn only. On another
//!   grid the camera flies the orthogonal loop in cells, taken onto the
//!   grid's cells (so it stays over the map, within an isometric map's
//!   diamond), at the rate that keeps its average speed the orthogonal
//!   camera's (`Flight`). Each map's tile draws a frame are printed beside
//!   its times; then each map's entities and occluder chains, and its first
//!   build (its landing job, the landing frame and the one after), stepped
//!   back to back, once. Then what a padded tileset cache hit costs, an
//!   atlas's and a 1,000-tile collection's;
//! - `edit`: on every grid, one-tile edits on a 1024 x 1024 four-layer map
//!   of 16,000 islands with two loops each spanning four chunks: a jagged
//!   skyline 128 cells wide and 32 high, whose column 99 holds only its
//!   bottom cell (a neck), and a one-cell serpentine filling the same 4 x 1
//!   chunks (the longest loop that area can hold). The shapes are laid in
//!   the grid's square lattice (the cells its collision traces in: the
//!   grid's own on orthogonal, isometric and oblique grids; on the
//!   staggered grid its diamonds' isometric lattice, from a lattice chunk
//!   whose shapes all lie in the map), so each loop spans four lattice
//!   chunks; the hexagonal grid has no square lattice, and its shapes are
//!   laid in its own cells, a row of hexagons for a row of cells, so each
//!   loop spans four of its chunks. The camera stands over the edited
//!   cells, each edit's chunk drawn in the last warm-up frame (checked).
//!   Each loop is edited 400 times a way: a bump (a tile set on top of it
//!   and cleared again: it stays one loop), and a split and a join (a cell
//!   cleared and set again: the skyline's neck; the middle of the
//!   serpentine's first row, and of its middle row, which splits it into
//!   halves of about 2,000 edges, the worst split a loop of four chunks can
//!   have). On the hexagonal map, whose colliders hold its cells' sides in
//!   parts of at most 128 edges (a chunk, else its quarters, else their
//!   blocks), each a side's earlier cell's, and its shape cells' rings in
//!   parts of their own (split likewise, a block's past the budget into its
//!   4 x 4 sub-blocks), six cases more, each with the camera over it, 400
//!   edits each: a lone cell set and cleared where four of the islands'
//!   chunks meet (its sides in four chunks' parts, and their shadows built
//!   again); part churn, a lone cell set and cleared where four empty chunks
//!   meet (four parts made and dropped again, checked); a chunk of 127 edges
//!   taken past the budget by a lone cell, which drops its one part and
//!   makes seven (its quarters', one of them split into its blocks:
//!   checked), and set back, the chunk and the cells giving its quarters an
//!   edge each emptied and filled again before each such pair, so each
//!   starts as one part; with no target, a lone cell set and cleared where
//!   four dense chunks meet (four cells in ten rock): the guard holds the
//!   islands map on every grid, and a dense corner on none; and a chunk
//!   heavy with rings, in the keep-out area apart from the islands, its
//!   every cell a ring tile whose shape is a hexagon inset within its cell's
//!   (six edges a cell, so each 8 x 8 block's rings, 384 edges, pass the
//!   budget and its 64 sub-blocks are its parts of rings, 96 edges each:
//!   checked once), edited twice: a ring cell in a block's middle cleared
//!   and set back (its sub-block's part of rings rebuilt, no part made or
//!   dropped: checked), and one where four blocks meet turned whole and back
//!   (its sub-block's part of rings rebuilt, and the chunk's part of sides,
//!   its six sides, made and dropped: checked). Each edit is timed in three
//!   parts, each held to 100 µs (the spec): the chunk rebuild (the render
//!   step of the frame after the edit, less the median render step of the
//!   ordinary frames between edits: the chunk's instances built and
//!   uploaded), the loop rebuild (the map's collision sync, which classifies
//!   the chunk again, then physics' tile collider rebuild, which retraces
//!   the loops through the cell and reshapes their chunk colliders, and its
//!   collider sync, which builds the new shapes for Rapier; each run on the
//!   engine's world in a schedule of its own, commands applied:
//!   `ryo2d::core::tilemap::edit_schedules`) and the occluder rebuild (the
//!   occluder sync, commands applied). A frame passes between edits, as in a
//!   game, so their data is not in cache. Beside them, with no target: the
//!   edit frame's whole step over an ordinary frame's, the loop and occluder
//!   rebuilds of as many edits back to back (their data in cache), and on
//!   the hexagonal map the loop and occluder rebuilds together, each edit's
//!   sum;
//! - `load`: on every grid, a 256 x 256 four-layer Tiled map (TMX, every
//!   layer base64 and zlib) over an external TSX tileset and its PNG, read
//!   from a `MemorySource`. Its load, from `load` to the map ready with its
//!   tileset and image, on the asset workers with the main thread pumping,
//!   is held to 20 ms (the spec; twenty loads of fresh files, the median),
//!   and so is the same map saved as an infinite TMX (16 x 16 chunks, zlib
//!   each); on the orthogonal grid a level change (the tileset loaded
//!   already) is printed beside it. Then, orthogonal, the landing: an engine
//!   at 60 Hz (paced with a spin wait) loads and spawns a fresh copy of the
//!   map every 40 frames and despawns it 32 frames later, twenty times. The
//!   frames the map's asset lands in, the map lands in and the one after
//!   are its landing frames, and the slowest of them over the run's median
//!   ordinary frame is held to 0.5 ms (the spec: no hitch). A run over that
//!   is measured once more, and the better run is held. The despawn frames
//!   count as neither. The first landing makes the padded tileset's texture
//!   (its landing job padded it; later ones find the padding kept), and is
//!   printed apart. The landing job's time, spawn to landed with frames
//!   stepped back to back, is printed too: on the web, which has no
//!   threads, the job runs in the frame it is sent;
//! - `stream`: on the orthogonal grid, then the isometric, an endless world
//!   streamed from a generator (`MapStream` with its default budgets,
//!   loading within 512 px of the view and unloading past 1,536): two
//!   unbounded layers over the grid's atlas, a background on every cell and
//!   cave rock from the same noise on the ground, which collides and casts
//!   shadows. Its camera, the stream's anchor, crosses it on a straight
//!   line at 2,000 px/s (heading 0.8, 0.6, so rows and columns of chunks
//!   come and go both) for the guard's frames and the warm-up before them,
//!   against an empty scene whose camera flies the same path; the engines
//!   (and the baseline below) step in turn, one frame each, each round
//!   paced at 60 Hz with a spin wait, as the landing is, so the generator's
//!   thread keeps the time a game gives it. A frame's cost is the whole
//!   step, the GPU finished before it, as the static guard's. The 99th
//!   percentile of the world's step over the empty frame's median is held
//!   to 0.5 ms (the spec: any frame; one slowest frame is the machine's
//!   noise, so the maximum is printed beside it, and so is the empty
//!   frame's own spread). Every frame, warm-up included, two checks fail
//!   the run at once (assertions): every chunk overlapping the camera's
//!   view as the frame drew it is resident (no unfilled chunk on screen),
//!   and the resident chunks number no more than the view grown by the
//!   unload distance can meet (printed; off the orthogonal grid, the most
//!   chunks' boxes a box that size meets), each chunk past that distance
//!   gone within the frames the unload budget takes to drain them all.
//!   Beside them, with no target: the world's 99th percentile less the
//!   empty frame's; a static baseline, a third engine stepped in the same
//!   rounds on the chunks the crossing meets (the view grown by the load
//!   distance: 1,144 chunks on the orthogonal grid, 1,228 on the isometric,
//!   2.3 million cells or more of the generator's own tiles) built in code
//!   as an infinite map with no stream, its step over the empty frame's and
//!   the world's over its own, so streaming's cost stands apart from the
//!   map's; the frames that landed chunks, unloaded them, brought a chunk
//!   into view (its first draw) and the others, each with its render step;
//!   then a teleport 51,200 px on (100 orthogonal chunks), the camera
//!   staying there 180 frames: the frames its view took to fill and the
//!   chunks left behind to go, and those frames' steps.
//!
//! Each verdict holds the median (the landing, its slowest frame; the
//! stream, its 99th percentile); the 95th percentile (the stream's, the
//! 99th) and the maximum are printed beside it. A miss, named by its grid,
//! fails the run once criterion's benches have run, printing the command
//! to profile it with. Each guard runs only where criterion measures the
//! bench of its name: with no filter or one naming it, and not for
//! `--list`, `--test` or `--profile-time`. The criterion benches, for the
//! profiles, on the orthogonal grid, or on the grid `RYO_TILEMAP_GRID`
//! names (`isometric`, `staggered`, `hexagonal` or `oblique`; `tall`, the
//! isometric map with tall props): `static_frame` steps the map engine with
//! its camera flying, `edit` sets or clears the serpentine's cell and steps
//! the engine, `load` loads the map with its tileset and image in a fresh
//! registry, `landing` (orthogonal) spawns the loaded map and times its
//! landing frame, and `stream` steps the endless world back to back, its
//! camera crossing it (out and back along a leg of 4,000 frames, so a long
//! profile stays within collision's reach of the origin). On a grid other
//! than the orthogonal and the isometric, the streaming guard runs on that
//! grid's endless world too, measured with no target.
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
use glam::DVec2;
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
/// shape), the edit map's ring tile (on the hexagonal grid a shape of its
/// own: `Grid::ring`), background, props and foreground details
const COLUMNS: u32 = 16;
const ROCK: std::ops::Range<u32> = 0..8;
const RING: u32 = 8;
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

/// The pixels of an atlas of `COLUMNS` x `COLUMNS` tiles of `tile` px (the
/// orthogonal maps' is 256 x 256), each tile its own colour with a darker
/// rim, so no two tiles look alike
fn atlas_pixels(tile: UVec2) -> Vec<u8> {
  let (width, height) = (COLUMNS * tile.x, COLUMNS * tile.y);
  let mut data = Vec::with_capacity((width * height * 4) as usize);
  for y in 0..height {
    for x in 0..width {
      let id = (y / tile.y) * COLUMNS + x / tile.x;
      let h = hash(id as i32, 0, 9);
      let (u, v) = (x % tile.x, y % tile.y);
      let rim = u == 0 || v == 0 || u == tile.x - 1 || v == tile.y - 1;
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

/// The tileset over `image`, `COLUMNS` x `COLUMNS` tiles of `tile` px, its
/// rock tiles carrying `rock` as their collision shape and its ring tile
/// `ring` (none without either)
fn tileset(
  image: Handle<Image>,
  tile: UVec2,
  rock: Option<Vec<Vec2>>,
  ring: Option<Vec<Vec2>>,
) -> Tileset {
  let mut set = Tileset::new(
    "bench",
    tile,
    TilesetImage::Atlas {
      image,
      size: tile * COLUMNS,
    },
  );
  set.columns = COLUMNS;
  set.tile_count = COLUMNS * COLUMNS;
  let shaped = rock
    .into_iter()
    .flat_map(|points| ROCK.map(move |id| (id, points.clone())))
    .chain(ring.map(|points| (RING, points)));
  for (id, points) in shaped {
    set.insert_tile(
      id,
      TileData {
        shapes: vec![TileShape {
          points,
          ..TileShape::default()
        }],
        ..TileData::default()
      },
    );
  }
  set
}

/// The grids the guards hold the spec's targets on. Orthogonal cells are
/// 16 px; the others' are as big (a diamond of 32 x 16, a 16 x 16 hexagon
/// of side 8 at a row step of 12, a 16 px square skewed 4 px across a row),
/// so a 1080p view holds about as many tiles on each
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Grid {
  Orthogonal,
  Isometric,
  Staggered,
  Hexagonal,
  Oblique,
}

impl Grid {
  const ALL: [Grid; 5] = [
    Grid::Orthogonal,
    Grid::Isometric,
    Grid::Staggered,
    Grid::Hexagonal,
    Grid::Oblique,
  ];

  fn shape(self) -> GridShape {
    match self {
      Grid::Orthogonal => GridShape::Orthogonal,
      Grid::Isometric => GridShape::Isometric,
      Grid::Staggered => GridShape::Staggered {
        axis: StaggerAxis::Y,
        index: StaggerIndex::Odd,
      },
      Grid::Hexagonal => GridShape::Hexagonal {
        axis: StaggerAxis::Y,
        index: StaggerIndex::Odd,
        side: 8,
      },
      Grid::Oblique => GridShape::Oblique {
        skew: IVec2::new(4, 0),
      },
    }
  }

  /// A cell's size: its tiles' too
  fn cell(self) -> UVec2 {
    match self {
      Grid::Isometric | Grid::Staggered => UVec2::new(32, 16),
      _ => UVec2::splat(TILE),
    }
  }

  /// What the guards' lines call it
  fn name(self) -> &'static str {
    match self {
      Grid::Orthogonal => "orthogonal",
      Grid::Isometric => "isometric 32 x 16",
      Grid::Staggered => "staggered 32 x 16",
      Grid::Hexagonal => "hexagonal 16 x 16 (side 8)",
      Grid::Oblique => "oblique 16 x 16 (skew 4, 0)",
    }
  }

  /// A file's name for it
  fn slug(self) -> &'static str {
    match self {
      Grid::Orthogonal => "ortho",
      Grid::Isometric => "iso",
      Grid::Staggered => "stag",
      Grid::Hexagonal => "hex",
      Grid::Oblique => "oblique",
    }
  }

  /// The rock tiles' collision shape, in their image's pixels: the cell's
  /// diamond on isometric and staggered grids, its hexagon on the hexagonal
  /// grid (a whole cell there), else the tile's rectangle
  fn rock(self) -> Vec<Vec2> {
    let (w, h) = (self.cell().x as f32, self.cell().y as f32);
    match self.shape() {
      GridShape::Isometric | GridShape::Staggered { .. } => vec![
        Vec2::new(w / 2.0, 0.0),
        Vec2::new(w, h / 2.0),
        Vec2::new(w / 2.0, h),
        Vec2::new(0.0, h / 2.0),
      ],
      // Pointy-topped, in rows: its slanted sides span the height its
      // upright ones leave
      GridShape::Hexagonal { side, .. } => {
        let slant = (h - side as f32) / 2.0;
        vec![
          Vec2::new(w / 2.0, 0.0),
          Vec2::new(w, slant),
          Vec2::new(w, h - slant),
          Vec2::new(w / 2.0, h),
          Vec2::new(0.0, h - slant),
          Vec2::new(0.0, slant),
        ]
      }
      _ => vec![
        Vec2::ZERO,
        Vec2::new(w, 0.0),
        Vec2::new(w, h),
        Vec2::new(0.0, h),
      ],
    }
  }

  /// The ring tile's collision shape, on the hexagonal grid alone: a hexagon
  /// inset within its cell's (`hex_ring_heavy_edit_timing`'s, in its image's
  /// pixels, wound as `rock`'s), so the tile collides as a shape of its own,
  /// a ring of six edges in its chunk's parts of rings
  fn ring(self) -> Option<Vec<Vec2>> {
    let inset = [
      (8.0, 2.0),
      (14.0, 5.0),
      (14.0, 11.0),
      (8.0, 14.0),
      (2.0, 11.0),
      (2.0, 5.0),
    ];
    (self == Grid::Hexagonal).then(|| inset.map(|(x, y)| Vec2::new(x, y)).to_vec())
  }

  /// A bounded map's grid of `side` cells
  fn map_grid(self, side: u32) -> MapGrid {
    MapGrid::for_map(self.shape(), self.cell(), UVec2::splat(side), false)
  }

  /// An infinite map's grid
  fn endless(self) -> MapGrid {
    TileMap::infinite(self.cell())
      .with_shape(self.shape())
      .grid()
  }
}

/// The tall props' tiles: 32 x 48, on the isometric grid's 32 x 16 cells
const TALL: UVec2 = UVec2::new(32, 48);

/// The grid the criterion benches step, for a profile of a grid's miss:
/// `RYO_TILEMAP_GRID` (`isometric`, `staggered`, `hexagonal` or `oblique`;
/// `tall` is the isometric map with tall props), else the orthogonal grid
fn benched() -> Show {
  match std::env::var("RYO_TILEMAP_GRID").as_deref() {
    Ok("isometric") => Show::Map(Grid::Isometric),
    Ok("tall") => Show::Tall,
    Ok("staggered") => Show::Map(Grid::Staggered),
    Ok("hexagonal") => Show::Map(Grid::Hexagonal),
    Ok("oblique") => Show::Map(Grid::Oblique),
    _ => Show::Map(Grid::Orthogonal),
  }
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

/// A map on `grid`'s cells of `cells` (as `cave_cells` gives them), each
/// layer's tiles from the first of `sets` but the props layer's, from the
/// last: bounded layers of `side` cells, or unbounded ones of an infinite
/// map
fn map_of(
  side: u32,
  grid: Grid,
  sets: &[TilesetSource],
  cells: &[Vec<Option<u32>>; 4],
  infinite: bool,
) -> TileMap {
  let mut map = if infinite {
    TileMap::infinite(grid.cell())
  } else {
    TileMap::new(UVec2::splat(side), grid.cell())
  }
  .with_shape(grid.shape());
  let sets: Vec<u16> = sets
    .iter()
    .map(|set| map.add_tileset(set.clone()))
    .collect();
  let (first, last) = (sets[0], sets[sets.len() - 1]);
  for (index, (name, layer_cells)) in LAYER_NAMES.iter().zip(cells).enumerate() {
    let set = if index == 2 { last } else { first };
    let layer = map.add_tile_layer(name);
    let tiles = map.tiles_mut(layer).expect("a tile layer");
    for (i, id) in layer_cells.iter().enumerate() {
      if let Some(id) = id {
        let cell = IVec2::new((i as u32 % side) as i32, (i as u32 / side) as i32);
        tiles.set(cell, Some(Tile::new(set, *id)));
      }
    }
  }
  map
}

/// An atlas tileset of `tile` px tiles added to `ctx`'s stores, its rock
/// tiles carrying `rock` and its ring tile `ring`
fn add_atlas(
  ctx: &mut SceneContext,
  tile: UVec2,
  rock: Option<Vec<Vec2>>,
  ring: Option<Vec<Vec2>>,
) -> TilesetSource {
  let size = tile * COLUMNS;
  let image = ctx
    .assets()
    .add(Image::from_rgba(atlas_pixels(tile), size.x, size.y));
  TilesetSource::Asset(ctx.assets().add(tileset(image, tile, rock, ring)))
}

/// `grid`'s tileset (tiles of its cell's size, the rock colliding as a
/// whole cell) added to `ctx`'s stores
fn add_tileset(ctx: &mut SceneContext, grid: Grid) -> TilesetSource {
  add_atlas(ctx, grid.cell(), Some(grid.rock()), None)
}

/// The edit map's tileset: `grid`'s, and on the hexagonal grid its ring
/// tile too (`Grid::ring`), which the chunk heavy with rings holds
fn add_edit_tileset(ctx: &mut SceneContext, grid: Grid) -> TilesetSource {
  add_atlas(ctx, grid.cell(), Some(grid.rock()), grid.ring())
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

/// The point `at` cells into `grid` (counted as on an orthogonal grid,
/// from cell (0, 0)'s corner): the centres of the four cells nearest it
/// blended, so on an orthogonal, isometric or oblique grid exactly the
/// grid's point, and on a staggered or hexagonal grid a point among those
/// cells
fn grid_point(grid: &MapGrid, at: Vec2) -> Vec2 {
  let at = at - 0.5;
  let base = at.floor();
  let (f, cell) = (at - base, base.as_ivec2());
  let centre = |x: i32, y: i32| grid.cell_center(cell + IVec2::new(x, y));
  let top = centre(0, 0).lerp(centre(1, 0), f.x);
  let bottom = centre(0, 1).lerp(centre(1, 1), f.x);
  top.lerp(bottom, f.y)
}

/// The static guard's camera over the 1024 x 1024 map on a grid: the loop
/// `flown` gives over the orthogonal map, at `FLY_SPEED`; on another grid
/// the same loop in cells taken onto its cells (`grid_point`), so it stays
/// over the map (within an isometric map's diamond), flown at the rate that
/// keeps its average speed over the guard's frames the orthogonal camera's
struct Flight {
  grid: Option<MapGrid>,
  rate: f32,
}

impl Flight {
  fn over(grid: Grid) -> Flight {
    let orthogonal = Flight {
      grid: None,
      rate: 1.0,
    };
    if grid == Grid::Orthogonal {
      return orthogonal;
    }
    let (_, target, _) = orthogonal.speeds();
    let mut flight = Flight {
      grid: Some(grid.map_grid(BIG)),
      rate: 1.0,
    };
    for _ in 0..4 {
      let (_, mean, _) = flight.speeds();
      flight.rate *= target / mean;
    }
    flight
  }

  /// Where the camera is at frame `frame`
  fn at(&self, frame: usize) -> Vec2 {
    let side = (BIG * TILE) as f32;
    let at = flown(
      frame as f32 * self.rate * FLY_SPEED / 60.0 / (side * 0.5),
      BIG,
    );
    match &self.grid {
      None => at,
      Some(grid) => grid_point(grid, at / TILE as f32),
    }
  }

  /// The camera's slowest, average and fastest speeds over the guard's
  /// frames, in px/s
  fn speeds(&self) -> (f32, f32, f32) {
    let speeds: Vec<f32> = (WARM..WARM + ROUNDS)
      .map(|frame| self.at(frame + 1).distance(self.at(frame)) * 60.0)
      .collect();
    let mean = speeds.iter().sum::<f32>() / speeds.len() as f32;
    let (slowest, fastest) = speeds.iter().fold((f32::INFINITY, 0.0f32), |(lo, hi), &s| {
      (lo.min(s), hi.max(s))
    });
    (slowest, mean, fastest)
  }
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
  /// The map on a grid's cells, colliding and casting shadows: each grid's
  /// guarded one
  Map(Grid),
  /// The orthogonal map with unbounded layers, also guarded
  MapUnbounded,
  /// The isometric map with its props 32 x 48 tiles of a tileset of their
  /// own, which leave their cells' tops, so the layer draws in Tiled's
  /// order: also guarded
  Tall,
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

  /// Whether its frame is held to the target
  fn held(self) -> bool {
    matches!(self, Show::Map(_) | Show::MapUnbounded | Show::Tall)
  }

  /// The grid its map is on
  fn grid(self) -> Grid {
    match self {
      Show::Map(grid) => grid,
      Show::Tall => Grid::Isometric,
      _ => Grid::Orthogonal,
    }
  }

  fn name(self) -> String {
    match self {
      Show::Empty => "an empty frame".into(),
      Show::Map(grid) => format!("the {} map, colliding and casting shadows", grid.name()),
      Show::MapUnbounded => {
        "the orthogonal map with unbounded layers, colliding and casting shadows".into()
      }
      Show::Tall => format!(
        "the {} map with tall props (32 x 48 tiles on its plain props layer, drawn in Tiled's \
         order), colliding and casting shadows",
        Grid::Isometric.name()
      ),
      Show::Drawn => "the orthogonal map drawn only (no collision, no shadows)".into(),
      Show::YSorted => "the orthogonal map with its props layer y-sorted".into(),
      Show::EmptyLit => "an empty lit frame".into(),
      Show::MapLit => "the orthogonal map lit (one shadowed light)".into(),
      Show::NoShadows => "the orthogonal map colliding, without shadows".into(),
      Show::NoCollision => "the orthogonal map casting shadows, without collision".into(),
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
    let grid = self.show.grid();
    let mut sets = vec![add_tileset(ctx, grid)];
    if self.show == Show::Tall {
      sets.push(add_atlas(ctx, TALL, None, None));
    }
    let unbounded = self.show == Show::MapUnbounded;
    let mut map = map_of(BIG, grid, &sets, &cave_cells(BIG), unbounded);
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

/// An engine of the static guard, its camera flying `flight`, landed and
/// warmed up; its first build if it has a map. None without a GPU
fn static_engine(show: Show, flight: &Flight) -> Option<(HeadlessEngine, Option<FirstBuild>)> {
  let builder = EngineBuilder::new()
    .resolution(RESOLUTION.0, RESOLUTION.1)
    .start_scene(BigScene { show });
  let mut engine = HeadlessEngine::new_or_skip(builder)?;
  place_camera(&mut engine, flight.at(0));
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
    place_camera(&mut engine, flight.at(frame));
    engine.step().expect("a warm-up frame");
  }
  if show == Show::Tall {
    assert_drawn_in_order(&mut engine, "props");
  }
  if show == Show::Map(Grid::Hexagonal) {
    let rock = (0..BIG as i32)
      .flat_map(|y| (0..BIG as i32).map(move |x| IVec2::new(x, y)))
      .find(|cell| rocky(cell.x, cell.y))
      .expect("a rock cell");
    assert_whole_hexagon(&mut engine, rock, "static_frame");
  }
  Some((engine, built))
}

/// That the hexagonal map's rock at `cell` collides as its whole cell
/// (`TileCell::Solid` in the map's tile colliders), so its tiles' hexagon is
/// their cell's: a polygon off the cell's corners collides as a shape of its
/// own, a ring in its cell's part
fn assert_whole_hexagon(engine: &mut HeadlessEngine, cell: IVec2, guard: &str) {
  let world = engine.world_mut();
  let solid = world
    .query::<&TileColliders>()
    .iter(world)
    .any(|tiles| tiles.get(cell) == TileCell::Solid);
  assert!(
    solid,
    "{guard}: the hexagonal map's rock at cell {cell} collides as its whole hexagon"
  );
}

/// That the hexagonal edit map's chunk heavy with rings is what its cases
/// edit: every cell a shape of its own (`TileCell::Shape` in the map's tile
/// colliders), and its rings in parts of 4 x 4 cells, 96 edges each, as
/// its 8 x 8 blocks' rings (384 each) pass the budget: 64 colliders sit in
/// the chunk's box (from its first cell's to the next chunk's), and every
/// one holds 96 edges
fn assert_ring_heavy(engine: &mut HeadlessEngine) {
  let first = RING_CHUNK * CHUNK;
  let world = engine.world_mut();
  let shapes = world.query::<&TileColliders>().iter(world).any(|tiles| {
    (0..CHUNK * CHUNK).all(|i| {
      let cell = first + IVec2::new(i % CHUNK, i / CHUNK);
      matches!(tiles.get(cell), TileCell::Shape(_))
    })
  });
  assert!(
    shapes,
    "edit: every cell of the hexagonal map's chunk {RING_CHUNK} heavy with rings collides as a \
     shape of its own"
  );
  let grid = Grid::Hexagonal.map_grid(BIG);
  let half = Grid::Hexagonal.cell().as_vec2() / 2.0;
  let lo = grid.cell_center(first) - half;
  let hi = grid.cell_center(first + IVec2::splat(CHUNK)) - half;
  let edges: Vec<usize> = world
    .query::<(&GlobalTransform, &Collider)>()
    .iter(world)
    .filter(|(at, _)| {
      let at = at.translation();
      at.cmpge(lo).all() && at.cmplt(hi).all()
    })
    .map(|(_, collider)| match &collider.shape {
      ColliderShape::Segments { segments, .. } => segments.len(),
      _ => 0,
    })
    .collect();
  assert!(
    edges.len() == 64 && edges.iter().all(|&n| n == 96),
    "edit: the hexagonal map's chunk {RING_CHUNK} heavy with rings collides in 64 parts of rings \
     of 4 x 4 cells, 96 edges each: its parts' edges {edges:?}"
  );
}

/// The entity of `engine`'s tile layer named `name`
fn layer_named(engine: &mut HeadlessEngine, name: &str) -> Entity {
  let world = engine.world_mut();
  world
    .query::<(Entity, &LayerInfo)>()
    .iter(world)
    .find(|(_, info)| info.name == name)
    .map(|(entity, _)| entity)
    .unwrap_or_else(|| panic!("a {name} layer"))
}

/// That the layer named `name` draws through the in-order path: every chunk
/// of it the last frame drew was built knowing its tiles leave their cells
/// (`HeadlessEngine::spilling_tile_chunks`), which only a layer drawn in
/// Tiled's order builds; so a change to which tilesets may leave their
/// cells cannot turn the tall props into a plain layer unseen
fn assert_drawn_in_order(engine: &mut HeadlessEngine, name: &str) {
  let layer = layer_named(engine, name);
  let spilling = engine.spilling_tile_chunks();
  let drawn: BTreeSet<(i32, i32)> = engine
    .tile_draws()
    .into_iter()
    .filter(|&(of, ..)| of == layer)
    .map(|(_, chunk, ..)| (chunk.x, chunk.y))
    .collect();
  let plain: Vec<_> = drawn
    .iter()
    .filter(|&&(x, y)| !spilling.contains(&(layer, IVec2::new(x, y))))
    .collect();
  assert!(
    !drawn.is_empty() && plain.is_empty(),
    "static_frame: the {name} layer of tall art draws in Tiled's order: of the {} chunks it drew, \
     {plain:?} were built as plain chunks",
    drawn.len(),
  );
}

/// One engine of the static guard and its frames: each one's step, render
/// step and tile draws
struct Probe {
  show: Show,
  flight: Flight,
  engine: HeadlessEngine,
  built: Option<FirstBuild>,
  steps: Vec<Duration>,
  renders: Vec<Duration>,
  draws: Vec<usize>,
}

/// A round of the static guard's engines, each against an empty frame of
/// its own and as many as the other (an engine stepping evicts the others'
/// data, so every held map steps in the same company)
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Round {
  /// The orthogonal map and its variants, as the guard has always stepped
  /// them
  Orthogonal,
  /// The other grids' maps, beside the orthogonal map again and two of its
  /// variants (fillers, held in the first round or with no target)
  Grids,
}

impl Round {
  fn name(self) -> &'static str {
    match self {
      Round::Orthogonal => "the orthogonal round",
      Round::Grids => "the grids' round",
    }
  }

  /// Its engines' shows, the first an empty frame
  fn shows(self) -> [Show; 9] {
    match self {
      Round::Orthogonal => [
        Show::Empty,
        Show::Map(Grid::Orthogonal),
        Show::MapUnbounded,
        Show::Drawn,
        Show::YSorted,
        Show::EmptyLit,
        Show::MapLit,
        Show::NoShadows,
        Show::NoCollision,
      ],
      Round::Grids => [
        Show::Empty,
        Show::Map(Grid::Orthogonal),
        Show::MapUnbounded,
        Show::Drawn,
        Show::Map(Grid::Isometric),
        Show::Tall,
        Show::Map(Grid::Staggered),
        Show::Map(Grid::Hexagonal),
        Show::Map(Grid::Oblique),
      ],
    }
  }

  /// Whether the round is the first to step `show`: every show in the
  /// orthogonal round, and the other grids' in theirs
  fn first_steps(self, show: Show) -> bool {
    self == Round::Orthogonal || show.grid() != Grid::Orthogonal
  }

  /// Whether the round holds `show`'s frame to the target: a held map, in
  /// the round that first steps it
  fn holds(self, show: Show) -> bool {
    show.held() && self.first_steps(show)
  }
}

/// The static frame guard: the orthogonal round, then the grids' round
fn static_guard() {
  static_round(Round::Orthogonal);
  static_round(Round::Grids);
}

/// One round of the static guard: its engines stepped in turn, each map
/// it holds held to the target
fn static_round(round: Round) {
  let name = round.name();
  let mut probes = Vec::new();
  for show in round.shows() {
    let flight = Flight::over(show.grid());
    let Some((engine, built)) = static_engine(show, &flight) else {
      return;
    };
    probes.push(Probe {
      show,
      flight,
      engine,
      built,
      steps: Vec::with_capacity(ROUNDS),
      renders: Vec::with_capacity(ROUNDS),
      draws: Vec::with_capacity(ROUNDS),
    });
  }
  // Round robin, as `benches/ui_render.rs`: each round starts at the next
  // engine, every other round in reverse, so no engine steps twice in a row
  let count = probes.len();
  for round in 0..ROUNDS {
    for i in 0..count {
      let index = if round.is_multiple_of(2) {
        (round + i) % count
      } else {
        (round + 1 + count - i) % count
      };
      let probe = &mut probes[index];
      place_camera(&mut probe.engine, probe.flight.at(WARM + round));
      settle_gpu();
      let start = Instant::now();
      probe.engine.step().expect("a guard frame");
      probe.steps.push(start.elapsed());
      probe.renders.push(probe.engine.render_time());
      if probe.show.has_map() {
        probe.draws.push(probe.engine.tile_draws().len());
      }
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
  let empty_lit = probes.iter().find(|probe| probe.show == Show::EmptyLit);
  let empty_lit_step = empty_lit.map_or(Duration::ZERO, |probe| median(probe.steps.clone()));
  let lit = empty_lit.map_or(String::new(), |_| {
    format!("; an empty lit frame {:.1}", us(empty_lit_step))
  });
  eprintln!(
    "static_frame guard, {name}: {ROUNDS} interleaved frames at {}x{}, each map's camera flying \
     at about 560 px/s on average. An empty frame: step median {:.1} µs (render step {:.1}){lit}",
    RESOLUTION.0,
    RESOLUTION.1,
    us(empty_step),
    us(empty_render),
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
    let mut draws = probe.draws.clone();
    draws.sort_unstable();
    let (slowest, mean, fastest) = probe.flight.speeds();
    let line = format!(
      "{} (its camera at {slowest:.0} to {fastest:.0} px/s, {mean:.0} on average): the step over \
       the empty frame's median, {}; its render step {:+.1} µs over the empty frame's; tile \
       draws a frame {} (median; {} to {})",
      probe.show.name(),
      shown(over(&probe.steps, base)),
      us(render) - us(empty_render),
      draws.get(draws.len() / 2).copied().unwrap_or(0),
      draws.first().copied().unwrap_or(0),
      draws.last().copied().unwrap_or(0),
    );
    if probe.show.held() && !round.holds(probe.show) {
      eprintln!("static_frame (no target: the orthogonal round holds it): {line}");
    } else if round.holds(probe.show) {
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
  // Each map's entities and first build, once
  for probe in probes
    .iter_mut()
    .filter(|probe| probe.show.has_map() && round.first_steps(probe.show))
  {
    let name = probe.show.name();
    let world = probe.engine.world_mut();
    let occluders = world.query::<&MapOccluder>().iter(world).count();
    let children = world.query::<&ChildOf>().iter(world).count();
    eprintln!(
      "static_frame (no target): {name}: {} entities, {children} of them children, {occluders} \
       occluder chains",
      world.entities().len(),
    );
  }
  for probe in probes.iter().filter(|probe| round.holds(probe.show)) {
    let Some(built) = &probe.built else { continue };
    eprintln!(
      "static_frame (no target): {}: its first build, stepped back to back: spawn to landed \
       {:.1} ms (the landing job on its worker, and the frames meanwhile), the landing frame \
       {:.1} ms, the frame after {:.1} ms",
      probe.show.name(),
      ms(built.job),
      ms(built.frame),
      ms(built.after),
    );
  }
}

/// The static frame bench: the map engine stepped back to back, its camera
/// flying (the orthogonal map's, or `benched`'s)
fn static_bench(c: &mut Criterion) {
  let mut made: Option<(HeadlessEngine, usize)> = None;
  let show = benched();
  let flight = Flight::over(show.grid());
  c.bench_function("static_frame", |b| {
    if made.is_none() {
      made = static_engine(show, &flight).map(|(engine, _)| (engine, WARM));
    }
    let Some((engine, frame)) = made.as_mut() else {
      return;
    };
    b.iter(|| {
      *frame += 1;
      place_camera(engine, flight.at(*frame));
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
    let (side, grid) = (COLUMNS * TILE, Grid::Orthogonal);
    let image = world.resource_mut::<Assets<Image>>().add(Image::from_rgba(
      atlas_pixels(grid.cell()),
      side,
      side,
    ));
    let set = tileset(image, grid.cell(), Some(grid.rock()), None);
    TilesetSource::Asset(world.resource_mut::<Assets<Tileset>>().add(set))
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

/// The edit map's loops: each in layout chunks (0..4, row) of the ground
/// layer (`Layout`)
const SKYLINE_ROW: i32 = 0;
const SERPENTINE_ROW: i32 = 2;

/// The skyline's column that holds only its bottom cell: the neck between
/// its two parts, which clearing that cell splits
const NECK: usize = 99;

/// Cells a side of a chunk
const CHUNK: i32 = CHUNK_SIZE as i32;

/// The hexagonal map's own cases, in its cells: the islands are kept out
/// of its first 5 x 5 chunks (a row of chunks more than on the other grids)
const HEX_KEPT_OUT: i32 = 5 * CHUNK;
/// A lone cell where four of the islands' chunks meet: its sides lie in
/// the four chunks' parts, each a quarter of four islands
const HEX_CORNER: IVec2 = IVec2::new(8 * CHUNK, 8 * CHUNK);
/// A lone cell where four empty chunks meet (chunks 0 and 1 of rows 3 and
/// 4, under the serpentine): each of its sides' owners is in a chunk of
/// its own that holds nothing else, so setting it makes four parts and
/// clearing it drops them
const HEX_CHURN: IVec2 = IVec2::new(CHUNK, 4 * CHUNK);
/// The chunk held at 127 edges (`budget_cells`), in the keep-out area under
/// the serpentine, and the cell in it whose edit takes it past the budget
/// (128 edges): in its first quarter's last block, which holds nothing till
/// then
const BUDGET_CHUNK: IVec2 = IVec2::new(3, 3);
const BUDGET_CELL: IVec2 = IVec2::new(13, 13);
/// The dense chunks (10..12 both ways), four cells in ten rock, and the
/// lone cell where the four meet
const DENSE: std::ops::Range<i32> = 10..12;
const DENSE_CORNER: IVec2 = IVec2::new(11 * CHUNK, 11 * CHUNK);
/// The chunk heavy with rings, every cell the ring tile, in the keep-out
/// area (no cell of it beside another chunk's rock), and its edited cells:
/// one in a block's middle, and one where four blocks meet (as
/// `hex_ring_heavy_edit_timing`'s)
const RING_CHUNK: IVec2 = IVec2::new(4, 4);
const RING_MIDDLE: IVec2 = IVec2::new(4, 4);
const RING_CORNER: IVec2 = IVec2::new(8, 8);

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

/// The hexagonal map's chunk at 127 edges, one part, and the cells round it
/// that give each of its other three quarters one edge. Its first quarter
/// holds 19 lone hexagons (six edges each, every side owned in it) in its
/// two upper blocks and a pair (ten) in its lower left block: 124 edges. A
/// hexagon right of the chunk on an odd row gives the cell left of it, on
/// the chunk's last column, one edge (its left side, which that cell owns),
/// once in the upper right quarter and once in the lower right; and one
/// left of the chunk on an odd row gives the chunk's first cell of the row
/// above, in its lower left quarter, one (that hexagon's upper right side).
/// So `BUDGET_CELL`, a lone hexagon more, takes the chunk to 133 edges and
/// its first quarter to 130, both past the budget: the chunk's part goes,
/// and seven come, its other quarters' and its first quarter's four blocks
fn budget_cells() -> Vec<IVec2> {
  let first = BUDGET_CHUNK * CHUNK;
  (0..21)
    .map(|i| IVec2::new(1 + 2 * (i % 7), 1 + 2 * (i / 7)))
    .filter(|&at| at != IVec2::new(11, 5) && at != IVec2::new(13, 5))
    .chain([IVec2::new(1, 15), IVec2::new(2, 15)])
    .chain([
      IVec2::new(CHUNK, 1),
      IVec2::new(CHUNK, 17),
      IVec2::new(-1, 17),
    ])
    .map(|at| first + at)
    .collect()
}

/// Where the edit map's shapes lie on a grid's cells. Off the hexagonal
/// grid, in its square lattice (`MapGrid::lattice`), the cells its
/// collision traces in, from `origin`: the origin on orthogonal, isometric
/// and oblique grids, whose cells are their lattice's; on the staggered
/// grid a lattice chunk's corner whose shapes' lattice cells all lie in the
/// map (its 1024 x 1024 cells make a diamond of lattice cells, not a
/// rectangle from the origin). The hexagonal grid has no square lattice:
/// its shapes lie in its own cells from (0, 0), a row of hexagons for a row
/// of cells
struct Layout {
  lattice: Option<Lattice>,
  origin: IVec2,
}

impl Layout {
  fn of(grid: Grid) -> Layout {
    let origin = match grid {
      Grid::Staggered => IVec2::new(320, -224),
      _ => IVec2::ZERO,
    };
    let lattice = (grid != Grid::Hexagonal).then(|| {
      grid
        .map_grid(BIG)
        .lattice()
        .expect("every grid but the hexagonal has a square lattice")
    });
    Layout { lattice, origin }
  }

  /// The grid cell at layout cell `at`
  fn cell(&self, at: IVec2) -> IVec2 {
    let at = self.origin + at;
    self.lattice.map_or(at, |lattice| lattice.grid_cell(at))
  }

  /// The layout cell at grid cell `cell`
  fn at(&self, cell: IVec2) -> IVec2 {
    self
      .lattice
      .map_or(cell, |lattice| lattice.lattice_cell(cell))
      - self.origin
  }
}

/// What a case's edits are called, and which of them each name takes: all
/// of them, or every other from the first or the second
type Kinds = &'static [(&'static str, Option<usize>)];
/// A tile set on top of a loop and cleared again: it stays one loop
const BUMP: Kinds = &[("a bump", None)];
/// A cell of a loop cleared and set again: the loop splits in two, and the
/// two join again
const CUT: Kinds = &[("a split", Some(0)), ("a join", Some(1))];
/// A lone cell set and cleared
const LONE: Kinds = &[("set and cleared", None)];
/// A lone cell set (parts made) and cleared (parts dropped)
const CHURN: Kinds = &[("parts made", Some(0)), ("parts dropped", Some(1))];
/// A cell taking its chunk past the budget (its part split), and cleared
const PAST: Kinds = &[("past the budget", Some(0)), ("set back", Some(1))];
/// A ring cell cleared and set back
const RING_CUT: Kinds = &[("cleared and set back", None)];
/// A ring cell turned whole (rock) and back
const RING_WHOLE: Kinds = &[("turned whole and back", None)];

/// What a case's edits put in its cell, in turn: a tile id, or nothing
type Turns = [Option<u32>; 2];
/// Rock set and cleared
const SET: Turns = [Some(ROCK.start), None];
/// Rock cleared and set again: the cell starts as rock
const CLEARED: Turns = [None, Some(ROCK.start)];

/// One case's edits: its cell's tile changed in turn (`turns`)
struct EditCase {
  name: &'static str,
  cell: IVec2,
  /// What its edits put in its cell, in turn, the second what it starts
  /// with
  turns: Turns,
  kinds: Kinds,
  /// Cells emptied and filled with rock again before each pair of its
  /// edits, so each pair starts alike (the chunk taken past the budget,
  /// which never merges back into one part until it holds nothing)
  reset: Vec<IVec2>,
  /// Its medians are held to the target
  held: bool,
  /// It has a camera of its own, over its cell; else the camera stands
  /// over the loops
  apart: bool,
  /// The colliders its first edit and its second leave, each counted from
  /// the colliders before its first edit, as it is laid out to make and
  /// drop parts (checked)
  laid_out: Option<[usize; 2]>,
}

impl EditCase {
  /// A loop's edits at `cell`
  fn of_loop(name: &'static str, cell: IVec2, cut: bool) -> EditCase {
    EditCase {
      name,
      cell,
      turns: if cut { CLEARED } else { SET },
      kinds: if cut { CUT } else { BUMP },
      reset: Vec::new(),
      held: true,
      apart: false,
      laid_out: None,
    }
  }

  /// A case of the hexagonal map's own at `cell`, under its own camera
  fn apart(name: &'static str, cell: IVec2, kinds: Kinds) -> EditCase {
    EditCase {
      name,
      cell,
      turns: SET,
      kinds,
      reset: Vec::new(),
      held: true,
      apart: true,
      laid_out: None,
    }
  }
}

/// The edits the guard makes on `grid`'s edit map: each loop's bump, and
/// each loop's cut (the skyline's neck, the middle of the serpentine's first
/// row, and the middle of its middle row, which splits it into halves of
/// about 2,000 edges, the smaller given the other loop's id edge by edge),
/// at the grid's cells of those layout cells; on the hexagonal map its own
/// cases too, all held but the dense corner
fn edit_cases(grid: Grid) -> Vec<EditCase> {
  let tops = skyline_tops();
  let layout = Layout::of(grid);
  let at = |x: i32, y: i32| layout.cell(IVec2::new(x, y));
  let skyline = |x: usize, y: i32| at(x as i32, SKYLINE_ROW * 32 + y);
  let serpentine = |x: i32, y: i32| at(x, SERPENTINE_ROW * 32 + y);
  let mut cases = vec![
    EditCase::of_loop("the skyline", skyline(64, tops[64] - 1), false),
    EditCase::of_loop("the skyline", skyline(NECK, 31), true),
    EditCase::of_loop("the serpentine", serpentine(64, 0), false),
    EditCase::of_loop("the serpentine", serpentine(64, 1), true),
    EditCase::of_loop(
      "the serpentine cut through its middle row",
      serpentine(64, 15),
      true,
    ),
  ];
  if grid == Grid::Hexagonal {
    cases.extend([
      EditCase::apart(
        "a lone cell where four of the islands' chunks meet",
        HEX_CORNER,
        LONE,
      ),
      EditCase {
        laid_out: Some([4, 0]),
        ..EditCase::apart(
          "part churn, a lone cell where four empty chunks meet",
          HEX_CHURN,
          CHURN,
        )
      },
      EditCase {
        reset: budget_cells(),
        laid_out: Some([6, 5]),
        ..EditCase::apart(
          "a chunk of 127 edges in one part",
          BUDGET_CHUNK * CHUNK + BUDGET_CELL,
          PAST,
        )
      },
      EditCase {
        held: false,
        ..EditCase::apart(
          "a lone cell where four dense chunks meet",
          DENSE_CORNER,
          LONE,
        )
      },
      // Its part of rings rebuilt, no part made or dropped
      EditCase {
        turns: [None, Some(RING)],
        laid_out: Some([0, 0]),
        ..EditCase::apart(
          "a ring cell in a block's middle, in a chunk heavy with rings",
          RING_CHUNK * CHUNK + RING_MIDDLE,
          RING_CUT,
        )
      },
      // Its part of rings rebuilt, and the chunk's part of sides, its six
      // sides, made and dropped
      EditCase {
        turns: [Some(ROCK.start), Some(RING)],
        laid_out: Some([1, 0]),
        ..EditCase::apart(
          "a ring cell where four blocks meet, in a chunk heavy with rings",
          RING_CHUNK * CHUNK + RING_CORNER,
          RING_WHOLE,
        )
      },
    ]);
  }
  cases
}

/// What the edit map's ground holds, by layout cell: the skyline's column
/// tops, and on the hexagonal map its own cases' cells
struct Ground {
  hexagonal: bool,
  tops: Vec<i32>,
  budget: Vec<IVec2>,
}

impl Ground {
  fn of(grid: Grid) -> Ground {
    let hexagonal = grid == Grid::Hexagonal;
    Ground {
      hexagonal,
      tops: skyline_tops(),
      budget: if hexagonal {
        budget_cells()
      } else {
        Vec::new()
      },
    }
  }

  /// Whether the ground holds rock at layout cell `at`: 3 x 3 islands every
  /// 8 cells, kept out of layout chunks 0..5 by 0..4 (0..5 by 0..5 on the
  /// hexagonal map), and in the 4 x 1 chunks of its row each the skyline
  /// (columns of `tops`) and the serpentine (every other row from row 1,
  /// joined at alternate ends); on the hexagonal map the chunk at the
  /// budget and the cells beside it (`budget_cells`), and the dense chunks
  /// (four cells in ten, none at their corner) in place of their islands
  fn rock(&self, at: IVec2) -> bool {
    let column = 0..128;
    if column.contains(&at.x) {
      let y = at.y - SKYLINE_ROW * 32;
      if (0..32).contains(&y) {
        return y >= self.tops[at.x as usize];
      }
      let y = at.y - SERPENTINE_ROW * 32;
      if (0..32).contains(&y) {
        if y % 2 == 1 {
          return true;
        }
        let end = if (y - 1) % 4 == 1 { 127 } else { 0 };
        return y > 0 && at.x == end;
      }
    }
    let mut rows = 0..128;
    if self.hexagonal {
      if self.budget.contains(&at) {
        return true;
      }
      let chunk = at.div_euclid(IVec2::splat(CHUNK));
      if DENSE.contains(&chunk.x) && DENSE.contains(&chunk.y) {
        return at != DENSE_CORNER && hash(at.x, at.y, 6) % 10 < 4;
      }
      rows = 0..HEX_KEPT_OUT;
    }
    let (x, y) = (at.x.rem_euclid(8), at.y.rem_euclid(8));
    let (bx, by) = (at.x - x, at.y - y);
    let kept_out = (0..160).contains(&bx) && rows.contains(&by);
    (2..5).contains(&x) && (2..5).contains(&y) && !kept_out
  }

  /// The ground's tile at layout cell `at`: on the hexagonal map the ring
  /// tile throughout the chunk heavy with rings (`RING_CHUNK`, in the
  /// keep-out area), else rock where `rock` takes it
  fn tile(&self, at: IVec2) -> Option<u32> {
    if self.hexagonal && at.div_euclid(IVec2::splat(CHUNK)) == RING_CHUNK {
      return Some(RING);
    }
    self.rock(at).then(|| pick(ROCK, hash(at.x, at.y, 5)))
  }
}

/// The edit map on `grid`'s cells: background on every cell; on the ground
/// layer, at the layout cells `Ground::rock` takes, about 16,000 islands
/// and the skyline and the serpentine, each loop in four layout chunks, and
/// on the hexagonal map the chunk heavy with rings (`Ground::tile`); props
/// and details as the cave map's where the ground is open
fn edit_map(grid: Grid, set: TilesetSource) -> TileMap {
  let mut cells = cave_cells(BIG);
  let (layout, ground) = (Layout::of(grid), Ground::of(grid));
  let side = BIG as i32;
  for y in 0..side {
    for x in 0..side {
      let i = (y * side + x) as usize;
      let at = layout.at(IVec2::new(x, y));
      cells[1][i] = ground.tile(at);
      // Props and details only where the ground is open
      if cells[1][i].is_some() {
        cells[2][i] = None;
        cells[3][i] = None;
      }
    }
  }
  map_of(BIG, grid, &[set], &cells, false)
}

/// The edit guard's scene: the edit map on a grid
struct EditScene {
  grid: Grid,
}

impl Scene for EditScene {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    let set = add_edit_tileset(ctx, self.grid);
    let map = edit_map(self.grid, set);
    let root = MapRoot::new(ctx.assets().add(map));
    ctx.world.spawn((Transform::IDENTITY, root));
  }
}

/// Where the camera stands for `case` on `grid`'s edit map (`cases` its
/// cases): over its own cell, if it is apart; else over the loops, on the
/// orthogonal grid at (1024, 540) and off it at the middle of the loops'
/// edited cells
fn case_view(grid: Grid, cases: &[EditCase], case: &EditCase) -> Vec2 {
  let map_grid = grid.map_grid(BIG);
  if case.apart {
    return map_grid.cell_center(case.cell);
  }
  if grid == Grid::Orthogonal {
    return Vec2::new(1024.0, 540.0);
  }
  let centres = cases
    .iter()
    .filter(|case| !case.apart)
    .map(|case| map_grid.cell_center(case.cell));
  let (lo, hi) = centres.fold((Vec2::INFINITY, Vec2::NEG_INFINITY), |(lo, hi), c| {
    (lo.min(c), hi.max(c))
  });
  (lo + hi) / 2.0
}

/// That the last frame of `engine` (its camera at `at`) drew the chunk that
/// holds `case`'s cell: the background layer's, which has a tile on every
/// cell (a chunk of the ground can be empty until the edit)
fn assert_case_drawn(engine: &mut HeadlessEngine, grid: Grid, case: &EditCase, at: Vec2) {
  let background = layer_named(engine, "background");
  let chunk = TileGrid::chunk_of(case.cell);
  let drawn = engine
    .tile_draws()
    .into_iter()
    .any(|(layer, at, ..)| layer == background && at == chunk);
  assert!(
    drawn,
    "edit, the {} map: the last warm-up frame, its camera at {at}, drew chunk {chunk} of {}'s \
     cell {}",
    grid.name(),
    case.name,
    case.cell,
  );
}

/// An engine on `grid`'s edit map, landed and warmed up, its camera over
/// both loops (each loop edit's chunk drawn); its map's root. None without
/// a GPU
fn edit_engine(grid: Grid) -> Option<(HeadlessEngine, Entity)> {
  let builder = EngineBuilder::new()
    .resolution(RESOLUTION.0, RESOLUTION.1)
    .start_scene(EditScene { grid });
  let mut engine = HeadlessEngine::new_or_skip(builder)?;
  let cases = edit_cases(grid);
  let at = case_view(grid, &cases, &cases[0]);
  place_camera(&mut engine, at);
  let start = Instant::now();
  while !landed(&mut engine) {
    engine.step().expect("a frame");
    assert!(start.elapsed() < Duration::from_secs(60), "the map landed");
  }
  engine.step_frames(WARM as u32).expect("the warm-up frames");
  let root = roots(&mut engine)[0];
  for case in cases.iter().filter(|case| !case.apart) {
    assert_case_drawn(&mut engine, grid, case, at);
  }
  if grid == Grid::Hexagonal {
    let neck = cases[1].cell;
    assert_whole_hexagon(&mut engine, neck, "edit");
    assert_ring_heavy(&mut engine);
  }
  Some((engine, root))
}

/// The tile edit `i` puts at `cell`: `turns`' first, then its second, in
/// turn
fn toggle(engine: &mut HeadlessEngine, root: Entity, cell: IVec2, i: usize, turns: Turns) {
  let tile = turns[i % 2].map(|id| Tile::new(0, id));
  let changed = MapsView::new(engine.world_mut()).set_tile(root, "ground", cell, tile);
  assert!(changed, "the edit changes its cell");
}

/// `cells` emptied and filled with rock again, each change taken in by
/// `settle` (a frame, or the edit's systems run)
fn refill(
  engine: &mut HeadlessEngine,
  root: Entity,
  cells: &[IVec2],
  settle: &mut dyn FnMut(&mut HeadlessEngine),
) {
  for tile in [None, Some(Tile::new(0, ROCK.start))] {
    let mut maps = MapsView::new(engine.world_mut());
    for &cell in cells {
      maps.set_tile(root, "ground", cell, tile);
    }
    settle(engine);
  }
}

/// How many colliders `engine`'s world holds: on the edit map, every one a
/// part of its tile colliders
fn colliders(engine: &mut HeadlessEngine) -> usize {
  let world = engine.world_mut();
  world
    .query_filtered::<(), With<Collider>>()
    .iter(world)
    .count()
}

/// One case's edits: each part's times, and the ordinary frames'
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

/// The colliders before `case`'s first two edits, after the first and
/// after the second, each taken in by the edit's systems (`schedules`, set
/// up; no frame)
fn parts_through(
  engine: &mut HeadlessEngine,
  root: Entity,
  case: &EditCase,
  schedules: &mut [Schedule; 3],
) -> [usize; 3] {
  let mut settle = |engine: &mut HeadlessEngine| {
    for schedule in schedules.iter_mut() {
      schedule.run(engine.world_mut());
    }
  };
  refill(engine, root, &case.reset, &mut settle);
  let before = colliders(engine);
  toggle(engine, root, case.cell, 0, case.turns);
  settle(engine);
  let first = colliders(engine);
  toggle(engine, root, case.cell, 1, case.turns);
  settle(engine);
  [before, first, colliders(engine)]
}

/// `case`'s edits, each part timed by the edit's systems (`schedules`, set
/// up), a frame before each edit and after it; then as many back to back
fn edits(
  engine: &mut HeadlessEngine,
  root: Entity,
  case: &EditCase,
  schedules: &mut [Schedule; 3],
) -> Edits {
  let [collision, rebuild, occluders] = schedules;
  let (cell, turns) = (case.cell, case.turns);
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
    if i.is_multiple_of(2) && !case.reset.is_empty() {
      refill(engine, root, &case.reset, &mut |engine| {
        engine.step().expect("a frame");
      });
    }
    let (render, whole) = step(engine);
    out.renders.push(render);
    out.steps.push(whole);
    toggle(engine, root, cell, i, turns);
    out.collision.push(timed(collision, engine));
    out.rebuild.push(timed(rebuild, engine));
    out.occluders.push(timed(occluders, engine));
    let (render, whole) = step(engine);
    out.edit_renders.push(render);
    out.edit_steps.push(whole);
  }
  // Back to back, no frame between: what the edits cost with their data
  // in cache
  for i in EDITS..2 * EDITS {
    if i.is_multiple_of(2) && !case.reset.is_empty() {
      refill(engine, root, &case.reset, &mut |engine| {
        for schedule in [&mut *collision, &mut *rebuild, &mut *occluders] {
          schedule.run(engine.world_mut());
        }
      });
    }
    toggle(engine, root, cell, i, turns);
    let collided = timed(collision, engine) + timed(rebuild, engine);
    out.warm_loops.push(collided);
    out.warm_occluders.push(timed(occluders, engine));
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

/// That `case` makes and drops the parts it is laid out to (`laid_out`):
/// the hexagonal map's churn makes four parts and drops them, its chunk at
/// the budget, one part, splits into seven, one of which its set back
/// drops, and its chunk heavy with rings rebuilds a part of rings with no
/// part made or dropped, but for the part of sides a cell turned whole
/// makes and turned back drops; `parts` the colliders before its first
/// edit, after it and after the second
fn assert_parts(case: &EditCase, parts: [usize; 3]) {
  let [before, first, second] = parts;
  let Some([made, kept]) = case.laid_out else {
    return;
  };
  assert!(
    first == before + made && second == before + kept,
    "edit, the hexagonal map, {}: the colliders before its first edit, after it and after the \
     second: {before}, {first}, {second} (laid out for {}, {})",
    case.name,
    before + made,
    before + kept,
  );
}

/// The edit guard: on every grid, each loop's bump, split and join, each's
/// chunk, loop and occluder rebuilds, each median held to 100 µs; on the
/// hexagonal map its own cases too
fn edit_guard() {
  for grid in Grid::ALL {
    let Some((mut engine, root)) = edit_engine(grid) else {
      return;
    };
    let map = format!("the {} map", grid.name());
    let cases = edit_cases(grid);
    let mut at = case_view(grid, &cases, &cases[0]);
    for case in &cases {
      let (name, cell) = (case.name, case.cell);
      let view = case_view(grid, &cases, case);
      if view != at {
        at = view;
        place_camera(&mut engine, at);
        engine.step_frames(WARM as u32).expect("the warm-up frames");
        assert_case_drawn(&mut engine, grid, case, at);
      }
      let mut schedules = ryo2d::core::tilemap::edit_schedules();
      // Their first runs set them up
      for schedule in schedules.iter_mut() {
        schedule.run(engine.world_mut());
      }
      if case.apart {
        let parts = parts_through(&mut engine, root, case, &mut schedules);
        assert_parts(case, parts);
        eprintln!(
          "edit guard, {map}, {name} (cell {cell}): its colliders before its first edit, after \
           it and after the second, {}, {}, {}",
          parts[0], parts[1], parts[2],
        );
        engine.step().expect("a frame");
      }
      let edits = edits(&mut engine, root, case, &mut schedules);
      let ordinary_render = median(edits.renders.clone());
      let ordinary_step = median(edits.steps.clone());
      let chunk = over(&edits.edit_renders, ordinary_render);
      let loops: Vec<Duration> = edits
        .collision
        .iter()
        .zip(&edits.rebuild)
        .map(|(a, b)| *a + *b)
        .collect();
      for &(kind, which) in case.kinds {
        eprintln!(
          "edit guard, {map}, {name}, {kind} (cell {cell}), {} edits, each beside an ordinary \
           frame: the collision sync {}; the tile collider rebuild and collider sync {}; the \
           edit frame's whole step over an ordinary frame's median ({:.1} µs) {} (no target)",
          picked(&edits.collision, which).len(),
          shown(picked(&edits.collision, which)),
          shown(picked(&edits.rebuild, which)),
          us(ordinary_step),
          shown(over(&picked(&edits.edit_steps, which), ordinary_step)),
        );
        eprintln!(
          "edit (no target), {map}, {name}, {kind}, as many edits back to back with no frame \
           between (their data in cache): the loop rebuild {}; the occluder rebuild {}",
          shown(picked(&edits.warm_loops, which)),
          shown(picked(&edits.warm_occluders, which)),
        );
        if grid == Grid::Hexagonal {
          let together: Vec<Duration> = loops
            .iter()
            .zip(&edits.occluders)
            .map(|(a, b)| *a + *b)
            .collect();
          eprintln!(
            "edit (no target), {map}, {name}, {kind}: the loop and occluder rebuilds together, \
             each edit's sum, {}",
            shown_at(picked(&together, which), 99),
          );
        }
        for (part, times) in [
          ("the chunk rebuild", &chunk),
          ("the loop rebuild", &loops),
          ("the occluder rebuild", &edits.occluders),
        ] {
          let times = picked(times, which);
          let line = format!("{map}, {name}, {kind}, {part}: {}", shown(times.clone()));
          if !case.held {
            eprintln!("edit (no target): {line}");
            continue;
          }
          let (met, verdict) = verdict(median(times), EDIT_TARGET);
          let line = format!("{line}; {verdict}");
          eprintln!("edit: {line}");
          if !met {
            guards::miss(format!("edit, {line}"));
          }
        }
      }
    }
  }
}

/// The edit bench: the serpentine's cell set or cleared, and the step
/// after (on the orthogonal map, or `benched`'s grid's)
fn edit_bench(c: &mut Criterion) {
  let mut made: Option<(HeadlessEngine, Entity, usize)> = None;
  let grid = benched().grid();
  let cell = edit_cases(grid)[2].cell;
  c.bench_function("edit", |b| {
    if made.is_none() {
      made = edit_engine(grid).map(|(engine, root)| (engine, root, 0));
    }
    let Some((engine, root, i)) = made.as_mut() else {
      return;
    };
    b.iter(|| {
      toggle(engine, *root, cell, *i, SET);
      *i += 1;
      engine.step().expect("a frame")
    });
  });
}

// load

/// The attributes a TMX map element of `grid` carries for its shape: its
/// orientation and that orientation's own, as Tiled writes them
fn orientation(grid: Grid) -> &'static str {
  match grid {
    Grid::Orthogonal => "orientation=\"orthogonal\"",
    Grid::Isometric => "orientation=\"isometric\"",
    Grid::Staggered => "orientation=\"staggered\" staggeraxis=\"y\" staggerindex=\"odd\"",
    Grid::Hexagonal => {
      "orientation=\"hexagonal\" hexsidelength=\"8\" staggeraxis=\"y\" staggerindex=\"odd\""
    }
    Grid::Oblique => "orientation=\"oblique\" skewx=\"4\" skewy=\"0\"",
  }
}

/// A TMX file of the 256 x 256 cave map on `grid`'s cells, every layer
/// base64 and zlib, over the tileset at `tileset` (relative to the map's
/// folder)
fn tmx(cells: &[Vec<Option<u32>>; 4], tileset: &str, grid: Grid) -> Vec<u8> {
  let (orientation, cell) = (orientation(grid), grid.cell());
  let mut out = String::new();
  out.push_str(&format!(
    "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<map version=\"1.10\" \
     tiledversion=\"1.12.0\" {orientation} renderorder=\"right-down\" width=\"{SMALL}\" \
     height=\"{SMALL}\" tilewidth=\"{}\" tileheight=\"{}\" infinite=\"0\" \
     nextlayerid=\"5\" nextobjectid=\"1\">\n <tileset firstgid=\"1\" source=\"{tileset}\"/>\n",
    cell.x, cell.y,
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
fn tmx_infinite(cells: &[Vec<Option<u32>>; 4], tileset: &str, grid: Grid) -> Vec<u8> {
  const CHUNK: u32 = 16;
  let (orientation, cell) = (orientation(grid), grid.cell());
  let mut out = String::new();
  out.push_str(&format!(
    "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<map version=\"1.10\" \
     tiledversion=\"1.12.0\" {orientation} renderorder=\"right-down\" width=\"0\" \
     height=\"0\" tilewidth=\"{}\" tileheight=\"{}\" infinite=\"1\" nextlayerid=\"5\" \
     nextobjectid=\"1\">\n <tileset firstgid=\"1\" source=\"{tileset}\"/>\n",
    cell.x, cell.y,
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

/// A TSX file of `grid`'s tileset over the image at `image` (relative to
/// the tileset's folder): tiles of its cell's size, the rock tiles with a
/// rectangle filling the tile, or on isometric and staggered grids a
/// polygon on its diamond, and on the hexagonal grid one on its hexagon
fn tsx(image: &str, grid: Grid) -> Vec<u8> {
  let tile = grid.cell();
  let (width, height) = (COLUMNS * tile.x, COLUMNS * tile.y);
  let mut out = format!(
    "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<tileset version=\"1.10\" \
     tiledversion=\"1.12.0\" name=\"terrain\" tilewidth=\"{}\" tileheight=\"{}\" \
     tilecount=\"{}\" columns=\"{COLUMNS}\">\n <image source=\"{image}\" width=\"{width}\" \
     height=\"{height}\"/>\n",
    tile.x,
    tile.y,
    COLUMNS * COLUMNS
  );
  let shape = match grid {
    Grid::Isometric | Grid::Staggered | Grid::Hexagonal => {
      let points: Vec<String> = grid
        .rock()
        .iter()
        .map(|p| format!("{},{}", p.x, p.y))
        .collect();
      format!(
        "<object id=\"1\" x=\"0\" y=\"0\">\n    <polygon points=\"{}\"/>\n   </object>",
        points.join(" ")
      )
    }
    _ => format!(
      "<object id=\"1\" x=\"0\" y=\"0\" width=\"{}\" height=\"{}\"/>",
      tile.x, tile.y
    ),
  };
  for id in ROCK {
    out.push_str(&format!(
      " <tile id=\"{id}\">\n  <objectgroup draworder=\"index\" id=\"2\">\n   {shape}\n  \
       </objectgroup>\n </tile>\n"
    ));
  }
  out.push_str("</tileset>\n");
  out.into_bytes()
}

/// `grid`'s atlas as a PNG
fn png(grid: Grid) -> Vec<u8> {
  let tile = grid.cell();
  let (width, height) = (COLUMNS * tile.x, COLUMNS * tile.y);
  let image = image::RgbaImage::from_raw(width, height, atlas_pixels(tile)).expect("the atlas");
  let mut bytes = std::io::Cursor::new(Vec::new());
  image
    .write_to(&mut bytes, image::ImageFormat::Png)
    .expect("a png");
  bytes.into_inner()
}

/// What the load guard's files of `grid` start with: nothing for the
/// orthogonal grid's, the grid's name for the others'
fn files_of(grid: Grid) -> String {
  match grid {
    Grid::Orthogonal => String::new(),
    _ => format!("{}-", grid.slug()),
  }
}

/// The load guard's files: for each grid, `maps/{grid}cold-{i}.tmx` over
/// `tilesets/{grid}cold-{i}.tsx` over `images/{grid}cold-{i}.png` (fresh
/// files for each load), and the same of `inf-cold`, its infinite file
/// (`files_of` the grid in place of `{grid}`); and `maps/warm-{i}.tmx` and
/// `maps/land-{run}-{i}.tmx`, orthogonal, over `tilesets/terrain.tsx` over
/// `images/terrain.png`
fn load_files() -> MemorySource {
  let files = MemorySource::new();
  let cells = cave_cells(SMALL);
  let ortho = Grid::Orthogonal;
  let terrain = tmx(&cells, "../tilesets/terrain.tsx", ortho);
  files.insert("tilesets/terrain.tsx", tsx("../images/terrain.png", ortho));
  files.insert("images/terrain.png", png(ortho));
  for grid in Grid::ALL {
    let (png, of) = (png(grid), files_of(grid));
    // One more than the guard loads, for the untimed load that starts the
    // workers
    for i in 0..=LOADS {
      for (kind, infinite) in [("cold", false), ("inf-cold", true)] {
        let tileset = format!("../tilesets/{of}{kind}-{i}.tsx");
        let map = if infinite {
          tmx_infinite(&cells, &tileset, grid)
        } else {
          tmx(&cells, &tileset, grid)
        };
        files.insert(&format!("maps/{of}{kind}-{i}.tmx"), map);
        files.insert(
          &format!("tilesets/{of}{kind}-{i}.tsx"),
          tsx(&format!("../images/{of}{kind}-{i}.png"), grid),
        );
        files.insert(&format!("images/{of}{kind}-{i}.png"), png.clone());
      }
    }
  }
  for i in 0..=LOADS {
    files.insert(&format!("maps/warm-{i}.tmx"), terrain.clone());
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

/// The loads, on every grid: twenty of fresh files (map, tileset and
/// image), bounded and infinite; on the orthogonal grid twenty level
/// changes too (the tileset loaded already), and the main thread's pumps
fn load_guard(files: &MemorySource) {
  let mut world = asset_world_over(files.clone());
  let server = world.resource::<AssetServer>().clone();
  // The workers start with the first load
  timed_load(&mut world, &server, &format!("maps/cold-{LOADS}.tmx"));
  for grid in Grid::ALL {
    let of = files_of(grid);
    let (mut cold, mut pumps) = (Vec::new(), Vec::new());
    for i in 0..LOADS {
      let path = format!("maps/{of}cold-{i}.tmx");
      let (took, pumping) = timed_load(&mut world, &server, &path);
      cold.push(took);
      pumps.push(pumping);
    }
    let mut infinite = Vec::new();
    for i in 0..LOADS {
      let path = format!("maps/{of}inf-cold-{i}.tmx");
      infinite.push(timed_load(&mut world, &server, &path).0);
    }
    let map = format!("the {} map", grid.name());
    let held = median(cold.clone());
    let (met, held_text) = verdict(held, LOAD_TARGET);
    let line = format!(
      "{map}: a 256 x 256 four-layer base64 and zlib TMX with its TSX and PNG, {LOADS} loads of \
       fresh files: {}; {held_text}",
      shown(cold)
    );
    eprintln!("load: {line}");
    if !met {
      guards::miss(format!("load, {line}"));
    }
    if grid == Grid::Orthogonal {
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
      eprintln!(
        "load (no target), {map}: the main thread's pumps meanwhile {}; a level change (the \
         tileset loaded already) {}",
        shown(pumps),
        shown(warm),
      );
    }
    let held = median(infinite.clone());
    let (met, held_text) = verdict(held, LOAD_TARGET);
    let line = format!(
      "{map}: a 256 x 256 four-layer infinite TMX (16 x 16 chunks, base64 and zlib) with its TSX \
       and PNG, {LOADS} loads of fresh files: {}; {held_text}",
      shown(infinite)
    );
    eprintln!("load: {line}");
    if !met {
      guards::miss(format!("load, {line}"));
    }
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
/// (the orthogonal map, or `benched`'s grid's)
fn load_bench(c: &mut Criterion, files: &mut Option<MemorySource>) {
  let path = format!("maps/{}cold-0.tmx", files_of(benched().grid()));
  c.bench_function("load", |b| {
    let files = files.get_or_insert_with(load_files).clone();
    b.iter_batched(
      || {
        let world = asset_world_over(files.clone());
        let server = world.resource::<AssetServer>().clone();
        (world, server)
      },
      |(mut world, server)| timed_load(&mut world, &server, &path),
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

/// The streaming guard's scene: an endless world on a grid's cells, of two
/// unbounded layers (the ground colliding and casting shadows) streamed
/// round the camera
struct EndlessScene {
  grid: Grid,
}

impl Scene for EndlessScene {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    let source = add_tileset(ctx, self.grid);
    let mut map = TileMap::infinite(self.grid.cell()).with_shape(self.grid.shape());
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

/// The chunks the streaming guard's crossing meets (warm-up included) on
/// `grid`'s endless world, the view grown by the load distance: those the
/// stream lands round the view, by row
fn crossed_chunks(grid: Grid) -> Vec<IVec2> {
  let half = Vec2::new(RESOLUTION.0 as f32, RESOLUTION.1 as f32) / 2.0 + LOAD;
  let endless = grid.endless();
  let mut chunks = BTreeSet::new();
  for frame in 0..=WARM + ROUNDS {
    let at = crossing_at(frame);
    let area = Rect::new(at.x - half.x, at.y - half.y, 2.0 * half.x, 2.0 * half.y);
    for chunk in chunks_over(&endless, area) {
      chunks.insert((chunk.y, chunk.x));
    }
  }
  chunks.into_iter().map(|(y, x)| IVec2::new(x, y)).collect()
}

/// The streaming guard's static baseline: the endless world's chunks the
/// crossing meets (`crossed_chunks`, the generator's own tiles), built in
/// code as an infinite map of the same two layers (the ground colliding
/// and casting shadows), with no stream: what the map costs drawn and
/// still, so the world over it is what streaming adds
struct CrossedScene {
  grid: Grid,
}

impl Scene for CrossedScene {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    let source = add_tileset(ctx, self.grid);
    let mut map = TileMap::infinite(self.grid.cell()).with_shape(self.grid.shape());
    let set = map.add_tileset(source);
    let layers = [
      ("background", map.add_tile_layer("background")),
      ("ground", map.add_tile_layer("ground")),
    ];
    for chunk in crossed_chunks(self.grid) {
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

/// The chunks of a streamed map on `grid`, at the origin, that `area`
/// takes, by row. On an orthogonal grid those it overlaps (touching is not
/// overlapping), as the stream takes them. On any other grid every chunk
/// whose box (its cells' outlines' box) meets it, touching included: the
/// stream takes those of them `MapGrid`'s chunk range over the area holds,
/// so this holds every chunk it takes
fn chunks_over(grid: &MapGrid, area: Rect) -> Vec<IVec2> {
  let (min, max) = (
    Vec2::new(area.x, area.y),
    Vec2::new(area.x + area.width, area.y + area.height),
  );
  if grid.shape == GridShape::Orthogonal {
    let side = grid.cell.as_vec2() * CHUNK_SIZE as f32;
    let (first, last) = (
      ((min - grid.origin) / side).floor().as_ivec2(),
      (((max - grid.origin) / side).ceil() - 1.0).as_ivec2(),
    );
    return (first.y..=last.y)
      .flat_map(|y| (first.x..=last.x).map(move |x| IVec2::new(x, y)))
      .collect();
  }
  let steps = ChunkSteps::of(grid);
  steps.meeting(min.as_dvec2(), max.as_dvec2())
}

/// The most chunks an area of `view` grown by `by` on every side can take
/// on `grid`, wherever it is. On an orthogonal grid a span of `n` chunks'
/// length meets `n + 1` of them at most; on any other grid, the most
/// chunks' boxes a box of that size meets (`chunks_over`)
fn chunks_bound(grid: &MapGrid, view: CameraBounds, by: f32) -> usize {
  let size = Vec2::new(view.width() + 2.0 * by, view.height() + 2.0 * by);
  if grid.shape == GridShape::Orthogonal {
    let side = grid.cell.as_vec2() * CHUNK_SIZE as f32;
    let meets = |length: f32, side: f32| (length / side).ceil() as usize + 1;
    return meets(size.x, side.x) * meets(size.y, side.y);
  }
  ChunkSteps::of(grid).most_meeting(size.as_dvec2())
}

/// Where a grid's chunks' boxes are: chunk (0, 0)'s, and the steps from a
/// chunk's box to the next along x and along y (every grid repeats by whole
/// chunks), so chunk `c`'s box is chunk (0, 0)'s moved by `c.x` steps along
/// x and `c.y` along y
struct ChunkSteps {
  lo: DVec2,
  hi: DVec2,
  x: DVec2,
  y: DVec2,
}

impl ChunkSteps {
  fn of(grid: &MapGrid) -> ChunkSteps {
    let side = CHUNK_SIZE as i32;
    let corners = |chunk: IVec2| {
      let first = chunk * side;
      let b = grid.bounds_of(CellRect::new(first, first + side));
      (
        DVec2::new(f64::from(b.x), f64::from(b.y)),
        DVec2::new(f64::from(b.x + b.width), f64::from(b.y + b.height)),
      )
    };
    let (lo, hi) = corners(IVec2::ZERO);
    ChunkSteps {
      lo,
      hi,
      x: corners(IVec2::X).0 - lo,
      y: corners(IVec2::Y).0 - lo,
    }
  }

  /// Where `chunk`'s box is moved from chunk (0, 0)'s
  fn moved(&self, chunk: IVec2) -> DVec2 {
    self.x * f64::from(chunk.x) + self.y * f64::from(chunk.y)
  }

  /// The chunks whose boxes meet the box `min..max`, touching included, by
  /// row: chunk `c`'s does where its move lies in `min - hi..max - lo`
  fn meeting(&self, min: DVec2, max: DVec2) -> Vec<IVec2> {
    let (a, b) = (min - self.hi, max - self.lo);
    // The chunks whose moves can lie in `a..b`: the box's corners taken
    // back through the steps, a chunk more each side for rounding
    let det = self.x.x * self.y.y - self.y.x * self.x.y;
    if det == 0.0 || !det.is_finite() {
      return Vec::new();
    }
    let back = |p: DVec2| {
      DVec2::new(
        (p.x * self.y.y - p.y * self.y.x) / det,
        (p.y * self.x.x - p.x * self.x.y) / det,
      )
    };
    let corners = [a, DVec2::new(b.x, a.y), b, DVec2::new(a.x, b.y)].map(back);
    let lo = corners.iter().fold(DVec2::INFINITY, |m, &c| m.min(c));
    let hi = corners.iter().fold(DVec2::NEG_INFINITY, |m, &c| m.max(c));
    let (first, last) = (
      lo.floor().as_ivec2() - IVec2::ONE,
      hi.ceil().as_ivec2() + IVec2::ONE,
    );
    (first.y..=last.y)
      .flat_map(|y| (first.x..=last.x).map(move |x| IVec2::new(x, y)))
      .filter(|&chunk| {
        let at = self.moved(chunk);
        at.cmpge(a).all() && at.cmple(b).all()
      })
      .collect()
  }

  /// The most chunks' boxes a box of `size` meets, wherever it is: the
  /// most moves a box of `size` plus a chunk's box holds. Slid right until
  /// the leftmost move it holds is on its left side, then down until the
  /// topmost is on its top, a box holds as many as it did, so the most is
  /// that of a box with a move on its left side and one on its top: the
  /// move (0, 0) on its left (every move is a step from another), and on
  /// its top any move it can then hold
  fn most_meeting(&self, size: DVec2) -> usize {
    // A box `r0..r1` meets the chunks whose moves lie in `r0 - hi..r1 - lo`,
    // a window `span` in size; the window `t..t + span` is the box's at
    // `t + hi`
    let span = size + (self.hi - self.lo);
    let held = |t: DVec2| self.meeting(t + self.hi, t + self.hi + size).len();
    let tops = self.meeting(
      DVec2::new(0.0, -span.y) + self.hi,
      DVec2::new(span.x, 0.0) + self.lo,
    );
    tops
      .into_iter()
      .map(|chunk| held(DVec2::new(0.0, self.moved(chunk).y)))
      .max()
      .unwrap_or(0)
  }
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

/// An engine of the streaming guard on `grid`'s cells, its camera at the
/// path's start: the endless world with every chunk of the view's load
/// area landed, and its root; the static baseline landed, and its root; or
/// an empty scene. None without a GPU
fn stream_engine(show: Crossed, grid: Grid) -> Option<(HeadlessEngine, Option<Entity>)> {
  let builder = EngineBuilder::new().resolution(RESOLUTION.0, RESOLUTION.1);
  let builder = match show {
    Crossed::Empty => builder.start_scene(Nothing),
    Crossed::Streamed => builder.start_scene(EndlessScene { grid }),
    Crossed::Baseline => builder.start_scene(CrossedScene { grid }),
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

/// How many chunks are resident in `engine`'s streamed world on `grid`
/// (each is in every unbounded layer, so the layer holding the most), and
/// those of them lying outside `keep`, into `outside`
fn residents(
  engine: &mut HeadlessEngine,
  grid: &MapGrid,
  keep: Rect,
  outside: &mut Vec<IVec2>,
) -> usize {
  outside.clear();
  let kept = chunks_over(grid, keep);
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
      .filter(|chunk| !kept.contains(chunk)),
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
  (root, grid): (Entity, &MapGrid),
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
    residents(streamed, grid, grown(view, UNLOAD), &mut outside);
    if out.settled.is_none() && outside.is_empty() {
      out.settled = Some(frame + 1);
    }
  }
  out.steps = over(&out.steps, median(empty_steps));
  out
}

/// The streaming guard on `grid`'s cells: the endless world's step over an
/// empty frame's median, its 99th percentile held to 0.5 ms if `held` (else
/// measured with no target); every frame shows no unfilled chunk and keeps
/// its resident chunks within the radii
fn stream_guard(grid: Grid, held: bool) {
  let (Some((mut empty, _)), Some((mut streamed, Some(root))), Some((mut baseline, _))) = (
    stream_engine(Crossed::Empty, grid),
    stream_engine(Crossed::Streamed, grid),
    stream_engine(Crossed::Baseline, grid),
  ) else {
    return;
  };
  let (endless, world_name) = (grid.endless(), format!("the {} world", grid.name()));
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
    let count = residents(&mut streamed, &endless, grown(seen, UNLOAD), &mut outside);
    let most = chunks_bound(&endless, seen, UNLOAD);
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
    let came_into_view =
      chunks_over(&endless, grown(drawn, 0.0)) != chunks_over(&endless, grown(seen, 0.0));
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
  let jump = teleport((&mut empty, &mut streamed), (root, &endless), &mut cursor);
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
    "stream guard, {world_name}: {ROUNDS} interleaved frames at {}x{} paced at 60 Hz, the camera \
     crossing an endless world at {STREAM_SPEED} px/s heading ({}, {}), chunks loading within \
     {LOAD} px of its view and unloading past {UNLOAD}. An empty frame (its own spread is the \
     machine's noise): step {}; render step {}",
    RESOLUTION.0,
    RESOLUTION.1,
    HEADING.x,
    HEADING.y,
    shown_at(empty_steps.clone(), 99),
    shown_at(empty_renders, 99),
  );
  let (all, renders) = added(|_| true);
  let p99_added = spread_at(all.clone(), 99).map_or(Duration::ZERO, |(_, p99, _)| p99);
  let line = format!(
    "{world_name}: the endless world's step over the empty frame's median, {}; its render step \
     over the empty frame's, {}",
    shown_at(all, 99),
    shown_at(renders, 99),
  );
  if held {
    let (met, verdict) = verdict(p99_added, STREAM_TARGET);
    eprintln!(
      "stream: {line}. Held, the 99th percentile: {:.1} µs, {verdict}",
      us(p99_added)
    );
    if !met {
      guards::miss(format!("stream, {line}"));
    }
  } else {
    eprintln!(
      "stream (no target): {line}. The 99th percentile: {:.1} µs",
      us(p99_added)
    );
  }
  // With no target, so one quiet run tells the machine's floor from the
  // stream's own cost: the 99th percentiles apart, and a static baseline
  let p99 = |times: &[Duration]| spread_at(times.to_vec(), 99).map_or(Duration::ZERO, |s| s.1);
  let world_steps: Vec<Duration> = frames.iter().map(|f| f.step).collect();
  let (world_p99, empty_p99, baseline_p99) =
    (p99(&world_steps), p99(&empty_steps), p99(&baseline_steps));
  let apart = |a: Duration, b: Duration| us(a) - us(b);
  eprintln!(
    "stream (no target), {world_name}: the endless world's 99th percentile less the empty \
     frame's, {:.1} µs (the world's {:.1}, the empty frame's {:.1})",
    apart(world_p99, empty_p99),
    us(world_p99),
    us(empty_p99),
  );
  let baseline_step = median(baseline_steps.clone());
  let baseline_render = median(baseline_renders.clone());
  let world_renders: Vec<Duration> = frames.iter().map(|f| f.render).collect();
  eprintln!(
    "stream (no target), {world_name}: a static baseline, the {} chunks the crossing meets (the \
     view grown by the load distance) built in code as an infinite map of the same two layers, \
     colliding and casting shadows, with no MapStream, stepped in the same rounds: its step over \
     the empty frame's median, {}; its render step over the empty frame's, {}; its 99th \
     percentile less the empty frame's, {:.1} µs",
    crossed_chunks(grid).len(),
    shown_at(over(&baseline_steps, empty_step), 99),
    shown_at(over(&baseline_renders, empty_render), 99),
    apart(baseline_p99, empty_p99),
  );
  eprintln!(
    "stream (no target), {world_name}: the endless world over the static baseline: its step \
     over the baseline's median, {}; its render step over the baseline's, {}; its 99th \
     percentile less the baseline's, {:.1} µs",
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
      "stream (no target), {world_name}: the frames {kind} ({}): {}; render step {}",
      steps.len(),
      shown_at(steps, 99),
      shown_at(renders, 99),
    );
  }
  let budgets = MapStream::new(|_: ChunkRequest| ChunkTiles::default());
  let frames_of = |frame: Option<usize>| frame.map_or("never".to_string(), |f| f.to_string());
  eprintln!(
    "stream (no target), {world_name}: a teleport {} px on ({} orthogonal chunks) after \
     the crossing, the camera staying there {TELEPORT_FRAMES} frames: its view filled in \
     {} frames, the chunks left behind gone in {} (at most {} landed and {} unloaded a frame; \
     the budgets {} and {}); those frames' steps over the empty frames' median, {}",
    JUMP.x,
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
    "stream, {world_name}: {landings} chunks landed and {unloads} unloaded over the measured \
     frames, both in {both} frames, the most in one frame {} ({} landed, {} unloaded); resident \
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
    "stream, {world_name}: {} of the {} frames (warm-up included) showed a chunk not landed; \
     the first, frame {}",
    unfilled.len(),
    WARM + ROUNDS,
    unfilled[0],
  );
  // A chunk past the unload distance waits at most as many frames as the
  // unload budget takes to drain every chunk the radii hold
  let budget = MapStream::new(|_: ChunkRequest| ChunkTiles::default()).unload_per_frame as usize;
  let drain = bound.div_ceil(budget.max(1));
  eprintln!(
    "stream, {world_name}: resident chunks past the unload distance, waiting their turn \
     ({budget} unload a frame): at most {most_outside} in a frame, none waiting more than \
     {longest} frames (the budget drains the radii's bound in {drain})"
  );
  assert!(
    over_bound.is_empty() && longest <= drain,
    "stream, {world_name}: resident chunks past the radii: frames over the bound (frame, \
     resident, bound) {over_bound:?}; a chunk past the unload distance waited {longest} frames, \
     past {drain}"
  );
  eprintln!(
    "stream, {world_name}: no unfilled chunk on screen in any of the {} frames, and the resident \
     chunks within the radii's bound in every one",
    WARM + ROUNDS
  );
}

/// The streaming bench: the endless world's engine stepped back to back,
/// its camera crossing the world (the orthogonal world, or `benched`'s
/// grid's)
fn stream_bench(c: &mut Criterion) {
  let mut made: Option<(HeadlessEngine, usize)> = None;
  let grid = benched().grid();
  c.bench_function("stream", |b| {
    if made.is_none() {
      made = stream_engine(Crossed::Streamed, grid).map(|(engine, _)| (engine, 0));
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
    stream_guard(Grid::Orthogonal, true);
    stream_guard(Grid::Isometric, true);
    // Any other grid `RYO_TILEMAP_GRID` names, with no target
    let other = benched().grid();
    if !matches!(other, Grid::Orthogonal | Grid::Isometric) {
      stream_guard(other, false);
    }
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
     edit, load, landing or stream; for a miss on another grid than the orthogonal, with \
     RYO_TILEMAP_GRID=isometric, staggered, hexagonal or oblique, as its line names, or tall for \
     the isometric map with tall props, in the environment: `--env RYO_TILEMAP_GRID=isometric` \
     before `--launch`) and charge each sample's time to the nearest ryo2d function from the \
     leaf up.",
  );
  eprintln!("tilemap guards: every guard that ran met its target");
}
