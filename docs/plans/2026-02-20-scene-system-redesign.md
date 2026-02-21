# Scene System Redesign

## Problem

Ryo2D's scene system has a solid stack-based scene manager but lacks a shared data layer between scenes. Passing data between scenes requires threading values through constructor arguments (`GameOverScene::new(score, high_score, is_new_high_score)`), which doesn't scale for larger games. There is no way to return data when popping a scene (e.g., a pause menu can't send changed settings back to the game scene). The `Game` trait and `SceneGame` adapter add unnecessary indirection since every example already uses `Scene`.

## Design

### 1. Resources: Typed Shared State

A type-erased map that stores one value per type. The engine owns it; scenes access it through `SceneContext`.

```rust
pub struct Resources {
    map: HashMap<TypeId, Box<dyn Any>>,
}

impl Resources {
    pub fn insert<T: 'static>(&mut self, value: T);
    pub fn get<T: 'static>(&self) -> Option<&T>;
    pub fn get_mut<T: 'static>(&mut self) -> Option<&mut T>;
    pub fn get_or_insert_with<T: 'static>(&mut self, f: impl FnOnce() -> T) -> &mut T;
    pub fn remove<T: 'static>(&mut self) -> Option<T>;
    pub fn contains<T: 'static>(&self) -> bool;
}
```

Resources are registered on `EngineBuilder` before the first scene runs, and are also insertable at runtime from any scene:

```rust
EngineBuilder::new()
    .title("My Game")
    .resource(Settings::load())
    .resource(Score::default())
    .start_scene(Box::new(MenuScene))
    .build()
    .run()?;
```

`SceneContext` gains a new field:

```rust
pub struct SceneContext<'a> {
    pub resources: &'a mut Resources,
    pub world: &'a mut World,
    pub camera: &'a mut Camera,
    // ... all existing fields unchanged
}
```

Resources are NOT cleared by `SceneCleanup`. They persist across all transitions by default. Explicit removal is done via `ctx.resources.remove::<T>()`.

### 2. Pop-With-Data

Scenes can return typed data when popping. The resumed scene receives it in `on_resume`.

New `SceneAction` variants:

```rust
pub enum SceneAction {
    // ... all existing variants unchanged ...
    PopReturn(Box<dyn Any>),                  // pop and send data back
    PopReturnWith(Box<dyn Any>, Transition),  // pop with data + transition
}
```

`Scene::on_resume` gains a parameter:

```rust
fn on_resume(&mut self, ctx: &mut SceneContext, returned: Option<Box<dyn Any>>) {}
```

Plain `Pop` and `PopWith(transition)` pass `None` to `on_resume`. Only `PopReturn` / `PopReturnWith` send data.

Usage example:

```rust
// PauseScene pops with data
fn update(&mut self, ctx: &mut SceneContext, dt: f32) -> SceneAction {
    if ctx.input.key_pressed(KeyCode::Escape) {
        SceneAction::PopReturn(Box::new(PauseResult {
            volume_changed: true,
            new_volume: 0.8,
        }))
    } else {
        SceneAction::Continue
    }
}

// GameScene receives it
fn on_resume(&mut self, ctx: &mut SceneContext, returned: Option<Box<dyn Any>>) {
    if let Some(data) = returned.and_then(|d| d.downcast::<PauseResult>().ok()) {
        if data.volume_changed {
            // apply new volume
        }
    }
}
```

### 3. Remove Game Trait, Engine Runs Scenes Natively

The `Game` trait, `GameContext`, and `SceneGame` adapter are removed. The engine loop calls `Scene` methods directly.

Removed:
- `Game` trait
- `GameContext` struct
- `SceneGame` struct

The engine's main loop takes over the responsibilities currently in `SceneGame`:
- Scope ID tracking for console command cleanup moves into `SceneManager`
- The `GameContext` to `SceneContext` conversion boilerplate disappears
- Transition handling, pending UI actions, quit detection move into the engine loop

Entry point changes:

```rust
// Before
let game = SceneGame::new(Box::new(MenuScene));
EngineBuilder::new()
    .title("My Game")
    .with_game(game)
    .build()
    .run()?;

// After
EngineBuilder::new()
    .title("My Game")
    .resource(Settings::load())
    .start_scene(Box::new(MenuScene))
    .build()
    .run()?;
```

### 4. Migration

Games resolve their pain points as follows:

- **Constructor parameter threading** (flappy score, breakout high_score): Shared state moves to resources. Scenes read/write `ctx.resources.get::<Score>()` instead of passing through constructors.
- **Music handle passing** (breakout): Music state lives in resources.
- **Pause menu can't return data** (breakout settings): Uses `SceneAction::PopReturn`.
- **Duplicated disk loads** (bullet-hell best_time): Loaded once at startup via `.resource()`.
- **Demos** (30 single-scene apps): Mechanical replacement of `SceneGame::new()` + `.with_game()` with `.start_scene()`.

### 5. What Stays Unchanged

- `SceneManager` stack, transitions, `is_transparent()`/`updates_below()` rendering
- `SceneCleanup` bitflags (no new flags; resources are exempt from cleanup)
- `Transition` effects (fade, slide)
- All `SceneContext` fields (world, camera, input, assets, audio, effects, lighting, events, time_control, time, window_request, cursor_request, command_registry, scope_id)
- All `Scene` trait methods except `on_resume` signature change
- Events system, ECS/World, renderer, physics, audio, input

## Change Surface

| Area | Change |
|------|--------|
| New: `Resources` struct | ~60 lines |
| New: `EngineBuilder::resource()` + `.start_scene()` | ~20 lines |
| Modified: `SceneAction` enum | +2 variants |
| Modified: `Scene::on_resume` signature | +1 parameter |
| Modified: `SceneManager::pop` | passes return data through |
| Modified: engine main loop | replaces SceneGame adapter logic |
| Removed: `Game`, `GameContext`, `SceneGame` | ~300 lines deleted |
| Updated: 35 example `main()` functions | mechanical replacement |
