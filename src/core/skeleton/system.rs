//! The skeletons' part of the fixed step: every skeleton poses its bones,
//! its sockets are put on them, and its events are sent. The module notes
//! give the order and the rules.

use super::component::{Fired, Skeleton};
use super::data::{DataVersion, SkeletonData};
use super::event::{SkeletonEvent, SkeletonTrackEvent};
use super::socket::{BoneSocket, SocketState};
use super::state::SkeletonAnimation;
use crate::core::asset::Assets;
use crate::core::ecs::{
  Children, DetectChanges, DetectChangesMut, Entity, GlobalTransform, Has, Local, MessageWriter,
  Mut, PreviousGlobalTransform, Query, Ref, Res, Transform, entity_order,
};
use crate::core::time::Time;

type Skeletons<'w, 's> = Query<
  'w,
  's,
  (
    Entity,
    &'static mut Skeleton,
    Option<&'static mut SkeletonAnimation>,
    Option<&'static Children>,
  ),
>;

/// Whatever a socket's placing can move: the socket, and everything below
/// it
type Placed<'w, 's> = Query<
  'w,
  's,
  (
    &'static mut Transform,
    &'static mut GlobalTransform,
    Option<&'static mut PreviousGlobalTransform>,
    Option<&'static Children>,
    Option<(Ref<'static, BoneSocket>, &'static mut SocketState)>,
    Has<Skeleton>,
  ),
>;

/// Where a skeleton's entity stands, and whether it is drawn there at once
/// (a teleport: its `PreviousGlobalTransform` skips)
fn standing(placed: &Placed, entity: Entity) -> Option<(GlobalTransform, bool)> {
  let (_, global, previous, ..) = placed.get(entity).ok()?;
  Some((*global, previous.is_some_and(|previous| previous.skip)))
}

/// What the step keeps between runs, so a steady step allocates nothing:
/// the skeletons in entity order, what the one being stepped fired, the
/// walk below a socket, and the skeletons a socket moved after their step
#[derive(Default)]
pub(crate) struct StepState {
  order: Vec<(u32, Entity)>,
  fired: Fired,
  walk: Vec<(Entity, GlobalTransform)>,
  moved: Vec<Entity>,
}

/// The fixed step's skeletons (`FixedSet::Skeletons`: after propagation,
/// so each entity's `GlobalTransform` is this step's, and before
/// particles). Each enabled skeleton whose data has loaded, in entity
/// order: its animation state advances by the step and poses it, the
/// game's attachments and bone overrides are put over that, its bones'
/// world transforms are computed under its entity's, its sockets (and what
/// is below them) are placed on their bones, and its events and track
/// events are written. One whose data is still loading waits, silently
pub(crate) fn update_skeletons(
  time: Option<Res<Time>>,
  data: Option<Res<Assets<SkeletonData>>>,
  mut skeletons: Skeletons,
  mut placed: Placed,
  mut events: MessageWriter<SkeletonEvent>,
  mut track_events: MessageWriter<SkeletonTrackEvent>,
  mut state: Local<StepState>,
) {
  crate::profile_scope!("cpu:skeletons");
  let (Some(time), Some(data)) = (time, data) else {
    return;
  };
  let dt = time.fixed.timestep();
  let StepState {
    order,
    fired,
    walk,
    moved,
  } = &mut *state;
  // Entity order, so the messages of several skeletons come in the same
  // order whatever the world's layout
  order.clear();
  order.extend(skeletons.iter().map(|(entity, ..)| entity_order(&entity)));
  order.sort_unstable();
  moved.clear();
  for &(_, entity) in order.iter() {
    let Ok((entity, mut skeleton, animation, children)) = skeletons.get_mut(entity) else {
      continue;
    };
    // Data still on its way has no version: there is nothing to pose yet
    let id = skeleton.data().id();
    let (Some(version), Some(rig)) = (data.version(id), data.get(id)) else {
      continue;
    };
    let Some((root, teleported)) = standing(&placed, entity) else {
      continue;
    };
    fired.events.clear();
    fired.track_events.clear();
    let animation = animation.map(Mut::into_inner);
    skeleton.step(rig, (id, version), animation, dt, root.affine(), fired);
    let mut sockets = Sockets {
      placed: &mut placed,
      walk,
      moved,
      entity,
      seen: (id, version),
      root,
      teleported,
      warn: true,
    };
    sockets.place_all(&skeleton, children);
    events.write_batch(
      fired
        .events
        .drain(..)
        .filter_map(|event| SkeletonEvent::new(entity, rig, event)),
    );
    track_events.write_batch(
      fired
        .track_events
        .drain(..)
        .map(|event| SkeletonTrackEvent::new(entity, event)),
    );
  }
  // A skeleton on another's socket was posed where it stood before the
  // socket moved it, if its turn came first: its bones are computed again
  // where it now stands, and its own sockets follow. One so moved may move
  // another in turn. A hierarchy has no cycles, so the list ends; the bound
  // is against one that has
  let most = order.len().saturating_mul(order.len()).saturating_add(1);
  let mut next = 0;
  while let Some(&entity) = moved.get(next).filter(|_| next < most) {
    next += 1;
    let Ok((entity, mut skeleton, _, children)) = skeletons.get_mut(entity) else {
      continue;
    };
    let id = skeleton.data().id();
    let (Some(version), Some(rig)) = (data.version(id), data.get(id)) else {
      continue;
    };
    let Some((root, teleported)) = standing(&placed, entity) else {
      continue;
    };
    // Only one this step posed, standing elsewhere than it was posed for
    if skeleton.seen() != Some((id, version)) || skeleton.root() == root.affine() {
      continue;
    }
    skeleton.place_again(rig, root.affine());
    let mut sockets = Sockets {
      placed: &mut placed,
      walk,
      moved,
      entity,
      seen: (id, version),
      root,
      teleported,
      warn: false,
    };
    sockets.place_all(&skeleton, children);
  }
}

