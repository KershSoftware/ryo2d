---
name: ryo-console
description: Developer console - registering commands for a scene (ctx.console().command) or the whole run (EngineBuilder::console_command), Command's arguments and printing, scene changes from a command, the built-in commands (debug overlays, time scale, log levels), help and Tab completion, the log's warnings in the console, and what a shipped build has (none of it).
metadata:
  version: 1.4.0
---

# Ryo2D Developer Console

You help users add commands to the developer console and use it to debug a game while it runs.

**For complete API signatures, see the `ryo-api` skill.**

## Overview

The console is a moveable, resizable window over the game, opened with the backquote key (`` ` ``, `KeyCode::Backquote`: the key left of `1` on a US keyboard, found by its place whatever character the layout gives it). Type a command and press Enter; its lines print below. It holds:

- the engine's built-in commands (overlays, time scale, the scene stack, log levels, ...);
- the game's own commands, registered by a scene or for the whole run;
- the log's warnings and errors, as they are logged.

The console is a developer tool. It is in every development build (`cargo run`, `ryo2d dev`) and in `ryo2d build --dev-tools`; a game built with plain `ryo2d build` has no console at all (see A shipped build, below). `ryo2d::DEV_TOOLS` (in the prelude) says which build is running.

Commands run once a frame before the scene UI and the fixed steps, on real time, so they work while the game is paused.

## A scene's command

A scene registers commands with `ctx.console().command`, usually in `on_enter`. The handler is called with the scene itself, the scene context `update` gets, and the running `Command`. It returns `Result<(), String>`: an `Err` prints in red.

```rust
struct Level {
  lives: u32,
}

impl Scene for Level {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    ctx.console().command(
      CommandSpec::new("game:lives", "Set the lives left").usage("game:lives <count>"),
      |level: &mut Level, _ctx, cmd| {
        level.lives = cmd.arg(0)?;
        cmd.print(format!("{} lives", level.lives));
        Ok(())
      },
    );
  }
}
```

- The command leaves with its scene: when the scene is popped, switched or reset away, its commands go too.
- Write the scene's type on the handler's first argument (`level: &mut Level`): that is the type the command is registered for. The handler is called with the scene that registered it, so the two must be the same type: a handler typed for another scene compiles, but typed in the console it runs nothing and prints `<name> was registered for another scene type` in red. A game that wraps its scenes (a scene that holds the level and forwards to it) meets this: register from the wrapper with the wrapper's type, and reach the inner scene through it.
- A `(name, description)` pair works wherever a `CommandSpec` does: `ctx.console().command(("game:heal", "Heal the player"), ...)`.
- What the handler does through `ctx` is its scene's, as in `update`: the sounds it plays and the particle effects it starts belong to that scene (held and stopped with it), and it can spawn and change entities and resources.
- Registering a name this scene registered already replaces its handler.
- When two scenes on the stack register the same name, the scene nearer the top answers; when it leaves, the one below answers again. So a pause menu pushed over a level can register its own `game:reset` without touching the level's.
- `ctx.console().remove(name)` removes a command, whoever registered it (a built-in too), and says whether there was one.
- `ctx.console().print(text)` and `print_colored(text, color)` print a line to the console at once, from anywhere in a scene.

## A command for the whole run

`EngineBuilder::console_command` registers a command that lasts for the whole run. Its handler gets the `World` and the `Command`:

<!-- with: #[derive(Resource, Default)] struct Gold(u32); struct Menu; impl Scene for Menu {} -->
```rust
EngineBuilder::new()
  .insert_resource(Gold::default())
  .console_command(
    CommandSpec::new("game:gold", "Add gold").usage("game:gold <amount>"),
    |world, cmd| {
      let amount: u32 = cmd.arg(0)?;
      world.resource_mut::<Gold>().0 += amount;
      cmd.print(format!("{amount} gold added"));
      Ok(())
    },
  )
  .remove_console_command("system:version") // a built-in the game does not want
  .start_scene(Menu)
  .build()
  .run()?;
