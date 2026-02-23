use std::sync::Arc;

use ryo2d::prelude::*;

use super::SceneBase;

pub(crate) fn create_fighter_idle(def: &Arc<SkeletonDef>) -> Arc<SkeletalAnimation> {
    Arc::new(
        SkeletalAnimation::builder("idle", 2.0, def.clone())
            .loop_mode(LoopMode::Loop)
            .track("spine", |t| {
                t.at(0.0, BoneTransform::rotation(0.0), Ease::InOutSine)
                    .at(1.0, BoneTransform::rotation(0.03), Ease::InOutSine)
                    .at(2.0, BoneTransform::rotation(0.0), Ease::InOutSine)
            })
            .track("head", |t| {
                t.at(0.0, BoneTransform::rotation(0.0), Ease::InOutSine)
                    .at(1.0, BoneTransform::rotation(-0.02), Ease::InOutSine)
                    .at(2.0, BoneTransform::rotation(0.0), Ease::InOutSine)
            })
            .track("l_upper_arm", |t| {
                t.at(0.0, BoneTransform::rotation(0.0), Ease::InOutSine)
                    .at(1.0, BoneTransform::rotation(0.05), Ease::InOutSine)
                    .at(2.0, BoneTransform::rotation(0.0), Ease::InOutSine)
            })
            .track("r_upper_arm", |t| {
                t.at(0.0, BoneTransform::rotation(0.0), Ease::InOutSine)
                    .at(1.0, BoneTransform::rotation(-0.05), Ease::InOutSine)
                    .at(2.0, BoneTransform::rotation(0.0), Ease::InOutSine)
            })
            .build(),
    )
}

fn create_fighter_walk(def: &Arc<SkeletonDef>) -> Arc<SkeletalAnimation> {
    Arc::new(
        SkeletalAnimation::builder("walk", 1.0, def.clone())
            .loop_mode(LoopMode::Loop)
            .track("l_upper_arm", |t| {
                t.at(0.0, BoneTransform::rotation(0.0), Ease::InOutSine)
                    .at(0.25, BoneTransform::rotation(0.4), Ease::InOutSine)
                    .at(0.5, BoneTransform::rotation(0.0), Ease::InOutSine)
                    .at(0.75, BoneTransform::rotation(-0.4), Ease::InOutSine)
                    .at(1.0, BoneTransform::rotation(0.0), Ease::InOutSine)
            })
            .track("r_upper_arm", |t| {
                t.at(0.0, BoneTransform::rotation(0.0), Ease::InOutSine)
                    .at(0.25, BoneTransform::rotation(-0.4), Ease::InOutSine)
                    .at(0.5, BoneTransform::rotation(0.0), Ease::InOutSine)
                    .at(0.75, BoneTransform::rotation(0.4), Ease::InOutSine)
                    .at(1.0, BoneTransform::rotation(0.0), Ease::InOutSine)
            })
            .track("l_thigh", |t| {
                t.at(0.0, BoneTransform::rotation(0.0), Ease::InOutSine)
                    .at(0.25, BoneTransform::rotation(-0.3), Ease::InOutSine)
                    .at(0.5, BoneTransform::rotation(0.0), Ease::InOutSine)
                    .at(0.75, BoneTransform::rotation(0.3), Ease::InOutSine)
                    .at(1.0, BoneTransform::rotation(0.0), Ease::InOutSine)
            })
            .track("r_thigh", |t| {
                t.at(0.0, BoneTransform::rotation(0.0), Ease::InOutSine)
                    .at(0.25, BoneTransform::rotation(0.3), Ease::InOutSine)
                    .at(0.5, BoneTransform::rotation(0.0), Ease::InOutSine)
                    .at(0.75, BoneTransform::rotation(-0.3), Ease::InOutSine)
                    .at(1.0, BoneTransform::rotation(0.0), Ease::InOutSine)
            })
            .track("spine", |t| {
                t.at(0.0, BoneTransform::rotation(0.0), Ease::InOutSine)
                    .at(0.25, BoneTransform::rotation(0.02), Ease::InOutSine)
                    .at(0.5, BoneTransform::rotation(0.0), Ease::InOutSine)
                    .at(0.75, BoneTransform::rotation(-0.02), Ease::InOutSine)
                    .at(1.0, BoneTransform::rotation(0.0), Ease::InOutSine)
            })
            .build(),
    )
}

