//! Player update and collision systems

use crate::components::*;
use crate::*;
use ryo2d::prelude::*;

use super::GameScene;

impl GameScene {
  pub(super) fn update_player(&mut self, ctx: &mut SceneContext, dt: f32) {
    profile_scope!("player");
    use crate::components::WallSide;
    use crate::constants::{
      WALL_JUMP_FORCE_X, WALL_JUMP_FORCE_Y, WALL_SLIDE_GRAVITY_SCALE, WALL_SLIDE_MAX_SPEED,
    };

    let Some(player_entity) = self.player else {
      return;
    };

    // Get components
    let Some(mut player) = ctx.world.get_mut::<Player>(player_entity) else {
      return;
    };
    let was_invincible = player.is_invincible();
    let was_grounded = player.is_grounded;
    let was_wall_sliding = player.wall_sliding;
    let can_perform_wall_jump = player.can_perform_wall_jump();
    let wall_jump_side = player.wall_jump_side();
    let is_wall_jump_locked = player.is_wall_jump_locked();
    player.update(dt);
    let is_invincible = player.is_invincible();

    // Animate flash shader during invincibility, clear when it ends
    if is_invincible {
      // Get remaining invincibility time for animation
      let invincible_timer = ctx
        .world
        .get::<Player>(player_entity)
        .map(|p| p.invincible_timer)
        .unwrap_or(0.0);

      // Pulsing flash: oscillate intensity based on time, fading as timer decreases
      let pulse = (invincible_timer * 8.0).sin().abs(); // Fast pulse
      let fade = (invincible_timer / INVINCIBLE_DURATION).min(1.0); // Fade out over time
      let intensity = pulse * fade * 0.8 + 0.2 * fade; // Range from 0.2*fade to 1.0*fade

      if let Some(mut renderable) = ctx.world.get_mut::<Renderable>(player_entity) {
        renderable.shader = Some(SpriteShader::flash(Color::rgb(255, 0, 0), intensity));
      }
    } else if was_invincible {
      // Restore outline shader when invincibility just ended
      if let Some(mut renderable) = ctx.world.get_mut::<Renderable>(player_entity) {
        renderable.shader = Some(SpriteShader::outline(Color::rgb(0, 0, 0), 2));
      }
    }

    let Some(mut velocity) = ctx.world.get_mut::<Velocity>(player_entity) else {
      return;
    };

    // Horizontal movement (skip if wall jump push is active)
    if !is_wall_jump_locked {
      let mut move_x = 0.0;
      // Keyboard input
      if ctx.input.key_down(KeyCode::KeyA) || ctx.input.key_down(KeyCode::ArrowLeft) {
        move_x -= 1.0;
      }
      if ctx.input.key_down(KeyCode::KeyD) || ctx.input.key_down(KeyCode::ArrowRight) {
        move_x += 1.0;
      }
      // Gamepad input (additive - left stick or D-pad)
      move_x += ctx.input.left_stick_x();
      if ctx.input.gamepad_button_down(GamepadButton::DPadLeft) {
        move_x -= 1.0;
      }
      if ctx.input.gamepad_button_down(GamepadButton::DPadRight) {
        move_x += 1.0;
      }
      // Clamp to -1.0..1.0 range
      move_x = move_x.clamp(-1.0, 1.0);
      velocity.dx = move_x * PLAYER_SPEED;
    }

    let jump_pressed = ctx.input.key_pressed(KeyCode::Space)
      || ctx.input.key_pressed(KeyCode::KeyW)
      || ctx.input.gamepad_button_pressed(GamepadButton::South);

    // Jumping - normal ground jump or wall jump
    if jump_pressed {
      if was_grounded {
        // Normal ground jump
        velocity.dy = -PLAYER_JUMP_FORCE;
        if let Some(mut player) = ctx.world.get_mut::<Player>(player_entity) {
          player.is_grounded = false;
          player.wall_sliding = None;
        }
        // Add elastic squash-and-stretch effect on jump
        // First stretch the sprite, then tween back to normal with elastic bounce
        if let Some(mut transform) = ctx.world.get_mut::<Transform>(player_entity) {
          transform.scale.x = 0.7;
          transform.scale.y = 1.4;
        }
        ctx
          .world
          .entity_mut(player_entity)
          .insert(Tween::to_scale(1.0, 1.0, 0.5).with_easing(Ease::OutElastic));
      } else if can_perform_wall_jump && wall_jump_side.is_some() {
        // Wall jump - push away from wall with forced momentum
        // Works both when actively sliding AND during grace period after releasing wall direction
        let wall_side = wall_jump_side.unwrap();
        velocity.dy = -WALL_JUMP_FORCE_Y;
        velocity.dx = match wall_side {
          WallSide::Left => WALL_JUMP_FORCE_X,   // Push right
          WallSide::Right => -WALL_JUMP_FORCE_X, // Push left
        };
        if let Some(mut player) = ctx.world.get_mut::<Player>(player_entity) {
          player.start_wall_jump(); // Sets cooldown to prevent horizontal override
          player.wall_sliding = None;
        }
        // Add elastic squash-and-stretch effect on wall jump
        if let Some(mut transform) = ctx.world.get_mut::<Transform>(player_entity) {
          transform.scale.x = 0.7;
          transform.scale.y = 1.4;
        }
        ctx
          .world
          .entity_mut(player_entity)
          .insert(Tween::to_scale(1.0, 1.0, 0.5).with_easing(Ease::OutElastic));
      }
    }

    // Check if player is standing on a moving barrier and ride it
    let riding_barrier = self.apply_barrier_riding(ctx, player_entity);

    // Apply gravity (skip if riding a barrier - they're supported)
    if !riding_barrier && let Some(mut velocity) = ctx.world.get_mut::<Velocity>(player_entity) {
      // Apply gravity - reduced when wall sliding for smoother slide
      if was_wall_sliding.is_some() && velocity.dy > 0.0 {
        // Wall sliding: reduced gravity and capped max speed
        velocity.dy += GRAVITY * WALL_SLIDE_GRAVITY_SCALE * dt;
        velocity.dy = velocity.dy.min(WALL_SLIDE_MAX_SPEED);
      } else {
        // Normal gravity
        velocity.dy += GRAVITY * dt;
      }
    }

    // Apply velocity to transform (player is kinematic, so we handle movement manually)
    let Some(velocity) = ctx.world.get::<Velocity>(player_entity).copied() else {
      return;
    };
    let Some(mut transform) = ctx.world.get_mut::<Transform>(player_entity) else {
      return;
    };

    transform.translation.x += velocity.dx * dt;
    // Skip Y velocity when riding barrier - apply_barrier_riding already snapped position
    if !riding_barrier {
      transform.translation.y += velocity.dy * dt;
    }

    let tx = transform.translation.x;
    let ty = transform.translation.y;
    let vel_y = velocity.dy;

    // Wall slide detection - check if player is against screen edge walls or barriers and falling
    // Player must be pressing towards the wall to initiate wall slide
    let pressing_left = ctx.input.key_down(KeyCode::KeyA)
      || ctx.input.key_down(KeyCode::ArrowLeft)
      || ctx.input.left_stick_x() < -0.5
      || ctx.input.gamepad_button_down(GamepadButton::DPadLeft);
    let pressing_right = ctx.input.key_down(KeyCode::KeyD)
      || ctx.input.key_down(KeyCode::ArrowRight)
      || ctx.input.left_stick_x() > 0.5
      || ctx.input.gamepad_button_down(GamepadButton::DPadRight);

    let mut new_wall_slide: Option<WallSide> = None;
    if vel_y > 0.0 && !was_grounded {
      // Falling and not grounded (player uses center pivot)
      let half_size = PLAYER_SIZE / 2.0;
      if tx <= half_size && pressing_left {
        // Against left screen wall and pressing into it
        new_wall_slide = Some(WallSide::Left);
      } else if tx >= self.screen_width - half_size && pressing_right {
        // Against right screen wall and pressing into it
        new_wall_slide = Some(WallSide::Right);
      } else {
        // Check if against a barrier (moving vertical platform)
        new_wall_slide = self.check_wall_slide_on_barrier(ctx, player_entity);
      }
    }

    // Update wall slide state
    if let Some(mut player) = ctx.world.get_mut::<Player>(player_entity) {
      if new_wall_slide.is_some() && was_wall_sliding.is_none() {
        // Just started wall sliding - can wall jump
        player.can_wall_jump = true;
        player.wall_slide_grace = 0.0; // Clear any existing grace period
      } else if new_wall_slide.is_none() && was_wall_sliding.is_some() {
        // Just released wall - start grace period for wall jump
        player.start_wall_slide_grace(was_wall_sliding.unwrap());
      }
      player.wall_sliding = new_wall_slide;

      // Reset wall jump ability when grounded
      if player.is_grounded {
        player.wall_sliding = None;
        player.can_wall_jump = false;
        player.wall_slide_grace = 0.0;
        player.last_wall_side = None;
      }
    }

    // Screen bounds (player uses center pivot)
    let half_size = PLAYER_SIZE / 2.0;
    if let Some(mut transform) = ctx.world.get_mut::<Transform>(player_entity) {
      transform.translation.x = transform
        .translation
        .x
        .clamp(half_size, self.screen_width - half_size);
    }

    // Ground collision (player uses center pivot, bottom edge is at y + half_size)
    if ty >= self.screen_height - half_size - 30.0 {
      // -30 accounts for ground platform thickness
      if let Some(mut transform) = ctx.world.get_mut::<Transform>(player_entity) {
        transform.translation.y = self.screen_height - half_size - 30.0;
      }
      if let Some(mut velocity) = ctx.world.get_mut::<Velocity>(player_entity) {
        velocity.dy = 0.0;
      }
      if let Some(mut player) = ctx.world.get_mut::<Player>(player_entity) {
        player.is_grounded = true;
        player.wall_sliding = None;
      }
    } else {
      // Check platform, barrier, turret, and drone collisions
      self.check_player_platform_collision(ctx, player_entity);
      self.check_player_barrier_collision(ctx, player_entity);
      self.check_player_turret_collision(ctx, player_entity);
      self.check_player_drone_collision(ctx, player_entity);
    }
  }

