# AGENTS.md - State Machine Module

## Overview

This module provides a generic, type-safe state machine framework for game development. It includes two implementations: a finite state machine (FSM) with explicit transitions and a state stack (pushdown automata) for nested/interruptible states.

## File Structure

| File | Purpose |
|------|---------|
| `mod.rs` | Public exports and module documentation |
| `fsm.rs` | `StateMachine<S>` - Finite state machine with transition rules |
| `stack.rs` | `StateStack<S>` - Push/pop state stack for nested states |

## Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                     Game Code                                │
│  - Define state enums (PlayerState, EnemyState, etc.)        │
│  - Create StateMachine or StateStack instances               │
│  - Call transition methods based on game logic               │
└─────────────────────────────────────────────────────────────┘
                           │
          ┌────────────────┼────────────────┐
          ▼                                  ▼
┌─────────────────────┐           ┌─────────────────────┐
│   StateMachine<S>   │           │    StateStack<S>    │
│                     │           │                     │
│  - Explicit rules   │           │  - Push/pop states  │
│  - Validated trans. │           │  - Nested states    │
│  - Time tracking    │           │  - Return to prev   │
│  - One-frame flags  │           │  - Max depth limit  │
└─────────────────────┘           └─────────────────────┘
```

## Key Concepts

### When to Use Each Type

| Use Case | Recommended Type | Rationale |
|----------|------------------|-----------|
| Player character states | `StateMachine` | Well-defined transitions (Idle → Run → Jump) |
| Enemy AI | `StateMachine` | Clear state graph (Patrol → Alert → Chase) |
| Navigation agents | `StateMachine` | Sequential flow (Idle → Computing → Moving → Arrived) |
| Menu systems | `StateStack` | Need to return to previous screen |
| Game pause | `StateStack` | Interrupt and resume gameplay |
| Dialogue/cutscenes | `StateStack` | Nested interrupts, return to original state |
| Modal UI | `StateStack` | Overlay screens that pop back |

### StateMachine<S> - Finite State Machine

A validated FSM that only allows explicitly defined transitions:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum EnemyState {
    Patrol,
    Alert,
    Chase,
    Attack,
    Dead,
}

// Build the transition graph
let mut fsm = StateMachine::new(EnemyState::Patrol)
    .with_transition(EnemyState::Patrol, EnemyState::Alert)
    .with_transition(EnemyState::Alert, EnemyState::Patrol)
    .with_transition(EnemyState::Alert, EnemyState::Chase)
    .with_transition(EnemyState::Chase, EnemyState::Attack)
    .with_transition(EnemyState::Attack, EnemyState::Chase)
    // Dead can be entered from any state
    .with_transition_from_any(EnemyState::Dead);

// Transitions are validated
fsm.transition_to(EnemyState::Alert);  // Ok
fsm.transition_to(EnemyState::Chase);  // Ok
fsm.transition_to(EnemyState::Patrol); // Error: Chase -> Patrol not allowed
```

**Key Features:**

- **Explicit transitions** - Only allowed transitions succeed
- **Time tracking** - `time_in_state()`, `total_time()`
- **One-frame flags** - `just_entered`, `just_exited`
- **History tracking** - Optional transition history for debugging
- **Force transitions** - `force_transition()` bypasses rules for resets

### StateStack<S> - Pushdown Automata

A stack-based state machine for interruptible and nested states:

```rust
#[derive(Debug, Clone, PartialEq)]
enum GameState {
    MainMenu,
    Playing,
    Paused,
    Settings,
}

let mut stack = StateStack::new(GameState::MainMenu);

// Push states onto the stack
stack.push(GameState::Playing);
// Stack: [MainMenu, Playing]

stack.push(GameState::Paused);
// Stack: [MainMenu, Playing, Paused]

stack.push(GameState::Settings);
// Stack: [MainMenu, Playing, Paused, Settings]

// Pop to return to previous state
stack.pop();  // Returns Settings
// Stack: [MainMenu, Playing, Paused]

stack.pop();  // Returns Paused
// Stack: [MainMenu, Playing]

assert_eq!(stack.current(), &GameState::Playing);
```

