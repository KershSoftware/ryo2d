//! How far a skeleton can draw from its bones: each bone's reach, worked
//! out once as the data is built, and the box each pose keeps in the
//! skeleton's own space, which the entity drawer takes to the world and
//! tests against the cameras before it walks the skeleton's parts.

use glam::{Affine2, Vec2};

use super::attachment::{Attachment, MeshVertices};
use super::data::{Region, SlotData};
use super::pose::SlotDeform;
use super::skin::Skin;
use crate::core::graphics::image::CHECKER_SIDE;

/// How far the bones that carry something reach, worked out once from the
/// data: only those bones are visited when a pose finds its box
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct BoneReaches {
  /// Each bone that carries something (an attachment in a slot of the
  /// bone, or a mesh vertex weighted to it), by its index, in bone order
  pub(crate) bones: Vec<(usize, BoneReach)>,
  /// Some bone carries a mesh vertex: without one, no deform moves anything
  pub(crate) meshed: bool,
}

/// How far what a bone carries can reach from its origin, in its own units,
/// over every attachment of every skin
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(crate) struct BoneReach {
  /// Something is on it
  holds: bool,
  /// The furthest any of those points is from its origin: a region's
  /// corners (placed by its pivot), a mesh's vertices (a weighted vertex's
  /// place for this bone), a point, a bounding box's corners
  pub(crate) reach: f32,
  /// A mesh vertex is given in its space, which a deform moves
  pub(crate) meshed: bool,
  /// The most a region's corner on it moves for each texel an effect grows
  /// its quad by (an outline, a glow): the length of a texel's diagonal,
  /// counted in the region's pixels or the checker's 16 (which stands in
  /// for an image that is not loaded), whichever are fewer
  pub(crate) texel: f32,
}

/// The reach of each bone of `bones` that carries something, from the
/// attachments `skins` put in `slots`
pub(crate) fn bone_reaches(
  bones: usize,
  slots: &[SlotData],
  regions: &[Region],
  skins: &[Skin],
) -> BoneReaches {
  let mut reaches = vec![BoneReach::default(); bones];
  // Bone `bone` carries a point at `at` in its space (a mesh's if `meshed`)
  let reach = |reaches: &mut [BoneReach], bone: usize, at: Vec2, meshed: bool| {
    if let Some(reach) = reaches.get_mut(bone) {
      reach.holds = true;
      reach.reach = reach.reach.max(at.length());
      reach.meshed |= meshed;
    }
  };
  for skin in skins {
    for (slot, _, attachment) in skin.attachments() {
      let Some(bone) = slots.get(slot.index()).map(|slot| slot.bone.index()) else {
        continue;
      };
      match attachment {
        Attachment::Region(shown) => {
          let Some(region) = regions.get(shown.region) else {
            continue;
          };
          let quad = shown.quad(region);
          for corner in [(-0.5, -0.5), (0.5, -0.5), (0.5, 0.5), (-0.5, 0.5)] {
            let at = quad.transform_point2(Vec2::from(corner));
            reach(&mut reaches, bone, at, false);
          }
          let across =
            |axis: Vec2, pixels: u32| axis.length() / pixels.clamp(1, CHECKER_SIDE) as f32;
          let x = across(quad.matrix2.x_axis, region.width);
          let y = across(quad.matrix2.y_axis, region.height);
          if let Some(reach) = reaches.get_mut(bone) {
            reach.texel = reach.texel.max(x.hypot(y));
          }
        }
        Attachment::Mesh(mesh) => match &mesh.vertices {
          MeshVertices::Unweighted(vertices) => {
            for &vertex in vertices {
              reach(&mut reaches, bone, vertex, true);
            }
          }
          MeshVertices::Weighted(weighted) => {
            for influence in weighted.each().flatten() {
              let at = Vec2::new(influence.x, influence.y);
              reach(&mut reaches, influence.bone.index(), at, true);
            }
          }
        },
        Attachment::Point(point) => {
          reach(&mut reaches, bone, Vec2::new(point.x, point.y), false);
        }
        Attachment::BoundingBox(shape) => {
          for &vertex in &shape.vertices {
            reach(&mut reaches, bone, vertex, false);
          }
        }
      }
    }
  }
  BoneReaches {
    meshed: reaches.iter().any(|reach| reach.meshed),
    bones: reaches
      .into_iter()
      .enumerate()
      .filter(|(_, reach)| reach.holds)
      .collect(),
  }
}

/// A box in a skeleton's own space that its parts stay inside, drawn at
/// its last pose, at the pose before, or anywhere between the two
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Bounds {
  pub(crate) min: Vec2,
  pub(crate) max: Vec2,
  /// How far, in the skeleton's space, a region's corner can move for each
  /// texel an effect grows its quad by
  pub(crate) texel: f32,
}

impl Bounds {
  /// Whether nothing is drawn: no bone carries anything
  pub(crate) fn is_empty(&self) -> bool {
    !(self.min.x <= self.max.x && self.min.y <= self.max.y)
  }
}

/// The box of a pose whose bones are at `now` in the skeleton's space and
/// were at `before` a step earlier (a bone missing there is where it is
/// now), its meshes moved by `deforms`.
///
/// Each bone that carries something gives the box between its two places,
/// widened by its reach (and the most any deform offset of either step
/// moves a mesh vertex, where one is given in its space) times the largest
/// stretch it has at either step. That stretch is taken as the length of
/// its two axes' longest lengths together, which no turn, scale or shear
/// of the bone can pass, and which the blend between the two steps
/// (`draw::blend`) keeps within: a place along a line between the two, and
/// axes of lengths between their two lengths. A weighted mesh vertex is a
/// weighted mean, with weights of 0 or more, of points each inside its
/// influence's bone's box, so it is inside the box too.
///
/// It runs after every posing of every skeleton, so it visits only the
/// bones that carry something, takes one square root a bone, and reads the
/// deform offsets only where a mesh could be moved by them.
///
/// A bone at no number adds nothing: what it carries is drawn nowhere
pub(crate) fn pose_bounds(
  reaches: &BoneReaches,
  now: &[Affine2],
  before: &[Affine2],
  deforms: &[SlotDeform],
) -> Bounds {
  let moved = if reaches.meshed {
    let lists = deforms
      .iter()
      .flat_map(|deform| [&deform.offsets[..], &deform.previous[..]]);
    lists
      .flat_map(|list| list.as_chunks::<2>().0)
      .map(|&[x, y]| x * x + y * y)
      .fold(0.0, f32::max)
      .sqrt()
  } else {
    0.0
  };
  let mut bounds = Bounds {
    min: Vec2::INFINITY,
    max: Vec2::NEG_INFINITY,
    texel: 0.0,
  };
  for &(bone, reach) in &reaches.bones {
    let Some(now) = now.get(bone) else {
      continue;
    };
    let before = before.get(bone).unwrap_or(now);
    let (a, b) = (now.matrix2, before.matrix2);
    let x = a.x_axis.length_squared().max(b.x_axis.length_squared());
    let y = a.y_axis.length_squared().max(b.y_axis.length_squared());
    let stretch = (x + y).sqrt();
    let reach_by = if reach.meshed { moved } else { 0.0 };
    let distance = Vec2::splat((reach.reach + reach_by) * stretch);
    let (low, high) = (
      now.translation.min(before.translation) - distance,
      now.translation.max(before.translation) + distance,
    );
    // `min` and `max` pass over a number that is not one: a bone at none
    // leaves the box as it was
    bounds.min = bounds.min.min(low);
    bounds.max = bounds.max.max(high);
    bounds.texel = bounds.texel.max(reach.texel * stretch);
  }
  bounds
}
