//! The `debug:skeletons` overlay: every skeleton's bones and joints, the
//! outlines of the regions its slots show, the triangles and hulls of
//! their meshes, its bounding boxes and points, and its constraints' marks
//! (each target, and the lines that reach for it), where the frame draws
//! the skeleton.

use glam::Vec2;

use super::attachment::Attachment;
use super::component::Skeleton;
use super::constraint::Constraint;
use super::data::{SkeletonData, SlotId};
use super::draw::{Carrier, Part, Parts};
use super::scope::SkeletonStore;
use crate::core::asset::Assets;
use crate::core::diagnostics::colors;
use crate::core::ecs::{DrawLift, GlobalTransform, PreviousGlobalTransform, World, YSort};
use crate::core::graphics::RenderLayers;
use crate::core::render::DebugDraw;
use crate::core::ui::Color;

/// A joint's radius, a point's cross from end to end and the length of the
/// line that shows which way a point faces, in the main camera's pixels
const JOINT_RADIUS: f32 = 2.0;
const POINT_SIZE: f32 = 7.0;
const POINT_REACH: f32 = 8.0;

/// What the overlay reads of a skeleton's entity
type Shown = (
  &'static Skeleton,
  &'static GlobalTransform,
  Option<&'static PreviousGlobalTransform>,
  Option<&'static DrawLift>,
  Option<&'static YSort>,
  Option<&'static RenderLayers>,
);

