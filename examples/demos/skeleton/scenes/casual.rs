use std::sync::Arc;

use demo_kit::prelude::*;

use super::SceneBase;
use super::fighter::create_fighter_idle;
use crate::create_stick_fighter;

fn create_casual_walk(def: &Arc<SkeletonDef>) -> Arc<SkeletalAnimation> {
  Arc::new(
    SkeletalAnimation::builder("walk", 1.2, def.clone())
      .loop_mode(LoopMode::Loop)
      .track("l_upper_arm", |t| {
        t.at(0.0, BoneTransform::rotation(0.0), Ease::InOutSine)
          .at(0.3, BoneTransform::rotation(0.3), Ease::InOutSine)
          .at(0.6, BoneTransform::rotation(0.0), Ease::InOutSine)
          .at(0.9, BoneTransform::rotation(-0.3), Ease::InOutSine)
          .at(1.2, BoneTransform::rotation(0.0), Ease::InOutSine)
      })
      .track("r_upper_arm", |t| {
        t.at(0.0, BoneTransform::rotation(0.0), Ease::InOutSine)
          .at(0.3, BoneTransform::rotation(-0.3), Ease::InOutSine)
          .at(0.6, BoneTransform::rotation(0.0), Ease::InOutSine)
          .at(0.9, BoneTransform::rotation(0.3), Ease::InOutSine)
          .at(1.2, BoneTransform::rotation(0.0), Ease::InOutSine)
      })
      .track("l_thigh", |t| {
        t.at(0.0, BoneTransform::rotation(0.0), Ease::InOutSine)
          .at(0.3, BoneTransform::rotation(-0.25), Ease::InOutSine)
          .at(0.6, BoneTransform::rotation(0.0), Ease::InOutSine)
          .at(0.9, BoneTransform::rotation(0.25), Ease::InOutSine)
          .at(1.2, BoneTransform::rotation(0.0), Ease::InOutSine)
      })
      .track("r_thigh", |t| {
        t.at(0.0, BoneTransform::rotation(0.0), Ease::InOutSine)
          .at(0.3, BoneTransform::rotation(0.25), Ease::InOutSine)
          .at(0.6, BoneTransform::rotation(0.0), Ease::InOutSine)
          .at(0.9, BoneTransform::rotation(-0.25), Ease::InOutSine)
          .at(1.2, BoneTransform::rotation(0.0), Ease::InOutSine)
      })
      .build(),
  )
}

fn create_casual_wave(def: &Arc<SkeletonDef>) -> Arc<SkeletalAnimation> {
  Arc::new(
    SkeletalAnimation::builder("wave", 1.5, def.clone())
      .loop_mode(LoopMode::Once)
      .track("r_upper_arm", |t| {
        t.at(0.0, BoneTransform::rotation(0.0), Ease::OutQuad)
          .at(0.3, BoneTransform::rotation(2.5), Ease::Linear)
          .at(1.5, BoneTransform::rotation(2.5), Ease::Linear)
      })
      .track("r_forearm", |t| {
        t.at(0.0, BoneTransform::rotation(0.0), Ease::InOutSine)
          .at(0.3, BoneTransform::rotation(0.5), Ease::InOutSine)
          .at(0.6, BoneTransform::rotation(-0.5), Ease::InOutSine)
          .at(0.9, BoneTransform::rotation(0.5), Ease::InOutSine)
          .at(1.2, BoneTransform::rotation(-0.5), Ease::InOutSine)
          .at(1.5, BoneTransform::rotation(0.0), Ease::InOutSine)
      })
      .build(),
  )
}

