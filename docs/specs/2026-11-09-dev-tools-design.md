# Dev Tools Design

## Overview

**Intent.** The engine's developer tools should work the way a professional engine's do. A shipped game contains none of them: no console, no dev windows, no hidden debug keys, no network code behind a command. A developer's build has all of them without asking. Logging is one system that every part of the engine, every library it uses and the game itself write to; it costs nothing when a message is filtered out, and it leaves a log file a player can send with a bug report. A console command can reach the game world, so a game's own commands can do real work. (Owner, 2026-11-09: go ahead without questions, as for particles.)

**Why now.** This is the third of the six sub-projects left from the engine audit (`docs/research/2026-10-11-engine-audit.md`, "Console and dev tools": "compile dev tools out of release by feature, remove network calls from the console, give handlers a proper context, and integrate with `log`/`tracing`"). Checked against the code on 2026-11-09 (main ad022a3e):

- **Every tool ships.** The console, the logger window, the debug keys and overlays are compiled into every build, and run-time flags decide whether they answer. Only the profiler is behind a cargo feature. F1 to F4 (overlays, collider outlines) work in a shipped game unless the project turns them off. `ryo2d build` warns about three flags and not about the console.
- **A console command blocks the game on the network.** `system:ip:wan` resolves a name with no timeout, then connects and reads with 5 s timeouts, on the main thread, over plain HTTP. Its test makes the same call during `cargo test`.
- **Command handlers get almost nothing.** A handler receives the console, the debug flags and a copy of the time scale. It cannot reach the world, so all three games with commands pass their requests through process-wide atomics, and the engine keeps one-off channels for quit, capture and time scale.
- **The registry has bugs.** `declare` replaces the whole entry, so it removes a handler another scene registered. A name declared with capitals can never be run. A handler's `Error` is never shown. `help` is hand-written and has drifted (two commands are missing, and a removed built-in still shows).
- **`debug:fps`, `debug:stats` and `debug:timing` bypass the engine's own state** (`Settings::show_fps` and the F1 overlay mode), so F1 then jumps from a stale mode.
- **The console re-wraps every line on every frame** while open: up to 2,000 wrap calls and about as many allocations a frame at its 1,000-line cap. (The audit called this quadratic. It is linear, but it runs every frame.)
- **The log is a global lock around `println!`.** Each call formats its message first, then takes the lock, then checks the level, then prints to stdout while holding the lock, in release builds too. `set_min_level` and `set_frame` have no callers, so nothing is ever filtered and every entry is stamped frame 0.
- **The engine installs no `log` logger**, so everything wgpu, naga, winit, bevy_ecs and rapier report is thrown away. There is no log file and no native panic hook. On Windows a shipped game's stdout goes nowhere.
- **The logger window stops after 10,000 entries.** It tails the ring by index and length, and the length stops growing once the ring is full.
- **The profiler window never removes its puffin sink.** Each reopen adds another, and scopes stay on after it closes. Closing either dev window with its X button leaves a stale focus flag, which keeps the game unthrottled in the background.
- **Tests read one process-wide log.** 94 places count warnings in the shared 10,000-entry ring; only the particle tests take a lock. The backlog lists five flake risks from it.
- **About 90 `println!`/`eprintln!` calls** in the engine bypass the log.

**Who it serves.** First, the player, who gets a game without developer keys or tools in it, and whose bug report can carry a log. Second, the game developer (often an agent), who gets a console that can act on the game, one logging system with filters, and tools that are simply there in a development build.

## Goals

