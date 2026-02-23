//! State-machine-driven skeletal animation controller with crossfade blending.

use std::collections::HashMap;
use std::sync::Arc;

use crate::core::state_machine::StateMachine;

use super::animation::SkeletalAnimator;
use super::blend::SkeletalBlender;
use super::bone::{Pose, SkeletonDef};
use super::SkeletalAnimation;

/// Animation state identifier.
pub type SkeletalAnimationId = &'static str;

/// ECS component that manages skeletal animation states with crossfade blending.
///
/// Wraps a `StateMachine` for transition validation, a map of named animations,
/// a `SkeletalAnimator` for playback, and a `SkeletalBlender` for crossfading.
#[derive(Debug, Clone)]
pub struct SkeletalController {
    fsm: StateMachine<SkeletalAnimationId>,
    animations: HashMap<SkeletalAnimationId, Arc<SkeletalAnimation>>,
    animator: SkeletalAnimator,
    blender: SkeletalBlender,
    rest_pose: Pose,
    blend_durations: HashMap<(SkeletalAnimationId, SkeletalAnimationId), f32>,
    default_blend_duration: f32,
    pending_transition: Option<SkeletalAnimationId>,
}

impl SkeletalController {
    /// Create a new controller with an initial animation and state.
    pub fn new(
        initial_anim: Arc<SkeletalAnimation>,
        initial_id: SkeletalAnimationId,
        skeleton_def: Arc<SkeletonDef>,
    ) -> Self {
        let animator = SkeletalAnimator::new(initial_anim.clone());
        let rest_pose = Pose::from_rest(&skeleton_def);
        let mut animations = HashMap::new();
        animations.insert(initial_id, initial_anim);
        Self {
            fsm: StateMachine::new(initial_id),
            animations,
            animator,
            blender: SkeletalBlender::new(),
            rest_pose,
            blend_durations: HashMap::new(),
            default_blend_duration: 0.15,
            pending_transition: None,
        }
    }

    /// Register an animation under a state name.
    pub fn with_animation(mut self, anim: Arc<SkeletalAnimation>, id: SkeletalAnimationId) -> Self {
        self.animations.insert(id, anim);
        self
    }

    /// Allow a transition from one state to another.
    pub fn with_transition(
        mut self,
        from: SkeletalAnimationId,
        to: SkeletalAnimationId,
    ) -> Self {
        self.fsm = self.fsm.with_transition(from, to);
        self
    }

    /// Allow bidirectional transitions between two states.
    pub fn with_bidirectional(
        mut self,
        a: SkeletalAnimationId,
        b: SkeletalAnimationId,
    ) -> Self {
        self.fsm = self.fsm.with_bidirectional(a, b);
        self
    }

    /// Allow a state to be entered from any other state.
    pub fn with_transition_from_any(mut self, to: SkeletalAnimationId) -> Self {
        self.fsm = self.fsm.with_transition_from_any(to);
        self
    }

    /// Set a custom blend duration for a specific transition.
    pub fn with_blend_duration(
        mut self,
        from: SkeletalAnimationId,
        to: SkeletalAnimationId,
        duration: f32,
    ) -> Self {
        self.blend_durations.insert((from, to), duration);
        self
    }

    /// Set the default blend duration for transitions without a custom duration.
    pub fn with_default_blend_duration(mut self, duration: f32) -> Self {
        self.default_blend_duration = duration;
        self
    }

    /// Attempt to transition to a new state with crossfade.
    ///
    /// Returns `true` if the transition was allowed.
    pub fn set_state(&mut self, id: SkeletalAnimationId) -> bool {
        let from_id = *self.fsm.current();
        if self.fsm.transition_to(id).is_err() {
            return false;
        }
        self.pending_transition = None;
        self.apply_transition(from_id, id);
        true
    }

    /// Transition to a state, then auto-transition to `next_id` when it finishes.
    ///
    /// Returns `true` if the initial transition was allowed.
    pub fn set_state_then(
        &mut self,
        id: SkeletalAnimationId,
        next_id: SkeletalAnimationId,
    ) -> bool {
        let from_id = *self.fsm.current();
        if self.fsm.transition_to(id).is_err() {
            return false;
        }
        self.apply_transition(from_id, id);
        self.pending_transition = Some(next_id);
        true
    }

    /// Force a transition, bypassing FSM rules.
    pub fn force_state(&mut self, id: SkeletalAnimationId) {
        let from_id = *self.fsm.current();
        let _ = self.fsm.force_transition(id);
        self.apply_transition(from_id, id);
    }

    /// Get the current state name.
    pub fn current(&self) -> SkeletalAnimationId {
        self.fsm.current()
    }

    /// Whether a crossfade blend is active.
    pub fn is_blending(&self) -> bool {
        self.blender.is_blending()
    }

    /// Update animation playback and blending.
    pub fn update(&mut self, dt: f32) {
        self.fsm.update(dt);
        self.animator.update(dt);
        self.blender.update(dt);

        // Auto-transition when animation finishes
        if self.animator.just_finished()
            && let Some(next_id) = self.pending_transition.take()
        {
            let from_id = *self.fsm.current();
            let _ = self.fsm.force_transition(next_id);
            self.apply_transition(from_id, next_id);
        }
    }

    /// Get the current blend progress (0.0 = fully "from", 1.0 = fully "to").
    ///
    /// Returns 1.0 when not blending.
    pub fn blend_progress(&self) -> f32 {
        self.blender.blend_progress()
    }

