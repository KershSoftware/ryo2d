//! Finite State Machine implementation.
//!
//! Provides a generic, type-safe state machine with explicit transition rules.

use std::collections::{HashMap, HashSet, VecDeque};
use std::fmt;
use std::hash::Hash;

/// Result type for state transitions.
pub type TransitionResult<S> = Result<S, TransitionError<S>>;

/// Error returned when a state transition fails.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransitionError<S> {
  /// The requested transition is not allowed by the state machine's rules.
  NotAllowed {
    /// The current state
    from: S,
    /// The attempted target state
    to: S,
  },
  /// Already in the target state.
  AlreadyInState(S),
}

impl<S: fmt::Debug> fmt::Display for TransitionError<S> {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    match self {
      TransitionError::NotAllowed { from, to } => {
        write!(f, "transition from {:?} to {:?} is not allowed", from, to)
      }
      TransitionError::AlreadyInState(state) => {
        write!(f, "already in state {:?}", state)
      }
    }
  }
}

impl<S: fmt::Debug> std::error::Error for TransitionError<S> {}

/// A record of a state transition for debugging.
#[derive(Debug, Clone)]
pub struct TransitionRecord<S> {
  /// State transitioned from
  pub from: S,
  /// State transitioned to
  pub to: S,
  /// Time when the transition occurred (seconds since FSM creation)
  pub timestamp: f32,
}

/// A finite state machine with explicit transition rules.
///
/// States are defined by the generic type `S`, which must implement `Clone`, `Eq`, and `Hash`.
/// Transitions between states must be explicitly allowed using builder methods.
///
/// # Type Parameters
///
/// * `S` - The state type. Typically an enum representing possible states.
///
/// # Example
///
/// ```ignore
/// use ryo2d::prelude::*;
///
/// #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
/// enum EnemyState {
///     Patrol,
///     Alert,
///     Chase,
///     Attack,
///     Stunned,
/// }
///
/// let mut fsm = StateMachine::new(EnemyState::Patrol)
///     .with_transition(EnemyState::Patrol, EnemyState::Alert)
///     .with_transition(EnemyState::Alert, EnemyState::Patrol)
///     .with_transition(EnemyState::Alert, EnemyState::Chase)
///     .with_transition(EnemyState::Chase, EnemyState::Attack)
///     .with_transition(EnemyState::Chase, EnemyState::Alert)
///     .with_transition(EnemyState::Attack, EnemyState::Chase)
///     // Stunned can be entered from any state
///     .with_transition_from_any(EnemyState::Stunned)
///     .with_transition(EnemyState::Stunned, EnemyState::Patrol);
///
/// // Use in game loop
/// if player_visible && fsm.current() == &EnemyState::Patrol {
///     fsm.transition_to(EnemyState::Alert);
/// }
/// ```
#[derive(Clone)]
pub struct StateMachine<S>
where
  S: Clone + Eq + Hash,
{
  /// Current state
  current: S,
  /// Previous state (None if never transitioned)
  previous: Option<S>,
  /// Allowed transitions: from -> set of valid targets
  transitions: HashMap<S, HashSet<S>>,
  /// States that can be entered from any other state
  any_transitions: HashSet<S>,
  /// Time spent in current state (seconds)
  time_in_state: f32,
  /// Total time since FSM creation (seconds)
  total_time: f32,
  /// Transition history (if enabled)
  history: Option<VecDeque<TransitionRecord<S>>>,
  /// Maximum history size
  history_capacity: usize,
  /// True for one frame after entering a new state
  pub just_entered: bool,
  /// True for one frame after exiting a state
  pub just_exited: bool,
}

impl<S> fmt::Debug for StateMachine<S>
where
  S: Clone + Eq + Hash + fmt::Debug,
{
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    f.debug_struct("StateMachine")
      .field("current", &self.current)
      .field("previous", &self.previous)
      .field("time_in_state", &self.time_in_state)
      .field("total_time", &self.total_time)
      .field("just_entered", &self.just_entered)
      .field("just_exited", &self.just_exited)
      .field("transitions", &self.transitions.len())
      .field("history_enabled", &self.history.is_some())
      .finish()
  }
}

