//! What a slot shows: a region of an image, a point, or a bounding box.

use crate::core::ecs::Vec2;
use crate::core::ui::Color;

/// Something a skin puts in a slot. Its offsets are in its bone's space
#[derive(Debug, Clone, PartialEq)]
pub enum Attachment {
  Region(RegionAttachment),
  Point(PointAttachment),
  BoundingBox(BoundingBoxAttachment),
}

/// A textured quad from one of the skeleton's image regions, centred on its
/// offset from the bone
#[derive(Debug, Clone, PartialEq)]
pub struct RegionAttachment {
  /// An index into [`SkeletonData::regions`](super::SkeletonData::regions)
  pub region: usize,
  pub x: f32,
  pub y: f32,
  /// Radians
  pub rotation: f32,
  pub scale_x: f32,
  pub scale_y: f32,
  /// The quad's size before scale, in the bone's units
  pub width: f32,
  pub height: f32,
  /// Multiplied with the slot's colour
  pub color: Color,
}

impl RegionAttachment {
  /// Region `region` drawn `width` by `height`, centred on the bone, with
  /// no rotation, unit scale and white colour
  pub fn new(region: usize, width: f32, height: f32) -> Self {
    Self {
      region,
      x: 0.0,
      y: 0.0,
      rotation: 0.0,
      scale_x: 1.0,
      scale_y: 1.0,
      width,
      height,
      color: Color::WHITE,
    }
  }
}

/// A position and direction on a bone, for spawning things (a muzzle, a
/// hand)
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct PointAttachment {
  pub x: f32,
  pub y: f32,
  /// Radians
  pub rotation: f32,
}

/// A polygon on a bone that the game can hit-test
#[derive(Debug, Clone, PartialEq, Default)]
pub struct BoundingBoxAttachment {
  /// At least three, in the bone's space
  pub vertices: Vec<Vec2>,
}
