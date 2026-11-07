//! Allocations of map drawing, counted by a global allocator in this test
//! binary: a padded tileset cache hit, a frame drawing a static map under a
//! shadowed light (its camera still, then panning over chunks it has
//! drawn) on every grid, its layers colliding and casting shadows (the
//! hexagonal map's too), an idle frame of a streamed map whose chunks have landed, on
//! orthogonal and on isometric cells, and a frame of characters walking on
//! a map's storeys, drawn lifted.
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
use std::sync::atomic::{AtomicBool, Ordering};

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
/// by the map (its shadow chains among them). Its tiles fit their cells
/// (`Art::Fitting`), or its plain layers draw in Tiled's order across
/// tilesets and chunks (the other `Art`s)
struct StaticMap {
  empty: bool,
  art: Art,
}

/// The static map's grid, and how its 16 px tiles sit in their cells
#[derive(Clone, Copy, PartialEq, Debug)]
enum Art {
  /// Orthogonal, on cells of 16 px: the tiles fit
  Fitting,
  /// Isometric, on cells of 16 x 8: the tiles stand taller than their
  /// cells, so its layers draw chunk by chunk in row order, a part a draw
  Tall,
  /// Staggered along y, on cells of 12 x 8: the tiles reach past their
  /// cells' sides too, so its layers draw every chunk's runs band by band
  Wide,
  /// Oblique, on cells of 16 px skewed 4 px down along their columns: the
  /// cells of a y-sorted layer share no sort point, so each draws alone
  Sheared,
  /// Hexagonal along x, flat-topped hexagons of 16 px with sides of 8, a
  /// row's unshifted cells painted before its shifted ones; a third of the
  /// tiles turned 60 degrees and a third 120, which reach past their
  /// cells' boxes though the tiles fit them unturned. Its ground's tiles
  /// are whole hexagons: its colliders and shadows the hexagons' sides
  Turned,
}

impl StaticMap {
  /// Where the map's middle is in its pixels
  fn centre(&self) -> Vec2 {
    match self.art {
      Art::Fitting => Vec2::splat(SIDE as f32 * 8.0),
      Art::Tall => Vec2::new(SIDE as f32 * 8.0, SIDE as f32 * 4.0),
      Art::Wide => Vec2::new(SIDE as f32 * 6.0, SIDE as f32 * 2.0),
      Art::Sheared => Vec2::new(SIDE as f32 * 8.0, SIDE as f32 * 10.0),
      Art::Turned => Vec2::new(SIDE as f32 * 6.0, SIDE as f32 * 8.0),
    }
  }
}

