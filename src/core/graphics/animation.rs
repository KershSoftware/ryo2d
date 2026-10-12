//! Sprite animation system for Ryo2D
//!
//! This module provides a flexible animation API:
//! - [`Animation`] - A sequence of sprite frames with timing
//! - [`Animator`] - Simple ECS component for playing animations
//! - [`AnimationController`] - State machine for complex character animations
//!
//! # Simple Animation
//!
//! For basic looping animations (spinning coins, flickering flames):
//!
//! ```ignore
//! let spin = Animation::new("spin", vec![frame1, frame2, frame3], 0.1);
//!
//! ctx.world.spawn((
//!     Transform::from_xy(x, y),
//!     Renderable::sprite(frame1),
//!     Animator::new(spin),
//! ));
//! // Engine handles frame timing automatically
//! ```
//!
//! # State-Based Animation
//!
//! For characters with multiple animation states:
//!
//! ```ignore
//! let mut controller = AnimationController::new();
//! controller.add_animation("idle", idle_anim);
//! controller.add_animation("walk", walk_anim);
//! controller.add_animation("attack", attack_anim.with_loop_mode(LoopMode::Once));
//! controller.set_state("idle");
//!
//! ctx.world.spawn((Transform::from_xy(x, y), Renderable::sprite(idle_frame), controller));
//!
//! // In game logic, set the state on the entity's controller
//! if let Some(mut controller) = ctx.world.get_mut::<AnimationController>(player) {
//!     controller.set_state("walk");
//! }
//! ```

use crate::core::ecs::Component;
use std::collections::HashMap;

use crate::core::asset::SpriteHandle;
use crate::core::state_machine::StateMachine;

/// How an animation behaves when it reaches the end
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LoopMode {
  /// Play once and stop on the last frame
  Once,
  /// Loop forever
  #[default]
  Loop,
  /// Play forward, then backward, repeat
  PingPong,
  /// Loop a specific number of times
  Times(u32),
}

/// A sequence of sprite frames with timing
#[derive(Clone)]
pub struct Animation {
  frames: Vec<SpriteHandle>,
  frame_durations: Vec<f32>,
  loop_mode: LoopMode,
  name: String,
}

impl Animation {
  /// Create a new animation with uniform frame timing
  ///
  /// # Arguments
  /// * `name` - Name for identification/debugging
  /// * `frames` - Sprite handles for each frame
  /// * `frame_duration` - Duration of each frame in seconds
  pub fn new(name: impl Into<String>, frames: Vec<SpriteHandle>, frame_duration: f32) -> Self {
    let count = frames.len();
    Animation {
      frames,
      frame_durations: vec![frame_duration; count],
      loop_mode: LoopMode::Loop,
      name: name.into(),
    }
  }

  /// Create animation with per-frame timing
  ///
  /// Useful when some frames need to linger longer than others.
  pub fn with_frame_durations(
    name: impl Into<String>,
    frames: Vec<SpriteHandle>,
    durations: Vec<f32>,
  ) -> Self {
    assert_eq!(
      frames.len(),
      durations.len(),
      "Frame count must match duration count"
    );
    Animation {
      frames,
      frame_durations: durations,
      loop_mode: LoopMode::Loop,
      name: name.into(),
    }
  }

  /// Set the loop mode
  pub fn with_loop_mode(mut self, mode: LoopMode) -> Self {
    self.loop_mode = mode;
    self
  }

  /// Get the animation name
  pub fn name(&self) -> &str {
    &self.name
  }

  /// Get the total duration of one cycle
  pub fn duration(&self) -> f32 {
    self.frame_durations.iter().sum()
  }

  /// Get frame count
  pub fn frame_count(&self) -> usize {
    self.frames.len()
  }

  /// Get a frame by index
  pub fn frame(&self, index: usize) -> Option<&SpriteHandle> {
    self.frames.get(index)
  }

