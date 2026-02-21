# Scene System Redesign Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add typed resources for cross-scene shared state, pop-with-data for returning values on scene pop, and remove the Game trait so the engine runs scenes natively.

**Architecture:** Engine owns a `Resources` type map alongside its existing systems. `SceneContext` gains a `resources` field. `SceneManager::pop` supports returning `Box<dyn Any>` to the resumed scene. The `Game` trait, `GameContext`, and `SceneGame` adapter are removed; their logic moves into `Engine` directly.

**Tech Stack:** Rust std (`HashMap<TypeId, Box<dyn Any>>`), no new dependencies.

**Design doc:** `docs/plans/2026-02-20-scene-system-redesign.md`

---

### Task 1: Create the Resources struct

**Files:**
- Create: `src/core/resources.rs`
- Modify: `src/core/mod.rs:52-54` (add `pub mod resources;`)
- Modify: `src/lib.rs:59-62` (add `Resources` to prelude)

**Step 1: Write the failing tests**

Create `src/core/resources.rs` with the tests first:

```rust
use std::any::{Any, TypeId};
use std::collections::HashMap;

pub struct Resources {
    map: HashMap<TypeId, Box<dyn Any>>,
}

impl Resources {
    pub fn new() -> Self {
        Self { map: HashMap::new() }
    }
}

impl Default for Resources {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Score(u32);
    struct Health(f32);

    #[test]
    fn insert_and_get() {
        let mut res = Resources::new();
        res.insert(Score(100));
        assert_eq!(res.get::<Score>().unwrap().0, 100);
    }

    #[test]
    fn get_returns_none_when_missing() {
        let res = Resources::new();
        assert!(res.get::<Score>().is_none());
    }

    #[test]
    fn get_mut_modifies_in_place() {
        let mut res = Resources::new();
        res.insert(Score(0));
        res.get_mut::<Score>().unwrap().0 = 42;
        assert_eq!(res.get::<Score>().unwrap().0, 42);
    }

    #[test]
    fn insert_overwrites_existing() {
        let mut res = Resources::new();
        res.insert(Score(10));
        res.insert(Score(20));
        assert_eq!(res.get::<Score>().unwrap().0, 20);
    }

    #[test]
    fn remove_returns_value() {
        let mut res = Resources::new();
        res.insert(Score(99));
        let removed = res.remove::<Score>().unwrap();
        assert_eq!(removed.0, 99);
        assert!(res.get::<Score>().is_none());
    }

    #[test]
    fn remove_returns_none_when_missing() {
        let mut res = Resources::new();
        assert!(res.remove::<Score>().is_none());
    }

    #[test]
    fn contains_checks_presence() {
        let mut res = Resources::new();
        assert!(!res.contains::<Score>());
        res.insert(Score(1));
        assert!(res.contains::<Score>());
    }

    #[test]
    fn get_or_insert_with_creates_if_missing() {
        let mut res = Resources::new();
        let score = res.get_or_insert_with(|| Score(42));
        assert_eq!(score.0, 42);
    }

    #[test]
    fn get_or_insert_with_returns_existing() {
        let mut res = Resources::new();
        res.insert(Score(10));
        let score = res.get_or_insert_with(|| Score(42));
        assert_eq!(score.0, 10); // closure NOT called
    }

    #[test]
    fn multiple_types_coexist() {
        let mut res = Resources::new();
        res.insert(Score(50));
        res.insert(Health(100.0));
        assert_eq!(res.get::<Score>().unwrap().0, 50);
        assert_eq!(res.get::<Health>().unwrap().0, 100.0);
    }

    #[test]
    fn default_creates_empty() {
        let res = Resources::default();
        assert!(!res.contains::<Score>());
    }
}
```

**Step 2: Run tests to verify they fail**

Run: `cargo test --lib resources::tests -p ryo2d`
Expected: Compilation errors — methods `insert`, `get`, `get_mut`, etc. don't exist yet.

**Step 3: Implement all methods**

Add to `src/core/resources.rs` (above the `#[cfg(test)]` block):