  pub(super) fn check_player_platform_collision(
    &mut self,
    ctx: &mut SceneContext,
    player_entity: Entity,
  ) {
    let Some(player_transform) = ctx.world.get::<Transform>(player_entity) else {
      return;
    };
    let Some(player_velocity) = ctx.world.get::<Velocity>(player_entity) else {
      return;
    };

    // Player uses center pivot
    let half_size = PLAYER_SIZE / 2.0;
    let player_left = player_transform.translation.x - half_size;
    let player_right = player_transform.translation.x + half_size;
    let player_top = player_transform.translation.y - half_size;
    let player_bottom = player_transform.translation.y + half_size;
    let vel_y = player_velocity.dy;
    let falling = vel_y > 0.0;

    for &platform_entity in &self.platforms {
      let Some(platform_transform) = ctx.world.get::<Transform>(platform_entity) else {
        continue;
      };

      // Platform sprite is 800px base, scaled by transform.scale.x and
      // centred on the transform
      let platform_half_width = 800.0 * platform_transform.scale.x / 2.0;
      let platform_half_height = PLATFORM_THICKNESS / 2.0;

      let platform_left = platform_transform.translation.x - platform_half_width;
      let platform_right = platform_transform.translation.x + platform_half_width;
      let platform_top = platform_transform.translation.y - platform_half_height;
      let platform_bottom = platform_transform.translation.y + platform_half_height;

      // Check horizontal overlap (must be within platform X bounds)
      let horizontal_overlap = player_right > platform_left && player_left < platform_right;

      if !horizontal_overlap {
        continue;
      }

      // Check if player is falling and has crossed or is on the platform
      // Use velocity-based tolerance to prevent tunneling at high speeds
      // Tolerance = max(15, velocity * dt) to catch fast-moving players
      let velocity_tolerance = (vel_y.abs() / 60.0).max(15.0); // Assume ~60fps for tolerance calc

      if falling
        && player_bottom >= platform_top
        && player_top < platform_bottom
        && player_bottom <= platform_top + velocity_tolerance
      {
        // Landing on top of platform - snap player to platform top (center pivot)
        if let Some(mut transform) = ctx.world.get_mut::<Transform>(player_entity) {
          transform.translation.y = platform_top - half_size;
        }
        if let Some(mut velocity) = ctx.world.get_mut::<Velocity>(player_entity) {
          velocity.dy = 0.0;
        }
        if let Some(mut player) = ctx.world.get_mut::<Player>(player_entity) {
          player.is_grounded = true;
        }
        break;
      }
    }
  }