/// Placing one skeleton's sockets
struct Sockets<'a, 'w, 's> {
  placed: &'a mut Placed<'w, 's>,
  walk: &'a mut Vec<(Entity, GlobalTransform)>,
  moved: &'a mut Vec<Entity>,
  /// The skeleton's entity
  entity: Entity,
  /// The data's id and version its pose was made from
  seen: DataVersion,
  /// The skeleton's entity's transform
  root: GlobalTransform,
  /// Whether the skeleton's entity is drawn where it stands at once: its
  /// sockets, and what is below them, are then too
  teleported: bool,
  /// Whether a socket whose bone is missing is told: not when its
  /// skeleton is placed a second time in a step
  warn: bool,
}

impl Sockets<'_, '_, '_> {
  /// Puts each of `skeleton`'s children that is a socket on its bone
  fn place_all(&mut self, skeleton: &Skeleton, children: Option<&Children>) {
    for child in children.into_iter().flatten().copied() {
      self.place(skeleton, child);
    }
  }

  /// Puts the socket `child` on its bone: its `Transform` is the bone's in
  /// the skeleton's space with the socket's offset, and its
  /// `GlobalTransform` is the skeleton's entity's times that, which is
  /// what propagation makes of the two, so whichever writes it last writes
  /// the same. Then everything below it is placed, as propagation would
  /// place it. A socket put on a bone after not standing on one (a new
  /// socket, its skeleton's first pose, a bone found again), or whose
  /// skeleton teleported, is drawn there at once, with all below it
  fn place(&mut self, skeleton: &Skeleton, child: Entity) {
    let Ok((mut transform, mut global, previous, children, socket, is_skeleton)) =
      self.placed.get_mut(child)
    else {
      return;
    };
    let Some((socket, mut state)) = socket else {
      return;
    };
    // The bone's name is looked up once a version of the data, and again
    // when the game changes the socket
    let known = !socket.is_changed() && state.found.is_some_and(|(seen, _)| seen == self.seen);
    if !known {
      state.found = Some((self.seen, skeleton.bone_id(&socket.bone)));
    }
    let bone = state.found.and_then(|(_, bone)| bone);
    let Some(bone) = bone.and_then(|bone| skeleton.bone_space(bone)) else {
      if state.on.is_some() {
        state.on = None;
      }
      // Told once for a name: another name it lacks is another mistake
      if self.warn && state.told.as_deref() != Some(socket.bone.as_str()) {
        log::warn!(
          "bone socket {child} names the bone {:?}, which its skeleton lacks; it stays where it is",
          socket.bone
        );
        state.told = Some(socket.bone.as_str().into());
      }
      return;
    };
    if state.told.is_some() {
      state.told = None;
    }
    let first = state.on != Some(self.entity);
    if first {
      state.on = Some(self.entity);
    }
    let skip = first || self.teleported;
    let local = socket.local(bone);
    transform.set_if_neq(local);
    let moved = global.set_if_neq(self.root.mul_transform(&local));
    skip_interpolation(previous, skip);
    if !moved && !skip {
      // It stands where it stood, and so does everything below it:
      // propagation has placed that already this step
      return;
    }
    if moved && is_skeleton {
      self.moved.push(child);
    }
    let here = *global;
    self.walk.clear();
    self
      .walk
      .extend(children.into_iter().flatten().map(|&below| (below, here)));
    while let Some((entity, parent)) = self.walk.pop() {
      // One without a `Transform` (or disabled) is not placed, and what is
      // below it does not follow the socket: as in propagation
      let Ok((transform, mut global, previous, children, _, is_skeleton)) =
        self.placed.get_mut(entity)
      else {
        continue;
      };
      let moved = global.set_if_neq(parent.mul_transform(&transform));
      skip_interpolation(previous, skip);
      if !moved && !skip {
        continue;
      }
      if moved && is_skeleton {
        self.moved.push(entity);
      }
      let here = *global;
      self
        .walk
        .extend(children.into_iter().flatten().map(|&below| (below, here)));
    }
  }
}

