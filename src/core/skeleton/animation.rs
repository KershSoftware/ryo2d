//! Per-bone-track skeletal animation and playback.

use crate::core::ecs::Component;
use std::sync::Arc;

use crate::core::ecs::Ease;
use crate::core::graphics::LoopMode;

use super::bone::{BoneTransform, Pose, SkeletonDef};

/// A single keyframe for one bone.
#[derive(Debug, Clone)]
pub struct BoneKeyframe {
  pub time: f32,
  pub transform: BoneTransform,
  pub easing: Ease,
}

/// Timeline of keyframes for a single bone.
#[derive(Debug, Clone)]
pub struct BoneTrack {
  pub bone_index: usize,
  pub keyframes: Vec<BoneKeyframe>,
}

impl BoneTrack {
  /// Sample this track at a given time, returning the interpolated transform.
  ///
  /// Returns `None` if the track has no keyframes.
  pub fn sample(&self, time: f32) -> Option<BoneTransform> {
    if self.keyframes.is_empty() {
      return None;
    }

    // Before first keyframe
    if time <= self.keyframes[0].time {
      return Some(self.keyframes[0].transform);
    }

    // After last keyframe
    let last = self.keyframes.len() - 1;
    if time >= self.keyframes[last].time {
      return Some(self.keyframes[last].transform);
    }

    // Find surrounding keyframes
    for i in 0..last {
      let curr = &self.keyframes[i];
      let next = &self.keyframes[i + 1];

      if time >= curr.time && time < next.time {
        let duration = next.time - curr.time;
        let raw_t = (time - curr.time) / duration;
        let eased_t = curr.easing.apply(raw_t);
        return Some(curr.transform.lerp(&next.transform, eased_t));
      }
    }

    Some(self.keyframes[last].transform)
  }
}

/// A skeletal animation clip with per-bone tracks.
#[derive(Debug, Clone)]
pub struct SkeletalAnimation {
  pub name: String,
  pub tracks: Vec<BoneTrack>,
  pub duration: f32,
  pub loop_mode: LoopMode,
}

impl SkeletalAnimation {
  /// Sample all tracks at a given time, producing a pose.
  ///
  /// Bones without tracks keep their values from `base_pose`.
  pub fn sample(&self, time: f32, base_pose: &Pose) -> Pose {
    let mut result = base_pose.clone();

    for track in &self.tracks {
      if let Some(transform) = track.sample(time)
        && track.bone_index < result.transforms.len()
      {
        result.transforms[track.bone_index] = transform;
      }
    }

    result
  }
}

/// ECS component controlling skeletal animation playback.
#[derive(Component, Debug, Clone)]
pub struct SkeletalAnimator {
  pub animation: Arc<SkeletalAnimation>,
  pub time: f32,
  pub speed: f32,
  pub playing: bool,
  pub just_finished: bool,
  forward: bool,
  loops_remaining: Option<u32>,
}

impl SkeletalAnimator {
  pub fn new(animation: Arc<SkeletalAnimation>) -> Self {
    let loops_remaining = match animation.loop_mode {
      LoopMode::Times(n) => Some(n),
      _ => None,
    };
    Self {
      animation,
      time: 0.0,
      speed: 1.0,
      playing: true,
      just_finished: false,
      forward: true,
      loops_remaining,
    }
  }

