//! Saves and settings against the spec's two speed targets, in a headless
//! engine (`HeadlessEngine`, which keeps its saves and `system.json` in
//! memory, so the disk's own cost is not in these figures, but for
//! `frames_during_a_write_disk`'s).
//!
//! Before criterion runs, three guards and four records:
//! - `save_handoff_1kb` and `save_handoff_1mb`: the `save` call alone
//!   (`ctx.saves().save`: the slot check, the header, queueing the job).
//!   The data is a list of items whose RON text is 1 KB or 1 MB long, cloned
//!   before the clock starts; before each timed call the previous write is
//!   finished and drained, so the writer thread is idle. The median of the
//!   calls is held to 20 µs (the spec);
//! - `idle_settings`: one frame's settings work with nothing changed
//!   (`Engine::apply_settings` and the settings writer's tick, through
//!   `HeadlessEngine::idle_settings`) on an engine that keeps every
//!   preference. 100 batches of 10,000 calls; the median batch's time a call
//!   is held to 1 µs (the spec). The criterion bench of the same name times
//!   the call for the profile;
//! - `frames_during_a_write`, recorded: a headless frame (1,000 sprites)
//!   while a 1 MB save writes against the same engine with no write, three
//!   rounds of 30 pairs, each pair a run of frames with a write queued (the
//!   frames from the save until the write is drained) and the same number
//!   without, in alternating order, so a change in the machine's load
//!   reaches both. The verdict is "within noise" when the pooled medians
//!   differ by no more than the spread of the no-write medians across the
//!   rounds, else the difference;
//! - `frames_during_a_write_disk`, recorded: the same, with the engine's
//!   saves in a temporary folder on the local disk
//!   (`HeadlessEngine::save_to_folder`), so each write is the save folder's:
//!   the temporary file written and synced, the current file read and
//!   checked, the renames and the folder's sync;
//! - `load_100kb` and `load_1mb`, recorded: `load` of a saved slot (the
//!   store's read, the checks, the inflate, RON and the upgrade check), and
//!   the size of the save file written, with its RON text's.
//!
//! A guard missed fails the run once criterion's bench has run. A guard runs
//! only where criterion would measure the bench of its name: with no filter
//! or one naming it, and not for `--list`, `--test` or `--profile-time`.

mod guards;

use std::process::Command;
use std::time::{Duration, Instant, SystemTime};

use criterion::Criterion;
use ryo2d::prelude::*;
use ryo2d::testing::HeadlessEngine;
use serde::{Deserialize, Serialize};

/// The spec's `save` on the main thread
const SAVE_TARGET: Duration = Duration::from_micros(20);
/// The spec's idle settings frame
const IDLE_TARGET: Duration = Duration::from_micros(1);
const SLOT: &str = "bench";
const SLOT_FILE: &str = "bench.ryo-save";
/// Timed `save` calls for the small and the large data
const SMALL_SAVES: usize = 400;
const LARGE_SAVES: usize = 100;
/// Timed loads
const LOADS: usize = 100;
/// Idle settings: batches, and calls in each
const IDLE_BATCHES: usize = 100;
const IDLE_CALLS: u32 = 10_000;
/// The frames' rounds, and the pairs in each
const ROUNDS: usize = 3;
const PAIRS: usize = 30;
const SPRITES: usize = 1_000;
const KB: usize = 1_000;
const MB: usize = 1_000_000;

#[derive(Clone, Serialize, Deserialize)]
struct Item {
  id: u32,
  name: String,
  at: (f32, f32),
  stack: u16,
}

