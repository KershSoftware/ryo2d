//! Bone definitions, transforms, and poses for skeletal animation.

use glam::Vec2;
use std::collections::HashMap;
use std::f32::consts::{PI, TAU};

/// Interpolate between two angles via the shortest path.
fn angle_lerp(a: f32, b: f32, t: f32) -> f32 {
  let mut diff = (b - a) % TAU;
  if diff > PI {
    diff -= TAU;
  }
  if diff < -PI {
    diff += TAU;
  }
  a + diff * t
}

/// Transform for a single bone in local space (relative to parent).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BoneTransform {
  pub position: Vec2,
  pub rotation: f32,
  pub scale: Vec2,
}

impl BoneTransform {
  pub const IDENTITY: Self = Self {
    position: Vec2::ZERO,
    rotation: 0.0,
    scale: Vec2::ONE,
  };

  pub fn new(position: Vec2, rotation: f32) -> Self {
    Self {
      position,
      rotation,
      scale: Vec2::ONE,
    }
  }

  pub fn rotation(rotation: f32) -> Self {
    Self {
      position: Vec2::ZERO,
      rotation,
      scale: Vec2::ONE,
    }
  }

  pub fn position(position: Vec2) -> Self {
    Self {
      position,
      rotation: 0.0,
      scale: Vec2::ONE,
    }
  }

  /// Linearly interpolate between two bone transforms.
  ///
  /// Uses shortest-path angle interpolation for rotation.
  pub fn lerp(&self, other: &Self, t: f32) -> Self {
    Self {
      position: self.position.lerp(other.position, t),
      rotation: angle_lerp(self.rotation, other.rotation, t),
      scale: self.scale.lerp(other.scale, t),
    }
  }
}

impl Default for BoneTransform {
  fn default() -> Self {
    Self::IDENTITY
  }
}

/// Definition of a single bone (immutable template data).
#[derive(Debug, Clone)]
pub struct BoneDef {
  pub name: String,
  pub parent: Option<usize>,
  pub rest_transform: BoneTransform,
  pub length: f32,
}

/// Shared skeleton template defining the bone hierarchy.
#[derive(Debug, Clone)]
pub struct SkeletonDef {
  pub bones: Vec<BoneDef>,
  pub bone_indices: HashMap<String, usize>,
}

/// A complete pose -- one BoneTransform per bone.
#[derive(Debug, Clone)]
pub struct Pose {
  pub transforms: Vec<BoneTransform>,
}

impl Pose {
  /// Create a rest pose from a skeleton definition.
  pub fn from_rest(def: &SkeletonDef) -> Self {
    Self {
      transforms: def.bones.iter().map(|b| b.rest_transform).collect(),
    }
  }

  /// Linearly interpolate between two poses.
  ///
  /// Both poses must have the same number of transforms.
  /// Uses `BoneTransform::lerp` for each bone (shortest-path angle interpolation).
  pub fn lerp(&self, other: &Self, t: f32) -> Self {
    assert_eq!(
      self.transforms.len(),
      other.transforms.len(),
      "Cannot lerp poses with different bone counts ({} vs {})",
      self.transforms.len(),
      other.transforms.len()
    );
    Self {
      transforms: self
        .transforms
        .iter()
        .zip(other.transforms.iter())
        .map(|(a, b)| a.lerp(b, t))
        .collect(),
    }
  }
}

/// Computed world-space position and rotation for a bone.
#[derive(Debug, Clone, Copy, Default)]
pub struct BoneWorldTransform {
  pub position: Vec2,
  pub rotation: f32,
}

