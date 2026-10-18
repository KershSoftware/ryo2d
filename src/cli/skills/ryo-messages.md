---
name: ryo-messages
description: Messages - typed, buffered notifications between scenes and systems; the engine's message types and how to define, write and read your own.
metadata:
  version: 2.2.0
---

# Ryo2D Messages

You help users tell one part of a game that something happened (a hit, a pickup, a finished tween) without coupling the two.

**For complete API signatures, see the `ryo-api` skill.**

## The model

- A message type derives `Message`. Each registered type has a buffer, the `Messages<T>` resource.
- Writers append to it. Every reader has its own cursor and sees each message once: a system through `MessageReader<T>`, a scene through `ctx.messages::<T>()`.
- Buffers advance in `First`, but only once a fixed step has run since the last advance: a message stays readable until two fixed steps have run after it was written, so every reader, per-frame or fixed-step, sees it at any frame rate. While no fixed step runs (the game clock paused) nothing is dropped.

Use a message for "this happened", a resource for shared state, a component for per-entity state.

## Defining a message

```rust
#[derive(Message, Clone, Debug)]
struct EnemyDefeated {
  enemy: Entity,
  points: u32,
}
```

`Clone` is required to read it in a scene (`ctx.messages` clones each message out). Carry `Entity` ids and plain values, never references.

Register it before anything uses it:

```rust
EngineBuilder::new()
  .add_message::<EnemyDefeated>()
  // ...
```

A scene's first `ctx.write_message` also registers the type, so a type only scenes use needs no `add_message`. A system that reads or writes an unregistered type cannot run, so register any type a system touches. Reading an unregistered type in a scene returns an empty `Vec`.

## In scenes

```rust
fn update(&mut self, ctx: &mut SceneContext, _dt: f32) -> SceneAction {
  // Everything written since this scene last read
  for message in ctx.messages::<EnemyDefeated>() {
    self.combo += 1;
    if Some(message.enemy) == self.boss {
      return SceneAction::Switch(Box::new(Victory::new()));
    }
  }

  if ctx.input.key_pressed(KeyCode::KeyK)
    && let Some(boss) = self.boss
  {
    ctx.write_message(EnemyDefeated { enemy: boss, points: 1000 });
  }
  SceneAction::Continue
}
```

- The cursor belongs to the scene instance and is shared by all its callbacks: what `ui` reads, `update` will not see again.
- Each scene on the stack has its own cursor, so an overlay and the scene below it both see every message.
- A scene's cursor starts when it enters: messages written before a `Push`, `Switch` or `Reset` (say, by the scene that pushed it) are not read by the new scene. Hand such data over in the scene's constructor.
- A scene's own systems read through `MessageReader`, whose cursor starts at the oldest buffered message the first time the system runs; they can see messages written just before the scene entered.

## In systems

```rust
fn defeat_enemies(
  mut commands: Commands,
  enemies: Query<(Entity, &Enemy)>,
  mut defeated: MessageWriter<EnemyDefeated>,
) {
  for (entity, enemy) in &enemies {
    if enemy.health <= 0 {
      defeated.write(EnemyDefeated { enemy: entity, points: enemy.points });
      commands.entity(entity).despawn();
    }
  }
}

fn award_points(mut defeated: MessageReader<EnemyDefeated>, mut score: ResMut<Score>) {
  for message in defeated.read() {
    score.0 += message.points;
  }
}

// In on_enter: run with this scene, in order
ctx
  .add_systems(Phase::FixedUpdate, (defeat_enemies, award_points).chain())
  .expect("FixedUpdate takes scene systems");
```

A reader ordered after the writer in the same schedule sees the message the same step; otherwise it sees it on its next run.

## When messages arrive

| Writer | Runs | Readers see it |
|--------|------|----------------|
| Physics (`CollisionStarted`, `CollisionStopped`, `TriggerEntered`, `TriggerExited`, `ContactForce`, `JointBroken`) | each fixed step, in `FixedSet::Physics` | the next step's `update` |
| Gameplay (`MovementEvent`, `PickedUp`, `Killed`, `CheckpointReached`, `KnockbackStarted`, `KnockbackFinished`) | each fixed step, in `FixedSet::Movement`: after `Scene::update` and the scene's fixed systems, before physics | the next step's `update` |
| Juice systems (tweens, squash and stretch) | each fixed step, before `Scene::update` | the same step's `update` |
| Hot reload (`AssetReloaded`, `AssetLoadFailed`) | `PreUpdate`, once a frame | this frame's `ui` and steps |
| `ParticleSystem::update` (`ParticleCollision`) | where the scene calls it | after that call |

The trigger behaviours (`Pickup`, `KillZone`, `Checkpoint`) read the previous step's `TriggerEntered` in `FixedSet::Movement`, so `PickedUp`, `Killed` and `CheckpointReached` come one step after the overlap, and `update` reads them one step after that. While physics is paused (`PhysicsPaused`) no collision or trigger messages are written; the behaviours keep running and still act on an entry that arrived just before the pause.

No reader misses a message, whether it runs every frame (`ui`, systems in `Update` or `PostUpdate`) or in fixed steps (`Scene::update`, `FixedUpdate` systems), at any frame rate: buffers only advance after a fixed step. A game that writes a message every frame while the game clock is paused accumulates them until it resumes.

## Scene changes

`SceneCleanup::MESSAGES` (in `SceneCleanup::DEFAULT`) empties every buffer on `Switch` and `Reset`, so the incoming scene does not react to the old one's messages. `Push` and `Pop` keep them. To carry messages across a switch:

```rust
fn cleanup_on_exit(&self) -> SceneCleanup {
  SceneCleanup::DEFAULT - SceneCleanup::MESSAGES
}
```

## Engine messages

All are in the prelude and registered at engine build.

| Message | Fields | Written when |
|---------|--------|--------------|
| `CollisionStarted` | `a`, `b` (colliders), `body_a`, `body_b`, `contact: Option<Contact { point, normal }>`; `involves(e)`, `other(e)` (the other side's body), `other_collider(e)` | two colliders start touching (solid contact only; triggers send the next pair) |
| `CollisionStopped` | same without `contact` | they stop touching, or one went away (either may be despawned) |
| `TriggerEntered` | `trigger` (the trigger collider), `other` (the collider that entered), `other_body` (its body, or itself); `actor()` = `other_body` | a body enters a trigger: once per body, on its first collider in (its trigger colliders, an aggro radius, count apart from its solid ones) |
| `TriggerExited` | same | the body's last collider leaves, or one of them went away (either may be despawned) |
| `ContactForce` | `a`, `b`, `body_a`, `body_b`, `force`, `direction`, `started` | a contact pushes above its collider's `with_force_events` threshold, each step |
| `JointBroken` | `joint`, `a`, `b` | a joint breaks |
| `TweenStarted` | `entity` | a `Tween`, `TweenSequence` or engine-driven `PropertyTween` starts |
| `TweenCompleted` | `entity`, `finished` | an iteration ends; `finished` is `true` when the tween is done (`Once`, or the last of `Times(n)`) |
| `KnockbackStarted` / `KnockbackFinished` | `entity` | a knockback starts / ends, once each: decayed, `stop()` (a kill zone's respawn included) or pushed straight into a surface (written in `FixedSet::Movement`) |
| `MovementEvent` | `entity`, `kind: MovementEventKind` (`Jumped`, `AirJumped`, `WallJumped`, `Dashed`, `Landed`; non-exhaustive) | a movement preset's character jumped, dashed or landed (see `ryo-character`) |
| `PickedUp` | `pickup`, `by` (the body that collected it) | a `Pickup` was collected |
| `Killed` | `entity`, `zone` | something entered a `KillZone` |
| `CheckpointReached` | `checkpoint`, `entity` | a `Checkpoint` moved `entity`'s `Respawn` point |
| `SquashStretchStarted` / `SquashStretchFinished` | `entity` | a squash starts / recovers |
| `AssetReloaded` | `path: PathBuf` | hot reload replaced an asset (dev builds) |
| `AssetLoadFailed` | `path`, `error: String` | a hot reload failed (dev builds); an async load reports failure on its `AsyncSpriteHandle` (`state()`) instead |
| `ParticleCollision` | `emitter_entity`, `collider_entity`, `position_x/y`, `normal_x/y`, `velocity_x/y` | a particle hits a collider (see `ryo-particles`) |
| `UiEvent` | `node`, `name`, `action`, `kind: UiEventKind` (`Clicked`, `SecondaryClicked`, `Changed`, `Submitted`, `Dropped { source }`, `Back`, `FocusGained`, `FocusLost`) | a UI node reported something, during the UI pass, before the fixed steps (see `ryo-ui`, Nodes). A scene reads its own nodes' with `ctx.ui_events()`, which shares this message's cursor |
| `UiReloaded` | `root: Entity` | a UI node tree was brought in line with its changed `.ui.ron` file (dev builds, or `ctx.assets.reload_ui`), once per root, before that frame's UI pass. Nodes the file gave a new kind are new entities, so a game that filled a placeholder fills it again here (see `ryo-ui`, Files) |

```rust
// A coin is a trigger: who entered is `actor()`, the coin is `trigger`
for entered in ctx.messages::<TriggerEntered>() {
  if entered.actor() == self.player && ctx.world.get::<Coin>(entered.trigger).is_some() {
    ctx.world.despawn(entered.trigger);
  }
}
// Solid contact: the other side's body
for hit in ctx.messages::<CollisionStarted>() {
  if let Some(other) = hit.other(self.player)
    && ctx.world.get::<Spikes>(other).is_some()
  {
    self.health -= 1;
  }
}
```

The entities in a message may have been despawned since it was written; `ctx.world.get::<T>(entity)` returns `None` for them.

Modules define no message they do not write. Navigation, procgen, inventory and networking report through return values (`PathResult`, generation results, `NetworkEvent`, ...); a game that wants a notification for them defines its own message type and writes it.

## Observers

The ECS's immediate events are available too: a `#[derive(Event)]` type, `ctx.world.add_observer(|event: On<Explode>| ..)` and `ctx.world.trigger(Explode { .. })` run the observer at once. The engine triggers none of its own. Observers are entities that `SceneCleanup::WORLD` keeps, so a scene that adds one despawns it in `on_exit`:

```rust
fn on_enter(&mut self, ctx: &mut SceneContext) {
  let observer = ctx
    .world
    .add_observer(|explode: On<Explode>| game_info!("boom, radius {}", explode.radius))
    .id();
  self.observer = Some(observer);
}

fn on_exit(&mut self, ctx: &mut SceneContext) {
  if let Some(observer) = self.observer.take() {
    ctx.world.despawn(observer);
  }
}
```

Prefer messages for gameplay: they are buffered, ordered with the frame, and readable by scenes.
