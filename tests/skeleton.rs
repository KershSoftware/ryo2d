//! Skeletons in the world: a skeleton entity animates in the engine's fixed
//! step, its events and track events reach the game as messages, a game's
//! skin, attachments and bone overrides are its own, sockets keep pace with
//! their bones, and a reloaded file is followed by name.
//!
//! The engine is headless: tests skip when no GPU is available.

use std::f32::consts::FRAC_PI_2;
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use ryo2d::prelude::*;
use ryo2d::testing::{HeadlessEngine, capture_log};

/// A scene that does nothing: the tests place entities between steps
struct Stage;

impl Scene for Stage {}

/// An entity whose `GlobalTransform` [`probe`] records
#[derive(Component)]
struct Watch;

/// What [`probe`] saw inside the last fixed step, after the skeletons' set
/// and before particles: where the watched entities were, and each
/// skeleton's bones
#[derive(Resource, Default)]
struct InStep {
  globals: Vec<(Entity, Affine2)>,
  bones: Vec<(Entity, &'static str, Affine2)>,
}

impl InStep {
  fn global(&self, entity: Entity) -> Affine2 {
    self
      .globals
      .iter()
      .find(|(seen, _)| *seen == entity)
      .map(|(_, global)| *global)
      .expect("a watched entity")
  }

  fn bone(&self, entity: Entity, name: &str) -> Affine2 {
    self
      .bones
      .iter()
      .find(|(seen, bone, _)| *seen == entity && *bone == name)
      .map(|(_, _, world)| *world)
      .expect("a bone of a stepped skeleton")
  }
}

const PROBED: [&str; 3] = ["root", "arm", "hand"];

fn probe(
  seen: Option<ResMut<InStep>>,
  watched: Query<(Entity, &GlobalTransform), With<Watch>>,
  skeletons: Query<(Entity, &Skeleton)>,
) {
  let Some(mut seen) = seen else {
    return;
  };
  seen.globals.clear();
  seen.bones.clear();
  for (entity, global) in &watched {
    seen.globals.push((entity, global.affine()));
  }
  for (entity, skeleton) in &skeletons {
    for name in PROBED {
      if let Some(world) = skeleton.bone_world(name) {
        seen.bones.push((entity, name, world));
      }
    }
  }
}

fn builder() -> EngineBuilder {
  EngineBuilder::new()
    .resolution(64, 64)
    .add_systems(
      Phase::FixedUpdate,
      probe.after(FixedSet::Skeletons).before(FixedSet::Particles),
    )
    .start_scene(Stage)
}

fn start(builder: EngineBuilder) -> Option<HeadlessEngine> {
  let mut engine = HeadlessEngine::new_or_skip(builder)?;
  engine.world_mut().insert_resource(InStep::default());
  Some(engine)
}

fn engine() -> Option<HeadlessEngine> {
  start(builder())
}

const ARM: BoneId = BoneId(1);
const WEAPON: SlotId = SlotId(1);
/// The events' indices, in the order [`rig`] adds them
const HIT: usize = 0;
const STEP: usize = 1;

fn clip(name: &str, duration: f32, timelines: Vec<SkeletonTimeline>) -> SkeletonClip {
  SkeletonClip {
    name: Arc::from(name),
    duration,
    timelines,
  }
}

fn turn_arm(keys: &[(f32, f32)]) -> SkeletonTimeline {
  SkeletonTimeline::Rotate {
    bone: ARM,
    frames: keys
      .iter()
      .map(|&(time, radians)| (time, radians, SkeletonCurve::Linear))
      .collect(),
  }
}

fn point(x: f32) -> SkeletonAttachment {
  SkeletonAttachment::Point(PointAttachment {
    x,
    y: 0.0,
    rotation: 0.0,
  })
}

fn bounds(width: f32) -> SkeletonAttachment {
  SkeletonAttachment::BoundingBox(BoundingBoxAttachment {
    vertices: vec![Vec2::ZERO, Vec2::new(width, 0.0), Vec2::new(width, 10.0)],
  })
}

fn step_key(time: f32, int: i32) -> (f32, usize, SkeletonEventValues) {
  let values = SkeletonEventValues {
    int: Some(int),
    ..SkeletonEventValues::default()
  };
  (time, STEP, values)
}

/// A root, an arm 10 along it and a hand 20 along the arm. The hand's slot
/// "weapon" shows the point "open" (5 along the hand; 9 in the skin "long")
/// and can show "closed" (2 along); the root's slot "body" shows the
/// bounding box "hit" (10 wide; 20 in the skin "wide"). The animations are
/// named for what they do; no mix takes time
fn rig() -> SkeletonData {
  let arm = BoneSetup {
    x: 10.0,
    length: 20.0,
    ..BoneSetup::default()
  };
  let hand = BoneSetup {
    x: 20.0,
    ..BoneSetup::default()
  };
  let shows = |name: &str| SlotSetup {
    attachment: Some(Arc::from(name)),
    ..SlotSetup::default()
  };
  let hit = SkeletonEventData {
    int: 1,
    float: 2.5,
    string: Arc::from("left"),
    audio: Some(SkeletonEventAudio {
      path: Arc::from("audio/hit.ogg"),
      volume: 0.8,
      balance: -0.5,
    }),
    ..SkeletonEventData::new("hit")
  };
  let louder = SkeletonEventValues {
    int: Some(9),
    volume: Some(0.3),
    ..SkeletonEventValues::default()
  };
  SkeletonData::builder()
    .bone("root", None, BoneSetup::default())
    .bone("arm", Some("root"), arm)
    .bone("hand", Some("arm"), hand)
    .slot("body", "root", shows("hit"))
    .slot("weapon", "hand", shows("open"))
    .skin("long")
    .skin("wide")
    .attachment("default", "weapon", "open", point(5.0))
    .attachment("default", "weapon", "closed", point(2.0))
    .attachment("default", "body", "hit", bounds(10.0))
    .attachment("long", "weapon", "open", point(9.0))
    .attachment("wide", "body", "hit", bounds(20.0))
    .event(hit)
    .event(SkeletonEventData::new("step"))
    // The arm turns a quarter turn in a second
    .animation(clip(
      "swing",
      1.0,
      vec![turn_arm(&[(0.0, 0.0), (1.0, FRAC_PI_2)])],
    ))
    // Two keys every tenth of a second
    .animation(clip(
      "ticks",
      0.1,
      vec![SkeletonTimeline::Event {
        frames: vec![
          (0.0, STEP, SkeletonEventValues::default()),
          (0.03, HIT, louder),
        ],
      }],
    ))
    // Three keys inside one 60 Hz step
    .animation(clip(
      "burst",
      1.0,
      vec![SkeletonTimeline::Event {
        frames: vec![step_key(0.0, 1), step_key(0.005, 2), step_key(0.010, 3)],
      }],
    ))
    // One key between the burst's first two
    .animation(clip(
      "other",
      1.0,
      vec![SkeletonTimeline::Event {
        frames: vec![(0.002, HIT, SkeletonEventValues::default())],
      }],
    ))
    .animation(clip("short", 0.05, vec![turn_arm(&[(0.0, 0.5)])]))
    .animation(clip("idle", 1.0, vec![turn_arm(&[(0.0, 0.0)])]))
    .animation(clip(
      "grip",
      1.0,
      vec![SkeletonTimeline::Attachment {
        slot: WEAPON,
        frames: vec![(0.0, Some(Arc::from("closed")))],
      }],
    ))
    .build()
    .expect("a valid skeleton")
}

/// A skeleton entity of `data` at `transform`, with an animation state
fn spawn(engine: &mut HeadlessEngine, data: &Handle<SkeletonData>, transform: Transform) -> Entity {
  engine
    .world_mut()
    .spawn((
      transform,
      Skeleton::new(data.clone()),
      SkeletonAnimation::new(),
    ))
    .id()
}

fn skeleton(engine: &HeadlessEngine, entity: Entity) -> &Skeleton {
  engine.world().get::<Skeleton>(entity).expect("a skeleton")
}

fn skeleton_mut(engine: &mut HeadlessEngine, entity: Entity) -> Mut<'_, Skeleton> {
  engine
    .world_mut()
    .get_mut::<Skeleton>(entity)
    .expect("a skeleton")
}