  pub(super) fn check_player_barrier_collision(
    &mut self,
    ctx: &mut SceneContext,
    player_entity: Entity,
  ) {
    let Some(player_transform) = ctx.world.get::<Transform>(player_entity) else {
      return;
    };
    let Some(player_velocity) = ctx.world.get::<Velocity>(player_entity) else {
      return;
    };

    // Player uses center pivot
    let half_size = PLAYER_SIZE / 2.0;
    let player_left = player_transform.translation.x - half_size;
    let player_right = player_transform.translation.x + half_size;
    let player_top = player_transform.translation.y - half_size;
    let player_bottom = player_transform.translation.y + half_size;
    let vel_x = player_velocity.dx;
    let vel_y = player_velocity.dy;

    for &barrier_entity in &self.barriers {
      let Some(barrier_transform) = ctx.world.get::<Transform>(barrier_entity) else {
        continue;
      };
      let barrier_vel_y = ctx
        .world
        .get::<Barrier>(barrier_entity)
        .map(|b| b.velocity_y)
        .unwrap_or(0.0);

      let barrier_left = barrier_transform.translation.x - BARRIER_WIDTH / 2.0;
      let barrier_right = barrier_transform.translation.x + BARRIER_WIDTH / 2.0;
      let barrier_top = barrier_transform.translation.y - BARRIER_HEIGHT / 2.0;
      let barrier_bottom = barrier_transform.translation.y + BARRIER_HEIGHT / 2.0;

      // Check AABB collision
      if player_right > barrier_left
        && player_left < barrier_right
        && player_bottom > barrier_top
        && player_top < barrier_bottom
      {
        // Push player out of barrier based on movement direction
        let overlap_left = player_right - barrier_left;
        let overlap_right = barrier_right - player_left;
        let overlap_top = player_bottom - barrier_top;
        let overlap_bottom = barrier_bottom - player_top;

        // Find smallest overlap to determine push direction
        let min_overlap = overlap_left
          .min(overlap_right)
          .min(overlap_top)
          .min(overlap_bottom);

        if let Some(mut transform) = ctx.world.get_mut::<Transform>(player_entity) {
          if min_overlap == overlap_left && vel_x > 0.0 {
            // Push left (center pivot)
            transform.translation.x = barrier_left - half_size;
          } else if min_overlap == overlap_right && vel_x < 0.0 {
            // Push right (center pivot)
            transform.translation.x = barrier_right + half_size;
          } else if min_overlap == overlap_top && vel_y > 0.0 {
            // Push up (landing on top, center pivot)
            transform.translation.y = barrier_top - half_size;
            if let Some(mut velocity) = ctx.world.get_mut::<Velocity>(player_entity) {
              // Inherit the barrier's vertical velocity so player moves with it
              velocity.dy = barrier_vel_y;
            }
            if let Some(mut player) = ctx.world.get_mut::<Player>(player_entity) {
              player.is_grounded = true;
            }
          } else if min_overlap == overlap_bottom && vel_y < 0.0 {
            // Push down (hitting from below, center pivot)
            transform.translation.y = barrier_bottom + half_size;
            if let Some(mut velocity) = ctx.world.get_mut::<Velocity>(player_entity) {
              velocity.dy = 0.0;
            }
          }
        }
        break;
      }
    }
  }

