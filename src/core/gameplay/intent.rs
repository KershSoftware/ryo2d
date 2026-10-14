//! Movement input: `MoveIntent`, which every preset reads, and
//! `MoveBindings`, which fills it from the keyboard and the gamepad.

use winit::keyboard::KeyCode;

use crate::core::ecs::{Component, DetectChangesMut, Query, Res, Vec2};
use crate::core::input::{GamepadButton, Input};

/// What a character wants to do this step. The movement presets read it;
/// `MoveBindings` fills it from input, an AI or networking can write it
/// instead.
#[derive(Component, Debug, Clone, Copy, Default, PartialEq)]
pub struct MoveIntent {
  /// Where to move, -1 to 1 per axis (+Y down), analog from sticks.
  /// Presets clamp it to length 1 and read a non-finite one as zero.
  pub direction: Vec2,
  /// Jump is held (a jump released early is cut short)
  pub jump_held: bool,
  /// Jump was pressed; the presets act on it and clear it
  pub jump_pressed: bool,
  /// Dash was pressed; the presets act on it and clear it
  pub dash_pressed: bool,
}

impl MoveIntent {
  /// The direction clamped to length 1; zero when it is not finite.
  pub fn clamped_direction(&self) -> Vec2 {
    if self.direction.is_finite() {
      self.direction.clamp_length_max(1.0)
    } else {
      Vec2::ZERO
    }
  }
}

/// Fill the entity's `MoveIntent` from the keyboard (and the first gamepad)
/// every fixed step. A press is seen in the first fixed step after it, once,
/// even in a frame that runs no fixed step (`Input`'s fixed view).
#[derive(Component, Debug, Clone, PartialEq)]
#[non_exhaustive]
#[require(MoveIntent)]
pub struct MoveBindings {
  pub left: Vec<KeyCode>,
  pub right: Vec<KeyCode>,
  pub up: Vec<KeyCode>,
  pub down: Vec<KeyCode>,
  pub jump: Vec<KeyCode>,
  pub dash: Vec<KeyCode>,
  /// Also read the first connected gamepad: the left stick and d-pad move,
  /// South jumps, West dashes
  pub gamepad: bool,
}

impl Default for MoveBindings {
  /// WASD and the arrows move, Space jumps, Shift dashes, plus the gamepad.
  fn default() -> Self {
    Self {
      left: vec![KeyCode::KeyA, KeyCode::ArrowLeft],
      right: vec![KeyCode::KeyD, KeyCode::ArrowRight],
      up: vec![KeyCode::KeyW, KeyCode::ArrowUp],
      down: vec![KeyCode::KeyS, KeyCode::ArrowDown],
      jump: vec![KeyCode::Space],
      dash: vec![KeyCode::ShiftLeft, KeyCode::ShiftRight],
      gamepad: true,
    }
  }
}

impl MoveBindings {
  pub fn with_left(mut self, keys: impl IntoIterator<Item = KeyCode>) -> Self {
    self.left = keys.into_iter().collect();
    self
  }

  pub fn with_right(mut self, keys: impl IntoIterator<Item = KeyCode>) -> Self {
    self.right = keys.into_iter().collect();
    self
  }

  pub fn with_up(mut self, keys: impl IntoIterator<Item = KeyCode>) -> Self {
    self.up = keys.into_iter().collect();
    self
  }

  pub fn with_down(mut self, keys: impl IntoIterator<Item = KeyCode>) -> Self {
    self.down = keys.into_iter().collect();
    self
  }

  pub fn with_jump(mut self, keys: impl IntoIterator<Item = KeyCode>) -> Self {
    self.jump = keys.into_iter().collect();
    self
  }

  pub fn with_dash(mut self, keys: impl IntoIterator<Item = KeyCode>) -> Self {
    self.dash = keys.into_iter().collect();
    self
  }

  /// Ignore the gamepad (a second keyboard player, say).
  pub fn keyboard_only(mut self) -> Self {
    self.gamepad = false;
    self
  }
}

