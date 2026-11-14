//! Drawing a skeleton: where its entity puts it this frame, and what its
//! slots show there, its bones between their last two steps: a quad for
//! each region, and each mesh's vertices skinned to the bones that hold
//! them. The entity drawer (`render/entities.rs`) turns the parts into
//! draw items.

use std::ops::Range;

use glam::{Affine2, Mat2, Vec2, Vec4};

use super::attachment::{Attachment, MeshAttachment, MeshVertices, RegionAttachment};
use super::component::Skeleton;
use super::data::{BoneId, DataVersion, SkeletonData, SlotBlend, SlotId};
use crate::core::asset::AssetId;
use crate::core::ecs::interpolate::render_global;
use crate::core::ecs::{GlobalTransform, PreviousGlobalTransform, YSort};
use crate::core::graphics::Image;
use crate::core::render::scratch::recycle;
use crate::core::ui::Color;

/// Below this, a bone's x axis has no direction to turn by
const FLAT: f32 = 1e-6;

#[cfg(test)]
thread_local! {
  /// How many times this thread's tests have asked where a bone draws
  static BONES_DRAWN: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Where a skeleton draws this frame
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Drawn {
  /// Takes the skeleton's own space to the world: its entity's transform as
  /// the frame draws it, mirrored as the skeleton is flipped
  pub(crate) root: Affine2,
  /// Where its entity draws, lifted on a floor
  pub(crate) position: Vec2,
  /// The way its entity is turned, in radians
  pub(crate) rotation: f32,
  /// How far between their last two steps its bones draw, or `None` where
  /// they are
  pub(crate) between: Option<f32>,
}

/// The entity that carries a skeleton, as a frame places it
#[derive(Debug, Clone, Copy)]
pub(crate) struct Carrier<'w> {
  pub(crate) global: &'w GlobalTransform,
  pub(crate) previous: Option<&'w PreviousGlobalTransform>,
  /// How far up it draws from where it stands (on a floor)
  pub(crate) lift: Vec2,
  pub(crate) y_sort: Option<&'w YSort>,
}

impl Skeleton {
  /// Where the skeleton draws in a frame `alpha` of the way from the last
  /// fixed step to the next, `step` being that step's number.
  ///
  /// Its entity's transform is the one a sprite's is drawn by: between its
  /// last two steps for an interpolated entity (`render_global`), lifted by
  /// its floor, and scaled with its depth. So a skeleton is where its
  /// entity is in every frame, also one in which the entity moved and no
  /// step posed the bones again. All of the transform carries the bones: a
  /// skew or a mirroring a sprite would not show.
  ///
  /// Its bones draw between their last two steps when its entity does (it
  /// has a `PreviousGlobalTransform` that does not skip) and the skeleton
  /// took the last step; else where they are: an entity that is not
  /// interpolated, one that teleported, and a skeleton held, disabled or
  /// waiting while the last step ran, whose pose would otherwise sway with
  /// the frame's fraction
  pub(crate) fn drawn(&self, carrier: &Carrier<'_>, alpha: f32, step: Option<u64>) -> Drawn {
    let drawn = render_global(carrier.global, carrier.previous, alpha);
    // Its depth scale goes by where it stands
    let ground = drawn.translation();
    let position = ground + carrier.lift;
    let mut root = drawn.affine();
    root.translation = position;
    let depth = carrier
      .y_sort
      .map_or((1.0, 1.0), |y_sort| y_sort.calculate_depth_scale(ground.y));
    if depth != (1.0, 1.0) {
      root.matrix2 *= Mat2::from_diagonal(Vec2::new(depth.0, depth.1));
    }
    let moving = carrier.previous.is_some_and(|previous| !previous.skip);
    let stepped = step.is_none_or(|step| self.stepped() == step);
    Drawn {
      root: self.flip().under(root),
      position,
      rotation: drawn.rotation(),
      between: (moving && stepped).then_some(alpha),
    }
  }

  /// Bone `bone`'s transform in the skeleton's own space as a frame draws
  /// it: `between` of the way from where the step before the last left it
  /// to where the last one did, or where it is
  pub(crate) fn drawn_bone(&self, bone: usize, between: Option<f32>) -> Option<Affine2> {
    #[cfg(test)]
    BONES_DRAWN.with(|drawn| drawn.set(drawn.get() + 1));
    let now = self.pose.skeleton_space.get(bone)?;
    Some(match (between, self.previous_space.get(bone)) {
      (Some(t), Some(before)) => blend(before, now, t),
      _ => *now,
    })
  }

  /// Slot `slot`'s deform offsets for `mesh`, the mesh it shows, as a frame
  /// draws them: `between` of the way from the step before the last's to
  /// the last's, or the last's.
  ///
  /// The lists were filled by a step, for the mesh the slot showed then,
  /// and a frame may find another there (a skin put on since, which shows
  /// at once), so each is taken only if it holds an x and a y for each of
  /// this mesh's positions. An empty one is the setup shape. This step's
  /// of another length moves nothing; the step before's of another length
  /// (another mesh's) is not drawn from, and this step's is drawn as it is
  fn drawn_deform(&self, slot: SlotId, mesh: &MeshAttachment, between: Option<f32>) -> Offsets<'_> {
    let Some(deform) = self.pose.deforms.get(slot.index()) else {
      return Offsets::NONE;
    };
    let width = mesh.positions() * 2;
    let (now, before) = (&deform.offsets[..], &deform.previous[..]);
    if !now.is_empty() && now.len() != width {
      return Offsets::NONE;
    }
    match between {
      Some(t) if before.is_empty() || before.len() == width => Offsets {
        from: before,
        to: now,
        t,
      },
      _ => Offsets {
        from: now,
        to: now,
        t: 1.0,
      },
    }
  }

  /// Fills `out` with what the skeleton's slots show, back to front in its
  /// draw order, under `root` (the skeleton's space to the world) with its
  /// bones `between` their last two steps: a region as a quad on its
  /// slot's bone, and a mesh as its vertices where the bones that hold
  /// them put them ([`Parts::vertices`]), each moved first by what the
  /// animations' deform keys give it. What `out` held is forgotten.
  ///
  /// Nothing for a skeleton with no pose, or whose pose was made from
  /// another version of its data than `seen`, the one `data` is: its slots
  /// and bones are not this data's. A slot showing nothing, a point or a
  /// bounding box, or a name its skins lack, adds nothing
  pub(crate) fn parts<'s>(
    &'s self,
    data: &SkeletonData,
    seen: DataVersion,
    root: Affine2,
    between: Option<f32>,
    out: &mut Parts<'s>,
  ) {
    out.clear();
    if self.seen() != Some(seen) {
      return;
    }
    let pose = &self.pose;
    for &slot in &pose.draw_order {
      let shown = pose.slot_attachments.get(slot.index());
      let Some(name) = shown.and_then(Option::as_deref) else {
        continue;
      };
      let Some(slot_data) = data.slots().get(slot.index()) else {
        continue;
      };
      let slot_color = || pose.slot_colors.get(slot.index()).copied();
      match self.skin_attachment(slot, name) {
        Some(Attachment::Region(shown)) => {
          let Some(region) = data.regions().get(shown.region) else {
            continue;
          };
          let Some(bone) = self.drawn_bone(slot_data.bone.index(), between) else {
            continue;
          };
          out.list.push(Part::Region(RegionQuad {
            image: region.image.id(),
            rect: [region.x, region.y, region.width, region.height],
            blend: slot_data.blend,
            quad: root * bone * local_quad(shown),
            color: tinted(slot_color().unwrap_or(Vec4::ONE), shown.color),
          }));
        }
        Some(Attachment::Mesh(mesh)) => {
          let Some(region) = data.regions().get(mesh.region) else {
            continue;
          };
          let deform = self.drawn_deform(slot, mesh, between);
          let Some(vertices) = out.skin(self, mesh, slot_data.bone, deform, root, between) else {
            continue;
          };
          out.list.push(Part::Mesh(MeshPart {
            image: region.image.id(),
            rect: [region.x, region.y, region.width, region.height],
            blend: slot_data.blend,
            color: tinted(slot_color().unwrap_or(Vec4::ONE), mesh.color),
            vertices,
            mesh,
          }));
        }
        _ => {}
      }
    }
  }
}

