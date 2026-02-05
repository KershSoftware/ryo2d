//! Integration tests for NetworkAction trait

use ryo2d::core::ecs::World;
use ryo2d::core::network::Channel;
use ryo2d::core::network::action::{
  ActionContext, NetworkAction, SerializedAction, apply_received_action,
};
use ryo2d::core::network::rpc::RpcTypeId;
use serde::{Deserialize, Serialize};
use std::cell::RefCell;
use std::rc::Rc;

/// Test action that increments a counter
#[derive(Clone, Debug, Serialize, Deserialize)]
struct IncrementAction {
  amount: i32,
}

impl NetworkAction for IncrementAction {
  fn action_id() -> RpcTypeId {
    RpcTypeId::from_name("Increment")
  }

  fn action_name() -> &'static str {
    "Increment"
  }

  fn apply(&self, ctx: &mut ActionContext) {
    if let Some(counter) = ctx.get_user_data::<Rc<RefCell<i32>>>() {
      *counter.borrow_mut() += self.amount;
    }
  }
}

/// Test action with custom channel
#[derive(Clone, Debug, Serialize, Deserialize)]
struct FrequentAction {
  value: f32,
}

impl NetworkAction for FrequentAction {
  fn action_id() -> RpcTypeId {
    RpcTypeId::from_name("Frequent")
  }

  fn action_name() -> &'static str {
    "Frequent"
  }

  fn apply(&self, _ctx: &mut ActionContext) {}

  fn channel(&self) -> Channel {
    Channel::Unreliable
  }
}

/// Test action that doesn't apply locally
#[derive(Clone, Debug, Serialize, Deserialize)]
struct ServerOnlyAction {
  data: String,
}

impl NetworkAction for ServerOnlyAction {
  fn action_id() -> RpcTypeId {
    RpcTypeId::from_name("ServerOnly")
  }

  fn action_name() -> &'static str {
    "ServerOnly"
  }

  fn apply(&self, _ctx: &mut ActionContext) {}

  fn apply_locally(&self) -> bool {
    false
  }
}

#[test]
fn action_applies_locally() {
  let mut world = World::new();
  let counter = Rc::new(RefCell::new(0));
  let mut counter_ref = counter.clone();

  let action = IncrementAction { amount: 5 };

  {
    let mut ctx = ActionContext::with_user_data(&mut world, &mut counter_ref);
    action.apply(&mut ctx);
  }

  assert_eq!(*counter.borrow(), 5);
}

#[test]
fn action_serialization_preserves_data() {
  let action = IncrementAction { amount: 42 };
  let serialized = SerializedAction::from_action(&action, 123).unwrap();

  assert_eq!(serialized.sender, 123);
  assert_eq!(serialized.action_id, IncrementAction::action_id());

  let bytes = serialized.to_bytes().unwrap();
  let recovered = SerializedAction::from_bytes(&bytes).unwrap();
  let deserialized: IncrementAction = recovered.into_action().unwrap();

  assert_eq!(deserialized.amount, 42);
}

#[test]
fn action_custom_channel() {
  let action = FrequentAction { value: 1.0 };
  assert_eq!(action.channel(), Channel::Unreliable);
}

#[test]
fn action_apply_locally_flag() {
  let normal = IncrementAction { amount: 1 };
  let server_only = ServerOnlyAction {
    data: "test".to_string(),
  };

  assert!(normal.apply_locally());
  assert!(!server_only.apply_locally());
}

#[test]
fn apply_received_action_works() {
  let mut world = World::new();
  let counter = Rc::new(RefCell::new(10));
  let mut counter_ref = counter.clone();

  let action = IncrementAction { amount: 7 };

  {
    let mut ctx = ActionContext::with_user_data(&mut world, &mut counter_ref);
    apply_received_action(action, &mut ctx);
  }

  assert_eq!(*counter.borrow(), 17);
}

#[test]
fn multiple_actions_independent_ids() {
  assert_ne!(IncrementAction::action_id(), FrequentAction::action_id());
  assert_ne!(IncrementAction::action_id(), ServerOnlyAction::action_id());
}