/// Adds every posed skeleton's overlay to `debug` for this frame, for the
/// cameras on its entity's layers: each bone as a line its length along
/// it, with a ring at its origin (its joint); the outline of each region
/// drawn; each mesh drawn, as its triangles with its hull over them (the
/// first vertices, as many as its `hull` says, joined in order and back to
/// the first); each bounding box its slots show; each point as a cross
/// with a line the way it faces; and each constraint's target, an IK
/// constraint's crossed and reached for from its last bone's tip, a
/// transform constraint's joined to each of its bones.
///
/// All of it is where the frame draws the skeleton (`alpha` of the way
/// from the last fixed step to the next, lifted on a floor, flipped), so
/// the lines lie on the art, where the engine's other overlays show where
/// things stand. `pixel` is the size of one of the main camera's pixels in
/// the world: joints and points are sized in them
pub(crate) fn draw(world: &mut World, alpha: f32, pixel: f32, debug: &mut DebugDraw) {
  let mut skeletons = world.query::<Shown>();
  let world: &World = world;
  let Some(rigs) = world.get_resource::<Assets<SkeletonData>>() else {
    return;
  };
  let step = world
    .get_resource::<SkeletonStore>()
    .map(SkeletonStore::step);
  let color = |c: [u8; 4]| Color::new(c[0], c[1], c[2], c[3]);
  let mut parts = Parts::default();
  let mut corners: Vec<Vec2> = Vec::new();
  for (skeleton, global, previous, lift, y_sort, layers) in skeletons.iter(world) {
    let id = skeleton.data().id();
    let (Some(data), Some(version)) = (rigs.get(id), rigs.version(id)) else {
      continue;
    };
    // One with no pose for the data as it is has nothing to show
    if skeleton.seen() != Some((id, version)) {
      continue;
    }
    let carrier = Carrier {
      global,
      previous,
      lift: lift.map_or(Vec2::ZERO, |drawn| drawn.lift),
      y_sort,
    };
    let drawn = skeleton.drawn(&carrier, alpha, step);
    let mut pen = debug.layers(layers.copied().unwrap_or_default());

    skeleton.parts(data, (id, version), drawn.root, drawn.between, &mut parts);
    for part in &parts.list {
      let mesh = match part {
        Part::Region(quad) => {
          let [a, b, c, d] = quad.corners();
          pen.polyline(&[a, b, c, d, a], color(colors::SKELETON_REGION));
          continue;
        }
        Part::Mesh(mesh) => mesh,
      };
      // Where the skinning put its vertices: each triangle, then the hull
      // over them
      let vertices = parts.vertices(mesh);
      let corner = |index: u16| vertices.get(usize::from(index)).copied();
      for &[a, b, c] in mesh.mesh.triangles.as_chunks::<3>().0 {
        let (Some(a), Some(b), Some(c)) = (corner(a), corner(b), corner(c)) else {
          continue;
        };
        pen.polyline(&[a, b, c, a], color(colors::SKELETON_MESH));
      }
      let hull = vertices.get(..usize::from(mesh.mesh.hull));
      if let Some(&first) = hull.and_then(<[Vec2]>::first) {
        corners.clear();
        corners.extend_from_slice(hull.unwrap_or_default());
        corners.push(first);
        pen.polyline(&corners, color(colors::SKELETON_REGION));
      }
    }

    // Each constraint's target and what it moves, where the bones are drawn
    let constraint_color = color(colors::SKELETON_CONSTRAINT);
    let placed = |index: usize| {
      let space = skeleton.drawn_bone(index, drawn.between)?;
      Some(drawn.root * space)
    };
    for constraint in data.constraints() {
      // A spring has no target, and no mark until its motion shows
      let Some(target) = constraint.target().and_then(|bone| placed(bone.index())) else {
        continue;
      };
      let goal = target.translation;
      match constraint {
        Constraint::Ik(ik) => {
          let last = ik.bones.last().map(|bone| bone.index());
          let tip = last.and_then(|index| {
            let length = data.bones().get(index)?.length;
            Some(placed(index)?.transform_point2(Vec2::new(length, 0.0)))
          });
          if let Some(tip) = tip {
            pen.line(tip, goal, constraint_color);
            pen.cross(goal, POINT_SIZE * pixel, constraint_color);
          }
        }
        Constraint::Transform(transform) => {
          for bone in &transform.bones {
            if let Some(held) = placed(bone.index()) {
              pen.line(held.translation, goal, constraint_color);
            }
          }
        }
        Constraint::Spring(_) => {}
      }
    }

    for (index, bone) in data.bones().iter().enumerate() {
      let Some(space) = skeleton.drawn_bone(index, drawn.between) else {
        continue;
      };
      let placed = drawn.root * space;
      let origin = placed.translation;
      if bone.length > 0.0 {
        let tip = placed.transform_point2(Vec2::new(bone.length, 0.0));
        pen.line(origin, tip, color(colors::BONE));
      }
      pen.circle(origin, JOINT_RADIUS * pixel, color(colors::BONE_JOINT));
    }

    let slots = skeleton.pose.slot_attachments.iter().zip(data.slots());
    for (slot, (shown, slot_data)) in slots.enumerate() {
      let (Some(name), Ok(slot)) = (shown.as_deref(), u8::try_from(slot)) else {
        continue;
      };
      let Some(space) = skeleton.drawn_bone(slot_data.bone.index(), drawn.between) else {
        continue;
      };
      let placed = drawn.root * space;
      match skeleton.skin_attachment(SlotId(slot), name) {
        Some(Attachment::BoundingBox(bounds)) => {
          corners.clear();
          let vertices = bounds.vertices.iter();
          corners.extend(vertices.map(|&corner| placed.transform_point2(corner)));
          // Closed: back to its first corner
          if let Some(&first) = corners.first() {
            corners.push(first);
          }
          pen.polyline(&corners, color(colors::BOUNDING_BOX));
        }
        Some(Attachment::Point(point)) => {
          let at = placed.transform_point2(Vec2::new(point.x, point.y));
          let faces = (placed.matrix2 * Vec2::from_angle(point.rotation)).normalize_or_zero();
          let point_color = color(colors::SKELETON_POINT);
          pen.cross(at, POINT_SIZE * pixel, point_color);
          pen.line(at, at + faces * POINT_REACH * pixel, point_color);
        }
        _ => {}
      }
    }
  }
}

#[cfg(test)]
mod tests {
  use std::f32::consts::FRAC_PI_2;
  use std::sync::Arc;

  use super::*;
  use crate::core::ecs::{Entity, Transform};
  use crate::core::graphics::Image;
  use crate::core::render::ShapeKind;
  use crate::core::skeleton::component::{Fired, Flip, Stand};
  use crate::core::skeleton::{
    BoneId, BoneOverride, BoneSetup, BoundingBoxAttachment, PointAttachment, Region,
    RegionAttachment, SlotSetup,
  };
  use crate::testing::asset_world;

