//! Allocations of map drawing, counted by a global allocator in this test
//! binary: a padded tileset cache hit; a frame drawing a static map under a
//! shadowed light (its camera still, then panning over chunks it has
//! drawn) on every grid, its layers colliding and casting shadows (the
//! hexagonal map's too), and the same map with a storey under a second
//! light on that storey; an idle frame of a streamed map whose chunks have
//! landed, on orthogonal and on isometric cells; a frame of characters
//! walking on a map's storeys, drawn lifted; and a lit frame of 64
//! shadowed lights, hard and soft, against the same lights unshadowed; and
//! a lit frame of 50 animated skeletons (two tracks, events firing, a
//! socket each, a mix started on some every frame) against the same entities
//! as sprites, of regions alone, with a weighted mesh among them whose
//! shape a deform keys, and with IK and transform constraints applied in
//! every step, their targets moved by the game. For the skeletons the GPU's
//! are compared too, site by site: no call the engine makes into the GPU
//! API may allocate more for the skeletons' frame than for the sprites',
//! but for a small room.
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
use std::f32::consts::{PI, TAU};
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
/// Where the floored static map's light on floor 1 stands from the map's
/// middle
const FLOOR_LIGHT: Vec2 = Vec2::new(48.0, 24.0);

/// A scene with the static map (or nothing, `empty`) under a shadowed
/// light: four tile layers of 128 x 128 cells over an atlas, the third
/// y-sorted; an image layer; an object layer holding a tile object. The
/// light is in both, so each frame is lit and the map's frame differs only
/// by the map (its shadow chains among them). Its tiles fit their cells
/// (`Art::Fitting`), or its plain layers draw in Tiled's order across
/// tilesets and chunks (the other `Art`s). With `floored`, a fifth tile
/// layer of blocks is on floor 1, 32 px up, and a second shadowed light
/// is on floor 1 in both
struct StaticMap {
  empty: bool,
  art: Art,
  floored: bool,
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
    if self.floored {
      ctx.world.spawn((
        Transform::from_translation(centre + FLOOR_LIGHT),
        Light2D::point(Color::WHITE, 200.0).with_shadows(),
        Floor::On(1),
      ));
    }
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
    if self.floored {
      let upper = map.add_tile_layer("upper");
      map.layers[upper].info.floor = 1;
      map.layers[upper].info.offset = Vec2::new(0.0, -32.0);
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
        if self.floored && (x / 4 + y / 6) % 5 == 1 {
          put(5, GROUND);
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

/// The entity drawer's functions that draw a skeleton and nothing else
/// (`render/entities.rs`), and the frame's mesh stream, which only a
/// skeleton's meshes join (`render/batch/mesh.rs`): they are outside the
/// skeleton module, and are the skeletons' path all the same
const SKELETON_DRAWER: [&str; 6] = [
  "ryo2d::core::render::entities::collect_skeleton",
  "ryo2d::core::render::entities::Emitter::draw_parts",
  "ryo2d::core::render::entities::Emitter::draw_shaded",
  "ryo2d::core::render::entities::Emitter::draw_skinned",
  "ryo2d::core::render::entities::Emitter::draw_mesh",
  "ryo2d::core::render::batch::mesh::",
];

/// One frame's allocations: how many; of those not made inside the GPU API,
/// how many were made where (the nearest engine function and the one it
/// called); and those made inside the GPU API whose nearest engine function
/// is the map's own (the map module or its drawer)
struct Frame {
  count: usize,
  own: BTreeMap<String, usize>,
  map_gpu: Vec<String>,
  /// The same for the skeletons' own path (the skeleton module, and the
  /// entity drawer's skeleton functions, [`SKELETON_DRAWER`])
  skeleton_gpu: Vec<String>,
  /// Those not made inside the GPU API with a function of the skeletons'
  /// path on their stack, by the function nearest the allocation
  in_skeleton: Vec<String>,
  /// Those made inside the GPU API from the lighting's own path (its
  /// nearest engine function in `lighting::`), by that function and the
  /// GPU API's one it called
  lighting_gpu: BTreeMap<String, usize>,
  /// Those made inside `Queue::write_buffer` or `Queue::submit`: wgpu's
  /// staging for a write reuses a buffer, and a submission reclaims what
  /// earlier ones held, only once the GPU is done with them, so their
  /// count moves with the GPU's timing
  timed: usize,
  /// Those made inside the GPU API, by where the engine called into it:
  /// the GPU API's function and the engine's that called it
  gpu: BTreeMap<String, usize>,
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
  traced_after(engine, |_| {})
}

/// A step of `engine` after `calls` made on it, the allocations of both
/// traced
fn traced_after(engine: &mut HeadlessEngine, calls: impl FnOnce(&mut HeadlessEngine)) -> Frame {
  TRACES.with(|t| t.borrow_mut().clear());
  let before = allocations();
  TRACING.with(|c| c.set(true));
  calls(engine);
  engine.step().expect("a frame");
  TRACING.with(|c| c.set(false));
  let count = allocations() - before;
  let traces = TRACES.with(|t| std::mem::take(&mut *t.borrow_mut()));
  let (mut own, mut map_gpu, mut lighting_gpu) = (BTreeMap::new(), Vec::new(), BTreeMap::new());
  let (mut skeleton_gpu, mut in_skeleton) = (Vec::new(), Vec::new());
  let mut gpu = BTreeMap::new();
  // A function of the skeletons' own path: one of the skeleton module (a
  // type that names it in its parameters is not one), or one of the
  // entity drawer's that only a skeleton's drawing reaches. A method's
  // name is its type's in angle brackets and then its own
  // (`<..::Emitter>::draw_parts::<..>`), so the brackets are taken out
  // before a drawer's path is looked for at its start
  let skeleton_fn = |name: &&&str| {
    name.starts_with("ryo2d::core::skeleton::")
      || name.starts_with("<ryo2d::core::skeleton::")
      || SKELETON_DRAWER
        .iter()
        .any(|drawer| name.replace(['<', '>'], "").starts_with(drawer))
  };
  let mut timed = 0;
  for trace in traces {
    let text = trace.to_string();
    let frames = functions(&text);
    let ours = frames.iter().position(|f| f.contains("ryo2d::"));
    if GPU.iter().any(|gpu| text.contains(gpu)) {
      if text.contains("Queue>::write_buffer") || text.contains("Queue>::submit") {
        timed += 1;
      }
      let site = match ours {
        Some(at) => format!("{} <- {}", frames[at.saturating_sub(1)], frames[at]),
        None => frames.first().copied().unwrap_or("?").to_string(),
      };
      *gpu.entry(site).or_insert(0) += 1;
      if let Some(at) = ours
        && (frames[at].contains("tilemap::") || frames[at].contains("render::tiles"))
      {
        map_gpu.push(frames[at].to_string());
      }
      if let Some(name) = frames.iter().find(skeleton_fn) {
        skeleton_gpu.push((*name).to_string());
      }
      if let Some(at) = ours
        && frames[at].contains("ryo2d::core::lighting::")
      {
        let site = format!("{} <- {}", frames[at.saturating_sub(1)], frames[at]);
        *lighting_gpu.entry(site).or_insert(0) += 1;
      }
      continue;
    }
    if let Some(name) = frames.iter().find(skeleton_fn) {
      in_skeleton.push((*name).to_string());
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
    skeleton_gpu,
    in_skeleton,
    lighting_gpu,
    timed,
    gpu,
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

/// The GPU API's call that checks and encodes what a frame's passes
/// recorded. What it allocates is the GPU API's own handling of those
/// commands (wgpu's Metal backend copies a pipeline's vertex buffer layouts
/// each time a pass sets it, for one): no code of the engine's runs there,
/// so nothing the engine makes for a drawable can hide in it. A test that
/// leaves this site out lets growth in wgpu's own encoding through: if a
/// later wgpu allocates more there for each pipeline change or draw, that
/// test will not say so
const ENCODES: &str = "<wgpu::api::command_encoder::CommandEncoder>::finish <- ";

/// How many more allocations a call of the engine's into the GPU API may
/// make for one frame than for another before it counts. A buffer's write
/// that the other frame lacks, a longer list of a pass's commands and what
/// moves with the GPU's timing are a few: the most seen at one site on a
/// quiet machine is thirteen (the encoder's finish for the lit frame's
/// shadow pass in a debug build), and beside other processes using the
/// GPU a frame's whole count moved by four either way. Measured on Metal
/// only: on another backend a site's own difference may be more, and the
/// message names the site and both counts. A buffer, a write or a bind
/// group for each of fifty drawables is fifty at the least
const GPU_ROOM: usize = 24;

/// The sites where `drawn` makes more allocations inside the GPU API than
/// `nothing` makes there and [`GPU_ROOM`] more, a site being the GPU API's
/// function and the engine's that called it; not a site that begins with
/// one of `apart` (the encoder's finish, [`ENCODES`], for a frame whose
/// drawables cost the GPU API something there each). So a GPU resource
/// made or written for each drawable is told wherever the engine does it:
/// laying out the frame, planning its draws, or recording a pass
fn more_gpu(drawn: &Frame, nothing: &Frame, apart: &[&str]) -> Vec<String> {
  let other = |site: &String| nothing.gpu.get(site).copied().unwrap_or(0);
  let compared = |site: &String| !apart.iter().any(|left| site.starts_with(left));
  drawn
    .gpu
    .iter()
    .filter(|(site, n)| compared(site) && other(site) + GPU_ROOM < **n)
    .map(|(site, n)| format!("{n} (the other frame {}) at {site}", other(site)))
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
  static_frames_allocate_nothing(Art::Fitting, false);
}

#[test]
fn a_lit_frame_drawing_a_static_map_with_floors_allocates_nothing() {
  static_frames_allocate_nothing(Art::Fitting, true);
}

#[test]
fn a_frame_drawing_an_isometric_map_of_tall_tiles_in_tileds_order_allocates_nothing() {
  static_frames_allocate_nothing(Art::Tall, false);
}

#[test]
fn a_frame_drawing_a_staggered_map_of_wide_tiles_run_by_run_allocates_nothing() {
  static_frames_allocate_nothing(Art::Wide, false);
}

#[test]
fn a_frame_drawing_a_y_sorted_oblique_map_cell_by_cell_allocates_nothing() {
  static_frames_allocate_nothing(Art::Sheared, false);
}

#[test]
fn a_frame_drawing_a_hexagonal_map_of_turned_tiles_allocates_nothing() {
  static_frames_allocate_nothing(Art::Turned, false);
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

/// A frame drawing the static map of `art` (`floored`, with a storey and a
/// light on it), still and panning over chunks it has drawn, makes no
/// allocation of the engine's own an empty frame does not, and its own
/// path none inside the GPU API
fn static_frames_allocate_nothing(art: Art, floored: bool) {
  let scene = |empty| StaticMap {
    empty,
    art,
    floored,
  };
  let centre = scene(true).centre();
  let (Some(mut empty), Some(mut map)) =
    (engine(scene(true), centre), engine(scene(false), centre))
  else {
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
  if floored {
    // Shadows on both floors, and a light on floor 1 drawn lifted
    let world = map.world_mut();
    let floors: Vec<Option<Floor>> = world
      .query::<(&MapOccluder, Option<&Floor>)>()
      .iter(world)
      .map(|(_, floor)| floor.copied())
      .collect();
    assert!(floors.contains(&None) && floors.contains(&Some(Floor::On(1))));
    let lights: Vec<Entity> = world
      .query_filtered::<Entity, (With<Light2D>, With<Floor>)>()
      .iter(world)
      .collect();
    let view = MapsView::new(world);
    assert_eq!(lights.len(), 1);
    assert_eq!(view.drawn_lift(lights[0]), Vec2::new(0.0, -32.0));
  }
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
      "{art:?}{}, frame {frame} at {at}: the map's frame {} allocations ({} the engine's own), an \
       empty frame {} ({} its own); {} tile draws, at most {} a part",
      if floored { " with floors" } else { "" },
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
/// `walking` is set. Two stairs on the ground lead into the block's left
/// side at row 32 (their foot at x 352, their top at x 384), and a climber
/// with a child sprite walks their lower half, on the ground floor and
/// drawn eased part-way up
struct FloorWalk {
  walking: Arc<AtomicBool>,
  walkers: Vec<Entity>,
  climber: Option<Entity>,
  steps: u32,
}

/// The ground tile `FloorWalk`'s tileset makes stairs up to floor 1
const STAIRS: u32 = GROUND;
/// The row of `FloorWalk`'s stairs, and where its climber stands on it
const STAIR_ROW: i32 = 32;
const CLIMBER_Y: f32 = 16.0 * STAIR_ROW as f32 + 8.0;

impl Scene for FloorWalk {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    ctx.world.spawn((
      Transform::from_xy(512.0, 512.0),
      Light2D::point(Color::WHITE, 240.0).with_shadows(),
    ));
    let mut set = atlas(&mut ctx.world.resource_mut::<Assets<Image>>(), 16, 4, 1);
    let stairs = TileData {
      stairs: Some(1),
      ..TileData::default()
    };
    set.insert_tile(STAIRS, stairs);
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
    let tiles = map.tiles_mut(ground).expect("a tile layer");
    for x in [22, 23] {
      tiles.set(IVec2::new(x, STAIR_ROW), Some(Tile::new(set, STAIRS)));
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
    let climber = ctx
      .world
      .spawn((
        Transform::from_xy(356.0, CLIMBER_Y),
        Renderable::image(body.clone())
          .with_anchor(Anchor::BottomCenter)
          .with_z_order(3),
        YSort::new(),
        Climber,
      ))
      .id();
    ctx.world.spawn_child(
      climber,
      (
        Transform::from_xy(0.0, -16.0),
        Renderable::image(hat.clone()).with_z_order(3),
      ),
    );
    self.climber = Some(climber);
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
    // The climber between 356 and 364, an eighth to three eighths up
    // the stairs: never halfway, so it keeps the ground floor
    if let Some(climber) = self.climber
      && let Some(mut transform) = ctx.world.get_mut::<Transform>(climber)
    {
      transform.translation.x = 356.0 + dx / 8.0;
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
      climber: None,
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

  // The climber, on the ground floor, draws eased part-way up its stairs
  let climber = world
    .query_filtered::<Entity, With<Climber>>()
    .single(world)
    .expect("one climber");
  let eased = |engine: &mut HeadlessEngine| {
    let world = engine.world_mut();
    let floor = world.get::<Floor>(climber).copied();
    let lift = MapsView::new(world).drawn_lift(climber);
    floor == Some(Floor::GROUND) && lift.x == 0.0 && lift.y < -1.0 && lift.y > -13.0
  };
  assert!(eased(&mut engine), "the climber draws part-way up");

  // Each frame the walkers move (their lifts found again on the map under
  // them, down their hierarchies, and drawn there) against one where they
  // stand still, in the same world: the frames draw the same entities
  // through the same queries, so any allocation the moving frame makes and
  // the still one does not is the walk's. (A frame drawing sprites makes
  // allocations an empty frame does not, finding their archetypes afresh
  // in the entity query, floors or not.) The climber looks its stairs up
  // in both frames, so neither may allocate in the stairs or the lift at
  // all
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
      eased(&mut engine),
      "frame {frame}: the climber draws part-way up"
    );
    let storeys: Vec<&String> = moving
      .own
      .keys()
      .chain(still.own.keys())
      .filter(|site| site.contains("tilemap::stairs") || site.contains("tilemap::floors"))
      .collect();
    assert!(
      storeys.is_empty(),
      "frame {frame}: the stairs or the lift allocated: {storeys:#?}"
    );
    assert!(
      moving.map_gpu.is_empty(),
      "frame {frame}: the map's path made GPU allocations: {:#?}",
      moving.map_gpu
    );
  }
}

/// 64 lights over 640 x 360 px in an 8 x 8 grid, and (`shadowed`) each
/// casting shadows, every other one soft, of a wall beside each: closed
/// blocks, two of them touching (a shared side), and an open chain. The
/// first light is `Swaying`, so a frame can change the plan
struct LitRoom {
  shadowed: bool,
}

impl Scene for LitRoom {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    let floor = ctx
      .assets()
      .add(Image::solid_rect(640, 360, 255, 255, 255, 255));
    ctx.world.spawn((
      Transform::from_xy(0.0, 0.0),
      Renderable::image(floor).with_anchor(Anchor::TopLeft),
    ));
    ctx
      .lighting()
      .set_ambient(AmbientLight::with_intensity(0.1));
    ctx.lighting().set_enabled(true);
    for i in 0..64 {
      let at = Vec2::new(40.0 + (i % 8) as f32 * 80.0, 20.0 + (i / 8) as f32 * 45.0);
      let light = Light2D::point(Color::WHITE, 120.0).with_intensity(0.3);
      let light = match (self.shadowed, i % 2) {
        (false, _) => light,
        (true, 0) => light.with_shadows(),
        (true, _) => light.with_soft_shadows(6.0),
      };
      let light = ctx
        .world
        .spawn((Transform::from_translation(at), light))
        .id();
      if i == 0 {
        ctx.world.entity_mut(light).insert(Swaying);
      }
    }
    if !self.shadowed {
      return;
    }
    for i in 0..24 {
      let at = Vec2::new(70.0 + (i % 6) as f32 * 100.0, 40.0 + (i / 6) as f32 * 90.0);
      ctx.world.spawn((
        Transform::from_translation(at),
        LightOccluder2D::rect(12.0, 24.0),
      ));
    }
    ctx.world.spawn((
      Transform::from_xy(82.0, 40.0),
      LightOccluder2D::rect(12.0, 24.0),
    ));
    ctx.world.spawn((
      Transform::from_xy(320.0, 300.0),
      LightOccluder2D::chain(vec![
        Vec2::new(-100.0, 0.0),
        Vec2::new(0.0, -30.0),
        Vec2::new(100.0, 0.0),
      ]),
    ));
  }

  fn update(&mut self, _ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    SceneAction::Continue
  }
}

/// The light a `LitRoom` frame moves
#[derive(Component)]
struct Swaying;

/// Moves the swaying light to one of its two places by `frame`'s parity:
/// the shadow plan changes each frame, so each frame draws the atlas (a
/// frame whose plan did not change keeps it), and the warm-up has seen
/// both plans
fn sway(engine: &mut HeadlessEngine, frame: usize) {
  let world = engine.world_mut();
  let mut lights = world.query_filtered::<&mut Transform, With<Swaying>>();
  for mut transform in lights.iter_mut(world) {
    transform.translation.x = 40.0 + (frame % 2) as f32 * 3.0;
  }
}

#[test]
fn a_lit_frame_with_64_shadowed_lights_allocates_nothing() {
  let start = |shadowed| {
    let builder = EngineBuilder::new()
      .resolution(640, 360)
      .start_scene(LitRoom { shadowed });
    let mut engine = HeadlessEngine::new_or_skip(builder)?;
    for frame in 0..30 {
      sway(&mut engine, frame);
      engine.step().expect("a warm-up frame");
    }
    Some(engine)
  };
  let (Some(mut shadowed), Some(mut plain)) = (start(true), start(false)) else {
    return;
  };
  // The shadows' atlas was made in the warm-up, and no frame makes it again
  let builds = shadowed.shadow_atlas_builds();
  assert!(builds > 0, "the shadowed lights have an atlas");
  // The same lights, unshadowed and with nothing to cast, are the
  // baseline: any allocation the shadowed frame makes and that one does
  // not is the shadows' (the plan, the atlas and its soft filter's uniform)
  for frame in 0..7 {
    sway(&mut shadowed, frame);
    sway(&mut plain, frame);
    let (cast, open) = (traced(&mut shadowed), traced(&mut plain));
    let own = |frame: &Frame| frame.own.values().sum::<usize>();
    println!(
      "frame {frame}: 64 shadowed lights {} allocations ({} the engine's own), unshadowed {} \
       ({} its own); {} and {} of them wgpu's staging and submission; the lighting's inside the GPU API {:#?}",
      cast.count,
      own(&cast),
      open.count,
      own(&open),
      cast.timed,
      open.timed,
      cast.lighting_gpu,
    );
    let more = more(&cast, &open);
    assert!(
      more.is_empty(),
      "frame {frame}: the shadowed frame made allocations of the engine's own the unshadowed one \
       did not: {more:#?}"
    );
    let lighting: Vec<&String> = cast
      .own
      .keys()
      .filter(|site| site.contains("lighting::"))
      .collect();
    assert!(
      lighting.is_empty(),
      "frame {frame}: the lighting allocated: {lighting:#?}"
    );
    assert_eq!(
      shadowed.shadow_atlas_builds(),
      builds,
      "frame {frame}: the atlas was made again"
    );
    // The lighting's GPU allocations come from recording its passes: the
    // atlas's (its edges written, its pass begun, drawn and ended) and the
    // compose pass both frames make (where wgpu's own vectors grow inside
    // it moves with what was recorded before it). None from a GPU
    // resource or bind group made
    let unexpected: Vec<&String> = cast
      .lighting_gpu
      .keys()
      .filter(|site| {
        let atlas = site.ends_with("ShadowAtlas>::encode")
          && ATLAS_CALLS.iter().any(|call| site.starts_with(call));
        let compose = site.ends_with("LightingRenderer>::compose_targets")
          || site.ends_with("lighting::renderer::copy_scene");
        !atlas && !compose
      })
      .collect();
    assert!(
      unexpected.is_empty(),
      "frame {frame}: the lighting made GPU allocations elsewhere: {unexpected:#?}"
    );
    assert!(
      cast
        .lighting_gpu
        .keys()
        .any(|site| site.ends_with("ShadowAtlas>::encode")),
      "frame {frame}: the atlas was drawn: {:#?}",
      cast.lighting_gpu
    );
    // The same work every frame: what the shadows cost inside the GPU API
    // is the atlas's one pass (its edges written, the pass begun, its
    // pipeline set, and wgpu tracking it as the frame's commands finish),
    // a few allocations at each of those sites and none for each light.
    // Compared with the unshadowed frame site by site, the encoder's
    // finish among them: the whole count moves by a few either way with
    // the GPU's timing when other processes use the GPU (an exact count
    // read 7, 9 and 15 for 11, and failed 15 runs of 40 beside three of
    // them), and no site moves by the room; something made or written for
    // each of the 64 lights is 64 at one site at the least
    let settled = |frame: &Frame| (frame.count - frame.timed) as isize;
    let extra = settled(&cast) - settled(&open);
    println!("frame {frame}: {extra} more of the GPU API's than the unshadowed frame, untimed");
    let more_gpu = more_gpu(&cast, &open, &[]);
    assert!(
      more_gpu.is_empty(),
      "frame {frame}: the shadowed frame made over {GPU_ROOM} more allocations inside the GPU API \
       than the unshadowed one, where the engine called into it: {more_gpu:#?}"
    );
  }
}

/// The GPU API's calls the atlas's draw may allocate inside
const ATLAS_CALLS: [&str; 7] = [
  "<wgpu::api::queue::Queue>::write_buffer",
  "<wgpu::api::command_encoder::CommandEncoder>::begin_render_pass",
  "<wgpu::api::render_pass::RenderPass>::set_pipeline",
  "<wgpu::api::render_pass::RenderPass>::set_bind_group",
  "<wgpu::api::render_pass::RenderPass>::set_vertex_buffer",
  "<wgpu::api::render_pass::RenderPass>::draw",
  "core::ptr::drop_glue::<wgpu::api::render_pass::RenderPass>",
];

/// How many skeletons the crowd holds
const CROWD: usize = 50;
/// The rows of the crowd's mesh: two vertices a row, 60 in all
const RIBBON_ROWS: usize = 30;
/// The region slot the mesh's slot follows: a skeleton draws five regions,
/// the mesh, and five regions more
const RIBBON_AFTER: usize = 4;

/// A weighted mesh of 60 vertices: a ribbon along the chain's first ten
/// bones, each vertex held by the two bones it lies between, so it has 120
/// positions for a deform to key
fn ribbon() -> MeshAttachment {
  let vertices = (0..RIBBON_ROWS * 2).map(|vertex| {
    let (row, side) = (vertex / 2, vertex % 2);
    let along = row as f32 * 9.0 / RIBBON_ROWS as f32;
    let (bone, part) = (along as u8, along.fract());
    let y = if side == 0 { -4.0 } else { 4.0 };
    let weight = 0.9 - 0.8 * part;
    vec![
      SkeletonInfluence::new(BoneId(bone), part * 5.0, y, weight),
      SkeletonInfluence::new(BoneId(bone + 1), (part - 1.0) * 5.0, y, 1.0 - weight),
    ]
  });
  let uvs = (0..RIBBON_ROWS * 2)
    .map(|vertex| {
      Vec2::new(
        (vertex % 2) as f32,
        (vertex / 2) as f32 / (RIBBON_ROWS - 1) as f32,
      )
    })
    .collect();
  let triangles = (0..RIBBON_ROWS as u16 - 1)
    .flat_map(|row| {
      let first = row * 2;
      [first, first + 1, first + 2, first + 1, first + 3, first + 2]
    })
    .collect();
  let vertices = MeshVertices::Weighted(WeightedVertices::new(vertices));
  MeshAttachment::new(0, vertices, uvs, triangles)
}

/// What the crowd's rig has beside its regions
#[derive(Clone, Copy, PartialEq)]
enum Extra {
  None,
  /// A weighted mesh whose shape a deform keys ([`ribbon`])
  Mesh,
  /// Constraints of every kind ([`CONSTRAINED`])
  Constraints,
}

/// The constrained rig's bones beside its chain, each with its parent, its
/// place in it, its turn and its length: two limbs of two bones (each 6
/// and 5 long), a chain of four (four parts of 5, bent 0.3 rad a joint),
/// the three bones their IK constraints reach for, and a bone for each mode
/// of transform constraint. Every one hangs from the root, at its origin
/// unless placed
const CONSTRAINED: [(&str, &str, f32, f32, f32); 15] = [
  ("arm_a", "b0", 0.0, 0.5, 6.0),
  ("hand_a", "arm_a", 6.0, 0.6, 5.0),
  ("arm_b", "b0", 0.0, -0.5, 6.0),
  ("hand_b", "arm_b", 6.0, 0.6, 5.0),
  ("tail_1", "b0", 0.0, 2.0, 5.0),
  ("tail_2", "tail_1", 5.0, 0.3, 5.0),
  ("tail_3", "tail_2", 5.0, 0.3, 5.0),
  ("tail_4", "tail_3", 5.0, 0.3, 5.0),
  ("goal_a", "b0", 8.0, 0.0, 0.0),
  ("goal_b", "b0", 8.0, 0.0, 0.0),
  ("goal_tail", "b0", 14.0, 0.0, 0.0),
  ("copy_world", "b0", 3.0, 0.0, 4.0),
  ("copy_world_relative", "b0", 3.0, 0.0, 4.0),
  ("copy_local", "b0", 3.0, 0.0, 4.0),
  ("copy_local_relative", "b0", 3.0, 0.0, 4.0),
];

/// Where the constrained crowd's skeleton `index` puts its targets in
/// `frame`, from its entity's origin (the limbs' and the chain's first
/// joint): within the limbs' reach (4 to 9 of their 11) and between the
/// chain's nearest shape and its reach (11 to 16 of its 20), moving every
/// frame
fn goals(index: usize, frame: usize) -> [Vec2; 3] {
  let t = (frame + 7 * index) as f32 * 0.05;
  [
    Vec2::from_angle(t) * (6.5 + 2.5 * (0.7 * t).sin()),
    Vec2::from_angle(-t + 1.0) * (6.5 + 2.5 * (0.9 * t).cos()),
    Vec2::from_angle(0.5 * t + 2.0) * (13.5 + 2.5 * (1.3 * t).sin()),
  ]
}

/// A rig of 12 bones in a chain and 10 region slots, a step event, and
/// the animations the crowd mixes between. With [`Extra::Mesh`], a slot
/// among the regions shows the [`ribbon`], and both of the base track's
/// animations key its shape (every position of it, by a different amount
/// each), so a deform plays in every step and two are blended through
/// every crossfade. With [`Extra::Constraints`], the bones of
/// [`CONSTRAINED`] and, in order, an IK constraint on each limb (the first
/// one's mix keyed by both of the base track's animations), one on the
/// chain, and a transform constraint of each mode
fn crowd_rig(image: Handle<Image>, extra: Extra) -> SkeletonData {
  let mesh = extra == Extra::Mesh;
  let turn = |bone: u8, keys: &[(f32, f32)]| SkeletonTimeline::Rotate {
    bone: BoneId(bone),
    frames: keys
      .iter()
      .map(|&(time, radians)| (time, radians, SkeletonCurve::Linear))
      .collect(),
  };
  // The ribbon's shape: an offset for each of its positions, x then y
  let shape = |amount: f32| -> Vec<f32> {
    (0..RIBBON_ROWS * 4)
      .flat_map(|position| [0.0, amount * ((position % 7) as f32 - 3.0) / 3.0])
      .collect()
  };
  let sway = |amount: f32| SkeletonTimeline::Deform {
    slot: SlotId(RIBBON_AFTER as u8 + 1),
    attachment: Arc::from("ribbon"),
    frames: [(0.0, -amount), (0.25, amount), (0.5, -amount)]
      .into_iter()
      .map(|(time, amount)| (time, shape(amount), SkeletonCurve::Linear))
      .collect(),
  };
  let clip = |name: &str, bones: u8, swing: f32, events: &[f32], deform: Option<f32>| {
    let mut timelines: Vec<SkeletonTimeline> = (0..bones)
      .map(|bone| turn(bone, &[(0.0, -swing), (0.25, swing), (0.5, -swing)]))
      .collect();
    if let Some(amount) = deform.filter(|_| mesh) {
      timelines.push(sway(amount));
    }
    // The first limb's mix, keyed by the animations a deform keys
    if extra == Extra::Constraints && deform.is_some() {
      let mix = |mix: f32| IkValues {
        mix,
        ..IkValues::of(&IkSetup::default())
      };
      timelines.push(SkeletonTimeline::Ik {
        constraint: SkeletonConstraintId(0),
        frames: vec![
          (0.0, mix(1.0), SkeletonCurve::Linear),
          (0.25, mix(0.4), SkeletonCurve::Linear),
          (0.5, mix(1.0), SkeletonCurve::Linear),
        ],
      });
    }
    if !events.is_empty() {
      timelines.push(SkeletonTimeline::Event {
        frames: events
          .iter()
          .map(|&time| (time, 0, SkeletonEventValues::default()))
          .collect(),
      });
    }
    SkeletonClip {
      name: Arc::from(name),
      duration: 0.5,
      timelines,
    }
  };
  let mut builder = SkeletonData::builder().bone("b0", None, BoneSetup::default());
  for bone in 1..12 {
    let setup = BoneSetup {
      x: 5.0,
      length: 5.0,
      ..BoneSetup::default()
    };
    builder = builder.bone(format!("b{bone}"), Some(&format!("b{}", bone - 1)), setup);
  }
  for part in 0..10usize {
    let (slot, name) = (format!("s{part}"), format!("p{part}"));
    let setup = SlotSetup {
      attachment: Some(Arc::from(name.as_str())),
      ..SlotSetup::default()
    };
    builder = builder
      .slot(slot.as_str(), &format!("b{part}"), setup)
      .region(SkeletonRegion {
        name: Arc::from(name.as_str()),
        image: image.clone(),
        x: part as u32 * 8,
        y: 0,
        width: 8,
        height: 8,
      })
      .attachment(
        SkeletonData::DEFAULT_SKIN,
        &slot,
        name.as_str(),
        SkeletonAttachment::Region(RegionAttachment::new(part, 8.0, 8.0)),
      );
    if mesh && part == RIBBON_AFTER {
      let setup = SlotSetup {
        attachment: Some(Arc::from("ribbon")),
        ..SlotSetup::default()
      };
      builder = builder.slot("ribbon", "b0", setup).attachment(
        SkeletonData::DEFAULT_SKIN,
        "ribbon",
        "ribbon",
        SkeletonAttachment::Mesh(ribbon()),
      );
    }
  }
  if extra == Extra::Constraints {
    for (name, parent, x, rotation, length) in CONSTRAINED {
      let setup = BoneSetup {
        x,
        rotation,
        length,
        ..BoneSetup::default()
      };
      builder = builder.bone(name, Some(parent), setup);
    }
    let copy = |local, relative, mix| TransformConstraintSetup {
      mix,
      local,
      relative,
      ..TransformConstraintSetup::default()
    };
    let half = TransformConstraintMix {
      rotate: 0.5,
      x: 0.5,
      y: 0.5,
      ..TransformConstraintMix::default()
    };
    builder = builder
      .ik("limb_a", &["arm_a", "hand_a"], "goal_a", IkSetup::default())
      .ik(
        "limb_b",
        &["arm_b", "hand_b"],
        "goal_b",
        IkSetup {
          bend_positive: false,
          ..IkSetup::default()
        },
      )
      .ik(
        "tail",
        &["tail_1", "tail_2", "tail_3", "tail_4"],
        "goal_tail",
        IkSetup::default(),
      )
      .transform_constraint(
        "world",
        &["copy_world"],
        "hand_b",
        copy(false, false, TransformConstraintMix::ALL),
      )
      .transform_constraint(
        "world_relative",
        &["copy_world_relative"],
        "tail_2",
        copy(false, true, half),
      )
      .transform_constraint(
        "local",
        &["copy_local"],
        "hand_a",
        copy(true, false, TransformConstraintMix::ALL),
      )
      .transform_constraint(
        "local_relative",
        &["copy_local_relative"],
        "tail_3",
        copy(true, true, half),
      );
  }
  builder
    .event(SkeletonEventData::new("step"))
    .animation(clip("idle", 12, 0.2, &[0.0, 0.2, 0.4], Some(1.0)))
    .animation(clip("walk", 12, 0.4, &[0.1, 0.3], Some(2.0)))
    .animation(clip("wave", 4, 0.6, &[0.25], None))
    .default_mix(0.4)
    .build()
    .expect("a valid rig")
}

/// `count` skeletons in a lit room, each with a socket that has a child; or
/// (`skeletons` false) the same entities as plain sprites, each with a
/// child that has a child: the baseline, which makes the allocations the
/// entity drawer and the engine's queries make for any entities. Each
/// skeleton's rig has the `extra` of [`crowd_rig`]
struct Crowd {
  count: usize,
  skeletons: bool,
  extra: Extra,
}

impl Scene for Crowd {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    ctx.camera().set_position(320.0, 180.0);
    ctx
      .lighting()
      .set_ambient(AmbientLight::with_intensity(0.2));
    ctx.lighting().set_enabled(true);
    ctx.world.spawn((
      Transform::from_xy(320.0, 180.0),
      Light2D::point(Color::WHITE, 500.0),
    ));
    let atlas = ctx
      .assets()
      .add(Image::solid_rect(80, 8, 255, 255, 255, 255));
    let data = ctx.assets().add(crowd_rig(atlas.clone(), self.extra));
    for i in 0..self.count {
      let at = Vec2::new(40.0 + (i % 10) as f32 * 60.0, 40.0 + (i / 10) as f32 * 60.0);
      let carrier = ctx
        .world
        .spawn((Transform::from_translation(at), Interpolated))
        .id();
      let socket = ctx
        .world
        .spawn((Transform::IDENTITY, ChildOf(carrier)))
        .id();
      if self.skeletons {
        ctx.world.entity_mut(carrier).insert((
          Skeleton::new(data.clone()),
          SkeletonAnimation::new(),
          Renderable::skeleton(),
        ));
        ctx.world.entity_mut(socket).insert(BoneSocket::new("b11"));
      } else {
        ctx
          .world
          .entity_mut(carrier)
          .insert(Renderable::image(atlas.clone()));
      }
      ctx
        .world
        .spawn((Transform::from_xy(3.0, 0.0), ChildOf(socket)));
    }
  }
}

/// Starts a mix on the skeleton `index` in frame `frame`: each skeleton makes
/// a call every third frame, and each of the four kinds (a track's
/// animation set, one queued, the second track's layer set, and that layer
/// faded out) every twelfth
fn mix(animation: &mut SkeletonAnimation, index: usize, frame: usize) {
  let k = (frame + index) % 12;
  if !k.is_multiple_of(3) {
    return;
  }
  let round = (frame + index) / 12;
  let name = ["idle", "walk"][round % 2];
  match k / 3 {
    0 => {
      animation.set_animation(0, name, true);
    }
    1 => {
      animation.add_animation(0, ["walk", "idle"][round % 2], true, 0.0);
    }
    2 => {
      animation.set_animation(1, "wave", true);
    }
    _ => animation.set_empty(1, 0.3),
  }
}

#[test]
fn fifty_animated_skeletons_allocate_nothing_per_frame() {
  skeletons_allocate_nothing(Extra::None);
}

/// The same crowd with a weighted mesh of 60 vertices among each skeleton's
/// regions, its shape keyed by the animations: skinned and deformed in
/// every frame, an item of its own between two runs of quads
#[test]
fn fifty_skeletons_with_a_deforming_mesh_allocate_nothing_per_frame() {
  skeletons_allocate_nothing(Extra::Mesh);
}

/// The same crowd with constraints of every kind applied in every step
/// ([`CONSTRAINED`]): two two-bone IK constraints, a chain of four bones and
/// a transform constraint in each of the four modes, the first limb's mix
/// keyed and blended through the crossfades (and overridden in every other
/// skeleton), an override on two of the constraints changed every 30
/// frames, and the three targets moved by world-space bone overrides every
/// frame
#[test]
fn fifty_skeletons_with_constraints_allocate_nothing_per_frame() {
  skeletons_allocate_nothing(Extra::Constraints);
}

/// What the game does to the constrained crowd's skeleton `index` before
/// `frame`: puts its three targets where [`goals`] says, in the world, and
/// every 30 frames changes its overrides of the first limb (its bend, and
/// in an odd skeleton its mix, which an even one's keys set) and of the
/// world-mode transform constraint (its rotate mix)
fn steer(skeleton: &mut Skeleton, at: Vec2, index: usize, frame: usize) {
  for (bone, goal) in ["goal_a", "goal_b", "goal_tail"]
    .into_iter()
    .zip(goals(index, frame))
  {
    let put = BoneOverride {
      translation: Some(at + goal),
      space: BoneSpace::World,
      ..BoneOverride::default()
    };
    skeleton.override_bone(bone, put);
  }
  if (frame + index).is_multiple_of(30) {
    let odd = (frame + index) / 30 % 2 == 1;
    // An odd skeleton's override sets the first limb's mix; an even one's
    // leaves it to the keys
    let mix = if odd { 0.5 } else { 0.9 };
    let limb = IkConstraintOverride {
      mix: (index % 2 == 1).then_some(mix),
      bend_positive: Some(odd),
      ..IkConstraintOverride::default()
    };
    skeleton.override_constraint("limb_a", SkeletonConstraintOverride::Ik(limb));
    let copy = TransformConstraintOverride {
      rotate: Some(if odd { 0.25 } else { 0.75 }),
      ..TransformConstraintOverride::default()
    };
    skeleton.override_constraint("world", SkeletonConstraintOverride::Transform(copy));
  }
}

/// Asserts that the constraints act in every constrained skeleton as the
/// crowd's last step left it: the second limb's hand and the chain's tip
/// are on their targets, where the game's overrides put them in that step;
/// each mode of transform constraint has put its bone where its rule says;
/// and the first limb's keyed mix, which only odd skeletons override,
/// holds the even ones' hands short of their targets
fn constraints_act(crowd: &mut HeadlessEngine, frame: usize, when: &str) {
  let world = crowd.world_mut();
  let mut skeletons = world.query::<(&Skeleton, &Transform)>();
  let (mut acted, mut keyed, mut short) = (0, 0, 0);
  for (index, (skeleton, transform)) in skeletons.iter(world).enumerate() {
    let at = transform.translation;
    let bone = |name: &str| {
      skeleton
        .bone_world(name)
        .unwrap_or_else(|| panic!("{when}: skeleton {index} has no {name}"))
    };
    let close = |what: &str, actual: Vec2, expected: Vec2| {
      assert!(
        actual.distance(expected) < 0.01,
        "{when}: skeleton {index}'s {what} is at {actual}, not {expected}"
      );
    };
    // World: the place copied fully (its rotate mix is overridden)
    close(
      "copy_world",
      bone("copy_world").translation,
      bone("hand_b").translation,
    );
    // World, relative: half the target's place in the skeleton's space
    // added to its own, and half the target's turn to its own
    let (b0, tail_2) = (bone("b0"), bone("tail_2"));
    let relative = bone("copy_world_relative");
    let own = b0.transform_point2(Vec2::new(3.0, 0.0));
    close(
      "copy_world_relative",
      relative.translation,
      own + (tail_2.translation - at) * 0.5,
    );
    let turned = relative.matrix2.x_axis.to_angle()
      - b0.matrix2.x_axis.to_angle()
      - tail_2.matrix2.x_axis.to_angle() * 0.5;
    let turned = (turned + PI).rem_euclid(TAU) - PI;
    assert!(
      turned.abs() < 0.001,
      "{when}: skeleton {index}'s copy_world_relative is turned {turned} from its rule"
    );
    // Local: the target's own values, under its own parent
    let hand_a_own = bone("arm_a").inverse() * bone("hand_a");
    let local = b0 * hand_a_own;
    let copied = bone("copy_local");
    close("copy_local", copied.translation, local.translation);
    close(
      "copy_local's x axis",
      copied.matrix2.x_axis,
      local.matrix2.x_axis,
    );
    // Local, relative: half the target's own place (5 along its parent)
    // added to its own (3 along the root)
    let tail_3_own = (tail_2.inverse() * bone("tail_3")).translation;
    close(
      "copy_local_relative",
      bone("copy_local_relative").translation,
      b0.transform_point2(Vec2::new(3.0, 0.0) + tail_3_own * 0.5),
    );
    if index % 2 == 0 {
      keyed += 1;
      let reached = bone("hand_a").transform_point2(Vec2::new(5.0, 0.0));
      if reached.distance(bone("goal_a").translation) > 0.01 {
        short += 1;
      }
    }
    let [_, limb, tail] = goals(index, frame);
    for (tip, length, goal, target) in [
      ("hand_b", 5.0, limb, "goal_b"),
      ("tail_4", 5.0, tail, "goal_tail"),
    ] {
      let reached = skeleton
        .bone_world(tip)
        .map(|bone| bone.transform_point2(Vec2::new(length, 0.0)));
      let target_at = skeleton.bone_world(target).map(|bone| bone.translation);
      let (Some(reached), Some(target_at)) = (reached, target_at) else {
        panic!("{when}: skeleton {index} has no {tip} or {target}");
      };
      assert!(
        target_at.distance(at + goal) < 0.01,
        "{when}: skeleton {index}'s {target} is at {target_at}, not {}",
        at + goal
      );
      assert!(
        reached.distance(target_at) < 0.01,
        "{when}: skeleton {index}'s {tip} reaches {reached}, its target is at {target_at}"
      );
    }
    acted += 1;
  }
  assert_eq!(acted, CROWD, "{when}: {acted} constrained skeletons");
  // The keys move the mix between 0.4 and 1: at the setup's 1 every hand
  // would be on its target
  assert!(
    short * 2 > keyed,
    "{when}: {short} of {keyed} hands held short by their keyed mix"
  );
}

/// How far the vertices of the skeleton's [`ribbon`] are from where its
/// bones alone put them (the setup shape, skinned), at most: what its
/// deform moves it by. None for a skeleton that shows no mesh
fn ribbon_moved(world: &World, entity: Entity, skeleton: &Skeleton) -> Option<f32> {
  let drawn = ryo2d::testing::skeleton_mesh(world, entity, "ribbon");
  let MeshVertices::Weighted(setup) = ribbon().vertices else {
    return None;
  };
  if drawn.len() != setup.vertex_count() {
    return None;
  }
  let mut moved = 0.0_f32;
  for (vertex, at) in drawn.iter().enumerate() {
    let mut plain = Vec2::ZERO;
    for influence in setup.influences(vertex) {
      let bone = skeleton.bone_world(&format!("b{}", influence.bone.0))?;
      plain += bone.transform_point2(Vec2::new(influence.x, influence.y)) * influence.weight;
    }
    moved = moved.max(at.distance(plain));
  }
  Some(moved)
}

/// The allocations of each kind, counted by name
fn counted(names: &[String]) -> BTreeMap<&str, usize> {
  let mut counts = BTreeMap::new();
  for name in names {
    *counts.entry(name.as_str()).or_insert(0) += 1;
  }
  counts
}

/// Fifty skeletons, of regions alone or with the `extra` of [`crowd_rig`],
/// allocate nothing in a steady frame that the same entities drawn as
/// sprites do not
fn skeletons_allocate_nothing(extra: Extra) {
  let (mesh, constrained) = (extra == Extra::Mesh, extra == Extra::Constraints);
  let start = |skeletons| {
    let builder = EngineBuilder::new()
      .resolution(640, 360)
      .start_scene(Crowd {
        count: CROWD,
        skeletons,
        extra,
      });
    HeadlessEngine::new_or_skip(builder)
  };
  let (Some(mut crowd), Some(mut empty)) = (start(true), start(false)) else {
    return;
  };
  let mut animations = crowd
    .world_mut()
    .query::<(&mut SkeletonAnimation, &mut Skeleton, &Transform)>();
  let mut steady = |engine: &mut HeadlessEngine, frame: usize| {
    let world = engine.world_mut();
    for (index, (mut animation, mut skeleton, transform)) in animations.iter_mut(world).enumerate()
    {
      mix(&mut animation, index, frame);
      if constrained {
        steer(&mut skeleton, transform.translation, index, frame);
      }
    }
  };
  // Warm: every kind of call has run on every skeleton, the data has
  // landed, and the buffers have the sizes a mix needs
  let warm = 240;
  for frame in 0..warm {
    steady(&mut crowd, frame);
    crowd.step().expect("a warm-up frame");
    empty.step().expect("a warm-up frame");
  }
  assert!(
    crowd
      .world_mut()
      .query::<&Skeleton>()
      .iter(crowd.world())
      .all(|skeleton| skeleton.bone_world("b11").is_some()),
    "every skeleton has its pose"
  );
  if mesh {
    // The case is the one it says: every skeleton's mesh is drawn, a draw
    // of its own between two of quads, and its deform is moving it
    let draws = crowd.debug_stats().draw_calls;
    assert!(
      draws > 2 * CROWD as u32,
      "{draws} draws: {CROWD} meshes among {} runs of quads",
      CROWD + 1
    );
    deform_plays(&mut crowd, "before the traced frames");
  }
  if constrained {
    constraints_act(&mut crowd, warm - 1, "before the traced frames");
  }
  for frame in warm..warm + 12 {
    let drawn = traced_after(&mut crowd, |engine| steady(engine, frame));
    let nothing = traced(&mut empty);
    let own = |frame: &Frame| frame.own.values().sum::<usize>();
    // Inside the GPU API: all of them, and those the encoder's finish made
    let inside = |frame: &Frame| frame.count - own(frame);
    let encoded = |frame: &Frame| {
      let finish = frame
        .gpu
        .iter()
        .filter(|(site, _)| site.starts_with(ENCODES));
      finish.map(|(_, n)| n).sum::<usize>()
    };
    println!(
      "frame {frame}: {CROWD} skeletons {} allocations ({} the engine's own), as sprites {} ({} its \
       own); inside the GPU API {} ({} as the encoder finished), the sprites' {} ({}); the skeletons' \
       own path made {} of them",
      drawn.count,
      own(&drawn),
      nothing.count,
      own(&nothing),
      inside(&drawn),
      encoded(&drawn),
      inside(&nothing),
      encoded(&nothing),
      drawn.skeleton_gpu.len(),
    );
    // All four are told together, so a failure names the site, the
    // function of the skeletons' path nearest it, and the GPU's share
    let (more, more_gpu) = (
      more(&drawn, &nothing),
      more_gpu(&drawn, &nothing, &[ENCODES]),
    );
    let clean = more.is_empty() && drawn.in_skeleton.is_empty() && drawn.skeleton_gpu.is_empty();
    assert!(
      clean && more_gpu.is_empty(),
      "frame {frame}: the skeletons' frame allocated.\nOf the engine's own, more than the sprites' \
       frame makes: {more:#?}\nWith a function of the skeletons' path on the stack, by the nearest \
       one: {:#?}\nInside the GPU API from the skeletons' path: {:#?}\nInside the GPU API, where \
       the engine called into it, over {GPU_ROOM} more than the sprites' frame makes there: \
       {more_gpu:#?}",
      counted(&drawn.in_skeleton),
      counted(&drawn.skeleton_gpu),
    );
    // And the frame traced applied them: the step in it reached the
    // targets the game moved before it
    if constrained {
      constraints_act(&mut crowd, frame, &format!("frame {frame}"));
    }
  }
  if mesh {
    // And it was so to the end: the frames traced drew a deforming mesh
    deform_plays(&mut crowd, "after the last traced frame");
  }
}

/// Asserts that a deform is moving the mesh of four skeletons in five at
/// the least, as the crowd's last step left them
fn deform_plays(crowd: &mut HeadlessEngine, when: &str) {
  let world = crowd.world_mut();
  let mut skeletons = world.query::<(Entity, &Skeleton)>();
  let moved: Vec<Option<f32>> = skeletons
    .iter(world)
    .map(|(entity, skeleton)| ribbon_moved(world, entity, skeleton))
    .collect();
  let deformed = moved
    .iter()
    .filter(|moved| moved.is_some_and(|by| by > 0.05))
    .count();
  assert!(
    moved.len() == CROWD && deformed * 5 >= CROWD * 4,
    "{when}, {deformed} of {} meshes are moved by their deform: {moved:?}",
    moved.len()
  );
}
