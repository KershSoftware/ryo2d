//! The player's invincibility, its squash and stretch, and stopping it when
//! it dies. The engine moves it (`PlatformerMovement` on a
//! `CharacterController`, spawned in `spawning.rs`).

use crate::components::*;
use crate::*;
use ryo2d::prelude::*;

use super::GameScene;

impl GameScene {
  pub(super) fn update_player(&mut self, ctx: &mut SceneContext, dt: f32) {
    profile_scope!("player");
    let Some(player_entity) = self.player else {
      return;
    };

    // The preset reports its jumps the step after: stretch on each
    let jumped = ctx.messages::<MovementEvent>().iter().any(|event| {
      event.entity == player_entity
        && matches!(
          event.kind,
          MovementEventKind::Jumped | MovementEventKind::WallJumped
        )
    });
    if jumped {
      stretch(ctx, player_entity);
    }

    let Some(mut player) = ctx.world.get_mut::<Player>(player_entity) else {
      return;
    };
    let was_invincible = player.is_invincible();
    player.update(dt);
    let invincible_timer = player.invincible_timer;

    // Animate flash shader during invincibility, clear when it ends
    let shader = if player.is_invincible() {
      // Pulsing flash: oscillate intensity based on time, fading as timer decreases
      let pulse = (invincible_timer * 8.0).sin().abs(); // Fast pulse
      let fade = (invincible_timer / INVINCIBLE_DURATION).min(1.0); // Fade out over time
      let intensity = pulse * fade * 0.8 + 0.2 * fade; // Range from 0.2*fade to 1.0*fade
      SpriteShader::flash(Color::rgb(255, 0, 0), intensity)
    } else if was_invincible {
      // Restore outline shader when invincibility just ended
      SpriteShader::outline(Color::rgb(0, 0, 0), 2)
    } else {
      return;
    };
    if let Some(visual) = player_visual(ctx.world, player_entity)
      && let Some(mut renderable) = ctx.world.get_mut::<Renderable>(visual)
    {
      renderable.shader = Some(shader);
    }
  }

  /// Hold the player still for the death effect: without its movement and
  /// input the controller sweeps a zero velocity.
  pub(super) fn freeze_player(&self, ctx: &mut SceneContext) {
    let Some(player_entity) = self.player else {
      return;
    };
    if let Ok(mut player) = ctx.world.get_entity_mut(player_entity) {
      player.remove::<(PlatformerMovement, MoveBindings)>();
      if let Some(mut velocity) = player.get_mut::<Velocity>() {
        *velocity = Velocity::zero();
      }
    }
  }
}

/// The player's sprite: a child of the player, so visual effects such as the
/// squash and stretch (a scale) leave the hitbox alone.
pub(super) fn player_visual(world: &World, player: Entity) -> Option<Entity> {
  world
    .get::<Children>(player)
    .and_then(|children| children.first().copied())
}

/// Stretch the player's sprite, then tween it back with an elastic bounce.
fn stretch(ctx: &mut SceneContext, player: Entity) {
  let Some(visual) = player_visual(ctx.world, player) else {
    return;
  };
  if let Some(mut transform) = ctx.world.get_mut::<Transform>(visual) {
    transform.scale.x = 0.7;
    transform.scale.y = 1.4;
  }
  ctx
    .world
    .entity_mut(visual)
    .insert(Tween::to_scale(1.0, 1.0, 0.5).with_easing(Ease::OutElastic));
}
