//! Debug drawing for skeletons.

use super::bone::{BoneWorldTransform, SkeletonDef};
use crate::core::render::DebugDraw;
use crate::core::ui::Color;

/// Draw a skeleton's bones (a line from each bone to its parent) and joints
/// (a circle of `joint_radius` at each bone) into `debug`, `width` screen
/// pixels wide. Call it every update to keep the skeleton drawn.
pub fn draw_skeleton(
  debug: &mut DebugDraw,
  def: &SkeletonDef,
  world_transforms: &[BoneWorldTransform],
  bone_color: Color,
  joint_color: Color,
  width: f32,
  joint_radius: f32,
) {
  let mut pen = debug.width(width);
  for (bone, transform) in def.bones.iter().zip(world_transforms) {
    let at = transform.position;
    if let Some(parent) = bone.parent.and_then(|parent| world_transforms.get(parent)) {
      pen.line(parent.position, at, bone_color);
    }
    pen.circle(at, joint_radius, joint_color);
  }
}
