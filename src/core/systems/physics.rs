//! Physics: keep Rapier in step with the world, step it, and read it back.
//!
//! Each fixed step, in `FixedSet::Physics` and only while `PhysicsPaused` is
//! false: register new bodies, unregister removed or disabled ones, push the
//! game's writes to `Transform` and `Velocity` into Rapier, step by the fixed
//! timestep, write the simulated positions back, and turn Rapier's contacts
//! into `CollisionStarted`/`CollisionStopped` messages.

use crate::core::ecs::{
  Added, Allow, Changed, DetectChangesMut, Disabled, Entity, IntoScheduleConfigs, MessageWriter,
  Or, Query, RemovedComponents, Res, ResMut, Transform, Velocity, With,
};
use crate::core::graphics::GroupedCollider;
use crate::core::messages::{CollisionStarted, CollisionStopped};
use crate::core::physics::{Collider, PhysicsConfig, RapierPhysicsWorld, RigidBody};
use crate::core::scene::PhysicsPaused;
use crate::core::schedule::{EngineSchedules, FixedSet, Phase};
use crate::core::time::Time;

/// Run condition: physics steps unless the scene stack paused it.
pub fn physics_enabled(paused: Res<PhysicsPaused>) -> bool {
  !paused.0
}

type BodyItem<'a> = (
  Entity,
  &'a Transform,
  Option<&'a Velocity>,
  &'a RigidBody,
  Option<&'a Collider>,
  Option<&'a GroupedCollider>,
);

/// A body gaining its `RigidBody` or a collider
type NewBody = Or<(Added<RigidBody>, Added<Collider>, Added<GroupedCollider>)>;
/// A body whose entity was just disabled
type NewlyDisabled = (With<RigidBody>, Added<Disabled>, Allow<Disabled>);
/// A body whose `Transform` changed
type Moved = (With<RigidBody>, Changed<Transform>);
/// A body whose `Velocity` changed
type Pushed = (With<RigidBody>, Changed<Velocity>);

/// Register bodies that gained a `RigidBody` or a collider, and bodies whose
/// entity was re-enabled.
pub fn register_new_bodies(
  mut physics: ResMut<RapierPhysicsWorld>,
  added: Query<BodyItem, NewBody>,
  bodies: Query<BodyItem, With<RigidBody>>,
  mut re_enabled: RemovedComponents<Disabled>,
) {
  for item in &added {
    register(&mut physics, item);
  }
  for entity in re_enabled.read() {
    if let Ok(item) = bodies.get(entity) {
      register(&mut physics, item);
    }
  }
}

fn register(
  physics: &mut RapierPhysicsWorld,
  (entity, transform, velocity, body, collider, grouped): BodyItem,
) {
  if physics.is_registered(entity) {
    return;
  }
  match (collider, grouped) {
    (Some(collider), _) => physics.register_entity(entity, transform, velocity, body, collider),
    (None, Some(grouped)) => {
      physics.register_grouped_entity(entity, transform, velocity, body, grouped)
    }
    (None, None) => {}
  }
}

/// Unregister bodies whose `RigidBody` went away (despawn included) and
/// bodies of entities that were disabled.
pub fn unregister_removed_bodies(
  mut physics: ResMut<RapierPhysicsWorld>,
  mut removed: RemovedComponents<RigidBody>,
  disabled: Query<Entity, NewlyDisabled>,
) {
  for entity in removed.read() {
    physics.unregister_entity(entity);
  }
  for entity in &disabled {
    physics.unregister_entity(entity);
  }
}

/// Push the game's writes since the last step: a moved `Transform` teleports
/// the body (or targets a kinematic body), a changed `Velocity` sets its
/// velocity.
///
/// Physics' own write-back also changes these components, and propagation
/// must see it. A value equal to the body's is that write-back (or a no-op),
/// so only values that differ reach Rapier: untouched bodies stay asleep.
pub fn sync_to_physics(
  mut physics: ResMut<RapierPhysicsWorld>,
  moved: Query<(Entity, &Transform), Moved>,
  pushed: Query<(Entity, &Velocity), Pushed>,
) {
  for (entity, transform) in &moved {
    if physics.body_position(entity) != Some(transform.translation) {
      physics.set_transform(entity, transform);
    }
  }
  for (entity, velocity) in &pushed {
    if physics.body_velocity(entity) != Some(velocity.as_vec2()) {
      physics.set_velocity(entity, velocity);
    }
  }
}

