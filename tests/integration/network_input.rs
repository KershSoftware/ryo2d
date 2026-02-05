//! Integration tests for NetworkInput derive macro

use ryo2d::core::network::input::NetworkInput;
use serde::{Deserialize, Serialize};

/// Test input with required fields
#[derive(NetworkInput, Clone, Default, Debug, PartialEq, Serialize, Deserialize)]
pub struct PlayerInput {
  #[network_input(required)]
  pub move_x: f32,
  #[network_input(required)]
  pub facing_right: bool,
  pub jump: bool,
  pub attack: bool,
}

/// Test input with no required fields
#[derive(NetworkInput, Clone, Default, Debug, PartialEq, Serialize, Deserialize)]
pub struct SimpleInput {
  pub value: i32,
  pub flag: bool,
}

/// Test input with all required fields
#[derive(NetworkInput, Clone, Default, Debug, PartialEq, Serialize, Deserialize)]
pub struct AllRequiredInput {
  #[network_input(required)]
  pub a: i32,
  #[network_input(required)]
  pub b: i32,
  #[network_input(required)]
  pub c: i32,
}

#[test]
fn builder_with_required_fields() {
  let input = PlayerInput::builder()
    .move_x(1.0)
    .facing_right(true)
    .build();

  assert_eq!(input.move_x, 1.0);
  assert!(input.facing_right);
  assert!(!input.jump); // Default
  assert!(!input.attack); // Default
}

#[test]
fn builder_with_optional_fields() {
  let input = PlayerInput::builder()
    .move_x(-0.5)
    .facing_right(false)
    .jump(true)
    .attack(true)
    .build();

  assert_eq!(input.move_x, -0.5);
  assert!(!input.facing_right);
  assert!(input.jump);
  assert!(input.attack);
}

#[test]
fn builder_no_required_fields() {
  let input = SimpleInput::builder().value(42).flag(true).build();

  assert_eq!(input.value, 42);
  assert!(input.flag);
}

#[test]
fn builder_no_required_fields_defaults() {
  let input = SimpleInput::builder().build();

  assert_eq!(input.value, 0);
  assert!(!input.flag);
}

#[test]
fn builder_all_required() {
  let input = AllRequiredInput::builder().a(1).b(2).c(3).build();

  assert_eq!(input.a, 1);
  assert_eq!(input.b, 2);
  assert_eq!(input.c, 3);
}

#[test]
fn builder_order_independent() {
  // Can set required fields in any order
  let input1 = PlayerInput::builder()
    .facing_right(true)
    .move_x(1.0)
    .build();

  let input2 = PlayerInput::builder()
    .move_x(1.0)
    .facing_right(true)
    .build();

  assert_eq!(input1, input2);
}

#[test]
fn builder_optional_before_required() {
  // Can set optional fields before required fields
  let input = PlayerInput::builder()
    .jump(true)
    .move_x(1.0)
    .attack(true)
    .facing_right(true)
    .build();

  assert_eq!(input.move_x, 1.0);
  assert!(input.facing_right);
  assert!(input.jump);
  assert!(input.attack);
}

#[test]
fn input_serialization() {
  let input = PlayerInput::builder()
    .move_x(1.5)
    .facing_right(true)
    .jump(true)
    .build();

  let json = serde_json::to_string(&input).unwrap();
  let deserialized: PlayerInput = serde_json::from_str(&json).unwrap();

  assert_eq!(input, deserialized);
}