fn animation(engine: &HeadlessEngine, entity: Entity) -> &SkeletonAnimation {
  engine
    .world()
    .get::<SkeletonAnimation>(entity)
    .expect("an animation state")
}

fn animation_mut(engine: &mut HeadlessEngine, entity: Entity) -> Mut<'_, SkeletonAnimation> {
  engine
    .world_mut()
    .get_mut::<SkeletonAnimation>(entity)
    .expect("an animation state")
}

fn play(
  engine: &mut HeadlessEngine,
  entity: Entity,
  track: usize,
  name: &str,
  looping: bool,
) -> TrackEntryId {
  animation_mut(engine, entity)
    .set_animation(track, name, looping)
    .expect("an animation of the rig")
}

/// The entry's track time
fn track_time(engine: &HeadlessEngine, entity: Entity, entry: TrackEntryId) -> f32 {
  animation(engine, entity)
    .entry(entry)
    .expect("a live entry")
    .track_time()
}

fn bone(engine: &HeadlessEngine, entity: Entity, name: &str) -> Affine2 {
  skeleton(engine, entity)
    .bone_world(name)
    .expect("a bone of a stepped skeleton")
}

fn global(engine: &HeadlessEngine, entity: Entity) -> Affine2 {
  engine
    .world()
    .get::<GlobalTransform>(entity)
    .expect("a placed entity")
    .affine()
}

fn in_step(engine: &HeadlessEngine) -> &InStep {
  engine.world().resource::<InStep>()
}

fn time(engine: &mut HeadlessEngine) -> Mut<'_, Time> {
  engine.world_mut().resource_mut::<Time>()
}

fn timestep(engine: &HeadlessEngine) -> f32 {
  engine.world().resource::<Time>().fixed.timestep()
}

fn fixed_steps(engine: &HeadlessEngine) -> u64 {
  engine.world().resource::<Time>().fixed.step_count()
}

/// The messages of one type written since it last took them
struct Inbox<T: Message> {
  cursor: MessageCursor<T>,
}

impl<T: Message + Clone> Inbox<T> {
  fn new(engine: &HeadlessEngine) -> Self {
    Self {
      cursor: engine.world().resource::<Messages<T>>().get_cursor(),
    }
  }

  fn take(&mut self, engine: &HeadlessEngine) -> Vec<T> {
    self
      .cursor
      .read(engine.world().resource::<Messages<T>>())
      .cloned()
      .collect()
  }

  /// Steps `steps` frames, taking each one's messages
  fn steps(&mut self, engine: &mut HeadlessEngine, steps: usize) -> Vec<T> {
    let mut taken = Vec::new();
    for _ in 0..steps {
      engine.step().expect("a frame");
      taken.extend(self.take(engine));
    }
    taken
  }
}

fn near(actual: Vec2, expected: Vec2) {
  assert!(
    (actual - expected).length() < 1e-3,
    "expected {expected}, got {actual}"
  );
}

fn close(actual: f32, expected: f32) {
  assert!(
    (actual - expected).abs() < 1e-4,
    "expected {expected}, got {actual}"
  );
}

fn same(actual: Affine2, expected: Affine2) {
  assert!(
    actual.abs_diff_eq(expected, 1e-4),
    "expected {expected:?}, got {actual:?}"
  );
}

/// The angle of a bone's x axis in the world
fn heading(world: Affine2) -> f32 {
  world.matrix2.x_axis.to_angle()
}

#[test]
fn a_socket_is_where_its_bone_is_in_the_same_step() {
  let Some(mut engine) = engine() else {
    return;
  };
  let data = engine.assets().add(rig());
  let place = Transform::from_xy(40.0, 30.0)
    .with_rotation(0.3)
    .with_uniform_scale(1.5);
  let body = spawn(&mut engine, &data, place);
  engine.world_mut().entity_mut(body).insert(Interpolated);
  play(&mut engine, body, 0, "swing", true);
  let offset = Transform::from_xy(4.0, 2.0).with_rotation(0.5);
  let socket = engine
    .world_mut()
    .spawn((
      BoneSocket {
        bone: "hand".to_owned(),
        offset,
        inherit_scale: true,
      },
      Watch,
      ChildOf(body),
    ))
    .id();
  let mut last = None;
  let mut last_at = None;
  for _ in 0..20 {
    // The entity moves and turns in this step, while its arm swings
    let moved = {
      let mut transform = engine
        .world_mut()
        .get_mut::<Transform>(body)
        .expect("the skeleton's transform");
      transform.translation += Vec2::new(3.0, -2.0);
      transform.rotation += 0.05;
      *transform
    };
    engine.step().expect("a frame");
    // Inside the step, before particles: the bones are this step's (the
    // root bone is the entity, where it now stands), and the socket is on
    // the hand
    let seen = in_step(&engine);
    same(seen.bone(body, "root"), moved.to_affine());
    let hand = seen.bone(body, "hand");
    let at = seen.global(socket);
    same(at, hand * offset.to_affine());
    assert_ne!(last, Some(hand), "the hand moved");
    last = Some(hand);
    // After the frame, propagation having run again, it is exactly there
    assert_eq!(global(&engine, socket), at);
    assert_eq!(bone(&engine, body, "hand"), hand);
    // It is drawn between steps as its skeleton's entity is: under an
    // interpolated entity, from where the last step's bone put it
    let previous = engine
      .world()
      .get::<PreviousGlobalTransform>(socket)
      .expect("a child of an interpolated entity");
    if let Some(before) = last_at {
      assert!(!previous.skip);
      assert_eq!(previous.global.affine(), before);
    }
    last_at = Some(at);
  }
}

