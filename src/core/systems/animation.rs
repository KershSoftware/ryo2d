//! Sprite animation: advance `Animator` and `AnimationController` and show
//! their current frame on the entity's `Renderable`.

use crate::core::asset::SpriteHandle;
use crate::core::ecs::{Or, Query, Renderable, Res, With};
use crate::core::graphics::{AnimationController, Animator};
use crate::core::time::Time;

/// Advance every `Animator` and `AnimationController` by one fixed step and
/// put the current frame on the entity's `Renderable`.
pub fn animate_sprites(
  time: Res<Time>,
  mut query: Query<
    (
      Option<&mut Animator>,
      Option<&mut AnimationController>,
      Option<&mut Renderable>,
    ),
    Or<(With<Animator>, With<AnimationController>)>,
  >,
) {
  let dt = time.fixed.timestep();
  for (animator, controller, mut renderable) in &mut query {
    if let Some(mut animator) = animator {
      animator.update(dt);
      show(animator.current_sprite(), renderable.as_deref_mut());
    }
    if let Some(mut controller) = controller {
      controller.update(dt);
      show(controller.current_sprite(), renderable.as_deref_mut());
    }
  }
}

/// Put `sprite` on `renderable` unless it is already showing.
fn show(sprite: Option<&SpriteHandle>, renderable: Option<&mut Renderable>) {
  if let (Some(sprite), Some(renderable)) = (sprite, renderable)
    && renderable.sprite.serial() != sprite.serial()
  {
    renderable.sprite = sprite.clone();
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::asset::SpriteHandle;
  use crate::core::ecs::{Disabled, Renderable};
  use crate::core::graphics::{Animation, Animator, Sprite};
  use crate::core::systems::test_util::{run, world};

  fn frames() -> Vec<SpriteHandle> {
    (0..3)
      .map(|i| SpriteHandle::from_sprite(Sprite::solid_rect(4, 4, i * 80, 0, 0, 255), "frame"))
      .collect()
  }

  #[test]
  fn runs_on_an_empty_world() {
    let mut world = world(60);
    run(&mut world, animate_sprites);
  }

  #[test]
  fn an_animator_shows_its_next_frame_on_the_renderable() {
    let frames = frames();
    let mut world = world(10);
    let entity = world
      .spawn((
        Renderable::sprite(frames[0].clone()),
        Animator::new(Animation::new("walk", frames.clone(), 0.1)),
      ))
      .id();
    run(&mut world, animate_sprites);
    let shown = world.get::<Renderable>(entity).unwrap().sprite.serial();
    assert_eq!(shown, frames[1].serial());
  }

  #[test]
  fn a_disabled_entity_is_untouched() {
    let frames = frames();
    let mut world = world(10);
    let entity = world
      .spawn((
        Renderable::sprite(frames[0].clone()),
        Animator::new(Animation::new("walk", frames.clone(), 0.1)),
        Disabled,
      ))
      .id();
    run(&mut world, animate_sprites);
    let shown = world.get::<Renderable>(entity).unwrap().sprite.serial();
    assert_eq!(shown, frames[0].serial());
  }
}
