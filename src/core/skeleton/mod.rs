//! Skeletal animation system.
//!
//! Provides bone hierarchies, per-bone-track keyframe animation,
//! and debug rendering for skeletal animation.

mod animation;
mod blend;
mod bone;
mod controller;
mod render;

pub use animation::{
  BoneKeyframe, BoneTrack, SkeletalAnimation, SkeletalAnimationBuilder, SkeletalAnimator,
  TrackBuilder,
};
pub use blend::SkeletalBlender;
pub use bone::{
  BoneDef, BoneTransform, BoneWorldTransform, Pose, SkeletonDef, SkeletonDefBuilder,
  compute_world_transforms,
};
pub use controller::{SkeletalAnimationId, SkeletalController};
pub use render::skeleton_debug_lines;

use std::sync::Arc;

use glam::Vec2;

/// ECS component for a skeleton instance.
#[derive(Debug, Clone)]
pub struct Skeleton {
  pub def: Arc<SkeletonDef>,
  pub pose: Pose,
  pub world_transforms: Vec<BoneWorldTransform>,
}

impl Skeleton {
  pub fn new(def: Arc<SkeletonDef>) -> Self {
    let pose = Pose::from_rest(&def);
    let world_transforms = vec![BoneWorldTransform::default(); def.bones.len()];
    Self {
      def,
      pose,
      world_transforms,
    }
  }

  pub fn update_world_transforms(&mut self, world_pos: Vec2, world_rot: f32) {
    self.world_transforms = compute_world_transforms(&self.def, &self.pose, world_pos, world_rot);
  }
}