#[test]
fn a_socket_child_s_children_follow_in_the_same_step() {
  let Some(mut engine) = engine() else {
    return;
  };
  let data = engine.assets().add(rig());
  let body = spawn(&mut engine, &data, Transform::from_xy(20.0, 10.0));
  play(&mut engine, body, 0, "swing", true);
  let socket = engine
    .world_mut()
    .spawn((BoneSocket::new("hand"), Watch, ChildOf(body)))
    .id();
  let held = Transform::from_xy(5.0, 0.0).with_rotation(0.2);
  let child = engine
    .world_mut()
    .spawn((held, Watch, ChildOf(socket)))
    .id();
  let charm = Transform::from_xy(0.0, 3.0).with_uniform_scale(2.0);
  let grandchild = engine
    .world_mut()
    .spawn((charm, Watch, ChildOf(child)))
    .id();
  let mut last = None;
  for _ in 0..10 {
    engine
      .world_mut()
      .get_mut::<Transform>(body)
      .expect("the skeleton's transform")
      .translation
      .x += 2.0;
    engine.step().expect("a frame");
    let seen = in_step(&engine);
    let hand = seen.bone(body, "hand");
    same(seen.global(socket), hand);
    same(seen.global(child), hand * held.to_affine());
    let at = seen.global(grandchild);
    same(at, hand * held.to_affine() * charm.to_affine());
    assert_ne!(last, Some(at), "the grandchild moved");
    last = Some(at);
    assert_eq!(global(&engine, grandchild), at);
  }
  // Propagation still sees a descendant's own changes: one moved between
  // steps is placed by its new transform in the next step
  let moved = Transform::from_xy(-4.0, 1.0);
  *engine
    .world_mut()
    .get_mut::<Transform>(grandchild)
    .expect("the grandchild's transform") = moved;
  engine.step().expect("a frame");
  let seen = in_step(&engine);
  let hand = seen.bone(body, "hand");
  same(
    seen.global(grandchild),
    hand * held.to_affine() * moved.to_affine(),
  );
  // And with the clock paused no step runs, yet a frame's propagation
  // places what moved: the child under the socket where the socket stands
  time(&mut engine).game.pause();
  let socket_at = global(&engine, socket);
  let shifted = Transform::from_xy(7.0, 7.0);
  *engine
    .world_mut()
    .get_mut::<Transform>(child)
    .expect("the child's transform") = shifted;
  engine.step().expect("a frame");
  assert_eq!(global(&engine, socket), socket_at);
  same(global(&engine, child), socket_at * shifted.to_affine());
  same(
    global(&engine, grandchild),
    socket_at * shifted.to_affine() * moved.to_affine(),
  );
}

#[test]
fn events_reach_the_game_as_messages_in_time_order() {
  let Some(mut engine) = engine() else {
    return;
  };
  let data = engine.assets().add(rig());
  let mut events = Inbox::<SkeletonEvent>::new(&engine);
  let body = spawn(&mut engine, &data, Transform::IDENTITY);
  play(&mut engine, body, 0, "burst", false);
  play(&mut engine, body, 1, "other", false);
  // One step crosses the burst's three keys and the other track's key
  // between the first two: they come in the order they happened
  let sent = events.steps(&mut engine, 1);
  let order: Vec<(usize, &str, &str, i32)> = sent
    .iter()
    .map(|event| (event.track, &*event.animation, &*event.name, event.int))
    .collect();
  assert_eq!(
    order,
    vec![
      (0, "burst", "step", 1),
      (1, "other", "hit", 1),
      (0, "burst", "step", 2),
      (0, "burst", "step", 3),
    ]
  );
  for (event, time) in sent.iter().zip([0.0, 0.002, 0.005, 0.010]) {
    assert_eq!(event.entity, body);
    close(event.time, time);
  }
  // A key with no values of its own carries its event's: the number, the
  // text and the sound for the game to play
  let hit = &sent[1];
  assert_eq!((hit.float, &*hit.string), (2.5, "left"));
  let audio = hit.audio.as_ref().expect("the event's audio");
  assert_eq!(
    (&*audio.path, audio.volume, audio.balance),
    ("audio/hit.ogg", 0.8, -0.5)
  );
  // And an event without any has none
  assert_eq!((sent[0].float, &*sent[0].string), (0.0, ""));
  assert!(sent[0].audio.is_none());
  assert!(events.steps(&mut engine, 3).is_empty());

  // A looping animation's keys come once a loop: over a quarter second,
  // three loops' two keys each (0 and 0.03 of every tenth), the second
  // with its key's own number and volume
  let looper = spawn(&mut engine, &data, Transform::IDENTITY);
  play(&mut engine, looper, 0, "ticks", true);
  let sent = events.steps(&mut engine, 15);
  let seen: Vec<(&str, i32)> = sent.iter().map(|event| (&*event.name, event.int)).collect();
  assert_eq!(
    seen,
    vec![
      ("step", 0),
      ("hit", 9),
      ("step", 0),
      ("hit", 9),
      ("step", 0),
      ("hit", 9)
    ]
  );
  for (event, time) in sent.iter().zip([0.0, 0.03].into_iter().cycle()) {
    assert_eq!(event.entity, looper);
    close(event.time, time);
  }
  let audio = sent[1].audio.as_ref().expect("the event's audio");
  assert_eq!((audio.volume, audio.balance), (0.3, -0.5));
}

