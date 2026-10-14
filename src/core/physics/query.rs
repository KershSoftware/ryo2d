//! The public side of `Physics`: forces and impulses and body reads (scene
//! queries and contact state join them later).

use glam::Mat2;
use rapier2d::parry::query::{QueryDispatcher, ShapeCastOptions};
use rapier2d::prelude::{
  Collider as RapierCollider, ColliderHandle, QueryFilter as RapierFilter, QueryPipeline, Ray,
};

use super::collider::ColliderShape;
use super::convert::{from_rapier, pose, to_rapier};
use super::shape;
use super::world::{Owner, Physics, Push, first_contact};
use crate::core::ecs::{Entity, Vec2};

/// What a query may hit. The default hits every layer and skips triggers
/// (so a line of sight is not blocked by a trigger zone).
#[derive(Debug, Clone, PartialEq)]
pub struct QueryFilter {
  mask: u32,
  excluded: Vec<Entity>,
  triggers: bool,
}

impl Default for QueryFilter {
  fn default() -> Self {
    Self {
      mask: u32::MAX,
      excluded: Vec::new(),
      triggers: false,
    }
  }
}

impl QueryFilter {
  pub fn new() -> Self {
    Self::default()
  }

  /// Hit only colliders whose layer meets `mask` (whatever their own mask).
  pub fn with_mask(mut self, mask: u32) -> Self {
    self.mask = mask;
    self
  }

  /// Skip a body (all its colliders) or one collider. Call it again to skip more.
  pub fn excluding(mut self, entity: Entity) -> Self {
    self.excluded.push(entity);
    self
  }

  /// Hit triggers too.
  pub fn with_triggers(mut self) -> Self {
    self.triggers = true;
    self
  }
}

/// A ray or shape cast's first contact: the world point, the surface normal
/// there and how far along the cast it is. A ray that starts inside a
/// collider hits it at distance 0 with a zero normal.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Hit {
  pub collider: Entity,
  pub body: Entity,
  pub point: Vec2,
  pub normal: Vec2,
  pub distance: f32,
}

/// A collider found by an overlap test.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Overlap {
  pub collider: Entity,
  pub body: Entity,
}

/// The closest point on a collider's surface, and whether the query point
/// was inside it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PointHit {
  pub collider: Entity,
  pub body: Entity,
  pub point: Vec2,
  pub inside: bool,
}

/// A normalised ray, or `None` for a zero or non-finite one or a negative or NaN range.
fn ray(origin: Vec2, direction: Vec2, max_distance: f32) -> Option<Ray> {
  let direction = direction.normalize_or_zero();
  (origin.is_finite() && direction != Vec2::ZERO && max_distance >= 0.0)
    .then(|| Ray::new(to_rapier(origin), to_rapier(direction)))
}

impl Physics {
  fn owner_of(&self, handle: ColliderHandle) -> Option<Owner> {
    self.owners.get(&handle).copied()
  }

