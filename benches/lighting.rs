//! Today's shadows over the 1024 x 1024 cave map `benches/tilemap.rs` builds
//! (2,378 occluder chains), measured with a GPU (`HeadlessEngine`, 1920 x
//! 1080, at zoom 1, ambient night).
//!
//! Each case is a median frame time in microseconds over 120 frames after 30
//! of warm-up. A frame is the whole step (every phase, the render step and
//! its submit) and the wait for the GPU to finish it, so work done on the
//! GPU is in the figure. Every case runs in its own engine, one after the
//! other, and a case's figure is printed as `name: X µs`:
//! - `one_light_over_cave`: the frame with one shadowed point light of 300 px
//!   following the camera as it flies a loop over the map, minus the same
//!   frame with no light (both are printed, as `cave_unlit` and
//!   `cave_one_light`);
//! - `sixty_four_lights_quarter_visible` (and `_lights`, minus the same
//!   scene with no lights, printed as `cave_still_unlit`): 64 shadowed
//!   lights of 200 px on a fixed grid of 8 x 8 (480 x 270 px apart) over
//!   the middle of the map, the still camera seeing 16 of them;
//! - `two_hundred_fifty_six_lights_in_view`: 256 shadowed lights of 120 px
//!   on a grid of 16 x 16 inside the still camera's view.
//!
//! All the bench lights are hard (no `with_soft_shadows`), as they were when
//! the Before figures were taken, so the guards compare like with like.
//!
//! The cases are guarded (see the constants): a miss prints its figures and
//! the run exits non-zero once every case has run.
//!
//! Measure on a quiet machine and record `uptime` with every run: the
//! figures move with the load.

mod common;
mod guards;

use std::time::{Duration, Instant};

use common::{COLUMNS, atlas_pixels, cave_cells, tileset};
use ryo2d::prelude::*;
use ryo2d::testing::{HeadlessEngine, shared_gpu};

const RESOLUTION: (u32, u32) = (1920, 1080);
/// The cave's side in tiles, and a tile's side in pixels
const SIDE: u32 = 1024;
const TILE: u32 = 16;
const WARMUP: usize = 30;
const FRAMES: usize = 120;
/// The camera's loop over the map: its radius and its step in radians a frame
const REACH: f32 = 1500.0;
const ARC: f32 = 0.005;

// The guards' constants come from the Before table in `docs/backlog/shadows.md`
// (the CPU shadow volumes; three runs at a one-minute load of 3.74, flagged).
/// `one_light_over_cave` is no slower than Before's slowest run (211.2 µs)
const ONE_LIGHT_MAX: f64 = 211.2;
/// `sixty_four_lights_quarter_visible_lights`, the frame with the lights over
/// the same frame without them, is at least 4x cheaper than Before's: Before's
/// median of runs less its no-light cave's median (1,306.9 - 590.9 = 716.0
/// µs), a quarter of it
const SIXTY_FOUR_MAX: f64 = (1306.9 - 590.9) / 4.0;
/// `two_hundred_fifty_six_lights_in_view` is under the spec's 16,600 µs
const MANY_MAX: f64 = 16_600.0;
/// ... and no slower than Before's slowest run (2,684.2 µs, same table)
const MANY_BEFORE_MAX: f64 = 2684.2;

/// What a case's scene holds
#[derive(Clone, Copy)]
enum Case {
  Unlit,
  /// No lights, the camera standing still as in `Sixty4`
  Empty,
  One,
  Sixty4,
  Many,
}

impl Case {
  /// Whether the camera flies (else it stands at the map's middle)
  fn flies(self) -> bool {
    matches!(self, Case::Unlit | Case::One)
  }
}

/// A light that follows the camera
#[derive(Component)]
struct Torch;

struct CaveScene {
  case: Case,
}

/// `columns` x `rows` shadowed lights of `radius` px, `step` apart, centred
/// on the map's middle
fn light_grid(ctx: &mut SceneContext, columns: u32, rows: u32, step: Vec2, radius: f32) {
  let middle = Vec2::splat((SIDE * TILE) as f32 / 2.0);
  let first = middle - Vec2::new((columns - 1) as f32 * step.x, (rows - 1) as f32 * step.y) / 2.0;
  for row in 0..rows {
    for column in 0..columns {
      let at = first + Vec2::new(column as f32 * step.x, row as f32 * step.y);
      ctx.world.spawn((
        Transform::from_xy(at.x, at.y),
        Light2D::point(Color::WHITE, radius).with_shadows(),
      ));
    }
  }
}