/// Items whose RON text is about `bytes` long: names and positions from a
/// fixed pseudo-random sequence, so the data does not compress to nothing
fn items(bytes: usize) -> Vec<Item> {
  let mut state = 0x2545_f491_4f6c_dd1du64;
  let mut next = move || {
    state ^= state << 13;
    state ^= state >> 7;
    state ^= state << 17;
    state
  };
  let mut items = Vec::new();
  let mut size = 2;
  while size < bytes {
    let n = next();
    let item = Item {
      id: (n >> 32) as u32,
      name: format!("item-{:05}", n % 100_000),
      at: ((n % 4096) as f32 / 4.0, ((n >> 12) % 4096) as f32 / 4.0),
      stack: (n >> 24) as u16 % 100,
    };
    size += ron::to_string(&item).map_or(0, |text| text.len()) + 1;
    items.push(item);
  }
  items
}

fn ron_len(items: &[Item]) -> usize {
  ron::to_string(items).map_or(0, |text| text.len())
}

fn us(time: Duration) -> f64 {
  time.as_secs_f64() * 1e6
}

fn median(times: &mut [Duration]) -> Duration {
  times.sort_unstable();
  times[times.len() / 2]
}

struct Sprites;

impl Scene for Sprites {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    let image = ctx.assets().add(Image::solid_rect(2, 2, 200, 60, 40, 255));
    for i in 0..SPRITES {
      let (col, row) = ((i % 40) as f32, (i / 40) as f32);
      ctx.world.spawn((
        Transform::from_xy(col * 7.0 + 2.0, row * 7.0 + 2.0),
        Renderable::image(image.clone()),
      ));
    }
  }
}

fn engine(keeps_settings: bool) -> Option<HeadlessEngine> {
  let mut builder = EngineBuilder::new()
    .resolution(300, 200)
    .start_scene(Sprites);
  if keeps_settings {
    builder = builder.with_user_preferences(UserPreferenceFilter::all());
  }
  let mut engine = HeadlessEngine::new_or_skip(builder)?;
  engine.step_frames(30).expect("warm-up frames");
  Some(engine)
}

/// Step frames until the slot's write is finished and drained
fn settle(engine: &mut HeadlessEngine) {
  let mut frames = 0;
  while SavesView::new(engine.world()).is_writing(SLOT) {
    engine.step().expect("frame");
    frames += 1;
    assert!(frames < 100_000, "a write never finished");
  }
}

fn save_handoff(engine: &mut HeadlessEngine, name: &str, bytes: usize, count: usize) {
  let data = items(bytes);
  let mut times = Vec::with_capacity(count);
  for i in 0..count + 5 {
    settle(engine);
    let data = data.clone();
    let saves = SavesView::new(engine.world());
    let start = Instant::now();
    let result = saves.save(SLOT, data);
    let took = start.elapsed();
    assert!(result.is_ok(), "save failed: {result:?}");
    // The first calls start the writer's thread
    if i >= 5 {
      times.push(took);
    }
  }
  settle(engine);
  let (min, max) = (times.iter().min().copied(), times.iter().max().copied());
  let mid = median(&mut times);
  let line = format!(
    "{name}: median {:.2} us over {count} saves of {} B of RON (min {:.2}, max {:.2}), target {} us",
    us(mid),
    ron_len(&data),
    us(min.unwrap_or_default()),
    us(max.unwrap_or_default()),
    us(SAVE_TARGET),
  );
  println!("{line}");
  if mid > SAVE_TARGET {
    guards::miss(format!(
      "{name} median {:.2} us over {} us",
      us(mid),
      us(SAVE_TARGET)
    ));
  }
}

fn idle_batch(engine: &mut HeadlessEngine) -> Duration {
  let start = Instant::now();
  for _ in 0..IDLE_CALLS {
    engine.idle_settings();
  }
  start.elapsed() / IDLE_CALLS
}