```rust
impl Resources {
    // new() and Default already exist from Step 1

    pub fn insert<T: 'static>(&mut self, value: T) {
        self.map.insert(TypeId::of::<T>(), Box::new(value));
    }

    pub fn get<T: 'static>(&self) -> Option<&T> {
        self.map.get(&TypeId::of::<T>())?.downcast_ref()
    }

    pub fn get_mut<T: 'static>(&mut self) -> Option<&mut T> {
        self.map.get_mut(&TypeId::of::<T>())?.downcast_mut()
    }

    pub fn get_or_insert_with<T: 'static>(&mut self, f: impl FnOnce() -> T) -> &mut T {
        self.map
            .entry(TypeId::of::<T>())
            .or_insert_with(|| Box::new(f()))
            .downcast_mut()
            .expect("type mismatch in Resources - this is a bug")
    }

    pub fn remove<T: 'static>(&mut self) -> Option<T> {
        self.map.remove(&TypeId::of::<T>())?.downcast().ok().map(|b| *b)
    }

    pub fn contains<T: 'static>(&self) -> bool {
        self.map.contains_key(&TypeId::of::<T>())
    }
}
```

**Step 4: Register the module**

In `src/core/mod.rs`, add after the `scene` module declaration (line 54):

```rust
pub mod resources;
```

In `src/lib.rs`, add `Resources` to the prelude scene management section (line 59-62):

```rust
pub use crate::core::resources::Resources;
```

**Step 5: Run tests to verify they pass**

Run: `cargo test --lib resources::tests -p ryo2d`
Expected: All 11 tests pass.

**Step 6: Commit**

```bash
git add src/core/resources.rs src/core/mod.rs src/lib.rs
git commit -m "feat: add Resources type map for cross-scene shared state"
```

---

### Task 2: Add pop-with-data to SceneAction and Scene trait

**Files:**
- Modify: `src/core/scene.rs:748-770` (SceneAction enum)
- Modify: `src/core/scene.rs:825` (Scene::on_resume signature)
- Modify: `src/core/scene.rs:1061-1066` (PendingAction enum)
- Modify: `src/core/scene.rs:1193-1209` (SceneManager::pop)
- Modify: `src/core/scene.rs:1320-1358` (SceneManager::handle_action)
- Modify: `src/core/scene.rs:1445-1460` (execute_pending_action)

**Step 1: Add SceneAction variants**

In `src/core/scene.rs`, add to the `SceneAction` enum (after `ResetWith` at line 767):

```rust
/// Pop and return typed data to the scene below
PopReturn(Box<dyn std::any::Any>),
/// Pop with data and a transition effect
PopReturnWith(Box<dyn std::any::Any>, Transition),
```

Add a matching `PendingAction` variant (around line 1063):

```rust
PopReturn(Box<dyn std::any::Any>),
```

Update `PendingAction`'s Debug impl to handle the new variant:

```rust
PendingAction::PopReturn(_) => write!(f, "PopReturn(...)"),
```

**Step 2: Change Scene::on_resume signature**

Change `on_resume` at line 825 from:

```rust
fn on_resume(&mut self, _ctx: &mut SceneContext) {}
```

to:

```rust
fn on_resume(&mut self, _ctx: &mut SceneContext, _returned: Option<Box<dyn std::any::Any>>) {}
```

**Step 3: Update SceneManager::pop to accept return data**

Change `SceneManager::pop` (line 1193) to accept optional return data:

```rust
pub fn pop(&mut self, ctx: &mut SceneContext, returned: Option<Box<dyn std::any::Any>>) -> Option<Box<dyn Scene>> {
    if let Some(mut scene) = self.scenes.pop() {
        scene.on_exit(ctx);

        if let Some(new_top) = self.scenes.last_mut() {
            new_top.on_resume(ctx, returned);
            self.skip_next_update = true;
        }

        Some(scene)
    } else {
        None
    }
}
```

**Step 4: Update handle_action for new variants**

In `handle_action` (line 1320), update the `Pop` arm to pass `None`:

```rust
SceneAction::Pop => {
    self.pop(ctx, None);
    !self.scenes.is_empty()
}
```

