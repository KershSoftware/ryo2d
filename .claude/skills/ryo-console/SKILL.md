---
name: ryo-console
description: Developer console system - commands, registration, debugging overlays.
metadata:
  version: 1.3.7
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
| `debug:ui-rebuilds` | | Outline the UI nodes rebuilt each frame in orange, fading over 0.5 s, with a "rebuilt N / replayed M nodes" counter in the bottom-left stack with the other overlays ("(spot check K)" in debug builds), a "fragments: N kept, M resolved again, K captured" line below it (unchanged subtrees copied, laid out again from what they kept, and kept for the next frame) and a render line below that: the frames the UI image was kept, and the texts built and reused. The lines update once a second, so the overlay does not keep the UI redrawing; the outlines show rebuilds as they happen |
| `ui:cache [on\|off]` | | Show or set UI node tree caching; off builds every node every frame (the same UI, slower), to rule caching in or out when chasing a UI bug |
| `ui:fragments [on\|off]` | | Show or set UI fragments; off replays and lays out every unchanged node subtree each frame instead of copying it (the same UI, slower), to rule the copies in or out when chasing a UI bug; it needs `ui:cache on` |
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

<!-- with: struct MenuScene; impl MenuScene { fn new() -> Self { Self } } impl Scene for MenuScene {} -->
```rust
EngineBuilder::new()
  .title("My Game")
  .console_command(("game:reset", "Reset the game to initial state"), |_world, cmd| {
    cmd.act(SceneAction::Switch(Box::new(MenuScene::new())));
    Ok(())
  })
  .console_command(
    CommandSpec::new("game:godmode", "Toggle invincibility")
      .help("Makes the player immune to all damage until toggled off."),
    |_world, cmd| {
      cmd.print("God mode toggled");
      Ok(())
    },
  )
  .remove_console_command("system:version") // drop a built-in the game does not want
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

<!-- with: struct GameScene; impl GameScene { fn new() -> Self { Self } } impl Scene for GameScene {} -->
```rust
fn on_enter(&mut self, ctx: &mut SceneContext) {
  // Register the command; it leaves with this scene
  ctx.console().command(("game:reset", "Reset the game"), |_game: &mut GameScene, _ctx, cmd| {
    cmd.print("Resetting game...");
    cmd.act(SceneAction::Switch(Box::new(GameScene::new())));
    Ok(())
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

```rust,ignore
// The engine's registrations
impl ConsoleScope<'_> {
  pub fn command<S: Scene>(&mut self, spec: impl Into<CommandSpec>,
    handler: impl Fn(&mut S, &mut SceneContext<'_>, &mut Command<'_>) -> Result<(), String> + Send + Sync + 'static);
}
impl EngineBuilder {
  pub fn console_command(self, spec: impl Into<CommandSpec>,
    handler: impl Fn(&mut World, &mut Command<'_>) -> Result<(), String> + Send + Sync + 'static) -> Self;
}
```

### What a handler receives

- `cmd: &mut Command` - the arguments (`args()`, typed `arg::<T>(i)` and `arg_or`, `rest(i)`), printing (`print`, `print_colored`), `act(SceneAction)` and `quit()`
- a scene's handler: its scene (`&mut S`) and the `SceneContext`; a builder's handler: the `World`

### Results

```rust,ignore
// A handler returns Result<(), String>
Ok(())                             // Command succeeded
Err("No player to heal".into())    // Command failed: printed in red
// cmd.quit() asks the game to exit; cmd.act(action) changes scenes
```

### Console Output Methods

<!-- given: cmd: &mut Command<'_> -->
```rust
cmd.print("Normal text");                                    // White
cmd.print_colored("System info", Color::rgb(150, 150, 150)); // Gray
ctx.console().print("From anywhere in a scene");             // White
```

## Complete Example

```rust,ignore
// Two files: main.rs and game.rs
// main.rs - a command for the whole run, with the world
EngineBuilder::new()
  .console_command(("game:gold", "Add 100 gold"), |world, cmd| {
    world.resource_mut::<Gold>().0 += 100;
    cmd.print("100 gold added");
    Ok(())
  })
  .start_scene(MenuScene::new())
  .build()
  .run()?;

// game.rs - a scene's command, with the scene itself
impl Scene for GameScene {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    ctx.console().command(("game:reset", "Reset the game"), |_game: &mut GameScene, _ctx, cmd| {
      cmd.print("Resetting game...");
      cmd.act(SceneAction::Switch(Box::new(GameScene::new())));
      Ok(())
    });
  }
}
```

A scene's handler is called with its scene and the scene context `update` gets; a builder's handler with the world.

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
- New output keeps the view at the bottom; scrolled up, the lines being read stay in place (wrapped lines included), until you submit a command, which jumps to the bottom as a terminal does
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
  mod.rs           - Console (a world resource), rendering, input handling
  command.rs       - CommandSpec, Command, splitting a line
  registry.rs      - ConsoleCommands: per name, a stack of entries by scope
  dispatch.rs      - running a line
  scope.rs         - ConsoleScope (ctx.console())
  builtins.rs      - Built-in debug/system commands
```

### Key Types

**Console** - UI state and rendering, a world resource:
- Window position/size, drag state
- Input buffer, cursor position
- Output lines (max 1000)
- Command history (max 100)
- Scroll offset, word wrapping

**ConsoleCommands** - the commands, a world resource in a build with the tools: per name, the run-long entry (a built-in or a builder command) and the scenes' entries by scope; the uppermost scene's answers, and the scene manager calls `clear_scope` when a scene leaves the stack

**ConsoleScope** - what `ctx.console()` returns: `command`, `remove`, `print` and `print_colored`, with the calling scene's scope id

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