  /// Run `f` with Rapier's queries, filtered by `filter`.
  fn with_queries<R>(&self, filter: &QueryFilter, f: impl FnOnce(QueryPipeline<'_>) -> R) -> R {
    let passes = |handle: ColliderHandle, collider: &RapierCollider| {
      u32::from(collider.collision_groups().memberships) & filter.mask != 0
        && self.owners.get(&handle).is_some_and(|owner| {
          !filter.excluded.contains(&owner.collider) && !filter.excluded.contains(&owner.body)
        })
    };
    let mut rapier_filter = RapierFilter::default().predicate(&passes);
    if !filter.triggers {
      rapier_filter = rapier_filter.exclude_sensors();
    }
    let dispatcher: &dyn QueryDispatcher = self.narrow_phase.query_dispatcher();
    f(self
      .broad_phase
      .as_query_pipeline(dispatcher, &self.bodies, &self.colliders, rapier_filter))
  }

  /// The first collider along a ray from `origin` (the direction need not be
  /// normalised), within `max_distance` px. A ray starting inside a collider
  /// hits it at distance 0 with a zero normal. Queries see the world as the
  /// last step left it.
  pub fn raycast(
    &self,
    origin: Vec2,
    direction: Vec2,
    max_distance: f32,
    filter: &QueryFilter,
  ) -> Option<Hit> {
    let ray = ray(origin, direction, max_distance)?;
    self.with_queries(filter, |queries| {
      let (handle, hit) = queries.cast_ray_and_get_normal(&ray, max_distance, true)?;
      let owner = self.owner_of(handle)?;
      Some(Hit {
        collider: owner.collider,
        body: owner.body,
        point: from_rapier(ray.point_at(hit.time_of_impact)),
        normal: from_rapier(hit.normal),
        distance: hit.time_of_impact,
      })
    })
  }

  /// Every collider along the ray, nearest first.
  pub fn raycast_all(
    &self,
    origin: Vec2,
    direction: Vec2,
    max_distance: f32,
    filter: &QueryFilter,
  ) -> Vec<Hit> {
    let Some(ray) = ray(origin, direction, max_distance) else {
      return Vec::new();
    };
    let mut hits: Vec<Hit> = self.with_queries(filter, |queries| {
      queries
        .intersect_ray(ray, max_distance, true)
        .filter_map(|(handle, _, hit)| {
          let owner = self.owner_of(handle)?;
          Some(Hit {
            collider: owner.collider,
            body: owner.body,
            point: from_rapier(ray.point_at(hit.time_of_impact)),
            normal: from_rapier(hit.normal),
            distance: hit.time_of_impact,
          })
        })
        .collect()
    });
    hits.sort_by(|a, b| a.distance.total_cmp(&b.distance));
    hits
  }

  /// Sweep `shape` (at `origin`, turned by `rotation`) along `direction`:
  /// the first collider it would touch. A shape that starts overlapping
  /// something hits it at distance 0.
  pub fn shape_cast(
    &self,
    shape: &ColliderShape,
    origin: Vec2,
    rotation: f32,
    direction: Vec2,
    max_distance: f32,
    filter: &QueryFilter,
  ) -> Option<Hit> {
    let direction = direction.normalize_or_zero();
    if direction == Vec2::ZERO
      || !origin.is_finite()
      || !rotation.is_finite()
      || (max_distance.is_nan() || max_distance < 0.0)
    {
      return None;
    }
    let shape = shape::build(shape, Mat2::IDENTITY)?;
    let options = ShapeCastOptions {
      stop_at_penetration: true,
      compute_impact_geometry_on_penetration: true,
      ..ShapeCastOptions::with_max_time_of_impact(max_distance)
    };
    self.with_queries(filter, |queries| {
      let (handle, hit) = queries.cast_shape(
        &pose(origin, rotation),
        to_rapier(direction),
        &*shape,
        options,
      )?;
      let owner = self.owner_of(handle)?;
      Some(Hit {
        collider: owner.collider,
        body: owner.body,
        point: from_rapier(hit.witness1),
        normal: from_rapier(hit.normal1),
        distance: hit.time_of_impact,
      })
    })
  }

  /// Every collider containing `point`.
  pub fn overlap_point(&self, point: Vec2, filter: &QueryFilter) -> Vec<Overlap> {
    if !point.is_finite() {
      return Vec::new();
    }
    self.with_queries(filter, |queries| {
      queries
        .intersect_point(to_rapier(point))
        .filter_map(|(handle, _)| self.owner_of(handle))
        .map(|owner| Overlap {
          collider: owner.collider,
          body: owner.body,
        })
        .collect()
    })
  }

  /// Every collider overlapping `shape` at `position`, turned by `rotation`.
  pub fn overlap_shape(
    &self,
    shape: &ColliderShape,
    position: Vec2,
    rotation: f32,
    filter: &QueryFilter,
  ) -> Vec<Overlap> {
    if !position.is_finite() || !rotation.is_finite() {
      return Vec::new();
    }
    let Some(shape) = shape::build(shape, Mat2::IDENTITY) else {
      return Vec::new();
    };
    self.with_queries(filter, |queries| {
      queries
        .intersect_shape(pose(position, rotation), &*shape)
        .filter_map(|(handle, _)| self.owner_of(handle))
        .map(|owner| Overlap {
          collider: owner.collider,
          body: owner.body,
        })
        .collect()
    })
  }

  /// The closest collider surface point to `point` within `max_distance`.
  pub fn nearest(&self, point: Vec2, max_distance: f32, filter: &QueryFilter) -> Option<PointHit> {
    if !point.is_finite() || (max_distance.is_nan() || max_distance < 0.0) {
      return None;
    }
    self.with_queries(filter, |queries| {
      let (handle, projection) = queries.project_point(to_rapier(point), max_distance, false)?;
      let owner = self.owner_of(handle)?;
      Some(PointHit {
        collider: owner.collider,
        body: owner.body,
        point: from_rapier(projection.point),
        inside: projection.is_inside,
      })
    })
  }

  /// An instant change of momentum (mass times px/s), applied before the
  /// next step. Ignored for bodies that are not dynamic.
  pub fn apply_impulse(&mut self, body: Entity, impulse: Vec2) {
    self.queue(body, Push::Impulse(impulse));
  }

  /// An impulse at a world point: off-centre it also spins the body.
  pub fn apply_impulse_at(&mut self, body: Entity, impulse: Vec2, point: Vec2) {
    self.queue(body, Push::ImpulseAt(impulse, point));
  }

  /// An instant change of angular momentum.
  pub fn apply_angular_impulse(&mut self, body: Entity, impulse: f32) {
    self.queue(body, Push::AngularImpulse(impulse));
  }

  /// A force (mass times px/s²) for the next step only: call it every step
  /// for a steady push.
  pub fn apply_force(&mut self, body: Entity, force: Vec2) {
    self.queue(body, Push::Force(force));
  }

  /// A force at a world point for the next step only.
  pub fn apply_force_at(&mut self, body: Entity, force: Vec2, point: Vec2) {
    self.queue(body, Push::ForceAt(force, point));
  }

  /// A torque for the next step only.
  pub fn apply_torque(&mut self, body: Entity, torque: f32) {
    self.queue(body, Push::Torque(torque));
  }

  /// The body's mass: the sum of its colliders' (a 100×100 px box at
  /// density 1 weighs 1). `None` for an entity without a simulated body.
  pub fn mass(&self, body: Entity) -> Option<f32> {
    let entry = self.body_entries.get(&body)?;
    Some(self.bodies.get(entry.handle)?.mass())
  }
}

/// A solid contact touching one of an entity's colliders: the other side,
/// the deepest point, the normal from the entity toward the other side
/// (ground below is `(0, +1)`) and the step's normal impulse.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Touch {
  pub collider: Entity,
  pub body: Entity,
  pub point: Vec2,
  pub normal: Vec2,
  pub impulse: f32,
}