#[test]
fn track_events_reach_the_game() {
  use TrackEventKind::*;
  let Some(mut engine) = engine() else {
    return;
  };
  let data = engine.assets().add(rig());
  let mut inbox = Inbox::<SkeletonTrackEvent>::new(&engine);
  let body = spawn(&mut engine, &data, Transform::IDENTITY);
  let mut sent = Vec::new();
  // "short" starts and, three steps on, completes
  let short = play(&mut engine, body, 0, "short", false);
  sent.extend(inbox.steps(&mut engine, 6));
  // "idle" replaces it: no mix, so "short" ends in the same step
  let idle = play(&mut engine, body, 0, "idle", true);
  sent.extend(inbox.steps(&mut engine, 2));
  // The track fades out through the empty animation, which "swing" then
  // replaces before the fade is over
  animation_mut(&mut engine, body).set_empty(0, 0.2);
  sent.extend(inbox.steps(&mut engine, 2));
  let swing = play(&mut engine, body, 0, "swing", true);
  sent.extend(inbox.steps(&mut engine, 2));
  let told: Vec<(&str, TrackEventKind)> = sent
    .iter()
    .map(|event| (&*event.animation, event.kind))
    .collect();
  let empty = SkeletonAnimation::EMPTY;
  assert_eq!(
    told,
    vec![
      ("short", Start),
      ("short", Complete),
      ("short", Interrupt),
      ("idle", Start),
      ("short", End),
      ("short", Dispose),
      ("idle", Interrupt),
      (empty, Start),
      (empty, Interrupt),
      ("swing", Start),
      ("idle", End),
      ("idle", Dispose),
      (empty, End),
      (empty, Dispose),
    ]
  );
  // Each says whose it is: the entity, the track and the entry's handle
  for event in &sent {
    assert_eq!((event.entity, event.track), (body, 0));
    let entry = match &*event.animation {
      "short" => Some(short),
      "idle" => Some(idle),
      "swing" => Some(swing),
      _ => None,
    };
    if let Some(entry) = entry {
      assert_eq!(event.entry, entry, "{event:?}");
    }
  }
  // The handles of entries that are gone find nothing
  let state = animation(&engine, body);
  assert!(state.entry(short).is_none() && state.entry(idle).is_none());
  assert_eq!(state.current(0), Some(swing));
}

#[test]
fn a_world_space_override_aims_a_bone_at_a_point() {
  let log = capture_log();
  let Some(mut engine) = engine() else {
    return;
  };
  let data = engine.assets().add(rig());
  let place = Transform::from_xy(30.0, 20.0)
    .with_rotation(0.4)
    .with_uniform_scale(1.5);
  let body = spawn(&mut engine, &data, place);
  play(&mut engine, body, 0, "idle", true);
  engine.step().expect("a frame");
  let rest = bone(&engine, body, "arm");
  close(heading(rest), 0.4);
  // The game aims the arm at a point in the world, from where the arm is
  let target = Vec2::new(-15.0, 60.0);
  let aim = (target - rest.translation).to_angle();
  let aimed = BoneOverride {
    rotation: Some(aim),
    space: BoneSpace::World,
    ..BoneOverride::default()
  };
  skeleton_mut(&mut engine, body).override_bone("arm", aimed);
  engine.step().expect("a frame");
  let arm = bone(&engine, body, "arm");
  near(arm.translation, rest.translation);
  near(
    arm.matrix2.x_axis.normalize(),
    (target - arm.translation).normalize(),
  );
  // The hand follows its parent, 20 along the arm at the entity's scale
  let hand = bone(&engine, body, "hand");
  near(
    hand.translation,
    arm.translation + (target - arm.translation).normalize() * 30.0,
  );
  // It is kept until cleared, and the entity turning does not turn the aim
  engine
    .world_mut()
    .get_mut::<Transform>(body)
    .expect("the skeleton's transform")
    .rotation = -1.0;
  engine.step().expect("a frame");
  close(heading(bone(&engine, body, "arm")), aim);
  // At half its mix it is half way from the animated pose to the aim
  engine
    .world_mut()
    .get_mut::<Transform>(body)
    .expect("the skeleton's transform")
    .rotation = 0.4;
  skeleton_mut(&mut engine, body).override_bone("arm", BoneOverride { mix: 0.5, ..aimed });
  engine.step().expect("a frame");
  close(heading(bone(&engine, body, "arm")), 0.4 + (aim - 0.4) * 0.5);
  // A world translation puts a bone at a point
  let grip = BoneOverride {
    translation: Some(Vec2::new(5.0, -40.0)),
    space: BoneSpace::World,
    ..BoneOverride::default()
  };
  skeleton_mut(&mut engine, body).override_bone("hand", grip);
  engine.step().expect("a frame");
  near(
    bone(&engine, body, "hand").translation,
    Vec2::new(5.0, -40.0),
  );
  // A local override sets the bone's own values: turned 0.7 from its
  // parent, at twice its length
  skeleton_mut(&mut engine, body).clear_override("hand");
  let turned = BoneOverride {
    rotation: Some(0.7),
    scale: Some(Vec2::new(2.0, 1.0)),
    ..BoneOverride::default()
  };
  skeleton_mut(&mut engine, body).override_bone("arm", turned);
  engine.step().expect("a frame");
  let arm = bone(&engine, body, "arm");
  close(heading(arm), 0.4 + 0.7);
  close(arm.matrix2.x_axis.length(), 3.0);
  // Cleared, the bone is the animation's again
  skeleton_mut(&mut engine, body).clear_override("arm");
  engine.step().expect("a frame");
  same(bone(&engine, body, "arm"), rest);
  // A value that is not a number is ignored, with one warning however
  // often it is set; the rest of the override applies
  for _ in 0..3 {
    let broken = BoneOverride {
      rotation: Some(f32::NAN),
      translation: Some(Vec2::new(12.0, 0.0)),
      ..BoneOverride::default()
    };
    skeleton_mut(&mut engine, body).override_bone("arm", broken);
    engine.step().expect("a frame");
  }
  let arm = bone(&engine, body, "arm");
  close(heading(arm), 0.4);
  near(
    arm.translation,
    place.to_affine().transform_point2(Vec2::new(12.0, 0.0)),
  );
  assert_eq!(log.containing("not finite"), 1, "{:?}", log.warnings());
  // A bone the skeleton lacks warns once per name and does nothing
  for _ in 0..3 {
    skeleton_mut(&mut engine, body).override_bone("tail", aimed);
    engine.step().expect("a frame");
  }
  assert_eq!(log.containing("\"tail\""), 1, "{:?}", log.warnings());
}

