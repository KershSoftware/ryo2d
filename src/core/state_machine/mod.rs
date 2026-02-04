//! Generic state machine framework for game development.
//!
//! Provides reusable state machine implementations for managing game states,
//! character behaviors, UI flows, and more.
//!
//! # Overview
//!
//! This module provides two state machine types:
//!
//! - [`StateMachine`] - A finite state machine (FSM) with explicit transitions
//! - [`StateStack`] - A pushdown automata for interruptible/nested states
//!
//! # When to Use Each Type
//!
//! | Use Case | Recommended Type |
//! |----------|------------------|
//! | Player states (idle, run, jump) | `StateMachine` |
//! | Enemy AI (patrol, alert, chase) | `StateMachine` |
//! | Menu navigation (push/pop screens) | `StateStack` |
//! | Game modes (play → pause → play) | `StateStack` |
//! | Cutscene interrupts | `StateStack` |
//!
//! # Example: Basic FSM
//!
//! ```ignore
//! use ryo2d::prelude::*;
//!
//! #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
//! enum PlayerState {
//!     Idle,
//!     Running,
//!     Jumping,
//!     Falling,
//! }
//!
//! // Create state machine with allowed transitions
//! let mut fsm = StateMachine::new(PlayerState::Idle)
//!     .with_transition(PlayerState::Idle, PlayerState::Running)
//!     .with_transition(PlayerState::Idle, PlayerState::Jumping)
//!     .with_transition(PlayerState::Running, PlayerState::Idle)
//!     .with_transition(PlayerState::Running, PlayerState::Jumping)
//!     .with_transition(PlayerState::Jumping, PlayerState::Falling)
//!     .with_transition(PlayerState::Falling, PlayerState::Idle);
//!
//! // Transition between states
//! fsm.transition_to(PlayerState::Running); // Ok
//! fsm.transition_to(PlayerState::Jumping); // Ok
//! fsm.transition_to(PlayerState::Idle);    // Error: can't go Jumping -> Idle
//! ```
//!
//! # Example: State Stack
//!
//! ```ignore
//! use ryo2d::prelude::*;
//!
//! #[derive(Debug, Clone, PartialEq)]
//! enum GameState {
//!     Playing,
//!     Paused,
//!     Inventory,
//!     Dialogue(String),
//! }
//!
//! let mut stack = StateStack::new(GameState::Playing);
//!
//! // Push states onto the stack
//! stack.push(GameState::Paused);
//! // Stack: [Playing, Paused]
//!
//! stack.push(GameState::Inventory);
//! // Stack: [Playing, Paused, Inventory]
//!
//! // Pop to return to previous state
//! stack.pop();
//! // Stack: [Playing, Paused]
//!
//! assert_eq!(stack.current(), &GameState::Paused);
//! ```
//!
//! # ECS Integration
//!
//! State machines work well as ECS components:
//!
//! ```ignore
//! // Attach to entities
//! world.spawn((
//!     Transform::new(100.0, 100.0),
//!     CharacterFsm(StateMachine::new(PlayerState::Idle)),
//! ));
//!
//! // Process in systems
//! for (entity, (fsm, input)) in world.query::<(&mut CharacterFsm, &Input)>() {
//!     if input.jump_pressed && fsm.0.current() == &PlayerState::Idle {
//!         fsm.0.transition_to(PlayerState::Jumping);
//!     }
//! }
//! ```

mod fsm;
mod stack;

pub use fsm::{StateMachine, TransitionError, TransitionResult};
pub use stack::StateStack;
