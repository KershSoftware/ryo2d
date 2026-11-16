//! A skeleton is not walked where no camera can see anything it draws, and
//! never culled where any part of it shows

use std::f32::consts::FRAC_PI_2;
use std::sync::Arc;

use glam::{Affine2, Vec2};

use super::{ground_shadow_quads, quad_box, skeleton_reach};
use crate::core::asset::{Assets, Handle};
use crate::core::ecs::{
  BlobShadow, ContactShadow, DropShadow, GlobalTransform, PreviousGlobalTransform, Renderable,
  SpriteEffect, SpriteShader, Transform, YSort,
};
use crate::core::graphics::sprite_gpu::effect_padding;
use crate::core::graphics::{Camera2d, CameraView, Image, SpriteInstance};
use crate::core::skeleton::{
  Animation, Attachment, BoneId, BoneOverride, BoneSetup, BoundingBoxAttachment, Carrier, Curve,
  Drawn, Influence, MeshAttachment, MeshVertices, MixBlend, Part, Parts, PointAttachment, Region,
  RegionAttachment, Skeleton, SkeletonAnimation, SkeletonData, SkeletonDataBuilder, SlotId,
  SlotSetup, Space, Timeline, WeightedVertices,
};
use crate::core::ui::Color;
use crate::testing::asset_world;

const BODY: usize = 0;
const LIMB: usize = 1;
const CLOTH: usize = 2;
const ROOT: BoneId = BoneId(0);
const ARM: BoneId = BoneId(1);
const HAND: BoneId = BoneId(2);
/// The bone 100 along the root in [`flag`]
const FAR: BoneId = BoneId(1);
const CAPE: SlotId = SlotId(2);
const BOOT: SlotId = SlotId(4);

/// A curve that overshoots both ends of its span
const OVERSHOOT: Curve = Curve::Bezier {
  cx1: 0.3,
  cy1: 1.6,
  cx2: 0.7,
  cy2: -0.6,
};