#[test]
fn a_paused_clock_freezes_animation_and_events() {
  let Some(mut engine) = engine() else {
    return;
  };
  let data = engine.assets().add(rig());
  let mut events = Inbox::<SkeletonEvent>::new(&engine);
  let mut track_events = Inbox::<SkeletonTrackEvent>::new(&engine);
  let body = spawn(&mut engine, &data, Transform::IDENTITY);
  let ticks = play(&mut engine, body, 0, "ticks", true);
  let swing = play(&mut engine, body, 1, "swing", true);
  assert!(!events.steps(&mut engine, 10).is_empty());
  track_events.take(&engine);
  let at = track_time(&engine, body, ticks);
  let arm = bone(&engine, body, "arm");
  let steps = fixed_steps(&engine);

  // Paused, the engine runs no fixed step: nothing advances and nothing is
  // sent, not even for an animation set meanwhile
  time(&mut engine).game.pause();
  let burst = play(&mut engine, body, 2, "burst", false);
  assert!(events.steps(&mut engine, 30).is_empty());
  assert!(track_events.take(&engine).is_empty());
  assert_eq!(fixed_steps(&engine), steps);
  assert_eq!(track_time(&engine, body, ticks), at);
  assert_eq!(track_time(&engine, body, burst), 0.0);
  assert_eq!(bone(&engine, body, "arm"), arm);

  // Resumed, it carries on from where it was, and what was set starts
  time(&mut engine).game.resume();
  let sent = events.steps(&mut engine, 1);
  let dt = timestep(&engine);
  close(track_time(&engine, body, ticks), at + dt);
  assert_ne!(bone(&engine, body, "arm"), arm);
  assert!(
    sent
      .iter()
      .any(|event| &*event.animation == "burst" && event.int == 1),
    "{sent:?}"
  );
  let started = track_events.take(&engine);
  assert_eq!(started.len(), 1, "{started:?}");
  assert_eq!(
    (started[0].entry, started[0].kind),
    (burst, TrackEventKind::Start)
  );

  // Hitstop is a pause too
  events.steps(&mut engine, 2);
  let at = track_time(&engine, body, swing);
  time(&mut engine).game.hitstop(10.0);
  assert!(events.steps(&mut engine, 10).is_empty());
  assert_eq!(track_time(&engine, body, swing), at);
  time(&mut engine).game.clear_hitstop();

  // A skeleton's own time scale at 0 stops it while the engine steps on:
  // an entry set then starts, fires its keys at time 0 once, and waits
  animation_mut(&mut engine, body).time_scale = 0.0;
  events.steps(&mut engine, 1);
  track_events.take(&engine);
  let again = play(&mut engine, body, 2, "burst", false);
  let (mut sent, mut told) = (Vec::new(), Vec::new());
  for _ in 0..5 {
    sent.extend(events.steps(&mut engine, 1));
    told.extend(track_events.take(&engine));
  }
  let ints: Vec<i32> = sent.iter().map(|event| event.int).collect();
  assert_eq!(ints, vec![1], "{sent:?}");
  let told: Vec<TrackEventKind> = told
    .iter()
    .filter(|event| event.entry == again)
    .map(|event| event.kind)
    .collect();
  assert_eq!(told, vec![TrackEventKind::Start]);
  assert_eq!(track_time(&engine, body, again), 0.0);
  assert_eq!(track_time(&engine, body, swing), at);
}

const PATH: &str = "rigs/hero.skeleton.ron";

/// The rig as a file: the bones written in `bones`, the slot "weapon" on
/// `weapon_bone`, and the animations written in `animations`
fn rig_file(bones: &str, weapon_bone: &str, animations: &str) -> String {
  format!(
    r#"(
      bones: [{bones}],
      slots: [(name: "weapon", bone: "{weapon_bone}", attachment: "open")],
      skins: {{
        "default": {{ "weapon": {{ "open": Point(x: 5), "closed": Point(x: 2) }} }},
        "long": {{ "weapon": {{ "open": Point(x: 9) }} }},
      }},
      events: {{ "step": () }},
      animations: {{ {animations} }},
    )"#
  )
}

const BONES: &str =
  r#"(name: "root"), (name: "arm", parent: "root", x: 10), (name: "hand", parent: "arm", x: 20)"#;
/// The arm turns a quarter turn in a second, with a key at its start
const WALK: &str = r#""walk": (duration: 1.0, bones: { "arm": (rotate: [(0.0, 0.0), (1.0, 90.0)]) }, events: [(0.0, "step")])"#;
const IDLE: &str = r#""idle": (duration: 1.0)"#;

fn first_file() -> String {
  rig_file(BONES, "hand", &format!("{WALK}, {IDLE}"))
}

/// An engine over `files`, with the skeleton at [`PATH`] loaded
fn engine_over(files: &MemorySource) -> Option<(HeadlessEngine, Handle<SkeletonData>)> {
  let mut engine = start(builder().asset_source(files.clone()))?;
  let data = engine.assets().load::<SkeletonData>(PATH);
  frames_until(&mut engine, |engine| {
    engine
      .world()
      .resource::<Assets<SkeletonData>>()
      .get(&data)
      .is_some()
  });
  Some((engine, data))
}

fn version(engine: &HeadlessEngine, data: &Handle<SkeletonData>) -> Option<u64> {
  engine
    .world()
    .resource::<Assets<SkeletonData>>()
    .version(data)
}

/// Runs frames until `done`, for at most 5 s
fn frames_until(engine: &mut HeadlessEngine, done: impl Fn(&HeadlessEngine) -> bool) {
  let start = Instant::now();
  while !done(engine) {
    assert!(
      start.elapsed() < Duration::from_secs(5),
      "timed out waiting"
    );
    std::thread::sleep(Duration::from_millis(1));
    engine.step().expect("a frame");
  }
}

/// Saves `text` over the skeleton's file and waits for the reload to land,
/// with the game clock paused so that no step runs meanwhile
fn save(
  engine: &mut HeadlessEngine,
  files: &MemorySource,
  data: &Handle<SkeletonData>,
  text: String,
) {
  let before = version(engine, data);
  time(engine).game.pause();
  files.insert(PATH, text);
  engine.world().resource::<AssetServer>().reload(PATH);
  frames_until(engine, |engine| version(engine, data) != before);
  time(engine).game.resume();
}

