//! Lines and trails with a GPU (`HeadlessEngine`, 1920 x 1080), unlit, none
//! moving, in two kinds of scene:
//! - `lines`: a thousand in a grid that fills the view, 500 `Line2D`s and
//!   500 `Trail2D`s of five to seven points that turn at every point, of
//!   mixed joins (miter, bevel, round), caps (butt, round, square) and widths
//!   (0.5 to 10 px); the width goes by the shape's index over three, so every
//!   join meets every width;
//! - `lines_only_{join}_{count}`: `Line2D`s alone, 1,000, 10,000 and 100,000
//!   of them, mitered or round, in a grid that fills the view, each the
//!   thousand's shape scaled to its cell, its width scaled with it but never
//!   under a pixel, the caps mixed.
//!
//! A run warms each engine up, then steps frames (so a frame extracts and
//! draws the lines again, as every frame does) and takes the median of each
//! figure, printed as `{scene}_{figure}: X µs`:
//! - `render`: the render step's CPU time as a whole (the lines' points
//!   copied in extract, under the `cpu:render:lines` profile scope, then the
//!   draw: placing, uploading and encoding);
//! - `prepare`: `Batch2D::prepare` within it (the points, codes and line
//!   table uploaded);
//! - `extract`: the rest of the render step (render less prepare), where the
//!   lines' points are copied and the frame is recorded and submitted;
//! - `gpu`: the GPU's time from just before the world pass to just after the
//!   unlit world pass, where every line draws (timestamp queries from two
//!   empty passes round them, where the device has `TIMESTAMP_QUERY`;
//!   native only), and the line corner pass's, which the frame encodes
//!   before its camera passes (stamped at its own start and end,
//!   `HeadlessEngine::stamp_line_corners`). It includes the world passes'
//!   own load and store of the camera's image. It leaves out the gap
//!   between the corner pass's end and the world pass's start, where the
//!   camera's clear runs too: 3 to 6 µs, measured from stamps on the world
//!   pass itself in the frames where those stamps held (on 1,000 lines
//!   they did not in up to 170 of 300).
//!
//! Every frame must have drawn something: a draw call and a prepare time
//! above nothing (a scene that drew no line would measure nothing and look
//! fast). A frame missing a stamp it needs (Metal returns zero now and
//! then, on main as on this code), the world passes' or the corner pass's,
//! is left out of the GPU's median and counted, never taken as no time. A
//! frame whose lines are too few for the corner pass to run
//! (`HeadlessEngine::line_corners_built`) times the world passes alone.
//!
//! Why two intervals, not one from the corner pass's start: Metal's stamps
//! on passes apart from each other do not keep their order. An empty pass's
//! outer stamps fall anywhere in the frame, before the corner pass even,
//! and an interval from the corner pass's start to the world passes' end
//! moved by up to three times from run to run on the small scenes.
//!
//! The figures are recorded against main in `docs/backlog/lines.md` (three
//! alternating runs). The guards are loose: `MAX` is far above the figures,
//! so only a regression of a different order fails the run.

mod guards;

use std::sync::Arc;
use std::time::Duration;

use ryo2d::prelude::*;
use ryo2d::testing::{HeadlessEngine, shared_gpu};

const RESOLUTION: (u32, u32) = (1920, 1080);
/// Lines in the mixed grid, and trails: a thousand together
const LINES: usize = 500;
const TRAILS: usize = 500;
/// The mixed grid's columns, and the cell each shape fills
const COLUMNS: usize = 40;
const CELL: (f32, f32) = (48.0, 43.2);
/// Frames before the measure, and measured
const WARMUP: usize = 60;
const FRAMES: usize = 300;
/// Widths, in pixels
const WIDTHS: [f32; 6] = [0.5, 1.0, 2.0, 3.0, 6.0, 10.0];
/// The lines-only scenes' counts
const COUNTS: [usize; 3] = [1_000, 10_000, 100_000];
/// Timestamps a frame: the world passes' two, then the corner pass's
const STAMPS: u32 = 4;
/// Whether the corner pass is stamped (this code) or, with no corner pass,
/// not (main's copy)
const CORNERS_STAMPED: bool = true;

/// Over which a figure of the mixed scene is a regression, not a measure: a
/// figure of the first runs' is a tenth of it or less. This guards against a
/// change of another order, not against a few per cent.
const EXTRACT_MAX: f64 = 8000.0;
const RENDER_MAX: f64 = 16000.0;

const JOINS: [LineJoin; 3] = [LineJoin::Miter, LineJoin::Bevel, LineJoin::Round];
const CAPS: [LineCap; 3] = [LineCap::Butt, LineCap::Round, LineCap::Square];

/// Where a scene's shapes lie: `columns` across, each in a cell of `cell`
#[derive(Clone, Copy)]
struct Grid {
  columns: usize,
  cell: Vec2,
}