/// `FixedSet::Movement`: fill every bound `MoveIntent` from this step's input.
pub fn read_move_bindings(input: Res<Input>, mut bound: Query<(&MoveBindings, &mut MoveIntent)>) {
  for (bindings, mut intent) in &mut bound {
    let down = |keys: &[KeyCode]| keys.iter().any(|&key| input.key_down(key));
    let pressed = |keys: &[KeyCode]| keys.iter().any(|&key| input.key_pressed(key));
    let pad = bindings.gamepad;
    let held = |button| pad && input.gamepad_button_down(button);
    let tapped = |button| pad && input.gamepad_button_pressed(button);
    let axis = |negative: bool, positive: bool| {
      f32::from(u8::from(positive)) - f32::from(u8::from(negative))
    };
    let (stick_x, stick_y) = if pad { input.left_stick() } else { (0.0, 0.0) };
    let x = axis(
      down(&bindings.left) || held(GamepadButton::DPadLeft),
      down(&bindings.right) || held(GamepadButton::DPadRight),
    ) + stick_x;
    // Sticks report +Y up; the world is +Y down
    let y = axis(
      down(&bindings.up) || held(GamepadButton::DPadUp),
      down(&bindings.down) || held(GamepadButton::DPadDown),
    ) - stick_y;
    intent.set_if_neq(MoveIntent {
      direction: Vec2::new(x.clamp(-1.0, 1.0), y.clamp(-1.0, 1.0)),
      jump_held: down(&bindings.jump) || held(GamepadButton::South),
      jump_pressed: pressed(&bindings.jump) || tapped(GamepadButton::South),
      dash_pressed: pressed(&bindings.dash) || tapped(GamepadButton::West),
    });
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::ecs::World;
  use crate::core::input::InputEvent;
  use crate::core::systems::test_util::run;

  fn bound(world: &mut World) -> crate::core::ecs::Entity {
    world.spawn(MoveBindings::default()).id()
  }

  fn press(world: &mut World, key: KeyCode) {
    world
      .resource_mut::<Input>()
      .queue(InputEvent::Key { key, pressed: true });
  }

  fn intent(world: &World, e: crate::core::ecs::Entity) -> MoveIntent {
    *world.get::<MoveIntent>(e).unwrap()
  }

  fn world() -> World {
    let mut world = World::new();
    world.insert_resource(Input::new());
    world
  }

  #[test]
  fn wasd_and_arrows_move() {
    let mut world = world();
    let e = bound(&mut world);
    press(&mut world, KeyCode::KeyD);
    press(&mut world, KeyCode::ArrowUp);
    world.resource_mut::<Input>().begin_frame();
    world.resource_mut::<Input>().set_fixed_step(0);
    run(&mut world, read_move_bindings);
    assert_eq!(intent(&world, e).direction, Vec2::new(1.0, -1.0));
  }

  #[test]
  fn a_press_shows_in_one_fixed_step_and_the_hold_in_every_one() {
    let mut world = world();
    let e = bound(&mut world);
    press(&mut world, KeyCode::Space);
    world.resource_mut::<Input>().begin_frame();
    world.resource_mut::<Input>().set_fixed_step(0);
    run(&mut world, read_move_bindings);
    assert!(intent(&world, e).jump_pressed && intent(&world, e).jump_held);
    world.resource_mut::<Input>().set_fixed_step(1);
    run(&mut world, read_move_bindings);
    assert!(!intent(&world, e).jump_pressed && intent(&world, e).jump_held);
  }

  #[test]
  fn a_press_in_a_frame_without_a_fixed_step_reaches_the_next_one() {
    let mut world = world();
    let e = bound(&mut world);
    press(&mut world, KeyCode::ShiftLeft);
    world.resource_mut::<Input>().begin_frame();
    world.resource_mut::<Input>().end_fixed_steps(false);
    world.resource_mut::<Input>().begin_frame();
    world.resource_mut::<Input>().set_fixed_step(0);
    run(&mut world, read_move_bindings);
    assert!(intent(&world, e).dash_pressed);
  }

  #[test]
  fn rebound_keys_replace_the_defaults() {
    let mut world = world();
    let e = world
      .spawn(MoveBindings::default().with_jump([KeyCode::KeyK]))
      .id();
    press(&mut world, KeyCode::Space);
    world.resource_mut::<Input>().begin_frame();
    world.resource_mut::<Input>().set_fixed_step(0);
    run(&mut world, read_move_bindings);
    assert!(!intent(&world, e).jump_pressed, "Space no longer jumps");
    world.resource_mut::<Input>().end_fixed_steps(true);
    press(&mut world, KeyCode::KeyK);
    world.resource_mut::<Input>().begin_frame();
    world.resource_mut::<Input>().set_fixed_step(0);
    run(&mut world, read_move_bindings);
    assert!(intent(&world, e).jump_pressed, "K does");
  }

  #[test]
  fn the_direction_is_clamped_to_length_one_and_nan_is_zero() {
    let at = |direction| {
      MoveIntent {
        direction,
        ..MoveIntent::default()
      }
      .clamped_direction()
    };
    assert_eq!(at(Vec2::new(5.0, 0.0)), Vec2::X);
    assert!((at(Vec2::ONE).length() - 1.0).abs() < 1e-6);
    assert_eq!(at(Vec2::new(0.5, 0.0)), Vec2::new(0.5, 0.0));
    assert_eq!(at(Vec2::new(f32::NAN, 1.0)), Vec2::ZERO);
  }
}
