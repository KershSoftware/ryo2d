//! Constrained skeletons drawn through the world batch: an arm whose IK
//! constraint brings its hand to a target is drawn where its step posed
//! it, and is flipped, y-sorted and drawn between steps as any skeleton
//! is, and a part a transform constraint holds level stays level while
//! the arm carrying it turns, read back as display bytes. The rigs and
//! the frame helpers are the skeleton pixel tests' own (`skeleton.rs`).
//!
//! Tests skip when no GPU is available.

use std::sync::Arc;

use ryo2d::prelude::*;
use ryo2d::testing::HeadlessEngine;

use super::skeleton::{
  BLUE, GREEN, Images, Part, RED, WHITE, assert_boxes, bone, engine, frame, part, part_step,
  rig_with, settled, square, stand, stepped,
};

/// A colour that draws nothing
const UNSEEN: [u8; 4] = [0, 0, 0, 0];

/// The arm's target bone, after its four parts' bones
const GOAL: BoneId = BoneId(4);

/// An arm of two parts, 20 and 15 px long, lying along x from its
/// skeleton's origin (its shoulder), with no art of its own: a 4 px square
/// of `elbow` marks its elbow and one of `hand` its hand, each upright
/// whatever the arm does. Its target is the bone "goal", at `goal` in the
/// skeleton's space, and the constraint "reach" brings the hand to it;
/// `more` adds to the rig.
///
/// With the target 25 px from the shoulder, the parts and the line to it
/// make a 3-4-5 triangle with its right angle at the elbow: the elbow is
/// 16 px along that line and 12 px to the side of it from which the
/// forearm turns clockwise on screen
fn arm(
  world: &mut World,
  (elbow, hand): ([u8; 4], [u8; 4]),
  goal: Vec2,
  more: impl FnOnce(SkeletonDataBuilder) -> SkeletonDataBuilder,
) -> Handle<SkeletonData> {
  let bare = |name: &'static str, length: f32| {
    let mut bare = Part {
      colour: UNSEEN,
      ..part(name, WHITE, (1, 1))
    };
    bare.bone.length = length;
    bare
  };
  let mark = |name: &'static str, colour: [u8; 4], along: f32| {
    let mut mark = part(name, colour, (4, 4)).under("lower", along, 0.0);
    mark.bone.inherit = BoneInherit::OnlyTranslation;
    mark
  };
  let parts = [
    bare("upper", 20.0),
    bare("lower", 15.0).under("upper", 20.0, 0.0),
    mark("elbow", elbow, 0.0),
    mark("hand", hand, 15.0),
  ];
  rig_with(world, &parts, Images::Each, |builder| {
    let target = BoneSetup {
      x: goal.x,
      y: goal.y,
      ..BoneSetup::default()
    };
    more(builder.bone("goal", None, target).ik(
      "reach",
      &["upper", "lower"],
      "goal",
      IkSetup::default(),
    ))
  })
}

/// Puts `hero`'s target at a point in the world
fn reach_for(engine: &mut HeadlessEngine, hero: Entity, target: Vec2) {
  let put = BoneOverride {
    translation: Some(target),
    space: BoneSpace::World,
    ..BoneOverride::default()
  };
  let mut skeleton = engine.world_mut().get_mut::<Skeleton>(hero).unwrap();
  skeleton.override_bone("goal", put);
}

#[test]
fn a_two_bone_arm_reaches_a_moved_target() {
  let Some(mut engine) = engine() else {
    return;
  };
  // The arm's shoulder is at (32, 20), and it lies straight to the right
  // until the game puts its target somewhere in the world
  let world = engine.world_mut();
  let data = arm(world, (GREEN, WHITE), Vec2::new(35.0, 0.0), |builder| {
    builder
  });
  let hero = stand(world, data, (32.0, 20.0), Renderable::skeleton());
  let hand_at = |engine: &HeadlessEngine| bone(engine, hero, "hand").translation;

  // 25 px below the shoulder: the hand is on the target, and the elbow 16
  // px down and 12 px to the right
  let target = Vec2::new(32.0, 45.0);
  reach_for(&mut engine, hero, target);
  assert_boxes(
    &settled(&mut engine),
    &[((42, 34, 4, 4), GREEN), ((30, 43, 4, 4), WHITE)],
    "reaching down",
  );
  assert!(hand_at(&engine).distance(target) < 0.01);

  // Moved 25 px to the right of the shoulder: the hand is there, and the
  // elbow 16 px right and 12 px up
  let target = Vec2::new(57.0, 20.0);
  reach_for(&mut engine, hero, target);
  assert_boxes(
    &settled(&mut engine),
    &[((46, 6, 4, 4), GREEN), ((55, 18, 4, 4), WHITE)],
    "reaching right",
  );
  assert!(hand_at(&engine).distance(target) < 0.01);
}

