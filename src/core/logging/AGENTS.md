# AGENTS.md - Logging Module

The engine's log: the `log` crate's macros in front, one `log::Log` (the engine's logger) behind them, and its outputs. Named `logging` so that `log::` in engine code always means the `log` crate. The design is `docs/specs/2026-11-09-dev-tools-design.md` ("Logging"; where it and the code differ, the code is current). The old log (`core/dev_log.rs`, the `engine_*!`/`game_*!` macros) still runs beside it until its last reader moves; this logger feeds its buffer (the bridge, below).

| File | What |
|------|------|
| `mod.rs` | The logger, `ensure` (installs it), `set_level`/`level`, the thread's frame (`set_frame`), `installed_by_engine`, and the exports (public as `ryo2d::logging`) |
| `levels.rs` | `Levels`: a default per origin, overrides by target prefix, `RYO_LOG`'s parser |
| `record.rs` | `LogRecord`, `Origin` (read from the target) and `under` (whole-segment prefix match) |
| `outputs.rs` | `emit`: the outputs in turn, the sequence counter, the bridge into the old buffer |
| `ring.rs` | The ring (`#[cfg(dev_tools)]`): the last `RING` (10,000) records, read with `after(seq, scope, out)` |
| `scope.rs` | Log scopes: the thread's scope, `in_scope`, `scoped` (a spawned thread's body), `new_scope` |
| `capture.rs` | `capture_log`, `capture_log_bounded`, `LogCapture` (re-exported by `ryo2d::testing`) |
| `file.rs` | The log file (native): `open`, `write`, `tick`, `flush`; `logs/latest.log` in the save folder |
| `panic.rs` | The panic hook: logs `panic at file:line: message` under `ryo2d::panic`, flushes the file, chains to the earlier hook |
| `macros.rs` | `trace!`, `debug!`, `info!`, `warn!`, `error!`, `warn_once!` (`#[macro_export]`, in the prelude) |

## Logging

- **Engine code** logs with `log::trace!` to `log::error!`; the target is the module path. No `println!`/`eprintln!` in engine library code outside `src/cli/`, `src/core/network/` and this module's own stderr output.
- **A game** logs with the prelude's macros, which call `ensure()` and then `log`'s macro with the target `concat!("game::", module_path!())`. So the first line a game logs, before any engine is built, installs the logger and is kept. `warn_once!` keeps one `AtomicBool` per call site and logs the first time the site is reached. A game that calls `log::info!` itself is a library to the log.
- **Origins** by target: `game` or under it is the game; `ryo2d`, under it, or `ryo2d_*` is the engine; anything else (`gamepad` too) is a library.

## Levels

- Defaults: `info` for the game and the engine, `warn` for libraries.
- Overrides are `(prefix, level)`; a target takes the longest prefix it is under, matched by whole path segments (`game` covers `game::menu`, not `gamepad`; `ryo2d` does not cover `ryo2d_procgen`). Else its origin's default.
- `RYO_LOG` is read once, when the logger installs: comma-separated entries, each a level (sets all three defaults) or `target=level`, trimmed, levels case-insensitive (`off`, `error`, `warn`, `info`, `debug`, `trace`). A bad entry is reported once on stderr (`ryo2d: RYO_LOG: ...`) and skipped; the rest applies.
- `log::set_max_level` follows `Levels::most_verbose()` after every change, so at the default levels a `debug!` costs one atomic read in `log`'s macro.
- The logger checks the record's level against its target before formatting; a record that passes is formatted once (`args().as_str()` when it has no arguments) and every output takes that string.

## Installing

`ensure()` installs once per process (a `OnceLock`, safe from any thread; later calls are one atomic read). `Engine::new` (first thing), the prelude's macros and `capture_log` call it. If another `log` logger was set first, it is kept: the engine's outputs stay off, one line says so on stderr, `installed_by_engine()` is false (`set_level` then leaves `log`'s maximum alone), and no log file is opened (it would empty `latest.log` and push the last run's to `previous.log` for nothing). The decision is the pure `outcome`, unit-tested, since a test process can only install once.

## Outputs

`outputs::emit`, in order, after the one format:

