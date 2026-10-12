//! Integration tests for NetworkedInputHandler

use ryo2d::core::ecs::{Entity, Transform, World};
use ryo2d::core::input::Input;
use ryo2d::core::network::Channel;
use ryo2d::core::network::input::{
  InputBuffer, InputProcessor, NetworkedInputHandler, SequencedInput,
};
use serde::{Deserialize, Serialize};

/// Simple input type for testing
#[derive(Clone, Default, Debug, PartialEq, Serialize, Deserialize)]
struct TestInput {
  move_x: f32,
  move_y: f32,
  jump: bool,
}

/// Test handler that moves entities
struct MovementHandler {
  entity: Option<Entity>,
  move_speed: f32,
}

impl NetworkedInputHandler for MovementHandler {
  type Input = TestInput;

  fn gather_input(&self, _state: &Input, _dt: f32) -> Self::Input {
    TestInput {
      move_x: 1.0,
      move_y: 0.0,
      jump: false,
    }
  }

  fn apply_input(&self, world: &mut World, entity: Entity, input: &Self::Input, dt: f32) {
    if let Some(mut transform) = world.get_mut::<Transform>(entity) {
      transform.translation.x += input.move_x * self.move_speed * dt;
      transform.translation.y += input.move_y * self.move_speed * dt;
    }
  }

  fn controlled_entity(&self) -> Option<Entity> {
    self.entity
  }
}

#[test]
fn handler_gather_and_apply() {
  let mut world = World::new();
  let entity = world.spawn((Transform::from_xy(0.0, 0.0),)).id();

  let handler = MovementHandler {
    entity: Some(entity),
    move_speed: 100.0,
  };

  let input_state = Input::default();
  let input = handler.gather_input(&input_state, 0.016);

  assert_eq!(input.move_x, 1.0);
  assert_eq!(input.move_y, 0.0);
  assert!(!input.jump);

  handler.apply_input(&mut world, entity, &input, 1.0);

  let transform = world.get::<Transform>(entity).unwrap();
  assert_eq!(transform.translation.x, 100.0);
}

#[test]
fn processor_multiple_frames() {
  let mut world = World::new();
  let entity = world.spawn((Transform::from_xy(0.0, 0.0),)).id();

  let handler = MovementHandler {
    entity: Some(entity),
    move_speed: 60.0,
  };
  let mut processor = InputProcessor::new(handler, 128);
  let input_state = Input::default();

  // Simulate 10 frames
  for i in 0..10 {
    let result = processor.process_client_frame(&mut world, &input_state, 1.0, i as f64);
    assert!(result.is_some());
    assert_eq!(result.unwrap().sequence, (i + 1) as u32);
  }

  // Position should be 10 * 60 = 600
  let transform = world.get::<Transform>(entity).unwrap();
  assert_eq!(transform.translation.x, 600.0);

  // Should have 10 pending inputs
  assert_eq!(processor.pending_count(), 10);
}

#[test]
fn processor_reconciliation_scenario() {
  let mut world = World::new();
  let entity = world.spawn((Transform::from_xy(0.0, 0.0),)).id();

  let handler = MovementHandler {
    entity: Some(entity),
    move_speed: 10.0,
  };
  let mut processor = InputProcessor::new(handler, 128);
  let input_state = Input::default();

  // Client predicts 5 frames
  for i in 0..5 {
    processor.process_client_frame(&mut world, &input_state, 1.0, i as f64);
  }

  // Client thinks position is 50
  let transform = world.get::<Transform>(entity).unwrap();
  assert_eq!(transform.translation.x, 50.0);

  // Server correction: "actually you're at 45 after frame 3"
  // (maybe server had different collision result)
  if let Some(mut t) = world.get_mut::<Transform>(entity) {
    t.translation.x = 45.0;
  }

  // Reconcile: replay inputs 4 and 5
  processor.reconcile(&mut world, 3, 1.0);

  // Should be 45 + 10 + 10 = 65
  let transform = world.get::<Transform>(entity).unwrap();
  assert_eq!(transform.translation.x, 65.0);
}

#[test]
fn input_buffer_sorted_replay() {
  let mut buffer: InputBuffer<TestInput> = InputBuffer::new(64);

  // Add inputs out of order (simulating network reordering)
  buffer.push(SequencedInput::new(
    TestInput {
      move_x: 3.0,
      move_y: 0.0,
      jump: false,
    },
    3,
    0.0,
  ));
  buffer.push(SequencedInput::new(
    TestInput {
      move_x: 1.0,
      move_y: 0.0,
      jump: false,
    },
    1,
    0.0,
  ));
  buffer.push(SequencedInput::new(
    TestInput {
      move_x: 2.0,
      move_y: 0.0,
      jump: false,
    },
    2,
    0.0,
  ));

  // Get inputs after sequence 0
  let mut inputs: Vec<_> = buffer.inputs_after(0).collect();
  inputs.sort_by_key(|i| i.sequence);

  // Should be in order
  assert_eq!(inputs[0].input.move_x, 1.0);
  assert_eq!(inputs[1].input.move_x, 2.0);
  assert_eq!(inputs[2].input.move_x, 3.0);
}

#[test]
fn handler_default_channel() {
  let handler = MovementHandler {
    entity: None,
    move_speed: 1.0,
  };

  assert_eq!(handler.channel(), Channel::ReliableOrdered);
  assert!(handler.predict_locally());
}