  /// A world with one skeleton entity at (100, 50): a root 16 long, and
  /// slots showing a region 8 x 4 about it, a bounding box and a point 6
  /// along it facing down the bone's y axis. Posed where it stands,
  /// flipped as `flip` says
  fn world_with_a_skeleton(flip: Flip) -> (World, Entity) {
    let mut world = asset_world();
    let image = world
      .resource_mut::<Assets<Image>>()
      .add(Image::solid_rect(8, 4, 255, 255, 255, 255));
    let shown = |name: &str| SlotSetup {
      attachment: Some(Arc::from(name)),
      ..SlotSetup::default()
    };
    let skin = SkeletonData::DEFAULT_SKIN;
    let root = BoneSetup {
      length: 16.0,
      ..BoneSetup::default()
    };
    let data = SkeletonData::builder()
      .bone("root", None, root)
      .bone("tail", Some("root"), BoneSetup::default())
      .slot("body", "root", shown("body"))
      .slot("hit", "root", shown("hit"))
      .slot("muzzle", "root", shown("muzzle"))
      .region(Region {
        name: Arc::from("body"),
        image,
        x: 0,
        y: 0,
        width: 8,
        height: 4,
      })
      .attachment(
        skin,
        "body",
        "body",
        Attachment::Region(RegionAttachment::new(0, 8.0, 4.0)),
      )
      .attachment(
        skin,
        "hit",
        "hit",
        Attachment::BoundingBox(BoundingBoxAttachment {
          vertices: vec![Vec2::ZERO, Vec2::new(4.0, 0.0), Vec2::new(0.0, 3.0)],
        }),
      )
      .attachment(
        skin,
        "muzzle",
        "muzzle",
        Attachment::Point(PointAttachment {
          x: 6.0,
          y: 0.0,
          rotation: FRAC_PI_2,
        }),
      )
      .build()
      .expect("a valid skeleton");
    let handle = world.resource_mut::<Assets<SkeletonData>>().add(data);
    let assets = world.resource::<Assets<SkeletonData>>();
    let seen = (handle.id(), assets.version(&handle).expect("a version"));
    let data = assets.get(&handle).expect("the data").clone();
    let at = Transform::from_xy(100.0, 50.0);
    let mut skeleton = Skeleton::new(handle);
    let stand = Stand {
      root: at.to_affine(),
      flip,
    };
    let fired = &mut Fired::default();
    skeleton.step(&data, seen, None, 1.0 / 60.0, stand, fired);
    let entity = world.spawn((at, skeleton)).id();
    (world, entity)
  }

  /// The overlay's shapes of one kind, each with its points
  fn shapes(debug: &DebugDraw, kind: ShapeKind) -> Vec<Vec<Vec2>> {
    let of_kind = debug.each().filter(|(shape, _)| shape.kind == kind);
    of_kind.map(|(_, points)| points.to_vec()).collect()
  }

  #[test]
  fn the_overlay_draws_bones_joints_regions_bounding_boxes_and_points() {
    let (mut world, _) = world_with_a_skeleton(Flip::default());
    let mut debug = DebugDraw::default();
    // Half a world unit to a pixel: joints and points are twice as large
    draw(&mut world, 0.0, 0.5, &mut debug);
    let at = Vec2::new(100.0, 50.0);
    // The root's bone, 16 along x from the entity, and the point's facing:
    // 8 px (4 units) down from 6 along the bone
    let lines = shapes(&debug, ShapeKind::Line);
    assert_eq!(lines.len(), 2, "{lines:?}");
    assert_eq!(lines[0], [at, at + Vec2::new(16.0, 0.0)]);
    assert_eq!(lines[1][0], at + Vec2::new(6.0, 0.0));
    assert!(lines[1][1].abs_diff_eq(at + Vec2::new(6.0, 4.0), 1e-5));
    // A joint at each bone's origin (the tail has no length: a joint alone)
    let joints = shapes(&debug, ShapeKind::Circle);
    assert_eq!(joints, vec![vec![at, Vec2::new(1.0, 0.0)]; 2]);
    // The region's outline and the bounding box, each closed
    let outlines = shapes(&debug, ShapeKind::Polyline);
    assert_eq!(outlines.len(), 2);
    let corner = |x: f32, y: f32| at + Vec2::new(x, y);
    assert_eq!(
      outlines[0],
      [
        corner(-4.0, -2.0),
        corner(4.0, -2.0),
        corner(4.0, 2.0),
        corner(-4.0, 2.0),
        corner(-4.0, -2.0)
      ]
    );
    assert_eq!(
      outlines[1],
      [
        corner(0.0, 0.0),
        corner(4.0, 0.0),
        corner(0.0, 3.0),
        corner(0.0, 0.0)
      ]
    );
    // The point's cross
    let crosses = shapes(&debug, ShapeKind::Cross);
    assert_eq!(crosses, [vec![corner(6.0, 0.0), Vec2::new(3.5, 0.0)]]);
  }

