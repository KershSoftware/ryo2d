//! State Stack implementation (Pushdown Automata).
//!
//! Provides a stack-based state machine for interruptible and nested states.

use std::fmt;

/// A stack-based state machine for interruptible states.
///
/// Unlike a standard FSM, a state stack allows states to be pushed and popped,
/// making it ideal for scenarios where you need to temporarily enter a state
/// and return to the previous one.
///
/// # Use Cases
///
/// - **Menu navigation**: Main menu → Settings → Controls → back to Settings → back to Main
/// - **Game pause**: Playing → Paused → Playing
/// - **Dialogue interrupts**: Playing → Cutscene → Dialogue → back to Cutscene → back to Playing
/// - **Modal UI**: Game → Inventory → Item details → back to Inventory
///
/// # Example
///
/// ```ignore
/// use ryo2d::prelude::*;
///
/// #[derive(Debug, Clone, PartialEq)]
/// enum GameState {
///     MainMenu,
///     Playing,
///     Paused,
///     Inventory,
///     Settings,
/// }
///
/// let mut stack = StateStack::new(GameState::MainMenu);
///
/// // Start game
/// stack.push(GameState::Playing);
/// // Stack: [MainMenu, Playing]
///
/// // Open pause menu
/// stack.push(GameState::Paused);
/// // Stack: [MainMenu, Playing, Paused]
///
/// // Open settings from pause
/// stack.push(GameState::Settings);
/// // Stack: [MainMenu, Playing, Paused, Settings]
///
/// // Close settings (back to pause)
/// stack.pop();
/// // Stack: [MainMenu, Playing, Paused]
///
/// // Resume game
/// stack.pop();
/// // Stack: [MainMenu, Playing]
///
/// assert_eq!(stack.current(), &GameState::Playing);
/// ```
#[derive(Clone)]
pub struct StateStack<S>
where
  S: Clone + PartialEq,
{
  /// The state stack (bottom to top)
  stack: Vec<S>,
  /// Maximum allowed stack depth (prevents runaway pushes)
  max_depth: usize,
  /// Time spent in current top state (seconds)
  time_in_state: f32,
  /// Total time since stack creation (seconds)
  total_time: f32,
  /// True for one frame after pushing a state
  pub just_pushed: bool,
  /// True for one frame after popping a state
  pub just_popped: bool,
  /// The state that was just popped (available for one frame)
  popped_state: Option<S>,
}

impl<S> fmt::Debug for StateStack<S>
where
  S: Clone + PartialEq + fmt::Debug,
{
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    f.debug_struct("StateStack")
      .field("stack", &self.stack)
      .field("depth", &self.stack.len())
      .field("max_depth", &self.max_depth)
      .field("time_in_state", &self.time_in_state)
      .field("just_pushed", &self.just_pushed)
      .field("just_popped", &self.just_popped)
      .finish()
  }
}