/// A root; an arm 10 along it; a hand 20 along the arm; a leg 10 below the
/// root, turned down. The root shows a region placed by its pivot (larger,
/// turned and scaled in the skin "big"), a bounding box and a point; the
/// arm a limb laid along it by two pixels and a cape weighted over the arm
/// and the hand; the hand a mirrored, turned region; the leg an unweighted
/// mesh. "wave" turns, scales, shears and moves the bones and deforms both
/// meshes past their keys (an overshooting curve), and "pulse" deforms them
/// again, to be added over it
fn rig(image: &Handle<Image>) -> SkeletonData {
  let region = |name: &str, [x, y, width, height]: [u32; 4]| Region {
    name: Arc::from(name),
    image: image.clone(),
    x,
    y,
    width,
    height,
  };
  let shows = |name: &str| SlotSetup {
    attachment: Some(Arc::from(name)),
    ..SlotSetup::default()
  };
  let skin = SkeletonData::DEFAULT_SKIN;
  let body = RegionAttachment {
    pivot: Some(Vec2::new(8.0, 16.0)),
    ..RegionAttachment::new(BODY, 16.0, 16.0)
  };
  let big = RegionAttachment {
    rotation: 0.4,
    scale_x: 2.0,
    scale_y: 1.5,
    pivot: Some(Vec2::new(2.0, 14.0)),
    ..RegionAttachment::new(BODY, 20.0, 20.0)
  };
  let limb = RegionAttachment {
    rotation: 0.2,
    pivot: Some(Vec2::new(0.0, 4.0)),
    along: Some(Vec2::new(16.0, 4.0)),
    ..RegionAttachment::new(LIMB, 16.0, 8.0)
  };
  let hand = RegionAttachment {
    x: 3.0,
    rotation: 0.3,
    scale_x: -1.0,
    pivot: Some(Vec2::new(4.0, 2.0)),
    ..RegionAttachment::new(LIMB, 8.0, 4.0)
  };
  // Each corner pulled by the arm and the hand, given in each one's space
  // (the hand stands 20 along the arm)
  let corner = |x: f32, y: f32, arm: f32| {
    vec![
      Influence::new(ARM, x, y, arm),
      Influence::new(HAND, x - 20.0, y, 1.0 - arm),
    ]
  };
  let cape = MeshAttachment::new(
    CLOTH,
    MeshVertices::Weighted(WeightedVertices::new([
      corner(0.0, -6.0, 1.0),
      corner(30.0, -6.0, 0.2),
      corner(0.0, 6.0, 0.8),
      corner(30.0, 6.0, 0.0),
    ])),
    vec![Vec2::ZERO, Vec2::X, Vec2::Y, Vec2::ONE],
    vec![0, 1, 2, 1, 3, 2],
  );
  let boot = MeshAttachment::new(
    CLOTH,
    MeshVertices::Unweighted(vec![
      Vec2::new(0.0, -4.0),
      Vec2::new(12.0, 0.0),
      Vec2::new(0.0, 4.0),
    ]),
    vec![Vec2::ZERO, Vec2::X, Vec2::Y],
    vec![0, 1, 2],
  );
  let cape_key: Vec<f32> = (0..16).map(|at| [9.0, -7.0, 4.0, 11.0][at % 4]).collect();
  let boot_key = vec![-6.0, 8.0, 10.0, -3.0, 2.0, 9.0];
  let none = |width: usize| vec![0.0; width];
  let deform = |slot: SlotId, name: &str, key: &[f32], curve: Curve| Timeline::Deform {
    slot,
    attachment: Arc::from(name),
    frames: vec![
      (0.0, none(key.len()), curve),
      (0.5, key.to_vec(), curve),
      (1.0, none(key.len()), Curve::Linear),
    ],
  };
  let wave = Animation {
    name: Arc::from("wave"),
    duration: 1.0,
    timelines: vec![
      Timeline::Rotate {
        bone: ARM,
        frames: vec![
          (0.0, 0.0, OVERSHOOT),
          (0.5, 2.2, Curve::Linear),
          (1.0, 0.0, Curve::Linear),
        ],
      },
      Timeline::Scale {
        bone: HAND,
        frames: vec![
          (0.0, Vec2::ONE, OVERSHOOT),
          (0.5, Vec2::new(1.6, 0.5), Curve::Linear),
          (1.0, Vec2::ONE, Curve::Linear),
        ],
      },
      Timeline::Shear {
        bone: HAND,
        frames: vec![
          (0.0, Vec2::ZERO, Curve::Linear),
          (0.5, Vec2::new(0.5, -0.3), Curve::Linear),
          (1.0, Vec2::ZERO, Curve::Linear),
        ],
      },
      Timeline::Translate {
        bone: ROOT,
        frames: vec![
          (0.0, Vec2::ZERO, Curve::Linear),
          (0.5, Vec2::new(6.0, -4.0), Curve::Linear),
          (1.0, Vec2::ZERO, Curve::Linear),
        ],
      },
      deform(CAPE, "cape", &cape_key, OVERSHOOT),
      deform(BOOT, "boot", &boot_key, OVERSHOOT),
    ],
  };
  let pulse = Animation {
    name: Arc::from("pulse"),
    duration: 1.0,
    timelines: vec![
      deform(CAPE, "cape", &cape_key, Curve::Linear),
      deform(BOOT, "boot", &boot_key, Curve::Linear),
    ],
  };
  SkeletonData::builder()
    .bone("root", None, BoneSetup::default())
    .bone(
      "arm",
      Some("root"),
      BoneSetup {
        x: 10.0,
        length: 20.0,
        ..BoneSetup::default()
      },
    )
    .bone(
      "hand",
      Some("arm"),
      BoneSetup {
        x: 20.0,
        ..BoneSetup::default()
      },
    )
    .bone(
      "leg",
      Some("root"),
      BoneSetup {
        y: 10.0,
        rotation: FRAC_PI_2,
        ..BoneSetup::default()
      },
    )
    .slot("body", "root", shows("body"))
    .slot("arm", "arm", shows("arm"))
    .slot("cape", "arm", shows("cape"))
    .slot("hand", "hand", shows("hand"))
    .slot("boot", "leg", shows("boot"))
    .slot("hit", "root", shows("hit"))
    .slot("muzzle", "hand", shows("muzzle"))
    .region(region("body", [0, 0, 16, 16]))
    .region(region("limb", [16, 0, 16, 8]))
    .region(region("cloth", [0, 16, 32, 16]))
    .skin("big")
    .attachment(skin, "body", "body", Attachment::Region(body))
    .attachment("big", "body", "body", Attachment::Region(big))
    .attachment(skin, "arm", "arm", Attachment::Region(limb))
    .attachment(skin, "cape", "cape", Attachment::Mesh(cape))
    .attachment(skin, "hand", "hand", Attachment::Region(hand))
    .attachment(skin, "boot", "boot", Attachment::Mesh(boot))
    .attachment(
      skin,
      "hit",
      "hit",
      Attachment::BoundingBox(BoundingBoxAttachment {
        vertices: vec![Vec2::new(-5.0, -20.0), Vec2::new(5.0, -20.0), Vec2::ZERO],
      }),
    )
    .attachment(
      skin,
      "muzzle",
      "muzzle",
      Attachment::Point(PointAttachment {
        x: 6.0,
        y: 0.0,
        rotation: 0.0,
      }),
    )
    .animation(wave)
    .animation(pulse)
    .build()
    .expect("a valid skeleton")
}

