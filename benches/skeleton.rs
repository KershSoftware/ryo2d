//! Skeletons against the spec's targets, with a GPU (`HeadlessEngine`, 1920 x
//! 1080, ambient 0.3 under one light): 200 skeletons of 30 bones and 25
//! region slots, each on an `Interpolated` entity playing two tracks (a base
//! of 30 moving bones, a layer of 12) with a crossfade always in progress,
//! lit, events firing, in a grid that fills the view.
//!
//! A run of a case warms its engine up, then steps frames of 1.3 fixed steps
//! (so a frame draws between two steps, never on one), and in every frame a
//! few skeletons START a mix (`set_animation` and `add_animation` on the
//! base track, `set_animation` and `set_empty` on the layer), so a call and
//! a new mix are in the figures. Every figure is a median, printed as
//! `name: X µs`:
//! - `skeletons_update`: the fixed step's skeleton work (`FixedSet::Skeletons`,
//!   between two probe systems) per fixed step, 200 skeletons;
//! - `skeletons_extract`: the entity drawer (`EntityDrawer::collect`: the
//!   quads and the draw list's runs) per frame, the same 200; and
//!   `skeletons_render`, the render step's CPU time as a whole;
//! - `skeletons_extract_shadow`: the same with every skeleton's drop shadow
//!   (a second quad per region);
//! - `skeletons_extract_offscreen`: the same, every skeleton wholly outside
//!   the view;
//! - `skeletons_extract_images`: an image for each of the 25 parts, where
//!   the others draw from one atlas;
//! - `one_skeleton_update`: the step's work for one skeleton, and
//!   `no_skeleton_update`, the probes round nothing, which is the floor
//!   the first has in it;
//! - `skeletons_update_sockets` and `skeletons_extract_sockets`: 200
//!   skeletons each carrying a socket with a child;
//! - `skeleton_pass_200` and `skeleton_pass_2000`: the frame pass that
//!   poses new skeletons (`pose_new_skeletons`) in a steady frame, per call,
//!   run outside a frame;
//! - `skeleton_pass_socket_spawn`: that pass in a frame in which a socketed
//!   entity was spawned, with 200 skeletons present (it looks through them);
//!   and `skeleton_pass_socket_spawn_sockets`, with a socket on each.
//!
//! The cases are guarded (see the constants): a miss prints its figures and
//! the run exits non-zero once every case has run.
//!
//! Measure on a quiet machine and record `uptime` with every run: the
//! figures move with the load.

mod guards;

use std::sync::Arc;
use std::time::{Duration, Instant};

use ryo2d::prelude::*;
use ryo2d::testing::{HeadlessEngine, shared_gpu};

const RESOLUTION: (u32, u32) = (1920, 1080);
const WARMUP: usize = 90;
const FRAMES: usize = 300;
/// A frame's length in fixed steps
const FRAME_STEPS: f64 = 1.3;
const BONES: usize = 30;
const SLOTS: usize = 25;
/// The grid the skeletons stand in: 20 by 10 cells of 96 x 108 fill the view
const COLUMNS: usize = 20;
const CELL: Vec2 = Vec2::new(96.0, 108.0);
/// How far right a wholly off-screen crowd stands
const OFF_SCREEN: f32 = 10_000.0;

/// The spec's targets
const UPDATE_MAX: f64 = 1000.0;
const EXTRACT_MAX: f64 = 600.0;
const ONE_MAX: f64 = 5.0;

/// What a case's scene holds
#[derive(Clone, Copy)]
struct Case {
  skeletons: usize,
  /// Parts drawn from one atlas (else an image each)
  atlas: bool,
  shadow: bool,
  offscreen: bool,
  sockets: bool,
}

impl Case {
  const fn crowd() -> Self {
    Case {
      skeletons: 200,
      atlas: true,
      shadow: false,
      offscreen: false,
      sockets: false,
    }
  }
}

