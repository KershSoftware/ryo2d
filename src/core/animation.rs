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
//!     Transform::new(x, y),
//!     SpriteRenderer::new(frame1),
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
//! // In game logic - just set the state
//! controller.set_state("walk");
//! ```

use std::collections::HashMap;

use super::asset::SpriteHandle;

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
/// Attach this to any entity with a [`SpriteRenderer`](crate::core::ecs::SpriteRenderer).
/// The engine automatically updates the sprite each frame.
#[derive(Clone)]
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
  /// This flag is set for one frame when the animation completes.
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

  /// Update the animation (called by engine each frame)
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
#[derive(Clone, Default)]
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

  /// Update the controller (called by engine each frame)
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

#[cfg(test)]
mod tests {
  use super::*;

  fn mock_sprite_handle() -> SpriteHandle {
    use crate::core::sprite::Sprite;
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
    let frames = vec![mock_sprite_handle(), mock_sprite_handle(), mock_sprite_handle()];
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
    controller.add_animation("idle", Animation::new("idle", vec![mock_sprite_handle()], 1.0));
    controller.add_animation("walk", Animation::new("walk", vec![mock_sprite_handle()], 1.0));

    controller.set_state("idle");
    assert_eq!(controller.current_state(), Some("idle"));

    controller.set_state("walk");
    assert_eq!(controller.current_state(), Some("walk"));
  }
}
