//! Skeletons against the spec's targets, with a GPU (`HeadlessEngine`, 1920 x
//! 1080, ambient 0.3 under one light): 200 skeletons of 30 bones and 25
//! region slots, each on an interpolated entity (a skeleton makes its own
//! `Interpolated`) playing two tracks (a base of 30 moving bones, a layer
//! of 12) with a crossfade always in progress, lit, events firing, in a
//! grid that fills the view.
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
//! - `skeletons_extract_plain`: the same with `Interpolated` taken off every
//!   skeleton's entity, so nothing is drawn between steps: what the bone
//!   blend costs is the crowd's figure less this one;
//! - `skeletons_update_mesh` and `skeletons_extract_mesh`: the crowd with one
//!   weighted mesh of 60 vertices (two influences a vertex) among each
//!   skeleton's 25 regions, its shape keyed by both of the base track's
//!   animations, so a deform plays in every step and a skeleton draws a run
//!   of quads, the mesh, and a run of quads; `skeletons_extract_mesh_shadow`,
//!   the same with a drop shadow, a second mesh a skeleton, held to the
//!   extract's target as the shadow case of regions is;
//! - beside a case's extract, where the rest of its frame goes:
//!   `skeletons_prepare*` (`Batch2D::prepare`: placing, planning the draws
//!   and uploading), `skeletons_world_pass*` (the camera's world pass as it
//!   is recorded, between two probe passes placed round it; wgpu checks and
//!   encodes what a pass recorded later, at the encoder's finish, which is
//!   in the render step's figure alone), `skeletons_render*` and
//!   `skeletons_draws*`, the frame's draw calls; and `skeleton_mesh_cost*`,
//!   what a mesh adds to each, from a mesh case less its regions-only twin;
//! - `skeleton_first_shader_frame*`: the render step of the frame in which
//!   the crowd first draws through a game sprite shader the engine has not
//!   seen, of regions alone and with the mesh, which has a pipeline of its
//!   own to make;
//! - `one_skeleton_update`: the step's work for one skeleton, and
//!   `no_skeleton_update`, the probes round nothing, which is the floor
//!   the first has in it;
//! - `skeletons_update_sockets` and `skeletons_extract_sockets`: 200
//!   skeletons each carrying a socket with a child;
//! - `skeletons_update_constraints`: the crowd with constraints, each
//!   skeleton's two limbs of two bones reaching by IK, a bone pointed by a
//!   one-bone IK constraint and a bone copying a turn by a transform
//!   constraint, the three targets moved by world-space bone overrides
//!   before every step; a step whose tips are not on their targets is a
//!   miss, so the constraints are known to act in the steps measured;
//! - `skeleton_pass_200` and `skeleton_pass_2000`: the frame pass that
//!   poses new skeletons (`pose_new_skeletons`) in a steady frame, per call,
//!   run outside a frame;
//! - `skeleton_pass_socket_spawn`: that pass in a frame in which a socketed
//!   entity was spawned, with 200 skeletons present (it looks through them);
//!   and `skeleton_pass_socket_spawn_sockets`, with a socket on each.
//!
//! The cases with a target are guarded (see the constants): a miss prints
//! its figures and the run exits non-zero once every case has run.
//!
//! Measure on a quiet machine and record `uptime` with every run: the
//! figures move with the load.

mod guards;

use std::sync::{Arc, Mutex};
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
/// The rows of a skeleton's mesh: two vertices a row, 60 in all
const MESH_ROWS: usize = 30;
/// The region slot the mesh's slot follows: a skeleton draws 13 regions,
/// the mesh, and 12 regions more
const MESH_AFTER: usize = 12;
/// The shaders the first-use case sets one after another, and the frames
/// it steps after each
const FIRST_SHADERS: usize = 5;
const FIRST_FRAMES: usize = 12;

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
  /// `Interpolated` taken off every skeleton's entity: drawn at the step
  /// rate
  plain: bool,
  /// A weighted mesh of 60 vertices in a slot among the regions, its shape
  /// keyed by both of the base track's animations
  mesh: bool,
  /// The constraints of [`LIMBS`] and their targets moved every step
  constraints: bool,
}

