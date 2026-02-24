//! Crossfade blending between two skeletal poses.

use super::bone::Pose;

/// Blends between a snapshot "from" pose and the current "to" pose.
///
/// When a blend is started, the current pose is snapshotted. Each frame,
/// the blend weight advances toward 1.0. The output pose is a lerp
/// between the snapshot and the live pose.
#[derive(Debug, Clone)]
pub struct SkeletalBlender {
  from_pose: Option<Pose>,
  blend_weight: f32,
  blend_duration: f32,
  blending: bool,
}

impl SkeletalBlender {
  pub fn new() -> Self {
    Self {
      from_pose: None,
      blend_weight: 1.0,
      blend_duration: 0.0,
      blending: false,
    }
  }

  /// Start blending from a snapshot of the current pose.
  pub fn start_blend(&mut self, from_pose: Pose, duration: f32) {
    if duration <= 0.0 {
      self.blending = false;
      self.from_pose = None;
      self.blend_weight = 1.0;
      return;
    }
    self.from_pose = Some(from_pose);
    self.blend_weight = 0.0;
    self.blend_duration = duration;
    self.blending = true;
  }

  /// Advance the blend weight by dt.
  pub fn update(&mut self, dt: f32) {
    if !self.blending {
      return;
    }
    self.blend_weight += dt / self.blend_duration;
    if self.blend_weight >= 1.0 {
      self.blend_weight = 1.0;
      self.blending = false;
      self.from_pose = None;
    }
  }

  /// Apply blending: lerp from snapshot toward the target pose.
  ///
  /// If not blending, returns `target` unchanged.
  pub fn apply(&self, target: &Pose) -> Pose {
    match &self.from_pose {
      Some(from) if self.blending => from.lerp(target, self.blend_weight),
      _ => target.clone(),
    }
  }

  /// Whether a blend is currently active.
  pub fn is_blending(&self) -> bool {
    self.blending
  }

  /// Get the current blend progress (0.0 = fully "from", 1.0 = fully "to").
  ///
  /// Returns 1.0 when not blending.
  pub fn blend_progress(&self) -> f32 {
    self.blend_weight
  }
}

impl Default for SkeletalBlender {
  fn default() -> Self {
    Self::new()
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::skeleton::BoneTransform;
  use glam::Vec2;

  fn pose_at(x: f32, rot: f32) -> Pose {
    Pose {
      transforms: vec![BoneTransform::new(Vec2::new(x, 0.0), rot)],
    }
  }

  #[test]
  fn new_blender_is_not_blending() {
    let blender = SkeletalBlender::new();
    assert!(!blender.is_blending());
  }

  #[test]
  fn start_blend_activates_blending() {
    let mut blender = SkeletalBlender::new();
    blender.start_blend(pose_at(0.0, 0.0), 0.5);
    assert!(blender.is_blending());
    assert!((blender.blend_weight).abs() < 0.001);
  }

  #[test]
  fn update_advances_blend_weight() {
    let mut blender = SkeletalBlender::new();
    blender.start_blend(pose_at(0.0, 0.0), 1.0);
    blender.update(0.5);
    assert!((blender.blend_weight - 0.5).abs() < 0.001);
  }

  #[test]
  fn blend_completes_at_duration() {
    let mut blender = SkeletalBlender::new();
    blender.start_blend(pose_at(0.0, 0.0), 0.2);
    blender.update(0.2);
    assert!(!blender.is_blending());
  }

  #[test]
  fn apply_lerps_between_poses() {
    let mut blender = SkeletalBlender::new();
    blender.start_blend(pose_at(0.0, 0.0), 1.0);
    blender.update(0.5);
    let target = pose_at(10.0, 2.0);
    let result = blender.apply(&target);
    assert!((result.transforms[0].position.x - 5.0).abs() < 0.01);
  }

  #[test]
  fn apply_returns_target_when_not_blending() {
    let blender = SkeletalBlender::new();
    let target = pose_at(10.0, 2.0);
    let result = blender.apply(&target);
    assert!((result.transforms[0].position.x - 10.0).abs() < 0.01);
  }

  #[test]
  fn zero_duration_blend_completes_immediately() {
    let mut blender = SkeletalBlender::new();
    blender.start_blend(pose_at(0.0, 0.0), 0.0);
    assert!(!blender.is_blending());
  }
}