impl Physics {
  /// The colliders of `entity`: all of a body's, or one collider.
  fn handles_of(&self, entity: Entity) -> Vec<ColliderHandle> {
    if let Some(entry) = self.body_entries.get(&entity)
      && let Some(body) = self.bodies.get(entry.handle)
    {
      return body.colliders().to_vec();
    }
    self
      .collider_entries
      .get(&entity)
      .map(|entry| vec![entry.handle])
      .unwrap_or_default()
  }

  /// The solid contacts touching `entity` (a body: all its colliders; or one
  /// collider) after the last step.
  pub fn contacts(&self, entity: Entity) -> Vec<Touch> {
    let mut touches = Vec::new();
    for handle in self.handles_of(entity) {
      for pair in self.narrow_phase.contact_pairs_with(handle) {
        if !pair.has_any_active_contact() {
          continue;
        }
        let other = if pair.collider1 == handle {
          pair.collider2
        } else {
          pair.collider1
        };
        let (Some(owner), Some((point, normal))) = (
          self.owner_of(other),
          first_contact(&self.colliders, pair, handle),
        ) else {
          continue;
        };
        touches.push(Touch {
          collider: owner.collider,
          body: owner.body,
          point,
          normal,
          impulse: pair.total_impulse_magnitude(),
        });
      }
    }
    touches
  }

  /// The colliders overlapping `entity`'s triggers, or the triggers `entity` is inside.
  /// Each of a body's colliders reports what it overlaps, so a collider
  /// overlapping several of them is listed once for each.
  pub fn overlaps(&self, entity: Entity) -> Vec<Overlap> {
    let mut overlaps = Vec::new();
    for handle in self.handles_of(entity) {
      for (a, b, intersecting) in self.narrow_phase.intersection_pairs_with(handle) {
        let other = if a == handle { b } else { a };
        if intersecting && let Some(owner) = self.owner_of(other) {
          overlaps.push(Overlap {
            collider: owner.collider,
            body: owner.body,
          });
        }
      }
    }
    overlaps
  }

