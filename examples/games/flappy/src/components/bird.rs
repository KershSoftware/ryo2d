//! Bird component - the player character

use ryo2d::prelude::*;

use crate::constants::*;

/// Create an AnimationController for the bird with flap and hurt animations
pub fn create_bird_animation_controller() -> AnimationController {
  let size = BIRD_SIZE as u32;

  // Load sprite frames
  let flap_up = SpriteHandle::from_sprite(
    Sprite::load_svg_fit("assets/bird-flap-up.svg", size, size)
      .expect("Failed to load bird-flap-up.svg"),
    "bird-flap-up.svg",
  );
  let flap_down = SpriteHandle::from_sprite(
    Sprite::load_svg_fit("assets/bird-flap-down.svg", size, size)
      .expect("Failed to load bird-flap-down.svg"),
    "bird-flap-down.svg",
  );
  let hurt_1 = SpriteHandle::from_sprite(
    Sprite::load_svg_fit("assets/bird-hurt-1.svg", size, size)
      .expect("Failed to load bird-hurt-1.svg"),
    "bird-hurt-1.svg",
  );
  let hurt_2 = SpriteHandle::from_sprite(
    Sprite::load_svg_fit("assets/bird-hurt-2.svg", size, size)
      .expect("Failed to load bird-hurt-2.svg"),
    "bird-hurt-2.svg",
  );

  // Create animations
  let flap_anim = Animation::new(
    "flap",
    vec![flap_down.clone(), flap_up.clone()],
    BIRD_FLAP_FRAME_TIME,
  );

  let hurt_anim = Animation::new(
    "hurt",
    vec![hurt_1.clone(), hurt_2.clone()],
    BIRD_HURT_FRAME_TIME,
  );

  // Build controller
  let mut controller = AnimationController::new();
  controller.add_animation("flap", flap_anim);
  controller.add_animation("hurt", hurt_anim);
  controller.set_state("flap"); // Start with flapping

  controller
}

/// Marks an entity as the player-controlled bird
#[derive(Debug, Clone, Copy)]
pub struct Bird {
  pub velocity_y: f32,
  pub is_alive: bool,
  pub has_started: bool,
}

impl Bird {
  pub fn new() -> Self {
    Self {
      velocity_y: 0.0,
      is_alive: true,
      has_started: false,
    }
  }

  pub fn flap(&mut self) {
    if self.is_alive {
      self.velocity_y = BIRD_FLAP_VELOCITY;
      self.has_started = true;
    }
  }

  pub fn apply_gravity(&mut self, dt: f32) {
    if self.has_started && self.is_alive {
      self.velocity_y += BIRD_GRAVITY * dt;
      self.velocity_y = self.velocity_y.min(BIRD_MAX_FALL_SPEED);
    }
  }

  pub fn kill(&mut self) {
    self.is_alive = false;
  }
}

impl Default for Bird {
  fn default() -> Self {
    Self::new()
  }
}