/// What a skeleton draws in a frame: its parts back to front, and the
/// buffers its meshes are skinned into. A drawer keeps one from frame to
/// frame ([`Parts::recycle`]) and fills it for each skeleton in turn
/// ([`Skeleton::parts`]), so a steady frame allocates nothing
#[derive(Debug, Default)]
pub(crate) struct Parts<'s> {
  /// The regions and the meshes the slots show, in the draw order
  pub(crate) list: Vec<Part<'s>>,
  /// Every mesh's vertices where they draw, in the world: each mesh's are
  /// a stretch of it
  positions: Vec<Vec2>,
  /// Each bone a mesh has used, as it draws in the world. A bone is
  /// placed (blended between its last two steps, then put under the root)
  /// once a skeleton, however many vertices it holds; `None` for one no
  /// mesh has used yet
  bones: Vec<Option<Affine2>>,
}

impl<'s> Parts<'s> {
  /// The buffers for skeletons borrowed for another lifetime: the list of
  /// parts emptied, since its parts borrow the last skeletons, and the
  /// positions and bones as the last skeleton left them, to be forgotten
  /// when the next is walked ([`Skeleton::parts`] clears all three). Kept
  /// as `Parts<'static>` between frames, their allocations are the next
  /// frame's
  pub(crate) fn recycle<'t>(self) -> Parts<'t> {
    Parts {
      list: recycle(self.list),
      positions: self.positions,
      bones: self.bones,
    }
  }

  /// Where `mesh`'s vertices draw, in the world, in the order of its
  /// attachment's vertices
  pub(crate) fn vertices(&self, mesh: &MeshPart<'_>) -> &[Vec2] {
    self.positions.get(mesh.vertices.clone()).unwrap_or(&[])
  }

  /// Forgets the last skeleton's parts, keeping the allocations
  fn clear(&mut self) {
    self.list.clear();
    self.positions.clear();
    self.bones.clear();
  }

  /// Skins `mesh`: adds each of its vertices where it draws, in the world,
  /// and gives where they went in the positions.
  ///
  /// An unweighted vertex is a place in the space of `slot_bone`, the bone
  /// of the slot that shows the mesh. A weighted vertex is the sum, over
  /// its influences, of where each one's bone puts the place given in that
  /// bone's space, times the influence's weight (the weights of a vertex
  /// sum to 1 in data that built). Each place is first moved by its offset
  /// in `deform` (the mesh's positions in order: a vertex each, or an
  /// influence each), in the space it is given in, so a keyed shape turns
  /// with the bone it rides. The bones are `skeleton`'s as the frame draws
  /// them, `between` their last two steps, under `root`.
  ///
  /// `None`, with nothing added, when the pose lacks a bone the mesh
  /// names: a pose made for other data. A vertex can come out at no finite
  /// place (a root or a bone that is not finite); the drawer draws no such
  /// mesh.
  ///
  /// Never inlined: inside the walk it slows the loop over a skeleton's
  /// regions, which most skeletons are made of
  /// (`docs/backlog/skeleton.md`, Meshes)
  #[inline(never)]
  fn skin(
    &mut self,
    skeleton: &Skeleton,
    mesh: &MeshAttachment,
    slot_bone: BoneId,
    deform: Offsets<'_>,
    root: Affine2,
    between: Option<f32>,
  ) -> Option<Range<usize>> {
    let first = self.positions.len();
    // A mesh no deform moves is placed with no offsets looked up
    let moved = !deform.none();
    let whole = match &mesh.vertices {
      MeshVertices::Unweighted(vertices) => {
        self.placed(skeleton, slot_bone, root, between).map(|bone| {
          if moved {
            let offset = vertices.iter().enumerate();
            let placed = offset.map(|(at, &vertex)| bone.transform_point2(vertex + deform.at(at)));
            self.positions.extend(placed);
          } else {
            let placed = vertices.iter().map(|&vertex| bone.transform_point2(vertex));
            self.positions.extend(placed);
          }
        })
      }
      MeshVertices::Weighted(weighted) => {
        // Its positions are its influences, in the order they are held
        let mut position = 0;
        weighted.each().try_for_each(|influences| {
          let mut at = Vec2::ZERO;
          for influence in influences {
            let bone = self.placed(skeleton, influence.bone, root, between)?;
            let mut held = Vec2::new(influence.x, influence.y);
            if moved {
              held += deform.at(position);
            }
            position += 1;
            at += bone.transform_point2(held) * influence.weight;
          }
          self.positions.push(at);
          Some(())
        })
      }
    };
    if whole.is_none() {
      self.positions.truncate(first);
      return None;
    }
    Some(first..self.positions.len())
  }

  /// Bone `bone` of `skeleton` as it draws in the world: `between` its
  /// last two steps, under `root`. Worked out the first time a mesh of the
  /// skeleton asks and kept for the rest of its meshes, so a bone that
  /// holds a hundred vertices is blended once
  fn placed(
    &mut self,
    skeleton: &Skeleton,
    bone: BoneId,
    root: Affine2,
    between: Option<f32>,
  ) -> Option<Affine2> {
    let index = bone.index();
    if let Some(Some(placed)) = self.bones.get(index) {
      return Some(*placed);
    }
    let placed = root * skeleton.drawn_bone(index, between)?;
    // The pose has the bone, so this grows to its bone count at most
    if self.bones.len() <= index {
      self.bones.resize(index + 1, None);
    }
    if let Some(kept) = self.bones.get_mut(index) {
      *kept = Some(placed);
    }
    Some(placed)
  }
}

/// A mesh's deform offsets as a frame draws them: `t` of the way from one
/// list to another, each an x and a y for each of the mesh's positions, or
/// empty for none
#[derive(Debug, Clone, Copy)]
struct Offsets<'s> {
  from: &'s [f32],
  to: &'s [f32],
  t: f32,
}

impl Offsets<'_> {
  /// No offsets: the setup shape
  const NONE: Offsets<'static> = Offsets {
    from: &[],
    to: &[],
    t: 1.0,
  };

  /// Whether no position is moved
  fn none(&self) -> bool {
    self.from.is_empty() && self.to.is_empty()
  }

  /// Position `index`'s offset; nothing for one a list does not reach
  fn at(&self, index: usize) -> Vec2 {
    let of = |list: &[f32]| match list.get(index * 2..index * 2 + 2) {
      Some(&[x, y]) => Vec2::new(x, y),
      _ => Vec2::ZERO,
    };
    let (from, to) = (of(self.from), of(self.to));
    // Exact where the two are the same
    from + (to - from) * self.t
  }
}

/// One part of a skeleton as a frame draws it
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Part<'s> {
  Region(RegionQuad),
  Mesh(MeshPart<'s>),
}

/// One region as a skeleton draws it
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct RegionQuad {
  pub(crate) image: AssetId<Image>,
  /// Its rectangle of the image, in pixels: left, top, width, height
  pub(crate) rect: [u32; 4],
  /// Its slot's blend
  pub(crate) blend: SlotBlend,
  /// Takes the unit quad (-0.5 to 0.5 on each axis) to where it draws: its
  /// x axis spans the region's width, its y axis its height, and its
  /// translation is the region's centre
  pub(crate) quad: Affine2,
  /// The slot's colour times the attachment's: sRGB with straight alpha,
  /// each within 0 to 1
  pub(crate) color: [f32; 4],
}