/// Draws an interpolated entity where it stands until the next step, when
/// `skip` asks for it
fn skip_interpolation(previous: Option<Mut<PreviousGlobalTransform>>, skip: bool) {
  if skip
    && let Some(mut previous) = previous
    && !previous.skip
  {
    previous.skip = true;
  }
}

#[cfg(test)]
mod tests {
  use std::sync::Arc;

  use glam::{Affine2, Vec2};

  use super::*;
  use crate::core::asset::{AssetServer, Handle, MemorySource, pump};
  use crate::core::ecs::interpolate::{render_global, snapshot_previous_transforms};
  use crate::core::ecs::propagate::propagate_transforms;
  use crate::core::ecs::test_util::{MAX_UNRELATED, Other, insert_unrelated};
  use crate::core::ecs::{
    Changed, ChildOf, Interpolated, IntoScheduleConfigs, Messages, PreviousGlobalTransform,
    Schedule, ScheduleLabel, World, WorldExt,
  };
  use crate::core::messages::register_message;
  use crate::core::skeleton::{
    Animation, BoneId, BoneSetup, Curve, EventData, EventValues, Timeline,
  };
  use crate::testing::{asset_world, asset_world_over, capture_log};

  #[derive(ScheduleLabel, Debug, Clone, Copy, PartialEq, Eq, Hash)]
  struct Step;

  #[derive(ScheduleLabel, Debug, Clone, Copy, PartialEq, Eq, Hash)]
  struct Frame;

  #[derive(ScheduleLabel, Debug, Clone, Copy, PartialEq, Eq, Hash)]
  struct Snapshot;

  /// A world with the asset stores, a 60 Hz clock and the skeleton
  /// messages; the fixed step's propagation then skeletons; and a frame's
  /// propagation, as `PostUpdate` runs it
  fn world() -> (World, Schedule, Schedule) {
    world_over(asset_world())
  }

  fn world_over(mut world: World) -> (World, Schedule, Schedule) {
    world.insert_resource(Time::new(60));
    register_message::<SkeletonEvent>(&mut world);
    register_message::<SkeletonTrackEvent>(&mut world);
    let mut step = Schedule::new(Step);
    step.add_systems((propagate_transforms, update_skeletons).chain());
    let mut frame = Schedule::new(Frame);
    frame.add_systems(propagate_transforms);
    (world, step, frame)
  }

