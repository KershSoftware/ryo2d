//! Convenience methods on `World` for the hierarchy and interpolation.

use super::kernel::{
  Allow, Bundle, ChildOf, Disabled, Entity, IsResource, Observer, SystemIdMarker, Without, World,
};
use super::transform::{GlobalTransform, PreviousGlobalTransform};
use crate::core::graphics::MainCamera;
use crate::core::particles::ParticleEmitter;
use crate::core::skeleton::{Skeleton, skip_sockets};

pub trait WorldExt {
  /// Spawn `bundle` as a child of `parent`.
  fn spawn_child(&mut self, parent: Entity, bundle: impl Bundle) -> Entity;
  /// Remove the entity from its parent, keeping its world position.
  fn detach_keep_world(&mut self, entity: Entity);
  /// Draw the entity at its new transform on the next frame instead of sliding there.
  /// A particle emitter on it spawns only at its new place in its next step,
  /// leaving no streak along the jump, and a skeleton on it takes its bone
  /// sockets, and what they hold, with it.
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
    // Interpolated or not, and called inside a step or between steps (the
    // step's snapshot clears `skip` before particles run)
    if let Some(mut emitter) = self.get_mut::<ParticleEmitter>(entity) {
      emitter.teleported = true;
    }
    // A skeleton's sockets ride its bones, so they jump with it
    if self.get::<Skeleton>(entity).is_some() {
      skip_sockets(self, entity);
    }
  }
}

/// Despawn every game entity, disabled ones included. Resources, observers
/// and registered systems are entities too, and stay, as does the main
/// camera (a camera cleanup resets it instead). Each hierarchy goes from its
/// top, so a component's hooks see the entity's children as it goes (a
/// streamed map saves its changed chunks from its layers).
pub(crate) fn despawn_game_entities(world: &mut World) {
  let mut entities: Vec<(usize, Entity)> = world
    .query_filtered::<Entity, (
      Allow<Disabled>,
      Without<IsResource>,
      Without<Observer>,
      Without<SystemIdMarker>,
      Without<MainCamera>,
    )>()
    .iter(world)
    .map(|entity| (depth(world, entity), entity))
    .collect();
  entities.sort_by_key(|&(depth, _)| depth);
  for (_, entity) in entities {
    // Children went with their parent already
    let _ = world.try_despawn(entity);
  }
}

/// How many ancestors `entity` has (a cycle, which the hierarchy forbids,
/// counts as deep as the world's entities)
fn depth(world: &World, entity: Entity) -> usize {
  let limit = world.entities().len() as usize;
  let mut at = entity;
  for depth in 0..limit {
    match world.get::<ChildOf>(at) {
      Some(child_of) => at = child_of.parent(),
      None => return depth,
    }
  }
  limit
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

  #[test]
  fn despawn_game_entities_despawns_each_hierarchy_from_its_top() {
    use crate::core::ecs::kernel::{Component, DeferredWorld, HookContext, Resource};

    /// What the hook of a parent's `Watch` saw: whether the child it
    /// watches was still there as the parent went
    #[derive(Resource, Default)]
    struct Seen(Vec<bool>);
    #[derive(Component)]
    #[component(on_discard = watched)]
    struct Watch(Entity);
    fn watched(mut world: DeferredWorld, context: HookContext) {
      let child = world.get::<Watch>(context.entity).map(|w| w.0);
      let there = child.is_some_and(|child| world.get_entity(child).is_ok());
      world.resource_mut::<Seen>().0.push(there);
    }

    let mut world = World::new();
    world.init_resource::<Seen>();
    // Children listed before their parents: a child's archetype made first
    let elder = world.spawn(Transform::IDENTITY).id();
    world.spawn_child(elder, Transform::IDENTITY);
    let mut parents = Vec::new();
    for _ in 0..3 {
      let parent = world.spawn(Transform::IDENTITY).id();
      let child = world.spawn_child(parent, Transform::IDENTITY);
      // Grandchildren too, the middle one watching its child
      let grandchild = world.spawn_child(child, Transform::IDENTITY);
      world.entity_mut(child).insert(Watch(grandchild));
      world.entity_mut(parent).insert(Watch(child));
      parents.push(parent);
    }

    despawn_game_entities(&mut world);

    assert_eq!(world.resource::<Seen>().0, [true; 6], "each saw its child");
    assert!(parents.iter().all(|&p| world.get_entity(p).is_err()));
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
  fn detach_keep_world_keeps_a_mirrored_entity_mirrored() {
    // A child of a turned parent mirrored left to right: detached, it is
    // where it was and still mirrored. (It has no turn of its own: under an
    // uneven scale that would skew it, which no `Transform` holds)
    let mut world = World::new();
    let p = world
      .spawn(
        Transform::from_xy(100.0, 0.0)
          .with_rotation(0.4)
          .with_scale(Vec2::new(-2.0, 1.0)),
      )
      .id();
    let c = world.spawn_child(p, Transform::from_xy(5.0, 3.0));
    propagate(&mut world);
    let before = world.get::<GlobalTransform>(c).unwrap().affine();
    assert!(before.matrix2.determinant() < 0.0);
    world.detach_keep_world(c);
    propagate(&mut world);
    let after = world.get::<GlobalTransform>(c).unwrap().affine();
    assert!(
      after.abs_diff_eq(before, 1e-4),
      "{after:?} is not {before:?}"
    );
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

  #[test]
  fn skip_interpolation_marks_a_particle_emitter_interpolated_or_not() {
    use crate::core::asset::Assets;
    use crate::core::graphics::Image;
    use crate::core::particles::{ParticleEffect, ParticleEmitter};

    let image = Assets::<Image>::new().add(Image::solid_rect(8, 8, 255, 255, 255, 255));
    let effect = Assets::<ParticleEffect>::new().add(ParticleEffect::new(image));
    let mut world = World::new();
    let plain = world.spawn(ParticleEmitter::new(effect.clone())).id();
    let smooth = world
      .spawn((ParticleEmitter::new(effect), Interpolated))
      .id();
    propagate(&mut world);
    for e in [plain, smooth] {
      assert!(!world.get::<ParticleEmitter>(e).unwrap().teleported);
      world.skip_interpolation(e);
      assert!(world.get::<ParticleEmitter>(e).unwrap().teleported);
    }
  }
}