  /// Whether any collider of `a` touches or overlaps any collider of `b`.
  pub fn in_contact(&self, a: Entity, b: Entity) -> bool {
    let theirs = self.handles_of(b);
    self.handles_of(a).into_iter().any(|mine| {
      theirs.iter().any(|&other| {
        self
          .narrow_phase
          .contact_pair(mine, other)
          .is_some_and(|pair| pair.has_any_active_contact())
          || self.narrow_phase.intersection_pair(mine, other) == Some(true)
      })
    })
  }
}

#[cfg(test)]
mod tests {
  use crate::core::ecs::{Transform, Vec2, Velocity};
  use crate::core::physics::test_util::*;
  use crate::core::physics::{AngularVelocity, Collider, Physics, RigidBody};

  fn crate_100(world: &mut crate::core::ecs::World, body: RigidBody) -> crate::core::ecs::Entity {
    world
      .spawn((Transform::IDENTITY, body, Collider::rect(100.0, 100.0)))
      .id()
  }

  #[test]
  fn mass_comes_from_collider_area() {
    let (mut world, mut s) = world(60);
    let one = crate_100(&mut world, RigidBody::dynamic());
    let two = world
      .spawn((
        Transform::from_xy(500.0, 0.0),
        RigidBody::dynamic(),
        Collider::rect(200.0, 100.0),
      ))
      .id();
    run(&mut world, &mut s, 1);
    let physics = world.resource::<Physics>();
    assert!((physics.mass(one).unwrap() - 1.0).abs() < 1e-3);
    assert!((physics.mass(two).unwrap() - 2.0).abs() < 1e-3);
  }

  #[test]
  fn an_impulse_changes_velocity_by_impulse_over_mass() {
    let (mut world, mut s) = world(60);
    no_gravity(&mut world);
    let e = crate_100(&mut world, RigidBody::dynamic().lock_rotation());
    // Queued before the body is registered: applied once it is
    world
      .resource_mut::<Physics>()
      .apply_impulse(e, Vec2::new(100.0, 0.0));
    run(&mut world, &mut s, 1);
    let dx = world.get::<Velocity>(e).unwrap().dx;
    assert!((dx - 100.0).abs() < 0.5, "{dx}");
  }

  #[test]
  fn a_force_acts_for_one_step() {
    let (mut world, mut s) = world(60);
    no_gravity(&mut world);
    let e = crate_100(&mut world, RigidBody::dynamic().lock_rotation());
    run(&mut world, &mut s, 1);
    world
      .resource_mut::<Physics>()
      .apply_force(e, Vec2::new(600.0, 0.0));
    run(&mut world, &mut s, 1);
    let after_one = world.get::<Velocity>(e).unwrap().dx;
    assert!((after_one - 10.0).abs() < 0.1, "600 / 1 / 60: {after_one}");
    run(&mut world, &mut s, 1);
    assert_eq!(
      world.get::<Velocity>(e).unwrap().dx,
      after_one,
      "gone after its step"
    );
  }

  #[test]
  fn torques_and_off_centre_impulses_spin_a_body() {
    let (mut world, mut s) = world(60);
    no_gravity(&mut world);
    let a = crate_100(&mut world, RigidBody::dynamic());
    let b = world
      .spawn((
        Transform::from_xy(500.0, 0.0),
        RigidBody::dynamic(),
        Collider::rect(100.0, 100.0),
      ))
      .id();
    run(&mut world, &mut s, 1);
    {
      let mut physics = world.resource_mut::<Physics>();
      physics.apply_torque(a, 5000.0);
      physics.apply_impulse_at(b, Vec2::new(0.0, 100.0), Vec2::new(550.0, 0.0));
      physics.apply_angular_impulse(a, 0.0);
    }
    run(&mut world, &mut s, 1);
    assert!(world.get::<AngularVelocity>(a).unwrap().0 > 0.0);
    assert!(world.get::<AngularVelocity>(b).unwrap().0 > 0.0);
    assert!(world.get::<Velocity>(b).unwrap().dy > 0.0);
  }

