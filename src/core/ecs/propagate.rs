//! Transform propagation: local `Transform`s down the hierarchy into
//! `GlobalTransform`s, recomputing only subtrees that changed.

use super::kernel::{
  Added, Changed, ChildOf, Children, Commands, DetectChanges, DetectChangesMut, Entity, Has, Or,
  Query, Ref, RemovedComponents, With, Without,
};
use super::transform::{GlobalTransform, Interpolated, PreviousGlobalTransform, Transform};

type RootItem<'a> = (
  Entity,
  Ref<'a, Transform>,
  &'a mut GlobalTransform,
  Option<&'a Children>,
  Has<Interpolated>,
  Has<PreviousGlobalTransform>,
);

/// Anything that can move an entity or change whether it is interpolated
type Placement = Or<(Changed<Transform>, Changed<ChildOf>, Added<Interpolated>)>;

type NodeItem<'a> = (
  Ref<'a, Transform>,
  Ref<'a, ChildOf>,
  &'a mut GlobalTransform,
  Option<&'a Children>,
  Has<Interpolated>,
  Has<PreviousGlobalTransform>,
);

/// Recompute `GlobalTransform` for every subtree whose root or any ancestor
/// changed, and keep `PreviousGlobalTransform` present on interpolated
/// subtrees (and absent elsewhere).
///
/// Roots are entities with a `Transform` and no parent. Children of a parent
/// that has no `Transform` of its own (a bare grouping entity) start from the
/// identity, as if they were roots.
#[allow(clippy::too_many_arguments)] // one query or removal reader per input
pub fn propagate_transforms(
  mut roots: Query<RootItem, Without<ChildOf>>,
  mut nodes: Query<NodeItem, With<ChildOf>>,
  grouping: Query<(Entity, &Children), Without<Transform>>,
  changed: Query<(), Placement>,
  mut detached: RemovedComponents<ChildOf>,
  mut uninterpolated: RemovedComponents<Interpolated>,
  mut untransformed: RemovedComponents<Transform>,
  mut commands: Commands,
) {
  // Nothing that places an entity changed since the last run, so every
  // `GlobalTransform` and `PreviousGlobalTransform` is already right. The
  // removal readers are drained either way so old removals don't count again.
  // An entity that lost its parent is a root placed by its own Transform
  let detached: Vec<Entity> = detached.read().collect();
  // A parent that lost its Transform now places its children from the identity
  let untransformed: Vec<Entity> = untransformed.read().collect();
  let removed = detached.len() + uninterpolated.read().count() + untransformed.len();
  if removed == 0 && changed.is_empty() {
    return;
  }

  // (entity, parent's global, parent changed, parent interpolated)
  let mut stack: Vec<(Entity, GlobalTransform, bool, bool)> = Vec::new();

  for (entity, transform, mut global, children, interpolated, has_previous) in roots.iter_mut() {
    let changed = transform.is_changed() || detached.contains(&entity);
    if changed {
      global.set_if_neq(GlobalTransform::from_transform(&transform));
    }
    sync_previous(&mut commands, entity, interpolated, has_previous, *global);
    if let Some(children) = children {
      for child in children.iter().copied() {
        stack.push((child, *global, changed, interpolated));
      }
    }
  }

  for (group, children) in grouping.iter() {
    let changed = untransformed.contains(&group);
    for child in children.iter().copied() {
      stack.push((child, GlobalTransform::IDENTITY, changed, false));
    }
  }

  while let Some((entity, parent_global, parent_changed, parent_interpolated)) = stack.pop() {
    let Ok((transform, child_of, mut global, children, interpolated, has_previous)) =
      nodes.get_mut(entity)
    else {
      // A child without a Transform: nothing to place, and its own children
      // (if any) are reached through the grouping query.
      continue;
    };
    let changed = parent_changed || transform.is_changed() || child_of.is_changed();
    if changed {
      global.set_if_neq(parent_global.mul_transform(&transform));
    }
    let interpolated = interpolated || parent_interpolated;
    sync_previous(&mut commands, entity, interpolated, has_previous, *global);
    if let Some(children) = children {
      for child in children.iter().copied() {
        stack.push((child, *global, changed, interpolated));
      }
    }
  }
}