impl Grid {
  /// A grid of `count` cells that fills the view
  fn filling(count: usize) -> Self {
    let (width, height) = (RESOLUTION.0 as f32, RESOLUTION.1 as f32);
    let columns = ((count as f32 * width / height).sqrt().ceil() as usize).max(1);
    let rows = count.div_ceil(columns);
    Self {
      columns,
      cell: Vec2::new(width / columns as f32, height / rows as f32),
    }
  }

  /// The centre of cell `index`
  fn centre(&self, index: usize) -> Vec2 {
    let (column, row) = (index % self.columns, index / self.columns);
    Vec2::new(
      (column as f32 + 0.5) * self.cell.x,
      (row as f32 + 0.5) * self.cell.y,
    )
  }

  /// How much smaller than the thousand's a shape is
  fn scale(&self) -> f32 {
    (self.cell.x / CELL.0).min(self.cell.y / CELL.1)
  }
}

/// The points of shape `index` about `centre`, scaled by `scale`: five to
/// seven, turning sharply and gently by turns, within its cell
fn points(index: usize, centre: Vec2, scale: f32) -> Vec<Vec2> {
  let count = 5 + index % 3;
  (0..count)
    .map(|i| {
      let along = i as f32 / (count - 1) as f32;
      let turn = if (i + index).is_multiple_of(2) {
        1.0
      } else {
        -1.0
      };
      let reach = 9.0 + (index % 5) as f32;
      centre
        + scale
          * Vec2::new(
            (along - 0.5) * 36.0,
            turn * reach * (0.4 + 0.6 * ((i % 3) as f32 / 2.0)),
          )
    })
    .collect()
}

fn color(index: usize) -> Color {
  Color::new(
    80 + (index * 37 % 170) as u8,
    80 + (index * 91 % 170) as u8,
    80 + (index * 53 % 170) as u8,
    if index.is_multiple_of(4) { 160 } else { 255 },
  )
}

/// The width of shape `index`: by its index over three, so every join (by
/// the index) meets every width
fn width(index: usize) -> f32 {
  WIDTHS[(index / 3) % WIDTHS.len()]
}

/// What a scene holds
#[derive(Clone, Copy)]
enum Shapes {
  /// The thousand lines and trails of mixed joins
  Mixed,
  /// `count` lines of one join
  LinesOnly { join: LineJoin, count: usize },
}

/// The GPU's time over the world passes and the line corner pass: a
/// timestamp before the world pass and one after the unlit world pass, and
/// two at the corner pass's start and end, resolved and read back each
/// frame
#[derive(Clone)]
struct Stamps {
  set: wgpu::QuerySet,
  resolved: wgpu::Buffer,
  read: wgpu::Buffer,
}

impl Stamps {
  /// `None` where the device cannot take timestamps
  fn new(device: &wgpu::Device) -> Option<Self> {
    if !device.features().contains(wgpu::Features::TIMESTAMP_QUERY) {
      return None;
    }
    let set = device.create_query_set(&wgpu::QuerySetDescriptor {
      label: Some("Line Bench Stamps"),
      ty: wgpu::QueryType::Timestamp,
      count: STAMPS,
    });
    let buffer = |label, usage| {
      device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(label),
        size: 8 * u64::from(STAMPS),
        usage,
        mapped_at_creation: false,
      })
    };
    Some(Self {
      set,
      resolved: buffer(
        "Line Bench Stamps Resolved",
        wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
      ),
      read: buffer(
        "Line Bench Stamps Read",
        wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
      ),
    })
  }

  /// The last frame's GPU time over the world passes and the corner pass,
  /// and the corner pass's apart, once its work is done: `None` when a
  /// stamp either needs is missing
  fn read(
    &self,
    device: &wgpu::Device,
    period: f32,
    corners_ran: bool,
  ) -> Option<(Duration, Duration)> {
    let slice = self.read.slice(..);
    slice.map_async(wgpu::MapMode::Read, |_| {});
    device.poll(wgpu::PollType::wait_indefinitely()).ok()?;
    let ticks: Vec<u64> = {
      let bytes = slice.get_mapped_range().ok()?;
      bytes
        .as_chunks::<8>()
        .0
        .iter()
        .map(|word| u64::from_le_bytes(*word))
        .collect()
    };
    self.read.unmap();
    let time = |start: u64, end: u64| {
      let nanos = end.saturating_sub(start) as f64 * f64::from(period);
      Duration::from_nanos(nanos as u64)
    };
    let &[world_start, world_end, corner_start, corner_end] = ticks.as_slice() else {
      return None;
    };
    let whole = |start: u64, end: u64| start != 0 && end > start;
    if !whole(world_start, world_end) {
      return None;
    }
    let world = time(world_start, world_end);
    if !CORNERS_STAMPED || !corners_ran {
      return Some((world, Duration::ZERO));
    }
    if !whole(corner_start, corner_end) {
      return None;
    }
    let corners = time(corner_start, corner_end);
    Some((world + corners, corners))
  }
}

