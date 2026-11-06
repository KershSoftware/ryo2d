//! Transform propagation: local `Transform`s down the hierarchy into
//! `GlobalTransform`s, walking only the subtrees that hold a change.

use super::kernel::{
  Added, Changed, ChildOf, Children, Commands, DetectChanges, DetectChangesMut, Entities, Entity,
  Has, Local, Mut, Or, Query, Ref, RemovedComponents, With, Without,
};
use super::transform::{
  GlobalTransform, Interpolated, ParentPlaced, PreviousGlobalTransform, Transform,
};

type RootItem<'a> = (
  Entity,
  Ref<'a, Transform>,
  &'a mut GlobalTransform,
  Option<&'a Children>,
  Has<Interpolated>,
  Has<PreviousGlobalTransform>,
);

/// Anything that can move an entity or change whether it is interpolated,
/// bar the entities only their parent's walk places ([`ParentPlaced`])
type Placement = (
  Or<(Changed<Transform>, Changed<ChildOf>, Added<Interpolated>)>,
  Without<ParentPlaced>,
);

type NodeItem<'a> = (
  Ref<'a, Transform>,
  Ref<'a, ChildOf>,
  &'a mut GlobalTransform,
  Option<&'a Children>,
  Has<Interpolated>,
  Has<PreviousGlobalTransform>,
);

/// (entity, parent's global, parent changed, parent interpolated): an entry
/// of the walk's stack
type Visit = (Entity, GlobalTransform, bool, bool);

/// A run walks every hierarchy from its roots, as it did before it walked
/// only the changed subtrees, once the changed entities are at least
/// 1 / `FULL_WALK_SHARE` of the entities it places (those with a
/// `Transform`), and more than `ALWAYS_SEEDED`; the scan for changes stops
/// there. The changed subtrees' walk costs the change filter's full scan
/// (about 0.7 ns an entity; the full walk's scan stops at the share) and,
/// for each changed entity, its walk up its ancestors by entity lookups (about
/// 25 ns for a leaf two deep) and its subtree; the full walk visits every
/// entity in about 5 ns. The two cost the same once about a fifth changed:
/// in `benches/foundation.rs`'s hierarchy of 10,000 entities, 2,000 leaves
/// moved cost 83 µs through the changed subtrees and about 81 through the
/// full walk (1,000: 47 against 68; 4,000: 154 against 108).
const FULL_WALK_SHARE: usize = 5;
/// Up to this many changed entities a run walks only the changed subtrees,
/// whatever their share: 65 changed leaves cost about 3 µs over the scan, so
/// the full walk saves a small world little, and small worlds (the tests
/// among them) keep to one path.
const ALWAYS_SEEDED: usize = 64;