impl Scene for CaveScene {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    ctx.lighting().set_ambient(AmbientLight::night());
    match self.case {
      Case::Unlit | Case::Empty => {}
      Case::One => {
        ctx.world.spawn((
          Transform::IDENTITY,
          Light2D::point(Color::WHITE, 300.0).with_shadows(),
          Torch,
        ));
      }
      Case::Sixty4 => light_grid(ctx, 8, 8, Vec2::new(480.0, 270.0), 200.0),
      Case::Many => light_grid(ctx, 16, 16, Vec2::new(120.0, 67.5), 120.0),
    }
    let tile = UVec2::splat(TILE);
    let size = tile * COLUMNS;
    let rock = vec![
      Vec2::ZERO,
      Vec2::new(TILE as f32, 0.0),
      Vec2::new(TILE as f32, TILE as f32),
      Vec2::new(0.0, TILE as f32),
    ];
    let image = ctx
      .assets()
      .add(Image::from_rgba(atlas_pixels(tile), size.x, size.y));
    let set = TilesetSource::Asset(ctx.assets().add(tileset(image, tile, Some(rock), None)));
    let mut map = TileMap::new(UVec2::splat(SIDE), tile).with_shape(GridShape::Orthogonal);
    let set = map.add_tileset(set);
    let names = ["background", "ground", "props", "front"];
    for (name, cells) in names.iter().zip(cave_cells(SIDE)) {
      let layer = map.add_tile_layer(name);
      let Some(tiles) = map.tiles_mut(layer) else {
        continue;
      };
      for (i, id) in cells.iter().enumerate() {
        if let Some(id) = id {
          let cell = IVec2::new((i as u32 % SIDE) as i32, (i as u32 / SIDE) as i32);
          tiles.set(cell, Some(Tile::new(set, *id)));
        }
      }
    }
    let root = MapRoot::new(ctx.assets().add(map));
    ctx.world.spawn((Transform::IDENTITY, root));
  }
}

/// Where the flying camera is at `frame`
fn flown(frame: usize) -> Vec2 {
  let middle = (SIDE * TILE) as f32 / 2.0;
  let path = frame as f32 * ARC;
  Vec2::new(
    middle + REACH * path.sin(),
    middle + REACH * (path * 1.37 + 1.0).sin(),
  )
}

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

fn landed(engine: &mut HeadlessEngine) -> bool {
  let world = engine.world_mut();
  let roots: Vec<Entity> = world
    .query_filtered::<Entity, With<MapRoot>>()
    .iter(world)
    .collect();
  let view = MapsView::new(world);
  roots.iter().all(|&root| view.is_ready(root))
}

/// A step and the wait for the GPU to finish it
fn frame(engine: &mut HeadlessEngine, device: &wgpu::Device) -> Duration {
  let start = Instant::now();
  engine.step().expect("a frame");
  device
    .poll(wgpu::PollType::wait_indefinitely())
    .expect("the GPU finishes its work");
  start.elapsed()
}

/// The median frame of `case`, in microseconds; none without a GPU
fn measure(case: Case) -> Option<f64> {
  let builder = EngineBuilder::new()
    .resolution(RESOLUTION.0, RESOLUTION.1)
    .start_scene(CaveScene { case });
  let mut engine = HeadlessEngine::new_or_skip(builder)?;
  let (device, _) = shared_gpu().ok()?;
  let at = |frame: usize| {
    if case.flies() {
      flown(frame)
    } else {
      Vec2::splat((SIDE * TILE) as f32 / 2.0)
    }
  };
  place_camera(&mut engine, at(0));
  let start = Instant::now();
  while !landed(&mut engine) {
    engine.step().expect("a frame");
    assert!(start.elapsed() < Duration::from_secs(60), "the map landed");
  }
  for i in 0..WARMUP {
    place_camera(&mut engine, at(i));
    frame(&mut engine, &device);
  }
  let mut times: Vec<Duration> = (0..FRAMES)
    .map(|i| {
      place_camera(&mut engine, at(WARMUP + i));
      frame(&mut engine, &device)
    })
    .collect();
  times.sort();
  Some(times[FRAMES / 2].as_secs_f64() * 1e6)
}

fn main() {
  // Criterion's runner passes flags (`--bench`) that this has no use for;
  // `--list` and `--test` run nothing, and a filter names the cases to run
  if !guards::runs(&[
    "one_light_over_cave",
    "sixty_four_lights_quarter_visible",
    "two_hundred_fifty_six_lights_in_view",
  ]) {
    return;
  }
  let Some(unlit) = measure(Case::Unlit) else {
    println!("no GPU: skipped");
    return;
  };
  let Some(one) = measure(Case::One) else {
    return;
  };
  println!("cave_unlit: {unlit:.1} µs");
  println!("cave_one_light: {one:.1} µs");
  let one_light = one - unlit;
  println!("one_light_over_cave: {one_light:.1} µs");
  held("one_light_over_cave", one_light, &[ONE_LIGHT_MAX]);
  if let (Some(empty), Some(lit)) = (measure(Case::Empty), measure(Case::Sixty4)) {
    println!("cave_still_unlit: {empty:.1} µs");
    println!("sixty_four_lights_quarter_visible: {lit:.1} µs");
    let lights = lit - empty;
    println!("sixty_four_lights_quarter_visible_lights: {lights:.1} µs");
    held(
      "sixty_four_lights_quarter_visible_lights",
      lights,
      &[SIXTY_FOUR_MAX],
    );
  }
  if let Some(time) = measure(Case::Many) {
    let name = "two_hundred_fifty_six_lights_in_view";
    println!("{name}: {time:.1} µs");
    held(name, time, &[MANY_MAX, MANY_BEFORE_MAX]);
  }
  guards::verdict(
    "lighting",
    "Profile a miss with the bench binary `cargo bench --bench lighting --no-run` names.",
  );
  eprintln!("lighting guards: every guard met its target");
}

/// Note a miss when `time` is over any of `limits`
fn held(name: &str, time: f64, limits: &[f64]) {
  for limit in limits {
    if time > *limit {
      guards::miss(format!("{name} {time:.1} µs over {limit:.1} µs"));
    }
  }
}