impl RegionQuad {
  /// Its four corners, in order round it
  pub(crate) fn corners(&self) -> [Vec2; 4] {
    [(-0.5, -0.5), (0.5, -0.5), (0.5, 0.5), (-0.5, 0.5)]
      .map(|(x, y)| self.quad.transform_point2(Vec2::new(x, y)))
  }
}

/// One mesh as a skeleton draws it. Its vertices' places are in the
/// [`Parts`] it was added to
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct MeshPart<'s> {
  pub(crate) image: AssetId<Image>,
  /// Its region's rectangle of the image, in pixels: left, top, width,
  /// height. Its vertices' UVs run across it
  pub(crate) rect: [u32; 4],
  /// Its slot's blend
  pub(crate) blend: SlotBlend,
  /// The slot's colour times the attachment's: sRGB with straight alpha,
  /// each within 0 to 1
  pub(crate) color: [f32; 4],
  /// Where its vertices are in the positions of its [`Parts`]
  vertices: Range<usize>,
  /// The attachment: a UV for each vertex, the triangles and the hull
  pub(crate) mesh: &'s MeshAttachment,
}

/// A region attachment's quad in its bone's space: the unit quad scaled to
/// its size, turned, and moved to its offset
fn local_quad(shown: &RegionAttachment) -> Affine2 {
  let (sin, cos) = shown.rotation.sin_cos();
  let size = Vec2::new(shown.scale_x * shown.width, shown.scale_y * shown.height);
  Affine2::from_cols(
    Vec2::new(cos, sin) * size.x,
    Vec2::new(-sin, cos) * size.y,
    Vec2::new(shown.x, shown.y),
  )
}

/// A slot's colour (float sRGB, which a curve can take past 0 to 1) times
/// its attachment's: each component clamped to 0 to 1 first, one that is
/// not a number counting as 0
fn tinted(slot: Vec4, attachment: Color) -> [f32; 4] {
  let unit = |c: f32| if c.is_nan() { 0.0 } else { c.clamp(0.0, 1.0) };
  let attachment = [attachment.r, attachment.g, attachment.b, attachment.a];
  let slot = slot.to_array();
  std::array::from_fn(|c| unit(slot[c]) * (f32::from(attachment[c]) / 255.0))
}

/// A bone's transform `t` of the way from `before` to `now`.
///
/// As `GlobalTransform::lerp` blends an entity's: its place along a line,
/// its direction (its x axis's) the short way round, and its lengths
/// evenly. Besides what an entity's transform holds, a bone's can be
/// skewed or mirrored, and both are kept as it turns: its y axis is blended
/// in the frame its x axis points along. What did not change stays exact,
/// so a still bone draws exactly where it is.
///
/// Where its x axis has no length to take a direction from, or the bone is
/// mirrored in one and not in the other (a scale keyed through zero), each
/// of its numbers is blended on its own: the bone flattens and opens again
/// the other way, with no turn
pub(crate) fn blend(before: &Affine2, now: &Affine2, t: f32) -> Affine2 {
  if before == now {
    return *now;
  }
  if t <= 0.0 {
    return *before;
  }
  // `a + (b - a) * t` is exact where a component did not change
  let mix = |a: Vec2, b: Vec2| a + (b - a) * t;
  let translation = mix(before.translation, now.translation);
  let (a, b) = (before.matrix2, now.matrix2);
  if a == b {
    return Affine2 {
      matrix2: b,
      translation,
    };
  }
  let (length_a, length_b) = (a.x_axis.length(), b.x_axis.length());
  let mirrored = (a.determinant() < 0.0) != (b.determinant() < 0.0);
  // NaN lengths compare false, and blend number by number too
  if mirrored || !(length_a >= FLAT && length_b >= FLAT) {
    return Affine2 {
      matrix2: Mat2::from_cols(mix(a.x_axis, b.x_axis), mix(a.y_axis, b.y_axis)),
      translation,
    };
  }
  // Each as the direction of its x axis, that axis's length, and its y
  // axis in the frame turned to that direction (which holds the bone's
  // height, its skew and its mirroring)
  let (along_a, along_b) = (a.x_axis / length_a, b.x_axis / length_b);
  let in_frame = |along: Vec2, y: Vec2| Vec2::new(along.dot(y), along.perp_dot(y));
  let (y_a, y_b) = (in_frame(along_a, a.y_axis), in_frame(along_b, b.y_axis));
  // The short way round from one direction to the other
  let turn = along_a.angle_to(along_b);
  let along = Vec2::from_angle(turn * t).rotate(along_a);
  let length = length_a + (length_b - length_a) * t;
  let y = mix(y_a, y_b);
  Affine2 {
    matrix2: Mat2::from_cols(along * length, along * y.x + along.perp() * y.y),
    translation,
  }
}

#[cfg(test)]
mod tests {
  use std::f32::consts::{FRAC_PI_2, FRAC_PI_4, PI};
  use std::sync::Arc;

  use super::*;
  use crate::core::asset::{Assets, Handle};
  use crate::core::ecs::{Renderable, Transform};
  use crate::core::skeleton::component::{Fired, Flip, Stand};
  use crate::core::skeleton::{
    BoneOverride, BoneSetup, BoundingBoxAttachment, Influence, PointAttachment, Region, SlotSetup,
    WeightedVertices,
  };
  use crate::testing::asset_world;

  fn same(actual: Affine2, expected: Affine2) {
    assert!(
      actual.abs_diff_eq(expected, 1e-5),
      "expected {expected:?}, got {actual:?}"
    );
  }

  fn turned(radians: f32, scale: Vec2, at: Vec2) -> Affine2 {
    Affine2::from_scale_angle_translation(scale, radians, at)
  }

  #[test]
  fn a_still_bone_blends_to_exactly_where_it_is() {
    let bone = turned(0.7, Vec2::new(1.3, 0.9), Vec2::new(432.0, 244.666_67));
    for t in [0.0, 0.077_4, 0.5, 0.999] {
      assert_eq!(blend(&bone, &bone, t), bone, "at {t}");
    }
    // One that only moved keeps its axes exactly, and an axis it did not
    // move along
    let moved = Affine2 {
      translation: Vec2::new(440.0, 244.666_67),
      ..bone
    };
    let half = blend(&bone, &moved, 0.5);
    assert_eq!(half.matrix2, bone.matrix2);
    assert_eq!(half.translation, Vec2::new(436.0, 244.666_67));
    // And none of the way along is where it was
    let other = turned(1.9, Vec2::new(2.0, 1.0), Vec2::new(-3.0, 8.0));
    assert_eq!(blend(&bone, &other, 0.0), bone);
  }

  #[test]
  fn a_turning_bone_blends_along_the_arc_the_short_way_as_an_entity_does() {
    // A quarter turn: half way it has turned an eighth, at full length (a
    // blend of the numbers alone would cut the corner, 0.707 as long)
    let (a, b) = (
      turned(0.0, Vec2::ONE, Vec2::ZERO),
      turned(FRAC_PI_2, Vec2::ONE, Vec2::new(10.0, 0.0)),
    );
    same(
      blend(&a, &b, 0.5),
      turned(FRAC_PI_4, Vec2::ONE, Vec2::new(5.0, 0.0)),
    );
    // Across the half turn it goes the short way: from 170 degrees to -170
    // is 20 degrees on through 180, not 340 back through 0
    let (a, b) = (
      turned(170f32.to_radians(), Vec2::ONE, Vec2::ZERO),
      turned(-170f32.to_radians(), Vec2::ONE, Vec2::ZERO),
    );
    same(blend(&a, &b, 0.5), turned(PI, Vec2::ONE, Vec2::ZERO));
    // Turning, growing and moving at once, it is what an entity's blend is
    let (from, to) = (
      Transform::from_xy(3.0, -4.0)
        .with_rotation(0.4)
        .with_scale(Vec2::new(1.0, 2.0)),
      Transform::from_xy(9.0, 1.0)
        .with_rotation(1.5)
        .with_scale(Vec2::new(3.0, 0.5)),
    );
    let (from, to) = (
      GlobalTransform::from_transform(&from),
      GlobalTransform::from_transform(&to),
    );
    for t in [0.1, 0.5, 0.9] {
      same(
        blend(&from.affine(), &to.affine(), t),
        from.lerp(&to, t).affine(),
      );
    }
  }