1. **The ring** (a build with tools): takes the record's sequence number under its own lock, so the ring is in sequence order, and keeps a copy. Without tools the number comes straight from the counter. Sequence numbers start at 1 and never repeat.
2. **stderr** (native, a build with tools): `LEVEL target: message` with `eprintln!`, so the test harness captures a test thread's lines. A record whose scope has a capture skips it.
3. **The browser console** (wasm, every build): `console.error`, `warn`, `info`, `debug` by level.
4. **A capture**, when the record's scope has one; **else the old buffer** (the bridge): pushed with `dev_log::push_quiet`, never printed, with the old level, the context from the origin (Engine, Game, External), and the module label from a `ryo2d::core::<segment>` target (`Particles`). A captured record skips the bridge, so a test's flood never pushes other tests' lines out of the old buffer.
5. **The log file** (native, a windowed engine only; a headless engine and a test open none): `HH:MM:SS.mmm LEVEL target: message`, the record's own time, every record whatever its scope. `open(folder)` makes `folder/logs/`, moves `latest.log` to `previous.log`, opens `latest.log`, else `latest-<pid>.log`, else returns the error (`open_log_file` says it once on stderr). An 8 KiB buffer; a warning or error flushes at once unless the last flush was under 50 ms ago, `tick` (each frame) flushes the rest, `flush` runs at exit (the event loop's `exiting`, and again when `Engine::run` returns, for lines logged after the loop's last event) and in the panic hook. After 8 MiB one `log full` line is written and no more. Its mutex is its own, held for its write and flush; a thread holding it that panics skips the file in the hook rather than wait. A failed write detaches the file with one stderr line. Tests take the file's static lock, and a test's file takes only its own thread's records.

No output holds a lock while another writes: the ring and each capture have their own mutex, held only for their push; the capture registry is read-locked only to find the capture. Per record, besides the level check and the outputs, the cost is the two thread-local reads (scope, frame).

## Scopes and the frame

- A scope is a number a thread holds (0 in a game). `new_scope` never gives 0 or a number twice. `in_scope(scope, f)` sets this thread's scope for `f` and puts the earlier one back, on a panic too.
- `Engine::new` remembers the scope of the thread that builds it (`Engine::log_scope`), and `Engine::frame` runs inside it.
- Every thread the engine spawns runs its body through `scoped(f)`, which reads the scope on the spawning thread: the asset workers (`asset/workers.rs`, one pool per `AssetServer`, started at its first queued load), the asset folder's debounce thread and notify's callback (`asset/watch.rs`, per server; the callback, on notify's own thread, goes through `in_scope`), the shader compiler (`shaders/compile.rs`, per renderer), the save writer (`saves/writer.rs`, per `SaveService`), the tile map job threads (`tilemap/jobs.rs`, one per landing system and per stream) and the capture writer (`capture/writer.rs`, one per capture). Each is owned by its engine and started from the engine's thread, so it holds that engine's scope; none is shared between engines. `network/` keeps its own threads. A new spawn site must go through `scoped`, or its records land in scope 0 and escape a test's capture.
- The frame is the logging thread's (`set_frame`): `Engine::frame` sets it on its own thread at the start of each frame, to the game clock's frame count once the frame has advanced it (1 for the first). A worker's records carry `None`.

## Capture (tests)

- `capture_log()` calls `ensure()`, opens `new_scope()` on its thread for its lifetime and gives back the earlier scope on drop (nested captures unwind in order). It collects every record logged in its scope: the test thread's and those of threads given its scope. It is `!Send`: it must drop on the thread that opened it.
- `capture_log_bounded(limit)` keeps only the newest `limit` records; `seen()` counts every one. For flood tests.
- Helpers: `records`, `count(level)` (exact level), `warnings` (messages at `Warn`), `containing(needle)`, `from_target(prefix)` (plain `starts_with`), `scope`.
- **Tests never read the ring or stderr.** Every test that logs opens a capture first and asserts on it; a test that changes a level puts it back. The ring's own tests ask for their scope (`after(seq, Some(scope), ..)`) and take turns on a test mutex, so a flood cannot push the other's records out.
