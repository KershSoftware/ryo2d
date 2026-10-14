//! Keep Rapier in step with the world. Each fixed step, in
//! `FixedSet::Physics` while `PhysicsPaused` is false: forget what went away,
//! track disabled and parented entities, register what is new, push the
//! game's changes, step, write the simulated bodies back and turn contacts
//! into messages.

use super::body::{AngularVelocity, BodyKind, RigidBody};
use super::collision::Collider;
use super::config::PhysicsConfig;
use super::convert::angle_delta;
use super::world::Physics;
use crate::core::ecs::{
  Added, Allow, Changed, ChildOf, DetectChanges, DetectChangesMut, Disabled, Entity, Has,
  IntoScheduleConfigs, MessageWriter, Mut, Or, Query, Ref, RemovedComponents, Res, ResMut,
  Transform, Vec2, Velocity, With, Without,
};
use crate::core::graphics::GroupedCollider;
use crate::core::messages::{CollisionStarted, CollisionStopped};
use crate::core::scene::PhysicsPaused;
use crate::core::schedule::{EngineSchedules, FixedSet, Phase};
use crate::core::time::Time;

/// Run condition: physics steps unless the scene stack paused it.
pub(crate) fn physics_enabled(paused: Res<PhysicsPaused>) -> bool {
  !paused.0
}

type BodyItem<'a> = (
  Entity,
  &'a Transform,
  &'a Velocity,
  &'a AngularVelocity,
  &'a RigidBody,
);
/// A body whose velocities the system may reset (see `apply_edit`)
type BodyRow<'a> = (
  Entity,
  &'a Transform,
  &'a mut Velocity,
  &'a mut AngularVelocity,
  Ref<'a, RigidBody>,
);
type ColliderItem<'a> = (
  Entity,
  &'a Transform,
  Option<&'a Collider>,
  Option<&'a GroupedCollider>,
  Has<RigidBody>,
);
/// A `ColliderItem` as a query yields it (`Has` reads as a `bool`).
type ColliderRow<'a> = (
  Entity,
  &'a Transform,
  Option<&'a Collider>,
  Option<&'a GroupedCollider>,
  bool,
);
type HasPhysics = Or<(With<RigidBody>, With<Collider>, With<GroupedCollider>)>;
/// A root entity that gained a collider
type NewCollider = (
  Or<(Added<Collider>, Added<GroupedCollider>)>,
  Without<ChildOf>,
);
/// A root body or collider whose `Transform` changed
type Moved = (Changed<Transform>, HasPhysics, Without<ChildOf>);
/// A body whose `Velocity` or `AngularVelocity` changed
type Pushed = (
  With<RigidBody>,
  Or<(Changed<Velocity>, Changed<AngularVelocity>)>,
);

fn add_body(physics: &mut Physics, (entity, transform, velocity, angular, body): BodyItem) {
  physics.add_body(
    entity,
    transform.translation,
    transform.rotation,
    velocity.as_vec2(),
    angular.0,
    body,
  );
}

fn add_collider(
  physics: &mut Physics,
  (entity, transform, collider, grouped, has_body): ColliderRow,
) {
  let collider = match (collider, grouped) {
    (Some(collider), _) => collider.clone(),
    (None, Some(grouped)) => grouped_collider(grouped),
    (None, None) => return,
  };
  physics.add_collider(
    entity,
    has_body.then_some(entity),
    transform.translation,
    transform.rotation,
    &collider,
  );
}

/// A composite tile's box as a `Collider`: its offset is the box's top-left.
fn grouped_collider(grouped: &GroupedCollider) -> Collider {
  let size = Vec2::new(grouped.width, grouped.height);
  let center = Vec2::new(grouped.offset_x, grouped.offset_y) + size / 2.0;
  let mut collider = Collider::box_collider(size.x, size.y)
    .with_offset(center.x, center.y)
    .with_restitution(grouped.restitution)
    .with_friction(grouped.friction);
  collider.is_trigger = grouped.is_trigger;
  collider
}

/// Forget colliders and bodies whose components went away (despawn
/// included). Colliders run first; a body's colliders that stay become fixed.
pub(crate) fn forget_removed(
  mut physics: ResMut<Physics>,
  mut colliders: RemovedComponents<Collider>,
  mut grouped: RemovedComponents<GroupedCollider>,
  mut bodies: RemovedComponents<RigidBody>,
) {
  for entity in colliders.read().chain(grouped.read()) {
    physics.remove_collider(entity);
  }
  for entity in bodies.read() {
    physics.remove_body(entity);
  }
}