```

- A builder command named like a built-in replaces the built-in, and a later `console_command` of the same name replaces an earlier one.
- A scene's command of the same name answers over a builder command while the scene is on the stack.
- `remove_console_command(name)` removes a command (a built-in included) before the engine starts, after the calls made before it.

## Names

- Names are lower-cased when registered and when typed, so `Game:Gold` and `game:gold` are the same command.
- Use a `category:command` form; the game's commands go under `game:`. `help` groups commands by the text before the first `:`.
- A name that is empty or has a space in it is refused, with a warning in the log naming it.

## The running `Command`

The arguments are the words typed after the name. Double quotes keep a phrase together as one argument (`game:say "hello there"`), and `""` is an empty argument. Indexes count from 0.

| Call | Gives |
|------|-------|
| `cmd.name()` | the command's name, lower case |
| `cmd.args()` | `&[String]`, the arguments |
| `cmd.arg::<T>(i)` | argument `i` parsed as any `FromStr` type; the error is ready to print: `argument 2 ("abc") is not a number; usage: game:spawn <kind> [count]` (counted from 1, with the usage line when the spec has one) |
| `cmd.arg_or::<T>(i, default)` | as `arg`, or `default` when there are fewer arguments; an argument that is there but does not parse is still an error |
| `cmd.rest(i)` | the arguments from `i` on, joined by spaces (empty when there are none), for a message or a path |
| `cmd.print(text)`, `cmd.print_colored(text, color)` | a line of output |
| `cmd.act(action)` | a `SceneAction` applied when the handler returns (the last one asked for wins) |
| `cmd.quit()` | quit the game when the handler returns |

<!-- with: struct Spawner { count: u32 } impl Scene for Spawner {} -->
```rust
fn register(ctx: &mut SceneContext) {
  ctx.console().command(
    CommandSpec::new("game:spawn", "Spawn enemies")
      .usage("game:spawn <kind> [count]")
      .help("Kinds: orc, bat. Count defaults to 1."),
    |spawner: &mut Spawner, _ctx, cmd| {
      let kind: String = cmd.arg(0)?;
      let count: u32 = cmd.arg_or(1, 1)?;
      if kind != "orc" && kind != "bat" {
        return Err(format!("no enemy called {kind}"));
      }
      spawner.count += count;
      cmd.print(format!("spawned {count} {kind}"));
      cmd.print_colored("(they spawn next step)", Color::rgb(150, 150, 150));
      Ok(())
    },
  );
}
```

The lines a handler prints through `cmd` go to the console when it returns, then its error. A line it prints through `ctx.console().print` goes in at once, so it comes before the command's own lines.

### Changing scenes

`cmd.act(SceneAction::...)` changes the stack as an `update` returning that action would:

<!-- with: struct Title; impl Scene for Title {} struct Game; impl Scene for Game {} -->
```rust
fn register(ctx: &mut SceneContext) {
  ctx.console().command(("game:title", "Back to the title screen"), |_game: &mut Game, _ctx, cmd| {
    cmd.act(SceneAction::Switch(Box::new(Title)));
    Ok(())
  });
}
```

- A builder command's action always applies. A scene's applies only when that scene is the top one: a scene lower on the stack (under a pause menu, say) cannot change scenes, and its action is dropped with a red line saying so, as a lower scene's `update` action is.
- While a scene transition runs, only `SceneAction::Quit` (and `cmd.quit()`) applies; any other change is dropped with a red line.
- A handler may register or remove commands, or pop its own scene: nothing of the console is borrowed while it runs. The popped scene's commands are gone afterwards.
- A stack left empty quits the game, as it does from `update`.

## Built-in commands

| Command | Shortcut | Does |
|---------|----------|------|
| `help [command]` | | List every command, grouped by category; with a name, its description, usage and help |
| `clear` | | Clear the console's output |
| `quit`, `exit` | | Quit the game |
| `debug:fps` | F1 cycles | Toggle the FPS counter (the player's setting: `ctx.settings()` follows it) |
| `debug:stats` | F1 cycles | Toggle the statistics overlay |
| `debug:timing` | F1 cycles | Toggle the frame timing overlay |
| `debug:colliders` | F2 | Toggle collision shapes |
| `debug:velocities` | F3 | Toggle velocity vectors |
| `debug:origins` | F4 | Toggle entity origin points |
| `debug:ui` | | Outline every UI widget with its id: hot yellow, active orange, focused green |
| `debug:ui-rebuilds` | | Outline the UI nodes rebuilt each frame (fading orange), with counters of nodes rebuilt and replayed, fragments kept and the render line, updated once a second (`ryo-ui`) |
| `debug:capture [path]` | F12 | Save the next frame as a PNG (default `captures/`, or `RYO_CAPTURE_DIR`) |
| `ui:cache [on\|off]` | | Show or set UI node tree caching; off builds every node every frame (the same UI, slower), to rule caching in or out of a UI bug |
| `ui:fragments [on\|off]` | | Show or set UI fragments; off replays and lays out every unchanged subtree each frame (needs `ui:cache on`) |
| `ui:retain [on\|off]` | | Show or set UI render retention; off draws the whole UI every frame (the same pixels, slower) |
| `system:timescale [n]` | | Show or set the game clock's speed, 0 to 100 (1 is normal) |
| `system:version` | | The game's name and version (`[project] version` in Ryo.toml), the engine's version and the build |
| `system:scenes` | | The scene stack, top first, each marked `updates` or `stopped` |
| `system:entities` | | Count the game's entities |
| `system:assets` | | The asset registry's counts per type: loaded, loading, failed |
| `log:level [target] [level]` | | Show or set the log's levels (below) |
| `log:show [level\|off]` | | Show or set the lowest level of log record the console shows (default `warn`) |

A game's builder command of the same name replaces a built-in; `remove_console_command` or `ctx.console().remove` removes one.

## Help and Tab

- `help` lists every command with its one-line description, those without a category first, then each category under its name. `help game:spawn` shows the description, the `Usage:` line and the help text.
- Tab on the first word completes the command's name: one match becomes the name and a space; several are completed to their longest common start, and a second Tab lists them.
- Up and Down walk the last 100 commands.

## The log in the console

The console shows the log's warnings and errors as `target: message`: warnings in yellow, errors in red. It catches up whenever it opens: what was logged while it was closed is there, the warnings from start-up (a misspelt Ryo.toml key, a save folder that cannot be written) included. It keeps its last 1,000 lines; the warnings and errors it can no longer show are counted in one grey line, `N earlier warnings and errors are no longer shown`.

- `log:show info` shows info lines too (white), `log:show debug` and `trace` the quieter ones (grey); `log:show off` shows none; `log:show warn` is the default.
- `log:level` with nothing lists the three default levels and every target's own level. `log:level game debug` lets the game's `debug!` lines through; `log:level engine warn` quietens the engine; `log:level library info` covers every other crate; any other word is a target, which covers the targets under it (`log:level game::my_game::ai trace`: a game's targets are `game::`, its crate and the module path). A level is one of `off`, `error`, `warn`, `info`, `debug`, `trace`. The levels are the log's own (the file and the terminal follow them too; see `ryo-game-dev`, Logging), and last for the run.

So to watch a part of the game closely: `log:level game::my_game::ai debug`, then `log:show debug`.

## The window

- **Open and close:** the backquote key opens it and, pressed again, closes it, whatever the input line holds; its own character is never typed. A half-typed line is kept and is there when the console opens again. Escape and the window's X close it too.
- **Keyboard:** while it is open and focused it takes the keyboard (the game sees no keys). Click outside it to unfocus it and play with it open; click inside to focus it again.
- **Mouse:** any mouse button pressed over the window stays away from the game and the scene UI, through its release. A drag the game started outside the window keeps its holds and release when the pointer crosses it.
- **Editing:** Left, Right, Home, End, Backspace, Delete; Enter runs the line.
- **Moving and sizing:** drag the title bar to move it, the bottom-right corner to resize it.
- **Scrolling:** the mouse wheel (three lines a notch), a trackpad (it follows the fingers), Page Up and Page Down, or the scrollbar. New output keeps the view at the bottom; scrolled up, the lines being read stay in place until a command is run, which jumps to the bottom.
- The text is JetBrains Mono (the `FontRole::Mono` font: a game that points that role at its own font changes the console too), wrapped to the window in any script the fonts have (`ryo-text`). Over the window the pointer is the default arrow, whatever cursor the game set.

## Ryo.toml

```toml
[debug]
console = false   # the backquote key does not open the console (default: true in a build with the tools)
```

With `console = false` the key is the game's and the console never opens; registered commands are kept but cannot be typed.

## A shipped build

A game built with `ryo2d build` has no developer tools, whatever Ryo.toml says: no console window, no built-in commands, no backquote key (it reaches the game like any other key), and none of the engine's command names in the binary.

The game's code compiles unchanged: `ctx.console().command`, `remove`, `print`, `EngineBuilder::console_command` and `remove_console_command` exist in every build and do nothing without the tools. The handlers are never called. A game's own `CommandSpec` text (its names and descriptions) stays in the binary, so keep nothing secret in it. To leave a command's code out of a shipped build, put it behind `#[cfg(debug_assertions)]` or check `DEV_TOOLS`.

`ryo2d build --dev-tools` keeps the console in a release build, for testing one; it prints `this build includes developer tools: do not ship it`.

## Testing commands

A headless test can type a line: `HeadlessEngine::console_run(line) -> Vec<String>` runs it as if typed and returns the lines it printed after the echo, its error last. It is in every build, so a game's command test compiles in a release test run (`cargo test --release`) too, but there, with no tools, it runs nothing and returns an empty `Vec`: a release test run sees no output. A scene change it asks for happens at once; a quit ends the next step.

```rust,ignore
let mut engine = HeadlessEngine::new(EngineBuilder::new().start_scene(Level { lives: 3 }))?;
engine.step()?; // the start scene entered as the engine was built, registering the command
assert_eq!(engine.console_run("game:lives 5"), ["5 lives"]);
assert_eq!(
  engine.console_run("game:lives many"),
  [r#"argument 1 ("many") is not a number; usage: game:lives <count>"#]
);
```

If the game's tests also run in release, assert on the lines only where the tools are in (`if ryo2d::DEV_TOOLS { ... }`).
