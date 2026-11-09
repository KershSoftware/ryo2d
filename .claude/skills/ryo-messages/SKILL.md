---
name: ryo-messages
description: Messages - typed, buffered notifications between scenes and systems; the engine's message types and how to define, write and read your own.
metadata:
  version: 2.9.0
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

<!-- with: #[derive(Message, Clone, Debug)] struct EnemyDefeated { enemy: Entity, points: u32 } -->
```rust
EngineBuilder::new()
  .add_message::<EnemyDefeated>()
  // ...
```

A scene's first `ctx.write_message` also registers the type, so a type only scenes use needs no `add_message`. A system that reads or writes an unregistered type cannot run, so register any type a system touches. Reading an unregistered type in a scene returns an empty `Vec`.

## In scenes

<!-- with: #[derive(Message, Clone, Debug)] struct EnemyDefeated { enemy: Entity, points: u32 } struct Victory; impl Victory { fn new() -> Self { Self } } impl Scene for Victory {} -->
<!-- self: combo: u32, boss: Option<Entity> -->
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

<!-- with: #[derive(Message, Clone, Debug)] struct EnemyDefeated { enemy: Entity, points: u32 } #[derive(Component)] struct Enemy { health: i32, points: u32 } #[derive(Resource)] struct Score(u32); -->
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
| Tile maps' stairs (`FloorChanged`: a climber changed storey) | each fixed step, in `FixedSet::Movement`, before physics | the next step's `update` |
| Juice systems (tweens, squash and stretch) | each fixed step, before `Scene::update` | the same step's `update` |
| The asset pump (`AssetEvent<T>` for each asset type: loads, reloads, frees and failures); `load_now` writes its own `Added` or `Failed` at once | `PreUpdate`, once a frame | this frame's `ui` and steps |
| Tile maps (`MapEvent`: a map landed, followed a saved file, or failed; `ChunkEvent`: a streamed map's chunk landed or unloaded) | `PreUpdate`, after the asset pump | this frame's `ui` and steps |
| The UI pass (`UiReloaded`, then `UiListBind`, before any tree is drawn; `UiEvent` as each scene's node trees are walked, or after `ui` for embedded trees) | once a frame, before the fixed steps | this frame's `ui` and steps |
| The saves (`SaveFinished` for each save and delete finished since the last frame; `SettingsSaveFailed` for a failed `system.json` write) | `PreUpdate`, once a frame | this frame's `ui` and steps |
| The particle step (`ParticleCollision`, `EmitterFinished`) | each fixed step, in `FixedSet::Particles`: after `Scene::update`, physics and propagation; an `unscaled` emitter's once a frame, in `PostUpdateSet::Extract` | the next step's `update` |

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

All are in the prelude and registered at engine build; `AssetEvent<T>` is registered with its asset type (the engine's at build, a game's with `EngineBuilder::asset_loader` or its first `ctx.assets().add`).

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
| `AssetEvent<T>` (one type per asset type: `AssetEvent<Image>`, `AssetEvent<Font>`, ...) | `Added { id }`, `Modified { id }`, `Removed { id }`, `Failed { id, error }`, `ReloadFailed { id, error }` (`id: AssetId<T>`, `error: Arc<AssetError>`) | an asset landed in its store (or was added in code), changed (reloaded, a dependency reloaded, or `get_mut`), was freed (its last handle dropped, if it had landed), did not load (it loads again when its file changes), or failed to reload and kept its last good value. An asset's events come in the order its changes happened (see `ryo-assets`) |
| `ParticleCollision` | `emitter_entity`, `collider_entity`, `position_x/y`, `normal_x/y`, `velocity_x/y` | a particle hits a collider, in the particle step (see `ryo-particles`) |
| `EmitterFinished` | `entity` | an emitter is not playing (stopped, or its one cycle over) and its last particle died, once per finish, in the step it finishes; `despawn_when_finished` despawns the entity in the next step, and an unscaled emitter can do both within one frame (see `ryo-particles`) |
| `MapEvent` | `Spawned { map }`, `Reloaded { map }`, `Failed { map, error: Arc<AssetError> }` | a tile map landed whole (its layers, objects, colliders and shadows arrived this frame), followed a saved file, or did not load or land (once per failure; see `ryo-tilemap`) |
| `FloorChanged` | `entity`, `from: Floor`, `to: Floor` | a `Climber` changed storey halfway along a tile map's stairs (onto the upper storey halfway up, back down halfway down), once per change; that step's physics already has it on `to` (see `ryo-tilemap`, Stairs) |
| `ChunkEvent` | `Loaded { map, chunk: IVec2 }`, `Unloading { map, chunk, changed: Option<ChunkTiles> }` | a streamed map's chunk landed this frame, its class tiles in, or unloaded this frame: by the time it is read the chunk's tiles, class tiles and `ChunkOwned` entities are gone (its colliders and shadows within the frame), and `changed` holds its tiles when the player changed it since it landed, else `None`; in a frame that does both, a map's `Loaded` come before its `Unloading` (see `ryo-tilemap`, Streaming an endless world) |
| `UiEvent` | `node`, `name`, `action`, `kind: UiEventKind` (`Clicked`, `SecondaryClicked`, `Changed`, `Submitted`, `Dropped { source }`, `Back`, `FocusGained`, `FocusLost`) | a UI node reported something, during the UI pass, before the fixed steps (see `ryo-ui`, Nodes). A scene reads its own nodes' with `ctx.ui_events()`, which shares this message's cursor |
| `UiListBind` | `list` (the list node), `row` (the pooled row), `index` (the item) | a UI list node gave one of its pooled rows an item (a new row, a row reused for another item, or `ctx.ui_list_refresh`), at the start of the UI pass, before any tree is drawn. The game fills the row (`ctx.ui_set(row, ..)`) in `ui`, and the row is drawn from the next frame. Every list's binds reach every scene: keep to your list's `list`. Also triggered on the row as an entity event (see `ryo-ui`, Virtual Lists) |
| `SaveFinished` | `slot: String`, `op: SaveOp` (`Save`, `Delete`), `result: Result<(), Arc<SaveError>>` | a save or delete finished, once per call, in a frame after it: `Ok`, or why it failed; saves of one slot that follow each other faster than they are written write only the last, and the ones before it report `Ok` (see `ryo-config-save`) |
| `SettingsSaveFailed` | `error: Arc<SaveError>` | the player's settings file, `system.json`, could not be written (a full disk, a read-only folder), once per failed write; the settings still apply, and the engine tries again at the next change and when the game quits (see `ryo-config-save`) |
| `UiReloaded` | `root: Entity`, `replaced: Option<Entity>` | a UI node tree was brought in line with its changed `.ui.ron` file (saved while hot reload is on, debug builds with a window, or reloaded with `ctx.assets().server().reload(path)`), once per root, before that frame's UI pass. `replaced` is the old root when the file gave the root a new kind, so a game holding it swaps in `root`. Nodes the file gave a new kind are new entities, so a game that filled a placeholder fills it again here (see `ryo-ui`, Files) |

<!-- with: #[derive(Component)] struct Coin; #[derive(Component)] struct Spikes; -->
<!-- self: player: Entity, health: i32 -->
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

The ECS's immediate events are available too: a `#[derive(Event)]` type, `ctx.world.add_observer(|event: On<Explode>| ..)` and `ctx.world.trigger(Explode { .. })` run the observer at once. The engine triggers only two of its own, on UI nodes: `UiEvent` on its node and `UiListBind` on its row, during the UI pass (an observer there must not change scenes; see `ryo-ui`). Observers are entities that `SceneCleanup::WORLD` keeps, so a scene that adds one despawns it in `on_exit`:

<!-- with: #[derive(Event)] struct Explode { radius: f32 } -->
<!-- self: observer: Option<Entity> -->
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

A component's lifecycle can be observed the same way: `On<Add<T>>` runs as an entity gains a `T`, `On<Insert<T>>` each time one is put on (added or put over), and `On<Remove<T>>` as it loses it, despawned too (`add.entity` is the entity). The tile maps teach this for objects arriving (`ryo-tilemap`, Objects and class tiles).

Prefer messages for gameplay: they are buffered, ordered with the frame, and readable by scenes.