  #[test]
  fn the_overlay_draws_a_mesh_s_triangles_and_its_hull_where_it_is_skinned() {
    use crate::core::skeleton::{Influence, MeshAttachment, MeshVertices, WeightedVertices};
    // A skeleton at (100, 50): a root, and a tip 8 px right of it. Its one
    // slot shows a mesh of four vertices in two triangles, its left pair
    // the root's and its right pair the tip's, with a hull of all four
    let mut world = asset_world();
    let image = world
      .resource_mut::<Assets<Image>>()
      .add(Image::solid_rect(8, 4, 255, 255, 255, 255));
    let (root, tip) = (BoneId(0), BoneId(1));
    let corners = [
      vec![Influence::new(root, 0.0, 0.0, 1.0)],
      vec![Influence::new(tip, 0.0, 0.0, 1.0)],
      vec![Influence::new(tip, 0.0, 4.0, 1.0)],
      vec![Influence::new(root, 0.0, 4.0, 1.0)],
    ];
    let cloth = |hull| {
      Attachment::Mesh(MeshAttachment {
        hull,
        ..MeshAttachment::new(
          0,
          MeshVertices::Weighted(WeightedVertices::new(corners.clone())),
          vec![Vec2::ZERO, Vec2::X, Vec2::ONE, Vec2::Y],
          vec![0, 1, 2, 0, 2, 3],
        )
      })
    };
    let shown = SlotSetup {
      attachment: Some(Arc::from("cloth")),
      ..SlotSetup::default()
    };
    let tip_setup = BoneSetup {
      x: 8.0,
      ..BoneSetup::default()
    };
    let data = SkeletonData::builder()
      .bone("root", None, BoneSetup::default())
      .bone("tip", Some("root"), tip_setup)
      .slot("cloth", "root", shown)
      .region(Region {
        name: Arc::from("cloth"),
        image,
        x: 0,
        y: 0,
        width: 8,
        height: 4,
      })
      .skin("bare")
      .attachment(SkeletonData::DEFAULT_SKIN, "cloth", "cloth", cloth(4))
      .attachment("bare", "cloth", "cloth", cloth(0))
      .build()
      .expect("a valid skeleton");
    let handle = world.resource_mut::<Assets<SkeletonData>>().add(data);
    let assets = world.resource::<Assets<SkeletonData>>();
    let seen = (handle.id(), assets.version(&handle).expect("a version"));
    let data = assets.get(&handle).expect("the data").clone();
    let at = Transform::from_xy(100.0, 50.0);
    let mut skeleton = Skeleton::new(handle);
    let fired = &mut Fired::default();
    skeleton.step(&data, seen, None, 1.0 / 60.0, at.to_affine().into(), fired);
    let entity = world.spawn((at, skeleton)).id();

    let outlines = |world: &mut World| {
      let mut debug = DebugDraw::default();
      draw(world, 0.0, 1.0, &mut debug);
      let lines = debug
        .each()
        .filter(|(shape, _)| shape.kind == ShapeKind::Polyline);
      let each = lines.map(|(shape, points)| (shape.color, points.to_vec()));
      each.collect::<Vec<_>>()
    };
    let color = |c: [u8; 4]| Color::new(c[0], c[1], c[2], c[3]);
    let (inner, outer) = (color(colors::SKELETON_MESH), color(colors::SKELETON_REGION));
    let corner = |x: f32, y: f32| Vec2::new(100.0 + x, 50.0 + y);
    let [a, b, c, d] = [
      corner(0.0, 0.0),
      corner(8.0, 0.0),
      corner(8.0, 4.0),
      corner(0.0, 4.0),
    ];
    // Each triangle closed, then the hull over them: its vertices in
    // order and back to the first
    assert_eq!(
      outlines(&mut world),
      [
        (inner, vec![a, b, c, a]),
        (inner, vec![a, c, d, a]),
        (outer, vec![a, b, c, d, a]),
      ]
    );
    // A deform key's offsets move the outlines as they move the mesh: the
    // tip's upper corner 2 px up
    let mut skeleton = world.get_mut::<Skeleton>(entity).expect("the skeleton");
    let offsets = skeleton.pose.deform_mut(0).expect("the slot");
    offsets.extend_from_slice(&[0.0, 0.0, 0.0, -2.0, 0.0, 0.0, 0.0, 0.0]);
    let raised = corner(8.0, -2.0);
    assert_eq!(
      outlines(&mut world),
      [
        (inner, vec![a, raised, c, a]),
        (inner, vec![a, c, d, a]),
        (outer, vec![a, raised, c, d, a]),
      ]
    );
    let mut skeleton = world.get_mut::<Skeleton>(entity).expect("the skeleton");
    skeleton.pose.deforms[0].offsets.clear();
    // A mesh with no hull shows its triangles alone
    skeleton.set_skin("bare");
    assert_eq!(
      outlines(&mut world),
      [(inner, vec![a, b, c, a]), (inner, vec![a, c, d, a])]
    );
  }