/// Step the simulation by the fixed timestep.
pub fn step_physics(
  mut physics: ResMut<RapierPhysicsWorld>,
  config: Res<PhysicsConfig>,
  time: Res<Time>,
) {
  crate::profile_scope!("cpu:physics");
  physics.step(config.gravity, time.fixed.timestep());
}

/// Write simulated positions and velocities of awake dynamic bodies back.
///
/// These are ordinary changes, so propagation and rendering see bodies move;
/// `sync_to_physics` tells them from game writes by value.
pub fn sync_from_physics(
  physics: Res<RapierPhysicsWorld>,
  mut bodies: Query<(&mut Transform, Option<&mut Velocity>), With<RigidBody>>,
) {
  for (entity, position, velocity) in physics.read_back() {
    let Ok((mut transform, body_velocity)) = bodies.get_mut(entity) else {
      continue;
    };
    if transform.translation != position {
      transform.translation = position;
    }
    if let Some(mut body_velocity) = body_velocity {
      let velocity = Velocity::from_vec2(velocity);
      body_velocity.set_if_neq(velocity);
    }
  }
}

/// Turn the step's contacts into messages.
pub fn emit_collision_messages(
  mut physics: ResMut<RapierPhysicsWorld>,
  mut started: MessageWriter<CollisionStarted>,
  mut stopped: MessageWriter<CollisionStopped>,
) {
  for event in physics.drain_collision_events() {
    if event.is_started() {
      started.write(CollisionStarted {
        entity_a: event.entity_a,
        entity_b: event.entity_b,
      });
    } else {
      stopped.write(CollisionStopped {
        entity_a: event.entity_a,
        entity_b: event.entity_b,
      });
    }
  }
}

