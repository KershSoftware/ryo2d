//! Skeleton system for updating skeletal animation each frame.
//!
//! This system handles three phases:
//! 1. Advancing animation time on all `SkeletalAnimator` components
//! 2. Sampling animations and writing poses to `Skeleton` components
//! 3. Recomputing `Skeleton.world_transforms` from the entity's `Transform`

use glam::Vec2;

use crate::core::ecs::{Inactive, Transform, World};
use crate::core::skeleton::{Pose, SkeletalAnimator, SkeletalController, Skeleton};

/// System for updating skeletal animation.
///
/// Updates `SkeletalAnimator` playback time, samples animations into
/// `Skeleton` poses, and recomputes world-space bone transforms.
pub struct SkeletonSystem;

impl SkeletonSystem {
  /// Update all skeletal animation components in the world.
  ///
  /// # Arguments
  /// * `world` - The ECS world containing entities
  /// * `dt` - Delta time (should be scaled for time effects)
  pub fn update(world: &mut World, dt: f32) {
    Self::update_animators(world, dt);
    Self::update_controllers(world, dt);
    Self::update_poses(world);
    Self::update_controller_poses(world);
    Self::update_world_transforms(world);
  }

  /// Advance animation time on standalone `SkeletalAnimator` components.
  ///
  /// Entities with a `SkeletalController` are skipped here because
  /// the controller manages its own animator in `update_controllers`.
  fn update_animators(world: &mut World, dt: f32) {
    for entity in world.entities_with::<SkeletalAnimator>() {
      if world.has::<Inactive>(entity) || world.has::<SkeletalController>(entity) {
        continue;
      }
      if let Some(mut animator) = world.get_mut::<SkeletalAnimator>(entity) {
        animator.update(dt);
      }
    }
  }

  /// Sample standalone animations and write the resulting poses to `Skeleton` components.
  ///
  /// Entities with a `SkeletalController` are skipped here because
  /// their poses are written in `update_controller_poses`.
  fn update_poses(world: &mut World) {
    for entity in world.entities_with::<SkeletalAnimator>() {
      if world.has::<Inactive>(entity) || world.has::<SkeletalController>(entity) {
        continue;
      }

      // Read data out (dropping borrows) to avoid borrow checker conflicts
      let anim_data = world
        .get::<SkeletalAnimator>(entity)
        .map(|a| (a.animation.clone(), a.time));
      let skeleton_def = world.get::<Skeleton>(entity).map(|s| s.def.clone());

      // Now write
      if let (Some((animation, time)), Some(def)) = (anim_data, skeleton_def) {
        let rest_pose = Pose::from_rest(&def);
        let new_pose = animation.sample(time, &rest_pose);
        if let Some(mut skeleton) = world.get_mut::<Skeleton>(entity) {
          skeleton.pose = new_pose;
        }
      }
    }
  }

  /// Update all `SkeletalController` components (FSM + blending).
  fn update_controllers(world: &mut World, dt: f32) {
    for entity in world.entities_with::<SkeletalController>() {
      if world.has::<Inactive>(entity) {
        continue;
      }
      if let Some(mut controller) = world.get_mut::<SkeletalController>(entity) {
        controller.update(dt);
      }
    }
  }

  /// Sample controller animations and write poses to `Skeleton` components.
  fn update_controller_poses(world: &mut World) {
    for entity in world.entities_with::<SkeletalController>() {
      if world.has::<Inactive>(entity) {
        continue;
      }
      let new_pose = world.get::<SkeletalController>(entity).map(|c| c.sample());

      if let Some(pose) = new_pose
        && let Some(mut skeleton) = world.get_mut::<Skeleton>(entity)
      {
        skeleton.pose = pose;
      }
    }
  }

  /// Recompute `Skeleton.world_transforms` from the entity's `Transform`.
  fn update_world_transforms(world: &mut World) {
    for entity in world.entities_with::<Skeleton>() {
      if world.has::<Inactive>(entity) {
        continue;
      }

      // Read position and rotation from the entity's Transform
      let transform_data = world
        .get::<Transform>(entity)
        .map(|t| (Vec2::new(t.x, t.y), t.rotation));

      let (world_pos, world_rot) = transform_data.unwrap_or((Vec2::ZERO, 0.0));

      if let Some(mut skeleton) = world.get_mut::<Skeleton>(entity) {
        skeleton.update_world_transforms(world_pos, world_rot);
      }
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn test_skeleton_system_no_crash_on_empty_world() {
    let mut world = World::default();
    // Should not panic on empty world
    SkeletonSystem::update(&mut world, 0.016);
  }

  #[test]
  fn test_skeleton_system_updates_controller_entities() {
    use std::sync::Arc;

    use glam::Vec2;

    use crate::core::ecs::{Ease, Transform};
    use crate::core::graphics::LoopMode;
    use crate::core::skeleton::{
      BoneTransform, SkeletalAnimation, SkeletalController, Skeleton, SkeletonDef,
    };

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

    let controller =
      SkeletalController::new(idle, "idle", def.clone()).with_default_blend_duration(0.0);

    let mut world = World::default();
    let entity = world.spawn((Transform::new(100.0, 200.0), Skeleton::new(def), controller));

    SkeletonSystem::update(&mut world, 0.5);

    let skeleton = world.get::<Skeleton>(entity).unwrap();
    // After 0.5s into a 1s animation with root rotating 0->1,
    // the root bone should have ~0.5 rotation in its pose
    assert!((skeleton.pose.transforms[0].rotation - 0.5).abs() < 0.05);
  }
}
