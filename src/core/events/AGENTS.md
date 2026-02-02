# AGENTS.md - Events Module

## Overview

The `events/` module provides a **decoupled publish-subscribe event bus** for inter-system communication. Events allow different parts of the game to communicate without direct dependencies.

## File Structure

| File | Purpose |
|------|---------|
| `mod.rs` | `Events` struct, `Event` trait, public API |
| `channel.rs` | Internal `EventChannel<E>` storage per event type |
| `builtin.rs` | Built-in event types (collision, network, asset) |

## Core Concepts

### Event Bus Pattern

The event bus decouples event producers from consumers:

```
┌─────────────┐     emit()      ┌─────────────┐
│   Combat    │ ───────────────▶│             │
│   System    │                 │             │
└─────────────┘                 │   Events    │
                                │    Bus      │
┌─────────────┐     emit()      │             │
│   Physics   │ ───────────────▶│             │
│   System    │                 └──────┬──────┘
└─────────────┘                        │
                                       │ read()
              ┌────────────────────────┼────────────────────────┐
              ▼                        ▼                        ▼
       ┌─────────────┐          ┌─────────────┐          ┌─────────────┐
       │    Audio    │          │     UI      │          │ Achievement │
       │   System    │          │   System    │          │   System    │
       └─────────────┘          └─────────────┘          └─────────────┘
```

### Event Lifetime

Events exist for exactly **one frame**:

1. `events.clear()` - Called by engine at frame start
2. Systems emit events during update
3. Systems read events during update (same frame)
4. Events are gone next frame

### Type Safety

Events are strongly typed via the `Event` trait (blanket impl for `'static + Send + Sync`):

```rust
// Any struct can be an event - no registration needed
struct PlayerDied { entity: Entity }
struct ScoreChanged { delta: i32 }

// Emit and read are type-safe
events.emit(PlayerDied { entity });
for event in events.read::<PlayerDied>() { ... }
```

## API Reference

### Events Struct

```rust
impl Events {
    // Publish an event (available this frame)
    fn emit<E: Event>(&mut self, event: E);

    // Read all events of type E (iterator)
    fn read<E: Event>(&self) -> impl Iterator<Item = &E>;

    // Check if any events of type E exist
    fn has<E: Event>(&self) -> bool;

    // Count events of type E
    fn count<E: Event>(&self) -> usize;

    // Clear all events (called by engine)
    fn clear(&mut self);
}
```

### Built-in Events

#### Automatically Emitted (by engine)

| Event | Emitted When |
|-------|--------------|
| `CollisionStarted` | Two physics entities begin colliding |
| `CollisionStopped` | Two physics entities stop colliding |

#### Game-Emitted (by user code)

| Event | Suggested Use |
|-------|---------------|
| `ClientConnected` | When `ListenServer` reports new client |
| `ClientDisconnected` | When `ListenServer` reports client left |
| `ConnectedToServer` | When `NetworkClient` connects |
| `DisconnectedFromServer` | When `NetworkClient` disconnects |
| `AssetReloaded` | When responding to hot reload |
| `AssetLoadFailed` | When asset loading fails |

## Usage Patterns

### Reading Collision Events (New Way)

```rust
// Recommended: Use event bus
for event in ctx.events.read::<CollisionStarted>() {
    if event.involves(player) {
        let other = event.other(player).unwrap();
        handle_collision(other);
    }
}
```

### Custom Game Events

```rust
// Define events as simple structs
struct EnemyDied { entity: Entity, score: u32 }
struct WaveCompleted { wave: u32 }
struct PowerUpCollected { kind: PowerUpKind }

// Emit when things happen
fn update(&mut self, ctx: &mut GameContext) {
    if enemy_health <= 0 {
        ctx.events.emit(EnemyDied {
            entity: enemy,
            score: 100
        });
    }
}

// React from anywhere
fn update_score(&mut self, ctx: &mut GameContext) {
    for event in ctx.events.read::<EnemyDied>() {
        self.score += event.score;
    }
}

fn update_audio(&mut self, ctx: &mut GameContext) {
    for _ in ctx.events.read::<EnemyDied>() {
        play_sound("enemy_death.wav");
    }
}
```

### Checking for Events Without Iterating

```rust
// Just check existence
if ctx.events.has::<PlayerDied>() {
    transition_to_game_over();
}

// Count events
let kills_this_frame = ctx.events.count::<EnemyDied>();
```

## Internal Implementation

### Type-Erased Storage

Events use `TypeId` to key a `HashMap` of type-erased channels:

```rust
struct Events {
    channels: HashMap<TypeId, Box<dyn AnyEventChannel>>,
}

// Each channel stores a Vec<E> for its event type
struct EventChannel<E> {
    events: Vec<E>,
}
```

### Performance Characteristics

- **emit()**: O(1) amortized (Vec push)
- **read()**: O(1) lookup + O(n) iteration
- **has()/count()**: O(1) lookup
- **clear()**: O(channels) but Vec capacity is retained

After warmup (all event types used once), no allocations occur during gameplay.

## Integration Points

The `Events` struct is:
1. Owned by `Engine`
2. Passed to `GameContext` and `SceneContext` as `&mut Events`
3. Cleared at the start of each `update()` call
4. Available during `init()`, `update()`, and `render_ui()`

## Notes for AI Agents

1. **Events are per-frame** - Don't expect events to persist across frames
2. **Multiple reads OK** - Reading doesn't consume events
3. **Order matters for emit** - Events are read in emit order
4. **No registration** - Just define a struct and use it
5. **Prefer events over callbacks** - Events decouple systems better
6. **Keep events small** - Store entity IDs, not full component data