/// Add the physics systems to `FixedSet::Physics`, chained, running only
/// while physics is not paused.
pub fn register_physics_systems(schedules: &mut EngineSchedules) {
  schedules.add_systems(
    Phase::FixedUpdate,
    (
      register_new_bodies,
      unregister_removed_bodies,
      sync_to_physics,
      step_physics,
      sync_from_physics,
      emit_collision_messages,
    )
      .chain()
      .in_set(FixedSet::Physics)
      .run_if(physics_enabled),
  );
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::ecs::{Disabled, Transform, World};
  use crate::core::physics::{Collider, PhysicsConfig, RapierPhysicsWorld, RigidBody};
  use crate::core::scene::PhysicsPaused;
  use crate::core::schedule::{EngineSchedules, Phase};
  use crate::core::time::Time;

  fn world(hz: u32) -> (World, EngineSchedules) {
    let mut world = World::new();
    world.insert_resource(Time::new(hz));
    world.insert_resource(RapierPhysicsWorld::new());
    world.insert_resource(PhysicsConfig {
      gravity: (0.0, 980.0),
    });
    world.insert_resource(PhysicsPaused(false));
    crate::core::messages::register_engine_messages(&mut world);
    let mut schedules = EngineSchedules::new();
    register_physics_systems(&mut schedules);
    (world, schedules)
  }

  fn drop_for_one_second(hz: u32) -> f32 {
    let (mut world, mut s) = world(hz);
    let e = world
      .spawn((
        Transform::from_xy(0.0, 0.0),
        RigidBody::new(),
        Collider::circle_collider(8.0),
      ))
      .id();
    for _ in 0..hz {
      s.run(Phase::FixedUpdate, &mut world);
    }
    world.get::<Transform>(e).unwrap().translation.y
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
  fn added_bodies_register_and_removed_or_disabled_bodies_unregister() {
    let (mut world, mut s) = world(60);
    let e = world
      .spawn((
        Transform::IDENTITY,
        RigidBody::new(),
        Collider::box_collider(4.0, 4.0),
      ))
      .id();
    s.run(Phase::FixedUpdate, &mut world);
    assert!(world.resource::<RapierPhysicsWorld>().is_registered(e));
    world.entity_mut(e).insert(Disabled);
    s.run(Phase::FixedUpdate, &mut world);
    assert!(!world.resource::<RapierPhysicsWorld>().is_registered(e));
    world.entity_mut(e).remove::<Disabled>();
    s.run(Phase::FixedUpdate, &mut world);
    assert!(world.resource::<RapierPhysicsWorld>().is_registered(e));
    world.despawn(e);
    s.run(Phase::FixedUpdate, &mut world);
    assert!(!world.resource::<RapierPhysicsWorld>().is_registered(e));
  }

  #[test]
  fn a_collider_added_after_the_body_still_registers() {
    let (mut world, mut s) = world(60);
    let e = world.spawn((Transform::IDENTITY, RigidBody::new())).id();
    s.run(Phase::FixedUpdate, &mut world);
    assert!(!world.resource::<RapierPhysicsWorld>().is_registered(e));
    world.entity_mut(e).insert(Collider::circle_collider(4.0));
    s.run(Phase::FixedUpdate, &mut world);
    assert!(world.resource::<RapierPhysicsWorld>().is_registered(e));
  }

  #[test]
  fn a_resting_body_is_not_woken_by_the_sync() {
    let (mut world, mut s) = world(60);
    world.spawn((
      Transform::from_xy(0.0, 100.0),
      RigidBody::kinematic(),
      Collider::box_collider(400.0, 20.0),
    ));
    let ball = world
      .spawn((
        Transform::from_xy(0.0, 50.0),
        RigidBody::new(),
        Collider::circle_collider(10.0),
      ))
      .id();
    for _ in 0..600 {
      s.run(Phase::FixedUpdate, &mut world);
    }
    assert!(
      world.resource::<RapierPhysicsWorld>().is_sleeping(ball),
      "ten seconds at rest"
    );
    let before = world.get::<Transform>(ball).unwrap().translation;
    for _ in 0..60 {
      s.run(Phase::FixedUpdate, &mut world);
    }
    assert!(
      world.resource::<RapierPhysicsWorld>().is_sleeping(ball),
      "sync must not wake it"
    );
    assert_eq!(world.get::<Transform>(ball).unwrap().translation, before);
  }

  #[test]
  fn a_game_write_to_transform_teleports_and_a_kinematic_moves_by_transform() {
    let (mut world, mut s) = world(60);
    let kin = world
      .spawn((
        Transform::from_xy(0.0, 0.0),
        RigidBody::kinematic(),
        Collider::box_collider(4.0, 4.0),
      ))
      .id();
    s.run(Phase::FixedUpdate, &mut world);
    world.get_mut::<Transform>(kin).unwrap().translation.x = 50.0;
    s.run(Phase::FixedUpdate, &mut world);
    let pos = world
      .resource::<RapierPhysicsWorld>()
      .body_position(kin)
      .unwrap();
    assert!((pos.x - 50.0).abs() < 1e-3);
  }

  #[test]
  fn a_falling_body_s_global_transform_follows_it() {
    use crate::core::ecs::{GlobalTransform, IntoScheduleConfigs, propagate_transforms};
    use crate::core::schedule::FixedSet;
    let (mut world, mut s) = world(60);
    s.add_systems(
      Phase::FixedUpdate,
      propagate_transforms.in_set(FixedSet::Transforms),
    );
    let e = world
      .spawn((
        Transform::IDENTITY,
        RigidBody::new(),
        Collider::circle_collider(8.0),
      ))
      .id();
    for _ in 0..30 {
      s.run(Phase::FixedUpdate, &mut world);
    }
    let local = world.get::<Transform>(e).unwrap().translation;
    let global = world.get::<GlobalTransform>(e).unwrap().translation();
    assert!(local.y > 0.0);
    assert_eq!(global, local, "propagation sees physics' writes");
  }

  #[test]
  fn paused_physics_runs_no_step() {
    let (mut world, mut s) = world(60);
    let e = world
      .spawn((
        Transform::IDENTITY,
        RigidBody::new(),
        Collider::circle_collider(8.0),
      ))
      .id();
    world.resource_mut::<PhysicsPaused>().0 = true;
    for _ in 0..30 {
      s.run(Phase::FixedUpdate, &mut world);
    }
    assert_eq!(world.get::<Transform>(e).unwrap().translation.y, 0.0);
  }

  #[test]
  fn touching_bodies_write_collision_messages() {
    use crate::core::messages::CollisionStarted;
    let (mut world, mut s) = world(60);
    let floor = world
      .spawn((
        Transform::from_xy(0.0, 30.0),
        RigidBody::kinematic(),
        Collider::box_collider(200.0, 20.0),
      ))
      .id();
    let ball = world
      .spawn((
        Transform::from_xy(0.0, 0.0),
        RigidBody::new(),
        Collider::circle_collider(10.0),
      ))
      .id();
    for _ in 0..60 {
      s.run(Phase::FixedUpdate, &mut world);
    }
    let messages = world.resource::<crate::core::ecs::Messages<CollisionStarted>>();
    let started: Vec<_> = messages.get_cursor().read(messages).copied().collect();
    assert!(
      started
        .iter()
        .any(|m| m.involves(ball) && m.other(ball) == Some(floor)),
      "{started:?}"
    );
  }
}