Add the `PopWith` transition's `PendingAction::Pop` to also pass `None` (it already uses `PendingAction::Pop`).

Add new match arms:

```rust
SceneAction::PopReturn(data) => {
    self.pop(ctx, Some(data));
    !self.scenes.is_empty()
}
SceneAction::PopReturnWith(data, transition) => {
    self.start_transition(transition, PendingAction::PopReturn(data));
    true
}
```

**Step 5: Update execute_pending_action**

In `execute_pending_action` (line 1445), add:

```rust
PendingAction::PopReturn(data) => {
    self.pop(ctx, Some(data));
}
```

And update the existing `PendingAction::Pop` arm:

```rust
PendingAction::Pop => {
    self.pop(ctx, None);
}
```

**Step 6: Update the transition midpoint for PopWith**

The `PopWith` transition currently creates `PendingAction::Pop`. This is fine — plain `PopWith` still passes `None`. No change needed here.

**Step 7: Fix tests**

Update `TestScene::on_resume` in the test module (line 1937):

```rust
fn on_resume(&mut self, _ctx: &mut SceneContext, _returned: Option<Box<dyn std::any::Any>>) {
    self.resumed = true;
}
```

**Step 8: Run tests**

Run: `cargo test --lib scene::tests -p ryo2d`
Expected: All existing scene tests pass.

**Step 9: Commit**

```bash
git add src/core/scene.rs
git commit -m "feat: add PopReturn/PopReturnWith for returning data on scene pop"
```

---

### Task 3: Wire Resources into SceneContext

**Files:**
- Modify: `src/core/scene.rs:147-198` (SceneContext struct)
- Modify: `src/core/scene.rs:1696-1876` (SceneGame — add resources field, pass through)
- Modify: `src/core/engine.rs:173-193` (Engine struct — add resources field)
- Modify: `src/core/engine.rs:505+` (Engine::new — accept resources)
- Modify: `src/core/engine.rs:1200-1230` (game.update context construction)
- Modify: `src/core/engine.rs:2210-2237` (game.init context construction)

**Step 1: Add resources to SceneContext**

In `src/core/scene.rs`, add to `SceneContext` struct (after line 148):