fn create_fighter_punch(def: &Arc<SkeletonDef>) -> Arc<SkeletalAnimation> {
    Arc::new(
        SkeletalAnimation::builder("punch", 0.4, def.clone())
            .loop_mode(LoopMode::Once)
            .track("r_upper_arm", |t| {
                t.at(0.0, BoneTransform::rotation(0.0), Ease::OutQuad)
                    .at(0.1, BoneTransform::rotation(-1.2), Ease::OutQuad)
                    .at(0.25, BoneTransform::rotation(-1.2), Ease::OutQuad)
                    .at(0.4, BoneTransform::rotation(0.0), Ease::OutQuad)
            })
            .track("r_forearm", |t| {
                t.at(0.0, BoneTransform::rotation(0.0), Ease::OutQuad)
                    .at(0.1, BoneTransform::rotation(-0.3), Ease::OutQuad)
                    .at(0.25, BoneTransform::rotation(-0.3), Ease::OutQuad)
                    .at(0.4, BoneTransform::rotation(0.0), Ease::OutQuad)
            })
            .track("spine", |t| {
                t.at(0.0, BoneTransform::rotation(0.0), Ease::InOutSine)
                    .at(0.1, BoneTransform::rotation(0.1), Ease::InOutSine)
                    .at(0.25, BoneTransform::rotation(0.1), Ease::InOutSine)
                    .at(0.4, BoneTransform::rotation(0.0), Ease::InOutSine)
            })
            .build(),
    )
}

fn create_fighter_kick(def: &Arc<SkeletonDef>) -> Arc<SkeletalAnimation> {
    Arc::new(
        SkeletalAnimation::builder("kick", 0.5, def.clone())
            .loop_mode(LoopMode::Once)
            .track("r_thigh", |t| {
                t.at(0.0, BoneTransform::rotation(0.0), Ease::OutQuad)
                    .at(0.15, BoneTransform::rotation(-0.8), Ease::OutQuad)
                    .at(0.3, BoneTransform::rotation(-0.8), Ease::OutQuad)
                    .at(0.5, BoneTransform::rotation(0.0), Ease::OutQuad)
            })
            .track("r_shin", |t| {
                t.at(0.0, BoneTransform::rotation(0.0), Ease::OutQuad)
                    .at(0.15, BoneTransform::rotation(0.4), Ease::OutQuad)
                    .at(0.3, BoneTransform::rotation(0.4), Ease::OutQuad)
                    .at(0.5, BoneTransform::rotation(0.0), Ease::OutQuad)
            })
            .track("spine", |t| {
                t.at(0.0, BoneTransform::rotation(0.0), Ease::InOutSine)
                    .at(0.15, BoneTransform::rotation(-0.05), Ease::InOutSine)
                    .at(0.3, BoneTransform::rotation(-0.05), Ease::InOutSine)
                    .at(0.5, BoneTransform::rotation(0.0), Ease::InOutSine)
            })
            .build(),
    )
}