/// A renderable drawing a skeleton, its look set by `set`
fn look(set: impl FnOnce(&mut Renderable)) -> Renderable {
  let mut renderable = Renderable::skeleton();
  set(&mut renderable);
  renderable
}

fn effect(effect: SpriteEffect) -> Renderable {
  look(|renderable| renderable.shader = Some(SpriteShader::BuiltIn(effect)))
}

/// The world box of a quad as a run draws it, its axes grown as the GPU
/// grows them for `padding` texels of an effect (`place_in_slot`)
fn run_box(quad: Affine2, rect: [u32; 4], padding: f32) -> (Vec2, Vec2) {
  let [_, _, width, height] = rect.map(|texels| texels as f32);
  let x = quad.matrix2.x_axis * (width + 2.0 * padding) / width;
  let y = quad.matrix2.y_axis * (height + 2.0 * padding) / height;
  let half = (x.abs() + y.abs()) / 2.0;
  (quad.translation - half, quad.translation + half)
}

fn points_box(points: impl IntoIterator<Item = Vec2>) -> (Vec2, Vec2) {
  points
    .into_iter()
    .fold((Vec2::INFINITY, Vec2::NEG_INFINITY), |(least, most), at| {
      (least.min(at), most.max(at))
    })
}

/// Every box the drawer culls by for what `renderable` draws of `parts`:
/// each region's run quad (grown by an effect's padding) and each mesh's
/// vertices, their drop shadows (cast as the drawer casts them), and the
/// blob and contact shadows
fn part_boxes(parts: &Parts<'_>, renderable: &Renderable, drawn: &Drawn) -> Vec<(Vec2, Vec2)> {
  let padding = match &renderable.shader {
    Some(SpriteShader::BuiltIn(effect)) => {
      effect_padding(&SpriteInstance::new().with_effect(effect).effect_params)
    }
    _ => 0.0,
  };
  let cast = renderable
    .drop_shadow
    .as_ref()
    .map(|shadow| super::drop_cast(shadow, drawn));
  let mut boxes = Vec::new();
  for part in &parts.list {
    match part {
      Part::Region(region) => {
        boxes.push(run_box(region.quad, region.rect, padding));
        if let Some(cast) = cast {
          boxes.push(run_box(cast * region.quad, region.rect, 0.0));
        }
      }
      Part::Mesh(mesh) => {
        let vertices = parts.vertices(mesh);
        boxes.push(points_box(vertices.iter().copied()));
        if let Some(cast) = cast {
          let cast = vertices.iter().map(|&at| cast.transform_point2(at));
          boxes.push(points_box(cast));
        }
      }
    }
  }
  let ground = ground_shadow_quads(renderable, drawn.position);
  for ((width, height), at, _) in ground.into_iter().flatten() {
    boxes.push(quad_box(Vec2::new(width as f32, height as f32), &at));
  }
  boxes
}

/// The region of 2 x 2 pixels drawn `width` by `height` at `(x, y)`, so an
/// effect's padding of a few texels grows it far
fn dot(x: f32, y: f32, width: f32, height: f32) -> Attachment {
  Attachment::Region(RegionAttachment {
    x,
    y,
    ..RegionAttachment::new(0, width, height)
  })
}

fn shows(name: &str) -> SlotSetup {
  SlotSetup {
    attachment: Some(Arc::from(name)),
    ..SlotSetup::default()
  }
}

/// A rig's root and its one region, 2 x 2 pixels of `image`, for the rigs
/// that test one rule each to add to
fn one_bone(image: &Handle<Image>) -> SkeletonDataBuilder {
  SkeletonData::builder()
    .bone("root", None, BoneSetup::default())
    .region(Region {
      name: Arc::from("dot"),
      image: image.clone(),
      x: 0,
      y: 0,
      width: 2,
      height: 2,
    })
}