  /// A root and a hand 10 along it. "turn" turns the root a radian a
  /// second, and fires "beat" at its start
  fn rig(world: &mut World) -> Handle<SkeletonData> {
    let hand = BoneSetup {
      x: 10.0,
      ..BoneSetup::default()
    };
    let turn = Animation {
      name: Arc::from("turn"),
      duration: 1.0,
      timelines: vec![
        Timeline::Rotate {
          bone: BoneId(0),
          frames: vec![(0.0, 0.0, Curve::Linear), (1.0, 1.0, Curve::Linear)],
        },
        Timeline::Event {
          frames: vec![(0.0, 0, EventValues::default())],
        },
      ],
    };
    let data = SkeletonData::builder()
      .bone("root", None, BoneSetup::default())
      .bone("hand", Some("root"), hand)
      .event(EventData::new("beat"))
      .animation(turn)
      .build()
      .expect("a valid skeleton");
    world.resource_mut::<Assets<SkeletonData>>().add(data)
  }

  /// The start of a fixed step: where everything is drawn from is where
  /// it stands now
  fn snapshot(world: &mut World) {
    let mut schedule = Schedule::new(Snapshot);
    schedule.add_systems(snapshot_previous_transforms);
    schedule.run(world);
  }

  /// Whether `entity`, under something interpolated, is drawn where it
  /// stands half way to the next step, not on its way there from where it
  /// stood before
  fn drawn_where_it_stands(world: &World, entity: Entity) -> bool {
    let global = world
      .get::<GlobalTransform>(entity)
      .expect("a placed entity");
    let previous = world.get::<PreviousGlobalTransform>(entity);
    assert!(previous.is_some(), "under an interpolated entity");
    render_global(global, previous, 0.5) == *global
  }

  /// An interpolated skeleton at (50, 20), a socket on its hand that
  /// waits at (500, 0) on it until it is placed, and a child 3 along the
  /// socket
  fn socketed(world: &mut World, data: Handle<SkeletonData>, bone: &str) -> [Entity; 3] {
    let body = world
      .spawn((
        Transform::from_xy(50.0, 20.0),
        Interpolated,
        Skeleton::new(data),
        turning(),
      ))
      .id();
    let socket = world
      .spawn((
        Transform::from_xy(500.0, 0.0),
        BoneSocket::new(bone),
        ChildOf(body),
      ))
      .id();
    let held = world
      .spawn((Transform::from_xy(3.0, 0.0), ChildOf(socket)))
      .id();
    [body, socket, held]
  }

  fn turning() -> SkeletonAnimation {
    let mut animation = SkeletonAnimation::new();
    animation.set_animation(0, "turn", true);
    animation
  }

  fn global(world: &World, entity: Entity) -> Affine2 {
    world
      .get::<GlobalTransform>(entity)
      .expect("a placed entity")
      .affine()
  }

  fn bone(world: &World, entity: Entity, name: &str) -> Affine2 {
    world
      .get::<Skeleton>(entity)
      .and_then(|skeleton| skeleton.bone_world(name))
      .expect("a bone of a stepped skeleton")
  }

  fn same(actual: Affine2, expected: Affine2) {
    assert!(
      actual.abs_diff_eq(expected, 1e-4),
      "expected {expected:?}, got {actual:?}"
    );
  }