/// Insert or remove `PreviousGlobalTransform` so that it exists exactly on
/// interpolated entities. A fresh one skips, so the first frame draws current.
fn sync_previous(
  commands: &mut Commands,
  entity: Entity,
  interpolated: bool,
  has_previous: bool,
  global: GlobalTransform,
) {
  match (interpolated, has_previous) {
    (true, false) => {
      commands
        .entity(entity)
        .insert(PreviousGlobalTransform { global, skip: true });
    }
    (false, true) => {
      commands.entity(entity).remove::<PreviousGlobalTransform>();
    }
    _ => {}
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::ecs::kernel::{Schedule, ScheduleLabel, World};
  use glam::Vec2;
  use std::f32::consts::FRAC_PI_2;

  #[derive(ScheduleLabel, Debug, Clone, Copy, PartialEq, Eq, Hash)]
  struct Step;

  fn schedule() -> Schedule {
    let mut s = Schedule::new(Step);
    s.add_systems(propagate_transforms);
    s
  }

  fn global(world: &World, e: Entity) -> GlobalTransform {
    *world.get::<GlobalTransform>(e).expect("GlobalTransform")
  }

  fn close(a: Vec2, b: Vec2) -> bool {
    (a - b).length() < 1e-4
  }

  #[test]
  fn a_spawned_entity_is_placed_before_propagation() {
    let mut world = World::new();
    let parent = world
      .spawn(Transform::from_xy(100.0, 100.0).with_rotation(FRAC_PI_2))
      .id();
    let child = world
      .spawn((Transform::from_xy(10.0, 0.0), ChildOf(parent)))
      .id();
    assert!(close(
      global(&world, parent).translation(),
      Vec2::new(100.0, 100.0)
    ));
    assert!(close(
      global(&world, child).translation(),
      Vec2::new(100.0, 110.0)
    ));
  }

  #[test]
  fn inserting_a_transform_places_the_entity_at_once() {
    let mut world = World::new();
    let e = world.spawn(Transform::from_xy(1.0, 2.0)).id();
    world.entity_mut(e).insert(Transform::from_xy(30.0, 40.0));
    assert!(close(
      global(&world, e).translation(),
      Vec2::new(30.0, 40.0)
    ));
  }

  #[test]
  fn removing_child_of_makes_the_entity_a_root_at_its_local_transform() {
    let mut world = World::new();
    let parent = world.spawn(Transform::from_xy(100.0, 0.0)).id();
    let child = world
      .spawn((Transform::from_xy(10.0, 0.0), ChildOf(parent)))
      .id();
    let mut s = schedule();
    s.run(&mut world);
    assert!(close(
      global(&world, child).translation(),
      Vec2::new(110.0, 0.0)
    ));
    world.entity_mut(child).remove::<ChildOf>();
    s.run(&mut world);
    assert!(
      close(global(&world, child).translation(), Vec2::new(10.0, 0.0)),
      "detached child at {:?}",
      global(&world, child).translation()
    );
  }

  #[test]
  fn a_still_world_is_not_rewritten() {
    let mut world = World::new();
    let parent = world.spawn(Transform::from_xy(1.0, 0.0)).id();
    world.spawn((Transform::from_xy(1.0, 0.0), ChildOf(parent)));
    let mut s = schedule();
    s.run(&mut world);
    world.clear_trackers();
    s.run(&mut world);
    let mut rewritten = world.query_filtered::<(), Changed<GlobalTransform>>();
    assert_eq!(rewritten.iter(&world).count(), 0);
  }

  #[test]
  fn a_child_of_a_grouping_entity_starts_from_the_identity() {
    let mut world = World::new();
    let group = world.spawn_empty().id();
    let child = world
      .spawn((Transform::from_xy(10.0, 5.0), ChildOf(group)))
      .id();
    schedule().run(&mut world);
    assert!(close(
      global(&world, child).translation(),
      Vec2::new(10.0, 5.0)
    ));
  }

  #[test]
  fn a_grouped_child_is_left_alone_when_something_else_moved() {
    let mut world = World::new();
    let group = world.spawn_empty().id();
    let child = world.spawn((Transform::IDENTITY, ChildOf(group))).id();
    let other = world.spawn(Transform::IDENTITY).id();
    let mut s = schedule();
    s.run(&mut world);
    // A value propagation would correct if it recomputed the child
    let planted = GlobalTransform::from_transform(&Transform::from_xy(7.0, 7.0));
    *world
      .get_mut::<GlobalTransform>(child)
      .unwrap()
      .bypass_change_detection() = planted;
    world.clear_trackers();
    world.get_mut::<Transform>(other).unwrap().translation.x = 1.0;
    s.run(&mut world);
    assert_eq!(global(&world, child), planted);
  }

  #[test]
  fn children_start_from_the_identity_once_their_parent_loses_its_transform() {
    let mut world = World::new();
    let parent = world.spawn(Transform::from_xy(100.0, 0.0)).id();
    let child = world
      .spawn((Transform::from_xy(10.0, 0.0), ChildOf(parent)))
      .id();
    let mut s = schedule();
    s.run(&mut world);
    world
      .entity_mut(parent)
      .remove::<(Transform, GlobalTransform)>();
    s.run(&mut world);
    assert!(close(
      global(&world, child).translation(),
      Vec2::new(10.0, 0.0)
    ));
  }

  #[test]
  fn child_of_rotated_parent() {
    let mut world = World::new();
    let parent = world
      .spawn(Transform::from_xy(100.0, 100.0).with_rotation(FRAC_PI_2))
      .id();
    let child = world
      .spawn((Transform::from_xy(10.0, 0.0), ChildOf(parent)))
      .id();
    schedule().run(&mut world);
    assert!(close(
      global(&world, child).translation(),
      Vec2::new(100.0, 110.0)
    ));
  }

  #[test]
  fn three_levels_compose() {
    let mut world = World::new();
    let a = world.spawn(Transform::from_xy(1.0, 0.0)).id();
    let b = world.spawn((Transform::from_xy(1.0, 0.0), ChildOf(a))).id();
    let c = world.spawn((Transform::from_xy(1.0, 0.0), ChildOf(b))).id();
    schedule().run(&mut world);
    assert!(close(global(&world, c).translation(), Vec2::new(3.0, 0.0)));
  }

  #[test]
  fn unchanged_subtrees_are_not_rewritten() {
    let mut world = World::new();
    let a = world.spawn(Transform::from_xy(1.0, 0.0)).id();
    let b = world.spawn((Transform::from_xy(1.0, 0.0), ChildOf(a))).id();
    let mut s = schedule();
    s.run(&mut world);
    world.clear_trackers();
    s.run(&mut world);
    let b_ref = world.entity(b).get_ref::<GlobalTransform>().unwrap();
    assert!(!b_ref.is_changed(), "nothing moved, so nothing was written");
    world.get_mut::<Transform>(a).unwrap().translation.x = 5.0;
    s.run(&mut world);
    assert!(close(global(&world, b).translation(), Vec2::new(6.0, 0.0)));
  }

  #[test]
  fn reparenting_recomputes() {
    let mut world = World::new();
    let a = world.spawn(Transform::from_xy(0.0, 0.0)).id();
    let b = world.spawn(Transform::from_xy(50.0, 0.0)).id();
    let c = world.spawn((Transform::from_xy(1.0, 0.0), ChildOf(a))).id();
    let mut s = schedule();
    s.run(&mut world);
    world.entity_mut(c).insert(ChildOf(b));
    s.run(&mut world);
    assert!(close(global(&world, c).translation(), Vec2::new(51.0, 0.0)));
  }

  #[test]
  fn child_of_a_transform_less_parent_treats_it_as_identity() {
    let mut world = World::new();
    let group = world.spawn_empty().id();
    let child = world
      .spawn((Transform::from_xy(7.0, 8.0), ChildOf(group)))
      .id();
    schedule().run(&mut world);
    assert!(close(
      global(&world, child).translation(),
      Vec2::new(7.0, 8.0)
    ));
  }

  #[test]
  fn interpolated_ancestors_mark_descendants() {
    let mut world = World::new();
    let ship = world.spawn((Transform::IDENTITY, Interpolated)).id();
    let turret = world
      .spawn((Transform::from_xy(1.0, 0.0), ChildOf(ship)))
      .id();
    let plain = world.spawn(Transform::IDENTITY).id();
    schedule().run(&mut world);
    assert!(world.get::<PreviousGlobalTransform>(ship).is_some());
    assert!(world.get::<PreviousGlobalTransform>(turret).is_some());
    assert!(world.get::<PreviousGlobalTransform>(plain).is_none());
    assert!(
      world.get::<PreviousGlobalTransform>(turret).unwrap().skip,
      "a fresh snapshot skips so the first frame does not lerp from identity"
    );
    world.entity_mut(ship).remove::<Interpolated>();
    schedule().run(&mut world);
    assert!(world.get::<PreviousGlobalTransform>(turret).is_none());
  }

  #[test]
  fn despawning_a_parent_removes_descendants() {
    let mut world = World::new();
    let a = world.spawn(Transform::IDENTITY).id();
    let b = world.spawn((Transform::IDENTITY, ChildOf(a))).id();
    world.despawn(a);
    assert!(world.get_entity(b).is_err());
  }
}