**Key Features:**

- **Push/pop** - Add states, return to previous
- **Protected root** - Bottom state cannot be popped
- **Pop-to** - Return to a specific state in the stack
- **Max depth** - Prevent runaway pushes (default 32)
- **Replace** - Pop and push in one operation

## State Type Requirements

### StateMachine<S>

```rust
S: Clone + Eq + Hash
```

- `Clone` - States can be cloned for history/previous tracking
- `Eq` - States can be compared for equality
- `Hash` - States can be stored in HashSet/HashMap for transition rules

### StateStack<S>

```rust
S: Clone + PartialEq
```

- `Clone` - States can be cloned when popping
- `PartialEq` - States can be compared for `pop_to()`, `contains()`

## Usage Patterns

### Basic FSM Setup

```rust
// 1. Define state enum with required derives
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum PlayerState {
    Idle,
    Running,
    Jumping,
    Falling,
    Attacking,
}

// 2. Create state machine with transitions
let mut fsm = StateMachine::new(PlayerState::Idle)
    .with_bidirectional(PlayerState::Idle, PlayerState::Running)
    .with_transitions_from(PlayerState::Idle, [
        PlayerState::Jumping,
        PlayerState::Attacking,
    ])
    .with_transitions_from(PlayerState::Running, [
        PlayerState::Jumping,
        PlayerState::Attacking,
    ])
    .with_transition(PlayerState::Jumping, PlayerState::Falling)
    .with_transition(PlayerState::Falling, PlayerState::Idle)
    .with_transition(PlayerState::Attacking, PlayerState::Idle);

// 3. Use in game loop
fn update(&mut self, dt: f32) {
    // Update timing first
    self.fsm.update(dt);

    // Handle one-frame events
    if self.fsm.just_entered {
        match self.fsm.current() {
            PlayerState::Jumping => self.play_sound("jump.wav"),
            PlayerState::Attacking => self.play_sound("attack.wav"),
            _ => {}
        }
    }

    // State-specific logic
    match self.fsm.current() {
        PlayerState::Idle => {
            if input.move_x != 0.0 {
                self.fsm.transition_to(PlayerState::Running);
            }
        }
        PlayerState::Running => {
            if input.move_x == 0.0 {
                self.fsm.transition_to(PlayerState::Idle);
            }
        }
        // ...
    }
}
```

### ECS Component Pattern

```rust
// Wrapper component for better type safety
struct PlayerFsm(StateMachine<PlayerState>);

// Spawn with component
world.spawn((
    Transform::new(100.0, 100.0),
    PlayerFsm(StateMachine::new(PlayerState::Idle)
        .with_transition(PlayerState::Idle, PlayerState::Running)),
));

// Query and update
for (entity, (fsm, input)) in world.query_mut::<(&mut PlayerFsm, &Input)>() {
    fsm.0.update(dt);

    if fsm.0.just_entered && fsm.0.is(&PlayerState::Attacking) {
        // Emit event for damage system
        events.emit(StateChanged::new(entity, PlayerState::Idle, PlayerState::Attacking));
    }
}
```

### Menu Navigation with StateStack

```rust
#[derive(Debug, Clone, PartialEq)]
enum MenuState {
    Main,
    Settings,
    Audio,
    Video,
    Controls,
}

let mut menu = StateStack::new(MenuState::Main);

// Navigate into settings
fn on_settings_clicked(&mut self) {
    self.menu.push(MenuState::Settings);
}

// Navigate into audio settings
fn on_audio_clicked(&mut self) {
    self.menu.push(MenuState::Audio);
}

// Back button - return to previous
fn on_back_clicked(&mut self) {
    self.menu.pop();
}

// Check where we are
fn render(&self, ctx: &mut RenderContext) {
    match self.menu.current() {
        MenuState::Main => self.render_main_menu(ctx),
        MenuState::Settings => self.render_settings(ctx),
        MenuState::Audio => self.render_audio_settings(ctx),
        // ...
    }
}
```