  #[test]
  fn a_skeleton_on_a_socket_is_posed_where_the_socket_puts_it() {
    let (mut world, mut step, _) = world();
    let data = rig(&mut world);
    // Spawned from the far end of the chain in, so each skeleton's turn
    // comes before that of the skeleton whose socket carries it
    let charm = world
      .spawn((Transform::IDENTITY, BoneSocket::new("hand")))
      .id();
    let rider = world
      .spawn((
        Transform::IDENTITY,
        Skeleton::new(data.clone()),
        turning(),
        BoneSocket::new("hand"),
      ))
      .id();
    let saddle = world
      .spawn((
        Transform::IDENTITY,
        Skeleton::new(data.clone()),
        turning(),
        BoneSocket::new("hand"),
      ))
      .id();
    let horse = world
      .spawn((
        Transform::from_xy(50.0, 20.0),
        Skeleton::new(data),
        turning(),
      ))
      .id();
    world.entity_mut(saddle).insert(ChildOf(horse));
    world.entity_mut(rider).insert(ChildOf(saddle));
    world.entity_mut(charm).insert(ChildOf(rider));
    assert!(rider.index() < saddle.index() && saddle.index() < horse.index());
    let mut last = None;
    for steps in 1..=5 {
      world
        .get_mut::<Transform>(horse)
        .expect("the horse's transform")
        .translation
        .x += 3.0;
      step.run(&mut world);
      // Each one down the chain stands on the hand above it, and its own
      // bones are computed from where it stands in this step: its root
      // bone is there, turned as far as "turn" has turned it
      let turned = Affine2::from_angle(steps as f32 / 60.0);
      same(global(&world, saddle), bone(&world, horse, "hand"));
      same(
        bone(&world, saddle, "root"),
        global(&world, saddle) * turned,
      );
      same(global(&world, rider), bone(&world, saddle, "hand"));
      same(bone(&world, rider, "root"), global(&world, rider) * turned);
      same(global(&world, charm), bone(&world, rider, "hand"));
      let at = global(&world, charm);
      assert_ne!(last, Some(at), "the far end moved");
      last = Some(at);
    }
    // Each skeleton's events are sent once a step, not again for being
    // placed again: three skeletons, one key each at their start
    let sent = world.resource::<Messages<SkeletonEvent>>().len();
    assert_eq!(sent, 3);
  }

  #[test]
  fn propagation_after_the_step_writes_what_the_step_wrote() {
    let (mut world, mut step, mut frame) = world();
    let data = rig(&mut world);
    let body = world
      .spawn((
        Transform::from_xy(5.0, 5.0)
          .with_rotation(0.3)
          .with_scale(Vec2::new(1.5, 0.5)),
        Skeleton::new(data),
        turning(),
      ))
      .id();
    let offset = Transform::from_xy(2.0, 1.0).with_rotation(0.4);
    let socket = world
      .spawn((BoneSocket::new("hand").with_offset(offset), ChildOf(body)))
      .id();
    let held = world
      .spawn((Transform::from_xy(3.0, 0.0), ChildOf(socket)))
      .id();
    step.run(&mut world);
    frame.run(&mut world);
    for _ in 0..5 {
      world
        .get_mut::<Transform>(body)
        .expect("the skeleton's transform")
        .translation
        .x += 3.0;
      step.run(&mut world);
      // The socket's own transform is set the ordinary way, so whatever
      // watches transforms sees it move
      let mut moved = world.query_filtered::<Entity, Changed<Transform>>();
      assert!(moved.iter(&world).any(|entity| entity == socket));
      let placed = [body, socket, held].map(|entity| global(&world, entity));
      same(placed[1], bone(&world, body, "hand") * offset.to_affine());
      same(
        placed[2],
        placed[1] * Affine2::from_translation(Vec2::new(3.0, 0.0)),
      );
      // The frame's propagation sees the skeleton's entity and the socket
      // changed, and makes of them exactly what the step made: nothing is
      // written
      world.clear_trackers();
      frame.run(&mut world);
      let mut rewritten = world.query_filtered::<Entity, Changed<GlobalTransform>>();
      assert_eq!(rewritten.iter(&world).count(), 0);
      assert_eq!(
        [body, socket, held].map(|entity| global(&world, entity)),
        placed
      );
    }
  }

  #[test]
  fn a_still_socket_is_not_rewritten() {
    let (mut world, mut step, _) = world();
    let data = rig(&mut world);
    let body = world
      .spawn((Transform::from_xy(5.0, 5.0), Skeleton::new(data)))
      .id();
    let socket = world.spawn((BoneSocket::new("hand"), ChildOf(body))).id();
    let held = world
      .spawn((Transform::from_xy(3.0, 0.0), ChildOf(socket)))
      .id();
    step.run(&mut world);
    world.clear_trackers();
    step.run(&mut world);
    // The skeleton holds its setup pose where it stands: its socket's
    // transforms are found unchanged and left alone, so nothing that
    // watches them is woken
    let mut moved = world.query_filtered::<Entity, Changed<Transform>>();
    assert_eq!(moved.iter(&world).count(), 0);
    let mut placed = world.query_filtered::<Entity, Changed<GlobalTransform>>();
    assert_eq!(placed.iter(&world).count(), 0);
    assert_eq!(global(&world, held).translation, Vec2::new(18.0, 5.0));
  }

