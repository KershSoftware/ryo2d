//! Allocations of map drawing, counted by a global allocator in this test
//! binary: a padded tileset cache hit, and a frame drawing a static map
//! under a shadowed light (its camera still, then panning over chunks it
//! has drawn).
//!
//! A frame's allocations are told apart by their stacks: those made inside
//! the GPU API (wgpu, naga and the Metal bindings, which allocate as they
//! record any frame's draw calls, and in debug builds as they check each
//! Objective-C message) are the GPU's; every other one is the engine's own.
//! A frame drawing a static map makes no more of the engine's own than an
//! empty frame; the GPU's, which grow with the draw calls the map adds,
//! are printed, and none may come from the map's own path (the map module or
//! its drawer), which would be a GPU resource made or written every frame.
//! The engine's own are compared site by site, so a map's allocation cannot
//! hide behind one only the empty frame makes.
// Each thread counts its own allocations, so the tests (one a thread)
// never count each other's

use std::alloc::{GlobalAlloc, Layout, System};
use std::backtrace::Backtrace;
use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::sync::Arc;

use ryo2d::core::tilemap::padded::PaddedTilesets;
use ryo2d::prelude::*;
use ryo2d::testing::{HeadlessEngine, asset_world};

thread_local! {
  static ALLOCATIONS: Cell<usize> = const { Cell::new(0) };
  /// Whether each allocation's stack is kept, and whether one is being
  /// taken (its own allocations are not)
  static TRACING: Cell<bool> = const { Cell::new(false) };
  static IN_TRACE: Cell<bool> = const { Cell::new(false) };
  static TRACES: RefCell<Vec<Backtrace>> = const { RefCell::new(Vec::new()) };
}

/// Counts an allocation, and keeps its stack while tracing (taking a stack
/// allocates, and those are not counted)
fn note() {
  if IN_TRACE.with(Cell::get) {
    return;
  }
  ALLOCATIONS.with(|c| c.set(c.get() + 1));
  if TRACING.with(Cell::get) {
    IN_TRACE.with(|c| c.set(true));
    let trace = Backtrace::force_capture();
    TRACES.with(|t| t.borrow_mut().push(trace));
    IN_TRACE.with(|c| c.set(false));
  }
}

/// Counts the allocations each thread makes
struct Counting;

// SAFETY: every call goes to `System`. The counters are const-initialised
// thread locals without destructors, so touching them never allocates; a
// stack is only taken while tracing, and its own allocations (and the
// list's) happen with `IN_TRACE` set, so they are neither counted nor
// traced
unsafe impl GlobalAlloc for Counting {
  unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
    note();
    unsafe { System.alloc(layout) }
  }
  unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
    unsafe { System.dealloc(ptr, layout) }
  }
  unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new: usize) -> *mut u8 {
    note();
    unsafe { System.realloc(ptr, layout, new) }
  }
}

#[global_allocator]
static COUNTING: Counting = Counting;

/// The allocations this thread has made
fn allocations() -> usize {
  ALLOCATIONS.with(Cell::get)
}

/// A `cols` x `rows` atlas of `tile` px tiles in `images`, each tile its
/// own colour
fn atlas(images: &mut Assets<Image>, tile: u32, cols: u32, rows: u32) -> Tileset {
  let (w, h) = (tile * cols, tile * rows);
  let mut data = Vec::with_capacity((w * h * 4) as usize);
  for y in 0..h {
    for x in 0..w {
      let id = (y / tile) * cols + x / tile;
      data.extend_from_slice(&[(id * 40) as u8, (id * 90) as u8, 255 - id as u8, 255]);
    }
  }
  let image = images.add(Image::from_rgba(data, w, h));
  let mut set = Tileset::new(
    "atlas",
    UVec2::splat(tile),
    TilesetImage::Atlas {
      image,
      size: UVec2::new(w, h),
    },
  );
  set.columns = cols;
  set.tile_count = cols * rows;
  set
}

#[test]
fn a_cache_hit_allocates_nothing() {
  let mut world = asset_world();
  world.resource_scope(|world, mut images: Mut<Assets<Image>>| {
    let mut tilesets = world.resource_mut::<Assets<Tileset>>();
    let atlas = atlas(&mut images, 16, 2, 2);
    let a = images.add(Image::solid_rect(16, 16, 255, 0, 0, 255));
    let b = images.add(Image::solid_rect(8, 24, 0, 255, 0, 255));
    let mut collection = Tileset::new("c", UVec2::splat(16), TilesetImage::Collection);
    collection.tile_count = 40;
    for id in 0..40 {
      let (handle, size) = if id % 2 == 0 {
        (a.clone(), UVec2::splat(16))
      } else {
        (b.clone(), UVec2::new(8, 24))
      };
      collection.insert_tile(
        id,
        TileData {
          image: Some((handle, size)),
          ..TileData::default()
        },
      );
    }
    let sources = [
      TilesetSource::Asset(tilesets.add(atlas)),
      TilesetSource::Asset(tilesets.add(collection.clone())),
      TilesetSource::Embedded(Arc::new(collection)),
    ];
    let mut padded = PaddedTilesets::default();
    for round in 0..2 {
      padded.begin_round();
      for source in &sources {
        assert!(
          padded.get(source, &tilesets, &mut images).is_some(),
          "round {round}"
        );
      }
      padded.end_round();
    }
    // Warm: every further round is a hit
    let before = allocations();
    for _ in 0..10 {
      padded.begin_round();
      for source in &sources {
        // Handle and layout clones are reference counts
        drop(padded.get(source, &tilesets, &mut images));
      }
      padded.end_round();
    }
    assert_eq!(allocations(), before, "a cache hit allocated");
  });
}

