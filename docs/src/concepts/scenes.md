# Scene Management

Scenes represent distinct states of your game - menu screens, gameplay levels, pause overlays, etc. The scene management system provides a clean way to organize game logic and handle transitions between states.

## Core Concepts

### Scene Trait

Every scene implements the `Scene` trait with lifecycle hooks:

```rust
impl Scene for GameplayScene {
    fn on_enter(&mut self, ctx: &mut SceneContext) {
        // Called when this scene becomes active
        // Set up entities, load assets, start music
    }

    fn on_exit(&mut self, ctx: &mut SceneContext) {
        // Called when scene is removed from stack
        // Clean up, save state if needed
    }

    fn on_pause(&mut self, ctx: &mut SceneContext) {
        // Called when another scene is pushed on top
        // Pause music, timers, etc.
    }

    fn on_resume(&mut self, ctx: &mut SceneContext) {
        // Called when this scene becomes active again
        // Resume music, timers, etc.
    }

    fn update(&mut self, ctx: &mut SceneContext, dt: f32) -> SceneAction {
        // Main update logic - return an action to control flow
        SceneAction::Continue
    }
}
```

### Scene Actions

The `update()` method returns a `SceneAction` to tell the manager what to do:

| Action | Effect |
|--------|--------|
| `Continue` | Keep running this scene (default) |
| `Push(scene)` | Add new scene on top, current pauses |
| `Pop` | Remove this scene, resume one below |
| `Switch(scene)` | Replace this scene with a new one |
| `Reset(scene)` | Clear all scenes, start fresh |
| `Quit` | Exit the game |

## Scene Stack

The `SceneManager` maintains a stack of scenes:

```text
┌─────────────────┐
│  PauseScene     │ ← Top (active, receives updates)
├─────────────────┤
│  GameplayScene  │ ← Paused (on_pause called)
├─────────────────┤
│  MenuScene      │ ← Paused
└─────────────────┘
```

- **Push**: Adds a scene on top, pauses the current one
- **Pop**: Removes top scene, resumes the one below
- **Switch**: Replaces top scene (exits old, enters new)
- **Reset**: Clears entire stack, starts with fresh scene

## Usage with Engine

The simplest way to use scenes is with the `with_scene()` method:

```rust
use ryo2d::prelude::*;

struct MenuScene;

impl Scene for MenuScene {
    fn on_enter(&mut self, ctx: &mut SceneContext) {
        // Create menu UI
        ctx.world.spawn((
            Transform::new(400.0, 300.0),
            SpriteRenderer::new(Sprite::solid_rect(200, 50, 0xff, 0xff, 0xff, 0xff)),
        ));
    }

    fn update(&mut self, ctx: &mut SceneContext, _dt: f32) -> SceneAction {
        if ctx.input.key_pressed(KeyCode::Enter) {
            SceneAction::Switch(Box::new(GameplayScene::new()))
        } else if ctx.input.key_pressed(KeyCode::Escape) {
            SceneAction::Quit
        } else {
            SceneAction::Continue
        }
    }
}

fn main() {
    EngineBuilder::new()
        .title("Scene Demo")
        .with_scene(MenuScene)  // Direct scene support!
        .build()
        .run()
        .unwrap();
}
```

> **Note:** You can also use `with_game(SceneGame::new(Box::new(scene)))` if you need 
> direct access to the `SceneManager`, but `with_scene()` is cleaner for most cases.
```

## Transparent Scenes (Overlays)

For pause menus, HUDs, or dialog boxes that should render on top of the game:

```rust
impl Scene for PauseScene {
    fn is_transparent(&self) -> bool {
        true  // Render scenes below this one
    }

    fn update(&mut self, ctx: &mut SceneContext, _dt: f32) -> SceneAction {
        if ctx.input.key_pressed(KeyCode::Escape) {
            SceneAction::Pop  // Resume gameplay
        } else {
            SceneAction::Continue
        }
    }
}
```

## Background Updates

By default, only the top scene updates. To allow background scenes to continue updating:

```rust
impl Scene for HudScene {
    fn updates_below(&self) -> bool {
        true  // Allow gameplay scene to update while HUD is on top
    }
}
```

## Common Patterns

### Menu → Gameplay Flow

```rust
// In MenuScene::update
if ctx.input.key_pressed(KeyCode::Enter) {
    SceneAction::Switch(Box::new(GameplayScene::new()))
}
```

### Pause/Resume

```rust
// In GameplayScene::update
if ctx.input.key_pressed(KeyCode::Escape) {
    SceneAction::Push(Box::new(PauseScene::new()))
}

// In PauseScene::update
if ctx.input.key_pressed(KeyCode::Escape) {
    SceneAction::Pop  // Back to gameplay
}
```

### Return to Menu

```rust
// From any scene
if want_main_menu {
    SceneAction::Reset(Box::new(MenuScene::new()))
}
```

## Best Practices

1. **Clean up in `on_exit`**: Despawn entities, stop sounds
2. **Use `World::clear()`**: For scene transitions that should reset everything
3. **Keep scenes focused**: Each scene should have a single responsibility
4. **Pass data between scenes**: Use scene struct fields to transfer state

```rust
struct GameOverScene {
    final_score: u32,
}

impl GameOverScene {
    fn new(score: u32) -> Self {
        Self { final_score: score }
    }
}
```

## API Reference

See the [rustdoc API documentation](../api/scene.html) for complete details on:

- `Scene` trait
- `SceneManager` methods
- `SceneAction` variants
- `SceneContext` fields
- `SceneGame` adapter