fn create_casual_jump(def: &Arc<SkeletonDef>) -> Arc<SkeletalAnimation> {
  Arc::new(
    SkeletalAnimation::builder("jump", 0.6, def.clone())
      .loop_mode(LoopMode::Once)
      .track("l_thigh", |t| {
        t.at(0.0, BoneTransform::rotation(0.0), Ease::InOutSine)
          .at(0.1, BoneTransform::rotation(0.3), Ease::InOutSine)
          .at(0.3, BoneTransform::rotation(-0.2), Ease::InOutSine)
          .at(0.6, BoneTransform::rotation(0.0), Ease::InOutSine)
      })
      .track("r_thigh", |t| {
        t.at(0.0, BoneTransform::rotation(0.0), Ease::InOutSine)
          .at(0.1, BoneTransform::rotation(0.3), Ease::InOutSine)
          .at(0.3, BoneTransform::rotation(-0.2), Ease::InOutSine)
          .at(0.6, BoneTransform::rotation(0.0), Ease::InOutSine)
      })
      .track("l_shin", |t| {
        t.at(0.0, BoneTransform::rotation(0.0), Ease::InOutSine)
          .at(0.1, BoneTransform::rotation(-0.3), Ease::InOutSine)
          .at(0.3, BoneTransform::rotation(0.2), Ease::InOutSine)
          .at(0.6, BoneTransform::rotation(0.0), Ease::InOutSine)
      })
      .track("r_shin", |t| {
        t.at(0.0, BoneTransform::rotation(0.0), Ease::InOutSine)
          .at(0.1, BoneTransform::rotation(-0.3), Ease::InOutSine)
          .at(0.3, BoneTransform::rotation(0.2), Ease::InOutSine)
          .at(0.6, BoneTransform::rotation(0.0), Ease::InOutSine)
      })
      .track("l_upper_arm", |t| {
        t.at(0.0, BoneTransform::rotation(0.0), Ease::InOutSine)
          .at(0.1, BoneTransform::rotation(0.0), Ease::InOutSine)
          .at(0.3, BoneTransform::rotation(-0.5), Ease::InOutSine)
          .at(0.6, BoneTransform::rotation(0.0), Ease::InOutSine)
      })
      .track("r_upper_arm", |t| {
        t.at(0.0, BoneTransform::rotation(0.0), Ease::InOutSine)
          .at(0.1, BoneTransform::rotation(0.0), Ease::InOutSine)
          .at(0.3, BoneTransform::rotation(0.5), Ease::InOutSine)
          .at(0.6, BoneTransform::rotation(0.0), Ease::InOutSine)
      })
      .build(),
  )
}

fn create_casual_dance(def: &Arc<SkeletonDef>) -> Arc<SkeletalAnimation> {
  Arc::new(
    SkeletalAnimation::builder("dance", 2.0, def.clone())
      .loop_mode(LoopMode::Loop)
      .track("spine", |t| {
        t.at(0.0, BoneTransform::rotation(0.0), Ease::InOutSine)
          .at(0.5, BoneTransform::rotation(0.1), Ease::InOutSine)
          .at(1.0, BoneTransform::rotation(0.0), Ease::InOutSine)
          .at(1.5, BoneTransform::rotation(-0.1), Ease::InOutSine)
          .at(2.0, BoneTransform::rotation(0.0), Ease::InOutSine)
      })
      .track("l_upper_arm", |t| {
        t.at(0.0, BoneTransform::rotation(0.0), Ease::InOutSine)
          .at(0.5, BoneTransform::rotation(-0.4), Ease::InOutSine)
          .at(1.0, BoneTransform::rotation(0.0), Ease::InOutSine)
          .at(1.5, BoneTransform::rotation(0.4), Ease::InOutSine)
          .at(2.0, BoneTransform::rotation(0.0), Ease::InOutSine)
      })
      .track("r_upper_arm", |t| {
        t.at(0.0, BoneTransform::rotation(0.0), Ease::InOutSine)
          .at(0.5, BoneTransform::rotation(0.4), Ease::InOutSine)
          .at(1.0, BoneTransform::rotation(0.0), Ease::InOutSine)
          .at(1.5, BoneTransform::rotation(-0.4), Ease::InOutSine)
          .at(2.0, BoneTransform::rotation(0.0), Ease::InOutSine)
      })
      .track("head", |t| {
        t.at(0.0, BoneTransform::rotation(0.0), Ease::InOutSine)
          .at(0.5, BoneTransform::rotation(-0.1), Ease::InOutSine)
          .at(1.0, BoneTransform::rotation(0.0), Ease::InOutSine)
          .at(1.5, BoneTransform::rotation(0.1), Ease::InOutSine)
          .at(2.0, BoneTransform::rotation(0.0), Ease::InOutSine)
      })
      .build(),
  )
}