/// The fixed step's skeleton work, between two probes
#[derive(Resource, Default)]
struct UpdateClock {
  started: Option<Instant>,
  steps: Vec<Duration>,
}

fn begin(mut clock: ResMut<UpdateClock>) {
  clock.started = Some(Instant::now());
}

fn end(mut clock: ResMut<UpdateClock>) {
  if let Some(started) = clock.started.take() {
    let took = started.elapsed();
    clock.steps.push(took);
  }
}

const EASE: SkeletonCurve = SkeletonCurve::Bezier {
  cx1: 0.25,
  cy1: 0.1,
  cx2: 0.25,
  cy2: 1.0,
};

/// The rig: a spine of 10 bones and 20 more hanging off it in chains of
/// up to 4; 25 slots on distinct bones, each with a region
fn rig(images: &[Handle<Image>]) -> SkeletonData {
  let turn = |bone: usize, amount: f32| SkeletonTimeline::Rotate {
    bone: BoneId(bone as u8),
    frames: vec![
      (0.0, -amount, EASE),
      (0.5, amount, EASE),
      (1.0, -amount, EASE),
    ],
  };
  let slide = |bone: usize| SkeletonTimeline::Translate {
    bone: BoneId(bone as u8),
    frames: vec![
      (0.0, Vec2::ZERO, SkeletonCurve::Linear),
      (0.5, Vec2::new(1.0, 2.0), SkeletonCurve::Linear),
      (1.0, Vec2::ZERO, SkeletonCurve::Linear),
    ],
  };
  let grow = |bone: usize| SkeletonTimeline::Scale {
    bone: BoneId(bone as u8),
    frames: vec![
      (0.0, Vec2::ONE, SkeletonCurve::Linear),
      (0.5, Vec2::new(1.2, 0.9), SkeletonCurve::Linear),
      (1.0, Vec2::ONE, SkeletonCurve::Linear),
    ],
  };
  let tint = |slot: usize| SkeletonTimeline::Color {
    slot: SlotId(slot as u8),
    frames: vec![
      (0.0, Color::WHITE, SkeletonCurve::Linear),
      (0.5, Color::new(255, 200, 200, 255), SkeletonCurve::Linear),
      (1.0, Color::WHITE, SkeletonCurve::Linear),
    ],
  };
  let events = |times: &[f32]| SkeletonTimeline::Event {
    frames: times
      .iter()
      .map(|&time| (time, 0, SkeletonEventValues::default()))
      .collect(),
  };
  let clip = |name: &str, timelines: Vec<SkeletonTimeline>| SkeletonClip {
    name: Arc::from(name),
    duration: 1.0,
    timelines,
  };
  let mut idle: Vec<_> = (0..BONES).map(|bone| turn(bone, 0.15)).collect();
  idle.extend((1..9).map(slide));
  idle.extend((0..5).map(tint));
  idle.push(events(&[0.0, 0.5]));
  let mut walk: Vec<_> = (0..BONES).map(|bone| turn(bone, 0.4)).collect();
  walk.extend((10..16).map(slide));
  walk.extend((4..10).map(grow));
  walk.push(events(&[0.25, 0.75]));
  let mut wave: Vec<_> = (3..15).map(|bone| turn(bone, 0.8)).collect();
  wave.extend((3..6).map(slide));
  let mut aim: Vec<_> = (20..30).map(|bone| turn(bone, 0.5)).collect();
  aim.extend((0..3).map(tint));

  let mut builder = SkeletonData::builder().bone("b0", None, BoneSetup::default());
  for bone in 1..BONES {
    let parent = if bone < 10 { bone - 1 } else { bone - 9 };
    let setup = BoneSetup {
      x: 4.0,
      length: 4.0,
      rotation: if bone < 10 { -0.2 } else { 0.4 },
      ..BoneSetup::default()
    };
    builder = builder.bone(format!("b{bone}"), Some(&format!("b{parent}")), setup);
  }
  for part in 0..SLOTS {
    let (slot, name) = (format!("s{part}"), format!("p{part}"));
    let setup = SlotSetup {
      attachment: Some(Arc::from(name.as_str())),
      ..SlotSetup::default()
    };
    // One atlas of 5 x 5 parts of 16 px, or an image each
    let (image, x, y) = match images {
      [atlas] => (
        atlas.clone(),
        (part % 5) as u32 * 16,
        (part / 5) as u32 * 16,
      ),
      many => (many[part].clone(), 0, 0),
    };
    builder = builder
      .slot(slot.as_str(), &format!("b{}", (part * 7) % BONES), setup)
      .region(SkeletonRegion {
        name: Arc::from(name.as_str()),
        image,
        x,
        y,
        width: 14,
        height: 14,
      })
      .attachment(
        SkeletonData::DEFAULT_SKIN,
        &slot,
        name.as_str(),
        SkeletonAttachment::Region(RegionAttachment::new(part, 12.0, 12.0)),
      );
  }
  builder
    .event(SkeletonEventData::new("step"))
    .animation(clip("idle", idle))
    .animation(clip("walk", walk))
    .animation(clip("wave", wave))
    .animation(clip("aim", aim))
    .default_mix(1.0)
    .build()
    .expect("a valid rig")
}