/// A camera pass writing one timestamp from an empty render pass on the
/// camera's image (Metal writes none from an empty compute pass); the last
/// resolves both and copies them to be read
struct Stamp {
  id: &'static str,
  index: u32,
  stamps: Arc<Stamps>,
}

impl RenderPass for Stamp {
  fn id(&self) -> &str {
    self.id
  }

  fn scope(&self) -> PassScope {
    PassScope::Camera
  }

  fn execute(&mut self, ctx: &mut RenderContext<'_>) {
    let stamps = &self.stamps;
    let (encoder, view) = ctx.encoder_and_current_view();
    drop(encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
      label: Some(self.id),
      color_attachments: &[Some(wgpu::RenderPassColorAttachment {
        view,
        depth_slice: None,
        resolve_target: None,
        ops: wgpu::Operations {
          load: wgpu::LoadOp::Load,
          store: wgpu::StoreOp::Store,
        },
      })],
      depth_stencil_attachment: None,
      timestamp_writes: Some(wgpu::RenderPassTimestampWrites {
        query_set: &stamps.set,
        // At the start pass's end and the end pass's start, so neither's
        // own load and store of the image is counted
        beginning_of_pass_write_index: (self.index == 1).then_some(1),
        end_of_pass_write_index: (self.index == 0).then_some(0),
      }),
      occlusion_query_set: None,
      multiview_mask: None,
    }));
    if self.index == 1 {
      encoder.resolve_query_set(&stamps.set, 0..STAMPS, &stamps.resolved, 0);
      encoder.copy_buffer_to_buffer(&stamps.resolved, 0, &stamps.read, 0, 8 * u64::from(STAMPS));
    }
  }
}

struct Field {
  shapes: Shapes,
  stamps: Option<Arc<Stamps>>,
}

impl Scene for Field {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    ctx
      .camera()
      .set_position(RESOLUTION.0 as f32 / 2.0, RESOLUTION.1 as f32 / 2.0);
    ctx.lighting().set_enabled(false);
    if let Some(stamps) = &self.stamps {
      let mut pipeline = ctx.render_pipeline();
      let stamp = |id, index| Stamp {
        id,
        index,
        stamps: Arc::clone(stamps),
      };
      let placed = pipeline
        .insert_before(pass_ids::WORLD, stamp("bench_world_start", 0))
        .and_then(|()| pipeline.insert_after(pass_ids::UNLIT_WORLD, stamp("bench_world_end", 1)));
      assert!(placed.is_ok(), "the stamps go round the world passes");
    }
    match self.shapes {
      Shapes::Mixed => mixed(ctx),
      Shapes::LinesOnly { join, count } => lines_only(ctx, join, count),
    }
  }
}

/// The thousand lines and trails of mixed joins
fn mixed(ctx: &mut SceneContext) {
  let grid = Grid {
    columns: COLUMNS,
    cell: Vec2::new(CELL.0, CELL.1),
  };
  for index in 0..LINES + TRAILS {
    let centre = grid.centre(index);
    let points = points(index, centre, 1.0);
    let width = width(index);
    let (join, cap) = (JOINS[index % 3], CAPS[(index / 3) % 3]);
    if index < LINES {
      // A line's points are offsets from its entity
      let offsets = points.iter().map(|point| *point - centre).collect();
      ctx.world.spawn((
        Transform::from_xy(centre.x, centre.y),
        Line2D::new(offsets, color(index), width)
          .with_join(join)
          .with_cap(cap),
      ));
    } else {
      // A trail's are where it draws, its head the entity's position,
      // so recording adds none
      let mut trail = Trail2D::new(color(index), width)
        .with_join(join)
        .with_cap(cap)
        .with_end_width(width)
        .with_fade(false)
        .with_length(8);
      for point in points.iter().rev() {
        trail.push_point(point.x, point.y);
      }
      let head = points[0];
      ctx.world.spawn((Transform::from_xy(head.x, head.y), trail));
    }
  }
}

/// `count` lines of `join`, filling the view
fn lines_only(ctx: &mut SceneContext, join: LineJoin, count: usize) {
  let grid = Grid::filling(count);
  let scale = grid.scale();
  for index in 0..count {
    let centre = grid.centre(index);
    let offsets = points(index, Vec2::ZERO, scale);
    let width = (width(index) * scale).max(1.0);
    ctx.world.spawn((
      Transform::from_xy(centre.x, centre.y),
      Line2D::new(offsets, color(index), width)
        .with_join(join)
        .with_cap(CAPS[index % 3]),
    ));
  }
}