  /// A skeleton at (100, 50) with `build`'s constraints over a bone "a"
  /// 10 long, its child "b" 8 along it and a target "goal" at (0, 20),
  /// posed in one step
  fn constrained(
    build: impl FnOnce(
      crate::core::skeleton::SkeletonDataBuilder,
    ) -> crate::core::skeleton::SkeletonDataBuilder,
  ) -> World {
    let mut world = asset_world();
    let long = BoneSetup {
      length: 10.0,
      ..BoneSetup::default()
    };
    let beside = BoneSetup {
      x: 8.0,
      ..BoneSetup::default()
    };
    let goal = BoneSetup {
      y: 20.0,
      ..BoneSetup::default()
    };
    let builder = SkeletonData::builder()
      .bone("a", None, long)
      .bone("b", Some("a"), beside)
      .bone("goal", None, goal);
    let data = build(builder).build().expect("a valid skeleton");
    let handle = world.resource_mut::<Assets<SkeletonData>>().add(data);
    let assets = world.resource::<Assets<SkeletonData>>();
    let seen = (handle.id(), assets.version(&handle).expect("a version"));
    let data = assets.get(&handle).expect("the data").clone();
    let at = Transform::from_xy(100.0, 50.0);
    let mut skeleton = Skeleton::new(handle);
    let fired = &mut Fired::default();
    skeleton.step(&data, seen, None, 1.0 / 60.0, at.to_affine().into(), fired);
    world.spawn((at, skeleton));
    world
  }

  /// The overlay's lines and crosses in the constraints' colour
  fn marks(world: &mut World, kind: ShapeKind) -> Vec<Vec<Vec2>> {
    let mut debug = DebugDraw::default();
    draw(world, 0.0, 1.0, &mut debug);
    let mark = Color::new(
      colors::SKELETON_CONSTRAINT[0],
      colors::SKELETON_CONSTRAINT[1],
      colors::SKELETON_CONSTRAINT[2],
      colors::SKELETON_CONSTRAINT[3],
    );
    let ours = debug
      .each()
      .filter(|(shape, _)| shape.kind == kind && shape.color == mark);
    ours.map(|(_, points)| points.to_vec()).collect()
  }

  #[test]
  fn the_overlay_marks_an_ik_constraint_s_target_and_reach() {
    use crate::core::skeleton::IkSetup;
    // "a" is brought to point at the target 20 below the entity, so its
    // tip, 10 along it, is 10 below: the line runs from there to the
    // target, which is crossed
    let mut world = constrained(|b| b.ik("reach", &["a"], "goal", IkSetup::default()));
    let at = Vec2::new(100.0, 50.0);
    let lines = marks(&mut world, ShapeKind::Line);
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert!(lines[0][0].abs_diff_eq(at + Vec2::new(0.0, 10.0), 1e-3));
    assert!(lines[0][1].abs_diff_eq(at + Vec2::new(0.0, 20.0), 1e-3));
    let crosses = marks(&mut world, ShapeKind::Cross);
    assert_eq!(crosses.len(), 1);
    assert!(crosses[0][0].abs_diff_eq(at + Vec2::new(0.0, 20.0), 1e-3));
  }

