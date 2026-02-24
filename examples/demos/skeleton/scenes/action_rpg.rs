use std::sync::Arc;

use ryo2d::prelude::*;

use super::SceneBase;

fn create_rpg_idle(def: &Arc<SkeletonDef>) -> Arc<SkeletalAnimation> {
  Arc::new(
    SkeletalAnimation::builder("idle", 2.0, def.clone())
      .loop_mode(LoopMode::Loop)
      .track("spine", |t| {
        t.at(0.0, BoneTransform::rotation(0.0), Ease::InOutSine)
          .at(0.5, BoneTransform::rotation(0.02), Ease::InOutSine)
          .at(1.0, BoneTransform::rotation(0.0), Ease::InOutSine)
          .at(1.5, BoneTransform::rotation(-0.02), Ease::InOutSine)
          .at(2.0, BoneTransform::rotation(0.0), Ease::InOutSine)
      })
      .track("head", |t| {
        t.at(0.0, BoneTransform::rotation(0.0), Ease::InOutSine)
          .at(0.5, BoneTransform::rotation(-0.03), Ease::InOutSine)
          .at(1.0, BoneTransform::rotation(0.0), Ease::InOutSine)
          .at(1.5, BoneTransform::rotation(0.03), Ease::InOutSine)
          .at(2.0, BoneTransform::rotation(0.0), Ease::InOutSine)
      })
      .track("l_upper_arm", |t| {
        t.at(0.0, BoneTransform::rotation(0.0), Ease::InOutSine)
          .at(1.0, BoneTransform::rotation(0.08), Ease::InOutSine)
          .at(2.0, BoneTransform::rotation(0.0), Ease::InOutSine)
      })
      .track("r_upper_arm", |t| {
        t.at(0.0, BoneTransform::rotation(0.0), Ease::InOutSine)
          .at(1.0, BoneTransform::rotation(-0.08), Ease::InOutSine)
          .at(2.0, BoneTransform::rotation(0.0), Ease::InOutSine)
      })
      .build(),
  )
}

fn create_rpg_run(def: &Arc<SkeletonDef>) -> Arc<SkeletalAnimation> {
  Arc::new(
    SkeletalAnimation::builder("run", 0.6, def.clone())
      .loop_mode(LoopMode::Loop)
      .track("l_upper_arm", |t| {
        t.at(0.0, BoneTransform::rotation(0.0), Ease::InOutSine)
          .at(0.15, BoneTransform::rotation(0.6), Ease::InOutSine)
          .at(0.3, BoneTransform::rotation(0.0), Ease::InOutSine)
          .at(0.45, BoneTransform::rotation(-0.6), Ease::InOutSine)
          .at(0.6, BoneTransform::rotation(0.0), Ease::InOutSine)
      })
      .track("r_upper_arm", |t| {
        t.at(0.0, BoneTransform::rotation(0.0), Ease::InOutSine)
          .at(0.15, BoneTransform::rotation(-0.6), Ease::InOutSine)
          .at(0.3, BoneTransform::rotation(0.0), Ease::InOutSine)
          .at(0.45, BoneTransform::rotation(0.6), Ease::InOutSine)
          .at(0.6, BoneTransform::rotation(0.0), Ease::InOutSine)
      })
      .track("l_forearm", |t| {
        t.at(0.0, BoneTransform::rotation(0.0), Ease::InOutSine)
          .at(0.15, BoneTransform::rotation(-0.3), Ease::InOutSine)
          .at(0.3, BoneTransform::rotation(0.0), Ease::InOutSine)
          .at(0.45, BoneTransform::rotation(-0.3), Ease::InOutSine)
          .at(0.6, BoneTransform::rotation(0.0), Ease::InOutSine)
      })
      .track("r_forearm", |t| {
        t.at(0.0, BoneTransform::rotation(0.0), Ease::InOutSine)
          .at(0.15, BoneTransform::rotation(-0.3), Ease::InOutSine)
          .at(0.3, BoneTransform::rotation(0.0), Ease::InOutSine)
          .at(0.45, BoneTransform::rotation(-0.3), Ease::InOutSine)
          .at(0.6, BoneTransform::rotation(0.0), Ease::InOutSine)
      })
      .track("l_thigh", |t| {
        t.at(0.0, BoneTransform::rotation(0.0), Ease::InOutSine)
          .at(0.15, BoneTransform::rotation(-0.5), Ease::InOutSine)
          .at(0.3, BoneTransform::rotation(0.0), Ease::InOutSine)
          .at(0.45, BoneTransform::rotation(0.5), Ease::InOutSine)
          .at(0.6, BoneTransform::rotation(0.0), Ease::InOutSine)
      })
      .track("r_thigh", |t| {
        t.at(0.0, BoneTransform::rotation(0.0), Ease::InOutSine)
          .at(0.15, BoneTransform::rotation(0.5), Ease::InOutSine)
          .at(0.3, BoneTransform::rotation(0.0), Ease::InOutSine)
          .at(0.45, BoneTransform::rotation(-0.5), Ease::InOutSine)
          .at(0.6, BoneTransform::rotation(0.0), Ease::InOutSine)
      })
      .track("spine", |t| {
        t.at(0.0, BoneTransform::rotation(0.0), Ease::InOutSine)
          .at(0.15, BoneTransform::rotation(0.05), Ease::InOutSine)
          .at(0.3, BoneTransform::rotation(0.0), Ease::InOutSine)
          .at(0.45, BoneTransform::rotation(-0.05), Ease::InOutSine)
          .at(0.6, BoneTransform::rotation(0.0), Ease::InOutSine)
      })
      .build(),
  )
}