#[test]
fn a_constrained_skeleton_flips_sorts_and_draws_between_steps_as_any_other() {
  // Flipped. The arm's target is 25 px to the right of its shoulder in
  // the skeleton's space, so standing at (32, 20) its hand is at (57, 20)
  // and its elbow at (48, 8)
  {
    let Some(mut engine) = engine() else {
      return;
    };
    let world = engine.world_mut();
    let data = arm(world, (GREEN, WHITE), Vec2::new(25.0, 0.0), |builder| {
      builder
    });
    let hero = stand(world, data, (32.0, 20.0), Renderable::skeleton());
    assert_boxes(
      &stepped(&mut engine),
      &[((46, 6, 4, 4), GREEN), ((55, 18, 4, 4), WHITE)],
      "unflipped",
    );
    // Flipped left to right about the entity's origin, the whole limb is
    // mirrored in that step, the elbow with the hand
    engine
      .world_mut()
      .get_mut::<Renderable>(hero)
      .unwrap()
      .flip_x = true;
    assert_boxes(
      &stepped(&mut engine),
      &[((14, 6, 4, 4), GREEN), ((5, 18, 4, 4), WHITE)],
      "flipped left to right",
    );
    let hand = bone(&engine, hero, "hand").translation;
    assert!(hand.distance(Vec2::new(7.0, 20.0)) < 0.01, "{hand}");
  }

  // Sorted. Two arms, y-sorted at one z, reach for one point, (32, 44),
  // below where both stand; the one spawned first stands lower on the
  // screen. They draw in the order of where they stand, as any y-sorted
  // pair does, whatever their hands reach for: the lower one's hand shows
  {
    let Some(mut engine) = engine() else {
      return;
    };
    let world = engine.world_mut();
    let standing = |world: &mut World, hand: [u8; 4], at: (f32, f32)| {
      let data = arm(world, (UNSEEN, hand), Vec2::new(35.0, 0.0), |builder| {
        builder
      });
      let hero = stand(world, data, at, Renderable::skeleton().with_z_order(1));
      world.entity_mut(hero).insert(YSort::new());
      hero
    };
    let blue = standing(world, BLUE, (40.0, 30.0));
    let red = standing(world, RED, (22.0, 20.0));
    let point = Vec2::new(32.0, 44.0);
    reach_for(&mut engine, blue, point);
    reach_for(&mut engine, red, point);
    let hands = (30, 42, 4, 4);
    assert_boxes(&settled(&mut engine), &[(hands, BLUE)], "blue stands lower");
    // The red one steps down below the blue one, and its hand is in front
    engine
      .world_mut()
      .get_mut::<Transform>(red)
      .unwrap()
      .translation
      .y = 36.0;
    assert_boxes(&settled(&mut engine), &[(hands, RED)], "red stands lower");
    for hero in [blue, red] {
      assert!(bone(&engine, hero, "hand").translation.distance(point) < 0.01);
    }
  }

  // Drawn between steps. An animation slides the target 4 px a step to
  // the right along a line 20 px below the shoulder, and the hand goes
  // with it
  let Some(mut engine) = engine() else {
    return;
  };
  let world = engine.world_mut();
  let slide = SkeletonClip {
    name: Arc::from("slide"),
    duration: 10.0,
    timelines: vec![SkeletonTimeline::Translate {
      bone: GOAL,
      frames: vec![
        (0.0, Vec2::ZERO, SkeletonCurve::Linear),
        (10.0, Vec2::new(2400.0, 0.0), SkeletonCurve::Linear),
      ],
    }],
  };
  let data = arm(world, (UNSEEN, WHITE), Vec2::new(-10.0, 20.0), |builder| {
    builder.animation(slide)
  });
  let hero = stand(world, data, (32.0, 20.0), Renderable::skeleton());
  let mut animation = SkeletonAnimation::new();
  animation.set_animation(0, "slide", false);
  world.entity_mut(hero).insert(animation);
  engine.step_frames(2).unwrap();
  let drawn = frame(&engine);
  let now = bone(&engine, hero, "hand").translation;
  assert!(
    now.distance(Vec2::new(30.0, 40.0)) < 0.01,
    "two steps of 4 px from (22, 40): {now}"
  );
  // A frame of whole steps draws the hand where the step before posed it
  assert_eq!(square(&drawn, 26.0, 40.0), Some(true), "the step before");
  assert_eq!(square(&drawn, 30.0, 40.0), Some(false));
  // Half a step later no step has run, and it is drawn half way between
  // the two constrained poses
  part_step(&mut engine, 5);
  let drawn = frame(&engine);
  assert_eq!(square(&drawn, 28.0, 40.0), Some(true), "half way");
  assert_eq!(square(&drawn, 24.0, 40.0), Some(false));
  assert_eq!(square(&drawn, 32.0, 40.0), Some(false));
}