struct Crowd {
  case: Case,
}

impl Scene for Crowd {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    let case = self.case;
    ctx
      .camera()
      .set_position(RESOLUTION.0 as f32 / 2.0, RESOLUTION.1 as f32 / 2.0);
    ctx
      .lighting()
      .set_ambient(AmbientLight::with_intensity(0.3));
    ctx.lighting().set_enabled(true);
    ctx.world.spawn((
      Transform::from_xy(RESOLUTION.0 as f32 / 2.0, RESOLUTION.1 as f32 / 2.0),
      Light2D::point(Color::WHITE, 1400.0),
    ));
    let images: Vec<Handle<Image>> = if case.atlas {
      vec![
        ctx
          .assets()
          .add(Image::solid_rect(80, 80, 200, 220, 255, 255)),
      ]
    } else {
      (0..SLOTS)
        .map(|part| {
          let shade = 100 + part as u8 * 6;
          ctx
            .assets()
            .add(Image::solid_rect(14, 14, shade, 220, 255, 255))
        })
        .collect()
    };
    let data = ctx.assets().add(rig(&images));
    let shift = if case.offscreen { OFF_SCREEN } else { 0.0 };
    for i in 0..case.skeletons {
      let at = Vec2::new(
        shift + 48.0 + (i % COLUMNS) as f32 * CELL.x,
        90.0 + (i / COLUMNS % 10) as f32 * CELL.y,
      );
      let mut renderable = Renderable::skeleton();
      if case.shadow {
        renderable = renderable.with_drop_shadow(DropShadow::default());
      }
      let skeleton = ctx
        .world
        .spawn((
          Transform::from_translation(at),
          Interpolated,
          Skeleton::new(data.clone()),
          SkeletonAnimation::new(),
          renderable,
        ))
        .id();
      if case.sockets {
        let socket = ctx
          .world
          .spawn((
            Transform::IDENTITY,
            BoneSocket::new("b29"),
            ChildOf(skeleton),
          ))
          .id();
        ctx
          .world
          .spawn((Transform::from_xy(3.0, 0.0), ChildOf(socket)));
      }
    }
  }
}

/// Starts a mix on skeleton `index` in `frame`: one of four calls every 15
/// frames, a different skeleton each frame
fn mix(animation: &mut SkeletonAnimation, index: usize, frame: usize) {
  let k = (frame + index) % 60;
  if !k.is_multiple_of(15) {
    return;
  }
  let round = (frame + index) / 60;
  match k / 15 {
    0 => {
      animation.set_animation(0, ["idle", "walk"][round % 2], true);
    }
    1 => {
      animation.set_animation(1, ["wave", "aim"][round % 2], true);
    }
    2 => {
      animation.add_animation(0, ["walk", "idle"][round % 2], true, 0.0);
    }
    _ => animation.set_empty(1, 0.5),
  }
}

