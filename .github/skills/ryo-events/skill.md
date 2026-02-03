---
name: ryo-events
description: Event bus system for decoupled inter-system communication.
metadata:
  version: 1.0.0
---

# Ryo2D Event Bus Specialist

You help users implement decoupled communication between game systems using the event bus pattern.

**For complete API signatures, see the `ryo-api` skill.**

## Core Concept

The event bus allows systems to communicate without direct dependencies:

- **emit()** - Publish an event (any system)
- **read()** - Subscribe to events (any system, same frame)
- Events live for one frame, then cleared automatically

## Basic Usage

### Defining Events

```rust
// Events are simple structs - no registration needed
struct PlayerDied { entity: Entity }
struct ScoreChanged { delta: i32, new_total: u32 }
struct EnemySpawned { entity: Entity, wave: u32 }
```

### Emitting Events

```rust
fn update(&mut self, ctx: &mut GameContext, dt: f32) {
    if player_health <= 0 {
        ctx.events.emit(PlayerDied { entity: self.player });
    }

    self.score += 100;
    ctx.events.emit(ScoreChanged {
        delta: 100,
        new_total: self.score
    });
}
```

### Reading Events

```rust
// Read all events of a type (returns iterator)
for event in ctx.events.read::<PlayerDied>() {
    self.play_death_animation(event.entity);
}

// Check if any events exist (no iteration)
if ctx.events.has::<PlayerDied>() {
    self.show_game_over();
}

// Count events
let kills = ctx.events.count::<EnemyDied>();
```

## Built-in Events

### Collision Events (Automatic)

The engine automatically emits collision events from physics:

```rust
use ryo2d::prelude::*;

// React to collisions starting
for event in ctx.events.read::<CollisionStarted>() {
    if event.involves(self.player) {
        let other = event.other(self.player).unwrap();
        self.handle_player_hit(other, ctx);
    }
}

// React to collisions ending
for event in ctx.events.read::<CollisionStopped>() {
    if event.involves(self.player) {
        self.player_left_trigger();
    }
}
```

### CollisionStarted / CollisionStopped API

```rust
event.entity_a           // First entity in collision
event.entity_b           // Second entity in collision
event.involves(entity)   // Check if entity is in this collision
event.other(entity)      // Get the other entity (returns Option<Entity>)
```

### Knockback Events (Automatic)

The engine emits events when knockback state changes:

```rust
// React to knockback starting (e.g., flash the entity)
for event in ctx.events.read::<KnockbackStarted>() {
    if let Some(mut renderable) = ctx.world.get_component_mut::<Renderable>(event.entity) {
        renderable.tint = Color::WHITE;  // Flash white on hit
    }
}

// React to knockback finishing
for event in ctx.events.read::<KnockbackFinished>() {
    // Entity has stopped being pushed
    self.on_knockback_ended(event.entity);
}
```

### Network Events (Game-Emitted)

When using networking, emit these from your game code:

```rust
// In your server/host code when handling ListenServer events
match server.poll() {
    NetworkEvent::ClientConnected(id) => {
        ctx.events.emit(ClientConnected { client_id: id });
    }
    NetworkEvent::ClientDisconnected(id) => {
        ctx.events.emit(ClientDisconnected { client_id: id });
    }
    _ => {}
}
```

## Common Patterns

### Multiple Systems Reacting to Same Event

```rust
// Combat system emits
ctx.events.emit(EnemyDied { entity: enemy, score: 100 });

// Audio system reads
fn update_audio(&mut self, ctx: &mut GameContext) {
    for _ in ctx.events.read::<EnemyDied>() {
        self.play_sound("enemy_death.wav");
    }
}

// Score system reads (same event)
fn update_score(&mut self, ctx: &mut GameContext) {
    for event in ctx.events.read::<EnemyDied>() {
        self.score += event.score;
    }
}

// Particle system reads (same event)
fn update_particles(&mut self, ctx: &mut GameContext) {
    for event in ctx.events.read::<EnemyDied>() {
        self.spawn_explosion(event.entity);
    }
}
```

### Chained Events

Events can trigger other events:

```rust
fn update_powerups(&mut self, ctx: &mut GameContext) {
    for event in ctx.events.read::<CollisionStarted>() {
        if event.involves(self.player) {
            let other = event.other(self.player).unwrap();
            if self.is_powerup(other) {
                // Emit new event for other systems
                ctx.events.emit(PowerUpCollected {
                    kind: self.get_powerup_kind(other)
                });
                ctx.world.despawn(other);
            }
        }
    }
}
```

### Achievement System

```rust
struct AchievementUnlocked { id: &'static str }

fn check_achievements(&mut self, ctx: &mut GameContext) {
    let kills = ctx.events.count::<EnemyDied>();
    if kills >= 10 && !self.has_achievement("mass_destruction") {
        self.unlock("mass_destruction");
        ctx.events.emit(AchievementUnlocked { id: "mass_destruction" });
    }
}
```

## Migration from Old Collision API

```rust
// OLD (deprecated)
for event in ctx.collisions_started() {
    if event.involves(player) { ... }
}

// NEW (recommended)
for event in ctx.events.read::<CollisionStarted>() {
    if event.involves(player) { ... }
}
```

## Performance Tips

1. **Events are per-frame** - Don't store references to events
2. **Reading is cheap** - Multiple reads of same type OK
3. **Keep events small** - Store Entity IDs, not full data
4. **Use has() for checks** - Faster than `read().next().is_some()`

## Event vs Direct Calls

| Use Events When | Use Direct Calls When |
|-----------------|----------------------|
| Multiple systems need to react | Only one consumer |
| Systems shouldn't know each other | Tight coupling is OK |
| Timing isn't critical | Immediate response needed |
| Debugging event flow is useful | Simple, obvious flow |