impl<S> StateMachine<S>
where
  S: Clone + Eq + Hash,
{
  /// Create a new state machine with the given initial state.
  ///
  /// By default, no transitions are allowed. Use `with_transition` to add them.
  ///
  /// # Example
  ///
  /// ```ignore
  /// let fsm = StateMachine::new(PlayerState::Idle);
  /// ```
  pub fn new(initial: S) -> Self {
    Self {
      current: initial,
      previous: None,
      transitions: HashMap::new(),
      any_transitions: HashSet::new(),
      time_in_state: 0.0,
      total_time: 0.0,
      history: None,
      history_capacity: 10,
      just_entered: true,
      just_exited: false,
    }
  }

  /// Allow a transition from one state to another.
  ///
  /// Returns self for builder chaining.
  ///
  /// # Example
  ///
  /// ```ignore
  /// let fsm = StateMachine::new(State::A)
  ///     .with_transition(State::A, State::B)
  ///     .with_transition(State::B, State::A);
  /// ```
  pub fn with_transition(mut self, from: S, to: S) -> Self {
    self.transitions.entry(from).or_default().insert(to);
    self
  }

  /// Allow transitions from one state to multiple target states.
  ///
  /// Returns self for builder chaining.
  ///
  /// # Example
  ///
  /// ```ignore
  /// let fsm = StateMachine::new(State::Idle)
  ///     .with_transitions_from(State::Idle, [State::Walking, State::Jumping, State::Attacking]);
  /// ```
  pub fn with_transitions_from<I>(mut self, from: S, to_states: I) -> Self
  where
    I: IntoIterator<Item = S>,
  {
    let targets = self.transitions.entry(from).or_default();
    for to in to_states {
      targets.insert(to);
    }
    self
  }

  /// Allow a state to be entered from any other state.
  ///
  /// Useful for states like "Stunned" or "Dead" that can interrupt anything.
  ///
  /// # Example
  ///
  /// ```ignore
  /// let fsm = StateMachine::new(State::Idle)
  ///     .with_transition_from_any(State::Dead);  // Can die from any state
  /// ```
  pub fn with_transition_from_any(mut self, to: S) -> Self {
    self.any_transitions.insert(to);
    self
  }

  /// Allow bidirectional transitions between two states.
  ///
  /// Equivalent to calling `with_transition` twice.
  ///
  /// # Example
  ///
  /// ```ignore
  /// let fsm = StateMachine::new(State::Idle)
  ///     .with_bidirectional(State::Idle, State::Walking);  // Can go both ways
  /// ```
  pub fn with_bidirectional(self, a: S, b: S) -> Self
  where
    S: Clone,
  {
    self
      .with_transition(a.clone(), b.clone())
      .with_transition(b, a)
  }

  /// Enable transition history tracking.
  ///
  /// When enabled, the FSM records recent transitions for debugging.
  ///
  /// # Arguments
  ///
  /// * `capacity` - Maximum number of transitions to remember
  ///
  /// # Example
  ///
  /// ```ignore
  /// let fsm = StateMachine::new(State::Idle)
  ///     .with_history(20);  // Remember last 20 transitions
  /// ```
  pub fn with_history(mut self, capacity: usize) -> Self {
    self.history = Some(VecDeque::with_capacity(capacity));
    self.history_capacity = capacity;
    self
  }

  /// Add a transition rule at runtime.
  ///
  /// Unlike builder methods, this modifies the FSM in place.
  pub fn add_transition(&mut self, from: S, to: S) {
    self.transitions.entry(from).or_default().insert(to);
  }

  /// Remove a transition rule at runtime.
  pub fn remove_transition(&mut self, from: &S, to: &S) {
    if let Some(targets) = self.transitions.get_mut(from) {
      targets.remove(to);
    }
  }

  /// Get the current state.
  pub fn current(&self) -> &S {
    &self.current
  }

  /// Get the previous state, if any.
  ///
  /// Returns `None` if no transition has occurred yet.
  pub fn previous(&self) -> Option<&S> {
    self.previous.as_ref()
  }

  /// Check if the FSM is in a specific state.
  pub fn is(&self, state: &S) -> bool {
    &self.current == state
  }

  /// Get the time spent in the current state (in seconds).
  pub fn time_in_state(&self) -> f32 {
    self.time_in_state
  }

  /// Get the total time since FSM creation (in seconds).
  pub fn total_time(&self) -> f32 {
    self.total_time
  }

  /// Check if a transition to a state is allowed.
  pub fn can_transition_to(&self, to: &S) -> bool {
    if &self.current == to {
      return false;
    }

    if self.any_transitions.contains(to) {
      return true;
    }

    self
      .transitions
      .get(&self.current)
      .is_some_and(|targets| targets.contains(to))
  }

  /// Get all states that can be transitioned to from the current state.
  pub fn available_transitions(&self) -> Vec<&S> {
    let mut available = Vec::new();

    // Add transitions from current state
    if let Some(targets) = self.transitions.get(&self.current) {
      for target in targets {
        available.push(target);
      }
    }

    // Add any-transitions
    for target in &self.any_transitions {
      if target != &self.current && !available.contains(&target) {
        available.push(target);
      }
    }

    available
  }

  /// Attempt to transition to a new state.
  ///
  /// Returns `Ok(previous_state)` if the transition succeeded, or an error if:
  /// - The transition is not allowed
  /// - Already in the target state
  ///
  /// # Example
  ///
  /// ```ignore
  /// match fsm.transition_to(State::Running) {
  ///     Ok(prev) => println!("Transitioned from {:?}", prev),
  ///     Err(e) => println!("Transition failed: {}", e),
  /// }
  /// ```
  pub fn transition_to(&mut self, to: S) -> TransitionResult<S> {
    if self.current == to {
      return Err(TransitionError::AlreadyInState(to));
    }

    if !self.can_transition_to(&to) {
      return Err(TransitionError::NotAllowed {
        from: self.current.clone(),
        to,
      });
    }

    self.force_transition(to)
  }

  /// Force a transition to a new state, bypassing transition rules.
  ///
  /// Useful for reset/respawn scenarios where normal rules don't apply.
  ///
  /// Returns `Ok(previous_state)` or `Err` if already in the state.
  ///
  /// # Example
  ///
  /// ```ignore
  /// // Reset to idle state regardless of current state
  /// fsm.force_transition(State::Idle);
  /// ```
  pub fn force_transition(&mut self, to: S) -> TransitionResult<S> {
    if self.current == to {
      return Err(TransitionError::AlreadyInState(to));
    }

    let from = std::mem::replace(&mut self.current, to.clone());
    self.previous = Some(from.clone());

    // Record in history if enabled
    if let Some(ref mut history) = self.history {
      if history.len() >= self.history_capacity {
        history.pop_front();
      }
      history.push_back(TransitionRecord {
        from: from.clone(),
        to,
        timestamp: self.total_time,
      });
    }

    // Reset state timing
    self.time_in_state = 0.0;
    self.just_entered = true;
    self.just_exited = true;

    Ok(from)
  }

  /// Set the state directly without recording history or firing events.
  ///
  /// Use sparingly - primarily for initialization or serialization.
  pub fn set_state(&mut self, state: S) {
    self.current = state;
    self.time_in_state = 0.0;
  }

  /// Update the state machine (call once per frame).
  ///
  /// This updates timing and clears one-frame flags.
  ///
  /// # Arguments
  ///
  /// * `dt` - Delta time in seconds
  pub fn update(&mut self, dt: f32) {
    self.just_entered = false;
    self.just_exited = false;
    self.time_in_state += dt;
    self.total_time += dt;
  }

  /// Get the transition history (if enabled).
  ///
  /// Returns `None` if history tracking was not enabled.
  pub fn history(&self) -> Option<&VecDeque<TransitionRecord<S>>> {
    self.history.as_ref()
  }

  /// Clear the transition history.
  pub fn clear_history(&mut self) {
    if let Some(ref mut history) = self.history {
      history.clear();
    }
  }

  /// Reset the state machine to its initial state.
  ///
  /// This resets timing and history but preserves transition rules.
  pub fn reset(&mut self, initial: S) {
    self.current = initial;
    self.previous = None;
    self.time_in_state = 0.0;
    self.total_time = 0.0;
    self.just_entered = true;
    self.just_exited = false;
    self.clear_history();
  }
}