/// Disable the simulation of disabled entities; re-enable (or register, if
/// they were spawned disabled) re-enabled ones. The steps skip a disabled
/// entity, so what the game changed meanwhile (`Transform`, `Velocity`,
/// `RigidBody`) is applied as it comes back.
pub(crate) fn track_disabled(
  mut physics: ResMut<Physics>,
  config: Res<PhysicsConfig>,
  disabled: Query<Entity, (Added<Disabled>, Allow<Disabled>, HasPhysics)>,
  mut enabled: RemovedComponents<Disabled>,
  mut bodies: Query<BodyRow, Without<ChildOf>>,
  colliders: Query<ColliderItem, Without<ChildOf>>,
) {
  // Density depends on the unit: configure before anything registers
  physics.configure(&config);
  for entity in &disabled {
    physics.set_enabled(entity, false);
  }
  for entity in enabled.read() {
    if let Ok((entity, transform, mut velocity, mut angular, body)) = bodies.get_mut(entity) {
      add_body(
        &mut physics,
        (entity, transform, &velocity, &angular, &body),
      );
      // Edits made while it was disabled; `register_new` applying a recent
      // one again changes nothing
      apply_edit(&mut physics, entity, &body, &mut velocity, &mut angular);
      physics.place_body(entity, transform.translation, transform.rotation);
      physics.push_velocity(entity, velocity.as_vec2(), angular.0);
    }
    if let Ok(item) = colliders.get(entity) {
      add_collider(&mut physics, item);
      physics.move_collider(entity, item.1.translation, item.1.rotation);
    }
    physics.set_enabled(entity, true);
  }
}

/// Bodies and colliders on child entities are not simulated yet: one that
/// gained a parent leaves (logged once), one that lost it comes back.
pub(crate) fn track_parents(
  mut physics: ResMut<Physics>,
  parented: Query<Entity, (Added<ChildOf>, HasPhysics)>,
  mut detached: RemovedComponents<ChildOf>,
  bodies: Query<BodyItem, Without<ChildOf>>,
  colliders: Query<ColliderItem, Without<ChildOf>>,
) {
  for entity in &parented {
    physics.remove_collider(entity);
    physics.remove_body(entity);
    physics.warn_once(
      entity,
      "bodies and colliders on child entities are not simulated yet",
    );
  }
  for entity in detached.read() {
    if let Ok(item) = bodies.get(entity) {
      add_body(&mut physics, item);
    }
    if let Ok(item) = colliders.get(entity) {
      add_collider(&mut physics, item);
    }
  }
}

/// Apply an edited `RigidBody` in place. A dynamic body made kinematic or
/// fixed stops: on those `Velocity` is the game's input, so the velocities
/// the simulation wrote back are cleared (a velocity the game set itself is
/// kept).
fn apply_edit(
  physics: &mut Physics,
  entity: Entity,
  body: &RigidBody,
  velocity: &mut Mut<Velocity>,
  angular: &mut Mut<AngularVelocity>,
) {
  if body.kind != BodyKind::Dynamic
    && physics.is_simulated_velocity(entity, velocity.as_vec2(), angular.0)
  {
    velocity.set_if_neq(Velocity::zero());
    angular.set_if_neq(AngularVelocity(0.0));
  }
  physics.update_body(entity, body);
}

/// Register new bodies, then new colliders (so a collider spawned with its
/// body attaches to it), and apply edited bodies in place.
pub(crate) fn register_new(
  mut physics: ResMut<Physics>,
  config: Res<PhysicsConfig>,
  mut bodies: Query<BodyRow, (Changed<RigidBody>, Without<ChildOf>)>,
  colliders: Query<ColliderItem, NewCollider>,
) {
  physics.configure(&config);
  for (entity, transform, mut velocity, mut angular, body) in &mut bodies {
    if body.is_added() {
      add_body(
        &mut physics,
        (entity, transform, &velocity, &angular, &body),
      );
    } else {
      apply_edit(&mut physics, entity, &body, &mut velocity, &mut angular);
    }
  }
  for item in &colliders {
    add_collider(&mut physics, item);
  }
}

/// Push the game's writes since the last step. The write-back is an
/// ordinary change too, so values equal to Rapier's (within the sync
/// tolerance) are skipped: untouched bodies stay asleep.
pub(crate) fn push_changes(
  mut physics: ResMut<Physics>,
  time: Res<Time>,
  moved: Query<(Entity, &Transform, Has<RigidBody>), Moved>,
  pushed: Query<(Entity, &Velocity, &AngularVelocity), Pushed>,
  kinematic: Query<(Entity, &RigidBody, &Velocity, &AngularVelocity)>,
) {
  physics.begin_push();
  for (entity, transform, has_body) in &moved {
    if !(transform.translation.is_finite() && transform.rotation.is_finite()) {
      physics.warn_once(entity, "a non-finite Transform is not pushed to physics");
      continue;
    }
    if has_body {
      physics.move_body(entity, transform.translation, transform.rotation);
    } else {
      physics.move_collider(entity, transform.translation, transform.rotation);
    }
  }
  for (entity, velocity, angular) in &pushed {
    physics.push_velocity(entity, velocity.as_vec2(), angular.0);
  }
  let dt = time.fixed.timestep();
  for (entity, body, velocity, angular) in &kinematic {
    let still = velocity.as_vec2() == Vec2::ZERO && angular.0 == 0.0;
    if body.kind == BodyKind::Kinematic && !still {
      physics.drive_kinematic(entity, velocity.as_vec2(), angular.0, dt);
    }
  }
}

