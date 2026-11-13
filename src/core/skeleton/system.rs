//! The skeletons' part of the fixed step: every skeleton poses its bones,
//! its sockets are put on them, and its events are sent; and their part of
//! each frame: a skeleton with no pose yet gets one. The module notes give
//! the order and the rules.

use super::component::{Fired, Skeleton};
use super::data::{DataVersion, SkeletonData};
use super::event::{SkeletonEvent, SkeletonTrackEvent};
use super::scope::SkeletonStore;
use super::socket::{BoneSocket, FoundBone, SocketState};
use super::state::SkeletonAnimation;
use crate::core::asset::Assets;
use crate::core::ecs::{
  Children, DetectChangesMut, Entity, GlobalTransform, Has, Local, MessageWriter, Mut,
  PreviousGlobalTransform, Query, Res, ResMut, Transform, entity_order,
};
use crate::core::particles::ParticleEmitter;
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
    Option<&'static mut ParticleEmitter>,
    Option<&'static Children>,
    Option<(&'static BoneSocket, &'static mut SocketState)>,
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
#[allow(clippy::too_many_arguments)] // a resource, query or writer per input
pub(crate) fn update_skeletons(
  time: Option<Res<Time>>,
  data: Option<Res<Assets<SkeletonData>>>,
  mut store: Option<ResMut<SkeletonStore>>,
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
    // A skeleton whose scene is stopped under another is frozen with it:
    // nothing advances and nothing is sent, and what its calls caused
    // waits for the step it updates again
    if store
      .as_ref()
      .is_some_and(|store| store.holds(skeleton.scope()))
    {
      continue;
    }
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
    skeleton.new_placing(store.as_mut().map(|store| store.next_placing()));
    let mut sockets = Sockets {
      placed: &mut placed,
      walk,
      moved,
      entity,
      seen: (id, version),
      root,
      teleported,
      stood: skeleton.placings(),
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
  place_moved(&mut skeletons, &mut placed, &data, walk, moved);
}

/// What the frame's pass keeps between runs: what it last looked at (how
/// many skeletons and sockets had been added, and how many versions their
/// data's store had given), and the buffers of the sockets' placing
#[derive(Default)]
pub(crate) struct AppearState {
  looked_at: Option<(u64, u64)>,
  walk: Vec<(Entity, GlobalTransform)>,
  moved: Vec<Entity>,
}

/// Each frame (`PostUpdate`, after its propagation and before the cameras
/// and the draw): a skeleton with no pose for its data as it now is gets
/// one where it stands, with its sockets placed. That is one added, or
/// whose data landed or reloaded, since its last step: in a frame that
/// runs no fixed step after it (the clock paused, hitstop, slow motion, a
/// fast display), it would otherwise have nothing to draw, or the old
/// data's pose, until the next step. The pose is where its animation
/// stands, an entry yet to start at its first moment; nothing advances,
/// and nothing is sent before a step runs ([`Skeleton::appear`]). A held
/// skeleton gets its pose too. And a socket added to a skeleton that has
/// its pose is put on its bone, where it would otherwise sit at its own
/// transform until the next step.
///
/// The pass looks through the skeletons only in a frame since whose last
/// look a skeleton or a socket was added or the store of their data gave a
/// version: any other frame it compares two counts and returns
pub(crate) fn pose_new_skeletons(
  data: Option<Res<Assets<SkeletonData>>>,
  store: Option<ResMut<SkeletonStore>>,
  mut skeletons: Skeletons,
  mut placed: Placed,
  mut state: Local<AppearState>,
) {
  let Some(data) = data else {
    return;
  };
  let mut store = store;
  // A world with no store counts no additions, so every frame looks
  let now = store
    .as_ref()
    .map(|store| (store.additions(), data.versions_given()));
  if now.is_some() && state.looked_at == now {
    return;
  }
  crate::profile_scope!("cpu:skeletons");
  let AppearState {
    looked_at,
    walk,
    moved,
  } = &mut *state;
  *looked_at = now;
  if let Some(store) = &mut store {
    store.looked();
  }
  moved.clear();
  for (entity, mut skeleton, animation, children) in &mut skeletons {
    let id = skeleton.data().id();
    let (Some(version), Some(rig)) = (data.version(id), data.get(id)) else {
      continue;
    };
    let Some((root, teleported)) = standing(&placed, entity) else {
      continue;
    };
    // One with its pose already keeps it; its sockets are placed on it, so
    // one added since the last step is on its bone (the rest stand where
    // they stood). Its placing is the same one, which only they stood in
    let mut stood = skeleton.placings();
    if skeleton.seen() == Some((id, version)) {
      stood[1] = stood[0];
    } else {
      let animation = animation.map(Mut::into_inner);
      skeleton.appear(rig, (id, version), animation, root.affine());
      skeleton.new_placing(store.as_mut().map(|store| store.next_placing()));
      stood = skeleton.placings();
    }
    let mut sockets = Sockets {
      placed: &mut placed,
      walk,
      moved,
      entity,
      seen: (id, version),
      root,
      teleported,
      stood,
      warn: true,
    };
    sockets.place_all(&skeleton, children);
  }
  place_moved(&mut skeletons, &mut placed, &data, walk, moved);
}

/// A skeleton on another's socket was posed where it stood before the
/// socket moved it, if its turn came first: the bones of each skeleton in
/// `moved` are computed again where it now stands, and its own sockets
/// follow. One so moved may move another in turn. A hierarchy has no
/// cycles, so the list ends; the bound is against one that has
fn place_moved(
  skeletons: &mut Skeletons,
  placed: &mut Placed,
  data: &Assets<SkeletonData>,
  walk: &mut Vec<(Entity, GlobalTransform)>,
  moved: &mut Vec<Entity>,
) {
  if moved.is_empty() {
    return;
  }
  let count = skeletons.iter().len();
  let most = count.saturating_mul(count).saturating_add(1);
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
    let Some((root, teleported)) = standing(placed, entity) else {
      continue;
    };
    // Only one that has a pose, standing elsewhere than it was posed for
    if skeleton.seen() != Some((id, version)) || skeleton.root() == root.affine() {
      continue;
    }
    skeleton.place_again(rig, root.affine());
    let mut sockets = Sockets {
      placed: &mut *placed,
      walk: &mut *walk,
      moved: &mut *moved,
      entity,
      seen: (id, version),
      root,
      teleported,
      stood: skeleton.placings(),
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
  /// The number of this placing, and of the placing a socket must have
  /// stood in to be moving with its bone, not arriving on it: the
  /// skeleton's one before, for a new pose; this one itself, when sockets
  /// are placed on a pose a second time
  stood: [u64; 2],
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
    let Ok((mut transform, mut global, previous, emitter, children, socket, is_skeleton)) =
      self.placed.get_mut(child)
    else {
      return;
    };
    let Some((socket, mut state)) = socket else {
      return;
    };
    // The bone's name is looked up once a version of the data and once a
    // name: the name asked for is kept and compared as it is, so a rename
    // is seen whenever it was made, even while nothing placed the socket
    let renamed = state
      .found
      .as_ref()
      .is_some_and(|found| *found.name != *socket.bone);
    let known = !renamed
      && state
        .found
        .as_ref()
        .is_some_and(|found| found.seen == self.seen);
    if !known {
      let bone = skeleton.bone_id(&socket.bone);
      match &mut state.found {
        Some(found) if !renamed => {
          found.seen = self.seen;
          found.bone = bone;
        }
        found => {
          *found = Some(FoundBone {
            name: socket.bone.as_str().into(),
            seen: self.seen,
            bone,
          });
        }
      }
    }
    let bone = state.found.as_ref().and_then(|found| found.bone);
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
    // It moves with its bone only if this skeleton's last placing put it
    // on this bone. Anything else is an arrival, drawn at once: a new
    // socket (or its `BoneSocket` put on again), its skeleton's first pose,
    // a bone found again, another bone named, another skeleton, or a
    // placing it missed (detached, disabled, or no socket for a while)
    let stood = state
      .on
      .is_some_and(|(on, placing)| on == self.entity && self.stood.contains(&placing));
    let here = (self.entity, self.stood[0]);
    if state.on != Some(here) {
      state.on = Some(here);
    }
    let skip = !stood || renamed || self.teleported;
    let local = socket.local(bone);
    transform.set_if_neq(local);
    let moved = global.set_if_neq(self.root.mul_transform(&local));
    if skip {
      jumped(previous, emitter);
    }
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
      let Ok((transform, mut global, previous, emitter, children, _, is_skeleton)) =
        self.placed.get_mut(entity)
      else {
        continue;
      };
      let moved = global.set_if_neq(parent.mul_transform(&transform));
      if skip {
        jumped(previous, emitter);
      }
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

/// An entity on or below a socket arrived where it stands, it did not
/// move there: an interpolated one is drawn there until the next step, and
/// an emitter on it spawns only there in its next step, as an entity's own
/// emitter does after `skip_interpolation`. The emitter is told itself,
/// interpolated or not, because the next step's snapshot clears the skip
/// before the particle step reads it
fn jumped(previous: Option<Mut<PreviousGlobalTransform>>, emitter: Option<Mut<ParticleEmitter>>) {
  if let Some(mut previous) = previous
    && !previous.skip
  {
    previous.skip = true;
  }
  if let Some(mut emitter) = emitter
    && !emitter.teleported
  {
    emitter.teleported = true;
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

  #[derive(ScheduleLabel, Debug, Clone, Copy, PartialEq, Eq, Hash)]
  struct Appear;

  /// A frame's propagation and then its pass for skeletons without a
  /// pose, as `PostUpdate` runs them
  fn appearing() -> Schedule {
    let mut frame = Schedule::new(Appear);
    frame.add_systems((propagate_transforms, pose_new_skeletons).chain());
    frame
  }

  fn looks(world: &World) -> u64 {
    world.resource::<SkeletonStore>().looks()
  }

  fn world_over(mut world: World) -> (World, Schedule, Schedule) {
    world.insert_resource(SkeletonStore::default());
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
  fn a_skeleton_whose_entity_skips_in_a_step_takes_its_sockets_with_it() {
    let (mut world, mut step, mut frame) = world();
    let data = rig(&mut world);
    let [body, socket, held] = socketed(&mut world, data, "hand");
    frame.run(&mut world);
    for _ in 0..2 {
      snapshot(&mut world);
      step.run(&mut world);
    }
    assert!(!drawn_where_it_stands(&world, socket), "between steps");
    // In a step, after its snapshot, the skeleton is moved far off and its
    // entity marked to be drawn where it lands (as a pool marks one it
    // hands out, and a teleport does): the step's placing carries the mark
    // to its socket and what the socket holds. The mark is set here by
    // hand, so nothing but the placing can have carried it
    snapshot(&mut world);
    world
      .get_mut::<Transform>(body)
      .expect("the skeleton's transform")
      .translation = Vec2::new(900.0, 900.0);
    world
      .get_mut::<PreviousGlobalTransform>(body)
      .expect("an interpolated entity")
      .skip = true;
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
  fn a_jump_made_outside_a_step_reaches_a_skeleton_s_sockets() {
    let (mut world, mut step, mut frame) = world();
    let data = rig(&mut world);
    let [body, socket, held] = socketed(&mut world, data, "hand");
    frame.run(&mut world);
    for _ in 0..2 {
      snapshot(&mut world);
      step.run(&mut world);
    }
    assert!(!drawn_where_it_stands(&world, socket), "between steps");
    // No step runs (the clock paused, say): the call itself marks the
    // sockets and what they hold, the frame's propagation carries them
    // with the skeleton, and they are drawn where it puts them
    world
      .get_mut::<Transform>(body)
      .expect("the skeleton's transform")
      .translation = Vec2::new(-400.0, 100.0);
    world.skip_interpolation(body);
    frame.run(&mut world);
    assert_eq!(global(&world, body).translation, Vec2::new(-400.0, 100.0));
    for entity in [body, socket, held] {
      assert!(drawn_where_it_stands(&world, entity));
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

  #[test]
  fn a_skeleton_posed_in_a_frame_starts_its_animation_in_its_first_step() {
    let (mut world, mut step, _) = world();
    let mut frame = appearing();
    let data = rig(&mut world);
    let [body, socket, held] = socketed(&mut world, data, "hand");
    // A frame with no fixed step: the skeleton has its pose, the root not
    // yet turned, and its socket is on its hand, drawn there at once with
    // what it holds
    frame.run(&mut world);
    same(bone(&world, body, "root"), global(&world, body));
    same(global(&world, socket), bone(&world, body, "hand"));
    assert!(drawn_where_it_stands(&world, socket));
    assert!(drawn_where_it_stands(&world, held));
    // Nothing is sent before a step runs, however many frames pass
    for _ in 0..3 {
      frame.run(&mut world);
    }
    assert_eq!(world.resource::<Messages<SkeletonEvent>>().len(), 0);
    assert_eq!(world.resource::<Messages<SkeletonTrackEvent>>().len(), 0);
    // The first step is the animation's first: its Start and the key at
    // its start, once, and one step's turn from where the frame showed it
    snapshot(&mut world);
    step.run(&mut world);
    assert_eq!(world.resource::<Messages<SkeletonEvent>>().len(), 1);
    assert_eq!(world.resource::<Messages<SkeletonTrackEvent>>().len(), 1);
    let turned = Affine2::from_angle(1.0 / 60.0);
    same(bone(&world, body, "root"), global(&world, body) * turned);
    // The socket is drawn from where the frame put it to where the step
    // does: it was on its bone already
    let previous = world
      .get::<PreviousGlobalTransform>(socket)
      .expect("under an interpolated entity");
    assert!(!previous.skip);
    step.run(&mut world);
    assert_eq!(world.resource::<Messages<SkeletonEvent>>().len(), 1);
    assert_eq!(world.resource::<Messages<SkeletonTrackEvent>>().len(), 1);
  }

  #[test]
  fn a_steady_frame_does_not_look_through_the_skeletons() {
    let (mut world, mut step, _) = world();
    let mut frame = appearing();
    let data = rig(&mut world);
    let bodies: Vec<Entity> = (0..3)
      .map(|_| socketed(&mut world, data.clone(), "hand")[0])
      .collect();
    // The frame they appear in looks once, and poses them
    frame.run(&mut world);
    assert_eq!(looks(&world), 1);
    for &body in &bodies {
      assert!(
        world
          .get::<Skeleton>(body)
          .is_some_and(|skeleton| skeleton.seen().is_some())
      );
    }
    // After that a frame compares two counts and goes: no look, and no
    // skeleton touched
    world.clear_trackers();
    for _ in 0..10 {
      frame.run(&mut world);
    }
    assert_eq!(looks(&world), 1);
    let mut touched = world.query_filtered::<Entity, Changed<Skeleton>>();
    assert_eq!(touched.iter(&world).count(), 0);
    // Steps do not make it look either
    for _ in 0..3 {
      step.run(&mut world);
      frame.run(&mut world);
    }
    assert_eq!(looks(&world), 1);
    // A skeleton added makes it look once: only the new one is posed
    world.clear_trackers();
    let added = socketed(&mut world, data.clone(), "hand")[0];
    frame.run(&mut world);
    frame.run(&mut world);
    assert_eq!(looks(&world), 2);
    let posed: Vec<Entity> = touched.iter(&world).collect();
    assert_eq!(posed, vec![added]);
    // New data makes it look once more, and pose those that use it
    let again = SkeletonData::builder()
      .bone("root", None, BoneSetup::default())
      .bone(
        "hand",
        Some("root"),
        BoneSetup {
          x: 30.0,
          ..BoneSetup::default()
        },
      )
      .build()
      .expect("a valid skeleton");
    *world
      .resource_mut::<Assets<SkeletonData>>()
      .get_mut(&data)
      .expect("the data") = again;
    frame.run(&mut world);
    frame.run(&mut world);
    assert_eq!(looks(&world), 3);
    for &body in &bodies {
      let hand = bone(&world, body, "hand").translation - global(&world, body).translation;
      assert!((hand.length() - 30.0).abs() < 1e-3, "{hand}");
    }
    // A step that has seen the new data first leaves the frame nothing to
    // pose: it looks, as the store moved, and touches none
    let other = rig(&mut world);
    let late = socketed(&mut world, other, "hand")[0];
    step.run(&mut world);
    world.clear_trackers();
    frame.run(&mut world);
    assert_eq!(looks(&world), 4);
    assert_eq!(touched.iter(&world).count(), 0);
    assert!(
      world
        .get::<Skeleton>(late)
        .is_some_and(|skeleton| skeleton.seen().is_some())
    );
  }

  #[test]
  fn a_held_skeleton_gets_its_pose_and_then_waits() {
    let (mut world, mut step, _) = world();
    let mut frame = appearing();
    let data = rig(&mut world);
    // Added by scene 4's code, which a scene above then stops
    let [body, socket, _] =
      crate::core::skeleton::in_scene(&mut world, 4, |world| socketed(world, data, "hand"));
    crate::core::skeleton::hold_scenes(&mut world, [4].into_iter());
    frame.run(&mut world);
    same(bone(&world, body, "root"), global(&world, body));
    same(global(&world, socket), bone(&world, body, "hand"));
    // Steps pass it by: it stays at its first moment and says nothing
    for _ in 0..5 {
      step.run(&mut world);
    }
    same(bone(&world, body, "root"), global(&world, body));
    assert_eq!(world.resource::<Messages<SkeletonEvent>>().len(), 0);
    assert_eq!(world.resource::<Messages<SkeletonTrackEvent>>().len(), 0);
    // Let go, it starts
    crate::core::skeleton::hold_scenes(&mut world, std::iter::empty());
    step.run(&mut world);
    assert_eq!(world.resource::<Messages<SkeletonEvent>>().len(), 1);
    assert_eq!(world.resource::<Messages<SkeletonTrackEvent>>().len(), 1);
  }

  /// Names another bone on `socket`
  fn rename(world: &mut World, socket: Entity, bone: &str) {
    world
      .get_mut::<BoneSocket>(socket)
      .expect("the socket")
      .bone = bone.to_owned();
  }

  /// A skeleton with a socket and its child, added by scene 4's code and
  /// stepped twice, so the socket stands on `bone` if the skeleton has it
  fn staged(bone: &str) -> (World, Schedule, [Entity; 3]) {
    let (mut world, mut step, _) = world();
    let data = rig(&mut world);
    let entities =
      crate::core::skeleton::in_scene(&mut world, 4, |world| socketed(world, data, bone));
    for _ in 0..2 {
      snapshot(&mut world);
      step.run(&mut world);
    }
    (world, step, entities)
  }

  #[test]
  fn a_socket_renamed_while_its_skeleton_is_held_takes_its_new_bone() {
    let log = capture_log();
    let (mut world, mut step, [body, socket, held]) = staged("hand");
    same(global(&world, socket), bone(&world, body, "hand"));
    // A scene above stops the skeleton's scene, and renames the socket
    // while steps go by without it
    crate::core::skeleton::hold_scenes(&mut world, [4].into_iter());
    rename(&mut world, socket, "root");
    for _ in 0..3 {
      snapshot(&mut world);
      step.run(&mut world);
    }
    // Let go, the socket is on its new bone in the first step, drawn there
    // at once with what it holds, and nothing is said
    crate::core::skeleton::hold_scenes(&mut world, std::iter::empty());
    snapshot(&mut world);
    step.run(&mut world);
    same(global(&world, socket), bone(&world, body, "root"));
    assert!(drawn_where_it_stands(&world, socket));
    assert!(drawn_where_it_stands(&world, held));
    assert_eq!(log.containing("lacks"), 0, "{:?}", log.warnings());
  }

  #[test]
  fn a_socket_renamed_while_its_skeleton_is_disabled_takes_its_new_bone() {
    let (mut world, mut step, [body, socket, _]) = staged("hand");
    world.entity_mut(body).insert(crate::core::ecs::Disabled);
    rename(&mut world, socket, "root");
    for _ in 0..3 {
      snapshot(&mut world);
      step.run(&mut world);
    }
    world
      .entity_mut(body)
      .remove::<crate::core::ecs::Disabled>();
    snapshot(&mut world);
    step.run(&mut world);
    same(global(&world, socket), bone(&world, body, "root"));
  }

  #[test]
  fn a_socket_given_a_real_bone_while_held_attaches_and_is_not_told_it_is_missing() {
    let log = capture_log();
    let (mut world, mut step, [body, socket, _]) = staged("wing");
    assert_eq!(log.containing("\"wing\""), 1);
    crate::core::skeleton::hold_scenes(&mut world, [4].into_iter());
    rename(&mut world, socket, "hand");
    for _ in 0..3 {
      snapshot(&mut world);
      step.run(&mut world);
    }
    crate::core::skeleton::hold_scenes(&mut world, std::iter::empty());
    snapshot(&mut world);
    step.run(&mut world);
    same(global(&world, socket), bone(&world, body, "hand"));
    assert_eq!(log.containing("\"hand\""), 0, "{:?}", log.warnings());
    assert_eq!(log.containing("lacks"), 1, "{:?}", log.warnings());
  }

  #[test]
  fn a_socket_put_over_another_while_held_takes_the_new_one_s_bone() {
    let (mut world, mut step, [body, socket, held]) = staged("hand");
    crate::core::skeleton::hold_scenes(&mut world, [4].into_iter());
    world.entity_mut(socket).insert(BoneSocket::new("root"));
    for _ in 0..3 {
      snapshot(&mut world);
      step.run(&mut world);
    }
    crate::core::skeleton::hold_scenes(&mut world, std::iter::empty());
    snapshot(&mut world);
    step.run(&mut world);
    same(global(&world, socket), bone(&world, body, "root"));
    assert!(drawn_where_it_stands(&world, socket));
    assert!(drawn_where_it_stands(&world, held));
  }

  #[test]
  fn a_socket_dropped_and_picked_up_again_does_not_slide_back() {
    let (mut world, mut step, [body, socket, held]) = staged("hand");
    // The item is interpolated in its own right, so it keeps a previous
    // transform while it lies on the ground
    world.entity_mut(socket).insert(Interpolated);
    snapshot(&mut world);
    step.run(&mut world);
    // Dropped: it leaves the skeleton and lies where it was, while the
    // skeleton's hand moves on
    world.detach_keep_world(socket);
    let dropped = global(&world, socket);
    for _ in 0..5 {
      snapshot(&mut world);
      step.run(&mut world);
    }
    assert_eq!(global(&world, socket), dropped);
    // Picked up by the same skeleton: the step puts it on the hand, drawn
    // there at once with what it holds, not sliding from where it lay
    world.entity_mut(socket).insert(ChildOf(body));
    snapshot(&mut world);
    step.run(&mut world);
    same(global(&world, socket), bone(&world, body, "hand"));
    assert!(drawn_where_it_stands(&world, socket));
    assert!(drawn_where_it_stands(&world, held));
    // And then between steps again
    snapshot(&mut world);
    step.run(&mut world);
    assert!(!drawn_where_it_stands(&world, socket));
  }

  #[test]
  fn a_socket_taken_off_and_put_back_does_not_slide_back() {
    let (mut world, mut step, [body, socket, held]) = staged("hand");
    // No longer a socket, the entity is its parent's ordinary child, and
    // the game puts it elsewhere
    world.entity_mut(socket).remove::<BoneSocket>();
    *world.get_mut::<Transform>(socket).expect("its transform") = Transform::from_xy(-80.0, 40.0);
    snapshot(&mut world);
    step.run(&mut world);
    assert_eq!(global(&world, socket).translation, Vec2::new(-30.0, 60.0));
    // A socket again, in a step's update after its snapshot: placed on the
    // hand and drawn there at once
    snapshot(&mut world);
    world.entity_mut(socket).insert(BoneSocket::new("hand"));
    step.run(&mut world);
    same(global(&world, socket), bone(&world, body, "hand"));
    assert!(drawn_where_it_stands(&world, socket));
    assert!(drawn_where_it_stands(&world, held));
    // A `BoneSocket` put on an entity is a new socket whenever it is put
    // on: one taken off and put back within a step, with no placing
    // missed, is drawn on its bone at once all the same
    snapshot(&mut world);
    step.run(&mut world);
    assert!(!drawn_where_it_stands(&world, socket), "between steps");
    snapshot(&mut world);
    world.entity_mut(socket).remove::<BoneSocket>();
    world.entity_mut(socket).insert(BoneSocket::new("hand"));
    step.run(&mut world);
    same(global(&world, socket), bone(&world, body, "hand"));
    assert!(drawn_where_it_stands(&world, socket));
  }

  #[test]
  fn a_socket_renamed_to_another_bone_is_drawn_on_it_at_once() {
    let (mut world, mut step, [body, socket, held]) = staged("hand");
    assert!(!drawn_where_it_stands(&world, socket), "between steps");
    // From the hand to the root, ten units away: a jump, not a move
    snapshot(&mut world);
    rename(&mut world, socket, "root");
    step.run(&mut world);
    same(global(&world, socket), bone(&world, body, "root"));
    assert!(drawn_where_it_stands(&world, socket));
    assert!(drawn_where_it_stands(&world, held));
    // On its new bone it is drawn between steps like any other
    snapshot(&mut world);
    step.run(&mut world);
    assert!(!drawn_where_it_stands(&world, socket));
  }

  /// An emitter on a child of `socket`, which has not jumped
  fn emitter_below(world: &mut World, socket: Entity) -> Entity {
    use crate::core::graphics::Image;
    use crate::core::particles::{ParticleEffect, ParticleEmitter};
    let image = world
      .resource_mut::<Assets<Image>>()
      .add(Image::solid_rect(2, 2, 255, 255, 255, 255));
    let effect = world
      .resource_mut::<Assets<ParticleEffect>>()
      .add(ParticleEffect::new(image));
    world
      .spawn((
        Transform::IDENTITY,
        ParticleEmitter::new(effect),
        ChildOf(socket),
      ))
      .id()
  }

  /// Whether the emitter is marked not to spread its next step's particles
  /// from where it stood, taking the mark off as the particle step does
  fn take_jump(world: &mut World, emitter: Entity) -> bool {
    let mut emitter = world
      .get_mut::<crate::core::particles::ParticleEmitter>(emitter)
      .expect("an emitter");
    std::mem::take(&mut emitter.teleported)
  }

  #[test]
  fn an_emitter_below_a_socket_is_marked_when_the_socket_jumps() {
    let (mut world, mut step, _) = world();
    let mut frame = appearing();
    let data = rig(&mut world);
    let [body, socket, _] = socketed(&mut world, data, "hand");
    let emitter = emitter_below(&mut world, socket);
    // The frame's pass puts the socket on its bone for the first time: the
    // emitter below it is told it jumped, as an entity's own emitter is on
    // a teleport, since the next step's snapshot clears the socket's skip
    // before the particle step can read it
    frame.run(&mut world);
    same(global(&world, socket), bone(&world, body, "hand"));
    assert!(take_jump(&mut world, emitter));
    // A steady step moves it with its bone and marks nothing
    for _ in 0..2 {
      snapshot(&mut world);
      step.run(&mut world);
      assert!(!take_jump(&mut world, emitter));
    }
    // A jump made outside a step marks it as it is made
    world.skip_interpolation(body);
    assert!(take_jump(&mut world, emitter));
    // And so does a first placing in a step, here on another bone
    snapshot(&mut world);
    rename(&mut world, socket, "root");
    step.run(&mut world);
    assert!(take_jump(&mut world, emitter));
  }

  #[test]
  fn a_socket_added_while_no_step_runs_is_on_its_bone_that_frame() {
    let (mut world, mut step, _) = world();
    let mut frame = appearing();
    let data = rig(&mut world);
    let body = world
      .spawn((
        Transform::from_xy(50.0, 20.0),
        Interpolated,
        Skeleton::new(data),
        turning(),
      ))
      .id();
    frame.run(&mut world);
    for _ in 0..2 {
      snapshot(&mut world);
      step.run(&mut world);
      frame.run(&mut world);
    }
    let looked = looks(&world);
    // The skeleton has its pose; a socket is added from a frame's code,
    // with no step to place it
    let socket = world.spawn((BoneSocket::new("hand"), ChildOf(body))).id();
    let held = world
      .spawn((Transform::from_xy(3.0, 0.0), ChildOf(socket)))
      .id();
    frame.run(&mut world);
    assert_eq!(looks(&world), looked + 1);
    same(global(&world, socket), bone(&world, body, "hand"));
    assert!(drawn_where_it_stands(&world, socket));
    assert!(drawn_where_it_stands(&world, held));
    // The frames after are steady again
    for _ in 0..5 {
      frame.run(&mut world);
    }
    assert_eq!(looks(&world), looked + 1);
    // And the first step moves it on with its bone, drawn between
    snapshot(&mut world);
    step.run(&mut world);
    same(global(&world, socket), bone(&world, body, "hand"));
    assert!(!drawn_where_it_stands(&world, socket));
  }
}