  #[test]
  fn a_skewed_or_mirrored_bone_keeps_its_shape_as_it_turns() {
    // A sheared bone (its y axis leans along its x) turned a quarter turn:
    // half way it is the same shape turned an eighth
    let shape = Mat2::from_cols(Vec2::new(2.0, 0.0), Vec2::new(1.0, 1.0));
    let at = |radians: f32| Affine2::from_mat2(Mat2::from_angle(radians) * shape);
    same(
      blend(&at(0.3), &at(0.3 + FRAC_PI_2), 0.5),
      at(0.3 + FRAC_PI_4),
    );
    // A mirrored bone turning stays mirrored, the same size
    let mirror = Mat2::from_cols(Vec2::new(1.0, 0.0), Vec2::new(0.0, -1.0));
    let at = |radians: f32| Affine2::from_mat2(Mat2::from_angle(radians) * mirror);
    let half = blend(&at(-0.5), &at(1.1), 0.5);
    same(half, at(0.3));
    assert!((half.matrix2.determinant() + 1.0).abs() < 1e-5);
  }

  #[test]
  fn a_scale_keyed_through_zero_flattens_the_bone_without_turning_it() {
    // Its x scale goes from 0.5 to -0.5 in one step: half way it is flat
    // along x, as tall as it was, and it never points up or down
    let (a, b) = (
      Affine2::from_scale(Vec2::new(0.5, 1.0)),
      Affine2::from_scale(Vec2::new(-0.5, 1.0)),
    );
    for t in [0.25, 0.5, 0.75] {
      let between = blend(&a, &b, t);
      assert_eq!(between.matrix2.y_axis, Vec2::new(0.0, 1.0), "at {t}");
      assert_eq!(between.matrix2.x_axis, Vec2::new(0.5 - t, 0.0), "at {t}");
    }
    // A bone with no length to take a direction from blends the same way,
    // and one that is not a number gives no panic
    let flat = Affine2::from_scale(Vec2::new(0.0, 1.0));
    let up = turned(FRAC_PI_2, Vec2::ONE, Vec2::ZERO);
    let half = blend(&flat, &up, 0.5);
    assert!(half.matrix2.x_axis.abs_diff_eq(Vec2::new(0.0, 0.5), 1e-6));
    let broken = Affine2::from_scale(Vec2::new(f32::NAN, 1.0));
    assert!(!blend(&broken, &up, 0.5).is_finite());
  }

  /// A rig for the region tests: a root, and an arm at (10, 0) turned a
  /// quarter turn. Slots, back to front: "body" on the root (region 0),
  /// "sleeve" on the arm (region 1, 6 along the bone, turned and scaled),
  /// "hit" (a bounding box), "muzzle" (a point) and "empty" (nothing)
  fn rig(image: &Handle<Image>) -> SkeletonData {
    let shown = |name: &str| SlotSetup {
      attachment: Some(Arc::from(name)),
      ..SlotSetup::default()
    };
    let region = |name: &str, x: u32, width: u32| Region {
      name: Arc::from(name),
      image: image.clone(),
      x,
      y: 2,
      width,
      height: 4,
    };
    let skin = SkeletonData::DEFAULT_SKIN;
    SkeletonData::builder()
      .bone("root", None, BoneSetup::default())
      .bone(
        "arm",
        Some("root"),
        BoneSetup {
          x: 10.0,
          rotation: FRAC_PI_2,
          ..BoneSetup::default()
        },
      )
      .slot("body", "root", shown("body"))
      .slot(
        "sleeve",
        "arm",
        SlotSetup {
          color: Color::new(255, 102, 51, 128),
          blend: SlotBlend::Additive,
          ..shown("sleeve")
        },
      )
      .slot("hit", "root", shown("hit"))
      .slot("muzzle", "arm", shown("muzzle"))
      .slot("empty", "root", SlotSetup::default())
      .region(region("body", 0, 8))
      .region(region("sleeve", 8, 12))
      .attachment(
        skin,
        "body",
        "body",
        Attachment::Region(RegionAttachment::new(0, 8.0, 4.0)),
      )
      .attachment(
        skin,
        "sleeve",
        "sleeve",
        Attachment::Region(RegionAttachment {
          x: 6.0,
          y: 1.0,
          rotation: FRAC_PI_2,
          scale_x: 2.0,
          color: Color::new(102, 255, 51, 204),
          ..RegionAttachment::new(1, 12.0, 4.0)
        }),
      )
      .attachment(
        skin,
        "hit",
        "hit",
        Attachment::BoundingBox(BoundingBoxAttachment {
          vertices: vec![Vec2::ZERO, Vec2::X, Vec2::Y],
        }),
      )
      .attachment(
        skin,
        "muzzle",
        "muzzle",
        Attachment::Point(PointAttachment::default()),
      )
      .build()
      .expect("a valid skeleton")
  }

  /// A skeleton of [`rig`] stepped once where `stand` puts it, after `set`,
  /// with its data and the version its pose was made from
  fn posed(stand: Stand, set: impl FnOnce(&mut Skeleton)) -> (Skeleton, SkeletonData, DataVersion) {
    posed_of(rig, stand, set)
  }

  /// A skeleton of the rig `build` makes, stepped once where `stand` puts
  /// it, after `set`, with its data and the version its pose was made from
  fn posed_of(
    build: fn(&Handle<Image>) -> SkeletonData,
    stand: Stand,
    set: impl FnOnce(&mut Skeleton),
  ) -> (Skeleton, SkeletonData, DataVersion) {
    let mut world = asset_world();
    let image = world
      .resource_mut::<Assets<Image>>()
      .add(Image::solid_rect(20, 8, 255, 255, 255, 255));
    let handle = world
      .resource_mut::<Assets<SkeletonData>>()
      .add(build(&image));
    let assets = world.resource::<Assets<SkeletonData>>();
    let data = assets.get(&handle).expect("the data").clone();
    let seen = (handle.id(), assets.version(&handle).expect("a version"));
    let mut skeleton = Skeleton::new(handle);
    set(&mut skeleton);
    let fired = &mut Fired::default();
    skeleton.step(&data, seen, None, 1.0 / 60.0, stand, fired);
    (skeleton, data, seen)
  }

  /// The regions a skeleton draws under `root`, back to front
  fn quads_under(
    (skeleton, data, seen): (&Skeleton, &SkeletonData, DataVersion),
    root: Affine2,
    between: Option<f32>,
  ) -> Vec<RegionQuad> {
    let mut parts = Parts::default();
    skeleton.parts(data, seen, root, between, &mut parts);
    let regions = parts.list.iter().filter_map(|part| match part {
      Part::Region(quad) => Some(*quad),
      Part::Mesh(_) => None,
    });
    regions.collect()
  }

  fn quads(skeleton: &Skeleton, data: &SkeletonData, seen: DataVersion) -> Vec<RegionQuad> {
    quads_under((skeleton, data, seen), Affine2::IDENTITY, None)
  }