/// Step the simulation by the fixed timestep.
pub(crate) fn step_simulation(mut physics: ResMut<Physics>, time: Res<Time>) {
  crate::profile_scope!("cpu:physics");
  physics.step(time.fixed.timestep());
}

/// Write the bodies the step moved back. Rotation stays continuous: the
/// turn is added instead of wrapping into (-PI, PI].
pub(crate) fn read_back(
  physics: Res<Physics>,
  mut bodies: Query<(&mut Transform, &mut Velocity, &mut AngularVelocity), With<RigidBody>>,
) {
  for state in physics.moved_bodies() {
    let Ok((mut transform, mut velocity, mut angular)) = bodies.get_mut(state.entity) else {
      continue;
    };
    if transform.translation != state.translation {
      transform.translation = state.translation;
    }
    let turn = angle_delta(transform.rotation, state.rotation);
    if turn != 0.0 {
      transform.rotation += turn;
    }
    if state.dynamic {
      velocity.set_if_neq(Velocity::from_vec2(state.linvel));
      angular.set_if_neq(AngularVelocity(state.angvel));
    }
  }
}

/// Turn the step's contacts into messages.
pub(crate) fn emit_messages(
  physics: Res<Physics>,
  mut started: MessageWriter<CollisionStarted>,
  mut stopped: MessageWriter<CollisionStopped>,
) {
  for collision in &physics.collisions {
    let (entity_a, entity_b) = (collision.a.collider, collision.b.collider);
    if collision.started {
      started.write(CollisionStarted { entity_a, entity_b });
    } else {
      stopped.write(CollisionStopped { entity_a, entity_b });
    }
  }
}

