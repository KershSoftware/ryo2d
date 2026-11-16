//! Lines and trails with a GPU (`HeadlessEngine`, 1920 x 1080): a thousand
//! of them in a grid that fills the view, 500 `Line2D`s and 500 `Trail2D`s
//! of five to seven points that turn at every point, of mixed joins
//! (miter, bevel, round), caps (butt, round, square) and widths (0.5 to 10
//! px), unlit, none moving.
//!
//! A run warms its engine up, then steps frames (so a frame extracts and
//! draws the lines again, as every frame does) and takes the median of each
//! figure, printed as `name: X µs`:
//! - `lines_render`: the render step's CPU time as a whole (the lines and
//!   trails built into triangles in extract, then the draw: placing,
//!   uploading and encoding). The entity drawer's own timer does not cover
//!   the lines, which are built after it;
//! - `lines_prepare`: `Batch2D::prepare` within it (the triangles placed and
//!   uploaded);
//! - `lines_extract`: the rest of the render step (render less prepare),
//!   where the lines and trails are built and the frame is recorded and
//!   submitted: the figure the line work moves.
//!
//! The figure recorded is the median of three runs (`docs/backlog/lines.md`).
//! The guards are loose: `MAX` is far above the figures, so only a
//! regression of a different order fails the run. The line anti-aliasing
//! work is measured against the recorded figures, not against the guard.

mod guards;

use std::time::Duration;

use ryo2d::prelude::*;
use ryo2d::testing::{HeadlessEngine, shared_gpu};

const RESOLUTION: (u32, u32) = (1920, 1080);
/// Lines in the grid, and trails: a thousand together
const LINES: usize = 500;
const TRAILS: usize = 500;
/// Grid columns, and the cell each shape fills
const COLUMNS: usize = 40;
const CELL: (f32, f32) = (48.0, 43.2);
/// Frames before the measure, and measured
const WARMUP: usize = 60;
const FRAMES: usize = 300;
/// Widths, in pixels
const WIDTHS: [f32; 6] = [0.5, 1.0, 2.0, 3.0, 6.0, 10.0];

/// Over which a figure is a regression, not a measure: a figure of the
/// first runs' is a tenth of it or less. This guards against a change of
/// another order, not against a few per cent.
const EXTRACT_MAX: f64 = 8000.0;
const RENDER_MAX: f64 = 16000.0;

const JOINS: [LineJoin; 3] = [LineJoin::Miter, LineJoin::Bevel, LineJoin::Round];
const CAPS: [LineCap; 3] = [LineCap::Butt, LineCap::Round, LineCap::Square];

/// The centre of cell `index`
fn cell(index: usize) -> Vec2 {
  let (column, row) = (index % COLUMNS, index / COLUMNS);
  Vec2::new((column as f32 + 0.5) * CELL.0, (row as f32 + 0.5) * CELL.1)
}

/// The points of shape `index` about `centre`: five to seven, turning
/// sharply and gently by turns, within the cell
fn points(index: usize, centre: Vec2) -> Vec<Vec2> {
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
        + Vec2::new(
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

struct Field;

impl Scene for Field {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    ctx
      .camera()
      .set_position(RESOLUTION.0 as f32 / 2.0, RESOLUTION.1 as f32 / 2.0);
    ctx.lighting().set_enabled(false);
    for index in 0..LINES + TRAILS {
      let centre = cell(index);
      let points = points(index, centre);
      let width = WIDTHS[index % WIDTHS.len()];
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
}

fn median(mut times: Vec<Duration>) -> f64 {
  times.sort();
  times
    .get(times.len() / 2)
    .map_or(0.0, |time| time.as_secs_f64() * 1e6)
}

/// Medians in microseconds: render, prepare, and the render step's rest
fn measure() -> Option<[f64; 3]> {
  let builder = EngineBuilder::new()
    .resolution(RESOLUTION.0, RESOLUTION.1)
    .start_scene(Field);
  let mut engine = HeadlessEngine::new_or_skip(builder)?;
  let (device, _) = shared_gpu().ok()?;
  let (mut render, mut prepare, mut rest) = (Vec::new(), Vec::new(), Vec::new());
  for frame in 0..WARMUP + FRAMES {
    engine.step_real(Duration::from_micros(16_667));
    device
      .poll(wgpu::PollType::wait_indefinitely())
      .expect("the GPU finishes its work");
    if frame >= WARMUP {
      let (frame, prepared) = (engine.render_time(), engine.batch_prepare_time());
      render.push(frame);
      prepare.push(prepared);
      rest.push(frame.saturating_sub(prepared));
    }
  }
  Some([median(render), median(prepare), median(rest)])
}

fn main() {
  // Criterion's runner passes flags (`--bench`) that this has no use for;
  // `--list` and `--test` run nothing, and a filter names the cases to run
  if !guards::runs(&["lines"]) {
    return;
  }
  let Some([render, prepare, extract]) = measure() else {
    println!("no GPU: skipped");
    return;
  };
  println!("lines_render: {render:.1} µs");
  println!("lines_prepare: {prepare:.1} µs");
  println!("lines_extract: {extract:.1} µs");
  held("lines_extract", extract, EXTRACT_MAX);
  held("lines_render", render, RENDER_MAX);
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