/// A patch of two texels a side drawn 16 across, which an outline of 3
/// texels grows by 24; much larger and further out in the skin "big"
fn patch(image: &Handle<Image>) -> SkeletonData {
  one_bone(image)
    .slot("patch", "root", shows("patch"))
    .skin("big")
    .attachment(
      SkeletonData::DEFAULT_SKIN,
      "patch",
      "patch",
      dot(0.0, 0.0, 16.0, 16.0),
    )
    .attachment("big", "patch", "patch", dot(40.0, -30.0, 40.0, 24.0))
    .build()
    .expect("a valid skeleton")
}

/// A small flag on the root that "wave" deforms by up to 60, and a sail
/// weighted to a bone 100 along with nothing of its own
fn flag(image: &Handle<Image>) -> SkeletonData {
  let flag = MeshAttachment::new(
    0,
    MeshVertices::Unweighted(vec![
      Vec2::new(0.0, -4.0),
      Vec2::new(12.0, 0.0),
      Vec2::new(0.0, 4.0),
    ]),
    vec![Vec2::ZERO, Vec2::X, Vec2::Y],
    vec![0, 1, 2],
  );
  let sail = MeshAttachment::new(
    0,
    MeshVertices::Weighted(WeightedVertices::new([
      vec![Influence::new(ROOT, 0.0, -5.0, 1.0)],
      vec![Influence::new(FAR, 10.0, 5.0, 1.0)],
      vec![
        Influence::new(ROOT, 0.0, 5.0, 0.5),
        Influence::new(FAR, 0.0, 5.0, 0.5),
      ],
    ])),
    vec![Vec2::ZERO, Vec2::X, Vec2::Y],
    vec![0, 1, 2],
  );
  let key = vec![-40.0, 45.0, 60.0, -10.0, 5.0, 50.0];
  let wave = Animation {
    name: Arc::from("wave"),
    duration: 1.0,
    timelines: vec![Timeline::Deform {
      slot: SlotId(0),
      attachment: Arc::from("flag"),
      frames: vec![
        (0.0, vec![0.0; 6], Curve::Linear),
        (0.5, key, Curve::Linear),
        (1.0, vec![0.0; 6], Curve::Linear),
      ],
    }],
  };
  let skin = SkeletonData::DEFAULT_SKIN;
  one_bone(image)
    .bone(
      "far",
      Some("root"),
      BoneSetup {
        x: 100.0,
        ..BoneSetup::default()
      },
    )
    .slot("flag", "root", shows("flag"))
    .slot("sail", "root", shows("sail"))
    .attachment(skin, "flag", "flag", Attachment::Mesh(flag))
    .attachment(skin, "sail", "sail", Attachment::Mesh(sail))
    .animation(wave)
    .build()
    .expect("a valid skeleton")
}

/// A small patch on a root that "wave" moves 90 across and back in a
/// second: far between two steps
fn dash(image: &Handle<Image>) -> SkeletonData {
  let wave = Animation {
    name: Arc::from("wave"),
    duration: 1.0,
    timelines: vec![Timeline::Translate {
      bone: ROOT,
      frames: vec![
        (0.0, Vec2::ZERO, Curve::Linear),
        (0.5, Vec2::new(90.0, -60.0), Curve::Linear),
        (1.0, Vec2::ZERO, Curve::Linear),
      ],
    }],
  };
  one_bone(image)
    .slot("patch", "root", shows("patch"))
    .attachment(
      SkeletonData::DEFAULT_SKIN,
      "patch",
      "patch",
      dot(0.0, 0.0, 8.0, 8.0),
    )
    .animation(wave)
    .build()
    .expect("a valid skeleton")
}

/// A patch on a bone scaled a tenth as wide and ten times as tall, so its
/// largest stretch is along its y axis
fn stilt(image: &Handle<Image>) -> SkeletonData {
  one_bone(image)
    .bone(
      "stilt",
      Some("root"),
      BoneSetup {
        scale_x: 0.1,
        scale_y: 10.0,
        ..BoneSetup::default()
      },
    )
    .slot("patch", "stilt", shows("patch"))
    .attachment(
      SkeletonData::DEFAULT_SKIN,
      "patch",
      "patch",
      dot(0.0, 0.0, 8.0, 8.0),
    )
    .build()
    .expect("a valid skeleton")
}

