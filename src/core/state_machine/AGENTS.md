# AGENTS.md - State Machine Module

## When to Use Each Type

| Use Case | Recommended Type | Rationale |
|----------|------------------|-----------|
| Player character states | `StateMachine` | Well-defined transitions (Idle → Run → Jump) |
| Enemy AI | `StateMachine` | Clear state graph (Patrol → Alert → Chase) |
| Navigation agents | `StateMachine` | Sequential flow (Idle → Computing → Moving → Arrived) |
| Menu systems | `StateStack` | Need to return to previous screen |
| Game pause | `StateStack` | Interrupt and resume gameplay |
| Dialogue/cutscenes | `StateStack` | Nested interrupts, return to original state |
| Modal UI | `StateStack` | Overlay screens that pop back |

## Gotchas

- **`update()` ordering** - `update(dt)` clears the one-frame flags (`just_entered`/`just_exited`; `just_pushed`/`just_popped` on `StateStack`). Read the flags in the same frame as the transition, then call `update(dt)` at the end of the frame. Calling `update()` first means transitions made during the previous frame's logic are never seen. Same for `NavAgent::update`.
- **`AlreadyInState`** - `transition_to` and `force_transition` to the current state return `Err(TransitionError::AlreadyInState)`. Self-transitions never succeed, even if defined with `with_transition(s, s)`.

## Design Decisions

### Why Explicit Transitions?

- **Catches bugs** - invalid transitions fail at runtime
- **Documents behavior** - the transition graph is code
- **Enforces design** - prevents "just add a transition" shortcuts

### Why No Hierarchical States?

Simplicity over HFSM complexity: most game states don't need nesting, composition (FSM + StateStack) covers most cases, and it has fewer edge cases. For hierarchical needs, use multiple or nested state machines.

### Why Separate FSM and Stack?

- FSM: "What state am I in?" (player, AI)
- Stack: "What states am I in, and what do I return to?" (menus, interrupts)

Combining them would complicate both without benefit.