#[test]
fn a_reload_keeps_the_playing_animation_and_its_time() {
  let log = capture_log();
  let files = MemorySource::new();
  files.insert(PATH, first_file());
  let Some((mut engine, data)) = engine_over(&files) else {
    return;
  };
  let mut track_events = Inbox::<SkeletonTrackEvent>::new(&engine);
  let body = spawn(&mut engine, &data, Transform::from_xy(8.0, 4.0));
  let walk = play(&mut engine, body, 0, "walk", true);
  let long = BoneOverride {
    scale: Some(Vec2::new(2.0, 1.0)),
    ..BoneOverride::default()
  };
  {
    let mut skeleton = skeleton_mut(&mut engine, body);
    skeleton.set_skin("long");
    skeleton.override_bone("arm", long);
  }
  engine.step_frames(12).expect("frames");
  let at = track_time(&engine, body, walk);
  let dt = timestep(&engine);
  close(at, 12.0 * dt);
  let turned = heading(bone(&engine, body, "arm"));
  assert!(skeleton(&engine, body).bone_world("tail").is_none());
  track_events.take(&engine);

  // The saved file adds a bone before the arm (so every later bone's place
  // in the list moves), and sets the arm further out
  let bones = r#"(name: "root"), (name: "tail", parent: "root", x: -6),
    (name: "arm", parent: "root", x: 14), (name: "hand", parent: "arm", x: 20)"#;
  save(
    &mut engine,
    &files,
    &data,
    rig_file(bones, "hand", &format!("{WALK}, {IDLE}")),
  );
  // Landed while paused: nothing has stepped, so the skeleton is as it was
  assert_eq!(track_time(&engine, body, walk), at);
  engine.step().expect("a frame");
  // The entry plays on from its time, and the pose is the new skeleton's
  let state = animation(&engine, body);
  assert_eq!(state.current(0), Some(walk));
  close(track_time(&engine, body, walk), at + dt);
  let arm = bone(&engine, body, "arm");
  near(arm.translation, Vec2::new(8.0 + 14.0, 4.0));
  assert!(heading(arm) > turned);
  near(
    bone(&engine, body, "tail").translation,
    Vec2::new(8.0 - 6.0, 4.0),
  );
  // The override and the skin are kept by name: the arm is twice as long,
  // and the weapon's point is the long skin's, 9 along the hand
  close(arm.matrix2.x_axis.length(), 2.0);
  let hand = bone(&engine, body, "hand");
  near(
    tip(&engine, body, "open"),
    hand.transform_point2(Vec2::new(9.0, 0.0)),
  );
  // Nothing was said, and no track event sent
  assert!(track_events.take(&engine).is_empty());
  assert!(
    log.from_target("ryo2d::core::skeleton").is_empty(),
    "{:?}",
    log.warnings()
  );
}

#[test]
fn a_reload_that_removes_the_playing_animation_clears_the_track_with_one_warning() {
  let log = capture_log();
  let files = MemorySource::new();
  files.insert(PATH, first_file());
  let Some((mut engine, data)) = engine_over(&files) else {
    return;
  };
  let mut track_events = Inbox::<SkeletonTrackEvent>::new(&engine);
  let body = spawn(&mut engine, &data, Transform::IDENTITY);
  let walk = play(&mut engine, body, 0, "walk", true);
  let idle = play(&mut engine, body, 1, "idle", true);
  engine.step_frames(30).expect("frames");
  assert!(heading(bone(&engine, body, "arm")) > 0.5);
  track_events.take(&engine);

  save(&mut engine, &files, &data, rig_file(BONES, "hand", IDLE));
  let sent = track_events.steps(&mut engine, 4);
  // The walk's track is cleared and its entry gone; the other track plays on
  let state = animation(&engine, body);
  assert_eq!(state.current(0), None);
  assert!(state.entry(walk).is_none());
  assert_eq!(state.current(1), Some(idle));
  let told: Vec<(&str, TrackEventKind)> = sent
    .iter()
    .map(|event| (&*event.animation, event.kind))
    .collect();
  assert_eq!(
    told,
    vec![
      ("walk", TrackEventKind::End),
      ("walk", TrackEventKind::Dispose)
    ]
  );
  // The arm is back in its setup pose
  close(heading(bone(&engine, body, "arm")), 0.0);
  // One warning, naming the animation: not one a step, nor one more when
  // the file is saved again without it
  save(&mut engine, &files, &data, rig_file(BONES, "hand", IDLE));
  engine.step_frames(4).expect("frames");
  assert_eq!(log.containing("\"walk\""), 1, "{:?}", log.warnings());
}

#[test]
fn a_reload_that_removes_a_socket_s_bone_warns_once_and_keeps_playing() {
  let log = capture_log();
  let files = MemorySource::new();
  files.insert(PATH, first_file());
  let Some((mut engine, data)) = engine_over(&files) else {
    return;
  };
  let body = spawn(&mut engine, &data, Transform::from_xy(8.0, 4.0));
  let walk = play(&mut engine, body, 0, "walk", true);
  let socket = engine
    .world_mut()
    .spawn((BoneSocket::new("hand"), ChildOf(body)))
    .id();
  // A socket that never had a bone to follow, where the game put it
  let lost = engine
    .world_mut()
    .spawn((
      Transform::from_xy(1.0, 2.0),
      BoneSocket::new("wing"),
      ChildOf(body),
    ))
    .id();
  engine.step_frames(12).expect("frames");
  same(global(&engine, socket), bone(&engine, body, "hand"));
  near(global(&engine, lost).translation, Vec2::new(9.0, 6.0));
  assert_eq!(log.containing("\"wing\""), 1, "{:?}", log.warnings());
  let held = *engine
    .world()
    .get::<Transform>(socket)
    .expect("the socket's transform");
  let at = track_time(&engine, body, walk);

  // The saved file has no hand: the weapon's slot moves to the arm
  let bones = r#"(name: "root"), (name: "arm", parent: "root", x: 10)"#;
  let handless = rig_file(bones, "arm", &format!("{WALK}, {IDLE}"));
  save(&mut engine, &files, &data, handless.clone());
  engine.step_frames(6).expect("frames");
  // The animation plays on
  let dt = timestep(&engine);
  assert_eq!(animation(&engine, body).current(0), Some(walk));
  close(track_time(&engine, body, walk), at + 6.0 * dt);
  assert!(skeleton(&engine, body).bone_world("hand").is_none());
  // The socket stays where it was on the skeleton, and says so once: not
  // each step, nor again when the file is saved again
  assert_eq!(
    *engine
      .world()
      .get::<Transform>(socket)
      .expect("the socket's transform"),
    held
  );
  save(&mut engine, &files, &data, handless);
  engine.step_frames(3).expect("frames");
  assert_eq!(log.containing("\"hand\""), 1, "{:?}", log.warnings());
  assert_eq!(log.containing("\"wing\""), 1, "{:?}", log.warnings());

  // With the bone back the socket follows again, and nothing more is said
  save(&mut engine, &files, &data, first_file());
  engine.step_frames(3).expect("frames");
  same(global(&engine, socket), bone(&engine, body, "hand"));
  assert_eq!(log.containing("\"hand\""), 1, "{:?}", log.warnings());
}