  /// Get frame duration by index
  pub fn frame_duration(&self, index: usize) -> f32 {
    self.frame_durations.get(index).copied().unwrap_or(0.0)
  }

  /// Get the loop mode
  pub fn loop_mode(&self) -> LoopMode {
    self.loop_mode
  }
}

/// Playback state for an animation
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PlaybackState {
  #[default]
  Playing,
  Paused,
  Stopped,
}

/// ECS component for playing animations on an entity
///
/// Attach this to any entity with a [`Renderable`](crate::core::ecs::Renderable).
/// The engine advances it each fixed step and puts the current frame on the
/// entity's `Renderable`.
#[derive(Component, Clone)]
pub struct Animator {
  animation: Animation,
  current_frame: usize,
  time_in_frame: f32,
  state: PlaybackState,
  speed: f32,
  forward: bool,
  loops_remaining: Option<u32>,
  just_finished: bool,
}

impl Animator {
  /// Create a new animator with the given animation
  pub fn new(animation: Animation) -> Self {
    let loops = match animation.loop_mode {
      LoopMode::Times(n) => Some(n),
      _ => None,
    };
    Animator {
      animation,
      current_frame: 0,
      time_in_frame: 0.0,
      state: PlaybackState::Playing,
      speed: 1.0,
      forward: true,
      loops_remaining: loops,
      just_finished: false,
    }
  }

  /// Set a new animation, resetting playback
  pub fn set_animation(&mut self, animation: Animation) {
    let loops = match animation.loop_mode {
      LoopMode::Times(n) => Some(n),
      _ => None,
    };
    self.animation = animation;
    self.current_frame = 0;
    self.time_in_frame = 0.0;
    self.forward = true;
    self.loops_remaining = loops;
    self.just_finished = false;
  }

  /// Play the animation
  pub fn play(&mut self) {
    self.state = PlaybackState::Playing;
  }

  /// Pause the animation
  pub fn pause(&mut self) {
    self.state = PlaybackState::Paused;
  }

  /// Stop and reset to first frame
  pub fn stop(&mut self) {
    self.state = PlaybackState::Stopped;
    self.current_frame = 0;
    self.time_in_frame = 0.0;
    self.forward = true;
  }

  /// Reset to first frame without changing playback state
  pub fn reset(&mut self) {
    self.current_frame = 0;
    self.time_in_frame = 0.0;
    self.forward = true;
  }

  /// Set playback speed (1.0 = normal, 2.0 = double speed)
  pub fn set_speed(&mut self, speed: f32) {
    self.speed = speed.max(0.0);
  }

  /// Get current playback speed
  pub fn speed(&self) -> f32 {
    self.speed
  }

  /// Check if currently playing
  pub fn is_playing(&self) -> bool {
    self.state == PlaybackState::Playing
  }

  /// Check if animation just finished (one-shot or limited loops)
  ///
  /// This flag is set for one update (one fixed step for engine-driven
  /// animators) when the animation completes.
  pub fn just_finished(&self) -> bool {
    self.just_finished
  }

  /// Get the current frame's sprite handle
  pub fn current_sprite(&self) -> Option<&SpriteHandle> {
    self.animation.frame(self.current_frame)
  }

  /// Get current frame index
  pub fn current_frame_index(&self) -> usize {
    self.current_frame
  }

  /// Get the animation name
  pub fn animation_name(&self) -> &str {
    self.animation.name()
  }

  /// Update the animation (called by the engine each fixed step)
  pub fn update(&mut self, dt: f32) {
    self.just_finished = false;

    if self.state != PlaybackState::Playing {
      return;
    }

    if self.animation.frames.is_empty() {
      return;
    }

    self.time_in_frame += dt * self.speed;

    while self.time_in_frame >= self.animation.frame_duration(self.current_frame) {
      self.time_in_frame -= self.animation.frame_duration(self.current_frame);

      if !self.advance_frame() {
        self.just_finished = true;
        self.state = PlaybackState::Stopped;
        break;
      }
    }
  }