impl Case {
  const fn crowd() -> Self {
    Case {
      skeletons: 200,
      atlas: true,
      shadow: false,
      offscreen: false,
      sockets: false,
      plain: false,
      mesh: false,
      constraints: false,
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

/// The camera's world pass as it is recorded, between two probe passes
/// placed round it: how long the frame's took
#[derive(Default)]
struct PassClock {
  started: Option<Instant>,
  frame: Duration,
}

/// One of the two probes round the world pass
struct PassProbe {
  ends: bool,
  clock: Arc<Mutex<PassClock>>,
}

impl RenderPass for PassProbe {
  fn id(&self) -> &str {
    if self.ends {
      "bench_world_end"
    } else {
      "bench_world_begin"
    }
  }

  fn scope(&self) -> PassScope {
    PassScope::Camera
  }

  fn execute(&mut self, _ctx: &mut RenderContext<'_>) {
    let Ok(mut clock) = self.clock.lock() else {
      return;
    };
    if !self.ends {
      clock.started = Some(Instant::now());
    } else if let Some(started) = clock.started.take() {
      clock.frame += started.elapsed();
    }
  }
}

/// Puts the two probes round `engine`'s world pass, and gives their clock
fn probe_world_pass(engine: &mut HeadlessEngine) -> Arc<Mutex<PassClock>> {
  let clock = Arc::new(Mutex::new(PassClock::default()));
  let probe = |ends| PassProbe {
    ends,
    clock: clock.clone(),
  };
  let mut pipeline = engine.world_mut().resource_mut::<RenderPipeline>();
  pipeline
    .insert_before(pass_ids::WORLD, probe(false))
    .expect("the world pass is in the pipeline");
  pipeline
    .insert_after(pass_ids::WORLD, probe(true))
    .expect("the world pass is in the pipeline");
  clock
}

const EASE: SkeletonCurve = SkeletonCurve::Bezier {
  cx1: 0.25,
  cy1: 0.1,
  cx2: 0.25,
  cy2: 1.0,
};

/// A weighted mesh of 60 vertices: a ribbon along the spine, each vertex
/// held by the two bones it lies between, so it has 120 positions for a
/// deform to key
fn ribbon() -> MeshAttachment {
  let vertices = (0..MESH_ROWS * 2).map(|vertex| {
    let (row, side) = (vertex / 2, vertex % 2);
    let along = row as f32 * 9.0 / MESH_ROWS as f32;
    let (bone, part) = (along as u8, along.fract());
    let y = if side == 0 { -6.0 } else { 6.0 };
    let weight = 0.9 - 0.8 * part;
    vec![
      SkeletonInfluence::new(BoneId(bone), part * 4.0, y, weight),
      SkeletonInfluence::new(BoneId(bone + 1), (part - 1.0) * 4.0, y, 1.0 - weight),
    ]
  });
  let uvs = (0..MESH_ROWS * 2)
    .map(|vertex| {
      Vec2::new(
        (vertex % 2) as f32,
        (vertex / 2) as f32 / (MESH_ROWS - 1) as f32,
      )
    })
    .collect();
  let triangles = (0..MESH_ROWS as u16 - 1)
    .flat_map(|row| {
      let first = row * 2;
      [first, first + 1, first + 2, first + 1, first + 3, first + 2]
    })
    .collect();
  let vertices = MeshVertices::Weighted(WeightedVertices::new(vertices));
  MeshAttachment::new(0, vertices, uvs, triangles)
}

/// The rig: a spine of 10 bones and 20 more hanging off it in chains of
/// up to 4; 25 slots on distinct bones, each with a region. With `mesh`, a
/// slot among them shows the [`ribbon`], and `idle` and `walk` each key
/// every position of it (three keys, a bezier on each, by a different
/// amount), so a deform plays in every step and two are blended through
/// every crossfade of the base track. With `constraints`, the IK
/// constraints of [`LIMBS`] and then a transform constraint that copies the
/// turn of the first limb's second bone (`b28`), in the world, onto `b26`
fn rig(images: &[Handle<Image>], mesh: bool, constraints: bool) -> SkeletonData {
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
  // The ribbon's shape: an offset for each of its positions, x then y
  let shape = |amount: f32| -> Vec<f32> {
    (0..MESH_ROWS * 4)
      .flat_map(|position| [0.0, amount * ((position % 7) as f32 - 3.0) / 3.0])
      .collect()
  };
  let sway = |amount: f32| SkeletonTimeline::Deform {
    slot: SlotId(MESH_AFTER as u8 + 1),
    attachment: Arc::from("ribbon"),
    frames: vec![
      (0.0, shape(-amount), EASE),
      (0.5, shape(amount), EASE),
      (1.0, shape(-amount), EASE),
    ],
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
  if mesh {
    idle.push(sway(1.0));
    walk.push(sway(2.0));
  }
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
    if mesh && part == MESH_AFTER {
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
  if constraints {
    for (name, [first, second, target]) in LIMBS {
      let bones: &[&str] = if first == second {
        &[first]
      } else {
        &[first, second]
      };
      builder = builder.ik(name, bones, target, IkSetup::default());
    }
    let turn = TransformSetup {
      mix: TransformMix {
        rotate: 1.0,
        ..TransformMix::default()
      },
      ..TransformSetup::default()
    };
    builder = builder.transform_constraint("copy", &["b26"], "b28", turn);
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
    let data = ctx.assets().add(rig(&images, case.mesh, case.constraints));
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
          Skeleton::new(data.clone()),
          SkeletonAnimation::new(),
          renderable,
        ))
        .id();
      // A skeleton makes its entity interpolated: the plain case takes
      // that off again
      if case.plain {
        ctx.world.entity_mut(skeleton).remove::<Interpolated>();
      }
      if case.constraints {
        ctx.world.entity_mut(skeleton).insert(Goals::default());
      }
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

/// The IK constraints of the constraint case, each named, with its first
/// and last bone and its target: two limbs of two bones (parts of 4 and 4,
/// hanging from the spine) and one bone pointed (the same bone twice).
/// The bones are among the rig's 30; the targets are leaves no constraint
/// moves
const LIMBS: [(&str, [&str; 3]); 3] = [
  ("limb_a", ["b19", "b28", "b21"]),
  ("limb_b", ["b20", "b29", "b22"]),
  ("point", ["b27", "b27", "b23"]),
];

/// The length of every bone of the rig: how far its tip is along its x axis
const BONE_LENGTH: f32 = 4.0;

/// Where a constrained skeleton's targets were put for its next step, in
/// the world, and how many times; and where each limb's first bone was in
/// the step before
#[derive(Component, Default)]
struct Goals {
  at: [Vec2; 3],
  steps: u32,
  origins: [Option<Vec2>; 3],
}

/// How the constrained crowd's steps went: those checked, those in which a
/// tip was off its target (or a target off where it was put), and the
/// farthest any was
#[derive(Resource, Default, Clone, Copy)]
struct Reached {
  steps: u32,
  missed: u32,
  worst: f32,
}

/// Before each step, puts every constrained skeleton's targets about where
/// the first bone of each limb is going (where it is, moved on as far as
/// it moved in the last step: the animation swings it by up to 5 a step),
/// 2.5 to 3.5 away, well within the limbs' reach of 8, each at a new place
/// every step
fn steer(mut skeletons: Query<(&mut Skeleton, &mut Goals)>) {
  for (index, (mut skeleton, mut goals)) in skeletons.iter_mut().enumerate() {
    goals.steps += 1;
    let t = goals.steps as f32 * 0.1 + index as f32;
    for (limb, (_, [first, _, target])) in LIMBS.into_iter().enumerate() {
      let Some(origin) = skeleton.bone_world(first).map(|bone| bone.translation) else {
        continue;
      };
      let last = goals.origins[limb].replace(origin).unwrap_or(origin);
      let turn = t + limb as f32 * 2.0;
      let goal = 2.0 * origin - last + Vec2::from_angle(turn) * (3.0 + 0.5 * (0.7 * turn).sin());
      goals.at[limb] = goal;
      let put = BoneOverride {
        translation: Some(goal),
        space: BoneSpace::World,
        ..BoneOverride::default()
      };
      skeleton.override_bone(target, put);
    }
  }
}

/// After each step, how far each constrained skeleton's targets are from
/// where they were put, and each limb's tip from its target (the pointed
/// bone's axis, from the line to its target). A target the limb's first
/// bone has moved out of reach of is reached for: the tip is then at the
/// full reach on the line to it
fn check(skeletons: Query<(&Skeleton, &Goals)>, mut reached: ResMut<Reached>) {
  let mut worst = 0.0_f32;
  let mut seen = 0;
  for (skeleton, goals) in &skeletons {
    for (limb, (_, [first, last, target])) in LIMBS.into_iter().enumerate() {
      let pointed = first == last;
      let (Some(origin), Some(last), Some(target)) = (
        skeleton.bone_world(first),
        skeleton.bone_world(last),
        skeleton.bone_world(target),
      ) else {
        worst = f32::INFINITY;
        continue;
      };
      let goal = goals.at[limb];
      let miss = if pointed {
        // The bone's axis passes through its target, ahead of it
        let axis = origin.matrix2.x_axis.normalize_or_zero();
        let to = goal - origin.translation;
        if axis.dot(to) > 0.0 {
          axis.perp_dot(to).abs()
        } else {
          f32::INFINITY
        }
      } else {
        let reach = 2.0 * BONE_LENGTH;
        let to = goal - origin.translation;
        let toward = if to.length() < reach {
          goal
        } else {
          origin.translation + to.normalize_or_zero() * reach
        };
        last
          .transform_point2(Vec2::new(BONE_LENGTH, 0.0))
          .distance(toward)
      };
      // A distance that is not a number is as far as any can be
      for off in [miss, target.translation.distance(goal)] {
        worst = if off.is_nan() {
          f32::INFINITY
        } else {
          worst.max(off)
        };
      }
    }
    seen += 1;
  }
  if seen == 0 {
    return;
  }
  reached.steps += 1;
  if worst >= 0.01 {
    reached.missed += 1;
  }
  reached.worst = reached.worst.max(worst);
}

/// A started engine over `case`, its probes in
fn start(case: Case) -> Option<HeadlessEngine> {
  let mut builder = EngineBuilder::new()
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
    );
  if case.constraints {
    builder = builder
      .add_systems(
        Phase::FixedUpdate,
        steer.after(FixedSet::Transforms).before(begin),
      )
      .add_systems(
        Phase::FixedUpdate,
        check.after(end).before(FixedSet::Particles),
      );
  }
  let mut engine = HeadlessEngine::new_or_skip(builder.start_scene(Crowd { case }))?;
  engine.world_mut().insert_resource(UpdateClock::default());
  engine.world_mut().insert_resource(Reached::default());
  Some(engine)
}

fn median(mut times: Vec<Duration>) -> f64 {
  times.sort();
  times
    .get(times.len() / 2)
    .map_or(0.0, |time| time.as_secs_f64() * 1e6)
}

/// What a case measured: medians in microseconds, and the draw calls of
/// its last frame
#[derive(Clone, Copy)]
struct Figures {
  update: f64,
  extract: f64,
  render: f64,
  /// `Batch2D::prepare`
  prepare: f64,
  /// The camera's world pass as it is recorded
  pass: f64,
  draws: u32,
  /// The skeletons whose mesh a deform is moving, after the last step
  deformed: usize,
  /// How the measured steps' constraints reached their targets
  reached: Reached,
  /// The fixed steps measured
  steps: usize,
}

impl Figures {
  /// The render step outside the entity drawer, `prepare` and the world
  /// pass's recording: the frame's other gathering and passes, and the
  /// encoder's finish and submit, where wgpu checks and encodes what the
  /// passes recorded
  fn rest(&self) -> f64 {
    self.render - self.extract - self.prepare - self.pass
  }
}

/// Runs `case` for `FRAMES` frames after the warm-up, starting mixes as it
/// goes; none without a GPU
fn measure(case: Case) -> Option<Figures> {
  let mut engine = start(case)?;
  let world_pass = probe_world_pass(&mut engine);
  let (device, _) = shared_gpu().ok()?;
  let step = engine.world().resource::<Time>().fixed.timestep();
  let frame_length = Duration::from_secs_f64(f64::from(step) * FRAME_STEPS);
  let mut animations = engine.world_mut().query::<&mut SkeletonAnimation>();
  let (mut extract, mut render) = (Vec::new(), Vec::new());
  let (mut prepare, mut pass) = (Vec::new(), Vec::new());
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
      *engine.world_mut().resource_mut::<Reached>() = Reached::default();
    }
    let recorded = world_pass
      .lock()
      .map_or(Duration::ZERO, |mut clock| std::mem::take(&mut clock.frame));
    if frame >= WARMUP {
      extract.push(engine.entity_draw_time());
      render.push(engine.render_time());
      prepare.push(engine.batch_prepare_time());
      pass.push(recorded);
    }
  }
  let update = std::mem::take(&mut engine.world_mut().resource_mut::<UpdateClock>().steps);
  let steps = update.len();
  Some(Figures {
    update: median(update),
    extract: median(extract),
    render: median(render),
    prepare: median(prepare),
    pass: median(pass),
    draws: engine.debug_stats().draw_calls,
    deformed: deformed(&mut engine),
    reached: *engine.world().resource::<Reached>(),
    steps,
  })
}

/// How many skeletons show a [`ribbon`] that stands off where its bones
/// alone put it (the setup shape, skinned) as the last step left it: those
/// a deform is moving
fn deformed(engine: &mut HeadlessEngine) -> usize {
  let MeshVertices::Weighted(setup) = ribbon().vertices else {
    return 0;
  };
  let world = engine.world_mut();
  let mut skeletons = world.query::<(Entity, &Skeleton)>();
  let world: &World = world;
  let moved = |&(entity, skeleton): &(Entity, &Skeleton)| {
    let drawn = ryo2d::testing::skeleton_mesh(world, entity, "ribbon");
    let off = |(vertex, at): (usize, &Vec2)| {
      let held = setup.influences(vertex).iter().map(|influence| {
        let bone = skeleton.bone_world(&format!("b{}", influence.bone.0))?;
        Some(bone.transform_point2(Vec2::new(influence.x, influence.y)) * influence.weight)
      });
      let plain: Option<Vec2> = held.sum();
      plain.is_some_and(|plain| at.distance(plain) > 0.05)
    };
    drawn.len() == setup.vertex_count() && drawn.iter().enumerate().any(off)
  };
  skeletons.iter(world).filter(moved).count()
}

/// Note a miss when a mesh case is not what its name says: `meshes` mesh
/// draws a skeleton among its runs of quads, and a deform moving the mesh
/// of three skeletons in four at least
fn is_the_case(name: &str, figures: &Figures, meshes: u32) {
  let skeletons = Case::crowd().skeletons;
  let drawn = figures.draws >= 2 * meshes * skeletons as u32;
  if !drawn || figures.deformed * 4 < skeletons * 3 {
    guards::miss(format!(
      "{name} drew {} draws, and a deform moved {} of its {skeletons} meshes",
      figures.draws, figures.deformed
    ));
  }
}

/// Prints where the rest of a case's frame goes, beside its extract: the
/// lines are named `skeletons_<what><suffix>`
fn context(suffix: &str, figures: &Figures) {
  println!("skeletons_prepare{suffix}: {:.1} µs", figures.prepare);
  println!("skeletons_world_pass{suffix}: {:.1} µs", figures.pass);
  println!("skeletons_draws{suffix}: {}", figures.draws);
}

/// Prints what each of `meshes` mesh draws adds to a frame: `with` them
/// less `without`, the same crowd of regions alone
fn mesh_cost(name: &str, with: &Figures, without: &Figures, meshes: f64) {
  let each = |with: f64, without: f64| (with - without) / meshes;
  println!(
    "{name}: {:.2} µs a mesh in the render step (extract {:.2}, prepare {:.2}, world pass {:.2}, \
     the rest {:.2}), {} draws against {}",
    each(with.render, without.render),
    each(with.extract, without.extract),
    each(with.prepare, without.prepare),
    each(with.pass, without.pass),
    each(with.rest(), without.rest()),
    with.draws,
    without.draws,
  );
}

/// A game sprite shader's source, another for each `salt` and `round`: the
/// three numbers are part of what it computes, so no two compile to the
/// same code
fn shader_source(salt: u32, round: usize) -> String {
  let part = |digits: u32| 0.5 + (digits % 1000) as f32 * 1e-4;
  let (r, g) = (part(salt), part(salt / 1000));
  let b = part(salt / 1_000_000) - round as f32 * 0.05;
  format!(
    "fn sprite(in: SpriteIn, params: Params) -> vec4<f32> {{\n  let c = sample(in.uv);\n  return \
     vec4<f32>(c.rgb * vec3<f32>({r:.4}, {g:.4}, {b:.4}), c.a);\n}}\n"
  )
}

/// The render step of the frame in which `case`'s crowd first draws
/// through a game sprite shader: one new to the machine (its source is
/// made from `salt`, which differs each run, so the GPU's own cache of
/// compiled shaders does not hold it), one that cache holds (the same
/// sources set a second time, their pipelines gone meanwhile), and a frame
/// once it draws through it. Each is the median, over `FIRST_SHADERS`
/// shaders set one after another, of the longest of the `FIRST_FRAMES`
/// frames after each is set; the last of their median frame
fn first_shader(case: Case, salt: u32) -> Option<[f64; 3]> {
  let mut engine = posed(case)?;
  let (device, _) = shared_gpu().ok()?;
  let mut settled = Vec::new();
  let mut pass = |engine: &mut HeadlessEngine| {
    let mut longest = Vec::new();
    for round in 0..FIRST_SHADERS {
      let name = format!("bench-first-{round}.wgsl");
      let shader = Shader::from_wgsl(name, shader_source(salt, round));
      let world = engine.world_mut();
      let shader = world.resource_mut::<Assets<Shader>>().add(shader);
      let mut renderables = world.query_filtered::<&mut Renderable, With<Skeleton>>();
      for mut renderable in renderables.iter_mut(world) {
        renderable.shader = Some(SpriteShader::custom(shader.clone()));
      }
      let mut frames = Vec::new();
      for _ in 0..FIRST_FRAMES {
        engine.step().expect("a frame");
        device
          .poll(wgpu::PollType::wait_indefinitely())
          .expect("the GPU finishes its work");
        frames.push(engine.render_time());
      }
      frames.sort();
      longest.push(frames.last().copied().unwrap_or_default());
      settled.push(frames.get(frames.len() / 2).copied().unwrap_or_default());
    }
    median(longest)
  };
  let new = pass(&mut engine);
  let cached = pass(&mut engine);
  Some([new, cached, median(settled)])
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

  let shadow = measure(Case {
    shadow: true,
    ..Case::crowd()
  });
  if let Some(figures) = &shadow {
    println!("skeletons_extract_shadow: {:.1} µs", figures.extract);
    println!("skeletons_extract_shadow_update: {:.1} µs", figures.update);
    held("skeletons_extract_shadow", figures.extract, EXTRACT_MAX);
  }
  let cases = [
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
    (
      "skeletons_extract_plain",
      Case {
        plain: true,
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
  if let Some(figures) = measure(Case {
    constraints: true,
    ..Case::crowd()
  }) {
    println!("skeletons_update_constraints: {:.1} µs", figures.update);
    held("skeletons_update_constraints", figures.update, UPDATE_MAX);
    let Reached {
      steps,
      missed,
      worst,
    } = figures.reached;
    println!(
      "skeletons_constraints_checked: {steps} steps, a tip at most {worst:.4} from its target"
    );
    // The constraints act in every step measured: no tip off its target
    let measured = figures.steps as u32;
    if missed > 0 || steps < measured {
      guards::miss(format!(
        "skeletons_update_constraints: {missed} of {steps} steps had a tip off its target (at most \
         {worst:.4}), of {measured} measured"
      ));
    }
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

  // A mesh among each skeleton's regions, with its deform playing
  let meshed = Case {
    mesh: true,
    ..Case::crowd()
  };
  if let Some(mesh) = measure(meshed) {
    println!("skeletons_update_mesh: {:.1} µs", mesh.update);
    println!("skeletons_extract_mesh: {:.1} µs", mesh.extract);
    println!("skeletons_render_mesh: {:.1} µs", mesh.render);
    held("skeletons_update_mesh", mesh.update, UPDATE_MAX);
    held("skeletons_extract_mesh", mesh.extract, EXTRACT_MAX);
    is_the_case("skeletons_extract_mesh", &mesh, 1);
    // Where the rest of the frame goes, of regions alone and with the mesh
    context("", &crowd);
    context("_mesh", &mesh);
    mesh_cost("skeleton_mesh_cost", &mesh, &crowd, crowd_size());
  }
  // The same under a drop shadow: a second mesh a skeleton, before its body
  let shadowed = measure(Case {
    shadow: true,
    ..meshed
  });
  if let (Some(mesh), Some(shadow)) = (shadowed, shadow) {
    println!("skeletons_extract_mesh_shadow: {:.1} µs", mesh.extract);
    held("skeletons_extract_mesh_shadow", mesh.extract, EXTRACT_MAX);
    is_the_case("skeletons_extract_mesh_shadow", &mesh, 2);
    println!("skeletons_render_shadow: {:.1} µs", shadow.render);
    println!("skeletons_render_mesh_shadow: {:.1} µs", mesh.render);
    context("_shadow", &shadow);
    context("_mesh_shadow", &mesh);
    mesh_cost(
      "skeleton_mesh_cost_shadow",
      &mesh,
      &shadow,
      2.0 * crowd_size(),
    );
  }
  // A source each run, so the first of each shader is new to the machine
  let salt = std::time::SystemTime::now()
    .duration_since(std::time::UNIX_EPOCH)
    .map_or(0, |since| since.subsec_nanos());
  for (name, case, salt) in [
    ("skeleton_first_shader_frame", Case::crowd(), salt),
    (
      "skeleton_first_shader_frame_mesh",
      meshed,
      salt ^ 0x2a5a_5a5a,
    ),
  ] {
    if let Some([new, cached, settled]) = first_shader(case, salt) {
      println!(
        "{name}: {new:.1} µs with a shader new to the machine, {cached:.1} µs with one the GPU's \
         cache holds, against {settled:.1} µs once it draws"
      );
    }
  }
  guards::verdict(
    "skeleton",
    "Profile a miss with the bench binary `cargo bench --bench skeleton --no-run` names.",
  );
  eprintln!("skeleton guards: every guard met its target");
}

/// How many skeletons the crowd holds
fn crowd_size() -> f64 {
  Case::crowd().skeletons as f64
}

/// Note a miss when `time` is over `limit`
fn held(name: &str, time: f64, limit: f64) {
  if time > limit {
    guards::miss(format!("{name} {time:.1} µs over {limit:.1} µs"));
  }
}