  #[test]
  fn the_overlay_links_a_transform_constraint_s_bones_to_its_target() {
    use crate::core::skeleton::TransformSetup;
    let mut world = constrained(|b| {
      b.transform_constraint("copy", &["a", "b"], "goal", TransformSetup::default())
    });
    let at = Vec2::new(100.0, 50.0);
    // From each bone's origin to the target, 20 below the entity
    let lines = marks(&mut world, ShapeKind::Line);
    let goal = at + Vec2::new(0.0, 20.0);
    assert_eq!(
      lines,
      [vec![at, goal], vec![at + Vec2::new(8.0, 0.0), goal],]
    );
    assert!(marks(&mut world, ShapeKind::Cross).is_empty());
  }

  #[test]
  fn the_overlay_marks_nothing_for_a_spring_and_still_marks_the_rest() {
    use crate::core::skeleton::{IkSetup, SpringSetup};
    let swing = SpringSetup {
      rotate: 1.0,
      ..SpringSetup::default()
    };
    // A spring alone: no target, nothing marked
    let mut world = constrained(|b| b.spring("sway", "a", swing));
    assert!(marks(&mut world, ShapeKind::Line).is_empty());
    assert!(marks(&mut world, ShapeKind::Cross).is_empty());
    // Among other constraints, theirs are marked as before
    let mut world = constrained(|b| {
      b.spring("sway", "a", swing)
        .ik("reach", &["a"], "goal", IkSetup::default())
    });
    assert_eq!(marks(&mut world, ShapeKind::Line).len(), 1);
    assert_eq!(marks(&mut world, ShapeKind::Cross).len(), 1);
  }

  /// A world with an entity at (100, 50) showing `data`, posed in one step
  fn spawned(data: SkeletonData) -> (World, Entity) {
    let mut world = asset_world();
    let handle = world.resource_mut::<Assets<SkeletonData>>().add(data);
    let at = Transform::from_xy(100.0, 50.0);
    let entity = world.spawn((at, Skeleton::new(handle))).id();
    step(&mut world, entity);
    (world, entity)
  }

  /// One fixed step of the entity's skeleton, where the entity stands
  fn step(world: &mut World, entity: Entity) {
    let skeleton = world.get::<Skeleton>(entity).expect("the skeleton");
    let handle = skeleton.data().clone();
    let assets = world.resource::<Assets<SkeletonData>>();
    let seen = (handle.id(), assets.version(&handle).expect("a version"));
    let data = assets.get(&handle).expect("the data").clone();
    let root = world.get::<Transform>(entity).expect("placed").to_affine();
    let mut skeleton = world.get_mut::<Skeleton>(entity).expect("the skeleton");
    skeleton.step(
      &data,
      seen,
      None,
      1.0 / 60.0,
      root.into(),
      &mut Fired::default(),
    );
  }

  /// Bones "a" (10 long) and its child "b" (8 along it), and a target
  /// "goal" at (0, 20): an IK constraint turns "a" toward the goal and a
  /// transform constraint (every mix 0, so it moves nothing) names "b"
  fn reaching_and_linked() -> SkeletonData {
    reaching_and_linked_from(false)
  }

  /// [`reaching_and_linked`], with "goal" the first bone when `goal_first`
  /// and the last otherwise
  fn reaching_and_linked_from(goal_first: bool) -> SkeletonData {
    use crate::core::skeleton::{IkSetup, TransformSetup};
    let long = BoneSetup {
      length: 10.0,
      ..BoneSetup::default()
    };
    let beside = BoneSetup {
      x: 8.0,
      ..BoneSetup::default()
    };
    let goal = BoneSetup {
      y: 20.0,
      ..BoneSetup::default()
    };
    let mut builder = SkeletonData::builder();
    if goal_first {
      builder = builder.bone("goal", None, goal);
    }
    builder = builder.bone("a", None, long).bone("b", Some("a"), beside);
    if !goal_first {
      builder = builder.bone("goal", None, goal);
    }
    builder
      .ik("reach", &["a"], "goal", IkSetup::default())
      .transform_constraint("link", &["b"], "goal", TransformSetup::default())
      .build()
      .expect("a valid skeleton")
  }