/// The map's tiles: a background, solid ground that collides and casts
/// shadows, an animated tile, a y-sorted layer
const BACK: u32 = 0;
const GROUND: u32 = 1;
const PROP: u32 = 2;
const WATER: u32 = 3;
const SIDE: u32 = 128;

/// A scene with the static map (or nothing, `empty`) under a shadowed
/// light: four tile layers of 128 x 128 cells over an atlas, the third
/// y-sorted; an image layer; an object layer holding a tile object. The
/// light is in both, so each frame is lit and the map's frame differs only
/// by the map (its shadow chains among them)
struct StaticMap {
  empty: bool,
}

impl Scene for StaticMap {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    let centre = Vec2::splat(SIDE as f32 * 8.0);
    ctx.world.spawn((
      Transform::from_translation(centre),
      Light2D::point(Color::WHITE, 240.0).with_shadows(),
    ));
    if self.empty {
      return;
    }
    let mut set = atlas(&mut ctx.world.resource_mut::<Assets<Image>>(), 16, 4, 1);
    set.insert_tile(
      GROUND,
      TileData {
        shapes: vec![TileShape {
          points: vec![
            Vec2::ZERO,
            Vec2::new(16.0, 0.0),
            Vec2::new(16.0, 16.0),
            Vec2::new(0.0, 16.0),
          ],
          ..TileShape::default()
        }],
        ..TileData::default()
      },
    );
    let frame = |tile| TileFrame {
      tile,
      duration: std::time::Duration::from_millis(250),
    };
    set.insert_tile(
      WATER,
      TileData {
        animation: vec![frame(WATER), frame(BACK)],
        ..TileData::default()
      },
    );
    let set = TilesetSource::Asset(ctx.assets().add(set));
    let mut map = TileMap::new(UVec2::splat(SIDE), UVec2::splat(16));
    let set = map.add_tileset(set);
    let backdrop = ctx.assets().add(Image::solid_rect(64, 64, 20, 30, 60, 255));
    let mut info = LayerInfo::new(100, "backdrop");
    info.collides = false;
    map.layers.push(MapLayer {
      info,
      kind: LayerKind::Image(ImageLayer {
        image: backdrop,
        repeat_x: true,
        repeat_y: true,
      }),
    });
    for name in ["back", "ground", "props", "water"] {
      map.add_tile_layer(name);
    }
    let side = SIDE as i32;
    for y in 0..side {
      for x in 0..side {
        let cell = IVec2::new(x, y);
        let mut put = |layer: usize, id: u32| {
          map
            .tiles_mut(layer)
            .expect("a tile layer")
            .set(cell, Some(Tile::new(set, id)));
        };
        put(1, BACK);
        if (x / 5 + y / 3) % 4 == 0 {
          put(2, GROUND);
        } else if (x * 7 + y * 3) % 11 == 0 {
          put(3, PROP);
        } else if (x + y) % 13 == 0 {
          put(4, WATER);
        }
      }
    }
    map.layers[3].info.y_sorted = true;
    let objects = map.add_object_layer("objects");
    if let LayerKind::Objects(objects) = &mut map.layers[objects].kind {
      objects.push(MapObjectData {
        id: "1".into(),
        name: "statue".into(),
        class: None,
        position: Vec2::new(64.0, 64.0),
        size: Vec2::splat(16.0),
        rotation: 0.0,
        shape: ObjectShape::Rect,
        tile: Some(Tile::new(set, PROP)),
        visible: true,
        properties: Properties::default(),
      });
    }
    let handle = ctx.assets().add(map);
    ctx.world.spawn((Transform::IDENTITY, MapRoot::new(handle)));
  }
}

/// An engine on the static map (or nothing), landed and warmed up, its
/// camera at `at`. None without a GPU
fn engine(empty: bool, at: Vec2) -> Option<HeadlessEngine> {
  let builder = EngineBuilder::new()
    .resolution(640, 360)
    .start_scene(StaticMap { empty });
  let mut engine = HeadlessEngine::new_or_skip(builder)?;
  if let Some(mut camera) = CameraMut::main(engine.world_mut()) {
    camera.set_position(at.x, at.y);
  }
  let start = std::time::Instant::now();
  loop {
    engine.step().expect("a frame");
    let world = engine.world_mut();
    let roots: Vec<Entity> = world
      .query_filtered::<Entity, With<MapRoot>>()
      .iter(world)
      .collect();
    let view = MapsView::new(world);
    if roots.iter().all(|&root| view.is_ready(root)) {
      break;
    }
    assert!(
      start.elapsed() < std::time::Duration::from_secs(10),
      "the map landed"
    );
  }
  engine.step_frames(30).expect("the warm-up frames");
  Some(engine)
}