  fn advance_frame(&mut self) -> bool {
    let frame_count = self.animation.frames.len();

    match self.animation.loop_mode {
      LoopMode::Once => {
        if self.current_frame + 1 >= frame_count {
          false
        } else {
          self.current_frame += 1;
          true
        }
      }
      LoopMode::Loop => {
        self.current_frame = (self.current_frame + 1) % frame_count;
        true
      }
      LoopMode::PingPong => {
        if self.forward {
          if self.current_frame + 1 >= frame_count {
            self.forward = false;
            self.current_frame = frame_count.saturating_sub(2);
          } else {
            self.current_frame += 1;
          }
        } else if self.current_frame == 0 {
          self.forward = true;
          self.current_frame = 1.min(frame_count - 1);
        } else {
          self.current_frame -= 1;
        }
        true
      }
      LoopMode::Times(_) => {
        if self.current_frame + 1 >= frame_count {
          match &mut self.loops_remaining {
            Some(n) if *n > 1 => {
              *n -= 1;
              self.current_frame = 0;
              true
            }
            _ => false,
          }
        } else {
          self.current_frame += 1;
          true
        }
      }
    }
  }
}

/// A named reference to an animation state
pub type AnimationId = &'static str;

/// State machine for complex character animations
///
/// Use when you have multiple animation states (idle, walk, run, jump, attack)
/// that need to transition between each other.
#[derive(Component, Clone, Default)]
pub struct AnimationController {
  animations: HashMap<AnimationId, Animation>,
  current_state: Option<AnimationId>,
  animator: Option<Animator>,
  pending_transition: Option<AnimationId>,
}

impl AnimationController {
  /// Create a new empty animation controller
  pub fn new() -> Self {
    AnimationController {
      animations: HashMap::new(),
      current_state: None,
      animator: None,
      pending_transition: None,
    }
  }

  /// Add an animation to the controller
  pub fn add_animation(&mut self, id: AnimationId, animation: Animation) {
    self.animations.insert(id, animation);
  }

  /// Set the current animation state
  ///
  /// If already in this state, does nothing (preserves current frame).
  /// To force restart, call `reset()` first.
  pub fn set_state(&mut self, state: AnimationId) {
    if self.current_state == Some(state) {
      return;
    }

    self.pending_transition = None;

    if let Some(anim) = self.animations.get(state).cloned() {
      self.current_state = Some(state);
      self.animator = Some(Animator::new(anim));
    }
  }

  /// Transition to a state after the current animation finishes
  ///
  /// Useful for attack -> idle, jump -> fall transitions
  pub fn transition_after_finish(&mut self, state: AnimationId) {
    self.pending_transition = Some(state);
  }

  /// Get current state name
  pub fn current_state(&self) -> Option<AnimationId> {
    self.current_state
  }

  /// Check if the current animation just finished
  pub fn just_finished(&self) -> bool {
    self.animator.as_ref().is_some_and(|a| a.just_finished())
  }

  /// Get current sprite handle
  pub fn current_sprite(&self) -> Option<&SpriteHandle> {
    self.animator.as_ref().and_then(|a| a.current_sprite())
  }

  /// Reset current animation to start
  pub fn reset(&mut self) {
    if let Some(animator) = &mut self.animator {
      animator.reset();
    }
  }

  /// Pause animation
  pub fn pause(&mut self) {
    if let Some(animator) = &mut self.animator {
      animator.pause();
    }
  }

  /// Resume animation
  pub fn play(&mut self) {
    if let Some(animator) = &mut self.animator {
      animator.play();
    }
  }

  /// Set playback speed
  pub fn set_speed(&mut self, speed: f32) {
    if let Some(animator) = &mut self.animator {
      animator.set_speed(speed);
    }
  }

  /// Update the controller (called by the engine each fixed step)
  pub fn update(&mut self, dt: f32) {
    if let Some(animator) = &mut self.animator {
      animator.update(dt);

      if animator.just_finished()
        && let Some(next_state) = self.pending_transition.take()
      {
        self.set_state(next_state);
      }
    }
  }
}