    /// Sample the current animation and apply blending, producing the output pose.
    pub fn sample(&self) -> Pose {
        let raw_pose = self.animator.sample(&self.rest_pose);
        self.blender.apply(&raw_pose)
    }

    // --- Private ---

    fn apply_transition(&mut self, from_id: SkeletalAnimationId, to_id: SkeletalAnimationId) {
        // Snapshot current pose for blending
        let current_pose = self.sample();

        // Switch animator to new animation
        if let Some(anim) = self.animations.get(to_id) {
            self.animator = SkeletalAnimator::new(anim.clone());
        }

        // Start crossfade
        let blend_duration = self
            .blend_durations
            .get(&(from_id, to_id))
            .copied()
            .unwrap_or(self.default_blend_duration);
        self.blender.start_blend(current_pose, blend_duration);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::ecs::Ease;
    use crate::core::graphics::LoopMode;
    use crate::core::skeleton::BoneTransform;
    use glam::Vec2;

    fn test_skeleton() -> Arc<SkeletonDef> {
        Arc::new(
            SkeletonDef::builder()
                .bone("root", None, Vec2::ZERO, 0.0, 10.0)
                .bone("child", Some("root"), Vec2::new(0.0, 10.0), 0.0, 10.0)
                .build(),
        )
    }

    fn test_anim(
        name: &str,
        def: &Arc<SkeletonDef>,
        loop_mode: LoopMode,
    ) -> Arc<SkeletalAnimation> {
        Arc::new(
            SkeletalAnimation::builder(name, 1.0, def.clone())
                .loop_mode(loop_mode)
                .track("root", |t| {
                    t.at(0.0, BoneTransform::rotation(0.0), Ease::Linear)
                        .at(1.0, BoneTransform::rotation(1.0), Ease::Linear)
                })
                .build(),
        )
    }

    #[test]
    fn new_controller_starts_in_initial_state() {
        let def = test_skeleton();
        let idle = test_anim("idle", &def, LoopMode::Loop);
        let ctrl = SkeletalController::new(idle, "idle", def);
        assert_eq!(ctrl.current(), "idle");
    }

    #[test]
    fn set_state_transitions_with_valid_rule() {
        let def = test_skeleton();
        let idle = test_anim("idle", &def, LoopMode::Loop);
        let walk = test_anim("walk", &def, LoopMode::Loop);
        let mut ctrl = SkeletalController::new(idle, "idle", def.clone())
            .with_animation(walk, "walk")
            .with_transition("idle", "walk");
        assert!(ctrl.set_state("walk"));
        assert_eq!(ctrl.current(), "walk");
    }

    #[test]
    fn set_state_fails_without_rule() {
        let def = test_skeleton();
        let idle = test_anim("idle", &def, LoopMode::Loop);
        let walk = test_anim("walk", &def, LoopMode::Loop);
        let mut ctrl = SkeletalController::new(idle, "idle", def.clone())
            .with_animation(walk, "walk");
        assert!(!ctrl.set_state("walk"));
        assert_eq!(ctrl.current(), "idle");
    }

    #[test]
    fn set_state_starts_blend() {
        let def = test_skeleton();
        let idle = test_anim("idle", &def, LoopMode::Loop);
        let walk = test_anim("walk", &def, LoopMode::Loop);
        let mut ctrl = SkeletalController::new(idle, "idle", def.clone())
            .with_animation(walk, "walk")
            .with_transition("idle", "walk")
            .with_default_blend_duration(0.3);
        ctrl.set_state("walk");
        assert!(ctrl.is_blending());
    }

    #[test]
    fn custom_blend_duration_used() {
        let def = test_skeleton();
        let idle = test_anim("idle", &def, LoopMode::Loop);
        let walk = test_anim("walk", &def, LoopMode::Loop);
        let mut ctrl = SkeletalController::new(idle, "idle", def.clone())
            .with_animation(walk, "walk")
            .with_transition("idle", "walk")
            .with_default_blend_duration(0.5)
            .with_blend_duration("idle", "walk", 0.1);
        ctrl.set_state("walk");
        ctrl.update(0.1);
        assert!(!ctrl.is_blending());
    }

    #[test]
    fn set_state_then_auto_transitions() {
        let def = test_skeleton();
        let idle = test_anim("idle", &def, LoopMode::Loop);
        let attack = test_anim("attack", &def, LoopMode::Once);
        let mut ctrl = SkeletalController::new(idle, "idle", def.clone())
            .with_animation(attack, "attack")
            .with_transition("idle", "attack")
            .with_transition("attack", "idle")
            .with_default_blend_duration(0.0);
        ctrl.set_state_then("attack", "idle");
        assert_eq!(ctrl.current(), "attack");
        // Advance past the 1.0s animation duration
        ctrl.update(1.1);
        assert_eq!(ctrl.current(), "idle");
    }

    #[test]
    fn force_state_bypasses_rules() {
        let def = test_skeleton();
        let idle = test_anim("idle", &def, LoopMode::Loop);
        let walk = test_anim("walk", &def, LoopMode::Loop);
        let mut ctrl = SkeletalController::new(idle, "idle", def.clone())
            .with_animation(walk, "walk");
        ctrl.force_state("walk");
        assert_eq!(ctrl.current(), "walk");
    }

    #[test]
    fn sample_produces_pose() {
        let def = test_skeleton();
        let idle = test_anim("idle", &def, LoopMode::Loop);
        let ctrl = SkeletalController::new(idle, "idle", def.clone());
        let pose = ctrl.sample();
        assert_eq!(pose.transforms.len(), 2);
    }
}