/// A started engine over `case`, its probes in
fn start(case: Case) -> Option<HeadlessEngine> {
  let builder = EngineBuilder::new()
    .resolution(RESOLUTION.0, RESOLUTION.1)
    .add_systems(
      Phase::FixedUpdate,
      begin
        .after(FixedSet::Transforms)
        .before(FixedSet::Skeletons),
    )
    .add_systems(
      Phase::FixedUpdate,
      end.after(FixedSet::Skeletons).before(FixedSet::Particles),
    )
    .start_scene(Crowd { case });
  let mut engine = HeadlessEngine::new_or_skip(builder)?;
  engine.world_mut().insert_resource(UpdateClock::default());
  Some(engine)
}

fn median(mut times: Vec<Duration>) -> f64 {
  times.sort();
  times
    .get(times.len() / 2)
    .map_or(0.0, |time| time.as_secs_f64() * 1e6)
}

/// What a case measured: medians in microseconds
struct Figures {
  update: f64,
  extract: f64,
  render: f64,
}

/// Runs `case` for `FRAMES` frames after the warm-up, starting mixes as it
/// goes; none without a GPU
fn measure(case: Case) -> Option<Figures> {
  let mut engine = start(case)?;
  let (device, _) = shared_gpu().ok()?;
  let step = engine.world().resource::<Time>().fixed.timestep();
  let frame_length = Duration::from_secs_f64(f64::from(step) * FRAME_STEPS);
  let mut animations = engine.world_mut().query::<&mut SkeletonAnimation>();
  let (mut extract, mut render) = (Vec::new(), Vec::new());
  for frame in 0..WARMUP + FRAMES {
    let world = engine.world_mut();
    for (index, mut animation) in animations.iter_mut(world).enumerate() {
      mix(&mut animation, index, frame);
    }
    engine.step_real(frame_length);
    device
      .poll(wgpu::PollType::wait_indefinitely())
      .expect("the GPU finishes its work");
    if frame + 1 == WARMUP {
      engine
        .world_mut()
        .resource_mut::<UpdateClock>()
        .steps
        .clear();
    }
    if frame >= WARMUP {
      extract.push(engine.entity_draw_time());
      render.push(engine.render_time());
    }
  }
  let update = std::mem::take(&mut engine.world_mut().resource_mut::<UpdateClock>().steps);
  Some(Figures {
    update: median(update),
    extract: median(extract),
    render: median(render),
  })
}

/// A started engine over `case`, warmed up until every skeleton has its pose
fn posed(case: Case) -> Option<HeadlessEngine> {
  let mut engine = start(case)?;
  engine.step_frames(30).expect("the warm-up frames");
  Some(engine)
}

/// The median microseconds a call of the frame pass takes with nothing new:
/// 100 batches of `calls`
fn steady_pass(case: Case, calls: u32) -> Option<f64> {
  let mut engine = posed(case)?;
  // The first call looks, and the rest do not
  engine.skeleton_pass_time(2);
  let batches: Vec<Duration> = (0..100)
    .map(|_| engine.skeleton_pass_time(calls) / calls)
    .collect();
  Some(median(batches))
}