  #[test]
  fn regions_are_quads_on_their_bones_in_draw_order_with_their_colours() {
    let (skeleton, data, seen) = posed(Affine2::IDENTITY.into(), |_| {});
    let drawn = quads(&skeleton, &data, seen);
    // The two region slots, back to front; the bounding box, the point and
    // the empty slot draw nothing
    assert_eq!(drawn.len(), 2);
    let (body, sleeve) = (drawn[0], drawn[1]);
    assert_eq!(body.rect, [0, 2, 8, 4]);
    assert_eq!(sleeve.rect, [8, 2, 12, 4]);
    assert_eq!(body.image, sleeve.image);
    assert_eq!(
      (body.blend, sleeve.blend),
      (SlotBlend::Normal, SlotBlend::Additive)
    );
    // The body: 8 x 4 about the root
    same(
      body.quad,
      Affine2::from_cols(Vec2::new(8.0, 0.0), Vec2::new(0.0, 4.0), Vec2::ZERO),
    );
    assert_eq!(body.color, [1.0; 4]);
    // The sleeve: its bone is at (10, 0) pointing down the screen (its x
    // axis (0, 1), its y axis (-1, 0)), so the attachment's offset (6, 1)
    // is at (10 - 1, 6); turned a quarter turn more its width (12, twice
    // over: 24) runs along the bone's y axis, (-1, 0), and its height (4)
    // back along the bone's x axis
    same(
      sleeve.quad,
      Affine2::from_cols(
        Vec2::new(-24.0, 0.0),
        Vec2::new(0.0, -4.0),
        Vec2::new(9.0, 6.0),
      ),
    );
    let corners = sleeve.corners();
    assert!(corners[0].abs_diff_eq(Vec2::new(21.0, 8.0), 1e-4));
    assert!(corners[2].abs_diff_eq(Vec2::new(-3.0, 4.0), 1e-4));
    // Its slot's colour times its attachment's, component by component:
    // (1, 0.4, 0.2, 0.5) x (0.4, 1, 0.2, 0.8). Blue and alpha are products
    // of two factors that are not 1, so nothing but multiplying gives them
    let slot = [255.0, 102.0, 51.0, 128.0].map(|c: f32| c / 255.0);
    let attachment = [102.0, 255.0, 51.0, 204.0].map(|c: f32| c / 255.0);
    for c in 0..4 {
      let want = slot[c] * attachment[c];
      assert!((sleeve.color[c] - want).abs() < 1e-6, "component {c}");
    }
    assert!((sleeve.color[2] - 0.04).abs() < 1e-6, "0.2 x 0.2");

    // Under a root, every quad is carried by it
    let root = Affine2::from_scale_angle_translation(Vec2::splat(2.0), FRAC_PI_2, Vec2::X);
    let carried = quads_under((&skeleton, &data, seen), root, None);
    assert!(carried[1].quad.abs_diff_eq(root * sleeve.quad, 1e-3));
  }

  #[test]
  fn what_a_game_sets_and_keys_decides_what_is_drawn() {
    // A slot the game hides draws nothing, and the pose's draw order is the
    // order drawn
    let (mut skeleton, data, seen) = posed(Affine2::IDENTITY.into(), |skeleton| {
      skeleton.set_attachment("body", None);
    });
    let drawn = quads(&skeleton, &data, seen);
    assert_eq!(drawn.len(), 1);
    assert_eq!(drawn[0].rect, [8, 2, 12, 4], "the sleeve alone");
    skeleton.pose.slot_attachments[0] = Some(Arc::from("body"));
    skeleton.pose.draw_order.swap(0, 1);
    let drawn = quads(&skeleton, &data, seen);
    assert_eq!(drawn.len(), 2);
    assert_eq!(drawn[0].rect, [8, 2, 12, 4], "the sleeve first");
    // A name no skin has in the slot draws nothing
    skeleton.pose.slot_attachments[0] = Some(Arc::from("missing"));
    assert_eq!(quads(&skeleton, &data, seen).len(), 1);
  }

  #[test]
  fn a_colour_past_its_range_is_clamped_as_it_is_drawn() {
    let (mut skeleton, data, seen) = posed(Affine2::IDENTITY.into(), |_| {});
    // A curve overshot: over 1, under 0, and not a number
    skeleton.pose.slot_colors[0] = Vec4::new(1.4, -0.2, f32::NAN, 0.5);
    let body = quads(&skeleton, &data, seen)[0];
    assert_eq!(body.color, [1.0, 0.0, 0.0, 0.5]);
  }

  #[test]
  fn a_skeleton_with_no_pose_for_the_data_draws_nothing() {
    // Before its data lands it has no pose
    let (skeleton, data, seen) = posed(Affine2::IDENTITY.into(), |_| {});
    let waiting = Skeleton::new(skeleton.data().clone());
    assert!(quads(&waiting, &data, seen).is_empty());
    // And a pose made from an older version of the data than the store
    // holds is not this data's: nothing, and nothing out of range. Not
    // for data with fewer slots than the pose, nor for data its slots and
    // bones would happen to fit
    let newer = (seen.0, seen.1 + 1);
    let shorter = SkeletonData::builder()
      .bone("root", None, BoneSetup::default())
      .build()
      .expect("a valid skeleton");
    assert!(quads(&skeleton, &shorter, newer).is_empty());
    assert!(quads(&skeleton, &data, newer).is_empty());
    assert_eq!(quads(&skeleton, &data, seen).len(), 2);
  }

  /// The arm of a posed [`rig`] turned to `turn` in its next step
  fn turn_arm(skeleton: &mut Skeleton, data: &SkeletonData, seen: DataVersion, turn: f32) {
    skeleton.override_bone(
      "arm",
      BoneOverride {
        rotation: Some(turn),
        ..BoneOverride::default()
      },
    );
    let stand = Stand {
      root: skeleton.root(),
      flip: skeleton.flip(),
    };
    let fired = &mut Fired::default();
    skeleton.step(data, seen, None, 1.0 / 60.0, stand, fired);
  }

  #[test]
  fn bones_draw_between_their_last_two_steps() {
    let (mut skeleton, data, seen) = posed(Affine2::IDENTITY.into(), |_| {});
    // The arm points down (a quarter turn); the next step turns it to
    // point left (a half turn)
    turn_arm(&mut skeleton, &data, seen, PI);
    let arm = |between: Option<f32>| skeleton.drawn_bone(1, between).expect("the arm");
    let at = |radians: f32| turned(radians, Vec2::ONE, Vec2::new(10.0, 0.0));
    same(arm(None), at(PI));
    same(arm(Some(0.0)), at(FRAC_PI_2));
    same(arm(Some(0.5)), at(FRAC_PI_2 + FRAC_PI_4));
    // And its region with it: half way round, the sleeve's centre is where
    // the bone half way round puts it
    let out = quads_under((&skeleton, &data, seen), Affine2::IDENTITY, Some(0.5));
    let centre = at(FRAC_PI_2 + FRAC_PI_4).transform_point2(Vec2::new(6.0, 1.0));
    assert!(out[1].quad.translation.abs_diff_eq(centre, 1e-4));
    // The root did not move: its region is exactly where it is
    assert_eq!(out[0].quad, quads(&skeleton, &data, seen)[0].quad);
  }