#[test]
fn a_part_held_level_stays_level_while_its_arm_turns() {
  let Some(mut engine) = engine() else {
    return;
  };
  // An arm 20 px long with no art, standing at (32, 24), carries at its
  // tip a part 12 px wide and 4 px tall. "hold" sets the part's turn in
  // the skeleton's space to that of "level", a bone that is not turned, so
  // whichever way the arm points the part is drawn 12 px wide and 4 px
  // tall, centred where the arm's tip is
  let world = engine.world_mut();
  let parts = [
    Part {
      colour: UNSEEN,
      ..part("arm", WHITE, (1, 1))
    },
    part("part", GREEN, (12, 4)).under("arm", 20.0, 0.0),
  ];
  let hold = TransformConstraintSetup {
    mix: TransformConstraintMix {
      rotate: 1.0,
      ..TransformConstraintMix::default()
    },
    ..TransformConstraintSetup::default()
  };
  let data = rig_with(world, &parts, Images::Each, |builder| {
    builder
      .bone("level", None, BoneSetup::default())
      .transform_constraint("hold", &["part"], "level", hold)
  });
  let hero = stand(world, data, (32.0, 24.0), Renderable::skeleton());
  // The arm pointing right, down and left: its tip at (52, 24), (32, 44)
  // and (12, 24)
  let turns = [
    (0.0, (46, 22, 12, 4), "pointing right"),
    (
      std::f32::consts::FRAC_PI_2,
      (26, 42, 12, 4),
      "pointing down",
    ),
    (std::f32::consts::PI, (6, 22, 12, 4), "pointing left"),
  ];
  for (turn, drawn, what) in turns {
    let turned = BoneOverride {
      rotation: Some(turn),
      ..BoneOverride::default()
    };
    let mut skeleton = engine.world_mut().get_mut::<Skeleton>(hero).unwrap();
    skeleton.override_bone("arm", turned);
    assert_boxes(&settled(&mut engine), &[(drawn, GREEN)], what);
  }
}

#[cfg(dev_tools)]
#[test]
fn the_overlay_draws_its_marks_at_the_target_and_the_tip() {
  let Some(mut engine) = engine() else {
    return;
  };
  // An arm 35 px long standing at (32.5, 20.5), on a pixel's centre, and a
  // target 40 px below its shoulder, out of its reach: the arm stretches
  // straight down, its tip at (32.5, 55.5), and the overlay crosses the
  // target at (32.5, 60.5) and runs a line from the tip to it
  let world = engine.world_mut();
  let data = arm(world, (UNSEEN, UNSEEN), Vec2::new(35.0, 0.0), |builder| {
    builder
  });
  let hero = stand(world, data, (32.5, 20.5), Renderable::skeleton());
  reach_for(&mut engine, hero, Vec2::new(32.5, 60.5));
  let off = settled(&mut engine);
  // `colors::SKELETON_CONSTRAINT`, which the crate does not export
  let mark = [0xff, 0x50, 0xa0, 0xff];
  let marked = |frame: &image::RgbaImage, x: u32, y: u32| frame.get_pixel(x, y).0 == mark;
  assert!(
    off.pixels().all(|pixel| pixel.0 != mark),
    "nothing while it is off"
  );

  assert!(!engine.console_run("debug:skeletons").is_empty());
  let on = settled(&mut engine);
  assert!(marked(&on, 32, 60), "the target's cross");
  assert!(marked(&on, 29, 60) && marked(&on, 35, 60), "its arms");
  // The bones are drawn over the marks, so a pixel or two of the line shows
  // under a joint
  assert!(
    marked(&on, 32, 56) && marked(&on, 32, 59),
    "the line from the tip to the target"
  );
  assert!(!marked(&on, 32, 40), "none along the arm");
}