  /// Every shape of the overlay drawn at `alpha`
  fn drawn_at(world: &mut World, alpha: f32) -> DebugDraw {
    let mut debug = DebugDraw::default();
    draw(world, alpha, 1.0, &mut debug);
    debug
  }

  /// The lines and crosses of `debug` in the constraints' colour
  fn marks_of(debug: &DebugDraw, kind: ShapeKind) -> Vec<Vec<Vec2>> {
    let [r, g, b, a] = colors::SKELETON_CONSTRAINT;
    let mark = Color::new(r, g, b, a);
    let ours = debug
      .each()
      .filter(|(shape, _)| shape.kind == kind && shape.color == mark);
    ours.map(|(_, points)| points.to_vec()).collect()
  }

  #[test]
  fn the_overlay_marks_constraints_where_a_frame_between_steps_draws_the_bones() {
    // The goal moves from (0, 20) to (20, 0) between two steps, so "a"
    // turns from pointing down to pointing right. Drawn half way between
    // the steps, the goal is drawn at (10, 10), "a" half way round, its
    // tip 10 along it at 45 degrees, and "b"'s origin half way from (0, 8)
    // to (8, 0); the last step's pose alone would put them elsewhere
    let (mut world, entity) = spawned(reaching_and_linked());
    let moved = BoneOverride {
      translation: Some(Vec2::new(20.0, 0.0)),
      ..BoneOverride::default()
    };
    let mut skeleton = world.get_mut::<Skeleton>(entity).expect("the skeleton");
    skeleton.override_bone("goal", moved);
    step(&mut world, entity);
    let global = *world.get::<GlobalTransform>(entity).expect("placed");
    world.entity_mut(entity).insert(PreviousGlobalTransform {
      global,
      skip: false,
    });
    let at = Vec2::new(100.0, 50.0);
    let near = |actual: Vec2, expected: Vec2| {
      assert!(
        actual.abs_diff_eq(expected, 1e-3),
        "expected {expected}, got {actual}"
      );
    };

    let debug = drawn_at(&mut world, 0.5);
    let goal = at + Vec2::new(10.0, 10.0);
    let lines = marks_of(&debug, ShapeKind::Line);
    assert_eq!(lines.len(), 2, "{lines:?}");
    // The IK constraint's reach, from the tip to the goal
    let tip = at + Vec2::from_angle(std::f32::consts::FRAC_PI_4) * 10.0;
    near(lines[0][0], tip);
    near(lines[0][1], goal);
    // The transform constraint's line, from "b"'s origin to the goal
    near(lines[1][0], at + Vec2::new(4.0, 4.0));
    near(lines[1][1], goal);
    let crosses = marks_of(&debug, ShapeKind::Cross);
    assert_eq!(crosses.len(), 1);
    near(crosses[0][0], goal);

    // At the end of the step, where the last step left them
    let debug = drawn_at(&mut world, 1.0);
    let lines = marks_of(&debug, ShapeKind::Line);
    near(lines[0][0], at + Vec2::new(10.0, 0.0));
    near(lines[0][1], at + Vec2::new(20.0, 0.0));
    near(lines[1][0], at + Vec2::new(8.0, 0.0));
  }