fn create_rpg_attack(def: &Arc<SkeletonDef>) -> Arc<SkeletalAnimation> {
  Arc::new(
    SkeletalAnimation::builder("attack", 0.5, def.clone())
      .loop_mode(LoopMode::Once)
      .track("r_upper_arm", |t| {
        t.at(0.0, BoneTransform::rotation(0.0), Ease::OutQuad)
          .at(0.1, BoneTransform::rotation(0.5), Ease::OutQuad)
          .at(0.25, BoneTransform::rotation(-2.0), Ease::OutQuad)
          .at(0.35, BoneTransform::rotation(-2.0), Ease::OutQuad)
          .at(0.5, BoneTransform::rotation(0.0), Ease::OutQuad)
      })
      .track("r_forearm", |t| {
        t.at(0.0, BoneTransform::rotation(0.0), Ease::OutQuad)
          .at(0.1, BoneTransform::rotation(0.3), Ease::OutQuad)
          .at(0.25, BoneTransform::rotation(-0.5), Ease::OutQuad)
          .at(0.35, BoneTransform::rotation(-0.5), Ease::OutQuad)
          .at(0.5, BoneTransform::rotation(0.0), Ease::OutQuad)
      })
      .track("spine", |t| {
        t.at(0.0, BoneTransform::rotation(0.0), Ease::InOutSine)
          .at(0.1, BoneTransform::rotation(-0.1), Ease::InOutSine)
          .at(0.25, BoneTransform::rotation(0.15), Ease::InOutSine)
          .at(0.35, BoneTransform::rotation(0.15), Ease::InOutSine)
          .at(0.5, BoneTransform::rotation(0.0), Ease::InOutSine)
      })
      .track("l_upper_arm", |t| {
        t.at(0.0, BoneTransform::rotation(0.0), Ease::InOutSine)
          .at(0.1, BoneTransform::rotation(0.0), Ease::InOutSine)
          .at(0.25, BoneTransform::rotation(0.3), Ease::InOutSine)
          .at(0.35, BoneTransform::rotation(0.3), Ease::InOutSine)
          .at(0.5, BoneTransform::rotation(0.0), Ease::InOutSine)
      })
      .build(),
  )
}

