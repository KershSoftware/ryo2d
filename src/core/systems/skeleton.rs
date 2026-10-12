//! Skeletal animation: advance each skeleton's controller or animator by the
//! fixed step, write the sampled pose to its `Skeleton`, then place the bones
//! at the entity's world transform.

use glam::Vec2;

use crate::core::ecs::{GlobalTransform, Query, Res};
use crate::core::skeleton::{Pose, SkeletalAnimator, SkeletalController, Skeleton};
use crate::core::time::Time;

/// A skeleton, where its entity is, and what drives its pose.
type SkeletonItem = (
  &'static mut Skeleton,
  Option<&'static GlobalTransform>,
  Option<&'static mut SkeletalController>,
  Option<&'static mut SkeletalAnimator>,
);

/// Advance and pose every skeleton, then recompute its bones' world
/// transforms from the entity's `GlobalTransform`.
///
/// A `SkeletalController` drives its own animator (state machine and
/// blending), so a standalone `SkeletalAnimator` on the same entity is left
/// alone.
pub fn update_skeletons(time: Res<Time>, mut skeletons: Query<SkeletonItem>) {
  let dt = time.fixed.timestep();
  for (mut skeleton, global, controller, animator) in &mut skeletons {
    if let Some(mut controller) = controller {
      controller.update(dt);
      skeleton.pose = controller.sample();
    } else if let Some(mut animator) = animator {
      animator.update(dt);
      let rest = Pose::from_rest(&skeleton.def);
      skeleton.pose = animator.animation.sample(animator.time, &rest);
    }
    let (position, rotation) = global.map_or((Vec2::ZERO, 0.0), |global| {
      (global.translation(), global.rotation())
    });
    skeleton.update_world_transforms(position, rotation);
  }
}

#[cfg(test)]
mod tests {
  use std::sync::Arc;

  use super::*;
  use crate::core::ecs::{Ease, Transform};
  use crate::core::graphics::LoopMode;
  use crate::core::skeleton::{BoneTransform, SkeletalAnimation, SkeletonDef};
  use crate::core::systems::test_util;

  /// One root bone whose rotation goes from 0 to 1 over a second, looping.
  fn rig() -> (Arc<SkeletonDef>, Arc<SkeletalAnimation>) {
    let def = Arc::new(
      SkeletonDef::builder()
        .bone("root", None, Vec2::ZERO, 0.0, 10.0)
        .build(),
    );
    let idle = Arc::new(
      SkeletalAnimation::builder("idle", 1.0, def.clone())
        .loop_mode(LoopMode::Loop)
        .track("root", |t| {
          t.at(0.0, BoneTransform::rotation(0.0), Ease::Linear).at(
            1.0,
            BoneTransform::rotation(1.0),
            Ease::Linear,
          )
        })
        .build(),
    );
    (def, idle)
  }

  #[test]
  fn an_empty_world_is_fine() {
    let mut world = test_util::world(60);
    test_util::run(&mut world, update_skeletons);
  }

  #[test]
  fn a_controller_poses_its_skeleton() {
    let (def, idle) = rig();
    let controller =
      SkeletalController::new(idle, "idle", def.clone()).with_default_blend_duration(0.0);
    // A 0.5 s step: half way through the root's 0 to 1 rotation
    let mut world = test_util::world(2);
    let entity = world
      .spawn((
        Transform::from_xy(100.0, 200.0),
        Skeleton::new(def),
        controller,
      ))
      .id();
    test_util::run(&mut world, update_skeletons);
    let skeleton = world.get::<Skeleton>(entity).unwrap();
    assert!((skeleton.pose.transforms[0].rotation - 0.5).abs() < 0.05);
  }

  #[test]
  fn a_standalone_animator_poses_its_skeleton() {
    let (def, idle) = rig();
    let mut world = test_util::world(4);
    let entity = world
      .spawn((Skeleton::new(def), SkeletalAnimator::new(idle)))
      .id();
    test_util::run(&mut world, update_skeletons);
    let skeleton = world.get::<Skeleton>(entity).unwrap();
    assert!((skeleton.pose.transforms[0].rotation - 0.25).abs() < 0.05);
  }

  #[test]
  fn bones_are_placed_at_the_entity_s_world_transform() {
    let (def, _) = rig();
    let mut world = test_util::world(60);
    let transform = Transform::from_xy(30.0, 40.0);
    let entity = world
      .spawn((
        transform,
        GlobalTransform::from_transform(&transform),
        Skeleton::new(def),
      ))
      .id();
    test_util::run(&mut world, update_skeletons);
    let skeleton = world.get::<Skeleton>(entity).unwrap();
    assert_eq!(skeleton.world_transforms[0].position, Vec2::new(30.0, 40.0));
  }
}