/// A source that holds every read until it is opened, so a load stays on
/// its way for as long as a test wants
#[derive(Clone)]
struct Gated {
  files: MemorySource,
  open: Arc<(Mutex<bool>, Condvar)>,
}

impl Gated {
  fn open(&self) {
    let (open, opened) = &*self.open;
    *open.lock().expect("the gate") = true;
    opened.notify_all();
  }
}

impl AssetSource for Gated {
  fn read(&self, path: &AssetPath) -> Result<Vec<u8>, AssetError> {
    let (open, opened) = &*self.open;
    // Bounded, so a failing test cannot leave a worker waiting for ever
    let wait = Duration::from_secs(10);
    let closed = open.lock().expect("the gate");
    drop(opened.wait_timeout_while(closed, wait, |open| !*open));
    self.files.read(path)
  }

  fn exists(&self, path: &AssetPath) -> bool {
    self.files.exists(path)
  }
}

#[test]
fn a_skeleton_waits_for_its_data_and_keeps_the_animation_set_meanwhile() {
  let log = capture_log();
  let files = MemorySource::new();
  files.insert(PATH, first_file());
  let gate = Gated {
    files,
    open: Arc::default(),
  };
  let Some(mut engine) = start(builder().asset_source(gate.clone())) else {
    return;
  };
  let mut events = Inbox::<SkeletonEvent>::new(&engine);
  let mut track_events = Inbox::<SkeletonTrackEvent>::new(&engine);
  // The load is on its way, and stays there
  let data = engine.assets().load::<SkeletonData>(PATH);
  let body = spawn(&mut engine, &data, Transform::from_xy(8.0, 4.0));
  let socket = engine
    .world_mut()
    .spawn((
      Transform::from_xy(1.0, 2.0),
      BoneSocket::new("hand"),
      ChildOf(body),
    ))
    .id();
  // The game sets everything up at once, as if the skeleton were there
  let walk = play(&mut engine, body, 0, "walk", true);
  let long = BoneOverride {
    scale: Some(Vec2::new(2.0, 1.0)),
    ..BoneOverride::default()
  };
  {
    let mut skeleton = skeleton_mut(&mut engine, body);
    skeleton.set_skin("long");
    skeleton.override_bone("arm", long);
    skeleton.set_attachment("weapon", Some("open"));
  }
  for _ in 0..10 {
    engine.step().expect("a frame");
    assert!(version(&engine, &data).is_none(), "still loading");
    // It has no bones yet, and nothing to say
    let waiting = skeleton(&engine, body);
    assert!(waiting.bone_world("arm").is_none());
    assert!(waiting.point("open").is_none());
    assert!(!waiting.bounding_box("hit", &mut Vec::new()));
    assert!(events.take(&engine).is_empty());
    assert!(track_events.take(&engine).is_empty());
    // The entry is kept, not started
    let state = animation(&engine, body);
    assert_eq!(state.current(0), Some(walk));
    assert_eq!(track_time(&engine, body, walk), 0.0);
    // And the socket is where the game put it
    near(global(&engine, socket).translation, Vec2::new(9.0, 6.0));
  }
  assert!(
    log.from_target("ryo2d::core::skeleton").is_empty(),
    "{:?}",
    log.warnings()
  );

  // The file arrives (with the clock paused, so the step it starts in is
  // the next one)
  time(&mut engine).game.pause();
  gate.open();
  frames_until(&mut engine, |engine| version(engine, &data).is_some());
  time(&mut engine).game.resume();
  engine.step().expect("a frame");
  // The animation set meanwhile starts: its Start, and its key at time 0
  let started = track_events.take(&engine);
  assert_eq!(started.len(), 1, "{started:?}");
  assert_eq!(
    (started[0].entry, &*started[0].animation, started[0].kind),
    (walk, "walk", TrackEventKind::Start)
  );
  let sent = events.take(&engine);
  assert_eq!(sent.len(), 1, "{sent:?}");
  assert_eq!((&*sent[0].name, sent[0].entity), ("step", body));
  let dt = timestep(&engine);
  close(track_time(&engine, body, walk), dt);
  // The pose is there, with the override, the skin and the socket in place
  let arm = bone(&engine, body, "arm");
  near(arm.translation, Vec2::new(18.0, 4.0));
  close(arm.matrix2.x_axis.length(), 2.0);
  assert!(heading(arm) > 0.0);
  let hand = bone(&engine, body, "hand");
  near(
    tip(&engine, body, "open"),
    hand.transform_point2(Vec2::new(9.0, 0.0)),
  );
  same(global(&engine, socket), hand);
  assert!(
    log.from_target("ryo2d::core::skeleton").is_empty(),
    "{:?}",
    log.warnings()
  );
}