  /// A rig for the mesh tests: a root, an arm at (10, 0) turned a quarter
  /// turn, and a hand at (0, 20). Slots, back to front: "body" on the root
  /// (a region), "sleeve" on the arm (a triangle of three unweighted
  /// vertices, its slot and attachment coloured, adding light), "cape" on
  /// the root (four vertices weighted over the arm and the hand) and
  /// "badge" on the hand (a region)
  fn mesh_rig(image: &Handle<Image>) -> SkeletonData {
    let shown = |name: &str| SlotSetup {
      attachment: Some(Arc::from(name)),
      ..SlotSetup::default()
    };
    let region = |name: &str, x: u32, width: u32| Region {
      name: Arc::from(name),
      image: image.clone(),
      x,
      y: 2,
      width,
      height: 4,
    };
    let (arm, hand) = (BoneId(1), BoneId(2));
    let held = Influence::new(hand, 2.0, 2.0, 1.0);
    let sleeve = MeshAttachment {
      color: Color::new(102, 255, 51, 204),
      ..MeshAttachment::new(
        1,
        MeshVertices::Unweighted(vec![Vec2::ZERO, Vec2::new(4.0, 0.0), Vec2::new(0.0, 2.0)]),
        vec![Vec2::ZERO, Vec2::X, Vec2::Y],
        vec![0, 1, 2],
      )
    };
    // The first vertex is three parts the arm's and one the hand's, the
    // last half each; the two between are one bone's alone
    let cape = MeshAttachment {
      hull: 4,
      ..MeshAttachment::new(
        1,
        MeshVertices::Weighted(WeightedVertices::new([
          vec![Influence::new(arm, 4.0, 0.0, 3.0), held],
          vec![held],
          vec![Influence::new(arm, 0.0, 0.0, 1.0)],
          vec![Influence::new(arm, 4.0, 0.0, 1.0), held],
        ])),
        vec![Vec2::ZERO, Vec2::X, Vec2::Y, Vec2::ONE],
        vec![0, 1, 2, 1, 3, 2],
      )
    };
    let skin = SkeletonData::DEFAULT_SKIN;
    let part = || Attachment::Region(RegionAttachment::new(0, 8.0, 4.0));
    SkeletonData::builder()
      .bone("root", None, BoneSetup::default())
      .bone(
        "arm",
        Some("root"),
        BoneSetup {
          x: 10.0,
          rotation: FRAC_PI_2,
          ..BoneSetup::default()
        },
      )
      .bone(
        "hand",
        Some("root"),
        BoneSetup {
          y: 20.0,
          ..BoneSetup::default()
        },
      )
      .slot("body", "root", shown("body"))
      .slot(
        "sleeve",
        "arm",
        SlotSetup {
          color: Color::new(255, 102, 51, 128),
          blend: SlotBlend::Additive,
          ..shown("sleeve")
        },
      )
      .slot("cape", "root", shown("cape"))
      .slot("badge", "hand", shown("badge"))
      .region(region("body", 0, 8))
      .region(region("cloth", 8, 12))
      .attachment(skin, "body", "body", part())
      .attachment(skin, "sleeve", "sleeve", Attachment::Mesh(sleeve))
      .attachment(skin, "cape", "cape", Attachment::Mesh(cape))
      .attachment(skin, "badge", "badge", part())
      .build()
      .expect("a valid skeleton")
  }

  /// What `parts` holds, in order: each region as `None`, each mesh as
  /// where its vertices draw
  fn meshes(parts: &Parts<'_>) -> Vec<Option<Vec<Vec2>>> {
    let each = parts.list.iter().map(|part| match part {
      Part::Region(_) => None,
      Part::Mesh(mesh) => Some(parts.vertices(mesh).to_vec()),
    });
    each.collect()
  }

  fn near(actual: &[Vec2], expected: &[Vec2]) {
    assert_eq!(actual.len(), expected.len(), "{actual:?}");
    for (index, (got, want)) in actual.iter().zip(expected).enumerate() {
      assert!(
        got.abs_diff_eq(*want, 1e-4),
        "vertex {index}: expected {want}, got {got}"
      );
    }
  }

  #[test]
  fn meshes_and_regions_come_in_the_draw_order_each_mesh_skinned_to_its_bones() {
    let (mut skeleton, data, seen) = posed_of(mesh_rig, Affine2::IDENTITY.into(), |_| {});
    let mut parts = Parts::default();
    skeleton.parts(&data, seen, Affine2::IDENTITY, None, &mut parts);
    // A region, the two meshes, a region: one walk, in the slots' order
    let drawn = meshes(&parts);
    assert_eq!(drawn.len(), 4);
    assert!(drawn[0].is_none() && drawn[3].is_none(), "{drawn:?}");
    let (Some(sleeve), Some(cape)) = (&drawn[1], &drawn[2]) else {
      panic!("the meshes are the second and third parts: {drawn:?}");
    };
    // The sleeve's vertices are in its slot's bone's space: the arm is at
    // (10, 0) pointing down the screen (its x axis (0, 1), its y axis
    // (-1, 0)), so (4, 0) is at (10, 4) and (0, 2) at (8, 0)
    let corners = [(10.0, 0.0), (10.0, 4.0), (8.0, 0.0)];
    near(sleeve, &corners.map(|(x, y)| Vec2::new(x, y)));
    // Each of the cape's is the weighted sum of where its bones put it,
    // each in its own space: the arm holds (4, 0) at (10, 4) and the hand,
    // at (0, 20), holds (2, 2) at (2, 22). Three parts and one of those is
    // (8, 8.5); half each is (6, 13). Its slot's bone, the root, plays no
    // part
    let corners = [(8.0, 8.5), (2.0, 22.0), (10.0, 0.0), (6.0, 13.0)];
    near(cape, &corners.map(|(x, y)| Vec2::new(x, y)));

    // What a mesh carries besides: its region's image and rectangle, its
    // slot's blend, and its slot's colour times its attachment's
    let (Part::Mesh(sleeve), Part::Mesh(cape)) = (&parts.list[1], &parts.list[2]) else {
      panic!("the meshes are the second and third parts");
    };
    let Part::Region(body) = &parts.list[0] else {
      panic!("the body is a region");
    };
    assert_eq!(sleeve.image, body.image);
    assert_eq!((sleeve.rect, cape.rect), ([8, 2, 12, 4], [8, 2, 12, 4]));
    assert_eq!(
      (sleeve.blend, cape.blend),
      (SlotBlend::Additive, SlotBlend::Normal)
    );
    let slot = [255.0, 102.0, 51.0, 128.0].map(|c: f32| c / 255.0);
    let attachment = [102.0, 255.0, 51.0, 204.0].map(|c: f32| c / 255.0);
    for c in 0..4 {
      let want = slot[c] * attachment[c];
      assert!((sleeve.color[c] - want).abs() < 1e-6, "component {c}");
    }
    assert_eq!(cape.color, [1.0; 4]);
    assert_eq!((sleeve.mesh.hull, cape.mesh.hull), (0, 4));
    assert_eq!(cape.mesh.triangles, [0, 1, 2, 1, 3, 2]);

    // Under a root every vertex is carried by it
    let root = Affine2::from_scale_angle_translation(Vec2::splat(2.0), FRAC_PI_2, Vec2::X);
    let mut carried = Parts::default();
    skeleton.parts(&data, seen, root, None, &mut carried);
    let under = meshes(&carried);
    let (Some(moved), Some(folded)) = (&under[1], &under[2]) else {
      panic!("the meshes are the second and third parts: {under:?}");
    };
    let carry = |vertices: &[Vec2]| -> Vec<Vec2> {
      let each = vertices.iter().map(|&at| root.transform_point2(at));
      each.collect()
    };
    near(moved, &carry(sleeve_at(&parts)));
    near(folded, &carry(parts.vertices(cape)));

    // The pose's draw order is the order walked, and a slot the game hides
    // adds nothing
    skeleton.pose.draw_order.swap(0, 2);
    skeleton.pose.slot_attachments[1] = None;
    let mut reordered = Parts::default();
    skeleton.parts(&data, seen, Affine2::IDENTITY, None, &mut reordered);
    let kinds: Vec<bool> = meshes(&reordered).iter().map(Option::is_some).collect();
    assert_eq!(kinds, [true, false, false], "the cape, the body, the badge");
  }

  /// Where the sleeve of a walked [`mesh_rig`] draws
  fn sleeve_at<'p>(parts: &'p Parts<'_>) -> &'p [Vec2] {
    match &parts.list[1] {
      Part::Mesh(sleeve) => parts.vertices(sleeve),
      Part::Region(_) => panic!("the sleeve is the second part"),
    }
  }