/// The median microseconds the pass takes in a frame in which a socketed
/// entity was spawned: 60 spawns, each timed once and then undone
fn spawn_pass(case: Case) -> Option<f64> {
  let mut engine = posed(case)?;
  engine.skeleton_pass_time(2);
  let mut skeletons = engine.world_mut().query::<(Entity, &Skeleton)>();
  let carrier = skeletons
    .iter(engine.world())
    .map(|(entity, _)| entity)
    .next()?;
  let mut times = Vec::new();
  for _ in 0..66 {
    let socket = engine
      .world_mut()
      .spawn((
        Transform::IDENTITY,
        BoneSocket::new("b29"),
        ChildOf(carrier),
      ))
      .id();
    times.push(engine.skeleton_pass_time(1));
    engine.world_mut().despawn(socket);
    // The look the despawn does not cause
    engine.skeleton_pass_time(1);
  }
  // The first few are left out
  Some(median(times.split_off(6)))
}

fn main() {
  // Criterion's runner passes flags (`--bench`) that this has no use for;
  // `--list` and `--test` run nothing, and a filter names the cases to run
  if !guards::runs(&["skeletons", "skeleton"]) {
    return;
  }
  let Some(crowd) = measure(Case::crowd()) else {
    println!("no GPU: skipped");
    return;
  };
  println!("skeletons_update: {:.1} µs", crowd.update);
  println!("skeletons_extract: {:.1} µs", crowd.extract);
  println!("skeletons_render: {:.1} µs", crowd.render);
  held("skeletons_update", crowd.update, UPDATE_MAX);
  held("skeletons_extract", crowd.extract, EXTRACT_MAX);

  let cases = [
    (
      "skeletons_extract_shadow",
      Case {
        shadow: true,
        ..Case::crowd()
      },
    ),
    (
      "skeletons_extract_offscreen",
      Case {
        offscreen: true,
        ..Case::crowd()
      },
    ),
    (
      "skeletons_extract_images",
      Case {
        atlas: false,
        ..Case::crowd()
      },
    ),
  ];
  for (name, case) in cases {
    if let Some(figures) = measure(case) {
      println!("{name}: {:.1} µs", figures.extract);
      println!("{name}_update: {:.1} µs", figures.update);
      held(name, figures.extract, EXTRACT_MAX);
    }
  }
  if let Some(figures) = measure(Case {
    sockets: true,
    ..Case::crowd()
  }) {
    println!("skeletons_update_sockets: {:.1} µs", figures.update);
    println!("skeletons_extract_sockets: {:.1} µs", figures.extract);
    held("skeletons_update_sockets", figures.update, UPDATE_MAX);
    held("skeletons_extract_sockets", figures.extract, EXTRACT_MAX);
  }
  if let Some(none) = measure(Case {
    skeletons: 0,
    ..Case::crowd()
  }) {
    println!("no_skeleton_update: {:.1} µs", none.update);
  }
  if let Some(one) = measure(Case {
    skeletons: 1,
    ..Case::crowd()
  }) {
    println!("one_skeleton_update: {:.1} µs", one.update);
    held("one_skeleton_update", one.update, ONE_MAX);
  }
  if let Some(time) = steady_pass(Case::crowd(), 1000) {
    println!("skeleton_pass_200: {time:.2} µs");
  }
  if let Some(time) = steady_pass(
    Case {
      skeletons: 2000,
      ..Case::crowd()
    },
    100,
  ) {
    println!("skeleton_pass_2000: {time:.2} µs");
  }
  if let Some(time) = spawn_pass(Case::crowd()) {
    println!("skeleton_pass_socket_spawn: {time:.1} µs");
  }
  if let Some(time) = spawn_pass(Case {
    sockets: true,
    ..Case::crowd()
  }) {
    println!("skeleton_pass_socket_spawn_sockets: {time:.1} µs");
  }
  guards::verdict(
    "skeleton",
    "Profile a miss with the bench binary `cargo bench --bench skeleton --no-run` names.",
  );
  eprintln!("skeleton guards: every guard met its target");
}

/// Note a miss when `time` is over `limit`
fn held(name: &str, time: f64, limit: f64) {
  if time > limit {
    guards::miss(format!("{name} {time:.1} µs over {limit:.1} µs"));
  }
}