#[test]
fn skeletons_sharing_data_keep_their_own_skin_overrides_and_attachments() {
  let log = capture_log();
  let Some(mut engine) = engine() else {
    return;
  };
  let data = engine.assets().add(rig());
  let made = version(&engine, &data);
  let plain = spawn(&mut engine, &data, Transform::from_xy(0.0, 0.0));
  let dressed = spawn(&mut engine, &data, Transform::from_xy(100.0, 0.0));
  let armed = spawn(&mut engine, &data, Transform::from_xy(200.0, 0.0));
  // One wears two skins and bends its arm; another closes its hand
  {
    let mut skeleton = skeleton_mut(&mut engine, dressed);
    skeleton.set_skins(&["long", "wide"]);
    let bent = BoneOverride {
      rotation: Some(FRAC_PI_2),
      ..BoneOverride::default()
    };
    skeleton.override_bone("arm", bent);
  }
  skeleton_mut(&mut engine, armed).set_attachment("weapon", Some("closed"));
  engine.step_frames(3).expect("frames");

  // The plain one: the setup pose, the default skin, the setup attachment
  // (root, arm 10 along, hand 20 further, the point 5 further)
  near(tip(&engine, plain, "open"), Vec2::new(35.0, 0.0));
  assert!(skeleton(&engine, plain).point("closed").is_none());
  assert_eq!(
    box_of(&engine, plain),
    vec![
      Vec2::new(0.0, 0.0),
      Vec2::new(10.0, 0.0),
      Vec2::new(10.0, 10.0)
    ]
  );
  // The dressed one: both skins' attachments (the point 9 along the hand,
  // the box 20 wide), on an arm bent a quarter turn down from (110, 0)
  near(tip(&engine, dressed, "open"), Vec2::new(110.0, 29.0));
  assert_eq!(
    box_of(&engine, dressed),
    vec![
      Vec2::new(100.0, 0.0),
      Vec2::new(120.0, 0.0),
      Vec2::new(120.0, 10.0)
    ]
  );
  // The armed one: the closed hand's point, 2 along, and no open one
  near(tip(&engine, armed, "closed"), Vec2::new(232.0, 0.0));
  assert!(skeleton(&engine, armed).point("open").is_none());
  assert_eq!(box_of(&engine, armed).len(), 3);
  // A point's direction is its bone's: the bent arm's hand points down
  let (_, direction) = skeleton(&engine, dressed).point("open").expect("its point");
  close(direction, FRAC_PI_2);

  // One skin replaces the set
  skeleton_mut(&mut engine, dressed).set_skin("wide");
  engine.step().expect("a frame");
  near(tip(&engine, dressed, "open"), Vec2::new(110.0, 25.0));
  // A skin the data lacks warns once per name and changes nothing
  for _ in 0..3 {
    skeleton_mut(&mut engine, dressed).set_skin("gold");
    engine.step().expect("a frame");
  }
  assert_eq!(box_of(&engine, dressed)[1], Vec2::new(120.0, 0.0));
  // In a set it is left out, and the others are worn
  for _ in 0..3 {
    skeleton_mut(&mut engine, dressed).set_skins(&["long", "gold"]);
    engine.step().expect("a frame");
  }
  near(tip(&engine, dressed, "open"), Vec2::new(110.0, 29.0));
  assert_eq!(box_of(&engine, dressed)[1], Vec2::new(110.0, 0.0));
  assert_eq!(log.containing("\"gold\""), 1, "{:?}", log.warnings());
  // A slot the data lacks likewise
  for _ in 0..3 {
    skeleton_mut(&mut engine, armed).set_attachment("tail", Some("open"));
    engine.step().expect("a frame");
  }
  assert_eq!(log.containing("\"tail\""), 1, "{:?}", log.warnings());
  // The shared data is as it was made
  assert_eq!(version(&engine, &data), made);
  near(tip(&engine, plain, "open"), Vec2::new(35.0, 0.0));
}

/// Where the point attachment `name` that a slot of `entity` shows is
fn tip(engine: &HeadlessEngine, entity: Entity, name: &str) -> Vec2 {
  let (at, _) = skeleton(engine, entity)
    .point(name)
    .expect("a point a slot shows");
  at
}

/// The corners of the bounding box "hit" that a slot of `entity` shows
fn box_of(engine: &HeadlessEngine, entity: Entity) -> Vec<Vec2> {
  let mut corners = Vec::new();
  assert!(skeleton(engine, entity).bounding_box("hit", &mut corners));
  corners
}

#[test]
fn a_game_set_attachment_beats_animation_until_it_is_handed_back() {
  let Some(mut engine) = engine() else {
    return;
  };
  let data = engine.assets().add(rig());
  let body = spawn(&mut engine, &data, Transform::IDENTITY);
  let shows = |engine: &HeadlessEngine| {
    let skeleton = skeleton(engine, body);
    (
      skeleton.point("open").is_some(),
      skeleton.point("closed").is_some(),
    )
  };
  // "grip" keys the weapon's slot to "closed" every step
  play(&mut engine, body, 0, "grip", true);
  engine.step_frames(2).expect("frames");
  assert_eq!(shows(&engine), (false, true));
  // What the game sets wins, for as long as it is set
  skeleton_mut(&mut engine, body).set_attachment("weapon", Some("open"));
  engine.step_frames(5).expect("frames");
  assert_eq!(shows(&engine), (true, false));
  // Setting none hides the slot
  skeleton_mut(&mut engine, body).set_attachment("weapon", None);
  engine.step_frames(2).expect("frames");
  assert_eq!(shows(&engine), (false, false));
  // A name no skin has in the slot shows nothing, without a warning
  skeleton_mut(&mut engine, body).set_attachment("weapon", Some("hit"));
  engine.step_frames(2).expect("frames");
  assert_eq!(shows(&engine), (false, false));
  // Handed back, the slot is the animation's again
  skeleton_mut(&mut engine, body).clear_attachment("weapon");
  engine.step_frames(2).expect("frames");
  assert_eq!(shows(&engine), (false, true));
  // And with no animation keying it, its setup attachment's
  animation_mut(&mut engine, body).clear(0);
  engine.step_frames(2).expect("frames");
  assert_eq!(shows(&engine), (true, false));
}

#[test]
fn a_despawned_skeleton_sends_nothing_more() {
  let Some(mut engine) = engine() else {
    return;
  };
  let data = engine.assets().add(rig());
  let mut events = Inbox::<SkeletonEvent>::new(&engine);
  let mut track_events = Inbox::<SkeletonTrackEvent>::new(&engine);
  let stays = spawn(&mut engine, &data, Transform::IDENTITY);
  let goes = spawn(&mut engine, &data, Transform::IDENTITY);
  play(&mut engine, stays, 0, "ticks", true);
  play(&mut engine, goes, 0, "ticks", true);
  let sent = events.steps(&mut engine, 12);
  assert!(sent.iter().any(|event| event.entity == stays));
  assert!(sent.iter().any(|event| event.entity == goes));
  track_events.take(&engine);
  // A call whose track events were still waiting for the next step
  play(&mut engine, goes, 0, "swing", true);
  engine.world_mut().despawn(goes);
  // The other one's keys and loops go on being told; nothing is of the
  // one that went
  let (mut sent, mut told) = (Vec::new(), Vec::new());
  for _ in 0..24 {
    sent.extend(events.steps(&mut engine, 1));
    told.extend(track_events.take(&engine));
  }
  assert!(!sent.is_empty() && !told.is_empty());
  assert!(sent.iter().all(|event| event.entity == stays), "{sent:?}");
  assert!(told.iter().all(|event| event.entity == stays), "{told:?}");
}