/// Recompute `GlobalTransform` for every subtree whose root or any ancestor
/// changed, and keep `PreviousGlobalTransform` present on interpolated
/// subtrees (and absent elsewhere).
///
/// Roots are entities with a `Transform` and no parent. Children of a parent
/// that has no `Transform` of its own (a bare grouping entity) start from the
/// identity, as if they were roots.
///
/// The walk starts at the entities that changed, each from its parent's
/// stored `GlobalTransform`, so a subtree where nothing changed is not
/// visited. Finding them is a change-tick check per entity.
#[allow(clippy::too_many_arguments)] // one query or removal reader per input
pub fn propagate_transforms(
  mut roots: Query<RootItem, Without<ChildOf>>,
  mut nodes: Query<NodeItem, With<ChildOf>>,
  grouping: Query<(Entity, &Children), Without<Transform>>,
  changed: Query<Entity, Placement>,
  placed: Query<(), With<Transform>>,
  entities: &Entities,
  mut detached: RemovedComponents<ChildOf>,
  mut uninterpolated: RemovedComponents<Interpolated>,
  mut untransformed: RemovedComponents<Transform>,
  mut commands: Commands,
  // The walk's stack, kept between runs so a run allocates nothing once it
  // has grown to the hierarchy (empty at the end of each run)
  mut stack: Local<Vec<Visit>>,
  // The run's changed entities, and the entities that lost their parent,
  // their `Interpolated` or their `Transform` since the last run (each
  // sorted), kept between runs as the stack is
  mut seeds: Local<Vec<Entity>>,
  mut lost_parent: Local<Vec<Entity>>,
  mut lost_interpolated: Local<Vec<Entity>>,
  mut lost_transform: Local<Vec<Entity>>,
) {
  // The removal readers are drained every run so old removals don't count
  // again. An entity that lost its parent is a root placed by its own
  // Transform
  refill(&mut lost_parent, detached.read());
  refill(&mut lost_interpolated, uninterpolated.read());
  // A parent that lost its Transform now places its children from the identity
  refill(&mut lost_transform, untransformed.read());
  let changes = Changes {
    placement: &changed,
    detached: &lost_parent,
    uninterpolated: &lost_interpolated,
    untransformed: &lost_transform,
  };
  // Exact from the matched tables' lengths: no entity is visited
  let placed = placed.iter().len();
  let full_at = (placed / FULL_WALK_SHARE).max(ALWAYS_SEEDED + 1);
  let seeded = collect_seeds(&mut seeds, &changes, &grouping, full_at);
  // Nothing that places an entity changed since the last run, so every
  // `GlobalTransform` and `PreviousGlobalTransform` is already right
  if seeded && seeds.is_empty() {
    return;
  }

  stack.clear();
  if seeded {
    // A chain of more parents than the world has entities is a cycle
    let depth = entities.len() as usize;
    for &entity in seeds.iter() {
      if let Ok(root) = roots.get_mut(entity) {
        walk_root(root, changes.detached, &mut commands, &mut stack);
      } else if let Some(visit) = start_below(entity, depth, &nodes, &roots, &grouping, &changes) {
        stack.push(visit);
      }
    }
  } else {
    for root in roots.iter_mut() {
      walk_root(root, changes.detached, &mut commands, &mut stack);
    }
    for (group, children) in grouping.iter() {
      let changed = changes.untransformed.binary_search(&group).is_ok();
      for child in children.iter().copied() {
        stack.push((child, GlobalTransform::IDENTITY, changed, false));
      }
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

/// What changed since a run's last run: the `Placement` filter's entities,
/// and the removals, each list sorted
struct Changes<'a, 'w, 's> {
  placement: &'a Query<'w, 's, Entity, Placement>,
  detached: &'a [Entity],
  uninterpolated: &'a [Entity],
  untransformed: &'a [Entity],
}

impl Changes<'_, '_, '_> {
  /// Whether `entity` is one of the run's changed entities, which a walk
  /// starts from (a child of a parent that lost its `Transform` is one too;
  /// `start_below` checks for it)
  fn is_seed(&self, entity: Entity) -> bool {
    self.placement.contains(entity)
      || self.detached.binary_search(&entity).is_ok()
      || self.uninterpolated.binary_search(&entity).is_ok()
  }
}

/// Replace `list` with `entities`, sorted and without repeats
fn refill(list: &mut Vec<Entity>, entities: impl Iterator<Item = Entity>) {
  list.clear();
  list.extend(entities);
  if list.len() > 1 {
    list.sort_unstable();
    list.dedup();
  }
}

/// Collect the run's changed entities into `seeds`, sorted and without
/// repeats: those the `Placement` filter finds, those that lost their
/// parent or `Interpolated`, and the children of a parent that lost its
/// `Transform`. False once there are `full_at` of them, for a full walk.
fn collect_seeds(
  seeds: &mut Vec<Entity>,
  changes: &Changes,
  grouping: &Query<(Entity, &Children), Without<Transform>>,
  full_at: usize,
) -> bool {
  seeds.clear();
  if changes.detached.len() + changes.uninterpolated.len() >= full_at {
    return false;
  }
  seeds.extend_from_slice(changes.detached);
  seeds.extend_from_slice(changes.uninterpolated);
  for &group in changes.untransformed {
    if let Ok((_, children)) = grouping.get(group) {
      seeds.extend(children.iter().copied());
    }
    if seeds.len() >= full_at {
      return false;
    }
  }
  for entity in changes.placement.iter() {
    seeds.push(entity);
    if seeds.len() >= full_at {
      return false;
    }
  }
  seeds.sort_unstable();
  seeds.dedup();
  true
}

type RootMut<'a> = (
  Entity,
  Ref<'a, Transform>,
  Mut<'a, GlobalTransform>,
  Option<&'a Children>,
  bool,
  bool,
);