impl<S> StateStack<S>
where
  S: Clone + PartialEq,
{
  /// Create a new state stack with the given initial (root) state.
  ///
  /// The root state cannot be popped.
  ///
  /// # Example
  ///
  /// ```ignore
  /// let stack = StateStack::new(GameState::MainMenu);
  /// ```
  pub fn new(initial: S) -> Self {
    Self {
      stack: vec![initial],
      max_depth: 32,
      time_in_state: 0.0,
      total_time: 0.0,
      just_pushed: true,
      just_popped: false,
      popped_state: None,
    }
  }

  /// Set the maximum stack depth.
  ///
  /// Prevents runaway pushes from consuming unbounded memory.
  /// Default is 32.
  ///
  /// # Panics
  ///
  /// Panics if `max_depth` is 0.
  pub fn with_max_depth(mut self, max_depth: usize) -> Self {
    assert!(max_depth > 0, "max_depth must be at least 1");
    self.max_depth = max_depth;
    self
  }

  /// Get the current (top) state.
  ///
  /// # Panics
  ///
  /// Panics if the stack is empty (should never happen with normal usage).
  pub fn current(&self) -> &S {
    self.stack.last().expect("StateStack should never be empty")
  }

  /// Get the current (top) state mutably.
  pub fn current_mut(&mut self) -> &mut S {
    self
      .stack
      .last_mut()
      .expect("StateStack should never be empty")
  }

  /// Get the root (bottom) state.
  pub fn root(&self) -> &S {
    self
      .stack
      .first()
      .expect("StateStack should never be empty")
  }

  /// Get the state below the current one, if any.
  ///
  /// Returns `None` if there's only one state (the root).
  pub fn previous(&self) -> Option<&S> {
    if self.stack.len() > 1 {
      self.stack.get(self.stack.len() - 2)
    } else {
      None
    }
  }

  /// Get the state that was just popped (available for one frame after pop).
  pub fn popped(&self) -> Option<&S> {
    self.popped_state.as_ref()
  }

  /// Check if the current state matches the given state.
  pub fn is(&self, state: &S) -> bool {
    self.current() == state
  }

  /// Check if a state exists anywhere in the stack.
  pub fn contains(&self, state: &S) -> bool {
    self.stack.contains(state)
  }

  /// Get the current stack depth.
  pub fn depth(&self) -> usize {
    self.stack.len()
  }

  /// Check if only the root state remains.
  pub fn is_at_root(&self) -> bool {
    self.stack.len() == 1
  }

  /// Get the time spent in the current state (in seconds).
  pub fn time_in_state(&self) -> f32 {
    self.time_in_state
  }

  /// Get the total time since stack creation (in seconds).
  pub fn total_time(&self) -> f32 {
    self.total_time
  }

  /// Push a new state onto the stack.
  ///
  /// Returns `true` if the state was pushed, `false` if at max depth.
  ///
  /// # Example
  ///
  /// ```ignore
  /// stack.push(GameState::Paused);
  /// ```
  pub fn push(&mut self, state: S) -> bool {
    if self.stack.len() >= self.max_depth {
      return false;
    }

    self.stack.push(state);
    self.time_in_state = 0.0;
    self.just_pushed = true;
    self.just_popped = false;
    self.popped_state = None;
    true
  }

  /// Push a state only if not already the current state.
  ///
  /// Returns `true` if the state was pushed.
  pub fn push_if_not_current(&mut self, state: S) -> bool {
    if self.current() != &state {
      self.push(state)
    } else {
      false
    }
  }

  /// Pop the current state and return to the previous one.
  ///
  /// Returns the popped state, or `None` if at the root (root cannot be popped).
  ///
  /// # Example
  ///
  /// ```ignore
  /// if let Some(popped) = stack.pop() {
  ///     println!("Left state: {:?}", popped);
  /// }
  /// ```
  pub fn pop(&mut self) -> Option<S> {
    if self.stack.len() <= 1 {
      return None;
    }

    let popped = self.stack.pop();
    self.time_in_state = 0.0;
    self.just_pushed = false;
    self.just_popped = true;
    self.popped_state = popped.clone();
    popped
  }

  /// Pop states until reaching a specific state.
  ///
  /// Returns the number of states popped, or 0 if the state wasn't found.
  /// The target state becomes the new current state.
  ///
  /// # Example
  ///
  /// ```ignore
  /// // Stack: [MainMenu, Playing, Paused, Inventory, ItemDetails]
  /// stack.pop_to(&GameState::Playing);
  /// // Stack: [MainMenu, Playing]
  /// ```
  pub fn pop_to(&mut self, target: &S) -> usize {
    // First check if target exists in stack (excluding current)
    let target_index = self.stack.iter().position(|s| s == target);

    // If target not found, or it's already current, do nothing
    let target_index = match target_index {
      Some(idx) if idx < self.stack.len() - 1 => idx,
      _ => return 0,
    };

    // Pop until we reach the target
    let count = self.stack.len() - 1 - target_index;
    self.stack.truncate(target_index + 1);

    if count > 0 {
      self.time_in_state = 0.0;
      self.just_pushed = false;
      self.just_popped = true;
      self.popped_state = None; // Multiple pops, no single popped state
    }

    count
  }

  /// Pop all states and return to the root.
  ///
  /// Returns the number of states popped.
  pub fn pop_to_root(&mut self) -> usize {
    let count = self.stack.len() - 1;

    if count > 0 {
      self.stack.truncate(1);
      self.time_in_state = 0.0;
      self.just_pushed = false;
      self.just_popped = true;
      self.popped_state = None;
    }

    count
  }

  /// Replace the current state with a new state (pop + push).
  ///
  /// Returns the replaced state, or `None` if at root and replace failed.
  ///
  /// # Example
  ///
  /// ```ignore
  /// // Stack: [MainMenu, Playing]
  /// stack.replace(GameState::Cutscene);
  /// // Stack: [MainMenu, Cutscene]
  /// ```
  pub fn replace(&mut self, state: S) -> Option<S> {
    if self.stack.len() == 1 {
      // At root - just replace it
      let old = std::mem::replace(&mut self.stack[0], state);
      self.time_in_state = 0.0;
      self.just_pushed = true;
      self.just_popped = true;
      self.popped_state = Some(old.clone());
      return Some(old);
    }

    let popped = self.stack.pop();
    self.stack.push(state);
    self.time_in_state = 0.0;
    self.just_pushed = true;
    self.just_popped = true;
    self.popped_state = popped.clone();
    popped
  }

  /// Clear all states and set a new root state.
  ///
  /// Useful for major state changes like returning to main menu.
  pub fn reset(&mut self, new_root: S) {
    self.stack.clear();
    self.stack.push(new_root);
    self.time_in_state = 0.0;
    self.total_time = 0.0;
    self.just_pushed = true;
    self.just_popped = false;
    self.popped_state = None;
  }

  /// Update the state stack (call once per frame).
  ///
  /// This updates timing and clears one-frame flags.
  ///
  /// # Arguments
  ///
  /// * `dt` - Delta time in seconds
  pub fn update(&mut self, dt: f32) {
    self.just_pushed = false;
    self.just_popped = false;
    self.popped_state = None;
    self.time_in_state += dt;
    self.total_time += dt;
  }

  /// Get an iterator over all states from bottom to top.
  pub fn iter(&self) -> impl Iterator<Item = &S> {
    self.stack.iter()
  }

  /// Get the entire stack as a slice.
  pub fn as_slice(&self) -> &[S] {
    &self.stack
  }
}