impl Scene for StaticMap {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    let centre = self.centre();
    ctx.world.spawn((
      Transform::from_translation(centre),
      Light2D::point(Color::WHITE, 240.0).with_shadows(),
    ));
    if self.empty {
      return;
    }
    let mut set = atlas(&mut ctx.world.resource_mut::<Assets<Image>>(), 16, 4, 1);
    // The ground fills its cell: the tile's square, or on the hexagonal grid
    // its hexagon, a solid cell however it is turned
    let ground = match self.art {
      Art::Turned => [
        (4.0, 0.0),
        (12.0, 0.0),
        (16.0, 8.0),
        (12.0, 16.0),
        (4.0, 16.0),
        (0.0, 8.0),
      ]
      .map(|(x, y)| Vec2::new(x, y))
      .to_vec(),
      _ => vec![
        Vec2::ZERO,
        Vec2::new(16.0, 0.0),
        Vec2::new(16.0, 16.0),
        Vec2::new(0.0, 16.0),
      ],
    };
    set.insert_tile(
      GROUND,
      TileData {
        shapes: vec![TileShape {
          points: ground,
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
    // The y-sorted props sort 4 px below where they stand, each alone
    set.insert_tile(
      PROP,
      TileData {
        sort_offset: 4.0,
        ..TileData::default()
      },
    );
    let set = TilesetSource::Asset(ctx.assets().add(set));
    let mut map = match self.art {
      Art::Fitting => TileMap::new(UVec2::splat(SIDE), UVec2::splat(16)),
      Art::Tall => {
        TileMap::new(UVec2::splat(SIDE), UVec2::new(16, 8)).with_shape(GridShape::Isometric)
      }
      Art::Wide => {
        TileMap::new(UVec2::splat(SIDE), UVec2::new(12, 8)).with_shape(GridShape::Staggered {
          axis: StaggerAxis::Y,
          index: StaggerIndex::Odd,
        })
      }
      Art::Sheared => {
        TileMap::new(UVec2::splat(SIDE), UVec2::splat(16)).with_shape(GridShape::Oblique {
          skew: IVec2::new(0, 4),
        })
      }
      Art::Turned => {
        TileMap::new(UVec2::splat(SIDE), UVec2::splat(16)).with_shape(GridShape::Hexagonal {
          axis: StaggerAxis::X,
          index: StaggerIndex::Odd,
          side: 8,
        })
      }
    };
    let hex = self.art == Art::Turned;
    let set = map.add_tileset(set);
    let backdrop = ctx.assets().add(Image::solid_rect(64, 64, 20, 30, 60, 255));
    let mut info = LayerInfo::new(100, "backdrop");
    info.collides = false;
    map.layers.push(MapLayer {
      info,
      kind: LayerKind::Image(ImageLayer {
        image: Some(backdrop),
        repeat_x: true,
        repeat_y: true,
        transparent: None,
      }),
    });
    // On every grid the tile layers collide and cast shadows
    for name in ["back", "ground", "props", "water"] {
      map.add_tile_layer(name);
    }
    let side = SIDE as i32;
    for y in 0..side {
      for x in 0..side {
        let cell = IVec2::new(x, y);
        let flip = match (x + 2 * y) % 3 {
          1 if hex => TileFlip::HEX_ROTATE_60,
          2 if hex => TileFlip::HEX_ROTATE_120,
          _ => TileFlip::NONE,
        };
        let mut put = |layer: usize, id: u32| {
          map
            .tiles_mut(layer)
            .expect("a tile layer")
            .set(cell, Some(Tile::new(set, id).with_flip(flip)));
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
fn engine(scene: StaticMap, at: Vec2) -> Option<HeadlessEngine> {
  let builder = EngineBuilder::new().resolution(640, 360).start_scene(scene);
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
  static_frames_allocate_nothing(Art::Fitting);
}

#[test]
fn a_frame_drawing_an_isometric_map_of_tall_tiles_in_tileds_order_allocates_nothing() {
  static_frames_allocate_nothing(Art::Tall);
}

#[test]
fn a_frame_drawing_a_staggered_map_of_wide_tiles_run_by_run_allocates_nothing() {
  static_frames_allocate_nothing(Art::Wide);
}

#[test]
fn a_frame_drawing_a_y_sorted_oblique_map_cell_by_cell_allocates_nothing() {
  static_frames_allocate_nothing(Art::Sheared);
}

#[test]
fn a_frame_drawing_a_hexagonal_map_of_turned_tiles_allocates_nothing() {
  static_frames_allocate_nothing(Art::Turned);
}

/// How many colliders `engine`'s tile grids built: the children of the
/// entities holding a `TileColliders`
fn tile_colliders(engine: &mut HeadlessEngine) -> usize {
  let world = engine.world_mut();
  let grids: Vec<Entity> = world
    .query_filtered::<Entity, With<TileColliders>>()
    .iter(world)
    .collect();
  world
    .query::<(&ChildOf, &Collider)>()
    .iter(world)
    .filter(|(child_of, _)| grids.contains(&child_of.parent()))
    .count()
}

/// How many occluders `engine`'s maps keep from their tiles
fn map_occluders(engine: &mut HeadlessEngine) -> usize {
  let world = engine.world_mut();
  world.query::<&MapOccluder>().iter(world).count()
}

/// The most draws a chunk's tileset takes in `engine`'s last frame, over
/// its plain tile layers: one where each part draws whole, more where a
/// layer draws its chunks' runs band by band
fn draws_a_part(engine: &mut HeadlessEngine) -> usize {
  let world = engine.world_mut();
  let y_sorted: Vec<Entity> = world
    .query::<(Entity, &LayerInfo)>()
    .iter(world)
    .filter(|(_, info)| info.y_sorted)
    .map(|(entity, _)| entity)
    .collect();
  let mut parts: BTreeMap<String, usize> = BTreeMap::new();
  for (layer, chunk, tileset, _, _) in engine.tile_draws() {
    if !y_sorted.contains(&layer) {
      *parts
        .entry(format!("{layer:?} {chunk} {tileset}"))
        .or_insert(0) += 1;
    }
  }
  parts.values().copied().max().unwrap_or(0)
}

/// A frame drawing the static map of `art`, still and panning over chunks
/// it has drawn, makes no allocation of the engine's own an empty frame
/// does not, and its own path none inside the GPU API
fn static_frames_allocate_nothing(art: Art) {
  let centre = StaticMap { empty: true, art }.centre();
  let (Some(mut empty), Some(mut map)) = (
    engine(StaticMap { empty: true, art }, centre),
    engine(StaticMap { empty: false, art }, centre),
  ) else {
    return;
  };
  // The path drawing it: a part a draw while its tiles fit or stand tall
  // on an isometric grid, its runs band by band while they reach across
  // on a staggered or hexagonal one
  // The map collides and casts shadows on every grid, the hexagonal one
  // too: its frame steps its colliders and casts its shadows as well as
  // drawing them
  assert!(
    tile_colliders(&mut map) > 0,
    "{art:?}: the map built colliders"
  );
  assert!(
    map_occluders(&mut map) > 0,
    "{art:?}: the map built its shadows"
  );
  let most = draws_a_part(&mut map);
  match art {
    Art::Fitting | Art::Tall => assert_eq!(most, 1, "{art:?}: each part draws whole"),
    Art::Wide | Art::Turned => {
      assert!(most > 1, "{art:?}: a part draws as runs, {most} at most")
    }
    Art::Sheared => {}
  }
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
      "{art:?}, frame {frame} at {at}: the map's frame {} allocations ({} the engine's own), an \
       empty frame {} ({} its own); {} tile draws, at most {} a part",
      drawn.count,
      own(&drawn),
      nothing.count,
      own(&nothing),
      map.tile_draws().len(),
      draws_a_part(&mut map),
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

/// A scene streaming an endless map round its camera (or nothing,
/// `empty`): one unbounded layer, each chunk a floor along its first row
/// that collides and casts shadows, and a coin, a tile of a class the root
/// registers, so the class tiles follow the chunks. On orthogonal cells of
/// 16 px, or isometric ones of 16 x 8, where the stream takes its chunks
/// through their boxes and the 16 px tiles stand taller than their cells
struct StreamedMap {
  empty: bool,
  isometric: bool,
}

impl Scene for StreamedMap {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    if self.empty {
      return;
    }
    let mut set = atlas(&mut ctx.world.resource_mut::<Assets<Image>>(), 16, 4, 1);
    // A whole cell: the tile's square, or the diamond of the isometric cell
    // under its bottom half
    let floor = if self.isometric {
      vec![
        Vec2::new(8.0, 8.0),
        Vec2::new(16.0, 12.0),
        Vec2::new(8.0, 16.0),
        Vec2::new(0.0, 12.0),
      ]
    } else {
      vec![
        Vec2::ZERO,
        Vec2::new(16.0, 0.0),
        Vec2::new(16.0, 16.0),
        Vec2::new(0.0, 16.0),
      ]
    };
    set.insert_tile(
      GROUND,
      TileData {
        shapes: vec![TileShape {
          points: floor,
          ..TileShape::default()
        }],
        ..TileData::default()
      },
    );
    set.insert_tile(
      PROP,
      TileData {
        class: Some("coin".to_string()),
        ..TileData::default()
      },
    );
    let set = TilesetSource::Asset(ctx.assets().add(set));
    let mut map = if self.isometric {
      TileMap::infinite(UVec2::new(16, 8)).with_shape(GridShape::Isometric)
    } else {
      TileMap::infinite(UVec2::splat(16))
    };
    let set = map.add_tileset(set);
    map.add_tile_layer("ground");
    let handle = ctx.assets().add(map);
    let stream = MapStream::new(move |_| {
      let mut tiles = ChunkTiles::default();
      for x in 0..32 {
        tiles.set("ground", UVec2::new(x, 0), Some(Tile::new(set, GROUND)));
      }
      tiles.set("ground", UVec2::new(3, 3), Some(Tile::new(set, PROP)));
      tiles
    });
    let root = MapRoot::new(handle).with_entity_tiles(&["coin"]);
    ctx.world.spawn((Transform::IDENTITY, root, stream));
  }
}

/// An engine on the streamed map (or nothing), its camera at the origin,
/// every chunk round the view landed and warmed up. None without a GPU
fn streamed_engine(empty: bool, isometric: bool) -> Option<HeadlessEngine> {
  let builder = EngineBuilder::new()
    .resolution(640, 360)
    .start_scene(StreamedMap { empty, isometric });
  let mut engine = HeadlessEngine::new_or_skip(builder)?;
  look_at(&mut engine, Vec2::ZERO);
  // The 640 x 360 view grown by the stream's 512 px on every side
  let wanted = Rect::new(-832.0, -692.0, 1664.0, 1384.0);
  let start = std::time::Instant::now();
  loop {
    engine.step().expect("a frame");
    let world = engine.world_mut();
    let roots: Vec<Entity> = world
      .query_filtered::<Entity, With<MapRoot>>()
      .iter(world)
      .collect();
    let view = MapsView::new(world);
    if roots.iter().all(|&root| view.stream_ready(root, wanted)) {
      break;
    }
    assert!(
      start.elapsed() < std::time::Duration::from_secs(10),
      "the chunks round the view landed"
    );
  }
  engine.step_frames(30).expect("the warm-up frames");
  if !empty {
    let world = engine.world_mut();
    let coins = world.query::<&MapTile>().iter(world).count();
    let chunks = world
      .query::<&TileLayerData>()
      .iter(world)
      .map(|data| data.tiles.chunk_coords().count())
      .sum::<usize>();
    if isometric {
      assert!(
        chunks > 0 && coins == chunks,
        "a coin in each of the {chunks} chunks round the view, not {coins}"
      );
    } else {
      assert_eq!(
        coins, 16,
        "a coin in each of the 4 x 4 chunks round the view"
      );
    }
  }
  Some(engine)
}

#[test]
fn an_idle_frame_of_a_streamed_map_allocates_nothing() {
  idle_streamed_frames_allocate_nothing(false);
}

#[test]
fn an_idle_frame_of_an_isometric_streamed_map_allocates_nothing() {
  idle_streamed_frames_allocate_nothing(true);
}

/// An idle frame of the streamed map (`isometric` or not), its chunks
/// landed round a still camera, makes no allocation of the engine's own an
/// empty frame does not, and its own path none inside the GPU API: the
/// stream's look over its areas included, which on isometric cells takes
/// the chunks through their boxes
fn idle_streamed_frames_allocate_nothing(isometric: bool) {
  let (Some(mut empty), Some(mut streamed)) = (
    streamed_engine(true, isometric),
    streamed_engine(false, isometric),
  ) else {
    return;
  };
  for frame in 0..4 {
    let (drawn, nothing) = (traced(&mut streamed), traced(&mut empty));
    let own = |frame: &Frame| frame.own.values().sum::<usize>();
    println!(
      "isometric {isometric}, frame {frame}: the streamed map's frame {} allocations ({} the \
       engine's own), an empty frame {} ({} its own)",
      drawn.count,
      own(&drawn),
      nothing.count,
      own(&nothing),
    );
    let more = more(&drawn, &nothing);
    assert!(
      more.is_empty(),
      "frame {frame}: the streamed map's idle frame made allocations of the engine's own an \
       empty frame did not: {more:#?}"
    );
    assert!(
      drawn.map_gpu.is_empty(),
      "frame {frame}: the map's path made GPU allocations: {:#?}",
      drawn.map_gpu
    );
  }
}

/// A scene of characters walking on a map's storeys under a shadowed
/// light: a map of 64 x 64 cells of 16 px whose "upper" layer is on floor
/// 1, 32 px up, holding a y-sorted block, and twelve characters, eight on
/// floor 1 and four on the ground floor, each a y-sorted sprite with a blob
/// shadow, a child sprite and a grandchild sprite, walking every step while
/// `walking` is set
struct FloorWalk {
  walking: Arc<AtomicBool>,
  walkers: Vec<Entity>,
  steps: u32,
}

impl Scene for FloorWalk {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    ctx.world.spawn((
      Transform::from_xy(512.0, 512.0),
      Light2D::point(Color::WHITE, 240.0).with_shadows(),
    ));
    let set = atlas(&mut ctx.world.resource_mut::<Assets<Image>>(), 16, 4, 1);
    let set = TilesetSource::Asset(ctx.assets().add(set));
    let mut map = TileMap::new(UVec2::splat(64), UVec2::splat(16));
    let set = map.add_tileset(set);
    let ground = map.add_tile_layer("ground");
    let upper = map.add_tile_layer("upper");
    map.layers[upper].info.floor = 1;
    map.layers[upper].info.offset = Vec2::new(0.0, -32.0);
    map.layers[upper].info.y_sorted = true;
    for y in 0..64 {
      for x in 0..64 {
        let cell = IVec2::new(x, y);
        let tiles = map.tiles_mut(ground).expect("a tile layer");
        tiles.set(cell, Some(Tile::new(set, BACK)));
        if (24..40).contains(&x) && (24..40).contains(&y) {
          let tiles = map.tiles_mut(upper).expect("a tile layer");
          tiles.set(cell, Some(Tile::new(set, PROP)));
        }
      }
    }
    let handle = ctx.assets().add(map);
    ctx
      .world
      .spawn((Transform::IDENTITY, MapRoot::new(handle).with_z(2)));
    let (body, hat) = {
      let mut images = ctx.world.resource_mut::<Assets<Image>>();
      (
        images.add(Image::solid_rect(12, 16, 200, 80, 40, 255)),
        images.add(Image::solid_rect(8, 4, 40, 80, 200, 255)),
      )
    };
    for i in 0..12 {
      let floor = if i < 8 { Floor::On(1) } else { Floor::GROUND };
      let at = Vec2::new(400.0 + 16.0 * i as f32, 420.0 + 12.0 * i as f32);
      let walker = ctx
        .world
        .spawn((
          Transform::from_translation(at),
          Renderable::image(body.clone())
            .with_anchor(Anchor::BottomCenter)
            .with_z_order(3)
            .with_blob_shadow(BlobShadow::circle(6.0)),
          YSort::new(),
          floor,
        ))
        .id();
      let head = ctx.world.spawn_child(
        walker,
        (
          Transform::from_xy(0.0, -16.0),
          Renderable::image(hat.clone()).with_z_order(3),
        ),
      );
      ctx.world.spawn_child(
        head,
        (
          Transform::from_xy(2.0, -2.0),
          Renderable::image(hat.clone()).with_z_order(3),
        ),
      );
      self.walkers.push(walker);
    }
  }

  fn update(&mut self, ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    if !self.walking.load(Ordering::Relaxed) {
      return SceneAction::Continue;
    }
    // Back and forth along x, 64 steps each way, over the map's cells
    self.steps += 1;
    let t = (self.steps % 128) as f32;
    let dx = if t < 64.0 { t } else { 128.0 - t };
    for (i, &walker) in self.walkers.iter().enumerate() {
      if let Some(mut transform) = ctx.world.get_mut::<Transform>(walker) {
        transform.translation.x = 400.0 + 16.0 * i as f32 + dx;
      }
    }
    SceneAction::Continue
  }
}

#[test]
fn a_frame_with_entities_moving_on_floors_allocates_nothing() {
  let walking = Arc::new(AtomicBool::new(true));
  let builder = EngineBuilder::new()
    .resolution(640, 360)
    .start_scene(FloorWalk {
      walking: walking.clone(),
      walkers: Vec::new(),
      steps: 0,
    });
  let Some(mut engine) = HeadlessEngine::new_or_skip(builder) else {
    return;
  };
  look_at(&mut engine, Vec2::new(512.0, 512.0));
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
  engine.step_frames(30).expect("the warm-up frames, walking");

  // The walkers on floor 1, and what they hold, draw lifted
  let world = engine.world_mut();
  let floored: Vec<Entity> = world
    .query_filtered::<(Entity, &Floor), With<Renderable>>()
    .iter(world)
    .filter(|(_, floor)| **floor == Floor::On(1))
    .map(|(entity, _)| entity)
    .collect();
  assert_eq!(floored.len(), 8, "eight walkers on floor 1");
  let mut lifted = floored.clone();
  for _ in 0..2 {
    let below: Vec<Entity> = world
      .query::<(Entity, &ChildOf)>()
      .iter(world)
      .filter(|(entity, child_of)| lifted.contains(&child_of.parent()) && !lifted.contains(entity))
      .map(|(entity, _)| entity)
      .collect();
    lifted.extend(below);
  }
  assert_eq!(lifted.len(), 24, "each with a child and a grandchild");
  let view = MapsView::new(world);
  for &entity in &lifted {
    assert_eq!(view.drawn_lift(entity), Vec2::new(0.0, -32.0), "{entity}");
  }

  // Each frame the walkers move (their lifts found again on the map under
  // them, down their hierarchies, and drawn there) against one where they
  // stand still, in the same world: the frames draw the same entities
  // through the same queries, so any allocation the moving frame makes and
  // the still one does not is the walk's. (A frame drawing sprites makes
  // allocations an empty frame does not, finding their archetypes afresh
  // in the entity query, floors or not)
  let walker = floored[0];
  let at = |engine: &mut HeadlessEngine| {
    let world = engine.world_mut();
    world
      .get::<GlobalTransform>(walker)
      .map(GlobalTransform::translation)
  };
  for frame in 0..6 {
    walking.store(true, Ordering::Relaxed);
    let before = at(&mut engine);
    let moving = traced(&mut engine);
    assert_ne!(at(&mut engine), before, "frame {frame}: the walkers moved");
    walking.store(false, Ordering::Relaxed);
    let still = traced(&mut engine);
    let own = |frame: &Frame| frame.own.values().sum::<usize>();
    println!(
      "frame {frame}: walking on floors {} allocations ({} the engine's own), standing still {} \
       ({} its own)",
      moving.count,
      own(&moving),
      still.count,
      own(&still),
    );
    let more = more(&moving, &still);
    assert!(
      more.is_empty(),
      "frame {frame}: walking on floors made allocations of the engine's own standing still did \
       not: {more:#?}"
    );
    assert!(
      moving.map_gpu.is_empty(),
      "frame {frame}: the map's path made GPU allocations: {:#?}",
      moving.map_gpu
    );
  }
}