```rust
/// Typed resource map for cross-scene shared state.
///
/// Resources persist across all scene transitions and are not affected
/// by `SceneCleanup`. Use for data that multiple scenes need to share
/// (scores, settings, game progress, etc.).
///
/// # Example
///
/// ```ignore
/// // Store data
/// ctx.resources.insert(Score { value: 100 });
///
/// // Read data in another scene
/// if let Some(score) = ctx.resources.get::<Score>() {
///     println!("Score: {}", score.value);
/// }
/// ```
pub resources: &'a mut super::resources::Resources,
```

**Step 2: Add resources to Engine struct**

In `src/core/engine.rs`, add to Engine struct (after line 188):

```rust
// Typed resource map for cross-scene shared state
resources: super::resources::Resources,
```

**Step 3: Update Engine::new to accept resources**

In `Engine::new` (line 505), add `resources: super::resources::Resources` parameter and store it in the struct. Also update `EngineBuilder::build` (line 3851) to pass resources.

**Step 4: Pass resources through every GameContext/SceneContext construction site**

There are multiple places in `engine.rs` where `GameContext` is constructed (search for `GameContext {` or `game.update` or `game.init`). Add `resources: &mut self.resources` is NOT directly possible here because `GameContext` doesn't have a resources field — it goes through `SceneGame` which constructs `SceneContext`.

Since we're keeping `Game`/`SceneGame` working temporarily in this task (removing them in Task 5), the path is:
1. Add `resources` to `GameContext` struct in `game.rs`
2. Pass `&mut self.resources` from engine at each `GameContext` construction
3. `SceneGame` already forwards `GameContext` fields to `SceneContext` — add the new field

Update `GameContext` in `src/core/game.rs` to include:

```rust
pub resources: &'a mut super::resources::Resources,
```

Update all `GameContext` construction sites in `engine.rs` to include `resources: &mut self.resources`.

Update `SceneGame` in `src/core/scene.rs` to forward `resources` from `GameContext` to `SceneContext` in `init()`, `update()`.

**Step 5: Run full test suite**

Run: `cargo test --lib -p ryo2d`
Expected: All tests pass (existing tests don't construct SceneContext/GameContext directly).

**Step 6: Commit**

```bash
git add src/core/resources.rs src/core/scene.rs src/core/game.rs src/core/engine.rs
git commit -m "feat: wire Resources into SceneContext via engine"
```

---

### Task 4: Add EngineBuilder::resource() and .start_scene()

**Files:**
- Modify: `src/core/engine.rs:3183-3209` (EngineBuilder struct — add resources field)
- Modify: `src/core/engine.rs:3212+` (EngineBuilder::new — init resources)
- Modify: `src/core/engine.rs:3604-3630` (with_game/with_scene — add resource() and start_scene())
- Modify: `src/core/engine.rs:3663+` (build — pass resources to Engine::new)

**Step 1: Add resources field to EngineBuilder**

In `src/core/engine.rs`, add to `EngineBuilder` struct (after line 3186):

```rust
resources: super::resources::Resources,
```

Initialize it in `EngineBuilder::new()`:

```rust
resources: super::resources::Resources::new(),
```

**Step 2: Add .resource() method**

Add after `with_scene()` (line 3630):

```rust
/// Register a typed resource that will be available to all scenes.
///
/// Resources persist across scene transitions and are accessible via
/// `ctx.resources` in any scene callback. Use this for shared game
/// state like scores, settings, or player data.
///
/// Multiple resources of different types can be registered. Each type
/// can only have one instance — inserting the same type twice overwrites.
///
/// # Example
///
/// ```rust,ignore
/// EngineBuilder::new()
///     .resource(Score::default())
///     .resource(Settings::load())
///     .start_scene(Box::new(MenuScene))
///     .build()
///     .run()?;
/// ```
pub fn resource(mut self, value: impl std::any::Any) -> Self {
    self.resources.insert(value);
    self
}
```

**Step 3: Add .start_scene() method**

Add after `.resource()`:

```rust
/// Set the initial scene that will run when the engine starts.
///
/// This is the primary way to launch a game. The scene's `on_enter`
/// will be called after engine initialization completes.
///
/// # Example
///
/// ```rust,ignore
/// EngineBuilder::new()
///     .title("My Game")
///     .start_scene(Box::new(MenuScene::new()))
///     .build()
///     .run()?;
/// ```
pub fn start_scene(mut self, scene: impl Scene + 'static) -> Self {
    self.game = Some(Box::new(SceneGame::new(Box::new(scene))));
    self
}
```

**Step 4: Pass resources in build()**

In `build()` (line 3663), pass `self.resources` to `Engine::new()`.

**Step 5: Run full test suite**

Run: `cargo test --lib -p ryo2d`
Expected: All tests pass.

**Step 6: Commit**

```bash
git add src/core/engine.rs
git commit -m "feat: add EngineBuilder::resource() and .start_scene()"
```

---

### Task 5: Remove Game trait, GameContext, SceneGame — engine runs scenes natively

This is the largest task. The engine currently stores `game: Option<Box<dyn Game>>` and calls `Game` trait methods. We need to replace this with `SceneManager` directly in the engine.

**Files:**
- Modify: `src/core/engine.rs` (replace `game: Box<dyn Game>` with `SceneManager` + scene state)
- Modify: `src/core/game.rs` (remove Game trait, GameContext; keep GpuRenderContext, TimeControl, TimeInfo, WindowRequest)
- Modify: `src/core/scene.rs` (remove SceneGame struct and its Game impl)
- Modify: `src/core/mod.rs` (keep game module for remaining types)
- Modify: `src/lib.rs` (update prelude — remove Game, GameContext, SceneGame exports)

**Step 1: Move SceneGame logic into Engine**

The `SceneGame` struct (scene.rs:1658-1876) manages:
- `manager: SceneManager`
- `initialized: bool`
- `quit_requested: bool`
- `next_scope_id: u64`
- `current_scope_id: u64`
- `pending_ui_action: Option<SceneAction>`

Move these fields into `Engine` struct (engine.rs:173), replacing `game: Option<Box<dyn Game>>`:

```rust
// Scene management (replaces game: Option<Box<dyn Game>>)
scene_manager: SceneManager,
scene_initialized: bool,
scene_quit_requested: bool,
next_scope_id: u64,
current_scope_id: u64,
pending_ui_action: Option<SceneAction>,
```

**Step 2: Update Engine::new**

Accept `initial_scene: Box<dyn Scene>` instead of `game: Box<dyn Game>`. Create `SceneManager::with_scene(initial_scene)`.

**Step 3: Replace all game.method() calls in engine.rs**

Find every place where the engine does:

```rust
if let Some(mut game) = self.game.take() {
    game.some_method(&mut ctx);
    self.game = Some(game);
}
```

Replace with direct SceneManager calls using SceneContext (not GameContext). The key call sites:

- **init** (line 2235): `game.init(&mut ctx)` becomes `self.scene_manager.top_mut().on_enter(&mut scene_ctx)` with scope setup
- **update** (line 1227): `game.update(&mut ctx, dt)` becomes the SceneGame update logic (process pending_ui_action, update_transition, scene_manager.update, handle_action) moved inline
- **render_ui** (line 1980): `game.render_ui(&mut ui_ctx)` becomes `self.scene_manager.render_ui(&mut ui_ctx)` with pending action capture
- **render_gpu** (line 887): `game.render_gpu(&mut gpu_ctx)` becomes `self.scene_manager.render_gpu(&mut gpu_ctx)`
- **is_physics_paused** (line 1256): `game.is_physics_paused()` becomes `self.scene_manager.is_physics_paused()`
- **get_overlay** (line 2022): `game.get_overlay()` becomes transition overlay logic from SceneManager
- **should_quit** (line 2661): `game.should_quit()` becomes `self.scene_quit_requested`

Construct `SceneContext` directly in the engine (no more GameContext intermediary):

```rust
let mut scene_ctx = SceneContext {
    resources: &mut self.resources,
    world: &mut self.world,
    camera: &mut self.camera,
    input: &mut self.input,
    assets: &mut self.assets,
    audio: &mut self.audio,
    effects: &mut self.effects,
    lighting: &mut self.lighting,
    events: &mut self.events,
    window_request: &mut self.window_request,
    cursor_request: &mut self.cursor_request,
    time_control: &mut self.time_control,
    time: time_info,
    scale_factor: self.scale_factor,
    display_mode: self.config.display_mode,
    command_registry: &mut self.command_registry,
    scope_id: self.current_scope_id,
};
```

**Step 4: Remove Game trait and GameContext from game.rs**

Delete the `Game` trait (lines 707-810) and `GameContext` struct (lines 159-567) from `src/core/game.rs`. Keep `GpuRenderContext`, `TimeControl`, `TimeInfo`, `WindowRequest` and their tests — these are used by the engine and scenes independently.

**Step 5: Remove SceneGame from scene.rs**

Delete the `SceneGame` struct and its `impl Game for SceneGame` block (lines 1658-1895).

**Step 6: Update EngineBuilder**

- Remove `with_game()` method (line 3604-3607)
- Remove `with_scene()` method (line 3627-3630) — replaced by `start_scene()`
- Change `build()` to accept `SceneManager` instead of `Box<dyn Game>`
- Update the error message in build() to reference `start_scene()`

**Step 7: Update prelude**

In `src/lib.rs`:

Remove from prelude:
```rust
// Remove: Game, GameContext from game exports
pub use crate::core::game::{GpuRenderContext, TimeControl, TimeInfo, WindowRequest};

// Remove: SceneGame from scene exports
pub use crate::core::scene::{
    Scene, SceneAction, SceneCleanup, SceneContext, SceneManager, SlideDirection, Transition,
};
```

**Step 8: Build the library**

Run: `cargo build -p ryo2d`
Expected: Library compiles. Examples will fail (they reference removed types) — that's expected, Task 6 handles migration.

**Step 9: Run library tests**

Run: `cargo test --lib -p ryo2d`
Expected: All library tests pass (game.rs tests for TimeControl/TimeInfo still work, scene tests still work, resources tests still work).

**Step 10: Commit**

```bash
git add src/core/engine.rs src/core/game.rs src/core/scene.rs src/lib.rs
git commit -m "refactor: remove Game trait, engine runs scenes natively"
```

---

### Task 6: Migrate all 35 examples to new API

**Files:**
- Modify: All `main.rs` files in `examples/demos/*/` and `examples/games/*/`
- Modify: All scene files that implement `on_resume` (games with pause menus)

This is a mechanical migration. Every example follows the same pattern.

**Step 1: Migrate demo main() functions (30 demos)**

Each demo's main() looks like:

```rust
// Before
let engine = EngineBuilder::new()
    .with_scene(MyScene::new())
    .build();
```

Change to:

```rust
// After
let engine = EngineBuilder::new()
    .start_scene(MyScene::new())
    .build();
```

This is a find-and-replace of `.with_scene(` to `.start_scene(` across all demo main.rs files.

**Step 2: Migrate game main() functions (5 games)**

Same pattern as demos, plus remove any `SceneGame` imports.

**Step 3: Fix on_resume signatures**

Search all scene files for `fn on_resume` and add the `returned` parameter:

```rust
// Before
fn on_resume(&mut self, ctx: &mut SceneContext) { ... }

// After
fn on_resume(&mut self, ctx: &mut SceneContext, _returned: Option<Box<dyn std::any::Any>>) { ... }
```

**Step 4: Fix any remaining use of Game/GameContext/SceneGame**

Search for `use ryo2d::prelude::*` — this should now work without Game/GameContext/SceneGame. If any file explicitly imports them, remove those imports.

Search for `use.*Game` and `use.*GameContext` across all examples.

**Step 5: Build the full workspace**

Run: `cargo build --workspace`
Expected: All 35 examples compile.

**Step 6: Run all tests**

Run: `cargo test --workspace`
Expected: All tests pass.

**Step 7: Commit**

```bash
git add examples/
git commit -m "refactor: migrate all examples to start_scene() API"
```

---

### Task 7: Update documentation

**Files:**
- Modify: `docs/src/getting-started/` (quick start, first game)
- Modify: `docs/src/concepts/` (any scene-related docs)
- Modify: `AGENTS.md` files that reference Game trait
- Modify: `ARCHITECTURE.md` if it references Game/SceneGame

**Step 1: Update getting-started guides**

Replace any `with_game`/`with_scene`/`SceneGame` references with `start_scene`. Add `resource()` to examples where appropriate.

**Step 2: Update AGENTS.md files**

Search all AGENTS.md for references to `Game` trait, `GameContext`, `SceneGame` and update.

**Step 3: Update ARCHITECTURE.md**

Update the architecture description to reflect scenes as the primary (only) game structure.

**Step 4: Commit**

```bash
git add docs/ AGENTS.md ARCHITECTURE.md
git commit -m "docs: update documentation for scene system redesign"
```

---

### Task 8: Clean up game.rs module

**Files:**
- Modify: `src/core/game.rs` (rename or restructure)

**Step 1: Evaluate remaining contents**

After removing `Game` and `GameContext`, `game.rs` still contains:
- `WindowRequest` struct
- `TimeControl` struct
- `TimeInfo` struct
- `GpuRenderContext` struct
- Tests for all of the above

These are all engine-level types that scenes use. Consider whether to:
- Keep `game.rs` as-is (just with less content) — simplest
- Rename to something more accurate if the name is misleading

The recommendation is to keep `game.rs` since renaming modules causes churn in imports across the codebase, and the types are still semantically "game" types.

**Step 2: Remove dead code**

Clean up any orphaned imports, unused `use` statements in `game.rs`.

**Step 3: Run tests**

Run: `cargo test --lib -p ryo2d`
Expected: All tests pass.

**Step 4: Commit**

```bash
git add src/core/game.rs
git commit -m "refactor: clean up game.rs after Game trait removal"
```