fn idle_guard(engine: &mut HeadlessEngine) {
  assert!(engine.idle_settings(), "the engine has no settings writer");
  idle_batch(engine);
  let mut batches: Vec<Duration> = (0..IDLE_BATCHES).map(|_| idle_batch(engine)).collect();
  let mid = median(&mut batches);
  println!(
    "idle_settings: median {:.1} ns a call over {IDLE_BATCHES} batches of {IDLE_CALLS} (min {:.1}, max {:.1}), target {} ns",
    mid.as_secs_f64() * 1e9,
    batches[0].as_secs_f64() * 1e9,
    batches[batches.len() - 1].as_secs_f64() * 1e9,
    IDLE_TARGET.as_nanos(),
  );
  if mid > IDLE_TARGET {
    guards::miss(format!(
      "idle_settings median {:.1} ns over {} ns",
      mid.as_secs_f64() * 1e9,
      IDLE_TARGET.as_nanos()
    ));
  }
}

/// Frames stepped while the slot's write is queued, each timed
fn frames_with_write(
  engine: &mut HeadlessEngine,
  data: &[Item],
  times: &mut Vec<Duration>,
) -> usize {
  let before = times.len();
  let result = SavesView::new(engine.world()).save(SLOT, data.to_vec());
  assert!(result.is_ok(), "save failed: {result:?}");
  loop {
    let start = Instant::now();
    engine.step().expect("frame");
    times.push(start.elapsed());
    if !SavesView::new(engine.world()).is_writing(SLOT) {
      return times.len() - before;
    }
  }
}

fn frames_without_write(engine: &mut HeadlessEngine, frames: usize, times: &mut Vec<Duration>) {
  for _ in 0..frames {
    let start = Instant::now();
    engine.step().expect("frame");
    times.push(start.elapsed());
  }
}

fn frames_record(engine: &mut HeadlessEngine, name: &str) {
  let data = items(MB);
  let (mut all_a, mut all_b) = (Vec::new(), Vec::new());
  let mut a_medians = Vec::new();
  for round in 1..=ROUNDS {
    let (mut a, mut b) = (Vec::new(), Vec::new());
    let mut last = 1;
    for pair in 0..PAIRS {
      settle(engine);
      if pair % 2 == 0 {
        last = frames_with_write(engine, &data, &mut b);
        settle(engine);
        frames_without_write(engine, last, &mut a);
      } else {
        // The no-write run first, as long as the last write's
        frames_without_write(engine, last, &mut a);
        last = frames_with_write(engine, &data, &mut b);
      }
    }
    let (frames_a, frames_b) = (a.len(), b.len());
    let (ma, mb) = (median(&mut a), median(&mut b));
    let p95 = |times: &[Duration]| times[times.len() * 95 / 100];
    println!(
      "{name} round {round}: no write median {:.1} us (p95 {:.1}, {frames_a} frames), write median {:.1} us (p95 {:.1}, {frames_b} frames), difference {:+.1} us",
      us(ma),
      us(p95(&a)),
      us(mb),
      us(p95(&b)),
      us(mb) - us(ma),
    );
    a_medians.push(ma);
    all_a.extend(a);
    all_b.extend(b);
  }
  let (ma, mb) = (median(&mut all_a), median(&mut all_b));
  let spread = a_medians.iter().max().copied().unwrap_or_default()
    - a_medians.iter().min().copied().unwrap_or_default();
  let diff = us(mb) - us(ma);
  let verdict = if diff.abs() <= us(spread) {
    "within noise".to_string()
  } else {
    format!("differs by {diff:+.1} us ({:+.1}%)", diff / us(ma) * 100.0)
  };
  println!(
    "{name}: pooled no write {:.1} us, write {:.1} us, difference {diff:+.1} us, no-write medians spread {:.1} us: {verdict}",
    us(ma),
    us(mb),
    us(spread),
  );
}