  /// Check if player is standing on a barrier and apply its movement
  /// Returns true if player is riding a barrier (gravity should be skipped)
  pub(super) fn apply_barrier_riding(&self, ctx: &mut SceneContext, player_entity: Entity) -> bool {
    let Some(player_transform) = ctx.world.get::<Transform>(player_entity) else {
      return false;
    };

    // Player uses center pivot
    let half_size = PLAYER_SIZE / 2.0;
    let player_left = player_transform.translation.x - half_size;
    let player_right = player_transform.translation.x + half_size;
    let player_bottom = player_transform.translation.y + half_size;

    for &barrier_entity in &self.barriers {
      let Some(barrier_transform) = ctx.world.get::<Transform>(barrier_entity) else {
        continue;
      };
      let Some(barrier) = ctx.world.get::<Barrier>(barrier_entity) else {
        continue;
      };

      let barrier_left = barrier_transform.translation.x - BARRIER_WIDTH / 2.0;
      let barrier_right = barrier_transform.translation.x + BARRIER_WIDTH / 2.0;
      let barrier_top = barrier_transform.translation.y - BARRIER_HEIGHT / 2.0;
      let barrier_vel_y = barrier.velocity_y;

      // Check horizontal overlap first
      let horizontal_overlap = player_right > barrier_left && player_left < barrier_right;
      if !horizontal_overlap {
        continue;
      }

      // Use larger tolerance when barrier is moving up to prevent bouncing
      // When barrier moves up, player can briefly separate before next frame
      let tolerance = if barrier_vel_y < 0.0 {
        // Moving up: larger tolerance proportional to speed
        8.0 + (-barrier_vel_y * 0.05).min(10.0)
      } else {
        // Stationary or moving down: standard tolerance
        4.0
      };

      // Check if player is standing on top of this barrier
      // Player's bottom must be near barrier's top
      let distance_to_top = player_bottom - barrier_top;
      let on_top = distance_to_top >= -tolerance && distance_to_top <= tolerance;

      if on_top {
        // Check if player is actively jumping (velocity significantly more negative than barrier)
        // This distinguishes a real jump from just riding an ascending barrier
        let player_vel_y = ctx
          .world
          .get::<Velocity>(player_entity)
          .map(|v| v.dy)
          .unwrap_or(0.0);

        // Player is jumping if their upward velocity exceeds barrier velocity by a threshold
        // (e.g., barrier at -200, player at -400 means player jumped off the barrier)
        let is_jumping = player_vel_y < barrier_vel_y - 50.0;

        // If jumping, don't apply barrier riding - let the jump happen
        if is_jumping {
          return false;
        }

        // Snap player to ride on top of the barrier (prevents bouncing)
        if let Some(mut transform) = ctx.world.get_mut::<Transform>(player_entity) {
          transform.translation.y = barrier_top - half_size;
        }

        // Set player velocity to match barrier movement
        if let Some(mut velocity) = ctx.world.get_mut::<Velocity>(player_entity) {
          velocity.dy = barrier_vel_y;
        }
        if let Some(mut player) = ctx.world.get_mut::<Player>(player_entity) {
          player.is_grounded = true;
        }
        return true;
      }
    }

    false
  }

