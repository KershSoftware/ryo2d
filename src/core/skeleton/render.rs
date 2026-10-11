//! Debug rendering for skeletons using Line2D components.

use super::bone::{BoneWorldTransform, SkeletonDef};
use crate::core::trail::Line2D;
use crate::core::ui::Color;

/// Z order of skeleton debug lines: above game content, below the engine's
/// own debug overlays (`i32::MAX`).
pub const SKELETON_DEBUG_Z_ORDER: i32 = i32::MAX - 1;

/// Generate Line2D components for debug visualization of a skeleton.
///
/// Returns a vec of Line2D segments (bones) and circles (joints) suitable
/// for spawning as entities or updating existing debug entities. The lines
/// draw on top of the world, at `SKELETON_DEBUG_Z_ORDER`.
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
      lines.push(
        Line2D::segment(parent_pos, bone_pos, bone_color, line_width)
          .with_z_order(SKELETON_DEBUG_Z_ORDER),
      );
    }

    // Draw joint circle
    lines.push(
      Line2D::circle(bone_pos, joint_radius, joint_color, line_width, 8)
        .with_z_order(SKELETON_DEBUG_Z_ORDER),
    );
  }

  lines
}