fn create_fighter_block(def: &Arc<SkeletonDef>) -> Arc<SkeletalAnimation> {
    Arc::new(
        SkeletalAnimation::builder("block", 0.3, def.clone())
            .loop_mode(LoopMode::Once)
            .track("l_upper_arm", |t| {
                t.at(0.0, BoneTransform::rotation(0.0), Ease::OutQuad)
                    .at(0.15, BoneTransform::rotation(-0.8), Ease::OutQuad)
                    .at(0.3, BoneTransform::rotation(-0.8), Ease::OutQuad)
            })
            .track("r_upper_arm", |t| {
                t.at(0.0, BoneTransform::rotation(0.0), Ease::OutQuad)
                    .at(0.15, BoneTransform::rotation(0.8), Ease::OutQuad)
                    .at(0.3, BoneTransform::rotation(0.8), Ease::OutQuad)
            })
            .track("l_forearm", |t| {
                t.at(0.0, BoneTransform::rotation(0.0), Ease::OutQuad)
                    .at(0.15, BoneTransform::rotation(-0.5), Ease::OutQuad)
                    .at(0.3, BoneTransform::rotation(-0.5), Ease::OutQuad)
            })
            .track("r_forearm", |t| {
                t.at(0.0, BoneTransform::rotation(0.0), Ease::OutQuad)
                    .at(0.15, BoneTransform::rotation(0.5), Ease::OutQuad)
                    .at(0.3, BoneTransform::rotation(0.5), Ease::OutQuad)
            })
            .build(),
    )
}

pub struct FighterScene {
    base: SceneBase,
}

impl FighterScene {
    pub fn new(def: &Arc<SkeletonDef>) -> Self {
        let idle = create_fighter_idle(def);
        let walk = create_fighter_walk(def);
        let punch = create_fighter_punch(def);
        let kick = create_fighter_kick(def);
        let block = create_fighter_block(def);

        let controller = SkeletalController::new(idle, "idle", def.clone())
            .with_animation(walk, "walk")
            .with_animation(punch, "punch")
            .with_animation(kick, "kick")
            .with_animation(block, "block")
            // idle <-> walk
            .with_bidirectional("idle", "walk")
            // idle -> attacks
            .with_transition("idle", "punch")
            .with_transition("idle", "kick")
            .with_transition("idle", "block")
            // walk -> attacks
            .with_transition("walk", "punch")
            .with_transition("walk", "kick")
            .with_transition("walk", "block")
            // attacks -> idle
            .with_transition("punch", "idle")
            .with_transition("kick", "idle")
            .with_transition("block", "idle")
            // blend durations
            .with_default_blend_duration(0.15)
            .with_blend_duration("idle", "punch", 0.05)
            .with_blend_duration("idle", "kick", 0.05)
            .with_blend_duration("walk", "punch", 0.05)
            .with_blend_duration("walk", "kick", 0.05);

        Self {
            base: SceneBase::new(
                controller,
                Color::rgb(100, 255, 100),
                Color::rgb(255, 255, 100),
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

            // Block: L held
            if input.key_down(KeyCode::KeyL) {
                if current != "block" {
                    ctrl.set_state("block");
                }
            } else if current == "block" {
                ctrl.set_state("idle");
            }

            // Only process movement/attack input if not blocking
            let current = ctrl.current();
            if current != "block" {
                // Punch: J pressed (only if not already in an action)
                if input.key_pressed(KeyCode::KeyJ)
                    && current != "punch"
                    && current != "kick"
                {
                    ctrl.set_state_then("punch", "idle");
                }

                // Kick: K pressed (only if not already in an action)
                if input.key_pressed(KeyCode::KeyK)
                    && current != "punch"
                    && current != "kick"
                {
                    ctrl.set_state_then("kick", "idle");
                }

                // Walk: A or D held (only if not in attack)
                let current = ctrl.current();
                if current != "punch" && current != "kick" {
                    if input.key_down(KeyCode::KeyA) || input.key_down(KeyCode::KeyD) {
                        if current != "walk" {
                            ctrl.set_state("walk");
                        }
                    } else if current == "walk" {
                        ctrl.set_state("idle");
                    }
                }
            }

            self.base.cache_state(&ctrl);
        }

        self.base.redraw_debug_lines(ctx.world);
    }

    pub fn render_ui(&self, ui: &mut UiContext) {
        self.base
            .render_ui(ui, "Fighter Scene", "A/D: Walk  J: Punch  K: Kick  L: Block");
    }
}