  /// Update playback time. Called by the skeleton system each frame.
  pub fn update(&mut self, dt: f32) {
    self.just_finished = false;

    if !self.playing {
      return;
    }

    let duration = self.animation.duration;
    if duration <= 0.0 {
      return;
    }

    self.time += dt * self.speed;

    match self.animation.loop_mode {
      LoopMode::Once => {
        if self.time >= duration {
          self.time = duration;
          self.playing = false;
          self.just_finished = true;
        }
      }
      LoopMode::Loop => {
        while self.time >= duration {
          self.time -= duration;
        }
      }
      LoopMode::PingPong => loop {
        if self.forward && self.time >= duration {
          self.time = 2.0 * duration - self.time;
          self.forward = false;
        } else if !self.forward && self.time <= 0.0 {
          self.time = -self.time;
          self.forward = true;
        } else {
          break;
        }
      },
      LoopMode::Times(_) => {
        if self.time >= duration {
          match &mut self.loops_remaining {
            Some(n) if *n > 1 => {
              *n -= 1;
              self.time -= duration;
            }
            _ => {
              self.time = duration;
              self.playing = false;
              self.just_finished = true;
            }
          }
        }
      }
    }
  }

  pub fn play(&mut self) {
    self.playing = true;
  }

  pub fn pause(&mut self) {
    self.playing = false;
  }

  pub fn reset(&mut self) {
    self.time = 0.0;
    self.forward = true;
    self.just_finished = false;
    self.loops_remaining = match self.animation.loop_mode {
      LoopMode::Times(n) => Some(n),
      _ => None,
    };
  }

  pub fn just_finished(&self) -> bool {
    self.just_finished
  }

  /// Sample the current animation at the current time, producing a pose.
  ///
  /// Bones without tracks keep their values from `base_pose`.
  pub fn sample(&self, base_pose: &Pose) -> Pose {
    self.animation.sample(self.time, base_pose)
  }
}

// --- Animation Builder ---

/// Builder for constructing skeletal animations with named bone tracks.
pub struct SkeletalAnimationBuilder {
  name: String,
  duration: f32,
  loop_mode: LoopMode,
  tracks: Vec<BoneTrack>,
  skeleton_def: Arc<SkeletonDef>,
}

impl SkeletalAnimationBuilder {
  pub fn new(name: &str, duration: f32, skeleton_def: Arc<SkeletonDef>) -> Self {
    Self {
      name: name.to_string(),
      duration,
      loop_mode: LoopMode::default(),
      tracks: Vec::new(),
      skeleton_def,
    }
  }

  pub fn loop_mode(mut self, mode: LoopMode) -> Self {
    self.loop_mode = mode;
    self
  }

  /// Add a track for a named bone using a closure that builds keyframes.
  ///
  /// Panics if the bone name doesn't exist in the skeleton definition.
  pub fn track(
    mut self,
    bone_name: &str,
    build: impl FnOnce(TrackBuilder) -> TrackBuilder,
  ) -> Self {
    let bone_index = *self
      .skeleton_def
      .bone_indices
      .get(bone_name)
      .unwrap_or_else(|| panic!("Bone '{}' not found in skeleton definition", bone_name));

    let builder = build(TrackBuilder::new());
    self.tracks.push(BoneTrack {
      bone_index,
      keyframes: builder.keyframes,
    });
    self
  }

  pub fn build(self) -> SkeletalAnimation {
    SkeletalAnimation {
      name: self.name,
      tracks: self.tracks,
      duration: self.duration,
      loop_mode: self.loop_mode,
    }
  }
}

/// Builder for keyframes within a single bone track.
pub struct TrackBuilder {
  keyframes: Vec<BoneKeyframe>,
}

impl TrackBuilder {
  fn new() -> Self {
    Self {
      keyframes: Vec::new(),
    }
  }

  /// Add a keyframe at the given time.
  pub fn at(mut self, time: f32, transform: BoneTransform, easing: Ease) -> Self {
    self.keyframes.push(BoneKeyframe {
      time,
      transform,
      easing,
    });
    self
  }
}

impl SkeletalAnimation {
  /// Create a builder for this animation.
  ///
  /// The skeleton definition is needed to resolve bone names to indices.
  pub fn builder(
    name: &str,
    duration: f32,
    skeleton_def: Arc<SkeletonDef>,
  ) -> SkeletalAnimationBuilder {
    SkeletalAnimationBuilder::new(name, duration, skeleton_def)
  }
}
