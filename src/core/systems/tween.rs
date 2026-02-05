//! Tween system for updating Tween and TweenSequence components.
//!
//! This system handles:
//! - Updating tween progress and interpolation
//! - Applying tweened values to Transform (position, scale, rotation)
//! - Applying tweened values to Renderable (alpha, tint)
//! - Managing tween sequences (advancing to next tween when current finishes)

use crate::core::ecs::{Inactive, Renderable, Transform, Tween, TweenSequence, World};

/// System for updating tweens and tween sequences.
///
/// Handles both single `Tween` components and `TweenSequence` components,
/// applying interpolated values to Transform and Renderable.
pub struct TweenSystem;

impl TweenSystem {
  /// Update all tween components in the world.
  ///
  /// # Arguments
  /// * `world` - The ECS world containing entities
  /// * `dt` - Delta time (should be scaled for time effects)
  pub fn update(world: &mut World, dt: f32) {
    Self::update_tweens(world, dt);
    Self::update_tween_sequences(world, dt);
  }

  /// Update all Tween components and apply to Transform/Renderable.
  fn update_tweens(world: &mut World, dt: f32) {
    for entity in world.entities_with::<Tween>() {
      // Skip inactive pooled entities
      if world.has::<Inactive>(entity) {
        continue;
      }

      // Check if tween is playing before borrowing mutably
      let should_update = world
        .get_component::<Tween>(entity)
        .is_some_and(|t| t.playing);

      if !should_update {
        continue;
      }

      // Capture color start values from Renderable if tween has color targets and hasn't started
      let needs_color_capture = world
        .get_component::<Tween>(entity)
        .is_some_and(|t| t.has_color_targets() && !t.has_started());

      if needs_color_capture && let Some(renderable) = world.get_component::<Renderable>(entity) {
        let alpha = renderable.alpha;
        let tint = renderable.tint;
        if let Some(mut tween) = world.get_component_mut::<Tween>(entity) {
          tween.capture_start_renderable(alpha, tint);
        }
      }

      // Get transform and tween together for update
      if let (Some(mut transform), Some(mut tween)) = (
        world.get_component_mut::<Transform>(entity),
        world.get_component_mut::<Tween>(entity),
      ) {
        tween.update(dt, &mut transform);
      }

      // Apply color/alpha tweening to Renderable
      if let (Some(tween), Some(mut renderable)) = (
        world.get_component::<Tween>(entity),
        world.get_component_mut::<Renderable>(entity),
      ) {
        if let Some(alpha) = tween.current_alpha() {
          renderable.alpha = alpha;
        }
        if let Some(tint) = tween.current_tint() {
          renderable.tint = tint;
        }
      }
    }
  }

  /// Update all TweenSequence components.
  fn update_tween_sequences(world: &mut World, dt: f32) {
    for entity in world.entities_with::<TweenSequence>() {
      // Skip inactive pooled entities
      if world.has::<Inactive>(entity) {
        continue;
      }

      // Check if sequence is playing and has a current tween
      let should_update = world
        .get_component::<TweenSequence>(entity)
        .is_some_and(|s| s.playing && s.current().is_some());

      if !should_update {
        continue;
      }

      // Capture color start values if current tween has color targets and hasn't started
      let needs_color_capture = world
        .get_component::<TweenSequence>(entity)
        .is_some_and(|s| {
          s.current()
            .is_some_and(|t| t.has_color_targets() && !t.has_started())
        });

      if needs_color_capture && let Some(renderable) = world.get_component::<Renderable>(entity) {
        let alpha = renderable.alpha;
        let tint = renderable.tint;
        if let Some(mut seq) = world.get_component_mut::<TweenSequence>(entity)
          && let Some(tween) = seq.current_mut()
        {
          tween.capture_start_renderable(alpha, tint);
        }
      }

      // Update the current tween in the sequence
      let tween_finished = if let (Some(mut transform), Some(mut seq)) = (
        world.get_component_mut::<Transform>(entity),
        world.get_component_mut::<TweenSequence>(entity),
      ) {
        if let Some(tween) = seq.current_mut() {
          tween.update(dt, &mut transform);
          tween.just_finished
        } else {
          false
        }
      } else {
        false
      };

      // Apply color/alpha tweening from current tween to Renderable
      if let (Some(seq), Some(mut renderable)) = (
        world.get_component::<TweenSequence>(entity),
        world.get_component_mut::<Renderable>(entity),
      ) && let Some(tween) = seq.current()
      {
        if let Some(alpha) = tween.current_alpha() {
          renderable.alpha = alpha;
        }
        if let Some(tint) = tween.current_tint() {
          renderable.tint = tint;
        }
      }

      // Advance to next tween if current one finished
      if tween_finished && let Some(mut seq) = world.get_component_mut::<TweenSequence>(entity) {
        seq.advance();
      }
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn test_tween_system_no_crash_on_empty_world() {
    let mut world = World::default();
    // Should not panic on empty world
    TweenSystem::update(&mut world, 0.016);
  }
}
