//! Animation system for updating Animator and AnimationController components.
//!
//! This system handles:
//! - Updating animation frame timers
//! - Syncing current animation frames to Renderable sprites

use crate::core::ecs::{Inactive, Renderable, World};
use crate::core::graphics::{AnimationController, Animator};

/// System for updating animations.
///
/// Updates both simple `Animator` components and state-machine based
/// `AnimationController` components, syncing their current frames
/// to the entity's `Renderable` sprite.
pub struct AnimationSystem;

impl AnimationSystem {
  /// Update all animation components in the world.
  ///
  /// # Arguments
  /// * `world` - The ECS world containing entities
  /// * `dt` - Delta time (should be scaled for time effects)
  pub fn update(world: &mut World, dt: f32) {
    Self::update_animators(world, dt);
    Self::sync_animator_sprites(world);
    Self::update_animation_controllers(world, dt);
    Self::sync_animation_controller_sprites(world);
  }

  /// Update all Animator components (skip inactive pooled entities).
  fn update_animators(world: &mut World, dt: f32) {
    for entity in world.entities_with::<Animator>() {
      if world.has::<Inactive>(entity) {
        continue;
      }
      if let Some(mut animator) = world.get_component_mut::<Animator>(entity) {
        animator.update(dt);
      }
    }
  }

  /// Sync Animator sprites to Renderable (skip inactive pooled entities).
  fn sync_animator_sprites(world: &mut World) {
    for entity in world.entities_with::<Animator>() {
      if world.has::<Inactive>(entity) {
        continue;
      }
      let sprite = world
        .get_component::<Animator>(entity)
        .and_then(|a| a.current_sprite().cloned());
      if let Some(sprite) = sprite
        && let Some(mut renderable) = world.get_component_mut::<Renderable>(entity)
      {
        renderable.sprite = sprite;
      }
    }
  }

  /// Update all AnimationController components (skip inactive pooled entities).
  fn update_animation_controllers(world: &mut World, dt: f32) {
    for entity in world.entities_with::<AnimationController>() {
      if world.has::<Inactive>(entity) {
        continue;
      }
      if let Some(mut controller) = world.get_component_mut::<AnimationController>(entity) {
        controller.update(dt);
      }
    }
  }

  /// Sync AnimationController sprites to Renderable (skip inactive pooled entities).
  fn sync_animation_controller_sprites(world: &mut World) {
    for entity in world.entities_with::<AnimationController>() {
      if world.has::<Inactive>(entity) {
        continue;
      }
      let sprite = world
        .get_component::<AnimationController>(entity)
        .and_then(|c| c.current_sprite().cloned());
      if let Some(sprite) = sprite
        && let Some(mut renderable) = world.get_component_mut::<Renderable>(entity)
      {
        renderable.sprite = sprite;
      }
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn test_animation_system_no_crash_on_empty_world() {
    let mut world = World::default();
    // Should not panic on empty world
    AnimationSystem::update(&mut world, 0.016);
  }
}
