//! Transform, hierarchy and anchor integration tests
//!
//! `Transform` is local to the parent (or the world, for a root); the engine
//! writes `GlobalTransform`. Single sprites are placed by their anchor, which
//! defaults to the centre.

use ryo2d::prelude::*;
use std::f32::consts::{FRAC_PI_2, PI, TAU};

#[derive(ScheduleLabel, Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct Step;

/// Run transform propagation once, as the engine does each step.
fn propagate(world: &mut World) {
  let mut schedule = Schedule::new(Step);
  schedule.add_systems(propagate_transforms);
  schedule.run(world);
}

fn close(a: Vec2, b: Vec2) -> bool {
  (a - b).length() < 1e-4
}

// ============================================================================
// DEFAULTS
// ============================================================================

#[test]
fn the_default_transform_is_the_identity() {
  assert_eq!(Transform::default(), Transform::IDENTITY);
}

#[test]
fn a_sprite_is_centred_on_its_position_by_default() {
  let sprite = SpriteHandle::from_sprite(Sprite::solid_rect(4, 4, 255, 255, 255, 255), "dot");
  assert_eq!(Renderable::sprite(sprite).anchor, Anchor::Center);
}

/// REGRESSION: Default rotation must be 0 (no rotation)
#[test]
fn regression_default_rotation_is_zero() {
  let transform = Transform::from_xy(100.0, 200.0);
  assert_eq!(transform.rotation, 0.0, "Default rotation must be 0");
}

/// REGRESSION: Default scale must be 1.0 (no scaling)
#[test]
fn regression_default_scale_is_one() {
  let transform = Transform::from_xy(100.0, 200.0);
  assert_eq!(transform.scale, Vec2::ONE, "Default scale must be 1.0");
}

/// REGRESSION: Engine uses radians for rotation, not degrees
///
/// This catches the bug where game code set rotation in degrees
/// (e.g., 25 degrees) but the engine interpreted it as radians
/// (25 radians = 1432 degrees), causing sprites to spin wildly.
#[test]
fn regression_rotation_is_in_radians() {
  let mut transform = Transform::from_xy(0.0, 0.0);

  transform.rotation = FRAC_PI_2;
  assert!((transform.rotation_degrees() - 90.0).abs() < 0.001);

  transform.rotation = PI;
  assert!((transform.rotation_degrees() - 180.0).abs() < 0.001);

  transform.rotation = TAU;
  assert!((transform.rotation_degrees() - 360.0).abs() < 0.001);

  assert!(
    (Transform::IDENTITY.with_rotation_degrees(45.0).rotation - PI / 4.0).abs() < 0.0001,
    "degrees go through the explicit helper"
  );
}

// ============================================================================
// BUILDERS
// ============================================================================

#[test]
fn builder_method_chaining() {
  let transform = Transform::from_xy(100.0, 200.0)
    .with_rotation(PI / 4.0)
    .with_scale(Vec2::new(2.0, 3.0));

  assert_eq!(transform.translation, Vec2::new(100.0, 200.0));
  assert!((transform.rotation - PI / 4.0).abs() < 0.0001);
  assert_eq!(transform.scale, Vec2::new(2.0, 3.0));
}

#[test]
fn uniform_scale() {
  let transform = Transform::from_xy(0.0, 0.0).with_uniform_scale(3.0);
  assert_eq!(transform.scale, Vec2::splat(3.0));
}

#[test]
fn from_translation_matches_from_xy() {
  assert_eq!(
    Transform::from_translation(Vec2::new(50.0, 75.0)),
    Transform::from_xy(50.0, 75.0)
  );
}

// ============================================================================
// HIERARCHY
// ============================================================================

#[test]
fn a_child_of_a_rotated_parent_is_placed_in_its_frame() {
  let mut world = World::new();
  let parent = world
    .spawn(Transform::from_xy(100.0, 100.0).with_rotation(FRAC_PI_2))
    .id();
  // 10 px along the parent's x axis, which points down the screen
  let child = world.spawn_child(parent, Transform::from_xy(10.0, 0.0));

  // Moving the parent moves the child once the step propagates
  world
    .get_mut::<Transform>(parent)
    .expect("parent")
    .translation = Vec2::new(200.0, 100.0);
  propagate(&mut world);

  let global = world.get::<GlobalTransform>(child).expect("child global");
  assert!(
    close(global.translation(), Vec2::new(200.0, 110.0)),
    "child at {:?}",
    global.translation()
  );
}

#[test]
fn detaching_keeps_the_world_position() {
  let mut world = World::new();
  let parent = world
    .spawn(Transform::from_xy(100.0, 50.0).with_rotation(FRAC_PI_2))
    .id();
  let child = world.spawn_child(parent, Transform::from_xy(10.0, 0.0));
  propagate(&mut world);
  let before = world
    .get::<GlobalTransform>(child)
    .expect("global")
    .translation();

  world.detach_keep_world(child);
  propagate(&mut world);

  assert!(world.get::<ChildOf>(child).is_none());
  let local = world.get::<Transform>(child).expect("transform");
  assert!(
    close(local.translation, before),
    "local {:?}",
    local.translation
  );
  let after = world
    .get::<GlobalTransform>(child)
    .expect("global")
    .translation();
  assert!(close(after, before), "moved from {before:?} to {after:?}");
}

// ============================================================================
// TRANSFORM + WORLD
// ============================================================================

#[test]
fn transform_spawn_and_query() {
  let mut world = World::new();
  let entity = world.spawn(Transform::from_xy(100.0, 200.0)).id();

  let transform = world.get::<Transform>(entity).expect("transform");
  assert_eq!(transform.translation, Vec2::new(100.0, 200.0));
  // Spawning places the entity at once
  let global = world.get::<GlobalTransform>(entity).expect("global");
  assert_eq!(global.translation(), Vec2::new(100.0, 200.0));
}

#[test]
fn transform_modification() {
  let mut world = World::new();
  let entity = world.spawn(Transform::from_xy(0.0, 0.0)).id();

  if let Some(mut transform) = world.get_mut::<Transform>(entity) {
    transform.translation = Vec2::new(50.0, 75.0);
    transform.rotation = PI / 2.0;
  }

  let transform = world.get::<Transform>(entity).expect("transform");
  assert_eq!(transform.translation, Vec2::new(50.0, 75.0));
  assert!((transform.rotation - PI / 2.0).abs() < 0.0001);
}

// ============================================================================
// VELOCITY
// ============================================================================

#[test]
fn velocity_creation() {
  let velocity = Velocity::new(5.0, -3.0);
  assert_eq!(velocity.dx, 5.0);
  assert_eq!(velocity.dy, -3.0);
}

#[test]
fn velocity_default_is_zero() {
  let velocity = Velocity::default();
  assert_eq!(velocity.dx, 0.0);
  assert_eq!(velocity.dy, 0.0);
}

#[test]
fn velocity_with_transform() {
  let mut world = World::new();
  world.spawn((Transform::from_xy(100.0, 100.0), Velocity::new(10.0, 5.0)));

  let mut query = world.query::<(&Transform, &Velocity)>();
  let found: Vec<_> = query.iter(&world).collect();
  assert_eq!(found.len(), 1);

  let (transform, velocity) = found[0];
  assert_eq!(transform.translation.x, 100.0);
  assert_eq!(velocity.dx, 10.0);
}
