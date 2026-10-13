//! Convenience methods on `World` for the hierarchy and interpolation.

use super::kernel::{
  Allow, Bundle, ChildOf, Disabled, Entity, IsResource, Observer, SystemIdMarker, Without, World,
};
use super::transform::{GlobalTransform, PreviousGlobalTransform};
use crate::core::graphics::MainCamera;

pub trait WorldExt {
  /// Spawn `bundle` as a child of `parent`.
  fn spawn_child(&mut self, parent: Entity, bundle: impl Bundle) -> Entity;
  /// Remove the entity from its parent, keeping its world position.
  fn detach_keep_world(&mut self, entity: Entity);
  /// Draw the entity at its new transform on the next frame instead of sliding there.
  fn skip_interpolation(&mut self, entity: Entity);
}

impl WorldExt for World {
  fn spawn_child(&mut self, parent: Entity, bundle: impl Bundle) -> Entity {
    self.spawn((bundle, ChildOf(parent))).id()
  }

  fn detach_keep_world(&mut self, entity: Entity) {
    let global = self.get::<GlobalTransform>(entity).copied();
    let Ok(mut e) = self.get_entity_mut(entity) else {
      return;
    };
    if e.get::<ChildOf>().is_none() {
      return;
    }
    e.remove::<ChildOf>();
    if let Some(global) = global {
      e.insert(global.to_transform());
    }
  }

  fn skip_interpolation(&mut self, entity: Entity) {
    if let Some(mut previous) = self.get_mut::<PreviousGlobalTransform>(entity) {
      previous.skip = true;
    }
  }
}

/// Despawn every game entity, disabled ones included. Resources, observers
/// and registered systems are entities too, and stay, as does the main
/// camera (a camera cleanup resets it instead).
pub(crate) fn despawn_game_entities(world: &mut World) {
  let entities: Vec<Entity> = world
    .query_filtered::<Entity, (
      Allow<Disabled>,
      Without<IsResource>,
      Without<Observer>,
      Without<SystemIdMarker>,
      Without<MainCamera>,
    )>()
    .iter(world)
    .collect();
  for entity in entities {
    // Children went with their parent already
    let _ = world.try_despawn(entity);
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::ecs::kernel::{Children, Schedule, ScheduleLabel};
  use crate::core::ecs::propagate::propagate_transforms;
  use crate::core::ecs::transform::{Interpolated, Transform};
  use glam::Vec2;
  use std::f32::consts::FRAC_PI_2;

  #[test]
  fn despawn_game_entities_keeps_resources_observers_and_systems() {
    use crate::core::ecs::kernel::{
      Allow, Disabled, Event, IsResource, Observer, On, Resource, Without,
    };

    #[derive(Resource)]
    struct Score(u32);
    #[derive(Event)]
    struct Ping;

    let mut world = World::new();
    world.insert_resource(Score(7));
    world.add_observer(|_: On<Ping>| {});
    let system = world.register_system(|| {});
    let parent = world.spawn(Transform::IDENTITY).id();
    world.spawn_child(parent, Transform::IDENTITY);
    world.spawn((Transform::IDENTITY, Disabled));

    despawn_game_entities(&mut world);

    let left = world
      .query_filtered::<Entity, (Allow<Disabled>, Without<IsResource>, Without<Observer>)>()
      .iter(&world)
      .count();
    assert_eq!(left, 1, "only the registered system's entity is left");
    assert_eq!(world.resource::<Score>().0, 7);
    assert_eq!(world.query::<&Observer>().iter(&world).count(), 1);
    assert!(world.run_system(system).is_ok());
  }

  #[derive(ScheduleLabel, Debug, Clone, Copy, PartialEq, Eq, Hash)]
  struct Step;

  fn propagate(world: &mut World) {
    let mut s = Schedule::new(Step);
    s.add_systems(propagate_transforms);
    s.run(world);
  }

  #[test]
  fn spawn_child_sets_child_of() {
    let mut world = World::new();
    let p = world.spawn(Transform::IDENTITY).id();
    let c = world.spawn_child(p, Transform::from_xy(1.0, 1.0));
    assert_eq!(world.get::<ChildOf>(c).map(|c| c.parent()), Some(p));
    assert!(world.get::<Children>(p).is_some_and(|ch| ch.contains(&c)));
  }

  #[test]
  fn detach_keep_world_leaves_the_world_position_unchanged() {
    let mut world = World::new();
    let p = world
      .spawn(
        Transform::from_xy(100.0, 0.0)
          .with_rotation(FRAC_PI_2)
          .with_uniform_scale(2.0),
      )
      .id();
    let c = world.spawn_child(p, Transform::from_xy(5.0, 0.0));
    propagate(&mut world);
    let before = world.get::<GlobalTransform>(c).unwrap().translation();
    world.detach_keep_world(c);
    propagate(&mut world);
    assert!(world.get::<ChildOf>(c).is_none());
    let after = world.get::<GlobalTransform>(c).unwrap().translation();
    assert!((before - after).length() < 1e-3, "{before} vs {after}");
    assert!((before - Vec2::new(100.0, 10.0)).length() < 1e-3);
  }

  #[test]
  fn skip_interpolation_sets_the_flag() {
    let mut world = World::new();
    let e = world.spawn((Transform::IDENTITY, Interpolated)).id();
    propagate(&mut world);
    world.get_mut::<PreviousGlobalTransform>(e).unwrap().skip = false;
    world.skip_interpolation(e);
    assert!(world.get::<PreviousGlobalTransform>(e).unwrap().skip);
    // No-op on an entity without the component
    let plain = world.spawn(Transform::IDENTITY).id();
    world.skip_interpolation(plain);
  }
}