- **A shipped game has no dev tools.** A release build contains no console, no dev windows, no debug keys or overlays and no log viewer, whatever Ryo.toml says. The FPS counter stays, because it is a player setting, and so does the fullscreen key.
- **A development build has them all** with no setup. A release build gets them only when asked: `ryo2d build --dev-tools`, or the `dev-tools` cargo feature.
- **Game code is the same in both builds.** `ctx.console()`, the builder's console calls and the logging macros compile and run in a build without tools; the console calls do nothing there.
- **One logging system, on the `log` crate.** The engine, the game and every library write through it. A filtered-out message costs a level check and no formatting. No lock is held while anything is printed or written.
- **Levels per target**, set by an environment variable (`RYO_LOG`, in the style of `RUST_LOG`) and changed live from the console.
- **A log file** in the game's data folder on native builds, development and release alike, with the previous run kept beside it. A panic is written to it before the process ends.
- **Library output is no longer lost.** What wgpu, naga and the rest report through `log` reaches the same outputs, at warning level and above by default.
- **Console commands reach the game.** A scene's command gets its own scene, the scene context that `update` gets, and its arguments (with quoting and typed parsing). It can print, and it can ask for a scene change. It runs as that scene's code and leaves with the scene. A command registered on the builder gets the world. One call registers a command.
- **`help` and completion come from the registry.** Every command, built in or the game's, is listed with its description and usage. Tab completes command names.
- **The console shows the log's warnings and errors**, so a developer sees them without opening anything else.
- **No network code in the console.** The `system:ip:*` commands go.
- **The console is cheap while open.** Lines are wrapped once, not every frame.
- **The dev windows work for a whole session.** The logger window follows the log by sequence number. The profiler window removes its sink and turns scopes off when it closes.
- **Tests read their own log.** A test captures the records its own thread logged, so counts no longer depend on what other tests log.
- **The engine does not print.** Engine library code reports through the log, not `println!`.

## Non-goals

These may become later work:

- `tracing` spans or a `tracing` subscriber (puffin is the profiler; winit's `tracing` output stays unread);
- console variables, key binds, aliases, script files, and history kept between runs;
- a search box in the logger window;
- moving the dev windows into the game window, or onto the web (the logger and profiler windows stay native);
- the profiler on the web;
- a crash reporter, or uploading logs;
- one shared helper for every "warn once" site (the per-system sets stay; a `warn_once!` macro is added for new code);
- the network module's own prints and commands (networking is a deferred sub-project);
- compiling `DebugDraw` and the overlay drawing code out of release (a release build cannot switch them on, which is what matters to a player).

## Anti-goals (the professional bar)

Any of these would be a failure even if everything else works:

- a player reaching a console, a dev window, a debug overlay or a capture key in a shipped game;
- a console command, or any dev tool, stalling the frame on the network or the disk;
- a game needing different code, or a cargo feature of its own, to build with and without tools;
- a log call that formats its message before knowing it is wanted, or prints while holding a lock other log calls wait on;
- a warning from wgpu or another library disappearing without a trace;
- a shipped game crashing and leaving nothing to read;
- the log, the logger window or the profiler degrading over a long session (stopping, leaking, growing without bound);
- a game command needing a process-wide atomic to change the game;
- a test's log assertion passing or failing because of another test.

## Decisions

### The owner's decisions

- Design dev tools as a professional engine would, without questions (2026-11-09).

### My calls (the owner may revisit them in review)

- **Tools follow the build, not a default feature.** They are compiled in when debug assertions are on (every `cargo run`, `ryo2d dev` and test) or the `dev-tools` feature is set, and out otherwise. A default cargo feature could not be switched off by `ryo2d build` without each game's `Cargo.toml` taking part; a dependency's feature can be switched on from the command line (`--features ryo2d/dev-tools`, checked with cargo 1.99), so opting in needs nothing from the game.
- **The `profiler` feature implies `dev-tools`.** `ryo2d dev -p` passes `ryo2d/profiler` itself, so games no longer need a `profiler` feature of their own.
- **No hidden keys in a shipped game.** F1 to F4, F9, F10, F12 and the backquote do nothing in a build without tools and are not taken from the game. F5 (borderless fullscreen) stays under `disable_debug_keys`, as today. The FPS counter stays reachable through `Settings::show_fps`.
- **The console is on by default in a development build.** Today it is off unless `[system] console = true`. Its key moves to `[debug] console`, default true, beside the other dev flags; `[system] console` becomes an unknown key, which the existing warning reports with the new name.
- **`ryo2d build` drops its three tool warnings.** A plain build cannot ship tools. `--dev-tools` prints one warning that the build is not for players.
- **`log`, not `tracing`.** Every library in the tree but winit already reports through `log`, the engine needs levels and targets and not spans, and puffin covers timing.
- **The `engine_*!` and `game_*!` macros go.** The prelude exports `trace!`, `debug!`, `info!`, `warn!` and `error!`, plus `warn_once!` (once per call site). They are the engine's own thin wrappers over `log`'s macros, and they put the game's records under a `game` target root (`game::leap_king::scenes::game`). Engine code calls `log::warn!` and the rest, whose target is the module path.
- **A record's origin is read from its target:** `game` and below is the game, `ryo2d` and the `ryo2d_*` crates are the engine, and anything else is a library. Nothing guesses a game's crate name, and `RYO_LOG=game=debug` switches on the whole game's debug lines. A game that calls `log::info!` directly is treated as a library.
- **Default levels.** Development: `info` for the engine and the game, `warn` for libraries. Release: the same, to the file only. `RYO_LOG` overrides both, in release too.
- **Outputs.** Development: stderr, the file and the in-memory ring the tools read. Release: the file. The web: the browser console, by level (`console.warn` for warnings and so on), with no file. Headless runs and tests write no file.
- **The log file** is `logs/latest.log` under the game's save folder (`Workspace::save_location`), with the previous run's as `logs/previous.log`. It is written through a buffer, flushed on every warning and error, once a second, at exit and on a panic, and it stops at 8 MiB with a last line saying so.
- **A panic hook on native** writes the panic and its place to the log, flushes, then runs the hook that was there before. The web keeps `console_error_panic_hook`.
- **A game that installs its own `log` logger first keeps it.** The engine's outputs are then off, with one line on stderr saying so.
- **One call registers a command.** `declare` then `register` becomes one registration that lasts while its scene is on the stack (or for the whole run, from the builder). The "(inactive)" listing of commands whose scene is not running goes with it. This removes the handler-wiping bug at its root.
- **A scene's command is a closure that receives its own scene by type**, the scene context, and the command (arguments and printing): `|scene: &mut GameScene, ctx, cmd| { scene.reset(ctx); Ok(()) }`. `Scene` gains `Any` as a supertrait for the downcast, which costs implementors nothing (every scene is already `'static`). A world-only closure would leave bullet-hell's reset (a scene switch) and leap-king's respawn (a method on its scene) passing flags again.
- **A command can ask for a scene action** (`Switch`, `Push`, `Pop`, `Quit`), applied when it returns, as `update`'s is.
- **A builder command is a closure over the world and the command.** It lives for the whole run and has no scene.
- **A handler returns `Result<(), String>`.** An error prints in red. Quitting is a request on the command, not a return value.
- **A scene's command runs inside that scene's scope**, as its `update` does, so sounds and particles it starts belong to the scene. Commands run once a frame before the UI pass, as today, and also while the game is paused.
- **A panic inside a game's command is not caught.** A command is game code; it fails as game code does.
- **Engine state that commands need becomes reachable from the context** (debug flags, the overlay mode, capture requests, quit), so no built-in keeps a side channel.
- **Both `system:ip:*` commands go**, the LAN one too. It makes no connection, but address lookups belong to the network module, which can add commands through the public API when it is reworked.
- **New built-ins:** `log:level` (show or set a target's level), `log:show` (the level the console mirrors, or off), `system:scenes` (the scene stack), `system:entities` (a count) and `system:assets` (the registry's stats).
- **The console mirrors warnings and errors** from the log by default.
- **The console's state becomes a world resource**, so a scene or system can print to it (`ctx.console().print`).
- **Records carry a log scope.** A scope is a number a thread holds. A thread the engine spawns (asset workers, the shader compiler, the save writer, tile map jobs, the capture writer, the file watcher) takes the scope of the engine that spawned it. In a game everything is scope 0. This exists for processes that hold many engines, which is every test run.
- **Test capture opens a scope.** `ryo2d::testing::capture_log()` returns a guard that opens a fresh scope on the test's thread and collects every record logged in it: the test's own, and those of the worker threads of an engine built inside it. No test reads the shared ring.
- **The frame number is the logging thread's.** The engine sets it on its own thread each frame; a worker thread's records carry none.
- **A console mirrors its own scope's records**, so in a test one engine's console never shows another's warnings.
- **The dev flags' defaults follow the build's tools.** In any build with tools, the console and the dev window keys default on and `[debug]` in Ryo.toml can turn each off. So `ryo2d build --dev-tools` gives a release build that behaves like a development one. `ryo2d build` embeds only the `[debug]` values the project set.
- **The overlay and outline drawing code stays compiled** in a release build, unreachable: the keys, the commands and the project flags that switch it on are what is removed. `DebugStats` and `FrameCost` keep feeding benches.

## Design

### Builds

One condition, "dev tools", is true when debug assertions are on or the `dev-tools` feature is set. Everything below that is marked a dev tool exists only then.

| Piece | Development build | Release build | Release with `--dev-tools` |
|---|---|---|---|
| Console, its commands and key | yes | no | yes |
| Logger and profiler windows (native) | yes (profiler with its feature) | no | yes |
| F1-F4, F9, F10, F12 | yes | no | yes |
| Debug overlays and outlines | switchable | never on | switchable |
| Log ring (what the tools read) | yes | no | yes |
| Log to stderr | yes | no | yes |
| Log file, panic hook | yes | yes | yes |
| FPS counter (`Settings::show_fps`), F5 | yes | yes | yes |

In a build without tools, `ctx.console()` returns the same type with methods that do nothing, `EngineBuilder`'s console calls do nothing, and a handler passed to them is dropped. The `[debug]` keys in Ryo.toml are read and ignored. `ryo2d build` passes `--features ryo2d/dev-tools` only for `--dev-tools`.

The engine's own checks build the workspace both ways, so the no-tools configuration cannot rot.

### Logging

- **The front.** `log`'s macros. The level check happens in the macro before any formatting.
- **The logger.** The engine installs one `log::Log` for the process the first time an engine is built (or a test asks for capture). It holds the levels: a default and per-target overrides matched by prefix, changed without a restart.
- **A record** carries a sequence number that never repeats, the level, the target, the origin (engine, game, library), the message, the time, its log scope and the frame number of the thread that logged it.
- **Outputs.** Each output takes the formatted record in turn; none holds a lock another output needs. The ring (a dev tool) keeps the last 10,000 records and answers "everything after sequence n".
- **`warn_once!`** logs the first time its call site is reached.
- **Scopes.** A thread's scope is set when the engine spawns it, from the spawning thread's. The cost in a game is one thread-local read per record that passes its level.
- **Capture for tests.** A guard opens a scope on the test's thread and collects that scope's records, with helpers to count by level, by target prefix and by text.

### The console

- **Registering.** A command has a name (lower-cased when registered, so any spelling runs), a one-line description, and optionally a usage line and longer help. A scene registers through `ctx.console()`; the builder registers commands that live for the whole run. A scene registering a name it already registered replaces its own handler, and the scene that leaves takes only its own commands. Each handler is called with the scene that registered it. When two scenes on the stack register one name, the later one answers, and the earlier one's is back when the later scene leaves.
- **Running.** The line is split into words, with double quotes keeping a phrase together. The command a handler receives has the arguments, typed access to an argument (an error names the argument and prints the usage), printing, and requests for a scene action or to quit. A scene's handler also receives its scene and the scene context, called through the scene host as `update` is; a builder's handler receives the world.
- **Built-ins** use the same context. `debug:fps` sets `Settings::show_fps`; `debug:stats` and `debug:timing` set the same overlay mode F1 cycles; `system:timescale` sets the clock and refuses NaN.
- **`help`** lists every command by its prefix, and `help <name>` shows usage and the long text. Tab completes a command name, or lists the matches.
- **Output.** The console is a world resource in a build with tools. It keeps each line's wrapped rows and re-wraps only when a line arrives or the width changes. It takes every mouse button over its window, and its hit tests use the same units as its drawing under a UI scale.

### Dev windows

- **Logger window:** follows the ring by sequence number, shows the frame and the origin, has a button for library records, and scrolls by logical pixels.
- **Profiler window:** removes its puffin sink when it closes, and scopes go off when no window is open and the project has not asked for them. Engine scopes without a `cpu:` or `gpu:` prefix get one.
- **Both:** closing with the X button clears the focus flag, as the key does.

### Errors

- A log file that cannot be opened or written is reported once on stderr; logging to the other outputs carries on.
- An unknown command, a bad argument and a handler's error each print one red line.
- A bad `RYO_LOG` entry is reported once and ignored; the rest applies.

## Public API and migration

Breaking changes are fine (owner's standing rule).

- **Removed:** `engine_*!`, `game_*!` and `dev_log!`; `dev_log`'s functions and types; `ConsoleScope::declare` and `register`, `ConsoleCommandBuilder`, `CommandResult`, the public `ConsoleState` and `CommandRegistry`; the `system:ip:*` commands; `[system] console`.
- **Added:** `trace!` to `error!` and `warn_once!` in the prelude; the new command registration and `CommandContext`; `ctx.console().print`; `ryo2d::testing::capture_log`; the `dev-tools` feature and `ryo2d build --dev-tools`; `[debug] console`; `RYO_LOG`.
- **Users:** the console demo, bullet-hell and leap-king drop their atomics: bullet-hell's reset switches scene from its command, leap-king's respawn calls its scene's method, and the demo's paint command sets a field. The profile-and-log demo and every `game_*!` call move to the new macros. The games' `profiler` features go.
- **Tests:** every test that reads the log moves to capture, and the particle tests' log lock goes.

## Acceptance

### Correctness (tests)

- **Builds.** The workspace checks with and without tools. In a build without tools the console calls compile and do nothing. A release build of a demo, without the feature, does not contain the console's command names; with `--dev-tools` it does.
- **Keys.** In a build without tools, F1 to F4, F9, F10, F12 and the backquote reach the game untouched.
- **Logging.**
  - A message below its target's level is not formatted.
  - A target's level can be raised and lowered live, and a prefix matches its sub-modules.
  - `RYO_LOG` parses levels and `target=level` pairs, and reports a bad entry once.
  - A library's warning reaches the outputs; its info does not by default.
  - Sequence numbers never repeat, and the ring answers "after n" correctly past 10,000 records.
  - The file rotates the previous run, flushes on a warning, and stops at its cap. A panic is in the file.
  - `warn_once!` logs once per call site.
  - Capture collects its own scope's records, a worker thread's included, and none of another test's; two engines in one process stamp their own frame numbers and mirror only their own warnings.
  - A record logged through the prelude's macros has a `game` target; one from the engine has a `ryo2d` target; the default levels treat a third crate as a library.
- **Console.**
  - A scene's command calls a method on its scene, changes the world, and switches scene; it is gone when the scene leaves, with other scenes' commands untouched.
  - A builder's command changes the world and lasts the whole run.
  - Two scenes registering one name: the upper one answers, and the lower one's is back when the upper leaves. A name with capitals runs.
  - Quoted arguments, typed arguments and a bad argument's error line.
  - A handler's error prints in red; a quit request quits.
  - `help` lists every registered command and no removed one; Tab completes.
  - `debug:fps` and F1 agree afterwards, as do `debug:stats` and `debug:timing`.
  - A sound or particle effect started by a scene's command belongs to that scene.
  - Printing 1,000 lines and holding the console open wraps each line once.
  - Every mouse button over the console stays out of the game.
- **Windows.** The logger window shows record 10,001 and after. Opening and closing the profiler window five times leaves no sink and scopes off. Closing either with the X button clears its focus flag.
- **No network.** Nothing in the console module names `std::net`.

### Targets (record machine, quiet, `uptime` recorded)

The plan's first task measures today's figures on the same benches.

- **A filtered-out log call at the default levels:** ≤ 5 ns.
- **A logged warning with every development output on:** recorded, and no slower than today's `engine_warn!`.
- **Dev tools closed, in a build that has them:** ≤ 1 µs a frame for the console and the dev windows together.
- **An open, idle console holding 1,000 lines:** no allocation of its own in a steady frame, and under a quarter of today's cost.
- **Recorded, with no target:** the release binary's size with and without tools.

### Demo

`examples/demos/console` shows the new console: a command that spawns and removes entities in the world, typed and quoted arguments, a scene-only command on one page, the log mirrored into the console, and `log:level`. The profile-and-log demo logs through the new macros at every level and from a library target.

## Documentation

- **`ryo-console`:** rewritten around the new registration, the context and the built-ins.
- **Other skills:** `ryo-profiler` (what is recorded and when, the scopes, no game feature), `ryo-cli` (`--dev-tools`), `ryo-api`, `ryo-game-dev` (logging), `ryo-engine-dev`, `ryo-config-save` (`[debug] console`), each bumped by a minor version, once.
- **Module notes:** the log, the console (it has none today) and the windows; the overview where it names any of them.
- **Records:** `docs/backlog/dev-tools.md` holds the measured figures and limits.

## Risks

- **Gating touches the engine's frame and event code in many places.** The plan gates one tool at a time, each behind the same condition, with the no-tools build checked after each.
- **220 log call sites and 94 test reads move.** They are mechanical, but a warning that tests count must keep its text; the plan moves the helpers first and the call sites in batches by module.
- **`log` is process-wide.** Tests in one process share the logger and its levels, so capture is by scope and tests must not change global levels without restoring them.
- **A missed spawn site** would log into scope 0 and escape a test's capture. The plan lists the sites (asset workers and watcher, shader compiler, save writer, tile map jobs, capture writer), and a test per site proves its warning is captured.
- **Feature unification.** A workspace member that enabled `ryo2d/dev-tools` by default would switch tools on for every release build in the workspace; none may.

## Details settled at planning (2026-11-09, my calls)

The code maps and two small experiments made at planning settled these. The owner may revisit any of them.

- **One condition name.** The engine's build script emits a `dev_tools` cfg when debug assertions are on or the `dev-tools` feature is set (checked: cargo 1.99 tells a build script both). Engine code writes `#[cfg(dev_tools)]`. `ryo2d::DEV_TOOLS` is a public `bool` a game can read.
- **The no-tools build is tested in release.** A small integration test target holds the assertions for a build without tools and runs with `cargo test --release`; `./dev.sh check` also checks the engine and one demo in release.
- **The log lives in `src/core/logging/`**, so `log::` in engine code always means the `log` crate.
- **The prelude's macros install the logger** on first use, so a line a game logs before it builds the engine is kept.
- **Per-target levels** are a default per origin plus overrides matched by the longest prefix. `log`'s global maximum follows the most verbose level in use, so at the default levels a `debug!` costs one atomic read.
- **File flushing is rate-limited.** A warning or error flushes at once unless the file was flushed in the last 50 ms; the engine's per-frame tick flushes what is left. A flood therefore cannot turn into a write per record.
- **A second copy of a game** that cannot rotate or open `latest.log` writes `latest-<pid>.log`; if that fails too, the file output is off with one line on stderr.
- **Built-in commands take crate-internal hooks** to the engine's own state (debug flags, overlay mode, capture, quit) beside the world. Game commands never see them.
- **A handler is taken out of the registry while it runs**, so it can register commands, remove them, or change scenes without a double borrow.
- **Old and new log run side by side while call sites move.** The new logger feeds the old buffer until the last reader has moved; then the old one is deleted.
- **Tests drive the console through a hidden `HeadlessEngine` hook** that runs a line as if typed and returns what it printed.