fn load_record(engine: &mut HeadlessEngine, name: &str, bytes: usize) {
  let data = items(bytes);
  let store = engine.saves();
  let result = SavesView::new(engine.world()).save(SLOT, data.clone());
  assert!(result.is_ok(), "save failed: {result:?}");
  settle(engine);
  let size = store.get(SLOT_FILE).map_or(0, |file| file.len());
  let saves = SavesView::new(engine.world());
  let mut times = Vec::with_capacity(LOADS);
  for i in 0..LOADS + 3 {
    let start = Instant::now();
    let loaded = saves.load::<Vec<Item>>(SLOT);
    let took = start.elapsed();
    let ok = loaded
      .as_ref()
      .is_ok_and(|loaded| loaded.data.len() == data.len());
    assert!(ok, "load failed or differs");
    if i >= 3 {
      times.push(took);
    }
  }
  let (min, max) = (times.iter().min().copied(), times.iter().max().copied());
  let mid = median(&mut times);
  println!(
    "{name}: median {:.1} us over {LOADS} loads (min {:.1}, max {:.1}); {} B of RON, {size} B on disk",
    us(mid),
    us(min.unwrap_or_default()),
    us(max.unwrap_or_default()),
    ron_len(&data),
  );
}

fn idle_bench(c: &mut Criterion, engine: &mut HeadlessEngine) {
  c.bench_function("idle_settings", |b| b.iter(|| engine.idle_settings()));
}

fn main() {
  let started = SystemTime::now();
  let load = Command::new("uptime")
    .output()
    .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
    .unwrap_or_else(|_| "uptime unavailable".into());
  println!("machine: {load}");
  println!(
    "saves bench: save hand-off, idle settings, frames during a write (memory, disk), loads"
  );
  let results = guards::results_dir();
  let mut criterion = Criterion::default()
    .sample_size(20)
    .output_directory(&results)
    .configure_from_args();
  if guards::runs(&["save_handoff_1kb", "save_handoff_1mb"])
    && let Some(mut engine) = engine(false)
  {
    save_handoff(&mut engine, "save_handoff_1kb", KB, SMALL_SAVES);
    save_handoff(&mut engine, "save_handoff_1mb", MB, LARGE_SAVES);
  }
  if guards::runs(&["frames_during_a_write"])
    && let Some(mut engine) = engine(false)
  {
    frames_record(&mut engine, "frames_during_a_write");
  }
  if guards::runs(&["frames_during_a_write_disk"])
    && let Some(mut engine) = engine(false)
  {
    let folder = tempfile::tempdir().expect("a temporary folder");
    engine
      .save_to_folder(folder.path())
      .expect("a save folder on disk");
    println!(
      "frames_during_a_write_disk: saves in {}",
      folder.path().display()
    );
    frames_record(&mut engine, "frames_during_a_write_disk");
    let size = std::fs::metadata(folder.path().join(SLOT_FILE)).map_or(0, |file| file.len());
    println!("frames_during_a_write_disk: the save file is {size} B on disk");
    // Its last writes finish before the folder goes
    drop(engine);
  }
  if guards::runs(&["load_100kb", "load_1mb"])
    && let Some(mut engine) = engine(false)
  {
    load_record(&mut engine, "load_100kb", 100 * KB);
    load_record(&mut engine, "load_1mb", MB);
  }
  let idle = guards::runs(&["idle_settings"]);
  let mut idle_engine = if idle { engine(true) } else { None };
  if let Some(engine) = &mut idle_engine {
    idle_guard(engine);
    idle_bench(&mut criterion, engine);
  }
  criterion.final_summary();
  guards::print_median(
    &results,
    "idle_settings",
    "apply_settings and the writer's tick, nothing changed",
    started,
    3,
  );
  guards::verdict(
    "saves",
    "Profile a miss (the bench binary is the one `cargo bench --bench saves --no-run` names) with\n  \
     xcrun xctrace record --template 'Time Profiler' --output saves.trace --launch -- \
     target/release/deps/saves-<hash> --bench --profile-time 20 '^idle_settings$'\n(for the \
     save hand-offs, which have no criterion bench, launch `--bench save_handoff` to profile \
     the guard).",
  );
  eprintln!("saves guards: every guard that ran met its target");
}