/// Animation controller with validated state transitions.
///
/// Combines [`StateMachine`] validation with [`AnimationController`] playback.
/// Use this when you need to enforce rules about which animation states can
/// transition to which others (e.g., fighting games where attack recovery
/// must return to idle before another attack).
///
/// For simpler cases where any animation can transition to any other,
/// use [`AnimationController`] directly.
///
/// # Example
///
/// ```ignore
/// use ryo2d::prelude::*;
///
/// let mut anim = ValidatedAnimator::new(idle_anim, "idle")
///     .with_animation(walk_anim, "walk")
///     .with_animation(attack_anim, "attack")
///     .with_animation(hurt_anim, "hurt")
///     // Define allowed transitions
///     .with_transition("idle", "walk")
///     .with_transition("walk", "idle")
///     .with_transition("idle", "attack")
///     .with_transition("walk", "attack")
///     .with_transition("attack", "idle")
///     // Hurt can be entered from any state
///     .with_transition_from_any("hurt");
///
/// // In game logic
/// if input.attack_pressed {
///     anim.set_animation("attack");  // Only works from idle/walk
/// }
///
/// if took_damage {
///     anim.set_animation("hurt");  // Always allowed
/// }
/// ```
#[derive(Component, Clone)]
pub struct ValidatedAnimator {
  fsm: StateMachine<AnimationId>,
  controller: AnimationController,
}

impl ValidatedAnimator {
  /// Create a new validated animator with an initial animation.
  pub fn new(initial_anim: Animation, initial_id: AnimationId) -> Self {
    let mut controller = AnimationController::new();
    controller.add_animation(initial_id, initial_anim);
    controller.set_state(initial_id);

    Self {
      fsm: StateMachine::new(initial_id),
      controller,
    }
  }

  /// Add an animation to the controller.
  pub fn with_animation(mut self, anim: Animation, id: AnimationId) -> Self {
    self.controller.add_animation(id, anim);
    self
  }

  /// Allow a transition from one animation state to another.
  pub fn with_transition(mut self, from: AnimationId, to: AnimationId) -> Self {
    self.fsm = self.fsm.with_transition(from, to);
    self
  }

  /// Allow bidirectional transitions between two animation states.
  pub fn with_bidirectional(mut self, a: AnimationId, b: AnimationId) -> Self {
    self.fsm = self.fsm.with_bidirectional(a, b);
    self
  }

  /// Allow a state to be entered from any other state.
  ///
  /// Useful for states like "hurt" or "death" that can interrupt anything.
  pub fn with_transition_from_any(mut self, to: AnimationId) -> Self {
    self.fsm = self.fsm.with_transition_from_any(to);
    self
  }

  /// Attempt to transition to a new animation state.
  ///
  /// Returns `true` if the transition was allowed and executed,
  /// `false` if the transition is not permitted by the defined rules.
  pub fn set_animation(&mut self, id: AnimationId) -> bool {
    if self.fsm.transition_to(id).is_ok() {
      self.controller.set_state(id);
      true
    } else {
      false
    }
  }

  /// Force a transition to an animation state, bypassing validation.
  ///
  /// Useful for reset/respawn scenarios where normal rules don't apply.
  pub fn force_animation(&mut self, id: AnimationId) {
    let _ = self.fsm.force_transition(id);
    self.controller.set_state(id);
  }

  /// Queue a transition to occur when the current animation finishes.
  ///
  /// Returns `true` if the transition is allowed, `false` otherwise.
  /// The actual state change happens when the animation completes.
  pub fn transition_after_finish(&mut self, id: AnimationId) -> bool {
    if self.fsm.can_transition_to(&id) {
      self.controller.transition_after_finish(id);
      true
    } else {
      false
    }
  }