  #[test]
  fn each_bone_a_mesh_uses_is_placed_once_a_skeleton() {
    let (mut skeleton, data, seen) = posed_of(mesh_rig, Affine2::IDENTITY.into(), |_| {});
    turn_arm(&mut skeleton, &data, seen, PI);
    let drawn = || BONES_DRAWN.with(std::cell::Cell::get);
    let mut parts = Parts::default();
    let before = drawn();
    skeleton.parts(&data, seen, Affine2::IDENTITY, Some(0.5), &mut parts);
    // Each region asks for its slot's bone. The meshes between them use
    // the arm (the sleeve's three vertices, three of the cape's) and the
    // hand (three of the cape's): each is blended between its last two
    // steps once, not once for each vertex it holds
    assert_eq!(drawn() - before, 2 + 2, "two regions, and two bones");
    // And the vertices are where the blended bones put them: the arm half
    // way from a quarter turn to a half turn
    let arm = turned(FRAC_PI_2 + FRAC_PI_4, Vec2::ONE, Vec2::new(10.0, 0.0));
    let corners = [Vec2::ZERO, Vec2::new(4.0, 0.0), Vec2::new(0.0, 2.0)];
    near(
      sleeve_at(&parts),
      &corners.map(|at| arm.transform_point2(at)),
    );

    // The next skeleton's bones are its own: nothing is kept from the last
    // walk but the buffers' room
    let root = Affine2::from_translation(Vec2::new(100.0, 0.0));
    let before = drawn();
    skeleton.parts(&data, seen, root, None, &mut parts);
    assert_eq!(drawn() - before, 2 + 2);
    let arm = root * turned(PI, Vec2::ONE, Vec2::new(10.0, 0.0));
    near(
      sleeve_at(&parts),
      &corners.map(|at| arm.transform_point2(at)),
    );
    assert_eq!(parts.list.len(), 4, "and its parts alone");
  }