  #[test]
  fn the_overlay_marks_nothing_for_bones_or_constraints_that_are_gone() {
    use crate::core::skeleton::IkSetup;
    let (mut world, entity) = spawned(reaching_and_linked());
    assert_eq!(
      marks_of(&drawn_at(&mut world, 0.0), ShapeKind::Line).len(),
      2
    );

    // New data without "b" or its transform constraint, before the next
    // step: the skeleton has no pose for it, so nothing is drawn
    let long = BoneSetup {
      length: 10.0,
      ..BoneSetup::default()
    };
    let goal = BoneSetup {
      y: 20.0,
      ..BoneSetup::default()
    };
    let fewer = SkeletonData::builder()
      .bone("a", None, long)
      .bone("goal", None, goal)
      .ik("reach", &["a"], "goal", IkSetup::default())
      .build()
      .expect("a valid skeleton");
    let handle = world
      .get::<Skeleton>(entity)
      .expect("the skeleton")
      .data()
      .clone();
    let mut assets = world.resource_mut::<Assets<SkeletonData>>();
    *assets.get_mut(&handle).expect("the data") = fewer;
    assert_eq!(drawn_at(&mut world, 0.0).each().count(), 0);
    // After a step, only the IK constraint is marked
    step(&mut world, entity);
    let debug = drawn_at(&mut world, 0.0);
    assert_eq!(marks_of(&debug, ShapeKind::Line).len(), 1);
    assert_eq!(marks_of(&debug, ShapeKind::Cross).len(), 1);

    // A pose shorter than its data: every index is looked up, so what
    // names a bone the pose lacks is not marked. Without "goal" neither
    // constraint has a target; without "b" alone the link has no bone
    let (mut world, entity) = spawned(reaching_and_linked());
    let mut skeleton = world.get_mut::<Skeleton>(entity).expect("the skeleton");
    skeleton.pose.skeleton_space.truncate(2);
    skeleton.previous_space.truncate(2);
    let debug = drawn_at(&mut world, 0.0);
    assert!(marks_of(&debug, ShapeKind::Line).is_empty());
    assert!(marks_of(&debug, ShapeKind::Cross).is_empty());
    // The bones it has are drawn: "a" as a line and both as joints
    let bones = Color::new(
      colors::BONE[0],
      colors::BONE[1],
      colors::BONE[2],
      colors::BONE[3],
    );
    let lines = debug.each().filter(|(shape, _)| shape.color == bones);
    assert_eq!(lines.count(), 1);
    assert_eq!(
      debug
        .each()
        .filter(|(shape, _)| shape.kind == ShapeKind::Circle)
        .count(),
      2
    );

    // With "goal" listed first, "b" is the last bone: without it the link
    // has no bone to mark, and the IK constraint still marks its reach
    let (mut world, entity) = spawned(reaching_and_linked_from(true));
    let mut skeleton = world.get_mut::<Skeleton>(entity).expect("the skeleton");
    skeleton.pose.skeleton_space.truncate(2);
    skeleton.previous_space.truncate(2);
    let debug = drawn_at(&mut world, 0.0);
    let lines = marks_of(&debug, ShapeKind::Line);
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert!(lines[0][1].abs_diff_eq(Vec2::new(100.0, 70.0), 1e-3));
    assert_eq!(marks_of(&debug, ShapeKind::Cross).len(), 1);
  }

  #[test]
  fn the_overlay_is_where_the_frame_draws_the_skeleton() {
    // Flipped left to right, and its entity moved since its step: the
    // overlay follows the entity as the regions do, mirrored about it
    let (mut world, entity) = world_with_a_skeleton(Flip { x: true, y: false });
    let moved = Transform::from_xy(200.0, 80.0);
    *world.get_mut::<GlobalTransform>(entity).expect("placed") =
      GlobalTransform::from_transform(&moved);
    let mut debug = DebugDraw::default();
    draw(&mut world, 0.0, 1.0, &mut debug);
    let at = Vec2::new(200.0, 80.0);
    let lines = shapes(&debug, ShapeKind::Line);
    assert_eq!(
      lines[0],
      [at, at + Vec2::new(-16.0, 0.0)],
      "the bone, mirrored"
    );
    let outlines = shapes(&debug, ShapeKind::Polyline);
    assert_eq!(
      outlines[1][1],
      at + Vec2::new(-4.0, 0.0),
      "the box, mirrored"
    );

    // A skeleton with no pose (its data not landed) shows nothing
    let mut world = asset_world();
    let waiting = Skeleton::new(crate::core::asset::Handle::detached());
    world.spawn((Transform::IDENTITY, waiting));
    let mut debug = DebugDraw::default();
    draw(&mut world, 0.0, 1.0, &mut debug);
    assert_eq!(debug.each().count(), 0);
  }
}
