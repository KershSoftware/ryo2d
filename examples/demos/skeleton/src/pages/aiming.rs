//! Aiming: bone overrides. After the animation has posed the skeleton, the
//! page points the near arm and turns the head at a place in the world; the
//! engine converts each to the bone's own space every step

use std::f32::consts::PI;

use demo_kit::prelude::*;

use crate::hero::{self, GROUND, SCALE};

/// What plays under the overrides
const BASES: [&str; 2] = ["idle", "walk"];
/// How far the head turns from straight ahead, in radians
const NECK: f32 = 0.35;
const TARGET: Color = Color::rgb(255, 214, 102);

pub struct AimingPage {
  hero: Option<Entity>,
  /// Settings: how far each override takes its bone from the animated pose
  arm: f32,
  head: f32,
  base: usize,
  playing_base: usize,
  /// Where the pointer was when the page opened: until it moves, the page
  /// aims at a place that circles the hero
  pointer_at_first: Option<Vec2>,
  pointing: bool,
  clock: f32,
  /// What the hero aims at, in the world
  target: Vec2,
}

impl Default for AimingPage {
  fn default() -> Self {
    Self {
      hero: None,
      arm: 1.0,
      head: 1.0,
      base: 0,
      playing_base: 0,
      pointer_at_first: None,
      pointing: false,
      clock: 0.0,
      target: Vec2::ZERO,
    }
  }
}

impl DemoPage for AimingPage {
  fn name(&self) -> &str {
    "Aiming"
  }

  fn description(&self) -> &str {
    "Bone overrides in world space: the near arm points at the pointer and the head turns to it, over whatever animation plays.\n\
     Each has a mix from the animated pose (0) to the override (1). The hero turns round to face the pointer."
  }

  fn settings(&mut self, s: &mut Settings) {
    s.float("Arm", &mut self.arm, 0.0..=1.0, 0.1);
    s.float("Head", &mut self.head, 0.0..=1.0, 0.1);
    s.choice("Under them", &mut self.base, &["Idle", "Walk"]);
  }

  fn controls(&self) -> Vec<Control> {
    vec![Control::mouse("Aim")]
  }

  fn enter(&mut self, ctx: &mut SceneContext) {
    hero::backdrop(ctx);
    let Some(data) = hero::load(ctx) else {
      return;
    };
    let hero = hero::spawn(ctx, &data, Vec2::new(400.0, GROUND), SCALE);
    self.hero = Some(hero);
    if let Some(mut state) = ctx.world.get_mut::<SkeletonAnimation>(hero) {
      state.set_animation(0, BASES[self.base], true);
    }
    self.playing_base = self.base;
  }

  fn update(&mut self, ctx: &mut SceneContext, dt: f32) {
    let Some(hero) = self.hero else {
      return;
    };
    self.clock += dt;
    let pointer = hero::pointer(ctx);
    match self.pointer_at_first {
      None => self.pointer_at_first = Some(pointer),
      Some(first) if first != pointer => self.pointing = true,
      Some(_) => {}
    }
    let Some(stands) = ctx.world.get::<Transform>(hero).map(|at| at.translation) else {
      return;
    };
    self.target = if self.pointing {
      pointer
    } else {
      let turn = -1.75 + self.clock * 0.9;
      Vec2::new(stands.x, GROUND - 270.0) + Vec2::new(turn.cos() * 330.0, turn.sin() * 200.0)
    };
    let target = self.target;

    if self.base != self.playing_base
      && let Some(mut state) = ctx.world.get_mut::<SkeletonAnimation>(hero)
    {
      state.set_animation(0, BASES[self.base], true);
      self.playing_base = self.base;
    }

    // Turn to face the target, with a little slack so it does not flicker
    // when the target is overhead
    let mut left = ctx
      .world
      .get::<Renderable>(hero)
      .is_some_and(|renderable| renderable.flip_x);
    let side = target.x - stands.x;
    if side.abs() > 12.0 {
      left = side < 0.0;
      hero::face(ctx, hero, left);
    }

    let Some(mut skeleton) = ctx.world.get_mut::<Skeleton>(hero) else {
      return;
    };
    // The arm's bone lies along the arm, so a world rotation points it:
    // from the shoulder, where the last step left it, to the target
    if let Some(shoulder) = skeleton.bone_world("arm_near") {
      skeleton.override_bone(
        "arm_near",
        BoneOverride {
          rotation: Some((target - shoulder.translation).to_angle()),
          space: BoneSpace::World,
          mix: self.arm,
          ..Default::default()
        },
      );
    }
    // The head's bone points the way the hero looks. It turns only so far
    // from straight ahead, which is left for a hero turned round
    if let Some(head) = skeleton.bone_world("head") {
      let to_target = target - head.translation;
      let ahead = if left {
        Vec2::new(-to_target.x, to_target.y)
      } else {
        to_target
      };
      let turn = ahead.to_angle().clamp(-NECK, NECK);
      skeleton.override_bone(
        "head",
        BoneOverride {
          rotation: Some(if left { PI - turn } else { turn }),
          space: BoneSpace::World,
          mix: self.head,
          ..Default::default()
        },
      );
    }
    let hand = skeleton
      .bone_world("hand_near")
      .map(|hand| hand.translation);
    let mut debug = ctx.debug_draw();
    if let Some(hand) = hand {
      debug.line(hand, target, Color::new(255, 214, 102, 90));
    }
    debug.width(2.0).cross(target, 16.0, TARGET);
    debug.width(2.0).circle(target, 10.0, TARGET);
  }

  fn ui(&mut self, ui: &mut UiContext, ctx: &mut SceneContext) {
    let Some(hero) = self.hero else {
      return;
    };
    let aim = if self.pointing {
      "Aiming at the pointer"
    } else {
      "Aiming at a circling target (move the pointer to aim)"
    };
    hero::readout(
      ui,
      &[
        aim.to_string(),
        format!("Arm mix {:.1}, head mix {:.1}", self.arm, self.head),
        hero::track_line(ctx, hero, 0),
      ],
    );
  }
}