  #[test]
  fn walking_a_skeleton_again_takes_no_more_room() {
    let (skeleton, data, seen) = posed_of(mesh_rig, Affine2::IDENTITY.into(), |_| {});
    let mut parts = Parts::default();
    skeleton.parts(&data, seen, Affine2::IDENTITY, None, &mut parts);
    let room = |parts: &Parts<'_>| {
      (
        (parts.list.as_ptr() as usize, parts.list.capacity()),
        (
          parts.positions.as_ptr() as usize,
          parts.positions.capacity(),
        ),
        (parts.bones.as_ptr() as usize, parts.bones.capacity()),
      )
    };
    let first = room(&parts);
    // Kept between frames for skeletons borrowed afresh, the buffers are
    // the same ones, emptied of the last frame's parts
    let mut parts: Parts<'_> = parts.recycle();
    assert!(parts.list.is_empty());
    assert_eq!(room(&parts), first);
    for _ in 0..3 {
      skeleton.parts(&data, seen, Affine2::IDENTITY, None, &mut parts);
      assert_eq!(room(&parts), first);
    }
    assert_eq!(parts.positions.len(), 3 + 4, "one skeleton's vertices");
  }

  #[test]
  fn a_mesh_that_cannot_be_placed_is_left_out_and_nothing_panics() {
    let (mut skeleton, data, seen) = posed_of(mesh_rig, Affine2::IDENTITY.into(), |_| {});
    // Under a root no number describes, every vertex is walked all the
    // same, to a place that is not finite: the drawer refuses such a mesh
    for scale in [f32::NAN, f32::INFINITY, f32::MAX] {
      let root = Affine2::from_scale(Vec2::splat(scale));
      let mut parts = Parts::default();
      skeleton.parts(&data, seen, root, Some(0.5), &mut parts);
      let cape = meshes(&parts)[2].clone().expect("the cape");
      assert_eq!(cape.len(), 4);
      assert!(!cape[1].is_finite(), "scaled by {scale}: {:?}", cape[1]);
    }
    // A pose with fewer bones than a mesh's influences name (which only a
    // pose made for other data could be): the mesh is left out whole, and
    // the vertices added for it taken back
    skeleton.pose.skeleton_space.truncate(2);
    let mut parts = Parts::default();
    skeleton.parts(&data, seen, Affine2::IDENTITY, None, &mut parts);
    let kinds: Vec<bool> = meshes(&parts).iter().map(Option::is_some).collect();
    assert_eq!(kinds, [false, true], "the body and the sleeve");
    assert_eq!(parts.positions.len(), 3, "the sleeve's vertices alone");
    // With no pose for the data nothing is walked, and what the buffers
    // held of the last skeleton is forgotten
    let newer = (seen.0, seen.1 + 1);
    skeleton.parts(&data, newer, Affine2::IDENTITY, None, &mut parts);
    assert!(parts.list.is_empty() && parts.positions.is_empty());
  }

  /// An entity at `global`, which was at `previous` a step ago
  fn carrier<'w>(
    global: &'w GlobalTransform,
    previous: Option<&'w PreviousGlobalTransform>,
  ) -> Carrier<'w> {
    Carrier {
      global,
      previous,
      lift: Vec2::ZERO,
      y_sort: None,
    }
  }

  #[test]
  fn a_skeleton_draws_where_its_entity_draws() {
    let (mut skeleton, ..) = posed(Affine2::IDENTITY.into(), |_| {});
    skeleton.took_step(4);
    let at = |x: f32| GlobalTransform::from_transform(&Transform::from_xy(x, 20.0));
    let (was, now) = (at(100.0), at(140.0));
    // A plain entity: where it is, its bones where they are
    let drawn = skeleton.drawn(&carrier(&now, None), 0.25, Some(4));
    assert_eq!(drawn.root, now.affine());
    assert_eq!(drawn.between, None);
    assert_eq!(drawn.position, Vec2::new(140.0, 20.0));
    // An interpolated one: between its last two steps, and its bones too
    let previous = PreviousGlobalTransform {
      global: was,
      skip: false,
    };
    let drawn = skeleton.drawn(&carrier(&now, Some(&previous)), 0.25, Some(4));
    assert_eq!(drawn.root.translation, Vec2::new(110.0, 20.0));
    assert_eq!(drawn.between, Some(0.25));
    // One that skips (a teleport): where it is, and its bones skip too
    let skipping = PreviousGlobalTransform {
      global: was,
      skip: true,
    };
    let drawn = skeleton.drawn(&carrier(&now, Some(&skipping)), 0.25, Some(4));
    assert_eq!(drawn.root, now.affine());
    assert_eq!(drawn.between, None);
    // A skeleton that did not take the last step (held, disabled) stands
    // still: its bones are where they are, though its entity may move
    let drawn = skeleton.drawn(&carrier(&now, Some(&previous)), 0.25, Some(5));
    assert_eq!(drawn.root.translation, Vec2::new(110.0, 20.0));
    assert_eq!(drawn.between, None);
    // In a world that counts no steps, it is taken to have stepped
    let drawn = skeleton.drawn(&carrier(&now, Some(&previous)), 0.25, None);
    assert_eq!(drawn.between, Some(0.25));
  }

  #[test]
  fn a_skeleton_is_lifted_scaled_with_its_depth_and_flipped_where_it_draws() {
    // An entity at (50, 80) turned a quarter turn, on a floor 32 px up,
    // half size at its depth, its skeleton flipped along its own x
    let stand = Stand {
      root: Affine2::IDENTITY,
      flip: Flip { x: true, y: false },
    };
    let (skeleton, ..) = posed(stand, |_| {});
    let global =
      GlobalTransform::from_transform(&Transform::from_xy(50.0, 80.0).with_rotation(FRAC_PI_2));
    let y_sort = YSort::new().with_depth_scale(0.5, 0.5, 0.0, 100.0);
    let carrier = Carrier {
      global: &global,
      previous: None,
      lift: Vec2::new(0.0, -32.0),
      y_sort: Some(&y_sort),
    };
    let drawn = skeleton.drawn(&carrier, 0.0, None);
    assert_eq!(drawn.position, Vec2::new(50.0, 48.0));
    assert!((drawn.rotation - FRAC_PI_2).abs() < 1e-6);
    // The skeleton's x axis points up the screen (the entity's points
    // down, mirrored), its y axis left, both half length
    same(
      drawn.root,
      Affine2::from_cols(
        Vec2::new(0.0, -0.5),
        Vec2::new(-0.5, 0.0),
        Vec2::new(50.0, 48.0),
      ),
    );
    // A renderable that draws something else flips no skeleton
    let sprite = Renderable::skeleton().with_flip_x(true);
    assert_eq!(Flip::of(Some(&sprite)), Flip { x: true, y: false });
    let mut sprite = sprite;
    sprite.style = crate::core::ecs::RenderStyle::Single;
    assert_eq!(Flip::of(Some(&sprite)), Flip::default());
    assert_eq!(Flip::of(None), Flip::default());
  }

  /// The sleeve's and the cape's vertices of a walked [`mesh_rig`]
  fn walked(
    skeleton: &Skeleton,
    data: &SkeletonData,
    seen: DataVersion,
    between: Option<f32>,
  ) -> [Vec<Vec2>; 2] {
    let mut parts = Parts::default();
    skeleton.parts(data, seen, Affine2::IDENTITY, between, &mut parts);
    let drawn = meshes(&parts);
    let (Some(sleeve), Some(cape)) = (&drawn[1], &drawn[2]) else {
      panic!("the meshes are the second and third parts: {drawn:?}");
    };
    [sleeve.clone(), cape.clone()]
  }

  /// Slot `slot`'s deform offsets of the last step, and of the one before
  fn set_deform(skeleton: &mut Skeleton, slot: usize, now: &[f32], before: &[f32]) {
    let offsets = skeleton.pose.deform_mut(slot).expect("the slot");
    offsets.clear();
    offsets.extend_from_slice(now);
    let previous = &mut skeleton.pose.deforms[slot].previous;
    previous.clear();
    previous.extend_from_slice(before);
  }

  fn points<const N: usize>(at: [(f32, f32); N]) -> [Vec2; N] {
    at.map(|(x, y)| Vec2::new(x, y))
  }

  #[test]
  fn a_deform_moves_each_position_in_its_bone_s_space_before_it_is_weighted() {
    let (mut skeleton, data, seen) = posed_of(mesh_rig, Affine2::IDENTITY.into(), |_| {});
    let [sleeve, cape] = walked(&skeleton, &data, seen, None);
    near(&sleeve, &points([(10.0, 0.0), (10.0, 4.0), (8.0, 0.0)]));
    near(
      &cape,
      &points([(8.0, 8.5), (2.0, 22.0), (10.0, 0.0), (6.0, 13.0)]),
    );
    // The sleeve's vertices are in its slot's bone, the arm, which points
    // down the screen from (10, 0): its second vertex moved 1 along the
    // arm goes 1 px down, and its third moved 3 back across it (the arm's
    // y axis points left) goes 3 px right
    set_deform(&mut skeleton, 1, &[0.0, 0.0, 1.0, 0.0, 0.0, -3.0], &[]);
    let [sleeve, unmoved] = walked(&skeleton, &data, seen, None);
    near(&sleeve, &points([(10.0, 0.0), (10.0, 5.0), (11.0, 0.0)]));
    assert_eq!(unmoved, cape, "each slot's offsets are its own");
    // The cape has a position for each influence, in its vertices' order:
    // the first vertex's two (the arm's, three parts of it; the hand's,
    // one), the second's one, the third's one, and the last's two (half
    // each). An offset moves one influence's place in that influence's
    // bone, and the vertex goes that far times the influence's weight:
    // 4 right in the hand is 1 px right for the first vertex, and 2 down
    // in the hand is 1 px down for the last
    let mut offsets = [0.0; 12];
    offsets[2] = 4.0;
    offsets[11] = 2.0;
    set_deform(&mut skeleton, 2, &offsets, &[]);
    let [_, moved] = walked(&skeleton, &data, seen, None);
    near(
      &moved,
      &points([(9.0, 8.5), (2.0, 22.0), (10.0, 0.0), (6.0, 14.0)]),
    );
    // And in the arm, which points down, 2 along it is 2 px down times the
    // arm's three parts in four
    let mut offsets = [0.0; 12];
    offsets[0] = 2.0;
    set_deform(&mut skeleton, 2, &offsets, &[]);
    let [_, moved] = walked(&skeleton, &data, seen, None);
    near(
      &moved,
      &points([(8.0, 10.0), (2.0, 22.0), (10.0, 0.0), (6.0, 13.0)]),
    );
  }

  #[test]
  fn a_deform_draws_between_its_last_two_steps_when_both_fit_the_mesh() {
    let (mut skeleton, data, seen) = posed_of(mesh_rig, Affine2::IDENTITY.into(), |_| {});
    let [setup, cape] = walked(&skeleton, &data, seen, None);
    // The sleeve's second vertex was 2 along the arm (2 px down) a step
    // ago and is 6 along it now
    let (before, now) = (
      [0.0, 0.0, 2.0, 0.0, 0.0, 0.0],
      [0.0, 0.0, 6.0, 0.0, 0.0, 0.0],
    );
    set_deform(&mut skeleton, 1, &now, &before);
    let second = |skeleton: &Skeleton, between: Option<f32>| {
      let [sleeve, _] = walked(skeleton, &data, seen, between);
      assert_eq!(
        (sleeve[0], sleeve[2]),
        (setup[0], setup[2]),
        "the rest stays"
      );
      sleeve[1]
    };
    // The vertex is `by` px further down the screen than in the setup shape
    let down = |at: Vec2, by: f32| near(&[at], &[Vec2::new(10.0, 4.0 + by)]);
    down(second(&skeleton, None), 6.0);
    down(second(&skeleton, Some(0.0)), 2.0);
    down(second(&skeleton, Some(0.25)), 3.0);
    down(second(&skeleton, Some(1.0)), 6.0);
    // A list with nothing in it is the setup shape: drawn from it, and
    // drawn to it
    set_deform(&mut skeleton, 1, &now, &[]);
    down(second(&skeleton, Some(0.5)), 3.0);
    set_deform(&mut skeleton, 1, &[], &before);
    down(second(&skeleton, Some(0.5)), 1.0);
    down(second(&skeleton, None), 0.0);
    // The step before's offsets were another mesh's (another count): this
    // step's are drawn as they are
    set_deform(&mut skeleton, 1, &now, &[9.0; 4]);
    down(second(&skeleton, Some(0.5)), 6.0);
    set_deform(&mut skeleton, 1, &now, &[9.0; 8]);
    down(second(&skeleton, Some(0.0)), 6.0);
    // This step's offsets are not this mesh's (a skin put on since the
    // step shows a mesh of another count): nothing is moved, whatever the
    // step before left, and no read goes past either list
    for stale in [&[9.0; 4][..], &[9.0; 5], &[9.0; 8], &[9.0; 1]] {
      for before in [&[][..], &before, &[9.0; 4], &[9.0; 8]] {
        set_deform(&mut skeleton, 1, stale, before);
        for between in [None, Some(0.0), Some(0.5), Some(1.0)] {
          let [sleeve, _] = walked(&skeleton, &data, seen, between);
          assert_eq!(sleeve, setup, "{stale:?} after {before:?}, {between:?}");
        }
      }
    }
    // The same for a weighted mesh, whose count is of its influences
    for stale in [&[9.0; 8][..], &[9.0; 11], &[9.0; 14]] {
      set_deform(&mut skeleton, 2, stale, &[9.0; 12]);
      let [_, drawn] = walked(&skeleton, &data, seen, Some(0.5));
      assert_eq!(drawn, cape, "{stale:?}");
    }
    // A pose whose deforms hold fewer slots than the data moves nothing
    skeleton.pose.deforms.truncate(1);
    let [sleeve, drawn] = walked(&skeleton, &data, seen, Some(0.5));
    assert_eq!((sleeve, drawn), (setup, cape));
  }
}
