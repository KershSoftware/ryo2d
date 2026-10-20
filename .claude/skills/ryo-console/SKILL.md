---
name: ryo-console
description: Developer console system - commands, registration, debugging overlays.
metadata:
  version: 1.3.4
---

# Ryo2D Developer Console

You help users work with the Valve-style developer console system, including registering custom commands, understanding the two-phase command pattern, and debugging games at runtime.

## Overview

The developer console is a draggable, resizable overlay toggled with the backtick key (`KeyCode::Backquote`, the `~` key on US layouts). It provides:
- Runtime command execution
- Debug toggles (FPS, colliders, timing stats)
- Custom game commands with scoped handlers
- Scrollable output with word wrapping

While it is open and focused it takes the keyboard (the game sees no keys); an unfocused console stays open while the game plays. The mouse over its window never reaches the game or the scene UI. Commands run once per frame before the scene UI and the fixed steps, on real time, so they work while the game is paused.

## Enabling the Console

In `Ryo.toml`:

```toml
[system]
console = true  # Works in both debug and release builds
```

## Built-in Commands

| Command | Shortcut | Description |
|---------|----------|-------------|
| `help [cmd]` | | List commands or show detailed help |
| `clear` | | Clear console output |
| `quit` / `exit` | | Exit game |
| `debug:fps` | F1 (cycles overlays) | Toggle FPS counter |
| `debug:stats` | F1 (cycles overlays) | Toggle detailed stats overlay |
| `debug:timing` | F1 (cycles overlays) | Toggle frame timing breakdown |
| `debug:colliders` | F2 | Toggle collision shape visualization |
| `debug:velocities` | F3 | Toggle velocity vectors |
| `debug:origins` | F4 | Toggle entity origin points |
| `debug:ui` | | Outline every UI widget with its id: hot yellow, active orange, focused green |
| `debug:ui-rebuilds` | | Outline the UI nodes rebuilt each frame in orange, fading over 0.5 s, with a "rebuilt N / replayed M nodes" counter in the bottom-left stack with the other overlays ("(spot check K)" in debug builds) and a render line below it: the frames the UI image was kept, and the texts built and reused. Both lines update once a second, so the overlay does not keep the UI redrawing; the outlines show rebuilds as they happen |
| `ui:cache [on\|off]` | | Show or set UI node tree caching; off builds every node every frame (the same UI, slower), to rule caching in or out when chasing a UI bug |
| `ui:retain [on\|off]` | | Show or set UI render retention; off draws the whole UI every frame (the same pixels, slower), to rule retention in or out when chasing a rendering bug |
| `debug:capture [path]` | F12 | Save the next frame as a PNG |
| `system:version` | | Show engine and game version |
| `system:timescale <n>` | | Set the game clock's speed (`ctx.time.game.speed()`, 0.0-10.0) |
| `system:ip:lan` / `system:ip:wan` | | Show local / public IP address |

Category commands list related commands:
- `debug` - List all debug commands
- `system` - List all system commands
- `game` - List all game-specific commands

## Two-Phase Command System

Game commands use a declare-then-register pattern for clean lifecycle management:

### Phase 1: Declaration (Startup)

Declare game-wide commands on the engine builder (recommended):

```rust
EngineBuilder::new()
  .title("My Game")
  .with_console_commands(|cmd| {
    cmd.declare("game:reset", "Reset the game to initial state");
    cmd.declare_verbose(
      "game:godmode",
      "Toggle invincibility",
      "Makes the player immune to all damage until toggled off.",
    );
    cmd.remove_builtin("system:ip:wan"); // drop a built-in the game does not want
  })
  .start_scene(MenuScene::new())
  .build()
  .run()?;
```

Scene-specific commands can also be declared in `on_enter` with `ctx.console().declare(name, description, verbose)` (`verbose` is an `Option`, e.g. `None::<&str>`). Declarations are global and idempotent.

Parameters:
- `name` - Command name (use `game:` prefix for game commands)
- `description` - Brief description shown in command listings
- `verbose_description` - Optional detailed help (shown with `help game:reset`)

### Phase 2: Registration (Scoped)

Register handlers where they make sense (e.g., in GameScene). `ctx.console()` returns a `ConsoleScope` tied to the calling scene; `register` returns `false` if the command was never declared:

```rust
fn on_enter(&mut self, ctx: &mut SceneContext) {
  // Register handler - command becomes active
  ctx.console().register("game:reset", |_args, cmd_ctx| {
    cmd_ctx.console.print_system("Resetting game...");
    // Trigger reset logic
    shared::request_reset();
    CommandResult::Ok
  });
}
```

`ctx.console().remove_builtin(name)` removes a built-in command at runtime.

### Automatic Cleanup

