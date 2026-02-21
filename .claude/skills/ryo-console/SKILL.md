---
name: ryo-console
description: Developer console system - commands, registration, debugging overlays.
metadata:
  version: 1.0.0
---

# Ryo2D Developer Console

You help users work with the Valve-style developer console system, including registering custom commands, understanding the two-phase command pattern, and debugging games at runtime.

## Overview

The developer console is a draggable, resizable overlay toggled with the tilde (`~`) key. It provides:
- Runtime command execution
- Debug toggles (FPS, colliders, timing stats)
- Custom game commands with scoped handlers
- Scrollable output with word wrapping

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
| `debug:fps` | F1 | Toggle FPS counter |
| `debug:stats` | F2 | Toggle detailed stats overlay |
| `debug:timing` | F3 | Toggle frame timing graph |
| `debug:colliders` | F4 | Toggle collision shape visualization |
| `system:version` | | Show engine and game version |
| `system:timescale <n>` | | Set game time scale (0.1-10.0) |

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
    })
    .start_scene(MenuScene::new())
    .build()
    .run()?;
```

Scene-specific commands can also be declared via `ctx.declare_console_command()` in `on_enter`.

Parameters:
- `name` - Command name (use `game:` prefix for game commands)
- `description` - Brief description shown in command listings
- `verbose_description` - Optional detailed help (shown with `help game:reset`)

### Phase 2: Registration (Scoped)

Register handlers where they make sense (e.g., in GameScene):

```rust
fn on_enter(&mut self, ctx: &mut SceneContext) {
    // Register handler - command becomes active
    ctx.register_console_command("game:reset", |args, cmd_ctx| {
        cmd_ctx.console.print_system("Resetting game...");
        // Trigger reset logic
        shared::request_reset();
        CommandResult::Ok
    });
}
```

### Automatic Cleanup

When a scene exits, all handlers registered in that scene's scope are automatically removed. The command reverts to "noop" state - it still appears in help but prints:

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
| `time_scale` | `&mut f32` | Adjust game speed |
| `commands` | `Vec<CommandInfo>` | List of all commands |
| `game_name` | `Option<String>` | Game name from Ryo.toml |
| `game_version` | `Option<String>` | Game version from Ryo.toml |

### CommandResult

```rust
pub enum CommandResult {
    Ok,                    // Command succeeded
    Error(String),         // Command failed with message
    Quit,                  // Request game exit
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
        ctx.register_console_command("game:reset", |_args, cmd_ctx| {
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
- **Mouse wheel** - Scroll output
- **Scrollbar** - Click and drag to scroll

### Visual Style
- Semi-transparent dark background
- Color-coded output (white, blue, red, gray)
- Blinking cursor
- Word wrapping for long lines

## Game Version in Console

Add version to `Ryo.toml`:

```toml
[project]
name = "My Game"
version = "1.0.0"
```

The `system:version` command will show:

```
Ryo2D Engine v0.0.1 (Dev)
My Game v1.0.0
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

**CommandRegistry** - Command management:
- `builtins: HashMap<&'static str, BuiltinCommand>` - Engine commands
- `declared: HashMap<String, DeclaredCommand>` - Game commands
- Scope-based handler cleanup

**DeclaredCommand** - Game command with optional handler:
```rust
pub struct DeclaredCommand {
    pub name: String,
    pub description: String,
    pub verbose_description: Option<String>,
    pub handler: Option<(u64, CommandFn)>,  // (scope_id, handler)
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