  #[test]
  fn pushes_on_non_dynamic_or_unknown_entities_are_ignored() {
    let (mut world, mut s) = world(60);
    let wall = crate_100(&mut world, RigidBody::fixed());
    let nobody = world.spawn(Transform::IDENTITY).id();
    {
      let mut physics = world.resource_mut::<Physics>();
      physics.apply_impulse(wall, Vec2::new(1e6, 0.0));
      physics.apply_force(nobody, Vec2::new(1e6, 0.0));
      physics.apply_impulse(wall, Vec2::new(f32::NAN, 0.0));
    }
    run(&mut world, &mut s, 10);
    assert_eq!(at(&world, wall).translation, Vec2::ZERO);
    assert_eq!(world.resource::<Physics>().mass(nobody), None);
  }

  use crate::core::physics::{ColliderShape, Hit, Overlap, QueryFilter};

  /// A floor whose top is at y 200, a fixed 20 px box at (100, 100) on layer 2,
  /// and a trigger zone 40 px wide at (0, 100); one step so queries see them.
  fn scene() -> (crate::core::ecs::World, [crate::core::ecs::Entity; 3]) {
    let (mut world, mut s) = world(60);
    let floor_e = floor(&mut world, 200.0);
    let block = world
      .spawn((
        Transform::from_xy(100.0, 100.0),
        RigidBody::fixed(),
        Collider::rect(20.0, 20.0).with_layer(0b10),
      ))
      .id();
    let zone = world
      .spawn((
        Transform::from_xy(0.0, 100.0),
        Collider::rect(40.0, 40.0).as_trigger(),
      ))
      .id();
    run(&mut world, &mut s, 1);
    (world, [floor_e, block, zone])
  }

  fn close(a: Vec2, b: Vec2) -> bool {
    (a - b).length() < 1e-2
  }

  #[test]
  fn a_ray_hits_the_first_solid_and_skips_triggers() {
    let (world, [floor_e, _, zone]) = scene();
    let physics = world.resource::<Physics>();
    let hit = physics
      .raycast(Vec2::ZERO, Vec2::Y, 1000.0, &QueryFilter::new())
      .unwrap();
    assert_eq!((hit.collider, hit.body), (floor_e, floor_e));
    assert!(
      close(hit.point, Vec2::new(0.0, 200.0)) && close(hit.normal, -Vec2::Y),
      "{hit:?}"
    );
    assert!((hit.distance - 200.0).abs() < 1e-2);
    let with = physics
      .raycast(
        Vec2::ZERO,
        Vec2::Y * 5.0,
        1000.0,
        &QueryFilter::new().with_triggers(),
      )
      .unwrap();
    assert_eq!(with.collider, zone);
    assert!(
      (with.distance - 80.0).abs() < 1e-2,
      "the direction is normalised: {with:?}"
    );
    assert!(
      physics
        .raycast(Vec2::ZERO, Vec2::Y, 150.0, &QueryFilter::new())
        .is_none(),
      "out of range"
    );
  }

  #[test]
  fn degenerate_rays_hit_nothing() {
    let (world, _) = scene();
    let physics = world.resource::<Physics>();
    assert!(
      physics
        .raycast(Vec2::ZERO, Vec2::ZERO, 1000.0, &QueryFilter::new())
        .is_none()
    );
    assert!(
      physics
        .raycast(
          Vec2::ZERO,
          Vec2::new(f32::NAN, 1.0),
          1000.0,
          &QueryFilter::new()
        )
        .is_none()
    );
    assert!(
      physics
        .raycast(
          Vec2::new(f32::INFINITY, 0.0),
          Vec2::Y,
          1000.0,
          &QueryFilter::new()
        )
        .is_none()
    );
    assert!(
      physics
        .raycast_all(Vec2::ZERO, Vec2::ZERO, 1000.0, &QueryFilter::new())
        .is_empty()
    );
  }

  #[test]
  fn raycast_all_is_nearest_first_and_filters_apply() {
    let (world, [floor_e, block, _]) = scene();
    let physics = world.resource::<Physics>();
    let all: Vec<_> = physics
      .raycast_all(Vec2::new(100.0, 0.0), Vec2::Y, 1000.0, &QueryFilter::new())
      .iter()
      .map(|hit: &Hit| hit.collider)
      .collect();
    assert_eq!(all, [block, floor_e]);
    let floor_only = QueryFilter::new().with_mask(0b1);
    assert_eq!(
      physics
        .raycast(Vec2::new(100.0, 0.0), Vec2::Y, 1000.0, &floor_only)
        .unwrap()
        .collider,
      floor_e
    );
    let skipping = QueryFilter::new().excluding(block);
    assert_eq!(
      physics
        .raycast(Vec2::new(100.0, 0.0), Vec2::Y, 1000.0, &skipping)
        .unwrap()
        .collider,
      floor_e
    );
  }

