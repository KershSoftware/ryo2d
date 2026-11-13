//! The skeletons' part of the fixed step: every skeleton poses its bones,
//! its sockets are put on them, and its events are sent. The module notes
//! give the order and the rules.

use super::component::{Fired, Skeleton};
use super::data::SkeletonData;
use super::event::{SkeletonEvent, SkeletonTrackEvent};
use super::socket::{BoneSocket, MissingBone};
use super::state::SkeletonAnimation;
use crate::core::asset::Assets;
use crate::core::ecs::{
  Children, Commands, DetectChangesMut, Entity, GlobalTransform, Has, Local, MessageWriter, Mut,
  Query, Res, Transform, entity_order,
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
    Option<&'static Children>,
    Option<&'static BoneSocket>,
    Has<MissingBone>,
    Has<Skeleton>,
  ),
>;

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
#[allow(clippy::too_many_arguments)] // a resource, query or writer per input
pub(crate) fn update_skeletons(
  time: Option<Res<Time>>,
  data: Option<Res<Assets<SkeletonData>>>,
  mut skeletons: Skeletons,
  mut placed: Placed,
  mut events: MessageWriter<SkeletonEvent>,
  mut track_events: MessageWriter<SkeletonTrackEvent>,
  mut commands: Commands,
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
    let Ok(root) = placed.get(entity).map(|(_, global, ..)| *global) else {
      continue;
    };
    fired.events.clear();
    fired.track_events.clear();
    let animation = animation.map(Mut::into_inner);
    skeleton.step(rig, (id, version), animation, dt, root.affine(), fired);
    let mut sockets = Sockets {
      placed: &mut placed,
      commands: &mut commands,
      walk,
      moved,
      warn: true,
    };
    sockets.place_all(&skeleton, children, root);
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
    let Ok(root) = placed.get(entity).map(|(_, global, ..)| *global) else {
      continue;
    };
    // Only one this step posed, standing elsewhere than it was posed for
    if skeleton.seen() != Some((id, version)) || skeleton.root() == root.affine() {
      continue;
    }
    skeleton.place_again(rig, root.affine());
    let mut sockets = Sockets {
      placed: &mut placed,
      commands: &mut commands,
      walk,
      moved,
      warn: false,
    };
    sockets.place_all(&skeleton, children, root);
  }
}

/// Placing one skeleton's sockets
struct Sockets<'a, 'w, 's, 'cw, 'cs> {
  placed: &'a mut Placed<'w, 's>,
  commands: &'a mut Commands<'cw, 'cs>,
  walk: &'a mut Vec<(Entity, GlobalTransform)>,
  moved: &'a mut Vec<Entity>,
  /// Whether a socket whose bone is missing is told: not when its
  /// skeleton is placed a second time in a step
  warn: bool,
}

impl Sockets<'_, '_, '_, '_, '_> {
  /// Puts each of `skeleton`'s children that is a socket on its bone,
  /// under `root`, the skeleton's entity's transform
  fn place_all(&mut self, skeleton: &Skeleton, children: Option<&Children>, root: GlobalTransform) {
    for child in children.into_iter().flatten().copied() {
      self.place(skeleton, child, root);
    }
  }

  /// Puts the socket `child` on its bone: its `Transform` is the bone's in
  /// the skeleton's space with the socket's offset, and its
  /// `GlobalTransform` is `root` times that, which is what propagation
  /// makes of the two, so whichever writes it last writes the same. Then
  /// everything below it is placed, as propagation would place it
  fn place(&mut self, skeleton: &Skeleton, child: Entity, root: GlobalTransform) {
    let Ok((mut transform, mut global, children, socket, told, is_skeleton)) =
      self.placed.get_mut(child)
    else {
      return;
    };
    let Some(socket) = socket else {
      return;
    };
    let Some(bone) = skeleton.bone_in_skeleton(&socket.bone) else {
      if self.warn && !told {
        log::warn!(
          "bone socket {child} names the bone {:?}, which its skeleton lacks; it stays where it is",
          socket.bone
        );
        self.commands.entity(child).try_insert(MissingBone);
      }
      return;
    };
    let local = socket.local(bone);
    transform.set_if_neq(local);
    if !global.set_if_neq(root.mul_transform(&local)) {
      // It stands where it stood, and so does everything below it:
      // propagation has placed that already this step
      return;
    }
    if is_skeleton {
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
      let Ok((transform, mut global, children, _, _, is_skeleton)) = self.placed.get_mut(entity)
      else {
        continue;
      };
      if !global.set_if_neq(parent.mul_transform(&transform)) {
        continue;
      }
      if is_skeleton {
        self.moved.push(entity);
      }
      let here = *global;
      self
        .walk
        .extend(children.into_iter().flatten().map(|&below| (below, here)));
    }
  }
}

#[cfg(test)]
mod tests {
  use std::sync::Arc;

  use glam::{Affine2, Vec2};

  use super::*;
  use crate::core::asset::Handle;
  use crate::core::ecs::propagate::propagate_transforms;
  use crate::core::ecs::test_util::{MAX_UNRELATED, Other, insert_unrelated};
  use crate::core::ecs::{
    Changed, ChildOf, IntoScheduleConfigs, Messages, Schedule, ScheduleLabel, World,
  };
  use crate::core::messages::register_message;
  use crate::core::skeleton::{
    Animation, BoneId, BoneSetup, Curve, EventData, EventValues, Timeline,
  };
  use crate::testing::asset_world;

  #[derive(ScheduleLabel, Debug, Clone, Copy, PartialEq, Eq, Hash)]
  struct Step;

  #[derive(ScheduleLabel, Debug, Clone, Copy, PartialEq, Eq, Hash)]
  struct Frame;

  /// A world with the asset stores, a 60 Hz clock and the skeleton
  /// messages; the fixed step's propagation then skeletons; and a frame's
  /// propagation, as `PostUpdate` runs it
  fn world() -> (World, Schedule, Schedule) {
    let mut world = asset_world();
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
}