/// Add the physics systems to `FixedSet::Physics`, chained, running only
/// while physics is not paused.
pub(crate) fn register_physics_systems(schedules: &mut EngineSchedules) {
  schedules.add_systems(
    Phase::FixedUpdate,
    (
      forget_removed,
      track_disabled,
      track_parents,
      register_new,
      push_changes,
      step_simulation,
      read_back,
      emit_messages,
    )
      .chain()
      .in_set(FixedSet::Physics)
      .run_if(physics_enabled),
  );
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::ecs::{Messages, World};
  use crate::core::messages::CollisionStarted;
  use crate::core::scene::PhysicsPaused;
  use crate::core::schedule::{EngineSchedules, Phase};
  use crate::core::time::Time;

  fn world(hz: u32) -> (World, EngineSchedules) {
    let mut world = World::new();
    world.insert_resource(Time::new(hz));
    world.insert_resource(Physics::new());
    world.insert_resource(PhysicsConfig::default());
    world.insert_resource(PhysicsPaused(false));
    crate::core::messages::register_engine_messages(&mut world);
    let mut schedules = EngineSchedules::new();
    register_physics_systems(&mut schedules);
    (world, schedules)
  }

  fn run(world: &mut World, s: &mut EngineSchedules, steps: usize) {
    for _ in 0..steps {
      s.run(Phase::FixedUpdate, world);
    }
  }

  fn no_gravity(world: &mut World) {
    *world.resource_mut::<PhysicsConfig>() = PhysicsConfig::default().zero_gravity();
  }

  fn at(world: &World, e: Entity) -> Transform {
    *world.get::<Transform>(e).unwrap()
  }

  /// A static floor: a bare collider 400 wide whose top is at `top`.
  fn floor(world: &mut World, top: f32) -> Entity {
    world
      .spawn((
        Transform::from_xy(0.0, top + 10.0),
        Collider::box_collider(400.0, 20.0),
      ))
      .id()
  }

  fn ball(world: &mut World, x: f32, y: f32) -> Entity {
    world
      .spawn((
        Transform::from_xy(x, y),
        RigidBody::dynamic(),
        Collider::circle_collider(10.0),
      ))
      .id()
  }

  fn drop_for_one_second(hz: u32) -> f32 {
    let (mut world, mut s) = world(hz);
    let e = ball(&mut world, 0.0, 0.0);
    run(&mut world, &mut s, hz as usize);
    at(&world, e).translation.y
  }

  #[test]
  fn fall_distance_per_second_is_independent_of_the_fixed_rate() {
    let at60 = drop_for_one_second(60);
    let at120 = drop_for_one_second(120);
    assert!((at60 - at120).abs() / at60 < 0.03, "{at60} vs {at120}");
    assert!(
      (at60 - 490.0).abs() < 60.0,
      "roughly g/2 after one second: {at60}"
    );
  }

  #[test]
  fn gravity_is_pixels_whatever_the_unit() {
    let fell = |ppm: f32| {
      let (mut world, mut s) = world(60);
      *world.resource_mut::<PhysicsConfig>() = PhysicsConfig::default().with_pixels_per_meter(ppm);
      let e = ball(&mut world, 0.0, 0.0);
      run(&mut world, &mut s, 60);
      at(&world, e).translation.y
    };
    let (at50, at100) = (fell(50.0), fell(100.0));
    assert!((at50 - at100).abs() < 2.0, "{at50} vs {at100}");
  }

  #[test]
  fn density_follows_the_configured_unit_from_the_first_step() {
    let (mut world, mut s) = world(60);
    *world.resource_mut::<PhysicsConfig>() = PhysicsConfig::default().with_pixels_per_meter(50.0);
    let e = world
      .spawn((
        Transform::IDENTITY,
        RigidBody::dynamic(),
        Collider::box_collider(100.0, 100.0),
      ))
      .id();
    run(&mut world, &mut s, 1);
    let physics = world.resource::<Physics>();
    let mass = physics
      .bodies
      .get(physics.body_entries[&e].handle)
      .unwrap()
      .mass();
    // 100 px at 50 px/m is 2 m: 4 m² at density 1
    assert!((mass - 4.0).abs() < 1e-3, "{mass}");
  }

  #[test]
  fn a_body_re_enabled_between_steps_registers_at_the_configured_unit() {
    let (mut world, mut s) = world(60);
    s.add_systems(Phase::Last, track_disabled);
    *world.resource_mut::<PhysicsConfig>() = PhysicsConfig::default().with_pixels_per_meter(50.0);
    let e = world
      .spawn((
        Transform::IDENTITY,
        RigidBody::dynamic(),
        Collider::box_collider(100.0, 100.0),
        Disabled,
      ))
      .id();
    world.entity_mut(e).remove::<Disabled>();
    // The frame's `Last` bookkeeping registers it before any fixed step runs
    s.run(Phase::Last, &mut world);
    let physics = world.resource::<Physics>();
    let mass = physics
      .bodies
      .get(physics.body_entries[&e].handle)
      .unwrap()
      .mass();
    assert!((mass - 4.0).abs() < 1e-3, "{mass}");
  }

  #[test]
  fn a_body_made_kinematic_with_a_velocity_moves_by_it() {
    let (mut world, mut s) = world(60);
    let e = ball(&mut world, 0.0, 0.0);
    run(&mut world, &mut s, 10);
    let made_still = ball(&mut world, 100.0, 0.0);
    run(&mut world, &mut s, 10);
    world.entity_mut(made_still).insert(RigidBody::kinematic());
    world
      .entity_mut(e)
      .insert((RigidBody::kinematic(), Velocity::new(60.0, 0.0)));
    let before = at(&world, e).translation;
    run(&mut world, &mut s, 60);
    let moved = at(&world, e).translation - before;
    assert!((moved - Vec2::new(60.0, 0.0)).length() < 0.5, "{moved}");
    assert_eq!(
      world.get::<Velocity>(made_still),
      Some(&Velocity::zero()),
      "the velocity the simulation wrote back is cleared"
    );
  }

  #[test]
  fn a_bare_collider_is_a_static_wall() {
    let (mut world, mut s) = world(60);
    let wall = floor(&mut world, 100.0);
    let b = ball(&mut world, 0.0, 0.0);
    run(&mut world, &mut s, 120);
    assert!(
      (at(&world, b).translation.y - 90.0).abs() < 2.0,
      "{:?}",
      at(&world, b)
    );
    assert_eq!(at(&world, wall).translation, Vec2::new(0.0, 110.0));
    let physics = world.resource::<Physics>();
    assert_eq!((physics.body_count(), physics.collider_count()), (1, 2));
  }

  #[test]
  fn a_grouped_collider_spans_its_offset_and_size() {
    let (mut world, mut s) = world(60);
    let e = world
      .spawn((
        Transform::from_xy(100.0, 100.0),
        GroupedCollider::new(40.0, 20.0).with_offset(-20.0, 0.0),
      ))
      .id();
    run(&mut world, &mut s, 1);
    let physics = world.resource::<Physics>();
    let collider = &physics.colliders[physics.collider_entries[&e].handle];
    let center = Vec2::new(collider.translation().x, collider.translation().y);
    assert!(
      (center - Vec2::new(100.0, 110.0)).length() < 1e-3,
      "{center}"
    );
  }

  #[test]
  fn a_fixed_body_never_moves() {
    let (mut world, mut s) = world(60);
    let platform = world
      .spawn((
        Transform::from_xy(0.0, 110.0),
        RigidBody::fixed(),
        Collider::box_collider(400.0, 20.0),
      ))
      .id();
    ball(&mut world, 0.0, 0.0);
    run(&mut world, &mut s, 120);
    assert_eq!(at(&world, platform).translation, Vec2::new(0.0, 110.0));
  }

  fn dropped_box(body: RigidBody) -> (f32, f32) {
    let (mut world, mut s) = world(60);
    floor(&mut world, 100.0);
    let e = world
      .spawn((
        Transform::from_xy(0.0, 40.0).with_rotation(0.4),
        body,
        Collider::box_collider(40.0, 40.0),
      ))
      .id();
    let mut fastest_spin: f32 = 0.0;
    for _ in 0..120 {
      run(&mut world, &mut s, 1);
      fastest_spin = fastest_spin.max(world.get::<AngularVelocity>(e).unwrap().0.abs());
    }
    (at(&world, e).rotation, fastest_spin)
  }

  #[test]
  fn a_box_landing_on_its_corner_rotates() {
    let (rotation, spin) = dropped_box(RigidBody::dynamic());
    assert!((rotation - 0.4).abs() > 0.1, "it tipped over: {rotation}");
    assert!(spin > 0.5, "AngularVelocity was written back: {spin}");
  }

  #[test]
  fn lock_rotation_holds() {
    let (rotation, spin) = dropped_box(RigidBody::dynamic().lock_rotation());
    assert!((rotation - 0.4).abs() < 1e-4, "{rotation}");
    assert_eq!(spin, 0.0);
  }

  #[test]
  fn a_spinning_body_s_rotation_stays_continuous() {
    let (mut world, mut s) = world(60);
    no_gravity(&mut world);
    let e = ball(&mut world, 0.0, 0.0);
    world.get_mut::<AngularVelocity>(e).unwrap().0 = 10.0;
    run(&mut world, &mut s, 60);
    let rotation = at(&world, e).rotation;
    assert!(
      (rotation - 10.0).abs() < 0.2,
      "not wrapped into (-PI, PI]: {rotation}"
    );
  }

  #[test]
  fn a_kinematic_body_moved_by_velocity_writes_its_transform_back() {
    let (mut world, mut s) = world(60);
    let e = world
      .spawn((
        Transform::IDENTITY,
        RigidBody::kinematic(),
        Velocity::new(60.0, 0.0),
        Collider::box_collider(10.0, 10.0),
      ))
      .id();
    run(&mut world, &mut s, 60);
    assert!(
      (at(&world, e).translation.x - 60.0).abs() < 0.5,
      "{:?}",
      at(&world, e)
    );
    assert_eq!(world.get::<Velocity>(e), Some(&Velocity::new(60.0, 0.0)));
  }

  #[test]
  fn a_kinematic_platform_moved_by_transform_carries_a_resting_box() {
    let (mut world, mut s) = world(60);
    let platform = world
      .spawn((
        Transform::from_xy(0.0, 110.0),
        RigidBody::kinematic(),
        Collider::box_collider(200.0, 20.0),
      ))
      .id();
    let crate_ = world
      .spawn((
        Transform::from_xy(0.0, 80.0),
        RigidBody::dynamic().lock_rotation(),
        Collider::box_collider(20.0, 20.0).with_friction(1.0),
      ))
      .id();
    run(&mut world, &mut s, 60);
    for _ in 0..60 {
      world.get_mut::<Transform>(platform).unwrap().translation.x += 1.0;
      run(&mut world, &mut s, 1);
    }
    let x = at(&world, crate_).translation.x;
    assert!((x - 60.0).abs() < 8.0, "carried with the platform: {x}");
  }

  #[test]
  fn higher_dominance_pushes_without_being_pushed() {
    let (mut world, mut s) = world(60);
    no_gravity(&mut world);
    let pusher = world
      .spawn((
        Transform::from_xy(0.0, 0.0),
        RigidBody::dynamic().lock_rotation().with_dominance(1),
        Velocity::new(100.0, 0.0),
        Collider::box_collider(20.0, 20.0),
      ))
      .id();
    let pushed = world
      .spawn((
        Transform::from_xy(50.0, 0.0),
        RigidBody::dynamic().lock_rotation(),
        Velocity::new(-100.0, 0.0),
        Collider::box_collider(20.0, 20.0),
      ))
      .id();
    run(&mut world, &mut s, 60);
    assert!(world.get::<Velocity>(pusher).unwrap().dx > 90.0);
    assert!(world.get::<Velocity>(pushed).unwrap().dx > 0.0);
  }

  #[test]
  fn a_resting_body_sleeps_and_the_sync_does_not_wake_it() {
    let (mut world, mut s) = world(60);
    floor(&mut world, 100.0);
    let b = ball(&mut world, 0.0, 50.0);
    run(&mut world, &mut s, 600);
    assert!(
      world.resource::<Physics>().is_sleeping(b),
      "ten seconds at rest"
    );
    let before = at(&world, b);
    run(&mut world, &mut s, 60);
    assert!(
      world.resource::<Physics>().is_sleeping(b),
      "sync must not wake it"
    );
    assert_eq!(at(&world, b), before);
  }

  #[test]
  fn never_sleep_keeps_a_resting_body_awake() {
    let (mut world, mut s) = world(60);
    floor(&mut world, 100.0);
    let b = world
      .spawn((
        Transform::from_xy(0.0, 50.0),
        RigidBody::dynamic().never_sleep(),
        Collider::circle_collider(10.0),
      ))
      .id();
    run(&mut world, &mut s, 600);
    assert!(!world.resource::<Physics>().is_sleeping(b));
  }

  fn fall_after(change: impl FnOnce(&mut World, Entity)) -> f32 {
    let (mut world, mut s) = world(60);
    let e = ball(&mut world, 0.0, 0.0);
    run(&mut world, &mut s, 10);
    change(&mut world, e);
    let before = at(&world, e).translation.y;
    run(&mut world, &mut s, 10);
    at(&world, e).translation.y - before
  }

  #[test]
  fn a_body_made_kinematic_stops_falling() {
    let fell = fall_after(|world, e| {
      world.entity_mut(e).insert(RigidBody::kinematic());
    });
    assert!(fell.abs() < 1e-3, "a kinematic body fell {fell}");
  }

  #[test]
  fn a_body_removed_and_added_again_in_one_tick_keeps_simulating() {
    let fell = fall_after(|world, e| {
      world.entity_mut(e).remove::<RigidBody>();
      world.entity_mut(e).insert(RigidBody::dynamic());
    });
    assert!(fell > 1.0, "the body stopped: fell {fell}");
  }

  #[test]
  fn a_non_finite_transform_is_ignored() {
    let (mut world, mut s) = world(60);
    let e = ball(&mut world, 0.0, 0.0);
    run(&mut world, &mut s, 5);
    world.get_mut::<Transform>(e).unwrap().translation.x = f32::NAN;
    run(&mut world, &mut s, 5);
    let (position, rotation) = world.resource::<Physics>().body_pose(e).unwrap();
    assert!(position.is_finite() && rotation.is_finite());
    assert!(
      at(&world, e).translation.is_finite(),
      "the write-back restored it"
    );
  }

  #[test]
  fn spawn_and_despawn_in_one_step_leaves_nothing() {
    let (mut world, mut s) = world(60);
    let b = ball(&mut world, 0.0, 0.0);
    let f = floor(&mut world, 100.0);
    world.despawn(b);
    world.despawn(f);
    run(&mut world, &mut s, 1);
    let b = ball(&mut world, 0.0, 0.0);
    run(&mut world, &mut s, 1);
    world.despawn(b);
    run(&mut world, &mut s, 1);
    let physics = world.resource::<Physics>();
    assert_eq!((physics.body_count(), physics.collider_count()), (0, 0));
  }

  #[test]
  fn removing_the_body_leaves_a_static_collider() {
    let (mut world, mut s) = world(60);
    let platform = world
      .spawn((
        Transform::from_xy(0.0, 110.0),
        RigidBody::kinematic(),
        Collider::box_collider(400.0, 20.0),
      ))
      .id();
    run(&mut world, &mut s, 1);
    world.entity_mut(platform).remove::<RigidBody>();
    let b = ball(&mut world, 0.0, 0.0);
    run(&mut world, &mut s, 120);
    let physics = world.resource::<Physics>();
    assert_eq!((physics.body_count(), physics.collider_count()), (1, 2));
    assert!((at(&world, b).translation.y - 90.0).abs() < 2.0);
  }

  #[test]
  fn a_disabled_floor_lets_a_ball_through_and_stays_registered() {
    let (mut world, mut s) = world(60);
    let f = floor(&mut world, 100.0);
    run(&mut world, &mut s, 1);
    world.entity_mut(f).insert(Disabled);
    let b = ball(&mut world, 0.0, 0.0);
    run(&mut world, &mut s, 60);
    assert!(at(&world, b).translation.y > 150.0, "fell through");
    assert_eq!(world.resource::<Physics>().collider_count(), 2);
    world.entity_mut(f).remove::<Disabled>();
    let caught = ball(&mut world, 0.0, 0.0);
    run(&mut world, &mut s, 120);
    assert!((at(&world, caught).translation.y - 90.0).abs() < 2.0);
  }

  #[test]
  fn edits_made_while_disabled_apply_when_re_enabled() {
    let (mut world, mut s) = world(60);
    let e = ball(&mut world, 0.0, 0.0);
    run(&mut world, &mut s, 10);
    world.entity_mut(e).insert(Disabled);
    run(&mut world, &mut s, 1);
    // Moved, stopped and made weightless while out of the simulation, with
    // steps running in between
    world.get_mut::<Transform>(e).unwrap().translation = Vec2::new(300.0, 0.0);
    *world.get_mut::<Velocity>(e).unwrap() = Velocity::zero();
    world
      .entity_mut(e)
      .insert(RigidBody::dynamic().with_gravity_scale(0.0));
    run(&mut world, &mut s, 5);
    world.entity_mut(e).remove::<Disabled>();
    run(&mut world, &mut s, 30);
    let (position, _) = world.resource::<Physics>().body_pose(e).unwrap();
    assert!(
      (position - Vec2::new(300.0, 0.0)).length() < 1e-3,
      "{position}"
    );
    assert_eq!(at(&world, e).translation, position);
  }

  #[test]
  fn re_enabled_and_made_kinematic_with_a_velocity_in_one_tick_moves_by_it() {
    let (mut world, mut s) = world(60);
    let e = ball(&mut world, 0.0, 0.0);
    run(&mut world, &mut s, 10);
    world.entity_mut(e).insert(Disabled);
    run(&mut world, &mut s, 1);
    world.entity_mut(e).remove::<Disabled>();
    world
      .entity_mut(e)
      .insert((RigidBody::kinematic(), Velocity::new(60.0, 0.0)));
    let before = at(&world, e).translation;
    run(&mut world, &mut s, 60);
    let moved = at(&world, e).translation - before;
    assert!((moved - Vec2::new(60.0, 0.0)).length() < 0.5, "{moved}");
    assert_eq!(world.get::<Velocity>(e), Some(&Velocity::new(60.0, 0.0)));
  }

  #[test]
  fn an_edit_made_while_disabled_applies_when_only_the_frame_end_sees_the_re_enable() {
    let (mut world, mut s) = world(60);
    s.add_systems(Phase::Last, track_disabled);
    let e = ball(&mut world, 0.0, 0.0);
    run(&mut world, &mut s, 10);
    world.entity_mut(e).insert(Disabled);
    run(&mut world, &mut s, 1);
    s.run(Phase::Last, &mut world);
    // Edited while disabled, then a fixed step, then re-enabled later in the
    // frame: only the frame-end bookkeeping sees the removal before the
    // removal events expire
    world.entity_mut(e).insert(RigidBody::kinematic());
    run(&mut world, &mut s, 1);
    world.entity_mut(e).remove::<Disabled>();
    s.run(Phase::Last, &mut world);
    world.clear_trackers();
    world.clear_trackers();
    let before = at(&world, e).translation;
    run(&mut world, &mut s, 10);
    assert_eq!(at(&world, e).translation, before, "it stayed dynamic");
  }

  /// A ball drifting slower than the sleep threshold, asleep after ten
  /// seconds: Rapier stops it, though it was still moving.
  fn sleeping_ball() -> (World, EngineSchedules, Entity) {
    let (mut world, mut s) = world(60);
    no_gravity(&mut world);
    let b = ball(&mut world, 0.0, 0.0);
    world.get_mut::<Velocity>(b).unwrap().dx = 10.0;
    run(&mut world, &mut s, 600);
    assert!(world.resource::<Physics>().is_sleeping(b));
    (world, s, b)
  }

  #[test]
  fn a_body_falling_asleep_writes_zero_velocities() {
    let (world, _, b) = sleeping_ball();
    assert_eq!(world.get::<Velocity>(b), Some(&Velocity::zero()));
    assert_eq!(world.get::<AngularVelocity>(b), Some(&AngularVelocity(0.0)));
  }

  #[test]
  fn a_sleeping_body_made_kinematic_with_a_velocity_moves_by_it() {
    let (mut world, mut s, b) = sleeping_ball();
    world
      .entity_mut(b)
      .insert((RigidBody::kinematic(), Velocity::new(60.0, 0.0)));
    let before = at(&world, b).translation;
    run(&mut world, &mut s, 60);
    let moved = at(&world, b).translation - before;
    assert!((moved - Vec2::new(60.0, 0.0)).length() < 0.5, "{moved}");
  }

  #[test]
  fn a_body_disabled_while_asleep_stays_asleep_and_put_when_re_enabled() {
    let (mut world, mut s, b) = sleeping_ball();
    let before = at(&world, b);
    world.entity_mut(b).insert(Disabled);
    run(&mut world, &mut s, 5);
    world.entity_mut(b).remove::<Disabled>();
    run(&mut world, &mut s, 60);
    assert!(world.resource::<Physics>().is_sleeping(b));
    assert_eq!(at(&world, b), before);
  }

  #[test]
  fn a_disabled_body_keeps_its_velocity() {
    let (mut world, mut s) = world(60);
    no_gravity(&mut world);
    let e = world
      .spawn((
        Transform::IDENTITY,
        RigidBody::dynamic(),
        Velocity::new(60.0, 0.0),
        Collider::circle_collider(4.0),
      ))
      .id();
    run(&mut world, &mut s, 30);
    world.entity_mut(e).insert(Disabled);
    let parked = at(&world, e).translation;
    run(&mut world, &mut s, 30);
    assert_eq!(
      at(&world, e).translation,
      parked,
      "it did not move while disabled"
    );
    world.entity_mut(e).remove::<Disabled>();
    run(&mut world, &mut s, 30);
    let x = at(&world, e).translation.x;
    assert!((x - 60.0).abs() < 0.5, "a second of motion at 60 px/s: {x}");
    assert_eq!(world.get::<Velocity>(e), Some(&Velocity::new(60.0, 0.0)));
  }

  #[test]
  fn a_non_finite_transform_never_reaches_a_re_enabled_collider() {
    let (mut world, mut s) = world(60);
    let f = floor(&mut world, 100.0);
    run(&mut world, &mut s, 1);
    world.entity_mut(f).insert(Disabled);
    run(&mut world, &mut s, 1);
    world.get_mut::<Transform>(f).unwrap().translation = Vec2::new(f32::NAN, 300.0);
    world.entity_mut(f).remove::<Disabled>();
    run(&mut world, &mut s, 1);
    let physics = world.resource::<Physics>();
    let collider = &physics.colliders[physics.collider_entries[&f].handle];
    assert!(
      collider.translation().is_finite(),
      "{:?}",
      collider.translation()
    );
  }

  #[test]
  fn a_body_turned_thousands_of_radians_still_sleeps() {
    let (mut world, mut s) = world(60);
    no_gravity(&mut world);
    // As if a wheel had rolled for minutes: the continuous rotation is large
    // and f32 round-off at that magnitude exceeds the sync tolerance
    let e = world
      .spawn((
        Transform::IDENTITY.with_rotation(3000.0),
        RigidBody::dynamic(),
        Collider::box_collider(10.0, 10.0),
      ))
      .id();
    run(&mut world, &mut s, 600);
    assert!(
      world.resource::<Physics>().is_sleeping(e),
      "the write-back's round-off must not count as a game write"
    );
  }

  #[test]
  fn a_body_made_kinematic_while_disabled_stays_put_when_re_enabled() {
    let (mut world, mut s) = world(60);
    let e = ball(&mut world, 0.0, 0.0);
    run(&mut world, &mut s, 10);
    world.entity_mut(e).insert(Disabled);
    run(&mut world, &mut s, 1);
    world.entity_mut(e).insert(RigidBody::kinematic());
    run(&mut world, &mut s, 1);
    world.entity_mut(e).remove::<Disabled>();
    let before = at(&world, e).translation;
    run(&mut world, &mut s, 10);
    assert_eq!(at(&world, e).translation, before);
    assert_eq!(world.get::<Velocity>(e), Some(&Velocity::zero()));
  }

  #[test]
  fn a_body_spawned_disabled_registers_when_enabled() {
    let (mut world, mut s) = world(60);
    let e = world
      .spawn((
        Transform::IDENTITY,
        RigidBody::dynamic(),
        Collider::circle_collider(4.0),
        Disabled,
      ))
      .id();
    run(&mut world, &mut s, 2);
    assert_eq!(world.resource::<Physics>().body_count(), 0);
    world.entity_mut(e).remove::<Disabled>();
    run(&mut world, &mut s, 2);
    assert_eq!(world.resource::<Physics>().body_count(), 1);
  }

  #[test]
  fn a_collider_added_after_the_body_still_registers() {
    let (mut world, mut s) = world(60);
    let e = world
      .spawn((Transform::IDENTITY, RigidBody::dynamic()))
      .id();
    run(&mut world, &mut s, 1);
    assert_eq!(world.resource::<Physics>().collider_count(), 0);
    world.entity_mut(e).insert(Collider::circle_collider(4.0));
    run(&mut world, &mut s, 1);
    assert_eq!(world.resource::<Physics>().collider_count(), 1);
  }

  #[test]
  fn a_game_write_to_transform_moves_a_kinematic_body_over_the_step() {
    let (mut world, mut s) = world(60);
    let e = world
      .spawn((
        Transform::IDENTITY,
        RigidBody::kinematic(),
        Collider::box_collider(4.0, 4.0),
      ))
      .id();
    run(&mut world, &mut s, 1);
    world.get_mut::<Transform>(e).unwrap().translation.x = 50.0;
    run(&mut world, &mut s, 1);
    let (position, _) = world.resource::<Physics>().body_pose(e).unwrap();
    assert!((position.x - 50.0).abs() < 1e-3);
  }

  #[test]
  fn a_body_on_a_child_is_not_simulated_until_detached() {
    use crate::core::ecs::ChildOf;
    let (mut world, mut s) = world(60);
    let parent = world.spawn(Transform::IDENTITY).id();
    let child = world
      .spawn((
        Transform::from_xy(5.0, 0.0),
        RigidBody::dynamic(),
        Collider::box_collider(4.0, 4.0),
        ChildOf(parent),
      ))
      .id();
    run(&mut world, &mut s, 1);
    assert_eq!(world.resource::<Physics>().body_count(), 0);
    world.entity_mut(child).remove::<ChildOf>();
    run(&mut world, &mut s, 1);
    assert_eq!(world.resource::<Physics>().body_count(), 1);
  }

  #[test]
  fn a_falling_body_s_global_transform_follows_it() {
    use crate::core::ecs::GlobalTransform;
    use crate::core::ecs::propagate::propagate_transforms;
    let (mut world, mut s) = world(60);
    s.add_systems(
      Phase::FixedUpdate,
      propagate_transforms.in_set(FixedSet::Transforms),
    );
    let e = ball(&mut world, 0.0, 0.0);
    run(&mut world, &mut s, 30);
    let local = at(&world, e).translation;
    let global = world.get::<GlobalTransform>(e).unwrap().translation();
    assert!(local.y > 0.0);
    assert_eq!(global, local, "propagation sees physics' writes");
  }

  #[test]
  fn paused_physics_runs_no_step() {
    let (mut world, mut s) = world(60);
    let e = ball(&mut world, 0.0, 0.0);
    world.resource_mut::<PhysicsPaused>().0 = true;
    run(&mut world, &mut s, 30);
    assert_eq!(at(&world, e).translation.y, 0.0);
  }

  #[test]
  fn touching_bodies_write_collision_messages() {
    let (mut world, mut s) = world(60);
    let f = floor(&mut world, 100.0);
    let b = ball(&mut world, 0.0, 0.0);
    run(&mut world, &mut s, 60);
    let messages = world.resource::<Messages<CollisionStarted>>();
    let started: Vec<_> = messages.get_cursor().read(messages).copied().collect();
    assert!(
      started
        .iter()
        .any(|m| m.involves(b) && m.other(b) == Some(f)),
      "{started:?}"
    );
  }
}