  #[test]
  fn skeletons_send_their_events_in_entity_order() {
    // Two skeletons in different archetypes, which a query visits in an
    // order that changes with the world's other archetypes
    for unrelated in 0..=MAX_UNRELATED {
      let (mut world, mut step, _) = world();
      insert_unrelated(&mut world, unrelated);
      let data = rig(&mut world);
      let first = world
        .spawn((Transform::IDENTITY, Skeleton::new(data.clone()), turning()))
        .id();
      let second = world
        .spawn((Transform::IDENTITY, Skeleton::new(data), turning(), Other))
        .id();
      step.run(&mut world);
      let messages = world.resource::<Messages<SkeletonEvent>>();
      let order: Vec<Entity> = messages
        .get_cursor()
        .read(messages)
        .map(|event| event.entity)
        .collect();
      assert_eq!(order, vec![first, second], "with {unrelated} resources");
    }
  }

  #[test]
  fn a_socket_first_placed_does_not_slide_from_where_it_waited() {
    let (mut world, mut step, mut frame) = world();
    let data = rig(&mut world);
    let [body, socket, held] = socketed(&mut world, data, "hand");
    // A frame that runs no fixed step: propagation places the socket where
    // its own transform has it, and gives it a previous transform
    frame.run(&mut world);
    assert_eq!(global(&world, socket).translation, Vec2::new(550.0, 20.0));
    // The first step: its snapshot takes that place as where the socket
    // was, and then the socket is put on its bone. It is drawn there, and
    // so is what is below it, not on the way from where they waited
    snapshot(&mut world);
    step.run(&mut world);
    same(global(&world, socket), bone(&world, body, "hand"));
    assert!(drawn_where_it_stands(&world, socket));
    assert!(drawn_where_it_stands(&world, held));
    // From the next step it is drawn between the steps' places, like
    // anything under an interpolated entity
    let before = global(&world, socket);
    snapshot(&mut world);
    step.run(&mut world);
    let previous = world
      .get::<PreviousGlobalTransform>(socket)
      .expect("under an interpolated entity");
    assert!(!previous.skip);
    assert_eq!(previous.global.affine(), before);
    assert_ne!(global(&world, socket), before);
  }

  #[test]
  fn a_socket_whose_skeleton_s_data_lands_later_does_not_slide() {
    const PATH: &str = "rigs/late.skeleton.ron";
    let files = MemorySource::new();
    files.insert(
      PATH,
      r#"(bones: [(name: "root"), (name: "hand", parent: "root", x: 10)])"#,
    );
    let (mut world, mut step, mut frame) = world_over(asset_world_over(files));
    // Asked for, and on its way for as long as nothing pumps the registry
    let data = world.resource::<AssetServer>().load::<SkeletonData>(PATH);
    let [body, socket, held] = socketed(&mut world, data.clone(), "hand");
    for _ in 0..3 {
      snapshot(&mut world);
      step.run(&mut world);
      frame.run(&mut world);
      assert_eq!(global(&world, socket).translation, Vec2::new(550.0, 20.0));
    }
    let start = std::time::Instant::now();
    while world
      .resource::<Assets<SkeletonData>>()
      .get(&data)
      .is_none()
    {
      assert!(start.elapsed().as_secs() < 5, "timed out waiting");
      std::thread::sleep(std::time::Duration::from_millis(1));
      pump(&mut world);
    }
    // The step that first poses the skeleton puts the socket on its bone,
    // drawn there at once
    snapshot(&mut world);
    step.run(&mut world);
    same(global(&world, socket), bone(&world, body, "hand"));
    assert!(drawn_where_it_stands(&world, socket));
    assert!(drawn_where_it_stands(&world, held));
  }