  pub(super) fn check_wall_slide_on_barrier(
    &self,
    ctx: &mut SceneContext,
    player_entity: Entity,
  ) -> Option<WallSide> {
    use crate::components::WallSide;

    let player_transform = ctx.world.get::<Transform>(player_entity)?;

    // Player uses center pivot
    let half_size = PLAYER_SIZE / 2.0;
    let player_left = player_transform.translation.x - half_size;
    let player_right = player_transform.translation.x + half_size;
    let player_top = player_transform.translation.y - half_size;
    let player_bottom = player_transform.translation.y + half_size;

    // Check input direction (player must be pressing towards the barrier)
    let pressing_right = ctx.input.key_down(KeyCode::KeyD)
      || ctx.input.key_down(KeyCode::ArrowRight)
      || ctx.input.left_stick_x() > 0.5
      || ctx.input.gamepad_button_down(GamepadButton::DPadRight);
    let pressing_left = ctx.input.key_down(KeyCode::KeyA)
      || ctx.input.key_down(KeyCode::ArrowLeft)
      || ctx.input.left_stick_x() < -0.5
      || ctx.input.gamepad_button_down(GamepadButton::DPadLeft);

    // Tolerance for detecting wall contact (larger to account for collision pushback)
    let tolerance = 12.0;

    for &barrier_entity in &self.barriers {
      let Some(barrier_transform) = ctx.world.get::<Transform>(barrier_entity) else {
        continue;
      };

      let barrier_left = barrier_transform.translation.x - BARRIER_WIDTH / 2.0;
      let barrier_right = barrier_transform.translation.x + BARRIER_WIDTH / 2.0;
      let barrier_top = barrier_transform.translation.y - BARRIER_HEIGHT / 2.0;
      let barrier_bottom = barrier_transform.translation.y + BARRIER_HEIGHT / 2.0;

      // Check if player overlaps vertically with barrier
      if player_bottom > barrier_top && player_top < barrier_bottom {
        // Check if player is against left side of barrier (player to the left, pressing right)
        if player_right >= barrier_left - tolerance
          && player_right <= barrier_left + tolerance
          && pressing_right
        {
          return Some(WallSide::Right); // Wall is to player's right, jump will go left
        }

        // Check if player is against right side of barrier (player to the right, pressing left)
        if player_left <= barrier_right + tolerance
          && player_left >= barrier_right - tolerance
          && pressing_left
        {
          return Some(WallSide::Left); // Wall is to player's left, jump will go right
        }
      }
    }

    None
  }