fn create_rpg_dodge(def: &Arc<SkeletonDef>) -> Arc<SkeletalAnimation> {
  Arc::new(
    SkeletalAnimation::builder("dodge", 0.3, def.clone())
      .loop_mode(LoopMode::Once)
      .track("spine", |t| {
        t.at(0.0, BoneTransform::rotation(0.0), Ease::InOutSine)
          .at(0.15, BoneTransform::rotation(-0.2), Ease::InOutSine)
          .at(0.3, BoneTransform::rotation(0.0), Ease::InOutSine)
      })
      .track("l_thigh", |t| {
        t.at(0.0, BoneTransform::rotation(0.0), Ease::InOutSine)
          .at(0.15, BoneTransform::rotation(0.4), Ease::InOutSine)
          .at(0.3, BoneTransform::rotation(0.0), Ease::InOutSine)
      })
      .track("r_thigh", |t| {
        t.at(0.0, BoneTransform::rotation(0.0), Ease::InOutSine)
          .at(0.15, BoneTransform::rotation(0.4), Ease::InOutSine)
          .at(0.3, BoneTransform::rotation(0.0), Ease::InOutSine)
      })
      .track("l_shin", |t| {
        t.at(0.0, BoneTransform::rotation(0.0), Ease::InOutSine)
          .at(0.15, BoneTransform::rotation(-0.4), Ease::InOutSine)
          .at(0.3, BoneTransform::rotation(0.0), Ease::InOutSine)
      })
      .track("r_shin", |t| {
        t.at(0.0, BoneTransform::rotation(0.0), Ease::InOutSine)
          .at(0.15, BoneTransform::rotation(-0.4), Ease::InOutSine)
          .at(0.3, BoneTransform::rotation(0.0), Ease::InOutSine)
      })
      .track("head", |t| {
        t.at(0.0, BoneTransform::rotation(0.0), Ease::InOutSine)
          .at(0.15, BoneTransform::rotation(-0.1), Ease::InOutSine)
          .at(0.3, BoneTransform::rotation(0.0), Ease::InOutSine)
      })
      .build(),
  )
}

fn create_rpg_victory(def: &Arc<SkeletonDef>) -> Arc<SkeletalAnimation> {
  Arc::new(
    SkeletalAnimation::builder("victory", 2.0, def.clone())
      .loop_mode(LoopMode::Once)
      .track("l_upper_arm", |t| {
        t.at(0.0, BoneTransform::rotation(0.0), Ease::InOutSine)
          .at(0.3, BoneTransform::rotation(0.0), Ease::InOutSine)
          .at(0.6, BoneTransform::rotation(-1.5), Ease::InOutSine)
          .at(1.5, BoneTransform::rotation(-1.5), Ease::InOutSine)
          .at(2.0, BoneTransform::rotation(0.0), Ease::InOutSine)
      })
      .track("r_upper_arm", |t| {
        t.at(0.0, BoneTransform::rotation(0.0), Ease::InOutSine)
          .at(0.3, BoneTransform::rotation(0.0), Ease::InOutSine)
          .at(0.6, BoneTransform::rotation(1.5), Ease::InOutSine)
          .at(1.5, BoneTransform::rotation(1.5), Ease::InOutSine)
          .at(2.0, BoneTransform::rotation(0.0), Ease::InOutSine)
      })
      .track("l_forearm", |t| {
        t.at(0.0, BoneTransform::rotation(0.0), Ease::InOutSine)
          .at(0.3, BoneTransform::rotation(0.0), Ease::InOutSine)
          .at(0.6, BoneTransform::rotation(-0.3), Ease::InOutSine)
          .at(1.5, BoneTransform::rotation(-0.3), Ease::InOutSine)
          .at(2.0, BoneTransform::rotation(0.0), Ease::InOutSine)
      })
      .track("r_forearm", |t| {
        t.at(0.0, BoneTransform::rotation(0.0), Ease::InOutSine)
          .at(0.3, BoneTransform::rotation(0.0), Ease::InOutSine)
          .at(0.6, BoneTransform::rotation(0.3), Ease::InOutSine)
          .at(1.5, BoneTransform::rotation(0.3), Ease::InOutSine)
          .at(2.0, BoneTransform::rotation(0.0), Ease::InOutSine)
      })
      .track("spine", |t| {
        t.at(0.0, BoneTransform::rotation(0.0), Ease::InOutSine)
          .at(0.3, BoneTransform::rotation(0.0), Ease::InOutSine)
          .at(0.6, BoneTransform::rotation(-0.05), Ease::InOutSine)
          .at(1.5, BoneTransform::rotation(-0.05), Ease::InOutSine)
          .at(2.0, BoneTransform::rotation(0.0), Ease::InOutSine)
      })
      .track("head", |t| {
        t.at(0.0, BoneTransform::rotation(0.0), Ease::InOutSine)
          .at(0.3, BoneTransform::rotation(0.0), Ease::InOutSine)
          .at(0.6, BoneTransform::rotation(-0.1), Ease::InOutSine)
          .at(1.5, BoneTransform::rotation(-0.1), Ease::InOutSine)
          .at(2.0, BoneTransform::rotation(0.0), Ease::InOutSine)
      })
      .build(),
  )
}