/// Compute world transforms for all bones given a pose and skeleton def.
///
/// Bones must be ordered so parents come before children (guaranteed by builder).
/// Note: `BoneTransform::scale` is stored and interpolated but not yet propagated
/// through the hierarchy. Scale support in world transforms is reserved for a future update.
pub fn compute_world_transforms(
  def: &SkeletonDef,
  pose: &Pose,
  world_offset: Vec2,
  world_rotation: f32,
) -> Vec<BoneWorldTransform> {
  debug_assert_eq!(
    pose.transforms.len(),
    def.bones.len(),
    "Pose transform count ({}) does not match skeleton bone count ({})",
    pose.transforms.len(),
    def.bones.len()
  );

  let mut result = vec![BoneWorldTransform::default(); def.bones.len()];

  for (i, bone) in def.bones.iter().enumerate() {
    let local = &pose.transforms[i];

    let (parent_pos, parent_rot) = match bone.parent {
      Some(p) => (result[p].position, result[p].rotation),
      None => (world_offset, world_rotation),
    };

    let total_rot = parent_rot + local.rotation;
    let rotated_offset = Vec2::new(
      local.position.x * parent_rot.cos() - local.position.y * parent_rot.sin(),
      local.position.x * parent_rot.sin() + local.position.y * parent_rot.cos(),
    );

    result[i] = BoneWorldTransform {
      position: parent_pos + rotated_offset,
      rotation: total_rot,
    };
  }

  result
}

// ---------------------------------------------------------------------------
// Builder API
// ---------------------------------------------------------------------------

/// Builder for constructing skeleton definitions with named bones.
pub struct SkeletonDefBuilder {
  bones: Vec<BoneDef>,
  indices: HashMap<String, usize>,
}

impl SkeletonDefBuilder {
  pub fn new() -> Self {
    Self {
      bones: Vec::new(),
      indices: HashMap::new(),
    }
  }

  /// Add a bone to the skeleton.
  ///
  /// `parent` is the name of the parent bone (None for root).
  /// Panics if the parent name doesn't exist.
  pub fn bone(
    mut self,
    name: &str,
    parent: Option<&str>,
    position: Vec2,
    rotation: f32,
    length: f32,
  ) -> Self {
    let parent_idx = parent.map(|p| {
      *self
        .indices
        .get(p)
        .unwrap_or_else(|| panic!("Parent bone '{}' not found for bone '{}'", p, name))
    });

    let idx = self.bones.len();
    self.indices.insert(name.to_string(), idx);
    self.bones.push(BoneDef {
      name: name.to_string(),
      parent: parent_idx,
      rest_transform: BoneTransform::new(position, rotation),
      length,
    });
    self
  }

  pub fn build(self) -> SkeletonDef {
    SkeletonDef {
      bones: self.bones,
      bone_indices: self.indices,
    }
  }
}

impl Default for SkeletonDefBuilder {
  fn default() -> Self {
    Self::new()
  }
}

impl SkeletonDef {
  pub fn builder() -> SkeletonDefBuilder {
    SkeletonDefBuilder::new()
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn pose_lerp_blends_transforms() {
    let a = Pose {
      transforms: vec![
        BoneTransform::new(Vec2::new(0.0, 0.0), 0.0),
        BoneTransform::new(Vec2::new(10.0, 0.0), 1.0),
      ],
    };
    let b = Pose {
      transforms: vec![
        BoneTransform::new(Vec2::new(10.0, 10.0), 2.0),
        BoneTransform::new(Vec2::new(20.0, 0.0), 3.0),
      ],
    };
    let result = a.lerp(&b, 0.5);
    assert_eq!(result.transforms.len(), 2);
    assert!((result.transforms[0].position.x - 5.0).abs() < 0.01);
    assert!((result.transforms[0].position.y - 5.0).abs() < 0.01);
    assert!((result.transforms[0].rotation - 1.0).abs() < 0.01);
  }

  #[test]
  fn pose_lerp_at_zero_returns_self() {
    let a = Pose {
      transforms: vec![BoneTransform::new(Vec2::new(0.0, 0.0), 0.0)],
    };
    let b = Pose {
      transforms: vec![BoneTransform::new(Vec2::new(10.0, 10.0), 2.0)],
    };
    let result = a.lerp(&b, 0.0);
    assert!((result.transforms[0].position.x).abs() < 0.01);
  }

  #[test]
  fn pose_lerp_at_one_returns_other() {
    let a = Pose {
      transforms: vec![BoneTransform::new(Vec2::new(0.0, 0.0), 0.0)],
    };
    let b = Pose {
      transforms: vec![BoneTransform::new(Vec2::new(10.0, 10.0), 2.0)],
    };
    let result = a.lerp(&b, 1.0);
    assert!((result.transforms[0].position.x - 10.0).abs() < 0.01);
  }
}