### Time-Based State Changes

```rust
// Auto-expire alert state after 3 seconds
if fsm.is(&EnemyState::Alert) && fsm.time_in_state() > 3.0 {
    fsm.transition_to(EnemyState::Patrol);
}

// Combo window - must attack again within 0.5s
if fsm.is(&PlayerState::AttackRecovery) && fsm.time_in_state() > 0.5 {
    self.combo_count = 0;
    fsm.transition_to(PlayerState::Idle);
}
```

### Debugging with History

```rust
let mut fsm = StateMachine::new(State::Idle)
    .with_history(20);  // Remember last 20 transitions

// Later, debug state history
if let Some(history) = fsm.history() {
    for record in history {
        println!("{:.2}s: {:?} -> {:?}",
            record.timestamp, record.from, record.to);
    }
}
```

## Engine Integration

### NavAgent

The `NavAgent` component uses `StateMachine<NavAgentState>` internally:

```rust
pub struct NavAgent {
    state_machine: StateMachine<NavAgentState>,
    // ... other fields
}

// Access state via methods
agent.state()              // Current state
agent.previous_state()     // Previous state (if any)
agent.just_entered_state() // True for one frame after transition
agent.time_in_state()      // Seconds in current state
agent.can_transition_to()  // Check if transition is valid
```

### StateChanged Events

Emit `StateChanged<S>` events to decouple state logic from reactions:

```rust
// When state changes
if fsm.transition_to(EnemyState::Alert).is_ok() {
    events.emit(StateChanged::new(
        entity,
        EnemyState::Patrol,
        EnemyState::Alert,
    ));
}

// React in other systems
for event in ctx.events.read::<StateChanged<EnemyState>>() {
    if event.changed_to(&EnemyState::Alert) {
        // Play alert sound, show exclamation mark
    }
}
```

## Design Decisions

### Why Explicit Transitions?

Requiring explicit transition definitions:
- **Catches bugs** - Invalid transitions fail at runtime
- **Documents behavior** - Transition graph is code
- **Enforces design** - Prevents "just add a transition" shortcuts

### Why No Hierarchical States?

We chose simplicity over HFSM complexity:
- Most game states don't need nesting
- Composition (FSM + StateStack) handles most cases
- Lower learning curve and fewer edge cases

For hierarchical needs, use multiple state machines or nest StateMachines.

### Why Separate FSM and Stack?

Different use cases need different semantics:
- FSM: "What state am I in?" (player, AI)
- Stack: "What states am I in, and what to return to?" (menus, interrupts)

Combining them would complicate both without benefit.

## Performance Notes

- **StateMachine** - O(1) transition check (HashSet lookup)
- **StateStack** - O(1) push/pop, O(n) for `pop_to()` and `contains()`
- **Memory** - Small footprint (one state + transition table)
- **No allocations** - After initial setup, no runtime allocations

## Testing

Both types include extensive unit tests. Key test patterns:

```rust
// Test valid transitions
#[test]
fn can_transition_through_valid_path() {
    let mut fsm = create_fsm();
    assert!(fsm.transition_to(State::B).is_ok());
    assert!(fsm.transition_to(State::C).is_ok());
}

// Test invalid transitions are rejected
#[test]
fn rejects_invalid_transition() {
    let mut fsm = StateMachine::new(State::A);
    // No transition defined
    assert!(matches!(
        fsm.transition_to(State::B),
        Err(TransitionError::NotAllowed { .. })
    ));
}

// Test timing behavior
#[test]
fn time_resets_on_transition() {
    let mut fsm = create_fsm();
    fsm.update(1.0);
    fsm.transition_to(State::B);
    assert_eq!(fsm.time_in_state(), 0.0);
}
```