fn median(mut times: Vec<Duration>) -> f64 {
  times.sort();
  times
    .get(times.len() / 2)
    .map_or(0.0, |time| time.as_secs_f64() * 1e6)
}

/// Medians in microseconds, and whether every frame did its work
struct Figures {
  render: f64,
  prepare: f64,
  extract: f64,
  /// None where the device takes no timestamps
  gpu: Option<f64>,
  /// Frames that drew nothing
  idle: usize,
  /// Frames the GPU gave no time for
  untimed: usize,
  /// The corner pass's median within `gpu`
  corners: Option<f64>,
}

fn measure(shapes: Shapes) -> Option<Figures> {
  let (device, queue) = shared_gpu().ok()?;
  let stamps = Stamps::new(&device).map(Arc::new);
  let builder = EngineBuilder::new()
    .resolution(RESOLUTION.0, RESOLUTION.1)
    .start_scene(Field {
      shapes,
      stamps: stamps.clone(),
    });
  let mut engine = HeadlessEngine::new_or_skip(builder)?;
  if let Some(stamps) = &stamps {
    engine.stamp_line_corners(Some((stamps.set.clone(), 2)));
  }
  let period = queue.get_timestamp_period();
  let (mut render, mut prepare, mut rest, mut gpu, mut corners) =
    (Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new());
  let (mut idle, mut untimed) = (0, 0);
  for frame in 0..WARMUP + FRAMES {
    engine.step_real(Duration::from_micros(16_667));
    device
      .poll(wgpu::PollType::wait_indefinitely())
      .expect("the GPU finishes its work");
    // A stream of few points runs no corner pass, and takes no stamps
    let corners_ran = engine.line_corners_built();
    let gpu_time = stamps
      .as_ref()
      .and_then(|stamps| stamps.read(&device, period, corners_ran));
    if frame < WARMUP {
      continue;
    }
    let (frame, prepared) = (engine.render_time(), engine.batch_prepare_time());
    if engine.debug_stats().draw_calls == 0 || prepared.is_zero() {
      idle += 1;
    }
    // Metal now and then returns no time for a frame (a stamp zero): such
    // a frame is left out of the GPU's median, not counted as idle
    if stamps.is_some() && gpu_time.is_none() {
      untimed += 1;
    }
    if let Some((time, corner)) = gpu_time {
      gpu.push(time);
      corners.push(corner);
    }
    render.push(frame);
    prepare.push(prepared);
    rest.push(frame.saturating_sub(prepared));
  }
  Some(Figures {
    render: median(render),
    prepare: median(prepare),
    extract: median(rest),
    gpu: stamps.is_some().then(|| median(gpu)),
    idle,
    untimed,
    corners: stamps.is_some().then(|| median(corners)),
  })
}

/// Measure `shapes` as `name`, printing its figures; `None` without a GPU
fn run(name: &str, shapes: Shapes) -> Option<Figures> {
  let figures = measure(shapes)?;
  println!("{name}_render: {:.1} µs", figures.render);
  println!("{name}_prepare: {:.1} µs", figures.prepare);
  println!("{name}_extract: {:.1} µs", figures.extract);
  match figures.gpu {
    Some(gpu) => println!(
      "{name}_gpu: {gpu:.1} µs ({} of {FRAMES} frames untimed; corner pass {:.1} µs)",
      figures.untimed,
      figures.corners.unwrap_or(0.0)
    ),
    None => println!("{name}_gpu: no timestamps on this device"),
  }
  if figures.idle > 0 {
    guards::miss(format!(
      "{name}: {} of {FRAMES} frames drew nothing",
      figures.idle
    ));
  }
  Some(figures)
}

fn main() {
  // Criterion's runner passes flags (`--bench`) that this has no use for;
  // `--list` and `--test` run nothing, and a filter names the cases to run
  if !guards::runs(&["lines"]) {
    return;
  }
  let Some(mixed) = run("lines", Shapes::Mixed) else {
    println!("no GPU: skipped");
    return;
  };
  held("lines_extract", mixed.extract, EXTRACT_MAX);
  held("lines_render", mixed.render, RENDER_MAX);
  for (join, name) in [(LineJoin::Miter, "miter"), (LineJoin::Round, "round")] {
    for count in COUNTS {
      run(
        &format!("lines_only_{name}_{count}"),
        Shapes::LinesOnly { join, count },
      );
    }
  }
  guards::verdict(
    "lines",
    "Profile a miss with the bench binary `cargo bench --bench lines --no-run` names.",
  );
  eprintln!("lines guards: every guard met its limit");
}

/// Note a miss when `time` is over `limit`
fn held(name: &str, time: f64, limit: f64) {
  if time > limit {
    guards::miss(format!("{name} {time:.1} µs over {limit:.1} µs"));
  }
}
