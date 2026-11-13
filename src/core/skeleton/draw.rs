//! Drawing a skeleton: where its entity puts it this frame, and the quads
//! its slots' regions make there, its bones between their last two steps.
//! The entity drawer (`render/entities.rs`) turns the quads into draw items.

use glam::{Affine2, Mat2, Vec2, Vec4};

use super::attachment::{Attachment, RegionAttachment};
use super::component::Skeleton;
use super::data::{DataVersion, SkeletonData, SlotBlend};
use crate::core::asset::AssetId;
use crate::core::ecs::interpolate::render_global;
use crate::core::ecs::{GlobalTransform, PreviousGlobalTransform, YSort};
use crate::core::graphics::Image;
use crate::core::ui::Color;

/// Below this, a bone's x axis has no direction to turn by
const FLAT: f32 = 1e-6;

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
    let now = self.pose.skeleton_space.get(bone)?;
    Some(match (between, self.previous_space.get(bone)) {
      (Some(t), Some(before)) => blend(before, now, t),
      _ => *now,
    })
  }

  /// Adds to `out` the regions the skeleton's slots show, back to front in
  /// its draw order, each a quad under `root` (the skeleton's space to the
  /// world) with its bone `between` its last two steps.
  ///
  /// Nothing for a skeleton with no pose, or whose pose was made from
  /// another version of its data than `seen`, the one `data` is: its slots
  /// and bones are not this data's. A slot showing nothing, a point or a
  /// bounding box, or a name its skins lack, adds nothing
  pub(crate) fn regions(
    &self,
    data: &SkeletonData,
    seen: DataVersion,
    root: Affine2,
    between: Option<f32>,
    out: &mut Vec<RegionQuad>,
  ) {
    if self.seen() != Some(seen) {
      return;
    }
    let pose = &self.pose;
    for &slot in &pose.draw_order {
      let shown = pose.slot_attachments.get(slot.index());
      let Some(name) = shown.and_then(Option::as_deref) else {
        continue;
      };
      let Some(Attachment::Region(shown)) = self.attachment(slot, name) else {
        continue;
      };
      let (Some(region), Some(slot_data)) = (
        data.regions().get(shown.region),
        data.slots().get(slot.index()),
      ) else {
        continue;
      };
      let Some(bone) = self.drawn_bone(slot_data.bone.index(), between) else {
        continue;
      };
      let slot_color = pose.slot_colors.get(slot.index());
      out.push(RegionQuad {
        image: region.image.id(),
        rect: [region.x, region.y, region.width, region.height],
        blend: slot_data.blend,
        quad: root * bone * local_quad(shown),
        color: tinted(slot_color.copied().unwrap_or(Vec4::ONE), shown.color),
      });
    }
  }
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
    BoneOverride, BoneSetup, BoundingBoxAttachment, PointAttachment, Region, SlotSetup,
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
          color: Color::new(255, 51, 0, 128),
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
          color: Color::new(255, 255, 51, 255),
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
    let mut world = asset_world();
    let image = world
      .resource_mut::<Assets<Image>>()
      .add(Image::solid_rect(20, 8, 255, 255, 255, 255));
    let handle = world
      .resource_mut::<Assets<SkeletonData>>()
      .add(rig(&image));
    let assets = world.resource::<Assets<SkeletonData>>();
    let data = assets.get(&handle).expect("the data").clone();
    let seen = (handle.id(), assets.version(&handle).expect("a version"));
    let mut skeleton = Skeleton::new(handle);
    set(&mut skeleton);
    let fired = &mut Fired::default();
    skeleton.step(&data, seen, None, 1.0 / 60.0, stand, fired);
    (skeleton, data, seen)
  }

  fn quads(skeleton: &Skeleton, data: &SkeletonData, seen: DataVersion) -> Vec<RegionQuad> {
    let mut out = Vec::new();
    skeleton.regions(data, seen, Affine2::IDENTITY, None, &mut out);
    out
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
    // Its slot's colour times its attachment's, component by component
    let expected = [1.0, 51.0 / 255.0, 0.0, 128.0 / 255.0];
    let attachment = [1.0, 1.0, 51.0 / 255.0, 1.0];
    for c in 0..4 {
      let want = expected[c] * attachment[c];
      assert!((sleeve.color[c] - want).abs() < 1e-6, "component {c}");
    }

    // Under a root, every quad is carried by it
    let root = Affine2::from_scale_angle_translation(Vec2::splat(2.0), FRAC_PI_2, Vec2::X);
    let mut carried = Vec::new();
    skeleton.regions(&data, seen, root, None, &mut carried);
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
    // holds is not this data's: nothing, and nothing out of range
    let newer = (seen.0, seen.1 + 1);
    let shorter = SkeletonData::builder()
      .bone("root", None, BoneSetup::default())
      .build()
      .expect("a valid skeleton");
    assert!(quads(&skeleton, &shorter, newer).is_empty());
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
    let mut out = Vec::new();
    skeleton.regions(&data, seen, Affine2::IDENTITY, Some(0.5), &mut out);
    let centre = at(FRAC_PI_2 + FRAC_PI_4).transform_point2(Vec2::new(6.0, 1.0));
    assert!(out[1].quad.translation.abs_diff_eq(centre, 1e-4));
    // The root did not move: its region is exactly where it is
    assert_eq!(out[0].quad, quads(&skeleton, &data, seen)[0].quad);
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
}