pub struct ActionRpgScene {
  base: SceneBase,
}

impl ActionRpgScene {
  pub fn new(def: &Arc<SkeletonDef>) -> Self {
    let idle = create_rpg_idle(def);
    let run = create_rpg_run(def);
    let attack = create_rpg_attack(def);
    let dodge = create_rpg_dodge(def);
    let victory = create_rpg_victory(def);

    let controller = SkeletalController::new(idle, "idle", def.clone())
      .with_animation(run, "run")
      .with_animation(attack, "attack")
      .with_animation(dodge, "dodge")
      .with_animation(victory, "victory")
      // idle <-> run
      .with_bidirectional("idle", "run")
      // idle -> attacks/actions
      .with_transition("idle", "attack")
      .with_transition("idle", "dodge")
      .with_transition("idle", "victory")
      // run -> attacks/actions
      .with_transition("run", "attack")
      .with_transition("run", "dodge")
      // actions -> idle
      .with_transition("attack", "idle")
      .with_transition("dodge", "idle")
      .with_transition("victory", "idle")
      // blend durations
      .with_default_blend_duration(0.15)
      .with_blend_duration("idle", "attack", 0.05)
      .with_blend_duration("run", "attack", 0.05)
      .with_blend_duration("idle", "dodge", 0.05)
      .with_blend_duration("run", "dodge", 0.05);

    Self {
      base: SceneBase::new(
        controller,
        Color::rgb(255, 100, 100),
        Color::rgb(255, 200, 50),
      ),
    }
  }

  pub fn enter(&mut self, world: &mut World, def: &Arc<SkeletonDef>) {
    self.base.enter(world, def);
  }

  pub fn exit(&mut self, world: &mut World) {
    self.base.exit(world);
  }

  pub fn update(&mut self, ctx: &mut SceneContext, _dt: f32) {
    let Some(entity) = self.base.skeleton_entity else {
      return;
    };
    let input = &ctx.input;

    if let Some(mut ctrl) = ctx.world.get_mut::<SkeletalController>(entity) {
      let current = ctrl.current();

      // Attack: J pressed (only when idle or running)
      if input.key_pressed(KeyCode::KeyJ) && (current == "idle" || current == "run") {
        ctrl.set_state_then("attack", "idle");
      }

      // Dodge: K pressed (only when idle or running)
      if input.key_pressed(KeyCode::KeyK) && (current == "idle" || current == "run") {
        ctrl.set_state_then("dodge", "idle");
      }

      // Victory: V pressed (only when idle)
      let current = ctrl.current();
      if input.key_pressed(KeyCode::KeyV) && current == "idle" {
        ctrl.set_state_then("victory", "idle");
      }

      // Run: A or D held (only if not in attack/dodge/victory)
      let current = ctrl.current();
      if current != "attack" && current != "dodge" && current != "victory" {
        if input.key_down(KeyCode::KeyA) || input.key_down(KeyCode::KeyD) {
          if current != "run" {
            ctrl.set_state("run");
          }
        } else if current == "run" {
          ctrl.set_state("idle");
        }
      }

      self.base.cache_state(&ctrl);
    }

    self.base.redraw_debug_lines(ctx.world);
  }

  pub fn render_ui(&self, ui: &mut UiContext) {
    self.base.render_ui(
      ui,
      "Action RPG Scene",
      "A/D: Run  J: Attack  K: Dodge  V: Victory",
    );
  }
}