impl<S> Default for StateMachine<S>
where
  S: Clone + Eq + Hash + Default,
{
  fn default() -> Self {
    Self::new(S::default())
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
  enum TestState {
    Idle,
    Walking,
    Running,
    Jumping,
    Falling,
    Dead,
  }

  #[test]
  fn new_creates_fsm_in_initial_state() {
    let fsm = StateMachine::new(TestState::Idle);
    assert_eq!(fsm.current(), &TestState::Idle);
    assert!(fsm.previous().is_none());
    assert!(fsm.just_entered);
  }

  #[test]
  fn transition_changes_state() {
    let mut fsm =
      StateMachine::new(TestState::Idle).with_transition(TestState::Idle, TestState::Walking);

    let result = fsm.transition_to(TestState::Walking);

    assert!(result.is_ok());
    assert_eq!(result.unwrap(), TestState::Idle);
    assert_eq!(fsm.current(), &TestState::Walking);
    assert_eq!(fsm.previous(), Some(&TestState::Idle));
  }

  #[test]
  fn transition_fails_when_not_allowed() {
    let mut fsm =
      StateMachine::new(TestState::Idle).with_transition(TestState::Idle, TestState::Walking);

    let result = fsm.transition_to(TestState::Running);

    assert!(result.is_err());
    assert!(matches!(result, Err(TransitionError::NotAllowed { .. })));
    assert_eq!(fsm.current(), &TestState::Idle);
  }

  #[test]
  fn transition_fails_when_already_in_state() {
    let mut fsm = StateMachine::new(TestState::Idle);

    let result = fsm.transition_to(TestState::Idle);

    assert!(result.is_err());
    assert!(matches!(result, Err(TransitionError::AlreadyInState(_))));
  }

  #[test]
  fn transition_from_any_works() {
    let mut fsm = StateMachine::new(TestState::Running).with_transition_from_any(TestState::Dead);

    // Should be able to die from Running
    assert!(fsm.can_transition_to(&TestState::Dead));

    let result = fsm.transition_to(TestState::Dead);
    assert!(result.is_ok());
    assert_eq!(fsm.current(), &TestState::Dead);
  }

  #[test]
  fn bidirectional_transition() {
    let mut fsm =
      StateMachine::new(TestState::Idle).with_bidirectional(TestState::Idle, TestState::Walking);

    // Can go Idle -> Walking
    fsm.transition_to(TestState::Walking).unwrap();
    assert_eq!(fsm.current(), &TestState::Walking);

    // Can go Walking -> Idle
    fsm.transition_to(TestState::Idle).unwrap();
    assert_eq!(fsm.current(), &TestState::Idle);
  }

  #[test]
  fn transitions_from_works() {
    let fsm = StateMachine::new(TestState::Idle).with_transitions_from(
      TestState::Idle,
      [TestState::Walking, TestState::Running, TestState::Jumping],
    );

    assert!(fsm.can_transition_to(&TestState::Walking));
    assert!(fsm.can_transition_to(&TestState::Running));
    assert!(fsm.can_transition_to(&TestState::Jumping));
    assert!(!fsm.can_transition_to(&TestState::Falling));
  }

  #[test]
  fn available_transitions_lists_valid_targets() {
    let fsm = StateMachine::new(TestState::Idle)
      .with_transition(TestState::Idle, TestState::Walking)
      .with_transition(TestState::Idle, TestState::Jumping)
      .with_transition_from_any(TestState::Dead);

    let available = fsm.available_transitions();

    assert!(available.contains(&&TestState::Walking));
    assert!(available.contains(&&TestState::Jumping));
    assert!(available.contains(&&TestState::Dead));
    assert!(!available.contains(&&TestState::Running));
  }

  #[test]
  fn force_transition_bypasses_rules() {
    let mut fsm = StateMachine::new(TestState::Idle);
    // No transitions defined, but force should work

    let result = fsm.force_transition(TestState::Running);

    assert!(result.is_ok());
    assert_eq!(fsm.current(), &TestState::Running);
  }

  #[test]
  fn update_advances_time() {
    let mut fsm = StateMachine::new(TestState::Idle);

    fsm.update(0.5);
    assert_eq!(fsm.time_in_state(), 0.5);
    assert_eq!(fsm.total_time(), 0.5);

    fsm.update(0.3);
    assert!((fsm.time_in_state() - 0.8).abs() < 0.001);
    assert!((fsm.total_time() - 0.8).abs() < 0.001);
  }

  #[test]
  fn transition_resets_time_in_state() {
    let mut fsm =
      StateMachine::new(TestState::Idle).with_transition(TestState::Idle, TestState::Walking);

    fsm.update(1.0);
    assert_eq!(fsm.time_in_state(), 1.0);

    fsm.transition_to(TestState::Walking).unwrap();
    assert_eq!(fsm.time_in_state(), 0.0);
    assert_eq!(fsm.total_time(), 1.0); // Total time preserved
  }

  #[test]
  fn just_entered_flag_behavior() {
    let mut fsm =
      StateMachine::new(TestState::Idle).with_transition(TestState::Idle, TestState::Walking);

    assert!(fsm.just_entered); // True on creation

    fsm.update(0.1);
    assert!(!fsm.just_entered); // Cleared after update

    fsm.transition_to(TestState::Walking).unwrap();
    assert!(fsm.just_entered); // True after transition

    fsm.update(0.1);
    assert!(!fsm.just_entered); // Cleared again
  }

  #[test]
  fn just_exited_flag_behavior() {
    let mut fsm =
      StateMachine::new(TestState::Idle).with_transition(TestState::Idle, TestState::Walking);

    assert!(!fsm.just_exited); // False on creation

    fsm.transition_to(TestState::Walking).unwrap();
    assert!(fsm.just_exited); // True after transition

    fsm.update(0.1);
    assert!(!fsm.just_exited); // Cleared after update
  }

  #[test]
  fn history_tracking() {
    let mut fsm = StateMachine::new(TestState::Idle)
      .with_transition(TestState::Idle, TestState::Walking)
      .with_transition(TestState::Walking, TestState::Running)
      .with_history(10);

    fsm.update(0.5);
    fsm.transition_to(TestState::Walking).unwrap();

    fsm.update(0.3);
    fsm.transition_to(TestState::Running).unwrap();

    let history = fsm.history().expect("History should be enabled");
    assert_eq!(history.len(), 2);

    let first = &history[0];
    assert_eq!(first.from, TestState::Idle);
    assert_eq!(first.to, TestState::Walking);
    assert!((first.timestamp - 0.5).abs() < 0.001);

    let second = &history[1];
    assert_eq!(second.from, TestState::Walking);
    assert_eq!(second.to, TestState::Running);
  }

  #[test]
  fn history_respects_capacity() {
    let mut fsm = StateMachine::new(TestState::Idle)
      .with_transition(TestState::Idle, TestState::Walking)
      .with_transition(TestState::Walking, TestState::Idle)
      .with_history(2);

    // Make 3 transitions
    fsm.transition_to(TestState::Walking).unwrap();
    fsm.transition_to(TestState::Idle).unwrap();
    fsm.transition_to(TestState::Walking).unwrap();

    let history = fsm.history().unwrap();
    assert_eq!(history.len(), 2); // Capped at 2

    // First transition should have been evicted
    assert_eq!(history[0].from, TestState::Walking);
    assert_eq!(history[0].to, TestState::Idle);
  }

  #[test]
  fn is_helper_works() {
    let fsm = StateMachine::new(TestState::Idle);

    assert!(fsm.is(&TestState::Idle));
    assert!(!fsm.is(&TestState::Walking));
  }

  #[test]
  fn add_remove_transition_at_runtime() {
    let mut fsm = StateMachine::new(TestState::Idle);

    assert!(!fsm.can_transition_to(&TestState::Walking));

    fsm.add_transition(TestState::Idle, TestState::Walking);
    assert!(fsm.can_transition_to(&TestState::Walking));

    fsm.remove_transition(&TestState::Idle, &TestState::Walking);
    assert!(!fsm.can_transition_to(&TestState::Walking));
  }

  #[test]
  fn reset_restores_initial_state() {
    let mut fsm = StateMachine::new(TestState::Idle)
      .with_transition(TestState::Idle, TestState::Walking)
      .with_history(10);

    fsm.update(1.0);
    fsm.transition_to(TestState::Walking).unwrap();
    fsm.update(0.5);

    fsm.reset(TestState::Idle);

    assert_eq!(fsm.current(), &TestState::Idle);
    assert!(fsm.previous().is_none());
    assert_eq!(fsm.time_in_state(), 0.0);
    assert_eq!(fsm.total_time(), 0.0);
    assert!(fsm.just_entered);
    assert!(fsm.history().unwrap().is_empty());
  }

  #[test]
  fn transition_error_display() {
    let error: TransitionError<TestState> = TransitionError::NotAllowed {
      from: TestState::Idle,
      to: TestState::Dead,
    };
    let msg = format!("{}", error);
    assert!(msg.contains("Idle"));
    assert!(msg.contains("Dead"));
    assert!(msg.contains("not allowed"));

    let error: TransitionError<TestState> = TransitionError::AlreadyInState(TestState::Idle);
    let msg = format!("{}", error);
    assert!(msg.contains("already"));
    assert!(msg.contains("Idle"));
  }

  #[test]
  fn default_impl_works() {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
    enum DefaultState {
      #[default]
      Initial,
      Other,
    }

    let fsm: StateMachine<DefaultState> = StateMachine::default();
    assert_eq!(fsm.current(), &DefaultState::Initial);
  }
}