pub struct CasualScene {
  base: SceneBase,
}

impl CasualScene {
  pub fn new() -> Self {
    let def = &create_stick_fighter();
    let idle = create_fighter_idle(def);
    let walk = create_casual_walk(def);
    let wave = create_casual_wave(def);
    let jump = create_casual_jump(def);
    let dance = create_casual_dance(def);

    let controller = SkeletalController::new(idle, "idle", def.clone())
      .with_animation(walk, "walk")
      .with_animation(wave, "wave")
      .with_animation(jump, "jump")
      .with_animation(dance, "dance")
      .with_bidirectional("idle", "walk")
      .with_transition("idle", "wave")
      .with_transition("wave", "idle")
      .with_transition("idle", "jump")
      .with_transition("jump", "idle")
      .with_bidirectional("idle", "dance")
      .with_transition("walk", "idle")
      .with_default_blend_duration(0.15);

    Self {
      base: SceneBase::new(
        def.clone(),
        controller,
        Color::rgb(100, 200, 255),
        Color::rgb(255, 200, 100),
      ),
    }
  }
}

impl Default for CasualScene {
  fn default() -> Self {
    Self::new()
  }
}

impl DemoPage for CasualScene {
  fn name(&self) -> &str {
    "Casual"
  }

  fn description(&self) -> &str {
    "Relaxed character with walk, wave, jump and a toggled dance."
  }

  fn controls(&self) -> Vec<Control> {
    vec![
      Control::keys("AD", &[KeyCode::KeyA, KeyCode::KeyD], "Walk"),
      Control::key(KeyCode::Digit1, "Wave"),
      Control::key(KeyCode::Digit2, "Jump"),
      Control::key(KeyCode::Digit3, "Dance (toggle)"),
    ]
  }

  fn enter(&mut self, ctx: &mut SceneContext) {
    self.base.enter(ctx);
  }

  fn update(&mut self, ctx: &mut SceneContext, _dt: f32) {
    let Some(entity) = self.base.skeleton_entity else {
      return;
    };
    let input = &ctx.input;

    if let Some(mut ctrl) = ctx.world.get_mut::<SkeletalController>(entity) {
      let current = ctrl.current();

      // Digit1: Wave (one-shot)
      if input.key_pressed(KeyCode::Digit1) && current != "wave" && current != "jump" {
        ctrl.set_state_then("wave", "idle");
      }

      // Digit2: Jump (one-shot)
      if input.key_pressed(KeyCode::Digit2) && current != "wave" && current != "jump" {
        ctrl.set_state_then("jump", "idle");
      }

      // Digit3: Dance (toggle)
      let current = ctrl.current();
      if input.key_pressed(KeyCode::Digit3) && current != "wave" && current != "jump" {
        if current == "dance" {
          ctrl.set_state("idle");
        } else {
          ctrl.set_state("dance");
        }
      }

      // Walk: A or D held (only if not in a one-shot action or dance)
      let current = ctrl.current();
      if current != "wave" && current != "jump" && current != "dance" {
        if input.key_down(KeyCode::KeyA) || input.key_down(KeyCode::KeyD) {
          if current != "walk" {
            ctrl.set_state("walk");
          }
        } else if current == "walk" {
          ctrl.set_state("idle");
        }
      }

      self.base.cache_state(&ctrl);
    }

    self.base.redraw_debug_lines(ctx.world);
  }

  fn render_ui(&mut self, ui: &mut UiContext) {
    self.base.render_ui(ui);
  }
}