  /// Get the current animation state.
  pub fn current(&self) -> AnimationId {
    self.fsm.current()
  }

  /// Get the current sprite handle for rendering.
  pub fn current_sprite(&self) -> Option<&SpriteHandle> {
    self.controller.current_sprite()
  }

  /// Check if the FSM just entered the current state this frame.
  pub fn just_entered(&self) -> bool {
    self.fsm.just_entered
  }

  /// Check if the current animation just finished this frame.
  pub fn just_finished(&self) -> bool {
    self.controller.just_finished()
  }

  /// Check if a transition to a state is allowed.
  pub fn can_transition_to(&self, id: AnimationId) -> bool {
    self.fsm.can_transition_to(&id)
  }

  /// Get time spent in the current animation state (seconds).
  pub fn time_in_state(&self) -> f32 {
    self.fsm.time_in_state()
  }

  /// Pause the animation playback.
  pub fn pause(&mut self) {
    self.controller.pause();
  }

  /// Resume the animation playback.
  pub fn play(&mut self) {
    self.controller.play();
  }

  /// Set playback speed (1.0 = normal, 2.0 = double speed).
  pub fn set_speed(&mut self, speed: f32) {
    self.controller.set_speed(speed);
  }

  /// Reset the current animation to its first frame.
  pub fn reset(&mut self) {
    self.controller.reset();
  }