  #[test]
  fn excluding_a_body_skips_all_its_colliders() {
    use crate::core::ecs::ChildOf;
    let (mut world, mut s) = world(60);
    floor(&mut world, 200.0);
    let body = world
      .spawn((Transform::from_xy(0.0, 100.0), RigidBody::fixed()))
      .id();
    for y in [-20.0, 20.0] {
      world.spawn((
        Transform::from_xy(0.0, y),
        Collider::rect(20.0, 10.0),
        ChildOf(body),
      ));
    }
    run(&mut world, &mut s, 1);
    let physics = world.resource::<Physics>();
    assert_eq!(
      physics
        .raycast(Vec2::ZERO, Vec2::Y, 1000.0, &QueryFilter::new())
        .unwrap()
        .body,
      body
    );
    let hit = physics
      .raycast(
        Vec2::ZERO,
        Vec2::Y,
        1000.0,
        &QueryFilter::new().excluding(body),
      )
      .unwrap();
    assert_ne!(hit.body, body);
  }

  #[test]
  fn a_shape_cast_stops_where_the_shape_touches() {
    let (world, [floor_e, ..]) = scene();
    let physics = world.resource::<Physics>();
    let circle = ColliderShape::Circle { radius: 10.0 };
    let hit = physics
      .shape_cast(
        &circle,
        Vec2::new(-100.0, 0.0),
        0.0,
        Vec2::Y,
        1000.0,
        &QueryFilter::new(),
      )
      .unwrap();
    assert_eq!(hit.collider, floor_e);
    assert!((hit.distance - 190.0).abs() < 0.05, "{hit:?}");
    assert!(close(hit.normal, -Vec2::Y), "{hit:?}");
    let inside = physics
      .shape_cast(
        &circle,
        Vec2::new(-100.0, 205.0),
        0.0,
        Vec2::Y,
        1000.0,
        &QueryFilter::new(),
      )
      .unwrap();
    assert_eq!(inside.distance, 0.0);
    assert!(close(hit.point, Vec2::new(-100.0, 200.0)), "{hit:?}");
  }

  #[test]
  fn a_ray_starting_inside_hits_at_zero_with_no_normal() {
    let (world, [floor_e, ..]) = scene();
    let physics = world.resource::<Physics>();
    let hit = physics
      .raycast(Vec2::new(0.0, 205.0), Vec2::Y, 1000.0, &QueryFilter::new())
      .unwrap();
    assert_eq!(hit.collider, floor_e);
    assert_eq!(hit.distance, 0.0);
    assert!(
      close(hit.point, Vec2::new(0.0, 205.0)) && hit.normal == Vec2::ZERO,
      "{hit:?}"
    );
  }

  #[test]
  fn nearest_from_inside_reports_the_closest_boundary_point() {
    let (world, [floor_e, ..]) = scene();
    let physics = world.resource::<Physics>();
    let near = physics
      .nearest(Vec2::new(0.0, 205.0), 50.0, &QueryFilter::new())
      .unwrap();
    assert_eq!(near.collider, floor_e);
    assert!(
      near.inside && close(near.point, Vec2::new(0.0, 200.0)),
      "{near:?}"
    );
  }

  #[test]
  fn overlaps_and_the_nearest_point() {
    let (world, [floor_e, block, _]) = scene();
    let physics = world.resource::<Physics>();
    let at_floor = physics.overlap_point(Vec2::new(0.0, 205.0), &QueryFilter::new());
    assert_eq!(
      at_floor,
      [Overlap {
        collider: floor_e,
        body: floor_e
      }]
    );
    let area = ColliderShape::Rect {
      size: Vec2::splat(300.0),
    };
    let mut found: Vec<_> = physics
      .overlap_shape(&area, Vec2::new(50.0, 150.0), 0.0, &QueryFilter::new())
      .into_iter()
      .map(|o| o.collider)
      .collect();
    found.sort();
    let mut expected = vec![floor_e, block];
    expected.sort();
    assert_eq!(found, expected, "the trigger only with with_triggers");
    assert_eq!(
      physics
        .overlap_shape(
          &area,
          Vec2::new(50.0, 150.0),
          0.0,
          &QueryFilter::new().with_triggers()
        )
        .len(),
      3
    );
    let near = physics
      .nearest(Vec2::new(-150.0, 150.0), 100.0, &QueryFilter::new())
      .unwrap();
    assert_eq!(near.collider, floor_e);
    assert!(
      close(near.point, Vec2::new(-150.0, 200.0)) && !near.inside,
      "{near:?}"
    );
    assert!(
      physics
        .nearest(Vec2::new(-150.0, 0.0), 50.0, &QueryFilter::new())
        .is_none()
    );
  }