  pub(super) fn check_player_turret_collision(
    &mut self,
    ctx: &mut SceneContext,
    player_entity: Entity,
  ) {
    let Some(player_transform) = ctx.world.get::<Transform>(player_entity) else {
      return;
    };
    let Some(player_velocity) = ctx.world.get::<Velocity>(player_entity) else {
      return;
    };

    // Player uses center pivot
    let half_size = PLAYER_SIZE / 2.0;
    let player_left = player_transform.translation.x - half_size;
    let player_right = player_transform.translation.x + half_size;
    let player_top = player_transform.translation.y - half_size;
    let player_bottom = player_transform.translation.y + half_size;
    let falling = player_velocity.dy > 0.0;

    for &turret_entity in &self.turrets {
      let Some(turret_transform) = ctx.world.get::<Transform>(turret_entity) else {
        continue;
      };

      let half_turret = TURRET_SIZE / 2.0;
      let turret_left = turret_transform.translation.x - half_turret;
      let turret_right = turret_transform.translation.x + half_turret;
      let turret_top = turret_transform.translation.y - half_turret;
      let turret_bottom = turret_transform.translation.y + half_turret;

      // Check AABB collision
      if player_right > turret_left
        && player_left < turret_right
        && player_bottom > turret_top
        && player_top < turret_bottom
      {
        // Landing on top of turret (player can use as platform)
        if falling && player_bottom <= turret_top + 15.0 {
          if let Some(mut transform) = ctx.world.get_mut::<Transform>(player_entity) {
            transform.translation.y = turret_top - half_size;
          }
          if let Some(mut velocity) = ctx.world.get_mut::<Velocity>(player_entity) {
            velocity.dy = 0.0;
          }
          if let Some(mut player) = ctx.world.get_mut::<Player>(player_entity) {
            player.is_grounded = true;
            player.wall_sliding = None;
          }
          break;
        }
      }
    }
  }

  pub(super) fn check_player_drone_collision(
    &mut self,
    ctx: &mut SceneContext,
    player_entity: Entity,
  ) {
    use crate::constants::DRONE_SIZE;

    let Some(player_transform) = ctx.world.get::<Transform>(player_entity) else {
      return;
    };
    let Some(player_velocity) = ctx.world.get::<Velocity>(player_entity) else {
      return;
    };

    // Player uses center pivot
    let half_size = PLAYER_SIZE / 2.0;
    let player_left = player_transform.translation.x - half_size;
    let player_right = player_transform.translation.x + half_size;
    let player_top = player_transform.translation.y - half_size;
    let player_bottom = player_transform.translation.y + half_size;
    let falling = player_velocity.dy > 0.0;

    for &drone_entity in &self.drones {
      let Some(drone_transform) = ctx.world.get::<Transform>(drone_entity) else {
        continue;
      };

      // The drone's position is its centre
      let drone_left = drone_transform.translation.x - DRONE_SIZE / 2.0;
      let drone_right = drone_transform.translation.x + DRONE_SIZE / 2.0;
      let drone_top = drone_transform.translation.y - DRONE_SIZE / 2.0;
      let drone_bottom = drone_transform.translation.y + DRONE_SIZE / 2.0;

      // Check AABB collision
      if player_right > drone_left
        && player_left < drone_right
        && player_bottom > drone_top
        && player_top < drone_bottom
      {
        // Landing on top of drone (player can use as platform)
        if falling && player_bottom <= drone_top + 15.0 {
          if let Some(mut transform) = ctx.world.get_mut::<Transform>(player_entity) {
            transform.translation.y = drone_top - half_size;
          }
          if let Some(mut velocity) = ctx.world.get_mut::<Velocity>(player_entity) {
            velocity.dy = 0.0;
          }
          if let Some(mut player) = ctx.world.get_mut::<Player>(player_entity) {
            player.is_grounded = true;
            player.wall_sliding = None;
          }
          break;
        }
      }
    }
  }
}