  #[test]
  fn a_teleported_skeleton_s_sockets_skip_with_it() {
    let (mut world, mut step, mut frame) = world();
    let data = rig(&mut world);
    let [body, socket, held] = socketed(&mut world, data, "hand");
    frame.run(&mut world);
    for _ in 0..2 {
      snapshot(&mut world);
      step.run(&mut world);
    }
    assert!(!drawn_where_it_stands(&world, socket), "between steps");
    // In a step, after its snapshot, the game moves the skeleton far off
    // and says it is a jump: the skeleton is drawn where it lands, and so
    // are its socket and what the socket holds
    snapshot(&mut world);
    world
      .get_mut::<Transform>(body)
      .expect("the skeleton's transform")
      .translation = Vec2::new(900.0, 900.0);
    world.skip_interpolation(body);
    step.run(&mut world);
    same(global(&world, socket), bone(&world, body, "hand"));
    for entity in [body, socket, held] {
      assert!(drawn_where_it_stands(&world, entity));
    }
    // The step after, all three are drawn between steps again
    snapshot(&mut world);
    step.run(&mut world);
    for entity in [socket, held] {
      let previous = world
        .get::<PreviousGlobalTransform>(entity)
        .expect("under an interpolated entity");
      assert!(!previous.skip);
    }
  }

  #[test]
  fn a_missing_bone_is_told_once_a_name_and_a_bone_found_again_is_not_slid_to() {
    let log = capture_log();
    let (mut world, mut step, mut frame) = world();
    let data = rig(&mut world);
    let [body, socket, held] = socketed(&mut world, data, "wing");
    let mut steps = |world: &mut World, step: &mut Schedule| {
      for _ in 0..3 {
        snapshot(world);
        step.run(world);
        frame.run(world);
      }
    };
    let rename = |world: &mut World, bone: &str| {
      world
        .get_mut::<BoneSocket>(socket)
        .expect("the socket")
        .bone = bone.to_owned();
    };
    // A bone the skeleton lacks: told once, and the socket waits
    steps(&mut world, &mut step);
    assert_eq!(log.containing("\"wing\""), 1);
    assert_eq!(global(&world, socket).translation, Vec2::new(550.0, 20.0));
    // Another name it lacks is another mistake, told once too
    rename(&mut world, "tail");
    steps(&mut world, &mut step);
    assert_eq!(log.containing("\"tail\""), 1);
    assert_eq!(log.containing("\"wing\""), 1);
    // A bone it has: the socket goes to it in the next step, drawn there
    // at once with what it holds
    rename(&mut world, "hand");
    snapshot(&mut world);
    step.run(&mut world);
    same(global(&world, socket), bone(&world, body, "hand"));
    assert!(drawn_where_it_stands(&world, socket));
    assert!(drawn_where_it_stands(&world, held));
    // And from then on between steps
    steps(&mut world, &mut step);
    assert!(!drawn_where_it_stands(&world, socket));
    // Named wrongly again, it is told again: the first telling was for a
    // mistake since put right
    rename(&mut world, "wing");
    steps(&mut world, &mut step);
    assert_eq!(log.containing("\"wing\""), 2);
    assert_eq!(log.containing("\"tail\""), 1);
  }

  #[test]
  fn a_socket_follows_its_bone_s_name_and_its_data_as_they_change() {
    let (mut world, mut step, _) = world();
    let data = rig(&mut world);
    let [body, socket, _] = socketed(&mut world, data.clone(), "hand");
    step.run(&mut world);
    same(global(&world, socket), bone(&world, body, "hand"));
    // The game names another bone: the next step finds it
    world
      .get_mut::<BoneSocket>(socket)
      .expect("the socket")
      .bone = "root".to_owned();
    step.run(&mut world);
    same(global(&world, socket), bone(&world, body, "root"));
    // New data with the bones in other places in the list: the name is
    // found again where it now is
    let longer = SkeletonData::builder()
      .bone("tail", None, BoneSetup::default())
      .bone(
        "root",
        None,
        BoneSetup {
          x: 7.0,
          ..BoneSetup::default()
        },
      )
      .build()
      .expect("a valid skeleton");
    *world
      .resource_mut::<Assets<SkeletonData>>()
      .get_mut(&data)
      .expect("the data") = longer;
    step.run(&mut world);
    same(global(&world, socket), bone(&world, body, "root"));
    assert_eq!(global(&world, socket).translation, Vec2::new(57.0, 20.0));
  }
}
