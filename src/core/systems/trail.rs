//! Trails: record each `Trail2D` entity's position every fixed step.

use crate::core::ecs::{DrawLift, GlobalTransform, Query, Res, Vec2};
use crate::core::time::Time;
use crate::core::trail::Trail2D;

/// Add the entity's world position to its trail (`FixedSet::Trails`, after
/// propagation), where it draws: lifted by its floor's height, on a floor.
pub(crate) fn record_trail_points(
  time: Res<Time>,
  mut query: Query<(&GlobalTransform, &mut Trail2D, Option<&DrawLift>)>,
) {
  let dt = time.fixed.timestep();
  for (global, mut trail, lift) in &mut query {
    let position = global.translation() + lift.map_or(Vec2::ZERO, |drawn| drawn.lift);
    trail.update(position.x, position.y, dt);
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::ecs::{Disabled, DrawLift, Floor, Transform, Vec2};
  use crate::core::systems::test_util::{run, world};
  use crate::core::ui::Color;

  #[test]
  fn a_trail_records_its_entity_s_world_position() {
    let mut world = world(60);
    let t = Transform::from_xy(100.0, 50.0);
    let entity = world
      .spawn((
        t,
        GlobalTransform::from_transform(&t),
        Trail2D::new(Color::WHITE, 2.0),
      ))
      .id();
    run(&mut world, record_trail_points);
    let trail = world.get::<Trail2D>(entity).unwrap();
    assert!(trail.point_count() > 0);
  }

  #[test]
  fn a_trail_on_a_floor_records_lifted_points() {
    // A trail held on floor 1, which draws 32 px up: it records where its
    // entity draws
    let mut world = world(60);
    let entity = world
      .spawn((
        Transform::from_xy(100.0, 50.0),
        Trail2D::new(Color::WHITE, 2.0),
        DrawLift {
          lift: Vec2::new(0.0, -32.0),
          floor: Floor::On(1),
        },
      ))
      .id();
    run(&mut world, record_trail_points);
    let trail = world.get::<Trail2D>(entity).unwrap();
    let points: Vec<(f32, f32)> = trail.points().iter().map(|p| (p.x, p.y)).collect();
    assert_eq!(points, [(100.0, 18.0)]);
  }

  #[test]
  fn a_disabled_entity_s_trail_is_untouched() {
    let mut world = world(60);
    let entity = world
      .spawn((
        Transform::IDENTITY,
        Trail2D::new(Color::WHITE, 2.0),
        Disabled,
      ))
      .id();
    run(&mut world, record_trail_points);
    assert_eq!(world.get::<Trail2D>(entity).unwrap().point_count(), 0);
  }
}