When a scene leaves the stack, every handler it registered is removed (each scene instance has its own scope, so a pushed overlay's handlers go with the overlay). The command reverts to "noop" state - it still appears in help but prints:

```
Command 'game:reset' has no effect in the current context
  Reset the game to initial state
```

## Command Handler Signature

```rust
type CommandFn = Box<dyn Fn(&[&str], &mut CommandContext) -> CommandResult + Send + Sync>;
```

### Arguments

- `args: &[&str]` - Command arguments (excludes command name)
- `ctx: &mut CommandContext` - Access to console and debug config

### CommandContext Fields

| Field | Type | Purpose |
|-------|------|---------|
| `console` | `&mut ConsoleState` | Print output to console |
| `debug_config` | `&mut DebugConfig` | Toggle debug flags |
| `quit_requested` | `bool` | Set to request game exit |
| `capture_requested` | `Option<CaptureTarget>` | Set to request a frame capture |
| `time_scale` | `&mut f32` | The game clock's speed; written back after the command |
| `commands` | `Vec<CommandInfo>` | List of all commands |
| `game_name` | `Option<String>` | Game name from Ryo.toml |
| `game_version` | `Option<String>` | Game version from Ryo.toml |

### CommandResult

```rust
pub enum CommandResult {
  Ok,            // Command succeeded
  Error(String), // Command failed with message
  Quit,          // Request game exit
}
```

### Console Output Methods

```rust
cmd_ctx.console.print("Normal text");           // White
cmd_ctx.console.print_command("> echoed cmd");  // Light blue
cmd_ctx.console.print_error("Error message");   // Red
cmd_ctx.console.print_system("System info");    // Gray
```

## Complete Example

```rust
// shared.rs - Cross-scene communication
use std::sync::atomic::{AtomicBool, Ordering};
static RESET_REQUESTED: AtomicBool = AtomicBool::new(false);

pub fn request_reset() {
  RESET_REQUESTED.store(true, Ordering::SeqCst);
}

pub fn take_reset_request() -> bool {
  RESET_REQUESTED.swap(false, Ordering::SeqCst)
}

// main.rs - Declare command at engine level
EngineBuilder::new()
  .with_console_commands(|cmd| {
    cmd.declare("game:reset", "Reset the game to initial state");
  })
  .start_scene(MenuScene::new())
  .build()
  .run()?;

// game.rs - Register handler
impl Scene for GameScene {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    ctx.console().register("game:reset", |_args, cmd_ctx| {
      cmd_ctx.console.print_system("Resetting game...");
      shared::request_reset();
      CommandResult::Ok
    });
  }

  fn update(&mut self, ctx: &mut SceneContext, dt: f32) -> SceneAction {
    // Check for reset request
    if shared::take_reset_request() {
      return SceneAction::Switch(Box::new(GameScene::new()));
    }
    SceneAction::Continue
  }
}
```

Handlers are `Fn(&[&str], &mut CommandContext) -> CommandResult + Send + Sync + 'static` and cannot reach the world, which is why the example passes the request through an atomic.

## Console UI Features

### Window Controls
- **Drag** title bar to move
- **Drag** bottom-right corner to resize
- **Click** X button or press Escape to close
- **Click** outside to unfocus (game receives input)
- **Click** inside to refocus

### Input
- **Up/Down arrows** - Navigate command history
- **Page Up/Down** - Scroll output
- **Mouse wheel** - Scroll output; a wheel notch eases three lines, a trackpad follows the fingers at once (as far as the pointer would move)
- New output keeps the view at the bottom; scrolled up, the lines being read stay in place
- **Scrollbar** - Click and drag to scroll

### Visual Style
- Semi-transparent dark background
- Color-coded output (white, blue, red, gray)
- Blinking cursor
- Over the console window the mouse cursor is the default arrow (a resize arrow on the corner handle, a move cursor while dragging), whatever the game set; the game's cursor returns when the pointer leaves or the drag ends
- JetBrains Mono (the `FontRole::Mono` font), so a game that points that role at its own font changes the console too
- Rows are sized by the font's real line height, not a fixed cell
- Word wrapping for long lines, in any script (the fonts must have the characters; see `ryo-text`)

## Game Version in Console

Add version to `Ryo.toml`:

```toml
[project]
name = "My Game"
version = "1.0.0"
```

The `system:version` command will show:

```
My Game
  Version: 1.0.0

Ryo2D Game Engine
  Engine: 0.0.1
  Build:  Dev
```

## Architecture

```
src/core/console/
  mod.rs           - ConsoleState, rendering, input handling
  commands.rs      - CommandRegistry, two-phase system
  builtins.rs      - Built-in debug/system commands
```

### Key Types

**ConsoleState** - UI state and rendering:
- Window position/size, drag state
- Input buffer, cursor position
- Output lines (max 1000)
- Command history (max 100)
- Scroll offset, word wrapping

**CommandRegistry** - Command management, a resource in the world:
- `builtins: HashMap<&'static str, BuiltinCommand>` - Engine commands
- `declared: HashMap<String, DeclaredCommand>` - Game commands
- Scope-based handler cleanup (the scene manager calls `clear_scope` when a scene leaves the stack)

**ConsoleScope** - what `ctx.console()` returns: `declare`, `register` and `remove_builtin` against the registry, with the calling scene's scope id

**DeclaredCommand** - Game command with optional handler:
```rust
pub struct DeclaredCommand {
  pub name: String,
  pub description: String,
  pub verbose_description: Option<String>,
  pub handler: Option<(u64, CommandFn)>, // (scope_id, handler)
}
```

## Testing Commands

The console module has comprehensive tests:

```bash
cargo test --lib console::
```

Tests cover:
- Command declaration and registration
- Handler lifecycle (register, execute, clear_scope)
- Noop behavior for inactive commands
- Scrollbar calculations
- Word wrapping
- Console state operations