  #[test]
  fn queries_see_the_world_as_the_last_step_left_it() {
    let (mut world, mut s) = world(60);
    floor(&mut world, 200.0);
    let probe = |world: &crate::core::ecs::World| {
      world
        .resource::<Physics>()
        .raycast(Vec2::ZERO, Vec2::Y, 1000.0, &QueryFilter::new())
    };
    assert!(probe(&world).is_none(), "not stepped yet");
    run(&mut world, &mut s, 1);
    assert!(probe(&world).is_some());
  }

  #[test]
  fn contacts_report_the_ground_below() {
    let (mut world, mut s) = world(60);
    let floor_e = floor(&mut world, 100.0);
    let crate_e = world
      .spawn((
        Transform::from_xy(0.0, 80.0),
        RigidBody::dynamic().lock_rotation(),
        Collider::rect(20.0, 20.0),
      ))
      .id();
    run(&mut world, &mut s, 30);
    let physics = world.resource::<Physics>();
    let touches = physics.contacts(crate_e);
    assert_eq!(touches.len(), 1, "{touches:?}");
    let touch = touches[0];
    assert_eq!((touch.collider, touch.body), (floor_e, floor_e));
    assert!(
      close(touch.normal, Vec2::Y),
      "from the crate toward the floor: {touch:?}"
    );
    assert!(touch.impulse > 0.0 && (touch.point.y - 100.0).abs() < 1.5);
    let from_floor = physics.contacts(floor_e);
    assert!(close(from_floor[0].normal, -Vec2::Y));
    assert!(physics.in_contact(crate_e, floor_e) && physics.in_contact(floor_e, crate_e));
  }

  #[test]
  fn overlaps_list_what_is_inside_a_trigger() {
    let (mut world, mut s) = world(60);
    no_gravity(&mut world);
    let zone = world
      .spawn((
        Transform::IDENTITY,
        Collider::rect(100.0, 100.0).as_trigger(),
      ))
      .id();
    let inside = ball(&mut world, 10.0, 10.0);
    ball(&mut world, 300.0, 0.0);
    run(&mut world, &mut s, 2);
    let physics = world.resource::<Physics>();
    assert_eq!(
      physics.overlaps(zone),
      [Overlap {
        collider: inside,
        body: inside
      }]
    );
    assert!(physics.in_contact(zone, inside));
    assert!(
      physics.contacts(zone).is_empty(),
      "triggers have no solid contacts"
    );
  }

  #[test]
  fn contact_ends_when_bodies_part() {
    let (mut world, mut s) = world(60);
    let floor_e = floor(&mut world, 100.0);
    let b = ball(&mut world, 0.0, 85.0);
    run(&mut world, &mut s, 30);
    assert!(world.resource::<Physics>().in_contact(b, floor_e));
    world.get_mut::<Transform>(b).unwrap().translation.y = -500.0;
    run(&mut world, &mut s, 2);
    assert!(!world.resource::<Physics>().in_contact(b, floor_e));
  }

  #[test]
  fn a_body_s_contacts_include_every_collider() {
    use crate::core::ecs::ChildOf;
    let (mut world, mut s) = world(60);
    floor(&mut world, 100.0);
    let body = world
      .spawn((
        Transform::from_xy(0.0, 80.0),
        RigidBody::dynamic().lock_rotation(),
      ))
      .id();
    for x in [-30.0, 30.0] {
      world.spawn((
        Transform::from_xy(x, 0.0),
        Collider::rect(20.0, 20.0),
        ChildOf(body),
      ));
    }
    run(&mut world, &mut s, 30);
    assert_eq!(world.resource::<Physics>().contacts(body).len(), 2);
  }
}
