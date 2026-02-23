//! Debug rendering for skeletons using Line2D components.

use super::bone::{BoneWorldTransform, SkeletonDef};
use crate::core::trail::Line2D;
use crate::core::ui::Color;

/// Generate Line2D components for debug visualization of a skeleton.
///
/// Returns a vec of Line2D segments (bones) and circles (joints) suitable
/// for spawning as entities or updating existing debug entities.
pub fn skeleton_debug_lines(
  def: &SkeletonDef,
  world_transforms: &[BoneWorldTransform],
  bone_color: Color,
  joint_color: Color,
  line_width: f32,
  joint_radius: f32,
) -> Vec<Line2D> {
  let mut lines = Vec::new();

  for (i, bone) in def.bones.iter().enumerate() {
    let bone_pos = world_transforms[i].position;

    // Draw bone line to parent
    if let Some(parent_idx) = bone.parent {
      let parent_pos = world_transforms[parent_idx].position;
      lines.push(Line2D::segment(
        parent_pos, bone_pos, bone_color, line_width,
      ));
    }

    // Draw joint circle
    lines.push(Line2D::circle(
      bone_pos,
      joint_radius,
      joint_color,
      line_width,
      8,
    ));
  }

  lines
}
