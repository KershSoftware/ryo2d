# AGENTS.md - The developer console

The console window (`mod.rs`, the `Console` world resource) and the commands typed into it. Design: `docs/specs/2026-11-09-dev-tools-design.md` ("The console").

| File | What |
|------|------|
| `mod.rs` | `Console`: the window, input line, history, output (1,000 lines), scrolling and drawing; `log_show`, the lowest log level the console shows (`log:show`, default `warn`) |
| `command.rs` | `CommandSpec` (name, description, usage, help) and `Command` (what a handler gets: arguments, typed `arg`, printing, `act`, `quit`); `split`, the line's words (double quotes keep a phrase) |
| `scope.rs` | `ConsoleScope`, what `ctx.console()` returns: `command`, `remove`, `print`, `print_colored` |
| `registry.rs` | `ConsoleCommands` (dev tools only): per name, a stack of entries; `Handler`; `erase`; `EngineHooks`; `valid_name` |
| `dispatch.rs` | `run_line` (dev tools only): how a line runs |
| `builtins.rs` | The built-in commands (dev tools only) |

## Three kinds of handler

- **Built-in** (`Handler::Builtin`): the world and `EngineHooks`, the engine's own state: its `DebugConfig`, the overlay mode F1 cycles (so `debug:stats` and F1 agree), the frame capture, quit, and the game's name and version. Game code never sees the hooks.
- **Builder** (`Handler::World`, `EngineBuilder::console_command`): the world, for the whole run.
- **Scene** (`Handler::Scene`, `ctx.console().command::<S>`): the scene itself (`&mut S`, downcast from `&mut dyn Scene` by `erase`; a scene of another type is refused with an error) and the `SceneContext` `update` gets. It runs through the scene host as `update` is, inside the scene's scope (`SceneManager::call_scope` calls the slot), so the sounds and particle effects it starts are that scene's.

## A name's stack

Names are lower-cased when added and when looked up. A name has at most one run-long entry (a built-in or a builder command) at the bottom, then the scenes' entries by scope id; scope ids grow with each scene made, so the uppermost scene's entry answers whatever order they registered in, and the one below answers again when it leaves. A scope re-adding its own name replaces its entry, and a second run-long entry replaces the first: the engine adds the built-ins first, then applies the builder's `console_command` and `remove_console_command` calls in the order made, so a builder `help` replaces the built-in. `remove` (the builder's, or `ConsoleScope::remove`) takes every entry of the name. A name that is empty or has whitespace is refused with one warning naming it.

A scene's entries go when its slot leaves the stack (pop, switch, reset, clear): `release_scope` (`scene/manager.rs`) calls `ConsoleCommands::clear_scope`, beside the audio and particle releases.

## How a line runs (`dispatch::run_line`)

1. `Console::handle_input` echoes the submitted line (`> line`, light blue); `console_tick` (`engine/frame.rs`) passes it to `Engine::run_console_line`, which builds the hooks from the engine's fields.
2. The line is split; the first word, lower-cased, is the name. An empty line (or `""`) does nothing; an unknown name prints `Unknown command: <name>. Type 'help'.` in red.
3. `find` clones the spec and the handler (`Arc`s) out, so nothing of the registry is borrowed while the handler runs: a handler may add or remove commands, or pop its own scene.
4. The handler runs (above). A scene entry whose scope has no slot prints an error and its scope's entries are removed.
5. Its printed lines go to the console, then an `Err` text in red. A line the handler prints through `ctx.console().print` goes in at once, so it comes before the command's own lines, which are buffered in `Command` until the handler returns.
6. A scene action it asked for goes through `SceneManager::handle_action` and `sync_pauses`, as `update`'s does: a builder's or a built-in's always, a scene's only from the top scene (a lower scene's is dropped with a red line, as `update` drops one). During a transition only `Quit` applies; a scene change asked for then is dropped with a red line. A stack left empty, or `cmd.quit()`, asks the engine to quit (`console_quit_requested`).

The console runs once a frame in `console_tick`, after `PreUpdate` and before the UI pass, on real time, so commands work while the game is paused. A closed console does no work a frame: `console_tick` and the UI pass only read it, so the resource is neither changed nor taken out of the world (`a_closed_console_does_no_per_frame_work`); its clock and scroll ease stand still until it opens.

`log:level`: `game`, `engine` and `library` name the three origin defaults (`logging::set_default_level`); any other word is a target prefix with an override. `game` is also the root of the game's targets, and `Levels::set("game", ..)` sets the game default too, so there is never an override named `game`. The listing shows a `defaults:` block and an `overrides:` block.

Tests drive it through `HeadlessEngine::console_run` (doc-hidden, dev tools): the line is echoed, run, and the lines printed after the echo are returned (`Console::capture`/`take_captured`).

## A build without the tools

`registry.rs`, `dispatch.rs` and `builtins.rs` are compiled only with `dev_tools`. `ctx.console()`, `ConsoleScope`'s calls, `Command`, `CommandSpec` and the builder's two calls compile in every build and do nothing. The `Console` resource and window still exist there for now (a submitted line runs nothing).

No file here names `std::net` (`the_console_names_no_network_code`).