impl<S> Default for StateStack<S>
where
  S: Clone + PartialEq + Default,
{
  fn default() -> Self {
    Self::new(S::default())
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[derive(Debug, Clone, PartialEq)]
  enum TestState {
    MainMenu,
    Playing,
    Paused,
    Inventory,
    Settings,
  }

  #[test]
  fn new_creates_stack_with_root() {
    let stack = StateStack::new(TestState::MainMenu);

    assert_eq!(stack.current(), &TestState::MainMenu);
    assert_eq!(stack.root(), &TestState::MainMenu);
    assert_eq!(stack.depth(), 1);
    assert!(stack.is_at_root());
    assert!(stack.just_pushed);
  }

  #[test]
  fn push_adds_state() {
    let mut stack = StateStack::new(TestState::MainMenu);

    stack.push(TestState::Playing);

    assert_eq!(stack.current(), &TestState::Playing);
    assert_eq!(stack.previous(), Some(&TestState::MainMenu));
    assert_eq!(stack.depth(), 2);
    assert!(!stack.is_at_root());
    assert!(stack.just_pushed);
  }

  #[test]
  fn pop_removes_state() {
    let mut stack = StateStack::new(TestState::MainMenu);
    stack.push(TestState::Playing);
    stack.push(TestState::Paused);

    let popped = stack.pop();

    assert_eq!(popped, Some(TestState::Paused));
    assert_eq!(stack.current(), &TestState::Playing);
    assert_eq!(stack.depth(), 2);
    assert!(stack.just_popped);
    assert_eq!(stack.popped(), Some(&TestState::Paused));
  }

  #[test]
  fn cannot_pop_root() {
    let mut stack = StateStack::new(TestState::MainMenu);

    let popped = stack.pop();

    assert!(popped.is_none());
    assert_eq!(stack.current(), &TestState::MainMenu);
    assert_eq!(stack.depth(), 1);
  }

  #[test]
  fn push_respects_max_depth() {
    let mut stack = StateStack::new(TestState::MainMenu).with_max_depth(3);

    assert!(stack.push(TestState::Playing));
    assert!(stack.push(TestState::Paused));
    assert!(!stack.push(TestState::Inventory)); // Should fail

    assert_eq!(stack.depth(), 3);
    assert_eq!(stack.current(), &TestState::Paused);
  }

  #[test]
  fn push_if_not_current_works() {
    let mut stack = StateStack::new(TestState::MainMenu);

    assert!(stack.push_if_not_current(TestState::Playing));
    assert!(!stack.push_if_not_current(TestState::Playing)); // Same state

    assert_eq!(stack.depth(), 2);
  }

  #[test]
  fn pop_to_works() {
    let mut stack = StateStack::new(TestState::MainMenu);
    stack.push(TestState::Playing);
    stack.push(TestState::Paused);
    stack.push(TestState::Inventory);
    stack.push(TestState::Settings);

    let count = stack.pop_to(&TestState::Playing);

    assert_eq!(count, 3);
    assert_eq!(stack.current(), &TestState::Playing);
    assert!(stack.just_popped);
  }

  #[test]
  fn pop_to_not_found_pops_nothing() {
    let mut stack = StateStack::new(TestState::MainMenu);
    stack.push(TestState::Playing);

    let count = stack.pop_to(&TestState::Settings); // Not in stack

    assert_eq!(count, 0);
    assert_eq!(stack.current(), &TestState::Playing);
  }

  #[test]
  fn pop_to_root_works() {
    let mut stack = StateStack::new(TestState::MainMenu);
    stack.push(TestState::Playing);
    stack.push(TestState::Paused);
    stack.push(TestState::Inventory);

    let count = stack.pop_to_root();

    assert_eq!(count, 3);
    assert_eq!(stack.current(), &TestState::MainMenu);
    assert!(stack.is_at_root());
  }

  #[test]
  fn replace_works() {
    let mut stack = StateStack::new(TestState::MainMenu);
    stack.push(TestState::Playing);

    let replaced = stack.replace(TestState::Paused);

    assert_eq!(replaced, Some(TestState::Playing));
    assert_eq!(stack.current(), &TestState::Paused);
    assert_eq!(stack.depth(), 2);
    assert!(stack.just_pushed);
    assert!(stack.just_popped);
  }

  #[test]
  fn replace_at_root_replaces_root() {
    let mut stack = StateStack::new(TestState::MainMenu);

    let replaced = stack.replace(TestState::Playing);

    assert_eq!(replaced, Some(TestState::MainMenu));
    assert_eq!(stack.current(), &TestState::Playing);
    assert_eq!(stack.root(), &TestState::Playing);
    assert_eq!(stack.depth(), 1);
  }

  #[test]
  fn contains_works() {
    let mut stack = StateStack::new(TestState::MainMenu);
    stack.push(TestState::Playing);
    stack.push(TestState::Paused);

    assert!(stack.contains(&TestState::MainMenu));
    assert!(stack.contains(&TestState::Playing));
    assert!(stack.contains(&TestState::Paused));
    assert!(!stack.contains(&TestState::Inventory));
  }

  #[test]
  fn is_helper_works() {
    let stack = StateStack::new(TestState::MainMenu);

    assert!(stack.is(&TestState::MainMenu));
    assert!(!stack.is(&TestState::Playing));
  }

  #[test]
  fn update_advances_time() {
    let mut stack = StateStack::new(TestState::MainMenu);

    stack.update(0.5);
    assert_eq!(stack.time_in_state(), 0.5);
    assert_eq!(stack.total_time(), 0.5);

    stack.update(0.3);
    assert!((stack.time_in_state() - 0.8).abs() < 0.001);
  }

  #[test]
  fn push_resets_time_in_state() {
    let mut stack = StateStack::new(TestState::MainMenu);

    stack.update(1.0);
    assert_eq!(stack.time_in_state(), 1.0);

    stack.push(TestState::Playing);
    assert_eq!(stack.time_in_state(), 0.0);
    assert_eq!(stack.total_time(), 1.0); // Total preserved
  }

  #[test]
  fn update_clears_flags() {
    let mut stack = StateStack::new(TestState::MainMenu);

    assert!(stack.just_pushed);

    stack.update(0.1);

    assert!(!stack.just_pushed);
    assert!(!stack.just_popped);
    assert!(stack.popped().is_none());
  }

  #[test]
  fn reset_clears_everything() {
    let mut stack = StateStack::new(TestState::MainMenu);
    stack.push(TestState::Playing);
    stack.push(TestState::Paused);
    stack.update(1.0);

    stack.reset(TestState::Settings);

    assert_eq!(stack.current(), &TestState::Settings);
    assert_eq!(stack.depth(), 1);
    assert_eq!(stack.time_in_state(), 0.0);
    assert_eq!(stack.total_time(), 0.0);
    assert!(stack.just_pushed);
  }

  #[test]
  fn iter_returns_bottom_to_top() {
    let mut stack = StateStack::new(TestState::MainMenu);
    stack.push(TestState::Playing);
    stack.push(TestState::Paused);

    let states: Vec<_> = stack.iter().collect();

    assert_eq!(states.len(), 3);
    assert_eq!(states[0], &TestState::MainMenu);
    assert_eq!(states[1], &TestState::Playing);
    assert_eq!(states[2], &TestState::Paused);
  }

  #[test]
  fn as_slice_works() {
    let mut stack = StateStack::new(TestState::MainMenu);
    stack.push(TestState::Playing);

    let slice = stack.as_slice();

    assert_eq!(slice.len(), 2);
    assert_eq!(slice[0], TestState::MainMenu);
    assert_eq!(slice[1], TestState::Playing);
  }

  #[test]
  fn current_mut_allows_modification() {
    let mut stack = StateStack::new(TestState::MainMenu);
    stack.push(TestState::Playing);

    *stack.current_mut() = TestState::Paused;

    assert_eq!(stack.current(), &TestState::Paused);
  }

  #[test]
  #[should_panic(expected = "max_depth must be at least 1")]
  fn zero_max_depth_panics() {
    StateStack::new(TestState::MainMenu).with_max_depth(0);
  }

  #[test]
  fn default_impl_works() {
    #[derive(Debug, Clone, PartialEq, Default)]
    enum DefaultState {
      #[default]
      Initial,
      Other,
    }

    let stack: StateStack<DefaultState> = StateStack::default();
    assert_eq!(stack.current(), &DefaultState::Initial);
  }
}