/// Where a stack's frames come from: the GPU API's crates
const GPU: [&str; 5] = ["wgpu", "naga::", "objc2", "metal::", "block2"];

/// One frame's allocations: how many; of those not made inside the GPU API,
/// how many were made where (the nearest engine function and the one it
/// called); and those made inside the GPU API whose nearest engine function
/// is the map's own (the map module or its drawer)
struct Frame {
  count: usize,
  own: BTreeMap<String, usize>,
  map_gpu: Vec<String>,
}

/// A stack's function names, leaf first
fn functions(text: &str) -> Vec<&str> {
  text
    .lines()
    .map(str::trim)
    .filter(|line| !line.starts_with("at "))
    .filter_map(|line| line.split_once(": ").map(|(_, name)| name))
    .collect()
}

/// A step of `engine`, its allocations traced
fn traced(engine: &mut HeadlessEngine) -> Frame {
  TRACES.with(|t| t.borrow_mut().clear());
  let before = allocations();
  TRACING.with(|c| c.set(true));
  engine.step().expect("a frame");
  TRACING.with(|c| c.set(false));
  let count = allocations() - before;
  let traces = TRACES.with(|t| std::mem::take(&mut *t.borrow_mut()));
  let (mut own, mut map_gpu) = (BTreeMap::new(), Vec::new());
  for trace in traces {
    let text = trace.to_string();
    let frames = functions(&text);
    let ours = frames.iter().position(|f| f.contains("ryo2d::"));
    if GPU.iter().any(|gpu| text.contains(gpu)) {
      if let Some(at) = ours
        && (frames[at].contains("tilemap::") || frames[at].contains("render::tiles"))
      {
        map_gpu.push(frames[at].to_string());
      }
      continue;
    }
    let site = match ours {
      Some(at) => format!("{} <- {}", frames[at.saturating_sub(1)], frames[at]),
      None => frames.first().copied().unwrap_or("?").to_string(),
    };
    *own.entry(site).or_insert(0) += 1;
  }
  Frame {
    count,
    own,
    map_gpu,
  }
}

/// The sites where `drawn` makes more allocations than `nothing`
fn more(drawn: &Frame, nothing: &Frame) -> Vec<String> {
  drawn
    .own
    .iter()
    .filter(|(site, n)| nothing.own.get(*site).copied().unwrap_or(0) < **n)
    .map(|(site, n)| format!("{n} at {site}"))
    .collect()
}

/// Puts `engine`'s camera at `at`
fn look_at(engine: &mut HeadlessEngine, at: Vec2) {
  if let Some(mut camera) = CameraMut::main(engine.world_mut()) {
    camera.set_position(at.x, at.y);
  }
}

#[test]
fn a_frame_drawing_a_static_map_allocates_nothing() {
  let centre = Vec2::splat(SIDE as f32 * 8.0);
  let (Some(mut empty), Some(mut map)) = (engine(true, centre), engine(false, centre)) else {
    return;
  };
  // Panning 256 px right and back, its chunks drawn the first time over
  let pan = |frame: usize| {
    let t = (frame % 64) as f32 / 64.0;
    centre + Vec2::new(256.0 * (1.0 - (2.0 * t - 1.0).abs()), 0.0)
  };
  for frame in 0..64 {
    look_at(&mut map, pan(frame));
    look_at(&mut empty, pan(frame));
    map.step().expect("a frame");
    empty.step().expect("a frame");
  }
  // Still, then at four places along the pan, two of them going each way
  let places = [centre, centre, pan(8), pan(24), pan(40), pan(56)];
  for (frame, at) in places.into_iter().enumerate() {
    look_at(&mut map, at);
    look_at(&mut empty, at);
    let (drawn, nothing) = (traced(&mut map), traced(&mut empty));
    let own = |frame: &Frame| frame.own.values().sum::<usize>();
    println!(
      "frame {frame} at {at}: the map's frame {} allocations ({} the engine's own), an empty \
       frame {} ({} its own)",
      drawn.count,
      own(&drawn),
      nothing.count,
      own(&nothing),
    );
    let more = more(&drawn, &nothing);
    assert!(
      more.is_empty(),
      "frame {frame}: the map's frame made allocations of the engine's own an empty frame did \
       not: {more:#?}"
    );
    assert!(
      drawn.map_gpu.is_empty(),
      "frame {frame}: the map's path made GPU allocations: {:#?}",
      drawn.map_gpu
    );
  }
}
