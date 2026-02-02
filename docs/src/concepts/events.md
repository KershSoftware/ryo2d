# Event Bus

The **event bus** is a decoupled publish-subscribe system that lets different parts of your game communicate without direct dependencies.

## Why Use Events?

Consider a game where enemies can die. When an enemy dies, several things need to happen:

- Play a death sound
- Update the score
- Spawn particle effects
- Check for achievements
- Update the UI

**Without events**, your combat code would need to know about all these systems:

```rust
// Tightly coupled - combat knows about everything
fn kill_enemy(&mut self, enemy: Entity, ctx: &mut GameContext) {
    ctx.world.despawn(enemy);
    self.audio.play("death.wav");           // Knows about audio
    self.score += 100;                       // Knows about scoring
    self.particles.spawn_explosion(pos);     // Knows about particles
    self.achievements.check_kill_count();    // Knows about achievements
    self.ui.update_score_display();          // Knows about UI
}
```

**With events**, systems are decoupled:

```rust
// Combat only cares about combat
fn kill_enemy(&mut self, enemy: Entity, ctx: &mut GameContext) {
    let pos = ctx.world.get_component::<Transform>(enemy).map(|t| (t.x, t.y));
    ctx.world.despawn(enemy);
    ctx.events.emit(EnemyDied { entity: enemy, position: pos, score: 100 });
}

// Audio reacts independently
fn update_audio(&mut self, ctx: &mut GameContext) {
    for _ in ctx.events.read::<EnemyDied>() {
        self.play("death.wav");
    }
}

// Scoring reacts independently
fn update_score(&mut self, ctx: &mut GameContext) {
    for event in ctx.events.read::<EnemyDied>() {
        self.score += event.score;
    }
}
```

## Basic Usage

### Defining Events

Events are simple Rust structs. No registration or setup required:

```rust
struct EnemyDied {
    entity: Entity,
    score: u32,
}

struct PlayerDied {
    entity: Entity,
    cause: String,
}

struct LevelCompleted {
    level: u32,
    time_seconds: f32,
}
```

### Emitting Events

Use `ctx.events.emit()` to publish an event:

```rust
fn update(&mut self, ctx: &mut GameContext, dt: f32) {
    if player_health <= 0 {
        ctx.events.emit(PlayerDied {
            entity: self.player,
            cause: "lava".into(),
        });
    }
}
```

### Reading Events

Use `ctx.events.read::<EventType>()` to get an iterator over all events of that type:

```rust
fn update(&mut self, ctx: &mut GameContext, dt: f32) {
    for event in ctx.events.read::<PlayerDied>() {
        println!("Player died from: {}", event.cause);
        self.show_game_over_screen();
    }
}
```

Multiple systems can read the same events - reading doesn't consume them.

### Checking for Events

For simple existence checks, use `has()` or `count()`:

```rust
// Check if any event exists
if ctx.events.has::<PlayerDied>() {
    self.pause_game();
}

// Count events
let kills_this_frame = ctx.events.count::<EnemyDied>();
if kills_this_frame >= 5 {
    self.show_combo_bonus();
}
```

## Event Lifetime

Events exist for exactly **one frame**:

1. Engine clears all events at the start of each frame
2. Your code emits events during update
3. Your code reads events during update (same frame)
4. Next frame starts, events are gone

This means you can't "miss" events by reading early - all events emitted during a frame are available until the frame ends.

## Built-in Events

Ryo2D automatically emits some events for you:

### Collision Events

The physics system emits `CollisionStarted` and `CollisionStopped`:

```rust
use ryo2d::prelude::*;

fn update(&mut self, ctx: &mut GameContext, dt: f32) {
    // React when collisions begin
    for event in ctx.events.read::<CollisionStarted>() {
        if event.involves(self.player) {
            let other = event.other(self.player).unwrap();
            self.handle_player_collision(other);
        }
    }

    // React when collisions end
    for event in ctx.events.read::<CollisionStopped>() {
        if event.involves(self.player) {
            self.player_left_trigger();
        }
    }
}
```

The `CollisionStarted` and `CollisionStopped` events have helper methods:

```rust
event.entity_a           // First entity
event.entity_b           // Second entity
event.involves(entity)   // Check if entity is part of this collision
event.other(entity)      // Get the other entity (returns Option)
```

> **Migration Note**: The old `ctx.collisions_started()` and `ctx.collision_events` APIs are deprecated. Use `ctx.events.read::<CollisionStarted>()` instead.

## Common Patterns

### Multiple Reactions to One Event

The same event can trigger multiple independent reactions:

```rust
struct ItemCollected { entity: Entity, item_type: ItemType }

// Inventory system
for event in ctx.events.read::<ItemCollected>() {
    self.inventory.add(event.item_type);
}

// Audio system
for event in ctx.events.read::<ItemCollected>() {
    self.play_sound(match event.item_type {
        ItemType::Coin => "coin.wav",
        ItemType::Health => "health.wav",
        _ => "pickup.wav",
    });
}

// UI system
for event in ctx.events.read::<ItemCollected>() {
    self.show_pickup_popup(event.item_type);
}
```

### Event Chains

Events can trigger other events:

```rust
// Collision system
for event in ctx.events.read::<CollisionStarted>() {
    if is_bullet(event.entity_a) && is_enemy(event.entity_b) {
        ctx.events.emit(EnemyHit {
            enemy: event.entity_b,
            damage: 25,
        });
    }
}

// Health system
for event in ctx.events.read::<EnemyHit>() {
    let health = get_health(event.enemy);
    if health - event.damage <= 0 {
        ctx.events.emit(EnemyDied { entity: event.enemy });
    }
}
```

### Achievement System

Events make achievement tracking clean:

```rust
fn check_achievements(&mut self, ctx: &mut GameContext) {
    // Track kills
    for _ in ctx.events.read::<EnemyDied>() {
        self.total_kills += 1;
    }

    // Check thresholds
    if self.total_kills >= 100 && !self.unlocked("century") {
        self.unlock("century");
        ctx.events.emit(AchievementUnlocked { id: "century" });
    }
}
```

## Performance Considerations

- **Events are cheap** - Just Vec pushes/iterations
- **No allocations after warmup** - Vecs reuse capacity
- **Keep events small** - Store entity IDs, not large data
- **Use `has()` for checks** - Faster than iterating

## When to Use Events vs Direct Calls

| Use Events | Use Direct Calls |
|------------|------------------|
| Multiple systems need to react | Single, known consumer |
| Systems shouldn't know about each other | Tight coupling is acceptable |
| Debugging event flow is valuable | Simple, obvious code flow |
| Timing within frame doesn't matter | Immediate response required |

## Summary

The event bus provides:

- **Decoupling** - Systems don't need to know about each other
- **Flexibility** - Easy to add new reactions without changing emitters
- **Debuggability** - Clear event flow through your game
- **Type safety** - Compile-time checking of event types

Access it through `ctx.events` in your game code.