/// Place a root, which its own `Transform` places, and push its children
fn walk_root(
  (entity, transform, mut global, children, interpolated, has_previous): RootMut,
  detached: &[Entity],
  commands: &mut Commands,
  stack: &mut Vec<Visit>,
) {
  let changed = transform.is_changed() || detached.binary_search(&entity).is_ok();
  if changed {
    global.set_if_neq(GlobalTransform::from_transform(&transform));
  }
  sync_previous(commands, entity, interpolated, has_previous, *global);
  if let Some(children) = children {
    for child in children.iter().copied() {
      stack.push((child, *global, changed, interpolated));
    }
  }
}

/// Where the walk from a changed child starts: its parent's stored global,
/// whether a full walk would reach it with its parent changed, and whether
/// an ancestor makes it interpolated. `None` when an ancestor is a seed
/// too, whose walk reaches it, or when no walk reaches it: it is gone,
/// disabled or under a disabled entity, or in or under a cycle.
///
/// The ancestors that count stop at the first parent without a
/// `Transform`: what is below one starts from the identity and inherits no
/// interpolation.
fn start_below(
  entity: Entity,
  depth: usize,
  nodes: &Query<NodeItem, With<ChildOf>>,
  roots: &Query<RootItem, Without<ChildOf>>,
  grouping: &Query<(Entity, &Children), Without<Transform>>,
  changes: &Changes,
) -> Option<Visit> {
  let (_, child_of, ..) = nodes.get(entity).ok()?;
  let mut parent = child_of.parent();
  let mut parent_global = None;
  let mut interpolated = false;
  for _ in 0..depth {
    if let Ok((_, child_of, global, _, is_interpolated, _)) = nodes.get(parent) {
      if changes.is_seed(parent) {
        return None;
      }
      parent_global.get_or_insert(*global);
      interpolated |= is_interpolated;
      parent = child_of.parent();
    } else if let Ok((root, _, global, _, is_interpolated, _)) = roots.get(parent) {
      if changes.is_seed(root) {
        return None;
      }
      let parent_global = parent_global.unwrap_or(*global);
      return Some((
        entity,
        parent_global,
        false,
        interpolated || is_interpolated,
      ));
    } else if grouping.contains(parent) {
      // A group that has just lost its Transform: a full walk reaches its
      // children changed, and each of them is a seed
      let regrouped = changes.untransformed.binary_search(&parent).is_ok();
      return match parent_global {
        None => Some((entity, GlobalTransform::IDENTITY, regrouped, false)),
        Some(_) if regrouped => None,
        Some(parent_global) => Some((entity, parent_global, false, interpolated)),
      };
    } else {
      // Disabled: no walk goes below it
      return None;
    }
  }
  None
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
  use crate::core::ecs::kernel::{Disabled, Schedule, ScheduleLabel, World};
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
  fn a_parent_placed_child_s_own_transform_change_seeds_no_walk() {
    let mut world = World::new();
    let parent = world.spawn(Transform::from_xy(10.0, 0.0)).id();
    let child = world
      .spawn((Transform::from_xy(1.0, 0.0), ParentPlaced, ChildOf(parent)))
      .id();
    let mut s = schedule();
    s.run(&mut world);
    assert!(close(
      global(&world, child).translation(),
      Vec2::new(11.0, 0.0)
    ));
    world.get_mut::<Transform>(child).unwrap().translation = Vec2::new(5.0, 0.0);
    s.run(&mut world);
    assert!(close(
      global(&world, child).translation(),
      Vec2::new(11.0, 0.0)
    ));
  }

  #[test]
  fn a_parent_placed_child_moves_with_its_parent_the_frame_it_moves() {
    let mut world = World::new();
    let parent = world.spawn(Transform::from_xy(10.0, 0.0)).id();
    let child = world
      .spawn((Transform::from_xy(1.0, 0.0), ParentPlaced, ChildOf(parent)))
      .id();
    let mut s = schedule();
    s.run(&mut world);
    world.get_mut::<Transform>(parent).unwrap().translation = Vec2::new(20.0, 0.0);
    s.run(&mut world);
    assert!(close(
      global(&world, child).translation(),
      Vec2::new(21.0, 0.0)
    ));
  }

  #[test]
  fn removing_interpolated_strips_a_parent_placed_child_s_previous_transform() {
    let mut world = World::new();
    let parent = world
      .spawn((Transform::from_xy(10.0, 0.0), Interpolated))
      .id();
    let child = world
      .spawn((Transform::from_xy(1.0, 0.0), ParentPlaced, ChildOf(parent)))
      .id();
    assert!(
      world.get::<PreviousGlobalTransform>(child).is_some(),
      "given at its spawn"
    );
    let mut s = schedule();
    s.run(&mut world);
    assert!(world.get::<PreviousGlobalTransform>(child).is_some());
    world.entity_mut(parent).remove::<Interpolated>();
    s.run(&mut world);
    assert!(world.get::<PreviousGlobalTransform>(child).is_none());
  }

  #[test]
  fn a_parent_placed_child_under_a_grouping_entity_gets_no_previous_transform() {
    let mut world = World::new();
    let group = world.spawn(Interpolated).id();
    let child = world
      .spawn((Transform::from_xy(1.0, 0.0), ParentPlaced, ChildOf(group)))
      .id();
    assert!(world.get::<PreviousGlobalTransform>(child).is_none());
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

  fn previous(world: &World, e: Entity) -> Option<PreviousGlobalTransform> {
    world.get::<PreviousGlobalTransform>(e).copied()
  }

  /// Run once, then clear the trackers: the world is settled
  fn settled(world: &mut World) -> Schedule {
    let mut s = schedule();
    s.run(world);
    world.clear_trackers();
    s
  }

  #[test]
  fn moving_one_leaf_of_a_big_tree_moves_only_that_leaf() {
    let mut world = World::new();
    let mut all = Vec::new();
    let mut leaves = Vec::new();
    for r in 0..10 {
      let root = world
        .spawn(Transform::from_xy(r as f32 * 10.0, 0.0).with_rotation(0.3))
        .id();
      all.push(root);
      for c in 0..10 {
        let child = world
          .spawn((Transform::from_xy(1.0, c as f32), ChildOf(root)))
          .id();
        all.push(child);
        for g in 0..9 {
          let leaf = world
            .spawn((Transform::from_xy(0.0, g as f32 + 1.0), ChildOf(child)))
            .id();
          all.push(leaf);
          leaves.push(leaf);
        }
      }
    }
    let mut s = settled(&mut world);
    let before: Vec<GlobalTransform> = all.iter().map(|&e| global(&world, e)).collect();
    let moved = leaves[400];
    world.get_mut::<Transform>(moved).unwrap().translation.x += 5.0;
    s.run(&mut world);
    let parent = world.get::<ChildOf>(moved).unwrap().parent();
    let local = *world.get::<Transform>(moved).unwrap();
    assert_eq!(
      global(&world, moved),
      global(&world, parent).mul_transform(&local)
    );
    assert_ne!(
      global(&world, moved),
      before[all.iter().position(|&e| e == moved).unwrap()]
    );
    for (i, &e) in all.iter().enumerate() {
      if e == moved {
        continue;
      }
      assert_eq!(global(&world, e), before[i], "entity {i} moved");
      let rewritten = world.entity(e).get_ref::<GlobalTransform>().unwrap();
      assert!(!rewritten.is_changed(), "entity {i} was rewritten");
    }
  }

  #[test]
  fn a_child_changed_under_an_unchanged_parent_composes_with_the_parent_s_stored_global() {
    let mut world = World::new();
    let root = world
      .spawn(
        Transform::from_xy(100.0, 0.0)
          .with_rotation(FRAC_PI_2)
          .with_uniform_scale(2.0),
      )
      .id();
    let child = world
      .spawn((Transform::from_xy(1.0, 0.0), ChildOf(root)))
      .id();
    let mut s = settled(&mut world);
    world.get_mut::<Transform>(child).unwrap().translation = Vec2::new(10.0, 0.0);
    s.run(&mut world);
    assert!(
      close(global(&world, child).translation(), Vec2::new(100.0, 20.0)),
      "child at {:?}",
      global(&world, child).translation()
    );
  }

  #[test]
  fn a_parent_and_its_child_changed_together_compose_with_the_new_parent() {
    let mut world = World::new();
    let root = world.spawn(Transform::from_xy(1.0, 1.0)).id();
    let child = world
      .spawn((
        Transform::from_xy(3.0, 0.0).with_rotation(0.5),
        ChildOf(root),
      ))
      .id();
    let grandchild = world
      .spawn((Transform::from_xy(0.0, 1.0), ChildOf(child)))
      .id();
    let mut s = settled(&mut world);
    world.get_mut::<Transform>(root).unwrap().translation = Vec2::new(50.0, 0.0);
    world.get_mut::<Transform>(grandchild).unwrap().translation = Vec2::new(0.0, 7.0);
    s.run(&mut world);
    let expected = GlobalTransform::from_transform(world.get::<Transform>(root).unwrap())
      .mul_transform(world.get::<Transform>(child).unwrap())
      .mul_transform(world.get::<Transform>(grandchild).unwrap());
    assert_eq!(global(&world, grandchild), expected);
  }

  #[test]
  fn a_parent_and_its_child_changed_together_compose_with_the_new_parent_whichever_sorts_first() {
    // The test above with the grandchild spawned first. The changed
    // entities sort by their bits, which run against the index, so here the
    // root's walk starts first and above the grandchild's: both orders
    let mut world = World::new();
    let grandchild = world.spawn(Transform::from_xy(0.0, 1.0)).id();
    let root = world.spawn(Transform::from_xy(1.0, 1.0)).id();
    let child = world
      .spawn((
        Transform::from_xy(3.0, 0.0).with_rotation(0.5),
        ChildOf(root),
      ))
      .id();
    world.entity_mut(grandchild).insert(ChildOf(child));
    assert!(grandchild.index() < root.index());
    let mut s = settled(&mut world);
    world.get_mut::<Transform>(root).unwrap().translation = Vec2::new(50.0, 0.0);
    world.get_mut::<Transform>(grandchild).unwrap().translation = Vec2::new(0.0, 7.0);
    s.run(&mut world);
    let expected = GlobalTransform::from_transform(world.get::<Transform>(root).unwrap())
      .mul_transform(world.get::<Transform>(child).unwrap())
      .mul_transform(world.get::<Transform>(grandchild).unwrap());
    assert_eq!(global(&world, grandchild), expected);
  }

  #[test]
  fn a_change_below_a_disabled_parent_is_not_placed() {
    // No walk goes below a disabled entity, as before propagation walked
    // only the changed subtrees
    let mut world = World::new();
    let parent = world.spawn(Transform::from_xy(100.0, 0.0)).id();
    let child = world
      .spawn((Transform::from_xy(1.0, 0.0), ChildOf(parent)))
      .id();
    let mut s = settled(&mut world);
    let before = global(&world, child);
    world.entity_mut(parent).insert(Disabled);
    world.get_mut::<Transform>(child).unwrap().translation.x = 2.0;
    s.run(&mut world);
    assert_eq!(global(&world, child), before);
    let placed = world.entity(child).get_ref::<GlobalTransform>().unwrap();
    assert!(!placed.is_changed(), "the child was rewritten");
  }

  #[test]
  fn a_change_below_a_transform_less_parent_starts_from_the_identity() {
    let mut world = World::new();
    let root = world
      .spawn((Transform::from_xy(100.0, 0.0), Interpolated))
      .id();
    let group = world.spawn(ChildOf(root)).id();
    let child = world
      .spawn((Transform::from_xy(3.0, 4.0), ChildOf(group)))
      .id();
    let mut s = settled(&mut world);
    world.get_mut::<Transform>(child).unwrap().translation = Vec2::new(6.0, 8.0);
    s.run(&mut world);
    assert!(close(
      global(&world, child).translation(),
      Vec2::new(6.0, 8.0)
    ));
    assert!(previous(&world, child).is_none());
  }

  #[test]
  fn interpolated_added_mid_tree_marks_its_subtree_only() {
    let mut world = World::new();
    let root = world.spawn(Transform::from_xy(1.0, 0.0)).id();
    let a = world
      .spawn((Transform::from_xy(1.0, 0.0), ChildOf(root)))
      .id();
    let b = world.spawn((Transform::from_xy(1.0, 0.0), ChildOf(a))).id();
    let c = world.spawn((Transform::from_xy(1.0, 0.0), ChildOf(b))).id();
    let mut s = settled(&mut world);
    world.entity_mut(b).insert(Interpolated);
    s.run(&mut world);
    assert!(previous(&world, b).is_some_and(|p| p.skip));
    assert!(previous(&world, c).is_some_and(|p| p.skip));
    assert!(previous(&world, root).is_none());
    assert!(previous(&world, a).is_none());
  }

  #[test]
  fn removing_interpolated_keeps_previous_under_another_interpolated_ancestor() {
    let mut world = World::new();
    let root = world.spawn((Transform::IDENTITY, Interpolated)).id();
    let a = world
      .spawn((Transform::from_xy(1.0, 0.0), Interpolated, ChildOf(root)))
      .id();
    let b = world.spawn((Transform::from_xy(1.0, 0.0), ChildOf(a))).id();
    let mut s = settled(&mut world);
    world.entity_mut(a).remove::<Interpolated>();
    s.run(&mut world);
    world.clear_trackers();
    assert!(previous(&world, a).is_some(), "the root still covers a");
    assert!(previous(&world, b).is_some(), "the root still covers b");
    world.entity_mut(root).remove::<Interpolated>();
    s.run(&mut world);
    for e in [root, a, b] {
      assert!(previous(&world, e).is_none());
    }
  }

  #[test]
  fn reparenting_into_and_out_of_an_interpolated_subtree_syncs_previous() {
    let mut world = World::new();
    let p = world
      .spawn((Transform::from_xy(100.0, 0.0), Interpolated))
      .id();
    let q = world.spawn(Transform::from_xy(0.0, 50.0)).id();
    let x = world.spawn((Transform::from_xy(1.0, 0.0), ChildOf(q))).id();
    let y = world.spawn((Transform::from_xy(0.0, 1.0), ChildOf(x))).id();
    let mut s = settled(&mut world);
    assert!(previous(&world, x).is_none());
    world.entity_mut(x).insert(ChildOf(p));
    s.run(&mut world);
    world.clear_trackers();
    assert!(previous(&world, x).is_some());
    assert!(previous(&world, y).is_some());
    assert!(close(
      global(&world, x).translation(),
      Vec2::new(101.0, 0.0)
    ));
    assert!(close(
      global(&world, y).translation(),
      Vec2::new(101.0, 1.0)
    ));
    world.entity_mut(x).insert(ChildOf(q));
    s.run(&mut world);
    assert!(previous(&world, x).is_none());
    assert!(previous(&world, y).is_none());
    assert!(close(global(&world, y).translation(), Vec2::new(1.0, 51.0)));
  }

  #[test]
  fn a_child_spawned_under_an_unchanged_parent_is_placed_and_marked() {
    let mut world = World::new();
    let parent = world
      .spawn((Transform::from_xy(10.0, 10.0), Interpolated))
      .id();
    let mut s = settled(&mut world);
    let child = world
      .spawn((Transform::from_xy(1.0, 0.0), ChildOf(parent)))
      .id();
    s.run(&mut world);
    assert!(close(
      global(&world, child).translation(),
      Vec2::new(11.0, 10.0)
    ));
    assert!(previous(&world, child).is_some());
  }

  #[test]
  fn a_changed_entity_despawned_before_the_run_is_skipped() {
    let mut world = World::new();
    let root = world.spawn(Transform::from_xy(100.0, 0.0)).id();
    let child = world
      .spawn((Transform::from_xy(1.0, 0.0), ChildOf(root)))
      .id();
    let other = world.spawn(Transform::IDENTITY).id();
    let third = world
      .spawn((Transform::from_xy(5.0, 0.0), ChildOf(root)))
      .id();
    let mut s = settled(&mut world);
    world.get_mut::<Transform>(child).unwrap().translation.x = 2.0;
    world.despawn(child);
    world.get_mut::<Transform>(other).unwrap().translation = Vec2::new(30.0, 40.0);
    world.entity_mut(third).remove::<ChildOf>();
    s.run(&mut world);
    assert!(close(
      global(&world, other).translation(),
      Vec2::new(30.0, 40.0)
    ));
    assert!(close(
      global(&world, third).translation(),
      Vec2::new(5.0, 0.0)
    ));
  }

  #[test]
  fn a_hierarchy_cycle_does_not_hang_the_walk() {
    // A cycle is a misuse (nothing places it), but a change in or under one
    // must not stop the run
    let mut world = World::new();
    let a = world.spawn(Transform::IDENTITY).id();
    let b = world.spawn((Transform::IDENTITY, ChildOf(a))).id();
    let c = world.spawn((Transform::IDENTITY, ChildOf(b))).id();
    let other = world.spawn(Transform::IDENTITY).id();
    let mut s = settled(&mut world);
    world.entity_mut(a).insert(ChildOf(b));
    assert_eq!(world.get::<ChildOf>(a).map(ChildOf::parent), Some(b));
    s.run(&mut world);
    world.clear_trackers();
    // Nothing in the cycle is a seed now, so the walk up from c goes round
    // it until the depth bound stops it
    world.get_mut::<Transform>(c).unwrap().translation.x = 1.0;
    world.get_mut::<Transform>(other).unwrap().translation.x = 1.0;
    s.run(&mut world);
    assert!(close(
      global(&world, other).translation(),
      Vec2::new(1.0, 0.0)
    ));
  }

  #[test]
  fn more_changes_than_the_seeded_walk_takes_walk_every_hierarchy() {
    let mut world = World::new();
    // Past the floor, and half of the entities placed: a full walk
    let pairs: Vec<(Entity, Entity)> = (0..ALWAYS_SEEDED + 1)
      .map(|_| {
        let root = world.spawn(Transform::IDENTITY).id();
        let child = world
          .spawn((Transform::from_xy(1.0, 0.0), ChildOf(root)))
          .id();
        (root, child)
      })
      .collect();
    let parent = world.spawn(Transform::from_xy(100.0, 0.0)).id();
    let grouped = world
      .spawn((Transform::from_xy(10.0, 0.0), ChildOf(parent)))
      .id();
    let detached = world
      .spawn((Transform::from_xy(5.0, 0.0), ChildOf(parent)))
      .id();
    let mut s = settled(&mut world);
    for &(root, _) in &pairs {
      world.get_mut::<Transform>(root).unwrap().translation.y = 3.0;
    }
    world.entity_mut(parent).remove::<Transform>();
    world.entity_mut(detached).remove::<ChildOf>();
    s.run(&mut world);
    for &(_, child) in &pairs {
      assert!(close(
        global(&world, child).translation(),
        Vec2::new(1.0, 3.0)
      ));
    }
    assert!(close(
      global(&world, grouped).translation(),
      Vec2::new(10.0, 0.0)
    ));
    assert!(close(
      global(&world, detached).translation(),
      Vec2::new(5.0, 0.0)
    ));
  }

  /// Whether a run that moves `moved` of `count` roots takes the full walk:
  /// only the full walk visits an interpolated entity that did not change,
  /// and so puts back the previous transform taken from it
  fn takes_the_full_walk(count: usize, moved: usize) -> bool {
    let mut world = World::new();
    let roots: Vec<Entity> = (0..count - 1)
      .map(|_| world.spawn(Transform::IDENTITY).id())
      .collect();
    let still = world.spawn((Transform::IDENTITY, Interpolated)).id();
    let mut s = settled(&mut world);
    world.entity_mut(still).remove::<PreviousGlobalTransform>();
    for &root in &roots[..moved] {
      world.get_mut::<Transform>(root).unwrap().translation.x = 1.0;
    }
    s.run(&mut world);
    previous(&world, still).is_some()
  }

  #[test]
  fn a_fifth_of_the_entities_changed_takes_the_full_walk() {
    assert!(!takes_the_full_walk(1000, 199));
    assert!(takes_the_full_walk(1000, 200));
    // Up to 64 changed take the changed subtrees, whatever their share
    assert!(!takes_the_full_walk(100, ALWAYS_SEEDED));
    assert!(takes_the_full_walk(100, ALWAYS_SEEDED + 1));
  }

  /// A small xorshift, so the random test is the same every run
  struct Rng(u64);

  impl Rng {
    fn below(&mut self, n: usize) -> usize {
      self.0 ^= self.0 << 13;
      self.0 ^= self.0 >> 7;
      self.0 ^= self.0 << 17;
      (self.0 % n as u64) as usize
    }
  }

  /// An entity's global and whether it is interpolated, placed from scratch
  fn placed_from_scratch(world: &World, e: Entity) -> (GlobalTransform, bool) {
    let local = world.get::<Transform>(e).expect("a Transform");
    let own = world.get::<Interpolated>(e).is_some();
    match world.get::<ChildOf>(e).map(ChildOf::parent) {
      Some(parent) if world.get::<Transform>(parent).is_some() => {
        let (parent_global, parent_interpolated) = placed_from_scratch(world, parent);
        (
          parent_global.mul_transform(local),
          own || parent_interpolated,
        )
      }
      Some(_) => (GlobalTransform::IDENTITY.mul_transform(local), own),
      None => (GlobalTransform::from_transform(local), own),
    }
  }

  fn is_below(world: &World, e: Entity, ancestor: Entity) -> bool {
    let mut at = Some(e);
    while let Some(entity) = at {
      if entity == ancestor {
        return true;
      }
      at = world.get::<ChildOf>(entity).map(ChildOf::parent);
    }
    false
  }

  #[test]
  fn random_edits_leave_every_entity_where_a_walk_from_scratch_puts_it() {
    let mut world = World::new();
    let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
    let mut entities: Vec<Entity> = Vec::new();
    let point = |rng: &mut Rng| {
      Transform::from_xy(rng.below(200) as f32 - 100.0, rng.below(200) as f32 - 100.0)
        .with_rotation(rng.below(628) as f32 / 100.0)
    };
    for i in 0..150 {
      let parent = (i > 0 && rng.below(4) != 0).then(|| entities[rng.below(entities.len())]);
      let mut e = if rng.below(10) == 0 {
        world.spawn_empty()
      } else {
        world.spawn(point(&mut rng))
      };
      if let Some(parent) = parent {
        e.insert(ChildOf(parent));
      }
      if rng.below(6) == 0 {
        e.insert(Interpolated);
      }
      entities.push(e.id());
    }
    let mut s = settled(&mut world);
    for round in 0..300 {
      // Every tenth round moves more than the changed subtrees' walk takes
      if round % 10 == 9 {
        for mut transform in world.query::<&mut Transform>().iter_mut(&mut world) {
          if rng.below(3) != 0 {
            transform.translation.x += 1.0;
          }
        }
      }
      for _ in 0..1 + rng.below(6) {
        entities.retain(|&e| world.get_entity(e).is_ok());
        let e = entities[rng.below(entities.len())];
        let other = entities[rng.below(entities.len())];
        match rng.below(20) {
          0..=7 => {
            if let Some(mut transform) = world.get_mut::<Transform>(e) {
              *transform = point(&mut rng);
            }
          }
          8..=10 if !is_below(&world, other, e) => {
            world.entity_mut(e).insert(ChildOf(other));
          }
          11 => {
            world.entity_mut(e).remove::<ChildOf>();
          }
          12 | 13 => {
            if world.get::<Interpolated>(e).is_some() {
              world.entity_mut(e).remove::<Interpolated>();
            } else {
              world.entity_mut(e).insert(Interpolated);
            }
          }
          14 => {
            if world.get::<Transform>(e).is_some() {
              world.entity_mut(e).remove::<Transform>();
            } else {
              world.entity_mut(e).insert(point(&mut rng));
            }
          }
          15 if entities.len() > 40 => {
            world.despawn(e);
          }
          16..=18 => {
            let child = world.spawn((point(&mut rng), ChildOf(e))).id();
            entities.push(child);
          }
          _ => {}
        }
      }
      s.run(&mut world);
      world.clear_trackers();
      let placed: Vec<Entity> = world
        .query_filtered::<Entity, With<Transform>>()
        .iter(&world)
        .collect();
      for e in placed {
        let (expected, interpolated) = placed_from_scratch(&world, e);
        assert_eq!(
          global(&world, e),
          expected,
          "round {round}: {e:?} misplaced"
        );
        assert_eq!(
          previous(&world, e).is_some(),
          interpolated,
          "round {round}: {e:?}'s previous transform"
        );
      }
    }
  }
}