/// A patch on a bone held at a place in the world, so each new stand moves
/// it in the skeleton's space: posed again with no step, its box must move
/// with it
fn pinned(image: &Handle<Image>) -> SkeletonData {
  one_bone(image)
    .bone(
      "pin",
      Some("root"),
      BoneSetup {
        x: 10.0,
        ..BoneSetup::default()
      },
    )
    .slot("patch", "pin", shows("patch"))
    .attachment(
      SkeletonData::DEFAULT_SKIN,
      "patch",
      "patch",
      dot(0.0, 0.0, 8.0, 8.0),
    )
    .build()
    .expect("a valid skeleton")
}

/// Walks cameras past a skeleton of the rig `build` makes, playing
/// `tracks` (an animation, how it blends and its speed, a track each) in
/// steps of `dt` and swapping between `skins` as it goes: at many poses,
/// drawn between steps and where it is, lifted, depth-scaled, flipped, by
/// an entity moved, turned and stretched between steps, outlined, glowing,
/// and with each kind of shadow, no camera sees a part of it but sees its
/// box
fn walk(
  build: fn(&Handle<Image>) -> SkeletonData,
  tracks: &[(&str, MixBlend, f32)],
  skins: &[&str],
  dt: f32,
) {
  let mut world = asset_world();
  let image = world
    .resource_mut::<Assets<Image>>()
    .add(Image::solid_rect(32, 32, 255, 255, 255, 255));
  let handle = world
    .resource_mut::<Assets<SkeletonData>>()
    .add(build(&image));
  let assets = world.resource::<Assets<SkeletonData>>();
  let data = assets.get(&handle).expect("the data");
  let seen = (handle.id(), assets.version(&handle).expect("a version"));

  let mut skeleton = Skeleton::new(handle.clone());
  // A bone it has called "pin" is held at a place in the world
  let pin = BoneOverride {
    translation: Some(Vec2::new(-30.0, 70.0)),
    space: Space::World,
    ..BoneOverride::default()
  };
  if data.bone("pin").is_some() {
    skeleton.override_bone("pin", pin);
  }
  let mut animation = SkeletonAnimation::new();
  skeleton.step_by_hand(
    (data, seen),
    Some(&mut animation),
    dt,
    Affine2::IDENTITY,
    (false, false),
  );
  for (track, &(name, blend, speed)) in tracks.iter().enumerate() {
    let entry = animation.set_animation(track, name, true).expect(name);
    let entry = animation.entry_mut(entry).expect(name);
    entry.mix_blend = blend;
    entry.speed = speed;
  }

  // The entity drawn between two steps (moved, turned and stretched), or
  // where it is
  let was = GlobalTransform::from_transform(&Transform::from_xy(-8.0, 4.0).with_rotation(-0.3));
  let now = GlobalTransform::from_transform(
    &Transform::from_xy(14.0, 9.0)
      .with_rotation(0.6)
      .with_scale(Vec2::new(1.3, 0.8)),
  );
  let previous = PreviousGlobalTransform {
    global: was,
    skip: false,
  };
  // Where it is carried to without a step
  let moved = GlobalTransform::from_transform(
    &Transform::from_xy(70.0, -40.0)
      .with_rotation(-0.5)
      .with_scale(Vec2::new(0.9, 1.2)),
  );
  let deep = YSort::new().with_depth_scale(0.5, 1.6, 0.0, 10.0);
  let red = Color::rgb(255, 0, 0);
  let looks = [
    look(|_| {}),
    effect(SpriteEffect::outline(red, 3)),
    effect(SpriteEffect::glow(red, 4.5)),
    effect(SpriteEffect::directional_outline(red, Color::WHITE, 2)),
    look(|renderable| {
      renderable.drop_shadow = Some(DropShadow {
        offset: Vec2::new(26.0, 19.0),
        scale: Vec2::new(1.7, 0.4),
        ..DropShadow::new()
      });
    }),
    look(|renderable| renderable.drop_shadow = Some(DropShadow::new().offset(-30.0, 12.0))),
    look(|renderable| {
      renderable.blob_shadow = Some(BlobShadow {
        offset_y: 40.0,
        ..BlobShadow::circle(30.0)
      });
    }),
    look(|renderable| {
      renderable.contact_shadow = Some(ContactShadow {
        offset_y: -35.0,
        ..ContactShadow::new().radius(45.0)
      });
    }),
  ];
  // Cameras 24 across, over a grid round where the skeleton draws
  let camera = Camera2d::default();
  let views: Vec<CameraView> = (-30..=30)
    .flat_map(|x| (-30..=30).map(move |y| Vec2::new(x as f32, y as f32) * 6.0))
    .map(|center| CameraView::compute(center, 0.0, &camera, Vec2::ZERO, Vec2::splat(24.0)))
    .collect();

  let (mut seen_parts, mut checked) = (0usize, 0usize);
  for step in 0..32 {
    let flip = (step % 5 == 3, step % 7 == 4);
    // Posed by a step where the entity stands; at every third step placed
    // again where it was carried to (as a socket's child is) after its
    // step; once posed outside a step (`appear`) there instead. A bone
    // held at a place in the world moves in the skeleton's space with
    // each new stand
    let stand = if step % 3 == 1 || step == 16 {
      &moved
    } else {
      &now
    };
    if step == 16 {
      skeleton.appear_by_hand((data, seen), Some(&mut animation), stand.affine(), flip);
    } else {
      skeleton.step_by_hand((data, seen), Some(&mut animation), dt, now.affine(), flip);
      if step % 3 == 1 {
        skeleton.place_again_by_hand(data, stand.affine(), flip);
      }
    }
    // A skin put on shows at once, before the next step
    if let Some(skin) = skins.get(step % 3) {
      skeleton.set_skin(skin);
    }
    let bounds = skeleton.bounds().expect("posed");
    for (alpha, previous, lift, y_sort) in [
      (0.0, None, Vec2::ZERO, None),
      (0.0, Some(&previous), Vec2::ZERO, None),
      (0.35, Some(&previous), Vec2::new(0.0, -40.0), None),
      (0.8, Some(&previous), Vec2::ZERO, Some(&deep)),
      (1.0, Some(&previous), Vec2::new(5.0, -12.0), Some(&deep)),
    ] {
      let carrier = Carrier {
        global: stand,
        previous,
        lift,
        y_sort,
      };
      let drawn = skeleton.drawn(&carrier, alpha, None);
      let mut parts = Parts::default();
      skeleton.parts(data, seen, drawn.root, drawn.between, &mut parts);
      for renderable in &looks {
        let padding = match &renderable.shader {
          Some(SpriteShader::BuiltIn(effect)) => {
            effect_padding(&SpriteInstance::new().with_effect(effect).effect_params)
          }
          _ => 0.0,
        };
        let (min, max) = skeleton_reach(&bounds, &drawn, renderable, padding);
        let boxes = part_boxes(&parts, renderable, &drawn);
        for view in &views {
          checked += 1;
          let shows = boxes
            .iter()
            .find(|(low, high)| view.is_visible(*low, *high));
          if let Some(part) = shows {
            seen_parts += 1;
            assert!(
              view.is_visible(min, max),
              "step {step}, at {alpha} between, lifted {lift}, {renderable:?}: the part in \
               {part:?} shows in {:?}, outside {min}..{max}",
              view.bounds
            );
          }
        }
      }
    }
  }
  // The walk saw parts, and passed by them
  assert!(
    seen_parts > 1000 && seen_parts < checked / 2,
    "{seen_parts} of {checked}"
  );
}

#[test]
fn a_skeleton_showing_any_part_is_never_culled() {
  let (replace, add) = (MixBlend::Replace, MixBlend::Add);
  // Pivoted and mirrored regions, weighted and unweighted meshes deformed
  // past their keys by an overshooting curve and an added layer, bones
  // turned, scaled, sheared and moved, and two skins
  walk(
    rig,
    &[("wave", replace, 1.0), ("pulse", add, 1.37)],
    &["big", SkeletonData::DEFAULT_SKIN],
    0.037,
  );
  // And each rule alone, where nothing else in the box hides it: an
  // effect's padding and a larger skin, a deform and a vertex weighted to a
  // bone with nothing of its own, the pose before the last, and a stretch
  // along a bone's y axis
  walk(
    patch,
    &[],
    &["big", SkeletonData::DEFAULT_SKIN, "big"],
    0.037,
  );
  walk(flag, &[("wave", replace, 1.0)], &[], 0.037);
  walk(dash, &[("wave", replace, 1.0)], &[], 0.11);
  walk(stilt, &[], &[], 0.037);
  walk(pinned, &[], &[], 0.037);
}