  /// Update the animator. The engine does not drive `ValidatedAnimator`:
  /// call this from `Scene::update` and copy `current_sprite()` to the
  /// entity's `Renderable`.
  pub fn update(&mut self, dt: f32) {
    self.fsm.update(dt);

    // Check if controller will transition after finish
    let prev_state = self.controller.current_state();
    self.controller.update(dt);
    let new_state = self.controller.current_state();

    // Sync FSM if controller transitioned (from transition_after_finish)
    if prev_state != new_state
      && let Some(new_id) = new_state
    {
      let _ = self.fsm.force_transition(new_id);
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  fn mock_sprite_handle() -> SpriteHandle {
    use crate::core::graphics::Sprite;
    SpriteHandle::from_sprite(Sprite::solid_rect(1, 1, 255, 0, 0, 255), "test.png")
  }

  #[test]
  fn animation_creation() {
    let frames = vec![mock_sprite_handle(), mock_sprite_handle()];
    let anim = Animation::new("test", frames, 0.1);
    assert_eq!(anim.name(), "test");
    assert_eq!(anim.frame_count(), 2);
    assert_eq!(anim.duration(), 0.2);
  }

  #[test]
  fn animator_advances_frames() {
    let frames = vec![
      mock_sprite_handle(),
      mock_sprite_handle(),
      mock_sprite_handle(),
    ];
    let anim = Animation::new("test", frames, 0.1);
    let mut animator = Animator::new(anim);

    assert_eq!(animator.current_frame_index(), 0);
    animator.update(0.1);
    assert_eq!(animator.current_frame_index(), 1);
    animator.update(0.1);
    assert_eq!(animator.current_frame_index(), 2);
    animator.update(0.1);
    assert_eq!(animator.current_frame_index(), 0); // Looped
  }

  #[test]
  fn animator_once_stops() {
    let frames = vec![mock_sprite_handle(), mock_sprite_handle()];
    let anim = Animation::new("test", frames, 0.1).with_loop_mode(LoopMode::Once);
    let mut animator = Animator::new(anim);

    animator.update(0.1);
    assert_eq!(animator.current_frame_index(), 1);
    assert!(!animator.just_finished());

    animator.update(0.1);
    assert!(animator.just_finished());
    assert_eq!(animator.state, PlaybackState::Stopped);
  }

  #[test]
  fn controller_state_changes() {
    let mut controller = AnimationController::new();
    controller.add_animation(
      "idle",
      Animation::new("idle", vec![mock_sprite_handle()], 1.0),
    );
    controller.add_animation(
      "walk",
      Animation::new("walk", vec![mock_sprite_handle()], 1.0),
    );

    controller.set_state("idle");
    assert_eq!(controller.current_state(), Some("idle"));

    controller.set_state("walk");
    assert_eq!(controller.current_state(), Some("walk"));
  }

  #[test]
  fn animation_builder() {
    let frames = vec![mock_sprite_handle(), mock_sprite_handle()];
    let anim = Animation::new("test", frames, 0.5).with_loop_mode(LoopMode::PingPong);

    assert_eq!(anim.name(), "test");
    assert!(matches!(anim.loop_mode, LoopMode::PingPong));
  }

  #[test]
  fn animator_pause_play() {
    let frames = vec![mock_sprite_handle(), mock_sprite_handle()];
    let anim = Animation::new("test", frames, 0.1);
    let mut animator = Animator::new(anim);

    animator.update(0.05);
    assert!(animator.is_playing());

    animator.pause();
    let frame_before = animator.current_frame_index();
    animator.update(0.5); // Time shouldn't advance when paused
    assert_eq!(animator.current_frame_index(), frame_before);

    animator.play();
    assert!(animator.is_playing());
  }

  #[test]
  fn animator_speed_control() {
    let frames = vec![mock_sprite_handle(), mock_sprite_handle()];
    let anim = Animation::new("test", frames, 1.0);
    let mut animator = Animator::new(anim);

    animator.set_speed(2.0);
    assert_eq!(animator.speed(), 2.0);

    animator.update(0.5); // Effective time: 0.5 * 2.0 = 1.0
    assert_eq!(animator.current_frame_index(), 1);
  }

  #[test]
  fn animator_reset() {
    let frames = vec![
      mock_sprite_handle(),
      mock_sprite_handle(),
      mock_sprite_handle(),
    ];
    let anim = Animation::new("test", frames, 0.1);
    let mut animator = Animator::new(anim);

    animator.update(0.25); // Advance past frame 0
    assert_ne!(animator.current_frame_index(), 0);

    animator.reset();
    assert_eq!(animator.current_frame_index(), 0);
  }

  #[test]
  fn loop_mode_pingpong() {
    let frames = vec![
      mock_sprite_handle(),
      mock_sprite_handle(),
      mock_sprite_handle(),
    ];
    let anim = Animation::new("test", frames, 0.1).with_loop_mode(LoopMode::PingPong);
    let mut animator = Animator::new(anim);

    // Forward: 0 -> 1 -> 2
    animator.update(0.1);
    assert_eq!(animator.current_frame_index(), 1);
    animator.update(0.1);
    assert_eq!(animator.current_frame_index(), 2);

    // Reverse: 2 -> 1 -> 0
    animator.update(0.1);
    assert_eq!(animator.current_frame_index(), 1);
    animator.update(0.1);
    assert_eq!(animator.current_frame_index(), 0);

    // Forward again
    animator.update(0.1);
    assert_eq!(animator.current_frame_index(), 1);
  }

  #[test]
  fn loop_mode_times() {
    let frames = vec![mock_sprite_handle(), mock_sprite_handle()];
    let anim = Animation::new("test", frames, 0.1).with_loop_mode(LoopMode::Times(2));
    let mut animator = Animator::new(anim);

    // First loop
    animator.update(0.1);
    assert_eq!(animator.current_frame_index(), 1);
    animator.update(0.1);
    assert_eq!(animator.current_frame_index(), 0); // Loop back

    // Second loop
    animator.update(0.1);
    assert_eq!(animator.current_frame_index(), 1);
    animator.update(0.1);
    assert!(animator.just_finished()); // Should stop after 2 loops
  }

  #[test]
  fn controller_default() {
    let controller = AnimationController::default();
    assert_eq!(controller.current_state(), None);
  }

  #[test]
  fn controller_clone() {
    let mut controller = AnimationController::new();
    controller.add_animation(
      "idle",
      Animation::new("idle", vec![mock_sprite_handle()], 1.0),
    );
    controller.set_state("idle");

    let cloned = controller.clone();
    assert_eq!(cloned.current_state(), Some("idle"));
  }

  #[test]
  fn validated_animator_allows_valid_transitions() {
    let idle = Animation::new("idle", vec![mock_sprite_handle()], 1.0);
    let walk = Animation::new("walk", vec![mock_sprite_handle()], 1.0);

    let mut anim = ValidatedAnimator::new(idle, "idle")
      .with_animation(walk, "walk")
      .with_transition("idle", "walk")
      .with_transition("walk", "idle");

    assert_eq!(anim.current(), "idle");
    assert!(anim.set_animation("walk"));
    assert_eq!(anim.current(), "walk");
    assert!(anim.set_animation("idle"));
    assert_eq!(anim.current(), "idle");
  }

  #[test]
  fn validated_animator_blocks_invalid_transitions() {
    let idle = Animation::new("idle", vec![mock_sprite_handle()], 1.0);
    let walk = Animation::new("walk", vec![mock_sprite_handle()], 1.0);
    let attack = Animation::new("attack", vec![mock_sprite_handle()], 1.0);

    let mut anim = ValidatedAnimator::new(idle, "idle")
      .with_animation(walk, "walk")
      .with_animation(attack, "attack")
      .with_transition("idle", "walk")
      .with_transition("idle", "attack")
      .with_transition("attack", "idle"); // attack must return to idle

    // Go to attack
    assert!(anim.set_animation("attack"));
    assert_eq!(anim.current(), "attack");

    // Can't go attack -> walk (not allowed)
    assert!(!anim.set_animation("walk"));
    assert_eq!(anim.current(), "attack"); // Still in attack

    // Can go attack -> idle
    assert!(anim.set_animation("idle"));
    assert_eq!(anim.current(), "idle");
  }

  #[test]
  fn validated_animator_transition_from_any() {
    let idle = Animation::new("idle", vec![mock_sprite_handle()], 1.0);
    let walk = Animation::new("walk", vec![mock_sprite_handle()], 1.0);
    let hurt = Animation::new("hurt", vec![mock_sprite_handle()], 1.0);

    let mut anim = ValidatedAnimator::new(idle, "idle")
      .with_animation(walk, "walk")
      .with_animation(hurt, "hurt")
      .with_transition("idle", "walk")
      .with_transition_from_any("hurt"); // hurt from anywhere

    // Start walking
    anim.set_animation("walk");
    assert_eq!(anim.current(), "walk");

    // Can be hurt from walk (no explicit walk->hurt transition needed)
    assert!(anim.set_animation("hurt"));
    assert_eq!(anim.current(), "hurt");
  }

  #[test]
  fn validated_animator_force_bypasses_rules() {
    let idle = Animation::new("idle", vec![mock_sprite_handle()], 1.0);
    let walk = Animation::new("walk", vec![mock_sprite_handle()], 1.0);

    let mut anim = ValidatedAnimator::new(idle, "idle").with_animation(walk, "walk");
    // No transitions defined

    // Normal transition fails
    assert!(!anim.set_animation("walk"));
    assert_eq!(anim.current(), "idle");

    // Force works
    anim.force_animation("walk");
    assert_eq!(anim.current(), "walk");
  }

  #[test]
  fn validated_animator_can_transition_to() {
    let idle = Animation::new("idle", vec![mock_sprite_handle()], 1.0);
    let walk = Animation::new("walk", vec![mock_sprite_handle()], 1.0);
    let run = Animation::new("run", vec![mock_sprite_handle()], 1.0);

    let anim = ValidatedAnimator::new(idle, "idle")
      .with_animation(walk, "walk")
      .with_animation(run, "run")
      .with_transition("idle", "walk");

    assert!(anim.can_transition_to("walk"));
    assert!(!anim.can_transition_to("run"));
    assert!(!anim.can_transition_to("idle")); // Already in idle
  }
}
